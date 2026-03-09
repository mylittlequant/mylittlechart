//! Chart module - main canvas with viewport, candles, grid, scales, overlays
//!
//! This module consolidates chart-related functionality:
//!
//! - **types/**: Core data types (Viewport, PriceScale, TimeScale, Crosshair, Grid, etc.)
//! - **render/**: Platform-agnostic rendering functions
//! - **input/**: Input handling (pan, zoom, drag)
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
//! ├── render/             # Rendering functions
//! │   ├── grid.rs         # draw_grid()
//! │   ├── candles.rs      # draw_candles(), draw_bars()
//! │   ├── series.rs       # draw_line(), draw_area()
//! │   ├── scales.rs       # draw_price_scale(), draw_time_scale()
//! │   ├── crosshair.rs    # draw_crosshair()
//! │   ├── legend.rs       # draw_legend()
//! │   ├── panes.rs        # draw_sub_panes()
//! │   └── frame.rs        # render_chart_frame() - main entry
//! └── input/              # Input handling (see engine/input)
//!     ├── pan_zoom.rs     # Pan, zoom logic
//!     ├── scale_drag.rs   # Scale dragging
//!     └── pane_resize.rs  # Pane separator drag
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use zengeld_chart::chart::{
//!     Viewport, PriceScale, TimeScale, Crosshair, GridOptions,
//!     render_chart_frame,
//! };
//!
//! // Create viewport
//! let viewport = Viewport::new(800.0, 600.0);
//!
//! // Render frame
//! render_chart_frame(&mut ctx, &input, &state, &theme);
//! ```

pub mod annotations;
pub mod render;
pub mod series;
pub mod types;

// Re-export all types for convenience
pub use types::*;

// Re-export render types
pub use render::{
    render_chart_frame, ChartRenderState, ChartRect, ChartTheme,
    draw_grid, draw_styled_line, draw_dashed_line,
    draw_candles, draw_bars, draw_hollow_candles, draw_heikin_ashi,
    draw_line_series, draw_area_series, draw_histogram,
    draw_baseline_series, draw_step_line, draw_line_with_markers,
    draw_line_from_data, draw_compare_overlay,
    draw_price_scale, draw_time_scale, ScaleConfig, ScaleTheme,
    draw_crosshair, draw_pane_crosshair, CrosshairConfig,
    // Overlays
    draw_watermark, draw_legend, draw_tooltip, draw_price_lines, draw_markers,
    LegendData, TooltipLines, PriceLine, Marker, MarkerShape,
    // Panes
    draw_pane_separator, draw_pane_background, draw_pane_grid,
    draw_pane_line, draw_pane_histogram, draw_pane_price_scale,
    PaneGeom, PaneTheme, HistogramStyle,
    // Utils
    GridRenderOptions, LineRenderStyle,
};
