use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MarketOverviewId(pub u64);

#[derive(Clone, Debug)]
pub struct MarketOverviewState {
    /// Major indices
    pub indices: HashMap<String, IndexData>,
    /// Sector performance
    pub sectors: Vec<SectorData>,
    /// Top gainers
    pub top_gainers: Vec<SymbolSnapshot>,
    /// Top losers
    pub top_losers: Vec<SymbolSnapshot>,
    /// Most active
    pub most_active: Vec<SymbolSnapshot>,
}

#[derive(Clone, Debug)]
pub struct IndexData {
    pub symbol: String,
    pub name: String,
    pub price: f64,
    pub change: f64,
    pub change_percent: f64,
}

#[derive(Clone, Debug)]
pub struct SectorData {
    pub name: String,
    pub change_percent: f64,
}

#[derive(Clone, Debug)]
pub struct SymbolSnapshot {
    pub symbol: String,
    pub price: f64,
    pub change_percent: f64,
    pub volume: f64,
}

impl MarketOverviewState {
    pub fn new() -> Self {
        Self {
            indices: HashMap::new(),
            sectors: Vec::new(),
            top_gainers: Vec::new(),
            top_losers: Vec::new(),
            most_active: Vec::new(),
        }
    }

    /// Get sections for rendering (indices, sectors, movers)
    pub fn sections(&self) -> Vec<Section> {
        let mut sections = Vec::new();

        if !self.indices.is_empty() {
            sections.push(Section::Indices);
        }
        if !self.sectors.is_empty() {
            sections.push(Section::Sectors);
        }
        if !self.top_gainers.is_empty() {
            sections.push(Section::TopGainers);
        }
        if !self.top_losers.is_empty() {
            sections.push(Section::TopLosers);
        }
        if !self.most_active.is_empty() {
            sections.push(Section::MostActive);
        }

        sections
    }

    /// Format section data for display
    pub fn format_section(&self, section: Section) -> Vec<(String, String, String)> {
        match section {
            Section::Indices => {
                self.indices.values()
                    .map(|idx| (
                        idx.symbol.clone(),
                        format!("{:.2}", idx.price),
                        format!("{:+.2}%", idx.change_percent)
                    ))
                    .collect()
            }
            Section::Sectors => {
                self.sectors.iter()
                    .map(|s| (
                        s.name.clone(),
                        "".to_string(),
                        format!("{:+.2}%", s.change_percent)
                    ))
                    .collect()
            }
            Section::TopGainers => {
                self.top_gainers.iter()
                    .map(|s| (
                        s.symbol.clone(),
                        format!("{:.2}", s.price),
                        format!("{:+.2}%", s.change_percent)
                    ))
                    .collect()
            }
            Section::TopLosers => {
                self.top_losers.iter()
                    .map(|s| (
                        s.symbol.clone(),
                        format!("{:.2}", s.price),
                        format!("{:+.2}%", s.change_percent)
                    ))
                    .collect()
            }
            Section::MostActive => {
                self.most_active.iter()
                    .map(|s| (
                        s.symbol.clone(),
                        format!("{:.2}", s.price),
                        format!("{:.0}", s.volume)
                    ))
                    .collect()
            }
        }
    }

    /// Get color based on change percentage
    pub fn change_color(&self, change_pct: f64) -> [f32; 4] {
        if change_pct > 0.0 {
            [0.2, 0.8, 0.3, 1.0] // green
        } else if change_pct < 0.0 {
            [0.9, 0.2, 0.2, 1.0] // red
        } else {
            [0.6, 0.6, 0.7, 1.0] // neutral
        }
    }
}

#[derive(Clone, Debug, Copy)]
pub enum Section {
    Indices,
    Sectors,
    TopGainers,
    TopLosers,
    MostActive,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketOverviewConfig {
    /// Indices to show
    pub tracked_indices: Vec<String>,
    /// Number of gainers/losers to show
    pub top_n: usize,
    /// Refresh interval
    pub refresh_interval: u64,
    /// Show sector heatmap
    pub show_sector_heatmap: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketOverviewPanel {
    id: MarketOverviewId,
    title: String,
}

impl MarketOverviewPanel {
    pub fn new(id: MarketOverviewId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> MarketOverviewId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "market_overview"
    }

    pub fn kind_label(&self) -> &'static str {
        "Market Overview"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (350.0, 250.0)
    }
}
