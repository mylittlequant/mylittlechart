//! [`UserProfileManager`] — profile + settings-snapshots + vault-key registry.
//!
//! Promoted from `mlc-app::user_manager::{UserManager, ProfileManager}` /
//! `mlc-app-shell-vello::AppState` per doctrine 1B: a headless daemon UI
//! needs `UserProfile` (bar_count, data_load, recalc_mode, scale_mode, ...)
//! to request the correct amount of data and apply the same behaviour a
//! native window would — this is domain state, not presentation, even
//! though `UserProfile` also carries presentation fields (sidebar width,
//! inline-bar position). Splitting `UserProfile` field-by-field is future
//! work (§1B "mixed structs split by field") — out of scope for this step;
//! the whole value type moves here unsplit, mirroring how `ChartPreset` and
//! `WatchlistList` are whole value types owned by their manager.
//!
//! `vault_key` ownership moves here too, but semantics are unchanged: still
//! `Option<VaultKey>`, still set post-hoc after passphrase entry. See
//! `docs/mlc/plans/doctrine-cutover-2026-06-27.md` and the step 3.6c agent
//! report for the pre-existing bootstrap-order gap this does NOT fix
//! (`AppState`/`ProfileManager` remain the write authority for encrypted
//! saves in the shell-vello multi-window flow; see report for detail).

use std::collections::HashMap;

use mlc_backend_indicator_engine::RecalcMode;
use mlc_core::chart::types::price_scale::ScaleMode;
use mlc_core::user_profile::{SettingsSnapshots, UserProfile};
use mlc_vault::vault::VaultKey;

/// Profile metadata + settings snapshots + vault key for one active user
/// session. One instance lives on `BackendCore`; per-window duplication is
/// what this promotion removes (step 3.6c, following 3.6a/3.6b/3.6d).
///
/// `connector_enabled`/`recalc_mode`/`scale_mode`/`server_enabled`/
/// `server_port` (shared-BackendCore arc Phase C,
/// `docs/mlc/plans/shared-backend-core-2026-07-03.md` §2.1) are the
/// strongly-typed runtime caches of the same-named `profile.domain.*`
/// fields — kept as separate fields (not re-parsed from `profile` on every
/// read) mirroring how `vault_key`/`snapshots` already sit alongside
/// `profile` rather than inside it. `AppState` used to own these; there is
/// now exactly one process-wide copy here instead of one-per-window.
#[derive(Debug, Clone, Default)]
pub struct UserProfileManager {
    profile: UserProfile,
    snapshots: SettingsSnapshots,
    vault_key: Option<VaultKey>,
    connector_enabled: HashMap<String, bool>,
    recalc_mode: RecalcMode,
    scale_mode: ScaleMode,
    server_enabled: bool,
    server_port: u16,
}

