//! mylittlechart-updater — OTA update client, telemetry, and OAuth for the chart application.
//!
//! Background task that checks for updates, sends anonymized metrics,
//! and manages user authentication via OAuth providers.

pub mod state;
pub mod platform;
pub mod token_store;
pub mod check;
pub mod device_auth;
pub mod download;
pub mod replace;
pub mod telemetry;
pub mod oauth;
pub mod key_sync;  // kept as stub — cloud sync of local agent keys was removed
pub mod cloud_sync;
pub mod vault_params;
pub mod verify;
pub mod attest;
pub mod entitlements;
pub mod redeem;
pub mod auth_probe;

pub use state::{UpdaterHandle, UpdaterCommand, UpdateStatus, UpdateInfo, AuthStatus, SyncStatus, BuildAttestation, SyncConflict, ConflictResolution, Entitlements, RedeemState};

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

/// Current wall-clock time as milliseconds since the Unix epoch — the same
/// clock the server's entitlements `expires_at` is stamped in.
fn now_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

use tokio::sync::{mpsc, watch};
use std::sync::Arc;

/// Base URL for the update server.
const UPDATE_SERVER: &str = "https://chart.example";

/// The release track this client asks the server for.
///
/// There is one: `"stable"`. The `"dev"` track is gone (owner, 2026-09-03:
/// "никакого дев больше нет"), and with it the `UPDATE_CHANNEL` override —
/// a client that can put itself on a different track is a client whose
/// behaviour nobody can predict from the outside, and the one thing the dev
/// track actually changed was how hard it hammered the server.
///
/// Kept as a function rather than a constant because the manifest query and
/// the release registration both name the track, and the day a second one
/// exists it will be decided here.
pub fn get_channel() -> String {
    "stable".to_string()
}

/// How often to ask the server whether a newer build exists: four hours,
/// unconditionally.
///
/// This used to be two minutes on the `dev` track — a factor of 120 between
/// two machines running the same binary, which is how one development machine
/// came to produce 26 992 of production's 27 085 telemetry rows back when the
/// heartbeat rode this same tick. Neither the interval nor the track is
/// configurable now.
const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(4 * 60 * 60);

/// How often telemetry and the heartbeat are sent — the same on every channel,
/// and deliberately not derived from [`CHECK_INTERVAL`].
///
/// Both used to ride the update-check tick. That coupling is why one
/// development machine produced 26 992 of the 27 085 telemetry rows in
/// production: on `dev` the check fires every two minutes, elsewhere every
/// four hours, a factor of 120 between two machines running the same build.
/// How often we look for a new version has nothing to do with how often we
/// want a metric.
///
/// Not configurable, and that is the point — a knob here is exactly what the
/// dev channel already was. The server enforces its own per-device daily cap
/// regardless of what any client decides to send.
const TELEMETRY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// How often the loop wakes to check whether an entitlements re-ask is due.
///
/// The wake itself is cheap — a timestamp compare against
/// `next_entitlements_at_ms` — the `GET /api/entitlements` call only fires
/// once that deadline has passed (see [`do_entitlements_check`]).
/// Deliberately its own clock rather than riding `check_interval` or
/// [`TELEMETRY_INTERVAL`]: same reasoning as that constant's own doc —
/// what one tick is for has nothing to do with another.
const ENTITLEMENTS_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

/// Backoff before retrying a failed entitlements fetch — mirrors
/// `mlc-app-shell-web`'s `cloud_session::ENTITLEMENTS_RETRY_MS`; see that
/// module's doc for the reasoning (self-heal a network blip without
/// hammering a real outage).
const ENTITLEMENTS_RETRY_SECS: u64 = 30;

/// Floor on the re-ask interval even if the server's own `expires_at`
/// comes back unreasonably soon (or already in the past) — mirrors
/// `cloud_session::ENTITLEMENTS_MIN_REASK_MS`. The endpoint answers the
/// same caller identity on every call, so there is nothing to gain from
/// asking more often than this regardless of what the server said.
const ENTITLEMENTS_MIN_REASK_SECS: u64 = 60;

/// How often `StartOAuth` polls `GET /api/auth/poll` once the code has been
/// minted and the browser opened.
const DEVICE_AUTH_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(2500);

