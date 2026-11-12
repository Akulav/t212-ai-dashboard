# Changelog

All notable changes to Aura Portfolio. Format: [Keep a Changelog](https://keepachangelog.com),
versions follow [SemVer](https://semver.org).

## [2.0.0] - 2026-11-13

### Added
- **Fundamentals for EU and UK stocks**, free and without a key: AI Insights now reads the companies' annual IFRS
  reports (ESEF in the EU, UKSEF in the UK) from filings.xbrl.org, found by ISIN through GLEIF. P/E, PEG, growth,
  margins, ROE, debt/equity, free-cash-flow yield, dividend yield and payout ratio, converted into the quote currency
  with ECB exchange rates (pence for London listings). US stocks still come from SEC EDGAR; the contact email is now
  needed only for them. The setting is now called "Public filings".

### Changed
- **New look**, in the style of Akulav's Hyve and Lumen: a deep blue-black background with soft glows, the sidebar as a floating glass panel with the app name at the top, glass cards, the current page highlighted with a gradient, softer translucent buttons and a clear accent ring around the focused text field.
- **License:** from 2.0.0 on, Aura is source-available under the Akulav Source-Available License (versions up to
  1.1.0 stay MIT).

## [1.1.0] - 2026-11-12

### Changed
- **One file per system** that both installs and updates: `AuraPortfolio-setup.exe` (Windows),
  `AuraPortfolio-linux-x64` (Linux) and `AuraPortfolio-macos-arm64` (macOS Apple Silicon), plus `SHA256SUMS.txt`.
  The bare Windows program, the `.dmg` and the 0.2.x bridge archives are no longer published.
- **You choose the install folder.** Linux and macOS now ask where to install (or offer to update the install that's
  there) instead of always using a fixed folder; on macOS `Aura Portfolio.app` is created inside that folder. The
  Windows setup asks once and later updates reuse the folder.
- **Windows updates run the setup** silently in update mode, into the same folder, then restart Aura. The downloaded
  setup is kept in the app's own `data/downloads` folder, never in `%TEMP%`.
- Updates are also checked to be a program for this system before anything is replaced.
- Uninstalling on Windows deletes only Aura's `data` folder when asked, never other files in the install folder.

### Upgrading from 1.0
- 1.0 looks for the old file names, so it can't update itself to 1.1: download the file for your system from the
  release page once and run it. It finds your existing install and keeps all your data.

## [1.0.0] - 2026-11-06

- 

## [0.3.0] — 2026-11-05

### Added
- **AI Insights (phase 4):** fundamentals per holding, a transparent 0–100 score
  (valuation, growth, quality, income), signals (Opportunity / Hold / Review), a
  buy-opportunity screen, portfolio observations and sector mix.
- **SEC EDGAR fundamentals** for US-listed companies: TTM EPS/revenue/net income/FCF,
  P/E from the live Trading212 price, PEG, FCF yield, margins, ROE, P/B, debt/equity,
  dividend yield and quarterly revenue. Requires a contact email (SEC policy), set in
  Settings → AI Insights data.
- **Local-LLM analysis:** JSON-mode prompts produce a verdict, thesis, strengths and
  risks per holding plus a portfolio summary with recommendations and opportunities.
- **Branded PDF report** (Typst): cover, KPIs, AI summary, opportunities, sectors,
  holdings table, per-holding analysis, income — "Powered by AI · Made by Akulav".
- `CLAUDE.md` project guide (release rules: bump version + changelog on every build).
- Examples: `report_demo` (sample PDF + page previews).

### Changed
- UI symmetry: one 4-column card grid with a single gutter, equal-height card rows,
  tables with fixed columns and right-aligned numbers, solid scrollbar gutter,
  quarterly chart labels, two-column allocation legend.
- Release builds: Windows x64, Linux x64 and macOS Apple Silicon (Intel Mac dropped).

## [0.2.1] — 2026-11-05

### Fixed
- llama.cpp install: upstream now ships binaries on per-build pre-releases; the installer
  scans recent releases, prefers the build named in `nightly-tag.txt`, and falls back to
  direct downloads when the GitHub API is rate-limited.
- AMD / Intel GPUs and their real VRAM are detected (Windows driver registry, Linux sysfs),
  so Vulkan offloads layers instead of running on the CPU.

### Added
- Qwen 3.8 27B in the model catalog (UD-IQ3_XXS 10.9 GB, UD-Q4_K_M 16.5 GB).
- `examples/llm_check` end-to-end hardware / install / device check.

## [0.2.0] — 2026-11-05

### Added
- Installers: NSIS setup (Windows), `.dmg` (macOS), `.deb` + AppImage (Linux).
- Icon embedded in the Windows executable, generated from the in-app logo.
- Demo mode (`AURA_DEMO=1`) and `scripts/screenshots.ps1`.

### Changed
- Redesigned UI: native system fonts, Phosphor icons, full-height sidebar, KPI sparklines,
  allocation donut, market session card.
- Installed builds store data in the user profile; portable archives opt in via a marker file.

### Fixed
- Sync no longer fails when the API key lacks a permission; it degrades and explains.

## [0.1.0] — 2026-11-05

### Added
- First release: egui app shell, Trading212 client, holdings/dividends/tax analytics,
  local llama.cpp runtime + model manager, telemetry, auto-updater, CI/CD.
