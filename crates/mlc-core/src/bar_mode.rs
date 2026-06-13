//! [`BarMode`] — controls the bar aggregation mode for a chart bubble.
//!
//! A chart bubble can subscribe to one of several derived-bar streams produced
//! by dig3's station layer. `BarMode` selects which stream and carries the
//! stream's configuration parameter (range, tick count, or volume threshold).
//!
//! ## Default parameters
//!
//! | Variant        | Default param      | Unit                        |
//! |----------------|--------------------|-----------------------------|
//! | `RangeBar`     | `range = 10.0`     | price in quote currency     |
//! | `TickBar`      | `count = 50`       | trade count per bar         |
//! | `VolumeBar`    | `volume = 1.0`     | base-asset volume per bar   |
//! | `Footprint`    | uses bubble TF     | bucketed by bubble timeframe |

use serde::{Deserialize, Serialize};

/// Bar aggregation mode for a chart data bubble.
///
/// # Default
///
/// `BarMode::TimeBased` — uses the bubble's `timeframe` field to subscribe to
/// a kline stream.  This is byte-for-byte identical to the previous behavior;
/// existing presets deserialize as `TimeBased` via `#[serde(default)]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BarMode {
    /// Standard time-based kline bars.  Uses the bubble's `timeframe` field.
    /// This is the default and maintains full backward compatibility.
    TimeBased,

    /// Range bar: a new bar is opened when the price moves more than `range`
    /// points from the bar's open price.
    ///
    /// Default `range = 10.0`.  Caller should override with an exchange-appropriate
    /// tick size (typically a small multiple of the instrument's minimum price move).
    RangeBar {
        /// Price range threshold in quote-currency units (not fixed-point).
        /// Sent to dig3 as `(range * 1e8) as u64`.
        range: f64,
    },

    /// Tick bar: a new bar is opened every `count` individual trades.
    ///
    /// Default `count = 50`.
    TickBar {
        /// Number of trades per bar.
        count: u32,
    },

    /// Volume bar: a new bar is opened when cumulative base-asset volume
    /// crosses the `volume` threshold.
    ///
    /// Default `volume = 1.0`.  Caller should set a meaningful value for the
    /// instrument (e.g. 1.0 BTC for BTCUSDT, 1000.0 for smaller-cap assets).
    VolumeBar {
        /// Volume threshold in base-asset units (not fixed-point).
        /// Sent to dig3 as `(volume * 1e8) as u64`.
        volume: f64,
    },

    /// Footprint bar: time-bucketed OHLCV with per-price-level buy/sell
    /// breakdown. The time bucket size is the bubble's `timeframe` field.
    Footprint,
}

impl Default for BarMode {
    fn default() -> Self {
        BarMode::TimeBased
    }
}

impl BarMode {
    /// Default parameter values for each derived-bar mode.
    ///
    /// Returns a `BarMode` with the canonical defaults for its variant:
    /// - `RangeBar` → `range = 10.0`
    /// - `TickBar`  → `count = 50`
    /// - `VolumeBar` → `volume = 1.0`
    /// - Others     → self (already param-less)
    pub fn with_defaults(variant: &str) -> Self {
        match variant {
            "range_bar"   => BarMode::RangeBar  { range: 10.0 },
            "tick_bar"    => BarMode::TickBar    { count: 50 },
            "volume_bar"  => BarMode::VolumeBar  { volume: 1.0 },
            "footprint"   => BarMode::Footprint,
            _             => BarMode::TimeBased,
        }
    }

    /// Return `true` if this is `TimeBased` (uses kline stream, same as old behavior).
    pub fn is_time_based(&self) -> bool {
        matches!(self, BarMode::TimeBased)
    }

    /// Short label for display (toolbar text fallback).
    pub fn short_label(&self) -> &'static str {
        match self {
            BarMode::TimeBased         => "",
            BarMode::RangeBar { .. }   => "Range",
            BarMode::TickBar  { .. }   => "Tick",
            BarMode::VolumeBar{ .. }   => "Vol",
            BarMode::Footprint         => "FP",
        }
    }

    /// The numeric parameter as an `f64` for text-input display.
    ///
    /// - `RangeBar`  → range value
    /// - `TickBar`   → count as f64
    /// - `VolumeBar` → volume value
    /// - Others      → `None`
    pub fn param_f64(&self) -> Option<f64> {
        match self {
            BarMode::RangeBar  { range  } => Some(*range),
            BarMode::TickBar   { count  } => Some(*count as f64),
            BarMode::VolumeBar { volume } => Some(*volume),
            _                             => None,
        }
    }

    /// Return a new `BarMode` with the numeric parameter updated.
    ///
    /// The variant is preserved; only the inner value is replaced.
    /// No-op for `TimeBased` and `Footprint`.
    pub fn with_param(self, param: f64) -> Self {
        match self {
            BarMode::RangeBar  { .. } => BarMode::RangeBar  { range:  param },
            BarMode::TickBar   { .. } => BarMode::TickBar   { count:  param.max(1.0) as u32 },
            BarMode::VolumeBar { .. } => BarMode::VolumeBar { volume: param },
            other                     => other,
        }
    }
}
