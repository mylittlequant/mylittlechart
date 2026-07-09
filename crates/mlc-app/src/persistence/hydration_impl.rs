//! Cold-path preset apply (~1300 LOC, lifted out of
//! `chart_out_events.rs::LoadPreset`).
//!
//! Stage 3a: relocation only. Still runs synchronously on the main thread
//! when called from the `LoadPreset` handler. The next two stages will
//!  (3b) carve out the bridge-free pure portion into
//!       `hydrate_preset(&Preset) -> HydratedPresetState` + the bridge
//!       portion into `ignite_subscriptions`, and
//!  (4)  dispatch hydrate onto a worker thread under a `LoadState::Loading`
//!       gate while the host paints the skeleton.
//!
//! Behaviour-preserving move. No log-line text or step ordering changed.

use crate::ChartApp;
use mlc_core::preset::preset::ChartPreset;
use mlc_core::chart::types::price_scale::ScaleMode;

impl ChartApp {
    /// Apply a freshly cloned `ChartPreset` into the active window slot.
    /// Caller must have already parked the previous preset and advanced
    /// `active_preset_id` to the new id.
    ///
    /// `lp_total` is the per-LoadPreset wall-clock anchor — passed in so
    /// the trailing log line still measures "click → fully restored" the
    /// same way as before the relocation.
    pub(crate) fn apply_preset_cold(
        &mut self,
        mut preset: ChartPreset,
        _lp_total: web_time::Instant,
        lp_materialized_us: u128,
        lp_autosave_us: u128,
        lp_park_us: u128,
        lp_unpark_us: u128,
        lp_clone_us: u128,
    ) {
                    let lp_cold_start = web_time::Instant::now();
                    let _t_prep = web_time::Instant::now();
                    // Recovery: preset files written by the startup-autosave bug
                    // carry empty symbol/exchange. Resurrect them with the
                    // standard default instead of an eternally-loading empty
                    // chart; the next (now-guarded) autosave heals the file.
                    for w in preset.windows.iter_mut() {
                        if w.symbol.is_empty() {
                            w.symbol = "BTCUSDT".to_string();
                            w.exchange = "binance".to_string();
                        }
                    }
                    for b in preset.bubbles.iter_mut() {
                        if b.symbol.is_empty() {
                            b.symbol = "BTCUSDT".to_string();
                            b.exchange = "binance".to_string();
                        }
                    }
                    // Tick-TF removal migration (resolution-axes rework,
                    // 2026-07-08 owner doctrine): ticks were removed from the
                    // kline-family Time dropdown — they now live exclusively
                    // as the `tick_bar` chart_type's own `chart_type_param`.
                    // A preset saved before this cutover can still carry a
                    // window `timeframe` with `TfKind::Tick { count }` (see
                    // `mlc-core/src/state/timeframe.rs`). Map it onto
                    // `chart_type = "tick_bar"` + `chart_type_param = count`
                    // + a sane time-TF fallback (1m) so the window still
                    // subscribes through the normal kline-family path after
                    // load — mirrors the `columns`/`histogram` chart_type-id
                    // migration precedent already handled below by
                    // `by_id(...).unwrap_or("candles")`.
                    for w in preset.windows.iter_mut() {
                        if let Some(count) = w.timeframe.tick_count() {
                            eprintln!(
                                "[ChartApp] migrating legacy tick TF '{}' (window {}) -> chart_type=tick_bar param={}",
                                w.timeframe.name, w.window_id, count,
                            );
                            w.chart_type = "tick_bar".to_string();
                            w.chart_type_param = count as f64;
                            w.timeframe = mlc_core::state::Timeframe::m1();
                        }
                    }
                    // `DataBubbleSnapshot` carries its OWN `timeframe` too (no
                    // `chart_type` field — stays window-resident, S1 note
                    // above), and it is what actually lands live: the bubble
                    // restore pass further down (`restore_from_snapshots`)
                    // reads `preset.bubbles` directly, `clear()`s any bubble
                    // the window loop above may have touched, and installs
                    // this snapshot's `timeframe` verbatim — the window-side
                    // fix at the migration above never reaches the bubble.
                    // Without this second pass a legacy tick-TF preset keeps
                    // `bubble.timeframe.is_tick() == true` after load even
                    // though `chart_type` correctly becomes `"tick_bar"`
                    // (stamped from the migrated window snapshot later in
                    // this fn) — a stale tick weight surviving on the one
                    // field every `timeframe_for_chart` read actually uses.
                    for b in preset.bubbles.iter_mut() {
                        if b.timeframe.is_tick() {
                            eprintln!(
                                "[ChartApp] migrating legacy tick TF '{}' (bubble {}) -> timeframe=1m (chart_type migrated via window snapshot)",
                                b.timeframe.name, b.id,
                            );
                            b.timeframe = mlc_core::state::Timeframe::m1();
                        }
                    }
                    let prep_us = _t_prep.elapsed().as_micros();
                    eprintln!(
                        "[ChartApp] applying preset '{}' COLD path: {} windows, {} groups, {} indicators (materialized={}us autosave={}us park={}us unpark_miss={}us clone={}us)",
                        preset.name, preset.windows.len(), preset.bubbles.len(), preset.indicators.len(),
                        lp_materialized_us, lp_autosave_us, lp_park_us, lp_unpark_us, lp_clone_us,
                    );

                    // ----------------------------------------------------------------
                    // Collect current (exchange_id, symbol) pairs before switching.
                    // Used after the preset is loaded to unsubscribe stale trade actors.
                    // ----------------------------------------------------------------
                    // Phase 4 S2.7.1c: read via branch_meta (B1 flip).
                    let _t_old_subs = web_time::Instant::now();
                    let old_subscriptions: std::collections::HashSet<(digdigdig3::ExchangeId, String)> = self
                        .panel_app
                        .panel_grid
                        .iter_containers()
                        .filter_map(|(_, w)| {
                            let cid = w.id;
                            let exch = self.exchange_for_chart(cid)?;
                            let sym = self.symbol_for_chart(cid)
                                .map(String::from).unwrap_or_default();
                            digdigdig3::ExchangeId::from_str(&exch).map(|eid| (eid, sym))
                        })
                        .collect();
                    let old_subs_us = _t_old_subs.elapsed().as_micros();

                    // ----------------------------------------------------------------
                    // Step 1: Attempt full layout restore (new presets with layout).
                    // Falls back to in-place window patching for old presets without.
                    // ----------------------------------------------------------------

                    let _t_layout = web_time::Instant::now();
                    let layout_restored = if !preset.layout.is_null() {
                        match serde_json::from_value::<uzor::panels::serialize::LayoutSnapshot>(preset.layout.clone()) {
                            Ok(layout_snap) => {
                                // Step 2: Build new windows + leaf_to_chart from snapshot
                                let mut new_containers = std::collections::HashMap::new();
                                let mut new_leaf_to_chart = std::collections::HashMap::new();

                                for snap in &preset.windows {
                                    let leaf_id = mlc_core::LeafId(snap.leaf_id);
                                    let chart_id = mlc_core::ids::ChartId(snap.window_id);

                                    let mut window = crate::chart_container::ChartContainer::with_id(
                                        chart_id,
                                        &snap.symbol,
                                        snap.timeframe.clone(),
                                    );

                                    // Build a data_provider for this window's own exchange and account_type.
                                    let snap_exchange_id = digdigdig3::ExchangeId::from_str(&snap.exchange)
                                        .unwrap_or(digdigdig3::ExchangeId::Binance);
                                    let snap_at = crate::account_type_from_label(&snap.account_type);
                                    window.data_provider = std::sync::Arc::new(
                                        mlc_backend_data_bridge::LiveDataProvider::new(
                                            snap_exchange_id,
                                            snap_exchange_id.as_str().to_string(),
                                            snap_at,
                                            self.core.bridge_arc(),
                                        ),
                                    );

                                    // Apply non-viewport snapshot fields. Viewport / price-scale
                                    // are restored below (bar_spacing + display config only —
                                    // viewport SWC arc, 2026-07-03): `window` was just constructed
                                    // via `with_id` above, so it already carries a fresh
                                    // `Viewport::default()` / `PriceScale::default()` — no
                                    // wholesale clone from the snapshot.
                                    window.scale_settings = snap.scale_settings.clone();

                                    // Phase 4 C+S2.8: restore display/series fields + drawing_manager into panel state.
                                    {
                                        let mut panel_state = crate::chart_panel_state::ChartPanelState::new(window.chart_panel_id);
                                        panel_state.grid_options = snap.grid_options.clone();
                                        panel_state.crosshair_options = snap.crosshair_options.clone();
                                        panel_state.legend = snap.legend.clone();
                                        panel_state.watermark = snap.watermark.clone();
                                        panel_state.tooltip = snap.tooltip.clone();
                                        panel_state.show_candles = snap.show_candles;
                                        panel_state.show_bars = snap.show_bars;
                                        panel_state.show_hollow_candles = snap.show_hollow_candles;
                                        panel_state.show_heikin_ashi = snap.show_heikin_ashi;
                                        panel_state.show_line = snap.show_line;
                                        panel_state.show_step_line = snap.show_step_line;
                                        panel_state.show_line_markers = snap.show_line_markers;
                                        panel_state.show_area = snap.show_area;
                                        panel_state.show_hlc_area = snap.show_hlc_area;
                                        panel_state.show_histogram = snap.show_histogram;
                                        panel_state.show_columns = snap.show_columns;
                                        panel_state.show_baseline = snap.show_baseline;
                                        panel_state.footprint_display = snap.footprint_display;
                                        self.panels_store.chart_panel.insert(window.chart_panel_id, panel_state);
                                        // Restored drawings are written into the window's
                                        // bubble further below, once `auto_bid` is known
                                        // (the per-bubble drawing model reads directly off
                                        // `window.current_tag_id`, which isn't assigned
                                        // yet at this point in the restore sequence).
                                    }

                                    // Restore command history
                                    if let Some(history) = &snap.command_history {
                                        window.command_history = history.clone();
                                    }

                                    // Create a fresh auto-bubble for this chart and restore
                                    // stashed state (pre-tag primitives, pre_tag_indicator_ids,
                                    // stashed command_history) into it.
                                    let auto_bid = self.create_auto_bubble(
                                        [0.0, 0.0, 0.0, 0.0],
                                        snap.symbol.clone(),
                                        snap.timeframe.clone(),
                                    );
                                    window.auto_tag_id = auto_bid;
                                    // current_tag_id is set later when we attach to the
                                    // color-bubble (Step 5); default to auto_bid for now.
                                    window.current_tag_id = auto_bid;
                                    self.attach_chart_to_tag(chart_id, auto_bid);
                                    // chart_type/param/interval restore deferred to the
                                    // unified post-restore pass below (after Step 5's
                                    // `bubbles_mut().clear()` + `restore_from_snapshots`)
                                    // — this `auto_bid` is a transient pre-clear bubble;
                                    // any write here would be discarded exactly like the
                                    // (pre-existing, likewise-transient) instrument/
                                    // timeframe writes in the legacy fallback branch.

                                    // Restore drawings into `auto_bid`'s primitives — under
                                    // the per-bubble drawing model this bubble IS what the
                                    // window renders (`current_tag_id == auto_bid` above).
                                    // `snap.drawings.primitives` is the window's live/active
                                    // set; `snap.stashed_primitives` is the pre-tag home stash
                                    // for windows that were tagged when saved (empty for
                                    // untagged windows). Prefer the live set when present —
                                    // it reflects what was actually visible on save.
                                    let restore_snap: &[mlc_core::drawing::PrimitiveSnapshot] =
                                        if !snap.drawings.primitives.is_empty() {
                                            &snap.drawings.primitives
                                        } else {
                                            &snap.stashed_primitives
                                        };
                                    self.core.bubbles_mut().with_bubble_mut(auto_bid, |auto_bub| {
                                        auto_bub.primitives.clear();
                                        if !restore_snap.is_empty() {
                                            if let Ok(reg) = mlc_core::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                                                for prim_snap in restore_snap {
                                                    if let Some(mut prim) = reg.from_json(&prim_snap.type_id, &prim_snap.json) {
                                                        prim.data_mut().id = mlc_core::drawing::alloc_primitive_id();
                                                        auto_bub.primitives.push(prim);
                                                    }
                                                }
                                            }
                                        }
                                        // Restore pre_tag_indicator_ids into auto_bubble.
                                        auto_bub.pre_tag_indicator_ids = snap.pre_tag_indicator_ids.clone();
                                        // Restore stashed command_history into auto_bubble.
                                        if let Some(h) = snap.stashed_command_history.clone() {
                                            auto_bub.command_history = h;
                                        }
                                    });

                                    // Restore per-symbol drawing cache
                                    window.symbol_drawings = snap.symbol_drawings_snapshots.clone();

                                    // Restore stashed scale mode (pre-join A/M/F, restored on desync).
                                    window.stashed_scale_mode = snap.stashed_scale_mode;

                                    // Restore bar_spacing (user's zoom level). chart_width is NOT restored  -
                                    // it comes from layout on the first frame; view_start is NOT restored  -
                                    // snap-to-end always positions to the latest bar. Price-scale display
                                    // config only (mode, base_price, width, user_precision) is restored;
                                    // price_min/price_max are NOT restored - calc_auto_scale recomputes
                                    // them from bars after the cold snap.
                                    window.restore_persisted_display(&snap.viewport, &snap.price_scale);
                                    // Cold-load rule (2026-06-24): every cold preset apply must
                                    // land on Auto scale mode — viewport snaps to the right edge
                                    // (`needs_snap_after_backfill = true` below) and the Y axis
                                    // auto-fits the visible bars. Saved `scale_mode` (M / F) is
                                    // ignored on cold path; user can re-toggle after the chart
                                    // settles. Applies uniformly across sole / sync-group /
                                    // all containers (this branch runs per-window in the
                                    // `preset.windows` loop).
                                    window.restore_scale_mode = Some(ScaleMode::Auto);

                                    // Bars arrive asynchronously via BarsLoaded.
                                    window.pending_symbol_load = true;
                                    // Cold preset restore: stale cached bars arrive via BarsLoaded
                                    // first; fresh exchange data arrives later via BackfillComplete.
                                    // Snap to end only after the fresh data lands.
                                    window.needs_snap_after_backfill = true;

                                    eprintln!(
                                        "[ChartApp] built window {} -> {}/{} ({} drawings)",
                                        snap.window_id, snap.symbol, snap.timeframe.name,
                                        snap.drawings.primitives.len()
                                    );

                                    if let Some(bid) = self.bubble_for_chart(chart_id) {
                                        self.core.bubbles_mut().set_bid_instrument_full(
                                            bid,
                                            snap.symbol.clone(),
                                            snap.exchange.clone(),
                                            snap.account_type.clone(),
                                            snap.timeframe.clone(),
                                        );
                                    }
                                    // Phase 4 C: panel state already written above.

                                    new_containers.insert(chart_id, window);
                                    new_leaf_to_chart.insert(leaf_id, chart_id);
                                }

                                // Bump global ChartId counter past any restored window IDs.
                                for snap in &preset.windows {
                                    mlc_core::bump_chart_id_past(snap.window_id);
                                }

                                // Step 3: Restore the docking tree.
                                //
                                // The factory emits a placeholder Panel::Chart(ChartId(0))
                                // for every leaf; each leaf is then patched with the real
                                // ChartId by walking `new_leaf_to_chart`.
                                match layout_snap.restore_tree(|_type_id| {
                                    Some(mlc_core::state::Panel::Chart(
                                        mlc_core::state::ChartId(0),
                                    ))
                                }) {
                                    Ok(mut restored_tree) => {
                                        for (&leaf_id, &chart_id) in &new_leaf_to_chart {
                                            if let Some(leaf) = restored_tree.leaf_mut(leaf_id) {
                                                if let Some(panel) = leaf.active_panel_mut() {
                                                    *panel = mlc_core::state::Panel::Chart(chart_id);
                                                }
                                            }
                                        }
                                        // Step 4: Replace panel grid state
                                        self.panel_app.panel_grid.replace_docking(restored_tree);
                                        self.panel_app.panel_grid.replace_containers(new_containers);
                                        self.panel_app.panel_grid.replace_leaf_to_chart(new_leaf_to_chart);
                                        // After restore the docking tree's active_leaf may be None
                                        // if the serialized snapshot did not include that field
                                        // (older presets). Activate the first available leaf so
                                        // subsequent calls to active_container() / active_leaf()
                                        // return a valid result rather than silently skipping.
                                        if self.panel_app.panel_grid.docking().active_leaf().is_none() {
                                            if let Some(lid) = self.panel_app.panel_grid.first_leaf_id() {
                                                self.panel_app.panel_grid.set_active_leaf(lid);
                                                eprintln!("[ChartApp] active_leaf was None after restore — activated first leaf {:?}", lid);
                                            }
                                        }
                                        eprintln!("[ChartApp] layout topology restored");
                                        true
                                    }
                                    Err(e) => {
                                        // Salvage: build a flat docking tree containing one
                                        // leaf per built window. Iterate in deterministic order
                                        // (sorted by original LeafId so visual order is stable).
                                        let mut sorted_entries: Vec<(uzor::panels::LeafId, mlc_core::state::ChartId)> =
                                            new_leaf_to_chart.into_iter().collect();
                                        sorted_entries.sort_by_key(|(leaf_id, _)| leaf_id.0);

                                        let n = sorted_entries.len();
                                        let mut flat_tree: uzor::panels::DockingTree<mlc_core::state::Panel> =
                                            uzor::panels::DockingTree::new();
                                        let mut fresh_leaf_to_chart: std::collections::HashMap<uzor::panels::LeafId, mlc_core::state::ChartId> =
                                            std::collections::HashMap::new();
                                        let mut first_leaf: Option<uzor::panels::LeafId> = None;

                                        for (_orig_leaf_id, chart_id) in sorted_entries {
                                            let panel = mlc_core::state::Panel::Chart(chart_id);
                                            let new_lid = flat_tree.add_leaf(panel);
                                            fresh_leaf_to_chart.insert(new_lid, chart_id);
                                            if first_leaf.is_none() {
                                                first_leaf = Some(new_lid);
                                            }
                                        }

                                        eprintln!("[ChartApp] restore_tree failed: {} - salvaged flat layout with {} windows", e, n);

                                        self.panel_app.panel_grid.replace_docking(flat_tree);
                                        self.panel_app.panel_grid.replace_containers(new_containers);
                                        self.panel_app.panel_grid.replace_leaf_to_chart(fresh_leaf_to_chart);
                                        if let Some(lid) = first_leaf {
                                            self.panel_app.panel_grid.set_active_leaf(lid);
                                        }
                                        true
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("[ChartApp] layout deserialize failed: {} - falling back to patch", e);
                                false
                            }
                        }
                    } else {
                        eprintln!("[ChartApp] preset has no layout snapshot - falling back to patch");
                        false
                    };

                    // ----------------------------------------------------------------
                    // Fallback: patch existing windows in-place (old presets)
                    // ----------------------------------------------------------------
                    if !layout_restored {
                        // Collect (chart_panel_id, display/series state) from windows
                        // before the panel_grid mut borrow; apply to panels_store after.
                        let mut panel_patches: Vec<(mlc_core::state::ChartPanelId, crate::chart_panel_state::ChartPanelState)> = Vec::new();
                        {
                            let windows = self.panel_app.panel_grid.containers_mut();
                            for snap in &preset.windows {
                                let chart_id = mlc_core::ids::ChartId(snap.window_id);
                                if let Some(window) = windows.get_mut(&chart_id) {
                                    // Rebuild data_provider for this window's (possibly new) exchange.
                                    let patch_exchange_id = digdigdig3::ExchangeId::from_str(&snap.exchange)
                                        .unwrap_or(digdigdig3::ExchangeId::Binance);
                                    let patch_at = crate::account_type_from_label(&snap.account_type);
                                    window.data_provider = std::sync::Arc::new(
                                        mlc_backend_data_bridge::LiveDataProvider::new(
                                            patch_exchange_id,
                                            patch_exchange_id.as_str().to_string(),
                                            patch_at,
                                            self.core.bridge_arc(),
                                        ),
                                    );
                                    // Viewport / price-scale are restored below (bar_spacing +
                                    // display config only — viewport SWC arc, 2026-07-03): this
                                    // branch patches an EXISTING window in place, so its
                                    // `chart_width`/`chart_height`/`bar_count` are already real
                                    // runtime values that must not be clobbered by a snapshot
                                    // clone or a fresh `Viewport::default()`.
                                    window.scale_settings = snap.scale_settings.clone();

                                    // Restore command history
                                    if let Some(history) = &snap.command_history {
                                        window.command_history = history.clone();
                                    }

                                    // Pre-auto-bubble presets serialised no auto_tag_id —
                                    // a placeholder of BubbleId(0) is patched up by the
                                    // post-loop pass below.  Use whatever the window
                                    // currently knows; if it is BubbleId(0) the
                                    // `bubble_mut` lookup misses and stashed_primitives
                                    // restoration is deferred until the post-loop pass
                                    // re-runs the same body with a real auto_bid.
                                    let auto_bid_for_window = window.auto_tag_id;
                                    self.core.bubbles_mut().with_bubble_mut(auto_bid_for_window, |auto_bub| {
                                        auto_bub.primitives.clear();
                                        if !snap.stashed_primitives.is_empty() {
                                            if let Ok(reg) = mlc_core::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                                                for prim_snap in &snap.stashed_primitives {
                                                    if let Some(prim) = reg.from_json(&prim_snap.type_id, &prim_snap.json) {
                                                        auto_bub.primitives.push(prim);
                                                    }
                                                }
                                            }
                                        }
                                        auto_bub.pre_tag_indicator_ids = snap.pre_tag_indicator_ids.clone();
                                        if let Some(h) = snap.stashed_command_history.clone() {
                                            auto_bub.command_history = h;
                                        }
                                    });

                                    // Restore per-symbol drawing cache
                                    window.symbol_drawings = snap.symbol_drawings_snapshots.clone();

                                    // Restore stashed scale mode (pre-join A/M/F, restored on desync).
                                    window.stashed_scale_mode = snap.stashed_scale_mode;

                                    // Restore bar_spacing (user's zoom level) + price-scale
                                    // display config only (see twin branch above for
                                    // rationale). price_min/price_max are left as-is —
                                    // calc_auto_scale recomputes them from bars after the cold snap.
                                    window.restore_persisted_display(&snap.viewport, &snap.price_scale);
                                    // Cold-load rule (2026-06-24): force Auto scale mode on
                                    // cold preset apply — see twin branch above for rationale.
                                    window.restore_scale_mode = Some(ScaleMode::Auto);

                                    // Bars arrive asynchronously via BarsLoaded.
                                    window.pending_symbol_load = true;
                                    // Cold preset restore: stale cached bars arrive via BarsLoaded
                                    // first; fresh exchange data arrives later via BackfillComplete.
                                    // Snap to end only after the fresh data lands.
                                    window.needs_snap_after_backfill = true;

                                    // Collect panel state for post-borrow write to panels_store.
                                    let mut ps = crate::chart_panel_state::ChartPanelState::new(window.chart_panel_id);
                                    ps.grid_options = snap.grid_options.clone();
                                    ps.crosshair_options = snap.crosshair_options.clone();
                                    ps.legend = snap.legend.clone();
                                    ps.watermark = snap.watermark.clone();
                                    ps.tooltip = snap.tooltip.clone();
                                    ps.show_candles = snap.show_candles;
                                    ps.show_bars = snap.show_bars;
                                    ps.show_hollow_candles = snap.show_hollow_candles;
                                    ps.show_heikin_ashi = snap.show_heikin_ashi;
                                    ps.show_line = snap.show_line;
                                    ps.show_step_line = snap.show_step_line;
                                    ps.show_line_markers = snap.show_line_markers;
                                    ps.show_area = snap.show_area;
                                    ps.show_hlc_area = snap.show_hlc_area;
                                    ps.show_histogram = snap.show_histogram;
                                    ps.show_columns = snap.show_columns;
                                    ps.show_baseline = snap.show_baseline;
                                    ps.footprint_display = snap.footprint_display;
                                    panel_patches.push((window.chart_panel_id, ps));
                                    // Restore drawings into the window's current bubble
                                    // directly — no separate per-window DrawingManager
                                    // under the per-bubble model.
                                    {
                                        let bid = window.current_tag_id;
                                        let mut guard = self.core.drawings_mut();
                                        guard.delete_all(bid);
                                        if let Ok(reg) = mlc_core::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                                            for prim_snap in &snap.drawings.primitives {
                                                if let Some(prim) = reg.from_json(&prim_snap.type_id, &prim_snap.json) {
                                                    guard.add_external(bid, prim);
                                                }
                                            }
                                        }
                                        guard.migrate_legacy_timestamps(bid);
                                    }

                                    eprintln!("[ChartApp] patched window {}  ->  {}/{} ({} drawings)",
                                        snap.window_id, snap.symbol, snap.timeframe.name,
                                        snap.drawings.primitives.len());
                                }
                            }
                        } // windows (panel_grid borrow) dropped here
                        // Restore instrument fields into the bubble for each window.
                        for snap in &preset.windows {
                            let chart_id = mlc_core::ids::ChartId(snap.window_id);
                            if let Some(bid) = self.bubble_for_chart(chart_id) {
                                self.core.bubbles_mut().set_bid_instrument_full(
                                    bid,
                                    snap.symbol.clone(),
                                    snap.exchange.clone(),
                                    snap.account_type.clone(),
                                    snap.timeframe.clone(),
                                );
                            }
                            // Note: pre_tag_indicator_ids and stashed_command_history are
                            // already written into auto_bubble above; no separate map needed.
                        }
                        // Phase 4 C: write panel states (panel_grid borrow is dropped above).
                        for (chart_panel_id, ps) in panel_patches {
                            self.panels_store.chart_panel.insert(chart_panel_id, ps);
                        }
                        // Post-loop pass: mint auto-bubbles for any windows whose
                        // preset carried no auto_tag_id (legacy presets).  Done
                        // here instead of inside the windows-mut loop because
                        // create_auto_bubble + attach_chart_to_tag take &mut self
                        // and would conflict with the live windows borrow.
                        let needs_auto: Vec<(mlc_core::ChartId, String, mlc_core::state::Timeframe)> =
                            preset.windows.iter().filter_map(|snap| {
                                let chart_id = mlc_core::ids::ChartId(snap.window_id);
                                let win = self.panel_app.panel_grid.containers().get(&chart_id)?;
                                if win.auto_tag_id == mlc_core::ids::BubbleId(0) {
                                    Some((chart_id, snap.symbol.clone(), snap.timeframe.clone()))
                                } else { None }
                            }).collect();
                        for (chart_id, sym, tf) in needs_auto {
                            let fresh = self.create_auto_bubble([0.0, 0.0, 0.0, 0.0], sym, tf);
                            if let Some(window) = self.panel_app.panel_grid.containers_mut().get_mut(&chart_id) {
                                window.auto_tag_id = fresh;
                                window.current_tag_id = fresh;
                            }
                            self.attach_chart_to_tag(chart_id, fresh);
                        }
                    }
                    let layout_us = _t_layout.elapsed().as_micros();

