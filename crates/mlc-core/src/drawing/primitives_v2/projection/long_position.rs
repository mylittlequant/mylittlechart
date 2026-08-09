//! Long Position - buy trade visualization

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, ControlPointCursor, PrimitiveColor,
    HIT_TOLERANCE, point_to_line_distance,
    RenderContext, crisp, LineStyle,
    render_primitive_text, fmt_price,
    config::{LabelStyle, ConfigProperty, PropertyValue, PropertyCategory},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LongPosition {
    pub data: PrimitiveData,
    #[serde(default)] pub ts_ms: i64,
    pub entry_price: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
    /// Right edge of the box (ms). Deserializes to 0 on every drawing saved
    /// before this field existed — `resolved_end_ts_ms` falls back to a
    /// bar-interval-derived span rather than the live viewport edge.
    #[serde(default)] pub end_ts_ms: i64,
    #[serde(default)] pub quantity: f64,
    #[serde(default = "default_true")] pub show_pnl: bool,
    #[serde(default = "default_true")] pub show_live_label: bool,
    #[serde(default = "default_stop_color")] pub stop_color: String,
    #[serde(default = "default_target_color")] pub target_color: String,
    #[serde(default = "default_zone_opacity")] pub zone_opacity: f64,
    #[serde(default)] pub label_style: LabelStyle,
}
fn default_true() -> bool { true }
fn default_stop_color() -> String { "#F23645".to_string() }
fn default_target_color() -> String { "#089981".to_string() }
fn default_zone_opacity() -> f64 { 0.15 }

/// Muted neutral — deliberately not the entry/stop/target colors, so the
/// live marker reads as subordinate to them rather than a fourth level.
const LIVE_MARKER_COLOR: &str = "#9598A1";

/// Extra alpha stacked on top of the base zone fill for the traversed
/// (entry → current price) portion of the position — reads visibly deeper
/// than the untraveled rest of the zone without repainting it from scratch.
const LIVE_ZONE_EXTRA_ALPHA: f64 = 0.15;

/// How many median bar intervals the box extends by default when
/// `end_ts_ms` is unset (legacy data) or degenerate.
const DEFAULT_BOX_BARS: i64 = 40;

/// Format a signed price delta with an explicit `+`/`-` — `fmt_price` alone
/// has no sign, and a P&L or distance-from-entry reads wrong without one.
fn signed_price(v: f64) -> String {
    if v >= 0.0 { format!("+{}", fmt_price(v)) } else { format!("-{}", fmt_price(-v)) }
}

/// Nudge a degenerate second click (landed exactly on entry) away from
/// entry by a small fraction of price, so the box a two-click diagonal
/// derives never collapses to zero height.
fn resolve_p2(entry: f64, raw_p2: f64) -> f64 {
    if (raw_p2 - entry).abs() > 1e-9 {
        raw_p2
    } else {
        entry + entry.abs().max(1.0) * 0.01
    }
}

impl LongPosition {
    pub fn new(ts_ms: i64, entry: f64, stop: f64, target: f64, end_ts_ms: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "long_position".to_string(), display_name: "Long Position".to_string(), color: PrimitiveColor::new(color), width: 1.0, ..Default::default() },
            ts_ms, entry_price: entry, stop_loss: stop, take_profit: target, end_ts_ms,
            quantity: 1.0, show_pnl: true, show_live_label: true,
            stop_color: default_stop_color(), target_color: default_target_color(), zone_opacity: default_zone_opacity(),
            label_style: LabelStyle::default(),
        }
    }
    pub fn risk_reward(&self) -> f64 {
        let risk = (self.entry_price - self.stop_loss).abs();
        let reward = (self.take_profit - self.entry_price).abs();
        if risk > 0.0 { reward / risk } else { 0.0 }
    }

    /// Resolve the box's right edge in ms. `end_ts_ms <= ts_ms` covers both
    /// legacy drawings (field defaults to 0) and a degenerate same-tick
    /// placement — fall back to a fixed span off the bar interval rather
    /// than the live viewport edge, so an old drawing still renders as a
    /// finite box instead of stretching to `chart_width()` forever.
    fn resolved_end_ts_ms(&self, bars: &[Bar]) -> i64 {
        if self.end_ts_ms > self.ts_ms {
            self.end_ts_ms
        } else {
            self.ts_ms + DEFAULT_BOX_BARS * crate::bar_interval_seconds(bars) * 1000
        }
    }
}

