//! prepare_frame: pre-render state sync — runs before each render to align
//! cached state, layout dirty flags, and downstream consumers with the
//! latest tick() output.

use crate::ChartApp;
use uzor::WidgetId;
use mlc_core::{
    ChartId, LayoutRect, localize_primitive_name,
};
use mlc_app_layout::panel_app::ChartPanelLayout;
use mlc_core::state::modal_state::OpenModal;
use mlc_backend_indicator_engine::RecalcMode;

impl ChartApp {
    /// Pre-render mutations — call once per frame on the mutable self BEFORE
    /// calling `render_to_scene`.
    ///
    /// Handles:
    /// - Layout computation and `content_rect` / `right_toolbar_left_x` sync
    /// - `indicator_manager.recalc_mode_label` sync
    /// - `diagnostics_enabled` sync
    /// - Viewport dimensions sync via `sync_viewport_from_layout()`
    /// - Alert-settings modal sync
    /// - Sidebar data rebuild (when `sidebar_data_dirty` is set)
    pub fn prepare_frame(&mut self, width: f64, height: f64) {
        // Advance the text-field store's frame counter so stale field geometry
        // from a previous frame is expired before new update_field calls arrive.
        // NOTE: render_to_scene also calls coordinator.begin_frame() which calls
        // text_fields.begin_frame() internally.  We call it here too so that
        // prepare_frame (called before render) stamps the correct frame on
        // update_field calls that follow render.
        self.input_coordinator.borrow_mut().text_fields_mut().begin_frame();

        // Sync text-field cursor → picker.hex_cursor before rendering.
        // The renderer reads hex_cursor from ColorPickerState, but the text-field
        // store owns the authoritative cursor position after mouse/keyboard events.
        let hex_id = WidgetId::from(crate::text_input::HEX_COLOR);
        if self.input_coordinator.borrow().text_fields().is_focused(&hex_id) {
            let coord = self.input_coordinator.borrow();
            let tf = coord.text_fields();
            let cursor = tf.cursor(&hex_id);
            let text = tf.text(&hex_id).to_string();
            let sel = tf.selection_range(&hex_id);
            drop(coord);
            let now_ms = web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let cursor_vis = self.input_coordinator.borrow().text_fields().cursor_visible(now_ms);
            for picker in [
                &mut self.panel_app.primitive_settings_state.color_picker,
                &mut self.panel_app.indicator_settings_state.color_picker,
                &mut self.panel_app.chart_settings_state.color_picker,
                &mut self.panel_app.compare_settings_state.color_picker,
                &mut self.panel_app.panel_color_picker,
            ] {
                if picker.hex_editing {
                    picker.hex_cursor = cursor;
                    picker.hex_input = text.clone();
                    picker.hex_selection_start = sel.map(|(s, _)| s);
                    picker.hex_selection_end = sel.map(|(_, e)| e);
                    picker.hex_cursor_visible = cursor_vis;
                }
            }
        }

        // Sync text-field store → sidebar_state for agent chat input rendering.
        let chat_id = WidgetId::from(crate::text_input::AGENT_CHAT);
        if self.input_coordinator.borrow().text_fields().is_focused(&chat_id) {
            let coord = self.input_coordinator.borrow();
            let tf = coord.text_fields();
            let cursor = tf.cursor(&chat_id);
            let text = tf.text(&chat_id).to_string();
            let sel = tf.selection_range(&chat_id);
            drop(coord);
            let now_ms = web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let cursor_vis = self.input_coordinator.borrow().text_fields().cursor_visible(now_ms);
            self.sidebar_state.agent_input_cursor_visible = cursor_vis;
            self.sidebar_state.agent_input_focused_leaf = self.sidebar_state.focused_agent_leaf;
            if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
                self.sidebar_state.agent_input_buffers.insert(leaf_id, text);
                self.sidebar_state.agent_input_cursors.insert(leaf_id, cursor);
                self.sidebar_state.agent_input_selections.insert(
                    leaf_id, (sel.map(|(s, _)| s), sel.map(|(_, e)| e))
                );
            }
        } else {
            self.sidebar_state.agent_input_focused_leaf = None;
        }

        let sidebar_w = self.sidebar_state.right_width();
        let window_rect = LayoutRect::new(0.0, 0.0, width, height);
        let panel_layout = ChartPanelLayout::compute(&window_rect, &self.panel_app.toolbar_config);

        // Sync content_rect and right_toolbar_left_x so input handlers have
        // correct coordinates before the frame is rendered.
        let content_rect = {
            let mut r = panel_layout.content_rect;
            r.width = (r.width - sidebar_w).max(0.0);
            r
        };
        self.content_rect = content_rect;
        self.right_toolbar_left_x = panel_layout.right_toolbar_rect.x;

        // Clamp open color picker popups to content_rect so they never
        // overlap toolbars or the right sidebar.
        {
            let cr = &content_rect;
            let margin = 4.0;
            for picker in [
                &mut self.panel_app.primitive_settings_state.color_picker,
                &mut self.panel_app.indicator_settings_state.color_picker,
                &mut self.panel_app.chart_settings_state.color_picker,
                &mut self.panel_app.compare_settings_state.color_picker,
                &mut self.panel_app.panel_color_picker,
            ] {
                if !picker.is_open() { continue; }
                let (pw, ph) = match picker.level {
                    mlc_app_layout::ui::color_picker_state::ColorPickerLevel::L1 => picker.l1_config().calculate_size(),
                    mlc_app_layout::ui::color_picker_state::ColorPickerLevel::L2 => picker.l2_config().calculate_size(),
                    _ => continue,
                };
                let min_x = cr.x + margin;
                let min_y = cr.y + margin;
                let max_x = (cr.x + cr.width - pw - margin).max(min_x);
                let max_y = (cr.y + cr.height - ph - margin).max(min_y);
                picker.origin.0 = picker.origin.0.clamp(min_x, max_x);
                picker.origin.1 = picker.origin.1.clamp(min_y, max_y);
            }
        }

        // Sync recalc_mode_label into user_settings_state so the modal can display it.
        // Bound to a `let` first so the `indicators()` read guard drops before the
        // match body runs, not at the end of it (clippy::significant_drop_in_scrutinee).
        let recalc_mode = self.core.indicators().recalc_mode();
        self.panel_app.user_settings_state.recalc_mode_label = match recalc_mode {
            RecalcMode::PerTick  => "Per Tick".to_string(),
            RecalcMode::PerFrame => "Per Frame".to_string(),
            RecalcMode::PerBar   => "Per Bar".to_string(),
        };
        // Sync diagnostics flag so the checkbox reflects the current state.
        self.panel_app.user_settings_state.diagnostics_enabled = self.diagnostics_enabled;
        // Mirror the persisted profile into the settings-modal state.
        //
        // ONE `core.user()` guard for the whole block. That call is a
        // `RwLock::read()`, and this runs every frame — taking a separate
        // guard per field (which is how the topbar-takeover and
        // control-point mirrors were first written) buys nothing and puts
        // three lock acquisitions per frame where one does the job.
        {
            let user_guard = self.core.user();
            let profile = user_guard.profile();
            self.panel_app.user_settings_state.trading_panel_topbar_takeover =
                profile.ui.trading_panel_topbar_takeover;
            self.panel_app.user_settings_state.persist_toolbar_last_tools =
                profile.ui.persist_toolbar_last_tools;
            self.panel_app.user_settings_state.persist_toolbar_toggles =
                profile.ui.persist_toolbar_toggles;
            self.panel_app.user_settings_state.control_point_shape = profile.ui.control_point_shape;
            self.panel_app.user_settings_state.control_point_size = profile.ui.control_point_size;
            // EWA engine configuration (owner ask 2026-08-14) — same
            // multi-window mirror class as the two lines above: keeps this
            // window's own EWA Engine Settings modal in sync even when a
            // DIFFERENT window is the one that changed it.
            self.panel_app.ewa_engine_settings_state.config = profile.ui.ewa_engine.clone();
            // The three markup engines, same mirror class and same reason:
            // another window's edit must show here too.
            self.panel_app.markup_engine_settings_state.smc = profile.ui.smc_engine.clone();
            self.panel_app.markup_engine_settings_state.orderflow = profile.ui.orderflow_engine.clone();
            self.panel_app.markup_engine_settings_state.candle = profile.ui.candle_pattern_engine.clone();
            // The DATA & CACHE sliders keep their committed value while the
            // user is dragging one, so the handle does not snap back on
            // every frame mid-drag.
            if !self.panel_app.user_settings_state.is_data_slider_dragging() {
                let dl = &profile.domain.data_load;
                self.panel_app.user_settings_state.data_bg_bars      = dl.background_bar_count;
                self.panel_app.user_settings_state.data_max_bars     = dl.max_loaded_bars;
            }
        }

        // Sync viewport dimensions.
        // In split mode, viewport sync is handled later in the split-pane
        // layout block (after panel_grid.layout() computes up-to-date rects).
        // Running it here too would read stale panel_rects from the previous
        // frame and apply an incorrect bar_shift to view_start.
        if !self.panel_app.panel_grid.is_split() {
            self.sync_viewport_from_layout();
        }

