//! Wire-format models for the MLC server API (`/api/sync/*`, `/api/auth/*`).
//!
//! These mirror the server-side shapes in
//! `mylittlechart-server/crates/server/src/routes/sync.rs`,
//! `.../routes/device_auth.rs` and `.../models/types.rs`, and the pre-strip
//! client models recovered from `mylittlechart-private-stash/updater`
//! (source commit `deedb7f`). The server is private; these DTOs carry no
//! secrets and are safe to keep in the OSS tree.

use serde::{Deserialize, Serialize};

// =============================================================================
// Sync items
// =============================================================================

/// Tier that governs how an item is transported to the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncTier {
    /// Plaintext structured data — no client-side encryption.
    CloudSync,
    /// Already-encrypted binary blob — pushed as-is, no re-encryption.
    ZtBlob,
    /// Device-local only — never sent to the server.
    DeviceLocal,
}

/// Lightweight metadata for a single sync item — used in change-detection
/// lists to avoid transferring full content on every tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncItemMeta {
    pub sync_id: String,
    pub category: String,
    pub name: String,
    pub checksum: String,
    pub modified_at: i64,
    #[serde(default)]
    pub deleted: bool,
}

/// A sync item with its full serialized content.
///
/// `content` is an opaque blob from the server's point of view (plaintext
/// JSON for [`SyncTier::CloudSync`], base64 ciphertext for
/// [`SyncTier::ZtBlob`]). `checksum` must be `SHA-256(content)` as 64
/// lowercase hex chars — the server verifies it before writing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncItem {
    pub sync_id: String,
    pub category: String,
    pub name: String,
    pub content: String,
    pub checksum: String,
    pub modified_at: i64,
    #[serde(default)]
    pub deleted: bool,
}

// =============================================================================
// /api/sync/* request/response shapes
// =============================================================================

/// Request body for `POST /api/sync/push` (max 50 items per request).
#[derive(Debug, Clone, Serialize)]
pub struct PushRequest {
    pub items: Vec<SyncItem>,
}

/// Response body from `POST /api/sync/push`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResponse {
    #[serde(alias = "synced")]
    pub accepted: usize,
    #[serde(default)]
    pub rejected: Vec<serde_json::Value>,
}

/// Response body from `GET /api/sync/changes?since=T`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangesResponse {
    pub items: Vec<SyncItemMeta>,
    pub server_timestamp: i64,
}

/// Response body from `GET /api/sync/pull` (full snapshot).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullResponse {
    pub items: Vec<SyncItem>,
}

/// Response body from `GET /api/sync/status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatusResponse {
    pub has_cloud_data: bool,
    pub item_count: i64,
    pub last_modified: Option<i64>,
    pub quota_used_bytes: Option<i64>,
    #[serde(default)]
    pub quota_limit_bytes: Option<i64>,
}

/// Response body from `DELETE /api/sync/all`
/// (requires the `X-Confirm-Delete: yes` header).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteAllResponse {
    pub deleted: u64,
}

/// Response body from `GET /api/sync/e2e-params`.
///
/// The server returns `{ "enabled": false }` when the user has not set up
/// vault recovery; the remaining fields are absent in that case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct E2EParamsResponse {
    pub enabled: bool,
    #[serde(default)]
    pub salt: Option<String>,
    #[serde(default)]
    pub iterations: Option<i32>,
    #[serde(default)]
    pub encrypted_master_key: Option<String>,
}

/// Vault parameters for an account with E2E recovery set up.
#[derive(Debug, Clone)]
pub struct VaultParams {
    /// Hex-encoded 16-byte random salt used for PBKDF2 key derivation.
    pub salt: String,
    /// Number of PBKDF2 iterations.
    pub iterations: i32,
    /// AES-256-GCM blob: master key encrypted with the user's recovery key
    /// (base64). The server stores it opaquely and never inspects it.
    pub encrypted_master_key: Option<String>,
}

/// Request body for `POST /api/sync/e2e-setup`.
///
/// `salt` must be exactly 32 lowercase hex chars (16 bytes);
/// `iterations` must be >= 100 000.
#[derive(Debug, Clone, Serialize)]
pub struct E2ESetupRequest {
    pub salt: String,
    pub iterations: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_master_key: Option<String>,
}

// =============================================================================
// /api/auth/* shapes (device-code login)
// =============================================================================

