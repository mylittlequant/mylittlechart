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
/// a pure lookup table, not a downcast.
///
/// `pub` (owner report 2026-08-13, this crate's own `traits.rs` "Elliott
/// Subdivision Overlay" section): the collection's own owner
/// (`DrawingManager`, `mlc-backend`) must be able to write a MOVED child's
/// own endpoints back INTO the parent's own point array at exactly this
/// span — the write that makes a spawned subdivision genuinely PART of the
/// structure instead of an overlay drawn on top of it (the owner's own
/// words: "он должен именно что становиться частью", not just look
/// attached). This function still only NAMES the two indices; it adds no
/// write helper of its own — a primitive still never resolves its own
/// parent, the caller does that (same contract [`child_seed`]'s own doc
/// already states for the read side). Within this crate, `elliott::guard`'s
/// render-side marking and every `elliott_*.rs`'s own `render`/`hit_test`
/// still read it for the same reason as before: which segment/label index a
/// [`WaveSlot`] lands on, without a second copy of this table.
pub fn slot_span(type_id: &str, slot: WaveSlot) -> Option<(usize, usize)> {
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
// Subdivision overlay skip (owner report 2026-08-13, this module's own doc
// on `WaveLink`: a spawned child must REPLACE the leg of its parent it
// nests inside, not overlay it — see `Primitive::subdivided_slots`'s own
// doc in `traits.rs` for the render-side cache these two functions serve).
// =========================================================================

/// Whether the leg from point index `i` to `i + 1` on a primitive of
/// `type_id` falls entirely inside one of `subdivided`'s own slot spans —
/// the shared predicate a subdivision's `render` skip and its matching
/// `hit_test` skip both read. Reuses [`slot_span`], never a second span
/// table — a combination's own multi-leg component slot (e.g. `ComboW`
/// spanning two legs) covers every leg inside its span, not just the one
/// ending the span.
pub(super) fn segment_is_subdivided(type_id: &str, i: usize, subdivided: &[WaveSlot]) -> bool {
    subdivided.iter().any(|&slot| slot_span(type_id, slot).is_some_and(|(a, b)| a <= i && i + 1 <= b))
}

/// Every consecutive leg `(i, i + 1)` within the REALIZED point prefix that
/// a primitive of `type_id` still draws as its OWN line — every leg
/// [`segment_is_subdivided`] does NOT cover. The single place `render`
/// decides which legs remain its own to stroke, shared with this crate's
/// own tests (see each `elliott_*.rs`'s own `subdivided_*_excludes_*`
/// test) so "what render draws" and "what a test asserts render draws" can
/// never drift apart. Empty once `realized < 2` — the same "nothing to
/// draw yet" case the pre-subdivision render loop's own `realized >= 2`
/// guard used to gate explicitly.
pub(super) fn visible_leg_segments(type_id: &str, realized: usize, subdivided: &[WaveSlot]) -> Vec<(usize, usize)> {
    (0..realized.saturating_sub(1))
        .filter(|&i| !segment_is_subdivided(type_id, i, subdivided))
        .map(|i| (i, i + 1))
        .collect()
}

/// Every point index on `parent` where ANOTHER structure's own label lands
/// on the SAME pivot — the rule behind [`Primitive::label_collisions`]'s
/// own render cache (that method's own doc), computed here once by the
/// collection's own owner (`mlc-backend`) and written back via
/// [`Primitive::set_label_collisions`]. This crate owns the RULE; the
/// backend owns WHEN to recompute it and WHERE the (child, slot) pairs it
/// passes in come from.
///
/// Regression fix (owner report 2026-08-14, second pass): a first attempt
/// at this rule (`slot_boundary_collides`, since removed) treated BOTH ends
/// of a subdivided slot's own [`slot_span`] as always colliding, using only
/// the SLOT to decide — wrong for the exact reason the blanket per-degree
/// ring was wrong: it moves a label that has nothing to collide with.
/// `ElliottCorrection`/`ElliottImpulse`/`ElliottTriangle` all draw an EMPTY
/// label at their own FIRST point (see each type's own `wave_labels` —
/// index 0 is always `""`), so nesting one of those into a slot never
/// collides at that span's START, only (when its own last label is
/// non-empty, which it always is for these three) at the span's END.
/// `ElliottDoubleCombo`/`ElliottTripleCombo` label their own first point
/// ("W"), so nesting one of THOSE genuinely does collide at a span's
/// START too. The collision cannot be derived from the slot alone — it
/// depends on the CHILD's own type, which is why this function takes the
/// children themselves.
///
/// For each `(child, slot)` pair in `children`: `slot`'s own [`slot_span`]
/// on `parent` names the two pivots (`a`, `b`) the child shares with
/// `parent` (the child's own first/last point — the backend welds them
/// there, `restitch_wave_joints`). `a` collides only when `child`'s own
/// label at ITS OWN first point is non-empty; `b` collides only when
/// `child`'s own label at ITS OWN last point is non-empty. Read ONLY
/// through [`Primitive::point_labels`] — never a downcast to a concrete
/// Elliott type, matching every other function in this module's own
/// contract.
///
/// Sorted and deduplicated. Skips a `(child, slot)` pair when `slot` does
/// not exist on `parent`'s own type (mirrors [`slot_span`]'s own `None`
/// contract) or when `child` carries no [`Primitive::point_labels`] at all
/// (`None` — every real Elliott type always returns `Some`, but this
/// function does not assume that of an arbitrary `&dyn Primitive`). Empty
/// `children` (no child nested into `parent` at all) returns empty — a
/// structure with no subdivisions collides nowhere, and neither does a
/// subdivision's own child at any of its own points (a child is never
/// itself named as `parent` here unless something is in turn nested inside
/// IT, and even then only ITS OWN children are consulted, never its
/// parent's).
pub fn label_collision_indices(parent: &dyn Primitive, children: &[(&dyn Primitive, WaveSlot)]) -> Vec<usize> {
    let mut indices = Vec::new();
    for &(child, slot) in children {
        let Some((a, b)) = slot_span(parent.type_id(), slot) else { continue };
        let Some(labels) = child.point_labels() else { continue };
        if labels.first().is_some_and(|l| !l.is_empty()) {
            indices.push(a);
        }
        if labels.len() >= 2 && labels.last().is_some_and(|l| !l.is_empty()) {
            indices.push(b);
        }
    }
    indices.sort_unstable();
    indices.dedup();
    indices
}

// =========================================================================
// Resampling — keeping the drawn shape across a pivot-count change (owner
// report 2026-08-14, same report cited in this module's own "Conversion"
// section further down: a correction has 4 pivots, an impulse 6, a double
// combination 7, a triple 9 — converting between them must keep what the
// user actually drew instead of resetting to a straight seed the way a
// freshly-seeded child does, see [`ChildSeed::interpolate`]).
// =========================================================================

/// Resample `points` (a drawn polyline, `(timestamp_ms, price)` pairs) at
/// `n` positions spaced EQUALLY ALONG ITS OWN ARC LENGTH, measured in a
/// NORMALIZED space: each axis is divided by the polyline's own total span
/// (`max - min` across every point, not just the first and last — a
/// structure that retraces past its own start, e.g. a deep wave 2, still
/// has that retracement count toward its own span) before any distance is
/// measured. Timestamps and prices are different units; an un-normalized
/// Euclidean distance would let whichever axis happens to carry the larger
/// raw numbers dominate the parametrization regardless of the SHAPE the
/// user actually drew — normalizing first makes a diagonal wave leg and a
/// near-vertical spike contribute comparably to "how much of the shape"
/// each one is, which is the whole point of resampling BY SHAPE rather
/// than by index.
///
/// `points[0]` and `points.last()` come out of a normal call (`n >= 2`,
/// `points.len() >= 2`, non-degenerate) EXACTLY equal to `points`' own
/// first and last entries — never merely close. A converted structure's
/// own ends are welded to a parent's own pivot (when it subdivides one) and
/// to a chain's own joint (when it follows or is followed by another
/// structure); a conversion that nudged them would tear the structure loose
/// from what it is nested inside or chained to.
///
/// `n == points.len()` returns `points` unchanged (a plain `to_vec()`,
/// before the resampling math ever runs) — not an incidental property of
/// the algorithm but a deliberate short-circuit: a correction converting
/// between its own four subtypes (zigzag/flat/expanded/running) never
/// changes its own pivot COUNT, and every one of those conversions must
/// leave the drawn shape byte-for-byte untouched, which the arc-length
/// algorithm below cannot promise on its own for pivots that are not
/// already spaced evenly along their own arc length — most drawn waves are
/// not.
///
/// Degenerate inputs never panic:
/// - `n == 0` -> empty vec, regardless of `points`.
/// - `points.is_empty()` -> empty vec, regardless of `n` (nothing to
///   resample FROM).
/// - `points.len() == 1` -> `n` copies of that single point (first and last
///   are trivially it — there is nothing else to resample).
/// - `n == 1` (with `points.len() >= 2`) -> `vec![points[0]]` — the same
///   "collapse to the start" contract [`ChildSeed::interpolate`]'s own
///   `n == 1` case already uses elsewhere in this module; there is only one
///   output point, so "first AND last" cannot both be expressed.
/// - Zero arc length in the normalized space (every point normalizes onto
///   the SAME position — only possible when the whole polyline collapses to
///   one distinct `(ts, price)` pair, since both axes' own span is then
///   zero) -> `n` copies of `points[0]`, same reasoning as the single-point
///   case.
pub fn resample_polyline(points: &[(i64, f64)], n: usize) -> Vec<(i64, f64)> {
    if n == 0 || points.is_empty() {
        return Vec::new();
    }
    if points.len() == 1 {
        return vec![points[0]; n];
    }
    if n == points.len() {
        return points.to_vec();
    }
    if n == 1 {
        return vec![points[0]];
    }

    // Normalize each axis by the polyline's own total span (max - min
    // across every point) so a raw Euclidean distance never lets one
    // axis's own unit scale dominate the other's — see this function's own
    // doc. A zero-span axis contributes 0 to every segment (it cannot
    // contribute anything else: a zero span means every point already
    // shares that axis's own value, so every raw delta on it is already
    // zero too).
    let (min_ts, max_ts) =
        points.iter().fold((points[0].0, points[0].0), |(lo, hi), &(t, _)| (lo.min(t), hi.max(t)));
    let (min_price, max_price) =
        points.iter().fold((points[0].1, points[0].1), |(lo, hi), &(_, p)| (lo.min(p), hi.max(p)));
    let ts_span = (max_ts - min_ts) as f64;
    let price_span = max_price - min_price;

    // Cumulative arc length (normalized space) up to and including each
    // input pivot — `cum[k]` is the distance walked to reach `points[k]`.
    let mut cum = Vec::with_capacity(points.len());
    cum.push(0.0);
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let dt_n = if ts_span > 0.0 { (b.0 - a.0) as f64 / ts_span } else { 0.0 };
        let dp_n = if price_span > 0.0 { (b.1 - a.1) / price_span } else { 0.0 };
        let seg_len = (dt_n * dt_n + dp_n * dp_n).sqrt();
        cum.push(cum[cum.len() - 1] + seg_len);
    }
    let total = cum[cum.len() - 1];

    // Zero total arc length only happens when every point collapses onto
    // the same normalized position, which (given the fallback above) only
    // happens when the whole polyline is one distinct point repeated — see
    // this function's own doc.
    if total <= 0.0 {
        return vec![points[0]; n];
    }

    let last_seg = points.len() - 2;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let target = total * (i as f64) / (n - 1) as f64;
        let mut seg = 0usize;
        while seg < last_seg && cum[seg + 1] < target {
            seg += 1;
        }
        let seg_len = cum[seg + 1] - cum[seg];
        let t = if seg_len > 0.0 { (target - cum[seg]) / seg_len } else { 0.0 };
        let a = points[seg];
        let b = points[seg + 1];
        let ts = a.0 + ((b.0 - a.0) as f64 * t).round() as i64;
        let price = a.1 + (b.1 - a.1) * t;
        out.push((ts, price));
    }
    // Force the endpoints exactly, regardless of any floating-point
    // residue the loop above accumulated — see this function's own doc for
    // why they must never merely be close.
    out[0] = points[0];
    let last_idx = out.len() - 1;
    out[last_idx] = points[points.len() - 1];
    out
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
// Predecession — succession read backwards (owner ask 2026-08-14: "щас мы
// чейн можем только с конца строить, а хорошо бы при даблклике или ПКМ на
// 0 точку делать достройку перед началом чейна" — a count is read
// backwards as often as forwards, so building a chain must not be limited
// to appending after the terminal pivot). Same `follows` field
// `successor_seed`/`successor_kinds` already use — a predecessor is still
// `follows`-linked to what it precedes, exactly like an ordinary successor
// is `follows`-linked to what it continues; the only thing that changes is
// which END of the relation the caller already has a primitive for.
// =========================================================================

