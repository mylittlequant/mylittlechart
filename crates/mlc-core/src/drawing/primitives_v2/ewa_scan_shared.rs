//! Shared "hit looks like a hand-placed structure" constructor — used by
//! BOTH `ElliottScan` (A2) and `HarmonicScan` (A3), both draw off the SAME
//! `mli::ewa::types::EwaPatternKind` vocabulary — one function, not two
//! per-family copies, per this codebase's own hard "no parallel
//! vocabularies" rule.
//!
//! `docs/mlc/plans/autodetectors-a2-a4-implementation-2026-09-24.md`, "Shared
//! hit-looks-like-a-hand-placed-structure constructor".

use std::ops::Range;

use crate::Bar;
use super::config::WaveDegree;
use super::elliott::{CorrectionType, ElliottCorrection, ElliottImpulse, ElliottTriangle, ImpulseKind};
use super::patterns::xabcd_pattern::XabcdHarmonicKind;
use super::patterns::{AbcdPattern, CypherPattern, FiveZeroPattern, ThreeDrives, XabcdPattern};
use super::scan_shape::{ScanShape, price_band_filter};
use super::Primitive;

/// Hard safety margin under `mli::ewa::scan::MAX_PIVOTS_PER_SCAN` (500,
/// oldest-first — the OLDEST `MAX_PIVOTS_PER_SCAN` pivots survive, every
/// newer one is silently dropped before `scan_waves`/
/// `scan_waves_with_harmonic_tolerance` ever apply their own
/// `completion_range` filter). [`ElliottScan`]/[`HarmonicScan`] used to feed
/// `mli::ewa::swing::extract_pivots` the WHOLE loaded bar series on every
/// call (mirroring `CandlePatternScan`'s own "whole series" shape, which is
/// safe for that scan only because its own `mli::events::candle_pattern`
/// detector carries no such cap) — on any real multi-month/year chart that
/// accumulates more than ~500 `swing_n` pivots, EVERY pivot in the survey's
/// own recent range was discarded before `completion_range` could ever
/// select it, so a scan dragged over current price action always returned
/// zero hits (owner report 2026-09-25: "EWA scan doesn't work, Harmonic
/// scan doesn't work").
///
/// Measured against 10,000 real 1H BTCUSDT bars
/// (`mlq-stand/data/btcusdt_h1_10000.json`, coordinator's own diagnosis
/// probe): the densest 800-bar window at `swing_n=1` (the most sensitive
/// per-drawing setting either scan's own Thresholds tab offers) carries 350
/// pivots — a 30% margin under the cap. 1000 bars already reaches 437 (a
/// 13% margin, too tight for a value meant to hold regardless of how choppy
/// a market gets) and 1500 bars exceeds the cap outright (651). 800 is
/// wider than `SmcEngineConfig::judge_lookback_bars`'s own default (300) —
/// `mli::ewa::scan`'s multi-leg pivot-chain enumeration needs more history
/// to find a genuine structure than `mli::smc`'s single-bar-anchored
/// detectors do.
pub const EWA_SCAN_LOOKBACK_BARS: usize = 800;

/// Bound `bars` down to the window [`ElliottScan`]/[`HarmonicScan`] should
/// feed `mli::ewa::{swing, scan}` for a survey anchored at `[start, end]`
/// (both GLOBAL, inclusive bar indices — `resolved_window`'s own return
/// shape) — see [`EWA_SCAN_LOOKBACK_BARS`]'s own doc for why a bound exists
/// at all. Anchored at the survey's own RIGHT edge (`end`),
/// [`EWA_SCAN_LOOKBACK_BARS`] bars back, clamped to bar zero on a short
/// series (`saturating_sub`) — mirrors `smc_judge::judge_window`'s own
/// fixed-size-lookback shape: the window size never grows with how wide a
/// range the survey itself covers (a `coverage == "all_bars"` survey over
/// 10,000 bars gets the SAME ~800-bar window as a narrow dragged range
/// would), because `mli::ewa::scan`'s own hard pivot cap makes a wider
/// window than that unsafe regardless of what the survey asked for.
///
/// Returns the windowed bars slice plus the survey's own `[start, end]`
/// translated into a HALF-OPEN range LOCAL to that slice — the same local
/// index space `extract_pivots`/`build_segments` produce pivots in when
/// handed this same slice, so neither `scan_waves` nor [`resolve_hit`]
/// (called with this same windowed slice, never the original `bars`) needs
/// any further index translation. When the survey's own `start` falls
/// BEFORE the window (a wide/`"all_bars"` survey whose own requested start
/// the lookback bound had to clip away), the local range's own start
/// clamps to `0` — every pivot the bounded window carries is fair game for
/// completion, since there is no earlier bar left to distinguish "inside
/// the survey's own range" from "inside the window."
pub fn ewa_scan_window(bars: &[Bar], start: usize, end: usize) -> (&[Bar], Range<usize>) {
    let window_start = end.saturating_sub(EWA_SCAN_LOOKBACK_BARS);
    let local_start = start.saturating_sub(window_start);
    let local_end = end - window_start;
    (&bars[window_start..=end], local_start..local_end + 1)
}

