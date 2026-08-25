//! [`UserProfile`] — top-level persistent metadata for a user session.
//!
//! Stores active selections and UI state.  All heavy data (presets, templates,
//! watchlists) live in their own files alongside this one; see
//! [`crate::user_profile::storage`] for the directory layout and generic I/O
//! helpers.

use serde::{Deserialize, Serialize};

use crate::drawing::primitives_v2::{ControlPointShape, ControlPointSize};
use crate::state::ewa_engine::EwaEngineConfig;
use crate::state::candle_pattern_engine::CandlePatternEngineConfig;
use crate::state::smc_engine::SmcEngineConfig;

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
    /// OpenCode / sst-opencode (PIPE transport).
    OpenCode,
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

    /// Ring capacity (points) for one bubble's raw kline layer, per
    /// touched timeframe (unified-data-plane Stage Э1). Default 200_000.
    #[serde(default = "default_kline_layer_capacity")]
    pub kline_layer_capacity: u32,

    /// Raw-trade retention window in UTC days (owner rule 2026-07-15):
    /// the shared `TradeStore` keeps raw trades no deeper than this many
    /// days behind its newest stored trade — THE single raw-trade memory
    /// bound (it replaced both the 1M count ring and the engine's 1.5M
    /// cumulative RAM cap, which were three uncoordinated limits in
    /// different units). Depth beyond the window: zip-covered days are
    /// fetched, folded, and their raw rows dropped by this same eviction
    /// (re-pullable from the immutable archive on refold); venues with no
    /// archive stop fetching at the window and latch dry. Default 4.
    #[serde(default = "default_trade_raw_window_days")]
    pub trade_raw_window_days: u32,

    /// Target depth (1m bars) the `BackfillEngine` ramps the shared "1m"
    /// kline layer toward for the 7 client-side fold chart types
    /// (range_bar/volume_bar/dollar_bar/renko/pnf/kagi/three_line_break —
    /// unified-data-plane Stage Э3). Default 10_000 (~6.9 days of 1m bars).
    #[serde(default = "default_derived_kline_target")]
    pub derived_kline_target: u32,

    /// LEGACY / UNUSED BY THE RAMP (owner doctrine 2026-07-12): the trade
    /// layer's `BackfillEngine` ramp goal is now a TIME LADDER (last hour →
    /// last 12 hours → last day → day-by-day on scroll demand, see
    /// `mlc_backend_data_bridge::backfill_engine::TradeLadderStage`), not a
    /// point-count target — this field is no longer read by
    /// `subscribe_trade_fold_chart_type`/`subscribe_cvd_fold`'s
    /// `ensure_trade_backfill` call. Kept in place (not removed) purely so
    /// already-persisted profiles round-trip through (de)serialization
    /// without a schema break; do not wire it into any new ramp-goal logic.
    /// Default 10_000 trades. `#[serde(default)]` — older profiles missing
    /// this field pick up the same default as a fresh profile.
    #[serde(default = "default_derived_trade_target")]
    pub derived_trade_target: u32,

    /// CORS proxy base URL for wasm archive day-file fetches
    /// (unified-data-plane Э6-Ф4). Browsers cannot cross-origin GET the
    /// bulk trade-archive hosts (`public.bybit.com`, `data.binance.vision`)
    /// directly — the wasm build routes those requests through this proxy
    /// in prefix mode (`{archive_proxy_base}/{host}/{path}`). Native
    /// fetches ignore this field entirely (direct HTTPS, no CORS concern
    /// off-browser). Default `http://127.0.0.1:17800` (the local
    /// `digdigdig3-proxy` dev instance — configurable, owner doctrine).
    /// `#[serde(default)]` — older profiles missing this field pick up the
    /// same default as a fresh profile.
    #[serde(default = "default_archive_proxy_base")]
    pub archive_proxy_base: String,
}

impl Default for DataLoadSettings {
    fn default() -> Self {
        Self {
            background_bar_count: default_background_bar_count(),
            max_loaded_bars: default_max_loaded_bars(),
            max_store_size_mb: default_max_store_size_mb(),
            store_cleanup_days: default_store_cleanup_days(),
            kline_layer_capacity: default_kline_layer_capacity(),
            trade_raw_window_days: default_trade_raw_window_days(),
            derived_kline_target: default_derived_kline_target(),
            derived_trade_target: default_derived_trade_target(),
            archive_proxy_base: default_archive_proxy_base(),
        }
    }
}

