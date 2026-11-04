//! Portfolio sync service: pulls data from Trading212, merges it into the
//! local cache, and publishes precomputed [`analytics::Analytics`] for the UI.
//!
//! Sync cadence (driven by [`PortfolioService::tick`] from the UI loop):
//!
//! | What                         | How often | Endpoint limit |
//! |------------------------------|-----------|----------------|
//! | Account summary + positions  | 60 s      | 1/5 s, 1/1 s   |
//! | Dividends + transactions     | 15 min, incremental | 20/min |
//!
//! Sync always uses a **read-only** client: it never needs write access.

pub mod analytics;
pub mod store;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use reqwest::Client;
use tokio::runtime::Handle;

use crate::config::{T212Environment, TradingMode};
use crate::paths::AppPaths;
use crate::secrets;
use crate::t212::models::{AccountSummary, Position};
use crate::t212::{T212Client, T212Error};
use analytics::Analytics;
use store::{PortfolioCache, Snapshot};

const QUICK_INTERVAL: Duration = Duration::from_secs(60);
const HISTORY_INTERVAL: Duration = Duration::from_secs(15 * 60);
/// Page cap for the very first history sync (50 items/page).
const FIRST_SYNC_MAX_PAGES: usize = 400;
/// Page cap for incremental syncs.
const INCREMENTAL_MAX_PAGES: usize = 20;

/// What the sync engine is doing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SyncStatus {
    /// Idle.
    #[default]
    Idle,
    /// Working; the string names the current step.
    Syncing(String),
    /// Last sync failed (data shown is from the cache).
    Failed(String),
}

/// Immutable view of the portfolio rendered by the UI.
#[derive(Debug, Clone)]
pub struct PortfolioView {
    /// Account summary.
    pub summary: AccountSummary,
    /// Open positions.
    pub positions: Vec<Position>,
    /// Derived analytics.
    pub analytics: Analytics,
    /// Value history.
    pub snapshots: Vec<Snapshot>,
    /// Number of cached dividend payments.
    pub dividend_count: usize,
    /// Most recent dividends (newest first, up to 200) for tables.
    pub recent_dividends: Vec<crate::t212::models::Dividend>,
    /// Last quick sync.
    pub synced_at: Option<DateTime<Utc>>,
    /// Last history sync.
    pub history_synced_at: Option<DateTime<Utc>>,
}

/// State polled by the UI each frame.
#[derive(Debug, Clone, Default)]
pub struct PortfolioState {
    /// Sync status.
    pub status: SyncStatus,
    /// Latest data, if any has ever been synced.
    pub view: Option<Arc<PortfolioView>>,
    /// Whether credentials exist for the active environment.
    pub has_credentials: bool,
    /// Active environment.
    pub environment: T212Environment,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SyncKind {
    Quick,
    Full,
}

struct Inner {
    rt: Handle,
    http: Client,
    paths: AppPaths,
    state: Mutex<PortfolioState>,
    cache: Mutex<PortfolioCache>,
    running: AtomicBool,
    last_quick: Mutex<Option<Instant>>,
    last_full: Mutex<Option<Instant>>,
}

/// Cheaply clonable handle to the sync service.
#[derive(Clone)]
pub struct PortfolioService {
    inner: Arc<Inner>,
}

impl PortfolioService {
    /// Loads the cache for `env` and publishes it immediately.
    pub fn new(rt: Handle, http: Client, paths: AppPaths, env: T212Environment) -> Self {
        let service = Self {
            inner: Arc::new(Inner {
                rt,
                http,
                paths,
                state: Mutex::new(PortfolioState::default()),
                cache: Mutex::new(PortfolioCache::default()),
                running: AtomicBool::new(false),
                last_quick: Mutex::new(None),
                last_full: Mutex::new(None),
            }),
        };
        service.set_environment(env);
        service
    }

    /// Current state (cheap: the view is behind an `Arc`).
    pub fn snapshot(&self) -> PortfolioState {
        self.inner.state.lock().clone()
    }

    /// Switches environment: loads that environment's cache and resets timers.
    pub fn set_environment(&self, env: T212Environment) {
        let cache = PortfolioCache::load(&store::cache_path(&self.inner.paths, env));
        let view = build_view(&cache);
        *self.inner.cache.lock() = cache;
        {
            let mut st = self.inner.state.lock();
            st.environment = env;
            st.view = view;
            st.status = SyncStatus::Idle;
            st.has_credentials = secrets::has_credentials(env);
        }
        *self.inner.last_quick.lock() = None;
        *self.inner.last_full.lock() = None;
    }

    /// Call after credentials are saved or removed.
    pub fn credentials_changed(&self) {
        let env = self.inner.state.lock().environment;
        let has = secrets::has_credentials(env);
        self.inner.state.lock().has_credentials = has;
        *self.inner.last_quick.lock() = None;
        *self.inner.last_full.lock() = None;
        if !has {
            self.inner.state.lock().status = SyncStatus::Idle;
        }
    }

    /// Deletes the local cache for the active environment.
    pub fn clear_cache(&self) {
        let env = self.inner.state.lock().environment;
        let _ = std::fs::remove_file(store::cache_path(&self.inner.paths, env));
        self.set_environment(env);
    }

