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
use crate::trading::market_data::market_depth_graph::MarketDepthGraphState;
use crate::trading::market_data::ticker_tape::{TickerTapeState, TickerTapeConfig};

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
const BG_DEFAULT: [f32; 4] = [0.05, 0.05, 0.09, 1.0];           // #0d1117ff
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
const DOM_PRICE_COL_WIDTH: f32 = 100.0;
const DOM_VOLUME_COL_WIDTH: f32 = 120.0;
const DOM_MAX_BAR_WIDTH: f32 = 60.0;

/// Render DOM (Depth of Market) panel - price ladder with bid/ask volume bars
pub fn render_dom_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &DomState,
) {
    // === STEP 1: Draw background ===
    ctx.set_fill_color(&rgba_to_hex(BG_DEFAULT));
    ctx.fill_rect(x as f64, y as f64, width as f64, height as f64);

    // === STEP 2: Calculate layout ===
    let levels = state.visible_levels();
    let row_height = DOM_ROW_HEIGHT;

    // Column layout: [BUY | Bid Volume Bar | Price | Ask Volume Bar | SELL]
    let buy_col_x = x + DOM_LEFT_PAD;
    let buy_col_w = 50.0;

    let bid_vol_col_x = buy_col_x + buy_col_w + 4.0;
    let bid_vol_col_w = DOM_VOLUME_COL_WIDTH;

    let price_col_x = bid_vol_col_x + bid_vol_col_w + 4.0;
    let price_col_w = DOM_PRICE_COL_WIDTH;

    let ask_vol_col_x = price_col_x + price_col_w + 4.0;
    let ask_vol_col_w = DOM_VOLUME_COL_WIDTH;

    let sell_col_x = ask_vol_col_x + ask_vol_col_w + 4.0;
    let sell_col_w = 50.0;

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
            let bar_width = state.bid_bar_width(level.bid_volume, DOM_MAX_BAR_WIDTH);
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
            let bar_width = state.ask_bar_width(level.ask_volume, DOM_MAX_BAR_WIDTH);
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
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);

            // Buy marker (left side)
            ctx.fill_text("▲", (buy_col_x + buy_col_w / 2.0) as f64, price_y as f64);

            // Sell marker (right side)
            ctx.fill_text("▼", (sell_col_x + sell_col_w / 2.0) as f64, price_y as f64);
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

// ============================
// Market Depth Graph Panel
// ============================

// Market Depth Colors
const BID_FILL: [f32; 4] = [0.0, 1.0, 0.53, 0.4];
const BID_STROKE: [f32; 4] = [0.0, 1.0, 0.53, 1.0];
const ASK_FILL: [f32; 4] = [1.0, 0.27, 0.4, 0.4];
const ASK_STROKE: [f32; 4] = [1.0, 0.27, 0.4, 1.0];
const MID_PRICE_LINE: [f32; 4] = [1.0, 0.87, 0.0, 1.0];
const GRID_LINE: [f32; 4] = [0.16, 0.16, 0.16, 1.0];

