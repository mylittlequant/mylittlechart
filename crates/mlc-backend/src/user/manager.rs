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

use mlc_core::user_profile::{SettingsSnapshots, UserProfile};
use mlc_vault::vault::VaultKey;

/// Profile metadata + settings snapshots + vault key for one active user
/// session. One instance lives on `BackendCore`; per-window duplication is
/// what this promotion removes (step 3.6c, following 3.6a/3.6b/3.6d).
#[derive(Debug, Clone, Default)]
pub struct UserProfileManager {
    profile: UserProfile,
    snapshots: SettingsSnapshots,
    vault_key: Option<VaultKey>,
}

impl UserProfileManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct directly from already-loaded parts (startup wiring).
    pub fn from_parts(profile: UserProfile, snapshots: SettingsSnapshots, vault_key: Option<VaultKey>) -> Self {
        Self { profile, snapshots, vault_key }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty_profile() {
        let mgr = UserProfileManager::new();
        assert!(mgr.vault_key().is_none());
        assert_eq!(mgr.profile().active_preset_id, "");
    }

    #[test]
    fn set_profile_and_snapshots_roundtrip() {
        let mut mgr = UserProfileManager::new();
        let mut profile = UserProfile::new();
        profile.active_preset_id = "preset_1".to_string();
        mgr.set_profile(profile);
        assert_eq!(mgr.profile().active_preset_id, "preset_1");

        let mut snaps = SettingsSnapshots::default();
        snaps.chart_settings = Some(serde_json::json!({"x": 1}));
        mgr.set_snapshots(snaps);
        assert!(mgr.snapshots().chart_settings.is_some());
    }

    #[test]
    fn replace_all_swaps_everything() {
        let mut mgr = UserProfileManager::new();
        let mut profile = UserProfile::new();
        profile.active_preset_id = "p2".to_string();
        mgr.replace_all(profile, SettingsSnapshots::default(), None);
        assert_eq!(mgr.profile().active_preset_id, "p2");
        assert!(mgr.vault_key().is_none());
    }
}
