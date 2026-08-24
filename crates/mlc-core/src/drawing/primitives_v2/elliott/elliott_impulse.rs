//! Elliott Impulse Wave - 5-wave motive pattern

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, Archetype, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle, CONTROL_POINT_HIT_RADIUS,
    render_primitive_text, draw_label_plate, hit_test_label_plates, wave_label_offset, stroke_leg_segments,
    config::{WaveDegree, NotationStyle, LabelStyle, ConfigProperty, PropertyValue, PropertyCategory, PropertyGroup, SelectOption},
};
use super::count::{WaveLink, WaveSlot, slot_span, segment_is_subdivided, visible_leg_segments};
use super::{guard, ratio, projection, wave_hit};

fn default_true() -> bool { true }
fn default_ratio_rib_style() -> LineStyle { LineStyle::Dotted }

/// Appended AFTER whatever bracket style the degree's own
/// [`WaveDegree::impulse_labels`] already produced for wave 5 — `(5)`
/// becomes `(5)ᵗ`, `[5]`-style degrees would become `[5]ᵗ`, and so on for
/// every degree, because it composes with the FINAL rendered string rather
/// than being special-cased per degree (see
/// [`ElliottImpulse::rendered_wave_labels`]).
///
/// NOT routed through `crate::i18n`: like the parentheses/brackets it sits
/// beside, this is Elliott notation punctuation, not prose — an analyst
/// reads "(5)ᵗ" the same way regardless of the UI's display language,
/// exactly as "(5)" itself is never translated. U+1D57 MODIFIER LETTER
/// SMALL T — a raised "t" for "truncated".
const TRUNCATED_FIFTH_MARK: char = '\u{1D57}';

/// Which motive shape this structure IS — the impulse-family counterpart of
/// [`super::elliott_correction::CorrectionType`]: a real, drawable subtype
/// selector on `ElliottImpulse`, the same "declared shape" role
/// `correction_type` already carries on `ElliottCorrection`.
///
/// `mli::ewa::grammar` has carried `LEADING_DIAGONAL_SLOTS`/
/// `ENDING_DIAGONAL_SLOTS` and
/// `EwaPatternKind::{LeadingDiagonalContracting, LeadingDiagonalExpanding,
/// EndingDiagonalContracting, EndingDiagonalExpanding}` for as long as the
/// plain impulse rule table existed — this crate simply had no field to say
/// a drawn wave IS one. Only the two POSITIONS a diagonal is ever legal in
/// (leading = wave 1/A, ending = wave 5/C — `mli::ewa::grammar::
/// allowed_in_position`) are represented, not the separate contracting/
/// expanding split the catalogue also carries for each: `ElliottTriangle`
/// sets the same precedent (`elliott::count::parent_pattern` maps it
/// unconditionally onto `EwaPatternKind::ContractingTriangle`, with no
/// symmetrical/expanding selector of its own) — contracting is the
/// classic, common shape, expanding the catalogue's own guidelines mark
/// `"rare"`/`"very_rare"`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
pub enum ImpulseKind {
    /// A plain, five-wave motive impulse — the default, and the ONLY kind
    /// every impulse drawn before this field existed carries
    /// (`#[serde(default)]`), so nothing about an existing drawing changes.
    #[default]
    Plain,
    /// A leading diagonal — legal only in wave 1 of an impulse or wave A of
    /// a zigzag (`mli::ewa::grammar::allowed_in_position`); wave 4
    /// overlapping wave 1's own territory is part of the shape, not a
    /// violation of it (`elliott::guard::impulse_violations`).
    LeadingDiagonal,
    /// An ending diagonal — legal only in wave 5 of an impulse or wave C of
    /// a corrective structure; same wave-4/wave-1 overlap allowance as
    /// [`LeadingDiagonal`](Self::LeadingDiagonal).
    EndingDiagonal,
}

