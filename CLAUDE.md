# CLAUDE.md — Aura Portfolio

Native Rust desktop app (egui/eframe window app) for Trading212 portfolio analytics with a local `llama.cpp` AI
analyst. App name **Aura Portfolio**, program `aura-portfolio` (`aura-portfolio.exe`), publisher **Akulav**, repo
`Akulav/t212-ai-dashboard` (public). Every release has exactly one file per system, which both installs and updates:
`AuraPortfolio-setup.exe` (Windows x64, Inno Setup), `AuraPortfolio-linux-x64`, `AuraPortfolio-macos-arm64`, plus
`SHA256SUMS.txt` (`updater::asset_name`). One workflow, `.github/workflows/build.yml`, lints/tests/builds and releases.

## Always bump the version and commit (every change that ships, no exceptions)

The release workflow publishes only when `VERSION` changes to a version that has no tag yet; a push that keeps the old
version is skipped on purpose (the release jobs show "Skipped"). So every piece of work that changes the app ends with:

1. **Bump** with `python tools_dev/bump_version.py X.Y.Z` — never edit the version files by hand. Base the number on
   the newest tag (`git fetch --tags; git tag --sort=-v:refname`); if `v<VERSION>` exists, bump past it.
   Patch = fixes only · minor = new features or behaviour · major = breaking changes to data, config or install layout.
2. **Fill in the new CHANGELOG section** for the person using the app, under `###` headings, in plain words. Never leave
   the `- ` placeholder, and never rewrite notes of a tagged version (compare with `git show v<old>:CHANGELOG.md`).
3. **Check it builds** with `--locked`: `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets -- -D warnings`,
   `cargo test --locked`, `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked`. The bump changes `Cargo.lock`:
   build once and commit it too.
4. **Commit** everything from the task in one commit: `X.Y.Z: <one-line summary>` (+ a short body). Stage the files you
   changed by name — never `git add -A` blindly. Never commit secrets or local config (`data/local.config`, tokens,
   `.env`), never `--no-verify`, never amend a pushed commit.
5. **Pushing to main publishes a release**: ask the user before pushing unless they said to push. Afterwards check the
   Actions run; "Skipped" release jobs mean the version wasn't new — go back to step 1.

If a task truly doesn't change what ships (e.g. comments in a non-shipped file), say so and ask whether to bump.

`git` isn't on PATH here: use `%LOCALAPPDATA%\GitHubDesktop\app-*\resources\app\git\cmd\git.exe`. It can't push
(no credentials): commit and ask the user to push from GitHub Desktop.

## Never

- **Never change the setup's AppId** `{6208BE64-B4D0-4EDB-BF3A-510EE88F9FB5}` (`installer/aura-portfolio.iss`, and
  `install::registry::SETUP_KEY`): upgrades and the uninstaller find the install by it.
- Never rename the release assets (`AuraPortfolio-setup.exe`, `AuraPortfolio-linux-x64`, `AuraPortfolio-macos-arm64`,
  `SHA256SUMS.txt`) and never add more: installed copies look for exactly these names.
- Never write outside the install folder the user chose (everything goes in `<folder>/data`); the only exceptions are
  the system's own app entries (shortcuts, Apps entry, Linux menu entry) and the keychain.
- Never log or print API keys; trading stays read-only unless enabled per session (`T212Client::mutating()`).
- Never commit real portfolio data: screenshots and fixtures use demo/synthetic data only.
- Never test install/uninstall against the real `%LOCALAPPDATA%\Aura Portfolio`: point `LOCALAPPDATA`/`APPDATA` at a
  scratch folder, stop only processes inside it, remove test registry entries afterwards.

## Commands

| Task | Command |
|---|---|
| Run (dev; never installs or updates itself) | `cargo run` |
| Run with sample data | `AURA_DEMO=1 cargo run` (PowerShell: `$env:AURA_DEMO=1; cargo run`) |
| Open a page directly | `AURA_PAGE=dashboard\|holdings\|dividends\|insights\|ai\|settings` |
| Bump the version | `python tools_dev/bump_version.py X.Y.Z` |
| Redraw all icons from `assets/icon.svg` | `python tools_dev/make_icons.py` (Pillow) — commit the outputs |
| Windows setup | `cargo build --release; & "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" /DAppVersion=X.Y.Z /DExe=$PWD\target\release\aura-portfolio.exe installer\aura-portfolio.iss` (→ `dist\AuraPortfolio-setup.exe`) |
| Screenshots (Windows) | `powershell -ExecutionPolicy Bypass -File scripts\screenshots.ps1 -Release` |
| Trading212 check (read-only) | `cargo run --example t212_diagnose` (`-- sync` for a full sync) |
| Local AI check | `cargo run --example llm_check` |
| Sample PDF + page previews | `cargo run --example report_demo` |

