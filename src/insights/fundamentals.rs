//! Company fundamentals (valuation, growth, quality, income) and the
//! providers that supply them.
//!
//! Trading212's API has no fundamentals, so they come from a pluggable
//! [`Provider`]. Values are in the instrument's quote currency; ratios are
//! fractions (`0.12` = 12 %). Every field is optional: ETFs and some
//! listings simply have fewer metrics, and the scoring copes with gaps.
//!
//! Results are cached per symbol on disk for [`CACHE_TTL_HOURS`] to keep
//! request volumes low.

use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// How long cached fundamentals stay fresh.
pub const CACHE_TTL_HOURS: i64 = 24;

/// One reported quarter.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Quarter {
    /// Label, e.g. `Q3 2026`.
    pub label: String,
    /// Revenue.
    pub revenue: Option<f64>,
    /// Net income.
    pub net_income: Option<f64>,
    /// Diluted EPS.
    pub eps: Option<f64>,
}

/// Fundamentals for one instrument.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fundamentals {
    /// Provider symbol (e.g. `AAPL`, `VUSA.L`).
    pub symbol: String,
    /// `"stock"` or `"etf"`.
    pub kind: String,
    /// Sector, if known.
    pub sector: Option<String>,
    /// Industry, if known.
    pub industry: Option<String>,
    /// Market capitalisation.
    pub market_cap: Option<f64>,
    /// Trailing twelve-month P/E.
    pub pe_ttm: Option<f64>,
    /// Forward P/E.
    pub pe_forward: Option<f64>,
    /// PEG ratio.
    pub peg: Option<f64>,
    /// Price / book.
    pub price_to_book: Option<f64>,
    /// Revenue growth, year over year.
    pub revenue_growth: Option<f64>,
    /// Earnings growth, year over year.
    pub earnings_growth: Option<f64>,
    /// Net profit margin.
    pub profit_margin: Option<f64>,
    /// Operating margin.
    pub operating_margin: Option<f64>,
    /// Return on equity.
    pub roe: Option<f64>,
    /// Debt / equity (as a ratio, `1.0` = 100 %).
    pub debt_to_equity: Option<f64>,
    /// Trailing free cash flow.
    pub free_cash_flow: Option<f64>,
    /// Free-cash-flow yield (FCF / market cap).
    pub fcf_yield: Option<f64>,
    /// Dividend yield.
    pub dividend_yield: Option<f64>,
    /// Payout ratio.
    pub payout_ratio: Option<f64>,
    /// ETF ongoing charge (TER).
    pub expense_ratio: Option<f64>,
    /// Consensus analyst price target.
    pub analyst_target: Option<f64>,
    /// Latest quarters, oldest first.
    pub quarters: Vec<Quarter>,
    /// When the data was fetched.
    pub fetched_at: Option<DateTime<Utc>>,
    /// Provider name for attribution.
    pub source: String,
}

impl Fundamentals {
    /// `true` for exchange-traded funds.
    pub fn is_etf(&self) -> bool {
        self.kind == "etf"
    }

    /// `true` if fetched within [`CACHE_TTL_HOURS`].
    pub fn is_fresh(&self, now: DateTime<Utc>) -> bool {
        self.fetched_at
            .is_some_and(|t| now - t < Duration::hours(CACHE_TTL_HOURS))
    }
}

/// What to look up: Trading212 identifiers of one holding.
#[derive(Debug, Clone)]
pub struct Lookup {
    /// Trading212 ticker, e.g. `AAPL_US_EQ`.
    pub ticker: String,
    /// ISIN (most reliable cross-provider identifier).
    pub isin: String,
    /// Instrument name.
    pub name: String,
    /// Quote currency.
    pub currency: String,
    /// Latest price in the quote currency (from Trading212).
    pub price: Option<f64>,
}

/// Where fundamentals come from.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Provider {
    /// No source configured: insights use portfolio data only.
    #[default]
    None,
    /// Built-in synthetic data (demo mode only).
    Demo,
    /// SEC EDGAR (US listings). SEC requires a contact email per request.
    SecEdgar {
        /// Contact email sent in the User-Agent, as SEC's policy requires.
        contact_email: String,
    },
}

impl Provider {
    /// Human-readable name.
    pub fn label(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Demo => "Demo data",
            Self::SecEdgar { .. } => "SEC EDGAR",
        }
    }

    /// Fetches fundamentals for one instrument. `Ok(None)` = not covered.
    pub async fn fetch(
        &self,
        http: &Client,
        cache_dir: &Path,
        lookup: &Lookup,
    ) -> Result<Option<Fundamentals>> {
        match self {
            Self::None => Ok(None),
            Self::Demo => Ok(super::demo::fundamentals(lookup)),
            Self::SecEdgar { contact_email } => {
                super::sec::fetch(http, contact_email, cache_dir, lookup).await
            }
        }
    }
}

fn cache_file(dir: &Path, ticker: &str) -> PathBuf {
    let safe: String = ticker
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    dir.join(format!("{safe}.json"))
}

/// Cached fetch: returns fresh cache hits, otherwise asks the provider and
/// stores the result. Stale cache is returned if the provider fails.
pub async fn fetch_cached(
    provider: &Provider,
    http: &Client,
    cache_dir: &Path,
    lookup: &Lookup,
) -> Result<Option<Fundamentals>> {
    let path = cache_file(cache_dir, &lookup.ticker);
    let cached: Option<Fundamentals> = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let now = Utc::now();
    if let Some(c) = &cached
        && c.is_fresh(now)
        && *provider != Provider::Demo
    {
        return Ok(Some(c.clone()));
    }
    match provider.fetch(http, cache_dir, lookup).await {
        Ok(Some(mut f)) => {
            f.fetched_at.get_or_insert(now);
            if *provider != Provider::Demo {
                let _ = std::fs::create_dir_all(cache_dir);
                let _ = std::fs::write(&path, serde_json::to_vec(&f)?);
            }
            Ok(Some(f))
        }
        Ok(None) => Ok(cached),
        Err(e) if cached.is_some() => {
            tracing::warn!(ticker = %lookup.ticker, "fundamentals refresh failed, using cache: {e:#}");
            Ok(cached)
        }
        Err(e) => Err(e),
    }
}
