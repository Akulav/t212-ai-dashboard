//! Deterministic, explainable scoring — the quantitative backbone of the
//! insights. Works without an LLM; the AI layer adds narrative on top.
//!
//! Each holding gets four 0–100 sub-scores and an overall score:
//!
//! | Score | Inputs |
//! |---|---|
//! | Valuation | P/E (trailing, forward), PEG, FCF yield, P/B |
//! | Growth | revenue and earnings growth (YoY) |
//! | Quality | margins, ROE, debt/equity |
//! | Income | dividend yield, payout sustainability |
//!
//! Missing inputs are skipped (a score averages only what is known), and a
//! score with no inputs is `None`. ETFs are rated on cost and income only.

use serde::{Deserialize, Serialize};

use super::fundamentals::Fundamentals;
use crate::portfolio::analytics::HoldingRow;

/// Sub-scores and overall score, each 0–100.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Scores {
    /// Cheapness.
    pub valuation: Option<f64>,
    /// Growth.
    pub growth: Option<f64>,
    /// Business quality / balance sheet.
    pub quality: Option<f64>,
    /// Income.
    pub income: Option<f64>,
    /// Weighted overall.
    pub overall: Option<f64>,
}

/// Deterministic call for a holding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Signal {
    /// Attractive on fundamentals — candidate to add to.
    Opportunity,
    /// Nothing pressing.
    Hold,
    /// Expensive, weak, or oversized — worth reviewing.
    Review,
    /// Not enough data to judge.
    NoData,
}

impl Signal {
    /// UI / report label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Opportunity => "Opportunity",
            Self::Hold => "Hold",
            Self::Review => "Review",
            Self::NoData => "No data",
        }
    }
}

/// Linear map of `v` from `[bad, good]` to `[0, 100]` (works for either
/// direction), clamped.
fn scale(v: f64, bad: f64, good: f64) -> f64 {
    ((v - bad) / (good - bad) * 100.0).clamp(0.0, 100.0)
}

fn mean(parts: &[Option<f64>]) -> Option<f64> {
    let known: Vec<f64> = parts.iter().flatten().copied().collect();
    (!known.is_empty()).then(|| known.iter().sum::<f64>() / known.len() as f64)
}

/// Computes sub-scores from fundamentals.
pub fn score(f: &Fundamentals) -> Scores {
    if f.is_etf() {
        let cost = f.expense_ratio.map(|t| scale(t, 0.008, 0.0005));
        let income = f.dividend_yield.map(|y| scale(y, 0.0, 0.05));
        let valuation = f.pe_ttm.filter(|p| *p > 0.0).map(|p| scale(p, 35.0, 12.0));
        return Scores {
            valuation,
            growth: None,
            quality: cost,
            income,
            overall: mean(&[valuation, cost, income]),
        };
    }

    // Negative earnings → P/E meaningless; treat as poor valuation evidence.
    let pe = |p: Option<f64>| p.map(|p| if p <= 0.0 { 0.0 } else { scale(p, 45.0, 10.0) });
    let valuation = mean(&[
        pe(f.pe_ttm),
        pe(f.pe_forward),
        f.peg.filter(|p| *p > 0.0).map(|p| scale(p, 3.0, 0.8)),
        f.fcf_yield.map(|y| scale(y, -0.01, 0.07)),
        f.price_to_book
            .filter(|p| *p > 0.0)
            .map(|p| scale(p, 12.0, 1.0)),
    ]);
    let growth = mean(&[
        f.revenue_growth.map(|g| scale(g, -0.05, 0.25)),
        f.earnings_growth.map(|g| scale(g, -0.10, 0.30)),
    ]);
    let quality = mean(&[
        f.profit_margin.map(|m| scale(m, 0.0, 0.25)),
        f.operating_margin.map(|m| scale(m, 0.0, 0.30)),
        f.roe.map(|r| scale(r, 0.0, 0.25)),
        f.debt_to_equity.map(|d| scale(d, 2.5, 0.2)),
    ]);
    let income = f.dividend_yield.filter(|y| *y > 0.0).map(|y| {
        let base = scale(y, 0.0, 0.05);
        // Payout above ~90 % of earnings is a sustainability warning.
        match f.payout_ratio {
            Some(p) if p > 0.9 => base * 0.6,
            _ => base,
        }
    });

    // Income only counts toward the overall when the stock pays one.
    let weights: [(Option<f64>, f64); 4] = [
        (valuation, 0.35),
        (growth, 0.25),
        (quality, 0.30),
        (income, 0.10),
    ];
    let (sum, w) = weights
        .iter()
        .filter_map(|(s, w)| s.map(|s| (s * w, *w)))
        .fold((0.0, 0.0), |(a, b), (s, w)| (a + s, b + w));
    Scores {
        valuation,
        growth,
        quality,
        income,
        overall: (w > 0.0).then(|| sum / w),
    }
}

