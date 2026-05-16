//! Vello Hybrid RenderContext implementation
//!
//! Thin wrapper around `uzor_render_vello_hybrid::VelloHybridRenderContext` that
//! delegates all drawing operations to the vello-hybrid backend and adds chart-domain
//! coordinate conversion (bar → X, price → Y) on top.

use zengeld_chart::{PriceScale, Viewport};
use zengeld_chart::render::RenderContext as ChartRenderContext;
use uzor::render::{
    RenderContext as UzorRenderContext, RenderContextExt,
    Painter, TextRenderer, TextMetrics, Masking, Effects,
    ShapeHelpers, BatchPainter, GradientPainter, UiEffectHelpers,
    TextAlign, TextBaseline, BlendMode, TextBounds,
    LineSegment, CircleBatch,
};
use uzor_render_vello_hybrid::VelloHybridRenderContext as InnerContext;

// ─── Coordinate-space override ────────────────────────────────────────────────

/// Per-window coordinate space set by `set_coordinate_space`.
/// When present, overrides `viewport`/`price_scale` for conversions.
#[derive(Clone, Debug)]
struct CoordinateSpaceOverride {
    chart_width: f64,
    chart_height: f64,
    view_start: f64,
    bar_spacing: f64,
    price_min: f64,
    price_max: f64,
}

// ─── Public wrapper ───────────────────────────────────────────────────────────

/// Chart render context backed by the vello-hybrid renderer.
///
/// All drawing primitives are delegated to [`uzor_render_vello_hybrid::VelloHybridRenderContext`].
/// This type only adds chart-domain coordinate conversion on top.
pub struct VelloHybridChartRenderContext<'a> {
    inner: InnerContext,
    viewport: Option<&'a Viewport>,
    price_scale: Option<&'a PriceScale>,
    coord_override: Option<CoordinateSpaceOverride>,
}

impl<'a> VelloHybridChartRenderContext<'a> {
    /// Create a new context for a frame.
    ///
    /// * `dpr` — device pixel ratio passed to the inner context.
    /// * `viewport` — optional viewport for bar→X conversion.
    /// * `price_scale` — optional price scale for price→Y conversion.
    pub fn new(
        dpr: f64,
        viewport: Option<&'a Viewport>,
        price_scale: Option<&'a PriceScale>,
    ) -> Self {
        Self {
            inner: InnerContext::new(dpr),
            viewport,
            price_scale,
            coord_override: None,
        }
    }

    /// Access the inner context (e.g., to call `begin_frame` or `render`).
    pub fn inner(&self) -> &InnerContext {
        &self.inner
    }

    /// Mutably access the inner context (e.g., to call `begin_frame` or `render`).
    pub fn inner_mut(&mut self) -> &mut InnerContext {
        &mut self.inner
    }
}

// ─── Painter ─────────────────────────────────────────────────────────────────

