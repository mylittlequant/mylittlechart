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
    /// order: `[MainChart, SubPane_1, SubPane_2, ...]`. The
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
            &std::collections::HashSet::new(),
        )
    }

    /// Same as `from_inner_docking` but takes pre-extracted instance ids and
    /// proportions directly. Used when the caller already has the
    /// `ChartContainer` handle and can pull instance ids out of the leaf
    /// payload typed as `ChartContainerLeaf`.
    ///
    /// `above_main_ids` — instance ids that should be placed ABOVE the main
    /// chart. Pass an empty set (or `&HashSet::new()`) to get the old
    /// all-below behaviour. The inner-docking tree order (and thus
    /// `sub_pane_instance_ids` / `sub_proportions`) is not changed; only the
    /// presentation geometry is reordered.
    pub fn from_proportions(
        chart_panel: &LayoutRect,
        sub_pane_instance_ids: &[u64],
        sub_proportions: &[f64],
        scale_settings: &ScaleSettings,
        separator_height: f64,
        above_main_ids: &std::collections::HashSet<u64>,
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

        // Partition sub-panes into above-main and below-main groups.
        // "Above" group order = top-to-bottom on screen (index 0 is topmost).
        // The inner-docking tree stores all panes in below order; above-main is a
        // presentation-only reordering applied here at geometry time.
        let is_above: Vec<bool> = sub_pane_instance_ids
            .iter()
            .map(|id| above_main_ids.contains(id))
            .collect();
        let above_total_h: f64 = is_above.iter().zip(scaled_heights.iter())
            .filter(|(&a, _)| a)
            .map(|(_, &h)| h + separator_height)
            .sum();

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

        // above_total_h is the pixel height consumed by above-main panes.
        // Layout order: [time_scale_top?][above_panes][main_chart][below_panes][time_scale_bottom?]
        let (chart_y, time_scale_y) = match scale_settings.time_scale_position {
            TimeScalePosition::Top => {
                // Time scale on top: [time_scale][above_panes][chart][below_panes]
                (
                    chart_panel.y + time_scale_height + above_total_h,
                    chart_panel.y,
                )
            }
            TimeScalePosition::Bottom | TimeScalePosition::Hidden => {
                // Time scale on bottom or hidden: [above_panes][chart][below_panes][time_scale?]
                (
                    chart_panel.y + above_total_h,
                    chart_panel.y + above_total_h + main_chart_height + actual_available_for_subs - above_total_h,
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

        // Total chart height for positioning (main + all sub-panes)
        let total_chart_height = main_chart_height + actual_available_for_subs;

        // Compute sub-pane layouts.
        // Above-main panes use a y-cursor that starts above the main chart.
        // Below-main panes use a y-cursor that starts below the main chart.
        // The starting y for above panes depends on scale position.
        let above_start_y = match scale_settings.time_scale_position {
            TimeScalePosition::Top => chart_panel.y + time_scale_height,
            TimeScalePosition::Bottom | TimeScalePosition::Hidden => chart_panel.y,
        };
        let mut above_y = above_start_y;
        let mut below_y = chart_y + main_chart_height;

        let mut sub_panes = Vec::with_capacity(sub_pane_count);

        for (i, &instance_id) in sub_pane_instance_ids.iter().enumerate() {
            let pane_h = scaled_heights[i];
            let pane_is_above = is_above[i];

            let (cur_y, next_y) = if pane_is_above {
                let y = above_y;
                above_y += separator_height + pane_h;
                (y, y + separator_height)
            } else {
                let y = below_y;
                below_y += separator_height + pane_h;
                (y, y + separator_height)
            };

            let separator = LayoutRect::new(chart_panel.x, cur_y, chart_panel.width, separator_height);
            let content = LayoutRect::new(chart_x, next_y, chart_width, pane_h);
            let pane_price_scale = LayoutRect::new(price_scale_x, next_y, price_scale_width, pane_h);

            sub_panes.push(SubPaneLayout {
                instance_id,
                separator,
                content,
                price_scale: pane_price_scale,
            });
        }

        Self {
            frame: FrameLayout::default(),
            main_chart,
            sub_panes,
            total_chart_height,
        }
    }
}

/// Margins around the chart area
///
/// These represent space consumed by external UI elements (toolbars, sidebars, etc.).
/// The chart library doesn't know or care what those elements are - it just knows
/// how much space they take.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Margins {
    /// Space consumed at top (e.g., toolbar)
    pub top: f64,
    /// Space consumed at left (e.g., drawing toolbar)
    pub left: f64,
    /// Space consumed at right (e.g., sidebar)
    pub right: f64,
    /// Space consumed at bottom (e.g., status bar, trading panel)
    pub bottom: f64,
}

impl Margins {
    /// Create new margins
    pub const fn new(top: f64, left: f64, right: f64, bottom: f64) -> Self {
        Self { top, left, right, bottom }
    }

    /// Create zero margins (chart takes full window)
    pub const fn zero() -> Self {
        Self { top: 0.0, left: 0.0, right: 0.0, bottom: 0.0 }
    }

    /// Create uniform margins
    pub const fn uniform(margin: f64) -> Self {
        Self { top: margin, left: margin, right: margin, bottom: margin }
    }

    /// Total horizontal margin (left + right)
    pub fn horizontal(&self) -> f64 {
        self.left + self.right
    }

    /// Total vertical margin (top + bottom)
    pub fn vertical(&self) -> f64 {
        self.top + self.bottom
    }
}

/// Configuration for layout computation
#[derive(Clone, Debug)]
pub struct LayoutConfig {
    /// Width of price scale
    pub price_scale_width: f64,
    /// Height of time scale
    pub time_scale_height: f64,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            price_scale_width: crate::types::PRICE_SCALE_WIDTH,
            time_scale_height: crate::types::TIME_SCALE_HEIGHT,
        }
    }
}

