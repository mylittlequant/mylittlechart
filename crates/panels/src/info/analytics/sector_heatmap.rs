use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SectorHeatmapId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct SectorHeatmapState {
    pub sectors: HashMap<String, SectorData>,
    pub view_mode: ViewMode,
    pub time_frame: TimeFrame,
}

#[derive(Clone, Debug)]
pub struct SectorData {
    pub name: String,
    pub stocks: Vec<StockData>,
    pub total_market_cap: f64,
    pub avg_change_pct: f64,
}

#[derive(Clone, Debug)]
pub struct StockData {
    pub symbol: String,
    pub name: String,
    pub market_cap: f64,
    pub price: f64,
    pub change_pct: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ViewMode {
    SectorLevel,  // Show only sectors
    StockLevel,   // Show stocks within sectors
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TimeFrame {
    Today,
    Week,
    Month,
    YTD,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SectorHeatmapConfig {
    pub color_scale: DivergingColorScale,
    pub min_stock_area_px: f32,  // Min area to show stock label
    pub show_sector_borders: bool,
    pub border_width: f32,
    pub tile_algorithm: TileAlgorithm,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DivergingColorScale {
    pub negative: [f32; 3],  // OKLCH for -5%
    pub neutral: [f32; 3],   // OKLCH for 0%
    pub positive: [f32; 3],  // OKLCH for +5%
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TileAlgorithm {
    Squarify,
    Slice,
    Dice,
    SliceDice,
}

impl Default for ViewMode {
    fn default() -> Self {
        Self::SectorLevel
    }
}

impl Default for TimeFrame {
    fn default() -> Self {
        Self::Today
    }
}

impl SectorHeatmapState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns sector rectangles for treemap rendering (x, y, w, h, color, label)
    pub fn sector_rects(&self, w: f32, h: f32) -> Vec<(f32, f32, f32, f32, [f32; 4], String)> {
        let mut rects = Vec::new();

        if self.sectors.is_empty() {
            return rects;
        }

        let total_market_cap: f64 = self.sectors.values().map(|s| s.total_market_cap).sum();

        if total_market_cap == 0.0 {
            return rects;
        }

        let mut sectors: Vec<_> = self.sectors.values().collect();
        sectors.sort_by(|a, b| b.total_market_cap.partial_cmp(&a.total_market_cap).unwrap_or(std::cmp::Ordering::Equal));

        let mut current_x = 0.0f32;
        let mut current_y = 0.0f32;
        let mut row_height = 0.0f32;

        for sector in sectors {
            let area_ratio = (sector.total_market_cap / total_market_cap) as f32;
            let area = w * h * area_ratio;

            let rect_w = (area / h).min(w - current_x);
            let rect_h = if rect_w > 0.0 { area / rect_w } else { 0.0 };

            if current_x + rect_w > w {
                current_y += row_height;
                current_x = 0.0;
                row_height = 0.0;
            }

            let change_pct = sector.avg_change_pct;
            let color = if change_pct > 0.0 {
                let intensity = (change_pct / 5.0).clamp(0.0, 1.0) as f32;
                [0.65, 0.15 * intensity, 145.0, 1.0]
            } else if change_pct < 0.0 {
                let intensity = (-change_pct / 5.0).clamp(0.0, 1.0) as f32;
                [0.60, 0.18 * intensity, 25.0, 1.0]
            } else {
                [0.5, 0.0, 0.0, 1.0]
            };

            rects.push((current_x, current_y, rect_w, rect_h, color, sector.name.clone()));

            current_x += rect_w;
            row_height = row_height.max(rect_h);
        }

        rects
    }

    /// Returns visible sectors (all sectors in current view)
    pub fn visible_sectors(&self) -> Vec<&SectorData> {
        self.sectors.values().collect()
    }

    /// Get color for a sector based on change percentage
    pub fn sector_color(&self, sector: &SectorData) -> [f32; 4] {
        let change_pct = sector.avg_change_pct;
        if change_pct > 0.0 {
            let intensity = (change_pct / 5.0).clamp(0.0, 1.0) as f32;
            [0.65, 0.15 * intensity, 145.0, 1.0]
        } else if change_pct < 0.0 {
            let intensity = (-change_pct / 5.0).clamp(0.0, 1.0) as f32;
            [0.60, 0.18 * intensity, 25.0, 1.0]
        } else {
            [0.5, 0.0, 0.0, 1.0]
        }
    }

    /// Format sector change for display
    pub fn format_change(&self, change_pct: f64) -> String {
        format!("{:+.2}%", change_pct)
    }
}

impl Default for SectorHeatmapConfig {
    fn default() -> Self {
        Self {
            color_scale: DivergingColorScale {
                negative: [0.60, 0.18, 25.0],   // Red
                neutral: [0.5, 0.0, 0.0],       // Gray
                positive: [0.65, 0.15, 145.0],  // Green
            },
            min_stock_area_px: 1000.0,
            show_sector_borders: true,
            border_width: 2.0,
            tile_algorithm: TileAlgorithm::Squarify,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SectorHeatmapPanel {
    id: SectorHeatmapId,
    title: String,
}

impl SectorHeatmapPanel {
    pub fn new(id: SectorHeatmapId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> SectorHeatmapId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "sector_heatmap"
    }

    pub fn kind_label(&self) -> &'static str {
        "Sector Heatmap"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
