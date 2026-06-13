//! Timeframe Management
//!
//! Manages current timeframe and available timeframes for the chart.

use serde::{Deserialize, Serialize};

/// Represents a chart timeframe
///
/// The `weight` field is private and computed from `interval_ms` — it is excluded
/// from serialization. On deserialization the value is reconstructed via the
/// `TimeframeHelper` newtype so that `weight` is always recomputed correctly.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Timeframe {
    /// Display name (e.g., "1H", "4H", "1D")
    pub name: String,
    /// Duration in milliseconds
    pub interval_ms: i64,
    /// Weight for sorting/comparison (higher = larger timeframe).
    /// Excluded from serde — always recomputed from `interval_ms` on deserialize.
    weight: i64,
}

impl Serialize for Timeframe {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("Timeframe", 2)?;
        s.serialize_field("name", &self.name)?;
        s.serialize_field("interval_ms", &self.interval_ms)?;
        s.end()
    }
}

impl<'de> Deserialize<'de> for Timeframe {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let helper = TimeframeHelper::deserialize(deserializer)?;
        Ok(Timeframe::from(helper))
    }
}

impl Timeframe {
    /// Create a new timeframe
    pub fn new(name: &str, interval_ms: i64) -> Self {
        Self {
            name: name.to_string(),
            interval_ms,
            weight: interval_ms,
        }
    }

    /// Get the weight (for comparison)
    pub fn weight(&self) -> i64 {
        self.weight
    }

    /// Check if this is an intraday timeframe
    pub fn is_intraday(&self) -> bool {
        self.interval_ms < 86_400_000 // Less than 1 day
    }

    /// Check if this is a daily or higher timeframe
    pub fn is_daily_or_higher(&self) -> bool {
        self.interval_ms >= 86_400_000
    }

    // Sub-second and second timeframes
    pub fn s1() -> Self { Self::new("1s", 1_000) }
    pub fn s3() -> Self { Self::new("3s", 3_000) }
    pub fn s5() -> Self { Self::new("5s", 5_000) }
    pub fn s10() -> Self { Self::new("10s", 10_000) }
    pub fn s15() -> Self { Self::new("15s", 15_000) }
    pub fn s30() -> Self { Self::new("30s", 30_000) }

    // Standard timeframes
    pub fn m1() -> Self { Self::new("1m", 60_000) }
    pub fn m5() -> Self { Self::new("5m", 300_000) }
    pub fn m15() -> Self { Self::new("15m", 900_000) }
    pub fn m30() -> Self { Self::new("30m", 1_800_000) }
    pub fn h1() -> Self { Self::new("1H", 3_600_000) }
    pub fn h4() -> Self { Self::new("4H", 14_400_000) }
    pub fn d1() -> Self { Self::new("1D", 86_400_000) }
    pub fn w1() -> Self { Self::new("1W", 604_800_000) }
    pub fn mn1() -> Self { Self::new("1M", 2_592_000_000) }
}

impl Default for Timeframe {
    fn default() -> Self {
        Self::h1()
    }
}

/// Private helper used by serde to deserialize [`Timeframe`].
///
/// Supports both the old format `{"name":"1H","minutes":60}` and the new
/// format `{"name":"1H","interval_ms":3600000}`. When both are present,
/// `interval_ms` takes precedence.
#[derive(Deserialize)]
struct TimeframeHelper {
    name: String,
    #[serde(default)]
    interval_ms: Option<i64>,
    #[serde(default)]
    minutes: Option<u32>,
}

impl From<TimeframeHelper> for Timeframe {
    fn from(h: TimeframeHelper) -> Self {
        let ms = if let Some(ms) = h.interval_ms {
            ms
        } else if let Some(m) = h.minutes {
            (m as i64) * 60_000
        } else {
            60_000 // default 1m
        };
        Self::new(&h.name, ms)
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
            Timeframe::s1(),
            Timeframe::s3(),
            Timeframe::s5(),
            Timeframe::s10(),
            Timeframe::s15(),
            Timeframe::s30(),
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
            self.favorites.sort_by_key(|t| t.interval_ms);
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
            self.available.sort_by_key(|t| t.interval_ms);
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

        let custom = Timeframe::new("2H", 7_200_000);
        manager.add_favorite(custom.clone());
        assert!(manager.is_favorite(&custom));

        manager.remove_favorite(&custom);
        assert!(!manager.is_favorite(&custom));
    }

    #[test]
    fn test_serde_backward_compat() {
        // Old format with minutes field
        let old_json = r#"{"name":"1H","minutes":60}"#;
        let tf: Timeframe = serde_json::from_str(old_json).unwrap();
        assert_eq!(tf.interval_ms, 3_600_000);
        assert_eq!(tf.name, "1H");

        // New format with interval_ms field
        let new_json = r#"{"name":"1H","interval_ms":3600000}"#;
        let tf2: Timeframe = serde_json::from_str(new_json).unwrap();
        assert_eq!(tf2.interval_ms, 3_600_000);

        // Serialize writes new format
        let serialized = serde_json::to_string(&tf).unwrap();
        assert!(serialized.contains("interval_ms"));
        assert!(!serialized.contains("minutes"));
    }

    #[test]
    fn test_sub_second_timeframes() {
        assert_eq!(Timeframe::s1().interval_ms, 1_000);
        assert_eq!(Timeframe::s30().interval_ms, 30_000);
        assert!(Timeframe::s1().is_intraday());
        assert!(!Timeframe::s1().is_daily_or_higher());
    }
}
