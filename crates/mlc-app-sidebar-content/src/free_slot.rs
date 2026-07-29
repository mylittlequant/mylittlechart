//! Per-slot docking grid types for the free-slot hyperspace (typed slots).
//!
//! Each of the 5 slot sidebars hosts its own `DockingManager<Panel>`. The
//! set `{Main, SlotTrading, SlotDom, SlotTape, SlotL2, SlotBigTrades}` forms
//! a single cross-container drag hyperspace. Since the typed-slot redesign
//! (2026-07-11) each slot is locked to ONE [`SlotKind`] — its `[+]` spawns
//! only panels of that kind, each new instance bound to its own instrument.
//!
//! `Panel` is the unified docking-payload enum defined in `mlc_core::state`.
//! K1.3: `FreeItem` removed; `Panel` is the single source of truth.
//!
//! See `docs/plans/chart-subpane-pid-refactor-2026-06-03.md`.

// =============================================================================
// SlotKind
// =============================================================================

/// The 5 typed right-sidebar slots. Index (0..=4) is the canonical ordering
/// used everywhere a slot is addressed positionally: `SidebarState::slot_dockings`,
/// `RightSidebarPanel::slot_index()` / `from_slot_index()`, and the rail
/// toolbar button ids (`slot_trading` .. `slot_big_trades`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    /// Hosts `Panel::TradingContainer` only — its children (order entry,
    /// positions, trade log, risk calc) are managed INSIDE the container,
    /// which renders as one self-contained leaf in the sidebar dock.
    Trading,
    /// Hosts `Panel::Dom` (multi-exchange view is a mode toggle on Dom).
    Dom,
    /// Hosts `Panel::TradeTape`.
    Tape,
    /// Hosts `Panel::L2Tape`.
    L2,
    /// Hosts `Panel::BigTrades`.
    BigTrades,
}

/// Number of typed right-sidebar slots — length of `SidebarState::slot_dockings`
/// and `SlotKind::ALL`.
pub const SLOT_COUNT: usize = 5;

impl SlotKind {
    /// All 5 kinds in canonical index order.
    pub const ALL: [SlotKind; SLOT_COUNT] = [
        SlotKind::Trading,
        SlotKind::Dom,
        SlotKind::Tape,
        SlotKind::L2,
        SlotKind::BigTrades,
    ];

    /// Canonical slot index (0..=4) — matches `slot_dockings[idx]`.
    pub fn index(self) -> usize {
        match self {
            SlotKind::Trading   => 0,
            SlotKind::Dom       => 1,
            SlotKind::Tape      => 2,
            SlotKind::L2        => 3,
            SlotKind::BigTrades => 4,
        }
    }

    /// Construct from a canonical slot index (0..=4). `None` when out of range.
    pub fn from_index(idx: usize) -> Option<Self> {
        match idx {
            0 => Some(SlotKind::Trading),
            1 => Some(SlotKind::Dom),
            2 => Some(SlotKind::Tape),
            3 => Some(SlotKind::L2),
            4 => Some(SlotKind::BigTrades),
            _ => None,
        }
    }

    /// Human-readable label for headers / rail tooltips.
    pub fn label(self) -> &'static str {
        match self {
            SlotKind::Trading   => "Trading",
            SlotKind::Dom       => "DOM",
            SlotKind::Tape      => "Trade Tape",
            SlotKind::L2        => "L2 Tape",
            SlotKind::BigTrades => "Big Trades",
        }
    }

    /// The `Panel` kind strings (as used by `render_slot_spawn_dropdown` /
    /// `panel_click.rs`'s `slot:{idx}:spawn:{kind_str}` dispatch) this slot
    /// kind accepts. Every slot accepts exactly one kind (Multi-DOM merged
    /// into Dom as a mode toggle, Stage A 2026-07-30). `SlotTrading` hosts
    /// only `trading_container` — OrderEntry / PositionManager / TradeLog /
    /// RiskCalculator live INSIDE the container (owner spec 2026-07-25).
    pub fn allowed_panel_kinds(self) -> &'static [&'static str] {
        match self {
            SlotKind::Trading   => &["trading_container"],
            SlotKind::Dom       => &["dom"],
            SlotKind::Tape      => &["trade_tape"],
            SlotKind::L2        => &["l2_tape"],
            SlotKind::BigTrades => &["big_trades"],
        }
    }

    /// The default panel kind string spawned on auto-seed (first open of an
    /// empty slot).
    pub fn default_panel_kind(self) -> &'static str {
        self.allowed_panel_kinds()[0]
    }
}

// =============================================================================
// PanelId — re-exported from mlc-core (K1 single source of truth)
// =============================================================================

/// Re-export of `mlc_core::state::PanelId`.  Existing
/// `mlc_app_sidebar_content::free_slot::PanelId` imports keep compiling; new
/// code should import from `mlc_core::state` directly.
pub use mlc_core::state::PanelId;

// =============================================================================
// SlotDockingManager — Clone/Debug wrapper
// =============================================================================

/// Newtype around `DockingManager<Panel>` providing manual `Clone` + `Debug`
/// so it can live inside the `#[derive]`-d `SidebarState`.
///
/// `Clone` returns an empty manager — same policy as `AgentDockingManager`,
/// used only for `SidebarState` snapshot/undo scenarios where a clean slate
/// is desired.
pub struct SlotDockingManager(pub uzor::layout::DockState<mlc_core::state::Panel>);

impl SlotDockingManager {
    pub fn new() -> Self {
        Self(uzor::layout::DockState::new())
    }

    pub fn inner(&self) -> &uzor::layout::DockState<mlc_core::state::Panel> {
        &self.0
    }

