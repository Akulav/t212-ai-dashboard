//! Aura Portfolio — the program.
//!
//! ```text
//!   aura-portfolio                  open the app (Linux/macOS, outside an app folder: first asks where to install)
//!   aura-portfolio --here           run in the program's own folder, don't install it
//!   aura-portfolio --dir <folder>   use another app folder than the program's own (data in <folder>/data)
//!   aura-portfolio --uninstall      take it out of the app list (the folder only if you say so)
//!   aura-portfolio --version        print "aura-portfolio X.Y.Z"
//!   aura-portfolio --check-update   ask GitHub for the latest release and say whether it's newer
//! ```
//!
//! Start-up order: install or update offer (`install.rs`, Linux/macOS) → remove what the last update left (`.old`,
//! a downloaded setup) → one instance per app folder (`data/.aura.lock`) → the window. The safety nets that guarantee the
//! `llama-server` child dies with us (panic hook, final shutdown, OS-level lifetime binding in `llm::server`) stay.

// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use aura_portfolio::llm::LlmManager;
use aura_portfolio::paths::AppPaths;
use aura_portfolio::{APP_NAME, VERSION, install, net, ui, updater};

#[derive(Default)]
struct Args {
    here: bool,
    dir: Option<PathBuf>,
    uninstall: bool,
    version: bool,
    after_update: bool,
    check_update: bool,
}

fn args() -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        match x.as_str() {
            "--here" | "--portable" => a.here = true,
            "--dir" => a.dir = Some(PathBuf::from(it.next().ok_or("--dir needs a folder")?)),
            "--uninstall" => a.uninstall = true,
            "--version" | "-V" => a.version = true,
            "--after-update" => a.after_update = true,
            "--check-update" => a.check_update = true,
            other => {
                return Err(format!(
                    "unknown option {other} (see --version, --check-update, --here, --dir <folder>, --uninstall)"
                ));
            }
        }
    }
    Ok(a)
}

/// A windows-subsystem program has no console: borrow the terminal it was started from, if any.
#[cfg(windows)]
fn attach_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: attaching to the parent's console has no preconditions; failure (no console) is fine.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}
#[cfg(not(windows))]
fn attach_console() {}

/// Windows' canonical form starts with `\\?\`; the ordinary form is used instead (left alone for network shares).
fn plain(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => p.to_path_buf(),
    }
}

fn this_exe() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| plain(&p))
}

/// The app folder: `--dir`, else the program's own folder (for a program inside `X.app`, the folder holding it).
fn app_dir(a: &Args) -> PathBuf {
    if let Some(d) = &a.dir {
        return d.clone();
    }
    this_exe()
        .and_then(|p| install::root_of_exe(&p))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Linux/macOS, started outside an app folder (Downloads): ask where to install (or whether to update the install
/// that's there), install, and start the installed program. `true` = done here (another process runs the app).
fn install_first(a: &Args) -> bool {
    if a.here || a.after_update || a.dir.is_some() || std::env::var_os("AURA_DATA_DIR").is_some() {
        return false;
    }
    let Some(exe) = this_exe() else { return false };
    if updater::self_built(&exe) {
        return false;
    }
    let passed: Vec<String> = std::env::args().skip(1).collect();
    let offer = install::offer_for(&exe);
    let target = match offer {
        install::Offer::None => return false,
        install::Offer::OpenInstalled { root } => {
            return match install::start_installed(&root, &passed) {
                Ok(()) => true,
                Err(e) => {
                    install::notice(&e);
                    false
                }
            };
        }
        offer => match ui::install_window::ask(offer) {
            ui::install_window::Choice::Install(dir) => dir,
            ui::install_window::Choice::Cancel => return true,
            ui::install_window::Choice::RunHere => {
                // (a fresh process: the install window's event loop has ended and can't be reused)
                let mut cmd = std::process::Command::new(&exe);
                cmd.arg("--here").args(&passed);
                return cmd.spawn().is_ok();
            }
        },
    };
    match install::install_and_start(&exe, &target, &passed) {
        Ok(()) => true,
        Err(e) => {
            install::notice(&format!("Couldn't install into {}: {e}", target.display()));
            true
        }
    }
}

fn main() -> anyhow::Result<()> {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            attach_console();
            eprintln!("aura-portfolio: {e}");
            std::process::exit(2);
        }
    };
    if a.version {
        attach_console();
        println!("aura-portfolio {VERSION}");
        return Ok(());
    }
    init_tracing();
    if a.check_update {
        attach_console();
        let rt = tokio::runtime::Runtime::new()?;
        let token = AppPaths::for_app(&app_dir(&a), false)
            .ok()
            .and_then(|p| updater::github_token(&p.root));
        match rt.block_on(updater::check_latest(&net::http_client(), token.as_deref())) {
            Ok(Some(u)) => println!("this: {VERSION}, latest release: {} (newer)", u.version),
            Ok(None) => println!("this: {VERSION} is the latest release"),
            Err(e) => println!("update check failed: {e:#}"),
        }
        return Ok(());
    }
    if a.uninstall {
        attach_console();
        install::uninstall(&app_dir(&a));
        return Ok(());
    }
    if install_first(&a) {
        return Ok(());
    }

    let dir = app_dir(&a);
    let exe = this_exe();
    if let Some(exe) = &exe {
        updater::clean_up(exe); // (the previous program an update left as .old, a download that didn't finish)
    }
    let built_here = exe.as_deref().is_some_and(updater::self_built);
    let paths =
        AppPaths::for_app(&dir, !built_here).context("failed to prepare the data folder")?;
    tracing::info!(
        "{APP_NAME} {VERSION} starting in {} (data: {})",
        dir.display(),
        paths.root.display()
    );

    // One Aura per app folder. Started by an update, the previous program may still be ending: wait up to a minute.
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(paths.lock_file());
    let waits = if a.after_update { 240 } else { 0 };
    let _lock = match lock {
        Ok(f) => {
            let held = (0..=waits).any(|i| {
                if i > 0 {
                    std::thread::sleep(Duration::from_millis(250));
                }
                f.try_lock().is_ok()
            });
            if !held {
                install::show_running();
                return Ok(());
            }
            Some(f)
        }
        Err(e) => {
            tracing::warn!(
                "couldn't open {} ({e}); going on without the one-instance check",
                paths.lock_file().display()
            );
            None
        }
    };
    let _ = std::fs::remove_file(updater::staged_setup(&paths.downloads)); // (the setup of the last update)

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
            .with_icon(Arc::new(ui::theme::app_icon())),
        ..Default::default()
    };

    let app_llm = llm.clone();
    let handle = runtime.handle().clone();
    let result = eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| {
            Ok(Box::new(ui::AuraApp::new(
                cc, handle, paths, http, app_llm, built_here,
            )))
        }),
    );

    // Safety net #2: eframe has returned (window closed or failed to start).
    // `AuraApp::on_exit` already did this; doing it again is idempotent.
    llm.shutdown();
    runtime.shutdown_timeout(Duration::from_secs(2));

    result.map_err(|e| anyhow::anyhow!("UI terminated with an error: {e}"))
}

/// Logs go to stderr; control verbosity with `RUST_LOG`, e.g. `RUST_LOG=aura_portfolio=debug`.
fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,wgpu=warn,eframe=warn,egui_glow=warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

/// Chains a hook in front of the default panic handler that kills the local LLM server before the process
/// unwinds/aborts.
fn install_panic_hook(llm: LlmManager) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        llm.shutdown();
        default_hook(info);
    }));
}
