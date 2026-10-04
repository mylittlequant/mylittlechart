//! Realized-liquidation writer: the chart's tape is PULLED from the shared
//! station rings, one cursor per venue.
//!
//! ## Why a pull and not a channel
//!
//! The rings are written by the station's own WS task and they keep their
//! history. A frame we never ran therefore costs nothing — the next frame
//! drains everything past the cursor. The previous design pushed each print
//! through the broadcast and applied it inside the frame loop, which is how
//! the DOM heat lost whole minutes whenever the tab sat in the background
//! (measured 2026-08-02: eight hidden minutes, eight missing columns).
//!
//! ## What it subscribes
//!
//! An aggregated liquidation overlay ARMS the whole liquidation roster —
//! `multi_dom::LIQ_EXCHANGES` × Futures — for its symbol, and then reads
//! back through `liq_series_for_symbol`.
//!
//! This is a deliberate reversal of the 2026-08-02 first cut, which only
//! enumerated rings other panels had opened (the DOM heat's contract, where
//! folding books nobody asked for would be real bandwidth). It was wrong
//! here for a simple reason: with no LIQ panel on screen the overlay saw
//! ONE venue, and a single venue's forced closes on a 1-minute chart are a
//! dot every few minutes — the layer looked broken because it had almost
//! nothing to draw. Liquidations are also cheap in a way books are not:
//! one event per forced close, nothing to reconstruct, no depth to
//! maintain, and the ring is shared with the LIQ panel when both are open.
//!
//! Venues with no public channel are never armed — see [`LIQ_EXCHANGES`]
//! for the roster and the reasoning behind each exclusion.
//!
//! [`LIQ_EXCHANGES`]: mlc_app_panels::trading::order_flow::multi_dom::LIQ_EXCHANGES

use mlc_core::chart::liquidations::LiquidationPrint;
use mlc_core::ids::BubbleId;

