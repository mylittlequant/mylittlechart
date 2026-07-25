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
#[derive(Clone, Debug)]
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

    /// Last rendered container size `(w, h)`, cached by `render()` (which
    /// only takes `&self`, hence the `Cell`). Separator drags only receive
    /// incremental pixel deltas (see `TradingPanel::handle_drag_move`), not
    /// the container rect, so this is how they convert pixels back to
    /// ratios. Not persisted — always repopulated by the next render.
    pub last_size: std::cell::Cell<(f64, f64)>,

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
            last_size: std::cell::Cell::new((0.0, 0.0)),
            dom,
            order_entry: OrderEntryState::new(symbol),
            position_manager: PositionManagerState::new(),
            trade_log: TradeLogState::new(),
            orders_log: OrdersLogState::new(),
            risk_calculator: RiskCalculatorState::new(),
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

    /// v1 fixed-region layout for the whole container: center (DOM or
    /// embedded chart), live right column (OrderEntry / RiskCalculator),
    /// live bottom strip (Positions / Orders / Trades tabs).
    pub fn v1_layout_rects(&self, x: f64, y: f64, w: f64, h: f64) -> TradingContainerV1Rects {
        let right_ratio = Self::clamp_outer_ratio(w, self.right_ratio, TC_RIGHT_COL_MIN_PX);
        let right_w = w * right_ratio;
        let bottom_ratio = Self::clamp_outer_ratio(h, self.bottom_ratio, TC_BOTTOM_MIN_PX);
        let bottom_h = h * bottom_ratio;
        let main_h = (h - bottom_h).max(0.0);
        let dom_col_w = (w - right_w).max(0.0);

        let right_col_x = x + dom_col_w;
        let right_split = Self::clamp_inner_split(self.right_split);
        let oe_h = main_h * right_split;
        let rc_h = (main_h - oe_h).max(0.0);

        let bottom_y = y + main_h;
        let tab_bar_h = TC_BOTTOM_TAB_HEIGHT.min(bottom_h);
        let content_h = (bottom_h - tab_bar_h).max(0.0);

        let sep_hit = TC_SEP_HIT_PX;
        TradingContainerV1Rects {
            dom: (x, y, dom_col_w, main_h),
            order_entry: (right_col_x, y, right_w, oe_h),
            risk_calculator: (right_col_x, y + oe_h, right_w, rc_h),
            bottom_tab_bar: (x, bottom_y, w, tab_bar_h),
            bottom_content: (x, bottom_y + tab_bar_h, w, content_h),
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
        } else {
            // Separator drags are stateless per-frame ratio nudges (see
            // `handle_drag_move`) — drag-start just needs to claim the
            // gesture so the app layer starts forwarding `handle_drag_move`.
            TradingSeparator::from_wire_id(local_id).is_some()
        }
    }

    fn handle_drag_move(&mut self, local_id: &str, dx: f64, dy: f64) -> bool {
        if local_id.starts_with("dom:") {
            return self.dom.handle_drag_move(local_id, dx, dy);
        }
        let Some(sep) = TradingSeparator::from_wire_id(local_id) else { return false };
        let (w, h) = self.last_size.get();
        if w <= 0.0 || h <= 0.0 {
            return true;
        }
        match sep {
            TradingSeparator::DomRightCol => {
                // Dragging right shrinks the right column (grows DOM).
                let right_w = (w * self.right_ratio - dx).max(0.0);
                self.right_ratio = Self::clamp_outer_ratio(w, right_w / w, TC_RIGHT_COL_MIN_PX);
            }
            TradingSeparator::OeRiskCalc => {
                let bottom_ratio = Self::clamp_outer_ratio(h, self.bottom_ratio, TC_BOTTOM_MIN_PX);
                let main_h = (h - h * bottom_ratio).max(1.0);
                self.right_split = Self::clamp_inner_split(self.right_split + dy / main_h);
            }
            TradingSeparator::BottomStrip => {
                // Dragging down shrinks the bottom strip (grows the main area).
                let bottom_h = (h * self.bottom_ratio - dy).max(0.0);
                self.bottom_ratio = Self::clamp_outer_ratio(h, bottom_h / h, TC_BOTTOM_MIN_PX);
            }
        }
        true
    }

    fn handle_drag_end(&mut self, local_id: &str) -> bool {
        if local_id.starts_with("dom:") {
            self.dom.handle_drag_end(local_id)
        } else {
            TradingSeparator::from_wire_id(local_id).is_some()
        }
    }
}

impl TradingContainerState {

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

            if is_active {
                ctx.set_fill_color(&theme.panel_bg);
                ctx.fill_rect(tab_x, y, tab_w, h);
                ctx.set_fill_color(&theme.oe_tab_active);
                ctx.fill_rect(tab_x, y + h - 2.0, tab_w, 2.0);
            }

            ctx.set_fill_color(if is_active { &theme.text_primary } else { &theme.text_muted });
            ctx.set_font("10px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(label, tab_x + tab_w / 2.0, y + h / 2.0);

            coordinator.register(
                format!("{}:{}", slot_prefix, tab.wire_id()).as_str(),
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
