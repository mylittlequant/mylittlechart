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
//! ## Why it opens nothing
//!
//! `liq_series_for_symbol` ENUMERATES what the workspace already streams —
//! the same rings the LIQ tape panel reads. The overlay's own venue is armed
//! by `ensure_bubble_indicator_feeds`; every other venue in the fold is one
//! some panel opened. Aggregation cannot add a channel, which is exactly the
//! property that makes it safe to leave on by default.

use mlc_core::chart::liquidations::LiquidationPrint;
use mlc_core::ids::BubbleId;

impl crate::ChartApp {
    /// Drain every venue ring for every bubble carrying a live liquidation
    /// overlay. Cheap when idle: a ring with nothing past the cursor costs
    /// one read lock and a comparison.
    pub(crate) fn pump_liquidations(&mut self) {
        // (bubble, symbol, own venue, own market, aggregate)
        let targets: Vec<(BubbleId, String, digdigdig3::ExchangeId, digdigdig3::AccountType, bool)> = {
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
                    ))
                })
                .collect()
        };

        for (bid, symbol, own_venue, own_at, aggregate) in targets {
            for (venue, at, handle) in self.core.liq_series_for_symbol(&symbol) {
                if !aggregate && (venue != own_venue || at != own_at) {
                    continue;
                }
                let label = format!("{}:{:?}", venue.as_str(), at);
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
                    // Venues that report no notional still report size and
                    // price, and a print with no size is the one thing the
                    // level fold cannot weigh — derive rather than drop.
                    let value = if p.value.is_finite() && p.value > 0.0 {
                        p.value
                    } else {
                        p.price * p.quantity
                    };
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
}
