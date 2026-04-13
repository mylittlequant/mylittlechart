use serde::{Serialize, Deserialize};

/// PositionManager panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PositionManagerId(pub u64);

/// PositionManager panel state (heavy data)
#[derive(Clone, Debug)]
pub struct PositionManagerState {
    /// Open positions
    pub positions: Vec<Position>,
    /// Selected position index
    pub selected: Option<usize>,
    /// Edit mode (for TP/SL adjustment)
    pub edit_mode: Option<PositionEditMode>,
    /// Total unrealized PnL
    pub total_unrealized_pnl: f64,
}

#[derive(Clone, Debug)]
pub struct Position {
    pub symbol: String,
    pub side: PositionSide,
    pub quantity: f64,
    pub entry_price: f64,
    pub mark_price: f64,
    pub unrealized_pnl: f64,
    pub liquidation_price: Option<f64>,
    pub leverage: u32,
}

#[derive(Clone, Debug)]
pub enum PositionSide {
    Long,
    Short,
}

#[derive(Clone, Debug)]
pub enum PositionEditMode {
    TakeProfit(usize),
    StopLoss(usize),
    Leverage(usize),
}

impl PositionManagerState {
    pub fn new() -> Self {
        Self {
            positions: Vec::new(),
            selected: None,
            edit_mode: None,
            total_unrealized_pnl: 0.0,
        }
    }

    /// Get visible positions for rendering
    pub fn visible_positions(&self, scroll_offset: usize, max_rows: usize) -> &[Position] {
        let end = (scroll_offset + max_rows).min(self.positions.len());
        &self.positions[scroll_offset..end]
    }

    /// Format position field for display
    pub fn format_position(&self, pos: &Position, field: PositionField) -> String {
        match field {
            PositionField::Symbol => pos.symbol.clone(),
            PositionField::Side => match pos.side {
                PositionSide::Long => "LONG".to_string(),
                PositionSide::Short => "SHORT".to_string(),
            },
            PositionField::Quantity => format!("{:.4}", pos.quantity),
            PositionField::EntryPrice => format!("{:.4}", pos.entry_price),
            PositionField::MarkPrice => format!("{:.4}", pos.mark_price),
            PositionField::UnrealizedPnL => format!("{:+.2}", pos.unrealized_pnl),
            PositionField::Leverage => format!("{}x", pos.leverage),
            PositionField::LiqPrice => {
                pos.liquidation_price
                    .map(|p| format!("{:.4}", p))
                    .unwrap_or_else(|| "—".to_string())
            }
        }
    }

    /// Get color based on PnL
    pub fn pnl_color(&self, pos: &Position) -> [f32; 4] {
        if pos.unrealized_pnl > 0.0 {
            [0.2, 0.8, 0.3, 1.0] // green
        } else if pos.unrealized_pnl < 0.0 {
            [0.9, 0.2, 0.2, 1.0] // red
        } else {
            [0.6, 0.6, 0.7, 1.0] // neutral
        }
    }

    /// Apply a position update event received from the private WebSocket stream.
    ///
    /// Parameters match the fields of `digdigdig3::core::types::websocket::PositionUpdateEvent`.
    /// Callers extract these values before calling, keeping this crate free of digdigdig3.
    ///
    /// - `side_long`: true = Long, false = Short (Both/OneWay mapped by caller)
    /// - `quantity`: absolute position size; 0.0 means the position is closed
    pub fn apply_position_update(
        &mut self,
        symbol: &str,
        side_long: bool,
        quantity: f64,
        entry_price: f64,
        mark_price: Option<f64>,
        unrealized_pnl: f64,
        liquidation_price: Option<f64>,
        leverage: Option<u32>,
    ) {
        let side = if side_long { PositionSide::Long } else { PositionSide::Short };

        if let Some(existing) = self.positions.iter_mut().find(|p| p.symbol == symbol) {
            if quantity == 0.0 {
                // Position closed — will be removed below after this block; mark for removal
                // by setting quantity to 0 so the retain call handles it.
                existing.quantity = 0.0;
            } else {
                existing.side = side;
                existing.quantity = quantity;
                existing.entry_price = entry_price;
                existing.mark_price = mark_price.unwrap_or(existing.mark_price);
                existing.unrealized_pnl = unrealized_pnl;
                existing.liquidation_price = liquidation_price.or(existing.liquidation_price);
                existing.leverage = leverage.unwrap_or(existing.leverage);
            }
        } else if quantity > 0.0 {
            // New position
            self.positions.push(Position {
                symbol: symbol.to_owned(),
                side,
                quantity,
                entry_price,
                mark_price: mark_price.unwrap_or(entry_price),
                unrealized_pnl,
                liquidation_price,
                leverage: leverage.unwrap_or(1),
            });
        }

        // Remove closed positions (quantity == 0).
        self.positions.retain(|p| p.quantity != 0.0);

        // Keep selected index in bounds after potential removal.
        if let Some(sel) = self.selected {
            if sel >= self.positions.len() {
                self.selected = self.positions.len().checked_sub(1);
            }
        }

        // Recompute aggregate unrealized PnL.
        self.total_unrealized_pnl = self.positions.iter().map(|p| p.unrealized_pnl).sum();
    }

    /// Get risk warning level based on position metrics
    pub fn risk_level(&self, pos: &Position) -> RiskLevel {
        if let Some(liq_price) = pos.liquidation_price {
            let distance_pct = ((pos.mark_price - liq_price) / pos.mark_price).abs() * 100.0;
            if distance_pct < 5.0 {
                RiskLevel::High
            } else if distance_pct < 15.0 {
                RiskLevel::Medium
            } else {
                RiskLevel::Low
            }
        } else {
            RiskLevel::Low
        }
    }
}

#[derive(Clone, Debug, Copy)]
pub enum PositionField {
    Symbol,
    Side,
    Quantity,
    EntryPrice,
    MarkPrice,
    UnrealizedPnL,
    Leverage,
    LiqPrice,
}

#[derive(Clone, Debug, Copy, PartialEq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

/// PositionManager panel configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PositionManagerConfig {
    /// Show percentage PnL
    pub show_pnl_percent: bool,
    /// Show liquidation price
    pub show_liq_price: bool,
    /// Quick close confirmation
    pub require_close_confirmation: bool,
    /// Risk warning threshold (% of margin)
    pub risk_warning_threshold: f64,
}

/// PositionManager panel wrapper (lightweight, lives in PanelKind)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PositionManagerPanel {
    id: PositionManagerId,
    title: String,
}

impl PositionManagerPanel {
    pub fn new(id: PositionManagerId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> PositionManagerId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "position_manager" }
    pub fn kind_label(&self) -> &'static str { "Positions" }
    pub fn min_size(&self) -> (f32, f32) { (300.0, 150.0) }
}
