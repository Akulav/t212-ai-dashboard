//! Serde models for the Trading212 Public API (v0).
//!
//! Field names mirror the official OpenAPI spec
//! (<https://docs.trading212.com/api>). The API is in beta, so every
//! non-essential field is `#[serde(default)]` / `Option` to stay forward- and
//! backward-compatible, and large enums (e.g. ~80 dividend types) are kept as
//! plain `String`s.
//!
//! Monetary units:
//! * fields under `walletImpact`, `amount`, account summary → **account currency**
//! * `currentPrice`, `averagePricePaid`, `grossAmountPerShare` → **instrument currency**
//!   (note: London listings quote in `GBX`, i.e. pence).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// `GET /equity/account/summary`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountSummary {
    /// Account id.
    pub id: i64,
    /// Primary account currency (ISO 4217).
    pub currency: String,
    /// Total account value: cash + investments.
    pub total_value: f64,
    /// Cash breakdown.
    pub cash: Cash,
    /// Investment breakdown.
    pub investments: Investments,
}

/// Cash part of [`AccountSummary`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Cash {
    /// Free cash that can be used to buy.
    pub available_to_trade: f64,
    /// Uninvested cash held inside pies.
    pub in_pies: f64,
    /// Cash blocked by pending orders.
    pub reserved_for_orders: f64,
}

/// Investments part of [`AccountSummary`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Investments {
    /// Market value of all positions.
    pub current_value: f64,
    /// Cost basis of open positions.
    pub total_cost: f64,
    /// Lifetime realised P/L.
    pub realized_profit_loss: f64,
    /// Unrealised P/L of open positions.
    pub unrealized_profit_loss: f64,
}

/// Instrument reference embedded in positions, dividends and orders.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Instrument {
    /// Trading212 ticker, e.g. `AAPL_US_EQ`.
    pub ticker: String,
    /// Company / fund name.
    pub name: String,
    /// ISIN.
    pub isin: String,
    /// Quote currency (may be `GBX`).
    pub currency: String,
}

/// One open position (`GET /equity/positions`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Position {
    /// Instrument held.
    pub instrument: Instrument,
    /// Shares held (fractional allowed).
    pub quantity: f64,
    /// Shares not locked in pies or orders.
    pub quantity_available_for_trading: f64,
    /// Shares held via pies.
    pub quantity_in_pies: f64,
    /// Average purchase price, instrument currency.
    pub average_price_paid: f64,
    /// Last price, instrument currency.
    pub current_price: f64,
    /// When the position was opened.
    pub created_at: Option<DateTime<Utc>>,
    /// Value / cost / P&L in account currency.
    pub wallet_impact: PositionWalletImpact,
}

/// Account-currency view of a [`Position`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PositionWalletImpact {
    /// Account currency.
    pub currency: String,
    /// Market value.
    pub current_value: f64,
    /// Cost basis.
    pub total_cost: f64,
    /// Unrealised P/L (includes FX impact).
    pub unrealized_profit_loss: f64,
    /// Portion of P/L caused by currency moves.
    pub fx_impact: Option<f64>,
}

/// Cursor-paginated list wrapper used by `/equity/history/*`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    /// Items on this page (newest first).
    #[serde(default = "Vec::new")]
    pub items: Vec<T>,
    /// Full path of the next page, `None` on the last page.
    pub next_page_path: Option<String>,
}

/// A dividend (or other distribution) payment.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Dividend {
    /// Unique reference; used for de-duplication and incremental sync.
    pub reference: String,
    /// Ticker that paid.
    pub ticker: String,
    /// Instrument details.
    pub instrument: Instrument,
    /// Net amount credited, account currency.
    pub amount: f64,
    /// Net amount in EUR.
    pub amount_in_euro: f64,
    /// Account currency of [`Self::amount`].
    pub currency: String,
    /// Gross amount per share, in [`Self::ticker_currency`].
    pub gross_amount_per_share: f64,
    /// Shares the payment was made on.
    pub quantity: f64,
    /// Currency of the gross amount (may be `GBX`).
    pub ticker_currency: String,
    /// Payment date.
    pub paid_on: Option<DateTime<Utc>>,
    /// Distribution type, e.g. `ORDINARY`.
    #[serde(rename = "type")]
    pub kind: String,
}

