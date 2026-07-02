//! Per-instance Y-axis state for chart-container indicator sub-panes.
//!
//! Pure value type — no manager semantics.  Owned by `ChartContainer`
//! (`mlc-app`, keyed by indicator `instance_id`) but referenced by
//! `mlc-app-layout` render code below `mlc-app`, so it stays in
//! `mlc-core` rather than moving with `ChartContainer` in Step 4
//! (state lift, see `docs/mlc/plans/doctrine-cutover-2026-06-27.md`).

/// Per-instance Y-axis state for indicator sub-panes.
///
/// Replaces the Y-axis fields that used to live on `SubPane`
/// (`price_min`, `price_max`, `auto_scale`, drag start state).
/// Keyed by `instance_id` in `ChartContainer.sub_pane_y_state`.
#[derive(Clone, Debug)]
pub struct SubPaneYState {
    /// Current Y-axis bottom price.
    pub price_min: f64,
    /// Current Y-axis top price.
    pub price_max: f64,
    /// Whether auto-scale is enabled for this sub-pane.
    pub auto_scale: bool,
    /// Saved price_min at drag start (for y-axis drag operations).
    pub drag_start_price_min: f64,
    /// Saved price_max at drag start (for y-axis drag operations).
    pub drag_start_price_max: f64,
}

impl Default for SubPaneYState {
    fn default() -> Self {
        Self {
            price_min: 0.0,
            price_max: 100.0,
            auto_scale: true,
            drag_start_price_min: 0.0,
            drag_start_price_max: 0.0,
        }
    }
}
