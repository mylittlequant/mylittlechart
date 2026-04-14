//! Render dispatch for `FreeItem` leaves inside the free-slot sidebars.
//!
//! `render_free_item` is the single entry point: it receives a reference to
//! the panel store and a `FreeItem`, looks up the matching state, and
//! delegates to the `TradingPanel::render()` trait method.
//!
//! All 11 trading panels are wired through `get_panel()`.

use sidebar_content::free_slot::FreeItem;
use zengeld_chart::render::RenderContext;

use zengeld_panels::trading::SymbolSource;

use crate::panels_store::TradingPanelsStore;

/// Info extracted from panel state for rendering the panel header bar.
pub struct PanelHeaderInfo {
    /// Display label for the SymbolSource mode.
    pub source_label: &'static str,
    /// Background color for the source pill.
    pub source_color: &'static str,
    /// Resolved symbol (e.g. "SOLUSDT").
    pub symbol: String,
    /// Whether this panel is a DOM (shows zoom/center controls).
    pub is_dom: bool,
}


/// Extract header info for a given FreeItem from the store.
pub fn panel_header_info(
    store: &TradingPanelsStore,
    item: &FreeItem,
) -> Option<PanelHeaderInfo> {
    fn source_label(s: &SymbolSource) -> &'static str {
        match s {
            SymbolSource::HyperFocus => "Auto",
            SymbolSource::Fixed { .. } => "Pinned",
            SymbolSource::BoundToChart { .. } => "Linked",
        }
    }
    fn source_color(s: &SymbolSource) -> &'static str {
        match s {
            SymbolSource::HyperFocus => "#374151",
            SymbolSource::Fixed { .. } => "#2563eb",
            SymbolSource::BoundToChart { .. } => "#16a34a",
        }
    }

    match item {
        FreeItem::Dom(id) => store.dom.get(id).map(|s| PanelHeaderInfo {
            source_label: source_label(&s.source),
            source_color: source_color(&s.source),
            symbol: s.symbol.clone(),
            is_dom: true,
        }),
        FreeItem::Footprint(id) => store.footprint.get(id).map(|s| PanelHeaderInfo {
            source_label: source_label(&s.source),
            source_color: source_color(&s.source),
            symbol: s.symbol.clone(),
            is_dom: false,
        }),
        FreeItem::VolumeProfile(id) => store.volume_profile.get(id).map(|s| PanelHeaderInfo {
            source_label: source_label(&s.source),
            source_color: source_color(&s.source),
            symbol: s.symbol.clone(),
            is_dom: false,
        }),
        FreeItem::LiquidityHeatmap(id) => store.liquidity_heatmap.get(id).map(|s| PanelHeaderInfo {
            source_label: source_label(&s.source),
            source_color: source_color(&s.source),
            symbol: s.symbol.clone(),
            is_dom: false,
        }),
        FreeItem::BigTrades(id) => store.big_trades.get(id).map(|s| PanelHeaderInfo {
            source_label: source_label(&s.source),
            source_color: source_color(&s.source),
            symbol: s.symbol.clone(),
            is_dom: false,
        }),
        FreeItem::L2Tape(id) => store.l2_tape.get(id).map(|s| PanelHeaderInfo {
            source_label: source_label(&s.source),
            source_color: source_color(&s.source),
            symbol: s.symbol.clone(),
            is_dom: false,
        }),
        FreeItem::OrderEntry(id) => store.order_entry.get(id).map(|s| PanelHeaderInfo {
            source_label: source_label(&s.source),
            source_color: source_color(&s.source),
            symbol: s.symbol.clone(),
            is_dom: false,
        }),
        FreeItem::TradingContainer(id) => store.trading_container.get(id).map(|s| PanelHeaderInfo {
            source_label: source_label(&s.source),
            source_color: source_color(&s.source),
            symbol: s.symbol.clone(),
            is_dom: false,
        }),
        // Panels without SymbolSource
        FreeItem::PositionManager(_) | FreeItem::TradeLog(_) | FreeItem::RiskCalculator(_) => None,
    }
}

/// Render the content of a `FreeItem` leaf into `(x, y, w, h)`.
///
/// If the item's state is missing from the store (e.g. the panel was removed
/// between frames), the slot is left blank. The function never panics.
pub fn render_free_item(
    store: &TradingPanelsStore,
    item: &FreeItem,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    ctx: &mut dyn RenderContext,
) {
    if let Some(panel) = store.get_panel(item) {
        panel.render(ctx, x, y, w, h);
    }
}
