//! Pure geometry data types shared between backend state and frontend layout.

use crate::{CursorStyle, ScaleSettings, PriceScalePosition, TimeScalePosition};
use crate::state::InnerLayout;

/// Simple rectangle for layout computation - no external dependencies
///
/// Coordinates use top-left origin (0,0 at top-left of screen).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayoutRect {
    /// X coordinate of left edge
    pub x: f64,
    /// Y coordinate of top edge
    pub y: f64,
    /// Width of rectangle
    pub width: f64,
    /// Height of rectangle
    pub height: f64,
}

impl LayoutRect {
    /// Create a new rectangle
    #[inline]
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }

    /// Create rectangle from min/max coordinates
    #[inline]
    pub fn from_min_max(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            x: min_x,
            y: min_y,
            width: max_x - min_x,
            height: max_y - min_y,
        }
    }

    /// Create a zero-sized rectangle at origin
    #[inline]
    pub const fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0, 0.0)
    }

    /// Right edge X coordinate
    #[inline]
    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    /// Bottom edge Y coordinate
    #[inline]
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    /// Left edge (alias for x)
    #[inline]
    pub fn left(&self) -> f64 {
        self.x
    }

    /// Top edge (alias for y)
    #[inline]
    pub fn top(&self) -> f64 {
        self.y
    }

    /// Center X coordinate
    #[inline]
    pub fn center_x(&self) -> f64 {
        self.x + self.width / 2.0
    }

    /// Center Y coordinate
    #[inline]
    pub fn center_y(&self) -> f64 {
        self.y + self.height / 2.0
    }

    /// Check if point is inside rectangle (inclusive)
    #[inline]
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.right() && py >= self.y && py <= self.bottom()
    }

    /// Check if rectangle is empty (zero or negative dimensions)
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// Shrink rectangle by margin on all sides
    #[inline]
    pub fn shrink(&self, margin: f64) -> Self {
        Self {
            x: self.x + margin,
            y: self.y + margin,
            width: (self.width - 2.0 * margin).max(0.0),
            height: (self.height - 2.0 * margin).max(0.0),
        }
    }

    /// Expand rectangle by margin on all sides
    #[inline]
    pub fn expand(&self, margin: f64) -> Self {
        Self {
            x: self.x - margin,
            y: self.y - margin,
            width: self.width + 2.0 * margin,
            height: self.height + 2.0 * margin,
        }
    }

    /// Translate rectangle by offset
    #[inline]
    pub fn translate(&self, dx: f64, dy: f64) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            width: self.width,
            height: self.height,
        }
    }

    /// Intersect with another rectangle
    pub fn intersect(&self, other: &Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        Self {
            x,
            y,
            width: (right - x).max(0.0),
            height: (bottom - y).max(0.0),
        }
    }
}

/// Layout for the chart area (candles + scales)
///
/// This represents the subdivision of the central chart panel:
/// ```text
/// +------------------+-------------+
/// |                  |             |
/// |   chart          | price_scale |
/// |   (candles)      |             |
/// |                  |             |
/// +------------------+-------------+
/// |   time_scale     | scale_corner|
/// +------------------+-------------+
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct ChartAreaLayout {
    /// Main chart area where candles/series are drawn
    pub chart: LayoutRect,
    /// Price scale on the right side
    pub price_scale: LayoutRect,
    /// Time scale at the bottom
    pub time_scale: LayoutRect,
    /// Corner between price and time scales (shows A/M and lin/log/% indicators)
    pub scale_corner: LayoutRect,
}

