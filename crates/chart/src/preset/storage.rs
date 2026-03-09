//! File I/O storage layer for [`ChartPreset`] persistence.
//!
//! Presets are stored as pretty-printed JSON files in a `presets/` directory
//! under the OS application data directory for zengeld.  Each file is named
//! `{id}.json`.
//!
//! # Directory resolution
//!
//! [`presets_dir`] delegates to [`crate::user_profile::storage::app_data_dir`]
//! and appends `presets/`.  The directory is created automatically on first
//! access.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::preset::ChartPreset;

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
/// Located under the OS application data directory:
/// `{APP_DATA_DIR}/zengeld/presets/`
pub fn presets_dir() -> PathBuf {
    let dir = crate::user_profile::storage::app_data_dir().join("presets");
    // Best-effort creation; callers will receive an Io error on the actual
    // read/write if the directory cannot be created.
    let _ = fs::create_dir_all(&dir);
    dir
}

// =============================================================================
// CRUD operations
// =============================================================================

/// Serialize `preset` to pretty JSON and write it to `{presets_dir}/{id}.json`.
///
/// Returns the path of the file that was written.
pub fn save_preset(preset: &ChartPreset) -> Result<PathBuf, PresetError> {
    let dir = presets_dir();
    // Ensure the directory exists (may have been deleted since startup).
    fs::create_dir_all(&dir)?;

    let path = dir.join(format!("{}.json", preset.id));
    let json = serde_json::to_string_pretty(preset)?;
    fs::write(&path, json)?;
    Ok(path)
}

/// Load and deserialize a preset from `{presets_dir}/{id}.json`.
pub fn load_preset(id: &str) -> Result<ChartPreset, PresetError> {
    let path = presets_dir().join(format!("{}.json", id));

    if !path.exists() {
        return Err(PresetError::NotFound(id.to_string()));
    }

    let json = fs::read_to_string(&path)?;
    let preset: ChartPreset = serde_json::from_str(&json)?;
    Ok(preset)
}

/// Scan the presets directory and return lightweight metadata for every
/// `*.json` file found.
///
/// Files that cannot be parsed are silently skipped so that a single corrupt
/// file does not prevent listing the rest.
pub fn list_presets() -> Result<Vec<PresetMeta>, PresetError> {
    let dir = presets_dir();

    // If the directory does not exist yet, return an empty list rather than
    // an error — the user simply has no saved presets.
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut metas: Vec<PresetMeta> = Vec::new();

    for entry in fs::read_dir(&dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();

        // Only process *.json files.
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        // Deserialize the full preset to extract meta fields.
        // This is acceptable for the typical number of presets a user will
        // have (tens, not thousands).
        let contents = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let preset: ChartPreset = match serde_json::from_str(&contents) {
            Ok(p) => p,
            Err(_) => continue,
        };

        metas.push(PresetMeta::from(&preset));
    }

    // Sort by creation time, oldest first.
    metas.sort_by_key(|m| m.created_at);

    Ok(metas)
}

/// Delete the preset file for `id`.
///
/// Returns `Ok(())` if the file was removed successfully.
/// Returns [`PresetError::NotFound`] if no file exists for that `id`.
pub fn delete_preset(id: &str) -> Result<(), PresetError> {
    let path = presets_dir().join(format!("{}.json", id));

    if !path.exists() {
        return Err(PresetError::NotFound(id.to_string()));
    }

    fs::remove_file(&path)?;
    Ok(())
}
