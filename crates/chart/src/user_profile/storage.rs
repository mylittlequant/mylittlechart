//! Storage layer for the user profile system.
//!
//! All user data files live under the OS application data directory:
//! - Windows: `%APPDATA%\zengeld\`
//! - macOS:   `~/Library/Application Support/zengeld/`
//! - Linux:   `$XDG_DATA_HOME/zengeld/` (default `~/.local/share/zengeld/`)
//!
//! The root is created automatically on first use.
//!
//! # Public API
//!
//! - [`app_data_dir`] — resolve (and create) the OS app-data root.
//! - [`get_user_data_dir`] — backward-compatible alias for [`app_data_dir`].
//! - [`save_profile`] / [`load_profile`] — write/read `profile.json` (plaintext,
//!   no credentials) and `vault.enc` (encrypted credentials, when key is available).
//! - [`save_json`] / [`load_json`] — generic helpers for any `Serialize` /
//!   `DeserializeOwned` type at an arbitrary path inside the data dir.
//!
//! # Error handling
//!
//! All fallible operations return [`Result<_, ProfileError>`].  Callers
//! typically log the error and fall back to a default value rather than
//! propagating it, since missing user data is not fatal.

use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::profile::{ProfileIndex, ProfileMeta, UserProfile, VaultSecrets};
use crate::vault::{self, VaultKey};

// =============================================================================
// ProfileError
// =============================================================================

/// Errors that can arise from user profile I/O.
#[derive(Debug)]
pub enum ProfileError {
    /// An underlying filesystem I/O error.
    Io(std::io::Error),
    /// JSON serialization or deserialization failure.
    Json(serde_json::Error),
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileError::Io(e) => write!(f, "profile I/O error: {}", e),
            ProfileError::Json(e) => write!(f, "profile JSON error: {}", e),
        }
    }
}

impl std::error::Error for ProfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ProfileError::Io(e) => Some(e),
            ProfileError::Json(e) => Some(e),
        }
    }
}

impl From<std::io::Error> for ProfileError {
    fn from(e: std::io::Error) -> Self {
        ProfileError::Io(e)
    }
}

impl From<serde_json::Error> for ProfileError {
    fn from(e: serde_json::Error) -> Self {
        ProfileError::Json(e)
    }
}

// =============================================================================
// Directory resolution
// =============================================================================

/// Resolves the platform-specific base data directory (the parent of the
/// `zengeld/` subfolder).
///
/// Resolution order:
/// 1. `%APPDATA%` (Windows)
/// 2. `$XDG_DATA_HOME` (Linux/BSD)
/// 3. `~/Library/Application Support` (macOS) or `~/.local/share` (Linux via `$HOME`)
/// 4. `%USERPROFILE%\AppData\Roaming` (Windows fallback)
/// 5. Directory of the running executable (last resort)
fn resolve_platform_data_dir() -> PathBuf {
    // Windows: %APPDATA%
    if let Ok(appdata) = std::env::var("APPDATA") {
        if !appdata.is_empty() {
            return PathBuf::from(appdata);
        }
    }

    // Linux / BSD: XDG_DATA_HOME
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg);
        }
    }

    // macOS / Linux fallback via $HOME
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            let home = PathBuf::from(home);
            if cfg!(target_os = "macos") {
                return home.join("Library").join("Application Support");
            }
            // Linux fallback
            return home.join(".local").join("share");
        }
    }

    // Windows fallback: %USERPROFILE%
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        if !userprofile.is_empty() {
            return PathBuf::from(userprofile)
                .join("AppData")
                .join("Roaming");
        }
    }

    // Last resort: directory of the running executable
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Returns the root application data directory for mylittlechart, creating
/// it if necessary.
///
/// Platform-specific paths:
/// - Windows: `%APPDATA%\mylittlechart\`
/// - macOS:   `~/Library/Application Support/mylittlechart/`
/// - Linux:   `$XDG_DATA_HOME/mylittlechart/` (default `~/.local/share/mylittlechart/`)
///
/// One-shot migration: if a legacy `zengeld/` directory exists and the new
/// `mylittlechart/` directory has no `profile_index.json` (which means it
/// either does not exist or was created empty by another helper such as
/// `diagnostics::default_log_dir()` writing to `mylittlechart/logs/`), the
/// legacy contents are merged into the new dir entry-by-entry. Existing
/// entries in the new dir are kept untouched.
pub fn app_data_dir() -> PathBuf {
    let base = resolve_platform_data_dir();
    let dir = base.join("mylittlechart");
    let legacy = base.join("zengeld");
    let index_path = dir.join("profile_index.json");
    if legacy.exists() && legacy.is_dir() && !index_path.exists() {
        // Ensure the destination exists before merging children.
        let _ = fs::create_dir_all(&dir);
        match merge_legacy_dir(&legacy, &dir) {
            Ok(moved) => {
                eprintln!(
                    "[app_data_dir] migrated {} entries from legacy dir: {} -> {}",
                    moved,
                    legacy.display(),
                    dir.display()
                );
                // Remove the (now empty) legacy dir if everything moved.
                let _ = fs::remove_dir(&legacy);
            }
            Err(e) => eprintln!(
                "[app_data_dir] WARN: legacy merge {} -> {} hit error: {} (some files may have been moved)",
                legacy.display(),
                dir.display(),
                e
            ),
        }
    }
    // Best-effort creation; callers receive an Io error on the actual
    // read/write if the directory cannot be created.
    let _ = fs::create_dir_all(&dir);
    // Restrict directory permissions to owner-only on Unix (rwx------).
    // This prevents other local users from reading stored API keys and
    // profile data.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    }
    // TODO: Windows ACL via icacls or windows-sys
    dir
}

