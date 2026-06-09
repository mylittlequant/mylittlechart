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
//! are skipped silently in [`load_all_templates`] so a single bad file does not
//! prevent loading the rest.

use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::vault::{self, VaultKey};

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
/// Located under the active profile's data directory:
/// `{APP_DATA_DIR}/zengeld/profiles/{active}/templates/`
pub fn templates_root() -> PathBuf {
    let dir = crate::user_profile::storage::active_profile_data_dir().join("templates");
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

/// Save a template — always plaintext JSON.
///
/// The `key` parameter is accepted for API compatibility but is ignored.
/// Templates contain no sensitive data and are always stored as `{id}.json`.
/// Any existing `{id}.enc` (from a previous encrypted session) is removed.
pub fn save_template<T: Serialize>(
    template: &T,
    id: &str,
    dir: &Path,
    _key: Option<&VaultKey>,
) -> Result<(), TemplateError> {
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.json", id));
    let json = serde_json::to_string_pretty(template)?;
    fs::write(&path, &json)?;
    // Remove any leftover encrypted version from before the plaintext-only policy.
    let _ = fs::remove_file(dir.join(format!("{}.enc", id)));
    Ok(())
}

/// Load a template — always plaintext JSON.
///
/// If an `.enc` file exists (from a previous encrypted session) and a key is
/// provided, the file is decrypted, re-saved as `.json`, and the `.enc` file
/// is deleted (one-time migration).  If no key is available the `.enc` file is
/// skipped and the `.json` version is tried instead.
pub fn load_template<T: DeserializeOwned + Serialize>(
    id: &str,
    dir: &Path,
    key: Option<&VaultKey>,
) -> Result<T, TemplateError> {
    let enc_path = dir.join(format!("{}.enc", id));
    let json_path = dir.join(format!("{}.json", id));

    // Migration path: decrypt the legacy .enc file, persist as plaintext, then
    // fall through to the normal JSON load.
    if enc_path.exists() {
        if let Some(k) = key {
            match vault::load_encrypted::<T>(k, &enc_path) {
                Ok(value) => {
                    if let Ok(json) = serde_json::to_string_pretty(&value) {
                        let _ = fs::write(&json_path, json);
                    }
                    let _ = fs::remove_file(&enc_path);
                    return Ok(value);
                }
                Err(e) => {
                    eprintln!("[templates] failed to decrypt legacy {}.enc: {}", id, e);
                    // Fall through to try .json.
                }
            }
        }
        // No key — skip .enc and try .json.
    }

    if !json_path.exists() {
        return Err(TemplateError::NotFound(id.to_string()));
    }

    let json = fs::read_to_string(&json_path)?;
    Ok(serde_json::from_str(&json)?)
}

/// Load all templates of type `T` from `dir`.
///
/// Always reads `.json` files as plaintext.  Legacy `.enc` files are migrated
/// on the fly when a key is provided (decrypt → write `.json` → delete `.enc`).
/// `.enc` files without a key are silently skipped.  Corrupted or unreadable
/// files are logged and skipped.
pub fn load_all_templates<T: DeserializeOwned + Serialize>(dir: &Path, key: Option<&VaultKey>) -> Vec<T> {
    let mut results = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return results,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext {
            "enc" => {
                if let Some(k) = key {
                    match vault::load_encrypted::<T>(k, &path) {
                        Ok(t) => {
                            // Migrate: write plaintext JSON and remove .enc.
                            let json_path = path.with_extension("json");
                            if let Ok(json) = serde_json::to_string_pretty(&t) {
                                let _ = fs::write(&json_path, json);
                            }
                            let _ = fs::remove_file(&path);
                            results.push(t);
                        }
                        Err(e) => eprintln!("[templates] failed to decrypt legacy {}: {}", path.display(), e),
                    }
                }
                // No key: silently skip .enc files.
            }
            "json" => {
                match fs::read_to_string(&path) {
                    Ok(json) => match serde_json::from_str::<T>(&json) {
                        Ok(t) => results.push(t),
                        Err(e) => eprintln!("[templates] failed to parse {}: {}", path.display(), e),
                    },
                    Err(e) => eprintln!("[templates] failed to read {}: {}", path.display(), e),
                }
            }
            _ => {}
        }
    }
    results
}

/// Delete a template — removes both `.json` and `.enc` variants if present.
pub fn delete_template(id: &str, dir: &Path) -> Result<(), TemplateError> {
    let _ = fs::remove_file(dir.join(format!("{}.json", id)));
    let _ = fs::remove_file(dir.join(format!("{}.enc", id)));
    Ok(())
}
