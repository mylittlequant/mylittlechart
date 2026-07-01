//! `Canvas2dChartRenderContext` — chart-domain wrapper around
//! `uzor_render_canvas2d::Canvas2dRenderContext`.
//!
//! Implements `mlc_core::render::RenderContext` (the chart-extended trait)
//! by delegating all drawing primitives to the uzor canvas2d backend and
//! adding coordinate conversion (bar→X, price→Y) from the viewport parameters
//! supplied via `set_coordinate_space`.
//!
//! # Design note
//!
//! The pattern mirrors `mlc-app-render-context-tiny-skia` and `mlc-app-render-context-vello-cpu`.
//! The only wasm-specific part is using `CanvasRenderingContext2d` as the
//! underlying surface instead of a CPU pixmap.

use mlc_core::{Bar, render::RenderContext as ChartRenderContext};
use uzor::render::{
    BatchPainter, BlendMode, Effects, GradientPainter, Masking, Painter,
    RenderContext as UzorRenderContext, RenderContextExt, ShapeHelpers,
    TextAlign, TextBaseline, TextBounds, TextMetrics, TextRenderer,
    UiEffectHelpers,
};
use uzor_render_canvas2d::Canvas2dRenderContext;
use web_sys::CanvasRenderingContext2d;

// ─── Coordinate-space parameters ──────────────────────────────────────────────

/// Viewport parameters injected by `set_coordinate_space`.
///
/// When present, these override any stale viewport reference held before the
/// frame.  The chart engine calls `set_coordinate_space` once per window before
/// drawing primitives so this is always fresh for the current sub-pane.
#[derive(Clone, Debug, Default)]
struct CoordSpace {
    chart_width: f64,
    chart_height: f64,
    view_start: f64,
    bar_spacing: f64,
    price_min: f64,
    price_max: f64,
}

impl CoordSpace {
    fn bar_to_x(&self, bar: f64) -> f64 {
        if self.bar_spacing.abs() < f64::EPSILON {
            return 0.0;
        }
        (bar - self.view_start) * self.bar_spacing
    }

    fn price_to_y(&self, price: f64) -> f64 {
        if (self.price_max - self.price_min).abs() < f64::EPSILON {
            return self.chart_height / 2.0;
        }
        let normalized = (self.price_max - price) / (self.price_max - self.price_min);
        normalized * self.chart_height
    }
}

// ─── Public context type ───────────────────────────────────────────────────────

/// Chart render context backed by the browser's Canvas 2D API.
///
/// Delegates all uzor drawing calls to [`Canvas2dRenderContext`] and provides
/// chart-domain coordinate conversion on top.
pub struct Canvas2dChartRenderContext {
    inner: Canvas2dRenderContext,
    coord: CoordSpace,
    bars: Vec<Bar>,
    canvas_width: f64,
    canvas_height: f64,
}

impl Canvas2dChartRenderContext {
    /// Construct from a raw `CanvasRenderingContext2d`.
    ///
    /// * `raw` — the browser 2D context (already has the DPR transform applied
    ///   by the caller in `lib.rs`).
    /// * `dpr` — device pixel ratio.
    /// * `logical_w / logical_h` — CSS pixel dimensions (used for canvas_width
    ///   / canvas_height and as the initial `chart_width / chart_height`).
    pub fn new(
        raw: CanvasRenderingContext2d,
        dpr: f64,
        logical_w: u32,
        logical_h: u32,
    ) -> Self {
        let w = logical_w as f64;
        let h = logical_h as f64;
        Self {
            inner: Canvas2dRenderContext::new(raw, dpr),
            coord: CoordSpace {
                chart_width: w,
                chart_height: h,
                ..CoordSpace::default()
            },
            bars: Vec::new(),
            canvas_width: w,
            canvas_height: h,
        }
    }
}

// ─── Delegation: uzor drawing traits ──────────────────────────────────────────

