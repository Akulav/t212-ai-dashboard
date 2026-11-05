//! Pure portfolio analytics (no I/O, fully unit-tested).
//!
//! The Trading212 API gives us raw positions, dividend payments and cash
//! movements. Several dashboard metrics are **derived** here:
//!
//! * **Total return**: `totalValue − net deposits` when the deposit history
//!   is available (captures realised P/L, dividends, interest and fees);
//!   otherwise realised + unrealised P/L.
//! * **Average tax withheld**: the API reports the *net* `amount` credited
//!   and the *gross* `grossAmountPerShare`, so
//!   `rate = 1 − net / (gross × quantity × fx)`. FX is exact when the quote
//!   and account currency match (incl. GBX→GBP); otherwise it is estimated
//!   from the position's current price vs. wallet value, and the result is
//!   flagged as an estimate.
//! * **Upcoming dividends**: the API has no calendar, so payments are
//!   *projected* from each holding's historical cadence and current share
//!   count. These are estimates and are labelled as such in the UI.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc, Weekday};

use crate::t212::models::{AccountSummary, Dividend, Position, Transaction};

/// One row of the holdings table (all money in account currency unless noted).
#[derive(Debug, Clone, PartialEq)]
pub struct HoldingRow {
    /// Trading212 ticker.
    pub ticker: String,
    /// Display name.
    pub name: String,
    /// ISIN.
    pub isin: String,
    /// Quote currency of prices.
    pub quote_currency: String,
    /// Shares held.
    pub quantity: f64,
    /// Average price paid, quote currency.
    pub avg_price: f64,
    /// Last price, quote currency.
    pub current_price: f64,
    /// Market value.
    pub value: f64,
    /// Cost basis.
    pub cost: f64,
    /// Unrealised P/L.
    pub pnl: f64,
    /// Unrealised P/L as a fraction of cost.
    pub pnl_pct: Option<f64>,
    /// FX component of P/L.
    pub fx_impact: Option<f64>,
    /// Share of total invested value (0..1).
    pub weight: f64,
    /// Lifetime dividends received from this holding (net).
    pub dividends_received: f64,
    /// Trailing-12-month net dividends ÷ cost.
    pub yield_on_cost: Option<f64>,
}

/// Effective dividend withholding tax.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TaxEstimate {
    /// Weighted average rate (0..1) over measurable payments.
    pub rate: Option<f64>,
    /// Total tax withheld (account currency).
    pub withheld: f64,
    /// Payments that could be measured.
    pub measured: usize,
    /// `true` if any payment needed an estimated FX rate.
    pub fx_estimated: bool,
}

/// Dividend aggregates.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DividendStats {
    /// Lifetime net dividends.
    pub total: f64,
    /// Net dividends in the trailing 12 months.
    pub last_12m: f64,
    /// Net dividends since 1 January.
    pub ytd: f64,
    /// Number of payments.
    pub payments: usize,
    /// Net income per calendar month for the last 24 months (oldest first).
    pub by_month: Vec<(i32, u32, f64)>,
    /// `(ticker, name, total)` sorted by total, descending.
    pub by_ticker: Vec<(String, String, f64)>,
    /// Withholding tax estimate.
    pub tax: TaxEstimate,
}

/// Payment frequency inferred from history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cadence {
    /// ~30 days.
    Monthly,
    /// ~91 days.
    Quarterly,
    /// ~182 days.
    SemiAnnual,
    /// ~365 days.
    Annual,
}

impl Cadence {
    /// Typical interval in days.
    pub fn days(self) -> i64 {
        match self {
            Self::Monthly => 30,
            Self::Quarterly => 91,
            Self::SemiAnnual => 182,
            Self::Annual => 365,
        }
    }

    /// UI label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Monthly => "monthly",
            Self::Quarterly => "quarterly",
            Self::SemiAnnual => "semi-annual",
            Self::Annual => "annual",
        }
    }

    fn from_median_days(days: i64) -> Option<Self> {
        match days {
            20..=45 => Some(Self::Monthly),
            46..=135 => Some(Self::Quarterly),
            136..=270 => Some(Self::SemiAnnual),
            271..=400 => Some(Self::Annual),
            _ => None,
        }
    }
}

