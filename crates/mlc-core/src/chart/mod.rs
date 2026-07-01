//! Chart module - main canvas with viewport, candles, grid, scales, overlays
//!
//! This module consolidates chart-related functionality:
//!
//! - **types/**: Core data types (Viewport, PriceScale, TimeScale, Crosshair, Grid, etc.)
//! - **render_data/**: Pure-data render types (ChartRect, ChartTheme, ScaleConfig, etc.)
//! - **input/**: Input handling (pan, zoom, drag)
//!
//! Painter functions (draw_*) have moved to `mlc_app_layout::chart_render`.
//!
//! # Architecture
//!
//! ```text
//! chart/
//! ├── mod.rs              # This file
//! ├── types/              # Data structures
//! │   ├── viewport.rs     # Coordinate system, bar↔pixel conversion
//! │   ├── price_scale.rs  # Y-axis calculations
//! │   ├── time_scale.rs   # X-axis calculations
//! │   ├── kinetic.rs      # Scroll physics
//! │   ├── crosshair.rs    # Crosshair state
//! │   ├── grid.rs         # Grid options
//! │   ├── legend.rs       # Legend state
//! │   ├── tooltip.rs      # Tooltip state
//! │   └── watermark.rs    # Watermark config
//! └── input/              # Input handling (see engine/input)
//!     ├── pan_zoom.rs     # Pan, zoom logic
//!     ├── scale_drag.rs   # Scale dragging
//!     └── pane_resize.rs  # Pane separator drag
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use mlc_core::chart::{Viewport, PriceScale, TimeScale, Crosshair, GridOptions};
//!
//! // Create viewport
//! let viewport = Viewport::new(800.0, 600.0);
//! ```

pub mod annotations;
pub mod dom_heatmap;
pub mod liquidation_heatmap;
pub mod render_data;
pub mod series;
pub mod types;
pub mod volume_profile;

// Re-export all types for convenience
pub use types::*;

// Re-export pure-data render types from their canonical location
pub use render_data::{
    ChartRect, ChartBounds, ChartTheme, ChartRenderState,
    ScaleConfig, ScaleTheme,
    CrosshairConfig,
    LegendData, TooltipLines,
    RenderPriceLine, RenderMarker, RenderMarkerShape,
    PaneGeom, PaneTheme, HistogramStyle,
    GridRenderOptions, LineRenderStyle,
};

