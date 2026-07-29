//! Trading Container: the single fixed-region trading workspace hosted by
//! the Trading sidebar tab (owner spec 2026-07-25 — zero slots, ONE
//! container filling the whole tab).
//!
//! v1 is a STATIC layout (no inner docking tree yet):
//! - center = DOM ladder
//! - right column (live-resizable width) = OrderEntry on top, RiskCalculator below
//! - bottom strip (live-resizable height, spans center + right column) = a
//!   tab row (`Positions (n) | Orders (n) | Trades (n)`, W3 2026-07-25) over
//!   whichever one of PositionManager / OrdersLog / TradeLog is active,
//!   full width — replaces the v1 PositionManager|TradeLog fixed split.
//!
//! The legacy `left_panel`/`right_panel`/`bottom_panel` `SubPanelSlot`
//! machinery (Footprint / VolumeProfile / BigTrades / L2Tape) predates this
//! topology and is kept for backwards compatibility, but it now only ever
//! carves space out of the DOM column — the new fixed regions always win.
//!
//! ## Region model (W1, quality arc 2026-07-25)
//!
//! [`TradingContainerState::regions`] is the single geometry source every
//! input router (click / hover / scroll / drag) ultimately agrees with:
//! child rects come straight from [`TradingContainerState::v1_layout_rects`]
//! (the same rects passed to each child's `render()`), and the 3 draggable
//! separators are computed alongside them (a 4th, PositionManager|TradeLog,
//! was retired in W3 2026-07-25 when the bottom strip became a tab strip).
//! `render()` uses `regions()` to
//! draw + register the separators; children still register their own
//! precise per-widget rects (DOM's `dom:body` BlackboxPanel, OrderEntry's
//! `oe:*` buttons, ...) — `regions()` does not re-litigate those, it is the
//! authority for the NEW separator geometry and a documented single source
//! of truth for where each child's outer rect sits.

use serde::{Serialize, Deserialize};
use crate::render::{RenderContext, TextAlign, TextBaseline};
use crate::panel_trait::TradingPanel;
use crate::trading::order_flow::dom::DomState;
use super::order_entry::OrderEntryState;
use super::position_manager::PositionManagerState;
use super::trade_log::TradeLogState;
use super::orders_log::OrdersLogState;
use super::risk_calculator::RiskCalculatorState;
use super::trading_metrics::TradingMetricsState;
use super::ticker_tape::TickerTapeState;

/// Minimum right-column width (px) below which the DOM|right-column
/// separator refuses to shrink it further — keeps OrderEntry's field
/// stack usable. Clamped against `w * 0.6` so it never inverts on a
/// narrow container.
const TC_RIGHT_COL_MIN_PX: f64 = 180.0;
/// Minimum bottom-strip height (px) below which the bottom-strip
/// separator refuses to shrink it further.
const TC_BOTTOM_MIN_PX: f64 = 100.0;
/// Hit-region padding (px, each side of the visual boundary) for the 4
/// draggable separators — mirrors the docking system's own separator hit
/// width (`sep_hit_w` in `mlc-app-sidebar-content::render`).
const TC_SEP_HIT_PX: f64 = 4.0;

/// Free mode: thickness of the container-edge drop bands (root-split
/// targets during a panel drag — "dock against the container side").
const TC_EDGE_BAND_PX: f32 = 20.0;
/// Default fraction of the right column's height given to OrderEntry (rest
/// goes to RiskCalculator). No owner spec for the exact split — chosen so
/// OrderEntry's Limit/Stop-Limit field stack fits comfortably.
const TC_RIGHT_SPLIT_DEFAULT: f64 = 0.55;
/// Min/max clamp for the OrderEntry|RiskCalculator inner split — no owner
/// spec; generous enough that neither pane collapses to nothing.
const TC_INNER_SPLIT_MIN: f64 = 0.2;
const TC_INNER_SPLIT_MAX: f64 = 0.8;
/// Bottom-strip tab row height (px) — `Positions (n) | Orders (n) | Trades
/// (n)`, W3 2026-07-25. Fixed, not live-resizable (unlike the 3 separators).
const TC_BOTTOM_TAB_HEIGHT: f64 = 22.0;
/// Trading-metrics strip height (px) — one-row fast-results widget under
/// the bottom strip (owner spec 2026-07-28). Fixed.
const TC_METRICS_HEIGHT: f64 = 22.0;
/// Ticker-tape strip height (px) — one-row watchlist marquee, the
/// container's bottom-most strip (owner spec 2026-07-28). Fixed.
const TC_TAPE_HEIGHT: f64 = 22.0;

/// Trading Container panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TradingContainerId(pub u64);

/// What occupies the container's CENTER (top-left) region (owner spec
/// 2026-07-26): DOM ladder or an embedded chart — ONLY these two (order-flow
/// slot panels live in their own sidebar tabs, never inside the container).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CenterKind {
    Dom,
    Chart,
}

impl Default for CenterKind {
    fn default() -> Self { Self::Dom }
}

impl CenterKind {
    pub fn label(&self) -> &'static str {
        match self { Self::Dom => "DOM", Self::Chart => "Chart" }
    }
    pub fn wire_id(&self) -> &'static str {
        match self { Self::Dom => "dom", Self::Chart => "chart" }
    }
    pub fn from_wire_id(id: &str) -> Option<Self> {
        match id { "dom" => Some(Self::Dom), "chart" => Some(Self::Chart), _ => None }
    }
}

/// Container interior layout mode (owner spec 2026-07-29): `Fixed` = the
/// static v1 region layout; `Free` = assembled/disassemblable docking on
/// the uzor engine (`DockState<TcLeaf>`) — the pilot for app-wide unified
/// docking (see `docs/mlc/plans/tc-free-panels-docking-2026-07-29.md`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TcLayoutMode {
    #[default]
    Fixed,
    Free,
}

impl TcLayoutMode {
    pub fn wire_id(&self) -> &'static str {
        match self { Self::Fixed => "fixed", Self::Free => "free" }
    }
    pub fn from_wire_id(id: &str) -> Option<Self> {
        match id { "fixed" => Some(Self::Fixed), "free" => Some(Self::Free), _ => None }
    }
}

/// A leaf panel of the Free-mode docking tree. Unlike [`TradingChild`]
/// (v1 fixed regions), Chart and Dom are BOTH first-class leaves here (the
/// exclusive `CenterKind` pair only governs Fixed mode), and the bottom
/// tables keep their internal Positions/Orders/Trades tab row as ONE leaf
/// (uzor-native Tabs adoption is a later increment).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TcLeaf {
    Chart,
    Dom,
    OrderEntry,
    RiskCalculator,
    /// The three bottom-table views are SEPARATE panels in Free mode
    /// (owner spec 2026-07-29): they migrate as one native tab stack —
    /// no "Tables panel with its own sub-tab row" nesting.
    Positions,
    Orders,
    Trades,
    Metrics,
    Tape,
}

impl TcLeaf {
    /// Wire id used in spawn/close/grip widget ids and the header spawn
    /// dropdown (`spawn:{wire_id}`). One leaf per kind (states are single
    /// instances on the container), so the wire id addresses a leaf.
    pub fn wire_id(&self) -> &'static str {
        match self {
            Self::Chart => "chart",
            Self::Dom => "dom",
            Self::OrderEntry => "order_entry",
            Self::RiskCalculator => "risk_calculator",
            Self::Positions => "positions",
            Self::Orders => "orders",
            Self::Trades => "trades",
            Self::Metrics => "metrics",
            Self::Tape => "tape",
        }
    }
    pub fn from_wire_id(id: &str) -> Option<Self> {
        match id {
            "chart" => Some(Self::Chart),
            "dom" => Some(Self::Dom),
            "order_entry" => Some(Self::OrderEntry),
            "risk_calculator" => Some(Self::RiskCalculator),
            "positions" => Some(Self::Positions),
            "orders" => Some(Self::Orders),
            "trades" => Some(Self::Trades),
            "metrics" => Some(Self::Metrics),
            "tape" => Some(Self::Tape),
        _ => None,
        }
    }
    pub const ALL: [TcLeaf; 9] = [
        TcLeaf::Chart,
        TcLeaf::Dom,
        TcLeaf::OrderEntry,
        TcLeaf::RiskCalculator,
        TcLeaf::Positions,
        TcLeaf::Orders,
        TcLeaf::Trades,
        TcLeaf::Metrics,
        TcLeaf::Tape,
    ];

    /// One-row strip panels (owner spec 2026-07-29): they dock ONLY as
    /// horizontal bands of their own fixed height — never full-height
    /// columns, never side slots, never tab stacks.
    pub fn is_strip(&self) -> bool {
        matches!(self, Self::Metrics | Self::Tape)
    }
}

impl uzor::panels::DockPanel for TcLeaf {
    fn title(&self) -> &str {
        match self {
            Self::Chart => "Chart",
            Self::Dom => "DOM",
            Self::OrderEntry => "Order Entry",
            Self::RiskCalculator => "Risk Calculator",
            Self::Positions => "Positions",
            Self::Orders => "Orders",
            Self::Trades => "Trades",
            Self::Metrics => "Metrics",
            Self::Tape => "Tape",
        }
    }
    fn type_id(&self) -> &'static str {
        match self {
            Self::Chart => "tc_chart",
            Self::Dom => "tc_dom",
            Self::OrderEntry => "tc_order_entry",
            Self::RiskCalculator => "tc_risk_calculator",
            Self::Positions => "tc_positions",
            Self::Orders => "tc_orders",
            Self::Trades => "tc_trades",
            Self::Metrics => "tc_metrics",
            Self::Tape => "tc_tape",
        }
    }
    fn min_size(&self) -> (f32, f32) {
        match self {
            Self::Chart => (160.0, 120.0),
            Self::Dom => (120.0, 160.0),
            Self::OrderEntry => (150.0, 120.0),
            Self::RiskCalculator => (150.0, 100.0),
            Self::Positions | Self::Orders | Self::Trades => (160.0, 60.0),
            // One-row strips — must never be forced taller than their row.
            Self::Metrics | Self::Tape => (80.0, 22.0),
        }
    }
    fn preferred_strip_height(&self) -> Option<f32> {
        // One-row strips keep exactly their row height wherever they dock
        // (the engine rewrites the fresh split's proportions from this).
        self.is_strip().then_some(22.0)
    }
}

