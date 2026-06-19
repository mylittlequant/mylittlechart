//! Sync group propagation: replicate state changes (symbol, timeframe, drawings,
//! crosshair, indicators) across all charts in the same sync group.
//! Also handles split, desync, color-group join.

use crate::ChartApp;
use super::sync_colors_match;

impl ChartApp {
    /// Hide crosshairs on all split leaves that are NOT in the same sync group
    /// as `source_leaf`.  Sync-group peers are updated by
    /// `propagate_crosshair_to_sync_group`; this covers the non-peer leaves.
    pub(super) fn hide_crosshairs_outside_sync_group(&mut self, source_leaf: mlc_core::LeafId) {
        let source_color = self.panel_app.leaf_color_tags.get(&source_leaf).copied();
        let crosshair_sync_enabled = self.panel_app.panel_grid
            .container_id_for_leaf(source_leaf)
            .and_then(|cid| self.managers.bubble.bubble_for_chart(cid))
            .and_then(|gid| self.managers.bubble.bubble(gid))
            .map(|g| g.sync_flags.sync_crosshair)
            .unwrap_or(true);
        let all_ids: Vec<mlc_core::LeafId> = self.panel_app
            .panel_grid
            .panel_rects()
            .keys()
            .copied()
            .filter(|&id| id != source_leaf)
            .collect();
        for other_id in all_ids {
            let in_sync_group = source_color
                .and_then(|sc| self.panel_app.leaf_color_tags.get(&other_id).copied()
                    .map(|c| sync_colors_match(sc, c)))
                .unwrap_or(false);
            if !(in_sync_group && crosshair_sync_enabled) {
                if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(other_id) {
                    window.crosshair.visible = false;
                }
            }
        }
    }

    // =========================================================================
    // Sync group helpers
    // =========================================================================

    /// Desync a leaf from its color tag sync group.
    ///
    /// Removes the color tag, purges cloned drawing primitives, and purges
    /// cloned indicator instances that were added when the split occurred.
    /// Disconnect a trading panel from its color sync group and move it to a private
    /// auto-created group so it is no longer visually tagged.
    pub(super) fn perform_panel_desync(&mut self, panel_id: u64) {
        use mlc_core::tag_manager::SyncMemberId;
        let member = SyncMemberId::Panel(panel_id);
        self.managers.bubble.disconnect(member);
        let gid = self.managers.bubble.create_group_auto(
            [0.5, 0.5, 0.5, 1.0],
            String::new(),
            mlc_core::state::Timeframe::m1(),
        );
        self.managers.bubble.set_auto_group(member, gid);
        eprintln!("[ChartApp] Panel {} desynced → auto group {:?}", panel_id, gid);
        self.sidebar_data_dirty = true;
        self.autosave_snapshot();
    }

    pub(super) fn perform_desync(&mut self, leaf_id: mlc_core::LeafId) {
        // 1. Remove color tag.
        self.panel_app.leaf_color_tags.remove(&leaf_id);

        // Snapshot group info BEFORE disconnect.
        let had_group_id = self.panel_app.panel_grid
            .container_id_for_leaf(leaf_id)
            .and_then(|cid| self.managers.bubble.bubble_for_chart(cid))
            .is_some();

        let chart_id = match self.panel_app.panel_grid.container_id_for_leaf(leaf_id) {
            Some(id) => id,
            None => {
                eprintln!("[sync_group] perform_desync: container_for_leaf({:?}) returned None — skipping desync", leaf_id);
                return;
            }
        };

        // Read auto_bubble_id BEFORE disconnect so we can re-attach.
        let auto_bubble_id = self.panel_app.panel_grid
            .containers()
            .get(&chart_id)
            .map(|w| w.auto_bubble_id);

        // 1b. Disconnect from current color-tag group.
        if let Some(old_group_id) = self.managers.bubble.detach_chart_from_bubble(chart_id) {
            let remaining = self.managers.bubble
                .members(old_group_id)
                .map_or(0, |m| m.len());
            eprintln!(
                "[BubbleStore] Disconnected chart {:?} from group {:?} ({} remaining)",
                chart_id, old_group_id, remaining
            );
        }

        // 2. Re-attach chart to its permanent home auto-bubble and restore state.
        let pid_desync = self.panel_app.panel_grid
            .containers()
            .get(&chart_id)
            .map(|w| w.chart_panel_id);

        if let Some(auto_bid) = auto_bubble_id {
            // Re-attach to home auto-bubble.
            let _ = self.managers.bubble.attach_chart_to_bubble(chart_id, auto_bid);

            if had_group_id {
                // Restore drawing_manager from auto_bubble.primitives (the home drawings
                // that were preserved during the sync-group membership).
                // Read pre_tag_ids from auto_bubble BEFORE taking command_history so
                // we can do the indicator cleanup step below.
                let (pre_tag_ids, restored_prims, old_history) = {
                    let auto_bub = self.managers.bubble.bubble_mut(auto_bid);
                    if let Some(bub) = auto_bub {
                        let pre_tag = bub.pre_tag_indicator_ids.clone();
                        bub.pre_tag_indicator_ids.clear();
                        let prims: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>> =
                            bub.primitives.iter().map(|p| p.clone_box()).collect();
                        // Swap window history back out of auto_bubble.
                        // auto_bubble.command_history holds the stashed pre-join history.
                        let old = std::mem::replace(
                            &mut bub.command_history,
                            mlc_core::CommandHistory::new(250),
                        );
                        (pre_tag, prims, Some(old))
                    } else {
                        (Vec::new(), Vec::new(), None)
                    }
                };

                // Apply restored state to window and drawing_manager.
                if let Some(pid) = pid_desync {
                    if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                        panel.drawing_manager.clear_all_primitives();
                        if !restored_prims.is_empty() {
                            let count = restored_prims.len();
                            panel.drawing_manager.add_synced_primitives(restored_prims);
                            eprintln!("[BubbleStore] Desync: restored {} primitives from auto-bubble", count);
                        } else {
                            eprintln!("[BubbleStore] Desync: auto-bubble had no primitives (fresh chart)");
                        }
                    }
                }

                // Restore command_history into the window.
                if let Some(h) = old_history {
                    if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                        window.command_history = h;
                        eprintln!("[BubbleStore] Desync: restored command history from auto-bubble");
                    }
                }

                // Update current_bubble_id on the window.
                if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                    window.current_bubble_id = auto_bid;
                }

