//! Updates of this program from the GitHub releases of [`GITHUB_REPO`]: checked a minute after start and every
//! 6 hours (or when asked from the sidebar).
//!
//! Every release carries exactly one file per system plus `SHA256SUMS.txt`, and that file both installs and updates:
//!
//! | System        | Asset ([`asset_name`])       | Applied by                                               |
//! |---------------|------------------------------|----------------------------------------------------------|
//! | Windows x64   | `AuraPortfolio-setup.exe`    | running the setup silently in update mode, same folder   |
//! | Linux x64     | `AuraPortfolio-linux-x64`    | swapping the program in the app folder                   |
//! | macOS arm64   | `AuraPortfolio-macos-arm64`  | swapping the program inside the app bundle (re-signed)   |
//!
//! The download is checked against the release's `SHA256SUMS.txt` (and must be a program for this system) before
//! anything changes. On Windows it is kept in the app folder (`data/downloads`), never in `%TEMP%`. On Linux/macOS
//! the running program is renamed to `<exe>.old` and the new one put in its place; the next start removes the `.old`.
//! Data, models, the runtime and settings are never touched.
//!
//! A program built on this computer (anything under a `target/` folder) never updates itself.
//! A private repository needs a read-only token: `github_token = <token>` in `data/local.config` (never committed).

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use reqwest::{Client, header};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::net::Progress;

/// Repository that publishes Aura releases.
pub const GITHUB_REPO: &str = "Akulav/t212-ai-dashboard";
/// First check after start.
pub const FIRST_CHECK: Duration = Duration::from_secs(60);
/// Then every …
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);

/// Windows release file: the setup (installs the first time, updates later).
pub const ASSET_WINDOWS: &str = "AuraPortfolio-setup.exe";
/// Linux release file: the program, which installs itself into a folder you pick.
pub const ASSET_LINUX: &str = "AuraPortfolio-linux-x64";
/// macOS release file: the program, which installs itself as an app into a folder you pick.
pub const ASSET_MACOS: &str = "AuraPortfolio-macos-arm64";
/// Checksums of the three files above.
pub const ASSET_SUMS: &str = "SHA256SUMS.txt";

/// The release file for this computer.
pub fn asset_name() -> &'static str {
    if cfg!(windows) {
        ASSET_WINDOWS
    } else if cfg!(target_os = "macos") {
        ASSET_MACOS
    } else {
        ASSET_LINUX
    }
}

/// Does `head` (the first bytes of a download) look like a program for this system?
pub fn looks_like_program(head: &[u8]) -> bool {
    if cfg!(windows) {
        head.starts_with(b"MZ")
    } else if cfg!(target_os = "macos") {
        head.len() >= 4
            && matches!(
                &head[..4],
                [0xcf, 0xfa, 0xed, 0xfe] | [0xca, 0xfe, 0xba, 0xbe]
            )
    } else {
        head.starts_with(b"\x7fELF")
    }
}

/// A newer release that can be installed.
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateInfo {
    /// Version without the `v`.
    pub version: String,
    /// Release web page.
    pub html_url: String,
    /// Release notes (Markdown).
    pub notes: String,
    /// Public download URL of this platform's program.
    pub asset_url: String,
    /// Public download URL of `SHA256SUMS.txt`.
    pub sums_url: String,
    /// The same two files through GitHub's API (used with a token).
    pub asset_api: String,
    /// `SHA256SUMS.txt` through the API.
    pub sums_api: String,
}

/// Updater state machine rendered by the sidebar.
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
    ReadyToRestart(String),
    /// Something went wrong.
    Failed(String),
    /// Built on this computer: never updates itself.
    SelfBuilt,
}

/// "v2.10.1" → [2, 10, 1] (a pre-release suffix is ignored for the numbers).
fn numbers(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches('v')
        .split(['-', '+'])
        .next()
        .unwrap_or("")
        .split('.')
        .map(|x| x.parse().unwrap_or(0))
        .collect()
}

