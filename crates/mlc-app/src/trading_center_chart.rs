//! Embedded chart in the trading container's CENTER region
//! (chart-in-container arc, 2026-07-26).
//!
//! The aux `ChartContainer` lives IN `panel_grid.containers` — every
//! reactive sweep (react.rs BarsLoaded reconcile, tick.rs scroll,
//! prepare.rs deferred snap / y-rescale), park/unpark bubble collection,
//! and drawing-points caches iterate the MAP and key off the chart's
//! bubble, so the aux chart participates for free. It has NO docking leaf
//! and NO `leaf_to_chart` entry: the grid never lays it out, the preset
//! snapshot (`iter_containers`, leaf-pair driven) never captures it, and
//! the split render loop never draws it. Identity is RAM-only — re-derived
//! every session from the trading container's symbol key
//! (symbol/exchange/account_type/timeframe), which is what persists.
//!
//! Lifecycle: [`ChartApp::ensure_trading_center_charts`] runs once per
//! tick (next to `poll_trading_container_group_follow`) and does three
//! jobs — lazy birth (first switch to `CenterKind::Chart`, cold boot with
//! a persisted chart center, AND preset cold-load recovery: cold-load
//! calls `replace_containers` which wipes the map, so a stale
//! `trading_center_charts` entry is detected and re-born), instrument/TF
//! reconcile (container key moved via symbol search, color-group follow,
//! or the header TF picker → re-key the bubble + re-subscribe klines),
//! and per-frame `resize_geometry` from the render-cached center rect
//! (mirrors prepare.rs's per-leaf viewport resize).

use crate::ChartApp;
use mlc_app_layout::layout::{
    ChartPanelRenderData, ChartRenderConfig, render_chart_crosshair_layer,
    render_full_chart_panel,
};
use mlc_app_panels::trading::trading::trading_container::CenterKind;
use mlc_core::indicator_source::{AlertRenderData, IndicatorSource};
use mlc_core::state::Timeframe;

impl ChartApp {
    /// Per-tick driver — see module doc. Cheap when the Trading sidebar is
    /// closed or the center shows DOM: a HashMap probe and a few string
    /// compares.
    pub(crate) fn ensure_trading_center_charts(&mut self) {
        if self.panels_store.trading_container.is_empty() {
            return;
        }

        // Snapshot container keys (single container today; map for later).
        let containers: Vec<(
            mlc_core::state::PanelId,
            String,
            String,
            String,
            String,
            CenterKind,
        )> = self
            .panels_store
            .trading_container
            .iter()
            .map(|(pid, tc)| {
                (
                    *pid,
                    tc.symbol.clone(),
                    tc.exchange.clone(),
                    tc.account_type.clone(),
                    tc.timeframe.clone(),
                    tc.center_kind,
                )
            })
            .collect();

        for (pid, symbol, exchange, account_type, tf_name, center_kind) in containers {
            let live_cid = self
                .trading_center_charts
                .get(&pid)
                .copied()
                .filter(|cid| self.panel_app.panel_grid.containers().contains_key(cid));

            let cid = match live_cid {
                Some(cid) => cid,
                None => {
                    // Stale entry (preset cold-load wiped the map) or never
                    // born. Birth only when the center actually shows the
                    // chart AND the container is bound to a real instrument.
                    self.trading_center_charts.remove(&pid);
                    if center_kind != CenterKind::Chart {
                        continue;
                    }
                    match self.birth_trading_center_chart(
                        pid, &symbol, &exchange, &account_type, &tf_name,
                    ) {
                        Some(cid) => cid,
                        None => continue,
                    }
                }
            };

            // Instrument/TF reconcile — compare the aux bubble's key against
            // the container's fields; on drift re-key + re-subscribe. Skipped
            // while the center shows DOM (the old subscription keeps flowing;
            // the next reconcile after switching back fixes the key).
            if center_kind == CenterKind::Chart {
                self.reconcile_trading_center_chart(
                    cid, &symbol, &exchange, &account_type, &tf_name,
                );
            }

            // Viewport geometry from the render-cached center rect (chart
            // area only — scales subtracted the same way the render pass
            // computes them, so zoom/pan math matches what's drawn).
            if let Some((rpid, rect)) = self.trading_center_rect {
                if rpid == pid {
                    if let Some(window) =
                        self.panel_app.panel_grid.containers_mut().get_mut(&cid)
                    {
                        let extended = mlc_core::ExtendedFrameLayout::from_proportions(
                            &rect,
                            &[],
                            &[],
                            None,
                            &window.scale_settings,
                            1.0,
                            &window.sub_pane_above_main,
                        );
                        window.resize_geometry(
                            extended.main_chart.chart.width,
                            extended.main_chart.chart.height,
                        );
                    }
                }
            }
        }
    }

