//! Order Flow Panel Renderers
//!
//! Rendering functions for specialized order flow panels:
//! - DOM (Depth of Market): Price ladder with bid/ask volume bars
//! - Footprint: Cluster chart with bid/ask volume cells
//! - Volume Profile: Horizontal histogram with POC/VAH/VAL
//! - Liquidity Heatmap: 2D heatmap of order book depth over time
//! - Market Depth Graph: Area chart of cumulative bid/ask depth
//! - Ticker Tape: Scrolling horizontal strip of ticker data

use crate::render::{RenderContext, TextAlign, TextBaseline};
use crate::trading::order_flow::dom::DomState;
use crate::trading::order_flow::footprint::{FootprintState, FootprintConfig};
use crate::trading::order_flow::volume_profile::{VolumeProfileState, VolumeProfileConfig};
use crate::trading::order_flow::liquidity_heatmap::{LiquidityHeatmapState, LiquidityHeatmapConfig};

use crate::trading::order_flow::big_trades::{BigTradesState, TradeSide};
use crate::trading::order_flow::l2_tape::{L2TapeState, L2Side};
use crate::trading::trading::risk_calculator::RiskCalculatorState;
use crate::trading::order_entry::{
    OrderEntryState,
    OrderSide as OeSide,
    OrderType as OeOrderType,
};

/// Convert RGBA array [0.0-1.0] to hex color string
fn rgba_to_hex(rgba: [f32; 4]) -> String {
    let r = (rgba[0].clamp(0.0, 1.0) * 255.0) as u8;
    let g = (rgba[1].clamp(0.0, 1.0) * 255.0) as u8;
    let b = (rgba[2].clamp(0.0, 1.0) * 255.0) as u8;
    let a = (rgba[3].clamp(0.0, 1.0) * 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", r, g, b, a)
}

// ===========================
// DOM (Depth of Market) Panel
// ===========================

// DOM Colors
const BG_DEFAULT: [f32; 4] = [0.11, 0.11, 0.16, 1.0];            // #1c1c28ff — match theme bg
const BG_BEST_BID: [f32; 4] = [0.04, 0.21, 0.13, 1.0];          // #0a3622ff
const BG_BEST_ASK: [f32; 4] = [0.23, 0.04, 0.04, 1.0];          // #3a0a0aff
const BG_SPREAD: [f32; 4] = [0.08, 0.09, 0.12, 1.0];            // #15181fff
const BG_CURRENT_PRICE: [f32; 4] = [0.16, 0.16, 0.0, 1.0];      // #2a2a00ff
const BG_HOVER: [f32; 4] = [0.16, 0.18, 0.25, 1.0];             // #292f40ff - subtle highlight

const TEXT_PRICE_DEFAULT: [f32; 4] = [0.88, 0.88, 0.88, 1.0];   // #e0e0e0ff
const TEXT_PRICE_BEST_BID: [f32; 4] = [0.0, 1.0, 0.53, 1.0];    // #00ff88ff
const TEXT_PRICE_BEST_ASK: [f32; 4] = [1.0, 0.27, 0.4, 1.0];    // #ff4466ff
const TEXT_PRICE_CURRENT: [f32; 4] = [1.0, 0.87, 0.0, 1.0];     // #ffdd00ff

const TEXT_VOL_BID: [f32; 4] = [0.4, 0.8, 0.53, 1.0];            // #66cc88ff
const TEXT_VOL_ASK: [f32; 4] = [1.0, 0.4, 0.47, 1.0];            // #ff6677ff

const BAR_BID: [f32; 4] = [0.0, 0.67, 0.33, 1.0];                // #00aa55ff
const BAR_BID_BRIGHT: [f32; 4] = [0.0, 1.0, 0.53, 1.0];          // #00ff88ff
const BAR_ASK: [f32; 4] = [0.8, 0.0, 0.2, 1.0];                  // #cc0033ff
const BAR_ASK_BRIGHT: [f32; 4] = [1.0, 0.27, 0.4, 1.0];          // #ff4466ff

const USER_ORDER_MARKER: [f32; 4] = [1.0, 1.0, 0.0, 1.0];        // #ffff00ff (yellow)

// DOM Layout
const DOM_ROW_HEIGHT: f32 = 20.0;
const DOM_LEFT_PAD: f32 = 6.0;
const DOM_PRICE_COL_WIDTH: f32 = 70.0;

/// Render DOM (Depth of Market) panel - price ladder with bid/ask volume bars
pub fn render_dom_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &DomState,
) {
    // Background fill — use standard theme bg color for DOM body area
    ctx.set_fill_color(&rgba_to_hex(BG_DEFAULT));
    ctx.fill_rect(x as f64, y as f64, width as f64, height as f64);

    // === STEP 1: Calculate layout ===
    let levels = state.visible_levels_for_height(height);
    let row_height = DOM_ROW_HEIGHT;

    // Column layout: [Bid Volume Bar | Price | Ask Volume Bar]
    // Sizes are dynamic based on available width.
    let pad = 4.0_f32;
    let price_col_w = DOM_PRICE_COL_WIDTH;
    let avail = (width - price_col_w - pad * 2.0 - DOM_LEFT_PAD * 2.0).max(0.0);
    let vol_col_w = avail / 2.0;

    let bid_vol_col_x = x + DOM_LEFT_PAD;
    let bid_vol_col_w = vol_col_w;

    let price_col_x = bid_vol_col_x + bid_vol_col_w + pad;

    let ask_vol_col_x = price_col_x + price_col_w + pad;

    // === STEP 3: Find best bid and best ask for highlighting ===
    let best_bid_price = levels.iter()
        .filter(|level| level.is_bid)
        .map(|level| level.price)
        .max_by(|a: &f64, b: &f64| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let best_ask_price = levels.iter()
        .filter(|level| level.is_ask)
        .map(|level| level.price)
        .min_by(|a: &f64, b: &f64| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // === STEP 4: Render each price level row ===
    for (i, level) in levels.iter().enumerate() {
        let row_y = y + (i as f32 * row_height);

        // --- Step 4.1: Row background ---
        let is_best_bid = best_bid_price.map_or(false, |p| (level.price - p).abs() < 0.001);
        let is_best_ask = best_ask_price.map_or(false, |p| (level.price - p).abs() < 0.001);
        let is_current_price = (level.price - state.market_price).abs() < state.tick_size * 0.5;

        let bg_color = if is_current_price {
            BG_CURRENT_PRICE
        } else if is_best_bid {
            BG_BEST_BID
        } else if is_best_ask {
            BG_BEST_ASK
        } else if level.is_spread {
            BG_SPREAD
        } else {
            BG_DEFAULT
        };

        ctx.set_fill_color(&rgba_to_hex(bg_color));
        ctx.fill_rect(x as f64, row_y as f64, width as f64, row_height as f64);

        // --- Step 4.1b: Hover highlight overlay ---
        let is_hovered = state.hovered_price
            .map_or(false, |hp| (level.price - hp).abs() < state.tick_size * 0.5);

        if is_hovered {
            ctx.set_fill_color(&rgba_to_hex(BG_HOVER));
            ctx.fill_rect(x as f64, row_y as f64, width as f64, row_height as f64);

            // Left accent bar (thin, bright)
            let accent_color = if level.is_bid {
                BAR_BID_BRIGHT
            } else if level.is_ask {
                BAR_ASK_BRIGHT
            } else {
                [0.4, 0.5, 0.8, 1.0] // neutral blue for spread levels
            };
            ctx.set_fill_color(&rgba_to_hex(accent_color));
            ctx.fill_rect(x as f64, row_y as f64, 3.0, row_height as f64);
        }

        // --- Step 4.2: Bid volume bar (right-aligned, grows leftward) ---
        if level.bid_volume > 0.0 {
            let bar_width = state.bid_bar_width(level.bid_volume, vol_col_w);
            let bar_x = bid_vol_col_x + bid_vol_col_w - bar_width;
            let bar_y = row_y + 2.0;
            let bar_h = row_height - 4.0;

            // Gradient color based on volume intensity
            let intensity = (level.bid_volume / state.max_volume).clamp(0.0, 1.0) as f32;
            let bar_color = [
                BAR_BID[0] * (1.0 - intensity) + BAR_BID_BRIGHT[0] * intensity,
                BAR_BID[1] * (1.0 - intensity) + BAR_BID_BRIGHT[1] * intensity,
                BAR_BID[2] * (1.0 - intensity) + BAR_BID_BRIGHT[2] * intensity,
                1.0,
            ];

            ctx.set_fill_color(&rgba_to_hex(bar_color));
            ctx.fill_rect(bar_x as f64, bar_y as f64, bar_width as f64, bar_h as f64);

            // Bid volume text (right-aligned)
            ctx.set_fill_color(&rgba_to_hex(TEXT_VOL_BID));
            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Right);
            ctx.set_text_baseline(TextBaseline::Middle);

            let text_x = bid_vol_col_x + bid_vol_col_w - 4.0;
            let text_y = row_y + row_height / 2.0;
            let vol_text = format!("{:.0}", level.bid_volume);
            ctx.fill_text(&vol_text, text_x as f64, text_y as f64);
        }

        // --- Step 4.3: Ask volume bar (left-aligned, grows rightward) ---
        if level.ask_volume > 0.0 {
            let bar_width = state.ask_bar_width(level.ask_volume, vol_col_w);
            let bar_x = ask_vol_col_x;
            let bar_y = row_y + 2.0;
            let bar_h = row_height - 4.0;

            // Gradient color based on volume intensity
            let intensity = (level.ask_volume / state.max_volume).clamp(0.0, 1.0) as f32;
            let bar_color = [
                BAR_ASK[0] * (1.0 - intensity) + BAR_ASK_BRIGHT[0] * intensity,
                BAR_ASK[1] * (1.0 - intensity) + BAR_ASK_BRIGHT[1] * intensity,
                BAR_ASK[2] * (1.0 - intensity) + BAR_ASK_BRIGHT[2] * intensity,
                1.0,
            ];

            ctx.set_fill_color(&rgba_to_hex(bar_color));
            ctx.fill_rect(bar_x as f64, bar_y as f64, bar_width as f64, bar_h as f64);

            // Ask volume text (left-aligned)
            ctx.set_fill_color(&rgba_to_hex(TEXT_VOL_ASK));
            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);

            let text_x = ask_vol_col_x + 4.0;
            let text_y = row_y + row_height / 2.0;
            let vol_text = format!("{:.0}", level.ask_volume);
            ctx.fill_text(&vol_text, text_x as f64, text_y as f64);
        }

        // --- Step 4.4: Price text (centered in price column) ---
        let price_text_color = if is_current_price {
            TEXT_PRICE_CURRENT
        } else if is_best_bid {
            TEXT_PRICE_BEST_BID
        } else if is_best_ask {
            TEXT_PRICE_BEST_ASK
        } else {
            TEXT_PRICE_DEFAULT
        };

        ctx.set_fill_color(&rgba_to_hex(price_text_color));
        ctx.set_font("11px monospace");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);

        let price_x = price_col_x + price_col_w / 2.0;
        let price_y = row_y + row_height / 2.0;
        let price_text = format!("{:.2}", level.price);
        ctx.fill_text(&price_text, price_x as f64, price_y as f64);

        // --- Step 4.5: User order markers ---
        if level.has_user_order {
            ctx.set_fill_color(&rgba_to_hex(USER_ORDER_MARKER));
            ctx.set_font("10px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);

            // Order marker on the left edge of the bid volume column
            ctx.fill_text("▲", (bid_vol_col_x) as f64, price_y as f64);
        }
    }

    // === STEP 5: Render spread separator (horizontal line) ===
    if let (Some(best_bid), Some(best_ask)) = (best_bid_price, best_ask_price) {
        if let (Some(bid_idx), Some(_ask_idx)) = (
            levels.iter().position(|l| (l.price - best_bid).abs() < 0.001),
            levels.iter().position(|l| (l.price - best_ask).abs() < 0.001),
        ) {
            let spread_y = y + (bid_idx as f32 + 0.5) * row_height;
            ctx.set_fill_color(&rgba_to_hex([0.4, 0.4, 0.5, 0.5]));
            ctx.fill_rect(x as f64, spread_y as f64, width as f64, 1.0);
        }
    }

    // === STEP 6: Flash animation for recent fills ===
    let now = std::time::Instant::now();
    for (price_tick, (_volume, timestamp)) in &state.recent_fills {
        let elapsed_ms = now.duration_since(*timestamp).as_millis() as u64;
        if elapsed_ms < 300 {
            let price = state.tick_to_price(*price_tick);
            if let Some(row_idx) = levels.iter().position(|l| (l.price - price).abs() < 0.001) {
                let flash_y = y + (row_idx as f32 * row_height);

                // Flash phase (0-100ms): bright, Fade phase (100-300ms): linear fade
                let alpha = if elapsed_ms < 100 {
                    0.4
                } else {
                    0.4 * (1.0 - (elapsed_ms - 100) as f32 / 200.0)
                };

                // Use side-aware color: green for bid fills, red for ask fills
                let level = &levels[row_idx];
                let flash_color = if level.is_bid {
                    [0.0, 1.0, 0.4, alpha]  // green for buys
                } else {
                    [1.0, 0.2, 0.3, alpha]  // red for sells
                };
                ctx.set_fill_color(&rgba_to_hex(flash_color));
                ctx.fill_rect(x as f64, flash_y as f64, width as f64, row_height as f64);
            }
        }
    }

}

