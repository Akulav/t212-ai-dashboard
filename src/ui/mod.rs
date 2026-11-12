//! The `egui` application shell: top bar (brand, market bell, updater),
//! navigation rail, page router and footer (credits + LLM telemetry).

pub mod ai;
pub mod charts;
pub mod dashboard;
pub mod dividends;
pub mod format;
pub mod holdings;
pub mod insights;
pub mod install_window;
pub mod settings;
pub mod theme;
pub mod widgets;

use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{self, Align, Layout, Margin, RichText, Stroke};
use parking_lot::Mutex;
use reqwest::Client;
use tokio::runtime::Handle;

use crate::config::{Settings, TradingMode};
use crate::llm::LlmManager;
use crate::market::hours::{self, MarketPhase};
use crate::paths::AppPaths;
use crate::portfolio::{PortfolioService, PortfolioState, SyncStatus};
use crate::telemetry::Telemetry;
use crate::updater::{self, UpdateState};
use egui_phosphor::regular as ph;
use theme::palette::*;

/// Top-level pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    /// Portfolio overview and market context.
    Dashboard,
    /// Positions table.
    Holdings,
    /// Dividend history and calendar.
    Dividends,
    /// AI fundamental & sentiment reports.
    Insights,
    /// Local LLM runtime and models.
    AiEngine,
    /// Credentials, safety and preferences.
    Settings,
}

impl Page {
    const ALL: [Page; 6] = [
        Page::Dashboard,
        Page::Holdings,
        Page::Dividends,
        Page::Insights,
        Page::AiEngine,
        Page::Settings,
    ];

    /// Start page from `AURA_PAGE` (`dashboard`, `holdings`, `dividends`,
    /// `insights`, `ai`, `settings`); used by `scripts/screenshots.ps1`.
    fn from_env() -> Self {
        match std::env::var("AURA_PAGE")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "holdings" => Page::Holdings,
            "dividends" => Page::Dividends,
            "insights" => Page::Insights,
            "ai" | "ai-engine" => Page::AiEngine,
            "settings" => Page::Settings,
            _ => Page::Dashboard,
        }
    }

    /// `(icon, label)` for the navigation rail.
    fn nav(self) -> (&'static str, &'static str) {
        match self {
            Page::Dashboard => (ph::SQUARES_FOUR, "Overview"),
            Page::Holdings => (ph::BRIEFCASE, "Holdings"),
            Page::Dividends => (ph::COINS, "Dividends"),
            Page::Insights => (ph::SPARKLE, "AI Insights"),
            Page::AiEngine => (ph::CPU, "AI Engine"),
            Page::Settings => (ph::GEAR_SIX, "Settings"),
        }
    }

    /// Navigation group heading shown above this page, if it starts a group.
    fn group(self) -> Option<&'static str> {
        match self {
            Page::Dashboard => Some("Portfolio"),
            Page::Insights => Some("Intelligence"),
            Page::Settings => Some("App"),
            _ => None,
        }
    }
}

/// Shared services every page can use.
pub struct Services {
    /// Tokio runtime handle for background work.
    pub rt: Handle,
    /// Shared HTTP client.
    pub http: Client,
    /// Data directories.
    pub paths: AppPaths,
    /// Local LLM subsystem.
    pub llm: LlmManager,
    /// Trading212 sync service.
    pub portfolio: PortfolioService,
    /// Phase-4 insights engine.
    pub insights: crate::insights::InsightsService,
}

/// The application state driven by `eframe`.
pub struct AuraApp {
    services: Services,
    settings: Settings,
    telemetry: Telemetry,
    update: Arc<Mutex<UpdateState>>,
    page: Page,
    /// Page rendered last frame (scroll reset on change).
    shown_page: Option<Page>,
    logo: egui::TextureHandle,
    settings_page: settings::SettingsPage,
    holdings_page: holdings::HoldingsPage,
    insights_page: insights::InsightsPage,
}

