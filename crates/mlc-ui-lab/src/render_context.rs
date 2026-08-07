//! `LabRenderContext` — chart-domain wrapper around
//! `uzor_render_canvas2d::Canvas2dRenderContext`.
//!
//! Implements `mlc_core::render::RenderContext` (the chart-extended trait)
//! by delegating all drawing primitives to the uzor canvas2d backend and
//! adding the bar/price coordinate conversion the trait requires.
//!
//! This is the same adapter shape as
//! `mlc-app-shell-web/src/context.rs::Canvas2dChartRenderContext` (and the
//! sibling `mlc-app-render-context-{tiny-skia,vello-cpu}` backends) — every
//! consumer of `uzor-render-canvas2d` writes one of these, because the two
//! trait surfaces (uzor's generic `RenderContext` and mlc-core's
//! chart-domain extension) live in different crates. The lab does not
//! render bar/candle geometry, so `bar_to_x` / `price_to_y` are simple
//! placeholders — the primitive-settings modal never calls them (it is
//! chrome, not chart geometry) — but the trait requires the methods to
//! exist regardless.

use mlc_core::{render::RenderContext as ChartRenderContext, Bar};
use uzor::render::{
    BatchPainter, BlendMode, Effects, GradientPainter, Masking, Painter, RenderContext as UzorRenderContext,
    RenderContextExt, ShapeHelpers, TextAlign, TextBaseline, TextBounds, TextMetrics, TextRenderer,
    UiEffectHelpers,
};
use uzor_render_canvas2d::Canvas2dRenderContext;
use web_sys::CanvasRenderingContext2d;

/// Chart render context backed by the browser's Canvas 2D API.
pub struct LabRenderContext {
    inner: Canvas2dRenderContext,
    chart_width: f64,
    chart_height: f64,
    canvas_width: f64,
    canvas_height: f64,
    bars: Vec<Bar>,
}

impl LabRenderContext {
    /// `raw` — the browser 2D context (DPR transform already applied by the caller).
    pub fn new(raw: CanvasRenderingContext2d, dpr: f64, logical_w: u32, logical_h: u32) -> Self {
        let w = logical_w as f64;
        let h = logical_h as f64;
        Self {
            inner: Canvas2dRenderContext::new(raw, dpr),
            chart_width: w,
            chart_height: h,
            canvas_width: w,
            canvas_height: h,
            bars: Vec::new(),
        }
    }
}

impl Painter for LabRenderContext {
    fn save(&mut self) { self.inner.save(); }
    fn restore(&mut self) { self.inner.restore(); }
    fn translate(&mut self, x: f64, y: f64) { self.inner.translate(x, y); }
    fn rotate(&mut self, angle: f64) { self.inner.rotate(angle); }
    fn scale(&mut self, x: f64, y: f64) { self.inner.scale(x, y); }
    fn set_fill_color(&mut self, color: &str) { self.inner.set_fill_color(color); }
    fn set_global_alpha(&mut self, alpha: f64) { self.inner.set_global_alpha(alpha); }
    fn set_stroke_color(&mut self, color: &str) { self.inner.set_stroke_color(color); }
    fn set_stroke_width(&mut self, width: f64) { self.inner.set_stroke_width(width); }
    fn set_line_dash(&mut self, pattern: &[f64]) { self.inner.set_line_dash(pattern); }
    fn set_line_cap(&mut self, cap: &str) { self.inner.set_line_cap(cap); }
    fn set_line_join(&mut self, join: &str) { self.inner.set_line_join(join); }
    fn begin_path(&mut self) { self.inner.begin_path(); }
    fn move_to(&mut self, x: f64, y: f64) { self.inner.move_to(x, y); }
    fn line_to(&mut self, x: f64, y: f64) { self.inner.line_to(x, y); }
    fn close_path(&mut self) { self.inner.close_path(); }
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) { self.inner.rect(x, y, w, h); }
    fn arc(&mut self, cx: f64, cy: f64, radius: f64, sa: f64, ea: f64) {
        self.inner.arc(cx, cy, radius, sa, ea);
    }
    fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64, rot: f64, sa: f64, ea: f64) {
        self.inner.ellipse(cx, cy, rx, ry, rot, sa, ea);
    }
    fn quadratic_curve_to(&mut self, cpx: f64, cpy: f64, x: f64, y: f64) {
        self.inner.quadratic_curve_to(cpx, cpy, x, y);
    }
    fn bezier_curve_to(&mut self, cp1x: f64, cp1y: f64, cp2x: f64, cp2y: f64, x: f64, y: f64) {
        self.inner.bezier_curve_to(cp1x, cp1y, cp2x, cp2y, x, y);
    }
    fn stroke(&mut self) { self.inner.stroke(); }
    fn fill(&mut self) { self.inner.fill(); }
}

