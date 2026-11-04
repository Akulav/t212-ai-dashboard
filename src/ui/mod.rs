//! The `egui` application shell: top bar (brand, market bell, updater),
//! navigation rail, page router and footer (credits + LLM telemetry).

pub mod ai;
pub mod charts;
pub mod dashboard;
pub mod dividends;
pub mod format;
pub mod holdings;
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
use theme::palette::*;

/// Top-level pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    fn label(self) -> &'static str {
        match self {
            Page::Dashboard => "📊  Dashboard",
            Page::Holdings => "💼  Holdings",
            Page::Dividends => "💰  Dividends",
            Page::Insights => "💡  AI Insights",
            Page::AiEngine => "🖥  AI Engine",
            Page::Settings => "⚙  Settings",
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
}

/// The application state driven by `eframe`.
pub struct AuraApp {
    services: Services,
    settings: Settings,
    telemetry: Telemetry,
    update: Arc<Mutex<UpdateState>>,
    page: Page,
    logo: egui::TextureHandle,
    settings_page: settings::SettingsPage,
    holdings_page: holdings::HoldingsPage,
}

/// Page title row with sync status, last-sync time and a Refresh button.
pub fn portfolio_header(
    ui: &mut egui::Ui,
    title: &str,
    portfolio: &PortfolioService,
    state: &PortfolioState,
) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new(title).strong());
        ui.add_space(8.0);
        if !state.has_credentials {
            widgets::chip(ui, "Not connected · add your API key in Settings", WARNING);
            return;
        }
        widgets::chip(ui, &format!("T212 · {}", state.environment.label()), ACCENT);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let syncing = portfolio.is_syncing();
            if ui
                .add_enabled(!syncing, egui::Button::new("⟳ Refresh"))
                .clicked()
            {
                portfolio.refresh_now();
            }
            match &state.status {
                SyncStatus::Syncing(step) => {
                    ui.label(RichText::new(step).size(12.0).color(MUTED));
                    ui.spinner();
                }
                SyncStatus::Failed(e) => {
                    ui.label(RichText::new("⚠ sync failed").size(12.0).color(NEGATIVE))
                        .on_hover_text(e);
                }
                SyncStatus::Idle => {
                    if let Some(v) = &state.view
                        && let Some(t) = v.synced_at
                    {
                        ui.label(
                            RichText::new(format!("updated {}", format::ago(t)))
                                .size(12.0)
                                .color(MUTED),
                        );
                    }
                }
            }
        });
    });
}

impl AuraApp {
    /// Builds the app, applies the theme and kicks off the update check.
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        rt: Handle,
        paths: AppPaths,
        http: Client,
        llm: LlmManager,
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

