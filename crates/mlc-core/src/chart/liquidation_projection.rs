//! Projected liquidation levels — where forced closes are LIKELY to fire.
//!
//! The realized layer (`chart::liquidations`) draws what already happened.
//! This one draws what is still loaded: the price rows a leveraged position
//! opened earlier would die at, and it is a MODEL, not data. No exchange
//! publishes other traders' positions; every reference tool that shows these
//! bands (CoinGlass, Hyblock) computes them the same way, from open interest.
//!
//! ## The derivation
//!
//! 1. Open interest that APPEARED during a bar is new leveraged exposure
//!    opened around that bar's price. `ΔOI × price` is its notional; a fall
//!    in OI is exposure leaving and creates nothing.
//! 2. That notional is split long/short (`long_share`) and spread over a
//!    leverage distribution — a set of (leverage, weight) tiers, because
//!    traders do not all pick the same leverage and the distribution is the
//!    knob the whole picture is tuned with.
//! 3. Each slice dies at
//!    `long:  P × (1 − 1/L + MMR)`
//!    `short: P × (1 + 1/L − MMR)`
//!    where MMR is the maintenance-margin rate (≈0.4 % on a major perp's
//!    lowest notional tier; it is tiered by size on every venue, and this
//!    takes it as one parameter rather than pretending to know the tier).
//! 4. Slices are grouped into price rows (`row_pct` of price) so a cluster
//!    reads as a band instead of a thousand hairlines.
//! 5. A row is CONSUMED the first time a later bar trades through it: those
//!    positions are gone. That is why the reference's bands END rather than
//!    wash the whole chart — an uneaten band is standing liquidity, an eaten
//!    one is history. Consumption ENDS the row, so exposure that rebuilds at
//!    the same price afterwards is a NEW band with its own start: a level is
//!    a segment in time, not a permanent property of a price.
//!
//! ## What it is honestly not
//!
//! The leverage distribution is an assumption. On thin open interest the
//! picture is noise (which is why the reference is credible on BTC/ETH and
//! not on small caps), and the model cannot see positions opened before the
//! OI series starts. Read it as "where pressure would build", never as a
//! list of orders.

use crate::types::OiDeltaBar;
use crate::Bar;

/// One leverage bucket of the assumed distribution.
#[derive(Clone, Copy, Debug)]
pub struct LeverageTier {
    /// Leverage multiple (25.0 = 25×).
    pub leverage: f64,
    /// Share of new exposure assumed to sit at this leverage. Weights are
    /// normalised, so they need not sum to 1.
    pub weight: f64,
}

/// Inputs of the projection, all owner-reachable as indicator parameters.
#[derive(Clone, Debug)]
pub struct ProjectionParams {
    /// Assumed leverage distribution.
    pub tiers: Vec<LeverageTier>,
    /// Maintenance margin rate (0.004 = 0.4 %).
    pub mmr: f64,
    /// Price row height as a PERCENT of price.
    pub row_pct: f64,
    /// Share of new exposure assumed long (0.5 = balanced).
    pub long_share: f64,
    /// Rows holding less than this notional are dropped before rendering.
    pub min_usd: f64,
}

impl Default for ProjectionParams {
    fn default() -> Self {
        Self {
            // A deliberately wide spread: retail perp flow concentrates at
            // 10–50×, with a long tail at 100× and a floor of low-leverage
            // size that rarely dies.
            tiers: vec![
                LeverageTier { leverage: 5.0, weight: 0.10 },
                LeverageTier { leverage: 10.0, weight: 0.25 },
                LeverageTier { leverage: 25.0, weight: 0.30 },
                LeverageTier { leverage: 50.0, weight: 0.25 },
                LeverageTier { leverage: 100.0, weight: 0.10 },
            ],
            mmr: 0.004,
            row_pct: 0.05,
            long_share: 0.5,
            min_usd: 0.0,
        }
    }
}

/// One projected price row.
#[derive(Clone, Copy, Debug)]
pub struct ProjectedLevel {
    /// Row centre price.
    pub price: f64,
    /// When the exposure behind it was opened (earliest contributing bar).
    pub from_ts: i64,
    /// When price first traded through the row, or `None` while it stands.
    pub consumed_ts: Option<i64>,
    /// Notional still parked at this row, in quote currency.
    pub notional: f64,
    /// `1` = longs die here (row below the entries), `0` = shorts.
    pub side: u8,
}

impl ProjectedLevel {
    /// Whether price has already traded through this row.
    #[inline]
    pub fn consumed(&self) -> bool {
        self.consumed_ts.is_some()
    }
}

