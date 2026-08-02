//! Bar-aligned series for the host-rendered sub-pane indicators.
//!
//! OI Delta and Funding Rate are ordinary `IndicatorInstance`s now, but they
//! carry `machine_id: None`, so the compute pass produces nothing for them
//! and the sub-pane's generic output loop has nothing to draw. Their data
//! arrives on the BUBBLE instead — open interest as REST history keyed by
//! venue, funding as a sparse event list — and neither is on the chart's bar
//! grid.
//!
//! This pass resamples both onto that grid and publishes them as an ordinary
//! output, so they render as a LINE through the same path RSI and every other
//! sub-pane indicator uses, instead of a bespoke painter.
//!
//! ## Why open interest does not come from the live stream
//!
//! Binance publishes no websocket open-interest channel at all (KuCoin and
//! HTX likewise), so an OI pane fed by live events stays empty forever on the
//! venue most charts sit on — measured 2026-08-03: `oi_points: 0` with the
//! subscription up and healthy. REST history is the source every venue that
//! matters actually serves, and it is already being fetched.
//!
//! ## Resampling rules follow the nature of the quantity
//!
//! * Funding is a STATE: it holds between events, so it carries forward and
//!   bars before the first event are blank.
//! * Open-interest DELTA is a FLOW: it is the change between consecutive
//!   readings, so a bar with no reading is zero, not the previous change
//!   repeated.

use mlc_core::ids::BubbleId;

/// `f64::NAN` marks "no value at this bar" for the renderer — the same
/// convention the compute indicators use for their warmup region.
const BLANK: f64 = f64::NAN;

impl crate::ChartApp {
    /// Publish bar-aligned series for every live sub-pane indicator.
    pub(crate) fn refresh_subpane_series(&mut self) {
        let targets: Vec<(BubbleId, String)> = {
            let mut seen: std::collections::HashSet<BubbleId> = std::collections::HashSet::new();
            self.panel_app
                .panel_grid
                .containers()
                .keys()
                .copied()
                .collect::<Vec<_>>()
                .into_iter()
                .filter_map(|cid| {
                    let bid = self.bubble_for_chart(cid)?;
                    if !seen.insert(bid) {
                        return None;
                    }
                    Some((bid, self.exchange_id_for_chart(cid).as_str().to_string()))
                })
                .collect()
        };

        for (bid, own_venue) in targets {
            self.publish_oi_delta_series(bid, &own_venue);
            self.publish_funding_series(bid);
        }
    }

    /// Instance ids of `type_id` on `bid` that are visible.
    fn live_instance_ids(&self, bid: BubbleId, type_id: &str) -> Vec<u64> {
        self.core
            .indicators()
            .instances_for_bubble(bid)
            .into_iter()
            .filter(|inst| inst.type_id == type_id && inst.visible)
            .map(|inst| inst.id)
            .collect()
    }

    /// Open-interest CHANGE per bar, from the venue's REST history.
    fn publish_oi_delta_series(&mut self, bid: BubbleId, own_venue: &str) {
        let ids = self.live_instance_ids(bid, Self::OI_DELTA_TYPE_ID);
        if ids.is_empty() {
            return;
        }

        // (bar_ts, series) read under one bubble borrow.
        let series = self.core.bubbles_mut().with_bubble_mut(bid, |bubble| {
            if bubble.bars.is_empty() {
                return None;
            }
            // The chart's own venue first; any venue that answered otherwise,
            // so a chart on a venue with no OI history still shows the shape
            // of the market rather than an empty pane.
            let rows: &[(i64, f64)] = bubble
                .oi_hist
                .get(own_venue)
                .map(|v| v.as_slice())
                .or_else(|| bubble.oi_hist.values().next().map(|v| v.as_slice()))?;
            if rows.len() < 2 {
                return None;
            }
            Some(resample_delta(&bubble.bars, rows))
        }).flatten();
        let Some(series) = series else { return };

        for id in ids {
            self.core.indicators_mut().publish_values(id, "value", series.clone());
        }
    }

