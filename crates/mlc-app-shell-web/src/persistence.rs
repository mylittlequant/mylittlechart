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
//!    nameless profile via `mlc_core::create_profile` (same non-UI call the
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

use crate::perf::{now_ms, PerfMeter};
use mlc_app::ChartApp;

/// First-run bootstrap: create a default profile + index entry if none
/// exists in storage yet.
///
/// Mirrors the vello `is_first_run` probe (`main.rs:4928-4953`), but unlike
/// native this shell has no "skeleton" state to defer profile creation
/// through — it always creates a real profile immediately so the app is
/// never profile-less. `ChartApp::new_window`'s fresh-state path (no saved
/// presets) then creates the first "Untitled 1" chart + preset on its own,
/// which is what makes `active_preset_id` non-empty and turns on autosave.
///
/// The profile is created **nameless** (owner's identity ruling 2026-09-02):
/// a profile is a local workspace whose identity is its UUID, and an
/// anonymous user has exactly one of them and needs no user-facing name for
/// it. A nickname is an ACCOUNT attribute that arrives with OAuth. This used
/// to pass the literal `"default"` — a lower-case second copy of the
/// `"Default"` that `ProfileUiSettings` carried, which is why clearing the
/// one in `mlc-core` left the web shell still stamping a name on every
/// first-run profile.
///
/// Owner's ruling 2026-08-31: the caller uses the `true` return (a profile
/// was just created from nothing) to raise the welcome wizard (theme →
/// language) instead of staying silent about it — dismissing the wizard is a
/// courtesy and keeps the profile created here.
///
/// No-op (returns `false`) if a profile index with at least one profile
/// already exists — the returning-visitor path, which never sees the wizard.
pub fn bootstrap_first_run() -> bool {
    let is_first_run = match mlc_core::load_profile_index() {
        Some(index) => index.profiles.is_empty(),
        None => true,
    };
    if !is_first_run {
        return false;
    }
    match mlc_core::create_profile("", "chart") {
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
///
/// Reports its own duration + successful write count into `perf` (wasm-only
/// perf arc, 2026-09-05 — see `perf.rs` module doc): this is the exact call
/// the owner's H1 hypothesis names (a click's dirty flags triggering a
/// same-frame write of EVERY preset, not just the one that changed). Bytes
/// are deliberately NOT counted — that would be a second serialization pass
/// over the very data this function is suspected of serializing too much of,
/// and an instrument must not inflate the thing it measures.
pub fn save_persistent_state(chart: &mut ChartApp, perf: &mut PerfMeter) {
    let perf_start_ms = now_ms();
    let mut writes = 0usize;

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
    // Screenshot destination (screenshot-destination arc) — live toolbar-
    // state value, same carry as the favorites bar above; the camera
    // button's destination dropdown is the only writer.
    profile.ui.screenshot_destination = chart.panel_app.toolbar_state.screenshot_destination;
    // Community modal per-tab size (community-modal chrome arc) — live
    // CommunityState value, same carry as the favorites bar above.
    #[cfg(feature = "cloud")]
    {
        profile.ui.community_forum_size = chart.panel_app.community_state.forum_size;
        profile.ui.community_messages_size = chart.panel_app.community_state.messages_size;
    }
    // System event log (system-event-bus arc, 2026-09-05) — same carry as
    // the screenshot destination above: `chart.system_events()` is the live
    // value, mutated in place by `push_system_event`/
    // `mark_all_system_events_read`, not by this snapshot.
    profile.ui.system_events = chart.system_events().clone();
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
    // Secrets only follow when the vault was unlocked this session — see
    // the native shell's `save_all` for the full reasoning (mirrored here
    // for shell-duty parity: native/wasm must agree on notification
    // persistence, same rule as every other shell-duty carried in
    // `apply_profile_state`).
    if chart.panel_app.alert_settings_state.secrets_available {
        profile.domain.notification_secrets =
            chart.panel_app.alert_settings_state.notification_secrets.clone();
    }

    // 3. Save profile.json — always plaintext, no vault key required.
    let vault_key = chart.core.user().vault_key();
    match mlc_core::save_profile(&profile, vault_key.as_ref()) {
        Ok(()) => {
            writes += 1;
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
        let snapshot = chart.core.watchlists().snapshot();
        match mlc_core::save_json(&watchlists_path, &snapshot, None) {
            Ok(()) => {
                writes += 1;
            }
            Err(e) => {
                web_sys::console::error_1(
                    &format!("[mlc-app-shell-web] failed to save watchlists: {}", e).into(),
                );
            }
        }
    }

    // 5. Save settings snapshots — always plaintext.
    {
        let path = mlc_core::active_profile_data_dir().join("settings_snapshots.json");
        let user = chart.core.user();
        let snapshots = user.snapshots();
        match mlc_core::save_json(&path, snapshots, None) {
            Ok(()) => {
                writes += 1;
            }
            Err(e) => {
                web_sys::console::error_1(
                    &format!("[mlc-app-shell-web] failed to save settings snapshots: {}", e).into(),
                );
            }
        }
    }

    // 6. Save templates — single canonical source of truth in the shared core.
    let vault_key = chart.core.user().vault_key();
    {
        let templates = chart.core.templates();
        match templates.save_to_default_dir(vault_key.as_ref()) {
            Ok(()) => {
                writes += 1;
            }
            Err(e) => {
                web_sys::console::error_1(
                    &format!("[mlc-app-shell-web] failed to save templates: {:?}", e).into(),
                );
            }
        }
    }

    // 7. Save every preset — single canonical source of truth in the shared
    //    core. THE call H1 names: one `save_preset` (one `localStorage.setItem`
    //    per the module doc's write-up) for every preset in the registry, not
    //    just the one a click just touched.
    for preset in chart.core.presets().values() {
        // A shared/read-only viewer preset (`ChartApp::is_readonly_tab`)
        // never reaches local preset storage — it is not the visitor's own
        // asset, and persisting it would let it survive a reload as an
        // ordinary WRITABLE tab (`readonly_preset_ids` is RAM-only, reset on
        // every fresh `ChartApp::new_window`), silently losing its
        // read-only status. `apply_cloned_preset` never marks its id
        // readonly, so a clone saves normally like any other preset.
        if chart.is_readonly_tab(&preset.id) {
            continue;
        }
        match mlc_core::preset::storage::save_preset(preset) {
            Ok(_path) => {
                writes += 1;
            }
            Err(e) => {
                web_sys::console::error_1(
                    &format!("[mlc-app-shell-web] failed to save preset {}: {}", preset.id, e).into(),
                );
            }
        }
    }

    // Clear dirty flags now that everything reached storage.
    chart.profile_dirty = false;
    chart.profile_geometry_dirty = false;
    chart.watchlists_dirty = false;

    perf.record_persist(now_ms() - perf_start_ms, writes);
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
