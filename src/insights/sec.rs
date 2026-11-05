//! SEC EDGAR fundamentals provider (US-listed companies).
//!
//! Endpoints (free, no key; SEC's fair-access policy requires a descriptive
//! User-Agent with a contact email and ≤ 10 requests/second):
//!
//! * `www.sec.gov/files/company_tickers.json` — ticker → CIK (cached 7 days)
//! * `data.sec.gov/api/xbrl/companyfacts/CIK##########.json` — XBRL facts
//! * `data.sec.gov/submissions/CIK##########.json` — SIC sector
//!
//! Trailing-twelve-month (TTM) values use the standard construction
//! `last fiscal year + current YTD − prior-year YTD`, which works for 10-K /
//! 10-Q filers whose fourth quarter is never reported on its own. Ratios
//! combine TTM figures with the live Trading212 price. Forward P/E and
//! analyst targets aren't in filings and stay empty.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use chrono::{Duration as Days, NaiveDate, Utc};
use reqwest::{Client, header};
use serde::Deserialize;

use super::fundamentals::{Fundamentals, Lookup, Quarter};

/// Polite gap between SEC requests (policy allows 10/s).
const REQUEST_GAP: Duration = Duration::from_millis(150);

/// One XBRL fact.
#[derive(Debug, Clone, Deserialize)]
pub struct Fact {
    /// Period start (duration facts only).
    pub start: Option<NaiveDate>,
    /// Period end / instant.
    pub end: NaiveDate,
    /// Value.
    pub val: f64,
    /// Filing form, e.g. `10-K`.
    #[serde(default)]
    pub form: String,
    /// Filing date.
    pub filed: NaiveDate,
}

impl Fact {
    fn days(&self) -> i64 {
        self.start.map_or(0, |s| (self.end - s).num_days())
    }
}

#[derive(Debug, Deserialize)]
struct Concept {
    units: HashMap<String, Vec<Fact>>,
}

/// The parts of `companyfacts` Aura uses.
#[derive(Debug, Deserialize)]
pub struct CompanyFacts {
    #[serde(rename = "entityName", default)]
    entity_name: String,
    facts: HashMap<String, HashMap<String, Concept>>,
}

impl CompanyFacts {
    /// Facts for the first concept (in order) that has data in `unit`,
    /// de-duplicated per period keeping the latest filing.
    pub fn series(&self, concepts: &[&str], unit: &str) -> Vec<Fact> {
        for taxonomy in ["us-gaap", "dei"] {
            let Some(t) = self.facts.get(taxonomy) else {
                continue;
            };
            for c in concepts {
                if let Some(facts) = t.get(*c).and_then(|c| c.units.get(unit))
                    && !facts.is_empty()
                {
                    let mut by_period: HashMap<(Option<NaiveDate>, NaiveDate), Fact> =
                        HashMap::new();
                    for f in facts {
                        let key = (f.start, f.end);
                        match by_period.get(&key) {
                            Some(existing) if existing.filed >= f.filed => {}
                            _ => {
                                by_period.insert(key, f.clone());
                            }
                        }
                    }
                    let mut v: Vec<Fact> = by_period.into_values().collect();
                    v.sort_by_key(|f| (f.end, f.start));
                    return v;
                }
            }
        }
        Vec::new()
    }
}

/// TTM value of a duration series as of the latest period ending on or
/// before `cutoff`: `FY + YTD − prior YTD`, or the FY alone when the latest
/// period is the fiscal year.
pub fn ttm(series: &[Fact], cutoff: NaiveDate) -> Option<f64> {
    let usable: Vec<&Fact> = series
        .iter()
        .filter(|f| f.start.is_some() && f.end <= cutoff)
        .collect();
    let annual = usable
        .iter()
        .filter(|f| (330..=400).contains(&f.days()))
        .max_by_key(|f| f.end)?;
    let latest_end = usable.iter().map(|f| f.end).max()?;
    if latest_end <= annual.end {
        return Some(annual.val);
    }
    // Year-to-date of the current fiscal year: starts after the annual
    // period ended, ends at the latest date.
    let ytd = usable
        .iter()
        .filter(|f| {
            f.end == latest_end && f.start.is_some_and(|s| s > annual.end) && f.days() < 330
        })
        .max_by_key(|f| f.days())?;
    let near = |a: NaiveDate, b: NaiveDate| (a - b).num_days().abs() <= 20;
    let prior = usable.iter().find(|f| {
        near(f.end, ytd.end - Days::days(365))
            && (f.days() - ytd.days()).abs() <= 20
            && f.days() < 330
    });
    match prior {
        Some(p) => Some(annual.val + ytd.val - p.val),
        None => Some(annual.val),
    }
}

