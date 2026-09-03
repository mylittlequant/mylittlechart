//! Redeems a gift/trial code via `POST /api/account/redeem`.
//!
//! Mirrors `entitlements.rs`'s shape: a thin request/parse function, with
//! the publish-and-log decision left to the caller in `lib.rs`.

use crate::UPDATE_SERVER;

/// Server's own error body shape on a non-success response, e.g.
/// `{"error": "No such code"}`.
#[derive(serde::Deserialize)]
struct ErrorBody {
    error: String,
}

/// Server's success body shape: `{"paid_until": "<rfc3339>"}`.
#[derive(serde::Deserialize)]
struct RedeemResponse {
    paid_until: String,
}

/// Redeem `code` for the caller identified by `auth_header` and return the
/// new paid-until timestamp (RFC3339) on success.
///
/// `auth_header` is the full `Authorization` header value (e.g. `"Bearer
/// <token>"`) — the caller must already hold a token; this function makes
/// no attempt to resolve one itself.
///
/// The submitted code is never logged, here or by any caller of this
/// function — only outcomes are.
pub async fn redeem(client: &reqwest::Client, auth_header: &str, code: &str) -> Result<String, String> {
    let url = format!("{}/api/account/redeem", UPDATE_SERVER);

    let resp = client
        .post(&url)
        .header("Authorization", auth_header)
        .json(&serde_json::json!({ "code": code }))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        // Prefer the server's own error message when the body carries one;
        // fall back to a status-shaped message otherwise.
        if let Ok(body) = resp.json::<ErrorBody>().await {
            return Err(body.error);
        }
        if status.as_u16() == 401 {
            return Err("Sign in again to redeem this code".to_string());
        }
        return Err(format!("Server returned {}", status));
    }

    resp.json::<RedeemResponse>()
        .await
        .map(|r| r.paid_until)
        .map_err(|e| format!("Parse error: {}", e))
}
