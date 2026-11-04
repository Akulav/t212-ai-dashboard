//! Local LLM orchestration.
//!
//! [`LlmManager`] is the single entry point the UI talks to. It owns:
//!
//! * the detected [`HardwareProfile`],
//! * the llama.cpp runtime installer ([`runtime`]),
//! * model downloads ([`models`]),
//! * the `llama-server` child process ([`server`]),
//! * a tiny OpenAI-compatible chat client ([`LlmManager::chat`]).
//!
//! All long-running work happens on the Tokio runtime; the UI polls
//! [`LlmManager::snapshot`] every frame, which is a cheap clone.
//!
//! ```text
//!  UI ──install_runtime()──▶ GitHub ggml-org/llama.cpp ──▶ runtime/llama.cpp/<tag>/
//!  UI ──download_model()───▶ huggingface.co/<repo>     ──▶ models/<file>.gguf
//!  UI ──load_model()───────▶ spawn llama-server ──poll /health──▶ Ready{port}
//!  AI features ──chat()────▶ http://127.0.0.1:<port>/v1/chat/completions
//!  exit / panic / crash ───▶ kill + reap (see `server` module docs)
//! ```

pub mod hardware;
pub mod models;
pub mod runtime;
pub mod server;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use parking_lot::Mutex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;

use crate::net::{self, Progress};
use crate::paths::AppPaths;
use hardware::HardwareProfile;
use models::ModelSpec;
use runtime::{InstallStage, RuntimeManifest};
use server::{LaunchConfig, ServerProcess};

/// How long to wait for a model to finish loading before giving up.
const LOAD_TIMEOUT: Duration = Duration::from_secs(300);

/// State of the llama.cpp runtime.
#[derive(Debug, Clone)]
pub enum RuntimeStatus {
    /// Not installed yet.
    Missing,
    /// Install in progress.
    Installing(InstallStage),
    /// Installed and ready.
    Ready(RuntimeManifest),
    /// Last install attempt failed.
    Failed(String),
}

/// State of the model server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStatus {
    /// No model loaded.
    Unloaded,
    /// Process started, model loading into memory.
    Starting {
        /// Catalog id.
        model_id: String,
    },
    /// Accepting requests.
    Ready {
        /// Catalog id.
        model_id: String,
        /// Loopback port.
        port: u16,
        /// Layers offloaded to the GPU.
        gpu_layers: u32,
    },
    /// Process exited unexpectedly or failed to load.
    Crashed(String),
}

/// Per-model download state (completed downloads live in [`LlmState::downloaded`]).
#[derive(Debug, Clone)]
pub enum DownloadStatus {
    /// In progress.
    Downloading(Progress),
    /// Failed with an error message.
    Failed(String),
}

/// Everything the UI needs to render the AI engine page.
#[derive(Debug, Clone)]
pub struct LlmState {
    /// Detected hardware.
    pub hardware: HardwareProfile,
    /// Runtime install state.
    pub runtime: RuntimeStatus,
    /// Ids of models present on disk.
    pub downloaded: HashSet<&'static str>,
    /// Active or failed downloads.
    pub downloads: HashMap<&'static str, DownloadStatus>,
    /// Server state.
    pub server: ServerStatus,
}

impl LlmState {
    /// `true` while anything is in flight (used to drive UI repaints).
    pub fn is_busy(&self) -> bool {
        matches!(self.runtime, RuntimeStatus::Installing(_))
            || matches!(self.server, ServerStatus::Starting { .. })
            || self
                .downloads
                .values()
                .any(|d| matches!(d, DownloadStatus::Downloading(_)))
    }
}

/// A chat message in OpenAI format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// `system`, `user` or `assistant`.
    pub role: String,
    /// Message text.
    pub content: String,
}

impl ChatMessage {
    /// System prompt helper.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }
    /// User message helper.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }
}

struct Inner {
    rt: Handle,
    paths: AppPaths,
    http: Client,
    state: Mutex<LlmState>,
    process: Mutex<Option<ServerProcess>>,
    /// PID of the running server (0 = none), shared with telemetry.
    pid: Arc<AtomicU32>,
    /// Bumped on every load/unload so stale health-check tasks bow out.
    generation: AtomicU64,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(mut p) = self.process.get_mut().take() {
            p.kill();
        }
    }
}

/// Cheaply clonable handle to the local LLM subsystem.
#[derive(Clone)]
pub struct LlmManager {
    inner: Arc<Inner>,
}

