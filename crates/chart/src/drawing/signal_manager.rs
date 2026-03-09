//! Signal Manager - manages strategy-generated system signals
//!
//! Separate from DrawingManager which handles user primitives.
//! System signals cannot be dragged, edited, or interacted with
//! through normal primitive UI.

use std::collections::HashMap;
use crate::drawing::primitives_v2::{
    SystemSignal, SignalType, StrategySignalConfig,
};
use crate::render::RenderContext;

/// Manager for system signals (strategy-generated markers)
///
/// Unlike DrawingManager, this manager:
/// - Does not support drag/drop
/// - Does not support selection for editing
/// - Groups signals by strategy
/// - Has separate configuration per strategy
#[derive(Clone, Debug, Default)]
pub struct SignalManager {
    /// All signals, keyed by signal ID
    signals: HashMap<u64, SystemSignal>,
    /// Next signal ID
    next_id: u64,
    /// Strategy configurations
    strategies: HashMap<String, StrategySignalConfig>,
    /// Global visibility toggle
    visible: bool,
}

impl SignalManager {
    pub fn new() -> Self {
        Self {
            signals: HashMap::new(),
            next_id: 1,
            strategies: HashMap::new(),
            visible: false, // Signals hidden by default
        }
    }

    // =========================================================================
    // Signal Management
    // =========================================================================

    /// Add a signal from a strategy
    pub fn add_signal(
        &mut self,
        strategy_tag: &str,
        signal_type: SignalType,
        bar: f64,
        price: f64,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let mut signal = SystemSignal::new(id, strategy_tag, signal_type, bar, price);

        // Apply strategy config if exists
        if let Some(config) = self.strategies.get(strategy_tag) {
            let color = config.color_for(signal_type);
            signal.set_color(color);
            if let Some(size) = config.size {
                signal.set_size(size);
            }
            signal.visible = config.visible;
        }

        self.signals.insert(id, signal);
        id
    }

    /// Add a signal with label
    pub fn add_signal_with_label(
        &mut self,
        strategy_tag: &str,
        signal_type: SignalType,
        bar: f64,
        price: f64,
        label: &str,
    ) -> u64 {
        let id = self.add_signal(strategy_tag, signal_type, bar, price);
        if let Some(signal) = self.signals.get_mut(&id) {
            signal.label = Some(label.to_string());
        }
        id
    }

    /// Remove a signal by ID
    pub fn remove_signal(&mut self, id: u64) -> Option<SystemSignal> {
        self.signals.remove(&id)
    }

    /// Remove all signals from a strategy
    pub fn remove_strategy_signals(&mut self, strategy_tag: &str) {
        self.signals.retain(|_, s| s.strategy_tag != strategy_tag);
    }

    /// Clear all signals
    pub fn clear(&mut self) {
        self.signals.clear();
    }

    /// Get signal by ID
    pub fn get(&self, id: u64) -> Option<&SystemSignal> {
        self.signals.get(&id)
    }

    /// Get mutable signal by ID
    pub fn get_mut(&mut self, id: u64) -> Option<&mut SystemSignal> {
        self.signals.get_mut(&id)
    }

