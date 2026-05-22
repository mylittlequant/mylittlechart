//! Serializable snapshot structs for the ChartPreset system.
//!
//! These types capture the persisted state of complex chart types that cannot
//! derive `Serialize` directly because they contain trait objects
//! (`Box<dyn Primitive>`, `Box<dyn IndicatorSource>`, etc.).

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

fn default_account_type_spot() -> String {
    "S".to_string()
}

use crate::drawing::primitives_v2::Primitive;
use crate::drawing::primitives_v2::config::TimeframeVisibilityConfig;
use crate::drawing::DrawingManager;
use crate::state::chart_window::ChartWindow;
use crate::state::history::CommandHistory;
use crate::state::Timeframe;
use crate::tag_manager::{IndicatorGroupConfig, SyncFlags, SyncGroup, SyncMemberId};
use crate::{CompareOverlay, CrosshairOptions, GridOptions, Legend, PriceScale, Tooltip, Viewport, Watermark};

// =============================================================================
// PrimitiveSnapshot
// =============================================================================

/// Serialized form of a single drawing primitive.
///
/// Because `Box<dyn Primitive>` cannot derive `Serialize`, each primitive is
/// captured via its `type_id()` (for factory reconstruction) and `to_json()`
/// (the full state in JSON form).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrimitiveSnapshot {
    /// Primitive type identifier (e.g. `"trend_line"`, `"fib_retracement"`).
    pub type_id: String,
    /// Full primitive state serialized to JSON by the primitive itself.
    pub json: String,
}

impl PrimitiveSnapshot {
    /// Capture a snapshot from any [`Primitive`] trait object.
    pub fn from_primitive(prim: &dyn Primitive) -> Self {
        Self {
            type_id: prim.type_id().to_string(),
            json: prim.to_json(),
        }
    }
}

// =============================================================================
// DrawingSnapshot
// =============================================================================

/// Snapshot of all drawing primitives belonging to a single chart window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawingSnapshot {
    /// The window these primitives belong to.
    pub window_id: u64,
    /// Ordered list of primitive snapshots.
    pub primitives: Vec<PrimitiveSnapshot>,
}

impl DrawingSnapshot {
    /// Capture a snapshot from a [`DrawingManager`] for the given window.
    pub fn from_manager(window_id: u64, mgr: &DrawingManager) -> Self {
        let primitives = mgr
            .primitives()
            .iter()
            .map(|p| PrimitiveSnapshot::from_primitive(p.as_ref()))
            .collect();

        Self {
            window_id,
            primitives,
        }
    }
}

// =============================================================================
// ChartWindowSnapshot
// =============================================================================