/// Is `latest` newer than `current`?
pub fn newer(latest: &str, current: &str) -> bool {
    let (a, b) = (numbers(latest), numbers(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    url: String,
}

/// Parses GitHub's "latest release" JSON into this platform's update.
pub fn parse_release(json: &str) -> Result<UpdateInfo> {
    let r: Release = serde_json::from_str(json).context("GitHub's answer")?;
    let find = |name: &str| r.assets.iter().find(|a| a.name == name);
    let (asset, sums) = (find(asset_name()), find(ASSET_SUMS));
    Ok(UpdateInfo {
        version: r.tag_name.trim_start_matches('v').to_string(),
        html_url: r.html_url,
        notes: r.body.unwrap_or_default().chars().take(4000).collect(),
        asset_url: asset
            .map(|a| a.browser_download_url.clone())
            .unwrap_or_default(),
        sums_url: sums
            .map(|a| a.browser_download_url.clone())
            .unwrap_or_default(),
        asset_api: asset.map(|a| a.url.clone()).unwrap_or_default(),
        sums_api: sums.map(|a| a.url.clone()).unwrap_or_default(),
    })
}

/// This program's checksum in `SHA256SUMS.txt` (`<hex>  <name>` lines, as `sha256sum` writes them).
pub fn checksum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut parts = l.split_whitespace();
        let (hash, file) = (parts.next()?, parts.next()?);
        (file.trim_start_matches('*') == name && hash.len() == 64).then(|| hash.to_lowercase())
    })
}

/// A read-only GitHub token for a private repository: `github_token = …` in `data/local.config`.
pub fn github_token(data_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(data_dir.join("local.config")).ok()?;
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == "github_token")
            .then(|| v.trim().trim_matches('"').to_string())
            .filter(|t| !t.is_empty())
    })
}

async fn get(
    http: &Client,
    url: &str,
    token: Option<&str>,
    accept: &str,
) -> Result<reqwest::Response> {
    let mut req = http.get(url).header(header::ACCEPT, accept);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req
        .send()
        .await
        .with_context(|| format!("couldn't reach GitHub ({url})"))?;
    match resp.status().as_u16() {
        404 if token.is_none() => bail!(
            "GitHub shows no release of {GITHUB_REPO} (no release yet, or a private repository: put a read-only \
             token in data/local.config as github_token = …)"
        ),
        404 => bail!(
            "GitHub shows no release of {GITHUB_REPO} (no release yet, or the token can't read it)"
        ),
        401 | 403 => bail!(
            "GitHub refused ({}): check the token in data/local.config, or try again later",
            resp.status()
        ),
        _ => Ok(resp.error_for_status()?),
    }
}

/// A release file: through the API with a token (private repository), else its public download address.
async fn fetch(
    http: &Client,
    public: &str,
    api: &str,
    token: Option<&str>,
) -> Result<reqwest::Response> {
    match token {
        Some(_) if !api.is_empty() => get(http, api, token, "application/octet-stream").await,
        _ => get(http, public, None, "application/octet-stream").await,
    }
}

/// Asks GitHub for the latest release; `Some` if it is newer than this program and has this platform's file.
pub async fn check_latest(http: &Client, token: Option<&str>) -> Result<Option<UpdateInfo>> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let body = get(http, &url, token, "application/vnd.github+json")
        .await?
        .text()
        .await?;
    let info = parse_release(&body)?;
    if !newer(&info.version, crate::VERSION) {
        return Ok(None);
    }
    if info.asset_url.is_empty() || info.sums_url.is_empty() {
        bail!(
            "release {} has no {} with a checksum yet",
            info.version,
            asset_name()
        );
    }
    Ok(Some(info))
}

/// A program built on this computer (`cargo build`: under a `target` folder) never updates or installs itself.
pub fn self_built(exe: &Path) -> bool {
    exe.components().any(|c| c.as_os_str() == "target")
}

/// `aura-portfolio.exe` → `aura-portfolio.exe.new` / `.old`.
pub fn sibling(exe: &Path, ext: &str) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(format!(".{ext}"));
    PathBuf::from(s)
}

