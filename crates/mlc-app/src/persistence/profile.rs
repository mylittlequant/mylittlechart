//! Profile / watchlist / template persistence: read/write user state to disk.
#[cfg(target_arch = "wasm32")]
use crate::agent as gate4agent;


use crate::ChartApp;
use crate::persistence::live_cache as preset_cache;

impl ChartApp {
    /// Collect the current app state into a [`UserProfile`] snapshot.
    ///
    /// Only lightweight metadata is captured here.  Heavy data (chart presets,
    /// templates, watchlists) are stored in separate files managed by their
    /// own sub-systems.
    pub fn build_user_profile(&self) -> mlc_core::UserProfile {
        // Preserve device identity and telemetry from the currently loaded
        // profile so that we don't clobber counters on every save.
        let user_guard = self.core.user();
        let existing = user_guard.profile();
        let inline = &self.panel_app.toolbar_state.floating_inline_bar;
        let inline_dock_str = match inline.dock_edge {
            mlc_core::InlineDockEdge::Bottom => "Bottom",
            mlc_core::InlineDockEdge::Top => "Top",
            mlc_core::InlineDockEdge::Free => "Free",
        };
        let fav = &self.panel_app.toolbar_state.favorites_bar;
        let fav_dock_str = match fav.dock_edge {
            mlc_core::InlineDockEdge::Bottom => "Bottom",
            mlc_core::InlineDockEdge::Top => "Top",
            mlc_core::InlineDockEdge::Free => "Free",
        };
        // Community modal per-tab size (community-modal chrome arc, cloud
        // builds only — `CommunityState` does not exist outside `cloud`).
        // Non-cloud builds carry forward whatever the loaded profile already
        // held, same "nothing fresher to pull" reasoning `notification_
        // settings` above documents.
        #[cfg(feature = "cloud")]
        let (community_forum_size, community_messages_size) = (
            self.panel_app.community_state.forum_size,
            self.panel_app.community_state.messages_size,
        );
        #[cfg(not(feature = "cloud"))]
        let (community_forum_size, community_messages_size) =
            (existing.ui.community_forum_size, existing.ui.community_messages_size);
        mlc_core::UserProfile {
            domain: mlc_core::user_profile::profile::ProfileDomainSettings {
                version: mlc_core::user_profile::profile::PROFILE_VERSION,
                // Preserve fields managed by the profile itself
                app_version: existing.domain.app_version.clone(),
                bar_count: existing.domain.bar_count,
                data_load: existing.domain.data_load.clone(),
                recalc_mode: existing.domain.recalc_mode.clone(),
                server_enabled: self.panel_app.user_settings_state.server_enabled,
                server_port: self.panel_app.user_settings_state.server_port,
                exchange_keys: existing.domain.exchange_keys.clone(),
                connector_enabled: self.sidebar_state.connector_enabled.clone(),
                notification_settings: existing.domain.notification_settings.clone(),
                // Secrets aren't touched by this snapshot builder either —
                // same preserve-live-value pattern as `notification_settings`
                // and `exchange_keys` above (this function never reads
                // `AlertSettingsState`, so there is nothing fresher to pull).
                notification_secrets: existing.domain.notification_secrets.clone(),
                // Preserve profile identity fields managed by the profile system
                profile_id: existing.domain.profile_id.clone(),
                // Granularity favorites (kusok 1): mutated in-place via
                // `self.core.user_mut().profile_mut()` at add/remove time
                // (chart_out_events.rs), not via this snapshot builder —
                // preserve whatever is already on the live profile.
                granularity_favorites: existing.domain.granularity_favorites.clone(),
                // Cloud connectivity — managed by the sync layer, not by this
                // snapshot builder; preserve whatever is on the live profile.
                linked_account: existing.domain.linked_account.clone(),
                cloud_enabled: existing.domain.cloud_enabled,
                sync_level: existing.domain.sync_level.clone(),
                sync_state: existing.domain.sync_state.clone(),
            },
            ui: mlc_core::user_profile::profile::ProfileUiSettings {
                active_preset_id: self.panel_app.active_preset_id.clone(),
                open_tabs: self.panel_app.open_tabs.clone(),
                // Legacy top-level mirror of `windows[0].open_shared_tabs`
                // — same "windows[0]" mirroring `open_tabs` above already
                // does. See `share_viewer::ChartApp::collect_open_shared_
                // tabs`'s own doc (shared-tab-reload-anchor arc).
                open_shared_tabs: self.collect_open_shared_tabs(),
                // Carried from the loaded profile, never recomputed: this
                // snapshot is built from live UI state, and "has the wizard
                // been shown" is not UI state — it is a fact about the
                // profile's history that only `mark_welcome_wizard_seen`
                // ever sets.
                welcome_wizard_seen: existing.ui.welcome_wizard_seen,
                active_theme: self.panel_app.theme_manager.preset_name().to_string(),
                active_ui_style: self.panel_app.theme_manager.current().style.as_name().to_string(),
                sidebar_visible: self.sidebar_state.is_right_open(),
                sidebar_panel: Self::panel_to_str(self.sidebar_state.right_panel),
                sidebar_width: Some(self.sidebar_state.right_sidebar_width),
                inline_bar_x: Some(inline.x),
                inline_bar_y: Some(inline.y),
                inline_bar_dock: Some(inline_dock_str.to_string()),
                favorites_bar_x: Some(fav.x),
                favorites_bar_y: Some(fav.y),
                favorites_bar_dock: Some(fav_dock_str.to_string()),
                favorites_bar_visible: self.panel_app.toolbar_state.favorites_bar_visible,
                // Screenshot destination (screenshot-destination arc): live
                // value comes from the toolbar state — the camera button's
                // destination dropdown is the only writer.
                screenshot_destination: self.panel_app.toolbar_state.screenshot_destination,
                // Favorite drawing tools (favorite-primitives arc): mutated
                // in-place via `self.core.user_mut().profile_mut()` by the
                // `ToggleFavoritePrimitive` handler (chart_out_events.rs),
                // not via this snapshot builder — preserve whatever is
                // already on the live profile (same pattern as
                // `pin_granularity_favorites` below).
                favorite_primitives: existing.ui.favorite_primitives.clone(),
                // Favorite indicators (indicator-search-favorites arc):
                // mutated in-place via `self.core.user_mut().profile_mut()`
                // by the `ToggleIndicatorStar` handler (chart_out_events.rs),
                // not via this snapshot builder — preserve the live vec, same
                // pattern as `favorite_primitives` above.
                favorite_indicators: existing.ui.favorite_indicators.clone(),
                // Preserve profile identity fields managed by the profile system
                display_name: existing.ui.display_name.clone(),
                avatar: existing.ui.avatar.clone(),
                profile_created_at: existing.ui.profile_created_at,
                device_name: existing.ui.device_name.clone(),
                language: self.panel_app.user_settings_state.language.clone(),
                windows: existing.ui.windows.clone(),
                // Pinned-favorites-row toggle (kusok 3): mutated in-place via
                // `self.core.user_mut().profile_mut()` at toggle time
                // (chart_out_events.rs's `ToggleGranularityPin` handler),
                // not via this snapshot builder — preserve the live value,
                // same pattern as `granularity_favorites` above.
                pin_granularity_favorites: existing.ui.pin_granularity_favorites,
                // Toolbar-merge / quick-search toggles (symbol-search-
                // compare-inner arc): mutated in-place via the user facade
                // at toggle time, same preserve-live-value pattern as
                // `pin_granularity_favorites` above.
                split_symbol_compare: existing.ui.split_symbol_compare,
                symbol_quick_search: existing.ui.symbol_quick_search,
                // Panel-gear presentation preference (overlay-gear arc,
                // 2026-07-28): mutated in-place at toggle time, same
                // preserve-live-value pattern.
                panel_gear_opens_modal: existing.ui.panel_gear_opens_modal,
                // Topbar-takeover toggle (workspace-panel arc, Stage 4):
                // mutated in-place via `toggle_trading_panel_topbar_takeover`
                // at toggle time, same preserve-live-value pattern as
                // `split_symbol_compare` above.
                trading_panel_topbar_takeover: existing.ui.trading_panel_topbar_takeover,
                // Control-point shape/size (control-point-style-ui arc,
                // 2026-08-08): mutated in-place via `set_control_point_shape`/
                // `set_control_point_size` at pick time, same
                // preserve-live-value pattern as `trading_panel_topbar_takeover`
                // above.
                control_point_shape: existing.ui.control_point_shape,
                control_point_size: existing.ui.control_point_size,
                // EWA engine configuration (owner ask 2026-08-14): mutated
                // in-place via `apply_ewa_engine_config_edit` at edit time,
                // same preserve-live-value pattern as `control_point_shape`/
                // `control_point_size` above.
                ewa_engine: existing.ui.ewa_engine.clone(),
                // Candle-pattern engine configuration (markup-engines wave,
                // 2026-08-25): no edit UI exists yet (see
                // `mlc_core::state::candle_pattern_engine`'s own module
                // doc), so this is a plain carry-forward, same
                // preserve-live-value pattern as `ewa_engine` above.
                candle_pattern_engine: existing.ui.candle_pattern_engine.clone(),
                // Smart-money engine configuration (markup-engines wave,
                // 2026-08-25): same preserve-live-value pattern as
                // `candle_pattern_engine` above — no edit UI exists yet.
                smc_engine: existing.ui.smc_engine.clone(),
                // Order-flow engine configuration (markup-engines wave,
                // 2026-08-25): same preserve-live-value pattern as
                // `smc_engine` above — no edit UI exists yet.
                orderflow_engine: existing.ui.orderflow_engine.clone(),
                persist_toolbar_last_tools: existing.ui.persist_toolbar_last_tools,
                toolbar_last_tools: if existing.ui.persist_toolbar_last_tools {
                    self.panel_app.toolbar_state.quick_select_memory
                        .iter()
                        .map(|(k, (t, _))| (k.clone(), t.clone()))
                        .collect()
                } else {
                    existing.ui.toolbar_last_tools.clone()
                },
                persist_toolbar_toggles: existing.ui.persist_toolbar_toggles,
                drawings_locked: if existing.ui.persist_toolbar_toggles {
                    self.panel_app.toolbar_state.drawings_locked
                } else {
                    existing.ui.drawings_locked
                },
                drawings_visible: if existing.ui.persist_toolbar_toggles {
                    self.panel_app.toolbar_state.drawings_visible
                } else {
                    existing.ui.drawings_visible
                },
                // Clock hover-glitch toggle (clock popup's third checkbox,
                // owner ask 2026-09-02): mutated in-place via
                // `self.core.user_mut().profile_mut()` at click time
                // (panel_click.rs), not via this snapshot builder — carried
                // forward exactly like `welcome_wizard_seen` above, because
                // this snapshot is built from live UI state and this is not
                // UI state.
                clock_glitch_enabled: existing.ui.clock_glitch_enabled,
                // Toast anchor/quiet-mode (toast restyle-and-anchor follow-up
                // to §8, owner ask 2026-09-09): mutated in-place via
                // `self.core.user_mut().profile_mut()` at toggle/drag time
                // (panel_click.rs), same preserve-live-value pattern as
                // `clock_glitch_enabled` above.
                toast_anchor: existing.ui.toast_anchor,
                toasts_enabled: existing.ui.toasts_enabled,
                // Header account-button display mode (owner ruling
                // 2026-09-10): same preserve-live-value pattern as
                // `toast_anchor` above — mutated in-place via
                // `self.core.user_mut().profile_mut()` at click time
                // (panel_click.rs).
                header_account_display: existing.ui.header_account_display,
                // System event log (system-event-bus arc, 2026-09-05): the
                // live value lives on `ChartApp` itself, mutated in place by
                // `push_system_event`/`mark_all_system_events_read` — not by
                // this snapshot builder, same preserve-live-value pattern as
                // `favorite_primitives` above.
                system_events: self.system_events.clone(),
                community_forum_size,
                community_messages_size,
                // Collapsed flyout sub-sections (toolbar-flyout-collapse
                // arc): mutated in-place via `ChartToolbarState::
                // toggle_flyout_section_collapsed` at double-click time
                // (`mlc-app`'s `on_double_click`), not via this snapshot
                // builder — preserve-live-value pattern, same as
                // `favorite_primitives` above.
                collapsed_flyout_sections: self.panel_app.toolbar_state.collapsed_flyout_sections.clone(),
            },
        }
    }