    /// Funding rate per bar in BASIS POINTS, carried forward between events.
    fn publish_funding_series(&mut self, bid: BubbleId) {
        let ids = self.live_instance_ids(bid, Self::FUNDING_RATE_TYPE_ID);
        if ids.is_empty() {
            return;
        }

        let series = self.core.bubbles_mut().with_bubble_mut(bid, |bubble| {
            if bubble.bars.is_empty() {
                return None;
            }
            let cells = bubble.funding_rate.as_ref()?;
            if cells.is_empty() {
                return None;
            }
            let points: Vec<(i64, f64)> = cells
                .iter()
                .filter(|c| c.rate.is_finite())
                // Basis points: a raw perp funding rate is ~1e-4, which on a
                // shared axis with anything else is a flat line at zero.
                .map(|c| (c.ts_ms, c.rate * 10_000.0))
                .collect();
            Some(resample_hold(&bubble.bars, &points))
        }).flatten();
        let Some(series) = series else { return };

        for id in ids {
            self.core.indicators_mut().publish_values(id, "value", series.clone());
        }
    }
}

/// Index of the last bar at or before `ts_ms`.
fn bar_index_at(bars: &[mlc_core::Bar], ts_ms: i64) -> Option<usize> {
    if bars.is_empty() || bars[0].ts_ms() > ts_ms {
        return None;
    }
    let mut lo = 0usize;
    let mut hi = bars.len();
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if bars[mid].ts_ms() <= ts_ms {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(lo)
}

/// STATE resample: the last value at or before each bar, blank before the
/// first reading. Funding holds between settlements, so a bar inherits the
/// rate in force during it.
fn resample_hold(bars: &[mlc_core::Bar], points: &[(i64, f64)]) -> Vec<f64> {
    let mut out = vec![BLANK; bars.len()];
    if points.is_empty() {
        return out;
    }
    let mut cursor = 0usize;
    let mut held: Option<f64> = None;
    for (i, bar) in bars.iter().enumerate() {
        let bar_ts = bar.ts_ms();
        while cursor < points.len() && points[cursor].0 <= bar_ts {
            held = Some(points[cursor].1);
            cursor += 1;
        }
        if let Some(v) = held {
            out[i] = v;
        }
    }
    out
}

/// FLOW resample: the change between consecutive readings, accumulated into
/// the bar each reading falls in. A bar with no reading is 0.0 — no change
/// was observed, which is not the same as repeating the previous change.
fn resample_delta(bars: &[mlc_core::Bar], rows: &[(i64, f64)]) -> Vec<f64> {
    let mut out = vec![BLANK; bars.len()];
    let mut prev: Option<f64> = None;
    let mut touched = vec![false; bars.len()];
    for (ts, oi) in rows {
        if !oi.is_finite() || *oi <= 0.0 {
            continue;
        }
        let d = match prev {
            Some(p) => *oi - p,
            None => 0.0,
        };
        prev = Some(*oi);
        if let Some(idx) = bar_index_at(bars, *ts) {
            let slot = &mut out[idx];
            if touched[idx] {
                *slot += d;
            } else {
                *slot = d;
                touched[idx] = true;
            }
        }
    }
    // Bars inside the covered span with no reading saw no change.
    if let (Some(first), Some(last)) = (
        touched.iter().position(|t| *t),
        touched.iter().rposition(|t| *t),
    ) {
        for (i, slot) in out.iter_mut().enumerate().take(last + 1).skip(first) {
            if !touched[i] {
                *slot = 0.0;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlc_core::Bar;

    /// `Bar::new` takes SECONDS; `ts_ms()` scales.
    fn bars(n: usize) -> Vec<Bar> {
        (0..n).map(|i| Bar::new(i as i64 * 60, 1.0, 1.0, 1.0, 1.0)).collect()
    }

    #[test]
    fn funding_holds_between_events_and_is_blank_before_the_first() {
        let b = bars(5);
        // Event on bar 1 only.
        let out = resample_hold(&b, &[(60_000, 1.5)]);
        assert!(out[0].is_nan(), "no rate was in force before the first event");
        assert_eq!(&out[1..], &[1.5, 1.5, 1.5, 1.5]);
    }

    #[test]
    fn oi_delta_is_a_change_per_bar_and_zero_where_nothing_was_read() {
        let b = bars(5);
        // Readings on bars 0, 1 and 4.
        let out = resample_delta(&b, &[(0, 100.0), (60_000, 130.0), (240_000, 120.0)]);
        assert_eq!(out[0], 0.0, "first reading has no predecessor");
        assert_eq!(out[1], 30.0);
        assert_eq!(out[2], 0.0, "covered by the span, but nothing was read");
        assert_eq!(out[3], 0.0);
        assert_eq!(out[4], -10.0);
    }

    #[test]
    fn two_readings_inside_one_bar_sum_into_it() {
        let b = bars(2);
        let out = resample_delta(&b, &[(0, 100.0), (10_000, 110.0), (20_000, 105.0)]);
        // +10 then -5, both inside bar 0.
        assert_eq!(out[0], 5.0);
    }
}
