//! Zero-config installation of a portable `llama.cpp` build.
//!
//! On first use Aura queries the latest release of
//! [`ggml-org/llama.cpp`](https://github.com/ggml-org/llama.cpp/releases),
//! picks the best prebuilt archive for this OS / CPU / GPU, downloads and
//! extracts it under `runtime/llama.cpp/<tag>-<backend>/`, and records the
//! result in `runtime/manifest.json`.
//!
//! Backend preference:
//!
//! | Platform              | Order                         |
//! |-----------------------|-------------------------------|
//! | Windows x64           | CUDA 12 (NVIDIA) → Vulkan → CPU |
//! | Windows arm64         | CPU                           |
//! | macOS Apple Silicon   | Metal                         |
//! | macOS Intel           | CPU                           |
//! | Linux x64             | Vulkan → CPU                  |
//!
//! To support a new backend, add an [`AssetRule`] in [`asset_rules`].

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::hardware::HardwareProfile;
use crate::archive;
use crate::net::{self, GithubAsset, GithubRelease, Progress};
use crate::paths::AppPaths;

/// Upstream repository providing prebuilt binaries.
pub const LLAMA_CPP_REPO: &str = "ggml-org/llama.cpp";

/// Name of the server executable inside the archive.
pub const SERVER_EXE: &str = if cfg!(windows) {
    "llama-server.exe"
} else {
    "llama-server"
};

/// Compute backend of an installed llama.cpp build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Backend {
    /// NVIDIA CUDA.
    Cuda,
    /// Cross-vendor Vulkan (AMD, Intel, NVIDIA).
    Vulkan,
    /// Apple Metal.
    Metal,
    /// CPU only.
    Cpu,
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Cuda => "CUDA",
            Self::Vulkan => "Vulkan",
            Self::Metal => "Metal",
            Self::Cpu => "CPU",
        })
    }
}

/// Record of an installed runtime, stored as `runtime/manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeManifest {
    /// llama.cpp release tag, e.g. `b6500`.
    pub tag: String,
    /// Backend of the installed build.
    pub backend: Backend,
    /// Archive names that were installed.
    pub assets: Vec<String>,
    /// Path of `llama-server`, relative to [`AppPaths::runtime`].
    pub server_exe: PathBuf,
    /// Installation time.
    pub installed_at: DateTime<Utc>,
}

impl RuntimeManifest {
    /// Absolute path of the `llama-server` executable.
    pub fn server_path(&self, paths: &AppPaths) -> PathBuf {
        paths.runtime.join(&self.server_exe)
    }
}

fn manifest_path(paths: &AppPaths) -> PathBuf {
    paths.runtime.join("manifest.json")
}

/// Returns the installed runtime if its manifest and executable both exist.
pub fn installed(paths: &AppPaths) -> Option<RuntimeManifest> {
    let bytes = std::fs::read(manifest_path(paths)).ok()?;
    let manifest: RuntimeManifest = serde_json::from_slice(&bytes).ok()?;
    manifest.server_path(paths).is_file().then_some(manifest)
}

/// Install progress reported to the UI.
#[derive(Debug, Clone)]
pub enum InstallStage {
    /// Querying GitHub for the latest release.
    Resolving,
    /// Downloading an archive.
    Downloading {
        /// Asset file name.
        asset: String,
        /// Byte progress.
        progress: Progress,
    },
    /// Unpacking archives.
    Extracting,
}

/// A rule matching a release asset name for a given backend.
#[derive(Debug, Clone)]
pub struct AssetRule {
    /// Backend this asset provides.
    pub backend: Backend,
    /// All substrings must be present (lower-case).
    pub include: &'static [&'static str],
    /// None of these substrings may be present.
    pub exclude: &'static [&'static str],
}

impl AssetRule {
    fn matches(&self, name: &str) -> bool {
        let name = name.to_lowercase();
        (name.ends_with(".zip") || name.ends_with(".tar.gz"))
            && !name.starts_with("cudart")
            && self.include.iter().all(|t| name.contains(t))
            && !self.exclude.iter().any(|t| name.contains(t))
    }
}

