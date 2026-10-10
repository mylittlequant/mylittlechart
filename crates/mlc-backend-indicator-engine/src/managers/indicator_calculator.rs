//! Indicator Calculator
//!
//! Builds a boxed [`ContractFactory`](mli::engine::contract_engine::ContractFactory)
//! and reads [`Render`](mli::contract::Render) output names. The chart stores
//! the box. The factory itself stays box-free.

use std::collections::HashMap;

use super::contract_instance::{ContractInstance, OutputSample};
use super::indicator_manager::IndicatorValue;
use super::stream_event::StreamEvent;
use crate::signals::rules::{default_profile, SignalEngine};
use crate::signals::signal::Signal;
use crate::Bar;
use crate::IndicatorId;

/// Result of indicator calculation including values and signals.
pub struct CalculationResult {
    pub values: HashMap<String, Vec<f64>>,
    pub signals: Vec<Signal>,
}

/// Calculator that drives the contract factory.
pub struct IndicatorCalculator;

impl IndicatorCalculator {
    /// Calculate one indicator over `bars`.
    ///
    /// `deltas`, when present, must match `bars`. CVD consumes
    /// `(buy_volume, sell_volume)`. Every other id is fed the bar.
    pub fn calculate_with_id(
        indicator_id: IndicatorId,
        params: &HashMap<String, IndicatorValue>,
        bars: &[Bar],
        deltas: Option<&[(f64, f64)]>,
    ) -> Option<HashMap<String, Vec<f64>>> {
        let mut instance = Self::build_compute_instance(indicator_id, params)?;
        let mut columns: HashMap<String, Vec<f64>> = HashMap::new();
        for (i, bar) in bars.iter().enumerate() {
            let delta = deltas.and_then(|d| d.get(i)).copied();
            let sample = Self::feed_bar_with_delta(&mut instance, bar, delta);
            for (name, value) in sample.by_name {
                columns.entry(name).or_default().push(value);
            }
        }
        Some(columns)
    }

    /// Calculate values and run the default signal profile.
    pub fn calculate_with_signals(
        indicator_id: IndicatorId,
        params: &HashMap<String, IndicatorValue>,
        bars: &[Bar],
        deltas: Option<&[(f64, f64)]>,
    ) -> Option<CalculationResult> {
        let mut instance = Self::build_compute_instance(indicator_id, params)?;
        let mut signal_engine = default_profile(indicator_id).map(|p| SignalEngine::from_profile(&p));
        let indicator_name = indicator_id.label();
        let mut columns: HashMap<String, Vec<f64>> = HashMap::new();
        let mut signals = Vec::new();
        let bar_count = bars.len();
        for (i, bar) in bars.iter().enumerate() {
            let delta = deltas.and_then(|d| d.get(i)).copied();
            let sample = Self::feed_bar_with_delta(&mut instance, bar, delta);
            if let Some(engine) = signal_engine.as_mut() {
                signals.extend(engine.process(
                    &sample,
                    bar.open,
                    bar.high,
                    bar.low,
                    bar.close,
                    bar.volume,
                    bar.time,
                    &indicator_name,
                    i + 1 == bar_count,
                ));
            }
            for (name, value) in sample.by_name {
                columns.entry(name).or_default().push(value);
            }
        }
        Some(CalculationResult {
            values: columns,
            signals,
        })
    }

    /// Integer params the settings modal stored, priority names first.
    /// Empty input yields `[14]`, matching the historical helper.
    pub fn extract_periods(params: &HashMap<String, IndicatorValue>) -> Vec<usize> {
        let mut periods = Vec::new();
        let priority_names = [
            "period", "length", "n", "window", "periods", "fast", "slow", "signal", "k", "d",
            "smooth",
        ];
        for name in priority_names {
            if let Some(value) = params.get(name).and_then(|v| v.as_int()) {
                periods.push(value.max(1) as usize);
            }
        }
        for (name, value) in params {
            let name_lower = name.to_lowercase();
            if priority_names.iter().any(|p| *p == name_lower) {
                continue;
            }
            if let Some(value) = value.as_int() {
                periods.push(value.max(1) as usize);
            }
        }
        if periods.is_empty() {
            periods.push(14);
        }
        periods
    }