impl crate::ChartApp {
    /// Drain every venue ring for every bubble carrying a live liquidation
    /// overlay. Cheap when idle: a ring with nothing past the cursor costs
    /// one read lock and a comparison.
    pub(crate) fn pump_liquidations(&mut self) {
        // (bubble, symbol, own venue, own market, aggregate, venue set)
        #[allow(clippy::type_complexity)]
        let targets: Vec<(
            BubbleId,
            String,
            digdigdig3::ExchangeId,
            digdigdig3::AccountType,
            bool,
            Vec<digdigdig3::ExchangeId>,
        )> = {
            let mut seen: std::collections::HashSet<BubbleId> = std::collections::HashSet::new();
            self.panel_app
                .panel_grid
                .containers()
                .keys()
                .copied()
                .collect::<Vec<_>>()
                .into_iter()
                .filter(|cid| self.liq_overlay_active_for_chart(*cid))
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
                    Some((
                        bid,
                        symbol,
                        self.exchange_id_for_chart(cid),
                        mlc_backend_data_bridge::account_type_to_dig3(at),
                        self.liq_aggregate(bid),
                        // The instance's venue set, or every venue that
                        // publishes forced closes when it has not been
                        // narrowed.
                        self.overlay_venues(bid, Self::LIQ_HEATMAP_TYPE_ID),
                    ))
                })
                .collect()
        };

        for (bid, symbol, own_venue, own_at, aggregate, venues) in targets {
            self.ensure_liquidation_roster(bid, &symbol, own_venue, own_at, aggregate, &venues);
            let _ = own_at;
            for (venue, at, handle) in self.core.liq_series_for_symbol(&symbol) {
                // Venue only, never the market type: liquidations exist on
                // Futures alone, so "own venue" already names the one ring
                // a non-aggregated overlay wants — matching the chart's own
                // account type would drop it on every Spot chart.
                if !aggregate && venue != own_venue {
                    continue;
                }
                // Aggregating reads exactly the venues this instance armed —
                // a ring some OTHER panel opened is not this layer's data,
                // and folding it made the picture depend on what else the
                // workspace happened to have open.
                if aggregate && !venues.contains(&venue) {
                    continue;
                }
                let label = format!("{}:{:?}", venue.as_str(), at);
                // Contracts → base units. Gate/OKX/KuCoin/HTX quote a
                // derivative size in contracts, so a raw `price × size` is
                // out by the contract multiplier — 10 000× on Gate's BTC
                // perp. One such print sets the scale for the whole layer,
                // which is how a working overlay reads as an empty one.
                let futures =
                    !matches!(at, digdigdig3::AccountType::Spot | digdigdig3::AccountType::Margin);
                let (contract_size, quotes_contracts) = {
                    let syms = self.core.exchange_symbols();
                    (
                        syms.contract_size_for(venue.as_str(), &symbol, futures),
                        futures && syms.quotes_contracts(venue.as_str()),
                    )
                };
                // Catalogue not in yet. Leave the cursor where it is and
                // come back: a print ingested at the wrong scale would
                // outlive the load and set the normaliser's maximum for
                // hours, which is worse than showing it a second late.
                if quotes_contracts && contract_size == 1.0 {
                    continue;
                }
                let cursor = self
                    .liq_cursors
                    .get(&(bid, label.clone()))
                    .copied()
                    .unwrap_or(0);
                let snapshot = {
                    let guard = handle.blocking_read();
                    guard.snapshot()
                };
                let mut new_cursor = cursor;
                let mut fresh: Vec<LiquidationPrint> = Vec::new();
                for p in snapshot.iter().filter(|p| p.ts_ms > cursor) {
                    if p.ts_ms > new_cursor {
                        new_cursor = p.ts_ms;
                    }
                    // Always derive from price × base size, never from the
                    // wire's own `value`: the venues that report one derive
                    // it the same way from the SAME contract-denominated
                    // size (OKX sends `price × sz` with `sz` in contracts),
                    // so trusting it just imports the error.
                    let value = p.price * p.quantity * contract_size;
                    fresh.push(LiquidationPrint {
                        ts_ms: p.ts_ms,
                        price: p.price,
                        value_usd: if value.is_finite() { value } else { 0.0 },
                        // MLC's shipped convention (bridge → core → render):
                        // `side == 0` a SHORT was liquidated, otherwise a
                        // LONG. The dig3-core doc states the opposite and is
                        // self-contradictory — do not re-derive it there.
                        side: if p.side == 0 { 0 } else { 1 },
                        venue: 0,
                    });
                }
                if fresh.is_empty() {
                    continue;
                }
                self.liq_cursors.insert((bid, label.clone()), new_cursor);
                self.core.bubbles_mut().with_bubble_mut(bid, |bubble| {
                    let tape = bubble
                        .liq_tape_mut()
                        .get_or_insert_with(mlc_core::chart::liquidations::LiquidationTape::default);
                    let slot = tape.venue_slot(&label);
                    for mut print in fresh {
                        print.venue = slot;
                        tape.push(print);
                    }
                });
            }
        }
    }

    /// Arm the liquidation feeds this bubble's overlay wants — the whole
    /// publisher roster on Futures when aggregating, its own venue alone
    /// otherwise. Runs on a change of (symbol, aggregate) and, while a
    /// venue is missing, on a throttled repair tick — never every frame.
    ///
    /// `subscribe_liquidations` is idempotent per (venue, market, symbol)
    /// and the ring is shared with the LIQ tape panel, so an overlay and a
    /// panel on the same symbol cost one subscription between them — which
    /// is why arming here does not fight the panel's own self-heal.
    ///
    /// The repair tick matters because a venue can fail its subscribe once
    /// and stay absent for the whole session otherwise: KuCoin's WS needs a
    /// REST-issued token and loses it to CORS in the browser, and any venue
    /// can lose a dial to a transient. Bounded by `LIQ_REPAIR_MS` so a
    /// permanently-dead venue costs one retry every 30s, not a dial storm.
    fn ensure_liquidation_roster(
        &mut self,
        bid: BubbleId,
        symbol: &str,
        own_venue: digdigdig3::ExchangeId,
        own_at: digdigdig3::AccountType,
        aggregate: bool,
        venues: &[digdigdig3::ExchangeId],
    ) {
        use mlc_app_panels::trading::order_flow::multi_dom;

        /// Minimum gap between repair passes for a roster missing a venue.
        const LIQ_REPAIR_MS: i64 = 30_000;

        let now_ms = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        // The armed key carries the venue SET, not just the aggregate flag:
        // narrowing the set is a change of what must be live, and keying on
        // the flag alone left the old subscriptions armed and the picker
        // looking inert.
        let want_key: Vec<String> = venues.iter().map(|e| e.as_str().to_string()).collect();
        if let Some((armed_symbol, armed_agg, armed_venues, last_ms)) = self.liq_armed.get(&bid) {
            if armed_symbol == symbol && *armed_agg == aggregate && *armed_venues == want_key {
                let want = if aggregate { venues.len() } else { 1 };
                let have = self.core.liq_series_for_symbol(symbol).len();
                if have >= want || now_ms - *last_ms < LIQ_REPAIR_MS {
                    return;
                }
                crate::sglog!(
                    "[bubble {:?}] liquidation roster repair ({} of {} venues live)",
                    bid, have, want
                );
            }
        }

        if aggregate {
            for eid in venues {
                let eid = *eid;
                let slug = eid.as_str();
                // A connector the owner switched off in the sidebar stays
                // off here too — the overlay is not a way around that.
                if !self
                    .sidebar_state
                    .connector_enabled
                    .get(slug)
                    .copied()
                    .unwrap_or(true)
                {
                    continue;
                }
                self.core.ensure_connector(eid);
                // The venue's symbol catalogue is what carries its contract
                // multiplier, and it is loaded lazily — a venue we only
                // fold liquidations from is never otherwise requested, so
                // its records stay empty and every size reads as coin.
                // Idempotent: the station caches the exchange-info fetch.
                self.core.bridge_handle().request_symbols(eid);
                let _ = self
                    .core
                    .subscribe_liquidations(eid, symbol, digdigdig3::AccountType::FuturesCross);
            }
        } else if multi_dom::venue_has_liquidations(own_venue.as_str()) {
            // Futures regardless of the chart's own market: a Spot chart
            // still means "this venue's forced closes", and a Spot
            // liquidation subscription is rejected by every venue that has
            // the feed at all.
            let _ = own_at;
            self.core.ensure_connector(own_venue);
            let _ = self.core.subscribe_liquidations(
                own_venue,
                symbol,
                digdigdig3::AccountType::FuturesCross,
            );
        }

        crate::sglog!(
            "[bubble {:?}] liquidation roster armed ({} aggregate={} venues={})",
            bid, symbol, aggregate, want_key.join(",")
        );
        self.liq_armed
            .insert(bid, (symbol.to_string(), aggregate, want_key, now_ms));
    }

    /// Feed every `Liquidation`-declaring compute indicator on a bubble
    /// every print newer than this feed's own cursor, oldest first (Arc D
    /// plan §3.3, `docs/mlc/plans/indicator-streams-arc-d-2026-09-25.md`).
    ///
    /// Deliberately independent of [`Self::liq_overlay_active_for_chart`]'s
    /// gate: unlike [`Self::pump_liquidations`], which only drains for a
    /// bubble with a VISIBLE liquidation overlay, a compute indicator can
    /// want this stream with no overlay open. Every individual print is fed
    /// (never just the latest) because `LiquidationConsumer` implementations
    /// (cascade/cooldown/rate detectors) need each one.
    ///
    /// Read-only: this drains whatever rings [`BackendCore::liq_series_for_symbol`]
    /// already resolves (armed by the LIQ tape/heatmap panel or an
    /// aggregating roster) — it never subscribes on its own, so an
    /// indicator with no accompanying panel stays at `is_ready() == false`
    /// until something arms that venue, the same graceful cold-start state
    /// the OrderBook feed (`frame_lifecycle/dom_books.rs`) already has.
    pub(crate) fn feed_liquidation_indicators(&mut self) {
        use mlc_backend_indicator_engine::StreamKind;

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
                    let wants_liq = self.core.indicators().instances_for_bubble(bid).iter().any(|inst| {
                        self.core
                            .indicators()
                            .definition_streams(&inst.type_id)
                            .contains(&StreamKind::Liquidation)
                    });
                    if !wants_liq {
                        return None;
                    }
                    let symbol = self.symbol_for_chart(cid)?;
                    if symbol.is_empty() {
                        return None;
                    }
                    Some((bid, symbol))
                })
                .collect()
        };

        for (bid, symbol) in targets {
            for (venue, at, handle) in self.core.liq_series_for_symbol(&symbol) {
                // Same contract-size normalisation `pump_liquidations` uses
                // (see its own comment): Gate/OKX/KuCoin/HTX quote a
                // derivative size in contracts, and feeding a raw contract
                // count into a cascade/rate detector calibrated on notional
                // size would be off by the contract multiplier.
                let futures =
                    !matches!(at, digdigdig3::AccountType::Spot | digdigdig3::AccountType::Margin);
                let (contract_size, quotes_contracts) = {
                    let syms = self.core.exchange_symbols();
                    (
                        syms.contract_size_for(venue.as_str(), &symbol, futures),
                        futures && syms.quotes_contracts(venue.as_str()),
                    )
                };
                if quotes_contracts && contract_size == 1.0 {
                    continue;
                }
                let label = format!("{}:{:?}", venue.as_str(), at);
                let cursor = self
                    .indicator_liq_cursors
                    .get(&(bid, label.clone()))
                    .copied()
                    .unwrap_or(0);
                let snapshot = {
                    let guard = handle.blocking_read();
                    guard.snapshot()
                };
                let mut new_cursor = cursor;
                for p in snapshot.iter().filter(|p| p.ts_ms > cursor) {
                    if p.ts_ms > new_cursor {
                        new_cursor = p.ts_ms;
                    }
                    let liq = liquidation_event(&symbol, p.ts_ms, p.price, p.quantity, p.side, contract_size);
                    self.core.indicators_mut().feed_stream(
                        bid,
                        &mlc_backend_indicator_engine::StreamEvent::Liquidation(liq),
                    );
                }
                if new_cursor != cursor {
                    self.indicator_liq_cursors.insert((bid, label), new_cursor);
                }
            }
        }
    }
}