/// Serializable snapshot of a [`ChartWindow`]'s configurable state.
///
/// Runtime-only fields (bars, computed MAs, drag state, etc.) are excluded.
/// Reconstruct a live window by deserializing this and then rehydrating the
/// data-provider and indicator-source via the host application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartWindowSnapshot {
    /// Stable window identifier.
    pub window_id: u64,
    /// Leaf (panel slot) identifier this window occupies in the docking tree.
    pub leaf_id: u64,
    /// Trading symbol (e.g. `"BTCUSDT"`).
    pub symbol: String,
    /// Exchange name (e.g. `"Binance"`).
    pub exchange: String,
    /// Account type short label (e.g. "S", "F"). Default "S" for Spot.
    #[serde(default = "default_account_type_spot")]
    pub account_type: String,
    /// Timeframe (e.g. 1H, 4H).
    pub timeframe: Timeframe,
    /// Viewport state (pan/zoom position).
    pub viewport: Viewport,
    /// Price scale configuration.
    pub price_scale: PriceScale,
    /// Sync group this window belongs to (if any).
    pub group_id: Option<u64>,
    /// Grid line display options.
    pub grid_options: GridOptions,
    /// Crosshair line style and mode.
    pub crosshair_options: CrosshairOptions,
    /// Legend (OHLC value display).
    pub legend: Legend,
    /// Watermark branding overlay.
    pub watermark: Option<Watermark>,
    /// Tooltip configuration.
    pub tooltip: Tooltip,
    /// Active chart type name (e.g. `"candles"`, `"line"`).
    pub chart_type: String,
    // --- Series visibility ---
    /// Show filled candlesticks.
    pub show_candles: bool,
    /// Show OHLC bar ticks.
    pub show_bars: bool,
    /// Show hollow candles.
    pub show_hollow_candles: bool,
    /// Show Heikin Ashi candles.
    pub show_heikin_ashi: bool,
    /// Show line series.
    pub show_line: bool,
    /// Show step-line (staircase) series.
    pub show_step_line: bool,
    /// Show line with dot markers.
    pub show_line_markers: bool,
    /// Show area series.
    pub show_area: bool,
    /// Show HLC area.
    pub show_hlc_area: bool,
    /// Show histogram series.
    pub show_histogram: bool,
    /// Show column series.
    pub show_columns: bool,
    /// Show baseline series.
    pub show_baseline: bool,
    /// Scale display settings (positions, dimensions, precision).
    pub scale_settings: crate::scale_settings::ScaleSettings,
    /// Local window drawings (only populated for ungrouped windows).
    pub drawings: DrawingSnapshot,
    /// Undo/redo command history for this window.
    #[serde(default)]
    pub command_history: Option<CommandHistory>,
    /// Stashed command history saved when the window joined a sync group.
    #[serde(default)]
    pub stashed_command_history: Option<CommandHistory>,
    /// Compare overlay (overlaid symbols for comparison).
    #[serde(default)]
    pub compare_overlay: CompareOverlay,
    /// Per-symbol drawing cache so drawings survive symbol switches.
    ///
    /// Keyed by `"symbol:exchange:account_type"` (e.g. `"BTCUSDT:binance:S"`), each
    /// entry is the list of primitive snapshots that were active on this window for
    /// that symbol+exchange+account_type combination.
    #[serde(default)]
    pub symbol_drawings_snapshots: std::collections::HashMap<String, Vec<PrimitiveSnapshot>>,
    /// Bars are never serialized to preset JSON — they come from bar-store
    /// (disk cache) or from the exchange at load time.
    ///
    /// Old presets that contain a `bars` field still deserialize fine via
    /// `serde(default)` — the field is simply ignored on write.
    #[serde(default, skip_serializing)]
    pub bars: Vec<crate::Bar>,
    /// Stashed primitives: the window's own drawing primitives saved when
    /// the window joined an existing sync group (color tag). Restored on desync.
    /// Empty for windows that were not in a tag or that seeded a new tag.
    #[serde(default)]
    pub stashed_primitives: Vec<PrimitiveSnapshot>,
    /// Indicator instance IDs that existed on this window BEFORE it joined a
    /// color tag. On desync, only indicators NOT in this set are removed.
    /// Empty for windows that were not in a tag.
    #[serde(default)]
    pub pre_tag_indicator_ids: Vec<u64>,
    /// Persisted height ratios for sub-panes, keyed by indicator instance_id.
    /// `0.0` (absent) means use the default 100 px height.
    #[serde(default)]
    pub sub_pane_height_ratios: HashMap<u64, f32>,
    /// Set of sub-pane instance_ids that have `above_main == true`.
    /// Empty means all panes are below the main chart (default).
    #[serde(default)]
    pub sub_pane_above_main: HashSet<u64>,
    /// Ordered list of sub-pane instance_ids capturing the Vec order at save
    /// time.  Used on restore to re-establish the correct display order
    /// (above-main panes are stored first, then below-main panes).
    #[serde(default)]
    pub sub_pane_order: Vec<u64>,
}

