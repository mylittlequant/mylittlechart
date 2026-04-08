//! Table and List Panel Renderers
//!
//! Rendering functions for table-style panels (watchlist, time & sales, positions, etc.)
//! Each function follows a consistent pattern:
//! 1. Background fill
//! 2. Header bar with column labels
//! 3. Scrollable rows with alternating backgrounds
//! 4. Borders and scrollbars

use crate::render::{RenderContext, TextAlign, TextBaseline};
use crate::trading::market_data::watchlist::WatchlistState;
use crate::trading::market_data::time_sales::TimeSalesState;
use crate::trading::order_flow::big_trades::BigTradesState;
use crate::trading::order_flow::l2_tape::L2TapeState;
use crate::trading::trading::trade_log::TradeLogState;
use crate::trading::trading::position_manager::PositionManagerState;
use crate::trading::trading::order_entry::OrderEntryState;
use crate::info::portfolio::account_summary::AccountSummaryState;
use crate::info::options::options_chain::OptionsChainState;
use crate::info::calendar::economic_calendar::EconomicCalendarState;
use crate::info::news::news_feed::NewsState;

/// Convert RGBA array [0.0-1.0] to hex color string
fn rgba_to_hex(rgba: [f32; 4]) -> String {
    let r = (rgba[0].clamp(0.0, 1.0) * 255.0) as u8;
    let g = (rgba[1].clamp(0.0, 1.0) * 255.0) as u8;
    let b = (rgba[2].clamp(0.0, 1.0) * 255.0) as u8;
    let a = (rgba[3].clamp(0.0, 1.0) * 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", r, g, b, a)
}

/// Helper: render vertical scrollbar
fn render_scrollbar_vertical(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, h: f64, ratio: f64) {
    // Track
    let track_color = [0.118, 0.133, 0.176, 1.0]; // #1e222d
    ctx.set_fill_color(&rgba_to_hex(track_color));
    ctx.fill_rect(x, y, w, h);

    // Thumb
    let thumb_h = (h * 0.3).max(40.0); // At least 40px
    let thumb_y = y + ratio * (h - thumb_h);
    let thumb_color = [0.212, 0.227, 0.271, 1.0]; // #363a45
    ctx.set_fill_color(&rgba_to_hex(thumb_color));
    ctx.fill_rect(x, thumb_y, w, thumb_h);
}

/// Helper: format volume (e.g., 1.2M, 3.4K)
fn format_volume(vol: f64) -> String {
    if vol >= 1_000_000.0 {
        format!("{:.1}M", vol / 1_000_000.0)
    } else if vol >= 1_000.0 {
        format!("{:.1}K", vol / 1_000.0)
    } else {
        format!("{:.0}", vol)
    }
}

// =============================================================================
// 1. WATCHLIST PANEL
// =============================================================================

pub fn render_watchlist_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &WatchlistState,
) {
    // Colors - Dark Theme (TradingView-style)
    const BG_COLOR: [f32; 4] = [0.075, 0.090, 0.133, 1.0];              // #131722
    const HEADER_BG: [f32; 4] = [0.118, 0.133, 0.176, 1.0];             // #1e222d
    const HEADER_TEXT: [f32; 4] = [0.698, 0.710, 0.745, 1.0];           // #b2b5be
    const ROW_EVEN: [f32; 4] = [0.075, 0.090, 0.133, 1.0];              // #131722
    const ROW_ODD: [f32; 4] = [0.118, 0.133, 0.176, 1.0];               // #1e222d
    const TEXT_PRIMARY: [f32; 4] = [0.820, 0.831, 0.863, 1.0];          // #d1d4dc
    const TEXT_SECONDARY: [f32; 4] = [0.471, 0.482, 0.525, 1.0];        // #787b86
    const POSITIVE_GREEN: [f32; 4] = [0.055, 0.796, 0.506, 1.0];        // #0ecb81
    const NEGATIVE_RED: [f32; 4] = [0.965, 0.275, 0.365, 1.0];          // #f6465d
    const BORDER_COLOR: [f32; 4] = [0.165, 0.180, 0.224, 1.0];          // #2a2e39

    // Layout
    const HEADER_HEIGHT: f64 = 32.0;
    const ROW_HEIGHT: f64 = 40.0;
    const CELL_PADDING_H: f64 = 12.0;

    // Column widths
    const COL_SYMBOL_W: f64 = 100.0;
    const COL_PRICE_W: f64 = 90.0;
    const COL_CHANGE_W: f64 = 80.0;
    const COL_VOLUME_W: f64 = 90.0;

    let x = x as f64;
    let y = y as f64;
    let w = width as f64;
    let h = height as f64;

    // 1. Background
    ctx.set_fill_color(&rgba_to_hex(BG_COLOR));
    ctx.fill_rect(x, y, w, h);

    // 2. Header bar
    ctx.set_fill_color(&rgba_to_hex(HEADER_BG));
    ctx.fill_rect(x, y, w, HEADER_HEIGHT);

    // Header text
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(HEADER_TEXT));

    let mut col_x = x + CELL_PADDING_H;
    ctx.fill_text("SYMBOL", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_SYMBOL_W;
    ctx.fill_text("PRICE", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_PRICE_W;
    ctx.fill_text("CHANGE %", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_CHANGE_W;
    ctx.fill_text("VOLUME", col_x, y + HEADER_HEIGHT / 2.0);

    // Header separator line
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x, y + HEADER_HEIGHT, w, 1.0);

    // 3. Rows
    let content_y = y + HEADER_HEIGHT + 1.0;
    let max_rows = ((h - HEADER_HEIGHT - 1.0) / ROW_HEIGHT).floor() as usize;

    for (i, symbol) in state.visible_rows(0, max_rows).iter().enumerate() {
        let row_y = content_y + (i as f64 * ROW_HEIGHT);

        // Alternating background
        let bg = if i % 2 == 0 { ROW_EVEN } else { ROW_ODD };
        ctx.set_fill_color(&rgba_to_hex(bg));
        ctx.fill_rect(x, row_y, w, ROW_HEIGHT);

        let row_center_y = row_y + ROW_HEIGHT / 2.0;
        let mut col_x = x + CELL_PADDING_H;

        // Symbol column
        ctx.set_font("14px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(symbol, col_x, row_center_y);
        col_x += COL_SYMBOL_W;

        // Price column (monospace, right-aligned)
        if let Some(ticker) = state.tickers.get(*symbol) {
            ctx.set_font("14px monospace");
            ctx.set_text_align(TextAlign::Right);
            ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
            ctx.fill_text(&format!("{:.2}", ticker.last_price), col_x + COL_PRICE_W - CELL_PADDING_H, row_center_y);
            col_x += COL_PRICE_W;

            // Change % column (colored)
            let change_pct = ticker.price_change_percent_24h;
            let change_color = if change_pct >= 0.0 { POSITIVE_GREEN } else { NEGATIVE_RED };

            ctx.set_font("14px monospace");
            ctx.set_text_align(TextAlign::Right);
            ctx.set_fill_color(&rgba_to_hex(change_color));
            let sign = if change_pct >= 0.0 { "+" } else { "" };
            ctx.fill_text(&format!("{}{:.2}%", sign, change_pct), col_x + COL_CHANGE_W - CELL_PADDING_H, row_center_y);
            col_x += COL_CHANGE_W;

            // Volume column (abbreviated, secondary text)
            ctx.set_font("12px sans-serif");
            ctx.set_text_align(TextAlign::Right);
            ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
            ctx.fill_text(&format_volume(ticker.volume_24h), col_x + COL_VOLUME_W - CELL_PADDING_H, row_center_y);
        }
    }

    // 4. Right border
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x + w - 1.0, y, 1.0, h);
}

// =============================================================================
// 2. TIME & SALES PANEL
// =============================================================================

pub fn render_time_sales_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &TimeSalesState,
    now_ms: u64,
) {
    // Colors - Very Dark Theme (Sierra Chart / Bookmap style)
    const BG_COLOR: [f32; 4] = [0.059, 0.059, 0.067, 1.0];              // #0f0f11
    const HEADER_BG: [f32; 4] = [0.102, 0.102, 0.110, 1.0];             // #1a1a1c
    const HEADER_TEXT: [f32; 4] = [0.620, 0.627, 0.647, 1.0];           // #9ea0a5
    const TEXT_PRIMARY: [f32; 4] = [0.882, 0.890, 0.910, 1.0];          // #e1e3e8
    const TEXT_SECONDARY: [f32; 4] = [0.471, 0.482, 0.525, 1.0];        // #787b86
    const ASK_BG: [f32; 4] = [0.055, 0.796, 0.506, 0.08];               // #0ecb81 at 8% alpha
    const BID_BG: [f32; 4] = [0.965, 0.275, 0.365, 0.08];               // #f6465d at 8% alpha
    const ASK_INDICATOR: [f32; 4] = [0.055, 0.796, 0.506, 1.0];         // #0ecb81
    const BID_INDICATOR: [f32; 4] = [0.965, 0.275, 0.365, 1.0];         // #f6465d
    const BORDER_COLOR: [f32; 4] = [0.165, 0.165, 0.180, 1.0];          // #2a2a2e

    // Layout
    const HEADER_HEIGHT: f64 = 28.0;
    const ROW_HEIGHT: f64 = 20.0;
    const CELL_PADDING_H: f64 = 4.0;
    const INDICATOR_WIDTH: f64 = 3.0;

    // Column widths
    const COL_TIME_W: f64 = 70.0;
    const COL_PRICE_W: f64 = 80.0;
    const COL_SIZE_W: f64 = 60.0;

    let x = x as f64;
    let y = y as f64;
    let w = width as f64;
    let h = height as f64;

    // 1. Background
    ctx.set_fill_color(&rgba_to_hex(BG_COLOR));
    ctx.fill_rect(x, y, w, h);

    // 2. Header bar
    ctx.set_fill_color(&rgba_to_hex(HEADER_BG));
    ctx.fill_rect(x, y, w, HEADER_HEIGHT);

    // Header text
    ctx.set_font("10px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(HEADER_TEXT));

    let mut col_x = x + INDICATOR_WIDTH + CELL_PADDING_H;
    ctx.fill_text("TIME", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_TIME_W;
    ctx.fill_text("PRICE", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_PRICE_W;
    ctx.fill_text("SIZE", col_x, y + HEADER_HEIGHT / 2.0);

    // Header separator
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x, y + HEADER_HEIGHT, w, 1.0);

    // 3. Rows (trades scroll from top)
    let content_y = y + HEADER_HEIGHT + 1.0;
    let max_rows = ((h - HEADER_HEIGHT - 1.0) / ROW_HEIGHT).floor() as usize;

    // Calculate max quantity for relative sizing bar
    let visible = state.visible_trades(max_rows);
    let max_qty = visible.iter().map(|t| t.quantity).fold(0.0_f64, f64::max);

    for (i, trade) in visible.iter().enumerate() {
        let row_y = content_y + (i as f64 * ROW_HEIGHT);

        // Row background based on trade side
        let (row_bg, indicator_color) = match trade.side {
            crate::trading::market_data::time_sales::TradeSide::Buy => (ASK_BG, ASK_INDICATOR),
            crate::trading::market_data::time_sales::TradeSide::Sell => (BID_BG, BID_INDICATOR),
        };

        ctx.set_fill_color(&rgba_to_hex(row_bg));
        ctx.fill_rect(x + INDICATOR_WIDTH, row_y, w - INDICATOR_WIDTH, ROW_HEIGHT);

        // Flash effect for new trades (bright flash fading over 500ms)
        for &(flash_idx, flash_start) in &state.flash_trades {
            if flash_idx == i {
                let age_ms = now_ms.saturating_sub(flash_start);
                if age_ms < 500 {
                    let alpha = 0.3 * (1.0 - age_ms as f64 / 500.0);
                    ctx.set_fill_color(&rgba_to_hex([1.0, 1.0, 1.0, alpha as f32]));
                    ctx.fill_rect(x + INDICATOR_WIDTH, row_y, w - INDICATOR_WIDTH, ROW_HEIGHT);
                }
            }
        }

        // Subtle alternating row overlay for readability
        if i % 2 == 1 {
            ctx.set_fill_color(&rgba_to_hex([1.0, 1.0, 1.0, 0.02])); // very subtle
            ctx.fill_rect(x + INDICATOR_WIDTH, row_y, w - INDICATOR_WIDTH, ROW_HEIGHT);
        }

        // Trade side indicator bar (3px wide, left edge)
        ctx.set_fill_color(&rgba_to_hex(indicator_color));
        ctx.fill_rect(x, row_y, INDICATOR_WIDTH, ROW_HEIGHT);

        // Highlight trades at DOM market price with gold indicator
        if let Some(dom_price) = state.dom_market_price {
            if let Some(tick_size) = state.dom_tick_size {
                if (trade.price - dom_price).abs() < tick_size * 0.5 {
                    ctx.set_fill_color(&rgba_to_hex([1.0, 0.843, 0.0, 0.5]));
                    ctx.fill_rect(x, row_y, INDICATOR_WIDTH, ROW_HEIGHT);
                }
            }
        }

        let row_center_y = row_y + ROW_HEIGHT / 2.0;
        let mut col_x = x + INDICATOR_WIDTH + CELL_PADDING_H;

        // Time column (format timestamp)
        ctx.set_font("11px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        let time_str = {
            let secs = (trade.timestamp / 1000) % 86400;
            let h = secs / 3600;
            let m = (secs % 3600) / 60;
            let s = secs % 60;
            format!("{:02}:{:02}:{:02}", h, m, s)
        };
        ctx.fill_text(&time_str, col_x, row_center_y);
        col_x += COL_TIME_W;

        // Price column
        ctx.set_font("12px monospace");
        ctx.set_text_align(TextAlign::Right);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&format!("{:.2}", trade.price), col_x + COL_PRICE_W - CELL_PADDING_H, row_center_y);
        col_x += COL_PRICE_W;

        // Size column
        ctx.set_font("12px monospace");
        ctx.set_text_align(TextAlign::Right);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&format!("{:.4}", trade.quantity), col_x + COL_SIZE_W - CELL_PADDING_H, row_center_y);

        // Size indicator bar (relative to max visible trade)
        if max_qty > 0.0 {
            let bar_ratio = (trade.quantity / max_qty).min(1.0);
            let bar_w = bar_ratio * (w - col_x - 10.0).max(0.0);
            let bar_h = 3.0;
            let bar_y = row_y + ROW_HEIGHT - bar_h - 1.0;

            ctx.set_fill_color(&rgba_to_hex(indicator_color));
            ctx.fill_rect(col_x + CELL_PADDING_H, bar_y, bar_w, bar_h);
        }
    }

    // 4. Right border
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x + w - 1.0, y, 1.0, h);
}