impl FrameLayout {
    /// Compute frame layout from total window size and margins
    ///
    /// The margins represent space consumed by external UI (toolbars, sidebars).
    /// The chart panel is the remaining space after margins are applied.
    ///
    /// # Arguments
    /// * `total_width` - Total window/frame width
    /// * `total_height` - Total window/frame height
    /// * `margins` - Space consumed by external UI elements
    pub fn compute(total_width: f64, total_height: f64, margins: &Margins) -> Self {
        Self::compute_with_config(total_width, total_height, margins, &LayoutConfig::default())
    }

    /// Compute layout with custom configuration
    pub fn compute_with_config(
        total_width: f64,
        total_height: f64,
        margins: &Margins,
        config: &LayoutConfig,
    ) -> Self {
        let total = LayoutRect::new(0.0, 0.0, total_width, total_height);

        // Chart panel is the space after margins are applied
        let chart_panel = LayoutRect::new(
            margins.left,
            margins.top,
            (total_width - margins.horizontal()).max(0.0),
            (total_height - margins.vertical()).max(0.0),
        );

        // Subdivide chart panel into chart + scales
        let chart_area = ChartAreaLayout::compute(
            chart_panel,
            config.price_scale_width,
            config.time_scale_height,
        );

        Self {
            total,
            chart_area,
            chart_panel,
        }
    }

    /// Compute from just the chart panel rect (no margins)
    ///
    /// Use this when you already know the available chart panel rect.
    pub fn from_chart_panel(chart_panel: LayoutRect) -> Self {
        Self::from_chart_panel_with_config(chart_panel, &LayoutConfig::default())
    }

    /// Compute from chart panel with custom config
    pub fn from_chart_panel_with_config(chart_panel: LayoutRect, config: &LayoutConfig) -> Self {
        let chart_area = ChartAreaLayout::compute(
            chart_panel,
            config.price_scale_width,
            config.time_scale_height,
        );

        Self {
            total: chart_panel, // Total equals chart_panel when no margins
            chart_area,
            chart_panel,
        }
    }

    /// Get the chart rect (main drawing area for candles)
    #[inline]
    pub fn chart_rect(&self) -> LayoutRect {
        self.chart_area.chart
    }

    /// Get price scale rect
    #[inline]
    pub fn price_scale_rect(&self) -> LayoutRect {
        self.chart_area.price_scale
    }

    /// Get time scale rect
    #[inline]
    pub fn time_scale_rect(&self) -> LayoutRect {
        self.chart_area.time_scale
    }

    /// Get scale corner rect
    #[inline]
    pub fn scale_corner_rect(&self) -> LayoutRect {
        self.chart_area.scale_corner
    }

    /// Check if a point is in the chart area (excluding scales)
    pub fn point_in_chart(&self, x: f64, y: f64) -> bool {
        self.chart_area.chart.contains(x, y)
    }

