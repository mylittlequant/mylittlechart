//! Unified drag mode for zengeld-chart
//!
//! This module defines the `DragMode` enum which represents what is currently
//! being dragged in the chart. This is the single source of truth for drag state.

/// Current drag interaction mode.
///
/// Describes what element the user is currently dragging.
///
/// # Variants
///
/// The variants are organized into categories:
///
/// ## No Drag
/// - `None` - No drag in progress
///
/// ## Main Chart Areas
/// - `Chart` - Panning the main chart area
/// - `PriceScale` - Vertical zoom on price scale (Y axis)
/// - `TimeScale` - Horizontal zoom on time scale (X axis)
///
/// ## Drawing Primitives
/// - `Primitive` - Dragging an entire primitive drawing object
/// - `ControlPoint` - Dragging a control point to resize/reshape
///
/// ## Sub-Panes (Indicators)
/// - `SubPaneChart` - Panning a sub-pane chart area
/// - `SubPanePriceScale` - Vertical zoom on sub-pane price scale
/// - `PaneSeparator` - Resizing pane heights
///
/// ## Selection
/// - `Selection` - Creating a selection rectangle
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum DragMode {
    /// No drag in progress.
    #[default]
    None,

    /// Panning the main chart area.
    Chart,

    /// Vertical zoom on price scale (Y axis).
    PriceScale,

    /// Horizontal zoom on time scale (X axis).
    TimeScale,

    /// Dragging a primitive drawing object (whole object move).
    ///
    /// The `id` is the unique identifier of the primitive being dragged.
    Primitive {
        /// Unique ID of the primitive being dragged.
        id: u64,
    },

    /// Dragging a control point to resize/reshape a primitive.
    ///
    /// Control points are the handles on primitives that allow precise
    /// adjustment of their shape and position.
    ControlPoint {
        /// ID of the primitive whose control point is being dragged.
        primitive_id: u64,
        /// Index of the control point within the primitive.
        point_index: usize,
    },

    /// Panning a sub-pane chart area (indicator pane).
    ///
    /// Sub-panes are separate chart areas below the main chart,
    /// typically used for indicators like RSI, MACD, etc.
    SubPaneChart {
        /// Index of the sub-pane being panned.
        pane_index: usize,
    },

    /// Vertical zoom on sub-pane price scale.
    ///
    /// Each sub-pane has its own Y axis that can be zoomed independently.
    SubPanePriceScale {
        /// Index of the sub-pane whose price scale is being zoomed.
        pane_index: usize,
    },

    /// Dragging a pane separator to resize pane heights.
    ///
    /// Separators are the horizontal dividers between panes that can
    /// be dragged to adjust relative pane heights.
    PaneSeparator {
        /// Index of the pane above the separator being dragged.
        pane_index: usize,
    },

    /// Creating a selection rectangle.
    ///
    /// Used for multi-select operations where the user drags to
    /// create a rectangle that selects all objects within it.
    Selection,
}

impl DragMode {
    /// Check if crosshair should be updated in this drag mode.
    ///
    /// Crosshair follows cursor when:
    /// - Not dragging (`None`)
    /// - Panning chart (`Chart`) - crosshair follows real cursor position
    /// - Dragging a primitive (`Primitive`) - user needs visual feedback
    /// - Dragging a control point (`ControlPoint`) - precise positioning needed
    ///
    /// Crosshair stays fixed when:
    /// - Zooming price/time scales - not useful during zoom
    ///
    /// # Example
    ///
    /// ```
    /// use zengeld_chart::input::DragMode;
    ///
    /// assert!(DragMode::None.allows_crosshair_update());
    /// assert!(DragMode::Chart.allows_crosshair_update());
    /// assert!(DragMode::Primitive { id: 1 }.allows_crosshair_update());
    /// ```
    #[inline]
    pub fn allows_crosshair_update(self) -> bool {
        matches!(
            self,
            DragMode::None
                | DragMode::Chart
                | DragMode::Primitive { .. }
                | DragMode::ControlPoint { .. }
                | DragMode::Selection
        )
    }

    /// Check if this is a sub-pane related drag mode.
    ///
    /// Returns `true` for `SubPaneChart` and `SubPanePriceScale` variants.
    ///
    /// # Example
    ///
    /// ```
    /// use zengeld_chart::input::DragMode;
    ///
    /// assert!(DragMode::SubPaneChart { pane_index: 0 }.is_sub_pane_drag());
    /// assert!(DragMode::SubPanePriceScale { pane_index: 1 }.is_sub_pane_drag());
    /// assert!(!DragMode::Chart.is_sub_pane_drag());
    /// ```
    #[inline]
    pub fn is_sub_pane_drag(self) -> bool {
        matches!(
            self,
            DragMode::SubPaneChart { .. } | DragMode::SubPanePriceScale { .. }
        )
    }

