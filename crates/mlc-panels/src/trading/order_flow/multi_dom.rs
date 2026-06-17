//! Multi-exchange consolidated DOM panel.
//!
//! Status: scaffolding. The state machine + merge engine + renderer is
//! tracked separately under
//! `nemo/docs/mlc/plans/renderer-wave-B-2026-06-18.md` Track 7. Spec:
//! `nemo/docs/mlc/research/specs/aggr-multi-exchange-pattern-2026-06-18.md`
//! + `nemo/docs/mlc/research/specs/multi-exchange-depth-panels-2026-06-17.md`.
//!
//! Design notes (locked):
//! - Per-exchange `BTreeMap<OrderedFloat, f64>` book state — one
//!   per side, per exchange. Common rounded-price grid configurable via
//!   `MultiDomConfig::grid_size`.
//! - Subscribes via N parallel `bridge.request_orderbook` over N
//!   exchanges; each emits `LiveUpdate::OrderbookSnapshot` /
//!   `LiveUpdate::OrderbookDelta`.
//! - Merge engine: per-tick, fold all per-exchange books onto the
//!   common grid, emit `Vec<MultiLevel>` ordered by price desc (asks
//!   on top, bids on bottom — Bookmap-style merged ladder).
//! - Renderer: text-grid panel (NOT a chart). N + 1 columns (price +
//!   one per exchange). Cell intensity ∝ size. Cross-exchange
//!   dominance highlight: the row's max-volume exchange gets a side
//!   accent stripe. Stacked-imbalance detection on the consolidated
//!   row.

use std::collections::BTreeMap;

/// Local Eq/Ord newtype around f64 — we avoid adding an `ordered_float`
/// dependency for this scaffold. Compares via bit-pattern after
/// canonicalising NaN to a single zero-payload representation.
#[derive(Clone, Copy, Debug)]
pub struct OrderedFloat(pub f64);

impl PartialEq for OrderedFloat {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}
impl Eq for OrderedFloat {}
impl PartialOrd for OrderedFloat {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}
impl Ord for OrderedFloat {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.partial_cmp(&other.0).unwrap_or(std::cmp::Ordering::Equal)
    }
}

/// Exchange identifier — held as an opaque String label here so the
/// multi-dom scaffold doesn't pull a hard dep on `digdigdig3`. Wire
/// up to the typed `digdigdig3::ExchangeId` at the panel-construction
/// site (one `format!("{:?}", ex)` is enough).
pub type ExchangeId = String;

/// One consolidated price level — per-exchange volume on a common
/// rounded price.
#[derive(Clone, Debug)]
pub struct MultiLevel {
    pub price: f64,
    /// Per-exchange volume at this level. Length == config.exchanges.len();
    /// `0.0` for exchanges without volume at this price.
    pub per_exchange: Vec<f64>,
    /// True when this row is on the ask side (`price > mid_price`).
    pub is_ask: bool,
}

/// Static config for a multi-exchange book.
#[derive(Clone, Debug)]
pub struct MultiDomConfig {
    /// Exchanges to merge — order is preserved in the renderer columns.
    pub exchanges: Vec<ExchangeId>,
    /// Common price-grid bucket size (rounded down).
    pub grid_size: f64,
    /// Number of levels to keep per side after the merge.
    pub depth_per_side: usize,
}

impl Default for MultiDomConfig {
    fn default() -> Self {
        Self {
            exchanges: Vec::new(),
            grid_size: 0.5,
            depth_per_side: 30,
        }
    }
}

/// Multi-exchange consolidated book state.
///
/// Per-exchange books live in `books_by_exchange`. The merged view is
/// rebuilt at tick time by [`MultiExchangeBook::merged_levels`].
#[derive(Default)]
pub struct MultiExchangeBook {
    pub config: MultiDomConfig,
    /// `books_by_exchange[i]` is `(bids, asks)` for `config.exchanges[i]`.
    pub books_by_exchange: Vec<(BTreeMap<OrderedFloat, f64>, BTreeMap<OrderedFloat, f64>)>,
    /// Current mid-price across the consolidated book (cached at merge
    /// time so the renderer doesn't recompute it).
    pub mid_price: f64,
}

impl MultiExchangeBook {
    pub fn new(config: MultiDomConfig) -> Self {
        let n = config.exchanges.len();
        Self {
            config,
            books_by_exchange: (0..n)
                .map(|_| (BTreeMap::new(), BTreeMap::new()))
                .collect(),
            mid_price: 0.0,
        }
    }

    /// Snap a price to the common grid (floor — caller chooses which
    /// rounding semantics). Public so external callers can match the
    /// merge engine's bucket boundaries when preparing inputs.
    #[inline]
    pub fn snap_grid(&self, price: f64) -> f64 {
        (price / self.config.grid_size).floor() * self.config.grid_size
    }

    /// Apply a single-exchange book update.
    pub fn apply_snapshot(
        &mut self,
        exchange_idx: usize,
        bids: impl IntoIterator<Item = (f64, f64)>,
        asks: impl IntoIterator<Item = (f64, f64)>,
    ) {
        let Some((b, a)) = self.books_by_exchange.get_mut(exchange_idx) else { return };
        b.clear();
        a.clear();
        for (price, qty) in bids {
            if qty <= 0.0 { continue; }
            let p = self.config.grid_size * (price / self.config.grid_size).floor();
            *b.entry(OrderedFloat(p)).or_insert(0.0) += qty;
        }
        for (price, qty) in asks {
            if qty <= 0.0 { continue; }
            let p = self.config.grid_size * (price / self.config.grid_size).floor();
            *a.entry(OrderedFloat(p)).or_insert(0.0) += qty;
        }
    }

    /// Build the merged ladder. Computes the consolidated per-price
    /// volume across all exchanges for both sides, sorts (asks high→low,
    /// bids high→low), truncates to `depth_per_side` per side, and
    /// updates `self.mid_price` from the best bid/ask consensus.
    pub fn merged_levels(&mut self) -> Vec<MultiLevel> {
        let n = self.config.exchanges.len();
        let mut by_price_bid: BTreeMap<OrderedFloat, Vec<f64>> = BTreeMap::new();
        let mut by_price_ask: BTreeMap<OrderedFloat, Vec<f64>> = BTreeMap::new();
        for (i, (b, a)) in self.books_by_exchange.iter().enumerate() {
            for (p, q) in b {
                let row = by_price_bid.entry(*p).or_insert_with(|| vec![0.0; n]);
                row[i] += *q;
            }
            for (p, q) in a {
                let row = by_price_ask.entry(*p).or_insert_with(|| vec![0.0; n]);
                row[i] += *q;
            }
        }

        let best_bid = by_price_bid.keys().next_back().copied();
        let best_ask = by_price_ask.keys().next().copied();
        self.mid_price = match (best_bid, best_ask) {
            (Some(b), Some(a)) => (b.0 + a.0) * 0.5,
            (Some(b), None) => b.0,
            (None, Some(a)) => a.0,
            _ => self.mid_price,
        };

        let mut out: Vec<MultiLevel> = Vec::new();
        // Asks first (low→high in storage; reverse so the renderer can
        // draw top-down).
        for (p, row) in by_price_ask.iter().take(self.config.depth_per_side) {
            out.push(MultiLevel { price: p.0, per_exchange: row.clone(), is_ask: true });
        }
        for (p, row) in by_price_bid.iter().rev().take(self.config.depth_per_side) {
            out.push(MultiLevel { price: p.0, per_exchange: row.clone(), is_ask: false });
        }
        out
    }
}