/// Signal + human-readable reasons for one holding.
pub fn assess(row: &HoldingRow, f: Option<&Fundamentals>, s: &Scores) -> (Signal, Vec<String>) {
    let mut reasons = Vec::new();
    let pct = |v: f64| format!("{:.0}%", v * 100.0);

    if row.weight > 0.20 {
        reasons.push(format!(
            "Large position: {} of the portfolio",
            pct(row.weight)
        ));
    }
    let Some(f) = f else {
        let signal = if row.weight > 0.25 {
            Signal::Review
        } else {
            Signal::NoData
        };
        reasons.push("No fundamentals available for this listing".into());
        return (signal, reasons);
    };

    if let Some(pe) = f.pe_ttm {
        if pe <= 0.0 {
            reasons.push("Loss-making on a trailing basis (negative P/E)".into());
        } else if pe < 15.0 {
            reasons.push(format!("Low P/E of {pe:.1}"));
        } else if pe > 40.0 {
            reasons.push(format!("Rich valuation: P/E {pe:.1}"));
        }
    }
    if let Some(peg) = f.peg.filter(|p| *p > 0.0) {
        if peg < 1.0 {
            reasons.push(format!("PEG {peg:.2} — growth looks cheaply priced"));
        } else if peg > 2.5 {
            reasons.push(format!("PEG {peg:.2} — paying up for growth"));
        }
    }
    if let Some(y) = f.fcf_yield {
        if y > 0.05 {
            reasons.push(format!("Strong free-cash-flow yield of {}", pct(y)));
        } else if y < 0.0 {
            reasons.push("Negative free cash flow".into());
        }
    }
    if let Some(g) = f.revenue_growth {
        if g > 0.15 {
            reasons.push(format!("Revenue growing {} YoY", pct(g)));
        } else if g < 0.0 {
            reasons.push(format!("Revenue shrinking {} YoY", pct(-g)));
        }
    }
    if let Some(d) = f.debt_to_equity.filter(|d| *d > 2.0) {
        reasons.push(format!("High leverage: debt/equity {d:.1}"));
    }
    if let (Some(y), Some(p)) = (f.dividend_yield, f.payout_ratio)
        && y > 0.0
        && p > 0.9
    {
        reasons.push(format!(
            "Dividend payout {} of earnings — limited cover",
            pct(p)
        ));
    }
    if let Some(t) = f.expense_ratio.filter(|_| f.is_etf()) {
        reasons.push(format!("Ongoing charge {:.2}%", t * 100.0));
    }
    if row.pnl_pct.is_some_and(|p| p < -0.15) && s.overall.is_some_and(|o| o >= 60.0) {
        reasons.push("Trading below your average cost with solid fundamentals".into());
    }

    let signal = match (s.overall, s.valuation) {
        (None, _) => Signal::NoData,
        _ if row.weight > 0.25 => Signal::Review,
        (Some(o), Some(v)) if o >= 65.0 && v >= 55.0 => Signal::Opportunity,
        (Some(o), _) if o < 35.0 => Signal::Review,
        (_, Some(v)) if v < 15.0 => Signal::Review,
        _ => Signal::Hold,
    };
    (signal, reasons)
}

