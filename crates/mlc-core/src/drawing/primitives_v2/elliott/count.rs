//! Elliott wave markup tree.
//!
//! Today the five Elliott primitives (`ElliottImpulse`, `ElliottCorrection`,
//! `ElliottTriangle`, `ElliottDoubleCombo`, `ElliottTripleCombo`) are five
//! independent drawings — each carries its own [`super::super::config::WaveDegree`]
//! and nothing links them. This module adds the link: a child primitive
//! carries a [`WaveLink`] naming which [`WaveSlot`] of which parent it
//! occupies, and [`child_seed`] tells a caller what degree/anchor points a
//! NEW child nested into a given slot must be constructed with.
//!
//! Pure data + pure functions — no rendering, no rule validation (a later
//! wave), no dependency outside `mlc-core`.

use serde::{Deserialize, Serialize};
use super::super::Primitive;
use super::super::config::WaveDegree;

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

    /// Whether this slot subdivides as a 5-wave MOTIVE structure (seeds an
    /// `ElliottImpulse` child) rather than a corrective one (seeds an
    /// `ElliottCorrection` child) — classic Elliott alternation: only
    /// impulse waves 1, 3, 5 are motive. Impulse waves 2/4, every A/B/C
    /// corrective leg, and every triangle/combination slot are corrective —
    /// none of those subdivide as a 5-wave motive.
    ///
    /// Read by the caller that creates a NEW child primitive for a slot
    /// (`elliott::count` itself never constructs a primitive, per this
    /// module's own doc) to decide which Elliott type to build.
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
        match n {
            0 => Vec::new(),
            1 => vec![self.start],
            _ => (0..n)
                .map(|i| {
                    let t = i as f64 / (n - 1) as f64;
                    let ts = self.start.0 + ((self.end.0 - self.start.0) as f64 * t).round() as i64;
                    let price = self.start.1 + (self.end.1 - self.start.1) * t;
                    (ts, price)
                })
                .collect(),
        }
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
fn slot_span(type_id: &str, slot: WaveSlot) -> Option<(usize, usize)> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing::primitives_v2::elliott::{ElliottImpulse, ElliottDoubleCombo};

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
}
