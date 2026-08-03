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
}
