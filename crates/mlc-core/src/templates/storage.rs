//! Storage layer for the template system.
//!
//! Templates are stored as pretty-printed JSON files in subdirectories under
//! the OS application data directory for zengeld, mirroring the preset storage
//! pattern.
//!
//! # Directory layout
//!
//! ```text
//! {APP_DATA_DIR}/mylittlechart/
//!   profiles/{active}/
//!     templates/
//!       primitives/       — PrimitiveTemplate  ({id}.json)
//!       indicators/       — IndicatorTemplate  ({id}.json)
//!       compare/          — CompareTemplate    ({id}.json)
//!       indicator_sets/   — IndicatorSet       ({id}.json)
//! ```
//!
//! All I/O is routed through [`mlc_crypto::global_storage()`].

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

use mlc_crypto::vault::{self, VaultKey};
use mlc_crypto::global_storage;

// =============================================================================
// TemplateError
// =============================================================================

/// Errors that can arise from template storage operations.
#[derive(Debug)]
pub enum TemplateError {
    /// An underlying I/O error.
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
// Path helpers
// =============================================================================

/// Convert an absolute `PathBuf` to a storage key relative to app_data_dir.
fn path_key(path: &PathBuf) -> String {
    let root = crate::user_profile::storage::app_data_dir();
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

/// Returns the root `templates/` directory path (no I/O).
pub fn templates_root() -> PathBuf {
    crate::user_profile::storage::active_profile_data_dir().join("templates")
}

/// Returns the sub-directory path for a given category (no I/O).
///
/// `category` should be one of `"primitives"`, `"indicators"`, `"compare"`,
/// or `"indicator_sets"`.
pub fn category_dir(category: &str) -> PathBuf {
    templates_root().join(category)
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
    let path = dir.join(format!("{}.json", id));
    let json = serde_json::to_string_pretty(template)?;
    global_storage().write(&path_ref_key(&path), json.as_bytes())?;
    // Remove any leftover encrypted version from before the plaintext-only policy.
    let enc_path = dir.join(format!("{}.enc", id));
    let _ = global_storage().delete(&path_ref_key(&enc_path));
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
    let storage = global_storage();

    // Migration path: decrypt the legacy .enc file, persist as plaintext, then
    // fall through to the normal JSON load.
    if storage.exists(&path_ref_key(&enc_path)) {
        if let Some(k) = key {
            match vault::load_encrypted::<T>(k, &enc_path) {
                Ok(value) => {
                    if let Ok(json) = serde_json::to_string_pretty(&value) {
                        let _ = storage.write(&path_ref_key(&json_path), json.as_bytes());
                    }
                    let _ = storage.delete(&path_ref_key(&enc_path));
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

    match storage.read(&path_ref_key(&json_path))? {
        None => Err(TemplateError::NotFound(id.to_string())),
        Some(bytes) => {
            let json = String::from_utf8(bytes)
                .map_err(|e| std::io::Error::other(format!("UTF-8: {e}")))?;
            Ok(serde_json::from_str(&json)?)
        }
    }
}

/// Load all templates of type `T` from `dir`.
///
/// Always reads `.json` files as plaintext.  Legacy `.enc` files are migrated
/// on the fly when a key is provided (decrypt → write `.json` → delete `.enc`).
/// `.enc` files without a key are silently skipped.  Corrupted or unreadable
/// files are logged and skipped.
pub fn load_all_templates<T: DeserializeOwned + Serialize>(dir: &Path, key: Option<&VaultKey>) -> Vec<T> {
    let mut results = Vec::new();
    let storage = global_storage();
    let prefix = path_ref_key(dir);

    let entries = match storage.list_prefix(&prefix) {
        Ok(e) => e,
        Err(_) => return results,
    };

    for name in entries {
        let path = dir.join(&name);
        let ext = std::path::Path::new(&name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        match ext {
            "enc" => {
                if let Some(k) = key {
                    match vault::load_encrypted::<T>(k, &path) {
                        Ok(t) => {
                            // Migrate: write plaintext JSON and remove .enc.
                            let json_path = path.with_extension("json");
                            if let Ok(json) = serde_json::to_string_pretty(&t) {
                                let _ = storage.write(&path_ref_key(&json_path), json.as_bytes());
                            }
                            let _ = storage.delete(&path_ref_key(&path));
                            results.push(t);
                        }
                        Err(e) => eprintln!("[templates] failed to decrypt legacy {}: {}", path.display(), e),
                    }
                }
                // No key: silently skip .enc files.
            }
            "json" => {
                match storage.read(&path_ref_key(&path)) {
                    Ok(Some(bytes)) => {
                        match String::from_utf8(bytes) {
                            Ok(json) => match serde_json::from_str::<T>(&json) {
                                Ok(t) => results.push(t),
                                Err(e) => eprintln!("[templates] failed to parse {}: {}", path.display(), e),
                            },
                            Err(e) => eprintln!("[templates] UTF-8 error {}: {}", path.display(), e),
                        }
                    }
                    Ok(None) => {}
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
    let storage = global_storage();
    let _ = storage.delete(&path_ref_key(&dir.join(format!("{}.json", id))));
    let _ = storage.delete(&path_ref_key(&dir.join(format!("{}.enc", id))));
    Ok(())
}
