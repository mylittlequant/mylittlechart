use serde::{Serialize, Deserialize};
use trading_manager::SharedTradingSnapshot;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};
use zengeld_chart::i18n::{TradingKey, current_language};

/// TradeLog panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TradeLogId(pub u64);

/// TradeLog panel state (heavy data)
#[derive(Clone, Debug)]
pub struct TradeLogState {
    /// List of user's executed trades
    pub trades: Vec<UserTrade>,
    /// Time range filter
    pub time_range: TimeRange,
    /// Symbol filter (optional)
    pub symbol_filter: Option<String>,
    /// Total PnL (computed)
    pub total_pnl: f64,
    /// Sort configuration
    pub sort: (TradeLogColumn, bool),
    /// Shared trading snapshot (source of truth)
    pub snapshot: Option<SharedTradingSnapshot>,
    /// Scroll offset (rows scrolled down from top)
    pub scroll_offset: usize,
}

#[derive(Clone, Debug)]
pub struct UserTrade {
    pub timestamp: i64,
    pub symbol: String,
    pub side: OrderSide,
    pub price: f64,
    pub quantity: f64,
    pub commission: f64,
}

#[derive(Clone, Debug)]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Clone, Debug)]
pub enum TimeRange {
    Today,
    Week,
    Month,
    All,
    Custom(i64, i64),
}

#[derive(Clone, Debug, Copy)]
pub enum TradeLogColumn {
    Time,
    Symbol,
    Side,
    Type,
    Price,
    Quantity,
    Commission,
    PnL,
}

impl TradeLogState {
    pub fn new() -> Self {
        Self {
            trades: Vec::new(),
            time_range: TimeRange::Today,
            symbol_filter: None,
            total_pnl: 0.0,
            sort: (TradeLogColumn::Time, false),
            snapshot: None,
            scroll_offset: 0,
        }
    }

    pub fn set_snapshot(&mut self, snap: SharedTradingSnapshot) {
        self.snapshot = Some(snap);
    }

    pub fn sync_from_snapshot(&mut self) {
        let snap = match &self.snapshot {
            Some(s) => match s.read() {
                Ok(guard) => guard,
                Err(_) => return,
            },
            None => return,
        };

        self.trades.clear();
        for fill in &snap.recent_fills {
            let side = match fill.side {
                trading_manager::OrderSide::Buy => OrderSide::Buy,
                trading_manager::OrderSide::Sell => OrderSide::Sell,
            };
            self.trades.push(UserTrade {
                timestamp: fill.timestamp,
                symbol: fill.symbol.clone(),
                side,
                price: fill.price,
                quantity: fill.quantity,
                commission: fill.fee,
            });
        }

        self.trades.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        self.total_pnl = snap.pnl.values().map(|p| p.realized).sum();
    }

    /// Get visible trades for rendering
    pub fn visible_trades(&self, scroll_offset: usize, max_rows: usize) -> &[UserTrade] {
        let end = (scroll_offset + max_rows).min(self.trades.len());
        &self.trades[scroll_offset..end]
    }

    /// Format trade for display
    pub fn format_trade(&self, trade: &UserTrade, column: TradeLogColumn) -> String {
        match column {
            TradeLogColumn::Time => format_timestamp(trade.timestamp),
            TradeLogColumn::Symbol => trade.symbol.clone(),
            TradeLogColumn::Side => match trade.side {
                OrderSide::Buy => "BUY".to_string(),
                OrderSide::Sell => "SELL".to_string(),
            },
            TradeLogColumn::Type => "LIMIT".to_string(), // placeholder
            TradeLogColumn::Price => format!("{:.4}", trade.price),
            TradeLogColumn::Quantity => format!("{:.4}", trade.quantity),
            TradeLogColumn::Commission => format!("{:.4}", trade.commission),
            TradeLogColumn::PnL => format!("{:+.2}", 0.0), // would calculate from position tracking
        }
    }

    /// Get color based on trade side or PnL
    pub fn pnl_color(&self, pnl: f64) -> [f32; 4] {
        if pnl > 0.0 {
            [0.2, 0.8, 0.3, 1.0] // green
        } else if pnl < 0.0 {
            [0.9, 0.2, 0.2, 1.0] // red
        } else {
            [0.6, 0.6, 0.7, 1.0] // neutral
        }
    }

