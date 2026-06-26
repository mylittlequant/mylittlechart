//! Signal Manager — global service for strategy-generated system signals.
//!
//! After K2.3 `SignalManager` is a **stateless dispatcher** that lives in
//! `Managers.signal`.  All signal data (the `signals` map, the ID counter,
//! strategy configs) now live on the `DataBubble` data slice
//! (`bubble.signals`, `bubble.signal_next_id`, `bubble.signal_strategies`).
//!
//! Every mutating method accepts `&mut DataBubble` (or the specific sub-slice)
//! and writes directly to the bubble.  No method reaches back to a
//! `signal_manager` field — that field is gone.
//!
//! Separate from DrawingManager which handles user primitives.
//! System signals cannot be dragged, edited, or interacted with
//! through normal primitive UI.
//!
//! Phase 2A of the Tag/Bubble/UiContainer cutover removed the
//! `signals_visible` flag + the orphaned `render()` body — neither was
//! reachable from any UI toggle or render pipeline.

use std::collections::HashMap;
use crate::drawing::primitives_v2::{SystemSignal, SignalType, StrategySignalConfig};
use crate::tag_manager::DataBubble;

/// Global service for strategy-generated signal annotations.
///
/// Stateless: all signal data lives on the [`DataBubble`] data slice.
/// Lives in [`crate::managers::Managers::signal`].
#[derive(Clone, Debug, Default)]
pub struct SignalManager;

impl SignalManager {
    pub fn new() -> Self {
        Self
    }

    // =========================================================================
    // Signal Management
    // =========================================================================

    /// Add a signal from a strategy. Returns the assigned signal ID.
    pub fn add_signal(
        &self,
        bubble: &mut DataBubble,
        strategy_tag: &str,
        signal_type: SignalType,
        ts_ms: i64,
        price: f64,
    ) -> u64 {
        let id = bubble.signal_next_id;
        bubble.signal_next_id += 1;

        let mut signal = SystemSignal::new(id, strategy_tag, signal_type, ts_ms, price);

        if let Some(config) = bubble.signal_strategies.get(strategy_tag) {
            let color = config.color_for(signal_type);
            signal.set_color(color);
            if let Some(size_ms) = config.size {
                signal.set_size(size_ms);
            }
            signal.visible = config.visible;
        }

        bubble.signals.insert(id, signal);
        id
    }

    /// Add a signal with a label. Returns the assigned signal ID.
    pub fn add_signal_with_label(
        &self,
        bubble: &mut DataBubble,
        strategy_tag: &str,
        signal_type: SignalType,
        ts_ms: i64,
        price: f64,
        label: &str,
    ) -> u64 {
        let id = self.add_signal(bubble, strategy_tag, signal_type, ts_ms, price);
        if let Some(signal) = bubble.signals.get_mut(&id) {
            signal.label = Some(label.to_string());
        }
        id
    }

    /// Remove a signal by ID.
    pub fn remove_signal(&self, bubble: &mut DataBubble, id: u64) -> Option<SystemSignal> {
        bubble.signals.remove(&id)
    }

    /// Remove all signals belonging to a strategy.
    pub fn remove_strategy_signals(&self, bubble: &mut DataBubble, strategy_tag: &str) {
        bubble.signals.retain(|_, s| s.strategy_tag != strategy_tag);
    }

    /// Clear all signals from a bubble.
    pub fn clear(&self, bubble: &mut DataBubble) {
        bubble.signals.clear();
    }

