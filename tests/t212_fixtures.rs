//! Deserialisation and end-to-end analytics tests against JSON fixtures that
//! follow the official Trading212 OpenAPI response schemas.
//!
//! When adding an endpoint, add a fixture here. Strip any real account ids,
//! references or amounts first.

use aura_portfolio::portfolio::analytics;
use aura_portfolio::t212::models::{AccountSummary, Dividend, Page, Position, Transaction};
use chrono::{TimeZone, Utc};

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/t212/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn load() -> (
    AccountSummary,
    Vec<Position>,
    Page<Dividend>,
    Page<Transaction>,
) {
    (
        serde_json::from_str(&fixture("account_summary.json")).unwrap(),
        serde_json::from_str(&fixture("positions.json")).unwrap(),
        serde_json::from_str(&fixture("dividends_page.json")).unwrap(),
        serde_json::from_str(&fixture("transactions_page.json")).unwrap(),
    )
}

#[test]
fn parses_every_fixture() {
    let (summary, positions, divs, txs) = load();
    assert_eq!(summary.currency, "GBP");
    assert_eq!(summary.cash.reserved_for_orders, 25.0);
    assert_eq!(positions.len(), 2);
    assert_eq!(positions[1].wallet_impact.fx_impact, None); // explicit null
    assert_eq!(positions[0].instrument.ticker, "AAPL_US_EQ");
    assert_eq!(divs.items.len(), 3); // unknown fields are ignored
    assert_eq!(
        divs.next_page_path.as_deref(),
        Some("/api/v0/equity/history/dividends?limit=50&cursor=987654321")
    );
    assert_eq!(txs.next_page_path, None);
    assert_eq!(txs.items[1].kind, "WITHDRAW");
}

#[test]
fn analytics_from_fixtures() {
    let (summary, positions, divs, txs) = load();
    let now = Utc.with_ymd_and_hms(2026, 11, 5, 12, 0, 0).unwrap();
    let a = analytics::compute(&summary, &positions, &divs.items, &txs.items, now);

    // Total return vs. net deposits: 10917.75 − (8000 + 2000 − 500) = 1417.75.
    assert!(a.total_return_from_deposits);
    assert!((a.total_return - 1417.75).abs() < 1e-6);

    // Holdings sorted by value, weights sum to 1.
    assert_eq!(a.holdings[0].ticker, "VUSAl_EQ");
    let w: f64 = a.holdings.iter().map(|h| h.weight).sum();
    assert!((w - 1.0).abs() < 1e-9);

    // Tax: VUSA (GBX, exact FX) 30p × 80.25 = £24.075 gross vs £24.08 net → ~0%.
    // AAPL (USD, FX implied 3200/(200×20) = 0.8): $0.25×20×0.8 = £4.00 gross, £3.40 net → 15%.
    // Weighted: (0 + 0.6 + 0.6) / (24.075 + 4 + 4) ≈ 3.74%.
    let tax = &a.dividends.tax;
    assert_eq!(tax.measured, 3);
    assert!(tax.fx_estimated);
    let rate = tax.rate.unwrap();
    assert!((rate - 1.2 / 32.075).abs() < 1e-3, "rate = {rate}");

    // AAPL pays quarterly → projected ~13 Nov 2026 within the horizon.
    let aapl = a
        .upcoming
        .iter()
        .find(|u| u.ticker == "AAPL_US_EQ")
        .expect("AAPL projected");
    assert_eq!(aapl.cadence, analytics::Cadence::Quarterly);
    assert!((aapl.amount - 3.40).abs() < 1e-6);
}
