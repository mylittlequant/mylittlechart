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
            // area only — scales/sub-panes subtracted the same way the
            // render pass computes them, so zoom/pan math matches what's
            // drawn).
            if let Some((rpid, rect)) = self.trading_center_rect {
                if rpid == pid {
                    let dims = self
                        .trading_center_layout(cid, &rect)
                        .map(|e| (e.main_chart.chart.width, e.main_chart.chart.height));
                    if let Some((w, h)) = dims {
                        if let Some(window) =
                            self.panel_app.panel_grid.containers_mut().get_mut(&cid)
                        {
                            window.resize_geometry(w, h);
                        }
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

        let (old_symbol, old_exchange, old_at, old_tf, ct) = {
            let bubbles = self.core.bubbles();
            (
                bubbles.symbol(bid).map(String::from).unwrap_or_default(),
                bubbles.exchange(bid).map(String::from).unwrap_or_default(),
                bubbles
                    .account_type(bid)
                    .map(|at| at.to_string())
                    .unwrap_or_default(),
                bubbles.timeframe(bid).cloned(),
                bubbles.chart_type(bid).unwrap_or("candles"),
            )
        };
        let old_tf_name = old_tf.as_ref().map(|tf| tf.name.clone()).unwrap_or_default();

        // footprint/tpo OWN the bubble timeframe (bucket interval / 1D
        // invariant, flipped by the chart-type switch itself) — the header
        // TF must not fight them. Every other type follows the container TF.
        let ct_owns_tf = matches!(ct, "footprint" | "tpo");
        let instrument_moved =
            old_symbol != symbol || old_exchange != exchange || old_at != account_type;
        let tf_moved = !ct_owns_tf && old_tf_name != tf_name;
        if !instrument_moved && !tf_moved {
            return;
        }

        let Some(new_eid) = digdigdig3::ExchangeId::from_str(exchange) else { return };
        let new_tf = if ct_owns_tf {
            old_tf.unwrap_or_else(Timeframe::h1)
        } else {
            Timeframe::parse(tf_name).unwrap_or_else(Timeframe::h1)
        };

        // Drop the OLD subscriptions at the OLD instrument coordinates —
        // both the kline forwarder and any derived/fold stream (the
        // chart-type re-subscribe below only knows the NEW coordinates).
        if !old_symbol.is_empty() {
            if let Some(old_eid) = digdigdig3::ExchangeId::from_str(&old_exchange) {
                let old_at3 = crate::account_type_from_label(&old_at);
                if !old_tf_name.is_empty() {
                    self.core
                        .abort_kline_bars(old_eid, &old_symbol, old_at3, &old_tf_name);
                }
                self.core
                    .bridge_handle()
                    .abort_derived_chart(old_eid, &old_symbol, old_at3);
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

        // Kline-family types: plain fetch at the new key. Derived types
        // (renko/footprint/range_bar/...): re-run the full chart-type
        // switch pipeline against the aux chart — it clears derived slots
        // and re-subscribes the fold/trade-fold/station stream at the NEW
        // instrument+TF (same machinery the active chart uses).
        if !crate::active_series::chart_type_is_derived_seed(ct) {
            let at3 = crate::account_type_from_label(account_type);
            let bar_count = self.core.user().profile().domain.bar_count as usize;
            self.core
                .request_kline(new_eid, symbol, &new_tf, at3, None, Some(bar_count), true);
        } else {
            let _ = self.apply_chart_type_switch(Some(cid), ct);
        }
        eprintln!(
            "[TradingCenter] rekeyed aux chart {:?} (ct={}): {}·{}·{}·{} -> {}·{}·{}·{}",
            cid, ct, old_symbol, old_exchange, old_at, old_tf_name,
            symbol, exchange, account_type, new_tf.name,
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
    ) -> Option<mlc_core::ScaleCornerHitZones> {
        let &cid = self.trading_center_charts.get(&pid)?;
        let window = self.panel_app.panel_grid.containers().get(&cid)?;

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
        let mut render_state = bubble_mgr.render_state_for(
            window.current_tag_id,
            window_inputs,
            chart_rect,
            &chart_theme,
            Some(tf_name.as_str()),
            Some(&window.scale_settings.time_format),
            tf_interval_ms,
            &panel.grid_options,
            &panel.legend,
        )?; // not attached to a bubble yet — placeholder stays
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

        // Clip everything the chart draws to the center rect — grid lines,
        // series and the crosshair layer otherwise bleed into the header /
        // right column / bottom strip (owner report 2026-07-26; grid draws
        // full-height rows and relies on neighbors overdrawing in the main
        // grid, which never happens here because the chart paints LAST).
        ctx.save();
        ctx.clip_rect(center_rect.x, center_rect.y, center_rect.width, center_rect.height);

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

        let render_result = render_full_chart_panel(ctx, center_rect, &panel_data);

        // Crosshair layer on top (same layer split leaves draw after the
        // panel body). Layout via the shared aux-layout builder so
        // sub-pane geometry matches the drawn panel.
        let crosshair_layout = self.trading_center_layout(cid, center_rect)?;
        render_chart_crosshair_layer(
            ctx,
            &render_state,
            &crosshair_layout,
            &render_config,
            None,
        );

        ctx.restore();

        // Region separators on the chart's right/bottom edges — the
        // container's own separator pass painted them BEFORE this chart
        // (which fills its rect to the last pixel), so re-stroke the two
        // boundary lines the DOM center keeps visible (owner report).
        let panel_theme =
            crate::panels_render::panel_theme_from_runtime(self.panel_app.theme_manager.current());
        ctx.set_fill_color(&panel_theme.tc_separator);
        ctx.fill_rect(
            center_rect.x + center_rect.width - 1.0,
            center_rect.y,
            1.0,
            center_rect.height,
        );
        ctx.fill_rect(
            center_rect.x,
            center_rect.y + center_rect.height - 1.0,
            center_rect.width,
            1.0,
        );

        Some(render_result.corner_zones)
    }

    // =========================================================================
    // Input — FULL chart mechanics through the aux chart's own
    // `DefaultChartInputHandler` (`trading_center_input`): chart pan,
    // price/time scale drags, hit-zone-aware wheel zoom, double-click
    // resets, scale-corner buttons. Routed by raw rect hit-test on the
    // render-cached `trading_center_rect`, NOT through
    // `active_container()` (the aux chart is never the active grid chart).
    // Not wired (later arcs): drawings/primitives, sub-panes (arrive with
    // indicators), context menu.
    // =========================================================================

    /// Hit-test the cached center rect against a cursor position. `Some`
    /// only while the Trading sidebar shows the chart center AND the aux
    /// chart is alive in the containers map. The rect INCLUDES the price/
    /// time scale strips — zone dispatch happens in the engine hit tester.
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

    /// The aux chart's `ExtendedFrameLayout`, built inline from the center
    /// rect (no leaf-keyed `frame_layouts` entry exists). Sub-panes:
    /// visible bubble-owned indicator instances filtered against the
    /// window's `inner_docking` — the same derivation the split render
    /// loop uses, so input/crosshair/resize geometry always matches what
    /// `render_full_chart_panel` draws.
    fn trading_center_layout(
        &self,
        cid: mlc_core::state::ChartId,
        rect: &mlc_core::LayoutRect,
    ) -> Option<mlc_core::ExtendedFrameLayout> {
        let window = self.panel_app.panel_grid.containers().get(&cid)?;
        let visible_set: std::collections::HashSet<u64> = self
            .core
            .indicators()
            .instances_for_bubble(window.current_tag_id)
            .into_iter()
            .filter(|i| i.visible && i.pane > 0)
            .map(|i| i.id)
            .collect();
        let all_ids = crate::chart_container::collect_inner_instance_ids(&window.inner_docking);
        let all_props = crate::chart_container::collect_inner_proportions(&window.inner_docking);
        let (sub_pane_ids, sub_proportions): (Vec<u64>, Vec<f64>) = all_ids
            .iter()
            .enumerate()
            .filter(|(_, id)| visible_set.contains(id))
            .map(|(i, &id)| (id, all_props.get(i + 1).copied().unwrap_or(0.0)))
            .unzip();
        Some(mlc_core::ExtendedFrameLayout::from_proportions(
            rect,
            &sub_pane_ids,
            &sub_proportions,
            all_props.first().copied().filter(|_| all_props.len() > 1),
            &window.scale_settings,
            1.0,
            &window.sub_pane_above_main,
        ))
    }

    /// Feed one engine input event for the aux chart: hit-test against the
    /// aux layout, run it through `trading_center_input`, apply the
    /// resulting actions to the aux window. Mirrors the main-chart tails
    /// of `on_drag_start`/`on_drag_move`/`on_drag_end`/`on_scroll`/
    /// `on_double_click` minus drawings.
    pub(crate) fn trading_center_process(
        &mut self,
        cid: mlc_core::state::ChartId,
        rect: &mlc_core::LayoutRect,
        action: mlc_core::engine::input::ChartInputAction,
    ) {
        let Some(layout) = self.trading_center_layout(cid, rect) else { return };
        let actions = {
            let hit_tester = mlc_app_layout::layout::ExtendedLayoutHitTester::new(&layout);
            self.trading_center_input.process_action(action, &hit_tester)
        };
        self.apply_trading_center_actions(cid, &layout, actions);
    }

    /// Target-aware reduction of `process_output_actions` for the aux
    /// chart: Pan / Zoom (X capped + Y manual range) / scale resets /
    /// price-scale mode toggle / crosshair hide. Sync-group propagation
    /// and undo recording are deliberately absent — the aux chart is a
    /// private single instance and undo/redo stays a main-toolbar,
    /// active-chart affair (owner spec 2026-07-26). Kinetic / cursor /
    /// repaint arms are ignored exactly like the main applier ignores
    /// them. Drawing/primitive arms are out of scope until that arc.
    fn apply_trading_center_actions(
        &mut self,
        cid: mlc_core::state::ChartId,
        layout: &mlc_core::ExtendedFrameLayout,
        actions: Vec<crate::ChartOutputAction>,
    ) {
        use crate::ChartOutputAction;
        let chart_x = layout.main_chart.chart.x;
        let fit_cap: Option<u32> = self
            .chart_type_for_chart(cid)
            .and_then(mlc_core::chart_type::by_id)
            .and_then(|d| d.viewport_fit_cap);
        let series = self.active_series_for_chart(cid);

        for action in actions {
            let Some(window) = self.panel_app.panel_grid.containers_mut().get_mut(&cid)
            else { return };
            match action {
                ChartOutputAction::Pan { bar_delta, price_delta } => {
                    window.pan(bar_delta);
                    if window.price_scale().scale_mode.is_auto_y() {
                        window.calc_auto_scale(series.as_active_series());
                    } else {
                        let price_range =
                            window.price_scale().price_max - window.price_scale().price_min;
                        let h = window.viewport().chart_height;
                        if h > 0.0 {
                            let scaled = price_delta * price_range / h;
                            window.set_price_range(
                                window.price_scale().price_min + scaled,
                                window.price_scale().price_max + scaled,
                            );
                        }
                    }
                }
                ChartOutputAction::Zoom { center_x, factor_x, factor_y, .. } => {
                    if factor_x != 1.0 {
                        window.zoom_horizontal_capped(
                            center_x - chart_x,
                            factor_x,
                            fit_cap,
                            series.as_active_series(),
                        );
                    }
                    if factor_y != 1.0 {
                        let center =
                            (window.price_scale().price_min + window.price_scale().price_max) / 2.0;
                        let half = (window.price_scale().price_max
                            - window.price_scale().price_min)
                            / 2.0
                            * factor_y;
                        window.set_price_range(center - half, center + half);
                    }
                }
                ChartOutputAction::ResetPriceScale => {
                    window.set_scale_mode(mlc_core::ScaleMode::Auto);
                    window.calc_auto_scale(series.as_active_series());
                }
                ChartOutputAction::ResetTimeScale => {
                    window.reset_time_scale();
                    window.set_scale_mode(mlc_core::ScaleMode::Auto);
                    window.calc_auto_scale(series.as_active_series());
                }
                ChartOutputAction::TogglePriceScaleMode => {
                    window.toggle_price_scale_mode();
                }
                ChartOutputAction::HideCrosshair => {
                    window.crosshair.visible = false;
                }
                // UpdateCrosshair is handled by `trading_center_crosshair`
                // (sub-pane-aware coordinate space) — same split as the main
                // applier. Everything else: ignored like the main applier
                // (kinetic/cursor/repaint) or out of scope (drawings, undo).
                _ => {}
            }
        }
        self.sidebar_data_dirty = true;
    }

    /// Scale-corner button click on the aux chart (A/M cycle + lin/log/%
    /// mode) — aux counterpart of `handle_canvas_click`'s corner branch,
    /// minus sync-group propagation. Returns `true` when a button consumed
    /// the click.
    pub(crate) fn trading_center_corner_click(
        &mut self,
        cid: mlc_core::state::ChartId,
        x: f64,
        y: f64,
    ) -> bool {
        use mlc_core::ScaleCornerButton;
        let Some(zones) = self.trading_center_corner_zones.as_ref() else { return false };
        let button = zones.hit_test(x, y);
        if button == ScaleCornerButton::None {
            return false;
        }
        let series = self.active_series_for_chart(cid);
        let Some(window) = self.panel_app.panel_grid.containers_mut().get_mut(&cid)
        else { return false };
        match button {
            ScaleCornerButton::AutoManual => {
                let next = window.price_scale().scale_mode.next();
                window.cycle_scale_mode(next, series.as_active_series());
            }
            ScaleCornerButton::Mode => {
                window.toggle_price_scale_mode();
            }
            ScaleCornerButton::None => unreachable!(),
        }
        self.sidebar_data_dirty = true;
        true
    }

    /// Cursor style over the aux chart — engine hit-zone cursors (resize
    /// arrows over scales, crosshair-hidden under magnet snap). `None`
    /// when the cursor is not the aux chart's business.
    pub(crate) fn trading_center_cursor(&self, x: f64, y: f64) -> Option<mlc_core::CursorStyle> {
        use mlc_core::engine::input::DragMode;
        // Mid-drag: hold the drag mode's cursor for the whole gesture.
        if self.trading_center_drag_active {
            return Some(match self.trading_center_input.state.drag_mode {
                DragMode::PriceScale => mlc_core::CursorStyle::NsResize,
                DragMode::TimeScale => mlc_core::CursorStyle::EwResize,
                _ => mlc_core::CursorStyle::Default,
            });
        }
        let (_pid, cid, rect) = self.trading_center_hit(x, y)?;
        // Magnet snap active on the aux chart → hide the system cursor
        // (crosshair renders at the snapped position) — mirrors
        // `is_magnet_snapped` for the active grid chart.
        let snapped = self
            .panel_app
            .panel_grid
            .containers()
            .get(&cid)
            .map(|w| w.crosshair.visible && w.crosshair.is_snapped())
            .unwrap_or(false);
        if snapped {
            return Some(mlc_core::CursorStyle::None);
        }
        let layout = self.trading_center_layout(cid, &rect)?;
        let tester = mlc_app_layout::layout::ExtendedLayoutHitTester::new(&layout);
        use mlc_core::input::ChartHitTester;
        Some(tester.hit_test(x, y).cursor())
    }

    /// Update the aux chart's crosshair from global cursor coords —
    /// `update_crosshair_from_global` with the inline aux layout.
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
        let Some(layout) = self.trading_center_layout(cid, rect) else { return };
        if let Some(window) = self.panel_app.panel_grid.containers_mut().get_mut(&cid) {
            let drag_pane = if dragging { Some(None) } else { None };
            window.update_crosshair_from_global(x, y, &layout, drag_pane, &bars_vec);
        }
    }
}
