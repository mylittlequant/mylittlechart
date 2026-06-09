//! OHLC Data Generator
//!
//! Generates consistent, reproducible OHLC data for demo symbols.
//! Uses seeded random generation so the same symbol always produces the same data.
//!
//! # Multi-Timeframe Support
//!
//! Base data is generated at 1-minute resolution and then aggregated to higher timeframes.
//! This ensures consistent data across all timeframes (15m, 1h, 4h, 1D all derived from same base).

use crate::Bar;
use super::DemoSymbol;
use std::collections::HashMap;

// =============================================================================
// Demo Data Time Range
// =============================================================================

/// Duration of demo data in seconds (92 days)
const DEMO_DURATION_SECONDS: i64 = 92 * 24 * 3600;

/// Get demo period timestamps based on current system time.
/// End = current time (aligned to minute), Start = end - 92 days.
/// This ensures demo data always ends "now" and live ticks can seamlessly continue.
fn demo_period() -> (i64, i64) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let end = (now / 60) * 60; // Align to minute boundary
    let start = end - DEMO_DURATION_SECONDS;
    (start, end)
}

/// Supported demo timeframes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DemoTimeframe {
    M1,   // 1 minute (base)
    M5,   // 5 minutes
    M15,  // 15 minutes
    M30,  // 30 minutes
    H1,   // 1 hour
    H4,   // 4 hours
    D1,   // 1 day
    W1,   // 1 week
}

impl DemoTimeframe {
    /// Get timeframe in seconds
    pub fn seconds(&self) -> i64 {
        match self {
            DemoTimeframe::M1 => 60,
            DemoTimeframe::M5 => 300,
            DemoTimeframe::M15 => 900,
            DemoTimeframe::M30 => 1800,
            DemoTimeframe::H1 => 3600,
            DemoTimeframe::H4 => 14400,
            DemoTimeframe::D1 => 86400,
            DemoTimeframe::W1 => 604800, // 7 days
        }
    }

    /// Get number of bars for demo period (Oct 1, 2025 - Jan 1, 2026)
    pub fn demo_bar_count(&self) -> usize {
        (DEMO_DURATION_SECONDS / self.seconds()) as usize
    }

    /// Get display label
    pub fn label(&self) -> &'static str {
        match self {
            DemoTimeframe::M1 => "1m",
            DemoTimeframe::M5 => "5m",
            DemoTimeframe::M15 => "15m",
            DemoTimeframe::M30 => "30m",
            DemoTimeframe::H1 => "1H",
            DemoTimeframe::H4 => "4H",
            DemoTimeframe::D1 => "1D",
            DemoTimeframe::W1 => "1W",
        }
    }

    /// Get all supported timeframes
    pub fn all() -> &'static [DemoTimeframe] {
        &[
            DemoTimeframe::M1,
            DemoTimeframe::M5,
            DemoTimeframe::M15,
            DemoTimeframe::M30,
            DemoTimeframe::H1,
            DemoTimeframe::H4,
            DemoTimeframe::D1,
            DemoTimeframe::W1,
        ]
    }

    /// Get commonly used timeframes (for UI display)
    pub fn common() -> &'static [DemoTimeframe] {
        &[
            DemoTimeframe::M15,
            DemoTimeframe::H1,
            DemoTimeframe::H4,
            DemoTimeframe::D1,
            DemoTimeframe::W1,
        ]
    }

    /// Parse from label string (e.g., "1m", "5m", "1H", "4h", "1D", "1w")
    /// Case-insensitive matching
    pub fn from_label(label: &str) -> Option<Self> {
        match label.to_lowercase().as_str() {
            "1m" => Some(DemoTimeframe::M1),
            "5m" => Some(DemoTimeframe::M5),
            "15m" => Some(DemoTimeframe::M15),
            "30m" => Some(DemoTimeframe::M30),
            "1h" => Some(DemoTimeframe::H1),
            "4h" => Some(DemoTimeframe::H4),
            "1d" => Some(DemoTimeframe::D1),
            "1w" => Some(DemoTimeframe::W1),
            _ => None,
        }
    }

    /// Get visibility params for TimeframeVisibilityConfig::is_visible_on()
    /// Returns (timeframe_type, value) tuple
    pub fn visibility_params(&self) -> (&'static str, u32) {
        match self {
            DemoTimeframe::M1 => ("minutes", 1),
            DemoTimeframe::M5 => ("minutes", 5),
            DemoTimeframe::M15 => ("minutes", 15),
            DemoTimeframe::M30 => ("minutes", 30),
            DemoTimeframe::H1 => ("hours", 1),
            DemoTimeframe::H4 => ("hours", 4),
            DemoTimeframe::D1 => ("days", 1),
            DemoTimeframe::W1 => ("weeks", 1),
        }
    }
}

