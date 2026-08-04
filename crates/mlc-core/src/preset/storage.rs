//! Storage layer for [`ChartPreset`] persistence.
//!
//! Presets are stored as pretty-printed JSON files in a `presets/` directory
//! under the OS application data directory for zengeld.  Each file is named
//! `{id}.json`.
//!
//! # Directory resolution
//!
//! [`presets_dir`] delegates to [`crate::user_profile::storage::active_profile_data_dir`]
//! and appends `presets/`.  The directory is created automatically on first
//! access (write path only).
//!
//! # Storage backend
//!
//! All I/O is routed through [`mlc_vault::global_storage()`] so the same
//! code runs on native (filesystem) and wasm (localStorage).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::preset::ChartPreset;
use mlc_vault::vault::{self, VaultKey};
use mlc_vault::global_storage;

// =============================================================================
// PresetMeta
// =============================================================================

/// Lightweight metadata for a preset, suitable for listing without loading the
/// full preset payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetMeta {
    /// Unique preset identifier (matches the filename without `.json`).
    pub id: String,
    /// User-visible display name.
    pub name: String,
    /// Unix timestamp (seconds) when the preset was created.
    pub created_at: u64,
}

impl From<&ChartPreset> for PresetMeta {
    fn from(preset: &ChartPreset) -> Self {
        Self {
            id: preset.id.clone(),
            name: preset.name.clone(),
            created_at: preset.created_at,
        }
    }
}

// =============================================================================
// PresetError
// =============================================================================

/// Errors that can arise from preset storage operations.
#[derive(Debug)]
pub enum PresetError {
    /// An underlying I/O error.
    Io(std::io::Error),
    /// JSON serialization or deserialization failure.
    Json(serde_json::Error),
    /// No preset found with the given identifier.
    NotFound(String),
}

impl std::fmt::Display for PresetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PresetError::Io(e) => write!(f, "preset I/O error: {}", e),
            PresetError::Json(e) => write!(f, "preset JSON error: {}", e),
            PresetError::NotFound(id) => write!(f, "preset not found: {}", id),
        }
    }
}

impl std::error::Error for PresetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PresetError::Io(e) => Some(e),
            PresetError::Json(e) => Some(e),
            PresetError::NotFound(_) => None,
        }
    }
}

impl From<std::io::Error> for PresetError {
    fn from(e: std::io::Error) -> Self {
        PresetError::Io(e)
    }
}

impl From<serde_json::Error> for PresetError {
    fn from(e: serde_json::Error) -> Self {
        PresetError::Json(e)
    }
}

// =============================================================================
// Path helpers (no I/O — path computation only)
// =============================================================================

/// Returns the path to the `presets/` directory (no I/O, path-only).
///
/// Located under the active profile's data directory:
/// `{APP_DATA_DIR}/zengeld/profiles/{active}/presets/`
pub fn presets_dir() -> PathBuf {
    crate::user_profile::storage::active_profile_data_dir().join("presets")
}

/// Convert an absolute `PathBuf` to a storage key (forward-slash relative path
/// relative to app_data_dir, or the full path if outside).
fn path_key(path: &PathBuf) -> String {
    let root = crate::user_profile::storage::app_data_dir();
    if let Ok(rel) = path.strip_prefix(&root) {
        rel.to_string_lossy().replace('\\', "/")
    } else {
        path.to_string_lossy().replace('\\', "/")
    }
}

/// Storage key for the presets directory prefix (used for listing).
fn presets_prefix() -> String {
    path_key(&presets_dir())
}

// =============================================================================
// CRUD operations
// =============================================================================

/// Save preset — always plaintext JSON.
///
/// Presets contain no sensitive data and are always stored as `{id}.json`.
/// Any existing `{id}.enc` (from a previous encrypted session) is removed.
/// Unlike [`load_preset`], this never takes a vault key — there is nothing
/// to encrypt on write. (If presets should be encrypted like templates,
/// that is a separate product decision, not covered here.)
pub fn save_preset(preset: &ChartPreset) -> Result<PathBuf, PresetError> {
    let dir = presets_dir();
    let path = dir.join(format!("{}.json", preset.id));

    // Undo/redo history is bounded on the way out — the newest steps only,
    // under a step count AND a byte budget, because a single recorded
    // delete-all carries every primitive it removed. The viewport field is
    // `ViewportPersist` (bar_spacing only), so nothing else needs trimming.
    let mut bounded = preset.clone();
    for win in &mut bounded.windows {
        if let Some(ref mut h) = win.command_history {
            *h = h.bounded_for_persistence();
        }
        if let Some(ref mut h) = win.stashed_command_history {
            *h = h.bounded_for_persistence();
        }
    }
    for group in &mut bounded.bubbles {
        if let Some(ref mut h) = group.command_history {
            *h = h.bounded_for_persistence();
        }
    }

    let json = serde_json::to_string_pretty(&bounded)?;
    global_storage().write(&path_key(&path), json.as_bytes())?;

    // Remove any leftover encrypted version from before the plaintext-only policy.
    let enc_path = dir.join(format!("{}.enc", preset.id));
    let _ = global_storage().delete(&path_key(&enc_path));

    Ok(path)
}

