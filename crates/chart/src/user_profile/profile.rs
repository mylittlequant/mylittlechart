//! [`UserProfile`] — top-level persistent metadata for a user session.
//!
//! Stores active selections and UI state.  All heavy data (presets, templates,
//! watchlists) live in their own files alongside this one; see
//! [`crate::user_profile::storage`] for the directory layout and generic I/O
//! helpers.

use serde::{Deserialize, Serialize};

// =============================================================================
// Schema version
// =============================================================================


/// Current schema version.  Increment when the serialized format changes in a
/// backward-incompatible way so that migration code can detect old files.
pub const PROFILE_VERSION: u32 = 3;

// =============================================================================
// WindowState
// =============================================================================

/// Persisted state for a single OS window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    /// Unique window identifier (e.g. "win_1728503941").
    #[serde(default)]
    pub window_id: String,
    /// Ordered list of preset IDs open as tabs in this window.
    #[serde(default)]
    pub open_tabs: Vec<String>,
    /// ID of the chart preset that was last active in this window.
    #[serde(default)]
    pub active_preset_id: String,
    /// Screen X coordinate of the window's outer position (physical pixels).
    #[serde(default)]
    pub x: Option<i32>,
    /// Screen Y coordinate of the window's outer position (physical pixels).
    #[serde(default)]
    pub y: Option<i32>,
    /// Inner width of the window in physical pixels.
    #[serde(default)]
    pub width: Option<u32>,
    /// Inner height of the window in physical pixels.
    #[serde(default)]
    pub height: Option<u32>,

    // -------------------------------------------------------------------------
    // Per-window sidebar / inline toolbar state
    // -------------------------------------------------------------------------

    /// Whether the right sidebar is visible in this window.
    #[serde(default)]
    pub sidebar_visible: bool,

    /// Which panel tab is selected in the right sidebar for this window.
    /// `None` means no panel selected / sidebar closed.
    #[serde(default)]
    pub sidebar_panel: Option<String>,

    /// Width of the right sidebar in this window (pixels).
    #[serde(default)]
    pub sidebar_width: Option<f64>,

    /// Horizontal offset of the floating inline toolbar in this window.
    #[serde(default)]
    pub inline_bar_x: Option<f64>,

    /// Vertical offset of the floating inline toolbar in this window.
    #[serde(default)]
    pub inline_bar_y: Option<f64>,

    /// Dock edge of the floating inline toolbar in this window
    /// ("Bottom", "Top", "Free").
    #[serde(default)]
    pub inline_bar_dock: Option<String>,

    // ── Agents tab docking grid (Step 1 scaffold) ─────────────────────────────

    /// Serialized `uzor::panels::LayoutSnapshot` JSON for the agents docking
    /// grid in this window.  `None` means no layout has been saved yet
    /// (the grid will start empty).
    ///
    /// Written by Step 2 save logic; read back on profile restore.
    #[serde(default)]
    pub agents_tab_layout: Option<String>,

    /// Per-pane descriptors for every agent leaf in the saved docking grid.
    ///
    /// The `leaf_id` inside each entry corresponds to the numeric leaf IDs
    /// embedded in `agents_tab_layout`.  On profile restore, Step 2 will
    /// re-create `AgentInstance`s from these descriptors and insert them
    /// into the `DockingManager`.
    #[serde(default)]
    pub agents_tab_leaves: Vec<PersistedAgentLeaf>,
}

// =============================================================================
// PersistedAgentLeaf
// =============================================================================

/// Mirror enums for `gate4agent::AgentCli` / `InstanceMode` that are
/// serializable without depending on gate4agent in the `chart` crate.
///
/// Thin wrappers — profile.rs does not import gate4agent directly.
/// Conversion (`From`/`Into`) is implemented in `chart-app/src/lib.rs`
/// (Step 2) where gate4agent is already a dependency.

/// Local serializable mirror of `gate4agent::AgentCli`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedAgentCli {
    Claude,
    Codex,
    Gemini,
}

/// Local serializable mirror of `gate4agent::InstanceMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedInstanceMode {
    Pty,
    Chat,
}

