//! Regression Trend primitive
//!
//! A linear regression channel: the centre line is a least-squares fit of
//! `source` price against bar index, over the bars inside the span the
//! analyst dragged (or, in [`RegressionPeriodMode::BarCount`], a fixed
//! lookback ending at the later anchor) — see [`RegressionTrend::fit`], the
//! ONE place this computation happens. The two sigma bands are drawn at
//! `std_dev_mult` standard deviations of the fit's own residuals, not a
//! fixed price offset — see [`RegressionFit::residual_std_dev`].
//!
//! Before this pass the "fit" was a straight segment between the two
//! dragged points plus a made-up offset (`(price_range * 0.15).max(...)`,
//! see git history) — never handed the bars, so nothing about it was
//! computed from price data despite the name. `render`/`hit_test` both
//! read [`RegressionTrend::fit`] for their geometry, so the drawn line and
//! the hit-testable line can never disagree.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::i18n::{ConceptKey, ModalKey, SettingsKey, current_language};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, Archetype, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    point_to_line_distance, HIT_TOLERANCE, CONTROL_POINT_RADIUS, CONTROL_POINT_HIT_RADIUS,
    extend_segment_to_edges,
    RenderContext, crisp, fmt_price,
    render_primitive_text_rotated, calculate_line_text_params,
    config::{ConfigProperty, PropertyValue, PropertyCategory, PropertyGroup, SelectOption, StatRow, REGRESSION_RESULT_STATS_ID},
    AREA_FILL_OPACITY, default_area_fill_opacity,
};

/// The price series [`RegressionTrend::fit`] least-squares-regresses
/// against bar index. Same seven-value vocabulary
/// `mlc-backend-indicator-engine`'s own
/// `IndicatorParamType::source_options()` gives an indicator's own source
/// picker (`managers/indicator_manager.rs`: `&["close", "open", "high",
/// "low", "hl2", "hlc3", "ohlc4"]`) — reproduced here rather than imported:
/// `mlc-core` sits at the bottom of the crate graph (types → engines →
/// backend → app → shells, this repo's `CLAUDE.md` crate-contract table),
/// and that indicator crate is one of the engines above it, so a
/// dependency the other way does not exist. The wire strings match byte
/// for byte, so a value round-trips identically through either vocabulary.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum RegressionSource {
    #[default]
    Close,
    Open,
    High,
    Low,
    Hl2,
    Hlc3,
    Ohlc4,
}

impl RegressionSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Close => "close",
            Self::Open => "open",
            Self::High => "high",
            Self::Low => "low",
            Self::Hl2 => "hl2",
            Self::Hlc3 => "hlc3",
            Self::Ohlc4 => "ohlc4",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "open" => Self::Open,
            "high" => Self::High,
            "low" => Self::Low,
            "hl2" => Self::Hl2,
            "hlc3" => Self::Hlc3,
            "ohlc4" => Self::Ohlc4,
            _ => Self::Close,
        }
    }

    /// This source's own price for one bar — `hl2`/`hlc3`/`ohlc4` average
    /// the same OHLC fields the identically-named indicator options do.
    pub fn price(&self, bar: &Bar) -> f64 {
        match self {
            Self::Close => bar.close,
            Self::Open => bar.open,
            Self::High => bar.high,
            Self::Low => bar.low,
            Self::Hl2 => (bar.high + bar.low) / 2.0,
            Self::Hlc3 => (bar.high + bar.low + bar.close) / 3.0,
            Self::Ohlc4 => (bar.open + bar.high + bar.low + bar.close) / 4.0,
        }
    }

    fn select_options() -> Vec<SelectOption> {
        let lang = current_language();
        vec![
            SelectOption::new(Self::Close.as_str(), ConceptKey::SourceClose.get(lang)),
            SelectOption::new(Self::Open.as_str(), ConceptKey::SourceOpen.get(lang)),
            SelectOption::new(Self::High.as_str(), ConceptKey::SourceHigh.get(lang)),
            SelectOption::new(Self::Low.as_str(), ConceptKey::SourceLow.get(lang)),
            SelectOption::new(Self::Hl2.as_str(), ConceptKey::SourceHl2.get(lang)),
            SelectOption::new(Self::Hlc3.as_str(), ConceptKey::SourceHlc3.get(lang)),
            SelectOption::new(Self::Ohlc4.as_str(), ConceptKey::SourceOhlc4.get(lang)),
        ]
    }
}

/// How [`RegressionTrend::fit`] chooses its own bar WINDOW — the design's
/// "Period" dropdown (`regression_tabs()`'s Calculation group,
/// `ConceptKey::Period` / `ConceptKey::ByPoints`).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum RegressionPeriodMode {
    /// Fit over every bar between the two dragged anchors — dragging IS
    /// the period control. Matches the design's own default value.
    #[default]
    Points,
    /// Fit over the last `bar_count` bars ending at whichever anchor sits
    /// later (by bar index), ignoring the earlier anchor's own position.
    BarCount,
}

impl RegressionPeriodMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Points => "points",
            Self::BarCount => "bar_count",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "bar_count" => Self::BarCount,
            _ => Self::Points,
        }
    }

    fn select_options() -> Vec<SelectOption> {
        let lang = current_language();
        vec![
            SelectOption::new(Self::Points.as_str(), ConceptKey::ByPoints.get(lang)),
            SelectOption::new(Self::BarCount.as_str(), ConceptKey::ByBarCount.get(lang)),
        ]
    }
}