/// Free-mode docking manager wrapper.
///
/// `RefCell` because `TradingPanel::render` takes `&self` while
/// `DockState::layout` needs `&mut` (same reason `last_size` is a `Cell`);
/// borrow discipline is strictly local — every borrow is dropped before
/// child renders run. Manual `Debug` because `DockState` carries live
/// drag/animation state that neither needs nor offers derives.
pub struct TcFreeDock(pub std::cell::RefCell<uzor::layout::DockState<TcLeaf>>);

impl std::fmt::Debug for TcFreeDock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TcFreeDock(..)")
    }
}

/// Which hosted child a v1 [`TradingRegion`] belongs to.
///
/// `PositionManager` / `OrdersLog` / `TradeLog` share ONE region (the bottom
/// strip's content area) — only whichever one [`BottomTab`] currently
/// selects is ever returned by [`TradingContainerState::regions`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TradingChild {
    Dom,
    OrderEntry,
    RiskCalculator,
    PositionManager,
    OrdersLog,
    TradeLog,
}

/// Which view the bottom strip's tab row currently shows (W3, 2026-07-25 —
/// replaces the v1 PositionManager|TradeLog fixed split).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BottomTab {
    Positions,
    Orders,
    Trades,
}

impl Default for BottomTab {
    fn default() -> Self { Self::Positions }
}

impl BottomTab {
    /// Local widget-id this tab registers as during render —
    /// `"{slot_prefix}:{wire_id}"` — and the exact string
    /// [`TradingContainerState::handle_click`] matches to switch tabs.
    fn wire_id(self) -> &'static str {
        match self {
            Self::Positions => "bstrip:positions",
            Self::Orders    => "bstrip:orders",
            Self::Trades    => "bstrip:trades",
        }
    }
}

/// A draggable separator between two v1 regions.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TradingSeparator {
    /// DOM | right column (OrderEntry/RiskCalculator) — drags `right_ratio`.
    DomRightCol,
    /// OrderEntry | RiskCalculator, inside the right column — drags `right_split`.
    OeRiskCalc,
    /// Main area | bottom strip — drags `bottom_ratio`.
    BottomStrip,
}

impl TradingSeparator {
    /// Local widget-id suffix this separator registers as during render —
    /// `"{slot_prefix}:{wire_id}"` — and the exact string the app-layer
    /// drag-start dispatcher matches to identify a separator drag.
    pub fn wire_id(self) -> &'static str {
        match self {
            Self::DomRightCol => "sep:dom_right",
            Self::OeRiskCalc  => "sep:oe_rc",
            Self::BottomStrip => "sep:bottom",
        }
    }

    /// Parse a local id (already stripped of the `slot_prefix`) back into a
    /// separator, if it matches one of the 3 wire ids exactly.
    pub fn from_wire_id(id: &str) -> Option<Self> {
        match id {
            "sep:dom_right" => Some(Self::DomRightCol),
            "sep:oe_rc"     => Some(Self::OeRiskCalc),
            "sep:bottom"    => Some(Self::BottomStrip),
            _ => None,
        }
    }
}

/// Which region kind a computed [`TradingRegion`] describes.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TradingRegionKind {
    Child(TradingChild),
    Separator(TradingSeparator),
}

/// One computed v1 region — child content or a draggable separator — with
/// its absolute screen rect. See [`TradingContainerState::regions`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TradingRegion {
    pub kind: TradingRegionKind,
    /// Absolute screen rect `(x, y, w, h)`.
    pub rect: (f64, f64, f64, f64),
}

/// Trading Container state (heavy data)
///
/// The container manages a DOM panel as its core, with optional
/// sub-panels on left, right, and bottom. All sub-panels share
/// the same symbol, price axis, and tick size.
///
/// NOT `Clone` since the Free-docking arc (2026-07-29): `free_dock` holds a
/// live `DockState` (drag state, animations) with no meaningful clone; no
/// call site ever cloned the whole container state.
#[derive(Debug)]
pub struct TradingContainerState {
    /// Symbol source binding (how to resolve which instrument to display)
    pub source: crate::trading::SymbolSource,

    /// Symbol being traded
    pub symbol: String,

    /// Exchange identifier string (e.g. "binance")
    pub exchange: String,

    /// Account type short label (e.g. "S", "F", "FI")
    pub account_type: String,

    /// Header-tab TF control's current value (trading-header-tf arc,
    /// 2026-07-25) — a display name string (`"1m"`, `"5m"`, ...), the same
    /// grammar `mlc_core::state::Timeframe::name` uses. Seeded at spawn/
    /// hydration from the active chart's live timeframe (fallback `"1m"`),
    /// then set by picking a row in the header TF button's favorites popup
    /// (`GranularityFavorites::starred_tfs` — the topbar's own store).
    ///
    /// No child of this container reads it yet: DOM is tick-by-tick
    /// (no candle concept), and `PositionManager`/`OrdersLog`/`TradeLog`
    /// are event logs, not TF-bucketed. The one plausible consumer today —
    /// the legacy `footprint`/`volume_profile` `SubPanelSlot` machinery's
    /// `candle_duration_ms` — is unreachable in the current UI (nothing
    /// ever sets `left_panel`/`right_panel`/`bottom_panel` away from
    /// `SubPanelSlot::None`), so wiring it now would be unverifiable. This
    /// field is store + display + persist ONLY until a real TF-consuming
    /// child exists (footprint sub-panel, or a future embedded mini-chart).
    pub timeframe: String,
    /// What the center region shows — DOM ladder or the embedded chart.
    pub center_kind: CenterKind,
    /// Tick size for price grid
    pub tick_size: f64,
    /// Market price (from exchange)
    pub market_price: f64,
    /// Center price for DOM ladder
    pub center_price: f64,

    /// Split ratios (0.0..1.0) — how much space each section gets.
    /// `left_ratio` is legacy-only (see [`Self::layout_rects`]). `right_ratio`
    /// and `bottom_ratio` are LIVE, drag-updated v1 knobs (W1, 2026-07-25):
    /// right column width fraction / bottom strip height fraction.
    pub left_ratio: f64,    // legacy left panel width ratio (default 0.3)
    pub right_ratio: f64,   // v1 right-column width ratio (default 0.3)
    pub bottom_ratio: f64,  // v1 bottom-strip height ratio (default 0.25)
    /// OrderEntry's height fraction of the right column (rest goes to
    /// RiskCalculator). New W1 field — drag-updated via the OE|RiskCalc
    /// separator.
    pub right_split: f64,
    /// Which view the bottom strip's tab row currently shows (W3,
    /// 2026-07-25). Replaces the v1 `bottom_split` fixed PositionManager|
    /// TradeLog split — only one view is ever visible now, full width.
    pub active_tab: BottomTab,

    /// Interior layout mode (owner spec 2026-07-29): Fixed v1 regions or
    /// Free uzor-docking. Persisted; switched via the tc_modal SETUP block.
    pub layout_mode: TcLayoutMode,
    /// Free-mode docking manager — built lazily on the first switch to
    /// `Free` (or at hydration when the persisted mode is Free) by
    /// [`Self::build_free_dock`]; `None` while in Fixed mode from spawn.
    pub free_dock: Option<TcFreeDock>,

    /// Last rendered container size `(w, h)`, cached by `render()` (which
    /// only takes `&self`, hence the `Cell`). Separator drags only receive
    /// incremental pixel deltas (see `TradingPanel::handle_drag_move`), not
    /// the container rect, so this is how they convert pixels back to
    /// ratios. Not persisted — always repopulated by the next render.
    pub last_size: std::cell::Cell<(f64, f64)>,
    /// Widget-id slot prefix of the last render, cached by `render_free`
    /// for the app-layer overlay pass ([`Self::render_free_overlay`] runs
    /// AFTER the embedded chart is painted, outside the panel render call,
    /// and has no other prefix source). Not persisted.
    pub last_slot_prefix: std::cell::RefCell<String>,

    /// DOM state (always present — base element, v1 center region)
    pub dom: DomState,

    /// Order entry — always-present hosted sub-panel (v1 right column, top)
    pub order_entry: OrderEntryState,
    /// Position manager — always-present hosted sub-panel (v1 bottom-left)
    pub position_manager: PositionManagerState,
    /// Trade log — always-present hosted sub-panel (v1 bottom strip, "Trades" tab)
    pub trade_log: TradeLogState,
    /// Orders log — always-present hosted sub-panel (v1 bottom strip,
    /// "Orders" tab, W3 2026-07-25). The missing orders log — open orders
    /// with per-row cancel + terminal-order history — distinct from
    /// `trade_log`'s fills.
    pub orders_log: OrdersLogState,
    /// Risk calculator — always-present hosted sub-panel (v1 right column, bottom)
    pub risk_calculator: RiskCalculatorState,
    /// Fast trading-results strip (owner spec 2026-07-28) — one row under
    /// the bottom strip; the Total-PnL summaries extracted out of
    /// PositionManager / TradeLog.
    pub metrics: TradingMetricsState,
    /// Watchlist ticker tape (owner spec 2026-07-28) — one-row marquee,
    /// the container's bottom-most strip.
    pub ticker_tape: TickerTapeState,
}

