//! Branded PDF report: portfolio state + AI insights, typeset with Typst.
//!
//! [`build_data`] turns the portfolio view and the latest
//! [`InsightsReport`] into a flat, pre-formatted JSON document; the Typst
//! template (`template.typ`) only lays it out. Fonts: the OS's own UI fonts
//! (Segoe UI / SF / Inter / Noto) are searched first, with Typst's embedded
//! fonts as a fallback, so the PDF renders anywhere.
//!
//! Footer on every page: "Aura Portfolio · Powered by AI · Made by Akulav".

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::{Local, Utc};
use serde::Serialize;
use typst::foundations::{Dict, IntoValue};
use typst_as_lib::TypstEngine;
use typst_as_lib::typst_kit_options::TypstKitFontOptions;
use typst_layout::PagedDocument;

use crate::insights::{HoldingInsight, InsightsReport};
use crate::portfolio::PortfolioView;
use crate::ui::dashboard::short_ticker;
use crate::ui::format;

const TEMPLATE: &str = include_str!("template.typ");
const LOGO: &[u8] = include_bytes!("../../assets/icons/256x256.png");

/// Holdings that get a detailed analysis card.
const ANALYSIS_CARDS: usize = 10;

#[derive(Serialize)]
struct Kpi {
    label: String,
    value: String,
    caption: String,
    /// `pos`, `neg` or `neu`.
    tone: &'static str,
}

#[derive(Serialize)]
struct AiBlock {
    headline: String,
    overview: String,
    recommendations: Vec<String>,
    opportunities: Vec<String>,
    risks: Vec<String>,
}

#[derive(Serialize)]
struct Opportunity {
    ticker: String,
    name: String,
    score: String,
    reasons: String,
}

#[derive(Serialize)]
struct Sector {
    name: String,
    /// 0–100.
    weight: f64,
    label: String,
}

#[derive(Serialize)]
struct Row {
    ticker: String,
    name: String,
    weight: String,
    value: String,
    pnl: String,
    pnl_tone: &'static str,
    pe: String,
    score: String,
    signal: String,
}

#[derive(Serialize)]
struct Metric {
    k: &'static str,
    v: String,
}

#[derive(Serialize)]
struct AiNote {
    verdict: String,
    thesis: String,
    strengths: Vec<String>,
    risks: Vec<String>,
}

#[derive(Serialize)]
struct Analysis {
    ticker: String,
    name: String,
    signal: String,
    score: String,
    metrics: Vec<Metric>,
    reasons: Vec<String>,
    ai: Option<AiNote>,
}

/// Everything the template renders (all strings pre-formatted).
#[derive(Serialize)]
pub struct ReportData {
    title: String,
    subtitle: String,
    generated: String,
    demo: bool,
    model_line: String,
    provider_line: String,
    holdings_note: String,
    kpis: Vec<Kpi>,
    ai: Option<AiBlock>,
    ai_note: String,
    opportunities: Vec<Opportunity>,
    flags: Vec<String>,
    sectors: Vec<Sector>,
    holdings: Vec<Row>,
    analyses: Vec<Analysis>,
    income: Vec<Kpi>,
}

fn tone(v: f64) -> &'static str {
    if v > 0.0 {
        "pos"
    } else if v < 0.0 {
        "neg"
    } else {
        "neu"
    }
}

fn opt(v: Option<f64>, f: impl Fn(f64) -> String) -> String {
    v.map_or_else(|| "—".into(), f)
}

fn ratio(v: f64) -> String {
    format!("{v:.1}")
}

fn metrics(h: &HoldingInsight) -> Vec<Metric> {
    let Some(f) = &h.fundamentals else {
        return vec![
            Metric {
                k: "Weight",
                v: format::pct(h.weight),
            },
            Metric {
                k: "P/L",
                v: opt(h.pnl_pct, format::signed_pct),
            },
        ];
    };
    if f.is_etf() {
        return vec![
            Metric {
                k: "Weight",
                v: format::pct(h.weight),
            },
            Metric {
                k: "Ongoing charge",
                v: opt(f.expense_ratio, |t| format!("{:.2}%", t * 100.0)),
            },
            Metric {
                k: "Yield",
                v: opt(f.dividend_yield, format::pct),
            },
            Metric {
                k: "P/E (holdings)",
                v: opt(f.pe_ttm, ratio),
            },
        ];
    }
    vec![
        Metric {
            k: "P/E ttm",
            v: opt(f.pe_ttm, ratio),
        },
        Metric {
            k: "P/E fwd",
            v: opt(f.pe_forward, ratio),
        },
        Metric {
            k: "PEG",
            v: opt(f.peg, |v| format!("{v:.2}")),
        },
        Metric {
            k: "FCF yield",
            v: opt(f.fcf_yield, format::pct),
        },
        Metric {
            k: "Revenue growth",
            v: opt(f.revenue_growth, format::signed_pct),
        },
        Metric {
            k: "Net margin",
            v: opt(f.profit_margin, format::pct),
        },
        Metric {
            k: "ROE",
            v: opt(f.roe, format::pct),
        },
        Metric {
            k: "Dividend yield",
            v: opt(f.dividend_yield, format::pct),
        },
    ]
}

