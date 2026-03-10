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
pub mod key_sync;
pub mod cloud_sync;
pub mod e2e_crypto;
pub mod verify;

pub use state::{UpdaterHandle, UpdaterCommand, UpdateStatus, UpdateInfo, AuthStatus, SyncStatus};

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
/// is generated until a `SetConnectedMode(true)` command arrives.
///
/// `telemetry_enabled` controls whether anonymized metrics are sent.  Can be
/// toggled at runtime via [`UpdaterCommand::SetTelemetryEnabled`].
///
/// `sync_enabled` seeds the initial cloud-sync enabled state from the user
/// profile.  Can be toggled at runtime via [`UpdaterCommand::SetSyncEnabled`].
pub fn start(
    runtime: &tokio::runtime::Handle,
    telemetry_source: Arc<dyn TelemetrySource>,
    connected: bool,
    telemetry_enabled: bool,
    sync_enabled: bool,
    data_dir: std::path::PathBuf,
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

    // Channel for server-synced API key hashes (Connected mode only).
    let (synced_keys_tx, synced_keys_rx) = watch::channel(Vec::<key_sync::SyncedKeyEntry>::new());

    // Channel for cloud sync status — starts Idle, updated by the updater loop.
    let (sync_status_tx, sync_status_rx) = watch::channel(state::SyncStatus::Idle);

    let handle = UpdaterHandle {
        status_rx,
        cmd_tx,
        auth_rx,
        synced_keys_rx,
        sync_status_rx,
    };

    runtime.spawn(updater_loop(status_tx, auth_tx, synced_keys_tx, sync_status_tx, cmd_rx, telemetry_source, connected, telemetry_enabled, sync_enabled, data_dir));

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
    synced_keys_tx: watch::Sender<Vec<key_sync::SyncedKeyEntry>>,
    sync_status_tx: watch::Sender<state::SyncStatus>,
    mut cmd_rx: mpsc::UnboundedReceiver<state::UpdaterCommand>,
    telemetry_source: Arc<dyn TelemetrySource>,
    mut connected: bool,
    mut telemetry_enabled: bool,
    sync_enabled_init: bool,
    data_dir: std::path::PathBuf,
) {
    let current_version = env!("CARGO_PKG_VERSION");

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
    let mut sync_state = cloud_sync::SyncState {
        enabled: sync_enabled_init,
        ..cloud_sync::SyncState::default()
    };

    // Initial check on startup (with small delay to let the app initialize).
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    let mut check_interval = tokio::time::interval(CHECK_INTERVAL);
    check_interval.tick().await; // consume first immediate tick

    // Do initial check + telemetry only in connected mode.
    if connected {
        let token = token_store::load_token();
        let auth_header = token.as_ref().map(|t| format!("Bearer {}", t.token));
        do_check_and_telemetry(&status_tx, current_version, auth_header.as_deref(), &telemetry_source, telemetry_enabled).await;
        // Initial key sync.
        if let Some(ref td) = token {
            do_key_sync(&http_client, &td.token, &synced_keys_tx).await;
        }
    } else {
        log::info!("[Updater] Standalone mode — skipping initial update check and telemetry");
    }

    let mut pending_update: Option<state::UpdateInfo> = None;

    // Auto-install if an update was found on startup.
    if let UpdateStatus::UpdateAvailable(info) = &*status_tx.borrow() {
        pending_update = Some(info.clone());
    }
    if let Some(ref info) = pending_update {
        do_install(&status_tx, info).await;
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
                        do_install(&status_tx, info).await;
                    }

                    // Key sync — best-effort, logged but not fatal.
                    if let Some(ref td) = token {
                        do_key_sync(&http_client, &td.token, &synced_keys_tx).await;
                    }

                    // Cloud sync — best-effort, only runs if user opted in.
                    if let Some(ref td) = token {
                        if sync_state.enabled {
                            do_cloud_sync(&http_client, &td.token, &sync_status_tx, &mut sync_state, &data_dir).await;
                        }
                    }
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
                                do_install(&status_tx, info).await;
                            }
                            // Also sync keys on forced check.
                            if let Some(ref td) = token {
                                do_key_sync(&http_client, &td.token, &synced_keys_tx).await;
                            }
                        } else {
                            log::warn!("[Updater] ForceCheck ignored — running in standalone mode");
                        }
                    }
                    state::UpdaterCommand::InstallNow => {
                        if let Some(ref info) = pending_update {
                            do_install(&status_tx, info).await;
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
                                    do_cloud_sync(&http_client, &td.token, &sync_status_tx, &mut sync_state, &data_dir).await;
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
                    state::UpdaterCommand::SetConnectedMode(new_mode) => {
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
                                do_install(&status_tx, info).await;
                            }
                            // Sync keys immediately after switching to connected.
                            if let Some(ref td) = token {
                                do_key_sync(&http_client, &td.token, &synced_keys_tx).await;
                            }
                            // Cloud sync immediately after switching to connected.
                            if let Some(ref td) = token {
                                if sync_state.enabled {
                                    do_cloud_sync(&http_client, &td.token, &sync_status_tx, &mut sync_state, &data_dir).await;
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

/// Run one cloud sync cycle and update the status watch channel.
///
/// Best-effort: a sync failure is logged and broadcast but never fatal.
/// The sync state is updated in-place on success so the next cycle is
/// incremental (only items changed since the last run are fetched).
async fn do_cloud_sync(
    client: &reqwest::Client,
    auth_token: &str,
    sync_status_tx: &watch::Sender<state::SyncStatus>,
    sync_state: &mut cloud_sync::SyncState,
    data_dir: &std::path::Path,
) {
    sync_status_tx.send_replace(state::SyncStatus::Syncing);

    match cloud_sync::do_sync_cycle(client, UPDATE_SERVER, auth_token, sync_state, data_dir).await {
        Ok((pushed, pulled, new_state)) => {
            log::debug!("[Updater] Cloud sync: pushed={} pulled={}", pushed, pulled);
            *sync_state = new_state;
            sync_status_tx.send_replace(state::SyncStatus::Completed { pushed, pulled });
        }
        Err(e) => {
            log::warn!("[Updater] Cloud sync failed: {}", e);
            sync_status_tx.send_replace(state::SyncStatus::Error(e));
        }
    }
}

/// Fetch key hashes from the server and broadcast them via the watch channel.
///
/// Best-effort: logs warnings on failure but never panics and never modifies
/// the local key registry directly — that happens in the main thread when it
/// drains the watch channel.
async fn do_key_sync(
    client: &reqwest::Client,
    auth_token: &str,
    synced_keys_tx: &watch::Sender<Vec<key_sync::SyncedKeyEntry>>,
) {
    match key_sync::fetch_key_hashes(client, auth_token).await {
        Ok(keys) => {
            let count = keys.len();
            let _ = synced_keys_tx.send(keys);
            log::debug!("[Updater] Key sync: {} key(s) received from server", count);
        }
        Err(e) => {
            log::warn!("[Updater] Key sync failed: {}", e);
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
                let _ = status_tx.send(UpdateStatus::Idle);
            }
        }
        Err(e) => {
            log::warn!("Update check failed: {}", e);
            let _ = status_tx.send(UpdateStatus::Idle);
        }
    }
}

async fn do_install(
    status_tx: &watch::Sender<UpdateStatus>,
    info: &state::UpdateInfo,
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
                    let _ = status_tx.send(UpdateStatus::RestartPending);
                    // Spawn new process and exit
                    if let Err(e) = replace::spawn_and_exit() {
                        let _ = status_tx.send(UpdateStatus::Error(format!("Restart failed: {}", e)));
                    }
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
