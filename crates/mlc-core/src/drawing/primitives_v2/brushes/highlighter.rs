//! Highlighter - semi-transparent highlight

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64, apply_opacity};
use crate::i18n::{ConfigKey, current_language};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, Archetype, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext,
    config::{ConfigProperty, PropertyValue, PropertyCategory},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Highlighter {
    pub data: PrimitiveData,
    pub points: Vec<(i64, f64)>,
    #[serde(default = "default_size")] pub brush_size: f64,
    #[serde(default = "default_opacity")] pub opacity: f64,
}
fn default_size() -> f64 { 20.0 }
fn default_opacity() -> f64 { 0.4 }

impl Highlighter {
    pub fn new(points: Vec<(i64, f64)>, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "highlighter".to_string(), display_name: "Highlighter".to_string(), color: PrimitiveColor::new(color), width: 20.0, ..Default::default() },
            points, brush_size: 20.0, opacity: 0.4,
        }
    }
}

impl Primitive for Highlighter {
    fn type_id(&self) -> &'static str { "highlighter" }
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
                let td = ts_ms - first.0; let pd = price - first.1;
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
        if self.points.is_empty() { return; }
        let _dpr = ctx.dpr();
        let color_with_opacity = apply_opacity(&self.data.color.stroke, self.opacity);
        ctx.set_stroke_color(&color_with_opacity);
        ctx.set_stroke_width(self.data.width);
        ctx.set_line_cap("round");
        ctx.set_line_join("round");

        let screen_pts: Vec<(f64, f64)> = self.points.iter()
            .map(|&(ts, price)| (ctx.ts_to_x_ms(ts), ctx.price_to_y(price)))
            .collect();

        ctx.begin_path();
        if screen_pts.len() == 1 {
            let (x, y) = screen_pts[0];
            ctx.arc(x, y, self.data.width / 2.0, 0.0, std::f64::consts::TAU);
            ctx.fill(); return;
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

    fn style_properties(&self) -> Vec<ConfigProperty> {
        // `opacity` and `brush_size` existed as struct fields with no way to
        // reach them from the settings modal — the low default opacity
        // (0.4) over a dark chart is what read as a muddy, khaki-looking
        // stroke instead of a bright highlighter; this at least lets the
        // owner brighten it per-primitive instead of only via the source default.
        vec![
            ConfigProperty::fill_opacity(self.opacity * 100.0)
                .with_category(PropertyCategory::AppearanceFill)
                .with_order(20),
            ConfigProperty::number("brush_size", ConfigKey::BrushSize.get(current_language()), self.brush_size, Some(4.0), Some(60.0))
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(21),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "fill_opacity" => {
                if let Some(v) = value.as_number() {
                    self.opacity = (v / 100.0).clamp(0.05, 1.0);
                    return true;
                }
            }
            "brush_size" => {
                if let Some(v) = value.as_number() {
                    self.brush_size = v.max(1.0);
                    self.data.width = self.brush_size;
                    return true;
                }
            }
            _ => {}
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
        type_id: "highlighter", display_name: "Highlighter", kind: PrimitiveKind::Annotation,
        click_behavior: ClickBehavior::FreehandDrag, tooltip: "Semi-transparent highlight",
        icon: "highlighter", default_color: "#FFEB3B",
        factory: |points, color| Box::new(Highlighter::new(points.to_vec(), color)),
        supports_text: false, has_levels: false,
    }
}