    /// Update in-memory profile state only.
    ///
    /// DEPRECATED: Disk writes are handled exclusively by `App::save_all()` in
    /// `main.rs`, which coordinates all windows before writing.  Calling this
    /// function from an individual window would write a stale `windows` list
    /// (because each window only knows its own state) and would clobber the
    /// correct multi-window state assembled by `save_all()`.
    ///
    /// This function now only refreshes the in-memory profile.  No file I/O is
    /// performed here.
    pub fn save_user_profile(&mut self) {
        // Only set dirty flags — actual disk writes are done by App
        // which has full context of all windows.
        self.profile_dirty = true;
        self.watchlists_dirty = true;
    }

    /// Set this window's unique identifier.  Call immediately after construction
    /// to override the auto-generated "win_<timestamp>" default.
    pub fn set_window_id(&mut self, id: String) {
        self.window_id = id;
    }

    /// Build a lightweight snapshot of this window's per-window state.
    ///
    /// Captures the `window_id`, the list of open tab preset IDs, and the
    /// currently active preset ID.  Used by the coordinated multi-window save
    /// in `main.rs` so that every OS window's state is stored in
    /// [`mlc_core::UserProfile::windows`] before the profile is written.
    pub fn build_window_state(&self) -> mlc_core::WindowState {
        let inline = &self.panel_app.toolbar_state.floating_inline_bar;
        let inline_dock_str = match inline.dock_edge {
            mlc_core::InlineDockEdge::Bottom => "Bottom",
            mlc_core::InlineDockEdge::Top => "Top",
            mlc_core::InlineDockEdge::Free => "Free",
        };
        let agents_tab_layout = {
            let tree = self.sidebar_state.agent_docking.inner().tree();
            uzor::panels::serialize::LayoutSnapshot::from_tree(tree, "agents")
                .to_json()
                .ok()
        };
        let agents_tab_leaves: Vec<mlc_core::PersistedAgentLeaf> = self
            .sidebar_state
            .agent_leaves
            .iter()
            .map(|(leaf_id, desc)| mlc_core::PersistedAgentLeaf {
                leaf_id: leaf_id.0,
                cli: match desc.cli {
                    gate4agent::AgentCli::Claude => mlc_core::PersistedAgentCli::Claude,
                    gate4agent::AgentCli::Codex => mlc_core::PersistedAgentCli::Codex,
                    gate4agent::AgentCli::Gemini => mlc_core::PersistedAgentCli::Gemini,
                    gate4agent::AgentCli::OpenCode => mlc_core::PersistedAgentCli::OpenCode,
                },
                mode: match desc.mode {
                    gate4agent::InstanceMode::Pty => mlc_core::PersistedInstanceMode::Pty,
                    gate4agent::InstanceMode::Chat => mlc_core::PersistedInstanceMode::Chat,
                },
                workdir: desc.workdir.clone(),
                chat_session_id: desc.chat_session_id.clone(),
            })
            .collect();
        // Log agents state for diagnostics (appears in structured log).
        log::info!(
            "[agents-diag] build_window_state: agents_layout={} agents_leaves={}",
            if agents_tab_layout.is_some() { "Some" } else { "None" },
            agents_tab_leaves.len(),
        );
        mlc_core::WindowState {
            window_id: self.window_id.clone(),
            open_tabs: self.panel_app.open_tabs.clone(),
            // See `share_viewer::ChartApp::collect_open_shared_tabs`'s own
            // doc (shared-tab-reload-anchor arc, item 1/2).
            open_shared_tabs: self.collect_open_shared_tabs(),
            active_preset_id: self.panel_app.active_preset_id.clone(),
            x: self.window_x,
            y: self.window_y,
            width: self.window_width,
            height: self.window_height,
            sidebar_visible: self.sidebar_state.is_right_open(),
            sidebar_panel: Self::panel_to_str(self.sidebar_state.right_panel),
            sidebar_width: Some(self.sidebar_state.right_sidebar_width),
            inline_bar_x: Some(inline.x),
            inline_bar_y: Some(inline.y),
            inline_bar_dock: Some(inline_dock_str.to_string()),
            agents_tab_layout,
            agents_tab_leaves,
        }
    }

