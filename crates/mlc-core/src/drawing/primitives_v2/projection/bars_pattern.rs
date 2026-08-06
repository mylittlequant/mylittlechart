//! Bars Pattern - copy and project price pattern
//!
//! Three anchors: `ts1`/`ts2` bracket a SOURCE range of already-drawn bars,
//! `ts3` is where that range's pattern is projected to. Rendering copies
//! the OHLC of every source bar onto a shifted timestamp starting at
//! `ts3`, optionally mirrored vertically (`mirror`) and shifted by
//! `price_offset`.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::i18n::{ConfigKey, current_language};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle,
    render_primitive_text,
    config::{ConfigProperty, PropertyValue},
};

/// Default up/down candle colors, matching the chart's own default theme
/// preset (`#26a69a` / `#ef5350`) — the projected copy is a self-contained
/// primitive render and does not thread the live `ThemeManager`.
const PROJECTED_UP_COLOR: &str = "#26a69a";
const PROJECTED_DOWN_COLOR: &str = "#ef5350";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BarsPattern {
    pub data: PrimitiveData,
    #[serde(default)] pub ts1: i64,
    #[serde(default)] pub ts2: i64,
    #[serde(default)] pub ts3: i64,
    #[serde(default)] pub price_offset: f64,
    #[serde(default = "default_true")] pub mirror: bool,
}
fn default_true() -> bool { true }

impl BarsPattern {
    pub fn new(ts1: i64, ts2: i64, ts3: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "bars_pattern".to_string(), display_name: "Bars Pattern".to_string(), color: PrimitiveColor::new(color), width: 1.0, ..Default::default() },
            ts1, ts2, ts3, price_offset: 0.0, mirror: false,
        }
    }

    /// The projected range's end timestamp (ms): the source range's own
    /// span, re-anchored at `ts3`.
    fn projected_end_ms(&self) -> i64 {
        let lo = self.ts1.min(self.ts2);
        let hi = self.ts1.max(self.ts2);
        self.ts3 + (hi - lo)
    }
}

