//! Indicator sets — named groups of indicator templates.
//!
//! An [`IndicatorSet`] bundles several [`IndicatorTemplate`]s under a single
//! name, making it easy to restore a complete indicator layout (e.g. a
//! "scalping setup" with EMA + RSI + volume) in one action.

use serde::{Deserialize, Serialize};

use crate::preset::preset::unix_timestamp_parts;
use super::indicator_template::IndicatorTemplate;

// =============================================================================
// IndicatorSet
// =============================================================================

/// A named collection of indicator templates.
///
/// Saving and loading an [`IndicatorSet`] replaces (or augments) the active
/// indicator layout with the stored set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorSet {
    /// Unique identifier generated at creation time.
    /// Format: `"iset_{unix_secs}_{nanos_suffix}"`.
    pub id: String,
    /// User-visible display name for this set.
    pub name: String,
    /// Ordered list of indicator templates that form this set.
    pub indicators: Vec<IndicatorTemplate>,
}

impl IndicatorSet {
    /// Create an empty indicator set with the given name.
    pub fn new(name: &str) -> Self {
        let (secs, nanos) = unix_timestamp_parts();
        Self {
            id: format!("iset_{}_{}", secs, nanos),
            name: name.to_string(),
            indicators: Vec::new(),
        }
    }

    /// Create an indicator set from an existing list of templates.
    pub fn from_templates(name: &str, indicators: Vec<IndicatorTemplate>) -> Self {
        let (secs, nanos) = unix_timestamp_parts();
        Self {
            id: format!("iset_{}_{}", secs, nanos),
            name: name.to_string(),
            indicators,
        }
    }

    /// Add an indicator template to this set.
    pub fn add(&mut self, template: IndicatorTemplate) {
        self.indicators.push(template);
    }

    /// Remove an indicator template by its `id`.
    ///
    /// Returns `true` if a template with the given `id` was found and removed.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.indicators.len();
        self.indicators.retain(|t| t.id != id);
        self.indicators.len() < before
    }

    /// Returns the number of indicator templates in this set.
    pub fn len(&self) -> usize {
        self.indicators.len()
    }

    /// Returns `true` if the set contains no indicator templates.
    pub fn is_empty(&self) -> bool {
        self.indicators.is_empty()
    }

    /// Append an indicator template to this set.
    ///
    /// Alias for [`add`] with the name expected by [`IndicatorSetManager`].
    pub fn add_indicator(&mut self, template: IndicatorTemplate) {
        self.indicators.push(template);
    }

    /// Remove an indicator template by its positional index.
    ///
    /// No-op if `index` is out of bounds.
    pub fn remove_indicator(&mut self, index: usize) {
        if index < self.indicators.len() {
            self.indicators.remove(index);
        }
    }

    /// Move an indicator template from position `from` to position `to`.
    ///
    /// Both indices are clamped; no-op if they are equal or out of bounds.
    pub fn reorder_indicator(&mut self, from: usize, to: usize) {
        if from == to {
            return;
        }
        let len = self.indicators.len();
        if from >= len || to >= len {
            return;
        }
        let item = self.indicators.remove(from);
        let insert_at = if to > from {
            to.min(self.indicators.len())
        } else {
            to
        };
        self.indicators.insert(insert_at, item);
    }
}
