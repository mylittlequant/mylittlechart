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

fn default_chart_type_interval() -> String {
    "1m".to_string()
}

use crate::chart::types::price_scale::{PriceScaleMode, ScaleMode};
use crate::drawing::primitives_v2::Primitive;
use crate::drawing::primitives_v2::config::TimeframeVisibilityConfig;
use crate::state::Timeframe;
use crate::state::history::CommandHistory;
use crate::indicator_types::IndicatorGroupConfig;
use crate::ids::SyncFlags;
use crate::{CompareOverlay, CrosshairOptions, GridOptions, Legend, Tooltip, Watermark};

// `PrimitiveSnapshot` lives in `mlc-core::drawing::PrimitiveSnapshot`
// (next to the `Primitive` trait) so both runtime caches and the preset
// system can speak about it without one depending on the other.
pub use crate::drawing::PrimitiveSnapshot;

// =============================================================================
// ViewportPersist / PriceScalePersist
// =============================================================================
//
// Viewport SWC arc (2026-07-03): `ChartWindowSnapshot` used to embed the
// full `Viewport` / `PriceScale` chart types, wholesale-cloning runtime
// fields (`view_start`, `chart_width`, `bar_count`, `price_min`/`price_max`,
// `scale_mode`) that must never round-trip through disk — see
// `docs/mlc/plans/viewport-swc-2026-07-03.md`. These minimal DTOs replace
// them, keeping the on-disk JSON keys `"viewport"` / `"price_scale"`
// unchanged so old preset files still deserialize (unknown/missing fields
// are handled by `#[serde(default)]`); the extra legacy fields on old JSON
// objects are simply ignored by serde.

// NO ViewportPersist exists anymore (owner doctrine 2026-07-12): NOTHING
// of the viewport survives a reload — not even `bar_spacing`. A cold load
// always seeds `Viewport::fit_default_view` (100 bars, right-snapped,
// Auto scale). The old persisted per-WINDOW `bar_spacing` had no
// chart-type identity: a save landing while a wide type was up (TPO
// `min_bar_spacing` 120px, footprint 50px) replayed that zoom onto
// candles at the next boot — a 3-bars-per-frame viewport. Old snapshot
// JSON still carries a `"viewport"` key; serde ignores unknown fields.