/// Render Market Depth Graph panel
pub fn render_market_depth_graph_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &MarketDepthGraphState,
) {
    // === STEP 1: Background ===
    ctx.set_fill_color(&rgba_to_hex([0.05, 0.05, 0.09, 1.0]));
    ctx.fill_rect(x as f64, y as f64, width as f64, height as f64);

    // === STEP 2: Grid lines ===
    ctx.set_fill_color(&rgba_to_hex(GRID_LINE));
    for i in 0..5 {
        let grid_y = y + (i as f32 / 4.0) * height;
        ctx.fill_rect(x as f64, grid_y as f64, width as f64, 0.5);
    }

    // === STEP 3: Render bid area (left side, green) ===
    let bid_points = state.bid_curve_points(width, height);
    if bid_points.len() >= 2 {
        // Fill
        ctx.set_fill_color(&rgba_to_hex(BID_FILL));
        ctx.begin_path();
        ctx.move_to((x + bid_points[0].0) as f64, (y + height) as f64);
        for &(px, py) in &bid_points {
            ctx.line_to((x + px) as f64, (y + py) as f64);
        }
        ctx.line_to((x + bid_points.last().unwrap().0) as f64, (y + height) as f64);
        ctx.close_path();
        ctx.fill();

        // Stroke
        ctx.set_stroke_color(&rgba_to_hex(BID_STROKE));
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.move_to((x + bid_points[0].0) as f64, (y + bid_points[0].1) as f64);
        for &(px, py) in &bid_points[1..] {
            ctx.line_to((x + px) as f64, (y + py) as f64);
        }
        ctx.stroke();
    }

    // === STEP 4: Render ask area (right side, red) ===
    let ask_points = state.ask_curve_points(width, height);
    if ask_points.len() >= 2 {
        // Fill
        ctx.set_fill_color(&rgba_to_hex(ASK_FILL));
        ctx.begin_path();
        ctx.move_to((x + ask_points[0].0) as f64, (y + height) as f64);
        for &(px, py) in &ask_points {
            ctx.line_to((x + px) as f64, (y + py) as f64);
        }
        ctx.line_to((x + ask_points.last().unwrap().0) as f64, (y + height) as f64);
        ctx.close_path();
        ctx.fill();

        // Stroke
        ctx.set_stroke_color(&rgba_to_hex(ASK_STROKE));
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.move_to((x + ask_points[0].0) as f64, (y + ask_points[0].1) as f64);
        for &(px, py) in &ask_points[1..] {
            ctx.line_to((x + px) as f64, (y + py) as f64);
        }
        ctx.stroke();
    }

    // === STEP 5: Mid-price vertical line ===
    let mid_x = x + state.price_to_x(state.mid_price, width);

    ctx.set_fill_color(&rgba_to_hex(MID_PRICE_LINE));
    ctx.fill_rect(mid_x as f64, y as f64, 2.0, height as f64);

    // Mid-price label
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_fill_color(&rgba_to_hex(MID_PRICE_LINE));

    let label_text = format!("{:.2}", state.mid_price);
    ctx.fill_text(&label_text, mid_x as f64, (y + 4.0) as f64);

    // === STEP 6: Axes labels ===
    ctx.set_font("9px sans-serif");
    ctx.set_fill_color(&rgba_to_hex([0.53, 0.53, 0.53, 1.0]));

    // Y-axis (volume) labels
    ctx.set_text_align(TextAlign::Right);
    ctx.set_text_baseline(TextBaseline::Middle);
    for i in 0..5 {
        let vol_y = y + (i as f32 / 4.0) * height;
        let vol_value = state.max_depth() * (1.0 - i as f64 / 4.0);
        let vol_text = format!("{:.0}", vol_value);
        ctx.fill_text(&vol_text, (x - 4.0) as f64, vol_y as f64);
    }
}

// ====================
// Ticker Tape Panel
// ====================

// Ticker Tape Colors
const TAPE_BG: [f32; 4] = [0.1, 0.11, 0.14, 1.0];
const BORDER_COLOR: [f32; 4] = [0.16, 0.16, 0.16, 1.0];
const SYMBOL_TEXT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const PRICE_UP: [f32; 4] = [0.0, 1.0, 0.53, 1.0];
const PRICE_DOWN: [f32; 4] = [1.0, 0.27, 0.4, 1.0];
const PRICE_UNCHANGED: [f32; 4] = [0.88, 0.88, 0.88, 1.0];
const SEPARATOR: [f32; 4] = [0.33, 0.33, 0.4, 1.0];

// Ticker Tape Layout
const ITEM_PADDING: f32 = 12.0;
const ITEM_SPACING: f32 = 10.0;