// =============================================================================
// GranularityFavorites — user-saved CUSTOM TF / param values + STARRED rows
// =============================================================================
//
// Two separate concepts (user-granularity-templates-inner arc, кусок 2 —
// owner spec 2026-07-16 replaces кусок 1's single-list semantics):
//
// - CUSTOM = a user-created value (a TF name like "7m", or a numeric param
//   per chart type) that does not exist in the builtin ladder. Lives in the
//   dropdown's main list, editable and deletable. Struct/field names below
//   are UNCHANGED from кусок 1 for on-disk compat — a profile saved under
//   the old "favorites" semantics loads its `time_tfs`/`params` entries as
//   customs today, which is the correct migration (an old favorite WAS a
//   user-typed custom value).
// - STARRED = a star on ANY row — builtin OR custom (watchlist-style star).
//   Starred rows additionally surface in a "FAVORITES" header section at
//   the top of the dropdown. New fields, additive, `#[serde(default)]` so
//   old profiles deserialize with empty star lists.

/// One saved numeric-parameter CUSTOM value for a `ResolutionKind::Param`
/// chart type (range/tick/volume/dollar/imbalance/run bars, renko/pnf/kagi/
/// three_line_break) — user-granularity-templates-inner arc.
///
/// `chart_type` is the registry id (e.g. `"range_bar"`); `value` is the raw
/// `chart_type_param` f64, unclamped (mirrors `ChartWindowSnapshot::
/// chart_type_param`'s existing "free f64, no ladder" contract — customs
/// add a discovery/reuse UI on top, not a new value constraint).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParamFavorite {
    pub chart_type: String,
    pub value: f64,
}

/// User-saved custom granularity values plus starred rows
/// (user-granularity-templates-inner arc): global custom timeframes and
/// per-chart-type numeric CUSTOM values, both minted from the toolbar's
/// TF/param dropdown "+ Add" row; plus independently-starred TF/param rows
/// (builtin or custom) surfaced in a "FAVORITES" dropdown section.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct GranularityFavorites {
    /// CUSTOM timeframe names (e.g. `["7m", "90m"]`) — global, offered to
    /// every chart type whose `resolution == ResolutionKind::Time` and
    /// `tf_whitelist == None` (never footprint/tpo). Each entry must be a
    /// string [`crate::state::Timeframe::parse`] accepts.
    #[serde(default)]
    pub time_tfs: Vec<String>,

    /// CUSTOM numeric param values, one entry per (chart_type, value) pair.
    #[serde(default)]
    pub params: Vec<ParamFavorite>,

    /// Starred TF names (e.g. `["1m", "7m"]`) — may reference a builtin
    /// (`ALL_TF_IDS`) or a `time_tfs` custom. A starred row surfaces in the
    /// "FAVORITES" header section in addition to its normal list position.
    #[serde(default)]
    pub starred_tfs: Vec<String>,

    /// Starred numeric param values, one entry per (chart_type, value) pair
    /// — may reference a builtin ladder preset or a `params` custom.
    #[serde(default)]
    pub starred_params: Vec<ParamFavorite>,

    /// CUSTOM footprint bucket-interval names (e.g. `["7m", "90s"]`) —
    /// per-family scoping arc 2026-07-16 (owner: "для футпринта вообще не
    /// вижу ни кастома ни фейворита"). Global, flat list — mirrors
    /// `time_tfs`'s shape exactly (footprint is the only
    /// `ResolutionKind::Interval` chart type today, so no chart_type
    /// scoping is needed), but kept in its OWN bucket, never merged into
    /// `time_tfs`: a footprint bucket interval and a kline candle
    /// timeframe are different concepts sharing the same string grammar
    /// (`Timeframe::parse`), and must not cross-render on each other's
    /// toolbar/dropdown (owner: "остался запинен 1m на футпринте — тупо").
    #[serde(default)]
    pub interval_tfs: Vec<String>,

    /// Starred footprint bucket-interval names — may reference a builtin
    /// whitelist entry (`FOOTPRINT_TF_WHITELIST`) or an `interval_tfs`
    /// custom. See `interval_tfs` doc comment for the separate-bucket
    /// rationale.
    #[serde(default)]
    pub starred_intervals: Vec<String>,
}

// =============================================================================
// ProfileDomainSettings
// =============================================================================

