//! URX CPU chart RenderContext implementation
//!
//! Thin wrapper around `uzor_render_urx::UrxRenderContext` (scene emission)
//! plus `uzor_urx_cpu::CpuBackend` (deterministic CPU rasterisation) that
//! delegates all drawing operations and adds chart-domain coordinate
//! conversion (bar → X, price → Y) on top — the uzor-urx sibling of
//! `mlc-app-render-context-tiny-skia`, same forwarding line for line.
//!
//! This is the CPU deterministic path of the URX family for headless /
//! offscreen chart rendering (the hub's `UrxCpu` arm /
//! `no_adapter_backend_for_family(RenderFamily::Urx)`); the live native
//! window picks its backend through `uzor-render-hub` autodetect
//! (`detect_backend_urx`), which defaults to the URX family.

use mlc_core::{Bar, PriceScale, Viewport};
use mlc_core::render::RenderContext as ChartRenderContext;
use uzor::render::{
    RenderContext as UzorRenderContext, RenderContextExt,
    Painter, TextRenderer, TextMetrics, Masking, Effects,
    ShapeHelpers, BatchPainter, GradientPainter, UiEffectHelpers, ImagePainter,
    TextAlign, TextBaseline, BlendMode, TextBounds,
    LineSegment, CircleBatch,
};
use uzor_render_urx::UrxRenderContext as InnerContext;

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

/// Chart render context backed by the URX CPU rasteriser.
///
/// Drawing emits a `urx_core::Scene`; [`Self::render`] rasterises the
/// buffered scene into the owned pixmap (premultiplied RGBA8) and
/// [`Self::pixels`] reads it back. Deterministic — no GPU, no wall clock.
pub struct UrxChartRenderContext<'a> {
    inner: InnerContext,
    backend: uzor_urx_cpu::backend::CpuBackend,
    pixmap: uzor_urx_cpu::pixmap::Pixmap,
    viewport: Option<&'a Viewport>,
    price_scale: Option<&'a PriceScale>,
    coord_override: Option<CoordinateSpaceOverride>,
    /// Bar slice for timestamp → X conversion in primitives.
    bars: Vec<Bar>,
}

impl<'a> UrxChartRenderContext<'a> {
    /// Create a new context for a frame.
    ///
    /// * `width` — surface width in physical pixels.
    /// * `height` — surface height in physical pixels.
    /// * `dpr` — device pixel ratio.
    /// * `viewport` — optional viewport for bar→X conversion.
    /// * `price_scale` — optional price scale for price→Y conversion.
    pub fn new(
        width: u32,
        height: u32,
        dpr: f64,
        viewport: Option<&'a Viewport>,
        price_scale: Option<&'a PriceScale>,
    ) -> Self {
        let mut inner = InnerContext::new(dpr);
        inner.begin_frame(width, height);
        Self {
            inner,
            backend: uzor_urx_cpu::backend::CpuBackend::new(),
            pixmap: uzor_urx_cpu::pixmap::Pixmap::new(width, height),
            viewport,
            price_scale,
            coord_override: None,
            bars: Vec::new(),
        }
    }

    /// Access the inner scene-emitting context.
    pub fn inner(&self) -> &InnerContext {
        &self.inner
    }

    /// Mutably access the inner scene-emitting context.
    pub fn inner_mut(&mut self) -> &mut InnerContext {
        &mut self.inner
    }

    /// Start a fresh frame (discards buffered draws, resizes the target).
    pub fn begin_frame(&mut self, width: u32, height: u32) {
        self.inner.begin_frame(width, height);
        if self.pixmap.width() != width || self.pixmap.height() != height {
            self.pixmap = uzor_urx_cpu::pixmap::Pixmap::new(width, height);
        }
    }