/// Render Ticker Tape panel (with config)
pub fn render_ticker_tape_panel_with_config(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &TickerTapeState,
    config: &TickerTapeConfig,
) {
    // === STEP 1: Background and borders ===
    ctx.set_fill_color(&rgba_to_hex(TAPE_BG));
    ctx.fill_rect(x as f64, y as f64, width as f64, height as f64);

    // Top border
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x as f64, y as f64, width as f64, 1.0);

    // Bottom border
    ctx.fill_rect(x as f64, (y + height - 1.0) as f64, width as f64, 1.0);

    // === STEP 2: Calculate visible items based on scroll offset ===
    let visible_items = state.visible_items(width);

    // === STEP 3: Render ticker items ===
    let mut current_x = x - state.scroll_offset;

    for ticker in visible_items {
        // Skip items completely off-screen to the left
        if current_x + config.item_width < x {
            current_x += config.item_width + ITEM_SPACING;
            continue;
        }

        // Stop if item is completely off-screen to the right
        if current_x > x + width {
            break;
        }

        let item_x = current_x;
        let item_w = config.item_width;

        // --- Symbol text (bold, white) ---
        ctx.set_font("13px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&rgba_to_hex(SYMBOL_TEXT));

        let symbol_x = item_x + ITEM_PADDING;
        let symbol_y = y + height * 0.4;
        ctx.fill_text(&ticker.symbol, symbol_x as f64, symbol_y as f64);

        // --- Price text (colored by direction) ---
        let price_color = if ticker.price_change_percent > 0.0 {
            PRICE_UP
        } else if ticker.price_change_percent < 0.0 {
            PRICE_DOWN
        } else {
            PRICE_UNCHANGED
        };

        ctx.set_font("13px monospace");
        ctx.set_fill_color(&rgba_to_hex(price_color));

        let price_x = symbol_x + 60.0;
        ctx.fill_text(&format!("{:.2}", ticker.last_price), price_x as f64, symbol_y as f64);

        // --- Change text (percentage with arrow) ---
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&rgba_to_hex(price_color));

        let change_x = price_x + 60.0;
        let change_text = format!("{:+.2}%", ticker.price_change_percent);
        ctx.fill_text(&change_text, change_x as f64, symbol_y as f64);

        // --- Arrow indicator ---
        let arrow = if ticker.price_change_percent > 0.0 {
            "▲"
        } else if ticker.price_change_percent < 0.0 {
            "▼"
        } else {
            "—"
        };

        ctx.set_font("10px sans-serif");
        let arrow_x = change_x + 40.0;
        ctx.fill_text(arrow, arrow_x as f64, symbol_y as f64);

        // --- Separator (vertical bar between items) ---
        ctx.set_fill_color(&rgba_to_hex(SEPARATOR));
        let sep_x = item_x + item_w;
        ctx.fill_rect(sep_x as f64, (y + 4.0) as f64, 1.0, (height - 8.0) as f64);

        current_x += item_w + ITEM_SPACING;
    }
}

/// Render Ticker Tape panel (standalone, without config)
pub fn render_ticker_tape_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &TickerTapeState,
) {
    // === STEP 1: Background and borders ===
    ctx.set_fill_color(&rgba_to_hex(TAPE_BG));
    ctx.fill_rect(x as f64, y as f64, width as f64, height as f64);

    // Top border
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x as f64, y as f64, width as f64, 1.0);

    // Bottom border
    ctx.fill_rect(x as f64, (y + height - 1.0) as f64, width as f64, 1.0);

    // === STEP 2: Calculate visible items based on scroll offset ===
    let visible_items = state.visible_items(width);

    // === STEP 3: Render ticker items ===
    let item_width = 200.0;
    let mut current_x = x - state.scroll_offset;

    for ticker in visible_items {
        // Skip items completely off-screen to the left
        if current_x + item_width < x {
            current_x += item_width + ITEM_SPACING;
            continue;
        }

        // Stop if item is completely off-screen to the right
        if current_x > x + width {
            break;
        }

        let item_x = current_x;

        // --- Symbol text (bold, white) ---
        ctx.set_font("13px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&rgba_to_hex(SYMBOL_TEXT));

        let symbol_x = item_x + ITEM_PADDING;
        let symbol_y = y + height * 0.4;
        ctx.fill_text(&ticker.symbol, symbol_x as f64, symbol_y as f64);

        // --- Price text (colored by direction) ---
        let price_color = if ticker.price_change_percent > 0.0 {
            PRICE_UP
        } else if ticker.price_change_percent < 0.0 {
            PRICE_DOWN
        } else {
            PRICE_UNCHANGED
        };

        ctx.set_font("13px monospace");
        ctx.set_fill_color(&rgba_to_hex(price_color));

        let price_x = symbol_x + 60.0;
        ctx.fill_text(&format!("{:.2}", ticker.last_price), price_x as f64, symbol_y as f64);

        // --- Change text (percentage with arrow) ---
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&rgba_to_hex(price_color));

        let change_x = price_x + 60.0;
        let change_text = format!("{:+.2}%", ticker.price_change_percent);
        ctx.fill_text(&change_text, change_x as f64, symbol_y as f64);

        // --- Arrow indicator ---
        let arrow = if ticker.price_change_percent > 0.0 {
            "▲"
        } else if ticker.price_change_percent < 0.0 {
            "▼"
        } else {
            "—"
        };

        ctx.set_font("10px sans-serif");
        let arrow_x = change_x + 40.0;
        ctx.fill_text(arrow, arrow_x as f64, symbol_y as f64);

        // --- Separator (vertical bar between items) ---
        ctx.set_fill_color(&rgba_to_hex(SEPARATOR));
        let sep_x = item_x + item_width;
        ctx.fill_rect(sep_x as f64, (y + 4.0) as f64, 1.0, (height - 8.0) as f64);

        current_x += item_width + ITEM_SPACING;
    }
}

