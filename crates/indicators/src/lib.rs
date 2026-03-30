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

// Core types (bar, tick, time, calendar)
pub mod types;

// Core indicator types
pub mod bar_indicators;

// Catalog system
pub mod catalog;

// Signal system - signal types, conditions, and detectors
pub mod signals;

// Re-exports for convenience
pub use bar_indicators::{
    bar_indicator_id::BarIndicatorId,
    indicator_value::IndicatorValue,
    instance_factory::{IndicatorConfig, IndicatorInstance},
};

pub use catalog::{
    master_catalog::MasterIndicatorCatalog,
    indicator_signature::IndicatorSignature,
    constraints::ParamConstraint,
    param_value::ParamValue,
};

pub use types::{Bar, Tick, CalendarService, TimeService};

// Signal system re-exports
pub use signals::{
    SignalKind, SignalCategory,
    ThresholdCondition, CrossoverType, CompareCondition, TrendCondition,
    DivergenceType, ChannelPosition, PatternState, CandlePattern,
    VolatilityRegime, VolumeCharacter, LogicOp, ConfirmationRequirement,
    CrossoverDetector, ThresholdMonitor, ZeroCrossDetector, HistogramDetector,
    ChannelDetector, DivergenceDetector, TrendDetector, VolatilityDetector,
    VolumeDetector, SwingDetector, MultiSignalDetector,
    Signal, Direction, BarConfirmation, SignalSource,
};

// Manager types: IndicatorManager, SignalManager, TradeManager + support types
pub mod managers;

pub use managers::{
    IndicatorManager, IndicatorDefinition,
    IndicatorInstance as IndicatorMgrInstance,
    IndicatorParam, IndicatorParamType,
    IndicatorValue as IndicatorParamValue,
    IndicatorOutput, IndicatorOutputType,
    HistogramStyle, OutputConfig,
    RecalcMode,
    IndicatorBridge,
    IndicatorCalculator, CalculationResult,
    SignalManager,
    Trade, TradeDirection, TradeManager,
};
