//! Panel renderers for UTILITY and MISCELLANEOUS panels
//!
//! This module contains render functions for 26 panel types.
//! Currently these are simplified stub implementations that will be expanded
//! as the State structs gain more helper methods.

use crate::render::{RenderContext, TextAlign, TextBaseline};
use crate::info::calculator::CalculatorState;
use crate::info::notes::NotesState;
use crate::info::journal::JournalState;
use crate::info::connection_status::ConnectionStatusState;
use crate::info::symbol_info::SymbolInfoState;
use crate::info::market_overview::MarketOverviewState;
use crate::info::alert_manager::AlertManagerState;
use crate::info::session_info::SessionInfoState;
use crate::info::screener::ScreenerState;
use crate::info::statistics::StatisticsState;
use crate::info::market_replay::MarketReplayState;
use crate::info::timeline::TimelineState;
use crate::info::graph::GraphState;
use crate::info::portfolio_overview::PortfolioOverviewState;
use crate::info::transaction_history::TransactionHistoryState;
use crate::info::risk_metrics::RiskMetricsState;
use crate::info::earnings_calendar::EarningsCalendarState;
use crate::info::dividend_calendar::DividendCalendarState;
use crate::info::options_expiry::OptionsExpiryState;
use crate::info::ipo_calendar::IpoCalendarState;
use crate::info::rss_feed::RssFeedState;
use crate::info::social_sentiment::SocialSentimentState;
use crate::info::analyst_ratings::AnalystRatingsState;
use crate::info::sec_filings::SecFilingsState;
use crate::info::greeks_panel::GreeksPanelState;
use crate::info::option_flow::OptionFlowState;
use crate::info::reference::table::TableState;
use crate::trading::trading::risk_calculator::RiskCalculatorState;

// Color palette (dark theme)
const BG_PANEL: [f32; 4] = [0.12, 0.12, 0.14, 1.0];
const BG_SECTION: [f32; 4] = [0.14, 0.14, 0.16, 1.0];
const BG_ELEMENT: [f32; 4] = [0.18, 0.18, 0.19, 1.0];

const TEXT_PRIMARY: [f32; 4] = [0.83, 0.83, 0.83, 1.0];
const TEXT_SECONDARY: [f32; 4] = [0.52, 0.52, 0.52, 1.0];
const TEXT_ACCENT: [f32; 4] = [0.31, 0.76, 1.0, 1.0];

const GREEN: [f32; 4] = [0.54, 0.82, 0.52, 1.0];
const RED: [f32; 4] = [0.96, 0.53, 0.44, 1.0];
const BLUE: [f32; 4] = [0.0, 0.48, 0.8, 1.0];

/// Convert RGBA array [0.0-1.0] to hex color string
fn rgba_to_hex(rgba: [f32; 4]) -> String {
    let r = (rgba[0].clamp(0.0, 1.0) * 255.0) as u8;
    let g = (rgba[1].clamp(0.0, 1.0) * 255.0) as u8;
    let b = (rgba[2].clamp(0.0, 1.0) * 255.0) as u8;
    let a = (rgba[3].clamp(0.0, 1.0) * 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", r, g, b, a)
}

/// Helper to render key-value rows
fn render_key_value_rows(
    ctx: &mut dyn RenderContext,
    rows: &[(&str, String)],
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    let row_h = 32.0;
    let gap = 8.0;
    let mut current_y = y + 16.0;

    for (key, value) in rows {
        if current_y + row_h > y + h {
            break;
        }

        ctx.set_font("13px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(key, (x + 16.0) as f64, (current_y + row_h / 2.0) as f64);

        ctx.set_font("14px monospace");
        ctx.set_text_align(TextAlign::Right);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(value, (x + w - 16.0) as f64, (current_y + row_h / 2.0) as f64);

        current_y += row_h + gap;
    }
}

// ====================
// UTILITY PANELS (4)
// ====================

pub fn render_calculator_panel(
    ctx: &mut dyn RenderContext,
    state: &CalculatorState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let display_h = 80.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, display_h as f64);

    ctx.set_font("14px sans-serif");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    ctx.fill_text(state.mode_label(), (x + w - 16.0) as f64, (y + 12.0) as f64);

    ctx.set_font("32px monospace");
    ctx.set_text_baseline(TextBaseline::Bottom);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text(state.display_value(), (x + w - 16.0) as f64, (y + display_h - 12.0) as f64);

    let rows: Vec<(&str, String)> = state.result_fields();
    render_key_value_rows(ctx, &rows, x, y + display_h, w, h - display_h);
}

pub fn render_notes_panel(
    ctx: &mut dyn RenderContext,
    state: &NotesState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let toolbar_h = 40.0;
    ctx.set_fill_color(&rgba_to_hex(BG_ELEMENT));
    ctx.fill_rect(x as f64, y as f64, w as f64, toolbar_h as f64);

    ctx.set_font("14px monospace");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));

    let text = state.visible_text();
    let line_height = 22.0;
    let padding = 16.0;
    let mut line_y = y + toolbar_h + padding;

    for line in text.lines().take(20) {
        if line_y + line_height > y + h - 24.0 {
            break;
        }
        ctx.fill_text(line, (x + padding) as f64, line_y as f64);
        line_y += line_height;
    }

    let status_y = y + h - 24.0;
    ctx.set_fill_color(&rgba_to_hex(BG_ELEMENT));
    ctx.fill_rect(x as f64, status_y as f64, w as f64, 24.0);

    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    let word_count = state.word_count();
    ctx.fill_text(&format!("Words: {}", word_count), (x + w - 8.0) as f64, (status_y + 12.0) as f64);
}