/// A cash movement (`GET /equity/history/transactions`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Transaction {
    /// Unique reference.
    pub reference: String,
    /// Amount (negative for outflows).
    pub amount: f64,
    /// Currency of [`Self::amount`].
    pub currency: String,
    /// Timestamp.
    pub date_time: Option<DateTime<Utc>>,
    /// `DEPOSIT`, `WITHDRAW`, `FEE`, `TRANSFER`, `INTEREST_ON_FREE_CASH`, `LENDING_INTEREST`.
    #[serde(rename = "type")]
    pub kind: String,
}

/// An order (pending or historical).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Order {
    /// Order id.
    pub id: i64,
    /// Instrument.
    pub instrument: Instrument,
    /// Ordered quantity (negative = sell).
    pub quantity: f64,
    /// Executed quantity.
    pub filled_quantity: f64,
    /// `BUY` / `SELL`.
    pub side: String,
    /// `NEW`, `FILLED`, `CANCELLED`, …
    pub status: String,
    /// Limit price, if any.
    pub limit_price: Option<f64>,
    /// Stop price, if any.
    pub stop_price: Option<f64>,
    /// Where the order came from (`API`, `IOS`, `WEB`, …).
    pub initiated_from: String,
    /// Creation time.
    pub created_at: Option<DateTime<Utc>>,
}

/// Historical order with its fill (`GET /equity/history/orders`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoricalOrder {
    /// The order.
    pub order: Order,
    /// Its execution, if filled.
    pub fill: Option<Fill>,
}

/// Execution details of a historical order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Fill {
    /// Fill id.
    pub id: i64,
    /// Execution price, instrument currency.
    pub price: f64,
    /// Executed quantity.
    pub quantity: f64,
    /// Execution time.
    pub filled_at: Option<DateTime<Utc>>,
    /// Account-currency impact incl. fees and taxes.
    pub wallet_impact: Option<FillWalletImpact>,
}

/// Account-currency effect of a [`Fill`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FillWalletImpact {
    /// Account currency.
    pub currency: String,
    /// Net cash effect.
    pub net_value: f64,
    /// FX rate applied.
    pub fx_rate: f64,
    /// Realised P/L of this fill.
    pub realised_profit_loss: f64,
    /// Transaction taxes and fees (stamp duty, FX fee, …).
    pub taxes: Vec<FillTax>,
}

/// A tax or fee charged on a fill.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FillTax {
    /// e.g. `STAMP_DUTY`, `CURRENCY_CONVERSION_FEE`.
    pub name: String,
    /// Amount charged.
    pub quantity: f64,
    /// Currency.
    pub currency: String,
}

/// Instrument metadata (`GET /equity/metadata/instruments`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TradableInstrument {
    /// Ticker.
    pub ticker: String,
    /// Full name.
    pub name: String,
    /// Short name / symbol.
    pub short_name: String,
    /// ISIN.
    pub isin: String,
    /// Quote currency.
    pub currency_code: String,
    /// `STOCK`, `ETF`, …
    #[serde(rename = "type")]
    pub kind: String,
}

/// How long a limit/stop order stays active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TimeValidity {
    /// Expires at the exchange's midnight.
    Day,
    /// Active until filled or cancelled.
    GoodTillCancel,
}

/// `POST /equity/orders/market` body. Negative quantity sells.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketOrderRequest {
    /// Ticker.
    pub ticker: String,
    /// Shares (negative = sell).
    pub quantity: f64,
    /// Allow execution outside regular hours.
    pub extended_hours: bool,
}

/// `POST /equity/orders/limit` body. Negative quantity sells.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitOrderRequest {
    /// Ticker.
    pub ticker: String,
    /// Shares (negative = sell).
    pub quantity: f64,
    /// Limit price, instrument currency.
    pub limit_price: f64,
    /// Validity.
    pub time_validity: TimeValidity,
}
