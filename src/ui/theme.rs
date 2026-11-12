//! "Aurora" dark theme, brand palette, fonts and the bundled icon. The look follows Akulav's newer apps (Hyve,
//! Lumen): a deep blue-black backdrop with soft glows ([`paint_backdrop`]) and glass panels and cards with hairline
//! borders ([`palette::GLASS`]).
//!
//! The icon artwork has one master, `assets/icon.svg`; `tools_dev/make_icons.py` draws every size from it. The window
//! icon and the in-app logo decode `assets/icon-256.png`, which is compiled into the program.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Vec2};

/// Brand palette.
pub mod palette {
    use eframe::egui::Color32;

    /// App background.
    pub const BG: Color32 = Color32::from_rgb(0x09, 0x0B, 0x14);
    /// Solid panel surface (popups, dialogs).
    pub const SURFACE: Color32 = Color32::from_rgb(0x10, 0x13, 0x1F);
    /// Raised surface (hover, inputs).
    pub const SURFACE_2: Color32 = Color32::from_rgb(0x17, 0x1B, 0x2A);
    /// Hairline borders.
    pub const BORDER: Color32 = Color32::from_rgb(0x2A, 0x2F, 0x45);
    /// Glass: translucent white over the backdrop (cards and the sidebar), so the glows show through.
    pub const GLASS: Color32 = Color32::from_rgba_premultiplied(14, 14, 15, 14);
    /// Hairline border of glass panels.
    pub const GLASS_BORDER: Color32 = Color32::from_rgba_premultiplied(24, 24, 26, 24);
    /// Primary text.
    pub const TEXT: Color32 = Color32::from_rgb(0xE8, 0xEA, 0xF6);
    /// Secondary text.
    pub const MUTED: Color32 = Color32::from_rgb(0x8F, 0x95, 0xB2);
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
    /// Sky blue (chart series).
    pub const SKY: Color32 = Color32::from_rgb(0x38, 0xBD, 0xF8);
    /// Pink (chart series).
    pub const PINK: Color32 = Color32::from_rgb(0xF4, 0x72, 0xB6);
    /// Orange (chart series).
    pub const ORANGE: Color32 = Color32::from_rgb(0xFB, 0x92, 0x3C);
    /// Lime (chart series).
    pub const LIME: Color32 = Color32::from_rgb(0xA3, 0xE6, 0x35);

    /// Categorical colours for allocation charts, in order.
    pub const SERIES: [Color32; 7] = [ACCENT, VIOLET, GOLD, SKY, PINK, LIME, ORANGE];
}

use palette::*;

/// Gap between the window edge, the sidebar panel and the content.
pub const WINDOW_GAP: f32 = 14.0;

/// Soft radial glow: `color` in the middle, fading to nothing at `radius`.
fn radial(mesh: &mut egui::Mesh, center: egui::Pos2, radius: Vec2, color: Color32) {
    use egui::epaint::{Vertex, WHITE_UV};
    let base = mesh.vertices.len() as u32;
    mesh.vertices.push(Vertex {
        pos: center,
        uv: WHITE_UV,
        color,
    });
    let n = 48;
    for i in 0..=n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        let pos = center + egui::vec2(a.cos() * radius.x, a.sin() * radius.y);
        mesh.vertices.push(Vertex {
            pos,
            uv: WHITE_UV,
            color: Color32::TRANSPARENT,
        });
    }
    for i in 0..n {
        mesh.add_triangle(base, base + 1 + i, base + 2 + i);
    }
}

/// Paints the window backdrop: the base colour with a teal glow top-left, a violet glow top-right and a faint sky
/// one below.
pub fn paint_backdrop(ctx: &egui::Context) {
    let r = ctx.screen_rect();
    let p = ctx.layer_painter(egui::LayerId::background());
    p.rect_filled(r, CornerRadius::ZERO, BG);
    let (w, h) = (r.width(), r.height());
    let mut mesh = egui::Mesh::default();
    radial(
        &mut mesh,
        r.min + egui::vec2(w * 0.08, -h * 0.10),
        egui::vec2(w * 0.62, h * 0.85),
        ACCENT.gamma_multiply(0.11),
    );
    radial(
        &mut mesh,
        r.min + egui::vec2(w * 1.05, h * 0.10),
        egui::vec2(w * 0.48, h * 0.75),
        VIOLET.gamma_multiply(0.14),
    );
    radial(
        &mut mesh,
        r.min + egui::vec2(w * 0.5, h * 1.2),
        egui::vec2(w * 0.45, h * 0.6),
        SKY.gamma_multiply(0.06),
    );
    p.add(mesh);
}