/// Ordered backend preferences for this platform and hardware.
pub fn asset_rules(hw: &HardwareProfile) -> Vec<AssetRule> {
    let mut rules = Vec::new();
    let rule = |backend, include, exclude| AssetRule {
        backend,
        include,
        exclude,
    };

    if cfg!(all(windows, target_arch = "x86_64")) {
        if hw.has_nvidia() {
            rules.push(rule(Backend::Cuda, &["-bin-win-cuda-12", "-x64"], &[]));
        }
        if hw.vulkan_available {
            rules.push(rule(Backend::Vulkan, &["-bin-win-vulkan-x64"], &[]));
        }
        rules.push(rule(Backend::Cpu, &["-bin-win-cpu-x64"], &[]));
    } else if cfg!(all(windows, target_arch = "aarch64")) {
        rules.push(rule(Backend::Cpu, &["-bin-win-cpu-arm64"], &[]));
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        rules.push(rule(Backend::Metal, &["-bin-macos-arm64"], &[]));
    } else if cfg!(target_os = "macos") {
        rules.push(rule(Backend::Cpu, &["-bin-macos-x64"], &[]));
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        if hw.vulkan_available {
            rules.push(rule(Backend::Vulkan, &["-bin-ubuntu-vulkan-x64"], &[]));
        }
        rules.push(rule(
            Backend::Cpu,
            &["-bin-ubuntu-x64"],
            &["vulkan", "cuda", "rocm", "hip"],
        ));
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        rules.push(rule(Backend::Cpu, &["-bin-ubuntu-arm64"], &["vulkan"]));
    }
    rules
}

/// Chooses the archives to install from `release`: the main build plus, for
/// CUDA on Windows, the matching `cudart-*` runtime DLL bundle.
pub fn select_assets<'a>(
    release: &'a GithubRelease,
    hw: &HardwareProfile,
) -> Result<(Backend, Vec<&'a GithubAsset>)> {
    for rule in asset_rules(hw) {
        let Some(main) = release.assets.iter().find(|a| rule.matches(&a.name)) else {
            continue;
        };
        let mut assets = vec![main];
        if rule.backend == Backend::Cuda {
            let cuda_tag = cuda_version_token(&main.name)
                .ok_or_else(|| anyhow!("cannot parse CUDA version from {}", main.name))?;
            match release.assets.iter().find(|a| {
                let n = a.name.to_lowercase();
                n.starts_with("cudart") && n.contains("win") && n.contains(&cuda_tag)
            }) {
                Some(rt) => assets.push(rt),
                None => continue, // without the DLLs the CUDA build will not start
            }
        }
        return Ok((rule.backend, assets));
    }
    Err(anyhow!(
        "no compatible llama.cpp build found in release {} for {}",
        release.tag_name,
        crate::TARGET
    ))
}

/// `"llama-b1-bin-win-cuda-12.4-x64.zip"` → `"cuda-12.4"`.
fn cuda_version_token(name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    let start = lower.find("cuda-")?;
    let rest = &lower[start + 5..];
    let end = rest.find('-')?;
    Some(format!("cuda-{}", &rest[..end]))
}

