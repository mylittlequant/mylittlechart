//! Shared plumbing for the cloud DOM trees: the paint-nothing geometry
//! context, the window-space shell lift, and the fixed-box / half-alpha
//! helpers the calibrated shells are built from.

use mlc_core::render::RenderContext as ChartRenderContext;
use uzor::render::{
    BatchPainter, Effects, GradientPainter, Masking, Painter, ShapeHelpers, TextAlign, TextBaseline,
    TextBounds, TextMetrics, TextRenderer, UiEffectHelpers,
};

/// A render context that paints nothing and measures nothing: the shell's
/// geometry pass only needs its rect arithmetic (the cloud surfaces' tabs
/// are `TabFit::Equal`, their titles static — nothing here measures text).
/// `mlc_app::paint_off::PaintOff` is the same idea one crate up, where the
/// real context is still needed for measurement; this one is pure geometry.
struct NullCtx;

impl Painter for NullCtx {
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _x: f64, _y: f64) {}
    fn rotate(&mut self, _a: f64) {}
    fn scale(&mut self, _x: f64, _y: f64) {}
    fn set_fill_color(&mut self, _c: &str) {}
    fn set_global_alpha(&mut self, _a: f64) {}
    fn set_stroke_color(&mut self, _c: &str) {}
    fn set_stroke_width(&mut self, _w: f64) {}
    fn set_line_dash(&mut self, _p: &[f64]) {}
    fn set_line_cap(&mut self, _c: &str) {}
    fn set_line_join(&mut self, _j: &str) {}
    fn begin_path(&mut self) {}
    fn move_to(&mut self, _x: f64, _y: f64) {}
    fn line_to(&mut self, _x: f64, _y: f64) {}
    fn close_path(&mut self) {}
    fn rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    fn arc(&mut self, _cx: f64, _cy: f64, _r: f64, _s: f64, _e: f64) {}
    fn ellipse(&mut self, _cx: f64, _cy: f64, _rx: f64, _ry: f64, _rot: f64, _s: f64, _e: f64) {}
    fn quadratic_curve_to(&mut self, _a: f64, _b: f64, _c: f64, _d: f64) {}
    fn bezier_curve_to(&mut self, _a: f64, _b: f64, _c: f64, _d: f64, _e: f64, _f: f64) {}
    fn stroke(&mut self) {}
    fn fill(&mut self) {}
}
impl TextRenderer for NullCtx {
    fn set_font(&mut self, _f: &str) {}
    fn set_text_align(&mut self, _a: TextAlign) {}
    fn set_text_baseline(&mut self, _b: TextBaseline) {}
    fn fill_text(&mut self, _t: &str, _x: f64, _y: f64) {}
    fn stroke_text(&mut self, _t: &str, _x: f64, _y: f64) {}
}
impl TextMetrics for NullCtx {
    fn measure_text(&self, _t: &str) -> f64 {
        0.0
    }
    fn text_bounds(&self, _t: &str, _f: &str) -> TextBounds {
        TextBounds { x: 0.0, y: 0.0, w: 0.0, h: 0.0, ascent: 0.0, descent: 0.0 }
    }
}
impl Masking for NullCtx {
    fn clip(&mut self) {}
}
impl Effects for NullCtx {}
impl ShapeHelpers for NullCtx {
    fn stroke_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    fn fill_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
}
impl GradientPainter for NullCtx {}
impl UiEffectHelpers for NullCtx {}
impl BatchPainter for NullCtx {}
impl uzor::render::RenderContext for NullCtx {
    fn dpr(&self) -> f64 {
        1.0
    }
}
impl ChartRenderContext for NullCtx {
    fn chart_width(&self) -> f64 {
        0.0
    }
    fn chart_height(&self) -> f64 {
        0.0
    }
    fn bar_to_x(&self, _bar: f64) -> f64 {
        0.0
    }
    fn price_to_y(&self, _price: f64) -> f64 {
        0.0
    }
    fn set_coordinate_space(&mut self, _w: f64, _h: f64, _vs: f64, _bs: f64, _pmin: f64, _pmax: f64) {}
}

