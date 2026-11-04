//! Aura Portfolio entry point.
//!
//! Responsibilities of this file are deliberately small:
//! 1. initialise logging,
//! 2. resolve data paths and build the shared Tokio runtime + HTTP client,
//! 3. construct the [`LlmManager`] and install the safety nets that guarantee
//!    the `llama-server` child process dies with us (panic hook + final
//!    shutdown + OS-level lifetime binding inside `llm::server`),
//! 4. hand control to `eframe`.

// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use aura_portfolio::llm::LlmManager;
use aura_portfolio::paths::AppPaths;
use aura_portfolio::{APP_NAME, VERSION, net, ui};

fn main() -> anyhow::Result<()> {
    init_tracing();
    tracing::info!("{APP_NAME} v{VERSION} starting");

    let paths = AppPaths::resolve().context("failed to resolve data directories")?;
    tracing::info!(root = %paths.root.display(), portable = paths.portable, "data directory");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("aura-worker")
        .build()
        .context("failed to build Tokio runtime")?;

    let http = net::http_client();
    let llm = LlmManager::new(runtime.handle().clone(), paths.clone(), http.clone());

    // Safety net #1: a panic anywhere must still take llama-server down with it.
    install_panic_hook(llm.clone());

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_app_id("aura-portfolio")
            .with_inner_size([1400.0, 880.0])
            .with_min_inner_size([1080.0, 680.0])
            .with_icon(Arc::new(ui::theme::app_icon(256))),
        ..Default::default()
    };

    let app_llm = llm.clone();
    let handle = runtime.handle().clone();
    let result = eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| Ok(Box::new(ui::AuraApp::new(cc, handle, paths, http, app_llm)))),
    );

    // Safety net #2: eframe has returned (window closed or failed to start).
    // `AuraApp::on_exit` already did this; doing it again is idempotent.
    llm.shutdown();
    runtime.shutdown_timeout(Duration::from_secs(2));

    result.map_err(|e| anyhow::anyhow!("UI terminated with an error: {e}"))
}

/// Logs go to stderr; control verbosity with `RUST_LOG`, e.g.
/// `RUST_LOG=aura_portfolio=debug`.
fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,wgpu=warn,eframe=warn,egui_glow=warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

/// Chains a hook in front of the default panic handler that kills the local
/// LLM server before the process unwinds/aborts.
fn install_panic_hook(llm: LlmManager) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        llm.shutdown();
        default_hook(info);
    }));
}
