//! Timeframe Management
//!
//! Manages current timeframe and available timeframes for the chart.

use serde::{Deserialize, Serialize};

/// What kind of bar boundary a timeframe defines.
///
/// A timeframe is no longer purely time-based: tick timeframes (N trades per
/// bar) live in the same selector as 1m / 1H / 1D but have no wall-clock
/// duration. `Time` keeps the legacy behavior; `Tick` selects the dig3 TickBar
/// stream and forces the chart onto the time-domain X axis (bars are
/// non-uniform in time). Range/Renko are NOT here — they are chart-type render
/// transforms of whatever TF is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TfKind {
    /// Wall-clock interval bars (kline). The `interval_ms` is the real duration.
    Time,
    /// Tick bars: a new bar every `count` trades. `interval_ms` is unused for
    /// aggregation (only a synthetic sort weight); the real param is `count`.
    Tick { count: u32 },
}

impl Default for TfKind {
    fn default() -> Self {
        TfKind::Time
    }
}

/// Represents a chart timeframe
///
/// The `weight` field is private and computed from `interval_ms` — it is excluded
/// from serialization. On deserialization the value is reconstructed via the
/// `TimeframeHelper` newtype so that `weight` is always recomputed correctly.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Timeframe {
    /// Display name (e.g., "1H", "4H", "1D", "100t")
    pub name: String,
    /// Duration in milliseconds (time TFs) or a synthetic sort weight (tick TFs).
    pub interval_ms: i64,
    /// Time vs tick aggregation. Serde-default `Time` so old presets load.
    pub kind: TfKind,
    /// Weight for sorting/comparison (higher = larger timeframe).
    /// Excluded from serde — always recomputed from `interval_ms` on deserialize.
    weight: i64,
}

impl Serialize for Timeframe {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        // Persist tick count only for tick TFs so old readers ignore it and new
        // readers reconstruct the kind. Time TFs serialize exactly as before.
        let tick_count: Option<u32> = match self.kind {
            TfKind::Time => None,
            TfKind::Tick { count } => Some(count),
        };
        let mut s = serializer.serialize_struct("Timeframe", 3)?;
        s.serialize_field("name", &self.name)?;
        s.serialize_field("interval_ms", &self.interval_ms)?;
        s.serialize_field("tick_count", &tick_count)?;
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
    /// Create a new time-based timeframe
    pub fn new(name: &str, interval_ms: i64) -> Self {
        Self {
            name: name.to_string(),
            interval_ms,
            kind: TfKind::Time,
            weight: interval_ms,
        }
    }

    /// Create a tick-based timeframe (`count` trades per bar).
    ///
    /// `interval_ms` carries a synthetic sort weight so tick TFs order BELOW
    /// the smallest time TF (they're the finest resolution). The real
    /// aggregation parameter is `count`.
    pub fn tick(count: u32) -> Self {
        // Synthetic weight in a negative band so ALL tick TFs sort strictly
        // below the smallest time TF (1s = 1000), while staying ordered by count
        // (1t < 10t < 100t < 1000t).
        let weight = count as i64 - 1_000_000;
        Self {
            name: format!("{}t", count),
            interval_ms: weight,
            kind: TfKind::Tick { count },
            weight,
        }
    }

    /// `true` if this is a tick timeframe.
    pub fn is_tick(&self) -> bool {
        matches!(self.kind, TfKind::Tick { .. })
    }

    /// The tick count, if this is a tick TF.
    pub fn tick_count(&self) -> Option<u32> {
        match self.kind {
            TfKind::Tick { count } => Some(count),
            TfKind::Time => None,
        }
    }

    // Tick timeframes (live in the TF selector, category "Ticks").
    pub fn t1() -> Self { Self::tick(1) }
    pub fn t10() -> Self { Self::tick(10) }
    pub fn t100() -> Self { Self::tick(100) }
    pub fn t1000() -> Self { Self::tick(1000) }

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

