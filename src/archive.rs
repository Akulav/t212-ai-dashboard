//! Safe archive extraction for `.zip` and `.tar.gz` files.
//!
//! Both extractors reject entries that would escape the destination directory
//! (zip-slip / path traversal). These functions block; call them from
//! `tokio::task::spawn_blocking`.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Extracts `archive` into `dest`, choosing the format from the file name.
pub fn extract(archive: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    let name = archive.to_string_lossy().to_lowercase();
    if name.ends_with(".zip") {
        extract_zip(archive, dest)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar_gz(archive, dest)
    } else {
        bail!("unsupported archive format: {}", archive.display())
    }
}

fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    let file = File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file).context("invalid zip archive")?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else {
            tracing::warn!("skipping unsafe zip entry {:?}", entry.name());
            continue;
        };
        let out = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut writer =
            File::create(&out).with_context(|| format!("creating {}", out.display()))?;
        io::copy(&mut entry, &mut writer)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode))?;
        }
    }
    Ok(())
}

fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<()> {
    let file = File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    tar.set_preserve_permissions(true);
    for entry in tar.entries()? {
        // `unpack_in` refuses paths that escape `dest` and handles symlinks.
        entry?.unpack_in(dest)?;
    }
    Ok(())
}

/// Recursively searches `root` for a file named exactly `file_name`.
pub fn find_file(root: &Path, file_name: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == file_name) {
                return Some(path);
            }
        }
    }
    None
}
