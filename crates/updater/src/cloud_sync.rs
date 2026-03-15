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

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::state::{BuildAttestation, SyncConflict};

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
    /// The server now returns `accepted`; `synced` is kept as an alias
    /// for backwards compatibility with older server deployments.
    #[serde(alias = "synced")]
    pub accepted: usize,
    /// Items the server rejected (e.g. validation failures, quota exceeded).
    /// Empty on success; populated when the server rejects specific items.
    #[serde(default)]
    pub rejected: Vec<serde_json::Value>,
}

// =============================================================================
// Sync cycle result
// =============================================================================

/// Outcome of a completed sync cycle.
#[derive(Debug, Clone)]
pub struct SyncCycleResult {
    /// Number of local items pushed to the server.
    pub pushed: usize,
    /// Number of server items pulled and written to disk.
    pub pulled: usize,
    /// Updated sync state (new timestamp, etc.).
    pub new_state: SyncState,
    /// Items that could not be auto-resolved — both local and server copies
    /// differ from the last successfully synced checksum.  These items are
    /// **not** pushed or pulled; the caller must surface them to the user.
    pub conflicts: Vec<SyncConflict>,
}

// =============================================================================
// Incremental sync state
// =============================================================================

fn default_true() -> bool { true }

/// Persisted state for incremental sync — stored in `profile.json` via
/// [`crate::chart::user_profile::profile::UserProfile::sync_state`].
///
/// Keeping this state allows subsequent syncs to send only `since` queries
/// instead of comparing the full item catalogue each time.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Whether the encrypted vault file (`vault.enc`) is included in cloud sync.
    /// Defaults to `true`.
    #[serde(default = "default_true")]
    pub sync_vault: bool,
    /// Whether chart presets are included in cloud sync.
    /// Defaults to `true`.
    #[serde(default = "default_true")]
    pub sync_presets: bool,
    /// Whether indicator and primitive templates are included in cloud sync.
    /// Defaults to `true`.
    #[serde(default = "default_true")]
    pub sync_templates: bool,
    /// Whether watchlists are included in cloud sync.
    /// Defaults to `true`.
    #[serde(default = "default_true")]
    pub sync_watchlists: bool,
    /// Whether the active theme is included in cloud sync.
    /// Defaults to `true`.
    #[serde(default = "default_true")]
    pub sync_theme: bool,
    /// Whether the recovery key (encrypted master key) is included in cloud sync.
    /// Defaults to `true`.
    #[serde(default = "default_true")]
    pub sync_recovery_key: bool,
    /// Checksums of items as they were after the last successful push/pull.
    ///
    /// Key: `sync_id`  (e.g. `"preset_my_chart"`).
    /// Value: SHA-256 hex of the item content at the time it was last
    /// successfully synced to *or* from the server.
    ///
    /// Used for true conflict detection: an item is a conflict if *both*
    /// the local checksum *and* the server checksum differ from this value —
    /// meaning both sides have been independently modified since the last sync.
    #[serde(default)]
    pub last_synced_checksums: std::collections::HashMap<String, String>,
    /// Set of `sync_id` strings that have been successfully pushed to the server
    /// at least once during this session.
    ///
    /// Used for tombstone detection: on each cycle, items present in this set
    /// but absent from the current local item list are treated as locally
    /// deleted.  A tombstone (`deleted: true`, `content: ""`) is pushed for
    /// each such item so the server marks it as deleted.
    ///
    /// This set is seeded from the persisted profile on startup and updated
    /// after each successful push.
    #[serde(default)]
    pub synced_items: std::collections::HashSet<String>,
}

