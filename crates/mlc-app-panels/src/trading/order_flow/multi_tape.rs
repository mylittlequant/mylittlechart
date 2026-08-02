//! Multi-venue tape roster (M/TT · M/L2 · M/BT — по образцу Multi-DOM,
//! owner order 2026-07-31, TapeSurf ref 17: the trades feed is
//! multi-market with an exchange column).
//!
//! One shared roster struct embedded in each tape panel's state the same
//! way `DomState.multi` embeds `MultiDomState`: the single-venue state is
//! untouched and survives toggling back; the roster survives too. Roster
//! entries reuse the Multi-DOM `"exchange[:AT]"` grammar and column
//! identity helpers (`multi_dom::col_*`). Handles are provisioned by the
//! app layer's feed-handle self-heal (`repair_panel_feed_handles`) — the
//! panel never dials anything itself.

use super::liq_tape::SharedLiquidationSeries;
use super::multi_dom::{col_at, col_entry, col_exchange, col_matches};
use super::trade_tape::{SharedOrderBookTracker, SharedTradeSeries};

/// Multi-venue roster + per-venue feed state for the tape panels.
///
/// Only the handle family the host panel reads is populated: Trade Tape /
/// Big Trades fill `trade_handles`/`trade_cursors`; L2 Tape fills
/// `book_handles`/`prev_books`. All vectors stay parallel to `entries`.
#[derive(Clone, Default)]
pub struct MultiTapeState {
    /// Base symbol shared by every roster venue.
    pub symbol: String,
    /// Roster entries `"exchange[:AT]"` — column identity per
    /// `multi_dom::col_matches`.
    pub entries: Vec<String>,
    /// Per-venue trade-series handles (TT / BT), parallel to `entries`.
    pub trade_handles: Vec<Option<SharedTradeSeries>>,
    /// Per-venue trade cursors — newest `ts_ms` already pulled. The LIQ
    /// tape reuses these cursors over `liq_handles`.
    pub trade_cursors: Vec<i64>,
    /// Per-venue liquidation-series handles (LIQ tape), parallel to
    /// `entries` — a distinct station ring type (`LiquidationPoint`).
    pub liq_handles: Vec<Option<SharedLiquidationSeries>>,
    /// Per-venue orderbook handles (L2), parallel to `entries`.
    pub book_handles: Vec<Option<SharedOrderBookTracker>>,
    /// Per-venue base units per contract, parallel to `entries`. `1.0` for
    /// venues that quote sizes in coin; the app layer fills the rest from
    /// the venue's own exchange-info record. **`0.0` means NOT YET KNOWN** —
    /// the venue quotes contracts and its symbol catalogue has not loaded —
    /// and a consumer must skip that venue rather than assume 1.
    ///
    /// Without it a Gate liquidation of 369 contracts (0.0369 BTC) reads as
    /// 369 BTC and prices at $23M — the SIZE and NOTIONAL columns are then
    /// both wrong by four orders of magnitude, and a size-scaled bar makes
    /// every other print in the ring invisible.
    pub venue_contract_size: Vec<f64>,
    /// Per-venue previous bucketed book `tick → (bid_qty, ask_qty)` —
    /// the L2 diff/classify state, parallel to `entries`.
    pub prev_books: Vec<std::collections::HashMap<i64, (f64, f64)>>,
    /// Per-venue `last_update_id` from the most recent tracker read (L2) —
    /// skips the diff when a venue's book has not advanced.
    pub book_uids: Vec<Option<u64>>,
    /// Per-venue last snapshot-diff wall-clock (ms) — bookkeeping for the
    /// L2 snapshot-diff RESYNC fallback (the primary feed is the raw
    /// delta drain since 2026-08-01, owner defect «лента стала будто
    /// медленнее» — the 250ms diff throttle was batching the stream).
    pub book_last_diff_ms: Vec<i64>,
    /// Per-venue last raw-delta arrival (ms). While a venue's delta
    /// stream is live the snapshot-diff fallback skips it — diffing on
    /// top of per-event deltas would double-emit.
    pub delta_fed_ms: Vec<i64>,
    /// Last dead-venue reseed attempt (ms) — throttles the app layer's
    /// re-subscribe of venues whose handle landed but never delivered
    /// (the Multi-DOM empty-column disease: subscribe raced the venue
    /// connector dial).
    pub last_reseed_ms: i64,
    /// Current reseed throttle (ms), escalating 5s→60s while dead venues
    /// persist, reset once every venue delivers — an unfixable venue
    /// must not turn the repair loop into a dial storm.
    pub reseed_backoff_ms: i64,
}

