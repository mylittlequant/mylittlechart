//! Sync group propagation: replicate state changes (symbol, timeframe, drawings,
//! crosshair, indicators) across all charts in the same sync group.
//! Also handles split, desync, color-group join.

use crate::{ChartApp, sglog};
use super::sync_colors_match;

impl ChartApp {
    /// Hide crosshairs on all split leaves that are NOT in the same sync group
    /// as `source_leaf`.  Sync-group peers are updated by
    /// `propagate_crosshair_to_sync_group`; this covers the non-peer leaves.
    pub(super) fn hide_crosshairs_outside_sync_group(&mut self, source_leaf: mlc_core::LeafId) {
        let source_color = self.panel_app.leaf_color_tags.get(&source_leaf).copied();
        let crosshair_sync_enabled = self.panel_app.panel_grid
            .container_id_for_leaf(source_leaf)
            .and_then(|cid| self.bubble_for_chart(cid))
            .and_then(|gid| self.with_bubble(gid, |g| g.sync_flags.sync_crosshair))
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
        use mlc_core::ids::SyncMemberId;
        let member = SyncMemberId::Panel(panel_id);
        self.disconnect_member(member);
        let gid = self.create_auto_bubble(
            [0.5, 0.5, 0.5, 1.0],
            String::new(),
            mlc_core::state::Timeframe::m1(),
        );
        self.set_panel_auto(member, gid);
        log::warn!("[ChartApp] Panel {} desynced → auto group {:?}", panel_id, gid);
        self.sidebar_data_dirty = true;
        self.autosave_snapshot();
    }

    pub(super) fn perform_desync(&mut self, leaf_id: mlc_core::LeafId) {
        // Diagnostic: dump auto_bubble state on ENTER.
        {
            let cid_dbg = self.panel_app.panel_grid.container_id_for_leaf(leaf_id);
            let auto_bid_dbg = cid_dbg
                .and_then(|cid| self.panel_app.panel_grid.containers().get(&cid).map(|w| w.auto_tag_id));
            let cur_bid_dbg = cid_dbg.and_then(|cid| self.bubble_for_chart(cid));
            let auto_snap = auto_bid_dbg
                .and_then(|bid| self.with_bubble(bid, |b| (b.address.symbol.clone(), b.address.exchange.clone(), b.address.account_type)));
            let cur_snap = cur_bid_dbg
                .and_then(|bid| self.with_bubble(bid, |b| (b.address.symbol.clone(), b.address.exchange.clone(), b.address.account_type)));
            sglog!(
                "[perform_desync] ENTER leaf={:?} cid={:?} auto_bid={:?} auto={:?} cur_bid={:?} cur={:?}",
                leaf_id, cid_dbg, auto_bid_dbg, auto_snap, cur_bid_dbg, cur_snap
            );
        }
        // 1. Remove color tag.
        self.panel_app.leaf_color_tags.remove(&leaf_id);

        // Snapshot group info BEFORE disconnect.
        let had_group_id = self.panel_app.panel_grid
            .container_id_for_leaf(leaf_id)
            .and_then(|cid| self.bubble_for_chart(cid))
            .is_some();

        let chart_id = match self.panel_app.panel_grid.container_id_for_leaf(leaf_id) {
            Some(id) => id,
            None => {
                log::warn!("[sync_group] perform_desync: container_for_leaf({:?}) returned None — skipping desync", leaf_id);
                return;
            }
        };

        // Read auto_tag_id BEFORE disconnect so we can re-attach.
        let auto_tag_id = self.panel_app.panel_grid
            .containers()
            .get(&chart_id)
            .map(|w| w.auto_tag_id);

        // 1b. Disconnect from current color-tag group.
        // The write guard from `self.core.bubble_mut()` lives in the if-let
        // scrutinee position, which Rust extends to the end of the if-body.
        // The inner `self.core.bubble()` then tries to take a READ guard while
        // the WRITE guard is still alive — std::sync::RwLock on wasm32 is
        // single-threaded and aborts on any read↔write overlap, which the JS
        // host surfaces as the RAF loop freezing (no panic, no JS error,
        // `MLC_FRAMES` stops advancing).  Drop the write guard explicitly by
        // binding the Option to a local first, then take the read guard in a
        // fresh scope.
        let detached_group: Option<mlc_core::ids::BubbleId> = {
            self.detach_chart_from_tag(chart_id)
        };
        if let Some(old_group_id) = detached_group {
            let remaining = {
                let tags = self.core.tags();
                tags.members(old_group_id).map_or(0, |m| m.len())
            };
            log::debug!(
                "[BubbleStore] Disconnected chart {:?} from group {:?} ({} remaining)",
                chart_id, old_group_id, remaining
            );
        }

        // 2. Re-attach chart to its permanent home auto-bubble and restore state.
        if let Some(auto_bid) = auto_tag_id {
            // Re-attach to home auto-bubble.
            self.attach_chart_to_tag(chart_id, auto_bid);

            if had_group_id {
                // Restore drawing_manager from auto_bubble.primitives (the home drawings
                // that were preserved during the sync-group membership).
                // Read pre_tag_ids from auto_bubble BEFORE taking command_history so
                // we can do the indicator cleanup step below.
                let (pre_tag_ids, restored_prims, old_history) = self.core.bubbles_mut()
                    .with_bubble_mut(auto_bid, |bub| {
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
                    })
                    .unwrap_or((Vec::new(), Vec::new(), None));

                // Restore state onto the auto-bubble. Under the per-bubble
                // drawing model the auto-bubble's `primitives` ARE what the
                // window will read once `current_tag_id` flips below — no
                // separate per-window DrawingManager to write into.
                let restored_count = restored_prims.len();
                self.core.drawings_mut().adopt(auto_bid, restored_prims);
                if restored_count > 0 {
                    log::warn!("[BubbleStore] Desync: restored {} primitives from auto-bubble", restored_count);
                } else {
                    log::warn!("[BubbleStore] Desync: auto-bubble had no primitives (fresh chart)");
                }

                // Restore command_history into the window.
                if let Some(h) = old_history {
                    if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                        window.command_history = h;
                        log::warn!("[BubbleStore] Desync: restored command history from auto-bubble");
                    }
                }

                // Update current_tag_id on the window.  Also clear the
                // home-overlay flag — the chart is back in its untagged home
                // state; the next tag entry decides afresh whether to surface
                // the overlay.
                if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                    window.current_tag_id = auto_bid;
                    window.tag_inherits_home_overlay = false;
                }

                // Restore the window's pre-join scale mode.
                let leaf_series_desync = self.panel_app.panel_grid.container_id_for_leaf(leaf_id)
                    .map(|cid| self.active_series_for_chart(cid))
                    .unwrap_or_default();
                if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                    if let Some(prev_mode) = window.stashed_scale_mode.take() {
                        window.set_scale_mode_with_recalc(prev_mode, leaf_series_desync.as_active_series());
                        log::warn!("[BubbleStore] Desync: restored stashed scale mode {:?}", prev_mode);
                    }
                }