/// Build one `mli`-side `Liquidation` event from a raw station print.
///
/// Kept standalone with primitive parameters (no `digdigdig3_station`
/// type in the signature — that crate is a dev-dependency here, not a
/// normal one) so the side-byte decode has a direct unit test: it is the
/// one genuinely easy-to-get-backwards part of this adapter.
///
/// `digdigdig3_station`'s own encode (`side_byte` in `digdigdig3-station/
/// src/data/liquidation_ext.rs`, mirrored by `LiquidationPoint::
/// from_stream_event`) writes `TradeSide::Buy -> 0`, `TradeSide::Sell ->
/// 1` — consistent across every liquidation point flavour in that crate,
/// and matching dig3-core's own doc-fixed (2026-08-01) semantics ("Sell =
/// long was liquidated"). `side_byte` is decoded the same way it was
/// encoded.
fn liquidation_event(
    symbol: &str,
    ts_ms: i64,
    price: f64,
    quantity: f64,
    side_byte: u8,
    contract_size: f64,
) -> mlc_backend_indicator_engine::types::Liquidation {
    let side = if side_byte == 0 {
        mlc_backend_indicator_engine::types::TradeSide::Buy
    } else {
        mlc_backend_indicator_engine::types::TradeSide::Sell
    };
    mlc_backend_indicator_engine::types::Liquidation {
        symbol: symbol.to_string(),
        side,
        price,
        quantity: quantity * contract_size,
        timestamp: ts_ms,
        value: Some(price * quantity * contract_size),
        ..Default::default()
    }
}

