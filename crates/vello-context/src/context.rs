//! Vello RenderContext implementation for vello 0.6
//!
//! Thin wrapper around `uzor_backend_vello_gpu::VelloGpuRenderContext` that
//! delegates all drawing operations to the GPU backend and adds chart-domain
//! coordinate conversion (bar → X, price → Y) on top.

use vello::Scene;
use zengeld_chart::{PriceScale, Viewport};
use zengeld_chart::render::RenderContext as ChartRenderContext;
use uzor::render::{RenderContext as UzorRenderContext, RenderContextExt, TextAlign, TextBaseline};
use uzor_backend_vello_gpu::VelloGpuRenderContext as InnerContext;

/// Vello color type alias — same underlying type as the GPU backend.
pub type Color = vello::peniko::color::AlphaColor<vello::peniko::color::Srgb>;

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

/// Chart render context backed by the vello GPU renderer.
///
/// All drawing primitives are delegated to [`uzor_backend_vello_gpu::VelloGpuRenderContext`].
/// This type only adds chart-domain coordinate conversion on top.
pub struct VelloGpuRenderContext<'a> {
    inner: InnerContext<'a>,
    viewport: Option<&'a Viewport>,
    price_scale: Option<&'a PriceScale>,
    coord_override: Option<CoordinateSpaceOverride>,
}

impl<'a> VelloGpuRenderContext<'a> {
    /// Create a new context.
    ///
    /// * `scene` — vello scene to render into.
    /// * `chart_rect_x / chart_rect_y` — canvas offset applied to all draw calls.
    /// * `viewport` — optional viewport for bar→X conversion.
    /// * `price_scale` — optional price scale for price→Y conversion.
    pub fn new(
        scene: &'a mut Scene,
        chart_rect_x: f64,
        chart_rect_y: f64,
        viewport: Option<&'a Viewport>,
        price_scale: Option<&'a PriceScale>,
    ) -> Self {
        Self {
            inner: InnerContext::new(scene, chart_rect_x, chart_rect_y),
            viewport,
            price_scale,
            coord_override: None,
        }
    }
}

// ─── UzorRenderContext — forward every method to inner ───────────────────────

impl<'a> UzorRenderContext for VelloGpuRenderContext<'a> {
    fn dpr(&self) -> f64 { self.inner.dpr() }

    // Stroke style
    fn set_stroke_color(&mut self, color: &str) { self.inner.set_stroke_color(color) }
    fn set_stroke_width(&mut self, width: f64) { self.inner.set_stroke_width(width) }
    fn set_line_dash(&mut self, pattern: &[f64]) { self.inner.set_line_dash(pattern) }
    fn set_line_cap(&mut self, cap: &str) { self.inner.set_line_cap(cap) }
    fn set_line_join(&mut self, join: &str) { self.inner.set_line_join(join) }

    // Fill style
    fn set_fill_color(&mut self, color: &str) { self.inner.set_fill_color(color) }
    fn set_global_alpha(&mut self, alpha: f64) { self.inner.set_global_alpha(alpha) }

    // Path operations
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

    // Stroke/fill/clip
    fn stroke(&mut self) { self.inner.stroke() }
    fn fill(&mut self) { self.inner.fill() }
    fn clip(&mut self) { self.inner.clip() }

    // Shape helpers
    fn stroke_rect(&mut self, x: f64, y: f64, w: f64, h: f64) { self.inner.stroke_rect(x, y, w, h) }
    fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) { self.inner.fill_rect(x, y, w, h) }

    // Text
    fn set_font(&mut self, font: &str) { self.inner.set_font(font) }
    fn set_text_align(&mut self, align: TextAlign) { self.inner.set_text_align(align) }
    fn set_text_baseline(&mut self, baseline: TextBaseline) { self.inner.set_text_baseline(baseline) }
    fn fill_text(&mut self, text: &str, x: f64, y: f64) { self.inner.fill_text(text, x, y) }
    fn stroke_text(&mut self, text: &str, x: f64, y: f64) { self.inner.stroke_text(text, x, y) }
    fn measure_text(&self, text: &str) -> f64 { self.inner.measure_text(text) }

    /// Override to use the inner context's corrected rotation implementation.
    fn fill_text_rotated(&mut self, text: &str, x: f64, y: f64, angle: f64) {
        self.inner.fill_text_rotated(text, x, y, angle)
    }

    // Transform
    fn save(&mut self) { self.inner.save() }
    fn restore(&mut self) { self.inner.restore() }
    fn translate(&mut self, x: f64, y: f64) { self.inner.translate(x, y) }
    fn rotate(&mut self, angle: f64) { self.inner.rotate(angle) }
    fn scale(&mut self, x: f64, y: f64) { self.inner.scale(x, y) }

    // Images
    fn draw_image_rgba(
        &mut self,
        data: &[u8],
        img_width: u32,
        img_height: u32,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) {
        self.inner.draw_image_rgba(data, img_width, img_height, x, y, width, height)
    }

    // Blur / glass effects
    fn draw_blur_background(&mut self, x: f64, y: f64, width: f64, height: f64) {
        self.inner.draw_blur_background(x, y, width, height)
    }
    fn has_blur_background(&self) -> bool { self.inner.has_blur_background() }
    fn use_convex_glass_buttons(&self) -> bool { self.inner.use_convex_glass_buttons() }
    fn draw_glass_button_3d(
        &mut self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f64,
        is_active: bool,
        color: &str,
    ) {
        self.inner.draw_glass_button_3d(x, y, width, height, radius, is_active, color)
    }
}

// ─── RenderContextExt — forward to inner ─────────────────────────────────────

impl<'a> RenderContextExt for VelloGpuRenderContext<'a> {
    type BlurImage = <InnerContext<'a> as RenderContextExt>::BlurImage;

    fn set_blur_image(&mut self, image: Option<Self::BlurImage>, width: u32, height: u32) {
        self.inner.set_blur_image(image, width, height)
    }

    fn set_use_convex_glass_buttons(&mut self, use_convex: bool) {
        self.inner.set_use_convex_glass_buttons(use_convex)
    }
}

// ─── ChartRenderContext — chart coordinate logic ──────────────────────────────

impl<'a> ChartRenderContext for VelloGpuRenderContext<'a> {
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
