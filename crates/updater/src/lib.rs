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

pub use state::{UpdaterHandle, UpdaterCommand, UpdateStatus, UpdateInfo};

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
pub fn start(
    runtime: &tokio::runtime::Handle,
    telemetry_source: Arc<dyn TelemetrySource>,
) -> UpdaterHandle {
    let (status_tx, status_rx) = watch::channel(UpdateStatus::Idle);
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

    let handle = UpdaterHandle {
        status_rx,
        cmd_tx,
    };

    runtime.spawn(updater_loop(status_tx, cmd_rx, telemetry_source));

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
    mut cmd_rx: mpsc::UnboundedReceiver<state::UpdaterCommand>,
    telemetry_source: Arc<dyn TelemetrySource>,
) {
    let current_version = env!("CARGO_PKG_VERSION");
    let token = token_store::load_token();
    let auth_header = token.as_ref().map(|t| format!("Bearer {}", t.token));

    // Initial check on startup (with small delay to let the app initialize).
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    let mut check_interval = tokio::time::interval(CHECK_INTERVAL);
    check_interval.tick().await; // consume first immediate tick

    // Do initial check + telemetry
    do_check_and_telemetry(&status_tx, current_version, auth_header.as_deref(), &telemetry_source).await;

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
                let auth = token_store::load_token().map(|t| format!("Bearer {}", t.token));
                do_check_and_telemetry(&status_tx, current_version, auth.as_deref(), &telemetry_source).await;

                // If we found an update, cache it and auto-install.
                if let UpdateStatus::UpdateAvailable(info) = &*status_tx.borrow() {
                    pending_update = Some(info.clone());
                }
                if let Some(ref info) = pending_update {
                    do_install(&status_tx, info).await;
                }
            }
            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    state::UpdaterCommand::ForceCheck => {
                        let auth = token_store::load_token().map(|t| format!("Bearer {}", t.token));
                        do_check_and_telemetry(&status_tx, current_version, auth.as_deref(), &telemetry_source).await;
                        if let UpdateStatus::UpdateAvailable(info) = &*status_tx.borrow() {
                            pending_update = Some(info.clone());
                        }
                        // Auto-install on forced check as well.
                        if let Some(ref info) = pending_update {
                            do_install(&status_tx, info).await;
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
                        match oauth::start_oauth_flow(&provider).await {
                            Ok(token) => {
                                let _ = token_store::save_token(&token);
                                log::info!("OAuth successful: {} ({})", token.display_name, token.provider);
                            }
                            Err(e) => {
                                log::error!("OAuth failed: {}", e);
                            }
                        }
                    }
                    state::UpdaterCommand::Logout => {
                        token_store::clear_token();
                        log::info!("Logged out");
                    }
                }
            }
        }
    }
}

async fn do_check_and_telemetry(
    status_tx: &watch::Sender<UpdateStatus>,
    current_version: &str,
    auth_header: Option<&str>,
    telemetry_source: &Arc<dyn TelemetrySource>,
) {
    // Send telemetry (fire-and-forget, don't block update check).
    let payload = telemetry_source.collect();
    let auth_clone = auth_header.map(String::from);
    tokio::spawn(async move {
        if let Err(e) = telemetry::send_telemetry(&payload, auth_clone.as_deref()).await {
            log::warn!("Telemetry send failed: {}", e);
        }
    });

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
    // Download
    let _ = status_tx.send(UpdateStatus::Downloading { percent: 0 });
    let progress_tx = status_tx.clone();
    let on_progress = move |pct: u8| {
        let _ = progress_tx.send(UpdateStatus::Downloading { percent: pct });
    };

    match download::download_and_verify(&info.download_url, &info.sha256, on_progress).await {
        Ok(binary_data) => {
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
