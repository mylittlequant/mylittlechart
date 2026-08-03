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
}