impl ChartAreaLayout {
    /// Create chart area layout from available space
    ///
    /// # Arguments
    /// * `available` - Rectangle of available space for the entire chart area
    /// * `price_scale_width` - Width of price scale (typically PRICE_SCALE_WIDTH)
    /// * `time_scale_height` - Height of time scale (typically TIME_SCALE_HEIGHT)
    pub fn compute(
        available: LayoutRect,
        price_scale_width: f64,
        time_scale_height: f64,
    ) -> Self {
        // Clamp scale dimensions so they never exceed the leaf's available space.
        // When the leaf is squeezed narrower than price_scale_width (e.g. sidebar
        // separator drag), we must not let the price scale overflow into the
        // neighbouring leaf — clip it to whatever room we have.
        let ps_w = price_scale_width.min(available.width).max(0.0);
        let ts_h = time_scale_height.min(available.height).max(0.0);

        // Chart = available minus price_scale on right and time_scale at bottom
        let chart_width = (available.width - ps_w).max(0.0);
        let chart_height = (available.height - ts_h).max(0.0);

        let chart = LayoutRect::new(
            available.x,
            available.y,
            chart_width,
            chart_height,
        );

        // Price scale = right side, same height as chart
        let price_scale = LayoutRect::new(
            available.x + chart_width,
            available.y,
            ps_w,
            chart_height,
        );

        // Time scale = bottom, same width as chart
        let time_scale = LayoutRect::new(
            available.x,
            available.y + chart_height,
            chart_width,
            ts_h,
        );

        // Scale corner = intersection at bottom-right
        let scale_corner = LayoutRect::new(
            available.x + chart_width,
            available.y + chart_height,
            ps_w,
            ts_h,
        );

        Self {
            chart,
            price_scale,
            time_scale,
            scale_corner,
        }
    }

    /// Compute layout with scale settings (handles positioning)
    pub fn compute_with_settings(available: LayoutRect, settings: &ScaleSettings) -> Self {
        // Clamp to available space so nothing overflows when the leaf is squeezed
        // narrower than the configured scale widths.
        let price_width = settings
            .effective_price_scale_width()
            .min(available.width)
            .max(0.0);
        let time_height = settings
            .effective_time_scale_height()
            .min(available.height)
            .max(0.0);

        // Calculate chart position and dimensions based on scale positions
        let (chart_x, chart_width, price_scale_x) = match settings.price_scale_position {
            PriceScalePosition::Left => {
                // Price scale on left: [price_scale][chart]
                (available.x + price_width, available.width - price_width, available.x)
            }
            PriceScalePosition::Right | PriceScalePosition::Hidden => {
                // Price scale on right or hidden: [chart][price_scale?]
                (available.x, available.width - price_width, available.x + available.width - price_width)
            }
        };

        let (chart_y, chart_height, time_scale_y) = match settings.time_scale_position {
            TimeScalePosition::Top => {
                // Time scale on top
                (available.y + time_height, available.height - time_height, available.y)
            }
            TimeScalePosition::Bottom | TimeScalePosition::Hidden => {
                // Time scale on bottom or hidden
                (available.y, available.height - time_height, available.y + available.height - time_height)
            }
        };

        let chart = LayoutRect::new(chart_x, chart_y, chart_width, chart_height);
        let price_scale = LayoutRect::new(price_scale_x, chart_y, price_width, chart_height);
        let time_scale = LayoutRect::new(chart_x, time_scale_y, chart_width, time_height);

        // Scale corner: intersection of price and time scale areas
        let corner_x = price_scale_x;
        let corner_y = time_scale_y;
        let scale_corner = LayoutRect::new(corner_x, corner_y, price_width, time_height);

        Self {
            chart,
            price_scale,
            time_scale,
            scale_corner,
        }
    }

    /// Hit test - determine what element is at the given coordinates
    ///
    /// Returns a HitZone describing which part of the chart area was hit.
    /// Coordinates are relative to the chart area origin.
    pub fn hit_test(&self, x: f64, y: f64) -> ChartHitZone {
        // Check in order of priority (smaller areas first)
        if self.scale_corner.contains(x, y) {
            return ChartHitZone::ScaleCorner;
        }
        if self.price_scale.contains(x, y) {
            return ChartHitZone::PriceScale;
        }
        if self.time_scale.contains(x, y) {
            return ChartHitZone::TimeScale;
        }
        if self.chart.contains(x, y) {
            return ChartHitZone::Chart;
        }
        ChartHitZone::None
    }

