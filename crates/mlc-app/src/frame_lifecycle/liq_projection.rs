//! Projected-liquidation levels: open-interest input and cache maintenance.
//!
//! ## Where the open interest comes from
//!
//! REST history, per venue, not a websocket stream. Binance, KuCoin and HTX
//! have no public WS open-interest channel at all — the station reports the
//! subscribe as wire-absent and the forwarder exits — so a layer fed only by
//! the live stream had NO input on the venue most charts sit on and drew
//! nothing (measured 2026-08-02: `oi_points: 0` on Binance BTCUSDT while the
//! overlay was on). `get_open_interest_history` is the source every venue
//! that matters actually serves, and it arrives as history, so the layer is
//! useful the moment it is switched on instead of after an hour of watching.
//!
//! ## Solo and aggregated
//!
//! Same contract as the liquidation layers: `aggregate` on (default) folds
//! every venue that serves OI history for the symbol; off reads the chart's
//! own venue. Aggregating is the default because price is one market — a
//! projection from one venue understates every level by whatever share the
//! others hold.
//!
//! ## Why the projection is cached
//!
//! The model is cheap per point but its consumption scan is
//! `O(rows × bars_after_creation)`. It runs when the inputs change — bars,
//! OI rows, or a parameter that changes the OUTPUT — never per frame. Ink
//! (opacity, sensitivity, consumed visibility) is read at draw time and
//! deliberately does not invalidate the cache.

use mlc_core::ids::BubbleId;

/// Minimum gap between open-interest history refreshes for one bubble.
const OI_REFRESH_MS: i64 = 120_000;

impl crate::ChartApp {
    /// Pull open-interest history for every bubble carrying a live
    /// projection overlay, then recompute the levels when anything moved.
    pub(crate) fn refresh_liq_projection(&mut self) {
        // (bubble, symbol, own venue, aggregate, bar_ms)
        #[allow(clippy::type_complexity)]
        let targets: Vec<(
            BubbleId,
            String,
            digdigdig3::ExchangeId,
            bool,
            i64,
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
                .filter_map(|cid| {
                    let bid = self.bubble_for_chart(cid)?;
                    if !seen.insert(bid) {
                        return None;
                    }
                    // Open-interest history feeds TWO consumers: the
                    // projection models from it, and the OI Delta sub-pane
                    // resamples it into its line. Gating the fetch on the
                    // projection alone left the sub-pane permanently empty
                    // whenever the projection was hidden — measured
                    // 2026-08-03.
                    let wants_oi = self.liq_projection_active(bid)
                        || self.overlay_instance_active(bid, Self::OI_DELTA_TYPE_ID);
                    if !wants_oi {
                        return None;
                    }
                    let symbol = self.symbol_for_chart(cid)?;
                    if symbol.is_empty() {
                        return None;
                    }
                    let bar_ms = self
                        .timeframe_for_chart(cid)
                        .filter(|tf| !tf.is_tick())
                        .map(|tf| tf.interval_ms)
                        .unwrap_or(60_000);
                    // The venue set of whichever consumer is live. The
                    // sub-pane has no aggregate control of its own, so a
                    // chart showing only the sub-pane reads the projection's
                    // set if one exists and every OI venue otherwise.
                    let venues = {
                        let v = self.overlay_venues(bid, Self::LIQ_PROJECTION_TYPE_ID);
                        if v.is_empty() {
                            self.venues_for_stream(
                                mlc_backend_indicator_engine::StreamKind::OpenInterest,
                            )
                        } else {
                            v
                        }
                    };
                    Some((
                        bid,
                        symbol,
                        self.exchange_id_for_chart(cid),
                        self.liq_projection_aggregate(bid),
                        bar_ms,
                        venues,
                    ))
                })
                .collect()
        };

        let now_ms = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        for (bid, symbol, own_venue, aggregate, bar_ms, venues) in targets {
            self.ensure_oi_history(bid, &symbol, own_venue, aggregate, bar_ms, now_ms, &venues);
            if self.liq_projection_active(bid) {
                self.rebuild_liq_projection(bid, own_venue, aggregate, bar_ms);
            }
        }
    }

