//! Built-in **sample portfolio** for demos, screenshots and UI development.
//!
//! Enable with `AURA_DEMO=1`. In demo mode the portfolio service never
//! touches the network, the keychain or the on-disk cache, and the UI shows
//! a "DEMO DATA" badge. The data is synthetic and deterministic (dates are
//! relative to "now" so the projected-dividend calendar stays populated).

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};

use super::store::{PortfolioCache, Snapshot};
use crate::t212::models::*;

/// `true` if `AURA_DEMO` is set to anything but `0`/empty.
pub fn enabled() -> bool {
    std::env::var("AURA_DEMO").is_ok_and(|v| !v.is_empty() && v != "0")
}

struct Holding {
    ticker: &'static str,
    name: &'static str,
    isin: &'static str,
    quote: &'static str,
    qty: f64,
    avg: f64,
    price: f64,
    /// Gross dividend per share per payment (quote currency), 0 = none.
    div_ps: f64,
    /// Months between payments.
    every_months: u32,
    /// Day-of-month the dividend is paid.
    pay_day: u32,
    /// First payment month (1-12) in the yearly pattern.
    first_month: u32,
    /// Withholding tax rate applied to the gross dividend.
    wht: f64,
}

const HOLDINGS: &[Holding] = &[
    Holding {
        ticker: "VUSAl_EQ",
        name: "Vanguard S&P 500 UCITS ETF",
        isin: "IE00B3XXRP09",
        quote: "GBX",
        qty: 120.0,
        avg: 7_600.0,
        price: 9_150.0,
        div_ps: 22.0,
        every_months: 3,
        pay_day: 24,
        first_month: 3,
        wht: 0.0,
    },
    Holding {
        ticker: "AAPL_US_EQ",
        name: "Apple",
        isin: "US0378331005",
        quote: "USD",
        qty: 25.0,
        avg: 165.0,
        price: 228.4,
        div_ps: 0.25,
        every_months: 3,
        pay_day: 14,
        first_month: 2,
        wht: 0.15,
    },
    Holding {
        ticker: "MSFT_US_EQ",
        name: "Microsoft",
        isin: "US5949181045",
        quote: "USD",
        qty: 12.0,
        avg: 310.0,
        price: 418.2,
        div_ps: 0.83,
        every_months: 3,
        pay_day: 12,
        first_month: 3,
        wht: 0.15,
    },
    Holding {
        ticker: "NVDA_US_EQ",
        name: "NVIDIA",
        isin: "US67066G1040",
        quote: "USD",
        qty: 30.0,
        avg: 45.0,
        price: 138.6,
        div_ps: 0.01,
        every_months: 3,
        pay_day: 27,
        first_month: 3,
        wht: 0.15,
    },
    Holding {
        ticker: "VHYLl_EQ",
        name: "Vanguard FTSE All-World High Dividend Yield",
        isin: "IE00B8GKDB10",
        quote: "GBX",
        qty: 150.0,
        avg: 5_900.0,
        price: 6_450.0,
        div_ps: 52.0,
        every_months: 3,
        pay_day: 26,
        first_month: 3,
        wht: 0.0,
    },
    Holding {
        ticker: "ULVRl_EQ",
        name: "Unilever",
        isin: "GB00B10RZP78",
        quote: "GBX",
        qty: 60.0,
        avg: 4_100.0,
        price: 4_650.0,
        div_ps: 37.0,
        every_months: 3,
        pay_day: 6,
        first_month: 3,
        wht: 0.0,
    },
    Holding {
        ticker: "KO_US_EQ",
        name: "Coca-Cola",
        isin: "US1912161007",
        quote: "USD",
        qty: 40.0,
        avg: 58.0,
        price: 63.1,
        div_ps: 0.485,
        every_months: 3,
        pay_day: 1,
        first_month: 1,
        wht: 0.15,
    },
    Holding {
        ticker: "O_US_EQ",
        name: "Realty Income",
        isin: "US7561091049",
        quote: "USD",
        qty: 35.0,
        avg: 57.0,
        price: 55.2,
        div_ps: 0.2635,
        every_months: 1,
        pay_day: 15,
        first_month: 1,
        wht: 0.15,
    },
    Holding {
        ticker: "LGENl_EQ",
        name: "Legal & General Group",
        isin: "GB0005603997",
        quote: "GBX",
        qty: 900.0,
        avg: 245.0,
        price: 236.8,
        div_ps: 6.5,
        every_months: 6,
        pay_day: 6,
        first_month: 6,
        wht: 0.0,
    },
    Holding {
        ticker: "ASML_NA_EQ",
        name: "ASML Holding",
        isin: "NL0010273215",
        quote: "EUR",
        qty: 4.0,
        avg: 700.0,
        price: 689.5,
        div_ps: 1.52,
        every_months: 3,
        pay_day: 8,
        first_month: 2,
        wht: 0.15,
    },
];

