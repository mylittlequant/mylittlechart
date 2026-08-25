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
