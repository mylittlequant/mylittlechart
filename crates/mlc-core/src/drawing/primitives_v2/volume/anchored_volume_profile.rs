//! Anchored Volume Profile - volume profile over an anchored [start, end] range
//!
//! Used to draw a single anchor + a histogram sized off VIEWPORT width
//! (`(chart_width - x) * 0.4`), so the tool visibly resized itself whenever
//! the user panned or zoomed — it was not anchored to anything in the data.
//! It now carries a real second anchor (`end_ts_ms`) and reads its geometry
//! from the `[anchor_ts, end_ts_ms]` range exactly like `FixedVolumeProfile`,
//! which is the in-family reference this converges on.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64, bar_interval_seconds};
use crate::i18n::{ConfigKey, current_language};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, render_primitive_text, TextAlign,
    config::{ConfigProperty, PropertyValue},
};

/// Bar span used to place the end anchor when a legacy save has none (see
/// `effective_end_ts`). Roughly the same "reach" the old viewport-relative
/// histogram used to occupy on a freshly placed drawing.
const FALLBACK_BAR_SPAN: i64 = 40;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnchoredVolumeProfile {
    pub data: PrimitiveData,
    #[serde(default)] pub anchor_ts: i64,
    /// End of the profile's time range. Legacy JSON saved before this field
    /// existed deserializes it to `0`, which `effective_end_ts` detects and
    /// replaces with a bar-derived fallback — never with viewport width.
    #[serde(default)] pub end_ts_ms: i64,
    #[serde(default = "default_rows")] pub rows: u16,
    #[serde(default = "default_true")] pub show_poc: bool,
}
fn default_rows() -> u16 { 24 }
fn default_true() -> bool { true }

impl AnchoredVolumeProfile {
    pub fn new(anchor_ts: i64, end_ts_ms: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "anchored_volume_profile".to_string(), display_name: "Anchored Volume Profile".to_string(), color: PrimitiveColor::new(color), width: 1.0, ..Default::default() },
            anchor_ts, end_ts_ms, rows: 24, show_poc: true,
        }
    }

    /// The real end of the range this profile covers.
    ///
    /// `end_ts_ms <= anchor_ts` covers both legacy saves (deserialized to
    /// `0`) and a not-yet-dragged fresh placement — in both cases fall back
    /// to a fixed bar span past the anchor, derived from the loaded bars.
    /// Never falls back to `chart_width()`, which is what made the old
    /// histogram resize itself on every pan/zoom.
    fn effective_end_ts(&self, bars: &[Bar]) -> i64 {
        if self.end_ts_ms <= self.anchor_ts {
            let interval_ms = bar_interval_seconds(bars) * 1000;
            self.anchor_ts + FALLBACK_BAR_SPAN * interval_ms
        } else {
            self.end_ts_ms
        }
    }
}