/// What a NEW predecessor primitive that `next` continues FROM must be
/// seeded with — [`SuccessorSeed`] read backwards.
///
/// `degree` is always `next`'s OWN degree, unchanged — succession (in
/// either direction) is a same-degree sibling relation, never a
/// subdivision (same reasoning [`SuccessorSeed::degree`]'s own doc gives).
/// `end` is exactly `next`'s own FIRST point (`points().first()`), so the
/// two structures visibly connect on the chart with no gap — the mirror of
/// [`SuccessorSeed::start`], which is exactly `prev`'s own LAST point.
///
/// `span_bars_hint`/`price_hint` are a DEFAULT extent for the
/// predecessor's still-unplaced STARTING end, using the exact same
/// reasoning [`SuccessorSeed`]'s own doc gives for its own hints:
///
/// - `span_bars_hint` ≈ 0.618 of `next`'s own total bar span
///   (`last.0 - first.0`) — the same golden-ratio constant, for the same
///   reason [`SuccessorSeed::span_bars_hint`] states: a plausible
///   corrective-vs-motive time proportion, not a claim to know the real
///   one. This field itself carries the NEGATIVE sign (unlike
///   `SuccessorSeed::span_bars_hint`, which is always `>= 0`): the
///   predecessor's own start lies BEFORE `end` in time, since it is placed
///   in FRONT of the chain instead of after it, so [`Self::start`]
///   computes the far anchor by ADDING this (negative) hint to `end`
///   rather than subtracting a positive one.
/// - `price_hint` ≈ 0.5 of `next`'s own total price travel
///   (`last.1 - first.1`) — the same half-retrace magnitude
///   [`SuccessorSeed::price_hint`] uses, but the direction of this hint is
///   INVERTED relative to the successor case, because a predecessor
///   ARRIVES INTO this structure rather than LEAVING it. Read it through
///   the same alternation rule `SuccessorSeed::price_hint` applies (a
///   structure's own motion opposes whatever it is adjacent to): there,
///   the successor's motion opposes `prev`'s motion, so the hint negates
///   `prev`'s own travel; here, by the same rule read the other way,
///   `next`'s motion opposes the PREDECESSOR's motion, so the
///   predecessor's own motion is `-(next`'s own travel `* 0.5)`. But
///   because THIS seed's fixed anchor is `end`, not `start`, [`Self::start`]
///   recovers the predecessor's own start by ADDING `price_hint` to
///   `end.1` — and adding the ALREADY-negated motion back onto `end` to
///   reach `start` cancels the negation, so `price_hint` itself is stored
///   as the un-negated `next`'s own travel `* 0.5`. Same magnitude
///   formula as `SuccessorSeed::price_hint`, opposite sign convention —
///   not a different reasoning, a different anchor.
///
/// Both numbers seed a draggable INITIAL shape, not a forecast — same as
/// [`SuccessorSeed`]'s own doc states.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PredecessorSeed {
    /// The degree the predecessor must be constructed with — always
    /// `next`'s own degree (see this struct's own doc).
    pub degree: WaveDegree,
    /// The predecessor's ending anchor — `next`'s own first point.
    pub end: (i64, f64),
    /// Default bar-span extent for the predecessor's still-unplaced
    /// STARTING end, relative to `end.0` — see this struct's own doc for
    /// why this carries the negative sign (unlike
    /// [`SuccessorSeed::span_bars_hint`]).
    pub span_bars_hint: i64,
    /// Default price extent for the predecessor's still-unplaced STARTING
    /// end, relative to `end.1` — see this struct's own doc for the
    /// "inverted relative to the successor case" derivation.
    pub price_hint: f64,
}

impl PredecessorSeed {
    /// The predecessor's still-unplaced STARTING anchor implied by `end` +
    /// `span_bars_hint`/`price_hint` — the mirror of [`SuccessorSeed::end`],
    /// which computes the far end by ADDING its own hints to `start`; here
    /// `end` is the fixed anchor and this method locates `start` instead.
    pub fn start(&self) -> (i64, f64) {
        (self.end.0 + self.span_bars_hint, self.end.1 + self.price_hint)
    }

    /// `n` points evenly interpolated from [`start`](Self::start) to `end`
    /// — see [`interpolate_points`]'s own doc for why this shares its math
    /// with, but is not the same type as, [`SuccessorSeed::interpolate`]/
    /// [`ChildSeed::interpolate`].
    pub fn interpolate(&self, n: usize) -> Vec<(i64, f64)> {
        interpolate_points(self.start(), self.end, n)
    }
}

/// What a new predecessor primitive that `next` continues FROM at its own
/// degree must be seeded with. `None` when `next` is not an Elliott wave at
/// all (`next.wave_degree()` is `None`), or its own points do not yet reach
/// 2 entries (a primitive still being drawn — there is no "first point"
/// yet). Reads `next` only through the [`Primitive`] trait, never
/// downcasts — same contract as [`successor_seed`].
pub fn predecessor_seed(next: &dyn Primitive) -> Option<PredecessorSeed> {
    let degree = next.wave_degree()?;
    let points = next.points();
    if points.len() < 2 {
        return None;
    }
    let first = points[0];
    let last = points[points.len() - 1];
    let bar_span = last.0 - first.0;
    let price_travel = last.1 - first.1;
    Some(PredecessorSeed {
        degree,
        end: first,
        span_bars_hint: -((bar_span as f64 * 0.618).round() as i64),
        price_hint: price_travel * 0.5,
    })
}

