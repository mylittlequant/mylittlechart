//! OS keychain integration for secure credential storage.
//! Uses Windows Credential Manager, macOS Keychain, or Linux libsecret.

use keyring::Entry;

const SERVICE_NAME: &str = "mylittlechart";

/// Store a credential in the OS keychain.
/// key_name format: "{exchange}:{account_id}" e.g. "binance:main"
pub fn store_credential(key_name: &str, secret: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, key_name)
        .map_err(|e| format!("keychain entry error: {}", e))?;
    entry.set_password(secret)
        .map_err(|e| format!("keychain store error: {}", e))?;
    Ok(())
}

/// Retrieve a credential from the OS keychain.
pub fn get_credential(key_name: &str) -> Result<Option<String>, String> {
    let entry = Entry::new(SERVICE_NAME, key_name)
        .map_err(|e| format!("keychain entry error: {}", e))?;
    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("keychain read error: {}", e)),
    }
}

/// Delete a credential from the OS keychain.
pub fn delete_credential(key_name: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, key_name)
        .map_err(|e| format!("keychain entry error: {}", e))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()), // already gone
        Err(e) => Err(format!("keychain delete error: {}", e)),
    }
}

/// List all stored credential names (not the secrets themselves).
/// Note: keyring crate doesn't support listing, so this is a placeholder
/// that would need to be backed by a local index.
pub fn list_credential_names() -> Vec<String> {
    // The keyring crate doesn't support enumeration.
    // Credential names should be tracked in UserProfile or a local index file.
    Vec::new()
}
