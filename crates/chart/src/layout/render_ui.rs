//! Chart-specific UI rendering types.
//!
//! This module contains pure chart-level rendering helpers and data types
//! that describe how chart modal overlays are structured.
//!
//! ## What lives here
//!
//! - `IndicatorOverlayInfo` — lightweight description of an indicator for overlay rendering
//! - `toolbar_to_widget_theme` — converts ToolbarTheme + FrameTheme to WidgetTheme
//!
//! ## What stays in core
//!
//! Actual rendering functions (modal drawing, context menus, picker popups, etc.)
//! depend on core-level render infrastructure (`draw_input`, `SliderConfig`,
//! `ZLayer`, etc.) and therefore live in `zengeld-terminal-core::layout::render_chart_modals`.
//! Core re-exports those functions via `pub use`.

use crate::ui::toolbar_render::ToolbarTheme;
use crate::layout::render_chart::FrameTheme;
use crate::ui::widgets::types::WidgetTheme;

// =============================================================================
// Helper Functions
// =============================================================================

/// Convert ToolbarTheme to WidgetTheme for slider rendering.
pub fn toolbar_to_widget_theme(toolbar_theme: &ToolbarTheme, frame_theme: &FrameTheme) -> WidgetTheme {
    WidgetTheme {
        bg_normal: frame_theme.toolbar_bg.clone(),
        bg_hover: toolbar_theme.item_bg_hover.clone(),
        bg_pressed: toolbar_theme.item_bg_active.clone(),
        bg_disabled: toolbar_theme.separator.clone(),
        text_normal: toolbar_theme.item_text.clone(),
        text_hover: "#ffffff".to_string(),
        text_disabled: toolbar_theme.item_text_muted.clone(),
        border_normal: toolbar_theme.separator.clone(),
        border_hover: toolbar_theme.item_bg_hover.clone(),
        border_focused: toolbar_theme.item_bg_active.clone(),
        accent: toolbar_theme.item_bg_active.clone(),
        accent_hover: toolbar_theme.item_bg_active.clone(),
        success: "#10b981".to_string(),
        warning: "#f59e0b".to_string(),
        danger: "#ef4444".to_string(),
    }
}

// =============================================================================
// Indicator Overlay
// =============================================================================

/// Lightweight description of an indicator instance for overlay rendering.
///
/// Used by both the compact "button" overlay and the expanded indicator list
/// drawn in the chart's top-left corner.
#[derive(Clone, Debug)]
pub struct IndicatorOverlayInfo {
    /// Instance ID (unique per active indicator; 0 for compare series entries)
    pub id: u64,
    /// Display name with parameters (e.g. "RSI 14" or "MACD 12 26 9")
    pub display_name: String,
    /// Whether the indicator is currently visible
    pub visible: bool,
    /// True when this entry represents a compare symbol overlay (not an indicator)
    pub is_compare: bool,
    /// Symbol name for compare entries (e.g. "ETHUSDT"); None for regular indicators
    pub symbol: Option<String>,
    /// Line color for compare entries as a hex string (e.g. "#2196F3"); None for regular indicators
    pub color: Option<String>,
}
