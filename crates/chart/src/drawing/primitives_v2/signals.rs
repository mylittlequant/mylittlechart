//! System Signals - strategy-generated markers
//!
//! SystemSignal wraps existing primitives (TriangleUp, TriangleDown, Sign) but:
//! - Blocks all interactive operations (drag, resize, edit)
//! - Stored separately from user primitives
//! - Has its own configuration panel
//! - Tagged with strategy identifier

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::{
    Primitive, HitTestResult, RenderContext,
    Sign,
};
use super::annotations::sign::SignType;
use super::annotations::triangle_up::TriangleUp;
use super::annotations::triangle_down::TriangleDown;

/// Signal type for strategy markers
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalType {
    /// Buy signal (bullish)
    Buy,
    /// Sell signal (bearish)
    Sell,
    /// Take profit level
    TakeProfit,
    /// Stop loss level
    StopLoss,
    /// Entry point
    Entry,
    /// Exit point
    Exit,
    /// Custom marker
    Custom,
}

impl Default for SignalType {
    fn default() -> Self {
        Self::Custom
    }
}

impl SignalType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Buy => "buy",
            Self::Sell => "sell",
            Self::TakeProfit => "take_profit",
            Self::StopLoss => "stop_loss",
            Self::Entry => "entry",
            Self::Exit => "exit",
            Self::Custom => "custom",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "buy" => Self::Buy,
            "sell" => Self::Sell,
            "take_profit" => Self::TakeProfit,
            "stop_loss" => Self::StopLoss,
            "entry" => Self::Entry,
            "exit" => Self::Exit,
            _ => Self::Custom,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Buy => "Buy",
            Self::Sell => "Sell",
            Self::TakeProfit => "Take Profit",
            Self::StopLoss => "Stop Loss",
            Self::Entry => "Entry",
            Self::Exit => "Exit",
            Self::Custom => "Custom",
        }
    }

    /// Default color for this signal type
    pub fn default_color(&self) -> &'static str {
        match self {
            Self::Buy | Self::Entry => "#4CAF50",      // Green
            Self::Sell | Self::Exit => "#F44336",      // Red
            Self::TakeProfit => "#2196F3",             // Blue
            Self::StopLoss => "#FF9800",               // Orange
            Self::Custom => "#9C27B0",                 // Purple
        }
    }
}

/// Inner primitive type for signal rendering
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SignalPrimitive {
    ArrowUp(TriangleUp),
    ArrowDown(TriangleDown),
    Sign(Sign),
}

impl SignalPrimitive {
    /// Create appropriate primitive for signal type
    pub fn for_signal_type(signal_type: SignalType, bar: f64, price: f64) -> Self {
        let color = signal_type.default_color();
        match signal_type {
            SignalType::Buy | SignalType::Entry => {
                Self::ArrowUp(TriangleUp::new(bar, price, bar + 2.0, price, color))
            }
            SignalType::Sell | SignalType::Exit => {
                Self::ArrowDown(TriangleDown::new(bar, price, bar + 2.0, price, color))
            }
    SignalType::TakeProfit => {
                let mut sign = Sign::new(bar, price, bar + 2.0, price, color);
                sign.sign_type = SignType::Check;
                Self::Sign(sign)
            }
            SignalType::StopLoss => {
                let mut sign = Sign::new(bar, price, bar + 2.0, price, color);
                sign.sign_type = SignType::X;
                Self::Sign(sign)
            }
            SignalType::Custom => {
                let mut sign = Sign::new(bar, price, bar + 2.0, price, color);
                sign.sign_type = SignType::Circle;
                Self::Sign(sign)
            }
        }
    }

    fn inner(&self) -> &dyn Primitive {
        match self {
            Self::ArrowUp(p) => p,
            Self::ArrowDown(p) => p,
            Self::Sign(p) => p,
        }
    }

    fn inner_mut(&mut self) -> &mut dyn Primitive {
        match self {
            Self::ArrowUp(p) => p,
            Self::ArrowDown(p) => p,
            Self::Sign(p) => p,
        }
    }
}

/// System Signal - a strategy-generated marker
///
/// Unlike user primitives, system signals:
/// - Cannot be dragged or resized
/// - Cannot have their shape edited
/// - Are stored in a separate collection
/// - Have their own configuration UI
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SystemSignal {
    /// Unique signal ID
    pub id: u64,
    /// Strategy tag (e.g., "momentum_v1", "scalper")
    pub strategy_tag: String,
    /// Signal type
    pub signal_type: SignalType,
    /// Optional label (e.g., "TP1", "Entry #3")
    pub label: Option<String>,
    /// Timestamp when signal was generated
    pub timestamp: i64,
    /// Inner primitive for rendering
    primitive: SignalPrimitive,
    /// Visibility flag
    pub visible: bool,
}

