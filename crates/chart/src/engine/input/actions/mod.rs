//! High-level chart actions (commands)
//!
//! User intents that can be triggered from UI, keyboard shortcuts, or programmatically.
//!
//! - `ChartAction` - All possible UI actions (toggle grid, zoom in, select tool, etc.)
//! - `Shortcut` - Keyboard shortcut definition

mod chart_action;

pub use chart_action::{ChartAction, Shortcut};
