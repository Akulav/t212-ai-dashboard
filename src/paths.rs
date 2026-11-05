//! Filesystem layout. Everything Aura keeps lives in **the app folder's `data` folder**, next to the program:
//!
//! | System | App folder (the setup's default / self-install target) |
//! |---|---|
//! | Windows | `%LOCALAPPDATA%\Aura Portfolio` |
//! | Linux | `$XDG_DATA_HOME/aura-portfolio` or `~/.local/share/aura-portfolio` |
//! | macOS | `~/Library/Application Support/Aura Portfolio` |
//!
//! `data/` holds settings, the portfolio cache, insights, models, the llama.cpp runtime, logs and downloads. Updates
//! replace only the program; uninstalling asks before deleting the folder. Trading212 credentials are never here —
//! they're in the OS keychain.
//!
//! Exceptions, in order: `AURA_DATA_DIR` overrides the data folder; a pre-1.0 portable folder (`portable` marker or an
//! existing `aura-data/`) keeps using `aura-data/`. Data from before 1.0 (the per-user profile folder
//! `…\akulav\AuraPortfolio\data`) is moved into the app folder once, on the first start of an installed program.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Resolved on-disk locations used by the application.
#[derive(Debug, Clone)]
pub struct AppPaths {
    /// The data folder; every other path below is inside it.
    pub root: PathBuf,
    /// Installed llama.cpp runtimes (`runtime/llama.cpp/<tag>-<backend>/`).
    pub runtime: PathBuf,
    /// Downloaded GGUF model files.
    pub models: PathBuf,
    /// Log files, including `llama-server.log`.
    pub logs: PathBuf,
    /// Scratch space for in-flight downloads.
    pub downloads: PathBuf,
    /// Settings JSON file.
    pub config_file: PathBuf,
    /// `true` for a pre-1.0 portable folder (`aura-data/` next to the program).
    pub portable: bool,
}

impl AppPaths {
    /// The data folder of the app folder `app_dir` (see the module docs). `migrate` moves pre-1.0 data in once —
    /// pass `false` for programs built on this computer, so a developer build never takes the real data.
    pub fn for_app(app_dir: &Path, migrate: bool) -> Result<Self> {
        if let Some(dir) = std::env::var_os("AURA_DATA_DIR") {
            return Self::at(PathBuf::from(dir), false);
        }
        let legacy_portable = app_dir.join("aura-data");
        if app_dir.join("portable").is_file() || legacy_portable.is_dir() {
            return Self::at(legacy_portable, true);
        }
        let root = app_dir.join("data");
        if migrate && let Some(old) = legacy_profile_dir() {
            move_legacy_data(&old, &root);
        }
        Self::at(root, false)
    }

    /// Builds the layout under an explicit data folder and creates the directories.
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

    /// File that records the PID of a running `llama-server`, used to reap orphans left behind by a hard crash on
    /// platforms without OS-level child lifetime binding (macOS).
    pub fn server_pid_file(&self) -> PathBuf {
        self.runtime.join("llama-server.pid")
    }

    /// Held (file lock) while the program runs from this data folder: one Aura per app folder.
    pub fn lock_file(&self) -> PathBuf {
        self.root.join(".aura.lock")
    }
}

/// Where 0.1–0.3 kept data when installed: the per-user profile folder.
///
/// On Windows it's derived from `%LOCALAPPDATA%` (the same folder the `directories` crate used), like the app folder
/// itself, so pointing `LOCALAPPDATA` elsewhere (a test) never reaches the real data.
fn legacy_profile_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    return std::env::var_os("LOCALAPPDATA")
        .filter(|d| !d.is_empty())
        .map(|d| {
            PathBuf::from(d)
                .join("akulav")
                .join("AuraPortfolio")
                .join("data")
        });
    #[cfg(not(windows))]
    return directories::ProjectDirs::from("com", "akulav", "AuraPortfolio")
        .map(|d| d.data_local_dir().to_path_buf());
}

fn is_empty_dir(p: &Path) -> bool {
    std::fs::read_dir(p)
        .map(|mut d| d.next().is_none())
        .unwrap_or(true)
}

/// Moves `old` (pre-1.0 data) into `new` once: only when `old` has content and `new` is missing or empty. Each entry
/// is renamed (same drive) or copied and then removed; the emptied old folders are deleted.
pub fn move_legacy_data(old: &Path, new: &Path) {
    if !old.is_dir() || is_empty_dir(old) || (new.exists() && !is_empty_dir(new)) {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(new) {
        tracing::warn!("couldn't create {}: {e}", new.display());
        return;
    }
    let Ok(entries) = std::fs::read_dir(old) else {
        return;
    };
    for entry in entries.flatten() {
        let (from, to) = (entry.path(), new.join(entry.file_name()));
        if std::fs::rename(&from, &to).is_err() {
            match copy_recursive(&from, &to) {
                Ok(()) => {
                    let _ = if from.is_dir() {
                        std::fs::remove_dir_all(&from)
                    } else {
                        std::fs::remove_file(&from)
                    };
                }
                Err(e) => tracing::warn!("couldn't move {}: {e}", from.display()),
            }
        }
    }
    tracing::info!(
        "moved the data from {} into {}",
        old.display(),
        new.display()
    );
    // (…\akulav\AuraPortfolio\data, then its now-empty parents)
    let mut dir = Some(old.to_path_buf());
    for _ in 0..3 {
        match dir {
            Some(d) if is_empty_dir(&d) => {
                let _ = std::fs::remove_dir(&d);
                dir = d.parent().map(Path::to_path_buf);
            }
            _ => break,
        }
    }
}

fn copy_recursive(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)?.flatten() {
            copy_recursive(&e.path(), &to.join(e.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aura-paths-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn legacy_data_moves_once_and_never_over_existing_data() {
        let base = scratch("move");
        let old = base.join("akulav").join("AuraPortfolio").join("data");
        std::fs::create_dir_all(old.join("models")).unwrap();
        std::fs::write(old.join("settings.json"), b"{}").unwrap();
        std::fs::write(old.join("models").join("m.gguf"), b"weights").unwrap();
        let new = base.join("Aura Portfolio").join("data");

        move_legacy_data(&old, &new);
        assert_eq!(
            std::fs::read(new.join("models").join("m.gguf")).unwrap(),
            b"weights"
        );
        assert!(new.join("settings.json").exists());
        assert!(
            !base.join("akulav").exists(),
            "emptied old folders are removed"
        );

        // A second copy of old data never overwrites the new folder.
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("settings.json"), b"stale").unwrap();
        move_legacy_data(&old, &new);
        assert_eq!(std::fs::read(new.join("settings.json")).unwrap(), b"{}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn data_lives_in_the_app_folder_unless_legacy_portable() {
        let app = scratch("layout");
        let p = AppPaths::for_app(&app, false).unwrap();
        assert_eq!(p.root, app.join("data"));
        assert!(!p.portable);
        std::fs::create_dir_all(app.join("aura-data")).unwrap();
        let p = AppPaths::for_app(&app, false).unwrap();
        assert_eq!(p.root, app.join("aura-data"));
        assert!(p.portable);
        let _ = std::fs::remove_dir_all(&app);
    }
}
