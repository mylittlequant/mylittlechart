use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnalystRatingsId(pub u64);

#[derive(Clone, Debug)]
pub struct AnalystRatingsState {
    /// Symbol being tracked
    pub symbol: String,
    /// Analyst ratings
    pub ratings: Vec<AnalystRating>,
    /// Consensus summary
    pub consensus: Option<RatingConsensus>,
    /// Time range filter
    pub time_range: TimeRange,
}

#[derive(Clone, Debug)]
pub struct AnalystRating {
    pub id: String,
    pub date: i64,
    pub analyst: String,
    pub firm: String,
    pub rating: Rating,
    pub price_target: Option<f64>,
    pub previous_rating: Option<Rating>,
    pub previous_target: Option<f64>,
}

#[derive(Clone, Debug)]
pub enum Rating {
    StrongBuy,
    Buy,
    Hold,
    Sell,
    StrongSell,
}

#[derive(Clone, Debug)]
pub struct RatingConsensus {
    pub symbol: String,
    pub average_rating: f64,
    pub average_target: f64,
    pub num_analysts: u32,
    pub buy_count: u32,
    pub hold_count: u32,
    pub sell_count: u32,
}

#[derive(Clone, Debug)]
pub enum TimeRange {
    Week,
    Month,
    Quarter,
    Year,
}

impl AnalystRatingsState {
    pub fn new() -> Self {
        Self {
            symbol: String::new(),
            ratings: Vec::new(),
            consensus: None,
            time_range: TimeRange::Month,
        }
    }

    /// Get visible ratings for rendering
    pub fn visible_ratings(&self, scroll_offset: usize, max_rows: usize) -> &[AnalystRating] {
        let end = (scroll_offset + max_rows).min(self.ratings.len());
        &self.ratings[scroll_offset..end]
    }

    /// Format rating for display
    pub fn format_rating(&self, rating: &AnalystRating) -> (String, String, String, String, String) {
        let date = format_date(rating.date);
        let analyst = rating.analyst.clone();
        let firm = rating.firm.clone();
        let rating_str = self.format_rating_enum(&rating.rating);
        let target = rating.price_target
            .map(|t| format!("${:.2}", t))
            .unwrap_or_else(|| "—".to_string());
        (date, analyst, firm, rating_str, target)
    }

    fn format_rating_enum(&self, rating: &Rating) -> String {
        match rating {
            Rating::StrongBuy => "Strong Buy".to_string(),
            Rating::Buy => "Buy".to_string(),
            Rating::Hold => "Hold".to_string(),
            Rating::Sell => "Sell".to_string(),
            Rating::StrongSell => "Strong Sell".to_string(),
        }
    }

    /// Get color based on rating
    pub fn rating_color(&self, rating: &AnalystRating) -> [f32; 4] {
        match rating.rating {
            Rating::StrongBuy => [0.0, 0.7, 0.2, 1.0],  // dark green
            Rating::Buy => [0.2, 0.8, 0.3, 1.0],        // green
            Rating::Hold => [0.9, 0.7, 0.2, 1.0],       // yellow
            Rating::Sell => [0.9, 0.2, 0.2, 1.0],       // red
            Rating::StrongSell => [0.7, 0.0, 0.0, 1.0], // dark red
        }
    }

    /// Check if rating changed (upgrade/downgrade)
    pub fn is_upgrade(&self, rating: &AnalystRating) -> Option<bool> {
        rating.previous_rating.as_ref().map(|prev| {
            rating_score(&rating.rating) > rating_score(prev)
        })
    }
}

fn rating_score(rating: &Rating) -> i32 {
    match rating {
        Rating::StrongSell => 1,
        Rating::Sell => 2,
        Rating::Hold => 3,
        Rating::Buy => 4,
        Rating::StrongBuy => 5,
    }
}

fn format_date(ts: i64) -> String {
    let days = ts / 86400000;
    format!("Day {}", days % 365)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnalystRatingsConfig {
    /// Show previous ratings
    pub show_previous: bool,
    /// Highlight upgrades/downgrades
    pub highlight_changes: bool,
    /// Minimum firm tier filter
    pub min_firm_tier: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnalystRatingsPanel {
    id: AnalystRatingsId,
    title: String,
}

impl AnalystRatingsPanel {
    pub fn new(id: AnalystRatingsId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> AnalystRatingsId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "analyst_ratings"
    }

    pub fn kind_label(&self) -> &'static str {
        "Analyst Ratings"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (250.0, 200.0)
    }
}
