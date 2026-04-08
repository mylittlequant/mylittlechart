use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PortfolioOverviewId(pub u64);

#[derive(Clone, Debug)]
pub struct PortfolioOverviewState {
    /// All balances
    pub balances: Vec<Balance>,
    /// Price data for USD conversion
    pub prices: HashMap<String, f64>,
    /// Enriched balances with USD value
    pub portfolio_items: Vec<PortfolioItem>,
    /// Total portfolio value
    pub total_value_usd: f64,
    /// Sort configuration
    pub sort: (PortfolioColumn, bool),
}

#[derive(Clone, Debug)]
pub struct Balance {
    pub asset: String,
    pub free: f64,
    pub locked: f64,
    pub total: f64,
}

#[derive(Clone, Debug)]
pub struct PortfolioItem {
    pub asset: String,
    pub free: f64,
    pub locked: f64,
    pub total: f64,
    pub usd_value: f64,
    pub percent_of_portfolio: f64,
}

#[derive(Clone, Debug, Copy)]
pub enum PortfolioColumn {
    Asset,
    Free,
    Locked,
    Total,
    UsdValue,
    PercentOfPortfolio,
}

impl PortfolioOverviewState {
    pub fn new() -> Self {
        Self {
            balances: Vec::new(),
            prices: HashMap::new(),
            portfolio_items: Vec::new(),
            total_value_usd: 0.0,
            sort: (PortfolioColumn::UsdValue, false),
        }
    }

    /// Get visible assets for rendering
    pub fn visible_assets(&self, scroll_offset: usize, max_rows: usize) -> &[PortfolioItem] {
        let end = (scroll_offset + max_rows).min(self.portfolio_items.len());
        &self.portfolio_items[scroll_offset..end]
    }

    /// Format asset field for display
    pub fn format_asset(&self, item: &PortfolioItem, column: PortfolioColumn) -> String {
        match column {
            PortfolioColumn::Asset => item.asset.clone(),
            PortfolioColumn::Free => format!("{:.4}", item.free),
            PortfolioColumn::Locked => format!("{:.4}", item.locked),
            PortfolioColumn::Total => format!("{:.4}", item.total),
            PortfolioColumn::UsdValue => format!("${:.2}", item.usd_value),
            PortfolioColumn::PercentOfPortfolio => format!("{:.1}%", item.percent_of_portfolio),
        }
    }

    /// Calculate allocation percentage
    pub fn allocation_pct(&self, item: &PortfolioItem) -> f64 {
        item.percent_of_portfolio
    }

    /// Get color for allocation bar
    pub fn allocation_color(&self, index: usize) -> [f32; 4] {
        // Cycle through distinct colors for different assets
        let colors = [
            [0.3, 0.6, 0.9, 1.0], // blue
            [0.2, 0.8, 0.3, 1.0], // green
            [0.9, 0.5, 0.2, 1.0], // orange
            [0.8, 0.2, 0.8, 1.0], // purple
            [0.9, 0.7, 0.2, 1.0], // yellow
            [0.2, 0.8, 0.8, 1.0], // cyan
        ];
        colors[index % colors.len()]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortfolioOverviewConfig {
    /// Hide zero balances
    pub hide_zero_balances: bool,
    /// Hide dust (<$1)
    pub hide_dust: bool,
    /// Show locked column
    pub show_locked: bool,
    /// Price update interval
    pub price_update_interval: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortfolioOverviewPanel {
    id: PortfolioOverviewId,
    title: String,
}

impl PortfolioOverviewPanel {
    pub fn new(id: PortfolioOverviewId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> PortfolioOverviewId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "portfolio_overview"
    }

    pub fn kind_label(&self) -> &'static str {
        "Portfolio"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 250.0)
    }
}
