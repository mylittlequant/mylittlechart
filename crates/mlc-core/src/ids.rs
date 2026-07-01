//! Shared identifier + sync types — vocabulary used by both the
//! backend data plane (`mlc-backend::bubble`, `mlc-backend::tag`) and the
//! app orchestrator (`mlc-app`).  Lives in `mlc-core` because both
//! layers speak about these but neither owns them as a domain concept.
//!
//! These types are intentionally light: no business logic, no
//! ownership of state — just IDs and small value types.

use serde::{Deserialize, Serialize};

// =============================================================================
// BubbleId
// =============================================================================

/// Unique identifier for a data bubble (the backend data hub keyed by
/// `(symbol, exchange, account_type, timeframe)`).  Paired 1:1 with a
/// `Tag` on the app side (`Tag.id == data_bubble_id`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BubbleId(pub u64);

static NEXT_BUBBLE_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

impl BubbleId {
    /// Generate a new unique `BubbleId`.
    pub fn generate() -> Self {
        BubbleId(NEXT_BUBBLE_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }

    /// Advance the counter past `raw` so future `generate()` calls never
    /// collide with restored preset IDs.
    pub fn bump_past(raw: u64) {
        let next = raw + 1;
        NEXT_BUBBLE_ID.fetch_max(next, std::sync::atomic::Ordering::SeqCst);
    }
}

// =============================================================================
// SyncFlags
// =============================================================================

/// Which properties are synchronised across the members of a sync
/// group.  Authoritative copy lives on the backend `DataBubble`; the
/// app inspects it through `TagManager` for UI rendering.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncFlags {
    pub sync_crosshair: bool,
    pub sync_viewport: bool,
    pub sync_symbol: bool,
    pub sync_timeframe: bool,
    pub sync_drawings: bool,
    pub sync_indicators: bool,
}

impl Default for SyncFlags {
    fn default() -> Self {
        Self {
            sync_crosshair: true,
            sync_viewport: true,
            sync_symbol: true,
            sync_timeframe: true,
            sync_drawings: true,
            sync_indicators: true,
        }
    }
}

// =============================================================================
// SyncMemberId
// =============================================================================

/// A member in a sync group — either a chart window or a trading panel.
///
/// The raw `u64` inside each variant is the ID from the respective ID
/// space:
/// - `Chart(id)` — corresponds to `ChartId(id)` from `chart_container`
/// - `Panel(id)` — corresponds to `PanelId(id)` from `sidebar-content`
///
/// Adapters (`From<ChartId>` and `From<PanelId>`) live in `mlc-app` to
/// avoid introducing a cross-crate dependency here.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SyncMemberId {
    Chart(u64),
    Panel(u64),
}

impl SyncMemberId {
    pub fn is_chart(&self) -> bool {
        matches!(self, Self::Chart(_))
    }

    pub fn is_panel(&self) -> bool {
        matches!(self, Self::Panel(_))
    }

    pub fn as_chart(&self) -> Option<u64> {
        if let Self::Chart(id) = self {
            Some(*id)
        } else {
            None
        }
    }

    pub fn as_panel(&self) -> Option<u64> {
        if let Self::Panel(id) = self {
            Some(*id)
        } else {
            None
        }
    }
}

// =============================================================================
// MemberSyncOverride
// =============================================================================

/// Per-member override for sync flags.  Only the flags set to
/// `Some(...)` override the group defaults.  Fields left as `None`
/// fall back to the group-level [`SyncFlags`].
///
/// For trading panels (DOM, Footprint, etc.) only `sync_symbol` and
/// `sync_crosshair` are meaningful — `sync_timeframe`, `sync_viewport`,
/// `sync_drawings`, and `sync_indicators` are always absent (panels
/// have no time axis, viewport, or indicator system).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MemberSyncOverride {
    pub sync_symbol: Option<bool>,
    pub sync_crosshair: Option<bool>,
}

// =============================================================================
// GroupView
// =============================================================================

/// Lightweight view of a sync-group used by render layers
/// (`mlc-app-layout`).  Built by the caller (`mlc-app`) from
/// `BubbleStore` + `TagStore` and passed in by value/slice.
///
/// Render layers MUST NOT depend on `DataBubble` or `BubbleStore` as
/// types — they accept this view instead.  That keeps `mlc-app-layout` in
/// the `mlc-core` dep-graph (no upward dep on `mlc-backend`).
#[derive(Clone, Debug)]
pub struct GroupView {
    pub id: BubbleId,
    pub color: [f32; 4],
    pub auto_created: bool,
    pub symbol: String,
    pub timeframe_name: String,
    pub members_count: usize,
    pub sync_flags: SyncFlags,
    /// Per-symbol aggregation: `(symbol, indicator_count, primitive_count)`.
    /// Built by the caller (`mlc-app`) so the render layer doesn't need
    /// to know the shape of `IndicatorGroupConfig` / `Primitive`.
    pub per_symbol_counts: Vec<(String, usize, usize)>,
}
