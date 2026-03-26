//! Indicator source trait — abstraction for indicator data access
//!
//! Allows chart rendering functions to query indicator information without
//! depending on the full IndicatorManager from zengeld-terminal-core.
//!
//! Core implements `IndicatorSource` for its `IndicatorManager` by converting
//! internal types to these chart-owned render types.

use std::collections::HashMap;
use std::sync::Arc;
use crate::ui::modal_settings::IndicatorDisplayInfo;
use crate::drawing::TimeframeVisibilityConfig;

// =============================================================================
// Minimal indicator info (used by ChartWindow for sub-pane layout)
// =============================================================================

/// Minimal indicator info needed by chart window for layout purposes
pub struct IndicatorInfo {
    /// Unique indicator instance identifier
    pub id: u64,
    /// Display name of the indicator
    pub name: String,
    /// Pane index: 0 = main pane, 1+ = sub-pane
    pub pane_index: usize,
    /// Whether this indicator instance is visible
    pub visible: bool,
}

// =============================================================================
// Rich render types (used by indicator/alert rendering functions in chart)
// =============================================================================

/// How histogram bars are rendered (mirrors core's HistogramStyle)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HistogramStyle {
    /// Bars grow from bottom (default for volume)
    #[default]
    FromBottom,
    /// Bars centered on zero line (MACD style)
    Centered,
}

/// Indicator output type for rendering
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndicatorOutputRenderType {
    Line,
    Histogram,
    Band,
    Area,
    Dots,
    Background,
}

/// Per-output configuration override (visibility, color, line width)
#[derive(Clone, Debug, Default)]
pub struct OutputRenderConfig {
    pub visible: bool,
    pub color: Option<String>,
    pub line_width: Option<f32>,
}

/// Output definition for rendering (name, type, default color/width)
#[derive(Clone, Debug)]
pub struct IndicatorOutputRenderDef {
    pub name: String,
    pub output_type: IndicatorOutputRenderType,
    /// Default color (may be overridden by OutputRenderConfig)
    pub color: String,
    pub line_width: f32,
}

/// Signal data for rendering on chart
#[derive(Clone, Debug)]
pub struct SignalRenderData {
    /// Bar index where the signal occurred
    pub bar_index: usize,
    /// Direction: >0 = bullish, <0 = bearish, 0 = neutral
    pub direction: i32,
    /// Price at which the signal occurred
    pub price: f64,
}

/// Full render data for one indicator instance
///
/// This is a flattened, chart-friendly view of an `IndicatorInstance`.
/// Core fills this in `get_render_instances_for_symbol`.
#[derive(Clone, Debug)]
pub struct IndicatorRenderInstance {
    /// Unique instance ID
    pub id: u64,
    /// Type ID string (e.g., "bb", "rsi")
    pub type_id: String,
    /// Pane index (0 = overlay on main chart, 1+ = sub-pane)
    pub pane: usize,
    /// Whether this instance is currently visible
    pub visible: bool,
    /// Formatted display title (e.g., "SMA(20)")
    pub title: String,
    /// Output definitions (in order)
    pub output_defs: Vec<IndicatorOutputRenderDef>,
    /// Per-output instance config overrides (keyed by output name)
    pub output_configs: HashMap<String, OutputRenderConfig>,
    /// Computed values per output (keyed by output name).
    /// Wrapped in `Arc` so that cloning a render instance does not deep-clone
    /// the value buffers — the reference count is bumped instead (O(1)).
    pub values: Arc<HashMap<String, Vec<f64>>>,
    /// Histogram style (for histogram outputs)
    pub histogram_style: HistogramStyle,
    /// Signal events to render
    pub signals: Vec<SignalRenderData>,
    /// Whether signal rendering is enabled
    pub signals_enabled: bool,
    /// Extra params needed by rendering (e.g. BB fill color, volume colors)
    /// Uses simple key->string format for color params
    pub color_params: HashMap<String, String>,
    /// Extra params needed by rendering (boolean params, e.g. color_by_direction)
    pub bool_params: HashMap<String, bool>,
    /// Optional timeframe visibility configuration — when set, the instance is
    /// only rendered on timeframes that pass `is_visible_on_label`.
    pub timeframe_visibility: Option<TimeframeVisibilityConfig>,
}

/// Alert status for rendering
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertRenderStatus {
    Active,
    Triggered,
    Paused,
    Expired,
}

