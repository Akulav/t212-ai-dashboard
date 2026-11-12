//! Fetches EU/UK fundamentals from ESEF/UKSEF filings for one ISIN and prints them (no Trading212 key needed).
//!
//! ```text
//! cargo run --example esef_check -- NL0010273215 EUR 640      # ASML in euros
//! cargo run --example esef_check -- GB00BP6MXD84 GBX 2700     # Shell in pence
//! ```

use aura_portfolio::insights::{esef, fundamentals::Lookup};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let isin = args.next().unwrap_or_else(|| "NL0010273215".into());
    let currency = args.next().unwrap_or_else(|| "EUR".into());
    let price = args.next().and_then(|p| p.parse().ok());
    let lookup = Lookup {
        ticker: isin.clone(),
        isin,
        name: "check".into(),
        currency,
        price,
    };
    match esef::fetch(&aura_portfolio::net::http_client(), &lookup).await? {
        Some(f) => println!("{f:#?}"),
        None => println!("not covered (no LEI, or no ESEF/UKSEF report)"),
    }
    Ok(())
}
