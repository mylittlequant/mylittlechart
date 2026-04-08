use serde::{Serialize, Deserialize};

/// RiskCalculator panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RiskCalculatorId(pub u64);

/// RiskCalculator panel state (heavy data)
#[derive(Clone, Debug)]
pub struct RiskCalculatorState {
    /// Input fields
    pub account_size: f64,
    pub risk_percent: f64,       // e.g., 2.0 for 2%
    pub entry_price: f64,
    pub stop_loss_price: f64,
    pub take_profit_price: Option<f64>,

    /// Calculated outputs
    pub risk_amount: f64,        // account_size * (risk_percent / 100)
    pub position_size: f64,      // risk_amount / risk_per_unit
    pub risk_per_unit: f64,      // abs(entry_price - stop_loss_price)
    pub potential_profit: Option<f64>, // (take_profit_price - entry_price) * position_size
    pub risk_reward_ratio: Option<f64>, // potential_profit / risk_amount

    /// Leverage (optional)
    pub leverage: Option<u32>,
    pub margin_required: f64,

    /// Validation
    pub errors: Vec<String>,
}

impl RiskCalculatorState {
    pub fn new() -> Self {
        Self {
            account_size: 10000.0,
            risk_percent: 2.0,
            entry_price: 0.0,
            stop_loss_price: 0.0,
            take_profit_price: None,
            risk_amount: 0.0,
            position_size: 0.0,
            risk_per_unit: 0.0,
            potential_profit: None,
            risk_reward_ratio: None,
            leverage: None,
            margin_required: 0.0,
            errors: Vec::new(),
        }
    }

    /// Calculate all output fields from input fields
    pub fn calculate(&mut self) {
        self.errors.clear();

        // Validate inputs
        if self.account_size <= 0.0 {
            self.errors.push("Account size must be positive".to_string());
            return;
        }

        if self.risk_percent <= 0.0 || self.risk_percent > 100.0 {
            self.errors.push("Risk percent must be between 0 and 100".to_string());
            return;
        }

        if self.entry_price <= 0.0 {
            self.errors.push("Entry price must be positive".to_string());
            return;
        }

        if self.stop_loss_price <= 0.0 {
            self.errors.push("Stop loss price must be positive".to_string());
            return;
        }

        // Calculate risk amount
        self.risk_amount = self.account_size * (self.risk_percent / 100.0);

        // Calculate risk per unit
        self.risk_per_unit = (self.entry_price - self.stop_loss_price).abs();

        if self.risk_per_unit == 0.0 {
            self.errors.push("Entry and stop loss cannot be equal".to_string());
            return;
        }

        // Calculate position size
        self.position_size = self.risk_amount / self.risk_per_unit;

        // Calculate margin if leverage is specified
        if let Some(lev) = self.leverage {
            if lev > 0 {
                self.margin_required = (self.position_size * self.entry_price) / lev as f64;
            } else {
                self.margin_required = self.position_size * self.entry_price;
            }
        } else {
            self.margin_required = self.position_size * self.entry_price;
        }

        // Calculate potential profit and R:R ratio if take profit is set
        if let Some(tp) = self.take_profit_price {
            if tp > 0.0 {
                let profit = (tp - self.entry_price).abs() * self.position_size;
                self.potential_profit = Some(profit);
                self.risk_reward_ratio = Some(profit / self.risk_amount);
            }
        }
    }

    /// Format output value as string
    pub fn format_output(&self, field: &str) -> String {
        match field {
            "risk_amount" => format!("${:.2}", self.risk_amount),
            "position_size" => format!("{:.4}", self.position_size),
            "risk_per_unit" => format!("${:.2}", self.risk_per_unit),
            "margin_required" => format!("${:.2}", self.margin_required),
            "potential_profit" => {
                if let Some(profit) = self.potential_profit {
                    format!("${:.2}", profit)
                } else {
                    "N/A".to_string()
                }
            }
            "risk_reward_ratio" => {
                if let Some(rr) = self.risk_reward_ratio {
                    format!("1:{:.2}", rr)
                } else {
                    "N/A".to_string()
                }
            }
            _ => "Unknown".to_string(),
        }
    }

    /// Get color for risk:reward ratio display
    pub fn risk_color(&self) -> [f32; 4] {
        if let Some(rr) = self.risk_reward_ratio {
            if rr >= 2.0 {
                [0.0, 0.8, 0.0, 1.0] // Green (good R:R)
            } else if rr >= 1.0 {
                [0.8, 0.8, 0.0, 1.0] // Yellow (acceptable)
            } else {
                [0.8, 0.0, 0.0, 1.0] // Red (poor R:R)
            }
        } else {
            [0.5, 0.5, 0.5, 1.0] // Gray (not set)
        }
    }
}

impl Default for RiskCalculatorState {
    fn default() -> Self {
        Self::new()
    }
}

/// RiskCalculator panel configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RiskCalculatorConfig {
    /// Default risk percent
    pub default_risk_percent: f64,

    /// Max risk percent allowed
    pub max_risk_percent: f64,

    /// Show leverage fields
    pub show_leverage: bool,

    /// Color coding for R:R ratio
    pub good_rr_threshold: f64,  // e.g., 2.0 (1:2 or better)
}

impl Default for RiskCalculatorConfig {
    fn default() -> Self {
        Self {
            default_risk_percent: 2.0,
            max_risk_percent: 10.0,
            show_leverage: false,
            good_rr_threshold: 2.0,
        }
    }
}

/// RiskCalculator panel wrapper (lightweight, lives in PanelKind)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RiskCalculatorPanel {
    id: RiskCalculatorId,
    title: String,
}

impl RiskCalculatorPanel {
    pub fn new(id: RiskCalculatorId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> RiskCalculatorId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "risk_calculator" }
    pub fn kind_label(&self) -> &'static str { "Risk Calculator" }
    pub fn min_size(&self) -> (f32, f32) { (250.0, 200.0) }
}