/// Minimal alert data needed for chart rendering
#[derive(Clone, Debug)]
pub struct AlertRenderData {
    /// Alert price level
    pub price: f64,
    /// Alert status
    pub status: AlertRenderStatus,
}

// =============================================================================
// Settings-level data (used by the indicator settings modal)
// =============================================================================

/// Settings-level data for one indicator instance, for the settings modal.
///
/// This is a flattened, chart-friendly view produced by `IndicatorSource::get_settings_data`.
/// The indicators crate fills this in its impl of `IndicatorSource`.
#[derive(Clone, Debug)]
pub struct IndicatorSettingsData {
    /// Display name of the indicator instance
    pub name: String,
    /// Parameters as ordered (name, value_string) pairs
    pub params: Vec<(String, String)>,
    /// Outputs as ordered (name, color_string) pairs
    pub outputs: Vec<(String, String)>,
    /// Optional display info (metadata) for the Info tab
    pub display_info: Option<IndicatorDisplayInfo>,
    /// Whether signal generation is enabled for this instance
    pub signals_enabled: bool,
    /// Optional timeframe visibility configuration
    pub timeframe_visibility: Option<TimeframeVisibilityConfig>,
}

// =============================================================================
// IndicatorSource trait
// =============================================================================

/// Trait for providing indicator data to chart rendering functions
pub trait IndicatorSource {
    // --- Layout-level methods (used by ChartWindow for sub-pane layout) ---

    /// Get minimal indicator info for all instances assigned to a symbol.
    /// Used by layout engine to determine sub-pane count and order.
    fn get_instances_for_symbol(&self, symbol: &str) -> Vec<IndicatorInfo>;

    /// Calculate the value range for an indicator in the visible bar range.
    /// Returns `None` if no data is available.
    fn calculate_pane_range(
        &self,
        instance_id: u64,
        visible_start: usize,
        visible_end: usize,
    ) -> Option<(f64, f64)>;

    // --- Render-level methods (used by indicator rendering functions) ---

    /// Get full render data for all instances assigned to a symbol.
    ///
    /// Used by `draw_overlay_indicators` and `render_sub_pane`.
    fn get_render_instances_for_symbol(&self, symbol: &str) -> Vec<IndicatorRenderInstance>;

    /// Get full render data for a single indicator instance by ID.
    ///
    /// Used by `render_sub_pane` to look up the indicator for a given sub-pane.
    fn get_render_instance(&self, instance_id: u64) -> Option<IndicatorRenderInstance>;

    /// Return the histogram style for the given indicator instance.
    ///
    /// Used by `update_sub_pane_ranges` to match the symmetrize logic in the
    /// render path.  The default implementation delegates to `get_render_instance`
    /// so existing impls that don't override this get correct behavior for free.
    fn histogram_style_for(&self, instance_id: u64) -> HistogramStyle {
        self.get_render_instance(instance_id)
            .map(|i| i.histogram_style)
            .unwrap_or_default()
    }

    // --- Settings-level methods (used by the indicator settings modal) ---

    /// Get settings data for a single indicator instance by ID.
    ///
    /// Returns all data required by `render_indicator_settings_modal`:
    /// - Ordered params as `(name, value_string)` pairs
    /// - Ordered outputs as `(name, color_string)` pairs
    /// - Optional `IndicatorDisplayInfo` for the Info/metadata tab
    /// - `signals_enabled` flag
    /// - Optional `TimeframeVisibilityConfig`
    ///
    /// Default implementation returns `None` (standalone/null mode).
    fn get_settings_data(&self, _instance_id: u64) -> Option<IndicatorSettingsData> {
        None
    }
}

// =============================================================================
// Null implementation
// =============================================================================

/// Null implementation for charts without indicators
pub struct NullIndicatorSource;

impl IndicatorSource for NullIndicatorSource {
    fn get_instances_for_symbol(&self, _symbol: &str) -> Vec<IndicatorInfo> {
        Vec::new()
    }

    fn calculate_pane_range(
        &self,
        _id: u64,
        _start: usize,
        _end: usize,
    ) -> Option<(f64, f64)> {
        None
    }

    fn get_render_instances_for_symbol(&self, _symbol: &str) -> Vec<IndicatorRenderInstance> {
        Vec::new()
    }

    fn get_render_instance(&self, _instance_id: u64) -> Option<IndicatorRenderInstance> {
        None
    }
}