impl TradingContainerState {
    pub fn new(symbol: String, tick_size: f64, market_price: f64) -> Self {
        let mut dom = DomState::new(symbol.clone(), tick_size);
        dom.market_price = market_price;
        dom.center_price = market_price;

        Self {
            source: crate::trading::SymbolSource::default(),
            symbol: symbol.clone(),
            exchange: String::new(),
            account_type: String::new(),
            // Fallback only — spawn/hydration call sites overwrite this from
            // the active chart's live TF when one is known (see the field
            // doc comment above).
            timeframe: "1m".to_string(),
            tick_size,
            market_price,
            center_price: market_price,
            center_kind: CenterKind::default(),
            left_ratio: 0.3,
            right_ratio: 0.3,
            bottom_ratio: 0.25,
            right_split: TC_RIGHT_SPLIT_DEFAULT,
            active_tab: BottomTab::default(),
            layout_mode: TcLayoutMode::default(),
            free_dock: None,
            last_size: std::cell::Cell::new((0.0, 0.0)),
            last_slot_prefix: std::cell::RefCell::new(String::new()),
            dom,
            order_entry: OrderEntryState::new(symbol),
            position_manager: PositionManagerState::new(),
            trade_log: TradeLogState::new(),
            orders_log: OrdersLogState::new(),
            risk_calculator: RiskCalculatorState::new(),
            metrics: TradingMetricsState::new(),
            ticker_tape: TickerTapeState::new(),
        }
    }

    /// Calculate legacy sub-panel rects given a rect — used ONLY for the

    /// Clamp a pixel size (`total * ratio`) to `[min(px_min, total*0.6),
    /// total*0.6]`, then convert back to a ratio — the min-size guard for
    /// the two "outer" v1 separators (DOM|right-column, main|bottom-strip).
    /// `total <= 0.0` returns `ratio` unchanged (defensive — never divides
    /// by zero).
    fn clamp_outer_ratio(total: f64, ratio: f64, px_min: f64) -> f64 {
        if total <= 0.0 {
            return ratio;
        }
        let max_px = total * 0.6;
        let min_px = px_min.min(max_px);
        (total * ratio).clamp(min_px, max_px) / total
    }

    /// Clamp an inner split ratio (OrderEntry|RiskCalculator or
    /// PositionManager|TradeLog) to `[TC_INNER_SPLIT_MIN, TC_INNER_SPLIT_MAX]`.
    fn clamp_inner_split(split: f64) -> f64 {
        split.clamp(TC_INNER_SPLIT_MIN, TC_INNER_SPLIT_MAX)
    }

    /// Restore the spawn-time geometry: the 3 separator ratios AND every
    /// table's user-resized column widths (trading-container modal "Reset
    /// layout", owner spec 2026-07-28). In Free mode the docking tree is
    /// rebuilt from the (freshly reset) v1 geometry.
    pub fn reset_layout(&mut self) {
        self.right_ratio = 0.3;
        self.bottom_ratio = 0.25;
        self.right_split = TC_RIGHT_SPLIT_DEFAULT;
        self.position_manager.cols.reset();
        self.trade_log.cols.reset();
        self.orders_log.cols.reset();
        if self.layout_mode == TcLayoutMode::Free {
            self.free_dock = Some(self.build_free_dock());
        }
    }

    /// Build the Free-mode docking tree by migrating the CURRENT v1
    /// geometry as-is (owner spec 2026-07-29): same regions, same
    /// proportions — flipping the mode must not visibly rearrange anything.
    ///
    /// Tree shape: `V[ H[ center | V[OE; RC] ]; V[tables; V[metrics; tape]] ]`
    /// — the center leaf is Dom or Chart per `center_kind`.
    pub fn build_free_dock(&self) -> TcFreeDock {
        use uzor::panels::{DockingTree, SplitKind, PanelRect};
        use uzor::layout::DockState;

        let (mut w, mut h) = self.last_size.get();
        if w <= 0.0 || h <= 0.0 {
            // Not rendered yet this session (hydration path) — a plausible
            // default aspect; per-frame layout() re-fits the real rect.
            w = 900.0;
            h = 640.0;
        }
        let r = self.v1_layout_rects(0.0, 0.0, w, h);

        let center_leaf = match self.center_kind {
            CenterKind::Dom => TcLeaf::Dom,
            CenterKind::Chart => TcLeaf::Chart,
        };
        let mut tree: DockingTree<TcLeaf> = DockingTree::with_single_leaf(center_leaf);
        let center_id = tree.leaves()[0].id;
        // Build order matters — each split nests a fresh binary branch:
        //   V[C; B] → V[C; V[B; M]] → V[C; V[B; V[M; T]]]
        //   → V[ H[C|OE]; ... ] → V[ H[C | V[OE;RC]]; ... ]
        // The bottom tables migrate as ONE native tab stack
        // Positions|Orders|Trades (owner spec 2026-07-29: the three views
        // ARE the tabs — no nested sub-tab row).
        let tables_id = tree
            .split_leaf(center_id, SplitKind::SplitBottom, TcLeaf::Positions)
            .expect("split center|tables");
        tree.add_tab(tables_id, TcLeaf::Orders);
        tree.add_tab(tables_id, TcLeaf::Trades);
        if let Some(l) = tree.leaf_mut(tables_id) {
            l.active_tab = match self.active_tab {
                BottomTab::Positions => 0,
                BottomTab::Orders => 1,
                BottomTab::Trades => 2,
            };
        }
        let metrics_id = tree
            .split_leaf(tables_id, SplitKind::SplitBottom, TcLeaf::Metrics)
            .expect("split tables|metrics");
        let _tape_id = tree
            .split_leaf(metrics_id, SplitKind::SplitBottom, TcLeaf::Tape)
            .expect("split metrics|tape");
        let oe_id = tree
            .split_leaf(center_id, SplitKind::SplitRight, TcLeaf::OrderEntry)
            .expect("split center|oe");
        let _rc_id = tree
            .split_leaf(oe_id, SplitKind::SplitBottom, TcLeaf::RiskCalculator)
            .expect("split oe|rc");

        // Proportions (weights normalized by sum at layout time) from the
        // live pixel geometry. Each nested branch is binary.
        let main_h = r.dom.3.max(1.0);
        let tables_h = (r.bottom_tab_bar.3 + r.bottom_content.3).max(1.0);
        let strips_h = (r.metrics.3 + r.ticker_tape.3).max(1.0);
        let set_parent_props = |tree: &mut DockingTree<TcLeaf>, leaf, props: Vec<f64>| {
            if let Some(bid) = tree.find_parent_of_leaf(leaf).map(|b| b.id) {
                if let Some(b) = tree.find_branch_mut(bid) {
                    b.proportions = props;
                }
            }
        };
        // H[C | V[OE;RC]]
        set_parent_props(&mut tree, center_id, vec![r.dom.2.max(1.0), r.order_entry.2.max(1.0)]);
        // V[OE; RC]
        set_parent_props(&mut tree, oe_id, vec![r.order_entry.3.max(1.0), r.risk_calculator.3.max(1.0)]);
        // V[tables; V[metrics; tape]]
        set_parent_props(&mut tree, tables_id, vec![tables_h, strips_h]);
        // V[metrics; tape]
        set_parent_props(&mut tree, metrics_id, vec![r.metrics.3.max(1.0), r.ticker_tape.3.max(1.0)]);
        // Root-level V[ H[..]; V[..] ] — the branch holding the main row is
        // the root's single child's... walk: parent of the H-branch is the
        // outermost V created by the FIRST split. Its two children are the
        // (now nested) main row and the bottom stack.
        if let Some(bid) = tree.root().children.first().and_then(|n| n.branch_id()) {
            if let Some(b) = tree.find_branch_mut(bid) {
                b.proportions = vec![main_h, tables_h + strips_h];
            }
        }

        let mut ds = DockState::from_tree(tree);
        // Panel-type drop constraints (owner spec 2026-07-29):
        // - one-row strips (metrics/tape) dock ONLY as horizontal bands —
        //   any body zone maps to Up/Down by cursor half, edge drops only
        //   top/bottom, never Center (no strip tabs), never full-height;
        // - full panels can't subdivide a strip (no side splits of a 22px
        //   band, no tabs on it);
        // - full panel onto full panel: all 5 zones incl. Center = tabify.
        ds.set_drop_policy(Box::new(|dragged: &TcLeaf, target, zone, is_edge, frac: (f32, f32)| {
            use uzor::panels::DropZone;
            let target_strip = target.map(TcLeaf::is_strip).unwrap_or(false);
            if dragged.is_strip() {
                if is_edge {
                    return matches!(zone, DropZone::Up | DropZone::Down).then_some(zone);
                }
                return Some(if frac.1 < 0.5 { DropZone::Up } else { DropZone::Down });
            }
            if target_strip && !is_edge {
                return None;
            }
            Some(zone)
        }));
        ds.layout(PanelRect::new(0.0, 0.0, w as f32, h as f32));
        TcFreeDock(std::cell::RefCell::new(ds))
    }

    /// Resolve the Free-tree leaf holding `kind` (one leaf per kind — the
    /// panel states are single instances on the container).
    pub fn free_leaf_for(&self, kind: &TcLeaf) -> Option<uzor::panels::LeafId> {
        let dock = self.free_dock.as_ref()?;
        let ds = dock.0.borrow();
        ds.tree()
            .leaves()
            .iter()
            .find(|l| l.active_panel() == Some(kind))
            .map(|l| l.id)
    }

    /// Parse the `{leaf_raw}:{tab_idx}` tail of a `tcfree:tab[x]:` widget id.
    fn parse_tab_wire(rest: &str) -> Option<(uzor::panels::LeafId, usize)> {
        let mut parts = rest.splitn(2, ':');
        let leaf = parts.next()?.parse::<u64>().ok()?;
        let idx = parts.next()?.parse::<usize>().ok()?;
        Some((uzor::panels::LeafId(leaf), idx))
    }

    /// The leaf hosting `kind` in ANY tab (active or not) + the tab index —
    /// the stack-aware lookup spawn/close need (one instance per kind).
    pub fn free_leaf_hosting(&self, kind: &TcLeaf) -> Option<(uzor::panels::LeafId, usize)> {
        let dock = self.free_dock.as_ref()?;
        let ds = dock.0.borrow();
        ds.tree().leaves().iter().find_map(|l| {
            l.panels
                .iter()
                .position(|p| p == kind)
                .map(|idx| (l.id, idx))
        })
    }