/// The shell geometry the app's own `begin_modal_shell` settles on for
/// `spec`, in WINDOW coordinates: run it over [`NullCtx`] (paint nothing)
/// with a throwaway coordinator, then lift the rects out of the chart space
/// the shell works in (origin at the chrome strip's bottom edge).
pub(crate) fn geometry(
    spec: &crate::ui::widgets::ModalShellSpec<'_>,
    window: uzor::types::Rect,
    chrome_h: f64,
) -> crate::ui::widgets::ModalShell {
    let theme = crate::ui::widgets::ModalTheme::default();
    let mut coord = uzor::input::InputCoordinator::default();
    let (shell, _layer) = crate::ui::widgets::begin_modal_shell(
        &mut NullCtx,
        &mut coord,
        spec,
        &theme,
        (window.width, window.height - chrome_h),
    );
    let lift = |r: uzor::types::Rect| {
        uzor::types::Rect::new(r.x + window.x, r.y + window.y + chrome_h, r.width, r.height)
    };
    crate::ui::widgets::ModalShell {
        frame: lift(shell.frame),
        header: lift(shell.header),
        content: lift(shell.content),
        overflow: shell.overflow,
        tabs: shell.tabs.map(lift),
        tab_rects: shell.tab_rects.into_iter().map(|(id, r)| (id, lift(r))).collect(),
        close_btn: shell.close_btn.map(lift),
        footer: shell.footer.map(lift),
        template_btn: shell.template_btn.map(lift),
        primary_btn: shell.primary_btn.map(lift),
    }
}

/// The legacy 1 px stroke's anti-aliased spread as a DOM colour: a stroke
/// centred on an integer edge covers half of each neighbouring pixel row,
/// which a 2 px box at half the alpha reproduces exactly (the chart-settings
/// DOM shell's own border trick).
pub(crate) fn half(color: &str) -> String {
    let (r, g, b, a) = mlc_core::parse_css_color(color);
    format!("rgba({r},{g},{b},{:.3})", a as f64 / 255.0 * 0.5)
}

/// A quarter of the colour's alpha — the legacy stroke's miter corner
/// covers a quarter of the outer corner pixel.
fn quarter(color: &str) -> String {
    let (r, g, b, a) = mlc_core::parse_css_color(color);
    format!("rgba({r},{g},{b},{:.3})", a as f64 / 255.0 * 0.25)
}

/// The legacy `stroke_rect(frame, 1px)` as DOM boxes, pixel-exact including
/// the corners: four 2 px half-alpha strips centred on the edges (each
/// neighbouring pixel row/column gets half coverage), plus 1 px quarter-alpha
/// outer corners (the miter join's quarter coverage). The inner corner pixel
/// gets half from each of its two strips — three quarters combined, exactly
/// the stroked L-band's area. A CSS border's own miter corner would paint
/// that outer pixel at half instead of a quarter (the 16 px corner residual
/// this replaces).
pub(crate) fn frame_border(f: uzor::types::Rect, line: &str) -> Vec<uzor::ui::dom::El> {
    let h = half(line);
    let q = quarter(line);
    let strip = |r: uzor::types::Rect| fixed("co-border", 150, r, &format!("background: {h}"));
    let corner = |x: f64, y: f64| {
        fixed("co-border", 150, uzor::types::Rect::new(x, y, 1.0, 1.0), &format!("background: {q}"))
    };
    vec![
        strip(uzor::types::Rect::new(f.x - 1.0, f.y, 2.0, f.height)),
        strip(uzor::types::Rect::new(f.right() - 1.0, f.y, 2.0, f.height)),
        strip(uzor::types::Rect::new(f.x, f.y - 1.0, f.width, 2.0)),
        strip(uzor::types::Rect::new(f.x, f.bottom() - 1.0, f.width, 2.0)),
        corner(f.x - 1.0, f.y - 1.0),
        corner(f.right(), f.y - 1.0),
        corner(f.x - 1.0, f.bottom()),
        corner(f.right(), f.bottom()),
    ]
}