/// Move every child of `from` into `to`, skipping entries that already exist
/// in `to`. Returns the number of top-level entries that were moved.
///
/// Used to merge the legacy `zengeld/` data dir into the new `mylittlechart/`
/// directory when the latter was pre-created by another helper (e.g. the
/// diagnostics logger creating `mylittlechart/logs/` before profile code
/// runs). Standard `fs::rename(zengeld, mylittlechart)` would fail because
/// the destination exists; entry-by-entry rename works since each child
/// path is fresh on the destination side (or, if it already exists, the
/// pre-created copy wins).
fn merge_legacy_dir(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<usize> {
    let mut moved = 0usize;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let src = entry.path();
        let name = entry.file_name();
        let dst = to.join(&name);
        if dst.exists() {
            // Destination already has this entry — leave the legacy copy in
            // place; user can merge manually if needed.
            continue;
        }
        if let Err(e) = fs::rename(&src, &dst) {
            eprintln!(
                "[merge_legacy_dir] skip {} -> {}: {}",
                src.display(),
                dst.display(),
                e
            );
            continue;
        }
        moved += 1;
    }
    Ok(moved)
}

/// Returns the root application data directory.
///
/// Backward-compatible alias for [`app_data_dir`].  All existing callers
/// automatically use the new OS-standard path without any further changes.
pub fn get_user_data_dir() -> PathBuf {
    app_data_dir()
}

// =============================================================================
// profile.json / profile.enc helpers
// =============================================================================

/// Save profile using the split model.
///
/// `profile.json` is **always** written as plaintext (no credentials).
/// Credential fields (`local_agent_keys`, `exchange_keys`, `notification_settings`,
/// `legacy_single_agent_key`) are extracted into a [`VaultSecrets`] struct and written
/// to `vault.enc` only when `key` is `Some`.
///
/// When `key` is `None` (vault not yet unlocked), the vault file from the
/// previous session is left untouched — credentials are not lost.
pub fn save_profile(profile: &UserProfile, key: Option<&VaultKey>) -> Result<(), ProfileError> {
    let dir = active_profile_data_dir();
    fs::create_dir_all(&dir)?;

    // Clone so we can strip credentials without mutating the caller's value.
    let mut plaintext_profile = profile.clone();

    // Extract credentials from the clone → VaultSecrets.
    let secrets = VaultSecrets::extract_from(&mut plaintext_profile);

    // Always write profile.json (plaintext, no credentials).
    let json_path = dir.join("profile.json");
    let json = serde_json::to_string_pretty(&plaintext_profile)?;
    fs::write(&json_path, &json)?;

    // Write vault.enc only when the key is available.
    // When key is None (before vault unlock), vault.enc from the previous session
    // is left untouched — credentials are not lost.
    if let Some(k) = key {
        let vault_path = dir.join("vault.enc");
        vault::save_encrypted(k, &vault_path, &secrets)
            .map_err(|e| ProfileError::Io(std::io::Error::other(e.to_string())))?;
    }

    Ok(())
}