    /// Whether the Free tree currently hosts a Chart leaf (drives the
    /// header's chart-mode controls in Free).
    pub fn has_free_chart_leaf(&self) -> bool {
        self.free_leaf_for(&TcLeaf::Chart).is_some()
    }

    /// Spawn a leaf of `kind` into the Free tree (owner spec 2026-07-29:
    /// the header's "+ Panel" dropdown). One instance per kind — spawning
    /// an existing kind just activates its leaf. A new leaf splits the
    /// LARGEST current leaf along its longer axis; an empty tree gets the
    /// leaf as its first panel.
    pub fn spawn_free_leaf(&mut self, kind: TcLeaf) {
        use uzor::panels::{DropZone, SplitKind};
        // Already hosted somewhere (possibly as an inactive tab) — just
        // bring it forward.
        if let Some((leaf, idx)) = self.free_leaf_hosting(&kind) {
            if let Some(dock) = &self.free_dock {
                let mut ds = dock.0.borrow_mut();
                if let Some(l) = ds.tree_mut().leaf_mut(leaf) {
                    l.active_tab = idx;
                }
                ds.set_active_leaf(leaf);
            }
            return;
        }
        let Some(dock) = &self.free_dock else { return };
        let mut ds = dock.0.borrow_mut();
        if kind.is_strip() {
            // One-row strips spawn as a full-width band docked to the
            // container bottom, at exactly their own height.
            ds.spawn_leaf_root_split(kind, DropZone::Down);
        } else if ds.tree().leaf_count() == 0 {
            ds.tree_mut().add_leaf(kind);
        } else {
            // Largest leaf by area, split along its longer axis.
            let largest = ds
                .panel_rects()
                .iter()
                .max_by(|a, b| {
                    (a.1.width * a.1.height)
                        .partial_cmp(&(b.1.width * b.1.height))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(id, r)| (*id, r.width >= r.height));
            if let Some((target, horizontal)) = largest {
                let dir = if horizontal { SplitKind::SplitRight } else { SplitKind::SplitBottom };
                ds.tree_mut().split_leaf(target, dir, kind);
            } else {
                ds.tree_mut().add_leaf(kind);
            }
        }
        // Re-layout with the last known area so panel_rects are fresh for
        // anything that reads them before the next render.
        let (w, h) = self.last_size.get();
        if w > 0.0 && h > 0.0 {
            ds.layout(uzor::panels::PanelRect::new(0.0, 0.0, w as f32, h as f32));
        }
    }

    /// Close `kind` — the overlay ✕ chip / a tab-chip ✕. Stack-aware: a
    /// tab leaves its stack, a sole panel removes its leaf. Zero leaves is
    /// a valid state (empty container + toolbar).
    pub fn close_free_leaf(&mut self, kind: &TcLeaf) {
        let Some((leaf, idx)) = self.free_leaf_hosting(kind) else { return };
        if let Some(dock) = &self.free_dock {
            // `remove_tab` handles both cases: the stack shrinks, and an
            // emptied leaf is removed from the tree.
            dock.0.borrow_mut().tree_mut().remove_tab(leaf, idx);
        }
    }

    /// Absolute screen rect of the embedded chart, if one is visible —
    /// dispatches on the layout mode (Fixed: the center region when
    /// `center_kind == Chart`; Free: the `TcLeaf::Chart` leaf's rect). The
    /// app layer renders the real chart into this rect and routes its input.
    pub fn embedded_chart_rect(&self, x: f64, y: f64, w: f64, h: f64) -> Option<(f64, f64, f64, f64)> {
        match self.layout_mode {
            TcLayoutMode::Fixed => {
                (self.center_kind == CenterKind::Chart).then(|| self.v1_layout_rects(x, y, w, h).dom)
            }
            TcLayoutMode::Free => {
                let dock = self.free_dock.as_ref()?;
                let mut ds = dock.0.borrow_mut();
                ds.layout(uzor::panels::PanelRect::new(x as f32, y as f32, w as f32, h as f32));
                // The chart shows only while it is the ACTIVE tab of its
                // leaf; a stacked leaf's content starts below the chip bar.
                let (leaf, stacked) = ds
                    .tree()
                    .leaves()
                    .iter()
                    .find(|l| l.active_panel() == Some(&TcLeaf::Chart))
                    .map(|l| (l.id, l.tab_count() > 1))?;
                let bar_h = if stacked { ds.tab_bar_height() as f64 } else { 0.0 };
                ds.panel_rects().get(&leaf).map(|r| {
                    (
                        r.x as f64,
                        r.y as f64 + bar_h,
                        r.width as f64,
                        (r.height as f64 - bar_h).max(0.0),
                    )
                })
            }
        }
    }

    /// v1 fixed-region layout for the whole container: center (DOM or
    /// embedded chart), live right column (OrderEntry / RiskCalculator),
    /// live bottom strip (Positions / Orders / Trades tabs).
    pub fn v1_layout_rects(&self, x: f64, y: f64, w: f64, h: f64) -> TradingContainerV1Rects {
        // The two fixed one-row strips (metrics + ticker tape, owner spec
        // 2026-07-28) come off the bottom first; every ratio applies to the
        // remaining inner height.
        let strips_h = (TC_METRICS_HEIGHT + TC_TAPE_HEIGHT).min(h);
        let inner_h = (h - strips_h).max(0.0);

        let right_ratio = Self::clamp_outer_ratio(w, self.right_ratio, TC_RIGHT_COL_MIN_PX);
        let right_w = w * right_ratio;
        let bottom_ratio = Self::clamp_outer_ratio(inner_h, self.bottom_ratio, TC_BOTTOM_MIN_PX);
        let bottom_h = inner_h * bottom_ratio;
        let main_h = (inner_h - bottom_h).max(0.0);
        let dom_col_w = (w - right_w).max(0.0);

        let right_col_x = x + dom_col_w;
        let right_split = Self::clamp_inner_split(self.right_split);
        let oe_h = main_h * right_split;
        let rc_h = (main_h - oe_h).max(0.0);

        let bottom_y = y + main_h;
        let tab_bar_h = TC_BOTTOM_TAB_HEIGHT.min(bottom_h);
        let content_h = (bottom_h - tab_bar_h).max(0.0);

        let metrics_y = y + inner_h;
        let sep_hit = TC_SEP_HIT_PX;
        TradingContainerV1Rects {
            dom: (x, y, dom_col_w, main_h),
            order_entry: (right_col_x, y, right_w, oe_h),
            risk_calculator: (right_col_x, y + oe_h, right_w, rc_h),
            bottom_tab_bar: (x, bottom_y, w, tab_bar_h),
            bottom_content: (x, bottom_y + tab_bar_h, w, content_h),
            metrics: (x, metrics_y, w, TC_METRICS_HEIGHT.min(strips_h)),
            ticker_tape: (x, metrics_y + TC_METRICS_HEIGHT.min(strips_h), w, (strips_h - TC_METRICS_HEIGHT).max(0.0)),
            sep_dom_right: (right_col_x - sep_hit, y, sep_hit * 2.0, main_h),
            sep_oe_rc: (right_col_x, y + oe_h - sep_hit, right_w, sep_hit * 2.0),
            sep_bottom: (x, bottom_y - sep_hit, w, sep_hit * 2.0),
        }
    }

    /// All v1 regions (children + separators) as absolute screen rects —
    /// the single per-frame geometry source `render()` draws separators
    /// from and the app-layer drag-start dispatcher's widgets are
    /// registered against. See the module-level "Region model" doc.
    ///
    /// The bottom strip's content area yields exactly ONE `Child` region —
    /// whichever of PositionManager/OrdersLog/TradeLog `self.active_tab`
    /// currently selects (W3, 2026-07-25) — mirroring how the legacy
    /// left/right/bottom sub-panel rects are already `Option`-gated.
    pub fn regions(&self, x: f64, y: f64, w: f64, h: f64) -> Vec<TradingRegion> {
        let r = self.v1_layout_rects(x, y, w, h);
        let bottom_child = match self.active_tab {
            BottomTab::Positions => TradingChild::PositionManager,
            BottomTab::Orders => TradingChild::OrdersLog,
            BottomTab::Trades => TradingChild::TradeLog,
        };
        vec![
            TradingRegion { kind: TradingRegionKind::Child(TradingChild::Dom), rect: r.dom },
            TradingRegion { kind: TradingRegionKind::Child(TradingChild::OrderEntry), rect: r.order_entry },
            TradingRegion { kind: TradingRegionKind::Child(TradingChild::RiskCalculator), rect: r.risk_calculator },
            TradingRegion { kind: TradingRegionKind::Child(bottom_child), rect: r.bottom_content },
            TradingRegion { kind: TradingRegionKind::Separator(TradingSeparator::DomRightCol), rect: r.sep_dom_right },
            TradingRegion { kind: TradingRegionKind::Separator(TradingSeparator::OeRiskCalc), rect: r.sep_oe_rc },
            TradingRegion { kind: TradingRegionKind::Separator(TradingSeparator::BottomStrip), rect: r.sep_bottom },
        ]
    }
}

/// Calculated v1 fixed-region rects (x, y, w, h) — see
/// [`TradingContainerState::v1_layout_rects`]. `sep_*` fields are
/// hit-padded (see `TC_SEP_HIT_PX`), centered on the visual boundary.
#[derive(Clone, Debug)]
pub struct TradingContainerV1Rects {
    pub dom: (f64, f64, f64, f64),
    pub order_entry: (f64, f64, f64, f64),
    pub risk_calculator: (f64, f64, f64, f64),
    /// Bottom-strip tab row (`Positions (n) | Orders (n) | Trades (n)`).
    pub bottom_tab_bar: (f64, f64, f64, f64),
    /// Bottom-strip active-view content area (full width, below the tab row) —
    /// shared by PositionManager / OrdersLog / TradeLog; only one renders
    /// per frame, per `active_tab`.
    pub bottom_content: (f64, f64, f64, f64),
    /// Fast trading-results strip (one row, under the bottom strip).
    pub metrics: (f64, f64, f64, f64),
    /// Watchlist ticker tape (one row, bottom-most).
    pub ticker_tape: (f64, f64, f64, f64),
    pub sep_dom_right: (f64, f64, f64, f64),
    pub sep_oe_rc: (f64, f64, f64, f64),
    pub sep_bottom: (f64, f64, f64, f64),
}

// ---------------------------------------------------------------------------
// TradingPanel trait impl
// ---------------------------------------------------------------------------

impl TradingPanel for TradingContainerState {
    fn kind(&self) -> &'static str { "trading_container" }
    fn label(&self) -> &'static str { "Trading" }

