//! Bars from other venues, for the two profiles that aggregate them.
//!
//! Volume Profile and TPO count what the chart's OWN bars say. That makes a
//! single venue a sample: the price level that mattered is the level where
//! the market traded, and no one venue is the market.
//!
//! Folding them is a different problem from the liquidation and book folds,
//! and this is the whole reason it came last. Those two read rings some
//! other panel already fills; bars for a venue nobody is charting exist
//! nowhere until someone asks. So this roster ARMS a kline subscription per
//! venue — the same `request_bars` a chart uses for itself — and keeps the
//! answers in a side store, deliberately NOT on a bubble: a bubble is one
//! instrument's series and these are five other instruments' series, useful
//! only as profile input.
//!
//! ## Why the events land with `BubbleId(0)`
//!
//! `BackendCore::tick_writers` routes `BarsLoaded` to every bubble
//! registered for that instrument, and a venue nobody charts has none — so
//! it emits the event with a zero bubble id and no write. That path already
//! existed for "no registered bubble yet"; this is its first real consumer.
//!
//! ## Merging is concatenation, on purpose
//!
//! Both profiles are histograms over price: Volume Profile sums a bar's
//! volume into the bucket its price falls in, TPO counts the buckets a
//! bar's high-low span crosses. Feeding the union of every venue's bars is
//! therefore exactly the sum of the per-venue profiles on the shared grid,
//! and the shared grid comes out of the combined price range for free.
//!
//! What it is NOT is an averaged OHLC series. There is no such thing as the
//! market's candle — venues disagree on high and low by real amounts — and
//! fabricating one would put a price on the chart that never traded.

use mlc_core::ids::BubbleId;

/// Foreign bars are pulled once and then followed live. A venue that never
/// answers costs one dial per this interval, not a dial storm.
const PROFILE_KLINE_REPAIR_MS: i64 = 60_000;

