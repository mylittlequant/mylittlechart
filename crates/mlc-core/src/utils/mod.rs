//! Utility functions
//!
//! Platform-independent utilities for charts:
//!
//! - `color` - CSS color parsing (`parse_css_color`)
//! - `format` - Number formatting (`format_indicator_value`)
//! - `math` - Mathematical functions (`catmull_rom_spline`)

pub mod color;
pub mod format;
pub mod math;

// Re-export main functions
pub use color::{parse_css_color, apply_opacity, rgba_to_hex};
pub use format::format_indicator_value;
pub use math::catmull_rom_spline;