    pub fn inner_mut(&mut self) -> &mut uzor::layout::DockState<mlc_core::state::Panel> {
        &mut self.0
    }
}

impl Default for SlotDockingManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SlotDockingManager {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SlotDockingManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlotDockingManager").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod slot_kind_tests {
    use super::{SlotKind, SLOT_COUNT};

    #[test]
    fn slot_count_matches_all_len() {
        assert_eq!(SLOT_COUNT, SlotKind::ALL.len());
        assert_eq!(SLOT_COUNT, 5);
    }

    /// `index()` / `from_index()` round-trip for every kind, and `ALL` is in
    /// canonical index order (0=Trading .. 4=BigTrades).
    #[test]
    fn index_round_trip_and_all_order() {
        for (expected_idx, kind) in SlotKind::ALL.iter().enumerate() {
            assert_eq!(kind.index(), expected_idx);
            assert_eq!(SlotKind::from_index(expected_idx), Some(*kind));
        }
        assert_eq!(SlotKind::ALL[0], SlotKind::Trading);
        assert_eq!(SlotKind::ALL[1], SlotKind::Dom);
        assert_eq!(SlotKind::ALL[2], SlotKind::Tape);
        assert_eq!(SlotKind::ALL[3], SlotKind::L2);
        assert_eq!(SlotKind::ALL[4], SlotKind::BigTrades);
    }

    #[test]
    fn from_index_out_of_range_is_none() {
        assert_eq!(SlotKind::from_index(5), None);
        assert_eq!(SlotKind::from_index(usize::MAX), None);
    }

    /// Spawn-filter table (owner spec): every slot accepts exactly 1 kind
    /// (Multi-DOM is a mode toggle on Dom since the Stage-A merge).
    #[test]
    fn allowed_panel_kinds_table() {
        assert_eq!(
            SlotKind::Trading.allowed_panel_kinds(),
            &["trading_container"],
        );
        assert_eq!(SlotKind::Dom.allowed_panel_kinds(), &["dom"]);
        assert_eq!(SlotKind::Tape.allowed_panel_kinds(), &["trade_tape"]);
        assert_eq!(SlotKind::L2.allowed_panel_kinds(), &["l2_tape"]);
        assert_eq!(SlotKind::BigTrades.allowed_panel_kinds(), &["big_trades"]);
    }

    /// No panel kind string is shared between two different slot kinds —
    /// the type-lock partition is total and non-overlapping.
    #[test]
    fn allowed_panel_kinds_are_disjoint_across_slots() {
        let mut seen = std::collections::HashSet::new();
        for kind in SlotKind::ALL {
            for panel_kind_str in kind.allowed_panel_kinds() {
                assert!(
                    seen.insert(*panel_kind_str),
                    "panel kind '{}' claimed by more than one SlotKind",
                    panel_kind_str
                );
            }
        }
    }

    /// `default_panel_kind()` is always the first (and only) allowed kind.
    #[test]
    fn default_panel_kind_matches_spec() {
        assert_eq!(SlotKind::Trading.default_panel_kind(), "trading_container");
        assert_eq!(SlotKind::Dom.default_panel_kind(), "dom");
        assert_eq!(SlotKind::Tape.default_panel_kind(), "trade_tape");
        assert_eq!(SlotKind::L2.default_panel_kind(), "l2_tape");
        assert_eq!(SlotKind::BigTrades.default_panel_kind(), "big_trades");
    }

    #[test]
    fn label_is_distinct_per_kind() {
        let mut labels: Vec<&str> = SlotKind::ALL.iter().map(|k| k.label()).collect();
        let mut deduped = labels.clone();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(deduped.len(), labels.len(), "SlotKind::label() collision: {:?}", labels);
        labels.sort_unstable();
        assert_eq!(labels, vec!["Big Trades", "DOM", "L2 Tape", "Trade Tape", "Trading"]);
    }

    // === Auto-seed preconditions (T1d) ===
    //
    // The auto-seed path (`ChartOutEvent::ToggleSlot` handler in
    // `mlc-app/src/input/chart_out_events.rs`) does: on first open, if
    // `slot_dockings[idx].inner().tree().leaves().is_empty()`, spawn
    // `slot_kind.default_panel_kind()` via `spawn_panel_in_slot`. That method
    // requires `ChartApp` (BackendCore + panels_store), so the actual spawn
    // call is exercised by the mlc-app crate; here we pin the two invariants
    // this file owns that the auto-seed logic depends on.

    /// `SlotDockingManager::new()` — what every slot starts as (see
    /// `SidebarState::default()`) — is empty, matching the auto-seed guard's
    /// precondition (`leaves().is_empty()` is true for a fresh slot).
    #[test]
    fn fresh_slot_docking_manager_is_empty() {
        let mgr = super::SlotDockingManager::new();
        assert!(mgr.inner().tree().leaves().is_empty(), "a freshly-constructed slot must start empty");
    }

    /// The auto-seed's `default_panel_kind()` is always a member of that same
    /// slot's `allowed_panel_kinds()` — i.e. auto-seed never tries to spawn a
    /// kind the slot would reject (which would silently no-op via the
    /// defense-in-depth guard in `spawn_panel_in_slot`, leaving the slot
    /// empty forever on first open).
    #[test]
    fn auto_seed_default_kind_is_always_allowed() {
        for kind in SlotKind::ALL {
            let default = kind.default_panel_kind();
            assert!(
                kind.allowed_panel_kinds().contains(&default),
                "{:?}::default_panel_kind() = '{}' is not in its own allowed_panel_kinds()",
                kind, default,
            );
        }
    }
}
