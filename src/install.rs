//! Installing, updating in place and uninstalling. **One file per system does both install and update**, and the
//! folder you choose is the only folder Aura writes to:
//!
//! | System | Release file | Installs to |
//! |---|---|---|
//! | Windows | `AuraPortfolio-setup.exe` (Inno Setup, `installer/aura-portfolio.iss`) | the folder picked in the wizard (default `%LOCALAPPDATA%\Aura Portfolio`); later setups and in-app updates reuse it |
//! | Linux | `AuraPortfolio-linux-x64` | a folder you pick (default `~/.local/share/aura-portfolio`) + app-menu entry and desktop launcher |
//! | macOS | `AuraPortfolio-macos-arm64` | `Aura Portfolio.app` inside a folder you pick (default `~/Applications/Aura Portfolio`) |
//!
//! On Linux/macOS the program, started from anywhere else (Downloads, the desktop), shows a small window that asks
//! where to install ([`Offer::Install`]), or offers to update an older install ([`Offer::Update`]). A newer or equal
//! install is simply started ([`Offer::OpenInstalled`]). It runs where it is when its folder already is an app folder
//! (`.aura-app`, `.git`, the setup's `unins000.exe`, a pre-1.0 portable folder), with `--here`, `--dir` or
//! `AURA_DATA_DIR`, after an update, without a desktop (Linux), and when it was built here (`cargo`, under `target/`).
//!
//! Everything Aura keeps (settings, cache, models, the llama.cpp runtime, logs, a downloaded update) is in the app
//! folder's `data/` (see [`crate::paths`]). Outside it there are only the system's own entries for an installed app:
//! shortcuts and the Apps entry (Windows), the app-menu entry and desktop launcher (Linux), and the Trading212 key in
//! the OS keychain.

use std::fs;
use std::path::{Path, PathBuf};

use crate::updater;

/// Product name (window title, shortcuts, the Apps entry).
pub const NAME: &str = "Aura Portfolio";
/// Marker in every app folder (written by the setup and by a Linux/macOS install).
pub const MARKER: &str = ".aura-app";
#[cfg(all(unix, not(target_os = "macos")))]
const ICON_PNG: &[u8] = include_bytes!("../assets/icon-256.png");
#[cfg(target_os = "macos")]
const ICON_ICNS: &[u8] = include_bytes!("../assets/icon.icns");
#[cfg(target_os = "macos")]
const INFO_PLIST: &str = include_str!("../installer/Info.plist");
/// The Windows setup's uninstaller (Inno Setup), in the folder it installed.
#[cfg(windows)]
const UNINSTALLER: &str = "unins000.exe";

/// The program's file name.
pub fn exe_name() -> &'static str {
    if cfg!(windows) {
        "aura-portfolio.exe"
    } else {
        "aura-portfolio"
    }
}

#[cfg(unix)]
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// The folder suggested for a first install.
pub fn default_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    return std::env::var_os("LOCALAPPDATA")
        .filter(|d| !d.is_empty())
        .map(|d| PathBuf::from(d).join(NAME));
    #[cfg(target_os = "macos")]
    return home().map(|h| h.join("Applications").join(NAME));
    #[cfg(all(unix, not(target_os = "macos")))]
    return xdg_data_home().map(|d| d.join("aura-portfolio"));
}

#[cfg(all(unix, not(target_os = "macos")))]
fn xdg_data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".local").join("share")))
}

/// The app bundle a program runs inside (`X.app/Contents/MacOS/<program>` → `X.app`).
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension().is_some_and(|e| e == "app"))
    .then(|| app.to_path_buf())
}

/// The app folder of a program: its own folder, or for a program inside `X.app` the folder that holds `X.app`.
pub fn root_of_exe(exe: &Path) -> Option<PathBuf> {
    match bundle_of(exe) {
        Some(app) => app.parent().map(Path::to_path_buf),
        None => exe.parent().map(Path::to_path_buf),
    }
}

