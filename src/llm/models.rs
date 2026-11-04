//! Curated GGUF model catalog, VRAM-aware recommendations, and download paths.
//!
//! Models are fetched from HuggingFace with
//! `https://huggingface.co/<repo>/resolve/main/<file>`. The catalog is plain
//! data: to add a model, append a [`ModelSpec`] to [`CATALOG`] (see
//! `CONTRIBUTING.md`). Sizes are used only for fit estimates.

use std::path::PathBuf;

use super::hardware::HardwareProfile;
use crate::paths::AppPaths;

const GIB: u64 = 1 << 30;

/// A downloadable model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelSpec {
    /// Stable id used in settings, e.g. `qwen2.5-7b-q4km`.
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Parameter count in billions.
    pub params_b: f32,
    /// Quantisation label, e.g. `Q4_K_M`.
    pub quant: &'static str,
    /// HuggingFace repository (`owner/name`).
    pub repo: &'static str,
    /// File name inside the repository.
    pub file: &'static str,
    /// Approximate file size in bytes.
    pub size_bytes: u64,
    /// Number of transformer layers (for partial GPU offload).
    pub layers: u32,
    /// One-line description for the UI.
    pub blurb: &'static str,
}

impl ModelSpec {
    /// Direct download URL.
    pub fn url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/main/{}",
            self.repo, self.file
        )
    }

    /// Estimated memory needed to run with an 8k context (weights + KV cache + scratch).
    pub fn memory_needed(&self) -> u64 {
        self.size_bytes + self.size_bytes / 10 + GIB
    }
}

/// Built-in model catalog, ordered from smallest to largest.
pub const CATALOG: &[ModelSpec] = &[
    ModelSpec {
        id: "qwen2.5-1.5b-q4km",
        name: "Qwen 2.5 1.5B Instruct",
        params_b: 1.5,
        quant: "Q4_K_M",
        repo: "bartowski/Qwen2.5-1.5B-Instruct-GGUF",
        file: "Qwen2.5-1.5B-Instruct-Q4_K_M.gguf",
        size_bytes: 986_000_000,
        layers: 28,
        blurb: "Featherweight. Runs anywhere; good for summaries.",
    },
    ModelSpec {
        id: "qwen2.5-3b-q4km",
        name: "Qwen 2.5 3B Instruct",
        params_b: 3.0,
        quant: "Q4_K_M",
        repo: "bartowski/Qwen2.5-3B-Instruct-GGUF",
        file: "Qwen2.5-3B-Instruct-Q4_K_M.gguf",
        size_bytes: 1_930_000_000,
        layers: 36,
        blurb: "Fast and capable on laptops and 4 GB GPUs.",
    },
    ModelSpec {
        id: "llama3.2-3b-q4km",
        name: "Llama 3.2 3B Instruct",
        params_b: 3.2,
        quant: "Q4_K_M",
        repo: "bartowski/Llama-3.2-3B-Instruct-GGUF",
        file: "Llama-3.2-3B-Instruct-Q4_K_M.gguf",
        size_bytes: 2_020_000_000,
        layers: 28,
        blurb: "Meta's compact model; strong instruction following.",
    },
    ModelSpec {
        id: "qwen2.5-7b-q4km",
        name: "Qwen 2.5 7B Instruct",
        params_b: 7.6,
        quant: "Q4_K_M",
        repo: "bartowski/Qwen2.5-7B-Instruct-GGUF",
        file: "Qwen2.5-7B-Instruct-Q4_K_M.gguf",
        size_bytes: 4_680_000_000,
        layers: 28,
        blurb: "Sweet spot for 8 GB GPUs. Great with numbers and tables.",
    },
    ModelSpec {
        id: "llama3.1-8b-q4km",
        name: "Llama 3.1 8B Instruct",
        params_b: 8.0,
        quant: "Q4_K_M",
        repo: "bartowski/Meta-Llama-3.1-8B-Instruct-GGUF",
        file: "Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf",
        size_bytes: 4_920_000_000,
        layers: 32,
        blurb: "Well-rounded general analyst for 8 GB GPUs.",
    },
    ModelSpec {
        id: "qwen2.5-14b-q4km",
        name: "Qwen 2.5 14B Instruct",
        params_b: 14.8,
        quant: "Q4_K_M",
        repo: "bartowski/Qwen2.5-14B-Instruct-GGUF",
        file: "Qwen2.5-14B-Instruct-Q4_K_M.gguf",
        size_bytes: 8_990_000_000,
        layers: 48,
        blurb: "Deeper reasoning for 12–16 GB GPUs.",
    },
    ModelSpec {
        id: "qwen2.5-32b-q4km",
        name: "Qwen 2.5 32B Instruct",
        params_b: 32.8,
        quant: "Q4_K_M",
        repo: "bartowski/Qwen2.5-32B-Instruct-GGUF",
        file: "Qwen2.5-32B-Instruct-Q4_K_M.gguf",
        size_bytes: 19_850_000_000,
        layers: 64,
        blurb: "Flagship quality for 24 GB GPUs or 32 GB+ Apple Silicon.",
    },
];

