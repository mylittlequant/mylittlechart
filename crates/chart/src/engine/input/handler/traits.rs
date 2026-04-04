//! Input handler traits for zengeld-chart
//!
//! This module defines traits for hit testing and input handling that
//! platform implementations must provide.

use super::super::events::ChartInputAction;
use super::super::objects::CursorStyle;
use crate::ui::modal_settings::SubPaneButton;

/// Result of a hit test on the chart.
///
/// Describes what element (if any) is at a given screen coordinate.
/// Used to determine how to handle user interactions like clicks and drags.
///
/// # Example
///
/// ```ignore
/// use zengeld_chart::input::HitResult;
///
/// let hit = hit_tester.hit_test(x, y);
/// match hit {
///     HitResult::Chart => {
///         // User clicked on main chart - start panning
///     }
///     HitResult::Primitive { id } => {
///         // User clicked on a drawing - select it
///     }
///     _ => {}
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HitResult {
    /// Main chart area.
    ///
    /// The central area where candlesticks/lines are displayed.
    Chart,

    /// Price scale (Y axis).
    ///
    /// The vertical scale showing price values, typically on the right.
    PriceScale,

    /// Time scale (X axis).
    ///
    /// The horizontal scale showing time/date values at the bottom.
    TimeScale,

    /// Scale corner (intersection of price and time scales).
    ///
    /// Contains A/M toggle and mode (lin/log/%) buttons.
    ScaleCorner,

    /// Sub-pane chart area.
    ///
    /// Chart area within an indicator pane (RSI, MACD, etc.).
    SubPaneChart {
        /// Index of the sub-pane (0 = first indicator pane).
        pane_index: usize,
    },

    /// Sub-pane price scale.
    ///
    /// Y axis of an indicator pane.
    SubPanePriceScale {
        /// Index of the sub-pane.
        pane_index: usize,
    },

    /// Pane separator handle.
    ///
    /// The draggable divider between panes used to resize them.
    PaneSeparator {
        /// Instance ID of the pane whose separator was hit.
        instance_id: u64,
    },

    /// Drawing primitive (trend line, rectangle, etc.).
    ///
    /// User clicked on a drawing object that can be selected/dragged.
    Primitive {
        /// Unique ID of the primitive.
        id: u64,
    },

    /// Control point on a primitive.
    ///
    /// The small handles used to resize/reshape primitives.
    ControlPoint {
        /// ID of the primitive this control point belongs to.
        primitive_id: u64,
        /// Index of the control point within the primitive.
        point_index: usize,
    },

    /// Sub-pane overlay button (delete, hide, move-up, expand/restore).
    ///
    /// User clicked on a button in the sub-pane's overlay bar.
    SubPaneOverlayButton {
        /// Index of the sub-pane this button belongs to.
        pane_index: usize,
        /// Which button was hit.
        button: SubPaneButton,
    },

    /// Toolbar area.
    ///
    /// Any toolbar (top, left, right, bottom) - should not start chart interactions.
    Toolbar,

    /// No interactive element hit.
    ///
    /// Outside all interactive regions.
    #[default]
    None,
}

impl HitResult {
    /// Check if this hit result represents a draggable chart element.
    ///
    /// Returns `true` for Chart, PriceScale, TimeScale, ScaleCorner, and sub-pane equivalents.
    #[inline]
    pub fn is_chart_element(&self) -> bool {
        matches!(
            self,
            HitResult::Chart
                | HitResult::PriceScale
                | HitResult::TimeScale
                | HitResult::ScaleCorner
                | HitResult::SubPaneChart { .. }
                | HitResult::SubPanePriceScale { .. }
        )
    }

    /// Check if this hit result represents a primitive/drawing.
    #[inline]
    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            HitResult::Primitive { .. } | HitResult::ControlPoint { .. }
        )
    }

    /// Check if this hit result is interactive (not None or Toolbar).
    #[inline]
    pub fn is_interactive(&self) -> bool {
        !matches!(self, HitResult::None | HitResult::Toolbar)
    }

    /// Get the appropriate cursor style for this hit zone.
    pub fn cursor(&self) -> CursorStyle {
        match self {
            HitResult::Chart | HitResult::SubPaneChart { .. } => CursorStyle::Crosshair,
            HitResult::PriceScale | HitResult::SubPanePriceScale { .. } => CursorStyle::NsResize,
            HitResult::TimeScale => CursorStyle::EwResize,
            HitResult::PaneSeparator { .. } => CursorStyle::NsResize,
            HitResult::Primitive { .. } => CursorStyle::Move,
            HitResult::ControlPoint { .. } => CursorStyle::Move,
            HitResult::SubPaneOverlayButton { .. } => CursorStyle::Pointer,
            HitResult::ScaleCorner | HitResult::Toolbar | HitResult::None => CursorStyle::Default,
        }
    }

    /// Get the primitive ID if this is a primitive hit.
    #[inline]
    pub fn primitive_id(&self) -> Option<u64> {
        match self {
            HitResult::Primitive { id } => Some(*id),
            HitResult::ControlPoint { primitive_id, .. } => Some(*primitive_id),
            _ => None,
        }
    }
}

