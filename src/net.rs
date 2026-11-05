//! Networking helpers shared by the LLM installer, model downloader and
//! auto-updater: a preconfigured HTTP client, resumable streaming downloads
//! with progress reporting, and GitHub Releases lookup.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::{Client, StatusCode, header};
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

/// Builds the shared HTTP client (rustls, sensible timeouts, identifying UA).
pub fn http_client() -> Client {
    Client::builder()
        .user_agent(format!("aura-portfolio/{}", crate::VERSION))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .expect("static reqwest configuration is valid")
}

/// Byte-level progress of a download.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    /// Bytes on disk so far (including any resumed portion).
    pub downloaded: u64,
    /// Total size, if the server reported it.
    pub total: Option<u64>,
}

impl Progress {
    /// Completion in `0.0..=1.0`, if the total is known.
    pub fn fraction(&self) -> Option<f32> {
        self.total
            .filter(|t| *t > 0)
            .map(|t| (self.downloaded as f64 / t as f64).clamp(0.0, 1.0) as f32)
    }
}

/// Streams `url` to `dest`, resuming a previous partial download if present.
///
/// Data is written to `<dest>.part` and atomically renamed on completion, so
/// `dest` existing always means the file is complete.
pub async fn download_to_file(
    client: &Client,
    url: &str,
    dest: &Path,
    mut on_progress: impl FnMut(Progress),
) -> Result<()> {
    let part = part_path(dest);
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut existing = tokio::fs::metadata(&part)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    let mut request = client.get(url);
    if existing > 0 {
        request = request.header(header::RANGE, format!("bytes={existing}-"));
    }
    let mut response = request.send().await.with_context(|| format!("GET {url}"))?;

    match response.status() {
        StatusCode::PARTIAL_CONTENT => {}
        StatusCode::RANGE_NOT_SATISFIABLE if existing > 0 => {
            // The partial file is already complete.
            tokio::fs::rename(&part, dest).await?;
            return Ok(());
        }
        s if s.is_success() => existing = 0, // server ignored Range: restart
        s => bail!("download failed: HTTP {s} for {url}"),
    }

    let total = response.content_length().map(|len| len + existing);
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(existing > 0)
        .truncate(existing == 0)
        .open(&part)
        .await
        .with_context(|| format!("opening {}", part.display()))?;

    let mut progress = Progress {
        downloaded: existing,
        total,
    };
    on_progress(progress);
    while let Some(chunk) = response.chunk().await.context("connection interrupted")? {
        file.write_all(&chunk).await?;
        progress.downloaded += chunk.len() as u64;
        on_progress(progress);
    }
    file.flush().await?;
    drop(file);

    if let Some(total) = total
        && progress.downloaded != total
    {
        bail!(
            "download truncated: got {} of {total} bytes",
            progress.downloaded
        );
    }
    tokio::fs::rename(&part, dest).await?;
    Ok(())
}

/// `<path>.part`
pub fn part_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// A GitHub release (subset of the REST API payload).
#[derive(Debug, Clone, Deserialize)]
pub struct GithubRelease {
    /// Git tag, e.g. `v0.2.0` or `b6123`.
    pub tag_name: String,
    /// Web page of the release.
    pub html_url: String,
    /// Markdown release notes.
    #[serde(default)]
    pub body: Option<String>,
    /// Downloadable files.
    pub assets: Vec<GithubAsset>,
    /// Marked as a pre-release (llama.cpp publishes its builds this way).
    #[serde(default)]
    pub prerelease: bool,
    /// Unpublished draft.
    #[serde(default)]
    pub draft: bool,
}

/// A file attached to a [`GithubRelease`].
#[derive(Debug, Clone, Deserialize)]
pub struct GithubAsset {
    /// File name.
    pub name: String,
    /// Direct download URL.
    pub browser_download_url: String,
    /// Size in bytes.
    pub size: u64,
}

/// Fetches the latest non-draft, non-prerelease release of `owner/repo`.
pub async fn latest_release(client: &Client, repo: &str) -> Result<GithubRelease> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let release = client
        .get(&url)
        .header(header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()?
        .json::<GithubRelease>()
        .await?;
    Ok(release)
}

/// Lists the most recent releases of `owner/repo` (newest first), including
/// pre-releases. `latest_release` skips pre-releases, which is where some
/// projects (e.g. llama.cpp) now publish their binaries.
pub async fn list_releases(
    client: &Client,
    repo: &str,
    per_page: u32,
) -> Result<Vec<GithubRelease>> {
    let url = format!("https://api.github.com/repos/{repo}/releases?per_page={per_page}");
    let releases = client
        .get(&url)
        .header(header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()?
        .json::<Vec<GithubRelease>>()
        .await?;
    Ok(releases)
}

/// Downloads a small text file.
pub async fn fetch_text(client: &Client, url: &str) -> Result<String> {
    Ok(client
        .get(url)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()?
        .text()
        .await?)
}

/// Formats a byte count as `1.23 GB` (decimal units, matching download sites).
pub fn fmt_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_format() {
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(4_680_000_000), "4.68 GB");
    }

    #[test]
    fn progress_fraction() {
        let p = Progress {
            downloaded: 50,
            total: Some(200),
        };
        assert_eq!(p.fraction(), Some(0.25));
        assert_eq!(
            Progress {
                downloaded: 5,
                total: None
            }
            .fraction(),
            None
        );
    }
}
