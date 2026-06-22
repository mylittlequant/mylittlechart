//! Platform-agnostic byte-blob storage trait.
//!
//! Keys are forward-slash-separated relative path strings (e.g.
//! `"profiles/abc123/profile.json"`).  Implementations map them to their
//! native representation:
//!
//! - [`FilesystemStorage`] joins the key onto `app_data_dir()` using the
//!   OS path separator.  Parent directories are created lazily on write.
//! - `LocalStorageBackend` (wasm32) uses the key verbatim as a
//!   `localStorage` entry.  Values are hex-encoded since localStorage only
//!   stores strings.
//!
//! # Process-wide handle
//!
//! Use [`global_storage()`] to obtain the process-wide instance.
//! - Native default: [`FilesystemStorage`] rooted at the OS app-data dir.
//! - Wasm default: `LocalStorageBackend`.
//! - Tests / embedders: call [`set_global_storage`] once before the first
//!   [`global_storage()`] call to override.

use std::io;
use std::sync::{Arc, OnceLock};

pub use fs_impl::FilesystemStorage;

pub(crate) mod fs_impl;

#[cfg(target_arch = "wasm32")]
mod wasm_impl;

// =============================================================================
// Storage trait
// =============================================================================

/// Key-value byte-blob store.
///
/// Keys are forward-slash-separated relative strings.  Implementations may
/// interpret path separators differently (filesystem vs. localStorage verbatim),
/// but callers must use `/` exclusively.
///
/// All methods are synchronous.  Filesystem ops block; wasm LocalStorage is
/// already synchronous (browser main thread).  The trait is `Send + Sync` so
/// the global handle can be shared across threads on native.
pub trait Storage: Send + Sync {
    /// Read the value for `key`.
    ///
    /// Returns `Ok(None)` when the key does not exist (missing file /
    /// missing localStorage entry).  Returns `Err` only on a genuine I/O
    /// failure.
    fn read(&self, key: &str) -> io::Result<Option<Vec<u8>>>;

    /// Write `bytes` as the value for `key`.
    ///
    /// Creates any necessary parent structure (directories on filesystem,
    /// no-op on localStorage) before writing.
    fn write(&self, key: &str, bytes: &[u8]) -> io::Result<()>;

    /// Delete the entry for `key`.
    ///
    /// A missing key is treated as a no-op (not an error).
    fn delete(&self, key: &str) -> io::Result<()>;

    /// Delete all entries whose key starts with `prefix`.
    ///
    /// On filesystem this removes the directory subtree rooted at the path
    /// mapped from `prefix`.  On wasm it iterates localStorage and removes
    /// matching keys.
    ///
    /// The default implementation returns `Ok(())` silently (safe no-op for
    /// implementations that do not support prefix deletion).  Override in
    /// impls that need it.
    fn delete_prefix(&self, prefix: &str) -> io::Result<()> {
        let _ = prefix;
        Ok(())
    }

    /// List all keys whose prefix matches `prefix`.
    ///
    /// Returns bare leaf names (not the full key), analogous to `read_dir`.
    /// On filesystem this reads the directory entries under the mapped path.
    /// On wasm it iterates localStorage keys and strips the prefix.
    ///
    /// The default implementation returns an empty list.
    fn list_prefix(&self, prefix: &str) -> io::Result<Vec<String>> {
        let _ = prefix;
        Ok(Vec::new())
    }

    /// Returns `true` when the key has a stored value.
    fn exists(&self, key: &str) -> bool {
        matches!(self.read(key), Ok(Some(_)))
    }
}

// =============================================================================
// Global handle
// =============================================================================

static GLOBAL: OnceLock<Arc<dyn Storage>> = OnceLock::new();

/// Override the process-wide storage backend.
///
/// Must be called before the first [`global_storage()`] invocation.
/// Calling it after the handle has been initialised is a no-op (the
/// `OnceLock` has already been set).
pub fn set_global_storage(s: Arc<dyn Storage>) {
    let _ = GLOBAL.set(s);
}

/// Return the process-wide storage handle.
///
/// The handle is initialised on the first call:
/// - Native: [`FilesystemStorage`] rooted at the OS application-data directory.
/// - Wasm32: `LocalStorageBackend` using the browser `localStorage` API.
pub fn global_storage() -> Arc<dyn Storage> {
    GLOBAL
        .get_or_init(|| {
            #[cfg(not(target_arch = "wasm32"))]
            {
                Arc::new(FilesystemStorage::new(crate::storage::fs_impl::app_data_dir()))
            }
            #[cfg(target_arch = "wasm32")]
            {
                Arc::new(wasm_impl::LocalStorageBackend::new())
            }
        })
        .clone()
}
