//! Settings page: Trading212 credentials (OS keychain), trading-mode safety
//! switch, environment, and updater preferences.

use eframe::egui::{self, RichText, Ui};
use egui_phosphor::regular as ph;

use super::theme::palette::*;
use super::widgets::{self, card, chip};
use crate::config::{Settings, T212Environment, TradingMode};
use crate::paths::AppPaths;
use crate::secrets::{self, T212Credentials};

/// Transient form state (never persisted; cleared after saving).
pub struct SettingsPage {
    api_key: String,
    api_secret: String,
    stored: bool,
    message: Option<(bool, String)>,
    confirm_write: bool,
    env_checked: T212Environment,
    credentials_changed: bool,
    clear_cache_requested: bool,
}

impl SettingsPage {
    /// `true` once after credentials were saved or removed.
    pub fn take_credentials_changed(&mut self) -> bool {
        std::mem::take(&mut self.credentials_changed)
    }

    /// `true` once after the user asked to wipe cached portfolio data.
    pub fn take_clear_cache_request(&mut self) -> bool {
        std::mem::take(&mut self.clear_cache_requested)
    }

    /// Creates the form and checks whether credentials exist for `env`.
    pub fn new(env: T212Environment) -> Self {
        Self {
            api_key: String::new(),
            api_secret: String::new(),
            stored: secrets::has_credentials(env),
            message: None,
            confirm_write: false,
            env_checked: env,
            credentials_changed: false,
            clear_cache_requested: false,
        }
    }

