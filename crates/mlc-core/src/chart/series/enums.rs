//! Series enums: LineType, PriceLineSource
//!
//! Note: LineStyle is re-exported from price_line module to avoid duplication

// Re-export LineStyle from price_line module (identical implementation)
pub use super::super::annotations::price_line::LineStyle;

/// Type of line connection between points
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum LineType {
    #[default]
    Simple = 0,    // Straight lines between points
    WithSteps = 1, // Step chart (horizontal then vertical)
    Curved = 2,    // Smoothed curves (cardinal splines)
}

/// Source of price for the price line
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PriceLineSource {
    #[default]
    LastBar,     // Last bar in data
    LastVisible, // Last visible bar
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_style_dash_patterns() {
        // Using the shared LineStyle from price_line
        assert_eq!(LineStyle::Solid.dash_pattern(2.0), Vec::<f64>::new());
        assert_eq!(LineStyle::Dotted.dash_pattern(2.0), vec![2.0, 2.0]);
        assert_eq!(LineStyle::Dashed.dash_pattern(2.0), vec![4.0, 4.0]);
        assert_eq!(LineStyle::LargeDashed.dash_pattern(2.0), vec![12.0, 12.0]);
        assert_eq!(LineStyle::SparseDotted.dash_pattern(2.0), vec![2.0, 8.0]);
    }

    #[test]
    fn test_line_style_defaults() {
        let style = LineStyle::default();
        assert_eq!(style, LineStyle::Solid);
    }

    #[test]
    fn test_line_type_defaults() {
        let line_type = LineType::default();
        assert_eq!(line_type, LineType::Simple);
    }
}
