//! Demo Indicator Calculations
//!
//! Simplified calculations for testing UI/rendering.
//! Real indicators will be connected from nemo library later.
//!
//! # Note
//!
//! These are demo stubs - not production indicator implementations.
//! When connecting to real APIs:
//! 1. Create indicator trait in nemo library
//! 2. Implement real indicators with proper algorithms
//! 3. Replace these demo functions with trait calls

use crate::Bar;

/// Calculate Simple Moving Average (demo)
pub fn calculate_sma(bars: &[Bar], period: usize) -> Vec<f64> {
    if bars.is_empty() || period == 0 {
        return vec![];
    }

    let mut result = Vec::with_capacity(bars.len());

    for i in 0..bars.len() {
        if i < period - 1 {
            result.push(f64::NAN);
        } else {
            let sum: f64 = bars[i + 1 - period..=i]
                .iter()
                .map(|b| b.close)
                .sum();
            result.push(sum / period as f64);
        }
    }

    result
}

/// Calculate Exponential Moving Average (demo)
pub fn calculate_ema(bars: &[Bar], period: usize) -> Vec<f64> {
    if bars.is_empty() || period == 0 {
        return vec![];
    }

    let mut result = Vec::with_capacity(bars.len());
    let multiplier = 2.0 / (period as f64 + 1.0);

    // First value is SMA
    let mut ema = if bars.len() >= period {
        bars[..period].iter().map(|b| b.close).sum::<f64>() / period as f64
    } else {
        bars[0].close
    };

    for (i, bar) in bars.iter().enumerate() {
        if i < period - 1 {
            result.push(f64::NAN);
        } else if i == period - 1 {
            result.push(ema);
        } else {
            ema = (bar.close - ema) * multiplier + ema;
            result.push(ema);
        }
    }

    result
}

/// Calculate RSI (demo) - simplified
pub fn calculate_rsi(bars: &[Bar], period: usize) -> Vec<f64> {
    if bars.len() < 2 || period == 0 {
        return vec![f64::NAN; bars.len()];
    }

    let mut result = vec![f64::NAN; bars.len()];
    let mut gains = Vec::with_capacity(bars.len() - 1);
    let mut losses = Vec::with_capacity(bars.len() - 1);

    // Calculate price changes
    for i in 1..bars.len() {
        let change = bars[i].close - bars[i - 1].close;
        if change > 0.0 {
            gains.push(change);
            losses.push(0.0);
        } else {
            gains.push(0.0);
            losses.push(-change);
        }
    }

    if gains.len() < period {
        return result;
    }

    // First RSI value
    let mut avg_gain: f64 = gains[..period].iter().sum::<f64>() / period as f64;
    let mut avg_loss: f64 = losses[..period].iter().sum::<f64>() / period as f64;

    for i in period..bars.len() {
        if avg_loss == 0.0 {
            result[i] = 100.0;
        } else {
            let rs = avg_gain / avg_loss;
            result[i] = 100.0 - (100.0 / (1.0 + rs));
        }

        if i < gains.len() {
            avg_gain = (avg_gain * (period as f64 - 1.0) + gains[i]) / period as f64;
            avg_loss = (avg_loss * (period as f64 - 1.0) + losses[i]) / period as f64;
        }
    }

    result
}

/// Calculate Bollinger Bands (demo)
/// Returns (middle, upper, lower)
pub fn calculate_bollinger(bars: &[Bar], period: usize, mult: f64) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let sma = calculate_sma(bars, period);
    let mut upper = Vec::with_capacity(bars.len());
    let mut lower = Vec::with_capacity(bars.len());

    for i in 0..bars.len() {
        if i < period - 1 {
            upper.push(f64::NAN);
            lower.push(f64::NAN);
        } else {
            // Calculate standard deviation
            let mean = sma[i];
            let variance: f64 = bars[i + 1 - period..=i]
                .iter()
                .map(|b| (b.close - mean).powi(2))
                .sum::<f64>() / period as f64;
            let std_dev = variance.sqrt();

            upper.push(mean + mult * std_dev);
            lower.push(mean - mult * std_dev);
        }
    }

    (sma, upper, lower)
}

/// Calculate Volume histogram values (returns actual volumes from bars)
pub fn calculate_volume(bars: &[Bar]) -> Vec<f64> {
    bars.iter().map(|b| b.volume).collect()
}

/// Calculate MACD (demo)
/// Returns (macd_line, signal_line, histogram)
pub fn calculate_macd(bars: &[Bar], fast: usize, slow: usize, signal: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    if bars.is_empty() {
        return (vec![], vec![], vec![]);
    }

    let fast_ema = calculate_ema(bars, fast);
    let slow_ema = calculate_ema(bars, slow);

    // MACD line = fast EMA - slow EMA
    let mut macd_line = Vec::with_capacity(bars.len());
    for i in 0..bars.len() {
        if fast_ema[i].is_nan() || slow_ema[i].is_nan() {
            macd_line.push(f64::NAN);
        } else {
            macd_line.push(fast_ema[i] - slow_ema[i]);
        }
    }

    // Signal line = EMA of MACD line
    let mut signal_line = vec![f64::NAN; bars.len()];
    let multiplier = 2.0 / (signal as f64 + 1.0);

    // Find first valid MACD value
    let first_valid = macd_line.iter().position(|v| !v.is_nan());
    if let Some(start) = first_valid {
        if start + signal <= bars.len() {
            // First signal value is SMA of MACD
            let mut sig_ema: f64 = macd_line[start..start + signal]
                .iter()
                .filter(|v| !v.is_nan())
                .sum::<f64>() / signal as f64;

            for i in (start + signal - 1)..bars.len() {
                if i == start + signal - 1 {
                    signal_line[i] = sig_ema;
                } else if !macd_line[i].is_nan() {
                    sig_ema = (macd_line[i] - sig_ema) * multiplier + sig_ema;
                    signal_line[i] = sig_ema;
                }
            }
        }
    }

    // Histogram = MACD - Signal
    let mut histogram = Vec::with_capacity(bars.len());
    for i in 0..bars.len() {
        if macd_line[i].is_nan() || signal_line[i].is_nan() {
            histogram.push(f64::NAN);
        } else {
            histogram.push(macd_line[i] - signal_line[i]);
        }
    }

    (macd_line, signal_line, histogram)
}

