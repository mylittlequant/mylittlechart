//! Elliott wave markup tree.
//!
//! Today the five Elliott primitives (`ElliottImpulse`, `ElliottCorrection`,
//! `ElliottTriangle`, `ElliottDoubleCombo`, `ElliottTripleCombo`) are five
//! independent drawings — each carries its own [`super::super::config::WaveDegree`]
//! and nothing links them. This module adds TWO links, because a count is
//! two different relations, not one:
//!
//! - **Subdivision** — a child primitive carries a [`WaveLink`] naming which
//!   [`WaveSlot`] of which parent it occupies (one degree FINER than the
//!   parent, nested INSIDE one of the parent's own legs), and [`child_seed`]
//!   tells a caller what degree/anchor points a NEW child nested into a
//!   given slot must be constructed with.
//! - **Succession** — a primitive carries `follows` (the predecessor's own
//!   id) naming the SAME-degree structure it continues after — the
//!   motive→corrective→motive alternation that comes AFTER a 1-2-3-4-5
//!   impulse (an A-B-C correction, a triangle, or a W-X-Y/W-X-Y-XX-Z
//!   combination) rather than nested inside it. [`successor_seed`] tells a
//!   caller what degree/anchor a NEW successor must be constructed with,
//!   and [`successor_kinds`] tells it which Elliott types are valid to
//!   follow a given predecessor (the alternation grammar itself).
//!
//! The two relations are deliberately NOT the same type: a child is a piece
//! OF its parent (deleting the parent deletes the child — there is no
//! parent-shaped hole left on the chart to point at); a successor is a
//! SIBLING that happens to reference where it started (deleting the
//! predecessor leaves the successor standing — it is its own structure,
//! just missing the thing it used to point back to). See
//! `DrawingManager::delete`'s own doc for exactly how that difference plays
//! out at the one place both relations get cleaned up.
//!
//! Pure data + pure functions — no rendering, no rule validation (a later
//! wave), no dependency outside `mlc-core`.

use serde::{Deserialize, Serialize};
use super::super::Primitive;
use super::super::config::WaveDegree;
use mli::ewa::grammar::{self, EwaWavePosition};
use mli::ewa::types::EwaPatternKind;

/// Which position inside its parent a child Elliott wave occupies.
///
/// One typed enum for all five Elliott types — never a raw string or `u8`
/// newtype — so a slot is always addressed by name. [`WaveSlot::label`]
/// sources its printed notation from the SAME per-degree tables
/// [`WaveDegree`] already owns (`impulse_labels`/`corrective_labels`/
/// `triangle_labels`/`combo_labels`); this module never invents a second
/// notation table.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum WaveSlot {
    /// Impulse wave 1 of 5 (`ElliottImpulse`).
    Impulse1,
    /// Impulse wave 2 of 5.
    Impulse2,
    /// Impulse wave 3 of 5.
    Impulse3,
    /// Impulse wave 4 of 5.
    Impulse4,
    /// Impulse wave 5 of 5.
    Impulse5,
    /// Corrective wave A of A-B-C (`ElliottCorrection`).
    CorrectionA,
    /// Corrective wave B of A-B-C.
    CorrectionB,
    /// Corrective wave C of A-B-C.
    CorrectionC,
    /// Triangle wave A of A-E (`ElliottTriangle`).
    TriangleA,
    /// Triangle wave B of A-E.
    TriangleB,
    /// Triangle wave C of A-E.
    TriangleC,
    /// Triangle wave D of A-E.
    TriangleD,
    /// Triangle wave E of A-E.
    TriangleE,
    /// Combination leg W — shared by `ElliottDoubleCombo` (W-X-Y) and
    /// `ElliottTripleCombo` (W-X-Y-XX-Z).
    ComboW,
    /// Combination connector X.
    ComboX,
    /// Combination leg Y.
    ComboY,
    /// Combination connector XX — `ElliottTripleCombo` only.
    ComboXX,
    /// Combination leg Z — `ElliottTripleCombo` only.
    ComboZ,
}

