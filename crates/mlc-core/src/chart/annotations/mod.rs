//! Annotations - Price lines.
//!
//! The `Marker` family (chart bar annotations: arrows, circles, etc.) was
//! removed — it duplicated `SystemSignal` + `Trade` (stratategy/trade
//! overlays) with zero unique callers.  `SignalManager` and `TradeManager`
//! cover the same use case with strategy/trade-id metadata and PnL.

pub mod price_line;

pub use price_line::{LineStyle, PriceLine, PriceLineOptions};