impl SystemSignal {
    /// Create a new system signal
    pub fn new(
        id: u64,
        strategy_tag: &str,
        signal_type: SignalType,
        bar: f64,
        price: f64,
    ) -> Self {
        Self {
            id,
            strategy_tag: strategy_tag.to_string(),
            signal_type,
            label: None,
            timestamp: 0,
            primitive: SignalPrimitive::for_signal_type(signal_type, bar, price),
            visible: true,
        }
    }

    /// Create with custom label
    pub fn with_label(mut self, label: &str) -> Self {
        self.label = Some(label.to_string());
        self
    }

    /// Create with timestamp
    pub fn with_timestamp(mut self, ts: i64) -> Self {
        self.timestamp = ts;
        self
    }

    /// Set color (overrides default)
    pub fn set_color(&mut self, color: &str) {
        self.primitive.inner_mut().data_mut().color.stroke = color.to_string();
    }

    /// Get color
    pub fn color(&self) -> &str {
        &self.primitive.inner().data().color.stroke
    }

    /// Set size (adjusts second point for TwoPoint primitives)
    pub fn set_size(&mut self, size: f64) {
        match &mut self.primitive {
            SignalPrimitive::ArrowUp(p) => {
                p.bar2 = p.bar1 + size / 10.0;
            }
            SignalPrimitive::ArrowDown(p) => {
                p.bar2 = p.bar1 + size / 10.0;
            }
            SignalPrimitive::Sign(p) => {
                p.bar2 = p.bar1 + size / 10.0;
            }
        }
    }

    /// Get size
    pub fn size(&self) -> f64 {
        match &self.primitive {
            SignalPrimitive::ArrowUp(p) => (p.bar2 - p.bar1).abs() * 10.0,
            SignalPrimitive::ArrowDown(p) => (p.bar2 - p.bar1).abs() * 10.0,
            SignalPrimitive::Sign(p) => (p.bar2 - p.bar1).abs() * 10.0,
        }
    }

    /// Get bar position
    pub fn bar(&self) -> f64 {
        match &self.primitive {
            SignalPrimitive::ArrowUp(p) => p.bar1,
            SignalPrimitive::ArrowDown(p) => p.bar1,
            SignalPrimitive::Sign(p) => p.bar1,
        }
    }

    /// Get price position
    pub fn price(&self) -> f64 {
        match &self.primitive {
            SignalPrimitive::ArrowUp(p) => p.price1,
            SignalPrimitive::ArrowDown(p) => p.price1,
            SignalPrimitive::Sign(p) => p.price1,
        }
    }

    /// Render the signal
    pub fn render(&self, ctx: &mut dyn RenderContext) {
        if !self.visible {
            return;
        }
        // Never render as selected - system signals can't be selected
        self.primitive.inner().render(ctx, false);
    }

    /// Hit test - always returns Miss (signals can't be interacted with)
    pub fn hit_test(&self, _sx: f64, _sy: f64, _vp: &Viewport, _ps: &PriceScale) -> HitTestResult {
        // System signals don't respond to clicks
        HitTestResult::Miss
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }
}

/// Configuration for a strategy's signals
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StrategySignalConfig {
    /// Strategy identifier
    pub strategy_tag: String,
    /// Display name
    pub display_name: String,
    /// Is this strategy's signals visible
    pub visible: bool,
    /// Color overrides by signal type
    pub colors: std::collections::HashMap<String, String>,
    /// Size override
    pub size: Option<f64>,
}

impl StrategySignalConfig {
    pub fn new(tag: &str, name: &str) -> Self {
        Self {
            strategy_tag: tag.to_string(),
            display_name: name.to_string(),
            visible: true,
            colors: std::collections::HashMap::new(),
            size: None,
        }
    }

    /// Get color for signal type (custom or default)
    pub fn color_for(&self, signal_type: SignalType) -> &str {
        self.colors
            .get(signal_type.as_str())
            .map(|s| s.as_str())
            .unwrap_or_else(|| signal_type.default_color())
    }

    /// Set color for signal type
    pub fn set_color_for(&mut self, signal_type: SignalType, color: &str) {
        self.colors.insert(signal_type.as_str().to_string(), color.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_creation() {
        let signal = SystemSignal::new(1, "test_strategy", SignalType::Buy, 100.0, 50000.0);
        assert_eq!(signal.signal_type, SignalType::Buy);
        assert_eq!(signal.strategy_tag, "test_strategy");
        assert_eq!(signal.bar(), 100.0);
        assert_eq!(signal.price(), 50000.0);
    }

    #[test]
    fn test_signal_type_colors() {
        assert_eq!(SignalType::Buy.default_color(), "#4CAF50");
        assert_eq!(SignalType::Sell.default_color(), "#F44336");
    }

    #[test]
    fn test_signal_serialization() {
        let signal = SystemSignal::new(1, "test", SignalType::TakeProfit, 100.0, 50000.0)
            .with_label("TP1");
        let json = signal.to_json();
        let restored = SystemSignal::from_json(&json).unwrap();
        assert_eq!(restored.label, Some("TP1".to_string()));
    }
}
