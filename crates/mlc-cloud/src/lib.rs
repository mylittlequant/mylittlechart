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
pub mod staff;

/// The one refusal [`forum_v2_error_message`]/[`dm_error_message`] special-
/// case rather than surfacing verbatim: a signed-in caller with no
/// nickname yet (owner mandate 2026-09-09 — `code: "nick_required"` on
/// `POST /api/forum/v2/*` thread/reply creation and `POST /dm/
/// conversations`/`POST /dm/conversations/{id}/messages`). Both functions
/// return this exact string instead of the server's own sentence so the
/// caller can tell it apart with a plain `==` and re-drive the "choose a
/// nick" prompt (`ForumV2State::nick_required`, `mlc-app-layout`) instead
/// of showing raw error text. Same VALUE as `mlc_updater::forum_v2::
/// NICK_REQUIRED_SENTINEL` — the native and wasm builds never share a
/// dependency for this constant, only its string value.
pub const NICK_REQUIRED_SENTINEL: &str = "__mlc_nick_required__";

#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(target_arch = "wasm32")]
pub use wasm::{FetchTransport, FriendRoster};

pub use models::*;
pub use staff::{
    staff_error, with_retry_after, StaffBatchRevoked, StaffError, StaffGiftBatch, StaffLedgerRow,
    StaffLifetimeGrant, StaffLifetimeRequest, StaffLifetimeRevokeRequest, StaffLifetimeRevoked,
    StaffMintRequest, StaffMintResponse, StaffPaidDays, StaffPaidDaysRequest, StaffUser,
};

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

/// Whether a 401 from an authenticated-only endpoint should demote the
/// caller's session to signed-out (drop the toast, clear the token, etc.).
///
/// `Unauthorized` means "this credential is not valid" — on a caller
/// holding no credential at all that fact is not a demotion, it is the
/// expected answer to a request that should not have gone out signed-out
/// in the first place. Every "sign out on 401" call site MUST hold this
/// invariant: a session that was never signed in must never see the
/// sign-out toast.
///
/// Owner repro 2026-09-24: `mlc-app-shell-web`'s Forum & Messages modal
/// fired the caller-identity fetch (`/api/forum/me`) unconditionally on
/// open, even for a never-signed-in visitor; the resulting 401 flipped the
/// session to "signed out" and produced a "Signed out — The server no
/// longer accepts this session — sign in again." toast on a session that
/// was never signed in. The stray request is fixed at its call site
/// (gated on `is_signed_in()`); this predicate is the second, independent
/// guard every `mark_unauthorized`-style call site applies before trusting
/// a 401 at all — see `mlc-app-shell-web::cloud_session::mark_unauthorized`,
/// this predicate's caller.
pub fn is_session_revoked_401(had_token: bool, is_unauthorized: bool) -> bool {
    had_token && is_unauthorized
}

#[cfg(test)]
mod session_revoked_401_tests {
    use super::is_session_revoked_401;

    #[test]
    fn a_401_with_no_held_token_is_never_a_revocation() {
        // The exact shape of the owner repro: a never-signed-in visitor's
        // stray authenticated-only request comes back 401.
        assert!(!is_session_revoked_401(false, true));
    }

    #[test]
    fn a_401_with_a_held_token_is_a_revocation() {
        // The intended case this whole mechanism exists for: a
        // previously-valid token got rejected server-side.
        assert!(is_session_revoked_401(true, true));
    }

    #[test]
    fn no_held_token_and_no_401_is_never_a_revocation() {
        assert!(!is_session_revoked_401(false, false));
    }

    #[test]
    fn a_held_token_with_a_non_401_error_is_never_a_revocation() {
        // A transport failure / 403 / decode error on a signed-in session
        // must not be treated as a revocation either — only `Unauthorized`
        // (401) is.
        assert!(!is_session_revoked_401(true, false));
    }
}

#[cfg(test)]
mod stream_open_wire_tests {
    use super::models::ShareStreamOpen;

    #[test]
    fn decodes_the_hub_answer() {
        let body = r#"{"ticket":"abc123","publish":true,"expires_in_s":30}"#;
        let open: ShareStreamOpen = serde_json::from_str(body).expect("hub answer decodes");
        assert_eq!(open.ticket, "abc123");
        assert!(open.publish);
        assert_eq!(open.expires_in_s, 30);
    }