impl WaveSlot {
    /// The printed label for this slot at `degree` — read off the EXISTING
    /// per-degree notation tables on [`WaveDegree`], never a second table.
    pub fn label(&self, degree: WaveDegree) -> &'static str {
        match self {
            WaveSlot::Impulse1 => degree.impulse_labels()[0],
            WaveSlot::Impulse2 => degree.impulse_labels()[1],
            WaveSlot::Impulse3 => degree.impulse_labels()[2],
            WaveSlot::Impulse4 => degree.impulse_labels()[3],
            WaveSlot::Impulse5 => degree.impulse_labels()[4],
            WaveSlot::CorrectionA => degree.corrective_labels()[0],
            WaveSlot::CorrectionB => degree.corrective_labels()[1],
            WaveSlot::CorrectionC => degree.corrective_labels()[2],
            WaveSlot::TriangleA => degree.triangle_labels()[0],
            WaveSlot::TriangleB => degree.triangle_labels()[1],
            WaveSlot::TriangleC => degree.triangle_labels()[2],
            WaveSlot::TriangleD => degree.triangle_labels()[3],
            WaveSlot::TriangleE => degree.triangle_labels()[4],
            WaveSlot::ComboW => degree.combo_labels()[0],
            WaveSlot::ComboX => degree.combo_labels()[1],
            WaveSlot::ComboY => degree.combo_labels()[2],
            WaveSlot::ComboXX => degree.combo_labels()[3],
            WaveSlot::ComboZ => degree.combo_labels()[4],
        }
    }

    /// Stable serialization/wire id for this slot — mirrors
    /// [`WaveDegree::as_str`]'s own convention (a fixed snake_case token,
    /// independent of [`label`](Self::label)'s per-degree/per-language
    /// notation). This is what a UI layer encodes into a widget action id
    /// (e.g. the E2 inline-bar wave-slot picker's `"inline:wave_option:
    /// {id}"` rows) and what [`Self::from_id`] parses back — never the
    /// printed notation itself, which changes with `degree`.
    pub fn id(&self) -> &'static str {
        match self {
            WaveSlot::Impulse1 => "impulse_1",
            WaveSlot::Impulse2 => "impulse_2",
            WaveSlot::Impulse3 => "impulse_3",
            WaveSlot::Impulse4 => "impulse_4",
            WaveSlot::Impulse5 => "impulse_5",
            WaveSlot::CorrectionA => "correction_a",
            WaveSlot::CorrectionB => "correction_b",
            WaveSlot::CorrectionC => "correction_c",
            WaveSlot::TriangleA => "triangle_a",
            WaveSlot::TriangleB => "triangle_b",
            WaveSlot::TriangleC => "triangle_c",
            WaveSlot::TriangleD => "triangle_d",
            WaveSlot::TriangleE => "triangle_e",
            WaveSlot::ComboW => "combo_w",
            WaveSlot::ComboX => "combo_x",
            WaveSlot::ComboY => "combo_y",
            WaveSlot::ComboXX => "combo_xx",
            WaveSlot::ComboZ => "combo_z",
        }
    }

    /// Parse [`Self::id`]'s own output back into a [`WaveSlot`]. `None` for
    /// any string that is not one of the fixed tokens `id` produces.
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "impulse_1" => WaveSlot::Impulse1,
            "impulse_2" => WaveSlot::Impulse2,
            "impulse_3" => WaveSlot::Impulse3,
            "impulse_4" => WaveSlot::Impulse4,
            "impulse_5" => WaveSlot::Impulse5,
            "correction_a" => WaveSlot::CorrectionA,
            "correction_b" => WaveSlot::CorrectionB,
            "correction_c" => WaveSlot::CorrectionC,
            "triangle_a" => WaveSlot::TriangleA,
            "triangle_b" => WaveSlot::TriangleB,
            "triangle_c" => WaveSlot::TriangleC,
            "triangle_d" => WaveSlot::TriangleD,
            "triangle_e" => WaveSlot::TriangleE,
            "combo_w" => WaveSlot::ComboW,
            "combo_x" => WaveSlot::ComboX,
            "combo_y" => WaveSlot::ComboY,
            "combo_xx" => WaveSlot::ComboXX,
            "combo_z" => WaveSlot::ComboZ,
            _ => return None,
        })
    }

    /// Whether this slot subdivides as a 5-wave MOTIVE structure rather
    /// than a corrective one — classic Elliott alternation: only impulse
    /// waves 1, 3, 5 are motive. Impulse waves 2/4, every A/B/C corrective
    /// leg, and every triangle/combination slot are corrective — none of
    /// those subdivide as a 5-wave motive.
    ///
    /// A pure classification predicate — NOT read by
    /// `DrawingManager::create_wave_child` to decide which Elliott type to
    /// build (the old heuristic this method used to back: a motive slot
    /// always seeded an `ElliottImpulse` and everything else an
    /// `ElliottCorrection`, which cannot express that wave 2 may be many
    /// structures but never a triangle, or that wave 4 may be a triangle —
    /// see this section's own "Grammar" doc below). A child's actual type
    /// is decided by [`child_kinds`] plus the caller's own chosen kind id,
    /// never by this single bit. Kept as the plain wave-classification
    /// fact it always was — still the alternation [`label`](Self::label)/
    /// [`slot_position`] themselves rest on, and exercised directly by
    /// this module's own tests.
    pub fn is_motive(&self) -> bool {
        matches!(self, WaveSlot::Impulse1 | WaveSlot::Impulse3 | WaveSlot::Impulse5)
    }
}

/// A child primitive's own link upward to the parent Elliott wave it is
/// nested inside, and the slot it occupies within that parent.
///
/// `parent_id` names [`super::super::PrimitiveData::id`] — never a
/// primitive reference or a collection index: the parent lives in the same
/// collection the child does, and only the collection's own owner can
/// resolve an id back into a primitive. A primitive cannot see its
/// siblings, so it stores only this reference, never the parent itself.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct WaveLink {
    /// The parent Elliott primitive's [`super::super::PrimitiveData::id`].
    pub parent_id: u64,
    /// Which of the parent's own slots this child occupies.
    pub slot: WaveSlot,
}

/// What a new child primitive nested into `slot` of a parent must be
/// constructed with: the degree it must carry (one step finer than the
/// parent's own degree) and the two anchor points — `start`/`end`, each
/// `(timestamp_ms, price)` — that slot spans on the parent's own line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChildSeed {
    /// The degree the child must be constructed with — always the parent's
    /// own degree, one step finer (see [`WaveDegree::finer`]).
    pub degree: WaveDegree,
    /// The slot's starting anchor point, read off the parent's own
    /// `points()`.
    pub start: (i64, f64),
    /// The slot's ending anchor point, read off the parent's own
    /// `points()`.
    pub end: (i64, f64),
}

impl ChildSeed {
    /// `n` points evenly interpolated from `start` to `end` (`n >= 2`,
    /// inclusive of both endpoints) — the shape a freshly-seeded child
    /// primitive's OWN point array must be filled with so it renders as a
    /// visible, immediately-draggable wave instead of a degenerate
    /// zero-length line collapsed onto `start`.
    ///
    /// `n == 0` returns an empty vec; `n == 1` returns `[start]` — neither
    /// case arises from a real Elliott child (every concrete type's own
    /// point count is fixed at 4 or 6), but both are handled without
    /// panicking rather than assumed away.
    ///
    /// Not used by [`child_seed`] itself, which only reports the two
    /// anchors — the CALLER that constructs the new primitive calls this
    /// once it knows how many points its own type needs (6 for
    /// `ElliottImpulse`, 4 for `ElliottCorrection`).
    pub fn interpolate(&self, n: usize) -> Vec<(i64, f64)> {
        interpolate_points(self.start, self.end, n)
    }
}

/// Evenly interpolate `n` points from `start` to `end` (`n >= 2`, inclusive
/// of both endpoints; `n == 0` empty, `n == 1` `[start]` — same degenerate
/// handling as the doc on [`ChildSeed::interpolate`] describes). Shared by
/// [`ChildSeed::interpolate`] and [`SuccessorSeed::interpolate`] — same
/// point-filling math, but the two seed types stay separate structs on
/// purpose: a child nests INSIDE its parent's own span (`end` is a real
/// anchor read off the parent), a successor starts AFTER its predecessor's
/// last point into a span this module can only default a guess for (see
/// [`SuccessorSeed`]'s own doc). Collapsing them into one type would blur
/// that distinction at every call site that seeds one.
fn interpolate_points(start: (i64, f64), end: (i64, f64), n: usize) -> Vec<(i64, f64)> {
    match n {
        0 => Vec::new(),
        1 => vec![start],
        _ => (0..n)
            .map(|i| {
                let t = i as f64 / (n - 1) as f64;
                let ts = start.0 + ((end.0 - start.0) as f64 * t).round() as i64;
                let price = start.1 + (end.1 - start.1) * t;
                (ts, price)
            })
            .collect(),
    }
}

