# Contributing to Aura Portfolio

Thanks for helping! This guide covers the dev setup, the documentation and quality bar, and
step-by-step recipes for the most common extensions.

## Dev setup

```bash
rustup update stable          # MSRV is declared in Cargo.toml (edition 2024)
cargo run                     # debug build; data goes to target/debug/aura-data/
RUST_LOG=aura_portfolio=debug cargo run
cargo test
```

Linux packages: `pkg-config libdbus-1-dev libxkbcommon-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libwayland-dev libgl1-mesa-dev`.

## Quality bar (CI enforces all of these)

| Check | Command |
|---|---|
| Formatting | `cargo fmt --all -- --check` |
| Lints, including **missing docs** | `cargo clippy --all-targets -- -D warnings` |
| Tests | `cargo test` |
| Rustdoc links | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` |

### Documentation standard
`src/lib.rs` sets `#![warn(missing_docs)]`, and CI turns warnings into errors, so **every public item needs `///` docs**.

- **Modules** (`//!`): what the module owns, how data flows, and any invariants (see `llm/server.rs` for the model).
- **API wrappers**: document the endpoint, HTTP method, rate limit, and whether it **mutates state**.
- **AI orchestration**: document blocking vs. async behaviour, which thread to call from, and failure modes.
- Prefer a runnable `# Examples` block for non-trivial public functions.

## Architecture rules

1. **The UI thread never blocks.** Long work runs on the Tokio runtime (`Services::rt`). Shared state goes behind a `parking_lot::Mutex`, and the UI reads a cheap snapshot each frame.
2. **Safety first for trading.** Anything that mutates the brokerage account must check `Settings::trading_mode == ReadWrite` *inside the client*, not only in the UI.
3. **Secrets never touch disk or logs.** Use `secrets::*`, and keep `Debug` impls redacted.
4. **The LLM process is owned by `LlmManager`.** Never spawn `llama-server` anywhere else.

---

## Recipe: add a Trading212 endpoint

```text
src/t212/
├── mod.rs         # T212Client: base URL, Basic auth, retries on 429, pagination, ReadOnly guard
├── models.rs      # serde types (AccountSummary, Position, Dividend, Order, …)
├── endpoints.rs   # one documented method per endpoint
└── rate_limit.rs  # per-endpoint-family pacing (Bucket)
```

The source of truth is the official spec at <https://docs.trading212.com/api>.

1. **Model the response** in `models.rs` with `#[serde(rename_all = "camelCase", default)]`. Document every field, including its units and currency (account vs. instrument currency, and `GBX` = pence). Use `Option` for anything nullable, and `String` for large enums.
2. **Pick or add a `Bucket`** in `rate_limit.rs` with the documented limit.
3. **Add the method** in `endpoints.rs`:

   ```rust
   /// Lists open positions.
   ///
   /// `GET /equity/positions` · 1 req / 1 s · read-only.
   pub async fn positions(&self) -> Result<Vec<Position>> {
       self.get(Bucket::Positions, "/equity/positions").await
   }
   ```

4. **Write endpoints** go through `self.post` / `self.delete`, which call `self.mutating()` and return `T212Error::ReadOnly` *before any network I/O* unless the client was built with `TradingMode::ReadWrite`. Never bypass this.
5. **Paginated endpoints** use `self.paginate(path, max_pages, keep_going)`. Return `false` from `keep_going` to stop early once cached items are reached (see `PortfolioService::sync`).
6. **Test** with a JSON fixture in `tests/fixtures/t212/` and a case in `tests/t212_fixtures.rs`. Strip real ids, references and amounts first.

Derived metrics (tax rate, projections, total return) belong in `portfolio/analytics.rs` as pure functions with unit tests. Keep the UI free of maths.

## Recipe: add an AI analysis

AI features call the local model through `LlmManager::chat`, which uses the OpenAI-compatible `/v1/chat/completions` endpoint of `llama-server`.

1. Create `src/analysis/<name>.rs` with a pure **prompt builder** (unit-testable) and an async **runner**:

   ```rust
   /// Builds the prompt for a cash-flow health check of one holding.
   pub fn cashflow_prompt(f: &Fundamentals) -> Vec<ChatMessage> { /* … */ }

   /// Runs the analysis on the loaded local model.
   pub async fn cashflow_health(llm: &LlmManager, f: &Fundamentals) -> Result<Insight> {
       let reply = llm.chat(cashflow_prompt(f), 700).await?;
       parse_insight(&reply)
   }
   ```

2. **Ask for structured output** (JSON in a fenced block) and parse it defensively. Small models drift, so always have a fallback.
3. **Keep prompts compact.** Pre-compute numbers (P/E, FCF yield, YoY growth) in Rust and give the model only the summary. Context is limited by `Settings::context_size`.
4. **Sentiment sources** (DuckDuckGo) belong in `src/market/sentiment.rs`. Scrape `html.duckduckgo.com/html/`, rate-limit politely, and pass only titles and snippets to the model.
5. Surface results through a snapshot struct, as `LlmState` does, so the UI stays non-blocking.

## Recipe: add or update a model

Edit `CATALOG` in `src/llm/models.rs`:

```rust
ModelSpec {
    id: "my-model-8b-q4km",            // stable, unique: saved in settings
    name: "My Model 8B Instruct",
    params_b: 8.0,
    quant: "Q4_K_M",
    repo: "someone/My-Model-8B-Instruct-GGUF",
    file: "My-Model-8B-Instruct-Q4_K_M.gguf",
    size_bytes: 4_900_000_000,          // from the HF file listing
    layers: 32,                         // from the model card (num_hidden_layers)
    blurb: "One line about what it's good at.",
},
```

Keep the catalog ordered by size, because `recommend()` relies on it. Then run `cargo test` (it checks that ids are unique and recommendations are sane).

## Recipe: support another llama.cpp backend

Add an `AssetRule` in `llm/runtime.rs::asset_rules` (e.g. ROCm/HIP for AMD on Windows), with a detection
flag in `HardwareProfile` if needed, and extend the `select_assets` tests with the real asset name.

## Demo mode & screenshots

- `AURA_DEMO=1` swaps the Trading212 sync for the synthetic portfolio in `src/portfolio/demo.rs`. It makes no network, keychain or cache access, so it's ideal for UI work.
- `AURA_PAGE=dashboard|holdings|dividends|insights|ai|settings` sets the start page.
- `scripts/screenshots.ps1 [-Release]` (Windows) rebuilds the app, captures each page into `docs/screenshots/`, and uses a throwaway data folder. Re-run it after any visible UI change.

## Releasing

1. Bump `version` in `Cargo.toml` (and run `cargo check` so `Cargo.lock` updates).
2. Merge to `main`. `release.yml` detects the new version, builds 4 targets, and publishes `vX.Y.Z` with checksums.
3. Running clients see the **Update available** button on their next launch.
