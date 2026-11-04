//! Filesystem layout.
//!
//! Aura prefers **portable mode**: all runtime data (llama.cpp binaries, GGUF
//! models, logs, settings) lives in an `aura-data/` folder next to the
//! executable, so "delete the folder" is a complete uninstall. If that folder
//! is not writable (e.g. installed under `Program Files`, `/usr/bin`, or inside
//! a macOS `.app` bundle) we fall back to the per-user OS data directory.
//!
//! Set `AURA_DATA_DIR` to override both.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Resolved on-disk locations used by the application.
#[derive(Debug, Clone)]
pub struct AppPaths {
    /// Root data directory; every other path is inside it.
    pub root: PathBuf,
    /// Installed llama.cpp runtimes (`runtime/llama.cpp/<tag>-<backend>/`).
    pub runtime: PathBuf,
    /// Downloaded GGUF model files.
    pub models: PathBuf,
    /// Log files, including `llama-server.log`.
    pub logs: PathBuf,
    /// Scratch space for in-flight downloads and update staging.
    pub downloads: PathBuf,
    /// Settings JSON file.
    pub config_file: PathBuf,
    /// `true` when data lives next to the executable.
    pub portable: bool,
}

impl AppPaths {
    /// Resolves the data root (env override → portable → OS data dir) and
    /// creates all sub-directories.
    pub fn resolve() -> Result<Self> {
        if let Some(dir) = std::env::var_os("AURA_DATA_DIR") {
            return Self::at(PathBuf::from(dir), false);
        }
        if let Some(dir) = portable_root() {
            return Self::at(dir, true);
        }
        let dirs = directories::ProjectDirs::from("com", "akulav", "AuraPortfolio")
            .context("could not determine a home directory")?;
        Self::at(dirs.data_local_dir().to_path_buf(), false)
    }

    /// Builds the layout under an explicit root and creates the directories.
    pub fn at(root: PathBuf, portable: bool) -> Result<Self> {
        let paths = Self {
            runtime: root.join("runtime"),
            models: root.join("models"),
            logs: root.join("logs"),
            downloads: root.join("downloads"),
            config_file: root.join("settings.json"),
            root,
            portable,
        };
        for dir in [
            &paths.root,
            &paths.runtime,
            &paths.models,
            &paths.logs,
            &paths.downloads,
        ] {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("failed to create {}", dir.display()))?;
        }
        Ok(paths)
    }

    /// File that records the PID of a running `llama-server`, used to reap
    /// orphans left behind by a hard crash on platforms without OS-level
    /// child lifetime binding (macOS).
    pub fn server_pid_file(&self) -> PathBuf {
        self.runtime.join("llama-server.pid")
    }
}

/// Returns `<exe_dir>/aura-data` if it exists or can be created and written to.
fn portable_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    if is_inside_macos_bundle(&exe) {
        return None;
    }
    let dir = exe.parent()?.join("aura-data");
    std::fs::create_dir_all(&dir).ok()?;
    let probe = dir.join(".write-test");
    std::fs::write(&probe, b"ok").ok()?;
    let _ = std::fs::remove_file(&probe);
    Some(dir)
}

fn is_inside_macos_bundle(exe: &Path) -> bool {
    exe.to_string_lossy().contains(".app/Contents/")
}