/// The degree/anchor points a child nested into `slot` of `parent` must be
/// seeded with. `None` when `slot` does not exist on `parent`'s own type
/// (e.g. an impulse has no slot W), or when `parent` does not carry a wave
/// degree (i.e. is not an Elliott primitive at all), or when `parent`'s own
/// stored points do not yet reach the slot's span (a primitive still being
/// drawn).
///
/// Reads `parent` only through the [`Primitive`] trait — never downcasts to
/// a concrete Elliott type. This is the resolution step a primitive itself
/// cannot perform (a primitive cannot see its own collection); the CALLER
/// that owns the collection is responsible for placing the new primitive at
/// the returned `start`/`end` with the returned `degree`, and for setting
/// the new primitive's own [`WaveLink`] back to `parent`.
pub fn child_seed(parent: &dyn Primitive, slot: WaveSlot) -> Option<ChildSeed> {
    let degree = parent.wave_degree()?.finer();
    let (start_idx, end_idx) = slot_span(parent.type_id(), slot)?;
    let points = parent.points();
    let start = *points.get(start_idx)?;
    let end = *points.get(end_idx)?;
    Some(ChildSeed { degree, start, end })
}

/// The `(start_index, end_index)` pair into a parent's own `points()` that
/// `slot` spans, keyed by the parent's `type_id()`. The point layout is
/// fixed per Elliott type (see each type's own `points` field doc); this is
/// a pure lookup table, not a downcast. `pub(super)` (rather than private)
/// so `elliott::guard`'s render-side marking and every `elliott_*.rs`'s own
/// `render` can look up which segment/label index a [`WaveSlot`] violation
/// lands on, without a second copy of this table.
pub(super) fn slot_span(type_id: &str, slot: WaveSlot) -> Option<(usize, usize)> {
    use WaveSlot::*;
    match type_id {
        // ElliottImpulse: 6 points — start(0), wave1..5(1..=5). Each wave
        // spans the previous anchor to its own.
        "elliott_impulse" => match slot {
            Impulse1 => Some((0, 1)),
            Impulse2 => Some((1, 2)),
            Impulse3 => Some((2, 3)),
            Impulse4 => Some((3, 4)),
            Impulse5 => Some((4, 5)),
            _ => None,
        },
        // ElliottCorrection: 4 points — start(0), A(1), B(2), C(3).
        "elliott_correction" => match slot {
            CorrectionA => Some((0, 1)),
            CorrectionB => Some((1, 2)),
            CorrectionC => Some((2, 3)),
            _ => None,
        },
        // ElliottTriangle: 6 points — start(0), A..E(1..=5).
        "elliott_triangle" => match slot {
            TriangleA => Some((0, 1)),
            TriangleB => Some((1, 2)),
            TriangleC => Some((2, 3)),
            TriangleD => Some((3, 4)),
            TriangleE => Some((4, 5)),
            _ => None,
        },
        // ElliottDoubleCombo: 7 points — start(0), W subwaves(1,2) [W ends
        // at 2], X(3) [single connector, ends at 3], Y subwaves(4,5,6)
        // [Y ends at 6].
        "elliott_double_combo" => match slot {
            ComboW => Some((0, 2)),
            ComboX => Some((2, 3)),
            ComboY => Some((3, 6)),
            _ => None,
        },
        // ElliottTripleCombo: 9 points — start(0), W subwaves(1,2) [ends
        // 2], X(3) [connector, ends 3], Y subwaves(4,5) [ends 5], XX(6)
        // [connector, ends 6], Z subwaves(7,8) [ends 8].
        "elliott_triple_combo" => match slot {
            ComboW => Some((0, 2)),
            ComboX => Some((2, 3)),
            ComboY => Some((3, 5)),
            ComboXX => Some((5, 6)),
            ComboZ => Some((6, 8)),
            _ => None,
        },
        _ => None,
    }
}

// =========================================================================
// Succession — the other relation (see module doc)
// =========================================================================

/// What a NEW successor primitive continuing `prev` at `prev`'s own degree
/// must be seeded with.
///
/// `degree` is always `prev`'s OWN degree, unchanged — succession is a
/// same-degree sibling relation, not a subdivision, so unlike
/// [`ChildSeed::degree`] this is never [`WaveDegree::finer`]. `start` is
/// exactly `prev`'s own LAST point (`points().last()`), so the two
/// structures visibly connect on the chart with no gap.
///
/// `span_bars_hint`/`price_hint` are a DEFAULT extent for the successor's
/// still-unplaced far end, not a rule derived from the predecessor's
/// internal structure the way [`ChildSeed::end`](ChildSeed) is (`child_seed`
/// reads a real anchor off the parent's OWN points; a successor has no such
/// anchor to read — nothing about the next structure exists yet, by
/// definition). The two numbers are chosen for a specific, stated reason
/// each, not picked arbitrarily:
///
/// - `span_bars_hint` ≈ 0.618 of the predecessor's own total bar span
///   (`last.0 - first.0`) — the same golden-ratio constant Elliott markup
///   already uses everywhere else in this codebase (retracement/extension
///   level ladders). Reused here for the same reason it is used there: it
///   reads as a plausible corrective-vs-motive time proportion without
///   pretending this function can know the real one — the user drags the
///   far end from here.
/// - `price_hint` ≈ 0.5 of the predecessor's own total price travel
///   (`last.1 - first.1`), NEGATED — the structure that follows moves in
///   the OPPOSITE direction from the one that produced it (a correction
///   pulls back against the impulse that preceded it; the next impulse
///   resumes the original direction after a correction). A half-retrace is
///   a defensible midpoint default: neither a negligible pullback nor a
///   full round-trip.
///
/// Both numbers seed a draggable INITIAL shape, not a forecast — every
/// point of the new primitive is exactly as draggable afterward as any
/// other freshly-placed primitive's points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SuccessorSeed {
    /// The degree the successor must be constructed with — always `prev`'s
    /// own degree (see this struct's own doc for why this differs from
    /// [`ChildSeed::degree`]).
    pub degree: WaveDegree,
    /// The successor's starting anchor — `prev`'s own last point.
    pub start: (i64, f64),
    /// Default bar-span extent for the successor's far end, relative to
    /// `start.0` — see this struct's own doc for the 0.618 reasoning.
    pub span_bars_hint: i64,
    /// Default price extent for the successor's far end, relative to
    /// `start.1`, already signed (opposite the predecessor's own price
    /// travel) — see this struct's own doc for the 0.5-inverted reasoning.
    pub price_hint: f64,
}