    /// Create the aux `ChartContainer` + its private auto-bubble and kick
    /// off the first kline fetch. The exact birth sequence `do_split`'s
    /// fix-up loop uses (sync_group.rs), minus bar seeding — there is no
    /// source pane to inherit bars from, the fresh fetch covers it.
    fn birth_trading_center_chart(
        &mut self,
        pid: mlc_core::state::PanelId,
        symbol: &str,
        exchange: &str,
        account_type: &str,
        tf_name: &str,
    ) -> Option<mlc_core::state::ChartId> {
        if symbol.is_empty() || exchange.is_empty() {
            return None; // unbound container — wait for a symbol bind
        }
        let eid = digdigdig3::ExchangeId::from_str(exchange)?;
        let tf = Timeframe::parse(tf_name).unwrap_or_else(Timeframe::h1);

        let mut window = crate::chart_container::ChartContainer::new(symbol, tf.clone());
        // Fresh-load contract: BarsLoaded's `reconcile_view_fresh` snaps to
        // the right edge on first data; deferred snap covers the case where
        // bars land before the first `resize_geometry` gives real dims.
        window.begin_fresh_load();
        window.needs_snap_after_backfill = true;
        let cid = window.id;

        let auto_bid =
            self.create_auto_bubble([0.0, 0.0, 0.0, 0.0], symbol.to_string(), tf.clone());
        self.core.bubbles_mut().set_bid_instrument_full(
            auto_bid,
            symbol.to_string(),
            exchange.to_string(),
            account_type.to_string(),
            tf.clone(),
        );
        window.auto_tag_id = auto_bid;
        window.current_tag_id = auto_bid;
        self.panel_app.panel_grid.containers_mut().insert(cid, window);
        self.attach_chart_to_tag(cid, auto_bid);

        let at3 = crate::account_type_from_label(account_type);
        let bar_count = self.core.user().profile().domain.bar_count as usize;
        self.core
            .request_kline(eid, symbol, &tf, at3, None, Some(bar_count), true);

        self.trading_center_charts.insert(pid, cid);
        eprintln!(
            "[TradingCenter] birthed aux chart {:?} bubble {:?} for container {:?} ({} {} {} {})",
            cid, auto_bid, pid, symbol, exchange, account_type, tf.name,
        );
        Some(cid)
    }

