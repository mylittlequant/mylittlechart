use serde::{Serialize, Deserialize};

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
    pub symbol: String,

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
}

impl OrderEntryState {
    pub fn new(symbol: String) -> Self {
        Self {
            symbol,
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