impl ChartWindowSnapshot {
    /// Capture a full snapshot from a live [`ChartWindow`].
    ///
    /// `leaf_id` is the docking-tree leaf slot this window occupies; it is
    /// stored so that presets can restore windows to the correct panel slot.
    pub fn from_window(window: &ChartWindow, leaf_id: u64) -> Self {
        let drawings = DrawingSnapshot::from_manager(window.id.0, &window.drawing_manager);

        // Save full viewport — chart_width, bar_spacing, bar_count are all valid
        // and needed for immediate snap in set_bars(). If app window was resized,
        // sync_viewport_from_layout will proportionally adjust chart_width.
        // Only zero view_start — snap-to-end always positions to latest bar.
        let mut saved_viewport = window.viewport.clone();
        saved_viewport.view_start = 0.0;

        let mut saved_price_scale = window.price_scale.clone();
        // price_min/price_max are recalculated from bars; zero them out.
        saved_price_scale.price_min = 0.0;
        saved_price_scale.price_max = 0.0;

        Self {
            window_id: window.id.0,
            leaf_id,
            symbol: window.symbol.clone(),
            exchange: window.exchange.clone(),
            account_type: window.account_type.clone(),
            timeframe: window.timeframe.clone(),
            viewport: saved_viewport,
            price_scale: saved_price_scale,
            group_id: window.group_id.map(|g| g.0),
            grid_options: window.grid_options.clone(),
            crosshair_options: window.crosshair_options.clone(),
            legend: window.legend.clone(),
            watermark: window.watermark.clone(),
            tooltip: window.tooltip.clone(),
            chart_type: window.chart_type.to_string(),
            show_candles: window.show_candles,
            show_bars: window.show_bars,
            show_hollow_candles: window.show_hollow_candles,
            show_heikin_ashi: window.show_heikin_ashi,
            show_line: window.show_line,
            show_step_line: window.show_step_line,
            show_line_markers: window.show_line_markers,
            show_area: window.show_area,
            show_hlc_area: window.show_hlc_area,
            show_histogram: window.show_histogram,
            show_columns: window.show_columns,
            show_baseline: window.show_baseline,
            scale_settings: window.scale_settings.clone(),
            drawings,
            command_history: Some(window.command_history.clone()),
            stashed_command_history: window.stashed_command_history.clone(),
            compare_overlay: window.compare_overlay.clone(),
            symbol_drawings_snapshots: window.symbol_drawings.clone(),
            bars: Vec::new(),
            stashed_primitives: window
                .stashed_primitives
                .iter()
                .map(|p| PrimitiveSnapshot::from_primitive(p.as_ref()))
                .collect(),
            pre_tag_indicator_ids: window.pre_tag_indicator_ids.clone(),
            sub_pane_height_ratios: window
                .sub_panes
                .iter()
                .filter(|p| p.height_ratio > 0.0)
                .map(|p| (p.instance_id, p.height_ratio))
                .collect(),
            sub_pane_above_main: window
                .sub_panes
                .iter()
                .filter(|p| p.above_main)
                .map(|p| p.instance_id)
                .collect(),
            sub_pane_order: window
                .sub_panes
                .iter()
                .map(|p| p.instance_id)
                .collect(),
        }
    }
}

// =============================================================================
// OutputConfigSnapshot
// =============================================================================

/// Serializable snapshot of a single indicator output's style overrides.
///
/// Mirrors `OutputConfig` from the indicators crate.  Stored as a separate
/// snapshot type so the chart crate does not need a hard dependency on the
/// indicators crate's internal structs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfigSnapshot {
    /// Override color (CSS/hex string).  `None` means use the indicator default.
    pub color: Option<String>,
    /// Override line width in pixels.  `None` means use the indicator default.
    pub line_width: Option<f32>,
    /// Visibility of this specific output.  `None` means inherit from the
    /// indicator-level `visible` flag.
    pub visible: Option<bool>,
}

// =============================================================================
// IndicatorSnapshot
// =============================================================================

/// Serializable representation of a single indicator instance.
///
/// Mirrors the persisted fields of `IndicatorInstance` from the indicators
/// crate.  `values` is included so that switching tabs can show cached
/// indicator output instantly (before a background recalc completes).
/// `signals` is still omitted (transient alert state).
///
/// Using a separate struct avoids a direct dependency on the indicators crate
/// from the chart crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorSnapshot {
    /// Unique instance identifier.
    pub id: u64,
    /// Indicator type identifier (references indicator catalog).
    pub type_id: String,
    /// Display name (may be user-customised).
    pub name: String,
    /// Serialised parameter values (key = param name, value = JSON string).
    pub params: HashMap<String, serde_json::Value>,
    /// Per-output style overrides (color, line width, visibility).
    #[serde(default)]
    pub outputs: HashMap<String, OutputConfigSnapshot>,
    /// Pane index: 0 = main chart, 1+ = separate sub-pane.
    pub pane: usize,
    /// Order within pane.
    pub order: i32,
    /// Whether the indicator is visible.
    pub visible: bool,
    /// Whether the indicator is locked (no editing).
    pub locked: bool,
    /// Legacy: symbol the indicator was bound to.
    /// Indicators are now bound to a *window*, not an instrument — this field
    /// is preserved on disk for backwards-compatible profile loading but is
    /// ignored at restore time.
    #[serde(default, skip_serializing)]
    pub symbol: String,
    /// Window ID (for multi-window support).
    pub window_id: Option<u64>,
    /// Origin instance ID (set when this instance was cloned for a sync group).
    pub origin_id: Option<u64>,
    /// Whether signal generation is enabled.
    pub signals_enabled: bool,
    /// Timeframe visibility configuration (which timeframes show this indicator).
    /// `None` means visible on all timeframes.
    #[serde(default)]
    pub timeframe_visibility: Option<TimeframeVisibilityConfig>,
    /// Cached computed output series (output key → values vector).
    ///
    /// Persisted in local preset snapshots so that tab switching shows
    /// indicator values immediately.  Stripped from cloud-sync payloads
    /// (regenerable data — see `strip_preset_for_sync`).
    /// Absent in older presets → deserialises as empty map via `default`.
    #[serde(default)]
    pub values: HashMap<String, Vec<f64>>,
}