    /// Check if a point is in the price scale
    pub fn point_in_price_scale(&self, x: f64, y: f64) -> bool {
        self.chart_area.price_scale.contains(x, y)
    }

    /// Check if a point is in the time scale
    pub fn point_in_time_scale(&self, x: f64, y: f64) -> bool {
        self.chart_area.time_scale.contains(x, y)
    }

    /// Check if a point is in the scale corner
    pub fn point_in_scale_corner(&self, x: f64, y: f64) -> bool {
        self.chart_area.scale_corner.contains(x, y)
    }
}

impl ExtendedFrameLayout {
    /// Find which sub-pane (if any) contains the given Y coordinate
    /// Returns (pane_index, local_y within pane)
    pub fn find_sub_pane_at_y(&self, y: f64) -> Option<(usize, f64)> {
        for (idx, pane) in self.sub_panes.iter().enumerate() {
            if pane.content.contains(pane.content.x, y) {
                let local_y = y - pane.content.y;
                return Some((idx, local_y));
            }
        }
        None
    }

    /// Check if Y coordinate is on a separator between panes.
    /// Returns the instance ID of the sub-pane whose separator was hit.  The
    /// hit zone is expanded to ±6 px around the separator centre so a 1-px
    /// separator line is reliably clickable.  Zero-height separators
    /// (maximized overlay) are skipped.
    pub fn find_separator_at_y(&self, y: f64) -> Option<u64> {
        const HIT_TOLERANCE: f64 = 6.0;
        for pane in &self.sub_panes {
            // Maximized panes have separator.height == 0 — no interactive separator.
            if pane.separator.height == 0.0 {
                continue;
            }
            let sep_center = pane.separator.y + pane.separator.height * 0.5;
            if (y - sep_center).abs() <= HIT_TOLERANCE {
                return Some(pane.instance_id);
            }
        }
        None
    }

    /// Get sub-pane layout by instance ID
    pub fn get_sub_pane_by_id(&self, instance_id: u64) -> Option<&SubPaneLayout> {
        self.sub_panes.iter().find(|p| p.instance_id == instance_id)
    }
}

/// Layout manager - convenience wrapper for computing layouts
///
/// This struct holds configuration and provides methods for computing layouts.
/// Use this when you need to compute layouts multiple times with the same config.
#[derive(Clone, Debug, Default)]
pub struct LayoutManager {
    config: LayoutConfig,
}

impl LayoutManager {
    /// Create a new layout manager with default configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with custom configuration
    pub fn with_config(config: LayoutConfig) -> Self {
        Self { config }
    }

    /// Get current configuration
    pub fn config(&self) -> &LayoutConfig {
        &self.config
    }

    /// Set configuration
    pub fn set_config(&mut self, config: LayoutConfig) {
        self.config = config;
    }

    /// Compute frame layout for given dimensions and margins
    pub fn compute_frame(
        &self,
        total_width: f64,
        total_height: f64,
        margins: &Margins,
    ) -> FrameLayout {
        FrameLayout::compute_with_config(total_width, total_height, margins, &self.config)
    }

    /// Compute extended frame layout with sub-panes
    pub fn compute_extended(
        &self,
        total_width: f64,
        total_height: f64,
        margins: &Margins,
        sub_pane_instance_ids: &[u64],
        sub_pane_height: f64,
        separator_height: f64,
    ) -> ExtendedFrameLayout {
        ExtendedFrameLayout::compute_with_config(
            total_width,
            total_height,
            margins,
            sub_pane_instance_ids,
            sub_pane_height,
            separator_height,
            &self.config,
        )
    }

    /// Compute only chart area from panel dimensions
    pub fn compute_chart_area(&self, panel_width: f64, panel_height: f64) -> ChartAreaLayout {
        ChartAreaLayout::compute(
            LayoutRect::new(0.0, 0.0, panel_width, panel_height),
            self.config.price_scale_width,
            self.config.time_scale_height,
        )
    }
}

impl ExtendedFrameLayout {
    /// Compute extended layout with sub-panes for indicators
    ///
    /// # Arguments
    /// * `total_width` - Total window width
    /// * `total_height` - Total window height
    /// * `margins` - Space consumed by external UI elements
    /// * `sub_pane_instance_ids` - Instance IDs of indicators that need sub-panes
    /// * `sub_pane_height` - Height of each sub-pane (typically 100.0)
    /// * `separator_height` - Height of separator between panes (typically 4.0)
    pub fn compute(
        total_width: f64,
        total_height: f64,
        margins: &Margins,
        sub_pane_instance_ids: &[u64],
        sub_pane_height: f64,
        separator_height: f64,
    ) -> Self {
        Self::compute_with_config(
            total_width,
            total_height,
            margins,
            sub_pane_instance_ids,
            sub_pane_height,
            separator_height,
            &LayoutConfig::default(),
        )
    }

