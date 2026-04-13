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

use crate::panels_store::TradingPanelsStore;

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

