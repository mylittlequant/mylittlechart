//! Annotations - Data-point markers and price lines
//!
//! This module contains:
//! - Markers for annotating specific bars
//! - Price lines for horizontal price levels

pub mod markers;
pub mod price_line;

// Re-exports
pub use markers::{Marker, MarkerCoordinates, MarkerManager, MarkerPosition, MarkerShape};
pub use price_line::{LineStyle, PriceLine, PriceLineOptions};
