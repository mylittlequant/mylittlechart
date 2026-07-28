//! OrdersLog — the bottom strip's "Orders" tab (W3 quality arc, 2026-07-25).
//!
//! Distinct from [`super::trade_log::TradeLogState`] (fills): this is the
//! order-level log — open (unfilled/partially-filled) orders with per-row
//! Cancel, plus a bounded history of orders that reached a terminal state
//! (Filled / Canceled / Rejected / Expired). One order can carry zero, one,
//! or several fills — the two logs answer different questions ("what's
//! resting on the book" vs "what actually traded").
//!
//! Only ever hosted inside [`super::trading_container::TradingContainerState`]
//! — there is no standalone OrdersLog panel kind. Cancel actions are
//! collected as pending flags (`pending_cancel`/`pending_cancel_all`) rather
//! than acted on directly — this panel has no access to `TradingMut`; the
//! app layer (`panel_click.rs`) reads and clears them after routing the
//! click, mirroring how `OrderEntryState::submitting` defers the actual
//! `TradingMut::place_order` call to the app layer.

use mlc_backend_trading_engine::{Order, OrderSide, OrderStatus, OrderType, SharedTradingSnapshot};

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

/// OrdersLog panel state (heavy data).
#[derive(Clone, Debug)]
pub struct OrdersLogState {
    /// Open (New / PartiallyFilled) orders, in snapshot order.
    pub open_orders: Vec<Order>,
    /// Terminal-state orders (Filled / Canceled / Rejected / Expired),
    /// newest first.
    pub history: Vec<Order>,
    /// Scroll offset (rows scrolled down from top) — applies to the open
    /// orders section.
    pub scroll_offset: usize,
    /// Scroll offset for the History section (rows scrolled down from the
    /// newest entry) — the section scrolls independently of the open list
    /// (owner defect 2026-07-26: history had no scroll of its own).
    pub history_scroll_offset: usize,
    /// Shared trading snapshot (source of truth).
    pub snapshot: Option<SharedTradingSnapshot>,
    /// Order id awaiting a single-order cancel — set by `"orders:cancel:{id}"`,
    /// read + cleared by the app layer after issuing `TradingMut::cancel_order`.
    pub pending_cancel: Option<String>,
    /// Cancel-all request — set by `"orders:cancel_all"`, read + cleared by
    /// the app layer (which loops `TradingMut::cancel_order` over every
    /// currently open order — the paper engine has no native "cancel all"
    /// scope).
    pub pending_cancel_all: bool,
}

impl OrdersLogState {
    pub fn new() -> Self {
        Self {
            open_orders: Vec::new(),
            history: Vec::new(),
            scroll_offset: 0,
            history_scroll_offset: 0,
            snapshot: None,
            pending_cancel: None,
            pending_cancel_all: false,
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

        self.open_orders = snap.open_orders.clone();
        self.history = snap.order_history.iter().rev().cloned().collect();

        let max_scroll = self.open_orders.len().saturating_sub(1);
        self.scroll_offset = self.scroll_offset.min(max_scroll);
        let max_hist_scroll = self.history.len().saturating_sub(1);
        self.history_scroll_offset = self.history_scroll_offset.min(max_hist_scroll);
    }

    /// Get visible open orders for rendering.
    fn visible_open(&self, scroll_offset: usize, max_rows: usize) -> &[Order] {
        let end = (scroll_offset + max_rows).min(self.open_orders.len());
        if scroll_offset >= self.open_orders.len() {
            return &[];
        }
        &self.open_orders[scroll_offset..end]
    }
}

impl Default for OrdersLogState {
    fn default() -> Self {
        Self::new()
    }
}

fn format_order_type(order_type: &OrderType) -> &'static str {
    match order_type {
        OrderType::Market => "MARKET",
        OrderType::Limit { .. } => "LIMIT",
        OrderType::StopMarket { .. } => "STOP-MKT",
        OrderType::StopLimit { .. } => "STOP-LMT",
        _ => "OTHER",
    }
}

fn format_price_stop(order: &Order) -> String {
    match (order.price, order.stop_price) {
        (Some(p), Some(sp)) => format!("{:.4} / {:.4}", sp, p),
        (Some(p), None) => format!("{:.4}", p),
        (None, Some(sp)) => format!("{:.4}", sp),
        (None, None) => "\u{2014}".to_string(),
    }
}