        // Deferred viewport snap: set_bars() defers snap-to-end + auto-scale
        // to here where layout dimensions are guaranteed valid.
        // In split mode this runs AFTER the split-layout block sets real
        // chart_width values (see below).
        if !self.panel_app.panel_grid.is_split() {
            let mut snapped_windows: Vec<(ChartId, f64, f64)> = Vec::new(); // (chart_id, view_start, bar_spacing)
            // Audit D8: snapshot each chart's OWN active series (renko/pnf/
            // kagi/... array, not always the kline array) up front to avoid
            // borrow conflict during the mut-iter loop below.
            let series_by_chart_pre: std::collections::HashMap<ChartId, crate::active_series::OwnedActiveSeries> =
                self.panel_app.panel_grid.containers().keys()
                    .map(|cid| (*cid, self.active_series_for_chart(*cid)))
                    .collect();
            let type_by_chart_pre: std::collections::HashMap<ChartId, &'static str> =
                self.panel_app.panel_grid.containers().keys()
                    .filter_map(|cid| self.chart_type_for_chart(*cid).map(|ct| (*cid, ct)))
                    .collect();
            for (&chart_id, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                // needs_snap_after_backfill alone (without needs_auto_scale_after_bars)
                // happens for startup-parked presets: update_bars() delivered bars
                // while chart_width was still 0, and update_bars never sets the
                // deferred-autoscale flag. Consume the cold-snap here once width
                // is known so the flag cannot strand.
                let series_pre = series_by_chart_pre.get(&chart_id)
                    .map(|s| s.as_active_series())
                    .unwrap_or(crate::active_series::ActiveSeries::Bars(&[]));
                if let Some((view_start, mut bar_spacing)) = window.consume_deferred_snap(series_pre) {
                    // Per-type min-usable spacing must also hold on fresh
                    // snaps (hydration restores chart_type WITHOUT going
                    // through the ChangeChartType handler — a persisted
                    // footprint at candle-zoom otherwise wakes up unreadable).
                    if let Some(min) = type_by_chart_pre.get(&chart_id)
                        .and_then(|ct| crate::input::chart_out_events::min_usable_bar_spacing(ct))
                    {
                        window.ensure_min_bar_spacing(min);
                        bar_spacing = window.viewport().bar_spacing;
                    }
                    snapped_windows.push((chart_id, view_start, bar_spacing));
                }
                // Y-invariant (2026-07-04): width materially changed →
                // refit Auto-mode Y to the new visible window.
                let series_y = series_by_chart_pre.get(&chart_id)
                    .map(|s| s.as_active_series())
                    .unwrap_or(crate::active_series::ActiveSeries::Bars(&[]));
                window.consume_y_rescale(series_y);
            }

            // Propagate viewport snap to sync-group peers so all synced windows
            // align to the same TIME position after bar load (not just user pan/zoom).
            for (chart_id, view_start, bar_spacing) in snapped_windows {
                if let Some(leaf_id) = self.panel_app.panel_grid.leaf_for_container_id(chart_id) {
                    self.propagate_viewport_to_sync_group(leaf_id, view_start, bar_spacing, None);
                }
            }
        }

        // X-axis is ALWAYS index-mode for bar charts (kline, range_bar,
        // volume_bar, tick TF, footprint). Industry standard: every major
        // platform (TradeStation, NinjaTrader, MultiCharts, TradingView,
        // ATAS, Sierra Chart, Bookmap footprint, Lightweight Charts)
        // lays bars out at uniform pixel spacing regardless of inter-bar
        // time gaps; the time scale shows non-uniform labels, but bars
        // are equally wide. Time-axis mode is reserved for heatmap-class
        // visualisations (DOM heatmap, liquidation heatmap, time & sales)
        // where time is a continuous axis, not a bar slot.
        //
        // Defensive cleanup: if a stale time_axis from an earlier session
        // (pre-cutover) lingers on the viewport, drop it back to index.
        // Phase X0 / C5b: ensure_ha_cache / ensure_vp now read bars from the
        // bubble. We collect the bubble snapshots up front (immutable borrow on
        // managers.bubble) so the mut-loop over containers can pass `&bubble`
        // into the per-window helpers without re-borrowing the store.
        let bubble_snapshot: std::collections::HashMap<
            mlc_core::ids::BubbleId,
            (Vec<mlc_core::Bar>,)
        > = {
            let mut map = std::collections::HashMap::new();
            self.core.bubbles().each_bubble(|bv| {
                map.insert(bv.id(), (bv.bars().to_vec(),));
            });
            map
        };

        // Diagnostic: dump per-container bubble pointers + bar counts ONCE per
        // dirty render so we can see whether `window.current_tag_id` lines
        // up with the bubble that actually has bars.  Owner repro: split or
        // new-preset sibling chart renders empty even though
        // `[do_split-key]` shows the auto_bubble was seeded with the source's
        // identity.  Suspicion: window.current_tag_id points at a stale
        // bubble id while the data lands in a different bubble.
        {
            let mut dump: Vec<String> = Vec::new();
            let bubble_mgr = self.core.bubbles();
            let tags_mgr = self.core.tags();
            for (cid, w) in self.panel_app.panel_grid.containers().iter() {
                let cur_bid = w.current_tag_id;
                let auto_bid = w.auto_tag_id;
                let bid_via_chart = tags_mgr.tag_for_chart(*cid);
                let cur_bars = bubble_mgr.bars(cur_bid).len();
                let cur_sym  = bubble_mgr.symbol(cur_bid).unwrap_or_default().to_string();
                let cur_exch = bubble_mgr.exchange(cur_bid).unwrap_or_default().to_string();
                let auto_bars = bubble_mgr.bars(auto_bid).len();
                let auto_sym  = bubble_mgr.symbol(auto_bid).unwrap_or_default().to_string();
                dump.push(format!(
                    "cid={:?} cur={:?}(sym='{}' exch='{}' bars={}) auto={:?}(sym='{}' bars={}) via_chart={:?}",
                    cid, cur_bid, cur_sym, cur_exch, cur_bars, auto_bid, auto_sym, auto_bars, bid_via_chart
                ));
            }
            #[cfg(target_arch = "wasm32")]
            {
                std::thread_local! {
                    static LAST_DUMP: std::cell::RefCell<Option<String>> = std::cell::RefCell::new(None);
                    static FRAME_COUNT: std::cell::Cell<u32> = std::cell::Cell::new(0);
                }
                let joined = dump.join(" | ");
                LAST_DUMP.with(|cell| {
                    let mut last = cell.borrow_mut();
                    let changed = last.as_ref().map_or(true, |s| s != &joined);
                    FRAME_COUNT.with(|f| f.set(f.get().wrapping_add(1)));
                    let every_sec = FRAME_COUNT.with(|f| f.get() % 120 == 0);
                    if changed || every_sec {
                        ::web_sys::console::log_1(&format!(
                            "[prepare-bubble-map] containers={} {}",
                            dump.len(), joined
                        ).into());
                        *last = Some(joined);
                    }
                });
            }
            #[cfg(not(target_arch = "wasm32"))]
            { let _ = dump; }
        }

        let is_ha_by_chart: std::collections::HashMap<ChartId, bool> =
            self.panel_app.panel_grid.containers().keys()
                .map(|cid| (*cid, self.chart_type_for_chart(*cid) == Some("heikin_ashi")))
                .collect();
        // Overlay on/off is DERIVED from a visible instance (owner doctrine
        // 2026-08-01) and the loop below holds `containers_mut()`, so it is
        // resolved up front — same shape as the Heikin-Ashi memo above.
        let vp_on_by_chart: std::collections::HashMap<ChartId, bool> =
            self.panel_app.panel_grid.containers().keys()
                .map(|cid| (*cid, self.overlay_instance_active_for_chart(*cid, Self::VOLUME_PROFILE_TYPE_ID)))
                .collect();
        let tpo_on_by_chart: std::collections::HashMap<ChartId, bool> =
            self.panel_app.panel_grid.containers().keys()
                .map(|cid| (*cid, self.overlay_instance_active_for_chart(*cid, Self::TPO_PROFILE_TYPE_ID)))
                .collect();
        // Shape params, resolved with the same up-front borrow as the gates:
        // rows and value area change what the profile IS, not how it reads,
        // so they belong to the prepass rather than to the ink.
        #[allow(clippy::type_complexity)]
        let profile_shape_by_chart: std::collections::HashMap<
            ChartId,
            ((usize, f64), (usize, f64), mlc_core::chart::volume_profile::VpMode),
        > = self.panel_app.panel_grid.containers().keys()
            .filter_map(|cid| {
                let bid = self.bubble_for_chart(*cid)?;
                Some((
                    *cid,
                    (
                        self.profile_shape(bid, Self::VOLUME_PROFILE_TYPE_ID),
                        self.profile_shape(bid, Self::TPO_PROFILE_TYPE_ID),
                        self.vp_mode(bid),
                    ),
                ))
            })
            .collect();
        // Other venues' bars for the profiles that fold them, resolved
        // before the `containers_mut()` borrow. Keyed by chart; empty for
        // every chart whose profiles are single-venue, which is the default.
        #[allow(clippy::type_complexity)]
        let profile_foreign_by_chart: std::collections::HashMap<ChartId, (Vec<mlc_core::Bar>, Vec<mlc_core::Bar>)> =
            self.panel_app.panel_grid.containers().keys()
                .filter(|cid| {
                    vp_on_by_chart.get(*cid).copied().unwrap_or(false)
                        || tpo_on_by_chart.get(*cid).copied().unwrap_or(false)
                })
                .filter_map(|cid| {
                    let bid = self.bubble_for_chart(*cid)?;
                    let symbol = self.symbol_for_chart(*cid)?;
                    let tf = self.timeframe_for_chart(*cid)?;
                    let bars = bubble_snapshot.get(&bid).map(|(b,)| b.as_slice()).unwrap_or(&[]);
                    // The whole loaded span: the mode picks its own window
                    // out of this, and TPO clips to the visible slice.
                    let t0 = bars.first().map(|b| b.ts_ms())?;
                    let t1 = bars.last().map(|b| b.ts_ms())?;
                    Some((
                        *cid,
                        (
                            self.profile_foreign_bars(bid, Self::VOLUME_PROFILE_TYPE_ID, &symbol, &tf.name, t0, t1),
                            self.profile_foreign_bars(bid, Self::TPO_PROFILE_TYPE_ID, &symbol, &tf.name, t0, t1),
                        ),
                    ))
                })
                .collect();

