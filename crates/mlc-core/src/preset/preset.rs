//! [`ChartPreset`] — the top-level persisted unit for chart state.
//!
//! A preset captures the complete, restorable state of the chart application:
//! panel layout, per-window configuration, sync groups, and indicators.
//! The [`crate::preset::storage`] module handles reading/writing presets to disk.

use serde::{Deserialize, Serialize};
use web_time::SystemTime;

use super::snapshots::{ChartWindowSnapshot, DataBubbleSnapshot, IndicatorSnapshot};
use crate::alert::AlertItem;

// =============================================================================
// ChartPreset
// =============================================================================

/// Current schema version. Increment when the serialized format changes in a
/// backward-incompatible way so that migration code can detect old files.
pub const PRESET_VERSION: u32 = 1;

/// Complete serializable snapshot of all chart state.
///
/// The `layout` field stores the docking-panel [`LayoutSnapshot`] as a raw
/// `serde_json::Value` to avoid a direct dependency on the `uzor-panels` crate
/// from within the chart crate.  The caller serializes `LayoutSnapshot` to a
/// `Value` before constructing the preset and deserializes it back when
/// restoring.
///
/// [`LayoutSnapshot`]: uzor::panels::serialize::LayoutSnapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartPreset {
    /// Unique identifier for this preset, generated at creation time.
    /// Format: `"preset_{unix_secs}_{nanos_suffix}"`.
    pub id: String,
    /// User-visible display name.
    pub name: String,
    /// Unix timestamp (seconds) when this preset was first created.
    pub created_at: u64,
    /// Schema version. Used for forward-compatible migration. Currently `1`.
    pub version: u32,
    /// Docking panel layout serialized as a JSON value.
    ///
    /// Callers should serialize `uzor::panels::serialize::LayoutSnapshot` via
    /// `serde_json::to_value(&layout_snapshot)` before storing here, and
    /// restore it with `serde_json::from_value(preset.layout.clone())`.
    pub layout: serde_json::Value,
    /// Per-window configuration snapshots.
    pub windows: Vec<ChartWindowSnapshot>,
    /// Data bubble snapshots (shared symbol/timeframe/indicators across windows).
    #[serde(alias = "sync_groups")]
    pub bubbles: Vec<DataBubbleSnapshot>,
    /// All indicator instance snapshots across all windows.
    pub indicators: Vec<IndicatorSnapshot>,
    /// Alert items associated with this preset.
    #[serde(default)]
    pub alerts: Vec<AlertItem>,
    /// Per-leaf color tag assignments (LeafId → [r,g,b,a]).
    /// Persisted so tags survive preset switching.
    #[serde(default)]
    pub leaf_color_tags: std::collections::HashMap<u64, [f32; 4]>,
    /// Per-slot `DockingManager<Panel>` layout snapshots (5 typed slots:
    /// index 0=Trading, 1=Dom, 2=Tape, 3=L2, 4=BigTrades — matches
    /// `mlc_app_sidebar_content::free_slot::SlotKind::index()`).
    ///
    /// Each entry is a serialized `uzor::panels::serialize::LayoutSnapshot`
    /// JSON string, or `None` if the slot is empty. Slots host trading panels
    /// and detached mini-charts.
    ///
    /// Grew from 4 to 5 entries in the typed-slot redesign (2026-07-11).
    /// Old 4-entry saves fail `serde`'s fixed-size array deserialization
    /// (array length mismatch) — `preset::storage`'s load path falls back to
    /// an empty preset rather than a partial one; see
    /// `hydration_impl.rs::hydrate_slots` for the legacy-index note (the
    /// array position itself was ALREADY the canonical slot index pre- and
    /// post-redesign, so no positional remap is needed for saves that DO
    /// load — only the widened array length is new).
    #[serde(default)]
    pub slot_layouts: [Option<String>; 5],
    /// Per-slot leaf descriptors (parallel to `slot_layouts`). Each vec lists
    /// the `Panel` payloads for leaves in the corresponding slot.
    #[serde(default)]
    pub slot_leaves: [Vec<PersistedFreeLeaf>; 5],
}

