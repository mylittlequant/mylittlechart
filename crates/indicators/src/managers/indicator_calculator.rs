//! Indicator Calculator
//!
//! Bridges indicator instances with the computational engine.
//! Uses BarIndicatorId directly for fast access (no string parsing!).

use std::collections::HashMap;
use crate::{
    BarIndicatorId, IndicatorConfig, IndicatorInstance as ComputeInstance,
    IndicatorValue as ComputeValue,
};
use crate::bar_indicators::ohlcv_field::OhlcvField;
use crate::catalog::{ValueAdapter, get_rendering};
use crate::signals::{SignalEvent, rules::{SignalEngine, default_profile}};

use crate::Bar;
use super::indicator_manager::IndicatorValue;

/// Result of indicator calculation including values and signals
pub struct CalculationResult {
    /// Output values by name
    pub values: HashMap<String, Vec<f64>>,
    /// Generated signals
    pub signals: Vec<SignalEvent>,
}

/// Calculator that computes indicator values using zengeld-chart-indicators
pub struct IndicatorCalculator;

impl IndicatorCalculator {
    /// Calculate indicator values using BarIndicatorId directly (no string parsing!)
    ///
    /// This is the correct approach - uses typed enum for fast factory access.
    /// Returns a HashMap of output_name -> Vec<f64> for rendering.
    pub fn calculate_with_id(
        indicator_id: BarIndicatorId,
        params: &HashMap<String, IndicatorValue>,
        bars: &[Bar],
    ) -> Option<HashMap<String, Vec<f64>>> {
        // Extract periods from params
        let periods = Self::extract_periods(params);

        // Create config using BarIndicatorId directly
        let mut config = IndicatorConfig::new(
            indicator_id,
            format!("{:?}", indicator_id), // name for debugging
            periods,
        );

        // Add additional params (multipliers, etc.)
        Self::add_additional_params(&mut config, params);

        // Create compute instance (NO Box::new - Factory already boxes large variants!)
        let mut instance = ComputeInstance::create(&config).ok()?;

        // Get rendering metadata to know output names
        // Now uses BarIndicatorId directly - no string conversion needed!
        let rendering = get_rendering(indicator_id);

        // Calculate values for each bar
        let mut all_values: Vec<ComputeValue> = Vec::with_capacity(bars.len());

        for bar in bars {
            let value = instance.update_bar(
                bar.open,
                bar.high,
                bar.low,
                bar.close,
                bar.volume,
                Some(bar.time),
            );
            all_values.push(value);
        }

        // Convert to output format
        let mut result: HashMap<String, Vec<f64>> = HashMap::new();

        if let Some(render_meta) = rendering {
            // Use rendering metadata to extract named outputs
            for output_spec in &render_meta.outputs {
                let values: Vec<f64> = all_values
                    .iter()
                    .map(|v| ValueAdapter::extract(v, &output_spec.value_extractor).unwrap_or(f64::NAN))
                    .collect();
                result.insert(output_spec.name.clone(), values);
            }
        } else {
            // Fallback: just use main value
            let values: Vec<f64> = all_values.iter().map(|v| v.main()).collect();
            result.insert("value".to_string(), values);
        }

        Some(result)
    }

    /// Calculate indicator values AND detect signals
    ///
    /// This is the enhanced version that also generates signals using SignalEngine.
    /// Returns CalculationResult with both values and signals.
    pub fn calculate_with_signals(
        indicator_id: BarIndicatorId,
        params: &HashMap<String, IndicatorValue>,
        bars: &[Bar],
    ) -> Option<CalculationResult> {
        // Extract periods from params
        let periods = Self::extract_periods(params);

        // Create config using BarIndicatorId directly
        let mut config = IndicatorConfig::new(
            indicator_id,
            format!("{:?}", indicator_id),
            periods,
        );

        // Add additional params
        Self::add_additional_params(&mut config, params);

        // Create compute instance
        let mut instance = ComputeInstance::create(&config).ok()?;

        // Get rendering metadata
        let rendering = get_rendering(indicator_id);

        // Try to get default signal profile for this indicator
        let signal_profile = default_profile(indicator_id);
        let mut signal_engine = signal_profile.as_ref().map(|p| {
            SignalEngine::from_profile(p)
        });

        // Calculate values for each bar and collect signals
        let mut all_values: Vec<ComputeValue> = Vec::with_capacity(bars.len());
        let mut all_signals: Vec<SignalEvent> = Vec::new();

        for bar in bars {
            let value = instance.update_bar(
                bar.open,
                bar.high,
                bar.low,
                bar.close,
                bar.volume,
                Some(bar.time),
            );

            // Generate signals if engine exists
            if let Some(ref mut engine) = signal_engine {
                let signals = engine.process(&value, bar.close);
                all_signals.extend(signals);
            }

            all_values.push(value);
        }

        // Convert to output format
        let mut result: HashMap<String, Vec<f64>> = HashMap::new();

        if let Some(render_meta) = rendering {
            for output_spec in &render_meta.outputs {
                let values: Vec<f64> = all_values
                    .iter()
                    .map(|v| ValueAdapter::extract(v, &output_spec.value_extractor).unwrap_or(f64::NAN))
                    .collect();
                result.insert(output_spec.name.clone(), values);
            }
        } else {
            let values: Vec<f64> = all_values.iter().map(|v| v.main()).collect();
            result.insert("value".to_string(), values);
        }

        Some(CalculationResult {
            values: result,
            signals: all_signals,
        })
    }