impl TextRenderer for LabRenderContext {
    fn set_font(&mut self, font: &str) { self.inner.set_font(font); }
    fn set_text_align(&mut self, align: TextAlign) { self.inner.set_text_align(align); }
    fn set_text_baseline(&mut self, baseline: TextBaseline) { self.inner.set_text_baseline(baseline); }
    fn fill_text(&mut self, text: &str, x: f64, y: f64) { self.inner.fill_text(text, x, y); }
    fn stroke_text(&mut self, text: &str, x: f64, y: f64) { self.inner.stroke_text(text, x, y); }
}

impl TextMetrics for LabRenderContext {
    fn measure_text(&self, text: &str) -> f64 { self.inner.measure_text(text) }
    fn text_bounds(&self, text: &str, font: &str) -> TextBounds { self.inner.text_bounds(text, font) }
}

impl Masking for LabRenderContext {
    fn clip(&mut self) { self.inner.clip(); }
}

impl Effects for LabRenderContext {
    fn set_shadow(&mut self, dx: f64, dy: f64, blur: f64, color: &str) {
        self.inner.set_shadow(dx, dy, blur, color);
    }
    fn clear_shadow(&mut self) { self.inner.clear_shadow(); }
    fn set_blend_mode(&mut self, mode: BlendMode) { self.inner.set_blend_mode(mode); }
}

impl ShapeHelpers for LabRenderContext {
    fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) { self.inner.fill_rect(x, y, w, h); }
    fn stroke_rect(&mut self, x: f64, y: f64, w: f64, h: f64) { self.inner.stroke_rect(x, y, w, h); }
    fn rounded_rect_corners(&mut self, x: f64, y: f64, w: f64, h: f64, tl: f64, tr: f64, br: f64, bl: f64) {
        self.inner.rounded_rect_corners(x, y, w, h, tl, tr, br, bl);
    }
}

impl GradientPainter for LabRenderContext {
    fn fill_linear_gradient(&mut self, stops: &[(f32, &str)], x1: f64, y1: f64, x2: f64, y2: f64) {
        self.inner.fill_linear_gradient(stops, x1, y1, x2, y2);
    }
    fn fill_radial_gradient(&mut self, cx: f64, cy: f64, r: f64, stops: &[(f32, &str)], x: f64, y: f64, w: f64, h: f64) {
        self.inner.fill_radial_gradient(cx, cy, r, stops, x, y, w, h);
    }
}

impl UiEffectHelpers for LabRenderContext {}
impl BatchPainter for LabRenderContext {}

impl UzorRenderContext for LabRenderContext {
    fn dpr(&self) -> f64 { self.inner.dpr() }
}

impl RenderContextExt for LabRenderContext {
    type BlurImage = ();
}

impl ChartRenderContext for LabRenderContext {
    fn chart_width(&self) -> f64 { self.chart_width }
    fn chart_height(&self) -> f64 { self.chart_height }
    fn canvas_width(&self) -> f64 { self.canvas_width }
    fn canvas_height(&self) -> f64 { self.canvas_height }

    // No chart geometry in the lab — the modal never calls these (it draws
    // chrome only), so a flat identity mapping is honest rather than a fake
    // viewport.
    fn bar_to_x(&self, bar: f64) -> f64 { bar }
    fn price_to_y(&self, price: f64) -> f64 { price }

    fn bars(&self) -> &[Bar] { &self.bars }
    fn set_bars(&mut self, bars: &[Bar]) { self.bars = bars.to_vec(); }

    fn set_coordinate_space(
        &mut self,
        chart_width: f64,
        chart_height: f64,
        _view_start: f64,
        _bar_spacing: f64,
        _price_min: f64,
        _price_max: f64,
    ) {
        self.chart_width = chart_width;
        self.chart_height = chart_height;
    }
}