/// Calculate ATR - Average True Range (demo)
pub fn calculate_atr(bars: &[Bar], period: usize) -> Vec<f64> {
    if bars.len() < 2 || period == 0 {
        return vec![f64::NAN; bars.len()];
    }

    let mut result = vec![f64::NAN; bars.len()];
    let mut tr_values = Vec::with_capacity(bars.len());

    // First TR is just high - low
    tr_values.push(bars[0].high - bars[0].low);

    // Calculate True Range for each bar
    for i in 1..bars.len() {
        let high_low = bars[i].high - bars[i].low;
        let high_prev_close = (bars[i].high - bars[i - 1].close).abs();
        let low_prev_close = (bars[i].low - bars[i - 1].close).abs();
        tr_values.push(high_low.max(high_prev_close).max(low_prev_close));
    }

    // First ATR is simple average
    if bars.len() >= period {
        let first_atr: f64 = tr_values[..period].iter().sum::<f64>() / period as f64;
        result[period - 1] = first_atr;

        // Subsequent ATR uses smoothing
        let mut atr = first_atr;
        for i in period..bars.len() {
            atr = (atr * (period as f64 - 1.0) + tr_values[i]) / period as f64;
            result[i] = atr;
        }
    }

    result
}

/// Calculate Stochastic Oscillator (demo)
/// Returns (%K, %D)
pub fn calculate_stochastic(bars: &[Bar], k_period: usize, d_period: usize, smooth: usize) -> (Vec<f64>, Vec<f64>) {
    if bars.is_empty() || k_period == 0 {
        return (vec![f64::NAN; bars.len()], vec![f64::NAN; bars.len()]);
    }

    let mut raw_k = vec![f64::NAN; bars.len()];

    // Calculate raw %K
    for i in (k_period - 1)..bars.len() {
        let mut highest_high = f64::NEG_INFINITY;
        let mut lowest_low = f64::INFINITY;

        for bar in &bars[(i + 1 - k_period)..=i] {
            highest_high = highest_high.max(bar.high);
            lowest_low = lowest_low.min(bar.low);
        }

        let range = highest_high - lowest_low;
        if range > 0.0 {
            raw_k[i] = ((bars[i].close - lowest_low) / range) * 100.0;
        } else {
            raw_k[i] = 50.0; // Default to middle if no range
        }
    }

    // Smooth %K (if smooth > 1)
    let k_line = if smooth > 1 {
        smooth_values(&raw_k, smooth)
    } else {
        raw_k
    };

    // %D is SMA of %K
    let d_line = smooth_values(&k_line, d_period);

    (k_line, d_line)
}

/// Helper: smooth values with simple moving average
fn smooth_values(values: &[f64], period: usize) -> Vec<f64> {
    if period <= 1 {
        return values.to_vec();
    }

    let mut result = vec![f64::NAN; values.len()];

    for (i, slot) in result.iter_mut().enumerate() {
        if i < period - 1 {
            continue;
        }

        let mut sum = 0.0;
        let mut count = 0;
        for &v in &values[(i + 1 - period)..=i] {
            if !v.is_nan() {
                sum += v;
                count += 1;
            }
        }

        if count == period {
            *slot = sum / period as f64;
        }
    }

    result
}

// =============================================================================
// Demo Signal and Trade Generation - MOVED TO TERMINAL
// =============================================================================
// Note: generate_demo_signals() and generate_demo_trades() were here but require
// SignalManager and TradeManager which are terminal-specific features.
// These functions have been moved to the terminal crate.

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bars(closes: &[f64]) -> Vec<Bar> {
        closes.iter().enumerate().map(|(i, &c)| {
            Bar::new(i as i64 * 60, c, c + 1.0, c - 1.0, c)
        }).collect()
    }

    #[test]
    fn test_sma() {
        let bars = make_bars(&[10.0, 11.0, 12.0, 13.0, 14.0]);
        let sma = calculate_sma(&bars, 3);

        assert!(sma[0].is_nan());
        assert!(sma[1].is_nan());
        assert!((sma[2] - 11.0).abs() < 0.001); // (10+11+12)/3
        assert!((sma[3] - 12.0).abs() < 0.001); // (11+12+13)/3
        assert!((sma[4] - 13.0).abs() < 0.001); // (12+13+14)/3
    }

    #[test]
    fn test_ema() {
        let bars = make_bars(&[10.0, 11.0, 12.0, 13.0, 14.0]);
        let ema = calculate_ema(&bars, 3);

        assert!(ema[0].is_nan());
        assert!(ema[1].is_nan());
        assert!((ema[2] - 11.0).abs() < 0.001); // First EMA = SMA
    }

    #[test]
    fn test_rsi() {
        let bars = make_bars(&[44.0, 44.5, 43.5, 44.5, 44.0, 43.5, 44.0, 44.5, 45.0, 45.5, 46.0, 45.5, 46.0, 46.5, 46.0, 47.0]);
        let rsi = calculate_rsi(&bars, 14);

        // RSI should be between 0 and 100
        for &v in &rsi {
            if !v.is_nan() {
                assert!(v >= 0.0 && v <= 100.0);
            }
        }
    }
}
