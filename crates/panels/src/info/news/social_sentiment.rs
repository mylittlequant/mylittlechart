use serde::{Serialize, Deserialize};
use std::collections::{HashMap, VecDeque};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SocialSentimentId(pub u64);

#[derive(Clone, Debug)]
pub struct SocialSentimentState {
    /// Symbol being tracked
    pub symbol: String,
    /// Aggregated sentiment
    pub sentiment: SentimentData,
    /// Recent mentions
    pub mentions: VecDeque<SocialMention>,
}

#[derive(Clone, Debug)]
pub struct SentimentData {
    pub symbol: String,
    pub score: f64,
    pub mentions_count: u64,
    pub trending_rank: Option<u32>,
    pub score_change_24h: f64,
    pub sources: HashMap<SocialSource, SourceSentiment>,
}

#[derive(Clone, Debug)]
pub struct SourceSentiment {
    pub score: f64,
    pub mentions: u64,
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub enum SocialSource {
    Twitter,
    Reddit,
    Stocktwits,
    Discord,
    Telegram,
}

#[derive(Clone, Debug)]
pub struct SocialMention {
    pub id: String,
    pub timestamp: i64,
    pub source: SocialSource,
    pub text: String,
    pub author: String,
    pub score: f64,
    pub url: Option<String>,
}

impl SocialSentimentState {
    pub fn new() -> Self {
        Self {
            symbol: String::new(),
            sentiment: SentimentData {
                symbol: String::new(),
                score: 0.0,
                mentions_count: 0,
                trending_rank: None,
                score_change_24h: 0.0,
                sources: HashMap::new(),
            },
            mentions: VecDeque::new(),
        }
    }

    /// Format sentiment metrics for display
    pub fn format_sentiment(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Score", format!("{:.2}", self.sentiment.score)),
            ("Mentions", format!("{}", self.sentiment.mentions_count)),
            ("Trending", self.sentiment.trending_rank.map(|r| format!("#{}", r)).unwrap_or_else(|| "—".to_string())),
            ("24h Change", format!("{:+.2}", self.sentiment.score_change_24h)),
        ]
    }

    /// Get color based on sentiment score (-1.0 to 1.0)
    pub fn sentiment_color(&self) -> [f32; 4] {
        let score = self.sentiment.score;
        if score > 0.3 {
            [0.2, 0.8, 0.3, 1.0] // green - positive
        } else if score < -0.3 {
            [0.9, 0.2, 0.2, 1.0] // red - negative
        } else {
            [0.9, 0.7, 0.2, 1.0] // yellow - neutral
        }
    }

    /// Get normalized sentiment for gauge (0.0 to 1.0)
    pub fn sentiment_normalized(&self) -> f32 {
        ((self.sentiment.score + 1.0) / 2.0).clamp(0.0, 1.0) as f32
    }

    /// Get visible mentions for rendering
    pub fn visible_mentions(&self, max_count: usize) -> Vec<&SocialMention> {
        self.mentions.iter().take(max_count).collect()
    }

    /// Format mention for display
    pub fn format_mention(&self, mention: &SocialMention) -> (String, String, String) {
        let time = format_timestamp(mention.timestamp);
        let source = format!("{:?}", mention.source);
        let text = if mention.text.len() > 100 {
            format!("{}...", &mention.text[..100])
        } else {
            mention.text.clone()
        };
        (time, source, text)
    }
}

fn format_timestamp(ts: i64) -> String {
    let secs = (ts / 1000) % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    format!("{:02}:{:02}", h, m)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SocialSentimentConfig {
    /// Sources to track
    pub enabled_sources: Vec<String>,
    /// Show individual mentions
    pub show_mentions: bool,
    /// Sentiment calculation method
    pub sentiment_method: SentimentMethod,
    /// Refresh interval
    pub refresh_interval: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SentimentMethod {
    Average,
    Weighted,
    VolumeWeighted,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SocialSentimentPanel {
    id: SocialSentimentId,
    title: String,
}

impl SocialSentimentPanel {
    pub fn new(id: SocialSentimentId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> SocialSentimentId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "social_sentiment"
    }

    pub fn kind_label(&self) -> &'static str {
        "Social Sentiment"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (250.0, 200.0)
    }
}
