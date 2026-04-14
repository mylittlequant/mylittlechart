//! [`ChartPreset`] — the top-level persisted unit for chart state.
//!
//! A preset captures the complete, restorable state of the chart application:
//! panel layout, per-window configuration, sync groups, and indicators.
//! The [`crate::preset::storage`] module handles reading/writing presets to disk.

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

use super::snapshots::{ChartWindowSnapshot, IndicatorSnapshot, SyncGroupSnapshot};
use alerts::AlertItem;

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
    /// Sync group snapshots (shared symbol/timeframe/indicators across windows).
    pub sync_groups: Vec<SyncGroupSnapshot>,
    /// All indicator instance snapshots across all windows.
    pub indicators: Vec<IndicatorSnapshot>,
    /// Alert items associated with this preset.
    #[serde(default)]
    pub alerts: Vec<AlertItem>,
    /// Per-leaf color tag assignments (LeafId → [r,g,b,a]).
    /// Persisted so tags survive preset switching.
    #[serde(default)]
    pub leaf_color_tags: std::collections::HashMap<u64, [f32; 4]>,
    /// Per-slot `DockingManager<FreeItem>` layout snapshots (4 slots).
    ///
    /// Each entry is a serialized `uzor::panels::serialize::LayoutSnapshot`
    /// JSON string, or `None` if the slot is empty. Slots host trading panels
    /// and detached mini-charts (see `sidebar_content::FreeItem`).
    #[serde(default)]
    pub slot_layouts: [Option<String>; 4],
    /// Per-slot leaf descriptors (parallel to `slot_layouts`). Each vec lists
    /// the `FreeItem` payloads for leaves in the corresponding slot.
    #[serde(default)]
    pub slot_leaves: [Vec<PersistedFreeLeaf>; 4],
}

/// Serializable descriptor for a single `FreeItem` leaf inside a slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedFreeLeaf {
    /// Leaf id from `LayoutSnapshot` (maps into the per-slot layout tree).
    pub leaf_id: u64,
    /// Stable panel id — unique across restarts, used to key state in the store.
    #[serde(default)]
    pub panel_id: u64,
    /// Which kind of `FreeItem` this leaf carries, including the minimal state
    /// snapshot needed to recreate the panel on restore.
    pub kind: PersistedFreeItemKind,
}

/// Local mirror of `zengeld_panels::trading::SymbolSource`.
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

/// Mirror of `sidebar_content::FreeItem` kept in the chart crate so the preset
/// schema has no dependency on `sidebar-content`.
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
    },
    L2Tape {
        #[serde(default)]
        source: PersistedSymbolSource,
    },
    OrderEntry {
        #[serde(default)]
        source: PersistedSymbolSource,
    },
    PositionManager,
    TradeLog,
    RiskCalculator,
    TradingContainer {
        #[serde(default)]
        source: PersistedSymbolSource,
        tick_size: f64,
        market_price: f64,
    },
}

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
            sync_groups: Vec::new(),
            indicators: Vec::new(),
            alerts: Vec::new(),
            leaf_color_tags: std::collections::HashMap::new(),
            slot_layouts: [None, None, None, None],
            slot_leaves: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
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
