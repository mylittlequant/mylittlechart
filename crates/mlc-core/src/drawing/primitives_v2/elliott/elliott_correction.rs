//! Elliott Correction Wave - ABC corrective pattern

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, Archetype, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle, CONTROL_POINT_HIT_RADIUS,
    render_primitive_text, draw_label_plate, hit_test_label_plates, wave_label_offset, stroke_leg_segments,
    config::{WaveDegree, LabelStyle, ConfigProperty, PropertyValue, PropertyCategory, SelectOption},
};
use super::count::{WaveLink, WaveSlot, slot_span, segment_is_subdivided, visible_leg_segments};
use super::{guard, ratio, wave_hit};

fn default_true() -> bool { true }
fn default_ratio_rib_style() -> LineStyle { LineStyle::Dotted }

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
pub enum CorrectionType {
    #[default]
    Zigzag,
    Flat,
    Expanded,
    Running,
}

impl CorrectionType {
    /// The same fixed token `style_properties`/`apply_style_property` below
    /// already read and wrote inline (`"zigzag"`/`"flat"`/`"expanded"`/
    /// `"running"`) — named here once so `Primitive::correction_subtype`
    /// (read by `elliott::guard` to pick the right catalogue rule set) uses
    /// the SAME mapping instead of a second copy of it.
    pub fn as_str(&self) -> &'static str {
        match self {
            CorrectionType::Zigzag => "zigzag",
            CorrectionType::Flat => "flat",
            CorrectionType::Expanded => "expanded",
            CorrectionType::Running => "running",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElliottCorrection {
    pub data: PrimitiveData,
    /// 4 points: Start, A, B, C. Each is (timestamp_ms, price).
    pub points: [(i64, f64); 4],
    #[serde(default = "default_true")]
    pub show_labels: bool,
    #[serde(default)]
    pub correction_type: CorrectionType,
    #[serde(default)]
    pub degree: WaveDegree,
    #[serde(default)]
    pub label_style: LabelStyle,
    #[serde(default = "default_true")]
    pub show_lines: bool,
    /// Link to the parent Elliott wave this primitive is nested inside, if
    /// any — see [`super::count::WaveLink`]. `None` (the default) for a
    /// standalone correction, exactly as every correction behaved before
    /// this field existed.
    #[serde(default)]
    pub link: Option<WaveLink>,
    /// The predecessor Elliott primitive's own id this correction continues
    /// — succession, not subdivision (see `super::count`'s own module doc).
    /// `None` (the default) for a correction that opens a count rather
    /// than continuing one.
    #[serde(default)]
    pub follows: Option<u64>,
    /// Show the dotted wave-ratio annotations (B vs A retracement, C vs A
    /// extension — see `super::ratio`). `false` (the default) — off until
    /// asked for, so every drawing saved before this field existed loads
    /// and renders unchanged.
    #[serde(default)]
    pub show_ratios: bool,
    /// The ratio ribs' own stroke colour — see `super::ratio`'s module doc.
    /// `None` (the default) falls back to this structure's own stroke
    /// colour at reduced alpha — EXACTLY what every rib drew before this
    /// field existed, so a drawing saved before this field existed renders
    /// unchanged.
    #[serde(default)]
    pub ratio_rib_color: Option<String>,
    /// The ratio ribs' own stroke width. `None` (the default) falls back to
    /// `data.width * 0.6` (floored at 1px) — EXACTLY what every rib drew
    /// before this field existed.
    #[serde(default)]
    pub ratio_rib_width: Option<f64>,
    /// The ratio ribs' own line style. Defaults to [`LineStyle::Dotted`] —
    /// EXACTLY what every rib drew before this field existed (a fixed 2px
    /// dash, see `ratio::draw_ratio_annotations`'s own dash table).
    #[serde(default = "default_ratio_rib_style")]
    pub ratio_rib_style: LineStyle,
    /// How many of `points` are REAL — see `ElliottImpulse::realized_points`
    /// own doc (`super::elliott_impulse`) for the full rationale
    /// (`DrawingManager::promote_wave_chain`'s own point padding). `None`
    /// (the default) means every point is real, exactly how this primitive
    /// behaved before this field existed. `render` draws the connecting
    /// line and wave labels ONLY for the first `realized_points` points; a
    /// later drag past the realized prefix (`move_control_point`/
    /// `set_points`) raises it back toward the full point count — never
    /// lowers it.
    #[serde(default)]
    pub realized_points: Option<usize>,
    /// The extra violations that depend on this correction's own siblings —
    /// see [`super::super::Primitive::context_violations`]'s own doc. Cache
    /// only, filled by the collection's own owner — skipped on
    /// (de)serialization so it never becomes a second source of truth on
    /// disk.
    #[serde(skip)]
    pub context_violations: Vec<guard::Violation>,
    /// The slots currently occupied by a child subdivision nested inside
    /// THIS correction — see
    /// [`super::super::Primitive::subdivided_slots`]'s own doc. Cache only,
    /// filled by the collection's own owner exactly like
    /// [`context_violations`](Self::context_violations) — skipped on
    /// (de)serialization so it never becomes a second source of truth on
    /// disk.
    #[serde(skip)]
    pub subdivided_slots: Vec<WaveSlot>,
    /// This correction's own point indices where a nested child's own label
    /// lands on the same pivot — see
    /// [`super::super::Primitive::label_collisions`]'s own doc. Cache only,
    /// filled by the collection's own owner exactly like
    /// [`subdivided_slots`](Self::subdivided_slots) — skipped on
    /// (de)serialization so it never becomes a second source of truth on
    /// disk.
    #[serde(skip)]
    pub label_collisions: Vec<usize>,
}

impl ElliottCorrection {
    pub fn new(points: [(i64, f64); 4], color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "elliott_correction".to_string(),
                display_name: "Elliott Correction".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            points,
            show_labels: true,
            correction_type: CorrectionType::Zigzag,
            degree: WaveDegree::Intermediate,
            label_style: LabelStyle::default(),
            show_lines: true,
            link: None,
            follows: None,
            show_ratios: false,
            ratio_rib_color: None,
            ratio_rib_width: None,
            ratio_rib_style: LineStyle::Dotted,
            realized_points: None,
            context_violations: Vec::new(),
            subdivided_slots: Vec::new(),
            label_collisions: Vec::new(),
        }
    }

    pub fn wave_labels(&self) -> [&'static str; 4] {
        let corrective = self.degree.corrective_labels();
        ["", corrective[0], corrective[1], corrective[2]]
    }

    /// How many of [`points`](Self::points) are real right now — see
    /// [`realized_points`](Self::realized_points)'s own field doc. Never
    /// exceeds `points.len()`.
    fn realized_len(&self) -> usize {
        self.realized_points.unwrap_or(self.points.len()).min(self.points.len())
    }

    /// Raise `realized_points` to at least `at_least` (clamped to the point
    /// count) — never lowers it. A no-op when `realized_points` is already
    /// `None`: every point is already real.
    fn grow_realized(&mut self, at_least: usize) {
        if let Some(n) = self.realized_points {
            if at_least > n {
                self.realized_points = Some(at_least.min(self.points.len()));
            }
        }
    }
}

impl Primitive for ElliottCorrection {
    fn type_id(&self) -> &'static str { "elliott_correction" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Pattern }
    // Elliott is `PrimitiveKind::Pattern`, shared with the harmonic
    // patterns — but is `Wave`-shaped, not `Pattern`-shaped, so the default
    // `Primitive::archetype` (kind-derived) is wrong here.
    fn archetype(&self) -> Archetype { Archetype::Wave }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::MultiPointFixed(4) }
    fn wave_link(&self) -> Option<WaveLink> { self.link }
    fn set_wave_link(&mut self, link: Option<WaveLink>) { self.link = link; }
    fn wave_degree(&self) -> Option<WaveDegree> { Some(self.degree) }
    fn wave_slots(&self) -> &'static [WaveSlot] {
        &[WaveSlot::CorrectionA, WaveSlot::CorrectionB, WaveSlot::CorrectionC]
    }
    fn wave_follows(&self) -> Option<u64> { self.follows }
    fn set_wave_follows(&mut self, follows: Option<u64>) { self.follows = follows; }
    fn realized_point_count(&self) -> usize { self.realized_len() }
    /// Write side of `realized_point_count` — the in-progress preview
    /// declares how many pivots the user has actually placed (see
    /// `Primitive::set_realized_point_count`'s own doc). Unlike
    /// `grow_realized`, this may LOWER the count: the preview rebuilds a
    /// fresh primitive on every cursor move, so it states the truth
    /// rather than accumulating it.
    fn set_realized_point_count(&mut self, n: usize) {
        self.realized_points = Some(n.min(self.points.len()));
    }
    fn correction_subtype(&self) -> Option<&'static str> { Some(self.correction_type.as_str()) }
    fn context_violations(&self) -> &[guard::Violation] { &self.context_violations }
    fn set_context_violations(&mut self, v: Vec<guard::Violation>) { self.context_violations = v; }
    fn subdivided_slots(&self) -> &[WaveSlot] { &self.subdivided_slots }
    fn set_subdivided_slots(&mut self, v: Vec<WaveSlot>) { self.subdivided_slots = v; }
    fn label_collisions(&self) -> &[usize] { &self.label_collisions }
    fn set_label_collisions(&mut self, v: Vec<usize>) { self.label_collisions = v; }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { self.points.to_vec() }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        for (i, &p) in pts.iter().take(4).enumerate() { self.points[i] = p; }
        self.grow_realized(pts.len().min(4));
    }
    fn point_labels(&self) -> Option<Vec<String>> {
        Some(self.wave_labels().iter().map(|s| s.to_string()).collect())
    }
    fn translate(&mut self, td: i64, pd: f64) { for p in &mut self.points { p.0 += td; p.1 += pd; } }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Index(i) if (i as usize) < 4 => {
                self.points[i as usize] = (ts_ms, price);
                self.grow_realized(i as usize + 1);
            }
            ControlPointType::Move => { let td = ts_ms - self.points[0].0; let pd = price - self.points[0].1; self.translate(td, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let screen: Vec<_> = self.points.iter().map(|(ts, p)| {
            let b = timestamp_ms_to_bar_f64(bars, *ts);
            (vp.bar_to_x_f64(b), vp.price_to_y(*p, ps.price_min, ps.price_max))
        }).collect();
        for (i, &(x, y)) in screen.iter().enumerate() {
            if (sx - x).powi(2) + (sy - y).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2) { return HitTestResult::ControlPoint(ControlPointType::Index(i as u8)); }
        }
        for i in 0..3 {
            // A leg a child subdivision now nests inside is no longer
            // drawn (see `render`, below) — an invisible leg that still
            // swallows clicks is the same "drawn but not clickable" defect
            // turned inside out.
            if segment_is_subdivided(self.type_id(), i, &self.subdivided_slots) { continue; }
            if ratio::point_to_line_dist(sx, sy, screen[i].0, screen[i].1, screen[i+1].0, screen[i+1].1) < HIT_TOLERANCE { return HitTestResult::Body; }
        }
        // Ratio ribs are secondary edges of the shape, live only while
        // `show_ratios` is on — no behaviour change, no extra hit area,
        // when it is off (see `super::ratio`'s own doc).
        if self.show_ratios && ratio::hit_test_ratio_ribs(sx, sy, &screen, &ratio::CORRECTION_RATIO_RIBS) {
            return HitTestResult::Body;
        }
        // A wave label's own plate is clickable too (owner ask 2026-08-13),
        // gated on `show_labels` exactly like the render side.
        if self.show_labels
            && hit_test_label_plates(sx, sy, &screen, &self.wave_labels(), self.realized_len(), &self.label_style, &self.label_collisions)
        {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        self.points.iter().enumerate().map(|(i, (ts, p))| {
            let b = timestamp_ms_to_bar_f64(bars, *ts);
            ControlPoint::index(i as u8, vp.bar_to_x_f64(b), vp.price_to_y(*p, ps.price_min, ps.price_max))
        }).collect()
    }

    fn wave_at(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Option<WaveSlot> {
        let screen: Vec<_> = self.points.iter().map(|(ts, p)| {
            let b = timestamp_ms_to_bar_f64(bars, *ts);
            (vp.bar_to_x_f64(b), vp.price_to_y(*p, ps.price_min, ps.price_max))
        }).collect();
        let labels = self.wave_labels();
        wave_hit::wave_slot_at(
            sx, sy, self.type_id(), &screen, self.realized_len(), self.wave_slots(),
            &labels, &self.label_style, &self.label_collisions, self.show_labels, self.show_ratios, &ratio::CORRECTION_RATIO_RIBS,
        )
    }

    fn terminal_control_point(&self, _bars: &[Bar], _viewport: &Viewport, _price_scale: &PriceScale) -> Option<ControlPointType> {
        let realized = self.realized_len();
        if realized == 0 { return None; }
        Some(ControlPointType::Index((realized - 1) as u8))
    }

    fn origin_control_point(&self, _bars: &[Bar], _viewport: &Viewport, _price_scale: &PriceScale) -> Option<ControlPointType> {
        if self.realized_len() == 0 { return None; }
        Some(ControlPointType::Index(0))
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let screen: Vec<(f64, f64)> = self.points.iter().map(|(ts, price)| (ctx.ts_to_x_ms(*ts), ctx.price_to_y(*price))).collect();
        let realized = self.realized_len();
        // Hard-rule guard (`super::guard`) — independent of `show_ratios`,
        // a broken count is broken whether or not the ratio ribs show. Never
        // blocks the drawing; only recolors the offending segment/label
        // below. Empty for a compliant structure, so a structure with no
        // violations renders byte-for-byte as before this existed. Merges
        // the SELF-only checks (recomputed live every frame, so they keep
        // updating while the user drags) with the CONTEXT cache
        // (`self.context_violations()`, filled by the collection's own
        // owner — see `Primitive::context_violations`'s own doc) so a
        // context violation marks its wave exactly like a self-only one.
        let mut violations = guard::violations(self);
        violations.extend_from_slice(self.context_violations());

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);

        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        if self.show_lines {
            // A slot in `subdivided_slots` no longer draws its own leg here
            // — the child subdivision nested into it draws that stretch of
            // the count now (owner report 2026-08-13: a subdivision must
            // REPLACE the leg, not overlay it). `visible_leg_segments`
            // returns one unbroken run when nothing is subdivided, so this
            // draws byte-for-byte the same path as before subdivision
            // skipping existed for a plain (non-subdivided) correction.
            let segments = visible_leg_segments(self.type_id(), realized, &self.subdivided_slots);
            stroke_leg_segments(ctx, &screen, &segments, dpr);
        }

        ctx.set_line_dash(&[]);

        if self.show_lines && !violations.is_empty() {
            ctx.set_stroke_color(guard::VIOLATION_COLOR);
            ctx.set_stroke_width(self.data.width);
            for v in &violations {
                // A subdivided wave's own leg is not drawn above — its
                // violation is still marked on its LABEL plate below, but
                // there is no leg left here to recolor.
                if self.subdivided_slots.contains(&v.wave) {
                    continue;
                }
                if let Some((a, b)) = slot_span(self.type_id(), v.wave) {
                    if b < realized {
                        ctx.begin_path();
                        ctx.move_to(crisp(screen[a].0, dpr), crisp(screen[a].1, dpr));
                        ctx.line_to(crisp(screen[b].0, dpr), crisp(screen[b].1, dpr));
                        ctx.stroke();
                    }
                }
            }
        }

        if self.show_labels {
            let labels = self.wave_labels();
            let label_color = self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke);
            ctx.set_fill_color(label_color);
            ctx.set_font(&self.label_style.font_string());
            ctx.set_text_align(crate::render::TextAlign::Center);
            ctx.set_text_baseline(crate::render::TextBaseline::Middle);

            for (i, label) in labels.iter().enumerate() {
                if i >= realized || label.is_empty() { continue; }
                let (x, y) = screen[i];
                let collides = self.label_collisions.contains(&i);
                let offset = wave_label_offset(&screen, i, &self.label_style, collides);
                let is_violating = violations.iter().any(|v| slot_span(self.type_id(), v.wave).map(|(_, e)| e) == Some(i));
                let this_label_color = if is_violating { guard::VIOLATION_COLOR } else { label_color };
                draw_label_plate(ctx, x, y + offset, label, self.label_style.font_size, &self.label_style, this_label_color);
            }
        }

        if self.show_ratios {
            ratio::draw_ratio_annotations(
                ctx, &self.points, realized, &ratio::CORRECTION_RATIO_LEGS, &ratio::CORRECTION_RATIO_RIBS,
                &self.data.color.stroke, self.data.width, &self.label_style,
                self.ratio_rib_color.as_deref(), self.ratio_rib_width, self.ratio_rib_style,
            );
        }

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let min_y = screen.iter().fold(f64::INFINITY, |a, (_, y)| a.min(*y));
                let max_y = screen.iter().fold(f64::NEG_INFINITY, |a, (_, y)| a.max(*y));
                let min_x = screen.iter().fold(f64::INFINITY, |a, (x, _)| a.min(*x));
                let max_x = screen.iter().fold(f64::NEG_INFINITY, |a, (x, _)| a.max(*x));

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
        let degree_options: Vec<SelectOption> = WaveDegree::all()
            .iter()
            .map(|d| SelectOption::new(d.as_str(), d.display_name()))
            .collect();

        let rib_color_default = self.ratio_rib_color.clone().unwrap_or_else(|| ratio::default_rib_color(&self.data.color.stroke));
        let rib_width_default = self.ratio_rib_width.unwrap_or_else(|| ratio::default_rib_width(self.data.width));

        let mut props = vec![
            // The pattern's own subtype — changes what shape it draws, so
            // it lives on Appearance next to the other shape-defining
            // controls, same placement as `patterns/triangle_pattern.rs`'s
            // own `triangle_type`. Real field, zero `ConfigProperty` until
            // this pass (2026-08-09 modal-surface-inventory audit).
            ConfigProperty::correction_type(self.correction_type.as_str()).with_category(PropertyCategory::AppearanceLine).with_order(9),
            ConfigProperty::show_labels(self.show_labels).with_category(PropertyCategory::AppearanceLine).with_order(10),
            ConfigProperty::show_lines(self.show_lines).with_category(PropertyCategory::AppearanceLine).with_order(11),
            ConfigProperty::show_ratios(self.show_ratios).with_category(PropertyCategory::AppearanceLine).with_order(12),
            // The ratio ribs' own style — next to `show_ratios` (see
            // `super::ratio`'s own module doc).
            ConfigProperty::ratio_rib_color(&rib_color_default).with_category(PropertyCategory::AppearanceLine).with_order(13),
            ConfigProperty::ratio_rib_width(rib_width_default).with_category(PropertyCategory::AppearanceLine).with_order(14),
            ConfigProperty::ratio_rib_style(self.ratio_rib_style.as_str()).with_category(PropertyCategory::AppearanceLine).with_order(15),
            ConfigProperty::wave_degree(self.degree.as_str(), degree_options).with_category(PropertyCategory::AppearanceLine).with_order(16),
            ConfigProperty::label_font_size(self.label_style.font_size).with_category(PropertyCategory::LabelsText).with_order(17),
            ConfigProperty::label_color(self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke)).with_category(PropertyCategory::LabelsText).with_order(18),
        ];
        props.extend(self.label_style.style_properties(&self.data.color.stroke, 30));
        props
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "correction_type" => {
                if let Some(s) = value.as_string() {
                    self.correction_type = match s {
                        "flat" => CorrectionType::Flat,
                        "expanded" => CorrectionType::Expanded,
                        "running" => CorrectionType::Running,
                        _ => CorrectionType::Zigzag,
                    };
                    return true;
                }
            }
            "show_labels" => { if let Some(v) = value.as_bool() { self.show_labels = v; return true; } }
            "show_lines" => { if let Some(v) = value.as_bool() { self.show_lines = v; return true; } }
            "show_ratios" => { if let Some(v) = value.as_bool() { self.show_ratios = v; return true; } }
            "ratio_rib_color" => { if let Some(c) = value.as_color() { self.ratio_rib_color = Some(c.to_string()); return true; } }
            "ratio_rib_width" => { if let Some(v) = value.as_number() { self.ratio_rib_width = Some(v); return true; } }
            "ratio_rib_style" => { if let Some(s) = value.as_string() { self.ratio_rib_style = LineStyle::from_str(s); return true; } }
            "degree" => { if let Some(s) = value.as_string() { if let Some(d) = WaveDegree::from_str(s) { self.degree = d; return true; } } }
            "label_font_size" => { if let Some(v) = value.as_number() { self.label_style.font_size = v; return true; } }
            "label_color" => { if let Some(c) = value.as_color() { self.label_style.color = Some(c.to_string()); return true; } }
            _ => {}
        }
        if self.label_style.apply_property(id, value, &self.data.color.stroke) { return true; }
        false
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "elliott_correction", display_name: "Elliott Correction", kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::MultiPointFixed(4), tooltip: "ABC corrective pattern", icon: "elliott_correction", default_color: "#FF5722",
        factory: |points, color| {
            let mut arr = [(0i64, 0.0f64); 4];
            for (i, &p) in points.iter().take(4).enumerate() { arr[i] = p; }
            Box::new(ElliottCorrection::new(arr, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod subdivision_overlay_tests {
    use super::*;

    /// Deliverable 2/3 (owner report 2026-08-13): once wave B carries a
    /// child subdivision, the segment list `render` would stroke excludes
    /// wave B's own span `(1, 2)` and still includes wave A and wave C.
    #[test]
    fn subdivided_wave_b_excludes_its_own_span_from_render_segments() {
        let points: [(i64, f64); 4] = [(0, 100.0), (1_000, 90.0), (2_000, 95.0), (3_000, 85.0)];
        let mut correction = ElliottCorrection::new(points, "#FF5722");
        correction.set_subdivided_slots(vec![WaveSlot::CorrectionB]);

        let segments =
            visible_leg_segments(correction.type_id(), correction.realized_point_count(), correction.subdivided_slots());
        assert_eq!(
            segments,
            vec![(0, 1), (2, 3)],
            "wave B's own span (1, 2) is replaced by its child, wave A and wave C are unaffected"
        );
    }
}