/// Puts `new` in `exe`'s place; the running program is kept as `.old` until the next start removes it.
pub fn swap(exe: &Path, new: &Path) -> Result<()> {
    let old = sibling(exe, "old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).context("couldn't move the running program aside")?;
    if let Err(e) = std::fs::rename(new, exe) {
        let _ = std::fs::rename(&old, exe); // (put it back: nothing changed)
        return Err(anyhow!("couldn't put the new program in place: {e}"));
    }
    Ok(())
}

/// (At start.) Removes what the last update left: the previous program (`.old`) and an unfinished download (`.new`).
pub fn clean_up(exe: &Path) {
    for ext in ["old", "new"] {
        let p = sibling(exe, ext);
        if p.exists() {
            for _ in 0..20 {
                if std::fs::remove_file(&p).is_ok() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(250)); // (the old one may still be ending)
            }
        }
    }
}

/// Where the downloaded Windows setup waits for [`restart`]: inside the app folder's `data/downloads`.
pub fn staged_setup(downloads: &Path) -> PathBuf {
    downloads.join(ASSET_WINDOWS)
}

/// Downloads and verifies this system's release file. Windows keeps the setup in `downloads` for [`restart`];
/// Linux/macOS swap the new program in right away. Afterwards [`restart`] starts the new version.
pub async fn install(
    http: &Client,
    info: &UpdateInfo,
    token: Option<&str>,
    downloads: &Path,
    mut on_progress: impl FnMut(Progress),
) -> Result<()> {
    let exe = std::env::current_exe().context("where is this program?")?;
    if self_built(&exe) {
        bail!("this program was built on this computer (cargo): update it from the source instead");
    }
    let sums = fetch(http, &info.sums_url, &info.sums_api, token)
        .await?
        .text()
        .await
        .context("the checksums")?;
    let want = checksum_for(&sums, asset_name())
        .ok_or_else(|| anyhow!("the release has no checksum for {}", asset_name()))?;

    let mut resp = fetch(http, &info.asset_url, &info.asset_api, token)
        .await
        .context("download failed")?;
    let total = resp.content_length();
    let mut data = Vec::with_capacity(total.unwrap_or(1 << 24) as usize);
    while let Some(chunk) = resp.chunk().await.context("download interrupted")? {
        data.extend_from_slice(&chunk);
        on_progress(Progress {
            downloaded: data.len() as u64,
            total,
        });
    }
    if total.is_some_and(|t| t != data.len() as u64) {
        bail!("download cut short");
    }
    let got = format!("{:x}", Sha256::digest(&data));
    if got != want {
        bail!("the download doesn't match the release's checksum: not installed");
    }
    if !looks_like_program(&data) {
        bail!("the download is not a program for this system: not installed");
    }
    if cfg!(windows) {
        std::fs::create_dir_all(downloads).context("couldn't make the downloads folder")?;
        std::fs::write(staged_setup(downloads), &data).context("couldn't save the setup")?;
        tracing::info!("downloaded the setup for {}", info.version);
        return Ok(());
    }
    let new = sibling(&exe, "new");
    std::fs::write(&new, &data).context("couldn't save the new program")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755));
    }
    swap(&exe, &new)?;
    tracing::info!(
        "updated the program from {} to {}",
        crate::VERSION,
        info.version
    );
    Ok(())
}

