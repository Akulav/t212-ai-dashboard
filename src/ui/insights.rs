//! AI Insights page (phase 4): generate fundamentals-based insights with an
//! optional local-LLM narrative, and export a branded PDF report.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, Align, Color32, Layout, RichText, Ui};
use egui_phosphor::regular as ph;
use parking_lot::Mutex;

use super::dashboard::short_ticker;
use super::holdings::right;
use super::theme::{palette::*, semibold};
use super::widgets::{self, GAP, Kpi, banner, card, card_header, card_row, chip, kpi_row};
use super::{Services, format};
use crate::insights::scoring::Signal;
use crate::insights::{HoldingInsight, InsightsPhase, InsightsReport};
use crate::portfolio::PortfolioState;

/// Export progress.
#[derive(Debug, Clone, Default)]
enum Export {
    #[default]
    Idle,
    Running,
    Done(PathBuf),
    Failed(String),
}

/// Persistent page state.
pub struct InsightsPage {
    use_ai: bool,
    export: Arc<Mutex<Export>>,
}

impl Default for InsightsPage {
    fn default() -> Self {
        Self {
            use_ai: true,
            export: Arc::new(Mutex::new(Export::Idle)),
        }
    }
}

fn signal_color(s: Signal) -> Color32 {
    match s {
        Signal::Opportunity => POSITIVE,
        Signal::Review => NEGATIVE,
        Signal::Hold => ACCENT,
        Signal::NoData => MUTED,
    }
}

fn verdict_color(v: &str) -> Color32 {
    match v {
        "Add" => POSITIVE,
        "Trim" => NEGATIVE,
        "Watch" => WARNING,
        _ => ACCENT,
    }
}

impl InsightsPage {
    /// `true` while an export is running (for repaint pacing).
    pub fn is_busy(&self) -> bool {
        matches!(*self.export.lock(), Export::Running)
    }