                    // ----------------------------------------------------------------
                    // Step 5: Restore TagManager (sync groups)
                    // ----------------------------------------------------------------
                    let _t_bubbles = web_time::Instant::now();
                    self.core.bubbles_mut().clear();
                    self.core.tags_mut().clear();
                    // `restore_from_snapshots` builds each bubble (including the
                    // symbol/exchange/account_type primitive backfill) and inserts
                    // it. `TagManager::restore_from_snapshots` rebuilds the paired
                    // Tags from the same snapshot list (color/auto_created/members/
                    // member_overrides, Step 6 doctrine cutover) and returns the
                    // synced-panel pairs so `set_panel_synced` (an app-level method,
                    // not a store method) stays in app code.
                    self.core.bubbles_mut().restore_from_snapshots(&preset.bubbles);
                    let panel_syncs = self.core.tags_mut().restore_from_snapshots(&preset.bubbles);
                    for (member, gid) in panel_syncs {
                        self.set_panel_synced(member, gid);
                    }
                    eprintln!("[ChartApp] restored {} sync groups", preset.bubbles.len());

                    // Restore chart_type/param/interval onto each window's FINAL
                    // bubble. Must run here — after `bubbles_mut().clear()` +
                    // `restore_from_snapshots` above — because `chart_type` is
                    // bubble-resident (symmetric with `timeframe`) but
                    // `DataBubbleSnapshot` does not carry it (it stays on the
                    // per-window `ChartWindowSnapshot`, S1 note). Any write onto
                    // `bubble_for_chart(chart_id)` taken BEFORE this point targets
                    // a transient pre-clear bubble (the `auto_bid` minted during
                    // window construction above) that this clear+restore just
                    // discarded — same hazard the "Window↔bubble pointer sync
                    // sweep" further below documents and fixes for other fields.
                    //
                    // Whitelist widened §3.4 (handoff 2026-07-06): derived types
                    // now round-trip. Step 10 (below in this file, or the startup
                    // post-load loop in lib.rs) ignites the matching bridge sub
                    // for each — previously the whole family collapsed to
                    // "candles" here because no cold-open subscribe existed
                    // outside footprint. Legacy `"columns"`/`"histogram"` ids no
                    // longer exist in the registry — `by_id(...).unwrap_or` below
                    // naturally migrates them to "candles" on restore.
                    for snap in &preset.windows {
                        let chart_id = mlc_core::ids::ChartId(snap.window_id);
                        let Some(bid) = self.bubble_for_chart(chart_id) else { continue };
                        let static_ct: &'static str = mlc_core::chart_type::by_id(snap.chart_type.as_str())
                            .map(|d| d.id)
                            .unwrap_or("candles");
                        let static_iv: &'static str = match snap.chart_type_interval.as_str() {
                            "1m" => "1m", "3m" => "3m", "5m" => "5m",
                            "15m" => "15m", "30m" => "30m", "1h" => "1h",
                            _ => "1m",
                        };
                        self.core.bubbles_mut().set_chart_type(bid, static_ct);
                        self.core.bubbles_mut().set_chart_type_param(bid, snap.chart_type_param);
                        self.core.bubbles_mut().set_chart_type_interval(bid, static_iv);
                    }
                    let bubbles_us = _t_bubbles.elapsed().as_micros();

                    // NOTE (Step 3.1): style-store rebinding is no longer needed —
                    // `DrawingManager::effective_color` reads `bubble.last_used_style`
                    // directly for the bid-keyed session, no per-window Arc to bind.
                    let rebind_us = 0u128;

                    // Bump BubbleId counter past any restored bubble IDs so
                    // future create_bubble / create_bubble_auto calls never collide.
                    let _t_seed_ids = web_time::Instant::now();
                    for sg_snap in &preset.bubbles {
                        mlc_core::ids::BubbleId::bump_past(sg_snap.id);
                    }

                    // Seed the global primitive ID counter so future allocations
                    // never collide with any ID loaded from disk. All primitives
                    // live on bubbles under the per-bubble drawing model — no
                    // separate per-window DrawingManager sweep needed.
                    {
                        let mut max_id = 0u64;
                        self.core.bubbles().each_bubble(|bv| {
                            if let Some(id) = bv.max_primitive_id() {
                                max_id = max_id.max(id);
                            }
                        });
                        mlc_core::drawing::seed_primitive_id_counter(max_id);
                    }

                    // Re-ID stashed primitives that collide with group primitives.
                    // Legacy autosave files may have saved both a stash primitive and
                    // its group counterpart with the same numeric ID, causing widget
                    // ID collisions in the Object Tree sidebar.  Assign fresh IDs to
                    // any stash primitive whose ID already exists in group.primitives.
                    {
                        // Phase 1: collect (window_key, group_id, set_of_group_prim_ids)
                        // using immutable borrows only.
                        let container_ids: Vec<mlc_core::ChartId> =
                            self.panel_app.panel_grid.containers().keys().copied().collect();
                        let collision_info: Vec<(mlc_core::ChartId, mlc_core::ids::BubbleId, std::collections::HashSet<u64>)> =
                            container_ids.into_iter()
                                .filter_map(|cid| {
                                    let gid = self.bubble_for_chart(cid)?;
                                    let group_ids: std::collections::HashSet<u64> =
                                        self.with_bubble(gid, |g| g.primitives.iter().map(|p| p.data().id).collect())
                                            .unwrap_or_default();
                                    Some((cid, gid, group_ids))
                                })
                                .collect();

                        // Phase 2: mutably iterate auto_bubble.primitives and re-ID colliding ones.
                        for (chart_id, _gid, group_ids) in &collision_info {
                            let auto_bid = self.panel_app.panel_grid.containers()
                                .get(chart_id)
                                .map(|w| w.auto_tag_id);
                            let reid_count = auto_bid.and_then(|bid| {
                                self.core.bubbles_mut().with_bubble_mut(bid, |auto_bub| {
                                    let mut reid_count = 0u32;
                                    for p in auto_bub.primitives.iter_mut() {
                                        if group_ids.contains(&p.data().id) {
                                            p.data_mut().id = mlc_core::drawing::alloc_primitive_id();
                                            reid_count += 1;
                                        }
                                    }
                                    reid_count
                                })
                            }).unwrap_or(0);
                            if reid_count > 0 {
                                eprintln!(
                                    "[ChartApp] Re-IDed {} auto-bubble primitive(s) in window {:?} (legacy autosave collision fix)",
                                    reid_count, chart_id
                                );
                            }
                        }
                    }

                    // Deduplicate primitive IDs globally, across every bubble.
                    // Old presets may have group primitives with identical IDs
                    // (from historical clone paths that didn't re-ID).
                    let total_dedup = self.core.drawings_mut().dedup_primitive_ids();
                    if total_dedup > 0 {
                        eprintln!("[ChartApp] Re-IDed {} duplicate primitive(s) across all groups", total_dedup);
                    }
                    let seed_ids_us = _t_seed_ids.elapsed().as_micros();

                    // Step 5b: If no sync groups exist but windows do, auto-create
                    // a default group so primitives always use the grouped path.
                    let _t_orphans = web_time::Instant::now();
                    if preset.bubbles.is_empty() && !self.panel_app.panel_grid.containers().is_empty() {
                        // Auto groups get transparent color  -  never occupy palette slots
                        let Some((_, first_win)) = self.panel_app.panel_grid.iter_containers().next() else {
                            eprintln!("[sync_group] preset restore step 5b: iter_containers empty despite non-empty guard - skipping auto group");
                            return;
                        };
                        // Phase 4 S2.7.1c: read via branch_meta (B6 capture flip).
                        let first_cid = first_win.id;
                        let symbol = self.symbol_for_chart(first_cid)
                            .map(String::from).unwrap_or_default();
                        let timeframe = self.timeframe_for_chart(first_cid).unwrap_or_else(mlc_core::state::Timeframe::h1);
                        let group_id = self.create_auto_bubble([0.0, 0.0, 0.0, 0.0], symbol, timeframe);
                        let leaf_chart_ids: Vec<(mlc_core::LeafId, mlc_core::ChartId)> =
                            self.panel_app.panel_grid.iter_containers()
                                .map(|(leaf_id, w)| (leaf_id, w.id))
                                .collect();
                        for &(_leaf_id, chart_id) in &leaf_chart_ids {
                            self.attach_chart_to_tag(chart_id, group_id);
                            // Sync the window-side bubble pointer so the renderer's
                            // `window.current_tag_id` lookup matches the writer's
                            // `bubble_for_chart` lookup. Without this, the BubbleStore
                            // member_to_bubble map points at the new group while the
                            // window still carries the sentinel BubbleId(0) — the
                            // writer fills group_id, the renderer reads sentinel,
                            // chart stays empty.
                            if let Some(window) = self.panel_app.panel_grid.containers_mut().get_mut(&chart_id) {
                                window.auto_tag_id = group_id;
                                window.current_tag_id = group_id;
                            }
                        }
                        eprintln!("[ChartApp] Auto-created invisible sync group {:?} for {} orphaned windows", group_id, leaf_chart_ids.len());
                    }

                    // Step 5c: membership safety net. Step 5b only fires when the
                    // preset has NO groups at all — but poisoned files (startup-
                    // autosave bug) carry orphaned groups whose member lists do
                    // NOT include the windows. A window without bubble membership
                    // is a dead chart: symbol_for_chart=None -> empty tag/toolbar,
                    // no bars requested, the preset is never considered
                    // materialized so it never parks. Give every orphaned window
                    // its own auto group seeded from its (recovered) snapshot.
                    {
                        let orphan_windows: Vec<(mlc_core::LeafId, mlc_core::ChartId)> =
                            self.panel_app.panel_grid.iter_containers()
                                .filter(|(_, w)| self.bubble_for_chart(w.id).is_none())
                                .map(|(leaf_id, w)| (leaf_id, w.id))
                                .collect();
                        for &(leaf_id, chart_id) in &orphan_windows {
                            let snap = preset.windows.iter().find(|s| s.window_id == chart_id.0);
                            let (sym, exch, at, tf) = snap
                                .map(|s| (
                                    s.symbol.clone(),
                                    s.exchange.clone(),
                                    s.account_type.clone(),
                                    s.timeframe.clone(),
                                ))
                                .unwrap_or_else(|| (
                                    "BTCUSDT".to_string(),
                                    "binance".to_string(),
                                    "S".to_string(),
                                    mlc_core::state::Timeframe::h1(),
                                ));
                            let group_id = self.create_auto_bubble(
                                [0.0, 0.0, 0.0, 0.0],
                                sym.clone(),
                                tf,
                            );
                            let at_normalized = if at.is_empty() { "S".to_string() } else { at };
                            self.core.bubbles_mut().set_bubble_instrument(group_id, exch.clone(), at_normalized);
                            self.attach_chart_to_tag(chart_id, group_id);
                            // Sync window-side bubble pointer (see Step 5b for rationale).
                            if let Some(window) = self.panel_app.panel_grid.containers_mut().get_mut(&chart_id) {
                                window.auto_tag_id = group_id;
                                window.current_tag_id = group_id;
                            }
                            if let Some(pid) = self.panel_app.panel_grid.container_for_leaf(leaf_id).map(|w| w.chart_panel_id) {
                                let _panel = crate::chart_panel_bridge::ensure_chart_panel_state(
                                    &mut self.panels_store,
                                    pid,
                                );
                            }
                            eprintln!(
                                "[ChartApp] Step 5c: re-membered orphaned window {:?} into auto group {:?} ({})",
                                chart_id, group_id, sym
                            );
                        }
                    }
                    let orphans_us = _t_orphans.elapsed().as_micros();

                    // ----------------------------------------------------------------
                    // Step 6: Restore indicators
                    // ----------------------------------------------------------------
                    let _t_indicators = web_time::Instant::now();
                    self.core.indicators_mut().clear_all();
                    // Re-arm the one-shot indicator heal (tick.rs): restored
                    // instances reuse ids from the previous preset.
                    self.indicator_heal_attempted.clear();
                    for ind_snap in &preset.indicators {
                        let bid = ind_snap.bubble_id.map(mlc_core::ids::BubbleId);
                        // Bind to the persisted bubble_id via `restore_with_id` —
                        // direct field assignment would leak the instance into the
                        // wrong by_bubble_id bucket.  Old preset files used
                        // `window_id` for the same slot; the snapshot type
                        // keeps a serde alias so they still load.
                        if self.core.indicators_mut().restore_with_id(
                            ind_snap.id,
                            bid,
                            &ind_snap.type_id,
                        ) {
                            self.core.indicators_mut().restore_from_snapshot(ind_snap);
                        }
                    }
                    eprintln!("[ChartApp] restored {} indicators", preset.indicators.len());
                    let indicators_us = _t_indicators.elapsed().as_micros();

                    // ----------------------------------------------------------------
                    // Step 6b: Immediately recalculate indicators for every window that
                    // already has bars.  Bars were restored in Steps 2-4 so we can feed
                    // them to the indicator engine right now  -  no need to wait for the
                    // next BarsLoaded / TradeUpdate round-trip.
                    // Collect (symbol, window_id, bars) first to avoid a split borrow of
                    // `self` (panel_grid vs indicator_manager both live on `self`).
                    // ----------------------------------------------------------------
                    let _t_recalc = web_time::Instant::now();
                    let window_bar_data: Vec<(u64, mlc_core::state::ChartId)> = self
                        .panel_app
                        .panel_grid
                        .iter_containers()
                        .map(|(_, w)| (w.id.0, w.id))
                        .collect();

                    let recalc_n = window_bar_data.len();
                    for (window_id, cid) in window_bar_data {
                        let bars = self.bars_for_chart(cid);
                        self.core.indicators_mut().recalc_bubble(mlc_core::ids::BubbleId(window_id), &bars);
                    }
                    eprintln!("[ChartApp] recalculated indicators for all windows with bars");
                    let recalc_us = _t_recalc.elapsed().as_micros();

                    // ----------------------------------------------------------------
                    // Step 6c: Stage sub-pane height ratios, above_main flags, and
                    // ordering for application on the next
                    // sync_sub_panes_from_manager() call.
                    // ----------------------------------------------------------------
                    let _t_sub_panes = web_time::Instant::now();
                    self.pending_sub_pane_ratios.clear();
                    self.pending_sub_pane_above_main.clear();
                    self.pending_sub_pane_order.clear();
                    self.pending_sub_pane_hidden.clear();
                    for snap in &preset.windows {
                        if !snap.sub_pane_height_ratios.is_empty() {
                            self.pending_sub_pane_ratios
                                .insert(snap.window_id, snap.sub_pane_height_ratios.clone());
                        }
                        if !snap.sub_pane_above_main.is_empty() {
                            self.pending_sub_pane_above_main
                                .insert(snap.window_id, snap.sub_pane_above_main.clone());
                        }
                        if !snap.sub_pane_order.is_empty() {
                            self.pending_sub_pane_order
                                .insert(snap.window_id, snap.sub_pane_order.clone());
                        }
                        if !snap.sub_pane_hidden.is_empty() {
                            self.pending_sub_pane_hidden
                                .insert(snap.window_id, snap.sub_pane_hidden.clone());
                        }
                    }
                    if !self.pending_sub_pane_ratios.is_empty() {
                        eprintln!(
                            "[ChartApp] staged sub-pane height ratios for {} windows",
                            self.pending_sub_pane_ratios.len()
                        );
                    }
                    let sub_panes_us = _t_sub_panes.elapsed().as_micros();

                    // ----------------------------------------------------------------
                    // Step 7: Clear stale per-leaf UI state
                    // ----------------------------------------------------------------
                    let _t_leaf_tags = web_time::Instant::now();
                    self.panel_app.indicator_overlay_states.clear();
                    self.panel_app.leaf_color_tags.clear();

                    // Restore leaf_color_tags directly from the preset snapshot.
                    for (&leaf_raw, &color) in &preset.leaf_color_tags {
                        let leaf_id = mlc_core::LeafId(leaf_raw);
                        self.panel_app.leaf_color_tags.insert(leaf_id, color);
                    }
                    eprintln!(
                        "[ChartApp] Step7: restored {} leaf_color_tags from snapshot",
                        self.panel_app.leaf_color_tags.len()
                    );
                    let leaf_tags_us = _t_leaf_tags.elapsed().as_micros();

                    // ----------------------------------------------------------------
                    // Step 8: Restore alerts
                    // ----------------------------------------------------------------
                    let _t_alerts = web_time::Instant::now();
                    // Alerts are now stored per-bubble via DataBubbleSnapshot.alerts.
                    // The legacy flat preset.alerts list is distributed to the appropriate
                    // bubble by group_id, or to the active bubble as a fallback.
                    {
                        let mut total_restored = 0usize;
                        for alert in preset.alerts.clone() {
                            let target_bid = alert.group_id
                                .map(mlc_core::ids::BubbleId)
                                .or_else(|| {
                                    // Fall back to the first available bubble.
                                    self.core.bubbles().bubble_ids().next()
                                });
                            if let Some(bid) = target_bid {
                                self.core.alerts_mut().restore_one(bid, alert);
                                total_restored += 1;
                            }
                        }
                        eprintln!(
                            "[ChartApp] restored {} alerts (per-bubble, {} total now in store)",
                            total_restored,
                            self.core.alerts().len_total(),
                        );
                    }
                    let alerts_us = _t_alerts.elapsed().as_micros();

                    // Step 8b: Restore per-slot Panel docking layouts.
                    // Pre-populate the store with panel state so the closure
                    // can construct the matching `Panel` variants.
                    let _t_slots = web_time::Instant::now();
                    {
                        // Determine max panel_id across ALL slots to update next_id.
                        let max_panel_id: u64 = preset.slot_leaves.iter()
                            .flat_map(|v| v.iter().map(|l| l.panel_id))
                            .max()
                            .unwrap_or(0);
                        self.panels_store.set_min_next_id(max_panel_id);
                    }

                    // Helper: normalize legacy account_type strings (e.g. "spot") to short labels ("S").
                    let normalize_account_type = |at: &str| -> String {
                        match at.to_lowercase().as_str() {
                            "spot" => "S".to_string(),
                            "margin" => "M".to_string(),
                            "futures" | "futurescross" => "F".to_string(),
                            "futuresiso" | "futuresisolated" => "FI".to_string(),
                            _ => at.to_string(),
                        }
                    };

                    // Helper: convert PersistedSymbolSource  ->  SymbolSource.
                    let convert_source = |src: &mlc_core::preset::preset::PersistedSymbolSource| -> mlc_app_panels::trading::SymbolSource {
                        match src {
                            mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => mlc_app_panels::trading::SymbolSource::HyperFocus,
                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, exchange, account_type } => mlc_app_panels::trading::SymbolSource::Fixed {
                                symbol: symbol.clone(),
                                exchange: exchange.clone(),
                                account_type: normalize_account_type(account_type),
                            },
                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { leaf_id } => mlc_app_panels::trading::SymbolSource::BoundToChart {
                                leaf_id: *leaf_id,
                            },
                        }
                    };

                    for i in 0..4 {
                        // Pre-insert state into the store keyed by the saved panel_id.
                        for pl in &preset.slot_leaves[i] {
                            let pid = mlc_core::state::PanelId(pl.panel_id);
                            use mlc_core::preset::preset::PersistedFreeItemKind;
                            match &pl.kind {
                                PersistedFreeItemKind::Dom { source, tick_size, levels_displayed, center_price } => {
                                    if !self.panels_store.dom.contains_key(&pid) {
                                        let symbol = match source {
                                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { .. } | mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = mlc_app_panels::trading::order_flow::dom::DomState::new(symbol, *tick_size);
                                        s.levels_displayed = *levels_displayed;
                                        s.center_price = *center_price;
                                        // Market-data panels no longer carry SymbolSource; TagManager tracks their group.
                                        if let mlc_core::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
                                            s.exchange = exchange.clone();
                                            s.account_type = normalize_account_type(account_type);
                                        }
                                        // Subscribe to the shared orderbook + trade series if we have a concrete symbol.
                                        if !s.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&s.exchange) {
                                                let at = crate::account_type_from_label(&s.account_type);
                                                s.shared_orderbook = self.core.subscribe_orderbook(eid, &s.symbol, at);
                                                let trade_handle = self.core.subscribe_trades(eid, &s.symbol, at);
                                                s.shared_trades = trade_handle;
                                            }
                                        }
                                        self.panels_store.dom.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::Footprint { source, tick_size } => {
                                    if !self.panels_store.footprint.contains_key(&pid) {
                                        let symbol = match source {
                                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { .. } | mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = mlc_app_panels::trading::order_flow::footprint::FootprintState::new(symbol, *tick_size);
                                        if let mlc_core::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
                                            s.exchange = exchange.clone();
                                            s.account_type = normalize_account_type(account_type);
                                        }
                                        // Subscribe to the shared trade ring if we have a concrete symbol.
                                        if !s.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&s.exchange) {
                                                let at = crate::account_type_from_label(&s.account_type);
                                                let handle = self.core.subscribe_trades(eid, &s.symbol, at);
                                                s.shared_trades = handle;
                                            }
                                        }
                                        self.panels_store.footprint.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::VolumeProfile { source, tick_size } => {
                                    if !self.panels_store.volume_profile.contains_key(&pid) {
                                        let symbol = match source {
                                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { .. } | mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = mlc_app_panels::trading::order_flow::volume_profile::VolumeProfileState::new(symbol, *tick_size);
                                        if let mlc_core::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
                                            s.exchange = exchange.clone();
                                            s.account_type = normalize_account_type(account_type);
                                        }
                                        // Subscribe to the shared trade ring if we have a concrete symbol.
                                        if !s.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&s.exchange) {
                                                let at = crate::account_type_from_label(&s.account_type);
                                                let handle = self.core.subscribe_trades(eid, &s.symbol, at);
                                                s.shared_trades = handle;
                                            }
                                        }
                                        self.panels_store.volume_profile.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::LiquidityHeatmap { source, tick_size, snapshot_interval_ms } => {
                                    if !self.panels_store.liquidity_heatmap.contains_key(&pid) {
                                        let symbol = match source {
                                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { .. } | mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = mlc_app_panels::trading::order_flow::liquidity_heatmap::LiquidityHeatmapState::new(symbol, *tick_size, *snapshot_interval_ms);
                                        if let mlc_core::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
                                            s.exchange = exchange.clone();
                                            s.account_type = normalize_account_type(account_type);
                                        }
                                        // Subscribe to the shared orderbook tracker if we have a concrete symbol.
                                        if !s.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&s.exchange) {
                                                let at = crate::account_type_from_label(&s.account_type);
                                                s.shared_orderbook = self.core.subscribe_orderbook(eid, &s.symbol, at);
                                            }
                                        }
                                        self.panels_store.liquidity_heatmap.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::BigTrades { source } => {
                                    if !self.panels_store.big_trades.contains_key(&pid) {
                                        let symbol = match source {
                                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { .. } | mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = mlc_app_panels::trading::order_flow::big_trades::BigTradesState::new();
                                        s.symbol = symbol;
                                        if let mlc_core::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
                                            s.exchange = exchange.clone();
                                            s.account_type = normalize_account_type(account_type);
                                        }
                                        // Subscribe to the shared trade ring if we have a concrete symbol.
                                        if !s.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&s.exchange) {
                                                let at = crate::account_type_from_label(&s.account_type);
                                                let handle = self.core.subscribe_trades(eid, &s.symbol, at);
                                                s.shared_trades = handle;
                                            }
                                        }
                                        self.panels_store.big_trades.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::L2Tape { source } => {
                                    if !self.panels_store.l2_tape.contains_key(&pid) {
                                        let symbol = match source {
                                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { .. } | mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = mlc_app_panels::trading::order_flow::l2_tape::L2TapeState::new();
                                        s.symbol = symbol;
                                        if let mlc_core::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
                                            s.exchange = exchange.clone();
                                            s.account_type = normalize_account_type(account_type);
                                        }
                                        // Subscribe to the shared orderbook series if we have a concrete symbol.
                                        if !s.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&s.exchange) {
                                                let at = crate::account_type_from_label(&s.account_type);
                                                s.shared_orderbook = self.core.subscribe_orderbook(eid, &s.symbol, at);
                                            }
                                        }
                                        self.panels_store.l2_tape.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::TradeTape { source } => {
                                    if !self.panels_store.trade_tape.contains_key(&pid) {
                                        let symbol = match source {
                                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { .. } | mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = mlc_app_panels::trading::order_flow::trade_tape::TradeTapeState::new();
                                        s.symbol = symbol;
                                        if let mlc_core::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
                                            s.exchange = exchange.clone();
                                            s.account_type = normalize_account_type(account_type);
                                        }
                                        // Subscribe to the shared trade ring if we have a concrete symbol.
                                        if !s.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&s.exchange) {
                                                let at = crate::account_type_from_label(&s.account_type);
                                                let handle = self.core.subscribe_trades(eid, &s.symbol, at);
                                                s.shared_trades = handle;
                                            }
                                        }
                                        self.panels_store.trade_tape.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::OrderEntry { source } => {
                                    if !self.panels_store.order_entry.contains_key(&pid) {
                                        let symbol = match source {
                                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { .. } | mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = mlc_app_panels::trading::trading::order_entry::OrderEntryState::new(symbol);
                                        s.source = convert_source(source);
                                        if let mlc_core::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
                                            s.exchange = exchange.clone();
                                            s.account_type = normalize_account_type(account_type);
                                        }
                                        self.panels_store.order_entry.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::PositionManager => {
                                    if !self.panels_store.position_manager.contains_key(&pid) {
                                        self.panels_store.position_manager.insert(pid, mlc_app_panels::trading::trading::position_manager::PositionManagerState::new());
                                    }
                                }
                                PersistedFreeItemKind::TradeLog => {
                                    if !self.panels_store.trade_log.contains_key(&pid) {
                                        self.panels_store.trade_log.insert(pid, mlc_app_panels::trading::trading::trade_log::TradeLogState::new());
                                    }
                                }
                                PersistedFreeItemKind::RiskCalculator => {
                                    if !self.panels_store.risk_calculator.contains_key(&pid) {
                                        self.panels_store.risk_calculator.insert(pid, mlc_app_panels::trading::trading::risk_calculator::RiskCalculatorState::new());
                                    }
                                }
                                PersistedFreeItemKind::TradingContainer { source, tick_size, market_price } => {
                                    if !self.panels_store.trading_container.contains_key(&pid) {
                                        let symbol = match source {
                                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { .. } | mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = mlc_app_panels::trading::trading::trading_container::TradingContainerState::new(symbol, *tick_size, *market_price);
                                        s.source = convert_source(source);
                                        self.panels_store.trading_container.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::IndicatorPanel {
                                    symbol, exchange, account_type, instance_id,
                                    indicator_config_id, price_min, price_max, auto_scale,
                                    bar_spacing, view_position, height_ratio,
                                } => {
                                    if !self.panels_store.indicator_panel.contains_key(&pid) {
                                        let mut s = mlc_app_panels::trading::order_flow::indicator_panel::IndicatorPanelState::with_instance(
                                            symbol.clone(), *instance_id,
                                        );
                                        s.exchange = exchange.clone();
                                        s.account_type = account_type.clone();
                                        s.indicator_config_id = *indicator_config_id;
                                        s.price_min = *price_min;
                                        s.price_max = *price_max;
                                        s.auto_scale = *auto_scale;
                                        s.bar_spacing = *bar_spacing;
                                        s.view_position = *view_position;
                                        s.height_ratio = *height_ratio;
                                        self.panels_store.indicator_panel.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::MultiDom { symbol, exchanges, grid_size, depth_per_side } => {
                                    if !self.panels_store.multi_dom.contains_key(&pid) {
                                        let config = mlc_app_panels::trading::order_flow::multi_dom::MultiDomConfig {
                                            exchanges: exchanges.clone(),
                                            grid_size: *grid_size,
                                            depth_per_side: *depth_per_side,
                                        };
                                        let s = mlc_app_panels::trading::order_flow::multi_dom::MultiDomState::new(
                                            symbol.clone(), config,
                                        );
                                        // Subscribe orderbook for each configured exchange.
                                        if !symbol.is_empty() {
                                            for ex_label in exchanges {
                                                if let Some(eid) = digdigdig3::ExchangeId::from_str(ex_label) {
                                                    let at = digdigdig3::AccountType::Spot;
                                                    let _ = self.core.subscribe_orderbook(eid, symbol, at);
                                                }
                                            }
                                        }
                                        self.panels_store.multi_dom.insert(pid, s);
                                    }
                                }
                            }
                        }

                        // Build a leaf_id  ->  PersistedFreeLeaf map for O(1) lookup
                        // inside the restore closure.  type_id() now returns only the
                        // variant kind (e.g. "free_dom")  -  panel identity comes from
                        // the leaf_id passed by restore_tree_with_id.
                        let leaves_by_id: std::collections::HashMap<u64, &mlc_core::preset::preset::PersistedFreeLeaf> =
                            preset.slot_leaves[i].iter().map(|l| (l.leaf_id, l)).collect();

                        let mgr = match preset.slot_layouts[i].as_deref() {
                            Some(json) => {
                                match uzor::panels::serialize::LayoutSnapshot::from_json(json) {
                                    Ok(snap) => {
                                        match snap.restore_tree_with_id::<mlc_core::state::Panel, _>(|leaf_id, _type_id| {
                                            // Look up the persisted leaf descriptor by leaf_id.
                                            // The panel_id and kind are stored there; type_id is
                                            // not used (it only carries the variant kind, no id).
                                            let pl = leaves_by_id.get(&leaf_id)?;
                                            let pid = mlc_core::state::PanelId(pl.panel_id);
                                            use mlc_core::state::Panel;
                                            let item = match &pl.kind {
                                                mlc_core::preset::preset::PersistedFreeItemKind::Dom { .. }               => Panel::Dom(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::Footprint { .. }         => Panel::Footprint(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::VolumeProfile { .. }     => Panel::VolumeProfile(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::LiquidityHeatmap { .. }  => Panel::LiquidityHeatmap(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::BigTrades { .. }         => Panel::BigTrades(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::L2Tape { .. }            => Panel::L2Tape(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::TradeTape { .. }         => Panel::TradeTape(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::OrderEntry { .. }        => Panel::OrderEntry(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::PositionManager          => Panel::PositionManager(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::TradeLog                 => Panel::TradeLog(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::RiskCalculator           => Panel::RiskCalculator(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::TradingContainer { .. }  => Panel::TradingContainer(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::IndicatorPanel { .. }   => Panel::IndicatorPanel(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::MultiDom { .. }         => Panel::MultiDom(pid),
                                            };
                                            Some(item)
                                        }) {
                                            Ok(tree) => {
                                                mlc_app_sidebar_content::SlotDockingManager(
                                                    uzor::layout::DockState::from_tree(tree),
                                                )
                                            }
                                            Err(e) => {
                                                eprintln!("[ChartApp] slot{} restore_tree failed: {} - empty fallback", i + 1, e);
                                                mlc_app_sidebar_content::SlotDockingManager::new()
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("[ChartApp] slot{} from_json failed: {} - empty fallback", i + 1, e);
                                        mlc_app_sidebar_content::SlotDockingManager::new()
                                    }
                                }
                            }
                            None => mlc_app_sidebar_content::SlotDockingManager::new(),
                        };
                        self.sidebar_state.slot_dockings[i] = mgr;
                    }
                    let slots_us = _t_slots.elapsed().as_micros();

                    // Subscribe depth for restored panels that require L2 data
                    // (DOM, L2Tape, LiquidityHeatmap).  For Fixed sources we use the
                    // stored exchange string; for HyperFocus/BoundToChart we fall back
                    // to the active exchange and Spot account type.
                    let _t_depth_subs = web_time::Instant::now();
                    {
                        // Build a deduplicated list of (exchange_id, symbol, account_type)
                        // tuples that need depth subscriptions.
                        let mut depth_subs: Vec<(digdigdig3::ExchangeId, String, digdigdig3::AccountType)> = Vec::new();

                        let resolve_eid = |exchange_str: &str| -> digdigdig3::ExchangeId {
                            self.core.exchange_symbols()
                                .find_by_label(exchange_str)
                                .unwrap_or(self.active_exchange)
                        };

                        // DOM panels subscribe via subscribe_orderbook() in the
                        // PersistedFreeItemKind::Dom block above.  No additional
                        // subscribe_depth() call is needed here for DOM.

                        for state in self.panels_store.l2_tape.values() {
                            if state.symbol.is_empty() { continue; }
                            let eid = if state.exchange.is_empty() {
                                self.active_exchange
                            } else {
                                resolve_eid(&state.exchange)
                            };
                            let at = if state.account_type.is_empty() {
                                digdigdig3::AccountType::Spot
                            } else {
                                crate::account_type_from_label(&state.account_type)
                            };
                            depth_subs.push((eid, state.symbol.clone(), at));
                        }

                        for state in self.panels_store.liquidity_heatmap.values() {
                            if state.symbol.is_empty() { continue; }
                            let eid = if state.exchange.is_empty() {
                                self.active_exchange
                            } else {
                                resolve_eid(&state.exchange)
                            };
                            let at = if state.account_type.is_empty() {
                                digdigdig3::AccountType::Spot
                            } else {
                                crate::account_type_from_label(&state.account_type)
                            };
                            depth_subs.push((eid, state.symbol.clone(), at));
                        }

                        // Deduplicate before subscribing.
                        let mut seen: std::collections::HashSet<(digdigdig3::ExchangeId, String, digdigdig3::AccountType)> = std::collections::HashSet::new();
                        for (eid, sym, at) in depth_subs {
                            if seen.insert((eid, sym.clone(), at)) {
                                self.core.subscribe_depth(eid, &sym, at);
                            }
                        }
                    }
                    let depth_subs_us = _t_depth_subs.elapsed().as_micros();

                    self.sidebar_data_dirty = true;

                    // ----------------------------------------------------------------
                    // Step 9: Unsubscribe trade actors for symbols that were in the
                    // old preset but are absent from the new one.  Uses targeted
                    // unsubscribe_trades() with a 30-second grace period so symbols
                    // that appear in both old and new presets are not disrupted.
                    // ----------------------------------------------------------------
                    // Phase 4 S2.7.1c: read via branch_meta (B1 flip).
                    let _t_unsubs = web_time::Instant::now();
                    let new_subscriptions: std::collections::HashSet<(digdigdig3::ExchangeId, String)> = self
                        .panel_app
                        .panel_grid
                        .iter_containers()
                        .filter_map(|(_, w)| {
                            let cid = w.id;
                            let exch = self.exchange_for_chart(cid)?;
                            let sym = self.symbol_for_chart(cid)
                                .map(String::from).unwrap_or_default();
                            digdigdig3::ExchangeId::from_str(&exch).map(|eid| (eid, sym))
                        })
                        .collect();

                    for (eid, symbol) in &old_subscriptions {
                        if !new_subscriptions.contains(&(*eid, symbol.clone())) {
                            self.core.unsubscribe_trades(*eid, symbol, digdigdig3::AccountType::default());
                            eprintln!("[ChartApp] unsubscribed trades: {}/{}", eid.as_str(), symbol);
                        }
                    }
                    let unsubs_us = _t_unsubs.elapsed().as_micros();

                    // ----------------------------------------------------------------
                    // Step 10: Request bars for every window in the newly loaded preset.
                    // At startup this is handled by a dedicated post-load loop in lib.rs,
                    // but at runtime (tab-bar switch) only this handler runs, so we must
                    // trigger the fetches here.
                    // ----------------------------------------------------------------
                    let _t_request_bars = web_time::Instant::now();
                    let bar_count = self.core.user().profile().domain.bar_count as usize;
                    let mut bars_requested: usize = 0;
                    // Phase 4 S2.7.1c: read via branch_meta (B6 tuple-collect flip).
                    // §3.4 (handoff 2026-07-06): pre-collect chart_type per window
                    // so the kline `request_bars` below can skip derived windows —
                    // `ignite_derived_windows` fires their matching bridge sub.
                    let window_bar_data: Vec<(mlc_core::state::ChartId, String, String, mlc_core::state::Timeframe, mlc_core::instrument::AccountType, &'static str)> = self
                        .panel_app
                        .panel_grid
                        .iter_containers()
                        .map(|(_, w)| {
                            let cid = w.id;
                            let sym = self.symbol_for_chart(cid)
                                .map(String::from).unwrap_or_default();
                            let exch = self.exchange_for_chart(cid)
                                .map(String::from).unwrap_or_default();
                            let tf = self.timeframe_for_chart(cid).unwrap_or_else(mlc_core::state::Timeframe::h1);
                            let at = self.account_type_for_chart(cid).unwrap_or_default();
                            let ct = self.chart_type_for_chart(cid).unwrap_or("candles");
                            (cid, sym, exch, tf, at, ct)
                        })
                        .collect();
                    for (_cid, sym, exch, tf, at_typed, ct) in &window_bar_data {
                        let eid = digdigdig3::ExchangeId::from_str(&exch)
                            .unwrap_or(digdigdig3::ExchangeId::Binance);
                        if !self.sidebar_state.connector_enabled.get(eid.as_str()).copied().unwrap_or(true) {
                            continue;
                        }
                        if crate::active_series::chart_type_is_derived_seed(ct) {
                            continue;
                        }
                        let at = mlc_backend_data_bridge::account_type_to_dig3(*at_typed);
                        self.core.ensure_connector(eid);
                        self.core.request_kline(eid, sym, tf, at, None, Some(bar_count), true);
                        bars_requested += 1;
                    }
                    // §3.4: derived windows go through the shared helper —
                    // same code path as startup / new-window Post-LoadPreset paths.
                    self.ignite_derived_windows();
                    eprintln!("[ChartApp] LoadPreset: requesting bars for {} windows", bars_requested);
                    // Default companions (klines → VOLUME, footprint → CVD)
                    // for the freshly hydrated windows — ChangeChartType
                    // never ran for them.
                    self.sync_default_indicators_for_all_windows();
                    let request_bars_us = _t_request_bars.elapsed().as_micros();

                    // Prune orphan Panel members from all restored sync groups.
                    // SyncGroupSnapshot.panel_members can carry ids from the prior
                    // session that were closed-then-saved in a race; those ids
                    // have no entry in panels_store after Step 8b. Leaving them
                    // in group.members + member_to_group leaks zombie references
                    // that mis-route propagate_* / apply_key_to_panels_in_group.
                    let _t_orphan_prune = web_time::Instant::now();
                    {
                        use mlc_core::ids::SyncMemberId;
                        use mlc_core::state::PanelId;
                        // Collect live PanelIds across every panel-state map.
                        let live_panel_ids: std::collections::HashSet<u64> = {
                            let store = &self.panels_store;
                            let mut s: std::collections::HashSet<u64> = std::collections::HashSet::new();
                            for PanelId(id) in store.dom.keys() { s.insert(*id); }
                            for PanelId(id) in store.footprint.keys() { s.insert(*id); }
                            for PanelId(id) in store.volume_profile.keys() { s.insert(*id); }
                            for PanelId(id) in store.liquidity_heatmap.keys() { s.insert(*id); }
                            for PanelId(id) in store.big_trades.keys() { s.insert(*id); }
                            for PanelId(id) in store.l2_tape.keys() { s.insert(*id); }
                            for PanelId(id) in store.trade_tape.keys() { s.insert(*id); }
                            for PanelId(id) in store.order_entry.keys() { s.insert(*id); }
                            for PanelId(id) in store.position_manager.keys() { s.insert(*id); }
                            for PanelId(id) in store.trade_log.keys() { s.insert(*id); }
                            for PanelId(id) in store.risk_calculator.keys() { s.insert(*id); }
                            for PanelId(id) in store.trading_container.keys() { s.insert(*id); }
                            s
                        };
                        // Collect orphans (Panel members in any group with no live state).
                        let mut orphans: Vec<SyncMemberId> = Vec::new();
                        for (_, tag) in self.core.tags().iter() {
                            for &member in &tag.members {
                                if let SyncMemberId::Panel(pid) = member {
                                    if !live_panel_ids.contains(&pid) {
                                        orphans.push(SyncMemberId::Panel(pid));
                                    }
                                }
                            }
                        }
                        let pruned = orphans.len();
                        for member in orphans {
                            self.remove_panel_completely(member);
                        }
                        if pruned > 0 {
                            eprintln!("[ChartApp] preset restore: pruned {} orphan panel members from sync groups", pruned);
                        }
                    }
                    let orphan_prune_us = _t_orphan_prune.elapsed().as_micros();

        // ── Window↔bubble pointer sync sweep ─────────────────────────────
        // Many code paths attach a chart to a bubble via
        // `attach_chart_to_bubble` (which updates BubbleStore.member_to_bubble)
        // WITHOUT also writing `window.auto_tag_id / current_tag_id`.
        // The writer reads `bubble_for_chart(cid)` (canonical); the renderer
        // reads `window.current_tag_id`. When they diverge, the writer
        // fills the right bubble and the renderer reads the wrong (empty)
        // one — the chart stays blank forever.
        //
        // This sweep is the single source-of-truth fixup: snapshot
        // `bubble_for_chart` per container BEFORE taking containers_mut,
        // then write the result onto each window's two bubble pointers.
        // Cheap (one HashMap lookup per chart) and idempotent.
        {
            let bid_by_chart: std::collections::HashMap<mlc_core::ChartId, mlc_core::ids::BubbleId> =
                self.panel_app.panel_grid.containers().keys()
                    .filter_map(|cid| self.bubble_for_chart(*cid).map(|bid| (*cid, bid)))
                    .collect();
            for (cid, bid) in &bid_by_chart {
                if let Some(window) = self.panel_app.panel_grid.containers_mut().get_mut(cid) {
                    if window.current_tag_id != *bid {
                        window.current_tag_id = *bid;
                    }
                    if window.auto_tag_id != *bid && window.auto_tag_id == mlc_core::ids::BubbleId(0) {
                        window.auto_tag_id = *bid;
                    }
                }
            }
        }

        let lp_cold_body_us = lp_cold_start.elapsed().as_micros();
        eprintln!("[ChartApp] preset '{}' fully restored (cold_body={}us total={}us)",
            preset.name, lp_cold_body_us, _lp_total.elapsed().as_micros());

        // ── COLD BODY PER-STEP BREAKDOWN ──────────────────────────────────────
        //
        // Sum of these = cold_body_us above (modulo overhead of timer reads
        // themselves, which is in the tens of nanoseconds per call).
        //
        // Layout above lists steps in the order they execute. Look for the
        // biggest bucket — that's where Stage 4 (Send carve-out + worker
        // dispatch) buys you the most off-main-thread time. Steps that
        // touch `self.core.bridge.*` (`depth_subs`, `unsubs`, `request_bars`,
        // and parts of `slots`) cannot move off main today; everything
        // else is pure CPU and is the target of the worker thread.
        eprintln!(
            "[ChartApp] cold_body[{}] breakdown (us): prep={} old_subs={} layout={} bubbles={} rebind={} seed_ids={} orphans={} indicators={} recalc[{}wins]={} sub_panes={} leaf_tags={} alerts={} slots={} depth_subs={} unsubs={} request_bars={} orphan_prune={}",
            preset.name,
            prep_us,
            old_subs_us,
            layout_us,
            bubbles_us,
            rebind_us,
            seed_ids_us,
            orphans_us,
            indicators_us,
            recalc_n,
            recalc_us,
            sub_panes_us,
            leaf_tags_us,
            alerts_us,
            slots_us,
            depth_subs_us,
            unsubs_us,
            request_bars_us,
            orphan_prune_us,
        );
    }
}