/// Account-per-quote FX (current, and at purchase for the FX-impact split).
fn fx(quote: &str) -> (f64, f64) {
    match quote {
        "USD" => (0.79, 0.77),
        "EUR" => (0.85, 0.86),
        "GBX" => (0.01, 0.01),
        _ => (1.0, 1.0),
    }
}

/// Builds the full sample cache as of `now`.
pub fn cache(now: DateTime<Utc>) -> PortfolioCache {
    let instrument = |h: &Holding| Instrument {
        ticker: h.ticker.into(),
        name: h.name.into(),
        isin: h.isin.into(),
        currency: h.quote.into(),
    };

    let positions: Vec<Position> = HOLDINGS
        .iter()
        .map(|h| {
            let (fx_now, fx_then) = fx(h.quote);
            let value = h.qty * h.price * fx_now;
            let cost = h.qty * h.avg * fx_then;
            let fx_impact = (h.quote != "GBX").then_some(h.qty * h.price * (fx_now - fx_then));
            Position {
                instrument: instrument(h),
                quantity: h.qty,
                quantity_available_for_trading: h.qty,
                quantity_in_pies: 0.0,
                average_price_paid: h.avg,
                current_price: h.price,
                created_at: Some(now - Duration::days(1_000)),
                wallet_impact: PositionWalletImpact {
                    currency: "GBP".into(),
                    current_value: round2(value),
                    total_cost: round2(cost),
                    unrealized_profit_loss: round2(value - cost),
                    fx_impact: fx_impact.map(round2),
                },
            }
        })
        .collect();

    // Dividends: three years of history on each holding's schedule.
    let start = now - Duration::days(3 * 365);
    let mut dividends = Vec::new();
    for h in HOLDINGS.iter().filter(|h| h.div_ps > 0.0) {
        let (fx_now, _) = fx(h.quote);
        let mut year = start.year();
        let mut month = h.first_month;
        while let Some(date) = NaiveDate::from_ymd_opt(year, month, h.pay_day.min(28)) {
            // Real payments land on business days; pull weekends back to Friday.
            let date = match date.weekday() {
                chrono::Weekday::Sat => date - Duration::days(1),
                chrono::Weekday::Sun => date - Duration::days(2),
                _ => date,
            };
            let paid = Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).expect("valid"));
            if paid > now {
                break;
            }
            if paid >= start {
                let gross = h.div_ps * h.qty * fx_now;
                dividends.push(Dividend {
                    reference: format!("demo-{}-{}", h.ticker, date),
                    ticker: h.ticker.into(),
                    instrument: instrument(h),
                    amount: round2(gross * (1.0 - h.wht)),
                    amount_in_euro: round2(gross * (1.0 - h.wht) * 1.17),
                    currency: "GBP".into(),
                    gross_amount_per_share: h.div_ps,
                    quantity: h.qty,
                    ticker_currency: h.quote.into(),
                    paid_on: Some(paid),
                    kind: if h.quote == "USD" {
                        "DIVIDENDS_PAID_BY_US_CORPORATIONS".into()
                    } else {
                        "ORDINARY".into()
                    },
                });
            }
            month += h.every_months;
            while month > 12 {
                month -= 12;
                year += 1;
            }
        }
    }
    dividends.sort_by_key(|d| std::cmp::Reverse(d.paid_on));

    // Totals.
    let invested: f64 = positions
        .iter()
        .map(|p| p.wallet_impact.current_value)
        .sum();
    let cost: f64 = positions.iter().map(|p| p.wallet_impact.total_cost).sum();
    let cash = 1_284.37;
    let realized = 642.18;
    let summary = AccountSummary {
        id: 1,
        currency: "GBP".into(),
        total_value: round2(invested + cash),
        cash: Cash {
            available_to_trade: cash,
            in_pies: 0.0,
            reserved_for_orders: 0.0,
        },
        investments: Investments {
            current_value: round2(invested),
            total_cost: round2(cost),
            realized_profit_loss: realized,
            unrealized_profit_loss: round2(invested - cost),
        },
    };

    // Cash flows: a lump sum, monthly top-ups and one withdrawal, sized so
    // net deposits sit ~16% below today's value.
    let months = 36;
    let monthly = 450.0;
    let withdrawal = 1_500.0;
    let target_net = summary.total_value / 1.16;
    let lump = target_net - monthly * months as f64 + withdrawal;
    let mut transactions = vec![tx("demo-dep-0", lump, "DEPOSIT", start - Duration::days(5))];
    for i in 0..months {
        transactions.push(tx(
            &format!("demo-dep-{}", i + 1),
            monthly,
            "DEPOSIT",
            start + Duration::days(30 * i + 2),
        ));
    }
    transactions.push(tx(
        "demo-wd-1",
        -withdrawal,
        "WITHDRAW",
        now - Duration::days(200),
    ));
    transactions.push(tx(
        "demo-int-1",
        3.71,
        "INTEREST_ON_FREE_CASH",
        now - Duration::days(4),
    ));
    transactions.sort_by_key(|t| std::cmp::Reverse(t.date_time));

    // A year of daily value snapshots: drift up to today with some wobble.
    let days = 365;
    let snapshots = (0..=days)
        .map(|i| {
            let t = i as f64 / days as f64;
            let wobble =
                (t * 23.0).sin() * 0.025 + (t * 61.0).sin() * 0.012 - (t * 7.0).cos() * 0.015;
            let factor = 0.80 + 0.20 * t + wobble * (1.0 - t * 0.6);
            let value = summary.total_value * if i == days { 1.0 } else { factor };
            Snapshot {
                at: now - Duration::days(days - i),
                total_value: round2(value),
                invested: round2(cost * (0.86 + 0.14 * t)),
                cash,
            }
        })
        .collect();

    PortfolioCache {
        version: 1,
        account_id: Some(1),
        summary: Some(summary),
        positions,
        dividends,
        transactions,
        snapshots,
        synced_at: Some(now),
        history_synced_at: Some(now),
    }
}

fn tx(reference: &str, amount: f64, kind: &str, at: DateTime<Utc>) -> Transaction {
    Transaction {
        reference: reference.into(),
        amount: round2(amount),
        currency: "GBP".into(),
        date_time: Some(at),
        kind: kind.into(),
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::analytics;

    #[test]
    fn demo_data_produces_full_analytics() {
        let now = Utc.with_ymd_and_hms(2026, 11, 5, 12, 0, 0).unwrap();
        let c = cache(now);
        let s = c.summary.as_ref().unwrap();
        let a = analytics::compute(s, &c.positions, &c.dividends, &c.transactions, now);
        assert_eq!(a.holdings.len(), HOLDINGS.len());
        assert!(a.total_return_from_deposits && a.total_return > 0.0);
        // Mixed US (15%) and UK/IE (0%) payers → a rate strictly in between.
        let rate = a.dividends.tax.rate.unwrap();
        assert!(rate > 0.01 && rate < 0.15, "{rate}");
        assert!(!a.upcoming.is_empty());
        assert!(a.dividends.by_month.iter().filter(|m| m.2 > 0.0).count() > 20);
    }
}
