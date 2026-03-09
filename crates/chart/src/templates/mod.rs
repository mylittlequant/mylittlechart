//! Template system for chart drawing tools, indicators, and compare overlays.
//!
//! Templates allow users to save and reuse style configurations for:
//! - Drawing primitives (trend lines, Fibonacci retracements, rectangles, etc.)
//! - Indicators (parameters + output style)
//! - Compare overlays (color, line width/style)
//! - Indicator sets (groups of indicators saved together)
//!
//! # Storage
//!
//! Templates are persisted as JSON files under a `templates/` directory
//! next to the application binary, mirroring the preset storage pattern.
//! Sub-directories:
//! - `templates/primitives/`   — [`PrimitiveTemplate`] files
//! - `templates/indicators/`   — [`IndicatorTemplate`] files
//! - `templates/compare/`      — [`CompareTemplate`] files
//! - `templates/indicator_sets/` — [`IndicatorSet`] files

pub mod primitive_template;
pub mod indicator_template;
pub mod compare_template;
pub mod chart_template;
pub mod indicator_set;
pub mod set_manager;
pub mod manager;
pub mod storage;

pub use primitive_template::PrimitiveTemplate;
pub use indicator_template::{IndicatorTemplate, OutputStyleConfig};
pub use compare_template::CompareTemplate;
pub use chart_template::ChartTemplate;
pub use indicator_set::IndicatorSet;
pub use set_manager::IndicatorSetManager;
pub use manager::TemplateManager;
pub use storage::TemplateError;