impl ImpulseKind {
    /// The same fixed token [`ElliottImpulse::apply_style_property`]'s own
    /// `"impulse_kind"` id reads and writes, and the SAME token
    /// `elliott::count::kind_id_of`/`elliott::count::parent_pattern` use
    /// after the colon in a kind id (e.g. `"elliott_impulse:leading_diagonal"`)
    /// — one mapping, not a second copy of it per caller, the same
    /// discipline [`CorrectionType::as_str`](super::elliott_correction::CorrectionType::as_str)
    /// already documents for its own family.
    pub fn as_str(&self) -> &'static str {
        match self {
            ImpulseKind::Plain => "plain",
            ImpulseKind::LeadingDiagonal => "leading_diagonal",
            ImpulseKind::EndingDiagonal => "ending_diagonal",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElliottImpulse {
    pub data: PrimitiveData,
    /// 6 points: start (0), then waves 1-5. Each is (timestamp_ms, price).
    pub points: [(i64, f64); 6],
    #[serde(default = "default_true")]
    pub show_labels: bool,
    /// Which motive shape this structure IS — plain / leading diagonal /
    /// ending diagonal (see [`ImpulseKind`]'s own doc). `Plain` (the
    /// default, `#[serde(default)]`) is EXACTLY how every impulse drawn
    /// before this field existed behaved — nothing about an existing
    /// drawing changes. Sits next to [`show_labels`](Self::show_labels),
    /// the same position [`super::elliott_correction::ElliottCorrection::correction_type`]
    /// occupies relative to ITS OWN `show_labels`.
    #[serde(default)]
    pub impulse_kind: ImpulseKind,
    #[serde(default)]
    pub degree: WaveDegree,
    /// The numeral family this structure's own wave-count labels draw in —
    /// see [`NotationStyle`]'s own doc. `FollowDegree` (the default) reads
    /// `degree`'s own table exactly as this primitive always has; the other
    /// three variants override it with one fixed family regardless of
    /// `degree`.
    ///
    /// Elliott-Impulse ONLY (owner correction 2026-08-17) — do NOT add this
    /// field to `ElliottCorrection`/`ElliottTriangle`/`ElliottDoubleCombo`/
    /// `ElliottTripleCombo`. This isn't a layout gap those four happen to
    /// share with impulse; it's a notation-correctness line. An impulse's
    /// five waves ARE numbers — arabic/roman/circled is a real, faithful
    /// choice of how to WRITE that number. A correction's A-B-C, a
    /// triangle's A-E, a combo's W-X-Y-Z are not numbers spelled a
    /// different way — they are the Elliott vocabulary for what those
    /// structures ARE. Offering "arabic" there and mapping by ordinal
    /// position (an earlier draft of this feature did exactly that) turns
    /// a zigzag's own A-B-C into "1-2-3", which reads as a THREE-WAVE
    /// segment of a motive impulse — the wrong notation for a correction,
    /// not a stylistic one. The four letter-labelled types keep drawing
    /// straight from `degree.{corrective,triangle,combo}_labels()`, with
    /// no notation field and no pills row.
    #[serde(default)]
    pub notation_style: NotationStyle,
    #[serde(default)]
    pub label_style: LabelStyle,
    #[serde(default = "default_true")]
    pub show_lines: bool,
    /// Link to the parent Elliott wave this primitive is nested inside, if
    /// any — see [`super::count::WaveLink`]. `None` (the default) for a
    /// standalone impulse, exactly as every impulse behaved before this
    /// field existed.
    #[serde(default)]
    pub link: Option<WaveLink>,
    /// The predecessor Elliott primitive's own id this impulse continues —
    /// succession, not subdivision (see `super::count`'s own module doc).
    /// `None` (the default) for an impulse that opens a count rather than
    /// continuing one.
    #[serde(default)]
    pub follows: Option<u64>,
    /// Show the dotted wave-ratio annotations (wave 2 vs wave 1, wave 4 vs
    /// wave 3 retracements; wave 3 vs wave 1, wave 5 vs wave 3 extensions —
    /// see `super::ratio`). `false` (the default) — off until asked for, so
    /// every drawing saved before this field existed loads and renders
    /// unchanged.
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
    /// Show the fibonacci-EXTENSION projection ticks for a wave that is
    /// still being placed (wave 3 off wave 1, wave 5 off wave 1 and off the
    /// 1-3 span — see `super::projection`). `false` (the default) — off
    /// until asked for, so every drawing saved before this field existed
    /// loads and renders unchanged.
    #[serde(default)]
    pub show_projections: bool,
    /// Declares THIS impulse's own fifth wave deliberately truncated — a
    /// real Elliott shape (`EwaPatternKind::TruncatedImpulse`, whose fifth
    /// legitimately fails to make new ground past wave 3) rather than a
    /// drawing mistake (owner ruling 2026-08-15: a fifth that fails to
    /// exceed wave 3 is a rare, real shape, not automatically an error).
    /// `false` (the default) — an existing drawing loads with wave 5
    /// checked for PROGRESSION, so every drawing saved before this field
    /// existed keeps being checked exactly as before: the analyst opts INTO
    /// calling his own fifth truncated, not the other way around.
    ///
    /// It does NOT silence a rule — owner correction, same day: "если
    /// поставил укороченную, то она уже должна давать ошибку если она
    /// выходит за пределы 3, раз уж её выбрал как укороченную". The flag is
    /// a CLAIM about the shape, and `guard::impulse_violations` judges the
    /// measured shape against whichever half of the claim is active:
    /// `false` reports `"wave5_beyond_wave3"` when wave 5 FAILS to exceed
    /// wave 3 (the progression check); `true` reports
    /// `"wave5_not_truncated"` when wave 5 DOES exceed wave 3 (the analyst's
    /// own claim contradicted by the measured shape). Exactly one of the
    /// two can ever be active for a given structure — see that function's
    /// own doc for why both reuse the identical `beyond` call, one negated.
    /// Every other hard rule (wave 2/4 overlap, wave 3 progression, wave 3
    /// not shortest, ...) keeps checking exactly as before regardless of
    /// this flag (see `guard`'s own `wave5_truncation_tests` module, the
    /// test asserting a structure that ALSO overlaps wave 1 still reports
    /// that violation with this flag on).
    ///
    /// Deliberately per-structure, not an `EwaEngineConfig` toggle —
    /// truncation is a property of ONE fifth wave, and a process-global
    /// switch would apply the same claim to every count on the chart
    /// (owner: "по дефолту выключены, но кликая по конкретной 5 и
    /// настройкам там просто тугл — усечённая, и оно снимает ошибку").
    #[serde(default)]
    pub truncated_fifth: bool,
    /// Per-wave custom label OVERRIDE — the design's own "Custom Labels"
    /// table (`wave_tabs()`'s own `CustomLabelsGroupTitle`/`OverrideHint`
    /// group, `mlc-ui-lab`'s `spec.rs`). One optional string per point
    /// index (`points[i]` <-> `custom_wave_labels[i]`); `Some` REPLACES
    /// whatever [`Self::wave_labels`] would have produced for that index,
    /// `None` (every slot, the default — `#[serde(default)]` so a drawing
    /// saved before this field existed loads with an empty `Vec`, which
    /// reads the same as "every slot `None`") changes nothing. Composes
    /// through the SAME single production point the notation style and the
    /// truncation mark already go through — [`Self::rendered_wave_labels`],
    /// below, applies it LAST, so an explicit override wins over both.
    #[serde(default)]
    pub custom_wave_labels: Vec<Option<String>>,
    /// How many of `points` are REAL — i.e. correspond to an actually-drawn
    /// pivot — as opposed to padding introduced when a `follows`-linked
    /// chain is promoted into a coarser structure before every one of its
    /// slots has been drawn (`DrawingManager::promote_wave_chain`'s own
    /// `pad_points_to` repeats the chain's own last real point to fill out
    /// this type's fixed 6-point count, so the parent stays a well-formed,
    /// draggable shape instead of collapsing every trailing point onto the
    /// origin). `None` (the default) means every point is real — exactly
    /// how this primitive behaved before this field existed, so every
    /// drawing saved before this field existed loads and renders unchanged.
    /// `render` draws the connecting line and wave labels ONLY for the
    /// first `realized_points` points (see `Self::realized_len`); a later
    /// drag past the realized prefix (`move_control_point`/`set_points`)
    /// raises it back toward the full point count (`Self::grow_realized`) —
    /// never lowers it.
    #[serde(default)]
    pub realized_points: Option<usize>,
    /// The extra violations that depend on this impulse's own siblings —
    /// see [`super::super::Primitive::context_violations`]'s own doc. Cache
    /// only, filled by the collection's own owner — skipped on
    /// (de)serialization so it never becomes a second source of truth on
    /// disk.
    #[serde(skip)]
    pub context_violations: Vec<guard::Violation>,
    /// The slots currently occupied by a child subdivision nested inside
    /// THIS impulse — see
    /// [`super::super::Primitive::subdivided_slots`]'s own doc. Cache only,
    /// filled by the collection's own owner exactly like
    /// [`context_violations`](Self::context_violations) — skipped on
    /// (de)serialization so it never becomes a second source of truth on
    /// disk.
    #[serde(skip)]
    pub subdivided_slots: Vec<WaveSlot>,
    /// This impulse's own point indices where a nested child's own label
    /// lands on the same pivot — see
    /// [`super::super::Primitive::label_collisions`]'s own doc. Cache only,
    /// filled by the collection's own owner exactly like
    /// [`subdivided_slots`](Self::subdivided_slots) — skipped on
    /// (de)serialization so it never becomes a second source of truth on
    /// disk.
    #[serde(skip)]
    pub label_collisions: Vec<usize>,
}

impl ElliottImpulse {
    pub fn new(points: [(i64, f64); 6], color: &str) -> Self {
        // Ratio-rib defaults AND the projection toggle's own starting value
        // come from the EWA engine's own configurable defaults (owner ask
        // 2026-08-14) — read ONCE, here, at construction, off the SAME
        // config read (one RwLock read, not two); never retro-applied to a
        // primitive that already exists. `EwaEngineConfig::default()`'s own
        // fields reproduce exactly `None`/`None`/`LineStyle::Dotted`/
        // `false`, so an untouched engine config changes nothing about a
        // freshly drawn impulse (see `state::ewa_engine::RatioRibDefaults`'s
        // own doc and `EwaEngineConfig::show_projections`'s own doc).
        let ewa_cfg = crate::state::ewa_engine::ewa_engine_config();
        let ratio_rib_defaults = ewa_cfg.ratio_rib_defaults;
        Self {
            data: PrimitiveData {
                type_id: "elliott_impulse".to_string(),
                display_name: "Elliott Impulse Wave".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            points,
            show_labels: true,
            impulse_kind: ImpulseKind::Plain,
            degree: WaveDegree::Intermediate,
            notation_style: NotationStyle::default(),
            label_style: LabelStyle::default(),
            show_lines: true,
            link: None,
            follows: None,
            show_ratios: false,
            ratio_rib_color: ratio_rib_defaults.color,
            ratio_rib_width: ratio_rib_defaults.width,
            ratio_rib_style: ratio_rib_defaults.style,
            show_projections: ewa_cfg.show_projections,
            // No `EwaEngineConfig` counterpart (owner ask 2026-08-15,
            // rejected — see the field's own doc): every freshly drawn
            // impulse starts with the rule active, the same posture a
            // pre-existing drawing loads into.
            truncated_fifth: false,
            custom_wave_labels: Vec::new(),
            realized_points: None,
            context_violations: Vec::new(),
            subdivided_slots: Vec::new(),
            label_collisions: Vec::new(),
        }
    }

    /// This structure's own five wave-count labels, in `notation_style`'s
    /// chosen family — `FollowDegree` (the default) reads `degree`'s own
    /// table exactly as before this field existed; the other three
    /// variants override it with one fixed family, independent of degree
    /// (see [`NotationStyle::positional_table`]'s own doc). The ONE place
    /// degree/notation is resolved for this type — `rendered_wave_labels`
    /// (below) is the ONE place the truncation mark composes on top of it.
    pub fn wave_labels(&self) -> [&'static str; 6] {
        let impulse = self.notation_style.positional_table().unwrap_or_else(|| self.degree.impulse_labels());
        ["", impulse[0], impulse[1], impulse[2], impulse[3], impulse[4]]
    }

    /// The notation the Notation pills row DECLARES as current — never
    /// used by `wave_labels()` (owner correction 2026-08-17: resolves
    /// `FollowDegree`'s effective family for DISPLAY only). An explicit
    /// choice (`Arabic`/`Roman`/`Circled`) passes straight through.
    /// `FollowDegree` resolves to whichever fixed family `degree`'s own
    /// table currently belongs to, decoration stripped
    /// ([`NotationStyle::family_of`]) — e.g. a `Primary`-degree impulse
    /// (circled table) declares `Circled`, a `Cycle`-degree impulse (bare
    /// roman) declares `Roman`, so a freshly placed structure — every
    /// structure, since `FollowDegree` is the default — lights the pill
    /// matching what is actually drawn instead of showing no selection.
    /// Every one of `WaveDegree`'s 15 degrees resolves this way (verified
    /// exhaustively by `config::notation_style_tests
    /// ::every_impulse_degree_resolves_to_a_family`); the stored-value
    /// fallback exists only for a table `family_of` genuinely can't place,
    /// which does not happen for `impulse_labels()` today.
    fn declared_notation_style(&self) -> NotationStyle {
        match self.notation_style {
            NotationStyle::FollowDegree => NotationStyle::family_of(&self.degree.impulse_labels()).unwrap_or(self.notation_style),
            other => other,
        }
    }

    /// The labels actually drawn and hit-tested — [`wave_labels`](Self::wave_labels)'s
    /// own degree-notation strings, with [`TRUNCATED_FIFTH_MARK`] appended
    /// to wave 5's own label (index 5, the last slot) when
    /// [`truncated_fifth`](Self::truncated_fifth) is set. This is the ONE
    /// place the mark is applied — `render`, `hit_test`, `wave_at` and
    /// `point_labels` all read THIS method, never `wave_labels()` directly,
    /// so the mark can never appear on the drawn plate but not the hit
    /// area (or vice versa), and never needs a second per-degree branch.
    ///
    /// `truncated_fifth == false` returns exactly `wave_labels()`'s own
    /// strings, byte for byte (`plate`/offset/collision handling/colour are
    /// untouched either way) — the flag being off changes nothing about
    /// what this method produces or what any of its callers do with it.
    fn rendered_wave_labels(&self) -> [String; 6] {
        let mut labels = self.wave_labels().map(|s| s.to_string());
        if self.truncated_fifth {
            labels[5].push(TRUNCATED_FIFTH_MARK);
        }
        super::labels::apply_overrides(&mut labels, &self.custom_wave_labels);
        labels
    }

    /// How many of [`points`](Self::points) are real right now — see
    /// [`realized_points`](Self::realized_points)'s own field doc. Never
    /// exceeds `points.len()`.
    fn realized_len(&self) -> usize {
        self.realized_points.unwrap_or(self.points.len()).min(self.points.len())
    }

    /// Raise `realized_points` to at least `at_least` (clamped to the point
    /// count) — never lowers it. A no-op when `realized_points` is already
    /// `None`: every point is already real, which is already the maximum
    /// this could ever raise it to.
    fn grow_realized(&mut self, at_least: usize) {
        if let Some(n) = self.realized_points {
            if at_least > n {
                self.realized_points = Some(at_least.min(self.points.len()));
            }
        }
    }
}

impl Primitive for ElliottImpulse {
    fn type_id(&self) -> &'static str { "elliott_impulse" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Pattern }
    // Elliott is `PrimitiveKind::Pattern`, shared with the harmonic
    // patterns — but is `Wave`-shaped, not `Pattern`-shaped, so the default
    // `Primitive::archetype` (kind-derived) is wrong here.
    fn archetype(&self) -> Archetype { Archetype::Wave }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::MultiPointFixed(6) }
    fn wave_link(&self) -> Option<WaveLink> { self.link }
    fn set_wave_link(&mut self, link: Option<WaveLink>) { self.link = link; }
    fn wave_degree(&self) -> Option<WaveDegree> { Some(self.degree) }
    fn set_wave_degree(&mut self, degree: WaveDegree) { self.degree = degree; }
    fn wave_slots(&self) -> &'static [WaveSlot] {
        &[WaveSlot::Impulse1, WaveSlot::Impulse2, WaveSlot::Impulse3, WaveSlot::Impulse4, WaveSlot::Impulse5]
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
    fn truncated_fifth(&self) -> bool { self.truncated_fifth }
    fn impulse_kind(&self) -> Option<&'static str> {
        match self.impulse_kind {
            ImpulseKind::Plain => None,
            other => Some(other.as_str()),
        }
    }
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
        for (i, &p) in pts.iter().take(6).enumerate() { self.points[i] = p; }
        self.grow_realized(pts.len().min(6));
    }
    fn point_labels(&self) -> Option<Vec<String>> {
        Some(self.rendered_wave_labels().to_vec())
    }
    fn translate(&mut self, td: i64, pd: f64) { for p in &mut self.points { p.0 += td; p.1 += pd; } }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Index(i) if (i as usize) < 6 => {
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
        for i in 0..5 {
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
        if self.show_ratios && ratio::hit_test_ratio_ribs(sx, sy, &screen, &ratio::IMPULSE_RATIO_RIBS) {
            return HitTestResult::Body;
        }
        // A wave label's own plate is clickable too (owner ask 2026-08-13),
        // gated on `show_labels` exactly like the render side.
        let hit_labels = self.rendered_wave_labels();
        let hit_label_refs: [&str; 6] = std::array::from_fn(|i| hit_labels[i].as_str());
        if self.show_labels
            && hit_test_label_plates(sx, sy, &screen, &hit_label_refs, self.realized_len(), &self.label_style, &self.label_collisions)
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
        let labels = self.rendered_wave_labels();
        let label_refs: [&str; 6] = std::array::from_fn(|i| labels[i].as_str());
        wave_hit::wave_slot_at(
            sx, sy, self.type_id(), &screen, self.realized_len(), self.wave_slots(),
            &label_refs, &self.label_style, &self.label_collisions, self.show_labels, self.show_ratios, &ratio::IMPULSE_RATIO_RIBS,
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

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
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
            // skipping existed for a plain (non-subdivided) impulse.
            let segments = visible_leg_segments(self.type_id(), realized, &self.subdivided_slots);
            stroke_leg_segments(ctx, &screen, &segments, dpr);
        }

        ctx.set_line_dash(&[]);

        if self.show_lines && !violations.is_empty() {
            ctx.set_stroke_color(guard::VIOLATION_COLOR);
            ctx.set_stroke_width(self.data.width);
            for v in &violations {
                // A violation naming no wave (`None`) is about the whole
                // structure, not one leg — there is no segment to recolor.
                let Some(wave) = v.wave else { continue };
                // A subdivided wave's own leg is not drawn above — its
                // violation is still marked on its LABEL plate below, but
                // there is no leg left here to recolor.
                if self.subdivided_slots.contains(&wave) {
                    continue;
                }
                if let Some((a, b)) = slot_span(self.type_id(), wave) {
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
            let labels = self.rendered_wave_labels();
            let label_color = self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke);
            ctx.set_fill_color(label_color);
            ctx.set_font(&self.label_style.font_string());
            ctx.set_text_align(crate::render::TextAlign::Center);
            ctx.set_text_baseline(crate::render::TextBaseline::Middle);

            for (i, label) in labels.iter().enumerate() {
                if i >= realized || label.is_empty() { continue; }
                let (x, y) = screen[i];
                let collides = self.label_collisions.contains(&i);
                let (dx, dy) = wave_label_offset(&screen, i, &self.label_style, collides);
                // `None` names no wave, so it can never match this label's own slot.
                let is_violating = violations
                    .iter()
                    .any(|v| v.wave.is_some_and(|w| slot_span(self.type_id(), w).map(|(_, e)| e) == Some(i)));
                let this_label_color = if is_violating { guard::VIOLATION_COLOR } else { label_color };
                draw_label_plate(ctx, x + dx, y + dy, label, self.label_style.font_size, &self.label_style, this_label_color);
            }
        }

        if self.show_ratios {
            ratio::draw_ratio_annotations(
                ctx, &self.points, realized, &ratio::IMPULSE_RATIO_LEGS, &ratio::IMPULSE_RATIO_RIBS,
                &self.data.color.stroke, self.data.width, &self.label_style,
                self.ratio_rib_color.as_deref(), self.ratio_rib_width, self.ratio_rib_style,
            );
        }

        // A SELECTED impulse shows its projections on demand; an UNFINISHED
        // one (realized < 6) shows them regardless of selection — the
        // in-progress preview primitive is always rendered with
        // `is_selected == false` (`render_chart.rs`'s two preview call
        // sites), so gating on `is_selected` alone would hide the ticks for
        // exactly the case the feature exists for (owner ask 2026-08-14:
        // see it WHILE placing the wave). Gating on `realized < 6` alone
        // would instead hide them once a FINISHED structure is selected to
        // inspect. The OR covers both without special-casing the preview.
        if self.show_projections && (is_selected || realized < 6) {
            projection::draw_projection_annotations(
                ctx, &self.points, realized, &projection::IMPULSE_PROJECTION_LEGS,
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
            // `show_labels`/`show_lines`/`show_ratios` stay ungrouped (W2):
            // the approved wave archetype's own Line group
            // (`line_group(None)`) is just colour/width/style — the bespoke
            // base-stroke row this primitive already falls back to (no
            // `LevelsBase` ladder) — so these three toggles keep rendering
            // in that same default run, exactly as before named groups
            // existed.
            ConfigProperty::show_labels(self.show_labels).with_category(PropertyCategory::AppearanceLine).with_order(10),
            ConfigProperty::show_lines(self.show_lines).with_category(PropertyCategory::AppearanceLine).with_order(11),
            ConfigProperty::show_ratios(self.show_ratios).with_category(PropertyCategory::AppearanceLine).with_order(12),
            // The ratio ribs' own style — next to `show_ratios`, same
            // ungrouped run (see `super::ratio`'s own module doc).
            ConfigProperty::ratio_rib_color(&rib_color_default).with_category(PropertyCategory::AppearanceLine).with_order(13),
            ConfigProperty::ratio_rib_width(rib_width_default).with_category(PropertyCategory::AppearanceLine).with_order(14),
            ConfigProperty::ratio_rib_style(self.ratio_rib_style.as_str()).with_category(PropertyCategory::AppearanceLine).with_order(15),
            // The fibonacci-EXTENSION projection toggle — immediately after
            // the ratio-rib block it shares its own style with (see
            // `elliott::projection`'s module doc).
            ConfigProperty::show_projections(self.show_projections).with_category(PropertyCategory::AppearanceLine).with_order(16),
            // The per-structure truncated-fifth escape hatch (owner ruling
            // 2026-08-15) — next to `show_projections`, same ungrouped run,
            // same "falls out of `style_properties()` for free" surface
            // (see `truncated_fifth`'s own field doc).
            ConfigProperty::truncated_fifth(self.truncated_fifth).with_category(PropertyCategory::AppearanceLine).with_order(17),
            // `PropertyGroup::Degree` (W2) — the approved design's own
            // `wave_tabs()` "Degree" group, pulled out of the default Line
            // run into its own header.
            ConfigProperty::wave_degree(self.degree.as_str(), degree_options).with_category(PropertyCategory::AppearanceLine).with_order(18).in_group(PropertyGroup::Degree),
            // `PropertyGroup::Notation` (W4) — the approved design's own
            // `wave_tabs()` "Notation" group, right after Degree, matching
            // the design's own reading order (Line → Degree → Notation →
            // fill for this type — the correction/pattern-type step
            // `elliott_correction.rs` has is not part of this type).
            ConfigProperty::notation_style_pills(self.declared_notation_style().as_str()).with_category(PropertyCategory::AppearanceLine).with_order(19).in_group(PropertyGroup::Notation),
            // `PropertyGroup::ImpulseKind` (the diagonal arc) — right after
            // Notation, matching where `ElliottCorrection::style_properties`
            // places its own `correction_type` dropdown relative to Degree:
            // the shape-defining selector for the ONE Elliott type that
            // carries it. See `ImpulseKind`'s own doc.
            ConfigProperty::impulse_kind(self.impulse_kind.as_str()).with_category(PropertyCategory::AppearanceLine).with_order(20).in_group(PropertyGroup::ImpulseKind),
            // `PropertyGroup::WaveLabels` (W2) — the approved design's own
            // `WaveLabelsGroupTitle`.
            ConfigProperty::label_font_size(self.label_style.font_size).with_category(PropertyCategory::LabelsText).with_order(21).in_group(PropertyGroup::WaveLabels),
            ConfigProperty::label_color(self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke)).with_category(PropertyCategory::LabelsText).with_order(22).in_group(PropertyGroup::WaveLabels),
            // `PropertyGroup::CustomWaveLabels` (W4) — the approved design's
            // own trailing "Custom Labels" table, seeded with what every
            // wave ACTUALLY renders right now (`rendered_wave_labels`, the
            // same production point notation/truncation already go
            // through) so opening this row never shows a stale value.
            super::labels::wave_label_override_property(&self.rendered_wave_labels()),
        ];
        props.extend(self.label_style.style_properties_grouped(&self.data.color.stroke, 30, PropertyGroup::WaveLabels));
        props
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        if let Some(applied) = super::labels::apply_wave_label_override(id, value, &mut self.custom_wave_labels, self.points.len()) {
            return applied;
        }
        match id {
            "show_labels" => { if let Some(v) = value.as_bool() { self.show_labels = v; return true; } }
            "show_lines" => { if let Some(v) = value.as_bool() { self.show_lines = v; return true; } }
            "show_ratios" => { if let Some(v) = value.as_bool() { self.show_ratios = v; return true; } }
            "ratio_rib_color" => { if let Some(c) = value.as_color() { self.ratio_rib_color = Some(c.to_string()); return true; } }
            "ratio_rib_width" => { if let Some(v) = value.as_number() { self.ratio_rib_width = Some(v); return true; } }
            "ratio_rib_style" => { if let Some(s) = value.as_string() { self.ratio_rib_style = LineStyle::from_str(s); return true; } }
            "show_projections" => { if let Some(v) = value.as_bool() { self.show_projections = v; return true; } }
            "truncated_fifth" => { if let Some(v) = value.as_bool() { self.truncated_fifth = v; return true; } }
            "impulse_kind" => {
                if let Some(s) = value.as_string() {
                    self.impulse_kind = match s {
                        "leading_diagonal" => ImpulseKind::LeadingDiagonal,
                        "ending_diagonal" => ImpulseKind::EndingDiagonal,
                        _ => ImpulseKind::Plain,
                    };
                    return true;
                }
            }
            "degree" => { if let Some(s) = value.as_string() { if let Some(d) = WaveDegree::from_str(s) { self.degree = d; return true; } } }
            "notation_style" => { if let Some(s) = value.as_string() { self.notation_style = NotationStyle::from_str(s); return true; } }
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
        type_id: "elliott_impulse", display_name: "Elliott Impulse Wave", kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::MultiPointFixed(6), tooltip: "5-wave impulse pattern", icon: "elliott_impulse", default_color: "#2196F3",
        factory: |points, color| {
            let mut arr = [(0i64, 0.0f64); 6];
            for (i, &p) in points.iter().take(6).enumerate() { arr[i] = p; }
            Box::new(ElliottImpulse::new(arr, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod subdivision_overlay_tests {
    use super::*;
    use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};

    fn sample_impulse() -> ElliottImpulse {
        let points: [(i64, f64); 6] = [
            (0, 100.0),
            (1_000, 110.0),
            (2_000, 105.0),
            (3_000, 130.0),
            (4_000, 120.0),
            (5_000, 140.0),
        ];
        ElliottImpulse::new(points, "#2196F3")
    }

    /// A fixture wide/tall enough that a leg's own midpoint sits well clear
    /// of both its endpoints' own control-point hit radius and its wave
    /// labels' own plates — the same `Bar`/`Viewport`/`PriceScale` triplet
    /// `hit_test` itself converts through, so the screen points this module
    /// computes below are exactly what `hit_test` sees, not a guess at them.
    fn fixture() -> (Vec<Bar>, Viewport, PriceScale) {
        let bars: Vec<Bar> = (0..6).map(|i| Bar::new(i, 100.0, 102.0, 98.0, 100.0)).collect();
        let viewport = Viewport {
            view_start: 0.0,
            bar_spacing: 100.0,
            bar_width_ratio: 0.8,
            chart_width: 1200.0,
            chart_height: 600.0,
            bar_count: bars.len(),
        };
        let price_scale = PriceScale { price_min: 50.0, price_max: 150.0, ..Default::default() };
        (bars, viewport, price_scale)
    }

    fn screen_point(pt: (i64, f64), bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> (f64, f64) {
        let b = timestamp_ms_to_bar_f64(bars, pt.0);
        (vp.bar_to_x_f64(b), vp.price_to_y(pt.1, ps.price_min, ps.price_max))
    }

    /// Deliverable 2/3 (owner report 2026-08-13): once wave 4 carries a
    /// child subdivision, the segment list `render` would stroke excludes
    /// wave 4's own span `(3, 4)` and still includes every other leg.
    #[test]
    fn subdivided_wave4_excludes_its_own_span_from_render_segments() {
        let mut impulse = sample_impulse();
        assert!(visible_leg_segments(impulse.type_id(), impulse.realized_point_count(), impulse.subdivided_slots())
            .contains(&(3, 4)), "an un-subdivided impulse draws its own wave-4 leg");

        impulse.set_subdivided_slots(vec![WaveSlot::Impulse4]);
        let segments =
            visible_leg_segments(impulse.type_id(), impulse.realized_point_count(), impulse.subdivided_slots());
        assert_eq!(
            segments,
            vec![(0, 1), (1, 2), (2, 3), (4, 5)],
            "wave 4's own span (3, 4) is replaced by its child, every other leg is unaffected"
        );
    }

    #[test]
    fn subdivided_wave4_hit_test_misses_its_own_leg_while_wave3_still_hits() {
        let (bars, viewport, price_scale) = fixture();
        let mut impulse = sample_impulse();
        impulse.set_subdivided_slots(vec![WaveSlot::Impulse4]);

        let points: [(i64, f64); 6] = [
            (0, 100.0), (1_000, 110.0), (2_000, 105.0), (3_000, 130.0), (4_000, 120.0), (5_000, 140.0),
        ];
        let p3 = screen_point(points[3], &bars, &viewport, &price_scale);
        let p4 = screen_point(points[4], &bars, &viewport, &price_scale);
        let p2 = screen_point(points[2], &bars, &viewport, &price_scale);
        let wave4_mid = ((p3.0 + p4.0) / 2.0, (p3.1 + p4.1) / 2.0);
        let wave3_mid = ((p2.0 + p3.0) / 2.0, (p2.1 + p3.1) / 2.0);

        assert!(
            matches!(impulse.hit_test(wave4_mid.0, wave4_mid.1, &bars, &viewport, &price_scale), HitTestResult::Miss),
            "wave 4's own leg is no longer drawn once subdivided, so a click on its old midpoint must miss"
        );
        assert!(
            matches!(impulse.hit_test(wave3_mid.0, wave3_mid.1, &bars, &viewport, &price_scale), HitTestResult::Body),
            "wave 3 was never subdivided — its leg still hits"
        );
    }

    #[test]
    fn subdivided_wave4_label_still_answers_wave_at() {
        let (bars, viewport, price_scale) = fixture();
        let mut impulse = sample_impulse();
        impulse.set_subdivided_slots(vec![WaveSlot::Impulse4]);
        // The collection's own owner would fill this via
        // `elliott::count::label_collision_indices` — a correction (or any
        // type whose own last label is non-empty) nested into wave 4 (span
        // (3, 4)) collides at index 4, matching that function's own
        // `correction_nested_in_wave4_collides_at_index_4_only` test.
        impulse.set_label_collisions(vec![4]);

        // Wave 4's own label sits at point index 4, nudged off it by
        // `wave_label_offset` — click dead center on the point itself,
        // which always falls inside the label plate's own rectangle
        // (`label_plate_rect` is centered on the label's anchor).
        let (x, y) = screen_point((4_000, 120.0), &bars, &viewport, &price_scale);
        let collides = impulse.label_collisions.contains(&4);
        let (dx, dy) = wave_label_offset(
            &impulse.points.iter().map(|(ts, p)| screen_point((*ts, *p), &bars, &viewport, &price_scale)).collect::<Vec<_>>(),
            4,
            &impulse.label_style,
            collides,
        );
        assert_eq!(
            impulse.wave_at(x + dx, y + dy, &bars, &viewport, &price_scale),
            Some(WaveSlot::Impulse4),
            "wave 4's own label plate still answers wave_at even though its leg no longer draws"
        );
    }
}

#[cfg(test)]
mod truncated_fifth_label_tests {
    use super::*;
    use super::super::count::label_collision_indices;

    fn sample_points() -> [(i64, f64); 6] {
        [
            (0, 100.0),
            (1_000, 110.0),
            (2_000, 105.0),
            (3_000, 130.0),
            (4_000, 120.0),
            (5_000, 140.0),
        ]
    }

    /// Flag off: `point_labels` is byte-for-byte what `wave_labels` itself
    /// produces — the mark is never applied — for two different degrees.
    #[test]
    fn flag_off_matches_wave_labels_exactly_for_two_degrees() {
        for degree in [WaveDegree::Intermediate, WaveDegree::Primary] {
            let mut impulse = ElliottImpulse::new(sample_points(), "#2196F3");
            impulse.degree = degree;
            let expected: Vec<String> = impulse.wave_labels().iter().map(|s| s.to_string()).collect();

            let labels = impulse.point_labels().expect("an ElliottImpulse always returns Some point_labels");
            assert_eq!(labels, expected, "flag off: point_labels equals wave_labels byte for byte for {degree:?}");
        }
    }

    /// Flag on: wave 5's own label (index 5, the last slot) gains
    /// [`TRUNCATED_FIFTH_MARK`] appended AFTER whatever bracket style the
    /// degree already produced — every other label (including the empty
    /// start-point label at index 0) is untouched, for two different
    /// degrees.
    #[test]
    fn flag_on_appends_mark_to_wave5_only_for_two_degrees() {
        for degree in [WaveDegree::Intermediate, WaveDegree::Primary] {
            let mut impulse = ElliottImpulse::new(sample_points(), "#2196F3");
            impulse.degree = degree;
            let unmarked: Vec<String> = impulse.wave_labels().iter().map(|s| s.to_string()).collect();

            impulse.truncated_fifth = true;
            let labels = impulse.point_labels().expect("an ElliottImpulse always returns Some point_labels");

            for i in 0..5 {
                assert_eq!(labels[i], unmarked[i], "point {i} untouched by the flag for {degree:?}");
            }
            assert_eq!(
                labels[5],
                format!("{}{}", unmarked[5], TRUNCATED_FIFTH_MARK),
                "wave 5's own label gains the trailing mark, appended after the degree's own notation, for {degree:?}"
            );
        }
    }

    /// `label_collision_indices` (`super::super::count`) derives a
    /// collision solely from whether the CHILD's own first/last
    /// `point_labels` are empty (see that function's own doc) — appending
    /// the mark never changes emptiness, so a parent's collision indices
    /// for a child nested at the same slot are identical whether the
    /// child's own `truncated_fifth` is on or off, on the same geometry: a
    /// pivot that was labelled stays labelled, a blank one stays blank.
    #[test]
    fn label_collision_indices_unaffected_by_truncated_fifth_flag() {
        let parent = ElliottImpulse::new(sample_points(), "#2196F3");
        let mut child = ElliottImpulse::new(sample_points(), "#2196F3");

        child.truncated_fifth = false;
        let indices_off = label_collision_indices(&parent, &[(&child, WaveSlot::Impulse3)]);

        child.truncated_fifth = true;
        let indices_on = label_collision_indices(&parent, &[(&child, WaveSlot::Impulse3)]);

        assert_eq!(indices_off, indices_on, "the truncation mark never changes whether a label counts as empty/non-empty");
        // The child's own last label (wave 5) is always non-empty (marked
        // or not), so its span's own END pivot on the parent always
        // collides — sanity check that this geometry exercises the rule
        // rather than vacuously comparing two empty vectors.
        assert!(!indices_off.is_empty(), "sanity: this geometry does produce a collision to compare");
    }
}

#[cfg(test)]
mod notation_style_label_tests {
    use super::*;

    fn sample_points() -> [(i64, f64); 6] {
        [(0, 100.0), (1_000, 110.0), (2_000, 105.0), (3_000, 130.0), (4_000, 120.0), (5_000, 140.0)]
    }

    /// `FollowDegree` (the default) reproduces `WaveDegree::impulse_labels`
    /// byte for byte, for two different degrees whose own tables differ
    /// (Primary is circled numbers, Minor is plain arabic) — the default
    /// choice never changes what a drawing saved before this field existed
    /// renders.
    #[test]
    fn follow_degree_default_matches_degree_table_for_two_degrees() {
        for degree in [WaveDegree::Primary, WaveDegree::Minor] {
            let mut impulse = ElliottImpulse::new(sample_points(), "#2196F3");
            impulse.degree = degree;
            assert_eq!(impulse.notation_style, NotationStyle::FollowDegree, "default is FollowDegree");
            let expected = degree.impulse_labels();
            assert_eq!(
                impulse.wave_labels(),
                ["", expected[0], expected[1], expected[2], expected[3], expected[4]],
                "FollowDegree reproduces {degree:?}'s own table exactly"
            );
        }
    }

    /// Owner correction 2026-08-17: a `FollowDegree` structure (the
    /// default, so EVERY freshly placed impulse) must declare the pill
    /// matching what its own degree actually draws, not "nothing lit" —
    /// checked for a roman-table degree (Cycle) and an arabic-table degree
    /// (Minor), while `wave_labels()` itself is untouched in both cases
    /// (the resolution is declaration-only, never a second place that
    /// computes labels).
    #[test]
    fn follow_degree_declares_the_pill_its_own_degree_table_actually_draws() {
        let mut impulse = ElliottImpulse::new(sample_points(), "#2196F3");
        impulse.degree = WaveDegree::Cycle;
        assert_eq!(impulse.declared_notation_style(), NotationStyle::Roman, "Cycle's own table is bare roman numerals");
        assert_eq!(impulse.wave_labels(), ["", "I", "II", "III", "IV", "V"], "wave_labels still reads FollowDegree's own table, unchanged");

        impulse.degree = WaveDegree::Minor;
        assert_eq!(impulse.declared_notation_style(), NotationStyle::Arabic, "Minor's own table is bare arabic digits");
        assert_eq!(impulse.wave_labels(), ["", "1", "2", "3", "4", "5"], "wave_labels still reads FollowDegree's own table, unchanged");
    }

    /// Each explicit notation choice produces the SAME fixed label set
    /// regardless of degree — the whole point (owner: "cannot choose arabic
    /// where the degree dictates roman") — checked for two degrees whose
    /// own `impulse_labels()` differ from both each other and from every
    /// fixed family (Cycle is bare roman, Intermediate is parenthesized
    /// arabic).
    #[test]
    fn explicit_notation_overrides_degree_for_two_degrees() {
        for degree in [WaveDegree::Cycle, WaveDegree::Intermediate] {
            let mut impulse = ElliottImpulse::new(sample_points(), "#2196F3");
            impulse.degree = degree;

            impulse.notation_style = NotationStyle::Arabic;
            assert_eq!(impulse.wave_labels(), ["", "1", "2", "3", "4", "5"], "Arabic ignores {degree:?}");

            impulse.notation_style = NotationStyle::Roman;
            assert_eq!(impulse.wave_labels(), ["", "I", "II", "III", "IV", "V"], "Roman ignores {degree:?}");

            impulse.notation_style = NotationStyle::Circled;
            assert_eq!(impulse.wave_labels(), ["", "①", "②", "③", "④", "⑤"], "Circled ignores {degree:?}");
        }
    }

    /// The truncated-fifth mark (`bed7fd21`) still composes on top of
    /// whichever notation is active — appended to wave 5's own label
    /// exactly as it does for the degree-driven default.
    #[test]
    fn truncation_mark_composes_with_every_notation() {
        let mut impulse = ElliottImpulse::new(sample_points(), "#2196F3");
        impulse.truncated_fifth = true;
        for style in [NotationStyle::FollowDegree, NotationStyle::Arabic, NotationStyle::Roman, NotationStyle::Circled] {
            impulse.notation_style = style;
            let unmarked = impulse.wave_labels();
            let rendered = impulse.point_labels().expect("an ElliottImpulse always returns Some point_labels");
            for i in 0..5 {
                assert_eq!(rendered[i], unmarked[i], "point {i} untouched by the flag for {style:?}");
            }
            assert_eq!(rendered[5], format!("{}{}", unmarked[5], TRUNCATED_FIFTH_MARK), "wave 5 gains the mark for {style:?}");
        }
    }
}

#[cfg(test)]
mod custom_wave_labels_tests {
    use super::*;

    fn sample() -> ElliottImpulse {
        ElliottImpulse::new(
            [(0, 100.0), (1_000, 110.0), (2_000, 105.0), (3_000, 130.0), (4_000, 120.0), (5_000, 140.0)],
            "#2196F3",
        )
    }

    /// A click on the Label column of the custom-labels table (row 2, the
    /// `col:1` the pure decision — `table_grid`, `mlc-app-layout` — hands an
    /// `EditableText` cell) lands in `custom_wave_labels[2]` through the
    /// exact same `apply_style_property` path any other declared property
    /// reaches, and the new text is what `rendered_wave_labels` reads back —
    /// every other index untouched.
    #[test]
    fn custom_wave_label_cell_click_round_trips_through_apply_style_property() {
        let mut impulse = sample();
        let before = impulse.wave_labels()[2].to_string();
        let applied = impulse.apply_style_property("custom_wave_labels::cell:2:1", &PropertyValue::String("Custom!".to_string()));
        assert!(applied, "a well-formed cell id must be recognised");
        assert_eq!(impulse.rendered_wave_labels()[2], "Custom!");
        assert_ne!(impulse.rendered_wave_labels()[2], before, "the override actually changed what index 2 renders");
        for i in [0, 1, 3, 4, 5] {
            assert_eq!(
                impulse.rendered_wave_labels()[i], impulse.wave_labels()[i],
                "point {i} has no override, so it still reads the computed label"
            );
        }
    }

    /// An empty string clears a previously-set override back to the
    /// computed default.
    #[test]
    fn empty_string_clears_the_override() {
        let mut impulse = sample();
        impulse.apply_style_property("custom_wave_labels::cell:1:1", &PropertyValue::String("X".to_string()));
        assert_eq!(impulse.rendered_wave_labels()[1], "X");
        impulse.apply_style_property("custom_wave_labels::cell:1:1", &PropertyValue::String(String::new()));
        assert_eq!(
            impulse.rendered_wave_labels()[1], impulse.wave_labels()[1],
            "a cleared override falls back to the computed label"
        );
    }

    /// The Wave# column (col 0) is inert by construction — the pure
    /// `table_grid` decision (`mlc-app-layout`) never hands it a click id at
    /// all, and even a hand-crafted id naming it is rejected here too,
    /// defense in depth against a future caller that bypasses the renderer.
    #[test]
    fn wave_number_column_is_rejected() {
        let mut impulse = sample();
        let before = impulse.custom_wave_labels.clone();
        let applied = impulse.apply_style_property("custom_wave_labels::cell:2:0", &PropertyValue::String("hacked".to_string()));
        assert!(!applied, "the inert Wave# column must never report as applied");
        assert_eq!(impulse.custom_wave_labels, before);
    }

    /// The override composes with the truncation mark by fully REPLACING
    /// it — an explicit override on wave 5 is applied LAST in
    /// `rendered_wave_labels`, so it wins over the mark truncation already
    /// appended (an "override" means override, not "append after").
    #[test]
    fn override_replaces_the_truncation_mark_too() {
        let mut impulse = sample();
        impulse.truncated_fifth = true;
        let marked = impulse.rendered_wave_labels()[5].clone();
        assert!(marked.contains(TRUNCATED_FIFTH_MARK), "sanity: this fixture does mark wave 5");
        impulse.apply_style_property("custom_wave_labels::cell:5:1", &PropertyValue::String("V".to_string()));
        assert_eq!(impulse.rendered_wave_labels()[5], "V", "an explicit override wins over the truncation mark");
    }

    /// The declared property itself: one row per point, `Wave`#/`Label`
    /// header, seeded with what `rendered_wave_labels` actually produces —
    /// so opening the row never shows a stale value.
    #[test]
    fn declared_property_seeds_from_rendered_labels() {
        use crate::drawing::primitives_v2::config::{PropertyType, TableCell, CUSTOM_WAVE_LABELS_PROPERTY_ID};

        let mut impulse = sample();
        impulse.apply_style_property("custom_wave_labels::cell:3:1", &PropertyValue::String("3ʳᵈ".to_string()));
        let props = impulse.style_properties();
        let table = props.iter().find(|p| p.id == CUSTOM_WAVE_LABELS_PROPERTY_ID).expect("declares the override table");
        let PropertyType::Table { head, rows } = &table.prop_type else { panic!("must be PropertyType::Table") };
        assert_eq!(head.len(), 2);
        assert_eq!(rows.len(), 6, "one row per point (Start + 5 waves)");
        assert_eq!(rows[3][1], TableCell::EditableText("3ʳᵈ".to_string()));
        assert_eq!(rows[3][0], TableCell::Text("3".to_string()), "Wave# column is inert plain text");
    }
}

#[cfg(test)]
mod impulse_kind_tests {
    //! `ElliottImpulse::impulse_kind` (the diagonal arc) — the field's own
    //! defaulting/round-trip contract, and the `apply_style_property` write
    //! path the settings-modal dropdown reaches it through.

    use super::*;

    fn sample_points() -> [(i64, f64); 6] {
        [(0, 100.0), (1_000, 110.0), (2_000, 105.0), (3_000, 130.0), (4_000, 120.0), (5_000, 140.0)]
    }

    /// A freshly constructed impulse is `ImpulseKind::Plain` — nothing about
    /// an existing drawing changes.
    #[test]
    fn new_impulse_defaults_to_plain() {
        let impulse = ElliottImpulse::new(sample_points(), "#2196F3");
        assert_eq!(impulse.impulse_kind, ImpulseKind::Plain);
        assert_eq!(impulse.impulse_kind(), None, "a plain impulse carries no subtype token — the bare \"elliott_impulse\" kind id");
    }

    /// `impulse_kind` (leading or ending) survives a full JSON round trip —
    /// the same wire shape every other Elliott field already gets via
    /// `to_json`/`serde_json::from_str`.
    #[test]
    fn leading_and_ending_diagonal_survive_a_json_round_trip() {
        for kind in [ImpulseKind::LeadingDiagonal, ImpulseKind::EndingDiagonal] {
            let mut impulse = ElliottImpulse::new(sample_points(), "#2196F3");
            impulse.impulse_kind = kind;
            let json = impulse.to_json();
            let restored: ElliottImpulse = serde_json::from_str(&json).expect("must deserialize");
            assert_eq!(restored.impulse_kind, kind, "{kind:?} must survive a JSON round trip");
        }
    }

    /// A JSON object with no `"impulse_kind"` key at all — exactly what
    /// every impulse ever saved before this field existed carries on disk —
    /// must still deserialize, defaulting to `ImpulseKind::Plain`.
    #[test]
    fn missing_field_in_saved_json_defaults_to_plain() {
        let impulse = ElliottImpulse::new(sample_points(), "#2196F3");
        let mut value: serde_json::Value = serde_json::from_str(&impulse.to_json()).expect("valid json");
        value.as_object_mut().expect("primitive serializes as a JSON object").remove("impulse_kind");
        assert!(
            !value.to_string().contains("impulse_kind"),
            "sanity: the key really is gone before deserializing"
        );
        let restored: ElliottImpulse = serde_json::from_value(value).expect("must deserialize without the field");
        assert_eq!(restored.impulse_kind, ImpulseKind::Plain, "a pre-existing drawing loads as a plain impulse");
    }

    /// `apply_style_property("impulse_kind", ..)` — the settings-modal
    /// dropdown's own write path, same shape
    /// `ElliottCorrection::apply_style_property`'s own `"correction_type"`
    /// arm already has.
    #[test]
    fn apply_style_property_writes_impulse_kind() {
        let mut impulse = ElliottImpulse::new(sample_points(), "#2196F3");
        assert!(impulse.apply_style_property("impulse_kind", &PropertyValue::String("leading_diagonal".to_string())));
        assert_eq!(impulse.impulse_kind, ImpulseKind::LeadingDiagonal);
        assert!(impulse.apply_style_property("impulse_kind", &PropertyValue::String("ending_diagonal".to_string())));
        assert_eq!(impulse.impulse_kind, ImpulseKind::EndingDiagonal);
        assert!(impulse.apply_style_property("impulse_kind", &PropertyValue::String("plain".to_string())));
        assert_eq!(impulse.impulse_kind, ImpulseKind::Plain);
    }
}