// =============================================================================
// 3. BIG TRADES PANEL
// =============================================================================

pub fn render_big_trades_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &BigTradesState,
    now_ms: u64,
) {
    // Colors - Dark Theme with emphasis
    const BG_COLOR: [f32; 4] = [0.059, 0.059, 0.067, 1.0];              // #0f0f11
    const HEADER_BG: [f32; 4] = [0.102, 0.102, 0.110, 1.0];             // #1a1a1c
    const HEADER_TEXT: [f32; 4] = [0.620, 0.627, 0.647, 1.0];           // #9ea0a5
    const TEXT_PRIMARY: [f32; 4] = [0.882, 0.890, 0.910, 1.0];          // #e1e3e8
    const TEXT_SECONDARY: [f32; 4] = [0.471, 0.482, 0.525, 1.0];        // #787b86
    const BUY_BG: [f32; 4] = [0.055, 0.796, 0.506, 0.12];               // #0ecb81 at 12% alpha
    const SELL_BG: [f32; 4] = [0.965, 0.275, 0.365, 0.12];              // #f6465d at 12% alpha
    const BUY_INDICATOR: [f32; 4] = [0.055, 0.796, 0.506, 1.0];         // #0ecb81
    const SELL_INDICATOR: [f32; 4] = [0.965, 0.275, 0.365, 1.0];        // #f6465d
    const LARGE_TRADE_BG: [f32; 4] = [1.0, 0.647, 0.0, 0.15];           // #ffa500 at 15% alpha
    const BORDER_COLOR: [f32; 4] = [0.165, 0.165, 0.180, 1.0];          // #2a2a2e

    // Layout
    const HEADER_HEIGHT: f64 = 28.0;
    const ROW_HEIGHT: f64 = 24.0;
    const CELL_PADDING_H: f64 = 6.0;
    const INDICATOR_WIDTH: f64 = 4.0;

    // Column widths
    const COL_TIME_W: f64 = 70.0;
    const COL_PRICE_W: f64 = 90.0;
    const COL_SIZE_W: f64 = 80.0;
    const COL_VALUE_W: f64 = 100.0;

    let x = x as f64;
    let y = y as f64;
    let w = width as f64;
    let h = height as f64;

    // 1. Background
    ctx.set_fill_color(&rgba_to_hex(BG_COLOR));
    ctx.fill_rect(x, y, w, h);

    // 2. Header bar
    ctx.set_fill_color(&rgba_to_hex(HEADER_BG));
    ctx.fill_rect(x, y, w, HEADER_HEIGHT);

    // Header text
    ctx.set_font("10px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(HEADER_TEXT));

    let mut col_x = x + INDICATOR_WIDTH + CELL_PADDING_H;
    ctx.fill_text("TIME", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_TIME_W;
    ctx.fill_text("PRICE", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_PRICE_W;
    ctx.fill_text("SIZE", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_SIZE_W;
    ctx.fill_text("VALUE", col_x, y + HEADER_HEIGHT / 2.0);

    // Header separator
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x, y + HEADER_HEIGHT, w, 1.0);

    // 3. Rows
    let content_y = y + HEADER_HEIGHT + 1.0;
    let max_rows = ((h - HEADER_HEIGHT - 1.0) / ROW_HEIGHT).floor() as usize;

    for (i, trade) in state.visible_trades(max_rows).iter().enumerate() {
        let row_y = content_y + (i as f64 * ROW_HEIGHT);

        // Row background based on trade side
        let (row_bg, indicator_color) = match trade.side {
            crate::trading::order_flow::big_trades::TradeSide::Buy => (BUY_BG, BUY_INDICATOR),
            crate::trading::order_flow::big_trades::TradeSide::Sell => (SELL_BG, SELL_INDICATOR),
        };

        ctx.set_fill_color(&rgba_to_hex(row_bg));
        ctx.fill_rect(x + INDICATOR_WIDTH, row_y, w - INDICATOR_WIDTH, ROW_HEIGHT);

        // Flash effect for new trades (bright flash fading over 500ms)
        for &(flash_idx, flash_start) in &state.flash_trades {
            if flash_idx == i {
                let age_ms = now_ms.saturating_sub(flash_start);
                if age_ms < 500 {
                    let alpha = 0.3 * (1.0 - age_ms as f64 / 500.0);
                    ctx.set_fill_color(&rgba_to_hex([1.0, 1.0, 1.0, alpha as f32]));
                    ctx.fill_rect(x + INDICATOR_WIDTH, row_y, w - INDICATOR_WIDTH, ROW_HEIGHT);
                }
            }
        }

        // Check if this is an extra large trade (>2x threshold)
        let is_extra_large = trade.quantity > state.size_threshold * 2.0;

        // Extra large trade highlighting
        if is_extra_large {
            ctx.set_fill_color(&rgba_to_hex(LARGE_TRADE_BG));
            ctx.fill_rect(x + INDICATOR_WIDTH, row_y, w - INDICATOR_WIDTH, ROW_HEIGHT);
        }

        // Trade side indicator bar
        ctx.set_fill_color(&rgba_to_hex(indicator_color));
        ctx.fill_rect(x, row_y, INDICATOR_WIDTH, ROW_HEIGHT);

        // Highlight trades at DOM market price with gold indicator
        if let Some(dom_price) = state.dom_market_price {
            if let Some(tick_size) = state.dom_tick_size {
                if (trade.price - dom_price).abs() < tick_size * 0.5 {
                    ctx.set_fill_color(&rgba_to_hex([1.0, 0.843, 0.0, 0.5]));
                    ctx.fill_rect(x, row_y, INDICATOR_WIDTH, ROW_HEIGHT);
                }
            }
        }

        let row_center_y = row_y + ROW_HEIGHT / 2.0;
        let mut col_x = x + INDICATOR_WIDTH + CELL_PADDING_H;

        // Time column (format timestamp)
        ctx.set_font("11px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        let time_str = {
            let secs = (trade.timestamp / 1000) % 86400;
            let h = secs / 3600;
            let m = (secs % 3600) / 60;
            let s = secs % 60;
            format!("{:02}:{:02}:{:02}", h, m, s)
        };
        ctx.fill_text(&time_str, col_x, row_center_y);
        col_x += COL_TIME_W;

        // Price column
        ctx.set_font("12px monospace");
        ctx.set_text_align(TextAlign::Right);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&format!("{:.2}", trade.price), col_x + COL_PRICE_W - CELL_PADDING_H, row_center_y);
        col_x += COL_PRICE_W;

        // Size column (bold if extra large)
        let font = if is_extra_large { "700 12px monospace" } else { "12px monospace" };
        ctx.set_font(font);
        ctx.set_text_align(TextAlign::Right);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&format!("{:.4}", trade.quantity), col_x + COL_SIZE_W - CELL_PADDING_H, row_center_y);
        col_x += COL_SIZE_W;

        // Value column (USD - calculated from price * quantity)
        ctx.set_font("12px monospace");
        ctx.set_text_align(TextAlign::Right);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        let value_usd = trade.price * trade.quantity;
        ctx.fill_text(&format!("${:.0}", value_usd), col_x + COL_VALUE_W - CELL_PADDING_H, row_center_y);
    }

    // 4. Right border
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x + w - 1.0, y, 1.0, h);
}

