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

/// Resolved feature-gate set from `GET /api/entitlements` on the update
/// server, broadcast via a watch channel so the UI can react.
///
/// Mirrors `mlc_cloud::EntitlementsResponse` (the wasm shell's own copy of
/// this shape) field-for-field, but this crate does not depend on
/// `mlc-cloud` — its native transport does not exist (see that crate's own
/// module doc) — so the type is duplicated here rather than shared.
///
/// The `Default` is the free set: no features, `expires_at = 0` so it
/// reads as already-expired. That default is also what a failed or
/// not-yet-attempted fetch must degrade to — never a fabricated paid set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Entitlements {
    pub features: Vec<String>,
    /// Millisecond Unix timestamp from the server; re-ask after it passes.
    pub expires_at: i64,
    /// Whether the caller currently has a paid (or redeemed-trial) tier.
    /// `#[serde(default)]` so an older server that doesn't emit this field
    /// still parses — the caller degrades to `false`, the free reading.
    #[serde(default)]
    pub paid: bool,
    /// RFC3339 timestamp of when the paid tier expires, if any.
    /// `#[serde(default)]` for the same reason as `paid`.
    #[serde(default)]
    pub paid_until: Option<String>,
}

impl Entitlements {
    /// Whether `feature` is present in the resolved set.
    pub fn has(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }
}

/// Outcome of the most recent gift/trial code redemption attempt, broadcast
/// via a watch channel so the UI reads it as a frame-local fact rather than
/// owning the request itself.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum RedeemState {
    /// No redemption has been attempted (or the last one was superseded).
    #[default]
    Idle,
    /// A redemption request is in flight.
    Working,
    /// The code was redeemed successfully; carries the new paid-until timestamp (RFC3339).
    Redeemed { paid_until: String },
    /// The redemption attempt failed; carries a human-readable message.
    Failed { message: String },
}

/// Outcome of the most recent account-nickname submission
/// ([`UpdaterCommand::SetNick`]), broadcast via a watch channel — same
/// mailbox idiom as [`RedeemState`], mapped by the shell onto
/// `mlc_app_layout::ui::modal_settings::NickStatus`.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum NickState {
    /// No submission since the settings row last opened (or the last one
    /// was superseded).
    #[default]
    Idle,
    /// A submission is in flight.
    Working,
    /// Accepted; carries the label to show for this account from now on.
    Saved { effective_label: String },
    /// Refused, with the sentence to show the person who typed it.
    Failed { message: String },
}

/// Commands for the paid-only forum board, wrapped in
/// [`UpdaterCommand::Forum`].
#[derive(Debug, Clone)]
pub enum ForumCommand {
    /// `GET /api/forum/threads?before=…` — `None` = first page (replaces
    /// [`ForumState::list`] wholesale); `Some(cursor)` = "Load more"
    /// (appends to it).
    ListThreads { before: Option<String> },
    /// `GET /api/forum/threads/{id}`.
    LoadThread { id: i64 },
    /// `POST /api/forum/threads`. On success the updater also re-lists
    /// (page one) and loads the new thread, so the overlay can land
    /// straight on it without a second click.
    CreateThread { title: String, body: String },
    /// `POST /api/forum/threads/{id}/replies`. On success the updater
    /// reloads the thread so [`ForumState::thread`] carries the new reply.
    Reply { thread_id: i64, body: String },
    /// `DELETE /api/forum/threads/{id}`. On success the updater clears
    /// [`ForumState::thread`] and re-lists page one.
    DeleteThread { id: i64 },
    /// `DELETE /api/forum/replies/{id}` — the endpoint itself takes only
    /// the reply id; `thread_id` is client-side bookkeeping so the updater
    /// knows which thread to reload on success.
    DeleteReply { id: i64, thread_id: i64 },
}