fn format_status(status: OrderStatus) -> &'static str {
    match status {
        OrderStatus::New => "NEW",
        OrderStatus::Open => "OPEN",
        OrderStatus::PartiallyFilled => "PARTIAL",
        OrderStatus::Filled => "FILLED",
        OrderStatus::Canceled => "CANCELED",
        OrderStatus::Rejected => "REJECTED",
        OrderStatus::Expired => "EXPIRED",
    }
}

/// Format an age in ms as a short human string (`"12s"` / `"3m"` / `"1h"`).
fn format_age(created_at_ms: i64, now_ms: i64) -> String {
    let age_s = ((now_ms - created_at_ms).max(0)) / 1000;
    if age_s < 60 {
        format!("{}s", age_s)
    } else if age_s < 3600 {
        format!("{}m", age_s / 60)
    } else {
        format!("{}h", age_s / 3600)
    }
}

fn format_timestamp(ts: i64) -> String {
    let secs = (ts / 1000) % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

const OL_HEADER_HEIGHT: f32 = 20.0;
const OL_ROW_HEIGHT: f32 = 20.0;
const OL_SECTION_LABEL_HEIGHT: f32 = 18.0;
const OL_LEFT_PAD: f32 = 6.0;
const OL_CANCEL_W: f32 = 22.0;

impl TradingPanel for OrdersLogState {
    fn kind(&self) -> &'static str { "orders_log" }
    fn label(&self) -> &'static str { "Orders" }

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
            let body_id = format!("{}:orders:body", slot_prefix);
            coordinator.register(
                body_id.as_str(),
                uzor::Rect::new(x as f64, y as f64, w as f64, h as f64),
                uzor::input::Sense::SCROLL,
            );
        }

        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        let cancel_x = x + w - OL_CANCEL_W;
        let table_w = w - OL_CANCEL_W;
        let side_w   = (table_w * 0.10).max(38.0);
        let type_w   = (table_w * 0.16).max(56.0);
        let price_w  = (table_w * 0.24).max(80.0);
        let qty_w    = (table_w * 0.16).max(52.0);
        let status_w = (table_w * 0.18).max(56.0);

        let col_side_x   = x + OL_LEFT_PAD;
        let col_type_x   = col_side_x + side_w;
        let col_price_x  = col_type_x + type_w;
        let col_qty_x    = col_price_x + price_w;
        let col_status_x = col_qty_x + qty_w;
        let col_age_x    = col_status_x + status_w;

        // --- Header + Cancel-All ---
        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, OL_HEADER_HEIGHT as f64);

        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&theme.text_header);
        let header_mid_y = (y + OL_HEADER_HEIGHT / 2.0) as f64;
        ctx.fill_text("Side", col_side_x as f64, header_mid_y);
        ctx.fill_text("Type", col_type_x as f64, header_mid_y);
        ctx.fill_text("Stop/Price", col_price_x as f64, header_mid_y);
        ctx.fill_text("Qty", col_qty_x as f64, header_mid_y);
        ctx.fill_text("Status", col_status_x as f64, header_mid_y);
        ctx.fill_text("Age", col_age_x as f64, header_mid_y);

        if !self.open_orders.is_empty() {
            let ca_id = format!("{}:orders:cancel_all", slot_prefix);
            if coordinator.is_hovered(&uzor::WidgetId::from(ca_id.as_str())) {
                ctx.set_fill_color(&theme.hover);
                ctx.fill_rect(cancel_x as f64, y as f64, OL_CANCEL_W as f64, OL_HEADER_HEIGHT as f64);
            }
            ctx.set_fill_color(&theme.sell_bright);
            ctx.set_text_align(TextAlign::Center);
            ctx.fill_text("Cancel All", (cancel_x + OL_CANCEL_W / 2.0) as f64, header_mid_y);
            coordinator.register(
                ca_id.as_str(),
                uzor::Rect::new(cancel_x as f64, y as f64, OL_CANCEL_W as f64, OL_HEADER_HEIGHT as f64),
                uzor::input::Sense::CLICK,
            );
        }

        let now_ms = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        // Reserve room for the "History" section label + at least a couple
        // of rows, same budget for both empty and non-empty open-order
        // lists so the divider position doesn't jump around.
        let open_area_h = ((h - OL_HEADER_HEIGHT) * 0.55).max(OL_ROW_HEIGHT);
        let max_open_rows = (open_area_h / OL_ROW_HEIGHT).floor().max(1.0) as usize;
        let visible_open = self.visible_open(self.scroll_offset, max_open_rows);

        if self.open_orders.is_empty() {
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.set_fill_color(&theme.text_header);
            ctx.fill_text("No open orders", (x + w / 2.0) as f64, (y + OL_HEADER_HEIGHT + open_area_h / 2.0) as f64);
        } else {
            for (row_idx, order) in visible_open.iter().enumerate() {
                let row_y = y + OL_HEADER_HEIGHT + (row_idx as f32 * OL_ROW_HEIGHT);
                let row_mid_y = (row_y + OL_ROW_HEIGHT / 2.0) as f64;

                // Interactivity pass (owner spec 2026-07-26): rows react to
                // the cursor.
                let row_id = format!("{}:orders:open_row:{}", slot_prefix, row_idx);
                let row_hov = coordinator.is_hovered(&uzor::WidgetId::from(row_id.as_str()));
                coordinator.register(
                    row_id.as_str(),
                    uzor::Rect::new(x as f64, row_y as f64, w as f64, OL_ROW_HEIGHT as f64),
                    uzor::input::Sense::CLICK,
                );
                ctx.set_fill_color(if row_hov { &theme.hover } else { &theme.panel_bg });
                ctx.fill_rect(x as f64, row_y as f64, w as f64, OL_ROW_HEIGHT as f64);

                ctx.set_font("10px monospace");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);

                let (side_str, side_color) = match order.side {
                    OrderSide::Buy => ("BUY", &theme.pm_long),
                    OrderSide::Sell => ("SELL", &theme.pm_short),
                };
                ctx.set_fill_color(side_color);
                ctx.fill_text(side_str, col_side_x as f64, row_mid_y);

                ctx.set_fill_color(&theme.text_primary);
                ctx.fill_text(format_order_type(&order.order_type), col_type_x as f64, row_mid_y);
                ctx.fill_text(&format_price_stop(order), col_price_x as f64, row_mid_y);
                ctx.fill_text(&format!("{:.4}", order.quantity), col_qty_x as f64, row_mid_y);

                ctx.set_fill_color(&theme.text_muted);
                ctx.fill_text(format_status(order.status), col_status_x as f64, row_mid_y);
                ctx.fill_text(&format_age(order.created_at, now_ms), col_age_x as f64, row_mid_y);

                let cancel_id = format!("{}:orders:cancel:{}", slot_prefix, order.id);
                if coordinator.is_hovered(&uzor::WidgetId::from(cancel_id.as_str())) {
                    ctx.set_fill_color(&theme.hover);
                    ctx.fill_rect(cancel_x as f64, row_y as f64, OL_CANCEL_W as f64, OL_ROW_HEIGHT as f64);
                }
                ctx.set_fill_color(&theme.sell_bright);
                ctx.set_text_align(TextAlign::Center);
                ctx.fill_text("\u{2715}", (cancel_x + OL_CANCEL_W / 2.0) as f64, row_mid_y);
                coordinator.register(
                    cancel_id.as_str(),
                    uzor::Rect::new(cancel_x as f64, row_y as f64, OL_CANCEL_W as f64, OL_ROW_HEIGHT as f64),
                    uzor::input::Sense::CLICK,
                );

                ctx.set_fill_color(&theme.separator);
                ctx.fill_rect(x as f64, (row_y + OL_ROW_HEIGHT - 1.0) as f64, w as f64, 1.0);
            }
        }

        // --- History section (below open orders, clipped — no scroll of
        // its own; the open-orders list is the one that scrolls). ---
        let history_y = y + OL_HEADER_HEIGHT + open_area_h;
        let history_h = (y + h - history_y).max(0.0);
        if history_h < OL_SECTION_LABEL_HEIGHT {
            return;
        }

        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, history_y as f64, w as f64, OL_SECTION_LABEL_HEIGHT as f64);
        ctx.set_fill_color(&theme.text_header);
        ctx.set_font("10px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("History", (x + OL_LEFT_PAD) as f64, (history_y + OL_SECTION_LABEL_HEIGHT / 2.0) as f64);

        let rows_h = history_h - OL_SECTION_LABEL_HEIGHT;
        let max_hist_rows = (rows_h / OL_ROW_HEIGHT).floor().max(0.0) as usize;

        // Own scroll zone for the History section — registered AFTER the
        // whole-panel `orders:body` zone so it wins the hit-test inside
        // its rect (same layering trick the container children use).
        coordinator.register(
            format!("{}:orders:history", slot_prefix).as_str(),
            uzor::Rect::new(x as f64, history_y as f64, w as f64, history_h as f64),
            uzor::input::Sense::SCROLL,
        );

        for (row_idx, order) in self.history.iter().skip(self.history_scroll_offset).take(max_hist_rows).enumerate() {
            let row_y = history_y + OL_SECTION_LABEL_HEIGHT + (row_idx as f32 * OL_ROW_HEIGHT);
            let row_mid_y = (row_y + OL_ROW_HEIGHT / 2.0) as f64;

            let hist_row_id = format!("{}:orders:hist_row:{}", slot_prefix, row_idx);
            let hist_hov = coordinator.is_hovered(&uzor::WidgetId::from(hist_row_id.as_str()));
            coordinator.register(
                hist_row_id.as_str(),
                uzor::Rect::new(x as f64, row_y as f64, w as f64, OL_ROW_HEIGHT as f64),
                uzor::input::Sense::CLICK,
            );
            ctx.set_fill_color(if hist_hov { &theme.hover }
                else if row_idx % 2 == 0 { &theme.panel_bg }
                else { &theme.row_bg_alt });
            ctx.fill_rect(x as f64, row_y as f64, w as f64, OL_ROW_HEIGHT as f64);

            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);

            let (side_str, side_color) = match order.side {
                OrderSide::Buy => ("BUY", &theme.pm_long),
                OrderSide::Sell => ("SELL", &theme.pm_short),
            };
            ctx.set_fill_color(side_color);
            ctx.fill_text(side_str, col_side_x as f64, row_mid_y);

            ctx.set_fill_color(&theme.text_primary);
            ctx.fill_text(format_order_type(&order.order_type), col_type_x as f64, row_mid_y);
            ctx.fill_text(&format_price_stop(order), col_price_x as f64, row_mid_y);
            ctx.fill_text(&format!("{:.4}", order.quantity), col_qty_x as f64, row_mid_y);

            let status_color = match order.status {
                OrderStatus::Filled => &theme.tl_profit,
                OrderStatus::Canceled | OrderStatus::Rejected | OrderStatus::Expired => &theme.tl_loss,
                _ => &theme.text_muted,
            };
            ctx.set_fill_color(status_color);
            ctx.fill_text(format_status(order.status), col_status_x as f64, row_mid_y);

            ctx.set_fill_color(&theme.text_muted);
            ctx.fill_text(&format_timestamp(order.updated_at.unwrap_or(order.created_at)), col_age_x as f64, row_mid_y);
        }
    }

    fn handle_click(&mut self, local_id: &str, _x: f64, _y: f64) -> bool {
        match local_id {
            "orders:body" => false,
            "orders:cancel_all" => {
                if !self.open_orders.is_empty() {
                    self.pending_cancel_all = true;
                }
                true
            }
            s if s.starts_with("orders:cancel:") => {
                let order_id = &s["orders:cancel:".len()..];
                self.pending_cancel = Some(order_id.to_string());
                true
            }
            _ => false,
        }
    }

    fn handle_scroll(&mut self, local_id: &str, _dx: f64, dy: f64) -> bool {
        let delta = if dy < 0.0 { -1i64 } else if dy > 0.0 { 1i64 } else { 0 };
        match local_id {
            "orders:body" => {
                let new_offset = (self.scroll_offset as i64 + delta).max(0) as usize;
                let max = self.open_orders.len().saturating_sub(1);
                self.scroll_offset = new_offset.min(max);
                true
            }
            "orders:history" => {
                let new_offset = (self.history_scroll_offset as i64 + delta).max(0) as usize;
                let max = self.history.len().saturating_sub(1);
                self.history_scroll_offset = new_offset.min(max);
                true
            }
            _ => false,
        }
    }
}