    /// Get pane index if this is a sub-pane drag mode.
    ///
    /// Returns `Some(pane_index)` for `SubPaneChart` and `SubPanePriceScale`,
    /// `None` for all other variants.
    ///
    /// # Example
    ///
    /// ```
    /// use zengeld_chart::input::DragMode;
    ///
    /// assert_eq!(DragMode::SubPaneChart { pane_index: 2 }.sub_pane_index(), Some(2));
    /// assert_eq!(DragMode::Chart.sub_pane_index(), None);
    /// ```
    #[inline]
    pub fn sub_pane_index(self) -> Option<usize> {
        match self {
            DragMode::SubPaneChart { pane_index } | DragMode::SubPanePriceScale { pane_index } => {
                Some(pane_index)
            }
            _ => None,
        }
    }

    /// Check if currently dragging anything.
    ///
    /// Returns `true` for all variants except `None`.
    ///
    /// # Example
    ///
    /// ```
    /// use zengeld_chart::input::DragMode;
    ///
    /// assert!(!DragMode::None.is_dragging());
    /// assert!(DragMode::Chart.is_dragging());
    /// assert!(DragMode::Primitive { id: 42 }.is_dragging());
    /// ```
    #[inline]
    pub fn is_dragging(self) -> bool {
        !matches!(self, DragMode::None)
    }

    /// Check if dragging affects the chart view (pan/zoom).
    ///
    /// Returns `true` for drag modes that modify the viewport:
    /// - `Chart` - panning changes visible range
    /// - `PriceScale` - zooming Y axis
    /// - `TimeScale` - zooming X axis
    /// - `SubPaneChart` - panning sub-pane
    /// - `SubPanePriceScale` - zooming sub-pane Y axis
    ///
    /// Returns `false` for primitive/selection drags which don't
    /// change the viewport.
    ///
    /// # Example
    ///
    /// ```
    /// use zengeld_chart::input::DragMode;
    ///
    /// assert!(DragMode::Chart.affects_view());
    /// assert!(DragMode::PriceScale.affects_view());
    /// assert!(!DragMode::Primitive { id: 1 }.affects_view());
    /// assert!(!DragMode::Selection.affects_view());
    /// ```
    #[inline]
    pub fn affects_view(self) -> bool {
        matches!(
            self,
            DragMode::Chart
                | DragMode::PriceScale
                | DragMode::TimeScale
                | DragMode::SubPaneChart { .. }
                | DragMode::SubPanePriceScale { .. }
        )
    }

    /// Get the primitive ID if this is a primitive-related drag.
    ///
    /// Returns the primitive ID for `Primitive` and `ControlPoint` variants.
    ///
    /// # Example
    ///
    /// ```
    /// use zengeld_chart::input::DragMode;
    ///
    /// assert_eq!(DragMode::Primitive { id: 42 }.primitive_id(), Some(42));
    /// assert_eq!(
    ///     DragMode::ControlPoint { primitive_id: 7, point_index: 0 }.primitive_id(),
    ///     Some(7)
    /// );
    /// assert_eq!(DragMode::Chart.primitive_id(), None);
    /// ```
    #[inline]
    pub fn primitive_id(self) -> Option<u64> {
        match self {
            DragMode::Primitive { id } => Some(id),
            DragMode::ControlPoint { primitive_id, .. } => Some(primitive_id),
            _ => None,
        }
    }