    fn render(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        theme: &crate::panel_theme::PanelTheme,
        coordinator: &mut uzor::InputCoordinator,
        slot_prefix: &str,
    ) {
        // Cache this frame's size for separator drags (see `last_size` doc).
        self.last_size.set((w as f64, h as f64));

        // Background
        ctx.set_fill_color(&theme.tc_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        // Free-docking interior (owner spec 2026-07-29) — a wholly separate
        // geometry path; Fixed v1 below stays untouched.
        if self.layout_mode == TcLayoutMode::Free {
            if let Some(dock) = &self.free_dock {
                self.render_free(ctx, x, y, w, h, theme, coordinator, slot_prefix, dock);
                return;
            }
            // Free selected but no dock built (defensive) — fall through to
            // Fixed rather than render nothing.
        }

        // v1 fixed-region layout: center DOM, live right column, live
        // bottom strip. Children namespace their own widget ids off
        // `slot_prefix` (unchanged) — e.g. OrderEntry emits
        // "{slot_prefix}:oe:*" — so the outer click router can address them
        // exactly as it would a standalone instance of that panel kind.
        let rects = self.v1_layout_rects(x as f64, y as f64, w as f64, h as f64);

        // --- Center: DOM ladder ---
        let (dx, dy, dw, dh) = rects.dom;
        ctx.set_fill_color(&theme.tc_inner_bg);
        ctx.fill_rect(dx, dy, dw, dh);
        match self.center_kind {
            CenterKind::Dom => {
                self.dom.render(ctx, dx as f32, dy as f32, dw as f32, dh as f32, theme, coordinator, slot_prefix);
            }
            CenterKind::Chart => {
                // Embedded chart render lands with the chart-in-container
                // piece — honest empty state until then, no fake chrome.
                ctx.set_fill_color(&theme.text_muted);
                ctx.set_font("11px sans-serif");
                ctx.set_text_align(TextAlign::Center);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("Chart — подключается", dx + dw / 2.0, dy + dh / 2.0);
            }
        }

        // --- Right column: OrderEntry (top), RiskCalculator (bottom) ---
        let (ox, oy, ow, oh) = rects.order_entry;
        self.order_entry.render(ctx, ox as f32, oy as f32, ow as f32, oh as f32, theme, coordinator, slot_prefix);

        let (rcx, rcy, rcw, rch) = rects.risk_calculator;
        self.risk_calculator.render(ctx, rcx as f32, rcy as f32, rcw as f32, rch as f32, theme, coordinator, slot_prefix);

        // --- Bottom strip: tab row + active view, full width (W3, 2026-07-25) ---
        let (tbx, tby, tbw, tbh) = rects.bottom_tab_bar;
        self.render_bottom_tabs(ctx, tbx, tby, tbw, tbh, theme, coordinator, slot_prefix);

        let (bcx, bcy, bcw, bch) = rects.bottom_content;
        match self.active_tab {
            BottomTab::Positions => self.position_manager.render(ctx, bcx as f32, bcy as f32, bcw as f32, bch as f32, theme, coordinator, slot_prefix),
            BottomTab::Orders => self.orders_log.render(ctx, bcx as f32, bcy as f32, bcw as f32, bch as f32, theme, coordinator, slot_prefix),
            BottomTab::Trades => self.trade_log.render(ctx, bcx as f32, bcy as f32, bcw as f32, bch as f32, theme, coordinator, slot_prefix),
        }

        // --- Fixed strips: metrics row + ticker tape (owner spec
        // 2026-07-28), 1px separator above each. ---
        let (mx, my, mw, mh) = rects.metrics;
        if mh > 0.0 {
            ctx.set_fill_color(&theme.tc_separator);
            ctx.fill_rect(mx, my - 0.5, mw, 1.0);
            self.metrics.render(ctx, mx as f32, my as f32, mw as f32, mh as f32, theme, coordinator, slot_prefix);
        }
        let (tx2, ty2, tw2, th2) = rects.ticker_tape;
        if th2 > 0.0 {
            ctx.set_fill_color(&theme.tc_separator);
            ctx.fill_rect(tx2, ty2 - 0.5, tw2, 1.0);
            self.ticker_tape.render(ctx, tx2 as f32, ty2 as f32, tw2 as f32, th2 as f32, theme, coordinator, slot_prefix);
        }

        // --- Separators: single region-driven pass (draw + register drag
        // widgets) — the region model's whole reason to exist: one loop
        // instead of 4 hand-copied draw+register blocks. ---
        {
            use uzor::input::Sense;
            let regions = self.regions(x as f64, y as f64, w as f64, h as f64);
            for region in &regions {
                let TradingRegionKind::Separator(sep) = region.kind else { continue };
                let (rx, ry, rw, rh) = region.rect;
                // Visual line: thin bar centered in the (hit-padded) rect.
                ctx.set_fill_color(&theme.tc_separator);
                if rw >= rh {
                    // Horizontal separator (bottom-strip height, OE|RC split).
                    let line_y = ry + rh / 2.0 - 0.5;
                    ctx.fill_rect(rx, line_y, rw, 1.0);
                } else {
                    // Vertical separator (DOM|right-column, PM|TL split).
                    let line_x = rx + rw / 2.0 - 0.5;
                    ctx.fill_rect(line_x, ry, 1.0, rh);
                }
                let wid = format!("{}:{}", slot_prefix, sep.wire_id());
                coordinator.register(
                    wid.as_str(),
                    uzor::Rect::new(rx, ry, rw, rh),
                    Sense::DRAG | Sense::HOVER,
                );
            }
        }
    }

    fn handle_click(&mut self, local_id: &str, x: f64, y: f64) -> bool {
        if local_id.starts_with("oe:") {
            self.order_entry.handle_click(local_id, x, y)
        } else if local_id.starts_with("dom:") {
            self.dom.handle_click(local_id, x, y)
        } else if local_id.starts_with("position_manager:") {
            self.position_manager.handle_click(local_id, x, y)
        } else if local_id.starts_with("trade_log:") {
            self.trade_log.handle_click(local_id, x, y)
        } else if local_id.starts_with("orders:") {
            self.orders_log.handle_click(local_id, x, y)
        } else if local_id.starts_with("risk_calculator:") {
            self.risk_calculator.handle_click(local_id, x, y)
        } else if let Some(wire) = local_id.strip_prefix("tcfree:close:") {
            // Overlay ✕ chip — remove the leaf (owner spec 2026-07-29).
            if let Some(kind) = TcLeaf::from_wire_id(wire) {
                self.close_free_leaf(&kind);
                return true;
            }
            false
        } else if let Some(rest) = local_id.strip_prefix("tcfree:tabx:") {
            // Tab-chip ✕ — the panel leaves the stack (an emptied leaf is
            // removed by the tree).
            let Some((leaf, idx)) = Self::parse_tab_wire(rest) else { return false };
            if let Some(dock) = &self.free_dock {
                dock.0.borrow_mut().tree_mut().remove_tab(leaf, idx);
                return true;
            }
            false
        } else if let Some(rest) = local_id.strip_prefix("tcfree:tab:") {
            // Tab-chip click — activate that tab.
            let Some((leaf, idx)) = Self::parse_tab_wire(rest) else { return false };
            if let Some(dock) = &self.free_dock {
                let mut ds = dock.0.borrow_mut();
                if let Some(l) = ds.tree_mut().leaf_mut(leaf) {
                    if idx < l.panels.len() {
                        l.active_tab = idx;
                    }
                }
                ds.set_active_leaf(leaf);
                return true;
            }
            false
        } else if local_id.starts_with("bstrip:") {
            // Bottom-strip tab switch — fully local, no app-layer follow-up
            // (unlike "oe:submit"/"orders:cancel*").
            match local_id {
                "bstrip:positions" => { self.active_tab = BottomTab::Positions; true }
                "bstrip:orders" => { self.active_tab = BottomTab::Orders; true }
                "bstrip:trades" => { self.active_tab = BottomTab::Trades; true }
                _ => false,
            }
        } else {
            false
        }
    }

    fn handle_hover(&mut self, local_id: &str) -> bool {
        if local_id.starts_with("oe:") {
            self.order_entry.handle_hover(local_id)
        } else if local_id.starts_with("dom:") {
            self.dom.handle_hover(local_id)
        } else if local_id.starts_with("position_manager:") {
            self.position_manager.handle_hover(local_id)
        } else if local_id.starts_with("trade_log:") {
            self.trade_log.handle_hover(local_id)
        } else if local_id.starts_with("orders:") {
            self.orders_log.handle_hover(local_id)
        } else if local_id.starts_with("risk_calculator:") {
            self.risk_calculator.handle_hover(local_id)
        } else {
            false
        }
    }

    fn handle_scroll(&mut self, local_id: &str, dx: f64, dy: f64) -> bool {
        if local_id.starts_with("dom:") {
            // DOM's own wheel handling lives in its `BlackboxEvent::Scroll`
            // arm (the ctrl+scroll tick-size zoom stays an app-layer-only
            // concern — mirrors the standalone Dom slot's scroll dispatch).
            use crate::panel_trait::BlackboxEvent;
            self.dom.handle_blackbox_event(0.0, 0.0, BlackboxEvent::Scroll { dy: dy as f32 })
        } else if local_id.starts_with("position_manager:") {
            self.position_manager.handle_scroll(local_id, dx, dy)
        } else if local_id.starts_with("trade_log:") {
            self.trade_log.handle_scroll(local_id, dx, dy)
        } else if local_id.starts_with("orders:") {
            self.orders_log.handle_scroll(local_id, dx, dy)
        } else {
            false
        }
    }

    fn handle_drag_start(&mut self, local_id: &str, x: f64, y: f64) -> bool {
        if local_id.starts_with("dom:") {
            self.dom.handle_drag_start(local_id, x, y)
        } else if local_id.starts_with("position_manager:") {
            // Table column-boundary drags (owner spec 2026-07-28) — the
            // hosted table owns the gesture, same forwarding shape as
            // handle_click.
            self.position_manager.handle_drag_start(local_id, x, y)
        } else if local_id.starts_with("trade_log:") {
            self.trade_log.handle_drag_start(local_id, x, y)
        } else if local_id.starts_with("orders:") {
            self.orders_log.handle_drag_start(local_id, x, y)
        } else if local_id.starts_with("tape:") {
            self.ticker_tape.handle_drag_start(local_id, x, y)
        } else if let Some(wire) = local_id.strip_prefix("tcfree:grip:") {
            // Leaf move gesture (owner spec 2026-07-29): the grip chip
            // starts a DockState panel drag — compass targeting runs in
            // `handle_drag_move`, the drop lands in `handle_drag_end`.
            let Some(kind) = TcLeaf::from_wire_id(wire) else { return false };
            let Some(leaf) = self.free_leaf_for(&kind) else { return false };
            if let Some(dock) = &self.free_dock {
                dock.0.borrow_mut().start_panel_drag(leaf, x as f32, y as f32);
                return true;
            }
            false
        } else if let Some(rest) = local_id.strip_prefix("tcfree:tab:") {
            // Tab tear-out (owner spec 2026-07-29, tessera parity): drag a
            // chip out of its stack — same targeting as a leaf drag, the
            // single panel leaves the stack on drop.
            let Some((leaf, idx)) = Self::parse_tab_wire(rest) else { return false };
            if let Some(dock) = &self.free_dock {
                dock.0.borrow_mut().start_tab_drag(leaf, idx, x as f32, y as f32);
                return true;
            }
            false
        } else {
            // Separator drags are stateless per-frame nudges (see
            // `handle_drag_move`) — drag-start just needs to claim the
            // gesture so the app layer starts forwarding `handle_drag_move`.
            TradingSeparator::from_wire_id(local_id).is_some()
                || local_id.starts_with("tcfree:sep:")
        }
    }

    fn handle_drag_move(&mut self, local_id: &str, dx: f64, dy: f64) -> bool {
        if local_id.starts_with("dom:") {
            return self.dom.handle_drag_move(local_id, dx, dy);
        }
        if local_id.starts_with("position_manager:") {
            return self.position_manager.handle_drag_move(local_id, dx, dy);
        }
        if local_id.starts_with("trade_log:") {
            return self.trade_log.handle_drag_move(local_id, dx, dy);
        }
        if local_id.starts_with("orders:") {
            return self.orders_log.handle_drag_move(local_id, dx, dy);
        }
        if local_id.starts_with("tape:") {
            return self.ticker_tape.handle_drag_move(local_id, dx, dy);
        }
        // Leaf/tab move: advance the DockState panel drag by the frame
        // delta (drag-move only carries deltas; the drag state holds the
        // absolute cursor).
        if local_id.starts_with("tcfree:grip:") || local_id.starts_with("tcfree:tab:") {
            if let Some(dock) = &self.free_dock {
                let mut ds = dock.0.borrow_mut();
                if let Some(d) = ds.panel_drag_state() {
                    let (nx, ny) = (d.current_x + dx as f32, d.current_y + dy as f32);
                    ds.update_panel_drag(nx, ny);
                }
                return true;
            }
            return false;
        }
        // Free-mode tree separators: `tcfree:sep:{v|h}:{idx}` — axis picks
        // the drag delta, idx addresses `DockState::separators()` (stable
        // during a gesture: only proportions mutate, never the tree).
        if let Some(rest) = local_id.strip_prefix("tcfree:sep:") {
            let Some(dock) = &self.free_dock else { return false };
            let mut parts = rest.splitn(2, ':');
            let axis = parts.next().unwrap_or("");
            let Some(idx) = parts.next().and_then(|s| s.parse::<usize>().ok()) else {
                return false;
            };
            let delta = if axis == "v" { dx } else { dy };
            let (w, h) = self.last_size.get();
            if w <= 0.0 || h <= 0.0 {
                return true;
            }
            dock.0
                .borrow_mut()
                .drag_separator(idx, delta as f32, w as f32, h as f32);
            return true;
        }
        let Some(sep) = TradingSeparator::from_wire_id(local_id) else { return false };
        let (w, h) = self.last_size.get();
        if w <= 0.0 || h <= 0.0 {
            return true;
        }
        // Ratios apply to the inner height (total minus the two fixed
        // strips) — mirrors `v1_layout_rects`.
        let inner_h = (h - TC_METRICS_HEIGHT - TC_TAPE_HEIGHT).max(1.0);
        match sep {
            TradingSeparator::DomRightCol => {
                // Dragging right shrinks the right column (grows DOM).
                let right_w = (w * self.right_ratio - dx).max(0.0);
                self.right_ratio = Self::clamp_outer_ratio(w, right_w / w, TC_RIGHT_COL_MIN_PX);
            }
            TradingSeparator::OeRiskCalc => {
                let bottom_ratio = Self::clamp_outer_ratio(inner_h, self.bottom_ratio, TC_BOTTOM_MIN_PX);
                let main_h = (inner_h - inner_h * bottom_ratio).max(1.0);
                self.right_split = Self::clamp_inner_split(self.right_split + dy / main_h);
            }
            TradingSeparator::BottomStrip => {
                // Dragging down shrinks the bottom strip (grows the main area).
                let bottom_h = (inner_h * self.bottom_ratio - dy).max(0.0);
                self.bottom_ratio = Self::clamp_outer_ratio(inner_h, bottom_h / inner_h, TC_BOTTOM_MIN_PX);
            }
        }
        true
    }

    fn handle_drag_end(&mut self, local_id: &str) -> bool {
        if local_id.starts_with("dom:") {
            self.dom.handle_drag_end(local_id)
        } else if local_id.starts_with("position_manager:") {
            self.position_manager.handle_drag_end(local_id)
        } else if local_id.starts_with("trade_log:") {
            self.trade_log.handle_drag_end(local_id)
        } else if local_id.starts_with("orders:") {
            self.orders_log.handle_drag_end(local_id)
        } else if local_id.starts_with("tape:") {
            self.ticker_tape.handle_drag_end(local_id)
        } else if local_id.starts_with("tcfree:grip:") || local_id.starts_with("tcfree:tab:") {
            // Drop the dragged leaf/tab. Sides split, Center tabifies
            // (both already filtered by the dock's drop policy); a miss
            // cancels — no floating windows in this increment.
            if let Some(dock) = &self.free_dock {
                let mut ds = dock.0.borrow_mut();
                let droppable = matches!(
                    ds.panel_drag_state(),
                    Some(d) if d.target_leaf_id.is_some() && d.drop_zone.is_some()
                );
                if droppable {
                    let (w, h) = self.last_size.get();
                    ds.end_panel_drag(w as f32, h as f32);
                } else {
                    ds.cancel_panel_drag();
                }
            }
            true
        } else {
            TradingSeparator::from_wire_id(local_id).is_some()
                || local_id.starts_with("tcfree:sep:")
        }
    }
}

impl TradingContainerState {

