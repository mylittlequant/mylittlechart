//! One-shot magic-hat animation math — pure functions only, no rendering.
//!
//! Two motions, each triggered once per tab-lifecycle event (owner ruling
//! 2026-09-10): [`tip_angle_rad`] for the tab-open "tip" (a rotation about
//! the hat's own brim bottom-center) and [`squash_scale_y`] for the
//! tab-close "squash" (a vertical scale, anchored at the icon box's bottom
//! edge by the caller). `toolbar_core.rs`'s "Bot" render arm is the only
//! caller — kept in its own module, independent of any `RenderContext`, so
//! the curve shapes stay unit-testable on their own.

/// Standard Penner ease-out-cubic: quick start, settles into the target.
/// Monotone non-decreasing over `[0, 1]`; `f(0) == 0.0`, `f(1) == 1.0`.
fn ease_out_cubic(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Standard Penner ease-in-out-cubic: slow start, slow end. Monotone
/// non-decreasing over `[0, 1]`; `f(0) == 0.0`, `f(1) == 1.0`.
fn ease_in_out_cubic(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Standard Penner ease-out-back: overshoots past the `1.0` target before
/// settling there. `f(0) == 0.0`, `f(1) == 1.0` — deliberately NOT monotone
/// (the overshoot bump is the entire point of this curve); used only for
/// the return legs below, where the overshoot reads as a little wobble.
fn ease_out_back(t: f64) -> f64 {
    const C1: f64 = 1.70158;
    const C3: f64 = C1 + 1.0;
    let t = t.clamp(0.0, 1.0) - 1.0;
    1.0 + C3 * t.powi(3) + C1 * t.powi(2)
}

/// Peak tip angle, degrees — negative rotates the crown away from the
/// brim's leading edge (owner spec: "rotate -14 deg").
const TIP_PEAK_DEG: f64 = -14.0;

/// Fraction of the animation spent tipping OUT (ease-out-cubic) before the
/// ease-out-back return leg takes over. The dip reads instantly; the
/// return — the leg the owner's spec explicitly asked to carry the
/// ease-out-back feel — still gets the majority of the duration.
const TIP_ATTACK_FRACTION: f64 = 0.35;

/// Rotation angle in RADIANS for the tab-open "tip" motion at elapsed
/// fraction `t` (`t = elapsed_ms / duration_ms`, expected in `[0, 1]` but
/// clamped defensively). Two legs, continuous at the seam: a quick
/// ease-out-cubic dip to [`TIP_PEAK_DEG`], then an ease-out-back return to
/// upright that overshoots slightly past 0 before settling — the "and
/// return" half of the owner's spec. `f(0) == 0.0` and `f(1) == 0.0`.
pub fn tip_angle_rad(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let peak_rad = TIP_PEAK_DEG.to_radians();
    if t < TIP_ATTACK_FRACTION {
        peak_rad * ease_out_cubic(t / TIP_ATTACK_FRACTION)
    } else {
        let t2 = (t - TIP_ATTACK_FRACTION) / (1.0 - TIP_ATTACK_FRACTION);
        peak_rad * (1.0 - ease_out_back(t2))
    }
}

/// Squash keyframe positions, as a fraction of the total duration.
const SQUASH_KEY1_T: f64 = 0.40;
const SQUASH_KEY2_T: f64 = 0.70;
/// Squash keyframe Y scales (owner spec: `1.0 -> 0.82 -> 1.04 -> 1.0`).
const SQUASH_MIN_SCALE: f64 = 0.82;
const SQUASH_REBOUND_SCALE: f64 = 1.04;

/// Vertical scale factor for the tab-close "squash" motion at elapsed
/// fraction `t` (`t = elapsed_ms / duration_ms`, expected in `[0, 1]` but
/// clamped defensively). Three ease-in-out-cubic segments, each monotone
/// on its own, continuous at both seams: squash down, rebound past rest,
/// settle back to rest. `f(0) == 1.0` and `f(1) == 1.0`.
pub fn squash_scale_y(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t < SQUASH_KEY1_T {
        let seg_t = t / SQUASH_KEY1_T;
        1.0 + (SQUASH_MIN_SCALE - 1.0) * ease_in_out_cubic(seg_t)
    } else if t < SQUASH_KEY2_T {
        let seg_t = (t - SQUASH_KEY1_T) / (SQUASH_KEY2_T - SQUASH_KEY1_T);
        SQUASH_MIN_SCALE + (SQUASH_REBOUND_SCALE - SQUASH_MIN_SCALE) * ease_in_out_cubic(seg_t)
    } else {
        let seg_t = (t - SQUASH_KEY2_T) / (1.0 - SQUASH_KEY2_T);
        SQUASH_REBOUND_SCALE + (1.0 - SQUASH_REBOUND_SCALE) * ease_in_out_cubic(seg_t)
    }
}

/// `magic_hat.svg`'s own viewBox dimensions (`width="197" height="142"`) —
/// duplicated here rather than parsed from the asset at render time,
/// because the pivot for the tab-open "tip" rotation must be known BEFORE
/// `uzor::render::draw_svg_multicolor` runs its own internal aspect-fit;
/// that helper exposes no way to ask in advance where a given viewBox
/// point will land on screen.
const HAT_VIEWBOX_W: f64 = 197.0;
const HAT_VIEWBOX_H: f64 = 142.0;

/// Brim bottom-center, in `magic_hat.svg`'s own viewBox coordinates — read
/// off the brim path's bounding box (`<path id="hat-brim">`, x in
/// `[5, 196]`, y in `[54, 135]`): center x ~100, bottom y ~135 (owner
/// spec: "about the brim's bottom-center").
const HAT_BRIM_PIVOT_VB: (f64, f64) = (100.0, 135.0);

/// Screen-space pivot for the tab-open "tip" rotation: the hat's own brim
/// bottom-center, mapped through the SAME aspect-fit
/// `draw_svg_multicolor` performs internally (fit `HAT_VIEWBOX_W x
/// HAT_VIEWBOX_H` into a `box_size x box_size` square, centered) for a hat
/// drawn at `(box_x, box_y)` with that box size.
pub fn hat_brim_pivot(box_x: f64, box_y: f64, box_size: f64) -> (f64, f64) {
    let scale = (box_size / HAT_VIEWBOX_W).min(box_size / HAT_VIEWBOX_H);
    let offset_x = box_x + (box_size - HAT_VIEWBOX_W * scale) / 2.0;
    let offset_y = box_y + (box_size - HAT_VIEWBOX_H * scale) / 2.0;
    (
        offset_x + HAT_BRIM_PIVOT_VB.0 * scale,
        offset_y + HAT_BRIM_PIVOT_VB.1 * scale,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    #[test]
    fn ease_out_cubic_endpoints_and_monotone() {
        assert!(ease_out_cubic(0.0).abs() < EPS);
        assert!((ease_out_cubic(1.0) - 1.0).abs() < EPS);
        let mut prev = ease_out_cubic(0.0);
        for i in 1..=20 {
            let v = ease_out_cubic(i as f64 / 20.0);
            assert!(v >= prev - EPS, "ease_out_cubic must be monotone non-decreasing");
            prev = v;
        }
    }

    #[test]
    fn ease_in_out_cubic_endpoints_and_monotone() {
        assert!(ease_in_out_cubic(0.0).abs() < EPS);
        assert!((ease_in_out_cubic(1.0) - 1.0).abs() < EPS);
        let mut prev = ease_in_out_cubic(0.0);
        for i in 1..=20 {
            let v = ease_in_out_cubic(i as f64 / 20.0);
            assert!(v >= prev - EPS, "ease_in_out_cubic must be monotone non-decreasing");
            prev = v;
        }
    }

    #[test]
    fn ease_out_back_endpoints_and_overshoot() {
        assert!(ease_out_back(0.0).abs() < EPS);
        assert!((ease_out_back(1.0) - 1.0).abs() < EPS);
        // The whole point of ease-out-back is an overshoot past the target
        // before settling — assert it actually happens somewhere in range.
        let overshoots = (1..20).any(|i| ease_out_back(i as f64 / 20.0) > 1.0);
        assert!(overshoots, "ease_out_back must overshoot past 1.0 before settling");
    }

    #[test]
    fn tip_angle_rad_endpoints() {
        assert!(tip_angle_rad(0.0).abs() < EPS);
        assert!(tip_angle_rad(1.0).abs() < EPS);
    }

    #[test]
    fn tip_angle_rad_dips_negative_during_attack_leg() {
        let dipped = (1..10).any(|i| tip_angle_rad(i as f64 * TIP_ATTACK_FRACTION / 10.0) < 0.0);
        assert!(dipped, "tip must rotate negative (TIP_PEAK_DEG < 0) during the attack leg");
    }

    #[test]
    fn squash_scale_y_endpoints() {
        assert!((squash_scale_y(0.0) - 1.0).abs() < EPS);
        assert!((squash_scale_y(1.0) - 1.0).abs() < EPS);
    }

    #[test]
    fn squash_scale_y_segments_monotone() {
        // Segment 1: squashing down — monotone decreasing.
        let mut prev = squash_scale_y(0.0);
        for i in 1..=8 {
            let v = squash_scale_y(i as f64 / 8.0 * SQUASH_KEY1_T);
            assert!(v <= prev + EPS, "squash-down segment must be monotone decreasing");
            prev = v;
        }
        // Segment 2: rebounding up past rest — monotone increasing.
        let mut prev = squash_scale_y(SQUASH_KEY1_T);
        for i in 1..=8 {
            let t = SQUASH_KEY1_T + i as f64 / 8.0 * (SQUASH_KEY2_T - SQUASH_KEY1_T);
            let v = squash_scale_y(t);
            assert!(v >= prev - EPS, "rebound segment must be monotone increasing");
            prev = v;
        }
        // Segment 3: settling back down to rest — monotone decreasing.
        let mut prev = squash_scale_y(SQUASH_KEY2_T);
        for i in 1..=8 {
            let t = SQUASH_KEY2_T + i as f64 / 8.0 * (1.0 - SQUASH_KEY2_T);
            let v = squash_scale_y(t);
            assert!(v <= prev + EPS, "settle segment must be monotone decreasing");
            prev = v;
        }
    }

    #[test]
    fn hat_brim_pivot_lands_inside_the_box() {
        let (px, py) = hat_brim_pivot(10.0, 20.0, 36.0);
        assert!((10.0..=46.0).contains(&px));
        assert!((20.0..=56.0).contains(&py));
    }
}