/// Load profile using the split model.
///
/// Always loads `profile.json` (plaintext, no credentials) first.
/// If `vault.enc` exists and a key is provided, the credentials are
/// decrypted and merged back into the returned [`UserProfile`].
///
/// When no key is available (vault not yet unlocked) the profile is still
/// returned — just without credentials populated.
pub fn load_profile(key: Option<&VaultKey>) -> Result<UserProfile, ProfileError> {
    let dir = active_profile_data_dir();
    let json_path = dir.join("profile.json");
    let vault_enc_path = dir.join("vault.enc");

    // Load plaintext profile (always readable without key).
    let mut profile = if json_path.exists() {
        let json = fs::read_to_string(&json_path)?;
        serde_json::from_str::<UserProfile>(&json)?
    } else {
        UserProfile::new()
    };

    // Merge encrypted vault secrets if key is available.
    if vault_enc_path.exists() {
        if let Some(k) = key {
            match vault::load_encrypted::<VaultSecrets>(k, &vault_enc_path) {
                Ok(secrets) => secrets.merge_into(&mut profile),
                Err(e) => {
                    eprintln!("[storage] Failed to decrypt vault.enc: {}", e);
                    // Profile is still usable — just without credentials.
                }
            }
        }
        // No key but vault.enc exists: profile loads fine, secrets stay empty.
    }

    Ok(profile)
}

/// Generic save — encrypted or plaintext.
///
/// When `key` is `Some`, the file is written with a `.enc` extension and any
/// existing plaintext file at `path` is removed.  When `key` is `None` the
/// value is written as pretty JSON at `path` unchanged.
pub fn save_json<T: Serialize>(path: &Path, data: &T, key: Option<&VaultKey>) -> Result<(), ProfileError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match key {
        Some(k) => {
            // Change extension to `.enc`.
            let enc_path = path.with_extension("enc");
            vault::save_encrypted(k, &enc_path, data)
                .map_err(|e| ProfileError::Io(std::io::Error::other(e.to_string())))?;
            // Remove plaintext if it exists.
            if path.exists() {
                let _ = fs::remove_file(path);
            }
        }
        None => {
            let json = serde_json::to_string_pretty(data)?;
            fs::write(path, json)?;
        }
    }
    Ok(())
}

/// Generic load — plaintext by default, with optional legacy `.enc` migration.
///
/// If an `.enc` file exists and a key is provided, the file is decrypted,
/// re-saved as plaintext JSON, and the `.enc` file is deleted (one-time
/// migration).  If an `.enc` file exists but no key is provided, it is
/// silently skipped and the plaintext `.json` version is tried instead
/// (non-sensitive files like watchlists should always be loadable without a key).
pub fn load_json<T: DeserializeOwned + Serialize>(path: &Path, key: Option<&VaultKey>) -> Result<T, ProfileError> {
    let enc_path = path.with_extension("enc");

    if enc_path.exists() {
        if let Some(k) = key {
            // Attempt to decrypt and migrate to plaintext.
            match vault::load_encrypted::<T>(k, &enc_path) {
                Ok(value) => {
                    // Persist as plaintext and remove .enc.
                    if let Ok(json) = serde_json::to_string_pretty(&value) {
                        let _ = fs::write(path, json);
                    }
                    let _ = fs::remove_file(&enc_path);
                    return Ok(value);
                }
                Err(e) => {
                    eprintln!("[storage] failed to decrypt legacy {:?}: {}", enc_path, e);
                    // Fall through to try plaintext.
                }
            }
        }
        // No key provided — skip .enc and fall through to plaintext.
    }

    // Load plaintext.
    let json = fs::read_to_string(path)?;
    let value: T = serde_json::from_str(&json)?;
    Ok(value)
}

// =============================================================================
// Convenience path builders
// =============================================================================

/// Returns the path to `watchlists.json` inside the active profile directory.
pub fn watchlists_path() -> PathBuf {
    active_profile_data_dir().join("watchlists.json")
}

// =============================================================================
// Multi-profile directory helpers
// =============================================================================

