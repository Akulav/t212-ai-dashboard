//! Phase 4 — AI insights.
//!
//! Pipeline (run in the background by [`InsightsService::generate`]):
//!
//! ```text
//! holdings ──▶ fundamentals (provider, 24 h cache) ──▶ scoring (deterministic)
//!          ──▶ signals, opportunities, portfolio flags
//!          ──▶ local LLM (optional): per-holding notes + portfolio summary
//!          ──▶ InsightsReport (persisted to insights.json) ──▶ UI / PDF
//! ```
//!
//! The deterministic layer always runs, so insights work without a model;
//! the LLM adds narrative and is clearly labelled as AI-generated.

pub mod ai;
pub mod demo;
pub mod fundamentals;
pub mod scoring;
pub mod sec;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;

use crate::llm::{LlmManager, models};
use crate::paths::AppPaths;
use crate::portfolio::PortfolioView;
use ai::{AiHoldingNote, AiPortfolioNote};
use fundamentals::{Fundamentals, Lookup, Provider};
use scoring::{Scores, Signal};

/// Holdings (largest first) that get an individual AI note.
pub const AI_HOLDINGS: usize = 12;

/// Insights for one holding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldingInsight {
    /// Trading212 ticker.
    pub ticker: String,
    /// Name.
    pub name: String,
    /// Portfolio weight (0..1).
    pub weight: f64,
    /// Market value, account currency.
    pub value: f64,
    /// Unrealised P/L fraction.
    pub pnl_pct: Option<f64>,
    /// Fundamentals, if covered.
    pub fundamentals: Option<Fundamentals>,
    /// Scores.
    pub scores: Scores,
    /// Deterministic signal.
    pub signal: Signal,
    /// Rule-based reasons.
    pub reasons: Vec<String>,
    /// AI commentary, if generated.
    pub ai: Option<AiHoldingNote>,
}

/// A complete insights run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InsightsReport {
    /// When it was generated.
    pub generated_at: DateTime<Utc>,
    /// Account currency.
    pub currency: String,
    /// Total portfolio value.
    pub total_value: f64,
    /// Fundamentals provider used.
    pub provider: String,
    /// Model used for AI text, if any.
    pub model: Option<String>,
    /// Per-holding insights, largest first.
    pub holdings: Vec<HoldingInsight>,
    /// Sector weights, largest first.
    pub sectors: Vec<(String, f64)>,
    /// Portfolio-level observations.
    pub flags: Vec<String>,
    /// Tickers flagged as opportunities, best first.
    pub opportunities: Vec<String>,
    /// Value-weighted overall score.
    pub portfolio_score: Option<f64>,
    /// AI portfolio summary.
    pub ai: Option<AiPortfolioNote>,
    /// Why AI text is missing or partial, if so.
    pub ai_error: Option<String>,
}

/// Progress of a run.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum InsightsPhase {
    /// Nothing running.
    #[default]
    Idle,
    /// Fetching fundamentals.
    Fetching {
        /// Done so far.
        done: usize,
        /// Total.
        total: usize,
    },
    /// Asking the model.
    Analysing {
        /// Done so far.
        done: usize,
        /// Total.
        total: usize,
        /// Current item.
        current: String,
    },
    /// Last run failed.
    Failed(String),
}

/// UI-facing state.
#[derive(Debug, Clone, Default)]
pub struct InsightsState {
    /// Progress.
    pub phase: InsightsPhase,
    /// Latest report.
    pub report: Option<Arc<InsightsReport>>,
}

struct Inner {
    rt: Handle,
    http: Client,
    paths: AppPaths,
    llm: LlmManager,
    provider: Mutex<Provider>,
    state: Mutex<InsightsState>,
    running: AtomicBool,
}

/// Cheaply clonable handle to the insights engine.
#[derive(Clone)]
pub struct InsightsService {
    inner: Arc<Inner>,
}

impl InsightsService {
    /// Creates the service and loads the last saved report.
    pub fn new(
        rt: Handle,
        http: Client,
        paths: AppPaths,
        llm: LlmManager,
        provider: Provider,
    ) -> Self {
        let report = std::fs::read(report_path(&paths))
            .ok()
            .and_then(|b| serde_json::from_slice::<InsightsReport>(&b).ok())
            .map(Arc::new);
        Self {
            inner: Arc::new(Inner {
                rt,
                http,
                paths,
                llm,
                provider: Mutex::new(provider),
                state: Mutex::new(InsightsState {
                    phase: InsightsPhase::Idle,
                    report,
                }),
                running: AtomicBool::new(false),
            }),
        }
    }