Program options: `--version`, `--check-update`, `--here`, `--dir <folder>`, `--uninstall` (and `--after-update`,
internal).

## Install, update, uninstall (how it fits together)

- **App folder** = the folder the user chose: program (macOS: `Aura Portfolio.app` holding it) + `data/` (settings,
  cache, insights, models, runtime, logs, downloads). Suggested defaults (`install::default_dir`): Windows
  `%LOCALAPPDATA%\Aura Portfolio`, Linux `$XDG_DATA_HOME|~/.local/share/aura-portfolio`, macOS
  `~/Applications/Aura Portfolio`. `install::root_of_exe` maps a bundled program to its folder. Pre-1.0 data
  (`…/akulav/AuraPortfolio`) is moved in once (`paths::move_legacy_data`); legacy portable `aura-data/` keeps working.
- **Windows install/update**: only the Inno setup (`DisableDirPage=auto` + `UsePreviousAppDir`: asks once). It writes
  the `.aura-app` marker and starts the app with `--after-update` when run with `/UPDATE`.
- **Linux/macOS install** (`install.rs` + `ui/install_window.rs`): outside an app folder (markers `.aura-app`, `.git`,
  `unins000.exe`, `portable`, `aura-data`) with a desktop, a small window asks for the folder (or offers to update the
  install found via the Linux menu entry / default folder); then copy, menu entry or app bundle, start detached, exit.
  "Just run it" restarts the program with `--here`. Opt-outs: `--here`, `--dir`, `AURA_DATA_DIR`, `--after-update`,
  `target/` builds, Linux without DISPLAY/WAYLAND. Never downgrades (asks the installed copy `--version`).
- **One instance per app folder**: `data/.aura.lock` (`File::try_lock`); a second start brings the window forward.
- **Updates** (`updater.rs`): a minute after start + every 6 h + "Check for updates"; asset → SHA256SUMS.txt + magic
  bytes → Windows: setup saved to `data/downloads`, run `/VERYSILENT /UPDATE /DIR=<folder>`; Linux/macOS: rename to
  `.old` → swap → (re-sign the bundle) → restart with `--after-update`. `target/` builds never update.
- **Uninstall**: Inno uninstaller (stops `{app}\aura-portfolio.exe`, asks before deleting `{app}\data`), or
  `--uninstall` (removes menu entries / the bundle, asks before deleting the folder).

## Quality rules

- Public items need `///` docs (`#![warn(missing_docs)]`); CI turns warnings into errors on all three OSes. Code behind
  `#[cfg(target_os = …)]` is only linted by that OS's CI job — keep constants/helpers used by all platforms or gate them.
- `.cargo/config.toml` raises rustc's stack (Typst crates overflow it otherwise); dev dependencies build without debug
  info (the test link otherwise runs out of memory).
- UI: `widgets::card_row` grid (one gap, equal-height rows), `widgets::Kpi`, Phosphor icons, `theme::semibold`; no
  emoji. Verify visually with the screenshot script before committing UI changes.
- AI output is labelled as AI-generated and not financial advice, in the UI and in reports.

## Architecture (src/)

| Module | Role |
|---|---|
| `main.rs`, `install.rs`, `updater.rs`, `paths.rs` | start-up, self-install, updates, the data folder |
| `t212/` | Trading212 API client: Basic auth, per-endpoint rate limits, pagination, write guard |
| `portfolio/` | sync service + cache (`store.rs`), pure analytics, demo data |
| `insights/` | fundamentals (SEC EDGAR `sec.rs` for US, ESEF/UKSEF `esef.rs` for EU/UK; `cargo run --example esef_check -- <ISIN> <CCY> <price>`), scoring, LLM prompts, insights service |
| `report/` | branded PDF report (Typst) — "Powered by AI · Made by Akulav" |
| `llm/` | llama.cpp runtime install, GPU/RAM detection, model catalog, server process |
| `ui/` | egui app: theme, widgets, one file per page |

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships. It is local only (`graphify-out/` is git-ignored).

Rules:
- When graphify-out/graph.json exists, run `graphify query "<question>"` before your first grep, search or source-file read for a codebase question, and put this rule in every subagent prompt that explores code. Read raw files only for lines the graph or an earlier read already located. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost). If graphify-out/ is missing, rebuild it the same way.