/// The result of least-squares-regressing `source` price against bar
/// index, over the bars `period_mode` selects — see
/// [`RegressionTrend::fit`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RegressionFit {
    /// Price change per bar.
    pub slope: f64,
    /// Fitted price at bar index 0 — evaluate at any bar index through
    /// [`Self::price_at`] rather than reading this directly.
    pub intercept: f64,
    /// Coefficient of determination, in `[0, 1]`. `0.0` when the window
    /// holds fewer than two bars — nothing for a line to explain.
    pub r_squared: f64,
    /// Population standard deviation of the fit's own residuals — the
    /// unit `std_dev_mult` scales to draw the sigma bands.
    pub residual_std_dev: f64,
}

impl RegressionFit {
    /// The fitted price at any bar index. Negative or beyond the fit's own
    /// window extrapolates the SAME line — exactly what `extend_left`/
    /// `extend_right`, and a `BarCount`-mode window narrower than the
    /// drawn span, both need.
    pub fn price_at(&self, bar_idx: f64) -> f64 {
        self.intercept + self.slope * bar_idx
    }
}

/// Ordinary least squares of `ys` against `xs`, paired by index. Fewer
/// than two points has no line to fit: `intercept` holds the lone price
/// (or `0.0` for an empty window), every other field `0.0`.
fn least_squares_fit(xs: &[f64], ys: &[f64]) -> RegressionFit {
    if xs.len() < 2 {
        return RegressionFit { intercept: ys.first().copied().unwrap_or(0.0), ..Default::default() };
    }
    let n = xs.len() as f64;
    let mean_x = xs.iter().sum::<f64>() / n;
    let mean_y = ys.iter().sum::<f64>() / n;

    let mut covariance = 0.0;
    let mut variance_x = 0.0;
    for (&x, &y) in xs.iter().zip(ys) {
        covariance += (x - mean_x) * (y - mean_y);
        variance_x += (x - mean_x).powi(2);
    }
    let slope = if variance_x > 0.0 { covariance / variance_x } else { 0.0 };
    let intercept = mean_y - slope * mean_x;

    let mut ss_res = 0.0;
    let mut ss_tot = 0.0;
    for (&x, &y) in xs.iter().zip(ys) {
        ss_res += (y - (intercept + slope * x)).powi(2);
        ss_tot += (y - mean_y).powi(2);
    }
    let r_squared = if ss_tot > 0.0 { (1.0 - ss_res / ss_tot).clamp(0.0, 1.0) } else { 1.0 };
    let residual_std_dev = (ss_res / n).sqrt();

    RegressionFit { slope, intercept, r_squared, residual_std_dev }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegressionTrend {
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64,
    pub price1: f64,
    #[serde(default)]
    pub ts2: i64,
    pub price2: f64,
    #[serde(default = "default_std_dev")]
    pub std_dev_mult: f64,
    #[serde(default)]
    pub use_upper_deviation: bool,
    #[serde(default = "default_true")]
    pub show_center: bool,
    #[serde(default = "default_true")]
    pub fill: bool,
    #[serde(default = "default_area_fill_opacity")]
    pub fill_opacity: f64,
    #[serde(default)]
    pub extend_left: bool,
    #[serde(default)]
    pub extend_right: bool,
    /// The price series the fit regresses against bar index — see
    /// [`RegressionSource`]'s own doc. Absent from every drawing saved
    /// before this field existed; defaults to `Close`.
    #[serde(default)]
    pub source: RegressionSource,
    /// How the fit's own bar window is chosen — see
    /// [`RegressionPeriodMode`]'s own doc. Absent from every drawing saved
    /// before this field existed; defaults to `Points`, which reproduces
    /// exactly the span the two dragged anchors already describe.
    #[serde(default)]
    pub period_mode: RegressionPeriodMode,
    /// Bar count for [`RegressionPeriodMode::BarCount`] — inert under
    /// `Points`. Absent from every drawing saved before this field
    /// existed; defaults to 120, matching the approved design's own demo
    /// value.
    #[serde(default = "default_bar_count")]
    pub bar_count: f64,

    /// The fit's own cached slope/intercept/R²/residual-std-dev — see
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract. Filled by
    /// that hook from the SAME [`Self::fit`] `render`/`hit_test` call
    /// directly (never a second formula); read back only by
    /// [`Self::result_stat_rows`] — a `&self`-only reader with no `bars` of
    /// its own. `render`/`hit_test` never read this field, so a primitive
    /// the hook has never touched still draws exactly right.
    ///
    /// `#[serde(skip)]`: a stale fit surviving a reload/undo would misreport
    /// a window that no longer matches the primitive's own current points —
    /// a freshly deserialized primitive starts at [`RegressionFit::default`]
    /// (matching what `fit(&[])` itself returns) until the owning bubble's
    /// own bar-derived-cache pass runs at least once.
    #[serde(skip)]
    pub cached_fit: RegressionFit,
}

fn default_true() -> bool { true }
fn default_std_dev() -> f64 { 2.0 }
fn default_bar_count() -> f64 { 120.0 }

impl RegressionTrend {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "regression_trend".to_string(),
                display_name: "Regression Trend".to_string(),
                color: PrimitiveColor::with_fill(color, color),
                width: 2.0,
                ..Default::default()
            },
            ts1, price1, ts2, price2,
            std_dev_mult: 2.0,
            use_upper_deviation: false,
            show_center: true,
            fill: true,
            fill_opacity: AREA_FILL_OPACITY,
            extend_left: false,
            extend_right: false,
            source: RegressionSource::default(),
            period_mode: RegressionPeriodMode::default(),
            bar_count: default_bar_count(),
            cached_fit: RegressionFit::default(),
        }
    }

    /// The absolute bar-index range [`Self::fit`] feeds to
    /// [`least_squares_fit`] — see [`RegressionPeriodMode`]'s own doc for
    /// what each mode selects. `None` when `bars` is empty.
    fn fit_window(&self, bars: &[Bar]) -> Option<(usize, usize)> {
        if bars.is_empty() {
            return None;
        }
        let last = bars.len() - 1;
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1).round().clamp(0.0, last as f64) as usize;
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2).round().clamp(0.0, last as f64) as usize;
        let (lo, hi) = (bar1.min(bar2), bar1.max(bar2));
        match self.period_mode {
            RegressionPeriodMode::Points => Some((lo, hi)),
            RegressionPeriodMode::BarCount => {
                let count = (self.bar_count.round().max(2.0) as usize).min(hi + 1);
                Some((hi + 1 - count, hi))
            }
        }
    }

    /// Least-squares fit of `source` against bar index (the bar's own
    /// position in `bars`), over the window [`Self::fit_window`] selects —
    /// the ONE computation `render`, `hit_test` and (once wired, see
    /// `style_properties`'s own doc) any reported metric all read. Bar
    /// index rather than timestamp so [`RegressionFit::price_at`] composes
    /// directly with [`timestamp_ms_to_bar_f64`].
    pub fn fit(&self, bars: &[Bar]) -> RegressionFit {
        let Some((start, end)) = self.fit_window(bars) else { return RegressionFit::default() };
        let xs: Vec<f64> = (start..=end).map(|i| i as f64).collect();
        let ys: Vec<f64> = (start..=end).map(|i| self.source.price(&bars[i])).collect();
        least_squares_fit(&xs, &ys)
    }

    /// The read-only Slope/R² stat block's own rows — the pure decision
    /// [`Self::style_properties`] wraps into a declared property
    /// ([`ConfigProperty::stat_block`]). Reads [`Self::cached_fit`], never
    /// `bars` directly: `style_properties(&self)` is never handed `bars`
    /// (see that method's own doc), and re-deriving a second formula here
    /// from nothing would defeat the one-computation-path point of caching
    /// [`Self::fit`]'s own result in the first place — see
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract for who
    /// fills the cache this reads.
    pub fn result_stat_rows(&self) -> Vec<StatRow> {
        let slope = self.cached_fit.slope;
        let sign = if slope >= 0.0 { "+" } else { "-" };
        vec![
            StatRow::regression_slope(&format!("{sign}{}", fmt_price(slope.abs()))),
            StatRow::regression_r_squared(&format!("{:.2}", self.cached_fit.r_squared)),
        ]
    }
}