/// Fold per-venue open-interest histories into ONE series of deltas.
///
/// Aggregating open interest is the default question, not a variant of it:
/// a position opened on one venue is liquidated on that venue, but price is
/// one market, and a projection built from a single venue's OI understates
/// every level by whatever share that venue holds. Summing is only valid on
/// a COMMON time grid — venues publish on the same ladder (5m/15m/1h/…) but
/// not at identical timestamps, so rows are bucketed to `grid_ms` first and
/// a venue missing a bucket carries its previous value forward rather than
/// dropping to zero (which would read as a mass close that never happened).
pub fn fold_oi(
    per_venue: &[(String, Vec<(i64, f64)>)],
    grid_ms: i64,
) -> Vec<OiDeltaBar> {
    let grid = grid_ms.max(1);
    let mut buckets: std::collections::BTreeMap<i64, Vec<(usize, f64)>> =
        std::collections::BTreeMap::new();
    for (vi, (_, rows)) in per_venue.iter().enumerate() {
        for (ts, oi) in rows {
            if !oi.is_finite() || *oi <= 0.0 {
                continue;
            }
            buckets.entry((ts / grid) * grid).or_default().push((vi, *oi));
        }
    }
    let mut last: Vec<Option<f64>> = vec![None; per_venue.len()];
    let mut out: Vec<OiDeltaBar> = Vec::with_capacity(buckets.len());
    let mut prev_total: Option<f64> = None;
    for (ts, samples) in buckets {
        for (vi, oi) in samples {
            last[vi] = Some(oi);
        }
        // Only venues that have reported at least once contribute; a venue
        // that never answers is absent, not zero.
        let total: f64 = last.iter().filter_map(|v| *v).sum();
        if total <= 0.0 {
            continue;
        }
        let delta = match prev_total {
            Some(p) => total - p,
            None => 0.0,
        };
        prev_total = Some(total);
        out.push(OiDeltaBar { ts_ms: ts, oi: total, delta });
    }
    out
}

/// Row key: quantised price plus side, so long and short pressure at the
/// same price stay distinct (they are opposite forces, not one pool).
///
/// `grid` is ONE absolute step for the whole projection, derived from a
/// reference price rather than from each contributing price. A per-point
/// step made the index unrecoverable — two points at different prices
/// produced different steps, so the same index meant two different prices
/// and rows collapsed into each other (2026-08-02: 500 open-interest rows
/// yielded 10 levels).
#[inline]
fn row_key(price: f64, grid: f64, side: u8) -> (i64, u8) {
    ((price / grid).round() as i64, side)
}

/// Release every standing row this bar's range trades through.
///
/// A row is eaten by the FIRST bar whose `[low, high]` contains it: those
/// positions are liquidated and the row stops existing. It is removed from
/// the live set, so exposure that arrives at the same price afterwards opens
/// a NEW row with its own start — that is the difference between "this level
/// was standing from here to here" and one band smeared across the window.
fn eat_bar(
    live: &mut std::collections::BTreeMap<(i64, u8), (f64, i64)>,
    out: &mut Vec<ProjectedLevel>,
    bar: &Bar,
    grid: f64,
    min_usd: f64,
) {
    if live.is_empty() || !(bar.low > 0.0) || !(bar.high >= bar.low) {
        return;
    }
    // Rows whose CENTRE the bar traded through — the same test the row grid
    // is built with (`row_key` rounds, so row `i` is centred on `i × grid`).
    let lo = (bar.low / grid).ceil() as i64;
    let hi = (bar.high / grid).floor() as i64;
    if hi < lo {
        return;
    }
    let ts = bar.ts_ms();
    let keys: Vec<(i64, u8)> = live.range((lo, 0u8)..=(hi, u8::MAX)).map(|(k, _)| *k).collect();
    for k in keys {
        let Some((notional, from_ts)) = live.remove(&k) else { continue };
        if notional < min_usd {
            continue;
        }
        let price = k.0 as f64 * grid;
        if !(price > 0.0) {
            continue;
        }
        out.push(ProjectedLevel {
            price,
            from_ts,
            consumed_ts: Some(ts),
            notional,
            side: k.1,
        });
    }
}

