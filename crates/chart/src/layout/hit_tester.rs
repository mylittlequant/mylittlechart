//! Hit testing implementation for layout
//!
//! This module provides ChartHitTester implementations:
//! - `LayoutHitTester` - for simple ChartAreaLayout (no sub-panes)
//! - `ExtendedLayoutHitTester` - for ExtendedFrameLayout (with sub-panes)

use crate::input::{ChartHitTester, HitResult};
use super::rects::{ChartAreaLayout, ChartHitZone, ExtendedFrameLayout};

/// Adapter that implements ChartHitTester for ChartAreaLayout
///
/// This allows the layout to be used directly with DefaultChartInputHandler.
pub struct LayoutHitTester<'a> {
    /// The chart area layout to test against
    pub layout: &'a ChartAreaLayout,
}

impl<'a> LayoutHitTester<'a> {
    /// Create a new hit tester for the given layout
    pub fn new(layout: &'a ChartAreaLayout) -> Self {
        Self { layout }
    }
}

impl<'a> ChartHitTester for LayoutHitTester<'a> {
    fn hit_test(&self, x: f64, y: f64) -> HitResult {
        match self.layout.hit_test(x, y) {
            ChartHitZone::Chart => HitResult::Chart,
            ChartHitZone::SubPane { pane_index } => HitResult::SubPaneChart { pane_index },
            ChartHitZone::SubPanePriceScale { pane_index } => HitResult::SubPanePriceScale { pane_index },
            ChartHitZone::PriceScale => HitResult::PriceScale,
            ChartHitZone::TimeScale => HitResult::TimeScale,
            ChartHitZone::ScaleCorner => HitResult::ScaleCorner,
            ChartHitZone::None => HitResult::None,
        }
    }

    fn chart_rect(&self) -> (f64, f64, f64, f64) {
        self.layout.chart_rect_tuple()
    }

    fn price_scale_rect(&self) -> Option<(f64, f64, f64, f64)> {
        Some(self.layout.price_scale_rect_tuple())
    }

    fn time_scale_rect(&self) -> Option<(f64, f64, f64, f64)> {
        Some(self.layout.time_scale_rect_tuple())
    }
}

// =============================================================================
// Extended Layout Hit Tester (with sub-panes)
// =============================================================================

/// Hit tester for ExtendedFrameLayout (supports sub-panes)
///
/// This hit tester properly handles:
/// - Main chart area
/// - Sub-pane chart areas (returns `SubPaneChart { pane_index }`)
/// - Sub-pane price scales (returns `SubPanePriceScale { pane_index }`)
/// - Pane separators (returns `PaneSeparator { pane_index }`)
pub struct ExtendedLayoutHitTester<'a> {
    /// The extended frame layout with sub-pane information
    pub layout: &'a ExtendedFrameLayout,
}

impl<'a> ExtendedLayoutHitTester<'a> {
    /// Create a new extended hit tester
    pub fn new(layout: &'a ExtendedFrameLayout) -> Self {
        Self { layout }
    }
}

impl<'a> ChartHitTester for ExtendedLayoutHitTester<'a> {
    fn hit_test(&self, x: f64, y: f64) -> HitResult {
        // 1. Check sub-pane separators first (small targets, highest priority)
        if let Some(pane_index) = self.layout.find_separator_at_y(y) {
            // Check if X is within the chart+scale area
            if x >= self.layout.main_chart.chart.x
                && x <= self.layout.main_chart.chart.x + self.layout.main_chart.chart.width + self.layout.main_chart.price_scale.width
            {
                return HitResult::PaneSeparator { pane_index };
            }
        }

        // 2. Check sub-pane areas
        for (idx, pane) in self.layout.sub_panes.iter().enumerate() {
            // Check sub-pane price scale
            if pane.price_scale.contains(x, y) {
                return HitResult::SubPanePriceScale { pane_index: idx };
            }
            // Check sub-pane chart content
            if pane.content.contains(x, y) {
                return HitResult::SubPaneChart { pane_index: idx };
            }
        }

        // 3. Check main chart areas (using reduced main_chart layout)
        // Note: main_chart.hit_test() won't return SubPane variants since those are
        // checked explicitly above, but we include them for exhaustiveness
        match self.layout.main_chart.hit_test(x, y) {
            ChartHitZone::Chart => HitResult::Chart,
            ChartHitZone::SubPane { pane_index } => HitResult::SubPaneChart { pane_index },
            ChartHitZone::SubPanePriceScale { pane_index } => HitResult::SubPanePriceScale { pane_index },
            ChartHitZone::PriceScale => HitResult::PriceScale,
            ChartHitZone::TimeScale => HitResult::TimeScale,
            ChartHitZone::ScaleCorner => HitResult::ScaleCorner,
            ChartHitZone::None => HitResult::None,
        }
    }

