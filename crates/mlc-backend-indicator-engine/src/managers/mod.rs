//! Indicator compute primitives.
//!
//! `SignalManager` and `TradeManager` previously lived here as dead
//! duplicates of the active copies on `mlc_core::drawing` (which are
//! owned by `DataBubble`).  K2.1 deleted them — the live signal/trade
//! state path is `DataBubble.signal_manager` / `DataBubble.trade_manager`
//! today, and will move to top-level `Managers` in K2.3/K2.4.
//!
//! `IndicatorManager` (the stateful registry) moved to
//! `mlc_backend::indicator::manager` in the 3.7a manager lift — this
//! module now holds only the value types (`IndicatorDefinition`,
//! `IndicatorInstance`, `ComputeCache`, …) and the pure compute helpers
//! (`IndicatorBridge`, `IndicatorCalculator`) the manager calls into.

pub mod indicator_bridge;
pub mod indicator_calculator;
pub mod indicator_manager;

pub use indicator_manager::{
    IndicatorDefinition, IndicatorInstance, IndicatorParam,
    IndicatorParamType, IndicatorValue, IndicatorOutput, IndicatorOutputType,
    HistogramStyle, OutputConfig, RecalcMode, ComputeCache,
};
pub use indicator_bridge::IndicatorBridge;
pub use indicator_calculator::{IndicatorCalculator, CalculationResult};