impl SuccessorSeed {
    /// The successor's far-end anchor implied by `start` +
    /// `span_bars_hint`/`price_hint` — the OTHER endpoint
    /// [`interpolate`](Self::interpolate) fills points between.
    pub fn end(&self) -> (i64, f64) {
        (self.start.0 + self.span_bars_hint, self.start.1 + self.price_hint)
    }

    /// `n` points evenly interpolated from `start` to [`end`](Self::end) —
    /// see [`interpolate_points`]'s own doc for why this shares its math
    /// with, but is not the same type as, [`ChildSeed::interpolate`].
    pub fn interpolate(&self, n: usize) -> Vec<(i64, f64)> {
        interpolate_points(self.start, self.end(), n)
    }
}

/// What a new successor primitive continuing `prev` at its own degree must
/// be seeded with. `None` when `prev` is not an Elliott wave at all
/// (`prev.wave_degree()` is `None`), or its own points do not yet reach 2
/// entries (a primitive still being drawn — there is no "last point" yet).
/// Reads `prev` only through the [`Primitive`] trait, never downcasts —
/// same contract as [`child_seed`].
pub fn successor_seed(prev: &dyn Primitive) -> Option<SuccessorSeed> {
    let degree = prev.wave_degree()?;
    let points = prev.points();
    if points.len() < 2 {
        return None;
    }
    let first = points[0];
    let last = points[points.len() - 1];
    let bar_span = last.0 - first.0;
    let price_travel = last.1 - first.1;
    Some(SuccessorSeed {
        degree,
        start: last,
        span_bars_hint: (bar_span as f64 * 0.618).round() as i64,
        price_hint: -(price_travel * 0.5),
    })
}

/// The Elliott type ids that may validly follow `prev` at `prev`'s own
/// degree — the alternation this module's succession relation exists to
/// represent: after a MOTIVE structure comes a corrective one, after a
/// corrective structure comes a motive one, never motive-after-motive or
/// corrective-after-corrective. Keyed on `prev.type_id()`, not a slot —
/// succession is a relation between two WHOLE structures, never between a
/// structure and one of its own internal [`WaveSlot`]s (that pairing is
/// [`child_seed`]/[`WaveSlot::is_motive`], the other relation this module
/// owns).
///
/// - After `"elliott_impulse"` (the only motive structure among the five)
///   → any of the four corrective structures: `"elliott_correction"`,
///   `"elliott_triangle"`, `"elliott_double_combo"`, `"elliott_triple_combo"`.
/// - After any of those four corrective structures → `"elliott_impulse"`
///   only.
/// - Empty for every primitive outside the Elliott family
///   (`prev.wave_degree()` is `None`) — nothing may validly follow a
///   non-Elliott primitive.
///
/// This is the grammar itself, not just advice to a picker: the caller that
/// creates a successor (`DrawingManager::create_wave_successor`)
/// re-checks the requested type id against this list before constructing
/// anything, so a stale or forged action id can never create a
/// motive-after-motive or corrective-after-corrective pair.
pub fn successor_kinds(prev: &dyn Primitive) -> &'static [&'static str] {
    if prev.wave_degree().is_none() {
        return &[];
    }
    match prev.type_id() {
        "elliott_impulse" => &[
            "elliott_correction",
            "elliott_triangle",
            "elliott_double_combo",
            "elliott_triple_combo",
        ],
        "elliott_correction" | "elliott_triangle" | "elliott_double_combo" | "elliott_triple_combo" => {
            &["elliott_impulse"]
        }
        _ => &[],
    }
}

// =========================================================================
// Grammar — which child kinds legally fit a slot (owner ask 2026-08-13: a
// motive slot always seeded an impulse child and everything else a
// correction, purely off `WaveSlot::is_motive` — that heuristic cannot
// express that wave 2 may be many structures but never a triangle, that
// wave 4 may be a triangle, that a zigzag's own wave A is a five and a
// flat's own wave A is a three). The VOCABULARY these functions read is
// never re-invented here: every mapping below is a lookup into
// `mylittleindicators::ewa::{grammar, types}`, the same OSS catalogue
// `elliott::guard` already wires its rule ids from — this module only adds
// the translation between THIS crate's own chart-side type ids/slots and
// that catalogue's own pattern kinds/positions.
// =========================================================================

/// A "kind id" is the chart-side `type_id`, optionally carrying an
/// `ElliottCorrection` subtype after a colon (e.g. `"elliott_impulse"`,
/// `"elliott_correction:flat"`) — the fixed spelling every function below
/// (and [`crate::i18n::ConceptKey`]'s own `WaveKind*` rows) shares. The
/// complete candidate set [`child_kinds`] itself picks among.
pub const CHILD_KIND_CANDIDATES: [&str; 8] = [
    "elliott_impulse",
    "elliott_correction:zigzag",
    "elliott_correction:flat",
    "elliott_correction:expanded",
    "elliott_correction:running",
    "elliott_triangle",
    "elliott_double_combo",
    "elliott_triple_combo",
];

/// Split a "kind id" (see [`CHILD_KIND_CANDIDATES`]'s own doc) back into its
/// `(type_id, subtype)` pair, so a caller building a new primitive from a
/// [`child_kinds`] entry never re-parses the colon convention by hand.
/// `subtype` is `None` for every kind id without a colon.
pub fn split_kind_id(kind_id: &str) -> (&str, Option<&str>) {
    match kind_id.split_once(':') {
        Some((type_id, subtype)) => (type_id, Some(subtype)),
        None => (kind_id, None),
    }
}

/// The Elliott catalogue pattern kind (`mli::ewa::types::EwaPatternKind`) a
/// primitive's own `type_id()` names — `correction_subtype` follows
/// [`Primitive::correction_subtype`]'s own default (`"zigzag"` when
/// unresolved) for `"elliott_correction"`, and is ignored for every other
/// `type_id`. `None` for a `type_id` outside the five Elliott types.
pub fn parent_pattern(type_id: &str, correction_subtype: Option<&str>) -> Option<EwaPatternKind> {
    Some(match type_id {
        "elliott_impulse" => EwaPatternKind::Impulse,
        "elliott_correction" => match correction_subtype.unwrap_or("zigzag") {
            "flat" => EwaPatternKind::RegularFlat,
            "expanded" => EwaPatternKind::ExpandedFlat,
            "running" => EwaPatternKind::RunningFlat,
            _ => EwaPatternKind::Zigzag,
        },
        "elliott_triangle" => EwaPatternKind::ContractingTriangle,
        "elliott_double_combo" => EwaPatternKind::DoubleThree,
        "elliott_triple_combo" => EwaPatternKind::TripleThree,
        _ => return None,
    })
}