/// Latest instant value (balance-sheet item) on or before `cutoff`.
pub fn latest_instant(series: &[Fact], cutoff: NaiveDate) -> Option<f64> {
    series
        .iter()
        .filter(|f| f.end <= cutoff)
        .max_by_key(|f| (f.end, f.filed))
        .map(|f| f.val)
}

/// Last `n` single-quarter values (~3-month durations), oldest first.
pub fn quarters(series: &[Fact], n: usize) -> Vec<(NaiveDate, f64)> {
    let mut q: Vec<(NaiveDate, f64)> = series
        .iter()
        .filter(|f| (80..=100).contains(&f.days()))
        .map(|f| (f.end, f.val))
        .collect();
    q.sort_by_key(|(d, _)| *d);
    q.split_off(q.len().saturating_sub(n))
}

fn growth(now: Option<f64>, before: Option<f64>) -> Option<f64> {
    match (now, before) {
        (Some(n), Some(b)) if b.abs() > f64::EPSILON && b > 0.0 => Some(n / b - 1.0),
        _ => None,
    }
}

const REVENUE: &[&str] = &[
    "RevenueFromContractWithCustomerExcludingAssessedTax",
    "Revenues",
    "SalesRevenueNet",
    "RevenueFromContractWithCustomerIncludingAssessedTax",
];

/// Derives [`Fundamentals`] from company facts and the live price.
pub fn derive(cf: &CompanyFacts, price: Option<f64>, today: NaiveDate) -> Fundamentals {
    let usd = |c: &[&str]| cf.series(c, "USD");
    let revenue = usd(REVENUE);
    let net_income = usd(&["NetIncomeLoss", "ProfitLoss"]);
    let op_income = usd(&["OperatingIncomeLoss"]);
    let ocf = usd(&["NetCashProvidedByUsedInOperatingActivities"]);
    let capex = usd(&["PaymentsToAcquirePropertyPlantAndEquipment"]);
    let equity = usd(&[
        "StockholdersEquity",
        "StockholdersEquityIncludingPortionAttributableToNoncontrollingInterest",
    ]);
    let debt = usd(&["LongTermDebt", "LongTermDebtNoncurrent"]);
    let eps = cf.series(
        &["EarningsPerShareDiluted", "EarningsPerShareBasic"],
        "USD/shares",
    );
    let dps = cf.series(
        &[
            "CommonStockDividendsPerShareDeclared",
            "CommonStockDividendsPerShareCashPaid",
        ],
        "USD/shares",
    );
    let shares = cf.series(
        &[
            "EntityCommonStockSharesOutstanding",
            "WeightedAverageNumberOfDilutedSharesOutstanding",
        ],
        "shares",
    );

    let year_ago = today - Days::days(365);
    let rev_ttm = ttm(&revenue, today);
    let ni_ttm = ttm(&net_income, today);
    let eps_ttm = ttm(&eps, today);
    let fcf = match (ttm(&ocf, today), ttm(&capex, today)) {
        (Some(o), Some(c)) => Some(o - c),
        (Some(o), None) => Some(o),
        _ => None,
    };
    let eq = latest_instant(&equity, today);
    let market_cap = price
        .zip(latest_instant(&shares, today))
        .map(|(p, s)| p * s);
    let pe = price.zip(eps_ttm).map(|(p, e)| p / e);
    let earnings_growth = growth(ni_ttm, ttm(&net_income, year_ago));
    let dps_ttm = ttm(&dps, today);

    let quarters = quarters(&revenue, 4)
        .into_iter()
        .map(|(end, rev)| {
            let ni = net_income
                .iter()
                .find(|f| f.end == end && (80..=100).contains(&f.days()))
                .map(|f| f.val);
            Quarter {
                label: end.format("%b %Y").to_string(),
                revenue: Some(rev),
                net_income: ni,
                eps: None,
            }
        })
        .collect();

    Fundamentals {
        symbol: cf.entity_name.clone(),
        kind: "stock".into(),
        market_cap,
        pe_ttm: pe,
        peg: pe
            .zip(earnings_growth)
            .filter(|(p, g)| *p > 0.0 && *g > 0.0)
            .map(|(p, g)| p / (g * 100.0)),
        price_to_book: market_cap
            .zip(eq)
            .filter(|(_, e)| *e > 0.0)
            .map(|(m, e)| m / e),
        revenue_growth: growth(rev_ttm, ttm(&revenue, year_ago)),
        earnings_growth,
        profit_margin: ni_ttm
            .zip(rev_ttm)
            .filter(|(_, r)| *r > 0.0)
            .map(|(n, r)| n / r),
        operating_margin: ttm(&op_income, today)
            .zip(rev_ttm)
            .filter(|(_, r)| *r > 0.0)
            .map(|(o, r)| o / r),
        roe: ni_ttm.zip(eq).filter(|(_, e)| *e > 0.0).map(|(n, e)| n / e),
        debt_to_equity: latest_instant(&debt, today)
            .zip(eq)
            .filter(|(_, e)| *e > 0.0)
            .map(|(d, e)| d / e),
        free_cash_flow: fcf,
        fcf_yield: fcf
            .zip(market_cap)
            .filter(|(_, m)| *m > 0.0)
            .map(|(f, m)| f / m),
        dividend_yield: dps_ttm
            .zip(price)
            .filter(|(_, p)| *p > 0.0)
            .map(|(d, p)| d / p),
        payout_ratio: dps_ttm
            .zip(eps_ttm)
            .filter(|(_, e)| *e > 0.0)
            .map(|(d, e)| d / e),
        quarters,
        source: "SEC EDGAR".into(),
        ..Default::default()
    }
}

