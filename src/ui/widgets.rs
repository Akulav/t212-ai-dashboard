//! Reusable dashboard widgets: cards, KPI tiles, chips, gradient text,
//! sparklines, the LLM telemetry widget and the "Made by akulav" credit.

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontId, Layout, Margin, RichText, Sense, Shape, Stroke, Ui,
    Vec2, text::LayoutJob,
};

use super::theme::{lerp, palette::*, semibold};
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
            .font(semibold(11.0))
            .color(MUTED)
            .extra_letter_spacing(0.6),
    );
}

/// Page title with a muted subtitle underneath.
pub fn page_title(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).font(semibold(26.0)).color(TEXT));
    if !subtitle.is_empty() {
        ui.label(RichText::new(subtitle).size(13.5).color(MUTED));
    }
}

/// An icon inside a softly tinted rounded square.
pub fn icon_badge(ui: &mut Ui, icon: &str, color: Color32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let p = ui.painter();
    p.rect_filled(
        rect,
        CornerRadius::same((size * 0.28) as u8),
        color.linear_multiply(0.16),
    );
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        FontId::proportional(size * 0.55),
        color,
    );
}

/// A KPI tile: icon badge + label, big value, muted caption.
pub fn kpi(ui: &mut Ui, icon: &str, label: &str, value: &str, caption: &str, accent: Color32) {
    kpi_with(ui, icon, label, value, caption, accent, TEXT, None);
}

/// KPI tile with a coloured value and an optional trend sparkline.
#[allow(clippy::too_many_arguments)]
pub fn kpi_with(
    ui: &mut Ui,
    icon: &str,
    label: &str,
    value: &str,
    caption: &str,
    accent: Color32,
    value_color: Color32,
    trend: Option<&[f64]>,
) {
    card(ui, |ui| {
        ui.set_min_height(118.0);
        ui.horizontal(|ui| {
            icon_badge(ui, icon, accent, 28.0);
            ui.add_space(2.0);
            section_title(ui, label);
        });
        ui.add_space(8.0);
        ui.label(RichText::new(value).font(semibold(28.0)).color(value_color));
        ui.label(RichText::new(caption).size(12.5).color(MUTED));
        // Always reserve the sparkline row so every KPI card in a row has
        // the same height, with or without a trend.
        ui.add_space(6.0);
        match trend.filter(|v| v.len() > 1) {
            Some(values) => trend_line(ui, values, accent, 30.0),
            None => {
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover());
            }
        }
    });
}

/// Borderless sparkline scaled to the series' own min..max.
pub fn trend_line(ui: &mut Ui, values: &[f64], color: Color32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let (min, max) = values
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    let span = (max - min).max(f64::EPSILON);
    let n = (values.len() - 1) as f32;
    let points: Vec<egui::Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let t = ((v - min) / span) as f32;
            egui::pos2(
                rect.left() + rect.width() * i as f32 / n,
                rect.bottom() - 2.0 - t * (rect.height() - 4.0),
            )
        })
        .collect();
    let painter = ui.painter_at(rect.expand(2.0));
    // The area under a line is not convex, so fill it as thin vertical quads.
    for w in points.windows(2) {
        painter.add(Shape::convex_polygon(
            vec![
                w[0],
                w[1],
                egui::pos2(w[1].x, rect.bottom()),
                egui::pos2(w[0].x, rect.bottom()),
            ],
            color.linear_multiply(0.10),
            Stroke::NONE,
        ));
    }
    painter.add(Shape::line(points, Stroke::new(1.8_f32, color)));
}

/// A pill-shaped status chip. Painted at a fixed height so it never
/// stretches to the height of the surrounding row.
pub fn chip(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), semibold(12.0), color);
    let size = egui::vec2(galley.size().x + 22.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    p.rect(
        rect,
        CornerRadius::same(13),
        color.linear_multiply(0.14),
        Stroke::new(1.0_f32, color.linear_multiply(0.45)),
        egui::StrokeKind::Inside,
    );
    p.galley(rect.center() - galley.size() / 2.0, galley, color);
    resp
}

/// Full-width callout with an icon, used for sync warnings and errors.
pub fn banner(ui: &mut Ui, icon: &str, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.linear_multiply(0.09))
        .stroke(Stroke::new(1.0_f32, color.linear_multiply(0.4)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).size(18.0).color(color));
                ui.add(egui::Label::new(RichText::new(text).size(13.0).color(TEXT)).wrap());
            });
        });
}

/// Donut chart with a legend. `slices` are `(label, value, colour)`.
pub fn donut(
    ui: &mut Ui,
    slices: &[(String, f64, Color32)],
    center_big: &str,
    center_small: &str,
    diameter: f32,
) {
    let total: f64 = slices.iter().map(|s| s.1).sum();
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::hover());
        let p = ui.painter_at(rect);
        let c = rect.center();
        let r_out = diameter / 2.0 - 2.0;
        let thickness = diameter * 0.16;
        let r_mid = r_out - thickness / 2.0;
        if total > 0.0 {
            let mut start = -std::f32::consts::FRAC_PI_2;
            let gap: f32 = 0.02;
            for (_, v, color) in slices {
                let sweep = (*v / total) as f32 * std::f32::consts::TAU;
                let steps = ((sweep / 0.05).ceil() as usize).max(2);
                let a0 = start + gap.min(sweep / 4.0);
                let a1 = start + sweep - gap.min(sweep / 4.0);
                let pts: Vec<egui::Pos2> = (0..=steps)
                    .map(|i| {
                        let a = a0 + (a1 - a0) * i as f32 / steps as f32;
                        c + Vec2::angled(a) * r_mid
                    })
                    .collect();
                p.add(Shape::line(pts, Stroke::new(thickness, *color)));
                start += sweep;
            }
        } else {
            p.circle_stroke(c, r_mid, Stroke::new(thickness, SURFACE_2));
        }
        p.text(
            c - Vec2::new(0.0, 8.0),
            egui::Align2::CENTER_CENTER,
            center_big,
            semibold(22.0),
            TEXT,
        );
        p.text(
            c + Vec2::new(0.0, 14.0),
            egui::Align2::CENTER_CENTER,
            center_small,
            FontId::proportional(11.5),
            MUTED,
        );

        ui.add_space(14.0);
        ui.vertical(|ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            for (label, v, color) in slices {
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                    ui.painter()
                        .rect_filled(dot.shrink(1.0), CornerRadius::same(2), *color);
                    let share = if total > 0.0 { v / total * 100.0 } else { 0.0 };
                    ui.label(
                        RichText::new(format!("{share:>4.1}%"))
                            .monospace()
                            .size(12.5),
                    );
                    ui.label(RichText::new(label).size(12.5).color(MUTED));
                });
            }
        });
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
        ui.set_min_width(ui.available_width());
        ui.vertical_centered(|ui| {
            ui.add_space(36.0);
            ui.label(RichText::new(icon).size(40.0).color(ACCENT));
            ui.add_space(6.0);
            ui.label(RichText::new(title).font(semibold(19.0)));
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
