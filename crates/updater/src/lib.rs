//! zengeld-updater — OTA update client, telemetry, and OAuth for the chart application.
//!
//! Background task that checks for updates, sends anonymized metrics,
//! and manages user authentication via OAuth providers.

pub mod state;
pub mod platform;
pub mod token_store;
pub mod check;
pub mod download;
pub mod replace;
pub mod telemetry;
pub mod oauth;
pub mod key_sync;  // kept as stub — cloud sync of local agent keys was removed
pub mod cloud_sync;
pub mod vault_params;
pub mod verify;
pub mod attest;

pub use state::{UpdaterHandle, UpdaterCommand, UpdateStatus, UpdateInfo, AuthStatus, SyncStatus, BuildAttestation, SyncConflict, ConflictResolution};

/// Format current time as `HH:MM:SS` for log lines.
pub fn now_ts() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // UTC time — simple formatting without chrono dependency.
    let day_secs = (secs % 86400) as u32;
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

use tokio::sync::{mpsc, watch};
use std::sync::Arc;

/// Interval between update checks (5 minutes — testing; bump to 4h for production).
const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Base URL for the update server.
const UPDATE_SERVER: &str = "https://chart.example";

/// Trait for providing telemetry data from the application.
/// Implemented by the main app to feed live metrics to the updater.
pub trait TelemetrySource: Send + Sync + 'static {
    fn collect(&self) -> telemetry::TelemetryPayload;
}

/// Start the updater background task. Returns a handle for UI interaction.
///
/// Call this after the DataBridge is created. The background task runs on
/// the existing tokio runtime (spawned via `tokio::spawn`).
///
/// `connected` controls whether the updater makes any HTTP calls to
/// chart.example on startup. In standalone mode (`connected = false`)
/// the loop still runs so it can handle commands, but no network traffic
/// is generated until a `SetCloudEnabled(true)` command arrives.
///
/// `telemetry_enabled` controls whether anonymized metrics are sent.  Can be
/// toggled at runtime via [`UpdaterCommand::SetTelemetryEnabled`].
///
/// `sync_enabled` seeds the initial cloud-sync enabled state from the user
/// profile.  Can be toggled at runtime via [`UpdaterCommand::SetSyncEnabled`].
///
/// `build_attest` carries the compile-time attestation values produced by
/// `chart-app-vello/build.rs`.  Pass [`BuildAttestation::default`] for dev
/// builds or when running without the binary crate context.
///
/// `initial_synced_items` seeds the tombstone-detection set from the persisted
/// profile so that items deleted between sessions are still tombstoned on the
/// next sync.  Pass an empty `HashSet` if the profile does not yet have this
/// data (first run or older profile).
///
/// `initial_last_synced_checksums` seeds the conflict-detection checksum map
/// from the persisted profile.  Without seeding, the map starts empty after
/// every restart, causing all items to appear as conflicts on the first sync.
/// Pass an empty `HashMap` if the profile does not yet have this data.
pub fn start(
    runtime: &tokio::runtime::Handle,
    telemetry_source: Arc<dyn TelemetrySource>,
    connected: bool,
    telemetry_enabled: bool,
    sync_enabled: bool,
    initial_synced_items: std::collections::HashSet<String>,
    initial_last_synced_checksums: std::collections::HashMap<String, String>,
    data_dir: std::path::PathBuf,
    build_attest: state::BuildAttestation,
    profile_id: String,
    server_port: u16,
) -> UpdaterHandle {
    let (status_tx, status_rx) = watch::channel(UpdateStatus::Idle);
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

    // Seed the initial auth status from whatever token is already on disk.
    let initial_auth = token_store::load_token()
        .map(|t| AuthStatus::LoggedIn {
            display_name: t.display_name,
            provider: t.provider,
            user_id: t.user_id,
        })
        .unwrap_or(AuthStatus::NotLoggedIn);
    let (auth_tx, auth_rx) = watch::channel(initial_auth);

    // Channel for cloud sync status — starts Idle, updated by the updater loop.
    let (sync_status_tx, sync_status_rx) = watch::channel(state::SyncStatus::Idle);

    // Channel for persisting last_synced_checksums — the updater sends the
    // updated map after each successful sync cycle so main.rs can write it
    // back into the profile for persistence across restarts.
    let (sync_checksums_tx, sync_checksums_rx) =
        watch::channel(std::collections::HashMap::<String, String>::new());

    let handle = UpdaterHandle {
        status_rx,
        cmd_tx,
        auth_rx,
        sync_status_rx,
        sync_checksums_rx,
    };

    runtime.spawn(updater_loop(status_tx, auth_tx, sync_status_tx, sync_checksums_tx, cmd_rx, telemetry_source, connected, telemetry_enabled, sync_enabled, initial_synced_items, initial_last_synced_checksums, data_dir, build_attest, profile_id, server_port));

    handle
}