// ==============================
// Footprint / Cluster Chart Panel
// ==============================

// Footprint Colors
const CELL_TEXT_DEFAULT: [f32; 4] = [0.88, 0.88, 0.88, 1.0];
const POC_MARKER: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const POC_BORDER: [f32; 4] = [1.0, 0.87, 0.0, 1.0];
const CANDLE_BULLISH: [f32; 4] = [0.0, 0.67, 0.33, 1.0];

// Footprint Layout
const CELL_MIN_HEIGHT: f32 = 8.0;
const CELL_MAX_HEIGHT: f32 = 30.0;

/// Render Footprint (Cluster Chart) panel
pub fn render_footprint_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &FootprintState,
    config: &FootprintConfig,
) {
    // === STEP 1: Background ===
    ctx.set_fill_color(&rgba_to_hex([0.05, 0.05, 0.09, 1.0]));
    ctx.fill_rect(x as f64, y as f64, width as f64, height as f64);

    // === STEP 2: Calculate visible candles ===
    let num_visible = (width / config.candle_width).floor() as usize;
    let start_idx = (state.scroll_x / config.candle_width) as usize;
    let end_idx = (start_idx + num_visible).min(state.footprints.len());

    let candles = state.visible_candles(start_idx, end_idx);

    // === STEP 3: Render each candle ===
    for (candle_idx, candle) in candles.iter().enumerate() {
        let candle_x = x + (candle_idx as f32 * config.candle_width);
        let candle_w = config.candle_width;

        // --- Step 3.1: Candle left border ---
        let candle_color = CANDLE_BULLISH; // Could add bullish/bearish detection
        ctx.set_fill_color(&rgba_to_hex(candle_color));
        ctx.fill_rect(candle_x as f64, y as f64, 2.0, height as f64);

        // --- Step 3.2: Calculate price levels in this candle ---
        let mut price_levels: Vec<(i64, f64, f64)> = candle.price_levels.clone();
        price_levels.sort_by_key(|&(tick, _, _)| std::cmp::Reverse(tick)); // High to low

        let num_levels = price_levels.len();
        if num_levels == 0 {
            continue;
        }

        let cell_height = (height / num_levels as f32).clamp(CELL_MIN_HEIGHT, CELL_MAX_HEIGHT);

        // --- Step 3.3: Render each price cell ---
        for (level_idx, &(price_tick, bid_vol, ask_vol)) in price_levels.iter().enumerate() {
            let cell_y = y + (level_idx as f32 * cell_height);
            let cell_w = candle_w - 4.0; // Subtract border width
            let cell_h = cell_height;

            // --- Cell background (imbalance coloring) ---
            let cell_bg = state.cell_color(bid_vol, ask_vol);
            ctx.set_fill_color(&rgba_to_hex(cell_bg));
            ctx.fill_rect((candle_x + 2.0) as f64, cell_y as f64, cell_w as f64, cell_h as f64);

            // --- Cell border ---
            ctx.set_fill_color(&rgba_to_hex([0.2, 0.2, 0.3, 0.5]));
            ctx.fill_rect((candle_x + 2.0) as f64, cell_y as f64, cell_w as f64, 0.5);

            // --- Cell text (bid × ask format) ---
            let cell_text = state.format_cell(bid_vol, ask_vol);

            ctx.set_font("9px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.set_fill_color(&rgba_to_hex(CELL_TEXT_DEFAULT));

            let text_x = candle_x + candle_w / 2.0;
            let text_y = cell_y + cell_h / 2.0;
            ctx.fill_text(&cell_text, text_x as f64, text_y as f64);

            // --- POC marker (if this is POC level) ---
            let price = price_tick as f64 * state.tick_size;
            if (price - candle.poc).abs() < state.tick_size * 0.5 {
                // Draw white marker on right edge
                ctx.set_fill_color(&rgba_to_hex(POC_MARKER));
                let marker_x = candle_x + candle_w - 6.0;
                let marker_y = cell_y + cell_h / 2.0 - 3.0;
                ctx.fill_rect(marker_x as f64, marker_y as f64, 6.0, 6.0);

                // Draw gold border around cell
                ctx.set_fill_color(&rgba_to_hex(POC_BORDER));
                ctx.fill_rect((candle_x + 2.0) as f64, cell_y as f64, cell_w as f64, 1.0); // Top
                ctx.fill_rect((candle_x + 2.0) as f64, (cell_y + cell_h - 1.0) as f64, cell_w as f64, 1.0); // Bottom
                ctx.fill_rect((candle_x + 2.0) as f64, cell_y as f64, 1.0, cell_h as f64); // Left
                ctx.fill_rect((candle_x + candle_w - 3.0) as f64, cell_y as f64, 1.0, cell_h as f64); // Right
            }
        }
    }
}

// =======================
// Volume Profile Panel
// =======================

// Volume Profile Colors
const PROFILE_BAR: [f32; 4] = [0.4, 0.6, 0.8, 0.7];
const PROFILE_BAR_POC: [f32; 4] = [0.53, 0.73, 1.0, 1.0];
const POC_LINE: [f32; 4] = [1.0, 0.87, 0.0, 1.0];
const VAH_LINE: [f32; 4] = [0.53, 0.67, 1.0, 1.0];
const VAL_LINE: [f32; 4] = [0.53, 0.67, 1.0, 1.0];
const VALUE_AREA_SHADE: [f32; 4] = [0.16, 0.23, 0.29, 0.2];


// Volume Profile Layout
const VOLUME_PROFILE_BAR_HEIGHT: f32 = 4.0;

/// Render Volume Profile panel
pub fn render_volume_profile_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &VolumeProfileState,
    config: &VolumeProfileConfig,
) {
    // === STEP 1: Background ===
    ctx.set_fill_color(&rgba_to_hex([0.05, 0.05, 0.09, 1.0]));
    ctx.fill_rect(x as f64, y as f64, width as f64, height as f64);

    // === STEP 2: Calculate Y positions for price levels ===
    let levels = state.visible_levels();
    if levels.is_empty() {
        return;
    }

    let num_levels = levels.len();
    let bar_height = (height / num_levels as f32).min(VOLUME_PROFILE_BAR_HEIGHT * 2.0).max(VOLUME_PROFILE_BAR_HEIGHT / 2.0);

    // === STEP 3: Render value area shading (between VAH and VAL) ===
    let vah_y = levels.iter()
        .position(|l| (l.price - state.vah).abs() < state.tick_size * 0.5)
        .map(|idx| y + idx as f32 * bar_height);
    let val_y = levels.iter()
        .position(|l| (l.price - state.val).abs() < state.tick_size * 0.5)
        .map(|idx| y + idx as f32 * bar_height);

    if let (Some(vah), Some(val)) = (vah_y, val_y) {
        let shade_h = (val - vah).abs() as f64;
        ctx.set_fill_color(&rgba_to_hex(VALUE_AREA_SHADE));
        ctx.fill_rect(x as f64, vah as f64, width as f64, shade_h);
    }

    // === STEP 4: Render horizontal bars for each price level ===
    for (i, level) in levels.iter().enumerate() {
        let bar_y = y + (i as f32 * bar_height);

        // Calculate bar width based on volume
        let max_bar_pixels = width * config.max_bar_width;
        let bar_w = state.bar_width(level.total_volume, max_bar_pixels);

        // Check if we have actual buy/sell split data
        let has_split = (level.buy_volume - level.sell_volume).abs() > 0.001;

        if has_split {
            // Stacked buy/sell bars
            let bid_w = state.bar_width(level.buy_volume, max_bar_pixels);
            let ask_w = state.bar_width(level.sell_volume, max_bar_pixels);

            // Buy bar (green, left portion)
            let buy_color = if level.is_poc {
                [0.055, 0.796, 0.506, 0.9]  // bright green for POC
            } else {
                [0.055, 0.796, 0.506, 0.5]  // green
            };
            ctx.set_fill_color(&rgba_to_hex(buy_color));
            ctx.fill_rect(x as f64, bar_y as f64, bid_w as f64, bar_height as f64);

            // Sell bar (red, right portion stacked after buy)
            let sell_color = if level.is_poc {
                [0.965, 0.275, 0.365, 0.9]  // bright red for POC
            } else {
                [0.965, 0.275, 0.365, 0.5]  // red
            };
            ctx.set_fill_color(&rgba_to_hex(sell_color));
            ctx.fill_rect((x + bid_w) as f64, bar_y as f64, ask_w as f64, bar_height as f64);
        } else {
            // Total volume bar (when no buy/sell split available)
            let bar_color = if level.is_poc {
                PROFILE_BAR_POC
            } else {
                PROFILE_BAR
            };
            ctx.set_fill_color(&rgba_to_hex(bar_color));
            ctx.fill_rect(x as f64, bar_y as f64, bar_w as f64, bar_height as f64);
        }

        // --- POC line (horizontal gold line extending beyond bar) ---
        if level.is_poc {
            ctx.set_fill_color(&rgba_to_hex(POC_LINE));
            let line_y = bar_y + bar_height / 2.0 - 1.0;
            ctx.fill_rect(x as f64, line_y as f64, (width * 0.7) as f64, 2.0);

            // POC label
            if config.show_labels {
                ctx.set_font("10px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.set_fill_color(&rgba_to_hex(POC_LINE));
                ctx.fill_text("POC", (x + width * 0.72) as f64, (bar_y + bar_height / 2.0) as f64);
            }
        }
    }

    // === STEP 4.5: DOM center price indicator (gold line) ===
    if let Some(dom_center) = state.dom_center_price {
        if let Some(idx) = levels.iter().position(|l| (l.price - dom_center).abs() < state.tick_size * 0.5) {
            let center_y = y + idx as f32 * bar_height + bar_height / 2.0;
            ctx.set_fill_color(&rgba_to_hex([1.0, 0.843, 0.0, 0.8]));
            ctx.fill_rect(x as f64, center_y as f64, (width * 0.8) as f64, 2.0);

            // Label
            ctx.set_font("10px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.set_fill_color(&rgba_to_hex([1.0, 0.843, 0.0, 1.0]));
            ctx.fill_text("MKT", (x + width * 0.82) as f64, center_y as f64);
        }
    }

    // === STEP 5: Render VAH and VAL lines ===
    if let Some(vah) = vah_y {
        ctx.set_fill_color(&rgba_to_hex(VAH_LINE));
        ctx.fill_rect(x as f64, vah as f64, (width * 0.6) as f64, 1.0);

        if config.show_labels {
            ctx.set_font("10px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.set_fill_color(&rgba_to_hex(VAH_LINE));
            ctx.fill_text("VAH", (x + width * 0.62) as f64, vah as f64);
        }
    }

    if let Some(val) = val_y {
        ctx.set_fill_color(&rgba_to_hex(VAL_LINE));
        ctx.fill_rect(x as f64, val as f64, (width * 0.6) as f64, 1.0);

        if config.show_labels {
            ctx.set_font("10px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.set_fill_color(&rgba_to_hex(VAL_LINE));
            ctx.fill_text("VAL", (x + width * 0.62) as f64, val as f64);
        }
    }
}

// ==========================
// Liquidity Heatmap Panel
// ==========================

// Heatmap Colors
const CURRENT_PRICE_LINE: [f32; 4] = [1.0, 0.87, 0.0, 1.0];

/// Render Liquidity Heatmap panel
pub fn render_liquidity_heatmap_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &LiquidityHeatmapState,
    config: &LiquidityHeatmapConfig,
) {
    // === STEP 1: Background ===
    ctx.set_fill_color(&rgba_to_hex([0.0, 0.0, 0.0, 1.0])); // Black for heatmap
    ctx.fill_rect(x as f64, y as f64, width as f64, height as f64);

    // === STEP 2: Get visible cells ===
    let cells = state.visible_cells(width, height);

    // === STEP 3: Render each heatmap cell ===
    for (time_idx, price_tick, color) in cells {
        // Convert to screen coordinates
        let cell_x = x + state.time_to_x(time_idx, width);
        let cell_y = y + state.price_to_y(price_tick, height);

        ctx.set_fill_color(&rgba_to_hex(color));
        ctx.fill_rect(
            cell_x as f64,
            cell_y as f64,
            config.cell_width as f64,
            config.cell_height as f64,
        );
    }

    // === STEP 4: Render current price line (horizontal gold line) ===
    if config.show_current_book {
        if let Some(snapshot) = state.snapshots.last() {
            if let Some(&current_tick) = snapshot.depth_by_price.keys().next() {
                let current_y = state.price_to_y(current_tick, height);

                ctx.set_fill_color(&rgba_to_hex(CURRENT_PRICE_LINE));
                ctx.fill_rect(x as f64, current_y as f64, width as f64, 2.0);

                // Shadow for contrast
                ctx.set_fill_color(&rgba_to_hex([0.0, 0.0, 0.0, 0.5]));
                ctx.fill_rect(x as f64, (current_y + 2.0) as f64, width as f64, 1.0);
            }
        }
    }

    // === STEP 5: Price axis labels (right side) ===
    ctx.set_font("9px sans-serif");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex([0.7, 0.7, 0.7, 1.0]));

    // Sample 10 price labels evenly spaced
    let num_labels = 10;
    for i in 0..num_labels {
        let label_y = y + (i as f32 / num_labels as f32) * height;
        // Placeholder price calculation (would need min/max from state)
        let label_text = format!("{:.2}", 50000.0 + i as f64 * 10.0);
        ctx.fill_text(&label_text, (x + width - 4.0) as f64, label_y as f64);
    }
}


// ==========================
// Big Trades Panel
// ==========================

// Big Trades Colors
const BT_BG: [f32; 4] = [0.051, 0.067, 0.090, 1.0];           // #0d1117ff
const BT_HEADER_BG: [f32; 4] = [0.071, 0.086, 0.110, 1.0];    // #121620ff
const BT_HEADER_TEXT: [f32; 4] = [0.5, 0.55, 0.65, 1.0];      // grey
const BT_TEXT_DEFAULT: [f32; 4] = [0.88, 0.88, 0.88, 1.0];    // #e0e0e0ff
const BT_BUY_TEXT: [f32; 4] = [0.2, 0.85, 0.4, 1.0];          // green
const BT_SELL_TEXT: [f32; 4] = [0.95, 0.27, 0.36, 1.0];       // red
const BT_BAR_BUY: [f32; 4] = [0.0, 0.67, 0.33, 0.18];         // faded green
const BT_BAR_SELL: [f32; 4] = [0.8, 0.1, 0.15, 0.18];         // faded red
const BT_SYMBOL_TEXT: [f32; 4] = [0.4, 0.45, 0.55, 1.0];      // dim grey

// Big Trades Layout
const BT_HEADER_HEIGHT: f32 = 18.0;
const BT_ROW_HEIGHT: f32 = 20.0;
const BT_LEFT_PAD: f32 = 6.0;

/// Render Big Trades panel — scrolling list of large trades above the size threshold
pub fn render_big_trades_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &BigTradesState,
) {
    // === STEP 1: Background ===
    ctx.set_fill_color(&rgba_to_hex(BT_BG));
    ctx.fill_rect(x as f64, y as f64, width as f64, height as f64);

    // === STEP 2: Column layout ===
    // TIME | SIDE | PRICE | SIZE | NOTIONAL
    let time_col_x = x + BT_LEFT_PAD;
    let time_col_w = 60.0_f32;

    let side_col_x = time_col_x + time_col_w + 4.0;
    let side_col_w = 36.0_f32;

    let price_col_x = side_col_x + side_col_w + 4.0;
    let price_col_w = 80.0_f32;

    let size_col_x = price_col_x + price_col_w + 4.0;
    let size_col_w = 70.0_f32;

    let notional_col_x = size_col_x + size_col_w + 4.0;

    // === STEP 3: Header row ===
    ctx.set_fill_color(&rgba_to_hex(BT_HEADER_BG));
    ctx.fill_rect(x as f64, y as f64, width as f64, BT_HEADER_HEIGHT as f64);

    ctx.set_fill_color(&rgba_to_hex(BT_HEADER_TEXT));
    ctx.set_font("10px monospace");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);

    let header_y = (y + BT_HEADER_HEIGHT / 2.0) as f64;
    ctx.fill_text("TIME", time_col_x as f64, header_y);
    ctx.fill_text("SIDE", side_col_x as f64, header_y);
    ctx.fill_text("PRICE", price_col_x as f64, header_y);
    ctx.fill_text("SIZE", size_col_x as f64, header_y);
    ctx.fill_text("NOTIONAL", notional_col_x as f64, header_y);

    // === STEP 4: Trade rows ===
    let available_height = height - BT_HEADER_HEIGHT;
    let max_rows = (available_height / BT_ROW_HEIGHT) as usize;
    let trades = state.visible_trades(max_rows);

    // Max size for bar proportions — use the largest trade in the visible set
    let bar_max_width = width - BT_LEFT_PAD * 2.0;

    for (i, trade) in trades.iter().enumerate() {
        let row_y = y + BT_HEADER_HEIGHT + (i as f32 * BT_ROW_HEIGHT);

        // --- Step 4.1: Background size bar ---
        let bar_width = state.size_bar_width(trade, bar_max_width);
        let bar_color = match trade.side {
            TradeSide::Buy => BT_BAR_BUY,
            TradeSide::Sell => BT_BAR_SELL,
        };
        ctx.set_fill_color(&rgba_to_hex(bar_color));
        ctx.fill_rect(x as f64, row_y as f64, bar_width as f64, BT_ROW_HEIGHT as f64);

        let text_y = (row_y + BT_ROW_HEIGHT / 2.0) as f64;

        // --- Step 4.2: TIME column ---
        let time_str = {
            let secs = (trade.timestamp / 1000) % 86400;
            let h = secs / 3600;
            let m = (secs % 3600) / 60;
            let s = secs % 60;
            format!("{:02}:{:02}:{:02}", h, m, s)
        };

        ctx.set_fill_color(&rgba_to_hex(BT_TEXT_DEFAULT));
        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&time_str, time_col_x as f64, text_y);

        // --- Step 4.3: SIDE column ---
        let (side_str, side_color) = match trade.side {
            TradeSide::Buy => ("BUY", BT_BUY_TEXT),
            TradeSide::Sell => ("SELL", BT_SELL_TEXT),
        };
        ctx.set_fill_color(&rgba_to_hex(side_color));
        ctx.fill_text(side_str, side_col_x as f64, text_y);

        // --- Step 4.4: PRICE column ---
        let price_str = format!("{:.4}", trade.price);
        ctx.set_fill_color(&rgba_to_hex(BT_TEXT_DEFAULT));
        ctx.fill_text(&price_str, price_col_x as f64, text_y);

        // --- Step 4.5: SIZE column ---
        let size_str = format!("{:.4}", trade.quantity);
        ctx.fill_text(&size_str, size_col_x as f64, text_y);

        // --- Step 4.6: NOTIONAL column ---
        let notional = trade.price * trade.quantity;
        let notional_str = format!("{:.2}", notional);
        ctx.fill_text(&notional_str, notional_col_x as f64, text_y);

        // --- Step 4.7: Row separator ---
        ctx.set_fill_color(&rgba_to_hex([0.15, 0.17, 0.22, 0.6]));
        ctx.fill_rect(x as f64, (row_y + BT_ROW_HEIGHT - 1.0) as f64, width as f64, 1.0);
    }

    // === STEP 5: Symbol label (top-right corner) ===
    if !state.symbol.is_empty() {
        ctx.set_fill_color(&rgba_to_hex(BT_SYMBOL_TEXT));
        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Right);
        ctx.set_text_baseline(TextBaseline::Middle);
        let sym_x = (x + width - BT_LEFT_PAD) as f64;
        let sym_y = (y + BT_HEADER_HEIGHT / 2.0) as f64;
        ctx.fill_text(&state.symbol, sym_x, sym_y);
    }
}