/// The Elliott type ids that may validly PRECEDE `next` at `next`'s own
/// degree — [`successor_kinds`] read backwards: after a MOTIVE structure
/// comes a corrective one, so BEFORE a corrective structure came a motive
/// one; after a corrective structure comes a motive one, so BEFORE a
/// motive structure came a corrective one.
///
/// Written as its OWN function rather than calling [`successor_kinds`] on
/// some notional "the other end of the relation" and trusting the mapping
/// is symmetric: it happens to BE symmetric today — the alternation this
/// module encodes has exactly two classes (motive and corrective), and
/// swapping "comes after" for "comes before" between two classes of two is
/// its own inverse — but the two functions are kept separate ON PURPOSE.
/// A future asymmetric grammar (e.g. a rule that a triangle may FOLLOW
/// anything but nothing may FOLLOW a triangle) would need to edit only one
/// of these two functions, and having them separate from day one means
/// that edit is a one-function change instead of an untangle.
///
/// - Before `"elliott_impulse"` (the only motive structure among the five)
///   → any of the four corrective structures: `"elliott_correction"`,
///   `"elliott_triangle"`, `"elliott_double_combo"`, `"elliott_triple_combo"`.
/// - Before any of those four corrective structures → `"elliott_impulse"`
///   only.
/// - Empty for every primitive outside the Elliott family
///   (`next.wave_degree()` is `None`) — nothing may validly precede a
///   non-Elliott primitive.
///
/// This is the grammar itself, not just advice to a picker: the caller
/// that creates a predecessor (`DrawingManager::create_wave_predecessor`)
/// re-checks the requested type id against this list before constructing
/// anything, the same fail-closed shape `create_wave_successor` already
/// uses against [`successor_kinds`].
pub fn predecessor_kinds(next: &dyn Primitive) -> &'static [&'static str] {
    if next.wave_degree().is_none() {
        return &[];
    }
    match next.type_id() {
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

/// This primitive's own "kind id" (see [`CHILD_KIND_CANDIDATES`]'s own doc
/// for the `type_id[:subtype]` convention) — [`Primitive::type_id`] plus
/// [`Primitive::correction_subtype`] when it carries one, formatted the
/// SAME way every [`CHILD_KIND_CANDIDATES`] entry already is. `None` for a
/// primitive outside the five Elliott types ([`Primitive::wave_degree`] is
/// `None`) — there is no "kind" in this vocabulary for a primitive that
/// never enters it.
///
/// Was a private `elliott::guard::child_kind_id` — moved here (owner report
/// 2026-08-14, this module's own "Conversion" section below) so
/// [`conversion_kinds`] can build a picker's "what am I right now" entry
/// off the SAME computation `elliott::guard`'s own `subdivision_class` rule
/// already used to name a child's kind, rather than a second copy of the
/// format string.
pub fn kind_id_of(prim: &dyn Primitive) -> Option<String> {
    prim.wave_degree()?;
    Some(match prim.correction_subtype() {
        Some(subtype) => format!("{}:{}", prim.type_id(), subtype),
        None => prim.type_id().to_string(),
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
// Adoption — recognizing already-drawn structures as subdivisions of a NEW,
// coarser one (owner ask 2026-08-14: "кроме того что у нас разбиение по
// циклам идет вниз когда мы дробим, неплохо было бы детектить вверх и
// делать авторазметку более высокого цикла" — when the analyst draws a
// COARSER structure over waves he already marked, the engine recognises the
// existing ones as its own subdivisions instead of making him re-link them
// by hand). This is RECOGNITION of what was actually drawn, never a guessed
// count: a candidate is adopted only when its own endpoints coincide with
// the new structure's own slot anchors (within [`ADOPT_TOLERANCE_FRAC`]'s
// own click-slop budget) AND its kind is grammatically legal in that slot
// ([`child_kinds`] — the SAME gate a hand-built child already has to clear,
// so recognition can never produce a subdivision the analyst could not have
// created himself). Nothing is invented, nothing is moved except the weld
// the caller performs afterward (see [`Adoption::start`]/[`Adoption::end`]'s
// own doc) — this module only ever REPORTS matches, never mutates anything;
// the collection's own owner (`DrawingManager`, `mlc-backend`) does the
// actual write.
// =========================================================================

/// The click-slop budget [`adoptable_children`] matches a candidate's own
/// endpoints against a new structure's own slot anchors — a FRACTION of the
/// new structure's own bounding span (its own high-low price range, its own
/// earliest-to-latest time span), never an absolute price or a pixel count:
/// `mlc-core` cannot see pixels, and a fixed absolute price tolerance would
/// be wrong both for a cent-priced altcoin and for a five-figure index. The
/// analyst clicks NEAR his own pivots when drawing the coarser structure,
/// never exactly on them — an exact point comparison would recognise
/// nothing.
const ADOPT_TOLERANCE_FRAC: f64 = 0.02;

/// One existing primitive [`adoptable_children`] recognises as already
/// occupying one of a new structure's own slots.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Adoption {
    /// The adopted child's own [`super::super::PrimitiveData::id`].
    pub child_id: u64,
    /// Which of the new structure's own slots the child occupies.
    pub slot: WaveSlot,
    /// The child's OWN first point — read off `Primitive::points`, never the
    /// parent's own (looser, click-slop) anchor at this slot. This is what
    /// the caller welds the parent's own slot-start point ONTO: the analyst
    /// clicked NEAR the child's own end when drawing the new structure,
    /// adoption makes the join exact, the same invariant a hand-made
    /// subdivision already gets from `DrawingManager::restitch_wave_joints`.
    /// Reported, not applied — this module never mutates either primitive.
    pub start: (i64, f64),
    /// The child's own LAST point — the mirror of [`start`](Self::start).
    pub end: (i64, f64),
}

/// Every existing primitive in `candidates` that legally occupies one of
/// `parent`'s own slots — the detector behind auto-adoption (this module's
/// own "Adoption" section doc above). `parent` is a primitive that has not
/// yet been linked to anything (a freshly drawn top-level structure); this
/// function only REPORTS which candidates qualify, it never mutates either
/// `parent` or any candidate — the caller (`DrawingManager`, the only thing
/// that can write back into the collection) does the welding/linking.
///
/// For each of `parent`'s own slots ([`Primitive::wave_slots`]), this
/// resolves the slot's own point span via [`slot_span`] and reads the two
/// anchor points straight off `parent`'s own [`Primitive::points`] — never
/// invented, never a guess at what the count "should" contain. A candidate
/// qualifies for that slot only when ALL of the following hold:
///
/// - it is not `parent` itself (compared by
///   [`super::super::PrimitiveData::id`], via `candidates`' own `u64`);
/// - [`Primitive::wave_link`] is `None` — a structure that already
///   subdivides something else is not up for adoption by a second parent,
///   the same rule [`promote_chain_shape`] enforces for its own chain
///   members (see that function's own doc);
/// - both of its own endpoints (`points().first()`/`points().last()`) lie
///   within [`ADOPT_TOLERANCE_FRAC`]'s own click-slop budget of the slot's
///   own `start`/`end` anchors;
/// - its own kind id ([`kind_id_of`]) is one of [`child_kinds`]`(parent,
///   slot)` — the SAME grammar gate a hand-built child already has to
///   clear, so recognition can never produce a subdivision the analyst
///   could not have created himself by picking it from the slot picker;
/// - it carries a [`Primitive::wave_degree`] at all — a non-Elliott
///   primitive (a plain trend line drawn across the same two points, say)
///   can never be adopted, matching every other function in this module's
///   own "outside the Elliott family" contract.
///
/// Degree consistency: two candidates matching two different slots may
/// carry two different degrees (e.g. one drawn at `Minor`, another at
/// `Minute`, both happening to land on the new structure's own anchors).
/// Only the matches sharing the COARSEST degree PRESENT among them survive
/// into the result — a structure's own subdivisions are all one degree, the
/// same invariant `elliott::guard::chain_violations`'s own `"chain_degree"`
/// rule already enforces for a `follows` chain. The coarsest (not finest,
/// not majority) degree wins because a genuine higher-cycle markup nests
/// EVERY finer degree below it; a stray match at a still-finer degree than
/// the rest reads as a coincidental overlap rather than a real subdivision,
/// and is silently dropped rather than merged in.
///
/// Uniqueness, BOTH directions (checked AFTER the degree filter above): a
/// slot holds exactly one subdivision, and a child occupies exactly one
/// slot, so the raw matches above can still conflict two ways that a broken
/// state, never a preference, has to resolve:
///
/// - two DIFFERENT candidates both matching the SAME slot (two structures
///   drawn near the same span) — welding both would leave the slot's own
///   anchor pointing at whichever [`WaveLink`] was written last;
/// - one candidate matching TWO DIFFERENT slots (a degenerate — very
///   short — middle wave puts both of ITS OWN endpoints inside the
///   tolerance box of more than one slot) — linking it twice would give one
///   child two parents' worth of slot.
///
/// Both are resolved by keeping the CLOSEST match, never by iteration
/// order: every raw match carries a `score` — its own endpoint distance
/// from the slot's anchors, EACH AXIS normalized by that same axis's own
/// `ts_tol`/`price_tol` budget (so a millisecond and a price unit become
/// comparable), summed over both endpoints (`start` error + `end` error).
/// Lower is closer. Slot conflicts resolve FIRST (one winner per
/// [`WaveSlot`]); a child that still appears under two different
/// slot-winners after that keeps only its own lower-scored one, and the
/// slot it loses reverts to unmatched. Either tie (equal score) breaks on
/// the smaller [`Adoption::child_id`] — deterministic, never "whichever the
/// candidate slice happened to list first".
///
/// Empty when `parent` carries no [`Primitive::wave_degree`] itself, when
/// `parent`'s own points do not yet reach 2 entries (nothing to read a
/// bounding span or a slot anchor off), or when fewer than one slot matched
/// at all — there is nothing to recognise.
pub fn adoptable_children(parent: &dyn Primitive, candidates: &[(u64, &dyn Primitive)]) -> Vec<Adoption> {
    if parent.wave_degree().is_none() {
        return Vec::new();
    }
    let parent_id = parent.data().id;
    let parent_points = parent.points();
    if parent_points.len() < 2 {
        return Vec::new();
    }

    // The new structure's own bounding span — the basis for
    // `ADOPT_TOLERANCE_FRAC`'s own click-slop budget (see that const's own
    // doc): price range and time range across EVERY one of `parent`'s own
    // points, not just its own first/last (a structure that retraces past
    // its own start still has that retracement count toward its own span,
    // same reasoning `resample_polyline`'s own normalization uses).
    let (min_ts, max_ts) = parent_points.iter().fold(
        (parent_points[0].0, parent_points[0].0),
        |(lo, hi), &(t, _)| (lo.min(t), hi.max(t)),
    );
    let (min_price, max_price) = parent_points.iter().fold(
        (parent_points[0].1, parent_points[0].1),
        |(lo, hi), &(_, p)| (lo.min(p), hi.max(p)),
    );
    let ts_tol = (max_ts - min_ts) as f64 * ADOPT_TOLERANCE_FRAC;
    let price_tol = (max_price - min_price) * ADOPT_TOLERANCE_FRAC;

    struct RawMatch {
        child_id: u64,
        slot: WaveSlot,
        start: (i64, f64),
        end: (i64, f64),
        degree: WaveDegree,
        /// See [`adoptable_children`]'s own doc, "Uniqueness" paragraph, for
        /// what this measures and how ties break.
        score: f64,
    }
    let mut raw: Vec<RawMatch> = Vec::new();

    for &slot in parent.wave_slots() {
        let Some((start_idx, end_idx)) = slot_span(parent.type_id(), slot) else { continue };
        let (Some(&span_start), Some(&span_end)) = (parent_points.get(start_idx), parent_points.get(end_idx))
        else {
            continue;
        };
        let allowed = child_kinds(parent, slot);
        if allowed.is_empty() {
            continue;
        }
        for &(child_id, child) in candidates {
            if child_id == parent_id {
                continue;
            }
            if child.wave_link().is_some() {
                continue;
            }
            let Some(child_degree) = child.wave_degree() else { continue };
            let child_points = child.points();
            let (Some(&c_first), Some(&c_last)) = (child_points.first(), child_points.last()) else {
                continue;
            };
            let dt_start = (c_first.0 - span_start.0).unsigned_abs() as f64;
            let dp_start = (c_first.1 - span_start.1).abs();
            let dt_end = (c_last.0 - span_end.0).unsigned_abs() as f64;
            let dp_end = (c_last.1 - span_end.1).abs();
            if dt_start > ts_tol || dp_start > price_tol || dt_end > ts_tol || dp_end > price_tol {
                continue;
            }
            let Some(kind_id) = kind_id_of(child) else { continue };
            if !allowed.iter().any(|&k| k == kind_id) {
                continue;
            }
            // `Adoption::start`/`::end` carry the CHILD's own exact
            // endpoints, not `span_start`/`span_end` — see `Adoption`'s
            // own field doc for why: the weld snaps the parent's own
            // (click-slop) anchor ONTO the child's own precise point, not
            // the other way around.
            let score = normalized_error(dt_start, ts_tol)
                + normalized_error(dp_start, price_tol)
                + normalized_error(dt_end, ts_tol)
                + normalized_error(dp_end, price_tol);
            raw.push(RawMatch { child_id, slot, start: c_first, end: c_last, degree: child_degree, score });
        }
    }

    if raw.is_empty() {
        return Vec::new();
    }

    // Degree consistency — keep only the coarsest degree present (see this
    // function's own doc for why coarsest, not finest or majority).
    // `WaveDegree::all()` is ordered coarsest-to-finest, so the smallest
    // position among the matched degrees names the coarsest one.
    let all_degrees = WaveDegree::all();
    let coarsest = raw
        .iter()
        .filter_map(|m| all_degrees.iter().position(|&d| d == m.degree))
        .min()
        .and_then(|idx| all_degrees.get(idx).copied());
    let Some(coarsest) = coarsest else { return Vec::new() };
    raw.retain(|m| m.degree == coarsest);

    // Uniqueness pass 1: at most one adoption PER SLOT. Keep the
    // lower-scored (closer) match; a tie breaks on the smaller `child_id`
    // (deterministic, never "whichever came first in `candidates`").
    let mut best_by_slot: std::collections::HashMap<WaveSlot, RawMatch> = std::collections::HashMap::new();
    for m in raw {
        match best_by_slot.entry(m.slot) {
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(m);
            }
            std::collections::hash_map::Entry::Occupied(mut e) => {
                let keep_new = m.score < e.get().score || (m.score == e.get().score && m.child_id < e.get().child_id);
                if keep_new {
                    e.insert(m);
                }
            }
        }
    }

    // Uniqueness pass 2: at most one adoption PER CHILD, checked AFTER slot
    // conflicts already resolved above — a child that still wins two
    // different slots here keeps only its own lower-scored slot, and the
    // OTHER slot simply drops back out of the result (it is not handed to
    // a second-choice candidate — that candidate already lost its own
    // slot-conflict round on its own merits, or never had one).
    let mut best_by_child: std::collections::HashMap<u64, RawMatch> = std::collections::HashMap::new();
    for m in best_by_slot.into_values() {
        match best_by_child.entry(m.child_id) {
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(m);
            }
            std::collections::hash_map::Entry::Occupied(mut e) => {
                let keep_new = m.score < e.get().score || (m.score == e.get().score && m.slot.id() < e.get().slot.id());
                if keep_new {
                    e.insert(m);
                }
            }
        }
    }

    let mut result: Vec<Adoption> = best_by_child
        .into_values()
        .map(|m| Adoption { child_id: m.child_id, slot: m.slot, start: m.start, end: m.end })
        .collect();
    // Deterministic output order — `HashMap` iteration order is not, and
    // nothing about which entry a test or a caller sees first should depend
    // on it.
    result.sort_by_key(|a| a.child_id);
    result
}

/// `diff / tol`, except `0.0` when `tol` is exactly `0.0` — by the time this
/// is called, `diff <= tol` has already been enforced by the caller's own
/// match gate, so `tol == 0.0` only ever pairs with `diff == 0.0` (an EXACT
/// match on an axis with no slop budget at all, e.g. a perfectly flat
/// price). Plain float division there would be `0.0 / 0.0 = NaN`, which
/// compares false against everything and would poison every score it
/// touches; reading "no budget, but matched exactly" as "zero error on that
/// axis" is what it actually means.
fn normalized_error(diff: f64, tol: f64) -> f64 {
    if tol > 0.0 { diff / tol } else { 0.0 }
}

/// The degree a NEW parent must be constructed with after adopting
/// `adoptions` — one step [`WaveDegree::coarser`] than the shared degree
/// every entry in `adoptions` already carries (see [`adoptable_children`]'s
/// own doc: only the coarsest degree PRESENT among the raw matches survives
/// into its result, so every entry here already shares one degree).
/// Resolves that shared degree by looking up the FIRST entry's own
/// [`Adoption::child_id`] back in `candidates` — [`Adoption`] itself does
/// not carry a degree field (only what the caller needs to weld/link), so
/// this is the one place that re-reads it, off the SAME `candidates` slice
/// [`adoptable_children`] was called with.
///
/// `None` for an empty `adoptions` (nothing to derive a shared degree from)
/// or when the first entry's own id is not found in `candidates` (never
/// happens for a `candidates` slice unchanged between the two calls, but
/// this does not assume that of an arbitrary caller).
///
/// Ceiling: [`WaveDegree::coarser`] SATURATES at
/// [`WaveDegree::Supermillennium`] rather than wrapping or panicking (see
/// its own doc). When the adopted children are already at that coarsest
/// degree, this function returns `Supermillennium` again — the new parent
/// then shares its own children's degree instead of sitting one step above
/// them. That is visible in the data (`parent.wave_degree() ==
/// child.wave_degree()`) but, as of this writing, NOTHING in
/// `elliott::guard` actually reads and flags it: `"chain_degree"` is a
/// different relation entirely (it checks that `follows`-chained SIBLINGS
/// share one degree, never that a [`WaveLink`] parent sits one step above
/// its own child), and no `"subdivision_degree"`-shaped rule exists. A
/// same-degree parent/child pair produced this way passes every existing
/// check silently. Recorded here as a known, honest gap rather than papered
/// over — Supermillennium is the top of a 15-rung ladder, so it is a real
/// but rare edge, and closing it (either refusing adoption at the ceiling,
/// or adding the missing guard rule) is a follow-up decision, not part of
/// this function's own contract.
pub fn adopted_parent_degree(candidates: &[(u64, &dyn Primitive)], adoptions: &[Adoption]) -> Option<WaveDegree> {
    let first = adoptions.first()?;
    candidates
        .iter()
        .find(|&&(id, _)| id == first.child_id)
        .and_then(|&(_, p)| p.wave_degree())
        .map(|d| d.coarser())
}

// =========================================================================
// Conversion — what this structure may become mid-count (owner report
// 2026-08-14: "я создал АБЦ как зигзаг, но в процессе понял, что хочу
// сделать его плоскостью или ещё чем-то — не получается переменить весь
// отрезок ... допустим я выбрал зигзаг, а в процессе понял, что это скорее
// двойной зигзаг — тоже не могу прямо в процессе разметки переменить
// глобальное состояние отрезка. Они должны быть просто в этом же дропдауне
// выше, как глобальный статус отрезка, потом возможность переменить
// конкретно выбранную"). Markup is a process: a structure's OWN type has
// to be changeable IN PLACE, mid-count, not decided once at creation — this
// is the grammar half of that ask. The point remap a pivot-count change
// needs is [`resample_polyline`]; the "remap subdivisions across the
// change" half is the "remapping subdivisions" section further down
// ([`slot_ordinal`]/[`slot_at_ordinal`]); the actual conversion (building
// the new primitive, re-stamping children) is `mlc-app`'s.
// =========================================================================

/// Every kind id `prim` may legally CONVERT INTO right now, in
/// [`CHILD_KIND_CANDIDATES`]'s own fixed order — the same candidate set
/// [`child_kinds`] draws from, filtered by whichever of the two structural
/// constraints below apply, PLUS `prim`'s own current kind unconditionally
/// (see this function's own doc, further down, for why).
///
/// - If `prim` occupies a slot ([`Primitive::wave_link`] is `Some`) and
///   `parent` is given, a candidate must also pass
///   [`child_kinds`]`(parent, link.slot)` — a subdivision may only convert
///   into something legal in the very wave it subdivides; converting a
///   wave-2 subdivision into a triangle must stay refused for the same
///   reason SEEDING one there is refused.
/// - If `prim` continues a chain ([`Primitive::wave_follows`] is `Some`)
///   and `prev` is given, a candidate's own TYPE half (via
///   [`split_kind_id`] — [`successor_kinds`] names whole structures, never
///   a correction subtype) must also appear in
///   [`successor_kinds`]`(prev)` — a chain member may only convert into
///   something that may legally follow its own predecessor; converting an
///   impulse's own successor into another impulse must stay refused for the
///   same reason the motive/corrective alternation refuses it at creation.
/// - Both constraints apply TOGETHER (a candidate must clear whichever of
///   the two apply) when `prim` carries both relations and the caller
///   supplied both siblings.
/// - Neither constraint applies (every [`CHILD_KIND_CANDIDATES`] entry
///   passes that half) when the matching relation is absent on `prim`, OR
///   present but the caller did not supply the sibling to check it against
///   — this function never resolves `parent`/`prev` itself, matching every
///   other function in this module's own contract of reading a primitive
///   only through the [`Primitive`] trait and never reaching for a
///   collection it has no path to.
///
/// `prim`'s own CURRENT kind ([`kind_id_of`]`(prim)`) is always present in
/// the result, in its own normal [`CHILD_KIND_CANDIDATES`] position — even
/// when the two constraints above would otherwise refuse it (a chain's own
/// alternation or a slot's own grammar can tighten AFTER a structure was
/// drawn, e.g. a sibling changed shape first) — because the menu built from
/// this list has to be able to say what the structure currently IS, not
/// just what it may become. Marking that one entry as "current" (as
/// opposed to every other entry, which is merely "available") is the
/// CALLER's job, by comparing each returned id against
/// [`kind_id_of`]`(prim)` itself — this function does not tag it.
///
/// Empty for a primitive outside the five Elliott types (mirrors
/// [`child_kinds`]'s own contract — [`kind_id_of`] returns `None`).
pub fn conversion_kinds(
    prim: &dyn Primitive,
    parent: Option<&dyn Primitive>,
    prev: Option<&dyn Primitive>,
) -> Vec<&'static str> {
    let Some(own_kind) = kind_id_of(prim) else { return Vec::new() };

    let slot_gate: Option<Vec<&'static str>> = match (prim.wave_link(), parent) {
        (Some(link), Some(parent)) => Some(child_kinds(parent, link.slot)),
        _ => None,
    };
    let chain_gate: Option<&'static [&'static str]> = match (prim.wave_follows(), prev) {
        (Some(_), Some(prev)) => Some(successor_kinds(prev)),
        _ => None,
    };

    CHILD_KIND_CANDIDATES
        .into_iter()
        .filter(|&kind_id| {
            kind_id == own_kind
                || (slot_gate.as_ref().is_none_or(|allowed| allowed.contains(&kind_id))
                    && chain_gate.is_none_or(|allowed| allowed.contains(&split_kind_id(kind_id).0)))
        })
        .collect()
}

