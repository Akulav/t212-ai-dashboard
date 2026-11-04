//! User settings persisted as JSON in [`AppPaths::config_file`](crate::paths::AppPaths).
//!
//! **Safety invariant:** [`Settings::trading_mode`] is never persisted. Every
//! launch starts in [`TradingMode::ReadOnly`]; the user must opt in to write
//! access again each session.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Whether the app may place/cancel orders through the Trading212 API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TradingMode {
    /// Only `GET` endpoints are ever called. The default.
    #[default]
    ReadOnly,
    /// Order endpoints are enabled (requires explicit confirmation in the UI).
    ReadWrite,
}

/// Which Trading212 environment to talk to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum T212Environment {
    /// Real-money account.
    #[default]
    Live,
    /// Practice (paper trading) account.
    Demo,
}

impl T212Environment {
    /// Base URL of the public REST API for this environment.
    pub fn base_url(self) -> &'static str {
        match self {
            Self::Live => "https://live.trading212.com/api/v0",
            Self::Demo => "https://demo.trading212.com/api/v0",
        }
    }

    /// Short label for the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Live => "Live",
            Self::Demo => "Practice",
        }
    }
}

/// All persisted user preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Current trading mode. Not persisted: always boots as read-only.
    #[serde(skip)]
    pub trading_mode: TradingMode,
    /// Trading212 environment.
    pub environment: T212Environment,
    /// Check GitHub Releases for a newer version on startup.
    pub auto_check_updates: bool,
    /// Catalog id of the last model the user selected.
    pub selected_model: Option<String>,
    /// Context window (tokens) passed to `llama-server -c`.
    pub context_size: u32,
    /// Load the selected model automatically on startup.
    pub auto_load_model: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            trading_mode: TradingMode::ReadOnly,
            environment: T212Environment::Live,
            auto_check_updates: true,
            selected_model: None,
            context_size: 8192,
            auto_load_model: false,
        }
    }
}

impl Settings {
    /// Loads settings, falling back to defaults if the file is missing or invalid.
    pub fn load(path: &Path) -> Self {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                tracing::warn!("settings file is invalid ({e}); using defaults");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Writes settings atomically (temp file + rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(self)?;
        std::fs::write(&tmp, json).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trading_mode_is_never_persisted() {
        let s = Settings {
            trading_mode: TradingMode::ReadWrite,
            ..Settings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("trading_mode"));
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.trading_mode, TradingMode::ReadOnly);
    }
}
