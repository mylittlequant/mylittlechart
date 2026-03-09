//! Template for compare overlay visual style.
//!
//! A [`CompareTemplate`] saves the line color, width, and dash style for a
//! compare overlay so the same appearance can be reused when adding new
//! comparison series.

use serde::{Deserialize, Serialize};

use crate::preset::preset::unix_timestamp_parts;

// =============================================================================
// CompareTemplate
// =============================================================================

/// Saved visual style for a compare overlay series.
///
/// Does not store the symbol — the user selects the symbol at apply time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareTemplate {
    /// Unique identifier generated at creation time.
    /// Format: `"ctmpl_{unix_secs}_{nanos_suffix}"`.
    pub id: String,
    /// User-visible display name for this template.
    pub name: String,
    /// Line color as a hex string (e.g. `"#2196F3"`).
    pub color: String,
    /// Line width in pixels.
    pub line_width: f32,
    /// Line dash style: `"solid"`, `"dashed"`, or `"dotted"`.
    pub line_style: String,
}

impl CompareTemplate {
    /// Create a template with the given visual style.
    pub fn new(name: &str, color: &str, line_width: f32, line_style: &str) -> Self {
        let (secs, nanos) = unix_timestamp_parts();
        Self {
            id: format!("ctmpl_{}_{}", secs, nanos),
            name: name.to_string(),
            color: color.to_string(),
            line_width,
            line_style: line_style.to_string(),
        }
    }

    /// Create a template with default solid-blue style.
    pub fn new_with_defaults(name: &str) -> Self {
        Self::new(name, "#2196F3", 2.0, "solid")
    }

    /// Apply this template's style to a [`CompareSeries`].
    ///
    /// Only visual fields (`color`, `line_width`, `line_style`) are written;
    /// symbol, bars, and base price are left untouched.
    pub fn apply_to_series(&self, series: &mut crate::chart::types::compare::CompareSeries) {
        series.color = self.color.clone();
        series.line_width = self.line_width;
        series.line_style = self.line_style.clone();
    }
}