    /// Get chart rect as tuple (x, y, width, height)
    pub fn chart_rect_tuple(&self) -> (f64, f64, f64, f64) {
        (self.chart.x, self.chart.y, self.chart.width, self.chart.height)
    }

    /// Get price scale rect as tuple
    pub fn price_scale_rect_tuple(&self) -> (f64, f64, f64, f64) {
        (self.price_scale.x, self.price_scale.y, self.price_scale.width, self.price_scale.height)
    }

    /// Get time scale rect as tuple
    pub fn time_scale_rect_tuple(&self) -> (f64, f64, f64, f64) {
        (self.time_scale.x, self.time_scale.y, self.time_scale.width, self.time_scale.height)
    }
}

/// Hit zone within chart area - used for input handling
///
/// SubPane is a special case - it's like Chart but with its own Y-axis coordinate system.
/// The X-axis (bar index) is shared with the main chart, but Y-axis (price/value)
/// uses the sub-pane's own price scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ChartHitZone {
    /// Main chart area (candles/series)
    Chart,
    /// Sub-pane chart area (indicator pane with separate Y-axis)
    /// Same X-axis as main chart, but independent Y-axis for indicator values
    SubPane { pane_index: usize },
    /// Sub-pane price scale (Y axis for sub-pane)
    SubPanePriceScale { pane_index: usize },
    /// Price scale (Y axis)
    PriceScale,
    /// Time scale (X axis)
    TimeScale,
    /// Scale corner (A/M, lin/log/% buttons)
    ScaleCorner,
    /// Outside all areas
    #[default]
    None,
}

impl ChartHitZone {
    /// Get the appropriate cursor style for this hit zone
    ///
    /// - Chart area: Crosshair cursor (for precise coordinate tracking)
    /// - SubPane: Crosshair cursor (same as chart, different Y-axis)
    /// - Price scale: Vertical resize arrows (↕) for price scaling
    /// - SubPanePriceScale: Vertical resize arrows (same as price scale)
    /// - Time scale: Horizontal resize arrows (↔) for time scaling
    /// - Scale corner: Default cursor (click-only interaction)
    /// - None: Default cursor
    pub fn cursor(&self) -> CursorStyle {
        match self {
            ChartHitZone::Chart => CursorStyle::Crosshair,
            ChartHitZone::SubPane { .. } => CursorStyle::Crosshair,
            ChartHitZone::PriceScale => CursorStyle::NsResize,
            ChartHitZone::SubPanePriceScale { .. } => CursorStyle::NsResize,
            ChartHitZone::TimeScale => CursorStyle::EwResize,
            ChartHitZone::ScaleCorner => CursorStyle::Default,
            ChartHitZone::None => CursorStyle::Default,
        }
    }

    /// Check if this hit zone is a drawable area (Chart or SubPane)
    pub fn is_drawable(&self) -> bool {
        matches!(self, ChartHitZone::Chart | ChartHitZone::SubPane { .. })
    }

    /// Get sub-pane index if this is a sub-pane hit zone
    pub fn pane_index(&self) -> Option<usize> {
        match self {
            ChartHitZone::SubPane { pane_index } => Some(*pane_index),
            ChartHitZone::SubPanePriceScale { pane_index } => Some(*pane_index),
            _ => None,
        }
    }
}