/// Domain half of [`UserProfile`] — settings a headless daemon-CLI consumer
/// needs to work CORRECTLY, not just prettily (doctrine §1B test,
/// `docs/mlc/plans/doctrine-cutover-2026-06-27.md`).
///
/// `#[serde(flatten)]`ed into `UserProfile` so `profile.json`'s on-disk
/// layout stays byte-for-byte flat — this is a type-clarity split, not a
/// format change (verified by `json_compat_tests::round_trips_current_shape`
/// below) and not an ownership move: `UserProfileManager` (backend) still
/// owns the whole `UserProfile`, domain and UI halves together. The UI half
/// moves to `mlc-app` only if/when the shared-core arc lifts app-only state
/// out of the backend-owned profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileDomainSettings {
    /// Schema version.  Used for forward-compatible migration.
    #[serde(default = "default_version")]
    pub version: u32,

    /// App version at the time of last launch. Used for migration/diagnostic
    /// gating, not just display.
    #[serde(default)]
    pub app_version: String,

    /// Number of historical bars to load on chart open. Drives the initial
    /// `request_bars` fetch size — a headless consumer needs this to fetch
    /// the correct amount of data.
    #[serde(default = "default_bar_count")]
    pub bar_count: u16,

    /// Indicator recalculation mode.
    ///
    /// Valid values: `"PerTick"`, `"PerFrame"`, `"PerBar"`. Drives
    /// recalculation semantics — correctness, not presentation. Defaults to
    /// `"PerFrame"` when the field is absent (older profiles).
    #[serde(default = "default_recalc_mode")]
    pub recalc_mode: String,

    /// Whether the internal Agent API server is enabled on startup.
    #[serde(default = "default_server_enabled")]
    pub server_enabled: bool,

    /// Port for the Agent API server.
    #[serde(default = "default_server_port")]
    pub server_port: u16,

    /// Persisted exchange API key entries.
    ///
    /// Each entry holds the public API key and either a plaintext secret
    /// (legacy) or a [`CredentialRef`] pointing to the OS keychain (preferred).
    /// An empty vec means no exchange credentials have been saved yet.
    #[serde(default)]
    pub exchange_keys: Vec<StoredExchangeKey>,

    /// Per-connector enabled/disabled toggle state.
    /// Key: exchange id string (e.g. `"binance"`, `"bybit"`).
    /// Value: `true` = enabled (default when absent), `false` = disabled.
    /// Drives which connectors the data bridge dials — functional.
    #[serde(default)]
    pub connector_enabled: std::collections::HashMap<String, bool>,

    /// Alert notification delivery settings (Telegram bot, toasts, sound,
    /// webhook). Functional — a daemon needs this to deliver alerts
    /// correctly, not just to show a settings form.
    #[serde(default)]
    pub notification_settings: mlc_alert_delivery::NotificationSettings,

    /// Unique profile identifier (UUID v4). Empty on legacy profiles.
    ///
    /// Classified domain conservatively (§1B: unclear -> domain): directory
    /// resolution today goes through the separate `ProfileIndex` /
    /// `ProfileMeta` file, not this field, but self-identity consistency is
    /// correctness-adjacent enough that presentation felt like the wrong
    /// default.
    #[serde(default)]
    pub profile_id: String,

    /// Historical bar data fetch and retention settings. Directly drives
    /// bridge fetch/eviction behaviour — correctness.
    #[serde(default)]
    pub data_load: DataLoadSettings,

    /// User-saved CUSTOM granularity values plus STARRED rows
    /// (user-granularity-templates-inner arc): custom timeframes (e.g. "7m",
    /// "90m") and per-chart-type numeric CUSTOM values, both minted from the
    /// toolbar's TF/param dropdown "+ Add" row; plus independently-starred
    /// builtin/custom TF and param rows. Functional, not cosmetic — a
    /// headless consumer replaying a saved custom/starred TF/param needs
    /// these to resolve the same value the desktop UI offered.
    #[serde(default)]
    pub granularity_favorites: GranularityFavorites,
}

impl ProfileDomainSettings {
    fn new() -> Self {
        Self {
            version: PROFILE_VERSION,
            app_version: String::new(),
            bar_count: default_bar_count(),
            recalc_mode: default_recalc_mode(),
            server_enabled: default_server_enabled(),
            server_port: default_server_port(),
            exchange_keys: Vec::new(),
            connector_enabled: std::collections::HashMap::new(),
            notification_settings: mlc_alert_delivery::NotificationSettings::default(),
            profile_id: String::new(),
            data_load: DataLoadSettings::default(),
            granularity_favorites: GranularityFavorites::default(),
        }
    }
}

impl Default for ProfileDomainSettings {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// ProfileUiSettings
// =============================================================================

/// Presentation half of [`UserProfile`] — desktop-shell-only UI instance
/// state. Zero consumers in the headless/daemon shells today (verified by
/// audit — none of these fields are read outside `mlc-app-shell-vello`).
///
/// `#[serde(flatten)]`ed into `UserProfile` for byte-identical on-disk
/// layout — see [`ProfileDomainSettings`] doc for the split rationale.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileUiSettings {
    /// ID of the chart preset that was last active (`"preset_{ts}_{nanos}"`).
    /// Legacy top-level mirror of `windows[0].active_preset_id`; empty
    /// string means no preset is active (use default state). No daemon
    /// consumer — desktop tab-bar bookkeeping only.
    #[serde(default)]
    pub active_preset_id: String,

    /// Ordered list of preset IDs open as tabs (persisted across sessions).
    /// Same legacy-mirror status as `active_preset_id`.
    #[serde(default)]
    pub open_tabs: Vec<String>,

