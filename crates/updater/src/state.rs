use tokio::sync::{mpsc, watch};
use serde::{Serialize, Deserialize};

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
}
