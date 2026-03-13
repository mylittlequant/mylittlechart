//! [`ProfileManager`] — encapsulates all multi-profile management logic.
//!
//! Replaces scattered inline profile code in `main.rs` with a focused struct
//! that owns the active profile, its encrypted vault key, and cached index.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;

use crate::crypto;
use crate::preset::preset::ChartPreset;
use crate::preset::storage::{list_presets, load_preset};
use crate::templates::TemplateManager;
use crate::user_profile::profile::{ProfileIndex, ProfileMeta, UserProfile, VaultSecrets};
use crate::user_profile::storage::{
    active_profile_data_dir, create_profile, delete_profile, load_json, load_profile,
    load_profile_index, profiles_dir, save_profile, save_profile_index,
};
use crate::vault::{self, VaultKey};

use super::manager::SettingsSnapshots;

/// Minimum length required for a vault passphrase.
///
/// Enforced in the UI (Encrypt/Complete Setup buttons are disabled until
/// the passphrase reaches this length) as well as in command handlers before
/// delegating to vault key derivation.
pub const MIN_PASSPHRASE_LENGTH: usize = 8;

// =============================================================================
// ProfileInfo
// =============================================================================

/// Information about a profile for UI display.
#[derive(Debug, Clone)]
pub struct ProfileInfo {
    /// UUID v4 identifier.
    pub id: String,
    /// Directory name on disk (usually same as id, but "default" for legacy).
    pub dir_name: String,
    /// User-visible display name.
    pub display_name: String,
    /// Avatar emoji key.
    pub avatar: String,
    /// Whether cloud connectivity (OTA, sync, telemetry) is enabled for this profile.
    pub cloud_enabled: bool,
    /// Whether this profile has an encrypted vault (`vault.enc` exists).
    pub has_vault: bool,
    /// Whether this profile is the currently active one.
    pub is_active: bool,
}

// =============================================================================
// SwitchData
// =============================================================================

/// Data returned by [`ProfileManager::prepare_switch`] — everything `main.rs`
/// needs to recreate windows after a profile switch.
#[derive(Debug)]
pub struct SwitchData {
    /// Window states from the profile that was just loaded (the new profile).
    pub saved_windows: Vec<crate::user_profile::profile::WindowState>,
    /// Whether the new profile has an existing vault that needs to be unlocked
    /// (i.e. the user must enter their passphrase).
    pub needs_vault_unlock: bool,
    /// Whether the new profile needs encryption setup (first-time passphrase entry).
    pub needs_encryption_setup: bool,
}

// =============================================================================
// ProfileManager
// =============================================================================

/// Encapsulates all profile management logic.
///
/// Owns the active profile data (profile JSON, presets, templates, snapshots)
/// and the cached profile index.  All mutations (create, delete, rename, switch)
/// go through this struct so that the index and on-disk state stay in sync.
pub struct ProfileManager {
    /// Cached profile index (refreshed on every mutation).
    index: ProfileIndex,
    /// The active profile data.
    pub profile: UserProfile,
    /// All templates for the active profile.
    pub template_manager: TemplateManager,
    /// Chart presets for the active profile.
    pub presets: HashMap<String, ChartPreset>,
    /// Settings snapshots for the active profile.
    pub snapshots: SettingsSnapshots,
    /// Encryption key for the active profile's vault.
    pub vault_key: Option<VaultKey>,
    /// Formatted recovery key to show once after vault creation.
    ///
    /// Set by [`derive_and_set_vault_key`] on first-time vault setup.
    /// Cleared when the UI confirms the user has recorded it
    /// (call [`clear_pending_recovery_key`]).
    pub pending_recovery_key: Option<String>,
    /// The master key wrapped with the recovery key, ready for server upload.
    ///
    /// Set alongside [`pending_recovery_key`].  The caller should upload
    /// this (base64-encoded) to the server's `encrypted_master_key` field
    /// on the `POST /api/sync/e2e-setup` endpoint.
    pub encrypted_master_key: Option<Vec<u8>>,
}

impl ProfileManager {
    // =========================================================================
    // Constructor
    // =========================================================================