/// Page header: title + subtitle on the left, connection chip, sync status
/// and Refresh on the right, then any sync error/warnings as banners.
pub fn portfolio_header(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    portfolio: &PortfolioService,
    state: &PortfolioState,
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| widgets::page_title(ui, title, subtitle));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if state.demo {
                widgets::chip(
                    ui,
                    &format!("{}  DEMO DATA · sample portfolio", ph::FLASK),
                    GOLD,
                );
                return;
            }
            if !state.has_credentials {
                widgets::chip(ui, &format!("{}  Not connected", ph::PLUGS), WARNING);
                return;
            }
            let syncing = portfolio.is_syncing();
            if ui
                .add_enabled(
                    !syncing,
                    egui::Button::new(format!("{}  Refresh", ph::ARROWS_CLOCKWISE)),
                )
                .clicked()
            {
                portfolio.refresh_now();
            }
            match &state.status {
                SyncStatus::Syncing(step) => {
                    ui.label(RichText::new(step).size(12.5).color(MUTED));
                    ui.spinner();
                }
                SyncStatus::Failed(_) => {
                    ui.label(RichText::new("sync failed").size(12.5).color(NEGATIVE));
                }
                SyncStatus::Idle => {
                    if let Some(t) = state.view.as_ref().and_then(|v| v.synced_at) {
                        ui.label(
                            RichText::new(format!("updated {}", format::ago(t)))
                                .size(12.5)
                                .color(MUTED),
                        );
                    }
                }
            }
            widgets::chip(
                ui,
                &format!(
                    "{}  Trading212 · {}",
                    ph::PLUGS_CONNECTED,
                    state.environment.label()
                ),
                ACCENT,
            );
        });
    });
    if !state.has_credentials && !state.demo {
        ui.add_space(10.0);
        widgets::banner(
            ui,
            ph::KEY,
            "Connect your Trading212 account: open Settings and paste your API key and secret. They are stored in your OS keychain.",
            ACCENT,
        );
    }
    if let SyncStatus::Failed(e) = &state.status {
        ui.add_space(10.0);
        widgets::banner(
            ui,
            ph::WARNING_OCTAGON,
            &format!("Sync failed: {e}"),
            NEGATIVE,
        );
    }
    for w in &state.warnings {
        ui.add_space(10.0);
        widgets::banner(ui, ph::WARNING, w, WARNING);
    }
}

