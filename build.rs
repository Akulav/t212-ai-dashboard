//! Build script:
//! * exposes the target triple (`AURA_TARGET`) for the auto-updater;
//! * on Windows, embeds `assets/icons/icon.ico` and version metadata into the
//!   executable, so Explorer, the taskbar, Task Manager and the installer all
//!   show the Aura icon. Regenerate the icons with
//!   `cargo run --example gen_icons`.

fn main() {
    let target = std::env::var("TARGET").expect("cargo always sets TARGET");
    println!("cargo:rustc-env=AURA_TARGET={target}");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icons/icon.ico");

    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/icon.ico")
            .set("ProductName", "Aura Portfolio")
            .set("FileDescription", "Aura Portfolio")
            .set("CompanyName", "akulav")
            .set("LegalCopyright", "© 2026 akulav. MIT License.")
            .set("OriginalFilename", "aura-portfolio.exe")
            .set("ProductVersion", &version)
            .set("FileVersion", &version);
        if let Err(e) = res.compile() {
            // Don't break builds on machines without the Windows SDK's rc.exe;
            // the window icon is still set at runtime.
            println!("cargo:warning=could not embed Windows icon resource: {e}");
        }
    }
}
