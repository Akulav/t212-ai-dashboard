//! Regenerates every platform icon from the procedural brand mark
//! (`ui::theme::app_icon`), so the exe, installers, window and in-app logo
//! are always pixel-identical.
//!
//! ```text
//! cargo run --example gen_icons
//! ```
//!
//! Writes to `assets/icons/`:
//! * `icon.ico`  — Windows exe + installer (16…256 px, PNG-compressed frames)
//! * `icon.icns` — macOS app bundle / DMG
//! * `32x32.png`, `128x128.png`, `256x256.png`, `512x512.png` — Linux & misc

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use aura_portfolio::ui::theme::app_icon;
use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::{ExtendedColorType, RgbaImage};

fn main() -> anyhow::Result<()> {
    let out = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("icons");
    std::fs::create_dir_all(&out)?;

    let rgba = |size: u32| app_icon(size).rgba;

    // PNGs.
    for size in [32, 128, 256, 512] {
        let img = RgbaImage::from_raw(size, size, rgba(size)).expect("buffer size");
        img.save(out.join(format!("{size}x{size}.png")))?;
    }

    // ICO with PNG-encoded frames.
    let frames = [16, 24, 32, 48, 64, 128, 256]
        .into_iter()
        .map(|s| IcoFrame::as_png(&rgba(s), s, s, ExtendedColorType::Rgba8))
        .collect::<Result<Vec<_>, _>>()?;
    IcoEncoder::new(BufWriter::new(File::create(out.join("icon.ico"))?)).encode_images(&frames)?;

    // ICNS.
    let mut family = icns::IconFamily::new();
    for size in [16, 32, 64, 128, 256, 512, 1024] {
        let image = icns::Image::from_data(icns::PixelFormat::RGBA, size, size, rgba(size))?;
        family.add_icon(&image)?;
    }
    family.write(BufWriter::new(File::create(out.join("icon.icns"))?))?;

    println!("icons written to {}", out.display());
    Ok(())
}
