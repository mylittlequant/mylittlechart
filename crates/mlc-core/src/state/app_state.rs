//! Application State Interface for toolbar rendering
//!
//! Provides traits and types for checking application state
//! when rendering toolbars (active buttons, selected tools, etc.)

use crate::engine::input::ChartAction;
use crate::state::selected_config::SelectedPrimitiveConfig;

// =============================================================================
// Window Layout Enum
// =============================================================================

/// Layout mode for multiple windows
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum WindowLayout {
    /// Single window (tabbed)
    #[default]
    Single,
    /// Two windows side by side
    SplitHorizontal,
    /// Two windows stacked
    SplitVertical,
    /// Four windows in grid
    Grid2x2,
    /// 2 stacked on left, 1 big on right
    TwoLeftOneRight,
    /// 1 big on left, 2 stacked on right
    OneLeftTwoRight,
    /// 2 side by side on top, 1 big on bottom
    TwoTopOneBottom,
    /// 1 big on top, 2 side by side on bottom
    OneTopTwoBottom,
    /// 3 vertical columns
    ThreeColumns,
    /// 3 horizontal rows
    ThreeRows,
    /// 1 big on left, 3 small stacked on right
    OneBigThreeSmall,
    /// Custom layout
    Custom,
}

// =============================================================================
// Window Sync Mode Enum
// =============================================================================

/// Sync mode between chart windows
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WindowSyncMode {
    /// No synchronization
    #[default]
    None,
    /// Sync symbol changes
    Symbol,
    /// Sync timeframe changes
    Timeframe,
    /// Sync crosshair position
    Crosshair,
    /// Sync viewport (pan/zoom)
    Viewport,
    /// Sync all of the above
    All,
}

// =============================================================================
// Application State Trait
// =============================================================================

/// State that the renderer needs to check for active buttons
pub trait AppState {
    fn is_candles_visible(&self) -> bool;
    fn is_line_visible(&self) -> bool;
    fn is_area_visible(&self) -> bool;
    fn is_histogram_visible(&self) -> bool;
    fn is_baseline_visible(&self) -> bool;
    fn is_legend_visible(&self) -> bool;
    fn is_tooltip_visible(&self) -> bool;
    fn is_watermark_visible(&self) -> bool;
    fn is_grid_visible(&self) -> bool;
    fn is_magnet_enabled(&self) -> bool;
    fn is_crosshair_visible(&self) -> bool;
    fn is_drawings_locked(&self) -> bool;
    fn is_drawings_visible(&self) -> bool;
    fn selected_tool(&self) -> &str;
    fn bar_count(&self) -> usize;

    /// Get selected primitive config (None if no primitive selected)
    fn selected_primitive_config(&self) -> Option<SelectedPrimitiveConfig> {
        None // Default implementation returns None
    }

    /// Get current symbol ticker (for dynamic toolbar display)
    fn current_symbol(&self) -> &str {
        "BTCUSDT" // Default fallback
    }

    /// Get current timeframe label (for dynamic toolbar display)
    fn current_timeframe(&self) -> &str {
        "1H" // Default fallback
    }

    /// Get current window layout
    fn window_layout(&self) -> WindowLayout {
        WindowLayout::Single // Default
    }

    /// Get last used multi-window layout (for restore after single)
    fn last_multi_layout(&self) -> WindowLayout {
        WindowLayout::SplitHorizontal // Default
    }

    /// Get window sync state as tuple (sync_symbol, sync_timeframe, sync_crosshair, sync_viewport)
    fn window_sync(&self) -> (bool, bool, bool, bool) {
        (false, false, false, false) // Default: no sync
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Check if an action is currently "active" based on app state
pub fn is_action_active(state: &dyn AppState, action: &ChartAction) -> bool {
    match action {
        ChartAction::ToggleCandles => state.is_candles_visible(),
        ChartAction::ToggleLine => state.is_line_visible(),
        ChartAction::ToggleArea => state.is_area_visible(),
        ChartAction::ToggleHistogram => state.is_histogram_visible(),
        ChartAction::ToggleBaseline => state.is_baseline_visible(),
        ChartAction::ToggleLegend => state.is_legend_visible(),
        ChartAction::ToggleTooltip => state.is_tooltip_visible(),
        ChartAction::ToggleWatermark => state.is_watermark_visible(),
        ChartAction::ToggleGrid => state.is_grid_visible(),
        ChartAction::ToggleMagnet => state.is_magnet_enabled(),
        ChartAction::ToggleCrosshair => state.is_crosshair_visible(),
        ChartAction::ToggleLockDrawings => state.is_drawings_locked(),
        ChartAction::ToggleDrawingsVisible => !state.is_drawings_visible(), // Active when hidden
        ChartAction::SelectTool(tool) => state.selected_tool() == *tool,
        _ => false,
    }
}