    /// Get column headers for the trade log table
    pub fn column_headers(&self) -> Vec<&'static str> {
        vec!["Time", "Symbol", "Side", "Type", "Price", "Quantity", "Commission", "PnL"]
    }
}

fn format_timestamp(ts: i64) -> String {
    let secs = (ts / 1000) % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

const TL_HEADER_HEIGHT: f32 = 18.0;
const TL_ROW_HEIGHT: f32 = 18.0;
const TL_SUMMARY_HEIGHT: f32 = 20.0;
const TL_LEFT_PAD: f32 = 6.0;

impl TradingPanel for TradeLogState {
    fn kind(&self) -> &'static str { "trade_log" }
    fn label(&self) -> &'static str { "Trade Log" }

    fn render(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        theme: &crate::panel_theme::PanelTheme,
        coordinator: &mut uzor::InputCoordinator,
        slot_prefix: &str,
    ) {
        {
            let body_id = format!("{}:trade_log:body", slot_prefix);
            coordinator.register(
                body_id.as_str(),
                uzor::Rect::new(x as f64, y as f64, w as f64, h as f64),
                uzor::input::Sense::SCROLL,
            );
        }

        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        let time_w   = (w * 0.20).max(64.0);
        let symbol_w = (w * 0.20).max(58.0);
        let side_w   = (w * 0.11).max(34.0);
        let price_w  = (w * 0.20).max(60.0);
        let qty_w    = (w * 0.16).max(48.0);

        let col_time_x   = x + TL_LEFT_PAD;
        let col_symbol_x = col_time_x   + time_w;
        let col_side_x   = col_symbol_x + symbol_w;
        let col_price_x  = col_side_x   + side_w;
        let col_qty_x    = col_price_x  + price_w;
        let col_fee_x    = col_qty_x    + qty_w;

        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, TL_HEADER_HEIGHT as f64);

        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&theme.text_header);

        let header_text_y = (y + TL_HEADER_HEIGHT / 2.0) as f64;
        let lang = current_language();
        ctx.fill_text(TradingKey::Time.get(lang),   col_time_x   as f64, header_text_y);
        ctx.fill_text(TradingKey::Symbol.get(lang), col_symbol_x as f64, header_text_y);
        ctx.fill_text(TradingKey::Side.get(lang),   col_side_x   as f64, header_text_y);
        ctx.fill_text(TradingKey::Price.get(lang),  col_price_x  as f64, header_text_y);
        ctx.fill_text(TradingKey::Qty.get(lang),    col_qty_x    as f64, header_text_y);
        ctx.fill_text(TradingKey::Fee.get(lang),    col_fee_x    as f64, header_text_y);

        let content_h = h - TL_HEADER_HEIGHT - TL_SUMMARY_HEIGHT;
        let max_rows = (content_h / TL_ROW_HEIGHT).floor() as usize;
        let trades = self.visible_trades(self.scroll_offset, max_rows);

        if trades.is_empty() {
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.set_fill_color(&theme.text_header);
            ctx.fill_text(TradingKey::NoTradesLog.get(current_language()), (x + w / 2.0) as f64, (y + TL_HEADER_HEIGHT + content_h / 2.0) as f64);
        } else {
            for (i, trade) in trades.iter().enumerate() {
                let row_y = y + TL_HEADER_HEIGHT + (i as f32 * TL_ROW_HEIGHT);

                let row_bg = if i % 2 == 0 { &theme.panel_bg } else { &theme.tl_row_bg_alt };
                ctx.set_fill_color(row_bg);
                ctx.fill_rect(x as f64, row_y as f64, w as f64, TL_ROW_HEIGHT as f64);

                let text_y = (row_y + TL_ROW_HEIGHT / 2.0) as f64;

                ctx.set_font("10px monospace");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);

                let secs = (trade.timestamp / 1000) % 86400;
                let hh = secs / 3600;
                let mm = (secs % 3600) / 60;
                let ss = secs % 60;
                let time_str = format!("{:02}:{:02}:{:02}", hh, mm, ss);
                ctx.set_fill_color(&theme.text_primary);
                ctx.fill_text(&time_str, col_time_x as f64, text_y);

                let symbol = if trade.symbol.len() > 9 { &trade.symbol[..9] } else { trade.symbol.as_str() };
                ctx.fill_text(symbol, col_symbol_x as f64, text_y);

                let (side_str, side_color) = match trade.side {
                    OrderSide::Buy  => ("BUY",  &theme.tl_profit),
                    OrderSide::Sell => ("SELL", &theme.tl_loss),
                };
                ctx.set_fill_color(side_color);
                ctx.fill_text(side_str, col_side_x as f64, text_y);

                ctx.set_fill_color(&theme.text_primary);
                let price_str = format!("{:.4}", trade.price);
                ctx.fill_text(&price_str, col_price_x as f64, text_y);

                let qty_str = format!("{:.4}", trade.quantity);
                ctx.fill_text(&qty_str, col_qty_x as f64, text_y);

                ctx.set_font("9px monospace");
                ctx.set_fill_color(&theme.text_muted);
                let fee_str = format!("{:.4}", trade.commission);
                ctx.fill_text(&fee_str, col_fee_x as f64, text_y);

                ctx.set_fill_color(&theme.separator);
                ctx.fill_rect(x as f64, (row_y + TL_ROW_HEIGHT - 1.0) as f64, w as f64, 1.0);
            }
        }

        let summary_y = y + h - TL_SUMMARY_HEIGHT;
        ctx.set_fill_color(&theme.separator);
        ctx.fill_rect(x as f64, summary_y as f64, w as f64, 1.0);

        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, (summary_y + 1.0) as f64, w as f64, (TL_SUMMARY_HEIGHT - 1.0) as f64);

        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);

        let summary_text_y = (summary_y + TL_SUMMARY_HEIGHT / 2.0) as f64;
        ctx.set_fill_color(&theme.text_header);
        ctx.fill_text(TradingKey::TotalPnlLog.get(current_language()), (x + TL_LEFT_PAD) as f64, summary_text_y);

        let pnl_color = if self.total_pnl >= 0.0 { &theme.tl_profit } else { &theme.tl_loss };
        ctx.set_fill_color(pnl_color);
        let pnl_str = format!("{:+.2}", self.total_pnl);
        ctx.fill_text(&pnl_str, (x + TL_LEFT_PAD + 68.0) as f64, summary_text_y);

        ctx.set_fill_color(&theme.text_header);
        ctx.set_text_align(TextAlign::Right);
        let count_str = format!("Trades: {}", self.trades.len());
        ctx.fill_text(&count_str, (x + w - TL_LEFT_PAD) as f64, summary_text_y);
    }

    fn handle_click(&mut self, _local_id: &str, _x: f64, _y: f64) -> bool { false }

    fn handle_scroll(&mut self, local_id: &str, _dx: f64, dy: f64) -> bool {
        if local_id == "trade_log:body" {
            let delta = if dy < 0.0 { -1i64 } else if dy > 0.0 { 1i64 } else { 0 };
            let new_offset = (self.scroll_offset as i64 + delta).max(0) as usize;
            let max = self.trades.len().saturating_sub(1);
            self.scroll_offset = new_offset.min(max);
            true
        } else {
            false
        }
    }
}

/// TradeLog panel configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeLogConfig {
    /// Show commission column
    pub show_commission: bool,
    /// Group by order
    pub group_by_order: bool,
    /// PnL calculation method
    pub pnl_method: PnLMethod,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PnLMethod {
    FIFO,
    LIFO,
    Average,
}

/// TradeLog panel wrapper (lightweight, lives in PanelKind)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeLogPanel {
    id: TradeLogId,
    title: String,
}

impl TradeLogPanel {
    pub fn new(id: TradeLogId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> TradeLogId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "tradelog" }
    pub fn kind_label(&self) -> &'static str { "Trade Log" }
    pub fn min_size(&self) -> (f32, f32) { (200.0, 150.0) }
}
