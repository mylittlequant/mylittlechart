//! Cypher Pattern primitive - harmonic pattern variant

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::i18n::{current_language, ConceptKey};
use mli::ewa::harmonic::{self, HarmonicLeg};
use mli::ewa::types::EwaPatternKind;
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle, CONTROL_POINT_HIT_RADIUS,
    render_primitive_text, draw_label_plate,
    config::{
        LabelStyle, ConfigProperty, PropertyValue, PropertyCategory, PropertyGroup,
        TableCell, Tone, TARGET_RATIOS_PROPERTY_ID,
    },
    elliott::guard,
};

fn default_true() -> bool { true }

fn default_harmonic_tolerance() -> f64 { harmonic::DEFAULT_HARMONIC_TOLERANCE }

/// The smallest value `harmonic_tolerance`'s own `Number` row allows — see
/// `xabcd_pattern::MIN_HARMONIC_TOLERANCE`'s own doc for why `0.0` (not a
/// positive epsilon) is the correct floor: `widen_band` narrows rather
/// than widens below `0.0`, which can invert a band (`min > max`) and
/// permanently fail every ratio on that leg; `0.0` itself is a no-op
/// (published band, unchanged) and never fails a well-formed ratio on
/// float imprecision, since every "exact" target is already a
/// `±DEFAULT_HARMONIC_TOLERANCE` band, not a bit-exact equality check.
const MIN_HARMONIC_TOLERANCE: f64 = 0.0;

/// This `Number` row's own id — shared by `style_properties()` (which
/// declares it) and `apply_style_property` (which parses it back).
const HARMONIC_TOLERANCE_PROPERTY_ID: &str = "harmonic_tolerance";

/// This primitive's own pivot names, in point-index order — see
/// `xabcd_pattern::PIVOT_NAMES`'s own doc for why this is duplicated per
/// file rather than shared: this primitive's own point ARRAY SIZE differs
/// per harmonic type, so there is no one generic array type to share it
/// through.
const PIVOT_NAMES: [&str; 5] = ["X", "A", "B", "C", "D"];

/// Cypher's own identity is fixed — `EwaPatternKind::Cypher` — this
/// primitive type never needs a field the way `XabcdPattern` does.
const PATTERN_KIND: EwaPatternKind = EwaPatternKind::Cypher;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CypherPattern {
    pub data: PrimitiveData,
    pub points: [(i64, f64); 5], // X, A, B, C, D
    #[serde(default = "default_true")]
    pub show_labels: bool,
    #[serde(default = "default_true")]
    pub show_ratios: bool,
    #[serde(default)]
    pub label_style: LabelStyle,
    #[serde(default = "default_true")]
    pub show_lines: bool,
    /// Widening delta (`mli::ewa::harmonic::widen_band`) applied to every
    /// target leg's own band before matching — defaults to the
    /// vocabulary's own [`harmonic::DEFAULT_HARMONIC_TOLERANCE`], never a
    /// second tolerance constant of this crate's own.
    #[serde(default = "default_harmonic_tolerance")]
    pub harmonic_tolerance: f64,
}

impl CypherPattern {
    pub fn new(points: [(i64, f64); 5], color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "cypher_pattern".to_string(),
                display_name: "Cypher Pattern".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            points,
            show_labels: true,
            show_ratios: true,
            label_style: LabelStyle::default(),
            show_lines: true,
            harmonic_tolerance: harmonic::DEFAULT_HARMONIC_TOLERANCE,
        }
    }
}

/// `length(leg.leg) / length(leg.reference)` for this pattern's own points
/// — the SAME per-price-difference computation used for every on-chart
/// ratio label in `render()` and for the target-ratio table
/// (`target_ratio_table_property`) — one function, never a second copy of
/// the arithmetic. `None` when a named pivot cannot be resolved, or when
/// the reference leg has zero length.
fn measured_ratio(points: &[(i64, f64); 5], leg: &HarmonicLeg) -> Option<f64> {
    let idx = |name: &str| PIVOT_NAMES.iter().position(|&n| n == name);
    let (l0, l1) = (idx(leg.leg.0)?, idx(leg.leg.1)?);
    let (r0, r1) = (idx(leg.reference.0)?, idx(leg.reference.1)?);
    let leg_len = (points[l1].1 - points[l0].1).abs();
    let ref_len = (points[r1].1 - points[r0].1).abs();
    (ref_len > 0.0).then(|| leg_len / ref_len)
}