/// Build the projected levels for `bars` from the open-interest series.
///
/// Walks open interest and bars TOGETHER on one clock. Exposure that arrives
/// at a point in time opens rows; every bar that closes after them eats the
/// rows it trades through and ends them there. A row is therefore a SEGMENT
/// — it starts when its exposure was opened and stops where price took it —
/// and the same price can hold several segments over the window.
///
/// The previous revision folded every contribution to one price into a
/// single row stamped with the EARLIEST contributing timestamp, then asked
/// once whether price had ever traded through it. Two defects followed and
/// both were visible on screen (owner report 2026-08-02): every band began
/// at the left edge of the window regardless of when its exposure actually
/// appeared, and a level eaten early was marked consumed forever, so
/// exposure rebuilt at that price afterwards was invisible.
///
/// `oi` must be time-ordered; bars likewise. Cost is `O(oi × tiers × log r +
/// bars × log r)` — one pass over each, no rescan per row.
pub fn project(bars: &[Bar], oi: &[OiDeltaBar], params: &ProjectionParams) -> Vec<ProjectedLevel> {
    if bars.is_empty() || oi.len() < 2 || params.tiers.is_empty() {
        return Vec::new();
    }
    let row_pct = params.row_pct.max(1e-6) / 100.0;
    let mmr = params.mmr.clamp(0.0, 0.5);
    let long_share = params.long_share.clamp(0.0, 1.0);
    let weight_sum: f64 = params.tiers.iter().map(|t| t.weight.max(0.0)).sum();
    if weight_sum <= 0.0 {
        return Vec::new();
    }

    // ONE absolute row step for the whole projection, taken from the last
    // bar's close: the grid has to be invertible, so `row_idx × grid` must
    // name exactly one price no matter which bar contributed to it.
    let reference_price = bars.last().map(|b| b.close).unwrap_or(0.0);
    if !(reference_price > 0.0) {
        return Vec::new();
    }
    let grid = (reference_price * row_pct).max(f64::MIN_POSITIVE);

    // Rows currently STANDING: (row, side) → (notional, opened at).
    let mut live: std::collections::BTreeMap<(i64, u8), (f64, i64)> =
        std::collections::BTreeMap::new();
    let mut out: Vec<ProjectedLevel> = Vec::new();
    let mut cursor = 0usize;

    for point in oi.iter() {
        // Bars that closed at or before this point eat first: exposure opened
        // now cannot be liquidated by a candle that is already history.
        while cursor < bars.len() && bars[cursor].ts_ms() <= point.ts_ms {
            eat_bar(&mut live, &mut out, &bars[cursor], grid, params.min_usd);
            cursor += 1;
        }
        // Only exposure that ARRIVED creates future liquidations. Exposure
        // leaving is a position closed — it takes its own level with it, and
        // we cannot know which one, so it is not modelled as a negative.
        if !(point.delta > 0.0) || !point.oi.is_finite() {
            continue;
        }
        let Some(price) = price_at(bars, point.ts_ms) else { continue };
        if !(price > 0.0) {
            continue;
        }
        let notional = point.delta * price;
        if !notional.is_finite() || notional <= 0.0 {
            continue;
        }
        for tier in &params.tiers {
            let l = tier.leverage;
            let w = tier.weight.max(0.0);
            if !(l > 1.0) || w <= 0.0 {
                continue;
            }
            let share = w / weight_sum;
            // Long side dies below, short side above.
            let long_price = price * (1.0 - 1.0 / l + mmr);
            let short_price = price * (1.0 + 1.0 / l - mmr);

            for (p_liq, side, part) in [
                (long_price, 1u8, long_share),
                (short_price, 0u8, 1.0 - long_share),
            ] {
                if part <= 0.0 || !(p_liq > 0.0) {
                    continue;
                }
                let e = live.entry(row_key(p_liq, grid, side)).or_insert((0.0, point.ts_ms));
                e.0 += notional * share * part;
            }
        }
    }

    // Bars after the last open-interest point still eat.
    while cursor < bars.len() {
        eat_bar(&mut live, &mut out, &bars[cursor], grid, params.min_usd);
        cursor += 1;
    }

    // Whatever survived every bar is STANDING liquidity — the half of the
    // layer that is a statement about where price is going.
    for ((row_idx, side), (notional, from_ts)) in live {
        if notional < params.min_usd {
            continue;
        }
        let price = row_idx as f64 * grid;
        if !(price > 0.0) {
            continue;
        }
        out.push(ProjectedLevel { price, from_ts, consumed_ts: None, notional, side });
    }

    out.sort_by(|a, b| b.notional.partial_cmp(&a.notional).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Close of the bar containing `ts_ms` — the price the exposure was opened
/// around. `None` when the timestamp predates the series.
fn price_at(bars: &[Bar], ts_ms: i64) -> Option<f64> {
    let idx = bar_index_at(bars, ts_ms)?;
    Some(bars[idx].close)
}

/// Index of the last bar at or before `ts_ms`.
fn bar_index_at(bars: &[Bar], ts_ms: i64) -> Option<usize> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `Bar::timestamp` is SECONDS (`ts_ms()` scales it) — the fixtures
    /// pass seconds and assert milliseconds, which is the trap that made
    /// the first run of these tests report 60_000_000.
    fn bar(ts_secs: i64, o: f64, h: f64, l: f64, c: f64) -> Bar {
        Bar::new(ts_secs, o, h, l, c)
    }

    fn params_single(leverage: f64) -> ProjectionParams {
        ProjectionParams {
            tiers: vec![LeverageTier { leverage, weight: 1.0 }],
            mmr: 0.0,
            row_pct: 0.01,
            long_share: 1.0,
            min_usd: 0.0,
        }
    }

    /// A 10× long opened at 100 dies at 90 — the level lands where the
    /// formula says, not somewhere near it.
    #[test]
    fn long_level_sits_at_the_liquidation_price() {
        let bars = vec![bar(0, 100.0, 100.0, 100.0, 100.0), bar(60, 100.0, 101.0, 99.0, 100.0)];
        let oi = vec![
            OiDeltaBar { ts_ms: 0, oi: 100.0, delta: 0.0 },
            OiDeltaBar { ts_ms: 0, oi: 200.0, delta: 100.0 },
        ];
        let levels = project(&bars, &oi, &params_single(10.0));
        assert_eq!(levels.len(), 1, "one tier, one side, one row");
        assert!((levels[0].price - 90.0).abs() < 0.5, "got {}", levels[0].price);
        assert_eq!(levels[0].side, 1);
        assert!(!levels[0].consumed(), "price never reached 90");
    }

    /// A level price traded through is marked consumed at that bar — the
    /// band has to END there, which is the whole reason the reference's
    /// picture is readable.
    #[test]
    fn level_is_consumed_when_price_trades_through() {
        let bars = vec![
            bar(0, 100.0, 100.0, 100.0, 100.0),
            bar(60, 100.0, 100.0, 89.0, 89.5),
        ];
        let oi = vec![
            OiDeltaBar { ts_ms: 0, oi: 100.0, delta: 0.0 },
            OiDeltaBar { ts_ms: 0, oi: 200.0, delta: 100.0 },
        ];
        let levels = project(&bars, &oi, &params_single(10.0));
        assert_eq!(levels.len(), 1);
        assert_eq!(levels[0].consumed_ts, Some(60_000));
    }

    /// Exposure rebuilt at a price price has ALREADY traded through is a
    /// second, separate band — not an invisible addition to a row that was
    /// marked consumed once and stayed that way.
    ///
    /// This is the defect that put every band at the left edge of the window
    /// (owner report 2026-08-02): the old fold stamped a row with its
    /// earliest contribution and asked about consumption once.
    #[test]
    fn a_level_rebuilt_after_being_eaten_is_a_second_segment() {
        // Price opens at 100, dives through 90 (eating the first level),
        // returns to 100 and the second wave of exposure opens there.
        let bars = vec![
            bar(0, 100.0, 100.0, 100.0, 100.0),
            bar(60, 100.0, 100.0, 89.0, 89.5),
            bar(120, 90.0, 100.0, 90.0, 100.0),
            bar(180, 100.0, 100.5, 99.5, 100.0),
        ];
        let oi = vec![
            OiDeltaBar { ts_ms: 0, oi: 100.0, delta: 0.0 },
            OiDeltaBar { ts_ms: 0, oi: 200.0, delta: 100.0 },
            OiDeltaBar { ts_ms: 180_000, oi: 300.0, delta: 100.0 },
        ];
        let levels = project(&bars, &oi, &params_single(10.0));
        assert_eq!(levels.len(), 2, "one eaten segment + one standing, got {levels:?}");
        let eaten = levels.iter().find(|l| l.consumed()).expect("first wave was eaten");
        let standing = levels.iter().find(|l| !l.consumed()).expect("second wave still stands");
        assert_eq!(eaten.consumed_ts, Some(60_000));
        assert_eq!(eaten.from_ts, 0);
        // The second segment starts when ITS exposure arrived, not at the
        // window's left edge.
        assert_eq!(standing.from_ts, 180_000);
        assert!((standing.price - 90.0).abs() < 0.5, "got {}", standing.price);
    }

    /// Open interest LEAVING creates nothing: a closed position takes its
    /// own level with it and we cannot know which.
    #[test]
    fn falling_open_interest_creates_no_levels() {
        let bars = vec![bar(0, 100.0, 100.0, 100.0, 100.0)];
        let oi = vec![
            OiDeltaBar { ts_ms: 0, oi: 200.0, delta: 0.0 },
            OiDeltaBar { ts_ms: 0, oi: 100.0, delta: -100.0 },
        ];
        assert!(project(&bars, &oi, &params_single(10.0)).is_empty());
    }
}
