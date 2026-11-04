//! Trading212 credential storage in the operating system's secure keychain
//! (Windows Credential Manager, macOS Keychain, Linux Secret Service) via the
//! [`keyring`] crate.
//!
//! Credentials are **never** written to disk by Aura, never logged, and the
//! [`Debug`] impl of [`T212Credentials`] is redacted.

use std::fmt;

use anyhow::{Context, Result};
use keyring::Entry;

use crate::APP_ID;
use crate::config::T212Environment;

/// API credentials for one Trading212 environment.
///
/// Trading212 issues an API key and (for newer keys) an API secret; when a
/// secret is present the client authenticates with HTTP Basic auth, otherwise
/// the key is sent as the `Authorization` header.
#[derive(Clone, PartialEq, Eq)]
pub struct T212Credentials {
    /// The API key.
    pub api_key: String,
    /// The API secret, if the key was issued with one.
    pub api_secret: Option<String>,
}

impl fmt::Debug for T212Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("T212Credentials")
            .field("api_key", &"<redacted>")
            .field(
                "api_secret",
                &self.api_secret.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

fn entry(env: T212Environment, field: &str) -> Result<Entry> {
    let user = format!("t212-{}-{field}", env.label().to_lowercase());
    Entry::new(APP_ID, &user).context("failed to open OS keychain entry")
}

/// Stores credentials for `env`, replacing any existing ones.
pub fn store(env: T212Environment, creds: &T212Credentials) -> Result<()> {
    entry(env, "api-key")?.set_password(creds.api_key.trim())?;
    match creds
        .api_secret
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(secret) => entry(env, "api-secret")?.set_password(secret)?,
        None => delete_quietly(entry(env, "api-secret")?)?,
    }
    Ok(())
}

/// Loads credentials for `env`, or `None` if none are stored.
pub fn load(env: T212Environment) -> Result<Option<T212Credentials>> {
    let api_key = match entry(env, "api-key")?.get_password() {
        Ok(k) => k,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let api_secret = match entry(env, "api-secret")?.get_password() {
        Ok(s) => Some(s),
        Err(keyring::Error::NoEntry) => None,
        Err(e) => return Err(e.into()),
    };
    Ok(Some(T212Credentials {
        api_key,
        api_secret,
    }))
}

/// Removes stored credentials for `env`.
pub fn clear(env: T212Environment) -> Result<()> {
    delete_quietly(entry(env, "api-key")?)?;
    delete_quietly(entry(env, "api-secret")?)
}

/// `true` if an API key is stored for `env`.
pub fn has_credentials(env: T212Environment) -> bool {
    matches!(load(env), Ok(Some(_)))
}

fn delete_quietly(entry: Entry) -> Result<()> {
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
