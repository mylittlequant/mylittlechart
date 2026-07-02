//! Cross-window shared application state.
//!
//! Shrunk by the shared-BackendCore arc (Phase B):
//! `watchlist_manager`/`presets`/`preset_dirty_ids`/`snapshots`/
//! `template_manager` and their dirty flags are gone — every window's
//! `ChartApp::core` is now the SAME `Arc<BackendCore>`, so there is no
//! longer a second copy to keep in sync. Domain reads/writes go through
//! `core.presets()`/`core.templates()`/`core.watchlists()`/`core.user()`
//! directly. `watchlist_ui` also dies from here (see the Phase-B verdict in
//! `docs/mlc/plans/shared-backend-core-2026-07-03.md`'s coordinator
//! follow-up: it is per-window presentation, already lives correctly on
//! `ChartApp.watchlist_ui`, and the agent-API snapshot builder
//! (`mlc-app-agent-api/src/snapshots.rs::update_watchlist_snapshot`)
//! already reads it that way — this `AppState` copy was a redundant
//! mirror, never the real source of truth).

/// Application-level shared state — single source of truth for data that is
/// legitimately process-wide but does NOT belong on `BackendCore` (visual
/// chrome, connector on/off flags, performance settings, vault key). See the
/// module doc for what left this struct and why.
pub(crate) struct AppState {
    /// Per-exchange enabled/disabled flag (keyed by `ExchangeId::as_str()`).
    pub(crate) connector_enabled: std::collections::HashMap<String, bool>,
    /// Active theme preset name (e.g. "dark", "light").
    /// Single source of truth — synced to all windows each frame.
    pub(crate) theme_preset: String,
    /// Device identity — read-only after startup, shared to avoid stale per-window copies.
    pub(crate) device_name: String,
    pub(crate) app_version: String,

    // ── Sync dirty flags ──────────────────────────────────────────────────────
    // Set to `true` when the corresponding data changes; reset after syncing to
    // all windows.  Prevents per-frame deep clones when nothing changed.
    /// Connector enabled map changed — need to clone to all windows.
    pub(crate) connectors_dirty: bool,

    // ── Performance settings ──────────────────────────────────────────────────

    /// Indicator recalculation mode — controls CPU/accuracy trade-off.
    /// Synced to every window's `indicator_manager.recalc_mode` each frame.
    pub(crate) recalc_mode: mlc_app::RecalcMode,

    /// User's preferred price scale mode (Auto / Focus / Manual).
    /// Applied as the default when windows load bars for the first time.
    pub(crate) scale_mode: mlc_core::ScaleMode,

    // ── Agent API server settings ────────────────────────────────────────────
    /// Whether the server is enabled.
    pub(crate) server_enabled: bool,
    /// Port the server listens on.
    pub(crate) server_port: u16,

    /// Encryption key for zero-trust storage. Derived from passphrase at startup.
    /// `None` during migration or when running without a passphrase (plaintext mode).
    pub(crate) vault_key: Option<mlc_vault::vault::VaultKey>,
}

impl AppState {
    /// Initialise from a loaded user profile. Domain data (presets,
    /// templates, watchlists, snapshots) is no longer threaded through here
    /// — the caller seeds `core` directly (see `main.rs::seed_core_from_profile`).
    /// `vault_key` is still a plain constructor param — Phase C moves it to
    /// `core.user()` alongside the rest of the user-domain fields; not this
    /// phase's scope.
    pub(crate) fn from_profile(
        profile: &mlc_core::UserProfile,
        vault_key: Option<mlc_vault::vault::VaultKey>,
    ) -> Self {
        // Parse recalc_mode from DeviceSettings (authoritative) with profile as fallback.
        let recalc_mode = {
            let ds = mlc_core::user_profile::DeviceSettings::load();
            let src = if ds.recalc_mode == "per_frame" && !profile.domain.recalc_mode.is_empty() {
                // DeviceSettings has the default value — use the profile value if it
                // carries a non-default mode so that existing saves are honoured.
                &profile.domain.recalc_mode
            } else {
                &ds.recalc_mode
            };
            match src.as_str() {
                "PerTick" => mlc_app::RecalcMode::PerTick,
                "PerBar"  => mlc_app::RecalcMode::PerBar,
                _         => mlc_app::RecalcMode::PerFrame,
            }
        };

        // Parse scale_mode from the profile string.
        let scale_mode = match profile.domain.scale_mode.as_str() {
            "Focus"  => mlc_core::ScaleMode::Focus,
            "Manual" => mlc_core::ScaleMode::Manual,
            _        => mlc_core::ScaleMode::Auto, // default / "Auto"
        };

        Self {
            connector_enabled: profile.domain.connector_enabled.clone(),
            theme_preset: profile.ui.active_theme.clone(),
            device_name: profile.ui.device_name.clone(),
            app_version: profile.domain.app_version.clone(),
            // Start dirty so the first frame syncs everything to all windows.
            connectors_dirty: true,
            recalc_mode,
            scale_mode,
            server_enabled: profile.domain.server_enabled,
            server_port: profile.domain.server_port,
            vault_key,
        }
    }
}