pub fn render_journal_panel(
    ctx: &mut dyn RenderContext,
    state: &JournalState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 40.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("16px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("Trading Journal", (x + 16.0) as f64, (y + header_h / 2.0) as f64);

    let entry_count = state.visible_entries().len();
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    ctx.fill_text(
        &format!("{} entries", entry_count),
        (x + w - 16.0) as f64,
        (y + header_h / 2.0) as f64,
    );

    // Render entries
    let entries = state.visible_entries();
    let row_h = 24.0;
    let padding = 12.0;
    let mut cy = y + header_h + padding;

    ctx.set_font("12px monospace");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);

    for entry in entries.iter().take(30) {
        if cy + row_h > y + h {
            break;
        }

        // Date column
        let date_x = x + padding;
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        let date = format_ts_date(entry.created_at);
        ctx.fill_text(&date, date_x as f64, cy as f64);

        // Symbol column
        let symbol_x = x + padding + 70.0;
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&entry.symbol, symbol_x as f64, cy as f64);

        // Direction column
        let dir_x = x + padding + 150.0;
        let dir_color = match entry.direction {
            crate::info::utility::journal::TradeDirection::Long => GREEN,
            crate::info::utility::journal::TradeDirection::Short => RED,
        };
        ctx.set_fill_color(&rgba_to_hex(dir_color));
        let dir_str = match entry.direction {
            crate::info::utility::journal::TradeDirection::Long => "LONG",
            crate::info::utility::journal::TradeDirection::Short => "SHORT",
        };
        ctx.fill_text(dir_str, dir_x as f64, cy as f64);

        // PnL column (if available)
        if let Some(pnl) = entry.pnl {
            let pnl_x = x + padding + 210.0;
            let pnl_color = if pnl > 0.0 { GREEN } else if pnl < 0.0 { RED } else { TEXT_SECONDARY };
            ctx.set_fill_color(&rgba_to_hex(pnl_color));
            ctx.fill_text(&format!("{:+.2}", pnl), pnl_x as f64, cy as f64);
        }

        // Tags (first tag only)
        if !entry.tags.is_empty() {
            let tag_x = x + w - padding - 80.0;
            ctx.set_fill_color(&rgba_to_hex(BLUE));
            ctx.set_font("11px sans-serif");
            ctx.fill_text(&format!("#{}", entry.tags[0]), tag_x as f64, cy as f64);
            ctx.set_font("12px monospace");
        }

        cy += row_h;
    }
}

fn format_ts_date(ts: i64) -> String {
    let secs = (ts / 1000) % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    format!("{:02}:{:02}", h, m)
}

