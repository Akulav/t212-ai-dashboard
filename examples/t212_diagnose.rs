//! Read-only Trading212 connectivity check.
//!
//! Loads the credentials Aura stored in the OS keychain and calls each
//! endpoint the sync uses, printing only HTTP outcomes and counts — never the
//! key or secret. Always uses a read-only client.
//!
//! ```text
//! cargo run --example t212_diagnose            # both environments
//! cargo run --example t212_diagnose -- demo    # one environment
//! ```

use aura_portfolio::config::{T212Environment, TradingMode};
use aura_portfolio::{net, secrets, t212::T212Client};

#[tokio::main]
async fn main() {
    let arg = std::env::args().nth(1).unwrap_or_default().to_lowercase();
    if arg == "sync" {
        return full_sync().await;
    }
    let envs: Vec<T212Environment> = match arg.as_str() {
        "live" => vec![T212Environment::Live],
        "demo" | "practice" => vec![T212Environment::Demo],
        _ => vec![T212Environment::Live, T212Environment::Demo],
    };

    for env in envs {
        println!("== {} ({})", env.label(), env.base_url());
        let creds = match secrets::load(env) {
            Ok(Some(c)) => c,
            Ok(None) => {
                println!("   no credentials stored");
                continue;
            }
            Err(e) => {
                println!("   keychain error: {e:#}");
                continue;
            }
        };
        println!(
            "   credentials: key {} chars, secret {}",
            creds.api_key.trim().len(),
            match &creds.api_secret {
                Some(s) if !s.trim().is_empty() => format!("{} chars", s.trim().len()),
                _ => "none (legacy header auth)".into(),
            }
        );
        let client = T212Client::new(net::http_client(), env, creds, TradingMode::ReadOnly);

        report(
            "account summary",
            client
                .account_summary()
                .await
                .map(|s| format!("currency {}, {} fields ok", s.currency, 1)),
        );
        report(
            "positions",
            client
                .positions()
                .await
                .map(|p| format!("{} positions", p.len())),
        );
        report(
            "dividends (1 page)",
            client
                .dividends(1, |_| false)
                .await
                .map(|d| format!("{} items", d.len())),
        );
        report(
            "transactions (1 page)",
            client
                .transactions(1, |_| false)
                .await
                .map(|t| format!("{} items", t.len())),
        );
    }
}

/// Runs the real `PortfolioService` (Live) into a throwaway data folder and
/// prints counts and warnings only — no amounts.
async fn full_sync() {
    use aura_portfolio::paths::AppPaths;
    use aura_portfolio::portfolio::{PortfolioService, SyncStatus};

    let dir = std::env::temp_dir().join(format!("aura-diag-{}", std::process::id()));
    let paths = AppPaths::at(dir.clone(), false).expect("temp dir");
    let svc = PortfolioService::new(
        tokio::runtime::Handle::current(),
        net::http_client(),
        paths,
        T212Environment::Live,
    );
    svc.refresh_now();
    let start = std::time::Instant::now();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    while svc.is_syncing() && start.elapsed().as_secs() < 900 {
        if let SyncStatus::Syncing(step) = svc.snapshot().status {
            println!("   … {step}");
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
    let st = svc.snapshot();
    println!("status: {:?}", st.status);
    for w in &st.warnings {
        println!("warning: {w}");
    }
    if let Some(v) = st.view {
        let a = &v.analytics;
        println!(
            "holdings {}, dividends {}, upcoming {}, deposit flows {}, tax rate measured on {} payments, total return basis: {}",
            a.holdings.len(),
            v.dividend_count,
            a.upcoming.len(),
            a.net_deposits.len(),
            a.dividends.tax.measured,
            if a.total_return_from_deposits {
                "net deposits"
            } else {
                "position P/L"
            }
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

fn report<T: std::fmt::Display, E: std::fmt::Display>(what: &str, r: Result<T, E>) {
    match r {
        Ok(v) => println!("   OK   {what}: {v}"),
        Err(e) => println!("   FAIL {what}: {e}"),
    }
}
