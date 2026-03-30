//! Signal Engine - runtime for processing indicator values through configured detectors
//!
//! The SignalEngine takes a SignalProfile and processes IndicatorValue through
//! the configured detectors to generate Signals.

use crate::bar_indicators::bar_indicator_id::BarIndicatorId;
use crate::bar_indicators::indicator_value::IndicatorValue;
use crate::signals::{
    ChannelDetector, CrossoverDetector, HistogramDetector, SignalKind,
    ThresholdMonitor, ZeroCrossDetector,
};
use crate::signals::signal::{Signal, Direction, BarConfirmation, SignalSource};

use super::config::{DetectorConfig, DetectorParams, DetectorType, ValueSource};
use super::profile::SignalProfile;

/// A detector instance with its configuration
enum DetectorInstance {
    Threshold {
        config_id: String,
        monitor: ThresholdMonitor,
        value_source: ValueSource,
    },
    ZeroCross {
        config_id: String,
        detector: ZeroCrossDetector,
        value_source: ValueSource,
    },
    Crossover {
        config_id: String,
        detector: CrossoverDetector,
        line_a: ValueSource,
        line_b: ValueSource,
    },
    Histogram {
        config_id: String,
        detector: HistogramDetector,
        value_source: ValueSource,
    },
    Channel {
        config_id: String,
        detector: ChannelDetector,
        upper_source: ValueSource,
        lower_source: ValueSource,
    },
}

impl DetectorInstance {
    /// Create from config
    fn from_config(config: &DetectorConfig) -> Option<Self> {
        match (&config.detector_type, &config.params) {
            (
                DetectorType::Threshold,
                DetectorParams::Threshold {
                    value_source,
                    upper,
                    lower,
                },
            ) => Some(DetectorInstance::Threshold {
                config_id: config.id.clone(),
                monitor: ThresholdMonitor::new(*upper, *lower),
                value_source: value_source.clone(),
            }),

            (
                DetectorType::ZeroCross,
                DetectorParams::ZeroCross {
                    value_source,
                    tolerance,
                },
            ) => Some(DetectorInstance::ZeroCross {
                config_id: config.id.clone(),
                detector: ZeroCrossDetector::with_tolerance(*tolerance),
                value_source: value_source.clone(),
            }),

            (DetectorType::Crossover, DetectorParams::Crossover { line_a, line_b }) => {
                Some(DetectorInstance::Crossover {
                    config_id: config.id.clone(),
                    detector: CrossoverDetector::new(),
                    line_a: line_a.clone(),
                    line_b: line_b.clone(),
                })
            }

            (DetectorType::Histogram, DetectorParams::Histogram { value_source }) => {
                Some(DetectorInstance::Histogram {
                    config_id: config.id.clone(),
                    detector: HistogramDetector::new(),
                    value_source: value_source.clone(),
                })
            }

            (
                DetectorType::Channel,
                DetectorParams::Channel {
                    upper_source,
                    lower_source,
                },
            ) => Some(DetectorInstance::Channel {
                config_id: config.id.clone(),
                detector: ChannelDetector::new(0.001),
                upper_source: upper_source.clone(),
                lower_source: lower_source.clone(),
            }),

            // Other types not yet implemented
            _ => None,
        }
    }

    /// Process a value and return signal if generated
    fn process(
        &mut self,
        value: &IndicatorValue,
        price: Option<f64>,
    ) -> Option<(String, SignalKind)> {
        match self {
            DetectorInstance::Threshold {
                config_id,
                monitor,
                value_source,
            } => {
                let v = value_source.extract(value)?;
                let signal = monitor.update(v)?;
                Some((config_id.clone(), signal))
            }

            DetectorInstance::ZeroCross {
                config_id,
                detector,
                value_source,
            } => {
                let v = value_source.extract(value)?;
                let signal = detector.update(v)?;
                Some((config_id.clone(), signal))
            }

            DetectorInstance::Crossover {
                config_id,
                detector,
                line_a,
                line_b,
            } => {
                // Handle Price source - use price parameter instead of extracting from value
                let a = if matches!(line_a, ValueSource::Price) {
                    price?
                } else {
                    line_a.extract(value)?
                };
                let b = if matches!(line_b, ValueSource::Price) {
                    price?
                } else {
                    line_b.extract(value)?
                };
                let signal = detector.update(a, b)?;
                Some((config_id.clone(), signal))
            }

            DetectorInstance::Histogram {
                config_id,
                detector,
                value_source,
            } => {
                let v = value_source.extract(value)?;
                let signal = detector.update(v)?;
                Some((config_id.clone(), signal))
            }

            DetectorInstance::Channel {
                config_id,
                detector,
                upper_source,
                lower_source,
            } => {
                let price_val = price?;
                let upper = upper_source.extract(value)?;
                let lower = lower_source.extract(value)?;
                let signal = detector.update(price_val, upper, lower)?;
                Some((config_id.clone(), signal))
            }
        }
    }

