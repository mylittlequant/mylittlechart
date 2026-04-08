//! Render dispatch for `FreeItem` leaves inside the free-slot sidebars.
//!
//! `render_free_item` is the single entry point: it receives a reference to
//! the panel store and a `FreeItem`, looks up the matching state, and
//! delegates to the appropriate renderer from `zengeld_panels::renderers`.
//!
//! Panels whose renderers are not yet implemented (OrderEntry, PositionManager,
//! TradeLog, RiskCalculator, TradingContainer) display a placeholder box with
//! the panel kind label. Real renderers will be wired in a later phase.

use sidebar_content::free_slot::FreeItem;
use zengeld_chart::render::RenderContext;

use zengeld_panels::renderers::panel_renderers_orderflow::{
    render_dom_panel,
    render_footprint_panel,
    render_volume_profile_panel,
    render_liquidity_heatmap_panel,
    render_trading_container,
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

        // Panels without a real renderer yet — render a labelled placeholder.
        FreeItem::BigTrades(_)
        | FreeItem::L2Tape(_)
        | FreeItem::OrderEntry(_)
        | FreeItem::PositionManager(_)
        | FreeItem::TradeLog(_)
        | FreeItem::RiskCalculator(_) => {
            use uzor::panels::DockPanel;
            render_placeholder(ctx, x, y, w, h, item.title());
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn render_placeholder(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    label: &str,
) {
    ctx.set_fill_color("#0d1117ff");
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color("#888888ff");
    ctx.set_text_align(zengeld_chart::render::TextAlign::Center);
    ctx.set_text_baseline(zengeld_chart::render::TextBaseline::Middle);
    ctx.fill_text(
        label,
        (x + w / 2.0) as f64,
        (y + h / 2.0) as f64,
    );
}