    /// Get all signals
    pub fn signals(&self) -> impl Iterator<Item = &SystemSignal> + '_ {
        self.signals.values()
    }

    /// Get signals for a specific strategy
    pub fn signals_for_strategy<'a>(&'a self, strategy_tag: &'a str) -> impl Iterator<Item = &'a SystemSignal> + 'a {
        self.signals.values().filter(move |s| s.strategy_tag == strategy_tag)
    }

    /// Get signal count
    pub fn count(&self) -> usize {
        self.signals.len()
    }

    /// Get signal count for strategy
    pub fn count_for_strategy(&self, strategy_tag: &str) -> usize {
        self.signals.values().filter(|s| s.strategy_tag == strategy_tag).count()
    }

    // =========================================================================
    // Strategy Configuration
    // =========================================================================

    /// Register a strategy with default configuration
    pub fn register_strategy(&mut self, tag: &str, display_name: &str) {
        if !self.strategies.contains_key(tag) {
            self.strategies.insert(tag.to_string(), StrategySignalConfig::new(tag, display_name));
        }
    }

    /// Get strategy config
    pub fn strategy_config(&self, tag: &str) -> Option<&StrategySignalConfig> {
        self.strategies.get(tag)
    }

    /// Get mutable strategy config
    pub fn strategy_config_mut(&mut self, tag: &str) -> Option<&mut StrategySignalConfig> {
        self.strategies.get_mut(tag)
    }

    /// Get all registered strategies
    pub fn strategies(&self) -> impl Iterator<Item = &StrategySignalConfig> + '_ {
        self.strategies.values()
    }

    /// Set strategy visibility (and update all its signals)
    pub fn set_strategy_visible(&mut self, tag: &str, visible: bool) {
        if let Some(config) = self.strategies.get_mut(tag) {
            config.visible = visible;
        }
        for signal in self.signals.values_mut() {
            if signal.strategy_tag == tag {
                signal.visible = visible;
            }
        }
    }

    /// Set color for signal type in strategy
    pub fn set_strategy_color(&mut self, tag: &str, signal_type: SignalType, color: &str) {
        if let Some(config) = self.strategies.get_mut(tag) {
            config.set_color_for(signal_type, color);
        }
        // Update existing signals
        for signal in self.signals.values_mut() {
            if signal.strategy_tag == tag && signal.signal_type == signal_type {
                signal.set_color(color);
            }
        }
    }

    /// Set size for all signals in strategy
    pub fn set_strategy_size(&mut self, tag: &str, size: f64) {
        if let Some(config) = self.strategies.get_mut(tag) {
            config.size = Some(size);
        }
        for signal in self.signals.values_mut() {
            if signal.strategy_tag == tag {
                signal.set_size(size);
            }
        }
    }

    // =========================================================================
    // Visibility
    // =========================================================================

    /// Get global visibility
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Set global visibility
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    // =========================================================================
    // Rendering
    // =========================================================================

    /// Render all visible signals
    pub fn render(&self, ctx: &mut dyn RenderContext) {
        if !self.visible {
            return;
        }

        for signal in self.signals.values() {
            // Check strategy visibility
            let strategy_visible = self.strategies
                .get(&signal.strategy_tag)
                .map(|c| c.visible)
                .unwrap_or(true);

            if strategy_visible && signal.visible {
                signal.render(ctx);
            }
        }
    }

    // =========================================================================
    // Serialization
    // =========================================================================

    /// Get all signals as JSON array
    pub fn to_json(&self) -> String {
        let signals: Vec<String> = self.signals.values()
            .map(|s| s.to_json())
            .collect();
        format!("[{}]", signals.join(","))
    }

    /// Load signals from JSON array
    pub fn load_json(&mut self, json: &str) {
        if let Ok(signals) = serde_json::from_str::<Vec<SystemSignal>>(json) {
            for signal in signals {
                let id = signal.id;
                if id >= self.next_id {
                    self.next_id = id + 1;
                }
                self.signals.insert(id, signal);
            }
        }
    }

    /// Get strategy configs as JSON
    pub fn strategies_to_json(&self) -> String {
        serde_json::to_string(&self.strategies).unwrap_or_default()
    }

    /// Load strategy configs from JSON
    pub fn load_strategies_json(&mut self, json: &str) {
        if let Ok(strategies) = serde_json::from_str::<HashMap<String, StrategySignalConfig>>(json) {
            self.strategies = strategies;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_signal() {
        let mut manager = SignalManager::new();
        let id = manager.add_signal("test_strategy", SignalType::Buy, 100.0, 50000.0);
        assert_eq!(id, 1);
        assert_eq!(manager.count(), 1);
    }

    #[test]
    fn test_remove_strategy_signals() {
        let mut manager = SignalManager::new();
        manager.add_signal("strategy_a", SignalType::Buy, 100.0, 50000.0);
        manager.add_signal("strategy_a", SignalType::Sell, 101.0, 50100.0);
        manager.add_signal("strategy_b", SignalType::Buy, 102.0, 50200.0);

        assert_eq!(manager.count(), 3);
        manager.remove_strategy_signals("strategy_a");
        assert_eq!(manager.count(), 1);
    }

    #[test]
    fn test_strategy_visibility() {
        let mut manager = SignalManager::new();
        manager.register_strategy("test", "Test Strategy");
        manager.add_signal("test", SignalType::Buy, 100.0, 50000.0);

        manager.set_strategy_visible("test", false);

        let signal = manager.signals().next().unwrap();
        assert!(!signal.visible);
    }
}
