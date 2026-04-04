//! Marker system for zengeld-chart
//!
//! Markers for annotating chart data with shapes and text.
//! Supports 4 built-in shapes (circle, square, arrowUp, arrowDown) and 6 positioning modes.

use crate::{Bar, PriceScale, Viewport};
use std::collections::HashMap;

// =============================================================================
// Marker Shape
// =============================================================================

/// Marker shape (4 built-in variants)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(serde::Serialize, serde::Deserialize)]
pub enum MarkerShape {
    /// Circle (size multiplier: 0.8)
    Circle,
    /// Square (size multiplier: 0.7)
    Square,
    /// Arrow pointing up (for buy signals)
    ArrowUp,
    /// Arrow pointing down (for sell signals)
    ArrowDown,
}

impl MarkerShape {
    /// Base size multiplier for the shape
    ///
    /// Used in formula: final_size = base_size * shape_multiplier * user_size
    #[inline]
    pub fn size_multiplier(&self) -> f64 {
        match self {
            MarkerShape::Circle => 0.8,
            MarkerShape::Square => 0.7,
            MarkerShape::ArrowUp | MarkerShape::ArrowDown => 1.0,
        }
    }
}

// =============================================================================
// Marker Position
// =============================================================================

/// Marker positioning relative to data or price
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(serde::Serialize, serde::Deserialize)]
pub enum MarkerPosition {
    // --- Data-Relative Positions (classic) ---

    /// Above the bar (uses high value)
    /// Y = priceToY(bar.high) - marker_height - padding
    AboveBar,

    /// Below the bar (uses low value)
    /// Y = priceToY(bar.low) + marker_height + padding
    BelowBar,

    /// Inside the bar (midpoint between high and low)
    /// Y = priceToY((bar.high + bar.low) / 2)
    InBar,

    // --- Price-Based Positions (require explicit price) ---

    /// At the top edge of specified price
    /// Requires: marker.price.is_some()
    AtPriceTop,

    /// At the bottom edge of specified price
    /// Requires: marker.price.is_some()
    AtPriceBottom,

    /// At the center of specified price
    /// Requires: marker.price.is_some()
    AtPriceMiddle,
}

impl MarkerPosition {
    /// Check if position requires explicit price specification
    #[inline]
    pub fn requires_explicit_price(&self) -> bool {
        matches!(
            self,
            MarkerPosition::AtPriceTop
                | MarkerPosition::AtPriceBottom
                | MarkerPosition::AtPriceMiddle
        )
    }
}

// =============================================================================
// Marker
// =============================================================================

/// Series marker - chart annotation
///
/// A marker is attached to a specific bar by time and positioned
/// vertically based on bar data or an explicit price.
///
/// # Example
///
/// ```
/// use zengeld_chart::{Marker, MarkerShape, MarkerPosition};
///
/// // Buy signal
/// let buy_marker = Marker {
///     time: 1609459200,
///     bar_idx: None,  // Will be calculated automatically
///     position: MarkerPosition::BelowBar,
///     shape: MarkerShape::ArrowUp,
///     color: "#4caf50".to_string(),
///     size: 1.0,
///     text: Some("BUY".to_string()),
///     id: Some("buy_signal_1".to_string()),
///     price: None,  // Not required for BelowBar
/// };
///
/// // Support level at exact price
/// let support_marker = Marker {
///     time: 1609459200,
///     bar_idx: None,
///     position: MarkerPosition::AtPriceMiddle,
///     shape: MarkerShape::Circle,
///     color: "#2196f3".to_string(),
///     size: 1.5,
///     text: Some("Support".to_string()),
///     id: Some("support_42500".to_string()),
///     price: Some(42500.0),  // REQUIRED for AtPriceMiddle
/// };
/// ```
#[derive(Debug, Clone)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Marker {
    /// Unix timestamp in seconds
    ///
    /// The marker will be attached to the bar with this timestamp.
    /// If no exact match exists, the marker may not display.
    pub time: i64,

    /// Bar index in data array
    ///
    /// Calculated automatically when markers are added.
    /// `None` until first recalculation.
    #[serde(skip)]
    pub bar_idx: Option<usize>,

    /// Marker position relative to bar or price
    pub position: MarkerPosition,

    /// Marker shape
    pub shape: MarkerShape,

    /// Marker color (CSS format: "#ff0000", "red", "rgb(255,0,0)")
    ///
    /// Applied to marker shape. Text uses global text_color.
    pub color: String,

    /// Size multiplier (default 1.0)
    ///
    /// - 0.0: hides shape but text remains
    /// - 0.5: marker is 2x smaller
    /// - 1.0: standard size
    /// - 2.0: marker is 2x larger
    #[serde(default = "default_size")]
    pub size: f64,

    /// Optional text label
    ///
    /// Displayed next to marker. Uses global font settings from Theme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Optional identifier
    ///
    /// Can be used for tracking and managing markers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Explicit price coordinate
    ///
    /// REQUIRED for positions: AtPriceTop, AtPriceBottom, AtPriceMiddle.
    /// Ignored for positions: AboveBar, BelowBar, InBar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
}