    fn chart_rect(&self) -> (f64, f64, f64, f64) {
        // Return main chart rect (not including sub-panes)
        self.layout.main_chart.chart_rect_tuple()
    }

    fn price_scale_rect(&self) -> Option<(f64, f64, f64, f64)> {
        Some(self.layout.main_chart.price_scale_rect_tuple())
    }

    fn time_scale_rect(&self) -> Option<(f64, f64, f64, f64)> {
        Some(self.layout.main_chart.time_scale_rect_tuple())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::LayoutRect;

    #[test]
    fn test_layout_hit_tester() {
        let available = LayoutRect::new(0.0, 0.0, 800.0, 600.0);
        let layout = ChartAreaLayout::compute(available, 70.0, 30.0);
        let tester = LayoutHitTester::new(&layout);

        // Test chart area
        assert_eq!(tester.hit_test(100.0, 100.0), HitResult::Chart);

        // Test price scale
        assert_eq!(tester.hit_test(750.0, 100.0), HitResult::PriceScale);

        // Test time scale
        assert_eq!(tester.hit_test(100.0, 580.0), HitResult::TimeScale);

        // Test scale corner
        assert_eq!(tester.hit_test(750.0, 580.0), HitResult::ScaleCorner);

        // Test outside
        assert_eq!(tester.hit_test(-10.0, -10.0), HitResult::None);
    }

    #[test]
    fn test_rect_methods() {
        let available = LayoutRect::new(0.0, 0.0, 800.0, 600.0);
        let layout = ChartAreaLayout::compute(available, 70.0, 30.0);
        let tester = LayoutHitTester::new(&layout);

        let (x, y, w, h) = tester.chart_rect();
        assert_eq!(x, 0.0);
        assert_eq!(y, 0.0);
        assert_eq!(w, 730.0);
        assert_eq!(h, 570.0);

        let price_rect = tester.price_scale_rect().unwrap();
        assert_eq!(price_rect.0, 730.0);
        assert_eq!(price_rect.2, 70.0);

        let time_rect = tester.time_scale_rect().unwrap();
        assert_eq!(time_rect.1, 570.0);
        assert_eq!(time_rect.3, 30.0);
    }

    #[test]
    fn test_extended_hit_tester_no_subpanes() {
        use crate::layout::Margins;

        let margins = Margins::zero();
        let layout = ExtendedFrameLayout::compute(
            800.0, 600.0, &margins,
            &[], // No sub-panes
            100.0, 1.0,
        );
        let tester = ExtendedLayoutHitTester::new(&layout);

        // Without sub-panes, should behave like basic tester
        // Chart area is inside chart_panel
        let chart_y = layout.main_chart.chart.y + 50.0;
        assert_eq!(tester.hit_test(100.0, chart_y), HitResult::Chart);
    }

    #[test]
    fn test_extended_hit_tester_with_subpanes() {
        use crate::layout::Margins;

        let margins = Margins::zero();
        let layout = ExtendedFrameLayout::compute(
            800.0, 600.0, &margins,
            &[1, 2], // Two sub-panes
            100.0, 1.0,
        );
        let tester = ExtendedLayoutHitTester::new(&layout);

        // Main chart should return Chart
        let main_chart_y = layout.main_chart.chart.y + 50.0;
        assert_eq!(tester.hit_test(100.0, main_chart_y), HitResult::Chart);

        // First sub-pane content should return SubPaneChart
        if let Some(pane) = layout.sub_panes.get(0) {
            let sub_pane_y = pane.content.y + 50.0;
            assert_eq!(
                tester.hit_test(100.0, sub_pane_y),
                HitResult::SubPaneChart { pane_index: 0 }
            );

            // First sub-pane price scale
            let price_scale_x = pane.price_scale.x + 10.0;
            assert_eq!(
                tester.hit_test(price_scale_x, sub_pane_y),
                HitResult::SubPanePriceScale { pane_index: 0 }
            );
        }

        // Second sub-pane
        if let Some(pane) = layout.sub_panes.get(1) {
            let sub_pane_y = pane.content.y + 50.0;
            assert_eq!(
                tester.hit_test(100.0, sub_pane_y),
                HitResult::SubPaneChart { pane_index: 1 }
            );
        }
    }
}