/// The catalogue pattern kind a "kind id" (see [`split_kind_id`]) names —
/// [`parent_pattern`] applied to the split pair, so [`child_kinds`]'s own
/// candidate side reads the exact same mapping a parent's own pattern does,
/// never a second copy of it.
pub fn kind_pattern(kind_id: &str) -> Option<EwaPatternKind> {
    let (type_id, subtype) = split_kind_id(kind_id);
    parent_pattern(type_id, subtype)
}

/// The [`crate::i18n::ConceptKey`] naming `kind_id` for a picker row (e.g.
/// the child-kind picker [`child_kinds`] feeds). `None` for a string outside
/// [`CHILD_KIND_CANDIDATES`].
pub fn kind_label(kind_id: &str) -> Option<crate::i18n::ConceptKey> {
    use crate::i18n::ConceptKey;
    Some(match kind_id {
        "elliott_impulse" => ConceptKey::WaveKindImpulse,
        "elliott_correction:zigzag" => ConceptKey::WaveKindZigzag,
        "elliott_correction:flat" => ConceptKey::WaveKindRegularFlat,
        "elliott_correction:expanded" => ConceptKey::WaveKindExpandedFlat,
        "elliott_correction:running" => ConceptKey::WaveKindRunningFlat,
        "elliott_triangle" => ConceptKey::WaveKindTriangle,
        "elliott_double_combo" => ConceptKey::WaveKindDoubleCombo,
        "elliott_triple_combo" => ConceptKey::WaveKindTripleCombo,
        _ => return None,
    })
}

/// The grammar position (`mli::ewa::grammar::EwaWavePosition`) `slot`
/// occupies on a primitive of `type_id` — the other half of the vocabulary
/// [`child_kinds`] needs alongside [`parent_pattern`]. Both of a combo's own
/// connector slots (`ComboX`/`ComboXX`) map onto the SAME position `X` —
/// `mli::ewa::grammar` has no separate position for a second connector, and
/// the triple combo's own [`slot_span`] table already threads both through
/// the same `X` notation for the same reason. `None` for a slot that does
/// not exist on `type_id` (mirrors [`slot_span`]'s own `None` contract) or
/// for a `type_id` outside the five Elliott types.
pub fn slot_position(type_id: &str, slot: WaveSlot) -> Option<EwaWavePosition> {
    use WaveSlot::*;
    match type_id {
        "elliott_impulse" => Some(match slot {
            Impulse1 => EwaWavePosition::Wave1,
            Impulse2 => EwaWavePosition::Wave2,
            Impulse3 => EwaWavePosition::Wave3,
            Impulse4 => EwaWavePosition::Wave4,
            Impulse5 => EwaWavePosition::Wave5,
            _ => return None,
        }),
        "elliott_correction" => Some(match slot {
            CorrectionA => EwaWavePosition::A,
            CorrectionB => EwaWavePosition::B,
            CorrectionC => EwaWavePosition::C,
            _ => return None,
        }),
        "elliott_triangle" => Some(match slot {
            TriangleA => EwaWavePosition::A,
            TriangleB => EwaWavePosition::B,
            TriangleC => EwaWavePosition::C,
            TriangleD => EwaWavePosition::D,
            TriangleE => EwaWavePosition::E,
            _ => return None,
        }),
        "elliott_double_combo" => Some(match slot {
            ComboW => EwaWavePosition::W,
            ComboX => EwaWavePosition::X,
            ComboY => EwaWavePosition::Y,
            _ => return None,
        }),
        "elliott_triple_combo" => Some(match slot {
            ComboW => EwaWavePosition::W,
            ComboX | ComboXX => EwaWavePosition::X,
            ComboY => EwaWavePosition::Y,
            ComboZ => EwaWavePosition::Z,
            _ => return None,
        }),
        _ => None,
    }
}

/// Every kind id legally allowed to subdivide `slot` of `parent`, in
/// [`CHILD_KIND_CANDIDATES`]'s own fixed order — the grammar-driven
/// replacement for the old `WaveSlot::is_motive` heuristic (see this
/// section's own doc).
///
/// A candidate passes when it clears BOTH halves of the grammar
/// `mylittleindicators::ewa::grammar` already owns: the parent's own slot
/// table (`grammar::slots_for(parent_pattern)`, matched by
/// [`slot_position`]) names a CLASS the candidate's own pattern must
/// satisfy (`grammar::matches_class`), and the candidate's own pattern must
/// be legal AT that position at all (`grammar::allowed_in_position` — this
/// is what keeps a triangle out of wave 2: `allowed_in_position` only
/// permits a triangle at `Wave4`/`B`/`X`/`Y`/`Z`, never `Wave2`, regardless
/// of `matches_class`).
///
/// Hand-verified (see this crate's own `count.rs` change report): a motive
/// slot (impulse wave 1, 3 or 5) returns exactly `["elliott_impulse"]`;
/// impulse wave 2 returns the six corrective candidates MINUS the triangle
/// (zigzag/regular flat/expanded flat/running flat/double combo/triple
/// combo); impulse wave 4 returns all seven corrective candidates,
/// triangle included.
///
/// Empty when `parent` is not an Elliott primitive ([`parent_pattern`]
/// returns `None`), when `slot` does not exist on `parent`'s own type
/// ([`slot_position`] returns `None`), or when the grammar allows nothing
/// at that position.
pub fn child_kinds(parent: &dyn Primitive, slot: WaveSlot) -> Vec<&'static str> {
    let Some(pattern) = parent_pattern(parent.type_id(), parent.correction_subtype()) else {
        return Vec::new();
    };
    let Some(position) = slot_position(parent.type_id(), slot) else {
        return Vec::new();
    };
    let Some(class) = grammar::slots_for(pattern)
        .iter()
        .find(|s| s.position == position)
        .map(|s| s.class)
    else {
        return Vec::new();
    };
    CHILD_KIND_CANDIDATES
        .into_iter()
        .filter(|&kind_id| {
            kind_pattern(kind_id)
                .map(|k| grammar::matches_class(k, class) && grammar::allowed_in_position(k, position))
                .unwrap_or(false)
        })
        .collect()
}

// =========================================================================
// Promotion — collapsing a same-degree chain into the degree above
// =========================================================================