fn default_size() -> f64 {
    1.0
}

impl Marker {
    /// Create a new marker with minimal parameters
    pub fn new(
        time: i64,
        position: MarkerPosition,
        shape: MarkerShape,
        color: impl Into<String>,
    ) -> Self {
        Self {
            time,
            bar_idx: None,
            position,
            shape,
            color: color.into(),
            size: 1.0,
            text: None,
            id: None,
            price: None,
        }
    }

    /// Builder: set size
    pub fn with_size(mut self, size: f64) -> Self {
        self.size = size;
        self
    }

    /// Builder: set text
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Builder: set identifier
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Builder: set price (for price-based positions)
    pub fn with_price(mut self, price: f64) -> Self {
        self.price = Some(price);
        self
    }

    /// Validate marker
    ///
    /// Checks:
    /// - Price presence for price-based positions
    /// - Size correctness (>= 0)
    pub fn validate(&self) -> Result<(), String> {
        // Check price for price-based positions
        if self.position.requires_explicit_price() && self.price.is_none() {
            return Err(format!(
                "Price is required for position {:?}",
                self.position
            ));
        }

        // Check size
        if self.size < 0.0 {
            return Err(format!("Size must be >= 0, got {}", self.size));
        }

        Ok(())
    }

    /// Calculate final marker size in pixels
    ///
    /// Formula: base_size * shape_multiplier * user_size
    #[inline]
    pub fn calc_final_size(&self, base_size: f64) -> f64 {
        base_size * self.shape.size_multiplier() * self.size
    }
}

// =============================================================================
// Marker Coordinates (internal use)
// =============================================================================

/// Calculated marker coordinates in pixels
///
/// Used by renderer for drawing.
#[derive(Debug, Clone, Copy)]
pub struct MarkerCoordinates {
    /// X coordinate of marker center
    pub x: f64,
    /// Y coordinate of marker center
    pub y: f64,
    /// Calculated marker size in pixels
    pub size: f64,
}

// =============================================================================
// Marker Manager
// =============================================================================

/// Marker manager with performance optimization
///
/// Manages marker collection, provides:
/// - Lazy recalculation (recalc only on changes)
/// - Time-based sorting
/// - Visible marker filtering
/// - Text measurement caching
pub struct MarkerManager {
    /// All markers
    markers: Vec<Marker>,

    /// Flag for index recalculation requirement
    recalculation_required: bool,

    /// Base marker size in pixels
    base_size: f64,

    /// Padding between marker and bar (pixels)
    padding: f64,
}

impl MarkerManager {
    /// Create a new marker manager
    pub fn new() -> Self {
        Self {
            markers: Vec::new(),
            recalculation_required: false,
            base_size: 10.0,  // Base size for markers
            padding: 3.0,     // Standard padding
        }
    }

    /// Set all markers (replaces existing)
    ///
    /// # Behavior
    /// - Empty array removes all markers
    /// - Markers automatically sorted by time
    /// - Marks index recalculation as required
    pub fn set_markers(&mut self, markers: Vec<Marker>) {
        self.markers = markers;
        self.sort_by_time();
        self.recalculation_required = true;
    }

    /// Add one marker
    pub fn add_marker(&mut self, marker: Marker) {
        self.markers.push(marker);
        self.sort_by_time();
        self.recalculation_required = true;
    }

    /// Remove all markers
    pub fn clear(&mut self) {
        self.markers.clear();
        self.recalculation_required = false;
    }

    /// Get all markers
    pub fn markers(&self) -> &[Marker] {
        &self.markers
    }

    /// Number of markers
    pub fn len(&self) -> usize {
        self.markers.len()
    }

    /// Is empty
    pub fn is_empty(&self) -> bool {
        self.markers.is_empty()
    }

    /// Sort markers by time (chronological order)
    ///
    /// CRITICAL: Markers must be sorted for correct display
    fn sort_by_time(&mut self) {
        self.markers.sort_by_key(|m| m.time);
    }

