//! A render context that lays out and measures through the real one and paints
//! nothing: for pieces a uzor-framework host draws itself (see
//! `ChartPanelApp::dom_hosted`), whose layout and hit registration stay the app's.

use mlc_core::render::RenderContext;
use uzor::render::{
    BatchPainter, Effects, GradientPainter, Masking, Painter, ShapeHelpers, TextAlign, TextBaseline, TextBounds, TextMetrics,
    TextRenderer, UiEffectHelpers,
};

/// Measures through `0`, draws nothing.
pub struct PaintOff<'a>(pub &'a mut dyn RenderContext);

impl Painter for PaintOff<'_> {
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
    fn quadratic_curve_to(&mut self, _a: f64, _b: f64, _x: f64, _y: f64) {}
    fn bezier_curve_to(&mut self, _a: f64, _b: f64, _c: f64, _d: f64, _x: f64, _y: f64) {}
    fn stroke(&mut self) {}
    fn fill(&mut self) {}
}
impl TextRenderer for PaintOff<'_> {
    fn set_font(&mut self, font: &str) {
        self.0.set_font(font);
    }
    fn set_text_align(&mut self, _a: TextAlign) {}
    fn set_text_baseline(&mut self, _b: TextBaseline) {}
    fn fill_text(&mut self, _t: &str, _x: f64, _y: f64) {}
    fn stroke_text(&mut self, _t: &str, _x: f64, _y: f64) {}
}
impl TextMetrics for PaintOff<'_> {
    fn measure_text(&self, text: &str) -> f64 {
        self.0.measure_text(text)
    }
    fn text_bounds(&self, text: &str, font: &str) -> TextBounds {
        self.0.text_bounds(text, font)
    }
}
impl Masking for PaintOff<'_> {
    fn clip(&mut self) {}
}
impl Effects for PaintOff<'_> {}
impl ShapeHelpers for PaintOff<'_> {
    fn stroke_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    fn fill_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
}
impl GradientPainter for PaintOff<'_> {}
impl UiEffectHelpers for PaintOff<'_> {}
impl BatchPainter for PaintOff<'_> {}
impl uzor::render::RenderContext for PaintOff<'_> {
    fn dpr(&self) -> f64 {
        self.0.dpr()
    }
}
impl RenderContext for PaintOff<'_> {
    fn chart_width(&self) -> f64 {
        self.0.chart_width()
    }
    fn chart_height(&self) -> f64 {
        self.0.chart_height()
    }
    fn bar_to_x(&self, bar: f64) -> f64 {
        self.0.bar_to_x(bar)
    }
    fn price_to_y(&self, price: f64) -> f64 {
        self.0.price_to_y(price)
    }
    fn set_coordinate_space(&mut self, _w: f64, _h: f64, _vs: f64, _bs: f64, _pmin: f64, _pmax: f64) {}
}
