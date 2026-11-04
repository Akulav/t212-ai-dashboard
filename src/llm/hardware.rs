//! Hardware detection used to pick a llama.cpp backend and recommend models.
//!
//! * System RAM / CPU via [`sysinfo`].
//! * NVIDIA GPUs + dedicated VRAM via NVML (`nvidia` feature; the NVML library
//!   ships with the driver and is loaded at runtime, so its absence is fine).
//! * Apple Silicon is modelled as a GPU with *unified* memory.
//! * Vulkan availability is probed by looking for the system loader library.

use serde::{Deserialize, Serialize};
use sysinfo::{CpuRefreshKind, System};

/// GPU vendor, as far as we can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuVendor {
    /// NVIDIA (CUDA capable).
    Nvidia,
    /// Apple Silicon integrated GPU (Metal).
    Apple,
    /// Something else (AMD / Intel), typically reached via Vulkan.
    Other,
}

/// One detected GPU.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    /// Marketing name, e.g. "NVIDIA GeForce RTX 4070".
    pub name: String,
    /// Vendor.
    pub vendor: GpuVendor,
    /// Memory usable by the GPU, in bytes.
    pub vram_total: u64,
    /// `true` when VRAM is shared with system RAM (Apple Silicon).
    pub unified: bool,
}

/// Snapshot of the machine's AI-relevant capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareProfile {
    /// CPU brand string.
    pub cpu_name: String,
    /// Logical CPU threads.
    pub cpu_threads: usize,
    /// Total system RAM in bytes.
    pub ram_total: u64,
    /// Currently available system RAM in bytes.
    pub ram_available: u64,
    /// Detected GPUs (may be empty).
    pub gpus: Vec<GpuInfo>,
    /// Whether a Vulkan loader is installed (enables the Vulkan backend).
    pub vulkan_available: bool,
}

impl HardwareProfile {
    /// The GPU with the most memory, if any.
    pub fn best_gpu(&self) -> Option<&GpuInfo> {
        self.gpus.iter().max_by_key(|g| g.vram_total)
    }

    /// `true` if any NVIDIA GPU was detected.
    pub fn has_nvidia(&self) -> bool {
        self.gpus.iter().any(|g| g.vendor == GpuVendor::Nvidia)
    }

    /// Bytes of memory a model can realistically occupy on the GPU.
    ///
    /// For unified memory we leave ~1/3 for the OS and the app itself.
    pub fn gpu_budget(&self) -> u64 {
        match self.best_gpu() {
            Some(g) if g.unified => g.vram_total / 3 * 2,
            Some(g) => g.vram_total,
            None => 0,
        }
    }
}

/// Probes the current machine. Takes a few milliseconds; call once at startup
/// (or when the user presses "Re-scan").
pub fn detect() -> HardwareProfile {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_list(CpuRefreshKind::nothing());

    let cpu_name = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown CPU".into());
    let cpu_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    let mut gpus = detect_nvidia();
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        gpus.push(GpuInfo {
            name: "Apple Silicon GPU".into(),
            vendor: GpuVendor::Apple,
            vram_total: sys.total_memory(),
            unified: true,
        });
    }

    HardwareProfile {
        cpu_name,
        cpu_threads,
        ram_total: sys.total_memory(),
        ram_available: sys.available_memory(),
        gpus,
        vulkan_available: vulkan_available(),
    }
}

#[cfg(feature = "nvidia")]
fn detect_nvidia() -> Vec<GpuInfo> {
    let Ok(nvml) = nvml_wrapper::Nvml::init() else {
        return Vec::new();
    };
    let count = nvml.device_count().unwrap_or(0);
    (0..count)
        .filter_map(|i| nvml.device_by_index(i).ok())
        .filter_map(|d| {
            Some(GpuInfo {
                name: d.name().unwrap_or_else(|_| "NVIDIA GPU".into()),
                vendor: GpuVendor::Nvidia,
                vram_total: d.memory_info().ok()?.total,
                unified: false,
            })
        })
        .collect()
}

#[cfg(not(feature = "nvidia"))]
fn detect_nvidia() -> Vec<GpuInfo> {
    Vec::new()
}

fn vulkan_available() -> bool {
    #[cfg(windows)]
    {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        std::path::Path::new(&root)
            .join("System32")
            .join("vulkan-1.dll")
            .exists()
    }
    #[cfg(target_os = "linux")]
    {
        [
            "/usr/lib/x86_64-linux-gnu/libvulkan.so.1",
            "/usr/lib64/libvulkan.so.1",
            "/usr/lib/libvulkan.so.1",
            "/usr/lib/aarch64-linux-gnu/libvulkan.so.1",
        ]
        .iter()
        .any(|p| std::path::Path::new(p).exists())
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        false
    }
}