// =============================================================================
// 3b. L2 TAPE PANEL (Order Book Event Stream)
// =============================================================================

pub fn render_l2_tape_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &L2TapeState,
    _now_ms: u64,
) {
    use crate::trading::order_flow::{L2TapeState, L2EventType, L2Side};

    // Colors
    const BG_COLOR: [f32; 4] = [0.059, 0.059, 0.067, 1.0];
    const HEADER_BG: [f32; 4] = [0.102, 0.102, 0.110, 1.0];
    const HEADER_TEXT: [f32; 4] = [0.620, 0.627, 0.647, 1.0];
    const TEXT_PRIMARY: [f32; 4] = [0.882, 0.890, 0.910, 1.0];
    const TEXT_SECONDARY: [f32; 4] = [0.471, 0.482, 0.525, 1.0];
    const BORDER_COLOR: [f32; 4] = [0.165, 0.165, 0.180, 1.0];

    // Event type colors
    const ADD_BID_BG: [f32; 4] = [0.055, 0.796, 0.506, 0.06];
    const ADD_ASK_BG: [f32; 4] = [0.965, 0.275, 0.365, 0.06];
    const CANCEL_BG: [f32; 4] = [0.471, 0.482, 0.525, 0.04];
    const MODIFY_BG: [f32; 4] = [0.529, 0.467, 0.878, 0.06];
    const EXECUTE_BG: [f32; 4] = [1.0, 0.843, 0.0, 0.08];

    // Layout
    const HEADER_HEIGHT: f64 = 28.0;
    const ROW_HEIGHT: f64 = 18.0;  // Smaller rows for high-frequency data
    const CELL_PADDING_H: f64 = 3.0;
    const INDICATOR_WIDTH: f64 = 3.0;

    let x = x as f64;
    let y = y as f64;
    let w = width as f64;
    let h = height as f64;

    // Background
    ctx.set_fill_color(&rgba_to_hex(BG_COLOR));
    ctx.fill_rect(x, y, w, h);

    // Header
    ctx.set_fill_color(&rgba_to_hex(HEADER_BG));
    ctx.fill_rect(x, y, w, HEADER_HEIGHT);

    ctx.set_font("9px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(HEADER_TEXT));

    let mut col_x = x + INDICATOR_WIDTH + CELL_PADDING_H;
    ctx.fill_text("TIME", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += 60.0;
    ctx.fill_text("TYPE", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += 35.0;
    ctx.fill_text("SIDE", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += 35.0;
    ctx.fill_text("PRICE", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += 70.0;
    ctx.fill_text("SIZE", col_x, y + HEADER_HEIGHT / 2.0);

    // Header separator
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x, y + HEADER_HEIGHT, w, 1.0);

    // Rows
    let content_y = y + HEADER_HEIGHT + 1.0;
    let max_rows = ((h - HEADER_HEIGHT - 1.0) / ROW_HEIGHT).floor() as usize;
    let visible = state.visible_events(max_rows);

    for (i, event) in visible.iter().enumerate() {
        let row_y = content_y + (i as f64 * ROW_HEIGHT);

        // Row background based on event type
        let row_bg = match event.event_type {
            L2EventType::Add => match event.side {
                L2Side::Bid => ADD_BID_BG,
                L2Side::Ask => ADD_ASK_BG,
            },
            L2EventType::Cancel => CANCEL_BG,
            L2EventType::Modify => MODIFY_BG,
            L2EventType::Execute => EXECUTE_BG,
        };

        ctx.set_fill_color(&rgba_to_hex(row_bg));
        ctx.fill_rect(x + INDICATOR_WIDTH, row_y, w - INDICATOR_WIDTH, ROW_HEIGHT);

        // Alternating row overlay
        if i % 2 == 1 {
            ctx.set_fill_color(&rgba_to_hex([1.0, 1.0, 1.0, 0.015]));
            ctx.fill_rect(x + INDICATOR_WIDTH, row_y, w - INDICATOR_WIDTH, ROW_HEIGHT);
        }

        // Side indicator bar
        let indicator_color = state.event_color(event);
        ctx.set_fill_color(&rgba_to_hex(indicator_color));
        ctx.fill_rect(x, row_y, INDICATOR_WIDTH, ROW_HEIGHT);

        let row_center_y = row_y + ROW_HEIGHT / 2.0;
        let mut col_x = x + INDICATOR_WIDTH + CELL_PADDING_H;

        // Time
        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        let time_str = {
            let secs = (event.timestamp / 1000) % 86400;
            let h = secs / 3600;
            let m = (secs % 3600) / 60;
            let s = secs % 60;
            let ms = event.timestamp % 1000;
            format!("{:02}:{:02}:{:02}.{:03}", h, m, s, ms)
        };
        ctx.fill_text(&time_str, col_x, row_center_y);
        col_x += 60.0;

        // Event type (colored label)
        let type_color = state.event_color(event);
        ctx.set_fill_color(&rgba_to_hex(type_color));
        ctx.set_font("700 10px monospace");
        ctx.fill_text(L2TapeState::event_label(&event.event_type), col_x, row_center_y);
        col_x += 35.0;

        // Side
        let side_color = match event.side {
            L2Side::Bid => [0.055, 0.796, 0.506, 1.0],
            L2Side::Ask => [0.965, 0.275, 0.365, 1.0],
        };
        ctx.set_fill_color(&rgba_to_hex(side_color));
        ctx.set_font("10px monospace");
        ctx.fill_text(L2TapeState::side_label(&event.side), col_x, row_center_y);
        col_x += 35.0;

        // Price
        ctx.set_font("11px monospace");
        ctx.set_text_align(TextAlign::Right);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&format!("{:.2}", event.price), col_x + 65.0, row_center_y);
        col_x += 70.0;

        // Size
        ctx.set_text_align(TextAlign::Right);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&format!("{:.4}", event.quantity), col_x + 55.0, row_center_y);

        // DOM market price highlight
        if let Some(dom_price) = state.dom_market_price {
            if let Some(tick_size) = state.dom_tick_size {
                if (event.price - dom_price).abs() < tick_size * 0.5 {
                    ctx.set_fill_color(&rgba_to_hex([1.0, 0.843, 0.0, 0.4]));
                    ctx.fill_rect(x, row_y, INDICATOR_WIDTH, ROW_HEIGHT);
                }
            }
        }
    }

    // Right border
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x + w - 1.0, y, 1.0, h);
}

// =============================================================================
// 4. TRADE LOG PANEL
// =============================================================================

pub fn render_trade_log_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &TradeLogState,
) {
    // Colors - Dark Theme
    const BG_COLOR: [f32; 4] = [0.075, 0.090, 0.133, 1.0];              // #131722
    const HEADER_BG: [f32; 4] = [0.118, 0.133, 0.176, 1.0];             // #1e222d
    const HEADER_TEXT: [f32; 4] = [0.698, 0.710, 0.745, 1.0];           // #b2b5be
    const ROW_EVEN: [f32; 4] = [0.075, 0.090, 0.133, 1.0];              // #131722
    const ROW_ODD: [f32; 4] = [0.118, 0.133, 0.176, 1.0];               // #1e222d
    const TEXT_PRIMARY: [f32; 4] = [0.820, 0.831, 0.863, 1.0];          // #d1d4dc
    const TEXT_SECONDARY: [f32; 4] = [0.471, 0.482, 0.525, 1.0];        // #787b86
    const STATUS_FILLED: [f32; 4] = [0.055, 0.796, 0.506, 1.0];         // #0ecb81
    const STATUS_CANCELLED: [f32; 4] = [0.965, 0.275, 0.365, 1.0];      // #f6465d
    const BORDER_COLOR: [f32; 4] = [0.165, 0.180, 0.224, 1.0];          // #2a2e39

    // Layout
    const HEADER_HEIGHT: f64 = 32.0;
    const ROW_HEIGHT: f64 = 36.0;
    const CELL_PADDING_H: f64 = 10.0;

    // Column widths
    const COL_TIME_W: f64 = 130.0;
    const COL_SYMBOL_W: f64 = 90.0;
    const COL_SIDE_W: f64 = 60.0;
    const COL_PRICE_W: f64 = 90.0;
    const COL_SIZE_W: f64 = 80.0;

    let x = x as f64;
    let y = y as f64;
    let w = width as f64;
    let h = height as f64;

    // 1. Background
    ctx.set_fill_color(&rgba_to_hex(BG_COLOR));
    ctx.fill_rect(x, y, w, h);

    // 2. Header bar
    ctx.set_fill_color(&rgba_to_hex(HEADER_BG));
    ctx.fill_rect(x, y, w, HEADER_HEIGHT);

    // Header text
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(HEADER_TEXT));

    let mut col_x = x + CELL_PADDING_H;
    ctx.fill_text("TIME", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_TIME_W;
    ctx.fill_text("SYMBOL", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_SYMBOL_W;
    ctx.fill_text("SIDE", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_SIDE_W;
    ctx.fill_text("PRICE", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_PRICE_W;
    ctx.fill_text("SIZE", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_SIZE_W;
    ctx.fill_text("STATUS", col_x, y + HEADER_HEIGHT / 2.0);

    // Header separator
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x, y + HEADER_HEIGHT, w, 1.0);

    // 3. Rows
    let content_y = y + HEADER_HEIGHT + 1.0;
    let max_rows = ((h - HEADER_HEIGHT - 1.0) / ROW_HEIGHT).floor() as usize;

    for (i, trade) in state.visible_trades(0, max_rows).iter().enumerate() {
        let row_y = content_y + (i as f64 * ROW_HEIGHT);

        // Alternating background
        let bg = if i % 2 == 0 { ROW_EVEN } else { ROW_ODD };
        ctx.set_fill_color(&rgba_to_hex(bg));
        ctx.fill_rect(x, row_y, w, ROW_HEIGHT);

        let row_center_y = row_y + ROW_HEIGHT / 2.0;
        let mut col_x = x + CELL_PADDING_H;

        // Time column (format timestamp)
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        let time_str = {
            let secs = (trade.timestamp / 1000) % 86400;
            let h = secs / 3600;
            let m = (secs % 3600) / 60;
            let s = secs % 60;
            format!("{:02}:{:02}:{:02}", h, m, s)
        };
        ctx.fill_text(&time_str, col_x, row_center_y);
        col_x += COL_TIME_W;

        // Symbol column
        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&trade.symbol, col_x, row_center_y);
        col_x += COL_SYMBOL_W;

        // Side badge
        let (badge_color, badge_text) = match trade.side {
            crate::trading::trading::trade_log::OrderSide::Buy => (STATUS_FILLED, "BUY"),
            crate::trading::trading::trade_log::OrderSide::Sell => (STATUS_CANCELLED, "SELL"),
        };

        let badge_w = 50.0;
        let badge_h = 18.0;
        let badge_x = col_x;
        let badge_y = row_center_y - badge_h / 2.0;

        ctx.set_fill_color(&rgba_to_hex([badge_color[0], badge_color[1], badge_color[2], 0.15]));
        ctx.fill_rect(badge_x, badge_y, badge_w, badge_h);

        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_fill_color(&rgba_to_hex(badge_color));
        ctx.fill_text(badge_text, badge_x + badge_w / 2.0, row_center_y);

        col_x += COL_SIDE_W;

        // Price column
        ctx.set_font("13px monospace");
        ctx.set_text_align(TextAlign::Right);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&format!("{:.2}", trade.price), col_x + COL_PRICE_W - CELL_PADDING_H, row_center_y);
        col_x += COL_PRICE_W;

        // Size column
        ctx.set_font("13px monospace");
        ctx.set_text_align(TextAlign::Right);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&format!("{:.4}", trade.quantity), col_x + COL_SIZE_W - CELL_PADDING_H, row_center_y);
        col_x += COL_SIZE_W;

        // Status badge (always "FILLED" for trade log - these are executed trades)
        let status_badge_w = 70.0;
        let status_badge_h = 18.0;
        let status_badge_x = col_x + 5.0;
        let status_badge_y = row_center_y - status_badge_h / 2.0;

        ctx.set_fill_color(&rgba_to_hex([STATUS_FILLED[0], STATUS_FILLED[1], STATUS_FILLED[2], 0.15]));
        ctx.fill_rect(status_badge_x, status_badge_y, status_badge_w, status_badge_h);

        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_fill_color(&rgba_to_hex(STATUS_FILLED));
        ctx.fill_text("FILLED", status_badge_x + status_badge_w / 2.0, row_center_y);
    }

    // 4. Right border
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x + w - 1.0, y, 1.0, h);
}

