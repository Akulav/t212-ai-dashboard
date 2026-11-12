//! The small window a downloaded Linux/macOS program shows before anything else: install into a folder you pick
//! (or update the install that's there), or run once without installing. It runs its own event loop and the process
//! ends after it ([`crate::install`] starts the installed program, or this program again with `--here`), so the main
//! window never shares a process with it.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, RichText};
use parking_lot::Mutex;

use super::theme::{self, palette::*};
use crate::install::{self, Offer};

/// What the person chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Install into (or update) this folder.
    Install(PathBuf),
    /// Run the downloaded program where it is, without installing.
    RunHere,
    /// Closed the window.
    Cancel,
}

struct InstallWindow {
    offer: Offer,
    folder: String,
    error: Option<String>,
    choice: Arc<Mutex<Choice>>,
    logo: egui::TextureHandle,
}

impl eframe::App for InstallWindow {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut decided = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(28))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Image::new(&self.logo).fit_to_exact_size(egui::vec2(56.0, 56.0)));
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(install::NAME).font(theme::semibold(24.0)).color(TEXT));
                        ui.label(
                            RichText::new(format!("Version {} · made by Akulav", crate::VERSION))
                                .size(13.0)
                                .color(MUTED),
                        );
                    });
                });
                ui.add_space(18.0);
                match &self.offer {
                    Offer::Update { root, installed } => {
                        ui.label(
                            RichText::new(format!(
                                "{} {} is installed in\n{}\n\nUpdate it to {}? Your data, models and settings stay.",
                                install::NAME,
                                installed.as_deref().unwrap_or("(an older version)"),
                                root.display(),
                                crate::VERSION
                            ))
                            .color(TEXT),
                        );
                        ui.add_space(18.0);
                        ui.horizontal(|ui| {
                            if accent_button(ui, "Update").clicked() {
                                decided = Some(Choice::Install(root.clone()));
                            }
                            if ui.button("Just run it").clicked() {
                                decided = Some(Choice::RunHere);
                            }
                        });
                    }
                    _ => {
                        ui.label(
                            RichText::new(
                                "Choose where to install. The app and everything it keeps (settings, portfolio \
                                 cache, AI models, the llama.cpp runtime, logs) live in this folder only.",
                            )
                            .color(TEXT),
                        );
                        ui.add_space(10.0);
                        let edit = egui::TextEdit::singleline(&mut self.folder)
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace);
                        let response = ui.add(edit);
                        if response.changed() {
                            self.error = None;
                        }
                        if let Some(e) = &self.error {
                            ui.label(RichText::new(e).color(NEGATIVE).size(12.0));
                        }
                        ui.add_space(18.0);
                        ui.horizontal(|ui| {
                            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if accent_button(ui, "Install").clicked() || enter {
                                match install::parse_dir(&self.folder) {
                                    Ok(dir) => decided = Some(Choice::Install(dir)),
                                    Err(e) => self.error = Some(e),
                                }
                            }
                            if ui
                                .button("Just run it")
                                .on_hover_text("Run once from where it is; its data is kept next to it")
                                .clicked()
                            {
                                decided = Some(Choice::RunHere);
                            }
                        });
                    }
                }
            });
        if let Some(c) = decided {
            *self.choice.lock() = c;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn accent_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).strong().color(BG))
            .fill(ACCENT)
            .min_size(egui::vec2(110.0, 32.0))
            .corner_radius(egui::CornerRadius::same(255)),
    )
}

/// Shows the window for `offer` ([`Offer::Install`] or [`Offer::Update`]) and returns the choice.
pub fn ask(offer: Offer) -> Choice {
    let folder = match &offer {
        Offer::Install { suggested } => suggested.display().to_string(),
        Offer::Update { root, .. } | Offer::OpenInstalled { root } => root.display().to_string(),
        Offer::None => String::new(),
    };
    let choice = Arc::new(Mutex::new(Choice::Cancel));
    let result = choice.clone();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("Install {}", install::NAME))
            .with_app_id("aura-portfolio")
            .with_inner_size([620.0, 320.0])
            .with_resizable(false)
            .with_icon(Arc::new(theme::app_icon())),
        centered: true,
        ..Default::default()
    };
    let run = eframe::run_native(
        &format!("Install {}", install::NAME),
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            let logo = theme::logo_texture(&cc.egui_ctx);
            Ok(Box::new(InstallWindow {
                offer,
                folder,
                error: None,
                choice,
                logo,
            }))
        }),
    );
    if let Err(e) = run {
        tracing::warn!("couldn't show the install window ({e}); running without installing");
        return Choice::RunHere;
    }
    result.lock().clone()
}
