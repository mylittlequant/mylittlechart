//! Cloud sync HTTP module — push/pull sync items to/from chart.example.
//!
//! This module handles:
//! - Collecting local profile data and classifying it into sync tiers.
//! - Pushing CloudSync-tier items (structured plaintext, no client-side encryption).
//! - Pushing ZtBlob-tier items (pre-encrypted binary blobs: vault.enc, recovery_key.enc).
//! - Writing server-pulled items back to disk.
//!
//! All operations are best-effort: errors are returned as `String` so the
//! caller can log and continue without disrupting normal app operation.

use std::path::Path;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::state::{BuildAttestation, SyncConflict};

// =============================================================================
// Sync tier classification
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

/// A single item collected from disk, tagged with its sync tier.
#[derive(Debug, Clone)]
pub struct LocalItem {
    pub sync_id: String,
    pub category: String,
    pub name: String,
    pub content: String,
    pub checksum: String,
    pub modified_at: i64,
    pub tier: SyncTier,
}

/// All items collected from a profile directory in a single disk-read pass.
pub struct LocalItems {
    pub items: Vec<LocalItem>,
}

impl LocalItems {
    /// Iterate over items destined for the CloudSync tier.
    pub fn cloud_sync_items(&self) -> impl Iterator<Item = &LocalItem> {
        self.items.iter().filter(|i| i.tier == SyncTier::CloudSync)
    }

    /// Iterate over items destined for the ZtBlob tier (pre-encrypted blobs).
    pub fn zt_blob_items(&self) -> impl Iterator<Item = &LocalItem> {
        self.items.iter().filter(|i| i.tier == SyncTier::ZtBlob)
    }
}

/// Result of a single ZT-blob push pass.
#[derive(Debug, Clone)]
pub struct ZtBlobResult {
    /// Number of blob items successfully accepted by the server.
    pub pushed: usize,
    /// Map of sync_id to checksum for all items that were pushed.
    pub id_to_checksum: std::collections::HashMap<String, String>,
}

// =============================================================================
// Sync item types
// =============================================================================

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
// Server response shapes
// =============================================================================

/// Response body from `GET /api/sync/status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatusResponse {
    pub has_cloud_data: bool,
    pub item_count: i64,
    pub last_modified: Option<i64>,
    pub quota_used_bytes: Option<i64>,
}

/// Response body from `POST /api/sync/push`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResponse {
    #[serde(alias = "synced")]
    pub accepted: usize,
    #[serde(default)]
    pub rejected: Vec<serde_json::Value>,
}

// =============================================================================
// Sync cycle result
// =============================================================================

/// Outcome of a completed sync cycle.
#[derive(Debug, Clone)]
pub struct SyncCycleResult {
    pub pushed: usize,
    pub pulled: usize,
    pub new_state: SyncState,
    pub conflicts: Vec<SyncConflict>,
}

// =============================================================================
// Incremental sync state
// =============================================================================

fn default_true() -> bool {
    true
}

/// Persisted state for incremental sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    pub last_sync_timestamp: i64,
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub sync_vault: bool,
    #[serde(default = "default_true")]
    pub sync_presets: bool,
    #[serde(default = "default_true")]
    pub sync_templates: bool,
    #[serde(default = "default_true")]
    pub sync_watchlists: bool,
    #[serde(default = "default_true")]
    pub sync_theme: bool,
    #[serde(default = "default_true")]
    pub sync_recovery_key: bool,
    #[serde(default)]
    pub last_synced_checksums: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub synced_items: std::collections::HashSet<String>,
}

