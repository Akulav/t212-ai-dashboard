//! Client-side pacing for Trading212's per-account rate limits.
//!
//! Each endpoint family gets a minimum interval between calls (taken from the
//! official docs). Callers `await` [`RateLimiter::acquire`] before sending; the
//! lock is never held across an `.await`.

use std::collections::HashMap;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::time::Instant;

/// Endpoint families with distinct limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bucket {
    /// `/equity/account/summary` — 1 req / 5 s.
    AccountSummary,
    /// `/equity/positions` — 1 req / 1 s.
    Positions,
    /// `/equity/history/*` — 20 req / 60 s.
    History,
    /// `/equity/metadata/instruments` — 1 req / 50 s.
    Instruments,
    /// `/equity/metadata/exchanges` — 1 req / 30 s.
    Exchanges,
    /// `/equity/orders*` — 1 req / 1 s (conservative; not documented for all).
    Orders,
}

impl Bucket {
    /// Minimum spacing between two requests in this bucket.
    pub fn interval(self) -> Duration {
        Duration::from_millis(match self {
            Self::AccountSummary => 5_000,
            Self::Positions => 1_000,
            Self::History => 3_000,
            Self::Instruments => 50_000,
            Self::Exchanges => 30_000,
            Self::Orders => 1_000,
        })
    }
}

/// Tracks the earliest permitted send time per [`Bucket`].
#[derive(Debug, Default)]
pub struct RateLimiter {
    next: Mutex<HashMap<Bucket, Instant>>,
}

impl RateLimiter {
    /// Waits until a request in `bucket` may be sent, and reserves that slot.
    pub async fn acquire(&self, bucket: Bucket) {
        let wait_until = {
            let mut next = self.next.lock();
            let now = Instant::now();
            let slot = next
                .get(&bucket)
                .copied()
                .filter(|t| *t > now)
                .unwrap_or(now);
            next.insert(bucket, slot + bucket.interval());
            slot
        };
        tokio::time::sleep_until(wait_until).await;
    }

    /// Pushes the bucket's next slot out to at least `until` (after a 429).
    pub fn back_off(&self, bucket: Bucket, until: Instant) {
        let mut next = self.next.lock();
        let entry = next.entry(bucket).or_insert(until);
        if *entry < until {
            *entry = until;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn spaces_requests_in_same_bucket() {
        let limiter = RateLimiter::default();
        let start = Instant::now();
        limiter.acquire(Bucket::AccountSummary).await;
        limiter.acquire(Bucket::AccountSummary).await;
        assert!(start.elapsed() >= Duration::from_secs(5));
        // Other buckets are independent.
        let t = Instant::now();
        limiter.acquire(Bucket::Positions).await;
        assert!(t.elapsed() < Duration::from_millis(10));
    }
}
