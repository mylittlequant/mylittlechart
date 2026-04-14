//! Order Flow Panel Renderers
//!
//! This file retains only the renderers that cannot yet be migrated to
//! `TradingPanel` trait (TradingContainer) because they have complex
//! cross-panel dependencies. All other 9 panels now have `TradingPanel`
//! impls co-located in their state files.

use crate::render::RenderContext;

/// Convert RGBA array [0.0-1.0] to hex color string
pub fn rgba_to_hex(rgba: [f32; 4]) -> String {
    let r = (rgba[0].clamp(0.0, 1.0) * 255.0) as u8;
    let g = (rgba[1].clamp(0.0, 1.0) * 255.0) as u8;
    let b = (rgba[2].clamp(0.0, 1.0) * 255.0) as u8;
    let a = (rgba[3].clamp(0.0, 1.0) * 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", r, g, b, a)
}

/// Render the trading container (DOM + sub-panels)
pub fn render_trading_container(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &crate::trading::trading::trading_container::TradingContainerState,
    now_ms: u64,
) {
    // Background
    ctx.set_fill_color(&rgba_to_hex([0.04, 0.04, 0.06, 1.0]));
    ctx.fill_rect(x as f64, y as f64, width as f64, height as f64);

    // Calculate layout rects
    let rects = state.layout_rects(x as f64, y as f64, width as f64, height as f64);

    // Render DOM area (placeholder background)
    let (dx, dy, dw, dh) = rects.dom;
    ctx.set_fill_color(&rgba_to_hex([0.11, 0.11, 0.16, 1.0]));
    ctx.fill_rect(dx, dy, dw, dh);

    // Render left sub-panel
    if let Some((lx, ly, lw, lh)) = rects.left {
        render_sub_panel(ctx, lx, ly, lw, lh, &state.left_panel, state, now_ms);
        // Separator line
        ctx.set_fill_color(&rgba_to_hex([0.2, 0.2, 0.25, 1.0]));
        ctx.fill_rect(lx + lw - 1.0, ly, 1.0, lh);
    }

    // Render right sub-panel
    if let Some((rx, ry, rw, rh)) = rects.right {
        render_sub_panel(ctx, rx, ry, rw, rh, &state.right_panel, state, now_ms);
        // Separator line
        ctx.set_fill_color(&rgba_to_hex([0.2, 0.2, 0.25, 1.0]));
        ctx.fill_rect(rx, ry, 1.0, rh);
    }

    // Render bottom sub-panel
    if let Some((bx, by, bw, bh)) = rects.bottom {
        render_sub_panel(ctx, bx, by, bw, bh, &state.bottom_panel, state, now_ms);
        // Separator line
        ctx.set_fill_color(&rgba_to_hex([0.2, 0.2, 0.25, 1.0]));
        ctx.fill_rect(bx, by, bw, 1.0);
    }
}

fn render_sub_panel(
    ctx: &mut dyn RenderContext,
    x: f64, y: f64, w: f64, h: f64,
    slot: &crate::trading::trading::trading_container::SubPanelSlot,
    state: &crate::trading::trading::trading_container::TradingContainerState,
    _now_ms: u64,
) {
    use crate::trading::trading::trading_container::SubPanelSlot;
    use crate::panel_trait::TradingPanel;

    match slot {
        SubPanelSlot::None => {}
        SubPanelSlot::Footprint => {
            if let Some(ref fp) = state.footprint {
                fp.render(ctx, x as f32, y as f32, w as f32, h as f32);
            }
        }
        SubPanelSlot::VolumeProfile => {
            if let Some(ref vp) = state.volume_profile {
                vp.render(ctx, x as f32, y as f32, w as f32, h as f32);
            }
        }
        SubPanelSlot::BigTrades => {
            if let Some(ref bt) = state.big_trades {
                bt.render(ctx, x as f32, y as f32, w as f32, h as f32);
            }
        }
        SubPanelSlot::L2Tape => {
            if let Some(ref tape) = state.l2_tape {
                tape.render(ctx, x as f32, y as f32, w as f32, h as f32);
            }
        }
    }
}