// =======================
// L2 Tape Panel
// =======================

// L2 Tape Colors
const L2_BG: [f32; 4] = [0.051, 0.067, 0.090, 1.0];
const L2_BG_ALT: [f32; 4] = [0.063, 0.082, 0.106, 1.0];
const L2_HEADER_BG: [f32; 4] = [0.075, 0.094, 0.118, 1.0];
const L2_HEADER_TEXT: [f32; 4] = [0.5, 0.52, 0.57, 1.0];
const L2_TEXT_WHITE: [f32; 4] = [0.88, 0.88, 0.90, 1.0];
const L2_SIDE_BID: [f32; 4] = [0.055, 0.796, 0.506, 1.0];
const L2_SIDE_ASK: [f32; 4] = [0.965, 0.275, 0.365, 1.0];
const L2_SYMBOL_TEXT: [f32; 4] = [0.4, 0.42, 0.47, 1.0];

// L2 Tape Layout
const L2_ROW_HEIGHT: f32 = 16.0;
const L2_HEADER_HEIGHT: f32 = 16.0;
const L2_LEFT_PAD: f32 = 6.0;

/// Render L2 Tape panel — scrolling table of order book events (Add/Modify/Cancel/Execute)
pub fn render_l2_tape_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &L2TapeState,
) {
    // === STEP 1: Background ===
    ctx.set_fill_color(&rgba_to_hex(L2_BG));
    ctx.fill_rect(x as f64, y as f64, width as f64, height as f64);

    // === STEP 2: Column layout ===
    // TIME | TYPE | SIDE | PRICE | QTY
    let time_w  = (width * 0.28).max(70.0);
    let type_w  = (width * 0.12).max(30.0);
    let side_w  = (width * 0.12).max(28.0);
    let price_w = (width * 0.24).max(60.0);

    let col_time_x  = x + L2_LEFT_PAD;
    let col_type_x  = col_time_x  + time_w;
    let col_side_x  = col_type_x  + type_w;
    let col_price_x = col_side_x  + side_w;
    let col_qty_x   = col_price_x + price_w;

    // === STEP 3: Header row ===
    ctx.set_fill_color(&rgba_to_hex(L2_HEADER_BG));
    ctx.fill_rect(x as f64, y as f64, width as f64, L2_HEADER_HEIGHT as f64);

    ctx.set_font("10px monospace");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(L2_HEADER_TEXT));

    let header_text_y = (y + L2_HEADER_HEIGHT / 2.0) as f64;
    ctx.fill_text("TIME",  col_time_x  as f64, header_text_y);
    ctx.fill_text("TYPE",  col_type_x  as f64, header_text_y);
    ctx.fill_text("SIDE",  col_side_x  as f64, header_text_y);
    ctx.fill_text("PRICE", col_price_x as f64, header_text_y);
    ctx.fill_text("QTY",   col_qty_x   as f64, header_text_y);

    // === STEP 4: Symbol label (top-right corner of header) ===
    if !state.symbol.is_empty() {
        ctx.set_font("9px sans-serif");
        ctx.set_text_align(TextAlign::Right);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&rgba_to_hex(L2_SYMBOL_TEXT));
        ctx.fill_text(&state.symbol, (x + width - 6.0) as f64, header_text_y);
    }

    // === STEP 5: Event rows ===
    let content_h = height - L2_HEADER_HEIGHT;
    let max_rows = (content_h / L2_ROW_HEIGHT).floor() as usize;
    if max_rows == 0 {
        return;
    }

    let events = state.visible_events(max_rows);

    for (row_idx, event) in events.iter().enumerate() {
        let row_y = y + L2_HEADER_HEIGHT + (row_idx as f32 * L2_ROW_HEIGHT);
        let row_mid_y = (row_y + L2_ROW_HEIGHT / 2.0) as f64;

        // --- Step 5.1: Row background (alternating) ---
        let row_bg = if row_idx % 2 == 0 { L2_BG } else { L2_BG_ALT };
        ctx.set_fill_color(&rgba_to_hex(row_bg));
        ctx.fill_rect(x as f64, row_y as f64, width as f64, L2_ROW_HEIGHT as f64);

        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);

        // --- Step 5.2: TIME column ---
        let total_secs = (event.timestamp / 1000) % 86400;
        let hours  = total_secs / 3600;
        let mins   = (total_secs % 3600) / 60;
        let secs   = total_secs % 60;
        let millis = event.timestamp % 1000;
        let time_str = format!("{:02}:{:02}:{:02}.{:03}", hours, mins, secs, millis);

        ctx.set_fill_color(&rgba_to_hex(L2_TEXT_WHITE));
        ctx.fill_text(&time_str, col_time_x as f64, row_mid_y);

        // --- Step 5.3: TYPE column (colored by event type + side) ---
        let type_color = state.event_color(event);
        ctx.set_fill_color(&rgba_to_hex(type_color));
        ctx.fill_text(L2TapeState::event_label(&event.event_type), col_type_x as f64, row_mid_y);

        // --- Step 5.4: SIDE column (green/red) ---
        let side_color = match event.side {
            L2Side::Bid => L2_SIDE_BID,
            L2Side::Ask => L2_SIDE_ASK,
        };
        ctx.set_fill_color(&rgba_to_hex(side_color));
        ctx.fill_text(L2TapeState::side_label(&event.side), col_side_x as f64, row_mid_y);

        // --- Step 5.5: PRICE column ---
        let decimals = if state.tick_size >= 1.0 {
            0usize
        } else if state.tick_size >= 0.1 {
            1
        } else if state.tick_size >= 0.01 {
            2
        } else if state.tick_size >= 0.001 {
            3
        } else {
            4
        };
        let price_str = format!("{:.prec$}", event.price, prec = decimals);
        ctx.set_fill_color(&rgba_to_hex(L2_TEXT_WHITE));
        ctx.fill_text(&price_str, col_price_x as f64, row_mid_y);

        // --- Step 5.6: QTY column ---
        let qty_str = if event.quantity >= 1000.0 {
            format!("{:.0}", event.quantity)
        } else if event.quantity >= 1.0 {
            format!("{:.2}", event.quantity)
        } else {
            format!("{:.4}", event.quantity)
        };
        ctx.fill_text(&qty_str, col_qty_x as f64, row_mid_y);
    }

    // === STEP 6: Empty state hint ===
    if events.is_empty() {
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&rgba_to_hex(L2_HEADER_TEXT));
        ctx.fill_text(
            "No events",
            (x + width / 2.0) as f64,
            (y + height / 2.0) as f64,
        );
    }
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

    // Render DOM (always present)
    let (dx, dy, dw, dh) = rects.dom;
    render_dom_panel(ctx, dx as f32, dy as f32, dw as f32, dh as f32, &state.dom);

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

