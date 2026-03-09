//! Visibility Management
//!
//! Centralized visibility tracking for all chart objects.

use std::collections::HashMap;

/// Manages visibility state for all objects
pub struct VisibilityManager {
    /// Visibility state per object ID
    states: HashMap<u64, bool>,
    /// Default visibility for new objects
    default_visible: bool,
}

impl VisibilityManager {
    /// Create a new visibility manager
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
            default_visible: true,
        }
    }

    /// Register an object with initial visibility
    pub fn register(&mut self, object_id: u64, visible: bool) {
        self.states.insert(object_id, visible);
    }

    /// Unregister an object
    pub fn unregister(&mut self, object_id: u64) {
        self.states.remove(&object_id);
    }

    /// Check if an object is visible
    pub fn is_visible(&self, object_id: u64) -> bool {
        self.states.get(&object_id).copied().unwrap_or(self.default_visible)
    }

    /// Set visibility for an object
    pub fn set(&mut self, object_id: u64, visible: bool) {
        self.states.insert(object_id, visible);
    }

    /// Toggle visibility for an object
    pub fn toggle(&mut self, object_id: u64) -> bool {
        let current = self.is_visible(object_id);
        let new_state = !current;
        self.states.insert(object_id, new_state);
        new_state
    }

    /// Get all visible object IDs
    pub fn visible_objects(&self) -> Vec<u64> {
        self.states
            .iter()
            .filter(|(_, &visible)| visible)
            .map(|(&id, _)| id)
            .collect()
    }

    /// Get all hidden object IDs
    pub fn hidden_objects(&self) -> Vec<u64> {
        self.states
            .iter()
            .filter(|(_, &visible)| !visible)
            .map(|(&id, _)| id)
            .collect()
    }

    /// Hide all objects
    pub fn hide_all(&mut self) {
        for visible in self.states.values_mut() {
            *visible = false;
        }
    }

    /// Show all objects
    pub fn show_all(&mut self) {
        for visible in self.states.values_mut() {
            *visible = true;
        }
    }

    /// Get count of visible objects
    pub fn visible_count(&self) -> usize {
        self.states.values().filter(|&&v| v).count()
    }

    /// Get count of hidden objects
    pub fn hidden_count(&self) -> usize {
        self.states.values().filter(|&&v| !v).count()
    }

    /// Get total object count
    pub fn total_count(&self) -> usize {
        self.states.len()
    }
}

impl Default for VisibilityManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_visibility() {
        let mut manager = VisibilityManager::new();

        manager.register(1, true);
        manager.register(2, false);

        assert!(manager.is_visible(1));
        assert!(!manager.is_visible(2));
        // Unregistered objects use default
        assert!(manager.is_visible(999));
    }

    #[test]
    fn test_toggle() {
        let mut manager = VisibilityManager::new();
        manager.register(1, true);

        assert!(manager.is_visible(1));

        let new_state = manager.toggle(1);
        assert!(!new_state);
        assert!(!manager.is_visible(1));

        let new_state = manager.toggle(1);
        assert!(new_state);
        assert!(manager.is_visible(1));
    }

    #[test]
    fn test_hide_show_all() {
        let mut manager = VisibilityManager::new();
        manager.register(1, true);
        manager.register(2, true);
        manager.register(3, false);

        manager.hide_all();
        assert!(!manager.is_visible(1));
        assert!(!manager.is_visible(2));
        assert!(!manager.is_visible(3));

        manager.show_all();
        assert!(manager.is_visible(1));
        assert!(manager.is_visible(2));
        assert!(manager.is_visible(3));
    }
}
