//! ESEF / UKSEF fundamentals provider (companies listed in the EU and the UK, and other ESEF filers).
//!
//! EU-listed companies must publish their annual financial reports as IFRS XBRL (ESEF); UK companies file the same
//! way (UKSEF). XBRL International collects them at [filings.xbrl.org](https://filings.xbrl.org). All three sources
//! below are free and need no key or account:
//!
//! * `api.gleif.org/api/v1/lei-records?filter[isin]=…`: the holding's ISIN → the company's LEI
//! * `filings.xbrl.org/api/entities/{LEI}/filings`: the company's reports; the newest is used
//! * the report's xBRL-JSON (`json_url`): IFRS facts for the latest year and the year before
//! * `api.frankfurter.app`: ECB exchange rates, when the report's currency differs from the quote currency
//!
//! Reports are annual, so "TTM" here is the latest financial year, growth compares it with the year before, and there
//! are no quarters. Values are converted into the quote currency (`GBX` = pence). US listings are left to
//! [`super::sec`].

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use chrono::{Duration as Days, NaiveDate, NaiveDateTime};
use reqwest::Client;
use serde::Deserialize;

use super::fundamentals::{Fundamentals, Lookup};
use super::sec::Fact;

const GLEIF: &str = "https://api.gleif.org/api/v1/lei-records";
const FILINGS: &str = "https://filings.xbrl.org";
const FX: &str = "https://api.frankfurter.app/latest";

/// One fact of an xBRL-JSON report.
#[derive(Debug, Deserialize)]
struct ReportFact {
    #[serde(default)]
    value: serde_json::Value,
    #[serde(default)]
    dimensions: HashMap<String, String>,
}

/// The parts of an xBRL-JSON report Aura uses.
#[derive(Debug, Deserialize)]
pub struct Report {
    #[serde(default)]
    facts: HashMap<String, ReportFact>,
}

/// Plain (undimensioned) numeric facts of a report, by concept and unit.
#[derive(Debug, Default)]
pub struct Facts {
    by_concept: HashMap<String, HashMap<String, Vec<Fact>>>,
    /// The reporting currency (`EUR`, `GBP`, …).
    pub currency: Option<String>,
}

