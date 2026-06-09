//! [`IndicatorSetManager`] — manages named collections of indicator templates.
//!
//! Mirrors the [`WatchlistManager`] pattern: stores a `Vec<IndicatorSet>` and
//! tracks which set is currently active via `active_index`.  Users can switch
//! between sets to quickly deploy different indicator combinations.

use serde::{Deserialize, Serialize};

use super::indicator_set::IndicatorSet;
use super::indicator_template::IndicatorTemplate;

// =============================================================================
// IndicatorSetManager
// =============================================================================

/// Manages multiple named indicator sets.
///
/// Each [`IndicatorSet`] bundles a collection of [`IndicatorTemplate`]s under a
/// user-defined name (e.g. "Scalping", "Swing Trading").  The manager keeps
/// track of the *active* set — the one currently applied to the chart.
///
/// # Persistence
///
/// The manager is serialized as a single JSON document (unlike the individual
/// [`IndicatorSet`] files stored by [`TemplateManager`]).  This allows the
/// active-index selection and set ordering to survive restarts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorSetManager {
    /// All available indicator sets, in display order.
    pub sets: Vec<IndicatorSet>,
    /// Index into `sets` for the currently active set, or `None` if no set is
    /// active (e.g. the manager is empty).
    pub active_index: Option<usize>,
}

impl IndicatorSetManager {
    /// Create an empty manager with no sets.
    pub fn new() -> Self {
        Self {
            sets: Vec::new(),
            active_index: None,
        }
    }

    // =========================================================================
    // Set management
    // =========================================================================

    /// Create a new empty set with the given name, append it to the list, and
    /// return its index.
    pub fn add_set(&mut self, name: &str) -> usize {
        let set = IndicatorSet::new(name);
        self.sets.push(set);
        self.sets.len() - 1
    }

    /// Snapshot the provided `indicators` into a new named set and return its
    /// index.
    ///
    /// This is the "Save As" path: the caller passes the current live
    /// indicators and this method stores them as an immutable set.
    pub fn add_set_from_current(&mut self, name: &str, indicators: Vec<IndicatorTemplate>) -> usize {
        let set = IndicatorSet::from_templates(name, indicators);
        self.sets.push(set);
        self.sets.len() - 1
    }

    /// Remove the set at `index`.
    ///
    /// If the removed set was active, `active_index` is cleared.  If the
    /// active set was after the removed one, its index is adjusted so it still
    /// points to the same set.
    ///
    /// No-op if `index` is out of bounds.
    pub fn remove_set(&mut self, index: usize) {
        if index >= self.sets.len() {
            return;
        }
        self.sets.remove(index);
        // Adjust active_index.
        self.active_index = match self.active_index {
            Some(ai) if ai == index => None,
            Some(ai) if ai > index => Some(ai - 1),
            other => other,
        };
    }

    /// Rename the set at `index`.
    ///
    /// No-op if `index` is out of bounds.
    pub fn rename_set(&mut self, index: usize, name: &str) {
        if let Some(set) = self.sets.get_mut(index) {
            set.name = name.to_string();
        }
    }

    // =========================================================================
    // Active set access
    // =========================================================================

    /// Return a reference to the currently active set, or `None`.
    pub fn active_set(&self) -> Option<&IndicatorSet> {
        self.active_index.and_then(|i| self.sets.get(i))
    }

    /// Return a mutable reference to the currently active set, or `None`.
    pub fn active_set_mut(&mut self) -> Option<&mut IndicatorSet> {
        self.active_index.and_then(|i| self.sets.get_mut(i))
    }

    /// Make the set at `index` the active set.
    ///
    /// Clamps to the last valid index if `index` is out of bounds and the
    /// list is non-empty; sets `active_index` to `None` if the list is empty.
    pub fn set_active(&mut self, index: usize) {
        if self.sets.is_empty() {
            self.active_index = None;
        } else {
            self.active_index = Some(index.min(self.sets.len() - 1));
        }
    }

    // =========================================================================
    // Random-access lookup
    // =========================================================================