/// Forum board state, broadcast via [`UpdaterHandle::forum_rx`] — mutated in
/// place by the updater loop rather than replaced wholesale like
/// [`RedeemState`]/[`NickState`], because the overlay needs `list` and
/// `thread` to persist across unrelated actions (e.g. posting a reply must
/// not blank the thread that reply belongs to).
#[derive(Debug, Clone, Default)]
pub struct ForumState {
    /// Most recent thread list page(s) — see [`ForumCommand::ListThreads`].
    pub list: Vec<crate::forum::ForumThreadSummary>,
    /// Pagination cursor for the next "Load more" — `None` once the list
    /// has run out (or nothing has been fetched yet).
    pub next_before: Option<String>,
    /// The thread currently open in the reading view, if any.
    pub thread: Option<crate::forum::ForumThread>,
    /// The most recent action's failure, if any. Cleared at the start of
    /// every new [`ForumCommand`] (so a stale error from a screen the user
    /// already left never lingers onto the next one) and set only if THAT
    /// command's own request fails.
    pub last_error: Option<String>,
    /// A forum request is in flight.
    pub busy: bool,
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
    /// Redeem a gift/trial code for the signed-in account.
    ///
    /// Requires a stored token — with none, the updater publishes
    /// [`RedeemState::Failed`] without making a request. The outcome is
    /// broadcast via [`UpdaterHandle::redeem_rx`]; a success also refreshes
    /// `entitlements_rx` immediately so the new tier is visible without
    /// waiting out the entitlements poll interval.
    RedeemCode(String),
    /// Set the signed-in account's nickname (account-level attribute,
    /// distinct from any local profile display name).
    ///
    /// Requires a stored token — with none, the updater publishes
    /// [`NickState::Failed`] without making a request. On success the token
    /// store's `display_name` is updated to the returned effective label and
    /// [`UpdaterHandle::auth_rx`] republishes it, so the chrome's account
    /// label picks it up the same frame. The outcome is broadcast via
    /// [`UpdaterHandle::nick_rx`].
    SetNick(String),
    /// A paid-only forum board action. Requires a stored token — with none,
    /// the updater publishes [`ForumState::last_error`] without making a
    /// request. Outcomes are broadcast via [`UpdaterHandle::forum_rx`].
    Forum(ForumCommand),
    /// Enable or disable cloud connectivity at runtime.
    /// `true` = cloud enabled (OTA, sync, telemetry),
    /// `false` = cloud disabled (stop all network activity).
    SetCloudEnabled(bool),
    /// Trigger an immediate cloud sync cycle (push local changes, pull remote).
    ///
    /// Ignored in standalone mode.  The updater broadcasts progress via the
    /// `sync_status_rx` watch channel on `UpdaterHandle`.
    ForceSync,
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
    /// Request a list of all profiles the authenticated user has stored on the
    /// server.  The updater responds via [`SyncStatus::CloudProfilesLoaded`] or
    /// [`SyncStatus::CloudProfilesError`].
    ListCloudProfiles,
    /// Restore a specific cloud profile to the local device.
    ///
    /// Downloads all sync items for `profile_id` from the server and writes
    /// them into `profiles/{profile_id}/`.  Also registers the profile in the
    /// local `index.json`.  The updater responds via
    /// [`SyncStatus::ProfileRestored`] or [`SyncStatus::ProfileRestoreError`].
    RestoreCloudProfile {
        /// UUID of the profile to restore.
        profile_id: String,
    },
    /// Switch the OTA update channel at runtime (`"stable"` or `"dev"`).
    ///
    /// Also resets the check interval to match the new channel:
    /// `"dev"` → 2 minutes, `"stable"` → 4 hours.
    SetChannel(String),
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
    /// Response to [`UpdaterCommand::ListCloudProfiles`] — the server returned
    /// the list of profiles the authenticated user has stored in the cloud.
    CloudProfilesLoaded(Vec<crate::cloud_sync::CloudProfileInfo>),
    /// Response to [`UpdaterCommand::ListCloudProfiles`] when the request
    /// failed (network error, not authenticated, etc.).
    CloudProfilesError(String),
    /// Response to [`UpdaterCommand::RestoreCloudProfile`] — the profile was
    /// downloaded from the server and written to disk successfully.
    ProfileRestored {
        /// UUID of the profile that was restored.
        profile_id: String,
    },
    /// Response to [`UpdaterCommand::RestoreCloudProfile`] when the restore
    /// operation failed.
    ProfileRestoreError(String),
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
    /// Current resolved feature-gate set from `GET /api/entitlements`.
    ///
    /// The UI polls this each frame (a channel read, not network IO) to
    /// decide whether to draw paid-tier surfaces such as the profile list.
    /// Starts at [`Entitlements::default`] (the free set) and only ever
    /// advances via a successful fetch — see that type's own doc for why a
    /// failed fetch must never publish anything else.
    pub entitlements_rx: watch::Receiver<Entitlements>,
    /// Outcome of the most recent [`UpdaterCommand::RedeemCode`] attempt.
    ///
    /// The UI polls this each frame (a channel read, not network IO) to
    /// drive a redeem-code modal's state. Starts at [`RedeemState::Idle`]
    /// and is republished on every attempt — see that type's own doc.
    pub redeem_rx: watch::Receiver<RedeemState>,
    /// Outcome of the most recent [`UpdaterCommand::SetNick`] attempt.
    ///
    /// Same polling contract as [`Self::redeem_rx`]; starts at
    /// [`NickState::Idle`].
    pub nick_rx: watch::Receiver<NickState>,
    /// Current forum board state — list/thread/error/busy, mutated in place
    /// by the updater loop. Starts at [`ForumState::default`] (empty list,
    /// no thread, no error, not busy).
    pub forum_rx: watch::Receiver<ForumState>,
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