/// Persisted representation of a single agent docking pane.
///
/// Stored inside [`WindowState::agents_tab_leaves`].  On application startup
/// the `chart-app` layer (Step 2) reads these and re-creates live
/// `AgentInstance`s in the `MultiCliManager`, then rebuilds the
/// `DockingManager<AgentPaneLeaf>` from `agents_tab_layout`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistedAgentLeaf {
    /// Numeric leaf ID matching the layout snapshot (from `uzor::panels::LeafId`).
    pub leaf_id: u64,
    /// Which AI CLI runs in this pane.
    pub cli: PersistedAgentCli,
    /// Transport mode.
    pub mode: PersistedInstanceMode,
    /// Working directory for the agent process.
    pub workdir: std::path::PathBuf,
    /// Chat session identifier — only meaningful for `PersistedInstanceMode::Chat`.
    pub chat_session_id: Option<String>,
}

// =============================================================================
// DataLoadSettings
// =============================================================================

/// Settings that control how historical bar data is fetched and retained.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataLoadSettings {
    /// Bars to backfill after initial 300. Default 2000.
    #[serde(default = "default_background_bar_count")]
    pub background_bar_count: u32,

    /// Max bars kept in memory per chart window. 0 = unlimited. Default 10000.
    #[serde(default = "default_max_loaded_bars")]
    pub max_loaded_bars: u32,

    /// Max total bar-store size on disk (MB). Default 500.
    #[serde(default = "default_max_store_size_mb")]
    pub max_store_size_mb: u32,

    /// Delete bar files not accessed in N days. Default 30.
    #[serde(default = "default_store_cleanup_days")]
    pub store_cleanup_days: u32,
}

impl Default for DataLoadSettings {
    fn default() -> Self {
        Self {
            background_bar_count: default_background_bar_count(),
            max_loaded_bars: default_max_loaded_bars(),
            max_store_size_mb: default_max_store_size_mb(),
            store_cleanup_days: default_store_cleanup_days(),
        }
    }
}

// =============================================================================
// UserProfile
// =============================================================================

