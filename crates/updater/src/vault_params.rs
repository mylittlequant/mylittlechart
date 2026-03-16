//! Vault parameter server sync — HTTP helpers for uploading and fetching
//! vault parameters (salt, encrypted_master_key) to/from the server.
//!
//! These parameters are stored server-side so that vault recovery is possible
//! on another device.  The server stores the salt so the master key can be
//! re-derived from the passphrase, and optionally stores an
//! `encrypted_master_key` blob (master key wrapped with the user's recovery
//! key) so the master key can be recovered without the passphrase.
//!
//! CloudSync items are NOT encrypted client-side; this module's HTTP helpers
//! exist solely for vault recovery purposes.

use zengeld_chart::crypto as crypto;

use crate::state::BuildAttestation;

/// Number of PBKDF2 iterations — re-exported for callers that need it.
pub const PBKDF2_ITERATIONS: u32 = crypto::PBKDF2_ITERATIONS;

// =============================================================================
// Parameter types
// =============================================================================

/// Parameters to register with the server when setting up vault recovery.
///
/// The server stores these so that the user's key can be re-derived on the
/// next login without asking for additional information beyond the passphrase.
pub struct VaultSetupParams {
    /// Hex-encoded 16-byte random salt.
    pub salt: String,
    /// Number of PBKDF2 iterations used during key derivation.
    pub iterations: i32,
    /// Optional AES-256-GCM blob containing the master key encrypted with the
    /// user's recovery key (base64-encoded for JSON transport).
    ///
    /// When present, the server stores it in `sync_e2e_params.encrypted_master_key`
    /// so that the user can recover their master key if they forget their passphrase.
    /// The server never decrypts or inspects this blob.
    pub encrypted_master_key: Option<Vec<u8>>,
}

// =============================================================================
// HTTP helpers
// =============================================================================

/// Fetch the vault parameters stored on the server for the authenticated user.
///
/// Returns `Some((salt_hex, iterations))` if vault params have been uploaded,
/// or `None` if the user has not yet set up vault recovery on the server.
pub async fn get_vault_params(
    client: &reqwest::Client,
    server_url: &str,
    token: &str,
    profile_id: &str,
    device_id: &str,
) -> Result<Option<(String, i32)>, String> {
    let resp = client
        .get(format!("{}/api/sync/e2e-params", server_url))
        .bearer_auth(token)
        .header("X-Profile-Id", profile_id)
        .header("X-Device-Id", device_id)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("e2e-params request: {}", e))?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    if !resp.status().is_success() {
        return Err(format!("e2e-params: HTTP {}", resp.status()));
    }

    #[derive(serde::Deserialize)]
    struct VaultParamsResponse {
        salt: String,
        iterations: i32,
    }

    let data: VaultParamsResponse = resp
        .json()
        .await
        .map_err(|e| format!("e2e-params parse: {}", e))?;

    Ok(Some((data.salt, data.iterations)))
}

/// Upload vault parameters to the server.
///
/// Call this once after vault creation.  The server stores the salt and
/// iteration count so the key can be re-derived from the passphrase on the
/// next session without any additional input.
///
/// Pass `encrypted_master_key` (a raw AES-GCM blob) to also store the
/// recovery-key-wrapped master key on the server.  The server stores this as
/// an opaque blob and never inspects its contents.  Pass `None` to skip.
///
/// The server must treat the stored parameters as immutable — changing them
/// would invalidate all existing encrypted blobs.
pub async fn upload_vault_params(
    client: &reqwest::Client,
    server_url: &str,
    token: &str,
    salt: &str,
    iterations: i32,
    encrypted_master_key: Option<&[u8]>,
    build_attest: &BuildAttestation,
    profile_id: &str,
    device_id: &str,
) -> Result<(), String> {
    #[derive(serde::Serialize)]
    struct VaultSetupRequest<'a> {
        salt: &'a str,
        iterations: i32,
        #[serde(skip_serializing_if = "Option::is_none")]
        encrypted_master_key: Option<String>,
    }

    use base64::Engine as _;
    let emk_b64 = encrypted_master_key
        .map(|b| base64::engine::general_purpose::STANDARD.encode(b));

    let builder = client
        .post(format!("{}/api/sync/e2e-setup", server_url))
        .bearer_auth(token)
        .header("X-Profile-Id", profile_id)
        .header("X-Device-Id", device_id)
        .json(&VaultSetupRequest {
            salt,
            iterations,
            encrypted_master_key: emk_b64,
        })
        .timeout(std::time::Duration::from_secs(10));

    let builder = crate::attest::with_attestation(builder, build_attest);

    let resp = builder
        .send()
        .await
        .map_err(|e| format!("e2e-setup request: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("e2e-setup: HTTP {}", resp.status()));
    }

    Ok(())
}
