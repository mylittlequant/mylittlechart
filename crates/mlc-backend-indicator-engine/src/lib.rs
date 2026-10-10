//! Technical Indicators Library for zengeld-chart
//!
//! This crate provides 480+ technical indicators organized into 23 categories.
//!
//! ## Categories
//!
//! | Category | Count | Description |
//! |----------|-------|-------------|
//! | momentum | 113 | RSI, MACD, Stochastic, ADX |
//! | signal_processing | 84 | Filters, FFT, wavelets |
//! | channels | 72 | Bollinger Bands, Keltner, Ichimoku |
//! | volatility | 52 | ATR, Historical Volatility |
//! | statistics | 29 | ADF, KPSS, cointegration tests |
//! | levels | 27 | Pivots, VWAP, FVG |
//! | average | 23 | SMA, EMA, HMA, DEMA |
//! | volume | 22 | OBV, MFI, VPIN |
//! | trend_stop | 21 | ATR Stop, Chandelier |
//! | entropy | 20 | Shannon, Fisher |
//! | trend | 19 | Supertrend, GMMA |
//! | position | 19 | Seasonal, temporal |
//! | candles | 19 | Candlestick patterns |
//! | chaos | 18 | Alligator, Fractals |
//! | kalman | 15 | Kalman, EKF, UKF |
//! | divergence | 13 | MACD Div, RSI Div |
//! | accumulation | 13 | A/D, CMF |
//! | adaptive | 7 | KAMA, FRAMA, VIDYA |
//! | regression | 7 | ARIMA, GARCH |
//! | clusters | 6 | Order Flow |
//! | zigzag | 5 | Zigzag variants |
//! | ratio | 4 | Efficiency Ratio |
//! | book | 4 | Order Book |
//!
//! ## Usage
//!
//! ```rust
//! use zengeld_chart_indicators::Bar;
//!
//! // Create a bar
//! let bar = Bar::new(1700000000, 100.0, 105.0, 99.0, 103.0, 1000.0);
//! assert_eq!(bar.close, 103.0);
//! ```

// Compute + catalog are provided by mli (mylittleindicators) — phase 1 of
// the dig3+mli migration deleted the in-tree bar_indicators/, catalog/, and
// types/ trees that were byte-for-byte duplicates of mli's code. Re-export
// mli's modules under the historical mlc paths so downstream consumers
// (chart-app, panels, sidebar-content, signals/, managers/) keep working
// with their existing `crate::bar_indicators::...` / `crate::catalog::...`
// / `crate::types::...` imports.
pub use mli::core::types;
/// The stream vocabulary indicators declare their data needs in.
pub use mli::engine::stream_kind::StreamKind;
pub use mli::engine::indicator_id::IndicatorId;
pub use IndicatorId as BarIndicatorId;

// Signal system — UI-side feature, NOT in mli. Lives in mlc permanently.
pub mod signals;

// Re-exports for convenience (same surface as before; types now resolve
// to mli's definitions through the module-level pub-uses above).
pub use mli::{
    Bar,
    Tick,
    CalendarService,
    TimeService,
};
pub use managers::contract_instance::{ContractInstance as IndicatorInstance, OutputSample};

// Signal system re-exports
pub use signals::{
    SignalKind, SignalCategory,
    // SignalKind subtypes
    ThresholdSub, HistogramSub, ChannelSub, DivergenceSub, TrendSub,
    VolatilitySub, VolumeSub, StructureSub, PatternSub, CompositeSub,
    // Condition primitives (used by detectors internally and by consumers)
    ThresholdCondition, CrossoverType, CompareCondition, TrendCondition,
    DivergenceType, ChannelPosition, PatternState, CandlePattern,
    VolatilityRegime, VolumeCharacter, LogicOp, ConfirmationRequirement,
    // Detectors
    CrossoverDetector, ThresholdMonitor, ZeroCrossDetector, HistogramDetector,
    ChannelDetector, DivergenceDetector, TrendDetector, VolatilityDetector,
    VolumeDetector, SwingDetector, MultiSignalDetector,
    // Signal types
    Signal, Direction, BarConfirmation, SignalSource,
};

// Indicator value types + compute primitives. `IndicatorManager` (the
// stateful registry) moved to `mlc_backend::indicator::manager` in the
// 3.7a manager lift — this crate keeps only the compute-side shell:
// catalog bridge, calculator, and the value types both sides speak.
// SignalManager and TradeManager were dead duplicates of
// `mlc_core::drawing::{SignalManager, TradeManager}` — deleted in K2.1.
pub mod managers;

pub use managers::{
    IndicatorDefinition,
    IndicatorInstance as IndicatorMgrInstance,
    IndicatorParam, IndicatorParamType,
    IndicatorValue as IndicatorParamValue,
    IndicatorOutput, IndicatorOutputType,
    HistogramStyle, OutputConfig,
    RecalcMode,
    IndicatorBridge,
    IndicatorCalculator, CalculationResult,
    StreamEvent,
};
