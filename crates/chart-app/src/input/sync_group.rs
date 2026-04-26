//! Sync group propagation: replicate state changes (symbol, timeframe, drawings,
//! crosshair, indicators) across all charts in the same sync group.
//! Also handles split, desync, color-group join.

use crate::ChartApp;
use super::sync_colors_match;

impl ChartApp {
    /// Hide crosshairs on all split leaves that are NOT in the same sync group
    /// as `source_leaf`.  Sync-group peers are updated by
    /// `propagate_crosshair_to_sync_group`; this covers the non-peer leaves.
    pub(super) fn hide_crosshairs_outside_sync_group(&mut self, source_leaf: zengeld_chart::LeafId) {
        let source_color = self.panel_app.leaf_color_tags.get(&source_leaf).copied();
        let crosshair_sync_enabled = self.panel_app.panel_grid
            .chart_id_for_leaf(source_leaf)
            .and_then(|cid| self.panel_app.tag_manager.group_for_window(cid))
            .and_then(|gid| self.panel_app.tag_manager.group(gid))
            .map(|g| g.sync_flags.sync_crosshair)
            .unwrap_or(true);
        let all_ids: Vec<zengeld_chart::LeafId> = self.panel_app
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
                if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(other_id) {
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
        use zengeld_chart::tag_manager::SyncMemberId;
        let member = SyncMemberId::Panel(panel_id);
        self.panel_app.tag_manager.disconnect(member);
        let gid = self.panel_app.tag_manager.create_group_auto(
            [0.5, 0.5, 0.5, 1.0],
            String::new(),
            zengeld_chart::state::Timeframe::m1(),
        );
        self.panel_app.tag_manager.set_auto_group(member, gid);
        eprintln!("[ChartApp] Panel {} desynced → auto group {:?}", panel_id, gid);
        self.sidebar_data_dirty = true;
        self.autosave_snapshot();
    }

    pub(super) fn perform_desync(&mut self, leaf_id: zengeld_chart::LeafId) {
        // 1. Remove color tag.
        self.panel_app.leaf_color_tags.remove(&leaf_id);

        // Snapshot group info BEFORE disconnect.
        let had_group_id = self.panel_app.panel_grid
            .window_for_leaf(leaf_id)
            .and_then(|w| w.group_id)
            .is_some();
        // Pre-tag indicator ids — these survive desync (will be unhidden).
        let pre_tag_ids: Vec<u64> = self.panel_app.panel_grid
            .window_for_leaf(leaf_id)
            .map(|w| w.pre_tag_indicator_ids.clone())
            .unwrap_or_default();
        // Has stashed primitives? (window joined an existing tag)
        let has_stash = self.panel_app.panel_grid
            .window_for_leaf(leaf_id)
            .is_some_and(|w| !w.stashed_primitives.is_empty());

        // 1b. Disconnect from TagManager group.
        if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(leaf_id) {
            if let Some(old_group_id) = self.panel_app.tag_manager.disconnect_chart(chart_id) {
                eprintln!(
                    "[TagManager] Disconnected chart {:?} from group {:?}",
                    chart_id, old_group_id
                );
                let remaining = self.panel_app.tag_manager
                    .members(old_group_id)
                    .map_or(0, |m| m.len());
                eprintln!("[TagManager] Group {:?} has {} remaining members", old_group_id, remaining);
            }
        }

        // 2. Restore window's own state: primitives and indicators.
        if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(leaf_id) {
            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                if had_group_id {
                    // Clear tag primitives (pre-render sync fills these from group).
                    window.drawing_manager.clear_all_primitives();

                    // Restore stashed primitives (window's own, hidden on join).
                    if has_stash {
                        let restored: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                            std::mem::take(&mut window.stashed_primitives);
                        let count = restored.len();
                        window.drawing_manager.add_synced_primitives(restored);
                        eprintln!("[TagManager] Desync: restored {} stashed primitives", count);
                    } else {
                        eprintln!("[TagManager] Desync: no stashed primitives (split child or seed source)");
                    }
                } else {
                    window.drawing_manager.purge_synced_primitives();
                }
                window.group_id = None; // temporary, set properly below
                window.pre_tag_indicator_ids.clear();
                window.stashed_primitives.clear();
                // Restore the window-local command history stashed at join time.
                if let Some(stashed) = window.stashed_command_history.take() {
                    window.command_history = stashed;
                    eprintln!("[TagManager] Desync: restored stashed command history");
                }
            }

            // Remove tag indicators: all indicators for this window EXCEPT pre-tag ones.
            // Then unhide pre-tag indicators.
            if had_group_id {
                let to_remove: Vec<u64> = self.indicator_manager
                    .instances_iter()
                    .filter(|i| {
                        i.window_id == Some(chart_id.0)
                            && !pre_tag_ids.contains(&i.id)
                    })
                    .map(|i| i.id)
                    .collect();
                let count = to_remove.len();
                for id in &to_remove {
                    self.indicator_manager.remove_instance(*id);
                }
                // Unhide pre-tag indicators.
                for &id in &pre_tag_ids {
                    if let Some(inst) = self.indicator_manager.get_instance_mut(id) {
                        inst.visible = true;
                    }
                }
                eprintln!("[TagManager] Desync: removed {} tag indicators, unhid {} pre-tag",
                    count, pre_tag_ids.len());
            } else {
                self.indicator_manager.purge_synced_instances_for_window(chart_id.0);
            }
        }

        // 4. Create a new auto_created group for the desynced window.
        // Every window must always be in a group — standalone path is dead.
        if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(leaf_id) {
            let (symbol, timeframe) = self.panel_app.panel_grid
                .window_for_leaf(leaf_id)
                .map(|w| (w.symbol.clone(), w.timeframe.clone()))
                .unwrap_or_else(|| ("BTCUSDT".to_string(), zengeld_chart::state::Timeframe::h1()));
            // Auto groups get transparent color — never occupy palette slots
            let new_group_id = self.panel_app.tag_manager.create_group_auto([0.0, 0.0, 0.0, 0.0], symbol, timeframe);
            let _ = self.panel_app.tag_manager.connect_chart(chart_id, new_group_id);
            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                window.group_id = Some(new_group_id);
            }
            // Move restored primitives from drawing_manager into the new group.
            let dm_prims: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                self.panel_app.panel_grid.window_for_leaf(leaf_id)
                    .map(|w| w.drawing_manager.primitives().iter().map(|p| p.clone_box()).collect())
                    .unwrap_or_default();
            if !dm_prims.is_empty() {
                if let Some(g) = self.panel_app.tag_manager.group_mut(new_group_id) {
                    g.primitives = dm_prims;
                }
            }
            eprintln!("[TagManager] Desync: created new auto group {:?} for desynced window", new_group_id);
        }