/// Downloads and installs the best llama.cpp build for this machine, replacing
/// any previously installed version once the new one is in place.
pub async fn install(
    http: &Client,
    paths: &AppPaths,
    hw: &HardwareProfile,
    mut on_stage: impl FnMut(InstallStage),
) -> Result<RuntimeManifest> {
    on_stage(InstallStage::Resolving);
    let release = net::latest_release(http, LLAMA_CPP_REPO)
        .await
        .context("could not query llama.cpp releases")?;
    let (backend, assets) = select_assets(&release, hw)?;
    tracing::info!(tag = %release.tag_name, %backend, "installing llama.cpp runtime");

    let mut archives = Vec::new();
    for asset in &assets {
        let dest = paths.downloads.join(&asset.name);
        let name = asset.name.clone();
        net::download_to_file(http, &asset.browser_download_url, &dest, |progress| {
            on_stage(InstallStage::Downloading {
                asset: name.clone(),
                progress,
            })
        })
        .await
        .with_context(|| format!("downloading {}", asset.name))?;
        archives.push(dest);
    }

    on_stage(InstallStage::Extracting);
    let dir_name = format!("{}-{}", release.tag_name, backend).to_lowercase();
    let install_root = paths.runtime.join("llama.cpp").join(&dir_name);
    let runtime_root = paths.runtime.clone();
    let archives_for_blocking = archives.clone();
    let server_rel = tokio::task::spawn_blocking(move || -> Result<PathBuf> {
        if install_root.exists() {
            std::fs::remove_dir_all(&install_root)?;
        }
        let (main, extras) = archives_for_blocking
            .split_first()
            .expect("at least one asset");
        archive::extract(main, &install_root)?;
        let server = archive::find_file(&install_root, SERVER_EXE)
            .ok_or_else(|| anyhow!("{SERVER_EXE} not found in {}", main.display()))?;
        // Runtime DLLs (cudart) must sit next to the executable.
        let exe_dir = server.parent().unwrap_or(&install_root).to_path_buf();
        for extra in extras {
            archive::extract(extra, &exe_dir)?;
        }
        Ok(server.strip_prefix(&runtime_root)?.to_path_buf())
    })
    .await??;

    let manifest = RuntimeManifest {
        tag: release.tag_name.clone(),
        backend,
        assets: assets.iter().map(|a| a.name.clone()).collect(),
        server_exe: server_rel,
        installed_at: Utc::now(),
    };
    std::fs::write(manifest_path(paths), serde_json::to_vec_pretty(&manifest)?)?;

    for archive in archives {
        let _ = tokio::fs::remove_file(archive).await;
    }
    remove_other_versions(&paths.runtime.join("llama.cpp"), &dir_name);
    Ok(manifest)
}

fn remove_other_versions(root: &Path, keep: &str) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name() != keep && entry.path().is_dir() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::hardware::{GpuInfo, GpuVendor};

    fn asset(name: &str) -> GithubAsset {
        GithubAsset {
            name: name.into(),
            browser_download_url: format!("https://x/{name}"),
            size: 1,
        }
    }

    fn release() -> GithubRelease {
        GithubRelease {
            tag_name: "b9999".into(),
            html_url: String::new(),
            body: None,
            assets: [
                "cudart-llama-bin-win-cuda-12.4-x64.zip",
                "llama-b9999-bin-win-cuda-12.4-x64.zip",
                "llama-b9999-bin-win-vulkan-x64.zip",
                "llama-b9999-bin-win-cpu-x64.zip",
                "llama-b9999-bin-macos-arm64.zip",
                "llama-b9999-bin-ubuntu-x64.zip",
                "llama-b9999-bin-ubuntu-vulkan-x64.zip",
            ]
            .into_iter()
            .map(asset)
            .collect(),
        }
    }

    fn hw(nvidia: bool, vulkan: bool) -> HardwareProfile {
        HardwareProfile {
            cpu_name: "test".into(),
            cpu_threads: 8,
            ram_total: 32 << 30,
            ram_available: 16 << 30,
            gpus: nvidia
                .then(|| GpuInfo {
                    name: "RTX".into(),
                    vendor: GpuVendor::Nvidia,
                    vram_total: 12 << 30,
                    unified: false,
                })
                .into_iter()
                .collect(),
            vulkan_available: vulkan,
        }
    }

    #[test]
    fn cuda_token() {
        assert_eq!(
            cuda_version_token("llama-b1-bin-win-cuda-12.4-x64.zip").as_deref(),
            Some("cuda-12.4")
        );
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    fn windows_prefers_cuda_with_runtime_dlls() {
        let rel = release();
        let (backend, assets) = select_assets(&rel, &hw(true, true)).unwrap();
        assert_eq!(backend, Backend::Cuda);
        assert_eq!(assets.len(), 2);
        assert!(assets[1].name.starts_with("cudart"));

        let (backend, _) = select_assets(&rel, &hw(false, true)).unwrap();
        assert_eq!(backend, Backend::Vulkan);
        let (backend, _) = select_assets(&rel, &hw(false, false)).unwrap();
        assert_eq!(backend, Backend::Cpu);
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn linux_cpu_rule_excludes_vulkan() {
        let rel = release();
        let (_, assets) = select_assets(&rel, &hw(false, false)).unwrap();
        assert_eq!(assets[0].name, "llama-b9999-bin-ubuntu-x64.zip");
    }
}