/// Frame layout for chart rendering
///
/// This represents the layout of the chart within its available space.
/// External UI elements (toolbars, sidebars) are handled by the terminal -
/// the chart only needs to know about margins.
///
/// ```text
/// +-----------------------------------------------+
/// |                   margins.top                 |
/// +--------+----------------------------+---------+
/// |        |                            |         |
/// |margins |      chart_panel           |margins  |
/// | .left  |   +----------------+-----+ | .right  |
/// |        |   |  chart_area    |price| |         |
/// |        |   |                |scale| |         |
/// |        |   +----------------+-----+ |         |
/// |        |   |  time_scale    |corner|         |
/// +--------+----------------------------+---------+
/// |                 margins.bottom                |
/// +-----------------------------------------------+
/// ```
#[derive(Clone, Debug, Default)]
pub struct FrameLayout {
    /// Total frame rectangle (entire window)
    pub total: LayoutRect,
    /// Central chart area (subdivided into chart + scales)
    pub chart_area: ChartAreaLayout,
    /// Combined area for all chart elements (chart + scales)
    pub chart_panel: LayoutRect,
}

/// Sub-pane layout for indicator panes below main chart
#[derive(Clone, Copy, Debug, Default)]
pub struct SubPaneLayout {
    /// Instance ID of the indicator in this pane
    pub instance_id: u64,
    /// Separator bar above this pane
    pub separator: LayoutRect,
    /// Content area of the pane
    pub content: LayoutRect,
    /// Price scale for this pane
    pub price_scale: LayoutRect,
}

/// Extended frame layout including sub-panes
#[derive(Clone, Debug, Default)]
pub struct ExtendedFrameLayout {
    /// Base frame layout
    pub frame: FrameLayout,
    /// Main chart area (candles) - adjusted for sub-panes
    pub main_chart: ChartAreaLayout,
    /// Sub-pane layouts
    pub sub_panes: Vec<SubPaneLayout>,
    /// Total height of main chart + all sub-panes (excluding time scale)
    pub total_chart_height: f64,
}

impl ExtendedFrameLayout {
    /// Compute extended layout from a chart panel rect and a container's
    /// inner uzor docking tree.
    ///
    /// The inner tree's root branch holds children in vertical SplitBottom
    /// order: `[MainChart, IndicatorPanel_1, IndicatorPanel_2, ...]`. The
    /// branch's `proportions` Vec is the single source of truth for how
    /// vertical space is split between the chart canvas and its sub-panes.
    ///
    /// This replaces the legacy `compute_from_chart_panel(... sub_pane_heights,
    /// above_main_flags, maximized_instance_id ...)`. Above-main panes,
    /// per-pane height_ratio, and the maximize overlay are no longer
    /// concepts in the chart-container model -- they roll into uzor leaves
    /// (`SplitTop` for above-main when reintroduced) and container-level
    /// flags (Phase 3 of the chart-container cut).
    ///
    /// # Arguments
    /// * `chart_panel` -- the available chart panel rect (already excludes
    ///   toolbars/sidebars).
    /// * `inner_docking` -- the container's inner uzor docking tree.
    /// * `scale_settings` -- scale positioning and dimensions.
    /// * `separator_height` -- pixel height of the horizontal separator
    ///   between stacked panes (typically 1).
    pub fn from_inner_docking<P: uzor::panels::DockPanel>(
        chart_panel: &LayoutRect,
        inner_docking: &uzor::layout::DockState<P>,
        scale_settings: &ScaleSettings,
        separator_height: f64,
    ) -> Self {
        // Extract sub-pane instance ids in tree order (skipping the main
        // chart leaf at index 0).
        let root = inner_docking.tree().root();
        let sub_pane_instance_ids: Vec<u64> = root
            .children
            .iter()
            .skip(1)
            .filter_map(|c| match c {
                uzor::panels::PanelNode::Leaf(l) => l
                    .panels
                    .first()
                    .and_then(|p| {
                        // We don't know the concrete payload type here; use
                        // type_id as a heuristic. The chart-container leaf
                        // uses "container_indicator_panel" for sub-panes and
                        // exposes nothing else publicly, so any leaf past
                        // index 0 is an indicator. Use leaf id as fallback
                        // identifier when payload doesn't expose instance.
                        let _ = p;
                        Some(l.id.0)
                    }),
                _ => None,
            })
            .collect();

        // Proportions parallel to children (main first, then sub-panes).
        let proportions: &[f64] = &root.proportions;
        let sub_proportions: Vec<f64> = if proportions.len() >= 1 {
            proportions.iter().skip(1).copied().collect()
        } else {
            Vec::new()
        };

        Self::from_proportions(
            chart_panel,
            &sub_pane_instance_ids,
            &sub_proportions,
            scale_settings,
            separator_height,
        )
    }