        for (cid, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
            // Refresh the Heikin Ashi memo if the chart panel is on
            // chart_type == "heikin_ashi" AND `bars` mutated since the
            // last frame. Cheap when stamp matches (single i64
            // comparison + Option check).
            let is_ha = is_ha_by_chart.get(cid).copied().unwrap_or(false);
            let bubble_bars: &[mlc_core::Bar] = bubble_snapshot
                .get(&window.current_tag_id)
                .map(|(b,)| b.as_slice())
                .unwrap_or(&[]);
            if is_ha {
                // Cheap proxy bubble carrying just the bars slice — ensure_ha_cache
                // only reads bars + stamp. Avoids re-borrowing self.managers.bubble.
                window.ensure_ha_cache_from_bars(bubble_bars);
            }

            // Volume Profile prepass — recompute when the visible slice, mode,
            // or bars change. Cheap when the stamp matches (one tuple comparison).
            if vp_on_by_chart.get(cid).copied().unwrap_or(false) {
                let (vis_start, vis_end) = window.viewport().visible_range();
                let vis_end_clamped = vis_end.min(bubble_bars.len());
                let visible_owned: Vec<mlc_core::Bar> = if vis_start <= vis_end_clamped {
                    bubble_bars[vis_start..vis_end_clamped].to_vec()
                } else {
                    Vec::new()
                };
                let ((rows, va), _, mode) = profile_shape_by_chart
                    .get(cid)
                    .copied()
                    .unwrap_or(((24, 0.70), (60, 0.70), Default::default()));
                let foreign = profile_foreign_by_chart
                    .get(cid)
                    .map(|(vp, _)| vp.as_slice())
                    .unwrap_or(&[]);
                window.ensure_vp_from_bars(bubble_bars, &visible_owned, rows, va, mode, foreign);
            }

            // TPO Profile prepass (item D) — mirrors the Volume Profile
            // prepass above, one layer simpler (always VisibleRange, no
            // mode). `min_tick` is a defensive floor only (see
            // `TpoProfileData::compute`'s doc comment) — `nice_step`
            // already targets ~60 rows from the visible price range
            // regardless, so a small fixed epsilon (rather than a real
            // per-instrument tick lookup, which nothing at this layer
            // exposes today — same simplification VP's own fixed
            // `bucket_count = 24` already makes) is sufficient.
            if tpo_on_by_chart.get(cid).copied().unwrap_or(false) {
                let (vis_start, vis_end) = window.viewport().visible_range();
                let vis_end_clamped = vis_end.min(bubble_bars.len());
                let visible_owned: Vec<mlc_core::Bar> = if vis_start <= vis_end_clamped {
                    bubble_bars[vis_start..vis_end_clamped].to_vec()
                } else {
                    Vec::new()
                };
                let (_, (rows, va), _) = profile_shape_by_chart
                    .get(cid)
                    .copied()
                    .unwrap_or(((24, 0.70), (60, 0.70), Default::default()));
                // TPO reads the VISIBLE slice, so the foreign bars are
                // clipped to the same span rather than the whole series.
                let (t0, t1) = (
                    visible_owned.first().map(|b| b.ts_ms()).unwrap_or(i64::MIN),
                    visible_owned.last().map(|b| b.ts_ms()).unwrap_or(i64::MAX),
                );
                let foreign: Vec<mlc_core::Bar> = profile_foreign_by_chart
                    .get(cid)
                    .map(|(_, tpo)| {
                        tpo.iter()
                            .filter(|b| {
                                let ts = b.ts_ms();
                                ts >= t0 && ts <= t1
                            })
                            .copied()
                            .collect()
                    })
                    .unwrap_or_default();
                window.ensure_tpo_profile_from_bars(&visible_owned, 1e-8, rows as f64, va, &foreign);
            }
        }

        // Keep the indicator-settings modal's venue picker in sync. The
        // offered venues are host state (which connectors serve this layer's
        // declared stream), so they are resolved per frame rather than
        // captured when the modal opened.
        if self.panel_app.indicator_settings_state.is_open() {
            self.refresh_indicator_venue_options();
        }

        // Keep the alert-settings modal's alerts list always in sync.
        if self.panel_app.alert_settings_state.is_open() {
            self.panel_app.alert_settings_state.all_alerts = self.core.alerts().items_all();
        }

        // Auto-dirty sidebar when active leaf changes (for object tree refresh).
        let current_leaf = self.panel_app.panel_grid.docking().active_leaf();
        if current_leaf != self.last_active_leaf {
            self.last_active_leaf = current_leaf;
            self.sidebar_data_dirty = true;

            // When the active chart changes, reassign all Synced panels to the new
            // chart's group and apply the group's instrument key to their states.
            if let Some(new_active_group) = self.panel_app.panel_grid
                .active_container_id()
                .and_then(|cid| self.bubble_for_chart(cid))
            {
                // Sync the group's full instrument key from the new active window.
                // symbol included — without this, restored groups carry stale
                // group.symbol from the prior session and apply_key_to_panels_in_group
                // overwrites panel state.symbol with that stale value on the very
                // first frame (when last_active_leaf=None ≠ current_leaf).
                if let Some(active_cid_sync) = self.panel_app.panel_grid.active_container_id() {
                    // KEY-INTEGRITY (series-leak fix 2026-07-17): the full
                    // (symbol, exchange, AT) key goes through ONE resetting
                    // setter. The previous shape — `set_bubble_instrument`
                    // (exchange+AT) plus a RAW `g.address.symbol = sym` — was
                    // a partial-key write that could flip the bubble's symbol
                    // without `reset_raw_layers()`, leaving the old symbol's
                    // series in the layers (two series on one chart). And no
                    // guessed parts: when any leg of the key is unresolved
                    // (empty symbol/exchange, unknown AT), SKIP the sync
                    // entirely rather than writing a fabricated key.
                    let sym = self.symbol_for_chart(active_cid_sync).map(String::from);
                    let exch = self.exchange_for_chart(active_cid_sync).map(String::from);
                    let at = self.account_type_for_chart(active_cid_sync).map(|at| at.to_string());
                    if let (Some(sym), Some(exch), Some(at)) = (sym, exch, at) {
                        if !sym.is_empty() && !exch.is_empty() {
                            self.core.bubbles_mut().set_instrument(new_active_group, sym, exch, at);
                        }
                    }
                }
                self.core.tags_mut().reassign_synced_panels(new_active_group);
                self.apply_key_to_panels_in_group(new_active_group);
            }
        }

