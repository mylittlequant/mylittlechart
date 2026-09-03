//! Device-code sign-in — replaces the loopback OAuth flow, which cannot work
//! against this provider configuration (see the header comment in `oauth.rs`).
//!
//! Flow:
//!   1. `create` — `POST /api/auth/device-code` mints a short code.
//!   2. `authorize_url` — the browser opens
//!      `{UPDATE_SERVER}/api/oauth/{provider}/authorize?device_code={code}`,
//!      which runs the ordinary WEB OAuth flow (registered redirect_uri, so
//!      the provider accepts it) and the server's own `web-callback` confirms
//!      the device code once the user finishes signing in there.
//!   3. `poll` — `GET /api/auth/poll?code=…`, called on an interval, until the
//!      code is confirmed (returns a token, once only) or expires.
//!
//! Mirrors `entitlements.rs`/`redeem.rs` in shape: thin request/parse
//! functions, no scheduling — the poll interval and its deadline live in the
//! caller (`lib.rs`).
//!
//! The device code and the resulting token are never logged by this module —
//! only outcomes are (see `lib.rs`'s handling of `PollOutcome`).

use crate::token_store::StoredToken;
use crate::UPDATE_SERVER;
use std::time::{SystemTime, UNIX_EPOCH};

/// Request body for `POST /api/auth/device-code`
/// (`mylittlechart-server/crates/server/src/models/types.rs:256-260`):
/// `{"device_id": "..."}`.
#[derive(serde::Serialize)]
struct DeviceCodeRequestBody<'a> {
    device_id: &'a str,
}

/// Server's response shape for `POST /api/auth/device-code`
/// (`mylittlechart-server/crates/server/src/models/types.rs:262-267`):
/// `{"code": "...", "expires_in": 600}`. Only `code` is needed here — the
/// poll deadline is this crate's own (`lib.rs`'s `DEVICE_AUTH_DEADLINE`),
/// not the server's `expires_in`; the unused field is simply not declared,
/// and serde ignores it on deserialize.
#[derive(serde::Deserialize)]
struct DeviceCodeResponseBody {
    code: String,
}

/// Mint a device code for `device_id` and return it.
///
/// The code is a short human-readable string the caller must not log —
/// callers pass it straight into [`authorize_url`] and [`poll`].
pub async fn create(client: &reqwest::Client, device_id: &str) -> Result<String, String> {
    let url = format!("{}/api/auth/device-code", UPDATE_SERVER);

    let resp = client
        .post(&url)
        .json(&DeviceCodeRequestBody { device_id })
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Server returned {}", resp.status()));
    }

    resp.json::<DeviceCodeResponseBody>()
        .await
        .map(|r| r.code)
        .map_err(|e| format!("Parse error: {}", e))
}

/// The browser URL for `provider`'s device-code sign-in.
///
/// Deliberately carries `device_code` and nothing else — no `redirect_port`.
/// A non-zero `redirect_port` is what makes the server ask the provider to
/// redirect to `http://127.0.0.1:<port>/callback`, a URI no provider app has
/// registered; see `oauth.rs`'s header comment for the failure this avoids.
pub fn authorize_url(provider: &str, code: &str) -> String {
    format!("{}/api/oauth/{}/authorize?device_code={}", UPDATE_SERVER, provider, code)
}

/// Outcome of a single `GET /api/auth/poll` call.
pub enum PollOutcome {
    /// The code exists but no web login has confirmed it yet.
    Pending,
    /// The code was confirmed; the server issued the bearer token (once
    /// only — a repeat poll on the same code answers `Expired`).
    Confirmed(StoredToken),
    /// The code is unknown, expired, or already claimed.
    Expired,
}

/// Server's success body shape for `GET /api/auth/poll?code=…`
/// (`mylittlechart-server/crates/server/src/routes/device_auth.rs:249-263`):
/// `{"status": "confirmed", "token": "...", "display_name": "...",
///   "provider": "...", "user_id": 123, "public_id": "..."}`. Pending is
/// `202 {"status": "pending"}`; expired/unknown/claimed is
/// `410 {"status": "expired"}` — this crate matches on the HTTP status
/// rather than the body's `status` field, so only the fields this crate's
/// `StoredToken` actually carries are declared here (`public_id` is
/// intentionally not — see `token_store::StoredToken`'s own field list).
#[derive(serde::Deserialize)]
struct ConfirmedBody {
    token: String,
    display_name: String,
    provider: String,
    user_id: i64,
}

/// Poll the state of `code` once.
pub async fn poll(client: &reqwest::Client, code: &str) -> Result<PollOutcome, String> {
    let url = format!("{}/api/auth/poll?code={}", UPDATE_SERVER, code);

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    match resp.status() {
        reqwest::StatusCode::ACCEPTED => Ok(PollOutcome::Pending),
        reqwest::StatusCode::GONE => Ok(PollOutcome::Expired),
        reqwest::StatusCode::OK => {
            let body: ConfirmedBody = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            Ok(PollOutcome::Confirmed(StoredToken {
                token: body.token,
                provider: body.provider,
                display_name: body.display_name,
                saved_at: now,
                user_id: body.user_id,
            }))
        }
        other => Err(format!("Server returned {}", other)),
    }
}
