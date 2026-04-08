use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JournalId(pub u64);

/// Journal sort order
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JournalSortOrder {
    DateDesc,
    DateAsc,
    PnLDesc,
    PnLAsc,
    SymbolAsc,
}

/// Configuration for journal panel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalConfig {
    pub auto_save_interval_secs: u64,
    pub default_sort: JournalSortOrder,
    pub show_statistics: bool,
    pub required_fields: Vec<String>,
}

impl Default for JournalConfig {
    fn default() -> Self {
        Self {
            auto_save_interval_secs: 60,
            default_sort: JournalSortOrder::DateDesc,
            show_statistics: true,
            required_fields: vec![
                "symbol".to_string(),
                "direction".to_string(),
                "entry_reasoning".to_string(),
            ],
        }
    }
}

/// Trade direction
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TradeDirection {
    Long,
    Short,
}

/// Trade quality rating
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TradeQuality {
    A,
    B,
    C,
    F,
}

/// Trade outcome
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TradeOutcome {
    Win,
    Loss,
    Breakeven,
}

/// Mood
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Mood {
    Confident,
    Anxious,
    Fearful,
    Greedy,
    Neutral,
    Frustrated,
    Euphoric,
}

/// Emotional state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmotionalState {
    pub pre_trade_mood: Mood,
    pub during_trade_mood: Mood,
    pub post_trade_mood: Mood,
    pub confidence_level: u8,
    pub stress_level: u8,
}

/// Market conditions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketConditions {
    pub trend: String,
    pub volatility: String,
    pub volume: String,
    pub news_events: Vec<String>,
}

/// Journal entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub trade_id: Option<String>,
    pub symbol: String,
    pub direction: TradeDirection,
    pub entry_price: f64,
    pub exit_price: Option<f64>,
    pub quantity: f64,
    pub pnl: Option<f64>,
    pub pnl_percentage: Option<f64>,
    pub fees: f64,
    pub holding_time_mins: Option<u64>,
    pub setup: String,
    pub strategy: String,
    pub thesis: String,
    pub planned_entry: f64,
    pub planned_stop_loss: f64,
    pub planned_take_profit: f64,
    pub risk_reward_ratio: f64,
    pub entry_reasoning: String,
    pub exit_reasoning: String,
    pub trade_quality: TradeQuality,
    pub mistakes: Vec<String>,
    pub outcome: TradeOutcome,
    pub lessons: String,
    pub followed_plan: bool,
    pub emotional_state: EmotionalState,
    pub market_conditions: MarketConditions,
    pub screenshots: Vec<String>,
    pub tags: Vec<String>,
    pub notes: String,
}

/// Journal filter
#[derive(Debug, Clone)]
pub struct JournalFilter {
    pub date_range: Option<(i64, i64)>,
    pub symbols: Vec<String>,
    pub strategies: Vec<String>,
    pub tags: Vec<String>,
    pub outcome: Option<TradeOutcome>,
    pub min_pnl: Option<f64>,
    pub max_pnl: Option<f64>,
}

impl Default for JournalFilter {
    fn default() -> Self {
        Self {
            date_range: None,
            symbols: Vec::new(),
            strategies: Vec::new(),
            tags: Vec::new(),
            outcome: None,
            min_pnl: None,
            max_pnl: None,
        }
    }
}

/// Journal statistics
#[derive(Debug, Clone, Default)]
pub struct JournalStatistics {
    pub total_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub breakeven_trades: usize,
    pub win_rate: f64,
    pub average_win: f64,
    pub average_loss: f64,
    pub profit_factor: f64,
    pub total_pnl: f64,
    pub largest_win: f64,
    pub largest_loss: f64,
    pub average_holding_time_mins: f64,
    pub best_strategy: String,
    pub worst_strategy: String,
    pub most_common_mistake: String,
}

/// Journal state
#[derive(Clone, Debug, Default)]
pub struct JournalState {
    pub entries: Vec<JournalEntry>,
    pub current_entry_id: Option<String>,
    pub filter: JournalFilter,
    pub sort_order: JournalSortOrder,
    pub dirty: bool,
    pub statistics: JournalStatistics,
}

impl Default for JournalSortOrder {
    fn default() -> Self {
        JournalSortOrder::DateDesc
    }
}

