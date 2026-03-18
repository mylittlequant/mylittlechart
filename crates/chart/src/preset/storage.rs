//! File I/O storage layer for [`ChartPreset`] persistence.
//!
//! Presets are stored as pretty-printed JSON files in a `presets/` directory
//! under the OS application data directory for zengeld.  Each file is named
//! `{id}.json`.
//!
//! # Directory resolution
//!
//! [`presets_dir`] delegates to [`crate::user_profile::storage::active_profile_data_dir`]
//! and appends `presets/`.  The directory is created automatically on first
//! access.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::preset::ChartPreset;
use crate::vault::{self, VaultKey};

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
    /// An underlying filesystem I/O error.
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
// Directory
// =============================================================================

/// Returns the path to the `presets/` directory, creating it if necessary.
///
/// Located under the active profile's data directory:
/// `{APP_DATA_DIR}/zengeld/profiles/{active}/presets/`
pub fn presets_dir() -> PathBuf {
    let dir = crate::user_profile::storage::active_profile_data_dir().join("presets");
    // Best-effort creation; callers will receive an Io error on the actual
    // read/write if the directory cannot be created.
    let _ = fs::create_dir_all(&dir);
    dir
}

// =============================================================================
// CRUD operations
// =============================================================================

/// Save preset — always plaintext JSON.
///
/// The `key` parameter is accepted for API compatibility but is ignored.
/// Presets contain no sensitive data and are always stored as `{id}.json`.
/// Any existing `{id}.enc` (from a previous encrypted session) is removed.
pub fn save_preset(preset: &ChartPreset, _key: Option<&VaultKey>) -> Result<PathBuf, PresetError> {
    let dir = presets_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", preset.id));

    // Strip viewport-related data before writing to disk.
    // Viewport is device-local — recalculated on bar load. Persisting it
    // causes stale-viewport conflicts on restart (set_bars margin vs saved
    // view_start).  ViewportChange commands are also stripped from undo/redo
    // stacks since they reference stale positions.
    let mut stripped = preset.clone();
    for win in &mut stripped.windows {
        win.viewport = Default::default();
        if let Some(ref mut history) = win.command_history {
            *history = history.stripped_for_persistence();
        }
        if let Some(ref mut stashed) = win.stashed_command_history {
            *stashed = stashed.stripped_for_persistence();
        }
    }
    for group in &mut stripped.sync_groups {
        if let Some(ref mut history) = group.command_history {
            *history = history.stripped_for_persistence();
        }
    }

    let json = serde_json::to_string_pretty(&stripped)?;
    fs::write(&path, &json)?;
    // Remove any leftover encrypted version from before the plaintext-only policy.
    let _ = fs::remove_file(dir.join(format!("{}.enc", preset.id)));
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
    if enc_path.exists() {
        if let Some(k) = key {
            match vault::load_encrypted::<ChartPreset>(k, &enc_path) {
                Ok(preset) => {
                    // Re-save as plaintext and delete the .enc file.
                    if let Ok(json) = serde_json::to_string_pretty(&preset) {
                        let _ = fs::write(&json_path, json);
                    }
                    let _ = fs::remove_file(&enc_path);
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

    if json_path.exists() {
        let json = fs::read_to_string(&json_path)?;
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
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut metas = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

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
                match fs::read_to_string(&path) {
                    Ok(json) => match serde_json::from_str::<ChartPreset>(&json) {
                        Ok(p) => metas.push(PresetMeta {
                            id: p.id.clone(),
                            name: p.name.clone(),
                            created_at: p.created_at,
                        }),
                        Err(e) => eprintln!("[presets] failed to parse {}: {}", path.display(), e),
                    },
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
    let _ = fs::remove_file(dir.join(format!("{}.json", id)));
    let _ = fs::remove_file(dir.join(format!("{}.enc", id)));
    Ok(())
}