/// Top-level persistent state for a user session.
///
/// Lightweight metadata — active selections and UI preferences.  Deserializing
/// this struct should never fail due to missing fields; every field uses
/// `#[serde(default)]` so that new fields added in future versions are simply
/// initialized to their defaults when loading an older profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    /// Schema version.  Used for forward-compatible migration.
    #[serde(default = "default_version")]
    pub version: u32,

    // -------------------------------------------------------------------------
    // Active selections
    // -------------------------------------------------------------------------

    /// ID of the chart preset that was last active (`"preset_{ts}_{nanos}"`).
    ///
    /// Empty string means no preset is active (use default state).
    #[serde(default)]
    pub active_preset_id: String,

    /// Ordered list of preset IDs open as tabs (persisted across sessions).
    #[serde(default)]
    pub open_tabs: Vec<String>,

    /// Name of the active theme (e.g. `"dark"`, `"light"`).
    #[serde(default = "default_theme")]
    pub active_theme: String,

    // -------------------------------------------------------------------------
    // Sidebar / panel UI state
    // -------------------------------------------------------------------------

    /// Whether the right sidebar is currently visible.
    #[serde(default)]
    pub sidebar_visible: bool,

    /// Which panel tab is selected in the right sidebar (e.g. `"watchlist"`,
    /// `"alerts"`).  `None` means default / no panel selected.
    #[serde(default)]
    pub sidebar_panel: Option<String>,

    /// Width of the right sidebar in pixels.  `None` means use the default.
    #[serde(default)]
    pub sidebar_width: Option<f64>,

    // -------------------------------------------------------------------------
    // Inline toolbar position
    // -------------------------------------------------------------------------

    /// Horizontal offset of the floating inline toolbar.
    #[serde(default)]
    pub inline_bar_x: Option<f64>,

    /// Vertical offset of the floating inline toolbar.
    #[serde(default)]
    pub inline_bar_y: Option<f64>,

    /// Dock edge of the floating inline toolbar ("Bottom", "Top", "Free").
    #[serde(default)]
    pub inline_bar_dock: Option<String>,

    // -------------------------------------------------------------------------
    // Device identity
    // -------------------------------------------------------------------------

    /// Human-readable device name (auto-detected or user-set).
    /// e.g. "VA-PC-WIN10", "MacBook Pro"
    #[serde(default)]
    pub device_name: String,

    /// App version at the time of last launch.
    #[serde(default)]
    pub app_version: String,

    // -------------------------------------------------------------------------
    // Optional authentication (user can link an account)
    // -------------------------------------------------------------------------

    /// Optional linked account info. None = anonymous user.
    #[serde(default)]
    pub linked_account: Option<LinkedAccount>,

    // -------------------------------------------------------------------------
    // Chart data preferences
    // -------------------------------------------------------------------------

    /// Number of historical bars to load on chart open.
    #[serde(default = "default_bar_count")]
    pub bar_count: u16,

    /// Indicator recalculation mode.
    ///
    /// Valid values: `"PerTick"`, `"PerFrame"`, `"PerBar"`.
    /// Defaults to `"PerFrame"` when the field is absent (older profiles).
    #[serde(default = "default_recalc_mode")]
    pub recalc_mode: String,

    /// Price scale mode preference.
    ///
    /// Valid values: `"Auto"`, `"Focus"`, `"Manual"`.
    /// Defaults to `"Auto"` when the field is absent (older profiles).
    #[serde(default = "default_scale_mode")]
    pub scale_mode: String,

    /// UI language preference (ISO 639-1 code: "en", "ru").
    /// Applied at startup via `set_language()`.
    #[serde(default = "default_language")]
    pub language: String,

    // -------------------------------------------------------------------------
    // Agent API server
    // -------------------------------------------------------------------------

    /// Whether the internal Agent API server is enabled on startup.
    #[serde(default = "default_server_enabled")]
    pub server_enabled: bool,

    /// Port for the Agent API server.
    #[serde(default = "default_server_port")]
    pub server_port: u16,

    /// Legacy single API key — kept for backward-compat deserialization only.
    ///
    /// If non-empty on load it will be migrated to a single admin entry in
    /// `local_agent_keys` by the startup code in `main.rs`.
    #[serde(default, rename = "agent_api_key")]
    pub legacy_single_agent_key: String,

    /// Registered local agent CLI connector keys with permission tiers.
    ///
    /// An empty vec means auth is disabled (open access).
    /// `alias` ensures old profiles serialized with `agent_api_keys` still load.
    #[serde(default, alias = "agent_api_keys")]
    pub local_agent_keys: Vec<StoredLocalAgentKey>,

    // -------------------------------------------------------------------------
    // Exchange API credentials (keychain-backed)
    // -------------------------------------------------------------------------

    /// Persisted exchange API key entries.
    ///
    /// Each entry holds the public API key and either a plaintext secret
    /// (legacy) or a [`CredentialRef`] pointing to the OS keychain (preferred).
    /// An empty vec means no exchange credentials have been saved yet.
    #[serde(default)]
    pub exchange_keys: Vec<StoredExchangeKey>,

    // -------------------------------------------------------------------------
    // Connector enable/disable state
    // -------------------------------------------------------------------------

    /// Per-connector enabled/disabled toggle state.
    /// Key: exchange id string (e.g. `"binance"`, `"bybit"`).
    /// Value: `true` = enabled (default when absent), `false` = disabled.
    #[serde(default)]
    pub connector_enabled: std::collections::HashMap<String, bool>,

    // -------------------------------------------------------------------------
    // Telemetry opt-out
    // -------------------------------------------------------------------------

    // -------------------------------------------------------------------------
    // Cloud connectivity
    // -------------------------------------------------------------------------

    /// Whether cloud connectivity (OTA, sync, telemetry) is enabled.
    ///
    /// Set at profile creation (connected vs standalone mode) and persisted in
    /// profile.json.  Renamed from the old `client_mode == Connected` pattern.
    #[serde(default)]
    pub cloud_enabled: bool,

    /// Sync level: "local", "connected", "cloud".
    /// Source of truth — index.json derives from this on every load.
    #[serde(default)]
    pub sync_level: String,

    /// Whether OTA (over-the-air) updates are enabled.
    ///
    /// When `false`, the app will not check for updates on startup.
    /// Defaults to `true`.
    #[serde(default = "default_true")]
    pub ota_enabled: bool,

    // -------------------------------------------------------------------------
    // Telemetry counters (accumulated since installation)
    // -------------------------------------------------------------------------

    #[serde(default)]
    pub telemetry: TelemetryData,

    // -------------------------------------------------------------------------
    // Notification / alert delivery settings
    // -------------------------------------------------------------------------

    /// Alert notification delivery settings (Telegram bot, toasts, sound, webhook).
    #[serde(default)]
    pub notification_settings: alert_delivery::NotificationSettings,

    // -------------------------------------------------------------------------
    // Per-window state
    // -------------------------------------------------------------------------

    /// Per-window tab/preset state.  When non-empty, each entry describes one
    /// OS window.  The first entry (`windows[0]`) is the primary window.
    /// Legacy fields `open_tabs` / `active_preset_id` are kept for backward
    /// compatibility and always reflect the primary window.
    #[serde(default)]
    pub windows: Vec<WindowState>,

    // -------------------------------------------------------------------------
    // Cloud sync
    // -------------------------------------------------------------------------

    /// Incremental sync state — persisted so subsequent syncs only request
    /// items changed since the last successful sync.
    #[serde(default)]
    pub sync_state: SyncState,

    // -------------------------------------------------------------------------
    // Multi-profile identity
    // -------------------------------------------------------------------------

    /// Unique profile identifier (UUID v4). Empty on legacy profiles.
    #[serde(default)]
    pub profile_id: String,

    /// User-visible profile name ("Default", "Trading", "Debug", etc.)
    #[serde(default = "default_profile_name")]
    pub display_name: String,

    /// Avatar emoji key ("chart", "rocket", "shield", "fire", "star", "moon", "sun", "ghost")
    #[serde(default = "default_avatar")]
    pub avatar: String,

    /// Profile creation timestamp (unix seconds)
    #[serde(default)]
    pub profile_created_at: i64,

    // -------------------------------------------------------------------------
    // Data load settings
    // -------------------------------------------------------------------------

    /// Historical bar data fetch and retention settings.
    #[serde(default)]
    pub data_load: DataLoadSettings,
}

