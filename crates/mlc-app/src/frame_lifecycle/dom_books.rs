//! Order books the DOM Heatmap overlay keeps live, per bubble.
//!
//! Until 2026-08-03 the heat layer subscribed to exactly ONE book — the
//! chart's own — and its `aggregate` flag folded whatever OTHER books some
//! unrelated panel happened to have open. With no Multi-DOM on screen the
//! toggle changed nothing, which is why its default was the odd one out
//! (`false`, while both liquidation layers defaulted to `true`): a control
//! that usually does nothing is worse than no control.
//!
//! Now the overlay owns a ROSTER, exactly like the liquidation layer: the
//! venue set on its instance says which books it wants, and this pass makes
//! the live subscriptions match. That also means the picture no longer
//! depends on what else the workspace has open — the same overlay with the
//! same settings draws the same thing.
//!
//! One owner, on purpose: the arming used to sit in the stream reconcile in
//! `input/overlay.rs`, which fires on a change of the DECLARED STREAM set. A
//! venue-set edit does not change that set, so the reconcile never saw it.
//! Arming in both places would have left the difference armed forever, which
//! is the subscription leak this file exists to prevent.

use mlc_core::ids::BubbleId;
use rust_decimal::prelude::ToPrimitive;

impl crate::ChartApp {
    /// Reconcile the books held for every DOM Heatmap overlay against the
    /// books its venue set asks for.
    pub(crate) fn ensure_dom_book_rosters(&mut self) {
        // (bubble, symbol, market, wanted venues) for every chart, resolved
        // before any mutation — the arming borrows `core` mutably.
        #[allow(clippy::type_complexity)]
        let wanted: Vec<(BubbleId, String, digdigdig3::AccountType, Vec<digdigdig3::ExchangeId>)> = {
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
                    let symbol = self.symbol_for_chart(cid)?;
                    if symbol.is_empty() {
                        return None;
                    }
                    let at = self.account_type_for_chart(cid)?;
                    let own = self.exchange_id_for_chart(cid);
                    let venues = if !self.dom_overlay_active(bid) {
                        Vec::new()
                    } else if self.dom_heatmap_aggregate(bid) {
                        // The chart's own venue is always in the fold: it is
                        // the market the candles are drawn from, and a heat
                        // layer that dropped it would disagree with the
                        // series it sits under.
                        let mut v = self.overlay_venues(bid, Self::DOM_HEATMAP_TYPE_ID);
                        if !v.contains(&own) {
                            v.push(own);
                        }
                        v
                    } else {
                        vec![own]
                    };
                    Some((
                        bid,
                        symbol,
                        mlc_backend_data_bridge::account_type_to_dig3(at),
                        venues,
                    ))
                })
                .collect()
        };

        let live_bubbles: std::collections::HashSet<BubbleId> =
            wanted.iter().map(|(bid, ..)| *bid).collect();

        for (bid, symbol, at, venues) in wanted {
            let held = self.dom_books_armed.get(&bid).cloned();

            // A symbol change invalidates every held book: they are keyed by
            // instrument, and leaving them up would stream the OLD symbol's
            // depth into a layer now drawn over a different market.
            if let Some((held_symbol, held_at, held_venues)) = held.as_ref() {
                if held_symbol != &symbol || held_at != &at {
                    for eid in held_venues {
                        self.core.unsubscribe_dom_heatmap(*eid, held_symbol, *held_at);
                        self.core.unsubscribe_depth(*eid, held_symbol, *held_at);
                    }
                    self.dom_books_armed.remove(&bid);
                }
            }
            let held_venues: Vec<digdigdig3::ExchangeId> = self
                .dom_books_armed
                .get(&bid)
                .map(|(_, _, v)| v.clone())
                .unwrap_or_default();

            if held_venues == venues {
                continue;
            }

            for eid in held_venues.iter().filter(|e| !venues.contains(e)) {
                self.core.unsubscribe_dom_heatmap(*eid, &symbol, at);
                // The tracker stays: it is shared station state a DOM ladder
                // may also be reading, and the fold already ignores a venue
                // that left the set.
                crate::sglog!("[bubble {:?}] book feed down ({} {:?})", bid, symbol, eid);
            }
            for eid in venues.iter().filter(|e| !held_venues.contains(e)) {
                // A connector the owner switched off in the sidebar stays off
                // here too — the overlay is not a way around that.
                if !self
                    .sidebar_state
                    .connector_enabled
                    .get(eid.as_str())
                    .copied()
                    .unwrap_or(true)
                {
                    continue;
                }
                self.core.ensure_connector(*eid);
                // TWO subscriptions, because they feed two different things
                // and measuring proved one is not enough (2026-08-03,
                // `book_venues: 0` with five venues armed):
                //
                // * `subscribe_dom_heatmap` delivers this venue's book as
                //   passthrough EVENTS — the layer's clock and its trade
                //   dots. It creates no tracker.
                // * `subscribe_depth` creates the shared `OrderBookTracker`,
                //   and the tracker registry is what the fold reads. Without
                //   it an armed venue contributes nothing: the aggregate
                //   toggle looked live, the roster looked armed, and the
                //   picture stayed single-venue.
                //
                // This is the same pair a DOM ladder opens, which is the
                // cost of asking for another venue's depth.
                self.core.subscribe_dom_heatmap(*eid, &symbol, at);
                self.core.subscribe_depth(*eid, &symbol, at);
                crate::sglog!("[bubble {:?}] book feed up ({} {:?})", bid, symbol, eid);
            }

            if venues.is_empty() {
                self.dom_books_armed.remove(&bid);
            } else {
                self.dom_books_armed.insert(bid, (symbol, at, venues));
            }
        }

        // A bubble that stopped being rendered by any chart releases its
        // books — otherwise closing the window that hosted the overlay
        // leaves its subscriptions up for the rest of the session.
        let orphans: Vec<BubbleId> = self
            .dom_books_armed
            .keys()
            .copied()
            .filter(|bid| !live_bubbles.contains(bid))
            .collect();
        for bid in orphans {
            if let Some((symbol, at, venues)) = self.dom_books_armed.remove(&bid) {
                for eid in venues {
                    self.core.unsubscribe_dom_heatmap(eid, &symbol, at);
                }
                crate::sglog!("[bubble {:?}] book feeds released (bubble gone)", bid);
            }
        }
    }

    /// Feed every `OrderBook`-declaring compute indicator on a bubble the
    /// currently reconstructed book, once per frame (Arc D plan §3.3,
    /// `docs/mlc/plans/indicator-streams-arc-d-2026-09-25.md`).
    /// `OrderBookConsumer::update_orderbook` wants "the current book", not
    /// an event stream, so a per-frame snapshot read loses no information
    /// an L2/DOM panel wouldn't also be sampling for its own redraw.
    ///
    /// Read-only: this never subscribes on its own — the tracker is armed
    /// by [`Self::ensure_dom_book_rosters`] (or any other consumer sharing
    /// the same key, e.g. a DOM ladder). A bubble whose only interest in
    /// the book is a compute indicator, with no DOM Heatmap overlay open,
    /// finds no tracker and is skipped — the indicator stays at
    /// `is_ready() == false`, the same graceful cold-start state §3.5
    /// already covers, not a new failure mode.
    pub(crate) fn feed_orderbook_indicators(&mut self) {
        use mlc_backend_indicator_engine::StreamKind;

        let targets: Vec<(BubbleId, digdigdig3::ExchangeId, String, digdigdig3::AccountType)> = {
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
                    let wants_book = self.core.indicators().instances_for_bubble(bid).iter().any(|inst| {
                        self.core
                            .indicators()
                            .definition_streams(&inst.type_id)
                            .contains(&StreamKind::OrderBook)
                    });
                    if !wants_book {
                        return None;
                    }
                    let symbol = self.symbol_for_chart(cid)?;
                    if symbol.is_empty() {
                        return None;
                    }
                    let at = self.account_type_for_chart(cid)?;
                    let eid = self.exchange_id_for_chart(cid);
                    Some((bid, eid, symbol, mlc_backend_data_bridge::account_type_to_dig3(at)))
                })
                .collect()
        };

        for (bid, eid, symbol, at) in targets {
            let Some(tracker) = self.core.bridge_handle().ob_tracker_for(eid, &symbol, at) else {
                continue;
            };
            let guard = tracker.blocking_read();
            if !guard.has_snapshot() {
                continue;
            }
            // mli's own documented depth (mylittleindicators/CLAUDE.md:107).
            let bids: Vec<mlc_backend_indicator_engine::types::OrderBookLevel> = guard
                .top_bids(50)
                .into_iter()
                .map(|(p, q)| {
                    mlc_backend_indicator_engine::types::OrderBookLevel::new(
                        p.to_f64().unwrap_or(0.0),
                        q.to_f64().unwrap_or(0.0),
                    )
                })
                .collect();
            let asks: Vec<mlc_backend_indicator_engine::types::OrderBookLevel> = guard
                .top_asks(50)
                .into_iter()
                .map(|(p, q)| {
                    mlc_backend_indicator_engine::types::OrderBookLevel::new(
                        p.to_f64().unwrap_or(0.0),
                        q.to_f64().unwrap_or(0.0),
                    )
                })
                .collect();
            let timestamp = guard.last_timestamp_ms();
            let last_update_id = guard.last_update_id();
            drop(guard);

            let book = mlc_backend_indicator_engine::types::OrderBook {
                bids,
                asks,
                timestamp,
                last_update_id,
                ..Default::default()
            };
            self.core
                .indicators_mut()
                .feed_stream(bid, &mlc_backend_indicator_engine::StreamEvent::OrderBook(book));
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod feed_orderbook_indicators_tests {
    //! `OrderBookTracker::blocking_read`/`blocking_write` panic when called
    //! from a task already running ON a tokio runtime (`tokio::future::
    //! block_on`'s own "Cannot block the current thread from within a
    //! runtime" guard) — so every test here builds its OWN dedicated
    //! runtime (mirrors `ChartApp::new`'s native constructor) and runs as a
    //! PLAIN `#[test]`, never `#[tokio::test]`: the test body must execute
    //! OUTSIDE of any task the runtime is driving.

    use digdigdig3::core::types::{AccountType, ExchangeId};
    use mlc_backend_data_bridge::DataBridge;
    use mlc_core::state::Timeframe;

    fn build_app() -> (crate::ChartApp, std::sync::Arc<tokio::runtime::Runtime>) {
        let rt = std::sync::Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime for test"),
        );
        let (bridge, _rx, _connector_ready_rx) = rt.block_on(DataBridge::new());
        let bridge = std::sync::Arc::new(bridge);
        let core = mlc_backend::BackendCore::new_with_runtime(bridge, None, rt.clone());
        (crate::ChartApp::new_empty(core), rt)
    }

    /// Flat bars, enough for `recalc_bubble` to build a `ComputeCache` —
    /// built LOCALLY rather than read back from `app.core.bubbles().bars(bid)`:
    /// `seed_default_chart` only KICKS OFF an async REST fetch (its own doc
    /// comment: "bars are requested asynchronously"), so with no live
    /// network in this test the bubble's own stored `bars` stays empty
    /// forever. `calculate_for_bubble`/`recalc_bubble` take bars as a
    /// parameter, not a read of the bubble's stored series, so a local
    /// array is all a compute-cache build needs (mirrors `flat_bars` in
    /// `mlc-backend/src/indicator/manager.rs`'s own tests).
    fn flat_bars(n: usize) -> Vec<mlc_core::Bar> {
        (0..n)
            .map(|i| mlc_core::Bar {
                timestamp: i as i64,
                open: 100.0,
                high: 101.0,
                low: 99.0,
                close: 100.0,
                volume: 10.0,
                ..Default::default()
            })
            .collect()
    }

    fn instance_values(
        app: &crate::ChartApp,
        bid: mlc_core::ids::BubbleId,
        id: u64,
    ) -> std::collections::HashMap<String, Vec<f64>> {
        app.core
            .indicators()
            .instances_for_bubble(bid)
            .into_iter()
            .find(|inst| inst.id == id)
            .map(|inst| (*inst.values).clone())
            .expect("instance exists")
    }

    #[test]
    fn feed_orderbook_indicators_moves_a_declaring_instance_and_skips_others() {
        let (mut app, _rt) = build_app();
        app.seed_default_chart(ExchangeId::Binance, "BTCUSDT", Timeframe::m1(), AccountType::Spot, 5);
        let cid = app.panel_app.panel_grid.active_container_id().expect("active container");
        let bid = app.bubble_for_chart(cid).expect("seeded bubble");

        // `book_imb` declares `OrderBook` as its only stream (mlc-backend's
        // own step 2 test: "book_imb declares OrderBook as its only stream
        // and is a no-op passthrough on update_bar" — the bar-only baseline
        // below never moves off its construction-time value on its own).
        let book_imb_id = app
            .core
            .indicators_mut()
            .add(Some(bid), "book_imb")
            .expect("book_imb catalog entry exists");
        // Declares no non-bar stream at all — must stay untouched.
        let sma_id = app
            .core
            .indicators_mut()
            .add(Some(bid), "sma")
            .expect("sma catalog entry exists");

        app.core.indicators_mut().recalc_bubble(bid, &flat_bars(5));

        let book_imb_before = instance_values(&app, bid, book_imb_id);
        let sma_before = instance_values(&app, bid, sma_id);

        // Arm the tracker + inject a fake snapshot directly — no network
        // traffic: `subscribe_depth`'s background REST/station tasks are
        // no-ops without a connected connector, but the DashMap tracker
        // ENTRY they create is synchronous.
        app.core
            .bridge_handle()
            .subscribe_depth(ExchangeId::Binance, "BTCUSDT", AccountType::Spot);
        let tracker = app
            .core
            .bridge_handle()
            .ob_tracker_for(ExchangeId::Binance, "BTCUSDT", AccountType::Spot)
            .expect("subscribe_depth creates the tracker entry synchronously");
        {
            let ob = digdigdig3::core::types::OrderBook {
                bids: vec![digdigdig3::core::types::OrderBookLevel::new(100.0, 30.0)],
                asks: vec![digdigdig3::core::types::OrderBookLevel::new(101.0, 10.0)],
                timestamp: 1,
                ..Default::default()
            };
            tracker.blocking_write().apply_snapshot(&ob).expect("apply_snapshot");
        }

        app.feed_orderbook_indicators();

        let book_imb_after = instance_values(&app, bid, book_imb_id);
        let sma_after = instance_values(&app, bid, sma_id);

        assert_ne!(
            book_imb_before, book_imb_after,
            "book_imb should move once the tracker has a real snapshot"
        );
        assert_eq!(
            sma_before, sma_after,
            "sma declares no OrderBook stream — the per-frame poll must not touch it"
        );
    }

    #[test]
    fn feed_orderbook_indicators_is_a_noop_with_no_armed_tracker() {
        let (mut app, _rt) = build_app();
        app.seed_default_chart(ExchangeId::Binance, "BTCUSDT", Timeframe::m1(), AccountType::Spot, 5);
        let cid = app.panel_app.panel_grid.active_container_id().expect("active container");
        let bid = app.bubble_for_chart(cid).expect("seeded bubble");

        let book_imb_id = app
            .core
            .indicators_mut()
            .add(Some(bid), "book_imb")
            .expect("book_imb catalog entry exists");
        app.core.indicators_mut().recalc_bubble(bid, &flat_bars(5));

        let before = instance_values(&app, bid, book_imb_id);
        // No `subscribe_depth` call — `ob_tracker_for` resolves to `None`.
        app.feed_orderbook_indicators();
        let after = instance_values(&app, bid, book_imb_id);

        assert_eq!(before, after, "no tracker armed — the indicator must stay untouched, not panic or fabricate a value");
    }
}