    /// Parse a `<count><unit>` timeframe name (e.g. `"7m"`, `"2H"`, `"90m"`,
    /// `"1D"`) into a [`Timeframe`], generalizing the closed per-name match
    /// tables that used to live independently in `mlc-app::parse_timeframe_name`
    /// and `ChangeTimeframe`'s dispatch (user-granularity-templates-inner
    /// arc, кусок 1).
    ///
    /// Unit letters: `s` seconds, `m` minutes, `h`/`H` hours, `d`/`D` days,
    /// `w`/`W` weeks, `M` months (~30 days, matching [`Self::mn1`]'s
    /// `interval_ms`). The case split is deliberately narrow: only `m`
    /// (minute) vs `M` (month) is ambiguous on a bare letter, so lowercase
    /// `m` always means minute and uppercase `M` always means month; every
    /// other unit letter is accepted in either case for convenience since it
    /// has no collision.
    ///
    /// Output name casing mirrors the existing named constructors exactly
    /// (`"1m"`, `"1H"`, `"1D"`, `"1W"`, `"1M"` for hour/day/week/month;
    /// seconds/minutes stay lowercase) so a parsed builtin TF is
    /// field-for-field identical to calling its named constructor.
    ///
    /// Rejects: empty/non-numeric count, count of `0`, counts longer than 4
    /// digits (guards against garbage like `"99999999m"`), unknown unit
    /// letters, and any other malformed input. Returns `None` for all of
    /// these — callers keep their existing "unknown timeframe_id" logging.
    pub fn parse(name: &str) -> Option<Self> {
        let split_at = name.find(|c: char| !c.is_ascii_digit())?;
        let (digits, unit) = name.split_at(split_at);
        if digits.is_empty() || digits.len() > 4 {
            return None;
        }
        let count: i64 = digits.parse().ok()?;
        if count == 0 {
            return None;
        }
        // Reject anything past the single unit letter (e.g. "5ms", "5mm").
        if unit.chars().count() != 1 {
            return None;
        }
        let unit_char = unit.chars().next()?;

        match unit_char {
            's' => Some(Self::new(&format!("{}s", count), count * 1_000)),
            'm' => Some(Self::new(&format!("{}m", count), count * 60_000)),
            'h' | 'H' => Some(Self::new(&format!("{}H", count), count * 3_600_000)),
            'd' | 'D' => Some(Self::new(&format!("{}D", count), count * 86_400_000)),
            'w' | 'W' => Some(Self::new(&format!("{}W", count), count * 604_800_000)),
            'M' => Some(Self::new(&format!("{}M", count), count * 2_592_000_000)),
            _ => None,
        }
    }
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
    #[serde(default)]
    tick_count: Option<u32>,
}