/// Seeded random number generator (simple LCG)
struct SeededRng {
    seed: u64,
}

impl SeededRng {
    fn new(seed: u64) -> Self {
        Self { seed }
    }

    /// Generate next random f64 in [0, 1)
    fn next_f64(&mut self) -> f64 {
        // Linear Congruential Generator
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        // Convert to f64 in [0, 1)
        (self.seed as f64) / (u64::MAX as f64)
    }

    /// Generate random f64 in [-1, 1)
    fn next_f64_signed(&mut self) -> f64 {
        self.next_f64() * 2.0 - 1.0
    }
}

/// Demo data generator
pub struct DemoDataGenerator {
    /// Cache of generated data per symbol per timeframe
    cache: HashMap<(String, DemoTimeframe), Vec<Bar>>,
}

impl DemoDataGenerator {
    /// Create new generator
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Generate OHLC data for a symbol at specified timeframe
    ///
    /// # Arguments
    /// * `symbol` - Demo symbol configuration
    /// * `timeframe` - Target timeframe
    /// * `count` - Number of bars to generate
    /// * `end_timestamp` - Timestamp of the last bar (optional, defaults to now)
    pub fn generate(
        &mut self,
        symbol: &DemoSymbol,
        timeframe: DemoTimeframe,
        count: usize,
        end_timestamp: Option<i64>,
    ) -> Vec<Bar> {
        // Check cache first
        let cache_key = (symbol.ticker().to_string(), timeframe);
        if let Some(cached) = self.cache.get(&cache_key) {
            if cached.len() >= count {
                return cached[cached.len() - count..].to_vec();
            }
        }

        // Generate base M1 data first
        let m1_bars = self.generate_m1_data(symbol, count * timeframe.seconds() as usize / 60 + 100, end_timestamp);

        // Aggregate to target timeframe
        let bars = if timeframe == DemoTimeframe::M1 {
            m1_bars
        } else {
            self.aggregate_bars(&m1_bars, timeframe)
        };

        // Trim to requested count
        let result: Vec<Bar> = if bars.len() > count {
            bars[bars.len() - count..].to_vec()
        } else {
            bars
        };

        // Cache result
        self.cache.insert(cache_key, result.clone());

        result
    }

    /// Generate raw 1-minute data for the demo period (92 days ending now)
    fn generate_m1_data(&self, symbol: &DemoSymbol, count: usize, end_timestamp: Option<i64>) -> Vec<Bar> {
        // Use demo period timestamps (defaults to current time)
        let (_, demo_end) = demo_period();
        let end_ts = end_timestamp.unwrap_or(demo_end);

        // Align to minute boundary
        let end_ts = (end_ts / 60) * 60;
        let start_ts = end_ts - (count as i64) * 60;

        // Initialize RNG with symbol-specific seed
        let mut rng = SeededRng::new(symbol.seed_offset);

        let mut bars = Vec::with_capacity(count);
        let mut price = symbol.base_price;

        // Pre-warm RNG to get to consistent state
        for _ in 0..1000 {
            rng.next_f64();
        }

        // Base volume for the symbol (higher priced assets typically have lower volume numbers)
        let base_volume = 1_000_000.0 / (symbol.base_price / 100.0).max(1.0);

        for i in 0..count {
            let timestamp = start_ts + (i as i64) * 60;

            // Calculate price movement
            // Base volatility (percentage of price)
            let base_volatility = symbol.base_price * 0.001 * symbol.volatility;

            // Trend component (slowly varying)
            let trend = symbol.trend_bias * (i as f64 / 500.0).sin() * base_volatility;

            // Random component
            let random = rng.next_f64_signed() * base_volatility;

            // Mean reversion (pull back to base price over time)
            let reversion = (symbol.base_price - price) * 0.001;

            // Total price change
            let change = trend + random + reversion;

            // Generate OHLC
            let open = price;
            let close = (price + change).max(symbol.base_price * 0.5).min(symbol.base_price * 2.0);

            // High/Low based on volatility
            let wick = rng.next_f64() * base_volatility * 0.5;
            let high = open.max(close) + wick;
            let low = open.min(close) - wick * rng.next_f64();

            // Generate volume (correlated with price movement - bigger moves = more volume)
            let price_move_ratio = (close - open).abs() / base_volatility;
            let volume_multiplier = 0.5 + rng.next_f64() * 1.5 + price_move_ratio * 0.5;
            let volume = base_volume * volume_multiplier;

            bars.push(Bar::with_volume(timestamp, open, high, low, close, volume));

            // Update price for next bar
            price = close;
        }

        bars
    }

