//! Anchored VWAP drawing primitive.
//!
//! Computes a running VWAP from a user-chosen anchor bar to the last bar.
//! Typical price = (high + low + close) / 3. Running variance yields ±1σ
//! and ±2σ bands drawn as semi-transparent fills.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, render_primitive_text, TextAlign,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnchoredVwap {
    pub data: PrimitiveData,
    /// Timestamp (ms) of the anchor bar. VWAP is accumulated from this bar onward.
    #[serde(default)]
    pub anchor_ts: i64,
    /// Whether to draw ±1 σ deviation bands.
    #[serde(default = "default_true")]
    pub show_stddev_1: bool,
    /// Whether to draw ±2 σ deviation bands.
    #[serde(default = "default_true")]
    pub show_stddev_2: bool,
}

fn default_true() -> bool { true }

impl AnchoredVwap {
    pub fn new(anchor_ts: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "anchored_vwap".to_string(),
                display_name: "Anchored VWAP".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            anchor_ts,
            show_stddev_1: true,
            show_stddev_2: true,
        }
    }
}

impl Primitive for AnchoredVwap {
    fn type_id(&self) -> &'static str { "anchored_vwap" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Measurement }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::SingleClick }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.anchor_ts, 0.0)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(t, _)) = pts.first() { self.anchor_ts = t; }
    }
    fn translate(&mut self, td: i64, _pd: f64) { self.anchor_ts += td; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, _price: f64) {
        if matches!(pt, ControlPointType::Point1 | ControlPointType::Move) {
            self.anchor_ts = ts_ms;
        }
    }

    /// The anchor, then the VWAP curve itself.
    ///
    /// It used to be the anchor alone: the curve — the thing the primitive IS
    /// — could not be clicked anywhere along its length. The accumulation is
    /// run only as far as the bar under the cursor, so this stays one pass
    /// over the bars left of `sx` rather than a full curve build per mouse
    /// move.
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let b = timestamp_ms_to_bar_f64(bars, self.anchor_ts);
        let x = vp.bar_to_x_f64(b);
        if (sx - x).abs() < 20.0 {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        if sx < x || bars.is_empty() {
            return HitTestResult::Miss;
        }
        let anchor_idx = {
            let anchor_sec = self.anchor_ts / 1000;
            match bars.binary_search_by_key(&anchor_sec, |b| b.timestamp) {
                Ok(i) => i,
                Err(i) => i.min(bars.len().saturating_sub(1)),
            }
        };
        // Which bar the cursor is over.
        let hover = vp.x_to_bar_f64(sx).round();
        if !hover.is_finite() || hover < anchor_idx as f64 {
            return HitTestResult::Miss;
        }
        let hover_idx = (hover as usize).min(bars.len() - 1);
        if hover_idx < anchor_idx {
            return HitTestResult::Miss;
        }

        let (mut cum_vp, mut cum_v, mut cum_vp2) = (0.0_f64, 0.0_f64, 0.0_f64);
        for b in &bars[anchor_idx..=hover_idx] {
            let typical = (b.high + b.low + b.close) / 3.0;
            let v = b.volume.max(0.0);
            cum_vp += v * typical;
            cum_v += v;
            cum_vp2 += v * typical * typical;
        }
        if cum_v <= 0.0 {
            return HitTestResult::Miss;
        }
        let vwap = cum_vp / cum_v;
        let stddev = ((cum_vp2 / cum_v) - vwap * vwap).max(0.0).sqrt();

        let y_of = |price: f64| vp.price_to_y(price, ps.price_min, ps.price_max);
        for price in [vwap, vwap + stddev, vwap - stddev, vwap + 2.0 * stddev, vwap - 2.0 * stddev] {
            if (sy - y_of(price)).abs() < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let b = timestamp_ms_to_bar_f64(bars, self.anchor_ts);
        let cy = vp.price_to_y((ps.price_min + ps.price_max) / 2.0, ps.price_min, ps.price_max);
        vec![ControlPoint::point1(vp.bar_to_x_f64(b), cy)]
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let bars = ctx.bars();
        if bars.is_empty() { return; }

        let chart_width = ctx.chart_width();
        let chart_height = ctx.chart_height();
        let anchor_x = ctx.ts_to_x_ms(self.anchor_ts);

        // ── Find anchor bar index ────────────────────────────────────────────
        let anchor_ts_sec = self.anchor_ts / 1000;
        let anchor_idx = match bars.binary_search_by_key(&anchor_ts_sec, |b| b.timestamp) {
            Ok(i) => i,
            Err(i) => i.min(bars.len().saturating_sub(1)),
        };
        if anchor_idx >= bars.len() { return; }

        // ── Compute running VWAP + stddev ────────────────────────────────────
        let n = bars.len() - anchor_idx;
        if n == 0 { return; }
        let mut vwap_points: Vec<(f64, f64)> = Vec::with_capacity(n);
        let mut sd1_upper: Vec<(f64, f64)> = Vec::with_capacity(n);
        let mut sd1_lower: Vec<(f64, f64)> = Vec::with_capacity(n);
        let mut sd2_upper: Vec<(f64, f64)> = Vec::with_capacity(n);
        let mut sd2_lower: Vec<(f64, f64)> = Vec::with_capacity(n);

        let mut cum_vol_price = 0.0_f64;
        let mut cum_vol = 0.0_f64;
        let mut cum_vol_price2 = 0.0_f64;

        for (step, b) in bars[anchor_idx..].iter().enumerate() {
            let typical = (b.high + b.low + b.close) / 3.0;
            let v = b.volume.max(0.0);
            cum_vol_price += v * typical;
            cum_vol += v;
            cum_vol_price2 += v * typical * typical;

            if cum_vol <= 0.0 { continue; }

            let vwap = cum_vol_price / cum_vol;
            // Running population variance: E[x²] - E[x]²
            let variance = (cum_vol_price2 / cum_vol) - (vwap * vwap);
            let stddev = variance.max(0.0).sqrt();

            let bar_idx = (anchor_idx + step) as f64;
            let x = ctx.bar_to_x(bar_idx);
            let y_vwap = ctx.price_to_y(vwap);

            vwap_points.push((x, y_vwap));
            sd1_upper.push((x, ctx.price_to_y(vwap + stddev)));
            sd1_lower.push((x, ctx.price_to_y(vwap - stddev)));
            sd2_upper.push((x, ctx.price_to_y(vwap + 2.0 * stddev)));
            sd2_lower.push((x, ctx.price_to_y(vwap - 2.0 * stddev)));
        }

        if vwap_points.is_empty() { return; }

        // ── Draw ±2σ band ────────────────────────────────────────────────────
        if self.show_stddev_2 && sd2_upper.len() >= 2 {
            draw_band(ctx, &sd2_upper, &sd2_lower, "rgba(100, 149, 237, 0.10)");
        }

        // ── Draw ±1σ band ────────────────────────────────────────────────────
        if self.show_stddev_1 && sd1_upper.len() >= 2 {
            draw_band(ctx, &sd1_upper, &sd1_lower, "rgba(100, 149, 237, 0.18)");
        }

        // ── Draw ±2σ outlines ────────────────────────────────────────────────
        if self.show_stddev_2 && sd2_upper.len() >= 2 {
            draw_polyline(ctx, &sd2_upper, "rgba(100, 149, 237, 0.40)", 1.0 * dpr);
            draw_polyline(ctx, &sd2_lower, "rgba(100, 149, 237, 0.40)", 1.0 * dpr);
        }

        // ── Draw ±1σ outlines ────────────────────────────────────────────────
        if self.show_stddev_1 && sd1_upper.len() >= 2 {
            draw_polyline(ctx, &sd1_upper, "rgba(100, 149, 237, 0.60)", 1.0 * dpr);
            draw_polyline(ctx, &sd1_lower, "rgba(100, 149, 237, 0.60)", 1.0 * dpr);
        }

        // ── Draw VWAP line ───────────────────────────────────────────────────
        if vwap_points.len() >= 2 {
            draw_polyline(ctx, &vwap_points, &self.data.color.stroke, self.data.width * dpr);
        }

        // ── Draw vertical anchor line ────────────────────────────────────────
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width * dpr);
        ctx.set_line_dash(&[4.0 * dpr, 4.0 * dpr]);
        ctx.begin_path();
        ctx.move_to(crisp(anchor_x, dpr), 0.0);
        ctx.line_to(crisp(anchor_x, dpr), chart_height);
        ctx.stroke();
        ctx.set_line_dash(&[]);

        // ── Anchor marker dot ────────────────────────────────────────────────
        let anchor_mid_y = chart_height / 2.0;
        ctx.set_fill_color(&self.data.color.stroke);
        ctx.set_global_alpha(1.0);
        ctx.begin_path();
        ctx.arc(anchor_x, anchor_mid_y, 4.0 * dpr, 0.0, std::f64::consts::TAU);
        ctx.fill();

        // ── Optional text label ──────────────────────────────────────────────
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => anchor_x,
                    TextAlign::Center => (anchor_x + chart_width) / 2.0,
                    TextAlign::End => chart_width,
                };
                let text_y = match text.v_align {
                    TextAlign::Start => -text_offset,
                    TextAlign::Center => chart_height / 2.0,
                    TextAlign::End => chart_height + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }

        let _ = chart_width;
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
}

