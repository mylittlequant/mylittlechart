use serde::{Serialize, Deserialize};
use std::collections::VecDeque;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GreeksPanelId(pub u64);

#[derive(Clone, Debug)]
pub struct GreeksPanelState {
    /// Selected option contract
    pub contract: Option<OptionContract>,
    /// Real-time Greeks
    pub greeks: Option<Greeks>,
    /// Historical Greeks (for trend)
    pub greeks_history: VecDeque<(i64, Greeks)>,
}

#[derive(Clone, Debug)]
pub struct OptionContract {
    pub symbol: String,
    pub strike: f64,
    pub expiration: String,
    pub option_type: OptionType,
}

#[derive(Clone, Debug)]
pub enum OptionType {
    Call,
    Put,
}

#[derive(Clone, Debug)]
pub struct Greeks {
    pub delta: f64,
    pub gamma: f64,
    pub theta: f64,
    pub vega: f64,
    pub rho: f64,
    pub implied_volatility: f64,
    pub intrinsic_value: f64,
    pub extrinsic_value: f64,
    pub time_value: f64,
}

impl GreeksPanelState {
    pub fn new() -> Self {
        Self {
            contract: None,
            greeks: None,
            greeks_history: VecDeque::new(),
        }
    }

    /// Get list of Greeks as key-value pairs for rendering
    pub fn greeks_list(&self) -> Vec<(&'static str, String)> {
        if let Some(ref g) = self.greeks {
            vec![
                ("Delta", format!("{:.4}", g.delta)),
                ("Gamma", format!("{:.4}", g.gamma)),
                ("Theta", format!("{:.4}", g.theta)),
                ("Vega", format!("{:.4}", g.vega)),
                ("Rho", format!("{:.4}", g.rho)),
                ("Implied Vol", format!("{:.1}%", g.implied_volatility * 100.0)),
                ("Intrinsic", format!("{:.2}", g.intrinsic_value)),
                ("Extrinsic", format!("{:.2}", g.extrinsic_value)),
                ("Time Value", format!("{:.2}", g.time_value)),
            ]
        } else {
            vec![("Status", "No contract selected".to_string())]
        }
    }

    /// Format a single Greek value
    pub fn format_greek(&self, key: &str) -> String {
        if let Some(ref g) = self.greeks {
            match key {
                "Delta" => format!("{:.4}", g.delta),
                "Gamma" => format!("{:.4}", g.gamma),
                "Theta" => format!("{:.4}", g.theta),
                "Vega" => format!("{:.4}", g.vega),
                "Rho" => format!("{:.4}", g.rho),
                "Implied Vol" => format!("{:.1}%", g.implied_volatility * 100.0),
                "Intrinsic" => format!("{:.2}", g.intrinsic_value),
                "Extrinsic" => format!("{:.2}", g.extrinsic_value),
                "Time Value" => format!("{:.2}", g.time_value),
                _ => "—".to_string(),
            }
        } else {
            "—".to_string()
        }
    }

    /// Get color for delta gauge (normalized -1 to 1)
    pub fn delta_color(&self) -> [f32; 4] {
        if let Some(ref g) = self.greeks {
            if g.delta > 0.5 {
                [0.2, 0.8, 0.3, 1.0] // green - bullish
            } else if g.delta < -0.5 {
                [0.9, 0.2, 0.2, 1.0] // red - bearish
            } else {
                [0.9, 0.7, 0.2, 1.0] // yellow - neutral
            }
        } else {
            [0.6, 0.6, 0.7, 1.0]
        }
    }

    /// Get normalized delta value for gauge rendering (0.0 to 1.0)
    pub fn delta_normalized(&self) -> f32 {
        if let Some(ref g) = self.greeks {
            ((g.delta + 1.0) / 2.0).clamp(0.0, 1.0) as f32
        } else {
            0.5
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GreeksConfig {
    /// Show extended Greeks
    pub show_extended: bool,
    /// Show Greek gauges (visual)
    pub show_gauges: bool,
    /// Greek trend sparklines
    pub show_trends: bool,
    /// Greeks refresh rate
    pub refresh_rate: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GreeksPanelPanel {
    id: GreeksPanelId,
    title: String,
}

impl GreeksPanelPanel {
    pub fn new(id: GreeksPanelId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> GreeksPanelId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "greeks_panel"
    }

    pub fn kind_label(&self) -> &'static str {
        "Greeks"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 200.0)
    }
}