    /// Name of the active theme (e.g. `"dark"`, `"light"`). Pure rendering
    /// cosmetics.
    #[serde(default = "default_theme")]
    pub active_theme: String,

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

    /// Horizontal offset of the floating inline toolbar.
    #[serde(default)]
    pub inline_bar_x: Option<f64>,

    /// Vertical offset of the floating inline toolbar.
    #[serde(default)]
    pub inline_bar_y: Option<f64>,

    /// Dock edge of the floating inline toolbar ("Bottom", "Top", "Free").
    #[serde(default)]
    pub inline_bar_dock: Option<String>,

    /// Human-readable device name (auto-detected or user-set).
    /// e.g. "VA-PC-WIN10", "MacBook Pro" — cosmetic label in settings UI.
    #[serde(default)]
    pub device_name: String,

    /// UI language preference (ISO 639-1 code: "en", "ru"). Display-only —
    /// selects an i18n string table, doesn't change behaviour.
    #[serde(default = "default_language")]
    pub language: String,

    /// Per-window tab/preset state.  When non-empty, each entry describes one
    /// OS window.  The first entry (`windows[0]`) is the primary window.
    /// Whole type is desktop-multi-window bookkeeping — zero concept in a
    /// headless/daemon shell (audited: no consumer outside
    /// `mlc-app-shell-vello`), so `WindowState`'s own domain-ish fields
    /// (`active_preset_id`, `open_tabs`) are NOT split out further — they
    /// only exist in service of desktop tab management.
    #[serde(default)]
    pub windows: Vec<WindowState>,

    /// User-visible profile name ("Default", "Trading", "Debug", etc.) —
    /// cosmetic profile-picker label.
    #[serde(default = "default_profile_name")]
    pub display_name: String,

    /// Avatar emoji key ("chart", "rocket", "shield", "fire", "star", "moon",
    /// "sun", "ghost") — cosmetic profile-picker icon.
    #[serde(default = "default_avatar")]
    pub avatar: String,

    /// Profile creation timestamp (unix seconds) — cosmetic metadata
    /// (sort/display in the profile picker).
    #[serde(default)]
    pub profile_created_at: i64,

    /// When `true`, starred granularity entries (starred TF names for
    /// `Time`-resolution chart types / starred numeric params for the
    /// active `Param`-resolution chart type) additionally render as a row
    /// of compact quick-pick buttons on the top toolbar, right after the
    /// `timeframe_selector` button — TradingView "pinned intervals" style
    /// (user-granularity-templates-inner arc, кусок 3 — owner spec
    /// 2026-07-16). `false` (default) keeps today's behaviour: starred
    /// entries only surface in the dropdown's FAVORITES section. Purely a
    /// toolbar-layout presentation toggle — no daemon/headless consumer
    /// reads this.
    #[serde(default)]
    pub pin_granularity_favorites: bool,

    /// When `true`, the top toolbar keeps `symbol_selector` and `compare`
    /// as two SEPARATE items (today's look). `false` (the new default)
    /// merges them: the standalone `compare` icon button is dropped from
    /// the control strip and its function is reached via the merged
    /// `symbol_selector` fast-search dropdown instead
    /// (symbol-search-compare-inner arc, Wave 2, кусок 3 — owner spec
    /// 2026-07-16). Purely a toolbar-layout presentation toggle — no
    /// daemon/headless consumer reads this.
    #[serde(default)]
    pub split_symbol_compare: bool,

    /// When `true` (the default), clicking `symbol_selector` starts the
    /// fast-search IN PLACE: the button's text becomes an editor seeded
    /// with the active instrument's composite key, and a dropdown with
    /// live results opens below (2026-07-17 rework — this button-hosted
    /// editor is the ONLY quick-search shape; the former separate
    /// dropdown-hosted input mode and its `symbol_inline_input` flag are
    /// deleted). `false`: click opens `ChartOutEvent::OpenSymbolSearch`'s
    /// full-screen modal directly. Purely a toolbar-layout presentation
    /// toggle — no daemon/headless consumer reads this.
    #[serde(default = "default_true")]
    pub symbol_quick_search: bool,

    /// How a standalone panel's overlay gear opens its settings surface
    /// (owner spec 2026-07-28): `true` = small centered modal, `false` =
    /// anchored dropdown. Toggled from inside the surface itself and
    /// persisted — same presentation-preference class as
    /// `symbol_quick_search`.
    #[serde(default)]
    pub panel_gear_opens_modal: bool,