    /// Request open-interest history for the venues this overlay reads, on a
    /// change of (symbol, aggregate) and then on a slow refresh tick. The
    /// history moves on the venue's own ladder (5m at the finest), so
    /// re-pulling faster than that only burns rate limit.
    fn ensure_oi_history(
        &mut self,
        bid: BubbleId,
        symbol: &str,
        own_venue: digdigdig3::ExchangeId,
        aggregate: bool,
        bar_ms: i64,
        now_ms: i64,
        venues: &[digdigdig3::ExchangeId],
    ) {
        let want_key: Vec<String> = venues.iter().map(|e| e.as_str().to_string()).collect();
        let fresh = self.oi_hist_armed.get(&bid).is_some_and(|(s, a, v, at_ms)| {
            s == symbol && *a == aggregate && *v == want_key && now_ms - *at_ms < OI_REFRESH_MS
        });
        if fresh {
            return;
        }

        let period = mlc_backend_data_bridge::oi_history_period(bar_ms);
        let venues: Vec<digdigdig3::ExchangeId> = if aggregate {
            venues.to_vec()
        } else {
            vec![own_venue]
        };
        for eid in venues {
            if !self
                .sidebar_state
                .connector_enabled
                .get(eid.as_str())
                .copied()
                .unwrap_or(true)
            {
                continue;
            }
            self.core.ensure_connector(eid);
            // Futures always: open interest is a derivatives quantity, and a
            // spot chart asking for the projection wants that instrument's
            // futures OI rather than nothing.
            self.core.bridge_handle().request_oi_history(
                eid,
                symbol,
                digdigdig3::AccountType::FuturesCross,
                period,
                500,
            );
        }
        crate::sglog!(
            "[bubble {:?}] OI history requested ({} period={} aggregate={} venues={})",
            bid, symbol, period, aggregate, want_key.join(",")
        );
        self.oi_hist_armed
            .insert(bid, (symbol.to_string(), aggregate, want_key, now_ms));
    }

    /// Recompute the cached levels when the inputs changed.
    fn rebuild_liq_projection(
        &mut self,
        bid: BubbleId,
        own_venue: digdigdig3::ExchangeId,
        aggregate: bool,
        bar_ms: i64,
    ) {
        use mlc_core::chart::liquidation_projection as proj;

        let params = self.liq_projection_params(bid);
        // Fingerprint only what changes the OUTPUT.
        let param_hash = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for t in &params.tiers {
                t.leverage.to_bits().hash(&mut h);
                t.weight.to_bits().hash(&mut h);
            }
            params.mmr.to_bits().hash(&mut h);
            params.row_pct.to_bits().hash(&mut h);
            params.long_share.to_bits().hash(&mut h);
            params.min_usd.to_bits().hash(&mut h);
            aggregate.hash(&mut h);
            h.finish()
        };
        let own = own_venue.as_str().to_string();

        self.core.bubbles_mut().with_bubble_mut(bid, |bubble| {
            let bars_len = bubble.bars.len();
            let sources: Vec<(String, Vec<(i64, f64)>)> = bubble
                .oi_hist
                .iter()
                .filter(|(venue, _)| aggregate || **venue == own)
                .map(|(venue, rows)| (venue.clone(), rows.clone()))
                .collect();
            let oi_rows: usize = sources.iter().map(|(_, r)| r.len()).sum();
            let key = (bars_len, oi_rows.wrapping_add(sources.len() << 24), param_hash);
            if bubble.liq_projection_key == key && bubble.liq_projection.is_some() {
                return;
            }
            let series = proj::fold_oi(&sources, bar_ms.max(1));
            let levels = if bars_len > 0 && series.len() >= 2 {
                proj::project(&bubble.bars, &series, &params)
            } else {
                // No open-interest series yet: an EMPTY projection, not a
                // stale one. Better a layer that visibly has nothing than
                // bands modelled from another instrument's history.
                Vec::new()
            };
            bubble.liq_projection = Some(levels);
            bubble.liq_projection_key = key;
        });
    }
}