    fn explicit_period(params: &HashMap<String, IndicatorValue>) -> Option<usize> {
        for name in ["period", "length", "n", "window"] {
            if let Some(value) = params.get(name).and_then(|v| v.as_int()) {
                return Some(value.max(1) as usize);
            }
        }
        None
    }

    /// Fresh factory. `None` when `indicator_id` is not in the contract.
    pub fn build_compute_instance(
        indicator_id: IndicatorId,
        params: &HashMap<String, IndicatorValue>,
    ) -> Option<ContractInstance> {
        ContractInstance::build(indicator_id, Self::explicit_period(params))
    }

    pub fn feed_bar(instance: &mut ContractInstance, bar: &Bar) -> OutputSample {
        instance.feed_bar(bar);
        instance.sample()
    }

    pub fn feed_bar_with_delta(
        instance: &mut ContractInstance,
        bar: &Bar,
        delta: Option<(f64, f64)>,
    ) -> OutputSample {
        instance.feed_bar_with_delta(bar, delta);
        instance.sample()
    }

    pub fn feed_stream_event(instance: &mut ContractInstance, ev: &StreamEvent) -> OutputSample {
        instance.feed_event(ev);
        instance.sample()
    }

    pub fn extract_single_outputs(
        _indicator_id: IndicatorId,
        value: OutputSample,
    ) -> HashMap<String, f64> {
        value.map()
    }

    pub fn params_fingerprint(
        indicator_id: IndicatorId,
        params: &HashMap<String, IndicatorValue>,
    ) -> u64 {
        const FNV_OFFSET: u64 = 14695981039346656037;
        const FNV_PRIME: u64 = 1099511628211;
        let mut hash = FNV_OFFSET;
        let fnv_byte = |h: &mut u64, b: u8| {
            *h ^= b as u64;
            *h = h.wrapping_mul(FNV_PRIME);
        };
        for b in format!("{indicator_id:?}").bytes() {
            fnv_byte(&mut hash, b);
        }
        let mut pairs: Vec<(&String, &IndicatorValue)> = params.iter().collect();
        pairs.sort_by_key(|(k, _)| k.as_str());
        for (k, v) in pairs {
            for b in k.bytes() {
                fnv_byte(&mut hash, b);
            }
            for b in format!("{v:?}").bytes() {
                fnv_byte(&mut hash, b);
            }
        }
        hash
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
        assert_eq!(periods, vec![14]);
    }

    #[test]
    fn test_calculate_sma() {
        let bars: Vec<Bar> = (0..100)
            .map(|i| Bar {
                time: i as i64,
                open: 100.0 + i as f64,
                high: 101.0 + i as f64,
                low: 99.0 + i as f64,
                close: 100.0 + i as f64,
                volume: 1000.0,
            })
            .collect();
        let mut params = HashMap::new();
        params.insert("period".to_string(), IndicatorValue::Int(20));
        let result = IndicatorCalculator::calculate_with_id(IndicatorId::Sma, &params, &bars, None);
        assert!(result.is_some());
        let values = result.unwrap();
        assert!(!values.is_empty());
    }

    fn zero_bar() -> Bar {
        Bar {
            time: 0,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 10.0,
        }
    }

    fn build(id: IndicatorId) -> ContractInstance {
        IndicatorCalculator::build_compute_instance(id, &HashMap::new()).expect("builds")
    }

