use serde::{Serialize, Deserialize};
use trading_manager::SharedTradingSnapshot;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

/// OrderEntry panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrderEntryId(pub u64);

/// Interactive elements in the Order Entry panel (for hover/click tracking)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OrderEntryElement {
    BuyButton,
    SellButton,
    OrderTypeButton(usize),    // 0=Limit, 1=Market, 2=StopLimit, 3=StopMarket
    PriceInput,
    StopPriceInput,
    QuantityInput,
    QuickQtyButton(usize),     // 0=25%, 1=50%, 2=75%, 3=100%
    TifButton(usize),          // 0=GTC, 1=IOC, 2=FOK
    SubmitButton,
}

/// OrderEntry panel state (heavy data)
#[derive(Clone, Debug)]
pub struct OrderEntryState {
    /// Symbol source binding (how to resolve which instrument to display)
    pub source: crate::trading::SymbolSource,

    pub symbol: String,

    /// Exchange identifier string (e.g. "binance")
    pub exchange: String,

    /// Account type short label (e.g. "S", "F", "FI")
    pub account_type: String,

    /// Order parameters
    pub side: OrderSide,         // Buy or Sell
    pub order_type: OrderType,   // Limit, Market, StopLimit, StopMarket
    pub price: Option<f64>,      // None for market orders
    pub quantity: f64,
    pub stop_price: Option<f64>, // For stop orders
    pub time_in_force: TimeInForce,

    /// Leverage (futures only)
    pub leverage: Option<u32>,

    /// Account info for validation
    pub available_balance: f64,

    /// Quick quantity buttons (% of balance or position)
    pub quick_qty_options: Vec<f32>, // e.g., [0.25, 0.5, 0.75, 1.0]

    /// Calculated values
    pub estimated_cost: f64,     // price * quantity
    pub estimated_fee: f64,      // exchange fee estimate
    pub post_order_balance: f64, // balance after order execution

    /// Validation errors
    pub errors: Vec<String>,

    /// Order submission status
    pub submitting: bool,

    /// Currently hovered element (for hover highlighting)
    pub hovered: Option<OrderEntryElement>,

    /// Currently active/editing text field (None = no field being edited)
    pub editing_field: Option<OrderEntryElement>,

    /// Inline text editing state (synced from centralized text_input_state)
    /// The renderer reads these directly for cursor/selection rendering
    pub editing_text: String,
    pub editing_cursor: usize,
    pub editing_selection: Option<usize>,
    pub editing_blink_time: u64,

    /// Scroll offset for when content exceeds panel height
    pub scroll_offset: f64,

    /// Shared snapshot from TradingManager
    pub snapshot: Option<SharedTradingSnapshot>,
}

impl OrderEntryState {
    pub fn new(symbol: String) -> Self {
        Self {
            source: crate::trading::SymbolSource::default(),
            symbol,
            exchange: String::new(),
            account_type: String::new(),
            side: OrderSide::Buy,
            order_type: OrderType::Limit,
            price: None,
            quantity: 0.0,
            stop_price: None,
            time_in_force: TimeInForce::GTC,
            leverage: None,
            available_balance: 0.0,
            quick_qty_options: vec![0.25, 0.5, 0.75, 1.0],
            estimated_cost: 0.0,
            estimated_fee: 0.0,
            post_order_balance: 0.0,
            errors: Vec::new(),
            submitting: false,
            hovered: None,
            editing_field: None,
            editing_text: String::new(),
            editing_cursor: 0,
            editing_selection: None,
            editing_blink_time: 0,
            scroll_offset: 0.0,
            snapshot: None,
        }
    }

    /// Check if order is valid and ready to submit
    pub fn is_valid(&self) -> bool {
        // Check required fields
        if self.quantity <= 0.0 {
            return false;
        }

        // Limit orders need price
        if matches!(self.order_type, OrderType::Limit | OrderType::StopLimit) && self.price.is_none() {
            return false;
        }

        // Stop orders need stop price
        if matches!(self.order_type, OrderType::StopLimit | OrderType::StopMarket) && self.stop_price.is_none() {
            return false;
        }

        // Check balance
        if self.estimated_cost > self.available_balance {
            return false;
        }

        // No validation errors
        self.errors.is_empty()
    }

    /// Get color for order side (green for buy, red for sell)
    pub fn side_color(&self) -> [f32; 4] {
        match self.side {
            OrderSide::Buy => [0.0, 0.8, 0.0, 1.0],  // Green
            OrderSide::Sell => [0.8, 0.0, 0.0, 1.0], // Red
        }
    }

