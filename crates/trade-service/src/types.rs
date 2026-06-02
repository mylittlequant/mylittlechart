use digdigdig3::{ExchangeId, AccountType};

/// A single public trade tick from the exchange.
///
/// 40 bytes with `#[repr(C)]`:
/// - `[0..8]`   `timestamp_ms: i64`  — exchange timestamp, milliseconds
/// - `[8..16]`  `price: f64`
/// - `[16..24]` `quantity: f64`
/// - `[24..32]` `trade_id: u64`      — exchange-assigned id; 0 if not provided
/// - `[32]`     `is_buyer_maker: u8` — 0 or 1
/// - `[33..40]` padding zeros
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Trade {
    pub timestamp_ms: i64,
    pub price: f64,
    pub quantity: f64,
    pub trade_id: u64,
    pub is_buyer_maker: u8,
    pub _pad: [u8; 7],
}

/// Canonical key for a trade series.
///
/// No `timeframe` field — trades are not keyed by timeframe.
/// Each panel that needs a specific timeframe view derives its own
/// aggregation from the same raw ring.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TradeKey {
    pub exchange_id: ExchangeId,
    pub account_type: AccountType,
    pub symbol: String,
}

impl TradeKey {
    pub fn new(
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: impl Into<String>,
    ) -> Self {
        Self {
            exchange_id,
            account_type,
            symbol: symbol.into(),
        }
    }


}