    /// Free-mode interior render: per-frame uzor layout, then each leaf's
    /// existing panel render into its computed rect, then tree separators
    /// (drawn + registered as drag widgets). The dock borrow is dropped
    /// BEFORE any child render runs — children re-enter the coordinator,
    /// never the dock.
    #[allow(clippy::too_many_arguments)]
    fn render_free(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        theme: &crate::panel_theme::PanelTheme,
        coordinator: &mut uzor::InputCoordinator,
        slot_prefix: &str,
        dock: &TcFreeDock,
    ) {
        use uzor::input::Sense;
        use uzor::panels::{PanelRect, SeparatorOrientation};

        // Cache the widget-id namespace for the app-layer overlay pass
        // (`render_free_overlay` — chips + drag feedback drawn AFTER the
        // embedded chart, which would otherwise paint over them).
        if *self.last_slot_prefix.borrow() != slot_prefix {
            *self.last_slot_prefix.borrow_mut() = slot_prefix.to_string();
        }

        let (leaf_rects, seps) = {
            let mut ds = dock.0.borrow_mut();
            ds.layout(PanelRect::new(x, y, w, h));
            // Container-edge drop bands (root-split targets) — recomputed
            // per layout so a drag can dock against the container sides.
            ds.compute_window_edge_bands(TC_EDGE_BAND_PX);
            // Content rect per leaf — a stacked leaf's ACTIVE panel renders
            // below the tab-chip bar (the bar itself is drawn by
            // `render_free_overlay`, above the embedded chart).
            let bar_h = ds.tab_bar_height() as f64;
            let rects: Vec<(TcLeaf, (f64, f64, f64, f64))> = ds
                .tree()
                .leaves()
                .iter()
                .filter_map(|l| {
                    let p = l.active_panel()?.clone();
                    let r = ds.panel_rects().get(&l.id)?;
                    let shift = if l.tab_count() > 1 { bar_h } else { 0.0 };
                    Some((
                        p,
                        (
                            r.x as f64,
                            r.y as f64 + shift,
                            r.width as f64,
                            (r.height as f64 - shift).max(0.0),
                        ),
                    ))
                })
                .collect();
            let seps: Vec<(usize, SeparatorOrientation, f64, f64, f64)> = ds
                .separators()
                .iter()
                .enumerate()
                .map(|(i, s)| (i, s.orientation, s.position as f64, s.start as f64, s.length as f64))
                .collect();
            (rects, seps)
        };

        for (leaf, (lx, ly, lw, lh)) in &leaf_rects {
            let (lx, ly, lw, lh) = (*lx, *ly, *lw, *lh);
            if lw <= 1.0 || lh <= 1.0 {
                continue;
            }
            let (fx, fy, fw, fh) = (lx as f32, ly as f32, lw as f32, lh as f32);
            // HARD per-leaf clip (owner spec 2026-07-29): panel content
            // must never bleed past its cell (DOM price rows, table
            // columns, strips — nothing escapes the leaf or the sidebar).
            ctx.save();
            ctx.clip_rect(lx, ly, lw, lh);
            match leaf {
                TcLeaf::Dom => {
                    ctx.set_fill_color(&theme.tc_inner_bg);
                    ctx.fill_rect(lx, ly, lw, lh);
                    self.dom.render(ctx, fx, fy, fw, fh, theme, coordinator, slot_prefix);
                }
                TcLeaf::Chart => {
                    // The real chart is drawn by the app layer into
                    // `embedded_chart_rect()` — same contract as the Fixed
                    // center; this is just the honest backdrop.
                    ctx.set_fill_color(&theme.tc_inner_bg);
                    ctx.fill_rect(lx, ly, lw, lh);
                }
                TcLeaf::OrderEntry => {
                    self.order_entry.render(ctx, fx, fy, fw, fh, theme, coordinator, slot_prefix);
                }
                TcLeaf::RiskCalculator => {
                    self.risk_calculator.render(ctx, fx, fy, fw, fh, theme, coordinator, slot_prefix);
                }
                TcLeaf::Positions => {
                    self.position_manager.render(ctx, fx, fy, fw, fh, theme, coordinator, slot_prefix);
                }
                TcLeaf::Orders => {
                    self.orders_log.render(ctx, fx, fy, fw, fh, theme, coordinator, slot_prefix);
                }
                TcLeaf::Trades => {
                    self.trade_log.render(ctx, fx, fy, fw, fh, theme, coordinator, slot_prefix);
                }
                TcLeaf::Metrics => {
                    self.metrics.render(ctx, fx, fy, fw, fh, theme, coordinator, slot_prefix);
                }
                TcLeaf::Tape => {
                    self.ticker_tape.render(ctx, fx, fy, fw, fh, theme, coordinator, slot_prefix);
                }
            }
            ctx.restore();
        }

        // Tree separators: thin visual line + hit-padded drag widget. The
        // orientation is encoded in the wire id (`v`/`h`) so the input
        // layer picks the cursor and drag axis without reaching the dock.
        for (i, orientation, position, start, length) in &seps {
            ctx.set_fill_color(&theme.tc_separator);
            let (rx, ry, rw, rh, axis) = match orientation {
                SeparatorOrientation::Vertical => {
                    ctx.fill_rect(position - 0.5, *start, 1.0, *length);
                    (position - TC_SEP_HIT_PX, *start, TC_SEP_HIT_PX * 2.0, *length, "v")
                }
                SeparatorOrientation::Horizontal => {
                    ctx.fill_rect(*start, position - 0.5, *length, 1.0);
                    (*start, position - TC_SEP_HIT_PX, *length, TC_SEP_HIT_PX * 2.0, "h")
                }
            };
            let wid = format!("{}:tcfree:sep:{}:{}", slot_prefix, axis, i);
            coordinator.register(
                wid.as_str(),
                uzor::Rect::new(rx, ry, rw, rh),
                Sense::DRAG | Sense::HOVER,
            );
        }

        // NOTE: the per-leaf overlay chips and the drag feedback live in
        // `render_free_overlay` — an app-layer pass that runs AFTER the
        // embedded center chart is painted (the chart would paint over
        // anything drawn here).

        // Empty tree (all panels closed) — honest empty state + hint.
        if leaf_rects.is_empty() {
            ctx.set_fill_color(&theme.text_muted);
            ctx.set_font("12px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(
                "No panels — use “+ Panel” in the header",
                x as f64 + w as f64 / 2.0,
                y as f64 + h as f64 / 2.0,
            );
        }

    }