// =============================================================================
// SyncState
// =============================================================================

/// Minimal state required to perform incremental cloud sync.
///
/// Stored inside [`UserProfile`] so it is automatically persisted to and
/// loaded from `profile.json` via the existing save/load infrastructure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    /// Unix timestamp (seconds) of the last successful sync.
    /// `0` means the client has never completed a sync.
    #[serde(default)]
    pub last_sync_timestamp: i64,
    /// Whether the user has opted into cloud sync.
    #[serde(default)]
    pub enabled: bool,
    /// Whether the encrypted vault file (`vault.enc`) is included in cloud sync.
    /// Defaults to `true` — the vault is always synced unless the user opts out.
    #[serde(default = "default_true")]
    pub sync_vault: bool,
    /// Whether chart presets are included in cloud sync.
    #[serde(default = "default_true")]
    pub sync_presets: bool,
    /// Whether indicator templates are included in cloud sync.
    #[serde(default = "default_true")]
    pub sync_templates: bool,
    /// Whether watchlists are included in cloud sync.
    #[serde(default = "default_true")]
    pub sync_watchlists: bool,
    /// Whether the active theme is included in cloud sync.
    #[serde(default = "default_true")]
    pub sync_theme: bool,
    /// Whether the recovery key (encrypted master key) is included in cloud sync.
    #[serde(default = "default_true")]
    pub sync_recovery_key: bool,
    /// Set of sync item keys (`"category:sync_id"`) that have been successfully
    /// pushed to the server at least once.
    ///
    /// Used for tombstone detection: if an item disappears from local storage
    /// but is present in this set, a tombstone (`deleted: true`) is pushed on
    /// the next sync cycle so the server marks the item as deleted.
    #[serde(default)]
    pub synced_items: std::collections::HashSet<String>,

    /// SHA-256 checksums of sync items as they were after the last successful
    /// push or pull.
    ///
    /// Key: `sync_id` (e.g. `"preset_1728503941_123456789"`).
    /// Value: SHA-256 hex digest of the item content at the time it was last
    /// successfully synced to *or* from the server.
    ///
    /// Persisted across restarts so that conflict detection works correctly
    /// after a restart.  Without this, an empty map would cause every item to
    /// appear as a conflict on the first sync after startup.
    ///
    /// Seeded into `cloud_sync::SyncState.last_synced_checksums` on startup
    /// and written back after each successful sync cycle.
    #[serde(default)]
    pub last_synced_checksums: std::collections::HashMap<String, String>,
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
            synced_items: std::collections::HashSet::new(),
            last_synced_checksums: std::collections::HashMap::new(),
        }
    }
}