/// A legacy `hairline(x0, y, x1)` stroke as DOM boxes: the 2 px half-alpha
/// row centred on the edge, plus 1×2 px quarter-alpha boxes at both ends —
/// the stroke inherits a round line cap from the chrome painted before it,
/// and the cap's half-pixel spill covers about a quarter of each outer end
/// pixel (measured: the news overlay's 12 px hairline-end residual).
pub(crate) fn hairline(x0: f64, y: f64, x1: f64, line: &str) -> Vec<uzor::ui::dom::El> {
    let h = half(line);
    let q = quarter(line);
    vec![
        fixed("co-rule", 151, uzor::types::Rect::new(x0, y - 1.0, x1 - x0, 2.0), &format!("background: {h}")),
        fixed("co-rule", 151, uzor::types::Rect::new(x0 - 1.0, y - 1.0, 1.0, 2.0), &format!("background: {q}")),
        fixed("co-rule", 151, uzor::types::Rect::new(x1, y - 1.0, 1.0, 2.0), &format!("background: {q}")),
    ]
}

/// A fixed-positioned box in window coordinates (escapes the dialog's
/// overflow clip; z-index orders it against the kit dialog's own 150).
pub(crate) fn fixed(class: &str, z: i32, r: uzor::types::Rect, css: &str) -> uzor::ui::dom::El {
    uzor::ui::dom::El::new(format!("div.{class}")).css(format!(
        "position: fixed; z-index: {z}; left: {}px; top: {}px; width: {}px; height: {}px; {css}",
        r.x, r.y, r.width, r.height
    ))
}

/// The chart render context over a DOM slot's uzor paint target: forwards
/// every drawing call, no viewport / price scale / bars (the slot bodies
/// draw at absolute shell rects, never in chart coordinates) — the same
/// shape `mlc_shell_fw::ctx::ChartCtx::new` gives the shell's own body
/// slots, lived here so the layout crate's DOM painters need no shell
/// dependency.
pub(crate) struct SlotChartCtx<'a>(pub &'a mut dyn uzor::render::RenderContext);

