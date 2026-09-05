//! `mlc-cloud` — client for the MLC server API (`/api/sync/*`, `/api/auth/*`).
//!
//! This crate is the OSS-safe half of the server integration: serde DTOs
//! ([`models`]) plus a [`CloudClient`] transport. It contains **no secrets**
//! and no server implementation — the server lives in the private
//! `mylittlechart-server` tree, and the build-attestation signing key is a
//! CI secret injected at packaging time (see [`BuildAttestation`]).
//!
//! # Transport
//!
//! - **wasm32**: fetch-based, compiled from `web-sys` (see `wasm` module).
//!   Used by `mlc-app-shell-web` behind its `cloud` feature.
//! - **native**: not implemented here yet — the native (reqwest) transport
//!   returns with updater phase 2. Models and config compile natively so
//!   `cargo check --workspace` stays green, mirroring the `mlc-vault` /
//!   `mlc-app-shell-web` target-gating pattern.
//!
//! # Auth model
//!
//! Bearer token from the device-code flow (`POST /api/auth/device-code` →
//! user confirms in a logged-in browser → `GET /api/auth/poll`), persisted
//! locally as [`models::StoredToken`]. Every request also carries
//! `X-Profile-Id` / `X-Device-Id`; sync endpoints additionally enforce the
//! build attestation headers when the server has `BUILD_ATTEST_ENFORCE=true`.

pub mod models;

#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(target_arch = "wasm32")]
pub use wasm::FetchTransport;

pub use models::*;

/// Build attestation values, produced at packaging time.
///
/// The canonical message `BUILD_ATTESTATION_V1:{version}:{timestamp}:{platform}`
/// is Ed25519-signed with the release signing key **during the build** (CI
/// secret); the client only attaches the resulting four static headers.
/// The private key must never ship in the bundle or this repository.
#[derive(Debug, Clone)]
pub struct BuildAttestation {
    /// Base64-encoded 64-byte Ed25519 signature of the canonical message.
    pub signature_b64: String,
    pub version: String,
    pub timestamp: u64,
    pub platform: String,
}

/// Connection parameters for a [`CloudClient`].
#[derive(Debug, Clone)]
pub struct CloudConfig {
    /// Base URL of the server, e.g. `https://chart.example`.
    /// Empty string means same-origin (the production wasm deployment).
    pub server_url: String,
    /// Bearer token from the device-code flow; `None` = anonymous
    /// (only the auth endpoints are callable).
    pub token: Option<String>,
    /// `X-Profile-Id` header — selects the sync namespace on the server.
    pub profile_id: String,
    /// `X-Device-Id` header — identifies this browser/device.
    pub device_id: String,
    /// Build attestation headers; required once the server enforces them.
    pub attestation: Option<BuildAttestation>,
}

/// Errors from the cloud transport.
#[derive(Debug)]
pub enum CloudError {
    /// Network-level failure (DNS, offline, CORS rejection, JS exception).
    Transport(String),
    /// The server answered with a non-success status we do not map
    /// semantically; `body` carries the server's error payload if any.
    Http { status: u16, body: String },
    /// The response body did not match the expected schema.
    Decode(String),
    /// 401 — token missing or revoked.
    Unauthorized,
    /// 403 — build attestation rejected (or otherwise forbidden).
    Forbidden(String),
}

impl CloudError {
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, CloudError::Unauthorized)
    }
}

impl std::fmt::Display for CloudError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloudError::Transport(e) => write!(f, "transport: {}", e),
            CloudError::Http { status, body } => write!(f, "HTTP {}: {}", status, body),
            CloudError::Decode(e) => write!(f, "decode: {}", e),
            CloudError::Unauthorized => write!(f, "unauthorized"),
            CloudError::Forbidden(e) => write!(f, "forbidden: {}", e),
        }
    }
}

impl std::error::Error for CloudError {}

/// The sentence to show someone who just submitted a gift code that did not
/// work.
///
/// Lives here rather than in the shell because both the wording and the status
/// mapping are one fact about one endpoint, and `CloudError`'s own `Display`
/// is a developer's line ("HTTP 409: …"), not a reader's. The desktop shell
/// has its own copy of this mapping in `mlc-updater` — it does not depend on
/// this crate at all (native transport lives there) — so the two are written
/// to say the same thing.
pub fn redeem_error_message(e: &CloudError) -> String {
    match e {
        CloudError::Http { status: 404, .. } => "No such code".to_string(),
        CloudError::Http { status: 409, .. } => "This code has already been used".to_string(),
        CloudError::Unauthorized => "Sign in again, then retry".to_string(),
        other => other.to_string(),
    }
}

/// The sentence to show someone whose nickname submission was refused.
/// Lives here for the same reason [`redeem_error_message`] does — see that
/// function's own doc.
pub fn nick_error_message(e: &CloudError) -> String {
    #[derive(serde::Deserialize)]
    struct ErrorBody {
        error: String,
    }
    #[derive(serde::Deserialize, Default)]
    struct RateLimitBody {
        #[serde(default)]
        error: String,
        #[serde(default)]
        next_allowed_at: String,
    }
    fn server_text(body: &str) -> Option<String> {
        serde_json::from_str::<ErrorBody>(body).ok().map(|b| b.error)
    }
    match e {
        CloudError::Http { status: 400, body } => {
            server_text(body).unwrap_or_else(|| "That nickname isn't valid".to_string())
        }
        CloudError::Http { status: 409, body } => {
            server_text(body).unwrap_or_else(|| "That nickname is already taken".to_string())
        }
        CloudError::Http { status: 429, body } => match serde_json::from_str::<RateLimitBody>(body) {
            Ok(b) if !b.next_allowed_at.is_empty() => format!(
                "Too many nickname changes — try again after {}",
                b.next_allowed_at
            ),
            Ok(b) if !b.error.is_empty() => b.error,
            _ => "Too many nickname changes — try again later".to_string(),
        },
        CloudError::Unauthorized => "Sign in again, then retry".to_string(),
        other => other.to_string(),
    }
}

/// The sentence to show someone whose forum action (thread create, reply,
/// delete) was refused. Lives here for the same reason
/// [`redeem_error_message`] does — see that function's own doc. 403 is
/// stated outright as "not entitled" (signed in but not paid, or the
/// feature is gated off) rather than passed through the raw HTTP
/// description; 429 prefers the server's `retry_after_secs` over its own
/// sentence when both are present.
pub fn forum_error_message(e: &CloudError) -> String {
    #[derive(serde::Deserialize)]
    struct ErrorBody {
        error: String,
    }
    #[derive(serde::Deserialize, Default)]
    struct RateLimitBody {
        #[serde(default)]
        error: String,
        #[serde(default)]
        retry_after_secs: u64,
    }
    match e {
        CloudError::Forbidden(_) => "Forum access requires a paid account".to_string(),
        CloudError::Http { status: 400, body } => serde_json::from_str::<ErrorBody>(body)
            .ok()
            .map(|b| b.error)
            .unwrap_or_else(|| "That isn't valid".to_string()),
        CloudError::Http { status: 429, body } => match serde_json::from_str::<RateLimitBody>(body) {
            Ok(b) if b.retry_after_secs > 0 => format!("Try again in {}s", b.retry_after_secs),
            Ok(b) if !b.error.is_empty() => b.error,
            _ => "Too many requests — try again shortly".to_string(),
        },
        CloudError::Unauthorized => "Sign in again, then retry".to_string(),
        other => other.to_string(),
    }
}
