//! File I/O storage layer for the template system.
//!
//! Templates are stored as pretty-printed JSON files in subdirectories under
//! the OS application data directory for zengeld, mirroring the preset storage
//! pattern.
//!
//! # Directory layout
//!
//! ```text
//! {APP_DATA_DIR}/zengeld/
//!   templates/
//!     primitives/       — PrimitiveTemplate  ({id}.json)
//!     indicators/       — IndicatorTemplate  ({id}.json)
//!     compare/          — CompareTemplate    ({id}.json)
//!     indicator_sets/   — IndicatorSet       ({id}.json)
//! ```
//!
//! # Error handling
//!
//! All public functions return [`Result<_, TemplateError>`].  Corrupted files
//! are skipped silently in [`list_templates`] so a single bad file does not
//! prevent loading the rest.

use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

// =============================================================================
// TemplateError
// =============================================================================

/// Errors that can arise from template storage operations.
#[derive(Debug)]
pub enum TemplateError {
    /// An underlying filesystem I/O error.
    Io(std::io::Error),
    /// JSON serialization or deserialization failure.
    Json(serde_json::Error),
    /// No template found with the given identifier.
    NotFound(String),
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TemplateError::Io(e) => write!(f, "template I/O error: {}", e),
            TemplateError::Json(e) => write!(f, "template JSON error: {}", e),
            TemplateError::NotFound(id) => write!(f, "template not found: {}", id),
        }
    }
}

impl std::error::Error for TemplateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TemplateError::Io(e) => Some(e),
            TemplateError::Json(e) => Some(e),
            TemplateError::NotFound(_) => None,
        }
    }
}

impl From<std::io::Error> for TemplateError {
    fn from(e: std::io::Error) -> Self {
        TemplateError::Io(e)
    }
}

impl From<serde_json::Error> for TemplateError {
    fn from(e: serde_json::Error) -> Self {
        TemplateError::Json(e)
    }
}

// =============================================================================
// Directory resolution
// =============================================================================

/// Returns the root `templates/` directory, creating it if necessary.
///
/// Located under the OS application data directory:
/// `{APP_DATA_DIR}/zengeld/templates/`
pub fn templates_root() -> PathBuf {
    let dir = crate::user_profile::storage::app_data_dir().join("templates");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Returns the sub-directory for a given category, creating it if necessary.
///
/// `category` should be one of `"primitives"`, `"indicators"`, `"compare"`,
/// or `"indicator_sets"`.
pub fn category_dir(category: &str) -> PathBuf {
    let dir = templates_root().join(category);
    let _ = fs::create_dir_all(&dir);
    dir
}

// =============================================================================
// Generic CRUD operations
// =============================================================================

/// Serialize `template` to pretty JSON and write it to `{dir}/{id}.json`.
///
/// Returns the path of the file that was written.
pub fn save_template<T: Serialize>(
    template: &T,
    id: &str,
    dir: &Path,
) -> Result<PathBuf, TemplateError> {
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.json", id));
    let json = serde_json::to_string_pretty(template)?;
    fs::write(&path, json)?;
    Ok(path)
}

/// Load and deserialize a template from `{dir}/{id}.json`.
pub fn load_template<T: DeserializeOwned>(
    id: &str,
    dir: &Path,
) -> Result<T, TemplateError> {
    let path = dir.join(format!("{}.json", id));

    if !path.exists() {
        return Err(TemplateError::NotFound(id.to_string()));
    }

    let json = fs::read_to_string(&path)?;
    let value: T = serde_json::from_str(&json)?;
    Ok(value)
}

/// Scan `dir` and return the `id` (stem) of every `*.json` file found.
///
/// Files that cannot be read are silently skipped.
pub fn list_templates(dir: &Path) -> Result<Vec<String>, TemplateError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut ids: Vec<String> = Vec::new();

    for entry in fs::read_dir(dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            ids.push(stem.to_string());
        }
    }

    ids.sort();
    Ok(ids)
}

/// Load all templates of type `T` from `dir`.
///
/// Files that fail to deserialize are silently skipped so that a single
/// corrupt file does not prevent loading the rest.
pub fn load_all_templates<T: DeserializeOwned>(dir: &Path) -> Vec<T> {
    if !dir.exists() {
        return Vec::new();
    }

    let mut items: Vec<T> = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return items,
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let contents = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if let Ok(item) = serde_json::from_str::<T>(&contents) {
            items.push(item);
        }
    }

    items
}

/// Delete the template file `{dir}/{id}.json`.
///
/// Returns `Ok(())` if the file was removed successfully.
/// Returns [`TemplateError::NotFound`] if no file exists for that `id`.
pub fn delete_template(id: &str, dir: &Path) -> Result<(), TemplateError> {
    let path = dir.join(format!("{}.json", id));

    if !path.exists() {
        return Err(TemplateError::NotFound(id.to_string()));
    }

    fs::remove_file(&path)?;
    Ok(())
}