    /// Re-key the aux bubble + re-subscribe klines when the container's
    /// symbol key or header TF moved. Same abort→re-request discipline as
    /// the topbar `ChangeTimeframe` handler (chart_out_events.rs).
    fn reconcile_trading_center_chart(
        &mut self,
        cid: mlc_core::state::ChartId,
        symbol: &str,
        exchange: &str,
        account_type: &str,
        tf_name: &str,
    ) {
        if symbol.is_empty() || exchange.is_empty() {
            return;
        }
        let Some(bid) = self.core.tags().tag_for_chart(cid) else { return };

        let (old_symbol, old_exchange, old_at, old_tf) = {
            let bubbles = self.core.bubbles();
            (
                bubbles.symbol(bid).map(String::from).unwrap_or_default(),
                bubbles.exchange(bid).map(String::from).unwrap_or_default(),
                bubbles
                    .account_type(bid)
                    .map(|at| at.to_string())
                    .unwrap_or_default(),
                bubbles.timeframe(bid).cloned(),
            )
        };
        let old_tf_name = old_tf.as_ref().map(|tf| tf.name.clone()).unwrap_or_default();

        if old_symbol == symbol
            && old_exchange == exchange
            && old_at == account_type
            && old_tf_name == tf_name
        {
            return;
        }

        let Some(new_eid) = digdigdig3::ExchangeId::from_str(exchange) else { return };
        let new_tf = Timeframe::parse(tf_name).unwrap_or_else(Timeframe::h1);

        // Drop the OLD kline forwarder before re-keying.
        if !old_symbol.is_empty() && !old_tf_name.is_empty() {
            if let Some(old_eid) = digdigdig3::ExchangeId::from_str(&old_exchange) {
                let old_at3 = crate::account_type_from_label(&old_at);
                self.core
                    .abort_kline_bars(old_eid, &old_symbol, old_at3, &old_tf_name);
            }
        }

        self.core.bubbles_mut().set_bid_instrument_full(
            bid,
            symbol.to_string(),
            exchange.to_string(),
            account_type.to_string(),
            new_tf.clone(),
        );

        if let Some(window) = self.panel_app.panel_grid.containers_mut().get_mut(&cid) {
            window.begin_fresh_load();
            window.needs_snap_after_backfill = true;
        }

        let at3 = crate::account_type_from_label(account_type);
        let bar_count = self.core.user().profile().domain.bar_count as usize;
        self.core
            .request_kline(new_eid, symbol, &new_tf, at3, None, Some(bar_count), true);
        eprintln!(
            "[TradingCenter] rekeyed aux chart {:?}: {}·{}·{}·{} -> {}·{}·{}·{}",
            cid, old_symbol, old_exchange, old_at, old_tf_name,
            symbol, exchange, account_type, tf_name,
        );
    }