    /// Update the in-memory profile's windows list.  Call this before
    /// `save_user_profile()` when multiple OS windows are open.
    pub fn set_profile_windows(&mut self, windows: Vec<mlc_core::WindowState>) {
        self.core.user_mut().profile_mut().ui.windows = windows;
    }

    // =========================================================================
    // Granular persistence helpers — call after each mutation
    // =========================================================================

    /// Persist the user profile (active_preset_id, sidebar state, inline bar, device, telemetry).
    ///
    /// Only sets the dirty flag — App monitors this and saves with full
    /// multi-window context.  Windows must never write profile.json
    /// directly because they don't know about other windows.
    pub fn persist_profile(&mut self) {
        self.profile_dirty = true;
    }

    /// Collect every `BubbleId` a `ChartPanelGrid` references — both the
    /// permanent home bubble (`auto_tag_id`) and the bubble currently
    /// displayed (`current_tag_id`, which differs from `auto_tag_id` while
    /// the window is in a color-tag sync group) for every chart window in
    /// the grid.
    ///
    /// Shared by [`Self::park_active_preset`] (scoped extraction under the
    /// shared-BackendCore arc) and the cold-load path
    /// (`hydration_impl.rs::apply_preset_cold`, which walks the same two
    /// fields while rebuilding `new_containers`) — this is the single
    /// collection routine for "which bubbles belong to this panel grid",
    /// per `docs/mlc/plans/shared-backend-core-2026-07-03.md` §5.3.2.
    pub(crate) fn bubble_ids_for_panel_grid(
        panel_grid: &crate::panel_grid::ChartPanelGrid,
    ) -> Vec<mlc_core::ids::BubbleId> {
        let mut seen = std::collections::HashSet::new();
        let mut ids = Vec::new();
        for window in panel_grid.containers().values() {
            if seen.insert(window.auto_tag_id) {
                ids.push(window.auto_tag_id);
            }
            if seen.insert(window.current_tag_id) {
                ids.push(window.current_tag_id);
            }
        }
        ids
    }