// ==========================
// Risk Calculator Panel
// ==========================

// Risk Calculator Colors
const RC_BG: [f32; 4] = [0.051, 0.067, 0.090, 1.0];           // #0d1117
const RC_TITLE_BG: [f32; 4] = [0.071, 0.090, 0.118, 1.0];     // slightly lighter
const RC_LABEL: [f32; 4] = [0.533, 0.533, 0.533, 1.0];         // #888888
const RC_VALUE: [f32; 4] = [0.878, 0.878, 0.878, 1.0];         // #e0e0e0
const RC_RED: [f32; 4] = [0.871, 0.204, 0.267, 1.0];           // red for risk
const RC_GREEN: [f32; 4] = [0.196, 0.804, 0.447, 1.0];         // green for profit
const RC_GOLD: [f32; 4] = [1.0, 0.843, 0.0, 1.0];              // gold for good R:R
const RC_DIVIDER: [f32; 4] = [0.2, 0.22, 0.27, 1.0];           // grey divider
const RC_TITLE_TEXT: [f32; 4] = [0.75, 0.78, 0.85, 1.0];       // title text
const RC_ERROR: [f32; 4] = [0.9, 0.3, 0.3, 1.0];               // validation error

// Risk Calculator Layout
const RC_TITLE_HEIGHT: f32 = 20.0;
const RC_ROW_HEIGHT: f32 = 20.0;
const RC_LEFT_PAD: f32 = 8.0;
const RC_LABEL_WIDTH: f32 = 105.0;