/// Builds the template input from the portfolio and (optional) insights.
pub fn build_data(
    view: &PortfolioView,
    insights: Option<&InsightsReport>,
    demo: bool,
) -> ReportData {
    let ccy = view.summary.currency.as_str();
    let a = &view.analytics;
    let d = &a.dividends;
    let now = Local::now();

    let kpis = vec![
        Kpi {
            label: "Portfolio value".into(),
            value: format::money(view.summary.total_value, ccy),
            caption: format!("{} positions", a.holdings.len()),
            tone: "neu",
        },
        Kpi {
            label: "Total return".into(),
            value: format::signed_money(a.total_return, ccy),
            caption: a.total_return_pct.map_or_else(String::new, |p| {
                format!(
                    "{} {}",
                    format::signed_pct(p),
                    if a.total_return_from_deposits {
                        "vs. deposits"
                    } else {
                        "on positions"
                    }
                )
            }),
            tone: tone(a.total_return),
        },
        Kpi {
            label: "Dividends (12m)".into(),
            value: format::money(d.last_12m, ccy),
            caption: format!("{} lifetime", format::money(d.total, ccy)),
            tone: "neu",
        },
        Kpi {
            label: "Fundamental score".into(),
            value: insights
                .and_then(|r| r.portfolio_score)
                .map_or_else(|| "—".into(), |s| format!("{s:.0} / 100")),
            caption: "value-weighted".into(),
            tone: "neu",
        },
    ];

    let (ai, ai_note, model_line) = match insights {
        Some(r) => {
            let block = r.ai.as_ref().map(|n| AiBlock {
                headline: n.headline.clone(),
                overview: n.overview.clone(),
                recommendations: n.recommendations.clone(),
                opportunities: n.opportunities.clone(),
                risks: n.risks.clone(),
            });
            let note = r
                .ai_error
                .clone()
                .unwrap_or_else(|| "AI commentary was not generated for this report.".into());
            let model = r.model.as_ref().map_or_else(
                || "rule-based analysis".into(),
                |m| format!("local model: {m}"),
            );
            (block, note, model)
        }
        None => (
            None,
            "Run “Generate insights” in Aura to add analysis to this report.".into(),
            String::new(),
        ),
    };

    let holdings_src: Vec<&HoldingInsight> = insights
        .map(|r| r.holdings.iter().collect())
        .unwrap_or_default();
    let holdings: Vec<Row> = if holdings_src.is_empty() {
        a.holdings
            .iter()
            .map(|h| Row {
                ticker: short_ticker(&h.ticker).into(),
                name: h.name.clone(),
                weight: format::pct(h.weight),
                value: format::money(h.value, ccy),
                pnl: opt(h.pnl_pct, format::signed_pct),
                pnl_tone: tone(h.pnl),
                pe: "—".into(),
                score: "—".into(),
                signal: "—".into(),
            })
            .collect()
    } else {
        holdings_src
            .iter()
            .map(|h| Row {
                ticker: short_ticker(&h.ticker).into(),
                name: h.name.clone(),
                weight: format::pct(h.weight),
                value: format::money(h.value, ccy),
                pnl: opt(h.pnl_pct, format::signed_pct),
                pnl_tone: tone(h.pnl_pct.unwrap_or(0.0)),
                pe: opt(h.fundamentals.as_ref().and_then(|f| f.pe_ttm), ratio),
                score: opt(h.scores.overall, |s| format!("{s:.0}")),
                signal: h.signal.label().into(),
            })
            .collect()
    };

    let opportunities = insights
        .map(|r| {
            r.opportunities
                .iter()
                .filter_map(|t| r.holdings.iter().find(|h| &h.ticker == t))
                .map(|h| Opportunity {
                    ticker: short_ticker(&h.ticker).into(),
                    name: h.name.clone(),
                    score: opt(h.scores.overall, |s| format!("{s:.0}")),
                    reasons: if h.reasons.is_empty() {
                        // Passed the screen without a stand-out rule: show the key metrics.
                        metrics(h)
                            .iter()
                            .take(4)
                            .map(|m| format!("{} {}", m.k, m.v))
                            .collect::<Vec<_>>()
                            .join(" · ")
                    } else {
                        h.reasons
                            .iter()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(" · ")
                    },
                })
                .collect()
        })
        .unwrap_or_default();

    let analyses = holdings_src
        .iter()
        .take(ANALYSIS_CARDS)
        .map(|h| Analysis {
            ticker: short_ticker(&h.ticker).into(),
            name: h.name.clone(),
            signal: h.signal.label().into(),
            score: opt(h.scores.overall, |s| format!("{s:.0}")),
            metrics: metrics(h),
            reasons: h.reasons.clone(),
            ai: h.ai.as_ref().map(|n| AiNote {
                verdict: n.verdict.clone(),
                thesis: n.thesis.clone(),
                strengths: n.strengths.clone(),
                risks: n.risks.clone(),
            }),
        })
        .collect();

    let cutoff = Utc::now().date_naive() + chrono::Duration::days(90);
    let next90: f64 = a
        .upcoming
        .iter()
        .filter(|u| u.expected_on <= cutoff)
        .map(|u| u.amount)
        .sum();
    let income = vec![
        Kpi {
            label: "Lifetime".into(),
            value: format::money(d.total, ccy),
            caption: format!("{} payments", d.payments),
            tone: "neu",
        },
        Kpi {
            label: "Year to date".into(),
            value: format::money(d.ytd, ccy),
            caption: "calendar year".into(),
            tone: "neu",
        },
        Kpi {
            label: "Avg. tax withheld".into(),
            value: opt(d.tax.rate, format::pct),
            caption: format!("{} withheld", format::money(d.tax.withheld, ccy)),
            tone: "neu",
        },
        Kpi {
            label: "Next 90 days".into(),
            value: format!("≈{}", format::money(next90, ccy)),
            caption: "projected".into(),
            tone: "neu",
        },
    ];

    ReportData {
        title: "Portfolio Report".into(),
        subtitle: format!(
            "{} · {} · {}",
            now.format("%d %B %Y"),
            format::money(view.summary.total_value, ccy),
            ccy
        ),
        generated: now.format("%d %b %Y %H:%M").to_string(),
        demo,
        model_line,
        provider_line: insights.map_or_else(
            || "".into(),
            |r| format!("fundamentals from {},", r.provider),
        ),
        holdings_note: format!("{} positions", holdings.len()),
        kpis,
        ai,
        ai_note,
        opportunities,
        flags: insights.map(|r| r.flags.clone()).unwrap_or_default(),
        sectors: insights
            .map(|r| {
                r.sectors
                    .iter()
                    .map(|(n, w)| Sector {
                        name: n.clone(),
                        weight: w * 100.0,
                        label: format::pct(*w),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        holdings,
        analyses,
        income,
    }
}

/// Lays out the report (pages). Blocking (font scan + layout take ~1 s);
/// call from a background thread.
pub fn layout(data: &ReportData) -> Result<PagedDocument> {
    let json = serde_json::to_string(data)?;
    let engine = TypstEngine::builder()
        .main_file(TEMPLATE)
        .search_fonts_with(TypstKitFontOptions::default())
        .with_static_file_resolver([("logo.png", LOGO)])
        .build();
    let mut inputs = Dict::new();
    inputs.insert("data".into(), json.into_value());
    engine
        .compile_with_input(inputs)
        .output
        .map_err(|e| anyhow!("report layout failed: {e:?}"))
}

/// Renders the report to PDF bytes (see [`layout`]).
pub fn render_pdf(data: &ReportData) -> Result<Vec<u8>> {
    let doc = layout(data)?;
    typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default())
        .map_err(|e| anyhow!("PDF export failed: {e:?}"))
}

/// Default folder for reports: `Documents/Aura Reports` (or the data dir).
pub fn reports_dir(fallback: &Path) -> PathBuf {
    directories::UserDirs::new()
        .and_then(|u| u.document_dir().map(|d| d.join("Aura Reports")))
        .unwrap_or_else(|| fallback.join("reports"))
}

/// Renders and saves a report, returning its path.
pub fn save(
    view: &PortfolioView,
    insights: Option<&InsightsReport>,
    demo: bool,
    dir: &Path,
) -> Result<PathBuf> {
    let pdf = render_pdf(&build_data(view, insights, demo))?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let name = format!(
        "Aura-Portfolio-Report-{}.pdf",
        Local::now().format("%Y-%m-%d-%H%M")
    );
    let path = dir.join(name);
    std::fs::write(&path, pdf).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// Opens a file with the OS default application.
pub fn open(path: &Path) -> Result<()> {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]).arg(path);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(path);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path);
        c
    };
    cmd.spawn().context("could not open the report")?;
    Ok(())
}