    /// Claim `id` for this window before materializing it via
    /// `apply_preset_cold` — the ONE real cross-window collision point
    /// (park/unpark only ever touch a window's own `live_preset_cache`
    /// entries, so they can never legitimately observe another window's
    /// claim; see the comments on `park_active_preset`/`unpark_preset`).
    ///
    /// Returns `true` if the claim succeeds (unclaimed, or already claimed
    /// by this same window — re-opening your own preset is not a
    /// collision). Returns `false` — and logs — if a DIFFERENT window's
    /// `window_id` already holds it: the caller MUST refuse the cold-load
    /// in that case (§5.3.5 "a preset is live in at most one window").
    /// Now enforced (not just logged) because Phase F wired
    /// `release_all_presets_for_app` into window close / profile-switch,
    /// so a refused claim is guaranteed to eventually clear rather than
    /// wedge the preset forever.
    pub(crate) fn try_claim_preset_for_cold_load(&mut self, id: &str) -> bool {
        if self.core.try_claim_preset(id, &self.window_id) {
            true
        } else {
            log::warn!(
                "[ChartApp] REFUSED: preset '{}' cold-load in window '{}' — already claimed by \
                 another window (same-preset-two-windows hazard, shared-backend-core-2026-07-03.md §5.3.5)",
                id, self.window_id,
            );
            false
        }
    }