/// Render Risk Calculator panel — form-style display of position sizing and risk/reward calculations
pub fn render_risk_calculator_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &RiskCalculatorState,
) {
    // === STEP 1: Background ===
    ctx.set_fill_color(&rgba_to_hex(RC_BG));
    ctx.fill_rect(x as f64, y as f64, width as f64, height as f64);

    // === STEP 2: Title bar ===
    ctx.set_fill_color(&rgba_to_hex(RC_TITLE_BG));
    ctx.fill_rect(x as f64, y as f64, width as f64, RC_TITLE_HEIGHT as f64);

    ctx.set_fill_color(&rgba_to_hex(RC_TITLE_TEXT));
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(
        "Risk Calculator",
        (x + width / 2.0) as f64,
        (y + RC_TITLE_HEIGHT / 2.0) as f64,
    );

    // === STEP 3: Input fields section ===
    let mut cursor_y = y + RC_TITLE_HEIGHT;

    let input_rows: &[(&str, String)] = &[
        ("Account Size:", format!("${:.2}", state.account_size)),
        ("Risk %:", format!("{:.1}%", state.risk_percent)),
        ("Entry Price:", format!("{:.4}", state.entry_price)),
        ("Stop Loss:", format!("{:.4}", state.stop_loss_price)),
        (
            "Take Profit:",
            state.take_profit_price
                .map(|tp| format!("{:.4}", tp))
                .unwrap_or_else(|| "—".to_string()),
        ),
    ];

    for (label, value) in input_rows {
        let row_mid_y = (cursor_y + RC_ROW_HEIGHT / 2.0) as f64;

        // Label (grey, left-aligned)
        ctx.set_fill_color(&rgba_to_hex(RC_LABEL));
        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, (x + RC_LEFT_PAD) as f64, row_mid_y);

        // Value (white, left-aligned after label column)
        ctx.set_fill_color(&rgba_to_hex(RC_VALUE));
        ctx.fill_text(value, (x + RC_LEFT_PAD + RC_LABEL_WIDTH) as f64, row_mid_y);

        cursor_y += RC_ROW_HEIGHT;
    }

    // === STEP 4: Divider line ===
    ctx.set_fill_color(&rgba_to_hex(RC_DIVIDER));
    ctx.fill_rect(
        (x + RC_LEFT_PAD) as f64,
        cursor_y as f64,
        (width - RC_LEFT_PAD * 2.0) as f64,
        1.0,
    );
    cursor_y += 6.0;

    // === STEP 5: Computed results section ===

    // R:R color logic: gold if >= 2.0, white otherwise
    let rr_color = if let Some(rr) = state.risk_reward_ratio {
        if rr >= 2.0 { RC_GOLD } else { RC_VALUE }
    } else {
        RC_VALUE
    };

    // Leverage display
    let leverage_str = state.leverage
        .map(|lev| format!("{}x", lev))
        .unwrap_or_else(|| "1x".to_string());

    let computed_rows: &[(&str, String, [f32; 4])] = &[
        (
            "Risk Amount:",
            state.format_output("risk_amount"),
            RC_RED,
        ),
        (
            "Position Size:",
            state.format_output("position_size"),
            RC_VALUE,
        ),
        (
            "Risk/Unit:",
            state.format_output("risk_per_unit"),
            RC_VALUE,
        ),
        (
            "Potential Profit:",
            state.format_output("potential_profit"),
            RC_GREEN,
        ),
        (
            "R:R Ratio:",
            state.format_output("risk_reward_ratio"),
            rr_color,
        ),
        (
            "Leverage:",
            leverage_str,
            RC_VALUE,
        ),
        (
            "Margin Req:",
            state.format_output("margin_required"),
            RC_VALUE,
        ),
    ];

    for (label, value, color) in computed_rows {
        // Guard: stop rendering if we've run out of panel height (leave 20px for potential errors)
        if cursor_y + RC_ROW_HEIGHT > y + height - 20.0 {
            break;
        }

        let row_mid_y = (cursor_y + RC_ROW_HEIGHT / 2.0) as f64;

        // Label (grey)
        ctx.set_fill_color(&rgba_to_hex(RC_LABEL));
        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, (x + RC_LEFT_PAD) as f64, row_mid_y);

        // Value (colored)
        ctx.set_fill_color(&rgba_to_hex(*color));
        ctx.fill_text(value, (x + RC_LEFT_PAD + RC_LABEL_WIDTH) as f64, row_mid_y);

        cursor_y += RC_ROW_HEIGHT;
    }

    // === STEP 6: Validation errors ===
    if !state.errors.is_empty() {
        cursor_y += 4.0;
        ctx.set_fill_color(&rgba_to_hex(RC_ERROR));
        ctx.set_font("10px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);

        for error in &state.errors {
            if cursor_y > y + height - RC_ROW_HEIGHT {
                break;
            }
            ctx.fill_text(error, (x + RC_LEFT_PAD) as f64, cursor_y as f64);
            cursor_y += RC_ROW_HEIGHT;
        }
    }
}

// =======================
// Trade Log Panel
// =======================

use crate::trading::trading::trade_log::{OrderSide, TradeLogState};

// Trade Log Colors
const TL_BG: [f32; 4] = [0.051, 0.067, 0.090, 1.0];
const TL_BG_ALT: [f32; 4] = [0.063, 0.082, 0.106, 1.0];
const TL_HEADER_BG: [f32; 4] = [0.071, 0.086, 0.110, 1.0];
const TL_HEADER_TEXT: [f32; 4] = [0.5, 0.55, 0.65, 1.0];
const TL_TEXT_WHITE: [f32; 4] = [0.88, 0.88, 0.90, 1.0];
const TL_TEXT_GREY: [f32; 4] = [0.45, 0.48, 0.54, 1.0];
const TL_BUY_TEXT: [f32; 4] = [0.2, 0.85, 0.4, 1.0];
const TL_SELL_TEXT: [f32; 4] = [0.95, 0.27, 0.36, 1.0];
const TL_PNL_POS: [f32; 4] = [0.2, 0.85, 0.4, 1.0];
const TL_PNL_NEG: [f32; 4] = [0.95, 0.27, 0.36, 1.0];

// Trade Log Layout
const TL_HEADER_HEIGHT: f32 = 18.0;
const TL_ROW_HEIGHT: f32 = 18.0;
const TL_SUMMARY_HEIGHT: f32 = 20.0;
const TL_LEFT_PAD: f32 = 6.0;