#[cfg(test)]
mod feed_liquidation_indicators_tests {
    use super::*;

    #[test]
    fn liquidation_event_decodes_the_station_side_byte_convention() {
        let buy = liquidation_event("BTCUSDT", 1_000, 100.0, 2.0, 0, 1.0);
        assert_eq!(
            buy.side,
            mlc_backend_indicator_engine::types::TradeSide::Buy,
            "side byte 0 = short liquidated = TradeSide::Buy"
        );
        let sell = liquidation_event("BTCUSDT", 1_000, 100.0, 2.0, 1, 1.0);
        assert_eq!(
            sell.side,
            mlc_backend_indicator_engine::types::TradeSide::Sell,
            "side byte 1 = long liquidated = TradeSide::Sell"
        );
    }

    /// The drain wraps this adapter in `StreamEvent::Liquidation` and
    /// nothing else. A fake ring cannot be planted from this crate:
    /// `Station::series_handles` is `pub(crate)` and is filled only by a
    /// live `Station::subscribe`, and `liq_series_for_symbol` only returns
    /// rings that subscribe already registered.
    #[test]
    fn liquidation_event_builds_the_liquidation_stream_variant() {
        let liq = liquidation_event("BTCUSDT", 5_000, 100.0, 2.0, 1, 1.0);
        let ev = mlc_backend_indicator_engine::StreamEvent::Liquidation(liq);
        assert_eq!(ev.kind(), mlc_backend_indicator_engine::StreamKind::Liquidation);
        match ev {
            mlc_backend_indicator_engine::StreamEvent::Liquidation(got) => {
                assert_eq!(got.symbol, "BTCUSDT");
                assert_eq!(got.side, mlc_backend_indicator_engine::types::TradeSide::Sell);
                assert_eq!(got.price, 100.0);
                assert_eq!(got.quantity, 2.0);
                assert_eq!(got.timestamp, 5_000);
                assert_eq!(got.value, Some(200.0));
            }
            other => panic!("liquidation_event must build StreamEvent::Liquidation, got {other:?}"),
        }
    }