    /// Check if this drag mode involves a primitive (drawing object).
    ///
    /// Returns `true` for `Primitive` and `ControlPoint` variants.
    ///
    /// # Example
    ///
    /// ```
    /// use zengeld_chart::input::DragMode;
    ///
    /// assert!(DragMode::Primitive { id: 1 }.is_primitive_drag());
    /// assert!(DragMode::ControlPoint { primitive_id: 1, point_index: 0 }.is_primitive_drag());
    /// assert!(!DragMode::Chart.is_primitive_drag());
    /// ```
    #[inline]
    pub fn is_primitive_drag(self) -> bool {
        matches!(
            self,
            DragMode::Primitive { .. } | DragMode::ControlPoint { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_none() {
        assert_eq!(DragMode::default(), DragMode::None);
    }

    #[test]
    fn test_is_dragging() {
        assert!(!DragMode::None.is_dragging());
        assert!(DragMode::Chart.is_dragging());
        assert!(DragMode::PriceScale.is_dragging());
        assert!(DragMode::TimeScale.is_dragging());
        assert!(DragMode::Primitive { id: 1 }.is_dragging());
        assert!(DragMode::ControlPoint {
            primitive_id: 1,
            point_index: 0
        }
        .is_dragging());
        assert!(DragMode::SubPaneChart { pane_index: 0 }.is_dragging());
        assert!(DragMode::SubPanePriceScale { pane_index: 0 }.is_dragging());
        assert!(DragMode::PaneSeparator { pane_index: 0 }.is_dragging());
        assert!(DragMode::Selection.is_dragging());
    }

    #[test]
    fn test_affects_view() {
        // View-affecting modes
        assert!(DragMode::Chart.affects_view());
        assert!(DragMode::PriceScale.affects_view());
        assert!(DragMode::TimeScale.affects_view());
        assert!(DragMode::SubPaneChart { pane_index: 0 }.affects_view());
        assert!(DragMode::SubPanePriceScale { pane_index: 0 }.affects_view());

        // Non-view-affecting modes
        assert!(!DragMode::None.affects_view());
        assert!(!DragMode::Primitive { id: 1 }.affects_view());
        assert!(!DragMode::ControlPoint {
            primitive_id: 1,
            point_index: 0
        }
        .affects_view());
        assert!(!DragMode::PaneSeparator { pane_index: 0 }.affects_view());
        assert!(!DragMode::Selection.affects_view());
    }

    #[test]
    fn test_allows_crosshair_update() {
        // Crosshair should update
        assert!(DragMode::None.allows_crosshair_update());
        assert!(DragMode::Chart.allows_crosshair_update());
        assert!(DragMode::Primitive { id: 1 }.allows_crosshair_update());
        assert!(DragMode::ControlPoint {
            primitive_id: 1,
            point_index: 0
        }
        .allows_crosshair_update());
        assert!(DragMode::Selection.allows_crosshair_update());

        // Crosshair should NOT update
        assert!(!DragMode::PriceScale.allows_crosshair_update());
        assert!(!DragMode::TimeScale.allows_crosshair_update());
        assert!(!DragMode::SubPaneChart { pane_index: 0 }.allows_crosshair_update());
        assert!(!DragMode::SubPanePriceScale { pane_index: 0 }.allows_crosshair_update());
        assert!(!DragMode::PaneSeparator { pane_index: 0 }.allows_crosshair_update());
    }

    #[test]
    fn test_is_sub_pane_drag() {
        assert!(DragMode::SubPaneChart { pane_index: 0 }.is_sub_pane_drag());
        assert!(DragMode::SubPanePriceScale { pane_index: 1 }.is_sub_pane_drag());

        assert!(!DragMode::None.is_sub_pane_drag());
        assert!(!DragMode::Chart.is_sub_pane_drag());
        assert!(!DragMode::PaneSeparator { pane_index: 0 }.is_sub_pane_drag());
    }

    #[test]
    fn test_sub_pane_index() {
        assert_eq!(
            DragMode::SubPaneChart { pane_index: 2 }.sub_pane_index(),
            Some(2)
        );
        assert_eq!(
            DragMode::SubPanePriceScale { pane_index: 3 }.sub_pane_index(),
            Some(3)
        );
        assert_eq!(DragMode::Chart.sub_pane_index(), None);
        assert_eq!(DragMode::None.sub_pane_index(), None);
    }

    #[test]
    fn test_primitive_id() {
        assert_eq!(DragMode::Primitive { id: 42 }.primitive_id(), Some(42));
        assert_eq!(
            DragMode::ControlPoint {
                primitive_id: 7,
                point_index: 2
            }
            .primitive_id(),
            Some(7)
        );
        assert_eq!(DragMode::Chart.primitive_id(), None);
        assert_eq!(DragMode::None.primitive_id(), None);
    }

    #[test]
    fn test_is_primitive_drag() {
        assert!(DragMode::Primitive { id: 1 }.is_primitive_drag());
        assert!(DragMode::ControlPoint {
            primitive_id: 1,
            point_index: 0
        }
        .is_primitive_drag());

        assert!(!DragMode::None.is_primitive_drag());
        assert!(!DragMode::Chart.is_primitive_drag());
        assert!(!DragMode::Selection.is_primitive_drag());
    }

    #[test]
    fn test_eq_and_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(DragMode::Chart);
        set.insert(DragMode::PriceScale);
        set.insert(DragMode::Primitive { id: 1 });
        set.insert(DragMode::Primitive { id: 1 }); // Duplicate

        assert_eq!(set.len(), 3);
        assert!(set.contains(&DragMode::Chart));
        assert!(set.contains(&DragMode::Primitive { id: 1 }));
        assert!(!set.contains(&DragMode::Primitive { id: 2 }));
    }
}
