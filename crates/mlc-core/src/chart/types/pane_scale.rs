//! Sub-pane Y-axis scale: range computation and label placement for
//! indicator sub-panes (RSI, Stochastic, MACD, CVD, ...).
//!
//! Owner rulings (2026-09-24, R24):
//! 1. A bounded oscillator (RSI, Stoch, Williams %R, MFI, ... — any
//!    indicator that declares a fixed `bounds` range) scales EXACTLY to
//!    those bounds. No value-space padding stretches `0..100` into
//!    `-5..105`; padding for label legibility is pixel-space only, applied
//!    at label-placement time by [`cull_axis_labels`]'s margin.
//! 2. An unbounded sub-pane (MACD histogram, CVD, ...) is symmetric around
//!    zero when the visible series crosses zero, with "nice" round-number
//!    ticks — never a raw linear interpolation of the data's own min/max
//!    (that produced ugly, uninformative labels like `-17.45`).
//! 3. An axis label that would not fit fully inside its own pane, or that
//!    collides with a reserved zone (a neighbouring pane's own labels at
//!    the shared boundary, a live value tag), is DROPPED rather than
//!    clipped or clamped into place.
//!
//! All functions here are pure — no rendering, no I/O — so the range and
//! culling logic is unit-testable without a `RenderContext`.

use super::price_scale::nice_price_step;

/// Compute the value range (`pane_min`, `pane_max`) for a sub-pane's Y-axis.
///
/// - `bounds`: the indicator's declared range (e.g. `Some((0.0, 100.0))`
///   for RSI). When present it is returned EXACTLY — ruling 1.
/// - `raw_min` / `raw_max`: the visible series' own min/max, used only when
///   `bounds` is `None`. Symmetrized around zero when the data crosses
///   zero (ruling 2), then given a small value-space padding so a plotted
///   line or histogram never touches the pane edge (bounded panes get no
///   such padding — their own bounds already ARE the edges).
pub fn sub_pane_value_range(bounds: Option<(f64, f64)>, raw_min: f64, raw_max: f64) -> (f64, f64) {
    if let Some((lo, hi)) = bounds {
        return (lo, hi);
    }

    let (mut lo, mut hi) = if raw_min < 0.0 && raw_max > 0.0 {
        let max_abs = raw_min.abs().max(raw_max.abs());
        (-max_abs, max_abs)
    } else {
        (raw_min, raw_max)
    };

    if hi > lo {
        let padding = (hi - lo) * 0.05;
        lo -= padding;
        hi += padding;
    }

    (lo, hi)
}

/// Generate "nice" round-number tick values strictly within
/// `[pane_min, pane_max]`.
///
/// Reuses the main price-scale's `nice_price_step` algorithm (the same
/// `[2, 2.5, 2]` pattern used by the main chart's own Y-axis) so sub-pane
/// axes read with the same round-number convention, instead of the raw
/// `pane_min + i/n * range` interpolation that produced labels like
/// `-17.45`. Sub-panes don't pan/overscan like the main price scale, so —
/// unlike [`super::price_scale::PriceScale::generate_ticks`] — ticks are
/// clamped to sit inside the pane's own value range, never beyond it.
pub fn sub_pane_ticks(pane_min: f64, pane_max: f64, target_count: usize) -> Vec<f64> {
    let range = pane_max - pane_min;
    if range <= 0.0 || target_count == 0 {
        return Vec::new();
    }

    let step = nice_price_step(range, target_count as f64);
    if step <= 0.0 {
        return Vec::new();
    }

    let first = (pane_min / step).ceil() * step;
    let num_ticks = ((pane_max - first) / step).ceil() as i64 + 1;
    let tolerance = step * 1e-9;

    let mut ticks = Vec::new();
    for i in 0..num_ticks.max(0) {
        let value = first + (i as f64) * step;
        if value > pane_max + tolerance {
            break;
        }
        if value >= pane_min - tolerance {
            ticks.push(value);
        }
    }
    ticks
}

/// A candidate axis label: a value with its already-resolved on-pane pixel
/// Y coordinate (vertical center of the text).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisLabel {
    pub value: f64,
    pub y: f64,
}

/// Decide which candidate labels survive the "never clip, never overlap"
/// rule (ruling 3).
///
/// - `pane_top` / `pane_bottom`: the pane's own vertical extent in pixels.
///   A label whose vertical footprint (`y - half_h ..= y + half_h`) crosses
///   either edge is dropped — this is exactly what used to render sliced in
///   half by the pane's own clip rect.
/// - `reserved`: extra pixel intervals that must stay clear (e.g. a live
///   value tag). Empty when there is none.
/// - `half_label_height`: half the label's own pixel footprint, including
///   whatever legibility margin the caller wants — this margin is where
///   "pixel padding" for a bounded oscillator's `0`/`100` extremes lives,
///   per ruling 1.
///
/// Candidates are considered in the given order; once accepted, a label's
/// own footprint is added to the reserved set for labels considered after
/// it, so two ticks never overlap each other either. Because every pane
/// applies this same edge margin independently, two adjacent panes' own
/// labels can never reach far enough to collide across the shared
/// boundary — no cross-pane coordination is needed.
pub fn cull_axis_labels(
    candidates: &[AxisLabel],
    pane_top: f64,
    pane_bottom: f64,
    reserved: &[(f64, f64)],
    half_label_height: f64,
) -> Vec<AxisLabel> {
    let mut occupied: Vec<(f64, f64)> = reserved.to_vec();
    let mut kept = Vec::new();

    for candidate in candidates {
        let top = candidate.y - half_label_height;
        let bottom = candidate.y + half_label_height;

        if top < pane_top || bottom > pane_bottom {
            continue;
        }
        if occupied.iter().any(|&(a, b)| intervals_overlap(a, b, top, bottom)) {
            continue;
        }

        occupied.push((top, bottom));
        kept.push(*candidate);
    }

    kept
}

