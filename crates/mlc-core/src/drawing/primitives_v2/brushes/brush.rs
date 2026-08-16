//! Brush - freehand drawing

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::i18n::{ConfigKey, current_language};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, Archetype, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, LineStyle,
    config::{ConfigProperty, PropertyValue, PropertyCategory, PropertyGroup},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Brush {
    pub data: PrimitiveData,
    pub points: Vec<(i64, f64)>,
    #[serde(default = "default_size")] pub brush_size: f64,
}
fn default_size() -> f64 { 3.0 }

impl Brush {
    pub fn new(points: Vec<(i64, f64)>, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "brush".to_string(), display_name: "Brush".to_string(), color: PrimitiveColor::new(color), width: 3.0, ..Default::default() },
            points, brush_size: 3.0,
        }
    }
}

impl Primitive for Brush {
    fn type_id(&self) -> &'static str { "brush" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Annotation }
    // A freehand stroke, not a textual annotation — `Brush` archetype, not
    // the majority `Annotation`->`Text` default.
    fn archetype(&self) -> Archetype { Archetype::Brush }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::FreehandDrag }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { self.points.clone() }
    fn set_points(&mut self, pts: &[(i64, f64)]) { self.points = pts.to_vec(); }
    fn translate(&mut self, ts_delta_ms: i64, pd: f64) {
        for p in &mut self.points { p.0 += ts_delta_ms; p.1 += pd; }
    }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        if let ControlPointType::Move = pt {
            if let Some(first) = self.points.first() {
                let td = ts_ms - first.0;
                let pd = price - first.1;
                self.translate(td, pd);
            }
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let screen: Vec<_> = self.points.iter().map(|(ts, p)| {
            let bar = timestamp_ms_to_bar_f64(bars, *ts);
            (vp.bar_to_x_f64(bar), vp.price_to_y(*p, ps.price_min, ps.price_max))
        }).collect();
        for i in 0..screen.len().saturating_sub(1) {
            if point_to_line_dist(sx, sy, screen[i].0, screen[i].1, screen[i+1].0, screen[i+1].1) < HIT_TOLERANCE + self.brush_size {
                return HitTestResult::Body;
            }
        }
        HitTestResult::Miss
    }
    fn control_points(&self, _bars: &[Bar], _vp: &Viewport, _ps: &PriceScale) -> Vec<ControlPoint> { vec![] }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        if self.points.is_empty() {
            return;
        }

        let _dpr = ctx.dpr();

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        // Round cap/join deliberately kept regardless of `data.style` (2026-08-09
        // brush-defect pass) — a hand-drawn freehand squiggle reads as a pen
        // stroke only with rounded corners at every curve segment join; a
        // butt/miter join here would show sharp spikes at the quadratic-bezier
        // segment boundaries below, which no freehand tool anywhere draws.
        // `Highlighter` (the other `FreehandDrag` primitive) makes the same
        // choice for the same reason.
        ctx.set_line_cap("round");
        ctx.set_line_join("round");
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Convert to screen coordinates
        let screen_pts: Vec<(f64, f64)> = self.points.iter()
            .map(|&(ts, price)| (ctx.ts_to_x_ms(ts), ctx.price_to_y(price)))
            .collect();

        // Draw smooth curve using quadratic bezier interpolation
        ctx.begin_path();
        if screen_pts.len() == 1 {
            let (x, y) = screen_pts[0];
            ctx.arc(x, y, self.data.width / 2.0, 0.0, std::f64::consts::TAU);
            ctx.fill();
            return;
        } else if screen_pts.len() == 2 {
            ctx.move_to(screen_pts[0].0, screen_pts[0].1);
            ctx.line_to(screen_pts[1].0, screen_pts[1].1);
        } else {
            ctx.move_to(screen_pts[0].0, screen_pts[0].1);

            let mid_x = (screen_pts[0].0 + screen_pts[1].0) / 2.0;
            let mid_y = (screen_pts[0].1 + screen_pts[1].1) / 2.0;
            ctx.line_to(mid_x, mid_y);

            for i in 1..screen_pts.len() - 1 {
                let next_mid_x = (screen_pts[i].0 + screen_pts[i + 1].0) / 2.0;
                let next_mid_y = (screen_pts[i].1 + screen_pts[i + 1].1) / 2.0;
                ctx.quadratic_curve_to(screen_pts[i].0, screen_pts[i].1, next_mid_x, next_mid_y);
            }

            let last = screen_pts.last().unwrap();
            ctx.line_to(last.0, last.1);
        }
        ctx.stroke();

    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    /// `brush_size` existed as a real struct field with no way to reach it
    /// from the settings modal (2026-08-09 modal-surface-inventory audit —
    /// it only ever drove `hit_test`'s tolerance, drifting silently from
    /// `data.width` the moment the base-stroke row's own width control was
    /// used). Mirrors `Highlighter::style_properties`'s identical fix for
    /// the same field, same bounds shape.
    ///
    /// Brush archetype (W3, `docs/mlc/plans/archetype-modals-into-the-app-2026-08-11.md`):
    /// `brush_tabs()` (`mlc-ui-lab`'s `spec.rs`) puts colour/width/opacity/
    /// style under a "Stroke" group whose own title is the SAME key as the
    /// tab label itself (`ConceptKey::StrokeGroupTitle` — see
    /// `subject_tab_label`'s `Archetype::Brush` arm) — the design repeats
    /// the subject word as the first group's own header too. `brush_size`
    /// is pulled into that group so it reads as belonging with the base
    /// stroke row rather than as an unlabeled extra.
    ///
    /// Colour/width/line style need no separate declaration: the bespoke
    /// base-stroke row every primitive without a level ladder already gets
    /// (`primitive_settings_vid`'s own doc) supplies exactly those three,
    /// reading/writing `data.color`/`data.width`/`data.style` directly.
    ///
    /// The lab's own "opacity" row in this group stays out: unlike
    /// `Highlighter` (see that file's own doc), `Brush` has no opacity field
    /// of any kind — `render` strokes at full alpha — so there is nothing to
    /// wire a slider to.
    ///
    /// `PropertyGroup::PointShape` (smooth / smoothing-strength / round-caps
    /// / close-outline) is left out entirely: `render` always draws rounded
    /// caps/joins and always applies the SAME fixed quadratic-bezier
    /// smoothing to every stroke with 3+ points (see the `set_line_cap` call
    /// above's own comment) — none of the four toggles/sliders the lab
    /// draws has a field, a strength parameter, or a conditional branch
    /// behind it; the smoothing is not optional today.
    ///
    /// `PropertyGroup::FillClosedOutline` is also left out: a brush stroke
    /// has no fill concept at all — `render`'s only `ctx.fill()` call is the
    /// degenerate single-point case (a stroke-coloured dot, not a togglable
    /// fill), so `fill`/`closed`/a separate fill colour have nothing to
    /// read from.
    fn style_properties(&self) -> Vec<ConfigProperty> {
        vec![
            ConfigProperty::number("brush_size", ConfigKey::BrushSize.get(current_language()), self.brush_size, Some(1.0), Some(20.0))
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(20)
                .in_group(PropertyGroup::Stroke),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        if id == "brush_size" {
            if let Some(v) = value.as_number() {
                self.brush_size = v.max(1.0);
                self.data.width = self.brush_size;
                return true;
            }
        }
        false
    }
}

fn point_to_line_dist(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 { return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt(); }
    let t = (((px - x1) * dx + (py - y1) * dy) / len_sq).clamp(0.0, 1.0);
    ((px - (x1 + t * dx)).powi(2) + (py - (y1 + t * dy)).powi(2)).sqrt()
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "brush", display_name: "Brush", kind: PrimitiveKind::Annotation,
        click_behavior: ClickBehavior::FreehandDrag, tooltip: "Freehand drawing", icon: "brush", default_color: "#F44336",
        factory: |points, color| Box::new(Brush::new(points.to_vec(), color)),
        supports_text: false,
        has_levels: false,
    }
}