/// Render the tick tape chart (scatter plot of trades)
pub fn render_tick_tape_chart(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &crate::trading::market_data::tick_tape_chart::TickTapeChartState,
    _now_ms: u64,
) {
    use crate::trading::market_data::tick_tape_chart::TickTapeStyle;

    let x = x as f64;
    let y = y as f64;
    let w = width as f64;
    let h = height as f64;

    // Background
    ctx.set_fill_color(&rgba_to_hex([0.05, 0.05, 0.07, 1.0]));
    ctx.fill_rect(x, y, w, h);

    // Price scale area (right side)
    let scale_w = 60.0;
    let chart_w = w - scale_w;

    // Draw price scale background
    ctx.set_fill_color(&rgba_to_hex([0.07, 0.07, 0.09, 1.0]));
    ctx.fill_rect(x + chart_w, y, scale_w, h);

    // Price scale separator
    ctx.set_fill_color(&rgba_to_hex([0.2, 0.2, 0.25, 1.0]));
    ctx.fill_rect(x + chart_w, y, 1.0, h);

    // Draw horizontal grid lines and price labels
    let vp = &state.viewport;
    let price_range = vp.price_max - vp.price_min;
    if price_range > 0.0 {
        let tick = state.dom_tick_size.unwrap_or(1.0);
        let grid_step = (price_range / 10.0 / tick).ceil() * tick; // ~10 grid lines
        if grid_step > 0.0 {
            let first_price = (vp.price_min / grid_step).ceil() * grid_step;
            let mut price = first_price;

            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Right);
            ctx.set_text_baseline(TextBaseline::Middle);

            while price <= vp.price_max {
                let py = vp.price_to_y(price, h) + y;

                // Grid line
                ctx.set_fill_color(&rgba_to_hex([0.15, 0.15, 0.2, 0.5]));
                ctx.fill_rect(x, py, chart_w, 0.5);

                // Price label
                ctx.set_fill_color(&rgba_to_hex([0.5, 0.5, 0.55, 1.0]));
                ctx.fill_text(&format!("{:.0}", price), x + chart_w + scale_w - 4.0, py);

                price += grid_step;
            }
        }
    }

    // DOM market price line
    if let Some(mp) = state.dom_market_price {
        if mp >= vp.price_min && mp <= vp.price_max {
            let my = vp.price_to_y(mp, h) + y;
            ctx.set_fill_color(&rgba_to_hex([1.0, 0.843, 0.0, 0.4]));
            ctx.fill_rect(x, my - 0.5, chart_w, 1.0);
        }
    }

    // Collect visible dots
    let visible = state.visible_dots();

    // Draw ONE continuous line through ALL dots chronologically (single thread)
    if matches!(state.style, TickTapeStyle::PointsAndLines | TickTapeStyle::LinesOnly) {
        if visible.len() > 1 {
            ctx.set_stroke_color(&rgba_to_hex([0.5, 0.5, 0.55, 0.35]));
            ctx.set_stroke_width(state.line_thickness);
            ctx.begin_path();
            for (i, dot) in visible.iter().enumerate() {
                let dx = vp.timestamp_to_x(dot.timestamp, chart_w) + x;
                let dy = vp.price_to_y(dot.price, h) + y;
                if i == 0 { ctx.move_to(dx, dy); } else { ctx.line_to(dx, dy); }
            }
            ctx.stroke();
        }
    }

    // Draw dots (circles)
    if matches!(state.style, TickTapeStyle::PointsAndLines | TickTapeStyle::PointsOnly) {
        for dot in &visible {
            let dx = vp.timestamp_to_x(dot.timestamp, chart_w) + x;
            let dy = vp.price_to_y(dot.price, h) + y;

            // Skip if outside chart area
            if dx < x || dx > x + chart_w || dy < y || dy > y + h { continue; }

            let radius = state.dot_radius(dot.quantity);

            // Fill
            let fill_color = if dot.is_buy {
                [0.055, 0.796, 0.506, 0.7]  // green
            } else {
                [0.965, 0.275, 0.365, 0.7]  // red
            };
            ctx.set_fill_color(&rgba_to_hex(fill_color));
            ctx.begin_path();
            ctx.arc(dx, dy, radius, 0.0, std::f64::consts::TAU);
            ctx.fill();

            // Stroke outline
            let stroke_color = if dot.is_buy {
                [0.055, 0.796, 0.506, 1.0]
            } else {
                [0.965, 0.275, 0.365, 1.0]
            };
            ctx.set_stroke_color(&rgba_to_hex(stroke_color));
            ctx.set_stroke_width(1.0);
            ctx.begin_path();
            ctx.arc(dx, dy, radius, 0.0, std::f64::consts::TAU);
            ctx.stroke();
        }
    }

    // Volume text only inside large enough dots (small dots get text on zoom)
    for dot in &visible {
        let radius = state.dot_radius(dot.quantity);
        if radius < 8.0 { continue; }

        let dx = vp.timestamp_to_x(dot.timestamp, chart_w) + x;
        let dy = vp.price_to_y(dot.price, h) + y;
        if dx < x || dx > x + chart_w || dy < y || dy > y + h { continue; }

        let font_size = (radius * 0.85).clamp(6.0, 11.0);
        ctx.set_font(&format!("{:.0}px sans-serif", font_size));
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&rgba_to_hex([1.0, 1.0, 1.0, 0.95]));

        let vol_str = if dot.quantity >= 10.0 {
            format!("{:.0}", dot.quantity)
        } else if dot.quantity >= 1.0 {
            format!("{:.1}", dot.quantity)
        } else {
            format!("{:.2}", dot.quantity)
        };
        ctx.fill_text(&vol_str, dx, dy);
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