// =============================================================================
// 5. POSITION MANAGER PANEL
// =============================================================================

pub fn render_position_manager_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &PositionManagerState,
) {
    // Colors - Dark Theme
    const BG_COLOR: [f32; 4] = [0.075, 0.090, 0.133, 1.0];              // #131722
    const HEADER_BG: [f32; 4] = [0.118, 0.133, 0.176, 1.0];             // #1e222d
    const HEADER_TEXT: [f32; 4] = [0.698, 0.710, 0.745, 1.0];           // #b2b5be
    const ROW_EVEN: [f32; 4] = [0.075, 0.090, 0.133, 1.0];              // #131722
    const ROW_ODD: [f32; 4] = [0.118, 0.133, 0.176, 1.0];               // #1e222d
    const TEXT_PRIMARY: [f32; 4] = [0.820, 0.831, 0.863, 1.0];          // #d1d4dc
    const POSITIVE_PNL: [f32; 4] = [0.055, 0.796, 0.506, 1.0];          // #0ecb81
    const NEGATIVE_PNL: [f32; 4] = [0.965, 0.275, 0.365, 1.0];          // #f6465d
    const LONG_BADGE_BG: [f32; 4] = [0.055, 0.796, 0.506, 0.15];        // #0ecb81 at 15% alpha
    const SHORT_BADGE_BG: [f32; 4] = [0.965, 0.275, 0.365, 0.15];       // #f6465d at 15% alpha
    const BORDER_COLOR: [f32; 4] = [0.165, 0.180, 0.224, 1.0];          // #2a2e39

    // Layout
    const HEADER_HEIGHT: f64 = 36.0;
    const ROW_HEIGHT: f64 = 48.0;
    const CELL_PADDING_H: f64 = 12.0;

    // Column widths
    const COL_SYMBOL_W: f64 = 100.0;
    const COL_SIDE_W: f64 = 60.0;
    const COL_SIZE_W: f64 = 80.0;
    const COL_ENTRY_W: f64 = 90.0;
    const COL_CURRENT_W: f64 = 90.0;
    const COL_PNL_W: f64 = 100.0;

    let x = x as f64;
    let y = y as f64;
    let w = width as f64;
    let h = height as f64;

    // 1. Background
    ctx.set_fill_color(&rgba_to_hex(BG_COLOR));
    ctx.fill_rect(x, y, w, h);

    // 2. Header bar
    ctx.set_fill_color(&rgba_to_hex(HEADER_BG));
    ctx.fill_rect(x, y, w, HEADER_HEIGHT);

    // Header text
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(HEADER_TEXT));

    let mut col_x = x + CELL_PADDING_H;
    ctx.fill_text("SYMBOL", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_SYMBOL_W;
    ctx.fill_text("SIDE", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_SIDE_W;
    ctx.fill_text("SIZE", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_SIZE_W;
    ctx.fill_text("ENTRY", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_ENTRY_W;
    ctx.fill_text("CURRENT", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_CURRENT_W;
    ctx.fill_text("PNL", col_x, y + HEADER_HEIGHT / 2.0);

    // Header separator
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x, y + HEADER_HEIGHT, w, 1.0);

    // 3. Rows
    let content_y = y + HEADER_HEIGHT + 1.0;
    let max_rows = ((h - HEADER_HEIGHT - 1.0) / ROW_HEIGHT).floor() as usize;

    for (i, pos) in state.visible_positions(0, max_rows).iter().enumerate() {
        let row_y = content_y + (i as f64 * ROW_HEIGHT);

        // Alternating background
        let bg = if i % 2 == 0 { ROW_EVEN } else { ROW_ODD };
        ctx.set_fill_color(&rgba_to_hex(bg));
        ctx.fill_rect(x, row_y, w, ROW_HEIGHT);

        // Selected row highlight
        if state.selected == Some(i) {
            ctx.set_fill_color(&rgba_to_hex([0.161, 0.384, 1.0, 0.15])); // blue highlight
            ctx.fill_rect(x, row_y, w, ROW_HEIGHT);
        }

        let row_center_y = row_y + ROW_HEIGHT / 2.0;
        let mut col_x = x + CELL_PADDING_H;

        // Symbol column
        ctx.set_font("14px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&pos.symbol, col_x, row_center_y);
        col_x += COL_SYMBOL_W;

        // Side badge (LONG/SHORT pill)
        let badge_w = 50.0;
        let badge_h = 20.0;
        let badge_x = col_x + 5.0;
        let badge_y = row_center_y - badge_h / 2.0;
        let (badge_bg, badge_text, badge_color) = match pos.side {
            crate::trading::trading::position_manager::PositionSide::Long => (LONG_BADGE_BG, "LONG", POSITIVE_PNL),
            crate::trading::trading::position_manager::PositionSide::Short => (SHORT_BADGE_BG, "SHORT", NEGATIVE_PNL),
        };

        ctx.set_fill_color(&rgba_to_hex(badge_bg));
        ctx.fill_rect(badge_x, badge_y, badge_w, badge_h);

        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_fill_color(&rgba_to_hex(badge_color));
        ctx.fill_text(badge_text, badge_x + badge_w / 2.0, row_center_y);

        col_x += COL_SIDE_W;

        // Size column
        ctx.set_font("13px monospace");
        ctx.set_text_align(TextAlign::Right);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&format!("{:.4}", pos.quantity), col_x + COL_SIZE_W - CELL_PADDING_H, row_center_y);
        col_x += COL_SIZE_W;

        // Entry price
        ctx.set_font("13px monospace");
        ctx.set_text_align(TextAlign::Right);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&format!("{:.2}", pos.entry_price), col_x + COL_ENTRY_W - CELL_PADDING_H, row_center_y);
        col_x += COL_ENTRY_W;

        // Current price (mark_price)
        ctx.set_font("13px monospace");
        ctx.set_text_align(TextAlign::Right);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&format!("{:.2}", pos.mark_price), col_x + COL_CURRENT_W - CELL_PADDING_H, row_center_y);
        col_x += COL_CURRENT_W;

        // PnL column (colored)
        let pnl_color = if pos.unrealized_pnl >= 0.0 { POSITIVE_PNL } else { NEGATIVE_PNL };
        ctx.set_font("14px monospace");
        ctx.set_text_align(TextAlign::Right);
        ctx.set_fill_color(&rgba_to_hex(pnl_color));
        let sign = if pos.unrealized_pnl >= 0.0 { "+" } else { "" };
        ctx.fill_text(&format!("{}{:.2}", sign, pos.unrealized_pnl), col_x + COL_PNL_W - CELL_PADDING_H, row_center_y);

        // Close button (right side of row)
        let close_btn_w = 52.0;
        let close_btn_h = 20.0;
        let close_btn_x = x + w - close_btn_w - 12.0;
        let close_btn_y = row_center_y - close_btn_h / 2.0;

        // Button background
        ctx.set_fill_color(&rgba_to_hex([0.965, 0.275, 0.365, 0.2])); // red tint
        ctx.fill_rounded_rect(close_btn_x, close_btn_y, close_btn_w, close_btn_h, 3.0);

        // Button text
        ctx.set_font("10px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&rgba_to_hex([0.965, 0.275, 0.365, 1.0])); // red text
        ctx.fill_text("CLOSE", close_btn_x + close_btn_w / 2.0, row_center_y);
    }

    // Footer: Total Unrealized PnL
    let footer_y = content_y + (state.visible_positions(0, max_rows).len() as f64 * ROW_HEIGHT);

    // Footer background
    ctx.set_fill_color(&rgba_to_hex(HEADER_BG));
    ctx.fill_rect(x, footer_y, w, 36.0);

    // Separator line
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x, footer_y, w, 1.0);

    // Label
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex([0.698, 0.710, 0.745, 1.0])); // header text color
    ctx.fill_text("TOTAL UNREALIZED PNL", x + 12.0, footer_y + 18.0);

    // Value
    let total_pnl_color = if state.total_unrealized_pnl >= 0.0 { POSITIVE_PNL } else { NEGATIVE_PNL };
    ctx.set_font("14px monospace");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_fill_color(&rgba_to_hex(total_pnl_color));
    let sign = if state.total_unrealized_pnl >= 0.0 { "+" } else { "" };
    ctx.fill_text(
        &format!("{}{:.2}", sign, state.total_unrealized_pnl),
        x + w - 12.0,
        footer_y + 18.0,
    );

    // 4. Right border
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x + w - 1.0, y, 1.0, h);
}

