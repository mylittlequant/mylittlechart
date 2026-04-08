use serde::{Serialize, Deserialize};
use std::collections::VecDeque;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RssFeedId(pub u64);

#[derive(Clone, Debug)]
pub struct RssFeedState {
    /// Feed URL
    pub feed_url: String,
    /// Feed items
    pub items: VecDeque<RssItem>,
    /// Feed metadata
    pub feed_info: Option<RssFeedInfo>,
}

#[derive(Clone, Debug)]
pub struct RssItem {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub link: String,
    pub pub_date: Option<i64>,
    pub author: Option<String>,
    pub categories: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct RssFeedInfo {
    pub title: String,
    pub description: String,
    pub link: String,
    pub last_updated: i64,
}

impl RssFeedState {
    pub fn new() -> Self {
        Self {
            feed_url: String::new(),
            items: VecDeque::new(),
            feed_info: None,
        }
    }

    /// Get visible RSS items for rendering
    pub fn visible_items(&self, max_count: usize) -> Vec<&RssItem> {
        self.items.iter().take(max_count).collect()
    }

    /// Format RSS item for display
    pub fn format_rss_item(&self, item: &RssItem) -> (String, String, String) {
        let time = item.pub_date.map(|ts| format_timestamp(ts)).unwrap_or_else(|| "—".to_string());
        let title = item.title.clone();
        let author = item.author.as_ref().map(|s| s.as_str()).unwrap_or("Unknown");
        (time, title, author.to_string())
    }

    /// Get truncated description for preview
    pub fn truncate_description(&self, item: &RssItem, max_len: usize) -> String {
        if let Some(ref desc) = item.description {
            if desc.len() > max_len {
                format!("{}...", &desc[..max_len])
            } else {
                desc.clone()
            }
        } else {
            "No description".to_string()
        }
    }

    /// Format RSS item for display (alias for format_rss_item)
    pub fn format_item(&self, item: &RssItem) -> (String, String, String) {
        self.format_rss_item(item)
    }
}

fn format_timestamp(ts: i64) -> String {
    let secs = (ts / 1000) % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    format!("{:02}:{:02}", h, m)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RssFeedConfig {
    /// Multiple feed URLs
    pub feed_urls: Vec<String>,
    /// Refresh interval
    pub refresh_interval: u64,
    /// Max items per feed
    pub max_items_per_feed: usize,
    /// Show descriptions
    pub show_descriptions: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RssFeedPanel {
    id: RssFeedId,
    title: String,
}

impl RssFeedPanel {
    pub fn new(id: RssFeedId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> RssFeedId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "rss_feed"
    }

    pub fn kind_label(&self) -> &'static str {
        "RSS Feed"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (250.0, 200.0)
    }
}