impl Primitive for BarsPattern {
    fn type_id(&self) -> &'static str { "bars_pattern" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Trading }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::ThreePoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, 0.0), (self.ts2, 0.0), (self.ts3, 0.0)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(t, _)) = pts.first() { self.ts1 = t; }
        if let Some(&(t, _)) = pts.get(1) { self.ts2 = t; }
        if let Some(&(t, _)) = pts.get(2) { self.ts3 = t; }
    }
    fn translate(&mut self, td: i64, pd: f64) { self.ts1 += td; self.ts2 += td; self.ts3 += td; self.price_offset += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, _price: f64) {
        match pt {
            ControlPointType::Point1 => self.ts1 = ts_ms,
            ControlPointType::Point2 => self.ts2 = ts_ms,
            ControlPointType::Point3 => self.ts3 = ts_ms,
            ControlPointType::Move => { let td = ts_ms - self.ts1; self.translate(td, 0.0); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, _sy: f64, bars: &[Bar], vp: &Viewport, _ps: &PriceScale) -> HitTestResult {
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts1));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts2));
        let x3 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts3));
        if (sx - x1).abs() < HIT_TOLERANCE { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).abs() < HIT_TOLERANCE { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        if (sx - x3).abs() < HIT_TOLERANCE { return HitTestResult::ControlPoint(ControlPointType::Point3); }

        // Body: anywhere across the source range or the projected copy
        // (full chart height, matching the vertical boundary markers).
        let x4 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.projected_end_ms()));
        let min_x = x1.min(x2).min(x3).min(x4);
        let max_x = x1.max(x2).max(x3).max(x4);
        if sx >= min_x - HIT_TOLERANCE && sx <= max_x + HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let cy = vp.price_to_y((ps.price_min + ps.price_max) / 2.0, ps.price_min, ps.price_max);
        vec![
            ControlPoint::point1(vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts1)), cy),
            ControlPoint::point2(vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts2)), cy),
            ControlPoint::point3(vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts3)), cy),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let x3 = ctx.ts_to_x_ms(self.ts3);
        let x4 = ctx.ts_to_x_ms(self.projected_end_ms());

        let chart_height = ctx.chart_height();

        // Boundary markers: source range (ts1, ts2) and projection start (ts3).
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }
        for x in [x1, x2, x3] {
            ctx.begin_path();
            ctx.move_to(crisp(x, dpr), 0.0);
            ctx.line_to(crisp(x, dpr), chart_height);
            ctx.stroke();
        }
        ctx.set_line_dash(&[]);

        // Copy and project the source candles onto the ts3-anchored range.
        //
        // Collect the projected (timestamp, OHLC) tuples first, while
        // `ctx.bars()` is borrowed immutably; the render pass below only
        // touches owned values, so it is free to take `ctx` mutably.
        let projected: Vec<(i64, f64, f64, f64, f64)> = {
            let bars = ctx.bars();
            if bars.is_empty() {
                Vec::new()
            } else {
                let lo_ms = self.ts1.min(self.ts2);
                let hi_ms = self.ts1.max(self.ts2);
                let lo_sec = lo_ms.div_euclid(1000);
                let hi_sec = hi_ms.div_euclid(1000);
                let start_idx = bars.partition_point(|b| b.timestamp < lo_sec);
                let end_idx = bars.partition_point(|b| b.timestamp <= hi_sec);
                let shift_ms = self.ts3 - lo_ms;
                // Mirror pivot: the source range's own opening price, so a
                // mirrored copy flips around where the pattern started.
                let pivot = bars.get(start_idx).map(|b| b.open).unwrap_or(0.0);

                bars[start_idx..end_idx].iter().map(|bar| {
                    let tgt_ts_ms = bar.timestamp * 1000 + shift_ms;
                    let (open, high, low, close) = if self.mirror {
                        (
                            2.0 * pivot - bar.open,
                            2.0 * pivot - bar.low,
                            2.0 * pivot - bar.high,
                            2.0 * pivot - bar.close,
                        )
                    } else {
                        (bar.open, bar.high, bar.low, bar.close)
                    };
                    (
                        tgt_ts_ms,
                        open + self.price_offset,
                        high + self.price_offset,
                        low + self.price_offset,
                        close + self.price_offset,
                    )
                }).collect()
            }
        };

        if !projected.is_empty() {
            let spacing = (ctx.bar_to_x(1.0) - ctx.bar_to_x(0.0)).abs();
            let body_w = (spacing * 0.7).max(1.0);

            for (tgt_ts_ms, open, high, low, close) in projected {
                let cx = ctx.ts_to_x_ms(tgt_ts_ms);
                let is_up = close >= open;
                let color = if is_up { PROJECTED_UP_COLOR } else { PROJECTED_DOWN_COLOR };

                let high_y = ctx.price_to_y(high);
                let low_y = ctx.price_to_y(low);
                let open_y = ctx.price_to_y(open);
                let close_y = ctx.price_to_y(close);

                ctx.set_stroke_color(color);
                ctx.set_stroke_width(1.0);
                ctx.set_line_dash(&[]);
                ctx.begin_path();
                ctx.move_to(crisp(cx, dpr), high_y);
                ctx.line_to(crisp(cx, dpr), low_y);
                ctx.stroke();

                let top = open_y.min(close_y);
                let h = (open_y - close_y).abs().max(1.0);
                ctx.set_fill_color(color);
                ctx.fill_rect(crisp(cx - body_w / 2.0, dpr), top, body_w, h);
            }
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let min_x = x1.min(x2).min(x3);
                let max_x = x1.max(x2).max(x3).max(x4);
                let min_y = 0.0;
                let max_y = chart_height;

                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => (min_x + max_x) / 2.0,
                    super::super::TextAlign::End => max_x,
                };
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => min_y - text_offset,
                    super::super::TextAlign::Center => (min_y + max_y) / 2.0,
                    super::super::TextAlign::End => max_y + text_offset,
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
            ConfigProperty::boolean("mirror", ConfigKey::Mirror.get(lang), self.mirror)
                .with_order(20),
            ConfigProperty::number("price_offset", ConfigKey::PriceOffset.get(lang), self.price_offset, None, None)
                .with_order(21),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "mirror" => { if let Some(v) = value.as_bool() { self.mirror = v; return true; } }
            "price_offset" => { if let Some(v) = value.as_number() { self.price_offset = v; return true; } }
            _ => {}
        }
        false
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "bars_pattern", display_name: "Bars Pattern", kind: PrimitiveKind::Trading,
        click_behavior: ClickBehavior::ThreePoint, tooltip: "Copy and project price pattern", icon: "bars_pattern", default_color: "#9C27B0",
        factory: |points, color| {
            let (t1, _) = points.first().copied().unwrap_or((0, 0.0));
            let (t2, _) = points.get(1).copied().unwrap_or((t1 + 72_000_000, 0.0));
            let (t3, _) = points.get(2).copied().unwrap_or((t2 + 36_000_000, 0.0));
            Box::new(BarsPattern::new(t1, t2, t3, color))
        },
        supports_text: true,
        has_levels: false,
    }
}