/// Serializable descriptor for a single `Panel` leaf inside a slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedFreeLeaf {
    /// Leaf id from `LayoutSnapshot` (maps into the per-slot layout tree).
    pub leaf_id: u64,
    /// Stable panel id — unique across restarts, used to key state in the store.
    #[serde(default)]
    pub panel_id: u64,
    /// Which kind of panel this leaf carries, including the minimal state
    /// snapshot needed to recreate the panel on restore.
    pub kind: PersistedFreeItemKind,
}

/// Local mirror of `mlc_app_panels::trading::SymbolSource`.
///
/// Kept here so the `chart` crate has no dependency on `zengeld-panels`.
/// Old presets that do not have a `source` field deserialize with
/// `SymbolSource::HyperFocus` via the `Default` impl.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PersistedSymbolSource {
    HyperFocus,
    Fixed {
        symbol: String,
        exchange: String,
        account_type: String,
    },
    BoundToChart {
        leaf_id: u64,
    },
}

impl Default for PersistedSymbolSource {
    fn default() -> Self {
        Self::HyperFocus
    }
}

/// Local mirror of `mlc_app_panels::trading::trading::trading_container::BottomTab`
/// — kept here for the same reason as [`PersistedSymbolSource`]: the `chart`
/// crate has no dependency on `zengeld-panels`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedBottomTab {
    Positions,
    Orders,
    Trades,
}

impl Default for PersistedBottomTab {
    fn default() -> Self {
        Self::Positions
    }
}