pub fn render_connection_status_panel(
    ctx: &mut dyn RenderContext,
    state: &ConnectionStatusState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 40.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("16px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("Connections", (x + 16.0) as f64, (y + header_h / 2.0) as f64);

    // Overall health status
    let (health_label, health_color) = state.overall_health();
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_fill_color(&rgba_to_hex(health_color));
    ctx.fill_text(health_label, (x + w - 16.0) as f64, (y + header_h / 2.0) as f64);

    // Render connection rows
    let rows = state.connection_rows();
    let row_h = 28.0;
    let padding = 12.0;
    let mut cy = y + header_h + padding;

    for row in rows.iter() {
        if cy + row_h > y + h {
            break;
        }

        // Connection name
        ctx.set_font("12px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&row.name, (x + padding) as f64, (cy + row_h / 2.0) as f64);

        // Status
        let status_x = x + w - padding - 140.0;
        ctx.set_fill_color(&rgba_to_hex(row.color));
        ctx.fill_text(&row.status, status_x as f64, (cy + row_h / 2.0) as f64);

        // Latency
        let latency_x = x + w - padding - 60.0;
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.set_font("11px monospace");
        ctx.fill_text(&row.latency, latency_x as f64, (cy + row_h / 2.0) as f64);

        cy += row_h;
    }
}

// ====================
// REFERENCE PANELS (5)
// ====================

pub fn render_symbol_info_panel(
    ctx: &mut dyn RenderContext,
    state: &SymbolInfoState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 48.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("20px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text(&state.symbol, (x + 16.0) as f64, (y + header_h / 2.0) as f64);

    render_key_value_rows(ctx, &state.info_rows(), x, y + header_h, w, h - header_h);
}

pub fn render_market_overview_panel(
    ctx: &mut dyn RenderContext,
    state: &MarketOverviewState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 40.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("16px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("Market Overview", (x + 16.0) as f64, (y + header_h / 2.0) as f64);

    let padding = 12.0;
    let mut cy = y + header_h + 16.0;
    let row_h = 24.0;

    ctx.set_font("12px monospace");
    ctx.set_text_baseline(TextBaseline::Top);

    // Render indices section
    if !state.indices.is_empty() {
        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text("Indices", (x + padding) as f64, cy as f64);
        cy += 24.0;

        ctx.set_font("12px monospace");
        for idx in state.indices.values() {
            if cy + row_h > y + h {
                break;
            }

            ctx.set_text_align(TextAlign::Left);
            ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
            ctx.fill_text(&idx.symbol, (x + padding + 8.0) as f64, cy as f64);

            ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
            ctx.fill_text(&format!("{:.2}", idx.price), (x + padding + 100.0) as f64, cy as f64);

            let change_color = state.change_color(idx.change_percent);
            ctx.set_fill_color(&rgba_to_hex(change_color));
            ctx.fill_text(&format!("{:+.2}%", idx.change_percent), (x + padding + 180.0) as f64, cy as f64);

            cy += row_h;
        }
        cy += 8.0;
    }

    // Render top gainers section
    if !state.top_gainers.is_empty() && cy + 50.0 < y + h {
        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text("Top Gainers", (x + padding) as f64, cy as f64);
        cy += 24.0;

        ctx.set_font("12px monospace");
        for gainer in state.top_gainers.iter().take(3) {
            if cy + row_h > y + h {
                break;
            }

            ctx.set_text_align(TextAlign::Left);
            ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
            ctx.fill_text(&gainer.symbol, (x + padding + 8.0) as f64, cy as f64);

            ctx.set_fill_color(&rgba_to_hex(GREEN));
            ctx.fill_text(&format!("{:+.2}%", gainer.change_percent), (x + padding + 100.0) as f64, cy as f64);

            cy += row_h;
        }
    }
}

pub fn render_alert_manager_panel(
    ctx: &mut dyn RenderContext,
    state: &AlertManagerState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 40.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("16px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("Alerts", (x + 16.0) as f64, (y + header_h / 2.0) as f64);

    let alert_count = state.visible_alerts(0, 100).len();
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    ctx.fill_text(
        &format!("{} alerts", alert_count),
        (x + w - 16.0) as f64,
        (y + header_h / 2.0) as f64,
    );

    // Render alerts
    let alerts = state.visible_alerts(0, 50);
    let row_h = 24.0;
    let padding = 12.0;
    let mut cy = y + header_h + padding;

    ctx.set_font("12px monospace");
    ctx.set_text_baseline(TextBaseline::Top);

    for alert in alerts {
        if cy + row_h > y + h {
            break;
        }

        let (symbol, condition, status_str, _created) = state.format_alert(alert);

        // Symbol
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&symbol, (x + padding) as f64, cy as f64);

        // Condition
        let cond_x = x + padding + 90.0;
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.set_font("11px monospace");
        ctx.fill_text(&condition, cond_x as f64, cy as f64);
        ctx.set_font("12px monospace");

        // Status
        let status_x = x + w - padding - 90.0;
        let status_color = state.status_color(alert);
        ctx.set_fill_color(&rgba_to_hex(status_color));
        ctx.set_font("11px sans-serif");
        ctx.fill_text(&status_str, status_x as f64, cy as f64);
        ctx.set_font("12px monospace");

        cy += row_h;
    }
}

pub fn render_session_info_panel(
    ctx: &mut dyn RenderContext,
    state: &SessionInfoState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 40.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("16px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("Session Info", (x + 16.0) as f64, (y + header_h / 2.0) as f64);

    // Render sessions
    let sessions = state.visible_sessions();
    let padding = 12.0;
    let row_h = 32.0;
    let mut cy = y + header_h + 12.0;

    ctx.set_font("12px monospace");
    ctx.set_text_baseline(TextBaseline::Top);

    for session in sessions.iter() {
        if cy + row_h > y + h {
            break;
        }

        let (name, _exchange, status, time) = state.format_session(session);

        // Market name
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&name, (x + padding) as f64, cy as f64);

        // Status with color
        let status_color = state.status_color(session);
        ctx.set_fill_color(&rgba_to_hex(status_color));
        ctx.fill_text(&status, (x + padding + 120.0) as f64, cy as f64);

        // Time
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.set_font("11px monospace");
        ctx.fill_text(&time, (x + padding) as f64, (cy + 16.0) as f64);
        ctx.set_font("12px monospace");

        cy += row_h;
    }
}

pub fn render_screener_panel(
    ctx: &mut dyn RenderContext,
    state: &ScreenerState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let filter_h = 60.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, filter_h as f64);

    ctx.set_font("16px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("Screener", (x + 16.0) as f64, (y + 12.0) as f64);

    // Show active filters
    if !state.filters.is_empty() {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        let filter_summary = format!("{} filter(s) active", state.filters.len());
        ctx.fill_text(&filter_summary, (x + 16.0) as f64, (y + 36.0) as f64);
    }

    let result_count = state.visible_results(0, 100).len();
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    ctx.fill_text(
        &format!("{} results", result_count),
        (x + w - 16.0) as f64,
        (y + 30.0) as f64,
    );

    // Table header
    let table_y = y + filter_h;
    let header_h = 28.0;
    ctx.set_fill_color(&rgba_to_hex(BG_ELEMENT));
    ctx.fill_rect(x as f64, table_y as f64, w as f64, header_h as f64);

    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));

    let padding = 12.0;
    ctx.fill_text("Symbol", (x + padding) as f64, (table_y + header_h / 2.0) as f64);
    ctx.fill_text("Price", (x + padding + 100.0) as f64, (table_y + header_h / 2.0) as f64);
    ctx.fill_text("Change %", (x + padding + 180.0) as f64, (table_y + header_h / 2.0) as f64);
    ctx.fill_text("Volume", (x + padding + 260.0) as f64, (table_y + header_h / 2.0) as f64);

    // Render results
    let results = state.visible_results(0, 50);
    let row_h = 24.0;
    let mut cy = table_y + header_h + 4.0;

    ctx.set_font("12px monospace");
    ctx.set_text_baseline(TextBaseline::Top);

    for result in results {
        if cy + row_h > y + h {
            break;
        }

        // Symbol
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&result.symbol, (x + padding) as f64, cy as f64);

        // Price
        let price_str = format!("${:.2}", result.price);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(&price_str, (x + padding + 100.0) as f64, cy as f64);

        // Change %
        let change_str = format!("{:+.2}%", result.change_percent);
        let change_color = state.change_color(result);
        ctx.set_fill_color(&rgba_to_hex(change_color));
        ctx.fill_text(&change_str, (x + padding + 180.0) as f64, cy as f64);

        // Volume
        let volume_str = format_large_volume(result.volume as u64);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(&volume_str, (x + padding + 260.0) as f64, cy as f64);

        cy += row_h;
    }
}

fn format_large_volume(v: u64) -> String {
    if v > 1_000_000_000 {
        format!("{:.1}B", v as f64 / 1_000_000_000.0)
    } else if v > 1_000_000 {
        format!("{:.1}M", v as f64 / 1_000_000.0)
    } else if v > 1_000 {
        format!("{:.1}K", v as f64 / 1_000.0)
    } else {
        format!("{}", v)
    }
}

// ====================
// ANALYTICS PANELS (5)
// ====================

pub fn render_statistics_panel(
    ctx: &mut dyn RenderContext,
    state: &StatisticsState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    render_key_value_rows(ctx, &state.stats_rows(), x, y, w, h);
}

pub fn render_risk_metrics_panel(
    ctx: &mut dyn RenderContext,
    state: &RiskMetricsState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    if state.metrics.is_some() {
        render_key_value_rows(ctx, &state.metrics_rows(), x, y, w, h);
    } else {
        ctx.set_font("14px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text("No metrics available", (x + w / 2.0) as f64, (y + h / 2.0) as f64);
    }
}

pub fn render_market_replay_panel(
    ctx: &mut dyn RenderContext,
    state: &MarketReplayState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 40.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("16px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("Market Replay", (x + 16.0) as f64, (y + header_h / 2.0) as f64);

    let padding = 16.0;
    let mut cy = y + header_h + 24.0;

    // Playback state
    ctx.set_font("14px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    let state_label = match state.playback_state {
        crate::info::market_replay::PlaybackState::Stopped => "Stopped",
        crate::info::market_replay::PlaybackState::Playing => "Playing",
        crate::info::market_replay::PlaybackState::Paused => "Paused",
    };
    ctx.fill_text(state_label, (x + w / 2.0) as f64, cy as f64);

    cy += 32.0;

    // Time display
    ctx.set_font("24px monospace");
    ctx.set_fill_color(&rgba_to_hex(TEXT_ACCENT));
    ctx.fill_text(&state.format_time(), (x + w / 2.0) as f64, cy as f64);

    cy += 40.0;

    // Speed
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    ctx.fill_text(&format!("Speed: {:.1}x", state.speed), (x + padding) as f64, cy as f64);

    cy += 32.0;

    // Progress bar
    let bar_y = cy;
    let bar_h = 24.0;
    let bar_w = w - (padding * 2.0);

    ctx.set_fill_color(&rgba_to_hex(BG_ELEMENT));
    ctx.fill_rect((x + padding) as f64, bar_y as f64, bar_w as f64, bar_h as f64);

    let progress = state.playback_progress();
    let progress_w = bar_w * progress as f32;
    ctx.set_fill_color(&rgba_to_hex(TEXT_ACCENT));
    ctx.fill_rect((x + padding) as f64, bar_y as f64, progress_w as f64, bar_h as f64);

    // Progress percentage
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text(&format!("{:.0}%", progress * 100.0), (x + w / 2.0) as f64, (bar_y + bar_h / 2.0) as f64);
}

pub fn render_timeline_panel(
    ctx: &mut dyn RenderContext,
    state: &TimelineState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let event_count = state.visible_events().len();
    ctx.set_font("14px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    ctx.fill_text(
        &format!("Timeline: {} events", event_count),
        (x + w / 2.0) as f64,
        (y + h / 2.0) as f64,
    );
}

pub fn render_graph_panel(
    ctx: &mut dyn RenderContext,
    state: &GraphState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let padding = 40.0;
    let chart_x = x + padding;
    let chart_y = y + padding;
    let chart_w = w - (padding * 2.0);
    let chart_h = h - (padding * 2.0) - 60.0; // Leave room for legend

    // Draw axes
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    // Y axis
    ctx.fill_rect((chart_x - 1.0) as f64, chart_y as f64, 1.0, chart_h as f64);
    // X axis
    ctx.fill_rect(chart_x as f64, (chart_y + chart_h) as f64, chart_w as f64, 1.0);

    // Draw Y axis labels
    ctx.set_font("10px monospace");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));

    for (pos, label) in state.y_labels(chart_h) {
        ctx.fill_text(&label, (chart_x - 4.0) as f64, (chart_y + pos) as f64);
    }

    // Draw X axis labels
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);

    for (pos, label) in state.x_labels(chart_w) {
        ctx.fill_text(&label, (chart_x + pos) as f64, (chart_y + chart_h + 4.0) as f64);
    }

    // Draw series lines
    let series_count = state.series_data.len();
    for series_idx in 0..series_count {
        let points = state.series_points(series_idx, chart_w, chart_h);
        if points.len() < 2 {
            continue;
        }

        let color = state.series_color(series_idx);
        ctx.set_fill_color(&rgba_to_hex(color));

        // Draw line segments
        for window in points.windows(2) {
            let (x1, y1) = window[0];
            let (x2, _y2) = window[1];

            // Simple line drawing (would use stroke_line in real impl)
            ctx.fill_rect((chart_x + x1) as f64, (chart_y + y1) as f64, (x2 - x1).abs() as f64 + 1.0, 2.0);
        }
    }

    // Draw legend
    let legend_y = y + h - 50.0;
    let legend_entries = state.legend_entries();
    let mut legend_x = x + padding;

    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);

    for (name, color) in legend_entries {
        // Color box
        ctx.set_fill_color(&rgba_to_hex(color));
        ctx.fill_rect(legend_x as f64, legend_y as f64, 12.0, 12.0);

        // Label
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(name, (legend_x + 16.0) as f64, (legend_y + 6.0) as f64);

        legend_x += 120.0;
    }
}