/// Render Trade Log panel — scrollable table of executed trades/fills
pub fn render_trade_log_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &TradeLogState,
) {
    // === STEP 1: Background ===
    ctx.set_fill_color(&rgba_to_hex(TL_BG));
    ctx.fill_rect(x as f64, y as f64, width as f64, height as f64);

    // === STEP 2: Column layout (TIME | SYMBOL | SIDE | PRICE | QTY | FEE) ===
    let time_w   = (width * 0.20).max(64.0);
    let symbol_w = (width * 0.20).max(58.0);
    let side_w   = (width * 0.11).max(34.0);
    let price_w  = (width * 0.20).max(60.0);
    let qty_w    = (width * 0.16).max(48.0);

    let col_time_x   = x + TL_LEFT_PAD;
    let col_symbol_x = col_time_x   + time_w;
    let col_side_x   = col_symbol_x + symbol_w;
    let col_price_x  = col_side_x   + side_w;
    let col_qty_x    = col_price_x  + price_w;
    let col_fee_x    = col_qty_x    + qty_w;

    // === STEP 3: Header row ===
    ctx.set_fill_color(&rgba_to_hex(TL_HEADER_BG));
    ctx.fill_rect(x as f64, y as f64, width as f64, TL_HEADER_HEIGHT as f64);

    ctx.set_font("10px monospace");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TL_HEADER_TEXT));

    let header_text_y = (y + TL_HEADER_HEIGHT / 2.0) as f64;
    ctx.fill_text("TIME",   col_time_x   as f64, header_text_y);
    ctx.fill_text("SYMBOL", col_symbol_x as f64, header_text_y);
    ctx.fill_text("SIDE",   col_side_x   as f64, header_text_y);
    ctx.fill_text("PRICE",  col_price_x  as f64, header_text_y);
    ctx.fill_text("QTY",    col_qty_x    as f64, header_text_y);
    ctx.fill_text("FEE",    col_fee_x    as f64, header_text_y);

    // === STEP 4: Trade rows ===
    let content_h = height - TL_HEADER_HEIGHT - TL_SUMMARY_HEIGHT;
    let max_rows = (content_h / TL_ROW_HEIGHT).floor() as usize;

    let trades = state.visible_trades(0, max_rows);

    if trades.is_empty() {
        // Empty state: centered "No trades" message
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&rgba_to_hex(TL_HEADER_TEXT));
        ctx.fill_text(
            "No trades",
            (x + width / 2.0) as f64,
            (y + TL_HEADER_HEIGHT + content_h / 2.0) as f64,
        );
    } else {
        for (i, trade) in trades.iter().enumerate() {
            let row_y = y + TL_HEADER_HEIGHT + (i as f32 * TL_ROW_HEIGHT);

            // Alternating row background
            let row_bg = if i % 2 == 0 { TL_BG } else { TL_BG_ALT };
            ctx.set_fill_color(&rgba_to_hex(row_bg));
            ctx.fill_rect(x as f64, row_y as f64, width as f64, TL_ROW_HEIGHT as f64);

            let text_y = (row_y + TL_ROW_HEIGHT / 2.0) as f64;

            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);

            // TIME column
            let secs = (trade.timestamp / 1000) % 86400;
            let h = secs / 3600;
            let m = (secs % 3600) / 60;
            let s = secs % 60;
            let time_str = format!("{:02}:{:02}:{:02}", h, m, s);
            ctx.set_fill_color(&rgba_to_hex(TL_TEXT_WHITE));
            ctx.fill_text(&time_str, col_time_x as f64, text_y);

            // SYMBOL column (truncate to 9 chars)
            let symbol = if trade.symbol.len() > 9 {
                &trade.symbol[..9]
            } else {
                trade.symbol.as_str()
            };
            ctx.fill_text(symbol, col_symbol_x as f64, text_y);

            // SIDE column (green/red)
            let (side_str, side_color) = match trade.side {
                OrderSide::Buy  => ("BUY",  TL_BUY_TEXT),
                OrderSide::Sell => ("SELL", TL_SELL_TEXT),
            };
            ctx.set_fill_color(&rgba_to_hex(side_color));
            ctx.fill_text(side_str, col_side_x as f64, text_y);

            // PRICE column
            ctx.set_fill_color(&rgba_to_hex(TL_TEXT_WHITE));
            let price_str = format!("{:.4}", trade.price);
            ctx.fill_text(&price_str, col_price_x as f64, text_y);

            // QTY column
            let qty_str = format!("{:.4}", trade.quantity);
            ctx.fill_text(&qty_str, col_qty_x as f64, text_y);

            // FEE column (grey, 9px)
            ctx.set_font("9px monospace");
            ctx.set_fill_color(&rgba_to_hex(TL_TEXT_GREY));
            let fee_str = format!("{:.4}", trade.commission);
            ctx.fill_text(&fee_str, col_fee_x as f64, text_y);

            // Row separator
            ctx.set_fill_color(&rgba_to_hex([0.15, 0.17, 0.22, 0.5]));
            ctx.fill_rect(x as f64, (row_y + TL_ROW_HEIGHT - 1.0) as f64, width as f64, 1.0);
        }
    }

    // === STEP 5: Summary bar at bottom ===
    let summary_y = y + height - TL_SUMMARY_HEIGHT;

    // Separator line above summary
    ctx.set_fill_color(&rgba_to_hex([0.2, 0.22, 0.28, 0.8]));
    ctx.fill_rect(x as f64, summary_y as f64, width as f64, 1.0);

    ctx.set_fill_color(&rgba_to_hex(TL_HEADER_BG));
    ctx.fill_rect(x as f64, (summary_y + 1.0) as f64, width as f64, (TL_SUMMARY_HEIGHT - 1.0) as f64);

    ctx.set_font("10px monospace");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);

    let summary_text_y = (summary_y + TL_SUMMARY_HEIGHT / 2.0) as f64;

    // "Total PnL:" label
    ctx.set_fill_color(&rgba_to_hex(TL_HEADER_TEXT));
    ctx.fill_text("Total PnL:", (x + TL_LEFT_PAD) as f64, summary_text_y);

    // PnL value (green/red)
    let pnl_color = if state.total_pnl >= 0.0 { TL_PNL_POS } else { TL_PNL_NEG };
    ctx.set_fill_color(&rgba_to_hex(pnl_color));
    let pnl_str = format!("{:+.2}", state.total_pnl);
    ctx.fill_text(&pnl_str, (x + TL_LEFT_PAD + 68.0) as f64, summary_text_y);

    // Trade count (right-aligned)
    ctx.set_fill_color(&rgba_to_hex(TL_HEADER_TEXT));
    ctx.set_text_align(TextAlign::Right);
    let count_str = format!("Trades: {}", state.trades.len());
    ctx.fill_text(&count_str, (x + width - TL_LEFT_PAD) as f64, summary_text_y);
}

// ===========================
// Position Manager Panel
// ===========================

use crate::trading::trading::position_manager::{PositionManagerState, PositionSide};

// Position Manager Colors
const PM_BG: [f32; 4] = [0.051, 0.067, 0.090, 1.0];
const PM_HEADER_BG: [f32; 4] = [0.071, 0.086, 0.110, 1.0];
const PM_HEADER_TEXT: [f32; 4] = [0.5, 0.55, 0.65, 1.0];
const PM_TEXT_WHITE: [f32; 4] = [0.88, 0.88, 0.88, 1.0];
const PM_LONG_TEXT: [f32; 4] = [0.2, 0.85, 0.4, 1.0];
const PM_SHORT_TEXT: [f32; 4] = [0.95, 0.27, 0.36, 1.0];
const PM_PNL_POS: [f32; 4] = [0.2, 0.8, 0.3, 1.0];
const PM_PNL_NEG: [f32; 4] = [0.9, 0.2, 0.2, 1.0];
const PM_PNL_NEUTRAL: [f32; 4] = [0.6, 0.6, 0.7, 1.0];
const PM_LIQ_TEXT: [f32; 4] = [1.0, 0.87, 0.2, 1.0];
const PM_SELECTED_BG: [f32; 4] = [0.12, 0.16, 0.22, 1.0];
const PM_SUMMARY_BG: [f32; 4] = [0.063, 0.078, 0.102, 1.0];
const PM_SEPARATOR: [f32; 4] = [0.15, 0.17, 0.22, 0.6];

// Position Manager Layout
const PM_HEADER_HEIGHT: f32 = 20.0;
const PM_ROW_HEIGHT: f32 = 20.0;
const PM_SUMMARY_HEIGHT: f32 = 20.0;
const PM_LEFT_PAD: f32 = 6.0;