impl UserProfile {
    /// Create a new profile with sensible defaults.
    pub fn new() -> Self {
        Self {
            version: PROFILE_VERSION,
            active_preset_id: String::new(),
            open_tabs: Vec::new(),
            active_theme: default_theme(),
            sidebar_visible: false,
            sidebar_panel: None,
            sidebar_width: None,
            inline_bar_x: None,
            inline_bar_y: None,
            inline_bar_dock: None,
            // New fields
            device_name: String::new(),
            app_version: String::new(),
            linked_account: None,
            bar_count: default_bar_count(),
            recalc_mode: default_recalc_mode(),
            scale_mode: default_scale_mode(),
            language: default_language(),
            server_enabled: default_server_enabled(),
            server_port: default_server_port(),
            legacy_single_agent_key: String::new(),
            local_agent_keys: Vec::new(),
            exchange_keys: Vec::new(),
            connector_enabled: std::collections::HashMap::new(),
            cloud_enabled: false,
            sync_level: String::new(),
            ota_enabled: true,
            telemetry: TelemetryData::default(),
            notification_settings: alert_delivery::NotificationSettings::default(),
            windows: Vec::new(),
            sync_state: SyncState::default(),
            profile_id: String::new(),
            display_name: "Default".to_string(),
            avatar: "chart".to_string(),
            profile_created_at: 0,
            data_load: DataLoadSettings::default(),
        }
    }

    /// Record a new app launch in telemetry.
    pub fn record_launch(&mut self, app_version: &str) {
        self.app_version = app_version.to_string();
        self.telemetry.total_launches += 1;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.telemetry.last_active_at = now;
        if self.telemetry.first_launch_at == 0 {
            self.telemetry.first_launch_at = now;
        }
    }
}

impl Default for UserProfile {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// CredentialRef — pointer to a secret stored in the OS keychain
// =============================================================================

/// A reference to a secret stored in the OS keychain.
///
/// The actual secret value is NEVER written to `profile.json`. Only this
/// lightweight pointer is persisted. The app resolves the real value at
/// runtime by calling into the OS keychain (Windows Credential Manager,
/// macOS Keychain, Linux libsecret).
///
/// Key naming conventions:
/// - Exchange API secrets: `service = "nemo-exchange-{exchange_id}"`,
///   `username = "{api_key}"` (so multiple accounts per exchange are supported)
/// - E2E master key:       `service = "nemo-e2e-master"`,
///   `username = "default"`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRef {
    /// Keychain service name (e.g. `"nemo-exchange-binance"`).
    pub service: String,
    /// Keychain username / key identifier
    /// (e.g. the API key string itself, acting as the account name).
    pub username: String,
}

impl CredentialRef {
    /// Construct a new `CredentialRef`.
    pub fn new(service: impl Into<String>, username: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            username: username.into(),
        }
    }

    /// Convenience constructor for an exchange API secret reference.
    ///
    /// `exchange_id` should be the lowercase exchange identifier,
    /// e.g. `"binance"`, `"bybit"`.
    /// `api_key` is the public API key string used as the account identifier.
    pub fn for_exchange_secret(exchange_id: &str, api_key: &str) -> Self {
        Self::new(format!("nemo-exchange-{}", exchange_id), api_key)
    }

    /// Convenience constructor for the E2E encryption master key.
    pub fn for_e2e_master() -> Self {
        Self::new("nemo-e2e-master", "default")
    }
}

// =============================================================================
// StoredExchangeKey — persisted exchange API key entry (scaffold)
// =============================================================================

/// Persisted representation of an exchange API key pair.
///
/// The `api_secret` field stores the plaintext secret for backward
/// compatibility. When `keychain_ref` is `Some`, the app MUST read the
/// secret from the OS keychain instead, and `api_secret` should be an
/// empty string.
///
/// Migration path (future step):
/// 1. On load: if `api_secret` is non-empty and `keychain_ref` is `None`,
///    migrate — store to keychain, set `keychain_ref`, clear `api_secret`.
/// 2. On save: never write `api_secret` when `keychain_ref` is `Some`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredExchangeKey {
    /// Lowercase exchange identifier (e.g. `"binance"`, `"bybit"`).
    pub exchange_id: String,
    /// Human-readable label for this key pair (e.g. `"main account"`).
    #[serde(default)]
    pub label: String,
    /// Public API key (not a secret; safe to store in profile.json).
    pub api_key: String,
    /// Plaintext API secret — kept for backward compatibility ONLY.
    ///
    /// When `keychain_ref` is `Some` this field MUST be an empty string.
    /// New code should always prefer `keychain_ref`.
    #[serde(default)]
    pub api_secret: String,
    /// Optional passphrase (OKX, KuCoin style) — plaintext fallback.
    ///
    /// When `passphrase_keychain_ref` is `Some` this field MUST be empty.
    #[serde(default)]
    pub passphrase: Option<String>,
    /// Reference to the API secret stored in the OS keychain.
    /// When present, `api_secret` must be empty and this takes precedence.
    #[serde(default)]
    pub keychain_ref: Option<CredentialRef>,
    /// Reference to the passphrase stored in the OS keychain (OKX/KuCoin).
    /// When present, `passphrase` must be `None` and this takes precedence.
    #[serde(default)]
    pub passphrase_keychain_ref: Option<CredentialRef>,
    /// Whether to connect to the testnet / sandbox endpoint.
    #[serde(default)]
    pub testnet: bool,
    /// Unix timestamp (seconds) when this entry was created.
    #[serde(default)]
    pub created_at: u64,
}

