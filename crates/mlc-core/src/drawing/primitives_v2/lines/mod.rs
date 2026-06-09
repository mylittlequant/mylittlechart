//! Line-based primitives
//!
//! This module contains all line-based drawing tools:
//! - Trend Line: simple two-point line
//! - Horizontal Line: single price level
//! - Vertical Line: single bar/time
//! - Ray: line extending to the right
//! - Extended Line: line extending both directions
//! - Info Line: line with price/percentage info
//! - Trend Angle: trend line with angle measurement
//! - Horizontal Ray: horizontal line extending right only
//! - Cross Line: crossing horizontal and vertical lines

pub mod trend_line;
pub mod horizontal_line;
pub mod vertical_line;
pub mod ray;
pub mod extended_line;
pub mod info_line;
pub mod trend_angle;
pub mod horizontal_ray;
pub mod cross_line;

// Re-export primitive types
pub use trend_line::TrendLine;
pub use horizontal_line::HorizontalLine;
pub use vertical_line::VerticalLine;
pub use ray::Ray;
pub use extended_line::ExtendedLine;
pub use info_line::InfoLine;
pub use trend_angle::TrendAngle;
pub use horizontal_ray::HorizontalRay;
pub use cross_line::CrossLine;