/// How long `StartOAuth` keeps polling before giving up on a code nobody
/// confirmed. Deliberately shorter than the server's own code TTL
/// (`DeviceCodeResponse::expires_in`, 10 minutes) — a user who never
/// finishes signing in should not leave this command in flight for the rest
/// of that window.
const DEVICE_AUTH_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5 * 60);

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
/// Cloud sync is not a parameter: it follows the account, seeded from the
/// token store and flipped by sign-in and sign-out (owner's ruling
/// 2026-09-02 — no OAuth, no cloud; OAuth, cloud; paid, more room).
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

    // Channel for entitlements (feature gates) — starts at the free
    // default and only ever advances via a successful fetch; see
    // `Entitlements`'s own doc for why a failed fetch must never publish
    // anything else.
    let (entitlements_tx, entitlements_rx) = watch::channel(state::Entitlements::default());

    // Channel for the outcome of the most recent redeem-code attempt —
    // starts Idle, republished on every `RedeemCode` command.
    let (redeem_tx, redeem_rx) = watch::channel(state::RedeemState::Idle);

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
        entitlements_rx,
        redeem_rx,
        sync_status_rx,
        sync_checksums_rx,
    };

    runtime.spawn(updater_loop(status_tx, auth_tx, entitlements_tx, redeem_tx, sync_status_tx, sync_checksums_tx, cmd_rx, telemetry_source, connected, initial_synced_items, initial_last_synced_checksums, data_dir, build_attest, profile_id, server_port));

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
    entitlements_tx: watch::Sender<state::Entitlements>,
    redeem_tx: watch::Sender<state::RedeemState>,
    sync_status_tx: watch::Sender<state::SyncStatus>,
    sync_checksums_tx: watch::Sender<std::collections::HashMap<String, String>>,
    mut cmd_rx: mpsc::UnboundedReceiver<state::UpdaterCommand>,
    telemetry_source: Arc<dyn TelemetrySource>,
    mut connected: bool,
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
    let channel = get_channel();
    let interval_secs = CHECK_INTERVAL.as_secs();
    eprintln!("[{} Updater] Starting — current version: v{}, device_id: {}", now_ts(), current_version, device_id);
    log::info!("[Updater] channel={}, interval={}s", &channel, interval_secs);

    // Shared HTTP client for key sync requests.
    // Built once here; reused on every interval tick.
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    // This build started, so the copy of the previous one kept for rollback
    // has done its job and is only occupying disk now.
    replace::clear_rollback_copy();

    // A stored token is a claim, not a fact. Confirm it before the rest of
    // this process treats the user as signed in — an expired or revoked token
    // otherwise shows an account name in the chrome, offers Sign out and
    // Redeem, and refuses every call behind them.
    {
        if let Some(t) = token_store::load_token() {
            let header = format!("Bearer {}", t.token);
            match auth_probe::probe(&http_client, &header).await {
                auth_probe::Verdict::Rejected => {
                    token_store::clear_token();
                    let _ = auth_tx.send(state::AuthStatus::NotLoggedIn);
                    eprintln!(
                        "[{} Updater] stored token rejected by the server — signed out",
                        now_ts()
                    );
                }
                auth_probe::Verdict::Valid => {}
                // Offline, or the server is unwell. Says nothing about the
                // token, so it changes nothing.
                auth_probe::Verdict::Unknown => {}
            }
        }
    }

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
    // Cloud sync follows the ACCOUNT and nothing else (owner's ruling
    // 2026-09-02): signed in means synced, signed out means not, a paid
    // account means more room on the server. There is no user-facing switch
    // and there should not be — the one that existed was for testing, and a
    // caller-supplied boolean is how it survived: the native shell passed a
    // hardcoded `false` with a comment about build attestation that had
    // stopped being true.
    //
    // Seeded here from whatever token is already on disk, which is the same
    // source `initial_auth` above reads, so the two cannot start out
    // disagreeing.
    let sync_enabled_init = token_store::load_token().is_some();
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

    // Telemetry runs on its own clock — see TELEMETRY_INTERVAL.
    let mut telemetry_interval = tokio::time::interval(TELEMETRY_INTERVAL);
    telemetry_interval.tick().await; // consume first immediate tick

    // Entitlements re-ask also runs on its own clock — see
    // ENTITLEMENTS_POLL_INTERVAL. `next_entitlements_at_ms = 0` means
    // "due immediately", which is how the very first fetch (below, in the
    // initial connected-mode check) gets scheduled.
    let mut entitlements_poll_interval = tokio::time::interval(ENTITLEMENTS_POLL_INTERVAL);
    entitlements_poll_interval.tick().await; // consume first immediate tick
    let mut next_entitlements_at_ms: i64 = 0;

    // Do initial check + telemetry + entitlements only in connected mode —
    // standalone mode makes no HTTP calls at all until SetCloudEnabled(true).
    if connected {
        let token = token_store::load_token();
        let auth_header = token.as_ref().map(|t| format!("Bearer {}", t.token));
        do_check(&status_tx, current_version, auth_header.as_deref(), &channel).await;
        send_telemetry_sample(auth_header.as_deref(), &telemetry_source);
        next_entitlements_at_ms = do_entitlements_check(&http_client, &entitlements_tx, auth_header.as_deref()).await;
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
            _ = telemetry_interval.tick() => {
                if connected {
                    let token = token_store::load_token();
                    let auth = token.as_ref().map(|t| format!("Bearer {}", t.token));
                    send_telemetry_sample(auth.as_deref(), &telemetry_source);
                }
            }

            _ = entitlements_poll_interval.tick() => {
                if connected && now_unix_ms() >= next_entitlements_at_ms {
                    let token = token_store::load_token();
                    let auth = token.as_ref().map(|t| format!("Bearer {}", t.token));
                    next_entitlements_at_ms = do_entitlements_check(&http_client, &entitlements_tx, auth.as_deref()).await;
                }
                // In standalone mode, or before the deadline: tick fires but we do nothing.
            }

            _ = check_interval.tick() => {
                if connected {
                    let token = token_store::load_token();
                    let auth = token.as_ref().map(|t| format!("Bearer {}", t.token));
                    do_check(&status_tx, current_version, auth.as_deref(), &channel).await;

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
                            do_check(&status_tx, current_version, auth.as_deref(), &channel).await;
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
                        match do_device_oauth(&http_client, &provider, &device_id).await {
                            Some(token) => {
                                let _ = token_store::save_token(&token);
                                log::info!("OAuth successful: {} ({})", token.display_name, token.provider);
                                // Signing in IS enabling cloud sync.
                                sync_state.enabled = true;
                                let _ = auth_tx.send(state::AuthStatus::LoggedIn {
                                    display_name: token.display_name,
                                    provider: token.provider,
                                    user_id: token.user_id,
                                });
                            }
                            None => {
                                // do_device_oauth already logged why (mint
                                // failure, browser-open failure, expiry, or
                                // timeout) — nothing more to say here.
                                let _ = auth_tx.send(state::AuthStatus::NotLoggedIn);
                            }
                        }
                    }
                    state::UpdaterCommand::Logout => {
                        token_store::clear_token();
                        // ...and signing out IS disabling it. No account, no
                        // cloud; the two are one fact, not two settings.
                        sync_state.enabled = false;
                        let _ = auth_tx.send(state::AuthStatus::NotLoggedIn);
                        log::info!("Logged out");
                    }
                    state::UpdaterCommand::RedeemCode(code) => {
                        let token = token_store::load_token();
                        let auth_header = token.as_ref().map(|t| format!("Bearer {}", t.token));
                        match auth_header {
                            None => {
                                let _ = redeem_tx.send(state::RedeemState::Failed {
                                    message: "Sign in first".to_string(),
                                });
                                log::warn!("[Updater] redeem failed: not signed in");
                            }
                            Some(auth) => {
                                let _ = redeem_tx.send(state::RedeemState::Working);
                                match redeem::redeem(&http_client, &auth, &code).await {
                                    Ok(paid_until) => {
                                        log::info!("[Updater] redeem: ok until {}", paid_until);
                                        let _ = redeem_tx.send(state::RedeemState::Redeemed {
                                            paid_until: paid_until.clone(),
                                        });
                                        // Refresh entitlements immediately so the new
                                        // paid tier is visible without waiting out the
                                        // poll interval — the whole point of redeeming
                                        // is that the app changes tier now.
                                        next_entitlements_at_ms = do_entitlements_check(&http_client, &entitlements_tx, Some(&auth)).await;
                                    }
                                    Err(e) => {
                                        log::warn!("[Updater] redeem failed: {}", e);
                                        // A 401 here is the boot probe's answer
                                        // arriving late — the session died while
                                        // the app was running. Same conclusion,
                                        // so the same demotion, or the user is
                                        // left retyping a good code at a dead
                                        // session forever.
                                        if matches!(
                                            auth_probe::probe(&http_client, &auth).await,
                                            auth_probe::Verdict::Rejected
                                        ) {
                                            token_store::clear_token();
                                            let _ = auth_tx.send(state::AuthStatus::NotLoggedIn);
                                            eprintln!(
                                                "[{} Updater] session rejected during redeem — signed out",
                                                now_ts()
                                            );
                                        }
                                        let _ = redeem_tx.send(state::RedeemState::Failed { message: e });
                                    }
                                }
                            }
                        }
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
                            do_check(&status_tx, current_version, auth.as_deref(), &channel).await;
                            if let UpdateStatus::UpdateAvailable(info) = &*status_tx.borrow() {
                                pending_update = Some(info.clone());
                            }
                            if let Some(ref info) = pending_update {
                                do_install(&status_tx, info, server_port).await;
                            }
                            // Entitlements immediately after switching to connected —
                            // standalone mode made no HTTP calls, so this may be the
                            // first opportunity to ask.
                            next_entitlements_at_ms = do_entitlements_check(&http_client, &entitlements_tx, auth.as_deref()).await;
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
                    // sync_vault and sync_recovery_key are always true (Cloud = always ZT)
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
                    state::UpdaterCommand::ListCloudProfiles => {
                        // Account-level operation — works regardless of OTA/sync
                        // mode. Profile manager needs this even when the active
                        // profile is "local".  Only requires a valid OAuth token.
                        let token = token_store::load_token();
                        match token {
                            None => {
                                sync_status_tx.send_replace(state::SyncStatus::CloudProfilesError(
                                    "Not logged in".to_string(),
                                ));
                            }
                            Some(ref td) => {
                                match cloud_sync::list_cloud_profiles(
                                    &http_client,
                                    UPDATE_SERVER,
                                    &td.token,
                                    &build_attest,
                                    &device_id,
                                )
                                .await
                                {
                                    Ok(profiles) => {
                                        eprintln!(
                                            "[{} Updater] ListCloudProfiles: {} profile(s) found",
                                            now_ts(),
                                            profiles.len()
                                        );
                                        sync_status_tx.send_replace(
                                            state::SyncStatus::CloudProfilesLoaded(profiles),
                                        );
                                    }
                                    Err(e) => {
                                        log::warn!("[Updater] ListCloudProfiles failed: {}", e);
                                        sync_status_tx.send_replace(
                                            state::SyncStatus::CloudProfilesError(e),
                                        );
                                    }
                                }
                            }
                        }
                    }
                    state::UpdaterCommand::RestoreCloudProfile { profile_id: restore_id } => {
                        // Account-level — same as ListCloudProfiles: works
                        // regardless of OTA mode, only needs OAuth token.
                        let token = token_store::load_token();
                        match token {
                            None => {
                                sync_status_tx.send_replace(
                                    state::SyncStatus::ProfileRestoreError(
                                        "Not logged in".to_string(),
                                    ),
                                );
                            }
                            Some(ref td) => {
                                eprintln!(
                                    "[{} Updater] RestoreCloudProfile: downloading profile '{}'",
                                    now_ts(),
                                    restore_id
                                );
                                sync_status_tx.send_replace(state::SyncStatus::Syncing);

                                // Step 1: pull all items for the target profile.
                                let items = cloud_sync::pull_profile(
                                    &http_client,
                                    UPDATE_SERVER,
                                    &td.token,
                                    &build_attest,
                                    &restore_id,
                                    &device_id,
                                )
                                .await;

                                let items = match items {
                                    Ok(v) => v,
                                    Err(e) => {
                                        log::warn!(
                                            "[Updater] RestoreCloudProfile pull failed: {}",
                                            e
                                        );
                                        sync_status_tx.send_replace(
                                            state::SyncStatus::ProfileRestoreError(e),
                                        );
                                        continue;
                                    }
                                };

                                // Step 2: write items to disk inside the profile dir.
                                match cloud_sync::restore_profile_to_disk(&items, &restore_id) {
                                    Err(e) => {
                                        log::warn!(
                                            "[Updater] RestoreCloudProfile write failed: {}",
                                            e
                                        );
                                        sync_status_tx.send_replace(
                                            state::SyncStatus::ProfileRestoreError(e),
                                        );
                                    }
                                    Ok(profile_dir) => {
                                        // Step 3: register the profile in the local index.
                                        let dir_name = restore_id.clone();
                                        let index =
                                            mlc_core::load_profile_index();
                                        let mut index = index.unwrap_or(
                                            mlc_core::ProfileIndex {
                                                active_profile_id: restore_id.clone(),
                                                profiles: Vec::new(),
                                            },
                                        );
                                        // Only add if not already present.
                                        if !index.profiles.iter().any(|m| m.id == restore_id) {
                                            // Derive a display name from the profile.json if
                                            // available; fall back to the UUID.
                                            let display_name = {
                                                let pjson = profile_dir.join("profile.json");
                                                std::fs::read_to_string(&pjson)
                                                    .ok()
                                                    .and_then(|s| {
                                                        serde_json::from_str::<serde_json::Value>(
                                                            &s,
                                                        )
                                                        .ok()
                                                    })
                                                    .and_then(|v| {
                                                        v.get("display_name")
                                                            .and_then(|n| n.as_str())
                                                            .map(|s| s.to_string())
                                                    })
                                                    .unwrap_or_else(|| {
                                                        format!("Restored ({})", &restore_id[..8.min(restore_id.len())])
                                                    })
                                            };
                                            let avatar = {
                                                let pjson = profile_dir.join("profile.json");
                                                std::fs::read_to_string(&pjson)
                                                    .ok()
                                                    .and_then(|s| {
                                                        serde_json::from_str::<serde_json::Value>(
                                                            &s,
                                                        )
                                                        .ok()
                                                    })
                                                    .and_then(|v| {
                                                        v.get("avatar")
                                                            .and_then(|n| n.as_str())
                                                            .map(|s| s.to_string())
                                                    })
                                                    .unwrap_or_else(|| "chart".to_string())
                                            };
                                            let now_secs = std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_secs()
                                                as i64;
                                            index.profiles.push(mlc_core::ProfileMeta {
                                                id: restore_id.clone(),
                                                display_name,
                                                avatar,
                                                created_at: now_secs,
                                                dir_name,
                                                // A profile restored from the cloud is
                                                // cloud-enabled by definition (pre-strip
                                                // behaviour; fields restored in mlc-core).
                                                cloud_enabled: true,
                                                sync_level: "cloud".to_string(),
                                            });
                                        }
                                        if let Err(e) =
                                            mlc_core::save_profile_index(&index)
                                        {
                                            log::warn!(
                                                "[Updater] RestoreCloudProfile: failed to save index: {}",
                                                e
                                            );
                                            // Non-fatal — the files are on disk; index update
                                            // will be retried on the next profile scan.
                                        }

                                        eprintln!(
                                            "[{} Updater] RestoreCloudProfile: profile '{}' restored ({} item(s))",
                                            now_ts(),
                                            restore_id,
                                            items.len()
                                        );
                                        sync_status_tx.send_replace(
                                            state::SyncStatus::ProfileRestored {
                                                profile_id: restore_id.clone(),
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }
                    state::UpdaterCommand::SetChannel(new_channel) => {
                        // There is one track and it is not switchable. Kept as
                        // a command so an old caller does not fail to compile,
                        // and loud so a caller that still sends it is found
                        // rather than silently ignored.
                        if new_channel != channel {
                            eprintln!(
                                "[{} Updater] SetChannel({}) refused — one track only, staying on '{}'",
                                now_ts(), new_channel, channel
                            );
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

    // Step 3: ZT blob push — vault.enc, recovery_key.enc (always for Cloud).
    {
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

/// Send one telemetry sample and one heartbeat, fire-and-forget.
///
/// Driven by [`TELEMETRY_INTERVAL`], never by the update-check tick — see that
/// constant for why the two were separated.
fn send_telemetry_sample(auth_header: Option<&str>, telemetry_source: &Arc<dyn TelemetrySource>) {
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
}

/// Check for a newer build on the current channel. Carries no telemetry —
/// that moved to [`send_telemetry_sample`] on its own schedule.
async fn do_check(
    status_tx: &watch::Sender<UpdateStatus>,
    current_version: &str,
    auth_header: Option<&str>,
    channel: &str,
) {
    // Check for updates.
    let _ = status_tx.send(UpdateStatus::Checking);
    match check::fetch_latest(auth_header, channel).await {
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

/// Fetch the feature-gate set and publish it — on success only.
///
/// Mirrors the failure contract `cloud_session.rs` settled on for the web
/// shell: any failure (network, non-200, unparseable body) leaves the
/// previously published value untouched — this function simply does not
/// send on `entitlements_tx` in that case — and schedules a retry on a
/// backoff. It must never publish an empty set over a good one, and never
/// publish a paid one it did not receive. If nothing has ever been
/// fetched successfully, the channel stays at [`state::Entitlements::default`].
///
/// Returns the unix-ms deadline after which the next fetch is due.
async fn do_entitlements_check(
    client: &reqwest::Client,
    entitlements_tx: &watch::Sender<state::Entitlements>,
    auth_header: Option<&str>,
) -> i64 {
    match entitlements::fetch(client, auth_header).await {
        Ok(ent) => {
            let expires_at = ent.expires_at;
            let _ = entitlements_tx.send(ent);
            // Floor so a server answer that is already stale (or asks to
            // be re-checked immediately) cannot turn the poll interval
            // into an effective fetch-every-tick loop.
            expires_at.max(now_unix_ms() + (ENTITLEMENTS_MIN_REASK_SECS * 1000) as i64)
        }
        Err(e) => {
            log::warn!("[Updater] entitlements fetch failed: {}", e);
            now_unix_ms() + (ENTITLEMENTS_RETRY_SECS * 1000) as i64
        }
    }
}

/// Run the device-code sign-in flow for `provider`: mint a code, open the
/// browser at its authorize URL, then poll [`device_auth::poll`] every
/// [`DEVICE_AUTH_POLL_INTERVAL`] until it is confirmed, expires, or
/// [`DEVICE_AUTH_DEADLINE`] passes — whichever comes first.
///
/// A plain sequential `.await` inside the command loop's `select!` arm — the
/// same shape the loopback flow it replaces already had (that one blocked
/// the same loop for up to 120s waiting on its callback listener), so the
/// command loop does not process another command while a sign-in is in
/// flight. Not restructured here; that is a separate decision.
///
/// Never logs the device code or the returned token — only outcomes.
async fn do_device_oauth(
    client: &reqwest::Client,
    provider: &str,
    device_id: &str,
) -> Option<token_store::StoredToken> {
    let code = match device_auth::create(client, device_id).await {
        Ok(c) => c,
        Err(e) => {
            log::error!("[Updater] device-code sign-in: could not mint a code: {}", e);
            return None;
        }
    };

    let url = device_auth::authorize_url(provider, &code);
    if let Err(e) = oauth::open_url(&url) {
        log::error!("[Updater] device-code sign-in: could not open the browser: {}", e);
        return None;
    }
    log::info!("[Updater] device-code sign-in: waiting for {} to confirm in the browser...", provider);

    let poll_loop = async {
        loop {
            tokio::time::sleep(DEVICE_AUTH_POLL_INTERVAL).await;
            match device_auth::poll(client, &code).await {
                Ok(device_auth::PollOutcome::Confirmed(token)) => return Some(token),
                Ok(device_auth::PollOutcome::Pending) => continue,
                Ok(device_auth::PollOutcome::Expired) => {
                    log::warn!("[Updater] device-code sign-in: code expired or was already used");
                    return None;
                }
                Err(e) => {
                    // Transient (network blip, server hiccup) — keep trying
                    // until the outer deadline.
                    log::warn!("[Updater] device-code sign-in: poll failed, retrying: {}", e);
                    continue;
                }
            }
        }
    };

    match tokio::time::timeout(DEVICE_AUTH_DEADLINE, poll_loop).await {
        Ok(outcome) => outcome,
        Err(_) => {
            log::warn!("[Updater] device-code sign-in: timed out after {}s waiting for confirmation", DEVICE_AUTH_DEADLINE.as_secs());
            None
        }
    }
}

async fn do_install(
    status_tx: &watch::Sender<UpdateStatus>,
    info: &state::UpdateInfo,
    _server_port: u16,
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
                    // Signal RestartPending. Nothing restarts from here: the
                    // chrome control widens into "Restart" and waits to be
                    // pressed, and only that press reaches save_all() and
                    // spawn_and_exit() (owner, 2026-09-04).
                    // We must NOT call process::exit() from the async task
                    // because the main thread would never get a chance to
                    // persist profile/agents/presets to disk.
                    let _ = status_tx.send(UpdateStatus::RestartPending);
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