    /// When `true`, the top toolbar's composition swaps to the Trading
    /// tab's container while it is the current workspace focus: a symbol
    /// selector targeting the container, its exchange/account_type display,
    /// and chart-semantic controls (timeframe picker, chart-type selector,
    /// indicators button, ...) suppressed — global controls (settings,
    /// undo/redo, screenshot, ...) stay. `false` (default) keeps today's
    /// behavior: the topbar always derives from the active chart, and the
    /// Trading tab's own header row carries its one control (a symbol-search
    /// button) instead (workspace-panel arc, Stage 4 — owner spec
    /// 2026-07-25). Purely a toolbar-layout presentation toggle — no
    /// daemon/headless consumer reads this.
    #[serde(default)]
    pub trading_panel_topbar_takeover: bool,

    /// Visual shape of every drawing primitive's control-point (handle)
    /// marker — a process-wide rendering preference, same class as
    /// `active_theme` (one look for every chart/window, not per-window
    /// state). Applied to the live global via
    /// `mlc_core::drawing::primitives_v2::set_control_point_style` at
    /// profile load (`ChartApp::apply_profile_state`) and again on every
    /// User Settings General-tab pick (control-point-style-ui arc,
    /// 2026-08-08). Mutated in-place at pick time, not by
    /// `build_user_profile`'s snapshot — same preserve-live-value pattern
    /// as `trading_panel_topbar_takeover` above. No daemon/headless
    /// consumer reads this (rendering-only).
    #[serde(default)]
    pub control_point_shape: ControlPointShape,

    /// Visual/hit-test size of every drawing primitive's control-point
    /// (handle) marker — see `control_point_shape`'s doc above for the full
    /// rationale; same process-wide, mutated-in-place, rendering-only class.
    #[serde(default)]
    pub control_point_size: ControlPointSize,

    /// EWA (Elliott Wave Analysis) engine configuration — the "EWA Engine
    /// Settings" modal's own edited value (owner ask 2026-08-14). Same
    /// process-wide, mutated-in-place, RAM+profile class as
    /// `control_point_shape`/`control_point_size` above: applied to the live
    /// global via `mlc_core::state::ewa_engine::set_ewa_engine_config` at
    /// profile load (`ChartApp::apply_profile_state`) and again on every
    /// modal edit. No daemon/headless consumer reads this (chart-rendering
    /// and drawing-construction only).
    #[serde(default)]
    pub ewa_engine: EwaEngineConfig,

    /// Candle-pattern engine configuration (markup-engines wave,
    /// 2026-08-25). Same process-wide, mutated-in-place, RAM+profile class
    /// as `ewa_engine` above: applied to the live global via
    /// `mlc_core::state::candle_pattern_engine::set_candle_pattern_engine_config`
    /// at profile load (`ChartApp::apply_profile_state`). No settings-modal
    /// UI edits this yet — see `state::candle_pattern_engine`'s own module
    /// doc for the two fields' own readers.
    #[serde(default)]
    pub candle_pattern_engine: CandlePatternEngineConfig,

    /// Smart-money engine configuration (markup-engines wave, 2026-08-25).
    /// Same process-wide, mutated-in-place, RAM+profile class as
    /// `candle_pattern_engine` above: applied to the live global via
    /// `mlc_core::state::smc_engine::set_smc_engine_config` at profile load
    /// (`ChartApp::apply_profile_state`). No settings-modal UI edits this
    /// yet — see `state::smc_engine`'s own module doc for every field's own
    /// reader.
    #[serde(default)]
    pub smc_engine: SmcEngineConfig,
}

fn default_true() -> bool {
    true
}

impl ProfileUiSettings {
    fn new() -> Self {
        Self {
            active_preset_id: String::new(),
            open_tabs: Vec::new(),
            active_theme: default_theme(),
            sidebar_visible: false,
            sidebar_panel: None,
            sidebar_width: None,
            inline_bar_x: None,
            inline_bar_y: None,
            inline_bar_dock: None,
            device_name: String::new(),
            language: default_language(),
            windows: Vec::new(),
            display_name: "Default".to_string(),
            avatar: "chart".to_string(),
            profile_created_at: 0,
            pin_granularity_favorites: false,
            split_symbol_compare: false,
            symbol_quick_search: true,
            panel_gear_opens_modal: false,
            trading_panel_topbar_takeover: false,
            control_point_shape: ControlPointShape::default(),
            control_point_size: ControlPointSize::default(),
            ewa_engine: EwaEngineConfig::default(),
            candle_pattern_engine: CandlePatternEngineConfig::default(),
            smc_engine: SmcEngineConfig::default(),
        }
    }
}

impl Default for ProfileUiSettings {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// UserProfile
// =============================================================================

/// Top-level persistent state for a user session.
///
/// Split into [`ProfileDomainSettings`] (`domain`) and [`ProfileUiSettings`]
/// (`ui`) per doctrine §1B — see their docs for the field classification.
/// Both halves are `#[serde(flatten)]`ed so `profile.json` keeps its
/// pre-split flat shape; deserializing this struct should never fail due to
/// missing fields (every leaf field uses `#[serde(default)]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    /// Domain settings — needed for a headless consumer to behave correctly.
    #[serde(flatten)]
    pub domain: ProfileDomainSettings,