// =============================================================================
// 6. ORDER ENTRY PANEL (Form-style, not table)
// =============================================================================

pub fn render_order_entry_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &OrderEntryState,
) {
    use crate::trading::trading::order_entry::{OrderSide, OrderType, TimeInForce, OrderEntryElement};

    // Colors
    const BG_COLOR: [f32; 4] = [0.075, 0.090, 0.133, 1.0];              // #131722
    const HEADER_BG: [f32; 4] = [0.118, 0.133, 0.176, 1.0];             // #1e222d
    const INPUT_BG: [f32; 4] = [0.059, 0.067, 0.094, 1.0];              // #0f1118
    const TEXT_PRIMARY: [f32; 4] = [0.820, 0.831, 0.863, 1.0];          // #d1d4dc
    const TEXT_SECONDARY: [f32; 4] = [0.471, 0.482, 0.525, 1.0];        // #787b86
    const BUY_GREEN: [f32; 4] = [0.055, 0.796, 0.506, 1.0];             // #0ecb81
    const SELL_RED: [f32; 4] = [0.965, 0.275, 0.365, 1.0];              // #f6465d
    const ACTIVE_ACCENT: [f32; 4] = [0.161, 0.384, 1.0, 1.0];           // #2962ff
    const BORDER_COLOR: [f32; 4] = [0.165, 0.180, 0.224, 1.0];          // #2a2e39
    const DISABLED_BG: [f32; 4] = [0.118, 0.133, 0.176, 0.5];           // #1e222d at 50%
    const HOVER_BG: [f32; 4] = [1.0, 1.0, 1.0, 0.06];                   // subtle white overlay
    const EDITING_BORDER: [f32; 4] = [0.161, 0.384, 1.0, 0.8];          // blue glow for active field

    let x = x as f64;
    let y = y as f64;
    let w = width as f64;
    let h = height as f64;

    // Background
    ctx.set_fill_color(&rgba_to_hex(BG_COLOR));
    ctx.fill_rect(x, y, w, h);

    let padding = 12.0;
    let mut cy = y + padding; // Current Y position
    let button_height = 32.0;
    let small_button_height = 24.0;
    let input_height = 36.0;
    let spacing = 8.0;

    // Calculate total content height to determine if scrollbar needed
    let mut total_h = padding; // top padding
    total_h += button_height + spacing * 2.0; // BUY/SELL
    total_h += small_button_height + spacing * 2.0; // Order type
    if matches!(state.order_type, OrderType::Limit | OrderType::StopLimit) {
        total_h += input_height + spacing; // Price
    }
    if matches!(state.order_type, OrderType::StopLimit | OrderType::StopMarket) {
        total_h += input_height + spacing; // Stop price
    }
    total_h += input_height + spacing; // Quantity
    total_h += small_button_height + spacing * 2.0; // Quick qty
    total_h += small_button_height + spacing * 2.0; // TIF
    total_h += 48.0 + spacing * 2.0; // Summary
    total_h += 40.0 + spacing; // Submit
    total_h += padding; // bottom padding

    let needs_scroll = total_h > h;
    let scroll_offset = if needs_scroll { state.scroll_offset.clamp(0.0, (total_h - h).max(0.0)) } else { 0.0 };

    // Apply scroll offset
    cy -= scroll_offset;

    // 1. Side toggle - BUY/SELL buttons
    let button_w = (w - padding * 2.0 - spacing) / 2.0;

    // BUY button
    let is_buy = matches!(state.side, OrderSide::Buy);
    if is_buy {
        ctx.set_fill_color(&rgba_to_hex(BUY_GREEN));
    } else {
        ctx.set_fill_color(&rgba_to_hex(HEADER_BG));
    }
    ctx.fill_rounded_rect(x + padding, cy, button_w, button_height, 4.0);

    // BUY button border if not active
    if !is_buy {
        ctx.set_fill_color(&rgba_to_hex(BUY_GREEN));
        ctx.fill_rect(x + padding, cy, button_w, 1.0);
        ctx.fill_rect(x + padding, cy + button_height - 1.0, button_w, 1.0);
        ctx.fill_rect(x + padding, cy, 1.0, button_height);
        ctx.fill_rect(x + padding + button_w - 1.0, cy, 1.0, button_height);
    }

    // Hover overlay for BUY
    if matches!(state.hovered, Some(OrderEntryElement::BuyButton)) {
        ctx.set_fill_color(&rgba_to_hex(HOVER_BG));
        ctx.fill_rounded_rect(x + padding, cy, button_w, button_height, 4.0);
    }

    ctx.set_font("14px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("BUY", x + padding + button_w / 2.0, cy + button_height / 2.0);

    // SELL button
    let is_sell = matches!(state.side, OrderSide::Sell);
    if is_sell {
        ctx.set_fill_color(&rgba_to_hex(SELL_RED));
    } else {
        ctx.set_fill_color(&rgba_to_hex(HEADER_BG));
    }
    ctx.fill_rounded_rect(x + padding + button_w + spacing, cy, button_w, button_height, 4.0);

    // SELL button border if not active
    if !is_sell {
        ctx.set_fill_color(&rgba_to_hex(SELL_RED));
        ctx.fill_rect(x + padding + button_w + spacing, cy, button_w, 1.0);
        ctx.fill_rect(x + padding + button_w + spacing, cy + button_height - 1.0, button_w, 1.0);
        ctx.fill_rect(x + padding + button_w + spacing, cy, 1.0, button_height);
        ctx.fill_rect(x + padding + button_w + spacing + button_w - 1.0, cy, 1.0, button_height);
    }

    // Hover overlay for SELL
    if matches!(state.hovered, Some(OrderEntryElement::SellButton)) {
        ctx.set_fill_color(&rgba_to_hex(HOVER_BG));
        ctx.fill_rounded_rect(x + padding + button_w + spacing, cy, button_w, button_height, 4.0);
    }

    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("SELL", x + padding + button_w + spacing + button_w / 2.0, cy + button_height / 2.0);

    cy += button_height + spacing * 2.0;

    // 2. Order Type selector - 4 buttons
    let type_button_w = (w - padding * 2.0 - spacing * 3.0) / 4.0;
    ctx.set_font("11px sans-serif");

    let order_types = [
        (OrderType::Limit, "Limit"),
        (OrderType::Market, "Market"),
        (OrderType::StopLimit, "Stop-Limit"),
        (OrderType::StopMarket, "Stop-Market"),
    ];

    for (i, (otype, label)) in order_types.iter().enumerate() {
        let btn_x = x + padding + (type_button_w + spacing) * i as f64;
        let is_active = state.order_type == *otype;

        if is_active {
            ctx.set_fill_color(&rgba_to_hex(ACTIVE_ACCENT));
        } else {
            ctx.set_fill_color(&rgba_to_hex(HEADER_BG));
        }
        ctx.fill_rounded_rect(btn_x, cy, type_button_w, small_button_height, 3.0);

        // Hover overlay for Order Type button
        if matches!(state.hovered, Some(OrderEntryElement::OrderTypeButton(hi)) if hi == i) {
            ctx.set_fill_color(&rgba_to_hex(HOVER_BG));
            ctx.fill_rounded_rect(btn_x, cy, type_button_w, small_button_height, 3.0);
        }

        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(label, btn_x + type_button_w / 2.0, cy + small_button_height / 2.0);
    }

    cy += small_button_height + spacing * 2.0;

    // 3. Price input (only for Limit and StopLimit)
    let show_price = matches!(state.order_type, OrderType::Limit | OrderType::StopLimit);
    if show_price {
        // Input background
        ctx.set_fill_color(&rgba_to_hex(INPUT_BG));
        ctx.fill_rounded_rect(x + padding, cy, w - padding * 2.0, input_height, 4.0);

        // Editing border or hover overlay
        if matches!(state.editing_field, Some(OrderEntryElement::PriceInput)) {
            ctx.set_fill_color(&rgba_to_hex(EDITING_BORDER));
            // Draw border as 4 rects (1px)
            let ix = x + padding;
            let iw = w - padding * 2.0;
            ctx.fill_rect(ix, cy, iw, 1.0);
            ctx.fill_rect(ix, cy + input_height - 1.0, iw, 1.0);
            ctx.fill_rect(ix, cy, 1.0, input_height);
            ctx.fill_rect(ix + iw - 1.0, cy, 1.0, input_height);
        } else if matches!(state.hovered, Some(OrderEntryElement::PriceInput)) {
            ctx.set_fill_color(&rgba_to_hex(HOVER_BG));
            ctx.fill_rounded_rect(x + padding, cy, w - padding * 2.0, input_height, 4.0);
        }

        // Label
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text("Price", x + padding + 10.0, cy + input_height / 2.0);

        // Value (or inline editable text with cursor)
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        if matches!(state.editing_field, Some(OrderEntryElement::PriceInput)) {
            // Inline editing mode - draw text with cursor
            render_inline_text_field(ctx, &state.editing_text, state.editing_cursor,
                state.editing_selection, state.editing_blink_time,
                x + padding + 10.0, cy, w - padding * 2.0 - 20.0, input_height);
        } else {
            ctx.set_text_align(TextAlign::Right);
            let price_str = state.price.map(|p| format!("{:.2}", p)).unwrap_or_else(|| "0.00".to_string());
            ctx.fill_text(&price_str, x + w - padding - 10.0, cy + input_height / 2.0);
        }

        cy += input_height + spacing;
    }

    // 4. Stop Price input (only for StopLimit and StopMarket)
    let show_stop_price = matches!(state.order_type, OrderType::StopLimit | OrderType::StopMarket);
    if show_stop_price {
        // Input background
        ctx.set_fill_color(&rgba_to_hex(INPUT_BG));
        ctx.fill_rounded_rect(x + padding, cy, w - padding * 2.0, input_height, 4.0);

        // Editing border or hover overlay
        if matches!(state.editing_field, Some(OrderEntryElement::StopPriceInput)) {
            ctx.set_fill_color(&rgba_to_hex(EDITING_BORDER));
            // Draw border as 4 rects (1px)
            let ix = x + padding;
            let iw = w - padding * 2.0;
            ctx.fill_rect(ix, cy, iw, 1.0);
            ctx.fill_rect(ix, cy + input_height - 1.0, iw, 1.0);
            ctx.fill_rect(ix, cy, 1.0, input_height);
            ctx.fill_rect(ix + iw - 1.0, cy, 1.0, input_height);
        } else if matches!(state.hovered, Some(OrderEntryElement::StopPriceInput)) {
            ctx.set_fill_color(&rgba_to_hex(HOVER_BG));
            ctx.fill_rounded_rect(x + padding, cy, w - padding * 2.0, input_height, 4.0);
        }

        // Label
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text("Stop Price", x + padding + 10.0, cy + input_height / 2.0);

        // Value (or inline editable text with cursor)
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        if matches!(state.editing_field, Some(OrderEntryElement::StopPriceInput)) {
            // Inline editing mode - draw text with cursor
            render_inline_text_field(ctx, &state.editing_text, state.editing_cursor,
                state.editing_selection, state.editing_blink_time,
                x + padding + 10.0, cy, w - padding * 2.0 - 20.0, input_height);
        } else {
            ctx.set_text_align(TextAlign::Right);
            let stop_str = state.stop_price.map(|sp| format!("{:.2}", sp)).unwrap_or_else(|| "0.00".to_string());
            ctx.fill_text(&stop_str, x + w - padding - 10.0, cy + input_height / 2.0);
        }

        cy += input_height + spacing;
    }

    // 5. Quantity input
    ctx.set_fill_color(&rgba_to_hex(INPUT_BG));
    ctx.fill_rounded_rect(x + padding, cy, w - padding * 2.0, input_height, 4.0);

    // Editing border or hover overlay
    if matches!(state.editing_field, Some(OrderEntryElement::QuantityInput)) {
        ctx.set_fill_color(&rgba_to_hex(EDITING_BORDER));
        // Draw border as 4 rects (1px)
        let ix = x + padding;
        let iw = w - padding * 2.0;
        ctx.fill_rect(ix, cy, iw, 1.0);
        ctx.fill_rect(ix, cy + input_height - 1.0, iw, 1.0);
        ctx.fill_rect(ix, cy, 1.0, input_height);
        ctx.fill_rect(ix + iw - 1.0, cy, 1.0, input_height);
    } else if matches!(state.hovered, Some(OrderEntryElement::QuantityInput)) {
        ctx.set_fill_color(&rgba_to_hex(HOVER_BG));
        ctx.fill_rounded_rect(x + padding, cy, w - padding * 2.0, input_height, 4.0);
    }

    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    ctx.fill_text("Quantity", x + padding + 10.0, cy + input_height / 2.0);

    // Value (or inline editable text with cursor)
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    if matches!(state.editing_field, Some(OrderEntryElement::QuantityInput)) {
        // Inline editing mode - draw text with cursor
        render_inline_text_field(ctx, &state.editing_text, state.editing_cursor,
            state.editing_selection, state.editing_blink_time,
            x + padding + 10.0, cy, w - padding * 2.0 - 20.0, input_height);
    } else {
        ctx.set_text_align(TextAlign::Right);
        let qty_str = state.format_quantity();
        ctx.fill_text(&qty_str, x + w - padding - 10.0, cy + input_height / 2.0);
    }

    cy += input_height + spacing;

    // 6. Quick quantity buttons - 25%, 50%, 75%, 100%
    let quick_btn_w = (w - padding * 2.0 - spacing * 3.0) / 4.0;
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Center);

    let quick_labels = ["25%", "50%", "75%", "100%"];
    for (i, label) in quick_labels.iter().enumerate() {
        let btn_x = x + padding + (quick_btn_w + spacing) * i as f64;

        ctx.set_fill_color(&rgba_to_hex(HEADER_BG));
        ctx.fill_rounded_rect(btn_x, cy, quick_btn_w, small_button_height, 3.0);

        // Hover overlay for Quick Qty button
        if matches!(state.hovered, Some(OrderEntryElement::QuickQtyButton(hi)) if hi == i) {
            ctx.set_fill_color(&rgba_to_hex(HOVER_BG));
            ctx.fill_rounded_rect(btn_x, cy, quick_btn_w, small_button_height, 3.0);
        }

        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(label, btn_x + quick_btn_w / 2.0, cy + small_button_height / 2.0);
    }

    cy += small_button_height + spacing * 2.0;

    // 7. Time in Force buttons - GTC | IOC | FOK
    let tif_btn_w = (w - padding * 2.0 - spacing * 2.0) / 3.0;
    ctx.set_font("11px sans-serif");

    let tifs = [
        (TimeInForce::GTC, "GTC"),
        (TimeInForce::IOC, "IOC"),
        (TimeInForce::FOK, "FOK"),
    ];

    for (i, (tif, label)) in tifs.iter().enumerate() {
        let btn_x = x + padding + (tif_btn_w + spacing) * i as f64;
        let is_active = state.time_in_force == *tif;

        if is_active {
            ctx.set_fill_color(&rgba_to_hex(ACTIVE_ACCENT));
        } else {
            ctx.set_fill_color(&rgba_to_hex(HEADER_BG));
        }
        ctx.fill_rounded_rect(btn_x, cy, tif_btn_w, small_button_height, 3.0);

        // Hover overlay for TIF button
        if matches!(state.hovered, Some(OrderEntryElement::TifButton(hi)) if hi == i) {
            ctx.set_fill_color(&rgba_to_hex(HOVER_BG));
            ctx.fill_rounded_rect(btn_x, cy, tif_btn_w, small_button_height, 3.0);
        }

        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(label, btn_x + tif_btn_w / 2.0, cy + small_button_height / 2.0);
    }

    cy += small_button_height + spacing * 2.0;

    // 8. Estimated summary - Cost, Fee, Balance after
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));

    let summary_y = cy;
    ctx.fill_text("Est. Cost:", x + padding, summary_y);
    ctx.set_text_align(TextAlign::Right);
    ctx.fill_text(&state.format_estimated_cost(), x + w - padding, summary_y);

    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text("Est. Fee:", x + padding, summary_y + 16.0);
    ctx.set_text_align(TextAlign::Right);
    ctx.fill_text(&state.format_estimated_fee(), x + w - padding, summary_y + 16.0);

    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text("Balance After:", x + padding, summary_y + 32.0);
    ctx.set_text_align(TextAlign::Right);
    let balance_str = format!("${:.2}", state.post_order_balance);
    ctx.fill_text(&balance_str, x + w - padding, summary_y + 32.0);

    cy += 48.0 + spacing * 2.0;

    // 9. Submit button - Full width
    let is_valid = state.is_valid() && !state.submitting;

    if is_valid {
        if is_buy {
            ctx.set_fill_color(&rgba_to_hex(BUY_GREEN));
        } else {
            ctx.set_fill_color(&rgba_to_hex(SELL_RED));
        }
    } else {
        ctx.set_fill_color(&rgba_to_hex(DISABLED_BG));
    }

    let submit_height = 40.0;
    ctx.fill_rounded_rect(x + padding, cy, w - padding * 2.0, submit_height, 4.0);

    // Hover overlay for Submit button (only if valid)
    if matches!(state.hovered, Some(OrderEntryElement::SubmitButton)) && is_valid {
        ctx.set_fill_color(&rgba_to_hex(HOVER_BG));
        ctx.fill_rounded_rect(x + padding, cy, w - padding * 2.0, submit_height, 4.0);
    }

    ctx.set_font("14px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));

    let btn_text = if state.submitting {
        "SUBMITTING...".to_string()
    } else {
        let side_str = if is_buy { "BUY" } else { "SELL" };
        format!("{} {}", side_str, state.symbol)
    };
    ctx.fill_text(&btn_text, x + w / 2.0, cy + submit_height / 2.0);

    cy += submit_height + spacing;

    // 10. Validation errors - Red text at bottom
    if !state.errors.is_empty() {
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.set_fill_color(&rgba_to_hex(SELL_RED));

        for (i, error) in state.errors.iter().enumerate() {
            ctx.fill_text(error, x + padding, cy + (i as f64 * 14.0));
        }
    }

    // Scrollbar (if content overflows)
    if needs_scroll {
        let scrollbar_w = 6.0;
        let scrollbar_x = x + w - scrollbar_w - 2.0;
        let ratio = scroll_offset / (total_h - h).max(1.0);
        render_scrollbar_vertical(ctx, scrollbar_x, y, scrollbar_w, h, ratio);
    }

    // Right border
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x + w - 1.0, y, 1.0, h);
}

