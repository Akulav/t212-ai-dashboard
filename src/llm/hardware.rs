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
    /// AMD Radeon (Vulkan).
    Amd,
    /// Intel Arc / Iris (Vulkan).
    Intel,
    /// Unknown vendor.
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
    // AMD / Intel (and NVIDIA when NVML is unavailable), reached via Vulkan.
    let have_nvml = !gpus.is_empty();
    for g in detect_other_gpus() {
        let duplicate = gpus.iter().any(|n| n.name.eq_ignore_ascii_case(&g.name))
            || (have_nvml && g.vendor == GpuVendor::Nvidia);
        if !duplicate {
            gpus.push(g);
        }
    }
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

/// GPUs with less dedicated memory than this are treated as integrated and
/// ignored for offload decisions.
const MIN_DEDICATED_VRAM: u64 = 2 << 30;

/// Best-effort vendor from a marketing name.
pub fn vendor_from_name(name: &str) -> GpuVendor {
    let n = name.to_lowercase();
    if n.contains("nvidia") || n.contains("geforce") || n.contains("rtx") || n.contains("quadro") {
        GpuVendor::Nvidia
    } else if n.contains("amd") || n.contains("radeon") {
        GpuVendor::Amd
    } else if n.contains("intel") || n.contains("arc") {
        GpuVendor::Intel
    } else {
        GpuVendor::Other
    }
}

/// Windows: reads each display adapter's dedicated VRAM from its driver key
/// (`HardwareInformation.qwMemorySize`). Unlike WMI's `AdapterRAM` this is
/// not capped at 4 GB.
#[cfg(windows)]
fn detect_other_gpus() -> Vec<GpuInfo> {
    use windows_sys::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, RRF_RT_ANY, RRF_RT_REG_SZ, RegGetValueW,
    };

    const DISPLAY_CLASS: &str =
        r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// Reads a registry value into `buf`; returns the byte length on success.
    fn read(sub: &str, value: &str, flags: u32, buf: &mut [u8]) -> Option<usize> {
        let (sub, value) = (wide(sub), wide(value));
        let mut len = buf.len() as u32;
        // SAFETY: valid NUL-terminated wide strings and a buffer of `len` bytes.
        let rc = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                sub.as_ptr(),
                value.as_ptr(),
                flags,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut len,
            )
        };
        (rc == 0).then_some(len as usize)
    }

    (0..32)
        .filter_map(|i| {
            let sub = format!(r"{DISPLAY_CLASS}\{i:04}");
            let mut raw = [0u8; 512];
            let n = read(&sub, "DriverDesc", RRF_RT_REG_SZ, &mut raw)?;
            let units: Vec<u16> = raw[..n]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|&u| u != 0)
                .collect();
            let name = String::from_utf16_lossy(&units).trim().to_string();

            let mut mem = [0u8; 8];
            let vram = match read(
                &sub,
                "HardwareInformation.qwMemorySize",
                RRF_RT_ANY,
                &mut mem,
            ) {
                Some(8) => u64::from_le_bytes(mem),
                _ => match read(&sub, "HardwareInformation.MemorySize", RRF_RT_ANY, &mut mem) {
                    Some(8) => u64::from_le_bytes(mem),
                    Some(4) => u32::from_le_bytes([mem[0], mem[1], mem[2], mem[3]]) as u64,
                    _ => 0,
                },
            };
            (vram >= MIN_DEDICATED_VRAM).then(|| GpuInfo {
                vendor: vendor_from_name(&name),
                name,
                vram_total: vram,
                unified: false,
            })
        })
        .collect()
}

/// Linux: amdgpu exposes VRAM in sysfs (`/sys/class/drm/cardN/device`).
#[cfg(target_os = "linux")]
fn detect_other_gpus() -> Vec<GpuInfo> {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.starts_with("card") && n[4..].chars().all(|c| c.is_ascii_digit())
        })
        .filter_map(|e| {
            let dev = e.path().join("device");
            let vram: u64 = std::fs::read_to_string(dev.join("mem_info_vram_total"))
                .ok()?
                .trim()
                .parse()
                .ok()?;
            let vendor = match std::fs::read_to_string(dev.join("vendor")).ok()?.trim() {
                "0x1002" => GpuVendor::Amd,
                "0x8086" => GpuVendor::Intel,
                "0x10de" => GpuVendor::Nvidia,
                _ => GpuVendor::Other,
            };
            let name = std::fs::read_to_string(dev.join("product_name"))
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("{vendor:?} GPU"));
            (vram >= MIN_DEDICATED_VRAM).then_some(GpuInfo {
                name,
                vendor,
                vram_total: vram,
                unified: false,
            })
        })
        .collect()
}

#[cfg(not(any(windows, target_os = "linux")))]
fn detect_other_gpus() -> Vec<GpuInfo> {
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
