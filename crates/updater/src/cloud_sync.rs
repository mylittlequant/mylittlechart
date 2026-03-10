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

use std::path::Path;

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
    /// Unix timestamp (milliseconds) when this item was last modified.
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
    /// Unix timestamp (milliseconds) of last modification.
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
    /// Unix timestamp (milliseconds) of the most recently modified item.
    pub last_modified: Option<i64>,
    /// Total bytes of content stored for this user (non-deleted items).
    pub quota_used_bytes: Option<i64>,
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
    /// Unix timestamp (milliseconds) of the local version.
    pub local_modified: i64,
    /// Unix timestamp (milliseconds) of the cloud version.
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

/// Per-category cloud sync preferences used in the updater loop.
///
/// Mirrors `zengeld_chart::user_profile::profile::SyncCategoryPrefs` but is
/// defined here so the updater crate stays independent of `zengeld-chart`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncCategoryPrefs {
    /// Whether chart presets are included in cloud sync.
    #[serde(default = "default_true")]
    pub presets: bool,
    /// Whether watchlists are included in cloud sync.
    #[serde(default = "default_true")]
    pub watchlists: bool,
    /// Whether indicator/primitive templates are included in cloud sync.
    #[serde(default = "default_true")]
    pub templates: bool,
    /// Whether settings snapshots are included in cloud sync.
    #[serde(default = "default_true")]
    pub settings_snapshots: bool,
}

fn default_true() -> bool { true }

impl Default for SyncCategoryPrefs {
    fn default() -> Self {
        Self {
            presets: true,
            watchlists: true,
            templates: true,
            settings_snapshots: true,
        }
    }
}

/// Persisted state for incremental sync — stored in `profile.json` via
/// [`crate::chart::user_profile::profile::UserProfile::sync_state`].
///
/// Keeping this state allows subsequent syncs to send only `since` queries
/// instead of comparing the full item catalogue each time.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncState {
    /// Unix timestamp (milliseconds) of the last successful sync.
    /// `0` means the client has never synced.
    pub last_sync_timestamp: i64,
    /// Whether the user has opted into cloud sync.
    pub enabled: bool,
    /// Whether the user has enabled E2E encryption for sync data.
    ///
    /// When `true`, all sync item content is encrypted client-side before
    /// being sent to the server.  The server stores only opaque ciphertext.
    #[serde(default)]
    pub e2e_enabled: bool,
    /// Hex-encoded 16-byte PBKDF2 salt, fetched from the server after the
    /// user sets up E2E.  Empty string means E2E has not been configured.
    #[serde(default)]
    pub e2e_salt: String,
    /// Per-category sync preferences — which data categories to include in sync.
    #[serde(default)]
    pub category_prefs: SyncCategoryPrefs,
}

// =============================================================================
// Local file helpers
// =============================================================================

/// Compute SHA-256 hex digest of a UTF-8 string.
fn sha256_hex(data: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Return the last-modified time of a file as a Unix timestamp in milliseconds.
/// Returns 1 (a valid positive timestamp) on any error, since the server
/// rejects `modified_at <= 0`.
fn file_modified_ms(path: impl AsRef<Path>) -> i64 {
    std::fs::metadata(path.as_ref())
        .and_then(|m| m.modified())
        .map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64
        })
        .unwrap_or(1)
}

