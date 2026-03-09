//! Chart type definitions
//!
//! This module contains chart-related types that were previously in:
//! - `engine/` → viewport, price_scale, time_scale, kinetic
//! - `overlays/` → crosshair, grid, legend, tooltip, watermark, compare
//!
//! # Architecture
//!
//! ```text
//! chart/types/
//! ├── mod.rs              # This file
//! ├── viewport.rs         # Coordinate system, bar↔pixel conversion
//! ├── price_scale.rs      # Y-axis calculations
//! ├── time_scale.rs       # X-axis calculations
//! ├── kinetic.rs          # Scroll physics
//! ├── crosshair.rs        # Crosshair state
//! ├── grid.rs             # Grid options
//! ├── legend.rs           # Legend state
//! ├── tooltip.rs          # Tooltip state
//! ├── watermark.rs        # Watermark config
//! └── compare.rs          # Symbol comparison overlay
//! ```

// Coordinate system and scales
pub mod viewport;
pub mod price_scale;
pub mod time_scale;
pub mod kinetic;

// Overlays
pub mod crosshair;
pub mod grid;
pub mod legend;
pub mod tooltip;
pub mod watermark;
pub mod compare;

// Re-export viewport
pub use viewport::Viewport;

// Re-export price scale
pub use price_scale::{
    format_price, format_price_with_precision, nice_number, nice_price_step, price_precision,
    PriceScale, PriceScaleMode, ScaleMode, NICE_MULTIPLIERS,
};

// Re-export time scale
pub use time_scale::{
    format_time_by_weight, format_time_full, format_time_by_weight_with_settings,
    format_time_full_with_settings, TickMarkWeight, TimeScale, TimeTick, DAY, HOUR,
    MINUTE,
};

// Re-export kinetic
pub use kinetic::{KineticState, KINETIC_DAMPING, KINETIC_FRICTION, KINETIC_MIN_VELOCITY};

// Re-export crosshair
pub use crosshair::{Crosshair, CrosshairLineOptions, CrosshairMode, CrosshairOptions};

// Re-export grid
pub use grid::{GridLineOptions, GridOptions};

// Re-export legend
pub use legend::{Legend, LegendData, LegendPosition};

// Re-export tooltip
pub use tooltip::{Tooltip, TooltipContent};

// Re-export watermark
pub use watermark::{FontStyle, HorzAlign, VertAlign, Watermark, WatermarkLine};

// Re-export compare
pub use compare::{get_compare_color, CompareOverlay, CompareSeries, COMPARE_COLORS};

// Re-export pane types from state/
pub use crate::state::{
    InteractionRegion, Pane, PaneGeometry, PaneId, PaneManager, SubPane, MAIN_PANE,
};
