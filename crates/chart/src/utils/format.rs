//! Formatting utilities for display
//!
//! Platform-independent formatting functions for numbers and values.

/// Format large numbers with K/M suffixes for display
///
/// # Examples
///
/// ```
/// use zengeld_chart::format_indicator_value;
///
/// assert_eq!(format_indicator_value(1_500_000.0), "1.50M");
/// assert_eq!(format_indicator_value(2_500.0), "2.50K");
/// assert_eq!(format_indicator_value(42.0), "42.00");
/// assert_eq!(format_indicator_value(0.1234), "0.1234");
/// ```
pub fn format_indicator_value(value: f64) -> String {
    if value.abs() >= 1_000_000.0 {
        format!("{:.2}M", value / 1_000_000.0)
    } else if value.abs() >= 1_000.0 {
        format!("{:.2}K", value / 1_000.0)
    } else if value.abs() >= 1.0 {
        format!("{:.2}", value)
    } else {
        format!("{:.4}", value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_millions() {
        assert_eq!(format_indicator_value(1_000_000.0), "1.00M");
        assert_eq!(format_indicator_value(1_500_000.0), "1.50M");
        assert_eq!(format_indicator_value(10_000_000.0), "10.00M");
        assert_eq!(format_indicator_value(-2_500_000.0), "-2.50M");
    }

    #[test]
    fn test_format_thousands() {
        assert_eq!(format_indicator_value(1_000.0), "1.00K");
        assert_eq!(format_indicator_value(2_500.0), "2.50K");
        assert_eq!(format_indicator_value(999_999.0), "1000.00K");
        assert_eq!(format_indicator_value(-5_000.0), "-5.00K");
    }

    #[test]
    fn test_format_regular() {
        assert_eq!(format_indicator_value(1.0), "1.00");
        assert_eq!(format_indicator_value(42.5), "42.50");
        assert_eq!(format_indicator_value(999.99), "999.99");
        assert_eq!(format_indicator_value(-50.0), "-50.00");
    }

    #[test]
    fn test_format_small() {
        assert_eq!(format_indicator_value(0.1), "0.1000");
        assert_eq!(format_indicator_value(0.1234), "0.1234");
        assert_eq!(format_indicator_value(0.00001), "0.0000");
        assert_eq!(format_indicator_value(-0.5), "-0.5000");
    }

    #[test]
    fn test_format_zero() {
        assert_eq!(format_indicator_value(0.0), "0.0000");
    }
}
