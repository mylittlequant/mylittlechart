//! Input event handlers
//!
//! Traits and implementations for processing input events.
//!
//! - `ChartInputHandler` - Trait for processing input actions
//! - `ChartHitTester` - Trait for hit testing
//! - `DefaultChartInputHandler` - Default implementation

mod default;
mod traits;

pub use default::{
    ChartInputState, ChartOutputAction, DefaultChartInputHandler, InputHandlerConfig, UndoAction,
};
pub use traits::{ChartHitTester, ChartInputHandler, HitResult};
