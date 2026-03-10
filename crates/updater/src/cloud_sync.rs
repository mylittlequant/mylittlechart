//! Cloud sync HTTP module — push/pull sync items to/from chart.example.
//!
//! This module is **HTTP-only**.  It never reads or writes local files.
//! The calling code (main.rs or the updater loop) is responsible for:
//! - Collecting local data and computing checksums before calling [`push_items`].
//! - Writing pulled items to disk after [`pull_all`] or processing
//!   [`fetch_changes`] results.
//!
//! All operations are best-effort: errors are returned as `String` so the
//! caller can log and continue without disrupting normal app operation.

use serde::{Deserialize, Serialize};

// =============================================================================
// Sync item types
// =============================================================================

/// Lightweight metadata for a single sync item — used in change-detection
/// lists to avoid transferring full content on every tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncItemMeta {
    /// Stable identifier for this item (e.g. `"preset_1728503941_123456789"`).
    pub sync_id: String,
    /// Category of the item: `"preset"`, `"template"`, `"watchlist"`, etc.
    pub category: String,
    /// Human-readable name (e.g. preset name, watchlist label).
    pub name: String,
    /// SHA-256 hex digest of the serialized content — used for conflict
    /// detection without transferring the full payload.
    pub checksum: String,
    /// Unix timestamp (seconds) when this item was last modified.
    pub modified_at: i64,
    /// Whether this item has been soft-deleted on the server.
    #[serde(default)]
    pub deleted: bool,
}

/// A sync item with its full serialized content.
///
/// Used for both push (client → server) and pull (server → client) operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncItem {
    /// Stable identifier (must match the local item ID).
    pub sync_id: String,
    /// Category: `"preset"`, `"template"`, `"watchlist"`, etc.
    pub category: String,
    /// Human-readable name.
    pub name: String,
    /// JSON-serialized content of the item.
    pub content: String,
    /// SHA-256 hex digest of `content`.
    pub checksum: String,
    /// Unix timestamp (seconds) of last modification.
    pub modified_at: i64,
    /// Whether this item has been soft-deleted.
    #[serde(default)]
    pub deleted: bool,
}

// =============================================================================
// Server response shapes
// =============================================================================

/// Response body from `GET /api/sync/status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatusResponse {
    /// True if the server has at least one sync item for this user.
    pub has_cloud_data: bool,
    /// Total number of non-deleted items stored for this user.
    pub item_count: i64,
    /// Unix timestamp (seconds) of the most recently modified item.
    pub last_modified: Option<i64>,
}

/// Response body from `POST /api/sync/push`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResponse {
    /// Number of items the server successfully recorded.
    pub synced: usize,
}

// =============================================================================
// Conflict detection
// =============================================================================

/// Describes a conflict between local and cloud versions of an item.
///
/// Both sides were modified since the last successful sync.  Resolution is
/// deferred to the caller (typically presented to the user).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConflict {
    pub sync_id: String,
    pub category: String,
    pub name: String,
    /// Unix timestamp (seconds) of the local version.
    pub local_modified: i64,
    /// Unix timestamp (seconds) of the cloud version.
    pub cloud_modified: i64,
    /// Checksum of the local version.
    pub local_checksum: String,
    /// Checksum of the cloud version.
    pub cloud_checksum: String,
}

// =============================================================================
// Sync action result
// =============================================================================

/// Outcome of a completed sync cycle.
#[derive(Debug, Clone)]
pub enum SyncAction {
    /// No conflicts — sync completed silently.
    Completed { pushed: usize, pulled: usize },
    /// One or more items conflict and need user resolution.
    ConflictsDetected(Vec<SyncConflict>),
    /// A non-fatal error occurred; app continues normally.
    Error(String),
}

// =============================================================================
// Incremental sync state
// =============================================================================

/// Persisted state for incremental sync — stored in `profile.json` via
/// [`crate::chart::user_profile::profile::UserProfile::sync_state`].
///
/// Keeping this state allows subsequent syncs to send only `since` queries
/// instead of comparing the full item catalogue each time.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncState {
    /// Unix timestamp (seconds) of the last successful sync.
    /// `0` means the client has never synced.
    pub last_sync_timestamp: i64,
    /// Whether the user has opted into cloud sync.
    pub enabled: bool,
}

// =============================================================================
// HTTP functions
// =============================================================================

