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
    // Connector enable/disable state
    // -------------------------------------------------------------------------

    /// Per-connector enabled/disabled toggle state.
    /// Key: exchange id string (e.g. `"binance"`, `"bybit"`).
    /// Value: `true` = enabled (default when absent), `false` = disabled.
    #[serde(default)]
    pub connector_enabled: std::collections::HashMap<String, bool>,

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
            server_enabled: default_server_enabled(),
            server_port: default_server_port(),
            agent_api_key: String::new(),
            agent_api_keys: Vec::new(),
            connector_enabled: std::collections::HashMap::new(),
            telemetry: TelemetryData::default(),
            notification_settings: alert_delivery::NotificationSettings::default(),
            windows: Vec::new(),
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