    #[test]
    fn feed_stream_event_tick_moves_trade_flow_imbalance() {
        let mut instance = build(IndicatorId::TradeFlowImbalance);
        let baseline = IndicatorCalculator::feed_bar(&mut instance, &zero_bar());
        let tick = crate::Tick::new(0, 100.0, 5.0, true);
        let updated = IndicatorCalculator::feed_stream_event(&mut instance, &StreamEvent::Tick(tick));
        assert_ne!(updated, baseline);
        assert_eq!(updated.lines, vec![1.0, 5.0]);
    }

    #[test]
    fn feed_stream_event_orderbook_moves_book_imbalance() {
        let mut instance = build(IndicatorId::BookImb);
        let baseline = IndicatorCalculator::feed_bar(&mut instance, &zero_bar());
        let book = crate::types::OrderBook {
            bids: vec![crate::types::OrderBookLevel::new(100.0, 30.0)],
            asks: vec![crate::types::OrderBookLevel::new(101.0, 10.0)],
            timestamp: 0,
            ..Default::default()
        };
        let updated =
            IndicatorCalculator::feed_stream_event(&mut instance, &StreamEvent::OrderBook(book));
        assert_ne!(updated, baseline);
        assert!((updated.main() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn feed_stream_event_orderbook_delta_moves_book_churn_rate() {
        let mut instance = build(IndicatorId::BookChurnRate);
        let baseline = IndicatorCalculator::feed_bar(&mut instance, &zero_bar());
        let delta = crate::types::OrderbookDelta {
            bids: vec![
                crate::types::OrderBookLevel::new(100.0, 5.0),
                crate::types::OrderBookLevel::new(99.0, 5.0),
            ],
            asks: vec![crate::types::OrderBookLevel::new(101.0, 5.0)],
            timestamp: 1000,
            first_update_id: None,
            last_update_id: None,
            prev_update_id: None,
            ..Default::default()
        };
        let updated = IndicatorCalculator::feed_stream_event(
            &mut instance,
            &StreamEvent::OrderbookDelta(delta),
        );
        assert_ne!(updated, baseline);
        assert!((updated.main() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn feed_stream_event_funding_moves_funding_momentum() {
        let mut instance = build(IndicatorId::FundingMomentum);
        let baseline = IndicatorCalculator::feed_bar(&mut instance, &zero_bar());
        let fr = crate::types::FundingRate {
            rate: 0.0001,
            next_funding_time: None,
            timestamp: 1000,
            ..Default::default()
        };
        let updated =
            IndicatorCalculator::feed_stream_event(&mut instance, &StreamEvent::Funding(fr));
        assert_ne!(updated, baseline);
        assert_eq!(updated.lines, vec![0.0001, 0.0001]);
    }

    #[test]
    fn feed_stream_event_open_interest_moves_oi_change_rate() {
        let mut instance = build(IndicatorId::OiChangeRate);
        let baseline = IndicatorCalculator::feed_bar(&mut instance, &zero_bar());
        let oi = crate::types::OpenInterest {
            open_interest: 1000.0,
            open_interest_value: None,
            timestamp: 0,
            ..Default::default()
        };
        let updated =
            IndicatorCalculator::feed_stream_event(&mut instance, &StreamEvent::OpenInterest(oi));
        assert_ne!(updated, baseline);
        assert_eq!(updated.lines, vec![0.0, 1000.0]);
    }

    #[test]
    fn feed_stream_event_liquidation_moves_liquidation_rate() {
        let mut instance = build(IndicatorId::LiquidationRate);
        let baseline = IndicatorCalculator::feed_bar(&mut instance, &zero_bar());
        let liq = crate::types::Liquidation {
            symbol: "BTCUSDT".to_string(),
            side: crate::types::TradeSide::Buy,
            price: 30_000.0,
            quantity: 0.1,
            timestamp: 0,
            value: None,
            ..Default::default()
        };
        let updated =
            IndicatorCalculator::feed_stream_event(&mut instance, &StreamEvent::Liquidation(liq));
        assert_ne!(updated, baseline);
        assert!((updated.main() - 1.0 / 60.0).abs() < 1e-9);
    }
}
