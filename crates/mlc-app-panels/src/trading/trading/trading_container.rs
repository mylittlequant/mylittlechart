//! Trading Container: the single fixed-region trading workspace hosted by
//! the Trading sidebar tab (owner spec 2026-07-25 — zero slots, ONE
//! container filling the whole tab).
//!
//! v1 is a STATIC layout (no inner docking tree yet):
//! - center = DOM ladder
//! - right column (fixed width) = OrderEntry on top, RiskCalculator below
//! - bottom strip (fixed height, spans center + right column) =
//!   PositionManager (left half), TradeLog (right half)
//!
//! The legacy `left_panel`/`right_panel`/`bottom_panel` `SubPanelSlot`
//! machinery (Footprint / VolumeProfile / BigTrades / L2Tape) predates this
//! topology and is kept for backwards compatibility, but it now only ever
//! carves space out of the DOM column — the new fixed regions always win.

use serde::{Serialize, Deserialize};
use crate::render::RenderContext;
use crate::panel_trait::TradingPanel;
use crate::trading::order_flow::dom::DomState;
use crate::trading::order_flow::footprint::FootprintState;
use crate::trading::order_flow::volume_profile::VolumeProfileState;
use crate::trading::order_flow::big_trades::BigTradesState;
use crate::trading::order_flow::l2_tape::L2TapeState;
use super::order_entry::OrderEntryState;
use super::position_manager::PositionManagerState;
use super::trade_log::TradeLogState;
use super::risk_calculator::RiskCalculatorState;

/// Fixed width of the right column (OrderEntry + RiskCalculator), in px.
const TC_RIGHT_COL_W: f64 = 260.0;
/// Fixed height of the bottom strip (PositionManager + TradeLog), in px.
const TC_BOTTOM_H: f64 = 180.0;
/// Fraction of the right column's height given to OrderEntry (rest goes
/// to RiskCalculator). No owner spec for the exact split — chosen so
/// OrderEntry's Limit/Stop-Limit field stack fits comfortably.
const TC_OE_HEIGHT_RATIO: f64 = 0.55;

/// Trading Container panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TradingContainerId(pub u64);

/// Layout arrangement of sub-panels within the trading container
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TradingLayout {
    /// Just DOM (no sub-panels)
    DomOnly,
    /// DOM right, sub-panel left (e.g. Footprint | DOM)
    LeftPanel,
    /// DOM left, sub-panel right (e.g. DOM | Volume Profile)
    RightPanel,
    /// Sub-panel left, DOM center, sub-panel right
    ThreeColumn,
    /// DOM center with tick tape below
    DomWithBottomTape,
    /// Full layout: left panel | DOM | right panel, tick tape bottom
    Full,
}

