//! Trading212 Public API client.
//!
//! * **Auth:** HTTP Basic (`API_KEY:API_SECRET`). Legacy keys without a secret
//!   are sent raw in the `Authorization` header.
//! * **Rate limits:** paced client-side per endpoint family ([`rate_limit`]);
//!   on `429` the client honours `x-ratelimit-reset` and retries.
//! * **Pagination:** history endpoints are cursor-based; [`T212Client::paginate`]
//!   follows `nextPagePath` and can stop early for incremental sync.
//! * **Safety:** every mutating call goes through [`T212Client::mutating`],
//!   which refuses unless the client was built with
//!   [`TradingMode::ReadWrite`]. The guard lives *in the client*, not the UI.
//!
//! Endpoint wrappers are in [`endpoints`]; response types in [`models`].

pub mod endpoints;
pub mod models;
pub mod rate_limit;

use std::sync::Arc;
use std::time::Duration;

use reqwest::{Client, Method, RequestBuilder, Response, StatusCode, header};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::config::{T212Environment, TradingMode};
use crate::secrets::T212Credentials;
use models::Page;
use rate_limit::{Bucket, RateLimiter};

/// Maximum page size accepted by history endpoints.
pub const MAX_PAGE_SIZE: u32 = 50;

/// Errors returned by the client, with user-presentable messages.
#[derive(Debug, thiserror::Error)]
pub enum T212Error {
    /// 401 / 403.
    #[error(
        "Trading212 rejected the credentials (HTTP {0}). Check the API key/secret and its permissions."
    )]
    Unauthorized(u16),
    /// Write call attempted in read-only mode.
    #[error(
        "blocked: Aura is in read-only mode. Enable write mode in Settings to place or cancel orders."
    )]
    ReadOnly,
    /// 429 after all retries.
    #[error("Trading212 rate limit reached; try again shortly")]
    RateLimited,
    /// Any other non-success status.
    #[error("Trading212 API error (HTTP {status}): {body}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// Response body (truncated).
        body: String,
    },
    /// Network / TLS / timeout.
    #[error("network error talking to Trading212: {0}")]
    Network(#[from] reqwest::Error),
    /// Response did not match the expected schema.
    #[error("unexpected Trading212 response: {0}")]
    Decode(#[from] serde_json::Error),
}

/// Result alias for this module.
pub type Result<T> = std::result::Result<T, T212Error>;

struct Inner {
    http: Client,
    env: T212Environment,
    creds: T212Credentials,
    mode: TradingMode,
    limiter: RateLimiter,
}

/// Cheaply clonable Trading212 client.
#[derive(Clone)]
pub struct T212Client {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for T212Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("T212Client")
            .field("env", &self.inner.env)
            .field("mode", &self.inner.mode)
            .finish_non_exhaustive()
    }
}