// =============================================================================
// StoredLocalAgentKey — persisted local agent CLI connector key entry
// =============================================================================

/// Persisted representation of a single local agent CLI connector key entry.
///
/// This type mirrors [`zengeld_server::state::LocalAgentKey`] but lives in the
/// `chart` crate so that `profile.rs` stays independent of `zengeld-server`.
/// Conversion to the server type happens in `chart-app-vello/src/main.rs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredLocalAgentKey {
    /// SHA-256 hex digest of the raw key.
    pub key_hash: String,
    /// Human-readable label.
    pub label: String,
    /// Tier: `"read_only"`, `"read_write"`, or `"admin"`.
    pub tier: String,
    /// Unix timestamp (seconds) when this entry was created.
    pub created_at: u64,
    /// Optional agent identifier.
    #[serde(default)]
    pub agent_id: Option<String>,
    /// Key origin: `"local"` or `"cloud"` (legacy).
    ///
    /// Uses `String` instead of an enum so the JSON stays human-readable and
    /// backward-compatible.  Missing field defaults to `"local"` so that keys
    /// loaded from older profile.json files are treated as local.
    #[serde(default = "default_key_source")]
    pub source: String,
}

/// Backward-compatible type alias — old code using `StoredApiKey` still compiles.
pub type StoredApiKey = StoredLocalAgentKey;

// =============================================================================
// LinkedAccount
// =============================================================================

/// Optional linked account for user identification.
/// Users can optionally link a Telegram, GitHub, Google, or Discord account,
/// or just set a local display name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkedAccount {
    /// Authentication provider: "local", "telegram", "github", "google", "discord"
    pub provider: String,

    /// Provider-specific user ID (e.g. Telegram user_id, GitHub user_id)
    #[serde(default)]
    pub provider_user_id: String,

    /// Display name chosen by user or fetched from provider
    #[serde(default)]
    pub display_name: String,

    /// When the account was linked (unix timestamp seconds)
    #[serde(default)]
    pub linked_at: u64,
}

// =============================================================================
// TelemetryData
// =============================================================================

/// Accumulated usage telemetry for analytics.
/// Counters reset only on profile reset, not on app restart.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelemetryData {
    /// Total app launches since installation
    #[serde(default)]
    pub total_launches: u64,

    /// Total active time in seconds (app in foreground)
    #[serde(default)]
    pub total_active_seconds: u64,

    /// Last active timestamp (unix seconds) — for "last seen"
    #[serde(default)]
    pub last_active_at: u64,

    /// First launch timestamp (unix seconds)
    #[serde(default)]
    pub first_launch_at: u64,

    /// Total chart windows opened
    #[serde(default)]
    pub charts_opened: u64,

    /// Total indicators added
    #[serde(default)]
    pub indicators_added: u64,

    /// Total drawing primitives created
    #[serde(default)]
    pub drawings_created: u64,

    /// Total presets saved
    #[serde(default)]
    pub presets_saved: u64,

    /// Total templates saved
    #[serde(default)]
    pub templates_saved: u64,

    /// Total click/interaction count (rough engagement metric)
    #[serde(default)]
    pub total_interactions: u64,

    /// Total symbols searched/viewed
    #[serde(default)]
    pub symbols_viewed: u64,
}

// =============================================================================
// ProfileMeta / ProfileIndex — multi-profile index
// =============================================================================

