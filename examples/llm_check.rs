//! End-to-end check of the local-AI setup on this machine:
//! hardware detection → model recommendation → real llama.cpp install into a
//! throwaway folder → `llama-server --version`.
//!
//! ```text
//! cargo run --example llm_check
//! ```

use aura_portfolio::llm::{hardware, models, runtime};
use aura_portfolio::net;
use aura_portfolio::paths::AppPaths;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let hw = hardware::detect();
    println!(
        "CPU: {} ({} threads), RAM {}",
        hw.cpu_name,
        hw.cpu_threads,
        net::fmt_bytes(hw.ram_total)
    );
    for g in &hw.gpus {
        println!(
            "GPU: {} [{:?}] {} {}",
            g.name,
            g.vendor,
            net::fmt_bytes(g.vram_total),
            if g.unified { "unified" } else { "dedicated" }
        );
    }
    println!("Vulkan: {}", hw.vulkan_available);
    let rec = models::recommend(&hw);
    println!(
        "Recommended: {} {} ({})",
        rec.name,
        rec.quant,
        net::fmt_bytes(rec.size_bytes)
    );
    for m in models::CATALOG
        .iter()
        .filter(|m| m.id.starts_with("qwen3.8"))
    {
        println!(
            "  {} {}: {} · -ngl {}",
            m.name,
            m.quant,
            models::assess(m, &hw).label(),
            models::gpu_layers(m, &hw)
        );
    }

    let dir = std::env::temp_dir().join(format!("aura-llm-check-{}", std::process::id()));
    let paths = AppPaths::at(dir.clone(), false)?;
    let http = net::http_client();
    let mut last_pct = -1i32;
    let manifest = runtime::install(&http, &paths, &hw, |stage| match stage {
        runtime::InstallStage::Downloading { asset, progress } => {
            let pct = (progress.fraction().unwrap_or(0.0) * 100.0) as i32;
            if pct / 25 != last_pct / 25 {
                println!("  downloading {asset}: {pct}%");
                last_pct = pct;
            }
        }
        other => println!("  {other:?}"),
    })
    .await?;
    println!(
        "Installed {} ({}) from {:?}",
        manifest.tag, manifest.backend, manifest.assets
    );

    let exe = manifest.server_path(&paths);
    let out = std::process::Command::new(&exe)
        .arg("--version")
        .current_dir(exe.parent().unwrap())
        .output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    println!("llama-server --version → exit {:?}", out.status.code());
    for line in text.lines().filter(|l| !l.trim().is_empty()).take(8) {
        println!("  {line}");
    }
    let out = std::process::Command::new(&exe)
        .arg("--list-devices")
        .current_dir(exe.parent().unwrap())
        .output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    println!("llama-server --list-devices → exit {:?}", out.status.code());
    for line in text.lines().filter(|l| !l.trim().is_empty()).take(12) {
        println!("  {line}");
    }
    let _ = std::fs::remove_dir_all(dir);
    Ok(())
}
