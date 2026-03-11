//! [`UserProfile`] — top-level persistent metadata for a user session.
//!
//! Stores active selections and UI state.  All heavy data (presets, templates,
//! watchlists) live in their own files alongside this one; see
//! [`crate::user_profile::storage`] for the directory layout and generic I/O
//! helpers.

use serde::{Deserialize, Serialize};

// =============================================================================
// ClientMode
// =============================================================================

/// Client operation mode — controls server connectivity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMode {
    /// No communication with chart.example. Zero phone-home.
    Standalone,
    /// Connected to chart.example — OTA, sync, centralized keys.
    Connected,
}

impl Default for ClientMode {
    fn default() -> Self {
        ClientMode::Standalone
    }
}

// =============================================================================
// Schema version
// =============================================================================


/// Current schema version.  Increment when the serialized format changes in a
/// backward-incompatible way so that migration code can detect old files.
pub const PROFILE_VERSION: u32 = 2;

// =============================================================================
// WindowState
// =============================================================================

/// Persisted state for a single OS window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    /// Unique window identifier (e.g. "win_1728503941").
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

    /// Client operation mode — whether to connect to chart.example or run
    /// fully standalone.  Defaults to `Standalone` for new installs (privacy-first).
    #[serde(default)]
    pub client_mode: ClientMode,

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
    /// `agent_api_keys` by the startup code in `main.rs`.
    #[serde(default)]
    pub agent_api_key: String,

    /// Registered API keys with permission tiers.
    ///
    /// An empty vec means auth is disabled (open access).
    /// Use `#[serde(default)]` so old profiles without this field load as empty.
    #[serde(default)]
    pub agent_api_keys: Vec<StoredApiKey>,

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

    /// Whether to send anonymized usage metrics to chart.example.
    /// Defaults to `true` (opt-in).  Requires Connected mode to have any effect.
    /// Changing this at runtime sends `UpdaterCommand::SetTelemetryEnabled`.
    #[serde(default = "default_true")]
    pub telemetry_enabled: bool,

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
}

// =============================================================================
// SyncCategoryPrefs
// =============================================================================

/// Per-category cloud sync preferences.
///
/// Each field controls whether items of that category are included in the
/// sync cycle.  All fields default to `true` (opt-in by category) so that
/// new installs sync everything unless the user explicitly disables a category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncCategoryPrefs {
    /// Whether chart presets are included in cloud sync.
    #[serde(default = "default_true")]
    pub presets: bool,
    /// Whether watchlists are included in cloud sync.
    #[serde(default = "default_true")]
    pub watchlists: bool,
    /// Whether indicator/primitive templates are included in cloud sync.
    #[serde(default = "default_true")]
    pub templates: bool,
    /// Whether settings snapshots are included in cloud sync.
    #[serde(default = "default_true")]
    pub settings_snapshots: bool,
    /// Whether the active theme identifier is included in cloud sync.
    #[serde(default = "default_true")]
    pub theme: bool,
    /// Whether notification/alert delivery settings are included in cloud sync.
    #[serde(default = "default_true")]
    pub notification_settings: bool,
}

impl Default for SyncCategoryPrefs {
    fn default() -> Self {
        Self {
            presets: true,
            watchlists: true,
            templates: true,
            settings_snapshots: true,
            theme: true,
            notification_settings: true,
        }
    }
}

// =============================================================================
// SyncState
// =============================================================================

/// Minimal state required to perform incremental cloud sync.
///
/// Stored inside [`UserProfile`] so it is automatically persisted to and
/// loaded from `profile.json` via the existing save/load infrastructure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncState {
    /// Unix timestamp (seconds) of the last successful sync.
    /// `0` means the client has never completed a sync.
    #[serde(default)]
    pub last_sync_timestamp: i64,
    /// Whether the user has opted into cloud sync.
    #[serde(default)]
    pub enabled: bool,
    /// Whether the user has enabled E2E encryption for sync data.
    ///
    /// When `true`, all sync item content is encrypted client-side before
    /// being sent to the server.  The server stores only opaque ciphertext.
    #[serde(default)]
    pub e2e_enabled: bool,
    /// Hex-encoded 16-byte PBKDF2 salt, fetched from the server after the
    /// user sets up E2E.  Empty string means E2E has not been configured.
    ///
    /// This value is safe to persist locally — without the passphrase it
    /// provides no useful information to an attacker.
    #[serde(default)]
    pub e2e_salt: String,
    /// Per-category sync preferences — which data categories to include in sync.
    #[serde(default)]
    pub category_prefs: SyncCategoryPrefs,
    /// Set of sync item keys (`"category:sync_id"`) that have been successfully
    /// pushed to the server at least once.
    ///
    /// Used for tombstone detection: if an item disappears from local storage
    /// but is present in this set, a tombstone (`deleted: true`) is pushed on
    /// the next sync cycle so the server marks the item as deleted.
    #[serde(default)]
    pub synced_items: std::collections::HashSet<String>,
    /// When `true` and E2E is enabled, the `name` field of each `SyncItem` is
    /// also encrypted before being sent to the server.  Defaults to `false`
    /// (names are sent in plaintext) for backward compatibility.
    #[serde(default)]
    pub sync_e2e_encrypt_names: bool,
}

impl UserProfile {
    /// Create a new profile with sensible defaults.
    pub fn new() -> Self {
        Self {
            version: PROFILE_VERSION,
            client_mode: ClientMode::default(),
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
            server_enabled: default_server_enabled(),
            server_port: default_server_port(),
            agent_api_key: String::new(),
            agent_api_keys: Vec::new(),
            exchange_keys: Vec::new(),
            connector_enabled: std::collections::HashMap::new(),
            telemetry_enabled: false,
            telemetry: TelemetryData::default(),
            notification_settings: alert_delivery::NotificationSettings::default(),
            windows: Vec::new(),
            sync_state: SyncState::default(),
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
// StoredApiKey — persisted API key entry (mirrors ApiKeyEntry in zengeld-server)
// =============================================================================

/// Persisted representation of a single API key entry.
///
/// This type mirrors [`zengeld_server::state::ApiKeyEntry`] but lives in the
/// `chart` crate so that `profile.rs` stays independent of `zengeld-server`.
/// Conversion to the server type happens in `chart-app-vello/src/main.rs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredApiKey {
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
    /// Key origin: `"local"` (never removed by cloud sync) or `"cloud"`.
    ///
    /// Uses `String` instead of an enum so the JSON stays human-readable and
    /// backward-compatible.  Missing field defaults to `"local"` so that keys
    /// loaded from older profile.json files are never accidentally evicted.
    #[serde(default = "default_key_source")]
    pub source: String,
}

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

fn default_server_enabled() -> bool {
    true
}

fn default_server_port() -> u16 {
    17420
}