/// A projected (not announced) future payment.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedDividend {
    /// Ticker.
    pub ticker: String,
    /// Name.
    pub name: String,
    /// Expected payment date.
    pub expected_on: NaiveDate,
    /// Expected net amount, account currency.
    pub amount: f64,
    /// Inferred cadence.
    pub cadence: Cadence,
}

/// Everything the UI renders, computed once per sync.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Analytics {
    /// Account currency.
    pub currency: String,
    /// Holdings sorted by value, descending.
    pub holdings: Vec<HoldingRow>,
    /// Dividend aggregates.
    pub dividends: DividendStats,
    /// Projected payments in the next 120 days, soonest first.
    pub upcoming: Vec<ProjectedDividend>,
    /// Cumulative net deposits over time (oldest first).
    pub net_deposits: Vec<(DateTime<Utc>, f64)>,
    /// Total return (see module docs).
    pub total_return: f64,
    /// Total return as a fraction of the capital base.
    pub total_return_pct: Option<f64>,
    /// `true` if total return is based on deposit history.
    pub total_return_from_deposits: bool,
}

/// Account-per-quote currency rate if it is known exactly.
fn exact_fx(quote: &str, account: &str) -> Option<f64> {
    if quote.eq_ignore_ascii_case(account) {
        Some(1.0)
    } else if quote == "GBX" && account == "GBP" {
        Some(0.01)
    } else {
        None
    }
}

/// Current account-per-quote rate implied by a position's wallet value.
fn implied_fx(p: &Position) -> Option<f64> {
    let quote_value = p.current_price * p.quantity;
    (quote_value > 0.0 && p.wallet_impact.current_value > 0.0)
        .then(|| p.wallet_impact.current_value / quote_value)
}

