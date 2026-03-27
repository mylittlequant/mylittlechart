//! Layout computation and chart rendering
//!
//! This module provides platform-agnostic layout computation and chart rendering.
//!
//! # Key Types
//!
//! - [`Margins`] - Space consumed by external UI elements
//! - [`FrameLayout`] - Computed layout for a chart frame
//! - [`ChartAreaLayout`] - Subdivision of chart into candle area and scales
//! - [`ExtendedFrameLayout`] - Layout with sub-panes for indicators
//!
//! # Usage
//!
//! ```ignore
//! use zengeld_chart::layout::{Margins, FrameLayout};
//!
//! // Terminal computes margins from its UI state
//! let margins = Margins::new(40.0, 50.0, 60.0, 30.0);
//!
//! // Chart computes its internal layout
//! let layout = FrameLayout::compute(window_width, window_height, &margins);
//!
//! // Use layout.chart_area for rendering
//! render_candles(ctx, &layout.chart_area.chart, ...);
//! ```

mod rects;
mod compute;
mod render_chart;
mod hit_tester;
pub mod toolbar_state;
pub mod render_frame;
pub mod render_ui;
pub mod render_chart_modals;
pub mod modals;
pub mod panel_overlay;

pub use rects::*;
pub use compute::*;
pub use render_chart::*;
pub use hit_tester::*;
pub use toolbar_state::{
    ToolbarState, ToolbarClickResult, ToggleIconPair,
};
pub use render_ui::{IndicatorOverlayInfo, toolbar_to_widget_theme};
pub use render_frame::{
    ContextMenuResult,
    ColorPickerRenderResult,
    InlineConfigResult,
    SliderTrackInfo,
    PrimitiveSettingsResult,
    PanelTreeManagerResult,
    ModalSearchResult,
    RightSidebarResult,
    ChartSettingsModalResult,
    IndicatorSettingsModalResult,
    IndicatorRowResult,
    IndicatorOverlayResult,
    SingleChartPanelResult,
    MultiChartRenderResult,
    ChartModalLayout,
    ChartModalRenderResult,
    SubPaneOverlayResult,
};