impl JournalState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get visible entries (filtered and sorted)
    pub fn visible_entries(&self) -> Vec<&JournalEntry> {
        let mut entries: Vec<&JournalEntry> = self.entries.iter()
            .filter(|entry| self.matches_filter(entry))
            .collect();

        // Sort based on sort_order
        match self.sort_order {
            JournalSortOrder::DateDesc => entries.sort_by(|a, b| b.created_at.cmp(&a.created_at)),
            JournalSortOrder::DateAsc => entries.sort_by(|a, b| a.created_at.cmp(&b.created_at)),
            JournalSortOrder::PnLDesc => entries.sort_by(|a, b| {
                b.pnl.unwrap_or(0.0).partial_cmp(&a.pnl.unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal)
            }),
            JournalSortOrder::PnLAsc => entries.sort_by(|a, b| {
                a.pnl.unwrap_or(0.0).partial_cmp(&b.pnl.unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal)
            }),
            JournalSortOrder::SymbolAsc => entries.sort_by(|a, b| a.symbol.cmp(&b.symbol)),
        }

        entries
    }

    fn matches_filter(&self, entry: &JournalEntry) -> bool {
        if let Some((start, end)) = self.filter.date_range {
            if entry.created_at < start || entry.created_at > end {
                return false;
            }
        }

        if !self.filter.symbols.is_empty() && !self.filter.symbols.contains(&entry.symbol) {
            return false;
        }

        if !self.filter.strategies.is_empty() && !self.filter.strategies.contains(&entry.strategy) {
            return false;
        }

        if !self.filter.tags.is_empty() {
            let has_tag = entry.tags.iter().any(|tag| self.filter.tags.contains(tag));
            if !has_tag {
                return false;
            }
        }

        if let Some(outcome) = self.filter.outcome {
            if entry.outcome != outcome {
                return false;
            }
        }

        if let Some(min_pnl) = self.filter.min_pnl {
            if entry.pnl.unwrap_or(0.0) < min_pnl {
                return false;
            }
        }

        if let Some(max_pnl) = self.filter.max_pnl {
            if entry.pnl.unwrap_or(0.0) > max_pnl {
                return false;
            }
        }

        true
    }

    /// Get one-line summary of an entry
    pub fn entry_summary(&self, entry: &JournalEntry) -> String {
        let direction_str = match entry.direction {
            TradeDirection::Long => "LONG",
            TradeDirection::Short => "SHORT",
        };
        let pnl_str = entry.pnl.map(|p| format!("{:+.2}", p)).unwrap_or_else(|| "N/A".to_string());
        format!(
            "{} {} @ {:.2} | PnL: {} | {}",
            direction_str,
            entry.symbol,
            entry.entry_price,
            pnl_str,
            entry.setup
        )
    }

    /// Get aggregate statistics as label-value pairs
    pub fn stats_summary(&self) -> Vec<(&str, String)> {
        vec![
            ("Total Trades", self.statistics.total_trades.to_string()),
            ("Win Rate", format!("{:.1}%", self.statistics.win_rate * 100.0)),
            ("Avg Win", format!("{:.2}", self.statistics.average_win)),
            ("Avg Loss", format!("{:.2}", self.statistics.average_loss)),
            ("Profit Factor", format!("{:.2}", self.statistics.profit_factor)),
            ("Total P&L", format!("{:+.2}", self.statistics.total_pnl)),
            ("Largest Win", format!("{:.2}", self.statistics.largest_win)),
            ("Largest Loss", format!("{:.2}", self.statistics.largest_loss)),
        ]
    }

    /// Get color for mood (RGBA)
    pub fn mood_color(mood: &Mood) -> [f32; 4] {
        match mood {
            Mood::Confident | Mood::Neutral => [0.4, 0.8, 0.4, 1.0], // green
            Mood::Anxious | Mood::Fearful => [0.9, 0.6, 0.2, 1.0],   // yellow
            Mood::Frustrated => [0.9, 0.3, 0.3, 1.0],                // red
            Mood::Greedy => [0.9, 0.5, 0.1, 1.0],                    // orange
            Mood::Euphoric => [0.5, 0.3, 0.9, 1.0],                  // purple
        }
    }

    /// Get color for trade grade (RGBA)
    pub fn grade_color(grade: &str) -> [f32; 4] {
        match grade {
            "A" => [0.2, 0.8, 0.2, 1.0],  // green
            "B" => [0.2, 0.6, 0.9, 1.0],  // blue
            "C" => [0.9, 0.8, 0.2, 1.0],  // yellow
            "F" => [0.9, 0.2, 0.2, 1.0],  // red
            _ => [0.6, 0.6, 0.6, 1.0],    // gray
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalPanel {
    id: JournalId,
    title: String,
}

impl JournalPanel {
    pub fn new(id: JournalId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> JournalId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "journal"
    }

    pub fn kind_label(&self) -> &'static str {
        "Journal"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 250.0)
    }
}
