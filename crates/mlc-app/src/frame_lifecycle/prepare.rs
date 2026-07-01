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
        self.panel_app.user_settings_state.recalc_mode_label = match self.core.indicators().recalc_mode() {
            RecalcMode::PerTick  => "Per Tick".to_string(),
            RecalcMode::PerFrame => "Per Frame".to_string(),
            RecalcMode::PerBar   => "Per Bar".to_string(),
        };
        // Sync diagnostics flag so the checkbox reflects the current state.
        self.panel_app.user_settings_state.diagnostics_enabled = self.diagnostics_enabled;
        // Sync data_load settings into user_settings_state for the DATA & CACHE sliders.
        // Only update the cached values when the slider is not being dragged so the
        // handle does not snap back to the committed value on every frame during drag.
        if !self.panel_app.user_settings_state.is_data_slider_dragging() {
            let dl = &self.panel_app.user_manager.profile.data_load;
            self.panel_app.user_settings_state.data_bg_bars      = dl.background_bar_count;
            self.panel_app.user_settings_state.data_max_bars     = dl.max_loaded_bars;
            self.panel_app.user_settings_state.data_store_size_mb = dl.max_store_size_mb;
            self.panel_app.user_settings_state.data_cleanup_days  = dl.store_cleanup_days;
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
            // Snapshot bars per chart up front to avoid borrow conflict during the
            // mut-iter loop below.
            let bars_by_chart_pre: std::collections::HashMap<ChartId, Vec<mlc_core::Bar>> =
                self.panel_app.panel_grid.containers().iter()
                    .map(|(cid, w)| {
                        let bars = self.core.bubbles().bars(w.current_bubble_id).to_vec();
                        (*cid, bars)
                    })
                    .collect();
            for (&chart_id, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                // needs_snap_after_backfill alone (without needs_auto_scale_after_bars)
                // happens for startup-parked presets: update_bars() delivered bars
                // while chart_width was still 0, and update_bars never sets the
                // deferred-autoscale flag. Consume the cold-snap here once width
                // is known so the flag cannot strand.
                let bars_pre: &[mlc_core::Bar] = bars_by_chart_pre.get(&chart_id)
                    .map(|v| v.as_slice()).unwrap_or(&[]);
                if (window.needs_auto_scale_after_bars || window.needs_snap_after_backfill)
                    && !bars_pre.is_empty()
                    && window.viewport.chart_width > 0.0
                {
                    window.needs_auto_scale_after_bars = false;
                    if window.needs_snap_after_backfill {
                        // Cold-restore snap supersedes the plain deferred snap:
                        // consume_cold_snap snaps + force-autoscales + clears the flag.
                        window.consume_cold_snap(bars_pre);
                    } else {
                        // Snap to end with standard margin.
                        window.snap_to_end(mlc_core::DEFAULT_SNAP_MARGIN);
                        // No snap_cooldown needed: snap fires in prepare_frame AFTER
                        // sync_viewport_from_layout, so bar_shift cannot undo it this frame.
                        // Next frame old_width == new_width → bar_shift = 0.
                        window.calc_auto_scale(bars_pre);
                    }
                    // restore_scale_mode is already consumed inside set_bars() for the eager
                    // path; for the deferred path (chart_width was 0 at set_bars time),
                    // consume it here.
                    if let Some(mode) = window.restore_scale_mode.take() {
                        window.price_scale.scale_mode = mode;
                    }
                    snapped_windows.push((chart_id, window.viewport.view_start, window.viewport.bar_spacing));
                }
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
        // dirty render so we can see whether `window.current_bubble_id` lines
        // up with the bubble that actually has bars.  Owner repro: split or
        // new-preset sibling chart renders empty even though
        // `[do_split-key]` shows the auto_bubble was seeded with the source's
        // identity.  Suspicion: window.current_bubble_id points at a stale
        // bubble id while the data lands in a different bubble.
        {
            let mut dump: Vec<String> = Vec::new();
            let bubble_mgr = self.core.bubbles();
            for (cid, w) in self.panel_app.panel_grid.containers().iter() {
                let cur_bid = w.current_bubble_id;
                let auto_bid = w.auto_bubble_id;
                let bid_via_chart = bubble_mgr.bubble_for_chart(*cid);
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

        for (_cid, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
            if window.viewport.is_time_axis() {
                window.viewport.disable_time_axis();
            }
            // Refresh the Heikin Ashi memo if the chart panel is on
            // chart_type == "heikin_ashi" AND `bars` mutated since the
            // last frame. Cheap when stamp matches (single i64
            // comparison + Option check).
            let is_ha = self.panels_store.chart_panel
                .get(&window.chart_panel_id)
                .map(|p| p.chart_type == "heikin_ashi")
                .unwrap_or(false);
            let bubble_bars: &[mlc_core::Bar] = bubble_snapshot
                .get(&window.current_bubble_id)
                .map(|(b,)| b.as_slice())
                .unwrap_or(&[]);
            if is_ha {
                // Cheap proxy bubble carrying just the bars slice — ensure_ha_cache
                // only reads bars + stamp. Avoids re-borrowing self.managers.bubble.
                window.ensure_ha_cache_from_bars(bubble_bars);
            }

            // Volume Profile prepass — recompute when the visible slice, mode,
            // or bars change. Cheap when the stamp matches (one tuple comparison).
            if window.vp_show {
                let (vis_start, vis_end) = window.viewport.visible_range();
                let vis_end_clamped = vis_end.min(bubble_bars.len());
                let visible_owned: Vec<mlc_core::Bar> = if vis_start <= vis_end_clamped {
                    bubble_bars[vis_start..vis_end_clamped].to_vec()
                } else {
                    Vec::new()
                };
                window.ensure_vp_from_bars(bubble_bars, &visible_owned, 24);
            }
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
                    let sym = self.symbol_for_chart(active_cid_sync)
                        .map(String::from).unwrap_or_default();
                    let exch = self.exchange_for_chart(active_cid_sync)
                        .map(String::from).unwrap_or_default();
                    let at = self.account_type_for_chart(active_cid_sync)
                        .map(String::from).unwrap_or_else(|| "S".to_string());
                    self.core.bubbles_mut().set_bubble_instrument(new_active_group, exch, at);
                    self.core.bubbles_mut().with_bubble_mut(new_active_group, |g| g.address.symbol = sym);
                }
                self.core.bubbles_mut().reassign_synced_panels(new_active_group);
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
                .and_then(|gid| self.with_bubble(gid, |g| {
                    if g.auto_created {
                        None
                    } else {
                        Some(TaggedGroupSnapshot {
                            sync_flags: g.sync_flags.clone(),
                            primitives: g.primitives.iter().map(|p| p.clone_box()).collect(),
                        })
                    }
                }).flatten());

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
                    .map(String::from).unwrap_or_else(|| "S".to_string());
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
                // whose `bubble_id == auto_bubble_id`) whenever the active
                // chart is in a tagged bubble distinct from its home auto.
                // Owner rule: Window section reflects the chart's own
                // stash; current Group section reflects the bubble the
                // chart is currently pointing at.  When both equal
                // (untagged), only the Group section is shown.
                let (active_auto_bid, active_cur_bid) = active_cid
                    .and_then(|cid| self.panel_app.panel_grid.containers().get(&cid))
                    .map(|w| (Some(w.auto_bubble_id), Some(w.current_bubble_id)))
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
                // matches auto_bubble_id.  Collect them live from the
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
                        if let Some(inst) = self.core.indicators().instance(iid)
                        {
                            let item = mlc_app_sidebar_content::types::ObjectTreeItem::new(
                                inst.id,
                                &inst.name,
                                mlc_core::ObjectCategory::Indicator,
                                &inst.type_id,
                            )
                            .with_visible(inst.visible)
                            .with_locked(inst.locked)
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
                    .map(|w| w.current_bubble_id)
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
                let watchlist_entries: Vec<(String, String, String)> = self
                    .core
                    .watchlists()
                    .find(self.watchlist_ui.active_list_id)
                    .map(|list| {
                        list.all_symbols()
                            .iter()
                            .map(|ws| (ws.symbol.clone(), ws.exchange.clone(), ws.account_type.as_label().to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                for (sym_name, sym_exchange, sym_account_type) in &watchlist_entries {
                    let price_data = self.panel_app.panel_grid.containers().iter()
                        .find(|(cid, _)| {
                            self.symbol_for_chart(**cid)
                                .map_or(false, |s| &s == sym_name)
                            && self.exchange_for_chart(**cid)
                                .map_or(false, |e| &e == sym_exchange)
                            && self.account_type_for_chart(**cid)
                                .map_or(false, |a| &a == sym_account_type)
                        })
                        .and_then(|(_, _w)| None::<&mlc_core::Bar>)
                        .map(|bar| (bar.close, bar.open, bar.high, bar.low, bar.volume));

                    if let Some((price, open, high, low, volume)) = price_data {
                        let change_pct = if open != 0.0 {
                            (price - open) / open * 100.0
                        } else {
                            0.0
                        };
                        self.sidebar_state.watchlist_items.push(WatchlistItem {
                            symbol: sym_name.clone(),
                            exchange: sym_exchange.clone(),
                            last_price: price,
                            change_percent: change_pct,
                            high_24h: high,
                            low_24h: low,
                            volume_24h: volume,
                            account_type: sym_account_type.clone(),
                        });
                    } else if let Some(ticker) = self.core.mini_ticker.get(&format!("{}:{}:{}", sym_name, sym_exchange, sym_account_type)) {
                        self.sidebar_state.watchlist_items.push(WatchlistItem {
                            symbol: sym_name.clone(),
                            exchange: sym_exchange.clone(),
                            last_price: ticker.last_price,
                            change_percent: ticker.price_change_percent,
                            high_24h: ticker.high_price,
                            low_24h: ticker.low_price,
                            volume_24h: ticker.volume,
                            account_type: sym_account_type.clone(),
                        });
                    } else {
                        self.sidebar_state.watchlist_items.push(WatchlistItem {
                            symbol: sym_name.clone(),
                            exchange: sym_exchange.clone(),
                            last_price: 0.0,
                            change_percent: 0.0,
                            high_24h: 0.0,
                            low_24h: 0.0,
                            volume_24h: 0.0,
                            account_type: sym_account_type.clone(),
                        });
                    }
                }
            }

            // --- Connectors: populate from active hub ---
            {
                use mlc_app_sidebar_content::types::ConnectorStatusItem;
                use mlc_app_sidebar_content::types::ConnectorGroup;

                self.sidebar_state.connector_items.clear();
                let active_ids = self.core.bridge.pool().list_connected();

                let metrics_map: std::collections::HashMap<String, (digdigdig3::core::types::ConnectorStats, usize)> =
                    self.core.bridge.collect_metrics()
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

                for exchange_id in &active_ids {
                    let hub = self.core.bridge.pool();
                    let at = digdigdig3::AccountType::Spot;
                    let connector = match hub.rest(*exchange_id) {
                        Some(c) => c,
                        None => continue,
                    };

                    let name = connector.exchange_name();
                    let md_caps = Some(connector.market_data_capabilities(at));
                    let tr_caps = Some(connector.trading_capabilities(at));
                    let ac_caps = Some(connector.account_capabilities(at));

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
                    item.rest_healthy = item.enabled;

                    let has_ws = md_caps.map_or(false, |md| md.has_ws_klines || md.has_ws_trades || md.has_ws_orderbook);
                    item.ws_connected = item.enabled && has_ws;

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
                Self::build_demo_symbol_results(&query, self.core.watchlists().find(self.watchlist_ui.active_list_id), &self.core.exchange_symbols_read());
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
                    let old_w = window.viewport.chart_width;
                    // Skip bar_shift when window hasn't been snapped yet (still has
                    // placeholder chart_width from Viewport::default).  The deferred
                    // snap below will compute view_start with the real chart_width.
                    // chart_width/chart_height are always updated regardless.
                    if !window.needs_auto_scale_after_bars
                        && (old_w - new_chart_w).abs() > 0.5
                        && window.viewport.bar_spacing > 0.0
                        && old_w > 0.0
                    {
                        let bar_shift = (old_w - new_chart_w) / window.viewport.bar_spacing;
                        window.viewport.view_start += bar_shift;
                    }
                    window.viewport.chart_width = new_chart_w;
                    window.viewport.chart_height = new_chart_h;
                }
            }

            // Deferred viewport snap for split mode: now chart_width is real.
            {
                let mut snapped_windows: Vec<(ChartId, f64, f64)> = Vec::new();
                // Snapshot bars per chart.
                let bars_by_chart_split: std::collections::HashMap<ChartId, Vec<mlc_core::Bar>> =
                    self.panel_app.panel_grid.containers().iter()
                        .map(|(cid, w)| {
                            let bars = self.core.bubbles().bars(w.current_bubble_id).to_vec();
                            (*cid, bars)
                        })
                        .collect();
                for (&chart_id, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                    let bars_split: &[mlc_core::Bar] = bars_by_chart_split.get(&chart_id)
                        .map(|v| v.as_slice()).unwrap_or(&[]);
                    if window.needs_auto_scale_after_bars && !bars_split.is_empty() && window.viewport.chart_width > 0.0 {
                        window.needs_auto_scale_after_bars = false;
                        // Snap to end with standard margin,
                        // using CURRENT bar_spacing (restored from preset).
                        window.snap_to_end(mlc_core::DEFAULT_SNAP_MARGIN);
                        window.calc_auto_scale(bars_split);
                        if let Some(mode) = window.restore_scale_mode.take() {
                            window.price_scale.scale_mode = mode;
                        }
                        snapped_windows.push((chart_id, window.viewport.view_start, window.viewport.bar_spacing));
                    }
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
            // tag already share `current_bubble_id` (`do_split` sets
            // `window.current_bubble_id = group_id` for every peer leaf),
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
                    .map(|w| w.current_bubble_id);
                let has_indicators = bid
                    .map(|b| !self.core.indicators().instances_for_bubble(b).is_empty())
                    .unwrap_or(false);
                let state = self.panel_app.indicator_overlay_state_for_leaf_mut(leaf_id);
                state.visible = has_indicators || has_compare;
            }
        } else {
            // Single pane: group-primitive sync — DEAD under the per-bubble
            // drawing model (Step 3.1). The active window's `current_bubble_id`
            // already equals `group_id`; `drawings().primitives(bid)` reads
            // `group.primitives` directly, no per-frame clone needed.

            // Single pane: sync indicator overlay visibility.
            let has_compare = self.panel_app.panel_grid.active_container()
                .map(|w| !w.compare_overlay.series.is_empty())
                .unwrap_or(false);
            let has_indicators = self.panel_app.panel_grid.active_container()
                .map(|w| !self.core.indicators().instances_for_bubble(w.current_bubble_id).is_empty())
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
