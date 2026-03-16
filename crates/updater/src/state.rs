use tokio::sync::{mpsc, watch};
use serde::{Serialize, Deserialize};

// =============================================================================
// Conflict types
// =============================================================================

/// Describes a conflict between local and cloud versions of an item.
///
/// Both sides were modified since the last successful sync.  Resolution is
/// deferred to the caller (typically presented to the user).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConflict {
    /// Stable identifier shared by both sides.
    pub sync_id: String,
    /// Category of the item: `"preset"`, `"watchlist"`, etc.
    pub category: String,
    /// Human-readable name.
    pub name: String,
    /// Unix timestamp (milliseconds) of the local version.
    pub local_modified: i64,
    /// Unix timestamp (milliseconds) of the cloud version.
    pub cloud_modified: i64,
    /// Checksum of the local version.
    pub local_checksum: String,
    /// Checksum of the cloud version.
    pub cloud_checksum: String,
    /// Full local content — stored so we can push it back if user picks KeepLocal.
    pub local_content: String,
}

/// How the user wants to resolve a sync conflict for a single item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConflictResolution {
    /// Discard the cloud version; push local version to the server.
    KeepLocal,
    /// Discard the local version; write cloud version to disk.
    KeepCloud,
}

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
    /// Enable or disable cloud connectivity at runtime.
    /// `true` = cloud enabled (OTA, sync, telemetry),
    /// `false` = cloud disabled (stop all phone-home).
    SetCloudEnabled(bool),
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
    /// Enable or disable syncing of chart presets at runtime.
    SetSyncPresets(bool),
    /// Enable or disable syncing of indicator and primitive templates at runtime.
    SetSyncTemplates(bool),
    /// Enable or disable syncing of watchlists at runtime.
    SetSyncWatchlists(bool),
    /// Enable or disable syncing of the active theme at runtime.
    SetSyncTheme(bool),
    /// Enable or disable syncing of the vault (API keys / exchange credentials) at runtime.
    SetSyncVault(bool),
    /// Enable or disable syncing of the recovery key at runtime.
    SetSyncRecoveryKey(bool),
    /// Update the data directory path used for collecting sync items.
    ///
    /// Must be sent after a profile switch so the updater reads from the
    /// new profile's directory rather than the old one.
    SetDataDir(std::path::PathBuf),
    /// Update the active profile ID used in sync HTTP request headers.
    ///
    /// Must be sent after a profile switch alongside [`SetDataDir`] so that
    /// `X-Profile-Id` headers on all sync requests reflect the current profile.
    SetProfileId(String),
    /// Resolve a sync conflict for a specific item.
    ///
    /// - `KeepLocal`: push the local version to the server.
    /// - `KeepCloud`: write the server version to disk.
    ///
    /// If `sync_id` is not in the pending conflicts list the command is a no-op.
    ResolveConflict {
        sync_id: String,
        resolution: ConflictResolution,
    },
    /// Notify that specific blob categories changed on disk and should be synced.
    ///
    /// The list of category strings is purely informational — the updater's
    /// `do_cloud_sync` reads all files and filters by `sync_state` toggles
    /// regardless of which categories are listed.  The categories are used only
    /// for logging so the operator can see which change triggered the push.
    ///
    /// Ignored if cloud sync is disabled or the user is not logged in.
    SyncPushChanged(Vec<String>),
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
    /// One or more items have conflicting changes on both local and cloud.
    ///
    /// The UI should surface a conflict resolution modal.  Items not listed
    /// here were synced successfully; they do **not** need re-resolution.
    ConflictsDetected(Vec<SyncConflict>),
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
    /// Current cloud sync status.
    ///
    /// The UI polls `has_changed()` each frame and displays progress
    /// indicators or error toasts as appropriate.  Starts as `Idle`.
    pub sync_status_rx: watch::Receiver<SyncStatus>,
    /// Latest `last_synced_checksums` map after each successful sync cycle.
    ///
    /// The main thread polls `has_changed()` each frame and writes the new
    /// map into `profile_manager.profile.sync_state.last_synced_checksums`
    /// so that it is persisted to disk on the next profile save.
    ///
    /// An empty map is the initial value — it is only populated after a sync
    /// cycle completes successfully.
    pub sync_checksums_rx: watch::Receiver<std::collections::HashMap<String, String>>,
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
