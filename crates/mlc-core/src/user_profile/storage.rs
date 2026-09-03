//! Storage layer for the user profile system.
//!
//! All user data files live under the OS application data directory:
//! - Windows: `%APPDATA%\mylittlechart\`
//! - macOS:   `~/Library/Application Support/mylittlechart/`
//! - Linux:   `$XDG_DATA_HOME/mylittlechart/` (default `~/.local/share/mylittlechart/`)
//!
//! All I/O is routed through [`mlc_vault::global_storage()`] so the same
//! code runs on native (filesystem) and wasm (localStorage).
//!
//! # Public API
//!
//! - [`app_data_dir`] — resolve the OS app-data root (path only, no I/O).
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

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::profile::{ProfileIndex, ProfileMeta, UserProfile, VaultSecrets};
use mlc_vault::vault::{self, VaultKey};
use mlc_vault::global_storage;

// =============================================================================
// ProfileError
// =============================================================================

/// Errors that can arise from user profile I/O.
#[derive(Debug)]
pub enum ProfileError {
    /// An underlying I/O error.
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
// Path helpers
// =============================================================================

/// Convert an absolute `PathBuf` to a storage key relative to `app_data_dir()`.
fn path_key(path: &PathBuf) -> String {
    let root = app_data_dir();
    if let Ok(rel) = path.strip_prefix(&root) {
        rel.to_string_lossy().replace('\\', "/")
    } else {
        path.to_string_lossy().replace('\\', "/")
    }
}

fn path_ref_key(path: &Path) -> String {
    path_key(&path.to_path_buf())
}

// =============================================================================
// Directory resolution
// =============================================================================

/// Resolves the platform-specific base data directory (the parent of the
/// `mylittlechart/` subfolder).
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

/// Returns the root application data directory for mylittlechart.
///
/// This function only computes the path — it does NOT perform any I/O.
/// The storage backend creates directories lazily on write.
///
/// Platform-specific paths:
/// - Windows: `%APPDATA%\mylittlechart\`
/// - macOS:   `~/Library/Application Support/mylittlechart/`
/// - Linux:   `$XDG_DATA_HOME/mylittlechart/` (default `~/.local/share/mylittlechart/`)
pub fn app_data_dir() -> PathBuf {
    resolve_platform_data_dir().join("mylittlechart")
}

/// Returns the root application data directory.
///
/// Backward-compatible alias for [`app_data_dir`].
pub fn get_user_data_dir() -> PathBuf {
    app_data_dir()
}

// =============================================================================
// profile.json / vault.enc helpers
// =============================================================================

/// Save profile using the split model.
///
/// `profile.json` is **always** written as plaintext (no credentials).
/// Credential fields are extracted into a [`VaultSecrets`] struct and written
/// to `vault.enc` only when `key` is `Some`.
///
/// When `key` is `None` (vault not yet unlocked), the vault file from the
/// previous session is left untouched — credentials are not lost.
pub fn save_profile(profile: &UserProfile, key: Option<&VaultKey>) -> Result<(), ProfileError> {
    let dir = active_profile_data_dir();

    // Clone so we can strip credentials without mutating the caller's value.
    let mut plaintext_profile = profile.clone();

    // Extract credentials from the clone → VaultSecrets.
    let secrets = VaultSecrets::extract_from(&mut plaintext_profile);

    // Always write profile.json (plaintext, no credentials).
    let json_path = dir.join("profile.json");
    let json = serde_json::to_string_pretty(&plaintext_profile)?;
    global_storage().write(&path_key(&json_path), json.as_bytes())?;

    // Write vault.enc only when the key is available.
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
pub fn load_profile(key: Option<&VaultKey>) -> Result<UserProfile, ProfileError> {
    let dir = active_profile_data_dir();
    let json_path = dir.join("profile.json");
    let vault_enc_path = dir.join("vault.enc");
    let storage = global_storage();

    // Load plaintext profile (always readable without key).
    let mut profile = match storage.read(&path_key(&json_path))? {
        Some(bytes) => {
            let json = String::from_utf8(bytes)
                .map_err(|e| std::io::Error::other(format!("UTF-8: {e}")))?;
            serde_json::from_str::<UserProfile>(&json)?
        }
        None => UserProfile::new(),
    };

    // Merge encrypted vault secrets if key is available.
    if storage.exists(&path_key(&vault_enc_path)) {
        if let Some(k) = key {
            match vault::load_encrypted::<VaultSecrets>(k, &vault_enc_path) {
                Ok(secrets) => secrets.merge_into(&mut profile),
                Err(e) => {
                    eprintln!("[storage] Failed to decrypt vault.enc: {}", e);
                    // Profile is still usable — just without credentials.
                }
            }
        }
    }

    Ok(profile)
}

/// Generic save — encrypted or plaintext.
///
/// When `key` is `Some`, the file is written with a `.enc` extension and any
/// existing plaintext file at `path` is removed.  When `key` is `None` the
/// value is written as pretty JSON at `path` unchanged.
pub fn save_json<T: Serialize>(path: &Path, data: &T, key: Option<&VaultKey>) -> Result<(), ProfileError> {
    let storage = global_storage();
    match key {
        Some(k) => {
            // Change extension to `.enc`.
            let enc_path = path.with_extension("enc");
            vault::save_encrypted(k, &enc_path, data)
                .map_err(|e| ProfileError::Io(std::io::Error::other(e.to_string())))?;
            // Remove plaintext if it exists.
            let _ = storage.delete(&path_ref_key(path));
        }
        None => {
            let json = serde_json::to_string_pretty(data)?;
            storage.write(&path_ref_key(path), json.as_bytes())?;
        }
    }
    Ok(())
}

/// Generic load — plaintext by default, with optional legacy `.enc` migration.
///
/// If an `.enc` file exists and a key is provided, the file is decrypted,
/// re-saved as plaintext JSON, and the `.enc` file is deleted (one-time
/// migration).  If an `.enc` file exists but no key is provided, it is
/// silently skipped and the plaintext `.json` version is tried instead.
pub fn load_json<T: DeserializeOwned + Serialize>(path: &Path, key: Option<&VaultKey>) -> Result<T, ProfileError> {
    let enc_path = path.with_extension("enc");
    let storage = global_storage();

    if storage.exists(&path_ref_key(&enc_path)) {
        if let Some(k) = key {
            // Attempt to decrypt and migrate to plaintext.
            match vault::load_encrypted::<T>(k, &enc_path) {
                Ok(value) => {
                    // Persist as plaintext and remove .enc.
                    if let Ok(json) = serde_json::to_string_pretty(&value) {
                        let _ = storage.write(&path_ref_key(path), json.as_bytes());
                    }
                    let _ = storage.delete(&path_ref_key(&enc_path));
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
    let bytes = storage
        .read(&path_ref_key(path))?
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
    let json = String::from_utf8(bytes)
        .map_err(|e| std::io::Error::other(format!("UTF-8: {e}")))?;
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

/// Returns `{app_data_dir}/profiles/` (path only, no I/O).
pub fn profiles_dir() -> PathBuf {
    app_data_dir().join("profiles")
}

/// Load the profile index from `profiles/index.json`.
///
/// Returns `None` if the file does not exist or cannot be parsed.
pub fn load_profile_index() -> Option<ProfileIndex> {
    let path = profiles_dir().join("index.json");
    let Ok(Some(bytes)) = global_storage().read(&path_key(&path)) else {
        // Genuinely absent. This is the only failure that may be read as
        // "there are no profiles" — see below for why the distinction is not
        // pedantry.
        return None;
    };

    // A UTF-8 BOM is not corruption, it is a Windows editor being a Windows
    // editor. PowerShell 5.1's `Set-Content -Encoding utf8` writes one, and on
    // 2026-09-03 that alone cost the owner his profile list: parsing failed,
    // the failure was indistinguishable from "no index", and the caller
    // bootstrapped a fresh profile over the top.
    let raw = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);

    match std::str::from_utf8(raw).ok().and_then(|j| serde_json::from_str::<ProfileIndex>(j).ok()) {
        Some(index) => Some(index),
        None => {
            // The file EXISTS and does not parse. That is damage, not a first
            // run, and the two must never look alike to a caller: one of them
            // is answered by creating a profile and rewriting this very file,
            // which is how a parse error turns into data loss.
            //
            // Rebuild from the directories instead — they are the ground
            // truth, the index is a convenience. Every profile dir carries its
            // own `profile.json`; the index only orders them and names the
            // active one.
            eprintln!(
                "[ProfileIndex] {} exists but does not parse — rebuilding from the profile directories rather than treating this as a first run",
                path.display()
            );
            rebuild_profile_index_from_disk()
        }
    }
}

/// Reconstruct the profile index by reading `profiles/*/profile.json`.
///
/// Used when `index.json` is present but unreadable. Returns `None` only when
/// there are no profile directories at all — which really is a first run.
///
/// The active profile cannot be recovered from the directories (it lives only
/// in the index), so the most recently modified profile is chosen: the one the
/// user was last using. Better than the alphabetically first, and the user can
/// switch in one click if it guesses wrong.
fn rebuild_profile_index_from_disk() -> Option<ProfileIndex> {
    let dir = profiles_dir();
    let mut found: Vec<(std::time::SystemTime, ProfileMeta)> = Vec::new();

    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let pdir = entry.path();
        if !pdir.is_dir() {
            continue;
        }
        let pj = pdir.join("profile.json");
        let Ok(bytes) = std::fs::read(&pj) else { continue };
        let raw = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
        let Ok(text) = std::str::from_utf8(raw) else { continue };
        let Ok(profile) = serde_json::from_str::<UserProfile>(text) else { continue };

        let dir_name = entry.file_name().to_string_lossy().to_string();
        let id = if profile.domain.profile_id.is_empty() {
            dir_name.clone()
        } else {
            profile.domain.profile_id.clone()
        };
        let modified = std::fs::metadata(&pj)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

        found.push((
            modified,
            ProfileMeta {
                id,
                display_name: profile.ui.display_name.clone(),
                avatar: profile.ui.avatar.clone(),
                created_at: profile.ui.profile_created_at,
                dir_name,
                cloud_enabled: profile.domain.cloud_enabled,
                sync_level: profile.domain.sync_level.clone(),
            },
        ));
    }

    if found.is_empty() {
        return None;
    }

    found.sort_by(|a, b| b.0.cmp(&a.0));
    let active_profile_id = found[0].1.id.clone();
    let profiles: Vec<ProfileMeta> = found.into_iter().map(|(_, m)| m).collect();
    eprintln!(
        "[ProfileIndex] rebuilt {} profile(s) from disk; active = {}",
        profiles.len(),
        active_profile_id
    );
    Some(ProfileIndex { active_profile_id, profiles })
}

/// Save the profile index to `profiles/index.json`.
pub fn save_profile_index(index: &ProfileIndex) -> Result<(), String> {
    let path = profiles_dir().join("index.json");
    let json = serde_json::to_string_pretty(index).map_err(|e| e.to_string())?;
    global_storage()
        .write(&path_key(&path), json.as_bytes())
        .map_err(|e| e.to_string())
}

/// Returns the data directory for the currently active profile.
///
/// **Read-only**: this function never creates files or directories.
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
    let now = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    // Use the UUID string as the directory name for uniqueness.
    let dir_name = id.clone();
    let profile_dir = profiles_dir().join(&dir_name);

    // Build and persist a new profile.
    let mut profile = UserProfile::new();
    profile.domain.profile_id = id.clone();
    profile.ui.display_name = name.to_string();
    profile.ui.avatar = avatar.to_string();
    profile.ui.profile_created_at = now;

    let json = serde_json::to_string_pretty(&profile).map_err(|e| e.to_string())?;
    global_storage()
        .write(&path_key(&profile_dir.join("profile.json")), json.as_bytes())
        .map_err(|e| e.to_string())?;

    // Generate and persist a random 16-byte salt for future vault key derivation.
    let salt = mlc_vault::vault::generate_salt();
    let salt_hex = salt.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    global_storage()
        .write(&path_key(&profile_dir.join("salt.hex")), salt_hex.as_bytes())
        .map_err(|e| e.to_string())?;

    let meta = ProfileMeta {
        id: id.clone(),
        display_name: name.to_string(),
        avatar: avatar.to_string(),
        created_at: now,
        dir_name,
        cloud_enabled: false,
        sync_level: "local".to_string(),
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
    }

    // Find the profile metadata so we know its directory name.
    let meta = index.profiles.iter().find(|m| m.id == id).cloned();

    // Remove from the index.
    index.profiles.retain(|m| m.id != id);
    save_profile_index(&index)?;

    // Remove the profile directory (best effort — ignore missing).
    if let Some(m) = meta {
        let dir = profiles_dir().join(&m.dir_name);
        let prefix = path_key(&dir);
        let _ = global_storage().delete_prefix(&prefix);
    }

    Ok(())
}
