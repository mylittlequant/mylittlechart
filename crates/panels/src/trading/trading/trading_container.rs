//! Trading Container: composite panel with DOM as the base element
//! and inner sub-panels (Footprint, Volume Profile, Tick Tape, etc.)
//! arranged in a split layout.

use serde::{Serialize, Deserialize};
use crate::trading::order_flow::dom::DomState;
use crate::trading::order_flow::footprint::FootprintState;
use crate::trading::order_flow::volume_profile::VolumeProfileState;
use crate::trading::order_flow::big_trades::BigTradesState;
use crate::trading::order_flow::l2_tape::L2TapeState;

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

    /// DOM state (always present — base element)
    pub dom: DomState,

    /// Optional sub-panel states
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
            symbol,
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
            footprint: None,
            volume_profile: None,
            big_trades: None,
            l2_tape: None,
        }
    }

    /// Calculate sub-panel rects given container rect
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
}

/// Calculated layout rects for sub-panels: (x, y, w, h)
#[derive(Clone, Debug)]
pub struct TradingLayoutRects {
    pub left: Option<(f64, f64, f64, f64)>,
    pub dom: (f64, f64, f64, f64),
    pub right: Option<(f64, f64, f64, f64)>,
    pub bottom: Option<(f64, f64, f64, f64)>,
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