/// Request body for `POST /api/auth/device-code`.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceCodeRequest {
    pub device_id: String,
}

/// Response body from `POST /api/auth/device-code`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCodeResponse {
    pub code: String,
    pub expires_in: u64,
}

/// Outcome of `GET /api/auth/poll?code=…`.
///
/// `pending` returns HTTP 202, `expired` HTTP 410, `confirmed` HTTP 200 —
/// the transport maps all three onto this enum (status code + body).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum DeviceAuthPoll {
    Pending,
    Expired,
    Confirmed {
        token: String,
        display_name: String,
        provider: String,
        user_id: i64,
        public_id: String,
    },
}

/// A single linked OAuth provider in a user's profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub provider: String,
    pub display_name: String,
}

/// Response body from `GET /api/auth/me`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeResponse {
    pub user_id: i64,
    /// Non-sequential public identifier — safe to expose in APIs.
    pub public_id: String,
    pub display_name: String,
    /// Primary provider (kept for backwards compatibility).
    pub provider: String,
    /// All providers linked to this account.
    pub providers: Vec<ProviderInfo>,
}

// =============================================================================
// /api/entitlements shape (feature gates)
// =============================================================================

/// Response body from `GET /api/entitlements`.
///
/// Answers for an anonymous caller too — the server treats "no credential"
/// as a supported caller (the free feature set), not a 401. Because of that,
/// `account_signed_in` (whether THIS install actually holds a credential) is
/// never derived from this response — each shell already knows it from its
/// own token store, and reads it separately
/// (`mlc_app_layout::ui::modal_settings::UserSettingsState::
/// account_signed_in`'s own doc has the full reasoning). `expires_at` is a
/// unix-ms timestamp: the caller re-asks once it passes rather than on every
/// frame or every modal open.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitlementsResponse {
    pub features: Vec<String>,
    pub expires_at: i64,
}

impl EntitlementsResponse {
    /// Wire name of the multi-profile feature gate.
    pub const MULTI_PROFILE: &'static str = "multi_profile";

    /// Whether the resolved feature set contains [`Self::MULTI_PROFILE`].
    pub fn has_multi_profile(&self) -> bool {
        self.features.iter().any(|f| f == Self::MULTI_PROFILE)
    }
}

// =============================================================================
// /api/updates/* shapes (anonymous release manifest)
// =============================================================================

/// Response body from `GET /api/updates/latest?platform=…`.
/// Mirrors `VersionManifest` in the server's `models/types.rs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub version: String,
    #[serde(default)]
    pub sha256: String,
    pub download_url: String,
    #[serde(default)]
    pub release_notes: String,
    #[serde(default)]
    pub file_size: u64,
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub channel: String,
}

/// One entry in the release catalogue (`GET /api/updates/list?channel=…`).
/// Mirrors one row of the server's `releases` table — every published
/// platform/version pair, not just the caller's own platform. Drives the
/// Download overlay's expanded "other platforms & versions" list; the
/// single-platform `VersionManifest` above stays what the collapsed view
/// (and the OTA updater) asks for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseEntry {
    pub platform: String,
    pub version: String,
    pub channel: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub file_size: u64,
    #[serde(default)]
    pub release_notes: String,
    pub created_at: String,
    pub download_url: String,
}

/// Response body from `GET /api/updates/list?channel=…`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseListResponse {
    pub releases: Vec<ReleaseEntry>,
}

/// One entry in the blog feed (`GET /blog/posts.json`, static file on the
/// same origin). Drives the News overlay in cloud builds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlogPost {
    pub id: String,
    pub date: String,
    pub title: String,
    pub summary: String,
    /// Full post text; paragraphs separated by blank lines. Older feeds may
    /// lack it — then the overlay falls back to the summary.
    #[serde(default)]
    pub body: String,
    pub url: String,
}

/// The blog feed document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlogFeed {
    pub posts: Vec<BlogPost>,
}

/// Locally persisted auth token (wasm: localStorage; native: config dir).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoredToken {
    pub token: String,
    pub provider: String,
    pub display_name: String,
    pub saved_at: u64,
    /// User ID returned by the server. Defaults to 0 for tokens saved before
    /// this field was added.
    #[serde(default)]
    pub user_id: i64,
    /// Non-sequential public identifier (present for tokens issued after the
    /// server started returning it from the poll endpoint).
    #[serde(default)]
    pub public_id: Option<String>,
}
