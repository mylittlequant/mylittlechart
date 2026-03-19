//! Primitives module - organized by category
//!
//! # Architecture
//!
//! Each primitive category is a submodule containing:
//! - Individual primitive types implementing the `Primitive` trait
//! - Category-specific shared utilities
//!
//! # Categories
//!
//! ```text
//! primitives/
//! ├── mod.rs              # This file - exports and registry
//! ├── traits.rs           # Core Primitive trait definition
//! ├── types.rs            # Shared types (PrimitiveColor, PrimitiveText, etc.)
//! ├── registry.rs         # PrimitiveRegistry for factory pattern
//! │
//! ├── lines/              # Line-based primitives
//! │   ├── mod.rs
//! │   ├── trend_line.rs
//! │   ├── horizontal_line.rs
//! │   ├── vertical_line.rs
//! │   ├── ray.rs
//! │   ├── extended_line.rs
//! │   ├── parallel_channel.rs
//! │   └── info_line.rs
//! │
//! ├── shapes/             # Geometric shapes
//! │   ├── mod.rs
//! │   ├── rectangle.rs
//! │   ├── ellipse.rs
//! │   ├── triangle.rs
//! │   ├── arc.rs
//! │   └── polyline.rs
//! │
//! ├── fibonacci/          # Fibonacci tools
//! │   ├── mod.rs
//! │   ├── retracement.rs
//! │   ├── extension.rs
//! │   ├── channel.rs
//! │   ├── circles.rs
//! │   ├── spiral.rs
//! │   ├── time_zones.rs
//! │   └── wedge.rs
//! │
//! ├── gann/               # Gann tools
//! │   ├── mod.rs
//! │   ├── fan.rs
//! │   ├── square.rs
//! │   └── box.rs
//! │
//! ├── patterns/           # Chart patterns
//! │   ├── mod.rs
//! │   ├── head_shoulders.rs
//! │   ├── elliott_wave.rs
//! │   ├── abcd.rs
//! │   ├── xabcd.rs
//! │   └── cypher.rs
//! │
//! ├── annotations/        # Text and markers
//! │   ├── mod.rs
//! │   ├── text.rs
//! │   ├── note.rs
//! │   ├── callout.rs
//! │   ├── price_label.rs
//! │   ├── arrow.rs
//! │   └── icon.rs
//! │
//! ├── measurement/        # Measuring tools
//! │   ├── mod.rs
//! │   ├── price_range.rs
//! │   ├── date_range.rs
//! │   ├── bars_pattern.rs
//! │   └── ruler.rs
//! │
//! └── trading/            # Trading-specific tools
//!     ├── mod.rs
//!     ├── position.rs
//!     ├── long_position.rs
//!     ├── short_position.rs
//!     └── risk_reward.rs
//! ```

mod traits;
mod types;
pub mod registry;
pub mod config;

/// Format a price value with smart precision for display in drawing primitives.
/// Handles everything from BTC ($70000) to PEPE ($0.000000003).
/// For prices below 1e-8, uses scientific notation (e.g. "1.23e-10").
pub fn fmt_price(price: f64) -> String {
    if price != 0.0 && price.abs() < 1e-8 {
        let exp = price.abs().log10().floor() as i32;
        let mantissa = price / 10f64.powi(exp);
        return format!("{:.2}e{}", mantissa, exp);
    }

    let precision = if price.abs() >= 10000.0 {
        2
    } else if price.abs() >= 1000.0 {
        2
    } else if price.abs() >= 100.0 {
        3
    } else if price.abs() >= 1.0 {
        4
    } else if price.abs() >= 0.01 {
        6
    } else {
        8
    };

    let formatted = format!("{:.prec$}", price, prec = precision);

    if formatted.contains('.') {
        let trimmed = formatted.trim_end_matches('0');
        let dot_pos = trimmed.find('.').unwrap();
        let decimals_len = trimmed.len() - dot_pos - 1;
        if decimals_len < 2 {
            format!("{:.2}", price)
        } else {
            trimmed.to_string()
        }
    } else {
        format!("{:.2}", price)
    }
}

// Category modules
pub mod lines;
pub mod channels;
pub mod shapes;
pub mod fibonacci;
pub mod pitchforks;
pub mod gann;
pub mod arrows;
pub mod annotations;
pub mod patterns;
pub mod elliott;
pub mod cycles;
pub mod projection;
pub mod volume;
pub mod measurement;
pub mod brushes;
pub mod icons;
pub mod signals;

// Re-export core types
pub use traits::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior,
    HitTestResult, ControlPointInfo, SyncMode,
    draw_control_points,
    PrimitiveExt,  // Extension trait with is_locked, set_locked, is_visible, set_visible
};
pub use types::{
    PrimitiveColor, PrimitiveText, TextAlign, ExtendMode, LineStyle,
    ControlPoint, ControlPointType, ControlPointCursor,
    HIT_TOLERANCE, CONTROL_POINT_RADIUS, CONTROL_POINT_HIT_RADIUS,
    CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    point_to_line_distance, TextAnchor, normalize_text_rotation,
    LineTextParams, calculate_line_text_params,
    LineSegment, line_segments_avoiding_text,
};
pub use registry::{PrimitiveRegistry, PrimitiveFactory, PrimitiveMetadata};
// Re-export render types from engine::render (unified RenderContext)
pub use crate::render::{
    RenderContext, RenderOp, RenderOps, TextAlign as RenderTextAlign, TextBaseline,
    crisp, crisp_rect, execute_ops,
    render_primitive_text, render_primitive_text_rotated, measure_primitive_text, render_text_with_background,
};
pub use config::{
    Configurable, ConfigProperty, PropertyType, PropertyValue, PropertyCategory,
    ContextMenuAction, PrimitiveFullConfig, FibLevelConfig, TimeframeVisibilityConfig,
    SelectOption, WaveDegree, LabelStyle,
};

// Re-export all primitives
pub use lines::*;
pub use channels::*;
pub use shapes::*;
pub use fibonacci::*;
pub use pitchforks::*;
pub use gann::*;
pub use arrows::*;
pub use annotations::*;
pub use patterns::*;
pub use elliott::*;
pub use cycles::*;
pub use projection::*;
pub use volume::*;
pub use measurement::*;
pub use brushes::*;
pub use icons::*;
pub use signals::{SystemSignal, SignalType, SignalPrimitive, StrategySignalConfig};