impl AuraApp {
    /// Builds the app, applies the theme and starts the update checks (a minute after start, then every 6 hours;
    /// never for a program built on this computer — `built_here`).
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        rt: Handle,
        paths: AppPaths,
        http: Client,
        llm: LlmManager,
        built_here: bool,
    ) -> Self {
        theme::apply(&cc.egui_ctx);
        let settings = Settings::load(&paths.config_file);
        let telemetry = Telemetry::start(llm.pid_handle());
        let logo = theme::logo_texture(&cc.egui_ctx);
        let settings_page = settings::SettingsPage::new(settings.environment);
        let portfolio = PortfolioService::new(
            rt.clone(),
            http.clone(),
            paths.clone(),
            settings.environment,
        );
        let provider = if crate::portfolio::demo::enabled() {
            crate::insights::fundamentals::Provider::Demo
        } else {
            settings.fundamentals_provider.clone()
        };
        let insights = crate::insights::InsightsService::new(
            rt.clone(),
            http.clone(),
            paths.clone(),
            llm.clone(),
            provider,
        );

        let app = Self {
            services: Services {
                rt,
                http,
                paths,
                llm,
                portfolio,
                insights,
            },
            holdings_page: holdings::HoldingsPage::default(),
            insights_page: insights::InsightsPage::default(),
            settings,
            telemetry,
            update: Arc::new(Mutex::new(if built_here {
                UpdateState::SelfBuilt
            } else {
                UpdateState::Idle
            })),
            page: Page::from_env(),
            shown_page: None,
            logo,
            settings_page,
        };
        if !built_here {
            app.schedule_update_checks();
        }
        if app.settings.auto_load_model
            && let Some(spec) = app
                .settings
                .selected_model
                .as_deref()
                .and_then(crate::llm::models::find)
            && let Err(e) = app.services.llm.load_model(spec, app.settings.context_size)
        {
            tracing::warn!("auto-load failed: {e:#}");
        }
        app
    }

    /// Background checks: a minute after start, then every 6 hours (when enabled in Settings).
    fn schedule_update_checks(&self) {
        let (state, http, auto) = (
            self.update.clone(),
            self.services.http.clone(),
            self.settings.auto_check_updates,
        );
        let token = updater::github_token(&self.services.paths.root);
        self.services.rt.spawn(async move {
            if !auto {
                return;
            }
            tokio::time::sleep(updater::FIRST_CHECK).await;
            loop {
                // (never interrupt a download or a pending restart)
                if matches!(
                    *state.lock(),
                    UpdateState::Idle | UpdateState::UpToDate | UpdateState::Failed(_)
                ) {
                    run_check(&state, &http, token.as_deref()).await;
                }
                tokio::time::sleep(updater::CHECK_EVERY).await;
            }
        });
    }

    /// "Check for updates" in the sidebar.
    fn check_for_updates(&self) {
        let (state, http) = (self.update.clone(), self.services.http.clone());
        let token = updater::github_token(&self.services.paths.root);
        self.services
            .rt
            .spawn(async move { run_check(&state, &http, token.as_deref()).await });
    }

    fn start_update(&self, info: updater::UpdateInfo) {
        let state = self.update.clone();
        let http = self.services.http.clone();
        let token = updater::github_token(&self.services.paths.root);
        let downloads = self.services.paths.downloads.clone();
        *state.lock() = UpdateState::Downloading(info.clone(), Default::default());
        self.services.rt.spawn(async move {
            let progress_state = state.clone();
            let progress_info = info.clone();
            let result = updater::install(&http, &info, token.as_deref(), &downloads, move |p| {
                *progress_state.lock() = UpdateState::Downloading(progress_info.clone(), p);
            })
            .await;
            *state.lock() = match result {
                Ok(()) => UpdateState::ReadyToRestart(info.version),
                Err(e) => UpdateState::Failed(format!("{e:#}")),
            };
        });
    }

    fn save_settings(&self) {
        if let Err(e) = self.settings.save(&self.services.paths.config_file) {
            tracing::error!("saving settings failed: {e:#}");
        }
    }

    fn update_button(&mut self, ui: &mut egui::Ui) {
        let state = self.update.lock().clone();
        match state {
            UpdateState::Available(info) => {
                let btn = egui::Button::new(
                    RichText::new(format!(
                        "{}  Update to v{}",
                        ph::ARROW_CIRCLE_UP,
                        info.version
                    ))
                    .strong()
                    .color(BG),
                )
                .fill(ACCENT)
                .corner_radius(egui::CornerRadius::same(255));
                if ui
                    .add(btn)
                    .on_hover_text("Download, verify and install the new version")
                    .clicked()
                {
                    self.start_update(info);
                }
            }
            UpdateState::Downloading(_, p) => {
                let frac = p.fraction().unwrap_or(0.0);
                ui.add(
                    egui::ProgressBar::new(frac)
                        .desired_width(160.0)
                        .text(format!("Updating… {:.0}%", frac * 100.0)),
                );
            }
            UpdateState::ReadyToRestart(v) => {
                let btn = egui::Button::new(
                    RichText::new(format!("{}  Restart into v{v}", ph::ARROW_CLOCKWISE))
                        .strong()
                        .color(BG),
                )
                .fill(POSITIVE)
                .corner_radius(egui::CornerRadius::same(255));
                if ui.add(btn).clicked() {
                    self.save_settings();
                    self.services.llm.shutdown();
                    if let Err(e) = updater::restart(&self.services.paths.downloads) {
                        *self.update.lock() = UpdateState::Failed(format!("{e:#}"));
                    }
                }
            }
            UpdateState::Checking => {
                ui.label(
                    RichText::new("checking for updates…")
                        .size(12.0)
                        .color(MUTED),
                );
            }
            UpdateState::SelfBuilt => {
                ui.label(
                    RichText::new(format!("v{} · built locally", crate::VERSION))
                        .size(12.0)
                        .color(MUTED),
                )
                .on_hover_text("Built on this computer: it never updates or installs itself.");
            }
            UpdateState::UpToDate | UpdateState::Idle | UpdateState::Failed(_) => {
                let hint = match &state {
                    UpdateState::UpToDate => "You have the latest version.".to_string(),
                    UpdateState::Failed(e) => format!("Last check failed: {e}"),
                    _ => "Check GitHub for a newer release".to_string(),
                };
                ui.label(
                    RichText::new(format!("v{}", crate::VERSION))
                        .size(12.0)
                        .color(MUTED),
                );
                if ui
                    .small_button(format!("{}  Check for updates", ph::ARROWS_CLOCKWISE))
                    .on_hover_text(hint)
                    .clicked()
                {
                    *self.update.lock() = UpdateState::Checking;
                    self.check_for_updates();
                }
            }
        }
    }

    /// Full-height sidebar: brand, grouped navigation, and a status stack
    /// (market bell, trading mode, version/update) pinned to the bottom.
    fn nav_rail(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("nav")
            .exact_width(232.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(SURFACE)
                    .stroke(Stroke::new(1.0_f32, BORDER))
                    .inner_margin(Margin {
                        left: 14,
                        right: 14,
                        top: 18,
                        bottom: 14,
                    }),
            )
            .show(ctx, |ui| {
                // Brand.
                ui.horizontal(|ui| {
                    ui.add(egui::Image::new((self.logo.id(), egui::vec2(36.0, 36.0))));
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(
                            RichText::new("Aura")
                                .font(theme::semibold(19.0))
                                .color(TEXT),
                        );
                        ui.label(
                            RichText::new("Portfolio intelligence")
                                .size(11.5)
                                .color(MUTED),
                        );
                    });
                });
                ui.add_space(18.0);

                for page in Page::ALL {
                    if let Some(group) = page.group() {
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            widgets::section_title(ui, group);
                        });
                        ui.add_space(2.0);
                    }
                    if nav_item(ui, page.nav(), self.page == page).clicked() {
                        self.page = page;
                    }
                }

                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.horizontal(|ui| self.update_button(ui));
                    ui.add_space(6.0);
                    let (icon, text, color) = match self.settings.trading_mode {
                        TradingMode::ReadOnly => (ph::LOCK_SIMPLE, "Read-only", ACCENT),
                        TradingMode::ReadWrite => (ph::WARNING, "Trading enabled", WARNING),
                    };
                    widgets::chip(ui, &format!("{icon}  {text}"), color);
                    ui.add_space(6.0);
                    market_bell(ui);
                });
            });
    }

    fn footer(&self, ctx: &egui::Context) {
        let telemetry = self.telemetry.snapshot();
        egui::TopBottomPanel::bottom("footer")
            .exact_height(34.0)
            .frame(
                egui::Frame::new()
                    .fill(SURFACE)
                    .stroke(Stroke::new(1.0_f32, BORDER))
                    .inner_margin(Margin::symmetric(16, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;
                    widgets::telemetry_compact(ui, &telemetry);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} v{}", crate::APP_NAME, crate::VERSION))
                                .size(11.5)
                                .color(MUTED),
                        );
                        ui.label(RichText::new("·").color(MUTED));
                        widgets::made_by(ui);
                    });
                });
            });
    }
}

