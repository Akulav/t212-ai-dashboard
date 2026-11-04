//! Self-update from GitHub Releases.
//!
//! 1. [`check_latest`] reads `releases/latest` of [`GITHUB_REPO`] and compares
//!    its semver tag with [`crate::VERSION`].
//! 2. The asset whose name contains this binary's target triple
//!    ([`crate::TARGET`]) is selected — names are produced by
//!    `.github/workflows/release.yml`.
//! 3. [`install`] downloads it, **verifies the published `.sha256`**, extracts
//!    the new executable and swaps it in place with `self-replace` (works even
//!    while the old exe is running on Windows).
//! 4. [`restart`] relaunches the new version.

use anyhow::{Context, Result, anyhow, bail};
use reqwest::Client;
use semver::Version;
use sha2::{Digest, Sha256};

use crate::net::{self, Progress};
use crate::paths::AppPaths;

/// Repository that publishes Aura releases.
pub const GITHUB_REPO: &str = "akulav/t212-ai-dashboard";

const EXE_NAME: &str = if cfg!(windows) {
    "aura-portfolio.exe"
} else {
    "aura-portfolio"
};

/// A newer release that can be installed.
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    /// New version.
    pub version: Version,
    /// Release web page.
    pub html_url: String,
    /// Release notes (Markdown).
    pub notes: String,
    /// Archive file name.
    pub asset_name: String,
    /// Archive download URL.
    pub asset_url: String,
    /// Checksum file URL, if published.
    pub checksum_url: Option<String>,
}

/// Updater state machine rendered by the top bar.
#[derive(Debug, Clone, Default)]
pub enum UpdateState {
    /// Nothing happened yet.
    #[default]
    Idle,
    /// Querying GitHub.
    Checking,
    /// Running the latest version.
    UpToDate,
    /// A newer version is available.
    Available(UpdateInfo),
    /// Downloading the update.
    Downloading(UpdateInfo, Progress),
    /// Installed; restart to apply.
    ReadyToRestart(Version),
    /// Something went wrong.
    Failed(String),
}

/// Returns `Some` if GitHub has a newer release with an asset for this platform.
pub async fn check_latest(http: &Client) -> Result<Option<UpdateInfo>> {
    let release = net::latest_release(http, GITHUB_REPO).await?;
    let latest = Version::parse(release.tag_name.trim_start_matches('v'))
        .with_context(|| format!("release tag {:?} is not semver", release.tag_name))?;
    let current = Version::parse(crate::VERSION)?;
    if latest <= current {
        return Ok(None);
    }
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.contains(crate::TARGET) && !a.name.ends_with(".sha256"))
        .ok_or_else(|| anyhow!("release {latest} has no build for {}", crate::TARGET))?;
    let checksum_url = release
        .assets
        .iter()
        .find(|a| a.name == format!("{}.sha256", asset.name))
        .map(|a| a.browser_download_url.clone());
    Ok(Some(UpdateInfo {
        version: latest,
        html_url: release.html_url,
        notes: release.body.unwrap_or_default(),
        asset_name: asset.name.clone(),
        asset_url: asset.browser_download_url.clone(),
        checksum_url,
    }))
}

/// Downloads, verifies and swaps in the new executable.
pub async fn install(
    http: &Client,
    paths: &AppPaths,
    info: &UpdateInfo,
    on_progress: impl FnMut(Progress),
) -> Result<()> {
    let archive = paths.downloads.join(&info.asset_name);
    let _ = tokio::fs::remove_file(&archive).await;
    net::download_to_file(http, &info.asset_url, &archive, on_progress).await?;

    match &info.checksum_url {
        Some(url) => {
            let expected = http
                .get(url)
                .send()
                .await?
                .error_for_status()?
                .text()
                .await?;
            let expected = expected
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_lowercase();
            let path = archive.clone();
            let actual = tokio::task::spawn_blocking(move || -> Result<String> {
                let mut hasher = Sha256::new();
                std::io::copy(&mut std::fs::File::open(path)?, &mut hasher)?;
                Ok(format!("{:x}", hasher.finalize()))
            })
            .await??;
            if actual != expected {
                let _ = tokio::fs::remove_file(&archive).await;
                bail!(
                    "checksum mismatch for {} (expected {expected}, got {actual})",
                    info.asset_name
                );
            }
        }
        None => tracing::warn!("release has no checksum file; skipping verification"),
    }

    let staging = paths.downloads.join(format!("update-{}", info.version));
    tokio::task::spawn_blocking(move || -> Result<()> {
        if staging.exists() {
            std::fs::remove_dir_all(&staging)?;
        }
        crate::archive::extract(&archive, &staging)?;
        let new_exe = crate::archive::find_file(&staging, EXE_NAME)
            .ok_or_else(|| anyhow!("{EXE_NAME} not found in update archive"))?;
        self_replace::self_replace(&new_exe).context("replacing the running executable")?;
        let _ = std::fs::remove_dir_all(&staging);
        let _ = std::fs::remove_file(&archive);
        Ok(())
    })
    .await??;
    Ok(())
}

/// Launches the (now replaced) executable and exits this process.
///
/// Call [`LlmManager::shutdown`](crate::llm::LlmManager::shutdown) first.
pub fn restart() -> Result<()> {
    let exe = std::env::current_exe()?;
    std::process::Command::new(exe)
        .spawn()
        .context("relaunching")?;
    std::process::exit(0);
}