    /// Draw the aux chart into the container's center rect. Read-only
    /// mirror of the split-loop leaf assembly (`render_to_scene`) reduced
    /// to what the embedded v1 chart needs: no alert bells, no leaf tab,
    /// no scale-corner writebacks, no sub-pane overlay writebacks. The
    /// heavy lifting — layout, skeleton, series, scales, crosshair — is
    /// the SAME `render_full_chart_panel` every grid leaf goes through.
    pub(crate) fn draw_trading_center_chart(
        &self,
        ctx: &mut dyn mlc_core::render::RenderContext,
        pid: mlc_core::state::PanelId,
        center_rect: &mlc_core::LayoutRect,
    ) {
        let Some(&cid) = self.trading_center_charts.get(&pid) else { return };
        let Some(window) = self.panel_app.panel_grid.containers().get(&cid) else { return };

        let chart_theme = self.panel_app.chart_theme_for_render();
        let frame_theme = self.panel_app.frame_theme_for_render();
        let no_toolbar = mlc_core::ToolbarConfig::minimal();

        let tf_name: String = self
            .timeframe_for_chart(cid)
            .map(|tf| tf.name.clone())
            .unwrap_or_default();
        let tf_interval_ms: Option<i64> = self.countdown_interval_ms_for_chart(cid);

        let panel_id = window.chart_panel_id;
        let default_panel = crate::chart_panel_state::ChartPanelState::new(panel_id);
        let panel = self
            .panels_store
            .chart_panel
            .get(&panel_id)
            .unwrap_or(&default_panel);
        let scale_corner_state = window.to_corner_state();

        // Audit D8: resolve BEFORE taking the bubbles() read guard.
        let ts_shadow = self.ts_shadow_for_chart(cid);
        let bubble_mgr = self.core.bubbles();
        let chart_type = bubble_mgr.chart_type(window.current_tag_id).unwrap_or("candles");
        let trade_archive_available =
            self.trade_archive_available_for(&bubble_mgr, window.current_tag_id);
        let window_inputs = mlc_backend::bubble::ChartWindowRenderInputs {
            viewport: window.viewport(),
            price_scale: window.price_scale(),
            time_scale: &window.time_scale,
            crosshair: &window.crosshair,
            scale_settings: &window.scale_settings,
            ha_cache: window.ha_cache.as_deref(),
            vp_overlay: if window.vp_show { window.vp_overlay.as_ref() } else { None },
            tpo_profile_overlay: if window.tpo_profile_show {
                window.tpo_profile_overlay.as_ref()
            } else {
                None
            },
            oi_show: window.oi_show,
            funding_show: window.funding_show,
            dom_heatmap_show: window.dom_heatmap_show,
            liq_heatmap_show: window.liq_heatmap_show,
            footprint_display: panel.footprint_display,
            tick_intensity: panel.tick_intensity,
            tpo_display: panel.tpo_display,
            line_family_display: panel.line_family_display,
            wicks_effective: crate::active_series::effective_wick_flag(
                chart_type,
                self.panel_app.chart_settings_state.instrument_wick_enabled,
                panel.bar_family_wicks,
            ),
            trade_archive_available,
        };
        let chart_rect = mlc_core::chart::ChartRect::new(
            center_rect.x,
            center_rect.y,
            center_rect.width,
            center_rect.height,
        );
        let mut render_state = match bubble_mgr.render_state_for(
            window.current_tag_id,
            window_inputs,
            chart_rect,
            &chart_theme,
            Some(tf_name.as_str()),
            Some(&window.scale_settings.time_format),
            tf_interval_ms,
            &panel.grid_options,
            &panel.legend,
        ) {
            Some(rs) => rs,
            None => return, // not attached to a bubble yet — placeholder stays
        };
        if let Some(shadow) = ts_shadow.as_deref() {
            render_state.bars = shadow;
            render_state.non_temporal_axis = true;
        }
        render_state.hide_time_grid =
            crate::active_series::chart_type_hides_time_grid(chart_type);
        render_state.cvd_paint_deltas =
            self.cvd_paint_deltas_for(window.current_tag_id, chart_type);

        let render_config = ChartRenderConfig {
            scale_theme: self.panel_app.scale_theme_for_render(),
            chart_type,
            crosshair_config: mlc_core::chart::CrosshairConfig::default(),
            ..ChartRenderConfig::default()
        };

        let indicator_guard = self.core.indicators();
        let drawings_guard = self.core.drawings();
        let drawing_view = drawings_guard.with_bubble(window.current_tag_id);
        let indicator_view = indicator_guard.with_bubble(window.current_tag_id);
        let inner_proportions =
            crate::chart_container::collect_inner_proportions(&window.inner_docking);
        let inner_sub_ids =
            crate::chart_container::collect_inner_instance_ids(&window.inner_docking);
        let empty_alerts: Vec<AlertRenderData> = Vec::new();
        let symbol_owned = self.symbol_for_chart(cid);

        let panel_data = ChartPanelRenderData {
            state: &render_state,
            config: &render_config,
            corner_state: &scale_corner_state,
            drawing_manager: Some(&drawing_view as &dyn mlc_core::drawing::DrawingRenderView),
            indicator_source: Some(&indicator_view as &dyn IndicatorSource),
            symbol: symbol_owned.as_deref(),
            sub_pane_y_state: Some(&window.sub_pane_y_state),
            compare_overlay: Some(&window.compare_overlay),
            watermark: panel.watermark.as_ref(),
            tooltip: Some(&panel.tooltip),
            alert_render_data: &empty_alerts,
            scale_settings: &window.scale_settings,
            selected_indicator_id: None,
            frame_theme: &frame_theme,
            sub_pane_overlay_states: &window.sub_pane_overlay_states,
            toolbar_config: &no_toolbar,
            is_split: true,
            inner_proportions: &inner_proportions,
            inner_sub_instance_ids: &inner_sub_ids,
            data_error: window.data_error.as_deref(),
            maximized_sub_pane_id: None,
            above_main_ids: Some(&window.sub_pane_above_main),
        };

        let _ = render_full_chart_panel(ctx, center_rect, &panel_data);

        // Crosshair layer on top (same layer split leaves draw after the
        // panel body). Layout built inline from the center rect — the aux
        // chart has no cached `frame_layouts` entry (leaf-keyed).
        let crosshair_layout = mlc_core::ExtendedFrameLayout::from_proportions(
            center_rect,
            &[],
            &[],
            None,
            &window.scale_settings,
            1.0,
            &window.sub_pane_above_main,
        );
        render_chart_crosshair_layer(
            ctx,
            &render_state,
            &crosshair_layout,
            &render_config,
            None,
        );
    }

