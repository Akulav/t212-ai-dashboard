//! Synthetic fundamentals for the demo portfolio (`AURA_DEMO=1`), so the
//! insights page and reports can be explored and screenshotted without a
//! data provider. Figures are plausible but made up.

use super::fundamentals::{Fundamentals, Lookup, Quarter};

struct Row {
    ticker: &'static str,
    kind: &'static str,
    sector: &'static str,
    pe: f64,
    fwd: f64,
    peg: f64,
    fcf_yield: f64,
    rev_g: f64,
    earn_g: f64,
    margin: f64,
    roe: f64,
    de: f64,
    div: f64,
    payout: f64,
    ter: f64,
    /// Latest quarterly revenue in billions (0 = not applicable).
    q_rev_bn: f64,
}

#[rustfmt::skip]
const ROWS: &[Row] = &[
    Row { ticker: "VUSAl_EQ", kind: "etf", sector: "Global equity ETF", pe: 26.0, fwd: 0.0, peg: 0.0, fcf_yield: 0.0, rev_g: 0.0, earn_g: 0.0, margin: 0.0, roe: 0.0, de: 0.0, div: 0.011, payout: 0.0, ter: 0.0007, q_rev_bn: 0.0 },
    Row { ticker: "VHYLl_EQ", kind: "etf", sector: "Global equity ETF", pe: 14.0, fwd: 0.0, peg: 0.0, fcf_yield: 0.0, rev_g: 0.0, earn_g: 0.0, margin: 0.0, roe: 0.0, de: 0.0, div: 0.031, payout: 0.0, ter: 0.0029, q_rev_bn: 0.0 },
    Row { ticker: "AAPL_US_EQ", kind: "stock", sector: "Technology", pe: 33.0, fwd: 29.0, peg: 2.4, fcf_yield: 0.032, rev_g: 0.06, earn_g: 0.10, margin: 0.25, roe: 0.24, de: 1.5, div: 0.0045, payout: 0.15, ter: 0.0, q_rev_bn: 102.0 },
    Row { ticker: "MSFT_US_EQ", kind: "stock", sector: "Technology", pe: 35.0, fwd: 31.0, peg: 2.1, fcf_yield: 0.025, rev_g: 0.15, earn_g: 0.17, margin: 0.36, roe: 0.33, de: 0.3, div: 0.0075, payout: 0.25, ter: 0.0, q_rev_bn: 74.0 },
    Row { ticker: "NVDA_US_EQ", kind: "stock", sector: "Technology", pe: 52.0, fwd: 33.0, peg: 1.1, fcf_yield: 0.02, rev_g: 0.62, earn_g: 0.71, margin: 0.55, roe: 0.25, de: 0.2, div: 0.0003, payout: 0.01, ter: 0.0, q_rev_bn: 51.0 },
    Row { ticker: "ULVRl_EQ", kind: "stock", sector: "Consumer Staples", pe: 19.0, fwd: 18.0, peg: 2.9, fcf_yield: 0.045, rev_g: 0.04, earn_g: 0.05, margin: 0.12, roe: 0.30, de: 1.6, div: 0.033, payout: 0.75, ter: 0.0, q_rev_bn: 15.2 },
    Row { ticker: "KO_US_EQ", kind: "stock", sector: "Consumer Staples", pe: 24.0, fwd: 22.0, peg: 3.1, fcf_yield: 0.028, rev_g: 0.03, earn_g: 0.06, margin: 0.23, roe: 0.40, de: 1.7, div: 0.031, payout: 0.72, ter: 0.0, q_rev_bn: 12.4 },
    Row { ticker: "O_US_EQ", kind: "stock", sector: "Real Estate", pe: 52.0, fwd: 40.0, peg: 5.0, fcf_yield: 0.058, rev_g: 0.09, earn_g: 0.02, margin: 0.17, roe: 0.03, de: 0.7, div: 0.058, payout: 2.9, ter: 0.0, q_rev_bn: 1.4 },
    Row { ticker: "LGENl_EQ", kind: "stock", sector: "Financials", pe: 11.0, fwd: 9.5, peg: 0.9, fcf_yield: 0.09, rev_g: -0.02, earn_g: 0.12, margin: 0.06, roe: 0.18, de: 1.9, div: 0.087, payout: 0.95, ter: 0.0, q_rev_bn: 5.1 },
    Row { ticker: "ASML_NA_EQ", kind: "stock", sector: "Technology", pe: 31.0, fwd: 26.0, peg: 1.3, fcf_yield: 0.03, rev_g: 0.12, earn_g: 0.18, margin: 0.27, roe: 0.45, de: 0.25, div: 0.009, payout: 0.3, ter: 0.0, q_rev_bn: 7.6 },
];

fn opt(v: f64) -> Option<f64> {
    (v != 0.0).then_some(v)
}

/// Demo fundamentals for a demo ticker, `None` for anything else.
pub fn fundamentals(lookup: &Lookup) -> Option<Fundamentals> {
    let r = ROWS.iter().find(|r| r.ticker == lookup.ticker)?;
    let quarters = if r.q_rev_bn > 0.0 {
        // Four quarters ending at the latest, back-solved from YoY growth.
        let q_growth = (1.0 + r.rev_g).powf(0.25);
        ["Q4 2025", "Q1 2026", "Q2 2026", "Q3 2026"]
            .iter()
            .enumerate()
            .map(|(i, label)| {
                let rev = r.q_rev_bn * 1e9 / q_growth.powi(3 - i as i32);
                Quarter {
                    label: (*label).into(),
                    revenue: Some(rev),
                    net_income: Some(rev * r.margin),
                    eps: None,
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    Some(Fundamentals {
        symbol: lookup
            .ticker
            .split('_')
            .next()
            .unwrap_or_default()
            .trim_end_matches('l')
            .to_string(),
        kind: r.kind.into(),
        sector: Some(r.sector.into()),
        pe_ttm: opt(r.pe),
        pe_forward: opt(r.fwd),
        peg: opt(r.peg),
        fcf_yield: opt(r.fcf_yield),
        revenue_growth: opt(r.rev_g),
        earnings_growth: opt(r.earn_g),
        profit_margin: opt(r.margin),
        roe: opt(r.roe),
        debt_to_equity: opt(r.de),
        dividend_yield: opt(r.div),
        payout_ratio: opt(r.payout),
        expense_ratio: opt(r.ter),
        quarters,
        source: "Demo data".into(),
        ..Default::default()
    })
}
