# Changelog

All notable changes to Aura Portfolio. Format: [Keep a Changelog](https://keepachangelog.com),
versions follow [SemVer](https://semver.org).

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