    /// Current state.
    pub fn snapshot(&self) -> InsightsState {
        self.inner.state.lock().clone()
    }

    /// Active provider.
    pub fn provider(&self) -> Provider {
        self.inner.provider.lock().clone()
    }

    /// Switches the fundamentals provider.
    pub fn set_provider(&self, provider: Provider) {
        *self.inner.provider.lock() = provider;
    }

    /// `true` while a run is in progress.
    pub fn is_running(&self) -> bool {
        self.inner.running.load(Ordering::SeqCst)
    }

    /// Starts a run in the background. `use_ai` adds LLM commentary when a
    /// model is loaded.
    pub fn generate(&self, view: Arc<PortfolioView>, use_ai: bool) {
        if self.inner.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let this = self.clone();
        self.inner.rt.spawn(async move {
            let result = this.run(&view, use_ai).await;
            let mut st = this.inner.state.lock();
            match result {
                Ok(report) => {
                    let _ = std::fs::write(
                        report_path(&this.inner.paths),
                        serde_json::to_vec_pretty(&report).unwrap_or_default(),
                    );
                    st.report = Some(Arc::new(report));
                    st.phase = InsightsPhase::Idle;
                }
                Err(e) => st.phase = InsightsPhase::Failed(format!("{e:#}")),
            }
            drop(st);
            this.inner.running.store(false, Ordering::SeqCst);
        });
    }

    fn set_phase(&self, phase: InsightsPhase) {
        self.inner.state.lock().phase = phase;
    }

    async fn run(&self, view: &PortfolioView, use_ai: bool) -> anyhow::Result<InsightsReport> {
        let provider = self.provider();
        let rows = &view.analytics.holdings;
        let cache_dir = self.inner.paths.root.join("fundamentals");

        // 1. Fundamentals.
        let mut fundamentals = Vec::with_capacity(rows.len());
        for (i, r) in rows.iter().enumerate() {
            self.set_phase(InsightsPhase::Fetching {
                done: i,
                total: rows.len(),
            });
            let lookup = Lookup {
                ticker: r.ticker.clone(),
                isin: r.isin.clone(),
                name: r.name.clone(),
                currency: r.quote_currency.clone(),
                price: Some(r.current_price),
            };
            let f =
                match fundamentals::fetch_cached(&provider, &self.inner.http, &cache_dir, &lookup)
                    .await
                {
                    Ok(f) => f,
                    Err(e) => {
                        tracing::warn!(ticker = %r.ticker, "fundamentals unavailable: {e:#}");
                        None
                    }
                };
            fundamentals.push(f);
        }

        // 2. Deterministic analysis.
        let mut report = build_report(view, fundamentals, provider.label());

        // 3. Optional AI narrative.
        if use_ai {
            match self.inner.llm.endpoint() {
                None => {
                    report.ai_error =
                        Some("No model loaded — load one in AI Engine for written analysis.".into())
                }
                Some(_) => self.add_ai(&mut report).await,
            }
        }
        Ok(report)
    }

    async fn add_ai(&self, report: &mut InsightsReport) {
        report.model = self
            .inner
            .llm
            .loaded_model_id()
            .and_then(|id| models::find(&id))
            .map(|m| format!("{} {}", m.name, m.quant));
        let n = report.holdings.len().min(AI_HOLDINGS);
        let total = n + 1;
        let mut failures = 0;
        for i in 0..n {
            let h = &report.holdings[i];
            self.set_phase(InsightsPhase::Analysing {
                done: i,
                total,
                current: h.name.clone(),
            });
            let prompt = ai::holding_prompt(h, &report.currency);
            match self
                .inner
                .llm
                .chat_json(prompt, 450)
                .await
                .and_then(|raw| ai::parse::<AiHoldingNote>(&raw))
            {
                Ok(mut note) => {
                    note.verdict = ai::normalise_verdict(&note.verdict);
                    note.strengths.truncate(3);
                    note.risks.truncate(3);
                    report.holdings[i].ai = Some(note);
                }
                Err(e) => {
                    failures += 1;
                    tracing::warn!(ticker = %h.ticker, "AI note failed: {e:#}");
                }
            }
        }
        self.set_phase(InsightsPhase::Analysing {
            done: n,
            total,
            current: "Portfolio summary".into(),
        });
        match self
            .inner
            .llm
            .chat_json(ai::portfolio_prompt(report), 900)
            .await
            .and_then(|raw| ai::parse::<AiPortfolioNote>(&raw))
        {
            Ok(note) => report.ai = Some(note),
            Err(e) => report.ai_error = Some(format!("Portfolio summary failed: {e:#}")),
        }
        if failures > 0 && report.ai_error.is_none() {
            report.ai_error = Some(format!(
                "{failures} holding note(s) could not be generated."
            ));
        }
    }
}