    /// Aggregate M1 bars to higher timeframe
    fn aggregate_bars(&self, m1_bars: &[Bar], timeframe: DemoTimeframe) -> Vec<Bar> {
        if m1_bars.is_empty() {
            return Vec::new();
        }

        let tf_seconds = timeframe.seconds();
        let mut result = Vec::new();
        let mut current_group: Vec<&Bar> = Vec::new();
        let mut current_period = m1_bars[0].timestamp / tf_seconds;

        for bar in m1_bars {
            let bar_period = bar.timestamp / tf_seconds;

            if bar_period != current_period && !current_group.is_empty() {
                // Aggregate current group
                result.push(self.aggregate_group(&current_group, current_period * tf_seconds));
                current_group.clear();
            }

            current_group.push(bar);
            current_period = bar_period;
        }

        // Don't forget last group
        if !current_group.is_empty() {
            result.push(self.aggregate_group(&current_group, current_period * tf_seconds));
        }

        result
    }

    /// Aggregate a group of bars into single OHLCV bar
    fn aggregate_group(&self, bars: &[&Bar], timestamp: i64) -> Bar {
        let open = bars[0].open;
        let close = bars.last().unwrap().close;
        let high = bars.iter().map(|b| b.high).fold(f64::NEG_INFINITY, f64::max);
        let low = bars.iter().map(|b| b.low).fold(f64::INFINITY, f64::min);
        let volume = bars.iter().map(|b| b.volume).sum();

        Bar::with_volume(timestamp, open, high, low, close, volume)
    }

    /// Clear cache (useful when regenerating data)
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Get cached data for symbol/timeframe if available
    pub fn get_cached(&self, symbol: &str, timeframe: DemoTimeframe) -> Option<&Vec<Bar>> {
        self.cache.get(&(symbol.to_string(), timeframe))
    }
}

impl Default for DemoDataGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to generate demo data for a symbol.
/// Generates full demo period (Oct 1, 2025 - Jan 1, 2026) for the given timeframe.
pub fn generate_demo_bars(
    symbol: &DemoSymbol,
    timeframe: DemoTimeframe,
) -> Vec<Bar> {
    let mut generator = DemoDataGenerator::new();
    let count = timeframe.demo_bar_count();
    generator.generate(symbol, timeframe, count, None)
}