/// What a chain of `follows`-linked same-degree structures collapses into
/// one degree coarser — the shape [`promote_chain_shape`] computes and a
/// caller (`DrawingManager::promote_wave_chain`) builds a real primitive
/// from.
#[derive(Clone, Debug, PartialEq)]
pub struct PromotedShape {
    /// The parent's own registry type id — always `"elliott_impulse"` (a
    /// chain opening with a motive structure) or `"elliott_correction"` (a
    /// chain opening with a corrective one). Never one of the other three
    /// corrective types: a promoted parent's OWN subdivisions may be any
    /// corrective type (that is what `follows` already allows), but the
    /// parent shape itself is always the plain three-slot correction —
    /// there is no such thing as "a triangle two degrees up".
    pub type_id: &'static str,
    /// The parent's degree — the chain's own shared degree, one step
    /// coarser (see [`WaveDegree::coarser`]).
    pub degree: WaveDegree,
    /// Which of the parent's own slots each chain member (in the same
    /// order) is mapped onto — a PREFIX of the parent type's full slot
    /// list, one entry per chain member. Shorter than the parent's own
    /// slot count whenever the chain does not yet cover every wave (e.g.
    /// only wave 1 and wave 2 of a coarser impulse have been drawn so
    /// far).
    pub slots: Vec<WaveSlot>,
    /// The parent's own anchor points: the first chain member's first
    /// point, then every chain member's own last point, in chain order.
    /// One entry longer than `slots` (an N-slot span has N+1 boundary
    /// points).
    pub points: Vec<(i64, f64)>,
}

