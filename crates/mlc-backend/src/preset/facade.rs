//! `Presets` / `PresetsMut` — the only doctrinally-sanctioned access path to
//! `PresetManager` from outside `mlc-backend`.
//!
//! `PresetManager` never touches `DataBubble` — it owns the preset registry
//! as its own state. Like `WatchlistManager`/`TagManager`, the facade holds
//! exactly one guard, no `BubbleStore` co-acquisition.
//!
//! See `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A.

use std::sync::{RwLockReadGuard, RwLockWriteGuard};

use mlc_core::preset::preset::ChartPreset;

use super::manager::PresetManager;

/// Read-only facade over `PresetManager`.
pub struct Presets<'a> {
    mgr: RwLockReadGuard<'a, PresetManager>,
}

impl<'a> Presets<'a> {
    pub(crate) fn new(mgr: RwLockReadGuard<'a, PresetManager>) -> Self {
        Self { mgr }
    }

    pub fn get(&self, id: &str) -> Option<&ChartPreset> {
        self.mgr.get(id)
    }

    pub fn contains_key(&self, id: &str) -> bool {
        self.mgr.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.mgr.len()
    }

    pub fn is_empty(&self) -> bool {
        self.mgr.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &ChartPreset)> {
        self.mgr.iter()
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.mgr.keys()
    }

    pub fn values(&self) -> impl Iterator<Item = &ChartPreset> {
        self.mgr.values()
    }

    /// Snapshot the whole registry (cross-window propagate-back).
    pub fn to_map(&self) -> std::collections::HashMap<String, ChartPreset> {
        self.mgr.to_map()
    }
}

/// Mutable facade over `PresetManager`.
pub struct PresetsMut<'a> {
    mgr: RwLockWriteGuard<'a, PresetManager>,
}

impl<'a> PresetsMut<'a> {
    pub(crate) fn new(mgr: RwLockWriteGuard<'a, PresetManager>) -> Self {
        Self { mgr }
    }

    pub fn get(&self, id: &str) -> Option<&ChartPreset> {
        self.mgr.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut ChartPreset> {
        self.mgr.get_mut(id)
    }

    pub fn contains_key(&self, id: &str) -> bool {
        self.mgr.contains_key(id)
    }

    pub fn insert(&mut self, id: String, preset: ChartPreset) -> Option<ChartPreset> {
        self.mgr.insert(id, preset)
    }

    pub fn remove(&mut self, id: &str) -> Option<ChartPreset> {
        self.mgr.remove(id)
    }

    /// Insert `preset` under `id` only if `id` is not already present.
    pub fn insert_if_absent(&mut self, id: String, make: impl FnOnce() -> ChartPreset) {
        self.mgr.insert_if_absent(id, make);
    }

    pub fn len(&self) -> usize {
        self.mgr.len()
    }

    pub fn is_empty(&self) -> bool {
        self.mgr.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &ChartPreset)> {
        self.mgr.iter()
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.mgr.keys()
    }

    pub fn values(&self) -> impl Iterator<Item = &ChartPreset> {
        self.mgr.values()
    }

    /// Replace the whole registry wholesale (startup load, cross-window sync,
    /// import).
    pub fn replace_all(&mut self, presets: std::collections::HashMap<String, ChartPreset>) {
        self.mgr.replace_all(presets);
    }

    /// Snapshot the whole registry (cross-window propagate-back).
    pub fn to_map(&self) -> std::collections::HashMap<String, ChartPreset> {
        self.mgr.to_map()
    }
}