// ─── Drawing helpers ──────────────────────────────────────────────────────────

fn draw_polyline(ctx: &mut dyn RenderContext, pts: &[(f64, f64)], color: &str, width: f64) {
    if pts.len() < 2 { return; }
    ctx.set_stroke_color(color);
    ctx.set_stroke_width(width);
    ctx.set_line_dash(&[]);
    ctx.begin_path();
    ctx.move_to(pts[0].0, pts[0].1);
    for &(x, y) in &pts[1..] {
        ctx.line_to(x, y);
    }
    ctx.stroke();
}

fn draw_band(
    ctx: &mut dyn RenderContext,
    upper: &[(f64, f64)],
    lower: &[(f64, f64)],
    fill_color: &str,
) {
    if upper.is_empty() || lower.is_empty() { return; }
    ctx.set_fill_color(fill_color);
    ctx.set_global_alpha(1.0);
    ctx.begin_path();
    // Forward along upper edge.
    ctx.move_to(upper[0].0, upper[0].1);
    for &(x, y) in &upper[1..] {
        ctx.line_to(x, y);
    }
    // Backward along lower edge (close the shape).
    for &(x, y) in lower.iter().rev() {
        ctx.line_to(x, y);
    }
    ctx.fill();
}

// ─── Registration ─────────────────────────────────────────────────────────────

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "anchored_vwap",
        display_name: "Anchored VWAP",
        kind: PrimitiveKind::Measurement,
        click_behavior: ClickBehavior::SingleClick,
        tooltip: "VWAP from anchor bar + ±1/±2σ deviation bands",
        icon: "anchored_vwap",
        default_color: "#5C6BC0",
        factory: |points, color| {
            let (t, _) = points.first().copied().unwrap_or((0, 0.0));
            Box::new(AnchoredVwap::new(t, color))
        },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