/// One resolved hit — pivots ALREADY `(ts_ms, price)` pairs (see this
/// module's own "pivot price" note on [`resolve_hit`]), never raw bar
/// indices, so [`build_wave_primitive`]'s caller never re-derives this.
/// Shared by BOTH `ElliottScan` and `HarmonicScan`'s own `cached_hits` — the
/// SAME "no parallel vocabularies" reasoning this module's own doc gives for
/// [`build_wave_primitive`] itself.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedWaveHit {
    pub kind: mli::ewa::types::EwaPatternKind,
    pub points: Vec<(i64, f64)>,
    pub score: f64,
}

/// Maps one `EwaWaveHit`'s raw bar-index pivots to `(ts_ms, price)` pairs —
/// price via the matching `EwaSwingPivot::price` (High/Low-aware: a pivot
/// carries its own High-vs-Low tag, `bars[idx].close` does not), timestamp
/// via `bars[idx].timestamp * 1000` (the ONE source of a pivot's own ms
/// timestamp — `EwaSwingPivot` carries no timestamp of its own), never
/// `bars[idx].close`. `None` when a pivot's own bar index has no matching
/// entry in `pivots` or falls outside `bars` — a defensive case, unreachable
/// for a hit `scan_waves`/`scan_waves_with_harmonic_tolerance` built from
/// these same two slices, never a panic either way. Filters on the hit's own
/// representative price (the LAST pivot's price, matching `CandlePatternHit`'s
/// own completion-bar convention) via [`price_band_filter`]. Shared by BOTH
/// `ElliottScan::resolved_scan` and `HarmonicScan::resolved_scan` — the ONE
/// place a raw `EwaWaveHit` becomes a [`ResolvedWaveHit`], never forked.
pub fn resolve_hit(
    hit: mli::ewa::scan::EwaWaveHit,
    pivots: &[mli::ewa::swing::EwaSwingPivot],
    bars: &[Bar],
    shape: ScanShape,
    p1: f64,
    p2: f64,
) -> Option<ResolvedWaveHit> {
    let mut points = Vec::with_capacity(hit.pivots.len());
    for &idx in &hit.pivots {
        let pivot = pivots.iter().find(|p| p.index == idx)?;
        let bar = bars.get(idx)?;
        points.push((bar.timestamp * 1000, pivot.price));
    }
    let representative_price = points.last()?.1;
    if !price_band_filter(shape, p1, p2, representative_price) {
        return None;
    }
    Some(ResolvedWaveHit { kind: hit.kind, points, score: hit.score })
}

fn arr6(p: &[(i64, f64)]) -> [(i64, f64); 6] {
    let mut a = [(0i64, 0.0); 6];
    for (i, &v) in p.iter().take(6).enumerate() {
        a[i] = v;
    }
    a
}

fn arr5(p: &[(i64, f64)]) -> [(i64, f64); 5] {
    let mut a = [(0i64, 0.0); 5];
    for (i, &v) in p.iter().take(5).enumerate() {
        a[i] = v;
    }
    a
}

fn arr4(p: &[(i64, f64)]) -> [(i64, f64); 4] {
    let mut a = [(0i64, 0.0); 4];
    for (i, &v) in p.iter().take(4).enumerate() {
        a[i] = v;
    }
    a
}

