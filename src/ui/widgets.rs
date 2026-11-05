//! Reusable dashboard widgets: cards, KPI tiles, chips, gradient text,
//! sparklines, the LLM telemetry widget and the "Made by akulav" credit.

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontId, Layout, Margin, RichText, Sense, Shape, Stroke, Ui,
    Vec2, text::LayoutJob,
};

use super::theme::{lerp, palette::*};
use crate::net::fmt_bytes;
use crate::telemetry::TelemetrySnapshot;

/// A rounded surface card.
pub fn card<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(16))
        .show(ui, add_contents)
        .inner
}

/// Prepares `ui` for a data table: cells never wrap (money stays on one
/// line) and the enclosing card stretches to the full available width.
pub fn table_mode(ui: &mut Ui) {
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    ui.set_min_width(ui.available_width());
}

/// Small upper-case section label.
pub fn section_title(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .size(11.5)
            .color(MUTED)
            .strong(),
    );
}

/// A KPI tile: label, big value, and a muted caption.
pub fn kpi(ui: &mut Ui, icon: &str, label: &str, value: &str, caption: &str, accent: Color32) {
    card(ui, |ui| {
        ui.set_min_height(104.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon).size(16.0).color(accent));
            section_title(ui, label);
        });
        ui.add_space(4.0);
        ui.label(RichText::new(value).size(26.0).strong().color(TEXT));
        ui.label(RichText::new(caption).size(12.0).color(MUTED));
    });
}

/// A pill-shaped status chip.
pub fn chip(ui: &mut Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.linear_multiply(0.15))
        .stroke(Stroke::new(1.0_f32, color.linear_multiply(0.6)))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(color).strong());
        });
}

/// Text whose characters fade from teal to violet.
pub fn gradient_text(ui: &mut Ui, text: &str, size: f32) -> egui::Response {
    let mut job = LayoutJob::default();
    let n = text.chars().count().max(2) - 1;
    for (i, ch) in text.chars().enumerate() {
        job.append(
            &ch.to_string(),
            0.0,
            egui::TextFormat {
                font_id: FontId::proportional(size),
                color: lerp(ACCENT, VIOLET, i as f32 / n as f32),
                ..Default::default()
            },
        );
    }
    ui.label(job)
}

/// Centered placeholder for not-yet-implemented areas.
pub fn empty_state(ui: &mut Ui, icon: &str, title: &str, body: &str) {
    card(ui, |ui| {
        ui.set_min_height(220.0);
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(RichText::new(icon).size(36.0).color(ACCENT));
            ui.label(RichText::new(title).size(18.0).strong());
            ui.label(RichText::new(body).color(MUTED));
        });
    });
}

/// Horizontal usage bar with a label and `used / total` text.
pub fn usage_bar(ui: &mut Ui, label: &str, used: u64, total: u64, color: Color32) {
    let frac = if total > 0 {
        used as f32 / total as f32
    } else {
        0.0
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(12.0).color(MUTED));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{} / {}", fmt_bytes(used), fmt_bytes(total)))
                    .size(12.0)
                    .monospace(),
            );
        });
    });
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), SURFACE_2);
    let mut fill = rect;
    fill.set_width(rect.width() * frac.clamp(0.0, 1.0));
    painter.rect_filled(fill, CornerRadius::same(3), color);
}

/// Minimal line sparkline of `values`, scaled to `max`.
pub fn sparkline(ui: &mut Ui, values: &[f32], max: f32, color: Color32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(6), SURFACE_2);
    if values.len() < 2 || max <= 0.0 {
        return;
    }
    let step = rect.width() / (crate::telemetry::HISTORY_LEN - 1) as f32;
    let x0 = rect.right() - step * (values.len() - 1) as f32;
    let points: Vec<egui::Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let y = rect.bottom() - 3.0 - (v / max).clamp(0.0, 1.0) * (rect.height() - 6.0);
            egui::pos2(x0 + i as f32 * step, y)
        })
        .collect();
    let mut area = points.clone();
    area.push(egui::pos2(points.last().unwrap().x, rect.bottom()));
    area.push(egui::pos2(points[0].x, rect.bottom()));
    painter.add(Shape::convex_polygon(
        area,
        color.linear_multiply(0.12),
        Stroke::NONE,
    ));
    painter.add(Shape::line(points, Stroke::new(1.6_f32, color)));
}