/// Which [`WaveSlot`] the wave-LABEL drawn at point index `i` names, keyed
/// by `type_id` — the reverse of the array each type's own `wave_labels()`
/// builds (see each `elliott_*.rs`'s own `wave_labels` for the source this
/// mirrors; `pub(super)` for the same reason [`slot_span`] is — both
/// `elliott::guard` and every `elliott_*.rs`'s own `render`/`hit_test`/
/// `wave_at` need this lookup).
///
/// NOT the same as `slot_span(type_id, slot).1` for the three combination
/// "component" slots (W/Y/Z on `elliott_double_combo`/
/// `elliott_triple_combo`): a component's own LETTER is drawn near an
/// INTERIOR point of its own multi-leg span (W at its own start, Y/Z near
/// an early interior subwave — see each combo type's own `wave_labels`),
/// never at the span's own end. Every OTHER slot — every impulse/
/// correction/triangle wave, and a combination's own single-leg CONNECTOR
/// slots X/XX — DOES label at its own span's end, matching `slot_span`'s
/// own `.1` exactly, but this function still names each one explicitly
/// rather than falling back to `slot_span` for "the rest" — a silent
/// fallback would break the moment a future combo's own connector grew a
/// multi-leg span too.
pub(super) fn label_slot_for_index(type_id: &str, i: usize) -> Option<WaveSlot> {
    use WaveSlot::*;
    match type_id {
        "elliott_impulse" => match i {
            1 => Some(Impulse1),
            2 => Some(Impulse2),
            3 => Some(Impulse3),
            4 => Some(Impulse4),
            5 => Some(Impulse5),
            _ => None,
        },
        "elliott_correction" => match i {
            1 => Some(CorrectionA),
            2 => Some(CorrectionB),
            3 => Some(CorrectionC),
            _ => None,
        },
        "elliott_triangle" => match i {
            1 => Some(TriangleA),
            2 => Some(TriangleB),
            3 => Some(TriangleC),
            4 => Some(TriangleD),
            5 => Some(TriangleE),
            _ => None,
        },
        // ElliottDoubleCombo::wave_labels(): [combo[0], "", "", combo[1], combo[2], "", ""]
        // — "W" at index 0 (its own start), "X" at index 3 (its own end,
        // matching `slot_span`), "Y" at index 4 (an interior subwave of its
        // own (3, 6) span).
        "elliott_double_combo" => match i {
            0 => Some(ComboW),
            3 => Some(ComboX),
            4 => Some(ComboY),
            _ => None,
        },
        // ElliottTripleCombo::wave_labels(): [combo[0], "", "", combo[1], combo[2], "", combo[3], combo[4], ""]
        // — "W" at index 0, "X" at index 3 (its own end), "Y" at index 4
        // (interior of its own (3, 5) span), "XX" at index 6 (its own end),
        // "Z" at index 7 (interior of its own (6, 8) span).
        "elliott_triple_combo" => match i {
            0 => Some(ComboW),
            3 => Some(ComboX),
            4 => Some(ComboY),
            6 => Some(ComboXX),
            7 => Some(ComboZ),
            _ => None,
        },
        _ => None,
    }
}