    #[test]
    fn liquidation_event_applies_contract_size_to_quantity_and_value() {
        let liq = liquidation_event("BTCUSDT", 1_000, 100.0, 2.0, 0, 10_000.0);
        assert_eq!(liq.quantity, 20_000.0, "contract count -> base units");
        assert_eq!(liq.value, Some(2_000_000.0), "price * base-unit quantity");
        assert_eq!(liq.timestamp, 1_000);
        assert_eq!(liq.symbol, "BTCUSDT");
    }

    /// `digdigdig3_station::Station`'s `series_handles` registry (the only
    /// place a `SharedLiquidationSeries` handle comes from) is populated
    /// exclusively by a successful `Station::subscribe` — synchronously
    /// inside `spawn_forwarder`, which the fail-closed 0.3.7 subscribe path
    /// never reaches without a connected WS connector — and offers no
    /// public test-seeding hook from outside its own crate (unlike the
    /// `OrderBookTracker` DashMap `mlc-backend-data-bridge` owns directly,
    /// see `dom_books.rs`'s own tests). Compounding that: NO `Liquidation`-
    /// declaring catalog entry is even creatable via `IndicatorManager::
    /// create_instance` today — a pre-existing, unrelated `mylittleindicators`
    /// bug found live while wiring this step (`UnifiedIndicatorCatalog::
    /// get_all()` never enumerates the `Liquidations` category; see
    /// `mlc-backend-indicator-engine`'s own `liquidation_funding_open_
    /// interest_entries_are_unreachable_pending_mli_get_all_fix` test,
    /// which pins this exact gap). So the only hermetically-testable slice
    /// of this feed's ORCHESTRATION half is "a bubble with NO
    /// Liquidation-declaring instance -> no panic, no fabricated value" —
    /// the conversion math is covered by
    /// [`liquidation_event_decodes_the_station_side_byte_convention`] and
    /// [`liquidation_event_applies_contract_size_to_quantity_and_value`];
    /// the "fires on a real print" half needs BOTH mli's gap closed AND a
    /// live d2b pass (the plan's own step 8) to prove end-to-end.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn feed_liquidation_indicators_is_a_noop_with_no_declaring_instance() {
        use digdigdig3::core::types::{AccountType, ExchangeId};
        use mlc_backend_data_bridge::DataBridge;
        use mlc_core::state::Timeframe;