impl LlmManager {
    /// Detects hardware, discovers installed runtime/models and reaps any
    /// orphaned server from a previous crash.
    pub fn new(rt: Handle, paths: AppPaths, http: Client) -> Self {
        server::reap_orphan(&paths.server_pid_file());
        let hardware = hardware::detect();
        tracing::info!(
            cpu = %hardware.cpu_name,
            ram = %net::fmt_bytes(hardware.ram_total),
            gpu = ?hardware.best_gpu().map(|g| &g.name),
            "hardware detected"
        );
        let runtime =
            runtime::installed(&paths).map_or(RuntimeStatus::Missing, RuntimeStatus::Ready);
        let downloaded = scan_downloaded(&paths);
        Self {
            inner: Arc::new(Inner {
                rt,
                paths,
                http,
                state: Mutex::new(LlmState {
                    hardware,
                    runtime,
                    downloaded,
                    downloads: HashMap::new(),
                    server: ServerStatus::Unloaded,
                }),
                process: Mutex::new(None),
                pid: Arc::new(AtomicU32::new(0)),
                generation: AtomicU64::new(0),
            }),
        }
    }

    /// Clone of the current state.
    pub fn snapshot(&self) -> LlmState {
        self.inner.state.lock().clone()
    }

    /// Shared PID cell for [`crate::telemetry`] (0 when no server runs).
    pub fn pid_handle(&self) -> Arc<AtomicU32> {
        self.inner.pid.clone()
    }

