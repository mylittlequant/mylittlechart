use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PayoffDiagramId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct PayoffDiagramState {
    pub strategy: OptionsStrategy,
    pub payoff_curve: Vec<(f64, f64)>,  // (underlying_price, pnl)
    pub current_spot: f64,
    pub breakeven_points: Vec<f64>,
    pub max_profit: Option<f64>,  // None if unlimited
    pub max_loss: Option<f64>,    // None if unlimited
}

#[derive(Clone, Debug, Default)]
pub struct OptionsStrategy {
    pub legs: Vec<OptionLeg>,
}

#[derive(Clone, Debug)]
pub struct OptionLeg {
    pub option_type: OptionType,  // Call or Put
    pub strike: f64,
    pub expiry: i64,  // Unix timestamp
    pub quantity: i32,  // Positive for long, negative for short
    pub premium: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OptionType {
    Call,
    Put,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PayoffDiagramConfig {
    pub price_range_pct: f64,  // ±% from current spot
    pub grid_resolution: usize,  // Number of price points to sample
    pub profit_color: [f32; 3],  // OKLCH
    pub loss_color: [f32; 3],    // OKLCH
    pub fill_areas: bool,  // Fill profit/loss zones
    pub show_current_pnl: bool,  // Mark current PnL at spot price
}

impl PayoffDiagramState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns payoff curve points in screen coordinates
    pub fn payoff_points(&self, w: f32, h: f32) -> Vec<(f32, f32)> {
        if self.payoff_curve.is_empty() {
            return Vec::new();
        }

        let (min_price, max_price) = self.payoff_curve.iter()
            .fold((f64::MAX, f64::MIN), |(min, max), (p, _)| {
                (min.min(*p), max.max(*p))
            });

        let (min_pnl, max_pnl) = self.payoff_curve.iter()
            .fold((f64::MAX, f64::MIN), |(min, max), (_, pnl)| {
                (min.min(*pnl), max.max(*pnl))
            });

        let price_range = max_price - min_price;
        let pnl_range = max_pnl - min_pnl;

        if price_range == 0.0 || pnl_range == 0.0 {
            return Vec::new();
        }

        self.payoff_curve.iter()
            .map(|(price, pnl)| {
                let x = (((price - min_price) / price_range) as f32 * w).clamp(0.0, w);
                let y = (((1.0 - (pnl - min_pnl) / pnl_range) * h as f64) as f32).clamp(0.0, h);
                (x, y)
            })
            .collect()
    }

    /// Returns X coordinates for breakeven points
    pub fn breakeven_x(&self, w: f32) -> Vec<f32> {
        if self.payoff_curve.is_empty() {
            return Vec::new();
        }

        let (min_price, max_price) = self.payoff_curve.iter()
            .fold((f64::MAX, f64::MIN), |(min, max), (p, _)| {
                (min.min(*p), max.max(*p))
            });

        let price_range = max_price - min_price;
        if price_range == 0.0 {
            return Vec::new();
        }

        self.breakeven_points.iter()
            .map(|price| {
                (((price - min_price) / price_range) as f32 * w).clamp(0.0, w)
            })
            .collect()
    }

    /// Get breakeven price(s) as formatted strings
    pub fn breakeven_price(&self) -> Vec<String> {
        self.breakeven_points.iter()
            .map(|price| format!("{:.2}", price))
            .collect()
    }

    /// Get max profit value (formatted)
    pub fn max_profit(&self) -> String {
        match self.max_profit {
            Some(profit) => format!("{:.2}", profit),
            None => "Unlimited".to_string(),
        }
    }

    /// Get max loss value (formatted)
    pub fn max_loss(&self) -> String {
        match self.max_loss {
            Some(loss) => format!("{:.2}", loss),
            None => "Unlimited".to_string(),
        }
    }
}

impl Default for PayoffDiagramConfig {
    fn default() -> Self {
        Self {
            price_range_pct: 0.25,
            grid_resolution: 100,
            profit_color: [0.65, 0.15, 145.0],  // Green
            loss_color: [0.60, 0.18, 25.0],     // Red
            fill_areas: true,
            show_current_pnl: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PayoffDiagramPanel {
    id: PayoffDiagramId,
    title: String,
}

impl PayoffDiagramPanel {
    pub fn new(id: PayoffDiagramId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> PayoffDiagramId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "payoff_diagram"
    }

    pub fn kind_label(&self) -> &'static str {
        "Payoff Diagram"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 250.0)
    }
}
