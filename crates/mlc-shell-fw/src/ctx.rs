//! A chart render context over the framework's paint target.
//!
//! `ChartApp` paints through `mlc_core::render::RenderContext` (the uzor context plus
//! chart coordinate conversion). Under uzor-framework the paint target is whatever the
//! host gives the panel (`PanelCx::render`: the native render hub, the browser canvas,
//! a tiny-skia pixmap in tests); this wrapper forwards every drawing call to it and
//! keeps the chart coordinate space the way the shell's own contexts do
//! (`mlc-app-render-context-tiny-skia`, whose forwarding it mirrors line for line).

use mlc_core::render::RenderContext as ChartRenderContext;
use mlc_core::{Bar, PriceScale, Viewport};
use uzor::render::{
    BatchPainter, BlendMode, CircleBatch, Effects, GradientPainter, LineSegment, Masking, Painter, RenderContext as UzorRenderContext,
    ShapeHelpers, TextAlign, TextBaseline, TextBounds, TextMetrics, TextRenderer, UiEffectHelpers,
};

/// Per-window coordinate space set by `set_coordinate_space`.
#[derive(Clone, Debug)]
struct CoordinateSpaceOverride {
    chart_width: f64,
    chart_height: f64,
    view_start: f64,
    bar_spacing: f64,
    price_min: f64,
    price_max: f64,
}

/// `ChartApp`'s render context over a borrowed uzor context.
pub struct ChartCtx<'a> {
    inner: &'a mut dyn UzorRenderContext,
    viewport: Option<&'a Viewport>,
    price_scale: Option<&'a PriceScale>,
    coord_override: Option<CoordinateSpaceOverride>,
    bars: Vec<Bar>,
    font: String,
}

thread_local! {
    static WIDTHS: std::cell::RefCell<std::collections::HashMap<(String, String), f64>> = Default::default();
}

/// `w`, the width `text` measured in `font` on the current context — or, on a
/// context that measures nothing (the passes that only lay out and hit-test), the
/// width a painting pass measured for it, so both passes lay out the same boxes.
pub fn measured(font: &str, text: &str, w: f64) -> f64 {
    WIDTHS.with(|m| {
        let mut m = m.borrow_mut();
        if w > 0.0 || text.is_empty() {
            if w > 0.0 && m.len() < 4096 {
                m.insert((font.to_string(), text.to_string()), w);
            }
            w
        } else {
            m.get(&(font.to_string(), text.to_string())).copied().unwrap_or(0.0)
        }
    })
}

impl<'a> ChartCtx<'a> {
    /// Wrap the frame's paint target (no viewport / price scale, as the shell passes).
    pub fn new(inner: &'a mut dyn UzorRenderContext) -> Self {
        ChartCtx { inner, viewport: None, price_scale: None, coord_override: None, bars: Vec::new(), font: String::new() }
    }
}

// ─── Painter ─────────────────────────────────────────────────────────────────

impl<'a> Painter for ChartCtx<'a> {
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

impl<'a> TextRenderer for ChartCtx<'a> {
    fn set_font(&mut self, font: &str) {
        self.font.clear();
        self.font.push_str(font);
        self.inner.set_font(font)
    }
    fn set_text_align(&mut self, align: TextAlign) { self.inner.set_text_align(align) }
    fn set_text_baseline(&mut self, baseline: TextBaseline) { self.inner.set_text_baseline(baseline) }
    fn fill_text(&mut self, text: &str, x: f64, y: f64) { self.inner.fill_text(text, x, y) }
    fn stroke_text(&mut self, text: &str, x: f64, y: f64) { self.inner.stroke_text(text, x, y) }
}

// ─── TextMetrics ──────────────────────────────────────────────────────────────

impl<'a> TextMetrics for ChartCtx<'a> {
    fn measure_text(&self, text: &str) -> f64 { measured(&self.font, text, self.inner.measure_text(text)) }
    fn text_bounds(&self, text: &str, font: &str) -> TextBounds { self.inner.text_bounds(text, font) }
}

// ─── Masking ──────────────────────────────────────────────────────────────────

impl<'a> Masking for ChartCtx<'a> {
    fn clip(&mut self) { self.inner.clip() }
}

// ─── Effects ──────────────────────────────────────────────────────────────────

impl<'a> Effects for ChartCtx<'a> {
    fn set_shadow(&mut self, dx: f64, dy: f64, blur: f64, color: &str) {
        self.inner.set_shadow(dx, dy, blur, color)
    }
    fn clear_shadow(&mut self) { self.inner.clear_shadow() }
    fn set_blend_mode(&mut self, mode: BlendMode) { self.inner.set_blend_mode(mode) }
}

// ─── ShapeHelpers ─────────────────────────────────────────────────────────────

impl<'a> ShapeHelpers for ChartCtx<'a> {
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

impl<'a> BatchPainter for ChartCtx<'a> {
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

impl<'a> GradientPainter for ChartCtx<'a> {
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

// ─── UiEffectHelpers ──────────────────────────────────────────────────────────

impl<'a> UiEffectHelpers for ChartCtx<'a> {
    fn draw_blur_background(&mut self, x: f64, y: f64, width: f64, height: f64) {
        self.inner.draw_blur_background(x, y, width, height)
    }
    fn has_blur_background(&self) -> bool { self.inner.has_blur_background() }
    fn use_convex_glass_buttons(&self) -> bool { self.inner.use_convex_glass_buttons() }
    fn draw_glass_button_3d(
        &mut self,
        x: f64, y: f64, width: f64, height: f64,
        radius: f64, is_active: bool, color: &str,
    ) {
        self.inner.draw_glass_button_3d(x, y, width, height, radius, is_active, color)
    }
}

// ─── UzorRenderContext ────────────────────────────────────────────────────────

impl<'a> UzorRenderContext for ChartCtx<'a> {
    fn dpr(&self) -> f64 { self.inner.dpr() }
    fn image_painter(&mut self) -> Option<&mut dyn uzor::render::ImagePainter> { self.inner.image_painter() }
}

// ─── ChartRenderContext — chart coordinate logic ──────────────────────────────

impl<'a> ChartRenderContext for ChartCtx<'a> {
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

    fn bars(&self) -> &[Bar] {
        &self.bars
    }

    fn set_bars(&mut self, bars: &[Bar]) {
        self.bars.clear();
        self.bars.extend_from_slice(bars);
    }
}