    /// Compute with custom configuration
    pub fn compute_with_config(
        total_width: f64,
        total_height: f64,
        margins: &Margins,
        sub_pane_instance_ids: &[u64],
        sub_pane_height: f64,
        separator_height: f64,
        config: &LayoutConfig,
    ) -> Self {
        // First compute base frame layout
        let frame = FrameLayout::compute_with_config(total_width, total_height, margins, config);

        let sub_pane_count = sub_pane_instance_ids.len();

        if sub_pane_count == 0 {
            // No sub-panes - main chart gets entire chart_area
            return Self {
                main_chart: frame.chart_area,
                sub_panes: Vec::new(),
                total_chart_height: frame.chart_area.chart.height,
                frame,
            };
        }

        // Calculate total height needed for sub-panes
        // Each sub-pane has: separator + content
        let total_sub_panes_height = (sub_pane_height + separator_height) * sub_pane_count as f64;

        // Main chart height is reduced by sub-panes total
        let min_main_chart = if sub_pane_count > 0 {
            let reserved = (separator_height + 20.0) * sub_pane_count as f64;
            (frame.chart_area.chart.height - reserved).clamp(50.0, 200.0)
        } else {
            200.0_f64.min(frame.chart_area.chart.height)
        };
        let main_chart_height = (frame.chart_area.chart.height - total_sub_panes_height).max(min_main_chart);

        // Recalculate actual sub-pane height if main chart was clamped
        let actual_available_for_sub = frame.chart_area.chart.height - main_chart_height;
        let actual_pane_height = if sub_pane_count > 0 {
            (actual_available_for_sub - separator_height * sub_pane_count as f64) / sub_pane_count as f64
        } else {
            0.0
        };

        // Compute main chart area (reduced height)
        let main_chart = ChartAreaLayout {
            chart: LayoutRect::new(
                frame.chart_area.chart.x,
                frame.chart_area.chart.y,
                frame.chart_area.chart.width,
                main_chart_height,
            ),
            price_scale: LayoutRect::new(
                frame.chart_area.price_scale.x,
                frame.chart_area.price_scale.y,
                frame.chart_area.price_scale.width,
                main_chart_height,
            ),
            // Time scale and scale_corner remain at their original positions
            // (at the bottom of the full chart area)
            time_scale: frame.chart_area.time_scale,
            scale_corner: frame.chart_area.scale_corner,
        };

        // Total height for positioning time scale
        let total_chart_height = main_chart_height + actual_available_for_sub;

        // Compute sub-pane layouts
        let mut sub_panes = Vec::with_capacity(sub_pane_count);
        let mut current_y = frame.chart_area.chart.y + main_chart_height;

        for &instance_id in sub_pane_instance_ids {
            // Separator
            let separator = LayoutRect::new(
                frame.chart_area.chart.x,
                current_y,
                frame.chart_panel.width, // Full width including price scale
                separator_height,
            );
            current_y += separator_height;

            // Content area
            let content = LayoutRect::new(
                frame.chart_area.chart.x,
                current_y,
                frame.chart_area.chart.width, // Same width as main chart
                actual_pane_height,
            );

            // Price scale for this pane
            let price_scale = LayoutRect::new(
                frame.chart_area.price_scale.x,
                current_y,
                frame.chart_area.price_scale.width,
                actual_pane_height,
            );

            sub_panes.push(SubPaneLayout {
                instance_id,
                separator,
                content,
                price_scale,
            });

            current_y += actual_pane_height;
        }

        Self {
            frame,
            main_chart,
            sub_panes,
            total_chart_height,
        }
    }
}

#[cfg(test)]
mod compute_tests {
    use super::*;

    #[test]
    fn test_frame_layout_no_margins() {
        let margins = Margins::zero();
        let layout = FrameLayout::compute(1920.0, 1080.0, &margins);

        // With zero margins, chart panel should be full window
        assert_eq!(layout.chart_panel.x, 0.0);
        assert_eq!(layout.chart_panel.y, 0.0);
        assert_eq!(layout.chart_panel.width, 1920.0);
        assert_eq!(layout.chart_panel.height, 1080.0);
    }