    /// Reset detector state
    fn reset(&mut self) {
        match self {
            DetectorInstance::Threshold { monitor, .. } => monitor.reset(),
            DetectorInstance::ZeroCross { detector, .. } => detector.reset(),
            DetectorInstance::Crossover { detector, .. } => detector.reset(),
            DetectorInstance::Histogram { detector, .. } => detector.reset(),
            DetectorInstance::Channel { detector, .. } => {
                *detector = ChannelDetector::new(0.001);
            }
        }
    }
}

/// Signal Engine - processes indicator values through configured detectors
pub struct SignalEngine {
    /// Indicator ID this engine is for
    indicator_id: BarIndicatorId,

    /// Profile name
    profile_name: String,

    /// Active detector instances
    detectors: Vec<DetectorInstance>,

    /// Bar index counter
    bar_index: usize,

    /// Counter for signals generated
    signal_counter: u64,
}

impl SignalEngine {
    /// Create a new engine from a signal profile
    pub fn from_profile(profile: &SignalProfile) -> Self {
        let detectors = profile
            .enabled_detectors()
            .filter_map(DetectorInstance::from_config)
            .collect();

        Self {
            indicator_id: profile.indicator_id,
            profile_name: profile.name.clone(),
            detectors,
            bar_index: 0,
            signal_counter: 0,
        }
    }

    /// Process an indicator value and return any generated signals
    ///
    /// # Arguments
    /// * `value` - The indicator value to process
    /// * `price` - Current price (needed for channel detectors)
    /// * `timestamp` - Unix timestamp (ms) of the bar
    /// * `indicator_name` - Name of the indicator emitting this signal
    /// * `is_last_bar` - Whether this is the last (potentially unclosed) bar
    pub fn process(
        &mut self,
        value: &IndicatorValue,
        price: f64,
        timestamp: i64,
        indicator_name: &str,
        is_last_bar: bool,
    ) -> Vec<Signal> {
        let mut signals = Vec::new();

        for detector in &mut self.detectors {
            if let Some((_detector_id, signal_kind)) = detector.process(value, Some(price)) {
                self.signal_counter += 1;

                signals.push(Signal::new(
                    self.signal_counter,
                    self.bar_index,
                    timestamp,
                    price,
                    signal_kind,
                    Direction::from_i8(signal_kind.direction()),
                    if is_last_bar { BarConfirmation::Pending } else { BarConfirmation::Closed },
                    SignalSource::Indicator(indicator_name.to_string()),
                ));
            }
        }

        self.bar_index += 1;
        signals
    }

    /// Process with just indicator value (no price, channel detectors won't work)
    ///
    /// # Arguments
    /// * `value` - The indicator value to process
    /// * `timestamp` - Unix timestamp (ms) of the bar
    /// * `indicator_name` - Name of the indicator emitting this signal
    /// * `is_last_bar` - Whether this is the last (potentially unclosed) bar
    pub fn process_simple(
        &mut self,
        value: &IndicatorValue,
        timestamp: i64,
        indicator_name: &str,
        is_last_bar: bool,
    ) -> Vec<Signal> {
        let price = value.main();
        self.process(value, price, timestamp, indicator_name, is_last_bar)
    }

    /// Reset all detector states
    pub fn reset(&mut self) {
        for detector in &mut self.detectors {
            detector.reset();
        }
        self.bar_index = 0;
        self.signal_counter = 0;
    }

    /// Get the indicator ID this engine is for
    pub fn indicator_id(&self) -> BarIndicatorId {
        self.indicator_id
    }

    /// Get the profile name
    pub fn profile_name(&self) -> &str {
        &self.profile_name
    }

    /// Get count of active detectors
    pub fn detector_count(&self) -> usize {
        self.detectors.len()
    }

    /// Get total signals generated
    pub fn signals_generated(&self) -> u64 {
        self.signal_counter
    }

    /// Get current bar index
    pub fn bar_index(&self) -> usize {
        self.bar_index
    }
}