#[inline]
fn intervals_overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> bool {
    a0 < b1 && b0 < a1
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- sub_pane_value_range ------------------------------------------

    #[test]
    fn bounded_range_ignores_raw_data_and_padding() {
        // RSI-style: declared 0..100 bounds must come back EXACT, no
        // matter what the visible data's own min/max looks like.
        let (lo, hi) = sub_pane_value_range(Some((0.0, 100.0)), 42.0, 57.0);
        assert_eq!((lo, hi), (0.0, 100.0));
    }

    #[test]
    fn unbounded_range_crossing_zero_is_symmetric() {
        let (lo, hi) = sub_pane_value_range(None, -3.0, 10.0);
        assert_eq!(lo, -hi, "range must be symmetric around zero");
        // max_abs = 10, padded 5% => 11
        assert!((hi - 11.0).abs() < 1e-9);
    }

    #[test]
    fn unbounded_range_not_crossing_zero_gets_value_padding() {
        let (lo, hi) = sub_pane_value_range(None, 5.0, 15.0);
        // range=10, 5% padding=0.5 each side
        assert!((lo - 4.5).abs() < 1e-9);
        assert!((hi - 15.5).abs() < 1e-9);
    }

    #[test]
    fn degenerate_raw_range_does_not_panic_or_invert() {
        let (lo, hi) = sub_pane_value_range(None, 5.0, 5.0);
        assert_eq!((lo, hi), (5.0, 5.0));
    }

    // -- sub_pane_ticks --------------------------------------------------

    #[test]
    fn bounded_ticks_hit_both_extremes() {
        let ticks = sub_pane_ticks(0.0, 100.0, 3);
        assert!(ticks.first().is_some_and(|&v| v == 0.0));
        assert!(ticks.last().is_some_and(|&v| v == 100.0));
        // All values are "nice" round numbers, never a raw fraction.
        for &v in &ticks {
            assert_eq!(v, v.round(), "tick {v} is not a round number");
        }
    }

    #[test]
    fn ticks_never_escape_the_pane_range() {
        let ticks = sub_pane_ticks(-17.3, 42.8, 4);
        for &v in &ticks {
            assert!(v >= -17.3 - 1e-6 && v <= 42.8 + 1e-6, "tick {v} escaped [-17.3, 42.8]");
        }
    }

    #[test]
    fn symmetric_range_includes_zero_tick() {
        let ticks = sub_pane_ticks(-1582.0, 1582.0, 2);
        assert!(ticks.iter().any(|&v| v == 0.0), "zero must be a tick when the range straddles it: {ticks:?}");
    }

    #[test]
    fn degenerate_or_empty_range_yields_no_ticks() {
        assert!(sub_pane_ticks(10.0, 10.0, 3).is_empty());
        assert!(sub_pane_ticks(0.0, 100.0, 0).is_empty());
    }

    // -- cull_axis_labels --------------------------------------------------

    #[test]
    fn label_crossing_top_or_bottom_edge_is_dropped() {
        let candidates = [
            AxisLabel { value: 100.0, y: 3.0 },  // top: footprint [-2, 8], crosses pane_top=0
            AxisLabel { value: 50.0, y: 50.0 },  // safely inside
            AxisLabel { value: 0.0, y: 97.0 },   // bottom: footprint [92, 102], crosses pane_bottom=100
        ];
        let kept = cull_axis_labels(&candidates, 0.0, 100.0, &[], 5.0);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].value, 50.0);
    }

    #[test]
    fn colliding_labels_keep_only_the_first() {
        let candidates = [
            AxisLabel { value: 1.0, y: 50.0 },
            AxisLabel { value: 2.0, y: 52.0 }, // overlaps the first at half_h=5
            AxisLabel { value: 3.0, y: 70.0 }, // clear of both
        ];
        let kept = cull_axis_labels(&candidates, 0.0, 100.0, &[], 5.0);
        let values: Vec<f64> = kept.iter().map(|l| l.value).collect();
        assert_eq!(values, vec![1.0, 3.0]);
    }

    #[test]
    fn reserved_zone_blocks_overlapping_labels() {
        // Simulates a live value tag reserving [40, 60].
        let candidates = [AxisLabel { value: 1.0, y: 50.0 }];
        let kept = cull_axis_labels(&candidates, 0.0, 100.0, &[(40.0, 60.0)], 5.0);
        assert!(kept.is_empty());
    }

    #[test]
    fn non_colliding_labels_all_survive() {
        let candidates = [
            AxisLabel { value: 0.0, y: 10.0 },
            AxisLabel { value: 1.0, y: 50.0 },
            AxisLabel { value: 2.0, y: 90.0 },
        ];
        let kept = cull_axis_labels(&candidates, 0.0, 100.0, &[], 5.0);
        assert_eq!(kept.len(), 3);
    }
}