/// Synchronously wait for the parent process to exit (called at top of main).
/// This is used during OTA self-replace: the new binary is launched with
/// `--wait-pid <old_pid>` and must wait for the old process to release resources.
pub fn wait_for_parent_exit_if_needed() {
    replace::wait_for_parent_exit();
}

async fn updater_loop(
    status_tx: watch::Sender<UpdateStatus>,
    auth_tx: watch::Sender<state::AuthStatus>,
    sync_status_tx: watch::Sender<state::SyncStatus>,
    sync_checksums_tx: watch::Sender<std::collections::HashMap<String, String>>,
    mut cmd_rx: mpsc::UnboundedReceiver<state::UpdaterCommand>,
    telemetry_source: Arc<dyn TelemetrySource>,
    mut connected: bool,
    mut telemetry_enabled: bool,
    sync_enabled_init: bool,
    initial_synced_items: std::collections::HashSet<String>,
    initial_last_synced_checksums: std::collections::HashMap<String, String>,
    mut data_dir: std::path::PathBuf,
    build_attest: state::BuildAttestation,
    mut profile_id: String,
    server_port: u16,
) {
    // Obtain device_id once at startup — stable for the life of this process.
    let device_id = telemetry::get_or_create_device_id();
    let current_version = env!("CARGO_PKG_VERSION");
    eprintln!("[{} Updater] Starting — current version: v{}, device_id: {}", now_ts(), current_version, device_id);

    // Shared HTTP client for key sync requests.
    // Built once here; reused on every interval tick.
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    // In-memory sync state — updated after each successful cycle.
    // The authoritative state is owned by main.rs (in UserProfile); this copy
    // lets the updater loop track the last-sync timestamp without needing a
    // channel back to main.rs on every tick.
    //
    // `synced_items` is seeded from the persisted profile so tombstone detection
    // works across restarts: if an item was pushed last session and is gone now,
    // the next cycle will detect it and push a tombstone.
    //
    // `last_synced_checksums` is seeded from the persisted profile so conflict
    // detection works correctly after a restart.  Without seeding, the empty map
    // would cause every locally-modified item to look like a conflict on the
    // first sync after startup.
    let mut sync_state = cloud_sync::SyncState {
        enabled: sync_enabled_init,
        synced_items: initial_synced_items,
        last_synced_checksums: initial_last_synced_checksums,
        ..cloud_sync::SyncState::default()
    };

    // Pending conflict map: sync_id → SyncConflict.
    //
    // Populated whenever a sync cycle returns conflicts.  Entries are removed
    // when the user resolves them via UpdaterCommand::ResolveConflict.
    let mut pending_conflicts: std::collections::HashMap<String, state::SyncConflict> =
        std::collections::HashMap::new();

    // Initial check on startup (with small delay to let the app initialize).
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    let mut check_interval = tokio::time::interval(CHECK_INTERVAL);
    check_interval.tick().await; // consume first immediate tick

    // Do initial check + telemetry only in connected mode.
    if connected {
        let token = token_store::load_token();
        let auth_header = token.as_ref().map(|t| format!("Bearer {}", t.token));
        do_check_and_telemetry(&status_tx, current_version, auth_header.as_deref(), &telemetry_source, telemetry_enabled).await;
    } else {
        log::info!("[Updater] Standalone mode — skipping initial update check and telemetry");
    }

    let mut pending_update: Option<state::UpdateInfo> = None;

    // Auto-install if an update was found on startup.
    if let UpdateStatus::UpdateAvailable(info) = &*status_tx.borrow() {
        pending_update = Some(info.clone());
    }
    if let Some(ref info) = pending_update {
        do_install(&status_tx, info, server_port).await;
    }

    loop {
        tokio::select! {
            _ = check_interval.tick() => {
                if connected {
                    let token = token_store::load_token();
                    let auth = token.as_ref().map(|t| format!("Bearer {}", t.token));
                    do_check_and_telemetry(&status_tx, current_version, auth.as_deref(), &telemetry_source, telemetry_enabled).await;

                    // If we found an update, cache it and auto-install.
                    if let UpdateStatus::UpdateAvailable(info) = &*status_tx.borrow() {
                        pending_update = Some(info.clone());
                    }
                    if let Some(ref info) = pending_update {
                        do_install(&status_tx, info, server_port).await;
                    }

                    // Cloud sync is now event-driven (SyncPushChanged command).
                    // The interval tick no longer triggers a sync cycle to avoid
                    // redundant full-read passes when the app is idle.
                }
                // In standalone mode: interval fires but we do nothing — no HTTP calls.
            }
            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    state::UpdaterCommand::ForceCheck => {
                        if connected {
                            let token = token_store::load_token();
                            let auth = token.as_ref().map(|t| format!("Bearer {}", t.token));
                            do_check_and_telemetry(&status_tx, current_version, auth.as_deref(), &telemetry_source, telemetry_enabled).await;
                            if let UpdateStatus::UpdateAvailable(info) = &*status_tx.borrow() {
                                pending_update = Some(info.clone());
                            }
                            // Auto-install on forced check as well.
                            if let Some(ref info) = pending_update {
                                do_install(&status_tx, info, server_port).await;
                            }
                        } else {
                            log::warn!("[Updater] ForceCheck ignored — running in standalone mode");
                        }
                    }
                    state::UpdaterCommand::InstallNow => {
                        if let Some(ref info) = pending_update {
                            do_install(&status_tx, info, server_port).await;
                        }
                    }
                    state::UpdaterCommand::DismissUpdate => {
                        let _ = status_tx.send(UpdateStatus::Idle);
                    }
                    state::UpdaterCommand::StartOAuth(provider) => {
                        let device_id = telemetry::get_or_create_device_id();
                        match oauth::start_oauth_flow(&provider, &device_id).await {
                            Ok(token) => {
                                let _ = token_store::save_token(&token);
                                log::info!("OAuth successful: {} ({})", token.display_name, token.provider);
                                let _ = auth_tx.send(state::AuthStatus::LoggedIn {
                                    display_name: token.display_name,
                                    provider: token.provider,
                                    user_id: token.user_id,
                                });
                            }
                            Err(e) => {
                                log::error!("OAuth failed: {}", e);
                            }
                        }
                    }
                    state::UpdaterCommand::Logout => {
                        token_store::clear_token();
                        let _ = auth_tx.send(state::AuthStatus::NotLoggedIn);
                        log::info!("Logged out");
                    }
                    state::UpdaterCommand::ForceSync => {
                        if connected {
                            let token = token_store::load_token();
                            if let Some(ref td) = token {
                                if sync_state.enabled {
                                    run_sync_pipeline(&http_client, &td.token, &sync_status_tx, &sync_checksums_tx, &mut sync_state, &data_dir, &build_attest, &mut pending_conflicts, &profile_id, &device_id).await;
                                } else {
                                    log::debug!("[Updater] ForceSync ignored — cloud sync not enabled by user");
                                }
                            } else {
                                log::debug!("[Updater] ForceSync ignored — not logged in");
                            }
                        } else {
                            log::warn!("[Updater] ForceSync ignored — running in standalone mode");
                        }
                    }
                    state::UpdaterCommand::SetCloudEnabled(new_mode) => {
                        let was_connected = connected;
                        connected = new_mode;
                        log::info!("[Updater] Client mode changed: connected={}", connected);
                        if connected && !was_connected {
                            // Switched from standalone → connected: do an immediate check.
                            log::info!("[Updater] Switched to connected mode — running immediate update check");
                            let token = token_store::load_token();
                            let auth = token.as_ref().map(|t| format!("Bearer {}", t.token));
                            do_check_and_telemetry(&status_tx, current_version, auth.as_deref(), &telemetry_source, telemetry_enabled).await;
                            if let UpdateStatus::UpdateAvailable(info) = &*status_tx.borrow() {
                                pending_update = Some(info.clone());
                            }
                            if let Some(ref info) = pending_update {
                                do_install(&status_tx, info, server_port).await;
                            }
                            // Cloud sync immediately after switching to connected.
                            if let Some(ref td) = token {
                                if sync_state.enabled {
                                    run_sync_pipeline(&http_client, &td.token, &sync_status_tx, &sync_checksums_tx, &mut sync_state, &data_dir, &build_attest, &mut pending_conflicts, &profile_id, &device_id).await;
                                }
                            }
                        }
                        // Switched connected → standalone: nothing to do, HTTP calls
                        // will simply be skipped on the next interval tick.
                    }
                    state::UpdaterCommand::SetTelemetryEnabled(enabled) => {
                        telemetry_enabled = enabled;
                        log::info!("[Updater] Telemetry enabled: {}", telemetry_enabled);
                    }
                    state::UpdaterCommand::SetSyncEnabled(enabled) => {
                        sync_state.enabled = enabled;
                        log::info!("[Updater] Cloud sync enabled: {}", sync_state.enabled);
                        if enabled && connected {
                            let token = token_store::load_token();
                            if let Some(ref td) = token {
                                eprintln!("[{} Updater] Sync enabled — triggering immediate sync", now_ts());
                                run_sync_pipeline(&http_client, &td.token, &sync_status_tx, &sync_checksums_tx, &mut sync_state, &data_dir, &build_attest, &mut pending_conflicts, &profile_id, &device_id).await;
                            }
                        }
                    }
                    state::UpdaterCommand::SetSyncPresets(val) => {
                        sync_state.sync_presets = val;
                        log::debug!("[Updater] sync_presets: {}", val);
                    }
                    state::UpdaterCommand::SetSyncTemplates(val) => {
                        sync_state.sync_templates = val;
                        log::debug!("[Updater] sync_templates: {}", val);
                    }
                    state::UpdaterCommand::SetSyncWatchlists(val) => {
                        sync_state.sync_watchlists = val;
                        log::debug!("[Updater] sync_watchlists: {}", val);
                    }
                    state::UpdaterCommand::SetSyncTheme(val) => {
                        sync_state.sync_theme = val;
                        log::debug!("[Updater] sync_theme: {}", val);
                    }
                    state::UpdaterCommand::SetSyncVault(val) => {
                        sync_state.sync_vault = val;
                        log::debug!("[Updater] sync_vault: {}", val);
                    }
                    state::UpdaterCommand::SetSyncRecoveryKey(val) => {
                        sync_state.sync_recovery_key = val;
                        log::debug!("[Updater] sync_recovery_key: {}", val);
                    }
                    state::UpdaterCommand::SetDataDir(path) => {
                        data_dir = path;
                        eprintln!("[{} Updater] data_dir updated to {:?}", now_ts(), data_dir);
                    }
                    state::UpdaterCommand::SetProfileId(id) => {
                        profile_id = id;
                        eprintln!("[{} Updater] profile_id updated to {}", now_ts(), profile_id);
                    }
                    state::UpdaterCommand::ResolveConflict { sync_id, resolution } => {
                        let conflict = match pending_conflicts.remove(&sync_id) {
                            Some(c) => c,
                            None => {
                                log::warn!("[Updater] ResolveConflict: unknown sync_id '{}'", sync_id);
                                continue;
                            }
                        };

                        if !connected {
                            log::warn!("[Updater] ResolveConflict ignored — running in standalone mode");
                            // Put the conflict back so it isn't silently lost.
                            pending_conflicts.insert(sync_id, conflict);
                            continue;
                        }

                        let token = token_store::load_token();
                        let td = match token {
                            Some(ref t) => t,
                            None => {
                                log::warn!("[Updater] ResolveConflict ignored — not logged in");
                                pending_conflicts.insert(sync_id, conflict);
                                continue;
                            }
                        };

                        match resolution {
                            state::ConflictResolution::KeepLocal => {
                                // Push local version to the server.
                                log::info!(
                                    "[Updater] Resolving conflict '{}' — KeepLocal: pushing local version",
                                    conflict.sync_id
                                );
                                let item = cloud_sync::SyncItem {
                                    sync_id: conflict.sync_id.clone(),
                                    category: conflict.category.clone(),
                                    name: conflict.name.clone(),
                                    content: conflict.local_content.clone(),
                                    checksum: conflict.local_checksum.clone(),
                                    modified_at: conflict.local_modified,
                                    deleted: false,
                                };
                                match cloud_sync::push_items(
                                    &http_client,
                                    UPDATE_SERVER,
                                    &td.token,
                                    &[item],
                                    &build_attest,
                                    &profile_id,
                                    &device_id,
                                )
                                .await
                                {
                                    Ok(_) => {
                                        log::info!(
                                            "[Updater] Conflict resolved (KeepLocal): '{}'",
                                            conflict.sync_id
                                        );
                                        // Update last-synced checksum.
                                        sync_state
                                            .last_synced_checksums
                                            .insert(conflict.sync_id.clone(), conflict.local_checksum.clone());
                                    }
                                    Err(e) => {
                                        log::warn!(
                                            "[Updater] KeepLocal push failed for '{}': {}",
                                            conflict.sync_id,
                                            e
                                        );
                                        // Re-queue so user can retry.
                                        pending_conflicts.insert(conflict.sync_id.clone(), conflict);
                                    }
                                }
                            }
                            state::ConflictResolution::KeepCloud => {
                                // Pull full item from server and write to disk.
                                //
                                // TODO(GAP-5): This calls pull_all() which fetches every item
                                // for the user, then filters to the one conflicted item.  This
                                // is suboptimal for users with many items.  A future improvement
                                // would add a `GET /api/sync/pull?sync_id={id}` endpoint on the
                                // server so only the single needed item is transferred.  For now
                                // the server does not support single-item pull, so we pull all
                                // and filter client-side.  Only the conflicted item is written to
                                // disk — all other pulled items are discarded.
                                log::info!(
                                    "[Updater] Resolving conflict '{}' — KeepCloud: pulling server version (note: fetches all items, then filters to this one)",
                                    conflict.sync_id
                                );
                                let conflict_id = conflict.sync_id.clone();
                                match cloud_sync::pull_all(
                                    &http_client,
                                    UPDATE_SERVER,
                                    &td.token,
                                    &build_attest,
                                    &profile_id,
                                    &device_id,
                                )
                                .await
                                {
                                    Ok(all_items) => {
                                        let found: Vec<cloud_sync::SyncItem> = all_items
                                            .into_iter()
                                            .filter(|i| i.sync_id == conflict_id)
                                            .collect();

                                        if found.is_empty() {
                                            log::warn!(
                                                "[Updater] KeepCloud: server item '{}' not found in pull response",
                                                conflict_id
                                            );
                                        } else {
                                            match cloud_sync::write_sync_items_to_disk(
                                                &data_dir,
                                                &found,
                                            ) {
                                                Ok(()) => {
                                                    log::info!(
                                                        "[Updater] Conflict resolved (KeepCloud): '{}'",
                                                        conflict_id
                                                    );
                                                    // Update last-synced checksum to cloud value.
                                                    sync_state
                                                        .last_synced_checksums
                                                        .insert(conflict_id.clone(), conflict.cloud_checksum.clone());
                                                }
                                                Err(e) => {
                                                    log::warn!(
                                                        "[Updater] KeepCloud write failed for '{}': {}",
                                                        conflict_id,
                                                        e
                                                    );
                                                    // Re-queue.
                                                    pending_conflicts.insert(conflict_id, conflict);
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        log::warn!(
                                            "[Updater] KeepCloud pull failed for '{}': {}",
                                            conflict_id,
                                            e
                                        );
                                        pending_conflicts.insert(conflict_id, conflict);
                                    }
                                }
                            }
                        }

                        // After resolution, re-broadcast remaining conflicts (or Idle if none).
                        if pending_conflicts.is_empty() {
                            sync_status_tx.send_replace(state::SyncStatus::Idle);
                        } else {
                            let remaining: Vec<state::SyncConflict> =
                                pending_conflicts.values().cloned().collect();
                            sync_status_tx.send_replace(state::SyncStatus::ConflictsDetected(remaining));
                        }
                    }
                    state::UpdaterCommand::SyncPushChanged(categories) => {
                        if connected && sync_state.enabled {
                            let token = token_store::load_token();
                            if let Some(ref td) = token {
                                eprintln!("[{} Updater] SyncPushChanged: {:?}", now_ts(), categories);
                                run_sync_pipeline(&http_client, &td.token, &sync_status_tx, &sync_checksums_tx, &mut sync_state, &data_dir, &build_attest, &mut pending_conflicts, &profile_id, &device_id).await;
                            } else {
                                log::debug!("[Updater] SyncPushChanged ignored — not logged in");
                            }
                        } else {
                            log::debug!("[Updater] SyncPushChanged ignored — cloud={} sync_enabled={}", connected, sync_state.enabled);
                        }
                    }
                    state::UpdaterCommand::Shutdown => {
                        log::info!("Updater shutdown requested");
                        break;
                    }
                }
            }
        }
    }
}

/// 3-step sync pipeline: collect → cloud_sync → zt_blob_push.
///
/// Step 1: Single disk-read pass — collect all local items with tier tags.
/// Step 2: CloudSync pipeline — structured plaintext data, LWW.
/// Step 3: ZT blob push — vault.enc, recovery_key.enc (already encrypted by
///         the vault layer).  Only attempted if Step 2 succeeded.
async fn run_sync_pipeline(
    client: &reqwest::Client,
    auth_token: &str,
    sync_status_tx: &watch::Sender<state::SyncStatus>,
    sync_checksums_tx: &watch::Sender<std::collections::HashMap<String, String>>,
    sync_state: &mut cloud_sync::SyncState,
    data_dir: &std::path::Path,
    build_attest: &state::BuildAttestation,
    pending_conflicts: &mut std::collections::HashMap<String, state::SyncConflict>,
    profile_id: &str,
    device_id: &str,
) {
    sync_status_tx.send_replace(state::SyncStatus::Syncing);

    // Step 1: Single disk-read pass — collect all local items with tier tags.
    let local_items = cloud_sync::collect_local_items(data_dir);

    // Step 2: CloudSync pipeline — structured plaintext data, LWW.
    match cloud_sync::do_cloud_sync(
        client, UPDATE_SERVER, auth_token, sync_state,
        &local_items, build_attest, profile_id, device_id,
    ).await {
        Ok(result) => {
            log::debug!(
                "[Updater] Cloud sync: pushed={} pulled={} conflicts={}",
                result.pushed, result.pulled, result.conflicts.len()
            );
            *sync_state = result.new_state;
            sync_checksums_tx.send_replace(sync_state.last_synced_checksums.clone());

            if result.conflicts.is_empty() {
                sync_status_tx.send_replace(state::SyncStatus::Completed {
                    pushed: result.pushed,
                    pulled: result.pulled,
                });
            } else {
                for conflict in &result.conflicts {
                    log::warn!(
                        "[Updater] Conflict detected: {} (local_cs={} cloud_cs={})",
                        conflict.sync_id,
                        &conflict.local_checksum[..8.min(conflict.local_checksum.len())],
                        &conflict.cloud_checksum[..8.min(conflict.cloud_checksum.len())]
                    );
                    pending_conflicts.insert(conflict.sync_id.clone(), conflict.clone());
                }
                let all_pending: Vec<state::SyncConflict> = pending_conflicts.values().cloned().collect();
                sync_status_tx.send_replace(state::SyncStatus::ConflictsDetected(all_pending));
            }
        }
        Err(e) => {
            log::warn!("[Updater] Cloud sync failed: {}", e);
            sync_status_tx.send_replace(state::SyncStatus::Error(e));
            return; // Don't attempt ZT blob push if cloud sync failed.
        }
    }

    // Step 3: ZT blob push — vault.enc, recovery_key.enc.
    if sync_state.sync_vault || sync_state.sync_recovery_key {
        match cloud_sync::do_zt_blob_push(
            client, UPDATE_SERVER, auth_token, sync_state,
            &local_items, build_attest, profile_id, device_id,
        ).await {
            Ok(zt_result) => {
                log::debug!("[Updater] ZT blob push: {} item(s) pushed", zt_result.pushed);
                for (sync_id, checksum) in &zt_result.id_to_checksum {
                    sync_state.last_synced_checksums.insert(sync_id.clone(), checksum.clone());
                    sync_state.synced_items.insert(sync_id.clone());
                }
                sync_checksums_tx.send_replace(sync_state.last_synced_checksums.clone());
            }
            Err(e) => {
                // Non-fatal: blob push failure does not invalidate cloud sync result.
                log::warn!("[Updater] ZT blob push failed: {}", e);
            }
        }
    }
}

async fn do_check_and_telemetry(
    status_tx: &watch::Sender<UpdateStatus>,
    current_version: &str,
    auth_header: Option<&str>,
    telemetry_source: &Arc<dyn TelemetrySource>,
    telemetry_enabled: bool,
) {
    // Send telemetry (fire-and-forget, don't block update check).
    if telemetry_enabled {
        let payload = telemetry_source.collect();
        let auth_clone = auth_header.map(String::from);

        let heartbeat = telemetry::HeartbeatPayload {
            device_id: payload.device_id.clone(),
            app_version: payload.app_version.clone(),
            uptime_seconds: payload.uptime_secs,
            os: payload.os.clone(),
            device_name: hostname::get()
                .ok()
                .and_then(|h| h.into_string().ok())
                .unwrap_or_default(),
        };

        tokio::spawn(async move {
            if let Err(e) = telemetry::send_telemetry(&payload, auth_clone.as_deref()).await {
                log::warn!("Telemetry send failed: {}", e);
            }
        });

        tokio::spawn(async move {
            if let Err(e) = telemetry::send_heartbeat(&heartbeat).await {
                log::warn!("Heartbeat send failed: {}", e);
            }
        });
    } else {
        log::debug!("[Updater] Telemetry disabled — skipping heartbeat and metrics send");
    }

    // Check for updates.
    let _ = status_tx.send(UpdateStatus::Checking);
    match check::fetch_latest(auth_header).await {
        Ok(manifest) => {
            if check::is_newer(current_version, &manifest.version) {
                eprintln!("[{} Updater] OTA: update available v{} → v{}", now_ts(), current_version, manifest.version);
                let info = UpdateInfo {
                    version: manifest.version,
                    sha256: manifest.sha256,
                    download_url: manifest.download_url,
                    release_notes: manifest.release_notes,
                    file_size: manifest.file_size,
                    signature: manifest.signature,
                };
                let _ = status_tx.send(UpdateStatus::UpdateAvailable(info));
            } else {
                eprintln!("[{} Updater] OTA: up to date (local v{}, server v{})", now_ts(), current_version, manifest.version);
                let _ = status_tx.send(UpdateStatus::Idle);
            }
        }
        Err(e) => {
            eprintln!("[{} Updater] OTA check failed: {}", now_ts(), e);
            log::warn!("Update check failed: {}", e);
            let _ = status_tx.send(UpdateStatus::Idle);
        }
    }
}

async fn do_install(
    status_tx: &watch::Sender<UpdateStatus>,
    info: &state::UpdateInfo,
    server_port: u16,
) {
    // Rollback / downgrade protection — defense-in-depth on top of check::is_newer().
    // This catches any path that bypasses the check (e.g. ManualInstall command).
    let current_version = env!("CARGO_PKG_VERSION");
    if verify::is_downgrade(current_version, &info.version) {
        log::warn!(
            "[Updater] Rejecting update v{}: would downgrade from current v{}",
            info.version, current_version
        );
        let _ = status_tx.send(UpdateStatus::Error(format!(
            "Update v{} rejected: would downgrade from v{}",
            info.version, current_version
        )));
        return;
    }

    // Download
    let _ = status_tx.send(UpdateStatus::Downloading { percent: 0 });
    let progress_tx = status_tx.clone();
    let on_progress = move |pct: u8| {
        let _ = progress_tx.send(UpdateStatus::Downloading { percent: pct });
    };

    match download::download_and_verify(&info.download_url, &info.sha256, on_progress).await {
        Ok(binary_data) => {
            // Verify Ed25519 signature BEFORE writing anything to disk.
            // The signature covers the identical bytes that SHA-256 was computed over.
            let _ = status_tx.send(UpdateStatus::Verifying);
            let sig_str = info.signature.as_deref().unwrap_or("");
            match verify::verify_binary_signature(&binary_data, sig_str) {
                verify::VerifyResult::Valid => {
                    log::info!("[Updater] Signature verified OK for v{}", info.version);
                }
                verify::VerifyResult::Unsigned => {
                    // Transition period: unsigned releases are warned but allowed.
                    // TODO: After all releases are signed, change this branch to reject.
                    log::warn!(
                        "[Updater] Update v{} has no signature — installing during transition period",
                        info.version
                    );
                }
                verify::VerifyResult::Invalid(reason) => {
                    log::error!(
                        "[Updater] SECURITY: Signature verification FAILED for v{}: {}",
                        info.version, reason
                    );
                    let _ = status_tx.send(UpdateStatus::Error(format!(
                        "Update v{} rejected: invalid signature. {}",
                        info.version, reason
                    )));
                    return; // DO NOT install
                }
                verify::VerifyResult::FormatError(reason) => {
                    log::error!(
                        "[Updater] SECURITY: Signature format error for v{}: {}",
                        info.version, reason
                    );
                    let _ = status_tx.send(UpdateStatus::Error(format!(
                        "Update v{} rejected: signature format error. {}",
                        info.version, reason
                    )));
                    return; // DO NOT install
                }
            }

            // Apply update
            let _ = status_tx.send(UpdateStatus::Installing);
            match replace::self_replace(&binary_data) {
                Ok(()) => {
                    // Signal main thread to do graceful shutdown + spawn new process.
                    // Main thread will call replace::spawn_new() then event_loop.exit().
                    let _ = status_tx.send(UpdateStatus::RestartNow { server_port });
                    // Do NOT call process::exit() here — let main thread handle it.
                }
                Err(e) => {
                    let _ = status_tx.send(UpdateStatus::Error(format!("Install failed: {}", e)));
                }
            }
        }
        Err(e) => {
            let _ = status_tx.send(UpdateStatus::Error(format!("Download failed: {}", e)));
        }
    }
}
