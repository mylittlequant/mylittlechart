//! Realized liquidations — the forced closes that actually happened, as a
//! tape of prints plus the price levels they cluster on.
//!
//! ## Why this is not a heatmap (rewritten 2026-08-02)
//!
//! The previous revision aggregated liquidations into a 250 ms × price grid
//! and painted it like the order-book heat. That is wrong twice over. A
//! liquidation is an INSTANT — it has no duration to fill a cell with — and
//! the grid was keyed to 250 ms while the chart's columns are bars, so on a
//! 1-minute chart every print landed 240× off its own time. What the
//! reference draws, and what the data supports, is prints at their real
//! `(ts, price)` plus horizontal marks where enough of them stacked on one
//! price.
//!
//! ## Two different things called "liquidation heatmap"
//!
//! 1. **This** — realized liquidations from the exchange feed. Fact.
//! 2. **Projected liquidation levels** (CoinGlass / Hyblock) — a MODEL over
//!    open-interest change and an assumed leverage distribution. No exchange
//!    publishes other people's positions; those bands are estimates, and they
//!    belong in their own layer with their own parameters. Not this module.
//!
//! ## Side encoding
//!
//! `side == 1` — a LONG was liquidated (the engine sold). `side == 0` — a
//! SHORT was liquidated (the engine bought). This is MLC's shipped
//! convention across bridge → core → render; the dig3-core struct doc says
//! the opposite and is self-contradictory, so do NOT re-derive it there
//! (`docs/mlc/research/liq-feed-path-2026-08-01.md` §4).

use std::collections::VecDeque;

/// Maximum prints retained. Liquidations are sparse — a few thousand covers
/// hours on a major perp and survives a cascade without evicting the cascade
/// itself, which is the one thing worth keeping.
pub const MAX_PRINTS: usize = 8_000;

/// Default price grouping for levels, as a fraction of price. Levels exist
/// to show that several forced closes stacked on ONE price; too fine and a
/// cascade reads as unrelated dots, too coarse and everything is one band.
pub const DEFAULT_ROW_FRACTION: f64 = 0.0002;

/// One realized liquidation, positioned at its real time and price.
#[derive(Clone, Copy, Debug)]
pub struct LiquidationPrint {
    /// Exchange timestamp (ms).
    pub ts_ms: i64,
    /// Price the position was force-closed at.
    pub price: f64,
    /// Notional in quote currency. `0.0` when the venue reports none —
    /// `quantity * price` is filled in by the writer where it can.
    pub value_usd: f64,
    /// `1` = long liquidated, `0` = short liquidated — see the module doc.
    pub side: u8,
    /// Index into the caller's venue roster. Prints keep their origin so an
    /// aggregated tape can still say WHERE a cascade ran.
    pub venue: u8,
}

/// Rolling tape of realized liquidations for one instrument.
#[derive(Clone, Debug, Default)]
pub struct LiquidationTape {
    /// Prints, oldest at front. Appended in arrival order, which is not
    /// strictly timestamp order across venues — the level fold does not care
    /// and the dot renderer places by timestamp anyway.
    pub prints: VecDeque<LiquidationPrint>,
    /// Venue labels in roster order, indexed by `LiquidationPrint::venue`.
    pub venues: Vec<String>,
}

impl LiquidationTape {
    /// Append one print, evicting the oldest past [`MAX_PRINTS`].
    pub fn push(&mut self, print: LiquidationPrint) {
        if self.prints.len() >= MAX_PRINTS {
            self.prints.pop_front();
        }
        self.prints.push_back(print);
    }

    /// Roster slot for a venue label, registering it if new.
    pub fn venue_slot(&mut self, label: &str) -> u8 {
        if let Some(i) = self.venues.iter().position(|v| v == label) {
            return i as u8;
        }
        self.venues.push(label.to_string());
        (self.venues.len() - 1).min(255) as u8
    }

    /// Total notional on the tape — the one number that says whether folding
    /// venues in changed anything.
    pub fn total_usd(&self) -> f64 {
        self.prints.iter().map(|p| p.value_usd).sum()
    }

    /// Fold prints within `[from_ts, to_ts]` into price levels.
    ///
    /// A level is one price row that collected at least `min_usd`, and it
    /// spans the time between its first and last print — NOT to the right
    /// edge. A realized liquidation is history: the mark says "this much was
    /// force-closed here, over this stretch", and extending it forward would
    /// be claiming something about the future, which is the other layer's job.
    pub fn levels(
        &self,
        row_fraction: f64,
        min_usd: f64,
        from_ts: i64,
        to_ts: i64,
    ) -> Vec<LiquidationLevel> {
        use std::collections::HashMap;
        let frac = if row_fraction > 0.0 { row_fraction } else { DEFAULT_ROW_FRACTION };
        let mut by_row: HashMap<i64, LiquidationLevel> = HashMap::new();
        for p in self.prints.iter() {
            if p.ts_ms < from_ts || p.ts_ms > to_ts || p.price <= 0.0 {
                continue;
            }
            // Row width is a fraction of price, so the grouping means the
            // same thing at 60k as at 0.6 — a fixed tick count does not.
            let row_size = p.price * frac;
            if row_size <= 0.0 {
                continue;
            }
            let row = (p.price / row_size).round() as i64;
            let e = by_row.entry(row).or_insert(LiquidationLevel {
                price: row as f64 * row_size,
                first_ts: p.ts_ms,
                last_ts: p.ts_ms,
                long_usd: 0.0,
                short_usd: 0.0,
                count: 0,
            });
            e.first_ts = e.first_ts.min(p.ts_ms);
            e.last_ts = e.last_ts.max(p.ts_ms);
            if p.side == 1 {
                e.long_usd += p.value_usd;
            } else {
                e.short_usd += p.value_usd;
            }
            e.count += 1;
        }
        let mut out: Vec<LiquidationLevel> = by_row
            .into_values()
            .filter(|l| l.long_usd + l.short_usd >= min_usd)
            .collect();
        out.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap_or(std::cmp::Ordering::Equal));
        out
    }
}

/// One price row that collected liquidations, with the window they ran in.
#[derive(Clone, Copy, Debug)]
pub struct LiquidationLevel {
    /// Row price (lower edge of the row).
    pub price: f64,
    /// First and last print in this row, within the queried window.
    pub first_ts: i64,
    pub last_ts: i64,
    /// Notional force-closed on each side.
    pub long_usd: f64,
    pub short_usd: f64,
    /// Number of prints folded — a single whale and fifty retail stops look
    /// the same in notional alone.
    pub count: u32,
}

impl LiquidationLevel {
    /// Combined notional.
    pub fn total_usd(&self) -> f64 {
        self.long_usd + self.short_usd
    }

    /// Whether longs dominated this level.
    pub fn long_dominant(&self) -> bool {
        self.long_usd >= self.short_usd
    }
}