// ====================
// PORTFOLIO PANELS (2)
// ====================

pub fn render_portfolio_overview_panel(
    ctx: &mut dyn RenderContext,
    state: &PortfolioOverviewState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 48.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("16px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("Portfolio Overview", (x + 16.0) as f64, (y + header_h / 2.0) as f64);

    // Total value
    ctx.set_font("20px sans-serif");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_fill_color(&rgba_to_hex(TEXT_ACCENT));
    ctx.fill_text(&format!("${:.2}", state.total_value_usd), (x + w - 16.0) as f64, (y + header_h / 2.0) as f64);

    // Table header
    let table_y = y + header_h;
    let table_header_h = 28.0;
    ctx.set_fill_color(&rgba_to_hex(BG_ELEMENT));
    ctx.fill_rect(x as f64, table_y as f64, w as f64, table_header_h as f64);

    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));

    let padding = 12.0;
    ctx.fill_text("Asset", (x + padding) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Balance", (x + padding + 100.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("USD Value", (x + padding + 200.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("%", (x + padding + 300.0) as f64, (table_y + table_header_h / 2.0) as f64);

    // Render asset rows
    let assets = state.visible_assets(0, 50);
    let row_h = 24.0;
    let mut cy = table_y + table_header_h + 4.0;

    ctx.set_font("12px monospace");
    ctx.set_text_baseline(TextBaseline::Top);

    for (idx, item) in assets.iter().enumerate() {
        if cy + row_h > y + h {
            break;
        }

        // Asset
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&item.asset, (x + padding) as f64, cy as f64);

        // Balance
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(&format!("{:.4}", item.total), (x + padding + 100.0) as f64, cy as f64);

        // USD Value
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&format!("${:.2}", item.usd_value), (x + padding + 200.0) as f64, cy as f64);

        // Percentage with color
        let alloc_color = state.allocation_color(idx);
        ctx.set_fill_color(&rgba_to_hex(alloc_color));
        ctx.fill_text(&format!("{:.1}%", item.percent_of_portfolio), (x + padding + 300.0) as f64, cy as f64);

        cy += row_h;
    }
}