/// The app bundle inside an app folder (macOS installs).
pub fn bundle_in(root: &Path) -> PathBuf {
    root.join(format!("{NAME}.app"))
}

/// The program inside an app folder: in the app bundle on macOS (or loose, as 1.0 installed it), else beside `data/`.
pub fn installed_program(root: &Path) -> PathBuf {
    let in_bundle = bundle_in(root)
        .join("Contents")
        .join("MacOS")
        .join(exe_name());
    if cfg!(target_os = "macos") && (in_bundle.exists() || !root.join(exe_name()).exists()) {
        in_bundle
    } else {
        root.join(exe_name())
    }
}

/// A folder the app already lives in (an install of any version, the setup's, a pre-1.0 portable folder, or a
/// developer's checkout).
pub fn is_app_folder(dir: &Path) -> bool {
    [MARKER, ".git", "unins000.exe", "portable", "aura-data"]
        .iter()
        .any(|n| dir.join(n).exists())
}

#[cfg(unix)]
fn same_place(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// What to do when the program starts outside an app folder (Linux/macOS).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// Run where it is.
    None,
    /// Not installed yet: ask for a folder (this one is suggested).
    Install {
        /// The suggested folder.
        suggested: PathBuf,
    },
    /// An older copy is installed in `root`: offer to update it.
    Update {
        /// The installed app folder.
        root: PathBuf,
        /// Its version, when it could be asked.
        installed: Option<String>,
    },
    /// The same or a newer version is installed in `root`: just start that one.
    OpenInstalled {
        /// The installed app folder.
        root: PathBuf,
    },
}

/// An existing install found on this computer (Linux: from the app-menu entry; macOS: the default folder or 1.0's).
#[cfg(unix)]
fn existing_install() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(entry) =
        xdg_data_home().map(|d| d.join("applications").join("aura-portfolio.desktop"))
        && let Ok(text) = fs::read_to_string(entry)
        && let Some(exec) = text.lines().find_map(|l| l.strip_prefix("Exec="))
    {
        // (undo the quoting of `desktop_entry`: a backslash keeps the next character)
        let mut exec_path = String::new();
        let mut chars = exec.trim().trim_matches('"').chars();
        while let Some(c) = chars.next() {
            exec_path.push(if c == '\\' {
                chars.next().unwrap_or(c)
            } else {
                c
            });
        }
        if let Some(root) = Path::new(&exec_path).parent() {
            candidates.push(root.to_path_buf());
        }
    }
    candidates.extend(default_dir());
    #[cfg(target_os = "macos")]
    candidates.extend(home().map(|h| h.join("Library").join("Application Support").join(NAME)));
    candidates
        .into_iter()
        .find(|r| is_app_folder(r) && installed_program(r).exists())
}

/// Decides what a start from `exe` (outside an app folder) should do. Windows always runs where it is: the setup
/// does the installing there.
pub fn offer_for(exe: &Path) -> Offer {
    #[cfg(windows)]
    {
        let _ = exe;
        Offer::None
    }
    #[cfg(unix)]
    {
        #[cfg(not(target_os = "macos"))]
        if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return Offer::None; // (no desktop: run where it was put)
        }
        let Some(here) = root_of_exe(exe) else {
            return Offer::None;
        };
        if is_app_folder(&here) {
            return Offer::None;
        }
        match existing_install() {
            Some(root) if same_place(&root, &here) => Offer::None,
            Some(root) => {
                let installed = version_of(&installed_program(&root));
                if installed
                    .as_deref()
                    .is_none_or(|v| updater::newer(crate::VERSION, v))
                {
                    Offer::Update { root, installed }
                } else {
                    Offer::OpenInstalled { root }
                }
            }
            None => default_dir().map_or(Offer::None, |suggested| Offer::Install { suggested }),
        }
    }
}