    /// Renders the page.
    pub fn show(&mut self, ui: &mut Ui, svc: &Services, portfolio: &PortfolioState) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                widgets::page_title(
                    ui,
                    "AI Insights",
                    "Fundamentals, opportunities and AI analysis of your holdings",
                )
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                chip(
                    ui,
                    &format!("{}  Powered by AI · Made by Akulav", ph::SPARKLE),
                    VIOLET,
                );
            });
        });
        ui.add_space(20.0);

        let state = svc.insights.snapshot();
        // Demo mode: show deterministic insights straight away (no model).
        if portfolio.demo
            && state.report.is_none()
            && !svc.insights.is_running()
            && let Some(view) = portfolio.view.clone()
        {
            svc.insights.generate(view, false);
        }
        self.action_bar(ui, svc, portfolio, &state.phase);
        ui.add_space(GAP);

        let Some(report) = state.report.as_deref() else {
            widgets::empty_state(
                ui,
                ph::SPARKLE,
                "No insights yet",
                "Click “Generate insights” to score your holdings and find opportunities.",
            );
            return;
        };
        if let Some(e) = &report.ai_error {
            banner(ui, ph::INFO, e, WARNING);
            ui.add_space(GAP);
        }

        let covered = report
            .holdings
            .iter()
            .filter(|h| h.fundamentals.is_some())
            .count();
        let reviews = report
            .holdings
            .iter()
            .filter(|h| h.signal == Signal::Review)
            .count();
        kpi_row(
            ui,
            "insight-kpis",
            &[
                Kpi::new(
                    ph::GAUGE,
                    "Portfolio score",
                    report
                        .portfolio_score
                        .map_or_else(|| "—".into(), |s| format!("{s:.0} / 100")),
                    "value-weighted fundamentals",
                    ACCENT,
                ),
                Kpi::new(
                    ph::TARGET,
                    "Opportunities",
                    report.opportunities.len().to_string(),
                    "pass valuation + quality screen",
                    POSITIVE,
                ),
                Kpi::new(
                    ph::WARNING_CIRCLE,
                    "To review",
                    reviews.to_string(),
                    "expensive, weak or oversized",
                    NEGATIVE,
                ),
                Kpi::new(
                    ph::DATABASE,
                    "Coverage",
                    format!("{covered} / {}", report.holdings.len()),
                    format!("fundamentals · {}", report.provider),
                    SKY,
                ),
            ],
        );
        ui.add_space(GAP);

        if let Some(ai) = &report.ai {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    card_header(ui, ph::SPARKLE, "AI summary", "");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(m) = &report.model {
                            ui.label(
                                RichText::new(format!("local model · {m}"))
                                    .size(11.5)
                                    .color(MUTED),
                            );
                        }
                    });
                });
                ui.label(RichText::new(&ai.headline).font(semibold(18.0)));
                ui.add_space(4.0);
                ui.label(RichText::new(&ai.overview).size(13.5));
                ui.add_space(10.0);
                ui.columns(2, |c| {
                    bullet_list(&mut c[0], "Recommendations", ACCENT, &ai.recommendations);
                    bullet_list(&mut c[1], "Key risks", NEGATIVE, &ai.risks);
                });
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "AI-generated analysis from a local model. Not financial advice.",
                    )
                    .size(11.0)
                    .color(MUTED),
                );
            });
            ui.add_space(GAP);
        }

        card_row(ui, "insight-lists", &[2, 2], |i, ui| match i {
            0 => opportunities(ui, report),
            _ => observations(ui, report),
        });
        ui.add_space(GAP);

        widgets::section_title(ui, "Holding analysis");
        ui.add_space(8.0);
        for (row, pair) in report.holdings.chunks(2).enumerate() {
            let spans = vec![1; 2];
            card_row(ui, &format!("holding-{row}"), &spans, |i, ui| {
                if let Some(h) = pair.get(i) {
                    holding_card(ui, h, &report.currency);
                }
            });
            ui.add_space(GAP);
        }
    }

    fn action_bar(
        &mut self,
        ui: &mut Ui,
        svc: &Services,
        portfolio: &PortfolioState,
        phase: &InsightsPhase,
    ) {
        let model_ready = svc.llm.endpoint().is_some();
        let running = svc.insights.is_running();
        card(ui, |ui| {
            ui.horizontal(|ui| {
                let can_run = portfolio.view.is_some() && !running;
                let generate = ui.add_enabled(
                    can_run,
                    egui::Button::new(
                        RichText::new(format!("{}  Generate insights", ph::SPARKLE))
                            .font(semibold(14.0))
                            .color(BG),
                    )
                    .fill(ACCENT)
                    .min_size(egui::vec2(0.0, 36.0)),
                );
                if generate.clicked()
                    && let Some(view) = portfolio.view.clone()
                {
                    svc.insights.generate(view, self.use_ai && model_ready);
                }

                let exporting = matches!(*self.export.lock(), Export::Running);
                let export = ui.add_enabled(
                    portfolio.view.is_some() && !exporting,
                    egui::Button::new(format!("{}  Export PDF report", ph::FILE_PDF))
                        .min_size(egui::vec2(0.0, 36.0)),
                );
                if export.clicked()
                    && let Some(view) = portfolio.view.clone()
                {
                    let insights = svc.insights.snapshot().report;
                    let slot = self.export.clone();
                    let dir = crate::report::reports_dir(&svc.paths.root);
                    let demo = portfolio.demo;
                    *slot.lock() = Export::Running;
                    std::thread::spawn(move || {
                        let result = crate::report::save(&view, insights.as_deref(), demo, &dir);
                        *slot.lock() = match result {
                            Ok(path) => {
                                let _ = crate::report::open(&path);
                                Export::Done(path)
                            }
                            Err(e) => Export::Failed(format!("{e:#}")),
                        };
                    });
                }

                ui.add_space(8.0);
                ui.add_enabled_ui(model_ready, |ui| {
                    ui.checkbox(&mut self.use_ai, "Include AI analysis");
                });
                if !model_ready {
                    ui.label(
                        RichText::new("load a model in AI Engine to enable")
                            .size(12.0)
                            .color(MUTED),
                    );
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| match phase {
                    InsightsPhase::Fetching { done, total } => {
                        ui.label(
                            RichText::new(format!("Fetching fundamentals {done}/{total}"))
                                .size(12.5)
                                .color(MUTED),
                        );
                        ui.spinner();
                    }
                    InsightsPhase::Analysing {
                        done,
                        total,
                        current,
                    } => {
                        ui.label(
                            RichText::new(format!("AI analysing {current} ({done}/{total})"))
                                .size(12.5)
                                .color(MUTED),
                        );
                        ui.spinner();
                    }
                    InsightsPhase::Failed(e) => {
                        ui.label(
                            RichText::new(format!("failed: {e}"))
                                .size(12.5)
                                .color(NEGATIVE),
                        );
                    }
                    InsightsPhase::Idle => {
                        if let Some(r) = svc.insights.snapshot().report {
                            let local = r.generated_at.with_timezone(&chrono::Local);
                            ui.label(
                                RichText::new(format!("generated {}", local.format("%d %b %H:%M")))
                                    .size(12.5)
                                    .color(MUTED),
                            );
                        }
                    }
                });
            });

            match &*self.export.lock() {
                Export::Running => {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new("Typesetting your report…")
                                .size(12.5)
                                .color(MUTED),
                        );
                    });
                }
                Export::Done(path) => {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(ph::CHECK_CIRCLE).color(POSITIVE));
                        ui.label(
                            RichText::new(format!("Saved {}", path.display()))
                                .size(12.5)
                                .color(MUTED),
                        );
                        if ui.small_button("Open").clicked() {
                            let _ = crate::report::open(path);
                        }
                    });
                }
                Export::Failed(e) => {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!("Export failed: {e}"))
                            .size(12.5)
                            .color(NEGATIVE),
                    );
                }
                Export::Idle => {}
            }
            let provider = svc.insights.provider();
            if provider == crate::insights::fundamentals::Provider::None {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!(
                        "{}  No fundamentals source configured — scores use portfolio data only. Choose a source in Settings.",
                        ph::INFO
                    ))
                    .size(12.0)
                    .color(WARNING),
                );
            }
        });
    }
}

