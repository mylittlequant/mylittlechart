use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountSummaryId(pub u64);

#[derive(Clone, Debug)]
pub struct AccountSummaryState {
    /// Account information
    pub account_info: Option<AccountInfo>,
    /// All balances
    pub balances: Vec<Balance>,
    /// Total equity in USD
    pub total_equity_usd: f64,
    /// Available balance
    pub available_balance: f64,
    /// Used margin
    pub used_margin: f64,
    /// Unrealized PnL (from positions)
    pub unrealized_pnl: f64,
    /// Margin ratio (used / equity)
    pub margin_ratio: f64,
}

#[derive(Clone, Debug)]
pub struct AccountInfo {
    pub can_trade: bool,
    pub can_withdraw: bool,
    pub account_type: String,
}

#[derive(Clone, Debug)]
pub struct Balance {
    pub asset: String,
    pub free: f64,
    pub locked: f64,
    pub total: f64,
}

impl AccountSummaryState {
    pub fn new() -> Self {
        Self {
            account_info: None,
            balances: Vec::new(),
            total_equity_usd: 0.0,
            available_balance: 0.0,
            used_margin: 0.0,
            unrealized_pnl: 0.0,
            margin_ratio: 0.0,
        }
    }

    /// Get list of key metrics for rendering
    pub fn metrics_list(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Total Equity", format!("${:.2}", self.total_equity_usd)),
            ("Available", format!("${:.2}", self.available_balance)),
            ("Used Margin", format!("${:.2}", self.used_margin)),
            ("Unrealized PnL", self.format_metric_pnl(self.unrealized_pnl)),
            ("Margin Ratio", format!("{:.1}%", self.margin_ratio * 100.0)),
        ]
    }

    /// Format a single metric value
    pub fn format_metric(&self, key: &str) -> String {
        match key {
            "Total Equity" => format!("${:.2}", self.total_equity_usd),
            "Available" => format!("${:.2}", self.available_balance),
            "Used Margin" => format!("${:.2}", self.used_margin),
            "Unrealized PnL" => self.format_metric_pnl(self.unrealized_pnl),
            "Margin Ratio" => format!("{:.1}%", self.margin_ratio * 100.0),
            _ => "—".to_string(),
        }
    }

    fn format_metric_pnl(&self, pnl: f64) -> String {
        format!("{:+.2}", pnl)
    }

    /// Get color for PnL value
    pub fn pnl_color(&self) -> [f32; 4] {
        if self.unrealized_pnl > 0.0 {
            [0.2, 0.8, 0.3, 1.0]
        } else if self.unrealized_pnl < 0.0 {
            [0.9, 0.2, 0.2, 1.0]
        } else {
            [0.6, 0.6, 0.7, 1.0]
        }
    }

    /// Get color for margin ratio (warning levels)
    pub fn margin_ratio_color(&self) -> [f32; 4] {
        if self.margin_ratio > 0.8 {
            [0.9, 0.2, 0.2, 1.0] // red - danger
        } else if self.margin_ratio > 0.6 {
            [0.9, 0.7, 0.2, 1.0] // yellow - warning
        } else {
            [0.6, 0.6, 0.7, 1.0] // neutral
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountSummaryConfig {
    /// Display currency (USD, BTC, etc.)
    pub display_currency: String,
    /// Refresh interval (seconds)
    pub refresh_interval: u64,
    /// Show zero balances
    pub show_zero_balances: bool,
    /// Margin warning level
    pub margin_warning_level: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountSummaryPanel {
    id: AccountSummaryId,
    title: String,
}

impl AccountSummaryPanel {
    pub fn new(id: AccountSummaryId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> AccountSummaryId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "account_summary"
    }

    pub fn kind_label(&self) -> &'static str {
        "Account"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (250.0, 150.0)
    }
}