    /// Park the active preset's live state into the cache.
    /// Active fields are replaced with cheap placeholders.
    ///
    /// Bubble/tag/indicator/alert/drawing managers are SCOPED extractions
    /// (`extract_bubbles` / `extract_tags_for_bubbles` / `extract_for_bubbles`
    /// ×3), not whole-manager swaps — under the shared-BackendCore arc,
    /// `self.core` is the SAME `Arc<BackendCore>` every other open window
    /// reads from, so a whole-manager `swap()` would blank every other
    /// window's bubbles/tags/indicators/alerts/drawings for as long as this
    /// preset stays parked (`docs/mlc/plans/shared-backend-core-2026-07-03.md`
    /// §5.1-§5.3).
    pub(crate) fn park_active_preset(&mut self, id: &str) {
        // Reaffirm ownership of this preset id before parking its bubbles
        // into this window's cache. Warn-only BY DESIGN (Phase F
        // decision, not an oversight): `park_active_preset` is only ever
        // called on `self.panel_app.active_preset_id` — a preset THIS
        // window is currently displaying live. It is structurally
        // impossible for another window's claim to be sitting on an id
        // that is simultaneously live in THIS window's own panel_grid
        // (the claim registry's own invariant — "live in at most one
        // window" — guarantees whoever is live already owns the claim).
        // A `false` return here means the invariant itself broke (a
        // real bug, not a normal contention case) — logged loudly so it
        // is never silent, but there is no correct action to "refuse"
        // into: the caller already committed to switching away from this
        // preset. Real cross-window collision enforcement lives at the
        // ONE place a genuinely new window can pick up a preset id it
        // does not yet own: `ChartApp::try_claim_preset_for_cold_load`,
        // gating `apply_preset_cold` (see `chart_out_events.rs`).
        if !self.core.try_claim_preset(id, &self.window_id) {
            log::warn!(
                "[ChartApp] WARNING: preset '{}' parked by window '{}' while claim registry \
                 shows a different owner — claim-registry invariant violated, \
                 see shared-backend-core-2026-07-03.md §5.3.5",
                id, self.window_id,
            );
        }
        let pk_total = web_time::Instant::now();
        let t0 = web_time::Instant::now();
        let panel_grid = std::mem::replace(
            &mut self.panel_app.panel_grid,
            crate::panel_grid::ChartPanelGrid::placeholder(),
        );
        let bubble_ids = Self::bubble_ids_for_panel_grid(&panel_grid);
        let pk_grid_us = t0.elapsed().as_micros();

        // Arc C3.2 park guard: an alerted bubble IS the live subscription
        // for its instrument (`BackendCore::ensure_alert_bubble`'s own doc)
        // — parking it into this window's per-tab cache, which the tick
        // loop never walks, would silently stop that alert from firing
        // until the tab is reopened. Exclude any bid whose
        // `has_active_alerts` mirror (kept fresh by
        // `AlertManager::resync_has_alerts` on every `AccountAlertStore`
        // mutation) is set from every extraction list below — it stays live
        // in the shared BackendCore store, park or no park, exactly like an
        // alert on an instrument with no window at all.
        let extract_ids: Vec<mlc_core::ids::BubbleId> = bubble_ids
            .iter()
            .copied()
            .filter(|&bid| !self.with_bubble(bid, |b| b.has_active_alerts).unwrap_or(false))
            .collect();

        let t0 = web_time::Instant::now();
        let bubble_store = self.core.bubbles_mut().extract_bubbles(&extract_ids);
        let tag_manager = self.core.tags_mut().extract_tags_for_bubbles(&extract_ids);
        let pk_bubble_us = t0.elapsed().as_micros();
        let t0 = web_time::Instant::now();
        let indicator_manager = self.core.indicators_mut().extract_for_bubbles(&extract_ids);
        let pk_ind_us = t0.elapsed().as_micros();
        let t0 = web_time::Instant::now();
        // Per-bubble drawing sessions (Step 3.1) must travel with
        // tag_manager since both are keyed by the same BubbleIds.
        let alert_manager = self.core.alerts_mut().extract_for_bubbles(&extract_ids);
        let pk_alert_us = t0.elapsed().as_micros();
        let t0 = web_time::Instant::now();
        let drawing_manager = self.core.drawings_mut().extract_for_bubbles(&extract_ids);
        let pk_drawing_us = t0.elapsed().as_micros();
        let t0 = web_time::Instant::now();
        let leaf_color_tags = std::mem::take(&mut self.panel_app.leaf_color_tags);
        let indicator_overlay_states = std::mem::take(&mut self.panel_app.indicator_overlay_states);
        let pending_sub_pane_ratios = std::mem::take(&mut self.pending_sub_pane_ratios);
        let pending_sub_pane_above_main = std::mem::take(&mut self.pending_sub_pane_above_main);
        let pending_sub_pane_order = std::mem::take(&mut self.pending_sub_pane_order);
        let pk_takes_us = t0.elapsed().as_micros();
        let t0 = web_time::Instant::now();
        let slot_dockings = std::mem::replace(
            &mut self.sidebar_state.slot_dockings,
            std::array::from_fn(|_| mlc_app_sidebar_content::SlotDockingManager::new()),
        );
        let pk_slots_us = t0.elapsed().as_micros();
        let t0 = web_time::Instant::now();
        let panels_store = std::mem::replace(
            &mut self.panels_store,
            crate::panels_store::TradingPanelsStore::new(),
        );
        let pk_panels_us = t0.elapsed().as_micros();
        let state = preset_cache::LivePresetState {
            panel_grid,
            bubble_store,
            tag_manager,
            indicator_manager,
            alert_manager,
            drawing_manager,
            leaf_color_tags,
            indicator_overlay_states,
            pending_sub_pane_ratios,
            pending_sub_pane_above_main,
            pending_sub_pane_order,
            needs_initial_viewport_fit: self.needs_initial_viewport_fit,
            slot_dockings,
            panels_store,
            focused_free_leaf: self.sidebar_state.focused_free_leaf.take(),
            parked_at: web_time::Instant::now(),
            bars_released: false,
        };
        let t0 = web_time::Instant::now();
        self.live_preset_cache.insert(id.to_string(), state);
        let pk_insert_us = t0.elapsed().as_micros();
        log::debug!(
            "[ChartApp] Parked preset '{}' into live cache ({} total cached) park_total={}us grid={}us bubble={}us ind={}us alert={}us drawing={}us takes={}us slots={}us panels={}us insert={}us",
            id, self.live_preset_cache.len(), pk_total.elapsed().as_micros(),
            pk_grid_us, pk_bubble_us, pk_ind_us, pk_alert_us, pk_drawing_us, pk_takes_us, pk_slots_us, pk_panels_us, pk_insert_us,
        );
    }