impl Default for SyncState {
    fn default() -> Self {
        Self {
            last_sync_timestamp: 0,
            enabled: false,
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

fn sha256_hex(data: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    format!("{:x}", hasher.finalize())
}

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

/// Collect all syncable items in a single disk-read pass, tagging each with
/// its [`SyncTier`].
///
/// Preset stripping is deferred to [`do_cloud_sync`].
/// - vault.enc and recovery_key.enc are tagged as [`SyncTier::ZtBlob`].
/// - New categories are included: template_compare, template_indicator_set,
///   salt, and recovery_key.
pub fn collect_local_items(data_dir: &Path) -> LocalItems {
    eprintln!("[{} CloudSync] collect_local_items: data_dir={:?}", crate::now_ts(), data_dir);
    let mut items: Vec<LocalItem> = Vec::new();

    // --- CloudSync tier ---

    {
        let path = data_dir.join("watchlists.json");
        if let Ok(content) = std::fs::read_to_string(&path) {
            let checksum = sha256_hex(&content);
            items.push(LocalItem {
                sync_id: "watchlists".to_string(),
                category: "watchlist".to_string(),
                name: "watchlists".to_string(),
                content,
                checksum,
                modified_at: file_modified_ms(&path),
                tier: SyncTier::CloudSync,
            });
        }
    }

    {
        let path = data_dir.join("settings_snapshots.json");
        if let Ok(content) = std::fs::read_to_string(&path) {
            let checksum = sha256_hex(&content);
            items.push(LocalItem {
                sync_id: "settings_snapshots".to_string(),
                category: "settings_snapshot".to_string(),
                name: "settings_snapshots".to_string(),
                content,
                checksum,
                modified_at: file_modified_ms(&path),
                tier: SyncTier::CloudSync,
            });
        }
    }

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
                    items.push(LocalItem {
                        sync_id: format!("preset_{}", id),
                        category: "preset".to_string(),
                        name: id,
                        content,
                        checksum,
                        modified_at: file_modified_ms(&path),
                        tier: SyncTier::CloudSync,
                    });
                }
            }
        }
    }

    for (subdir, category, prefix) in &[
        ("primitives", "template_primitive", "template_primitive_"),
        ("indicators", "template_indicator", "template_indicator_"),
        ("compare", "template_compare", "template_compare_"),
        (
            "indicator_sets",
            "template_indicator_set",
            "template_indicator_set_",
        ),
        ("chart", "template_chart", "template_chart_"),
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
                        items.push(LocalItem {
                            sync_id: format!("{}{}", prefix, id),
                            category: category.to_string(),
                            name: id,
                            content,
                            checksum,
                            modified_at: file_modified_ms(&path),
                            tier: SyncTier::CloudSync,
                        });
                    }
                }
            }
        }
    }

    {
        let path = data_dir.join("templates").join("indicator_set_manager.json");
        if let Ok(content) = std::fs::read_to_string(&path) {
            let checksum = sha256_hex(&content);
            items.push(LocalItem {
                sync_id: "indicator_set_manager".to_string(),
                category: "indicator_set_manager".to_string(),
                name: "indicator_set_manager".to_string(),
                content,
                checksum,
                modified_at: file_modified_ms(&path),
                tier: SyncTier::CloudSync,
            });
        }
    }

    // Read profile.json once; reused for theme and window_layout below.
    let profile_path = data_dir.join("profile.json");
    let profile_json_value: Option<serde_json::Value> = std::fs::read_to_string(&profile_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());

    {
        // Theme is stored as the `active_theme` field inside profile.json, not in a
        // separate theme.json file.
        if let Some(ref profile_val) = profile_json_value {
            if let Some(theme_str) = profile_val
                .get("active_theme")
                .and_then(|v| v.as_str())
            {
                let content = theme_str.to_string();
                let checksum = sha256_hex(&content);
                items.push(LocalItem {
                    sync_id: "theme".to_string(),
                    category: "theme".to_string(),
                    name: "active_theme".to_string(),
                    content,
                    checksum,
                    modified_at: file_modified_ms(&profile_path),
                    tier: SyncTier::CloudSync,
                });
            }
        }
    }

    {
        // Sync stripped window layout (open_tabs, active_preset_id, sidebar state).
        // Strip device-local geometry before pushing.
        if let Some(ref profile_val) = profile_json_value {
            if let Some(windows_arr) = profile_val.get("windows").and_then(|w| w.as_array()) {
                let stripped: Vec<serde_json::Value> = windows_arr
                    .iter()
                    .filter_map(|win| win.as_object())
                    .map(|obj| {
                        let mut stripped_win = serde_json::Map::new();
                        for key in &["open_tabs", "active_preset_id", "sidebar_visible", "sidebar_panel"] {
                            if let Some(v) = obj.get(*key) {
                                stripped_win.insert(key.to_string(), v.clone());
                            }
                        }
                        serde_json::Value::Object(stripped_win)
                    })
                    .collect();
                if !stripped.is_empty() {
                    let content = serde_json::to_string(&stripped)
                        .unwrap_or_default();
                    let checksum = sha256_hex(&content);
                    items.push(LocalItem {
                        sync_id: "window_layout".to_string(),
                        category: "window_layout".to_string(),
                        name: "window_layout".to_string(),
                        content,
                        checksum,
                        modified_at: file_modified_ms(&profile_path),
                        tier: SyncTier::CloudSync,
                    });
                }
            }
        }
    }

    {
        let path = data_dir.join("salt.hex");
        if let Ok(content) = std::fs::read_to_string(&path) {
            // salt.hex contains a raw 32-char hex string (e.g. "bdbe042807d325f69c7763cab16cdd4d")
            let content = content.trim().to_string();
            let checksum = sha256_hex(&content);
            items.push(LocalItem {
                sync_id: "salt".to_string(),
                category: "salt".to_string(),
                name: "salt".to_string(),
                content,
                checksum,
                modified_at: file_modified_ms(&path),
                tier: SyncTier::CloudSync,
            });
        }
    }

    // --- ZtBlob tier ---

    {
        let path = data_dir.join("vault.enc");
        if path.exists() {
            if let Ok(bytes) = std::fs::read(&path) {
                let content = base64::engine::general_purpose::STANDARD.encode(&bytes);
                let checksum = sha256_hex(&content);
                items.push(LocalItem {
                    sync_id: "vault".to_string(),
                    category: "vault".to_string(),
                    name: "vault".to_string(),
                    content,
                    checksum,
                    modified_at: file_modified_ms(&path),
                    tier: SyncTier::ZtBlob,
                });
            }
        }
    }

    // recovery_key.enc — ZtBlob tier (pre-encrypted recovery blob)
    {
        let path = data_dir.join("recovery_key.enc");
        if path.exists() {
            if let Ok(bytes) = std::fs::read(&path) {
                let content = base64::engine::general_purpose::STANDARD.encode(&bytes);
                let checksum = sha256_hex(&content);
                let mtime = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as i64
                    })
                    .unwrap_or(0);
                items.push(LocalItem {
                    sync_id: "recovery_key".to_string(),
                    category: "recovery_key".to_string(),
                    name: "recovery_key".to_string(),
                    content,
                    checksum,
                    modified_at: mtime,
                    tier: SyncTier::ZtBlob,
                });
            }
        }
    }

    eprintln!(
        "[{} CloudSync] collect_local_items: {} total items",
        crate::now_ts(),
        items.len()
    );
    LocalItems { items }
}

