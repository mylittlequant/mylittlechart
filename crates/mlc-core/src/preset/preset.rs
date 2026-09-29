//! [`ChartPreset`] — the top-level persisted unit for chart state.
//!
//! A preset captures the complete, restorable state of the chart application:
//! panel layout, per-window configuration, sync groups, and indicators.
//! The [`crate::preset::storage`] module handles reading/writing presets to disk.

use serde::{Deserialize, Serialize};
use web_time::SystemTime;

use super::snapshots::{ChartWindowSnapshot, DataBubbleSnapshot, IndicatorSnapshot};
use crate::alert::AlertItem;
use crate::drawing::PrimitiveSnapshot;

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
    /// Per-slot `DockingManager<Panel>` layout snapshots (typed slots:
    /// index 0=Trading, 1=Dom, 2=Tape, 3=L2, 4=BigTrades, 5=Liq — matches
    /// `mlc_app_sidebar_content::free_slot::SlotKind::index()`).
    ///
    /// Each entry is a serialized `uzor::panels::serialize::LayoutSnapshot`
    /// JSON string, or `None` if the slot is empty. Slots host trading panels
    /// and detached mini-charts.
    ///
    /// `Vec`, not a fixed array (since the 5→6 widening, 2026-08-01): the
    /// 4→5 widening proved a fixed `[T; N]` DROPS every older save on the
    /// serde length mismatch. A Vec deserializes any length; readers index
    /// with `.get(i)` and the snapshot writer resizes up before writing.
    #[serde(default)]
    pub slot_layouts: Vec<Option<String>>,
    /// Per-slot leaf descriptors (parallel to `slot_layouts`). Each vec lists
    /// the `Panel` payloads for leaves in the corresponding slot.
    #[serde(default)]
    pub slot_leaves: Vec<Vec<PersistedFreeLeaf>>,
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
        /// Aggregated sub-mode (one combined ladder, owner 2026-08-02).
        /// Defaulted so older presets restore in plain Multi-DOM.
        #[serde(default)]
        agg_mode: bool,
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
        /// Depth-bounds readout toggle (TapeSurf ref 11/12, 2026-08-02).
        #[serde(default)]
        multi_show_depth_bounds: bool,
        /// Depth-bounds band half-width, percent of mid.
        #[serde(default = "default_depth_bounds_pct")]
        multi_depth_bounds_pct: f64,
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
    /// Liquidation tape — all-venue merged forced-close stream (plan D,
    /// owner 2026-08-01). Roster is implicit (all futures venues); the
    /// persisted entries allow future per-panel pruning.
    LiqTape {
        #[serde(default)]
        source: PersistedSymbolSource,
        #[serde(default)]
        multi_entries: Vec<String>,
        /// Minimum base-qty to show (0 = all).
        #[serde(default)]
        size_threshold: f64,
        /// Minimum notional to show (`None` = all).
        #[serde(default)]
        notional_threshold: Option<f64>,
        /// Mode triad (owner 2026-08-01): defaults keep older saves in
        /// the original all-venue Aggregated behavior.
        #[serde(default = "default_true")]
        multi_mode: bool,
        #[serde(default = "default_true")]
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
fn default_depth_bounds_pct() -> f64 { 0.3 }
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
            slot_layouts: vec![None; 6],
            slot_leaves: vec![Vec::new(); 6],
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

    /// Rewrite every process-wide identifier this preset embeds to freshly
    /// allocated, never-before-used local ids, before the preset is
    /// inserted into a recipient's own live state — a share-link open or a
    /// chart clone (`mlc-app::share_viewer`).
    ///
    /// A preset built by ANOTHER process (the sharer's own session) carries
    /// raw numeric ids from THAT process's own counters
    /// (`ChartId`/`BubbleId`/primitive ids) — small and sequential, so they
    /// routinely collide with the recipient's own already-live ids. A raw
    /// keyed insert (`BubbleStore::insert_bubble_raw`) then silently
    /// aliases/overwrites the recipient's own window/bubble/primitive at
    /// that id — owner report M5, "VOLUME drawn twice": a shared chart's
    /// own default Volume bubble landing on the recipient's own bubble 1.
    ///
    /// Remaps, consistently, every window id (`ChartWindowSnapshot::
    /// window_id`) and every bubble id (`DataBubbleSnapshot::id` — the same
    /// id space a `Tag` is keyed by: see `mlc_core::ids::BubbleId`'s own
    /// doc), rewriting every in-preset reference to each
    /// (`bubbles[].members`, chart entries of `member_overrides`,
    /// `windows[].bubble_id`, `indicators[].bubble_id`) so the preset stays
    /// internally consistent — a window's `bubble_id` always follows its
    /// bubble. Also remaps every drawing-primitive id embedded anywhere in
    /// the preset (bubble-owned, window-owned, stashed, and
    /// per-symbol-cache primitives) via [`PrimitiveSnapshot::remap_ids`],
    /// fixing up `origin_id` / `window_id` / Elliott `link.parent_id` /
    /// `follows` references along with it.
    ///
    /// Deliberately NOT remapped:
    /// - `leaf_id` — purely structural, scoped to this preset's own
    ///   docking-tree JSON (`layout`), rebuilt fresh on every load and
    ///   never compared across presets.
    /// - `PanelId` (`slot_leaves`, `panel_members`, `synced_panel_ids`, the
    ///   Panel arm of `member_overrides`) and indicator-instance ids
    ///   (`IndicatorSnapshot::id`/`origin_id`, `IndicatorGroupConfig::id`,
    ///   primitive `pane_id`) — both live in a store the app
    ///   wholesale-replaces per preset (`TradingPanelsStore::new()` on
    ///   park; `IndicatorManager::clear_all()` on cold load), so a numeric
    ///   collision with a DIFFERENT preset's own ids there cannot alias a
    ///   live value the way window/bubble/primitive ids — which are NOT
    ///   reset per preset — can.
    /// Returns author primitive id → the id this process assigned.
    /// Callers that only need the remap can ignore the map.
    pub fn remap_ids_for_import(&mut self, keep_primitive_ids: bool) -> std::collections::HashMap<u64, u64> {
        use std::collections::HashMap;

        let window_id_map: HashMap<u64, u64> = self
            .windows
            .iter()
            .map(|w| (w.window_id, crate::ids::generate_chart_id().0))
            .collect();
        let bubble_id_map: HashMap<u64, u64> = self
            .bubbles
            .iter()
            .map(|b| (b.id, crate::ids::BubbleId::generate().0))
            .collect();

        for window in &mut self.windows {
            if let Some(&new_id) = window_id_map.get(&window.window_id) {
                window.window_id = new_id;
            }
            window.bubble_id = window.bubble_id.and_then(|bid| bubble_id_map.get(&bid).copied());
        }
        for bubble in &mut self.bubbles {
            if let Some(&new_id) = bubble_id_map.get(&bubble.id) {
                bubble.id = new_id;
            }
            bubble.members = bubble
                .members
                .iter()
                .filter_map(|id| window_id_map.get(id).copied())
                .collect();
            bubble.member_overrides.retain_mut(|ov| {
                // kind 0 = Chart (this preset's own window id space —
                // remap or drop a dangling reference); kind 1 = Panel
                // (untouched, see the doc comment above).
                if ov.kind == 0 {
                    match window_id_map.get(&ov.id) {
                        Some(&new_id) => {
                            ov.id = new_id;
                            true
                        }
                        None => false,
                    }
                } else {
                    true
                }
            });
        }
        for ind in &mut self.indicators {
            ind.bubble_id = ind.bubble_id.and_then(|bid| bubble_id_map.get(&bid).copied());
        }

        // A share viewer watches the author's objects. Their numbers stay.
        // Window ids inside each primitive still follow the remapped window,
        // or the drawings name a window that no longer exists and the reload
        // drops them. Clone passes false: that chart is new data.
        if keep_primitive_ids {
            let mut identity: HashMap<u64, u64> = HashMap::new();
            let mut note = |snaps: &[PrimitiveSnapshot]| {
                for snap in snaps {
                    if let Some(old_id) = snap.id() {
                        identity.insert(old_id, old_id);
                    }
                }
            };
            for bubble in &self.bubbles {
                note(&bubble.primitives);
            }
            for window in &self.windows {
                note(&window.drawings.primitives);
                note(&window.stashed_primitives);
                for snaps in window.symbol_drawings_snapshots.values() {
                    note(snaps);
                }
            }
            let rewrite = |snaps: &mut Vec<PrimitiveSnapshot>| {
                for snap in snaps.iter_mut() {
                    *snap = snap.remap_ids(&identity, &window_id_map);
                }
            };
            for bubble in &mut self.bubbles {
                rewrite(&mut bubble.primitives);
            }
            for window in &mut self.windows {
                rewrite(&mut window.drawings.primitives);
                rewrite(&mut window.stashed_primitives);
                for snaps in window.symbol_drawings_snapshots.values_mut() {
                    rewrite(snaps);
                }
            }
            return HashMap::new();
        }

        // Primitive ids: collect one fresh id per unique old id across
        // every collection that carries `PrimitiveSnapshot`s, then rewrite
        // every snapshot + every internal reference in a second pass.
        let mut primitive_id_map: HashMap<u64, u64> = HashMap::new();
        let mut collect_ids = |snaps: &[PrimitiveSnapshot]| {
            for snap in snaps {
                if let Some(old_id) = snap.id() {
                    primitive_id_map
                        .entry(old_id)
                        .or_insert_with(crate::drawing::alloc_primitive_id);
                }
            }
        };
        for bubble in &self.bubbles {
            collect_ids(&bubble.primitives);
        }
        for window in &self.windows {
            collect_ids(&window.drawings.primitives);
            collect_ids(&window.stashed_primitives);
            for snaps in window.symbol_drawings_snapshots.values() {
                collect_ids(snaps);
            }
        }
        drop(collect_ids);

        let remap_snaps = |snaps: &mut Vec<PrimitiveSnapshot>| {
            for snap in snaps.iter_mut() {
                *snap = snap.remap_ids(&primitive_id_map, &window_id_map);
            }
        };
        for bubble in &mut self.bubbles {
            remap_snaps(&mut bubble.primitives);
        }
        for window in &mut self.windows {
            remap_snaps(&mut window.drawings.primitives);
            remap_snaps(&mut window.stashed_primitives);
            for snaps in window.symbol_drawings_snapshots.values_mut() {
                remap_snaps(snaps);
            }
        }
        let _ = remap_snaps;
        primitive_id_map
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
