//! Per-frame-budgeted exchange catalogue ingestion (owner ruling
//! 2026-09-05, `docs/mlc/research/wasm-lag-measurements-2026-09-05.md`).
//!
//! `LiveUpdate::SymbolsLoaded` now carries a notification only
//! (`exchange_id` + `count`) — the `tick_writers` arm just enqueues the
//! exchange id here (see [`BackendCore::enqueue_catalogue_ingest`]).
//! Turning the notification into MLC's lean [`CatalogueSymbol`] records is
//! this module's job, walked a few symbols at a time out of the render
//! tick: [`BackendCore::ingest_catalogues`] is called once per frame by the
//! shell (right after `tick_writers`) and stops as soon as its time budget
//! is spent, resuming the SAME exchange's catalogue from where it left off
//! on the next call. A Deribit-sized (11k+ symbol) catalogue therefore
//! lands over a handful of frames instead of one multi-second render-tick
//! stall — "tens of seconds is fine" per the owner's ruling, since data
//! work is deliberately kept off the render path.

use std::collections::VecDeque;
use std::time::Duration;

use digdigdig3::ExchangeId;

use crate::exchange_symbols::CatalogueSymbol;
use crate::{BackendCore, BubbleEvent};

/// Default per-frame time budget for [`BackendCore::ingest_catalogues`] on
/// the interactive wasm shell — the render tick must never carry more than
/// this (owner ruling 2026-09-05).
pub const CATALOGUE_INGEST_BUDGET_WEB: Duration = Duration::from_millis(4);

/// Native shells share a thread with layout/paint too, but have far more
/// headroom than wasm's single JS thread. A larger per-frame slice still
/// keeps warmup well under a frame budget while finishing large catalogues
/// in fewer frames.
pub const CATALOGUE_INGEST_BUDGET_NATIVE: Duration = Duration::from_millis(16);

/// How often (in convert-eligible items) the walk re-checks the deadline.
/// Each check is a clock read — cheap on native, a JS FFI round-trip via
/// `web_time` on wasm — so it is throttled rather than done per symbol.
const CHECK_EVERY: usize = 64;

/// One exchange's catalogue conversion in progress.
struct CatalogueIngestCursor {
    exchange_id: ExchangeId,
    /// How many of the exchange's catalogue entries (in station walk order)
    /// have already landed in `partial` — the resume point for the next
    /// slice.
    position: usize,
    /// Lean records converted so far.
    partial: Vec<CatalogueSymbol>,
    /// Slices consumed so far — diagnostic only (`dlog!` line on
    /// completion).
    slices: u32,
    /// Wall time actually spent walking/converting this exchange's
    /// catalogue, summed across slices (excludes inter-frame idle time).
    elapsed: Duration,
}

impl CatalogueIngestCursor {
    fn new(exchange_id: ExchangeId) -> Self {
        Self {
            exchange_id,
            position: 0,
            partial: Vec::new(),
            slices: 0,
            elapsed: Duration::ZERO,
        }
    }
}

/// Queue of exchanges whose catalogue notification landed and are waiting
/// (or in progress) — see the module doc.
#[derive(Default)]
pub(crate) struct CatalogueIngestState {
    queue: VecDeque<ExchangeId>,
    current: Option<CatalogueIngestCursor>,
}

/// Per-item ingestion decision, shared by the real station-driven walk
/// (`BackendCore::ingest_catalogues`, push-based over `Station::
/// for_each_symbol`) and the pure-slice walk used in this module's own
/// tests (`Station` needs live connector I/O to construct, so the
/// resumable-cursor / throttled-budget-check behaviour is exercised on a
/// plain slice instead — see the `tests` module below).
///
/// `idx` is this item's 0-based position in walk order. Returns whether the
/// item at `idx` should be converted. `since_check` and `budget_hit` are
/// the caller's running state (bumped/reset in place) — once `budget_hit`
/// flips true, every later item in the same walk is skipped too (cheaply:
/// no further `should_stop` calls, no conversion work).
fn ingest_step(
    idx: usize,
    resume_from: usize,
    check_every: usize,
    since_check: &mut usize,
    budget_hit: &mut bool,
    mut should_stop: impl FnMut() -> bool,
) -> bool {
    if idx < resume_from || *budget_hit {
        return false;
    }
    *since_check += 1;
    if *since_check >= check_every.max(1) {
        *since_check = 0;
        if should_stop() {
            *budget_hit = true;
            return false;
        }
    }
    true
}

