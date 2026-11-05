# CLAUDE.md — Aura Portfolio

Native Rust desktop app (egui/eframe) for Trading212 portfolio analytics with a local
`llama.cpp` AI analyst. Author/brand: **Akulav**. Read this before changing anything.

## Rules (always)

1. **Finishing a build = release hygiene.** Whenever a piece of work is complete and verified:
   1. Bump `version` in `Cargo.toml` (SemVer: fix → patch, feature → minor), then run
      `cargo check` so `Cargo.lock` updates.
   2. Add an entry at the top of `CHANGELOG.md` (Keep a Changelog style: Added / Changed /
      Fixed, with the date).
   3. Commit with a message whose subject is `vX.Y.Z: <summary>` and whose body is the
      changelog entry. Never commit without the version bump + changelog.
   4. Pushing `main` with a new version publishes a GitHub Release automatically once CI is
      green (see `.github/workflows/release.yml`). If a push isn't possible from the shell,
      tell the user to push from GitHub Desktop.
2. **Quality gate before every commit** (CI enforces the same):
   `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`,
   `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`. Public items need `///` docs
   (`#![warn(missing_docs)]`).
3. **Safety:** trading is read-only by default; any order endpoint must go through
   `T212Client::mutating()`. Never log or print API keys/secrets. Never commit real
   portfolio data — screenshots and fixtures use demo/synthetic data only.
4. **Verify visually** after UI changes: `scripts/screenshots.ps1 -Release` (demo mode,
   `PrintWindow` capture) and look at the PNGs before committing.
5. **Not financial advice:** AI output must be labelled as such in the UI and reports.

## Commands

| Task | Command |
|---|---|
| Run (dev) | `cargo run` |
| Run with sample data | `AURA_DEMO=1 cargo run` (PowerShell: `$env:AURA_DEMO=1; cargo run`) |
| Open a page directly | `AURA_PAGE=dashboard\|holdings\|dividends\|insights\|ai\|settings` |
| Tests / lints | `cargo test`, `cargo clippy --all-targets -- -D warnings` |
| Screenshots (Windows) | `powershell -ExecutionPolicy Bypass -File scripts\screenshots.ps1 -Release` |
| Regenerate icons | `cargo run --example gen_icons` |
| Windows installer | `cargo build --release; cargo packager --release --formats nsis` |
| Trading212 check (read-only) | `cargo run --example t212_diagnose` (`-- sync` for a full sync) |
| Local AI check | `cargo run --example llm_check` |

`git` is not on PATH in this environment; use
`%LOCALAPPDATA%\GitHubDesktop\app-*\resources\app\git\cmd\git.exe`. It cannot push
(no credentials) — commit locally and ask the user to push.

## Architecture (src/)

| Module | Role |
|---|---|
| `t212/` | Trading212 API client: Basic auth, per-endpoint rate limits, pagination, write guard |
| `portfolio/` | Sync service + on-disk cache (`store.rs`), pure analytics (`analytics.rs`), demo data (`demo.rs`) |
| `insights/` | Phase 4: fundamentals providers, scoring, LLM prompts, insights service |
| `report/` | Branded PDF report (Typst) — "Powered by AI · Made by Akulav" |
| `llm/` | llama.cpp runtime install (`runtime.rs`), GPU/RAM detection, model catalog, server process |
| `market/` | NYSE session clock |
| `ui/` | egui app: `theme.rs` (palette, fonts, icons), `widgets.rs`, one file per page |
| `updater.rs`, `paths.rs`, `secrets.rs`, `config.rs`, `net.rs`, `telemetry.rs` | infrastructure |

Key facts worth knowing:
- Trading212 API keys need permissions per area; a 403 on one endpoint degrades gracefully
  (`portfolio::WARN_ACCOUNT_SCOPE`).
- llama.cpp publishes binaries on per-build **pre-releases**; `runtime::pick_release` handles
  this and falls back to direct downloads when the GitHub API is rate-limited.
- Non-NVIDIA GPUs are detected via the Windows display-driver registry / Linux sysfs.
- UI: use `widgets::grid_row` for card rows (equal widths, equal heights, one gap constant),
  `widgets::card`, `widgets::kpi*`, Phosphor icons (`egui_phosphor::regular as ph`), and
  `theme::semibold` for emphasis. Don't use emoji — egui's fonts lack many glyphs.
- Release builds: Windows x64, Linux x64, macOS Apple Silicon (no Intel Mac builds).