    #[test]
    fn a_viewer_answer_and_an_old_edge_answer_both_decode() {
        let viewer: ShareStreamOpen =
            serde_json::from_str(r#"{"ticket":"t","publish":false,"expires_in_s":30}"#).expect("viewer answer");
        assert!(!viewer.publish);
        // The edge fields are gone from the contract; a stray one is ignored.
        let old: ShareStreamOpen =
            serde_json::from_str(r#"{"ticket":"t","publish":true,"owner":"utah","origin":"x","edge":"ams"}"#)
                .expect("unknown fields are ignored");
        assert_eq!(old.ticket, "t");
        assert_eq!(old.expires_in_s, 0);
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

/// The sentence to show someone whose forum-v2 action (board thread create,
/// reply, delete, moderate) was refused. Lives here for the same reason
/// [`redeem_error_message`] does — see that function's own doc. Unlike v1's
/// [`forum_error_message`], 403 reads the server's own body when present:
/// a v2 board's write-class refusal and a moderate action's role refusal
/// carry different sentences, not one fixed "requires a paid account" (a
/// v2 board may not even be paid-gated at all). 404 covers an unknown
/// board/thread/reply, and 429 prefers the server's `retry_after_secs` the
/// same way [`forum_error_message`] does.
pub fn forum_v2_error_message(e: &CloudError) -> String {
    #[derive(serde::Deserialize)]
    struct ErrorBody {
        error: String,
        #[serde(default)]
        code: Option<String>,
    }
    #[derive(serde::Deserialize, Default)]
    struct RateLimitBody {
        #[serde(default)]
        error: String,
        #[serde(default)]
        retry_after_secs: u64,
    }
    match e {
        CloudError::Forbidden(body) => match serde_json::from_str::<ErrorBody>(body) {
            Ok(b) if b.code.as_deref() == Some("nick_required") => NICK_REQUIRED_SENTINEL.to_string(),
            Ok(b) => b.error,
            Err(_) => "You don't have write access to this board".to_string(),
        },
        CloudError::Http { status: 400, body } => serde_json::from_str::<ErrorBody>(body)
            .ok()
            .map(|b| b.error)
            .unwrap_or_else(|| "That isn't valid".to_string()),
        CloudError::Http { status: 404, .. } => "Not found".to_string(),
        CloudError::Http { status: 429, body } => match serde_json::from_str::<RateLimitBody>(body) {
            Ok(b) if b.retry_after_secs > 0 => format!("Try again in {}s", b.retry_after_secs),
            Ok(b) if !b.error.is_empty() => b.error,
            _ => "Too many requests — try again shortly".to_string(),
        },
        CloudError::Unauthorized => "Sign in again, then retry".to_string(),
        other => other.to_string(),
    }
}

/// The sentence to show someone whose direct-message action (publish
/// identity, open a conversation, send a message, mark read) was refused.
/// Lives here for the same reason [`redeem_error_message`] does — see that
/// function's own doc. 403 covers two distinct refusals the server words
/// itself (the caller is not paid, or the named peer is not paid) — this
/// reads the server's own body when present rather than picking one fixed
/// sentence for both. 413 is the ciphertext-too-large cap
/// (`dm_db::DM_MAX_CIPHERTEXT_BYTES`); 429 is the per-sender daily send
/// limit, worded from `retry_after_secs` when the server sends one.
pub fn dm_error_message(e: &CloudError) -> String {
    #[derive(serde::Deserialize)]
    struct ErrorBody {
        error: String,
        #[serde(default)]
        code: Option<String>,
    }
    #[derive(serde::Deserialize, Default)]
    struct RateLimitBody {
        #[serde(default)]
        error: String,
        #[serde(default)]
        retry_after_secs: u64,
    }
    match e {
        CloudError::Forbidden(body) => match serde_json::from_str::<ErrorBody>(body) {
            Ok(b) if b.code.as_deref() == Some("nick_required") => NICK_REQUIRED_SENTINEL.to_string(),
            Ok(b) => b.error,
            Err(_) => "Direct messages require a paid account".to_string(),
        },
        CloudError::Http { status: 400, body } => serde_json::from_str::<ErrorBody>(body)
            .ok()
            .map(|b| b.error)
            .unwrap_or_else(|| "That isn't valid".to_string()),
        CloudError::Http { status: 404, .. } => "Not found".to_string(),
        CloudError::Http { status: 413, body } => serde_json::from_str::<ErrorBody>(body)
            .ok()
            .map(|b| b.error)
            .unwrap_or_else(|| "Message is too large".to_string()),
        CloudError::Http { status: 429, body } => match serde_json::from_str::<RateLimitBody>(body) {
            Ok(b) if b.retry_after_secs > 0 => format!("Try again in {}s", b.retry_after_secs),
            Ok(b) if !b.error.is_empty() => b.error,
            _ => "Too many messages today — try again tomorrow".to_string(),
        },
        CloudError::Unauthorized => "Sign in again, then retry".to_string(),
        other => other.to_string(),
    }
}