/// Check whether the server has any cloud data for the authenticated user.
///
/// Use this on startup (in Connected mode) to decide whether to prompt the
/// user about initial sync.
pub async fn check_status(
    client: &reqwest::Client,
    server_url: &str,
    token: &str,
) -> Result<SyncStatusResponse, String> {
    let resp = client
        .get(format!("{}/api/sync/status", server_url))
        .bearer_auth(token)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("sync status request: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("sync status: HTTP {}", resp.status()));
    }

    resp.json::<SyncStatusResponse>()
        .await
        .map_err(|e| format!("sync status parse: {}", e))
}

/// Fetch item metadata for everything changed on the server since `since`.
///
/// Pass `since = 0` to receive metadata for all items (full catalogue).
/// The returned list may include items marked `deleted = true`; callers
/// should handle soft-deletes when they write to local storage.
pub async fn fetch_changes(
    client: &reqwest::Client,
    server_url: &str,
    token: &str,
    since: i64,
) -> Result<Vec<SyncItemMeta>, String> {
    let resp = client
        .get(format!("{}/api/sync/changes?since={}", server_url, since))
        .bearer_auth(token)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("sync changes request: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("sync changes: HTTP {}", resp.status()));
    }

    #[derive(Deserialize)]
    struct Resp {
        items: Vec<SyncItemMeta>,
    }

    let data: Resp = resp
        .json()
        .await
        .map_err(|e| format!("sync changes parse: {}", e))?;

    Ok(data.items)
}

/// Push a batch of items to the server.
///
/// Items may be new or updated; the server merges them by `sync_id`.
/// Returns the number of items successfully recorded by the server.
pub async fn push_items(
    client: &reqwest::Client,
    server_url: &str,
    token: &str,
    items: &[SyncItem],
) -> Result<usize, String> {
    #[derive(Serialize)]
    struct Req<'a> {
        items: &'a [SyncItem],
    }

    let resp = client
        .post(format!("{}/api/sync/push", server_url))
        .bearer_auth(token)
        .json(&Req { items })
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("sync push request: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("sync push: HTTP {}", resp.status()));
    }

    let data: PushResponse = resp
        .json()
        .await
        .map_err(|e| format!("sync push parse: {}", e))?;

    Ok(data.synced)
}

/// Pull every item for this user from the server (full download).
///
/// Use for the initial sync when `last_sync_timestamp == 0` or when a
/// full refresh is required.  For incremental updates use [`fetch_changes`].
pub async fn pull_all(
    client: &reqwest::Client,
    server_url: &str,
    token: &str,
) -> Result<Vec<SyncItem>, String> {
    let resp = client
        .get(format!("{}/api/sync/pull", server_url))
        .bearer_auth(token)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("sync pull request: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("sync pull: HTTP {}", resp.status()));
    }

    #[derive(Deserialize)]
    struct Resp {
        items: Vec<SyncItem>,
    }

    let data: Resp = resp
        .json()
        .await
        .map_err(|e| format!("sync pull parse: {}", e))?;

    Ok(data.items)
}

// =============================================================================
// Stub sync cycle — orchestration lives in main.rs
// =============================================================================

/// Perform one incremental sync cycle (stub implementation).
///
/// This function only handles HTTP communication.  It does **not** read or
/// write local files — that happens in main.rs via the existing save/load
/// infrastructure.
///
/// Current behaviour:
/// 1. Fetch change metadata from the server since `state.last_sync_timestamp`.
/// 2. Log what changed.
/// 3. Return `(pushed, pulled, updated_state)`.
///
/// The actual push of local data will be wired from main.rs once the
/// serialization layer is connected.
pub async fn do_sync_cycle(
    client: &reqwest::Client,
    server_url: &str,
    token: &str,
    state: &SyncState,
) -> Result<(usize, usize, SyncState), String> {
    let changes = fetch_changes(client, server_url, token, state.last_sync_timestamp).await?;

    let pulled = changes.len();
    if pulled > 0 {
        log::debug!(
            "[CloudSync] {} item(s) changed on server since timestamp {}",
            pulled,
            state.last_sync_timestamp
        );
        for item in &changes {
            log::trace!(
                "[CloudSync]   {} {:?} {} (deleted={})",
                item.category,
                item.sync_id,
                item.name,
                item.deleted
            );
        }
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let new_state = SyncState {
        last_sync_timestamp: now,
        enabled: state.enabled,
    };

    Ok((0, pulled, new_state))
}