impl Primitive for LongPosition {
    fn type_id(&self) -> &'static str { "long_position" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Trading }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts_ms, self.entry_price), (self.end_ts_ms, self.take_profit)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(t, p)) = pts.first() { self.ts_ms = t; self.entry_price = p; }
        if let Some(&(t, p)) = pts.get(1) { self.end_ts_ms = t; self.take_profit = p; }
    }
    fn translate(&mut self, td: i64, pd: f64) {
        self.ts_ms += td;
        // Only carry a REAL end_ts_ms along — a legacy/unset one (<= 0)
        // must keep tracking the fallback relative to the new ts_ms rather
        // than being baked into a bogus absolute timestamp near `td`.
        if self.end_ts_ms > 0 { self.end_ts_ms += td; }
        self.entry_price += pd; self.stop_loss += pd; self.take_profit += pd;
    }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.ts_ms = ts_ms; self.entry_price = price; }
            ControlPointType::Point2 => self.stop_loss = price,
            ControlPointType::Point3 => self.take_profit = price,
            // Right-edge box corners — the two corners not already covered
            // by Point2/Point3 (which own the left edge, x1, where the time
            // coordinate is Point1/entry's). Each writes BOTH the time edge
            // (end_ts_ms) and the price level (stop_loss/take_profit) it
            // sits on, so dragging it genuinely resizes the box in both axes.
            ControlPointType::Corner(0) => { self.end_ts_ms = ts_ms; self.stop_loss = price; }
            ControlPointType::Corner(1) => { self.end_ts_ms = ts_ms; self.take_profit = price; }
            // Left-edge time-only handle, mirroring Edge(1) on the right —
            // moves ts_ms alone, entry/stop/target prices untouched. Bounded
            // against end_ts_ms only while the latter holds a REAL value
            // (the same `> self.ts_ms` test `resolved_end_ts_ms` uses to
            // decide "set" vs "fallback"); an unset end_ts_ms tracks a fixed
            // offset ahead of ts_ms through that helper, so it can never be
            // crossed by ts_ms alone and needs no clamp.
            ControlPointType::Edge(0) => {
                if self.end_ts_ms > self.ts_ms {
                    self.ts_ms = ts_ms.min(self.end_ts_ms - 1);
                } else {
                    self.ts_ms = ts_ms;
                }
            }
            ControlPointType::Edge(1) => self.end_ts_ms = ts_ms,
            ControlPointType::Move => { let td = ts_ms - self.ts_ms; let pd = price - self.entry_price; self.translate(td, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts_ms));
        let end_ts = self.resolved_end_ts_ms(bars);
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, end_ts));
        let ye = vp.price_to_y(self.entry_price, ps.price_min, ps.price_max);
        let ys = vp.price_to_y(self.stop_loss, ps.price_min, ps.price_max);
        let yt = vp.price_to_y(self.take_profit, ps.price_min, ps.price_max);
        let mid_y = (ys + yt) / 2.0;
        let r = 8.0;
        if (sx - x1).powi(2) + (sy - ye).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x1).powi(2) + (sy - ys).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        if (sx - x1).powi(2) + (sy - yt).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point3); }
        // Left-edge time-only handle, mirroring Edge(1) on the right.
        // Checked AFTER Point2/Point3 on purpose: their hit circles share
        // x1 with this one, so on a very flat box (stop/target closer than
        // ~2*r on screen) the circles overlap and the price handles —
        // preserved, pre-existing — win the tie by being checked first.
        // The handle stays genuinely reachable off dead-center (a click
        // offset sideways from x1 at mid_y, still within radius r of this
        // point but outside radius r of both ys and yt) for any nonzero
        // box height; it only fully coincides with Point2/Point3 in the
        // degenerate stop_loss == take_profit case, where the box has no
        // height to resize left-to-right along in the first place.
        if (sx - x1).powi(2) + (sy - mid_y).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Edge(0)); }
        // Right-edge box corners, checked before Edge(1) — Edge(1) sits at
        // the same x2 but at the midpoint between ys/yt, so a corner click
        // (exactly at ys or yt) always resolves to the corner, never falls
        // through to the edge handle.
        if (sx - x2).powi(2) + (sy - ys).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Corner(0)); }
        if (sx - x2).powi(2) + (sy - yt).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Corner(1)); }
        if (sx - x2).powi(2) + (sy - mid_y).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Edge(1)); }

        // Every drawn stroke, hit-testable over its true extent — entry/
        // stop/target lines span the full box width, the two vertical box
        // edges span its full height. The interior fill is deliberately
        // NOT hit-testable: grabbing the frame must work on any edge, not
        // on the fill (owner complaint this replaces).
        let (min_y, max_y) = (ys.min(yt), ys.max(yt));
        let strokes = [
            (x1, ye, x2, ye),
            (x1, ys, x2, ys),
            (x1, yt, x2, yt),
            (x1, min_y, x1, max_y),
            (x2, min_y, x2, max_y),
        ];
        for (lx1, ly1, lx2, ly2) in strokes {
            if point_to_line_distance(sx, sy, lx1, ly1, lx2, ly2) < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts_ms));
        let end_ts = self.resolved_end_ts_ms(bars);
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, end_ts));
        let stop_y = vp.price_to_y(self.stop_loss, ps.price_min, ps.price_max);
        let target_y = vp.price_to_y(self.take_profit, ps.price_min, ps.price_max);
        // Diagonal-cursor convention shared with shapes/rectangle.rs:
        // whichever of stop/target sits higher on screen gets the NE-SW
        // resize cursor, the lower one NW-SE.
        let (stop_cursor, target_cursor) = if stop_y <= target_y {
            (ControlPointCursor::ResizeNESW, ControlPointCursor::ResizeNWSE)
        } else {
            (ControlPointCursor::ResizeNWSE, ControlPointCursor::ResizeNESW)
        };
        vec![
            ControlPoint::point1(x1, vp.price_to_y(self.entry_price, ps.price_min, ps.price_max)),
            ControlPoint::point2(x1, stop_y),
            ControlPoint::point3(x1, target_y),
            ControlPoint::new(ControlPointType::Edge(0), x1, (stop_y + target_y) / 2.0, ControlPointCursor::ResizeEW),
            ControlPoint::new(ControlPointType::Corner(0), x2, stop_y, stop_cursor),
            ControlPoint::new(ControlPointType::Corner(1), x2, target_y, target_cursor),
            ControlPoint::new(ControlPointType::Edge(1), x2, (stop_y + target_y) / 2.0, ControlPointCursor::ResizeEW),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let end_ts_ms = {
            let bars = ctx.bars();
            self.resolved_end_ts_ms(bars)
        };
        let x1 = ctx.ts_to_x_ms(self.ts_ms);
        let x2 = ctx.ts_to_x_ms(end_ts_ms);
        let entry_y = ctx.price_to_y(self.entry_price);
        let stop_y = ctx.price_to_y(self.stop_loss);
        let target_y = ctx.price_to_y(self.take_profit);
        let box_width = x2 - x1;
        let alpha = ((self.zone_opacity.clamp(0.0, 1.0)) * 255.0) as u8;

        // Draw stop loss zone, coloured by the configurable stop-zone colour
        let stop_base = if self.stop_color.len() >= 7 { &self.stop_color[..7] } else { &self.stop_color[..] };
        ctx.set_fill_color(&format!("{}{:02x}", stop_base, alpha));
        ctx.fill_rect(crisp(x1, dpr), stop_y.min(entry_y), box_width, (stop_y - entry_y).abs());

        // Draw take profit zone, coloured by the configurable target-zone colour
        let target_base = if self.target_color.len() >= 7 { &self.target_color[..7] } else { &self.target_color[..] };
        ctx.set_fill_color(&format!("{}{:02x}", target_base, alpha));
        ctx.fill_rect(crisp(x1, dpr), target_y.min(entry_y), box_width, (target_y - entry_y).abs());

        // Set line dash based on style
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Draw entry line (the primitive's own stroke color, themable)
        ctx.set_stroke_width(self.data.width);
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(entry_y, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(entry_y, dpr));
        ctx.stroke();

        // Draw stop loss line
        ctx.set_stroke_color(&self.stop_color);
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(stop_y, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(stop_y, dpr));
        ctx.stroke();

        // Draw take profit line
        ctx.set_stroke_color(&self.target_color);
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(target_y, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(target_y, dpr));
        ctx.stroke();

        // Reset line dash
        ctx.set_line_dash(&[]);

        // Entry/stop/target price labels, R:R ratio, quantity and (optionally)
        // the resulting P&L — the whole point of a position tool, and
        // previously nowhere on screen at all.
        let risk_per_unit = (self.entry_price - self.stop_loss).abs();
        let reward_per_unit = (self.take_profit - self.entry_price).abs();
        let qty_suffix = if self.quantity > 0.0 { format!("  x{}", fmt_price(self.quantity)) } else { String::new() };

        let entry_text = format!("Entry {}{}", fmt_price(self.entry_price), qty_suffix);
        draw_line_label(ctx, x1 + 8.0, entry_y, &entry_text, &self.label_style, &self.data.color.stroke);

        let mut stop_text = format!("Stop {}", fmt_price(self.stop_loss));
        if self.show_pnl {
            stop_text.push_str(&format!("  -{}", fmt_price(risk_per_unit * self.quantity)));
        }
        draw_line_label(ctx, x1 + 8.0, stop_y, &stop_text, &self.label_style, &self.stop_color);

        let mut target_text = format!("Target {}  R:R {:.2}", fmt_price(self.take_profit), self.risk_reward());
        if self.show_pnl {
            target_text.push_str(&format!("  +{}", fmt_price(reward_per_unit * self.quantity)));
        }
        draw_line_label(ctx, x1 + 8.0, target_y, &target_text, &self.label_style, &self.target_color);

        // Live price layer — reads `ctx.bars()` the same way the chart's
        // own last-price line does, so this moves every frame on its own.
        // A second, deeper fill over the traversed (entry → current price)
        // slice of whichever zone price is currently sitting in, plus a
        // thin marker line across the FULL box width (not a 40px stub).
        // Gate the whole live layer on the current price sitting inside the
        // box's own price span `[min(stop_loss, take_profit),
        // max(stop_loss, take_profit)]` — once price has run past the box's
        // own bounds the overlay has nothing left to say about this
        // position, so no line, no label, no shading, not clamped to the
        // edge: absent.
        let box_price_lo = self.stop_loss.min(self.take_profit);
        let box_price_hi = self.stop_loss.max(self.take_profit);
        if let Some(last_bar) = ctx.bars().last() {
            let current_price = last_bar.close;
            if current_price >= box_price_lo && current_price <= box_price_hi {
                let current_y = ctx.price_to_y(current_price);

                // Long: price above entry is profit. Clamp the traversed slice
                // to the zone's own boundary — this paints "the position's
                // area" deeper, it does not extend the highlight past the
                // drawn box once price runs beyond stop/target. Both this
                // clamp and the price-in-box gate above resolve to the same
                // stop/target span, so a shaded slice never appears without
                // the marker line that explains it.
                let is_profit_side = current_price > self.entry_price;
                let zone_color: &str = if is_profit_side { &self.target_color } else { &self.stop_color };
                let zone_boundary_y = if is_profit_side { target_y } else { stop_y };
                let (zone_lo, zone_hi) = if entry_y <= zone_boundary_y { (entry_y, zone_boundary_y) } else { (zone_boundary_y, entry_y) };
                let clamped_y = current_y.clamp(zone_lo, zone_hi);
                let zone_base = if zone_color.len() >= 7 { &zone_color[..7] } else { zone_color };
                let live_alpha = ((self.zone_opacity.clamp(0.0, 1.0) + LIVE_ZONE_EXTRA_ALPHA).min(1.0) * 255.0) as u8;
                let traversed_top = entry_y.min(clamped_y);
                let traversed_h = (clamped_y - entry_y).abs();
                if traversed_h > 0.0 {
                    ctx.set_fill_color(&format!("{}{:02x}", zone_base, live_alpha));
                    ctx.fill_rect(crisp(x1, dpr), traversed_top, box_width, traversed_h);
                }

                ctx.set_stroke_color(LIVE_MARKER_COLOR);
                ctx.set_stroke_width(1.0);
                ctx.set_line_dash(&[3.0, 3.0]);
                ctx.begin_path();
                ctx.move_to(crisp(x1, dpr), crisp(current_y, dpr));
                ctx.line_to(crisp(x2, dpr), crisp(current_y, dpr));
                ctx.stroke();
                ctx.set_line_dash(&[]);

                if self.show_live_label {
                    let signed_distance = current_price - self.entry_price;
                    let pct = if self.entry_price != 0.0 { signed_distance / self.entry_price * 100.0 } else { 0.0 };
                    let mut live_text = format!("Live {}  {} ({:+.2}%)", fmt_price(current_price), signed_price(signed_distance), pct);
                    if self.show_pnl {
                        live_text.push_str(&format!("  {}", signed_price(signed_distance * self.quantity)));
                    }

                    // Clear of the entry label when the live price sits close
                    // enough to collide with it — the common case right after
                    // placing the tool, before price has moved away from entry.
                    let collision_band = self.label_style.font_size + self.label_style.background_padding * 2.0 + 4.0;
                    let live_label_y = if (current_y - entry_y).abs() < collision_band {
                        entry_y + collision_band + 2.0
                    } else {
                        current_y
                    };

                    draw_line_label(ctx, x1 + 8.0, live_label_y, &live_text, &self.label_style, LIVE_MARKER_COLOR);
                }
            }
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Bounding box from stop loss to take profit, x1 to x2
                let min_x = x1;
                let max_x = x2;
                let min_y = stop_y.min(target_y).min(entry_y);
                let max_y = stop_y.max(target_y).max(entry_y);

                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => (min_x + max_x) / 2.0,
                    super::super::TextAlign::End => max_x,
                };
                // Start = ABOVE upper boundary, Center = middle, End = BELOW lower boundary
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
        // Risk/reward, recomputed fresh every call (this method runs once
        // per frame — see `PropertyCategory::AppearanceStats`'s own doc) so
        // the three read-only rows below always match the live entry/stop/
        // target points, the same numbers `render`'s own on-chart label
        // shows via the identical formulas.
        let risk_per_unit = (self.entry_price - self.stop_loss).abs();
        let reward_per_unit = (self.take_profit - self.entry_price).abs();

        let mut props = vec![
            ConfigProperty::show_pnl(self.show_pnl)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(10),
            ConfigProperty::quantity(self.quantity)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(11),
            ConfigProperty::stop_zone_color(&self.stop_color)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(12),
            ConfigProperty::target_zone_color(&self.target_color)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(13),
            ConfigProperty::fill_opacity(self.zone_opacity * 100.0)
                .with_category(PropertyCategory::AppearanceFill)
                .with_order(14),
            ConfigProperty::label_font_size(self.label_style.font_size)
                .with_category(PropertyCategory::LabelsText)
                .with_order(15),
            ConfigProperty::label_color(self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke))
                .with_category(PropertyCategory::LabelsText)
                .with_order(16),
            ConfigProperty::show_live_label(self.show_live_label)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(17),
            ConfigProperty::position_risk_stat(&fmt_price(risk_per_unit)).with_order(90),
            ConfigProperty::position_target_stat(&fmt_price(reward_per_unit)).with_order(91),
            ConfigProperty::position_risk_reward_stat(&format!("{:.2}", self.risk_reward())).with_order(92),
        ];
        props.extend(self.label_style.style_properties(&self.data.color.stroke, 30));
        props
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "show_pnl" => {
                if let Some(v) = value.as_bool() { self.show_pnl = v; return true; }
            }
            "show_live_label" => {
                if let Some(v) = value.as_bool() { self.show_live_label = v; return true; }
            }
            "quantity" => {
                if let Some(v) = value.as_number() { self.quantity = v.max(0.0); return true; }
            }
            "stop_color" => {
                if let Some(c) = value.as_color() { self.stop_color = c.to_string(); return true; }
            }
            "target_color" => {
                if let Some(c) = value.as_color() { self.target_color = c.to_string(); return true; }
            }
            "fill_opacity" => {
                if let Some(v) = value.as_number() { self.zone_opacity = (v / 100.0).clamp(0.0, 1.0); return true; }
            }
            "label_font_size" => {
                if let Some(v) = value.as_number() { self.label_style.font_size = v; return true; }
            }
            "label_color" => {
                if let Some(c) = value.as_color() { self.label_style.color = Some(c.to_string()); return true; }
            }
            _ => {}
        }
        if self.label_style.apply_property(id, value, &self.data.color.stroke) { return true; }
        false
    }
}