    /// Free-mode overlay pass — drawn by the APP layer after the embedded
    /// center chart (which paints over the container body and would eat
    /// anything drawn during the panel render — the reason this cannot live
    /// inside `render_free`).
    ///
    /// Draws, per frame:
    /// - the per-leaf chips `[grip ⠿ = move][✕ = close]` at the leaf's
    ///   top-right — ONLY while the pointer is inside that leaf and no
    ///   panel drag is active (owner spec 2026-07-29: hover-revealed, no
    ///   new title bars);
    /// - during a panel drag: the container-edge drop bands (root-split
    ///   targets), the armed drop-zone preview and a title ghost at the
    ///   cursor. Center (tabify) does not exist in this increment — the
    ///   dock classifies body drops sides-only.
    pub fn render_free_overlay(
        &self,
        ctx: &mut dyn RenderContext,
        theme: &crate::panel_theme::PanelTheme,
        coordinator: &mut uzor::InputCoordinator,
    ) {
        use uzor::input::Sense;
        use uzor::panels::DropZone;

        if self.layout_mode != TcLayoutMode::Free {
            return;
        }
        let Some(dock) = &self.free_dock else { return };
        let slot_prefix = self.last_slot_prefix.borrow().clone();
        if slot_prefix.is_empty() {
            return;
        }

        let ds = dock.0.borrow();
        let leaf_rects: Vec<(TcLeaf, (f64, f64, f64, f64))> = ds
            .tree()
            .leaves()
            .iter()
            .filter_map(|l| {
                let p = l.active_panel()?.clone();
                let r = ds.panel_rects().get(&l.id)?;
                Some((p, (r.x as f64, r.y as f64, r.width as f64, r.height as f64)))
            })
            .collect();
        let drag_active = ds.panel_drag_state().is_some();
        let pointer = coordinator.pointer_pos();

        // The whole overlay clips to the container area — chrome must not
        // bleed onto the sidebar rail or neighbouring panels.
        let area = ds.layout_area();
        ctx.save();
        ctx.clip_rect(area.x as f64, area.y as f64, area.width as f64, area.height as f64);

        // Tab-chip bars for stacked leaves — always visible (this IS the
        // tab switcher; tessera parity). A chip click activates its tab,
        // a chip drag tears the tab out, the chip ✕ closes it.
        let bar_h = ds.tab_bar_height() as f64;
        for bar in ds.tab_bars() {
            let leaf_raw = bar.container_id.0;
            if let Some(r) = ds.panel_rects().get(&bar.container_id) {
                ctx.set_fill_color(&theme.header_bg);
                ctx.fill_rect(r.x as f64, r.y as f64, r.width as f64, bar_h);
            }
            for (idx, tab) in bar.tabs.iter().enumerate() {
                let (tx, ty, tw, th) = (
                    tab.rect.x as f64,
                    tab.rect.y as f64,
                    tab.rect.width as f64,
                    tab.rect.height as f64,
                );
                if tw <= 1.0 {
                    continue;
                }
                let wid = format!("{}:tcfree:tab:{}:{}", slot_prefix, leaf_raw, idx);
                let hovered = coordinator.is_hovered(&uzor::WidgetId::from(wid.as_str()));
                ctx.set_fill_color(if tab.is_active {
                    &theme.tc_inner_bg
                } else if hovered {
                    &theme.hover
                } else {
                    &theme.header_bg
                });
                ctx.fill_rect(tx, ty, tw, th);
                if tab.is_active {
                    ctx.set_fill_color(&theme.accent);
                    ctx.fill_rect(tx, ty + th - 2.0, tw, 2.0);
                }
                ctx.set_fill_color(if tab.is_active { &theme.text_primary } else { &theme.text_muted });
                ctx.set_font("11px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(&tab.title, tx + 8.0, ty + th / 2.0);
                let (xx, xy, xs) = (
                    tab.close_rect.x as f64,
                    tab.close_rect.y as f64,
                    tab.close_rect.width as f64,
                );
                ctx.set_stroke_color(&theme.text_muted);
                ctx.set_stroke_width(1.0);
                ctx.set_line_dash(&[]);
                ctx.begin_path();
                ctx.move_to(xx + 4.0, xy + 4.0);
                ctx.line_to(xx + xs - 4.0, xy + xs - 4.0);
                ctx.stroke();
                ctx.begin_path();
                ctx.move_to(xx + xs - 4.0, xy + 4.0);
                ctx.line_to(xx + 4.0, xy + xs - 4.0);
                ctx.stroke();
                // Registration order matters: the ✕ is INSIDE the chip
                // rect and must be registered after it (last wins the
                // same-layer hit test).
                coordinator.register(
                    wid.as_str(),
                    uzor::Rect::new(tx, ty, tw, th),
                    Sense::CLICK | Sense::DRAG | Sense::HOVER,
                );
                let xid = format!("{}:tcfree:tabx:{}:{}", slot_prefix, leaf_raw, idx);
                coordinator.register(
                    xid.as_str(),
                    uzor::Rect::new(xx, xy, xs, xs),
                    Sense::CLICK | Sense::HOVER,
                );
            }
        }

        // Per-leaf overlay chips, hover-revealed.
        const CHIP: f64 = 16.0;
        const CHIP_GAP: f64 = 2.0;
        const CHIP_INSET: f64 = 3.0;
        if !drag_active {
            for (leaf, (lx, ly, lw, lh)) in &leaf_rects {
                let (lx, ly, lw, lh) = (*lx, *ly, *lw, *lh);
                if lw < CHIP * 2.0 + CHIP_GAP + CHIP_INSET * 2.0 || lh < CHIP + CHIP_INSET {
                    continue;
                }
                let inside = pointer
                    .map(|(px, py)| px >= lx && px <= lx + lw && py >= ly && py <= ly + lh)
                    .unwrap_or(false);
                if !inside {
                    continue;
                }
                let close_x = lx + lw - CHIP_INSET - CHIP;
                let grip_x = close_x - CHIP_GAP - CHIP;
                let cy = ly + CHIP_INSET.min((lh - CHIP).max(0.0) / 2.0);
                let wire = leaf.wire_id();

                for (cx, id_kind) in [(grip_x, "grip"), (close_x, "close")] {
                    let wid = format!("{}:tcfree:{}:{}", slot_prefix, id_kind, wire);
                    let hovered = coordinator.is_hovered(&uzor::WidgetId::from(wid.as_str()));
                    ctx.set_fill_color(if hovered { &theme.hover } else { &theme.header_bg });
                    ctx.fill_rounded_rect(cx, cy, CHIP, CHIP, 3.0);
                    ctx.set_stroke_color(if hovered { &theme.text_primary } else { &theme.text_muted });
                    ctx.set_stroke_width(1.2);
                    ctx.set_line_dash(&[]);
                    if id_kind == "grip" {
                        // Three horizontal bars.
                        for k in 0..3 {
                            let yy = cy + 4.5 + k as f64 * 3.5;
                            ctx.begin_path();
                            ctx.move_to(cx + 4.0, yy);
                            ctx.line_to(cx + CHIP - 4.0, yy);
                            ctx.stroke();
                        }
                    } else {
                        // Diagonal cross.
                        ctx.begin_path();
                        ctx.move_to(cx + 4.5, cy + 4.5);
                        ctx.line_to(cx + CHIP - 4.5, cy + CHIP - 4.5);
                        ctx.stroke();
                        ctx.begin_path();
                        ctx.move_to(cx + CHIP - 4.5, cy + 4.5);
                        ctx.line_to(cx + 4.5, cy + CHIP - 4.5);
                        ctx.stroke();
                    }
                    let sense = if id_kind == "grip" {
                        Sense::DRAG | Sense::HOVER
                    } else {
                        Sense::CLICK | Sense::HOVER
                    };
                    coordinator.register(wid.as_str(), uzor::Rect::new(cx, cy, CHIP, CHIP), sense);
                }
            }
        }

        // Live panel-drag feedback.
        let Some(drag) = ds.panel_drag_state() else {
            ctx.restore();
            return;
        };

        // Container-edge drop bands — visible for the whole drag so the
        // "dock against the container side" targets are discoverable;
        // the armed band lights up.
        if let Some(bands) = ds.window_edge_rects() {
            let band_zones = [DropZone::Up, DropZone::Down, DropZone::Left, DropZone::Right];
            for (i, b) in bands.iter().enumerate() {
                let armed = drag.is_window_edge && drag.drop_zone == Some(band_zones[i]);
                ctx.set_fill_color(&theme.hover_overlay);
                ctx.fill_rect(b.x as f64, b.y as f64, b.width as f64, b.height as f64);
                if armed {
                    ctx.set_stroke_color(&theme.accent);
                    ctx.set_stroke_width(1.5);
                    ctx.set_line_dash(&[]);
                    ctx.stroke_rect(
                        b.x as f64 + 1.0,
                        b.y as f64 + 1.0,
                        (b.width as f64 - 2.0).max(0.0),
                        (b.height as f64 - 2.0).max(0.0),
                    );
                }
            }
        }

        // Mini-compass on the hovered panel (tessera parity): four edge
        // ribs + a center square, the armed one lit — the drop lands in
        // THIS leaf, splitting it (or joining its stack on Center).
        if let (Some(tid), false) = (drag.target_leaf_id, drag.is_window_edge) {
            if let Some(r) = ds.panel_rects().get(&tid) {
                let (rx, ry, rw, rh) = (r.x as f64, r.y as f64, r.width as f64, r.height as f64);
                let (ccx, ccy) = (rx + rw / 2.0, ry + rh / 2.0);
                let rib_l = 26.0_f64.min(rw * 0.3).min(rh * 0.3).max(6.0);
                let rib_t = 6.0;
                let inset = 8.0;
                let ribs = [
                    (DropZone::Up, (ccx - rib_l / 2.0, ry + inset, rib_l, rib_t)),
                    (DropZone::Down, (ccx - rib_l / 2.0, ry + rh - inset - rib_t, rib_l, rib_t)),
                    (DropZone::Left, (rx + inset, ccy - rib_l / 2.0, rib_t, rib_l)),
                    (DropZone::Right, (rx + rw - inset - rib_t, ccy - rib_l / 2.0, rib_t, rib_l)),
                ];
                for (z, (zx, zy, zw, zh)) in ribs {
                    let armed = drag.drop_zone == Some(z);
                    ctx.set_fill_color(if armed { &theme.accent } else { &theme.text_muted });
                    ctx.fill_rounded_rect(zx, zy, zw, zh, 2.0);
                }
                let cs = 16.0;
                let armed = drag.drop_zone == Some(DropZone::Center);
                ctx.set_fill_color(if armed { &theme.accent } else { &theme.text_muted });
                ctx.fill_rounded_rect(ccx - cs / 2.0, ccy - cs / 2.0, cs, cs, 3.0);
            }
        }

        // Armed drop-zone preview: edge drop previews the root-split half
        // on the container area; a body drop previews the target-leaf half
        // (Center → the whole leaf: the tab join).
        let preview = drag.target_leaf_id.zip(drag.drop_zone).and_then(|(tid, zone)| {
            let base = if drag.is_window_edge {
                ds.layout_area()
            } else {
                *ds.panel_rects().get(&tid)?
            };
            Some(zone.preview_rect(base))
        });
        if let Some(p) = preview {
            ctx.set_fill_color(&theme.hover_overlay);
            ctx.fill_rect(p.x as f64, p.y as f64, p.width as f64, p.height as f64);
            ctx.set_stroke_color(&theme.accent);
            ctx.set_stroke_width(1.5);
            ctx.set_line_dash(&[]);
            ctx.stroke_rect(
                p.x as f64 + 1.0,
                p.y as f64 + 1.0,
                (p.width as f64 - 2.0).max(0.0),
                (p.height as f64 - 2.0).max(0.0),
            );
        }

        // Cursor ghost with the dragged panel's title.
        let title = ds
            .tree()
            .leaves()
            .iter()
            .find(|l| l.id == drag.dragged_leaf_id)
            .and_then(|l| l.active_panel())
            .map(|p| uzor::panels::DockPanel::title(p).to_string())
            .unwrap_or_default();
        let (cx, cy) = (drag.current_x as f64, drag.current_y as f64);
        let gw = 110.0;
        let gh = 22.0;
        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rounded_rect(cx + 12.0, cy + 12.0, gw, gh, 4.0);
        ctx.set_stroke_color(&theme.accent);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rect(cx + 12.0, cy + 12.0, gw, gh);
        ctx.set_fill_color(&theme.text_primary);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&title, cx + 12.0 + gw / 2.0, cy + 12.0 + gh / 2.0);

        ctx.restore();
    }