/// Metadata for one profile entry in the index.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileMeta {
    /// UUID v4 identifier for this profile.
    pub id: String,
    /// User-visible display name.
    pub display_name: String,
    /// Avatar emoji key.
    pub avatar: String,
    /// Unix timestamp (seconds) when this profile was created.
    pub created_at: i64,
    /// Relative subdirectory name under `profiles/` (e.g. "default" or a UUID).
    pub dir_name: String,
    /// Whether cloud connectivity (OTA, sync, telemetry) is enabled.
    /// Old index.json files without this field default to false (cloud disabled).
    #[serde(default)]
    pub cloud_enabled: bool,
    /// Sync level label for display: "local", "connected", "cloud".
    /// Old index.json files without this field derive it from cloud_enabled.
    #[serde(default)]
    pub sync_level: String,
}

/// The profile index file — lists all profiles and which is active.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileIndex {
    /// UUID of the currently active profile.
    pub active_profile_id: String,
    /// Ordered list of all profile metadata entries.
    pub profiles: Vec<ProfileMeta>,
}

// =============================================================================
// VaultSecrets — encrypted credential store (vault.enc)
// =============================================================================

/// Sensitive credentials that must be encrypted at rest.
///
/// Stored separately in `vault.enc`, never in plaintext `profile.json`.
/// When a vault key is present, these fields are extracted from
/// [`UserProfile`] before writing `profile.json` and saved encrypted.
/// On load, they are decrypted from `vault.enc` and merged back into
/// the in-memory `UserProfile`.
///
/// A vault key is always required — set during the welcome wizard.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VaultSecrets {
    /// Legacy single API key — kept for backward-compat migration only.
    #[serde(default, rename = "agent_api_key")]
    pub legacy_single_agent_key: String,

    /// Registered local agent CLI connector keys with permission tiers (contains `key_hash`).
    #[serde(default, alias = "agent_api_keys")]
    pub local_agent_keys: Vec<StoredLocalAgentKey>,

    /// Exchange API key entries (contains `api_secret` and `passphrase`).
    #[serde(default)]
    pub exchange_keys: Vec<StoredExchangeKey>,

    /// Alert notification delivery settings.
    ///
    /// Stored here because it contains `telegram.bot_token`,
    /// `telegram.subscribers` (PII — chat IDs), and `webhook.url`
    /// (may embed auth tokens).
    #[serde(default)]
    pub notification_settings: alert_delivery::NotificationSettings,
}

impl VaultSecrets {
    /// Extract credential fields from a [`UserProfile`], returning a
    /// `VaultSecrets` and clearing those fields in the profile so they
    /// are not written to plaintext storage.
    pub fn extract_from(profile: &mut UserProfile) -> Self {
        
        Self {
            legacy_single_agent_key: std::mem::take(&mut profile.legacy_single_agent_key),
            local_agent_keys: std::mem::take(&mut profile.local_agent_keys),
            exchange_keys: std::mem::take(&mut profile.exchange_keys),
            notification_settings: std::mem::take(&mut profile.notification_settings),
        }
    }

    /// Merge secrets back into a [`UserProfile`] after decrypting from vault.
    pub fn merge_into(self, profile: &mut UserProfile) {
        profile.legacy_single_agent_key = self.legacy_single_agent_key;
        profile.local_agent_keys = self.local_agent_keys;
        profile.exchange_keys = self.exchange_keys;
        profile.notification_settings = self.notification_settings;
        profile.notification_settings.telegram.migrate_legacy();
    }
}

// =============================================================================
// serde defaults
// =============================================================================

fn default_version() -> u32 {
    PROFILE_VERSION
}

fn default_true() -> bool {
    true
}

fn default_key_source() -> String {
    "local".to_string()
}

fn default_theme() -> String {
    "dark".to_string()
}

fn default_bar_count() -> u16 {
    2000
}

fn default_recalc_mode() -> String {
    "PerFrame".to_string()
}

fn default_scale_mode() -> String {
    "Auto".to_string()
}

fn default_language() -> String {
    "en".to_string()
}

fn default_server_enabled() -> bool {
    true
}

fn default_server_port() -> u16 {
    17420
}

fn default_profile_name() -> String {
    "Default".to_string()
}

fn default_avatar() -> String {
    "chart".to_string()
}

fn default_background_bar_count() -> u32 {
    2000
}

fn default_max_loaded_bars() -> u32 {
    10_000
}

fn default_max_store_size_mb() -> u32 {
    500
}

fn default_store_cleanup_days() -> u32 {
    30
}