/// Applies the update and exits. Windows runs the downloaded setup silently in update mode into this program's
/// folder (it closes this program, installs and starts the new version with `--after-update`); Linux/macOS start the
/// already swapped-in program with `--after-update` (on macOS after re-signing the app bundle). The new program waits
/// for this one to release the data folder.
///
/// Call [`LlmManager::shutdown`](crate::llm::LlmManager::shutdown) first.
pub fn restart(downloads: &Path) -> Result<()> {
    let exe = std::env::current_exe()?;
    if cfg!(windows) {
        let setup = staged_setup(downloads);
        let dir = exe.parent().context("where is this program?")?;
        let mut cmd = std::process::Command::new(&setup);
        cmd.args([
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/CLOSEAPPLICATIONS",
            "/UPDATE",
        ])
        .arg(format!("/FROM={}", crate::VERSION))
        .arg(format!("/DIR={}", dir.display()));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0000_0008 | 0x0000_0200); // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
        }
        cmd.spawn()
            .with_context(|| format!("couldn't start {}", setup.display()))?;
        std::process::exit(0);
    }
    #[cfg(target_os = "macos")]
    if let Some(app) = crate::install::bundle_of(&exe) {
        crate::install::sign_bundle(&app);
    }
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--after-update");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0008 | 0x0000_0200); // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
    }
    cmd.spawn().context("couldn't start the new program")?;
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("1.10.0", "1.9.9"));
        assert!(newer("v1.3.1", "1.3.0"));
        assert!(!newer("1.3.0", "1.3.0"));
        assert!(!newer("1.2.9", "1.3.0"));
        assert!(newer("2", "1.99.99"));
        assert!(!newer("1.3.0-beta", "1.3.0"));
    }

    #[test]
    fn release_json_gives_this_computers_program() {
        let json = format!(
            r#"{{"tag_name": "v1.4.0", "html_url": "https://github.com/x/y/releases/tag/v1.4.0", "body": "- things",
              "assets": [
                {{"name": "SHA256SUMS.txt", "browser_download_url": "https://dl/sums", "url": "https://api/sums"}},
                {{"name": "{}", "browser_download_url": "https://dl/prog", "url": "https://api/prog"}},
                {{"name": "Aura-Portfolio-Setup.exe", "browser_download_url": "https://dl/setup", "url": "https://api/setup"}}]}}"#,
            asset_name()
        );
        let r = parse_release(&json).unwrap();
        assert_eq!(r.version, "1.4.0");
        assert_eq!(
            (r.asset_url.as_str(), r.asset_api.as_str()),
            ("https://dl/prog", "https://api/prog")
        );
        assert_eq!(r.sums_url, "https://dl/sums");
        assert!(r.notes.contains("things"));
        assert!(parse_release("{}").is_err());
    }

    #[test]
    fn checksum_lines_are_read_like_sha256sum_writes_them() {
        let h = "a".repeat(64);
        let sums = format!("{h}  {}\n{}  other\n", asset_name(), "b".repeat(64));
        assert_eq!(checksum_for(&sums, asset_name()), Some(h.clone()));
        assert_eq!(
            checksum_for(&format!("{h} *{}", asset_name()), asset_name()),
            Some(h)
        );
        assert_eq!(checksum_for(&sums, "missing"), None);
        assert_eq!(checksum_for("short  x", "x"), None);
    }

    #[test]
    fn swap_puts_the_new_program_in_place_and_keeps_the_old() {
        let dir = std::env::temp_dir().join(format!("aura-swap-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let exe = dir.join("prog.exe");
        std::fs::write(&exe, b"old").unwrap();
        let new = sibling(&exe, "new");
        std::fs::write(&new, b"new").unwrap();
        swap(&exe, &new).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert_eq!(std::fs::read(sibling(&exe, "old")).unwrap(), b"old");
        clean_up(&exe);
        assert!(!sibling(&exe, "old").exists());
        assert!(
            swap(&exe, &sibling(&exe, "new")).is_err(),
            "a missing download changes nothing"
        );
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_program_built_here_is_never_replaced() {
        assert!(self_built(Path::new(
            "D:/src/aura/target/release/aura-portfolio.exe"
        )));
        assert!(!self_built(Path::new(
            "C:/Users/me/AppData/Local/Aura Portfolio/aura-portfolio.exe"
        )));
    }

    #[test]
    fn token_comes_from_local_config() {
        let dir = std::env::temp_dir().join(format!("aura-token-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        assert_eq!(github_token(&dir), None);
        std::fs::write(
            dir.join("local.config"),
            "# private repo\ngithub_token = \"ghp_x\"\n",
        )
        .unwrap();
        assert_eq!(github_token(&dir).as_deref(), Some("ghp_x"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