/// Checks a folder typed into the install window. `~` is the home folder.
pub fn parse_dir(text: &str) -> Result<PathBuf, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Choose a folder.".into());
    }
    #[cfg(unix)]
    let path = match text.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => home()
            .ok_or("Couldn't find your home folder.")?
            .join(rest.trim_start_matches('/')),
        _ => PathBuf::from(text),
    };
    #[cfg(not(unix))]
    let path = PathBuf::from(text);
    if !path.is_absolute() {
        return Err("Use a full path, for example ~/Apps/Aura Portfolio.".into());
    }
    if path.parent().is_none() {
        return Err("Pick a folder, not the root of a drive.".into());
    }
    if path.is_file() {
        return Err("That's a file, not a folder.".into());
    }
    Ok(path)
}

/// Is the program in `dir` running (it holds `data/.aura.lock`)?
pub fn running_in(dir: &Path) -> bool {
    match fs::OpenOptions::new()
        .write(true)
        .open(dir.join("data").join(".aura.lock"))
    {
        Ok(f) => f.try_lock().is_err(),
        Err(_) => false,
    }
}

/// A command that shows no console window on Windows.
fn quiet(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// The version of the program at `exe` (it's asked: `--version` prints "aura-portfolio X.Y.Z").
fn version_of(exe: &Path) -> Option<String> {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--version")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let out = quiet(&mut cmd).output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .nth(1)
        .map(str::to_string)
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A message for the person: a dialog on Windows, the terminal (and a desktop notification on Linux) elsewhere.
pub fn notice(text: &str) {
    tracing::info!("{text}");
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW};
        let (t, c) = (wide(text), wide(NAME));
        // SAFETY: valid NUL-terminated wide strings; no owner window.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                t.as_ptr(),
                c.as_ptr(),
                MB_OK | MB_ICONINFORMATION,
            );
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!("{text}");
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("notify-send")
            .args([NAME, text])
            .spawn();
    }
}

/// Yes or no (a dialog on Windows; elsewhere the terminal's answer). No is the default.
fn ask(text: &str) -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO, MessageBoxW,
        };
        let (t, c) = (wide(text), wide(NAME));
        // SAFETY: valid NUL-terminated wide strings; no owner window.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                t.as_ptr(),
                c.as_ptr(),
                MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
            ) == IDYES
        }
    }
    #[cfg(not(windows))]
    {
        use std::io::Write;
        eprint!("{text} [y/N] ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).is_ok()
            && matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
    }
}

/// Brings the running Aura window to the front (Windows; elsewhere a notice says it's already open).
pub fn show_running() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow,
        };
        let title = wide(NAME);
        // SAFETY: a valid window title; the handle is checked before use.
        unsafe {
            let h = FindWindowW(std::ptr::null(), title.as_ptr());
            if !h.is_null() {
                ShowWindow(h, SW_RESTORE);
                SetForegroundWindow(h);
                return;
            }
        }
    }
    notice(&format!("{NAME} is already open."));
}