    /// Return a reference to the set at `index`, or `None` if out of bounds.
    pub fn get_set(&self, index: usize) -> Option<&IndicatorSet> {
        self.sets.get(index)
    }

    /// Return the total number of sets.
    pub fn sets_count(&self) -> usize {
        self.sets.len()
    }
}

impl Default for IndicatorSetManager {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_manager_is_empty() {
        let mgr = IndicatorSetManager::new();
        assert_eq!(mgr.sets_count(), 0);
        assert!(mgr.active_index.is_none());
        assert!(mgr.active_set().is_none());
    }

    #[test]
    fn test_add_set_returns_index() {
        let mut mgr = IndicatorSetManager::new();
        let i0 = mgr.add_set("Scalping");
        let i1 = mgr.add_set("Swing");
        assert_eq!(i0, 0);
        assert_eq!(i1, 1);
        assert_eq!(mgr.sets_count(), 2);
    }

    #[test]
    fn test_add_set_from_current() {
        let mut mgr = IndicatorSetManager::new();
        let templates = vec![IndicatorTemplate::new_with_defaults("sma", "SMA 20")];
        let idx = mgr.add_set_from_current("My Setup", templates);
        assert_eq!(idx, 0);
        assert_eq!(mgr.sets[0].indicators.len(), 1);
    }

    #[test]
    fn test_set_active_and_retrieve() {
        let mut mgr = IndicatorSetManager::new();
        mgr.add_set("A");
        mgr.add_set("B");
        mgr.set_active(1);
        assert_eq!(mgr.active_index, Some(1));
        let set = mgr.active_set().unwrap();
        assert_eq!(set.name, "B");
    }

    #[test]
    fn test_remove_set_clears_active_when_active_removed() {
        let mut mgr = IndicatorSetManager::new();
        mgr.add_set("A");
        mgr.add_set("B");
        mgr.set_active(0);
        mgr.remove_set(0);
        assert!(mgr.active_index.is_none());
        assert_eq!(mgr.sets_count(), 1);
        assert_eq!(mgr.sets[0].name, "B");
    }

    #[test]
    fn test_remove_set_adjusts_active_index() {
        let mut mgr = IndicatorSetManager::new();
        mgr.add_set("A");
        mgr.add_set("B");
        mgr.add_set("C");
        mgr.set_active(2); // points to "C"
        mgr.remove_set(0); // removes "A"
        // active should now be index 1 (still "C")
        assert_eq!(mgr.active_index, Some(1));
        assert_eq!(mgr.active_set().unwrap().name, "C");
    }

    #[test]
    fn test_rename_set() {
        let mut mgr = IndicatorSetManager::new();
        mgr.add_set("Old Name");
        mgr.rename_set(0, "New Name");
        assert_eq!(mgr.sets[0].name, "New Name");
    }

    #[test]
    fn test_set_active_clamps_to_last() {
        let mut mgr = IndicatorSetManager::new();
        mgr.add_set("Only");
        mgr.set_active(999);
        assert_eq!(mgr.active_index, Some(0));
    }

    #[test]
    fn test_indicator_add_remove_reorder() {
        let mut set = IndicatorSet::new("Test");
        let t0 = IndicatorTemplate::new_with_defaults("sma", "SMA");
        let t1 = IndicatorTemplate::new_with_defaults("rsi", "RSI");
        let t2 = IndicatorTemplate::new_with_defaults("macd", "MACD");
        set.add_indicator(t0.clone());
        set.add_indicator(t1.clone());
        set.add_indicator(t2.clone());
        assert_eq!(set.len(), 3);

        // Reorder: move index 0 (SMA) to index 2 -> [RSI, MACD, SMA]
        set.reorder_indicator(0, 2);
        assert_eq!(set.indicators[0].type_id, "rsi");
        assert_eq!(set.indicators[1].type_id, "macd");
        assert_eq!(set.indicators[2].type_id, "sma");

        // Remove index 1 (MACD) -> [RSI, SMA]
        set.remove_indicator(1);
        assert_eq!(set.len(), 2);
        assert_eq!(set.indicators[0].type_id, "rsi");
        assert_eq!(set.indicators[1].type_id, "sma");
    }
}