                // Rebind drawing_manager to the auto-bubble's style store.
                if let Some(pid) = pid_desync {
                    if let Some(style_arc) = self.managers.bubble
                        .bubble(auto_bid)
                        .map(|g| g.last_used_style.clone())
                    {
                        if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                            panel.drawing_manager.bind_style_store(style_arc);
                        }
                    }
                }

                // Restore the window's pre-join scale mode.
                let leaf_bars_desync: Vec<mlc_core::Bar> = self.bars_for_leaf(leaf_id);
                if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                    if let Some(prev_mode) = window.stashed_scale_mode.take() {
                        window.price_scale.scale_mode = prev_mode;
                        let is_auto = prev_mode.is_auto_y();
                        for state in window.sub_pane_y_state.values_mut() {
                            state.auto_scale = is_auto;
                        }
                        if is_auto {
                            window.calc_auto_scale(&leaf_bars_desync);
                        }
                        eprintln!("[BubbleStore] Desync: restored stashed scale mode {:?}", prev_mode);
                    }
                }

                // Remove tag indicators; unhide pre-tag indicators.
                let to_remove: Vec<u64> = self.indicator_manager
                    .instances_iter()
                    .filter(|i| {
                        i.window_id == Some(chart_id.0)
                            && !pre_tag_ids.contains(&i.id)
                    })
                    .map(|i| i.id)
                    .collect();
                let remove_count = to_remove.len();
                for id in &to_remove {
                    self.indicator_manager.remove_instance(*id);
                }
                for &id in &pre_tag_ids {
                    if let Some(inst) = self.indicator_manager.get_instance_mut(id) {
                        inst.visible = true;
                    }
                }
                eprintln!(
                    "[BubbleStore] Desync: removed {} tag indicators, unhid {} pre-tag",
                    remove_count, pre_tag_ids.len()
                );

                // Sync drawing_manager primitives back into the auto-bubble so they
                // are the authoritative home-state for future join cycles.
                let dm_prims: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>> =
                    pid_desync
                        .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                        .map(|panel| panel.drawing_manager.primitives().iter().map(|p| p.clone_box()).collect())
                        .unwrap_or_default();
                if let Some(auto_bub) = self.managers.bubble.bubble_mut(auto_bid) {
                    auto_bub.primitives = dm_prims;
                }
            } else {
                // Was not in a color group — just purge synced primitives and update ids.
                if let Some(pid) = pid_desync {
                    if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                        panel.drawing_manager.purge_synced_primitives();
                    }
                }
                self.indicator_manager.purge_synced_instances_for_window(chart_id.0);
                if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                    window.current_bubble_id = auto_bid;
                }
            }

            eprintln!("[BubbleStore] Desync: chart {:?} re-attached to auto-bubble {:?}", chart_id, auto_bid);
        } else {
            eprintln!(
                "[BubbleStore] Desync: chart {:?} has sentinel auto_bubble_id — construction site missed post-init. Skipping desync.",
                chart_id
            );
            return;
        }

        // 5. Recalculate sub-panes so indicator panels stay in sync.
        self.sync_sub_panes_from_manager();
        self.sidebar_data_dirty = true;

        eprintln!("[ChartApp] Desynced leaf {:?} from color tag group", leaf_id);
    }

    /// Join an existing color group: clone primitives and indicators from a peer
    /// that already belongs to this color group into the joining leaf.
    pub(super) fn sync_join_color_group(&mut self, joining_leaf: mlc_core::LeafId, color: [f32; 4]) {
        // Connect the joining leaf's chart to the TagManager group for this color.
        // Find-or-create the group so TagManager stays consistent regardless of
        // whether this is the first leaf in the group or a later joiner.
        if let Some(chart_id) = self.panel_app.panel_grid.container_id_for_leaf(joining_leaf) {
            let existing_group = self.managers.bubble.find_group_by_color(color);
            let is_new_group = existing_group.is_none();
            let group_id = if let Some(gid) = existing_group {
                gid
            } else {
                if self.panel_app.panel_grid.container_for_leaf(joining_leaf).is_none() {
                    eprintln!("[sync_group] sync_join_color_group: container_for_leaf({:?}) returned None — skipping join", joining_leaf);
                    return;
                }
                let sym = self.managers.bubble.symbol_for_chart(chart_id)
                    .map(String::from)
                    .unwrap_or_default();
                let tf = self.managers.bubble.timeframe_for_chart(chart_id)
                    .cloned()
                    .unwrap_or_else(mlc_core::state::Timeframe::h1);
                self.managers.bubble.create_group(color, sym, tf)
            };
            let _ = self.managers.bubble.attach_chart_to_bubble(chart_id, group_id);
            eprintln!(
                "[BubbleStore] Connected chart {:?} to group {:?} (new={})",
                chart_id, group_id, is_new_group
            );

            // Rebind the joining window's DM to the group's shared style store so
            // last-used style changes are immediately visible to all group peers.
            let pid_join = self.panel_app.panel_grid.container_for_leaf(joining_leaf).map(|w| w.chart_panel_id);
            if let Some(style_arc) = self.managers.bubble
                .bubble(group_id)
                .map(|g| g.last_used_style.clone())
            {
                if let Some(pid) = pid_join {
                    if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                        panel.drawing_manager.bind_style_store(style_arc);
                    }
                }
            }

            // Snapshot pre-tag indicator ids (these survive desync).
            let pre_tag_ids: Vec<u64> = self.indicator_manager
                .instances_iter()
                .filter(|i| i.window_id == Some(chart_id.0) && i.origin_id.is_none())
                .map(|i| i.id)
                .collect();

            if is_new_group {
                // NEW GROUP: seed tag with window's current state.
                // Auto-bubble holds home drawings (no explicit stash needed).
                // Pre-render sync will fill drawing_manager from group each frame.

                // Seed group.scale_mode from the creating window's current mode so the
                // group starts with the correct A/M/F and any later joiners inherit it.
                let seed_mode = self.panel_app.panel_grid
                    .container_for_leaf(joining_leaf)
                    .map(|w| w.price_scale.scale_mode);
                if let (Some(mode), Some(group)) = (seed_mode, self.managers.bubble.bubble_mut(group_id)) {
                    group.scale_mode = mode;
                }

                // Get auto_bubble_id before taking mutable window borrow.
                let auto_bid = self.panel_app.panel_grid
                    .containers()
                    .get(&chart_id)
                    .map(|w| w.auto_bubble_id);

                // Stash command_history and pre_tag_ids into auto_bubble.
                // Replace window.command_history with a fresh one so group ops are independent.
                if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(joining_leaf) {
                    let old_history = std::mem::replace(
                        &mut window.command_history,
                        mlc_core::CommandHistory::new(250),
                    );
                    if let Some(bid) = auto_bid {
                        if let Some(auto_bub) = self.managers.bubble.bubble_mut(bid) {
                            auto_bub.command_history = old_history;
                            auto_bub.pre_tag_indicator_ids = pre_tag_ids.clone();
                        }
                    }
                    // Update current_bubble_id to the new color group.
                    window.current_bubble_id = group_id;
                }

                // Seed group.primitives from auto_bubble.primitives (clone + re-ID).
                // auto_bubble.primitives already holds the chart's home drawings.
                let (prim_symbol, prim_exchange, prim_account_type) = {
                    let sym = self.managers.bubble.symbol_for_chart(chart_id)
                        .map(String::from)
                        .unwrap_or_default();
                    let exch = self.managers.bubble.exchange_for_chart(chart_id)
                        .map(String::from)
                        .unwrap_or_default();
                    let at = self.managers.bubble.account_type_for_chart(chart_id)
                        .map(String::from)
                        .unwrap_or_else(|| "S".to_string());
                    (sym, exch, at)
                };
                let source_prims: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>> =
                    if let Some(bid) = auto_bid {
                        self.managers.bubble.bubble(bid)
                            .map(|bub| bub.primitives.iter().map(|p| p.clone_box()).collect())
                            .unwrap_or_default()
                    } else {
                        // Fallback: read from drawing_manager if no valid auto_bubble.
                        pid_join.and_then(|pid| self.panels_store.chart_panel.get(&pid))
                            .map(|panel| panel.drawing_manager.primitives().iter()
                                .filter(|p| p.data().origin_id.is_none())
                                .map(|p| p.clone_box())
                                .collect())
                            .unwrap_or_default()
                    };
                let mut group_prims = source_prims;
                for p in &mut group_prims {
                    let d = p.data_mut();
                    d.symbol = prim_symbol.clone();
                    d.exchange = prim_exchange.clone();
                    d.account_type = prim_account_type.clone();
                    // Re-ID group copies so they are globally unique; auto_bubble keeps originals.
                    d.id = mlc_core::drawing::alloc_primitive_id();
                }
                if let Some(group) = self.managers.bubble.bubble_mut(group_id) {
                    group.primitives = group_prims;
                }

                // Seed indicator configs from the creating window's indicators.
                let (seed_symbol, seed_exchange, seed_account_type) = {
                    let sym = self.managers.bubble.symbol_for_chart(chart_id)
                        .map(String::from)
                        .unwrap_or_default();
                    let exch = self.managers.bubble.exchange_for_chart(chart_id)
                        .map(String::from)
                        .unwrap_or_default();
                    let at = self.managers.bubble.account_type_for_chart(chart_id)
                        .map(String::from)
                        .unwrap_or_else(|| "S".to_string());
                    (sym, exch, at)
                };
                let mut configs: Vec<mlc_core::tag_manager::IndicatorGroupConfig> = self
                    .indicator_manager
                    .instances_iter()
                    .filter(|i| i.origin_id.is_none() && i.window_id == Some(chart_id.0))
                    .map(|i| mlc_core::tag_manager::IndicatorGroupConfig {
                        id: i.id,
                        type_id: i.type_id.clone(),
                        name: i.name.clone(),
                        params: std::collections::HashMap::new(),
                        pane: i.pane as u32,
                        visible: i.visible,
                        symbol: seed_symbol.clone(),
                        exchange: seed_exchange.clone(),
                        account_type: seed_account_type.clone(),
                    })
                    .collect();
                // Sort by id to preserve original creation order (HashMap iteration is random).
                configs.sort_by_key(|c| c.id);
                if let Some(group) = self.managers.bubble.bubble_mut(group_id) {
                    eprintln!(
                        "[BubbleStore] Seeded new tag {:?} with {} primitives, {} indicators",
                        group_id, group.primitives.len(), configs.len()
                    );
                    group.indicator_configs = configs;
                }
                return;
            }

            // EXISTING GROUP: auto_bubble holds home drawings (no explicit stash needed).
            // Clear drawing_manager; per-frame sync will fill it from group each frame.

            // When sync_viewport is ON for this group, stash the window's current
            // scale mode and apply the group mode. F (Focus) implies a pinned viewport,
            // so A/M/F must follow the group while viewport sync is active.
            let viewport_sync_on = self.managers.bubble.bubble(group_id)
                .map(|g| g.sync_flags.sync_viewport)
                .unwrap_or(false);
            if viewport_sync_on {
                let group_mode = self.managers.bubble.bubble(group_id)
                    .map(|g| g.scale_mode);
                let joining_bars: Vec<mlc_core::Bar> = self.bars_for_leaf(joining_leaf);
                if let (Some(group_mode), Some(window)) = (
                    group_mode,
                    self.panel_app.panel_grid.container_for_leaf_mut(joining_leaf),
                ) {
                    if window.stashed_scale_mode.is_none() {
                        window.stashed_scale_mode = Some(window.price_scale.scale_mode);
                    }
                    window.price_scale.scale_mode = group_mode;
                    let is_auto = group_mode.is_auto_y();
                    for state in window.sub_pane_y_state.values_mut() {
                        state.auto_scale = is_auto;
                    }
                    if is_auto {
                        window.calc_auto_scale(&joining_bars);
                    }
                }
            }

            // Clear drawing_manager; group prims arrive via per-frame sync.
            if let Some(pid) = pid_join {
                if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                    panel.drawing_manager.clear_all_primitives();
                }
            }

            // Get auto_bubble_id before taking mutable window borrow.
            let auto_bid_existing = self.panel_app.panel_grid
                .containers()
                .get(&chart_id)
                .map(|w| w.auto_bubble_id);

            // Stash command_history and pre_tag_ids into auto_bubble.
            if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(joining_leaf) {
                let old_history = std::mem::replace(
                    &mut window.command_history,
                    mlc_core::CommandHistory::new(250),
                );
                if let Some(bid) = auto_bid_existing {
                    if let Some(auto_bub) = self.managers.bubble.bubble_mut(bid) {
                        auto_bub.command_history = old_history;
                        auto_bub.pre_tag_indicator_ids = pre_tag_ids.clone();
                        eprintln!(
                            "[BubbleStore] Stashed command_history and {} pre_tag_ids into auto-bubble {:?}",
                            pre_tag_ids.len(), bid
                        );
                    }
                }
                // Update current_bubble_id to the existing color group.
                window.current_bubble_id = group_id;
            }

            // Hide pre-tag indicators by setting visible=false.
            for &id in &pre_tag_ids {
                if let Some(inst) = self.indicator_manager.get_instance_mut(id) {
                    inst.visible = false;
                }
            }
            eprintln!("[BubbleStore] Hid {} pre-tag indicators", pre_tag_ids.len());

            // Sync tag indicators to joining window.
            let leaf_chart_ids = vec![(joining_leaf, chart_id)];
            self.sync_group_indicators_to_new_members(group_id, &leaf_chart_ids);
            self.sync_sub_panes_from_manager();
            self.sidebar_data_dirty = true;
            eprintln!("[BubbleStore] Join existing group: cleared window drawings, synced tag content");
        }

        // Dead fallback removed — TagManager handles all sync join logic above.
    }

    /// Split the active leaf: snapshot its color tag before split (the old
    /// LeafId is destroyed by `split_leaf`), then assign inherited or new color
    /// to all resulting leaves and propagate crosshair/viewport.
    pub(super) fn do_split(&mut self, kind: mlc_core::SplitKind) {
        // Snapshot color, group_id, and chart_id BEFORE split — active_leaf will be destroyed.
        let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
        let pre_split_color = active_leaf
            .and_then(|lid| self.panel_app.leaf_color_tags.remove(&lid));
        let pre_split_group_id = active_leaf
            .and_then(|lid| self.panel_app.panel_grid.container_id_for_leaf(lid))
            .and_then(|cid| self.managers.bubble.bubble_for_chart(cid));
        let pre_split_chart_id = active_leaf
            .and_then(|lid| self.panel_app.panel_grid.container_id_for_leaf(lid));

        let new_leaves = self.panel_app.panel_grid.split_active(kind);

        // Assign fresh auto-bubbles for any split-produced charts that still carry
        // the BubbleId(0) sentinel (clone_for_split_shell sets the sentinel so
        // do_split is the designated fix-up site).
        {
            let needs_auto: Vec<mlc_core::state::ChartId> = new_leaves
                .iter()
                .filter_map(|&lid| self.panel_app.panel_grid.container_id_for_leaf(lid))
                .filter(|&cid| {
                    self.panel_app.panel_grid.containers()
                        .get(&cid)
                        .map(|w| w.auto_bubble_id == mlc_core::tag_manager::BubbleId(0))
                        .unwrap_or(false)
                })
                .collect();
            for cid in needs_auto {
                let sym = self.managers.bubble.symbol_for_chart(cid)
                    .map(String::from)
                    .unwrap_or_default();
                let tf = self.managers.bubble.timeframe_for_chart(cid)
                    .cloned()
                    .unwrap_or_else(mlc_core::state::Timeframe::h1);
                let auto_bid = self.managers.bubble.create_group_auto(
                    [0.0, 0.0, 0.0, 0.0], sym, tf,
                );
                if let Some(window) = self.panel_app.panel_grid.containers_mut().get_mut(&cid) {
                    window.auto_bubble_id = auto_bid;
                    window.current_bubble_id = auto_bid;
                }
                let _ = self.managers.bubble.attach_chart_to_bubble(cid, auto_bid);
                eprintln!("[BubbleStore] do_split: created auto-bubble {:?} for chart {:?}", auto_bid, cid);
            }
        }

        self.propagate_crosshair_after_split(&new_leaves);

        // Phase 4 B: seed branch_meta for every new leaf produced by the
        // split. Instrument fields are no longer stored on ChartContainer —
        // read them from the source window's branch_meta entry and propagate
        // to all new leaves (they are all clones of the same source).
        //
        // Two-phase: collect chart_ids while holding shared panel_grid borrow,
        // then mirror with mut borrow on branch_meta after the read borrow drops.
        let new_chart_ids: Vec<mlc_core::state::ChartId> = new_leaves
            .iter()
            .filter_map(|&lid| self.panel_app.panel_grid.container_id_for_leaf(lid))
            .collect();

        // Read source instrument from branch_meta (pre_split_chart_id is the source).
        let source_sym = pre_split_chart_id
            .and_then(|cid| self.managers.bubble.symbol_for_chart(cid))
            .map(String::from)
            .unwrap_or_default();
        let source_tf = pre_split_chart_id
            .and_then(|cid| self.managers.bubble.timeframe_for_chart(cid))
            .cloned()
            .unwrap_or_else(mlc_core::state::Timeframe::h1);
        let source_exch = pre_split_chart_id
            .and_then(|cid| self.managers.bubble.exchange_for_chart(cid))
            .map(String::from)
            .unwrap_or_default();
        let source_at = pre_split_chart_id
            .and_then(|cid| self.managers.bubble.account_type_for_chart(cid))
            .map(String::from)
            .unwrap_or_else(|| "S".to_string());

        for chart_id in new_chart_ids {
            if let Some(bid) = self.managers.bubble.bubble_for_chart(chart_id) {
                if let Some(bubble) = self.managers.bubble.bubble_mut(bid) {
                    bubble.symbol = source_sym.clone();
                    bubble.timeframe = source_tf.clone();
                    bubble.exchange = source_exch.clone();
                    bubble.account_type = source_at.clone();
                }
            }
            // Containers born in this split carry a fresh ChartPanelId with NO
            // ChartPanelState behind it — the restore cycle (LoadPreset) inserts
            // panels explicitly, but the live cycle never did. Every non-lazy
            // panel read (drawing clicks, toolbar tool activation, sub-pane
            // interactions) then silently no-ops on the sibling leaf — proven
            // live: canvas_click pid=ChartPanelId(11) panel_exists=false.
            // Create the panel and stamp it with the source instrument.
            let pid_opt = self
                .panel_app
                .panel_grid
                .containers()
                .get(&chart_id)
                .map(|w| w.chart_panel_id);
            if let Some(pid) = pid_opt {
                let style_arc = self
                    .managers
                    .bubble
                    .bubble_for_chart(chart_id)
                    .and_then(|bid| self.managers.bubble.bubble(bid))
                    .map(|g| g.last_used_style.clone());
                let panel = crate::chart_panel_bridge::ensure_chart_panel_state(
                    &mut self.panels_store,
                    pid,
                );
                panel.drawing_manager.set_current_symbol_key(
                    &source_sym,
                    &source_exch,
                    &source_at,
                );
                if let Some(style_arc) = style_arc {
                    panel.drawing_manager.bind_style_store(style_arc);
                }
            }
        }

        // When "Split Without Group" is enabled, the mother window (new_leaves[0])
        // keeps its existing group/tag state.  Only the new sibling leaves
        // (new_leaves[1..]) get fresh independent auto_created groups.
        if self.split_without_group && !new_leaves.is_empty() {
            // Reconnect the mother (new_leaves[0]) to its pre-split group.
            // split_active may have reassigned the chart ID, so we re-establish
            // the TagManager link rather than assuming old_cid is still valid.
            if let Some(mother_leaf) = new_leaves.first().copied() {
                if let (Some(old_cid), Some(old_gid)) = (pre_split_chart_id, pre_split_group_id) {
                    if let Some(new_cid) = self.panel_app.panel_grid.container_id_for_leaf(mother_leaf) {
                        if new_cid != old_cid {
                            // The chart got a new ID after the split — connect the new one.
                            let _ = self.managers.bubble.attach_chart_to_bubble(new_cid, old_gid);
                            eprintln!("[BubbleStore] (SplitUntagged) Mother re-connected new cid {:?} → group {:?}", new_cid, old_gid);
                        }
                        // else: same chart_id survived the split, TagManager link is intact.
                    }
                    // BubbleStore reverse index maintained by attach_chart_to_bubble above.
                    // Restore the mother's color tag that was snapshotted before the split.
                    if let Some(color) = pre_split_color {
                        self.panel_app.leaf_color_tags.insert(mother_leaf, color);
                    }
                    eprintln!("[BubbleStore] (SplitUntagged) Mother leaf {:?} keeps group {:?}", mother_leaf, old_gid);
                }
            }

            // Give each NEW (non-mother) leaf its own fresh auto_created group.
            for &leaf_id in new_leaves.iter().skip(1) {
                let new_chart_id = self.panel_app.panel_grid.container_id_for_leaf(leaf_id);
                if self.panel_app.panel_grid.container_for_leaf(leaf_id).is_none() {
                    eprintln!("[sync_group] split untagged: container_for_leaf({:?}) returned None — skipping new leaf", leaf_id);
                    continue;
                }
                let symbol = new_chart_id
                    .and_then(|c| self.managers.bubble.symbol_for_chart(c))
                    .map(String::from)
                    .unwrap_or_default();
                let timeframe = new_chart_id
                    .and_then(|c| self.managers.bubble.timeframe_for_chart(c))
                    .cloned()
                    .unwrap_or_else(mlc_core::state::Timeframe::h1);
                let gid = self.managers.bubble.create_group_auto(
                    [0.0, 0.0, 0.0, 0.0],
                    symbol,
                    timeframe,
                );
                if let Some(chart_id) = new_chart_id {
                    let _ = self.managers.bubble.attach_chart_to_bubble(chart_id, gid);
                }
                // BubbleStore reverse index updated by attach_chart_to_bubble above.
                self.panel_app.leaf_color_tags.remove(&leaf_id);
                eprintln!("[BubbleStore] (SplitUntagged) Created auto group {:?} for new leaf {:?}", gid, leaf_id);
            }
            self.sync_sub_panes_from_manager();
            return;
        }

        // TagManager: if the source was already in a user-tagged group, connect new
        // leaves to THAT group.  If the source was in an auto_created group (every
        // window gets one automatically), we must NOT share it — each split window
        // must get its own fresh auto group so primitives don't bleed between
        // supposedly independent windows.  If there was no group at all, create a
        // new user-visible group (same as before).
        if !new_leaves.is_empty() {
            // Determine whether the pre-existing group is auto_created.
            let pre_split_is_auto = pre_split_group_id
                .and_then(|gid| self.managers.bubble.bubble(gid))
                .map(|g| g.auto_created)
                .unwrap_or(false);

            // Disconnect the old chart from its group (leaf destroyed by split).
            if let (Some(old_cid), Some(old_gid)) = (pre_split_chart_id, pre_split_group_id) {
                let _ = self.managers.bubble.detach_chart_from_bubble(old_cid);
                eprintln!("[BubbleStore] Disconnected destroyed chart {:?} from group {:?}", old_cid, old_gid);
                // Bug C fix: remove the old group if it became empty and was auto-created.
                if let Some(g) = self.managers.bubble.bubble(old_gid) {
                    if g.auto_created && g.members.is_empty() {
                        self.managers.bubble.remove_group(old_gid);
                        eprintln!("[BubbleStore] Removed empty auto group {:?} after split", old_gid);
                    }
                }
            }

            // Resolve first-leaf window once — needed by both auto-promote and no-group branches.
            let first_leaf_win = self.panel_app.panel_grid.container_for_leaf(new_leaves[0]);
            if first_leaf_win.is_none() {
                eprintln!("[sync_group] split tagged: container_for_leaf({:?}) returned None — skipping split tag", new_leaves[0]);
                return;
            }
            let first_leaf_chart_id = self.panel_app.panel_grid.container_id_for_leaf(new_leaves[0]);
            // Use the source instrument captured BEFORE the detach above:
            // re-reading symbol_for_chart at this point always returned None
            // (the chart's bubble membership was just severed), so every
            // freshly-split group was created with an EMPTY symbol — which
            // then became the authority for all leaves (blank corner tags,
            // broken sym:exch:AT panel keys, dead routing in secondary leaves).
            let first_leaf_symbol = source_sym.clone();
            let first_leaf_timeframe = source_tf.clone();
            let mut created_fresh_group = false;
            let group_id = if let Some(existing_group) = pre_split_group_id {
                if pre_split_is_auto {
                    // Was untagged (auto group) — split promotes to a real color tag.
                    // Create a NEW visible group with the first free color.
                    let color = self.managers.bubble.next_unused_color();
                    let gid = self.managers.bubble.create_group(color, first_leaf_symbol, first_leaf_timeframe);
                    created_fresh_group = true;
                    eprintln!("[BubbleStore] Split promoted auto group to visible tag {:?}", gid);
                    gid
                } else {
                    // User-tagged group: all split windows intentionally share it.
                    existing_group
                }
            } else {
                // No existing group — pick color from TagManager's palette.
                let color = pre_split_color
                    .unwrap_or_else(|| self.managers.bubble.next_unused_color());
                created_fresh_group = true;
                self.managers.bubble.create_group(color, first_leaf_symbol, first_leaf_timeframe)
            };

            // create_group leaves exchange/account_type empty — complete the
            // identity record from the source instrument for groups born here.
            // (Existing user-tagged groups keep their own identity untouched.)
            if created_fresh_group {
                if let Some(bubble) = self.managers.bubble.bubble_mut(group_id) {
                    bubble.exchange = source_exch.clone();
                    bubble.account_type = source_at.clone();
                }
            }

            // --- Shared group path (user-tagged groups or brand-new groups) ---

            // Assign the group's color to leaf_color_tags for UI display.
            let group_color = self.managers.bubble.bubble(group_id)
                .map(|g| g.color)
                .unwrap_or([0.5, 0.5, 0.5, 0.9]);
            for &leaf_id in &new_leaves {
                self.panel_app.leaf_color_tags.insert(leaf_id, group_color);
            }
            // Determine if this is a truly new group (just created, has no state yet).
            let is_new_group = self.managers.bubble.bubble(group_id)
                .is_none_or(|g| g.members.is_empty() && g.indicator_configs.is_empty() && g.primitives.is_empty());
            // Collect (leaf_id, chart_id) first to avoid mixed borrow when setting group_id.
            let leaf_chart_ids: Vec<(mlc_core::LeafId, mlc_core::ChartId)> = new_leaves
                .iter()
                .filter_map(|&leaf_id| {
                    self.panel_app.panel_grid.container_id_for_leaf(leaf_id)
                        .map(|cid| (leaf_id, cid))
                })
                .collect();
            for &(_, chart_id) in &leaf_chart_ids {
                let _ = self.managers.bubble.attach_chart_to_bubble(chart_id, group_id);
            }
            // Rebind every new window's DM to the group's shared style store.
            let style_arc = self.managers.bubble
                .bubble(group_id)
                .map(|g| g.last_used_style.clone());
            for &(leaf_id, _) in &leaf_chart_ids {
                if let Some(pid) = self.panel_app.panel_grid.container_for_leaf(leaf_id).map(|w| w.chart_panel_id) {
                    if let Some(ref arc) = style_arc {
                        if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                            panel.drawing_manager.bind_style_store(arc.clone());
                        }
                    }
                }
            }
            // BubbleStore reverse index updated by attach_chart_to_bubble loop above.

            // For new groups: snapshot source window's indicator ids and command_history
            // into its auto_bubble so desync can restore them.
            if is_new_group {
                if let Some(&(source_leaf, source_cid)) = leaf_chart_ids.first() {
                    let pre_tag_ids: Vec<u64> = self.indicator_manager
                        .instances_iter()
                        .filter(|i| i.window_id == Some(source_cid.0) && i.origin_id.is_none())
                        .map(|i| i.id)
                        .collect();

                    // Read auto_bubble_id before taking mutable window borrow.
                    let source_auto_bid = self.panel_app.panel_grid
                        .containers()
                        .get(&source_cid)
                        .map(|w| w.auto_bubble_id);

                    if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(source_leaf) {
                        // Stash command_history into auto_bubble; window gets a fresh one.
                        let old_history = std::mem::replace(
                            &mut window.command_history,
                            mlc_core::CommandHistory::new(250),
                        );
                        if let Some(bid) = source_auto_bid {
                            if let Some(auto_bub) = self.managers.bubble.bubble_mut(bid) {
                                auto_bub.command_history = old_history;
                                auto_bub.pre_tag_indicator_ids = pre_tag_ids;
                            }
                        }
                        // Update current_bubble_id to the new group.
                        window.current_bubble_id = group_id;
                    }
                    // auto_bubble.primitives already holds the source's home drawings —
                    // no explicit stash needed.
                }
            }
            eprintln!(
                "[BubbleStore] {} {:?} with {} members after split",
                if is_new_group { "Created group" } else { "Joined existing group" },
                group_id,
                new_leaves.len()
            );

            // Clear legacy-cloned primitives from non-source windows.
            // clone_for_split_shell copies no primitives in S2.8, but earlier paths
            // may have left some. For grouped windows the pre-render sync fills them
            // from the group each frame.
            for &(leaf_id, _) in leaf_chart_ids.iter().skip(1) {
                if let Some(pid) = self.panel_app.panel_grid.container_for_leaf(leaf_id).map(|w| w.chart_panel_id) {
                    if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                        panel.drawing_manager.clear_all_primitives();
                    }
                }
            }

            // For EXISTING group: primitives are synced per-frame via pre-render,
            // but indicators need to be created on new windows NOW.
            if !is_new_group {
                self.sync_group_indicators_to_new_members(group_id, &leaf_chart_ids);
            }

            if is_new_group {
                // Seed the tag with the source window's current state.
                // Everything in the window at tag creation time becomes tag state.

                // Primitives: clone source originals into the group.
                // B1 S2.7.1d: read prim key from branch_meta (first_leaf_chart_id already computed above).
                let (prim_sym, prim_exch, prim_at) = {
                    let cid = first_leaf_chart_id;
                    let sym = cid.and_then(|c| self.managers.bubble.symbol_for_chart(c))
                        .map(String::from)
                        .unwrap_or_default();
                    let exch = cid.and_then(|c| self.managers.bubble.exchange_for_chart(c))
                        .map(String::from)
                        .unwrap_or_default();
                    let at = cid.and_then(|c| self.managers.bubble.account_type_for_chart(c))
                        .map(String::from)
                        .unwrap_or_else(|| "S".to_string());
                    (sym, exch, at)
                };
                let mut source_prims: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>> =
                    self.panel_app.panel_grid
                        .container_for_leaf(new_leaves[0])
                        .and_then(|w| self.panels_store.chart_panel.get(&w.chart_panel_id))
                        .map(|panel| {
                            panel.drawing_manager.primitives().iter()
                                .filter(|p| p.data().origin_id.is_none())
                                .map(|p| p.clone_box())
                                .collect()
                        })
                        .unwrap_or_default();
                for p in &mut source_prims {
                    let d = p.data_mut();
                    d.symbol = prim_sym.clone();
                    d.exchange = prim_exch.clone();
                    d.account_type = prim_at.clone();
                }
                // Re-ID the group copies so they are globally unique.
                // The source window's drawing_manager keeps the original IDs.
                for p in &mut source_prims {
                    p.data_mut().id = mlc_core::drawing::alloc_primitive_id();
                }
                if !source_prims.is_empty() {
                    if let Some(group) = self.managers.bubble.bubble_mut(group_id) {
                        group.primitives = source_prims;
                        eprintln!(
                            "[BubbleStore] Seeded group {:?} with {} primitives",
                            group_id, group.primitives.len()
                        );
                    }
                }

                // Indicators: snapshot source window's indicators into group configs.
                if let Some(source_chart_id) = leaf_chart_ids.first().map(|(_, cid)| *cid) {
                    // B1 S2.7.1d: read indicator seed key from branch_meta.
                    let (source_symbol, source_exchange, source_account_type) = {
                        let cid = first_leaf_chart_id;
                        let sym = cid.and_then(|c| self.managers.bubble.symbol_for_chart(c))
                            .map(String::from)
                            .unwrap_or_default();
                        let exch = cid.and_then(|c| self.managers.bubble.exchange_for_chart(c))
                            .map(String::from)
                            .unwrap_or_default();
                        let at = cid.and_then(|c| self.managers.bubble.account_type_for_chart(c))
                            .map(String::from)
                            .unwrap_or_else(|| "S".to_string());
                        (sym, exch, at)
                    };
                    let mut configs: Vec<mlc_core::tag_manager::IndicatorGroupConfig> = self
                        .indicator_manager
                        .instances_iter()
                        .filter(|i| {
                            i.origin_id.is_none() && i.window_id == Some(source_chart_id.0)
                        })
                        .map(|i| mlc_core::tag_manager::IndicatorGroupConfig {
                            id: i.id,
                            type_id: i.type_id.clone(),
                            name: i.name.clone(),
                            params: std::collections::HashMap::new(),
                            pane: i.pane as u32,
                            visible: i.visible,
                            symbol: source_symbol.clone(),
                            exchange: source_exchange.clone(),
                            account_type: source_account_type.clone(),
                        })
                        .collect();
                    // Sort by id to preserve original creation order (HashMap iteration is random).
                    configs.sort_by_key(|c| c.id);
                    if let Some(group) = self.managers.bubble.bubble_mut(group_id) {
                        let count = configs.len();
                        group.indicator_configs = configs;
                        eprintln!(
                            "[BubbleStore] Seeded group {:?} with {} indicator configs",
                            group_id, count
                        );
                    }
                }
            }

            // For non-source leaves (split children): create indicator instances
            // from the tag. These are empty windows that only see tag state.
            {
                let non_source: Vec<(mlc_core::LeafId, mlc_core::ChartId)> =
                    leaf_chart_ids.iter().skip(1).copied().collect();
                if !non_source.is_empty() {
                    self.sync_group_indicators_to_new_members(group_id, &non_source);
                }
            }

            // Reconcile sub_panes so split children don't show source's indicator panels.
            self.sync_sub_panes_from_manager();
        }
    }

    /// After a split, propagate the crosshair and viewport from the first leaf
    /// (which inherits the original active window) to all other new leaves in the
    /// sync group so the synced crosshair and viewport appear immediately.
    pub(super) fn propagate_crosshair_after_split(&mut self, new_leaves: &[mlc_core::LeafId]) {
        if new_leaves.len() < 2 {
            return;
        }
        let source_leaf = new_leaves[0];
        let source_bars: Vec<mlc_core::Bar> = self.bars_for_leaf(source_leaf);
        // Read crosshair + viewport state from source window.
        let (timestamp, price, visible, pane_index, view_start, bar_spacing) =
            match self.panel_app.panel_grid.container_for_leaf(source_leaf) {
                Some(w) => {
                    let bar_f64 = w.crosshair.bar_f64;
                    let timestamp = mlc_core::bar_f64_to_timestamp_ms(&source_bars[..], bar_f64);
                    (
                        timestamp,
                        w.crosshair.price,
                        w.crosshair.visible,
                        w.crosshair.pane_index,
                        w.viewport.view_start,
                        w.viewport.bar_spacing,
                    )
                }
                None => return,
            };
        // Propagate crosshair to all sync peers.
        self.propagate_crosshair_to_sync_group(source_leaf, timestamp, price, visible, pane_index);
        // Propagate viewport to all sync peers.
        self.propagate_viewport_to_sync_group(source_leaf, view_start, bar_spacing, None);
    }

    /// Propagate a symbol change to all leaves in the same sync color group.
    ///
    /// `source_leaf` is the leaf that already had its symbol changed.
    /// All other leaves sharing the same color tag get the same symbol, exchange,
    /// and account_type applied so the full instrument key is consistent.
    pub(super) fn propagate_symbol_to_sync_group(
        &mut self,
        source_leaf: mlc_core::LeafId,
        symbol: &str,
        exchange: &str,
        account_type: &str,
    ) {
        let should_sync = self.panel_app.panel_grid.container_id_for_leaf(source_leaf)
            .and_then(|cid| self.managers.bubble.bubble_for_chart(cid))
            .and_then(|gid| self.managers.bubble.bubble(gid))
            .map(|g| g.sync_flags.sync_symbol)
            .unwrap_or(true);
        if !should_sync { return; }

        // Enumerate peer leaves via TagManager group membership (not leaf_color_tags).
        // This makes propagation work for untagged (auto-created) groups too.
        let source_chart_id_for_peers = self.panel_app.panel_grid.container_id_for_leaf(source_leaf);
        let group_id_for_peers = source_chart_id_for_peers
            .and_then(|cid| self.managers.bubble.bubble_for_chart(cid));
        let sync_leaves: Vec<mlc_core::LeafId> = if let Some(gid) = group_id_for_peers {
            self.managers.bubble
                .chart_member_ids(gid)
                .into_iter()
                .filter_map(|cid| self.panel_app.panel_grid.leaf_for_container_id(cid))
                .filter(|&lid| lid != source_leaf)
                .collect()
        } else {
            // No group: fall back to color-tag matching for legacy ungrouped windows.
            match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
                Some(source_color) => self.panel_app.leaf_color_tags.iter()
                    .filter(|(&lid, &c)| lid != source_leaf && sync_colors_match(c, source_color))
                    .map(|(&lid, _)| lid)
                    .collect(),
                None => return,
            }
        };
        let symbol_owned = symbol.to_string();
        let exchange_owned = exchange.to_string();
        let account_type_owned = account_type.to_string();

        // Collect (old_symbol, old_exchange, old_account_type, timeframe) for each peer BEFORE
        // mutating windows so we can restore drawings and request bars after the mutations.
        // B1 S2.7.1d: read from branch_meta, fall back to struct field.
        let mut peer_info: Vec<(String, String, String, mlc_core::state::Timeframe)> = Vec::new();
        for leaf_id in &sync_leaves {
            let cid = self.panel_app.panel_grid.container_id_for_leaf(*leaf_id);
            let sym = cid.and_then(|c| self.managers.bubble.symbol_for_chart(c))
                .map(String::from)
                .unwrap_or_default();
            let exch = cid.and_then(|c| self.managers.bubble.exchange_for_chart(c))
                .map(String::from)
                .unwrap_or_default();
            let at = cid.and_then(|c| self.managers.bubble.account_type_for_chart(c))
                .map(String::from)
                .unwrap_or_else(|| "S".to_string());
            let tf = cid.and_then(|c| self.managers.bubble.timeframe_for_chart(c))
                .cloned()
                .unwrap_or_else(mlc_core::state::Timeframe::h1);
            peer_info.push((sym, exch, at, tf));
        }

        // Phase 4 S2.2: pre-collect (LeafId, ChartId) so we can update
        // `branch_meta` after the panel_grid mutating borrow drops. Read-only
        // borrow of `panel_grid` is dropped here before the mutating loop.
        let peer_chart_ids: Vec<(mlc_core::LeafId, Option<mlc_core::state::ChartId>)> =
            sync_leaves
                .iter()
                .map(|lid| (*lid, self.panel_app.panel_grid.container_id_for_leaf(*lid)))
                .collect();

        // Apply the full instrument key change (symbol + exchange + account_type) to each peer
        // window directly, bypassing change_symbol() which uses NullDataProvider and always fails.
        for (leaf_id, (old_symbol, old_exchange, old_account_type, _)) in sync_leaves.iter().zip(peer_info.iter()) {
            let pid_opt = self.panel_app.panel_grid.container_for_leaf(*leaf_id).map(|w| w.chart_panel_id);
            if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(*leaf_id) {
                if let Some(pid) = pid_opt {
                    if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                        window.snapshot_drawings_for_symbol(old_symbol, old_exchange, old_account_type, &panel.drawing_manager);
                        panel.drawing_manager.set_current_symbol_key(&symbol_owned, &exchange_owned, &account_type_owned);
                        panel.drawing_manager.clear_all_primitives();
                        window.restore_drawings_for_symbol(&symbol_owned, &exchange_owned, &account_type_owned, &mut panel.drawing_manager);
                    }
                }
                {};
                window.viewport.bar_count = 0;
                window.viewport.view_start = 0.0;
                window.pending_symbol_load = true;
            }
        }

        for ((_, chart_id_opt), (_, _, _, peer_tf)) in peer_chart_ids.iter().zip(peer_info.iter()) {
            if let Some(chart_id) = chart_id_opt {
                if let Some(bid) = self.managers.bubble.bubble_for_chart(*chart_id) {
                    if let Some(bubble) = self.managers.bubble.bubble_mut(bid) {
                        bubble.symbol = symbol_owned.clone();
                        bubble.timeframe = peer_tf.clone();
                        bubble.exchange = exchange_owned.clone();
                        bubble.account_type = account_type_owned.clone();
                    }
                }
            }
        }

        // Request fresh bars for each peer via the bridge using the new exchange + account_type.
        let bar_count = self.panel_app.user_manager.profile.bar_count as usize;
        for (_, _, _, timeframe) in peer_info {
            if symbol_owned.is_empty() {
                continue;
            }
            let resolved_exchange = self.core.exchange_symbols_read()
                .keys()
                .find(|eid| eid.as_str() == exchange_owned)
                .copied()
                .unwrap_or(self.active_exchange);
            let eid_str = resolved_exchange.as_str();
            if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                eprintln!(
                    "[ChartApp] Exchange {} is disabled, skipping request_bars (symbol propagation)",
                    eid_str
                );
                continue;
            }
            let at = crate::account_type_from_label(&account_type_owned);
            self.core.bridge.ensure_connector(resolved_exchange);
            self.core.bridge.request_bars(resolved_exchange, &symbol_owned, &timeframe, at, None, Some(bar_count), false);
            eprintln!(
                "[BubbleStore] Requested bars for peer {} @ {} tf={:?} (symbol propagation)",
                symbol_owned, eid_str, timeframe
            );
        }

        // Also update the TagManager group's canonical instrument so the group
        // state stays consistent with the displayed windows.
        if let Some(chart_id) = self.panel_app.panel_grid.container_id_for_leaf(source_leaf) {
            let source_exchange = self.managers.bubble.exchange_for_chart(chart_id)
                .map(String::from)
                .unwrap_or_default();
            let source_account_type = self.managers.bubble.account_type_for_chart(chart_id)
                .map(String::from)
                .unwrap_or_else(|| "S".to_string());

            let group_id = self.panel_app.panel_grid.container_id_for_leaf(source_leaf)
                .and_then(|cid| self.managers.bubble.bubble_for_chart(cid));

            let member = mlc_core::tag_manager::SyncMemberId::Chart(chart_id.0);
            self.managers.bubble.set_instrument(
                member,
                symbol.to_string(),
                source_exchange,
                source_account_type,
            );
            eprintln!(
                "[BubbleStore] Updated group symbol to '{}' via chart {:?}",
                symbol, chart_id
            );

            // Propagate the new key to all panel members of the same group.
            if let Some(gid) = group_id {
                self.apply_key_to_panels_in_group(gid);
            }
        }
    }

    /// Propagate a timeframe change to all leaves in the same sync color group.
    ///
    /// `source_leaf` is the leaf that already had its timeframe changed.
    pub(super) fn propagate_timeframe_to_sync_group(
        &mut self,
        source_leaf: mlc_core::LeafId,
        tf: mlc_core::state::Timeframe,
    ) {
        let should_sync = self.panel_app.panel_grid.container_id_for_leaf(source_leaf)
            .and_then(|cid| self.managers.bubble.bubble_for_chart(cid))
            .and_then(|gid| self.managers.bubble.bubble(gid))
            .map(|g| g.sync_flags.sync_timeframe)
            .unwrap_or(true);
        if !should_sync { return; }

        // Enumerate peer leaves via TagManager group membership (not leaf_color_tags).
        let source_chart_id_for_tf = self.panel_app.panel_grid.container_id_for_leaf(source_leaf);
        let group_id_for_tf = source_chart_id_for_tf
            .and_then(|cid| self.managers.bubble.bubble_for_chart(cid));
        let sync_leaves: Vec<mlc_core::LeafId> = if let Some(gid) = group_id_for_tf {
            self.managers.bubble
                .chart_member_ids(gid)
                .into_iter()
                .filter_map(|cid| self.panel_app.panel_grid.leaf_for_container_id(cid))
                .filter(|&lid| lid != source_leaf)
                .collect()
        } else {
            match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
                Some(source_color) => self.panel_app.leaf_color_tags.iter()
                    .filter(|(&lid, &c)| lid != source_leaf && sync_colors_match(c, source_color))
                    .map(|(&lid, _)| lid)
                    .collect(),
                None => return,
            }
        };

        // Collect (exchange_string, symbol, account_type) for each peer BEFORE mutating windows,
        // so we can call bridge.request_bars() after setting the new timeframe.
        // B1 S2.7.1d: read from branch_meta, fall back to struct field.
        let mut peer_requests: Vec<(String, String, String)> = Vec::new();
        for leaf_id in &sync_leaves {
            let cid = self.panel_app.panel_grid.container_id_for_leaf(*leaf_id);
            let exch = cid.and_then(|c| self.managers.bubble.exchange_for_chart(c))
                .map(String::from)
                .unwrap_or_default();
            let sym = cid.and_then(|c| self.managers.bubble.symbol_for_chart(c))
                .map(String::from)
                .unwrap_or_default();
            let at = cid.and_then(|c| self.managers.bubble.account_type_for_chart(c))
                .map(String::from)
                .unwrap_or_else(|| "S".to_string());
            peer_requests.push((exch, sym, at));
        }

        // Phase 4 S2.2: pre-collect (LeafId, ChartId) for branch_meta dual-write
        // after the mutating loop drops the panel_grid borrow.
        let peer_chart_ids: Vec<(mlc_core::LeafId, Option<mlc_core::state::ChartId>)> =
            sync_leaves
                .iter()
                .map(|lid| (*lid, self.panel_app.panel_grid.container_id_for_leaf(*lid)))
                .collect();

        // Set timeframe and clear stale bars on each peer window directly.
        // The timeframe is now owned by branch_meta; the mirror happens below
        // in the dual-write phase after the panel_grid mut borrow drops.
        for leaf_id in &sync_leaves {
            if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(*leaf_id) {
                {};
                window.viewport.bar_count = 0;
                window.viewport.view_start = 0.0;
            }
        }

        for (_, chart_id_opt) in &peer_chart_ids {
            if let Some(chart_id) = chart_id_opt {
                if let Some(bid) = self.managers.bubble.bubble_for_chart(*chart_id) {
                    if let Some(bubble) = self.managers.bubble.bubble_mut(bid) {
                        bubble.timeframe = tf.clone();
                    }
                }
            }
        }

        // Request fresh bars for each peer via the bridge.
        let bar_count = self.panel_app.user_manager.profile.bar_count as usize;
        for (exchange_str, symbol, at_label) in peer_requests {
            if symbol.is_empty() {
                continue;
            }
            let resolved_exchange = self.core.exchange_symbols_read()
                .keys()
                .find(|eid| eid.as_str() == exchange_str)
                .copied()
                .unwrap_or(self.active_exchange);
            let eid_str = resolved_exchange.as_str();
            if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                eprintln!(
                    "[ChartApp] Exchange {} is disabled, skipping request_bars (timeframe propagation)",
                    eid_str
                );
                continue;
            }
            let at = crate::account_type_from_label(&at_label);
            self.core.bridge.ensure_connector(resolved_exchange);
            if let Some(count) = tf.tick_count() {
                // Tick TF peer: subscribe to the dig3 TickBar stream.
                self.core.bridge.request_tick_bars(resolved_exchange, &symbol, at, count);
            } else {
                self.core.bridge.abort_tick_bars(resolved_exchange, &symbol, at);
                self.core.bridge.request_bars(resolved_exchange, &symbol, &tf, at, None, Some(bar_count), true);
            }
            eprintln!(
                "[BubbleStore] Requested bars for peer {} @ {} tf={:?} (timeframe propagation)",
                symbol, eid_str, tf
            );
        }

        // Also update the TagManager group's canonical timeframe so the group
        // state stays consistent with the displayed windows.
        if let Some(chart_id) = self.panel_app.panel_grid.container_id_for_leaf(source_leaf) {
            self.managers.bubble.set_timeframe(chart_id, tf.clone());
            eprintln!(
                "[BubbleStore] Updated group timeframe to {:?} via chart {:?}",
                tf, chart_id
            );
        }
    }

    /// DEPRECATED: Legacy clone-based primitive propagation for non-grouped windows.
    /// For grouped windows (TagManager), primitives are shared via pre-render sync
    /// from the group's primitive list. This function is skipped when `group_id.is_some()`.
    /// Will be removed once all windows are guaranteed to use TagManager groups.
    pub(super) fn propagate_new_primitive_to_sync_group(
        &mut self,
        source_leaf: mlc_core::LeafId,
    ) {
        // Determine the source window's color tag.
        let source_color = match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
            Some(c) => c,
            None => return,
        };

        // Collect peer leaf IDs that share the same color tag.
        let peer_leaves: Vec<mlc_core::LeafId> = self.panel_app.leaf_color_tags.iter()
            .filter(|(&lid, &c)| lid != source_leaf && sync_colors_match(c, source_color))
            .map(|(&lid, _)| lid)
            .collect();

        if peer_leaves.is_empty() {
            return;
        }

        // Get the chart ID for the source leaf, then read the new primitive's ID.
        let source_chart_id = match self.panel_app.panel_grid.container_id_for_leaf(source_leaf) {
            Some(id) => id,
            None => return,
        };

        // If the source window is in a TagManager group, primitives are shared via the
        // group — no clone-based propagation is needed.
        if self.managers.bubble.bubble_for_chart(source_chart_id).is_some() {
            return;
        }

        let src_pid = self.panel_app.panel_grid.containers().get(&source_chart_id).map(|w| w.chart_panel_id);
        let prim_id = match src_pid.and_then(|pid| self.panels_store.chart_panel.get(&pid))
            .and_then(|panel| panel.drawing_manager.last_original_id())
        {
            Some(id) => id,
            None => return,
        };

        // For each peer leaf, clone the primitive into its drawing manager.
        for peer_leaf in peer_leaves {
            let peer_chart_id = match self.panel_app.panel_grid.container_id_for_leaf(peer_leaf) {
                Some(id) => id,
                None => continue,
            };

            // Clone the primitive from the source window.
            let cloned = match src_pid.and_then(|pid| self.panels_store.chart_panel.get(&pid))
                .and_then(|panel| panel.drawing_manager.clone_primitive_for_sync(prim_id, peer_chart_id.0))
            {
                Some(c) => c,
                None => continue,
            };

            // Insert the clone into the peer window.
            if let Some(peer_pid) = self.panel_app.panel_grid.containers().get(&peer_chart_id).map(|w| w.chart_panel_id) {
                if let Some(peer_panel) = self.panels_store.chart_panel.get_mut(&peer_pid) {
                    peer_panel.drawing_manager.add_synced_primitives(vec![cloned]);
                    eprintln!("[ChartApp] Synced primitive {} to peer chart {:?}", prim_id, peer_chart_id);
                }
            }
        }
    }

    /// Propagate crosshair position to all leaves in the same sync color group.
    ///
    /// `source_leaf` is the leaf whose crosshair was just updated.
    /// All other leaves sharing the same color tag receive a matching position
    /// via [`ChartContainer::set_crosshair_from_timestamp`], which converts the
    /// universal timestamp to each peer's local fractional bar index.
    pub(super) fn propagate_crosshair_to_sync_group(
        &mut self,
        source_leaf: mlc_core::LeafId,
        timestamp: i64,
        price: f64,
        visible: bool,
        pane_index: Option<usize>,
    ) {
        let source_chart_id = self.panel_app.panel_grid.container_id_for_leaf(source_leaf);
        let group_id = source_chart_id
            .and_then(|cid| self.managers.bubble.bubble_for_chart(cid));

        let should_sync = group_id
            .and_then(|gid| self.managers.bubble.bubble(gid))
            .map(|g| g.sync_flags.sync_crosshair)
            .unwrap_or(true);
        if !should_sync { return; }

        // Propagate crosshair to peer chart windows in the same color-tag group.
        // Per-member override (`effective_sync_crosshair`) takes precedence over
        // the group-level flag — a peer with `sync_crosshair = Some(false)` must
        // not receive updates even when the group default is enabled.
        // Use TagManager membership so untagged groups propagate too.
        let sync_leaves: Vec<mlc_core::LeafId> = if let Some(gid) = group_id {
            self.managers.bubble
                .chart_member_ids(gid)
                .into_iter()
                .filter_map(|cid| self.panel_app.panel_grid.leaf_for_container_id(cid))
                .filter(|&lid| lid != source_leaf)
                .collect::<Vec<_>>()
                .into_iter()
                .filter(|&lid| {
                    let peer_chart_id = self.panel_app.panel_grid.container_id_for_leaf(lid);
                    let peer_should_sync = group_id
                        .zip(peer_chart_id)
                        .and_then(|(gid2, cid)| {
                            self.managers.bubble.bubble(gid2).map(|g| {
                                g.effective_sync_crosshair(
                                    mlc_core::tag_manager::SyncMemberId::Chart(cid.0),
                                )
                            })
                        })
                        .unwrap_or(true);
                    peer_should_sync
                })
                .collect()
        } else {
            match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
                Some(source_color) => self.panel_app.leaf_color_tags.iter()
                    .filter(|(&lid, &c)| lid != source_leaf && sync_colors_match(c, source_color))
                    .map(|(&lid, _)| lid)
                    .collect(),
                None => Vec::new(),
            }
        };
        for leaf_id in sync_leaves {
            let leaf_bars = self.bars_for_leaf(leaf_id);
            if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                window.set_crosshair_from_timestamp(timestamp, price, visible, pane_index, &leaf_bars);
            }
        }

        // Propagate crosshair price to all order-flow panels that belong to the same sync group.
        if let Some(gid) = group_id {
            let panel_ids = self.managers.bubble.panel_members(gid);
            let crosshair_price = if visible { Some(price) } else { None };
            for pid in panel_ids {
                let panel_id = mlc_sidebar_content::free_slot::PanelId(pid);
                if let Some(s) = self.panels_store.dom.get_mut(&panel_id) {
                    s.crosshair_price = crosshair_price;
                }
                if let Some(s) = self.panels_store.liquidity_heatmap.get_mut(&panel_id) {
                    s.crosshair_price = crosshair_price;
                }
                if let Some(s) = self.panels_store.l2_tape.get_mut(&panel_id) {
                    s.crosshair_price = crosshair_price;
                }
                if let Some(s) = self.panels_store.footprint.get_mut(&panel_id) {
                    s.crosshair_price = crosshair_price;
                }
                if let Some(s) = self.panels_store.big_trades.get_mut(&panel_id) {
                    s.crosshair_price = crosshair_price;
                }
                if let Some(s) = self.panels_store.volume_profile.get_mut(&panel_id) {
                    s.crosshair_price = crosshair_price;
                }
                if let Some(s) = self.panels_store.trade_tape.get_mut(&panel_id) {
                    s.crosshair_price = crosshair_price;
                }
            }
        }
    }

    /// DEPRECATED: Legacy drawing state propagation for non-grouped windows.
    /// For grouped windows, drawing state sync should go through TagManager.
    /// Will be removed once all windows use TagManager groups.
    pub(super) fn propagate_drawing_state_to_sync_group(
        &mut self,
        source_leaf: mlc_core::LeafId,
    ) {
        // Resolve the source window's TagManager group.
        // Bug F5 fix: fan-out is gated on actual TagManager group membership, not
        // on the `leaf_color_tags` map which can include windows from different groups
        // that happen to share the same color value.
        let group_id = self.panel_app.panel_grid
            .container_id_for_leaf(source_leaf)
            .and_then(|cid| self.managers.bubble.bubble_for_chart(cid));

        let should_sync = group_id
            .and_then(|gid| self.managers.bubble.bubble(gid))
            .map(|g| g.sync_flags.sync_drawings)
            .unwrap_or(true);
        if !should_sync { return; }

        // Collect peer leaf IDs using the TagManager group boundary.
        let peer_leaves: Vec<mlc_core::LeafId> = if let Some(gid) = group_id {
            // Every chart member of the group except the source leaf itself.
            self.managers.bubble
                .chart_member_ids(gid)
                .into_iter()
                .filter_map(|cid| self.panel_app.panel_grid.leaf_for_container_id(cid))
                .filter(|&lid| lid != source_leaf)
                .collect()
        } else {
            // No group — fall back to color-tag matching for ungrouped legacy windows.
            let source_color = match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
                Some(c) => c,
                None => return,
            };
            self.panel_app.leaf_color_tags.iter()
                .filter(|(&lid, &c)| lid != source_leaf && sync_colors_match(c, source_color))
                .map(|(&lid, _)| lid)
                .collect()
        };

        if peer_leaves.is_empty() {
            return;
        }

        // Read the current drawing state and active pane from the source window.
        // Extracted before taking mutable borrows on peer windows.
        let src_pid_ds = self.panel_app.panel_grid.container_for_leaf(source_leaf).map(|w| w.chart_panel_id);
        let (tool_id, points, src_pane_id) = match src_pid_ds.and_then(|pid| self.panels_store.chart_panel.get(&pid)) {
            Some(panel) => {
                let pane = panel.drawing_manager.current_pane();
                match panel.drawing_manager.drawing_state() {
                    mlc_core::drawing::DrawingState::Creating { tool_id, points } => {
                        (Some(tool_id.clone()), points.clone(), pane)
                    }
                    mlc_core::drawing::DrawingState::Idle => (None, Vec::new(), pane),
                }
            }
            None => return,
        };

        // Build a reverse map: source instance_id -> (type_id, ordinal).
        // This is the same remap strategy used by prepare.rs for completed primitives.
        // We build it once from the source chart and apply per-peer.
        let source_cid = self.panel_app.panel_grid.container_id_for_leaf(source_leaf);
        let src_pane_type_ord: Option<(String, usize)> = src_pane_id.and_then(|pid| {
            let cid = source_cid?;
            let mut sub_pane_insts: Vec<(u64, String, usize, u64)> = self.indicator_manager
                .get_instances_for_window(cid.0)
                .into_iter()
                .filter(|i| i.pane > 0)
                .map(|i| (i.id, i.type_id.clone(), i.pane, i.id))
                .collect();
            sub_pane_insts.sort_by(|a, b| a.2.cmp(&b.2).then(a.3.cmp(&b.3)));
            let mut type_count: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for (iid, type_id, _pane, _) in &sub_pane_insts {
                let ord = type_count.entry(type_id.clone()).or_insert(0);
                if *iid == pid {
                    return Some((type_id.clone(), *ord));
                }
                *ord += 1;
            }
            None
        });

        // Apply to every peer leaf.
        for peer_leaf in &peer_leaves {
            let peer_cid = self.panel_app.panel_grid.container_id_for_leaf(*peer_leaf);

            // Remap source pane_id to the peer's corresponding instance_id.
            let peer_pane_id: Option<u64> = src_pane_type_ord.as_ref().and_then(|(type_id, ord)| {
                let cid = peer_cid?;
                let mut sub_pane_insts: Vec<(u64, String, usize, u64)> = self.indicator_manager
                    .get_instances_for_window(cid.0)
                    .into_iter()
                    .filter(|i| i.pane > 0)
                    .map(|i| (i.id, i.type_id.clone(), i.pane, i.id))
                    .collect();
                sub_pane_insts.sort_by(|a, b| a.2.cmp(&b.2).then(a.3.cmp(&b.3)));
                let mut type_count: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
                for (iid, t, _pane, _) in &sub_pane_insts {
                    let o = type_count.entry(t.clone()).or_insert(0);
                    if t == type_id && *o == *ord {
                        return Some(*iid);
                    }
                    *o += 1;
                }
                None
            });

            if let Some(peer_pid) = self.panel_app.panel_grid.container_for_leaf(*peer_leaf).map(|w| w.chart_panel_id) {
                if let Some(peer_panel) = self.panels_store.chart_panel.get_mut(&peer_pid) {
                    peer_panel.drawing_manager.set_synced_drawing_state(tool_id.clone(), points.clone());
                    // Sync the pane context so the preview renders in the correct sub-pane
                    // on the peer (not on the main chart). When the source drawing is on
                    // the main chart (src_pane_id = None), clear any stale pane context.
                    if src_pane_id.is_some() {
                        // peer_pane_id is None when the peer lacks the indicator — the
                        // preview then falls back to the main chart, which is acceptable.
                        peer_panel.drawing_manager.set_current_pane(peer_pane_id);
                    } else {
                        peer_panel.drawing_manager.set_current_pane(None);
                    }
                }
            }
        }
    }

    /// If the active window belongs to a TagManager group, move the most recently
    /// completed primitive out of its `drawing_manager` and into the group's
    /// authoritative primitive list.
    ///
    /// This is called immediately after any drawing-completion event (DrawingClick,
    /// freehand drag-end, finish_multipoint) on a grouped window.  The per-frame
    /// render-cache sync then distributes the primitive to all member windows.
    ///
    /// Returns `true` when a primitive was transferred to the group.
    pub(super) fn intercept_completed_primitive_to_group(&mut self) -> bool {
        // Check if the active window is in a TagManager group.
        let chart_id = match self.panel_app.panel_grid.active_container_id() {
            Some(id) => id,
            None => return false,
        };
        let group_id = match self.managers.bubble.bubble_for_chart(chart_id) {
            Some(gid) => gid,
            None => return false,
        };

        // If sync_drawings is OFF for this group, leave the primitive in the
        // drawing_manager for this window only — forward sync is blocked anyway,
        // so moving it to the group would cause it to disappear visually.
        let (sync_drawings_on, is_auto_mono) = self.managers.bubble
            .bubble(group_id)
            .map(|g| (g.sync_flags.sync_drawings, g.auto_created && g.members.len() <= 1))
            .unwrap_or((true, false));
        if !sync_drawings_on {
            return false;
        }
        // Symmetric auto-mono guard: forward sync skips auto-created solo groups to
        // avoid wiping their locally-managed drawing_manager primitives. Intercept
        // mirrors this so primitives aren't moved to a group that never syncs back.
        // Non-auto tagged groups with a single member DO use the group path — their
        // primitives are stored in group.primitives and synced via prepare_frame.
        if is_auto_mono {
            return false;
        }

        // Pop the last primitive from the window's drawing_manager.
        let active_pid_intercept = self.panel_app.panel_grid.containers().get(&chart_id).map(|w| w.chart_panel_id);
        let prim = {
            let panel = match active_pid_intercept.and_then(|pid| self.panels_store.chart_panel.get_mut(&pid)) {
                Some(p) => p,
                None => return false,
            };
            let idx = match panel.drawing_manager.last_index() {
                Some(i) => i,
                None => return false,
            };
            match panel.drawing_manager.remove(idx) {
                Some(p) => p,
                None => return false,
            }
        };

        // Stamp the full instrument key (symbol, exchange, account_type) on the primitive
        // so per-instrument tracking works correctly across multi-exchange groups.
        let mut prim = prim;
        let (prim_symbol, prim_exchange, prim_account_type) = {
            let sym = self.managers.bubble.symbol_for_chart(chart_id)
                .map(String::from)
                .unwrap_or_default();
            let exch = self.managers.bubble.exchange_for_chart(chart_id)
                .map(String::from)
                .unwrap_or_default();
            let at = self.managers.bubble.account_type_for_chart(chart_id)
                .map(String::from)
                .unwrap_or_else(|| "S".to_string());
            (sym, exch, at)
        };
        {
            let d = prim.data_mut();
            d.symbol = prim_symbol;
            d.exchange = prim_exchange;
            d.account_type = prim_account_type;
        }

        // Add the primitive to the TagManager group so all members share it.
        if let Some(group) = self.managers.bubble.bubble_mut(group_id) {
            group.primitives.push(prim);
            eprintln!(
                "[BubbleStore] Moved completed primitive into group {:?} (now {} primitives)",
                group_id,
                group.primitives.len()
            );
            true
        } else {
            // Group disappeared — put the primitive back into drawing_manager.
            if let Some(pid) = active_pid_intercept {
                if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                    panel.drawing_manager.add_synced_primitives(vec![prim]);
                }
            }
            false
        }
    }

    /// Disconnect the window identified by `leaf_id` from its current group, and
    /// remove that group if it is now empty and was auto-created.
    ///
    /// This is a low-level helper used before joining a new color tag or before
    /// creating a new auto group. It does NOT purge cloned primitives/indicators —
    /// for a full desync use `perform_desync` instead.
    pub(super) fn disconnect_from_current_group(&mut self, leaf_id: mlc_core::LeafId) {
        let chart_id = match self.panel_app.panel_grid.container_id_for_leaf(leaf_id) {
            Some(id) => id,
            None => return,
        };
        let old_gid = match self.managers.bubble.bubble_for_chart(chart_id) {
            Some(gid) => gid,
            None => return,
        };
        let _ = self.managers.bubble.detach_chart_from_bubble(chart_id);
        // Remove the old group if it is now empty and was auto-created.
        if let Some(g) = self.managers.bubble.bubble(old_gid) {
            if g.auto_created && g.members.is_empty() {
                self.managers.bubble.remove_group(old_gid);
                eprintln!("[BubbleStore] Removed empty auto group {:?}", old_gid);
            }
        }
    }

    /// Write the active window's drawing_manager primitives back to the group.
    /// Call this after ANY mutation (delete, color change, style change, etc.)
    /// on drawing_manager for a grouped window. The per-frame forward sync
    /// (group → drawing_manager) will then distribute the change to all members.
    /// Sync the drawing_manager primitives from the window identified by
    /// `leaf_id` back into its sync group.  Callers that mutate a specific
    /// non-active window should use this variant so the correct window is read.
    pub(super) fn sync_drawing_back_to_group_for(&mut self, leaf_id: mlc_core::LeafId) {
        let chart_id = match self.panel_app.panel_grid.container_id_for_leaf(leaf_id) {
            Some(cid) => cid,
            None => return,
        };
        let group_id = match self.managers.bubble.bubble_for_chart(chart_id) {
            Some(gid) => gid,
            None => return,
        };
        // Respect the sync_drawings flag — if disabled, do not write back to group.
        if let Some(group) = self.managers.bubble.bubble(group_id) {
            if !group.sync_flags.sync_drawings {
                return;
            }
        }
        let new_prims: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>> =
            self.panel_app.panel_grid.containers().get(&chart_id)
                .and_then(|w| self.panels_store.chart_panel.get(&w.chart_panel_id))
                .map(|panel| panel.drawing_manager.primitives().iter().map(|p| p.clone_box()).collect())
                .unwrap_or_default();
        if let Some(group) = self.managers.bubble.bubble_mut(group_id) {
            group.primitives = new_prims;
        }
        self.sidebar_data_dirty = true;
    }

    /// Convenience wrapper: syncs the active window's drawing_manager back to
    /// its group.  All existing call sites that mutate the active window use
    /// this; callers that mutate a non-active window should use
    /// `sync_drawing_back_to_group_for` with an explicit leaf_id instead.
    pub(super) fn sync_drawing_back_to_group(&mut self) {
        // Resolve the active leaf via the docking manager, then delegate.
        if let Some(leaf_id) = self.panel_app.panel_grid.docking().active_leaf() {
            self.sync_drawing_back_to_group_for(leaf_id);
        }
    }

    /// After split joins an existing group, create indicator instances on the
    /// new windows for each indicator config already in the group.
    pub(super) fn sync_group_indicators_to_new_members(
        &mut self,
        group_id: mlc_core::tag_manager::BubbleId,
        new_leaf_chart_ids: &[(mlc_core::LeafId, mlc_core::ChartId)],
    ) {
        // Respect the sync_indicators flag — if disabled, skip indicator sync.
        if let Some(group) = self.managers.bubble.bubble(group_id) {
            if !group.sync_flags.sync_indicators {
                return;
            }
        }

        // Collect indicator configs from the group.
        let configs: Vec<(String, String)> = self.managers.bubble
            .bubble(group_id)
            .map(|g| {
                g.indicator_configs.iter()
                    .map(|c| (c.type_id.clone(), c.name.clone()))
                    .collect()
            })
            .unwrap_or_default();

        if configs.is_empty() {
            return;
        }

        for &(leaf_id, chart_id) in new_leaf_chart_ids {
            for (type_id, name) in &configs {
                // Check if this window already has this indicator type.
                let already_has = self.indicator_manager.instances_iter()
                    .any(|i| i.window_id == Some(chart_id.0) && i.type_id == *type_id);
                if already_has {
                    continue;
                }

                if let Some(new_id) = self.indicator_manager.create_instance(type_id) {
                    self.indicator_manager.assign_window(new_id, Some(chart_id.0));
                    eprintln!(
                        "[BubbleStore] Split sync: created indicator '{}' (id={}) for chart {:?}",
                        name, new_id, chart_id
                    );
                }
            }

            // Calculate indicators with this window's bars.
            let bars: Option<Vec<mlc_core::Bar>> = self.panel_app.panel_grid
                .container_for_leaf(leaf_id)
                .map(|w| Vec::<mlc_core::Bar>::new());
            if let Some(bars) = bars {
                self.indicator_manager.calculate_all_for_window(chart_id.0, &bars);
            }
        }
        self.sync_sub_panes_from_manager();
        eprintln!(
            "[BubbleStore] Synced {} indicator configs to {} new members",
            configs.len(), new_leaf_chart_ids.len()
        );
    }

    /// When an indicator is created on a grouped window, create matching instances
    /// on all peer windows in the same group.
    pub(super) fn sync_group_indicator_to_peers(
        &mut self,
        group_id: mlc_core::tag_manager::BubbleId,
        type_id: &str,
        source_leaf: mlc_core::LeafId,
    ) {
        // Respect the sync_indicators flag — if disabled, skip peer sync.
        if let Some(group) = self.managers.bubble.bubble(group_id) {
            if !group.sync_flags.sync_indicators {
                return;
            }
        }

        // Collect peer chart_ids (excluding source). Symbol no longer matters
        // — each peer recalculates indicators against its own bars.
        let source_chart_id = self.panel_app.panel_grid.container_id_for_leaf(source_leaf);
        let peer_cids: Vec<mlc_core::ChartId> = self.managers.bubble
            .chart_member_ids(group_id)
            .into_iter()
            .filter(|&cid| Some(cid) != source_chart_id)
            .collect();

        for peer_cid in peer_cids {
            if let Some(new_id) = self.indicator_manager.create_instance(type_id) {
                self.indicator_manager.assign_window(new_id, Some(peer_cid.0));
                if let Some(inst) = self.indicator_manager.get_instance_mut(new_id) {
                    inst.origin_id = None; // It's a group-managed indicator, not a clone.
                }
                // Calculate with peer's bars.
                let bars: Option<Vec<mlc_core::Bar>> = self.panel_app.panel_grid
                    .containers().get(&peer_cid)
                    .map(|w| Vec::<mlc_core::Bar>::new());
                if let Some(bars) = bars {
                    self.indicator_manager.calculate_all_for_window(peer_cid.0, &bars);
                }
                eprintln!(
                    "[BubbleStore] Synced indicator '{}' (id={}) to peer chart {:?}",
                    type_id, new_id, peer_cid
                );
            }
        }
        self.sync_sub_panes_from_manager();
    }
}
