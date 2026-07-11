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
use mlc_core::preset::snapshots::{
    ChartWindowSnapshot, DrawingSnapshot, PriceScalePersist, PrimitiveSnapshot, ViewportPersist,
};
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
/// `ChartContainer`). `chart_type`/`chart_type_param`/`chart_type_interval`
/// are likewise read from the bubble by the caller (chart_type moved off
/// `ChartPanelState` onto `DataBubble`, symmetric with `timeframe`).
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
    chart_type: String,
    chart_type_param: f64,
    chart_type_interval: String,
) -> ChartWindowSnapshot {
    let drawings = DrawingSnapshot::from_primitives(window.id.0, primitives);

    // Persist only the zoom level (bar_spacing) and price-scale display
    // config. `view_start`, `chart_width`/`chart_height`, `bar_count`,
    // `price_min`/`price_max`, and `scale_mode` are runtime-only —
    // never serialized (viewport SWC arc, 2026-07-03).
    let saved_viewport = ViewportPersist {
        bar_spacing: window.viewport().bar_spacing,
    };

    let saved_price_scale = PriceScalePersist {
        mode: window.price_scale().mode,
        base_price: window.price_scale().base_price,
        width: window.price_scale().width,
        user_precision: window.price_scale().user_precision,
    };

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
        chart_type,
        chart_type_param,
        chart_type_interval,
        footprint_display: panel.footprint_display,
        tick_intensity: panel.tick_intensity,
        bar_family_wicks: panel.bar_family_wicks,
        tpo_display: panel.tpo_display,
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

#[cfg(test)]
mod json_compat_tests {
    //! Persistence fixture gate for the Step 4 state-lift move: this
    //! constructor moved from `mlc-core::preset::snapshots` to
    //! `mlc-app::persistence::window_snapshot`, but `ChartWindowSnapshot`'s
    //! on-disk JSON shape must be byte-identical — the field definitions
    //! never moved, only the builder function's crate.
    use super::*;
    use mlc_core::ids::ChartPanelId;
    use mlc_core::state::Timeframe;

    #[test]
    fn round_trips_current_shape() {
        let window = ChartContainer::new("BTCUSDT", Timeframe::h1());
        let mut panel = ChartPanelState::new(ChartPanelId(1));
        panel.footprint_display.show_abs_markers = false;
        panel.footprint_display.show_volume_text = false;
        panel.footprint_display.heat_intensity = mlc_core::chart::FootprintHeatIntensity::High;
        panel.footprint_display.imbalance_threshold = 3.0;
        panel.tick_intensity = true;
        panel.tpo_display.show_session_dates = true;
        panel.tpo_display.show_poc = false;
        panel.tpo_display.ib_periods = 4;

        let snap = build_chart_window_snapshot(
            &window,
            &panel,
            &[],
            /* leaf_id */ 7,
            /* bubble_id */ Some(42),
            /* pre_tag_indicator_ids */ vec![1, 2, 3],
            /* stashed_command_history */ None,
            /* stashed_primitives_snap */ vec![],
            "BTCUSDT".to_string(),
            Timeframe::h1(),
            "binance".to_string(),
            "S".to_string(),
            "candles".to_string(),
            25.5,
            "5m".to_string(),
        );

        let json = serde_json::to_string(&snap).expect("snapshot must serialize");

        // Spot-check the flat on-disk field names a preset-loading consumer
        // depends on — regressions here mean old preset files stop loading.
        assert!(json.contains("\"window_id\""));
        assert!(json.contains("\"leaf_id\":7"));
        assert!(json.contains("\"symbol\":\"BTCUSDT\""));
        assert!(json.contains("\"exchange\":\"binance\""));
        assert!(json.contains("\"account_type\":\"S\""));
        assert!(json.contains("\"bubble_id\":42"));
        assert!(json.contains("\"chart_type\":\"candles\""));
        assert!(json.contains("\"chart_type_param\":25.5"));
        assert!(json.contains("\"chart_type_interval\":\"5m\""));
        assert!(json.contains("\"footprint_display\""));
        assert!(json.contains("\"tick_intensity\":true"));
        assert!(json.contains("\"tpo_display\""));
        assert!(json.contains("\"show_candles\":true"));
        assert!(json.contains("\"pre_tag_indicator_ids\":[1,2,3]"));

        let restored: mlc_core::preset::snapshots::ChartWindowSnapshot =
            serde_json::from_str(&json).expect("snapshot must deserialize");
        assert_eq!(restored.window_id, snap.window_id);
        assert_eq!(restored.leaf_id, 7);
        assert_eq!(restored.symbol, "BTCUSDT");
        assert_eq!(restored.bubble_id, Some(42));
        assert_eq!(restored.pre_tag_indicator_ids, vec![1, 2, 3]);
        assert_eq!(restored.chart_type_param, 25.5);
        assert_eq!(restored.chart_type_interval, "5m");
        assert!(!restored.footprint_display.show_abs_markers);
        assert!(!restored.footprint_display.show_volume_text);
        assert_eq!(restored.footprint_display.heat_intensity, mlc_core::chart::FootprintHeatIntensity::High);
        assert_eq!(restored.footprint_display.imbalance_threshold, 3.0);
        assert!(restored.tick_intensity);
        assert!(restored.tpo_display.show_session_dates);
        assert!(!restored.tpo_display.show_poc);
        assert_eq!(restored.tpo_display.ib_periods, 4);
    }

    /// Old preset JSON (pre-2026-07-05, no `chart_type_param`/
    /// `chart_type_interval`/`footprint_display` keys at all) must still
    /// deserialize — the new fields fall back to their `#[serde(default)]`
    /// sentinels instead of failing the whole preset load.
    #[test]
    fn old_snapshot_without_chart_type_param_fields_still_loads() {
        let window = ChartContainer::new("ETHUSDT", Timeframe::h1());
        let panel = ChartPanelState::new(ChartPanelId(2));

        let snap = build_chart_window_snapshot(
            &window,
            &panel,
            &[],
            9,
            None,
            vec![],
            None,
            vec![],
            "ETHUSDT".to_string(),
            Timeframe::h1(),
            "binance".to_string(),
            "S".to_string(),
            "candles".to_string(),
            0.0,
            "1m".to_string(),
        );

        let mut json_value = serde_json::to_value(&snap).expect("snapshot must serialize to value");
        let obj = json_value.as_object_mut().expect("snapshot serializes to a JSON object");
        obj.remove("chart_type_param");
        obj.remove("chart_type_interval");
        obj.remove("footprint_display");
        obj.remove("tick_intensity");
        obj.remove("tpo_display");
        let old_json = serde_json::to_string(&json_value).expect("stripped snapshot must serialize");

        let restored: mlc_core::preset::snapshots::ChartWindowSnapshot =
            serde_json::from_str(&old_json).expect("old-shape snapshot (missing new fields) must still deserialize");
        assert_eq!(restored.chart_type_param, 0.0);
        assert_eq!(restored.chart_type_interval, "1m");
        assert_eq!(restored.footprint_display, mlc_core::chart::render_data::FootprintDisplayOptions::default());
        assert!(!restored.tick_intensity);
        assert_eq!(restored.tpo_display, mlc_core::chart::render_data::TpoDisplayOptions::default());
        // Pixel-identical-to-pre-toggle sentinels: session-date header off,
        // everything else on, IB periods = 2 (canon A/B convention).
        assert!(!restored.tpo_display.show_session_dates);
        assert!(restored.tpo_display.show_poc);
        assert!(restored.tpo_display.show_value_area);
        assert!(restored.tpo_display.show_single_prints);
        assert!(restored.tpo_display.show_poor_hl);
        assert!(restored.tpo_display.show_initial_balance);
        assert_eq!(restored.tpo_display.ib_periods, 2);
    }
}