/// xBRL-JSON periods: `start/end` for durations, one date-time for instants. Both use the exclusive end
/// (`…/2026-01-01T00:00:00` ends on 31 December 2025).
fn period(p: &str) -> Option<(Option<NaiveDate>, NaiveDate)> {
    let date = |s: &str| -> Option<NaiveDate> {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
            .map(|d| d.date())
            .or_else(|_| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
            .ok()
    };
    match p.split_once('/') {
        Some((a, b)) => Some((Some(date(a)?), date(b)? - Days::days(1))),
        None => Some((None, date(p)? - Days::days(1))),
    }
}

impl Facts {
    /// Collects the facts that have no dimensions besides concept, entity, period and unit (company totals).
    pub fn from_report(report: &Report, filed: NaiveDate) -> Self {
        let mut out = Facts::default();
        for f in report.facts.values() {
            let d = &f.dimensions;
            if d.keys()
                .any(|k| !matches!(k.as_str(), "concept" | "entity" | "period" | "unit"))
            {
                continue;
            }
            let (Some(concept), Some(unit), Some(p)) =
                (d.get("concept"), d.get("unit"), d.get("period"))
            else {
                continue;
            };
            let val = match &f.value {
                serde_json::Value::Number(n) => n.as_f64(),
                serde_json::Value::String(s) => s.trim().parse().ok(),
                _ => None,
            };
            let (Some(val), Some((start, end))) = (val, period(p)) else {
                continue;
            };
            let unit = unit.replace("iso4217:", "").replace("xbrli:", "");
            if out.currency.is_none()
                && unit.len() == 3
                && unit.chars().all(|c| c.is_ascii_uppercase())
            {
                out.currency = Some(unit.clone());
            }
            let list = out
                .by_concept
                .entry(concept.clone())
                .or_default()
                .entry(unit)
                .or_default();
            if !list.iter().any(|x| x.start == start && x.end == end) {
                list.push(Fact {
                    start,
                    end,
                    val,
                    form: "ESEF".into(),
                    filed,
                });
            }
        }
        out
    }

    /// Facts of the first concept (in order) that has any in a unit accepted by `unit_ok`, oldest first.
    fn series(&self, concepts: &[&str], unit_ok: impl Fn(&str) -> bool) -> Vec<Fact> {
        for c in concepts {
            if let Some(units) = self.by_concept.get(*c)
                && let Some((_, facts)) = units.iter().find(|(u, f)| unit_ok(u) && !f.is_empty())
            {
                let mut v = facts.clone();
                v.sort_by_key(|f| (f.end, f.start));
                return v;
            }
        }
        Vec::new()
    }

    fn money(&self, concepts: &[&str]) -> Vec<Fact> {
        let cur = self.currency.clone().unwrap_or_default();
        self.series(concepts, |u| u == cur)
    }

    fn per_share(&self, concepts: &[&str]) -> Vec<Fact> {
        let cur = self.currency.clone().unwrap_or_default();
        self.series(concepts, |u| u == format!("{cur}/shares"))
    }
}

/// The latest financial-year value and the one a year before it.
fn years(series: &[Fact]) -> (Option<f64>, Option<f64>) {
    let annual: Vec<&Fact> = series
        .iter()
        .filter(|f| {
            f.start
                .is_some_and(|s| (330..=400).contains(&(f.end - s).num_days()))
        })
        .collect();
    let Some(latest) = annual.iter().max_by_key(|f| f.end) else {
        return (None, None);
    };
    let prior = annual
        .iter()
        .find(|f| ((latest.end - f.end).num_days() - 365).abs() <= 20)
        .map(|f| f.val);
    (Some(latest.val), prior)
}

/// The latest balance-sheet value.
fn latest(series: &[Fact]) -> Option<f64> {
    series
        .iter()
        .filter(|f| f.start.is_none())
        .max_by_key(|f| f.end)
        .map(|f| f.val)
}

fn growth(now: Option<f64>, before: Option<f64>) -> Option<f64> {
    match (now, before) {
        (Some(n), Some(b)) if b > 0.0 => Some(n / b - 1.0),
        _ => None,
    }
}

const REVENUE: &[&str] = &[
    "ifrs-full:Revenue",
    "ifrs-full:RevenueFromContractsWithCustomers",
    "ifrs-full:RevenueFromSaleOfGoods",
    "ifrs-full:RevenueFromRenderingOfServices",
];
const NET_INCOME: &[&str] = &[
    "ifrs-full:ProfitLossAttributableToOwnersOfParent",
    "ifrs-full:ProfitLoss",
];
const EQUITY: &[&str] = &[
    "ifrs-full:EquityAttributableToOwnersOfParent",
    "ifrs-full:Equity",
];
const DEBT: &[&str] = &[
    "ifrs-full:NoncurrentPortionOfNoncurrentBorrowings",
    "ifrs-full:LongtermBorrowings",
    "ifrs-full:NoncurrentBorrowings",
    "ifrs-full:Borrowings",
];

/// Derives [`Fundamentals`] from a report's facts, the live price (quote currency) and `fx`, the number of quote
/// currency units per unit of the reporting currency (`None` when unknown: price-based ratios then stay empty).
pub fn derive(facts: &Facts, price: Option<f64>, fx: Option<f64>) -> Fundamentals {
    let (rev, rev_prev) = years(&facts.money(REVENUE));
    let (ni, ni_prev) = years(&facts.money(NET_INCOME));
    let (op, _) = years(&facts.money(&["ifrs-full:ProfitLossFromOperatingActivities"]));
    let (ocf, _) = years(&facts.money(&["ifrs-full:CashFlowsFromUsedInOperatingActivities"]));
    let (capex, _) = years(&facts.money(&[
        "ifrs-full:PurchaseOfPropertyPlantAndEquipmentClassifiedAsInvestingActivities",
        "ifrs-full:PurchaseOfPropertyPlantAndEquipment",
    ]));
    let (eps, _) = years(&facts.per_share(&[
        "ifrs-full:DilutedEarningsLossPerShare",
        "ifrs-full:BasicEarningsLossPerShare",
        "ifrs-full:DilutedEarningsLossPerShareFromContinuingOperations",
        "ifrs-full:BasicEarningsLossPerShareFromContinuingOperations",
    ]));
    let (dps, _) =
        years(&facts.per_share(&["ifrs-full:DividendsRecognisedAsDistributionsToOwnersPerShare"]));
    // Not every report tags the share count; profit ÷ EPS gives it closely enough for market cap.
    let (shares, _) = years(&facts.series(&["ifrs-full:WeightedAverageShares"], |u| u == "shares"));
    let shares = shares.or_else(|| ni.zip(eps).filter(|(_, e)| *e > 0.0).map(|(n, e)| n / e));
    let eq = latest(&facts.money(EQUITY));
    let debt = latest(&facts.money(DEBT));

    // (capex is reported as a positive outflow)
    let fcf = match (ocf, capex) {
        (Some(o), Some(c)) => Some(o - c.abs()),
        (Some(o), None) => Some(o),
        _ => None,
    };
    let price_fx = price.zip(fx);
    let eps_q = eps.zip(fx).map(|(e, r)| e * r);
    let market_cap = price
        .zip(shares)
        .map(|(p, s)| p * s)
        .filter(|_| fx.is_some());
    let market_cap_rep = price_fx.zip(shares).map(|((p, r), s)| p * s / r); // in the reporting currency
    let pe = price
        .zip(eps_q)
        .filter(|(_, e)| *e > 0.0)
        .map(|(p, e)| p / e);
    let earnings_growth = growth(ni, ni_prev);
    let dps_q = dps.zip(fx).map(|(d, r)| d * r);

    Fundamentals {
        kind: "stock".into(),
        market_cap,
        pe_ttm: pe,
        peg: pe
            .zip(earnings_growth)
            .filter(|(p, g)| *p > 0.0 && *g > 0.0)
            .map(|(p, g)| p / (g * 100.0)),
        price_to_book: market_cap_rep
            .zip(eq)
            .filter(|(_, e)| *e > 0.0)
            .map(|(m, e)| m / e),
        revenue_growth: growth(rev, rev_prev),
        earnings_growth,
        profit_margin: ni.zip(rev).filter(|(_, r)| *r > 0.0).map(|(n, r)| n / r),
        operating_margin: op.zip(rev).filter(|(_, r)| *r > 0.0).map(|(o, r)| o / r),
        roe: ni.zip(eq).filter(|(_, e)| *e > 0.0).map(|(n, e)| n / e),
        debt_to_equity: debt.zip(eq).filter(|(_, e)| *e > 0.0).map(|(d, e)| d / e),
        free_cash_flow: fcf.zip(fx).map(|(f, r)| f * r),
        fcf_yield: fcf
            .zip(market_cap_rep)
            .filter(|(_, m)| *m > 0.0)
            .map(|(f, m)| f / m),
        dividend_yield: dps_q
            .zip(price)
            .filter(|(_, p)| *p > 0.0)
            .map(|(d, p)| d / p),
        payout_ratio: dps.zip(eps).filter(|(_, e)| *e > 0.0).map(|(d, e)| d / e),
        source: "ESEF filings (filings.xbrl.org)".into(),
        ..Default::default()
    }
}

async fn get_json<T: serde::de::DeserializeOwned>(http: &Client, url: &str) -> Result<Option<T>> {
    let resp = http
        .get(url)
        .header(
            reqwest::header::ACCEPT,
            "application/json, application/vnd.api+json",
        )
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    Ok(Some(
        resp.error_for_status()?
            .json()
            .await
            .with_context(|| format!("reading {url}"))?,
    ))
}

#[derive(Deserialize)]
struct JsonApi<T> {
    #[serde(default = "Vec::new")]
    data: Vec<T>,
}

#[derive(Deserialize)]
struct LeiRecord {
    id: String,
}

#[derive(Deserialize)]
struct Filing {
    attributes: FilingAttributes,
}

#[derive(Deserialize)]
struct FilingAttributes {
    period_end: Option<NaiveDate>,
    json_url: Option<String>,
    date_added: Option<String>,
}

#[derive(Deserialize)]
struct Rates {
    rates: HashMap<String, f64>,
}

/// Quote-currency units per unit of `from` (`GBX` = pence). `None` if the rate is unknown.
pub async fn fx_rate(http: &Client, from: &str, quote: &str) -> Option<f64> {
    let (to, scale) = match quote {
        "GBX" | "GBp" => ("GBP", 100.0),
        q => (q, 1.0),
    };
    if from.eq_ignore_ascii_case(to) {
        return Some(scale);
    }
    let rates: Rates = get_json(http, &format!("{FX}?from={from}&to={to}"))
        .await
        .ok()
        .flatten()?;
    rates.rates.get(to).map(|r| r * scale)
}

/// Fetches and derives fundamentals for one holding listed outside the US. `Ok(None)` = not covered (no LEI, or the
/// company files no ESEF/UKSEF report).
pub async fn fetch(http: &Client, lookup: &Lookup) -> Result<Option<Fundamentals>> {
    let isin = lookup.isin.trim();
    if isin.len() != 12 || isin.starts_with("US") {
        return Ok(None);
    }
    let Some(leis) =
        get_json::<JsonApi<LeiRecord>>(http, &format!("{GLEIF}?filter[isin]={isin}")).await?
    else {
        return Ok(None);
    };
    let Some(lei) = leis.data.first().map(|r| r.id.clone()) else {
        return Ok(None);
    };
    let Some(filings) =
        get_json::<JsonApi<Filing>>(http, &format!("{FILINGS}/api/entities/{lei}/filings")).await?
    else {
        return Ok(None);
    };
    let Some(latest) = filings
        .data
        .into_iter()
        .filter(|f| f.attributes.json_url.is_some() && f.attributes.period_end.is_some())
        .max_by(|a, b| {
            (a.attributes.period_end, &a.attributes.date_added)
                .cmp(&(b.attributes.period_end, &b.attributes.date_added))
        })
    else {
        return Ok(None);
    };
    let url = format!(
        "{FILINGS}{}",
        latest.attributes.json_url.as_deref().unwrap_or_default()
    );
    let report: Report = get_json(http, &url)
        .await?
        .ok_or_else(|| anyhow!("the report listed for {lei} is missing at filings.xbrl.org"))?;
    let filed = latest.attributes.period_end.unwrap_or_default();
    let facts = Facts::from_report(&report, filed);
    let fx = match &facts.currency {
        Some(cur) => fx_rate(http, cur, &lookup.currency).await,
        None => None,
    };
    let mut f = derive(&facts, lookup.price, fx);
    f.symbol = lookup.name.clone();
    Ok(Some(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> Report {
        let fact = |concept: &str, unit: &str, period: &str, value: &str| {
            serde_json::json!({ "value": value, "dimensions": {
                "concept": concept, "entity": "scheme:LEI", "unit": unit, "period": period } })
        };
        let y25 = "2025-01-01T00:00:00/2026-01-01T00:00:00";
        let y24 = "2024-01-01T00:00:00/2025-01-01T00:00:00";
        serde_json::from_value(serde_json::json!({ "facts": {
            "r25": fact("ifrs-full:RevenueFromContractsWithCustomers", "iso4217:EUR", y25, "1200"),
            "r24": fact("ifrs-full:RevenueFromContractsWithCustomers", "iso4217:EUR", y24, "1000"),
            "n25": fact("ifrs-full:ProfitLoss", "iso4217:EUR", y25, "240"),
            "n24": fact("ifrs-full:ProfitLoss", "iso4217:EUR", y24, "200"),
            "e25": fact("ifrs-full:DilutedEarningsLossPerShare", "iso4217:EUR/xbrli:shares", y25, "2.40"),
            "s25": fact("ifrs-full:WeightedAverageShares", "xbrli:shares", y25, "100"),
            "q25": fact("ifrs-full:Equity", "iso4217:EUR", "2026-01-01T00:00:00", "1200"),
            "o25": fact("ifrs-full:CashFlowsFromUsedInOperatingActivities", "iso4217:EUR", y25, "300"),
            "c25": fact("ifrs-full:PurchaseOfPropertyPlantAndEquipmentClassifiedAsInvestingActivities", "iso4217:EUR", y25, "60"),
            "d25": fact("ifrs-full:DividendsRecognisedAsDistributionsToOwnersPerShare", "iso4217:EUR/xbrli:shares", y25, "1.20"),
            // a segment figure (extra dimension) must not count as the company total
            "seg": { "value": "999999", "dimensions": { "concept": "ifrs-full:RevenueFromContractsWithCustomers",
                "entity": "scheme:LEI", "unit": "iso4217:EUR", "period": y25, "ifrs-full:SegmentsAxis": "x" } }
        }}))
        .unwrap()
    }

    #[test]
    fn periods_use_the_exclusive_end() {
        let d = |s| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        assert_eq!(
            period("2025-01-01T00:00:00/2026-01-01T00:00:00"),
            Some((Some(d("2025-01-01")), d("2025-12-31")))
        );
        assert_eq!(period("2026-01-01T00:00:00"), Some((None, d("2025-12-31"))));
    }

    #[test]
    fn derives_ratios_from_an_esef_report() {
        let facts = Facts::from_report(&report(), NaiveDate::from_ymd_opt(2026, 3, 1).unwrap());
        assert_eq!(facts.currency.as_deref(), Some("EUR"));
        // listed in pence: 1 EUR = 0.85 GBP = 85 GBX; price 2040 GBX = 24 EUR
        let f = derive(&facts, Some(2040.0), Some(85.0));
        assert!(
            (f.revenue_growth.unwrap() - 0.2).abs() < 1e-9,
            "segment figures are ignored"
        );
        assert!((f.earnings_growth.unwrap() - 0.2).abs() < 1e-9);
        assert!((f.profit_margin.unwrap() - 0.2).abs() < 1e-9);
        assert!((f.roe.unwrap() - 0.2).abs() < 1e-9);
        assert!((f.pe_ttm.unwrap() - 10.0).abs() < 1e-9); // 24 EUR / 2.40 EUR
        assert!((f.price_to_book.unwrap() - 2.0).abs() < 1e-9); // 2400 EUR / 1200 EUR
        assert!((f.fcf_yield.unwrap() - 0.1).abs() < 1e-9); // (300 − 60) / 2400
        assert!((f.dividend_yield.unwrap() - 0.05).abs() < 1e-9); // 1.20 / 24
        assert!((f.payout_ratio.unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(f.market_cap, Some(204_000.0)); // in pence
    }

    #[test]
    fn without_an_exchange_rate_price_ratios_stay_empty() {
        let facts = Facts::from_report(&report(), NaiveDate::from_ymd_opt(2026, 3, 1).unwrap());
        let f = derive(&facts, Some(20.0), None);
        assert!(f.pe_ttm.is_none() && f.market_cap.is_none() && f.dividend_yield.is_none());
        assert!(
            f.profit_margin.is_some() && f.revenue_growth.is_some(),
            "report-only ratios still work"
        );
    }
}