impl BackendCore {
    /// Enqueue `exchange_id` for lean-catalogue conversion. Called from the
    /// `LiveUpdate::SymbolsLoaded` arm in `tick_writers` — deduplicated: an
    /// exchange already queued or currently in progress is not requeued (the
    /// next `ingest_catalogues` slice re-walks the live station cache via
    /// `for_each_symbol`, so it naturally picks up anything the venue added
    /// between notifications).
    pub(crate) fn enqueue_catalogue_ingest(&self, exchange_id: ExchangeId) {
        let mut st = self
            .catalogue_ingest
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let already_in_flight = st
            .current
            .as_ref()
            .map(|c| c.exchange_id)
            == Some(exchange_id);
        if !already_in_flight && !st.queue.contains(&exchange_id) {
            st.queue.push_back(exchange_id);
        }
    }

    /// Walk the pending catalogue-ingest queue, converting `SymbolInfo`
    /// records into lean [`CatalogueSymbol`]s, bounded by `budget` (wall
    /// time spent inside this call). Called once per frame by the shell,
    /// right after `tick_writers`.
    ///
    /// When an exchange's catalogue finishes converting, this swaps the new
    /// `Vec<CatalogueSymbol>` into [`crate::exchange_symbols`] (`set_symbols`),
    /// runs [`mlc_backend_data_bridge::DataBridge::prewarm_wire_symbol_cache`]
    /// (already lean), and returns a [`BubbleEvent::SymbolsLoaded`] for the
    /// caller to fan into `react_to_events` — so downstream reactors see the
    /// exact same event shape they always did, just later.
    pub fn ingest_catalogues(&self, budget: Duration) -> Vec<BubbleEvent> {
        let call_start = web_time::Instant::now();
        let mut events = Vec::new();

        loop {
            if call_start.elapsed() >= budget {
                break;
            }

            {
                let mut st = self
                    .catalogue_ingest
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if st.current.is_none() {
                    match st.queue.pop_front() {
                        Some(exchange_id) => {
                            st.current = Some(CatalogueIngestCursor::new(exchange_id));
                        }
                        None => break, // nothing pending
                    }
                }
            }

            let mut cursor = {
                let mut st = self
                    .catalogue_ingest
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                st.current.take().expect("just ensured Some above")
            };

            let label = cursor.exchange_id.as_str();
            let resume_from = cursor.position;
            let slice_t0 = web_time::Instant::now();
            let mut idx = 0usize;
            let mut since_check = 0usize;
            let mut budget_hit = false;
            self.bridge.station().for_each_symbol(cursor.exchange_id, |s| {
                if ingest_step(
                    idx,
                    resume_from,
                    CHECK_EVERY,
                    &mut since_check,
                    &mut budget_hit,
                    || call_start.elapsed() >= budget,
                ) {
                    cursor.partial.push(CatalogueSymbol::from_symbol_info(label, s));
                    cursor.position = idx + 1;
                }
                idx += 1;
            });
            let total_seen = idx;
            cursor.slices += 1;
            cursor.elapsed += slice_t0.elapsed();

            if cursor.position < total_seen {
                // Budget spent (or an unexpected catalogue shrink) — resume
                // this exchange on the next call.
                let mut st = self
                    .catalogue_ingest
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                st.current = Some(cursor);
                break;
            }

            // Finished: swap the lean catalogue in, prewarm, emit, move on.
            let exchange_id = cursor.exchange_id;
            let symbols = cursor.partial;
            let n = symbols.len();
            self.exchange_symbols_mut().set_symbols(exchange_id, symbols);
            self.bridge.prewarm_wire_symbol_cache(exchange_id);
            mlc_backend_data_bridge::dlog!(
                "[catalogue] {:?}: {} symbols in {} slices, {:.1}ms total",
                exchange_id, n, cursor.slices, cursor.elapsed.as_secs_f64() * 1000.0,
            );
            events.push(BubbleEvent::SymbolsLoaded { exchange_id, count: n });

            let mut st = self
                .catalogue_ingest
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            st.current = None;
            // Loop continues — if budget remains, the next queued exchange
            // (if any) starts on the next iteration.
        }

        events
    }

