//! Manager types for indicators, signals, and trades.
//!
//! This module contains the high-level managers that live in the indicators crate:
//! - `IndicatorManager` — manages indicator definitions and instances
//! - `SignalManager` — manages strategy-generated system signals
//! - `TradeManager` — manages trade visualization data

pub mod indicator_bridge;
pub mod indicator_calculator;
pub mod indicator_manager;
pub mod signal_manager;
pub mod trade_manager;

pub use indicator_manager::{
    IndicatorManager, IndicatorDefinition, IndicatorInstance, IndicatorParam,
    IndicatorParamType, IndicatorValue, IndicatorOutput, IndicatorOutputType,
    HistogramStyle, OutputConfig, RecalcMode,
};
pub use indicator_bridge::IndicatorBridge;
pub use indicator_calculator::{IndicatorCalculator, CalculationResult};
pub use signal_manager::SignalManager;
pub use trade_manager::{Trade, TradeDirection, TradeManager};