/// Load preset — always plaintext JSON.
///
/// If an `.enc` file exists (from a previous encrypted session) and a key is
/// provided, the file is decrypted, re-saved as `.json`, and the `.enc` file
/// is deleted (one-time migration).  If no key is available the `.enc` file is
/// skipped and the `.json` version is tried instead.
pub fn load_preset(id: &str, key: Option<&VaultKey>) -> Result<ChartPreset, PresetError> {
    let dir = presets_dir();
    let enc_path = dir.join(format!("{}.enc", id));
    let json_path = dir.join(format!("{}.json", id));

    // Migration path: decrypt the legacy .enc file, persist as plaintext, then
    // fall through to the normal JSON load below.
    if global_storage().exists(&path_key(&enc_path)) {
        if let Some(k) = key {
            match vault::load_encrypted::<ChartPreset>(k, &enc_path) {
                Ok(preset) => {
                    // Re-save as plaintext and delete the .enc file.
                    if let Ok(json) = serde_json::to_string_pretty(&preset) {
                        let _ = global_storage().write(&path_key(&json_path), json.as_bytes());
                    }
                    let _ = global_storage().delete(&path_key(&enc_path));
                    return Ok(preset);
                }
                Err(e) => {
                    eprintln!("[presets] failed to decrypt legacy {}.enc: {}", id, e);
                    // Fall through to try .json below.
                }
            }
        }
        // No key available — skip the .enc file and try .json.
    }

    if let Some(bytes) = global_storage().read(&path_key(&json_path))? {
        let json = String::from_utf8(bytes)
            .map_err(|e| std::io::Error::other(format!("UTF-8 decode: {e}")))?;
        let preset: ChartPreset = serde_json::from_str(&json)?;
        return Ok(preset);
    }

    Err(PresetError::NotFound(id.to_string()))
}

/// List presets — handles both `.enc` and `.json` files.
///
/// Encrypted files without a key are silently skipped.  Corrupted files are
/// logged and skipped so that one bad file does not prevent listing the rest.
pub fn list_presets(key: Option<&VaultKey>) -> Result<Vec<PresetMeta>, PresetError> {
    let dir = presets_dir();
    let prefix = presets_prefix();
    let storage = global_storage();

    let entries = match storage.list_prefix(&prefix) {
        Ok(e) => e,
        Err(_) => return Ok(Vec::new()),
    };

    let mut metas = Vec::new();
    for name in entries {
        let path = dir.join(&name);
        let ext = std::path::Path::new(&name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let stem = std::path::Path::new(&name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        if stem.is_empty() {
            continue;
        }

        match ext {
            "enc" => {
                if let Some(k) = key {
                    match vault::load_encrypted::<ChartPreset>(k, &path) {
                        Ok(p) => metas.push(PresetMeta {
                            id: p.id.clone(),
                            name: p.name.clone(),
                            created_at: p.created_at,
                        }),
                        Err(e) => eprintln!("[presets] failed to load encrypted preset {}: {}", stem, e),
                    }
                }
            }
            "json" => {
                match storage.read(&path_key(&path)) {
                    Ok(Some(bytes)) => {
                        match String::from_utf8(bytes) {
                            Ok(json) => match serde_json::from_str::<ChartPreset>(&json) {
                                Ok(p) => metas.push(PresetMeta {
                                    id: p.id.clone(),
                                    name: p.name.clone(),
                                    created_at: p.created_at,
                                }),
                                Err(e) => eprintln!("[presets] failed to parse {}: {}", path.display(), e),
                            },
                            Err(e) => eprintln!("[presets] UTF-8 decode error for {}: {}", path.display(), e),
                        }
                    }
                    Ok(None) => {}
                    Err(e) => eprintln!("[presets] failed to read {}: {}", path.display(), e),
                }
            }
            _ => {}
        }
    }

    // Sort by creation time, oldest first.
    metas.sort_by_key(|m| m.created_at);
    Ok(metas)
}

/// Delete preset — removes both `.json` and `.enc` variants if present.
pub fn delete_preset(id: &str) -> Result<(), PresetError> {
    let dir = presets_dir();
    let storage = global_storage();
    let _ = storage.delete(&path_key(&dir.join(format!("{}.json", id))));
    let _ = storage.delete(&path_key(&dir.join(format!("{}.enc", id))));
    Ok(())
}