    /// Draw + register the bottom-strip tab row — `Positions (n) | Orders
    /// (n) | Trades (n)` — 3 equal-width click regions, counts read live off
    /// each hosted child's already-synced state (W3, 2026-07-25).
    #[allow(clippy::too_many_arguments)]
    fn render_bottom_tabs(
        &self,
        ctx: &mut dyn RenderContext,
        x: f64, y: f64, w: f64, h: f64,
        theme: &crate::panel_theme::PanelTheme,
        coordinator: &mut uzor::InputCoordinator,
        slot_prefix: &str,
    ) {
        use uzor::input::Sense;

        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x, y, w, h);

        let tabs = [
            (BottomTab::Positions, format!("Positions ({})", self.position_manager.positions.len())),
            (BottomTab::Orders, format!("Orders ({})", self.orders_log.open_orders.len())),
            (BottomTab::Trades, format!("Trades ({})", self.trade_log.trades.len())),
        ];
        let tab_w = w / tabs.len() as f64;

        for (i, (tab, label)) in tabs.iter().enumerate() {
            let tab_x = x + i as f64 * tab_w;
            let is_active = self.active_tab == *tab;
            let tab_id = format!("{}:{}", slot_prefix, tab.wire_id());
            let tab_hov = coordinator.is_hovered(&uzor::WidgetId::from(tab_id.as_str()));

            if is_active {
                ctx.set_fill_color(&theme.panel_bg);
                ctx.fill_rect(tab_x, y, tab_w, h);
                ctx.set_fill_color(&theme.oe_tab_active);
                ctx.fill_rect(tab_x, y + h - 2.0, tab_w, 2.0);
            } else if tab_hov {
                // Interactivity pass (owner spec 2026-07-26): inactive tab
                // brightens under the cursor; the active one already reads
                // as pressed.
                ctx.set_fill_color(&theme.hover);
                ctx.fill_rect(tab_x, y, tab_w, h);
            }

            ctx.set_fill_color(if is_active || tab_hov { &theme.text_primary } else { &theme.text_muted });
            ctx.set_font("10px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(label, tab_x + tab_w / 2.0, y + h / 2.0);

            coordinator.register(
                tab_id.as_str(),
                uzor::Rect::new(tab_x, y, tab_w, h),
                Sense::CLICK,
            );

            if i + 1 < tabs.len() {
                ctx.set_fill_color(&theme.tc_separator);
                ctx.fill_rect(tab_x + tab_w - 1.0, y, 1.0, h);
            }
        }
    }
}

/// Trading Container panel wrapper (lightweight, for PanelKind)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradingContainerPanel {
    id: TradingContainerId,
    title: String,
}

impl TradingContainerPanel {
    pub fn new(id: TradingContainerId, title: String) -> Self { Self { id, title } }
    pub fn id(&self) -> TradingContainerId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }
    pub fn type_id(&self) -> &'static str { "trading_container" }
    pub fn kind_label(&self) -> &'static str { "Trading" }
    pub fn min_size(&self) -> (f32, f32) { (400.0, 300.0) }
}