    /// UI/presentation settings — desktop-shell instance state.
    #[serde(flatten)]
    pub ui: ProfileUiSettings,
}

impl UserProfile {
    /// Create a new profile with sensible defaults.
    pub fn new() -> Self {
        Self {
            domain: ProfileDomainSettings::new(),
            ui: ProfileUiSettings::new(),
        }
    }

    /// Record a new app launch (update app_version field).
    pub fn record_launch(&mut self, app_version: &str) {
        self.domain.app_version = app_version.to_string();
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
    /// Exchange API key entries (contains `api_secret` and `passphrase`).
    #[serde(default)]
    pub exchange_keys: Vec<StoredExchangeKey>,

    /// Alert notification delivery settings.
    ///
    /// Stored here because it contains `telegram.bot_token`,
    /// `telegram.subscribers` (PII — chat IDs), and `webhook.url`
    /// (may embed auth tokens).
    #[serde(default)]
    pub notification_settings: mlc_alert_delivery::NotificationSettings,
}

impl VaultSecrets {
    /// Extract credential fields from a [`UserProfile`], returning a
    /// `VaultSecrets` and clearing those fields in the profile so they
    /// are not written to plaintext storage.
    pub fn extract_from(profile: &mut UserProfile) -> Self {
        Self {
            exchange_keys: std::mem::take(&mut profile.domain.exchange_keys),
            notification_settings: std::mem::take(&mut profile.domain.notification_settings),
        }
    }

    /// Merge secrets back into a [`UserProfile`] after decrypting from vault.
    pub fn merge_into(self, profile: &mut UserProfile) {
        profile.domain.exchange_keys = self.exchange_keys;
        profile.domain.notification_settings = self.notification_settings;
        profile.domain.notification_settings.telegram.migrate_legacy();
    }
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

fn default_kline_layer_capacity() -> u32 {
    200_000
}

fn default_trade_raw_window_days() -> u32 {
    4
}

fn default_derived_kline_target() -> u32 {
    10_000
}

fn default_derived_trade_target() -> u32 {
    10_000
}

/// Public because the CORS proxy is needed BEFORE a profile has been applied:
/// the first connector dial can happen while the workspace is still loading,
/// and a venue dialled without the override keeps its direct base for the
/// life of the connector (the hub reads the override map at construction).
/// Callers that need the base early take this default and let a loaded
/// profile override it afterwards.
pub fn default_archive_proxy_base() -> String {
    "http://127.0.0.1:17800".to_string()
}

// =============================================================================
// Persistence fixture gates — UserProfile domain/ui split (2026-07-02)
// =============================================================================

#[cfg(test)]
mod json_compat_tests {
    //! `UserProfile` was split into `domain: ProfileDomainSettings` +
    //! `ui: ProfileUiSettings`, both `#[serde(flatten)]`ed. This is a
    //! type-clarity split, not a format change: `profile.json` must stay
    //! byte-for-byte flat before and after.
    //!
    //! (1) proves a legacy (pre-split) flat `profile.json` — where every
    //! field sat directly on `UserProfile` — still deserializes correctly
    //! into the new nested `domain` / `ui` shape.
    //! (2) proves the new struct still serializes back to a flat shape (no
    //! `"domain": {...}` / `"ui": {...}` nesting on disk).

    use super::*;

    /// Literal fixture: a pre-split `profile.json` exactly as it existed
    /// before this commit — every field flat on the top-level object.
    const LEGACY_PROFILE_JSON: &str = r#"{
        "version": 3,
        "active_preset_id": "preset_123_456",
        "open_tabs": ["preset_123_456"],
        "active_theme": "dark",
        "sidebar_visible": true,
        "sidebar_panel": "watchlist",
        "sidebar_width": 340.0,
        "inline_bar_x": 100.0,
        "inline_bar_y": 200.0,
        "inline_bar_dock": "Bottom",
        "device_name": "VA-PC-WIN10",
        "app_version": "0.5.100",
        "bar_count": 2000,
        "recalc_mode": "PerFrame",
        "scale_mode": "Auto",
        "language": "en",
        "server_enabled": true,
        "server_port": 17420,
        "exchange_keys": [],
        "connector_enabled": {"binance": true},
        "notification_settings": {},
        "windows": [],
        "profile_id": "profile-uuid-1",
        "display_name": "Default",
        "avatar": "chart",
        "profile_created_at": 1700000000,
        "data_load": {
            "background_bar_count": 2000,
            "max_loaded_bars": 10000,
            "max_store_size_mb": 500,
            "store_cleanup_days": 30
        }
    }"#;

