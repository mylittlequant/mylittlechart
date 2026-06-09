//! Projection module - trading positions and forecasts

pub mod long_position;
pub mod short_position;
pub mod bars_pattern;
pub mod price_projection;
pub mod projection;

pub use long_position::LongPosition;
pub use short_position::ShortPosition;
pub use bars_pattern::BarsPattern;
pub use price_projection::PriceProjection;
pub use projection::Projection;