/// Computes all analytics. `now` is injected for testability.
pub fn compute(
    summary: &AccountSummary,
    positions: &[Position],
    dividends: &[Dividend],
    transactions: &[Transaction],
    now: DateTime<Utc>,
) -> Analytics {
    let currency = summary.currency.clone();
    let by_ticker_pos: HashMap<&str, &Position> = positions
        .iter()
        .map(|p| (p.instrument.ticker.as_str(), p))
        .collect();
    let fx_for = |ticker: &str, quote: &str| -> Option<(f64, bool)> {
        exact_fx(quote, &currency)
            .map(|fx| (fx, false))
            .or_else(|| {
                by_ticker_pos
                    .get(ticker)
                    .and_then(|p| implied_fx(p))
                    .map(|fx| (fx, true))
            })
    };

    // ---- Dividends ----
    let year_ago = now - Duration::days(365);
    let mut stats = DividendStats {
        payments: dividends.len(),
        ..Default::default()
    };
    let mut per_ticker_total: HashMap<&str, (String, f64)> = HashMap::new();
    let mut per_ticker_12m: HashMap<&str, f64> = HashMap::new();
    let mut per_ticker_tax: HashMap<&str, (f64, f64)> = HashMap::new(); // (withheld, gross)
    let mut gross_sum = 0.0;
    let mut months: BTreeMap<(i32, u32), f64> = BTreeMap::new();

    for d in dividends {
        stats.total += d.amount;
        let entry = per_ticker_total
            .entry(&d.ticker)
            .or_insert_with(|| (d.instrument.name.clone(), 0.0));
        entry.1 += d.amount;
        if let Some(paid) = d.paid_on {
            if paid >= year_ago {
                stats.last_12m += d.amount;
                *per_ticker_12m.entry(&d.ticker).or_default() += d.amount;
            }
            if paid.year() == now.year() {
                stats.ytd += d.amount;
            }
            *months.entry((paid.year(), paid.month())).or_default() += d.amount;
        }
        if let Some((fx, estimated)) = fx_for(&d.ticker, &d.ticker_currency) {
            let gross = d.gross_amount_per_share * d.quantity * fx;
            if gross > 0.0 {
                let rate = 1.0 - d.amount / gross;
                if (-0.05..=0.6).contains(&rate) {
                    let withheld = (gross - d.amount).max(0.0);
                    stats.tax.withheld += withheld;
                    stats.tax.measured += 1;
                    stats.tax.fx_estimated |= estimated;
                    gross_sum += gross;
                    let t = per_ticker_tax.entry(&d.ticker).or_default();
                    t.0 += withheld;
                    t.1 += gross;
                }
            }
        }
    }
    stats.tax.rate = (gross_sum > 0.0).then(|| stats.tax.withheld / gross_sum);
    stats.by_month = last_n_months(now, 24)
        .into_iter()
        .map(|(y, m)| (y, m, *months.get(&(y, m)).unwrap_or(&0.0)))
        .collect();
    stats.by_ticker = per_ticker_total
        .iter()
        .map(|(t, (n, v))| (t.to_string(), n.clone(), *v))
        .collect();
    stats.by_ticker.sort_by(|a, b| b.2.total_cmp(&a.2));

    // ---- Holdings ----
    let invested: f64 = positions
        .iter()
        .map(|p| p.wallet_impact.current_value)
        .sum();
    let mut holdings: Vec<HoldingRow> = positions
        .iter()
        .map(|p| {
            let w = &p.wallet_impact;
            let ticker = p.instrument.ticker.as_str();
            let d12 = per_ticker_12m.get(ticker).copied().unwrap_or(0.0);
            HoldingRow {
                ticker: ticker.to_string(),
                name: p.instrument.name.clone(),
                isin: p.instrument.isin.clone(),
                quote_currency: p.instrument.currency.clone(),
                quantity: p.quantity,
                avg_price: p.average_price_paid,
                current_price: p.current_price,
                value: w.current_value,
                cost: w.total_cost,
                pnl: w.unrealized_profit_loss,
                pnl_pct: (w.total_cost > 0.0).then(|| w.unrealized_profit_loss / w.total_cost),
                fx_impact: w.fx_impact,
                weight: if invested > 0.0 {
                    w.current_value / invested
                } else {
                    0.0
                },
                dividends_received: per_ticker_total.get(ticker).map_or(0.0, |t| t.1),
                yield_on_cost: (w.total_cost > 0.0 && d12 > 0.0).then(|| d12 / w.total_cost),
            }
        })
        .collect();
    holdings.sort_by(|a, b| b.value.total_cmp(&a.value));

    // ---- Projections ----
    let overall_rate = stats.tax.rate.unwrap_or(0.0);
    let today = now.date_naive();
    let horizon = today + Duration::days(120);
    let mut grouped: HashMap<&str, Vec<&Dividend>> = HashMap::new();
    for d in dividends.iter().filter(|d| d.paid_on.is_some()) {
        grouped.entry(&d.ticker).or_default().push(d);
    }
    let mut upcoming = Vec::new();
    for (ticker, mut pays) in grouped {
        let Some(pos) = by_ticker_pos.get(ticker) else {
            continue;
        };
        pays.sort_by_key(|d| d.paid_on);
        let mut dates: Vec<NaiveDate> = pays
            .iter()
            .filter_map(|d| d.paid_on.map(|t| t.date_naive()))
            .collect();
        dates.dedup();
        if dates.len() < 2 {
            continue;
        }
        let mut gaps: Vec<i64> = dates.windows(2).map(|w| (w[1] - w[0]).num_days()).collect();
        gaps.sort_unstable();
        let Some(cadence) = Cadence::from_median_days(gaps[gaps.len() / 2]) else {
            continue;
        };
        let last = *dates.last().expect("len >= 2");
        let step = Duration::days(cadence.days());
        if last + step * 2 < today {
            continue; // missed two payments: probably suspended
        }
        let mut next = last + step;
        while next < today {
            next += step;
        }
        // Payments settle on business days: roll weekend projections to Monday.
        while matches!(next.weekday(), Weekday::Sat | Weekday::Sun) {
            next += Duration::days(1);
        }
        if next > horizon {
            continue;
        }
        let latest = pays.last().expect("non-empty");
        let Some((fx, _)) = fx_for(ticker, &latest.ticker_currency) else {
            continue;
        };
        let rate = per_ticker_tax
            .get(ticker)
            .filter(|t| t.1 > 0.0)
            .map_or(overall_rate, |t| t.0 / t.1);
        upcoming.push(ProjectedDividend {
            ticker: ticker.to_string(),
            name: pos.instrument.name.clone(),
            expected_on: next,
            amount: latest.gross_amount_per_share * pos.quantity * fx * (1.0 - rate),
            cadence,
        });
    }
    upcoming.sort_by_key(|u| u.expected_on);

    // ---- Deposits & total return ----
    let mut flows: Vec<&Transaction> = transactions
        .iter()
        .filter(|t| {
            t.date_time.is_some() && matches!(t.kind.as_str(), "DEPOSIT" | "WITHDRAW" | "TRANSFER")
        })
        .collect();
    flows.sort_by_key(|t| t.date_time);
    let mut running = 0.0;
    let net_deposits: Vec<(DateTime<Utc>, f64)> = flows
        .iter()
        .map(|t| {
            running += match t.kind.as_str() {
                "DEPOSIT" => t.amount.abs(),
                "WITHDRAW" => -t.amount.abs(),
                _ => t.amount,
            };
            (t.date_time.expect("filtered"), running)
        })
        .collect();

    let (total_return, total_return_pct, from_deposits) = match net_deposits.last() {
        Some(&(_, deposited)) if deposited > 0.0 => {
            let r = summary.total_value - deposited;
            (r, Some(r / deposited), true)
        }
        _ => {
            let inv = &summary.investments;
            let r = inv.unrealized_profit_loss + inv.realized_profit_loss;
            (r, (inv.total_cost > 0.0).then(|| r / inv.total_cost), false)
        }
    };

    Analytics {
        currency,
        holdings,
        dividends: stats,
        upcoming,
        net_deposits,
        total_return,
        total_return_pct,
        total_return_from_deposits: from_deposits,
    }
}

