use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PnlSurfaceId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct PnlSurfaceState {
    pub strategy: OptionsStrategy,
    pub surface_data: Vec<Vec<f64>>,  // [price_idx][time_idx] = PnL
    pub underlying_prices: Vec<f64>,
    pub times_to_expiry: Vec<f64>,  // Days until expiration
    pub current_spot: f64,
    pub rotation: (f32, f32),
    pub zoom: f32,
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
pub struct PnlSurfaceConfig {
    pub price_range_pct: f64,  // e.g., ±20% from current spot
    pub time_range_days: f64,  // e.g., 0 to 30 days
    pub color_gradient: PnlGradient,
    pub show_breakeven_contour: bool,
    pub grid_resolution: (usize, usize),  // (price_steps, time_steps)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PnlGradient {
    pub loss: [f32; 3],   // OKLCH: deep red
    pub neutral: [f32; 3], // OKLCH: gray
    pub profit: [f32; 3],  // OKLCH: green
}

impl PnlSurfaceState {
    pub fn new() -> Self {
        Self {
            zoom: 1.0,
            ..Default::default()
        }
    }

    /// Returns PnL value at the given underlying price and days to expiry
    pub fn pnl_at(&self, underlying_price: f64, days_to_expiry: f64) -> f64 {
        if self.underlying_prices.is_empty() || self.times_to_expiry.is_empty() || self.surface_data.is_empty() {
            return 0.0;
        }

        let price_idx = self.underlying_prices.iter()
            .position(|&p| p >= underlying_price)
            .unwrap_or(self.underlying_prices.len().saturating_sub(1));

        let time_idx = self.times_to_expiry.iter()
            .position(|&t| t >= days_to_expiry)
            .unwrap_or(self.times_to_expiry.len().saturating_sub(1));

        if price_idx >= self.surface_data.len() {
            return 0.0;
        }

        if time_idx >= self.surface_data[price_idx].len() {
            return 0.0;
        }

        self.surface_data[price_idx][time_idx]
    }

    /// Returns surface grid points for rendering
    pub fn surface_grid(&self, w: f32, h: f32) -> Vec<(f32, f32, f64)> {
        let mut points = Vec::new();

        if self.surface_data.is_empty() || self.underlying_prices.is_empty() || self.times_to_expiry.is_empty() {
            return points;
        }

        let price_steps = self.underlying_prices.len();
        let time_steps = self.times_to_expiry.len();

        for (p_idx, row) in self.surface_data.iter().enumerate() {
            if p_idx >= price_steps {
                break;
            }
            for (t_idx, &pnl) in row.iter().enumerate() {
                if t_idx >= time_steps {
                    break;
                }
                let x = (p_idx as f32 / price_steps as f32) * w;
                let y = (t_idx as f32 / time_steps as f32) * h;
                points.push((x, y, pnl));
            }
        }

        points
    }

    /// Returns color for PnL value (green positive, red negative)
    pub fn pnl_color(&self, pnl: f64) -> [f32; 4] {
        if pnl > 0.0 {
            let intensity = (pnl / 1000.0).clamp(0.0, 1.0) as f32;
            [0.65, 0.15, 145.0, 0.5 + intensity * 0.5]
        } else if pnl < 0.0 {
            let intensity = (-pnl / 1000.0).clamp(0.0, 1.0) as f32;
            [0.60, 0.18, 25.0, 0.5 + intensity * 0.5]
        } else {
            [0.5, 0.0, 0.0, 0.5]
        }
    }

    /// Returns breakeven line points where PnL = 0
    pub fn breakeven_line(&self, w: f32, h: f32) -> Vec<(f32, f32)> {
        let mut points = Vec::new();

        if self.surface_data.is_empty() || self.underlying_prices.is_empty() {
            return points;
        }

        for (p_idx, row) in self.surface_data.iter().enumerate() {
            for (t_idx, &pnl) in row.iter().enumerate() {
                if pnl.abs() < 1.0 { // Close to breakeven
                    let x = (p_idx as f32 / self.underlying_prices.len() as f32) * w;
                    let y = (t_idx as f32 / row.len() as f32) * h;
                    points.push((x, y));
                }
            }
        }

        points
    }
}

impl Default for PnlSurfaceConfig {
    fn default() -> Self {
        Self {
            price_range_pct: 0.2,
            time_range_days: 30.0,
            color_gradient: PnlGradient {
                loss: [0.60, 0.18, 25.0],   // Red
                neutral: [0.5, 0.0, 0.0],   // Gray
                profit: [0.65, 0.15, 145.0], // Green
            },
            show_breakeven_contour: true,
            grid_resolution: (50, 50),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PnlSurfacePanel {
    id: PnlSurfaceId,
    title: String,
}

impl PnlSurfacePanel {
    pub fn new(id: PnlSurfaceId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> PnlSurfaceId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "pnl_surface"
    }

    pub fn kind_label(&self) -> &'static str {
        "PnL Surface"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
