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
pub mod time_utils;

// Re-export main functions
pub use color::{parse_css_color, apply_opacity, mix_colors, rgba_to_hex, color_alpha, with_alpha};
pub use format::format_indicator_value;
pub use math::catmull_rom_spline;
pub use time_utils::{unix_now_secs, unix_timestamp_parts};
