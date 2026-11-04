//! `llama-server` child-process lifecycle.
//!
//! The hard requirement is that **closing Aura always kills the model server
//! and frees its RAM/VRAM**, including on crashes. We layer several guarantees:
//!
//! 1. [`ServerProcess`] kills the child in `Drop` and in [`ServerProcess::kill`].
//! 2. `LlmManager::shutdown` is called from `eframe::App::on_exit`, after the
//!    event loop returns, and from a panic hook.
//! 3. **OS-level binding** so even `kill -9` / Task Manager on Aura reaps it:
//!    * Windows: the child is placed in a Job Object with
//!      `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`; the kernel kills it when our
//!      process (holder of the job handle) exits for any reason.
//!    * Linux: `prctl(PR_SET_PDEATHSIG, SIGKILL)` in the child.
//!    * macOS (no equivalent): the PID is written to a file and any orphan
//!      is reaped by [`reap_orphan`] on next launch.

use std::fs::File;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result};

/// Everything needed to launch a server.
#[derive(Debug, Clone)]
pub struct LaunchConfig {
    /// Path to `llama-server`.
    pub exe: PathBuf,
    /// Path to the `.gguf` model.
    pub model: PathBuf,
    /// Context size in tokens (`-c`).
    pub context: u32,
    /// Layers to offload to the GPU (`-ngl`).
    pub gpu_layers: u32,
    /// Server stdout/stderr destination.
    pub log_file: PathBuf,
    /// PID file used for orphan recovery.
    pub pid_file: PathBuf,
}

/// A running `llama-server`. Dropping it kills the process.
pub struct ServerProcess {
    child: Child,
    port: u16,
    pid_file: PathBuf,
    #[cfg(windows)]
    _job: windows_job::JobGuard,
}

impl std::fmt::Debug for ServerProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerProcess")
            .field("pid", &self.pid())
            .field("port", &self.port)
            .finish()
    }
}

impl ServerProcess {
    /// Spawns the server on a free loopback port.
    ///
    /// Call this from a long-lived thread (the UI thread is ideal): on Linux
    /// the parent-death signal fires when the *spawning thread* exits.
    pub fn spawn(cfg: &LaunchConfig) -> Result<Self> {
        let port = free_port()?;
        let log = File::create(&cfg.log_file)
            .with_context(|| format!("creating {}", cfg.log_file.display()))?;

        let mut cmd = Command::new(&cfg.exe);
        cmd.arg("--model")
            .arg(&cfg.model)
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .args(["--ctx-size", &cfg.context.to_string()])
            .args(["--n-gpu-layers", &cfg.gpu_layers.to_string()])
            .args(["--alias", "aura"])
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        if let Some(dir) = cfg.exe.parent() {
            cmd.current_dir(dir); // shared libraries live next to the exe
        }
        platform_configure(&mut cmd);

        let child = cmd
            .spawn()
            .with_context(|| format!("launching {}", cfg.exe.display()))?;
        #[cfg(windows)]
        let job =
            windows_job::JobGuard::bind(&child).context("binding llama-server to a job object")?;

        let _ = std::fs::write(&cfg.pid_file, child.id().to_string());
        tracing::info!(pid = child.id(), port, "llama-server started");
        Ok(Self {
            child,
            port,
            pid_file: cfg.pid_file.clone(),
            #[cfg(windows)]
            _job: job,
        })
    }

    /// OS process id.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Loopback port the HTTP API listens on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// `Some(description)` if the process has exited.
    pub fn exit_status(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.to_string()),
            Ok(None) => None,
            Err(e) => Some(e.to_string()),
        }
    }

    /// Forcefully terminates the server and waits for it to be reaped, which
    /// is what actually releases its RAM and VRAM.
    pub fn kill(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
            tracing::info!(pid = self.child.id(), "llama-server stopped");
        }
        let _ = std::fs::remove_file(&self.pid_file);
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Kills a `llama-server` left running by a previous crashed session.
pub fn reap_orphan(pid_file: &Path) {
    let Some(pid) = std::fs::read_to_string(pid_file)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
    else {
        return;
    };
    let mut sys = sysinfo::System::new();
    let pid = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    if let Some(process) = sys.process(pid)
        && process.name().to_string_lossy().contains("llama-server")
    {
        tracing::warn!(%pid, "reaping orphaned llama-server from a previous session");
        process.kill();
    }
    let _ = std::fs::remove_file(pid_file);
}

/// Last `max_lines` lines of the server log, for error messages.
pub fn log_tail(log_file: &Path, max_lines: usize) -> String {
    let text = std::fs::read_to_string(log_file).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(max_lines)..].join("\n")
}

fn free_port() -> Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).context("no free loopback port")?;
    Ok(listener.local_addr()?.port())
}

#[cfg(windows)]
fn platform_configure(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(target_os = "linux")]
fn platform_configure(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: only async-signal-safe libc calls happen between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn platform_configure(_cmd: &mut Command) {}

#[cfg(windows)]
mod windows_job {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    /// Owns a Job Object handle; when the last handle closes (our process
    /// exits or this guard drops) Windows kills every process in the job.
    pub struct JobGuard(HANDLE);

    // SAFETY: a job handle is a kernel object reference usable from any thread.
    unsafe impl Send for JobGuard {}
    unsafe impl Sync for JobGuard {}

    impl JobGuard {
        pub fn bind(child: &Child) -> std::io::Result<Self> {
            // SAFETY: plain Win32 calls with valid arguments; every failure is checked.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return Err(std::io::Error::last_os_error());
                }
                let guard = Self(job);
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) == 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(guard)
            }
        }
    }

    impl Drop for JobGuard {
        fn drop(&mut self) {
            // SAFETY: we own the handle and close it exactly once.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
