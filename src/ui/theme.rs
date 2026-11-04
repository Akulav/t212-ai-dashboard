//! "Aurora" dark theme, brand palette and the procedurally drawn logo/icon.
//!
//! The logo — three ascending bars inside a teal→violet rounded tile — is
//! rendered in code (both the window icon and the in-app mark), so the repo
//! needs no binary image assets. `assets/logo.svg` mirrors it for the README.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Vec2};

/// Brand palette.
pub mod palette {
    use eframe::egui::Color32;

    /// App background.
    pub const BG: Color32 = Color32::from_rgb(0x0A, 0x0E, 0x17);
    /// Card / panel surface.
    pub const SURFACE: Color32 = Color32::from_rgb(0x11, 0x17, 0x24);
    /// Raised surface (hover, inputs).
    pub const SURFACE_2: Color32 = Color32::from_rgb(0x18, 0x20, 0x31);
    /// Hairline borders.
    pub const BORDER: Color32 = Color32::from_rgb(0x23, 0x2D, 0x42);
    /// Primary text.
    pub const TEXT: Color32 = Color32::from_rgb(0xE6, 0xEA, 0xF2);
    /// Secondary text.
    pub const MUTED: Color32 = Color32::from_rgb(0x8A, 0x94, 0xA8);
    /// Aurora teal (primary accent).
    pub const ACCENT: Color32 = Color32::from_rgb(0x2D, 0xD4, 0xBF);
    /// Aurora violet (secondary accent).
    pub const VIOLET: Color32 = Color32::from_rgb(0x8B, 0x5C, 0xF6);
    /// Gold.
    pub const GOLD: Color32 = Color32::from_rgb(0xF5, 0xC4, 0x51);
    /// Gains.
    pub const POSITIVE: Color32 = Color32::from_rgb(0x22, 0xC5, 0x5E);
    /// Losses / errors.
    pub const NEGATIVE: Color32 = Color32::from_rgb(0xEF, 0x44, 0x44);
    /// Warnings.
    pub const WARNING: Color32 = Color32::from_rgb(0xF5, 0x9E, 0x0B);
}

use palette::*;

/// Applies the dark theme, typography and spacing.
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);

    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = SURFACE;
    v.extreme_bg_color = Color32::from_rgb(0x0D, 0x12, 0x1D);
    v.faint_bg_color = SURFACE_2;
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = ACCENT.linear_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.window_corner_radius = CornerRadius::same(12);
    v.window_stroke = Stroke::new(1.0_f32, BORDER);

    let r = CornerRadius::same(8);
    v.widgets.noninteractive.bg_fill = SURFACE;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.inactive.bg_fill = SURFACE_2;
    v.widgets.inactive.weak_bg_fill = SURFACE_2;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.hovered.bg_fill = Color32::from_rgb(0x20, 0x2A, 0x3F);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x20, 0x2A, 0x3F);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT.linear_multiply(0.6));
    v.widgets.active.bg_fill = ACCENT.linear_multiply(0.25);
    v.widgets.active.weak_bg_fill = ACCENT.linear_multiply(0.25);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
    }
    ctx.set_visuals_of(egui::Theme::Dark, v);

    ctx.style_mut_of(egui::Theme::Dark, |style| {
        use FontFamily::{Monospace, Proportional};
        style.text_styles = [
            (TextStyle::Heading, FontId::new(22.0, Proportional)),
            (TextStyle::Body, FontId::new(14.0, Proportional)),
            (TextStyle::Button, FontId::new(14.0, Proportional)),
            (TextStyle::Small, FontId::new(11.5, Proportional)),
            (TextStyle::Monospace, FontId::new(13.0, Monospace)),
        ]
        .into();
        style.spacing.item_spacing = Vec2::new(10.0, 8.0);
        style.spacing.button_padding = Vec2::new(12.0, 6.0);
        style.spacing.interact_size.y = 30.0;
    });
}

/// Linear blend between two colours (`t` in `0..=1`).
pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// Uploads the brand mark as a GPU texture for use in the UI (`ui.image`).
pub fn logo_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let icon = app_icon(128);
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    );
    ctx.load_texture("aura-logo", image, egui::TextureOptions::LINEAR)
}

/// Renders the brand mark as RGBA pixels for the window/taskbar icon.
pub fn app_icon(size: u32) -> egui::IconData {
    let n = size as f32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    let corner = n * 0.24;
    let bar_w = n * 0.14;
    let gap = n * 0.07;
    let base = n - n * 0.22;
    let left = n / 2.0 - (bar_w * 1.5 + gap);
    let bars: Vec<(f32, f32, f32)> = [0.28_f32, 0.42, 0.58]
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let x = left + i as f32 * (bar_w + gap);
            (x, x + bar_w, base - n * h)
        })
        .collect();

    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Rounded-square coverage.
            let dx = (corner - fx).max(fx - (n - corner)).max(0.0);
            let dy = (corner - fy).max(fy - (n - corner)).max(0.0);
            let dist = (dx * dx + dy * dy).sqrt();
            let alpha = (corner - dist + 0.5).clamp(0.0, 1.0);
            if alpha <= 0.0 {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let on_bar = bars
                .iter()
                .any(|&(x0, x1, top)| fx >= x0 && fx <= x1 && fy >= top && fy <= base);
            let c = if on_bar {
                Color32::WHITE
            } else {
                lerp(ACCENT, VIOLET, (fx + fy) / (2.0 * n))
            };
            rgba.extend_from_slice(&[c.r(), c.g(), c.b(), (alpha * 255.0) as u8]);
        }
    }
    egui::IconData {
        rgba,
        width: size,
        height: size,
    }
}