impl From<TimeframeHelper> for Timeframe {
    fn from(h: TimeframeHelper) -> Self {
        // Tick TF reconstructs from tick_count regardless of interval_ms.
        if let Some(count) = h.tick_count {
            return Timeframe::tick(count);
        }
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
    fn test_tick_timeframes() {
        let t = Timeframe::t100();
        assert!(t.is_tick());
        assert_eq!(t.tick_count(), Some(100));
        assert_eq!(t.name, "100t");
        assert!(!Timeframe::h1().is_tick());
        assert_eq!(Timeframe::h1().tick_count(), None);
        // Tick TFs sort below the smallest time TF.
        assert!(Timeframe::t1000().weight() < Timeframe::s1().weight());
        assert!(Timeframe::t1().weight() < Timeframe::t10().weight());
    }

    #[test]
    fn test_tick_serde_roundtrip() {
        let t = Timeframe::t100();
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("tick_count"));
        let back: Timeframe = serde_json::from_str(&json).unwrap();
        assert!(back.is_tick());
        assert_eq!(back.tick_count(), Some(100));
        // Old time-TF json (no tick_count) still deserializes as Time.
        let old: Timeframe = serde_json::from_str(r#"{"name":"1H","interval_ms":3600000}"#).unwrap();
        assert!(!old.is_tick());
    }

    #[test]
    fn test_sub_second_timeframes() {
        assert_eq!(Timeframe::s1().interval_ms, 1_000);
        assert_eq!(Timeframe::s30().interval_ms, 30_000);
        assert!(Timeframe::s1().is_intraday());
        assert!(!Timeframe::s1().is_daily_or_higher());
    }

    /// `Timeframe::parse` must reproduce every one of the 17 builtin named
    /// constructors field-for-field (user-granularity-templates-inner arc,
    /// кусок 1) — the parser is the single source of truth the three former
    /// independent closed match tables (`ALL_TF_IDS`, `ChangeTimeframe`,
    /// `parse_timeframe_name`) now funnel through.
    #[test]
    fn parse_matches_all_builtin_named_ctors() {
        let cases: &[(&str, Timeframe)] = &[
            ("1s", Timeframe::s1()),
            ("5s", Timeframe::s5()),
            ("15s", Timeframe::s15()),
            ("30s", Timeframe::s30()),
            ("1m", Timeframe::m1()),
            ("3m", Timeframe::new("3m", 3 * 60_000)),
            ("5m", Timeframe::m5()),
            ("15m", Timeframe::m15()),
            ("30m", Timeframe::m30()),
            ("1h", Timeframe::h1()),
            ("2h", Timeframe::new("2H", 2 * 3_600_000)),
            ("4h", Timeframe::h4()),
            ("6h", Timeframe::new("6H", 6 * 3_600_000)),
            ("12h", Timeframe::new("12H", 12 * 3_600_000)),
            ("1d", Timeframe::d1()),
            ("1w", Timeframe::w1()),
            ("1M", Timeframe::mn1()),
        ];
        for (input, expected) in cases {
            let parsed = Timeframe::parse(input)
                .unwrap_or_else(|| panic!("parse({input:?}) returned None"));
            assert_eq!(parsed, *expected, "parse({input:?}) mismatch");
        }

        // Uppercase hour/day/week inputs must resolve identically to their
        // lowercase counterparts above (both forms accepted, single output).
        assert_eq!(Timeframe::parse("1H"), Some(Timeframe::h1()));
        assert_eq!(Timeframe::parse("1D"), Some(Timeframe::d1()));
        assert_eq!(Timeframe::parse("1W"), Some(Timeframe::w1()));
    }

    #[test]
    fn parse_custom_timeframes_produce_sane_interval_ms() {
        let seven_m = Timeframe::parse("7m").expect("7m must parse");
        assert_eq!(seven_m.name, "7m");
        assert_eq!(seven_m.interval_ms, 7 * 60_000);
        assert_eq!(seven_m.kind, TfKind::Time);

        let ninety_m = Timeframe::parse("90m").expect("90m must parse");
        assert_eq!(ninety_m.name, "90m");
        assert_eq!(ninety_m.interval_ms, 90 * 60_000);

        let ten_s = Timeframe::parse("10s").expect("10s must parse");
        assert_eq!(ten_s.name, "10s");
        assert_eq!(ten_s.interval_ms, 10_000);

        let two_d = Timeframe::parse("2D").expect("2D must parse");
        assert_eq!(two_d.name, "2D");
        assert_eq!(two_d.interval_ms, 2 * 86_400_000);
    }

    #[test]
    fn parse_rejects_invalid_input() {
        assert_eq!(Timeframe::parse("0m"), None, "zero count must be rejected");
        assert_eq!(Timeframe::parse("banana"), None, "non-numeric input must be rejected");
        assert_eq!(Timeframe::parse("1x"), None, "unknown unit letter must be rejected");
        assert_eq!(Timeframe::parse(""), None, "empty input must be rejected");
        assert_eq!(Timeframe::parse("m"), None, "missing count must be rejected");
        assert_eq!(Timeframe::parse("99999m"), None, "5-digit count must be rejected");
        assert_eq!(Timeframe::parse("5ms"), None, "trailing garbage after unit must be rejected");
    }
}
