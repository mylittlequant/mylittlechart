use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MarketReplayId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct MarketReplayState {
    pub recording: RecordedData,
    pub current_time: i64,  // Unix timestamp
    pub playback_state: PlaybackState,
    pub speed: f32,  // 1.0 = real-time, 2.0 = 2x, etc.
    pub selected_instruments: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct RecordedData {
    pub start_time: i64,  // Unix timestamp
    pub end_time: i64,    // Unix timestamp
    pub snapshots: Vec<MarketSnapshot>,  // Time-ordered snapshots
}

#[derive(Clone, Debug)]
pub struct MarketSnapshot {
    pub timestamp: i64,  // Unix timestamp
    pub prices: HashMap<String, f64>,
    pub volumes: HashMap<String, f64>,
    pub order_books: HashMap<String, OrderBook>,
}

#[derive(Clone, Debug, Default)]
pub struct OrderBook {
    pub bids: Vec<(f64, f64)>,  // (price, size)
    pub asks: Vec<(f64, f64)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketReplayConfig {
    pub timeline_height: f32,  // Height of timeline control bar
    pub chart_types: Vec<ChartType>,  // What to display: Price, Volume, OrderBook
    pub auto_scroll: bool,  // Auto-scroll charts as playback progresses
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ChartType {
    Price,
    Volume,
    OrderBook,
    Trades,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self::Stopped
    }
}

impl MarketReplayState {
    pub fn new() -> Self {
        Self {
            speed: 1.0,
            ..Default::default()
        }
    }

    /// Returns the current playback progress as a percentage (0.0-1.0)
    pub fn progress_pct(&self) -> f64 {
        let start = self.recording.start_time;
        let end = self.recording.end_time;

        if end <= start {
            return 0.0;
        }

        let elapsed = self.current_time - start;
        let total = end - start;

        (elapsed as f64 / total as f64).clamp(0.0, 1.0)
    }

    /// Returns formatted time string for current playback position
    pub fn format_time(&self) -> String {
        let timestamp = self.current_time;

        let hours = (timestamp / 3600) % 24;
        let minutes = (timestamp / 60) % 60;
        let seconds = timestamp % 60;

        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    }

    /// Returns the current market snapshot bar/candle
    pub fn current_bar(&self) -> Option<&MarketSnapshot> {
        // Find the snapshot closest to current_time
        self.recording.snapshots.iter()
            .rfind(|snapshot| snapshot.timestamp <= self.current_time)
    }

    /// Returns playback progress (alias for progress_pct)
    pub fn playback_progress(&self) -> f64 {
        self.progress_pct()
    }
}

impl Default for MarketReplayConfig {
    fn default() -> Self {
        Self {
            timeline_height: 60.0,
            chart_types: vec![ChartType::Price],
            auto_scroll: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketReplayPanel {
    id: MarketReplayId,
    title: String,
}

impl MarketReplayPanel {
    pub fn new(id: MarketReplayId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> MarketReplayId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "market_replay"
    }

    pub fn kind_label(&self) -> &'static str {
        "Market Replay"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