    /// Unpack a cached preset into active fields. Returns true on cache hit.
    pub(crate) fn unpark_preset(&mut self, id: &str) -> bool {
        let up_total = web_time::Instant::now();
        let t0 = web_time::Instant::now();
        let Some(state) = self.live_preset_cache.remove(id) else { return false; };
        // Reaffirm ownership — see the matching comment in
        // `park_active_preset`. Warn-only BY DESIGN, same reasoning: this
        // preset was already parked in THIS window's own
        // `live_preset_cache` (a per-window field, we just removed it
        // above), so the claim can only ever belong to `self.window_id`
        // here. A `false` return means the invariant broke elsewhere —
        // surfaced loudly, but by this point `state` has already been
        // removed from the cache, so refusing here would strand it
        // (neither restored to `self` nor left recoverable in the cache).
        if !self.core.try_claim_preset(id, &self.window_id) {
            log::warn!(
                "[ChartApp] WARNING: preset '{}' unparked by window '{}' while claim registry \
                 shows a different owner — same-preset-two-windows hazard, \
                 see shared-backend-core-2026-07-03.md §5.3.5",
                id, self.window_id,
            );
        }
        let up_remove_us = t0.elapsed().as_micros();
        let t0 = web_time::Instant::now();
        // Each of the following lines DROPS whatever lives in the field
        // currently (placeholder from when we parked) — Drops of complex
        // managers can be expensive. Track them individually.
        self.panel_app.panel_grid = state.panel_grid;
        let up_grid_us = t0.elapsed().as_micros();
        let t0 = web_time::Instant::now();
        self.core.bubbles_mut().merge_bubbles(state.bubble_store);
        self.core.tags_mut().merge_tags(state.tag_manager);
        let up_bubble_us = t0.elapsed().as_micros();
        let t0 = web_time::Instant::now();
        self.core.indicators_mut().merge_for_bubbles(state.indicator_manager);
        let up_ind_us = t0.elapsed().as_micros();
        let t0 = web_time::Instant::now();
        self.core.alerts_mut().merge_for_bubbles(state.alert_manager);
        let up_alert_us = t0.elapsed().as_micros();
        let t0 = web_time::Instant::now();
        self.core.drawings_mut().merge_for_bubbles(state.drawing_manager);
        let up_drawing_us = t0.elapsed().as_micros();
        let t0 = web_time::Instant::now();
        self.panel_app.leaf_color_tags = state.leaf_color_tags;
        self.panel_app.indicator_overlay_states = state.indicator_overlay_states;
        self.pending_sub_pane_ratios = state.pending_sub_pane_ratios;
        self.pending_sub_pane_above_main = state.pending_sub_pane_above_main;
        self.pending_sub_pane_order = state.pending_sub_pane_order;
        self.needs_initial_viewport_fit = state.needs_initial_viewport_fit;
        let up_misc_us = t0.elapsed().as_micros();
        let t0 = web_time::Instant::now();
        self.sidebar_state.slot_dockings = state.slot_dockings;
        let up_slots_us = t0.elapsed().as_micros();
        let t0 = web_time::Instant::now();
        self.panels_store = state.panels_store;
        let up_panels_us = t0.elapsed().as_micros();
        self.sidebar_state.focused_free_leaf = state.focused_free_leaf;
        self.sidebar_data_dirty = true;
        log::debug!(
            "[ChartApp] Unpacked preset '{}' from live cache unpark_total={}us remove={}us grid={}us bubble={}us ind={}us alert={}us drawing={}us misc={}us slots={}us panels={}us",
            id, up_total.elapsed().as_micros(),
            up_remove_us, up_grid_us, up_bubble_us, up_ind_us, up_alert_us, up_drawing_us, up_misc_us, up_slots_us, up_panels_us,
        );
        true
    }