impl crate::ChartApp {
    /// Arm kline subscriptions for the venues the profiles fold.
    pub(crate) fn ensure_profile_kline_rosters(&mut self) {
        #[allow(clippy::type_complexity)]
        let wanted: Vec<(
            BubbleId,
            String,
            mlc_core::state::Timeframe,
            mlc_core::instrument::AccountType,
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
                    let symbol = self.symbol_for_chart(cid)?;
                    if symbol.is_empty() {
                        return None;
                    }
                    // A tick chart has no kline interval to ask other venues
                    // for; the profile stays single-venue there rather than
                    // folding something else's minute bars into it.
                    let tf = self.timeframe_for_chart(cid).filter(|tf| !tf.is_tick())?;
                    let at = self.account_type_for_chart(cid)?;
                    let own = self.exchange_id_for_chart(cid);

                    let mut venues: Vec<digdigdig3::ExchangeId> = Vec::new();
                    for type_id in [Self::VOLUME_PROFILE_TYPE_ID, Self::TPO_PROFILE_TYPE_ID] {
                        if !self.profile_aggregate(bid, type_id) {
                            continue;
                        }
                        for eid in self.overlay_venues(bid, type_id) {
                            // The chart's own venue is already subscribed by
                            // the chart itself.
                            if eid != own && !venues.contains(&eid) {
                                venues.push(eid);
                            }
                        }
                    }
                    Some((bid, symbol, tf, at, venues))
                })
                .collect()
        };

        let now_ms = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        // What any live profile still wants, so the side store can drop the
        // rest instead of growing for the session.
        let mut keep: std::collections::HashSet<(String, String, digdigdig3::ExchangeId)> =
            std::collections::HashSet::new();

        for (bid, symbol, tf, at, venues) in wanted {
            for eid in &venues {
                keep.insert((symbol.clone(), tf.name.clone(), *eid));
            }

            let want_key: Vec<String> = venues.iter().map(|e| e.as_str().to_string()).collect();
            let fresh = self.profile_klines_armed.get(&bid).is_some_and(|(s, t, v, at_ms)| {
                s == &symbol && t == &tf.name && v == &want_key && now_ms - *at_ms < PROFILE_KLINE_REPAIR_MS
            });
            if fresh || venues.is_empty() {
                if venues.is_empty() {
                    self.profile_klines_armed.remove(&bid);
                }
                continue;
            }

            let at_dig3 = mlc_backend_data_bridge::account_type_to_dig3(at);
            for eid in &venues {
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
                self.core.bridge_handle().request_bars(
                    *eid,
                    &symbol,
                    &tf,
                    at_dig3,
                    None,
                    None,
                    false,
                );
            }
            crate::sglog!(
                "[bubble {:?}] profile klines armed ({} {} venues={})",
                bid, symbol, tf.name, want_key.join(",")
            );
            self.profile_klines_armed
                .insert(bid, (symbol.clone(), tf.name.clone(), want_key, now_ms));
        }

        if !self.profile_bars.is_empty() {
            self.profile_bars.retain(|k, _| keep.contains(k));
        }
    }

    /// Store bars that arrived for a venue no bubble charts, when some
    /// profile asked for them. Called from the `BarsLoaded` reactor.
    pub(crate) fn absorb_profile_bars(
        &mut self,
        exchange_id: digdigdig3::ExchangeId,
        symbol: &str,
        timeframe: &str,
        bars: &[mlc_core::Bar],
    ) {
        let key = (symbol.to_string(), timeframe.to_string(), exchange_id);
        // Only what a live roster asked for — a `BubbleId(0)` event can also
        // be an instrument the workspace merely touched.
        let wanted = self.profile_klines_armed.values().any(|(s, t, v, _)| {
            s == symbol && t == timeframe && v.iter().any(|e| e == exchange_id.as_str())
        });
        if !wanted {
            return;
        }
        self.profile_bars.insert(key, bars.to_vec());
    }

    /// Update the last stored bar for a venue, or append a new one — the
    /// live half of [`Self::absorb_profile_bars`].
    pub(crate) fn absorb_profile_bar_update(
        &mut self,
        exchange_id: digdigdig3::ExchangeId,
        symbol: &str,
        timeframe: &str,
        bar: &mlc_core::Bar,
    ) {
        let key = (symbol.to_string(), timeframe.to_string(), exchange_id);
        let Some(series) = self.profile_bars.get_mut(&key) else { return };
        match series.last_mut() {
            Some(last) if last.timestamp == bar.timestamp => *last = *bar,
            Some(last) if last.timestamp < bar.timestamp => series.push(*bar),
            None => series.push(*bar),
            // Out of order: a late update for a bar that is no longer the
            // tail. Dropping it costs one bar of one venue's volume and
            // keeps the series monotonic, which every consumer assumes.
            _ => {}
        }
    }

    /// The foreign bar series a bubble's profiles fold, in [t0, t1].
    ///
    /// Time-filtered rather than index-filtered: another venue's array has
    /// its own indices and its own gaps, so only the timestamps line up.
    pub(crate) fn profile_foreign_bars(
        &self,
        bid: BubbleId,
        type_id: &str,
        symbol: &str,
        tf_name: &str,
        t0: i64,
        t1: i64,
    ) -> Vec<mlc_core::Bar> {
        if !self.profile_aggregate(bid, type_id) {
            return Vec::new();
        }
        let mut out: Vec<mlc_core::Bar> = Vec::new();
        for eid in self.overlay_venues(bid, type_id) {
            let key = (symbol.to_string(), tf_name.to_string(), eid);
            let Some(series) = self.profile_bars.get(&key) else { continue };
            out.extend(
                series
                    .iter()
                    .filter(|b| {
                        let ts = b.ts_ms();
                        ts >= t0 && ts <= t1
                    })
                    .copied(),
            );
        }
        out
    }

    /// Whether `type_id`'s instance on `bid` folds other venues.
    pub(crate) fn profile_aggregate(&self, bid: BubbleId, type_id: &str) -> bool {
        self.core
            .indicators()
            .instances_for_bubble(bid)
            .into_iter()
            .find(|inst| inst.type_id == type_id && inst.visible)
            .and_then(|inst| inst.params.get("aggregate").and_then(|v| v.as_bool()))
            .unwrap_or(false)
    }
}