impl Primitive for AnchoredVolumeProfile {
    fn type_id(&self) -> &'static str { "anchored_volume_profile" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Measurement }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.anchor_ts, 0.0), (self.end_ts_ms, 0.0)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(t, _)) = pts.first() { self.anchor_ts = t; }
        if let Some(&(t, _)) = pts.get(1) { self.end_ts_ms = t; }
    }
    fn translate(&mut self, td: i64, _pd: f64) { self.anchor_ts += td; self.end_ts_ms += td; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, _price: f64) {
        match pt {
            ControlPointType::Point1 => self.anchor_ts = ts_ms,
            ControlPointType::Point2 => self.end_ts_ms = ts_ms,
            ControlPointType::Move => { let td = ts_ms - self.anchor_ts; self.translate(td, 0.0); }
            _ => {}
        }
    }
    /// The two boundaries, then the histogram's own area — same shape as
    /// `FixedVolumeProfile::hit_test`, since this is now the same kind of
    /// two-anchor range. A profile occupies the price band its bars cover,
    /// and nothing above or below it.
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let end_ts = self.effective_end_ts(bars);
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.anchor_ts));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, end_ts));
        if (sx - x1).abs() < HIT_TOLERANCE { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).abs() < HIT_TOLERANCE { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        if sx < min_x || sx > max_x { return HitTestResult::Miss; }
        let window = super::profile_render::bars_in_window(bars, self.anchor_ts, end_ts);
        let Some((lo, hi)) = super::profile_render::price_span(window) else { return HitTestResult::Miss };
        let y_lo = vp.price_to_y(hi, ps.price_min, ps.price_max);
        let y_hi = vp.price_to_y(lo, ps.price_min, ps.price_max);
        if sy >= y_lo - HIT_TOLERANCE && sy <= y_hi + HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }
    /// Two boundaries, each with a handle at both ends of the price band the
    /// profile actually covers — same reasoning as
    /// `FixedVolumeProfile::control_points`, since this is now the same
    /// kind of two-anchor range: the vertical extent is DERIVED from the
    /// bars in `[anchor_ts, end_ts]`, so there is no price for a corner to
    /// write and this stays a two-anchor range, never a box with corners.
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let end_ts = self.effective_end_ts(bars);
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.anchor_ts));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, end_ts));
        let window = super::profile_render::bars_in_window(bars, self.anchor_ts, end_ts);
        let (y_top, y_bot) = match super::profile_render::price_span(window) {
            Some((lo, hi)) => (
                vp.price_to_y(hi, ps.price_min, ps.price_max),
                vp.price_to_y(lo, ps.price_min, ps.price_max),
            ),
            None => {
                let cy = vp.price_to_y((ps.price_min + ps.price_max) / 2.0, ps.price_min, ps.price_max);
                (cy, cy)
            }
        };
        vec![
            ControlPoint::point1(x1, y_top),
            ControlPoint::point1(x1, y_bot),
            ControlPoint::point2(x2, y_top),
            ControlPoint::point2(x2, y_bot),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let end_ts = self.effective_end_ts(ctx.bars());
        let x1 = ctx.ts_to_x_ms(self.anchor_ts);
        let x2 = ctx.ts_to_x_ms(end_ts);
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        let chart_height = ctx.chart_height();

        // Draw the two vertical boundary lines — the box this profile is
        // now genuinely anchored inside, instead of running to infinity.
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        ctx.set_line_dash(&[]);

        ctx.begin_path();
        ctx.move_to(crisp(min_x, dpr), 0.0);
        ctx.line_to(crisp(min_x, dpr), chart_height);
        ctx.stroke();

        ctx.begin_path();
        ctx.move_to(crisp(max_x, dpr), 0.0);
        ctx.line_to(crisp(max_x, dpr), chart_height);
        ctx.stroke();

        // The real profile over the bars this range covers, growing
        // rightward from the LEFT boundary. Width comes from the anchored
        // range in chart space (min_x/max_x), never from ctx.chart_width().
        let window = super::profile_render::bars_in_window(ctx.bars(), self.anchor_ts, end_ts);
        if let Some(vp) = super::profile_render::profile_for(window, self.rows) {
            let geom = super::profile_render::ProfileGeometry {
                origin_x: min_x,
                max_width: max_x - min_x,
                span: (min_x, max_x),
            };
            // Still POC-only: this keeps the legacy single-metric display
            // even now that the range has a real far boundary. A
            // value-area toggle here is a feature addition, out of scope
            // for this geometry fix.
            super::profile_render::draw_profile(ctx, &vp, &geom, self.show_poc, false);
        }

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => min_x,
                    TextAlign::Center => (min_x + max_x) / 2.0,
                    TextAlign::End => max_x,
                };
                let text_y = match text.v_align {
                    TextAlign::Start => 0.0 - text_offset,
                    TextAlign::Center => chart_height / 2.0,
                    TextAlign::End => chart_height + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        let lang = current_language();
        vec![
            ConfigProperty::number("rows", ConfigKey::Rows.get(lang), self.rows as f64, Some(5.0), Some(100.0))
                .with_order(20),
            ConfigProperty::boolean("show_poc", ConfigKey::ShowPoc.get(lang), self.show_poc)
                .with_order(21),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "rows" => { if let Some(v) = value.as_number() { self.rows = v.round().clamp(5.0, 100.0) as u16; return true; } }
            "show_poc" => { if let Some(v) = value.as_bool() { self.show_poc = v; return true; } }
            _ => {}
        }
        false
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "anchored_volume_profile", display_name: "Anchored Volume Profile", kind: PrimitiveKind::Measurement,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Volume profile over an anchored range", icon: "anchored_volume_profile", default_color: "#9C27B0",
        factory: |points, color| {
            let (t1, _) = points.first().copied().unwrap_or((0, 0.0));
            let (t2, _) = points.get(1).copied().unwrap_or((t1 + 180_000_000, 0.0));
            Box::new(AnchoredVolumeProfile::new(t1, t2, color))
        },
        supports_text: true,
        has_levels: false,
    }
}
