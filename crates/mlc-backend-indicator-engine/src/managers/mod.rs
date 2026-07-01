//! Indicator-side managers.
//!
//! `SignalManager` and `TradeManager` previously lived here as dead
//! duplicates of the active copies on `mlc_core::drawing` (which are
//! owned by `DataBubble`).  K2.1 deleted them — the live signal/trade
//! state path is `DataBubble.signal_manager` / `DataBubble.trade_manager`
//! today, and will move to top-level `Managers` in K2.3/K2.4.

pub mod indicator_bridge;
pub mod indicator_calculator;
pub mod indicator_manager;

pub use indicator_manager::{
    IndicatorManager, IndicatorDefinition, IndicatorInstance, IndicatorParam,
    IndicatorParamType, IndicatorValue, IndicatorOutput, IndicatorOutputType,
    HistogramStyle, OutputConfig, RecalcMode,
};
pub use indicator_bridge::IndicatorBridge;
pub use indicator_calculator::{IndicatorCalculator, CalculationResult};
