//! Timeframe Management
//!
//! Manages current timeframe and available timeframes for the chart.

use serde::{Deserialize, Serialize};

/// Represents a chart timeframe
///
/// The `weight` field is private and computed from `minutes` — it is excluded
/// from serialization. On deserialization the value is reconstructed via the
/// `TimeframeHelper` newtype so that `weight` is always recomputed correctly.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "TimeframeHelper")]
pub struct Timeframe {
    /// Display name (e.g., "1H", "4H", "1D")
    pub name: String,
    /// Duration in minutes
    pub minutes: u32,
    /// Weight for sorting/comparison (higher = larger timeframe).
    /// Excluded from serde — always recomputed from `minutes` on deserialize.
    #[serde(skip)]
    weight: u32,
}

impl Timeframe {
    /// Create a new timeframe
    pub fn new(name: &str, minutes: u32) -> Self {
        Self {
            name: name.to_string(),
            minutes,
            weight: minutes,
        }
    }

    /// Create with explicit weight
    pub fn with_weight(name: &str, minutes: u32, weight: u32) -> Self {
        Self {
            name: name.to_string(),
            minutes,
            weight,
        }
    }

    /// Get the weight (for comparison)
    pub fn weight(&self) -> u32 {
        self.weight
    }

    /// Check if this is an intraday timeframe
    pub fn is_intraday(&self) -> bool {
        self.minutes < 1440 // Less than 1 day
    }

    /// Check if this is a daily or higher timeframe
    pub fn is_daily_or_higher(&self) -> bool {
        self.minutes >= 1440
    }

    // Standard timeframes
    pub fn m1() -> Self { Self::new("1m", 1) }
    pub fn m5() -> Self { Self::new("5m", 5) }
    pub fn m15() -> Self { Self::new("15m", 15) }
    pub fn m30() -> Self { Self::new("30m", 30) }
    pub fn h1() -> Self { Self::new("1H", 60) }
    pub fn h4() -> Self { Self::new("4H", 240) }
    pub fn d1() -> Self { Self::new("1D", 1440) }
    pub fn w1() -> Self { Self::new("1W", 10080) }
    pub fn mn1() -> Self { Self::new("1M", 43200) }
}

impl Default for Timeframe {
    fn default() -> Self {
        Self::h1()
    }
}

/// Private helper used by serde to deserialize [`Timeframe`].
///
/// Only `name` and `minutes` are stored in JSON. `weight` is recomputed via
/// [`Timeframe::new`] so that it is always consistent after deserialization.
#[derive(Deserialize)]
struct TimeframeHelper {
    name: String,
    minutes: u32,
}

impl From<TimeframeHelper> for Timeframe {
    fn from(h: TimeframeHelper) -> Self {
        // Use `new()` so that `weight` is always recomputed from `minutes`.
        Self::new(&h.name, h.minutes)
    }
}

/// Manages timeframe state
pub struct TimeframeManager {
    /// Current active timeframe
    current: Timeframe,
    /// Available timeframes
    available: Vec<Timeframe>,
    /// Favorite/pinned timeframes
    favorites: Vec<Timeframe>,
}

impl TimeframeManager {
    /// Create a new timeframe manager with default timeframes
    pub fn new() -> Self {
        let available = vec![
            Timeframe::m1(),
            Timeframe::m5(),
            Timeframe::m15(),
            Timeframe::m30(),
            Timeframe::h1(),
            Timeframe::h4(),
            Timeframe::d1(),
            Timeframe::w1(),
            Timeframe::mn1(),
        ];

        let favorites = vec![
            Timeframe::m15(),
            Timeframe::h1(),
            Timeframe::h4(),
            Timeframe::d1(),
        ];

        Self {
            current: Timeframe::h1(),
            available,
            favorites,
        }
    }

    /// Get the current timeframe
    pub fn current(&self) -> &Timeframe {
        &self.current
    }

    /// Set the current timeframe
    pub fn set_current(&mut self, tf: Timeframe) {
        self.current = tf;
    }

    /// Get available timeframes
    pub fn available(&self) -> &[Timeframe] {
        &self.available
    }

    /// Get favorite timeframes
    pub fn favorites(&self) -> &[Timeframe] {
        &self.favorites
    }

    /// Add a timeframe to favorites
    pub fn add_favorite(&mut self, tf: Timeframe) {
        if !self.favorites.contains(&tf) {
            self.favorites.push(tf);
            self.favorites.sort_by_key(|t| t.minutes);
        }
    }

    /// Remove a timeframe from favorites
    pub fn remove_favorite(&mut self, tf: &Timeframe) {
        self.favorites.retain(|t| t != tf);
    }

    /// Check if a timeframe is in favorites
    pub fn is_favorite(&self, tf: &Timeframe) -> bool {
        self.favorites.contains(tf)
    }

    /// Get the next higher timeframe
    pub fn next_higher(&self) -> Option<&Timeframe> {
        let current_weight = self.current.weight();
        self.available
            .iter()
            .filter(|tf| tf.weight() > current_weight)
            .min_by_key(|tf| tf.weight())
    }

    /// Get the next lower timeframe
    pub fn next_lower(&self) -> Option<&Timeframe> {
        let current_weight = self.current.weight();
        self.available
            .iter()
            .filter(|tf| tf.weight() < current_weight)
            .max_by_key(|tf| tf.weight())
    }

    /// Move to next higher timeframe
    pub fn go_higher(&mut self) -> bool {
        if let Some(tf) = self.next_higher().cloned() {
            self.current = tf;
            true
        } else {
            false
        }
    }

    /// Move to next lower timeframe
    pub fn go_lower(&mut self) -> bool {
        if let Some(tf) = self.next_lower().cloned() {
            self.current = tf;
            true
        } else {
            false
        }
    }

    /// Add a custom timeframe
    pub fn add_custom(&mut self, tf: Timeframe) {
        if !self.available.contains(&tf) {
            self.available.push(tf);
            self.available.sort_by_key(|t| t.minutes);
        }
    }
}

impl Default for TimeframeManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeframe_comparison() {
        let h1 = Timeframe::h1();
        let h4 = Timeframe::h4();
        let d1 = Timeframe::d1();

        assert!(h1.weight() < h4.weight());
        assert!(h4.weight() < d1.weight());
    }

    #[test]
    fn test_timeframe_categories() {
        assert!(Timeframe::h1().is_intraday());
        assert!(Timeframe::h4().is_intraday());
        assert!(!Timeframe::d1().is_intraday());
        assert!(Timeframe::d1().is_daily_or_higher());
    }

    #[test]
    fn test_manager_navigation() {
        let mut manager = TimeframeManager::new();
        manager.set_current(Timeframe::h1());

        assert!(manager.go_higher());
        assert_eq!(manager.current().name, "4H");

        assert!(manager.go_lower());
        assert_eq!(manager.current().name, "1H");
    }

    #[test]
    fn test_favorites() {
        let mut manager = TimeframeManager::new();

        assert!(manager.is_favorite(&Timeframe::h1()));

        let custom = Timeframe::new("2H", 120);
        manager.add_favorite(custom.clone());
        assert!(manager.is_favorite(&custom));

        manager.remove_favorite(&custom);
        assert!(!manager.is_favorite(&custom));
    }
}