/// Persisted price-scale display configuration. `price_min`/`price_max`
/// (recomputed from bars) and `scale_mode` (the A/M/F corner button —
/// governed by the cold-load rule, never restored raw) are runtime-only
/// and are never written to disk.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct PriceScalePersist {
    /// Normal / Percent / Logarithmic display mode.
    #[serde(default)]
    pub mode: PriceScaleMode,
    /// Base price for percent mode.
    #[serde(default)]
    pub base_price: f64,
    /// Calculated width of the price scale area.
    #[serde(default)]
    pub width: f64,
    /// User-configured precision override.
    #[serde(default)]
    pub user_precision: Option<usize>,
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
    /// Capture a snapshot from a slice of primitives for the given window.
    ///
    /// Decoupled from `DrawingManager` so `mlc-core::preset` does not need
    /// to mention the backend manager that owns those primitives.
    pub fn from_primitives(window_id: u64, primitives: &[Box<dyn Primitive>]) -> Self {
        let primitives = primitives
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

/// Serializable snapshot of a [`ChartContainer`]'s configurable state.
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
    /// Persisted price-scale display config (see `PriceScalePersist`).
    #[serde(default)]
    pub price_scale: PriceScalePersist,
    /// Data bubble this window belongs to (if any).
    #[serde(alias = "group_id")]
    pub bubble_id: Option<u64>,
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
    /// Numeric parameter for derived-bar chart types (range_bar / renko /
    /// tick_bar / ...). Mirrors `ChartPanelState::chart_type_param` — see
    /// that field's doc comment for the per-type unit table. `0.0` means
    /// "use the code-side default at subscribe time" (never actually
    /// user-set). Additive field (2026-07-05); old presets deserialize via
    /// `#[serde(default)]` and land on the same `0.0` sentinel they always
    /// implicitly had.
    #[serde(default)]
    pub chart_type_param: f64,
    /// Box-size computation mode for `chart_type_param` — meaningful only
    /// for `"renko"`/`"kagi"` (granularity arc, кусок 2). Mirrors
    /// `DataBubble::chart_type_param_mode`. Additive field (2026-07-15); old
    /// presets deserialize via `#[serde(default)]` and land on
    /// `ParamMode::Fixed` (pixel-identical to the pre-ATR-mode behavior —
    /// every chart_type_param was a plain ladder/custom value).
    #[serde(default)]
    pub chart_type_param_mode: crate::chart_type::ParamMode,
    /// Kline bucket interval for the `"footprint"` chart type (e.g.
    /// `"1m"`, `"5m"`). Mirrors `ChartPanelState::chart_type_interval`.
    /// Additive field (2026-07-05); old presets default to `"1m"`.
    #[serde(default = "default_chart_type_interval")]
    pub chart_type_interval: String,
    /// Footprint cluster display toggles (ABS markers / volume text / heat
    /// intensity). Mirrors `ChartPanelState::footprint_display`. Unused for
    /// every other chart type. Additive field (2026-07-05); old presets
    /// deserialize via `#[serde(default)]` and land on
    /// `FootprintDisplayOptions::default()` (both checkboxes on, Normal
    /// heat intensity — pixel-identical to the pre-toggle render).
    #[serde(default)]
    pub footprint_display: crate::chart::render_data::FootprintDisplayOptions,
    /// Tick-bar body-opacity intensity toggle. Mirrors
    /// `ChartPanelState::tick_intensity`. Unused for every other chart
    /// type. Additive field (2026-07-10); old presets deserialize via
    /// `#[serde(default)]` and land on `false` (pixel-identical to the
    /// pre-toggle render — plain solid bodies).
    #[serde(default)]
    pub tick_intensity: bool,
    /// Bar-family "Wicks" re-enable toggle (range_bar/volume_bar/dollar_bar
    /// render WITHOUT wicks by default). Mirrors
    /// `ChartPanelState::bar_family_wicks`. Unused for every other chart
    /// type. Additive field (2026-07-10); old presets deserialize via
    /// `#[serde(default)]` and land on `false` (pixel-identical to the
    /// pre-toggle render — no wicks for the bar family).
    #[serde(default)]
    pub bar_family_wicks: bool,
    /// TPO (Market Profile) display toggles (session-date header / POC /
    /// VA / single-prints / poor-HL / initial-balance / IB period count).
    /// Mirrors `ChartPanelState::tpo_display`. Unused for every other
    /// chart type. Additive field (2026-07-11); old presets deserialize
    /// via `#[serde(default)]` and land on `TpoDisplayOptions::default()`
    /// (pixel-identical to the pre-toggle render).
    #[serde(default)]
    pub tpo_display: crate::chart::render_data::TpoDisplayOptions,
    /// Simple line-family per-type display options (line/step-line/area/
    /// baseline widths + line_markers' marker style/size). Mirrors
    /// `ChartPanelState::line_family_display`. Unused for every other
    /// chart type. Additive field (owner-approved settings matrix wave 3,
    /// 2026-07-11); old presets deserialize via `#[serde(default)]` and
    /// land on `LineFamilyDisplayOptions::default()` (pixel-identical to
    /// the pre-toggle render — same widths/marker style/size every
    /// renderer hardcoded).
    #[serde(default)]
    pub line_family_display: crate::chart::render_data::LineFamilyDisplayOptions,
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
    /// Undo/redo history for this window, bounded on write (newest steps
    /// only, under a step count and a byte budget — see
    /// `CommandHistory::bounded_for_persistence`).
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
    /// Set of sub-pane instance_ids whose leaf was hidden at save time.
    /// On restore, `hide_leaf` is called for each instance_id in this set.
    /// Fixes the pre-existing "hidden sub-pane never restored" bug.
    #[serde(default)]
    pub sub_pane_hidden: HashSet<u64>,
    /// Pre-join scale mode stashed when this window entered a group with
    /// `sync_viewport = ON`. Restored on desync. `None` = not stashed.
    #[serde(default)]
    pub stashed_scale_mode: Option<ScaleMode>,
}