/// `AAPL_US_EQ` → `AAPL`, `BRK.B_US_EQ` → `BRK-B`; `None` for non-US listings.
pub fn us_symbol(t212_ticker: &str) -> Option<String> {
    let base = t212_ticker.strip_suffix("_US_EQ")?;
    Some(base.replace('.', "-").to_uppercase())
}

fn user_agent(contact: &str) -> String {
    format!("Aura Portfolio/{} {}", crate::VERSION, contact.trim())
}

async fn get_json<T: serde::de::DeserializeOwned>(
    http: &Client,
    url: &str,
    contact: &str,
) -> Result<T> {
    tokio::time::sleep(REQUEST_GAP).await;
    let resp = http
        .get(url)
        .header(header::USER_AGENT, user_agent(contact))
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    match resp.status().as_u16() {
        403 => Err(anyhow!(
            "SEC refused the request (403). Check the contact email in Settings → AI Insights data."
        )),
        404 => Err(anyhow!("not found at SEC")),
        _ => Ok(resp.error_for_status()?.json().await?),
    }
}

#[derive(Deserialize)]
struct TickerRow {
    cik_str: u64,
    ticker: String,
}

/// Ticker → CIK map, cached on disk for 7 days.
async fn cik_for(
    http: &Client,
    contact: &str,
    cache_dir: &Path,
    symbol: &str,
) -> Result<Option<u64>> {
    let path = cache_dir.join("sec-company-tickers.json");
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .is_ok_and(|t| {
            t.elapsed()
                .is_ok_and(|e| e < Duration::from_secs(7 * 86_400))
        });
    let map: HashMap<String, TickerRow> = match std::fs::read(&path).ok().filter(|_| fresh) {
        Some(bytes) => serde_json::from_slice(&bytes)?,
        None => {
            let map: HashMap<String, TickerRow> = get_json(
                http,
                "https://www.sec.gov/files/company_tickers.json",
                contact,
            )
            .await?;
            let _ = std::fs::create_dir_all(cache_dir);
            let _ = std::fs::write(
                &path,
                serde_json::to_vec(
                    &map.iter()
                        .map(|(k, v)| {
                            (
                                k,
                                serde_json::json!({"cik_str": v.cik_str, "ticker": v.ticker}),
                            )
                        })
                        .collect::<HashMap<_, _>>(),
                )?,
            );
            map
        }
    };
    Ok(map
        .values()
        .find(|r| r.ticker.eq_ignore_ascii_case(symbol))
        .map(|r| r.cik_str))
}

#[derive(Deserialize)]
struct Submissions {
    #[serde(rename = "sicDescription", default)]
    sic_description: String,
}