/// Compact telemetry readout for the footer: `LLM  RAM 4.1 GB · VRAM 5.2 GB`.
pub fn telemetry_compact(ui: &mut Ui, t: &TelemetrySnapshot) {
    let dot = if t.llm_ram.is_some() { POSITIVE } else { MUTED };
    // Painted, not a glyph: "●" is missing from egui's bundled fonts.
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, dot);
    ui.label(RichText::new("LLM").size(12.0).color(MUTED).strong());
    match t.llm_ram {
        Some(ram) => {
            ui.label(
                RichText::new(format!("RAM {}", fmt_bytes(ram)))
                    .size(12.0)
                    .monospace(),
            );
            if let Some(vram) = t.llm_vram {
                let prefix = if t.llm_vram_estimated { "≈" } else { "" };
                ui.label(
                    RichText::new(format!("VRAM {prefix}{}", fmt_bytes(vram)))
                        .size(12.0)
                        .monospace()
                        .color(VIOLET),
                );
            }
        }
        None => {
            ui.label(RichText::new("unloaded").size(12.0).color(MUTED));
        }
    }
}

/// Full telemetry card with usage bars and sparklines.
pub fn telemetry_panel(ui: &mut Ui, t: &TelemetrySnapshot) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            section_title(ui, "Live telemetry");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if t.llm_ram.is_some() {
                    chip(ui, "MODEL RUNNING", POSITIVE);
                } else {
                    chip(ui, "IDLE", MUTED);
                }
            });
        });
        ui.add_space(6.0);

        usage_bar(ui, "System RAM", t.sys_ram_used, t.sys_ram_total, ACCENT);
        ui.add_space(4.0);
        if let Some(gpu) = &t.gpu {
            let label = match gpu.utilization {
                Some(u) => format!("{} · {u}% load", gpu.name),
                None => gpu.name.clone(),
            };
            usage_bar(ui, &label, gpu.vram_used, gpu.vram_total, VIOLET);
        } else {
            ui.label(
                RichText::new("GPU telemetry unavailable (NVIDIA only for now)")
                    .size(12.0)
                    .color(MUTED),
            );
        }
        ui.add_space(10.0);

        let ram: Vec<f32> = t.history.iter().map(|s| s.llm_ram as f32).collect();
        let vram: Vec<f32> = t.history.iter().map(|s| s.llm_vram as f32).collect();
        let ram_max = ram.iter().cloned().fold(1.0, f32::max) * 1.2;
        let vram_max = vram.iter().cloned().fold(1.0, f32::max) * 1.2;

        ui.columns(2, |cols| {
            cols[0].label(
                RichText::new(format!(
                    "LLM RAM  {}",
                    t.llm_ram.map(fmt_bytes).unwrap_or("—".into())
                ))
                .size(12.0)
                .color(ACCENT),
            );
            sparkline(&mut cols[0], &ram, ram_max, ACCENT, 46.0);
            let vram_text = match t.llm_vram {
                Some(v) if t.llm_vram_estimated => format!("≈{}", fmt_bytes(v)),
                Some(v) => fmt_bytes(v),
                None => "—".into(),
            };
            cols[1].label(
                RichText::new(format!("LLM VRAM  {vram_text}"))
                    .size(12.0)
                    .color(VIOLET),
            );
            sparkline(&mut cols[1], &vram, vram_max, VIOLET, 46.0);
        });
    });
}

/// The footer credit: "Made with ♥ by akulav" (author name in brand gradient,
/// clickable). Built as a single text job so it renders correctly in any
/// layout direction.
pub fn made_by(ui: &mut Ui) {
    let fmt = |size: f32, color: Color32| egui::TextFormat {
        font_id: FontId::proportional(size),
        color,
        ..Default::default()
    };
    let mut job = LayoutJob::default();
    job.append("Made with ", 0.0, fmt(12.5, MUTED));
    job.append("♥", 0.0, fmt(12.5, NEGATIVE));
    job.append(" by ", 0.0, fmt(12.5, MUTED));
    let author: Vec<char> = crate::AUTHOR.chars().collect();
    let n = author.len().max(2) - 1;
    for (i, ch) in author.iter().enumerate() {
        let mut f = fmt(14.0, lerp(ACCENT, VIOLET, i as f32 / n as f32));
        f.underline = Stroke::new(1.0_f32, VIOLET.linear_multiply(0.5));
        job.append(&ch.to_string(), 0.0, f);
    }
    let resp = ui
        .add(egui::Label::new(job).sense(Sense::click()))
        .on_hover_text("github.com/akulav")
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() {
        ui.ctx()
            .open_url(egui::OpenUrl::new_tab("https://github.com/akulav"));
    }
}
