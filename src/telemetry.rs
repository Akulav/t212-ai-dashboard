//! Real-time resource telemetry for the local LLM.
//!
//! A background thread samples once per second:
//!
//! * system RAM used / total,
//! * resident memory of the `llama-server` process,
//! * GPU VRAM used / total and utilisation (NVIDIA via NVML),
//! * VRAM attributed to `llama-server` — exact when NVML exposes per-process
//!   accounting (Linux, TCC), otherwise *estimated* as the device-level delta
//!   since the server started (Windows WDDM).
//!
//! The UI reads [`Telemetry::snapshot`]; history powers the sparklines.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;
use sysinfo::{Pid, ProcessesToUpdate, System};

/// Number of samples kept for sparklines (one per second).
pub const HISTORY_LEN: usize = 90;

/// GPU readings.
#[derive(Debug, Clone, Default)]
pub struct GpuSample {
    /// GPU name.
    pub name: String,
    /// Device-wide VRAM in use, bytes.
    pub vram_used: u64,
    /// Total VRAM, bytes.
    pub vram_total: u64,
    /// Core utilisation percentage.
    pub utilization: Option<u32>,
}

/// One point of history.
#[derive(Debug, Clone, Copy, Default)]
pub struct Sample {
    /// LLM process RAM, bytes.
    pub llm_ram: u64,
    /// LLM VRAM (exact or estimated), bytes.
    pub llm_vram: u64,
}

/// Latest telemetry plus history.
#[derive(Debug, Clone, Default)]
pub struct TelemetrySnapshot {
    /// System RAM in use, bytes.
    pub sys_ram_used: u64,
    /// System RAM total, bytes.
    pub sys_ram_total: u64,
    /// `llama-server` resident memory, if running.
    pub llm_ram: Option<u64>,
    /// VRAM used by `llama-server`, if running and measurable.
    pub llm_vram: Option<u64>,
    /// `true` when [`Self::llm_vram`] is a delta estimate, not exact.
    pub llm_vram_estimated: bool,
    /// GPU readings, if an NVIDIA GPU is present.
    pub gpu: Option<GpuSample>,
    /// Rolling history, oldest first.
    pub history: VecDeque<Sample>,
}

/// Handle to the sampling thread. Dropping it stops the thread.
pub struct Telemetry {
    shared: Arc<Mutex<TelemetrySnapshot>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Telemetry {
    /// Starts sampling. `pid` is the cell published by
    /// [`LlmManager::pid_handle`](crate::llm::LlmManager::pid_handle).
    pub fn start(pid: Arc<AtomicU32>) -> Self {
        let shared = Arc::new(Mutex::new(TelemetrySnapshot::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let shared = shared.clone();
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("aura-telemetry".into())
                .spawn(move || sampler(shared, stop, pid))
                .ok()
        };
        Self {
            shared,
            stop,
            thread,
        }
    }

    /// Latest readings.
    pub fn snapshot(&self) -> TelemetrySnapshot {
        self.shared.lock().clone()
    }
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn sampler(shared: Arc<Mutex<TelemetrySnapshot>>, stop: Arc<AtomicBool>, pid: Arc<AtomicU32>) {
    let mut sys = System::new();
    let gpu = GpuProbe::new();
    let mut baseline: Option<(u32, u64)> = None; // (pid, device VRAM before load)

    while !stop.load(Ordering::Relaxed) {
        sys.refresh_memory();
        let current_pid = pid.load(Ordering::SeqCst);

        let llm_ram = (current_pid != 0)
            .then(|| {
                let p = Pid::from_u32(current_pid);
                sys.refresh_processes(ProcessesToUpdate::Some(&[p]), true);
                sys.process(p).map(|proc| proc.memory())
            })
            .flatten();

        let gpu_sample = gpu.sample();
        let (mut llm_vram, mut estimated) = (None, false);
        if current_pid != 0 {
            if let Some(exact) = gpu.process_vram(current_pid) {
                llm_vram = Some(exact);
            } else if let Some(g) = &gpu_sample {
                let base = match baseline {
                    Some((bp, b)) if bp == current_pid => b,
                    _ => {
                        baseline = Some((current_pid, g.vram_used));
                        g.vram_used
                    }
                };
                llm_vram = Some(g.vram_used.saturating_sub(base));
                estimated = true;
            }
        } else {
            baseline = None;
        }

        {
            let mut snap = shared.lock();
            snap.sys_ram_used = sys.used_memory();
            snap.sys_ram_total = sys.total_memory();
            snap.llm_ram = llm_ram;
            snap.llm_vram = llm_vram;
            snap.llm_vram_estimated = estimated;
            snap.gpu = gpu_sample;
            snap.history.push_back(Sample {
                llm_ram: llm_ram.unwrap_or(0),
                llm_vram: llm_vram.unwrap_or(0),
            });
            while snap.history.len() > HISTORY_LEN {
                snap.history.pop_front();
            }
        }

        // Sleep in small slices so shutdown is responsive.
        for _ in 0..10 {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(feature = "nvidia")]
struct GpuProbe {
    nvml: Option<nvml_wrapper::Nvml>,
}

#[cfg(feature = "nvidia")]
impl GpuProbe {
    fn new() -> Self {
        Self {
            nvml: nvml_wrapper::Nvml::init().ok(),
        }
    }

    fn sample(&self) -> Option<GpuSample> {
        let device = self.nvml.as_ref()?.device_by_index(0).ok()?;
        let mem = device.memory_info().ok()?;
        Some(GpuSample {
            name: device.name().unwrap_or_default(),
            vram_used: mem.used,
            vram_total: mem.total,
            utilization: device.utilization_rates().ok().map(|u| u.gpu),
        })
    }

    fn process_vram(&self, pid: u32) -> Option<u64> {
        use nvml_wrapper::enums::device::UsedGpuMemory;
        let device = self.nvml.as_ref()?.device_by_index(0).ok()?;
        device
            .running_compute_processes()
            .ok()?
            .into_iter()
            .find(|p| p.pid == pid)
            .and_then(|p| match p.used_gpu_memory {
                UsedGpuMemory::Used(bytes) => Some(bytes),
                UsedGpuMemory::Unavailable => None,
            })
    }
}

#[cfg(not(feature = "nvidia"))]
struct GpuProbe;

#[cfg(not(feature = "nvidia"))]
impl GpuProbe {
    fn new() -> Self {
        Self
    }
    fn sample(&self) -> Option<GpuSample> {
        None
    }
    fn process_vram(&self, _pid: u32) -> Option<u64> {
        None
    }
}
