//! # Aura Portfolio
//!
//! A native, fully-Rust desktop dashboard for Trading212 portfolios with a
//! zero-config **local** AI analyst powered by `llama.cpp`.
//!
//! The crate is split into a library (everything testable and documented) and
//! a thin binary (`src/main.rs`) that wires the pieces together.
//!
//! ## Module map
//!
//! | Module        | Responsibility                                                      |
//! |---------------|---------------------------------------------------------------------|
//! | [`paths`]     | Where data lives (portable folder next to the exe, or OS data dir)  |
//! | [`config`]    | User settings persisted as JSON (trading mode always boots read-only) |
//! | [`secrets`]   | Trading212 credentials in the OS keychain via `keyring`             |
//! | [`t212`]      | Trading212 API client: auth, rate limits, pagination, write guard   |
//! | [`portfolio`] | Sync service, local cache, holdings/dividend/tax analytics          |
//! | [`insights`] | Phase 4: fundamentals, scoring, opportunities, local-LLM analysis   |
//! | [`report`]   | Branded PDF report (Typst): portfolio + AI insights                 |
//! | [`net`]       | Shared HTTP client, resumable downloads, GitHub release lookup      |
//! | [`archive`]   | Safe `.zip` / `.tar.gz` extraction                                  |
//! | [`llm`]       | llama.cpp runtime install, model catalog, server process lifecycle  |
//! | [`telemetry`] | Real-time RAM / VRAM sampling of the local LLM process              |
//! | [`market`]    | Market context (NYSE bell countdown; macro data in a later phase)   |
//! | [`updater`]   | Self-update from GitHub Releases with SHA-256 verification          |
//! | [`ui`]        | The `egui` application, pages, theme and widgets                    |

#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

pub mod archive;
pub mod config;
pub mod insights;
pub mod llm;
pub mod market;
pub mod net;
pub mod paths;
pub mod portfolio;
pub mod report;
pub mod secrets;
pub mod t212;
pub mod telemetry;
pub mod ui;
pub mod updater;

/// Human-readable product name.
pub const APP_NAME: &str = "Aura Portfolio";

/// Reverse-DNS style identifier, used for the keychain service name.
pub const APP_ID: &str = "com.akulav.aura-portfolio";

/// Crate version, taken from `Cargo.toml` at compile time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Author credited in the UI footer.
pub const AUTHOR: &str = "akulav";

/// Rust target triple this binary was built for (set by `build.rs`).
pub const TARGET: &str = env!("AURA_TARGET");