/// Returns `{app_data_dir}/profiles/`, creating it if it does not exist.
pub fn profiles_dir() -> PathBuf {
    let dir = app_data_dir().join("profiles");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Load the profile index from `profiles/index.json`.
///
/// Returns `None` if the file does not exist or cannot be parsed.
pub fn load_profile_index() -> Option<ProfileIndex> {
    let path = profiles_dir().join("index.json");
    if !path.exists() {
        return None;
    }
    let json = fs::read_to_string(&path).ok()?;
    let index: ProfileIndex = serde_json::from_str(&json).ok()?;

    Some(index)
}

/// Save the profile index to `profiles/index.json`.
pub fn save_profile_index(index: &ProfileIndex) -> Result<(), String> {
    let path = profiles_dir().join("index.json");
    let json = serde_json::to_string_pretty(index).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

/// Returns the data directory for the currently active profile.
///
/// **Read-only**: this function never creates files or directories.
///
/// - If a `profiles/index.json` exists and the active profile is found,
///   returns `profiles/{dir_name}/`.
/// - If the index exists but the active id is missing, falls back to the
///   first profile in the list.
/// - If no index exists or the index is empty, returns a placeholder path
///   `profiles/_pending` that does not exist on disk.  Callers that try to
///   read `profile.json` from this path will get `NotFound` and fall back to
///   an in-memory default.  The real profile directory is created only when
///   the welcome wizard completes via `create_profile()`.
pub fn active_profile_data_dir() -> PathBuf {
    if let Some(index) = load_profile_index() {
        if let Some(meta) = index
            .profiles
            .iter()
            .find(|m| m.id == index.active_profile_id)
            .or_else(|| {
                if index.profiles.first().is_some() {
                    eprintln!(
                        "[storage] WARNING: active_profile_id '{}' not found in index, falling back to first profile",
                        index.active_profile_id
                    );
                }
                index.profiles.first()
            })
        {
            return profiles_dir().join(&meta.dir_name);
        }
    }
    // No index or empty index — return a placeholder that does not exist on
    // disk.  Callers receive NotFound on any read and fall back to defaults.
    // The welcome wizard creates a real profile via create_profile().
    profiles_dir().join("_pending")
}

// =============================================================================
// Legacy profile migration
// =============================================================================

/// Legacy migration removed — all profiles are already in the split format.
pub fn migrate_legacy_profile_if_needed() -> Result<bool, String> {
    Ok(false)
}

// =============================================================================
// Profile creation
// =============================================================================

/// Create a new profile with the given display name, avatar, and cloud mode.
///
/// Creates `profiles/{uuid}/profile.json` and adds the entry to the index.
/// Does NOT switch the active profile.
pub fn create_profile(name: &str, avatar: &str) -> Result<ProfileMeta, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    // Use the UUID string as the directory name for uniqueness.
    let dir_name = id.clone();
    let profile_dir = profiles_dir().join(&dir_name);
    fs::create_dir_all(&profile_dir).map_err(|e| e.to_string())?;

    // Build and persist a new profile.
    let mut profile = UserProfile::new();
    profile.profile_id = id.clone();
    profile.display_name = name.to_string();
    profile.avatar = avatar.to_string();
    profile.profile_created_at = now;

    let json = serde_json::to_string_pretty(&profile).map_err(|e| e.to_string())?;
    fs::write(profile_dir.join("profile.json"), json).map_err(|e| e.to_string())?;

    // Generate and persist a random 16-byte salt for future vault key derivation.
    // The salt is safe to store in plaintext — it is only useful with the passphrase.
    let salt = crate::vault::generate_salt();
    let salt_hex = salt.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    fs::write(profile_dir.join("salt.hex"), salt_hex).map_err(|e| e.to_string())?;

    let meta = ProfileMeta {
        id: id.clone(),
        display_name: name.to_string(),
        avatar: avatar.to_string(),
        created_at: now,
        dir_name,
    };

    // Append to existing index (or create one if it doesn't exist yet).
    let mut index = load_profile_index().unwrap_or_else(|| ProfileIndex {
        active_profile_id: id.clone(),
        profiles: Vec::new(),
    });
    index.profiles.push(meta.clone());
    save_profile_index(&index)?;

    Ok(meta)
}

// =============================================================================
// Profile deletion
// =============================================================================

/// Delete a profile by ID.
///
/// Removes the profile directory from disk and removes the entry from the
/// index.  Does NOT allow deleting the active profile — callers must check
/// that `id != index.active_profile_id` before calling this.
///
/// Returns `Ok(())` if the profile was deleted or was not found.
/// Returns `Err` if the index cannot be loaded or saved, or if `id` matches
/// the active profile (safety guard).
pub fn delete_profile(id: &str) -> Result<(), String> {
    let mut index = load_profile_index()
        .ok_or_else(|| "No profile index found".to_string())?;

    // If deleting the active-in-index profile, switch active to the first remaining.
    if index.active_profile_id == id {
        let fallback = index.profiles.iter().find(|m| m.id != id).map(|m| m.id.clone());
        if let Some(next_id) = fallback {
            index.active_profile_id = next_id;
        }
        // If no other profiles remain, active_profile_id stays stale but harmless.
    }

    // Find the profile metadata so we know its directory name.
    let meta = index.profiles.iter().find(|m| m.id == id).cloned();

    // Remove from the index.
    index.profiles.retain(|m| m.id != id);
    save_profile_index(&index)?;

    // Remove the profile directory (best effort — ignore missing).
    if let Some(m) = meta {
        let dir = profiles_dir().join(&m.dir_name);
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .map_err(|e| format!("Failed to remove profile directory: {}", e))?;
        }
    }

    Ok(())
}
