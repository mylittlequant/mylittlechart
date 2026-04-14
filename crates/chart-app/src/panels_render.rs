//! Render dispatch for `FreeItem` leaves inside the free-slot sidebars.
//!
//! `render_free_item` is the single entry point: it receives a reference to
//! the panel store and a `FreeItem`, looks up the matching state, and
//! delegates to the appropriate renderer from `zengeld_panels::renderers`.
//!
//! All trading panel renderers are now wired.

use sidebar_content::free_slot::FreeItem;
use zengeld_chart::render::RenderContext;

use zengeld_panels::renderers::panel_renderers_orderflow::{
    render_dom_panel,
    render_footprint_panel,
    render_volume_profile_panel,
    render_liquidity_heatmap_panel,
    render_trading_container,
    render_l2_tape_panel,
    render_big_trades_panel,
    render_risk_calculator_panel,
    render_order_entry_panel,
    render_position_manager_panel,
    render_trade_log_panel,
};
use zengeld_panels::trading::order_flow::footprint::FootprintConfig;
use zengeld_panels::trading::order_flow::volume_profile::VolumeProfileConfig;
use zengeld_panels::trading::order_flow::liquidity_heatmap::LiquidityHeatmapConfig;
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

/// Extract the lightweight `PanelHeaderHint` used by `sidebar-content` render.
///
/// Returns `None` for panels without a `SymbolSource` (PositionManager, TradeLog,
/// RiskCalculator).
pub fn panel_header_hint(
    store: &TradingPanelsStore,
    item: &FreeItem,
) -> Option<sidebar_content::free_slot::PanelHeaderHint> {
    let info = panel_header_info(store, item)?;
    Some(sidebar_content::free_slot::PanelHeaderHint {
        source_label: info.source_label,
        source_color: info.source_color,
        symbol: info.symbol,
        is_dom: info.is_dom,
    })
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
    match item {
        FreeItem::Dom(id) => {
            if let Some(state) = store.dom.get(id) {
                render_dom_panel(ctx, x, y, w, h, state);
            }
        }

        FreeItem::Footprint(id) => {
            if let Some(state) = store.footprint.get(id) {
                let config = FootprintConfig::default();
                render_footprint_panel(ctx, x, y, w, h, state, &config);
            }
        }

        FreeItem::VolumeProfile(id) => {
            if let Some(state) = store.volume_profile.get(id) {
                let config = VolumeProfileConfig::default();
                render_volume_profile_panel(ctx, x, y, w, h, state, &config);
            }
        }

        FreeItem::LiquidityHeatmap(id) => {
            if let Some(state) = store.liquidity_heatmap.get(id) {
                let config = LiquidityHeatmapConfig::default();
                render_liquidity_heatmap_panel(ctx, x, y, w, h, state, &config);
            }
        }

        FreeItem::TradingContainer(id) => {
            if let Some(state) = store.trading_container.get(id) {
                // `now_ms` is used only for animation; 0 is safe for a static render.
                render_trading_container(ctx, x, y, w, h, state, 0);
            }
        }

        FreeItem::L2Tape(id) => {
            if let Some(state) = store.l2_tape.get(id) {
                render_l2_tape_panel(ctx, x, y, w, h, state);
            }
        }

        FreeItem::BigTrades(id) => {
            if let Some(state) = store.big_trades.get(id) {
                render_big_trades_panel(ctx, x, y, w, h, state);
            }
        }

        FreeItem::OrderEntry(id) => {
            if let Some(state) = store.order_entry.get(id) {
                render_order_entry_panel(ctx, x, y, w, h, state);
            }
        }

        FreeItem::PositionManager(id) => {
            if let Some(state) = store.position_manager.get(id) {
                render_position_manager_panel(ctx, x, y, w, h, state);
            }
        }

        FreeItem::TradeLog(id) => {
            if let Some(state) = store.trade_log.get(id) {
                render_trade_log_panel(ctx, x, y, w, h, state);
            }
        }

        FreeItem::RiskCalculator(id) => {
            if let Some(state) = store.risk_calculator.get(id) {
                render_risk_calculator_panel(ctx, x, y, w, h, state);
            }
        }
    }
}
