//! `ChartWindowSnapshot` constructor — reads a live `ChartContainer` +
//! `ChartPanelState` (both `mlc-app`-owned since Step 4, the state lift)
//! and builds the serializable snapshot type.
//!
//! Lives in `mlc-app` (not `mlc-core::preset::snapshots`, where
//! `ChartWindowSnapshot` itself is still defined) because it needs
//! `ChartContainer`/`ChartPanelState` in scope — the same "Scenario B"
//! split doctrine used for `build_chart_render_state` /
//! `DataBubbleSnapshot::from_group` in Step 2a.  `ChartWindowSnapshot`'s
//! field definitions stay in `mlc-core::preset::snapshots` because
//! `ChartPreset.windows: Vec<ChartWindowSnapshot>` needs no
//! `ChartContainer` reference — only this constructor does.

use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::preset::snapshots::{ChartWindowSnapshot, DrawingSnapshot, PrimitiveSnapshot};
use mlc_core::state::{CommandHistory, Timeframe};
use crate::chart_container::ChartContainer;
use crate::chart_panel_state::ChartPanelState;

/// Capture a full snapshot from a live [`ChartContainer`].
///
/// `leaf_id` is the docking-tree leaf slot this window occupies; it is
/// stored so that presets can restore windows to the correct panel slot.
///
/// `symbol`, `timeframe`, `exchange`, and `account_type` are read from
/// `branch_meta` by the caller (Phase 4 B — instrument fields removed from
/// `ChartContainer`).
#[allow(clippy::too_many_arguments)]
pub fn build_chart_window_snapshot(
    window: &ChartContainer,
    panel: &ChartPanelState,
    primitives: &[Box<dyn Primitive>],
    leaf_id: u64,
    bubble_id: Option<u64>,
    pre_tag_indicator_ids: Vec<u64>,
    stashed_command_history: Option<CommandHistory>,
    stashed_primitives_snap: Vec<PrimitiveSnapshot>,
    symbol: String,
    timeframe: Timeframe,
    exchange: String,
    account_type: String,
) -> ChartWindowSnapshot {
    let drawings = DrawingSnapshot::from_primitives(window.id.0, primitives);

    // Save full viewport — chart_width, bar_spacing, bar_count are all valid
    // and needed for immediate snap in set_bars(). If app window was resized,
    // sync_viewport_from_layout will proportionally adjust chart_width.
    // Only zero view_start — snap-to-end always positions to latest bar.
    let mut saved_viewport = window.viewport.clone();
    saved_viewport.view_start = 0.0;

    let mut saved_price_scale = window.price_scale.clone();
    // price_min/price_max are recalculated from bars; zero them out.
    saved_price_scale.price_min = 0.0;
    saved_price_scale.price_max = 0.0;

    ChartWindowSnapshot {
        window_id: window.id.0,
        leaf_id,
        symbol,
        exchange,
        account_type,
        timeframe,
        viewport: saved_viewport,
        price_scale: saved_price_scale,
        bubble_id,
        grid_options: panel.grid_options.clone(),
        crosshair_options: panel.crosshair_options.clone(),
        legend: panel.legend.clone(),
        watermark: panel.watermark.clone(),
        tooltip: panel.tooltip.clone(),
        chart_type: panel.chart_type.to_string(),
        show_candles: panel.show_candles,
        show_bars: panel.show_bars,
        show_hollow_candles: panel.show_hollow_candles,
        show_heikin_ashi: panel.show_heikin_ashi,
        show_line: panel.show_line,
        show_step_line: panel.show_step_line,
        show_line_markers: panel.show_line_markers,
        show_area: panel.show_area,
        show_hlc_area: panel.show_hlc_area,
        show_histogram: panel.show_histogram,
        show_columns: panel.show_columns,
        show_baseline: panel.show_baseline,
        scale_settings: window.scale_settings.clone(),
        drawings,
        command_history: Some(window.command_history.clone()),
        stashed_command_history,
        compare_overlay: window.compare_overlay.clone(),
        symbol_drawings_snapshots: window.symbol_drawings.clone(),
        bars: Vec::new(),
        stashed_primitives: stashed_primitives_snap,
        pre_tag_indicator_ids,
        // Collect sub-pane order, height ratios, and hidden set from
        // the inner docking tree (single source of truth after S4).
        sub_pane_height_ratios: {
            let proportions = &window.inner_docking.tree().root().proportions;
            let instance_ids = crate::chart_container::collect_inner_instance_ids(
                &window.inner_docking,
            );
            instance_ids
                .iter()
                .enumerate()
                .filter_map(|(i, &iid)| {
                    let prop_idx = i + 1;
                    let ratio = proportions.get(prop_idx).copied().unwrap_or(0.0) as f32;
                    if ratio > 0.0 { Some((iid, ratio)) } else { None }
                })
                .collect()
        },
        // Emit the runtime above-main set so presets round-trip correctly.
        sub_pane_above_main: window.sub_pane_above_main.clone(),
        sub_pane_order: crate::chart_container::collect_inner_instance_ids(
            &window.inner_docking,
        ),
        sub_pane_hidden: crate::chart_container::collect_hidden_instance_ids(
            &window.inner_docking,
        ),
        stashed_scale_mode: window.stashed_scale_mode,
    }
}