pub fn render_transaction_history_panel(
    ctx: &mut dyn RenderContext,
    state: &TransactionHistoryState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 40.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("16px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("Transaction History", (x + 16.0) as f64, (y + header_h / 2.0) as f64);

    let tx_count = state.visible_transactions(0, 100).len();
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    ctx.fill_text(
        &format!("{} transactions", tx_count),
        (x + w - 16.0) as f64,
        (y + header_h / 2.0) as f64,
    );

    // Table header
    let table_y = y + header_h;
    let table_header_h = 28.0;
    ctx.set_fill_color(&rgba_to_hex(BG_ELEMENT));
    ctx.fill_rect(x as f64, table_y as f64, w as f64, table_header_h as f64);

    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));

    let padding = 12.0;
    ctx.fill_text("Time", (x + padding) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Type", (x + padding + 80.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Asset", (x + padding + 160.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Amount", (x + padding + 230.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Status", (x + padding + 310.0) as f64, (table_y + table_header_h / 2.0) as f64);

    // Render transactions
    let transactions = state.visible_transactions(0, 50);
    let row_h = 24.0;
    let mut cy = table_y + table_header_h + 4.0;

    ctx.set_font("12px monospace");
    ctx.set_text_baseline(TextBaseline::Top);

    for tx in transactions {
        if cy + row_h > y + h {
            break;
        }

        let (time, tx_type, asset, amount, status) = state.format_transaction(tx);

        // Time
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(&time, (x + padding) as f64, cy as f64);

        // Type with color
        let type_color = state.type_color(tx);
        ctx.set_fill_color(&rgba_to_hex(type_color));
        ctx.fill_text(&tx_type, (x + padding + 80.0) as f64, cy as f64);

        // Asset
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&asset, (x + padding + 160.0) as f64, cy as f64);

        // Amount
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(&amount, (x + padding + 230.0) as f64, cy as f64);

        // Status with color
        let status_color = state.status_color(tx);
        ctx.set_fill_color(&rgba_to_hex(status_color));
        ctx.set_font("11px sans-serif");
        ctx.fill_text(&status, (x + padding + 310.0) as f64, cy as f64);
        ctx.set_font("12px monospace");

        cy += row_h;
    }
}

// ====================
// CALENDAR PANELS (4)
// ====================

pub fn render_earnings_calendar_panel(
    ctx: &mut dyn RenderContext,
    state: &EarningsCalendarState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 40.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("16px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("Earnings Calendar", (x + 16.0) as f64, (y + header_h / 2.0) as f64);

    let earnings = state.visible_earnings(0, 100);
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    ctx.fill_text(&format!("{} events", earnings.len()), (x + w - 16.0) as f64, (y + header_h / 2.0) as f64);

    // Table header
    let table_y = y + header_h;
    let table_header_h = 28.0;
    ctx.set_fill_color(&rgba_to_hex(BG_ELEMENT));
    ctx.fill_rect(x as f64, table_y as f64, w as f64, table_header_h as f64);

    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));

    let padding = 12.0;
    ctx.fill_text("Date", (x + padding) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Symbol", (x + padding + 80.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("EPS Act/Est", (x + padding + 160.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Surprise", (x + padding + 280.0) as f64, (table_y + table_header_h / 2.0) as f64);

    // Render earnings rows
    let row_h = 24.0;
    let mut cy = table_y + table_header_h + 4.0;

    ctx.set_font("12px monospace");
    ctx.set_text_baseline(TextBaseline::Top);

    for earning in earnings.iter() {
        if cy + row_h > y + h {
            break;
        }

        let (date, symbol, _company, eps, surprise, _time) = state.format_earning(earning);

        // Date
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(&date, (x + padding) as f64, cy as f64);

        // Symbol
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&symbol, (x + padding + 80.0) as f64, cy as f64);

        // EPS
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(&eps, (x + padding + 160.0) as f64, cy as f64);

        // Surprise % with color
        let surprise_color = state.surprise_color(earning);
        ctx.set_fill_color(&rgba_to_hex(surprise_color));
        ctx.fill_text(&surprise, (x + padding + 280.0) as f64, cy as f64);

        cy += row_h;
    }
}

pub fn render_dividend_calendar_panel(
    ctx: &mut dyn RenderContext,
    state: &DividendCalendarState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 40.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("16px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("Dividend Calendar", (x + 16.0) as f64, (y + header_h / 2.0) as f64);

    let dividends = state.visible_dividends(0, 100);
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    ctx.fill_text(&format!("{} events", dividends.len()), (x + w - 16.0) as f64, (y + header_h / 2.0) as f64);

    // Table header
    let table_y = y + header_h;
    let table_header_h = 28.0;
    ctx.set_fill_color(&rgba_to_hex(BG_ELEMENT));
    ctx.fill_rect(x as f64, table_y as f64, w as f64, table_header_h as f64);

    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));

    let padding = 12.0;
    ctx.fill_text("Ex-Date", (x + padding) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Symbol", (x + padding + 80.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Amount", (x + padding + 160.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Yield", (x + padding + 240.0) as f64, (table_y + table_header_h / 2.0) as f64);

    // Render dividend rows
    let row_h = 24.0;
    let mut cy = table_y + table_header_h + 4.0;

    ctx.set_font("12px monospace");
    ctx.set_text_baseline(TextBaseline::Top);

    for div in dividends.iter() {
        if cy + row_h > y + h {
            break;
        }

        let (ex_date, symbol, _company, amount, yield_str) = state.format_dividend(div);

        // Ex-Date
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(&ex_date, (x + padding) as f64, cy as f64);

        // Symbol
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&symbol, (x + padding + 80.0) as f64, cy as f64);

        // Amount
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(&amount, (x + padding + 160.0) as f64, cy as f64);

        // Yield with color
        let yield_color = state.yield_color(div);
        ctx.set_fill_color(&rgba_to_hex(yield_color));
        ctx.fill_text(&yield_str, (x + padding + 240.0) as f64, cy as f64);

        cy += row_h;
    }
}