/// Sidebar navigation row: icon + label, accent bar and tint when active.
fn nav_item(ui: &mut egui::Ui, (icon, label): (&str, &str), selected: bool) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), egui::Sense::click());
    let p = ui.painter();
    let hovered = resp.hovered();
    if selected || hovered {
        let fill = if selected {
            ACCENT.linear_multiply(0.12)
        } else {
            SURFACE_2
        };
        p.rect_filled(rect, egui::CornerRadius::same(10), fill);
    }
    if selected {
        let bar = egui::Rect::from_min_size(
            rect.left_top() + egui::vec2(0.0, 10.0),
            egui::vec2(3.0, 20.0),
        );
        p.rect_filled(bar, egui::CornerRadius::same(2), ACCENT);
    }
    let color = if selected {
        TEXT
    } else if hovered {
        TEXT.linear_multiply(0.85)
    } else {
        MUTED
    };
    let icon_color = if selected { ACCENT } else { color };
    p.text(
        rect.left_center() + egui::vec2(16.0, 0.0),
        egui::Align2::LEFT_CENTER,
        icon,
        egui::FontId::proportional(18.0),
        icon_color,
    );
    let font = if selected {
        theme::semibold(14.5)
    } else {
        egui::FontId::proportional(14.5)
    };
    p.text(
        rect.left_center() + egui::vec2(44.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        color,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// NYSE session status for the sidebar.
fn market_bell(ui: &mut egui::Ui) {
    let clock = hours::nyse_clock(chrono::Utc::now());
    let (label, color) = match clock.phase {
        MarketPhase::Open => ("NYSE closes in", POSITIVE),
        MarketPhase::Closed => ("NYSE opens in", MUTED),
    };
    widgets::chip(
        ui,
        &format!("{}  {label} {}", ph::BELL_SIMPLE, clock.countdown()),
        color,
    );
}

impl eframe::App for AuraApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.services.portfolio.tick();
        let portfolio_state = self.services.portfolio.snapshot();

        // Sidebar first so it spans the full window height.
        self.nav_rail(ctx);
        self.footer(ctx);

        let env_before = self.settings.environment;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(Margin {
                left: 28,
                right: 28,
                top: 24,
                bottom: 16,
            }))
            .show(ctx, |ui| {
                // One scroll state per page, starting at the top whenever the
                // page is (re)opened, including on launch.
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt(self.page)
                    .auto_shrink([false, false]);
                if self.shown_page != Some(self.page) {
                    scroll = scroll.vertical_scroll_offset(0.0);
                    self.shown_page = Some(self.page);
                }
                scroll.show(ui, |ui| match self.page {
                    Page::Dashboard => {
                        dashboard::show(ui, &self.services.portfolio, &portfolio_state)
                    }
                    Page::Holdings => {
                        self.holdings_page
                            .show(ui, &self.services.portfolio, &portfolio_state)
                    }
                    Page::Dividends => {
                        dividends::show(ui, &self.services.portfolio, &portfolio_state)
                    }
                    Page::Insights => self
                        .insights_page
                        .show(ui, &self.services, &portfolio_state),
                    Page::AiEngine => {
                        if ai::show(
                            ui,
                            &self.services,
                            &mut self.settings,
                            &self.telemetry.snapshot(),
                        ) {
                            self.save_settings();
                        }
                    }
                    Page::Settings => {
                        if self
                            .settings_page
                            .show(ui, &mut self.settings, &self.services.paths)
                        {
                            self.save_settings();
                            if !crate::portfolio::demo::enabled() {
                                self.services
                                    .insights
                                    .set_provider(self.settings.fundamentals_provider.clone());
                            }
                        }
                    }
                });
            });

        if self.settings.environment != env_before {
            self.services
                .portfolio
                .set_environment(self.settings.environment);
        }
        if self.settings_page.take_credentials_changed() {
            self.services.portfolio.credentials_changed();
        }
        if self.settings_page.take_clear_cache_request() {
            self.services.portfolio.clear_cache();
        }

        // Live clock + telemetry need ~1 Hz; downloads/loading animate faster.
        let busy = self.services.llm.snapshot().is_busy()
            || self.services.portfolio.is_syncing()
            || self.services.insights.is_running()
            || self.insights_page.is_busy()
            || matches!(
                *self.update.lock(),
                UpdateState::Downloading(..) | UpdateState::Checking
            );
        ctx.request_repaint_after(if busy {
            Duration::from_millis(200)
        } else {
            Duration::from_secs(1)
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Forcefully stop llama-server and release RAM/VRAM before the window closes.
        self.services.llm.shutdown();
        self.save_settings();
    }
}

/// One update check: updates the shared state.
async fn run_check(state: &Mutex<UpdateState>, http: &Client, token: Option<&str>) {
    *state.lock() = UpdateState::Checking;
    let next = match updater::check_latest(http, token).await {
        Ok(Some(info)) => UpdateState::Available(info),
        Ok(None) => UpdateState::UpToDate,
        Err(e) => {
            tracing::info!("update check failed: {e:#}");
            UpdateState::Failed(format!("{e:#}"))
        }
    };
    *state.lock() = next;
}