    #[test]
    fn legacy_flat_profile_deserializes_into_domain_ui_split() {
        let profile: UserProfile = serde_json::from_str(LEGACY_PROFILE_JSON)
            .expect("legacy flat profile.json must deserialize");

        // Domain fields landed on `domain`.
        assert_eq!(profile.domain.version, 3);
        assert_eq!(profile.domain.app_version, "0.5.100");
        assert_eq!(profile.domain.bar_count, 2000);
        assert_eq!(profile.domain.recalc_mode, "PerFrame");
        assert!(profile.domain.server_enabled);
        assert_eq!(profile.domain.server_port, 17420);
        assert_eq!(profile.domain.connector_enabled.get("binance"), Some(&true));
        assert_eq!(profile.domain.profile_id, "profile-uuid-1");
        assert_eq!(profile.domain.data_load.background_bar_count, 2000);
        // Fixture predates `granularity_favorites` entirely — must default
        // to empty, never fail deserialization (kusok 1 regression gate).
        assert!(profile.domain.granularity_favorites.time_tfs.is_empty());
        assert!(profile.domain.granularity_favorites.params.is_empty());
        // Fixture also predates `starred_tfs`/`starred_params` (kusok 2) —
        // same empty-default regression gate.
        assert!(profile.domain.granularity_favorites.starred_tfs.is_empty());
        assert!(profile.domain.granularity_favorites.starred_params.is_empty());
        // Fixture also predates `interval_tfs`/`starred_intervals`
        // (per-family scoping arc 2026-07-16) — same empty-default
        // regression gate.
        assert!(profile.domain.granularity_favorites.interval_tfs.is_empty());
        assert!(profile.domain.granularity_favorites.starred_intervals.is_empty());

        // UI fields landed on `ui`.
        assert_eq!(profile.ui.active_preset_id, "preset_123_456");
        assert_eq!(profile.ui.open_tabs, vec!["preset_123_456".to_string()]);
        assert_eq!(profile.ui.active_theme, "dark");
        assert!(profile.ui.sidebar_visible);
        assert_eq!(profile.ui.sidebar_panel.as_deref(), Some("watchlist"));
        assert_eq!(profile.ui.sidebar_width, Some(340.0));
        assert_eq!(profile.ui.inline_bar_dock.as_deref(), Some("Bottom"));
        assert_eq!(profile.ui.device_name, "VA-PC-WIN10");
        assert_eq!(profile.ui.language, "en");
        assert_eq!(profile.ui.display_name, "Default");
        assert_eq!(profile.ui.avatar, "chart");
        assert_eq!(profile.ui.profile_created_at, 1700000000);
        // Fixture predates `pin_granularity_favorites` (кусок 3) — must
        // default to false, never fail deserialization.
        assert!(!profile.ui.pin_granularity_favorites);
    }

    #[test]
    fn round_trips_flat_on_disk_shape() {
        let profile: UserProfile = serde_json::from_str(LEGACY_PROFILE_JSON)
            .expect("legacy flat profile.json must deserialize");

        let json = serde_json::to_string(&profile).expect("profile must serialize");

        // The flatten must keep the shape flat — no nested "domain"/"ui" keys
        // on disk, and every field name from the legacy shape still present
        // as a direct top-level key.
        assert!(!json.contains("\"domain\":{"), "domain must not nest: {json}");
        assert!(!json.contains("\"ui\":{"), "ui must not nest: {json}");
        for key in [
            "\"version\"", "\"active_preset_id\"", "\"open_tabs\"", "\"active_theme\"",
            "\"sidebar_visible\"", "\"sidebar_panel\"", "\"sidebar_width\"",
            "\"inline_bar_x\"", "\"inline_bar_y\"", "\"inline_bar_dock\"",
            "\"device_name\"", "\"app_version\"", "\"bar_count\"", "\"recalc_mode\"",
            "\"language\"", "\"server_enabled\"", "\"server_port\"",
            "\"exchange_keys\"", "\"connector_enabled\"", "\"notification_settings\"",
            "\"windows\"", "\"profile_id\"", "\"display_name\"", "\"avatar\"",
            "\"profile_created_at\"", "\"data_load\"", "\"granularity_favorites\"",
        ] {
            assert!(json.contains(key), "missing flat key {key} in {json}");
        }

        // Full round-trip: deserialize what we just wrote and compare.
        let restored: UserProfile = serde_json::from_str(&json).expect("must deserialize back");
        assert_eq!(restored.domain.profile_id, profile.domain.profile_id);
        assert_eq!(restored.ui.active_preset_id, profile.ui.active_preset_id);
        assert_eq!(restored.domain.bar_count, profile.domain.bar_count);
    }