    /// Rasterise the buffered scene into the pixmap on the CPU. Call once
    /// after painting, before [`Self::pixels`]. The scene is consumed;
    /// paint a new frame with [`Self::begin_frame`] afterwards.
    pub fn render(&mut self) -> Result<(), uzor_urx_cpu::backend::RenderError> {
        let scene = self.inner.take_scene();
        self.pixmap.fill([0, 0, 0, 0]);
        self.backend.render(&scene, &mut self.pixmap)
    }

    /// The rasterised frame (premultiplied RGBA8, row-major).
    pub fn pixels(&self) -> &[u8] {
        self.pixmap.pixels()
    }

    /// Surface width in physical pixels.
    pub fn width(&self) -> u32 { self.pixmap.width() }

    /// Surface height in physical pixels.
    pub fn height(&self) -> u32 { self.pixmap.height() }
}

// ─── Painter ─────────────────────────────────────────────────────────────────

impl<'a> Painter for UrxChartRenderContext<'a> {
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

impl<'a> TextRenderer for UrxChartRenderContext<'a> {
    fn set_font(&mut self, font: &str) { self.inner.set_font(font) }
    fn set_text_align(&mut self, align: TextAlign) { self.inner.set_text_align(align) }
    fn set_text_baseline(&mut self, baseline: TextBaseline) { self.inner.set_text_baseline(baseline) }
    fn fill_text(&mut self, text: &str, x: f64, y: f64) { self.inner.fill_text(text, x, y) }
    fn stroke_text(&mut self, text: &str, x: f64, y: f64) { self.inner.stroke_text(text, x, y) }
}

// ─── TextMetrics ──────────────────────────────────────────────────────────────

impl<'a> TextMetrics for UrxChartRenderContext<'a> {
    fn measure_text(&self, text: &str) -> f64 { self.inner.measure_text(text) }
    fn text_bounds(&self, text: &str, font: &str) -> TextBounds { self.inner.text_bounds(text, font) }
    fn measure_text_glyphs(&self, text: &str, font: &str) -> Vec<uzor::render::GlyphMetric> {
        self.inner.measure_text_glyphs(text, font)
    }
    fn measure_text_wrapped(&self, text: &str, font: &str, max_width: f64) -> Vec<uzor::render::WrappedLine> {
        self.inner.measure_text_wrapped(text, font, max_width)
    }
    fn text_to_path(&self, text: &str, font: &str) -> String { self.inner.text_to_path(text, font) }
}

// ─── Masking ──────────────────────────────────────────────────────────────────

impl<'a> Masking for UrxChartRenderContext<'a> {
    fn clip(&mut self) { self.inner.clip() }
}

// ─── Effects ──────────────────────────────────────────────────────────────────

impl<'a> Effects for UrxChartRenderContext<'a> {
    fn set_shadow(&mut self, dx: f64, dy: f64, blur: f64, color: &str) {
        self.inner.set_shadow(dx, dy, blur, color)
    }
    fn clear_shadow(&mut self) { self.inner.clear_shadow() }
    fn set_blend_mode(&mut self, mode: BlendMode) { self.inner.set_blend_mode(mode) }
}

// ─── ShapeHelpers ─────────────────────────────────────────────────────────────

impl<'a> ShapeHelpers for UrxChartRenderContext<'a> {
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

impl<'a> BatchPainter for UrxChartRenderContext<'a> {
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

impl<'a> GradientPainter for UrxChartRenderContext<'a> {
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

impl<'a> UiEffectHelpers for UrxChartRenderContext<'a> {
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

// ─── ImagePainter (opt-in capability — URX composites raster images) ─────────

impl<'a> ImagePainter for UrxChartRenderContext<'a> {
    fn draw_image(&mut self, image_id: &str, x: f64, y: f64, width: f64, height: f64) -> bool {
        self.inner.draw_image(image_id, x, y, width, height)
    }
    fn draw_image_rgba(&mut self, data: &[u8], img_width: u32, img_height: u32, x: f64, y: f64, width: f64, height: f64) {
        self.inner.draw_image_rgba(data, img_width, img_height, x, y, width, height)
    }
    fn draw_image_rgba_src(
        &mut self, data: &[u8], img_width: u32, img_height: u32,
        sx: f64, sy: f64, sw: f64, sh: f64,
        x: f64, y: f64, w: f64, h: f64,
    ) -> bool {
        self.inner.draw_image_rgba_src(data, img_width, img_height, sx, sy, sw, sh, x, y, w, h)
    }
}

// ─── UzorRenderContext ────────────────────────────────────────────────────────

impl<'a> UzorRenderContext for UrxChartRenderContext<'a> {
    fn dpr(&self) -> f64 { self.inner.dpr() }