    /// Same as `from_inner_docking` but takes pre-extracted instance ids and
    /// proportions directly. Used when the caller already has the
    /// `ChartContainer` handle and can pull instance ids out of the leaf
    /// payload typed as `ChartContainerLeaf`.
    pub fn from_proportions(
        chart_panel: &LayoutRect,
        sub_pane_instance_ids: &[u64],
        sub_proportions: &[f64],
        scale_settings: &ScaleSettings,
        separator_height: f64,
    ) -> Self {
        // Clamp scale dimensions to the chart_panel bounds so nothing can
        // overflow into neighbouring leaves when the leaf is squeezed below
        // the configured scale size (sidebar separator drag, narrow splits).
        // Mirrors the guard in ChartAreaLayout::compute / compute_with_settings.
        let price_scale_width = scale_settings
            .effective_price_scale_width()
            .min(chart_panel.width)
            .max(0.0);
        let time_scale_height = scale_settings
            .effective_time_scale_height()
            .min(chart_panel.height)
            .max(0.0);
        let chart_width = (chart_panel.width - price_scale_width).max(0.0);
        let sub_pane_count = sub_pane_instance_ids.len();

        if sub_pane_count == 0 {
            // No sub-panes - return simple layout
            let chart_area = ChartAreaLayout::compute_with_settings(*chart_panel, scale_settings);
            return Self {
                frame: FrameLayout::default(),
                main_chart: chart_area,
                sub_panes: Vec::new(),
                total_chart_height: chart_area.chart.height,
            };
        }

        let available_height = chart_panel.height - time_scale_height;

        // Build a local InnerLayout from the caller-supplied proportions
        // and let it own the heights math (per-slot min enforcement +
        // separator budget). Single source of truth for vertical layout.
        let mut local = InnerLayout::new();
        local.separator_px = separator_height;
        let want_ids: Vec<u64> = sub_pane_instance_ids
            .iter()
            .copied()
            .take(sub_pane_count)
            .collect();
        local.sync_with(&want_ids);
        // Apply the caller's share Vec verbatim (skip the main slot --
        // InnerLayout already keeps it at proportions[0]).
        if sub_proportions.len() >= sub_pane_count {
            local.proportions = std::iter::once(local.proportions[0])
                .chain(sub_proportions.iter().take(sub_pane_count).copied())
                .collect();
            local.normalize();
        }
        let heights = local.heights(available_height);
        let main_chart_height = heights.first().copied().unwrap_or(available_height);
        let scaled_heights: Vec<f64> = heights.iter().skip(1).copied().collect();
        let separator_budget = separator_height * sub_pane_count as f64;
        let actual_available_for_subs: f64 =
            scaled_heights.iter().sum::<f64>() + separator_budget;

        // Calculate chart and scale positions based on scale settings
        let (chart_x, price_scale_x) = match scale_settings.price_scale_position {
            PriceScalePosition::Left => {
                // Price scale on left: [price_scale][chart]
                (chart_panel.x + price_scale_width, chart_panel.x)
            }
            PriceScalePosition::Right | PriceScalePosition::Hidden => {
                // Price scale on right or hidden: [chart][price_scale?]
                (chart_panel.x, chart_panel.x + chart_width)
            }
        };

        let (chart_y, time_scale_y) = match scale_settings.time_scale_position {
            TimeScalePosition::Top => {
                // Time scale on top: [time_scale][chart][below_panes]
                (
                    chart_panel.y + time_scale_height,
                    chart_panel.y,
                )
            }
            TimeScalePosition::Bottom | TimeScalePosition::Hidden => {
                // Time scale on bottom or hidden: [chart][below_panes][time_scale?]
                (
                    chart_panel.y,
                    chart_panel.y + main_chart_height + actual_available_for_subs,
                )
            }
        };

        // Compute main chart area (reduced height) with proper positioning
        let main_chart = ChartAreaLayout {
            chart: LayoutRect::new(
                chart_x,
                chart_y,
                chart_width,
                main_chart_height,
            ),
            price_scale: LayoutRect::new(
                price_scale_x,
                chart_y,
                price_scale_width,
                main_chart_height,
            ),
            time_scale: LayoutRect::new(
                chart_x,
                time_scale_y,
                chart_width,
                time_scale_height,
            ),
            scale_corner: LayoutRect::new(
                price_scale_x,
                time_scale_y,
                price_scale_width,
                time_scale_height,
            ),
        };

        // Total chart height for positioning
        let total_chart_height = main_chart_height + actual_available_for_subs;

        // Compute sub-pane layouts. All sub-panes go below the main chart
        // in inner-docking tree order. Above-main is no longer supported
        // (Phase 2 of the chart-container cut -- reintroduce later via
        // uzor SplitTop if the feature is needed).
        let mut sub_panes = Vec::with_capacity(sub_pane_count);
        let mut below_y = chart_y + main_chart_height;

        for (i, &instance_id) in sub_pane_instance_ids.iter().enumerate() {
            let pane_h = scaled_heights[i];

            let separator = LayoutRect::new(
                chart_panel.x,
                below_y,
                chart_panel.width,
                separator_height,
            );
            below_y += separator_height;

            let content = LayoutRect::new(chart_x, below_y, chart_width, pane_h);
            let pane_price_scale = LayoutRect::new(
                price_scale_x,
                below_y,
                price_scale_width,
                pane_h,
            );

            sub_panes.push(SubPaneLayout {
                instance_id,
                separator,
                content,
                price_scale: pane_price_scale,
            });

            below_y += pane_h;
        }

        Self {
            frame: FrameLayout::default(),
            main_chart,
            sub_panes,
            total_chart_height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_rect_basics() {
        let rect = LayoutRect::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(rect.right(), 110.0);
        assert_eq!(rect.bottom(), 70.0);
        assert_eq!(rect.center_x(), 60.0);
        assert_eq!(rect.center_y(), 45.0);
    }

    #[test]
    fn test_layout_rect_contains() {
        let rect = LayoutRect::new(0.0, 0.0, 100.0, 100.0);
        assert!(rect.contains(50.0, 50.0));
        assert!(rect.contains(0.0, 0.0));
        assert!(rect.contains(100.0, 100.0));
        assert!(!rect.contains(-1.0, 50.0));
        assert!(!rect.contains(101.0, 50.0));
    }

    #[test]
    fn test_layout_rect_shrink() {
        let rect = LayoutRect::new(0.0, 0.0, 100.0, 100.0);
        let shrunk = rect.shrink(10.0);
        assert_eq!(shrunk.x, 10.0);
        assert_eq!(shrunk.y, 10.0);
        assert_eq!(shrunk.width, 80.0);
        assert_eq!(shrunk.height, 80.0);
    }

    #[test]
    fn test_chart_area_layout() {
        let available = LayoutRect::new(0.0, 0.0, 800.0, 600.0);
        let layout = ChartAreaLayout::compute(available, 70.0, 30.0);

        assert_eq!(layout.chart.width, 730.0);
        assert_eq!(layout.chart.height, 570.0);
        assert_eq!(layout.price_scale.x, 730.0);
        assert_eq!(layout.price_scale.width, 70.0);
        assert_eq!(layout.time_scale.y, 570.0);
        assert_eq!(layout.time_scale.height, 30.0);
        assert_eq!(layout.scale_corner.x, 730.0);
        assert_eq!(layout.scale_corner.y, 570.0);
    }
}