impl Painter for Canvas2dChartRenderContext {
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

impl TextRenderer for Canvas2dChartRenderContext {
    fn set_font(&mut self, font: &str) { self.inner.set_font(font); }
    fn set_text_align(&mut self, align: TextAlign) { self.inner.set_text_align(align); }
    fn set_text_baseline(&mut self, baseline: TextBaseline) { self.inner.set_text_baseline(baseline); }
    fn fill_text(&mut self, text: &str, x: f64, y: f64) { self.inner.fill_text(text, x, y); }
    fn stroke_text(&mut self, text: &str, x: f64, y: f64) { self.inner.stroke_text(text, x, y); }
}

impl TextMetrics for Canvas2dChartRenderContext {
    fn measure_text(&self, text: &str) -> f64 { self.inner.measure_text(text) }
    fn text_bounds(&self, text: &str, font: &str) -> TextBounds { self.inner.text_bounds(text, font) }
}

impl Masking for Canvas2dChartRenderContext {
    fn clip(&mut self) { self.inner.clip(); }
}

impl Effects for Canvas2dChartRenderContext {
    fn set_shadow(&mut self, dx: f64, dy: f64, blur: f64, color: &str) {
        self.inner.set_shadow(dx, dy, blur, color);
    }
    fn clear_shadow(&mut self) { self.inner.clear_shadow(); }
    fn set_blend_mode(&mut self, mode: BlendMode) { self.inner.set_blend_mode(mode); }
}

impl ShapeHelpers for Canvas2dChartRenderContext {
    fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) { self.inner.fill_rect(x, y, w, h); }
    fn stroke_rect(&mut self, x: f64, y: f64, w: f64, h: f64) { self.inner.stroke_rect(x, y, w, h); }
    fn rounded_rect_corners(&mut self, x: f64, y: f64, w: f64, h: f64,
                             tl: f64, tr: f64, br: f64, bl: f64) {
        self.inner.rounded_rect_corners(x, y, w, h, tl, tr, br, bl);
    }
}

impl GradientPainter for Canvas2dChartRenderContext {
    fn fill_linear_gradient(&mut self, stops: &[(f32, &str)], x1: f64, y1: f64, x2: f64, y2: f64) {
        self.inner.fill_linear_gradient(stops, x1, y1, x2, y2);
    }
    fn fill_radial_gradient(&mut self, cx: f64, cy: f64, r: f64, stops: &[(f32, &str)],
                             x: f64, y: f64, w: f64, h: f64) {
        self.inner.fill_radial_gradient(cx, cy, r, stops, x, y, w, h);
    }
}

impl UiEffectHelpers for Canvas2dChartRenderContext {}

impl BatchPainter for Canvas2dChartRenderContext {}

// ─── uzor base RenderContext ───────────────────────────────────────────────────

impl UzorRenderContext for Canvas2dChartRenderContext {
    fn dpr(&self) -> f64 { self.inner.dpr() }
}

impl RenderContextExt for Canvas2dChartRenderContext {
    type BlurImage = ();
}

// ─── mlc_core::render::RenderContext ──────────────────────────────────────────

impl ChartRenderContext for Canvas2dChartRenderContext {
    fn chart_width(&self) -> f64 { self.coord.chart_width }
    fn chart_height(&self) -> f64 { self.coord.chart_height }
    fn canvas_width(&self) -> f64 { self.canvas_width }
    fn canvas_height(&self) -> f64 { self.canvas_height }

    fn bar_to_x(&self, bar: f64) -> f64 { self.coord.bar_to_x(bar) }
    fn price_to_y(&self, price: f64) -> f64 { self.coord.price_to_y(price) }

    fn bars(&self) -> &[Bar] { &self.bars }
    fn set_bars(&mut self, bars: &[Bar]) { self.bars = bars.to_vec(); }

    fn set_coordinate_space(
        &mut self,
        chart_width: f64,
        chart_height: f64,
        view_start: f64,
        bar_spacing: f64,
        price_min: f64,
        price_max: f64,
    ) {
        self.coord = CoordSpace { chart_width, chart_height, view_start, bar_spacing, price_min, price_max };
    }
}
