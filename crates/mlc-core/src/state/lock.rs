//! Lock System
//!
//! Manages both global lock (locks all objects) and individual object locks.

use std::collections::HashMap;

/// Manages lock state for objects
pub struct LockManager {
    /// Global lock state
    global_locked: bool,
    /// Per-object lock states
    locks: HashMap<u64, bool>,
}

impl LockManager {
    /// Create a new lock manager
    pub fn new() -> Self {
        Self {
            global_locked: false,
            locks: HashMap::new(),
        }
    }

    /// Register an object with initial lock state
    pub fn register(&mut self, object_id: u64, locked: bool) {
        self.locks.insert(object_id, locked);
    }

    /// Unregister an object
    pub fn unregister(&mut self, object_id: u64) {
        self.locks.remove(&object_id);
    }

    /// Check if an object is individually locked
    pub fn is_locked(&self, object_id: u64) -> bool {
        self.locks.get(&object_id).copied().unwrap_or(false)
    }

    /// Set individual lock state
    pub fn set(&mut self, object_id: u64, locked: bool) {
        self.locks.insert(object_id, locked);
    }

    /// Toggle individual lock
    pub fn toggle(&mut self, object_id: u64) -> bool {
        let current = self.is_locked(object_id);
        let new_state = !current;
        self.locks.insert(object_id, new_state);
        new_state
    }

    /// Check if global lock is enabled
    pub fn is_global_locked(&self) -> bool {
        self.global_locked
    }

    /// Set global lock state
    pub fn set_global(&mut self, locked: bool) {
        self.global_locked = locked;
    }

    /// Toggle global lock
    pub fn toggle_global(&mut self) -> bool {
        self.global_locked = !self.global_locked;
        self.global_locked
    }

    /// Check if an object is effectively locked (individual OR global)
    ///
    /// An object is effectively locked if:
    /// - Global lock is enabled, OR
    /// - The object is individually locked
    pub fn is_effectively_locked(&self, object_id: u64) -> bool {
        self.global_locked || self.is_locked(object_id)
    }

    /// Get all individually locked object IDs
    pub fn locked_objects(&self) -> Vec<u64> {
        self.locks
            .iter()
            .filter(|(_, &locked)| locked)
            .map(|(&id, _)| id)
            .collect()
    }

    /// Get count of individually locked objects
    pub fn locked_count(&self) -> usize {
        self.locks.values().filter(|&&locked| locked).count()
    }

    /// Unlock all individual locks (doesn't affect global lock)
    pub fn unlock_all_individual(&mut self) {
        for locked in self.locks.values_mut() {
            *locked = false;
        }
    }

    /// Lock all objects individually
    pub fn lock_all_individual(&mut self) {
        for locked in self.locks.values_mut() {
            *locked = true;
        }
    }
}

impl Default for LockManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_individual_lock() {
        let mut manager = LockManager::new();

        manager.register(1, false);
        manager.register(2, true);

        assert!(!manager.is_locked(1));
        assert!(manager.is_locked(2));

        manager.set(1, true);
        assert!(manager.is_locked(1));
    }

    #[test]
    fn test_global_lock() {
        let mut manager = LockManager::new();

        manager.register(1, false);
        manager.register(2, false);

        assert!(!manager.is_effectively_locked(1));
        assert!(!manager.is_effectively_locked(2));

        manager.set_global(true);

        // Both should be effectively locked now
        assert!(manager.is_effectively_locked(1));
        assert!(manager.is_effectively_locked(2));

        // But individual lock states unchanged
        assert!(!manager.is_locked(1));
        assert!(!manager.is_locked(2));
    }

    #[test]
    fn test_toggle() {
        let mut manager = LockManager::new();
        manager.register(1, false);

        assert!(!manager.is_locked(1));
        manager.toggle(1);
        assert!(manager.is_locked(1));
        manager.toggle(1);
        assert!(!manager.is_locked(1));
    }

    #[test]
    fn test_effective_lock_combination() {
        let mut manager = LockManager::new();

        manager.register(1, true);  // Individually locked
        manager.register(2, false); // Not individually locked

        // Object 1 is effectively locked (individual)
        assert!(manager.is_effectively_locked(1));
        // Object 2 is not effectively locked
        assert!(!manager.is_effectively_locked(2));

        // Enable global lock
        manager.set_global(true);

        // Both are now effectively locked
        assert!(manager.is_effectively_locked(1));
        assert!(manager.is_effectively_locked(2));

        // Disable global lock
        manager.set_global(false);

        // Object 1 still effectively locked (individual)
        assert!(manager.is_effectively_locked(1));
        // Object 2 no longer effectively locked
        assert!(!manager.is_effectively_locked(2));
    }
}
