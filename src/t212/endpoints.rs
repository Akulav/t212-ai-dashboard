//! Typed wrappers for each Trading212 endpoint.
//!
//! Every method documents its HTTP method, path, rate limit and whether it
//! **mutates** the account. Mutating methods are rejected in read-only mode
//! before any network I/O happens.

use super::models::*;
use super::rate_limit::Bucket;
use super::{MAX_PAGE_SIZE, Result, T212Client};

impl T212Client {
    // ---- Account ----------------------------------------------------------

    /// Cash and investment totals.
    ///
    /// `GET /equity/account/summary` · 1 req / 5 s · read-only.
    pub async fn account_summary(&self) -> Result<AccountSummary> {
        self.get(Bucket::AccountSummary, "/equity/account/summary")
            .await
    }

    // ---- Positions --------------------------------------------------------

    /// All open positions.
    ///
    /// `GET /equity/positions` · 1 req / 1 s · read-only.
    pub async fn positions(&self) -> Result<Vec<Position>> {
        self.get(Bucket::Positions, "/equity/positions").await
    }

    // ---- History ----------------------------------------------------------

    /// Dividend payments, newest first. Stops once `stop_at` returns `true`
    /// for an item (used for incremental sync against a local cache).
    ///
    /// `GET /equity/history/dividends` · 20 req / 60 s · paginated · read-only.
    pub async fn dividends(
        &self,
        max_pages: usize,
        stop_at: impl Fn(&Dividend) -> bool,
    ) -> Result<Vec<Dividend>> {
        let path = format!("/equity/history/dividends?limit={MAX_PAGE_SIZE}");
        let mut items = self
            .paginate(path, max_pages, |page: &[Dividend]| {
                !page.iter().any(&stop_at)
            })
            .await?;
        if let Some(pos) = items.iter().position(&stop_at) {
            items.truncate(pos);
        }
        Ok(items)
    }

    /// Deposits, withdrawals, fees and interest, newest first.
    ///
    /// `GET /equity/history/transactions` · 20 req / 60 s · paginated · read-only.
    pub async fn transactions(
        &self,
        max_pages: usize,
        stop_at: impl Fn(&Transaction) -> bool,
    ) -> Result<Vec<Transaction>> {
        let path = format!("/equity/history/transactions?limit={MAX_PAGE_SIZE}");
        let mut items = self
            .paginate(path, max_pages, |page: &[Transaction]| {
                !page.iter().any(&stop_at)
            })
            .await?;
        if let Some(pos) = items.iter().position(&stop_at) {
            items.truncate(pos);
        }
        Ok(items)
    }

    /// Historical orders with fills, newest first, optionally for one ticker.
    ///
    /// `GET /equity/history/orders` · 20 req / 60 s · paginated · read-only.
    pub async fn order_history(
        &self,
        ticker: Option<&str>,
        max_pages: usize,
    ) -> Result<Vec<HistoricalOrder>> {
        let mut path = format!("/equity/history/orders?limit={MAX_PAGE_SIZE}");
        if let Some(t) = ticker {
            path.push_str(&format!("&ticker={t}"));
        }
        self.paginate(path, max_pages, |_: &[HistoricalOrder]| true)
            .await
    }

    // ---- Metadata ---------------------------------------------------------

    /// Every tradable instrument (large payload; cache it).
    ///
    /// `GET /equity/metadata/instruments` · 1 req / 50 s · read-only.
    pub async fn instruments(&self) -> Result<Vec<TradableInstrument>> {
        self.get(Bucket::Instruments, "/equity/metadata/instruments")
            .await
    }

    // ---- Orders -----------------------------------------------------------

    /// Pending (unfilled) orders.
    ///
    /// `GET /equity/orders` · read-only.
    pub async fn active_orders(&self) -> Result<Vec<Order>> {
        self.get(Bucket::Orders, "/equity/orders").await
    }

    /// Places a market order (negative quantity sells). **Mutates the account.**
    ///
    /// `POST /equity/orders/market` · requires [`TradingMode::ReadWrite`](crate::config::TradingMode).
    pub async fn place_market_order(&self, req: &MarketOrderRequest) -> Result<Order> {
        self.post(Bucket::Orders, "/equity/orders/market", req)
            .await
    }

    /// Places a limit order (negative quantity sells). **Mutates the account.**
    ///
    /// `POST /equity/orders/limit` · requires [`TradingMode::ReadWrite`](crate::config::TradingMode).
    pub async fn place_limit_order(&self, req: &LimitOrderRequest) -> Result<Order> {
        self.post(Bucket::Orders, "/equity/orders/limit", req).await
    }

    /// Cancels a pending order. **Mutates the account.** Not guaranteed if the
    /// order is already being filled.
    ///
    /// `DELETE /equity/orders/{id}` · requires [`TradingMode::ReadWrite`](crate::config::TradingMode).
    pub async fn cancel_order(&self, id: i64) -> Result<()> {
        self.delete(Bucket::Orders, &format!("/equity/orders/{id}"))
            .await
    }
}