// =============================================================================
// 7. ACCOUNT SUMMARY PANEL
// =============================================================================

pub fn render_account_summary_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &AccountSummaryState,
) {
    // Colors
    const BG_COLOR: [f32; 4] = [0.075, 0.090, 0.133, 1.0];              // #131722
    const CARD_BG: [f32; 4] = [0.118, 0.133, 0.176, 1.0];               // #1e222d
    const TEXT_PRIMARY: [f32; 4] = [0.820, 0.831, 0.863, 1.0];          // #d1d4dc
    const TEXT_SECONDARY: [f32; 4] = [0.471, 0.482, 0.525, 1.0];        // #787b86
    const POSITIVE: [f32; 4] = [0.055, 0.796, 0.506, 1.0];              // #0ecb81
    const NEGATIVE: [f32; 4] = [0.965, 0.275, 0.365, 1.0];              // #f6465d
    const BORDER_COLOR: [f32; 4] = [0.165, 0.180, 0.224, 1.0];          // #2a2e39

    let x = x as f64;
    let y = y as f64;
    let w = width as f64;
    let h = height as f64;

    // Background
    ctx.set_fill_color(&rgba_to_hex(BG_COLOR));
    ctx.fill_rect(x, y, w, h);

    // Summary cards in grid layout
    let card_h = 80.0;
    let card_gap = 12.0;
    let mut card_y = y + 20.0;

    // Balance card
    ctx.set_fill_color(&rgba_to_hex(CARD_BG));
    ctx.fill_rect(x + 12.0, card_y, w - 24.0, card_h);

    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    ctx.fill_text("TOTAL EQUITY", x + 24.0, card_y + 20.0);

    ctx.set_font("24px monospace");
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text(&format!("${:.2}", state.total_equity_usd), x + 24.0, card_y + 50.0);

    card_y += card_h + card_gap;

    // PnL card
    ctx.set_fill_color(&rgba_to_hex(CARD_BG));
    ctx.fill_rect(x + 12.0, card_y, w - 24.0, card_h);

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    ctx.fill_text("UNREALIZED PNL", x + 24.0, card_y + 20.0);

    let pnl_color = if state.unrealized_pnl >= 0.0 { POSITIVE } else { NEGATIVE };
    ctx.set_font("24px monospace");
    ctx.set_fill_color(&rgba_to_hex(pnl_color));
    let sign = if state.unrealized_pnl >= 0.0 { "+" } else { "" };
    ctx.fill_text(&format!("{}{:.2}", sign, state.unrealized_pnl), x + 24.0, card_y + 50.0);

    // Border
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x + w - 1.0, y, 1.0, h);
}