/// The reverse of `XabcdHarmonicKind::as_ewa_kind` — a small match this
/// function needs and `xabcd_pattern.rs` has no reason to carry itself
/// (its own `from_str` maps a wire-id STRING, not an `EwaPatternKind`).
/// Only ever called with the 7 XABCD-shaped kinds [`build_wave_primitive`]'s
/// own match arm has already narrowed `kind` to — the trailing
/// `Unclassified` fallback is unreachable from that call site, never
/// panics on one anyway.
fn xabcd_kind_of(kind: mli::ewa::types::EwaPatternKind) -> XabcdHarmonicKind {
    use mli::ewa::types::EwaPatternKind as K;
    match kind {
        K::Gartley => XabcdHarmonicKind::Gartley,
        K::Bat => XabcdHarmonicKind::Bat,
        K::AltBat => XabcdHarmonicKind::AltBat,
        K::Butterfly => XabcdHarmonicKind::Butterfly,
        K::Crab => XabcdHarmonicKind::Crab,
        K::DeepCrab => XabcdHarmonicKind::DeepCrab,
        K::Shark => XabcdHarmonicKind::Shark,
        _ => XabcdHarmonicKind::Unclassified,
    }
}

/// mli's `ThreeDrives` names 6 pivots (`X,1,A,2,B,3` —
/// `mylittleindicators/src/ewa/scan.rs`'s own `three_drives_position`), but
/// the mlc `ThreeDrives` primitive (`patterns/three_drives.rs`) declares 7
/// points. Read in full to resolve the mismatch (not guessed): that
/// primitive's own `PIVOT_NAMES` array names only the first six, and
/// neither `measured_ratio` (the ratio arithmetic) nor `render`'s own label
/// array (`["", "D1", "R1", "D2", "R2", "D3", ""]`) ever reads index 6 — the
/// 7th point is a cosmetic trailing handle, not a real pivot. Repeating the
/// last real pivot (`"3"`) gives a zero-length trailing segment, the only
/// geometrically honest placement with no 7th pivot to put it at.
fn three_drives_from_pivots(points: &[(i64, f64)], color: &str) -> Option<Box<dyn Primitive>> {
    if points.len() < 6 {
        return None;
    }
    let six = arr6(points);
    let mut seven = [(0i64, 0.0); 7];
    seven[..6].copy_from_slice(&six);
    seven[6] = six[5];
    Some(Box::new(ThreeDrives::new(seven, color)))
}