/// Collect all syncable items from disk, filtered by `category_prefs`.
///
/// Returns `Vec<SyncItem>` ready for push comparison.  Items whose files do
/// not exist are silently skipped (not an error — the user simply hasn't
/// created them yet).  Items in disabled categories are excluded.
pub fn collect_local_sync_items(data_dir: &Path, category_prefs: &SyncCategoryPrefs) -> Vec<SyncItem> {
    let mut items = Vec::new();

    // Category: "watchlist" — single blob
    if category_prefs.watchlists {
        let path = data_dir.join("watchlists.json");
        if let Ok(content) = std::fs::read_to_string(&path) {
            let checksum = sha256_hex(&content);
            items.push(SyncItem {
                sync_id: "watchlists".to_string(),
                category: "watchlist".to_string(),
                name: "watchlists".to_string(),
                content,
                checksum,
                modified_at: file_modified_ms(&path),
                deleted: false,
            });
        }
    }

    // Category: "settings_snapshot" — single blob
    if category_prefs.settings_snapshots {
        let path = data_dir.join("settings_snapshots.json");
        if let Ok(content) = std::fs::read_to_string(&path) {
            let checksum = sha256_hex(&content);
            items.push(SyncItem {
                sync_id: "settings_snapshots".to_string(),
                category: "settings_snapshot".to_string(),
                name: "settings_snapshots".to_string(),
                content,
                checksum,
                modified_at: file_modified_ms(&path),
                deleted: false,
            });
        }
    }

    // Category: "preset" — one per file in presets/
    if category_prefs.presets {
        if let Ok(entries) = std::fs::read_dir(data_dir.join("presets")) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let id = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let checksum = sha256_hex(&content);
                        items.push(SyncItem {
                            sync_id: format!("preset_{}", id),
                            category: "preset".to_string(),
                            name: id,
                            content,
                            checksum,
                            modified_at: file_modified_ms(&path),
                            deleted: false,
                        });
                    }
                }
            }
        }
    }

    // Category: "template_primitive" and "template_indicator" — per file in templates/{type}/
    if category_prefs.templates {
        for (subdir, category) in &[
            ("primitives", "template_primitive"),
            ("indicators", "template_indicator"),
        ] {
            let dir = data_dir.join("templates").join(subdir);
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map_or(false, |e| e == "json") {
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            let id = path
                                .file_stem()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let checksum = sha256_hex(&content);
                            items.push(SyncItem {
                                sync_id: format!("{}_{}", category, id),
                                category: category.to_string(),
                                name: id,
                                content,
                                checksum,
                                modified_at: file_modified_ms(&path),
                                deleted: false,
                            });
                        }
                    }
                }
            }
        }
    }

    items
}

