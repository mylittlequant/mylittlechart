//! XABCD Pattern primitive - 5-point harmonic pattern

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::i18n::{current_language, ConceptKey, ConfigKey, Language};
use mli::ewa::harmonic::{self, HarmonicLeg};
use mli::ewa::types::EwaPatternKind;
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle, CONTROL_POINT_HIT_RADIUS,
    render_primitive_text, draw_label_plate,
    config::{
        LabelStyle, ConfigProperty, PropertyValue, PropertyCategory, PropertyGroup,
        SelectOption, TableCell, Tone, TARGET_RATIOS_PROPERTY_ID,
    },
    elliott::guard,
};

fn default_true() -> bool { true }

fn default_harmonic_tolerance() -> f64 { harmonic::DEFAULT_HARMONIC_TOLERANCE }

/// The smallest value `harmonic_tolerance`'s own `Select`/`Number` row
/// allows. `widen_band(band, delta)` (`mli::ewa::harmonic`) adds `delta` to
/// a band's own `max` and SUBTRACTS it from `min` — a NEGATIVE delta
/// therefore narrows the band rather than widening it, and once `delta`
/// passes below a band's own half-width, `min` ends up greater than `max`:
/// `RatioBand::contains` (`value >= min && value <= max`) can then never be
/// true again, so `ratio_matches` would report every ratio on that leg as
/// out of band regardless of how well the drawing actually matches — a
/// permanent, un-fixable failure. `0.0` is the smallest delta that can
/// never invert ANY band in `mli::ewa::harmonic`'s own table (at `0.0`,
/// `widen_band` is a no-op — every band renders exactly as published,
/// which is itself already tolerant: an "exact" target like Gartley's own
/// `AB = 0.618 XA` is a `±DEFAULT_HARMONIC_TOLERANCE` band, not a bit-exact
/// equality check, so `0.0` never makes a well-formed leg fail on float
/// imprecision either).
const MIN_HARMONIC_TOLERANCE: f64 = 0.0;

/// This `Select` row's own id — shared by `style_properties()` (which
/// declares it) and `apply_style_property` (which parses it back), the
/// same "one constant, both directions" contract every other property id
/// in this file already follows.
const PATTERN_KIND_PROPERTY_ID: &str = "pattern_kind";

/// The `harmonic_tolerance` field's own property id — see
/// [`PATTERN_KIND_PROPERTY_ID`]'s own doc for why this is a constant.
const HARMONIC_TOLERANCE_PROPERTY_ID: &str = "harmonic_tolerance";

/// This primitive's own pivot names, in point-index order — the same
/// letters `render()`'s own point-label loop draws, reused so a
/// [`HarmonicLeg`]'s pivot names (`mli::ewa::harmonic`) resolve to a point
/// index without a second lookup table.
const PIVOT_NAMES: [&str; 5] = ["X", "A", "B", "C", "D"];

/// Which named harmonic pattern this XABCD drawing claims to be — the
/// identity `mli::ewa::harmonic::targets_for` needs to pick the right
/// target-ratio table. `Unclassified` (the default) reproduces today's
/// behaviour exactly: no table, no target-band recolouring — matching
/// `mli::ewa::harmonic::targets_for(EwaPatternKind::Xabcd)`, which is
/// deliberately empty for the unclassified five-point container (see that
/// function's own doc).
///
/// `mli::ewa::types::EwaPatternKind` is the vocabulary's own type for this,
/// but it derives neither `Serialize` nor `Deserialize` (it names EVERY
/// Elliott/harmonic shape, not a wire type this crate controls) — so a
/// `#[derive(Serialize, Deserialize)]` primitive cannot store it directly.
/// This type mirrors the five named-harmonic variants this primitive can
/// actually claim and maps onto the vocabulary's own enum in exactly one
/// place, [`XabcdHarmonicKind::as_ewa_kind`] — no second vocabulary, just a
/// serializable shim over the same five names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum XabcdHarmonicKind {
    #[default]
    Unclassified,
    Gartley,
    Bat,
    AltBat,
    Butterfly,
    Crab,
    DeepCrab,
    Shark,
}