    /// Extract period parameters from indicator params
    ///
    /// Looks for ALL integer parameters and uses them as periods.
    /// Common period names are prioritized, but any integer param is included.
    fn extract_periods(params: &HashMap<String, IndicatorValue>) -> Vec<usize> {
        let mut periods = Vec::new();

        // Priority order for period names (these are checked first)
        let priority_names = ["period", "length", "n", "window", "periods", "fast", "slow", "signal", "k", "d", "smooth"];

        // First, add params by priority order
        for name in priority_names {
            if let Some(value) = params.get(name) {
                if let Some(v) = value.as_int() {
                    periods.push(v.max(1) as usize);
                }
            }
        }

        // Then add any other integer params not already added (case-insensitive check)
        for (name, value) in params {
            let name_lower = name.to_lowercase();
            // Skip if already added via priority names
            if priority_names.iter().any(|p| *p == name_lower) {
                continue;
            }
            // Add any other integer value
            if let Some(v) = value.as_int() {
                periods.push(v.max(1) as usize);
            }
        }

        // Default period if none found
        if periods.is_empty() {
            periods.push(14);
        }

        periods
    }

    /// Add additional (non-period) parameters to config
    ///
    /// Adds ALL float parameters to additional_params, bool params to flags,
    /// and handles string parameters (especially 'source' for OHLCV field selection).
    /// Also handles component-level parameters like fast_ma_type, slow_source, etc.
    fn add_additional_params(
        config: &mut IndicatorConfig,
        params: &HashMap<String, IndicatorValue>,
    ) {
        for (name, value) in params {
            // Add float parameters
            if let Some(v) = value.as_float() {
                config.additional_params.insert(name.clone(), v);
            }
            // Add bool flags
            if let Some(v) = value.as_bool() {
                config.flags.insert(name.clone(), v);
            }
            // Handle string parameters
            if let Some(v) = value.as_string() {
                // Check for MA type parameters (e.g., fast_ma_type, slow_ma_type, signal_ma_type)
                if name.ends_with("_ma_type") || name == "ma_type" {
                    if let Some(ma_type) = Self::parse_ma_type_string(v) {
                        config.ma_types.insert(name.clone(), ma_type);
                    }
                }
                // Check for component-level source parameters (e.g., fast_source, slow_source)
                else if name.ends_with("_source") && name != "source" {
                    if let Some(field) = OhlcvField::from_str(v) {
                        // Extract component name (e.g., "fast" from "fast_source")
                        let component_name = name.strip_suffix("_source").unwrap_or(name);

                        // Get or create ComponentConfig for this component using Default
                        config.component_configs
                            .entry(component_name.to_string())
                            .or_insert_with(Default::default)
                            .source = Some(field);
                    }
                }
                // Global source parameter
                else if name == "source" {
                    if let Some(field) = OhlcvField::from_str(v) {
                        config.source = field;
                    }
                }
            }
        }

    }

    /// Parse a string to MovingAverageType
    fn parse_ma_type_string(s: &str) -> Option<crate::bar_indicators::average::moving_average::MovingAverageType> {
        use crate::bar_indicators::average::moving_average::MovingAverageType;

        match s.to_uppercase().as_str() {
            "SMA" => Some(MovingAverageType::SMA),
            "EMA" => Some(MovingAverageType::EMA),
            "WMA" => Some(MovingAverageType::WMA),
            "RMA" => Some(MovingAverageType::RMA),
            "DEMA" => Some(MovingAverageType::DEMA),
            "TEMA" => Some(MovingAverageType::TEMA),
            "HMA" => Some(MovingAverageType::HMA),
            "TMA" => Some(MovingAverageType::TMA),
            "VWMA" => Some(MovingAverageType::VWMA),
            "VWAP" => Some(MovingAverageType::VWAP),
            "AMA" => Some(MovingAverageType::AMA),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_periods() {
        let mut params = HashMap::new();
        params.insert("period".to_string(), IndicatorValue::Int(20));

        let periods = IndicatorCalculator::extract_periods(&params);
        assert_eq!(periods, vec![20]);
    }

    #[test]
    fn test_extract_periods_default() {
        let params = HashMap::new();
        let periods = IndicatorCalculator::extract_periods(&params);
        assert_eq!(periods, vec![14]); // default
    }

    #[test]
    fn test_calculate_sma() {
        // Create test bars
        let bars: Vec<Bar> = (0..100).map(|i| Bar {
            time: i as i64,
            open: 100.0 + i as f64,
            high: 101.0 + i as f64,
            low: 99.0 + i as f64,
            close: 100.0 + i as f64,
            volume: 1000.0,
        }).collect();

        let mut params = HashMap::new();
        params.insert("period".to_string(), IndicatorValue::Int(20));

        let result = IndicatorCalculator::calculate_with_id(
            BarIndicatorId::Sma,
            &params,
            &bars,
        );

        assert!(result.is_some());
        let values = result.unwrap();
        assert!(!values.is_empty());
    }
}