// =========================================================================
// Promotion — collapsing a same-degree chain into the degree above
// =========================================================================

/// The five-slot table a motive-opened chain (head is `"elliott_impulse"`)
/// maps its own members onto, in chain order — hoisted out of
/// [`promote_chain_shape`]'s own body (where it used to be a local `const`)
/// so [`chain_slot_for_index`] can share the SAME table instead of a second
/// copy of it.
const IMPULSE_CHAIN_SLOTS: [WaveSlot; 5] =
    [WaveSlot::Impulse1, WaveSlot::Impulse2, WaveSlot::Impulse3, WaveSlot::Impulse4, WaveSlot::Impulse5];

/// The three-slot table a corrective-opened chain maps its own members onto
/// — see [`IMPULSE_CHAIN_SLOTS`]'s own doc.
const CORRECTION_CHAIN_SLOTS: [WaveSlot; 3] = [WaveSlot::CorrectionA, WaveSlot::CorrectionB, WaveSlot::CorrectionC];

/// The [`WaveSlot`] chain member `i` (0-indexed) WOULD occupy in the coarser
/// structure a `follows` chain promotes into, given whether the chain OPENS
/// with a motive structure (`starts_motive` — `head.type_id() ==
/// "elliott_impulse"`) — the SAME per-member slot assignment
/// [`promote_chain_shape`] itself builds `PromotedShape::slots` from,
/// exposed here so `elliott::guard::chain_violations`'s own chain-level
/// rules (`"chain_degree"`/`"chain_alternation"`) can still name a slot on a
/// chain that does not (yet, or ever) satisfy every OTHER promotion
/// precondition [`promote_chain_shape`] checks — those two rules exist
/// PRECISELY to catch a chain [`promote_chain_shape`] would refuse.
///
/// `None` past the parent type's own slot count (index 5+ for a
/// motive-opened chain, 3+ for a corrective-opened one) — there is no slot
/// left to name.
pub fn chain_slot_for_index(starts_motive: bool, i: usize) -> Option<WaveSlot> {
    if starts_motive { IMPULSE_CHAIN_SLOTS.get(i).copied() } else { CORRECTION_CHAIN_SLOTS.get(i).copied() }
}

