//! Unified member identifier for sync groups.
//!
//! A sync group can contain both chart windows and trading panels (DOM, Footprint, etc.).
//! `SyncMemberId` wraps both kinds without importing cross-crate panel types directly into
//! `zengeld-chart`, keeping crate boundaries clean.

use serde::{Deserialize, Serialize};

// =============================================================================
// SyncMemberId
// =============================================================================

/// A member in a sync group — either a chart window or a trading panel.
///
/// The raw `u64` inside each variant is the ID from the respective ID space:
/// - `Chart(id)` — corresponds to `ChartId(id)` from `chart_window`
/// - `Panel(id)` — corresponds to `PanelId(id)` from `sidebar-content`
///
/// Adapters (`From<ChartId>` and `From<PanelId>`) live in `chart-app` to
/// avoid introducing a cross-crate dependency here.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SyncMemberId {
    /// A chart window member.
    Chart(u64),
    /// A trading panel member (DOM, Footprint, Volume Profile, etc.).
    Panel(u64),
}

impl SyncMemberId {
    /// Returns `true` if this member is a chart window.
    pub fn is_chart(&self) -> bool {
        matches!(self, Self::Chart(_))
    }

    /// Returns `true` if this member is a trading panel.
    pub fn is_panel(&self) -> bool {
        matches!(self, Self::Panel(_))
    }

    /// Returns the inner id if this is a `Chart` variant.
    pub fn as_chart(&self) -> Option<u64> {
        if let Self::Chart(id) = self {
            Some(*id)
        } else {
            None
        }
    }

    /// Returns the inner id if this is a `Panel` variant.
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

/// Per-member override for sync flags.
///
/// Only the flags set to `Some(...)` override the group's defaults.
/// Fields left as `None` fall back to the group-level [`SyncFlags`].
///
/// For trading panels (DOM, Footprint, etc.) only `sync_symbol` and
/// `sync_crosshair` are meaningful — `sync_timeframe`, `sync_viewport`,
/// `sync_drawings`, and `sync_indicators` are always absent (panels have no
/// time axis, viewport, or indicator system).
///
/// [`SyncFlags`]: crate::tag_manager::SyncFlags
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MemberSyncOverride {
    /// Override for symbol synchronisation. `None` = use group default.
    pub sync_symbol: Option<bool>,
    /// Override for crosshair synchronisation. `None` = use group default.
    pub sync_crosshair: Option<bool>,
}
