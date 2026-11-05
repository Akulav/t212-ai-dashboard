//! Installing. On Windows the usual way is the setup wizard (`installer/aura-portfolio.iss`: `Aura-Portfolio-Setup.exe`,
//! with its uninstaller); on a Mac the .dmg's `Aura Portfolio.app`. Besides that the program installs itself: started
//! from anywhere else (Downloads, the desktop, inside the Mac app) with a desktop session, it copies itself into this
//! computer's app folder, adds itself to the app list and the desktop, and starts from there:
//!
//! | System | App folder | Shortcuts |
//! |---|---|---|
//! | Windows | `%LOCALAPPDATA%\Aura Portfolio` | Start menu + desktop, Settings → Apps entry (Uninstall) |
//! | Linux | `$XDG_DATA_HOME/aura-portfolio` or `~/.local/share/aura-portfolio` | app menu entry + desktop launcher |
//! | macOS | `~/Library/Application Support/Aura Portfolio` | the .dmg's app (or one in `~/Applications`) + desktop |
//!
//! It runs where it is when its folder already is an app folder (`.aura-app`, `.git`, the setup's `unins000.exe`, or a
//! pre-1.0 portable folder: an install of any version, in any folder, keeps working there), with `--here`, `--dir` or
//! `AURA_DATA_DIR`, after an update, without a desktop (Linux), and when it was built here (`cargo`, under `target/`).
//! An installed program that's newer (it updated itself) is never replaced by an older copy. `--uninstall` runs the
//! setup's uninstaller, or takes a self-made install out of the app list and deletes the folder only when you say so.

use std::fs;
use std::path::{Path, PathBuf};

use crate::updater;

/// Product name (window title, shortcuts, the Apps entry).
pub const NAME: &str = "Aura Portfolio";
/// Marker a self-install writes into the app folder.
pub const MARKER: &str = ".aura-app";
#[cfg(all(unix, not(target_os = "macos")))]
const ICON_PNG: &[u8] = include_bytes!("../assets/icon-256.png");
#[cfg(target_os = "macos")]
const ICON_ICNS: &[u8] = include_bytes!("../assets/icon.icns");
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

/// This computer's app folder for this user.
pub fn default_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    return std::env::var_os("LOCALAPPDATA")
        .filter(|d| !d.is_empty())
        .map(|d| PathBuf::from(d).join(NAME));
    #[cfg(target_os = "macos")]
    return home().map(|h| h.join("Library").join("Application Support").join(NAME));
    #[cfg(all(unix, not(target_os = "macos")))]
    return std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".local").join("share")))
        .map(|d| d.join("aura-portfolio"));
}

/// A folder the app already lives in (an install of any version, the setup's, a pre-1.0 portable folder, or a
/// developer's checkout).
pub fn is_app_folder(dir: &Path) -> bool {
    [MARKER, ".git", "unins000.exe", "portable", "aura-data"]
        .iter()
        .any(|n| dir.join(n).exists())
}