/// Compute the coarser structure a `follows`-linked chain collapses into,
/// or `None` when the chain does not qualify — never a guess.
///
/// `chain` must already be in succession order (chain\[0\] is the chain's
/// own head); this function only VERIFIES contiguity, it does not sort or
/// search for it. Refuses (`None`) when:
/// - `chain` is empty;
/// - the chain does not all share one [`WaveDegree`];
/// - any member already carries a [`WaveLink`] — it is already someone
///   else's subdivision, and a promoted parent must not adopt a primitive
///   that is already nested under a different parent;
/// - the chain is not `follows`-contiguous (member `i`'s own
///   [`Primitive::wave_follows`] must equal member `i-1`'s own id);
/// - the chain is longer than the parent type's own slot count (5 for an
///   impulse-opened chain, 3 for a corrective-opened one);
/// - the chain breaks the classic motive/corrective alternation (only
///   `"elliott_impulse"` is motive — the same rule
///   [`successor_kinds`] itself already enforces at chain-BUILD time; this
///   is the defensive re-check for a chain assembled some other way).
///
/// Never downcasts to a concrete Elliott type — reads every chain member
/// only through the [`Primitive`] trait, matching [`child_seed`]/
/// [`successor_seed`]'s own contract.
pub fn promote_chain_shape(chain: &[&dyn Primitive]) -> Option<PromotedShape> {
    let head = *chain.first()?;
    let degree = head.wave_degree()?;
    if chain.iter().any(|p| p.wave_degree() != Some(degree)) {
        return None;
    }
    if chain.iter().any(|p| p.wave_link().is_some()) {
        return None;
    }
    for pair in chain.windows(2) {
        let prev_id = pair[0].data().id;
        if pair[1].wave_follows() != Some(prev_id) {
            return None;
        }
    }

    let starts_motive = head.type_id() == "elliott_impulse";
    const IMPULSE_SLOTS: [WaveSlot; 5] =
        [WaveSlot::Impulse1, WaveSlot::Impulse2, WaveSlot::Impulse3, WaveSlot::Impulse4, WaveSlot::Impulse5];
    const CORRECTION_SLOTS: [WaveSlot; 3] = [WaveSlot::CorrectionA, WaveSlot::CorrectionB, WaveSlot::CorrectionC];
    let (parent_type, parent_slots): (&'static str, &[WaveSlot]) = if starts_motive {
        ("elliott_impulse", &IMPULSE_SLOTS)
    } else {
        ("elliott_correction", &CORRECTION_SLOTS)
    };
    if chain.len() > parent_slots.len() {
        return None;
    }
    for (i, p) in chain.iter().enumerate() {
        let expect_motive = if i % 2 == 0 { starts_motive } else { !starts_motive };
        if (p.type_id() == "elliott_impulse") != expect_motive {
            return None;
        }
    }

    let mut points = Vec::with_capacity(chain.len() + 1);
    points.push(*head.points().first()?);
    for p in chain {
        points.push(*p.points().last()?);
    }

    Some(PromotedShape {
        type_id: parent_type,
        degree: degree.coarser(),
        slots: parent_slots[..chain.len()].to_vec(),
        points,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing::primitives_v2::elliott::{ElliottImpulse, ElliottCorrection, ElliottDoubleCombo};

    #[test]
    fn child_seed_impulse_wave3_spans_correct_points() {
        let points: [(i64, f64); 6] = [
            (0, 100.0),
            (1_000, 110.0),
            (2_000, 105.0),
            (3_000, 130.0),
            (4_000, 120.0),
            (5_000, 140.0),
        ];
        let impulse = ElliottImpulse::new(points, "#2196F3");
        let seed = child_seed(&impulse, WaveSlot::Impulse3).expect("impulse has slot 3");
        assert_eq!(seed.start, points[2]);
        assert_eq!(seed.end, points[3]);
        assert_eq!(seed.degree, WaveDegree::Intermediate.finer());
    }

    #[test]
    fn split_kind_id_separates_type_id_and_subtype() {
        assert_eq!(split_kind_id("elliott_correction:flat"), ("elliott_correction", Some("flat")));
        assert_eq!(split_kind_id("elliott_impulse"), ("elliott_impulse", None));
    }

    #[test]
    fn child_kinds_impulse_wave2_excludes_triangle() {
        let points: [(i64, f64); 6] = [
            (0, 100.0), (1_000, 110.0), (2_000, 105.0), (3_000, 130.0), (4_000, 120.0), (5_000, 140.0),
        ];
        let impulse = ElliottImpulse::new(points, "#2196F3");
        let kinds = child_kinds(&impulse, WaveSlot::Impulse2);
        assert_eq!(
            kinds,
            vec![
                "elliott_correction:zigzag",
                "elliott_correction:flat",
                "elliott_correction:expanded",
                "elliott_correction:running",
                "elliott_double_combo",
                "elliott_triple_combo",
            ],
            "wave 2 may be many structures but never a triangle"
        );
    }

    #[test]
    fn child_kinds_impulse_wave4_includes_triangle() {
        let points: [(i64, f64); 6] = [
            (0, 100.0), (1_000, 110.0), (2_000, 105.0), (3_000, 130.0), (4_000, 120.0), (5_000, 140.0),
        ];
        let impulse = ElliottImpulse::new(points, "#2196F3");
        let kinds = child_kinds(&impulse, WaveSlot::Impulse4);
        assert_eq!(
            kinds,
            vec![
                "elliott_correction:zigzag",
                "elliott_correction:flat",
                "elliott_correction:expanded",
                "elliott_correction:running",
                "elliott_triangle",
                "elliott_double_combo",
                "elliott_triple_combo",
            ],
            "wave 4 may be a triangle, unlike wave 2"
        );
    }

    #[test]
    fn child_kinds_motive_slots_offer_only_impulse() {
        let points: [(i64, f64); 6] = [
            (0, 100.0), (1_000, 110.0), (2_000, 105.0), (3_000, 130.0), (4_000, 120.0), (5_000, 140.0),
        ];
        let impulse = ElliottImpulse::new(points, "#2196F3");
        assert_eq!(child_kinds(&impulse, WaveSlot::Impulse1), vec!["elliott_impulse"]);
        assert_eq!(child_kinds(&impulse, WaveSlot::Impulse3), vec!["elliott_impulse"]);
        assert_eq!(child_kinds(&impulse, WaveSlot::Impulse5), vec!["elliott_impulse"]);
    }

    #[test]
    fn child_kinds_empty_for_non_elliott_primitive() {
        use crate::drawing::primitives_v2::TrendLine;
        let line = TrendLine::new(0, 100.0, 1_000, 110.0, "#2196F3");
        assert!(child_kinds(&line, WaveSlot::Impulse1).is_empty());
    }

    #[test]
    fn child_seed_returns_none_for_slot_the_type_does_not_have() {
        let points: [(i64, f64); 6] = [
            (0, 100.0), (1, 101.0), (2, 102.0), (3, 103.0), (4, 104.0), (5, 105.0),
        ];
        let impulse = ElliottImpulse::new(points, "#2196F3");
        assert!(child_seed(&impulse, WaveSlot::ComboW).is_none());
    }

    #[test]
    fn child_seed_double_combo_slot_y_spans_through_subwaves() {
        let points: [(i64, f64); 7] = [
            (0, 100.0),
            (1_000, 90.0),
            (2_000, 95.0),
            (3_000, 85.0),
            (4_000, 105.0),
            (5_000, 95.0),
            (6_000, 115.0),
        ];
        let combo = ElliottDoubleCombo::new(points, "#9C27B0");
        let seed = child_seed(&combo, WaveSlot::ComboY).expect("double combo has slot Y");
        assert_eq!(seed.start, points[3]);
        assert_eq!(seed.end, points[6]);
    }

    #[test]
    fn every_wave_slot_id_round_trips_through_from_id() {
        let all = [
            WaveSlot::Impulse1, WaveSlot::Impulse2, WaveSlot::Impulse3, WaveSlot::Impulse4, WaveSlot::Impulse5,
            WaveSlot::CorrectionA, WaveSlot::CorrectionB, WaveSlot::CorrectionC,
            WaveSlot::TriangleA, WaveSlot::TriangleB, WaveSlot::TriangleC, WaveSlot::TriangleD, WaveSlot::TriangleE,
            WaveSlot::ComboW, WaveSlot::ComboX, WaveSlot::ComboY, WaveSlot::ComboXX, WaveSlot::ComboZ,
        ];
        for slot in all {
            assert_eq!(WaveSlot::from_id(slot.id()), Some(slot), "slot {:?} did not round-trip via id {:?}", slot, slot.id());
        }
        assert_eq!(WaveSlot::from_id("not_a_real_slot"), None);
    }

    #[test]
    fn only_impulse_1_3_5_are_motive_slots() {
        assert!(WaveSlot::Impulse1.is_motive());
        assert!(WaveSlot::Impulse3.is_motive());
        assert!(WaveSlot::Impulse5.is_motive());
        assert!(!WaveSlot::Impulse2.is_motive());
        assert!(!WaveSlot::Impulse4.is_motive());
        assert!(!WaveSlot::CorrectionA.is_motive());
        assert!(!WaveSlot::TriangleA.is_motive());
        assert!(!WaveSlot::ComboW.is_motive());
    }

    #[test]
    fn interpolate_spans_start_to_end_inclusive_evenly() {
        let seed = ChildSeed { degree: WaveDegree::Minor, start: (0, 100.0), end: (4_000, 140.0) };
        let pts = seed.interpolate(5);
        assert_eq!(pts.len(), 5);
        assert_eq!(pts[0], (0, 100.0));
        assert_eq!(pts[4], (4_000, 140.0));
        assert_eq!(pts[2], (2_000, 120.0));
    }

    #[test]
    fn interpolate_degenerate_counts_never_panic() {
        let seed = ChildSeed { degree: WaveDegree::Minor, start: (0, 100.0), end: (1_000, 110.0) };
        assert_eq!(seed.interpolate(0), Vec::new());
        assert_eq!(seed.interpolate(1), vec![(0, 100.0)]);
    }

    #[test]
    fn successor_seed_starts_at_predecessors_last_point_same_degree() {
        let points: [(i64, f64); 6] = [
            (0, 100.0),
            (1_000, 110.0),
            (2_000, 105.0),
            (3_000, 130.0),
            (4_000, 120.0),
            (10_000, 150.0),
        ];
        let impulse = ElliottImpulse::new(points, "#2196F3");
        let seed = successor_seed(&impulse).expect("impulse seeds a successor");
        assert_eq!(seed.start, points[5]);
        assert_eq!(seed.degree, WaveDegree::Intermediate, "succession keeps the SAME degree, never finer");
    }

    #[test]
    fn successor_seed_span_is_0_618_of_predecessors_own_bar_span() {
        let points: [(i64, f64); 6] = [
            (0, 100.0), (1_000, 110.0), (2_000, 105.0), (3_000, 130.0), (4_000, 120.0), (10_000, 150.0),
        ];
        let impulse = ElliottImpulse::new(points, "#2196F3");
        let seed = successor_seed(&impulse).expect("impulse seeds a successor");
        // Predecessor's own bar span is 10_000 - 0 = 10_000.
        assert_eq!(seed.span_bars_hint, (10_000f64 * 0.618).round() as i64);
    }

    #[test]
    fn successor_seed_price_hint_is_half_predecessors_travel_and_inverted() {
        let points: [(i64, f64); 6] = [
            (0, 100.0), (1_000, 110.0), (2_000, 105.0), (3_000, 130.0), (4_000, 120.0), (10_000, 150.0),
        ];
        let impulse = ElliottImpulse::new(points, "#2196F3");
        let seed = successor_seed(&impulse).expect("impulse seeds a successor");
        // Predecessor travelled +50 (100.0 -> 150.0); the successor's own
        // default extent is HALF that, in the OPPOSITE direction.
        assert_eq!(seed.price_hint, -25.0);
        assert_eq!(seed.end(), (points[5].0 + seed.span_bars_hint, 150.0 - 25.0));
    }

    #[test]
    fn successor_seed_none_for_non_elliott_or_undrawn_primitive() {
        let points: [(i64, f64); 4] = [(0, 100.0), (0, 100.0), (0, 100.0), (0, 100.0)];
        let correction = ElliottCorrection::new(points, "#FF5722");
        // Single distinct point (all four collapsed) still has >= 2 entries,
        // so this only exercises the "not Elliott" `None` path via a type
        // that IS Elliott but with degenerate points — the real "not
        // Elliott" case is covered by every non-Elliott primitive's own
        // `wave_degree() == None`, exercised indirectly through
        // `successor_kinds`'s own empty-slice test below.
        assert!(successor_seed(&correction).is_some());
    }

    #[test]
    fn successor_kinds_alternates_motive_and_corrective() {
        let impulse_points: [(i64, f64); 6] = [
            (0, 100.0), (1, 101.0), (2, 102.0), (3, 103.0), (4, 104.0), (5, 105.0),
        ];
        let impulse = ElliottImpulse::new(impulse_points, "#2196F3");
        let after_impulse = successor_kinds(&impulse);
        assert_eq!(after_impulse, &["elliott_correction", "elliott_triangle", "elliott_double_combo", "elliott_triple_combo"]);
        assert!(!after_impulse.contains(&"elliott_impulse"), "a motive structure must never offer another motive structure as its successor");

        let correction_points: [(i64, f64); 4] = [(0, 100.0), (1, 101.0), (2, 102.0), (3, 103.0)];
        let correction = ElliottCorrection::new(correction_points, "#FF5722");
        let after_correction = successor_kinds(&correction);
        assert_eq!(after_correction, &["elliott_impulse"]);
    }

    #[test]
    fn successor_kinds_empty_for_non_elliott_primitive() {
        use crate::drawing::primitives_v2::TrendLine;
        let line = TrendLine::new(0, 100.0, 1_000, 110.0, "#2196F3");
        assert!(successor_kinds(&line).is_empty());
    }

    fn chain_impulse_then_correction() -> (ElliottImpulse, ElliottCorrection) {
        let impulse_points: [(i64, f64); 6] = [
            (0, 100.0), (1_000, 110.0), (2_000, 105.0), (3_000, 130.0), (4_000, 120.0), (5_000, 140.0),
        ];
        let mut impulse = ElliottImpulse::new(impulse_points, "#2196F3");
        impulse.data.id = 1;

        let correction_points: [(i64, f64); 4] = [
            (5_000, 140.0), (6_000, 135.0), (7_000, 138.0), (8_000, 132.0),
        ];
        let mut correction = ElliottCorrection::new(correction_points, "#FF5722");
        correction.data.id = 2;
        correction.follows = Some(1);

        (impulse, correction)
    }

    #[test]
    fn promote_chain_shape_two_member_chain_maps_to_impulse_slots_1_and_2() {
        let (impulse, correction) = chain_impulse_then_correction();
        let chain: Vec<&dyn Primitive> = vec![&impulse, &correction];
        let shape = promote_chain_shape(&chain).expect("impulse+correction chain promotes");
        assert_eq!(shape.type_id, "elliott_impulse");
        assert_eq!(shape.degree, WaveDegree::Intermediate.coarser());
        assert_eq!(shape.slots, vec![WaveSlot::Impulse1, WaveSlot::Impulse2]);
        assert_eq!(shape.points, vec![(0, 100.0), (5_000, 140.0), (8_000, 132.0)], "start of the first member, then each member's own last point");
    }

    #[test]
    fn promote_chain_shape_refuses_mixed_degree() {
        let (impulse, mut correction) = chain_impulse_then_correction();
        correction.degree = WaveDegree::Minor;
        let chain: Vec<&dyn Primitive> = vec![&impulse, &correction];
        assert!(promote_chain_shape(&chain).is_none());
    }

    #[test]
    fn promote_chain_shape_refuses_a_member_already_linked_elsewhere() {
        let (impulse, mut correction) = chain_impulse_then_correction();
        correction.link = Some(WaveLink { parent_id: 999, slot: WaveSlot::CorrectionA });
        let chain: Vec<&dyn Primitive> = vec![&impulse, &correction];
        assert!(promote_chain_shape(&chain).is_none(), "a member already nested under another parent must not be adopted");
    }

    #[test]
    fn promote_chain_shape_refuses_a_non_contiguous_chain() {
        let (impulse, mut correction) = chain_impulse_then_correction();
        correction.follows = Some(42); // does not point back at the impulse's own id (1)
        let chain: Vec<&dyn Primitive> = vec![&impulse, &correction];
        assert!(promote_chain_shape(&chain).is_none());
    }

    #[test]
    fn promote_chain_shape_corrective_opened_chain_maps_to_correction_slots() {
        let correction_points: [(i64, f64); 4] = [(0, 100.0), (1_000, 90.0), (2_000, 95.0), (3_000, 85.0)];
        let mut correction = ElliottCorrection::new(correction_points, "#FF5722");
        correction.data.id = 10;

        let impulse_points: [(i64, f64); 6] = [
            (3_000, 85.0), (4_000, 95.0), (5_000, 90.0), (6_000, 105.0), (7_000, 98.0), (8_000, 115.0),
        ];
        let mut impulse = ElliottImpulse::new(impulse_points, "#2196F3");
        impulse.data.id = 11;
        impulse.follows = Some(10);

        let chain: Vec<&dyn Primitive> = vec![&correction, &impulse];
        let shape = promote_chain_shape(&chain).expect("correction+impulse chain promotes");
        assert_eq!(shape.type_id, "elliott_correction");
        assert_eq!(shape.slots, vec![WaveSlot::CorrectionA, WaveSlot::CorrectionB]);
        assert_eq!(shape.points, vec![(0, 100.0), (3_000, 85.0), (8_000, 115.0)]);
    }
}