    /// The model recommended for this machine.
    pub fn recommended_model(&self) -> &'static ModelSpec {
        models::recommend(&self.inner.state.lock().hardware)
    }

    /// Re-runs hardware detection.
    pub fn rescan_hardware(&self) {
        let hw = hardware::detect();
        self.inner.state.lock().hardware = hw;
    }

    /// Starts (or retries) the llama.cpp runtime installation in the background.
    pub fn install_runtime(&self) {
        let this = self.clone();
        {
            let mut st = self.inner.state.lock();
            if matches!(st.runtime, RuntimeStatus::Installing(_)) {
                return;
            }
            st.runtime = RuntimeStatus::Installing(InstallStage::Resolving);
        }
        self.inner.rt.spawn(async move {
            let inner = &this.inner;
            let hw = inner.state.lock().hardware.clone();
            let result = runtime::install(&inner.http, &inner.paths, &hw, |stage| {
                inner.state.lock().runtime = RuntimeStatus::Installing(stage);
            })
            .await;
            inner.state.lock().runtime = match result {
                Ok(m) => RuntimeStatus::Ready(m),
                Err(e) => {
                    tracing::error!("runtime install failed: {e:#}");
                    RuntimeStatus::Failed(format!("{e:#}"))
                }
            };
        });
    }

    /// Downloads a catalog model in the background (resumable).
    pub fn download_model(&self, spec: &'static ModelSpec) {
        {
            let mut st = self.inner.state.lock();
            if matches!(
                st.downloads.get(spec.id),
                Some(DownloadStatus::Downloading(_))
            ) {
                return;
            }
            st.downloads
                .insert(spec.id, DownloadStatus::Downloading(Progress::default()));
        }
        let this = self.clone();
        self.inner.rt.spawn(async move {
            let inner = &this.inner;
            let dest = models::model_path(&inner.paths, spec);
            let result = net::download_to_file(&inner.http, &spec.url(), &dest, |p| {
                inner
                    .state
                    .lock()
                    .downloads
                    .insert(spec.id, DownloadStatus::Downloading(p));
            })
            .await;
            let mut st = inner.state.lock();
            match result {
                Ok(()) => {
                    st.downloads.remove(spec.id);
                    st.downloaded.insert(spec.id);
                }
                Err(e) => {
                    tracing::error!(model = spec.id, "download failed: {e:#}");
                    st.downloads
                        .insert(spec.id, DownloadStatus::Failed(format!("{e:#}")));
                }
            }
        });
    }

    /// Deletes a downloaded model (unloading it first if it is running).
    pub fn delete_model(&self, spec: &'static ModelSpec) -> Result<()> {
        if self.loaded_model_id().as_deref() == Some(spec.id) {
            self.unload();
        }
        let path = models::model_path(&self.inner.paths, spec);
        let _ = std::fs::remove_file(net::part_path(&path));
        if path.exists() {
            std::fs::remove_file(&path).with_context(|| format!("deleting {}", path.display()))?;
        }
        let mut st = self.inner.state.lock();
        st.downloaded.remove(spec.id);
        st.downloads.remove(spec.id);
        Ok(())
    }

    /// Loads `spec` into a fresh `llama-server`, replacing any running one.
    ///
    /// The process is spawned synchronously on the calling thread (see
    /// [`ServerProcess::spawn`]); readiness is awaited in the background.
    pub fn load_model(&self, spec: &'static ModelSpec, context: u32) -> Result<()> {
        self.unload();
        let (manifest, gpu_layers) = {
            let st = self.inner.state.lock();
            let RuntimeStatus::Ready(m) = &st.runtime else {
                return Err(anyhow!("the llama.cpp runtime is not installed yet"));
            };
            if !st.downloaded.contains(spec.id) {
                return Err(anyhow!("{} is not downloaded", spec.name));
            }
            (m.clone(), models::gpu_layers(spec, &st.hardware))
        };
        let paths = &self.inner.paths;
        let cfg = LaunchConfig {
            exe: manifest.server_path(paths),
            model: models::model_path(paths, spec),
            context,
            gpu_layers,
            log_file: paths.logs.join("llama-server.log"),
            pid_file: paths.server_pid_file(),
        };
        let process = ServerProcess::spawn(&cfg)?;
        let port = process.port();
        self.inner.pid.store(process.pid(), Ordering::SeqCst);
        *self.inner.process.lock() = Some(process);
        let generation = self.inner.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.inner.state.lock().server = ServerStatus::Starting {
            model_id: spec.id.into(),
        };

        let this = self.clone();
        self.inner.rt.spawn(async move {
            let status = this.wait_until_ready(port, generation, &cfg).await;
            if this.inner.generation.load(Ordering::SeqCst) != generation {
                return; // superseded by another load/unload
            }
            let status = match status {
                Ok(()) => ServerStatus::Ready {
                    model_id: spec.id.into(),
                    port,
                    gpu_layers,
                },
                Err(e) => {
                    this.kill_process();
                    ServerStatus::Crashed(format!("{e:#}"))
                }
            };
            this.inner.state.lock().server = status;
        });
        Ok(())
    }

    async fn wait_until_ready(&self, port: u16, generation: u64, cfg: &LaunchConfig) -> Result<()> {
        let url = format!("http://127.0.0.1:{port}/health");
        let started = Instant::now();
        loop {
            if self.inner.generation.load(Ordering::SeqCst) != generation {
                return Ok(());
            }
            let exited = self
                .inner
                .process
                .lock()
                .as_mut()
                .and_then(|p| p.exit_status());
            if let Some(status) = exited {
                return Err(anyhow!(
                    "llama-server exited ({status}).\n{}",
                    server::log_tail(&cfg.log_file, 15)
                ));
            }
            if let Ok(resp) = self
                .inner
                .http
                .get(&url)
                .timeout(Duration::from_secs(2))
                .send()
                .await
                && resp.status().is_success()
            {
                return Ok(());
            }
            if started.elapsed() > LOAD_TIMEOUT {
                return Err(anyhow!(
                    "model did not become ready within {LOAD_TIMEOUT:?}"
                ));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Stops the server and frees its memory. Safe to call at any time.
    pub fn unload(&self) {
        self.inner.generation.fetch_add(1, Ordering::SeqCst);
        self.kill_process();
        self.inner.state.lock().server = ServerStatus::Unloaded;
    }

    /// Synchronous, idempotent, panic-safe kill used on exit. Never blocks
    /// for more than ~0.5 s waiting on a lock (the lock holder may be the
    /// panicking thread).
    pub fn shutdown(&self) {
        self.inner.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(mut guard) = self.inner.process.try_lock_for(Duration::from_millis(500))
            && let Some(mut p) = guard.take()
        {
            p.kill();
        }
        self.inner.pid.store(0, Ordering::SeqCst);
    }

    fn kill_process(&self) {
        let taken = self.inner.process.lock().take();
        if let Some(mut p) = taken {
            p.kill();
        }
        self.inner.pid.store(0, Ordering::SeqCst);
    }

    /// Catalog id of the model currently loaded or loading.
    pub fn loaded_model_id(&self) -> Option<String> {
        match &self.inner.state.lock().server {
            ServerStatus::Starting { model_id } | ServerStatus::Ready { model_id, .. } => {
                Some(model_id.clone())
            }
            _ => None,
        }
    }

    /// Base URL of the ready server, e.g. `http://127.0.0.1:51234`.
    pub fn endpoint(&self) -> Option<String> {
        match &self.inner.state.lock().server {
            ServerStatus::Ready { port, .. } => Some(format!("http://127.0.0.1:{port}")),
            _ => None,
        }
    }

    /// Sends a non-streaming chat completion to the local model.
    pub async fn chat(&self, messages: Vec<ChatMessage>, max_tokens: u32) -> Result<String> {
        #[derive(Deserialize)]
        struct Choice {
            message: ChatMessage,
        }
        #[derive(Deserialize)]
        struct Response {
            choices: Vec<Choice>,
        }

        let base = self
            .endpoint()
            .ok_or_else(|| anyhow!("no model is loaded"))?;
        let body = serde_json::json!({
            "model": "aura",
            "messages": messages,
            "max_tokens": max_tokens,
            "temperature": 0.3,
            "stream": false,
        });
        let resp: Response = self
            .inner
            .http
            .post(format!("{base}/v1/chat/completions"))
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        resp.choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| anyhow!("model returned no choices"))
    }
}

fn scan_downloaded(paths: &AppPaths) -> HashSet<&'static str> {
    models::CATALOG
        .iter()
        .filter(|m| models::is_downloaded(paths, m))
        .map(|m| m.id)
        .collect()
}