        // Populate sidebar data from chart state (guarded by dirty flag).
        if self.sidebar_state.is_right_open() && self.sidebar_data_dirty {
            // --- ObjectTree: drawing primitives + indicators ---
            self.sidebar_state.object_tree_items.clear();

            let active_cid = self.panel_app.panel_grid.active_container_id();

            // Determine whether the active window is in a real (non-auto_created) tag group.
            // Snapshot the fields we need (sync flags, primitives, indicator configs) so
            // the RwLock guard can drop before iterating them.
            struct TaggedGroupSnapshot {
                sync_flags: mlc_core::ids::SyncFlags,
                primitives: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>>,
            }
            let tagged_group: Option<TaggedGroupSnapshot> = active_cid
                .and_then(|cid| self.bubble_for_chart(cid))
                .and_then(|gid| {
                    let is_auto_created = self.core.tags().get(gid).map(|t| t.auto_created).unwrap_or(true);
                    if is_auto_created {
                        None
                    } else {
                        self.with_bubble(gid, |g| TaggedGroupSnapshot {
                            sync_flags: g.sync_flags.clone(),
                            primitives: g.primitives.iter().map(|p| p.clone_box()).collect(),
                        })
                    }
                });

            // Helper: convert a PrimitiveKind into an ObjectCategory.
            let prim_category = |kind: mlc_core::PrimitiveKind| match kind {
                mlc_core::PrimitiveKind::Annotation => mlc_core::ObjectCategory::Text,
                mlc_core::PrimitiveKind::Measurement => mlc_core::ObjectCategory::Measurement,
                mlc_core::PrimitiveKind::Trading => mlc_core::ObjectCategory::Position,
                mlc_core::PrimitiveKind::Signal => mlc_core::ObjectCategory::Signal,
                _ => mlc_core::ObjectCategory::Drawing,
            };

            // Collect active window key fields before any borrows.
            let active_cid_obj = self.panel_app.panel_grid.active_container_id();
            let (active_window_sym, active_window_exchange, active_window_account_type) = {
                let sym = active_cid_obj
                    .and_then(|cid| self.symbol_for_chart(cid))
                    .map(String::from).unwrap_or_default();
                let exch = active_cid_obj
                    .and_then(|cid| self.exchange_for_chart(cid))
                    .map(String::from).unwrap_or_default();
                let at = active_cid_obj
                    .and_then(|cid| self.account_type_for_chart(cid))
                    .map(|at| at.to_string()).unwrap_or_else(|| "S".to_string());
                (sym, exch, at)
            };

            if let Some(group) = tagged_group {
                // ----------------------------------------------------------------
                // TAGGED window: two sections — "Group" and (optionally) "Window"
                // ----------------------------------------------------------------

                // --- Section "Group": primitives from group.primitives ---
                if group.sync_flags.sync_drawings {
                    for p in group.primitives.iter() {
                        let data = p.data();
                        let kind = p.kind();
                        let display = localize_primitive_name(&data.type_id, p.display_name());
                        let name = if display.is_empty() { data.type_id.as_str() } else { display.as_str() };
                        // Compare the full instrument key (symbol, exchange, account_type).
                        // For primitives created before exchange/account_type fields existed,
                        // exchange will be empty — fall back to symbol-only comparison so they
                        // are not incorrectly shown as "other key".
                        let prim_sym = data.symbol.clone();
                        let prim_exchange = data.exchange.clone();
                        let prim_account_type = data.account_type.clone();
                        let is_active_sym = if prim_exchange.is_empty() {
                            prim_sym == active_window_sym
                        } else {
                            prim_sym == active_window_sym
                                && prim_exchange == active_window_exchange
                                && prim_account_type == active_window_account_type
                        };
                        let item_state = if is_active_sym {
                            mlc_app_sidebar_content::types::ObjectItemState::Active
                        } else {
                            mlc_app_sidebar_content::types::ObjectItemState::Memory
                        };
                        let memory_kind = if is_active_sym {
                            mlc_app_sidebar_content::types::MemoryKind::None
                        } else {
                            mlc_app_sidebar_content::types::MemoryKind::GroupOtherKey
                        };
                        let item = mlc_app_sidebar_content::types::ObjectTreeItem::new(
                            data.id, name, prim_category(kind), &data.type_id,
                        )
                        .with_visible(data.visible)
                        .with_locked(data.locked)
                        .with_color(Some(data.color.stroke.clone()))
                        .with_section("Group")
                        .with_key(&prim_sym, &active_window_exchange, &active_window_account_type)
                        .with_item_state(item_state)
                        .with_memory_kind(memory_kind);
                        self.sidebar_state.object_tree_items.push(item);
                    }
                }

                // --- Section "Group": indicators that LIVE in this bubble ---
                // Read instances directly from the indicator manager filtered
                // by `bubble_id == group_id`.  Group = tag = bubble — there is
                // no separate "indicator_configs" list; the bubble's own
                // instance set IS the indicator list.
                let active_bubble_id = active_cid
                    .and_then(|cid| self.bubble_for_chart(cid));
                let group_indicator_data: Vec<(u64, String, String, bool, bool)> = active_bubble_id
                    .map(|bid| {
                        self.core.indicators().instances_for_bubble(bid)
                            .into_iter()
                            .map(|i| (i.id, i.name.clone(), i.type_id.clone(), i.visible, i.locked))
                            .collect()
                    })
                    .unwrap_or_default();
                for (id, name, type_id, visible, locked) in group_indicator_data {
                    let item = mlc_app_sidebar_content::types::ObjectTreeItem::new(
                        id, &name, mlc_core::ObjectCategory::Indicator, &type_id,
                    )
                    .with_visible(visible)
                    .with_locked(locked)
                    .with_section("Group")
                    .with_key(&active_window_sym, &active_window_exchange, &active_window_account_type)
                    .with_item_state(mlc_app_sidebar_content::types::ObjectItemState::Active)
                    .with_memory_kind(mlc_app_sidebar_content::types::MemoryKind::None);
                    self.sidebar_state.object_tree_items.push(item);
                }

                // --- Section "Window": chart's auto-bubble stash ---
                // Show the home stash (auto_bubble.primitives + indicators
                // whose `bubble_id == auto_tag_id`) whenever the active
                // chart is in a tagged bubble distinct from its home auto.
                // Owner rule: Window section reflects the chart's own
                // stash; current Group section reflects the bubble the
                // chart is currently pointing at.  When both equal
                // (untagged), only the Group section is shown.
                let (active_auto_bid, active_cur_bid) = active_cid
                    .and_then(|cid| self.panel_app.panel_grid.containers().get(&cid))
                    .map(|w| (Some(w.auto_tag_id), Some(w.current_tag_id)))
                    .unwrap_or((None, None));
                let show_home_overlay = match (active_auto_bid, active_cur_bid) {
                    (Some(a), Some(c)) => a != c,
                    _ => false,
                };

                let stashed_prim_data: Vec<_> = if show_home_overlay {
                    active_auto_bid
                        .and_then(|auto_bid| self.with_bubble(auto_bid, |auto_bub| {
                            auto_bub.primitives.iter()
                                .map(|p| {
                                    let data = p.data();
                                    let kind = p.kind();
                                    let display = localize_primitive_name(&data.type_id, p.display_name());
                                    (data.id, display, data.type_id.clone(), kind, data.visible, data.locked, data.color.stroke.clone(), data.symbol.clone())
                                })
                                .collect()
                        }))
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };

                // Home indicators live IN the auto-bubble: their bubble_id
                // matches auto_tag_id.  Collect them live from the
                // indicator manager, not from the stale pre_tag_indicator_ids
                // list which the bubble-cutover obsoleted.
                let pre_tag_ids: Vec<u64> = if show_home_overlay {
                    active_auto_bid
                        .map(|auto_bid| {
                            self.core.indicators().instances_for_bubble(auto_bid)
                                .into_iter()
                                .map(|i| i.id)
                                .collect()
                        })
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };

                let has_window_section = !stashed_prim_data.is_empty() || !pre_tag_ids.is_empty();

                if has_window_section {
                    for (id, display, type_id, kind, visible, locked, stroke, prim_symbol) in &stashed_prim_data {
                        let name = if display.is_empty() { type_id.as_str() } else { display.as_str() };
                        let item = mlc_app_sidebar_content::types::ObjectTreeItem::new(
                            *id, name, prim_category(*kind), type_id,
                        )
                        .with_visible(*visible)
                        .with_locked(*locked)
                        .with_color(Some(stroke.clone()))
                        .with_section("Window")
                        .with_key(prim_symbol, &active_window_exchange, &active_window_account_type)
                        .with_item_state(mlc_app_sidebar_content::types::ObjectItemState::Memory)
                        .with_memory_kind(mlc_app_sidebar_content::types::MemoryKind::WindowStash);
                        self.sidebar_state.object_tree_items.push(item);
                    }

                    for &iid in &pre_tag_ids {
                        // Bound first so the `indicators()` read guard drops before the
                        // body runs (clippy::significant_drop_in_scrutinee).
                        let summary = self.core.indicators().summary(iid);
                        if let Some(summary) = summary
                        {
                            let item = mlc_app_sidebar_content::types::ObjectTreeItem::new(
                                summary.id,
                                &summary.name,
                                mlc_core::ObjectCategory::Indicator,
                                &summary.type_id,
                            )
                            .with_visible(summary.visible)
                            .with_locked(summary.locked)
                            .with_section("Window")
                            .with_key(&active_window_sym, &active_window_exchange, &active_window_account_type)
                            // Home-stash indicators are memory-overlay only:
                            // they appear under the Memory divider in the
                            // "Window" section and behave the same as the
                            // stashed primitives above (no live mutation
                            // mirror; delete is suppressed by the UI).
                            .with_item_state(mlc_app_sidebar_content::types::ObjectItemState::Memory)
                            .with_memory_kind(mlc_app_sidebar_content::types::MemoryKind::WindowStash);
                            self.sidebar_state.object_tree_items.push(item);
                        }
                    }
                }
            } else {
                // ----------------------------------------------------------------
                // UNTAGGED window (auto_created group): flat list, no section headers
                // ----------------------------------------------------------------

                // Primitives from the bubble's drawing data — all symbols, annotated by state.
                let local_prims: Vec<_> = self.panel_app.panel_grid.active_container()
                    .map(|w| w.current_tag_id)
                    .map(|bid| {
                        let guard = self.core.drawings();
                        guard.primitives(bid).iter()
                            .map(|p| {
                                let data = p.data();
                                let kind = p.kind();
                                let display = localize_primitive_name(&data.type_id, p.display_name());
                                (data.id, display, data.type_id.clone(), kind, data.visible, data.locked, data.color.stroke.clone(), data.symbol.clone(), data.exchange.clone(), data.account_type.clone())
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                for (id, display, type_id, kind, visible, locked, stroke, prim_sym, prim_exchange, prim_account_type) in &local_prims {
                    let name = if display.is_empty() { type_id.as_str() } else { display.as_str() };
                    // Compare full instrument key; fall back to symbol-only for primitives without exchange (legacy).
                    let is_active_sym = if prim_exchange.is_empty() {
                        *prim_sym == active_window_sym
                    } else {
                        *prim_sym == active_window_sym
                            && *prim_exchange == active_window_exchange
                            && *prim_account_type == active_window_account_type
                    };
                    let item_state = if is_active_sym {
                        mlc_app_sidebar_content::types::ObjectItemState::Active
                    } else {
                        mlc_app_sidebar_content::types::ObjectItemState::Memory
                    };
                    let memory_kind = if is_active_sym {
                        mlc_app_sidebar_content::types::MemoryKind::None
                    } else {
                        mlc_app_sidebar_content::types::MemoryKind::WindowOtherKey
                    };
                    let key_exchange = if prim_exchange.is_empty() { &active_window_exchange } else { prim_exchange };
                    let key_account_type = if prim_account_type.is_empty() { &active_window_account_type } else { prim_account_type };
                    let item = mlc_app_sidebar_content::types::ObjectTreeItem::new(
                        *id, name, prim_category(*kind), type_id,
                    )
                    .with_visible(*visible)
                    .with_locked(*locked)
                    .with_color(Some(stroke.clone()))
                    .with_key(prim_sym, key_exchange, key_account_type)
                    .with_item_state(item_state)
                    .with_memory_kind(memory_kind);
                    self.sidebar_state.object_tree_items.push(item);
                }

                // Indicators bound to this chart's CURRENT bubble — always
                // Active.  Indicators follow the bubble, not the window.
                let active_bubble_id = active_cid
                    .and_then(|cid| self.bubble_for_chart(cid));
                let total_insts = self.core.indicators().count();
                let inst_dump: Vec<(u64, Option<u64>, String)> = self.core.indicators().all_instances()
                    .into_iter()
                    .map(|i| (i.id, i.bubble_id, i.type_id.clone()))
                    .collect();
                crate::sglog!(
                    "[obj-tree-untagged] active_cid={:?} active_bubble_id={:?} total_insts={} dump={:?}",
                    active_cid, active_bubble_id, total_insts, inst_dump
                );
                if let Some(bid) = active_bubble_id {
                    let insts: Vec<_> = self.core.indicators().instances_for_bubble(bid)
                        .into_iter()
                        .map(|i| (i.id, i.name.clone(), i.type_id.clone(), i.visible, i.locked))
                        .collect();
                    for (id, name, type_id, visible, locked) in &insts {
                        let item = mlc_app_sidebar_content::types::ObjectTreeItem::new(
                            *id,
                            name,
                            mlc_core::ObjectCategory::Indicator,
                            type_id,
                        )
                        .with_visible(*visible)
                        .with_locked(*locked)
                        .with_key(&active_window_sym, &active_window_exchange, &active_window_account_type)
                        .with_item_state(mlc_app_sidebar_content::types::ObjectItemState::Active)
                        .with_memory_kind(mlc_app_sidebar_content::types::MemoryKind::None);
                        self.sidebar_state.object_tree_items.push(item);
                    }
                }
            }

            // Sort within sections: Active first, then Memory (stable to preserve sub-order).
            self.sidebar_state.object_tree_items.sort_by_key(|item| {
                let section_order: u8 = match item.section.as_deref() {
                    Some("Group") => 0,
                    Some("Window") => 1,
                    _ => 2,
                };
                let state_order: u8 = match item.item_state {
                    mlc_app_sidebar_content::types::ObjectItemState::Active => 0,
                    mlc_app_sidebar_content::types::ObjectItemState::Memory => 1,
                };
                (section_order, state_order)
            });

            // --- ObjectTree: compare overlay series ---
            if let Some(window) = self.panel_app.panel_grid.active_container() {
                for (i, series) in window.compare_overlay.series.iter().enumerate() {
                    let item = mlc_app_sidebar_content::types::ObjectTreeItem::new(
                        i as u64,
                        &series.symbol,
                        mlc_core::ObjectCategory::Compare,
                        "Compare",
                    )
                    .with_visible(series.visible)
                    .with_color(Some(series.color.clone()));
                    self.sidebar_state.object_tree_items.push(item);
                }
            }

            // --- Signals panel: collect per-instance SignalEvents ---
            use mlc_app_sidebar_content::types::{IndicatorsTabData, IndicatorSignalGroup, IndicatorSignalRow};

            // Only show signals for indicator instances that belong to the
            // active chart's bubble (indicator ownership = bubble).
            let active_bubble_id_for_signals = active_cid
                .and_then(|cid| self.bubble_for_chart(cid));
            let indicators_guard_signals = self.core.indicators();
            let signal_groups: Vec<IndicatorSignalGroup> = active_bubble_id_for_signals
                .map(|bid| indicators_guard_signals.instances_for_bubble(bid))
                .unwrap_or_default()
                .into_iter()
                .filter(|inst| !inst.signals.is_empty())
                .map(|inst| {
                    let mut rows: Vec<IndicatorSignalRow> = inst
                        .signals
                        .iter()
                        .map(|ev| IndicatorSignalRow {
                            bar_index: ev.bar_index as i64,
                            signal_type: format!("{:?}", ev.kind),
                            price: ev.price,
                            strength: 0.0,
                            direction: ev.direction.as_i8() as i32,
                        })
                        .collect();
                    rows.sort_by(|a, b| b.bar_index.cmp(&a.bar_index));
                    IndicatorSignalGroup {
                        instance_id: inst.id,
                        indicator_name: inst.name.clone(),
                        collapsed: self
                            .sidebar_state
                            .collapsed_signal_groups
                            .contains(&inst.id),
                        signals: rows,
                    }
                })
                .collect();

            let total_count = signal_groups.iter().map(|g| g.signals.len()).sum();
            self.sidebar_state.indicator_signals = IndicatorsTabData {
                groups: signal_groups,
                total_count,
            };

            // --- Watchlist: populate from WatchlistManager symbol list ---
            {
                use mlc_app_sidebar_content::types::WatchlistItem;

                self.sidebar_state.watchlist_items.clear();
                let watchlist_entries: Vec<mlc_core::InstrumentAddress> = self
                    .core
                    .watchlists()
                    .find(self.watchlist_ui.active_list_id)
                    .map(|list| list.all_symbols().into_iter().cloned().collect())
                    .unwrap_or_default();

                for address in &watchlist_entries {
                    let sym_name = address.symbol.as_str();
                    let sym_exchange = address.exchange.as_str();
                    let sym_account_type = address.account_type;
                    let ticker_key = format!("{}:{}:{}", sym_name, sym_exchange, sym_account_type);

                    let (last_price, change_percent, high_24h, low_24h, volume_24h, spark) =
                        if let Some(ticker) = self.core.mini_ticker.get(&ticker_key) {
                            (
                                ticker.last_price,
                                ticker.price_change_percent,
                                ticker.high_price,
                                ticker.low_price,
                                ticker.volume,
                                ticker.spark.iter().copied().collect(),
                            )
                        } else {
                            (0.0, 0.0, 0.0, 0.0, 0.0, Vec::new())
                        };

                    // A currently-open chart on this EXACT instrument tracks its
                    // own kline-specific error in `window.data_error`, cleared
                    // precisely on the next successful reload
                    // (`ChartContainer::reconcile_view_fresh`) independent of the
                    // mini-ticker's own health. Checked AHEAD of `last_price`
                    // (owner repro 2026-08-30 round 3: Bitfinex WS ticker alive,
                    // REST klines dead behind a killed proxy — the row showed a
                    // plain live price with no error at all while its own chart
                    // was failing) so a klines-dead-but-ticker-alive venue still
                    // surfaces the class on the row instead of reading as healthy.
                    let open_chart_error = self.panel_app.panel_grid.containers().iter()
                        .find(|(cid, _)| {
                            self.symbol_for_chart(**cid)
                                .map_or(false, |s| s == sym_name)
                            && self.exchange_for_chart(**cid)
                                .map_or(false, |e| e == sym_exchange)
                            && self.account_type_for_chart(**cid)
                                .map_or(false, |a| a == sym_account_type)
                        })
                        .and_then(|(_, w)| w.data_error.as_deref())
                        .map(classify_watchlist_feed_error);

                    let status = if let Some(s) = open_chart_error {
                        Some(s)
                    } else if last_price > 0.0 {
                        self.sidebar_state.feed_status.remove(&ticker_key);
                        None
                    } else if let Some(s) = self.sidebar_state.feed_status.get(&ticker_key) {
                        Some(s.clone())
                    } else {
                        None
                    };

                    self.sidebar_state.watchlist_items.push(WatchlistItem {
                        symbol: sym_name.to_string(),
                        exchange: sym_exchange.to_string(),
                        last_price,
                        change_percent,
                        high_24h,
                        low_24h,
                        volume_24h,
                        account_type: sym_account_type.to_string(),
                        address: address.clone(),
                        spark,
                        status,
                    });
                }
            }

            // --- Screener: mirror the watchlist rows just built above +
            // attach cached RSI/ATR/dist metrics (plan F v1, owner spec
            // 2026-08-01). Symbol/Last/24h%/Vol are the SAME live
            // `watchlist_items` data (zero new subscriptions — mini_ticker
            // is already streaming for every watchlist symbol). RSI/ATR%/
            // dist-to-24h-high-low come from `BackendCore::screener_metrics`,
            // populated only by a manual Scan (see the "screener_scan" /
            // "screener_row:" click handlers in `panel_click.rs`); `None`
            // until the symbol has been scanned at least once — rendered as
            // "…" by `mlc-app-sidebar-content::render`.
            {
                use mlc_app_sidebar_content::types::ScreenerItem;

                self.sidebar_state.screener_items = self.sidebar_state.watchlist_items.iter().map(|w| {
                    let key = format!("{}:{}:{}", w.symbol, w.exchange, w.account_type);
                    let metrics = self.core.screener_metrics.get(&key);
                    let (rsi14, atr_pct14, dist_high_pct, dist_low_pct) = match metrics {
                        Some(m) => (m.rsi14, m.atr_pct14, m.dist_high_pct, m.dist_low_pct),
                        None => (None, None, None, None),
                    };
                    ScreenerItem {
                        symbol: w.symbol.clone(),
                        exchange: w.exchange.clone(),
                        account_type: w.account_type.clone(),
                        address: w.address.clone(),
                        last_price: w.last_price,
                        change_percent: w.change_percent,
                        // mini_ticker exposes only base-asset volume; approximate
                        // quote volume as base_volume * last_price — see
                        // `ScreenerItem::volume_24h_quote`'s doc comment.
                        volume_24h_quote: w.volume_24h * w.last_price,
                        rsi14,
                        atr_pct14,
                        dist_high_pct,
                        dist_low_pct,
                    }
                }).collect();
            }

            // --- Watchlist TICKER section (TapeSurf Ticker widget borrow,
            // owner order 2026-07-31): extended info for the ACTIVE
            // chart's instrument, pinned to the watchlist bottom.
            {
                use mlc_app_sidebar_content::types::TickerSectionData;
                self.sidebar_state.ticker_section = self
                    .panel_app
                    .panel_grid
                    .active_container_id()
                    .and_then(|cid| {
                        let symbol = self.symbol_for_chart(cid)?;
                        let exchange = self.exchange_for_chart(cid)?;
                        let at = self.account_type_for_chart(cid).unwrap_or_default();
                        if let Some(ticker) = self
                            .core
                            .mini_ticker
                            .get(&format!("{}:{}:{}", symbol, exchange, at))
                        {
                            let pct = ticker.price_change_percent;
                            // Absolute change back-derived from the percent
                            // (display only — the wire event carries no abs).
                            let change_abs = if (100.0 + pct).abs() > f64::EPSILON {
                                ticker.last_price * pct / (100.0 + pct)
                            } else {
                                0.0
                            };
                            Some(TickerSectionData {
                                symbol,
                                exchange,
                                account_type: at.to_string(),
                                last_price: ticker.last_price,
                                change_pct: pct,
                                change_abs,
                                spark: ticker.spark.iter().copied().collect(),
                            })
                        } else {
                            Some(TickerSectionData {
                                symbol,
                                exchange,
                                account_type: at.to_string(),
                                last_price: 0.0,
                                change_pct: 0.0,
                                change_abs: 0.0,
                                spark: Vec::new(),
                            })
                        }
                    });
            }

            // --- Watchlist market-counters block (funding rate + OI Δ%,
            // owner spec 2026-08-01): cross-venue rows for the ACTIVE
            // chart's instrument, stacked above the TICKER section.
            // Subscription is ALWAYS Futures, independent of the active
            // chart's own account type/exchange — see
            // `docs/mlc/research/oi-funding-mlc-wiring-2026-08-01.md`.
            {
                use mlc_app_panels::trading::order_flow::multi_dom::KNOWN_EXCHANGES;
                use mlc_app_sidebar_content::types::CounterRowData;

                let desired_symbol = self
                    .panel_app
                    .panel_grid
                    .active_container_id()
                    .and_then(|cid| self.symbol_for_chart(cid))
                    .filter(|s| !s.is_empty());

                if desired_symbol != self.counters_symbol {
                    if let Some(old_symbol) = self.counters_symbol.take() {
                        for venue in KNOWN_EXCHANGES {
                            if let Some(eid) = digdigdig3::ExchangeId::from_str(venue) {
                                self.core.unsubscribe_market_counters(
                                    eid, &old_symbol, digdigdig3::AccountType::FuturesCross,
                                );
                            }
                        }
                    }
                    if let Some(ref new_symbol) = desired_symbol {
                        for venue in KNOWN_EXCHANGES {
                            if let Some(eid) = digdigdig3::ExchangeId::from_str(venue) {
                                self.core.subscribe_market_counters(
                                    eid, new_symbol, digdigdig3::AccountType::FuturesCross,
                                );
                            }
                        }
                    }
                    self.counters_symbol = desired_symbol;
                }

                self.sidebar_state.counter_rows = match &self.counters_symbol {
                    Some(symbol) => {
                        let now_ms = web_time::SystemTime::now()
                            .duration_since(web_time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as i64)
                            .unwrap_or(0);
                        KNOWN_EXCHANGES.iter().filter_map(|venue| {
                            let key = mlc_backend::market_counters::market_counters_key(symbol, venue, "F");
                            let mc = self.core.market_counters.get(&key)?;
                            if mc.funding_rate.is_none() && mc.oi.is_none() {
                                return None;
                            }
                            // Clamp to 0 rather than go negative when the
                            // countdown target has just elapsed (render
                            // shows "-00:00:00" for one frame, not an
                            // underflow/garbage duration).
                            let countdown_secs = mc.next_funding_time_ms
                                .map(|t| ((t - now_ms) / 1000).max(0));
                            Some(CounterRowData {
                                venue: (*venue).to_string(),
                                funding_rate: mc.funding_rate,
                                countdown_secs,
                                d1h_pct: mc.oi_d1h_pct,
                                d24h_pct: mc.oi_d24h_pct,
                            })
                        }).collect()
                    }
                    None => Vec::new(),
                };
            }

            // --- Connectors: populate from active hub ---
            {
                use mlc_app_sidebar_content::types::ConnectorStatusItem;
                use mlc_app_sidebar_content::types::ConnectorGroup;

                self.sidebar_state.connector_items.clear();
                let active_ids = self.core.bridge_handle().pool().list_connected();

                // Union: active (REST-registered) ∪ persisted-disabled ids so a
                // user-disabled connector doesn't vanish from the panel — otherwise
                // there's nowhere to click to re-enable it. Order: actives first
                // (registration order), then disabled (alphabetical for stability).
                let disabled_ids: Vec<digdigdig3::ExchangeId> = {
                    let mut disabled: Vec<String> = self.sidebar_state.connector_enabled
                        .iter()
                        .filter(|(_, on)| !**on)
                        .map(|(k, _)| k.clone())
                        .collect();
                    disabled.sort();
                    disabled
                        .into_iter()
                        .filter_map(|s| digdigdig3::ExchangeId::from_str(&s))
                        .filter(|eid| !active_ids.contains(eid))
                        .collect()
                };
                let ordered_ids: Vec<digdigdig3::ExchangeId> = active_ids
                    .iter()
                    .copied()
                    .chain(disabled_ids.into_iter())
                    .collect();

                let metrics_map: std::collections::HashMap<String, (digdigdig3::core::types::ConnectorStats, usize)> =
                    self.core.agent_metrics_snapshot()
                        .into_iter()
                        .map(|(eid, stats, ws)| (eid.as_str().to_string(), (stats, ws)))
                        .collect();

                {
                    use mlc_app_sidebar_content::MetricsSnapshot;
                    let now = web_time::Instant::now();
                    let should_sample = self.sidebar_state.metrics_last_sample
                        .is_none_or(|last| now.duration_since(last).as_secs_f64() >= 1.0);
                    if should_sample {
                        self.sidebar_state.metrics_last_sample = Some(now);
                        for (exchange_id, (stats, ws_count)) in &metrics_map {
                            self.sidebar_state.push_metrics_sample(exchange_id, MetricsSnapshot {
                                http_requests: stats.http_requests,
                                http_errors: stats.http_errors,
                                latency_ms: stats.last_latency_ms,
                                rate_used: stats.rate_used,
                                rate_max: stats.rate_max,
                                ws_count: *ws_count,
                                ws_ping_rtt_ms: stats.ws_ping_rtt_ms,
                            });
                        }
                    }
                }

                for exchange_id in &ordered_ids {
                    let hub = self.core.bridge_handle().pool();
                    let at = digdigdig3::AccountType::Spot;
                    // hub.rest returns None for disabled connectors (removed by
                    // hub.shutdown on toggle) — for those we still render a card
                    // with the exchange name from the enum so the user can flip
                    // the toggle back on. Capabilities are `None`, all metrics 0.
                    let connector = hub.rest(*exchange_id);
                    let name = connector.as_ref().map(|c| c.exchange_name()).unwrap_or(exchange_id.as_str());
                    let md_caps = connector.as_ref().map(|c| c.market_data_capabilities(at));
                    let tr_caps = connector.as_ref().map(|c| c.trading_capabilities(at));
                    let ac_caps = connector.as_ref().map(|c| c.account_capabilities(at));

                    let mut item = ConnectorStatusItem::new(
                        exchange_id.as_str(),
                        name,
                    );

                    item.enabled = *self.sidebar_state.connector_enabled
                        .get(exchange_id.as_str())
                        .unwrap_or(&true);
                    item.expanded = *self.sidebar_state.connector_expanded
                        .get(exchange_id.as_str())
                        .unwrap_or(&false);

                    let has_ws = md_caps.map_or(false, |md| md.has_ws_klines || md.has_ws_trades || md.has_ws_orderbook);

                    // --- Rate-limit descriptors (static, declared per-connector) ---
                    // `RateLimitCapabilities` (via `ExchangeIdentity::rate_limit_capabilities`)
                    // has no direct auth_type/requires_api_key/free_tier/base_url/ws_url
                    // fields (those live in the pub(crate) registry, unreachable from
                    // here) — those five stay at construction default. Only the
                    // rate-limit numbers are derived from the primary REST pool.
                    if let Some(primary) = connector
                        .as_ref()
                        .map(|c| c.rate_limit_capabilities())
                        .as_ref()
                        .and_then(|rc| rc.rest_pools.first())
                    {
                        item.rate_window_seconds = primary.window_seconds;
                        let per_minute = if primary.window_seconds > 0 {
                            Some((primary.max_budget as u64 * 60 / primary.window_seconds as u64) as u32)
                        } else {
                            None
                        };
                        if primary.is_weight {
                            item.weight_per_minute = per_minute;
                        } else {
                            item.rate_limit_per_minute = per_minute;
                            if primary.window_seconds <= 1 {
                                item.rate_limit_per_second = Some(primary.max_budget);
                            }
                        }
                    }

                    item.group = if md_caps.map_or(true, |md| !md.has_klines) {
                        ConnectorGroup::NonChartData
                    } else {
                        ConnectorGroup::NoApiKey
                    };

                    if let Some(md) = md_caps {
                        item.has_klines = md.has_klines;
                        item.has_trades = md.has_recent_trades;
                        item.has_orderbook = md.has_orderbook;
                        item.has_aggregated_bars = md.has_klines;
                    }
                    if let Some(md) = md_caps {
                        item.has_ws_klines = md.has_ws_klines;
                        item.has_ws_trades = md.has_ws_trades;
                        item.has_ws_orderbook = md.has_ws_orderbook;
                    }
                    if let Some(tr) = tr_caps {
                        item.has_trading = tr.has_market_order || tr.has_limit_order;
                    }
                    if let Some(ac) = ac_caps {
                        item.has_account = ac.has_balances;
                    }
                    if let Some(ac) = ac_caps {
                        item.has_positions = ac.has_positions;
                    }

                    item.rest_status = "active".to_string();
                    item.ws_status = if has_ws {
                        "available".to_string()
                    } else {
                        "n/a".to_string()
                    };

                    item.kline_batch_size = md_caps
                        .and_then(|md| md.max_kline_limit)
                        .unwrap_or(0);

                    item.supported_timeframes = md_caps
                        .map(|md| md.supported_intervals.iter().map(|s| s.to_string()).collect())
                        .unwrap_or_default();

                    if let Some((stats, ws_count)) = metrics_map.get(exchange_id.as_str()) {
                        item.ws_active_count = *ws_count;
                        item.http_requests_total = stats.http_requests;
                        item.http_errors_total = stats.http_errors;
                        item.last_latency_ms = stats.last_latency_ms;
                        item.rate_used = stats.rate_used;
                        item.rate_max = stats.rate_max;
                        item.rate_groups = stats.rate_groups.clone();
                        item.ws_ping_rtt_ms = stats.ws_ping_rtt_ms;
                    }

                    // --- Health dots: derived from real REST error ratio, not the
                    // enabled toggle bit. Simple first cut per owner spec:
                    //   disabled            -> red   (rest_healthy = false)
                    //   enabled + no traffic-> GREEN (rest_healthy = true; connector
                    //                          registered = REST endpoint reachable in
                    //                          principle, no dial errors on record).
                    //                          Prior rule required http_requests_total>0
                    //                          which painted every idle-but-live
                    //                          connector amber/red — regression, fixed.
                    //   errors/requests<5%  -> green (rest_healthy = true)
                    //   errors/requests>=5% -> amber (partial-health branch in render.rs)
                    item.rest_healthy = item.enabled
                        && (item.http_requests_total == 0
                            || (item.http_errors_total as f64)
                                < (item.http_requests_total as f64) * 0.05);

                    // WS liveness: `has_ws` is a declared capability, not a live socket
                    // check. Green when we have some live signal (ping RTT or active
                    // WS task count > 0) OR when the connector is enabled and the
                    // capability exists and there simply hasn't been traffic yet
                    // (idle-but-live, same rationale as REST above).
                    item.ws_connected = item.enabled && has_ws;

                    item.show_metrics = *self.sidebar_state.connector_metrics_visible
                        .get(exchange_id.as_str())
                        .unwrap_or(&false);

                    if let Some(history) = self.sidebar_state.metrics_history.get(exchange_id.as_str()) {
                        item.metrics_history = history.iter().cloned().collect();
                    }

                    self.sidebar_state.connector_items.push(item);
                }
            }

            // --- Alerts: copy from alert manager ---
            self.sidebar_state.alert_items = self.core.alerts().items_all();

            // --- ObjectTree: mark items that have bound alerts ---
            {
                let alert_items: Vec<mlc_core::alert::AlertItem> = self.core.alerts().items_all();
                let alert_items = alert_items.as_slice();
                for tree_item in &mut self.sidebar_state.object_tree_items {
                    tree_item.has_alert = alert_items.iter().any(|a| match &a.source {
                        mlc_core::alert::AlertSource::Drawing { primitive_id, .. } => {
                            *primitive_id == tree_item.id
                        }
                        mlc_core::alert::AlertSource::Indicator { indicator_id, .. } => {
                            *indicator_id == tree_item.id
                        }
                        mlc_core::alert::AlertSource::Signal { indicator_id, .. } => {
                            *indicator_id == tree_item.id
                        }
                        _ => false,
                    });
                }
            }

            self.sidebar_data_dirty = false;
        }

        // Keep symbol / compare search results filtered by current query so
        // render_to_scene (which takes &self) can read the pre-filtered list.
        if self.modal_state.current == OpenModal::SymbolSearch
            || self.modal_state.current == OpenModal::CompareSearch
        {
            let query = self.modal_state.search_query.clone();
            self.modal_state.symbol_search_results =
                Self::build_demo_symbol_results(&query, self.core.watchlists().find(self.watchlist_ui.active_list_id), self.core.exchange_symbols().as_map());
        }

        // --- Split-pane layout and viewport sync ---
        //
        // When the grid is split, call panel_grid.layout() so sub-chart rects
        // are computed, then sync each leaf window's viewport dimensions so that
        // bar_to_x, visible_range, and crosshair calculations are correct.
        // Also sync group primitives into each window's drawing_manager.
        let sidebar_w = self.sidebar_state.right_width();
        let window_rect = LayoutRect::new(0.0, 0.0, width, height);
        let panel_layout_pf = ChartPanelLayout::compute(&window_rect, &self.panel_app.toolbar_config);
        let content_rect_pf = {
            let mut r = panel_layout_pf.content_rect;
            r.width = (r.width - sidebar_w).max(0.0);
            r
        };

        // Always recompute panel_grid layout — even for single-leaf charts.
        // panel_rects is the source of truth for leaf rectangles used by
        // get_leaf_absolute_rect, build_extended_layout_for_leaf, the drag
        // separator handler, and several hit-testers. Pre-cutover the
        // single-leaf renderer used a separate fast path that didn't need
        // panel_rects, so the layout call was guarded behind is_split().
        // After backend cutover any code that asks panel_rects for a leaf
        // rect (e.g. sub-pane separator drag) gets None in single-leaf mode
        // and silently breaks. Call layout() unconditionally.
        let single_or_split_rect = mlc_core::PanelRect {
            x: 0.0,
            y: 0.0,
            width: content_rect_pf.width as f32,
            height: content_rect_pf.height as f32,
        };
        self.panel_app.panel_grid.layout(single_or_split_rect);

        if self.panel_app.panel_grid.is_split() {
            let leaf_rects: Vec<_> = self.panel_app.panel_grid.panel_rects()
                .iter()
                .map(|(&leaf_id, &sub_rect)| (leaf_id, sub_rect))
                .collect();

            // Sync viewport.chart_width/chart_height for all split windows.
            // Pre-compute target dimensions using immutable borrows (build_extended_layout_for_leaf
            // needs &self), then apply with mutable borrows in a second pass.
            let leaf_dims: Vec<(mlc_core::LeafId, f64, f64)> = leaf_rects.iter()
                .filter_map(|&(leaf_id, sub_rect)| {
                    let leaf_layout_rect = LayoutRect {
                        x: content_rect_pf.x + sub_rect.x as f64,
                        y: content_rect_pf.y + sub_rect.y as f64,
                        width: sub_rect.width as f64,
                        height: sub_rect.height as f64,
                    };
                    // build_extended_layout_for_leaf accounts for sub-panes so that
                    // chart_height reflects only the main chart area, matching the
                    // render path and eliminating the hit-test Y offset.
                    let extended = self.build_extended_layout_for_leaf(leaf_id, &leaf_layout_rect)?;
                    Some((leaf_id, extended.main_chart.chart.width, extended.main_chart.chart.height))
                })
                .collect();

            for (leaf_id, new_chart_w, new_chart_h) in leaf_dims {
                if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                    // Skip bar_shift when window hasn't been snapped yet (still has
                    // placeholder chart_width from Viewport::default).  The deferred
                    // snap below will compute view_start with the real chart_width.
                    // chart_width/chart_height are always updated regardless.
                    window.resize_geometry(new_chart_w, new_chart_h);
                }
            }

            // Deferred viewport snap for split mode: now chart_width is real.
            {
                let mut snapped_windows: Vec<(ChartId, f64, f64)> = Vec::new();
                // Audit D8: snapshot each chart's own active series.
                let series_by_chart_split: std::collections::HashMap<ChartId, crate::active_series::OwnedActiveSeries> =
                    self.panel_app.panel_grid.containers().keys()
                        .map(|cid| (*cid, self.active_series_for_chart(*cid)))
                        .collect();
                let type_by_chart_split: std::collections::HashMap<ChartId, &'static str> =
                    self.panel_app.panel_grid.containers().keys()
                        .filter_map(|cid| self.chart_type_for_chart(*cid).map(|ct| (*cid, ct)))
                        .collect();
                for (&chart_id, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                    let series_split = series_by_chart_split.get(&chart_id)
                        .map(|s| s.as_active_series())
                        .unwrap_or(crate::active_series::ActiveSeries::Bars(&[]));
                    if let Some((view_start, mut bar_spacing)) = window.consume_deferred_snap(series_split) {
                        // Same per-type min-spacing rule as the non-split
                        // site — hydration bypasses ChangeChartType.
                        if let Some(min) = type_by_chart_split.get(&chart_id)
                            .and_then(|ct| crate::input::chart_out_events::min_usable_bar_spacing(ct))
                        {
                            window.ensure_min_bar_spacing(min);
                            bar_spacing = window.viewport().bar_spacing;
                        }
                        snapped_windows.push((chart_id, view_start, bar_spacing));
                    }
                    // Y-invariant (2026-07-04): split/resize changed this
                    // leaf's width → refit Auto-mode Y to its new visible
                    // window (both panes of a fresh split hit this — they
                    // inherit a Y computed for the pre-split width).
                    let series_y = series_by_chart_split.get(&chart_id)
                        .map(|s| s.as_active_series())
                        .unwrap_or(crate::active_series::ActiveSeries::Bars(&[]));
                    window.consume_y_rescale(series_y);
                }
                // (diagnostic logging removed — snap-to-end confirmed working)
                for (chart_id, view_start, bar_spacing) in snapped_windows {
                    if let Some(leaf_id) = self.panel_app.panel_grid.leaf_for_container_id(chart_id) {
                        self.propagate_viewport_to_sync_group(leaf_id, view_start, bar_spacing, None);
                    }
                }
            }

            // Group primitive sync into split windows — DEAD under the
            // per-bubble drawing model (Step 3.1). Peer windows sharing a
            // tag already share `current_tag_id` (`do_split` sets
            // `window.current_tag_id = group_id` for every peer leaf),
            // so `drawings().primitives(bid)` already returns the exact
            // same `DataBubble.primitives` Vec for every peer — no clone,
            // no pane_id remap, no per-frame copy needed. Indicator
            // instances are bubble-owned the same way, so the sub-pane
            // instance-id remap this block used to compute is moot too.

            // Sync indicator overlay visibility per leaf.
            for &(leaf_id, _) in &leaf_rects {
                let has_compare = self.panel_app.panel_grid.container_for_leaf(leaf_id)
                    .map(|w| !w.compare_overlay.series.is_empty())
                    .unwrap_or(false);
                let bid = self.panel_app.panel_grid.container_for_leaf(leaf_id)
                    .map(|w| w.current_tag_id);
                let has_indicators = bid
                    .map(|b| !self.core.indicators().instances_for_bubble(b).is_empty())
                    .unwrap_or(false);
                let state = self.panel_app.indicator_overlay_state_for_leaf_mut(leaf_id);
                state.visible = has_indicators || has_compare;
            }
        } else {
            // Single pane: group-primitive sync — DEAD under the per-bubble
            // drawing model (Step 3.1). The active window's `current_tag_id`
            // already equals `group_id`; `drawings().primitives(bid)` reads
            // `group.primitives` directly, no per-frame clone needed.

            // Single pane: sync indicator overlay visibility.
            let has_compare = self.panel_app.panel_grid.active_container()
                .map(|w| !w.compare_overlay.series.is_empty())
                .unwrap_or(false);
            let has_indicators = self.panel_app.panel_grid.active_container()
                .map(|w| !self.core.indicators().instances_for_bubble(w.current_tag_id).is_empty())
                .unwrap_or(false);
            self.panel_app.indicator_overlay_state.visible = has_indicators || has_compare;
        }

        // Sync sub-pane pixel geometry into window.sub_panes so that
        // PanSubPane handlers and other &mut code see up-to-date values.
        self.sync_sub_pane_geometry();

        // Snapshot agent state for the sidebar renderer (agents panel).
        // Done here in prepare_frame (&mut self) because render_to_scene takes &self.
        // Iterate all registered agent leaves and snapshot each instance.
        {
            let leaf_ids: Vec<uzor::panels::LeafId> = self.sidebar_state.agent_leaves.keys().copied().collect();
            for leaf_id in leaf_ids {
                if let Some(desc) = self.sidebar_state.agent_leaves.get(&leaf_id).cloned() {
                    if let Some(snap) = self.agent.snapshot_instance(desc.instance_id) {
                        self.sidebar_state.agent_leaf_snapshots.insert(leaf_id, snap);
                    }
                }
            }
        }
    }
}

/// Classify a venue feed-error message into a short, stable class label.
///
/// `None` for a message shape this classifier doesn't recognize yet — both
/// renderings below fall back to a raw clip of `msg` itself in that case.
///
/// The single decision point behind BOTH user-facing renderings (owner spec
/// 2026-08-30, revised: "the watchlist row and the chart draw from ONE
/// classification decision" — `classify_watchlist_feed_error` renders the
/// label alone for the narrow sidebar column; `classify_chart_feed_error`
/// renders the label PLUS the literal cause for the ~450px chart pane,
/// which has room for a full sentence and — per the owner's own read of
/// `render_chart.rs`'s existing comment — is meant to show it). Splitting
/// this out is also what stops the two renderings from drifting into two
/// independent keyword lists.
///
/// Checks are ordered most-specific-first so an unambiguous signal (a venue
/// body code, an exact wire phrase) is never shadowed by a broader keyword
/// match lower down. Message shapes are wire-verified against
/// `digdigdig3::core::types::common::ExchangeError`'s `Display` impl
/// (`"API error: {code} - {message}"`, `"Network error: ..."`,
/// `"Timeout: ..."`, `"Rate limit exceeded..."`, ...) plus the wrappers
/// `mlc-backend-data-bridge` adds on top: `"warmup failed: {err}"` (bulk
/// connector warm-up, empty symbol), a bare `err.to_string()` (on-demand
/// `ensure_connector` dial failure, empty symbol), and
/// `Bridge::phase_a_fail_message`'s three Phase-A-REST-failure shapes —
/// `"{exch} klines unavailable: {err} (proxy down?)"`,
/// `"{exch} klines unavailable: {err}. Paid relay proxy is planned for
/// geo-blocks."`, `"{exch} needs the data proxy (paid). ..."` — plus its
/// sibling terminal-empty message `"{exch} klines unavailable: venue
/// returned no bars for {symbol} {interval}"` (`bridge.rs:2331`), which is
/// checked AHEAD of the generic "klines unavailable" match below because it
/// is a genuinely different class: the venue answered, it just had nothing.
fn classify_label(msg: &str) -> Option<&'static str> {
    let l = msg.to_lowercase();

    // Geo-blocked / region-restricted. Covers a plain HTTP 403 AND the
    // KuCoin shape actually seen from the production egress: HTTP 200 with
    // body `{"code":"400302","msg":"IP is from restricted area"}`, which
    // `digdigdig3`'s KuCoin connector already turns into a proper `Err`
    // (`ExchangeError::Api{code:400302,..}`) rather than reading it as
    // success — this classifier just has to recognize the resulting text.
    // Which side is blocked is decided upstream, where the routing is known:
    // a must-proxy venue was dialed by our server, anything else left the
    // user's own browser (`DataBridge::phase_a_fail_message`). A geo-block is
    // a final answer here — we do not relay around it (owner 2026-08-30) — so
    // the row has to say whose region it is.
    if l.contains("geo-blocks our server") {
        return Some("Geo-blocked at our server");
    }
    if l.contains("geo-blocks your region") {
        return Some("Geo-blocked in your region");
    }

    if l.contains("400302")
        || l.contains("restricted area")
        || l.contains("403")
        || l.contains("forbidden")
        // Anchored spellings only — a bare "geo" substring also matches the
        // ticker GEO (GEOUSDT), which would report a missing symbol as a
        // geo-block and send the reader hunting for an egress that is fine.
        || l.contains("geo-block")
        || l.contains("geoblock")
        || l.contains("geo block")
        || l.contains("geographic")
        || l.contains("geo restriction")
        || l.contains("region restricted")
        || l.contains("not available in your")
        || l.contains("cloudfront")
    {
        return Some("Geo-blocked by venue");
    }

    // Rate limited — checked before the generic API-error bucket so a 429
    // body reads as a limit, not a mystery error.
    if l.contains("rate limit") || l.contains("429") || l.contains("too many requests") {
        return Some("Rate limited");
    }

    // Symbol rejected by the venue (HTX: concatenated "ETHUSDT" -> "invalid
    // symbol").
    if l.contains("invalid symbol") || l.contains("symbol not found") || l.contains("unknown symbol") {
        return Some("Symbol not on venue");
    }

    // Timeframe rejected by the venue (HTX: `period=1h` -> "invalid
    // period").
    if l.contains("invalid period") || l.contains("invalid interval") || l.contains("invalid timeframe") {
        return Some("Timeframe not supported");
    }

    // The venue answered but sent zero bars (`bridge.rs:2331`'s terminal-
    // empty path, dig3's own retry budget already spent) — a different
    // class from "couldn't reach the venue at all", so it must be caught
    // ahead of the generic "klines unavailable" wrapper match below (this
    // message shape contains that same wrapper text).
    if l.contains("returned no bars") {
        return Some("No data returned by venue");
    }

    // The connector itself never came up. `"warmup failed: {err}"` is the
    // bulk connector warm-up's own wrapper text (`Bridge::
    // warmup_all_connectors`) — recognized ahead of the generic
    // network/unreachable bucket below so this reads as "the venue never
    // started" rather than a mid-session drop.
    if l.contains("warmup failed") {
        return Some("Connector failed to start");
    }

    // Unreachable — transport failed before any venue response existed to
    // classify (wasm's CORS `"Failed to fetch"`, an on-demand connector
    // dial that failed outright), OR the bridge's own Phase-A-REST wrapper
    // said so in plain words: `"klines unavailable"` / `"(proxy down?)"` /
    // `"needs the data proxy"` (`Bridge::phase_a_fail_message`) — added
    // 2026-08-30 after a dead local CORS proxy (`:17800`) produced an inner
    // `{err}` text (wasm fetch failure) that matched none of the patterns
    // above; the OUTER wrapper the bridge always adds is the reliable,
    // transport-independent signal, so match on that instead of chasing
    // every possible inner error string.
    if l.contains("failed to fetch")
        || l.contains("network error")
        || l.contains("cors")
        || l.contains("blocked")
        || l.contains("connection error")
        || l.contains("not connected")
        || l.contains("timeout")
        || l.contains("klines unavailable")
        || l.contains("proxy down")
        || l.contains("needs the data proxy")
    {
        return Some("Venue unreachable");
    }

    None
}

/// Short watchlist-row copy for a venue feed error (Last/spark
/// replacement). The class label alone — the column is narrow and the
/// class is the useful part (owner spec 2026-08-30: "This is right today,
/// keep it."). Falls back to a word-boundary clip of the raw message when
/// [`classify_label`] doesn't recognize the shape.
pub(crate) fn classify_watchlist_feed_error(msg: &str) -> String {
    match classify_label(msg) {
        Some(label) => label.to_string(),
        None => clip_on_word_boundary(msg.trim(), 32),
    }
}

/// Chart empty-state copy for a venue feed error (`ChartContainer::
/// data_error`, drawn by `render_full_chart_panel` —
/// `mlc-app-layout/src/layout/render_chart.rs`). The class label PLUS the
/// literal cause: the chart pane has room for a full sentence, and
/// downgrading it to the row's short label alone threw away exactly the
/// cause the user needs (owner spec 2026-08-30, e.g. `"Venue unreachable —
/// Bitfinex klines unavailable: <err> (proxy down?)"`).
///
/// Returns the FULL composed string, uncapped — a fixed character budget
/// here overshot on a narrow pane and ran the text off both edges (live
/// regression, 2026-08-30 round 2: a 120-char budget on a ~450px pane).
/// Composition (this function) and fitting are two different jobs: this
/// function only decides WHAT the text says; wrapping it to the pane's
/// actual measured width belongs at the draw site where that width is
/// known (`mlc-app-layout::layout::render_chart::wrap_and_ellipsize`).
pub(crate) fn classify_chart_feed_error(msg: &str) -> String {
    let cause = strip_wrapper_noise(msg.trim());
    match classify_label(msg) {
        Some(label) => format!("{} — {}", label, cause),
        None => cause,
    }
}

/// Plumbing-layer prefixes that tell the user nothing about WHAT failed —
/// stripped entirely from the front of the chart's cause text. Fixed list,
/// exact match only (owner spec 2026-08-30 round 2: "keep this
/// conservative... do not attempt clever parsing").
const PLUMBING_SEGMENTS: &[&str] = &["core error", "get_klines failed"];

/// Strip leading plumbing prefixes, then collapse an exact-repeat
/// `"<segment>: <segment>: "` prefix down to one copy. Each connector layer
/// re-wraps the error from the layer below it, so the SAME segment name can
/// end up doubled at the front — e.g. `"core error: get_klines failed:
/// Invalid request: Invalid request: <the actual venue text>"`. Plumbing
/// prefixes are dropped outright (`"core error:"`, `"get_klines failed:"`);
/// a repeated segment like `"Invalid request:"` is real information and
/// kept, just de-duplicated to one copy. Conservative on purpose: exact
/// prefix matches only, no parsing of the message's structure.
fn strip_wrapper_noise(msg: &str) -> String {
    // Segment-wise, not prefix-wise: each layer wraps the one below as
    // "<layer>: <inner>", so the plumbing lands in the MIDDLE of the string
    // ("Bitfinex klines unavailable: core error: get_klines failed: ...").
    // Anchoring at the front only, as the first version did, matched nothing
    // and left the duplicate visible on the chart.
    // Split/rejoin on the same separator is lossless, so a segment that
    // legitimately contains a colon inside parentheses survives intact.
    let mut out: Vec<&str> = Vec::new();
    for seg in msg.split(": ") {
        let seg = seg.trim();
        if seg.is_empty() || PLUMBING_SEGMENTS.contains(&seg) {
            continue;
        }
        // Collapse an immediate repeat of the layer below ("Invalid
        // request: Invalid request: ...").
        if out.last() == Some(&seg) {
            continue;
        }
        out.push(seg);
    }
    out.join(": ")
}

/// Clip `text` to at most `max_chars`, breaking at the last whitespace
/// boundary within budget instead of mid-word, with a trailing ellipsis.
/// Falls back to a hard character cutoff only when `text` has no whitespace
/// at all within budget (a single long token — no boundary exists to back
/// off to).
fn clip_on_word_boundary(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_string();
    }
    let hard_cut = max_chars.saturating_sub(1); // room for the ellipsis
    let cut = chars[..hard_cut].iter().rposition(|c| c.is_whitespace()).unwrap_or(hard_cut);
    let clipped: String = chars[..cut].iter().collect();
    format!("{}…", clipped.trim_end())
}
