use tokio::sync::{mpsc, watch};
use serde::{Serialize, Deserialize};

// =============================================================================
// BuildAttestation
// =============================================================================

/// Compile-time build attestation values, set by `chart-app-vello/build.rs`.
///
/// The binary crate reads these from `env!()` macros and passes them into
/// [`crate::start`].  The updater library does not use `env!()` directly
/// because the compile-time constants live in the binary crate's build graph,
/// not the library's.
///
/// Dev builds (no `RELEASE_SIGNING_KEY`) produce an empty `attestation` field,
/// which causes [`crate::attest::attestation_headers`] to return no headers.
#[derive(Clone, Debug, Default)]
pub struct BuildAttestation {
    /// Base64-encoded Ed25519 signature over the canonical message, or empty for dev builds.
    pub attestation: String,
    /// App version string (e.g. `"0.2.8"`).
    pub version: String,
    /// Target platform (e.g. `"windows"`, `"linux"`, `"macos"`).
    pub platform: String,
    /// Unix timestamp (seconds) when this binary was built.
    pub timestamp: String,
}

/// Authentication status — broadcast via a watch channel so the UI can react.
#[derive(Clone, Debug)]
pub enum AuthStatus {
    /// No token on disk, user has not authenticated.
    NotLoggedIn,
    /// User is authenticated.
    LoggedIn {
        display_name: String,
        provider: String,
        user_id: i64,
    },
}

/// Information about an available update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub version: String,
    pub sha256: String,
    pub download_url: String,
    pub release_notes: String,
    pub file_size: u64,
    /// Ed25519 signature (base64-encoded, 88 chars) over the binary bytes.
    /// None = old server that doesn't emit this field.
    /// Some("") = server present but release was not signed.
    /// Some(b64) = signed release — must verify before installing.
    pub signature: Option<String>,
}

/// Current status of the updater.
#[derive(Debug, Clone)]
pub enum UpdateStatus {
    /// Idle — no update activity.
    Idle,
    /// Checking for updates.
    Checking,
    /// An update is available.
    UpdateAvailable(UpdateInfo),
    /// Downloading the update (progress 0-100).
    Downloading { percent: u8 },
    /// Verifying SHA256 hash.
    Verifying,
    /// Applying update (replacing binary).
    Installing,
    /// Ready to restart.
    RestartPending,
    /// Error during update process.
    Error(String),
}

/// Commands sent from the UI thread to the updater background task.
#[derive(Debug)]
pub enum UpdaterCommand {
    /// User clicked "Update Now".
    InstallNow,
    /// User dismissed the update notification.
    DismissUpdate,
    /// Force an immediate check.
    ForceCheck,
    /// Start OAuth flow for a provider.
    StartOAuth(String),
    /// Log out (clear stored token).
    Logout,
    /// Switch connected mode at runtime.
    /// `true` = Connected (enable server communication),
    /// `false` = Standalone (stop all phone-home).
    SetConnectedMode(bool),
    /// Trigger an immediate cloud sync cycle (push local changes, pull remote).
    ///
    /// Ignored in standalone mode.  The updater broadcasts progress via the
    /// `sync_status_rx` watch channel on `UpdaterHandle`.
    ForceSync,
    /// Enable or disable telemetry at runtime.
    ///
    /// When `false`, update checks still run but heartbeat/telemetry payloads
    /// are not sent to the server.
    SetTelemetryEnabled(bool),
    /// Enable or disable cloud sync at runtime.
    ///
    /// Mirrors `UserProfile.sync_state.enabled` so the updater loop does not
    /// need a channel back to main to query the profile on every tick.
    SetSyncEnabled(bool),
    /// Shut down the updater background task cleanly.
    ///
    /// After receiving this command the loop exits; no further network calls
    /// are made.  Send this before the process exits.
    Shutdown,
}

// =============================================================================
// SyncStatus
// =============================================================================

/// Current state of the cloud sync subsystem.
///
/// Broadcast via [`UpdaterHandle::sync_status_rx`] so the UI can display
/// progress indicators and surface errors without polling.
#[derive(Debug, Clone, Default)]
pub enum SyncStatus {
    /// No sync in progress; last sync either succeeded or hasn't run yet.
    #[default]
    Idle,
    /// A sync cycle is currently running.
    Syncing,
    /// Last sync cycle completed successfully.
    Completed { pushed: usize, pulled: usize },
    /// Last sync cycle failed.  App continues normally — sync will retry on
    /// the next interval tick.
    Error(String),
    /// Sync has never run and the server has cloud data — user should be
    /// prompted to decide whether to download it.
    NeedsSetup,
}

/// Handle for the UI to interact with the updater.
#[derive(Clone)]
pub struct UpdaterHandle {
    /// Current update status — watch channel for efficient polling.
    pub status_rx: watch::Receiver<UpdateStatus>,
    /// Send commands to the background task.
    pub cmd_tx: mpsc::UnboundedSender<UpdaterCommand>,
    /// Current authentication status — watch channel updated on login/logout.
    pub auth_rx: watch::Receiver<AuthStatus>,
    /// Latest batch of key hashes synced from the server.
    ///
    /// The main thread polls `has_changed()` each frame and merges the new
    /// set into the Agent API key registry.  Empty vec = no data yet or sync
    /// failed (local keys are unaffected).
    pub synced_keys_rx: watch::Receiver<Vec<crate::key_sync::SyncedKeyEntry>>,
    /// Current cloud sync status.
    ///
    /// The UI polls `has_changed()` each frame and displays progress
    /// indicators or error toasts as appropriate.  Starts as `Idle`.
    pub sync_status_rx: watch::Receiver<SyncStatus>,
}

/// Server manifest response for latest version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub version: String,
    pub sha256: String,
    pub download_url: String,
    pub release_notes: String,
    pub file_size: u64,
    /// Ed25519 signature (base64-encoded) over the raw binary bytes.
    /// `#[serde(default)]` ensures old servers that omit this field
    /// deserialize to `None` rather than failing.
    #[serde(default)]
    pub signature: Option<String>,
}