impl std::fmt::Debug for SignalEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalEngine")
            .field("indicator_id", &self.indicator_id)
            .field("profile_name", &self.profile_name)
            .field("detector_count", &self.detectors.len())
            .field("bar_index", &self.bar_index)
            .field("signals_generated", &self.signal_counter)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::rules::defaults;

    #[test]
    fn test_engine_from_rsi_profile() {
        let profile = defaults::rsi_profile();
        let engine = SignalEngine::from_profile(&profile);

        assert_eq!(engine.indicator_id(), BarIndicatorId::Rsi);
        assert!(engine.detector_count() > 0);
    }

    #[test]
    fn test_engine_process_rsi_overbought() {
        let profile = defaults::rsi_profile();
        let mut engine = SignalEngine::from_profile(&profile);

        // Feed values that go into overbought territory
        let values = [50.0, 60.0, 70.0, 75.0, 80.0];

        let mut all_signals = Vec::new();
        for &v in &values {
            let signals = engine.process_simple(&IndicatorValue::Single(v), 0, "Test", false);
            all_signals.extend(signals);
        }

        // Should have generated at least one overbought signal
        let has_overbought = all_signals
            .iter()
            .any(|s| matches!(s.kind, SignalKind::OscillatorOverbought));
        assert!(has_overbought, "Expected overbought signal");
    }

    #[test]
    fn test_engine_process_macd_crossover() {
        let profile = defaults::macd_profile();
        let mut engine = SignalEngine::from_profile(&profile);

        // Simulate MACD line crossing above signal line
        let values = [
            IndicatorValue::Macd {
                line: -0.5,
                signal: 0.0,
                histogram: -0.5,
            },
            IndicatorValue::Macd {
                line: -0.2,
                signal: 0.0,
                histogram: -0.2,
            },
            IndicatorValue::Macd {
                line: 0.3,
                signal: 0.0,
                histogram: 0.3,
            },
        ];

        let mut all_signals = Vec::new();
        for v in &values {
            let signals = engine.process_simple(v, 0, "Test", false);
            all_signals.extend(signals);
        }

        // Should have crossover up signal
        let has_crossup = all_signals
            .iter()
            .any(|s| matches!(s.kind, SignalKind::CrossoverUp));
        assert!(has_crossup, "Expected crossover up signal");
    }

    #[test]
    fn test_engine_process_stochastic() {
        let profile = defaults::stochastic_profile();
        let mut engine = SignalEngine::from_profile(&profile);

        // Simulate %K crossing above %D using Double
        let values = [
            IndicatorValue::Double(20.0, 30.0), // K=20, D=30
            IndicatorValue::Double(28.0, 29.0), // K=28, D=29
            IndicatorValue::Double(35.0, 30.0), // K=35, D=30 - crossover!
        ];

        let mut all_signals = Vec::new();
        for v in &values {
            let signals = engine.process_simple(v, 0, "Test", false);
            all_signals.extend(signals);
        }

        // Should have crossover signal
        let has_cross = all_signals
            .iter()
            .any(|s| matches!(s.kind, SignalKind::CrossoverUp | SignalKind::CrossoverDown));
        assert!(has_cross, "Expected K/D crossover signal");
    }

    #[test]
    fn test_engine_reset() {
        let profile = defaults::rsi_profile();
        let mut engine = SignalEngine::from_profile(&profile);

        // Process some values
        engine.process_simple(&IndicatorValue::Single(75.0), 0, "Test", false);
        engine.process_simple(&IndicatorValue::Single(80.0), 0, "Test", false);

        assert!(engine.bar_index() > 0);

        // Reset
        engine.reset();
        assert_eq!(engine.signals_generated(), 0);
        assert_eq!(engine.bar_index(), 0);
    }

    #[test]
    fn test_engine_with_disabled_detectors() {
        let mut profile = defaults::rsi_profile();
        profile.disable_all();
        profile.set_detector_enabled("ob_os", true);

        let engine = SignalEngine::from_profile(&profile);
        assert_eq!(engine.detector_count(), 1);
    }

    #[test]
    fn test_engine_histogram_detector() {
        let profile = defaults::macd_profile();
        let mut engine = SignalEngine::from_profile(&profile);

        // Simulate histogram going from negative to positive
        let values = [
            IndicatorValue::Macd {
                line: -0.5,
                signal: -0.3,
                histogram: -0.2,
            },
            IndicatorValue::Macd {
                line: -0.1,
                signal: -0.2,
                histogram: 0.1,
            },
            IndicatorValue::Macd {
                line: 0.2,
                signal: 0.0,
                histogram: 0.2,
            },
        ];

        let mut all_signals = Vec::new();
        for v in &values {
            let signals = engine.process_simple(v, 0, "Test", false);
            all_signals.extend(signals);
        }

        // Should have histogram positive signal
        let has_hist_pos = all_signals
            .iter()
            .any(|s| matches!(s.kind, SignalKind::HistogramPositive));
        assert!(has_hist_pos, "Expected histogram positive signal");
    }
}
