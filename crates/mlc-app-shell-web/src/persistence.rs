//! Shell-persistence duties — the wasm sibling of the native shell's
//! `ProfileManager::load` boot sequence + `App::save_all` dirty-drain
//! (`mlc-app-shell-vello/src/main.rs`).
//!
//! Storage backend: `mlc_vault::global_storage()` → `LocalStorageBackend`
//! (self-initialized, hex-encoded values). This module adds NO new storage
//! infrastructure — it wires the same read/write calls the native shell
//! already makes through `mlc_core::user_profile::storage` /
//! `mlc_core::preset::storage`, all of which are storage-backend-agnostic.
//!
//! # Boot path (mirrors `main.rs:4928-4990`)
//!
//! 1. [`bootstrap_first_run`] — probe the profile index; if empty, create a
//!    "default" profile via `mlc_core::create_profile` (same non-UI call the
//!    welcome wizard's command handlers use).
//! 2. `ProfileManager::load(None)` (caller, `lib.rs`) — now resolves to the
//!    profile just created (or an existing one on a returning visit).
//! 3. `record_launch` + `save_profile()` (caller) — same as native.
//! 4. `ChartApp::new_window(..)` (caller) — its built-in fresh-state path
//!    creates the "Untitled 1" preset and sets `active_preset_id` when the
//!    profile has no presets yet, which is exactly the first-run case. No
//!    separate default-preset bootstrap is needed here.
//!
//! # Runtime saves (mirrors `main.rs` `save_all` / dirty-flag drain,
//! `main.rs:2128-2230` / `2743-2864`)
//!
//! [`save_persistent_state`] is the wasm-shaped equivalent: drain
//! `preset_actions` into the shared core, then write profile.json,
//! watchlists.json, templates, settings snapshots and all presets to
//! `global_storage()`. Native gates this behind per-window dirty flags
//! (`profile_dirty` / `profile_geometry_dirty` / `watchlists_dirty`); wasm
//! has exactly one `ChartApp`, so the caller checks those same flags
//! (shared fields on `ChartApp`, already set by shared input-handling code)
//! before calling this function once per RAF frame — see `lib.rs`'s render
//! loop. `flush_pending_autosave` (already wired, `mlc-app/src/frame_lifecycle/tick.rs`)
//! separately persists the active preset's live edits every tick; this
//! module additionally covers `profile.json` + watchlists + templates +
//! snapshots + the full preset set, none of which shell-web wrote before.

use mlc_app::ChartApp;

/// First-run bootstrap: create a default profile + index entry if none
/// exists in storage yet.
///
/// Mirrors the vello `is_first_run` probe (`main.rs:4928-4953`) but skips
/// the welcome-wizard UI entirely — wasm v1 is single-user, so a fresh
/// browser profile always gets one profile named "default" with no
/// interactive setup. `ChartApp::new_window`'s fresh-state path (no saved
/// presets) then creates the first "Untitled 1" chart + preset on its own,
/// which is what makes `active_preset_id` non-empty and turns on autosave.
///
/// No-op (returns `false`) if a profile index with at least one profile
/// already exists — the returning-visitor path.
pub fn bootstrap_first_run() -> bool {
    let is_first_run = match mlc_core::load_profile_index() {
        Some(index) => index.profiles.is_empty(),
        None => true,
    };
    if !is_first_run {
        return false;
    }
    match mlc_core::create_profile("default", "chart") {
        Ok(meta) => {
            web_sys::console::log_1(
                &format!("[mlc-app-shell-web] first-run bootstrap: created profile '{}' ({})",
                    meta.display_name, meta.id).into(),
            );
            true
        }
        Err(e) => {
            web_sys::console::error_1(
                &format!("[mlc-app-shell-web] first-run bootstrap failed: {}", e).into(),
            );
            false
        }
    }
}