    /// One-shot lean-catalogue seed for every currently-connected exchange.
    ///
    /// Used by shell startup (`mlc-app-shell-vello::App::new`), which runs
    /// OUTSIDE the render loop — a synchronous walk here is not subject to
    /// the per-frame budget `ingest_catalogues` enforces. Replaces the old
    /// `DataBridge::symbols_snapshot()` + `replace_all` path, which
    /// deep-cloned every `SymbolInfo` (including `extra`) for every
    /// connected exchange.
    pub fn seed_exchange_symbols_from_bridge(&self) {
        let station = self.bridge.station();
        let mut by_exchange = std::collections::HashMap::new();
        for exchange_id in self.bridge.pool().list_connected() {
            let label = exchange_id.as_str();
            let mut symbols = Vec::new();
            station.for_each_symbol(exchange_id, |s| {
                symbols.push(CatalogueSymbol::from_symbol_info(label, s));
            });
            if !symbols.is_empty() {
                by_exchange.insert(exchange_id, symbols);
            }
        }
        self.exchange_symbols_mut().replace_all(by_exchange);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digdigdig3::SymbolInfo;

    /// Plain-slice harness driving the SAME `ingest_step` decision function
    /// `BackendCore::ingest_catalogues` uses — a real `Station` needs live
    /// connector I/O to construct, so this proves the resumable-cursor /
    /// throttled-budget-check behaviour without one.
    fn ingest_slice(
        label: &str,
        catalogue: &[SymbolInfo],
        resume_from: usize,
        check_every: usize,
        mut should_stop: impl FnMut() -> bool,
    ) -> (Vec<CatalogueSymbol>, usize) {
        let mut out = Vec::new();
        let mut since_check = 0usize;
        let mut budget_hit = false;
        for (idx, s) in catalogue.iter().enumerate() {
            if ingest_step(idx, resume_from, check_every, &mut since_check, &mut budget_hit, &mut should_stop) {
                out.push(CatalogueSymbol::from_symbol_info(label, s));
            }
        }
        let new_cursor = resume_from + out.len();
        (out, new_cursor)
    }

    fn fake_catalogue(n: usize) -> Vec<SymbolInfo> {
        (0..n)
            .map(|i| SymbolInfo { symbol: format!("SYM{i}"), ..Default::default() })
            .collect()
    }

    #[test]
    fn ingest_slice_stops_at_budget_and_resumes() {
        let catalogue = fake_catalogue(10);
        // check_every=1 → every convert-eligible item runs `should_stop`.
        let mut calls = 0u32;
        let (first, cursor) = ingest_slice("binance", &catalogue, 0, 1, || {
            calls += 1;
            calls > 3
        });
        assert_eq!(first.len(), 3);
        assert_eq!(cursor, 3);
        assert_eq!(first.last().unwrap().symbol, "SYM2");

        // Second slice resumes from the cursor and finishes the rest.
        let (second, cursor) = ingest_slice("binance", &catalogue, cursor, 1, || false);
        assert_eq!(second.len(), 7);
        assert_eq!(cursor, 10);
        assert_eq!(second.first().unwrap().symbol, "SYM3");
        assert_eq!(second.last().unwrap().symbol, "SYM9");
    }

    #[test]
    fn ingest_slice_resume_from_skips_already_converted() {
        let catalogue = fake_catalogue(5);
        let (out, cursor) = ingest_slice("binance", &catalogue, 2, 1, || false);
        assert_eq!(cursor, 5);
        assert_eq!(
            out.iter().map(|s| s.symbol.clone()).collect::<Vec<_>>(),
            vec!["SYM2", "SYM3", "SYM4"],
        );
    }

    #[test]
    fn ingest_slice_check_every_throttles_stop_calls() {
        let catalogue = fake_catalogue(200);
        let mut calls = 0u32;
        // check_every=64 → `should_stop` runs on items 64 and 128 (1-based
        // counting within the eligible range), not on every item.
        let (out, cursor) = ingest_slice("binance", &catalogue, 0, 64, || {
            calls += 1;
            false
        });
        assert_eq!(out.len(), 200);
        assert_eq!(cursor, 200);
        assert_eq!(calls, 200 / 64);
    }
}
