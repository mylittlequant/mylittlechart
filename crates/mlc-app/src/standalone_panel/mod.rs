//! `StandalonePanelRegistry` — outer-dock panels without `inner_docking`.
//!
//! Doctrine (`docs/mlc/plans/doctrine-cutover-2026-06-27.md` Step 10) asks
//! for a `HashMap<PanelId, PanelKind>` + per-kind state + `panel_id ->
//! tag_id` membership map for standalone panels that follow a tag (Dom,
//! Watchlist overlay, Alerts overlay).
//!
//! **Reality check (2026-07-02 audit, this module's introduction):** the
//! only genuinely tag-scoped multi-instance panels in the codebase today
//! (Dom, L2Tape, Footprint, LiquidityHeatmap, ...) are K1 unified-taxonomy
//! panels (`mlc_core::state::Panel`) that already join sync groups via
//! `SyncMemberId::Panel(pid)` and are routed through
//! `mlc_backend::tag::TagManager::panel_members(tag_id)` — a real,
//! already-correct `panel_id -> tag_id` membership map, just living in the
//! domain layer (backend) instead of here. Building a second, competing
//! membership map in this registry would fork that routing, not consolidate
//! it — so this registry does NOT duplicate `SyncMemberId::Panel` tracking.
//!
//! What genuinely matches "standalone panel state scattered on `ChartApp`,
//! not tag-scoped, not part of any registry" is narrower than the doctrine
//! text implies:
//!
//! * [`watchlist_modal`](WatchlistModalState) / `wl_group_name_input`
//!   (`WatchlistGroupNameInputState`) — global floating/draggable modal
//!   singletons (one instance for the whole app, not per-tag). Doctrine §1
//!   already names `WatchlistModalState` as one of the four canonical
//!   UI-proxies; it was sitting directly on `ChartApp` instead of under a
//!   named registry. Consolidated here.
//! * The `OrderbookDeltaForMultiDom` "Dom multi-fan-out" — symbol-scoped
//!   (not tag-scoped: a multi-exchange consolidated Dom panel tracks one
//!   symbol across N exchanges, independent of any bubble/tag), previously
//!   inlined as a `values_mut().filter(...)` scan directly in the tick
//!   loop. Replaced by [`StandalonePanelRegistry::multi_dom_ids_for_symbol`]
//!   — a named, testable lookup instead of an ad-hoc scan at the call site.
//!   Kept as a derived (on-demand) filter rather than a cached
//!   `symbol -> Vec<PanelId>` index: `TradingPanelsStore.multi_dom` panels
//!   are created/cloned/removed/restored from four separate call sites
//!   (`panels_store.rs`, `persistence/hydration_impl.rs`) and a cached
//!   index would need invalidation at every one of them — a classic
//!   double-bookkeeping desync trap for a handful of panels. `ObjectTree`
//!   size here is small (per-user panel count, not per-tick), so an
//!   on-demand filter costs nothing measurable.
//!
//! `Alerts overlay` has no standalone per-instance state to migrate: alerts
//! are rendered as one global cross-bubble list in the right sidebar
//! (`RightSidebarPanel::Alerts`), not a multi-instance panel — nothing to
//! register.

use mlc_app_layout::ui::modal_settings::{WatchlistGroupNameInputState, WatchlistModalState};

/// Registry for outer-dock standalone panels (no `inner_docking`).
///
/// Owns the global watchlist overlay modal singletons. See module docs for
/// why this is narrower than the doctrine's original `HashMap<PanelId,
/// PanelKind>` sketch — the multi-instance tag-following case is already
/// correctly handled by `TagManager::panel_members` (`SyncMemberId::Panel`).
#[derive(Debug, Default)]
pub struct StandalonePanelRegistry {
    /// Expanded watchlist overlay modal — open/closed by the watchlist
    /// sidebar header button or a keyboard shortcut. Displays all watchlist
    /// symbols in a searchable, scrollable list with price data and delete
    /// buttons.
    pub watchlist_modal: WatchlistModalState,
    /// State for the watchlist group name input modal (create new / rename).
    pub wl_group_name_input: WatchlistGroupNameInputState,
}

impl StandalonePanelRegistry {
    pub fn new() -> Self {
        Self {
            watchlist_modal: WatchlistModalState::new(),
            wl_group_name_input: WatchlistGroupNameInputState::new(),
        }
    }

    // Multi-DOM delta fan-out helper removed 2026-07-30 (multi-dom v2):
    // the panel reads per-exchange station trackers directly in tick().
}