        let app = Self {
            services: Services {
                rt,
                http,
                paths,
                llm,
                portfolio,
            },
            holdings_page: holdings::HoldingsPage::default(),
            settings,
            telemetry,
            update: Arc::new(Mutex::new(UpdateState::Idle)),
            page: Page::Dashboard,
            logo,
            settings_page,
        };
        if app.settings.auto_check_updates {
            app.check_for_updates();
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

    fn check_for_updates(&self) {
        let state = self.update.clone();
        let http = self.services.http.clone();
        *state.lock() = UpdateState::Checking;
        self.services.rt.spawn(async move {
            let next = match updater::check_latest(&http).await {
                Ok(Some(info)) => UpdateState::Available(info),
                Ok(None) => UpdateState::UpToDate,
                Err(e) => {
                    tracing::info!("update check failed: {e:#}");
                    UpdateState::Failed(format!("{e:#}"))
                }
            };
            *state.lock() = next;
        });
    }

    fn start_update(&self, info: updater::UpdateInfo) {
        let state = self.update.clone();
        let http = self.services.http.clone();
        let paths = self.services.paths.clone();
        *state.lock() = UpdateState::Downloading(info.clone(), Default::default());
        self.services.rt.spawn(async move {
            let progress_state = state.clone();
            let progress_info = info.clone();
            let result = updater::install(&http, &paths, &info, move |p| {
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

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_bar")
            .exact_height(60.0)
            .frame(
                egui::Frame::new()
                    .fill(SURFACE)
                    .stroke(Stroke::new(1.0_f32, BORDER))
                    .inner_margin(Margin::symmetric(18, 10)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add(egui::Image::new((self.logo.id(), egui::vec2(34.0, 34.0))));
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(RichText::new("AURA").size(17.0).strong().color(TEXT));
                        ui.label(
                            RichText::new("PORTFOLIO INTELLIGENCE")
                                .size(9.5)
                                .color(MUTED),
                        );
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        self.update_button(ui);
                        ui.add_space(8.0);
                        let mode = match self.settings.trading_mode {
                            TradingMode::ReadOnly => ("🔒 READ-ONLY", ACCENT),
                            TradingMode::ReadWrite => ("⚠ TRADING ENABLED", WARNING),
                        };
                        widgets::chip(ui, mode.0, mode.1);
                        ui.add_space(8.0);
                        market_bell(ui);
                    });
                });
            });
    }

    fn update_button(&mut self, ui: &mut egui::Ui) {
        let state = self.update.lock().clone();
        match state {
            UpdateState::Available(info) => {
                let btn = egui::Button::new(
                    RichText::new(format!("⬆  Update to v{}", info.version))
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
                    RichText::new(format!("↻  Restart into v{v}"))
                        .strong()
                        .color(BG),
                )
                .fill(POSITIVE)
                .corner_radius(egui::CornerRadius::same(255));
                if ui.add(btn).clicked() {
                    self.save_settings();
                    self.services.llm.shutdown();
                    if let Err(e) = updater::restart() {
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
            UpdateState::UpToDate | UpdateState::Idle | UpdateState::Failed(_) => {
                ui.label(
                    RichText::new(format!("v{}", crate::VERSION))
                        .size(12.0)
                        .color(MUTED),
                );
            }
        }
    }

    fn nav_rail(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("nav")
            .exact_width(212.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(SURFACE)
                    .stroke(Stroke::new(1.0_f32, BORDER))
                    .inner_margin(Margin::symmetric(12, 16)),
            )
            .show(ctx, |ui| {
                widgets::section_title(ui, "Navigate");
                ui.add_space(6.0);
                for page in Page::ALL {
                    let selected = self.page == page;
                    let text = RichText::new(page.label()).size(14.5).color(if selected {
                        TEXT
                    } else {
                        MUTED
                    });
                    let btn = egui::Button::new(text)
                        .fill(if selected {
                            ACCENT.linear_multiply(0.14)
                        } else {
                            egui::Color32::TRANSPARENT
                        })
                        .stroke(if selected {
                            Stroke::new(1.0_f32, ACCENT.linear_multiply(0.5))
                        } else {
                            Stroke::NONE
                        })
                        .min_size(egui::vec2(ui.available_width(), 38.0));
                    if ui.add(btn).clicked() {
                        self.page = page;
                    }
                }
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

fn market_bell(ui: &mut egui::Ui) {
    let clock = hours::nyse_clock(chrono::Utc::now());
    let (label, color) = match clock.phase {
        MarketPhase::Open => ("NYSE OPEN · closes in", POSITIVE),
        MarketPhase::Closed => ("NYSE CLOSED · opens in", MUTED),
    };
    widgets::chip(ui, &format!("🔔 {label} {}", clock.countdown()), color);
}

impl eframe::App for AuraApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.services.portfolio.tick();
        let portfolio_state = self.services.portfolio.snapshot();

        self.top_bar(ctx);
        self.nav_rail(ctx);
        self.footer(ctx);

        let env_before = self.settings.environment;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(Margin::same(22)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.page {
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
                        Page::Insights => widgets::empty_state(
                            ui,
                            "💡",
                            "AI Insights",
                            "Fundamental + sentiment reports and PDF export arrive in phase 4.",
                        ),
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