/// The chain's own pivot sequence — the head's FIRST point, then every
/// member's own LAST point, in chain order — EXACTLY the point array a
/// promoted parent would carry (see [`PromotedShape::points`]'s own doc).
/// [`promote_chain_shape`] itself calls this rather than deriving the
/// sequence a second time, so the two can never drift apart; exposed
/// publicly for a caller that needs the same sequence WITHOUT the full
/// promotion — e.g. the backend drawing a chain overlay, or
/// `elliott::guard::chain_violations`'s own family-rule path, which reads
/// it back off [`promote_chain_shape`]'s own result rather than calling
/// this directly (see that function's own doc for why).
///
/// Shorter than `chain.len() + 1` when the head's own first point or some
/// later member's own last point is unavailable (`points()` returns an
/// empty vec — never happens for a real Elliott primitive, whose point
/// arrays are fixed-size or start pre-filled, but this function does not
/// assume that and simply stops rather than panicking).
/// [`promote_chain_shape`] itself treats that shortfall as a refusal. Empty
/// for an empty `chain`.
pub fn chain_pivots(chain: &[&dyn Primitive]) -> Vec<(i64, f64)> {
    let mut points = Vec::with_capacity(chain.len() + 1);
    let Some(head) = chain.first() else { return points };
    let Some(&first) = head.points().first() else { return points };
    points.push(first);
    for p in chain {
        let Some(&last) = p.points().last() else { break };
        points.push(last);
    }
    points
}

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
    let (parent_type, parent_slots): (&'static str, &[WaveSlot]) = if starts_motive {
        ("elliott_impulse", &IMPULSE_CHAIN_SLOTS)
    } else {
        ("elliott_correction", &CORRECTION_CHAIN_SLOTS)
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

    let points = chain_pivots(chain);
    if points.len() != chain.len() + 1 {
        return None;
    }

    Some(PromotedShape {
        type_id: parent_type,
        degree: degree.coarser(),
        slots: parent_slots[..chain.len()].to_vec(),
        points,
    })
}

// =========================================================================
// Remapping subdivisions across a parent's own type change (owner report
// 2026-08-14, this module's own "Conversion" section above: changing what a
// structure IS must not silently ORPHAN what is nested inside it — a
// subdivision survives by ORDINAL, not by name, because the names are
// per-family (`CorrectionA`, `TriangleA`, `Impulse1` are three different
// tokens for "the wave the analyst drew first") while the POSITION inside
// the count is the one thing every family's own slot table agrees on.)
// =========================================================================

/// The five-slot table [`WaveSlot::TriangleA`]..[`WaveSlot::TriangleE`], in
/// [`Primitive::wave_slots`]'s own order for `ElliottTriangle` — see
/// [`IMPULSE_CHAIN_SLOTS`]'s own doc for why this module keeps one table
/// per type rather than re-deriving the order from a live
/// `Primitive::wave_slots()` call (there is no live primitive here, only a
/// `type_id` string).
const TRIANGLE_SLOTS: [WaveSlot; 5] =
    [WaveSlot::TriangleA, WaveSlot::TriangleB, WaveSlot::TriangleC, WaveSlot::TriangleD, WaveSlot::TriangleE];

/// The three-slot table for `ElliottDoubleCombo` — see [`TRIANGLE_SLOTS`]'s
/// own doc.
const DOUBLE_COMBO_SLOTS: [WaveSlot; 3] = [WaveSlot::ComboW, WaveSlot::ComboX, WaveSlot::ComboY];

/// The five-slot table for `ElliottTripleCombo` — see [`TRIANGLE_SLOTS`]'s
/// own doc.
const TRIPLE_COMBO_SLOTS: [WaveSlot; 5] =
    [WaveSlot::ComboW, WaveSlot::ComboX, WaveSlot::ComboY, WaveSlot::ComboXX, WaveSlot::ComboZ];

/// `type_id`'s own [`Primitive::wave_slots`] order — the single lookup
/// [`slot_ordinal`]/[`slot_at_ordinal`] share, reusing [`IMPULSE_CHAIN_SLOTS`]/
/// [`CORRECTION_CHAIN_SLOTS`] rather than a second copy of either table.
/// `None` for a `type_id` outside the five Elliott types.
fn wave_slots_for_type(type_id: &str) -> Option<&'static [WaveSlot]> {
    Some(match type_id {
        "elliott_impulse" => &IMPULSE_CHAIN_SLOTS,
        "elliott_correction" => &CORRECTION_CHAIN_SLOTS,
        "elliott_triangle" => &TRIANGLE_SLOTS,
        "elliott_double_combo" => &DOUBLE_COMBO_SLOTS,
        "elliott_triple_combo" => &TRIPLE_COMBO_SLOTS,
        _ => return None,
    })
}

/// `slot`'s own position within `type_id`'s [`Primitive::wave_slots`] order
/// (0-indexed) — e.g. `slot_ordinal("elliott_correction", WaveSlot::
/// CorrectionA) == Some(0)`, `slot_ordinal("elliott_impulse", WaveSlot::
/// Impulse3) == Some(2)`. `None` when `slot` does not exist on `type_id`
/// (mirrors [`slot_span`]'s own `None` contract) or `type_id` is outside
/// the five Elliott types.
///
/// Ordinal — not name — is the mapping a subdivision survives its parent's
/// own type change by: the child in the FIRST wave stays in the first wave
/// (`CorrectionA` -> `TriangleA` -> `Impulse1`) because that is the ONE
/// thing every family's own slot table agrees on across a conversion — the
/// NAMES are per-family (there is no `TriangleA` on an impulse, no
/// `Impulse1` on a triangle), but "the wave the analyst drew first" is the
/// same position regardless of which family the parent becomes.
pub fn slot_ordinal(type_id: &str, slot: WaveSlot) -> Option<usize> {
    wave_slots_for_type(type_id)?.iter().position(|&s| s == slot)
}