/// Which of this primitive's own already-drawn segments (the solid
/// X-A-B-C-D run, or the dashed X-B/A-C projections) shows `leg` — every
/// Cypher target leg (AB, AC, CD) resolves to one of these, so unlike
/// `xabcd_pattern`'s own version of this function there is no leg here
/// that ever falls through to `None`.
fn leg_line_span(leg: (&str, &str)) -> Option<(usize, usize)> {
    match leg {
        ("X", "A") => Some((0, 1)),
        ("A", "B") => Some((1, 2)),
        ("B", "C") => Some((2, 3)),
        ("C", "D") => Some((3, 4)),
        ("X", "B") => Some((0, 2)),
        ("A", "C") => Some((1, 3)),
        _ => None,
    }
}

/// The design's own "target ratios" table (`ConceptKey::TargetRatiosGroupTitle`,
/// `PropertyType::Table`) — one row per [`HarmonicLeg`] in
/// `mli::ewa::harmonic::targets_for(PATTERN_KIND)`: the leg, its
/// (tolerance-widened) target band, and the measured ratio
/// ([`measured_ratio`] — the SAME function `render()` uses for its own
/// on-chart labels), toned by [`harmonic::ratio_matches`] (never a local
/// comparison).
fn target_ratio_table_property(points: &[(i64, f64); 5], tolerance: f64) -> Option<ConfigProperty> {
    let table = harmonic::targets_for(PATTERN_KIND);
    if table.is_empty() {
        return None;
    }
    let lang = current_language();
    let head = vec![
        ConceptKey::SegmentColumnHeader.get(lang).to_string(),
        ConceptKey::ToleranceColumnHeader.get(lang).to_string(),
        ConceptKey::ActualColumnHeader.get(lang).to_string(),
    ];
    let rows: Vec<Vec<TableCell>> = table
        .iter()
        .map(|entry| {
            let band = harmonic::widen_band(entry.band, tolerance);
            let segment = TableCell::Text(format!("{}{}", entry.leg.0, entry.leg.1));
            let target = TableCell::Text(format!("{:.3}\u{2013}{:.3}", band.min, band.max));
            let actual = match measured_ratio(points, entry) {
                Some(ratio) => {
                    let widened = HarmonicLeg { band, ..*entry };
                    let tone = if harmonic::ratio_matches(ratio, &widened) { Tone::Good } else { Tone::Bad };
                    TableCell::Toned(format!("{ratio:.3}"), tone)
                }
                None => TableCell::Text("\u{2014}".to_string()),
            };
            vec![segment, target, actual]
        })
        .collect();
    Some(
        ConfigProperty::table(TARGET_RATIOS_PROPERTY_ID, ConceptKey::TargetRatiosGroupTitle.get(lang), head, rows)
            .with_category(PropertyCategory::AppearanceStats)
            .with_order(20)
            .in_group(PropertyGroup::TargetRatios),
    )
}