// =============================================================================
// File backup helpers
// =============================================================================

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

    cleanup_old_backups(&backup_dir, &filename, 5)?;

    Ok(())
}

fn cleanup_old_backups(
    backup_dir: &Path,
    original_name: &str,
    keep: usize,
) -> std::io::Result<()> {
    let mut backups: Vec<_> = std::fs::read_dir(backup_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(original_name))
        .collect();

    backups.sort_by_key(|e| e.file_name());

    if backups.len() > keep {
        for entry in &backups[..backups.len() - keep] {
            let _ = std::fs::remove_file(entry.path());
        }
    }

    Ok(())
}

/// Write pulled sync items to disk.
///
/// Checksums are verified before write; mismatches are skipped.
/// Writes are atomic via a `.tmp` side-file.
pub fn write_sync_items_to_disk(data_dir: &Path, items: &[SyncItem]) -> std::io::Result<()> {
    for item in items {
        let actual_checksum = sha256_hex(&item.content);
        if actual_checksum != item.checksum {
            log::warn!(
                "[CloudSync] Checksum mismatch for '{}' — skipping write",
                item.sync_id
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
            "template_compare" => {
                let dir = data_dir.join("templates").join("compare");
                std::fs::create_dir_all(&dir)?;
                let target = dir.join(format!("{}.json", item.name));
                backup_file(&target)?;
                let tmp = target.with_extension("tmp");
                std::fs::write(&tmp, &item.content)?;
                std::fs::rename(&tmp, &target)?;
            }
            "template_indicator_set" => {
                let dir = data_dir.join("templates").join("indicator_sets");
                std::fs::create_dir_all(&dir)?;
                let target = dir.join(format!("{}.json", item.name));
                backup_file(&target)?;
                let tmp = target.with_extension("tmp");
                std::fs::write(&tmp, &item.content)?;
                std::fs::rename(&tmp, &target)?;
            }
            "template_chart" => {
                let dir = data_dir.join("templates").join("chart");
                std::fs::create_dir_all(&dir)?;
                let target = dir.join(format!("{}.json", item.name));
                backup_file(&target)?;
                let tmp = target.with_extension("tmp");
                std::fs::write(&tmp, &item.content)?;
                std::fs::rename(&tmp, &target)?;
            }
            "indicator_set_manager" => {
                let dir = data_dir.join("templates");
                std::fs::create_dir_all(&dir)?;
                let target = dir.join("indicator_set_manager.json");
                backup_file(&target)?;
                let tmp = target.with_extension("tmp");
                std::fs::write(&tmp, &item.content)?;
                std::fs::rename(&tmp, &target)?;
            }
            "window_layout" => {
                // Merge synced tab/preset layout back into profile.json, preserving
                // all device-local geometry fields (x, y, width, height, etc.).
                let target = data_dir.join("profile.json");
                backup_file(&target)?;
                let mut profile_val: serde_json::Value = if target.exists() {
                    let raw = std::fs::read_to_string(&target)?;
                    serde_json::from_str(&raw).map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
                    })?
                } else {
                    serde_json::Value::Object(serde_json::Map::new())
                };
                let synced_windows: Vec<serde_json::Value> =
                    serde_json::from_str(&item.content).map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
                    })?;
                // Merge: for each synced window (by index), overlay the synced layout
                // keys onto the existing local window entry.
                if let Some(local_windows) = profile_val
                    .get_mut("windows")
                    .and_then(|w| w.as_array_mut())
                {
                    for (i, synced_win) in synced_windows.iter().enumerate() {
                        if let (Some(local_win), Some(synced_obj)) =
                            (local_windows.get_mut(i), synced_win.as_object())
                        {
                            if let Some(local_obj) = local_win.as_object_mut() {
                                for key in &["open_tabs", "active_preset_id", "sidebar_visible", "sidebar_panel"] {
                                    if let Some(v) = synced_obj.get(*key) {
                                        local_obj.insert(key.to_string(), v.clone());
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // No existing windows array — insert the synced one as-is.
                    if let Some(obj) = profile_val.as_object_mut() {
                        obj.insert(
                            "windows".to_string(),
                            serde_json::Value::Array(synced_windows),
                        );
                    }
                }
                let updated = serde_json::to_string_pretty(&profile_val).map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
                })?;
                let tmp = target.with_extension("tmp");
                std::fs::write(&tmp, &updated)?;
                std::fs::rename(&tmp, &target)?;
            }
            "theme" => {
                // Theme is the active_theme string inside profile.json.
                // Read profile.json, update the field, write back atomically.
                let target = data_dir.join("profile.json");
                backup_file(&target)?;
                let mut profile_val: serde_json::Value = if target.exists() {
                    let raw = std::fs::read_to_string(&target)?;
                    serde_json::from_str(&raw).map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
                    })?
                } else {
                    serde_json::Value::Object(serde_json::Map::new())
                };
                if let Some(obj) = profile_val.as_object_mut() {
                    obj.insert(
                        "active_theme".to_string(),
                        serde_json::Value::String(item.content.clone()),
                    );
                }
                let updated = serde_json::to_string_pretty(&profile_val).map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
                })?;
                let tmp = target.with_extension("tmp");
                std::fs::write(&tmp, &updated)?;
                std::fs::rename(&tmp, &target)?;
            }
            "vault" => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(&item.content)
                    .map_err(|e| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("vault base64 decode: {}", e),
                        )
                    })?;
                let target = data_dir.join("vault.enc");
                backup_file(&target)?;
                let tmp = target.with_extension("tmp");
                std::fs::write(&tmp, &bytes)?;
                std::fs::rename(&tmp, &target)?;
            }
            "recovery_key" => {
                let target = data_dir.join("recovery_key.enc");
                // Don't overwrite if already exists — recovery blob is immutable.
                if !target.exists() {
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(&item.content)
                        .map_err(|e| {
                            std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!("recovery_key base64 decode: {}", e),
                            )
                        })?;
                    backup_file(&target)?;
                    let tmp = target.with_extension("tmp");
                    std::fs::write(&tmp, &bytes)?;
                    std::fs::rename(&tmp, &target)?;
                }
            }
            "salt" => {
                let target = data_dir.join("salt.hex");
                backup_file(&target)?;
                let tmp = target.with_extension("tmp");
                std::fs::write(&tmp, &item.content)?;
                std::fs::rename(&tmp, &target)?;
            }
            other => {
                log::warn!(
                    "[CloudSync] Unknown sync category '{}' — skipping write",
                    other
                );
            }
        }
    }
    Ok(())
}