/// Mirror of `crate::state::Panel` kept in the preset module for JSON
/// persistence — schema has no runtime dependency on `sidebar-content`.
///
/// Each variant stores the minimal fields needed to recreate the matching
/// panel state via the panel's `new()` constructor. Additional state (live
/// order book, scrolled position, etc.) is ephemeral and not persisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PersistedFreeItemKind {
    Dom {
        #[serde(default)]
        source: PersistedSymbolSource,
        tick_size: f64,
        levels_displayed: usize,
        center_price: f64,
        /// Multi-exchange consolidated mode (merged Multi-DOM, global-docking
        /// Stage A 2026-07-30). All defaulted so older presets restore in
        /// single-ladder mode.
        #[serde(default)]
        multi_mode: bool,
        #[serde(default)]
        multi_exchanges: Vec<String>,
        #[serde(default = "default_multi_dom_grid_size")]
        multi_grid_size: f64,
        #[serde(default = "default_multi_dom_depth")]
        multi_depth: usize,
        /// Bottom ±2% / ±10% liquidity-share strip (owner toggle 2026-07-30).
        #[serde(default = "default_true")]
        multi_show_imbalance: bool,
        /// Intensity normalization: per-column max vs cross-exchange max
        /// (TapeSurf Individual vs Cross Exchange, 2026-07-31).
        #[serde(default)]
        multi_per_column_norm: bool,
        /// Solo-ladder depth wave + stepped outline toggle (Multi-DOM
        /// parity, 2026-07-31).
        #[serde(default = "default_true")]
        dom_depth_wave: bool,
        /// Solo-ladder big-order marker toggle.
        #[serde(default = "default_true")]
        dom_big_markers: bool,
    },
    Footprint {
        #[serde(default)]
        source: PersistedSymbolSource,
        tick_size: f64,
    },
    VolumeProfile {
        #[serde(default)]
        source: PersistedSymbolSource,
        tick_size: f64,
    },
    LiquidityHeatmap {
        #[serde(default)]
        source: PersistedSymbolSource,
        tick_size: f64,
        snapshot_interval_ms: u64,
    },
    BigTrades {
        #[serde(default)]
        source: PersistedSymbolSource,
        /// Multi-venue mode + roster (M/BT, owner 2026-07-31). Defaulted
        /// so older presets restore in single-venue mode.
        #[serde(default)]
        multi_mode: bool,
        #[serde(default)]
        multi_entries: Vec<String>,
        /// Aggregated all-venue single-column mode (owner 2026-08-01).
        #[serde(default)]
        agg_mode: bool,
    },
    L2Tape {
        #[serde(default)]
        source: PersistedSymbolSource,
        /// Multi-venue mode + roster (M/L2, owner 2026-07-31).
        #[serde(default)]
        multi_mode: bool,
        #[serde(default)]
        multi_entries: Vec<String>,
        /// Add/Cancel imbalance footer + its window (owner 2026-08-01).
        #[serde(default = "default_true")]
        show_imbalance: bool,
        #[serde(default = "default_imb_window_min")]
        imb_window_min: u8,
        /// Per-event-type row visibility (owner 2026-08-01).
        #[serde(default = "default_true")]
        show_adds: bool,
        #[serde(default = "default_true")]
        show_mods: bool,
        #[serde(default = "default_true")]
        show_cxls: bool,
        #[serde(default = "default_true")]
        show_execs: bool,
    },
    TradeTape {
        #[serde(default)]
        source: PersistedSymbolSource,
        /// Multi-venue mode + roster (M/TT, owner 2026-07-31).
        #[serde(default)]
        multi_mode: bool,
        #[serde(default)]
        multi_entries: Vec<String>,
        /// Imbalance footer strip + its window (owner 2026-07-31).
        #[serde(default = "default_true")]
        show_imbalance: bool,
        #[serde(default = "default_imb_window_min")]
        imb_window_min: u8,
    },
    OrderEntry {
        #[serde(default)]
        source: PersistedSymbolSource,
    },
    PositionManager,
    TradeLog,
    RiskCalculator,
    /// LEGACY DECODE ONLY (pre Stage-A merge, 2026-07-30): Multi-DOM is now a
    /// mode toggle on `Dom`. Never written anymore; old presets carrying this
    /// variant hydrate into a `Dom` with `multi_mode = true`. Remove together
    /// with slot persistence in Stage B.
    MultiDom {
        /// Symbol (e.g. "BTCUSDT").
        #[serde(default)]
        symbol: String,
        /// Ordered list of exchange labels (e.g. ["binance", "bybit"]).
        #[serde(default)]
        exchanges: Vec<String>,
        /// Grid size (tick coalescence) — default 0.5.
        #[serde(default = "default_multi_dom_grid_size")]
        grid_size: f64,
        /// Rows visible per side — default 25.
        #[serde(default = "default_multi_dom_depth")]
        depth_per_side: usize,
    },
    TradingContainer {
        #[serde(default)]
        source: PersistedSymbolSource,
        tick_size: f64,
        market_price: f64,
        /// v1 region ratios (quality arc W1, 2026-07-25) — right column
        /// width fraction, bottom strip height fraction, OrderEntry|
        /// RiskCalculator split. All `#[serde(default)]` so presets saved
        /// before this field existed still restore (falling back to the
        /// same defaults `TradingContainerState::new` uses).
        #[serde(default = "default_tc_right_ratio")]
        right_ratio: f64,
        #[serde(default = "default_tc_bottom_ratio")]
        bottom_ratio: f64,
        #[serde(default = "default_tc_right_split")]
        right_split: f64,
        /// Bottom-strip active tab (quality arc W3, 2026-07-25 — the
        /// PositionManager|TradeLog fixed split was replaced by a
        /// Positions|Orders|Trades tab strip). `#[serde(default)]` so
        /// presets saved before this field existed restore on the
        /// `Positions` tab.
        #[serde(default)]
        active_tab: PersistedBottomTab,
        /// Header-tab TF control's value (trading-header-tf arc,
        /// 2026-07-25). `#[serde(default = ...)]` so presets saved before
        /// this field existed restore to the same `"1m"` fallback
        /// `TradingContainerState::new` uses.
        #[serde(default = "default_tc_timeframe")]
        timeframe: String,
        /// Center-slot content (owner spec 2026-07-26): "dom" | "chart".
        /// `#[serde(default = ...)]` so older presets restore to DOM.
        #[serde(default = "default_tc_center_kind")]
        center_kind: String,
        /// Interior layout mode (owner spec 2026-07-29): "fixed" | "free".
        /// `#[serde(default = ...)]` so older presets restore Fixed.
        #[serde(default = "default_tc_layout_mode")]
        layout_mode: String,
        /// Free docking tree snapshot as an OPAQUE JSON string (uzor
        /// `LayoutSnapshot::to_json` — mlc-core must stay uzor-free, so
        /// the engine type never appears here). Empty = no snapshot; a
        /// Free restore then re-migrates from the v1 ratios above.
        #[serde(default)]
        free_tree: String,
        /// Risk-calculator hosting (owner spec 2026-07-29): false = its
        /// own panel/leaf (default), true = overlay popover behind the
        /// gauge chip on Order Entry.
        #[serde(default)]
        rc_overlay: bool,
        /// Free-mode focus-highlight style (owner spec 2026-07-29):
        /// false = accent frame, true = main-chart-style corner bar.
        #[serde(default)]
        focus_bar: bool,
    },
    /// Standalone IndicatorPanel — symbol + which indicator + axis state.
    /// Live data (bars Vec, last_seen_bar_version) is ephemeral and re-pulled
    /// from the bar pool on restore.
    IndicatorPanel {
        #[serde(default)]
        symbol: String,
        #[serde(default)]
        exchange: String,
        #[serde(default)]
        account_type: String,
        #[serde(default)]
        instance_id: u64,
        #[serde(default)]
        indicator_config_id: u64,
        #[serde(default)]
        price_min: f64,
        #[serde(default = "default_price_max")]
        price_max: f64,
        #[serde(default = "default_true")]
        auto_scale: bool,
        #[serde(default = "default_bar_spacing")]
        bar_spacing: f64,
        #[serde(default)]
        view_position: f64,
        #[serde(default = "default_height_ratio")]
        height_ratio: f32,
    },
}