impl Primitive for RegressionTrend {
    fn type_id(&self) -> &'static str { "regression_trend" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Channel }
    // Shares `PrimitiveKind::Channel` with parallel/disjoint/flat-top
    // channels, but is its own sigma-band archetype, not the generic
    // `Channel` shape.
    fn archetype(&self) -> Archetype { Archetype::Regression }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, self.price1), (self.ts2, self.price2)] }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if points.len() >= 2 {
            self.ts1 = points[0].0; self.price1 = points[0].1;
            self.ts2 = points[1].0; self.price2 = points[1].1;
        }
    }

    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64) {
        self.ts1 += ts_delta_ms; self.ts2 += ts_delta_ms;
        self.price1 += price_delta; self.price2 += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64) {
        match point_type {
            ControlPointType::Point1 => { self.ts1 = ts_ms; self.price1 = price; }
            ControlPointType::Point2 => { self.ts2 = ts_ms; self.price2 = price; }
            ControlPointType::Move => {
                let mid_ts = (self.ts1 + self.ts2) / 2;
                let mid_price = (self.price1 + self.price2) / 2.0;
                let td = ts_ms - mid_ts;
                let pd = price - mid_price;
                self.translate(td, pd);
            }
            _ => {}
        }
    }

    fn hit_test(&self, screen_x: f64, screen_y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> HitTestResult {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let x2 = viewport.bar_to_x_f64(bar2);
        // The two draggable handles and the Move handle stay exactly where
        // the analyst dropped them — raw `price1`/`price2`, never the fit
        // (see `control_points`'s own doc for why).
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        if check_point_hit(screen_x, screen_y, x1, y1) { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if check_point_hit(screen_x, screen_y, x2, y2) { return HitTestResult::ControlPoint(ControlPointType::Point2); }

        let cx = (x1 + x2) / 2.0; let cy = (y1 + y2) / 2.0;
        if check_point_hit(screen_x, screen_y, cx, cy) { return HitTestResult::ControlPoint(ControlPointType::Move); }

        // The BODY strokes — fitted centre line + sigma bands, exactly what
        // `render` draws (see the module doc, and `fit`'s own doc).
        let fit = self.fit(bars);
        let center_price1 = fit.price_at(bar1);
        let center_price2 = fit.price_at(bar2);
        let center_y1 = viewport.price_to_y(center_price1, price_scale.price_min, price_scale.price_max);
        let center_y2 = viewport.price_to_y(center_price2, price_scale.price_min, price_scale.price_max);
        let offset = self.std_dev_mult * fit.residual_std_dev;
        let upper_y1 = viewport.price_to_y(center_price1 + offset, price_scale.price_min, price_scale.price_max);
        let upper_y2 = viewport.price_to_y(center_price2 + offset, price_scale.price_min, price_scale.price_max);
        let lower_y1 = viewport.price_to_y(center_price1 - offset, price_scale.price_min, price_scale.price_max);
        let lower_y2 = viewport.price_to_y(center_price2 - offset, price_scale.price_min, price_scale.price_max);

        // Reach exactly as far as `render` draws — all three lines share
        // `ts1`/`ts2`, so their extended x-coordinates always agree.
        let (cx1, cy1, cx2, cy2) = extend_segment_to_edges(x1, center_y1, x2, center_y2, self.extend_left, self.extend_right, viewport.chart_width);
        let (eu_x1, eu_y1, eu_x2, eu_y2) = extend_segment_to_edges(x1, upper_y1, x2, upper_y2, self.extend_left, self.extend_right, viewport.chart_width);
        let (el_x1, el_y1, el_x2, el_y2) = extend_segment_to_edges(x1, lower_y1, x2, lower_y2, self.extend_left, self.extend_right, viewport.chart_width);
        if self.show_center && point_to_line_distance(screen_x, screen_y, cx1, cy1, cx2, cy2) < HIT_TOLERANCE { return HitTestResult::Body; }
        if point_to_line_distance(screen_x, screen_y, eu_x1, eu_y1, eu_x2, eu_y2) < HIT_TOLERANCE { return HitTestResult::Body; }
        if point_to_line_distance(screen_x, screen_y, el_x1, el_y1, el_x2, el_y2) < HIT_TOLERANCE { return HitTestResult::Body; }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Vec<ControlPoint> {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let x2 = viewport.bar_to_x_f64(bar2);
        // Handles read the raw dragged prices, not the fit: they are what
        // the analyst is holding, and the fit is free to disagree with
        // them (that disagreement is the whole point of a regression
        // channel — see the module doc).
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        let mut points = vec![ControlPoint::point1(x1, y1), ControlPoint::point2(x2, y2)];
        let cx = (x1 + x2) / 2.0; let cy = (y1 + y2) / 2.0;
        let dist = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
        if dist > CONTROL_POINT_RADIUS * 4.0 { points.push(ControlPoint::move_point(cx, cy)); }
        points
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let bars = ctx.bars();
        let fit = self.fit(bars);
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = ctx.bar_to_x(bar1);
        let x2 = ctx.bar_to_x(bar2);

        // The centre line is the fit evaluated at the two anchors' own bar
        // index — NOT the raw `price1`/`price2` the analyst dropped it at
        // (see the module doc: this is the actual fix, and the line moves
        // because of it).
        let center_price1 = fit.price_at(bar1);
        let center_price2 = fit.price_at(bar2);
        let y1 = ctx.price_to_y(center_price1);
        let y2 = ctx.price_to_y(center_price2);

        // The bands are ± `std_dev_mult` standard deviations of the fit's
        // own residuals, not a fixed price offset.
        let offset = self.std_dev_mult * fit.residual_std_dev;
        let upper_y1 = ctx.price_to_y(center_price1 + offset);
        let upper_y2 = ctx.price_to_y(center_price2 + offset);
        let lower_y1 = ctx.price_to_y(center_price1 - offset);
        let lower_y2 = ctx.price_to_y(center_price2 - offset);

        // An extended side draws to the panel edge in that direction — off
        // by default, so an unextended primitive draws the exact same
        // three segments as before this pass. All three lines share
        // `ts1`/`ts2`, so their extended x-coordinates always agree.
        let chart_width = ctx.chart_width();
        let (cx1, cy1, cx2, cy2) = extend_segment_to_edges(x1, y1, x2, y2, self.extend_left, self.extend_right, chart_width);
        let (eu_x1, eu_y1, eu_x2, eu_y2) = extend_segment_to_edges(x1, upper_y1, x2, upper_y2, self.extend_left, self.extend_right, chart_width);
        let (el_x1, el_y1, el_x2, el_y2) = extend_segment_to_edges(x1, lower_y1, x2, lower_y2, self.extend_left, self.extend_right, chart_width);

        if self.fill {
            let base = self.data.color.fill.as_deref().unwrap_or(&self.data.color.stroke);
            let fill_color = format!("{}{:02x}", &base[..7], (self.fill_opacity * 255.0) as u8);
            ctx.set_fill_color(&fill_color);
            ctx.begin_path();
            ctx.move_to(eu_x1, eu_y1); ctx.line_to(eu_x2, eu_y2);
            ctx.line_to(el_x2, el_y2); ctx.line_to(el_x1, el_y1);
            ctx.close_path(); ctx.fill();
        }

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        ctx.begin_path();
        ctx.move_to(crisp(eu_x1, dpr), crisp(eu_y1, dpr));
        ctx.line_to(crisp(eu_x2, dpr), crisp(eu_y2, dpr));
        ctx.stroke();

        ctx.begin_path();
        ctx.move_to(crisp(el_x1, dpr), crisp(el_y1, dpr));
        ctx.line_to(crisp(el_x2, dpr), crisp(el_y2, dpr));
        ctx.stroke();

        if self.show_center {
            ctx.set_line_dash(&[4.0, 4.0]);
            ctx.begin_path();
            ctx.move_to(crisp(cx1, dpr), crisp(cy1, dpr));
            ctx.line_to(crisp(cx2, dpr), crisp(cy2, dpr));
            ctx.stroke();
        }
        ctx.set_line_dash(&[]);

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let (text_y1, text_y2) = match text.v_align {
                    super::super::TextAlign::Start => (upper_y1, upper_y2),
                    super::super::TextAlign::End => (lower_y1, lower_y2),
                    super::super::TextAlign::Center => (y1, y2),
                };
                let params = calculate_line_text_params(x1, text_y1, x2, text_y2, text);
                render_primitive_text_rotated(ctx, text, params.x, params.y, &self.data.color.stroke, params.rotation);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    /// Recompute [`Self::cached_fit`] from `bars` through the SAME
    /// [`Self::fit`] `render`/`hit_test` call — see
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract. `fit` is
    /// already cheap (one pass over the fit window, never the whole bar
    /// array), so this needs no separate short-circuit of its own.
    fn refresh_bar_derived_cache(&mut self, bars: &[Bar]) {
        self.cached_fit = self.fit(bars);
    }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        // Regression archetype (W3, `docs/mlc/plans/archetype-modals-into-the-app-2026-08-11.md`):
        // `regression_tabs()` (`mlc-ui-lab`'s `spec.rs`) declares four
        // groups on the "sigma" subject tab — Calculation, Sigmas,
        // Baseline, Metrics — plus an Extend group. This primitive's own
        // `archetype()` override already points at `Archetype::Regression`
        // (see above).
        //
        // `PropertyGroup::Calculation` (source / period mode / bar count)
        // is now declared for real: `Self::fit` least-squares-regresses
        // `source` price against bar index over the bars `period_mode`
        // selects (see the module doc and `fit`'s own doc) — the centre
        // line these three properties feed IS the line `render`/`hit_test`
        // draw, not a second computation reading them for show.
        //
        // `PropertyGroup::Sigmas` — the lab's own ratio-style TABLE (one
        // row per ±1σ/±2σ/±3σ band: multiplier, colour swatch, per-band
        // fill checkbox — `PropertyType::Table`, landed W4) is still not
        // declared: this primitive tracks only ONE band width
        // (`std_dev_mult`) and one shared fill toggle, not three
        // independent bands, so there is no honest per-row mapping — a
        // three-band table needs three multiplier/colour/fill fields this
        // struct does not have, which is a wider redesign than fitting the
        // line this pass is scoped to. What DOES have a real single-band
        // equivalent is the group's own trailing "Fill Opacity" slider and
        // this primitive's existing `fill` master toggle, both re-homed
        // into this group below rather than left in the default run.
        //
        // `PropertyGroup::Baseline` — unchanged by this pass: the lab's
        // "Show"/colour/width trio for the dashed centre line.
        // `show_center` is a real, already-rendered field (`render`'s own
        // `if self.show_center` branch draws the dashed line; `hit_test`
        // gates the same way) — declared below. Colour and width are NOT
        // declared: this primitive has no baseline-specific stroke of its
        // own, the centre line reuses `data.color.stroke`/`data.width` —
        // the SAME pair the two sigma band lines draw with — so there is
        // no independent field a baseline-only colour/width control could
        // write into without silently also repainting the bands.
        //
        // `PropertyGroup::Extend` — `extend_left`/`extend_right` extend
        // all three (now fitted) lines to the panel edge
        // (`render`/`hit_test` above, via `extend_segment_to_edges`).
        //
        // The Metrics group's own Slope/R² stat rows are wired now (the
        // `ShowSlope`/`ShowRSquared` per-row visibility toggles the lab's
        // spec also shows are NOT — a separate, purely-UI addition, not a
        // computation gap): `Primitive::refresh_bar_derived_cache` closes
        // the plumbing gap the earlier version of this comment described
        // (`style_properties(&self)` still has no `bars` of its own — that
        // has not changed — but `Self::cached_fit` now gets filled by that
        // hook, from the SAME `Self::fit` `render`/`hit_test` already call,
        // and `Self::result_stat_rows` below reads the cache rather than
        // inventing a second formula). See `mlc-backend`'s
        // `crate::bubble::bar_derived_cache` module for the sweep that
        // calls the hook — same convergence points
        // (`BubbleStore::restore_from_snapshots`, every bar-array write)
        // Elliott's own `wave_guards::refresh_wave_guards` uses, but a
        // genuinely separate pass: this one never reads sibling primitives,
        // only `bars`, and Elliott's never reads `bars`, only siblings.
        vec![
            ConfigProperty::select("source", ModalKey::AlertSource.get(current_language()), self.source.as_str(), RegressionSource::select_options())
                .with_category(PropertyCategory::AppearanceLine).with_order(5).in_group(PropertyGroup::Calculation),
            ConfigProperty::select("period_mode", ConceptKey::Period.get(current_language()), self.period_mode.as_str(), RegressionPeriodMode::select_options())
                .with_category(PropertyCategory::AppearanceLine).with_order(6).in_group(PropertyGroup::Calculation),
            ConfigProperty::number("bar_count", ConceptKey::BarCount.get(current_language()), self.bar_count, Some(2.0), None)
                .with_category(PropertyCategory::AppearanceLine).with_order(7).in_group(PropertyGroup::Calculation),
            ConfigProperty::fill(self.fill).with_category(PropertyCategory::AppearanceFill).with_order(20).in_group(PropertyGroup::Sigmas),
            ConfigProperty::fill_opacity(self.fill_opacity * 100.0).with_category(PropertyCategory::AppearanceFill).with_order(21).in_group(PropertyGroup::Sigmas),
            ConfigProperty::boolean("show_center", SettingsKey::Show.get(current_language()), self.show_center)
                .with_category(PropertyCategory::AppearanceLine).with_order(22).in_group(PropertyGroup::Baseline),
            ConfigProperty::extend_left(self.extend_left).with_category(PropertyCategory::AppearanceLine).with_order(23).in_group(PropertyGroup::Extend),
            ConfigProperty::extend_right(self.extend_right).with_category(PropertyCategory::AppearanceLine).with_order(24).in_group(PropertyGroup::Extend),
            // `PropertyGroup::Metrics` (W4 follow-up) — the read-only
            // Slope/R² stat block. See `Self::result_stat_rows`'s own doc.
            ConfigProperty::stat_block(REGRESSION_RESULT_STATS_ID, self.result_stat_rows())
                .with_order(30).in_group(PropertyGroup::Metrics),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            // The computed Slope/R² stat block is declared `readonly`
            // (`style_properties`'s own `ConfigProperty::stat_block(
            // REGRESSION_RESULT_STATS_ID, ...)` call, fed by
            // `Self::result_stat_rows`) and carries no field of its own to
            // write — explicit rejection here, not merely the absence of a
            // matching arm, so a future edit near this match can never
            // accidentally start honouring the id (mirrors
            // `LongPosition::apply_style_property`'s own
            // `POSITION_RESULT_STATS_ID` arm).
            REGRESSION_RESULT_STATS_ID => {}
            "source" => { if let Some(s) = value.as_string() { self.source = RegressionSource::from_str(s); return true; } }
            "period_mode" => { if let Some(s) = value.as_string() { self.period_mode = RegressionPeriodMode::from_str(s); return true; } }
            "bar_count" => { if let Some(v) = value.as_number() { self.bar_count = v.max(2.0); return true; } }
            "fill" => { if let Some(v) = value.as_bool() { self.fill = v; return true; } }
            "fill_opacity" => { if let Some(v) = value.as_number() { self.fill_opacity = (v / 100.0).clamp(0.0, 1.0); return true; } }
            "show_center" => { if let Some(v) = value.as_bool() { self.show_center = v; return true; } }
            "extend_left" => { if let Some(v) = value.as_bool() { self.extend_left = v; return true; } }
            "extend_right" => { if let Some(v) = value.as_bool() { self.extend_right = v; return true; } }
            _ => {}
        }
        false
    }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    (sx - px).powi(2) + (sy - py).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2)
}

fn create_regression_trend(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
    let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, price1));
    Box::new(RegressionTrend::new(ts1, price1, ts2, price2, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "regression_trend",
        display_name: "Regression Trend",
        kind: PrimitiveKind::Channel,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Draw a linear regression channel",
        icon: "regression_trend",
        default_color: "#9C27B0",
        factory: create_regression_trend,
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod extend_tests {
    use super::*;
    use crate::{Bar, PriceScale, Viewport};

    fn fixture() -> (Vec<Bar>, Viewport, PriceScale) {
        let bars: Vec<Bar> = (0..6).map(|i| Bar::new(i, 100.0, 102.0, 98.0, 100.0)).collect();
        let viewport = Viewport { chart_width: 1200.0, chart_height: 600.0, bar_count: bars.len(), ..Default::default() };
        let price_scale = PriceScale { price_min: 50.0, price_max: 150.0, ..Default::default() };
        (bars, viewport, price_scale)
    }

    /// A probe point sitting exactly on the extended centre line, at the
    /// panel's own left edge — misses an unextended regression channel but
    /// resolves to `Body` once `extend_left` is on, because `hit_test` now
    /// tests the SAME extended segment `render` draws
    /// (`extend_segment_to_edges`).
    ///
    /// `fixture()`'s bars carry a CONSTANT close (100.0 on every bar), so
    /// the fit's slope is exactly zero and its centre line sits flat at
    /// price 100 regardless of the anchors' own raw `price1`/`price2` —
    /// the probe is derived from `channel.fit(&bars)`, the same source
    /// `hit_test` reads, rather than from the raw anchor prices the
    /// pre-fit version of this test used.
    #[test]
    fn extend_left_reaches_the_panel_edge_on_the_centre_line() {
        let (bars, viewport, price_scale) = fixture();
        // ts values land exactly on bar indices 2 and 5 of `fixture()`'s
        // 6-bar slice (NOT 0 — the left anchor must not already sit at the
        // panel's own left edge, or "extending" it would be a no-op and
        // the probe would already be an existing control point).
        let channel = RegressionTrend::new(2_000, 100.0, 5_000, 110.0, "#9C27B0");
        assert!(channel.show_center, "the centre line must be on for this probe to land on it");

        let bar1 = timestamp_ms_to_bar_f64(&bars, channel.ts1);
        let bar2 = timestamp_ms_to_bar_f64(&bars, channel.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let x2 = viewport.bar_to_x_f64(bar2);
        let fit = channel.fit(&bars);
        let y1 = viewport.price_to_y(fit.price_at(bar1), price_scale.price_min, price_scale.price_max);
        let y2 = viewport.price_to_y(fit.price_at(bar2), price_scale.price_min, price_scale.price_max);
        let (probe_x, probe_y, _, _) = extend_segment_to_edges(x1, y1, x2, y2, true, false, viewport.chart_width);
        assert_eq!(probe_x, 0.0, "the probe sits at the panel's own left edge");

        assert!(
            matches!(channel.hit_test(probe_x, probe_y, &bars, &viewport, &price_scale), HitTestResult::Miss),
            "unextended channel misses a point on the panel's left edge",
        );

        let mut extended = channel;
        extended.extend_left = true;
        assert!(
            matches!(extended.hit_test(probe_x, probe_y, &bars, &viewport, &price_scale), HitTestResult::Body),
            "extend_left reaches the same point once the centre line is drawn that far",
        );
    }
}

#[cfg(test)]
mod fit_tests {
    use super::*;

    /// `bars.len()` bars, timestamp `= i` seconds (so `ts_ms = i * 1000`
    /// lands exactly on bar `i` — same convention `extend_tests::fixture`
    /// uses), open/high/low/close all equal to `closes[i]`.
    ///
    /// `pub(super)`: also reused by the sibling `bar_derived_cache_tests`
    /// module below, rather than a second copy of the same fixture.
    pub(super) fn bars_from(closes: &[f64]) -> Vec<Bar> {
        closes.iter().enumerate().map(|(i, &c)| Bar::new(i as i64, c, c, c, c)).collect()
    }

    /// A `RegressionTrend` whose two anchors sit exactly on `bars`' own
    /// first and last bar, so `RegressionPeriodMode::Points` (the default)
    /// fits over the WHOLE slice.
    pub(super) fn span(bars: &[Bar], color: &str) -> RegressionTrend {
        let last = (bars.len() - 1) as i64;
        RegressionTrend::new(0, bars[0].close, last * 1000, bars[bars.len() - 1].close, color)
    }

    /// A perfectly linear series fits EXACTLY: the least-squares slope is
    /// the series' own slope, and R² is 1 — no residual at all.
    #[test]
    fn perfectly_linear_series_fits_exactly() {
        let closes: Vec<f64> = (0..6).map(|i| 3.0 + 2.0 * i as f64).collect();
        let bars = bars_from(&closes);
        let trend = span(&bars, "#9C27B0");
        let fit = trend.fit(&bars);
        assert_eq!(fit.slope, 2.0, "slope = {}", fit.slope);
        assert!((fit.r_squared - 1.0).abs() < 1e-9, "r_squared = {}", fit.r_squared);
        assert!(fit.residual_std_dev < 1e-9, "residual_std_dev = {}", fit.residual_std_dev);
    }

    /// A flat series — every bar the same price — has nothing for a line
    /// to explain: the least-squares slope is exactly zero.
    #[test]
    fn flat_series_fits_zero_slope() {
        let bars = bars_from(&[100.0; 6]);
        let trend = span(&bars, "#9C27B0");
        assert_eq!(trend.fit(&bars).slope, 0.0);
    }

    /// Hand-computed example: closes `[1, 2, 3, 5]` against bar index
    /// `[0, 1, 2, 3]`.
    ///
    /// Standard OLS: `slope = (nΣxy - ΣxΣy) / (nΣx² - (Σx)²)
    /// = (4·23 - 6·11) / (4·14 - 36) = 26 / 20 = 1.3`;
    /// `intercept = ȳ - slope·x̄ = 2.75 - 1.3·1.5 = 0.8`.
    /// Residuals `[1-0.8, 2-2.1, 3-3.4, 5-4.7] = [0.2, -0.1, -0.4, 0.3]`,
    /// sum of squares `0.04 + 0.01 + 0.16 + 0.09 = 0.30`, population
    /// variance `0.30 / 4 = 0.075`.
    #[test]
    fn residual_band_matches_hand_computed_values() {
        let bars = bars_from(&[1.0, 2.0, 3.0, 5.0]);
        let mut trend = span(&bars, "#9C27B0");
        trend.std_dev_mult = 2.0;
        let fit = trend.fit(&bars);

        assert!((fit.slope - 1.3).abs() < 1e-9, "slope = {}", fit.slope);
        assert!((fit.intercept - 0.8).abs() < 1e-9, "intercept = {}", fit.intercept);
        let expected_std_dev = (0.30_f64 / 4.0).sqrt();
        assert!(
            (fit.residual_std_dev - expected_std_dev).abs() < 1e-9,
            "residual_std_dev = {}, expected {}", fit.residual_std_dev, expected_std_dev,
        );

        // The band width `render`/`hit_test` actually draw: `std_dev_mult`
        // standard deviations of the residuals.
        let expected_band = trend.std_dev_mult * expected_std_dev;
        assert!((trend.std_dev_mult * fit.residual_std_dev - expected_band).abs() < 1e-12);
    }

    /// The price source genuinely changes the fit: a series whose closes
    /// are flat but whose highs climb produces two different slopes for
    /// the exact same bars.
    #[test]
    fn source_selection_changes_the_fit() {
        let bars: Vec<Bar> = (0..6).map(|i| Bar::new(i, 100.0, 100.0 + i as f64 * 5.0, 100.0, 100.0)).collect();
        let mut trend = span(&bars, "#9C27B0");

        trend.source = RegressionSource::Close;
        let close_fit = trend.fit(&bars);
        assert_eq!(close_fit.slope, 0.0, "flat close: slope = {}", close_fit.slope);

        trend.source = RegressionSource::High;
        let high_fit = trend.fit(&bars);
        assert_eq!(high_fit.slope, 5.0, "climbing high: slope = {}", high_fit.slope);

        assert_ne!(close_fit.slope, high_fit.slope);
    }
}

#[cfg(test)]
mod bar_derived_cache_tests {
    //! `Primitive::refresh_bar_derived_cache` coverage for `RegressionTrend`
    //! — the one primitive that overrides it today. `fit_tests::bars_from`/
    //! `span` build the fixtures; see that module's own doc for the
    //! convention (bar `i`'s OHLC all equal `closes[i]`, anchors span the
    //! whole slice under the default `Points` period mode).

    use super::*;
    use super::fit_tests::{bars_from, span};

    /// The hook fills [`RegressionTrend::cached_fit`] from real bars
    /// through the SAME [`RegressionTrend::fit`] `render`/`hit_test` call,
    /// and [`RegressionTrend::result_stat_rows`] — the rows
    /// [`ConfigProperty::stat_block`] declares — carry those exact fitted
    /// numbers, not a second, independently-computed formula.
    #[test]
    fn hook_fills_cached_fit_and_the_declared_stat_rows_carry_it() {
        let bars = bars_from(&[1.0, 2.0, 3.0, 5.0]);
        let mut trend = span(&bars, "#9C27B0");

        // Before the hook ever runs, a freshly constructed primitive reads
        // the safe default — matching what `fit(&[])` itself returns, never
        // a stale or partially-computed number.
        assert_eq!(trend.cached_fit, RegressionFit::default());

        trend.refresh_bar_derived_cache(&bars);

        let direct_fit = trend.fit(&bars);
        assert_eq!(trend.cached_fit, direct_fit, "cache must equal the SAME fit() render/hit_test call");
        assert!((trend.cached_fit.slope - 1.3).abs() < 1e-9, "slope = {}", trend.cached_fit.slope);

        let rows = trend.result_stat_rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].value, format!("+{}", fmt_price(direct_fit.slope)), "slope row");
        assert_eq!(rows[1].value, format!("{:.2}", direct_fit.r_squared), "R\u{b2} row");

        let stat_prop = trend.style_properties().into_iter().find(|p| p.id == REGRESSION_RESULT_STATS_ID)
            .expect("stat block declared");
        assert_eq!(stat_prop.value, PropertyValue::Stat(rows));
        assert!(stat_prop.readonly);
        assert_eq!(stat_prop.group, Some(PropertyGroup::Metrics));
    }

    /// A negative slope renders with the minus sign, not a doubled sign
    /// from `fmt_price` formatting an already-negative number.
    #[test]
    fn negative_slope_renders_with_a_single_minus_sign() {
        let bars = bars_from(&[10.0, 8.0, 6.0, 4.0]);
        let mut trend = span(&bars, "#9C27B0");
        trend.refresh_bar_derived_cache(&bars);
        assert!(trend.cached_fit.slope < 0.0, "slope = {}", trend.cached_fit.slope);
        let rows = trend.result_stat_rows();
        assert_eq!(rows[0].value, format!("-{}", fmt_price(trend.cached_fit.slope.abs())));
    }

    /// `refresh_bar_derived_cache` never mutates anything an analyst set —
    /// running it twice over a primitive with edited style leaves every
    /// non-cache field byte-for-byte identical. `to_json()` is the right
    /// probe for exactly this claim: `cached_fit` is `#[serde(skip)]`, so
    /// it never appears in the serialized form at all — a regression that
    /// had the hook touch a real field (colour, width, points, ...) would
    /// change this string; a regression that only touched `cached_fit`
    /// (the intended behaviour) would not.
    #[test]
    fn pass_never_mutates_analyst_set_style_run_twice() {
        let bars = bars_from(&[1.0, 2.0, 3.0, 5.0, 4.0, 6.0]);
        let mut trend = span(&bars, "#9C27B0");
        // Edit style the way an analyst would through the settings modal.
        trend.data.color.stroke = "#ff0000".to_string();
        trend.data.width = 5.0;
        trend.std_dev_mult = 3.0;
        trend.extend_left = true;
        trend.source = RegressionSource::High;
        trend.period_mode = RegressionPeriodMode::BarCount;
        trend.bar_count = 3.0;

        let before = trend.to_json();
        trend.refresh_bar_derived_cache(&bars);
        let after_first = trend.to_json();
        assert_eq!(before, after_first, "the pass must not change any persisted field");

        let fit_after_first = trend.cached_fit;
        trend.refresh_bar_derived_cache(&bars);
        let after_second = trend.to_json();
        assert_eq!(after_first, after_second, "a second run over the same bars must not drift anything either");
        assert_eq!(trend.cached_fit, fit_after_first, "re-running over the SAME bars must be idempotent");
    }
}