/// Fetches and derives fundamentals for one holding (US listings only).
pub async fn fetch(
    http: &Client,
    contact: &str,
    cache_dir: &Path,
    lookup: &Lookup,
) -> Result<Option<Fundamentals>> {
    if contact.trim().is_empty() || !contact.contains('@') {
        return Err(anyhow!(
            "SEC EDGAR needs a contact email (Settings → AI Insights data)"
        ));
    }
    let Some(symbol) = us_symbol(&lookup.ticker) else {
        return Ok(None);
    };
    let Some(cik) = cik_for(http, contact, cache_dir, &symbol).await? else {
        return Ok(None);
    };
    let cf: CompanyFacts = match get_json(
        http,
        &format!("https://data.sec.gov/api/xbrl/companyfacts/CIK{cik:010}.json"),
        contact,
    )
    .await
    {
        Ok(cf) => cf,
        Err(e) if e.to_string().contains("not found") => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut f = derive(&cf, lookup.price, Utc::now().date_naive());
    f.symbol = symbol;
    if let Ok(s) = get_json::<Submissions>(
        http,
        &format!("https://data.sec.gov/submissions/CIK{cik:010}.json"),
        contact,
    )
    .await
        && !s.sic_description.is_empty()
    {
        f.sector = Some(title_case(&s.sic_description));
    }
    Ok(Some(f))
}

/// SEC SIC descriptions are upper-case ("ELECTRONIC COMPUTERS").
fn title_case(s: &str) -> String {
    s.split_whitespace()
        .map(|w| {
            let lower = w.to_lowercase();
            let mut c = lower.chars();
            c.next().map_or_else(String::new, |f| {
                f.to_uppercase().collect::<String>() + c.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn fact(start: &str, end: &str, val: f64, form: &str) -> Fact {
        Fact {
            start: Some(d(start)),
            end: d(end),
            val,
            form: form.into(),
            filed: d(end) + Days::days(30),
        }
    }

    /// Fiscal year ends late September (Apple-like).
    fn revenue() -> Vec<Fact> {
        vec![
            fact("2024-09-29", "2025-09-27", 400.0, "10-K"),
            fact("2023-10-01", "2024-09-28", 380.0, "10-K"),
            // FY2026 YTD after two quarters, and the matching prior-year YTD.
            fact("2025-09-28", "2026-03-28", 230.0, "10-Q"),
            fact("2024-09-29", "2025-03-29", 210.0, "10-Q"),
            // single quarters
            fact("2025-12-28", "2026-03-28", 110.0, "10-Q"),
            fact("2025-09-28", "2025-12-27", 120.0, "10-Q"),
        ]
    }

    #[test]
    fn ttm_adds_ytd_and_removes_prior_ytd() {
        // 400 + 230 − 210 = 420
        assert_eq!(ttm(&revenue(), d("2026-06-01")), Some(420.0));
        // As of the fiscal year end, TTM is the annual figure.
        assert_eq!(ttm(&revenue(), d("2025-10-15")), Some(400.0));
    }

    #[test]
    fn quarters_are_three_month_periods() {
        let q = quarters(&revenue(), 4);
        assert_eq!(q.len(), 2);
        assert_eq!(q.last().unwrap().1, 110.0);
    }

    #[test]
    fn maps_t212_tickers_to_sec_symbols() {
        assert_eq!(us_symbol("AAPL_US_EQ").as_deref(), Some("AAPL"));
        assert_eq!(us_symbol("BRK.B_US_EQ").as_deref(), Some("BRK-B"));
        assert_eq!(us_symbol("VUSAl_EQ"), None);
    }

    #[test]
    fn derives_ratios_from_companyfacts() {
        let json = serde_json::json!({
            "entityName": "Example Corp",
            "facts": {
                "us-gaap": {
                    "Revenues": { "units": { "USD": [
                        {"start": "2024-01-01", "end": "2024-12-31", "val": 1000.0, "form": "10-K", "filed": "2025-02-01"},
                        {"start": "2023-01-01", "end": "2023-12-31", "val": 800.0, "form": "10-K", "filed": "2024-02-01"}
                    ]}},
                    "NetIncomeLoss": { "units": { "USD": [
                        {"start": "2024-01-01", "end": "2024-12-31", "val": 200.0, "form": "10-K", "filed": "2025-02-01"},
                        {"start": "2023-01-01", "end": "2023-12-31", "val": 100.0, "form": "10-K", "filed": "2024-02-01"}
                    ]}},
                    "EarningsPerShareDiluted": { "units": { "USD/shares": [
                        {"start": "2024-01-01", "end": "2024-12-31", "val": 2.0, "form": "10-K", "filed": "2025-02-01"}
                    ]}},
                    "StockholdersEquity": { "units": { "USD": [
                        {"end": "2024-12-31", "val": 1000.0, "form": "10-K", "filed": "2025-02-01"}
                    ]}}
                },
                "dei": {
                    "EntityCommonStockSharesOutstanding": { "units": { "shares": [
                        {"end": "2025-01-31", "val": 100.0, "form": "10-K", "filed": "2025-02-01"}
                    ]}}
                }
            }
        });
        let cf: CompanyFacts = serde_json::from_value(json).unwrap();
        let f = derive(&cf, Some(30.0), d("2025-06-01"));
        assert_eq!(f.pe_ttm, Some(15.0)); // 30 / 2
        assert_eq!(f.market_cap, Some(3000.0));
        assert!((f.revenue_growth.unwrap() - 0.25).abs() < 1e-9);
        assert!((f.earnings_growth.unwrap() - 1.0).abs() < 1e-9);
        assert!((f.profit_margin.unwrap() - 0.2).abs() < 1e-9);
        assert!((f.roe.unwrap() - 0.2).abs() < 1e-9);
        assert!((f.peg.unwrap() - 0.15).abs() < 1e-9); // 15 / 100
        assert_eq!(f.price_to_book, Some(3.0));
    }
}