impl Painter for SlotChartCtx<'_> {
    fn save(&mut self) { self.0.save() }
    fn restore(&mut self) { self.0.restore() }
    fn translate(&mut self, x: f64, y: f64) { self.0.translate(x, y) }
    fn rotate(&mut self, a: f64) { self.0.rotate(a) }
    fn scale(&mut self, x: f64, y: f64) { self.0.scale(x, y) }
    fn set_fill_color(&mut self, c: &str) { self.0.set_fill_color(c) }
    fn set_global_alpha(&mut self, a: f64) { self.0.set_global_alpha(a) }
    fn set_stroke_color(&mut self, c: &str) { self.0.set_stroke_color(c) }
    fn set_stroke_width(&mut self, w: f64) { self.0.set_stroke_width(w) }
    fn set_line_dash(&mut self, p: &[f64]) { self.0.set_line_dash(p) }
    fn set_line_cap(&mut self, c: &str) { self.0.set_line_cap(c) }
    fn set_line_join(&mut self, j: &str) { self.0.set_line_join(j) }
    fn begin_path(&mut self) { self.0.begin_path() }
    fn move_to(&mut self, x: f64, y: f64) { self.0.move_to(x, y) }
    fn line_to(&mut self, x: f64, y: f64) { self.0.line_to(x, y) }
    fn close_path(&mut self) { self.0.close_path() }
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) { self.0.rect(x, y, w, h) }
    fn arc(&mut self, cx: f64, cy: f64, r: f64, s: f64, e: f64) { self.0.arc(cx, cy, r, s, e) }
    fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64, rot: f64, s: f64, e: f64) { self.0.ellipse(cx, cy, rx, ry, rot, s, e) }
    fn quadratic_curve_to(&mut self, a: f64, b: f64, c: f64, d: f64) { self.0.quadratic_curve_to(a, b, c, d) }
    fn bezier_curve_to(&mut self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) { self.0.bezier_curve_to(a, b, c, d, e, f) }
    fn stroke(&mut self) { self.0.stroke() }
    fn fill(&mut self) { self.0.fill() }
}
impl TextRenderer for SlotChartCtx<'_> {
    fn set_font(&mut self, f: &str) { self.0.set_font(f) }
    fn set_text_align(&mut self, a: TextAlign) { self.0.set_text_align(a) }
    fn set_text_baseline(&mut self, b: TextBaseline) { self.0.set_text_baseline(b) }
    fn fill_text(&mut self, t: &str, x: f64, y: f64) { self.0.fill_text(t, x, y) }
    fn stroke_text(&mut self, t: &str, x: f64, y: f64) { self.0.stroke_text(t, x, y) }
}
impl TextMetrics for SlotChartCtx<'_> {
    fn measure_text(&self, t: &str) -> f64 { self.0.measure_text(t) }
    fn text_bounds(&self, t: &str, f: &str) -> TextBounds { self.0.text_bounds(t, f) }
}
impl Masking for SlotChartCtx<'_> {
    fn clip(&mut self) { self.0.clip() }
}
impl Effects for SlotChartCtx<'_> {
    fn set_shadow(&mut self, dx: f64, dy: f64, blur: f64, c: &str) { self.0.set_shadow(dx, dy, blur, c) }
    fn clear_shadow(&mut self) { self.0.clear_shadow() }
    fn set_blend_mode(&mut self, m: uzor::render::BlendMode) { self.0.set_blend_mode(m) }
}
impl ShapeHelpers for SlotChartCtx<'_> {
    fn stroke_rect(&mut self, x: f64, y: f64, w: f64, h: f64) { self.0.stroke_rect(x, y, w, h) }
    fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) { self.0.fill_rect(x, y, w, h) }
    fn rounded_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) { self.0.rounded_rect(x, y, w, h, r) }
    fn rounded_rect_corners(&mut self, x: f64, y: f64, w: f64, h: f64, tl: f64, tr: f64, br: f64, bl: f64) {
        self.0.rounded_rect_corners(x, y, w, h, tl, tr, br, bl)
    }
}
impl BatchPainter for SlotChartCtx<'_> {
    fn draw_line_batch(&mut self, lines: &[uzor::render::LineSegment], c: &str, w: f64) { self.0.draw_line_batch(lines, c, w) }
    fn draw_circle_batch(&mut self, circles: &[uzor::render::CircleBatch], c: &str) { self.0.draw_circle_batch(circles, c) }
    fn stroke_polyline(&mut self, pts: &[(f64, f64)], c: &str, w: f64) { self.0.stroke_polyline(pts, c, w) }
}
impl GradientPainter for SlotChartCtx<'_> {
    fn fill_linear_gradient(&mut self, stops: &[(f32, &str)], x1: f64, y1: f64, x2: f64, y2: f64) {
        self.0.fill_linear_gradient(stops, x1, y1, x2, y2)
    }
    fn fill_radial_gradient(&mut self, cx: f64, cy: f64, r: f64, stops: &[(f32, &str)], x: f64, y: f64, w: f64, h: f64) {
        self.0.fill_radial_gradient(cx, cy, r, stops, x, y, w, h)
    }
}
impl UiEffectHelpers for SlotChartCtx<'_> {
    fn draw_blur_background(&mut self, x: f64, y: f64, w: f64, h: f64) { self.0.draw_blur_background(x, y, w, h) }
    fn blur_overlay_backdrop(&mut self, x: f64, y: f64, w: f64, h: f64) { self.0.blur_overlay_backdrop(x, y, w, h) }
    fn blur_backdrop(&mut self, x: f64, y: f64, w: f64, h: f64, blur: f64, corner: f64) { self.0.blur_backdrop(x, y, w, h, blur, corner) }
    fn has_blur_background(&self) -> bool { self.0.has_blur_background() }
    fn use_convex_glass_buttons(&self) -> bool { self.0.use_convex_glass_buttons() }
    fn draw_glass_button_3d(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64, active: bool, c: &str) {
        self.0.draw_glass_button_3d(x, y, w, h, r, active, c)
    }
}
impl uzor::render::RenderContext for SlotChartCtx<'_> {
    fn dpr(&self) -> f64 { self.0.dpr() }
    fn image_painter(&mut self) -> Option<&mut dyn uzor::render::ImagePainter> { self.0.image_painter() }
}
impl ChartRenderContext for SlotChartCtx<'_> {
    fn chart_width(&self) -> f64 { 0.0 }
    fn chart_height(&self) -> f64 { 0.0 }
    fn bar_to_x(&self, _bar: f64) -> f64 { 0.0 }
    fn price_to_y(&self, _price: f64) -> f64 { 0.0 }
    fn set_coordinate_space(&mut self, _w: f64, _h: f64, _vs: f64, _bs: f64, _pmin: f64, _pmax: f64) {}
}
