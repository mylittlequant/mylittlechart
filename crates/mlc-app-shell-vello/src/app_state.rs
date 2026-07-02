//! Cross-window shared application state.
//!
//! Shrunk to three survivor fields by the shared-BackendCore arc
//! (`docs/mlc/plans/shared-backend-core-2026-07-03.md` §2.3, Phase B+C):
//! every domain field (`watchlist_manager`/`presets`/`preset_dirty_ids`/
//! `snapshots`/`template_manager`, then `connector_enabled`/`recalc_mode`/
//! `scale_mode`/`server_enabled`/`server_port`/`vault_key`) and their dirty
//! flags are gone — every window's `ChartApp::core` is now the SAME
//! `Arc<BackendCore>`, so there is no longer a second copy to keep in sync.
//! Domain reads/writes go through `core.presets()`/`core.templates()`/
//! `core.watchlists()`/`core.user()` directly. `watchlist_ui` also died
//! from here (Phase B verdict: per-window presentation, already lives on
//! `ChartApp.watchlist_ui`).
//!
//! What remains is genuinely process-wide but NOT domain: visual chrome
//! (`theme_preset`) and read-only-after-startup process identity
//! (`device_name`, `app_version`).

/// Application-level shared state — the three fields that are legitimately
/// process-wide but do NOT belong on `BackendCore`. See the module doc for
/// what left this struct and why.
pub(crate) struct AppState {
    /// Active theme preset name (e.g. "dark", "light").
    /// Single source of truth — synced to all windows each frame.
    pub(crate) theme_preset: String,
    /// Device identity — read-only after startup, shared to avoid stale per-window copies.
    pub(crate) device_name: String,
    pub(crate) app_version: String,
}

impl AppState {
    /// Initialise from a loaded user profile. Domain data (presets,
    /// templates, watchlists, snapshots, connector_enabled, recalc_mode,
    /// scale_mode, server_enabled, server_port, vault_key) is no longer
    /// threaded through here — the caller seeds `core` directly (see
    /// `main.rs::seed_core_from_profile`).
    pub(crate) fn from_profile(profile: &mlc_core::UserProfile) -> Self {
        Self {
            theme_preset: profile.ui.active_theme.clone(),
            device_name: profile.ui.device_name.clone(),
            app_version: profile.domain.app_version.clone(),
        }
    }
}
