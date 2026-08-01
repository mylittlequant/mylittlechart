//! Funding-rate / open-interest counters for the watchlist TICKER section's
//! active-instrument block (owner spec 2026-08-01,
//! `docs/mlc/research/oi-funding-mlc-wiring-2026-08-01.md`).
//!
//! Pure cache shape + key helper — no compute here (unlike `screener`), the
//! forwarder in `mlc-backend-data-bridge` already delivers finished
//! `rate`/`oi`/`d1h_pct`/`d24h_pct` values via `LiveUpdate::FundingCounterUpdate`
//! / `LiveUpdate::OiCounterUpdate`. Keyed the SAME grammar as `mini_ticker`
//! (`"{symbol}:{exchange}:{account_type}"`) so lookups never need reformatting.

/// Cached per-venue funding-rate + open-interest counter for the ACTIVE
/// chart's instrument. One entry per `(symbol, exchange, account_type)` —
/// counters are always subscribed at `AccountType::Futures` (`"F"`)
/// regardless of the active chart's own account type.
#[derive(Clone, Copy, Debug, Default)]
pub struct MarketCounters {
    /// Dimensionless funding rate (0.0001 = 1 bps = 0.01%). `None` until the
    /// first `FundingCounterUpdate` for this key arrives.
    pub funding_rate: Option<f64>,
    /// Epoch ms of the next funding settlement — only ever set to a value
    /// strictly in the future (the bridge forwarder filters degenerate/past
    /// warm-start rows before emitting; see `FundingCounterUpdate`'s doc).
    pub next_funding_time_ms: Option<i64>,
    /// Wall-clock ms when `funding_rate`/`next_funding_time_ms` were last
    /// written.
    pub funding_updated_ms: i64,
    /// Absolute open interest (contracts or base units, venue-dependent).
    pub oi: Option<f64>,
    /// Open interest notional (quote/USD), when the venue reports it.
    pub oi_value: Option<f64>,
    /// OI Δ% over the trailing 1h — `None` until the forwarder's history
    /// ring has a sample close enough to that lookback.
    pub oi_d1h_pct: Option<f64>,
    /// OI Δ% over the trailing 24h — same honesty rule as `oi_d1h_pct`.
    pub oi_d24h_pct: Option<f64>,
    /// Wall-clock ms when `oi`/`oi_value`/`oi_d1h_pct`/`oi_d24h_pct` were
    /// last written.
    pub oi_updated_ms: i64,
}

/// Build the market-counters cache key — SAME grammar as
/// `BackendCore::mini_ticker` / `screener::screener_key`
/// (`"{symbol}:{exchange}:{account_type}"`).
pub fn market_counters_key(symbol: &str, exchange: &str, account_type_label: &str) -> String {
    format!("{}:{}:{}", symbol, exchange, account_type_label)
}