/// Builds the SAME primitive type a hand-drawn structure of `kind` would be
/// — `render()` calls this to construct an EPHEMERAL instance and calls its
/// own `.render(ctx, false)` (never re-implementing that primitive's own
/// draw code a second time); `take_scan_placements` calls this to build the
/// REAL instance inserted into the collection. One function, two callers —
/// a detected wave and a hand-placed one can never visually drift apart.
///
/// `points` are ALREADY (timestamp_ms, price) pairs, one per pivot, in the
/// exact order each target primitive's own `points` array expects — see
/// each match arm's own comment for the pivot-name mapping. `None` for a
/// kind with no chart-side primitive counterpart yet (double/triple combo
/// legs, the plain `ImpulseExtendedWave*`/`TruncatedImpulse`/
/// `RunningZigzag`/`RunningFlat` variants `scan_waves` does not produce —
/// unreachable for a hit `scan_waves` actually returned, listed only so a
/// future kind addition fails loudly here instead of silently rendering
/// nothing).
pub fn build_wave_primitive(
    kind: mli::ewa::types::EwaPatternKind,
    points: &[(i64, f64)],
    color: &str,
    degree: WaveDegree,
) -> Option<Box<dyn Primitive>> {
    use mli::ewa::types::EwaPatternKind as K;
    match kind {
        K::Impulse => {
            let mut p = ElliottImpulse::new(arr6(points), color);
            p.degree = degree;
            Some(Box::new(p))
        }
        K::LeadingDiagonalContracting => {
            let mut p = ElliottImpulse::new(arr6(points), color);
            p.impulse_kind = ImpulseKind::LeadingDiagonal;
            p.degree = degree;
            Some(Box::new(p))
        }
        K::EndingDiagonalContracting => {
            let mut p = ElliottImpulse::new(arr6(points), color);
            p.impulse_kind = ImpulseKind::EndingDiagonal;
            p.degree = degree;
            Some(Box::new(p))
        }
        // Expanding diagonals: `ElliottImpulse::ImpulseKind` has no
        // symmetrical/expanding selector (elliott_impulse.rs's own
        // `ImpulseKind`, the SAME precedent `ElliottTriangle` already sets
        // for contracting-only) — both Expanding variants render/place as
        // the SAME Contracting-labelled kind; a documented, not silent,
        // narrowing (flagged for owner sign-off in the arc's own brief).
        K::LeadingDiagonalExpanding => {
            let mut p = ElliottImpulse::new(arr6(points), color);
            p.impulse_kind = ImpulseKind::LeadingDiagonal;
            p.degree = degree;
            Some(Box::new(p))
        }
        K::EndingDiagonalExpanding => {
            let mut p = ElliottImpulse::new(arr6(points), color);
            p.impulse_kind = ImpulseKind::EndingDiagonal;
            p.degree = degree;
            Some(Box::new(p))
        }
        K::Zigzag => {
            let mut p = ElliottCorrection::new(arr4(points), color);
            p.correction_type = CorrectionType::Zigzag;
            p.degree = degree;
            Some(Box::new(p))
        }
        K::Flat | K::RegularFlat => {
            let mut p = ElliottCorrection::new(arr4(points), color);
            p.correction_type = CorrectionType::Flat;
            p.degree = degree;
            Some(Box::new(p))
        }
        K::Triangle | K::ContractingTriangle => {
            let mut p = ElliottTriangle::new(arr6(points), color);
            p.degree = degree;
            Some(Box::new(p))
        }
        K::Gartley | K::Bat | K::AltBat | K::Butterfly | K::Crab | K::DeepCrab | K::Shark => {
            let mut p = XabcdPattern::new(arr5(points), color);
            p.pattern_kind = xabcd_kind_of(kind);
            Some(Box::new(p))
        }
        K::Cypher => Some(Box::new(CypherPattern::new(arr5(points), color))),
        K::Abcd => Some(Box::new(AbcdPattern::new(arr4(points), color))),
        K::ThreeDrives => three_drives_from_pivots(points, color),
        K::FiveZero => {
            let mut a = [(0i64, 0.0); 6];
            for (i, &v) in points.iter().take(6).enumerate() {
                a[i] = v;
            }
            Some(Box::new(FiveZeroPattern::new(a, color)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bars(n: usize) -> Vec<Bar> {
        (0..n as i64).map(|i| Bar::new(i, 100.0, 101.0, 99.0, 100.5)).collect()
    }

    #[test]
    fn a_narrow_survey_range_lands_fully_inside_the_fixed_lookback_window() {
        let series = bars(2_000);
        // Survey [1_000, 1_050] — 50 bars, far narrower than the 800-bar
        // lookback: the window still reaches all the way back to
        // `end - EWA_SCAN_LOOKBACK_BARS`, but the LOCAL range must resolve
        // back to exactly [1_000, 1_050] once translated — the survey's own
        // requested range is preserved, not widened or clipped.
        let (window, local_range) = ewa_scan_window(&series, 1_000, 1_050);
        let window_start = 1_050 - EWA_SCAN_LOOKBACK_BARS;
        assert_eq!(window.len(), EWA_SCAN_LOOKBACK_BARS + 1);
        assert_eq!(local_range, (1_000 - window_start)..(1_050 - window_start + 1));
        assert_eq!(window.last().map(|b| b.timestamp), series.get(1_050).map(|b| b.timestamp));
    }

    #[test]
    fn a_wide_all_bars_survey_clamps_to_the_lookback_bound_on_a_long_series() {
        let series = bars(10_000);
        // Survey the WHOLE series (`coverage == "all_bars"` shape) —
        // [0, 9_999]. The window must clamp to the last
        // `EWA_SCAN_LOOKBACK_BARS` bars, not the whole 10,000-bar series
        // (the exact defect this fn exists to fix), and the local range
        // must clamp to 0 since the survey's own `start` (0) falls before
        // the window.
        let (window, local_range) = ewa_scan_window(&series, 0, 9_999);
        assert_eq!(window.len(), EWA_SCAN_LOOKBACK_BARS + 1);
        assert_eq!(local_range, 0..(EWA_SCAN_LOOKBACK_BARS + 1));
        // The window's own LAST bar must be the series' own last bar (the
        // survey's right edge is never clipped, only its left edge is).
        assert_eq!(window.last().map(|b| b.timestamp), series.last().map(|b| b.timestamp));
    }

    #[test]
    fn window_never_starts_before_bar_zero_on_a_short_series() {
        let series = bars(50);
        let (window, local_range) = ewa_scan_window(&series, 0, 49);
        assert_eq!(window.len(), 50);
        assert_eq!(local_range, 0..50);
    }

    /// Every named pivot as a distinct, strictly increasing (ts, price)
    /// pair — six is enough for every kind this function matches (a 4- or
    /// 5-point kind just reads a prefix).
    fn synthetic_points() -> Vec<(i64, f64)> {
        (0..6).map(|i| (i as i64 * 1_000, 100.0 + i as f64 * 10.0)).collect()
    }

    /// The 19 kinds this arc actually scans (8 Elliott + 11 harmonic — see
    /// this brief's own "Grounding correction"), each paired with the
    /// `type_id()` its own match arm must produce.
    const SCANNED_KINDS: &[(mli::ewa::types::EwaPatternKind, &str)] = {
        use mli::ewa::types::EwaPatternKind as K;
        &[
            (K::Impulse, "elliott_impulse"),
            (K::LeadingDiagonalContracting, "elliott_impulse"),
            (K::LeadingDiagonalExpanding, "elliott_impulse"),
            (K::EndingDiagonalContracting, "elliott_impulse"),
            (K::EndingDiagonalExpanding, "elliott_impulse"),
            (K::Zigzag, "elliott_correction"),
            (K::Flat, "elliott_correction"),
            (K::Triangle, "elliott_triangle"),
            (K::Gartley, "xabcd_pattern"),
            (K::Bat, "xabcd_pattern"),
            (K::AltBat, "xabcd_pattern"),
            (K::Butterfly, "xabcd_pattern"),
            (K::Crab, "xabcd_pattern"),
            (K::DeepCrab, "xabcd_pattern"),
            (K::Shark, "xabcd_pattern"),
            (K::Cypher, "cypher_pattern"),
            (K::Abcd, "abcd_pattern"),
            (K::ThreeDrives, "three_drives"),
            (K::FiveZero, "five_zero_pattern"),
        ]
    };

    #[test]
    fn every_scanned_kind_builds_the_expected_concrete_type_with_matching_points() {
        let points = synthetic_points();
        for &(kind, expected_type_id) in SCANNED_KINDS {
            let built = build_wave_primitive(kind, &points, "#2196F3", WaveDegree::Intermediate)
                .unwrap_or_else(|| panic!("{kind:?} must build a primitive"));
            assert_eq!(
                built.type_id(),
                expected_type_id,
                "{kind:?} should build a {expected_type_id}, got {}",
                built.type_id()
            );
            let built_points = built.points();
            if kind == mli::ewa::types::EwaPatternKind::ThreeDrives {
                // mlc's own 7th point is a repeat of the 6th pivot, never a
                // 7th pivot mli has — see `three_drives_from_pivots`'s own
                // doc for why.
                assert_eq!(&built_points[..6], &points[..6], "{kind:?}'s first six points must match the pivots given");
                assert_eq!(built_points[6], points[5], "{kind:?}'s 7th point must repeat the 6th");
            } else {
                let expected_len = built_points.len();
                assert_eq!(
                    built_points,
                    points[..expected_len],
                    "{kind:?}'s own points must match the pivots it was given"
                );
            }
        }
    }

    #[test]
    fn kinds_scan_waves_never_produces_yield_none() {
        use mli::ewa::types::EwaPatternKind as K;
        let points = synthetic_points();
        // `RegularFlat`/`ContractingTriangle` are deliberately NOT in this
        // list — they alias onto `Flat`/`Triangle`'s own arm and DO build,
        // covered by the positive test above instead.
        let excluded = [
            K::ImpulseExtendedWave1,
            K::ImpulseExtendedWave3,
            K::ImpulseExtendedWave5,
            K::TruncatedImpulse,
            K::RunningZigzag,
            K::DoubleZigzag,
            K::TripleZigzag,
            K::ExpandedFlat,
            K::RunningFlat,
            K::BarrierTriangle,
            K::ExpandingTriangle,
            K::RunningTriangle,
            K::DoubleCombo,
            K::DoubleThree,
            K::TripleCombo,
            K::TripleThree,
            K::Xabcd,
            K::Correction,
        ];
        for kind in excluded {
            assert!(
                build_wave_primitive(kind, &points, "#2196F3", WaveDegree::Intermediate).is_none(),
                "{kind:?} must build nothing — scan_waves never produces it"
            );
        }
    }
}