pub fn render_options_expiry_panel(
    ctx: &mut dyn RenderContext,
    state: &OptionsExpiryState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 40.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("16px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("Options Expiry", (x + 16.0) as f64, (y + header_h / 2.0) as f64);

    let expiries = state.visible_expiries(0, 100);
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    ctx.fill_text(&format!("{} expiries", expiries.len()), (x + w - 16.0) as f64, (y + header_h / 2.0) as f64);

    // Table header
    let table_y = y + header_h;
    let table_header_h = 28.0;
    ctx.set_fill_color(&rgba_to_hex(BG_ELEMENT));
    ctx.fill_rect(x as f64, table_y as f64, w as f64, table_header_h as f64);

    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));

    let padding = 12.0;
    ctx.fill_text("Date", (x + padding) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Symbol", (x + padding + 80.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Total OI", (x + padding + 160.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("P/C Ratio", (x + padding + 240.0) as f64, (table_y + table_header_h / 2.0) as f64);

    // Render expiry rows
    let row_h = 24.0;
    let mut cy = table_y + table_header_h + 4.0;

    ctx.set_font("12px monospace");
    ctx.set_text_baseline(TextBaseline::Top);

    for exp in expiries.iter() {
        if cy + row_h > y + h {
            break;
        }

        let (date, symbol, total_oi, pcr, _max_pain, _spot) = state.format_expiry(exp);

        // Date
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(&date, (x + padding) as f64, cy as f64);

        // Symbol
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&symbol, (x + padding + 80.0) as f64, cy as f64);

        // Total OI
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(&total_oi, (x + padding + 160.0) as f64, cy as f64);

        // P/C Ratio with color
        let pcr_color = state.pcr_color(exp);
        ctx.set_fill_color(&rgba_to_hex(pcr_color));
        ctx.fill_text(&pcr, (x + padding + 240.0) as f64, cy as f64);

        cy += row_h;
    }
}

pub fn render_ipo_calendar_panel(
    ctx: &mut dyn RenderContext,
    state: &IpoCalendarState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 40.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("16px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("IPO Calendar", (x + 16.0) as f64, (y + header_h / 2.0) as f64);

    let ipos = state.visible_ipos(0, 100);
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    ctx.fill_text(&format!("{} IPOs", ipos.len()), (x + w - 16.0) as f64, (y + header_h / 2.0) as f64);

    // Table header
    let table_y = y + header_h;
    let table_header_h = 28.0;
    ctx.set_fill_color(&rgba_to_hex(BG_ELEMENT));
    ctx.fill_rect(x as f64, table_y as f64, w as f64, table_header_h as f64);

    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));

    let padding = 12.0;
    ctx.fill_text("Date", (x + padding) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Company", (x + padding + 80.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Symbol", (x + padding + 200.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Price Range", (x + padding + 280.0) as f64, (table_y + table_header_h / 2.0) as f64);

    // Render IPO rows
    let row_h = 24.0;
    let mut cy = table_y + table_header_h + 4.0;

    ctx.set_font("12px monospace");
    ctx.set_text_baseline(TextBaseline::Top);

    for ipo in ipos.iter() {
        if cy + row_h > y + h {
            break;
        }

        let (date, company, symbol, price_range, _market_cap) = state.format_ipo(ipo);

        // Date
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(&date, (x + padding) as f64, cy as f64);

        // Company (truncate if needed)
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        let company_short = if company.len() > 15 {
            format!("{}...", &company[..15])
        } else {
            company
        };
        ctx.fill_text(&company_short, (x + padding + 80.0) as f64, cy as f64);

        // Symbol
        let status_color = state.status_color(ipo);
        ctx.set_fill_color(&rgba_to_hex(status_color));
        ctx.fill_text(&symbol, (x + padding + 200.0) as f64, cy as f64);

        // Price Range
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(&price_range, (x + padding + 280.0) as f64, cy as f64);

        cy += row_h;
    }
}


// ====================
// NEWS PANELS (4)
// ====================

pub fn render_rss_feed_panel(
    ctx: &mut dyn RenderContext,
    state: &RssFeedState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 40.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("16px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("RSS Feed", (x + 16.0) as f64, (y + header_h / 2.0) as f64);

    let items = state.visible_items(100);
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    ctx.fill_text(&format!("{} items", items.len()), (x + w - 16.0) as f64, (y + header_h / 2.0) as f64);

    // Render items
    let padding = 12.0;
    let item_h = 48.0; // Two lines per item
    let mut cy = y + header_h + padding;

    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);

    for item in items.iter() {
        if cy + item_h > y + h {
            break;
        }

        let (time, title, author) = state.format_item(item);

        // Title line (bigger, primary color)
        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        let title_short = if title.len() > 60 {
            format!("{}...", &title[..60])
        } else {
            title
        };
        ctx.fill_text(&title_short, (x + padding) as f64, cy as f64);

        // Second line: Author + Time (smaller, dimmer)
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        let meta_line = format!("{} • {}", author, time);
        ctx.fill_text(&meta_line, (x + padding) as f64, (cy + 20.0) as f64);

        cy += item_h;
    }
}

pub fn render_social_sentiment_panel(
    ctx: &mut dyn RenderContext,
    state: &SocialSentimentState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 40.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("16px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    let title = if state.symbol.is_empty() {
        "Social Sentiment".to_string()
    } else {
        format!("Social Sentiment: {}", state.symbol)
    };
    ctx.fill_text(&title, (x + 16.0) as f64, (y + header_h / 2.0) as f64);

    // Sentiment score section
    let score_y = y + header_h + 20.0;
    ctx.set_font("48px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    let sentiment_color = state.sentiment_color();
    ctx.set_fill_color(&rgba_to_hex(sentiment_color));
    ctx.fill_text(&format!("{:.2}", state.sentiment.score), (x + w / 2.0) as f64, score_y as f64);

    // Mentions count
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    ctx.fill_text(
        &format!("{} mentions", state.sentiment.mentions_count),
        (x + w / 2.0) as f64,
        (score_y + 40.0) as f64,
    );

    // Source breakdown
    let sources_y = score_y + 80.0;
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);

    let padding = 16.0;
    let mut sy = sources_y;
    let row_h = 22.0;

    for (source, source_sent) in state.sentiment.sources.iter() {
        if sy + row_h > y + h - 20.0 {
            break;
        }

        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&format!("{:?}", source), (x + padding) as f64, sy as f64);

        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(
            &format!("{:.2} ({} mentions)", source_sent.score, source_sent.mentions),
            (x + padding + 120.0) as f64,
            sy as f64,
        );

        sy += row_h;
    }
}

