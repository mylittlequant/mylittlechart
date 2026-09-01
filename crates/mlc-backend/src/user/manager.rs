//! [`UserProfileManager`] — profile + settings-snapshots + vault-key registry.
//!
//! Promoted from `mlc-app::user_manager::{UserManager, ProfileManager}` /
//! `mlc-app-shell-vello::AppState` per doctrine 1B: a headless daemon UI
//! needs `UserProfile` (bar_count, data_load, recalc_mode, ...)
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
use mlc_core::user_profile::{SettingsSnapshots, UserProfile, VaultSecrets};
use mlc_vault::vault::VaultKey;

/// Profile metadata + settings snapshots + vault key for one active user
/// session. One instance lives on `BackendCore`; per-window duplication is
/// what this promotion removes (step 3.6c, following 3.6a/3.6b/3.6d).
///
/// `connector_enabled`/`recalc_mode`/`server_enabled`/
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

    /// Deliberately seal the vault mid-session: drop the key and wipe the
    /// secret sub-fields of every live in-memory secret-bearing struct.
    ///
    /// Dropping `vault_key` alone is not a real seal — it only gates the
    /// read side (`secrets_available`-style checks); the plaintext secret
    /// bytes would still sit in `self.profile` in RAM regardless, readable
    /// by anything that skipped the gate. This wipes them for real.
    ///
    /// `exchange_keys` entries keep `exchange_id`/`label`/`api_key`/
    /// `testnet`/`created_at` — [`crate::user_profile::profile::
    /// StoredExchangeKey`]'s own doc calls `api_key` "not a secret; safe to
    /// store in profile.json", and identity fields are exactly what a
    /// sealed credentials list still shows by name. Only `api_secret` and
    /// `passphrase` are redacted. `keychain_ref`/`passphrase_keychain_ref`
    /// are untouched — a [`crate::user_profile::profile::CredentialRef`] is
    /// a keychain lookup pointer (service + account strings), never the
    /// secret itself.
    pub fn seal_vault(&mut self) {
        self.vault_key = None;
        for key in &mut self.profile.domain.exchange_keys {
            key.api_secret.clear();
            key.passphrase = None;
        }
        self.profile.domain.notification_secrets = Default::default();
    }

    /// Re-derive live secret state after a mid-session unlock: set the key
    /// and merge the freshly-decrypted `vault.enc` payload back into the
    /// live profile.
    ///
    /// Counterpart to [`Self::seal_vault`]. Deliberately does NOT touch
    /// anything else (windows, presets, panel state) — the caller already
    /// has a fully live session; re-deriving the key must not rebuild it,
    /// only restore what sealing redacted.
    pub fn unseal_vault(&mut self, key: VaultKey, secrets: VaultSecrets) {
        self.vault_key = Some(key);
        secrets.merge_into(&mut self.profile);
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

        let snaps = SettingsSnapshots {
            chart_settings: Some(serde_json::json!({"x": 1})),
            ..Default::default()
        };
        mgr.set_snapshots(snaps);
        assert!(mgr.snapshots().chart_settings.is_some());
    }

    #[test]
    fn seal_vault_drops_key_and_redacts_secrets() {
        use mlc_core::user_profile::profile::StoredExchangeKey;

        let mut mgr = UserProfileManager::new();
        mgr.set_vault_key(Some([7u8; 32]));
        mgr.profile_mut().domain.exchange_keys.push(StoredExchangeKey {
            exchange_id: "binance".to_string(),
            label: "main".to_string(),
            api_key: "pub-key".to_string(),
            api_secret: "shh".to_string(),
            passphrase: Some("also-shh".to_string()),
            keychain_ref: None,
            passphrase_keychain_ref: None,
            testnet: false,
            created_at: 1,
        });
        mgr.profile_mut().domain.notification_secrets.telegram.bot_token = "tg-secret".to_string();

        mgr.seal_vault();

        assert!(mgr.vault_key().is_none());
        let key = &mgr.profile().domain.exchange_keys[0];
        assert_eq!(key.exchange_id, "binance", "identity fields survive sealing");
        assert_eq!(key.api_key, "pub-key", "api_key is not a secret — survives sealing");
        assert!(key.api_secret.is_empty(), "api_secret must be wiped on seal");
        assert!(key.passphrase.is_none(), "passphrase must be wiped on seal");
        assert!(mgr.profile().domain.notification_secrets.telegram.bot_token.is_empty());
    }

    #[test]
    fn unseal_vault_restores_key_and_merges_secrets() {
        use mlc_core::user_profile::profile::StoredExchangeKey;
        use mlc_core::user_profile::VaultSecrets;

        let mut mgr = UserProfileManager::new();
        // `VaultSecrets` carries a crate-private field (`legacy_notifications`,
        // mlc-core-internal migration seam) — struct-update syntax needs every
        // field visible at the use site, so build from `default()` and set the
        // one `pub` field this test needs instead of a struct literal.
        let mut secrets = VaultSecrets::default();
        secrets.exchange_keys = vec![StoredExchangeKey {
            exchange_id: "bybit".to_string(),
            label: "".to_string(),
            api_key: "k".to_string(),
            api_secret: "s".to_string(),
            passphrase: None,
            keychain_ref: None,
            passphrase_keychain_ref: None,
            testnet: false,
            created_at: 2,
        }];

        mgr.unseal_vault([9u8; 32], secrets);

        assert_eq!(mgr.vault_key(), Some([9u8; 32]));
        assert_eq!(mgr.profile().domain.exchange_keys.len(), 1);
        assert_eq!(mgr.profile().domain.exchange_keys[0].api_secret, "s");
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

    // ── Phase C: connector_enabled / recalc_mode / server settings, ────────
    // promoted from shell-vello's per-window AppState mirror
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
    fn recalc_mode_roundtrip() {
        let mut mgr = UserProfileManager::new();
        assert_eq!(mgr.recalc_mode(), RecalcMode::default());
        mgr.set_recalc_mode(RecalcMode::PerTick);
        assert_eq!(mgr.recalc_mode(), RecalcMode::PerTick);
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