    /// Load active profile data from disk.
    ///
    /// Mirrors the logic of [`UserManager::load_with_key`] but also loads and
    /// caches the profile index.  Pass `Some(key)` for encrypted installs,
    /// `None` for plaintext / first-run.
    pub fn load(key: Option<VaultKey>) -> Self {
        let data_dir = active_profile_data_dir();
        eprintln!(
            "[ProfileManager] profile data directory: {}",
            data_dir.display()
        );

        let key_ref = key.as_ref();

        // Load the profile (profile.json + optional vault.enc merge).
        let profile = match load_profile(key_ref) {
            Ok(p) => {
                eprintln!(
                    "[ProfileManager] loaded profile (active_preset={})",
                    p.active_preset_id
                );
                p
            }
            Err(e) => {
                if key_ref.is_some() {
                    eprintln!(
                        "[ProfileManager] WARNING: decryption failed with provided key, falling back to defaults: {}",
                        e
                    );
                } else {
                    eprintln!(
                        "[ProfileManager] failed to load profile: {}, using defaults",
                        e
                    );
                }
                UserProfile::new()
            }
        };

        // Templates.
        let template_manager = {
            let tm = TemplateManager::load_from_default_dir(key_ref);
            eprintln!(
                "[ProfileManager] loaded templates: {} prim, {} ind, {} cmp, {} chart, {} sets",
                tm.primitive_templates.len(),
                tm.indicator_templates.len(),
                tm.compare_templates.len(),
                tm.chart_templates.len(),
                tm.indicator_sets.len(),
            );
            tm
        };

        // Presets.
        let mut presets = HashMap::new();
        match list_presets(key_ref) {
            Ok(metas) => {
                for meta in &metas {
                    match load_preset(&meta.id, key_ref) {
                        Ok(preset) => {
                            presets.insert(meta.id.clone(), preset);
                        }
                        Err(e) => {
                            eprintln!(
                                "[ProfileManager] failed to load preset {}: {}",
                                meta.id, e
                            );
                        }
                    }
                }
                eprintln!("[ProfileManager] loaded {} presets", presets.len());
            }
            Err(e) => eprintln!("[ProfileManager] failed to list presets: {}", e),
        }

        // Settings snapshots — always plaintext, never encrypted.
        let snapshots_path = active_profile_data_dir().join("settings_snapshots.json");
        let snapshots = match load_json::<SettingsSnapshots>(&snapshots_path, None) {
            Ok(s) => {
                eprintln!("[ProfileManager] loaded settings snapshots");
                s
            }
            Err(_) => {
                eprintln!("[ProfileManager] no settings snapshots found, using defaults");
                SettingsSnapshots::default()
            }
        };

        // Load (or build a fallback) profile index.
        let index = load_profile_index().unwrap_or_else(|| ProfileIndex {
            active_profile_id: profile.profile_id.clone(),
            profiles: Vec::new(),
        });

        Self {
            index,
            profile,
            template_manager,
            presets,
            snapshots,
            vault_key: key,
            pending_recovery_key: None,
            encrypted_master_key: None,
        }
    }

    // =========================================================================
    // Profile CRUD
    // =========================================================================

    /// Create a new profile.
    ///
    /// If `name` is `None` or empty, an auto-generated name ("New Profile",
    /// "New Profile 2", …) is used.  The new profile is NOT made active.
    /// Returns the newly created [`ProfileMeta`].
    pub fn create_profile(
        &mut self,
        name: Option<&str>,
        avatar: &str,
        cloud_enabled: bool,
    ) -> Result<ProfileMeta, String> {
        let final_name = match name {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => self.auto_generate_name(),
        };

        let meta = create_profile(&final_name, avatar, cloud_enabled)?;
        self.refresh_index();
        Ok(meta)
    }

    /// Delete a profile by ID.
    ///
    /// Returns an error if `id` matches the currently active profile (safety
    /// guard — callers must switch away first).
    pub fn delete_profile(&mut self, id: &str) -> Result<(), String> {
        if id == self.index.active_profile_id {
            return Err("Cannot delete the active profile".to_string());
        }
        delete_profile(id)?;
        self.refresh_index();
        Ok(())
    }

    /// Rename a profile by ID.
    ///
    /// Updates both the in-memory index entry and, when renaming the active
    /// profile, the in-memory `UserProfile` as well.  Persists both to disk.
    pub fn rename_profile(&mut self, id: &str, new_name: &str) -> Result<(), String> {
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return Err("Profile name must not be empty".to_string());
        }

        // If renaming the active profile, update the in-memory profile too.
        if id == self.profile.profile_id {
            self.profile.display_name = new_name.to_string();
            save_profile(&self.profile, self.vault_key.as_ref())
                .map_err(|e| e.to_string())?;
        }

        // Update the index entry.
        if let Some(entry) = self.index.profiles.iter_mut().find(|m| m.id == id) {
            entry.display_name = new_name.to_string();
        }
        save_profile_index(&self.index)?;