    #[test]
    fn new_profile_defaults_round_trip() {
        let profile = UserProfile::new();
        let json = serde_json::to_string(&profile).expect("must serialize");
        let restored: UserProfile = serde_json::from_str(&json).expect("must deserialize");
        assert_eq!(restored.domain.version, profile.domain.version);
        assert_eq!(restored.ui.display_name, profile.ui.display_name);
    }

    #[test]
    fn granularity_favorites_populated_round_trip() {
        let mut profile = UserProfile::new();
        profile.domain.granularity_favorites.time_tfs = vec!["7m".to_string(), "90m".to_string()];
        profile.domain.granularity_favorites.params = vec![
            ParamFavorite { chart_type: "range_bar".to_string(), value: 17.0 },
            ParamFavorite { chart_type: "volume_bar".to_string(), value: 12.5 },
        ];
        profile.domain.granularity_favorites.starred_tfs = vec!["1m".to_string(), "7m".to_string()];
        profile.domain.granularity_favorites.starred_params = vec![
            ParamFavorite { chart_type: "range_bar".to_string(), value: 8.0 },
        ];
        profile.domain.granularity_favorites.interval_tfs = vec!["7m".to_string(), "90s".to_string()];
        profile.domain.granularity_favorites.starred_intervals = vec!["5m".to_string()];

        let json = serde_json::to_string(&profile).expect("must serialize");
        // `granularity_favorites` is itself a nested object (like `data_load`)
        // — the flatten contract only requires it sit directly on the
        // top-level object, not nested under "domain"/"ui".
        assert!(json.contains("\"granularity_favorites\":{"), "granularity_favorites must be present: {json}");
        assert!(!json.contains("\"domain\":{"), "must not double-nest under domain: {json}");

        let restored: UserProfile = serde_json::from_str(&json).expect("must deserialize");
        assert_eq!(restored.domain.granularity_favorites, profile.domain.granularity_favorites);
        assert_eq!(restored.domain.granularity_favorites.time_tfs, vec!["7m".to_string(), "90m".to_string()]);
        assert_eq!(restored.domain.granularity_favorites.params.len(), 2);
        assert_eq!(restored.domain.granularity_favorites.params[0].chart_type, "range_bar");
        assert_eq!(restored.domain.granularity_favorites.params[0].value, 17.0);
        assert_eq!(restored.domain.granularity_favorites.starred_tfs, vec!["1m".to_string(), "7m".to_string()]);
        assert_eq!(restored.domain.granularity_favorites.starred_params.len(), 1);
        assert_eq!(restored.domain.granularity_favorites.starred_params[0].chart_type, "range_bar");
        assert_eq!(restored.domain.granularity_favorites.interval_tfs, vec!["7m".to_string(), "90s".to_string()]);
        assert_eq!(restored.domain.granularity_favorites.starred_intervals, vec!["5m".to_string()]);
    }

    #[test]
    fn pin_granularity_favorites_flag_round_trips() {
        let mut profile = UserProfile::new();
        assert!(!profile.ui.pin_granularity_favorites, "default must be false");

        profile.ui.pin_granularity_favorites = true;
        let json = serde_json::to_string(&profile).expect("must serialize");
        assert!(json.contains("\"pin_granularity_favorites\":true"), "flag must be flat on disk: {json}");
        assert!(!json.contains("\"ui\":{"), "must not nest under ui: {json}");

        let restored: UserProfile = serde_json::from_str(&json).expect("must deserialize");
        assert!(restored.ui.pin_granularity_favorites);
    }

    #[test]
    fn symbol_compare_toolbar_flags_round_trip() {
        let mut profile = UserProfile::new();
        assert!(!profile.ui.split_symbol_compare, "split_symbol_compare default must be false (merged)");
        assert!(profile.ui.symbol_quick_search, "symbol_quick_search default must be true (button-hosted fast-search)");

        profile.ui.split_symbol_compare = true;
        profile.ui.symbol_quick_search = false;
        let json = serde_json::to_string(&profile).expect("must serialize");
        assert!(json.contains("\"split_symbol_compare\":true"), "flag must be flat on disk: {json}");
        assert!(json.contains("\"symbol_quick_search\":false"), "flag must be flat on disk: {json}");
        assert!(!json.contains("\"ui\":{"), "must not nest under ui: {json}");

        let restored: UserProfile = serde_json::from_str(&json).expect("must deserialize");
        assert!(restored.ui.split_symbol_compare);
        assert!(!restored.ui.symbol_quick_search);

        // Old profile.json missing the keys (or carrying the deleted
        // `symbol_inline_input` flag, 2026-07-17 rework) must deserialize
        // with the new defaults.
        let legacy_json = r#"{"pin_granularity_favorites":true,"symbol_inline_input":true}"#;
        let legacy: UserProfile = serde_json::from_str(legacy_json).expect("must deserialize legacy json");
        assert!(!legacy.ui.split_symbol_compare);
        assert!(legacy.ui.symbol_quick_search);
    }
}
