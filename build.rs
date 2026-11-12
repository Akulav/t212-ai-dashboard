//! Build script:
//! * refuses to build when `VERSION` (the single source of truth) and the version in `Cargo.toml` differ — bump both
//!   at once with `python tools_dev/bump_version.py X.Y.Z`;
//! * exposes the target triple (`AURA_TARGET`);
//! * on Windows, embeds `assets/icon.ico` (drawn from `assets/icon.svg` by `tools_dev/make_icons.py`) and version info
//!   into the executable, so Explorer, the taskbar, Task Manager (FileDescription) and Settings → Apps show it.

use std::path::PathBuf;

fn main() {
    let root =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
    let version = std::fs::read_to_string(root.join("VERSION"))
        .expect("VERSION file")
        .trim()
        .to_string();
    let crate_version = std::env::var("CARGO_PKG_VERSION").expect("cargo sets CARGO_PKG_VERSION");
    if version != crate_version {
        panic!(
            "VERSION says {version} but Cargo.toml says {crate_version}: run `python tools_dev/bump_version.py {version}`"
        );
    }
    println!("cargo:rerun-if-changed=VERSION");
    println!("cargo:rerun-if-changed=build.rs");

    let target = std::env::var("TARGET").expect("cargo always sets TARGET");
    println!("cargo:rustc-env=AURA_TARGET={target}");

    windows_resources(&root, &version);
}

/// The .exe's icon and version info — only when building for Windows.
#[cfg(windows)]
fn windows_resources(root: &std::path::Path, version: &str) {
    let ico = root.join("assets").join("icon.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon(&ico.to_string_lossy())
        // (Task Manager names the process by FileDescription)
        .set("FileDescription", "Aura Portfolio")
        .set("ProductName", "Aura Portfolio")
        .set("CompanyName", "Akulav")
        .set("InternalName", "aura-portfolio")
        .set("OriginalFilename", "aura-portfolio.exe")
        .set("LegalCopyright", "© 2026 Akulav. Source-available license.")
        .set("FileVersion", version)
        .set("ProductVersion", version);
    if let Err(e) = res.compile() {
        panic!("couldn't embed the icon and version info (needs the Windows SDK's rc.exe): {e}");
    }
}

#[cfg(not(windows))]
fn windows_resources(_root: &std::path::Path, _version: &str) {}