    /// Get signal by ID (immutable).
    pub fn get<'b>(&self, bubble: &'b DataBubble, id: u64) -> Option<&'b SystemSignal> {
        bubble.signals.get(&id)
    }

    /// Get signal by ID (mutable).
    pub fn get_mut<'b>(&self, bubble: &'b mut DataBubble, id: u64) -> Option<&'b mut SystemSignal> {
        bubble.signals.get_mut(&id)
    }

    /// Iterate over all signals in a bubble.
    pub fn signals<'b>(&self, bubble: &'b DataBubble) -> impl Iterator<Item = &'b SystemSignal> {
        bubble.signals.values()
    }

    /// Iterate over signals belonging to a specific strategy.
    pub fn signals_for_strategy<'b>(
        &self,
        bubble: &'b DataBubble,
        strategy_tag: &'b str,
    ) -> impl Iterator<Item = &'b SystemSignal> + 'b {
        bubble.signals.values().filter(move |s| s.strategy_tag == strategy_tag)
    }

    /// Total signal count in a bubble.
    pub fn count(&self, bubble: &DataBubble) -> usize {
        bubble.signals.len()
    }

    /// Signal count for a specific strategy in a bubble.
    pub fn count_for_strategy(&self, bubble: &DataBubble, strategy_tag: &str) -> usize {
        bubble.signals.values().filter(|s| s.strategy_tag == strategy_tag).count()
    }

    // =========================================================================
    // Strategy Configuration
    // =========================================================================

    /// Register a strategy with default configuration if not already present.
    pub fn register_strategy(&self, bubble: &mut DataBubble, tag: &str, display_name: &str) {
        if !bubble.signal_strategies.contains_key(tag) {
            bubble.signal_strategies.insert(
                tag.to_string(),
                StrategySignalConfig::new(tag, display_name),
            );
        }
    }

    /// Get immutable strategy config from a bubble.
    pub fn strategy_config<'b>(
        &self,
        bubble: &'b DataBubble,
        tag: &str,
    ) -> Option<&'b StrategySignalConfig> {
        bubble.signal_strategies.get(tag)
    }

    /// Get mutable strategy config from a bubble.
    pub fn strategy_config_mut<'b>(
        &self,
        bubble: &'b mut DataBubble,
        tag: &str,
    ) -> Option<&'b mut StrategySignalConfig> {
        bubble.signal_strategies.get_mut(tag)
    }

    /// Iterate over all registered strategy configs in a bubble.
    pub fn strategies<'b>(
        &self,
        bubble: &'b DataBubble,
    ) -> impl Iterator<Item = &'b StrategySignalConfig> {
        bubble.signal_strategies.values()
    }

    /// Set strategy visibility and cascade to all its signals.
    pub fn set_strategy_visible(&self, bubble: &mut DataBubble, tag: &str, visible: bool) {
        if let Some(config) = bubble.signal_strategies.get_mut(tag) {
            config.visible = visible;
        }
        for signal in bubble.signals.values_mut() {
            if signal.strategy_tag == tag {
                signal.visible = visible;
            }
        }
    }

    /// Set color for a signal type in a strategy and cascade to existing signals.
    pub fn set_strategy_color(
        &self,
        bubble: &mut DataBubble,
        tag: &str,
        signal_type: SignalType,
        color: &str,
    ) {
        if let Some(config) = bubble.signal_strategies.get_mut(tag) {
            config.set_color_for(signal_type, color);
        }
        for signal in bubble.signals.values_mut() {
            if signal.strategy_tag == tag && signal.signal_type == signal_type {
                signal.set_color(color);
            }
        }
    }

    /// Set size for all signals in a strategy and cascade to existing signals.
    pub fn set_strategy_size(&self, bubble: &mut DataBubble, tag: &str, size_ms: i64) {
        if let Some(config) = bubble.signal_strategies.get_mut(tag) {
            config.size = Some(size_ms);
        }
        for signal in bubble.signals.values_mut() {
            if signal.strategy_tag == tag {
                signal.set_size(size_ms);
            }
        }
    }

    // =========================================================================
    // Serialization helpers
    // =========================================================================

    /// Serialize all signals in a bubble to a JSON array string.
    pub fn to_json(&self, bubble: &DataBubble) -> String {
        let signals: Vec<String> = bubble.signals.values()
            .map(|s| s.to_json())
            .collect();
        format!("[{}]", signals.join(","))
    }

    /// Load signals from a JSON array string into a bubble.
    ///
    /// Also advances `bubble.signal_next_id` past any restored IDs.
    pub fn load_json(&self, bubble: &mut DataBubble, json: &str) {
        if let Ok(signals) = serde_json::from_str::<Vec<SystemSignal>>(json) {
            for signal in signals {
                let id = signal.id;
                if id >= bubble.signal_next_id {
                    bubble.signal_next_id = id + 1;
                }
                bubble.signals.insert(id, signal);
            }
        }
    }

    /// Serialize strategy configs for a bubble to a JSON string.
    pub fn strategies_to_json(&self, bubble: &DataBubble) -> String {
        serde_json::to_string(&bubble.signal_strategies).unwrap_or_default()
    }

    /// Load strategy configs from a JSON string into a bubble.
    pub fn load_strategies_json(&self, bubble: &mut DataBubble, json: &str) {
        if let Ok(strategies) =
            serde_json::from_str::<HashMap<String, StrategySignalConfig>>(json)
        {
            bubble.signal_strategies = strategies;
        }
    }

    // =========================================================================
    // ID counter repair (snapshot restore)
    // =========================================================================

    /// After restoring `bubble.signals` from a snapshot, advance `signal_next_id`
    /// past all restored IDs so that new signals never collide.
    pub fn repair_next_id(&self, bubble: &mut DataBubble) {
        if let Some(&max_id) = bubble.signals.keys().max() {
            if max_id >= bubble.signal_next_id {
                bubble.signal_next_id = max_id + 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tag_manager::BubbleStore;
    use crate::state::Timeframe;

    fn with_bubble<F: FnOnce(&mut DataBubble)>(f: F) {
        let mut store = BubbleStore::new();
        let id = store.create_group(
            [1.0, 0.0, 0.0, 1.0],
            "BTCUSDT".to_string(),
            Timeframe::m1(),
        );
        let bubble = store.bubble_mut(id).expect("bubble just created");
        f(bubble);
    }

    #[test]
    fn test_add_signal() {
        let mgr = SignalManager::new();
        with_bubble(|bubble| {
            let id = mgr.add_signal(bubble, "test_strategy", SignalType::Buy, 1_700_000_000_000, 50000.0);
            assert_eq!(id, 1);
            assert_eq!(mgr.count(bubble), 1);
        });
    }

    #[test]
    fn test_remove_strategy_signals() {
        let mgr = SignalManager::new();
        with_bubble(|bubble| {
            mgr.add_signal(bubble, "strategy_a", SignalType::Buy, 1_700_000_000_000, 50000.0);
            mgr.add_signal(bubble, "strategy_a", SignalType::Sell, 1_700_000_060_000, 50100.0);
            mgr.add_signal(bubble, "strategy_b", SignalType::Buy, 1_700_000_120_000, 50200.0);

            assert_eq!(mgr.count(bubble), 3);
            mgr.remove_strategy_signals(bubble, "strategy_a");
            assert_eq!(mgr.count(bubble), 1);
        });
    }

    #[test]
    fn test_strategy_visibility() {
        let mgr = SignalManager::new();
        with_bubble(|bubble| {
            mgr.register_strategy(bubble, "test", "Test Strategy");
            mgr.add_signal(bubble, "test", SignalType::Buy, 1_700_000_000_000, 50000.0);

            mgr.set_strategy_visible(bubble, "test", false);

            let signal = mgr.signals(bubble).next().unwrap();
            assert!(!signal.visible);
        });
    }
}