// =============================================================================
// HTTP functions
// =============================================================================

/// Check whether the server has any cloud data for the authenticated user.
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
/// Returns `(items, server_timestamp)`.
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

    let resp = builder.send().await.map_err(|e| {
        eprintln!("[{} CloudSync] push_items send error: {}", crate::now_ts(), e);
        format!("sync push request: {}", e)
    })?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("sync push read body: {}", e))?;
    eprintln!(
        "[{} CloudSync] push_items response: status={} body_len={} body={}",
        crate::now_ts(),
        status,
        body.len(),
        &body[..500.min(body.len())]
    );

    if !status.is_success() {
        return Err(format!(
            "sync push: HTTP {} body={}",
            status,
            &body[..500.min(body.len())]
        ));
    }

    let data: PushResponse =
        serde_json::from_str(&body).map_err(|e| format!("sync push parse: {}", e))?;

    if !data.rejected.is_empty() {
        eprintln!(
            "[{} CloudSync] Push: {} item(s) rejected by server: {:?}",
            crate::now_ts(),
            data.rejected.len(),
            data.rejected
        );
    }

    Ok(data.accepted)
}

// =============================================================================
// Cloud Profile Restore — list profiles, pull a specific profile, write to disk
// =============================================================================

/// Summary information about one profile stored on the server.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CloudProfileInfo {
    /// UUID identifier for this profile.
    pub profile_id: String,
    /// Total number of sync items for this profile.
    pub item_count: i64,
    /// Total bytes across all sync items.
    pub total_bytes: i64,
    /// Unix timestamp (milliseconds) of the most recently modified item.
    pub last_modified: i64,
    /// Whether the server holds a `vault` category item for this profile.
    pub has_vault: bool,
    /// Whether the server holds a `recovery_key` category item for this profile.
    pub has_recovery_key: bool,
}

