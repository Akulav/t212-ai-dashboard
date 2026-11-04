//! Build script: exposes the compilation target triple to the crate so the
//! auto-updater can pick the matching GitHub Release asset.

fn main() {
    let target = std::env::var("TARGET").expect("cargo always sets TARGET");
    println!("cargo:rustc-env=AURA_TARGET={target}");
    println!("cargo:rerun-if-changed=build.rs");
}