// =============================================================================
// SyncGroupSnapshot
// =============================================================================

// =============================================================================
// MemberOverrideSnapshot
// =============================================================================

/// Serializable snapshot of a per-member sync flag override.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberOverrideSnapshot {
    /// Member kind: `0` = Chart, `1` = Panel.
    pub kind: u8,
    /// Raw member id.
    pub id: u64,
    /// Override for symbol synchronisation. `None` = use group default.
    pub sync_symbol: Option<bool>,
    /// Override for crosshair synchronisation. `None` = use group default.
    pub sync_crosshair: Option<bool>,
}

// =============================================================================
// SyncGroupSnapshot
// =============================================================================

/// Serializable snapshot of a [`SyncGroup`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncGroupSnapshot {
    /// Unique group identifier.
    pub id: u64,
    /// Display color `[r, g, b, a]` used to identify this group in the UI.
    pub color: [f32; 4],
    /// Shared symbol for the group.
    pub symbol: String,
    /// Exchange name (e.g. `"binance"`). Old presets default to empty string.
    #[serde(default)]
    pub exchange: String,
    /// Account type (e.g. `"S"`). Old presets default to empty string.
    #[serde(default)]
    pub account_type: String,
    /// Shared timeframe for the group.
    pub timeframe: Timeframe,
    /// Which properties are synchronised across member windows.
    pub sync_flags: SyncFlags,
    /// Shared indicator configurations propagated to all member windows.
    pub indicator_configs: Vec<IndicatorGroupConfig>,
    /// Chart member window IDs (backward compat — only chart ids).
    pub members: Vec<u64>,
    /// Panel member IDs. Absent in old presets → empty vec via `serde(default)`.
    #[serde(default)]
    pub panel_members: Vec<u64>,
    /// Per-member flag overrides. Absent in old presets → empty vec.
    #[serde(default)]
    pub member_overrides: Vec<MemberOverrideSnapshot>,
    /// Panel IDs that are in Synced state. Absent in old presets → empty vec.
    #[serde(default)]
    pub synced_panel_ids: Vec<u64>,
    /// Shared drawing primitives owned by this group.
    pub primitives: Vec<PrimitiveSnapshot>,
    /// Undo/redo command history for this sync group.
    #[serde(default)]
    pub command_history: Option<CommandHistory>,
    /// Invisible auto-created group (no color tag in UI).
    #[serde(default)]
    pub auto_created: bool,
}

impl SyncGroupSnapshot {
    /// Capture a snapshot from a live [`SyncGroup`].
    pub fn from_group(group: &SyncGroup) -> Self {
        let primitives = group
            .primitives
            .iter()
            .map(|p| PrimitiveSnapshot::from_primitive(p.as_ref()))
            .collect();

        // Separate chart and panel members
        let mut members: Vec<u64> = Vec::new();
        let mut panel_members: Vec<u64> = Vec::new();
        for member in &group.members {
            match member {
                SyncMemberId::Chart(id) => members.push(*id),
                SyncMemberId::Panel(id) => panel_members.push(*id),
            }
        }

        // Serialize per-member overrides
        let member_overrides: Vec<MemberOverrideSnapshot> = group
            .member_overrides
            .iter()
            .map(|(member, ovr)| {
                let (kind, id) = match member {
                    SyncMemberId::Chart(id) => (0u8, *id),
                    SyncMemberId::Panel(id) => (1u8, *id),
                };
                MemberOverrideSnapshot {
                    kind,
                    id,
                    sync_symbol: ovr.sync_symbol,
                    sync_crosshair: ovr.sync_crosshair,
                }
            })
            .collect();

        Self {
            id: group.id.0,
            color: group.color,
            symbol: group.symbol.clone(),
            exchange: group.exchange.clone(),
            account_type: group.account_type.clone(),
            timeframe: group.timeframe.clone(),
            sync_flags: group.sync_flags.clone(),
            indicator_configs: group.indicator_configs.clone(),
            members,
            panel_members,
            member_overrides,
            synced_panel_ids: Vec::new(), // populated by TagManager on save
            primitives,
            command_history: Some(group.command_history.clone()),
            auto_created: group.auto_created,
        }
    }
}