impl Primitive for CypherPattern {
    fn type_id(&self) -> &'static str { "cypher_pattern" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Pattern }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::MultiPointFixed(5) }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { self.points.to_vec() }
    fn set_points(&mut self, pts: &[(i64, f64)]) { for (i, &p) in pts.iter().take(5).enumerate() { self.points[i] = p; } }
    fn translate(&mut self, ts_delta_ms: i64, pd: f64) { for p in &mut self.points { p.0 += ts_delta_ms; p.1 += pd; } }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Index(i) if (i as usize) < 5 => self.points[i as usize] = (ts_ms, price),
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
        for i in 0..4 {
            if point_to_line_dist(sx, sy, screen[i].0, screen[i].1, screen[i+1].0, screen[i+1].1) < HIT_TOLERANCE { return HitTestResult::Body; }
        }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        self.points.iter().enumerate().map(|(i, (ts, p))| {
            let b = timestamp_ms_to_bar_f64(bars, *ts);
            ControlPoint::index(i as u8, vp.bar_to_x_f64(b), vp.price_to_y(*p, ps.price_min, ps.price_max))
        }).collect()
    }
    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let screen: Vec<_> = self.points.iter().map(|(ts, p)| (ctx.ts_to_x_ms(*ts), ctx.price_to_y(*p))).collect();
        let labels = PIVOT_NAMES;

        let target_table = harmonic::targets_for(PATTERN_KIND);
        // Every currently out-of-band target leg (tolerance-widened,
        // `ratio_matches` — the SAME rule `style_properties()`'s own table
        // reads).
        let out_of_band: Vec<HarmonicLeg> = target_table
            .iter()
            .filter_map(|entry| {
                let ratio = measured_ratio(&self.points, entry)?;
                let widened = HarmonicLeg { band: harmonic::widen_band(entry.band, self.harmonic_tolerance), ..*entry };
                (!harmonic::ratio_matches(ratio, &widened)).then_some(*entry)
            })
            .collect();

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Draw connecting lines X-A-B-C-D
        if self.show_lines {
            ctx.begin_path();
            ctx.move_to(crisp(screen[0].0, dpr), crisp(screen[0].1, dpr));
            for (x, y) in screen.iter().skip(1) {
                ctx.line_to(crisp(*x, dpr), crisp(*y, dpr));
            }
            ctx.stroke();

            // Draw X-B and A-C projection lines (dashed)
            ctx.set_line_dash(&[4.0, 4.0]);
            ctx.begin_path();
            ctx.move_to(crisp(screen[0].0, dpr), crisp(screen[0].1, dpr));
            ctx.line_to(crisp(screen[2].0, dpr), crisp(screen[2].1, dpr));
            ctx.stroke();
            ctx.begin_path();
            ctx.move_to(crisp(screen[1].0, dpr), crisp(screen[1].1, dpr));
            ctx.line_to(crisp(screen[3].0, dpr), crisp(screen[3].1, dpr));
            ctx.stroke();
            ctx.set_line_dash(&[]);

            // Out-of-band marking — same register the Elliott guard already
            // uses for a broken rule (`elliott::guard::VIOLATION_COLOR`,
            // recolouring the offending segment). Every Cypher target leg
            // resolves to a drawn segment (`leg_line_span`), so this loop
            // never skips one.
            if !out_of_band.is_empty() {
                ctx.set_stroke_color(guard::VIOLATION_COLOR);
                ctx.set_stroke_width(self.data.width);
                for entry in &out_of_band {
                    if let Some((a, b)) = leg_line_span(entry.leg) {
                        ctx.begin_path();
                        ctx.move_to(crisp(screen[a].0, dpr), crisp(screen[a].1, dpr));
                        ctx.line_to(crisp(screen[b].0, dpr), crisp(screen[b].1, dpr));
                        ctx.stroke();
                    }
                }
                ctx.set_stroke_color(&self.data.color.stroke);
            }
        }

        // Draw labels
        if self.show_labels {
            let label_color = self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke);
            ctx.set_fill_color(label_color);
            ctx.set_font(&self.label_style.font_string());
            ctx.set_text_align(crate::render::TextAlign::Center);
            ctx.set_text_baseline(crate::render::TextBaseline::Middle);

            for (i, (x, y)) in screen.iter().enumerate() {
                let label = labels[i];
                let offset = if i % 2 == 0 { -self.label_style.offset_y } else { self.label_style.offset_y };
                draw_label_plate(ctx, *x, *y + offset, label, self.label_style.font_size, &self.label_style, label_color);
            }
        }

        if self.show_ratios {
            let ratio_color = self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke);
            let ratio_font_size = (self.label_style.font_size - 1.0).max(9.0);
            ctx.set_font(&format!("{}px sans-serif", ratio_font_size as i32));
            ctx.set_fill_color(ratio_color);
            ctx.set_text_align(crate::render::TextAlign::Center);
            ctx.set_text_baseline(crate::render::TextBaseline::Middle);

            // One label per target leg (AB/XA, AC/XA, CD/XC — Cypher's own
            // table, `mli::ewa::harmonic::targets_for`), using the SAME
            // `measured_ratio` the target-ratio table reads, marked with
            // `guard::VIOLATION_COLOR` when out of band.
            for entry in target_table {
                let Some(ratio) = measured_ratio(&self.points, entry) else { continue };
                let is_violating = out_of_band.contains(entry);
                let this_label_color = if is_violating { guard::VIOLATION_COLOR } else { ratio_color };
                if let Some((a, b)) = leg_line_span(entry.leg) {
                    draw_ratio_label(ctx, screen[a], screen[b], ratio, ratio_font_size, &self.label_style, this_label_color);
                }
            }
        }

        // Render text if present
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
        let mut props = vec![
            ConfigProperty::show_labels(self.show_labels)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(10),
            ConfigProperty::show_lines(self.show_lines)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(11),
            ConfigProperty::show_ratios(self.show_ratios)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(12),
            // `PropertyGroup::PointLabels` (W2) — the approved pattern
            // archetype's own `PointLabelsGroupTitle`, matching
            // `XabcdPattern::style_properties()`'s own choice for the same
            // font/colour pair.
            ConfigProperty::label_font_size(self.label_style.font_size)
                .with_category(PropertyCategory::LabelsText)
                .with_order(13).in_group(PropertyGroup::PointLabels),
            ConfigProperty::label_color(self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke))
                .with_category(PropertyCategory::LabelsText)
                .with_order(14).in_group(PropertyGroup::PointLabels),
        ];
        props.extend(self.label_style.style_properties_grouped(&self.data.color.stroke, 30, PropertyGroup::PointLabels));
        // Tolerance — declared in `PropertyGroup::TargetRatios` beside the
        // table it governs, written back through `apply_style_property`
        // like every other declared property.
        props.push(
            ConfigProperty::number(HARMONIC_TOLERANCE_PROPERTY_ID, ConceptKey::ToleranceColumnHeader.get(current_language()), self.harmonic_tolerance, Some(MIN_HARMONIC_TOLERANCE), Some(0.2))
                .with_category(PropertyCategory::AppearanceStats)
                .with_order(19)
                .in_group(PropertyGroup::TargetRatios),
        );
        if let Some(table) = target_ratio_table_property(&self.points, self.harmonic_tolerance) {
            props.push(table);
        }
        props
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "show_labels" => {
                if let Some(v) = value.as_bool() { self.show_labels = v; return true; }
            }
            "show_lines" => {
                if let Some(v) = value.as_bool() { self.show_lines = v; return true; }
            }
            "show_ratios" => {
                if let Some(v) = value.as_bool() { self.show_ratios = v; return true; }
            }
            HARMONIC_TOLERANCE_PROPERTY_ID => {
                if let Some(v) = value.as_number() { self.harmonic_tolerance = v.max(MIN_HARMONIC_TOLERANCE); return true; }
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

fn draw_ratio_label(ctx: &mut dyn RenderContext, p1: (f64, f64), p2: (f64, f64), ratio: f64, font_size: f64, label_style: &LabelStyle, label_color: &str) {
    let mid_x = (p1.0 + p2.0) / 2.0;
    let mid_y = (p1.1 + p2.1) / 2.0;
    let dx = p2.0 - p1.0;
    let dy = p2.1 - p1.1;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let ox = -dy / len * 10.0;
    let oy = dx / len * 10.0;
    let label = format!("{:.3}", ratio);
    draw_label_plate(ctx, mid_x + ox, mid_y + oy, &label, font_size, label_style, label_color);
}

fn point_to_line_dist(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 { return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt(); }
    let t = ((px - x1) * dx + (py - y1) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);
    ((px - (x1 + t * dx)).powi(2) + (py - (y1 + t * dy)).powi(2)).sqrt()
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "cypher_pattern", display_name: "Cypher Pattern", kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::MultiPointFixed(5), tooltip: "Cypher harmonic pattern", icon: "cypher_pattern", default_color: "#673AB7",
        factory: |points, color| {
            let mut arr = [(0i64, 0.0); 5];
            for (i, &p) in points.iter().take(5).enumerate() { arr[i] = p; }
            Box::new(CypherPattern::new(arr, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod harmonic_tests {
    use super::*;
    use super::super::super::config::PropertyType;

    /// A Cypher-shaped drawing, by construction: AB/XA = 0.5 (in
    /// [0.382, 0.618]), AC/XA = 1.34 (in [1.272, 1.414]), CD/XC = 0.786
    /// (exact) — every leg lands inside its own Cypher target band
    /// (`mli::ewa::harmonic::CYPHER`, read via `targets_for`, never
    /// re-typed here).
    fn cypher_points() -> [(i64, f64); 5] {
        [(0, 0.0), (1_000, 100.0), (2_000, 50.0), (3_000, 234.0), (4_000, 50.076)]
    }

    #[test]
    fn cypher_shaped_drawing_reports_every_leg_inside() {
        let prim = CypherPattern::new(cypher_points(), "#673AB7");
        let table = harmonic::targets_for(PATTERN_KIND);
        assert_eq!(table.len(), 3, "Cypher's own table has 3 legs");
        for entry in table {
            let ratio = measured_ratio(&prim.points, entry).expect("nonzero reference on this fixture");
            let widened = HarmonicLeg { band: harmonic::widen_band(entry.band, prim.harmonic_tolerance), ..*entry };
            assert!(
                harmonic::ratio_matches(ratio, &widened),
                "leg {:?} measured {ratio} should be inside its own band {:?}",
                entry.leg, widened.band
            );
        }
    }

    #[test]
    fn moving_one_pivot_reports_exactly_that_leg_outside() {
        let mut prim = CypherPattern::new(cypher_points(), "#673AB7");
        // B is used ONLY by the AB leg (AC and CD's own reference is XA/XC,
        // neither touches B) — moving it in isolation breaks exactly one
        // leg. AB/XA goes from 0.5 to 0.70, clearly outside [0.382, 0.618].
        prim.points[2].1 = 30.0;

        let table = harmonic::targets_for(PATTERN_KIND);
        let out_of_band: Vec<(&str, &str)> = table
            .iter()
            .filter(|entry| {
                let ratio = measured_ratio(&prim.points, entry).expect("nonzero reference on this fixture");
                let widened = HarmonicLeg { band: harmonic::widen_band(entry.band, prim.harmonic_tolerance), ..**entry };
                !harmonic::ratio_matches(ratio, &widened)
            })
            .map(|entry| entry.leg)
            .collect();

        assert_eq!(out_of_band, vec![("A", "B")], "exactly the AB leg should report outside, nothing else: {out_of_band:?}");
    }

    #[test]
    fn table_actual_column_equals_the_measured_ratio_the_labels_use() {
        let prim = CypherPattern::new(cypher_points(), "#673AB7");
        let table_property = target_ratio_table_property(&prim.points, prim.harmonic_tolerance)
            .expect("Cypher is always classified");
        let PropertyType::Table { rows, .. } = &table_property.prop_type else { panic!("must be PropertyType::Table") };

        let target_table = harmonic::targets_for(PATTERN_KIND);
        assert_eq!(rows.len(), target_table.len());
        for (row, entry) in rows.iter().zip(target_table.iter()) {
            let TableCell::Toned(actual_text, _) = &row[2] else { panic!("Actual column must be a Toned cell") };
            let actual: f64 = actual_text.parse().expect("Actual cell must be a formatted number");
            let expected = measured_ratio(&prim.points, entry).expect("nonzero reference on this fixture");
            assert!((actual - expected).abs() < 1e-3, "leg {:?}: table shows {actual}, measured_ratio gives {expected}", entry.leg);
        }
    }

    #[test]
    fn applying_tolerance_through_apply_path_widens_the_table_bands() {
        let mut prim = CypherPattern::new(cypher_points(), "#673AB7");
        let applied = prim.apply_style_property(HARMONIC_TOLERANCE_PROPERTY_ID, &PropertyValue::Number(0.05));
        assert!(applied, "apply_style_property must recognise the harmonic_tolerance id");
        assert!((prim.harmonic_tolerance - 0.05).abs() < 1e-12);

        let table_property = target_ratio_table_property(&prim.points, prim.harmonic_tolerance)
            .expect("Cypher is always classified");
        let PropertyType::Table { rows, .. } = &table_property.prop_type else { panic!("must be PropertyType::Table") };

        // AB's own published band [0.382, 0.618] widened by 0.05 becomes
        // [0.332, 0.668].
        let TableCell::Text(band_text) = &rows[0][1] else { panic!("Tolerance column must be Text") };
        assert_eq!(band_text, "0.332\u{2013}0.668");
    }
}