/// Looks up a catalog entry by id.
pub fn find(id: &str) -> Option<&'static ModelSpec> {
    CATALOG.iter().find(|m| m.id == id)
}

/// Where a model is (or will be) stored.
pub fn model_path(paths: &AppPaths, spec: &ModelSpec) -> PathBuf {
    paths.models.join(spec.file)
}

/// `true` if the model file is fully downloaded.
pub fn is_downloaded(paths: &AppPaths, spec: &ModelSpec) -> bool {
    model_path(paths, spec).is_file()
}

/// How well a model fits this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fit {
    /// Entirely in VRAM: fastest.
    FullGpu,
    /// Split between VRAM and RAM.
    PartialOffload,
    /// Runs on CPU from system RAM (slow but works).
    CpuOnly,
    /// Not enough memory.
    TooLarge,
}

impl Fit {
    /// Short UI label.
    pub fn label(self) -> &'static str {
        match self {
            Self::FullGpu => "Full GPU",
            Self::PartialOffload => "Partial offload",
            Self::CpuOnly => "CPU only",
            Self::TooLarge => "Too large",
        }
    }
}

/// Classifies how `spec` would run on `hw`.
pub fn assess(spec: &ModelSpec, hw: &HardwareProfile) -> Fit {
    let need = spec.memory_needed();
    let gpu = hw.gpu_budget();
    let ram_usable = hw.ram_total / 10 * 7;
    if gpu >= need {
        Fit::FullGpu
    } else if gpu >= 2 * GIB && gpu + ram_usable >= need {
        Fit::PartialOffload
    } else if ram_usable >= need {
        Fit::CpuOnly
    } else {
        Fit::TooLarge
    }
}

/// Picks the most capable model that runs well: the largest full-GPU fit,
/// else the largest partial-offload fit, else the largest CPU model ≤ 8B.
pub fn recommend(hw: &HardwareProfile) -> &'static ModelSpec {
    let largest = |fit: Fit, max_params: f32| {
        CATALOG
            .iter()
            .rev()
            .find(|m| assess(m, hw) == fit && m.params_b <= max_params)
    };
    largest(Fit::FullGpu, f32::MAX)
        .or_else(|| largest(Fit::PartialOffload, f32::MAX))
        .or_else(|| largest(Fit::CpuOnly, 8.5))
        .unwrap_or(&CATALOG[0])
}

/// Number of layers to pass to `llama-server -ngl`.
pub fn gpu_layers(spec: &ModelSpec, hw: &HardwareProfile) -> u32 {
    match assess(spec, hw) {
        Fit::FullGpu => 999,
        Fit::PartialOffload => {
            let budget = hw.gpu_budget().saturating_sub(GIB) as f64;
            let fraction = (budget / spec.size_bytes as f64).clamp(0.0, 1.0);
            (spec.layers as f64 * fraction).floor() as u32
        }
        Fit::CpuOnly | Fit::TooLarge => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::hardware::{GpuInfo, GpuVendor};

    fn machine(ram_gib: u64, vram_gib: Option<u64>) -> HardwareProfile {
        HardwareProfile {
            cpu_name: "test".into(),
            cpu_threads: 8,
            ram_total: ram_gib * GIB,
            ram_available: ram_gib * GIB / 2,
            gpus: vram_gib
                .map(|v| GpuInfo {
                    name: "GPU".into(),
                    vendor: GpuVendor::Nvidia,
                    vram_total: v * GIB,
                    unified: false,
                })
                .into_iter()
                .collect(),
            vulkan_available: true,
        }
    }

    #[test]
    fn catalog_ids_are_unique() {
        let mut ids: Vec<_> = CATALOG.iter().map(|m| m.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
    }

    #[test]
    fn recommends_by_vram() {
        assert_eq!(recommend(&machine(32, Some(8))).id, "llama3.1-8b-q4km");
        assert_eq!(recommend(&machine(32, Some(12))).id, "qwen2.5-14b-q4km");
        assert_eq!(recommend(&machine(64, Some(24))).id, "qwen2.5-32b-q4km");
        // No GPU: cap at an 8B-class model on CPU.
        assert!(recommend(&machine(16, None)).params_b <= 8.5);
    }

    #[test]
    fn partial_offload_layers_are_bounded() {
        let hw = machine(32, Some(6));
        let spec = find("qwen2.5-14b-q4km").unwrap();
        assert_eq!(assess(spec, &hw), Fit::PartialOffload);
        let ngl = gpu_layers(spec, &hw);
        assert!(ngl > 0 && ngl < spec.layers);
    }
}