// =============================================================================
// 8. OPTIONS CHAIN PANEL
// =============================================================================

pub fn render_options_chain_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &OptionsChainState,
) {
    // Colors
    const BG_COLOR: [f32; 4] = [0.075, 0.090, 0.133, 1.0];              // #131722
    const HEADER_BG: [f32; 4] = [0.118, 0.133, 0.176, 1.0];             // #1e222d
    const HEADER_TEXT: [f32; 4] = [0.698, 0.710, 0.745, 1.0];           // #b2b5be
    const CALL_BG: [f32; 4] = [0.055, 0.796, 0.506, 0.05];              // #0ecb81 at 5% alpha
    const PUT_BG: [f32; 4] = [0.965, 0.275, 0.365, 0.05];               // #f6465d at 5% alpha
    const TEXT_PRIMARY: [f32; 4] = [0.820, 0.831, 0.863, 1.0];          // #d1d4dc
    const BORDER_COLOR: [f32; 4] = [0.165, 0.180, 0.224, 1.0];          // #2a2e39

    // Layout
    const HEADER_HEIGHT: f64 = 32.0;
    const ROW_HEIGHT: f64 = 32.0;

    let x = x as f64;
    let y = y as f64;
    let w = width as f64;
    let h = height as f64;

    // Background
    ctx.set_fill_color(&rgba_to_hex(BG_COLOR));
    ctx.fill_rect(x, y, w, h);

    // Header
    ctx.set_fill_color(&rgba_to_hex(HEADER_BG));
    ctx.fill_rect(x, y, w, HEADER_HEIGHT);

    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(HEADER_TEXT));
    ctx.fill_text("CALLS", x + w * 0.25, y + HEADER_HEIGHT / 2.0);
    ctx.fill_text("STRIKE", x + w * 0.5, y + HEADER_HEIGHT / 2.0);
    ctx.fill_text("PUTS", x + w * 0.75, y + HEADER_HEIGHT / 2.0);

    // Header separator
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x, y + HEADER_HEIGHT, w, 1.0);

    // Rows
    let content_y = y + HEADER_HEIGHT + 1.0;
    let max_rows = ((h - HEADER_HEIGHT - 1.0) / ROW_HEIGHT).floor() as usize;

    for (i, contract) in state.visible_contracts(0, max_rows).iter().enumerate() {
        let row_y = content_y + (i as f64 * ROW_HEIGHT);

        // Determine background color based on option type
        match contract.option_type {
            crate::info::options::options_chain::OptionType::Call => {
                ctx.set_fill_color(&rgba_to_hex(CALL_BG));
                ctx.fill_rect(x, row_y, w / 2.0, ROW_HEIGHT);
            }
            crate::info::options::options_chain::OptionType::Put => {
                ctx.set_fill_color(&rgba_to_hex(PUT_BG));
                ctx.fill_rect(x + w / 2.0, row_y, w / 2.0, ROW_HEIGHT);
            }
        }

        // Strike price (center)
        ctx.set_font("13px monospace");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&format!("{:.2}", contract.strike), x + w / 2.0, row_y + ROW_HEIGHT / 2.0);
    }

    // Border
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x + w - 1.0, y, 1.0, h);
}