impl T212Client {
    /// Creates a client. Use [`TradingMode::ReadOnly`] unless the user has
    /// explicitly enabled trading for this session.
    pub fn new(
        http: Client,
        env: T212Environment,
        creds: T212Credentials,
        mode: TradingMode,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                http,
                env,
                creds,
                mode,
                limiter: RateLimiter::default(),
            }),
        }
    }

    /// Environment this client talks to.
    pub fn environment(&self) -> T212Environment {
        self.inner.env
    }

    /// Trading mode this client was built with.
    pub fn mode(&self) -> TradingMode {
        self.inner.mode
    }

    /// Guard for state-changing endpoints. Returns [`T212Error::ReadOnly`]
    /// unless the client is in [`TradingMode::ReadWrite`].
    pub fn mutating(&self) -> Result<()> {
        match self.inner.mode {
            TradingMode::ReadWrite => Ok(()),
            TradingMode::ReadOnly => Err(T212Error::ReadOnly),
        }
    }

    /// Resolves an API path (`/equity/...`) or a full `nextPagePath`
    /// (`/api/v0/equity/...?cursor=…`) to an absolute URL.
    fn url(&self, path: &str) -> String {
        let base = self.inner.env.base_url(); // https://host/api/v0
        if path.starts_with("/api/") {
            let origin = base.trim_end_matches("/api/v0");
            format!("{origin}{path}")
        } else {
            format!("{base}{path}")
        }
    }

    fn authorize(&self, req: RequestBuilder) -> RequestBuilder {
        let creds = &self.inner.creds;
        match creds.api_secret.as_deref().filter(|s| !s.is_empty()) {
            Some(secret) => req.basic_auth(&creds.api_key, Some(secret)),
            None => req.header(header::AUTHORIZATION, &creds.api_key),
        }
    }

    async fn send(
        &self,
        bucket: Bucket,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<Response> {
        const MAX_ATTEMPTS: u32 = 3;
        for attempt in 1..=MAX_ATTEMPTS {
            self.inner.limiter.acquire(bucket).await;
            let mut req = self
                .authorize(self.inner.http.request(method.clone(), self.url(path)))
                .timeout(Duration::from_secs(30));
            if let Some(body) = body {
                req = req.json(body);
            }
            let resp = req.send().await?;
            match resp.status() {
                s if s.is_success() => return Ok(resp),
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                    return Err(T212Error::Unauthorized(resp.status().as_u16()));
                }
                StatusCode::TOO_MANY_REQUESTS if attempt < MAX_ATTEMPTS => {
                    let wait = retry_after(&resp).unwrap_or(bucket.interval());
                    tracing::warn!(?bucket, ?wait, "rate limited by Trading212; backing off");
                    self.inner
                        .limiter
                        .back_off(bucket, tokio::time::Instant::now() + wait);
                }
                StatusCode::TOO_MANY_REQUESTS => return Err(T212Error::RateLimited),
                s => {
                    let mut body = resp.text().await.unwrap_or_default();
                    body.truncate(300);
                    return Err(T212Error::Api {
                        status: s.as_u16(),
                        body,
                    });
                }
            }
        }
        Err(T212Error::RateLimited)
    }

    /// `GET` and decode JSON.
    pub(crate) async fn get<T: DeserializeOwned>(&self, bucket: Bucket, path: &str) -> Result<T> {
        let bytes = self
            .send(bucket, Method::GET, path, None)
            .await?
            .bytes()
            .await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// `POST` a JSON body and decode the JSON response. Mutating: guarded.
    pub(crate) async fn post<B: Serialize, T: DeserializeOwned>(
        &self,
        bucket: Bucket,
        path: &str,
        body: &B,
    ) -> Result<T> {
        self.mutating()?;
        let value = serde_json::to_value(body)?;
        let bytes = self
            .send(bucket, Method::POST, path, Some(&value))
            .await?
            .bytes()
            .await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// `DELETE`. Mutating: guarded.
    pub(crate) async fn delete(&self, bucket: Bucket, path: &str) -> Result<()> {
        self.mutating()?;
        self.send(bucket, Method::DELETE, path, None).await?;
        Ok(())
    }

    /// Follows cursor pagination starting at `first_path`.
    ///
    /// `keep_going` is called with each page's items; return `false` to stop
    /// early (e.g. once already-cached items are reached). At most
    /// `max_pages` pages are fetched.
    pub async fn paginate<T: DeserializeOwned>(
        &self,
        first_path: String,
        max_pages: usize,
        mut keep_going: impl FnMut(&[T]) -> bool,
    ) -> Result<Vec<T>> {
        let mut all = Vec::new();
        let mut path = Some(first_path);
        let mut pages = 0;
        while let Some(p) = path.take() {
            let page: Page<T> = self.get(Bucket::History, &p).await?;
            pages += 1;
            let more = keep_going(&page.items);
            all.extend(page.items);
            if more && pages < max_pages {
                path = page.next_page_path.filter(|s| !s.is_empty());
            }
        }
        Ok(all)
    }
}

/// Seconds until `x-ratelimit-reset` (unix timestamp), capped at 60 s.
fn retry_after(resp: &Response) -> Option<Duration> {
    let reset: i64 = resp
        .headers()
        .get("x-ratelimit-reset")?
        .to_str()
        .ok()?
        .parse()
        .ok()?;
    let secs = (reset - chrono::Utc::now().timestamp()).clamp(1, 60);
    Some(Duration::from_secs(secs as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(mode: TradingMode) -> T212Client {
        T212Client::new(
            Client::new(),
            T212Environment::Demo,
            T212Credentials {
                api_key: "k".into(),
                api_secret: Some("s".into()),
            },
            mode,
        )
    }

    #[test]
    fn read_only_blocks_mutations() {
        assert!(matches!(
            client(TradingMode::ReadOnly).mutating(),
            Err(T212Error::ReadOnly)
        ));
        assert!(client(TradingMode::ReadWrite).mutating().is_ok());
    }

    #[tokio::test]
    async fn read_only_order_never_hits_network() {
        // Unroutable credentials + read-only: must fail with ReadOnly, not Network.
        let req = models::MarketOrderRequest {
            ticker: "AAPL_US_EQ".into(),
            quantity: 1.0,
            extended_hours: false,
        };
        let err = client(TradingMode::ReadOnly)
            .place_market_order(&req)
            .await
            .unwrap_err();
        assert!(matches!(err, T212Error::ReadOnly));
    }

    #[test]
    fn resolves_next_page_paths() {
        let c = client(TradingMode::ReadOnly);
        assert_eq!(
            c.url("/equity/positions"),
            "https://demo.trading212.com/api/v0/equity/positions"
        );
        assert_eq!(
            c.url("/api/v0/equity/history/dividends?limit=50&cursor=9"),
            "https://demo.trading212.com/api/v0/equity/history/dividends?limit=50&cursor=9"
        );
    }
}