        let (bridge, _rx, _connector_ready_rx) = DataBridge::new().await;
        let bridge = std::sync::Arc::new(bridge);
        let core = mlc_backend::BackendCore::new(bridge, None);
        let mut app = crate::ChartApp::new_empty(core);
        app.seed_default_chart(ExchangeId::Binance, "BTCUSDT", Timeframe::m1(), AccountType::Spot, 5);
        let cid = app.panel_app.panel_grid.active_container_id().expect("active container");
        let bid = app.bubble_for_chart(cid).expect("seeded bubble");

        // `book_imb` declares `OrderBook`, not `Liquidation` — a control
        // showing the target scan finds nothing to feed on this bubble.
        let id = app
            .core
            .indicators_mut()
            .add(Some(bid), "book_imb")
            .expect("book_imb catalog entry exists");
        let bars: Vec<mlc_core::Bar> = (0..5)
            .map(|i| mlc_core::Bar {
                timestamp: i as i64,
                open: 100.0,
                high: 101.0,
                low: 99.0,
                close: 100.0,
                volume: 10.0,
                ..Default::default()
            })
            .collect();
        app.core.indicators_mut().recalc_bubble(bid, &bars);

        let values_of = |app: &crate::ChartApp| -> std::collections::HashMap<String, Vec<f64>> {
            app.core
                .indicators()
                .instances_for_bubble(bid)
                .into_iter()
                .find(|inst| inst.id == id)
                .map(|inst| (*inst.values).clone())
                .expect("instance exists")
        };

        let before = values_of(&app);
        // No Liquidation-declaring instance on this bubble — the target
        // scan in `feed_liquidation_indicators` finds nothing, so the drain
        // loop body never runs (never even calls `liq_series_for_symbol`).
        app.feed_liquidation_indicators();
        let after = values_of(&app);

        assert_eq!(
            before, after,
            "no Liquidation-declaring instance — book_imb must stay untouched, not panic"
        );
    }

    /// `seed_default_chart` installs the bar-only `volume` histogram. That
    /// is not a Liquidation consumer. The cursor map is written only after
    /// the drain has decided to feed a point, so it must stay empty.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn feed_liquidation_indicators_does_not_fire_for_a_bubble_with_no_stream_consumer() {
        use digdigdig3::core::types::{AccountType, ExchangeId};
        use mlc_backend_data_bridge::DataBridge;
        use mlc_backend_indicator_engine::StreamKind;
        use mlc_core::state::Timeframe;

        let (bridge, _rx, _connector_ready_rx) = DataBridge::new().await;
        let bridge = std::sync::Arc::new(bridge);
        let core = mlc_backend::BackendCore::new(bridge, None);
        let mut app = crate::ChartApp::new_empty(core);
        app.seed_default_chart(ExchangeId::Binance, "BTCUSDT", Timeframe::m1(), AccountType::Spot, 5);
        let cid = app.panel_app.panel_grid.active_container_id().expect("active container");
        let bid = app.bubble_for_chart(cid).expect("seeded bubble");
        let type_ids: Vec<String> = app
            .core
            .indicators()
            .instances_for_bubble(bid)
            .into_iter()
            .map(|inst| inst.type_id.clone())
            .collect();
        assert!(!type_ids.is_empty(), "seed installs the default volume histogram");
        assert!(
            type_ids.iter().all(|type_id| {
                !app.core.indicators().definition_streams(type_id).contains(&StreamKind::Liquidation)
            }),
            "the seeded bubble must not already declare Liquidation"
        );
        let before: Vec<_> = app
            .core
            .indicators()
            .instances_for_bubble(bid)
            .into_iter()
            .map(|inst| (inst.id, (*inst.values).clone()))
            .collect();

        app.feed_liquidation_indicators();

        assert!(
            app.indicator_liq_cursors.is_empty(),
            "no Liquidation consumer — the drain must not record a cursor"
        );
        let after: Vec<_> = app
            .core
            .indicators()
            .instances_for_bubble(bid)
            .into_iter()
            .map(|inst| (inst.id, (*inst.values).clone()))
            .collect();
        assert_eq!(before, after, "the drain must not touch a bubble with no Liquidation consumer");
    }
}