    /// Format estimated cost as currency string
    pub fn format_estimated_cost(&self) -> String {
        if self.estimated_cost >= 1_000_000.0 {
            format!("${:.2}M", self.estimated_cost / 1_000_000.0)
        } else if self.estimated_cost >= 1_000.0 {
            format!("${:.2}K", self.estimated_cost / 1_000.0)
        } else {
            format!("${:.2}", self.estimated_cost)
        }
    }

    /// Format estimated fee as currency string
    pub fn format_estimated_fee(&self) -> String {
        format!("${:.2}", self.estimated_fee)
    }

    /// Get quick quantity buttons with labels and values
    pub fn quick_qty_values(&self) -> Vec<(String, f64)> {
        self.quick_qty_options
            .iter()
            .map(|pct| {
                let label = format!("{}%", (pct * 100.0) as u32);
                let qty = if let Some(price) = self.price {
                    (self.available_balance * (*pct as f64)) / price
                } else {
                    0.0
                };
                (label, qty)
            })
            .collect()
    }

    /// Validate order and populate errors vector
    pub fn validate(&mut self) {
        self.errors.clear();

        if self.quantity <= 0.0 {
            self.errors.push("Quantity must be greater than 0".to_string());
        }

        if matches!(self.order_type, OrderType::Limit | OrderType::StopLimit) && self.price.is_none() {
            self.errors.push("Limit orders require a price".to_string());
        }

        if matches!(self.order_type, OrderType::StopLimit | OrderType::StopMarket) && self.stop_price.is_none() {
            self.errors.push("Stop orders require a stop price".to_string());
        }

        if self.estimated_cost > self.available_balance {
            self.errors.push(format!(
                "Insufficient balance: need {} but have {}",
                self.format_estimated_cost(),
                format_currency(self.available_balance)
            ));
        }
    }

    /// Calculate estimated cost for the order
    pub fn estimated_cost(&self) -> f64 {
        let price = match self.order_type {
            OrderType::Market | OrderType::StopMarket => self.price.unwrap_or(0.0), // Would use market price
            OrderType::Limit | OrderType::StopLimit => self.price.unwrap_or(0.0),
        };

        price * self.quantity
    }

    /// Format quantity with appropriate precision
    pub fn format_quantity(&self) -> String {
        if self.quantity >= 1000.0 {
            format!("{:.1}K", self.quantity / 1000.0)
        } else if self.quantity >= 1.0 {
            format!("{:.2}", self.quantity)
        } else if self.quantity >= 0.01 {
            format!("{:.4}", self.quantity)
        } else {
            format!("{:.8}", self.quantity)
        }
    }

    /// Attach shared snapshot from TradingManager
    pub fn set_snapshot(&mut self, snap: SharedTradingSnapshot) {
        self.snapshot = Some(snap);
    }

    /// Pull balances and order status from snapshot
    pub fn sync_from_snapshot(&mut self) {
        let snap = match &self.snapshot {
            Some(s) => match s.read() {
                Ok(guard) => guard,
                Err(_) => return,
            },
            None => return,
        };

        // Update balance — take the largest free balance as "available"
        self.available_balance = snap.balances.iter()
            .map(|b| b.free)
            .fold(0.0_f64, f64::max);

        // Update submitting state from order_in_flight
        if !snap.order_in_flight && self.submitting {
            self.submitting = false;
        }

        // Show last error from trading manager
        if let Some(err) = &snap.last_error {
            if self.errors.is_empty() || self.errors.last().map(|e| e != err).unwrap_or(true) {
                self.errors.clear();
                self.errors.push(err.clone());
            }
        } else {
            self.errors.clear();
        }

        // Recalculate post_order_balance
        self.post_order_balance = (self.available_balance - self.estimated_cost).max(0.0);
    }

    /// Check if cursor should be visible (500ms blink cycle)
    pub fn is_cursor_visible(&self, now_ms: u64) -> bool {
        if self.editing_field.is_none() { return false; }
        let elapsed = now_ms.wrapping_sub(self.editing_blink_time);
        (elapsed / 500) % 2 == 0
    }

}

