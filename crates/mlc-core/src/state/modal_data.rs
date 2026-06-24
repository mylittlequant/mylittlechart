//! Pure-data types cut from `mlc-layout/src/ui/modal_settings.rs`.
//!
//! These types are consumed by the backend (state/, engine/, indicator_source,
//! tag_manager) and must therefore live in mlc-core, not in mlc-layout.
//! mlc-layout re-exports them via shims in ui/modal_settings.rs.

// =============================================================================
// Sub-Pane Overlay State
// =============================================================================

/// Which button on the sub-pane overlay bar is being referenced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SubPaneButton {
    /// Delete the indicator (and its sub-pane).
    Delete,
    /// Hide / unhide the sub-pane (indicator still exists).
    Hide,
    /// Move this sub-pane up in the ordering.
    MoveUp,
    /// Expand this sub-pane to fill the entire chart area.
    Expand,
    /// Shown instead of Expand when the pane is already maximized — restores it.
    Restore,
    /// Move this sub-pane down in the ordering.
    MoveDown,
    // ── Left-side per-indicator buttons ──────────────────────────────────────
    /// Toggle indicator visibility (left overlay).
    IndicatorEye,
    /// Open alert settings for this indicator (left overlay).
    IndicatorAlert,
    /// Open indicator settings modal (left overlay).
    IndicatorSettings,
    /// Delete this indicator (left overlay).
    IndicatorDelete,
}

/// Per-sub-pane overlay button UI state (hover triggered visibility).
///
/// One instance lives in `ChartApp` keyed by `instance_id`.
#[derive(Clone, Debug, Default)]
pub struct SubPaneOverlayState {
    /// Whether the button bar is visible (mouse inside sub-pane content area).
    pub visible: bool,
    /// Which right-side button (if any) is currently hovered.
    pub hovered_button: Option<SubPaneButton>,
    /// Which left-side per-indicator button (if any) is currently hovered.
    pub hovered_left_button: Option<SubPaneButton>,
}

// =============================================================================
// Indicator display info (for indicator settings modal)
// =============================================================================

/// Parameter type for indicator settings rendering
#[derive(Clone, Debug, PartialEq)]
pub enum IndicatorParamType {
    Int,
    Float,
    Bool,
    Source,
    Select { options: Vec<String> },
    Color,
}

/// A single parameter definition for display in the indicator settings modal
#[derive(Clone, Debug)]
pub struct IndicatorParamDef {
    pub name: String,
    pub param_type: IndicatorParamType,
}

impl IndicatorParamDef {
    /// Get options as strings (for Select type)
    pub fn get_options_as_strings(&self) -> Vec<String> {
        match &self.param_type {
            IndicatorParamType::Select { options } => options.clone(),
            _ => Vec::new(),
        }
    }
}

/// Output type for display
#[derive(Clone, Debug)]
pub enum IndicatorOutputType {
    Line,
    Histogram,
    Shapes,
    Arrows,
    Other(String),
}

impl IndicatorOutputType {
    pub fn as_str(&self) -> &str {
        match self {
            IndicatorOutputType::Line => "Line",
            IndicatorOutputType::Histogram => "Histogram",
            IndicatorOutputType::Shapes => "Shapes",
            IndicatorOutputType::Arrows => "Arrows",
            IndicatorOutputType::Other(s) => s.as_str(),
        }
    }
}

/// An output definition for the indicator
#[derive(Clone, Debug)]
pub struct IndicatorOutputDef {
    pub display_name: String,
    pub output_type: IndicatorOutputType,
}

/// Display-only indicator info used in `render_indicator_settings_modal`.
///
/// Core converts `IndicatorDefinition` → `IndicatorDisplayInfo` before calling
/// chart's renderer, avoiding a circular dependency (indicators → chart).
#[derive(Clone, Debug, Default)]
pub struct IndicatorDisplayInfo {
    pub name: String,
    pub short_name: String,
    pub description: String,
    pub overlay: bool,
    pub bounds: Option<(f64, f64)>,
    pub category_name: String,
    pub params: Vec<IndicatorParamDef>,
    pub outputs: Vec<IndicatorOutputDef>,
}
