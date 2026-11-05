//! Renders a sample PDF report from the demo portfolio, to check the report
//! layout without Trading212 or a model. Sample AI text is filled in here
//! only so every section of the template renders; the app never shows AI
//! text that a real model didn't produce.
//!
//! ```text
//! cargo run --example report_demo -- [output.pdf]
//! ```

use aura_portfolio::insights::ai::{AiHoldingNote, AiPortfolioNote};
use aura_portfolio::insights::fundamentals::Lookup;
use aura_portfolio::insights::{build_report, demo as idemo};
use aura_portfolio::portfolio::{PortfolioView, analytics, demo};
use aura_portfolio::report;
use chrono::Utc;

fn main() -> anyhow::Result<()> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/report-demo.pdf".into());
    let now = Utc::now();
    let c = demo::cache(now);
    let summary = c.summary.clone().unwrap();
    let analytics = analytics::compute(&summary, &c.positions, &c.dividends, &c.transactions, now);
    let view = PortfolioView {
        summary,
        positions: c.positions.clone(),
        analytics,
        snapshots: c.snapshots.clone(),
        dividend_count: c.dividends.len(),
        recent_dividends: c.dividends.iter().take(50).cloned().collect(),
        synced_at: Some(now),
        history_synced_at: Some(now),
    };
    let fundamentals = view
        .analytics
        .holdings
        .iter()
        .map(|r| {
            idemo::fundamentals(&Lookup {
                ticker: r.ticker.clone(),
                isin: r.isin.clone(),
                name: r.name.clone(),
                currency: r.quote_currency.clone(),
                price: None,
            })
        })
        .collect();
    let mut r = build_report(&view, fundamentals, "Demo data");
    r.model = Some("Sample text (layout check)".into());
    r.ai = Some(AiPortfolioNote {
        headline: "Quality-tilted growth portfolio with a heavy tech core".into(),
        overview: "Two broad ETFs anchor roughly half the portfolio, while technology names drive most of the gains. \
                   Valuations on the US mega-caps are rich, but cash generation and margins remain strong."
            .into(),
        recommendations: vec![
            "Direct new contributions to the cheapest quality holdings rather than the largest positions.".into(),
            "Review Realty Income: payout exceeds earnings and growth is slowing.".into(),
            "Keep technology exposure below a third of the portfolio.".into(),
        ],
        opportunities: vec!["LGEN: P/E 11 with a 9% FCF yield; watch dividend cover.".into()],
        risks: vec!["Concentration in US large-cap tech".into(), "GBP/USD moves affect most returns".into()],
    });
    for h in r.holdings.iter_mut().take(4) {
        h.ai = Some(AiHoldingNote {
            verdict: "Hold".into(),
            thesis: "Solid fundamentals at a full valuation; position size is appropriate.".into(),
            strengths: vec!["High margins".into(), "Strong cash flow".into()],
            risks: vec!["Valuation".into()],
        });
    }
    let data = report::build_data(&view, Some(&r), true);
    let started = std::time::Instant::now();
    let pdf = report::render_pdf(&data)?;
    std::fs::write(&out, &pdf)?;
    println!(
        "wrote {out} ({} KB) in {:?}",
        pdf.len() / 1024,
        started.elapsed()
    );

    // Page previews (PNG) next to the PDF, for visual review.
    let doc = report::layout(&data)?;
    let opts = typst_render::RenderOptions {
        pixel_per_pt: 1.4.into(),
        ..Default::default()
    };
    for (i, page) in doc.pages().iter().enumerate() {
        let png = format!("{}-page{}.png", out.trim_end_matches(".pdf"), i + 1);
        typst_render::render(page, &opts).save_png(&png)?;
        println!("preview {png}");
    }
    Ok(())
}
