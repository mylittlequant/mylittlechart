//! Render dispatch for `FreeItem` leaves inside the free-slot sidebars.
//!
//! `render_free_item` is the single entry point: it receives a reference to
//! the panel store and a `FreeItem`, looks up the matching state, and
//! delegates to the appropriate renderer from `zengeld_panels::renderers`.
//!
//! All trading panel renderers are now wired.

use sidebar_content::free_slot::FreeItem;
use uzor::render::{TextAlign, TextBaseline};
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
                render_source_badge(ctx, &state.source, x, y, w);
            }
        }

        FreeItem::Footprint(id) => {
            if let Some(state) = store.footprint.get(id) {
                let config = FootprintConfig::default();
                render_footprint_panel(ctx, x, y, w, h, state, &config);
                render_source_badge(ctx, &state.source, x, y, w);
            }
        }

        FreeItem::VolumeProfile(id) => {
            if let Some(state) = store.volume_profile.get(id) {
                let config = VolumeProfileConfig::default();
                render_volume_profile_panel(ctx, x, y, w, h, state, &config);
                render_source_badge(ctx, &state.source, x, y, w);
            }
        }

        FreeItem::LiquidityHeatmap(id) => {
            if let Some(state) = store.liquidity_heatmap.get(id) {
                let config = LiquidityHeatmapConfig::default();
                render_liquidity_heatmap_panel(ctx, x, y, w, h, state, &config);
                render_source_badge(ctx, &state.source, x, y, w);
            }
        }

        FreeItem::TradingContainer(id) => {
            if let Some(state) = store.trading_container.get(id) {
                // `now_ms` is used only for animation; 0 is safe for a static render.
                render_trading_container(ctx, x, y, w, h, state, 0);
                render_source_badge(ctx, &state.source, x, y, w);
            }
        }

        FreeItem::L2Tape(id) => {
            if let Some(state) = store.l2_tape.get(id) {
                render_l2_tape_panel(ctx, x, y, w, h, state);
                render_source_badge(ctx, &state.source, x, y, w);
            }
        }

        FreeItem::BigTrades(id) => {
            if let Some(state) = store.big_trades.get(id) {
                render_big_trades_panel(ctx, x, y, w, h, state);
                render_source_badge(ctx, &state.source, x, y, w);
            }
        }

        FreeItem::OrderEntry(id) => {
            if let Some(state) = store.order_entry.get(id) {
                render_order_entry_panel(ctx, x, y, w, h, state);
                render_source_badge(ctx, &state.source, x, y, w);
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

/// Draw a small source-mode badge in the top-right corner of a panel.
///
/// - `HyperFocus` (Auto/default) — no badge rendered, avoids visual noise.
/// - `Fixed` (Pinned) — blue pill with "P".
/// - `BoundToChart` (Linked) — green pill with "L".
///
/// The badge is a 14×14 rounded rectangle, 3 px from the top-right corner.
fn render_source_badge(
    ctx: &mut dyn RenderContext,
    source: &SymbolSource,
    x: f32,
    y: f32,
    w: f32,
) {
    let (label, bg) = match source {
        SymbolSource::HyperFocus => return,
        SymbolSource::Fixed { .. } => ("P", "#2563eb"),
        SymbolSource::BoundToChart { .. } => ("L", "#16a34a"),
    };

    const BADGE_W: f64 = 14.0;
    const BADGE_H: f64 = 14.0;
    const MARGIN: f64 = 3.0;
    const RADIUS: f64 = 3.0;

    let bx = f64::from(x) + f64::from(w) - BADGE_W - MARGIN;
    let by = f64::from(y) + MARGIN;

    // Background pill
    ctx.set_fill_color(bg);
    ctx.fill_rounded_rect(bx, by, BADGE_W, BADGE_H, RADIUS);

    // Label text — centered inside the pill
    ctx.set_fill_color("#ffffff");
    ctx.set_font("bold 9px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, bx + BADGE_W / 2.0, by + BADGE_H / 2.0);
}

