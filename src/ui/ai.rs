//! AI Engine page: hardware summary, one-click llama.cpp install, model
//! catalog with VRAM-aware fit badges, load/unload, and live telemetry.

use eframe::egui::{self, Align, Layout, RichText, Ui};

use super::Services;
use super::theme::palette::*;
use super::widgets::{self, card, chip};
use crate::config::Settings;
use crate::llm::models::{self, Fit, ModelSpec};
use crate::llm::runtime::InstallStage;
use crate::llm::{DownloadStatus, LlmState, RuntimeStatus, ServerStatus};
use crate::net::fmt_bytes;
use crate::telemetry::TelemetrySnapshot;

/// Renders the page. Returns `true` if settings changed.
pub fn show(
    ui: &mut Ui,
    svc: &Services,
    settings: &mut Settings,
    telemetry: &TelemetrySnapshot,
) -> bool {
    let state = svc.llm.snapshot();
    let mut changed = false;

    ui.heading(RichText::new("Local AI engine").strong());
    ui.label(
        RichText::new("Everything runs on your machine. No data leaves your computer.")
            .color(MUTED),
    );
    ui.add_space(12.0);

    ui.columns(3, |c| {
        hardware_card(&mut c[0], &state, svc);
        runtime_card(&mut c[1], &state, svc);
        changed |= server_card(&mut c[2], &state, svc, settings);
    });
    ui.add_space(12.0);
    widgets::telemetry_panel(ui, telemetry);
    ui.add_space(12.0);
    changed |= catalog(ui, &state, svc, settings);
    changed
}

fn hardware_card(ui: &mut Ui, st: &LlmState, svc: &Services) {
    card(ui, |ui| {
        ui.set_min_height(150.0);
        ui.horizontal(|ui| {
            widgets::section_title(ui, "Hardware");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("Re-scan").clicked() {
                    svc.llm.rescan_hardware();
                }
            });
        });
        let hw = &st.hardware;
        ui.label(RichText::new(&hw.cpu_name).strong());
        ui.label(
            RichText::new(format!(
                "{} threads · {} RAM",
                hw.cpu_threads,
                fmt_bytes(hw.ram_total)
            ))
            .color(MUTED),
        );
        match hw.best_gpu() {
            Some(g) => {
                ui.label(RichText::new(&g.name).strong().color(VIOLET));
                let kind = if g.unified { "unified memory" } else { "VRAM" };
                ui.label(RichText::new(format!("{} {kind}", fmt_bytes(g.vram_total))).color(MUTED));
            }
            None => {
                ui.label(RichText::new("No CUDA/Metal GPU detected").color(MUTED));
                if hw.vulkan_available {
                    ui.label(
                        RichText::new("Vulkan available — GPU offload via Vulkan").color(MUTED),
                    );
                }
            }
        }
    });
}

fn runtime_card(ui: &mut Ui, st: &LlmState, svc: &Services) {
    card(ui, |ui| {
        ui.set_min_height(150.0);
        widgets::section_title(ui, "llama.cpp runtime");
        match &st.runtime {
            RuntimeStatus::Missing => {
                ui.label("Not installed. Aura downloads a portable build tuned for your hardware.");
                if accent_button(ui, "⬇  Install runtime").clicked() {
                    svc.llm.install_runtime();
                }
            }
            RuntimeStatus::Installing(stage) => match stage {
                InstallStage::Resolving => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Finding the best build…");
                    });
                }
                InstallStage::Downloading { asset, progress } => {
                    ui.label(RichText::new(asset).size(12.0).color(MUTED));
                    ui.add(
                        egui::ProgressBar::new(progress.fraction().unwrap_or(0.0))
                            .show_percentage()
                            .animate(true),
                    );
                }
                InstallStage::Extracting => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Extracting…");
                    });
                }
            },
            RuntimeStatus::Ready(m) => {
                ui.horizontal(|ui| {
                    chip(ui, "INSTALLED", POSITIVE);
                    chip(ui, &m.backend.to_string(), VIOLET);
                });
                ui.label(RichText::new(format!("Release {}", m.tag)).monospace());
                if ui.small_button("Reinstall / update").clicked() {
                    svc.llm.install_runtime();
                }
            }
            RuntimeStatus::Failed(e) => {
                ui.colored_label(NEGATIVE, "Install failed");
                ui.label(RichText::new(e).size(12.0).color(MUTED));
                if ui.button("Retry").clicked() {
                    svc.llm.install_runtime();
                }
            }
        }
    });
}

