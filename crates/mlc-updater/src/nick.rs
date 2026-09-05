//! Sets the account nickname via `POST /api/account/nick`.
//!
//! Mirrors `redeem.rs`'s shape: a thin request/parse function, with the
//! publish-and-log decision left to the caller in `lib.rs`.

use crate::UPDATE_SERVER;

/// Server's own error body shape on a non-success response, e.g.
/// `{"error": "That nickname is already taken"}`.
#[derive(serde::Deserialize)]
struct ErrorBody {
    error: String,
}

/// The 429 body additionally carries when the next attempt is allowed.
#[derive(serde::Deserialize, Default)]
struct RateLimitBody {
    #[serde(default)]
    error: String,
    #[serde(default)]
    next_allowed_at: String,
}

/// Server's success body shape: `{"nick": "...", "effective_label": "..."}`.
/// `nick` itself is not modeled — the caller only ever wants the label to
/// display, which `effective_label` already is (server-computed: the nick
/// when one is set, the OAuth display name otherwise).
#[derive(serde::Deserialize)]
struct SetNickResponse {
    effective_label: String,
}

/// Set the account nickname for the caller identified by `auth_header`,
/// returning the new effective label on success.
///
/// `auth_header` is the full `Authorization` header value (e.g. `"Bearer
/// <token>"`) — the caller must already hold a token; this function makes
/// no attempt to resolve one itself.
pub async fn set_nick(client: &reqwest::Client, auth_header: &str, nick: &str) -> Result<String, String> {
    let url = format!("{}/api/account/nick", UPDATE_SERVER);

    let resp = client
        .post(&url)
        .header("Authorization", auth_header)
        .json(&serde_json::json!({ "nick": nick }))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        // 429 carries a machine-readable cooldown instant in addition to a
        // sentence — prefer it when present, same as `mlc_cloud::
        // nick_error_message`'s wasm twin.
        if status.as_u16() == 429 {
            return Err(match resp.json::<RateLimitBody>().await {
                Ok(body) if !body.next_allowed_at.is_empty() => format!(
                    "Too many nickname changes — try again after {}",
                    body.next_allowed_at
                ),
                Ok(body) if !body.error.is_empty() => body.error,
                _ => "Too many nickname changes — try again later".to_string(),
            });
        }
        if let Ok(body) = resp.json::<ErrorBody>().await {
            return Err(body.error);
        }
        if status.as_u16() == 401 {
            return Err("Sign in again to change your nickname".to_string());
        }
        return Err(format!("Server returned {}", status));
    }

    resp.json::<SetNickResponse>()
        .await
        .map(|r| r.effective_label)
        .map_err(|e| format!("Parse error: {}", e))
}