/// Name of the semibold font family registered by [`install_fonts`].
pub const SEMIBOLD: &str = "semibold";

/// Semibold font of `size` (headings, KPI values, active navigation).
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

/// Native UI fonts per platform, tried in order (regular, semibold).
/// Loaded at runtime from the OS: nothing is bundled or downloaded, and
/// egui's built-in font remains the fallback.
const FONT_CANDIDATES: &[(&str, &str)] = &[
    // Windows
    (
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "C:\\Windows\\Fonts\\seguisb.ttf",
    ),
    // macOS
    (
        "/System/Library/Fonts/SFNS.ttf",
        "/System/Library/Fonts/SFNS.ttf",
    ),
    (
        "/System/Library/Fonts/Helvetica.ttc",
        "/System/Library/Fonts/Helvetica.ttc",
    ),
    // Linux
    (
        "/usr/share/fonts/opentype/inter/Inter-Regular.otf",
        "/usr/share/fonts/opentype/inter/Inter-SemiBold.otf",
    ),
    (
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-SemiBold.ttf",
    ),
    (
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/noto/NotoSans-SemiBold.ttf",
    ),
    (
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    ),
];

/// Registers the native UI font (if found), a semibold family and the
/// Phosphor icon font.
pub fn install_fonts(ctx: &egui::Context) {
    use std::sync::Arc;
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

    let read = |p: &str| std::fs::read(p).ok();
    let found = FONT_CANDIDATES.iter().find_map(|(reg, semi)| {
        let regular = read(reg)?;
        Some((regular.clone(), read(semi).unwrap_or(regular)))
    });

    let base: Vec<String> = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut semibold_family = base.clone();
    if let Some((regular, semi)) = found {
        fonts.font_data.insert(
            "ui-regular".into(),
            Arc::new(egui::FontData::from_owned(regular)),
        );
        fonts.font_data.insert(
            "ui-semibold".into(),
            Arc::new(egui::FontData::from_owned(semi)),
        );
        if let Some(prop) = fonts.families.get_mut(&FontFamily::Proportional) {
            prop.insert(0, "ui-regular".into());
        }
        semibold_family.insert(0, "ui-semibold".into());
    }
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), semibold_family);
    ctx.set_fonts(fonts);
}

/// Applies the dark theme, typography and spacing.
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_theme(egui::ThemePreference::Dark);

    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = SURFACE;
    v.extreme_bg_color = Color32::from_rgb(0x07, 0x08, 0x0F); // text fields: darker than the glass around them
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
            (TextStyle::Heading, semibold(24.0)),
            (TextStyle::Body, FontId::new(14.0, Proportional)),
            (TextStyle::Button, FontId::new(14.0, Proportional)),
            (TextStyle::Small, FontId::new(11.5, Proportional)),
            (TextStyle::Monospace, FontId::new(13.0, Monospace)),
        ]
        .into();
        style.spacing.item_spacing = Vec2::new(10.0, 8.0);
        style.spacing.button_padding = Vec2::new(12.0, 6.0);
        style.spacing.interact_size.y = 30.0;
        // Solid scrollbar with its own gutter, so it never overlaps card edges.
        style.spacing.scroll = egui::style::ScrollStyle::solid();
    });
}

/// Linear blend between two colours (`t` in `0..=1`).
pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// The icon artwork shipped inside the program (`assets/icon-256.png`, drawn from `assets/icon.svg` by
/// `tools_dev/make_icons.py`): the window/taskbar icon and the in-app logo use exactly this image.
pub const ICON_PNG: &[u8] = include_bytes!("../../assets/icon-256.png");

/// The window / taskbar icon, decoded from [`ICON_PNG`].
pub fn app_icon() -> egui::IconData {
    let img = image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png)
        .expect("the bundled icon PNG decodes")
        .to_rgba8();
    let (width, height) = img.dimensions();
    egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    }
}

/// Uploads the icon as a GPU texture for use in the UI (`ui.image`).
pub fn logo_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let icon = app_icon();
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    );
    ctx.load_texture("aura-logo", image, egui::TextureOptions::LINEAR)
}

#[cfg(test)]
mod tests {
    #[test]
    fn bundled_icon_decodes_at_256px() {
        let icon = super::app_icon();
        assert_eq!((icon.width, icon.height), (256, 256));
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
    }
}
