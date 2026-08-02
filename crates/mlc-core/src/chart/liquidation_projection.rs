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
//!    one is history.
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
#[inline]
fn row_key(price: f64, row: f64, side: u8) -> (i64, u8) {
    ((price / row).round() as i64, side)
}

/// Build the projected levels for `bars` from the open-interest series.
///
/// `oi` must be time-ordered; bars likewise. Cost is
/// `O(oi × tiers + rows × bars_after_creation)` — the consumption scan is
/// the expensive half, and it stops at the first bar that eats the row.
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

    // (row, side) → (notional, earliest ts)
    let mut rows: std::collections::HashMap<(i64, u8), (f64, i64)> = std::collections::HashMap::new();

    for point in oi.iter() {
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
        let row = (price * row_pct).max(f64::MIN_POSITIVE);

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
                let e = rows.entry(row_key(p_liq, row, side)).or_insert((0.0, point.ts_ms));
                e.0 += notional * share * part;
                if point.ts_ms < e.1 {
                    e.1 = point.ts_ms;
                }
            }
        }
    }

    let mut out: Vec<ProjectedLevel> = rows
        .into_iter()
        .filter_map(|((row_idx, side), (notional, from_ts))| {
            if notional < params.min_usd {
                return None;
            }
            // Recover the row centre from the index: the grid step is a
            // fraction of price, so derive it from a representative price
            // rather than storing it per row.
            let price = price_at(bars, from_ts)? * row_pct * row_idx as f64;
            if !(price > 0.0) {
                return None;
            }
            Some(ProjectedLevel {
                price,
                from_ts,
                consumed_ts: consumed_at(bars, price, from_ts),
                notional,
                side,
            })
        })
        .collect();
    out.sort_by(|a, b| b.notional.partial_cmp(&a.notional).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Close of the bar containing `ts_ms` — the price the exposure was opened
/// around. `None` when the timestamp predates the series.
fn price_at(bars: &[Bar], ts_ms: i64) -> Option<f64> {
    let idx = bar_index_at(bars, ts_ms)?;
    Some(bars[idx].close)
}

/// The timestamp of the first bar AFTER `from_ts` whose range contains
/// `price` — the moment the level was eaten. `None` while it stands.
fn consumed_at(bars: &[Bar], price: f64, from_ts: i64) -> Option<i64> {
    let start = bar_index_at(bars, from_ts).map(|i| i + 1).unwrap_or(0);
    bars.get(start..)?
        .iter()
        .find(|b| b.low <= price && price <= b.high)
        .map(|b| b.ts_ms())
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