fn default_price_max() -> f64 { 100.0 }
fn default_true() -> bool { true }
fn default_imb_window_min() -> u8 { 2 }
fn default_bar_spacing() -> f64 { 8.0 }
fn default_height_ratio() -> f32 { 1.0 }
fn default_multi_dom_grid_size() -> f64 { 0.5 }
fn default_multi_dom_depth() -> usize { 25 }
// TradingContainer v1 region-ratio defaults — mirror
// `TradingContainerState::new()` / its `TC_*_DEFAULT` constants so a preset
// missing these fields (saved before W1) restores identically to a
// freshly-constructed container.
fn default_tc_right_ratio() -> f64 { 0.3 }
fn default_tc_bottom_ratio() -> f64 { 0.25 }
fn default_tc_right_split() -> f64 { 0.55 }
fn default_tc_timeframe() -> String { "1m".to_string() }
fn default_tc_center_kind() -> String { "dom".to_string() }
fn default_tc_layout_mode() -> String { "fixed".to_string() }

impl ChartPreset {
    /// Create a new, empty preset with the given display name.
    ///
    /// `id` is generated from the current Unix timestamp in seconds combined
    /// with the sub-second nanosecond component to ensure uniqueness even when
    /// multiple presets are created within the same second.
    ///
    /// `layout` defaults to `serde_json::Value::Null` and should be replaced
    /// by the caller before saving.
    pub fn new(name: String) -> Self {
        let (created_at, suffix) = unix_timestamp_parts();
        let id = format!("preset_{}_{}", created_at, suffix);

        Self {
            id,
            name,
            created_at,
            version: PRESET_VERSION,
            layout: serde_json::Value::Null,
            windows: Vec::new(),
            bubbles: Vec::new(),
            indicators: Vec::new(),
            alerts: Vec::new(),
            leaf_color_tags: std::collections::HashMap::new(),
            slot_layouts: [None, None, None, None, None],
            slot_leaves: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
        }
    }

    /// Returns this preset's unique identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the user-visible display name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

// =============================================================================
// Helpers
// =============================================================================

/// Returns `(unix_seconds, nanos_suffix)` using [`SystemTime`].
///
/// `nanos_suffix` is the nanosecond component of the current time modulo
/// 1_000_000 — small enough to be a readable suffix while still providing
/// sub-millisecond uniqueness.
pub fn unix_timestamp_parts() -> (u64, u32) {
    let now = SystemTime::now();
    let duration = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();

    let secs = duration.as_secs();
    let nanos_suffix = duration.subsec_nanos() % 1_000_000;
    (secs, nanos_suffix)
}

/// Returns the current Unix timestamp in seconds.
pub fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
