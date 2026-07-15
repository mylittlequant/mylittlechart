//! Instrument-relative default resolver for `ResolutionKind::Param` chart
//! types whose registry `inline_default` is a static fallback ONLY
//! (granularity arc, кусок 2 —
//! `docs/mlc/research/industry-resolution-defaults-2026-07-15.md`
//! "Recommended defaults" §2/§8).
//!
//! `ChartTypeDef::default_param_value()` answers "what does the registry say
//! before the user has picked anything" — a single static number, correct
//! for a BTCUSDT-scale instrument but meaningless for a $0.0001-tick low-cap
//! alt (range_bar) or an instrument whose native units are worth orders of
//! magnitude more/less than $1 (volume_bar). [`resolve_instrument_relative_default`]
//! is the live, per-instrument override: called at subscribe time with the
//! ACTIVE instrument's `tick_size`/`last_price`, it returns `Some(value)` for
//! the two families that need it and `None` for everything else (the caller
//! keeps `default_param_value()` as its fallback in that case, unchanged
//! behavior for every type this doesn't apply to).
//!
//! Pure function, no IO — callers resolve `tick_size`/`last_price` themselves
//! (mirrors `ChartApp::resolve_tick_size`'s existing fallback chain) and
//! write the resolved value back via `set_chart_type_param` BEFORE building
//! subscribe params, so the toolbar button and the settings-modal both show
//! the REAL resolved number instead of re-deriving it independently.

use super::registry::ChartTypeDef;

/// Nearest-value snap onto a ladder of tick-count options — used by
/// range_bar to pick the closest published rung to the ideal instrument-
/// relative tick count, rather than subscribing an arbitrary un-curated
/// number. Ties break toward the SMALLER (earlier) rung — `partial_cmp`
/// finds the first minimal distance, and the ladder is ascending, so the
/// first equal-distance candidate encountered is the smaller one.
fn snap_to_nearest(ideal: f64, ladder: &[f64]) -> Option<f64> {
    ladder
        .iter()
        .copied()
        .min_by(|a, b| {
            (a - ideal).abs().partial_cmp(&(b - ideal).abs()).unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// Round a positive value to the nearest "1-2-5 mantissa" — the classic
/// engineering-notation preferred-number series (1, 2, 5, 10, 20, 50, ...)
/// used for volume_bar's notional-derived threshold so the resolved value
/// reads as a clean round number instead of a raw division result (e.g.
/// `100_000.0 / 65_000.0 == 1.538...` rounds to `2.0`, not "1.54V").
fn round_to_1_2_5(value: f64) -> f64 {
    if !(value > 0.0) || !value.is_finite() {
        return value;
    }
    let exponent = value.log10().floor();
    let magnitude = 10f64.powf(exponent);
    let mantissa = value / magnitude;
    let snapped_mantissa = if mantissa < 1.5 {
        1.0
    } else if mantissa < 3.5 {
        2.0
    } else if mantissa < 7.5 {
        5.0
    } else {
        10.0
    };
    snapped_mantissa * magnitude
}

/// Resolve the LIVE, instrument-relative default for a `ResolutionKind::
/// Param` chart type's numeric parameter — `None` for every chart type
/// other than `range_bar`/`volume_bar` (the caller falls back to
/// [`ChartTypeDef::default_param_value`] in that case, exactly as before
/// this resolver existed).
///
/// - **`range_bar`**: target bar height ≈ 0.075% of `last_price`
///   (industry-survey midpoint of the "0.05-0.1% of price" range cited for
///   BTCUSDT-scale instruments) → `ideal_ticks = last_price * 0.00075 /
///   tick_size`, snapped to the nearest rung in `RANGE_BAR_OPTIONS`'s ladder
///   (10/25/50/100/250/500/1000). Requires `tick_size > 0.0 && last_price >
///   0.0`, else `None`.
/// - **`volume_bar`**: threshold = `100_000.0 / last_price` (the same
///   $100K notional-per-bar convention as `dollar_bar`'s shipped default,
///   converted to native units at the instrument's live price), rounded to
///   a 1-2-5 mantissa. Requires `last_price > 0.0`, else `None`.
/// - Every other chart type: `None` unconditionally.
pub fn resolve_instrument_relative_default(
    def: &ChartTypeDef,
    tick_size: f64,
    last_price: f64,
) -> Option<f64> {
    match def.id {
        "range_bar" => {
            if !(tick_size > 0.0) || !(last_price > 0.0) {
                return None;
            }
            let ideal_ticks = last_price * 0.00075 / tick_size;
            const RANGE_BAR_LADDER: &[f64] = &[10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0];
            snap_to_nearest(ideal_ticks, RANGE_BAR_LADDER)
        }
        "volume_bar" => {
            if !(last_price > 0.0) {
                return None;
            }
            Some(round_to_1_2_5(100_000.0 / last_price))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart_type::by_id;

    #[test]
    fn range_bar_btc_scale_snaps_to_500() {
        let def = by_id("range_bar").unwrap();
        // 65_000 * 0.00075 / 0.1 == 487.5 -> nearest rung is 500.
        let resolved = resolve_instrument_relative_default(def, 0.1, 65_000.0);
        assert_eq!(resolved, Some(500.0));
    }

    #[test]
    fn volume_bar_btc_scale_resolves_to_2() {
        let def = by_id("volume_bar").unwrap();
        // 100_000 / 65_000 == 1.538... -> 1-2-5 snap rounds to 2.0.
        let resolved = resolve_instrument_relative_default(def, 0.0, 65_000.0);
        assert_eq!(resolved, Some(2.0));
    }

    #[test]
    fn range_bar_low_cap_scale_snaps_to_10() {
        let def = by_id("range_bar").unwrap();
        // 0.5 * 0.00075 / 0.0001 == 3.75 -> nearest rung is 10 (25 is farther: |3.75-10|=6.25 < |3.75-25|=21.25).
        let resolved = resolve_instrument_relative_default(def, 0.0001, 0.5);
        assert_eq!(resolved, Some(10.0));
    }

    #[test]
    fn range_bar_garbage_inputs_resolve_to_none() {
        let def = by_id("range_bar").unwrap();
        assert_eq!(resolve_instrument_relative_default(def, 0.0, 65_000.0), None);
        assert_eq!(resolve_instrument_relative_default(def, 0.1, 0.0), None);
        assert_eq!(resolve_instrument_relative_default(def, -1.0, 65_000.0), None);
        assert_eq!(resolve_instrument_relative_default(def, 0.1, -1.0), None);
    }

    #[test]
    fn volume_bar_garbage_inputs_resolve_to_none() {
        let def = by_id("volume_bar").unwrap();
        assert_eq!(resolve_instrument_relative_default(def, 0.1, 0.0), None);
        assert_eq!(resolve_instrument_relative_default(def, 0.1, -1.0), None);
    }

    #[test]
    fn every_other_chart_type_resolves_to_none() {
        for d in crate::chart_type::all() {
            if matches!(d.id, "range_bar" | "volume_bar") {
                continue;
            }
            assert_eq!(
                resolve_instrument_relative_default(d, 0.1, 65_000.0),
                None,
                "{} must resolve to None (not an instrument-relative type)",
                d.id
            );
        }
    }
}
