//! Per-frame sub-pane overlay hit-test rect cache, stored on ChartContainer.

use crate::state::geometry::LayoutRect;

// =============================================================================
// Sub-Pane Overlay Render Result
// =============================================================================

/// Hit-test data for a single sub-pane overlay button bar.
/// Produced each frame by `render_sub_pane_overlay()` (right side) and
/// `render_sub_pane_left_overlay()` (left side).
#[derive(Clone, Debug, Default)]
pub struct SubPaneOverlayResult {
    /// Indicator instance ID this overlay belongs to.
    pub instance_id: u64,
    /// Rect for the delete button (right overlay).
    pub delete_rect: Option<LayoutRect>,
    /// Rect for the hide/show button (right overlay).
    pub hide_rect: Option<LayoutRect>,
    /// Rect for the move-up button (right overlay).
    pub move_up_rect: Option<LayoutRect>,
    /// Rect for the move-down button (right overlay).
    pub move_down_rect: Option<LayoutRect>,
    /// Rect for the expand button (shown when not maximized, right overlay).
    pub expand_rect: Option<LayoutRect>,
    /// Rect for the restore button (shown when maximized, right overlay).
    pub restore_rect: Option<LayoutRect>,
    // ── Left-side per-indicator buttons ──────────────────────────────────────
    /// Rect for the indicator eye (visibility toggle) button (left overlay).
    pub left_eye_rect: Option<LayoutRect>,
    /// Rect for the indicator alert button (left overlay).
    pub left_alert_rect: Option<LayoutRect>,
    /// Rect for the indicator settings button (left overlay).
    pub left_settings_rect: Option<LayoutRect>,
    /// Rect for the indicator delete button (left overlay).
    pub left_delete_rect: Option<LayoutRect>,
}