impl<'a> Painter for VelloHybridChartRenderContext<'a> {
    fn save(&mut self) { self.inner.save() }
    fn restore(&mut self) { self.inner.restore() }
    fn translate(&mut self, x: f64, y: f64) { self.inner.translate(x, y) }
    fn rotate(&mut self, angle: f64) { self.inner.rotate(angle) }
    fn scale(&mut self, x: f64, y: f64) { self.inner.scale(x, y) }
    fn set_fill_color(&mut self, color: &str) { self.inner.set_fill_color(color) }
    fn set_global_alpha(&mut self, alpha: f64) { self.inner.set_global_alpha(alpha) }
    fn set_stroke_color(&mut self, color: &str) { self.inner.set_stroke_color(color) }
    fn set_stroke_width(&mut self, width: f64) { self.inner.set_stroke_width(width) }
    fn set_line_dash(&mut self, pattern: &[f64]) { self.inner.set_line_dash(pattern) }
    fn set_line_cap(&mut self, cap: &str) { self.inner.set_line_cap(cap) }
    fn set_line_join(&mut self, join: &str) { self.inner.set_line_join(join) }
    fn begin_path(&mut self) { self.inner.begin_path() }
    fn move_to(&mut self, x: f64, y: f64) { self.inner.move_to(x, y) }
    fn line_to(&mut self, x: f64, y: f64) { self.inner.line_to(x, y) }
    fn close_path(&mut self) { self.inner.close_path() }
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) { self.inner.rect(x, y, w, h) }
    fn arc(&mut self, cx: f64, cy: f64, radius: f64, start_angle: f64, end_angle: f64) {
        self.inner.arc(cx, cy, radius, start_angle, end_angle)
    }
    fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64, rotation: f64, start: f64, end: f64) {
        self.inner.ellipse(cx, cy, rx, ry, rotation, start, end)
    }
    fn quadratic_curve_to(&mut self, cpx: f64, cpy: f64, x: f64, y: f64) {
        self.inner.quadratic_curve_to(cpx, cpy, x, y)
    }
    fn bezier_curve_to(&mut self, cp1x: f64, cp1y: f64, cp2x: f64, cp2y: f64, x: f64, y: f64) {
        self.inner.bezier_curve_to(cp1x, cp1y, cp2x, cp2y, x, y)
    }
    fn stroke(&mut self) { self.inner.stroke() }
    fn fill(&mut self) { self.inner.fill() }
}

// ─── TextRenderer ─────────────────────────────────────────────────────────────

impl<'a> TextRenderer for VelloHybridChartRenderContext<'a> {
    fn set_font(&mut self, font: &str) { self.inner.set_font(font) }
    fn set_text_align(&mut self, align: TextAlign) { self.inner.set_text_align(align) }
    fn set_text_baseline(&mut self, baseline: TextBaseline) { self.inner.set_text_baseline(baseline) }
    fn fill_text(&mut self, text: &str, x: f64, y: f64) { self.inner.fill_text(text, x, y) }
    fn stroke_text(&mut self, text: &str, x: f64, y: f64) { self.inner.stroke_text(text, x, y) }
}

// ─── TextMetrics ──────────────────────────────────────────────────────────────

impl<'a> TextMetrics for VelloHybridChartRenderContext<'a> {
    fn measure_text(&self, text: &str) -> f64 { self.inner.measure_text(text) }
    fn text_bounds(&self, text: &str, font: &str) -> TextBounds { self.inner.text_bounds(text, font) }
}

// ─── Masking ──────────────────────────────────────────────────────────────────

impl<'a> Masking for VelloHybridChartRenderContext<'a> {
    fn clip(&mut self) { self.inner.clip() }
}

// ─── Effects ──────────────────────────────────────────────────────────────────

impl<'a> Effects for VelloHybridChartRenderContext<'a> {
    fn set_shadow(&mut self, dx: f64, dy: f64, blur: f64, color: &str) {
        self.inner.set_shadow(dx, dy, blur, color)
    }
    fn clear_shadow(&mut self) { self.inner.clear_shadow() }
    fn set_blend_mode(&mut self, mode: BlendMode) { self.inner.set_blend_mode(mode) }
}

// ─── ShapeHelpers ─────────────────────────────────────────────────────────────

impl<'a> ShapeHelpers for VelloHybridChartRenderContext<'a> {
    fn stroke_rect(&mut self, x: f64, y: f64, w: f64, h: f64) { self.inner.stroke_rect(x, y, w, h) }
    fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) { self.inner.fill_rect(x, y, w, h) }
    fn rounded_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        self.inner.rounded_rect(x, y, w, h, r)
    }
    fn rounded_rect_corners(
        &mut self, x: f64, y: f64, w: f64, h: f64,
        tl: f64, tr: f64, br: f64, bl: f64,
    ) {
        self.inner.rounded_rect_corners(x, y, w, h, tl, tr, br, bl)
    }
}

// ─── BatchPainter ─────────────────────────────────────────────────────────────