/// Trait for hit testing on the chart.
///
/// Platform implementations must provide this to allow the input system
/// to determine what element is under the cursor at any given position.
///
/// # Example
///
/// ```ignore
/// struct MyChart {
///     chart_rect: (f64, f64, f64, f64),
///     // ...
/// }
///
/// impl ChartHitTester for MyChart {
///     fn hit_test(&self, x: f64, y: f64) -> HitResult {
///         let (cx, cy, cw, ch) = self.chart_rect;
///         if x >= cx && x < cx + cw && y >= cy && y < cy + ch {
///             HitResult::Chart
///         } else {
///             HitResult::None
///         }
///     }
///
///     fn chart_rect(&self) -> (f64, f64, f64, f64) {
///         self.chart_rect
///     }
///     // ...
/// }
/// ```
pub trait ChartHitTester {
    /// Determine what element is at the given screen coordinates.
    ///
    /// # Arguments
    ///
    /// * `x` - X coordinate in screen pixels
    /// * `y` - Y coordinate in screen pixels
    ///
    /// # Returns
    ///
    /// The `HitResult` describing what element (if any) is at that position.
    fn hit_test(&self, x: f64, y: f64) -> HitResult;

    /// Get the main chart area rectangle.
    ///
    /// # Returns
    ///
    /// Tuple of (x, y, width, height) in screen pixels.
    fn chart_rect(&self) -> (f64, f64, f64, f64);

    /// Get the price scale rectangle if visible.
    ///
    /// # Returns
    ///
    /// `Some((x, y, width, height))` if price scale is visible, `None` otherwise.
    fn price_scale_rect(&self) -> Option<(f64, f64, f64, f64)>;

    /// Get the time scale rectangle if visible.
    ///
    /// # Returns
    ///
    /// `Some((x, y, width, height))` if time scale is visible, `None` otherwise.
    fn time_scale_rect(&self) -> Option<(f64, f64, f64, f64)>;
}

/// Trait for handling chart input actions.
///
/// Implement this trait to process input actions and update chart state.
///
/// # Example
///
/// ```ignore
/// struct MyChartHandler {
///     viewport: Viewport,
/// }
///
/// impl ChartInputHandler for MyChartHandler {
///     fn handle_action(&mut self, action: ChartInputAction, hit_tester: &dyn ChartHitTester) {
///         match action {
///             ChartInputAction::Pan { delta_x, delta_y } => {
///                 self.viewport.pan(delta_x, delta_y);
///             }
///             ChartInputAction::Zoom { center_x, center_y, factor_x, factor_y } => {
///                 self.viewport.zoom(center_x, center_y, factor_x, factor_y);
///             }
///             _ => {}
///         }
///     }
/// }
/// ```
pub trait ChartInputHandler {
    /// Handle an input action.
    ///
    /// This method is called when the input system produces an action
    /// that should be processed by the chart.
    ///
    /// # Arguments
    ///
    /// * `action` - The action to handle
    /// * `hit_tester` - Reference to hit tester for additional queries
    fn handle_action(&mut self, action: ChartInputAction, hit_tester: &dyn ChartHitTester);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hit_result_is_chart_element() {
        assert!(HitResult::Chart.is_chart_element());
        assert!(HitResult::PriceScale.is_chart_element());
        assert!(HitResult::TimeScale.is_chart_element());
        assert!(HitResult::ScaleCorner.is_chart_element());
        assert!(HitResult::SubPaneChart { pane_index: 0 }.is_chart_element());
        assert!(HitResult::SubPanePriceScale { pane_index: 0 }.is_chart_element());

        assert!(!HitResult::Primitive { id: 1 }.is_chart_element());
        assert!(!HitResult::Toolbar.is_chart_element());
        assert!(!HitResult::None.is_chart_element());
    }

    #[test]
    fn test_hit_result_is_primitive() {
        assert!(HitResult::Primitive { id: 1 }.is_primitive());
        assert!(HitResult::ControlPoint {
            primitive_id: 1,
            point_index: 0
        }
        .is_primitive());

        assert!(!HitResult::Chart.is_primitive());
        assert!(!HitResult::None.is_primitive());
    }

    #[test]
    fn test_hit_result_is_interactive() {
        assert!(HitResult::Chart.is_interactive());
        assert!(HitResult::Primitive { id: 1 }.is_interactive());
        assert!(HitResult::PaneSeparator { instance_id: 0 }.is_interactive());

        assert!(!HitResult::None.is_interactive());
        assert!(!HitResult::Toolbar.is_interactive());
    }

    #[test]
    fn test_hit_result_primitive_id() {
        assert_eq!(HitResult::Primitive { id: 42 }.primitive_id(), Some(42));
        assert_eq!(
            HitResult::ControlPoint {
                primitive_id: 7,
                point_index: 2
            }
            .primitive_id(),
            Some(7)
        );
        assert_eq!(HitResult::Chart.primitive_id(), None);
    }

    #[test]
    fn test_hit_result_default() {
        assert_eq!(HitResult::default(), HitResult::None);
    }
}