    fn image_painter(&mut self) -> Option<&mut dyn ImagePainter> {
        Some(self)
    }
}

// ─── RenderContextExt — URX CPU has no blur image ────────────────────────────

impl<'a> RenderContextExt for UrxChartRenderContext<'a> {
    type BlurImage = ();

    fn set_blur_image(&mut self, _image: Option<()>, _width: u32, _height: u32) {
        // CPU backend does not support blur backgrounds.
    }

    fn set_use_convex_glass_buttons(&mut self, _use_convex: bool) {
        // CPU backend does not support convex glass buttons.
    }
}

// ─── ChartRenderContext — chart coordinate logic ──────────────────────────────

impl<'a> ChartRenderContext for UrxChartRenderContext<'a> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use uzor::render::{Painter, ShapeHelpers, TextRenderer, TextBaseline};

    /// The wrapper paints, rasterises on the CPU and reads back ink —
    /// the deterministic headless path end to end.
    #[test]
    fn paints_and_rasterises_a_chart_like_frame() {
        let mut ctx = UrxChartRenderContext::new(120, 80, 1.0, None, None);
        ctx.set_fill_color("#131722");
        ctx.fill_rect(0.0, 0.0, 120.0, 80.0);
        ctx.set_fill_color("#2962ff");
        ctx.fill_rect(10.0, 20.0, 40.0, 30.0);
        ctx.set_stroke_color("#ef5350");
        ctx.set_stroke_width(1.0);
        ctx.stroke_rect(60.5, 10.5, 30.0, 50.0);
        ctx.set_fill_color("#e6e6eb");
        ctx.set_font("12px Roboto");
        ctx.set_text_baseline(TextBaseline::Alphabetic);
        ctx.fill_text("BTCUSDT", 10.0, 70.0);
        ctx.render().expect("urx cpu render");
        assert_eq!(ctx.pixels().len(), 120 * 80 * 4);
        let blue = ctx.pixels().chunks_exact(4).enumerate()
            .filter(|(i, c)| { let (x, y) = (i % 120, i / 120); (12..48).contains(&x) && (22..48).contains(&y) && c[2] > 100 })
            .count();
        assert!(blue > 500, "the fill rect rasterised, got {blue} px");
        let ink = ctx.pixels().chunks_exact(4).filter(|c| c[0] > 150 && c[1] > 150 && c[2] > 150).count();
        assert!(ink > 50, "the label rasterised, got {ink} px");
    }

    /// A second frame reuses the context (begin_frame resets the scene).
    #[test]
    fn begin_frame_reuses_the_context() {
        let mut ctx = UrxChartRenderContext::new(60, 40, 1.0, None, None);
        ctx.set_fill_color("#ff0000");
        ctx.fill_rect(0.0, 0.0, 60.0, 40.0);
        ctx.render().expect("first frame");
        ctx.begin_frame(60, 40);
        ctx.set_fill_color("#00ff00");
        ctx.fill_rect(0.0, 0.0, 60.0, 40.0);
        ctx.render().expect("second frame");
        let c = &ctx.pixels()[0..4];
        assert!(c[1] > 200 && c[0] < 50, "second frame is green, got {c:?}");
    }
}