fn report_path(paths: &AppPaths) -> std::path::PathBuf {
    paths.root.join("insights.json")
}

/// Builds the deterministic part of a report (pure; unit-tested).
pub fn build_report(
    view: &PortfolioView,
    fundamentals: Vec<Option<Fundamentals>>,
    provider: &str,
) -> InsightsReport {
    let rows = &view.analytics.holdings;
    let mut holdings: Vec<HoldingInsight> = rows
        .iter()
        .zip(fundamentals)
        .map(|(r, f)| {
            let scores = f.as_ref().map(scoring::score).unwrap_or_default();
            let (signal, reasons) = scoring::assess(r, f.as_ref(), &scores);
            HoldingInsight {
                ticker: r.ticker.clone(),
                name: r.name.clone(),
                weight: r.weight,
                value: r.value,
                pnl_pct: r.pnl_pct,
                fundamentals: f,
                scores,
                signal,
                reasons,
                ai: None,
            }
        })
        .collect();
    holdings.sort_by(|a, b| b.value.total_cmp(&a.value));

    let mut sector_map: BTreeMap<String, f64> = BTreeMap::new();
    for h in &holdings {
        let sector = h
            .fundamentals
            .as_ref()
            .and_then(|f| f.sector.clone())
            .unwrap_or_else(|| "Unclassified".into());
        *sector_map.entry(sector).or_default() += h.weight;
    }
    let mut sectors: Vec<(String, f64)> = sector_map.into_iter().collect();
    sectors.sort_by(|a, b| b.1.total_cmp(&a.1));

    let mut opps: Vec<&HoldingInsight> = holdings
        .iter()
        .filter(|h| h.signal == Signal::Opportunity)
        .collect();
    opps.sort_by(|a, b| {
        b.scores
            .overall
            .unwrap_or(0.0)
            .total_cmp(&a.scores.overall.unwrap_or(0.0))
    });
    let opportunities = opps.iter().map(|h| h.ticker.clone()).collect();

    let (wsum, wscore) = holdings
        .iter()
        .filter_map(|h| h.scores.overall.map(|s| (h.weight, h.weight * s)))
        .fold((0.0, 0.0), |(a, b), (w, s)| (a + w, b + s));

    InsightsReport {
        generated_at: Utc::now(),
        currency: view.summary.currency.clone(),
        total_value: view.summary.total_value,
        provider: provider.into(),
        model: None,
        flags: scoring::portfolio_flags(rows, &sectors),
        holdings,
        sectors,
        opportunities,
        portfolio_score: (wsum > 0.0).then(|| wscore / wsum),
        ai: None,
        ai_error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::demo;
    use crate::portfolio::store::PortfolioCache;

    fn demo_view() -> PortfolioView {
        let now = Utc::now();
        let c: PortfolioCache = demo::cache(now);
        let summary = c.summary.clone().unwrap();
        let analytics = crate::portfolio::analytics::compute(
            &summary,
            &c.positions,
            &c.dividends,
            &c.transactions,
            now,
        );
        PortfolioView {
            summary,
            positions: c.positions.clone(),
            analytics,
            snapshots: vec![],
            dividend_count: 0,
            recent_dividends: vec![],
            synced_at: None,
            history_synced_at: None,
        }
    }

    #[test]
    fn demo_portfolio_produces_full_report() {
        let view = demo_view();
        let f = view
            .analytics
            .holdings
            .iter()
            .map(|r| {
                super::demo::fundamentals(&Lookup {
                    ticker: r.ticker.clone(),
                    isin: r.isin.clone(),
                    name: r.name.clone(),
                    currency: r.quote_currency.clone(),
                    price: Some(r.current_price),
                })
            })
            .collect();
        let r = build_report(&view, f, "Demo data");
        assert_eq!(r.holdings.len(), view.analytics.holdings.len());
        assert!(r.holdings.iter().all(|h| h.signal != Signal::NoData));
        assert!(r.portfolio_score.is_some());
        assert!(
            !r.opportunities.is_empty(),
            "demo data should surface at least one opportunity"
        );
        assert!(r.sectors.iter().any(|(s, _)| s == "Technology"));
        let w: f64 = r.sectors.iter().map(|s| s.1).sum();
        assert!((w - 1.0).abs() < 1e-6);
    }
}
