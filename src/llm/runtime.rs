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

use anyhow::{Context, Result, anyhow, bail};
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

/// The build tag upstream currently recommends, if it publishes one.
///
/// Since late 2026 llama.cpp's "latest" release is a versioned stub (e.g.
/// `v0.9.0`) whose only asset is `nightly-tag.txt`, naming the current build
/// (e.g. `b12110`); the binaries live on per-build pre-releases.
async fn preferred_build_tag(http: &Client, releases: &[GithubRelease]) -> Option<String> {
    let asset = releases
        .iter()
        .filter(|r| !r.draft)
        .flat_map(|r| r.assets.iter())
        .find(|a| a.name == "nightly-tag.txt")?;
    let tag = net::fetch_text(http, &asset.browser_download_url)
        .await
        .ok()?;
    let tag = tag.trim();
    (!tag.is_empty() && tag.len() < 40).then(|| tag.to_string())
}

/// Chooses the release to install: the `preferred` build if it has a
/// compatible asset, otherwise the newest release that does. Drafts and
/// asset-less stubs are skipped; a build still uploading simply won't match
/// yet and the next one is used.
pub fn pick_release<'a>(
    releases: &'a [GithubRelease],
    preferred: Option<&str>,
    hw: &HardwareProfile,
) -> Result<(&'a GithubRelease, Backend, Vec<GithubAsset>)> {
    let preferred_first = preferred
        .and_then(|tag| releases.iter().find(|r| r.tag_name == tag))
        .into_iter()
        .chain(releases.iter());
    preferred_first
        .filter(|r| !r.draft)
        .find_map(|r| {
            select_assets(r, hw)
                .ok()
                .map(|(b, assets)| (r, b, assets.into_iter().cloned().collect()))
        })
        .ok_or_else(|| {
            anyhow!(
                "no compatible llama.cpp build found in the {} most recent releases for {}",
                releases.len(),
                crate::TARGET
            )
        })
}

/// Archive names a rule would match for build `tag`, for probing download
/// URLs directly. Only single-token rules (`-bin-<platform>`) qualify; CUDA
/// needs a matching runtime-DLL bundle, so it is left to the API path.
pub fn direct_asset_names(rule: &AssetRule, tag: &str) -> Vec<String> {
    match rule.include {
        [token] if token.starts_with("-bin-") && rule.backend != Backend::Cuda => {
            vec![
                format!("llama-{tag}{token}.zip"),
                format!("llama-{tag}{token}.tar.gz"),
            ]
        }
        _ => Vec::new(),
    }
}

