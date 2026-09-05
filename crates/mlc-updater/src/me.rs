//! Fetches the caller's own account info via `GET /api/auth/me`.
//!
//! Mirrors `nick.rs`'s shape: a thin request/parse function, with the
//! publish-and-log decision left to the caller in `lib.rs`
//! (`refresh_account_label`). `auth_probe.rs` hits this same endpoint but
//! only reads the status code (`Verdict`), discarding the body — this
//! module wants the payload.

use crate::UPDATE_SERVER;

/// Server's own error body shape on a non-success response.
#[derive(serde::Deserialize)]
struct ErrorBody {
    error: String,
}

/// Server success body — only the fields this client uses. `display_name`
/// is the OAuth-provider name; `effective_label` (nick -> display_name ->
/// public_id, server-computed) is the label everyone else sees once a nick
/// is set. `#[serde(default)]` so an older server that has not shipped this
/// field yet still parses — the caller falls back to `display_name`.
#[derive(serde::Deserialize)]
pub struct MeResponse {
    pub display_name: String,
    #[serde(default)]
    pub effective_label: Option<String>,
}

/// `GET /api/auth/me` for the caller identified by `auth_header` (the full
/// `Authorization` header value, e.g. `"Bearer <token>"`).
pub async fn fetch(client: &reqwest::Client, auth_header: &str) -> Result<MeResponse, String> {
    let url = format!("{}/api/auth/me", UPDATE_SERVER);

    let resp = client
        .get(&url)
        .header("Authorization", auth_header)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        if let Ok(body) = resp.json::<ErrorBody>().await {
            return Err(body.error);
        }
        return Err(format!("Server returned {}", status));
    }

    resp.json::<MeResponse>()
        .await
        .map_err(|e| format!("Parse error: {}", e))
}