impl std::fmt::Debug for MultiTapeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiTapeState")
            .field("symbol", &self.symbol)
            .field("entries", &self.entries)
            .field(
                "live_trade_handles",
                &self.trade_handles.iter().filter(|h| h.is_some()).count(),
            )
            .field(
                "live_book_handles",
                &self.book_handles.iter().filter(|h| h.is_some()).count(),
            )
            .finish()
    }
}

impl MultiTapeState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a venue if not already present (entry = `"exchange[:AT]"`,
    /// normalized). Appends blank parallel slots.
    pub fn add_entry(&mut self, entry: &str) {
        let ex = col_exchange(entry).to_string();
        let at = col_at(entry).to_string();
        if self.entries.iter().any(|e| col_matches(e, &ex, &at)) {
            return;
        }
        self.entries.push(col_entry(&ex, &at));
        self.trade_handles.push(None);
        self.trade_cursors.push(0);
        self.liq_handles.push(None);
        self.book_handles.push(None);
        self.venue_contract_size.push(1.0);
        self.prev_books.push(std::collections::HashMap::new());
        self.book_uids.push(None);
        self.book_last_diff_ms.push(0);
        self.delta_fed_ms.push(0);
    }

    /// Remove a venue and its parallel slots by entry.
    pub fn remove_entry(&mut self, entry: &str) {
        let ex = col_exchange(entry);
        let at = col_at(entry);
        if let Some(i) = self.entries.iter().position(|e| col_matches(e, ex, at)) {
            self.entries.remove(i);
            if i < self.trade_handles.len() { self.trade_handles.remove(i); }
            if i < self.trade_cursors.len() { self.trade_cursors.remove(i); }
            if i < self.liq_handles.len() { self.liq_handles.remove(i); }
            if i < self.book_handles.len() { self.book_handles.remove(i); }
            if i < self.venue_contract_size.len() { self.venue_contract_size.remove(i); }
            if i < self.prev_books.len() { self.prev_books.remove(i); }
            if i < self.book_uids.len() { self.book_uids.remove(i); }
            if i < self.book_last_diff_ms.len() { self.book_last_diff_ms.remove(i); }
            if i < self.delta_fed_ms.len() { self.delta_fed_ms.remove(i); }
        }
    }

    /// Switch the base symbol: stale handles/cursors/books are dropped
    /// (the app layer's self-heal re-provisions for the new key), roster
    /// kept.
    pub fn reset_for_symbol(&mut self, symbol: &str) {
        self.symbol = symbol.to_string();
        for h in &mut self.trade_handles { *h = None; }
        for c in &mut self.trade_cursors { *c = 0; }
        for h in &mut self.liq_handles { *h = None; }
        for h in &mut self.book_handles { *h = None; }
        // Contract size is a property of (venue, SYMBOL) — a symbol switch
        // invalidates it, so it resets with the handles rather than
        // carrying the old instrument's multiplier into the new one.
        for m in &mut self.venue_contract_size { *m = 1.0; }
        for b in &mut self.prev_books { b.clear(); }
        for u in &mut self.book_uids { *u = None; }
        for d in &mut self.book_last_diff_ms { *d = 0; }
        for d in &mut self.delta_fed_ms { *d = 0; }
        self.last_reseed_ms = 0;
        self.reseed_backoff_ms = 0;
    }

    /// Keep every parallel vector sized to the roster (defensive resize —
    /// hydration may restore entries alone).
    pub fn sync_len(&mut self) {
        let n = self.entries.len();
        if self.trade_handles.len() != n { self.trade_handles.resize_with(n, || None); }
        if self.trade_cursors.len() != n { self.trade_cursors.resize(n, 0); }
        if self.liq_handles.len() != n { self.liq_handles.resize_with(n, || None); }
        if self.book_handles.len() != n { self.book_handles.resize_with(n, || None); }
        if self.venue_contract_size.len() != n { self.venue_contract_size.resize(n, 1.0); }
        if self.prev_books.len() != n {
            self.prev_books.resize_with(n, std::collections::HashMap::new);
        }
        if self.book_uids.len() != n { self.book_uids.resize(n, None); }
        if self.book_last_diff_ms.len() != n { self.book_last_diff_ms.resize(n, 0); }
        if self.delta_fed_ms.len() != n { self.delta_fed_ms.resize(n, 0); }
    }

    /// Short venue tag for the row column — `"BI:F"` (same format the
    /// Multi-DOM narrow header uses).
    pub fn venue_tag(&self, i: usize) -> String {
        self.entries
            .get(i)
            .map(|e| {
                let exch: String = col_exchange(e).chars().take(2).collect();
                format!("{}:{}", exch.to_uppercase(), col_at(e).to_uppercase())
            })
            .unwrap_or_default()
    }
}
