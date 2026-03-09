//! Version checking against the update server.

use crate::state::VersionManifest;
use crate::{UPDATE_SERVER, platform};

/// Fetch the latest version manifest from the server.
pub async fn fetch_latest(auth_header: Option<&str>) -> Result<VersionManifest, String> {
    let url = format!("{}/api/updates/latest?platform={}", UPDATE_SERVER, platform::current_platform());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let mut req = client.get(&url);
    if let Some(auth) = auth_header {
        req = req.header("Authorization", auth);
    }

    let resp = req.send().await.map_err(|e| format!("Request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Server returned {}", resp.status()));
    }

    let manifest: VersionManifest = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
    Ok(manifest)
}

/// Check if `server_version` is newer than `current_version` using semver.
pub fn is_newer(current: &str, server: &str) -> bool {
    let current = match semver::Version::parse(current) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let server = match semver::Version::parse(server) {
        Ok(v) => v,
        Err(_) => return false,
    };
    server > current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer() {
        assert!(is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("1.0.0", "1.0.1"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("2.0.0", "1.0.0"));
        assert!(is_newer("0.1.0", "1.0.0"));
    }
}