// =============================================================================
// 9. ECONOMIC CALENDAR PANEL
// =============================================================================

pub fn render_economic_calendar_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &EconomicCalendarState,
) {
    // Colors
    const BG_COLOR: [f32; 4] = [0.075, 0.090, 0.133, 1.0];              // #131722
    const HEADER_BG: [f32; 4] = [0.118, 0.133, 0.176, 1.0];             // #1e222d
    const HEADER_TEXT: [f32; 4] = [0.698, 0.710, 0.745, 1.0];           // #b2b5be
    const ROW_EVEN: [f32; 4] = [0.075, 0.090, 0.133, 1.0];              // #131722
    const ROW_ODD: [f32; 4] = [0.118, 0.133, 0.176, 1.0];               // #1e222d
    const TEXT_PRIMARY: [f32; 4] = [0.820, 0.831, 0.863, 1.0];          // #d1d4dc
    const TEXT_SECONDARY: [f32; 4] = [0.471, 0.482, 0.525, 1.0];        // #787b86
    const HIGH_IMPACT: [f32; 4] = [0.965, 0.275, 0.365, 1.0];           // #f6465d
    const MEDIUM_IMPACT: [f32; 4] = [1.0, 0.647, 0.0, 1.0];             // #ffa500
    const LOW_IMPACT: [f32; 4] = [0.471, 0.482, 0.525, 1.0];            // #787b86
    const BORDER_COLOR: [f32; 4] = [0.165, 0.180, 0.224, 1.0];          // #2a2e39

    // Layout
    const HEADER_HEIGHT: f64 = 32.0;
    const ROW_HEIGHT: f64 = 40.0;
    const CELL_PADDING_H: f64 = 10.0;

    // Column widths
    const COL_TIME_W: f64 = 80.0;
    const COL_COUNTRY_W: f64 = 60.0;
    const COL_EVENT_W: f64 = 200.0;

    let x = x as f64;
    let y = y as f64;
    let w = width as f64;
    let h = height as f64;

    // Background
    ctx.set_fill_color(&rgba_to_hex(BG_COLOR));
    ctx.fill_rect(x, y, w, h);

    // Header
    ctx.set_fill_color(&rgba_to_hex(HEADER_BG));
    ctx.fill_rect(x, y, w, HEADER_HEIGHT);

    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(HEADER_TEXT));

    let mut col_x = x + CELL_PADDING_H;
    ctx.fill_text("TIME", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_TIME_W;
    ctx.fill_text("COUNTRY", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_COUNTRY_W;
    ctx.fill_text("EVENT", col_x, y + HEADER_HEIGHT / 2.0);
    col_x += COL_EVENT_W;
    ctx.fill_text("IMPACT", col_x, y + HEADER_HEIGHT / 2.0);

    // Header separator
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x, y + HEADER_HEIGHT, w, 1.0);

    // Rows
    let content_y = y + HEADER_HEIGHT + 1.0;
    let max_rows = ((h - HEADER_HEIGHT - 1.0) / ROW_HEIGHT).floor() as usize;

    for (i, event) in state.visible_events(0, max_rows).iter().enumerate() {
        let row_y = content_y + (i as f64 * ROW_HEIGHT);

        // Alternating background
        let bg = if i % 2 == 0 { ROW_EVEN } else { ROW_ODD };
        ctx.set_fill_color(&rgba_to_hex(bg));
        ctx.fill_rect(x, row_y, w, ROW_HEIGHT);

        let row_center_y = row_y + ROW_HEIGHT / 2.0;
        let mut col_x = x + CELL_PADDING_H;

        // Time column (format timestamp)
        ctx.set_font("12px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        let time_str = {
            let secs = (event.timestamp / 1000) % 86400;
            let h = secs / 3600;
            let m = (secs % 3600) / 60;
            format!("{:02}:{:02}", h, m)
        };
        ctx.fill_text(&time_str, col_x, row_center_y);
        col_x += COL_TIME_W;

        // Country column
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&event.country, col_x, row_center_y);
        col_x += COL_COUNTRY_W;

        // Event name
        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&event.event_name, col_x, row_center_y);
        col_x += COL_EVENT_W;

        // Impact indicator
        let (impact_color, impact_level) = match event.impact {
            crate::info::calendar::economic_calendar::EventImpact::High => (HIGH_IMPACT, 3),
            crate::info::calendar::economic_calendar::EventImpact::Medium => (MEDIUM_IMPACT, 2),
            crate::info::calendar::economic_calendar::EventImpact::Low => (LOW_IMPACT, 1),
        };
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&rgba_to_hex(impact_color));
        ctx.fill_text(&"●".repeat(impact_level), col_x, row_center_y);
    }

    // Border
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x + w - 1.0, y, 1.0, h);
}

// =============================================================================
// 10. NEWS FEED PANEL
// =============================================================================

pub fn render_news_feed_panel(
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &NewsState,
) {
    // Colors
    const BG_COLOR: [f32; 4] = [0.075, 0.090, 0.133, 1.0];              // #131722
    const ITEM_BG: [f32; 4] = [0.118, 0.133, 0.176, 1.0];               // #1e222d
    const TEXT_PRIMARY: [f32; 4] = [0.820, 0.831, 0.863, 1.0];          // #d1d4dc
    const TEXT_SECONDARY: [f32; 4] = [0.471, 0.482, 0.525, 1.0];        // #787b86
    const BORDER_COLOR: [f32; 4] = [0.165, 0.180, 0.224, 1.0];          // #2a2e39

    // Layout
    const ITEM_HEIGHT: f64 = 80.0;
    const ITEM_GAP: f64 = 8.0;
    const PADDING: f64 = 12.0;

    let x = x as f64;
    let y = y as f64;
    let w = width as f64;
    let h = height as f64;

    // Background
    ctx.set_fill_color(&rgba_to_hex(BG_COLOR));
    ctx.fill_rect(x, y, w, h);

    // News items (card-style layout)
    let max_items = ((h - PADDING) / (ITEM_HEIGHT + ITEM_GAP)).floor() as usize;
    let mut item_y = y + PADDING;

    for (i, article) in state.visible_news(max_items).iter().enumerate() {
        if i > 0 {
            item_y += ITEM_GAP;
        }

        // Item card background
        ctx.set_fill_color(&rgba_to_hex(ITEM_BG));
        ctx.fill_rect(x + PADDING, item_y, w - 2.0 * PADDING, ITEM_HEIGHT);

        // Title (headline)
        ctx.set_font("14px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        // Truncate long headlines
        let headline = if article.headline.len() > 60 {
            format!("{}...", &article.headline[..60])
        } else {
            article.headline.clone()
        };
        ctx.fill_text(&headline, x + PADDING + 10.0, item_y + 10.0);

        // Source and time
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        let time_str = {
            let secs = (article.timestamp / 1000) % 86400;
            let h = secs / 3600;
            let m = (secs % 3600) / 60;
            format!("{:02}:{:02}", h, m)
        };
        ctx.fill_text(&format!("{} • {}", article.source, time_str), x + PADDING + 10.0, item_y + 35.0);

        // Category
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        let category = format!("{:?}", article.category);
        ctx.fill_text(&category, x + PADDING + 10.0, item_y + 55.0);

        item_y += ITEM_HEIGHT;
    }

    // Border
    ctx.set_fill_color(&rgba_to_hex(BORDER_COLOR));
    ctx.fill_rect(x + w - 1.0, y, 1.0, h);
}

// =============================================================================
// HELPER: Text Field with Cursor Rendering (for Order Entry)
// =============================================================================

/// Render a text input field with cursor (for Order Entry editable fields)
/// Render an inline editable text field with cursor and selection
fn render_inline_text_field(
    ctx: &mut dyn RenderContext,
    text: &str,
    cursor: usize,
    selection: Option<usize>,
    _blink_time: u64,
    field_x: f64,
    field_y: f64,
    field_w: f64,
    field_h: f64,
) {
    let char_width = 7.2; // Approximate monospace char width at 12px
    let text_len = text.len() as f64;

    // Right-align text within the field (matching non-editing right-aligned layout)
    let text_right = field_x + field_w;
    let text_start_x = text_right - text_len * char_width;

    // Draw selection highlight if any
    if let Some(sel_start) = selection {
        let start = sel_start.min(cursor);
        let end = sel_start.max(cursor);
        let sel_x = text_start_x + start as f64 * char_width;
        let sel_w = (end - start) as f64 * char_width;
        ctx.set_fill_color("#2962ff40"); // Blue selection
        ctx.fill_rect(sel_x, field_y + 4.0, sel_w, field_h - 8.0);
    }

    // Draw text
    ctx.set_font("12px monospace");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color("#d1d4dc");
    ctx.fill_text(text, text_start_x, field_y + field_h / 2.0);

    // Draw cursor
    let cursor_x = text_start_x + cursor as f64 * char_width;
    ctx.set_fill_color("#d1d4dc");
    ctx.fill_rect(cursor_x, field_y + 6.0, 1.5, field_h - 12.0);
}