/// Drain queued preset actions into the shared core, then persist
/// profile.json, watchlists, templates, settings snapshots and every
/// preset to `global_storage()`.
///
/// Equivalent of native `App::save_all` (`main.rs:2128-2230`), minus the
/// concepts that don't exist on a single-canvas wasm page (OS window
/// geometry sync, multi-window `last_focused` preference, `AppState`
/// theme/device-name fields — shell-web has no `AppState`).
pub fn save_persistent_state(chart: &mut ChartApp) {
    // 1. Drain PresetAction queue (populated by autosave_snapshot / preset
    //    edit handlers) into the shared core's in-memory preset registry.
    //    PresetManager::insert is RAM-only — the disk write happens in step 5.
    for action in chart.preset_actions.drain(..) {
        match action {
            mlc_app::PresetAction::Upsert(preset) => {
                let id = preset.id.clone();
                chart.core.presets_mut().insert(id, preset);
            }
            mlc_app::PresetAction::Delete { id } => {
                chart.core.presets_mut().remove(&id);
            }
            mlc_app::PresetAction::Rename { id, new_name } => {
                if let Some(p) = chart.core.presets_mut().get_mut(&id) {
                    p.name = new_name;
                }
            }
        }
    }

    // 2. Build the window state for this shell's one window and fold it
    //    into a profile snapshot (base = the live profile already held in
    //    core.user(), same source native reads via BackendCore).
    let window_state = chart.build_window_state();
    let mut profile = chart.core.user().profile().clone();
    profile.ui.windows = vec![window_state];
    // Shell duty (native parity: main.rs profile save writes
    // `profile.ui.active_theme` from AppState.theme_preset): persist the
    // active theme preset so a reload restores it. `preset_name()` is the
    // base preset id ("dark"/"surf"/...), always non-empty.
    profile.ui.active_theme = chart.panel_app.theme_manager.preset_name().to_string();
    profile.ui.active_ui_style = chart.panel_app.theme_manager.current().style.as_name().to_string();
    profile.domain.connector_enabled = chart.core.user().connector_enabled().clone();
    profile.domain.server_enabled = chart.core.user().server_enabled();
    profile.domain.server_port = chart.core.user().server_port();
    profile.ui.sidebar_visible = chart.sidebar_state.is_right_open();
    profile.ui.sidebar_panel = ChartApp::panel_to_str(chart.sidebar_state.right_panel);
    profile.ui.sidebar_width = Some(chart.sidebar_state.right_sidebar_width);
    let inline = &chart.panel_app.toolbar_state.floating_inline_bar;
    profile.ui.inline_bar_x = Some(inline.x);
    profile.ui.inline_bar_y = Some(inline.y);
    profile.ui.inline_bar_dock = Some(match inline.dock_edge {
        mlc_core::InlineDockEdge::Bottom => "Bottom".to_string(),
        mlc_core::InlineDockEdge::Top => "Top".to_string(),
        mlc_core::InlineDockEdge::Free => "Free".to_string(),
    });
    // Favorites quick-pick bar position + visibility (favorite-primitives
    // arc) — same carry as the inline bar above. The `favorite_primitives`
    // vec itself needs no explicit line: `profile` above is a clone of the
    // live core profile, which `ToggleFavoritePrimitive` mutates in place.
    let fav = &chart.panel_app.toolbar_state.favorites_bar;
    profile.ui.favorites_bar_x = Some(fav.x);
    profile.ui.favorites_bar_y = Some(fav.y);
    profile.ui.favorites_bar_dock = Some(match fav.dock_edge {
        mlc_core::InlineDockEdge::Bottom => "Bottom".to_string(),
        mlc_core::InlineDockEdge::Top => "Top".to_string(),
        mlc_core::InlineDockEdge::Free => "Free".to_string(),
    });
    profile.ui.favorites_bar_visible = chart.panel_app.toolbar_state.favorites_bar_visible;
    profile.ui.persist_toolbar_last_tools = chart.core.user().profile().ui.persist_toolbar_last_tools;
    profile.ui.persist_toolbar_toggles = chart.core.user().profile().ui.persist_toolbar_toggles;
    if profile.ui.persist_toolbar_last_tools {
        profile.ui.toolbar_last_tools = chart.panel_app.toolbar_state.quick_select_memory
            .iter()
            .map(|(k, (t, _))| (k.clone(), t.clone()))
            .collect();
    }
    if profile.ui.persist_toolbar_toggles {
        profile.ui.drawings_locked = chart.panel_app.toolbar_state.drawings_locked;
        profile.ui.drawings_visible = chart.panel_app.toolbar_state.drawings_visible;
    }
    profile.domain.notification_settings =
        chart.panel_app.alert_settings_state.notification_settings.clone();

    // 3. Save profile.json — always plaintext, no vault key required.
    let vault_key = chart.core.user().vault_key();
    match mlc_core::save_profile(&profile, vault_key.as_ref()) {
        Ok(()) => {
            // Guard rule: hoist the read BEFORE user_mut() — the receiver
            // (write guard) is evaluated before the argument expressions,
            // so a `user()` read inside the arg list is a same-thread
            // read-under-write on one RwLock = instant wasm abort.
            let snapshots = chart.core.user().snapshots().clone();
            chart.core.user_mut().replace_all(
                profile,
                snapshots,
                vault_key,
            );
        }
        Err(e) => {
            web_sys::console::error_1(
                &format!("[mlc-app-shell-web] failed to save profile: {}", e).into(),
            );
        }
    }

    // 4. Save watchlists — always plaintext.
    {
        let watchlists_path = mlc_core::user_profile::storage::watchlists_path();
        if let Err(e) = mlc_core::save_json(&watchlists_path, &chart.core.watchlists().snapshot(), None) {
            web_sys::console::error_1(
                &format!("[mlc-app-shell-web] failed to save watchlists: {}", e).into(),
            );
        }
    }

    // 5. Save settings snapshots — always plaintext.
    {
        let path = mlc_core::active_profile_data_dir().join("settings_snapshots.json");
        if let Err(e) = mlc_core::save_json(&path, chart.core.user().snapshots(), None) {
            web_sys::console::error_1(
                &format!("[mlc-app-shell-web] failed to save settings snapshots: {}", e).into(),
            );
        }
    }

    // 6. Save templates — single canonical source of truth in the shared core.
    let vault_key = chart.core.user().vault_key();
    if let Err(e) = chart.core.templates().save_to_default_dir(vault_key.as_ref()) {
        web_sys::console::error_1(
            &format!("[mlc-app-shell-web] failed to save templates: {:?}", e).into(),
        );
    }

    // 7. Save every preset — single canonical source of truth in the shared core.
    for preset in chart.core.presets().values() {
        if let Err(e) = mlc_core::preset::storage::save_preset(preset) {
            web_sys::console::error_1(
                &format!("[mlc-app-shell-web] failed to save preset {}: {}", preset.id, e).into(),
            );
        }
    }

    // Clear dirty flags now that everything reached storage.
    chart.profile_dirty = false;
    chart.profile_geometry_dirty = false;
    chart.watchlists_dirty = false;
}

/// Request persistent storage from the browser (best-effort — eviction
/// under storage pressure is otherwise allowed at any time). Fire-and-log,
/// non-blocking; the boot sequence does not wait on the result.
pub fn request_storage_persistence() {
    let Some(win) = web_sys::window() else { return; };
    let navigator = win.navigator();
    let storage = navigator.storage();
    let promise = match storage.persist() {
        Ok(p) => p,
        Err(e) => {
            web_sys::console::warn_1(
                &format!("[mlc-app-shell-web] navigator.storage.persist() call failed: {:?}", e).into(),
            );
            return;
        }
    };
    wasm_bindgen_futures::spawn_local(async move {
        match wasm_bindgen_futures::JsFuture::from(promise).await {
            Ok(granted) => {
                let granted = granted.as_bool().unwrap_or(false);
                web_sys::console::log_1(
                    &format!("[mlc-app-shell-web] navigator.storage.persist() granted={}", granted).into(),
                );
            }
            Err(e) => {
                web_sys::console::warn_1(
                    &format!("[mlc-app-shell-web] navigator.storage.persist() rejected: {:?}", e).into(),
                );
            }
        }
    });
}