/// Which sub-panel type is in a given slot
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SubPanelSlot {
    None,
    Footprint,
    VolumeProfile,
    BigTrades,
    L2Tape,
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
    /// Tick size for price grid
    pub tick_size: f64,
    /// Market price (from exchange)
    pub market_price: f64,
    /// Center price for DOM ladder
    pub center_price: f64,

    /// Layout arrangement
    pub layout: TradingLayout,

    /// Left sub-panel type
    pub left_panel: SubPanelSlot,
    /// Right sub-panel type
    pub right_panel: SubPanelSlot,
    /// Bottom sub-panel type
    pub bottom_panel: SubPanelSlot,

    /// Split ratios (0.0..1.0) — how much space each section gets
    pub left_ratio: f64,    // left panel width ratio (default 0.3)
    pub right_ratio: f64,   // right panel width ratio (default 0.3)
    pub bottom_ratio: f64,  // bottom panel height ratio (default 0.25)

    /// DOM state (always present — base element, v1 center region)
    pub dom: DomState,

    /// Order entry — always-present hosted sub-panel (v1 right column, top)
    pub order_entry: OrderEntryState,
    /// Position manager — always-present hosted sub-panel (v1 bottom-left)
    pub position_manager: PositionManagerState,
    /// Trade log — always-present hosted sub-panel (v1 bottom-right)
    pub trade_log: TradeLogState,
    /// Risk calculator — always-present hosted sub-panel (v1 right column, bottom)
    pub risk_calculator: RiskCalculatorState,

    /// Optional legacy sub-panel states (pre-dates the fixed v1 regions;
    /// only ever carves space out of the DOM column — see module docs).
    pub footprint: Option<FootprintState>,
    pub volume_profile: Option<VolumeProfileState>,
    pub big_trades: Option<BigTradesState>,
    pub l2_tape: Option<L2TapeState>,
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
            tick_size,
            market_price,
            center_price: market_price,
            layout: TradingLayout::DomOnly,
            left_panel: SubPanelSlot::None,
            right_panel: SubPanelSlot::None,
            bottom_panel: SubPanelSlot::None,
            left_ratio: 0.3,
            right_ratio: 0.3,
            bottom_ratio: 0.25,
            dom,
            order_entry: OrderEntryState::new(symbol),
            position_manager: PositionManagerState::new(),
            trade_log: TradeLogState::new(),
            risk_calculator: RiskCalculatorState::new(),
            footprint: None,
            volume_profile: None,
            big_trades: None,
            l2_tape: None,
        }
    }

    /// Calculate legacy sub-panel rects given a rect — used ONLY for the
    /// left/right/bottom `SubPanelSlot` machinery, scoped to the DOM column
    /// (see [`Self::v1_layout_rects`]), never the full container.
    pub fn layout_rects(&self, x: f64, y: f64, w: f64, h: f64) -> TradingLayoutRects {
        let has_bottom = self.bottom_panel != SubPanelSlot::None;
        let has_left = self.left_panel != SubPanelSlot::None;
        let has_right = self.right_panel != SubPanelSlot::None;

        let bottom_h = if has_bottom { h * self.bottom_ratio } else { 0.0 };
        let main_h = h - bottom_h;

        let left_w = if has_left { w * self.left_ratio } else { 0.0 };
        let right_w = if has_right { w * self.right_ratio } else { 0.0 };
        let dom_w = w - left_w - right_w;

        TradingLayoutRects {
            left: if has_left { Some((x, y, left_w, main_h)) } else { None },
            dom: (x + left_w, y, dom_w, main_h),
            right: if has_right { Some((x + left_w + dom_w, y, right_w, main_h)) } else { None },
            bottom: if has_bottom { Some((x, y + main_h, w, bottom_h)) } else { None },
        }
    }

    /// v1 fixed-region layout for the whole container: DOM center, fixed
    /// right column (OrderEntry / RiskCalculator), fixed bottom strip
    /// (PositionManager / TradeLog). The legacy left/right/bottom
    /// `SubPanelSlot` rects (when configured) carve INTO the DOM column
    /// only — the new fixed regions always win the right column / bottom
    /// strip real estate.
    pub fn v1_layout_rects(&self, x: f64, y: f64, w: f64, h: f64) -> TradingContainerV1Rects {
        let right_w = TC_RIGHT_COL_W.min(w * 0.5);
        let bottom_h = TC_BOTTOM_H.min(h * 0.5);
        let main_h = (h - bottom_h).max(0.0);
        let dom_col_w = (w - right_w).max(0.0);

        let legacy = self.layout_rects(x, y, dom_col_w, main_h);

        let right_col_x = x + dom_col_w;
        let oe_h = main_h * TC_OE_HEIGHT_RATIO;
        let rc_h = (main_h - oe_h).max(0.0);

        let bottom_y = y + main_h;
        let pm_w = w * 0.5;
        let tl_w = (w - pm_w).max(0.0);

        TradingContainerV1Rects {
            dom: legacy.dom,
            legacy_left: legacy.left,
            legacy_right: legacy.right,
            legacy_bottom: legacy.bottom,
            order_entry: (right_col_x, y, right_w, oe_h),
            risk_calculator: (right_col_x, y + oe_h, right_w, rc_h),
            position_manager: (x, bottom_y, pm_w, bottom_h),
            trade_log: (x + pm_w, bottom_y, tl_w, bottom_h),
        }
    }
}

/// Calculated legacy sub-panel rects (x, y, w, h) — see [`TradingContainerState::layout_rects`].
#[derive(Clone, Debug)]
pub struct TradingLayoutRects {
    pub left: Option<(f64, f64, f64, f64)>,
    pub dom: (f64, f64, f64, f64),
    pub right: Option<(f64, f64, f64, f64)>,
    pub bottom: Option<(f64, f64, f64, f64)>,
}