    /// Recalculate bar indices for all markers
    ///
    /// Called only when markers or data changes.
    /// Uses recalculation_required flag for optimization.
    ///
    /// # Arguments
    /// - `bars`: chart bar array
    pub fn recalculate_indices(&mut self, bars: &[Bar]) {
        if !self.recalculation_required {
            return;
        }

        // Create hash map time -> index for fast lookup
        let time_to_index: HashMap<i64, usize> = bars
            .iter()
            .enumerate()
            .map(|(idx, bar)| (bar.timestamp, idx))
            .collect();

        // Update indices for all markers
        for marker in &mut self.markers {
            marker.bar_idx = time_to_index.get(&marker.time).copied();
        }

        self.recalculation_required = false;
    }

    /// Get visible markers in range
    ///
    /// Filters markers for rendering only in visible area.
    /// Critical for performance with large marker counts.
    pub fn visible_markers(&self, start_idx: usize, end_idx: usize) -> Vec<&Marker> {
        self.markers
            .iter()
            .filter(|m| {
                m.bar_idx
                    .map(|idx| idx >= start_idx && idx < end_idx)
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Find marker by ID
    pub fn find_by_id(&self, id: &str) -> Option<&Marker> {
        self.markers.iter().find(|m| {
            m.id.as_deref() == Some(id)
        })
    }

    /// Remove marker by ID
    pub fn remove_by_id(&mut self, id: &str) -> bool {
        if let Some(pos) = self.markers.iter().position(|m| {
            m.id.as_deref() == Some(id)
        }) {
            self.markers.remove(pos);
            self.recalculation_required = true;
            true
        } else {
            false
        }
    }

    /// Validate all markers
    pub fn validate_all(&self) -> Result<(), Vec<String>> {
        let errors: Vec<String> = self
            .markers
            .iter()
            .enumerate()
            .filter_map(|(idx, marker)| {
                marker.validate().err().map(|e| format!("Marker {}: {}", idx, e))
            })
            .collect();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Calculate marker coordinates for rendering
    ///
    /// # Arguments
    /// - `marker`: marker to calculate
    /// - `bars`: bar array
    /// - `viewport`: current viewport
    /// - `price_scale`: current price scale
    ///
    /// # Returns
    /// - `Some(MarkerCoordinates)` if marker is visible
    /// - `None` if marker is off-screen or bar_idx not found
    pub fn calc_marker_coords(
        &self,
        marker: &Marker,
        bars: &[Bar],
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Option<MarkerCoordinates> {
        // Get bar index
        let bar_idx = marker.bar_idx?;
        if bar_idx >= bars.len() {
            return None;
        }

        let bar = &bars[bar_idx];

        // Calculate X coordinate (bar center)
        let x = viewport.bar_to_x(bar_idx);

        // Calculate final marker size
        let marker_size = marker.calc_final_size(self.base_size);

        // Calculate Y coordinate based on position
        let y = match marker.position {
            // --- Data-Relative Positions ---
            MarkerPosition::AboveBar => {
                // Y = priceToY(bar.high) - marker_height - padding
                let price_y = viewport.price_to_y(
                    bar.high,
                    price_scale.price_min,
                    price_scale.price_max,
                );
                price_y - marker_size - self.padding
            }

            MarkerPosition::BelowBar => {
                // Y = priceToY(bar.low) + marker_height + padding
                let price_y = viewport.price_to_y(
                    bar.low,
                    price_scale.price_min,
                    price_scale.price_max,
                );
                price_y + marker_size + self.padding
            }

            MarkerPosition::InBar => {
                // Y = priceToY((bar.high + bar.low) / 2)
                let mid_price = (bar.high + bar.low) / 2.0;
                viewport.price_to_y(
                    mid_price,
                    price_scale.price_min,
                    price_scale.price_max,
                )
            }

            // --- Price-Based Positions ---
            MarkerPosition::AtPriceTop => {
                // Y = priceToY(marker.price) - marker_height / 2
                let price = marker.price?;
                let price_y = viewport.price_to_y(
                    price,
                    price_scale.price_min,
                    price_scale.price_max,
                );
                price_y - marker_size / 2.0
            }

            MarkerPosition::AtPriceBottom => {
                // Y = priceToY(marker.price) + marker_height / 2
                let price = marker.price?;
                let price_y = viewport.price_to_y(
                    price,
                    price_scale.price_min,
                    price_scale.price_max,
                );
                price_y + marker_size / 2.0
            }

            MarkerPosition::AtPriceMiddle => {
                // Y = priceToY(marker.price)
                let price = marker.price?;
                viewport.price_to_y(
                    price,
                    price_scale.price_min,
                    price_scale.price_max,
                )
            }
        };

        Some(MarkerCoordinates { x, y, size: marker_size })
    }
}

impl Default for MarkerManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_shape_multipliers() {
        assert_eq!(MarkerShape::Circle.size_multiplier(), 0.8);
        assert_eq!(MarkerShape::Square.size_multiplier(), 0.7);
        assert_eq!(MarkerShape::ArrowUp.size_multiplier(), 1.0);
        assert_eq!(MarkerShape::ArrowDown.size_multiplier(), 1.0);
    }

    #[test]
    fn test_marker_position_requires_price() {
        assert!(!MarkerPosition::AboveBar.requires_explicit_price());
        assert!(!MarkerPosition::BelowBar.requires_explicit_price());
        assert!(!MarkerPosition::InBar.requires_explicit_price());
        assert!(MarkerPosition::AtPriceTop.requires_explicit_price());
        assert!(MarkerPosition::AtPriceBottom.requires_explicit_price());
        assert!(MarkerPosition::AtPriceMiddle.requires_explicit_price());
    }

    #[test]
    fn test_marker_validation() {
        // Valid marker without price
        let marker = Marker::new(
            1609459200,
            MarkerPosition::BelowBar,
            MarkerShape::ArrowUp,
            "#4caf50",
        );
        assert!(marker.validate().is_ok());

        // Invalid: price-based position without price
        let marker = Marker::new(
            1609459200,
            MarkerPosition::AtPriceMiddle,
            MarkerShape::Circle,
            "#2196f3",
        );
        assert!(marker.validate().is_err());

        // Valid: price-based position with price
        let marker = Marker::new(
            1609459200,
            MarkerPosition::AtPriceMiddle,
            MarkerShape::Circle,
            "#2196f3",
        ).with_price(42500.0);
        assert!(marker.validate().is_ok());

        // Invalid: negative size
        let marker = Marker::new(
            1609459200,
            MarkerPosition::BelowBar,
            MarkerShape::ArrowUp,
            "#4caf50",
        ).with_size(-1.0);
        assert!(marker.validate().is_err());
    }

    #[test]
    fn test_marker_calc_final_size() {
        let base_size = 10.0;

        let marker = Marker::new(
            1609459200,
            MarkerPosition::BelowBar,
            MarkerShape::Circle,
            "#4caf50",
        ).with_size(2.0);

        // Circle multiplier (0.8) * user size (2.0) * base (10.0) = 16.0
        assert_eq!(marker.calc_final_size(base_size), 16.0);
    }

    #[test]
    fn test_marker_manager_basic() {
        let mut manager = MarkerManager::new();
        assert!(manager.is_empty());
        assert_eq!(manager.len(), 0);

        let marker = Marker::new(
            1609459200,
            MarkerPosition::BelowBar,
            MarkerShape::ArrowUp,
            "#4caf50",
        );

        manager.add_marker(marker);
        assert_eq!(manager.len(), 1);
        assert!(!manager.is_empty());

        manager.clear();
        assert!(manager.is_empty());
    }

    #[test]
    fn test_marker_manager_sorting() {
        let mut manager = MarkerManager::new();

        // Add markers in reverse chronological order
        manager.add_marker(Marker::new(
            1609545600,
            MarkerPosition::BelowBar,
            MarkerShape::ArrowUp,
            "#4caf50",
        ));
        manager.add_marker(Marker::new(
            1609459200,
            MarkerPosition::AboveBar,
            MarkerShape::ArrowDown,
            "#f44336",
        ));

        // Should be sorted by time
        let markers = manager.markers();
        assert_eq!(markers[0].time, 1609459200);
        assert_eq!(markers[1].time, 1609545600);
    }

    #[test]
    fn test_marker_manager_recalculate_indices() {
        let mut manager = MarkerManager::new();

        let bars = vec![
            Bar::new(1609459200, 100.0, 105.0, 95.0, 102.0),
            Bar::new(1609545600, 102.0, 108.0, 100.0, 106.0),
            Bar::new(1609632000, 106.0, 110.0, 104.0, 108.0),
        ];

        manager.add_marker(Marker::new(
            1609545600,
            MarkerPosition::BelowBar,
            MarkerShape::ArrowUp,
            "#4caf50",
        ));

        manager.recalculate_indices(&bars);

        let markers = manager.markers();
        assert_eq!(markers[0].bar_idx, Some(1));
    }
}