impl XabcdHarmonicKind {
    /// Maps to `mli::ewa::harmonic::targets_for`'s own vocabulary.
    /// `Unclassified` maps to `EwaPatternKind::Xabcd` — the unclassified
    /// container `targets_for` itself always returns an empty table for.
    pub fn as_ewa_kind(self) -> EwaPatternKind {
        match self {
            Self::Unclassified => EwaPatternKind::Xabcd,
            Self::Gartley => EwaPatternKind::Gartley,
            Self::Bat => EwaPatternKind::Bat,
            Self::AltBat => EwaPatternKind::AltBat,
            Self::Butterfly => EwaPatternKind::Butterfly,
            Self::Crab => EwaPatternKind::Crab,
            Self::DeepCrab => EwaPatternKind::DeepCrab,
            Self::Shark => EwaPatternKind::Shark,
        }
    }

    /// The `Select` row's own value id for this variant — stable, ASCII,
    /// never localized (the wire/UI value, not the display label).
    fn as_str(self) -> &'static str {
        match self {
            Self::Unclassified => "unclassified",
            Self::Gartley => "gartley",
            Self::Bat => "bat",
            Self::AltBat => "alt_bat",
            Self::Butterfly => "butterfly",
            Self::Crab => "crab",
            Self::DeepCrab => "deep_crab",
            Self::Shark => "shark",
        }
    }

    /// The reverse of [`Self::as_str`] — an id this crate never produced
    /// (a stale preset, a hand-edited value) falls back to `Unclassified`,
    /// the same "claims nothing" default every other unrecognised value in
    /// this file already falls back to.
    fn from_str(s: &str) -> Self {
        match s {
            "gartley" => Self::Gartley,
            "bat" => Self::Bat,
            "alt_bat" => Self::AltBat,
            "butterfly" => Self::Butterfly,
            "crab" => Self::Crab,
            "deep_crab" => Self::DeepCrab,
            "shark" => Self::Shark,
            _ => Self::Unclassified,
        }
    }

    /// This variant's own display label. `Unclassified` reuses
    /// `ConfigKey::LevelSetCustom` ("Custom") — the same generic word
    /// `NotePatternSubtypesList`'s own hint sentence already uses for it —
    /// rather than adding a near-duplicate harmonic-name key; the seven
    /// named patterns are proper nouns (`ConceptKey::HarmonicKind*`),
    /// carried in Latin spelling in every language column, matching how
    /// that same hint sentence already treats them.
    fn label(self, lang: Language) -> &'static str {
        match self {
            Self::Unclassified => ConfigKey::LevelSetCustom.get(lang),
            Self::Gartley => ConceptKey::HarmonicKindGartley.get(lang),
            Self::Bat => ConceptKey::HarmonicKindBat.get(lang),
            Self::AltBat => ConceptKey::HarmonicKindAltBat.get(lang),
            Self::Butterfly => ConceptKey::HarmonicKindButterfly.get(lang),
            Self::Crab => ConceptKey::HarmonicKindCrab.get(lang),
            Self::DeepCrab => ConceptKey::HarmonicKindDeepCrab.get(lang),
            Self::Shark => ConceptKey::HarmonicKindShark.get(lang),
        }
    }

    /// Every variant, as one `Select` option each — the row's own current
    /// value (`ConfigProperty::select`'s own `value` argument) is read
    /// separately, off the live `pattern_kind` field.
    fn select_options(lang: Language) -> Vec<SelectOption> {
        [
            Self::Unclassified, Self::Gartley, Self::Bat, Self::AltBat,
            Self::Butterfly, Self::Crab, Self::DeepCrab, Self::Shark,
        ]
            .into_iter()
            .map(|kind| SelectOption::new(kind.as_str(), kind.label(lang)))
            .collect()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct XabcdPattern {
    pub data: PrimitiveData,
    pub points: [(i64, f64); 5], // X, A, B, C, D
    #[serde(default = "default_true")]
    pub show_labels: bool,
    #[serde(default = "default_true")]
    pub show_ratios: bool,
    /// Label styling configuration
    #[serde(default)]
    pub label_style: LabelStyle,
    /// Show connecting lines between points
    #[serde(default = "default_true")]
    pub show_lines: bool,
    /// Which harmonic pattern this drawing claims to be — see
    /// [`XabcdHarmonicKind`]. `Unclassified` (default) claims nothing: no
    /// target-ratio table, no out-of-band marking.
    #[serde(default)]
    pub pattern_kind: XabcdHarmonicKind,
    /// Widening delta (`mli::ewa::harmonic::widen_band`) applied to every
    /// target leg's own band before matching — defaults to the
    /// vocabulary's own [`harmonic::DEFAULT_HARMONIC_TOLERANCE`], never a
    /// second tolerance constant of this crate's own.
    #[serde(default = "default_harmonic_tolerance")]
    pub harmonic_tolerance: f64,
}

impl XabcdPattern {
    pub fn new(points: [(i64, f64); 5], color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "xabcd_pattern".to_string(),
                display_name: "XABCD Pattern".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            points,
            show_labels: true,
            show_ratios: true,
            label_style: LabelStyle::default(),
            show_lines: true,
            pattern_kind: XabcdHarmonicKind::default(),
            harmonic_tolerance: harmonic::DEFAULT_HARMONIC_TOLERANCE,
        }
    }
}