impl<'a> BatchPainter for VelloHybridChartRenderContext<'a> {
    fn draw_line_batch(&mut self, lines: &[LineSegment], color: &str, width: f64) {
        self.inner.draw_line_batch(lines, color, width)
    }
    fn draw_circle_batch(&mut self, circles: &[CircleBatch], color: &str) {
        self.inner.draw_circle_batch(circles, color)
    }
    fn stroke_polyline(&mut self, pts: &[(f64, f64)], color: &str, width: f64) {
        self.inner.stroke_polyline(pts, color, width)
    }
}

// ─── GradientPainter ──────────────────────────────────────────────────────────

impl<'a> GradientPainter for VelloHybridChartRenderContext<'a> {
    fn fill_linear_gradient(
        &mut self, stops: &[(f32, &str)],
        x1: f64, y1: f64, x2: f64, y2: f64,
    ) {
        self.inner.fill_linear_gradient(stops, x1, y1, x2, y2)
    }
    fn fill_radial_gradient(
        &mut self, cx: f64, cy: f64, r: f64,
        stops: &[(f32, &str)],
        x: f64, y: f64, w: f64, h: f64,
    ) {
        self.inner.fill_radial_gradient(cx, cy, r, stops, x, y, w, h)
    }
}

// ─── UiEffectHelpers — default impls (no blur in vello-hybrid backend) ────────

impl<'a> UiEffectHelpers for VelloHybridChartRenderContext<'a> {}

// ─── UzorRenderContext ────────────────────────────────────────────────────────

impl<'a> UzorRenderContext for VelloHybridChartRenderContext<'a> {
    fn dpr(&self) -> f64 { self.inner.dpr() }
}

// ─── RenderContextExt — vello-hybrid backend has no blur image ───────────────

impl<'a> RenderContextExt for VelloHybridChartRenderContext<'a> {
    type BlurImage = ();

    fn set_blur_image(&mut self, _image: Option<()>, _width: u32, _height: u32) {
        // vello-hybrid backend does not support blur backgrounds.
    }

    fn set_use_convex_glass_buttons(&mut self, _use_convex: bool) {
        // vello-hybrid backend does not support convex glass buttons.
    }
}

// ─── ChartRenderContext — chart coordinate logic ──────────────────────────────

impl<'a> ChartRenderContext for VelloHybridChartRenderContext<'a> {
    fn chart_width(&self) -> f64 {
        if let Some(ref ovr) = self.coord_override {
            ovr.chart_width
        } else {
            self.viewport.map(|v| v.chart_width).unwrap_or(0.0)
        }
    }

    fn chart_height(&self) -> f64 {
        if let Some(ref ovr) = self.coord_override {
            ovr.chart_height
        } else {
            self.viewport.map(|v| v.chart_height).unwrap_or(0.0)
        }
    }

    fn bar_to_x(&self, bar: f64) -> f64 {
        if let Some(ref ovr) = self.coord_override {
            let offset = bar - ovr.view_start;
            offset * ovr.bar_spacing + ovr.bar_spacing / 2.0
        } else if let Some(viewport) = self.viewport {
            viewport.bar_to_x_f64(bar)
        } else {
            0.0
        }
    }

    fn price_to_y(&self, price: f64) -> f64 {
        if let Some(ref ovr) = self.coord_override {
            let range = ovr.price_max - ovr.price_min;
            if range <= 0.0 {
                return ovr.chart_height / 2.0;
            }
            ovr.chart_height * (1.0 - (price - ovr.price_min) / range)
        } else if let (Some(viewport), Some(price_scale)) = (self.viewport, self.price_scale) {
            let height = viewport.chart_height;
            let range = price_scale.price_max - price_scale.price_min;
            if range <= 0.0 {
                return height / 2.0;
            }
            height * (1.0 - (price - price_scale.price_min) / range)
        } else {
            0.0
        }
    }

    fn set_coordinate_space(
        &mut self,
        chart_width: f64,
        chart_height: f64,
        view_start: f64,
        bar_spacing: f64,
        price_min: f64,
        price_max: f64,
    ) {
        self.coord_override = Some(CoordinateSpaceOverride {
            chart_width,
            chart_height,
            view_start,
            bar_spacing,
            price_min,
            price_max,
        });
    }
}
