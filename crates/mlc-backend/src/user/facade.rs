//! `User` / `UserMut` — the only doctrinally-sanctioned access path to
//! `UserProfileManager` from outside `mlc-backend`.
//!
//! `UserProfileManager` never touches `DataBubble` — it owns profile +
//! snapshots + vault key as its own state. Like `PresetManager`/
//! `TemplateManager`, the facade holds exactly one guard, no `BubbleStore`
//! co-acquisition.
//!
//! See `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A/§1B.

use std::sync::{RwLockReadGuard, RwLockWriteGuard};

use mlc_core::user_profile::{SettingsSnapshots, UserProfile};
use mlc_vault::vault::VaultKey;

use super::manager::UserProfileManager;

/// Read-only facade over `UserProfileManager`.
pub struct User<'a> {
    mgr: RwLockReadGuard<'a, UserProfileManager>,
}

impl<'a> User<'a> {
    pub(crate) fn new(mgr: RwLockReadGuard<'a, UserProfileManager>) -> Self {
        Self { mgr }
    }

    pub fn profile(&self) -> &UserProfile {
        self.mgr.profile()
    }

    pub fn snapshots(&self) -> &SettingsSnapshots {
        self.mgr.snapshots()
    }

    pub fn vault_key(&self) -> Option<VaultKey> {
        self.mgr.vault_key()
    }

    pub fn save_profile(&self) -> Result<(), mlc_core::user_profile::storage::ProfileError> {
        self.mgr.save_profile()
    }

    pub fn connector_enabled(&self) -> &std::collections::HashMap<String, bool> {
        self.mgr.connector_enabled()
    }

    pub fn is_connector_enabled(&self, exchange_key: &str) -> bool {
        self.mgr.is_connector_enabled(exchange_key)
    }

    pub fn recalc_mode(&self) -> mlc_backend_indicator_engine::RecalcMode {
        self.mgr.recalc_mode()
    }

    pub fn server_enabled(&self) -> bool {
        self.mgr.server_enabled()
    }

    pub fn server_port(&self) -> u16 {
        self.mgr.server_port()
    }
}

/// Mutable facade over `UserProfileManager`.
pub struct UserMut<'a> {
    mgr: RwLockWriteGuard<'a, UserProfileManager>,
}

impl<'a> UserMut<'a> {
    pub(crate) fn new(mgr: RwLockWriteGuard<'a, UserProfileManager>) -> Self {
        Self { mgr }
    }

    pub fn profile(&self) -> &UserProfile {
        self.mgr.profile()
    }

    pub fn profile_mut(&mut self) -> &mut UserProfile {
        self.mgr.profile_mut()
    }

    pub fn set_profile(&mut self, profile: UserProfile) {
        self.mgr.set_profile(profile);
    }

    pub fn snapshots(&self) -> &SettingsSnapshots {
        self.mgr.snapshots()
    }

    pub fn snapshots_mut(&mut self) -> &mut SettingsSnapshots {
        self.mgr.snapshots_mut()
    }

    pub fn set_snapshots(&mut self, snapshots: SettingsSnapshots) {
        self.mgr.set_snapshots(snapshots);
    }

    pub fn vault_key(&self) -> Option<VaultKey> {
        self.mgr.vault_key()
    }

    pub fn set_vault_key(&mut self, key: Option<VaultKey>) {
        self.mgr.set_vault_key(key);
    }

    /// Deliberately seal the vault mid-session — see
    /// [`super::manager::UserProfileManager::seal_vault`].
    pub fn seal_vault(&mut self) {
        self.mgr.seal_vault();
    }

    /// Re-derive live secret state after a mid-session unlock — see
    /// [`super::manager::UserProfileManager::unseal_vault`].
    pub fn unseal_vault(&mut self, key: VaultKey, secrets: mlc_core::user_profile::VaultSecrets) {
        self.mgr.unseal_vault(key, secrets);
    }

    pub fn save_profile(&self) -> Result<(), mlc_core::user_profile::storage::ProfileError> {
        self.mgr.save_profile()
    }

    /// Replace the whole manager wholesale (startup load, profile switch).
    pub fn replace_all(&mut self, profile: UserProfile, snapshots: SettingsSnapshots, vault_key: Option<VaultKey>) {
        self.mgr.replace_all(profile, snapshots, vault_key);
    }

    // =========================================================================
    // Per-exchange connector enable/disable (Phase C)
    // =========================================================================

    pub fn connector_enabled(&self) -> &std::collections::HashMap<String, bool> {
        self.mgr.connector_enabled()
    }

    pub fn is_connector_enabled(&self, exchange_key: &str) -> bool {
        self.mgr.is_connector_enabled(exchange_key)
    }

    pub fn set_connector_enabled(&mut self, map: std::collections::HashMap<String, bool>) {
        self.mgr.set_connector_enabled(map);
    }

    pub fn set_connector_enabled_one(&mut self, exchange_key: String, enabled: bool) {
        self.mgr.set_connector_enabled_one(exchange_key, enabled);
    }

    // =========================================================================
    // Performance settings (Phase C)
    // =========================================================================

    pub fn recalc_mode(&self) -> mlc_backend_indicator_engine::RecalcMode {
        self.mgr.recalc_mode()
    }

    pub fn set_recalc_mode(&mut self, mode: mlc_backend_indicator_engine::RecalcMode) {
        self.mgr.set_recalc_mode(mode);
    }

    // =========================================================================
    // Agent API server settings (Phase C)
    // =========================================================================

    pub fn server_enabled(&self) -> bool {
        self.mgr.server_enabled()
    }

    pub fn set_server_enabled(&mut self, enabled: bool) {
        self.mgr.set_server_enabled(enabled);
    }

    pub fn server_port(&self) -> u16 {
        self.mgr.server_port()
    }

    pub fn set_server_port(&mut self, port: u16) {
        self.mgr.set_server_port(port);
    }
}
