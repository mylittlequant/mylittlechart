//! Wasm32 [`Storage`] implementation backed by the browser `localStorage` API.
//!
//! Binary values are hex-encoded before storing (localStorage only accepts
//! strings).  The `hex` crate is already a workspace dependency so no new
//! dep is required.

use std::io;

use super::Storage;

/// [`Storage`] implementation that persists to `window.localStorage`.
pub struct LocalStorageBackend;

impl LocalStorageBackend {
    pub fn new() -> Self {
        Self
    }

    fn get_storage() -> io::Result<web_sys::Storage> {
        web_sys::window()
            .ok_or_else(|| io::Error::other("no window object"))?
            .local_storage()
            .map_err(|_| io::Error::other("localStorage access denied"))?
            .ok_or_else(|| io::Error::other("localStorage is null"))
    }
}

impl Storage for LocalStorageBackend {
    fn read(&self, key: &str) -> io::Result<Option<Vec<u8>>> {
        let storage = Self::get_storage()?;
        match storage
            .get_item(key)
            .map_err(|_| io::Error::other("localStorage.getItem failed"))?
        {
            None => Ok(None),
            Some(hex_str) => {
                let bytes = hex::decode(&hex_str)
                    .map_err(|e| io::Error::other(format!("hex decode: {e}")))?;
                Ok(Some(bytes))
            }
        }
    }

    fn write(&self, key: &str, bytes: &[u8]) -> io::Result<()> {
        let storage = Self::get_storage()?;
        let hex_str = hex::encode(bytes);
        storage
            .set_item(key, &hex_str)
            .map_err(|_| io::Error::other("localStorage.setItem failed"))
    }

    fn delete(&self, key: &str) -> io::Result<()> {
        let storage = Self::get_storage()?;
        storage
            .remove_item(key)
            .map_err(|_| io::Error::other("localStorage.removeItem failed"))
    }

    fn delete_prefix(&self, prefix: &str) -> io::Result<()> {
        let storage = Self::get_storage()?;
        let length = storage
            .length()
            .map_err(|_| io::Error::other("localStorage.length failed"))?;

        // Collect matching keys first to avoid mutating during iteration.
        let mut keys_to_delete: Vec<String> = Vec::new();
        for i in 0..length {
            if let Ok(Some(k)) = storage.key(i) {
                if k.starts_with(prefix) {
                    keys_to_delete.push(k);
                }
            }
        }
        for k in keys_to_delete {
            storage
                .remove_item(&k)
                .map_err(|_| io::Error::other("localStorage.removeItem failed"))?;
        }
        Ok(())
    }

    fn list_prefix(&self, prefix: &str) -> std::io::Result<Vec<String>> {
        let storage = Self::get_storage()?;
        let length = storage
            .length()
            .map_err(|_| std::io::Error::other("localStorage.length failed"))?;

        let mut names = Vec::new();
        for i in 0..length {
            if let Ok(Some(k)) = storage.key(i) {
                if let Some(rest) = k.strip_prefix(prefix) {
                    // Return only the immediate leaf component (first segment).
                    let leaf = rest.trim_start_matches('/');
                    let first = leaf.split('/').next().unwrap_or(leaf);
                    if !first.is_empty() && !names.contains(&first.to_string()) {
                        names.push(first.to_string());
                    }
                }
            }
        }
        Ok(names)
    }

    fn exists(&self, key: &str) -> bool {
        Self::get_storage()
            .ok()
            .and_then(|s| s.get_item(key).ok())
            .flatten()
            .is_some()
    }
}