pub fn render_analyst_ratings_panel(
    ctx: &mut dyn RenderContext,
    state: &AnalystRatingsState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 40.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("16px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    let title = if state.symbol.is_empty() {
        "Analyst Ratings".to_string()
    } else {
        format!("Analyst Ratings: {}", state.symbol)
    };
    ctx.fill_text(&title, (x + 16.0) as f64, (y + header_h / 2.0) as f64);

    let padding = 16.0;
    let mut cy = y + header_h + 16.0;

    // Consensus summary if available
    if let Some(ref consensus) = state.consensus {
        ctx.set_fill_color(&rgba_to_hex(BG_ELEMENT));
        let consensus_h = 60.0;
        ctx.fill_rect(x as f64, cy as f64, w as f64, consensus_h as f64);

        ctx.set_font("13px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));

        let line1 = format!("Consensus: {} Buy / {} Hold / {} Sell",
            consensus.buy_count, consensus.hold_count, consensus.sell_count);
        ctx.fill_text(&line1, (x + padding) as f64, (cy + 12.0) as f64);

        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        let line2 = format!("Avg Target: ${:.2} ({} analysts)",
            consensus.average_target, consensus.num_analysts);
        ctx.fill_text(&line2, (x + padding) as f64, (cy + 34.0) as f64);

        cy += consensus_h + 12.0;
    }

    // Ratings table header
    let table_header_h = 28.0;
    ctx.set_fill_color(&rgba_to_hex(BG_ELEMENT));
    ctx.fill_rect(x as f64, cy as f64, w as f64, table_header_h as f64);

    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));

    ctx.fill_text("Date", (x + padding) as f64, (cy + table_header_h / 2.0) as f64);
    ctx.fill_text("Analyst", (x + padding + 70.0) as f64, (cy + table_header_h / 2.0) as f64);
    ctx.fill_text("Rating", (x + padding + 180.0) as f64, (cy + table_header_h / 2.0) as f64);
    ctx.fill_text("Target", (x + padding + 280.0) as f64, (cy + table_header_h / 2.0) as f64);

    cy += table_header_h + 4.0;

    // Render ratings
    let ratings = state.visible_ratings(0, 50);
    let row_h = 24.0;

    ctx.set_font("12px monospace");
    ctx.set_text_baseline(TextBaseline::Top);

    for rating in ratings.iter() {
        if cy + row_h > y + h {
            break;
        }

        let (date, analyst, _firm, rating_str, target) = state.format_rating(rating);

        // Date
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(&date, (x + padding) as f64, cy as f64);

        // Analyst
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        let analyst_short = if analyst.len() > 12 {
            format!("{}...", &analyst[..12])
        } else {
            analyst
        };
        ctx.fill_text(&analyst_short, (x + padding + 70.0) as f64, cy as f64);

        // Rating with color
        let rating_color = state.rating_color(rating);
        ctx.set_fill_color(&rgba_to_hex(rating_color));
        ctx.fill_text(&rating_str, (x + padding + 180.0) as f64, cy as f64);

        // Target
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(&target, (x + padding + 280.0) as f64, cy as f64);

        cy += row_h;
    }
}

pub fn render_sec_filings_panel(
    ctx: &mut dyn RenderContext,
    state: &SecFilingsState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 40.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("16px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("SEC Filings", (x + 16.0) as f64, (y + header_h / 2.0) as f64);

    let filings = state.visible_filings(0, 100);
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    ctx.fill_text(&format!("{} filings", filings.len()), (x + w - 16.0) as f64, (y + header_h / 2.0) as f64);

    // Table header
    let table_y = y + header_h;
    let table_header_h = 28.0;
    ctx.set_fill_color(&rgba_to_hex(BG_ELEMENT));
    ctx.fill_rect(x as f64, table_y as f64, w as f64, table_header_h as f64);

    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));

    let padding = 12.0;
    ctx.fill_text("Date", (x + padding) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Symbol", (x + padding + 70.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Type", (x + padding + 140.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Description", (x + padding + 200.0) as f64, (table_y + table_header_h / 2.0) as f64);

    // Render filing rows
    let row_h = 24.0;
    let mut cy = table_y + table_header_h + 4.0;

    ctx.set_font("12px monospace");
    ctx.set_text_baseline(TextBaseline::Top);

    for filing in filings.iter() {
        if cy + row_h > y + h {
            break;
        }

        let (date, symbol, _company, filing_type, desc) = state.format_filing(filing);

        // Date
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(&date, (x + padding) as f64, cy as f64);

        // Symbol
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&symbol, (x + padding + 70.0) as f64, cy as f64);

        // Type with color
        let type_color = state.filing_type_color(&filing.filing_type);
        ctx.set_fill_color(&rgba_to_hex(type_color));
        ctx.fill_text(&filing_type, (x + padding + 140.0) as f64, cy as f64);

        // Description
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.set_font("11px monospace");
        ctx.fill_text(&desc, (x + padding + 200.0) as f64, cy as f64);
        ctx.set_font("12px monospace");

        cy += row_h;
    }
}

// ====================
// OPTIONS PANELS (2)
// ====================

pub fn render_greeks_panel(
    ctx: &mut dyn RenderContext,
    state: &GreeksPanelState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    render_key_value_rows(ctx, &state.greeks_list(), x, y, w, h);
}