/// Copies this program into the app folder `target` (unless the same or a newer one is there), makes the macOS app
/// bundle or the Linux menu entry, and starts the installed program, detached. `Ok(())` = it runs (this one ends).
pub fn install_and_start(exe: &Path, target: &Path, args: &[String]) -> Result<(), String> {
    let this = crate::VERSION;
    fs::create_dir_all(target).map_err(|e| format!("couldn't make {}: {e}", target.display()))?;
    let installed = installed_program(target);
    let first = !installed.exists();
    let same = !first
        && fs::read(&installed)
            .ok()
            .zip(fs::read(exe).ok())
            .is_some_and(|(a, b)| a == b);
    // (the installed one may have updated itself since: an older copy only starts it)
    let replace =
        !first && !same && version_of(&installed).is_none_or(|v| updater::newer(this, &v));
    if running_in(target) {
        if replace {
            notice(&format!(
                "{NAME} is already running from {}.\n\nTo install version {this}, quit it first and start this again.",
                target.display()
            ));
        } else {
            show_running();
        }
        return Ok(());
    }
    if first || replace {
        if let Some(p) = installed.parent() {
            fs::create_dir_all(p).map_err(|e| format!("couldn't make {}: {e}", p.display()))?;
        }
        let new = updater::sibling(&installed, "new");
        fs::copy(exe, &new)
            .map_err(|e| format!("couldn't copy the program to {}: {e}", target.display()))?;
        prepare_program(&new);
        if installed.exists() {
            updater::swap(&installed, &new).map_err(|e| e.to_string())?;
        } else {
            fs::rename(&new, &installed)
                .map_err(|e| format!("couldn't put the program in place: {e}"))?;
        }
        let _ = fs::write(target.join(MARKER), format!("{this}\n"));
        tracing::info!("installed the program ({this}) into {}", target.display());
        if let Err(e) = add_to_system(&installed, target) {
            tracing::warn!("couldn't add {NAME} to the app list: {e}");
        }
    }
    start_installed(target, args)
}

/// Starts the program installed in `target`, detached from this process and its terminal.
pub fn start_installed(target: &Path, args: &[String]) -> Result<(), String> {
    let program = installed_program(target);
    #[cfg(target_os = "macos")]
    let mut cmd = {
        // (through the app bundle, so the Dock shows its icon and name)
        let mut c = std::process::Command::new("/usr/bin/open");
        match bundle_of(&program) {
            Some(app) => {
                c.arg("-n").arg(app);
                if !args.is_empty() {
                    c.arg("--args");
                }
            }
            None => c = std::process::Command::new(&program),
        }
        c
    };
    #[cfg(not(target_os = "macos"))]
    let mut cmd = std::process::Command::new(&program);
    cmd.args(args)
        .current_dir(target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0008 | 0x0000_0200); // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
    }
    #[cfg(unix)]
    // SAFETY: setsid is async-signal-safe; nothing else runs between fork and exec.
    unsafe {
        // (its own session: closing the terminal it was started from doesn't stop it)
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("couldn't start the installed program: {e}"))
}

/// A copied program ready to run: executable on Unix, and on macOS without the download's quarantine mark.
fn prepare_program(p: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o755));
    }
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("xattr")
        .arg("-d")
        .arg("com.apple.quarantine")
        .arg(p)
        .stderr(std::process::Stdio::null())
        .status();
    let _ = p;
}

/// Ad-hoc signs an app bundle (no certificate: macOS asks once, then remembers).
#[cfg(target_os = "macos")]
pub fn sign_bundle(app: &Path) {
    let _ = std::process::Command::new("/usr/bin/codesign")
        .args(["--force", "--deep", "-s", "-"])
        .arg(app)
        .stderr(std::process::Stdio::null())
        .status();
}

// ------------------------------------------------------------------ the app list and the desktop, per system

#[cfg(windows)]
fn add_to_system(_exe: &Path, _dir: &Path) -> Result<(), String> {
    Ok(()) // (the setup makes the shortcuts and the Apps entry)
}

