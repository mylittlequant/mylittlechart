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
//! - [`save_profile`] / [`load_profile`] — write/read `profile.json`.
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

use super::profile::{ProfileIndex, ProfileMeta, UserProfile};

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

/// Returns the root application data directory for zengeld, creating it if
/// necessary.
///
/// Platform-specific paths:
/// - Windows: `%APPDATA%\zengeld\`
/// - macOS:   `~/Library/Application Support/zengeld/`
/// - Linux:   `$XDG_DATA_HOME/zengeld/` (default `~/.local/share/zengeld/`)
pub fn app_data_dir() -> PathBuf {
    let base = resolve_platform_data_dir();
    let dir = base.join("zengeld");
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

/// Returns the root application data directory.
///
/// Backward-compatible alias for [`app_data_dir`].  All existing callers
/// automatically use the new OS-standard path without any further changes.
pub fn get_user_data_dir() -> PathBuf {
    app_data_dir()
}

// =============================================================================
// profile.json helpers
// =============================================================================

/// Serialize `profile` as pretty JSON and write it to the active profile's
/// `profile.json` (routed through `active_profile_data_dir()`).
///
/// The directory is created automatically if it does not exist.
pub fn save_profile(profile: &UserProfile) -> Result<(), ProfileError> {
    let dir = active_profile_data_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join("profile.json");
    let json = serde_json::to_string_pretty(profile)?;
    fs::write(&path, json)?;
    Ok(())
}

/// Load and deserialize the user profile from the active profile's
/// `profile.json` (routed through `active_profile_data_dir()`).
///
/// Returns a default [`UserProfile`] if the file does not exist, so startup
/// always succeeds even without prior data.
pub fn load_profile() -> Result<UserProfile, ProfileError> {
    let path = active_profile_data_dir().join("profile.json");
    if !path.exists() {
        return Ok(UserProfile::new());
    }
    let json = fs::read_to_string(&path)?;
    let mut profile: UserProfile = serde_json::from_str(&json)?;
    // Migrate legacy single chat_id to subscribers list.
    profile.notification_settings.telegram.migrate_legacy();
    Ok(profile)
}

// =============================================================================
// Generic JSON helpers
// =============================================================================

/// Serialize `data` as pretty JSON and write it to `path`.
///
/// The parent directory is created automatically if it does not exist.
pub fn save_json<T: Serialize>(path: &Path, data: &T) -> Result<(), ProfileError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(data)?;
    fs::write(path, json)?;
    Ok(())
}

/// Load and deserialize a value of type `T` from `path`.
///
/// Returns `Err(ProfileError::Io)` if the file does not exist (use the calling
/// code to decide on a fallback default).
pub fn load_json<T: DeserializeOwned>(path: &Path) -> Result<T, ProfileError> {
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
    serde_json::from_str(&json).ok()
}

/// Save the profile index to `profiles/index.json`.
pub fn save_profile_index(index: &ProfileIndex) -> Result<(), String> {
    let path = profiles_dir().join("index.json");
    let json = serde_json::to_string_pretty(index).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

/// Returns the data directory for the currently active profile.
///
/// - If a `profiles/index.json` exists, reads the active profile's `dir_name`
///   and returns `profiles/{dir_name}/`.
/// - If no index exists (legacy install or fresh install), falls back to
///   `app_data_dir()` so existing code continues to work unchanged.
pub fn active_profile_data_dir() -> PathBuf {
    if let Some(index) = load_profile_index() {
        if let Some(meta) = index
            .profiles
            .iter()
            .find(|m| m.id == index.active_profile_id)
        {
            let dir = profiles_dir().join(&meta.dir_name);
            let _ = fs::create_dir_all(&dir);
            return dir;
        }
    }
    // Legacy fallback — root app data dir
    app_data_dir()
}

// =============================================================================
// Legacy profile migration
// =============================================================================

/// Migrate an existing flat-layout profile into the new `profiles/` structure.
///
/// Returns `Ok(true)` if migration was performed, `Ok(false)` if it was
/// skipped (already migrated or no existing data to migrate).
///
/// Migration steps:
/// 1. Copy `profile.json` → `profiles/default/profile.json`
/// 2. Move `presets/`, `watchlists.json`, `templates/`, `snapshots/` into
///    `profiles/default/`
/// 3. Assign a UUID and creation timestamp to the migrated profile.
/// 4. Write `profiles/index.json` with this profile as active.
pub fn migrate_legacy_profile_if_needed() -> Result<bool, String> {
    let index_path = profiles_dir().join("index.json");
    if index_path.exists() {
        // Already migrated.
        return Ok(false);
    }

    let root = app_data_dir();
    let legacy_profile = root.join("profile.json");
    if !legacy_profile.exists() {
        // Fresh install — nothing to migrate.
        return Ok(false);
    }

    // Create the default profile subdirectory.
    let default_dir = profiles_dir().join("default");
    fs::create_dir_all(&default_dir).map_err(|e| e.to_string())?;

    // Move profile.json.
    fs::rename(&legacy_profile, default_dir.join("profile.json")).map_err(|e| e.to_string())?;

    // Move optional data files/dirs (best effort — ignore missing).
    let moves: &[(&str, &str)] = &[
        ("presets", "presets"),
        ("watchlists.json", "watchlists.json"),
        ("templates", "templates"),
        ("snapshots", "snapshots"),
    ];
    for (src_name, dst_name) in moves {
        let src = root.join(src_name);
        if src.exists() {
            let dst = default_dir.join(dst_name);
            let _ = fs::rename(&src, &dst);
        }
    }

    // Load the migrated profile, assign UUID + timestamp, save back.
    let profile_path = default_dir.join("profile.json");
    let mut profile: UserProfile = fs::read_to_string(&profile_path)
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_else(UserProfile::new);

    let new_id = uuid::Uuid::new_v4().to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    profile.profile_id = new_id.clone();
    profile.profile_created_at = now;
    if profile.display_name.is_empty() || profile.display_name == "Default" {
        profile.display_name = "Default".to_string();
    }
    if profile.avatar.is_empty() {
        profile.avatar = "chart".to_string();
    }

    let json = serde_json::to_string_pretty(&profile).map_err(|e| e.to_string())?;
    fs::write(&profile_path, json).map_err(|e| e.to_string())?;

    // Write the index.
    let meta = ProfileMeta {
        id: new_id.clone(),
        display_name: profile.display_name.clone(),
        avatar: profile.avatar.clone(),
        created_at: now,
        dir_name: "default".to_string(),
    };
    let index = ProfileIndex {
        active_profile_id: new_id,
        profiles: vec![meta],
    };
    save_profile_index(&index)?;

    Ok(true)
}

// =============================================================================
// Profile creation
// =============================================================================

/// Create a new profile with the given display name and avatar.
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