pub fn render_option_flow_panel(
    ctx: &mut dyn RenderContext,
    state: &OptionFlowState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 40.0;
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("16px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("Option Flow", (x + 16.0) as f64, (y + header_h / 2.0) as f64);

    let flow_count = state.visible_flows(100).len();
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
    ctx.fill_text(
        &format!("{} flows", flow_count),
        (x + w - 16.0) as f64,
        (y + header_h / 2.0) as f64,
    );

    // Table header
    let table_y = y + header_h;
    let table_header_h = 28.0;
    ctx.set_fill_color(&rgba_to_hex(BG_ELEMENT));
    ctx.fill_rect(x as f64, table_y as f64, w as f64, table_header_h as f64);

    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));

    let padding = 12.0;
    ctx.fill_text("Time", (x + padding) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Symbol", (x + padding + 70.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Contract", (x + padding + 140.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Side", (x + padding + 250.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Premium", (x + padding + 310.0) as f64, (table_y + table_header_h / 2.0) as f64);
    ctx.fill_text("Type", (x + padding + 380.0) as f64, (table_y + table_header_h / 2.0) as f64);

    // Render flows
    let flows = state.visible_flows(50);
    let row_h = 24.0;
    let mut cy = table_y + table_header_h + 4.0;

    ctx.set_font("12px monospace");
    ctx.set_text_baseline(TextBaseline::Top);

    for flow in flows {
        if cy + row_h > y + h {
            break;
        }

        let (time, symbol, contract, side, premium, flow_type) = state.format_flow(flow);

        // Time
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(&time, (x + padding) as f64, cy as f64);

        // Symbol
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
        ctx.fill_text(&symbol, (x + padding + 70.0) as f64, cy as f64);

        // Contract
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.set_font("11px monospace");
        ctx.fill_text(&contract, (x + padding + 140.0) as f64, cy as f64);
        ctx.set_font("12px monospace");

        // Side with color
        let flow_color = state.flow_color(flow);
        ctx.set_fill_color(&rgba_to_hex(flow_color));
        ctx.fill_text(&side, (x + padding + 250.0) as f64, cy as f64);

        // Premium
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text(&premium, (x + padding + 310.0) as f64, cy as f64);

        // Flow type
        ctx.set_font("10px sans-serif");
        let type_color = state.flow_type_color(&flow.flow_type);
        ctx.set_fill_color(&rgba_to_hex(type_color));
        ctx.fill_text(&flow_type, (x + padding + 380.0) as f64, cy as f64);
        ctx.set_font("12px monospace");

        cy += row_h;
    }
}

// ====================
// RISK CALCULATOR
// ====================

pub fn render_risk_calculator_panel(
    ctx: &mut dyn RenderContext,
    state: &RiskCalculatorState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    // Background
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    // Title section
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, 40.0);

    ctx.set_font("14px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));
    ctx.fill_text("Risk Calculator", (x + w / 2.0) as f64, (y + 20.0) as f64);

    // Render input/output fields as key-value rows
    let rows: Vec<(&str, String)> = vec![
        ("Account Size", format!("${:.2}", state.account_size)),
        ("Risk %", format!("{:.2}%", state.risk_percent)),
        ("Entry Price", format!("${:.2}", state.entry_price)),
        ("Stop Loss", format!("${:.2}", state.stop_loss_price)),
        ("Take Profit", state.take_profit_price.map(|tp| format!("${:.2}", tp)).unwrap_or_else(|| "N/A".to_string())),
        ("Risk Amount", format!("${:.2}", state.risk_amount)),
        ("Position Size", format!("{:.4}", state.position_size)),
        ("Risk/Reward", state.risk_reward_ratio.map(|rr| format!("1:{:.2}", rr)).unwrap_or_else(|| "N/A".to_string())),
    ];

    render_key_value_rows(ctx, &rows, x, y + 40.0, w, h - 40.0);

    // Color-code the R:R ratio if present
    if let Some((_, rr_text)) = rows.iter().find(|(key, _)| *key == "Risk/Reward") {
        if state.risk_reward_ratio.is_some() {
            let rr_color = rgba_to_hex(state.risk_color());
            ctx.set_fill_color(&rr_color);
            ctx.set_font("14px monospace");
            ctx.set_text_align(TextAlign::Right);
            ctx.set_text_baseline(TextBaseline::Middle);
            let rr_y = y + 40.0 + 8.0 * (32.0 + 8.0);
            ctx.fill_text(rr_text, (x + w - 16.0) as f64, rr_y as f64);
        }
    }
}

// ====================
// TABLE (GENERIC)
// ====================

pub fn render_table_panel(
    ctx: &mut dyn RenderContext,
    state: &TableState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    // Background
    ctx.set_fill_color(&rgba_to_hex(BG_PANEL));
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let header_h = 32.0;
    let row_h = 28.0;

    // Get headers and calculate column widths
    let headers = state.column_headers();
    let col_widths = state.column_widths(w);

    if headers.is_empty() {
        // Empty table
        ctx.set_font("14px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&rgba_to_hex(TEXT_SECONDARY));
        ctx.fill_text("No data", (x + w / 2.0) as f64, (y + h / 2.0) as f64);
        return;
    }

    // Render header row
    ctx.set_fill_color(&rgba_to_hex(BG_SECTION));
    ctx.fill_rect(x as f64, y as f64, w as f64, header_h as f64);

    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));

    let mut col_x = x + 8.0;
    for (i, header) in headers.iter().enumerate() {
        let col_w = col_widths.get(i).copied().unwrap_or(100.0);
        ctx.fill_text(header, col_x as f64, (y + header_h / 2.0) as f64);

        // Sort indicator
        if let Some((sort_idx, ascending)) = state.sort_indicator() {
            if sort_idx == i {
                let arrow = if ascending { "▲" } else { "▼" };
                ctx.set_font("10px sans-serif");
                ctx.fill_text(arrow, (col_x + col_w - 16.0) as f64, (y + header_h / 2.0) as f64);
                ctx.set_font("13px sans-serif");
            }
        }

        col_x += col_w;
    }

    // Render data rows
    let max_visible_rows = ((h - header_h) / row_h).floor() as usize;
    let visible_rows = state.visible_rows(max_visible_rows);

    let mut row_y = y + header_h;
    for (row_idx, row) in visible_rows.iter().enumerate() {
        // Alternate row background
        if row_idx % 2 == 1 {
            ctx.set_fill_color(&rgba_to_hex(BG_ELEMENT));
            ctx.fill_rect(x as f64, row_y as f64, w as f64, row_h as f64);
        }

        ctx.set_font("12px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&rgba_to_hex(TEXT_PRIMARY));

        let mut col_x = x + 8.0;
        for (i, cell_text) in row.iter().enumerate() {
            let col_w = col_widths.get(i).copied().unwrap_or(100.0);
            ctx.fill_text(cell_text, col_x as f64, (row_y + row_h / 2.0) as f64);
            col_x += col_w;
        }

        row_y += row_h;
    }
}
