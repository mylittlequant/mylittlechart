//! Profile / watchlist / template persistence: read/write user state to disk.

use crate::ChartApp;
use crate::preset_cache;
use zengeld_terminal_indicators::IndicatorManager;
use zengeld_chart::ScaleMode;

impl ChartApp {
    /// Collect the current app state into a [`UserProfile`] snapshot.
    ///
    /// Only lightweight metadata is captured here.  Heavy data (chart presets,
    /// templates, watchlists) are stored in separate files managed by their
    /// own sub-systems.
    pub fn build_user_profile(&self) -> zengeld_chart::UserProfile {
        // Preserve device identity and telemetry from the currently loaded
        // profile so that we don't clobber counters on every save.
        let existing = &self.panel_app.user_manager.profile;
        let inline = &self.panel_app.toolbar_state.floating_inline_bar;
        let inline_dock_str = match inline.dock_edge {
            zengeld_chart::InlineDockEdge::Bottom => "Bottom",
            zengeld_chart::InlineDockEdge::Top => "Top",
            zengeld_chart::InlineDockEdge::Free => "Free",
        };
        zengeld_chart::UserProfile {
            version: zengeld_chart::user_profile::profile::PROFILE_VERSION,
            active_preset_id: self.panel_app.active_preset_id.clone(),
            open_tabs: self.panel_app.open_tabs.clone(),
            active_theme: self.panel_app.theme_manager.preset_name().to_string(),
            sidebar_visible: self.sidebar_state.is_right_open(),
            sidebar_panel: Self::panel_to_str(self.sidebar_state.right_panel),
            sidebar_width: Some(self.sidebar_state.right_sidebar_width),
            inline_bar_x: Some(inline.x),
            inline_bar_y: Some(inline.y),
            inline_bar_dock: Some(inline_dock_str.to_string()),
            // Preserve profile identity fields managed by the profile system
            profile_id: existing.profile_id.clone(),
            display_name: existing.display_name.clone(),
            avatar: existing.avatar.clone(),
            profile_created_at: existing.profile_created_at,
            // Preserve fields managed by the profile itself
            device_name: existing.device_name.clone(),
            app_version: existing.app_version.clone(),
            linked_account: existing.linked_account.clone(),
            telemetry: existing.telemetry.clone(),
            bar_count: existing.bar_count,
            data_load: existing.data_load.clone(),
            recalc_mode: existing.recalc_mode.clone(),
            language: self.panel_app.user_settings_state.language.clone(),
            scale_mode: match self.default_scale_mode {
                ScaleMode::Auto   => "Auto".to_string(),
                ScaleMode::Focus  => "Focus".to_string(),
                ScaleMode::Manual => "Manual".to_string(),
            },
            cloud_enabled: existing.cloud_enabled,
            sync_level: existing.sync_level.clone(),
            ota_enabled: self.panel_app.user_settings_state.ota_enabled,
            server_enabled: self.panel_app.user_settings_state.server_enabled,
            server_port: self.panel_app.user_settings_state.server_port,
            legacy_single_agent_key: String::new(),
            local_agent_keys: existing.local_agent_keys.clone(),
            exchange_keys: existing.exchange_keys.clone(),
            connector_enabled: self.sidebar_state.connector_enabled.clone(),
            notification_settings: existing.notification_settings.clone(),
            windows: existing.windows.clone(),
            sync_state: {
                let ui = &self.panel_app.user_settings_state;
                zengeld_chart::user_profile::profile::SyncState {
                    enabled: ui.sync_enabled,
                    last_sync_timestamp: existing.sync_state.last_sync_timestamp,
                    sync_vault: true,
                    sync_presets: ui.sync_presets,
                    sync_templates: ui.sync_templates,
                    sync_watchlists: ui.sync_watchlists,
                    sync_theme: ui.sync_theme_toggle,
                    sync_recovery_key: true,
                    // Preserve the synced_items set — it is managed by the updater
                    // loop and must not be reset when the user changes settings.
                    synced_items: existing.sync_state.synced_items.clone(),
                    // Preserve the last-synced checksum map — managed by the updater
                    // loop and written back to the profile for cross-restart persistence.
                    last_synced_checksums: existing.sync_state.last_synced_checksums.clone(),
                }
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
    /// [`zengeld_chart::UserProfile::windows`] before the profile is written.
    pub fn build_window_state(&self) -> zengeld_chart::WindowState {
        let inline = &self.panel_app.toolbar_state.floating_inline_bar;
        let inline_dock_str = match inline.dock_edge {
            zengeld_chart::InlineDockEdge::Bottom => "Bottom",
            zengeld_chart::InlineDockEdge::Top => "Top",
            zengeld_chart::InlineDockEdge::Free => "Free",
        };
        let agents_tab_layout = {
            let tree = self.sidebar_state.agent_docking.inner().tree();
            uzor::panels::serialize::LayoutSnapshot::from_tree(tree, "agents")
                .to_json()
                .ok()
        };
        let agents_tab_leaves: Vec<zengeld_chart::PersistedAgentLeaf> = self
            .sidebar_state
            .agent_leaves
            .iter()
            .map(|(leaf_id, desc)| zengeld_chart::PersistedAgentLeaf {
                leaf_id: leaf_id.0,
                cli: match desc.cli {
                    gate4agent::AgentCli::Claude => zengeld_chart::PersistedAgentCli::Claude,
                    gate4agent::AgentCli::Codex => zengeld_chart::PersistedAgentCli::Codex,
                    gate4agent::AgentCli::Gemini => zengeld_chart::PersistedAgentCli::Gemini,
                    gate4agent::AgentCli::OpenCode => zengeld_chart::PersistedAgentCli::OpenCode,
                },
                mode: match desc.mode {
                    gate4agent::InstanceMode::Pty => zengeld_chart::PersistedInstanceMode::Pty,
                    gate4agent::InstanceMode::Chat => zengeld_chart::PersistedInstanceMode::Chat,
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
        zengeld_chart::WindowState {
            window_id: self.window_id.clone(),
            open_tabs: self.panel_app.open_tabs.clone(),
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
    pub fn set_profile_windows(&mut self, windows: Vec<zengeld_chart::WindowState>) {
        self.panel_app.user_manager.profile.windows = windows;
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

    /// Park the active preset's live state into the cache.
    /// Active fields are replaced with cheap placeholders.
    pub(crate) fn park_active_preset(&mut self, id: &str) {
        let state = preset_cache::LivePresetState {
            panel_grid: std::mem::replace(
                &mut self.panel_app.panel_grid,
                zengeld_chart::state::panel_grid::ChartPanelGrid::placeholder(),
            ),
            tag_manager: std::mem::replace(
                &mut self.panel_app.tag_manager,
                zengeld_chart::tag_manager::TagManager::new(),
            ),
            indicator_manager: std::mem::replace(
                &mut self.indicator_manager,
                IndicatorManager::new(),
            ),
            alert_manager: std::mem::replace(
                &mut self.alert_manager,
                alerts::AlertManager::new(),
            ),
            leaf_color_tags: std::mem::take(&mut self.panel_app.leaf_color_tags),
            indicator_overlay_states: std::mem::take(&mut self.panel_app.indicator_overlay_states),
            series_handles: std::mem::take(&mut self.series_handles),
            pending_sub_pane_ratios: std::mem::take(&mut self.pending_sub_pane_ratios),
            pending_sub_pane_above_main: std::mem::take(&mut self.pending_sub_pane_above_main),
            pending_sub_pane_order: std::mem::take(&mut self.pending_sub_pane_order),
            needs_initial_viewport_fit: self.needs_initial_viewport_fit,
            slot_dockings: std::mem::replace(
                &mut self.sidebar_state.slot_dockings,
                std::array::from_fn(|_| sidebar_content::SlotDockingManager::new()),
            ),
            panels_store: std::mem::replace(
                &mut self.panels_store,
                crate::panels_store::TradingPanelsStore::new(),
            ),
            focused_free_leaf: self.sidebar_state.focused_free_leaf.take(),
        };
        self.live_preset_cache.insert(id.to_string(), state);
        eprintln!("[ChartApp] Parked preset '{}' into live cache ({} total cached)", id, self.live_preset_cache.len());
    }

    /// Unpack a cached preset into active fields. Returns true on cache hit.
    pub(crate) fn unpark_preset(&mut self, id: &str) -> bool {
        let Some(state) = self.live_preset_cache.remove(id) else { return false; };
        self.panel_app.panel_grid = state.panel_grid;
        self.panel_app.tag_manager = state.tag_manager;
        self.indicator_manager = state.indicator_manager;
        self.alert_manager = state.alert_manager;
        self.panel_app.leaf_color_tags = state.leaf_color_tags;
        self.panel_app.indicator_overlay_states = state.indicator_overlay_states;
        self.series_handles = state.series_handles;
        self.pending_sub_pane_ratios = state.pending_sub_pane_ratios;
        self.pending_sub_pane_above_main = state.pending_sub_pane_above_main;
        self.pending_sub_pane_order = state.pending_sub_pane_order;
        self.needs_initial_viewport_fit = state.needs_initial_viewport_fit;
        self.sidebar_state.slot_dockings = state.slot_dockings;
        self.panels_store = state.panels_store;
        self.sidebar_state.focused_free_leaf = state.focused_free_leaf;
        self.sidebar_data_dirty = true;
        eprintln!("[ChartApp] Unpacked preset '{}' from live cache", id);
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
        if let Err(e) = self.panel_app.template_manager.save_to_default_dir(None) {
            eprintln!("[persist] templates: {:?}", e);
        }
    }

    /// Load and apply a previously saved user profile from `user_data/profile.json`.
    ///
    /// Also restores the [`sidebar_content::watchlist::WatchlistManager`] from
    /// `user_data/watchlists.json` when that file exists.
    ///
    /// Missing files are silently ignored so that a fresh install with no
    /// saved data still starts correctly.
    pub fn load_user_profile(&mut self) {
        // Load profile metadata.
        match zengeld_chart::load_profile(None) {
            Ok(profile) => {
                // Restore active preset id.
                self.panel_app.active_preset_id = profile.active_preset_id;

                // Restore sidebar width first (before opening, so the correct
                // width is applied when the panel is opened).
                if let Some(width) = profile.sidebar_width {
                    self.sidebar_state.set_right_width(width);
                }

                // Restore the open panel (or leave closed if None/unknown).
                if profile.sidebar_visible {
                    if let Some(panel_name) = &profile.sidebar_panel {
                        let panel = Self::str_to_panel(panel_name);
                        self.sidebar_state.set_right_panel(panel);
                    }
                }

                // Restore connector enabled/disabled state.
                if !profile.connector_enabled.is_empty() {
                    self.sidebar_state.connector_enabled = profile.connector_enabled.clone();
                }

                // Restore inline toolbar position.
                if let Some(x) = profile.inline_bar_x {
                    self.panel_app.toolbar_state.floating_inline_bar.x = x;
                }
                if let Some(y) = profile.inline_bar_y {
                    self.panel_app.toolbar_state.floating_inline_bar.y = y;
                }
                if let Some(ref dock) = profile.inline_bar_dock {
                    self.panel_app.toolbar_state.floating_inline_bar.dock_edge = match dock.as_str() {
                        "Top" => zengeld_chart::InlineDockEdge::Top,
                        "Free" => zengeld_chart::InlineDockEdge::Free,
                        _ => zengeld_chart::InlineDockEdge::Bottom,
                    };
                }
            }
            Err(e) => {
                eprintln!("[UserProfile] Failed to load profile: {}", e);
            }
        }

        // Restore watchlist manager.
        let watchlists_path = zengeld_chart::user_profile::storage::watchlists_path();
        if watchlists_path.exists() {
            match zengeld_chart::load_json::<sidebar_content::watchlist::WatchlistManager>(&watchlists_path, None) {
                Ok(manager) => {
                    self.sidebar_state.watchlist_manager = manager;
                }
                Err(e) => {
                    eprintln!("[UserProfile] Failed to load watchlists: {}", e);
                }
            }
        }
    }

    /// Load watchlists from disk without touching the user profile.
    ///
    /// Called by `new_window()` so that each window starts with the persisted
    /// watchlist state.  Profile loading (`profile.json`) is NOT performed here
    /// — that is done once at startup in `main()` and passed in via `apply_profile_state`.
    pub fn load_watchlists(&mut self) {
        let watchlists_path = zengeld_chart::user_profile::storage::watchlists_path();
        if watchlists_path.exists() {
            match zengeld_chart::load_json::<sidebar_content::watchlist::WatchlistManager>(&watchlists_path, None) {
                Ok(manager) => {
                    self.sidebar_state.watchlist_manager = manager;
                }
                Err(e) => {
                    eprintln!("[UserProfile] Failed to load watchlists: {}", e);
                }
            }
        }
    }
}