        // 5. Recalculate sub-panes so indicator panels stay in sync.
        self.sync_sub_panes_from_manager();
        self.sidebar_data_dirty = true;

        eprintln!("[ChartApp] Desynced leaf {:?} from color tag group", leaf_id);
    }

    /// Join an existing color group: clone primitives and indicators from a peer
    /// that already belongs to this color group into the joining leaf.
    pub(super) fn sync_join_color_group(&mut self, joining_leaf: zengeld_chart::LeafId, color: [f32; 4]) {
        // Connect the joining leaf's chart to the TagManager group for this color.
        // Find-or-create the group so TagManager stays consistent regardless of
        // whether this is the first leaf in the group or a later joiner.
        if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(joining_leaf) {
            let existing_group = self.panel_app.tag_manager.find_group_by_color(color);
            let is_new_group = existing_group.is_none();
            let group_id = existing_group.unwrap_or_else(|| {
                let (symbol, tf) = self.panel_app.panel_grid
                    .window_for_leaf(joining_leaf)
                    .map(|w| (w.symbol.clone(), w.timeframe.clone()))
                    .unwrap_or_else(|| (
                        "BTCUSDT".to_string(),
                        zengeld_chart::state::Timeframe::h1(),
                    ));
                self.panel_app.tag_manager.create_group(color, symbol, tf)
            });
            let _ = self.panel_app.tag_manager.connect_chart(chart_id, group_id);
            eprintln!(
                "[TagManager] Connected chart {:?} to group {:?} (new={})",
                chart_id, group_id, is_new_group
            );

            // Snapshot pre-tag indicator ids (these survive desync).
            let pre_tag_ids: Vec<u64> = self.indicator_manager
                .instances_iter()
                .filter(|i| i.window_id == Some(chart_id.0) && i.origin_id.is_none())
                .map(|i| i.id)
                .collect();

            if is_new_group {
                // NEW GROUP: seed tag with window's current state.
                // Stash window's own primitives, copy them to group seed.
                // Pre-render sync will fill drawing_manager from group each frame.
                if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(joining_leaf) {
                    window.group_id = Some(group_id);
                    window.pre_tag_indicator_ids = pre_tag_ids;
                    // Stash window's own primitives (for restore on desync).
                    let own_prims: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                        window.drawing_manager.primitives().iter()
                            .filter(|p| p.data().origin_id.is_none())
                            .map(|p| p.clone_box())
                            .collect();
                    window.stashed_primitives = own_prims;
                    // Stash window's command history — shared ops will go to group history.
                    window.stashed_command_history = Some(std::mem::replace(
                        &mut window.command_history,
                        zengeld_chart::CommandHistory::new(250),
                    ));
                }

                // Seed primitives into group (clone from stash).
                let prim_symbol = self.panel_app.panel_grid.window_for_leaf(joining_leaf)
                    .map(|w| w.symbol.clone())
                    .unwrap_or_default();
                let mut source_prims: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                    self.panel_app.panel_grid.window_for_leaf(joining_leaf)
                        .map(|w| {
                            w.stashed_primitives.iter()
                                .map(|p| p.clone_box())
                                .collect()
                        })
                        .unwrap_or_default();
                for p in &mut source_prims {
                    p.data_mut().symbol = prim_symbol.clone();
                }
                // Re-ID the group copies so they are globally unique.
                // The stash retains the original IDs for restoration on leave.
                for p in &mut source_prims {
                    p.data_mut().id = zengeld_chart::drawing::alloc_primitive_id();
                }
                if let Some(group) = self.panel_app.tag_manager.group_mut(group_id) {
                    group.primitives = source_prims;
                }

                // Seed indicator configs.
                let seed_symbol = self.panel_app.panel_grid.window_for_leaf(joining_leaf)
                    .map(|w| w.symbol.clone())
                    .unwrap_or_default();
                let mut configs: Vec<zengeld_chart::tag_manager::IndicatorGroupConfig> = self
                    .indicator_manager
                    .instances_iter()
                    .filter(|i| i.origin_id.is_none() && i.window_id == Some(chart_id.0))
                    .map(|i| zengeld_chart::tag_manager::IndicatorGroupConfig {
                        id: i.id,
                        type_id: i.type_id.clone(),
                        name: i.name.clone(),
                        params: std::collections::HashMap::new(),
                        pane: i.pane as u32,
                        visible: i.visible,
                        symbol: seed_symbol.clone(),
                    })
                    .collect();
                // Sort by id to preserve original creation order (HashMap iteration is random).
                configs.sort_by_key(|c| c.id);
                if let Some(group) = self.panel_app.tag_manager.group_mut(group_id) {
                    eprintln!(
                        "[TagManager] Seeded new tag {:?} with {} primitives, {} indicators",
                        group_id, group.primitives.len(), configs.len()
                    );
                    group.indicator_configs = configs;
                }
                return;
            }

            // EXISTING GROUP: stash window's own primitives and hide its indicators.
            // The window switches to showing tag content only.

            // Stash primitives: move window's own primitives into stashed_primitives.
            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(joining_leaf) {
                let own_prims: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                    window.drawing_manager.primitives().iter()
                        .filter(|p| p.data().origin_id.is_none())
                        .map(|p| p.clone_box())
                        .collect();
                let stash_count = own_prims.len();
                window.stashed_primitives = own_prims;
                window.drawing_manager.clear_all_primitives();
                window.group_id = Some(group_id);
                window.pre_tag_indicator_ids = pre_tag_ids.clone();
                // Stash window's command history — shared ops will go to group history.
                window.stashed_command_history = Some(std::mem::replace(
                    &mut window.command_history,
                    zengeld_chart::CommandHistory::new(250),
                ));
                eprintln!("[TagManager] Stashed {} window primitives before joining existing group", stash_count);
            }

            // Hide pre-tag indicators by setting visible=false.
            for &id in &pre_tag_ids {
                if let Some(inst) = self.indicator_manager.get_instance_mut(id) {
                    inst.visible = false;
                }
            }
            eprintln!("[TagManager] Hid {} pre-tag indicators", pre_tag_ids.len());

            // Sync tag indicators to joining window.
            let leaf_chart_ids = vec![(joining_leaf, chart_id)];
            self.sync_group_indicators_to_new_members(group_id, &leaf_chart_ids);
            self.sync_sub_panes_from_manager();
            self.sidebar_data_dirty = true;
            eprintln!("[TagManager] Join existing group: stashed own state, synced tag content");
        }

        // Dead fallback removed — TagManager handles all sync join logic above.
    }

    /// Split the active leaf: snapshot its color tag before split (the old
    /// LeafId is destroyed by `split_leaf`), then assign inherited or new color
    /// to all resulting leaves and propagate crosshair/viewport.
    pub(super) fn do_split(&mut self, kind: zengeld_chart::SplitKind) {
        // Snapshot color, group_id, and chart_id BEFORE split — active_leaf will be destroyed.
        let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
        let pre_split_color = active_leaf
            .and_then(|lid| self.panel_app.leaf_color_tags.remove(&lid));
        let pre_split_group_id = active_leaf
            .and_then(|lid| self.panel_app.panel_grid.window_for_leaf(lid))
            .and_then(|w| w.group_id);
        let pre_split_chart_id = active_leaf
            .and_then(|lid| self.panel_app.panel_grid.chart_id_for_leaf(lid));

        let new_leaves = self.panel_app.panel_grid.split_active(kind);
        self.propagate_crosshair_after_split(&new_leaves);

        // When "Split Without Group" is enabled, the mother window (new_leaves[0])
        // keeps its existing group/tag state.  Only the new sibling leaves
        // (new_leaves[1..]) get fresh independent auto_created groups.
        if self.split_without_group && !new_leaves.is_empty() {
            // Reconnect the mother (new_leaves[0]) to its pre-split group.
            // split_active may have reassigned the chart ID, so we re-establish
            // the TagManager link rather than assuming old_cid is still valid.
            if let Some(mother_leaf) = new_leaves.first().copied() {
                if let (Some(old_cid), Some(old_gid)) = (pre_split_chart_id, pre_split_group_id) {
                    if let Some(new_cid) = self.panel_app.panel_grid.chart_id_for_leaf(mother_leaf) {
                        if new_cid != old_cid {
                            // The chart got a new ID after the split — connect the new one.
                            let _ = self.panel_app.tag_manager.connect_chart(new_cid, old_gid);
                            eprintln!("[TagManager] (SplitUntagged) Mother re-connected new cid {:?} → group {:?}", new_cid, old_gid);
                        }
                        // else: same chart_id survived the split, TagManager link is intact.
                    }
                    if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(mother_leaf) {
                        window.group_id = Some(old_gid);
                    }
                    // Restore the mother's color tag that was snapshotted before the split.
                    if let Some(color) = pre_split_color {
                        self.panel_app.leaf_color_tags.insert(mother_leaf, color);
                    }
                    eprintln!("[TagManager] (SplitUntagged) Mother leaf {:?} keeps group {:?}", mother_leaf, old_gid);
                }
            }

            // Give each NEW (non-mother) leaf its own fresh auto_created group.
            for &leaf_id in new_leaves.iter().skip(1) {
                let (symbol, timeframe) = self.panel_app.panel_grid
                    .window_for_leaf(leaf_id)
                    .map(|w| (w.symbol.clone(), w.timeframe.clone()))
                    .unwrap_or_else(|| (
                        "BTCUSDT".to_string(),
                        zengeld_chart::state::Timeframe::h1(),
                    ));
                let gid = self.panel_app.tag_manager.create_group_auto(
                    [0.0, 0.0, 0.0, 0.0],
                    symbol,
                    timeframe,
                );
                if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(leaf_id) {
                    let _ = self.panel_app.tag_manager.connect_chart(chart_id, gid);
                }
                if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                    window.group_id = Some(gid);
                }
                self.panel_app.leaf_color_tags.remove(&leaf_id);
                eprintln!("[TagManager] (SplitUntagged) Created auto group {:?} for new leaf {:?}", gid, leaf_id);
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
                .and_then(|gid| self.panel_app.tag_manager.group(gid))
                .map(|g| g.auto_created)
                .unwrap_or(false);

            // Disconnect the old chart from its group (leaf destroyed by split).
            if let (Some(old_cid), Some(old_gid)) = (pre_split_chart_id, pre_split_group_id) {
                let _ = self.panel_app.tag_manager.disconnect_chart(old_cid);
                eprintln!("[TagManager] Disconnected destroyed chart {:?} from group {:?}", old_cid, old_gid);
                // Bug C fix: remove the old group if it became empty and was auto-created.
                if let Some(g) = self.panel_app.tag_manager.group(old_gid) {
                    if g.auto_created && g.members.is_empty() {
                        self.panel_app.tag_manager.remove_group(old_gid);
                        eprintln!("[TagManager] Removed empty auto group {:?} after split", old_gid);
                    }
                }
            }

            let group_id = if let Some(existing_group) = pre_split_group_id {
                if pre_split_is_auto {
                    // Was untagged (auto group) — split promotes to a real color tag.
                    // Create a NEW visible group with the first free color.
                    let color = self.panel_app.tag_manager.next_unused_color();
                    let (symbol, timeframe) = self.panel_app.panel_grid
                        .window_for_leaf(new_leaves[0])
                        .map(|w| (w.symbol.clone(), w.timeframe.clone()))
                        .unwrap_or_else(|| (
                            "BTCUSDT".to_string(),
                            zengeld_chart::state::Timeframe::h1(),
                        ));
                    let gid = self.panel_app.tag_manager.create_group(color, symbol, timeframe);
                    eprintln!("[TagManager] Split promoted auto group to visible tag {:?}", gid);
                    gid
                } else {
                    // User-tagged group: all split windows intentionally share it.
                    existing_group
                }
            } else {
                // No existing group — pick color from TagManager's palette.
                let color = pre_split_color
                    .unwrap_or_else(|| self.panel_app.tag_manager.next_unused_color());
                let (symbol, timeframe) = self.panel_app.panel_grid
                    .window_for_leaf(new_leaves[0])
                    .map(|w| (w.symbol.clone(), w.timeframe.clone()))
                    .unwrap_or_else(|| (
                        "BTCUSDT".to_string(),
                        zengeld_chart::state::Timeframe::h1(),
                    ));
                self.panel_app.tag_manager.create_group(color, symbol, timeframe)
            };

            // --- Shared group path (user-tagged groups or brand-new groups) ---

            // Assign the group's color to leaf_color_tags for UI display.
            let group_color = self.panel_app.tag_manager.group(group_id)
                .map(|g| g.color)
                .unwrap_or([0.5, 0.5, 0.5, 0.9]);
            for &leaf_id in &new_leaves {
                self.panel_app.leaf_color_tags.insert(leaf_id, group_color);
            }
            // Determine if this is a truly new group (just created, has no state yet).
            let is_new_group = self.panel_app.tag_manager.group(group_id)
                .is_none_or(|g| g.members.is_empty() && g.indicator_configs.is_empty() && g.primitives.is_empty());
            // Collect (leaf_id, chart_id) first to avoid mixed borrow when setting group_id.
            let leaf_chart_ids: Vec<(zengeld_chart::LeafId, zengeld_chart::ChartId)> = new_leaves
                .iter()
                .filter_map(|&leaf_id| {
                    self.panel_app.panel_grid.chart_id_for_leaf(leaf_id)
                        .map(|cid| (leaf_id, cid))
                })
                .collect();
            for &(_, chart_id) in &leaf_chart_ids {
                let _ = self.panel_app.tag_manager.connect_chart(chart_id, group_id);
            }
            for &(leaf_id, _) in &leaf_chart_ids {
                if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                    window.group_id = Some(group_id);
                }
            }

            // For new groups: snapshot source window's current indicator and primitive ids as pre-tag.
            // On desync, only items NOT in these lists will be removed.
            if is_new_group {
                if let Some(&(source_leaf, source_cid)) = leaf_chart_ids.first() {
                    let pre_tag_ids: Vec<u64> = self.indicator_manager
                        .instances_iter()
                        .filter(|i| i.window_id == Some(source_cid.0) && i.origin_id.is_none())
                        .map(|i| i.id)
                        .collect();
                    if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(source_leaf) {
                        window.pre_tag_indicator_ids = pre_tag_ids;
                        // Stash source window's primitives for restore on desync.
                        let own_prims: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                            window.drawing_manager.primitives().iter()
                                .filter(|p| p.data().origin_id.is_none())
                                .map(|p| p.clone_box())
                                .collect();
                        window.stashed_primitives = own_prims;
                        // Stash source window's command history for restore on desync.
                        window.stashed_command_history = Some(std::mem::replace(
                            &mut window.command_history,
                            zengeld_chart::CommandHistory::new(250),
                        ));
                    }
                }
            }
            eprintln!(
                "[TagManager] {} {:?} with {} members after split",
                if is_new_group { "Created group" } else { "Joined existing group" },
                group_id,
                new_leaves.len()
            );

            // Clear legacy-cloned primitives from non-source windows.
            // clone_for_split copies primitives via clone_primitives_for_sync,
            // but for grouped windows the pre-render sync will fill them from
            // the group each frame.  The source (first) leaf keeps its originals
            // since they'll be seeded into the group below.
            for &(leaf_id, _) in leaf_chart_ids.iter().skip(1) {
                if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                    window.drawing_manager.clear_all_primitives();
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
                let prim_sym = self.panel_app.panel_grid
                    .window_for_leaf(new_leaves[0])
                    .map(|w| w.symbol.clone())
                    .unwrap_or_default();
                let mut source_prims: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                    self.panel_app.panel_grid
                        .window_for_leaf(new_leaves[0])
                        .map(|w| {
                            w.drawing_manager.primitives().iter()
                                .filter(|p| p.data().origin_id.is_none())
                                .map(|p| p.clone_box())
                                .collect()
                        })
                        .unwrap_or_default();
                for p in &mut source_prims {
                    p.data_mut().symbol = prim_sym.clone();
                }
                // Re-ID the group copies so they are globally unique.
                // The source window's drawing_manager keeps the original IDs.
                for p in &mut source_prims {
                    p.data_mut().id = zengeld_chart::drawing::alloc_primitive_id();
                }
                if !source_prims.is_empty() {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(group_id) {
                        group.primitives = source_prims;
                        eprintln!(
                            "[TagManager] Seeded group {:?} with {} primitives",
                            group_id, group.primitives.len()
                        );
                    }
                }

                // Indicators: snapshot source window's indicators into group configs.
                if let Some(source_chart_id) = leaf_chart_ids.first().map(|(_, cid)| *cid) {
                    let source_symbol = leaf_chart_ids.first()
                        .and_then(|(lid, _)| self.panel_app.panel_grid.window_for_leaf(*lid))
                        .map(|w| w.symbol.clone())
                        .unwrap_or_default();
                    let mut configs: Vec<zengeld_chart::tag_manager::IndicatorGroupConfig> = self
                        .indicator_manager
                        .instances_iter()
                        .filter(|i| {
                            i.origin_id.is_none() && i.window_id == Some(source_chart_id.0)
                        })
                        .map(|i| zengeld_chart::tag_manager::IndicatorGroupConfig {
                            id: i.id,
                            type_id: i.type_id.clone(),
                            name: i.name.clone(),
                            params: std::collections::HashMap::new(),
                            pane: i.pane as u32,
                            visible: i.visible,
                            symbol: source_symbol.clone(),
                        })
                        .collect();
                    // Sort by id to preserve original creation order (HashMap iteration is random).
                    configs.sort_by_key(|c| c.id);
                    if let Some(group) = self.panel_app.tag_manager.group_mut(group_id) {
                        let count = configs.len();
                        group.indicator_configs = configs;
                        eprintln!(
                            "[TagManager] Seeded group {:?} with {} indicator configs",
                            group_id, count
                        );
                    }
                }
            }

            // For non-source leaves (split children): create indicator instances
            // from the tag. These are empty windows that only see tag state.
            {
                let non_source: Vec<(zengeld_chart::LeafId, zengeld_chart::ChartId)> =
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
    pub(super) fn propagate_crosshair_after_split(&mut self, new_leaves: &[zengeld_chart::LeafId]) {
        if new_leaves.len() < 2 {
            return;
        }
        let source_leaf = new_leaves[0];
        // Read crosshair + viewport state from source window.
        let (timestamp, price, visible, pane_index, view_start, bar_spacing) =
            match self.panel_app.panel_grid.window_for_leaf(source_leaf) {
                Some(w) => {
                    let bar_f64 = w.crosshair.bar_f64;
                    let bar_idx = bar_f64 as usize;
                    let timestamp = if bar_idx < w.bars.len() {
                        w.bars[bar_idx].timestamp
                    } else if w.bars.len() >= 2 {
                        let last = w.bars[w.bars.len() - 1].timestamp;
                        let prev = w.bars[w.bars.len() - 2].timestamp;
                        let interval = last - prev;
                        let bars_past = bar_f64 - (w.bars.len() - 1) as f64;
                        last + (bars_past * interval as f64).round() as i64
                    } else if !w.bars.is_empty() {
                        w.bars[0].timestamp
                    } else {
                        0
                    };
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
    /// All other leaves sharing the same color tag get the same symbol applied.
    pub(super) fn propagate_symbol_to_sync_group(&mut self, source_leaf: zengeld_chart::LeafId, symbol: &str) {
        let should_sync = self.panel_app.panel_grid.chart_id_for_leaf(source_leaf)
            .and_then(|cid| self.panel_app.tag_manager.group_for_window(cid))
            .and_then(|gid| self.panel_app.tag_manager.group(gid))
            .map(|g| g.sync_flags.sync_symbol)
            .unwrap_or(true);
        if !should_sync { return; }

        let source_color = match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
            Some(c) => c,
            None => return,
        };
        let sync_leaves: Vec<zengeld_chart::LeafId> = self.panel_app.leaf_color_tags.iter()
            .filter(|(&lid, &c)| lid != source_leaf && sync_colors_match(c, source_color))
            .map(|(&lid, _)| lid)
            .collect();
        let symbol_owned = symbol.to_string();

        // Collect (exchange_string, old_symbol, timeframe, account_type) for each peer BEFORE mutating
        // windows so we can call bridge.request_bars() after the window mutations.
        let mut peer_requests: Vec<(String, String, zengeld_chart::state::Timeframe, String)> = Vec::new();
        for leaf_id in &sync_leaves {
            if let Some(window) = self.panel_app.panel_grid.window_for_leaf(*leaf_id) {
                peer_requests.push((
                    window.exchange.clone(),
                    window.symbol.clone(),
                    window.timeframe.clone(),
                    window.account_type.clone(),
                ));
            }
        }

        // Apply the symbol change to each peer window directly, bypassing
        // change_symbol() which uses NullDataProvider and always fails.
        for (leaf_id, (old_exchange, old_symbol, _, old_account_type)) in sync_leaves.iter().zip(peer_requests.iter()) {
            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(*leaf_id) {
                window.snapshot_drawings_for_symbol(old_symbol, old_exchange, old_account_type);
                window.symbol = symbol_owned.clone();
                window.drawing_manager.set_current_symbol_key(&window.symbol, &window.exchange, &window.account_type);
                window.bars.clear();
                window.viewport.bar_count = 0;
                window.viewport.view_start = 0.0;
                window.pending_symbol_load = true;
                window.drawing_manager.clear_all_primitives();
                window.restore_drawings_for_symbol(&symbol_owned, old_exchange, old_account_type);
                window.update_title();
            }
        }

        // Request fresh bars for each peer via the bridge.
        let bar_count = self.panel_app.user_manager.profile.bar_count as usize;
        for (exchange_str, _, timeframe, at_label) in peer_requests {
            if symbol_owned.is_empty() {
                continue;
            }
            let resolved_exchange = self.exchange_symbols
                .keys()
                .find(|eid| eid.as_str() == exchange_str)
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
            let at = crate::account_type_from_label(&at_label);
            self.bridge.ensure_connector(resolved_exchange);
            self.bridge.request_bars(resolved_exchange, &symbol_owned, &timeframe, at, None, Some(bar_count), false);
            eprintln!(
                "[TagManager] Requested bars for peer {} @ {} tf={:?} (symbol propagation)",
                symbol_owned, eid_str, timeframe
            );
        }

        // Also update the TagManager group's canonical instrument so the group
        // state stays consistent with the displayed windows.
        if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(source_leaf) {
            // Collect exchange + account_type from the source window before TagManager borrow.
            let (source_exchange, source_account_type) =
                self.panel_app.panel_grid.window_for_leaf(source_leaf)
                    .map(|w| (w.exchange.clone(), w.account_type.clone()))
                    .unwrap_or_default();

            let group_id = self.panel_app.panel_grid.chart_id_for_leaf(source_leaf)
                .and_then(|cid| self.panel_app.tag_manager.group_for_window(cid));

            let member = zengeld_chart::tag_manager::SyncMemberId::Chart(chart_id.0);
            self.panel_app.tag_manager.set_instrument(
                member,
                symbol.to_string(),
                source_exchange,
                source_account_type,
            );
            eprintln!(
                "[TagManager] Updated group symbol to '{}' via chart {:?}",
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
        source_leaf: zengeld_chart::LeafId,
        tf: zengeld_chart::state::Timeframe,
    ) {
        let should_sync = self.panel_app.panel_grid.chart_id_for_leaf(source_leaf)
            .and_then(|cid| self.panel_app.tag_manager.group_for_window(cid))
            .and_then(|gid| self.panel_app.tag_manager.group(gid))
            .map(|g| g.sync_flags.sync_timeframe)
            .unwrap_or(true);
        if !should_sync { return; }

        let source_color = match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
            Some(c) => c,
            None => return,
        };
        let sync_leaves: Vec<zengeld_chart::LeafId> = self.panel_app.leaf_color_tags.iter()
            .filter(|(&lid, &c)| lid != source_leaf && sync_colors_match(c, source_color))
            .map(|(&lid, _)| lid)
            .collect();

        // Collect (exchange_string, symbol, account_type) for each peer BEFORE mutating windows,
        // so we can call bridge.request_bars() after setting the new timeframe.
        let mut peer_requests: Vec<(String, String, String)> = Vec::new();
        for leaf_id in &sync_leaves {
            if let Some(window) = self.panel_app.panel_grid.window_for_leaf(*leaf_id) {
                peer_requests.push((window.exchange.clone(), window.symbol.clone(), window.account_type.clone()));
            }
        }

        // Set timeframe and clear stale bars on each peer window directly,
        // bypassing change_timeframe() which uses NullDataProvider and fails.
        for leaf_id in &sync_leaves {
            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(*leaf_id) {
                window.timeframe = tf.clone();
                window.bars.clear();
                window.viewport.bar_count = 0;
                window.viewport.view_start = 0.0;
                window.update_title();
            }
        }

        // Request fresh bars for each peer via the bridge.
        let bar_count = self.panel_app.user_manager.profile.bar_count as usize;
        for (exchange_str, symbol, at_label) in peer_requests {
            if symbol.is_empty() {
                continue;
            }
            let resolved_exchange = self.exchange_symbols
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
            self.bridge.ensure_connector(resolved_exchange);
            self.bridge.request_bars(resolved_exchange, &symbol, &tf, at, None, Some(bar_count), true);
            eprintln!(
                "[TagManager] Requested bars for peer {} @ {} tf={:?} (timeframe propagation)",
                symbol, eid_str, tf
            );
        }

        // Also update the TagManager group's canonical timeframe so the group
        // state stays consistent with the displayed windows.
        if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(source_leaf) {
            self.panel_app.tag_manager.set_timeframe(chart_id, tf.clone());
            eprintln!(
                "[TagManager] Updated group timeframe to {:?} via chart {:?}",
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
        source_leaf: zengeld_chart::LeafId,
    ) {
        // Determine the source window's color tag.
        let source_color = match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
            Some(c) => c,
            None => return,
        };

        // Collect peer leaf IDs that share the same color tag.
        let peer_leaves: Vec<zengeld_chart::LeafId> = self.panel_app.leaf_color_tags.iter()
            .filter(|(&lid, &c)| lid != source_leaf && sync_colors_match(c, source_color))
            .map(|(&lid, _)| lid)
            .collect();

        if peer_leaves.is_empty() {
            return;
        }

        // Get the chart ID for the source leaf, then read the new primitive's ID.
        let source_chart_id = match self.panel_app.panel_grid.chart_id_for_leaf(source_leaf) {
            Some(id) => id,
            None => return,
        };

        // If the source window is in a TagManager group, primitives are shared via the
        // group — no clone-based propagation is needed.
        if self.panel_app.panel_grid
            .windows()
            .get(&source_chart_id)
            .map(|w| w.group_id.is_some())
            .unwrap_or(false)
        {
            return;
        }

        let prim_id = match self.panel_app.panel_grid
            .windows()
            .get(&source_chart_id)
            .and_then(|w| w.drawing_manager.last_original_id())
        {
            Some(id) => id,
            None => return,
        };

        // For each peer leaf, clone the primitive into its drawing manager.
        for peer_leaf in peer_leaves {
            let peer_chart_id = match self.panel_app.panel_grid.chart_id_for_leaf(peer_leaf) {
                Some(id) => id,
                None => continue,
            };

            // Clone the primitive from the source window.
            let cloned = match self.panel_app.panel_grid
                .windows()
                .get(&source_chart_id)
                .and_then(|w| w.drawing_manager.clone_primitive_for_sync(prim_id, peer_chart_id.0))
            {
                Some(c) => c,
                None => continue,
            };

            // Insert the clone into the peer window.
            if let Some(peer_window) = self.panel_app.panel_grid.windows_mut().get_mut(&peer_chart_id) {
                peer_window.drawing_manager.add_synced_primitives(vec![cloned]);
                eprintln!("[ChartApp] Synced primitive {} to peer chart {:?}", prim_id, peer_chart_id);
            }
        }
    }

    /// Propagate crosshair position to all leaves in the same sync color group.
    ///
    /// `source_leaf` is the leaf whose crosshair was just updated.
    /// All other leaves sharing the same color tag receive a matching position
    /// via [`ChartWindow::set_crosshair_from_timestamp`], which converts the
    /// universal timestamp to each peer's local fractional bar index.
    pub(super) fn propagate_crosshair_to_sync_group(
        &mut self,
        source_leaf: zengeld_chart::LeafId,
        timestamp: i64,
        price: f64,
        visible: bool,
        pane_index: Option<usize>,
    ) {
        let source_chart_id = self.panel_app.panel_grid.chart_id_for_leaf(source_leaf);
        let group_id = source_chart_id
            .and_then(|cid| self.panel_app.tag_manager.group_for_window(cid));

        let should_sync = group_id
            .and_then(|gid| self.panel_app.tag_manager.group(gid))
            .map(|g| g.sync_flags.sync_crosshair)
            .unwrap_or(true);
        if !should_sync { return; }

        let source_color = match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
            Some(c) => c,
            None => return,
        };

        // Propagate crosshair to peer chart windows in the same color-tag group.
        let sync_leaves: Vec<zengeld_chart::LeafId> = self.panel_app.leaf_color_tags.iter()
            .filter(|(&lid, &c)| lid != source_leaf && sync_colors_match(c, source_color))
            .map(|(&lid, _)| lid)
            .collect();
        for leaf_id in sync_leaves {
            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                window.set_crosshair_from_timestamp(timestamp, price, visible, pane_index);
            }
        }

        // Propagate crosshair price to all order-flow panels that belong to the same sync group.
        if let Some(gid) = group_id {
            let panel_ids = self.panel_app.tag_manager.panel_members(gid);
            let crosshair_price = if visible { Some(price) } else { None };
            for pid in panel_ids {
                let panel_id = sidebar_content::free_slot::PanelId(pid);
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
            }
        }
    }

    /// DEPRECATED: Legacy drawing state propagation for non-grouped windows.
    /// For grouped windows, drawing state sync should go through TagManager.
    /// Will be removed once all windows use TagManager groups.
    pub(super) fn propagate_drawing_state_to_sync_group(
        &mut self,
        source_leaf: zengeld_chart::LeafId,
    ) {
        let should_sync = self.panel_app.panel_grid.chart_id_for_leaf(source_leaf)
            .and_then(|cid| self.panel_app.tag_manager.group_for_window(cid))
            .and_then(|gid| self.panel_app.tag_manager.group(gid))
            .map(|g| g.sync_flags.sync_drawings)
            .unwrap_or(true);
        if !should_sync { return; }

        // Determine the source window's color tag.
        let source_color = match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
            Some(c) => c,
            None => return,
        };

        // Collect peer leaf IDs that share the same color tag.
        let peer_leaves: Vec<zengeld_chart::LeafId> = self.panel_app.leaf_color_tags.iter()
            .filter(|(&lid, &c)| lid != source_leaf && sync_colors_match(c, source_color))
            .map(|(&lid, _)| lid)
            .collect();

        if peer_leaves.is_empty() {
            return;
        }

        // Read the current drawing state from the source window.
        // We extract tool_id and points so we can release the borrow before
        // mutating peer windows.
        let (tool_id, points) = match self.panel_app.panel_grid.window_for_leaf(source_leaf) {
            Some(w) => match w.drawing_manager.drawing_state() {
                zengeld_chart::drawing::DrawingState::Creating { tool_id, points } => {
                    (Some(tool_id.clone()), points.clone())
                }
                zengeld_chart::drawing::DrawingState::Idle => (None, Vec::new()),
            },
            None => return,
        };

        // Apply to every peer leaf.
        for peer_leaf in peer_leaves {
            if let Some(peer_window) = self.panel_app.panel_grid.window_for_leaf_mut(peer_leaf) {
                peer_window.drawing_manager.set_synced_drawing_state(tool_id.clone(), points.clone());
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
        let (chart_id, group_id) = match self.panel_app.panel_grid.active_window() {
            Some(w) if w.group_id.is_some() => {
                let cid = match self.panel_app.panel_grid.active_chart_id() {
                    Some(id) => id,
                    None => return false,
                };
                (cid, w.group_id.unwrap())
            }
            _ => return false,
        };

        // If sync_drawings is OFF for this group, leave the primitive in the
        // drawing_manager for this window only — forward sync is blocked anyway,
        // so moving it to the group would cause it to disappear visually.
        let (sync_drawings_on, is_mono) = self.panel_app.tag_manager
            .group(group_id)
            .map(|g| (g.sync_flags.sync_drawings, g.members.len() <= 1))
            .unwrap_or((true, false));
        if !sync_drawings_on {
            return false;
        }
        // Symmetric mono-group guard: forward sync (group → window) in
        // prepare_frame skips when members <= 1 to avoid wiping local
        // primitives. Intercept must do the same — otherwise the primitive
        // is moved into group.primitives and never returns to drawing_manager,
        // becoming invisible on the chart while still listed in Object Tree.
        if is_mono {
            return false;
        }

        // Pop the last primitive from the window's drawing_manager.
        let prim = {
            let window = match self.panel_app.panel_grid.windows_mut().get_mut(&chart_id) {
                Some(w) => w,
                None => return false,
            };
            let idx = match window.drawing_manager.last_index() {
                Some(i) => i,
                None => return false,
            };
            match window.drawing_manager.remove(idx) {
                Some(p) => p,
                None => return false,
            }
        };

        // Stamp the symbol on the primitive so per-symbol tracking works.
        let mut prim = prim;
        let prim_symbol = self.panel_app.panel_grid.windows().get(&chart_id)
            .map(|w| w.symbol.clone())
            .unwrap_or_default();
        prim.data_mut().symbol = prim_symbol;

        // Add the primitive to the TagManager group so all members share it.
        if let Some(group) = self.panel_app.tag_manager.group_mut(group_id) {
            group.primitives.push(prim);
            eprintln!(
                "[TagManager] Moved completed primitive into group {:?} (now {} primitives)",
                group_id,
                group.primitives.len()
            );
            true
        } else {
            // Group disappeared — put the primitive back into drawing_manager.
            if let Some(window) = self.panel_app.panel_grid.windows_mut().get_mut(&chart_id) {
                window.drawing_manager.add_synced_primitives(vec![prim]);
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
    pub(super) fn disconnect_from_current_group(&mut self, leaf_id: zengeld_chart::LeafId) {
        let chart_id = match self.panel_app.panel_grid.chart_id_for_leaf(leaf_id) {
            Some(id) => id,
            None => return,
        };
        let old_gid = match self.panel_app.panel_grid.window_for_leaf(leaf_id)
            .and_then(|w| w.group_id)
        {
            Some(gid) => gid,
            None => return,
        };
        let _ = self.panel_app.tag_manager.disconnect_chart(chart_id);
        // Remove the old group if it is now empty and was auto-created.
        if let Some(g) = self.panel_app.tag_manager.group(old_gid) {
            if g.auto_created && g.members.is_empty() {
                self.panel_app.tag_manager.remove_group(old_gid);
                eprintln!("[TagManager] Removed empty auto group {:?}", old_gid);
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
    pub(super) fn sync_drawing_back_to_group_for(&mut self, leaf_id: zengeld_chart::LeafId) {
        let chart_id = match self.panel_app.panel_grid.chart_id_for_leaf(leaf_id) {
            Some(cid) => cid,
            None => return,
        };
        let group_id = match self.panel_app.panel_grid.windows().get(&chart_id)
            .and_then(|w| w.group_id)
        {
            Some(gid) => gid,
            None => return,
        };
        // Respect the sync_drawings flag — if disabled, do not write back to group.
        if let Some(group) = self.panel_app.tag_manager.group(group_id) {
            if !group.sync_flags.sync_drawings {
                return;
            }
        }
        let new_prims: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
            self.panel_app.panel_grid.windows().get(&chart_id)
                .map(|w| w.drawing_manager.primitives().iter().map(|p| p.clone_box()).collect())
                .unwrap_or_default();
        if let Some(group) = self.panel_app.tag_manager.group_mut(group_id) {
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
        group_id: zengeld_chart::tag_manager::SyncGroupId,
        new_leaf_chart_ids: &[(zengeld_chart::LeafId, zengeld_chart::ChartId)],
    ) {
        // Respect the sync_indicators flag — if disabled, skip indicator sync.
        if let Some(group) = self.panel_app.tag_manager.group(group_id) {
            if !group.sync_flags.sync_indicators {
                return;
            }
        }

        // Collect indicator configs from the group.
        let configs: Vec<(String, String)> = self.panel_app.tag_manager
            .group(group_id)
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
            let symbol = self.panel_app.panel_grid
                .window_for_leaf(leaf_id)
                .map(|w| w.symbol.clone())
                .unwrap_or_default();

            for (type_id, name) in &configs {
                // Check if this window already has this indicator type.
                let already_has = self.indicator_manager.instances_iter()
                    .any(|i| i.window_id == Some(chart_id.0) && i.type_id == *type_id);
                if already_has {
                    continue;
                }

                if let Some(new_id) = self.indicator_manager.create_instance(type_id, &symbol) {
                    if let Some(inst) = self.indicator_manager.get_instance_mut(new_id) {
                        inst.window_id = Some(chart_id.0);
                    }
                    eprintln!(
                        "[TagManager] Split sync: created indicator '{}' (id={}) for chart {:?}",
                        name, new_id, chart_id
                    );
                }
            }

            // Calculate indicators with this window's bars.
            let bars: Option<Vec<zengeld_chart::Bar>> = self.panel_app.panel_grid
                .window_for_leaf(leaf_id)
                .map(|w| w.bars.clone());
            if let Some(bars) = bars {
                self.indicator_manager.calculate_all_for_symbol(&symbol, &bars);
            }
        }
        self.sync_sub_panes_from_manager();
        eprintln!(
            "[TagManager] Synced {} indicator configs to {} new members",
            configs.len(), new_leaf_chart_ids.len()
        );
    }

    /// When an indicator is created on a grouped window, create matching instances
    /// on all peer windows in the same group.
    pub(super) fn sync_group_indicator_to_peers(
        &mut self,
        group_id: zengeld_chart::tag_manager::SyncGroupId,
        type_id: &str,
        source_leaf: zengeld_chart::LeafId,
    ) {
        // Respect the sync_indicators flag — if disabled, skip peer sync.
        if let Some(group) = self.panel_app.tag_manager.group(group_id) {
            if !group.sync_flags.sync_indicators {
                return;
            }
        }

        // Collect peer chart_ids and their symbols (excluding source).
        let source_chart_id = self.panel_app.panel_grid.chart_id_for_leaf(source_leaf);
        let peer_info: Vec<(zengeld_chart::ChartId, String)> = self.panel_app.tag_manager
            .chart_members(group_id)
            .into_iter()
            .filter(|&cid| Some(cid) != source_chart_id)
            .filter_map(|cid| {
                self.panel_app.panel_grid.windows().get(&cid)
                    .map(|w| (cid, w.symbol.clone()))
            })
            .collect();

        for (peer_cid, peer_symbol) in peer_info {
            if let Some(new_id) = self.indicator_manager.create_instance(type_id, &peer_symbol) {
                if let Some(inst) = self.indicator_manager.get_instance_mut(new_id) {
                    inst.window_id = Some(peer_cid.0);
                    inst.origin_id = None; // It's a group-managed indicator, not a clone.
                }
                // Calculate with peer's bars.
                let bars: Option<Vec<zengeld_chart::Bar>> = self.panel_app.panel_grid
                    .windows().get(&peer_cid)
                    .map(|w| w.bars.clone());
                if let Some(bars) = bars {
                    self.indicator_manager.calculate_all_for_symbol(&peer_symbol, &bars);
                }
                eprintln!(
                    "[TagManager] Synced indicator '{}' (id={}) to peer chart {:?}",
                    type_id, new_id, peer_cid
                );
            }
        }
        self.sync_sub_panes_from_manager();
    }
}
