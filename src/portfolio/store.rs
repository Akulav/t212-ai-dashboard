//! Local portfolio cache: `portfolio-<env>.json` in the data directory.
//!
//! Caching gives three things:
//! 1. **Instant startup / offline view**: the last sync is shown immediately.
//! 2. **Incremental history sync**: only pages newer than the newest cached
//!    item are fetched, which matters with a 20 req/min history limit.
//! 3. **Portfolio history**: the API has no value-over-time endpoint, so a
//!    [`Snapshot`] of account value is recorded on every sync.
//!
//! The file contains financial data (no credentials). It lives in the user's
//! data folder, which `.gitignore` excludes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::config::T212Environment;
use crate::paths::AppPaths;
use crate::t212::models::{AccountSummary, Dividend, Position, Transaction};

/// Bump when the format changes incompatibly; old caches are discarded.
const CACHE_VERSION: u32 = 1;
/// Snapshots closer together than this replace each other.
const SNAPSHOT_SPACING_MINUTES: i64 = 30;
/// Upper bound on stored snapshots (~1 year at one per 30 min).
const MAX_SNAPSHOTS: usize = 20_000;

/// Point-in-time account value.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// When it was taken.
    pub at: DateTime<Utc>,
    /// Account total value.
    pub total_value: f64,
    /// Cost basis of open positions.
    pub invested: f64,
    /// Free cash.
    pub cash: f64,
}

/// Everything persisted between sessions for one environment.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PortfolioCache {
    /// Format version.
    pub version: u32,
    /// Account the cache belongs to (reset if a different account syncs).
    pub account_id: Option<i64>,
    /// Last account summary.
    pub summary: Option<AccountSummary>,
    /// `true` if [`Self::summary`] was rebuilt from positions because the
    /// API key may not read the account summary (no cash / realised P/L).
    pub summary_partial: bool,
    /// Last positions.
    pub positions: Vec<Position>,
    /// All known dividends, newest first.
    pub dividends: Vec<Dividend>,
    /// All known cash transactions, newest first.
    pub transactions: Vec<Transaction>,
    /// Value history.
    pub snapshots: Vec<Snapshot>,
    /// Last summary/positions sync.
    pub synced_at: Option<DateTime<Utc>>,
    /// Last history (dividends/transactions) sync.
    pub history_synced_at: Option<DateTime<Utc>>,
}

/// Cache file for `env`.
pub fn cache_path(paths: &AppPaths, env: T212Environment) -> PathBuf {
    paths
        .root
        .join(format!("portfolio-{}.json", env.label().to_lowercase()))
}

impl PortfolioCache {
    /// Loads the cache, returning an empty one if missing, corrupt or outdated.
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Self>(&b).ok())
            .filter(|c| c.version == CACHE_VERSION)
            .unwrap_or_else(|| Self {
                version: CACHE_VERSION,
                ..Self::default()
            })
    }

    /// Atomically writes the cache.
    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(self)?)
            .with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// References of cached dividends (for incremental sync).
    pub fn dividend_refs(&self) -> HashSet<String> {
        self.dividends.iter().map(dividend_key).collect()
    }

    /// References of cached transactions (for incremental sync).
    pub fn transaction_refs(&self) -> HashSet<String> {
        self.transactions.iter().map(transaction_key).collect()
    }

    /// Merges newly fetched dividends, de-duplicating and sorting newest first.
    pub fn merge_dividends(&mut self, new: Vec<Dividend>) {
        let mut seen = HashSet::new();
        let mut all: Vec<Dividend> = new
            .into_iter()
            .chain(self.dividends.drain(..))
            .filter(|d| seen.insert(dividend_key(d)))
            .collect();
        all.sort_by_key(|d| std::cmp::Reverse(d.paid_on));
        self.dividends = all;
    }

    /// Merges newly fetched transactions, de-duplicating and sorting newest first.
    pub fn merge_transactions(&mut self, new: Vec<Transaction>) {
        let mut seen = HashSet::new();
        let mut all: Vec<Transaction> = new
            .into_iter()
            .chain(self.transactions.drain(..))
            .filter(|t| seen.insert(transaction_key(t)))
            .collect();
        all.sort_by_key(|t| std::cmp::Reverse(t.date_time));
        self.transactions = all;
    }

    /// Records a value snapshot, coalescing snapshots taken close together.
    pub fn record_snapshot(&mut self, summary: &AccountSummary, at: DateTime<Utc>) {
        let snap = Snapshot {
            at,
            total_value: summary.total_value,
            invested: summary.investments.total_cost,
            cash: summary.cash.available_to_trade,
        };
        match self.snapshots.last_mut() {
            Some(last) if at - last.at < Duration::minutes(SNAPSHOT_SPACING_MINUTES) => {
                *last = snap
            }
            _ => self.snapshots.push(snap),
        }
        if self.snapshots.len() > MAX_SNAPSHOTS {
            let excess = self.snapshots.len() - MAX_SNAPSHOTS;
            self.snapshots.drain(..excess);
        }
    }
}

/// Stable identity of a dividend.
pub fn dividend_key(d: &Dividend) -> String {
    if d.reference.is_empty() {
        format!("{}|{:?}|{}", d.ticker, d.paid_on, d.amount)
    } else {
        d.reference.clone()
    }
}

/// Stable identity of a transaction.
pub fn transaction_key(t: &Transaction) -> String {
    if t.reference.is_empty() {
        format!("{}|{:?}|{}", t.kind, t.date_time, t.amount)
    } else {
        t.reference.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn div(r: &str, day: u32) -> Dividend {
        Dividend {
            reference: r.into(),
            paid_on: Some(Utc.with_ymd_and_hms(2026, 1, day, 0, 0, 0).unwrap()),
            ..Default::default()
        }
    }

    #[test]
    fn merge_dedupes_and_sorts() {
        let mut c = PortfolioCache {
            dividends: vec![div("a", 1), div("b", 2)],
            ..Default::default()
        };
        c.merge_dividends(vec![div("c", 3), div("b", 2)]);
        let refs: Vec<_> = c.dividends.iter().map(|d| d.reference.as_str()).collect();
        assert_eq!(refs, ["c", "b", "a"]);
    }

    #[test]
    fn snapshots_coalesce() {
        let mut c = PortfolioCache::default();
        let s = AccountSummary {
            total_value: 1.0,
            ..Default::default()
        };
        let t0 = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        c.record_snapshot(&s, t0);
        c.record_snapshot(&s, t0 + Duration::minutes(10));
        c.record_snapshot(&s, t0 + Duration::minutes(45));
        assert_eq!(c.snapshots.len(), 2);
    }
}