fn render_sub_panel(
    ctx: &mut dyn RenderContext,
    x: f64, y: f64, w: f64, h: f64,
    slot: &crate::trading::trading::trading_container::SubPanelSlot,
    state: &crate::trading::trading::trading_container::TradingContainerState,
    now_ms: u64,
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
        SubPanelSlot::TimeSales => {
            if let Some(ref ts) = state.time_sales {
                super::panel_renderers_table::render_time_sales_panel(ctx, x as f32, y as f32, w as f32, h as f32, ts, now_ms);
            }
        }
        SubPanelSlot::BigTrades => {
            if let Some(ref bt) = state.big_trades {
                super::panel_renderers_table::render_big_trades_panel(ctx, x as f32, y as f32, w as f32, h as f32, bt, now_ms);
            }
        }
        SubPanelSlot::L2Tape => {
            if let Some(ref l2) = state.l2_tape {
                super::panel_renderers_table::render_l2_tape_panel(ctx, x as f32, y as f32, w as f32, h as f32, l2, now_ms);
            }
        }
        SubPanelSlot::TickTapeChart => {
            if let Some(ref tt) = state.tick_tape {
                render_tick_tape_chart(ctx, x as f32, y as f32, w as f32, h as f32, tt, now_ms);
            }
        }
    }
}