impl UserProfileManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct directly from already-loaded parts (startup wiring).
    pub fn from_parts(profile: UserProfile, snapshots: SettingsSnapshots, vault_key: Option<VaultKey>) -> Self {
        Self { profile, snapshots, vault_key, ..Default::default() }
    }

    // =========================================================================
    // Profile
    // =========================================================================

    pub fn profile(&self) -> &UserProfile {
        &self.profile
    }

    pub fn profile_mut(&mut self) -> &mut UserProfile {
        &mut self.profile
    }

    pub fn set_profile(&mut self, profile: UserProfile) {
        self.profile = profile;
    }

    // =========================================================================
    // Settings snapshots
    // =========================================================================

    pub fn snapshots(&self) -> &SettingsSnapshots {
        &self.snapshots
    }

    pub fn snapshots_mut(&mut self) -> &mut SettingsSnapshots {
        &mut self.snapshots
    }

    pub fn set_snapshots(&mut self, snapshots: SettingsSnapshots) {
        self.snapshots = snapshots;
    }

    // =========================================================================
    // Vault key
    // =========================================================================

    pub fn vault_key(&self) -> Option<VaultKey> {
        self.vault_key
    }

    pub fn set_vault_key(&mut self, key: Option<VaultKey>) {
        self.vault_key = key;
    }

    /// Persist the profile to disk using the current vault key.
    pub fn save_profile(&self) -> Result<(), mlc_core::user_profile::storage::ProfileError> {
        mlc_core::user_profile::storage::save_profile(&self.profile, self.vault_key.as_ref())
    }

    /// Replace the whole manager wholesale (startup load, profile switch).
    pub fn replace_all(&mut self, profile: UserProfile, snapshots: SettingsSnapshots, vault_key: Option<VaultKey>) {
        self.profile = profile;
        self.snapshots = snapshots;
        self.vault_key = vault_key;
    }

    // =========================================================================
    // Per-exchange connector enable/disable (Phase C)
    // =========================================================================

    pub fn connector_enabled(&self) -> &HashMap<String, bool> {
        &self.connector_enabled
    }

    pub fn set_connector_enabled(&mut self, map: HashMap<String, bool>) {
        self.connector_enabled = map;
    }

    pub fn is_connector_enabled(&self, exchange_key: &str) -> bool {
        self.connector_enabled.get(exchange_key).copied().unwrap_or(true)
    }

    pub fn set_connector_enabled_one(&mut self, exchange_key: String, enabled: bool) {
        self.connector_enabled.insert(exchange_key, enabled);
    }

    // =========================================================================
    // Performance settings (Phase C)
    // =========================================================================

    pub fn recalc_mode(&self) -> RecalcMode {
        self.recalc_mode
    }

    pub fn set_recalc_mode(&mut self, mode: RecalcMode) {
        self.recalc_mode = mode;
    }

    pub fn scale_mode(&self) -> ScaleMode {
        self.scale_mode
    }

    pub fn set_scale_mode(&mut self, mode: ScaleMode) {
        self.scale_mode = mode;
    }

    // =========================================================================
    // Agent API server settings (Phase C)
    // =========================================================================

    pub fn server_enabled(&self) -> bool {
        self.server_enabled
    }

    pub fn set_server_enabled(&mut self, enabled: bool) {
        self.server_enabled = enabled;
    }

    pub fn server_port(&self) -> u16 {
        self.server_port
    }

    pub fn set_server_port(&mut self, port: u16) {
        self.server_port = port;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty_profile() {
        let mgr = UserProfileManager::new();
        assert!(mgr.vault_key().is_none());
        assert_eq!(mgr.profile().ui.active_preset_id, "");
    }

    #[test]
    fn set_profile_and_snapshots_roundtrip() {
        let mut mgr = UserProfileManager::new();
        let mut profile = UserProfile::new();
        profile.ui.active_preset_id = "preset_1".to_string();
        mgr.set_profile(profile);
        assert_eq!(mgr.profile().ui.active_preset_id, "preset_1");

        let mut snaps = SettingsSnapshots::default();
        snaps.chart_settings = Some(serde_json::json!({"x": 1}));
        mgr.set_snapshots(snaps);
        assert!(mgr.snapshots().chart_settings.is_some());
    }

    #[test]
    fn replace_all_swaps_everything() {
        let mut mgr = UserProfileManager::new();
        let mut profile = UserProfile::new();
        profile.ui.active_preset_id = "p2".to_string();
        mgr.replace_all(profile, SettingsSnapshots::default(), None);
        assert_eq!(mgr.profile().ui.active_preset_id, "p2");
        assert!(mgr.vault_key().is_none());
    }

    // ── Phase C: connector_enabled / recalc_mode / scale_mode / server ────
    // settings, promoted from shell-vello's per-window AppState mirror
    // (`docs/mlc/plans/shared-backend-core-2026-07-03.md` §2.1).

    #[test]
    fn connector_enabled_defaults_to_true_when_unset() {
        let mgr = UserProfileManager::new();
        assert!(mgr.is_connector_enabled("binance"), "unknown exchange must default to enabled");
    }

    #[test]
    fn connector_enabled_one_and_bulk_set_roundtrip() {
        let mut mgr = UserProfileManager::new();
        mgr.set_connector_enabled_one("binance".to_string(), false);
        assert!(!mgr.is_connector_enabled("binance"));
        assert!(mgr.is_connector_enabled("bybit"), "unrelated exchange untouched");

        let mut map = HashMap::new();
        map.insert("okx".to_string(), false);
        mgr.set_connector_enabled(map);
        assert!(!mgr.is_connector_enabled("okx"));
        // Bulk set replaces wholesale — the earlier binance=false entry is gone.
        assert!(mgr.is_connector_enabled("binance"));
    }

    #[test]
    fn recalc_mode_and_scale_mode_roundtrip() {
        let mut mgr = UserProfileManager::new();
        assert_eq!(mgr.recalc_mode(), RecalcMode::default());
        mgr.set_recalc_mode(RecalcMode::PerTick);
        assert_eq!(mgr.recalc_mode(), RecalcMode::PerTick);

        assert_eq!(mgr.scale_mode(), ScaleMode::default());
        mgr.set_scale_mode(ScaleMode::Manual);
        assert_eq!(mgr.scale_mode(), ScaleMode::Manual);
    }

    #[test]
    fn server_settings_roundtrip() {
        let mut mgr = UserProfileManager::new();
        assert!(!mgr.server_enabled());
        assert_eq!(mgr.server_port(), 0);

        mgr.set_server_enabled(true);
        mgr.set_server_port(19460);
        assert!(mgr.server_enabled());
        assert_eq!(mgr.server_port(), 19460);
    }
}
