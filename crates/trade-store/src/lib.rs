mod error;
mod format;
mod store;

pub use error::TradeStoreError;
pub use store::TradeStoreHandle;

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
