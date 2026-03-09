use serde::{Serialize, Deserialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoredToken {
    pub token: String,
    pub provider: String,
    pub display_name: String,
    pub saved_at: u64,
}

/// Get the path for the auth token file.
fn token_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("zengeld").join("auth_token.json"))
}

/// Load stored auth token from disk.
pub fn load_token() -> Option<StoredToken> {
    let path = token_path()?;
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Save auth token to disk.
pub fn save_token(token: &StoredToken) -> Result<(), String> {
    let path = token_path().ok_or("No config directory found")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create config dir: {}", e))?;
    }
    let json = serde_json::to_string_pretty(token).map_err(|e| format!("Serialize error: {}", e))?;
    std::fs::write(&path, json).map_err(|e| format!("Write error: {}", e))?;

    // On Unix, set permissions to owner-only
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        let _ = std::fs::set_permissions(&path, perms);
    }

    Ok(())
}

/// Delete stored auth token.
pub fn clear_token() {
    if let Some(path) = token_path() {
        let _ = std::fs::remove_file(&path);
    }
}