/// List all profiles the authenticated user has stored on the server.
///
/// Returns a vec of [`CloudProfileInfo`] — one entry per profile UUID.
/// Returns an empty vec if the user has no synced profiles.
pub async fn list_cloud_profiles(
    client: &reqwest::Client,
    server_url: &str,
    token: &str,
    build_attest: &BuildAttestation,
    device_id: &str,
) -> Result<Vec<CloudProfileInfo>, String> {
    let builder = client
        .get(format!("{}/api/sync/profiles", server_url))
        .bearer_auth(token)
        .header("X-Device-Id", device_id)
        .timeout(std::time::Duration::from_secs(15));

    let builder = crate::attest::with_attestation(builder, build_attest);

    let resp = builder
        .send()
        .await
        .map_err(|e| format!("list cloud profiles request: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("list cloud profiles: HTTP {}", resp.status()));
    }

    #[derive(Deserialize)]
    struct Resp {
        profiles: Vec<CloudProfileInfo>,
    }

    let data: Resp = resp
        .json()
        .await
        .map_err(|e| format!("list cloud profiles parse: {}", e))?;

    Ok(data.profiles)
}

/// Pull all sync items for a specific profile from the server.
///
/// Used during Cloud Profile Restore to download a profile the user has
/// stored from another device.
pub async fn pull_profile(
    client: &reqwest::Client,
    server_url: &str,
    token: &str,
    build_attest: &BuildAttestation,
    profile_id: &str,
    device_id: &str,
) -> Result<Vec<SyncItem>, String> {
    let builder = client
        .get(format!(
            "{}/api/sync/pull-profile?profile_id={}",
            server_url, profile_id
        ))
        .bearer_auth(token)
        .header("X-Device-Id", device_id)
        .timeout(std::time::Duration::from_secs(30));

    let builder = crate::attest::with_attestation(builder, build_attest);

    let resp = builder
        .send()
        .await
        .map_err(|e| format!("pull profile request: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("pull profile: HTTP {}", resp.status()));
    }

    #[derive(Deserialize)]
    struct Resp {
        items: Vec<SyncItem>,
    }

    let data: Resp = resp
        .json()
        .await
        .map_err(|e| format!("pull profile parse: {}", e))?;

    Ok(data.items)
}

/// Write a set of pulled sync items into a new profile directory on disk.
///
/// Creates `{profiles_dir}/{profile_id}/` and writes all items into it via
/// [`write_sync_items_to_disk`].  If no `profile.json` was included in the
/// sync items a minimal skeleton is written so the profile can be loaded.
///
/// Returns the path to the newly created profile directory.
pub fn restore_profile_to_disk(
    items: &[SyncItem],
    profile_id: &str,
) -> Result<std::path::PathBuf, String> {
    let profiles_dir = zengeld_chart::user_profile::storage::profiles_dir();
    let profile_dir = profiles_dir.join(profile_id);

    std::fs::create_dir_all(&profile_dir)
        .map_err(|e| format!("create profile dir: {}", e))?;

    write_sync_items_to_disk(&profile_dir, items)
        .map_err(|e| format!("write sync items: {}", e))?;

    // If no profile.json was produced by write_sync_items_to_disk, write a
    // minimal skeleton so the profile can be opened by the app.
    let profile_json = profile_dir.join("profile.json");
    if !profile_json.exists() {
        let skeleton = serde_json::json!({
            "profile_id": profile_id,
            "display_name": "Restored Profile",
            "avatar": "chart",
            "cloud_enabled": true,
        });
        let content = serde_json::to_string_pretty(&skeleton)
            .map_err(|e| format!("serialize profile skeleton: {}", e))?;
        std::fs::write(&profile_json, content)
            .map_err(|e| format!("write profile.json skeleton: {}", e))?;
    }

    Ok(profile_dir)
}

/// Pull every item for this user from the server (full download).
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
// Cloud sync cycle — CloudSync-tier items (LWW push-only)
// =============================================================================

/// Perform one incremental sync cycle for CloudSync-tier items.
///
/// Items are pushed as plaintext — no client-side encryption.
/// Vault blobs are handled separately by [`do_zt_blob_push`].
pub async fn do_cloud_sync(
    client: &reqwest::Client,
    server_url: &str,
    token: &str,
    state: &SyncState,
    local_items: &LocalItems,
    build_attest: &BuildAttestation,
    profile_id: &str,
    device_id: &str,
) -> Result<SyncCycleResult, String> {
    // Filter to CloudSync tier and apply toggle gates.
    // For presets, strip heavy fields before computing effective checksum.
    let filtered: Vec<SyncItem> = local_items
        .cloud_sync_items()
        .filter(|item| match item.category.as_str() {
            "preset" | "window_layout" => state.sync_presets,
            "template_indicator"
            | "template_primitive"
            | "template_compare"
            | "template_indicator_set"
            | "template_chart"
            | "indicator_set_manager" => state.sync_templates,
            "watchlist" => state.sync_watchlists,
            "theme" => state.sync_theme,
            _ => true,
        })
        .map(|item| {
            if item.category == "preset" {
                let stripped = strip_preset_for_sync(&item.content);
                let checksum = sha256_hex(&stripped);
                SyncItem {
                    sync_id: item.sync_id.clone(),
                    category: item.category.clone(),
                    name: item.name.clone(),
                    content: stripped,
                    checksum,
                    modified_at: item.modified_at,
                    deleted: false,
                }
            } else {
                SyncItem {
                    sync_id: item.sync_id.clone(),
                    category: item.category.clone(),
                    name: item.name.clone(),
                    content: item.content.clone(),
                    checksum: item.checksum.clone(),
                    modified_at: item.modified_at,
                    deleted: false,
                }
            }
        })
        .collect();

    let local_index: std::collections::HashMap<&str, &SyncItem> =
        filtered.iter().map(|i| (i.sync_id.as_str(), i)).collect();

    // Tombstone detection.
    let tombstone_checksum = sha256_hex("");
    let now_for_tombstones = {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        if ms > 0 { ms } else { 1 }
    };

    // ZtBlob sync_ids must not generate tombstones in the cloud sync path —
    // they live in a separate pipeline and their absence from CloudSync items
    // is expected, not a deletion.
    let zt_blob_ids: std::collections::HashSet<&str> = local_items
        .zt_blob_items()
        .map(|i| i.sync_id.as_str())
        .collect();

    let mut tombstones: Vec<SyncItem> = state
        .synced_items
        .iter()
        .filter(|id| !local_index.contains_key(id.as_str()))
        .filter(|id| !zt_blob_ids.contains(id.as_str()))
        // Also skip well-known ZtBlob ids even if no file exists on disk.
        .filter(|id| id.as_str() != "vault" && id.as_str() != "recovery_key")
        .map(|id| {
            // IMPORTANT: check longer prefixes before shorter ones.
            let (category, name) = if id == "watchlists" {
                ("watchlist".to_string(), "watchlists".to_string())
            } else if id == "settings_snapshots" {
                ("settings_snapshot".to_string(), "settings_snapshots".to_string())
            } else if id == "theme" {
                ("theme".to_string(), "active_theme".to_string())
            } else if id == "salt" {
                ("salt".to_string(), "salt".to_string())
            } else if id == "indicator_set_manager" {
                ("indicator_set_manager".to_string(), "indicator_set_manager".to_string())
            } else if id == "window_layout" {
                ("window_layout".to_string(), "window_layout".to_string())
            } else if let Some(rest) = id.strip_prefix("template_indicator_set_") {
                ("template_indicator_set".to_string(), rest.to_string())
            } else if let Some(rest) = id.strip_prefix("template_indicator_") {
                ("template_indicator".to_string(), rest.to_string())
            } else if let Some(rest) = id.strip_prefix("template_primitive_") {
                ("template_primitive".to_string(), rest.to_string())
            } else if let Some(rest) = id.strip_prefix("template_compare_") {
                ("template_compare".to_string(), rest.to_string())
            } else if let Some(rest) = id.strip_prefix("template_chart_") {
                ("template_chart".to_string(), rest.to_string())
            } else if let Some(rest) = id.strip_prefix("preset_") {
                ("preset".to_string(), rest.to_string())
            } else {
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
            "[CloudSync] {} tombstone(s) for locally deleted items: {:?}",
            tombstones.len(),
            tombstones.iter().map(|t| t.sync_id.as_str()).collect::<Vec<_>>()
        );
    }

    let (server_changes, server_timestamp) = fetch_changes(
        client,
        server_url,
        token,
        state.last_sync_timestamp,
        build_attest,
        profile_id,
        device_id,
    )
    .await?;

    log::debug!(
        "[CloudSync] do_cloud_sync: {} local, {} server changes since ts={}",
        filtered.len(),
        server_changes.len(),
        state.last_sync_timestamp
    );

    let server_index: std::collections::HashMap<&str, &SyncItemMeta> =
        server_changes.iter().map(|m| (m.sync_id.as_str(), m)).collect();

    let mut to_push: Vec<SyncItem> = Vec::new();
    let conflicts: Vec<SyncConflict> = Vec::new();

    for local in &filtered {
        // Skip if checksum matches what we last successfully pushed
        if let Some(last_cs) = state.last_synced_checksums.get(&local.sync_id) {
            if last_cs == &local.checksum {
                log::trace!("[CloudSync] Skipping unchanged: {}", local.sync_id);
                continue;
            }
        }
        // Then check server state for conflict detection
        match server_index.get(local.sync_id.as_str()) {
            Some(server_meta)
                if !server_meta.deleted && server_meta.checksum == local.checksum =>
            {
                log::trace!("[CloudSync] In sync with server: {}", local.sync_id);
            }
            _ => {
                log::debug!("[CloudSync] Will push (changed): {}", local.sync_id);
                to_push.push(local.clone());
            }
        }
    }

    to_push.append(&mut tombstones);

    // CloudSync pushes plaintext — no client-side encryption.
    let items_to_push = &to_push;

    eprintln!("[{} CloudSync] do_cloud_sync: to_push={}", crate::now_ts(), to_push.len());
    for item in &to_push {
        eprintln!(
            "[{} CloudSync]   push: id={} cat={} bytes={}",
            crate::now_ts(),
            item.sync_id,
            item.category,
            item.content.len()
        );
    }

    let mut pushed_count = 0usize;
    let mut pushed_ids: Vec<String> = Vec::new();

    for batch in items_to_push.chunks(50) {
        eprintln!(
            "[{} CloudSync] pushing batch of {} items to {}/api/sync/push",
            crate::now_ts(),
            batch.len(),
            server_url
        );
        match push_items(client, server_url, token, batch, build_attest, profile_id, device_id)
            .await
        {
            Ok(n) => {
                eprintln!("[{} CloudSync] push OK: accepted={}", crate::now_ts(), n);
                pushed_count += n;
                for item in batch {
                    pushed_ids.push(item.sync_id.clone());
                }
            }
            Err(e) => {
                eprintln!("[{} CloudSync] push FAILED: {}", crate::now_ts(), e);
            }
        }
    }

    let pulled_count = 0usize;
    let written_items: Vec<SyncItem> = Vec::new();

    let sync_timestamp = if server_timestamp > 0 {
        server_timestamp
    } else {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    };

    let mut new_checksums = state.last_synced_checksums.clone();

    for item in &to_push {
        if pushed_ids.contains(&item.sync_id) {
            if !item.deleted {
                new_checksums.insert(item.sync_id.clone(), item.checksum.clone());
            } else {
                new_checksums.remove(&item.sync_id);
            }
        }
    }
    for item in &written_items {
        let cs = sha256_hex(&item.content);
        new_checksums.insert(item.sync_id.clone(), cs);
    }

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
        "[CloudSync] do_cloud_sync complete: pushed={} pulled={} conflicts={}",
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
// ZT blob push — pre-encrypted vault and recovery-key blobs
// =============================================================================

/// Push ZtBlob-tier items to the server without re-encryption.
///
/// Toggle flags and LWW dedup are applied.  No tombstones, pull, or conflicts.
pub async fn do_zt_blob_push(
    client: &reqwest::Client,
    server_url: &str,
    token: &str,
    state: &SyncState,
    local_items: &LocalItems,
    build_attest: &BuildAttestation,
    profile_id: &str,
    device_id: &str,
) -> Result<ZtBlobResult, String> {
    let blobs: Vec<SyncItem> = local_items
        .zt_blob_items()
        .filter(|item| match item.sync_id.as_str() {
            "vault" => state.sync_vault,
            "recovery_key" => state.sync_recovery_key,
            _ => true,
        })
        .filter(|item| {
            state
                .last_synced_checksums
                .get(&item.sync_id)
                .map_or(true, |last| last != &item.checksum)
        })
        .map(|item| SyncItem {
            sync_id: item.sync_id.clone(),
            category: item.category.clone(),
            name: item.name.clone(),
            content: item.content.clone(),
            checksum: item.checksum.clone(),
            modified_at: item.modified_at,
            deleted: false,
        })
        .collect();

    if blobs.is_empty() {
        log::debug!("[CloudSync] do_zt_blob_push: nothing to push");
        return Ok(ZtBlobResult {
            pushed: 0,
            id_to_checksum: std::collections::HashMap::new(),
        });
    }

    eprintln!("[{} CloudSync] do_zt_blob_push: pushing {} blob(s)", crate::now_ts(), blobs.len());

    let mut pushed_count = 0usize;
    let mut id_to_checksum: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for batch in blobs.chunks(50) {
        match push_items(client, server_url, token, batch, build_attest, profile_id, device_id)
            .await
        {
            Ok(n) => {
                pushed_count += n;
                for item in batch {
                    id_to_checksum.insert(item.sync_id.clone(), item.checksum.clone());
                }
            }
            Err(e) => {
                log::warn!("[CloudSync] do_zt_blob_push batch failed: {}", e);
            }
        }
    }

    Ok(ZtBlobResult {
        pushed: pushed_count,
        id_to_checksum,
    })
}

// =============================================================================
// Preset stripping helpers
// =============================================================================

/// Strip heavy/device-specific fields from a preset JSON before syncing.
///
/// Removes from each window snapshot:
/// - `bars` — re-fetchable from exchanges.
/// - `viewport` — device-specific pan/zoom state.
/// - `stashed_command_history` — local undo stash.
/// - `symbol_drawings_snapshots` — per-symbol drawing cache.
/// - `ViewportChange` commands from `command_history.{undo,redo}_stack`.
///
/// Same stripping applied to compare-overlay series bars and to windows
/// nested inside `sync_groups`.
fn strip_preset_for_sync(raw_json: &str) -> String {
    let Ok(mut val) = serde_json::from_str::<serde_json::Value>(raw_json) else {
        return raw_json.to_string();
    };

    if let Some(windows) = val.get_mut("windows").and_then(|w| w.as_array_mut()) {
        for win in windows.iter_mut() {
            if let Some(obj) = win.as_object_mut() {
                obj.remove("bars");
                obj.remove("viewport");
                obj.remove("stashed_command_history");
                obj.remove("symbol_drawings_snapshots");

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

    if let Some(groups) = val.get_mut("sync_groups").and_then(|g| g.as_array_mut()) {
        for group in groups.iter_mut() {
            if let Some(windows) = group.get_mut("windows").and_then(|w| w.as_array_mut()) {
                for win in windows.iter_mut() {
                    if let Some(obj) = win.as_object_mut() {
                        obj.remove("bars");
                        obj.remove("viewport");
                        obj.remove("stashed_command_history");
                        obj.remove("symbol_drawings_snapshots");

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

    // Strip cached indicator values — regenerable data, must not inflate sync payloads.
    if let Some(indicators) = val.get_mut("indicators").and_then(|i| i.as_array_mut()) {
        for ind in indicators.iter_mut() {
            if let Some(obj) = ind.as_object_mut() {
                obj.remove("values");
            }
        }
    }

    serde_json::to_string(&val).unwrap_or_else(|_| raw_json.to_string())
}