/// The last `n` calendar months ending with `now`'s month, oldest first.
fn last_n_months(now: DateTime<Utc>, n: u32) -> Vec<(i32, u32)> {
    let (mut y, mut m) = (now.year(), now.month());
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        out.push((y, m));
        if m == 1 {
            y -= 1;
            m = 12;
        } else {
            m -= 1;
        }
    }
    out.reverse();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::t212::models::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 5, 12, 0, 0).unwrap()
    }

    fn position(
        ticker: &str,
        quote: &str,
        qty: f64,
        price: f64,
        value: f64,
        cost: f64,
    ) -> Position {
        Position {
            instrument: Instrument {
                ticker: ticker.into(),
                name: ticker.into(),
                isin: String::new(),
                currency: quote.into(),
            },
            quantity: qty,
            current_price: price,
            average_price_paid: cost / qty,
            wallet_impact: PositionWalletImpact {
                currency: "GBP".into(),
                current_value: value,
                total_cost: cost,
                unrealized_profit_loss: value - cost,
                fx_impact: None,
            },
            ..Default::default()
        }
    }

    fn dividend(
        ticker: &str,
        quote: &str,
        date: (i32, u32, u32),
        qty: f64,
        gross_ps: f64,
        net: f64,
    ) -> Dividend {
        Dividend {
            reference: format!("{ticker}-{date:?}"),
            ticker: ticker.into(),
            instrument: Instrument {
                ticker: ticker.into(),
                name: ticker.into(),
                ..Default::default()
            },
            amount: net,
            currency: "GBP".into(),
            gross_amount_per_share: gross_ps,
            quantity: qty,
            ticker_currency: quote.into(),
            paid_on: Some(
                Utc.with_ymd_and_hms(date.0, date.1, date.2, 0, 0, 0)
                    .unwrap(),
            ),
            kind: "ORDINARY".into(),
            ..Default::default()
        }
    }

    fn summary(total: f64) -> AccountSummary {
        AccountSummary {
            currency: "GBP".into(),
            total_value: total,
            ..Default::default()
        }
    }

    #[test]
    fn tax_rate_exact_for_gbx() {
        // 100 shares × 50p gross = £50 gross, £50 net → 0% (UK dividends).
        let divs = [dividend("VOD_EQ", "GBX", (2026, 8, 1), 100.0, 50.0, 50.0)];
        let a = compute(&summary(0.0), &[], &divs, &[], now());
        assert_eq!(a.dividends.tax.rate, Some(0.0));
        assert!(!a.dividends.tax.fx_estimated);
    }

    #[test]
    fn tax_rate_estimated_via_position_fx() {
        // USD stock, GBP account. Position implies 0.8 GBP/USD.
        let pos = position("AAPL_US_EQ", "USD", 10.0, 200.0, 1600.0, 1000.0);
        // gross $1 × 10 = $10 = £8; net £6.80 → 15% withheld.
        let divs = [dividend("AAPL_US_EQ", "USD", (2026, 8, 1), 10.0, 1.0, 6.8)];
        let a = compute(&summary(1600.0), &[pos], &divs, &[], now());
        let rate = a.dividends.tax.rate.unwrap();
        assert!((rate - 0.15).abs() < 1e-9, "{rate}");
        assert!(a.dividends.tax.fx_estimated);
    }

    #[test]
    fn projects_quarterly_dividend() {
        let pos = position("KO_US_EQ", "USD", 20.0, 50.0, 800.0, 700.0);
        let divs = [
            dividend("KO_US_EQ", "USD", (2026, 4, 1), 20.0, 0.5, 6.8),
            dividend("KO_US_EQ", "USD", (2026, 7, 1), 20.0, 0.5, 6.8),
            dividend("KO_US_EQ", "USD", (2026, 10, 1), 20.0, 0.5, 6.8),
        ];
        let a = compute(&summary(800.0), &[pos], &divs, &[], now());
        let next = &a.upcoming[0];
        assert_eq!(next.cadence, Cadence::Quarterly);
        assert_eq!(
            next.expected_on,
            NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()
        );
        assert!((next.amount - 6.8).abs() < 1e-9);
    }

    #[test]
    fn sold_positions_are_not_projected() {
        let divs = [
            dividend("X_EQ", "GBP", (2026, 6, 1), 1.0, 1.0, 1.0),
            dividend("X_EQ", "GBP", (2026, 9, 1), 1.0, 1.0, 1.0),
        ];
        assert!(
            compute(&summary(0.0), &[], &divs, &[], now())
                .upcoming
                .is_empty()
        );
    }

    #[test]
    fn total_return_uses_net_deposits() {
        let tx = |kind: &str, amount: f64, m: u32| Transaction {
            reference: format!("{kind}{m}"),
            amount,
            currency: "GBP".into(),
            date_time: Some(Utc.with_ymd_and_hms(2026, m, 1, 0, 0, 0).unwrap()),
            kind: kind.into(),
        };
        let txs = [
            tx("DEPOSIT", 1000.0, 1),
            tx("DEPOSIT", 500.0, 2),
            tx("WITHDRAW", -200.0, 3),
            tx("FEE", -1.0, 3),
        ];
        let a = compute(&summary(1430.0), &[], &[], &txs, now());
        assert!(a.total_return_from_deposits);
        assert!((a.total_return - 130.0).abs() < 1e-9);
        assert_eq!(a.net_deposits.last().unwrap().1, 1300.0);
    }

    #[test]
    fn monthly_buckets_cover_24_months() {
        let divs = [dividend("A", "GBP", (2026, 11, 1), 1.0, 2.0, 2.0)];
        let a = compute(&summary(0.0), &[], &divs, &[], now());
        assert_eq!(a.dividends.by_month.len(), 24);
        assert_eq!(a.dividends.by_month.last().unwrap(), &(2026, 11, 2.0));
        assert_eq!(a.dividends.by_month[0].0, 2024);
        assert_eq!(a.dividends.ytd, 2.0);
    }
}