// The `ChartWindowSnapshot::from_window` constructor (reads a live
// `ChartContainer` + `ChartPanelState`) moved to
// `mlc-app::persistence::window_snapshot::build_chart_window_snapshot` in
// Step 4 (state lift) — `ChartContainer`/`ChartPanelState` now live in
// `mlc-app`, which `mlc-core` cannot depend on.  This struct's field
// definitions stay here because `ChartPreset.windows: Vec<ChartWindowSnapshot>`
// needs no `ChartContainer` reference, only the constructor did.

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
    /// Bubble id the indicator lives in (color tag / sync group / home auto-bubble).
    /// Older preset files persist this under the legacy `window_id` JSON key;
    /// the serde alias keeps them loading transparently.
    #[serde(alias = "window_id")]
    pub bubble_id: Option<u64>,
    /// Origin instance ID (set when this instance was cloned for a sync group).
    pub origin_id: Option<u64>,
    /// Whether signal generation is enabled.
    #[serde(default)]
    pub signals_enabled: bool,
    /// Timeframe visibility configuration (which timeframes show this indicator).
    /// `None` means visible on all timeframes.
    #[serde(default)]
    pub timeframe_visibility: Option<TimeframeVisibilityConfig>,
    /// Cached computed output series (output key → values vector).
    ///
    /// **Never serialized**, on the same terms as `WindowSnapshot::bars` and
    /// for the same reason: an indicator's output is a function of the bars,
    /// and the bars are not stored either. It is recomputed the moment they
    /// arrive — the `indicator heal` path already does exactly that for any
    /// instance that loads with none.
    ///
    /// It used to be written locally "so that tab switching shows indicator
    /// values immediately", while `strip_preset_for_sync` removed it from
    /// every cloud payload as regenerable. Both cannot be right, and the disk
    /// measured the cost of the wrong one: 356 270 values across 16 presets,
    /// 8 MB, one preset carrying 200 000 volume values in a 3.97 MB file of
    /// which they were 1.78 MB (2026-09-03). A cache of derived data, kept
    /// forever, uncapped, in the file that is supposed to describe a layout.
    ///
    /// Kept as a field because it is the live in-memory home of those values
    /// during a session; `default` fills it on load, here and for the older
    /// presets that still carry the key on disk.
    #[serde(default, skip_serializing)]
    pub values: HashMap<String, Vec<f64>>,
}

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
// DataBubbleSnapshot
// =============================================================================

/// Serializable snapshot of a [`DataBubble`].
///
/// The serde rename keeps the on-disk JSON key as `"SyncGroupSnapshot"` for
/// backward-compatible reading of preset files written before this rename.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataBubbleSnapshot {
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
    /// Undo/redo history for this sync group, bounded on write.
    #[serde(default)]
    pub command_history: Option<CommandHistory>,
    /// Invisible auto-created group (no color tag in UI).
    #[serde(default)]
    pub auto_created: bool,
    /// Per-tool last-used drawing style for the group.
    /// Absent in old presets → empty map (first style change after load populates it).
    #[serde(default)]
    pub last_used_style: std::collections::HashMap<String, serde_json::Value>,
    /// Group-persisted price-scale corner mode. Authoritative when
    /// `sync_flags.sync_viewport = true`. Old presets default to Auto.
    #[serde(default)]
    pub scale_mode: ScaleMode,

    /// Alerts attached to this bubble.  Absent in old presets → empty vec.
    #[serde(default)]
    pub alerts: Vec<crate::alert::AlertItem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A preset saved before `signals_enabled` existed on `IndicatorSnapshot`
    /// must still deserialize, defaulting the field to `false` rather than
    /// failing to load.
    #[test]
    fn indicator_snapshot_without_signals_enabled_field_defaults_to_false() {
        let json = serde_json::json!({
            "id": 1,
            "type_id": "sma",
            "name": "SMA",
            "params": {},
            "pane": 0,
            "order": 0,
            "visible": true,
            "locked": false,
            "bubble_id": null,
            "origin_id": null,
        });
        let snap: IndicatorSnapshot = serde_json::from_value(json)
            .expect("old-format snapshot without signals_enabled must deserialize");
        assert!(!snap.signals_enabled);
    }
}