    /// Renders the page. Returns `true` if persisted settings changed.
    pub fn show(&mut self, ui: &mut Ui, settings: &mut Settings, paths: &AppPaths) -> bool {
        let mut changed = false;
        if self.env_checked != settings.environment {
            *self = Self::new(settings.environment);
        }

        widgets::page_title(ui, "Settings", "Connection, safety and preferences");
        ui.add_space(18.0);

        card(ui, |ui| {
            widgets::section_title(ui, "Trading212 connection");
            ui.horizontal(|ui| {
                ui.label("Environment");
                for env in [T212Environment::Live, T212Environment::Demo] {
                    changed |= ui
                        .radio_value(&mut settings.environment, env, env.label())
                        .changed();
                }
                if self.stored {
                    chip(
                        ui,
                        &format!("{}  Key stored in OS keychain", ph::KEY),
                        POSITIVE,
                    );
                } else {
                    chip(ui, "No key stored", WARNING);
                }
            });
            ui.add_space(6.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.api_key)
                    .password(true)
                    .hint_text("API key")
                    .desired_width(420.0),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.api_secret)
                    .password(true)
                    .hint_text("API secret (if your key has one)")
                    .desired_width(420.0),
            );
            ui.horizontal(|ui| {
                let can_save = !self.api_key.trim().is_empty();
                if ui
                    .add_enabled(can_save, egui::Button::new("Save to keychain"))
                    .clicked()
                {
                    let creds = T212Credentials {
                        api_key: std::mem::take(&mut self.api_key),
                        api_secret: Some(std::mem::take(&mut self.api_secret)),
                    };
                    self.message = Some(match secrets::store(settings.environment, &creds) {
                        Ok(()) => (true, "Saved securely.".into()),
                        Err(e) => (false, format!("Could not save: {e:#}")),
                    });
                    self.stored = secrets::has_credentials(settings.environment);
                    self.credentials_changed = true;
                }
                if self.stored && ui.button("Remove").clicked() {
                    self.message = Some(match secrets::clear(settings.environment) {
                        Ok(()) => (true, "Removed.".into()),
                        Err(e) => (false, format!("Could not remove: {e:#}")),
                    });
                    self.stored = secrets::has_credentials(settings.environment);
                    self.credentials_changed = true;
                }
            });
            if let Some((ok, msg)) = &self.message {
                ui.colored_label(if *ok { POSITIVE } else { NEGATIVE }, msg);
            }
            ui.label(
                RichText::new("Generate keys in Trading212 → Settings → API. Grant read-only scopes unless you plan to trade.")
                    .size(12.0)
                    .color(MUTED),
            );
        });
        ui.add_space(12.0);

        card(ui, |ui| {
            widgets::section_title(ui, "Trading permissions");
            ui.radio_value(
                &mut settings.trading_mode,
                TradingMode::ReadOnly,
                format!("{}  Read-only (recommended)", ph::LOCK_SIMPLE),
            );
            ui.horizontal(|ui| {
                let enabled = self.confirm_write;
                ui.add_enabled_ui(enabled, |ui| {
                    ui.radio_value(
                        &mut settings.trading_mode,
                        TradingMode::ReadWrite,
                        format!("{}  Read & write (can place orders)", ph::WARNING),
                    );
                });
            });
            ui.checkbox(
                &mut self.confirm_write,
                "I understand Aura may place real orders when write mode is enabled",
            );
            if !self.confirm_write && settings.trading_mode == TradingMode::ReadWrite {
                settings.trading_mode = TradingMode::ReadOnly;
            }
            ui.label(
                RichText::new("Write mode always resets to read-only when Aura restarts.")
                    .size(12.0)
                    .color(MUTED),
            );
        });
        ui.add_space(12.0);

        card(ui, |ui| {
            use crate::insights::fundamentals::Provider;
            widgets::section_title(ui, "AI Insights data");
            ui.label(
                RichText::new("Company fundamentals (P/E, cash flow, margins, quarterly results) for AI Insights and reports.")
                    .size(12.5)
                    .color(MUTED),
            );
            ui.add_space(6.0);
            let is_sec = matches!(settings.fundamentals_provider, Provider::SecEdgar { .. });
            ui.horizontal(|ui| {
                if ui.radio(!is_sec, "None (portfolio data only)").clicked() && is_sec {
                    settings.fundamentals_provider = Provider::None;
                    changed = true;
                }
                if ui
                    .radio(
                        is_sec,
                        "Public filings: US (SEC EDGAR), EU and UK (ESEF), free",
                    )
                    .clicked()
                    && !is_sec
                {
                    settings.fundamentals_provider = Provider::SecEdgar {
                        contact_email: String::new(),
                    };
                    changed = true;
                }
            });
            if let Provider::SecEdgar { contact_email } = &mut settings.fundamentals_provider {
                ui.horizontal(|ui| {
                    ui.label("Contact email (US only)");
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(contact_email)
                                .hint_text("you@example.com")
                                .desired_width(300.0),
                        )
                        .changed();
                    if !contact_email.contains('@') {
                        chip(ui, "needed for US stocks", WARNING);
                    }
                });
                ui.label(
                    RichText::new(
                        "US stocks: SEC's fair-access policy requires a contact email in each request; it is sent only \
                         to sec.gov. EU and UK stocks: annual IFRS reports (ESEF/UKSEF) from filings.xbrl.org, found by \
                         ISIN through GLEIF, converted with ECB rates; no email or key needed. Other markets and ETFs \
                         show “No data”.",
                    )
                    .size(11.5)
                    .color(MUTED),
                );
            }
        });
        ui.add_space(12.0);

        card(ui, |ui| {
            widgets::section_title(ui, "Application");
            changed |= ui
                .checkbox(
                    &mut settings.auto_check_updates,
                    "Check for updates on startup",
                )
                .changed();
            ui.label(
                RichText::new(format!(
                    "Data folder ({}): {}",
                    if paths.portable {
                        "portable (aura-data)"
                    } else {
                        "app folder"
                    },
                    paths.root.display()
                ))
                .size(12.0)
                .monospace()
                .color(MUTED),
            );
            ui.horizontal(|ui| {
                if ui
                    .button(format!("{}  Clear cached portfolio data", ph::TRASH))
                    .on_hover_text("Deletes the local copy of positions, dividends, deposits and value history for this environment")
                    .clicked()
                {
                    self.clear_cache_requested = true;
                }
                ui.label(
                    RichText::new("Value history is only stored locally and cannot be re-downloaded.")
                        .size(11.5)
                        .color(MUTED),
                );
            });
        });
        changed
    }
}