/// Render Position Manager panel — open positions table with PnL, entry/mark price, leverage
pub fn render_position_manager_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &PositionManagerState,
) {
    // === STEP 1: Background ===
    ctx.set_fill_color(&rgba_to_hex(PM_BG));
    ctx.fill_rect(x as f64, y as f64, width as f64, height as f64);

    // === STEP 2: Column layout ===
    // SYMBOL | SIDE | QTY | ENTRY | MARK | PNL | LIQ | LEV
    let sym_w   = (width * 0.14).max(52.0);
    let side_w  = (width * 0.08).max(38.0);
    let qty_w   = (width * 0.10).max(44.0);
    let entry_w = (width * 0.14).max(56.0);
    let mark_w  = (width * 0.14).max(56.0);
    let pnl_w   = (width * 0.14).max(52.0);
    let liq_w   = (width * 0.14).max(52.0);
    // LEV takes the remaining space

    let col_sym_x   = x + PM_LEFT_PAD;
    let col_side_x  = col_sym_x  + sym_w;
    let col_qty_x   = col_side_x + side_w;
    let col_entry_x = col_qty_x  + qty_w;
    let col_mark_x  = col_entry_x + entry_w;
    let col_pnl_x   = col_mark_x + mark_w;
    let col_liq_x   = col_pnl_x  + pnl_w;
    let col_lev_x   = col_liq_x  + liq_w;

    // === STEP 3: Header row ===
    ctx.set_fill_color(&rgba_to_hex(PM_HEADER_BG));
    ctx.fill_rect(x as f64, y as f64, width as f64, PM_HEADER_HEIGHT as f64);

    ctx.set_font("10px monospace");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(PM_HEADER_TEXT));

    let header_mid_y = (y + PM_HEADER_HEIGHT / 2.0) as f64;
    ctx.fill_text("SYMBOL", col_sym_x   as f64, header_mid_y);
    ctx.fill_text("SIDE",   col_side_x  as f64, header_mid_y);
    ctx.fill_text("QTY",    col_qty_x   as f64, header_mid_y);
    ctx.fill_text("ENTRY",  col_entry_x as f64, header_mid_y);
    ctx.fill_text("MARK",   col_mark_x  as f64, header_mid_y);
    ctx.fill_text("PNL",    col_pnl_x   as f64, header_mid_y);
    ctx.fill_text("LIQ",    col_liq_x   as f64, header_mid_y);
    ctx.fill_text("LEV",    col_lev_x   as f64, header_mid_y);

    // === STEP 4: Empty state ===
    if state.positions.is_empty() {
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&rgba_to_hex(PM_HEADER_TEXT));
        ctx.fill_text(
            "No open positions",
            (x + width / 2.0) as f64,
            (y + height / 2.0) as f64,
        );
        return;
    }

    // === STEP 5: Position rows ===
    let content_h = height - PM_HEADER_HEIGHT - PM_SUMMARY_HEIGHT;
    let max_rows  = (content_h / PM_ROW_HEIGHT).floor() as usize;
    let visible   = state.visible_positions(0, max_rows);

    for (row_idx, pos) in visible.iter().enumerate() {
        let row_y     = y + PM_HEADER_HEIGHT + (row_idx as f32 * PM_ROW_HEIGHT);
        let row_mid_y = (row_y + PM_ROW_HEIGHT / 2.0) as f64;

        // --- Step 5.1: Row background (selected highlight) ---
        let is_selected = state.selected == Some(row_idx);
        let row_bg = if is_selected { PM_SELECTED_BG } else { PM_BG };
        ctx.set_fill_color(&rgba_to_hex(row_bg));
        ctx.fill_rect(x as f64, row_y as f64, width as f64, PM_ROW_HEIGHT as f64);

        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);

        // --- Step 5.2: SYMBOL ---
        ctx.set_fill_color(&rgba_to_hex(PM_TEXT_WHITE));
        ctx.fill_text(&pos.symbol, col_sym_x as f64, row_mid_y);

        // --- Step 5.3: SIDE (green LONG / red SHORT) ---
        let (side_text, side_color) = match pos.side {
            PositionSide::Long  => ("LONG",  PM_LONG_TEXT),
            PositionSide::Short => ("SHORT", PM_SHORT_TEXT),
        };
        ctx.set_fill_color(&rgba_to_hex(side_color));
        ctx.fill_text(side_text, col_side_x as f64, row_mid_y);

        // --- Step 5.4: QTY ---
        let qty_str = format!("{:.4}", pos.quantity);
        ctx.set_fill_color(&rgba_to_hex(PM_TEXT_WHITE));
        ctx.fill_text(&qty_str, col_qty_x as f64, row_mid_y);

        // --- Step 5.5: ENTRY ---
        let entry_str = format!("{:.4}", pos.entry_price);
        ctx.fill_text(&entry_str, col_entry_x as f64, row_mid_y);

        // --- Step 5.6: MARK ---
        let mark_str = format!("{:.4}", pos.mark_price);
        ctx.fill_text(&mark_str, col_mark_x as f64, row_mid_y);

        // --- Step 5.7: PNL (green +, red -, grey 0) ---
        let pnl_color = if pos.unrealized_pnl > 0.0 {
            PM_PNL_POS
        } else if pos.unrealized_pnl < 0.0 {
            PM_PNL_NEG
        } else {
            PM_PNL_NEUTRAL
        };
        let pnl_str = format!("{:+.2}", pos.unrealized_pnl);
        ctx.set_fill_color(&rgba_to_hex(pnl_color));
        ctx.fill_text(&pnl_str, col_pnl_x as f64, row_mid_y);

        // --- Step 5.8: LIQ (yellow, or "--" if None) ---
        let liq_str = pos.liquidation_price
            .map(|p| format!("{:.4}", p))
            .unwrap_or_else(|| "--".to_string());
        ctx.set_fill_color(&rgba_to_hex(PM_LIQ_TEXT));
        ctx.fill_text(&liq_str, col_liq_x as f64, row_mid_y);

        // --- Step 5.9: LEV ---
        let lev_str = format!("{}x", pos.leverage);
        ctx.set_fill_color(&rgba_to_hex(PM_TEXT_WHITE));
        ctx.fill_text(&lev_str, col_lev_x as f64, row_mid_y);

        // --- Step 5.10: Row separator ---
        ctx.set_fill_color(&rgba_to_hex(PM_SEPARATOR));
        ctx.fill_rect(x as f64, (row_y + PM_ROW_HEIGHT - 1.0) as f64, width as f64, 1.0);
    }

    // === STEP 6: Summary row at bottom ===
    let summary_y = y + height - PM_SUMMARY_HEIGHT;

    ctx.set_fill_color(&rgba_to_hex(PM_SUMMARY_BG));
    ctx.fill_rect(x as f64, summary_y as f64, width as f64, PM_SUMMARY_HEIGHT as f64);

    // Top separator above summary
    ctx.set_fill_color(&rgba_to_hex([0.2, 0.23, 0.30, 1.0]));
    ctx.fill_rect(x as f64, summary_y as f64, width as f64, 1.0);

    let summary_mid_y = (summary_y + PM_SUMMARY_HEIGHT / 2.0) as f64;

    ctx.set_font("10px monospace");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(PM_HEADER_TEXT));
    ctx.fill_text("Total PnL:", (x + PM_LEFT_PAD) as f64, summary_mid_y);

    let total_pnl   = state.total_unrealized_pnl;
    let total_color = if total_pnl > 0.0 {
        PM_PNL_POS
    } else if total_pnl < 0.0 {
        PM_PNL_NEG
    } else {
        PM_PNL_NEUTRAL
    };
    let total_str = format!("{:+.2}", total_pnl);
    ctx.set_fill_color(&rgba_to_hex(total_color));
    ctx.fill_text(&total_str, (x + PM_LEFT_PAD + 70.0) as f64, summary_mid_y);
}

fn render_sub_panel(
    ctx: &mut dyn RenderContext,
    x: f64, y: f64, w: f64, h: f64,
    slot: &crate::trading::trading::trading_container::SubPanelSlot,
    state: &crate::trading::trading::trading_container::TradingContainerState,
    _now_ms: u64,
) {
    use crate::trading::trading::trading_container::SubPanelSlot;
    use crate::trading::footprint::FootprintConfig;

    match slot {
        SubPanelSlot::None => {}
        SubPanelSlot::Footprint => {
            if let Some(ref fp) = state.footprint {
                let config = FootprintConfig::default();
                render_footprint_panel(ctx, x as f32, y as f32, w as f32, h as f32, fp, &config);
            }
        }
        SubPanelSlot::VolumeProfile => {
            if let Some(ref vp) = state.volume_profile {
                let config = crate::trading::volume_profile::VolumeProfileConfig::default();
                render_volume_profile_panel(ctx, x as f32, y as f32, w as f32, h as f32, vp, &config);
            }
        }
        SubPanelSlot::BigTrades => {
            if let Some(ref bt) = state.big_trades {
                render_big_trades_panel(ctx, x as f32, y as f32, w as f32, h as f32, bt);
            }
        }
        SubPanelSlot::L2Tape => {
            if let Some(ref tape) = state.l2_tape {
                render_l2_tape_panel(ctx, x as f32, y as f32, w as f32, h as f32, tape);
            }
        }
    }
}


// ==========================
// Order Entry Panel
// ==========================

// Order Entry Colors
const OE_BG: [f32; 4] = [0.051, 0.067, 0.090, 1.0];
const OE_FIELD_BG: [f32; 4] = [0.071, 0.090, 0.118, 1.0];
const OE_TITLE_BG: [f32; 4] = [0.063, 0.082, 0.106, 1.0];
const OE_TITLE_TEXT: [f32; 4] = [0.88, 0.88, 0.88, 1.0];
const OE_SYMBOL_TEXT: [f32; 4] = [0.5, 0.53, 0.60, 1.0];
const OE_LABEL_TEXT: [f32; 4] = [0.55, 0.58, 0.65, 1.0];
const OE_VALUE_TEXT: [f32; 4] = [0.92, 0.92, 0.92, 1.0];
const OE_ERROR_TEXT: [f32; 4] = [0.95, 0.27, 0.36, 1.0];

// Buy/Sell toggle
const OE_BUY_ACTIVE: [f32; 4] = [0.0, 0.667, 0.333, 1.0];
const OE_BUY_TEXT: [f32; 4] = [0.0, 1.0, 0.533, 1.0];
const OE_BUY_INACTIVE: [f32; 4] = [0.0, 0.20, 0.13, 1.0];
const OE_SELL_ACTIVE: [f32; 4] = [0.8, 0.0, 0.2, 1.0];
const OE_SELL_TEXT: [f32; 4] = [1.0, 0.267, 0.4, 1.0];
const OE_SELL_INACTIVE: [f32; 4] = [0.22, 0.04, 0.08, 1.0];

// Order type tabs
const OE_TAB_ACTIVE_BG: [f32; 4] = [0.14, 0.18, 0.26, 1.0];
const OE_TAB_INACTIVE_BG: [f32; 4] = [0.071, 0.090, 0.118, 1.0];
const OE_TAB_TEXT: [f32; 4] = [0.88, 0.88, 0.88, 1.0];

// Submit button
const OE_SUBMIT_BUY: [f32; 4] = [0.0, 0.667, 0.333, 1.0];
const OE_SUBMIT_SELL: [f32; 4] = [0.8, 0.0, 0.2, 1.0];
const OE_SUBMIT_TEXT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

// Layout
const OE_TITLE_HEIGHT: f32 = 22.0;
const OE_TOGGLE_HEIGHT: f32 = 28.0;
const OE_TAB_HEIGHT: f32 = 22.0;
const OE_FIELD_HEIGHT: f32 = 22.0;
const OE_SUBMIT_HEIGHT: f32 = 30.0;
const OE_PAD: f32 = 6.0;
const OE_ERROR_HEIGHT: f32 = 16.0;