fn same_place(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Where to install to, when this start should install; `None` = run where it is.
pub fn target_for(exe_dir: &Path) -> Option<PathBuf> {
    #[cfg(all(unix, not(target_os = "macos")))]
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return None; // (no desktop: run where it was put)
    }
    if is_app_folder(exe_dir) {
        return None;
    }
    let target = default_dir()?;
    (!same_place(&target, exe_dir)).then_some(target)
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

/// Copies this program into `target` (unless the same or a newer one is there), adds it to the app list and the
/// desktop, and starts it there, detached. `Ok(true)` = the installed one runs (this one ends).
pub fn install_and_start(exe: &Path, target: &Path, args: &[String]) -> Result<bool, String> {
    let this = crate::VERSION;
    fs::create_dir_all(target).map_err(|e| format!("couldn't make {}: {e}", target.display()))?;
    let installed = target.join(exe_name());
    let first = !installed.exists();
    let same = !first
        && fs::read(&installed)
            .ok()
            .zip(fs::read(exe).ok())
            .is_some_and(|(a, b)| a == b);
    // (the installed one may have updated itself since: an older copy - the Mac app, an old download - only starts it)
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
        return Ok(true);
    }
    if first || replace {
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
        let _ = fs::write(
            target.join(MARKER),
            format!("{NAME} app folder (self-installed {this})\n"),
        );
        tracing::info!("installed the program ({this}) into {}", target.display());
        if let Err(e) = add_to_system(&installed, target, exe) {
            tracing::warn!("couldn't add {NAME} to the app list: {e}");
        }
    }
    let mut cmd = std::process::Command::new(&installed);
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
        .map_err(|e| format!("couldn't start the installed program: {e}"))?;
    if first && !from_mac_app(exe) {
        notice(&format!(
            "{NAME} is installed in {}\nand starting now. It's on your desktop and in your app list too.\n\n\
             You can delete the file you downloaded.",
            target.display()
        ));
    }
    Ok(true)
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

/// The Mac app (`X.app/Contents/MacOS/<program>`) this program is inside, when it's an installed one (not on the
/// .dmg itself, under `/Volumes`).
fn mac_app(exe: &Path) -> Option<PathBuf> {
    let app = exe.parent()?.parent()?.parent()?;
    let is_app =
        app.extension().is_some_and(|e| e == "app") && exe.parent()?.ends_with("Contents/MacOS");
    (is_app && !app.starts_with("/Volumes")).then(|| app.to_path_buf())
}

fn from_mac_app(exe: &Path) -> bool {
    cfg!(target_os = "macos") && mac_app(exe).is_some()
}

/// (At every start in an app folder.) Keeps the Apps entry at the version an update brought (Windows; only an entry
/// that's there — the setup's or a self-made install's — is touched, none is ever created).
pub fn refresh(dir: &Path) {
    #[cfg(windows)]
    {
        if dir.join(UNINSTALLER).exists() {
            registry::set_version(registry::SETUP_KEY);
        } else if default_dir().is_some_and(|d| same_place(&d, dir)) {
            registry::set_version(registry::KEY);
        }
    }
    let _ = dir;
}

// ------------------------------------------------------------------ the app list and the desktop, per system

#[cfg(windows)]
fn start_menu_link() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| {
        PathBuf::from(d)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join(format!("{NAME}.lnk"))
    })
}