                // Untag = pure detach + attach to home auto-bubble.  No
                // instance reassignment: indicators in the detached tag
                // bubble stay with that tag (peer charts still see them);
                // indicators in the home auto-bubble were there all along.
                // Owner UX: "untag is just a pointer flip from the tag
                // bubble to the chart's auto-bubble; nothing moves".
                // `auto_bub.primitives` was already set by `adopt()` above —
                // that IS the authoritative home-state now, no separate
                // per-window DrawingManager copy to sync back.
                let _ = &pre_tag_ids;
                log::warn!("[BubbleStore] Desync: pointer-flip to home auto_bubble {:?}", auto_bid);
            } else {
                // Was not in a color group — the window's `current_tag_id`
                // simply flips to `auto_bid`; `auto_bid`'s own primitives are
                // already authoritative (no per-window synced copy to purge
                // under the per-bubble model). Desync moves a PANEL between
                // bubbles; it destroys no bubble, so it destroys no indicator.
                // (There used to be a `purge_for_bubble(BubbleId(chart_id.0))`
                // here — a panel id cast to a bubble id, against a purge that
                // only reaped clones. Now that the purge is literal it would
                // wipe the indicators of whatever bubble happened to share the
                // panel's number, so it is gone.)
                if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                    window.current_tag_id = auto_bid;
                }
            }

            log::warn!("[BubbleStore] Desync: chart {:?} re-attached to auto-bubble {:?}", chart_id, auto_bid);
        } else {
            log::warn!(
                "[BubbleStore] Desync: chart {:?} has sentinel auto_tag_id — construction site missed post-init. Skipping desync.",
                chart_id
            );
            return;
        }

        // 5. Recalculate sub-panes so indicator panels stay in sync.
        self.sync_sub_panes_from_manager();
        self.sidebar_data_dirty = true;

        log::warn!("[ChartApp] Desynced leaf {:?} from color tag group", leaf_id);
    }

    /// Join an existing color group: clone primitives and indicators from a peer
    /// that already belongs to this color group into the joining leaf.
    pub(super) fn sync_join_color_group(&mut self, joining_leaf: mlc_core::LeafId, color: [f32; 4]) {
        self.sync_join_color_group_inner(joining_leaf, color, false);
    }

    /// Join a color group as a re-tag (chart was already in some color group).
    /// Differences from a fresh join:
    ///   - When the target group is NEW: do NOT seed primitives / indicator
    ///     configs from drawing_manager + home indicators.  The target tag is
    ///     a brand-new empty bucket and must NOT inherit the chart's home
    ///     stash (drawings + indicators that lived ungrouped before the very
    ///     first tag).  Owner UX: re-tag (tag1 -> tag2) is just a membership
    ///     flip; only split / untag-then-tag carry home content forward.
    ///   - Do NOT overwrite auto_bubble.command_history /
    ///     auto_bubble.pre_tag_indicator_ids — they were set on the first
    ///     tag entry and must be preserved across re-tags so the eventual
    ///     untag still restores the original home state.
    ///   - When the target group is EXISTING: peer-join works the same as a
    ///     fresh join (chart inherits the existing group's content; that is
    ///     the user-requested "join an existing tag" semantics).
    pub(super) fn sync_join_color_group_retag(&mut self, joining_leaf: mlc_core::LeafId, color: [f32; 4]) {
        self.sync_join_color_group_inner(joining_leaf, color, true);
    }

    fn sync_join_color_group_inner(&mut self, joining_leaf: mlc_core::LeafId, color: [f32; 4], is_retag: bool) {
        {
            let cid_dbg = self.panel_app.panel_grid.container_id_for_leaf(joining_leaf);
            let auto_bid_dbg = cid_dbg
                .and_then(|cid| self.panel_app.panel_grid.containers().get(&cid).map(|w| w.auto_tag_id));
            let cur_bid_dbg = cid_dbg.and_then(|cid| self.bubble_for_chart(cid));
            let auto_snap = auto_bid_dbg
                .and_then(|bid| self.with_bubble(bid, |b| (b.address.symbol.clone(), b.address.exchange.clone(), b.address.account_type)));
            let cur_snap = cur_bid_dbg
                .and_then(|bid| self.with_bubble(bid, |b| (b.address.symbol.clone(), b.address.exchange.clone(), b.address.account_type)));
            sglog!(
                "[sync_join_color_group] ENTER leaf={:?} cid={:?} auto_bid={:?} auto={:?} cur_bid={:?} cur={:?} color=[{:.2},{:.2},{:.2},{:.2}]",
                joining_leaf, cid_dbg, auto_bid_dbg, auto_snap, cur_bid_dbg, cur_snap,
                color[0], color[1], color[2], color[3]
            );
        }
        // Connect the joining leaf's chart to the TagManager group for this color.
        // Find-or-create the group so TagManager stays consistent regardless of
        // whether this is the first leaf in the group or a later joiner.
        if let Some(chart_id) = self.panel_app.panel_grid.container_id_for_leaf(joining_leaf) {
            let existing_group = self.core.tags().find_tag_by_color(color);
            let is_new_group = existing_group.is_none();
            let group_id = if let Some(gid) = existing_group {
                gid
            } else {
                if self.panel_app.panel_grid.container_for_leaf(joining_leaf).is_none() {
                    log::debug!("[sync_group] sync_join_color_group: container_for_leaf({:?}) returned None — skipping join", joining_leaf);
                    return;
                }
                // The instrument key MUST be inherited from the chart's home
                // auto-bubble — not from `symbol_for_chart(chart_id)`, because
                // `disconnect_from_current_group` (or `perform_desync`) called
                // just before this join detaches `chart_id` from any bubble in
                // the swatch-click flow.  After detach `bubble_for_chart` is
                // None and `symbol_for_chart` returns "", so the new color
                // group inherits an EMPTY key and the chart renders with no
                // title and broken sym:exch:AT panel routing.  Owner's repro
                // 2026-06-25: split (auto-tag) -> untag -> tag-in-new-color
                // landed in a group with blank key — exactly this path.
                //
                // The home auto_bubble is the source of truth: it is created
                // and populated in do_split / the constructor flows and is
                // never detached from the chart's container (window.auto_tag_id
                // is a permanent home pointer).  Read identity straight from
                // there so a transient detach in the middle of a re-tag
                // cannot blank the new tag-group's key.
                let auto_bid = self.panel_app.panel_grid.containers().get(&chart_id)
                    .map(|w| w.auto_tag_id);
                let auto_view: Option<(String, String, String, mlc_core::state::Timeframe)> = auto_bid
                    .and_then(|bid| self.with_bubble(bid, |b| {
                        (b.address.symbol.clone(), b.address.exchange.clone(), b.address.account_type.as_label().to_string(), b.timeframe.clone())
                    }));
                let (sym, exch_join, at_join, tf) = if let Some((s, e, a, t)) = auto_view {
                    (s, e, a, t)
                } else {
                    let s = self.symbol_for_chart(chart_id).map(String::from).unwrap_or_default();
                    let e = self.exchange_for_chart(chart_id).map(String::from).unwrap_or_default();
                    let a = self.account_type_for_chart(chart_id).map(|at| at.to_string()).unwrap_or_else(|| "S".to_string());
                    let t = self.timeframe_for_chart(chart_id).unwrap_or_else(mlc_core::state::Timeframe::h1);
                    (s, e, a, t)
                };
                let at_join = if at_join.is_empty() { "S".to_string() } else { at_join };
                sglog!(
                    "[sync_join_color_group] new color group: read from auto_bid={:?} (fallback chart_id={:?}) sym='{}' exch='{}' at='{}' tf={:?}",
                    auto_bid, chart_id, sym, exch_join, at_join, tf
                );
                let new_gid = self.create_tagged_bubble(color, sym.clone(), tf.clone());
                // create_bubble only takes (color, symbol, timeframe) — fill exchange + account_type explicitly.
                self.core.bubbles_mut().set_bubble_instrument(new_gid, exch_join.clone(), at_join.clone());
                sglog!(
                    "[sync_join_color_group] new gid={:?} seeded sym='{}' exch='{}' at='{}'",
                    new_gid, sym, exch_join, at_join
                );
                new_gid
            };
            self.attach_chart_to_tag(chart_id, group_id);
            log::debug!(
                "[BubbleStore] Connected chart {:?} to group {:?} (new={})",
                chart_id, group_id, is_new_group
            );

            // NOTE (Step 3.1): style-store rebinding is no longer needed —
            // `DrawingManager::effective_color` already reads
            // `bubble.last_used_style` directly for the bubble the window
            // points at (bid-keyed sessions, no per-window Arc to rebind).
            let pid_join = self.panel_app.panel_grid.container_for_leaf(joining_leaf).map(|w| w.current_tag_id);

            // Snapshot pre-tag indicator ids (these survive desync).
            // Indicators owned by the chart's home auto-bubble are the
            // pre-tag set under the bubble-ownership model.
            let auto_bid_raw_pre = self.panel_app.panel_grid.containers()
                .get(&chart_id)
                .map(|w| w.auto_tag_id.0);
            let pre_tag_ids: Vec<u64> = self.core.indicators()
                                .all_instances()
                .into_iter()
                .filter(|i| auto_bid_raw_pre.map_or(false, |b| i.bubble_id == Some(b)) && i.origin_id.is_none())
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
                    .map(|w| w.price_scale().scale_mode);
                if let Some(mode) = seed_mode {
                    self.core.bubbles_mut().set_scale_mode(group_id, mode);
                }

                // Get auto_tag_id before taking mutable window borrow.
                let auto_bid = self.panel_app.panel_grid
                    .containers()
                    .get(&chart_id)
                    .map(|w| w.auto_tag_id);

                // Stash command_history and pre_tag_ids into auto_bubble.
                // Replace window.command_history with a fresh one so group ops are independent.
                //
                // On RE-TAG (was already in some color group) the stash already
                // holds the original home state from the very first tag entry —
                // do NOT overwrite it, and do NOT replace window.command_history
                // (window.command_history at this point is already the empty
                // tag-scoped history left by the disconnect step; stashing it
                // back into auto_bubble would clobber the home history).
                if is_retag {
                    if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(joining_leaf) {
                        window.current_tag_id = group_id;
                        // Re-tag: this chart did NOT bring home content into
                        // the new tag (the previous tag owned it).  Do not
                        // render home stash as a Memory overlay in this tag.
                        window.tag_inherits_home_overlay = false;
                    }
                } else if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(joining_leaf) {
                    let old_history = std::mem::replace(
                        &mut window.command_history,
                        mlc_core::CommandHistory::new(250),
                    );
                    if let Some(bid) = auto_bid {
                        self.core.bubbles_mut().with_bubble_mut(bid, |auto_bub| {
                            auto_bub.command_history = old_history;
                            auto_bub.pre_tag_indicator_ids = pre_tag_ids.clone();
                        });
                    }
                    // Update current_tag_id to the new color group.
                    window.current_tag_id = group_id;
                    // First tag entry from untagged: the chart's home stash
                    // does flow into this tag's Memory overlay.
                    window.tag_inherits_home_overlay = true;
                }

                // First tag entry (not re-tag): CLONE the chart's home
                // indicators into the new tag bubble.  Owner UX: "new tag
                // bubble clones the contents of the source auto-bubble".
                // Home originals stay in auto_bubble untouched; clones live
                // in the tag bubble.
                if !is_retag {
                    let clones = {
                        let im = self.core.indicators();
                        pre_tag_ids.iter().filter_map(|iid| {
                            im.full_clone(*iid)
                        }).collect::<Vec<_>>()
                    };
                    for src in clones {
                        let mut config = mlc_backend::IndicatorFullConfig::from(&src);
                        config.origin_id = Some(src.id);
                        let mut im = self.core.indicators_mut();
                        if let Some(new_id) = im.add(Some(group_id), &src.type_id) {
                            im.apply_full_config(new_id, config);
                        }
                    }
                }

                // Seed group.primitives from auto_bubble.primitives (clone + re-ID).
                // auto_bubble.primitives already holds the chart's home drawings.
                //
                // RE-TAG note: on a tag-to-tag transition the source home
                // drawings already lived inside the previous tag group.  The
                // new group must NOT inherit them — owner UX is "re-tag is a
                // bare membership flip, not a stash carry-forward".  Skip the
                // whole seed + auto_bubble.primitives re-stash block.
                if !is_retag {
                let (prim_symbol, prim_exchange, prim_account_type) = {
                    let sym = self.symbol_for_chart(chart_id)
                        .map(String::from)
                        .unwrap_or_default();
                    let exch = self.exchange_for_chart(chart_id)
                        .map(String::from)
                        .unwrap_or_default();
                    let at = self.account_type_for_chart(chart_id)
                        .map(|at| at.to_string())
                        .unwrap_or_else(|| "S".to_string());
                    (sym, exch, at)
                };
                // Source-of-truth for an untagged chart's home drawings is the
                // window's `drawing_manager`, NOT `auto_bubble.primitives`.
                // `intercept_completed_primitive_to_group` returns early for
                // auto-mono groups (the chart's untagged home state) so newly
                // drawn primitives never flow into `auto_bubble.primitives`.
                // Owner repro 2026-06-26: split -> untag -> draw two primitives
                // -> tag-in-new-color landed in a group with empty drawings
                // because we read from the empty auto_bubble.primitives.  Read
                // drawing_manager first; fall back to auto_bubble.primitives if
                // drawing_manager has nothing (e.g. cold-load presets).
                let dm_prims: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>> =
                    pid_join.map(|bid| self.core.drawings().primitives(bid).iter()
                            .filter(|p| p.data().origin_id.is_none())
                            .map(|p| p.clone_box())
                            .collect::<Vec<_>>())
                        .unwrap_or_default();
                let auto_prims: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>> =
                    if let Some(bid) = auto_bid {
                        self.with_bubble(bid, |bub| bub.primitives.iter().map(|p| p.clone_box()).collect())
                            .unwrap_or_default()
                    } else { Vec::new() };
                let source_prims = if !dm_prims.is_empty() { dm_prims } else { auto_prims };
                sglog!(
                    "[sync_join_color_group] new-group seed primitives count={} (dm-primary, auto-fallback)",
                    source_prims.len()
                );
                let mut group_prims = source_prims;
                for p in &mut group_prims {
                    let d = p.data_mut();
                    d.symbol = prim_symbol.clone();
                    d.exchange = prim_exchange.clone();
                    d.account_type = prim_account_type.clone();
                }
                // Re-ID group copies so they are globally unique; auto_bubble
                // keeps originals. A batch remap (not a per-primitive
                // independent restamp) so any Elliott subdivision/chain
                // inside this copy still points at the COPY's own new ids
                // instead of the ORIGINAL bubble's — see
                // `reassign_primitive_ids_remapped`'s own doc (2026-08-14
                // report, WaveLink cold-restore audit).
                mlc_core::drawing::reassign_primitive_ids_remapped(&mut group_prims);
                self.core.bubbles_mut().with_bubble_mut(group_id, |group| {
                    group.primitives = group_prims;
                });
                // Stash a copy of the home drawings into `auto_bubble.primitives`
                // so a later `perform_desync` can restore them.  source_prims
                // above is the pre-clone drawing list (with original ids); we
                // need the originals here so the stash survives any re-ID
                // applied to the group copies.  Without this stash, the
                // drawing_manager gets cleared in the next step + the
                // auto_bubble is empty -> untag would render a blank chart.
                if let Some(bid) = auto_bid {
                    // Re-read drawing_manager since source_prims was moved into
                    // group_prims.  drawing_manager still holds the originals.
                    let home_copy: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>> =
                        pid_join.map(|bid| self.core.drawings().primitives(bid).iter()
                                .filter(|p| p.data().origin_id.is_none())
                                .map(|p| p.clone_box())
                                .collect::<Vec<_>>())
                            .unwrap_or_default();
                    let home_count = home_copy.len();
                    self.core.bubbles_mut().with_bubble_mut(bid, |auto_bub| {
                        auto_bub.primitives = home_copy;
                    });
                    sglog!(
                        "[sync_join_color_group] stashed {} home primitives into auto_bubble {:?}",
                        home_count, bid
                    );
                }
                } // end if !is_retag
                // NOTE (Step 3.1): no separate per-window DrawingManager to
                // clear any more — once `window.current_tag_id` flips to
                // `group_id` below, the window reads `group.primitives`
                // directly. `auto_bid`'s primitives were already stashed
                // above and must NOT be cleared here (that stash is what
                // `perform_desync` restores from later).

                // Seed bars into the brand-new color group from the chart's
                // home auto_bubble so the chart paints immediately under the
                // group view.  Without this seed the chart's
                // `current_tag_id = group_id` points at an empty
                // `bubble.bars` (create_bubble initialises it empty), and the
                // chart sits blank until the next live BarsLoaded happens to
                // run against `bubbles_for_instrument` — which only fires when
                // the REST sweep is re-issued, not on tag.  Owner repro
                // 2026-06-26: tagged sibling chart stayed blank because no
                // new REST fetch was triggered and the group bubble was empty
                // while the auto_bubble had the full 300 bars.
                let auto_bars: Vec<mlc_core::Bar> = auto_bid
                    .and_then(|bid| self.with_bubble(bid, |b| b.bars.clone()))
                    .unwrap_or_default();
                if !auto_bars.is_empty() {
                    let n = auto_bars.len();
                    self.core.bubbles_mut().set_bars(group_id, auto_bars);
                    sglog!("[sync_join_color_group] seeded {} bars from auto_bubble into new group {:?}", n, group_id);
                }

                // Enqueue an auto-cure REST sweep so the brand-new group also
                // gets a fresh authoritative payload (in case the home bubble's
                // bars were stale).  Drains on the next frame via the host's
                // pending_cure_after_preset_load drain loop.
                let cure_eid = self.exchange_for_chart(chart_id)
                    .and_then(|e| digdigdig3::ExchangeId::from_str(&e));
                let cure_sym = self.symbol_for_chart(chart_id).map(String::from).unwrap_or_default();
                let cure_at_label = self.account_type_for_chart(chart_id).map(|at| at.to_string()).unwrap_or_else(|| "S".to_string());
                let cure_tf = self.timeframe_for_chart(chart_id).unwrap_or_else(mlc_core::state::Timeframe::h1);
                if let Some(eid) = cure_eid {
                    if !cure_sym.is_empty()
                        && self.sidebar_state.connector_enabled.get(eid.as_str()).copied().unwrap_or(true)
                    {
                        let at = crate::account_type_from_label(&cure_at_label);
                        self.pending_cure_after_preset_load.push((eid, cure_sym.clone(), cure_tf, at, true));
                        sglog!("[sync_join_color_group] enqueued cure for {:?} {} (force=true)", eid, cure_sym);
                    }
                }

                // Seed indicator configs from the creating window's indicators.
                //
                // RE-TAG note: same rationale as the primitives skip above —
                // the previous tag already owned these indicators, the new
                // tag must start empty.
                if !is_retag {
                let (seed_symbol, seed_exchange, seed_account_type) = {
                    let sym = self.symbol_for_chart(chart_id)
                        .map(String::from)
                        .unwrap_or_default();
                    let exch = self.exchange_for_chart(chart_id)
                        .map(String::from)
                        .unwrap_or_default();
                    let at = self.account_type_for_chart(chart_id)
                        .map(|at| at.to_string())
                        .unwrap_or_else(|| "S".to_string());
                    (sym, exch, at)
                };
                let auto_bid_raw_cfg = self.panel_app.panel_grid.containers()
                    .get(&chart_id)
                    .map(|w| w.auto_tag_id);
                let indicators_guard_cfg = self.core.indicators();
                let mut configs: Vec<mlc_core::indicator_types::IndicatorGroupConfig> = auto_bid_raw_cfg
                    .map(|bid| indicators_guard_cfg.instances_for_bubble(bid))
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|i| i.origin_id.is_none())
                    .map(|i| mlc_core::indicator_types::IndicatorGroupConfig {
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
                let configs_len = configs.len();
                let seeded = self.core.bubbles_mut().with_bubble_mut(group_id, |group| {
                    group.indicator_configs = configs;
                    group.primitives.len()
                });
                if let Some(prim_count) = seeded {
                    log::debug!(
                        "[BubbleStore] Seeded new tag {:?} with {} primitives, {} indicators",
                        group_id, prim_count, configs_len
                    );
                }
                }
                return;
            }

            // EXISTING GROUP: auto_bubble holds home drawings untouched; the
            // window's `current_tag_id` flip to `group_id` (below) is all
            // that's needed for it to read the group's own primitives.

            // When sync_viewport is ON for this group, stash the window's current
            // scale mode and apply the group mode. F (Focus) implies a pinned viewport,
            // so A/M/F must follow the group while viewport sync is active.
            let viewport_sync_on = self.core.bubbles().sync_flags(group_id)
                .map(|sf| sf.sync_viewport)
                .unwrap_or(false);
            if viewport_sync_on {
                let group_mode = self.core.bubbles().scale_mode(group_id);
                // Audit D8: `current_tag_id` hasn't flipped to `group_id` yet
                // (happens further below) but the window is about to render
                // the GROUP's data, so resolve the active series against
                // `group_id`'s bubble (not the not-yet-updated `current_tag_id`).
                let joining_chart_type = self.panel_app.panel_grid.container_id_for_leaf(joining_leaf)
                    .and_then(|cid| self.chart_type_for_chart(cid))
                    .unwrap_or("candles");
                let joining_series_owned = self.with_bubble(group_id, |b| {
                    crate::active_series::resolve_active_series_owned(joining_chart_type, b)
                }).unwrap_or_default();
                if let (Some(group_mode), Some(window)) = (
                    group_mode,
                    self.panel_app.panel_grid.container_for_leaf_mut(joining_leaf),
                ) {
                    if window.stashed_scale_mode.is_none() {
                        window.stashed_scale_mode = Some(window.price_scale().scale_mode);
                    }
                    window.set_scale_mode_with_recalc(group_mode, joining_series_owned.as_active_series());
                }
            }

            // If the existing color group's `bars` is empty (cold-load preset
            // path that hasn't yet seen a BarsLoaded for this instrument), seed
            // it from the joining chart's auto_bubble so the chart paints
            // immediately under the group view.  An auto-cure request below
            // refreshes the data authoritatively once REST returns.
            let existing_bars_empty = self.with_bubble(group_id, |g| g.bars.is_empty()).unwrap_or(true);
            if existing_bars_empty {
                let auto_bid_existing = self.panel_app.panel_grid.containers().get(&chart_id)
                    .map(|w| w.auto_tag_id);
                let auto_bars: Vec<mlc_core::Bar> = auto_bid_existing
                    .and_then(|bid| self.with_bubble(bid, |b| b.bars.clone()))
                    .unwrap_or_default();
                if !auto_bars.is_empty() {
                    let n = auto_bars.len();
                    self.core.bubbles_mut().set_bars(group_id, auto_bars);
                    sglog!("[sync_join_color_group] existing-group {:?} was empty; seeded {} bars from auto_bubble", group_id, n);
                }
            }

            // Auto-cure for existing-group join too — same rationale as new-group.
            {
                let cure_eid = self.exchange_for_chart(chart_id)
                    .and_then(|e| digdigdig3::ExchangeId::from_str(&e));
                let cure_sym = self.symbol_for_chart(chart_id).map(String::from).unwrap_or_default();
                let cure_at_label = self.account_type_for_chart(chart_id).map(|at| at.to_string()).unwrap_or_else(|| "S".to_string());
                let cure_tf = self.timeframe_for_chart(chart_id).unwrap_or_else(mlc_core::state::Timeframe::h1);
                if let Some(eid) = cure_eid {
                    if !cure_sym.is_empty()
                        && self.sidebar_state.connector_enabled.get(eid.as_str()).copied().unwrap_or(true)
                    {
                        let at = crate::account_type_from_label(&cure_at_label);
                        self.pending_cure_after_preset_load.push((eid, cure_sym.clone(), cure_tf, at, true));
                        sglog!("[sync_join_color_group] enqueued cure for existing group {:?} {} (force=true)", eid, cure_sym);
                    }
                }
            }

            // Get auto_tag_id before taking mutable window borrow.
            let auto_bid_existing = self.panel_app.panel_grid
                .containers()
                .get(&chart_id)
                .map(|w| w.auto_tag_id);

            // Stash command_history and pre_tag_ids into auto_bubble.
            //
            // RE-TAG: same rationale as the new-group branch above — the
            // home stash already lives in auto_bubble from the very first
            // tag entry; preserve it and do not overwrite command_history.
            if is_retag {
                if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(joining_leaf) {
                    window.current_tag_id = group_id;
                    window.tag_inherits_home_overlay = false;
                }
            } else if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(joining_leaf) {
                let old_history = std::mem::replace(
                    &mut window.command_history,
                    mlc_core::CommandHistory::new(250),
                );
                if let Some(bid) = auto_bid_existing {
                    let pre_tag_len = pre_tag_ids.len();
                    let stashed = self.core.bubbles_mut().with_bubble_mut(bid, |auto_bub| {
                        auto_bub.command_history = old_history;
                        auto_bub.pre_tag_indicator_ids = pre_tag_ids.clone();
                    }).is_some();
                    if stashed {
                        log::debug!(
                            "[BubbleStore] Stashed command_history and {} pre_tag_ids into auto-bubble {:?}",
                            pre_tag_len, bid
                        );
                    }
                }
                // Update current_tag_id to the existing color group.
                window.current_tag_id = group_id;
                window.tag_inherits_home_overlay = true;
            }

            // First tag entry into an EXISTING group: CLONE the chart's
            // home indicators into the group bubble.  Same model as the
            // new-group branch above — home stays intact, tag gets copies.
            if !is_retag {
                let clones = {
                    let im = self.core.indicators();
                    pre_tag_ids.iter().filter_map(|iid| im.full_clone(*iid)).collect::<Vec<_>>()
                };
                for src in clones {
                    let mut config = mlc_backend::IndicatorFullConfig::from(&src);
                    config.origin_id = Some(src.id);
                    let mut im = self.core.indicators_mut();
                    if let Some(new_id) = im.add(Some(group_id), &src.type_id) {
                        im.apply_full_config(new_id, config);
                    }
                }
            }

            // Sync tag indicators to joining window — clones any extra
            // indicator_configs the group already had from other peers
            // (e.g. peer chart added something while this chart was
            // untagged).  Under bubble ownership the join doesn't need a
            // per-chart clone, but keep the call so config-driven seeding
            // still runs for any group that pre-existed without instances.
            let leaf_chart_ids = vec![(joining_leaf, chart_id)];
            self.sync_group_indicators_to_new_members(group_id, &leaf_chart_ids);
            self.sync_sub_panes_from_manager();
            self.sidebar_data_dirty = true;
            log::debug!("[BubbleStore] Join existing group: cleared window drawings, synced tag content");
        }

        // Dead fallback removed — TagManager handles all sync join logic above.
    }

    /// Split the active leaf: snapshot its color tag before split (the old
    /// LeafId is destroyed by `split_leaf`), then assign inherited or new color
    /// to all resulting leaves and propagate crosshair/viewport.
    pub(super) fn do_split(&mut self, kind: mlc_core::SplitKind) {
        sglog!(
            "[do_split] ENTER kind={:?} split_without_group={}",
            kind, self.split_without_group
        );
        // Snapshot color, group_id, and chart_id BEFORE split — active_leaf will be destroyed.
        let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
        let pre_split_color = active_leaf
            .and_then(|lid| self.panel_app.leaf_color_tags.remove(&lid));
        let pre_split_group_id = active_leaf
            .and_then(|lid| self.panel_app.panel_grid.container_id_for_leaf(lid))
            .and_then(|cid| self.bubble_for_chart(cid));
        let pre_split_chart_id = active_leaf
            .and_then(|lid| self.panel_app.panel_grid.container_id_for_leaf(lid));

        // Phase X0 / C5c regression fix: snapshot the bars from the source
        // bubble BEFORE the split.  Pre-migration, `clone_for_split_shell`
        // physically copied `window.bars` into the new ChartContainer so the
        // sibling started life with bars already mounted.  After the bubble
        // ownership refactor bars live on `DataBubble.bars`; the split creates
        // brand-new bubbles initialised with `bars: Vec::new()` and the source
        // bars never make it across.  Renderer reads
        // `bars_for_chart(new_cid)` -> `bubble.bars.clone()` -> empty vec ->
        // blank chart.  Snapshot here; seed below for every new bubble
        // (auto-mono fix-up loop + the shared `group_id` path).
        //
        // Read into an owned Vec inside its own scope so the RwLock read
        // guard drops before any subsequent bubble_mut() — on wasm32
        // std::sync::RwLock is single-threaded and aborts on any
        // overlap between read and write acquisitions.
        let source_bars: Vec<mlc_core::Bar> = {
            let bm = self.core.bubbles();
            pre_split_group_id
                .map(|bid| bm.bars(bid).to_vec())
                .unwrap_or_default()
        };

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
                        .map(|w| w.auto_tag_id == mlc_core::ids::BubbleId(0))
                        .unwrap_or(false)
                })
                .collect();
            for cid in needs_auto {
                let sym = self.symbol_for_chart(cid)
                    .map(String::from)
                    .unwrap_or_default();
                let tf = self.timeframe_for_chart(cid)
                    .unwrap_or_else(mlc_core::state::Timeframe::h1);
                let auto_bid = self.create_auto_bubble(
                    [0.0, 0.0, 0.0, 0.0], sym, tf,
                );
                // Seed bars from the source bubble so the new chart paints
                // immediately instead of staying blank until the next live
                // tick arrives.  Each bubble lock acquisition is scoped to its
                // own block — on wasm32 std::sync::RwLock is single-threaded
                // and aborts on any overlap, even between two consecutive
                // bubble_mut() calls if the first guard's drop is delayed past
                // the start of the second call (which rustc can do when the
                // first lives inside a method-chain temporary).
                if !source_bars.is_empty() {
                    self.core.bubbles_mut().set_bars(auto_bid, source_bars.clone());
                }
                {
                    if let Some(window) = self.panel_app.panel_grid.containers_mut().get_mut(&cid) {
                        window.auto_tag_id = auto_bid;
                        window.current_tag_id = auto_bid;
                    }
                }
                self.attach_chart_to_tag(cid, auto_bid);
                log::debug!("[BubbleStore] do_split: created auto-bubble {:?} for chart {:?} (seeded {} bars)",
                    auto_bid, cid, source_bars.len());
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
            .and_then(|cid| self.symbol_for_chart(cid))
            .map(String::from)
            .unwrap_or_default();
        let source_tf = pre_split_chart_id
            .and_then(|cid| self.timeframe_for_chart(cid))
            .unwrap_or_else(mlc_core::state::Timeframe::h1);
        let source_exch = pre_split_chart_id
            .and_then(|cid| self.exchange_for_chart(cid))
            .map(String::from)
            .unwrap_or_default();
        let source_at = pre_split_chart_id
            .and_then(|cid| self.account_type_for_chart(cid))
            .map(|at| at.to_string())
            .unwrap_or_else(|| "S".to_string());

        sglog!(
            "[do_split-key] source pre_split_cid={:?} sym='{}' exch='{}' at='{}' tf={:?} (will seed {} new chart_ids)",
            pre_split_chart_id, source_sym, source_exch, source_at, source_tf, new_chart_ids.len()
        );
        for chart_id in new_chart_ids {
            let pre_bid = self.bubble_for_chart(chart_id);
            if let Some(bid) = pre_bid {
                self.core.bubbles_mut().set_bid_instrument_full(
                    bid,
                    source_sym.clone(),
                    source_exch.clone(),
                    source_at.clone(),
                    source_tf.clone(),
                );
                sglog!(
                    "[do_split-key] cid={:?} bid={:?} <- sym='{}' exch='{}' at='{}'",
                    chart_id, bid, source_sym, source_exch, source_at
                );
            } else {
                sglog!(
                    "[do_split-key] cid={:?} NOT ATTACHED to any bubble — skipping seed (this chart will render with blank key)",
                    chart_id
                );
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
                let _panel = crate::chart_panel_bridge::ensure_chart_panel_state(
                    &mut self.panels_store,
                    pid,
                );
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
                            self.attach_chart_to_tag(new_cid, old_gid);
                            log::debug!("[BubbleStore] (SplitUntagged) Mother re-connected new cid {:?} → group {:?}", new_cid, old_gid);
                        }
                        // else: same chart_id survived the split, TagManager link is intact.
                    }
                    // BubbleStore reverse index maintained by attach_chart_to_bubble above.
                    // Restore the mother's color tag that was snapshotted before the split.
                    if let Some(color) = pre_split_color {
                        self.panel_app.leaf_color_tags.insert(mother_leaf, color);
                    }
                    log::debug!("[BubbleStore] (SplitUntagged) Mother leaf {:?} keeps group {:?}", mother_leaf, old_gid);
                }
            }

            // Give each NEW (non-mother) leaf its own fresh auto_created group.
            // Read the instrument from `pre_split_chart_id` (the source) — at
            // this point the new leaves are not yet attached to any bubble so
            // `symbol_for_chart(new_cid)` would always return `""`, and the
            // auto_bubble gets seeded with a blank instrument key.  Owner repro
            // 2026-06-25: split (auto-tag) → untag → tag-in-new-color landed in
            // a group with empty symbol/exchange/account_type because untag
            // re-attaches to the auto_bubble (whose seed was blank) and the
            // subsequent join reads identity from there.
            let split_untagged_sym = pre_split_chart_id
                .and_then(|c| self.symbol_for_chart(c))
                .map(String::from)
                .unwrap_or_default();
            let split_untagged_tf = pre_split_chart_id
                .and_then(|c| self.timeframe_for_chart(c))
                .unwrap_or_else(mlc_core::state::Timeframe::h1);
            let split_untagged_exch = pre_split_chart_id
                .and_then(|c| self.exchange_for_chart(c))
                .map(String::from)
                .unwrap_or_default();
            let split_untagged_at = pre_split_chart_id
                .and_then(|c| self.account_type_for_chart(c))
                .map(|at| at.to_string())
                .unwrap_or_else(|| "S".to_string());

            for &leaf_id in new_leaves.iter().skip(1) {
                let new_chart_id = self.panel_app.panel_grid.container_id_for_leaf(leaf_id);
                if self.panel_app.panel_grid.container_for_leaf(leaf_id).is_none() {
                    log::debug!("[sync_group] split untagged: container_for_leaf({:?}) returned None — skipping new leaf", leaf_id);
                    continue;
                }
                let gid = self.create_auto_bubble(
                    [0.0, 0.0, 0.0, 0.0],
                    split_untagged_sym.clone(),
                    split_untagged_tf.clone(),
                );
                // Complete the identity record (create_bubble_auto only takes
                // symbol + timeframe; exchange + account_type stay blank
                // unless filled here).  Without these the post-untag retag
                // flow seeds the new color group with an empty exchange key,
                // which then breaks bridge routing and panel sym:exch:AT keys.
                self.core.bubbles_mut().set_bubble_instrument(gid, split_untagged_exch.clone(), split_untagged_at.clone());
                if let Some(chart_id) = new_chart_id {
                    self.attach_chart_to_tag(chart_id, gid);
                }
                // Seed bars from the source bubble so the new untagged sibling
                // paints immediately instead of waiting for a live tick.
                if !source_bars.is_empty() {
                    self.core.bubbles_mut().set_bars(gid, source_bars.clone());
                }
                // BubbleStore reverse index updated by attach_chart_to_bubble above.
                self.panel_app.leaf_color_tags.remove(&leaf_id);
                log::debug!(
                    "[BubbleStore] (SplitUntagged) Created auto group {:?} for new leaf {:?} sym='{}' exch='{}' at='{}' bars={}",
                    gid, leaf_id, split_untagged_sym, split_untagged_exch, split_untagged_at, source_bars.len()
                );
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
                .and_then(|gid| self.core.tags().get(gid).map(|t| t.auto_created))
                .unwrap_or(false);

            // Disconnect the old chart from its group (leaf destroyed by split).
            if let (Some(old_cid), Some(old_gid)) = (pre_split_chart_id, pre_split_group_id) {
                // Whether `old_gid` is the chart's permanent HOME auto_bubble.
                // Read BEFORE detach so the auto_tag_id is still resolvable
                // via window.auto_tag_id (which persists across detach).
                let old_cid_auto_home = self.panel_app.panel_grid.containers().get(&old_cid)
                    .map(|w| w.auto_tag_id);
                let _ = self.detach_chart_from_tag(old_cid);
                sglog!("[BubbleStore] Disconnected destroyed chart {:?} from group {:?} (home auto_bid={:?})", old_cid, old_gid, old_cid_auto_home);
                // Bug C fix: remove the old group if it became empty and was
                // auto-created.  Scoped to keep wasm32 RwLock contract.
                // NEVER remove the chart's permanent home auto_bubble — see
                // `disconnect_from_current_group` for full reasoning.
                let should_remove = self.core.tags().get(old_gid)
                    .map(|t| t.auto_created && t.members.is_empty())
                    .unwrap_or(false)
                    && old_cid_auto_home != Some(old_gid);
                if should_remove {
                    self.remove_tag(old_gid);
                    sglog!("[BubbleStore] Removed empty auto group {:?} after split", old_gid);
                } else if old_cid_auto_home == Some(old_gid) {
                    sglog!("[BubbleStore] PRESERVED home auto_bubble {:?} after split-detach", old_gid);
                }
            }

            // Resolve first-leaf window once — needed by both auto-promote and no-group branches.
            let first_leaf_win = self.panel_app.panel_grid.container_for_leaf(new_leaves[0]);
            if first_leaf_win.is_none() {
                log::debug!("[sync_group] split tagged: container_for_leaf({:?}) returned None — skipping split tag", new_leaves[0]);
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
                    let color = self.core.tags().next_unused_color();
                    let gid = self.create_tagged_bubble(color, first_leaf_symbol, first_leaf_timeframe);
                    created_fresh_group = true;
                    log::debug!("[BubbleStore] Split promoted auto group to visible tag {:?}", gid);
                    gid
                } else {
                    // User-tagged group: all split windows intentionally share it.
                    existing_group
                }
            } else {
                // No existing group — pick color from TagManager's palette.
                let color = pre_split_color
                    .unwrap_or_else(|| self.core.tags().next_unused_color());
                created_fresh_group = true;
                self.create_tagged_bubble(color, first_leaf_symbol, first_leaf_timeframe)
            };

            // create_bubble leaves exchange/account_type empty — complete the
            // identity record from the source instrument for groups born here.
            // (Existing user-tagged groups keep their own identity untouched.)
            // Scoped block ensures the write guard drops before the next read
            // below at line ~784 — see the wasm32 RwLock note in the auto-mono
            // loop above for why this matters.
            if created_fresh_group {
                // KEY-INTEGRITY (series-leak fix 2026-07-17): complete the
                // fresh group's identity through the resetting setter, not
                // raw partial field writes — exchange+AT here, symbol was
                // already set by `create_tagged_bubble`. On a fresh bubble
                // the reset is a no-op; the point is that NO code path
                // writes `address.*` fields directly.
                self.core.bubbles_mut().set_bubble_instrument(
                    group_id, source_exch.clone(), source_at.clone(),
                );
                self.core.bubbles_mut().with_bubble_mut(group_id, |bubble| {
                    // Phase X0 / C5c regression fix: seed bars from the source
                    // bubble snapshot taken at the top of do_split.  Without
                    // this, every new sibling chart renders blank until the
                    // next live tick because `bars_for_chart` reads
                    // `bubble.bars` which `create_bubble` initialises empty.
                    if !source_bars.is_empty() && bubble.bars.is_empty() {
                        bubble.bars = source_bars.clone();
                    }
                });
            }

            // --- Shared group path (user-tagged groups or brand-new groups) ---

            // Assign the group's color to leaf_color_tags for UI display.
            let group_color = self.core.tags().get(group_id)
                .map(|t| t.color)
                .unwrap_or([0.5, 0.5, 0.5, 0.9]);
            for &leaf_id in &new_leaves {
                self.panel_app.leaf_color_tags.insert(leaf_id, group_color);
            }
            // Determine if this is a truly new group (just created, has no state yet).
            let group_has_no_members = self.core.tags().get(group_id)
                .map(|t| t.members.is_empty())
                .unwrap_or(true);
            let is_new_group = group_has_no_members
                && self.with_bubble(group_id, |g| g.indicator_configs.is_empty() && g.primitives.is_empty())
                    .unwrap_or(true);
            // Collect (leaf_id, chart_id) first to avoid mixed borrow when setting group_id.
            let leaf_chart_ids: Vec<(mlc_core::LeafId, mlc_core::ChartId)> = new_leaves
                .iter()
                .filter_map(|&leaf_id| {
                    self.panel_app.panel_grid.container_id_for_leaf(leaf_id)
                        .map(|cid| (leaf_id, cid))
                })
                .collect();
            for &(_, chart_id) in &leaf_chart_ids {
                self.attach_chart_to_tag(chart_id, group_id);
            }
            // Point every leaf's `window.current_tag_id` at the shared
            // group bubble.  Without this, sibling leaves born by the split
            // keep their freshly-created auto_bubble as `current_tag_id`
            // while `BubbleStore.bubble_for_chart` (= reverse map) returns
            // group_id — the two paths get out of sync and the renderer +
            // sub_pane geometry read the wrong bucket, so the sibling shows
            // no group indicators / drawings while the Object Tree shows
            // them (Object Tree uses bubble_for_chart, canvas uses
            // window.current_tag_id via the atomic).
            for &(leaf_id, _) in &leaf_chart_ids {
                if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                    window.current_tag_id = group_id;
                }
            }
            for &(leaf_id, _) in &leaf_chart_ids {
                if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                    window.tag_inherits_home_overlay = true;
                }
            }
            // BubbleStore reverse index updated by attach_chart_to_bubble loop above.

            // For new groups: snapshot source window's indicator ids and command_history
            // into its auto_bubble so desync can restore them.
            if is_new_group {
                if let Some(&(source_leaf, source_cid)) = leaf_chart_ids.first() {
                    let source_auto_bid_raw = self.panel_app.panel_grid.containers()
                        .get(&source_cid)
                        .map(|w| w.auto_tag_id);
                    let pre_tag_ids: Vec<u64> = {
                        let indicators_guard = self.core.indicators();
                        source_auto_bid_raw
                            .map(|bid| indicators_guard.instances_for_bubble(bid))
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|i| i.origin_id.is_none())
                            .map(|i| i.id)
                            .collect()
                    };

                    // Read auto_tag_id before taking mutable window borrow.
                    let source_auto_bid = self.panel_app.panel_grid
                        .containers()
                        .get(&source_cid)
                        .map(|w| w.auto_tag_id);

                    if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(source_leaf) {
                        // Stash command_history into auto_bubble; window gets a fresh one.
                        let old_history = std::mem::replace(
                            &mut window.command_history,
                            mlc_core::CommandHistory::new(250),
                        );
                        if let Some(bid) = source_auto_bid {
                            self.core.bubbles_mut().with_bubble_mut(bid, |auto_bub| {
                                auto_bub.command_history = old_history;
                                auto_bub.pre_tag_indicator_ids = pre_tag_ids.clone();
                            });
                        }
                        // Update current_tag_id to the new group.
                        window.current_tag_id = group_id;
                    }
                    // auto_bubble.primitives already holds the source's home drawings —
                    // no explicit stash needed.

                    // Clone source's home indicators into the new tag bubble.
                    // Owner UX: split-from-untagged creates a new tag bubble
                    // whose contents are a clone of the source auto-bubble.
                    let clones = {
                        let im = self.core.indicators();
                        pre_tag_ids.iter().filter_map(|iid| im.full_clone(*iid)).collect::<Vec<_>>()
                    };
                    for src in clones {
                        let mut config = mlc_backend::IndicatorFullConfig::from(&src);
                        config.origin_id = Some(src.id);
                        let mut im = self.core.indicators_mut();
                        if let Some(new_id) = im.add(Some(group_id), &src.type_id) {
                            im.apply_full_config(new_id, config);
                        }
                    }
                }
            }
            log::debug!(
                "[BubbleStore] {} {:?} with {} members after split",
                if is_new_group { "Created group" } else { "Joined existing group" },
                group_id,
                new_leaves.len()
            );

            // NOTE (Step 3.1): no legacy-cloned primitives to purge from
            // non-source windows any more — every leaf in `leaf_chart_ids`
            // now has `current_tag_id == group_id` (set above), so they
            // all read `group.primitives` directly.

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
                    let sym = cid.and_then(|c| self.symbol_for_chart(c))
                        .map(String::from)
                        .unwrap_or_default();
                    let exch = cid.and_then(|c| self.exchange_for_chart(c))
                        .map(String::from)
                        .unwrap_or_default();
                    let at = cid.and_then(|c| self.account_type_for_chart(c))
                        .map(|at| at.to_string())
                        .unwrap_or_else(|| "S".to_string());
                    (sym, exch, at)
                };
                // NOTE (Step 3.1): the source window's `current_tag_id`
                // was already flipped to `group_id` in the block above —
                // read from its home `auto_tag_id` instead, since that's
                // where the "window's current drawings" actually still live.
                let seed_auto_bid = first_leaf_chart_id
                    .and_then(|cid| self.panel_app.panel_grid.containers().get(&cid))
                    .map(|w| w.auto_tag_id);
                let mut source_prims: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>> =
                    seed_auto_bid
                        .map(|bid| self.core.drawings().primitives(bid).iter()
                            .filter(|p| p.data().origin_id.is_none())
                            .map(|p| p.clone_box())
                            .collect::<Vec<_>>())
                        .unwrap_or_default();
                for p in &mut source_prims {
                    let d = p.data_mut();
                    d.symbol = prim_sym.clone();
                    d.exchange = prim_exch.clone();
                    d.account_type = prim_at.clone();
                }
                // Re-ID the group copies so they are globally unique. The
                // source window's drawing_manager keeps the original IDs. A
                // batch remap (not a per-primitive independent restamp) so
                // any Elliott subdivision/chain inside this copy still
                // points at the COPY's own new ids instead of the source
                // window's — see `reassign_primitive_ids_remapped`'s own doc
                // (2026-08-14 report, WaveLink cold-restore audit).
                mlc_core::drawing::reassign_primitive_ids_remapped(&mut source_prims);
                if !source_prims.is_empty() {
                    let count = source_prims.len();
                    let seeded = self.core.bubbles_mut().with_bubble_mut(group_id, |group| {
                        group.primitives = source_prims;
                    }).is_some();
                    if seeded {
                        log::debug!(
                            "[BubbleStore] Seeded group {:?} with {} primitives",
                            group_id, count
                        );
                    }
                }

                // Indicators: snapshot source window's indicators into group configs.
                if let Some(source_chart_id) = leaf_chart_ids.first().map(|(_, cid)| *cid) {
                    // B1 S2.7.1d: read indicator seed key from branch_meta.
                    let (source_symbol, source_exchange, source_account_type) = {
                        let cid = first_leaf_chart_id;
                        let sym = cid.and_then(|c| self.symbol_for_chart(c))
                            .map(String::from)
                            .unwrap_or_default();
                        let exch = cid.and_then(|c| self.exchange_for_chart(c))
                            .map(String::from)
                            .unwrap_or_default();
                        let at = cid.and_then(|c| self.account_type_for_chart(c))
                            .map(|at| at.to_string())
                            .unwrap_or_else(|| "S".to_string());
                        (sym, exch, at)
                    };
                    let source_chart_auto_bid_raw = self.panel_app.panel_grid.containers()
                        .get(&source_chart_id)
                        .map(|w| w.auto_tag_id);
                    let indicators_guard_split = self.core.indicators();
                    let mut configs: Vec<mlc_core::indicator_types::IndicatorGroupConfig> = source_chart_auto_bid_raw
                        .map(|bid| indicators_guard_split.instances_for_bubble(bid))
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|i| i.origin_id.is_none())
                        .map(|i| mlc_core::indicator_types::IndicatorGroupConfig {
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
                    let count = configs.len();
                    let seeded = self.core.bubbles_mut().with_bubble_mut(group_id, |group| {
                        group.indicator_configs = configs;
                    }).is_some();
                    if seeded {
                        log::debug!(
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

        // Auto-cure: every chart born by this split needs a fresh REST sweep
        // so its bubble's `bars` is repopulated from authoritative data (the
        // source_bars seed at the top of `do_split` is best-effort — it gets
        // the chart painting immediately, but doesn't guarantee the bars are
        // complete or current).  Defer via `pending_cure_after_preset_load`
        // so the request goes out AFTER the next frame paints (skeleton +
        // spinner while the request lands instead of freezing the click).
        // Mirrors the LoadPreset warm-path cure pattern.
        let cure_chart_ids: Vec<mlc_core::state::ChartId> = new_leaves
            .iter()
            .filter_map(|&lid| self.panel_app.panel_grid.container_id_for_leaf(lid))
            .collect();
        for cid in &cure_chart_ids {
            let sym = self.symbol_for_chart(*cid).map(String::from).unwrap_or_default();
            let exch = self.exchange_for_chart(*cid).map(String::from).unwrap_or_default();
            let at_label = self.account_type_for_chart(*cid).map(|at| at.to_string()).unwrap_or_else(|| "S".to_string());
            let tf = self.timeframe_for_chart(*cid).unwrap_or_else(mlc_core::state::Timeframe::h1);
            if sym.is_empty() || exch.is_empty() { continue; }
            let eid = match digdigdig3::ExchangeId::from_str(&exch) {
                Some(e) => e,
                None => continue,
            };
            if !self.sidebar_state.connector_enabled.get(eid.as_str()).copied().unwrap_or(true) { continue; }
            let at = crate::account_type_from_label(&at_label);
            let has_bars = !self.bars_for_chart(*cid).is_empty();
            let force = !has_bars;
            self.pending_cure_after_preset_load.push((eid, sym, tf, at, force));
        }
        sglog!("[do_split] enqueued cure for {} new chart_ids", cure_chart_ids.len());
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
                        w.viewport().view_start,
                        w.viewport().bar_spacing,
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
            .and_then(|cid| self.bubble_for_chart(cid))
            .and_then(|gid| self.with_bubble(gid, |g| g.sync_flags.sync_symbol))
            .unwrap_or(true);
        if !should_sync { return; }

        // Enumerate peer leaves via TagManager group membership (not leaf_color_tags).
        // This makes propagation work for untagged (auto-created) groups too.
        let source_chart_id_for_peers = self.panel_app.panel_grid.container_id_for_leaf(source_leaf);
        let group_id_for_peers = source_chart_id_for_peers
            .and_then(|cid| self.bubble_for_chart(cid));
        let sync_leaves: Vec<mlc_core::LeafId> = if let Some(gid) = group_id_for_peers {
            self.core.tags()
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
        let new_address = mlc_core::instrument::InstrumentAddress::new(
            exchange_owned.clone(),
            symbol_owned.clone(),
            account_type_owned.parse().unwrap_or_default(),
        );

        // Collect (old_symbol, old_exchange, old_account_type, timeframe) for each peer BEFORE
        // mutating windows so we can restore drawings and request bars after the mutations.
        // B1 S2.7.1d: read from branch_meta, fall back to struct field.
        let mut peer_info: Vec<(String, String, String, mlc_core::state::Timeframe)> = Vec::new();
        for leaf_id in &sync_leaves {
            let cid = self.panel_app.panel_grid.container_id_for_leaf(*leaf_id);
            let sym = cid.and_then(|c| self.symbol_for_chart(c))
                .map(String::from)
                .unwrap_or_default();
            let exch = cid.and_then(|c| self.exchange_for_chart(c))
                .map(String::from)
                .unwrap_or_default();
            let at = cid.and_then(|c| self.account_type_for_chart(c))
                .map(|at| at.to_string())
                .unwrap_or_else(|| "S".to_string());
            let tf = cid.and_then(|c| self.timeframe_for_chart(c))
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

        // Apply the full instrument key change (symbol + exchange + account_type).
        //
        // NOTE (Step 3.1): under the per-bubble drawing model, sync leaves
        // that share a tag also share `current_tag_id` — calling
        // `swap_symbol` once per leaf would clear + restore the SAME
        // bubble's primitives N times (idempotent-redundant at best, a
        // double-restore risk at worst). Deduplicate by bid: call
        // `swap_symbol` once per distinct bubble, then run the per-leaf
        // viewport reset (genuinely per-window state) for every leaf.
        let mut swapped_bids: std::collections::HashSet<mlc_core::ids::BubbleId> = std::collections::HashSet::new();
        for (leaf_id, (old_symbol, old_exchange, old_account_type, _)) in sync_leaves.iter().zip(peer_info.iter()) {
            let bid_opt = self.panel_app.panel_grid.container_for_leaf(*leaf_id).map(|w| w.current_tag_id);
            if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(*leaf_id) {
                if let Some(bid) = bid_opt {
                    if swapped_bids.insert(bid) {
                        let restore_snaps = window.take_drawings_snapshot_for_symbol(&symbol_owned, &exchange_owned, &account_type_owned);
                        let result = self.core.drawings_mut().swap_symbol(
                            bid, &new_address, restore_snaps.as_deref(),
                        );
                        let old_key = format!("{}:{}:{}", old_symbol, old_exchange, old_account_type);
                        if result.old_primitives.is_empty() {
                            window.symbol_drawings.remove(&old_key);
                        } else {
                            window.symbol_drawings.insert(old_key, result.old_primitives);
                        }
                    }
                }
                window.begin_fresh_load();
            }
        }

        for ((_, chart_id_opt), (_, _, _, peer_tf)) in peer_chart_ids.iter().zip(peer_info.iter()) {
            if let Some(chart_id) = chart_id_opt {
                if let Some(bid) = self.bubble_for_chart(*chart_id) {
                    self.core.bubbles_mut().set_bid_instrument_full(
                        bid,
                        symbol_owned.clone(),
                        exchange_owned.clone(),
                        account_type_owned.clone(),
                        peer_tf.clone(),
                    );
                }
            }
        }

        // Request fresh bars for each peer via the bridge using the new exchange + account_type.
        let bar_count = self.core.user().profile().domain.bar_count as usize;
        for (_, _, _, timeframe) in peer_info {
            if symbol_owned.is_empty() {
                continue;
            }
            let resolved_exchange = self.core.exchange_symbols()
                .find_by_label(&exchange_owned)
                .unwrap_or(self.active_exchange);
            let eid_str = resolved_exchange.as_str();
            if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                log::debug!(
                    "[ChartApp] Exchange {} is disabled, skipping request_bars (symbol propagation)",
                    eid_str
                );
                continue;
            }
            let at = crate::account_type_from_label(&account_type_owned);
            self.core.ensure_connector(resolved_exchange);
            // force=true — a sync-propagated symbol switch is the same
            // explicit user action as ApplySymbol's own request: dedup-L2
            // silently skips non-force requests when a station kline
            // subscriber for this key is still alive, leaving the peer
            // chart empty after the identity reset (see the matching
            // comment in chart_out_events.rs's ApplySymbol handler).
            self.core.request_kline(resolved_exchange, &symbol_owned, &timeframe, at, None, Some(bar_count), true);
            log::debug!(
                "[BubbleStore] Requested bars for peer {} @ {} tf={:?} (symbol propagation)",
                symbol_owned, eid_str, timeframe
            );
        }

        // Also update the TagManager group's canonical instrument so the group
        // state stays consistent with the displayed windows.
        if let Some(chart_id) = self.panel_app.panel_grid.container_id_for_leaf(source_leaf) {
            // KEY-INTEGRITY (series-leak fix 2026-07-17): no guessed key
            // parts — when exchange/AT can't be resolved from the source
            // chart, skip the group-key update instead of fabricating
            // ("" / "S") legs. A fabricated leg here flipped the group
            // bubble onto a WRONG instrument key without the layers ever
            // matching it (dead routing + leak surface).
            let source_exchange = self.exchange_for_chart(chart_id).map(String::from);
            let source_account_type = self.account_type_for_chart(chart_id).map(|at| at.to_string());

            let group_id = self.panel_app.panel_grid.container_id_for_leaf(source_leaf)
                .and_then(|cid| self.bubble_for_chart(cid));

            if let (Some(bid), Some(source_exchange), Some(source_account_type)) =
                (group_id, source_exchange, source_account_type)
            {
                if !source_exchange.is_empty() {
                    self.core.bubbles_mut().set_instrument(
                        bid,
                        symbol.to_string(),
                        source_exchange,
                        source_account_type,
                    );
                }
            }
            log::debug!(
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
            .and_then(|cid| self.bubble_for_chart(cid))
            .and_then(|gid| self.with_bubble(gid, |g| g.sync_flags.sync_timeframe))
            .unwrap_or(true);
        if !should_sync { return; }

        // Enumerate peer leaves via TagManager group membership (not leaf_color_tags).
        let source_chart_id_for_tf = self.panel_app.panel_grid.container_id_for_leaf(source_leaf);
        let group_id_for_tf = source_chart_id_for_tf
            .and_then(|cid| self.bubble_for_chart(cid));
        let sync_leaves: Vec<mlc_core::LeafId> = if let Some(gid) = group_id_for_tf {
            self.core.tags()
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
            let exch = cid.and_then(|c| self.exchange_for_chart(c))
                .map(String::from)
                .unwrap_or_default();
            let sym = cid.and_then(|c| self.symbol_for_chart(c))
                .map(String::from)
                .unwrap_or_default();
            let at = cid.and_then(|c| self.account_type_for_chart(c))
                .map(|at| at.to_string())
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
                // Deliberately does NOT set pending_symbol_load (unlike
                // begin_fresh_load / W31's symbol propagation): the peer TF
                // switch relies on the subsequent BarsLoaded matching by
                // instrument only, not the fresh-vs-backfill branch that
                // pending_symbol_load gates.
                window.begin_fresh_load_keep_pending();
            }
        }

        for (_, chart_id_opt) in &peer_chart_ids {
            if let Some(chart_id) = chart_id_opt {
                if let Some(bid) = self.bubble_for_chart(*chart_id) {
                    self.core.bubbles_mut().with_bubble_mut(bid, |bubble| bubble.timeframe = tf.clone());
                }
            }
        }

        // Request fresh bars for each peer via the bridge.
        let bar_count = self.core.user().profile().domain.bar_count as usize;
        for (exchange_str, symbol, at_label) in peer_requests {
            if symbol.is_empty() {
                continue;
            }
            let resolved_exchange = self.core.exchange_symbols()
                .find_by_label(&exchange_str)
                .unwrap_or(self.active_exchange);
            let eid_str = resolved_exchange.as_str();
            if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                log::debug!(
                    "[ChartApp] Exchange {} is disabled, skipping request_bars (timeframe propagation)",
                    eid_str
                );
                continue;
            }
            let at = crate::account_type_from_label(&at_label);
            self.core.ensure_connector(resolved_exchange);
            self.core.request_kline(resolved_exchange, &symbol, &tf, at, None, Some(bar_count), true);
            log::debug!(
                "[BubbleStore] Requested bars for peer {} @ {} tf={:?} (timeframe propagation)",
                symbol, eid_str, tf
            );
        }

        // Also update the TagManager group's canonical timeframe so the group
        // state stays consistent with the displayed windows.
        if let Some(chart_id) = self.panel_app.panel_grid.container_id_for_leaf(source_leaf) {
            // Bound first so the `tags()` read guard drops before the
            // `bubbles_mut()` write below (clippy::significant_drop_in_scrutinee).
            let tag_bid = self.core.tags().tag_for_chart(chart_id);
            if let Some(bid) = tag_bid {
                self.core.bubbles_mut().set_timeframe(bid, tf.clone());
            }
            log::debug!(
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
        // DEPRECATED (Step 3.1): this legacy clone-based path only ever ran
        // for the "ungrouped peers" case (it already early-returned once a
        // TagManager group existed). The manager methods it depended on
        // (`last_original_id`, `clone_primitive_for_sync`,
        // `add_synced_primitives`) were removed with the per-bubble
        // DrawingManager reset — grouped peers share one bubble's
        // `primitives` Vec directly, and ungrouped-peer clone propagation
        // was already the deprecated tail this doc comment describes.
        let _ = source_leaf;
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
            .and_then(|cid| self.bubble_for_chart(cid));

        let should_sync = group_id
            .and_then(|gid| self.with_bubble(gid, |g| g.sync_flags.sync_crosshair))
            .unwrap_or(true);
        if !should_sync { return; }

        // Propagate crosshair to peer chart windows in the same color-tag group.
        // Per-member override (`effective_sync_crosshair`) takes precedence over
        // the group-level flag — a peer with `sync_crosshair = Some(false)` must
        // not receive updates even when the group default is enabled.
        // Use TagManager membership so untagged groups propagate too.
        let sync_leaves: Vec<mlc_core::LeafId> = if let Some(gid) = group_id {
            self.core.tags()
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
                            self.core.tags().get(gid2).map(|t| {
                                t.effective_sync_crosshair(
                                    mlc_core::ids::SyncMemberId::Chart(cid.0),
                                    should_sync,
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
            let panel_ids = self.core.tags().panel_members(gid);
            let crosshair_price = if visible { Some(price) } else { None };
            for pid in panel_ids {
                let panel_id = mlc_app_sidebar_content::free_slot::PanelId(pid);
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

    /// DEAD (Step 3.1): grouped peer leaves already share one bubble's
    /// `BubbleDrawingSession` (the manager is keyed by `BubbleId`, not by
    /// panel), so the in-progress multipoint construction state this
    /// function used to clone into each peer's separate `DrawingManager` is
    /// visible to every peer automatically — there is no separate session
    /// to propagate into. The legacy ungrouped-peer fallback this function
    /// also handled was already the deprecated tail (see the doc comment
    /// history above this function before the reset).
    pub(super) fn propagate_drawing_state_to_sync_group(
        &mut self,
        _source_leaf: mlc_core::LeafId,
    ) {
    }

    /// Stamp the full instrument key (symbol, exchange, account_type) onto the
    /// most recently completed primitive on the active window's bubble.
    ///
    /// Called immediately after any drawing-completion event (DrawingClick,
    /// freehand drag-end, finish_multipoint). Under the per-bubble drawing
    /// model there is no separate per-window `DrawingManager` to move the
    /// primitive OUT of — the completed primitive already lives on
    /// `window.current_tag_id`'s `DataBubble.primitives`, which IS the
    /// group's authoritative list once the window is tagged. This just
    /// applies the instrument-key stamp that used to happen as part of the
    /// pop/push dance.
    ///
    /// Returns `true` when a primitive was found and stamped.
    pub(super) fn intercept_completed_primitive_to_group(&mut self) -> bool {
        let chart_id = match self.panel_app.panel_grid.active_container_id() {
            Some(id) => id,
            None => return false,
        };
        let bid = match self.bubble_for_chart(chart_id) {
            Some(bid) => bid,
            None => return false,
        };

        // If sync_drawings is OFF for this group, leave stamping to whatever
        // wrote the primitive in the first place — matches the previous
        // early-return semantics for consistency.
        let sync_drawings_on = self.with_bubble(bid, |g| g.sync_flags.sync_drawings).unwrap_or(true);
        let is_auto_mono = self.core.tags().get(bid)
            .map(|t| t.auto_created && t.members.len() <= 1)
            .unwrap_or(false);
        if !sync_drawings_on || is_auto_mono {
            return false;
        }

        let Some(prim_id) = self.core.drawings().last_index(bid) else { return false };
        let (prim_symbol, prim_exchange, prim_account_type) = {
            let sym = self.symbol_for_chart(chart_id)
                .map(String::from)
                .unwrap_or_default();
            let exch = self.exchange_for_chart(chart_id)
                .map(String::from)
                .unwrap_or_default();
            let at = self.account_type_for_chart(chart_id)
                .map(|at| at.to_string())
                .unwrap_or_else(|| "S".to_string());
            (sym, exch, at)
        };
        let Some(mut data) = self.core.drawings().get_data(bid, prim_id) else { return false };
        data.symbol = prim_symbol;
        data.exchange = prim_exchange;
        data.account_type = prim_account_type;
        self.core.drawings_mut().set_data(bid, prim_id, &data);
        true
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
        let old_gid = match self.bubble_for_chart(chart_id) {
            Some(gid) => gid,
            None => return,
        };
        // The permanent home auto_bubble for this chart — MUST survive any
        // disconnect/cleanup, otherwise the chart loses its identity (symbol,
        // exchange, account_type, drawings, command history) and every
        // subsequent join reads an empty key.  Owner repro 2026-06-26:
        // split (auto-tag) -> untag -> tag-in-new-color landed with sym=''
        // because `disconnect_from_current_group` saw the auto_bubble empty
        // after detach and removed it as "an empty auto group", taking the
        // chart's home with it.  Skip removal when `old_gid == auto_tag_id`.
        let auto_bid_home = self.panel_app.panel_grid.containers().get(&chart_id)
            .map(|w| w.auto_tag_id);
        sglog!(
            "[disconnect_from_current_group] leaf={:?} cid={:?} old_gid={:?} auto_bid_home={:?}",
            leaf_id, chart_id, old_gid, auto_bid_home
        );
        let _ = self.detach_chart_from_tag(chart_id);
        // Remove the old group if it is now empty and was auto-created — but
        // NEVER remove the chart's permanent home auto_bubble.
        let should_remove = self.core.tags().get(old_gid)
            .map(|t| t.auto_created && t.members.is_empty())
            .unwrap_or(false)
            && auto_bid_home != Some(old_gid);
        if should_remove {
            self.remove_tag(old_gid);
            sglog!("[disconnect_from_current_group] removed empty auto group {:?}", old_gid);
        } else if auto_bid_home == Some(old_gid) {
            sglog!(
                "[disconnect_from_current_group] PRESERVED home auto_bubble {:?} (would have orphaned chart {:?})",
                old_gid, chart_id
            );
        }
    }

    /// DEAD (Step 3.1): under the per-bubble drawing model, a grouped
    /// window's `current_tag_id` already equals the group's bid — any
    /// mutation made through `core.drawings_mut()` on that bid writes
    /// directly into `DataBubble.primitives`, which IS the group's
    /// authoritative primitive list. There is no separate per-window
    /// `DrawingManager` copy left to write back. Only marks the sidebar
    /// dirty; kept as a call-site marker for Step 4/5 cleanup.
    pub(super) fn sync_drawing_back_to_group(&mut self) {
        self.sidebar_data_dirty = true;
    }

    /// After split joins an existing group, create indicator instances on the
    /// new windows for each indicator config already in the group.
    pub(super) fn sync_group_indicators_to_new_members(
        &mut self,
        group_id: mlc_core::ids::BubbleId,
        new_leaf_chart_ids: &[(mlc_core::LeafId, mlc_core::ChartId)],
    ) {
        // Respect the sync_indicators flag — if disabled, skip indicator sync.
        if !self.core.bubbles().sync_flags(group_id).map(|sf| sf.sync_indicators).unwrap_or(true) {
            return;
        }

        // Collect indicator configs from the group.
        let configs: Vec<(String, String)> = self.core.bubbles().indicator_configs(group_id)
            .iter()
            .map(|c| (c.type_id.clone(), c.name.clone()))
            .collect();

        if configs.is_empty() {
            return;
        }

        let group_bid_raw = group_id.0;
        for &(_leaf_id, chart_id) in new_leaf_chart_ids {
            // Already-present type_ids in this group (= bubble).
            let existing_types: std::collections::HashSet<String> = {
                let im = self.core.indicators();
                im.instances_for_bubble(group_id)
                    .into_iter()
                    .map(|i| i.type_id.clone())
                    .collect()
            };

            for (type_id, name) in &configs {
                if existing_types.contains(type_id) {
                    continue;
                }
                let new_id_opt = self.core.indicators_mut().add(Some(group_id), type_id);
                if let Some(new_id) = new_id_opt {
                    log::debug!(
                        "[BubbleStore] Split sync: created indicator '{}' (id={}) on bubble {} (chart {:?})",
                        name, new_id, group_bid_raw, chart_id
                    );
                }
            }

            // Calculate indicators on the shared bubble using this chart's bars.
            let bars = self.bars_for_chart(chart_id);
            self.core.indicators_mut().recalc_bubble(group_id, &bars);
        }
        self.sync_sub_panes_from_manager();
        log::debug!(
            "[BubbleStore] Synced {} indicator configs to {} new members",
            configs.len(), new_leaf_chart_ids.len()
        );
    }

    /// When an indicator is created on a grouped window, create matching instances
    /// on all peer windows in the same group.
    pub(super) fn sync_group_indicator_to_peers(
        &mut self,
        group_id: mlc_core::ids::BubbleId,
        type_id: &str,
        source_leaf: mlc_core::LeafId,
    ) {
        // Respect the sync_indicators flag — if disabled, skip peer sync.
        if !self.core.bubbles().sync_flags(group_id).map(|sf| sf.sync_indicators).unwrap_or(true) {
            return;
        }

        // Collect peer chart_ids (excluding source). Symbol no longer matters
        // — each peer recalculates indicators against its own bars.
        let source_chart_id = self.panel_app.panel_grid.container_id_for_leaf(source_leaf);
        let peer_cids: Vec<mlc_core::ChartId> = self.core.tags()
            .chart_member_ids(group_id)
            .into_iter()
            .filter(|&cid| Some(cid) != source_chart_id)
            .collect();

        for peer_cid in &peer_cids {
            let peer_cid = *peer_cid;
            // Single write guard per peer — covers create_instance, assign_window,
            // origin_id reset, and the recalc all in one short scope. Pre-cutover
            // this used a 3x-repeated `self.indicator_manager.*` chain (no RwLock);
            // re-grabbing `indicators_mut()` per call after the BackendCore lift
            // is what wedged the UI on 2026-06-20 (each create_instance fired its
            // own implicit drop before the next take, so technically not a hard
            // deadlock — but the repeated lock/unlock churn combined with the
            // implicit `calculate_all_for_window` rayon pool put the main thread
            // in a state where DWM marked the window Not Responding).
            let new_id_opt = {
                let mut im = self.core.indicators_mut();
                // All peers share the same group bubble; the indicator
                // lives once on the bubble, not per-chart.
                let nid = im.add(Some(group_id), type_id);
                if let Some(new_id) = nid {
                    im.clear_origin(new_id);
                }
                nid
            };
            if let Some(new_id) = new_id_opt {
                // Real bars from the peer chart's bubble (cutover sweep stub'd this empty).
                let bars: Option<Vec<mlc_core::Bar>> = self.panel_app.panel_grid
                    .containers().get(&peer_cid)
                    .map(|w| w.current_tag_id)
                    .and_then(|bid| self.with_bubble(bid, |b| b.bars.clone()));
                if let Some(bars) = bars {
                    self.core.indicators_mut().recalc_bubble(group_id, &bars);
                }
                log::debug!(
                    "[BubbleStore] Synced indicator '{}' (id={}) to peer chart {:?}",
                    type_id, new_id, peer_cid
                );
            }
        }
        self.sync_sub_panes_from_manager();
    }
}