impl Default for SyncState {
    fn default() -> Self {
        Self {
            last_sync_timestamp: 0,
            enabled: false,
            e2e_enabled: false,
            e2e_salt: String::new(),
            sync_vault: true,
            sync_presets: true,
            sync_templates: true,
            sync_watchlists: true,
            sync_theme: true,
            sync_recovery_key: true,
            last_synced_checksums: std::collections::HashMap::new(),
            synced_items: std::collections::HashSet::new(),
        }
    }
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

/// Collect all syncable items from disk.
///
/// Returns `Vec<SyncItem>` ready for push comparison.  Items whose files do
/// not exist are silently skipped (not an error — the user simply hasn't
/// created them yet).
///
/// All categories are always collected.  The caller is responsible for
/// filtering out items by category (e.g. filtering `"vault"` when
/// `sync_vault` is `false`).
pub fn collect_local_sync_items(data_dir: &Path) -> Vec<SyncItem> {
    eprintln!("[CloudSync] collect_local_sync_items: data_dir={:?}", data_dir);
    let mut items = Vec::new();

    // Category: "watchlist" — single blob
    {
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
    {
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
    if let Ok(entries) = std::fs::read_dir(data_dir.join("presets")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "json") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    // Strip heavy fields before sync: bars (re-fetchable from exchange),
                    // command_history and stashed_command_history (local undo/redo only).
                    let content = strip_preset_for_sync(&content);
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

    // Category: "template_primitive" and "template_indicator" — per file in templates/{type}/
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

    // Category: "theme" — single value stored in theme.json
    {
        let path = data_dir.join("theme.json");
        if let Ok(content) = std::fs::read_to_string(&path) {
            let checksum = sha256_hex(&content);
            items.push(SyncItem {
                sync_id: "theme".to_string(),
                category: "theme".to_string(),
                name: "active_theme".to_string(),
                content,
                checksum,
                modified_at: file_modified_ms(&path),
                deleted: false,
            });
        }
    }

    // Category: "vault" — encrypted vault blob (vault.enc), base64-encoded as content.
    {
        let vault_path = data_dir.join("vault.enc");
        if vault_path.exists() {
            if let Ok(bytes) = std::fs::read(&vault_path) {
                let content = base64::engine::general_purpose::STANDARD.encode(&bytes);
                let checksum = sha256_hex(&content);
                let modified_at = file_modified_ms(&vault_path);
                items.push(SyncItem {
                    sync_id: "vault".to_string(),
                    category: "vault".to_string(),
                    name: "vault".to_string(),
                    content,
                    checksum,
                    modified_at,
                    deleted: false,
                });
            }
        }
    }

    eprintln!("[CloudSync] collected {} local items", items.len());
    items
}

/// Create a timestamped backup of `path` in a `.sync_backups` subdirectory,
/// then prune old backups so only the most recent `keep` copies are retained.
///
/// Does nothing if the file does not yet exist.
fn backup_file(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let parent = path.parent().unwrap_or(Path::new("."));
    let backup_dir = parent.join(".sync_backups");
    std::fs::create_dir_all(&backup_dir)?;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let backup_name = format!("{}_{}", timestamp, filename);
    std::fs::copy(path, backup_dir.join(&backup_name))?;

    // Keep only the last 5 backups per original file name.
    cleanup_old_backups(&backup_dir, &filename, 5)?;

    Ok(())
}

/// Remove the oldest backups in `backup_dir` that end with `original_name`,
/// keeping at most `keep` copies.
fn cleanup_old_backups(backup_dir: &Path, original_name: &str, keep: usize) -> std::io::Result<()> {
    let mut backups: Vec<_> = std::fs::read_dir(backup_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name().to_string_lossy().ends_with(original_name)
        })
        .collect();

    // Lexicographic sort is chronological because entries are prefixed with
    // a Unix timestamp (e.g. "1741699200_watchlists.json").
    backups.sort_by_key(|e| e.file_name());

    if backups.len() > keep {
        for entry in &backups[..backups.len() - keep] {
            let _ = std::fs::remove_file(entry.path());
        }
    }

    Ok(())
}

/// Write pulled sync items to their appropriate files on disk.
///
/// A timestamped backup is created in `.sync_backups/` before overwriting any
/// file that already exists (last 5 backups per file are kept).  Directories
/// are created as needed.  Unknown categories are logged and skipped rather
/// than returning an error, so a single unrecognised item does not abort the
/// whole write pass.
///
/// Each item's content is verified against its `checksum` field before writing.
/// Items whose checksum does not match are skipped with a warning — this
/// prevents corrupted server data from being persisted locally.
///
/// Writes are performed atomically via a `.tmp` side-file that is renamed into
/// place after a successful `write`, so a crash mid-write cannot produce a
/// half-written file.
pub fn write_sync_items_to_disk(data_dir: &Path, items: &[SyncItem]) -> std::io::Result<()> {
    for item in items {
        // N10: Verify checksum before writing — skip corrupted items.
        let actual_checksum = sha256_hex(&item.content);
        if actual_checksum != item.checksum {
            log::warn!(
                "[CloudSync] Checksum mismatch for '{}' (category='{}') — expected {} got {} — skipping write",
                item.sync_id,
                item.category,
                item.checksum,
                actual_checksum
            );
            continue;
        }

        match item.category.as_str() {
            "watchlist" => {
                let target = data_dir.join("watchlists.json");
                backup_file(&target)?;
                let tmp = target.with_extension("tmp");
                std::fs::write(&tmp, &item.content)?;
                std::fs::rename(&tmp, &target)?;
            }
            "settings_snapshot" => {
                let target = data_dir.join("settings_snapshots.json");
                backup_file(&target)?;
                let tmp = target.with_extension("tmp");
                std::fs::write(&tmp, &item.content)?;
                std::fs::rename(&tmp, &target)?;
            }
            "preset" => {
                let dir = data_dir.join("presets");
                std::fs::create_dir_all(&dir)?;
                let target = dir.join(format!("{}.json", item.name));
                backup_file(&target)?;
                let tmp = target.with_extension("tmp");
                std::fs::write(&tmp, &item.content)?;
                std::fs::rename(&tmp, &target)?;
            }
            "template_primitive" => {
                let dir = data_dir.join("templates").join("primitives");
                std::fs::create_dir_all(&dir)?;
                let target = dir.join(format!("{}.json", item.name));
                backup_file(&target)?;
                let tmp = target.with_extension("tmp");
                std::fs::write(&tmp, &item.content)?;
                std::fs::rename(&tmp, &target)?;
            }
            "template_indicator" => {
                let dir = data_dir.join("templates").join("indicators");
                std::fs::create_dir_all(&dir)?;
                let target = dir.join(format!("{}.json", item.name));
                backup_file(&target)?;
                let tmp = target.with_extension("tmp");
                std::fs::write(&tmp, &item.content)?;
                std::fs::rename(&tmp, &target)?;
            }
            "theme" => {
                let target = data_dir.join("theme.json");
                backup_file(&target)?;
                let tmp = target.with_extension("tmp");
                std::fs::write(&tmp, &item.content)?;
                std::fs::rename(&tmp, &target)?;
            }
            "vault" => {
                // vault.enc is stored as base64-encoded raw bytes on the server.
                // Decode back to raw bytes before writing to disk.
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(&item.content)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("vault base64 decode: {}", e)))?;
                let target = data_dir.join("vault.enc");
                backup_file(&target)?;
                let tmp = target.with_extension("tmp");
                std::fs::write(&tmp, &bytes)?;
                std::fs::rename(&tmp, &target)?;
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
    build_attest: &BuildAttestation,
    profile_id: &str,
    device_id: &str,
) -> Result<SyncStatusResponse, String> {
    let builder = client
        .get(format!("{}/api/sync/status", server_url))
        .bearer_auth(token)
        .header("X-Profile-Id", profile_id)
        .header("X-Device-Id", device_id)
        .timeout(std::time::Duration::from_secs(10));

    let builder = crate::attest::with_attestation(builder, build_attest);

    let resp = builder
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
///
/// Returns `(items, server_timestamp)` where `server_timestamp` is the
/// authoritative server-side timestamp for this response (milliseconds).
/// Falls back to `0` if the server does not include a timestamp field.
pub async fn fetch_changes(
    client: &reqwest::Client,
    server_url: &str,
    token: &str,
    since: i64,
    build_attest: &BuildAttestation,
    profile_id: &str,
    device_id: &str,
) -> Result<(Vec<SyncItemMeta>, i64), String> {
    let builder = client
        .get(format!("{}/api/sync/changes?since={}", server_url, since))
        .bearer_auth(token)
        .header("X-Profile-Id", profile_id)
        .header("X-Device-Id", device_id)
        .timeout(std::time::Duration::from_secs(15));

    let builder = crate::attest::with_attestation(builder, build_attest);

    let resp = builder
        .send()
        .await
        .map_err(|e| format!("sync changes request: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("sync changes: HTTP {}", resp.status()));
    }

    #[derive(Deserialize)]
    struct Resp {
        items: Vec<SyncItemMeta>,
        #[serde(default)]
        server_timestamp: i64,
    }

    let data: Resp = resp
        .json()
        .await
        .map_err(|e| format!("sync changes parse: {}", e))?;

    Ok((data.items, data.server_timestamp))
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
    build_attest: &BuildAttestation,
    profile_id: &str,
    device_id: &str,
) -> Result<usize, String> {
    #[derive(Serialize)]
    struct Req<'a> {
        items: &'a [SyncItem],
    }

    let builder = client
        .post(format!("{}/api/sync/push", server_url))
        .bearer_auth(token)
        .header("X-Profile-Id", profile_id)
        .header("X-Device-Id", device_id)
        .json(&Req { items })
        .timeout(std::time::Duration::from_secs(30));

    let builder = crate::attest::with_attestation(builder, build_attest);

    let resp = builder
        .send()
        .await
        .map_err(|e| { eprintln!("[CloudSync] push_items send error: {}", e); format!("sync push request: {}", e) })?;

    let status = resp.status();
    let body = resp.text().await.map_err(|e| format!("sync push read body: {}", e))?;
    eprintln!("[CloudSync] push_items response: status={} body_len={} body={}", status, body.len(), &body[..500.min(body.len())]);

    if !status.is_success() {
        return Err(format!("sync push: HTTP {} body={}", status, &body[..500.min(body.len())]));
    }

    let data: PushResponse = serde_json::from_str(&body).map_err(|e| format!("sync push parse: {}", e))?;

    if !data.rejected.is_empty() {
        eprintln!(
            "[CloudSync] Push: {} item(s) rejected by server: {:?}",
            data.rejected.len(),
            data.rejected
        );
    }

    Ok(data.accepted)
}

/// Pull every item for this user from the server (full download).
///
/// Use for the initial sync when `last_sync_timestamp == 0` or when a
/// full refresh is required.  For incremental updates use [`fetch_changes`].
pub async fn pull_all(
    client: &reqwest::Client,
    server_url: &str,
    token: &str,
    build_attest: &BuildAttestation,
    profile_id: &str,
    device_id: &str,
) -> Result<Vec<SyncItem>, String> {
    let builder = client
        .get(format!("{}/api/sync/pull", server_url))
        .bearer_auth(token)
        .header("X-Profile-Id", profile_id)
        .header("X-Device-Id", device_id)
        .timeout(std::time::Duration::from_secs(30));

    let builder = crate::attest::with_attestation(builder, build_attest);

    let resp = builder
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
/// 3. Build indices for local and server items.
/// 4. For each item that exists on both sides with differing checksums:
///    - Compare both checksums against the last-synced checkpoint stored in
///      `state.last_synced_checksums`.
///    - If only the server side changed → pull (safe auto-merge).
///    - If only the local side changed → push (safe auto-merge).
///    - If **both** sides changed → true conflict; added to the conflicts vec,
///      not pushed or pulled.  Caller surfaces it to the user.
/// 5. Items only on one side are pushed/pulled normally.
/// 6. Update `last_sync_timestamp` and `last_synced_checksums` for all
///    successfully synced items.
/// 7. Return `SyncCycleResult` which includes the conflicts vec.
pub async fn do_sync_cycle(
    client: &reqwest::Client,
    server_url: &str,
    token: &str,
    state: &SyncState,
    data_dir: &Path,
    build_attest: &BuildAttestation,
    e2e_key: Option<[u8; 32]>,
    profile_id: &str,
    device_id: &str,
) -> Result<SyncCycleResult, String> {
    // Guard: never sync without E2E key — all blobs must be encrypted.
    if e2e_key.is_none() {
        eprintln!("[CloudSync] e2e_key not set — skipping sync cycle (waiting for vault unlock)");
        return Ok(SyncCycleResult {
            pushed: 0,
            pulled: 0,
            new_state: state.clone(),
            conflicts: vec![],
        });
    }

    // Step 1: collect all local items, then apply per-category filters.
    let local_items = {
        let mut all = collect_local_sync_items(data_dir);
        if !state.sync_vault {
            all.retain(|item| item.category != "vault");
        }
        if !state.sync_presets {
            all.retain(|item| item.category != "preset");
        }
        if !state.sync_templates {
            all.retain(|item| item.category != "template_indicator" && item.category != "template_primitive");
        }
        if !state.sync_watchlists {
            all.retain(|item| item.category != "watchlist");
        }
        if !state.sync_theme {
            all.retain(|item| item.category != "theme");
        }
        // sync_recovery_key is managed at the E2E-setup level and is not
        // represented as a distinct sync category; no filter needed here.
        all
    };

    // Build a local index: sync_id → &SyncItem.
    let local_index: std::collections::HashMap<&str, &SyncItem> = local_items
        .iter()
        .map(|i| (i.sync_id.as_str(), i))
        .collect();

    // Step 1b: tombstone detection — items previously pushed but no longer on disk.
    //
    // For each sync_id in `state.synced_items` that is absent from the current
    // local index, we synthesise a tombstone SyncItem (`deleted: true`, `content: ""`).
    // The server validates checksum == SHA-256(content), so we compute that
    // for the empty string.  These tombstones are prepended to the push list.
    let tombstone_checksum = sha256_hex("");
    let now_for_tombstones = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    // Use 1 if clock is broken (server rejects modified_at <= 0).
    let now_for_tombstones = if now_for_tombstones > 0 { now_for_tombstones } else { 1 };

    let mut tombstones: Vec<SyncItem> = state
        .synced_items
        .iter()
        .filter(|id| !local_index.contains_key(id.as_str()))
        .map(|id| {
            // Derive name and category from the sync_id so the server can
            // identify and tombstone the correct row.  sync_id format examples:
            //   "preset_my_chart"            → category="preset", name="my_chart"
            //   "template_indicator_rsi"     → category="template_indicator", name="rsi"
            //   "template_primitive_fib"     → category="template_primitive", name="fib"
            //   "watchlists"                 → category="watchlist", name="watchlists"
            //   "settings_snapshots"         → category="settings_snapshot", name="settings_snapshots"
            let (category, name) = if id == "watchlists" {
                ("watchlist".to_string(), "watchlists".to_string())
            } else if id == "settings_snapshots" {
                ("settings_snapshot".to_string(), "settings_snapshots".to_string())
            } else if id == "theme" {
                ("theme".to_string(), "active_theme".to_string())
            } else if let Some(rest) = id.strip_prefix("template_indicator_") {
                ("template_indicator".to_string(), rest.to_string())
            } else if let Some(rest) = id.strip_prefix("template_primitive_") {
                ("template_primitive".to_string(), rest.to_string())
            } else if let Some(rest) = id.strip_prefix("preset_") {
                ("preset".to_string(), rest.to_string())
            } else {
                // Unknown format — use sync_id as both category and name; server
                // will reject with an unknown-category error which is logged below.
                (id.clone(), id.clone())
            };
            SyncItem {
                sync_id: id.clone(),
                category,
                name,
                content: String::new(),
                checksum: tombstone_checksum.clone(),
                modified_at: now_for_tombstones,
                deleted: true,
            }
        })
        .collect();

    if !tombstones.is_empty() {
        log::debug!(
            "[CloudSync] {} tombstone(s) detected for locally deleted items: {:?}",
            tombstones.len(),
            tombstones.iter().map(|t| t.sync_id.as_str()).collect::<Vec<_>>()
        );
    }

    // Step 2: fetch server change metadata.
    // On the very first sync (last_sync_timestamp == 0) we fetch everything.
    let (server_changes, server_timestamp) = fetch_changes(client, server_url, token, state.last_sync_timestamp, build_attest, profile_id, device_id).await?;

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

    // Step 4: Last-write-wins push-only — client is always source of truth.
    // Push any local item whose checksum differs from what the server has.
    // No pull, no conflict detection — server is just backup storage.
    let mut to_push: Vec<SyncItem> = Vec::new();
    let conflicts: Vec<SyncConflict> = Vec::new(); // Always empty — no conflicts in LWW model.

    for local in &local_items {
        match server_index.get(local.sync_id.as_str()) {
            Some(server_meta) if !server_meta.deleted && server_meta.checksum == local.checksum => {
                // Identical on both sides — already in sync, skip.
                log::trace!("[CloudSync] In sync: {}", local.sync_id);
            }
            _ => {
                // Different or missing on server — push local (last-write-wins).
                log::debug!("[CloudSync] Will push (LWW): {}", local.sync_id);
                to_push.push(local.clone());
            }
        }
    }

    // Append tombstones for locally deleted items to the push list.
    // Tombstones have `deleted: true` and empty content; they are never
    // E2E-encrypted because there is no content to protect.
    to_push.append(&mut tombstones);

    // Execute push.
    let mut pushed_count = 0usize;
    // Track which sync_ids were successfully pushed so we can update checksum state.
    let mut pushed_ids: Vec<String> = Vec::new();

    // If E2E is active, produce an encrypted copy of every item to push.
    // The original `to_push` holds plaintext — we build a parallel `encrypted_push`
    // that replaces `content` with `base64(nonce || ciphertext)`.  The checksum
    // stored on the server covers the *ciphertext*, which is fine — it's only used
    // for change-detection, not integrity verification (the GCM tag handles that).
    // Tombstones (`deleted: true`) are passed through as-is: their content is empty
    // and their checksum is already SHA-256(""), so no encryption is needed.
    //
    // Item names are ALWAYS plaintext metadata — never encrypted.
    let items_to_push: Vec<SyncItem> = if let Some(ref key) = e2e_key {
        let mut enc_items = Vec::with_capacity(to_push.len());
        for item in &to_push {
            if item.deleted {
                // Pass tombstones through without encryption.
                enc_items.push(item.clone());
                continue;
            }
            match crate::e2e_crypto::encrypt(key, item.content.as_bytes()) {
                Ok(ciphertext) => {
                    let encoded = base64::engine::general_purpose::STANDARD.encode(&ciphertext);
                    let enc_checksum = sha256_hex(&encoded);

                    enc_items.push(SyncItem {
                        sync_id: item.sync_id.clone(),
                        category: item.category.clone(),
                        name: item.name.clone(),
                        content: encoded,
                        checksum: enc_checksum,
                        modified_at: item.modified_at,
                        deleted: item.deleted,
                    });
                }
                Err(e) => {
                    log::warn!("[CloudSync] E2E encrypt failed for {}: {} — skipping item", item.sync_id, e);
                }
            }
        }
        enc_items
    } else {
        to_push.clone()
    };

    eprintln!("[CloudSync] LWW push-only: to_push={} e2e_key={}",
        to_push.len(),
        if e2e_key.is_some() { "set" } else { "none" });
    for item in &to_push {
        eprintln!("[CloudSync]   push: id={} cat={} bytes={}", item.sync_id, item.category, item.content.len());
    }

    // Push in batches of 50 (server limit).
    for batch in items_to_push.chunks(50) {
        eprintln!("[CloudSync] pushing batch of {} items to {}/api/sync/push", batch.len(), server_url);
        match push_items(client, server_url, token, batch, build_attest, profile_id, device_id).await {
            Ok(n) => {
                eprintln!("[CloudSync] push OK: accepted={}", n);
                pushed_count += n;
                // Record the sync_ids from this batch as successfully pushed.
                for item in batch {
                    pushed_ids.push(item.sync_id.clone());
                }
            }
            Err(e) => {
                eprintln!("[CloudSync] push FAILED: {}", e);
                // Continue — partial push is still progress.
            }
        }
    }

    // No automatic pull — client is source of truth. Pull only on explicit
    // user request ("Restore from Backup"). Keep counters for logging.
    let pulled_count = 0usize;
    let written_items: Vec<SyncItem> = Vec::new();

    // Step 6: update timestamp, checksum state, and synced_items tracking.
    //
    // Only update `last_synced_checksums` for items that were *actually*
    // successfully synced (pushed or pulled).  Conflicted items are left
    // untouched so the next cycle still sees them as conflicts.
    //
    // Use the server's authoritative timestamp when available (> 0); fall back
    // to local clock only when the server did not supply one (older server build).
    let sync_timestamp = if server_timestamp > 0 {
        server_timestamp
    } else {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    };

    let mut new_checksums = state.last_synced_checksums.clone();

    // Update checksums for pushed items (use plaintext checksum — it is what
    // was on disk; the server stored the encrypted variant but we compare locally).
    for item in &to_push {
        if pushed_ids.contains(&item.sync_id) {
            if !item.deleted {
                new_checksums.insert(item.sync_id.clone(), item.checksum.clone());
            } else {
                // Tombstone was accepted — remove from checksum tracking too.
                new_checksums.remove(&item.sync_id);
            }
        }
    }

    // Update checksums for successfully written pulled items.
    for item in &written_items {
        // For pulled items, compute the checksum of the plaintext we wrote to disk.
        let cs = sha256_hex(&item.content);
        new_checksums.insert(item.sync_id.clone(), cs);
    }

    // Rebuild synced_items: start from the previous set, apply changes from
    // this cycle so future cycles can detect newly deleted items.
    //
    // Rules:
    // - Successfully pushed non-tombstone items → add to set (now known to server).
    // - Successfully pushed tombstone items → remove from set (deleted on server).
    // - Successfully pulled items → add to set (server has them; also now local).
    let mut new_synced_items = state.synced_items.clone();
    for item in &to_push {
        if pushed_ids.contains(&item.sync_id) {
            if item.deleted {
                new_synced_items.remove(&item.sync_id);
            } else {
                new_synced_items.insert(item.sync_id.clone());
            }
        }
    }
    for item in &written_items {
        new_synced_items.insert(item.sync_id.clone());
    }

    let new_state = SyncState {
        last_sync_timestamp: sync_timestamp,
        enabled: state.enabled,
        e2e_enabled: state.e2e_enabled,
        e2e_salt: state.e2e_salt.clone(),
        sync_vault: state.sync_vault,
        sync_presets: state.sync_presets,
        sync_templates: state.sync_templates,
        sync_watchlists: state.sync_watchlists,
        sync_theme: state.sync_theme,
        sync_recovery_key: state.sync_recovery_key,
        last_synced_checksums: new_checksums,
        synced_items: new_synced_items,
    };

    log::info!(
        "[CloudSync] Cycle complete: pushed={} pulled={} conflicts={}",
        pushed_count,
        pulled_count,
        conflicts.len()
    );

    Ok(SyncCycleResult {
        pushed: pushed_count,
        pulled: pulled_count,
        new_state,
        conflicts,
    })
}

// =============================================================================
// Preset stripping helpers
// =============================================================================

/// Strip heavy/device-specific fields from a preset JSON before syncing.
///
/// Removes `bars` from each window snapshot and compare overlay series —
/// bar data is re-fetchable from exchanges and inflates presets to 400KB+.
/// Removes `ViewportChange` commands from `command_history` (undo/redo stacks)
/// — pan/zoom state is device-specific and ephemeral; it should not cross devices.
fn strip_preset_for_sync(raw_json: &str) -> String {
    let Ok(mut val) = serde_json::from_str::<serde_json::Value>(raw_json) else {
        return raw_json.to_string();
    };

    if let Some(windows) = val.get_mut("windows").and_then(|w| w.as_array_mut()) {
        for win in windows.iter_mut() {
            if let Some(obj) = win.as_object_mut() {
                obj.remove("bars");

                // Strip ViewportChange commands from command_history.
                // Viewport (pan/zoom) is device-specific and ephemeral — keep
                // locally for undo/redo but don't sync between devices.
                if let Some(history) = obj.get_mut("command_history") {
                    for stack_name in &["undo_stack", "redo_stack"] {
                        if let Some(stack) = history
                            .get_mut(*stack_name)
                            .and_then(|s| s.as_array_mut())
                        {
                            stack.retain(|cmd| cmd.get("ViewportChange").is_none());
                        }
                    }
                }

                // Strip bars from compare overlay series too.
                if let Some(co) = obj.get_mut("compare_overlay") {
                    if let Some(series) = co.get_mut("series").and_then(|s| s.as_array_mut()) {
                        for s in series.iter_mut() {
                            if let Some(so) = s.as_object_mut() {
                                so.remove("bars");
                            }
                        }
                    }
                }
            }
        }
    }

    // Also handle sync_groups which contain SyncGroupSnapshot with windows.
    if let Some(groups) = val.get_mut("sync_groups").and_then(|g| g.as_array_mut()) {
        for group in groups.iter_mut() {
            if let Some(windows) = group.get_mut("windows").and_then(|w| w.as_array_mut()) {
                for win in windows.iter_mut() {
                    if let Some(obj) = win.as_object_mut() {
                        obj.remove("bars");

                        // Strip ViewportChange commands from command_history.
                        // Viewport (pan/zoom) is device-specific and ephemeral — keep
                        // locally for undo/redo but don't sync between devices.
                        if let Some(history) = obj.get_mut("command_history") {
                            for stack_name in &["undo_stack", "redo_stack"] {
                                if let Some(stack) = history
                                    .get_mut(*stack_name)
                                    .and_then(|s| s.as_array_mut())
                                {
                                    stack.retain(|cmd| cmd.get("ViewportChange").is_none());
                                }
                            }
                        }

                        if let Some(co) = obj.get_mut("compare_overlay") {
                            if let Some(series) =
                                co.get_mut("series").and_then(|s| s.as_array_mut())
                            {
                                for s in series.iter_mut() {
                                    if let Some(so) = s.as_object_mut() {
                                        so.remove("bars");
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    serde_json::to_string(&val).unwrap_or_else(|_| raw_json.to_string())
}