    /// Persist watchlists to disk.
    ///
    /// Only sets the dirty flag — App saves watchlists from AppState
    /// (the single source of truth shared across all windows).
    pub fn persist_watchlists(&mut self) {
        self.watchlists_dirty = true;
    }

    /// Persist templates to disk.
    pub fn persist_templates(&self) {
        // Bound first so the `templates()` guard drops before the body runs
        // (clippy::significant_drop_in_scrutinee).
        let save_result = self.core.templates().save_to_default_dir(None);
        if let Err(e) = save_result {
            log::debug!("[persist] templates: {:?}", e);
        }
    }

    /// Anonymous/local scope for the account-wide alert store
    /// (`mlc-backend`'s `AccountAlertStore`, keyed `(account_id,
    /// InstrumentAddress, condition)` — Arc C3, owner ruling 2026-09-06 §2).
    /// Mirrors `mlc-backend`'s crate-private `alert::manager::
    /// LOCAL_SHIM_ACCOUNT_ID` literal by value: that constant is
    /// `pub(crate)` by doctrine (app code goes through the account-wide
    /// facade, never a raw scope string), and every alert the piece-1
    /// bid-based shim already stamped (`AlertManager::add`/`restore_one`) is
    /// stamped under this EXACT literal — a different anonymous fallback
    /// here would silently orphan them the moment the user signs out.
    const ANONYMOUS_ALERT_ACCOUNT_ID: &'static str = "local";

    /// Resolve this window's `AccountAlertStore` scope key: the signed-in
    /// account's public id when the live profile has a linked account, else
    /// [`Self::ANONYMOUS_ALERT_ACCOUNT_ID`]. Used by the account-wide
    /// evaluation walk (`frame_lifecycle/tick.rs`) and the legacy-alert
    /// migration fold (`hydration_impl.rs`).
    pub(crate) fn alert_account_id(&self) -> String {
        let user = self.core.user();
        let profile = user.profile();
        if let Some(linked) = profile.domain.linked_account.as_ref() {
            if !linked.public_id.is_empty() {
                return linked.public_id.clone();
            }
        }
        Self::ANONYMOUS_ALERT_ACCOUNT_ID.to_string()
    }

    /// Persist the account-wide alert store to its own file (`alerts.json`,
    /// Arc C3.3) — synchronous, immediate write, same pattern as
    /// `persist_templates` above rather than the dirty-flag `save_all()`
    /// indirection `persist_profile`/`persist_watchlists` use: under the
    /// shared-BackendCore arc, `core.alerts()` is already the ONE
    /// process-wide store every window reads and writes through, so there is
    /// no multi-window aggregation step to wait for.
    ///
    /// `preset.alerts` (the legacy flat field, `input/mod.rs`'s save path)
    /// keeps being written on every preset save too — a compatibility mirror
    /// for older clients reading a preset file directly, not this store's
    /// source of truth anymore.
    pub fn persist_alerts(&mut self) {
        let items = self.core.alerts().items_all();
        let path = mlc_core::active_profile_data_dir().join("alerts.json");
        if let Err(e) = mlc_core::save_json(&path, &items, None) {
            log::debug!("[persist] alerts: {:?}", e);
        }
        // Tell the shell's per-frame drain to push an alerts sync cycle —
        // see `alerts_sync_dirty`'s own doc.
        self.alerts_sync_dirty = true;
    }