    // =========================================================================
    // Input v1 (wheel zoom / drag pan / hover crosshair) — routed by raw
    // rect hit-test on the render-cached `trading_center_rect`, NOT through
    // `active_container()` (the aux chart is never the active grid chart).
    // =========================================================================

    /// Hit-test the cached center rect against a cursor position. `Some`
    /// only while the Trading sidebar shows the chart center AND the aux
    /// chart is alive in the containers map.
    pub(crate) fn trading_center_hit(
        &self,
        x: f64,
        y: f64,
    ) -> Option<(mlc_core::state::PanelId, mlc_core::state::ChartId, mlc_core::LayoutRect)>
    {
        let (pid, rect) = self.trading_center_rect?;
        if x < rect.x || x >= rect.x + rect.width || y < rect.y || y >= rect.y + rect.height {
            return None;
        }
        let cid = *self.trading_center_charts.get(&pid)?;
        if !self.panel_app.panel_grid.containers().contains_key(&cid) {
            return None;
        }
        Some((pid, cid, rect))
    }

    /// Wheel X-zoom around the cursor. Factor convention mirrors
    /// `DefaultChartInputHandler::handle_scroll` (`scroll_zoom_factor`
    /// default 0.05: dy > 0 zooms in), fit cap mirrors the main Zoom arm
    /// in `process_output_actions`.
    pub(crate) fn trading_center_zoom(
        &mut self,
        cid: mlc_core::state::ChartId,
        rect: &mlc_core::LayoutRect,
        x: f64,
        dy: f64,
    ) {
        let factor = if dy > 0.0 { 1.05 } else { 0.95 };
        let fit_cap: Option<u32> = self
            .chart_type_for_chart(cid)
            .and_then(mlc_core::chart_type::by_id)
            .and_then(|d| d.viewport_fit_cap);
        let series = self.active_series_for_chart(cid);
        if let Some(window) = self.panel_app.panel_grid.containers_mut().get_mut(&cid) {
            window.zoom_horizontal_capped(
                x - rect.x,
                factor,
                fit_cap,
                series.as_active_series(),
            );
        }
    }

    /// Apply a horizontal pan pixel delta (content follows cursor) +
    /// auto-Y refit — the Pan arm of `process_output_actions` reduced to
    /// the main-chart / auto-Y case (no sub-panes, no manual-Y price shift
    /// in v1).
    pub(crate) fn trading_center_pan_move(&mut self, cid: mlc_core::state::ChartId, dx: f64) {
        let series = self.active_series_for_chart(cid);
        if let Some(window) = self.panel_app.panel_grid.containers_mut().get_mut(&cid) {
            window.pan(dx);
            if window.price_scale().scale_mode.is_auto_y() {
                window.calc_auto_scale(series.as_active_series());
            }
        }
    }

    /// Update the aux chart's crosshair from global cursor coords —
    /// `update_crosshair_from_global` with a layout built inline from the
    /// center rect (no leaf-keyed `frame_layouts` entry exists).
    pub(crate) fn trading_center_crosshair(
        &mut self,
        cid: mlc_core::state::ChartId,
        rect: &mlc_core::LayoutRect,
        x: f64,
        y: f64,
        dragging: bool,
    ) {
        let Some(bid) = self.core.tags().tag_for_chart(cid) else { return };
        let bars_vec: Vec<mlc_core::Bar> = self.core.bubbles().bars(bid).to_vec();
        if let Some(window) = self.panel_app.panel_grid.containers_mut().get_mut(&cid) {
            let layout = mlc_core::ExtendedFrameLayout::from_proportions(
                rect,
                &[],
                &[],
                None,
                &window.scale_settings,
                1.0,
                &window.sub_pane_above_main,
            );
            let drag_pane = if dragging { Some(None) } else { None };
            window.update_crosshair_from_global(x, y, &layout, drag_pane, &bars_vec);
        }
    }
}