    /// Schedules syncs when due. Cheap; call every frame.
    pub fn tick(&self) {
        if !self.inner.state.lock().has_credentials || self.inner.running.load(Ordering::SeqCst) {
            return;
        }
        let due = |slot: &Mutex<Option<Instant>>, every: Duration| {
            slot.lock().is_none_or(|t| t.elapsed() >= every)
        };
        if due(&self.inner.last_full, HISTORY_INTERVAL) {
            self.start(SyncKind::Full);
        } else if due(&self.inner.last_quick, QUICK_INTERVAL) {
            self.start(SyncKind::Quick);
        }
    }

    /// Forces a full sync now (the Refresh button).
    pub fn refresh_now(&self) {
        if !self.inner.running.load(Ordering::SeqCst) {
            self.start(SyncKind::Full);
        }
    }

    /// `true` while a sync is running.
    pub fn is_syncing(&self) -> bool {
        self.inner.running.load(Ordering::SeqCst)
    }

    fn start(&self, kind: SyncKind) {
        if self.inner.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let now = Some(Instant::now());
        *self.inner.last_quick.lock() = now;
        if kind == SyncKind::Full {
            *self.inner.last_full.lock() = now;
        }
        let this = self.clone();
        self.inner.rt.spawn(async move {
            let env = this.inner.state.lock().environment;
            let result = this.sync(env, kind).await;
            let mut st = this.inner.state.lock();
            if st.environment == env {
                st.status = match result {
                    Ok(()) => SyncStatus::Idle,
                    Err(e) => {
                        tracing::warn!("portfolio sync failed: {e:#}");
                        SyncStatus::Failed(format!("{e:#}"))
                    }
                };
            }
            drop(st);
            this.inner.running.store(false, Ordering::SeqCst);
        });
    }

    fn set_status(&self, step: &str) {
        self.inner.state.lock().status = SyncStatus::Syncing(step.to_string());
    }

    async fn sync(&self, env: T212Environment, kind: SyncKind) -> anyhow::Result<()> {
        let creds = tokio::task::spawn_blocking(move || secrets::load(env))
            .await??
            .ok_or_else(|| anyhow::anyhow!("no Trading212 credentials stored"))?;
        // Sync never needs write access.
        let client = T212Client::new(self.inner.http.clone(), env, creds, TradingMode::ReadOnly);

        self.set_status("Account summary");
        let summary = client.account_summary().await.map_err(map_err)?;
        self.set_status("Positions");
        let positions = client.positions().await.map_err(map_err)?;

        let now = Utc::now();
        {
            let mut cache = self.inner.cache.lock();
            if cache.account_id.is_some_and(|id| id != summary.id) {
                tracing::info!("different account detected; resetting cache");
                *cache = PortfolioCache {
                    version: cache.version,
                    ..Default::default()
                };
            }
            cache.account_id = Some(summary.id);
            cache.record_snapshot(&summary, now);
            cache.summary = Some(summary);
            cache.positions = positions;
            cache.synced_at = Some(now);
        }
        self.publish(env);

        if kind == SyncKind::Full {
            let (div_refs, tx_refs, first) = {
                let c = self.inner.cache.lock();
                (
                    c.dividend_refs(),
                    c.transaction_refs(),
                    c.history_synced_at.is_none(),
                )
            };
            let max_pages = if first {
                FIRST_SYNC_MAX_PAGES
            } else {
                INCREMENTAL_MAX_PAGES
            };

            self.set_status(if first {
                "Dividend history (first sync can take a few minutes)"
            } else {
                "Dividends"
            });
            let dividends = client
                .dividends(max_pages, |d| div_refs.contains(&store::dividend_key(d)))
                .await
                .map_err(map_err)?;
            self.set_status("Deposits & withdrawals");
            let transactions = client
                .transactions(max_pages, |t| tx_refs.contains(&store::transaction_key(t)))
                .await
                .map_err(map_err)?;
            tracing::info!(
                new_dividends = dividends.len(),
                new_transactions = transactions.len(),
                "history synced"
            );

            let mut cache = self.inner.cache.lock();
            cache.merge_dividends(dividends);
            cache.merge_transactions(transactions);
            cache.history_synced_at = Some(Utc::now());
        }

        let cache = self.inner.cache.lock().clone();
        let path = store::cache_path(&self.inner.paths, env);
        tokio::task::spawn_blocking(move || cache.save(&path)).await??;
        self.publish(env);
        Ok(())
    }

    fn publish(&self, env: T212Environment) {
        let view = build_view(&self.inner.cache.lock());
        let mut st = self.inner.state.lock();
        if st.environment == env {
            st.view = view;
        }
    }
}

fn map_err(e: T212Error) -> anyhow::Error {
    anyhow::Error::new(e)
}

fn build_view(cache: &PortfolioCache) -> Option<Arc<PortfolioView>> {
    let summary = cache.summary.clone()?;
    let analytics = analytics::compute(
        &summary,
        &cache.positions,
        &cache.dividends,
        &cache.transactions,
        Utc::now(),
    );
    Some(Arc::new(PortfolioView {
        summary,
        positions: cache.positions.clone(),
        analytics,
        snapshots: cache.snapshots.clone(),
        dividend_count: cache.dividends.len(),
        recent_dividends: cache.dividends.iter().take(200).cloned().collect(),
        synced_at: cache.synced_at,
        history_synced_at: cache.history_synced_at,
    }))
}