    /// Load and apply a previously saved user profile from `user_data/profile.json`.
    ///
    /// Also restores the [`mlc_app_sidebar_content::watchlist::WatchlistManager`] from
    /// `user_data/watchlists.json` when that file exists.
    ///
    /// Missing files are silently ignored so that a fresh install with no
    /// saved data still starts correctly.
    pub fn load_user_profile(&mut self) {
        // Load profile metadata.
        match mlc_core::load_profile(None) {
            Ok(profile) => {
                // Restore active preset id.
                self.panel_app.active_preset_id = profile.ui.active_preset_id;

                // Restore sidebar width first (before opening, so the correct
                // width is applied when the panel is opened).
                if let Some(width) = profile.ui.sidebar_width {
                    self.sidebar_state.set_right_width(width);
                }

                // Restore the open panel (or leave closed if None/unknown).
                if profile.ui.sidebar_visible {
                    if let Some(panel_name) = &profile.ui.sidebar_panel {
                        let panel = Self::str_to_panel(panel_name);
                        self.sidebar_state.set_right_panel(panel);
                    }
                }

                // Restore connector enabled/disabled state.
                if !profile.domain.connector_enabled.is_empty() {
                    self.sidebar_state.connector_enabled = profile.domain.connector_enabled.clone();
                }

                // Restore inline toolbar position.
                if let Some(x) = profile.ui.inline_bar_x {
                    self.panel_app.toolbar_state.floating_inline_bar.x = x;
                }
                if let Some(y) = profile.ui.inline_bar_y {
                    self.panel_app.toolbar_state.floating_inline_bar.y = y;
                }
                if let Some(ref dock) = profile.ui.inline_bar_dock {
                    self.panel_app.toolbar_state.floating_inline_bar.dock_edge = match dock.as_str() {
                        "Top" => mlc_core::InlineDockEdge::Top,
                        "Free" => mlc_core::InlineDockEdge::Free,
                        _ => mlc_core::InlineDockEdge::Bottom,
                    };
                }

                // Restore favorites quick-pick bar position + visibility
                // (favorite-primitives arc) — same flat-field pattern as the
                // inline bar above.
                if let Some(x) = profile.ui.favorites_bar_x {
                    self.panel_app.toolbar_state.favorites_bar.x = x;
                }
                if let Some(y) = profile.ui.favorites_bar_y {
                    self.panel_app.toolbar_state.favorites_bar.y = y;
                }
                if let Some(ref dock) = profile.ui.favorites_bar_dock {
                    self.panel_app.toolbar_state.favorites_bar.dock_edge = match dock.as_str() {
                        "Top" => mlc_core::InlineDockEdge::Top,
                        "Free" => mlc_core::InlineDockEdge::Free,
                        _ => mlc_core::InlineDockEdge::Bottom,
                    };
                }
                self.panel_app.toolbar_state.favorites_bar_visible = profile.ui.favorites_bar_visible;
                self.panel_app.toolbar_state.screenshot_destination = profile.ui.screenshot_destination;
                // Community modal per-tab size (community-modal chrome arc,
                // cloud builds only) — `None` leaves `CommunityState::
                // default`'s own 760×560/1000×640 fallback in place.
                #[cfg(feature = "cloud")]
                {
                    if let Some(size) = profile.ui.community_forum_size {
                        self.panel_app.community_state.forum_size = Some(size);
                    }
                    if let Some(size) = profile.ui.community_messages_size {
                        self.panel_app.community_state.messages_size = Some(size);
                    }
                }
                // System event log (system-event-bus arc, 2026-09-05):
                // restore the persisted ring into live app state so events
                // survive a restart; new pushes append onto this.
                self.system_events = profile.ui.system_events.clone();
            }
            Err(e) => {
                log::warn!("[UserProfile] Failed to load profile: {}", e);
            }
        }

        // Restore watchlist manager. `load_json` itself returns `Err` (and is
        // handled below as a silent miss) when the key has never been
        // written — no separate existence probe needed. A prior
        // `watchlists_path.exists()` guard here relied on `std::path::Path::exists()`,
        // which always reports `false` on wasm32 (no real filesystem behind
        // the `LocalStorageBackend` paths), silently skipping the read on
        // every wasm boot even after a watchlist had been saved.
        let watchlists_path = mlc_core::user_profile::storage::watchlists_path();
        match mlc_core::load_json::<mlc_backend::watchlist::WatchlistManager>(&watchlists_path, None) {
            Ok(manager) => self.adopt_loaded_watchlists(manager),
            Err(e) => {
                log::debug!("[UserProfile] no saved watchlists ({}) — keeping defaults", e);
            }
        }

        // Restore the account-wide alert store (Arc C3.3) from its own
        // file — same shape as watchlists.json above. `load_user_profile` is
        // called once at process startup, never per-window (see
        // `load_watchlists`'s own doc), so the shared-BackendCore
        // `AccountAlertStore` is seeded exactly once; every later
        // `apply_preset_cold` migration fold (`hydration_impl.rs`) dedups
        // against whatever already landed here.
        let alerts_path = mlc_core::active_profile_data_dir().join("alerts.json");
        match mlc_core::load_json::<Vec<mlc_core::alert::AlertItem>>(&alerts_path, None) {
            Ok(items) => {
                let restored = items.len();
                for item in items {
                    self.core.alerts_mut().restore_item(item);
                }
                log::debug!("[UserProfile] restored {} alerts from alerts.json", restored);
            }
            Err(e) => {
                log::debug!("[UserProfile] no saved alerts.json ({}) — starting empty", e);
            }
        }
    }

    /// Load watchlists from disk without touching the user profile.
    ///
    /// Called by `new_window()` so that each window starts with the persisted
    /// watchlist state.  Profile loading (`profile.json`) is NOT performed here
    /// — that is done once at startup in `main()` and passed in via `apply_profile_state`.
    pub fn load_watchlists(&mut self) {
        // See the comment in `load_user_state` above — no `.exists()` guard;
        // `load_json` already treats a missing key as `Err` and is handled below.
        let watchlists_path = mlc_core::user_profile::storage::watchlists_path();
        match mlc_core::load_json::<mlc_backend::watchlist::WatchlistManager>(&watchlists_path, None) {
            Ok(manager) => self.adopt_loaded_watchlists(manager),
            Err(e) => {
                log::debug!("[UserProfile] no saved watchlists ({}) — keeping defaults", e);
            }
        }
    }

    fn adopt_loaded_watchlists(&mut self, manager: mlc_backend::watchlist::WatchlistManager) {
        self.core.watchlists_mut().replace_all(manager);
        if self.core.watchlists_mut().apply_seed_migration() {
            log::debug!("[UserProfile] watchlist seed migration — missing venues appended");
            self.persist_watchlists();
        }
    }
}