/// Fallback that avoids the GitHub REST API: read the current build tag from
/// the stable release's `nightly-tag.txt` (a plain download URL), then probe
/// the expected archive names for this platform in preference order.
async fn direct_download_plan(
    http: &Client,
    hw: &HardwareProfile,
) -> Result<(String, Backend, Vec<GithubAsset>)> {
    let base = format!("https://github.com/{LLAMA_CPP_REPO}/releases");
    let tag = net::fetch_text(http, &format!("{base}/latest/download/nightly-tag.txt"))
        .await
        .context("could not read the current llama.cpp build tag")?
        .trim()
        .to_string();
    if tag.is_empty() || tag.len() > 40 || tag.contains('/') {
        bail!("unexpected llama.cpp build tag {tag:?}");
    }
    for rule in asset_rules(hw) {
        for name in direct_asset_names(&rule, &tag) {
            let url = format!("{base}/download/{tag}/{name}");
            let ok = http
                .head(&url)
                .timeout(std::time::Duration::from_secs(20))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success());
            if ok {
                return Ok((
                    tag,
                    rule.backend,
                    vec![GithubAsset {
                        name,
                        browser_download_url: url,
                        size: 0,
                    }],
                ));
            }
        }
    }
    bail!(
        "no compatible llama.cpp archive found for build {tag} on {}",
        crate::TARGET
    )
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
    let (tag, backend, assets) = match net::list_releases(http, LLAMA_CPP_REPO, 30).await {
        Ok(releases) => {
            let preferred = preferred_build_tag(http, &releases).await;
            let (release, backend, assets) = pick_release(&releases, preferred.as_deref(), hw)?;
            (release.tag_name.clone(), backend, assets)
        }
        Err(api_err) => {
            // Typically the unauthenticated GitHub API rate limit (60/h per IP).
            tracing::warn!("GitHub API unavailable ({api_err:#}); trying direct downloads");
            direct_download_plan(http, hw).await.with_context(|| {
                format!(
                    "GitHub API unavailable ({api_err:#}) and the direct-download fallback failed"
                )
            })?
        }
    };
    tracing::info!(%tag, %backend, "installing llama.cpp runtime");

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
    let dir_name = format!("{tag}-{backend}").to_lowercase();
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
        tag,
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
                "llama-b9999-bin-win-cpu-arm64.zip",
                "llama-b9999-bin-macos-arm64.zip",
                "llama-b9999-bin-macos-x64.zip",
                "llama-b9999-bin-ubuntu-x64.zip",
                "llama-b9999-bin-ubuntu-vulkan-x64.zip",
                "llama-b9999-bin-ubuntu-arm64.zip",
            ]
            .into_iter()
            .map(asset)
            .collect(),
            prerelease: true,
            draft: false,
        }
    }

    fn build(tag: &str) -> GithubRelease {
        let mut r = release();
        r.tag_name = tag.into();
        r
    }

    /// Upstream layout since late 2026: a versioned stub "latest" release with
    /// only `nightly-tag.txt`, and binaries on per-build pre-releases.
    fn upstream_layout() -> Vec<GithubRelease> {
        let stub = GithubRelease {
            tag_name: "v0.9.0".into(),
            html_url: String::new(),
            body: None,
            assets: vec![asset("nightly-tag.txt")],
            prerelease: false,
            draft: false,
        };
        let uploading = GithubRelease {
            assets: vec![asset("llama-b12120-ui.tar.gz")],
            ..build("b12120")
        };
        vec![uploading, stub, build("b12119"), build("b12110")]
    }

    #[test]
    fn skips_stub_and_incomplete_releases() {
        let rels = upstream_layout();
        let (r, _, assets) = pick_release(&rels, None, &hw(false, false)).unwrap();
        assert_eq!(r.tag_name, "b12119");
        assert!(!assets.is_empty());
    }

    #[test]
    fn prefers_nightly_tag_build() {
        let rels = upstream_layout();
        let (r, _, _) = pick_release(&rels, Some("b12110"), &hw(false, false)).unwrap();
        assert_eq!(r.tag_name, "b12110");
        // Unknown preferred tag falls back to the newest compatible build.
        let (r, _, _) = pick_release(&rels, Some("b1"), &hw(false, false)).unwrap();
        assert_eq!(r.tag_name, "b12119");
    }

    #[test]
    fn direct_names_match_real_upstream_assets() {
        // Every non-CUDA rule's derived names must include an archive that
        // the rule itself matches (i.e. the fallback probes real file names).
        for rule in asset_rules(&hw(true, true)) {
            let names = direct_asset_names(&rule, "b12119");
            if rule.backend == Backend::Cuda {
                assert!(names.is_empty());
            } else {
                assert!(names.iter().all(|n| rule.matches(n)), "{names:?}");
            }
        }
        let vulkan = AssetRule {
            backend: Backend::Vulkan,
            include: &["-bin-win-vulkan-x64"],
            exclude: &[],
        };
        assert_eq!(
            direct_asset_names(&vulkan, "b12119")[0],
            "llama-b12119-bin-win-vulkan-x64.zip"
        );
    }

    #[test]
    fn no_compatible_release_is_an_error() {
        let rels = vec![upstream_layout().remove(1)];
        assert!(pick_release(&rels, None, &hw(false, false)).is_err());
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

    /// Runs on every CI platform: whatever this target is, a CPU-only
    /// machine must resolve to exactly one asset matched by its own rules.
    #[test]
    fn current_platform_resolves_an_asset() {
        let rel = release();
        let machine = hw(false, false);
        let (backend, assets) = select_assets(&rel, &machine).unwrap();
        assert_eq!(assets.len(), 1);
        let rules = asset_rules(&machine);
        assert!(
            rules
                .iter()
                .any(|r| r.backend == backend && r.matches(&assets[0].name))
        );
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn apple_silicon_uses_metal() {
        let rel = release();
        let (backend, assets) = select_assets(&rel, &hw(false, false)).unwrap();
        assert_eq!(backend, Backend::Metal);
        assert_eq!(assets[0].name, "llama-b9999-bin-macos-arm64.zip");
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