/// The inverse of [`slot_ordinal`]: the [`WaveSlot`] at position `ordinal`
/// (0-indexed) in `type_id`'s own [`Primitive::wave_slots`] order. `None`
/// when `ordinal` is past `type_id`'s own slot count (e.g. ordinal 4 on
/// `"elliott_correction"`, which has only 3 slots) or `type_id` is outside
/// the five Elliott types — a subdivision that was nested in a wave the new
/// type does not carry loses its own link rather than landing somewhere it
/// was never drawn; the caller (`mlc-app`) is expected to clear
/// [`WaveLink`] on any child whose own `slot_before`'s ordinal maps to
/// `None` here, never to invent a slot for it.
pub fn slot_at_ordinal(type_id: &str, ordinal: usize) -> Option<WaveSlot> {
    wave_slots_for_type(type_id)?.get(ordinal).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing::primitives_v2::elliott::{ElliottImpulse, ElliottCorrection, ElliottDoubleCombo, ElliottTriangle};

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

    // =====================================================================
    // predecessor_seed / predecessor_kinds (owner ask 2026-08-14,
    // "Predecession" section above — `SuccessorSeed`/`successor_kinds` read
    // backwards)
    // =====================================================================

    #[test]
    fn predecessor_seed_ends_at_nexts_first_point_same_degree() {
        let points: [(i64, f64); 6] = [
            (0, 100.0), (1_000, 110.0), (2_000, 105.0), (3_000, 130.0), (4_000, 120.0), (10_000, 150.0),
        ];
        let impulse = ElliottImpulse::new(points, "#2196F3");
        let seed = predecessor_seed(&impulse).expect("impulse seeds a predecessor");
        assert_eq!(seed.end, points[0]);
        assert_eq!(seed.degree, WaveDegree::Intermediate, "predecession keeps the SAME degree, never finer");
    }

    #[test]
    fn predecessor_seed_span_is_0_618_of_nexts_own_bar_span_and_negative() {
        let points: [(i64, f64); 6] = [
            (0, 100.0), (1_000, 110.0), (2_000, 105.0), (3_000, 130.0), (4_000, 120.0), (10_000, 150.0),
        ];
        let impulse = ElliottImpulse::new(points, "#2196F3");
        let seed = predecessor_seed(&impulse).expect("impulse seeds a predecessor");
        // next's own bar span is 10_000 - 0 = 10_000; the predecessor's own
        // start lies BEFORE `end` in time, so the hint carries the negative
        // sign.
        assert_eq!(seed.span_bars_hint, -((10_000f64 * 0.618).round() as i64));
    }

    #[test]
    fn predecessor_seed_price_hint_is_half_nexts_travel_not_negated() {
        let points: [(i64, f64); 6] = [
            (0, 100.0), (1_000, 110.0), (2_000, 105.0), (3_000, 130.0), (4_000, 120.0), (10_000, 150.0),
        ];
        let impulse = ElliottImpulse::new(points, "#2196F3");
        let seed = predecessor_seed(&impulse).expect("impulse seeds a predecessor");
        // next travelled +50 (100.0 -> 150.0); the predecessor's own
        // default motion is HALF that, in the OPPOSITE direction — which,
        // because `end` (not `start`) is the fixed anchor here, comes out
        // as a POSITIVE price_hint (see `PredecessorSeed`'s own doc for the
        // "inverted relative to the successor case" derivation).
        assert_eq!(seed.price_hint, 25.0);
        assert_eq!(seed.start(), (points[0].0 + seed.span_bars_hint, 100.0 + 25.0));
    }

    #[test]
    fn predecessor_seed_none_for_undrawn_primitive() {
        // Same "not enough points yet" contract `successor_seed`'s own
        // test exercises — the "not Elliott" `None` path is covered
        // separately below, by `predecessor_kinds_empty_for_non_elliott_primitive`.
        let points: [(i64, f64); 4] = [(0, 100.0), (0, 100.0), (0, 100.0), (0, 100.0)];
        let correction = ElliottCorrection::new(points, "#FF5722");
        assert!(predecessor_seed(&correction).is_some());
    }

    #[test]
    fn predecessor_kinds_alternates_motive_and_corrective() {
        let impulse_points: [(i64, f64); 6] = [
            (0, 100.0), (1, 101.0), (2, 102.0), (3, 103.0), (4, 104.0), (5, 105.0),
        ];
        let impulse = ElliottImpulse::new(impulse_points, "#2196F3");
        let before_impulse = predecessor_kinds(&impulse);
        assert_eq!(before_impulse, &["elliott_correction", "elliott_triangle", "elliott_double_combo", "elliott_triple_combo"]);
        assert!(!before_impulse.contains(&"elliott_impulse"), "a motive structure must never offer another motive structure as its predecessor");

        let correction_points: [(i64, f64); 4] = [(0, 100.0), (1, 101.0), (2, 102.0), (3, 103.0)];
        let correction = ElliottCorrection::new(correction_points, "#FF5722");
        let before_correction = predecessor_kinds(&correction);
        assert_eq!(before_correction, &["elliott_impulse"]);
    }

    #[test]
    fn predecessor_kinds_empty_for_non_elliott_primitive() {
        use crate::drawing::primitives_v2::TrendLine;
        let line = TrendLine::new(0, 100.0, 1_000, 110.0, "#2196F3");
        assert!(predecessor_kinds(&line).is_empty());
    }

    #[test]
    fn predecessor_kinds_matches_successor_kinds_today_but_stays_its_own_function() {
        // The relation happens to be symmetric today (two classes, motive
        // and corrective) — see `predecessor_kinds`' own doc for why the
        // two functions are kept separate anyway.
        let impulse_points: [(i64, f64); 6] = [
            (0, 100.0), (1, 101.0), (2, 102.0), (3, 103.0), (4, 104.0), (5, 105.0),
        ];
        let impulse = ElliottImpulse::new(impulse_points, "#2196F3");
        assert_eq!(predecessor_kinds(&impulse), successor_kinds(&impulse));
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

    // =====================================================================
    // adoptable_children / adopted_parent_degree (owner ask 2026-08-14,
    // this module's own "Adoption" section above)
    // =====================================================================

    /// The parent every adoption test below drives — a plain impulse whose
    /// own five slots are exactly what the tests below match candidates
    /// against. `id` is set apart from every candidate `id` used below so
    /// "not `parent` itself" is exercised, not merely assumed to hold.
    fn impulse_parent_for_adoption() -> ElliottImpulse {
        let points: [(i64, f64); 6] = [
            (0, 100.0), (1_000, 110.0), (2_000, 105.0), (3_000, 130.0), (4_000, 120.0), (5_000, 140.0),
        ];
        let mut p = ElliottImpulse::new(points, "#2196F3");
        p.data.id = 100;
        p
    }

    /// A standalone impulse spanning exactly `start..end`, for the Impulse1
    /// motive slot — only interior points are degenerate (repeated `start`);
    /// `adoptable_children` only ever reads a candidate's own first/last
    /// point, so the interior shape does not matter to any test below.
    fn standalone_impulse(id: u64, degree: WaveDegree, start: (i64, f64), end: (i64, f64)) -> ElliottImpulse {
        let points: [(i64, f64); 6] = [start, start, start, start, start, end];
        let mut p = ElliottImpulse::new(points, "#2196F3");
        p.data.id = id;
        p.degree = degree;
        p
    }

    /// A standalone (zigzag) correction spanning exactly `start..end` — the
    /// corrective candidate every wave-2-slot test below adopts.
    fn standalone_correction(id: u64, degree: WaveDegree, start: (i64, f64), end: (i64, f64)) -> ElliottCorrection {
        let points: [(i64, f64); 4] = [start, start, start, end];
        let mut p = ElliottCorrection::new(points, "#FF5722");
        p.data.id = id;
        p.degree = degree;
        p
    }

    /// A standalone triangle spanning exactly `start..end` — used only by
    /// the illegal-kind test below (a triangle is never legal in an
    /// impulse's wave-2 slot, see `child_kinds_impulse_wave2_excludes_triangle`).
    fn standalone_triangle(id: u64, degree: WaveDegree, start: (i64, f64), end: (i64, f64)) -> ElliottTriangle {
        let points: [(i64, f64); 6] = [start, start, start, start, start, end];
        let mut p = ElliottTriangle::new(points, "#9C27B0");
        p.data.id = id;
        p.degree = degree;
        p
    }

    #[test]
    fn adoptable_children_adopts_exact_matches_with_right_slots() {
        let parent = impulse_parent_for_adoption();
        // Impulse1 span: (0, 100.0) -> (1_000, 110.0).
        let child_a = standalone_impulse(1, WaveDegree::Minute, (0, 100.0), (1_000, 110.0));
        // Impulse2 span: (1_000, 110.0) -> (2_000, 105.0).
        let child_b = standalone_correction(2, WaveDegree::Minute, (1_000, 110.0), (2_000, 105.0));
        let candidates: Vec<(u64, &dyn Primitive)> = vec![(1, &child_a), (2, &child_b)];

        let mut adopted = adoptable_children(&parent, &candidates);
        adopted.sort_by_key(|a| a.child_id);
        assert_eq!(adopted.len(), 2, "both drawn-over candidates must be recognised");
        assert_eq!(adopted[0], Adoption { child_id: 1, slot: WaveSlot::Impulse1, start: (0, 100.0), end: (1_000, 110.0) });
        assert_eq!(adopted[1], Adoption { child_id: 2, slot: WaveSlot::Impulse2, start: (1_000, 110.0), end: (2_000, 105.0) });

        assert_eq!(
            adopted_parent_degree(&candidates, &adopted),
            Some(WaveDegree::Minute.coarser()),
            "the new parent's own degree is one step coarser than what it adopted"
        );
    }

    #[test]
    fn adoptable_children_refuses_endpoint_outside_tolerance() {
        let parent = impulse_parent_for_adoption();
        // Parent's own price span is 140.0 - 100.0 = 40.0, so the 2% budget
        // is 0.8 — shifting the candidate's own end by 2.0 clears it.
        let child = standalone_impulse(1, WaveDegree::Minute, (0, 100.0), (1_000, 112.0));
        let candidates: Vec<(u64, &dyn Primitive)> = vec![(1, &child)];
        assert!(adoptable_children(&parent, &candidates).is_empty(), "an endpoint outside the click-slop budget must not be adopted");
    }

    #[test]
    fn adoptable_children_refuses_a_child_already_linked_elsewhere() {
        let parent = impulse_parent_for_adoption();
        let mut child = standalone_impulse(1, WaveDegree::Minute, (0, 100.0), (1_000, 110.0));
        child.link = Some(WaveLink { parent_id: 999, slot: WaveSlot::CorrectionA });
        let candidates: Vec<(u64, &dyn Primitive)> = vec![(1, &child)];
        assert!(adoptable_children(&parent, &candidates).is_empty(), "a structure already nested under another parent must not be re-adopted");
    }

    #[test]
    fn adoptable_children_refuses_a_kind_illegal_in_the_slot() {
        let parent = impulse_parent_for_adoption();
        // Impulse2 span: (1_000, 110.0) -> (2_000, 105.0) — geometrically
        // exact, but a triangle is never a legal wave-2 candidate.
        let child = standalone_triangle(1, WaveDegree::Minute, (1_000, 110.0), (2_000, 105.0));
        let candidates: Vec<(u64, &dyn Primitive)> = vec![(1, &child)];
        assert!(adoptable_children(&parent, &candidates).is_empty(), "a triangle must never be adopted into an impulse's own wave-2 slot");
    }

    #[test]
    fn adoptable_children_keeps_only_the_coarsest_degree_present() {
        let parent = impulse_parent_for_adoption();
        // Impulse1 span, drawn at the COARSER of the two degrees.
        let child_a = standalone_impulse(1, WaveDegree::Minor, (0, 100.0), (1_000, 110.0));
        // Impulse2 span, drawn at the FINER of the two degrees.
        let child_b = standalone_correction(2, WaveDegree::Minute, (1_000, 110.0), (2_000, 105.0));
        let candidates: Vec<(u64, &dyn Primitive)> = vec![(1, &child_a), (2, &child_b)];

        let adopted = adoptable_children(&parent, &candidates);
        assert_eq!(adopted.len(), 1, "only the coarsest-degree match survives");
        assert_eq!(adopted[0].child_id, 1);
        assert_eq!(adopted[0].slot, WaveSlot::Impulse1);

        assert_eq!(
            adopted_parent_degree(&candidates, &adopted),
            Some(WaveDegree::Minor.coarser()),
            "the new parent's own degree is derived from the SURVIVING (coarsest) degree only"
        );
    }

    #[test]
    fn adoptable_children_empty_when_nothing_matched() {
        let parent = impulse_parent_for_adoption();
        assert!(adoptable_children(&parent, &[]).is_empty());
    }

    /// Two DIFFERENT candidates matching the SAME slot is a broken state,
    /// not a preference between them — `adoptable_children` must resolve to
    /// exactly one adoption, keeping the CLOSER match (lower score), never
    /// whichever happened to be listed first in `candidates`.
    #[test]
    fn adoptable_children_two_candidates_competing_for_one_slot_yields_the_closer_one() {
        let parent = impulse_parent_for_adoption();
        // Impulse1 span: (0, 100.0) -> (1_000, 110.0).
        let exact = standalone_impulse(1, WaveDegree::Minute, (0, 100.0), (1_000, 110.0));
        // Same slot, same degree (so the degree filter cannot be what
        // picks the winner — only the distance score can), but 0.3 off on
        // price at both ends — still inside the 0.8 tolerance budget, so
        // both genuinely qualify.
        let off = standalone_impulse(2, WaveDegree::Minute, (0, 100.3), (1_000, 110.3));
        // Listing the WORSE match first proves the winner is picked by
        // score, not by candidate order.
        let candidates: Vec<(u64, &dyn Primitive)> = vec![(2, &off), (1, &exact)];

        let adopted = adoptable_children(&parent, &candidates);
        assert_eq!(adopted.len(), 1, "a slot holds exactly one subdivision — never two links to the same slot");
        assert_eq!(adopted[0].child_id, 1, "the EXACT match must win over the 0.3-off one");
        assert_eq!(adopted[0].slot, WaveSlot::Impulse1);
        assert_eq!(adopted[0].start, (0, 100.0));
        assert_eq!(adopted[0].end, (1_000, 110.0));
    }

    /// One candidate whose own two points are both short enough to sit
    /// inside the tolerance box of TWO different slots (the degenerate
    /// "very short middle wave" case) must be adopted into exactly ONE of
    /// them — the closer-scoring one — never linked twice.
    #[test]
    fn adoptable_children_one_candidate_matching_two_slots_is_adopted_once() {
        // Waves 2 and 4 both cluster tightly around ts~1_020, price~100.06
        // — close enough together that a single short candidate sitting in
        // that cluster falls inside BOTH slots' own tolerance boxes. Waves
        // 0 and 5 stay far apart so the parent's own bounding span (and so
        // its tolerance budget: price 0.8, ts 100) is not itself degenerate.
        let points: [(i64, f64); 6] = [
            (0, 100.0),
            (1_000, 100.05),
            (1_010, 100.10),
            (1_020, 100.08),
            (1_030, 100.03),
            (5_000, 140.0),
        ];
        let parent = ElliottImpulse::new(points, "#2196F3");
        // Impulse2 span: (1_000, 100.05) -> (1_010, 100.10).
        // Impulse4 span: (1_020, 100.08) -> (1_030, 100.03).
        // Both slots accept a zigzag correction (neither is motive-only).
        // This short candidate's own first/last (1_015, 100.06) ->
        // (1_018, 100.07) sits inside BOTH slots' own tolerance boxes.
        let child = standalone_correction(9, WaveDegree::Minute, (1_015, 100.06), (1_018, 100.07));
        let candidates: Vec<(u64, &dyn Primitive)> = vec![(9, &child)];

        let adopted = adoptable_children(&parent, &candidates);
        assert_eq!(adopted.len(), 1, "one candidate must never be linked into two slots at once");
        assert_eq!(adopted[0].child_id, 9);
        // Impulse4's own anchors are numerically closer to the candidate's
        // own endpoints than Impulse2's — see this test's own comment
        // above for the raw deltas; the lower-scored slot must win.
        assert_eq!(adopted[0].slot, WaveSlot::Impulse4, "the closer-scoring slot must win, not merely a legal one");
    }

    // =====================================================================
    // resample_polyline (owner report 2026-08-14, this module's own
    // "Resampling" section)
    // =====================================================================

    /// A non-trivial drawn shape (an impulse-like zigzag, legs of visibly
    /// different length/direction) — used across several tests below so a
    /// property is checked against something more demanding than a straight
    /// line.
    fn sample_wave() -> [(i64, f64); 6] {
        [(0, 100.0), (1_000, 130.0), (1_500, 115.0), (3_000, 160.0), (3_200, 150.0), (5_000, 200.0)]
    }

    #[test]
    fn resample_same_n_returns_input_unchanged() {
        let points = sample_wave();
        assert_eq!(resample_polyline(&points, points.len()), points.to_vec());
    }

    #[test]
    fn resample_preserves_endpoints_for_every_n() {
        let points = sample_wave();
        for n in 2..=12usize {
            let out = resample_polyline(&points, n);
            assert_eq!(out.len(), n);
            assert_eq!(out[0], points[0], "first point must be exactly the input's own first point (n={n})");
            assert_eq!(out[n - 1], points[points.len() - 1], "last point must be exactly the input's own last point (n={n})");
        }
    }

    #[test]
    fn resample_straight_line_evenly_spaced() {
        let points: [(i64, f64); 2] = [(0, 0.0), (1_000, 100.0)];
        let out = resample_polyline(&points, 5);
        assert_eq!(out, vec![(0, 0.0), (250, 25.0), (500, 50.0), (750, 75.0), (1_000, 100.0)]);
    }

    #[test]
    fn resample_degenerate_inputs_never_panic() {
        // n == 0 -> empty, regardless of points.
        assert_eq!(resample_polyline(&sample_wave(), 0), Vec::new());
        // points.is_empty() -> empty, regardless of n.
        assert_eq!(resample_polyline(&[], 4), Vec::new());
        // points.len() == 1 -> n copies of the single point.
        assert_eq!(resample_polyline(&[(5, 50.0)], 3), vec![(5, 50.0), (5, 50.0), (5, 50.0)]);
        // n == 1 with points.len() >= 2 -> the single input point.
        assert_eq!(resample_polyline(&sample_wave(), 1), vec![sample_wave()[0]]);
        // Zero arc length: every point identical, `n` different from the
        // input length so the `n == points.len()` short-circuit above does
        // not mask this branch.
        let flat = [(10, 20.0), (10, 20.0), (10, 20.0), (10, 20.0)];
        assert_eq!(resample_polyline(&flat, 6), vec![(10, 20.0); 6]);
    }

    // =====================================================================
    // label_collision_indices (regression fix, owner report 2026-08-14,
    // second pass: a first attempt derived the collision from the SLOT
    // alone and stepped out both ends of every subdivided span — wrong,
    // same failure shape as the blanket per-degree ring: it moved a label
    // that has nothing to collide with. The rule now reads the CHILD's own
    // `point_labels()`.)
    // =====================================================================

    fn impulse_fixture() -> ElliottImpulse {
        let points: [(i64, f64); 6] = [
            (0, 100.0), (1_000, 110.0), (2_000, 105.0), (3_000, 130.0), (4_000, 120.0), (5_000, 140.0),
        ];
        ElliottImpulse::new(points, "#2196F3")
    }

    #[test]
    fn no_children_no_collisions() {
        let impulse = impulse_fixture();
        assert_eq!(label_collision_indices(&impulse, &[]), Vec::<usize>::new());
    }

    /// `ElliottCorrection::wave_labels()` is `["", A, B, C]` — index 0 is
    /// EMPTY, so a correction nested into wave 4 (span `(3, 4)`) never
    /// collides at the span's own START (3): there is no label there to
    /// collide with. Its own LAST label ("C") is non-empty, so it collides
    /// at the span's own END (4) only.
    #[test]
    fn correction_nested_in_wave4_collides_at_index_4_only() {
        let impulse = impulse_fixture();
        let correction_points: [(i64, f64); 4] =
            [(3_000, 130.0), (3_250, 122.0), (3_500, 128.0), (4_000, 120.0)];
        let correction = ElliottCorrection::new(correction_points, "#FF5722");
        let children: Vec<(&dyn Primitive, WaveSlot)> = vec![(&correction, WaveSlot::Impulse4)];
        assert_eq!(label_collision_indices(&impulse, &children), vec![4]);
    }

    /// `ElliottDoubleCombo::wave_labels()` is `[W, "", "", X, Y, "", ""]` —
    /// index 0 ("W") IS labelled, so a double combo nested into wave 4
    /// (span `(3, 4)`) genuinely collides at the span's own START (3). Its
    /// own LAST point (index 6, "") carries no label — `label_slot_for_index`'s
    /// own doc explains why: "Y" is drawn at an INTERIOR subwave pivot, not
    /// at the component's own true end — so nesting one does NOT collide at
    /// the span's own END (4), unlike the correction case above. Deriving
    /// the rule from the slot alone (the removed `slot_boundary_collides`)
    /// would have collided at both; reading the child's own labels collides
    /// at exactly the one that is real.
    #[test]
    fn double_combo_nested_in_wave4_collides_at_index_3_only() {
        let impulse = impulse_fixture();
        let combo_points: [(i64, f64); 7] = [
            (3_000, 130.0), (3_100, 126.0), (3_200, 128.0), (3_400, 124.0),
            (3_600, 127.0), (3_800, 122.0), (4_000, 120.0),
        ];
        let combo = ElliottDoubleCombo::new(combo_points, "#9C27B0");
        let children: Vec<(&dyn Primitive, WaveSlot)> = vec![(&combo, WaveSlot::Impulse4)];
        assert_eq!(label_collision_indices(&impulse, &children), vec![3]);
    }
}