/// Draw a single price-line label (small text plate) anchored just right of
/// `(x, y)` on its own line.
fn draw_line_label(ctx: &mut dyn RenderContext, x: f64, y: f64, text: &str, label_style: &LabelStyle, fallback_color: &str) {
    ctx.set_font(&label_style.font_string());
    let text_width = ctx.measure_text(text);
    let padding = label_style.background_padding;
    let text_color = label_style.color.as_deref().unwrap_or(fallback_color);

    if let Some(ref bg) = label_style.background_color {
        let bg_y = y - label_style.font_size / 2.0 - padding;
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(x, bg_y, text_width + padding * 2.0, label_style.font_size + padding * 2.0, label_style.background_radius);
        ctx.fill();

        // Gated on the explicit flag — see `LabelStyle::border_enabled`.
        if label_style.border_enabled {
            ctx.set_stroke_color(label_style.border_color.as_deref().unwrap_or(text_color));
            ctx.set_stroke_width(label_style.border_width);
            ctx.stroke();
        }
    }

    ctx.set_fill_color(text_color);
    ctx.set_text_align(crate::render::TextAlign::Left);
    ctx.set_text_baseline(crate::render::TextBaseline::Middle);
    ctx.fill_text(text, x + padding, y);
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "long_position", display_name: "Long Position", kind: PrimitiveKind::Trading,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Buy trade with stop/target", icon: "long_position", default_color: "#4CAF50",
        factory: |points, color| {
            let (t, entry) = points.first().copied().unwrap_or((0, 100.0));
            let (end_t, raw_p2) = points.get(1).copied().unwrap_or((t + 3_600_000, entry + 10.0));
            let p2 = resolve_p2(entry, raw_p2);
            // p2 above entry: it IS the target, stop derived at half the
            // distance (default 1:2 R:R). p2 below entry: it IS the stop,
            // target derived at double the distance.
            let (stop, target) = if p2 > entry {
                (entry - (p2 - entry) / 2.0, p2)
            } else {
                (p2, entry + (entry - p2) * 2.0)
            };
            Box::new(LongPosition::new(t, entry, stop, target, end_t, color))
        },
        supports_text: true,
        has_levels: false,
    }
}
