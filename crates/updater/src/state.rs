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