/// `length(leg.leg) / length(leg.reference)` for this pattern's own points
/// — the SAME per-price-difference computation used for every on-chart
/// ratio label (both the unclassified fallback and the classified
/// per-target-leg loop in `render()`) and for the target-ratio table
/// (`target_ratio_table_property`) — one function, never a second copy of
/// the arithmetic. `None` when a named pivot cannot be resolved, or when
/// the reference leg has zero length (mirrors every other zero-length
/// guard in this file, e.g. `if xa > 0.0`).
fn measured_ratio(points: &[(i64, f64); 5], leg: &HarmonicLeg) -> Option<f64> {
    let idx = |name: &str| PIVOT_NAMES.iter().position(|&n| n == name);
    let (l0, l1) = (idx(leg.leg.0)?, idx(leg.leg.1)?);
    let (r0, r1) = (idx(leg.reference.0)?, idx(leg.reference.1)?);
    let leg_len = (points[l1].1 - points[l0].1).abs();
    let ref_len = (points[r1].1 - points[r0].1).abs();
    (ref_len > 0.0).then(|| leg_len / ref_len)
}

/// Which of this primitive's own already-drawn segments (the solid
/// X-A-B-C-D run, or the dashed X-B/A-C projections) shows `leg` — `None`
/// when no drawn segment corresponds (e.g. the Gartley/Bat/Butterfly/Crab
/// family's own AD completion leg, which this primitive never draws as a
/// direct line). Both out-of-band line recolouring and classified-label
/// placement in `render()` use this so a leg is only ever marked on a line
/// that is actually there.
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
/// `mli::ewa::harmonic::targets_for(kind)`: the leg, its (tolerance-widened)
/// target band, and the measured ratio ([`measured_ratio`] — the SAME
/// function `render()` uses for its own on-chart labels), toned by
/// [`harmonic::ratio_matches`] (never a local comparison). `None` for an
/// unclassified drawing — `targets_for` has no legs to tabulate.
fn target_ratio_table_property(points: &[(i64, f64); 5], kind: EwaPatternKind, tolerance: f64) -> Option<ConfigProperty> {
    let table = harmonic::targets_for(kind);
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

impl Primitive for XabcdPattern {
    fn type_id(&self) -> &'static str { "xabcd_pattern" }
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

        // Harmonic classification (owner ask): `target_table` is empty for
        // an unclassified drawing, by construction (`harmonic::targets_for`)
        // — every block below branches on that to leave an unclassified
        // XABCD rendering byte-identical to before this field existed.
        let target_table = harmonic::targets_for(self.pattern_kind.as_ewa_kind());
        // Every currently out-of-band target leg (tolerance-widened,
        // `ratio_matches` — the SAME rule `style_properties()`'s own table
        // reads) — empty for an unclassified drawing.
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

            // Draw X-B and A-C lines (dashed)
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
            // recolouring the offending segment): only for a classified
            // drawing, and only on a leg this primitive actually draws as a
            // direct line (`leg_line_span`) — an unclassified drawing has
            // `out_of_band` empty and marks nothing.
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

            if target_table.is_empty() {
                // Unclassified — byte-identical to this primitive's own
                // pre-classification behaviour (owner hard limit: nothing
                // already drawn changes for a drawing that stays
                // unclassified).
                // Points: X=0, A=1, B=2, C=3, D=4
                let xa = (self.points[1].1 - self.points[0].1).abs();
                let ab = (self.points[2].1 - self.points[1].1).abs();
                let bc = (self.points[3].1 - self.points[2].1).abs();
                let cd = (self.points[4].1 - self.points[3].1).abs();
                let ad = (self.points[4].1 - self.points[1].1).abs();

                // AB/XA on AB leg midpoint
                if xa > 0.0 {
                    draw_ratio_label(ctx, screen[1], screen[2], ab / xa, ratio_font_size, &self.label_style, ratio_color);
                }
                // BC/AB on BC leg midpoint
                if ab > 0.0 {
                    draw_ratio_label(ctx, screen[2], screen[3], bc / ab, ratio_font_size, &self.label_style, ratio_color);
                }
                // CD/BC on CD leg midpoint
                if bc > 0.0 {
                    draw_ratio_label(ctx, screen[3], screen[4], cd / bc, ratio_font_size, &self.label_style, ratio_color);
                }
                // AD/XA near D point (completion ratio)
                if xa > 0.0 {
                    let (dx, dy) = screen[4];
                    let label = format!("{:.3}", ad / xa);
                    draw_label_plate(ctx, dx, dy + self.label_style.offset_y + 12.0, &label, ratio_font_size, &self.label_style, ratio_color);
                }
            } else {
                // Classified — one label per target leg, using the SAME
                // `measured_ratio` the target-ratio table reads, marked
                // with `guard::VIOLATION_COLOR` when out of band.
                for entry in target_table {
                    let Some(ratio) = measured_ratio(&self.points, entry) else { continue };
                    let is_violating = out_of_band.contains(entry);
                    let this_label_color = if is_violating { guard::VIOLATION_COLOR } else { ratio_color };
                    if let Some((a, b)) = leg_line_span(entry.leg) {
                        draw_ratio_label(ctx, screen[a], screen[b], ratio, ratio_font_size, &self.label_style, this_label_color);
                    } else {
                        // No drawn segment for this leg (e.g. the Carney
                        // four-leg family's own AD completion leg) — same
                        // near-D placement the legacy completion ratio
                        // already used.
                        let (dx, dy) = screen[4];
                        let label = format!("{ratio:.3}");
                        draw_label_plate(ctx, dx, dy + self.label_style.offset_y + 12.0, &label, ratio_font_size, &self.label_style, this_label_color);
                    }
                }
            }
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Get bounding box from all points
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
        // `show_labels`/`show_lines`/`show_ratios` stay ungrouped (W2): the
        // approved pattern archetype's "ratios" subject tab names Subtype/
        // Line/Fill/TargetRatios/Scoring groups, none of which is this
        // primitive's own on-chart-toggle trio — they keep rendering in the
        // default Line run, exactly as before named groups existed.
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
            // `PropertyGroup::PointLabels` (W2) — the approved design's own
            // `PointLabelsGroupTitle`.
            ConfigProperty::label_font_size(self.label_style.font_size)
                .with_category(PropertyCategory::LabelsText)
                .with_order(13).in_group(PropertyGroup::PointLabels),
            ConfigProperty::label_color(self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke))
                .with_category(PropertyCategory::LabelsText)
                .with_order(14).in_group(PropertyGroup::PointLabels),
        ];
        props.extend(self.label_style.style_properties_grouped(&self.data.color.stroke, 30, PropertyGroup::PointLabels));
        // Identity + tolerance — declared in `PropertyGroup::TargetRatios`
        // beside the table they govern (owner ask: a field the analyst
        // cannot reach is a dead field), both written back through
        // `apply_style_property` by id like every other declared property.
        let lang = current_language();
        props.push(
            ConfigProperty::select(PATTERN_KIND_PROPERTY_ID, ConceptKey::PatternDropdownLabel.get(lang), self.pattern_kind.as_str(), XabcdHarmonicKind::select_options(lang))
                .with_category(PropertyCategory::AppearanceStats)
                .with_order(18)
                .in_group(PropertyGroup::TargetRatios),
        );
        props.push(
            ConfigProperty::number(HARMONIC_TOLERANCE_PROPERTY_ID, ConceptKey::ToleranceColumnHeader.get(lang), self.harmonic_tolerance, Some(MIN_HARMONIC_TOLERANCE), Some(0.2))
                .with_category(PropertyCategory::AppearanceStats)
                .with_order(19)
                .in_group(PropertyGroup::TargetRatios),
        );
        if let Some(table) = target_ratio_table_property(&self.points, self.pattern_kind.as_ewa_kind(), self.harmonic_tolerance) {
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
            PATTERN_KIND_PROPERTY_ID => {
                if let Some(s) = value.as_string() { self.pattern_kind = XabcdHarmonicKind::from_str(s); return true; }
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
        type_id: "xabcd_pattern", display_name: "XABCD Pattern", kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::MultiPointFixed(5), tooltip: "5-point harmonic pattern", icon: "xabcd_pattern", default_color: "#9C27B0",
        factory: |points, color| {
            let mut arr = [(0i64, 0.0); 5];
            for (i, &p) in points.iter().take(5).enumerate() { arr[i] = p; }
            Box::new(XabcdPattern::new(arr, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod harmonic_tests {
    use super::*;
    use super::super::super::config::PropertyType;

    /// A Gartley-shaped XABCD, by construction: AB/XA = 0.618 (exact),
    /// BC/AB = 0.5 (in [0.382, 0.886]), CD/BC ≈ 1.544 (in [1.13, 1.618]),
    /// AD/XA = 0.786 (exact) — every leg lands squarely inside its own
    /// Gartley target band (`mli::ewa::harmonic::GARTLEY`, read via
    /// `targets_for`, never re-typed here). Direction/sign is irrelevant —
    /// `measured_ratio` only ever reads absolute price differences.
    fn gartley_points() -> [(i64, f64); 5] {
        [(0, 0.0), (1_000, 100.0), (2_000, 38.2), (3_000, 69.1), (4_000, 21.4)]
    }

    #[test]
    fn gartley_shaped_drawing_reports_every_leg_inside() {
        let mut prim = XabcdPattern::new(gartley_points(), "#9C27B0");
        prim.pattern_kind = XabcdHarmonicKind::Gartley;

        let table = harmonic::targets_for(prim.pattern_kind.as_ewa_kind());
        assert_eq!(table.len(), 4, "Gartley's own table has 4 legs");
        for entry in table {
            let ratio = measured_ratio(&prim.points, entry).expect("every Gartley leg has a nonzero reference on this fixture");
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
        let mut prim = XabcdPattern::new(gartley_points(), "#9C27B0");
        prim.pattern_kind = XabcdHarmonicKind::Gartley;
        // Move B only: AB/XA goes from 0.618 (exact match) to 0.70 (clearly
        // outside the exact [0.613, 0.623] band); BC/AB moves from 0.5 to
        // ~0.559, still comfortably inside [0.382, 0.886] — CD and AD never
        // touch B at all, so this isolates exactly one broken leg.
        prim.points[2].1 = 30.0;

        let table = harmonic::targets_for(prim.pattern_kind.as_ewa_kind());
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
    fn unclassified_default_reports_no_targets_and_marks_nothing() {
        let prim = XabcdPattern::new(gartley_points(), "#9C27B0");
        assert_eq!(prim.pattern_kind, XabcdHarmonicKind::Unclassified);
        assert!(harmonic::targets_for(prim.pattern_kind.as_ewa_kind()).is_empty());
        assert!(target_ratio_table_property(&prim.points, prim.pattern_kind.as_ewa_kind(), prim.harmonic_tolerance).is_none());
    }

    #[test]
    fn tolerance_field_widens_the_band() {
        let ab_leg = harmonic::targets_for(EwaPatternKind::Gartley)
            .iter()
            .find(|entry| entry.leg == ("A", "B"))
            .expect("Gartley has an AB leg");
        // 0.630 is outside the default-tolerance band [0.608, 0.628]
        // (0.618 exact ±0.005 published, widened by the default
        // `DEFAULT_HARMONIC_TOLERANCE` delta of 0.005) but inside a band
        // widened by 0.02.
        let ratio = 0.630;
        let default_band = harmonic::widen_band(ab_leg.band, harmonic::DEFAULT_HARMONIC_TOLERANCE);
        let default_leg = HarmonicLeg { band: default_band, ..*ab_leg };
        assert!(!harmonic::ratio_matches(ratio, &default_leg), "0.630 should sit just outside the default-tolerance band {:?}", default_band);

        let widened_band = harmonic::widen_band(ab_leg.band, 0.02);
        let widened_leg = HarmonicLeg { band: widened_band, ..*ab_leg };
        assert!(harmonic::ratio_matches(ratio, &widened_leg), "0.630 should be inside the widened band {:?}", widened_band);
    }

    /// `select_options()` now names all 8 kinds (`Unclassified` + 7 named
    /// harmonics) — the A0 wiring for `AltBat`/`DeepCrab`
    /// (`docs/mlc/plans/autodetectors-a0-implementation-2026-09-24.md`,
    /// piece 1).
    #[test]
    fn select_options_now_returns_eight_kinds() {
        let options = XabcdHarmonicKind::select_options(Language::En);
        assert_eq!(options.len(), 8, "Unclassified + 7 named harmonics: {options:?}");
    }

    /// `as_str`/`from_str` round-trip for the 2 new ids — same contract
    /// every pre-existing named kind already carries.
    #[test]
    fn alt_bat_and_deep_crab_ids_round_trip() {
        assert_eq!(XabcdHarmonicKind::from_str("alt_bat"), XabcdHarmonicKind::AltBat);
        assert_eq!(XabcdHarmonicKind::AltBat.as_str(), "alt_bat");
        assert_eq!(XabcdHarmonicKind::from_str("deep_crab"), XabcdHarmonicKind::DeepCrab);
        assert_eq!(XabcdHarmonicKind::DeepCrab.as_str(), "deep_crab");
    }

    /// Alt Bat's own nominal ratios (`mli::ewa::harmonic`'s own fixture
    /// values: `AB/XA = 0.382`, `AD/XA = 1.13`), with `BC`/`CD` picked to
    /// land inside ALT_BAT's own `BC/AB` ([0.382, 0.886]) and `CD/BC`
    /// ([2.0, 3.618]) bands too — a hand-built point set landing on all 4
    /// legs at once should report every leg inside its own band.
    fn alt_bat_points() -> [(i64, f64); 5] {
        // XA = 100 (0 -> 100). AB/XA = 0.382 exact -> AB = 38.2 (100 -> 61.8).
        // AD/XA = 1.13 exact -> AD = 113, D = 100 - 113 = -13.
        // BC/AB = 0.8 (in-band) -> BC = 30.56, C = 61.8 + 30.56 = 92.36.
        // CD = |92.36 - (-13)| = 105.36, CD/BC = 105.36/30.56 ≈ 3.448 (in [2.0, 3.618]).
        [(0, 0.0), (1_000, 100.0), (2_000, 61.8), (3_000, 92.36), (4_000, -13.0)]
    }

    #[test]
    fn alt_bat_shaped_drawing_reports_every_leg_inside() {
        let mut prim = XabcdPattern::new(alt_bat_points(), "#9C27B0");
        prim.pattern_kind = XabcdHarmonicKind::AltBat;

        let table = harmonic::targets_for(prim.pattern_kind.as_ewa_kind());
        assert!(!table.is_empty(), "AltBat must have a real target-ratio table");
        for entry in table {
            let ratio = measured_ratio(&prim.points, entry).expect("every AltBat leg has a nonzero reference on this fixture");
            let widened = HarmonicLeg { band: harmonic::widen_band(entry.band, prim.harmonic_tolerance), ..*entry };
            assert!(
                harmonic::ratio_matches(ratio, &widened),
                "leg {:?} measured {ratio} should be inside its own band {:?}",
                entry.leg, widened.band
            );
        }
    }

    /// Deep Crab's own nominal ratios (`AB/XA = 0.886`, `AD/XA = 1.618`),
    /// with `BC`/`CD` picked to land inside DEEP_CRAB's own `BC/AB`
    /// ([0.382, 0.886]) and `CD/BC` ([2.24, 3.618]) bands too.
    fn deep_crab_points() -> [(i64, f64); 5] {
        // XA = 100. AB/XA = 0.886 exact -> AB = 88.6 (100 -> 11.4).
        // AD/XA = 1.618 exact -> AD = 161.8, D = 100 - 161.8 = -61.8.
        // BC/AB = 0.6 (in-band) -> BC = 53.16, C = 11.4 + 53.16 = 64.56.
        // CD = |64.56 - (-61.8)| = 126.36, CD/BC = 126.36/53.16 ≈ 2.377 (in [2.24, 3.618]).
        [(0, 0.0), (1_000, 100.0), (2_000, 11.4), (3_000, 64.56), (4_000, -61.8)]
    }

    #[test]
    fn deep_crab_shaped_drawing_reports_every_leg_inside() {
        let mut prim = XabcdPattern::new(deep_crab_points(), "#9C27B0");
        prim.pattern_kind = XabcdHarmonicKind::DeepCrab;

        let table = harmonic::targets_for(prim.pattern_kind.as_ewa_kind());
        assert!(!table.is_empty(), "DeepCrab must have a real target-ratio table");
        for entry in table {
            let ratio = measured_ratio(&prim.points, entry).expect("every DeepCrab leg has a nonzero reference on this fixture");
            let widened = HarmonicLeg { band: harmonic::widen_band(entry.band, prim.harmonic_tolerance), ..*entry };
            assert!(
                harmonic::ratio_matches(ratio, &widened),
                "leg {:?} measured {ratio} should be inside its own band {:?}",
                entry.leg, widened.band
            );
        }
    }

    #[test]
    fn table_actual_column_equals_the_measured_ratio_the_labels_use() {
        let mut prim = XabcdPattern::new(gartley_points(), "#9C27B0");
        prim.pattern_kind = XabcdHarmonicKind::Gartley;

        let table_property = target_ratio_table_property(&prim.points, prim.pattern_kind.as_ewa_kind(), prim.harmonic_tolerance)
            .expect("a classified drawing has a target-ratio table");
        let PropertyType::Table { rows, .. } = &table_property.prop_type else { panic!("must be PropertyType::Table") };

        let target_table = harmonic::targets_for(prim.pattern_kind.as_ewa_kind());
        assert_eq!(rows.len(), target_table.len());
        for (row, entry) in rows.iter().zip(target_table.iter()) {
            let TableCell::Toned(actual_text, _) = &row[2] else { panic!("Actual column must be a Toned cell") };
            let actual: f64 = actual_text.parse().expect("Actual cell must be a formatted number");
            // render()'s own on-chart label reads the SAME `measured_ratio`
            // call — this asserts the table's own "Actual" cell is that
            // exact number, not a second, independently-formatted copy.
            let expected = measured_ratio(&prim.points, entry).expect("nonzero reference on this fixture");
            assert!((actual - expected).abs() < 1e-3, "leg {:?}: table shows {actual}, measured_ratio gives {expected}", entry.leg);
        }
    }

    /// The reachability test the owner called for: `pattern_kind` is a
    /// dead field unless the SAME `apply_style_property` path every other
    /// declared property goes through can actually reach it, and the very
    /// next table build must reflect the new kind's own legs.
    #[test]
    fn applying_pattern_kind_through_apply_path_changes_the_table() {
        let mut prim = XabcdPattern::new(gartley_points(), "#9C27B0");
        assert_eq!(prim.pattern_kind, XabcdHarmonicKind::Unclassified);
        assert!(target_ratio_table_property(&prim.points, prim.pattern_kind.as_ewa_kind(), prim.harmonic_tolerance).is_none());

        let applied = prim.apply_style_property(PATTERN_KIND_PROPERTY_ID, &PropertyValue::String("gartley".to_string()));
        assert!(applied, "apply_style_property must recognise the pattern_kind id");
        assert_eq!(prim.pattern_kind, XabcdHarmonicKind::Gartley);

        let table_property = target_ratio_table_property(&prim.points, prim.pattern_kind.as_ewa_kind(), prim.harmonic_tolerance)
            .expect("classified after apply_style_property");
        let PropertyType::Table { rows, .. } = &table_property.prop_type else { panic!("must be PropertyType::Table") };
        let segments: Vec<String> = rows
            .iter()
            .map(|row| match &row[0] {
                TableCell::Text(s) => s.clone(),
                _ => panic!("Segment column must be Text"),
            })
            .collect();
        assert_eq!(segments, vec!["AB", "BC", "CD", "AD"], "table rows should now be Gartley's own legs");
    }

    #[test]
    fn applying_tolerance_through_apply_path_widens_the_table_bands() {
        let mut prim = XabcdPattern::new(gartley_points(), "#9C27B0");
        prim.pattern_kind = XabcdHarmonicKind::Gartley;

        let applied = prim.apply_style_property(HARMONIC_TOLERANCE_PROPERTY_ID, &PropertyValue::Number(0.05));
        assert!(applied, "apply_style_property must recognise the harmonic_tolerance id");
        assert!((prim.harmonic_tolerance - 0.05).abs() < 1e-12);

        let table_property = target_ratio_table_property(&prim.points, prim.pattern_kind.as_ewa_kind(), prim.harmonic_tolerance)
            .expect("classified drawing has a table");
        let PropertyType::Table { rows, .. } = &table_property.prop_type else { panic!("must be PropertyType::Table") };

        // AB's own published band (Gartley, exact 0.618) is [0.613, 0.623]
        // — widened by 0.05 it becomes [0.563, 0.673]. Asserts the table's
        // own Tolerance column reflects the WIDENED range, not the narrow
        // published one.
        let TableCell::Text(band_text) = &rows[0][1] else { panic!("Tolerance column must be Text") };
        assert_eq!(band_text, "0.563\u{2013}0.673");
    }
}