fn bullet_list(ui: &mut Ui, title: &str, color: Color32, items: &[String]) {
    ui.label(
        RichText::new(title.to_uppercase())
            .font(semibold(11.0))
            .color(color),
    );
    ui.add_space(2.0);
    for item in items {
        ui.horizontal_top(|ui| {
            ui.label(RichText::new("•").color(color));
            ui.add(egui::Label::new(RichText::new(item).size(13.0)).wrap());
        });
    }
}

fn opportunities(ui: &mut Ui, r: &InsightsReport) {
    card_header(
        ui,
        ph::TARGET,
        "Buy opportunities",
        "overall ≥ 65 · valuation ≥ 55",
    );
    if let Some(ai) = &r.ai
        && !ai.opportunities.is_empty()
    {
        bullet_list(ui, "AI view", POSITIVE, &ai.opportunities);
        ui.add_space(8.0);
    }
    if r.opportunities.is_empty() {
        ui.label(RichText::new("No holding passes the opportunity screen right now.").color(MUTED));
        return;
    }
    for t in &r.opportunities {
        let Some(h) = r.holdings.iter().find(|h| &h.ticker == t) else {
            continue;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(short_ticker(&h.ticker)).font(semibold(14.0)));
            ui.label(RichText::new(&h.name).size(12.5).color(MUTED));
            right(ui, |ui| {
                chip(
                    ui,
                    &format!("Score {:.0}", h.scores.overall.unwrap_or(0.0)),
                    POSITIVE,
                );
            });
        });
        if h.reasons.is_empty()
            && let Some(f) = &h.fundamentals
        {
            // Passed the screen without a stand-out rule: show key metrics.
            let parts: Vec<String> = [
                f.pe_ttm.map(|v| format!("P/E {v:.1}")),
                f.peg.map(|v| format!("PEG {v:.2}")),
                f.revenue_growth
                    .map(|v| format!("revenue {}", format::signed_pct(v))),
                f.roe.map(|v| format!("ROE {}", format::pct(v))),
            ]
            .into_iter()
            .flatten()
            .collect();
            ui.label(RichText::new(format!("  · {}", parts.join(" · "))).size(12.5));
        }
        for reason in h.reasons.iter().take(3) {
            ui.label(RichText::new(format!("  · {reason}")).size(12.5));
        }
        ui.add_space(8.0);
    }
}

fn observations(ui: &mut Ui, r: &InsightsReport) {
    card_header(ui, ph::CHART_PIE_SLICE, "Portfolio observations", "");
    if r.flags.is_empty() {
        ui.label(RichText::new("No structural issues detected.").color(MUTED));
    }
    for f in &r.flags {
        ui.horizontal_top(|ui| {
            ui.label(RichText::new(ph::WARNING).color(WARNING));
            ui.add(egui::Label::new(RichText::new(f).size(13.0)).wrap());
        });
    }
    ui.add_space(10.0);
    widgets::section_title(ui, "Sectors");
    ui.add_space(4.0);
    for (i, (name, w)) in r.sectors.iter().take(7).enumerate() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(name).size(12.5));
            right(ui, |ui| {
                ui.label(RichText::new(format::pct(*w)).monospace().size(12.5));
            });
        });
        super::dashboard::weight_bar(ui, *w, SERIES[i % SERIES.len()]);
        ui.add_space(4.0);
    }
}