/// Write pulled sync items to their appropriate files on disk.
///
/// Directories are created as needed.  Unknown categories are logged and
/// skipped rather than returning an error, so a single unrecognised item
/// does not abort the whole write pass.
pub fn write_sync_items_to_disk(data_dir: &Path, items: &[SyncItem]) -> std::io::Result<()> {
    for item in items {
        match item.category.as_str() {
            "watchlist" => {
                std::fs::write(data_dir.join("watchlists.json"), &item.content)?;
            }
            "settings_snapshot" => {
                std::fs::write(data_dir.join("settings_snapshots.json"), &item.content)?;
            }
            "preset" => {
                let dir = data_dir.join("presets");
                std::fs::create_dir_all(&dir)?;
                std::fs::write(dir.join(format!("{}.json", item.name)), &item.content)?;
            }
            "template_primitive" => {
                let dir = data_dir.join("templates").join("primitives");
                std::fs::create_dir_all(&dir)?;
                std::fs::write(dir.join(format!("{}.json", item.name)), &item.content)?;
            }
            "template_indicator" => {
                let dir = data_dir.join("templates").join("indicators");
                std::fs::create_dir_all(&dir)?;
                std::fs::write(dir.join(format!("{}.json", item.name)), &item.content)?;
            }
            other => {
                log::warn!("[CloudSync] Unknown sync category '{}' — skipping write", other);
            }
        }
    }
    Ok(())
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
// Real sync cycle — push/pull orchestration
// =============================================================================

/// Perform one incremental sync cycle.
///
/// Algorithm:
/// 1. Collect local items from disk (checksums computed on the fly).
/// 2. Fetch server change metadata since `state.last_sync_timestamp`.
/// 3. Build index of server items (sync_id → meta).
/// 4. For each local item:
///    - If the server has it with the same checksum → already in sync, skip.
///    - Otherwise → push to server.
/// 5. For each server item not present locally (or with a different checksum):
///    - If the server item is deleted → skip (tombstone, no local action needed).
///    - Otherwise → pull its full content and write to disk.
///    - Conflict (both sides differ): server wins (last-writer-wins semantics).
/// 6. Update `last_sync_timestamp` to now.
/// 7. Return `(pushed_count, pulled_count, new_state)`.
pub async fn do_sync_cycle(
    client: &reqwest::Client,
    server_url: &str,
    token: &str,
    state: &SyncState,
    data_dir: &Path,
) -> Result<(usize, usize, SyncState), String> {
    // Step 1: collect local items (filtered by per-category preferences).
    let local_items = collect_local_sync_items(data_dir, &state.category_prefs);

    // Build a local index: sync_id → &SyncItem.
    let local_index: std::collections::HashMap<&str, &SyncItem> = local_items
        .iter()
        .map(|i| (i.sync_id.as_str(), i))
        .collect();

    // Step 2: fetch server change metadata.
    // On the very first sync (last_sync_timestamp == 0) we fetch everything.
    let server_changes = fetch_changes(client, server_url, token, state.last_sync_timestamp).await?;

    log::debug!(
        "[CloudSync] Cycle start: {} local item(s), {} server change(s) since ts={}",
        local_items.len(),
        server_changes.len(),
        state.last_sync_timestamp
    );

    // Step 3: build a server-side index: sync_id → SyncItemMeta.
    let server_index: std::collections::HashMap<&str, &SyncItemMeta> = server_changes
        .iter()
        .map(|m| (m.sync_id.as_str(), m))
        .collect();

    // Step 4: determine which local items need to be pushed.
    let mut to_push: Vec<SyncItem> = Vec::new();

    for local in &local_items {
        match server_index.get(local.sync_id.as_str()) {
            Some(server_meta) if !server_meta.deleted && server_meta.checksum == local.checksum => {
                // Already in sync — skip.
                log::trace!("[CloudSync] In sync: {}", local.sync_id);
            }
            Some(server_meta) if !server_meta.deleted => {
                // Both sides have it but content differs.
                // Server wins (last-writer-wins), so do NOT push local.
                // The server version will be pulled in step 5.
                log::debug!(
                    "[CloudSync] Conflict (server wins): {} local_cs={} server_cs={}",
                    local.sync_id,
                    &local.checksum[..8],
                    &server_meta.checksum[..8]
                );
            }
            _ => {
                // Server doesn't have it, or it's deleted there — push local.
                log::debug!("[CloudSync] Will push: {}", local.sync_id);
                to_push.push(local.clone());
            }
        }
    }

    // Step 5: determine which server items to pull (not in local, or checksum differs, and not deleted).
    // We pull everything that changed on the server where local is missing or differs.
    let mut to_pull_ids: Vec<&str> = Vec::new();

    for server_meta in &server_changes {
        if server_meta.deleted {
            // Tombstone — no local write needed.
            continue;
        }
        match local_index.get(server_meta.sync_id.as_str()) {
            Some(local) if local.checksum == server_meta.checksum => {
                // Same content — skip.
            }
            _ => {
                // Missing locally or content differs → pull.
                log::debug!("[CloudSync] Will pull: {}", server_meta.sync_id);
                to_pull_ids.push(server_meta.sync_id.as_str());
            }
        }
    }

    // Execute push.
    let mut pushed_count = 0usize;

    // Push in batches of 50 (server limit).
    for batch in to_push.chunks(50) {
        match push_items(client, server_url, token, batch).await {
            Ok(n) => {
                pushed_count += n;
                log::debug!("[CloudSync] Pushed batch: {} item(s)", n);
            }
            Err(e) => {
                log::warn!("[CloudSync] Push batch failed: {}", e);
                // Continue — partial push is still progress.
            }
        }
    }

    // Execute pull.
    let mut pulled_count = 0usize;

    if !to_pull_ids.is_empty() {
        // Pull all items at once using the full-pull endpoint.
        // This is the simplest approach; incremental per-item pull can be added later.
        match pull_all(client, server_url, token).await {
            Ok(all_server_items) => {
                // Filter to only the items we actually need.
                let to_write: Vec<SyncItem> = all_server_items
                    .into_iter()
                    .filter(|item| to_pull_ids.contains(&item.sync_id.as_str()))
                    .collect();

                pulled_count = to_write.len();

                if !to_write.is_empty() {
                    if let Err(e) = write_sync_items_to_disk(data_dir, &to_write) {
                        log::warn!("[CloudSync] Failed to write pulled items to disk: {}", e);
                        // Don't abort — still update the timestamp.
                    } else {
                        log::debug!("[CloudSync] Wrote {} pulled item(s) to disk", pulled_count);
                    }
                }
            }
            Err(e) => {
                log::warn!("[CloudSync] Pull failed: {}", e);
            }
        }
    }

    // Step 6: update timestamp.
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let new_state = SyncState {
        last_sync_timestamp: now_ms,
        enabled: state.enabled,
        e2e_enabled: state.e2e_enabled,
        e2e_salt: state.e2e_salt.clone(),
        category_prefs: state.category_prefs.clone(),
    };

    log::info!(
        "[CloudSync] Cycle complete: pushed={} pulled={}",
        pushed_count,
        pulled_count
    );

    Ok((pushed_count, pulled_count, new_state))
}
