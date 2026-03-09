//! Grid overlay configuration
//!
//! Provides customizable grid lines with dash patterns and colors
//! for both vertical (time) and horizontal (price) lines.

use serde::{Deserialize, Serialize};

// Re-export LineStyle from price_line for consistency
pub use super::super::annotations::price_line::LineStyle;

// =============================================================================
// Grid Line Options
// =============================================================================

/// Options for grid lines (vertical or horizontal)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GridLineOptions {
    /// Line color
    pub color: String,

    /// Line style (solid, dashed, etc)
    #[serde(default)]
    pub style: LineStyle,

    /// Visibility of lines
    #[serde(default = "default_true")]
    pub visible: bool,
}

fn default_true() -> bool {
    true
}

impl Default for GridLineOptions {
    fn default() -> Self {
        Self {
            color: "rgba(42, 46, 57, 0.6)".to_string(),
            style: LineStyle::Solid,
            visible: true,
        }
    }
}

// =============================================================================
// Grid Options
// =============================================================================

/// Complete grid configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GridOptions {
    /// Vertical lines (time axis)
    #[serde(default)]
    pub vert_lines: GridLineOptions,

    /// Horizontal lines (price axis)
    #[serde(default)]
    pub horz_lines: GridLineOptions,
}

impl Default for GridOptions {
    fn default() -> Self {
        Self {
            vert_lines: GridLineOptions::default(),
            horz_lines: GridLineOptions::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_options_default() {
        let grid = GridOptions::default();
        assert!(grid.vert_lines.visible);
        assert!(grid.horz_lines.visible);
    }

    #[test]
    fn test_grid_line_options_serialization() {
        let opts = GridLineOptions {
            color: "#ff0000".to_string(),
            style: LineStyle::Dashed,
            visible: true,
        };

        let json = serde_json::to_string(&opts).unwrap();
        let parsed: GridLineOptions = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.color, "#ff0000");
    }

    #[test]
    fn test_grid_options_hidden() {
        let mut grid = GridOptions::default();
        grid.vert_lines.visible = false;
        grid.horz_lines.visible = false;

        assert!(!grid.vert_lines.visible);
        assert!(!grid.horz_lines.visible);
    }
}
