//! Coordinate Helper - price/pixel coordinate transformations
//!
//! This helper provides a unified interface for converting between price
//! and pixel coordinates using viewport and price scale.

use crate::chart::{PriceScale, Viewport};

/// Helper for coordinate transformations between price and pixel space
///
/// This struct wraps references to Viewport and PriceScale to provide
/// convenient coordinate conversion methods.
///
/// # Example
/// ```
/// use zengeld_chart::{CoordinateHelper, Viewport, PriceScale};
///
/// let viewport = Viewport::new(800.0, 400.0);
/// let price_scale = PriceScale::new(100.0, 200.0);
/// let helper = CoordinateHelper::new(&viewport, &price_scale);
///
/// let y = helper.price_to_y(150.0);
/// let price = helper.y_to_price(y);
/// assert!((price - 150.0).abs() < 0.001);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct CoordinateHelper<'a> {
    viewport: &'a Viewport,
    price_scale: &'a PriceScale,
}

impl<'a> CoordinateHelper<'a> {
    /// Create a new coordinate helper with references to viewport and price scale
    ///
    /// # Arguments
    /// * `viewport` - Reference to the chart viewport for dimension information
    /// * `price_scale` - Reference to the price scale for price range information
    #[inline]
    pub fn new(viewport: &'a Viewport, price_scale: &'a PriceScale) -> Self {
        Self {
            viewport,
            price_scale,
        }
    }

    /// Convert price to Y pixel coordinate
    ///
    /// Uses the viewport's price_to_y method with the current price scale range.
    /// The Y coordinate is inverted (higher prices = lower Y values).
    ///
    /// # Arguments
    /// * `price` - Price value to convert
    ///
    /// # Returns
    /// Y pixel coordinate in the chart area
    #[inline]
    pub fn price_to_y(&self, price: f64) -> f64 {
        self.viewport.price_to_y(
            price,
            self.price_scale.price_min,
            self.price_scale.price_max,
        )
    }

    /// Convert Y pixel coordinate to price
    ///
    /// Uses the viewport's y_to_price method with the current price scale range.
    /// The Y coordinate is inverted (lower Y values = higher prices).
    ///
    /// # Arguments
    /// * `y` - Y pixel coordinate in the chart area
    ///
    /// # Returns
    /// Price value at the given Y coordinate
    #[inline]
    pub fn y_to_price(&self, y: f64) -> f64 {
        self.viewport.y_to_price(
            y,
            self.price_scale.price_min,
            self.price_scale.price_max,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinate_helper_price_to_y() {
        let viewport = Viewport {
            chart_height: 400.0,
            ..Default::default()
        };
        let price_scale = PriceScale::new(100.0, 200.0);
        let helper = CoordinateHelper::new(&viewport, &price_scale);

        // At price_min (100.0), Y should be at bottom (400.0)
        let y = helper.price_to_y(100.0);
        assert!((y - 400.0).abs() < 0.001);

        // At price_max (200.0), Y should be at top (0.0)
        let y = helper.price_to_y(200.0);
        assert!((y - 0.0).abs() < 0.001);

        // At midpoint (150.0), Y should be at middle (200.0)
        let y = helper.price_to_y(150.0);
        assert!((y - 200.0).abs() < 0.001);
    }

    #[test]
    fn test_coordinate_helper_y_to_price() {
        let viewport = Viewport {
            chart_height: 400.0,
            ..Default::default()
        };
        let price_scale = PriceScale::new(100.0, 200.0);
        let helper = CoordinateHelper::new(&viewport, &price_scale);

        // At Y=0 (top), price should be max (200.0)
        let price = helper.y_to_price(0.0);
        assert!((price - 200.0).abs() < 0.001);

        // At Y=400 (bottom), price should be min (100.0)
        let price = helper.y_to_price(400.0);
        assert!((price - 100.0).abs() < 0.001);

        // At Y=200 (middle), price should be 150.0
        let price = helper.y_to_price(200.0);
        assert!((price - 150.0).abs() < 0.001);
    }

    #[test]
    fn test_coordinate_helper_roundtrip() {
        let viewport = Viewport {
            chart_height: 400.0,
            ..Default::default()
        };
        let price_scale = PriceScale::new(100.0, 200.0);
        let helper = CoordinateHelper::new(&viewport, &price_scale);

        // Test roundtrip conversion
        let original_price = 175.0;
        let y = helper.price_to_y(original_price);
        let recovered_price = helper.y_to_price(y);
        assert!((recovered_price - original_price).abs() < 0.001);
    }
}
