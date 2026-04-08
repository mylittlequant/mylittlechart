use serde::{Serialize, Deserialize};
use std::collections::VecDeque;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OptionFlowId(pub u64);

#[derive(Clone, Debug)]
pub struct OptionFlowState {
    /// Recent large option trades
    pub flows: VecDeque<OptionFlow>,
    /// Size threshold (premium dollars)
    pub threshold: f64,
    /// Underlying filter
    pub underlying_filter: Option<String>,
    /// Sentiment (Bullish/Bearish/Neutral)
    pub sentiment: OptionSentiment,
}

#[derive(Clone, Debug)]
pub struct OptionFlow {
    pub id: String,
    pub timestamp: i64,
    pub underlying: String,
    pub strike: f64,
    pub expiration: String,
    pub option_type: OptionType,
    pub side: TradeSide,
    pub size: u64,
    pub premium: f64,
    pub implied_volatility: f64,
    pub spot_price: f64,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug)]
pub enum OptionType {
    Call,
    Put,
}

#[derive(Clone, Debug)]
pub enum TradeSide {
    Buy,
    Sell,
}

#[derive(Clone, Debug)]
pub enum FlowType {
    Sweep,
    Block,
    Split,
    Unusual,
}

#[derive(Clone, Debug)]
pub enum OptionSentiment {
    Bullish,
    Bearish,
    Neutral,
}

impl OptionFlowState {
    pub fn new() -> Self {
        Self {
            flows: VecDeque::new(),
            threshold: 0.0,
            underlying_filter: None,
            sentiment: OptionSentiment::Neutral,
        }
    }

    /// Get visible flows for rendering (most recent first)
    pub fn visible_flows(&self, max_count: usize) -> Vec<&OptionFlow> {
        self.flows.iter().rev().take(max_count).collect()
    }

    /// Format flow for display
    pub fn format_flow(&self, flow: &OptionFlow) -> (String, String, String, String, String, String) {
        let time = format_timestamp(flow.timestamp);
        let underlying = flow.underlying.clone();
        let contract = format!(
            "{} ${:.0} {}",
            match flow.option_type {
                OptionType::Call => "C",
                OptionType::Put => "P",
            },
            flow.strike,
            flow.expiration
        );
        let side = match flow.side {
            TradeSide::Buy => "BUY",
            TradeSide::Sell => "SELL",
        };
        let premium = format!("${:.0}K", flow.premium / 1000.0);
        let flow_type = match flow.flow_type {
            FlowType::Sweep => "SWEEP",
            FlowType::Block => "BLOCK",
            FlowType::Split => "SPLIT",
            FlowType::Unusual => "UNUSUAL",
        };
        (time, underlying, contract, side.to_string(), premium, flow_type.to_string())
    }

    /// Get color based on flow sentiment
    pub fn flow_color(&self, flow: &OptionFlow) -> [f32; 4] {
        let is_bullish = match (&flow.option_type, &flow.side) {
            (OptionType::Call, TradeSide::Buy) => true,
            (OptionType::Put, TradeSide::Sell) => true,
            _ => false,
        };

        if is_bullish {
            [0.2, 0.8, 0.3, 1.0] // green
        } else {
            [0.9, 0.2, 0.2, 1.0] // red
        }
    }

    /// Get badge color for flow type
    pub fn flow_type_color(&self, flow_type: &FlowType) -> [f32; 4] {
        match flow_type {
            FlowType::Sweep => [0.9, 0.2, 0.2, 1.0],   // red
            FlowType::Block => [0.3, 0.6, 0.9, 1.0],   // blue
            FlowType::Split => [0.9, 0.7, 0.2, 1.0],   // yellow
            FlowType::Unusual => [0.8, 0.2, 0.8, 1.0], // purple
        }
    }
}

fn format_timestamp(ts: i64) -> String {
    let secs = (ts / 1000) % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OptionFlowConfig {
    /// Minimum premium threshold
    pub min_premium: f64,
    /// Show sweep/block badges
    pub show_flow_badges: bool,
    /// Alert on large flow
    pub enable_alerts: bool,
    /// Max flows to display
    pub max_flows: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OptionFlowPanel {
    id: OptionFlowId,
    title: String,
}

impl OptionFlowPanel {
    pub fn new(id: OptionFlowId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> OptionFlowId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "option_flow"
    }

    pub fn kind_label(&self) -> &'static str {
        "Option Flow"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 200.0)
    }
}