/// Runs a PowerShell line hidden, with values passed through the environment (no quoting to get wrong).
#[cfg(windows)]
fn powershell(script: &str, env: &[(&str, &std::ffi::OsStr)]) -> bool {
    let mut cmd = std::process::Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ])
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    quiet(&mut cmd)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn add_to_system(exe: &Path, dir: &Path, _source: &Path) -> Result<(), String> {
    registry::write(exe, dir)?;
    let link = start_menu_link().ok_or("no APPDATA folder")?;
    if let Some(p) = link.parent() {
        fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    // (a .lnk is a COM object: PowerShell's WScript.Shell writes it - in the Start menu and on the desktop)
    let script = "$w = New-Object -ComObject WScript.Shell; \
                  $desk = Join-Path ([Environment]::GetFolderPath('Desktop')) ($env:AURA_NAME + '.lnk'); \
                  foreach ($l in @($env:AURA_LINK, $desk)) { $s = $w.CreateShortcut($l); \
                  $s.TargetPath = $env:AURA_EXE; $s.WorkingDirectory = $env:AURA_DIR; \
                  $s.IconLocation = $env:AURA_EXE + ',0'; \
                  $s.Description = 'Trading212 portfolio analytics with a local AI analyst'; $s.Save() }";
    let ok = powershell(
        script,
        &[
            ("AURA_NAME", std::ffi::OsStr::new(NAME)),
            ("AURA_LINK", link.as_os_str()),
            ("AURA_EXE", exe.as_os_str()),
            ("AURA_DIR", dir.as_os_str()),
        ],
    );
    if ok && link.exists() {
        Ok(())
    } else {
        Err("the shortcuts weren't made".into())
    }
}

#[cfg(windows)]
fn remove_from_system() {
    if let Some(l) = start_menu_link() {
        let _ = fs::remove_file(l);
    }
    powershell(
        "Remove-Item -LiteralPath (Join-Path ([Environment]::GetFolderPath('Desktop')) ($env:AURA_NAME + '.lnk')) \
         -ErrorAction SilentlyContinue",
        &[("AURA_NAME", std::ffi::OsStr::new(NAME))],
    );
    registry::remove();
}

#[cfg(windows)]
mod registry {
    //! Settings → Apps lists programs from `HKCU\…\Uninstall\<key>`: name, version, icon, folder, how to uninstall.
    use std::path::Path;

    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_DWORD, REG_SZ, RegCloseKey, RegDeleteTreeW,
        RegOpenKeyExW, RegSetKeyValueW, RegSetValueExW,
    };

    const ROOT: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\";
    /// A self-made install's entry (written here; the setup removes it when it takes over the folder).
    pub const KEY: &str = "Aura Portfolio";
    /// The setup's entry: Inno Setup names it after the AppId in installer/aura-portfolio.iss (never change it).
    pub const SETUP_KEY: &str = "{6208BE64-B4D0-4EDB-BF3A-510EE88F9FB5}_is1";

    fn set_sz(key: &[u16], name: &str, value: &str) -> u32 {
        let (n, v) = (super::wide(name), super::wide(value));
        // SAFETY: valid NUL-terminated wide strings; the byte length covers the terminator.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                n.as_ptr(),
                REG_SZ,
                v.as_ptr().cast(),
                (v.len() * 2) as u32,
            )
        }
    }

    pub fn write(exe: &Path, dir: &Path) -> Result<(), String> {
        let exe_s = exe.to_string_lossy();
        let key = super::wide(&format!("{ROOT}{KEY}"));
        for (name, value) in [
            ("DisplayName", super::NAME.to_string()),
            ("DisplayVersion", crate::VERSION.to_string()),
            ("DisplayIcon", format!("{exe_s},0")),
            ("Publisher", "Akulav".to_string()),
            ("InstallLocation", dir.to_string_lossy().to_string()),
            ("UninstallString", format!("\"{exe_s}\" --uninstall")),
            (
                "URLInfoAbout",
                "https://github.com/Akulav/t212-ai-dashboard".to_string(),
            ),
        ] {
            let rc = set_sz(&key, name, &value);
            if rc != 0 {
                return Err(format!("registry error {rc} writing {name}"));
            }
        }
        for name in ["NoModify", "NoRepair"] {
            let (n, one) = (super::wide(name), 1u32);
            // SAFETY: a DWORD value of 4 bytes.
            unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    n.as_ptr(),
                    REG_DWORD,
                    (&one as *const u32).cast(),
                    4,
                );
            }
        }
        Ok(())
    }

    /// Sets DisplayVersion of an entry that exists (never makes one).
    pub fn set_version(which: &str) {
        let key = super::wide(&format!("{ROOT}{which}"));
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: the key is opened, written and closed here; failures leave it untouched.
        unsafe {
            if RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_SET_VALUE, &mut h) != 0 {
                return;
            }
            let (n, v) = (super::wide("DisplayVersion"), super::wide(crate::VERSION));
            RegSetValueExW(
                h,
                n.as_ptr(),
                0,
                REG_SZ,
                v.as_ptr().cast(),
                (v.len() * 2) as u32,
            );
            RegCloseKey(h);
        }
    }

    pub fn remove() {
        let key = super::wide(&format!("{ROOT}{KEY}"));
        // SAFETY: deleting our own key; a missing key is fine.
        unsafe {
            RegDeleteTreeW(HKEY_CURRENT_USER, key.as_ptr());
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn shortcut_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".local").join("share")));
    if let Some(d) = data {
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

#[cfg(all(unix, not(target_os = "macos")))]
fn add_to_system(exe: &Path, dir: &Path, _source: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let icon = dir.join("aura-portfolio.png");
    fs::write(&icon, ICON_PNG).map_err(|e| e.to_string())?;
    let q = |p: &Path| {
        format!(
            "\"{}\"",
            p.to_string_lossy()
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
        )
    };
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName={NAME}\nComment=Trading212 portfolio analytics with a local AI \
         analyst\nExec={}\nPath={}\nIcon={}\nTerminal=false\nCategories=Office;Finance;\n",
        q(exe),
        dir.display(),
        icon.display()
    );
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

#[cfg(target_os = "macos")]
fn wrapper_app() -> Option<PathBuf> {
    home().map(|h| h.join("Applications").join(format!("{NAME}.app")))
}

#[cfg(target_os = "macos")]
fn desktop_link() -> Option<PathBuf> {
    home().map(|h| h.join("Desktop").join(format!("{NAME}.app")))
}

#[cfg(target_os = "macos")]
fn add_to_system(exe: &Path, _dir: &Path, source: &Path) -> Result<(), String> {
    // started from the .dmg's app (copied to /Applications): that's the app to show; else a small one in
    // ~/Applications that starts the installed program (Launchpad, Spotlight, the Dock)
    let app = match mac_app(source) {
        Some(a) => a,
        None => make_wrapper_app(exe)?,
    };
    if let Some(link) = desktop_link() {
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(&app, &link).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn make_wrapper_app(exe: &Path) -> Result<PathBuf, String> {
    let app = wrapper_app().ok_or("no home folder")?;
    let contents = app.join("Contents");
    let err = |e: std::io::Error| e.to_string();
    fs::create_dir_all(contents.join("MacOS")).map_err(err)?;
    fs::create_dir_all(contents.join("Resources")).map_err(err)?;
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>{NAME}</string>
<key>CFBundleDisplayName</key><string>{NAME}</string>
<key>CFBundleIdentifier</key><string>com.akulav.aura-portfolio.launcher</string>
<key>CFBundleVersion</key><string>{v}</string>
<key>CFBundleShortVersionString</key><string>{v}</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleExecutable</key><string>aura-portfolio</string>
<key>CFBundleIconFile</key><string>icon</string>
</dict></plist>
"#,
        v = crate::VERSION
    );
    fs::write(contents.join("Info.plist"), plist).map_err(err)?;
    fs::write(contents.join("Resources").join("icon.icns"), ICON_ICNS).map_err(err)?;
    let launcher = contents.join("MacOS").join("aura-portfolio");
    let quoted = exe.to_string_lossy().replace('\'', r"'\''");
    fs::write(&launcher, format!("#!/bin/sh\nexec '{quoted}' \"$@\"\n")).map_err(err)?;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).map_err(err)?;
    Ok(app)
}

#[cfg(target_os = "macos")]
fn remove_from_system() {
    if let Some(p) = wrapper_app() {
        let _ = fs::remove_dir_all(p);
    }
    if let Some(p) = desktop_link() {
        let _ = fs::remove_file(p);
    }
}

/// `aura-portfolio --uninstall`: the setup's uninstaller when the setup installed it; otherwise out of the app list
/// and off the desktop, and the folder only when the person says so.
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
    tracing::info!("uninstalled: removed from the app list and the desktop");
    let wipe = ask(&format!(
        "{NAME} is removed from your app list and desktop.\n\nAlso delete its folder {}?\nThat deletes your settings, \
         the cached portfolio history, insights, the downloaded AI models and the llama.cpp runtime for good.\n(Your \
         Trading212 API key is in the system keychain, not in this folder.)\n\nNo keeps the folder: starting the \
         program in it again brings everything back.",
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
    fn an_app_folder_runs_where_it_is_and_others_install() {
        let dir = std::env::temp_dir().join(format!("aura-setup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert!(!is_app_folder(&dir));
        let target = target_for(&dir);
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        assert_eq!(
            target,
            default_dir(),
            "a plain folder (Downloads) installs into the app folder"
        );
        let _ = target;
        for marker in [MARKER, "unins000.exe", "portable"] {
            fs::write(dir.join(marker), b"x").unwrap();
            assert!(is_app_folder(&dir));
            assert_eq!(
                target_for(&dir),
                None,
                "an existing install ({marker}) runs where it is"
            );
            fs::remove_file(dir.join(marker)).unwrap();
        }
        assert!(!running_in(&dir));
        let _ = fs::remove_dir_all(&dir);
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
    fn the_mac_app_is_found_from_its_program() {
        assert_eq!(
            mac_app(Path::new(
                "/Applications/Aura Portfolio.app/Contents/MacOS/aura-portfolio"
            )),
            Some(PathBuf::from("/Applications/Aura Portfolio.app"))
        );
        assert_eq!(
            mac_app(Path::new(
                "/Volumes/Aura Portfolio/Aura Portfolio.app/Contents/MacOS/aura-portfolio"
            )),
            None
        );
        assert_eq!(
            mac_app(Path::new("/Users/me/Downloads/aura-portfolio")),
            None
        );
    }
}