fn server_card(ui: &mut Ui, st: &LlmState, svc: &Services, settings: &mut Settings) -> bool {
    let mut changed = false;
    card(ui, |ui| {
        ui.set_min_height(150.0);
        widgets::section_title(ui, "Model server");
        match &st.server {
            ServerStatus::Unloaded => {
                ui.label(RichText::new("No model loaded · 0 B in use").color(MUTED));
            }
            ServerStatus::Starting { model_id } => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Loading {}…", display_name(model_id)));
                });
                if ui.button("Cancel").clicked() {
                    svc.llm.unload();
                }
            }
            ServerStatus::Ready {
                model_id,
                port,
                gpu_layers,
            } => {
                ui.horizontal(|ui| {
                    chip(ui, "READY", POSITIVE);
                    ui.label(RichText::new(display_name(model_id)).strong());
                });
                let offload = if *gpu_layers >= 999 {
                    "all".to_string()
                } else {
                    gpu_layers.to_string()
                };
                ui.label(
                    RichText::new(format!("127.0.0.1:{port} · GPU layers: {offload}"))
                        .monospace()
                        .color(MUTED),
                );
                if ui.button("⏏  Unload & free memory").clicked() {
                    svc.llm.unload();
                }
            }
            ServerStatus::Crashed(e) => {
                ui.colored_label(NEGATIVE, "Model failed to load");
                egui::ScrollArea::vertical()
                    .max_height(70.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new(e).size(11.5).monospace().color(MUTED));
                    });
            }
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Context").color(MUTED));
            egui::ComboBox::from_id_salt("ctx_size")
                .selected_text(format!("{} tokens", settings.context_size))
                .show_ui(ui, |ui| {
                    for size in [4096, 8192, 16384, 32768] {
                        changed |= ui
                            .selectable_value(
                                &mut settings.context_size,
                                size,
                                format!("{size} tokens"),
                            )
                            .changed();
                    }
                });
        });
        changed |= ui
            .checkbox(
                &mut settings.auto_load_model,
                "Load selected model on startup",
            )
            .changed();
    });
    changed
}

fn catalog(ui: &mut Ui, st: &LlmState, svc: &Services, settings: &mut Settings) -> bool {
    let mut changed = false;
    let recommended = models::recommend(&st.hardware);
    card(ui, |ui| {
        widgets::section_title(ui, "Model library");
        ui.label(
            RichText::new(format!(
                "Recommended for this machine: {} ⭐",
                recommended.name
            ))
            .color(ACCENT),
        );
        ui.add_space(6.0);
        widgets::table_mode(ui);
        egui::Grid::new("model_catalog")
            .num_columns(5)
            .spacing([18.0, 10.0])
            .striped(true)
            .show(ui, |ui| {
                for h in ["Model", "Size", "Fit", "Status", ""] {
                    ui.label(RichText::new(h).size(12.0).color(MUTED).strong());
                }
                ui.end_row();
                for spec in models::CATALOG {
                    changed |= catalog_row(ui, spec, st, svc, settings, spec.id == recommended.id);
                    ui.end_row();
                }
            });
    });
    changed
}

fn catalog_row(
    ui: &mut Ui,
    spec: &'static ModelSpec,
    st: &LlmState,
    svc: &Services,
    settings: &mut Settings,
    recommended: bool,
) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        let star = if recommended { " ⭐" } else { "" };
        ui.label(RichText::new(format!("{}{star}", spec.name)).strong());
        ui.label(
            RichText::new(format!("{} · {}", spec.quant, spec.blurb))
                .size(12.0)
                .color(MUTED),
        );
    });
    ui.label(RichText::new(fmt_bytes(spec.size_bytes)).monospace());

    let fit = models::assess(spec, &st.hardware);
    let fit_color = match fit {
        Fit::FullGpu => POSITIVE,
        Fit::PartialOffload => ACCENT,
        Fit::CpuOnly => WARNING,
        Fit::TooLarge => NEGATIVE,
    };
    chip(ui, fit.label(), fit_color);

    let downloaded = st.downloaded.contains(spec.id);
    let loaded = matches!(&st.server,
        ServerStatus::Ready { model_id, .. } | ServerStatus::Starting { model_id } if model_id == spec.id);
    match st.downloads.get(spec.id) {
        Some(DownloadStatus::Downloading(p)) => {
            ui.add(
                egui::ProgressBar::new(p.fraction().unwrap_or(0.0))
                    .desired_width(160.0)
                    .text(format!(
                        "{} / {}",
                        fmt_bytes(p.downloaded),
                        fmt_bytes(p.total.unwrap_or(spec.size_bytes))
                    )),
            );
        }
        Some(DownloadStatus::Failed(e)) => {
            ui.colored_label(NEGATIVE, "failed").on_hover_text(e);
        }
        None if loaded => {
            ui.colored_label(POSITIVE, "● loaded");
        }
        None if downloaded => {
            ui.label("on disk");
        }
        None => {
            ui.label(RichText::new("—").color(MUTED));
        }
    }

    ui.horizontal(|ui| {
        let downloading = matches!(
            st.downloads.get(spec.id),
            Some(DownloadStatus::Downloading(_))
        );
        if !downloaded && !downloading {
            let btn = ui.add_enabled(fit != Fit::TooLarge, egui::Button::new("⬇ Download"));
            if btn.clicked() {
                svc.llm.download_model(spec);
            }
        }
        if downloaded && !loaded {
            let runtime_ready = matches!(st.runtime, RuntimeStatus::Ready(_));
            let btn = ui
                .add_enabled(runtime_ready, egui::Button::new("▶ Load"))
                .on_disabled_hover_text("Install the llama.cpp runtime first");
            if btn.clicked() {
                settings.selected_model = Some(spec.id.to_string());
                changed = true;
                if let Err(e) = svc.llm.load_model(spec, settings.context_size) {
                    tracing::error!("load failed: {e:#}");
                }
            }
            if ui
                .small_button("🗑")
                .on_hover_text("Delete from disk")
                .clicked()
                && let Err(e) = svc.llm.delete_model(spec)
            {
                tracing::error!("delete failed: {e:#}");
            }
        }
        if loaded && ui.button("⏏ Unload").clicked() {
            svc.llm.unload();
        }
    });
    changed
}

fn accent_button(ui: &mut Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).strong().color(BG)).fill(ACCENT))
}

fn display_name(id: &str) -> &str {
    models::find(id).map_or(id, |m| m.name)
}
