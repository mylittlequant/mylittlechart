//! API key hash synchronization from chart.example.
//!
//! In Connected mode the updater periodically fetches the set of key hashes
//! registered for the authenticated user on the server.  These are merged with
//! locally-generated keys in the main thread via a watch channel.
//!
//! Only hashes (SHA-256 hex digests) are ever transmitted — raw key material
//! never leaves the client.

use reqwest::Client;
use crate::UPDATE_SERVER;

// ===========================================================================
// Response types
// ===========================================================================

/// Top-level response from `GET /api/auth/keys?format=sync`.
#[derive(serde::Deserialize, Debug)]
pub struct KeySyncResponse {
    pub keys: Vec<SyncedKeyEntry>,
}

/// One key entry as returned by the server sync endpoint.
///
/// Contains only the hash and metadata — the raw key is never sent.
#[derive(serde::Deserialize, Debug, Clone)]
pub struct SyncedKeyEntry {
    /// SHA-256 hex digest of the raw key.
    pub token_hash: String,
    /// Permission tier string: `"read_only"`, `"read_write"`, or `"admin"`.
    pub tier: String,
    /// Human-readable label assigned by the user on the server.
    pub label: String,
}

// ===========================================================================
// Fetch
// ===========================================================================

/// Fetch key hashes from the server for the authenticated user.
///
/// Uses the existing shared `Client` so no extra connections are opened.
/// Returns an error string on any network or parse failure; callers should
/// log the error and continue — local keys are never affected by a failed sync.
pub async fn fetch_key_hashes(
    client: &Client,
    auth_token: &str,
) -> Result<Vec<SyncedKeyEntry>, String> {
    let url = format!("{}/api/auth/keys?format=sync", UPDATE_SERVER);

    let resp = client
        .get(&url)
        .bearer_auth(auth_token)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("key sync request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("key sync returned {}", resp.status()));
    }

    let data: KeySyncResponse = resp
        .json()
        .await
        .map_err(|e| format!("key sync parse error: {}", e))?;

    Ok(data.keys)
}