/// Render Order Entry panel — Buy/Sell toggle, order type tabs, form fields,
/// available balance, and submit button.
pub fn render_order_entry_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &OrderEntryState,
) {
    // === STEP 1: Background ===
    ctx.set_fill_color(&rgba_to_hex(OE_BG));
    ctx.fill_rect(x as f64, y as f64, width as f64, height as f64);

    let mut cursor_y = y;

    // === STEP 2: Title bar ===
    ctx.set_fill_color(&rgba_to_hex(OE_TITLE_BG));
    ctx.fill_rect(x as f64, cursor_y as f64, width as f64, OE_TITLE_HEIGHT as f64);

    ctx.set_fill_color(&rgba_to_hex(OE_TITLE_TEXT));
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(
        "Order Entry",
        (x + OE_PAD) as f64,
        (cursor_y + OE_TITLE_HEIGHT / 2.0) as f64,
    );

    if !state.symbol.is_empty() {
        ctx.set_fill_color(&rgba_to_hex(OE_SYMBOL_TEXT));
        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Right);
        ctx.fill_text(
            &state.symbol,
            (x + width - OE_PAD) as f64,
            (cursor_y + OE_TITLE_HEIGHT / 2.0) as f64,
        );
    }

    cursor_y += OE_TITLE_HEIGHT;

    // === STEP 3: Buy/Sell toggle ===
    let half_w = width / 2.0;

    let buy_bg = if state.side == OeSide::Buy { OE_BUY_ACTIVE } else { OE_BUY_INACTIVE };
    ctx.set_fill_color(&rgba_to_hex(buy_bg));
    ctx.fill_rect(x as f64, cursor_y as f64, half_w as f64, OE_TOGGLE_HEIGHT as f64);

    ctx.set_fill_color(&rgba_to_hex(OE_BUY_TEXT));
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(
        "BUY",
        (x + half_w / 2.0) as f64,
        (cursor_y + OE_TOGGLE_HEIGHT / 2.0) as f64,
    );

    let sell_bg = if state.side == OeSide::Sell { OE_SELL_ACTIVE } else { OE_SELL_INACTIVE };
    ctx.set_fill_color(&rgba_to_hex(sell_bg));
    ctx.fill_rect((x + half_w) as f64, cursor_y as f64, half_w as f64, OE_TOGGLE_HEIGHT as f64);

    ctx.set_fill_color(&rgba_to_hex(OE_SELL_TEXT));
    ctx.fill_text(
        "SELL",
        (x + half_w + half_w / 2.0) as f64,
        (cursor_y + OE_TOGGLE_HEIGHT / 2.0) as f64,
    );

    cursor_y += OE_TOGGLE_HEIGHT;

    // === STEP 4: Order type tabs ===
    let tabs: &[(&str, OeOrderType)] = &[
        ("Limit",   OeOrderType::Limit),
        ("Market",  OeOrderType::Market),
        ("Stp-Lmt", OeOrderType::StopLimit),
        ("Stp-Mkt", OeOrderType::StopMarket),
    ];
    let tab_w = width / tabs.len() as f32;

    for (i, (label, ot)) in tabs.iter().enumerate() {
        let tab_x = x + i as f32 * tab_w;
        let is_active = state.order_type == *ot;

        let tab_bg = if is_active { OE_TAB_ACTIVE_BG } else { OE_TAB_INACTIVE_BG };
        ctx.set_fill_color(&rgba_to_hex(tab_bg));
        ctx.fill_rect(tab_x as f64, cursor_y as f64, tab_w as f64, OE_TAB_HEIGHT as f64);

        if is_active {
            let accent = match state.side {
                OeSide::Buy => OE_BUY_ACTIVE,
                OeSide::Sell => OE_SELL_ACTIVE,
            };
            ctx.set_fill_color(&rgba_to_hex(accent));
            ctx.fill_rect(
                tab_x as f64,
                (cursor_y + OE_TAB_HEIGHT - 2.0) as f64,
                tab_w as f64,
                2.0,
            );
        }

        ctx.set_fill_color(&rgba_to_hex(OE_TAB_TEXT));
        ctx.set_font("9px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(
            label,
            (tab_x + tab_w / 2.0) as f64,
            (cursor_y + OE_TAB_HEIGHT / 2.0) as f64,
        );

        if i + 1 < tabs.len() {
            ctx.set_fill_color(&rgba_to_hex([0.15, 0.18, 0.24, 1.0]));
            ctx.fill_rect(
                (tab_x + tab_w - 1.0) as f64,
                cursor_y as f64,
                1.0,
                OE_TAB_HEIGHT as f64,
            );
        }
    }

    cursor_y += OE_TAB_HEIGHT;

    // === STEP 5: Form fields ===
    let field_value_right = x + width - OE_PAD;

    let draw_field = |ctx: &mut dyn RenderContext, row_y: f32, label: &str, value: &str| {
        ctx.set_fill_color(&rgba_to_hex(OE_FIELD_BG));
        ctx.fill_rect(x as f64, row_y as f64, width as f64, OE_FIELD_HEIGHT as f64);

        ctx.set_fill_color(&rgba_to_hex([0.12, 0.15, 0.20, 1.0]));
        ctx.fill_rect(x as f64, (row_y + OE_FIELD_HEIGHT - 1.0) as f64, width as f64, 1.0);

        ctx.set_fill_color(&rgba_to_hex(OE_LABEL_TEXT));
        ctx.set_font("10px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, (x + OE_PAD) as f64, (row_y + OE_FIELD_HEIGHT / 2.0) as f64);

        ctx.set_fill_color(&rgba_to_hex(OE_VALUE_TEXT));
        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Right);
        ctx.fill_text(value, field_value_right as f64, (row_y + OE_FIELD_HEIGHT / 2.0) as f64);
    };

    // Price — Limit / StopLimit
    if matches!(state.order_type, OeOrderType::Limit | OeOrderType::StopLimit) {
        let price_str = state.price
            .map(|p| format!("{:.4}", p))
            .unwrap_or_else(|| "\u{2014}".to_string());
        draw_field(ctx, cursor_y, "Price:", &price_str);
        cursor_y += OE_FIELD_HEIGHT;
    }

    // Stop price — StopLimit / StopMarket
    if matches!(state.order_type, OeOrderType::StopLimit | OeOrderType::StopMarket) {
        let stop_str = state.stop_price
            .map(|p| format!("{:.4}", p))
            .unwrap_or_else(|| "\u{2014}".to_string());
        draw_field(ctx, cursor_y, "Stop:", &stop_str);
        cursor_y += OE_FIELD_HEIGHT;
    }

    // Quantity — always
    draw_field(ctx, cursor_y, "Quantity:", &state.format_quantity());
    cursor_y += OE_FIELD_HEIGHT;

    // Leverage — futures only
    if let Some(lev) = state.leverage {
        draw_field(ctx, cursor_y, "Leverage:", &format!("{}x", lev));
        cursor_y += OE_FIELD_HEIGHT;
    }

    // === STEP 6: Available balance ===
    let bal = state.available_balance;
    let balance_str = if bal >= 1_000_000.0 {
        format!("${:.2}M", bal / 1_000_000.0)
    } else if bal >= 1_000.0 {
        format!("${:.2}K", bal / 1_000.0)
    } else {
        format!("${:.2}", bal)
    };
    draw_field(ctx, cursor_y, "Available:", &balance_str);
    cursor_y += OE_FIELD_HEIGHT;

    // Estimated cost row (when non-zero)
    if state.estimated_cost > 0.0 {
        draw_field(ctx, cursor_y, "Est. Cost:", &state.format_estimated_cost());
        cursor_y += OE_FIELD_HEIGHT;
    }

    // === STEP 7: Submit button ===
    let error_area_h = state.errors.len() as f32 * OE_ERROR_HEIGHT;
    let remaining = y + height - cursor_y - error_area_h;

    if remaining >= OE_SUBMIT_HEIGHT {
        let submit_y = cursor_y + (remaining - OE_SUBMIT_HEIGHT).max(0.0);

        let base_color = match state.side {
            OeSide::Buy => OE_SUBMIT_BUY,
            OeSide::Sell => OE_SUBMIT_SELL,
        };
        let submit_color = if state.submitting {
            [base_color[0] * 0.6, base_color[1] * 0.6, base_color[2] * 0.6, 1.0]
        } else {
            base_color
        };

        ctx.set_fill_color(&rgba_to_hex(submit_color));
        ctx.fill_rect(
            (x + OE_PAD) as f64,
            submit_y as f64,
            (width - OE_PAD * 2.0) as f64,
            OE_SUBMIT_HEIGHT as f64,
        );

        let submit_label = if state.submitting {
            "..."
        } else {
            match state.side {
                OeSide::Buy => "BUY",
                OeSide::Sell => "SELL",
            }
        };

        ctx.set_fill_color(&rgba_to_hex(OE_SUBMIT_TEXT));
        ctx.set_font("13px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(
            submit_label,
            (x + width / 2.0) as f64,
            (submit_y + OE_SUBMIT_HEIGHT / 2.0) as f64,
        );

        cursor_y = submit_y + OE_SUBMIT_HEIGHT;
    }

    // === STEP 8: Validation errors ===
    for error in &state.errors {
        if cursor_y + OE_ERROR_HEIGHT > y + height {
            break;
        }
        ctx.set_fill_color(&rgba_to_hex(OE_ERROR_TEXT));
        ctx.set_font("9px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(
            error,
            (x + OE_PAD) as f64,
            (cursor_y + OE_ERROR_HEIGHT / 2.0) as f64,
        );
        cursor_y += OE_ERROR_HEIGHT;
    }

    let _ = cursor_y;
}
