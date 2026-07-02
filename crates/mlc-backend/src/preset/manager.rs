//! `PresetManager` — CRUD over the in-memory chart-preset registry.
//!
//! Wraps `HashMap<String, ChartPreset>` (keyed by preset id). `ChartPreset`
//! itself stays the domain value type in `mlc-core` (doctrine 1A — the
//! manager owns the registry, not the value type). Never serialized
//! wholesale: each `ChartPreset` is persisted individually via
//! `mlc_core::preset::storage::{save_preset, load_preset}`
//! (`mlc-app/src/user_manager/manager.rs`, `mlc-app/src/lib.rs`).

use std::collections::HashMap;

use mlc_core::preset::preset::ChartPreset;

/// In-memory registry of chart presets keyed by preset id.
#[derive(Debug, Default)]
pub struct PresetManager {
    presets: HashMap<String, ChartPreset>,
}

impl PresetManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct directly from a loaded map (startup / cross-window sync).
    pub fn from_map(presets: HashMap<String, ChartPreset>) -> Self {
        Self { presets }
    }

    pub fn get(&self, id: &str) -> Option<&ChartPreset> {
        self.presets.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut ChartPreset> {
        self.presets.get_mut(id)
    }

    pub fn contains_key(&self, id: &str) -> bool {
        self.presets.contains_key(id)
    }

    pub fn insert(&mut self, id: String, preset: ChartPreset) -> Option<ChartPreset> {
        self.presets.insert(id, preset)
    }

    pub fn remove(&mut self, id: &str) -> Option<ChartPreset> {
        self.presets.remove(id)
    }

    /// Insert `preset` under `id` only if `id` is not already present. Mirrors
    /// `HashMap::entry(id).or_insert_with(f)` without leaking the `Entry` API.
    pub fn insert_if_absent(&mut self, id: String, make: impl FnOnce() -> ChartPreset) {
        self.presets.entry(id).or_insert_with(make);
    }

    pub fn len(&self) -> usize {
        self.presets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.presets.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &ChartPreset)> {
        self.presets.iter()
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.presets.keys()
    }

    pub fn values(&self) -> impl Iterator<Item = &ChartPreset> {
        self.presets.values()
    }

    /// Replace the whole registry wholesale (startup load, cross-window sync,
    /// import). Mirrors `WatchlistsMut::replace_all`.
    pub fn replace_all(&mut self, presets: HashMap<String, ChartPreset>) {
        self.presets = presets;
    }

    /// Snapshot the whole registry (cross-window propagate-back). Mirrors the
    /// `for (k, v) in core.presets().iter() { app_state.presets.entry(...) }`
    /// call sites in `mlc-app-shell-vello/src/main.rs`.
    pub fn to_map(&self) -> HashMap<String, ChartPreset> {
        self.presets.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_get_remove_roundtrip() {
        let mut mgr = PresetManager::new();
        assert!(mgr.is_empty());
        let preset = ChartPreset::new("Untitled".to_string());
        let id = preset.id.clone();
        mgr.insert(id.clone(), preset);
        assert_eq!(mgr.len(), 1);
        assert!(mgr.contains_key(&id));
        assert_eq!(mgr.get(&id).map(|p| p.name.as_str()), Some("Untitled"));
        let removed = mgr.remove(&id);
        assert!(removed.is_some());
        assert!(mgr.is_empty());
    }

    #[test]
    fn insert_if_absent_does_not_overwrite() {
        let mut mgr = PresetManager::new();
        let mut first = ChartPreset::new("First".to_string());
        first.id = "p1".to_string();
        mgr.insert(first.id.clone(), first);
        mgr.insert_if_absent("p1".to_string(), || ChartPreset::new("Second".to_string()));
        assert_eq!(mgr.get("p1").map(|p| p.name.as_str()), Some("First"));
    }

    #[test]
    fn replace_all_swaps_registry() {
        let mut mgr = PresetManager::new();
        mgr.insert("a".to_string(), ChartPreset::new("A".to_string()));
        let mut replacement = HashMap::new();
        replacement.insert("b".to_string(), ChartPreset::new("B".to_string()));
        mgr.replace_all(replacement);
        assert!(!mgr.contains_key("a"));
        assert!(mgr.contains_key("b"));
    }
}