/// Calculated v1 fixed-region rects (x, y, w, h) — see
/// [`TradingContainerState::v1_layout_rects`].
#[derive(Clone, Debug)]
pub struct TradingContainerV1Rects {
    pub dom: (f64, f64, f64, f64),
    pub legacy_left: Option<(f64, f64, f64, f64)>,
    pub legacy_right: Option<(f64, f64, f64, f64)>,
    pub legacy_bottom: Option<(f64, f64, f64, f64)>,
    pub order_entry: (f64, f64, f64, f64),
    pub risk_calculator: (f64, f64, f64, f64),
    pub position_manager: (f64, f64, f64, f64),
    pub trade_log: (f64, f64, f64, f64),
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
        // Background
        ctx.set_fill_color(&theme.tc_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        // v1 fixed-region layout: center DOM, fixed right column, fixed
        // bottom strip. Children namespace their own widget ids off
        // `slot_prefix` (unchanged) — e.g. OrderEntry emits
        // "{slot_prefix}:oe:*" — so the outer click router can address them
        // exactly as it would a standalone instance of that panel kind.
        let rects = self.v1_layout_rects(x as f64, y as f64, w as f64, h as f64);

        // --- Center: DOM ladder ---
        let (dx, dy, dw, dh) = rects.dom;
        ctx.set_fill_color(&theme.tc_inner_bg);
        ctx.fill_rect(dx, dy, dw, dh);
        self.dom.render(ctx, dx as f32, dy as f32, dw as f32, dh as f32, theme, coordinator, slot_prefix);

        // --- Legacy left/right/bottom sub-panels (only when configured —
        // scoped to the DOM column so they never collide with the new
        // fixed right column / bottom strip) ---
        if let Some((lx, ly, lw, lh)) = rects.legacy_left {
            Self::render_sub_panel(ctx, lx, ly, lw, lh, &self.left_panel, self, theme, coordinator, slot_prefix);
            ctx.set_fill_color(&theme.tc_separator);
            ctx.fill_rect(lx + lw - 1.0, ly, 1.0, lh);
        }
        if let Some((rx, ry, rw, rh)) = rects.legacy_right {
            Self::render_sub_panel(ctx, rx, ry, rw, rh, &self.right_panel, self, theme, coordinator, slot_prefix);
            ctx.set_fill_color(&theme.tc_separator);
            ctx.fill_rect(rx, ry, 1.0, rh);
        }
        if let Some((bx, by, bw, bh)) = rects.legacy_bottom {
            Self::render_sub_panel(ctx, bx, by, bw, bh, &self.bottom_panel, self, theme, coordinator, slot_prefix);
            ctx.set_fill_color(&theme.tc_separator);
            ctx.fill_rect(bx, by, bw, 1.0);
        }

        // --- Right column: OrderEntry (top), RiskCalculator (bottom) ---
        let (ox, oy, ow, oh) = rects.order_entry;
        self.order_entry.render(ctx, ox as f32, oy as f32, ow as f32, oh as f32, theme, coordinator, slot_prefix);
        ctx.set_fill_color(&theme.tc_separator);
        ctx.fill_rect(ox, oy + oh - 1.0, ow, 1.0);

        let (rcx, rcy, rcw, rch) = rects.risk_calculator;
        self.risk_calculator.render(ctx, rcx as f32, rcy as f32, rcw as f32, rch as f32, theme, coordinator, slot_prefix);

        ctx.set_fill_color(&theme.tc_separator);
        ctx.fill_rect(ox - 1.0, y as f64, 1.0, h as f64);

        // --- Bottom strip: PositionManager (left), TradeLog (right) ---
        let (pmx, pmy, pmw, pmh) = rects.position_manager;
        self.position_manager.render(ctx, pmx as f32, pmy as f32, pmw as f32, pmh as f32, theme, coordinator, slot_prefix);

        let (tlx, tly, tlw, tlh) = rects.trade_log;
        self.trade_log.render(ctx, tlx as f32, tly as f32, tlw as f32, tlh as f32, theme, coordinator, slot_prefix);

        ctx.set_fill_color(&theme.tc_separator);
        ctx.fill_rect(x as f64, pmy, w as f64, 1.0);
        ctx.fill_rect(tlx, tly, 1.0, tlh);
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
        } else if local_id.starts_with("risk_calculator:") {
            self.risk_calculator.handle_click(local_id, x, y)
        } else {
            false
        }
    }
}

impl TradingContainerState {
    fn render_sub_panel(
        ctx: &mut dyn RenderContext,
        x: f64, y: f64, w: f64, h: f64,
        slot: &SubPanelSlot,
        state: &TradingContainerState,
        theme: &crate::panel_theme::PanelTheme,
        coordinator: &mut uzor::InputCoordinator,
        slot_prefix: &str,
    ) {
        match slot {
            SubPanelSlot::None => {}
            SubPanelSlot::Footprint => {
                if let Some(ref fp) = state.footprint {
                    fp.render(ctx, x as f32, y as f32, w as f32, h as f32, theme, coordinator, slot_prefix);
                }
            }
            SubPanelSlot::VolumeProfile => {
                if let Some(ref vp) = state.volume_profile {
                    vp.render(ctx, x as f32, y as f32, w as f32, h as f32, theme, coordinator, slot_prefix);
                }
            }
            SubPanelSlot::BigTrades => {
                if let Some(ref bt) = state.big_trades {
                    bt.render(ctx, x as f32, y as f32, w as f32, h as f32, theme, coordinator, slot_prefix);
                }
            }
            SubPanelSlot::L2Tape => {
                if let Some(ref tape) = state.l2_tape {
                    tape.render(ctx, x as f32, y as f32, w as f32, h as f32, theme, coordinator, slot_prefix);
                }
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