/// Portfolio-level observations from holdings and sectors.
pub fn portfolio_flags(rows: &[HoldingRow], sectors: &[(String, f64)]) -> Vec<String> {
    let mut flags = Vec::new();
    let top3: f64 = rows.iter().take(3).map(|r| r.weight).sum();
    if top3 > 0.6 {
        flags.push(format!(
            "Concentrated: top 3 positions are {:.0}% of the portfolio",
            top3 * 100.0
        ));
    }
    if let Some((sector, w)) = sectors.first()
        && *w > 0.4
    {
        flags.push(format!("{sector} is {:.0}% of the portfolio", w * 100.0));
    }
    if rows.len() < 8 {
        flags.push(format!(
            "Only {} positions — limited diversification",
            rows.len()
        ));
    }
    let losers = rows
        .iter()
        .filter(|r| r.pnl_pct.is_some_and(|p| p < -0.2))
        .count();
    if losers > 0 {
        flags.push(format!("{losers} position(s) down more than 20% from cost"));
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stock() -> Fundamentals {
        Fundamentals {
            kind: "stock".into(),
            ..Default::default()
        }
    }

    fn row(weight: f64) -> HoldingRow {
        HoldingRow {
            ticker: "X".into(),
            name: "X".into(),
            isin: String::new(),
            quote_currency: "USD".into(),
            quantity: 1.0,
            avg_price: 1.0,
            current_price: 1.0,
            value: 1.0,
            cost: 1.0,
            pnl: 0.0,
            pnl_pct: Some(0.0),
            fx_impact: None,
            weight,
            dividends_received: 0.0,
            yield_on_cost: None,
        }
    }

    #[test]
    fn cheap_quality_growth_is_an_opportunity() {
        let f = Fundamentals {
            pe_ttm: Some(12.0),
            pe_forward: Some(11.0),
            peg: Some(0.8),
            fcf_yield: Some(0.07),
            revenue_growth: Some(0.2),
            earnings_growth: Some(0.25),
            profit_margin: Some(0.22),
            roe: Some(0.24),
            debt_to_equity: Some(0.3),
            ..stock()
        };
        let s = score(&f);
        assert!(s.overall.unwrap() > 80.0, "{s:?}");
        assert_eq!(assess(&row(0.05), Some(&f), &s).0, Signal::Opportunity);
    }

    #[test]
    fn expensive_shrinking_stock_needs_review() {
        let f = Fundamentals {
            pe_ttm: Some(80.0),
            peg: Some(4.0),
            fcf_yield: Some(-0.02),
            revenue_growth: Some(-0.08),
            profit_margin: Some(0.01),
            debt_to_equity: Some(3.0),
            ..stock()
        };
        let s = score(&f);
        let (sig, reasons) = assess(&row(0.05), Some(&f), &s);
        assert_eq!(sig, Signal::Review);
        assert!(reasons.iter().any(|r| r.contains("Revenue shrinking")));
    }

    #[test]
    fn missing_data_is_no_data_not_a_guess() {
        let s = score(&stock());
        assert_eq!(s.overall, None);
        assert_eq!(assess(&row(0.05), Some(&stock()), &s).0, Signal::NoData);
        assert_eq!(assess(&row(0.05), None, &s).0, Signal::NoData);
    }

    #[test]
    fn oversized_position_is_reviewed_regardless() {
        let f = Fundamentals {
            pe_ttm: Some(10.0),
            revenue_growth: Some(0.3),
            ..stock()
        };
        let s = score(&f);
        assert_eq!(assess(&row(0.3), Some(&f), &s).0, Signal::Review);
    }

    #[test]
    fn etfs_scored_on_cost_and_income() {
        let f = Fundamentals {
            kind: "etf".into(),
            expense_ratio: Some(0.0007),
            dividend_yield: Some(0.03),
            ..Default::default()
        };
        let s = score(&f);
        assert!(s.quality.unwrap() > 90.0);
        assert!(s.growth.is_none());
    }
}