        Ok(())
    }

    /// Set the avatar for a profile by ID.
    ///
    /// Follows the same dual-write pattern as [`rename_profile`].
    pub fn set_avatar(&mut self, id: &str, avatar: &str) -> Result<(), String> {
        // If changing the active profile's avatar, update in-memory profile.
        if id == self.profile.profile_id {
            self.profile.avatar = avatar.to_string();
            save_profile(&self.profile, self.vault_key.as_ref())
                .map_err(|e| e.to_string())?;
        }

        // Update the index entry.
        if let Some(entry) = self.index.profiles.iter_mut().find(|m| m.id == id) {
            entry.avatar = avatar.to_string();
        }
        save_profile_index(&self.index)?;

        Ok(())
    }

    // =========================================================================
    // Vault operations
    // =========================================================================

    /// Derive an encryption key from `passphrase` and store it.
    ///
    /// Creates `salt.hex` in the active profile directory if it does not already
    /// exist.  Sets `self.vault_key` and returns the derived key.
    ///
    /// On **first-time vault setup** (no pre-existing `vault.enc`), also:
    /// - Generates a random recovery key.
    /// - Encrypts the master key with it and stores the blob in
    ///   [`Self::encrypted_master_key`] for upload to the server.
    /// - Stores the formatted recovery key in [`Self::pending_recovery_key`]
    ///   so the UI can display it once for the user to write down.
    pub fn derive_and_set_vault_key(&mut self, passphrase: &str) -> Result<VaultKey, String> {
        let profile_dir = self.active_profile_dir();
        let salt_path = profile_dir.join("salt.hex");
        let vault_path = profile_dir.join("vault.enc");
        let is_first_setup = !vault_path.exists();

        let salt = vault::load_or_create_salt(&salt_path).map_err(|e| e.to_string())?;

        // Derive master key directly so we can use it for recovery key wrapping.
        let master_key = crypto::derive_master_key(passphrase, &salt);
        let vault_key = crypto::derive_vault_key(&master_key, &salt);

        self.vault_key = Some(vault_key);

        // On first-time vault creation, generate a recovery key and wrap the master key.
        if is_first_setup {
            let recovery_key = crypto::generate_recovery_key();
            match crypto::encrypt_master_key_for_recovery(&master_key, &recovery_key, &salt) {
                Ok(encrypted) => {
                    self.pending_recovery_key = Some(crypto::format_recovery_key(&recovery_key));
                    self.encrypted_master_key = Some(encrypted);
                }
                Err(e) => {
                    eprintln!("[ProfileManager] failed to encrypt master key for recovery: {}", e);
                }
            }
        }

        Ok(vault_key)
    }

    /// Clear the pending recovery key after the UI has shown it to the user.
    ///
    /// Call this after the user acknowledges the "I have written it down" confirmation.
    pub fn clear_pending_recovery_key(&mut self) {
        self.pending_recovery_key = None;
    }

    /// Take the encrypted master key blob for server upload.
    ///
    /// Returns `None` if the blob has already been taken or was never generated.
    /// Clears the stored blob from memory.
    pub fn take_encrypted_master_key(&mut self) -> Option<Vec<u8>> {
        self.encrypted_master_key.take()
    }

    /// Validate a passphrase against the existing `vault.enc`.
    ///
    /// Derives the key, then attempts decryption.  Returns the derived key on
    /// success so the caller can pass it to [`set_vault_key`].  Does NOT set
    /// `self.vault_key` — use [`set_vault_key`] for that after confirming the
    /// UI flow.
    pub fn validate_passphrase(&self, passphrase: &str) -> Result<VaultKey, String> {
        let profile_dir = self.active_profile_dir();
        let salt_path = profile_dir.join("salt.hex");
        let vault_path = profile_dir.join("vault.enc");

        if !salt_path.exists() {
            return Err("No salt file found — vault has not been set up".to_string());
        }

        let salt = vault::load_or_create_salt(&salt_path).map_err(|e| e.to_string())?;
        let key = vault::derive_key(passphrase, &salt);

        // Try to decrypt vault.enc with the derived key.
        match vault::load_encrypted::<VaultSecrets>(&key, &vault_path) {
            Ok(_) => Ok(key),
            Err(_) => Err(
                "Decryption failed — wrong passphrase or corrupted data".to_string(),
            ),
        }
    }

    /// Set the vault key directly (e.g. after successful [`validate_passphrase`]).
    pub fn set_vault_key(&mut self, key: VaultKey) {
        self.vault_key = Some(key);
    }

    /// Decrypt vault secrets and merge them into the active profile.
    ///
    /// Requires `vault_key` to be set.  A missing `vault.enc` is treated as
    /// a no-op (profile was created without encryption).
    pub fn load_vault_secrets(&mut self) -> Result<(), String> {
        let key = match self.vault_key {
            Some(k) => k,
            None => return Err("Vault key is not set".to_string()),
        };

        let vault_path = self.active_profile_dir().join("vault.enc");
        if !vault_path.exists() {
            return Ok(());
        }

        let secrets = vault::load_encrypted::<VaultSecrets>(&key, &vault_path)
            .map_err(|e| e.to_string())?;
        secrets.merge_into(&mut self.profile);
        Ok(())
    }

    // =========================================================================
    // Profile switching
    // =========================================================================

    /// Prepare a switch to a different profile.
    ///
    /// 1. Saves the current profile (profile.json + vault.enc if key is set).
    /// 2. Updates `index.active_profile_id` and persists the index.
    /// 3. Reloads all data for the new profile into `self`.
    /// 4. Returns [`SwitchData`] so `main.rs` can rebuild its windows.
    ///
    /// The `current_windows` slice is used only to save the current window
    /// layout before the switch; pass an empty slice if you have no windows.
    pub fn prepare_switch(
        &mut self,
        target_id: &str,
        current_windows: &[crate::user_profile::profile::WindowState],
    ) -> Result<SwitchData, String> {
        // Persist current window state into the active profile before saving.
        if !current_windows.is_empty() {
            self.profile.windows = current_windows.to_vec();
        }

        // Save the current profile.
        save_profile(&self.profile, self.vault_key.as_ref()).map_err(|e| e.to_string())?;

        // Update and save the index.
        self.index.active_profile_id = target_id.to_string();
        save_profile_index(&self.index)?;

        // Reload from disk for the new active profile.
        // We don't carry the vault key across profiles — the new profile may
        // have its own passphrase.
        *self = ProfileManager::load(None);

        let needs_vault_unlock = self.needs_vault_unlock();
        let needs_encryption_setup = self.needs_encryption_setup();
        let saved_windows = self.profile.windows.clone();

        Ok(SwitchData {
            saved_windows,
            needs_vault_unlock,
            needs_encryption_setup,
        })
    }

    // =========================================================================
    // Save
    // =========================================================================

    /// Save the active profile to disk.
    ///
    /// Writes `profile.json` (plaintext) and, when `vault_key` is set,
    /// re-encrypts credentials into `vault.enc`.
    pub fn save_profile(&self) -> Result<(), String> {
        save_profile(&self.profile, self.vault_key.as_ref()).map_err(|e| e.to_string())
    }

    // =========================================================================
    // Queries
    // =========================================================================

    /// Return display information for all known profiles, ordered as stored in
    /// the index.
    pub fn available_profiles(&self) -> Vec<ProfileInfo> {
        let pdir = profiles_dir();
        self.index
            .profiles
            .iter()
            .map(|m| {
                let dir = pdir.join(&m.dir_name);
                let has_vault = dir.join("vault.enc").exists();
                let is_active = m.id == self.index.active_profile_id;
                ProfileInfo {
                    id: m.id.clone(),
                    dir_name: m.dir_name.clone(),
                    display_name: m.display_name.clone(),
                    avatar: m.avatar.clone(),
                    cloud_enabled: m.cloud_enabled,
                    has_vault,
                    is_active,
                }
            })
            .collect()
    }

    /// Whether the active profile has an encrypted vault that has not yet been
    /// unlocked this session.
    pub fn needs_vault_unlock(&self) -> bool {
        if self.vault_key.is_some() {
            return false;
        }
        let dir = self.active_profile_dir();
        dir.join("vault.enc").exists() && dir.join("salt.hex").exists()
    }

    /// Whether the active profile exists but has never had encryption set up.
    ///
    /// Returns `true` when `profile.json` is present but neither `vault.enc`
    /// nor `salt.hex` exist.
    pub fn needs_encryption_setup(&self) -> bool {
        let dir = self.active_profile_dir();
        dir.join("profile.json").exists()
            && !dir.join("vault.enc").exists()
            && !dir.join("salt.hex").exists()
    }

    /// Returns the data directory for the currently active profile.
    pub fn active_profile_dir(&self) -> PathBuf {
        active_profile_data_dir()
    }

    /// Refresh the cached index from disk.
    pub fn refresh_index(&mut self) {
        if let Some(idx) = load_profile_index() {
            self.index = idx;
        }
    }

    // =========================================================================
    // Private helpers
    // =========================================================================

    /// Generate a unique "New Profile [N]" name based on existing profiles.
    fn auto_generate_name(&self) -> String {
        let base = "New Profile";
        let count = self
            .index
            .profiles
            .iter()
            .filter(|m| m.display_name == base || m.display_name.starts_with(&format!("{} ", base)))
            .count();

        if count == 0 {
            base.to_string()
        } else {
            format!("{} {}", base, count + 1)
        }
    }
}
