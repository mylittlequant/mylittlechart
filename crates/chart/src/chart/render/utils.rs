//! Chart rendering utilities
//!
//! Shared types and helpers for chart rendering.

use super::super::annotations::LineStyle;

/// Options for grid rendering
#[derive(Clone, Debug)]
pub struct GridRenderOptions {
    /// Line color
    pub color: String,
    /// Line width
    pub width: f64,
    /// Line style
    pub style: LineStyle,
    /// Whether grid is visible
    pub visible: bool,
}

impl Default for GridRenderOptions {
    fn default() -> Self {
        Self {
            color: "#2a2e39".to_string(),
            width: 1.0,
            style: LineStyle::Solid,
            visible: true,
        }
    }
}

/// Line rendering style
#[derive(Clone, Copy, Debug, Default)]
pub struct LineRenderStyle {
    /// Dash length (0 for solid)
    pub dash: f64,
    /// Gap length
    pub gap: f64,
}

impl LineRenderStyle {
    pub fn solid() -> Self {
        Self { dash: 0.0, gap: 0.0 }
    }

    pub fn dashed() -> Self {
        Self { dash: 8.0, gap: 4.0 }
    }

    pub fn dotted() -> Self {
        Self { dash: 2.0, gap: 2.0 }
    }

    pub fn large_dashed() -> Self {
        Self { dash: 12.0, gap: 6.0 }
    }

    pub fn sparse_dotted() -> Self {
        Self { dash: 2.0, gap: 8.0 }
    }

    pub fn from_line_style(style: LineStyle) -> Self {
        match style {
            LineStyle::Solid => Self::solid(),
            LineStyle::Dashed => Self::dashed(),
            LineStyle::Dotted => Self::dotted(),
            LineStyle::LargeDashed => Self::large_dashed(),
            LineStyle::SparseDotted => Self::sparse_dotted(),
        }
    }

    pub fn is_solid(&self) -> bool {
        self.dash <= 0.0
    }
}