/// Horizontal 0–100 score bar with label.
fn score_bar(ui: &mut Ui, label: &str, score: Option<f64>) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [70.0, 16.0],
            egui::Label::new(RichText::new(label).size(11.5).color(MUTED)),
        );
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width() - 34.0, 6.0),
            egui::Sense::hover(),
        );
        let p = ui.painter();
        p.rect_filled(rect, egui::CornerRadius::same(3), SURFACE_2);
        if let Some(s) = score {
            let mut fill = rect;
            fill.set_width(rect.width() * (s / 100.0) as f32);
            let color = if s >= 65.0 {
                POSITIVE
            } else if s >= 40.0 {
                ACCENT
            } else {
                NEGATIVE
            };
            p.rect_filled(fill, egui::CornerRadius::same(3), color);
        }
        right(ui, |ui| {
            ui.label(
                RichText::new(score.map_or_else(|| "—".into(), |s| format!("{s:.0}")))
                    .monospace()
                    .size(12.0),
            );
        });
    });
}

fn holding_card(ui: &mut Ui, h: &HoldingInsight, _ccy: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(short_ticker(&h.ticker)).font(semibold(16.0)));
        ui.label(RichText::new(&h.name).size(12.5).color(MUTED));
        right(ui, |ui| {
            chip(ui, h.signal.label(), signal_color(h.signal));
            ui.label(
                RichText::new(format!("{} of portfolio", format::pct(h.weight)))
                    .size(11.5)
                    .color(MUTED),
            );
        });
    });
    ui.add_space(8.0);

    let s = &h.scores;
    score_bar(ui, "Overall", s.overall);
    score_bar(ui, "Valuation", s.valuation);
    score_bar(ui, "Growth", s.growth);
    score_bar(ui, "Quality", s.quality);
    score_bar(ui, "Income", s.income);

    if let Some(f) = &h.fundamentals {
        ui.add_space(8.0);
        let fmt = |v: Option<f64>, f: fn(f64) -> String| v.map_or_else(|| "—".to_string(), f);
        let metrics: Vec<(&str, String)> = if f.is_etf() {
            vec![
                (
                    "TER",
                    fmt(f.expense_ratio, |t| format!("{:.2}%", t * 100.0)),
                ),
                ("Yield", fmt(f.dividend_yield, format::pct)),
                ("P/E", fmt(f.pe_ttm, |v| format!("{v:.1}"))),
            ]
        } else {
            vec![
                ("P/E", fmt(f.pe_ttm, |v| format!("{v:.1}"))),
                ("PEG", fmt(f.peg, |v| format!("{v:.2}"))),
                ("FCF yld", fmt(f.fcf_yield, format::pct)),
                ("Rev gr.", fmt(f.revenue_growth, format::signed_pct)),
            ]
        };
        let w = (ui.available_width() - 8.0 * (metrics.len() - 1) as f32) / metrics.len() as f32;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (k, v) in metrics {
                egui::Frame::new()
                    .fill(SURFACE_2)
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.set_width(w - 16.0);
                        ui.label(RichText::new(k.to_uppercase()).size(10.0).color(MUTED));
                        ui.label(RichText::new(v).font(semibold(13.5)));
                    });
            }
        });
    }

    if !h.reasons.is_empty() {
        ui.add_space(8.0);
        for r in h.reasons.iter().take(4) {
            ui.label(RichText::new(format!("· {r}")).size(12.5));
        }
    }
    if let Some(ai) = &h.ai {
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(VIOLET.linear_multiply(0.10))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{}  AI VIEW", ph::SPARKLE))
                            .font(semibold(11.0))
                            .color(VIOLET),
                    );
                    chip(ui, &ai.verdict, verdict_color(&ai.verdict));
                });
                ui.add(egui::Label::new(RichText::new(&ai.thesis).size(13.0)).wrap());
                if !ai.strengths.is_empty() {
                    ui.label(
                        RichText::new(format!("+ {}", ai.strengths.join(" · ")))
                            .size(12.0)
                            .color(POSITIVE),
                    );
                }
                if !ai.risks.is_empty() {
                    ui.label(
                        RichText::new(format!("− {}", ai.risks.join(" · ")))
                            .size(12.0)
                            .color(NEGATIVE),
                    );
                }
            });
    }
}