/// 1.0 could install itself without the setup: its Start-menu and desktop shortcuts and its own Apps entry.
#[cfg(windows)]
fn remove_from_system() {
    if let Some(d) = std::env::var_os("APPDATA") {
        let link = PathBuf::from(d)
            .join(r"Microsoft\Windows\Start Menu\Programs")
            .join(format!("{NAME}.lnk"));
        let _ = fs::remove_file(link);
    }
    if let Some(p) = std::env::var_os("USERPROFILE") {
        let _ = fs::remove_file(PathBuf::from(p).join("Desktop").join(format!("{NAME}.lnk")));
    }
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Aura Portfolio");
    // SAFETY: deleting our own key; a missing key is fine.
    unsafe {
        windows_sys::Win32::System::Registry::RegDeleteTreeW(
            windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
            key.as_ptr(),
        );
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn shortcut_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(d) = xdg_data_home() {
        out.push(d.join("applications").join("aura-portfolio.desktop"));
    }
    // the desktop folder: where xdg-user-dir says (it's localised), else ~/Desktop
    let desktop = std::process::Command::new("xdg-user-dir")
        .arg("DESKTOP")
        .output()
        .ok()
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
        .filter(|p| p.is_dir() && Some(p.as_path()) != home().as_deref())
        .or_else(|| home().map(|h| h.join("Desktop")).filter(|p| p.is_dir()));
    if let Some(d) = desktop {
        out.push(d.join("aura-portfolio.desktop"));
    }
    out
}

/// The `.desktop` entry for the program at `exe` in the app folder `dir` (icon kept in the app folder).
#[cfg(all(unix, not(target_os = "macos")))]
fn desktop_entry(exe: &Path, dir: &Path) -> String {
    let q = |p: &Path| {
        let mut out = String::from("\"");
        for c in p.to_string_lossy().chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('"');
        out
    };
    format!(
        "[Desktop Entry]\nType=Application\nName={NAME}\nGenericName=Portfolio dashboard\nComment=Trading212 \
         portfolio analytics with a local AI analyst - made by Akulav\nExec={}\nPath={}\nIcon={}\nTerminal=false\n\
         Categories=Office;Finance;\nStartupWMClass=aura-portfolio\n",
        q(exe),
        dir.display(),
        dir.join("aura-portfolio.png").display()
    )
}

#[cfg(all(unix, not(target_os = "macos")))]
fn add_to_system(exe: &Path, dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::write(dir.join("aura-portfolio.png"), ICON_PNG).map_err(|e| e.to_string())?;
    let entry = desktop_entry(exe, dir);
    for path in shortcut_paths() {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        fs::write(&path, &entry).map_err(|e| e.to_string())?;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o755));
        // (GNOME starts a desktop launcher only once it's trusted)
        let _ = std::process::Command::new("gio")
            .arg("set")
            .arg(&path)
            .args(["metadata::trusted", "true"])
            .stderr(std::process::Stdio::null())
            .status();
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn remove_from_system() {
    for p in shortcut_paths() {
        let _ = fs::remove_file(p);
    }
}

/// The app bundle around the program already copied into `<folder>/Aura Portfolio.app/Contents/MacOS`.
#[cfg(target_os = "macos")]
fn add_to_system(exe: &Path, _dir: &Path) -> Result<(), String> {
    let Some(app) = bundle_of(exe) else {
        return Ok(()); // (a 1.0 install keeps its loose program)
    };
    let contents = app.join("Contents");
    fs::create_dir_all(contents.join("Resources")).map_err(|e| e.to_string())?;
    fs::write(contents.join("Resources").join("icon.icns"), ICON_ICNS)
        .map_err(|e| e.to_string())?;
    let plist = INFO_PLIST.replace("@VERSION@", crate::VERSION);
    fs::write(contents.join("Info.plist"), plist).map_err(|e| e.to_string())?;
    sign_bundle(&app);
    Ok(())
}

/// 1.0 made a small launcher app in `~/Applications` and a desktop link.
#[cfg(target_os = "macos")]
fn remove_from_system() {
    if let Some(h) = home() {
        let _ = fs::remove_dir_all(h.join("Applications").join(format!("{NAME}.app")));
        let _ = fs::remove_file(h.join("Desktop").join(format!("{NAME}.app")));
    }
}

/// `aura-portfolio --uninstall`: the setup's uninstaller when the setup installed it; otherwise the program and its
/// menu entries go, and the folder with your data only when you say so.
pub fn uninstall(dir: &Path) {
    #[cfg(windows)]
    if dir.join(UNINSTALLER).exists() {
        let _ = std::process::Command::new(dir.join(UNINSTALLER)).spawn();
        return;
    }
    if running_in(dir) {
        notice(&format!(
            "{NAME} is running. Quit it first, then uninstall again."
        ));
        return;
    }
    remove_from_system();
    #[cfg(target_os = "macos")]
    let _ = fs::remove_dir_all(bundle_in(dir));
    let _ = fs::remove_file(dir.join(MARKER));
    tracing::info!("uninstalled: removed the program's menu entries");
    let wipe = ask(&format!(
        "{NAME} is uninstalled.\n\nAlso delete its folder {}?\nThat deletes your settings, the cached portfolio \
         history, insights, the downloaded AI models and the llama.cpp runtime for good.\n(Your Trading212 API key is \
         in the system keychain, not in this folder.)\n\nNo keeps the folder: installing into it again brings \
         everything back.",
        dir.display()
    ));
    if !wipe {
        return;
    }
    // (a running program can't delete itself on Windows: a separate command does it a moment after this one ends)
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("cmd")
            .args(["/C", "ping -n 4 127.0.0.1 >NUL & rmdir /S /Q"])
            .arg(dir)
            .creation_flags(0x0800_0000)
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = fs::remove_dir_all(dir);
        eprintln!("Deleted {}", dir.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_app_folder_is_recognised_by_its_markers() {
        let dir = std::env::temp_dir().join(format!("aura-setup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert!(!is_app_folder(&dir));
        for marker in [MARKER, "unins000.exe", "portable"] {
            fs::write(dir.join(marker), b"x").unwrap();
            assert!(is_app_folder(&dir), "{marker}");
            fs::remove_file(dir.join(marker)).unwrap();
        }
        assert!(!running_in(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn windows_never_installs_itself() {
        if cfg!(windows) {
            assert_eq!(
                offer_for(Path::new(r"C:\Users\me\Downloads\aura-portfolio.exe")),
                Offer::None
            );
        }
    }

    #[test]
    fn a_held_lock_means_running() {
        let dir = std::env::temp_dir().join(format!("aura-lock-{}", std::process::id()));
        fs::create_dir_all(dir.join("data")).unwrap();
        let f = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("data").join(".aura.lock"))
            .unwrap();
        f.try_lock().unwrap();
        assert!(running_in(&dir));
        drop(f);
        assert!(!running_in(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_app_folder_of_a_bundled_program_holds_the_bundle() {
        let p = |s: &str| PathBuf::from(s);
        let exe = p(
            "/Users/me/Applications/Aura Portfolio/Aura Portfolio.app/Contents/MacOS/aura-portfolio",
        );
        assert_eq!(
            bundle_of(&exe),
            Some(p(
                "/Users/me/Applications/Aura Portfolio/Aura Portfolio.app"
            ))
        );
        assert_eq!(
            root_of_exe(&exe),
            Some(p("/Users/me/Applications/Aura Portfolio"))
        );
        assert_eq!(
            bundle_of(&p("/x/MacOS/aura-portfolio")),
            None,
            "not a bundle"
        );
        assert_eq!(
            root_of_exe(&p("/opt/aura/aura-portfolio")),
            Some(p("/opt/aura"))
        );
    }

    #[test]
    fn typed_folders_are_checked() {
        assert!(parse_dir("").is_err());
        assert!(parse_dir("relative/dir").is_err());
        if cfg!(unix) {
            assert!(parse_dir("/").is_err());
            assert!(parse_dir("~/Apps/Aura").unwrap().ends_with("Apps/Aura"));
        } else {
            assert!(parse_dir(r"C:\").is_err());
            assert_eq!(
                parse_dir(r" D:\Apps\Aura ").unwrap(),
                PathBuf::from(r"D:\Apps\Aura")
            );
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn desktop_entry_quotes_exec() {
        let e = desktop_entry(
            Path::new("/home/a b/$x/aura-portfolio"),
            Path::new("/home/a b/$x"),
        );
        assert!(e.contains(r#"Exec="/home/a b/\$x/aura-portfolio""#), "{e}");
    }
}
