use serde::{Serialize, Deserialize};
use std::collections::VecDeque;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NewsId(pub u64);

#[derive(Clone, Debug)]
pub struct NewsState {
    /// News items
    pub news: VecDeque<NewsItem>,
    /// Symbol filter (related symbols)
    pub symbol_filter: Option<String>,
    /// Category filter
    pub category_filter: Option<NewsCategory>,
    /// Sentiment filter
    pub sentiment_filter: Option<NewsSentiment>,
}

#[derive(Clone, Debug)]
pub struct NewsItem {
    pub id: String,
    pub timestamp: i64,
    pub headline: String,
    pub source: String,
    pub url: String,
    pub category: NewsCategory,
    pub sentiment: Option<NewsSentiment>,
    pub sentiment_score: Option<f64>,
    pub related_symbols: Vec<String>,
    pub image_url: Option<String>,
}

#[derive(Clone, Debug)]
pub enum NewsCategory {
    Breaking,
    Markets,
    Crypto,
    Earnings,
    Economy,
    Politics,
    Technology,
}

#[derive(Clone, Debug)]
pub enum NewsSentiment {
    Positive,
    Neutral,
    Negative,
}

impl NewsState {
    pub fn new() -> Self {
        Self {
            news: VecDeque::new(),
            symbol_filter: None,
            category_filter: None,
            sentiment_filter: None,
        }
    }

    /// Get visible news items for rendering (most recent first)
    pub fn visible_news(&self, max_count: usize) -> Vec<&NewsItem> {
        self.news.iter().take(max_count).collect()
    }

    /// Format news item for display
    pub fn format_news_item(&self, item: &NewsItem) -> (String, String, String, String) {
        let time = format_timestamp(item.timestamp);
        let headline = item.headline.clone();
        let source = item.source.clone();
        let category = format!("{:?}", item.category);
        (time, headline, source, category)
    }

    /// Get color based on sentiment
    pub fn sentiment_color(&self, item: &NewsItem) -> [f32; 4] {
        if let Some(ref sentiment) = item.sentiment {
            match sentiment {
                NewsSentiment::Positive => [0.2, 0.8, 0.3, 1.0], // green
                NewsSentiment::Neutral => [0.6, 0.6, 0.7, 1.0],  // neutral
                NewsSentiment::Negative => [0.9, 0.2, 0.2, 1.0], // red
            }
        } else {
            [0.6, 0.6, 0.7, 1.0]
        }
    }

    /// Get badge color for news category
    pub fn category_color(&self, category: &NewsCategory) -> [f32; 4] {
        match category {
            NewsCategory::Breaking => [0.9, 0.2, 0.2, 1.0],    // red
            NewsCategory::Markets => [0.3, 0.6, 0.9, 1.0],     // blue
            NewsCategory::Crypto => [0.9, 0.5, 0.2, 1.0],      // orange
            NewsCategory::Earnings => [0.2, 0.8, 0.3, 1.0],    // green
            NewsCategory::Economy => [0.8, 0.2, 0.8, 1.0],     // purple
            NewsCategory::Politics => [0.5, 0.5, 0.5, 1.0],    // gray
            NewsCategory::Technology => [0.2, 0.8, 0.8, 1.0],  // cyan
        }
    }

    /// Format news item for display (alias for format_news_item)
    pub fn format_item(&self, item: &NewsItem) -> (String, String, String, String) {
        self.format_news_item(item)
    }
}

fn format_timestamp(ts: i64) -> String {
    let secs = (ts / 1000) % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    format!("{:02}:{:02}", h, m)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewsFeedConfig {
    /// Max news items
    pub max_items: usize,
    /// Show images/thumbnails
    pub show_images: bool,
    /// Show sentiment badges
    pub show_sentiment: bool,
    /// Auto-refresh interval
    pub refresh_interval: u64,
    /// Preferred sources
    pub preferred_sources: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewsPanel {
    id: NewsId,
    title: String,
}

impl NewsPanel {
    pub fn new(id: NewsId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> NewsId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "news"
    }

    pub fn kind_label(&self) -> &'static str {
        "News"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (200.0, 150.0)
    }
}
