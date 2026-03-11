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

use super::profile::UserProfile;

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

/// Serialize `profile` as pretty JSON and write it to `user_data/profile.json`.
///
/// The `user_data/` directory is created automatically if it does not exist.
pub fn save_profile(profile: &UserProfile) -> Result<(), ProfileError> {
    let dir = get_user_data_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join("profile.json");
    let json = serde_json::to_string_pretty(profile)?;
    fs::write(&path, json)?;
    Ok(())
}

/// Load and deserialize the user profile from `user_data/profile.json`.
///
/// Returns a default [`UserProfile`] if the file does not exist, so startup
/// always succeeds even without prior data.
pub fn load_profile() -> Result<UserProfile, ProfileError> {
    let path = get_user_data_dir().join("profile.json");
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

/// Returns the path to `user_data/watchlists.json`.
pub fn watchlists_path() -> PathBuf {
    get_user_data_dir().join("watchlists.json")
}