    #[test]
    fn test_frame_layout_with_margins() {
        let margins = Margins::new(40.0, 50.0, 60.0, 30.0); // top, left, right, bottom
        let layout = FrameLayout::compute(1920.0, 1080.0, &margins);

        // Chart panel should be offset by margins
        assert_eq!(layout.chart_panel.x, 50.0); // left margin
        assert_eq!(layout.chart_panel.y, 40.0); // top margin
        assert_eq!(layout.chart_panel.width, 1920.0 - 50.0 - 60.0); // width - left - right
        assert_eq!(layout.chart_panel.height, 1080.0 - 40.0 - 30.0); // height - top - bottom
    }

    #[test]
    fn test_chart_area_subdivision() {
        let margins = Margins::zero();
        let layout = FrameLayout::compute(1000.0, 800.0, &margins);

        // Chart + price_scale should equal panel width
        let chart_width = layout.chart_area.chart.width;
        let price_scale_width = layout.chart_area.price_scale.width;
        assert!((chart_width + price_scale_width - layout.chart_panel.width).abs() < 0.001);

        // Chart + time_scale should equal panel height
        let chart_height = layout.chart_area.chart.height;
        let time_scale_height = layout.chart_area.time_scale.height;
        assert!((chart_height + time_scale_height - layout.chart_panel.height).abs() < 0.001);

        // Scale corner should be at intersection
        assert_eq!(layout.chart_area.scale_corner.x, layout.chart_area.price_scale.x);
        assert_eq!(layout.chart_area.scale_corner.y, layout.chart_area.time_scale.y);
    }

    #[test]
    fn test_extended_layout_with_sub_panes() {
        let margins = Margins::zero();
        let sub_pane_ids = vec![1, 2];
        let layout = ExtendedFrameLayout::compute(
            1920.0, 1080.0, &margins, &sub_pane_ids, 100.0, 4.0,
        );

        // Should have 2 sub-panes
        assert_eq!(layout.sub_panes.len(), 2);

        // Main chart height should be reduced
        let main_chart_height = layout.main_chart.chart.height;
        assert!(main_chart_height < layout.frame.chart_area.chart.height);

        // First sub-pane separator should start at main chart bottom
        let main_chart_bottom = layout.main_chart.chart.bottom();
        assert!((layout.sub_panes[0].separator.y - main_chart_bottom).abs() < 0.001);

        // Second sub-pane separator should be after first pane content (or equal due to rounding)
        assert!(layout.sub_panes[1].separator.y >= layout.sub_panes[0].content.bottom() - 0.001);
    }

    #[test]
    fn test_hit_testing() {
        let margins = Margins::zero();
        let layout = FrameLayout::compute(1000.0, 800.0, &margins);

        // Chart center should be in chart
        let chart_center_x = layout.chart_area.chart.center_x();
        let chart_center_y = layout.chart_area.chart.center_y();
        assert!(layout.point_in_chart(chart_center_x, chart_center_y));

        // Price scale should be detected
        let price_scale_center_x = layout.chart_area.price_scale.center_x();
        let price_scale_center_y = layout.chart_area.price_scale.center_y();
        assert!(layout.point_in_price_scale(price_scale_center_x, price_scale_center_y));
        assert!(!layout.point_in_chart(price_scale_center_x, price_scale_center_y));
    }

    #[test]
    fn test_margins_helpers() {
        let margins = Margins::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(margins.horizontal(), 50.0); // left + right
        assert_eq!(margins.vertical(), 50.0); // top + bottom

        let uniform = Margins::uniform(15.0);
        assert_eq!(uniform.top, 15.0);
        assert_eq!(uniform.left, 15.0);
        assert_eq!(uniform.right, 15.0);
        assert_eq!(uniform.bottom, 15.0);
    }

    #[test]
    fn test_from_chart_panel() {
        let panel = LayoutRect::new(100.0, 50.0, 800.0, 600.0);
        let layout = FrameLayout::from_chart_panel(panel);

        assert_eq!(layout.chart_panel.x, 100.0);
        assert_eq!(layout.chart_panel.y, 50.0);
        assert_eq!(layout.chart_panel.width, 800.0);
        assert_eq!(layout.chart_panel.height, 600.0);
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
