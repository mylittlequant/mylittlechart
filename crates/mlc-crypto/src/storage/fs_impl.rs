//! Filesystem-backed [`Storage`] implementation for native targets.

use std::io;
use std::path::PathBuf;

use super::Storage;

// =============================================================================
// app_data_dir (mirrors mlc-core's user_profile::storage::app_data_dir)
// =============================================================================

/// Resolves the OS application-data root for mylittlechart.
///
/// Platform paths:
/// - Windows: `%APPDATA%\mylittlechart\`
/// - macOS:   `~/Library/Application Support/mylittlechart/`
/// - Linux:   `$XDG_DATA_HOME/mylittlechart/` or `~/.local/share/mylittlechart/`
///
/// This duplicates the logic in `mlc-core::user_profile::storage::app_data_dir`
/// to keep `mlc-crypto` a pure leaf with no upward dependencies.
pub(crate) fn app_data_dir() -> PathBuf {
    let base = resolve_platform_data_dir();
    let dir = base.join("mylittlechart");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn resolve_platform_data_dir() -> PathBuf {
    if let Ok(v) = std::env::var("APPDATA") {
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    if let Ok(v) = std::env::var("XDG_DATA_HOME") {
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            let h = PathBuf::from(home);
            if cfg!(target_os = "macos") {
                return h.join("Library").join("Application Support");
            }
            return h.join(".local").join("share");
        }
    }
    if let Ok(v) = std::env::var("USERPROFILE") {
        if !v.is_empty() {
            return PathBuf::from(v).join("AppData").join("Roaming");
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

// =============================================================================
// FilesystemStorage
// =============================================================================

/// [`Storage`] implementation backed by the OS filesystem.
///
/// All keys are treated as forward-slash-separated relative paths and joined
/// onto the `root` directory supplied at construction time.
pub struct FilesystemStorage {
    root: PathBuf,
}

impl FilesystemStorage {
    /// Create a new instance rooted at `root`.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn key_to_path(&self, key: &str) -> PathBuf {
        // Split on '/' and join onto root so this works on Windows too.
        let mut p = self.root.clone();
        for component in key.split('/') {
            if !component.is_empty() {
                p.push(component);
            }
        }
        p
    }
}

impl Storage for FilesystemStorage {
    fn read(&self, key: &str) -> io::Result<Option<Vec<u8>>> {
        let path = self.key_to_path(key);
        match std::fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn write(&self, key: &str, bytes: &[u8]) -> io::Result<()> {
        let path = self.key_to_path(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes)
    }

    fn delete(&self, key: &str) -> io::Result<()> {
        let path = self.key_to_path(key);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn delete_prefix(&self, prefix: &str) -> io::Result<()> {
        let path = self.key_to_path(prefix);
        match std::fs::remove_dir_all(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn list_prefix(&self, prefix: &str) -> io::Result<Vec<String>> {
        let path = self.key_to_path(prefix);
        let mut names = Vec::new();
        let entries = match std::fs::read_dir(&path) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(names),
            Err(e) => return Err(e),
        };
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
        Ok(names)
    }

    fn exists(&self, key: &str) -> bool {
        self.key_to_path(key).exists()
    }
}