/// Generate data for all timeframes for a symbol.
/// Each timeframe gets full demo period (Oct 1, 2025 - Jan 1, 2026).
pub fn generate_all_timeframes(
    symbol: &DemoSymbol,
) -> HashMap<DemoTimeframe, Vec<Bar>> {
    let mut generator = DemoDataGenerator::new();
    let mut result = HashMap::new();

    for &tf in DemoTimeframe::common() {
        let count = tf.demo_bar_count();
        result.insert(tf, generator.generate(symbol, tf, count, None));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::get_demo_symbol;

    #[test]
    fn test_generate_demo_data() {
        let symbol = get_demo_symbol("BTCUSD").unwrap();
        let bars = generate_demo_bars(&symbol, DemoTimeframe::H1);

        // Should have full demo period of H1 bars (92 days * 24 hours = 2208 bars)
        let expected_count = DemoTimeframe::H1.demo_bar_count();
        assert_eq!(bars.len(), expected_count);

        // Check bars are ordered by timestamp
        for i in 1..bars.len() {
            assert!(bars[i].timestamp > bars[i-1].timestamp);
        }

        // Check OHLC validity
        for bar in &bars {
            assert!(bar.high >= bar.open);
            assert!(bar.high >= bar.close);
            assert!(bar.low <= bar.open);
            assert!(bar.low <= bar.close);
        }
    }

    #[test]
    fn test_consistent_generation() {
        let symbol = get_demo_symbol("AAPL").unwrap();

        // Generate twice - should be identical
        let bars1 = generate_demo_bars(&symbol, DemoTimeframe::H1);
        let bars2 = generate_demo_bars(&symbol, DemoTimeframe::H1);

        assert_eq!(bars1.len(), bars2.len());
        for (b1, b2) in bars1.iter().zip(bars2.iter()) {
            assert_eq!(b1.timestamp, b2.timestamp);
            assert!((b1.open - b2.open).abs() < 0.0001);
            assert!((b1.close - b2.close).abs() < 0.0001);
        }
    }

    #[test]
    fn test_different_symbols_different_data() {
        let aapl = get_demo_symbol("AAPL").unwrap();
        let btc = get_demo_symbol("BTCUSD").unwrap();

        let bars_aapl = generate_demo_bars(&aapl, DemoTimeframe::H1);
        let bars_btc = generate_demo_bars(&btc, DemoTimeframe::H1);

        // Prices should be very different
        assert!((bars_aapl[0].close - bars_btc[0].close).abs() > 1000.0);
    }

    #[test]
    fn test_timeframe_aggregation() {
        let symbol = get_demo_symbol("EURUSD").unwrap();

        let h1_bars = generate_demo_bars(&symbol, DemoTimeframe::H1);
        let d1_bars = generate_demo_bars(&symbol, DemoTimeframe::D1);

        // D1 should have fewer bars than H1
        assert!(d1_bars.len() < h1_bars.len());

        // D1 bar should span correct time (aligned to day)
        assert_eq!(d1_bars[0].timestamp % 86400, 0);
    }

    #[test]
    fn test_all_timeframes() {
        let symbol = get_demo_symbol("SPX").unwrap();
        let all_data = generate_all_timeframes(&symbol);

        // Should have data for all common timeframes
        assert!(all_data.contains_key(&DemoTimeframe::M15));
        assert!(all_data.contains_key(&DemoTimeframe::H1));
        assert!(all_data.contains_key(&DemoTimeframe::H4));
        assert!(all_data.contains_key(&DemoTimeframe::D1));

        // Each timeframe should have correct bar count
        for (&tf, bars) in &all_data {
            assert_eq!(bars.len(), tf.demo_bar_count());
        }
    }

    #[test]
    fn test_demo_bar_count() {
        // 92 days = DEMO_DURATION_SECONDS / seconds_per_unit
        assert_eq!(DemoTimeframe::M1.demo_bar_count(), 92 * 24 * 60);  // 132480
        assert_eq!(DemoTimeframe::H1.demo_bar_count(), 92 * 24);       // 2208
        assert_eq!(DemoTimeframe::D1.demo_bar_count(), 92);            // 92
    }

    #[test]
    fn test_demo_period_timestamps() {
        let symbol = get_demo_symbol("BTCUSD").unwrap();
        let bars = generate_demo_bars(&symbol, DemoTimeframe::D1);

        let (demo_start, demo_end) = demo_period();

        // First bar should be at or after demo start
        assert!(bars[0].timestamp >= demo_start);

        // Last bar should be before demo end
        assert!(bars.last().unwrap().timestamp < demo_end);

        // Bars should span approximately 92 days
        let span_days = (bars.last().unwrap().timestamp - bars[0].timestamp) / 86400;
        assert!(span_days >= 90 && span_days <= 92);
    }
}