fn format_currency(value: f64) -> String {
    if value >= 1_000_000.0 {
        format!("${:.2}M", value / 1_000_000.0)
    } else if value >= 1_000.0 {
        format!("${:.2}K", value / 1_000.0)
    } else {
        format!("${:.2}", value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum OrderType {
    Limit,
    Market,
    StopLimit,
    StopMarket,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TimeInForce {
    GTC, // Good Till Cancelled
    IOC, // Immediate or Cancel
    FOK, // Fill or Kill
}

const OE_TITLE_HEIGHT: f32 = 22.0;
const OE_TOGGLE_HEIGHT: f32 = 28.0;
const OE_TAB_HEIGHT: f32 = 22.0;
const OE_FIELD_HEIGHT: f32 = 22.0;
const OE_SUBMIT_HEIGHT: f32 = 30.0;
const OE_PAD: f32 = 6.0;
const OE_ERROR_HEIGHT: f32 = 16.0;

impl TradingPanel for OrderEntryState {
    fn kind(&self) -> &'static str { "order_entry" }
    fn label(&self) -> &'static str { "Order Entry" }

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
        // Register base body layer so the coordinator recognises this panel as hovered.
        coordinator.register(
            format!("{}:oe:body", slot_prefix).as_str(),
            uzor::Rect::new(x as f64, y as f64, w as f64, h as f64),
            uzor::input::Sense::CLICK,
        );

        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        let mut cursor_y = y;

        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, cursor_y as f64, w as f64, OE_TITLE_HEIGHT as f64);

        ctx.set_fill_color(&theme.text_primary);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Order Entry", (x + OE_PAD) as f64, (cursor_y + OE_TITLE_HEIGHT / 2.0) as f64);

        if !self.symbol.is_empty() {
            ctx.set_fill_color(&theme.text_muted);
            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Right);
            ctx.fill_text(&self.symbol, (x + w - OE_PAD) as f64, (cursor_y + OE_TITLE_HEIGHT / 2.0) as f64);
        }

        cursor_y += OE_TITLE_HEIGHT;

        let half_w = w / 2.0;

        // BUY toggle button.
        let buy_bg = if self.side == OrderSide::Buy { &theme.oe_buy_button } else { &theme.oe_tab_inactive };
        ctx.set_fill_color(buy_bg);
        ctx.fill_rect(x as f64, cursor_y as f64, half_w as f64, OE_TOGGLE_HEIGHT as f64);
        coordinator.register(
            format!("{}:oe:buy", slot_prefix).as_str(),
            uzor::Rect::new(x as f64, cursor_y as f64, half_w as f64, OE_TOGGLE_HEIGHT as f64),
            uzor::input::Sense::CLICK,
        );

        ctx.set_fill_color(&theme.oe_buy_button_text);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("BUY", (x + half_w / 2.0) as f64, (cursor_y + OE_TOGGLE_HEIGHT / 2.0) as f64);

        // SELL toggle button.
        let sell_bg = if self.side == OrderSide::Sell { &theme.oe_sell_button } else { &theme.oe_tab_inactive };
        ctx.set_fill_color(sell_bg);
        ctx.fill_rect((x + half_w) as f64, cursor_y as f64, half_w as f64, OE_TOGGLE_HEIGHT as f64);
        coordinator.register(
            format!("{}:oe:sell", slot_prefix).as_str(),
            uzor::Rect::new((x + half_w) as f64, cursor_y as f64, half_w as f64, OE_TOGGLE_HEIGHT as f64),
            uzor::input::Sense::CLICK,
        );

        ctx.set_fill_color(&theme.oe_sell_button_text);
        ctx.fill_text("SELL", (x + half_w + half_w / 2.0) as f64, (cursor_y + OE_TOGGLE_HEIGHT / 2.0) as f64);

        cursor_y += OE_TOGGLE_HEIGHT;

        let tabs: &[(&str, OrderType)] = &[
            ("Limit",   OrderType::Limit),
            ("Market",  OrderType::Market),
            ("Stp-Lmt", OrderType::StopLimit),
            ("Stp-Mkt", OrderType::StopMarket),
        ];
        let tab_w = w / tabs.len() as f32;

        for (i, (label, ot)) in tabs.iter().enumerate() {
            let tab_x = x + i as f32 * tab_w;
            let is_active = self.order_type == *ot;

            let tab_bg = if is_active { &theme.oe_tab_active } else { &theme.oe_tab_inactive };
            ctx.set_fill_color(tab_bg);
            ctx.fill_rect(tab_x as f64, cursor_y as f64, tab_w as f64, OE_TAB_HEIGHT as f64);
            coordinator.register(
                format!("{}:oe:tab:{}", slot_prefix, i).as_str(),
                uzor::Rect::new(tab_x as f64, cursor_y as f64, tab_w as f64, OE_TAB_HEIGHT as f64),
                uzor::input::Sense::CLICK,
            );

            if is_active {
                let accent = match self.side {
                    OrderSide::Buy => &theme.oe_buy_button,
                    OrderSide::Sell => &theme.oe_sell_button,
                };
                ctx.set_fill_color(accent);
                ctx.fill_rect(tab_x as f64, (cursor_y + OE_TAB_HEIGHT - 2.0) as f64, tab_w as f64, 2.0);
            }

            ctx.set_fill_color(&theme.text_primary);
            ctx.set_font("9px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(label, (tab_x + tab_w / 2.0) as f64, (cursor_y + OE_TAB_HEIGHT / 2.0) as f64);

            if i + 1 < tabs.len() {
                ctx.set_fill_color(&theme.separator);
                ctx.fill_rect((tab_x + tab_w - 1.0) as f64, cursor_y as f64, 1.0, OE_TAB_HEIGHT as f64);
            }
        }

        cursor_y += OE_TAB_HEIGHT;

        let field_value_right = x + w - OE_PAD;

        let draw_field = |ctx: &mut dyn RenderContext, row_y: f32, label: &str, value: &str| {
            ctx.set_fill_color(&theme.oe_input_bg);
            ctx.fill_rect(x as f64, row_y as f64, w as f64, OE_FIELD_HEIGHT as f64);

            ctx.set_fill_color(&theme.oe_input_border);
            ctx.fill_rect(x as f64, (row_y + OE_FIELD_HEIGHT - 1.0) as f64, w as f64, 1.0);

            ctx.set_fill_color(&theme.text_muted);
            ctx.set_font("10px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(label, (x + OE_PAD) as f64, (row_y + OE_FIELD_HEIGHT / 2.0) as f64);

            ctx.set_fill_color(&theme.text_primary);
            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Right);
            ctx.fill_text(value, field_value_right as f64, (row_y + OE_FIELD_HEIGHT / 2.0) as f64);
        };

        if matches!(self.order_type, OrderType::Limit | OrderType::StopLimit) {
            let price_str = self.price
                .map(|p| format!("{:.4}", p))
                .unwrap_or_else(|| "\u{2014}".to_string());
            draw_field(ctx, cursor_y, "Price:", &price_str);
            coordinator.register(
                format!("{}:oe:price", slot_prefix).as_str(),
                uzor::Rect::new(x as f64, cursor_y as f64, w as f64, OE_FIELD_HEIGHT as f64),
                uzor::input::Sense::CLICK,
            );
            cursor_y += OE_FIELD_HEIGHT;
        }

        if matches!(self.order_type, OrderType::StopLimit | OrderType::StopMarket) {
            let stop_str = self.stop_price
                .map(|p| format!("{:.4}", p))
                .unwrap_or_else(|| "\u{2014}".to_string());
            draw_field(ctx, cursor_y, "Stop:", &stop_str);
            coordinator.register(
                format!("{}:oe:stop_price", slot_prefix).as_str(),
                uzor::Rect::new(x as f64, cursor_y as f64, w as f64, OE_FIELD_HEIGHT as f64),
                uzor::input::Sense::CLICK,
            );
            cursor_y += OE_FIELD_HEIGHT;
        }

        draw_field(ctx, cursor_y, "Quantity:", &self.format_quantity());
        coordinator.register(
            format!("{}:oe:qty", slot_prefix).as_str(),
            uzor::Rect::new(x as f64, cursor_y as f64, w as f64, OE_FIELD_HEIGHT as f64),
            uzor::input::Sense::CLICK,
        );
        cursor_y += OE_FIELD_HEIGHT;

        if let Some(lev) = self.leverage {
            draw_field(ctx, cursor_y, "Leverage:", &format!("{}x", lev));
            cursor_y += OE_FIELD_HEIGHT;
        }

        let bal = self.available_balance;
        let balance_str = if bal >= 1_000_000.0 {
            format!("${:.2}M", bal / 1_000_000.0)
        } else if bal >= 1_000.0 {
            format!("${:.2}K", bal / 1_000.0)
        } else {
            format!("${:.2}", bal)
        };
        draw_field(ctx, cursor_y, "Available:", &balance_str);
        cursor_y += OE_FIELD_HEIGHT;

        if self.estimated_cost > 0.0 {
            draw_field(ctx, cursor_y, "Est. Cost:", &self.format_estimated_cost());
            cursor_y += OE_FIELD_HEIGHT;
        }

        let error_area_h = self.errors.len() as f32 * OE_ERROR_HEIGHT;
        let remaining = y + h - cursor_y - error_area_h;

        if remaining >= OE_SUBMIT_HEIGHT {
            let submit_y = cursor_y + (remaining - OE_SUBMIT_HEIGHT).max(0.0);

            let submit_color = match self.side {
                OrderSide::Buy => &theme.oe_buy_button,
                OrderSide::Sell => &theme.oe_sell_button,
            };

            ctx.set_fill_color(submit_color);
            ctx.fill_rect((x + OE_PAD) as f64, submit_y as f64, (w - OE_PAD * 2.0) as f64, OE_SUBMIT_HEIGHT as f64);
            coordinator.register(
                format!("{}:oe:submit", slot_prefix).as_str(),
                uzor::Rect::new((x + OE_PAD) as f64, submit_y as f64, (w - OE_PAD * 2.0) as f64, OE_SUBMIT_HEIGHT as f64),
                uzor::input::Sense::CLICK,
            );

            let submit_label = if self.submitting {
                "..."
            } else {
                match self.side {
                    OrderSide::Buy => "BUY",
                    OrderSide::Sell => "SELL",
                }
            };

            let submit_text_color = match self.side {
                OrderSide::Buy => &theme.oe_buy_button_text,
                OrderSide::Sell => &theme.oe_sell_button_text,
            };
            ctx.set_fill_color(submit_text_color);
            ctx.set_font("13px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(submit_label, (x + w / 2.0) as f64, (submit_y + OE_SUBMIT_HEIGHT / 2.0) as f64);

            cursor_y = submit_y + OE_SUBMIT_HEIGHT;
        }

        for error in &self.errors {
            if cursor_y + OE_ERROR_HEIGHT > y + h {
                break;
            }
            ctx.set_fill_color(&theme.sell_bright);
            ctx.set_font("9px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(error, (x + OE_PAD) as f64, (cursor_y + OE_ERROR_HEIGHT / 2.0) as f64);
            cursor_y += OE_ERROR_HEIGHT;
        }

        let _ = cursor_y;
    }

    fn handle_click(&mut self, local_id: &str, _x: f64, _y: f64) -> bool {
        match local_id {
            "oe:buy" => {
                self.side = OrderSide::Buy;
                true
            }
            "oe:sell" => {
                self.side = OrderSide::Sell;
                true
            }
            s if s.starts_with("oe:tab:") => {
                if let Ok(idx) = s["oe:tab:".len()..].parse::<usize>() {
                    self.order_type = match idx {
                        0 => OrderType::Limit,
                        1 => OrderType::Market,
                        2 => OrderType::StopLimit,
                        3 => OrderType::StopMarket,
                        _ => self.order_type,
                    };
                }
                true
            }
            "oe:price" => {
                self.editing_field = Some(OrderEntryElement::PriceInput);
                self.editing_text = self.price.map(|p| format!("{:.4}", p)).unwrap_or_default();
                self.editing_cursor = self.editing_text.len();
                true
            }
            "oe:stop_price" => {
                self.editing_field = Some(OrderEntryElement::StopPriceInput);
                self.editing_text = self.stop_price.map(|p| format!("{:.4}", p)).unwrap_or_default();
                self.editing_cursor = self.editing_text.len();
                true
            }
            "oe:qty" => {
                self.editing_field = Some(OrderEntryElement::QuantityInput);
                self.editing_text = self.format_quantity();
                self.editing_cursor = self.editing_text.len();
                true
            }
            "oe:submit" => {
                self.submitting = true;
                true
            }
            // Base body layer — consumed to prevent click-through, but no action.
            "oe:body" => false,
            _ => false,
        }
    }
}

/// OrderEntry panel configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderEntryConfig {
    /// Default order type
    pub default_order_type: OrderType,

    /// Default time in force
    pub default_tif: TimeInForce,

    /// Quick quantity percentages
    pub quick_qty_buttons: Vec<f32>,

    /// Show leverage slider (futures only)
    pub show_leverage: bool,

    /// Max leverage allowed
    pub max_leverage: u32,

    /// Fee rate (% for estimation)
    pub fee_rate: f64, // e.g., 0.001 for 0.1%

    /// Auto-calculate quantity from risk % (if linked to RiskCalculator)
    pub auto_quantity: bool,
}

impl Default for OrderEntryConfig {
    fn default() -> Self {
        Self {
            default_order_type: OrderType::Limit,
            default_tif: TimeInForce::GTC,
            quick_qty_buttons: vec![0.25, 0.5, 0.75, 1.0],
            show_leverage: false,
            max_leverage: 10,
            fee_rate: 0.001,
            auto_quantity: false,
        }
    }
}

/// OrderEntry panel wrapper (lightweight, lives in PanelKind)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderEntryPanel {
    id: OrderEntryId,
    title: String,
}

impl OrderEntryPanel {
    pub fn new(id: OrderEntryId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> OrderEntryId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "order_entry" }
    pub fn kind_label(&self) -> &'static str { "Order Entry" }
    pub fn min_size(&self) -> (f32, f32) { (250.0, 300.0) }
}
