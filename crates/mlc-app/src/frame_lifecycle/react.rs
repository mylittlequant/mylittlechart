//! Per-app reactor: consume [`BubbleEvent`]s emitted by [`BackendCore::tick_writers`]
//! and run per-window side-effects (viewport snap, `pending_symbol_load` clear,
//! indicator recalc trigger, persistence flush).
//!
//! The write half (inserting data into the shared bubble store) has already
//! happened inside `tick_writers`. This module is the read/react half.

use crate::ChartApp;
use crate::WarmupBudget;
use crate::chart_container::DEFAULT_SNAP_MARGIN;
use mlc_backend::BubbleEvent;
use mlc_backend_indicator_engine::RecalcMode;

impl ChartApp {
    /// React to one [`BubbleEvent`] from the backend writer.
    ///
    /// Handles per-app side-effects: viewport reconcile, `pending_symbol_load`
    /// clear, cold-snap, indicator recalc trigger, backfill subscription,
    /// panel state ticks. Does NOT touch the shared bubble store directly
    /// (the write already happened in `tick_writers`).
    pub(crate) fn react_to_bubble_event(&mut self, event: &BubbleEvent, now_ms: u64) {
        let _ = now_ms;
        match event {
            // ── BarsLoaded ─────────────────────────────────────────────────────
            BubbleEvent::BarsLoaded { bid, exchange_id, symbol, account_type, timeframe: tf_name, bars, prev_bars } => {
                if bid.0 == 0 {
                    // A venue no bubble charts — which is exactly what the
                    // profile venue fold asked for. Anything else with a
                    // zero id is ignored inside.
                    self.absorb_profile_bars(*exchange_id, symbol, tf_name, bars);
                }
                if bid.0 == 0 {
                    // No registered bubble — reactor still needs to handle parked presets
                    // and initial bar delivery to newly-created windows.
                    // Falls through to the parked-preset path below.
                }

                // Short-circuit: no bars, nothing to do for viewport.
                let new_bars = bars;
                let event_account_type: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();

                let mut any_matched = false;
                let mut backfill_requests: Vec<(String, mlc_core::state::Timeframe, mlc_core::instrument::AccountType)> = Vec::new();
                let mut prev_close_mirrors: Vec<(mlc_core::state::ChartId, Option<f64>)> = Vec::new();

                let instrument_by_cid: std::collections::HashMap<mlc_core::state::ChartId, (String, mlc_core::state::Timeframe, String, mlc_core::instrument::AccountType)> = {
                    self.panel_app.panel_grid.containers().keys()
                        .filter_map(|cid| {
                            let sym = self.symbol_for_chart(*cid)?;
                            let tf = self.timeframe_for_chart(*cid)?;
                            let exch = self.exchange_for_chart(*cid)?;
                            let at: mlc_core::instrument::AccountType = self.account_type_for_chart(*cid)?;
                            Some((*cid, (sym, tf, exch, at)))
                        })
                        .collect()
                };

                // Use the snapshot of the bubble's PRIOR bars taken by
                // `tick_writers` before it overwrote bubble.bars with the new
                // payload. Re-reading the bubble here yields `new_bars` (the
                // same data) and collapses the anchor onto itself.
                let prev_bars_for_anchor: &[mlc_core::Bar] = prev_bars;

                // Collect matched windows + anchor info.
                //
                // Match by `window.current_tag_id == *bid` (NOT by instrument).
                // tick_writers emits one BarsLoaded per matching bubble; if we
                // matched by instrument key (sym/exch/tf/at) here, every BarsLoaded
                // would touch EVERY chart on the same instrument — N charts × M
                // bubbles = N×M autoscale calls per cure REST.  The visible
                // symptom: any tag op (which enqueues a cure for the joining
                // bubble) triggers a `calc_auto_scale` on unrelated charts of
                // the same symbol; their Y axes drift even though their X is
                // untouched and they have nothing to do with the tag op.
                //
                // The bid carried by the event is the bubble whose `bars` was
                // overwritten in tick_writers — only charts pointing at THAT
                // bubble should reconcile.
                let mut matched_windows: Vec<(mlc_core::state::ChartId, bool, Option<(i64, f64)>)> = Vec::new();
                for (chart_id_key, window) in self.panel_app.panel_grid.containers().iter() {
                    let (win_sym, win_tf, win_exch, win_at) = match instrument_by_cid.get(chart_id_key) {
                        Some(t) => t,
                        None => continue,
                    };
                    let matched = window.current_tag_id == *bid
                        && win_sym.as_str() == symbol.as_str()
                        && win_exch.as_str() == exchange_id.as_str()
                        && win_tf.name == *tf_name
                        && *win_at == event_account_type;
                    if matched {
                        any_matched = true;
                        // FRESH-LOAD only when the window is explicitly waiting
                        // for the first delivery of a new instrument
                        // (`pending_symbol_load=true`).  Empty `prev_bars` ALONE
                        // is not enough: a warm preset switch + cure-path
                        // `request_bars` legitimately fires `BarsLoaded` on a
                        // bubble whose bars haven't been populated yet, and
                        // treating that as a cold load resets viewport +
                        // scale_mode — exactly what owner saw (3-month
                        // X-jump, scale_mode flipped from Manual back to
                        // Auto on every tab switch).  Fresh = pending only.
                        let is_backfill = !window.pending_symbol_load;
                        let anchor = if is_backfill {
                            // Stale-snapshot guard (live-caught 2026-07-07,
                            // VPJUMP repro): two backfill-class events can
                            // stack in ONE drain. Event #2's `prev_bars`
                            // snapshot predates event #1's reconcile, so the
                            // window's `view_start` is already in POST-#1
                            // coordinates.
                            //
                            // Directional refinement (2026-07-18, trade-built
                            // 15s drift fix): only PROTUKHSHIY snapshot is
                            // dangerous — the one that's SHORTER than what the
                            // window already knows (event #2 pre-#1 snapshot).
                            // A snapshot LARGER than the window's bar_count is
                            // an honest big prepend/append — the exact case
                            // reconcile_view_keep_anchor was designed for, and
                            // exactly what trade-built reseed produces at 2Hz
                            // when the trade-store snapshot re-folds after
                            // TradeGapFilled / LayerBackfilled. Previously we
                            // skipped BOTH via `abs_diff > 2` and killed the
                            // trade-built compensation entirely — that path
                            // drifted the viewport right on every tick.
                            //
                            // Wider tolerance below (`< wc - 2`) preserves the
                            // original stack-race skip for the small-shorter
                            // case while letting the large-larger case through.
                            let wc = window.viewport().bar_count;
                            if wc != 0 && prev_bars_for_anchor.len() + 2 < wc {
                                log::warn!(
                                    "[VPANCHOR stale] BarsLoaded: snapshot len {} shorter than window bar_count {} — skipping anchor reconcile (stack-race)",
                                    prev_bars_for_anchor.len(), wc
                                );
                                None
                            } else {
                                window.capture_view_anchor(prev_bars_for_anchor)
                            }
                        } else {
                            None
                        };
                        matched_windows.push((*chart_id_key, is_backfill, anchor));
                    }
                }

                // Audit D8: `BarsLoaded` fires per-BUBBLE, but a bubble can be
                // displayed by peer windows on DIFFERENT chart_types (one
                // candles, one renko) — resolve each matched window's OWN
                // active series (kline bars still feed bar_count/autoscale
                // correctly for candle-family; a renko peer must key off its
                // own bricks instead). Snapshot before the mutable loop.
                let series_by_chart: std::collections::HashMap<mlc_core::state::ChartId, crate::active_series::OwnedActiveSeries> =
                    matched_windows.iter()
                        .map(|(cid, _, _)| (*cid, self.active_series_for_chart(*cid)))
                        .collect();

                // Reconcile loop.
                for (chart_id_key, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                    let Some((_, is_backfill, anchor)) = matched_windows.iter()
                        .find(|(cid, _, _)| cid == chart_id_key)
                        .copied()
                        .map(|t| Some(t))
                        .unwrap_or(None)
                    else { continue; };
                    let win_at = instrument_by_cid.get(chart_id_key).map(|(_, _, _, at)| *at).unwrap_or_default();
                    let win_sym = instrument_by_cid.get(chart_id_key).map(|(s, _, _, _)| s.clone()).unwrap_or_default();
                    let win_tf = instrument_by_cid.get(chart_id_key).map(|(_, tf, _, _)| tf.clone()).unwrap_or_else(mlc_core::state::Timeframe::h1);
                    let series = series_by_chart.get(chart_id_key)
                        .map(|s| s.as_active_series())
                        .unwrap_or(crate::active_series::ActiveSeries::Bars(new_bars));
                    if is_backfill {
                        // Detect right-edge state BEFORE reconcile: was the user
                        // viewing the most recent bars when this backfill arrived?
                        // If yes, the backfill (prepend of older bars) shouldn't
                        // sneak the viewport off the right edge — re-snap after.
                        // Use `viewport.bar_count` here (still the pre-reconcile
                        // value — only `reconcile_view_*` updates it) instead of
                        // `prev_bars_for_anchor.len()` which already reflects
                        // the post-tick_writers bubble state.
                        let was_at_end = {
                            let bs = window.viewport().bar_spacing.max(1.0);
                            let visible_f = window.viewport().chart_width / bs;
                            let prev_count = window.viewport().bar_count as f64;
                            let right_edge_vs = (prev_count + DEFAULT_SNAP_MARGIN - visible_f).max(0.0);
                            (window.viewport().view_start - right_edge_vs).abs() < 1.5
                                && window.viewport().chart_width > 0.0
                                && prev_count > 0.0
                        };
                        window.reconcile_view_keep_anchor(new_bars, series, anchor);
                        if window.needs_snap_after_backfill
                            && !series.is_empty()
                            && window.viewport().chart_width > 0.0
                        {
                            window.consume_cold_snap(series);
                        } else if was_at_end && !series.is_empty() {
                            // EDGE-GLUE (owner ruling 2026-07-07): re-snap only
                            // restores "was glued to live edge" — legal data-side move.
                            // User was on right edge; backfill prepended older
                            // bars — re-snap to keep them on the right edge.
                            window.snap_to_end(DEFAULT_SNAP_MARGIN);
                        }
                        let target = self.core.user().profile().domain.data_load.background_bar_count;
                        if target > 0 && (new_bars.len() as u32) < target {
                            backfill_requests.push((win_sym, win_tf, win_at));
                        }
                    } else {
                        // Global scale_mode persistence channel removed (viewport
                        // SWC arc, 2026-07-03) — fresh loads always seed Auto;
                        // `restore_scale_mode` (set by cold preset-load paths)
                        // overrides this inside `reconcile_view_fresh` when present.
                        window.seed_fresh_load(new_bars, series);
                        // Fix 3 (chart-type-engine-consolidation B1): keep the
                        // loading skeleton up until the event that actually
                        // delivers series data — mirrors the derived-Appended
                        // arm's guard below (`window.pending_symbol_load &&
                        // !series.is_empty()`). An install-time fold reseed
                        // that produces zero output bars (chart-type switch /
                        // param change landing before the re-armed backfill's
                        // first page arrives — fix 1/2 above) used to
                        // unconditionally clear `pending_symbol_load` right
                        // here, which made `has_any_series_data()`'s skeleton
                        // gate irrelevant: the window itself no longer
                        // considered itself "pending", so nothing re-triggers
                        // the cold-snap/loading-state bookkeeping once real
                        // bars land later via `LayerBackfilled`'s refold.
                        // Clearing only on non-empty matches the contract the
                        // derived-slot chart types (renko/pnf/kagi/tlb/tpo/
                        // footprint) already had.
                        if !series.is_empty() {
                            window.pending_symbol_load = false;
                        }
                        if window.needs_snap_after_backfill
                            && !series.is_empty()
                            && window.viewport().chart_width > 0.0
                        {
                            window.consume_cold_snap(series);
                        }
                        let target = self.core.user().profile().domain.data_load.background_bar_count;
                        if target > 300 {
                            backfill_requests.push((win_sym, win_tf, win_at));
                        }
                    }
                    prev_close_mirrors.push((*chart_id_key, new_bars.first().map(|b| b.open)));
                }

                // Write prev_close_price.
                for (chart_id, prev_close) in prev_close_mirrors {
                    if let Some(b) = self.bubble_for_chart(chart_id) {
                        self.core.bubbles_mut().set_prev_close(b, prev_close);
                    }
                }

                // Trigger background backfill.
                let bg_target = self.core.user().profile().domain.data_load.background_bar_count;
                for (sym, tf, at) in backfill_requests {
                    let at = mlc_backend_data_bridge::account_type_to_dig3(at);
                    self.core.request_background_backfill(*exchange_id, &sym, &tf, at, bg_target);
                }

                // Parked preset delivery.
                {
                    let matching_parked: Vec<(String, Vec<mlc_core::state::ChartId>)> =
                        self.live_preset_cache
                            .iter()
                            .map(|(preset_key, parked)| {
                                let cids: Vec<mlc_core::state::ChartId> = parked
                                    .panel_grid
                                    .containers()
                                    .keys()
                                    .filter(|cid| {
                                        let Some(bid) = parked.tag_manager.tag_for_chart(**cid) else { return false };
                                        parked.bubble_store.symbol(bid).map_or(false, |s| s == *symbol)
                                            && parked.bubble_store.exchange(bid).map_or(false, |e| e == exchange_id.as_str())
                                            && parked.bubble_store.timeframe(bid).map_or(false, |tf| tf.name == *tf_name)
                                            && parked.bubble_store.account_type(bid).map_or(false, |at| at == account_type.as_str())
                                    })
                                    .copied()
                                    .collect();
                                (preset_key.clone(), cids)
                            })
                            .filter(|(_, cids)| !cids.is_empty())
                            .collect();

                    for (preset_key, cids) in matching_parked {
                        if let Some(parked) = self.live_preset_cache.get_mut(&preset_key) {
                            let parked_bubble_ids: Vec<mlc_core::ids::BubbleId> = cids.iter()
                                .filter_map(|cid| parked.tag_manager.tag_for_chart(*cid))
                                .collect::<std::collections::HashSet<_>>()
                                .into_iter()
                                .collect();
                            for bid_p in &parked_bubble_ids {
                                parked.bubble_store.set_bars(*bid_p, new_bars.clone());
                            }
                            for cid in cids {
                                // Audit D8: resolve this parked window's OWN
                                // chart_type against the parked BubbleStore
                                // (not always the just-replaced kline array).
                                let bid_for_series = parked.tag_manager.tag_for_chart(cid);
                                let chart_type = bid_for_series
                                    .and_then(|b| parked.bubble_store.chart_type(b))
                                    .unwrap_or("candles");
                                let series_owned = bid_for_series.map(|b| crate::active_series::resolve_active_series_owned_from_store(
                                    chart_type, &parked.bubble_store, b,
                                )).unwrap_or_default();
                                if let Some(window) = parked.panel_grid.containers_mut().get_mut(&cid) {
                                    let is_backfill = !window.pending_symbol_load
                                        && window.viewport().bar_count > 0;
                                    if is_backfill {
                                        window.reconcile_view_keep_anchor(new_bars, series_owned.as_active_series(), None);
                                    } else {
                                        window.reconcile_view_fresh(new_bars, series_owned.as_active_series());
                                        window.pending_symbol_load = false;
                                    }
                                }
                            }
                        }
                    }
                }

                // data_provider cache.
                if any_matched {
                    if let Some(w) = self.panel_app.panel_grid.containers().iter()
                        .find(|(cid, _)| {
                            self.symbol_for_chart(**cid).map_or(false, |s| s == *symbol)
                                && self.exchange_for_chart(**cid).map_or(false, |e| e == exchange_id.as_str())
                                && self.timeframe_for_chart(**cid).map_or(false, |tf| tf.name == *tf_name)
                                && self.account_type_for_chart(**cid).map_or(false, |at| at == event_account_type)
                        })
                        .map(|(_, w)| w)
                    {
                        w.data_provider.insert_bars(symbol, tf_name, new_bars.clone());
                    }
                }

                // Indicator recalc — keyed by the bubble whose bars just changed.
                // `calculate_for_bubble` looks up instances via `by_bubble_id`;
                // passing chart_id here found nothing because indicators are
                // bubble-bound, not chart-bound.
                self.core.indicators_mut().recalc_bubble(*bid, new_bars);

                // Subscribe trades for matched windows — ALWAYS, regardless of
                // chart_type (unified-data-plane Stage Э1: every active bubble
                // keeps a live trade feed so `trade_layer` fills continuously,
                // not just tick-TF/footprint windows). `subscribe_trades` is
                // idempotent per (exchange_id, account_type, symbol) — gated
                // by the bridge's own `station_trade_subscribed` set, so a
                // repeat call for an already-live stream is a cheap no-op.
                //
                // The b67bda42 skip-for-trade-agg-bar gate that used to live
                // here (root cause fixed 2026-07-08) protected against a
                // DIFFERENT hazard: `TradeUpdatePassthrough`'s kline-candle-
                // builder writing a second series into a trade-agg-bar
                // bubble's `bubble.bars`. That protection is the bars-
                // ownership check inside `TradeUpdatePassthrough` itself
                // (`derived_cids_tu` / `bars_source != Kline` below) — it does
                // NOT depend on whether a subscription exists, so removing
                // this gate does not reintroduce the double-writer bug; it
                // only lets the WS stream reach that (already-guarded) arm
                // continuously, which is exactly what the raw trade layer
                // needs. Footprint has its own subscribe flow, untouched.
                if any_matched && self.sidebar_state.connector_enabled.get(exchange_id.as_str()).copied().unwrap_or(true) {
                    let at = mlc_backend_data_bridge::account_type_to_dig3(event_account_type);
                    self.core.subscribe_trades(*exchange_id, symbol, at);
                }

                // ── Pending compare retry ─────────────────────────────────────────
                // If BarsLoaded carries data for a compare symbol (not the main window
                // symbol), find queued PendingCompareAdd entries that match, fill the
                // data_provider cache, and retry add_compare_symbol.
                {
                    // Collect indices of pending entries that match this BarsLoaded.
                    let matching_indices: Vec<usize> = self.pending_compare
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| {
                            p.symbol == *symbol
                                && p.exchange.as_str() == exchange_id.as_str()
                                && p.account_type_label == account_type.as_str()
                        })
                        .map(|(i, _)| i)
                        .collect();

                    if !matching_indices.is_empty() {
                        // Fill the data_provider cache on every container so that
                        // add_compare_symbol's cache lookup succeeds regardless of which
                        // window holds the overlay.
                        for w in self.panel_app.panel_grid.containers().values() {
                            w.data_provider.insert_bars(symbol, tf_name, new_bars.clone());
                        }

                        // Collect the pending entries we are going to consume (in reverse
                        // index order so swap_remove doesn't shift the wrong index).
                        let mut to_process: Vec<crate::PendingCompareAdd> = {
                            let mut idxs = matching_indices;
                            idxs.sort_unstable();
                            idxs.dedup();
                            idxs.into_iter().rev()
                                .map(|i| self.pending_compare.swap_remove(i))
                                .collect()
                        };
                        to_process.reverse(); // restore original order for determinism

                        for pending in to_process {
                            // Collect what the target window needs to know.
                            let cid = pending.chart_id;
                            let main_sym = self.symbol_for_chart(cid)
                                .map(String::from)
                                .unwrap_or_default();
                            let main_tf = self.timeframe_for_chart(cid)
                                .unwrap_or_else(mlc_core::state::Timeframe::h1);
                            let main_bars = self.panel_app.panel_grid.containers()
                                .get(&cid)
                                .and_then(|w| self.bubble_for_chart(cid)
                                    .map(|bid| {
                                        // Read bars from the bubble rather than re-fetching.
                                        let _ = w; // use w to satisfy borrow checker
                                        self.core.bubbles().bars(bid).to_vec()
                                    }))
                                .unwrap_or_default();

                            let added_series =
                                if let Some(window) = self.panel_app.panel_grid.containers_mut().get_mut(&cid) {
                                    let added = window.add_compare_symbol(
                                        &pending.symbol,
                                        &main_sym,
                                        &main_tf,
                                        &main_bars,
                                    );
                                    eprintln!(
                                        "[Compare] Retry {} on cid={}: {}",
                                        pending.symbol,
                                        cid.0,
                                        if added { "added" } else { "still unavailable" }
                                    );
                                    if added {
                                        window.compare_overlay.get_series(&pending.symbol).cloned()
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                };

                            if let Some(series) = added_series {
                                self.push_undo_command(mlc_core::Command::AddCompareSeries {
                                    series: series.without_bars(),
                                });
                                eprintln!("[Compare] Recorded AddCompareSeries {} (retry)", pending.symbol);
                                self.autosave_snapshot();
                            }
                        }
                    }
                }
            }

            // ── BackfillCompleted ───────────────────────────────────────────────
            BubbleEvent::BackfillCompleted { bid, exchange_id, symbol, account_type, timeframe: tf_name, bars, prev_bars } => {
                let new_bars = bars;
                let event_account_type: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
                let mut prev_close_mirrors: Vec<(mlc_core::state::ChartId, Option<f64>)> = Vec::new();

                let instrument_by_cid: std::collections::HashMap<mlc_core::state::ChartId, (String, mlc_core::state::Timeframe, String, mlc_core::instrument::AccountType)> = {
                    self.panel_app.panel_grid.containers().keys()
                        .filter_map(|cid| {
                            let sym = self.symbol_for_chart(*cid)?;
                            let tf = self.timeframe_for_chart(*cid)?;
                            let exch = self.exchange_for_chart(*cid)?;
                            let at = self.account_type_for_chart(*cid)?;
                            Some((*cid, (sym, tf, exch, at)))
                        })
                        .collect()
                };

                // Use the snapshot of bubble's PRIOR bars captured in
                // tick_writers before bubble.bars was overwritten. Re-reading
                // bubble here would give back `new_bars` and collapse the
                // anchor — see BarsLoaded branch for the same fix.
                let prev_bars_for_anchor: &[mlc_core::Bar] = prev_bars;

                let mut matched_anchors: Vec<(mlc_core::state::ChartId, Option<(i64, f64)>, bool)> = Vec::new();
                for (chart_id_key, window) in self.panel_app.panel_grid.containers().iter() {
                    let (win_sym, win_tf, win_exch, win_at) = match instrument_by_cid.get(chart_id_key) {
                        Some(t) => t,
                        None => continue,
                    };
                    // Match by current_tag_id == *bid (see BarsLoaded branch
                    // for the rationale — broadcasting backfill events across
                    // unrelated charts of the same instrument was leaking Y-axis
                    // moves on any tag op).
                    if !(window.current_tag_id == *bid
                        && win_sym.as_str() == symbol.as_str()
                        && win_exch.as_str() == exchange_id.as_str()
                        && win_tf.name == *tf_name
                        && *win_at == event_account_type)
                    {
                        continue;
                    }
                    // Right-edge detection BEFORE reconcile — see BarsLoaded
                    // branch for the rationale. Use the still-pre-reconcile
                    // `viewport.bar_count` not `prev_bars.len()`.
                    let was_at_end = {
                        let bs = window.viewport().bar_spacing.max(1.0);
                        let visible_f = window.viewport().chart_width / bs;
                        let prev_count = window.viewport().bar_count as f64;
                        let right_edge_vs = (prev_count + DEFAULT_SNAP_MARGIN - visible_f).max(0.0);
                        (window.viewport().view_start - right_edge_vs).abs() < 1.5
                            && window.viewport().chart_width > 0.0
                            && prev_count > 0.0
                    };
                    // Stale-snapshot guard — see BarsLoaded branch. Skip the
                    // anchor math when this event's pre-merge snapshot no
                    // longer matches the window's bookkeeping (an earlier
                    // event in the same drain already reconciled it).
                    // Directional refinement (2026-07-18): only PROTUKHSHIY
                    // snapshot (shorter than window's bar_count — event #2
                    // pre-dating event #1's reconcile) is dangerous. Bigger
                    // snapshot = honest large prepend/append — reconcile
                    // MUST run. Was killing trade-built compensation on
                    // reseed dumps (15s drift). See BarsLoaded branch for full
                    // rationale.
                    let anchor = {
                        let wc = window.viewport().bar_count;
                        if wc != 0 && prev_bars_for_anchor.len() + 2 < wc {
                            log::warn!(
                                "[VPANCHOR stale] BackfillCompleted: snapshot len {} shorter than window bar_count {} — skipping anchor reconcile (stack-race)",
                                prev_bars_for_anchor.len(), wc
                            );
                            None
                        } else {
                            window.capture_view_anchor(prev_bars_for_anchor)
                        }
                    };
                    matched_anchors.push((*chart_id_key, anchor, was_at_end));
                }

                // Audit D8: resolve each matched window's OWN active series
                // before the mutable loop (see BarsLoaded branch for the
                // cross-chart_type-peer rationale).
                let series_by_chart: std::collections::HashMap<mlc_core::state::ChartId, crate::active_series::OwnedActiveSeries> =
                    matched_anchors.iter()
                        .map(|(cid, _, _)| (*cid, self.active_series_for_chart(*cid)))
                        .collect();

                for (chart_id_key, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                    let Some((_, anchor, was_at_end)) = matched_anchors.iter().find(|(c, _, _)| c == chart_id_key).copied() else { continue; };
                    let series = series_by_chart.get(chart_id_key)
                        .map(|s| s.as_active_series())
                        .unwrap_or(crate::active_series::ActiveSeries::Bars(new_bars));
                    window.reconcile_view_keep_anchor(new_bars, series, anchor);
                    // chart_width == 0 → window not laid out yet (e.g. TC aux
                    // minichart with its tab closed): keep the flag armed, the
                    // per-frame deferred snap fires once geometry exists.
                    if window.needs_snap_after_backfill
                        && !series.is_empty()
                        && window.viewport().chart_width > 0.0
                    {
                        window.consume_cold_snap(series);
                    } else if was_at_end && !series.is_empty() {
                        // EDGE-GLUE (owner ruling 2026-07-07): re-snap only
                        // restores "was glued to live edge" — legal data-side move.
                        // User was at right edge before backfill prepended
                        // older bars — re-snap so the live tick stays visible.
                        window.snap_to_end(DEFAULT_SNAP_MARGIN);
                    } else if window.price_scale().scale_mode.is_auto_y() {
                        window.calc_auto_scale(series);
                    }
                    prev_close_mirrors.push((*chart_id_key, new_bars.first().map(|b| b.open)));
                }

                for (chart_id, prev_close) in prev_close_mirrors {
                    if let Some(b) = self.bubble_for_chart(chart_id) {
                        self.core.bubbles_mut().set_prev_close(b, prev_close);
                    }
                }

                // Recalc by the event's BUBBLE id — instances hang off
                // `current_tag_id`, and ChartId/BubbleId come from separate
                // counters. The old `BubbleId(cid.0)` cast recalced a
                // usually-nonexistent bubble (owner report 2026-07-05); the
                // instrument-wide sweep that replaced it recalced every
                // same-instrument bubble with THIS bubble's array — foreign
                // depth poisoning peer windows' indicator output (2026-08-01
                // F-chart incident). `new_bars` belongs to `*bid` alone;
                // peer bubbles receive their own per-bubble event from the
                // fan-out writer.
                self.core.indicators_mut().recalc_bubble(*bid, new_bars);
            }

            // ── ScrollBarsAppended ─────────────────────────────────────────────
            BubbleEvent::ScrollBarsAppended { bid, exchange_id, symbol, account_type, timeframe: tf_name, bars, prev_bars, prepend_count } => {
                let new_bars = bars;
                // F4 (viewport doctrine audit 2026-07-07): the in-flight
                // guard lives on the bubble now, not the window — clear it
                // once for the bubble this event actually landed on.
                //
                // Task 2 (derived-prepend-fold-plan-2026-07-07, "one batch
                // now, one batch lazy"): kline's `ScrollBarsLoaded` REST path
                // is the sole producer of this event (sets
                // `scroll_fetch_in_flight`) — the trade-agg family's own
                // scroll-left deepen now runs entirely client-side via
                // `TradeFoldRuntime`/`FoldRuntime` (unified-data-plane Э5b),
                // never touching this flag or this event. The lazy follow-up
                // prefetch below is kline-only by design (Task 2 scope).
                let was_kline_scroll_fetch = self.core.bubbles()
                    .with_bubble(*bid, |b| b.scroll_fetch_in_flight)
                    .unwrap_or(false);
                self.core.bubbles_mut().with_bubble_mut(*bid, |b| {
                    b.scroll_fetch_in_flight = false;
                    b.scroll_fetch_started = None;
                });
                // One prefetch pair per left-edge crossing: schedule exactly
                // ONE background follow-up page (same 500-bar batch size,
                // anchored at the NEW oldest bar) right after the first page
                // lands — "one batch now, one batch lazy". Guarded by
                // `scroll_prefetch_done` so a second `ScrollBarsAppended`
                // for the same bubble (e.g. the prefetch's own page landing)
                // does not chain a third fetch; `tick.rs`'s threshold check
                // clears the guard on the NEXT user-triggered crossing (by
                // re-arming `scroll_fetch_in_flight`, which this arm reads
                // fresh each time — no separate reset needed here).
                if was_kline_scroll_fetch && !new_bars.is_empty() && *prepend_count > 0 {
                    let already_prefetched = self.core.bubbles()
                        .with_bubble(*bid, |b| b.scroll_prefetch_done)
                        .unwrap_or(true);
                    if !already_prefetched {
                        if let Some(tf) = crate::parse_timeframe_name(tf_name) {
                            if let Some(oldest_ts) = new_bars.first().map(|b| b.timestamp) {
                                let eid = *exchange_id;
                                let at_prefetch: mlc_core::instrument::AccountType =
                                    account_type.parse().unwrap_or_default();
                                let at_dig3 = mlc_backend_data_bridge::account_type_to_dig3(at_prefetch);
                                self.core.bubbles_mut().with_bubble_mut(*bid, |b| {
                                    b.scroll_prefetch_done = true;
                                    // Re-arm the in-flight guard for the
                                    // prefetch's own flight — otherwise a
                                    // user crossing mid-prefetch fires a
                                    // second concurrent fetch on the same
                                    // bubble (the F4 race this flag exists
                                    // to prevent).
                                    b.scroll_fetch_in_flight = true;
                                    b.scroll_fetch_started = Some(web_time::Instant::now());
                                });
                                self.core.request_scroll_backfill(eid, symbol, &tf, at_dig3, oldest_ts, 500);
                            }
                        }
                    }
                } else if was_kline_scroll_fetch {
                    // Venue exhausted (empty page / no older bars spliced) —
                    // nothing to prefetch; leave the guard as-is so the next
                    // real crossing (fresh `scroll_fetch_in_flight` arm)
                    // starts a clean pair.
                    self.core.bubbles_mut().with_bubble_mut(*bid, |b| {
                        b.scroll_prefetch_done = false;
                    });
                }
                let event_account_type: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
                let mut prev_close_mirrors: Vec<(mlc_core::state::ChartId, Option<f64>)> = Vec::new();
                let mut any_matched = false;

                let instrument_by_cid: std::collections::HashMap<mlc_core::state::ChartId, (String, mlc_core::state::Timeframe, String, mlc_core::instrument::AccountType)> = {
                    self.panel_app.panel_grid.containers().keys()
                        .filter_map(|cid| {
                            let sym = self.symbol_for_chart(*cid)?;
                            let tf = self.timeframe_for_chart(*cid)?;
                            let exch = self.exchange_for_chart(*cid)?;
                            let at = self.account_type_for_chart(*cid)?;
                            Some((*cid, (sym, tf, exch, at)))
                        })
                        .collect()
                };

                // Pre-merge snapshot from event: pinned to bubble.bars state
                // BEFORE tick_writers' layer prepend + display rebuild ran
                // (P0 fix 2026-07-09). `capture_view_anchor` must index
                // `view_start` into this OLD coordinate space (pre-prepend),
                // otherwise the anchor collapses onto a bar `prepend_count`
                // positions further back in time than what was actually
                // visible before the merge. See BubbleEvent::
                // ScrollBarsAppended doc comment (mlc-backend/src/lib.rs).
                let prev_bars_for_anchor: &[mlc_core::Bar] = prev_bars;

                let mut matched_anchors: Vec<(mlc_core::state::ChartId, Option<(i64, f64)>)> = Vec::new();
                for (chart_id_key, window) in self.panel_app.panel_grid.containers().iter() {
                    let (win_sym, win_tf, win_exch, win_at) = match instrument_by_cid.get(chart_id_key) {
                        Some(t) => t,
                        None => continue,
                    };
                    // Match by `current_tag_id == *bid` like the BarsLoaded /
                    // BackfillCompleted arms — NOT by instrument alone. The
                    // ScrollBarsLoaded writer fans the page out per bubble and
                    // emits one event PER bubble with that bubble's own
                    // prev_bars/bars; two unsynced windows on the same
                    // instrument (main chart + trading-container chart) hold
                    // DIFFERENT bubbles whose arrays differ in depth. Without
                    // the bid gate each window also reconciled against the
                    // OTHER bubble's arrays: once the foreign array outgrew
                    // the window's own bar_count, the anchor was captured in
                    // foreign (deeper) coordinates and view_start walked off
                    // the window's own array by one page per event — the
                    // F-chart axis breakage of 2026-08-01 (candles collapse
                    // to dashes + zero-spike + blown scale ~10s after load).
                    if !(window.current_tag_id == *bid
                        && win_sym.as_str() == symbol.as_str()
                        && win_exch.as_str() == exchange_id.as_str()
                        && win_tf.name == *tf_name
                        && *win_at == event_account_type)
                    {
                        continue;
                    }
                    any_matched = true;
                    // Stale-snapshot guard — see BarsLoaded branch.
                    // Directional refinement 2026-07-18: shorter-than-window
                    // = stack-race (skip); larger = honest big prepend (run).
                    let anchor = {
                        let wc = window.viewport().bar_count;
                        if wc != 0 && prev_bars_for_anchor.len() + 2 < wc {
                            log::warn!(
                                "[VPANCHOR stale] ScrollBarsAppended: snapshot len {} shorter than window bar_count {} — skipping anchor reconcile (stack-race)",
                                prev_bars_for_anchor.len(), wc
                            );
                            None
                        } else {
                            window.capture_view_anchor(prev_bars_for_anchor)
                        }
                    };
                    matched_anchors.push((*chart_id_key, anchor));
                }

                // Audit D8: resolve each matched window's own active series
                // (see BarsLoaded branch for the cross-chart_type-peer rationale).
                let series_by_chart: std::collections::HashMap<mlc_core::state::ChartId, crate::active_series::OwnedActiveSeries> =
                    matched_anchors.iter()
                        .map(|(cid, _)| (*cid, self.active_series_for_chart(*cid)))
                        .collect();

                for (chart_id_key, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                    let Some((_, anchor)) = matched_anchors.iter().find(|(c, _)| c == chart_id_key).copied() else { continue; };
                    let series = series_by_chart.get(chart_id_key)
                        .map(|s| s.as_active_series())
                        .unwrap_or(crate::active_series::ActiveSeries::Bars(new_bars));
                    window.reconcile_view_keep_anchor(new_bars, series, anchor);
                    if window.price_scale().scale_mode.is_auto_y() {
                        window.calc_auto_scale(series);
                    }
                    prev_close_mirrors.push((*chart_id_key, new_bars.first().map(|b| b.open)));
                }

                for (chart_id, prev_close) in prev_close_mirrors {
                    if let Some(b) = self.bubble_for_chart(chart_id) {
                        self.core.bubbles_mut().set_prev_close(b, prev_close);
                    }
                }

                if any_matched {
                    // Recalc ONLY the event's own bubble: `new_bars` is THAT
                    // bubble's post-merge array. The instrument-wide sweep
                    // this replaces recalced every same-instrument bubble
                    // with a FOREIGN array — indicator output (VOLUME) sized
                    // to another bubble's depth, rendered against this
                    // bubble's own bars (the sparse-spike volume row of the
                    // 2026-08-01 F-chart incident). Peer bubbles get their
                    // own ScrollBarsAppended (the writer fans out per
                    // bubble), so nobody is starved by the narrowing.
                    self.core.indicators_mut().recalc_bubble(*bid, new_bars);
                }
            }

            // ── BarUpdatePassthrough ────────────────────────────────────────────
            BubbleEvent::BarUpdatePassthrough { exchange_id: _bar_exch, account_type: bar_account_type, symbol: bar_symbol, timeframe: bar_tf, bar: _bar } => {
                // A live tick for a venue the profiles fold. Cheap when
                // nothing folds: one map probe keyed by the event's own
                // (symbol, TF, venue).
                self.absorb_profile_bar_update(*_bar_exch, bar_symbol, bar_tf, _bar);
                let bar_event_at: mlc_core::instrument::AccountType = bar_account_type.parse().unwrap_or_default();

                let instrument_by_cid_bu: std::collections::HashMap<mlc_core::state::ChartId, (String, mlc_core::instrument::AccountType, bool, bool, String)> = {
                    self.panel_app.panel_grid.containers().keys()
                        .filter_map(|cid| {
                            let sym = self.symbol_for_chart(*cid)?;
                            let at = self.account_type_for_chart(*cid)?;
                            // NOT footprint: its bars come from FootprintUpdate
                            // and its viewport bookkeeping is driven exclusively
                            // by the FootprintBarAppended arm — letting a live
                            // kline BarUpdate through here could consume the
                            // one-shot cold snap against an empty/foreign series
                            // before the first footprint bar ever lands. Covers
                            // tick_bar (unified-data-plane Э5b: tick-TF kline
                            // windows are migrated to chart_type="tick_bar" on
                            // hydration, see `hydration_impl.rs`) as well as
                            // range/volume/dollar/imbalance/run bars.
                            let is_derived = self.chart_type_for_chart(*cid)
                                .map(crate::active_series::chart_type_is_trade_agg_bar)
                                .unwrap_or(false);
                            // TRADE-BUILT kline windows (owner incident
                            // 2026-07-16) get their per-append viewport
                            // bookkeeping (cold-snap consumption,
                            // set_bar_count, auto-scale, `advance_view_on_tick`
                            // with the len-derived is_new_bar) from THIS arm —
                            // the backend emits BarUpdatePassthrough for every
                            // trade-built live fold, and no other path
                            // advances their view (the wall-clock builder that
                            // accidentally did is now excluded, FIX 1).
                            let is_trade_built = self.core.tags().tag_for_chart(*cid)
                                .and_then(|bid| self.core.bubbles().with_bubble(bid, |b| b.active_trade_built_kline.is_some()))
                                .unwrap_or(false);
                            let exch = self.exchange_for_chart(*cid)?;
                            Some((*cid, (sym, at, is_derived, is_trade_built, exch)))
                        })
                        .collect()
                };

                let mut bu_symbol_for_recalc: Option<String> = None;
                // Per-window "did a new bar land" flag, fed to
                // `advance_view_on_tick`/`mark_new_bar` below — read-only
                // now (see the no-write doc comment below), derived per
                // window from `series.len()` vs. that window's own
                // previously-tracked `viewport().bar_count`, NOT from a
                // single instrument-wide bool the old write path used to
                // set (a bug in its own right: two windows on the same
                // instrument with different bubbles — e.g. two range_bar
                // panes at different R — could disagree on whether THIS
                // tick was their own new bar).
                let mut bu_is_new_bar = false;

                // Fold-owned bubbles (trade-agg-bar family: range/volume/
                // dollar/tick/tick_imbalance/volume_imbalance/run bars) —
                // NO write here. `BarUpdate`'s `bar` payload is the RAW
                // wall-clock 1m kline (`tick_writers`'s unconditional
                // passthrough at the bottom of the `LiveUpdate::BarUpdate`
                // arm), never the fold-derived bar — its OHLC/timestamp
                // identity has nothing to do with the fold's own chained
                // output (a $0.30 range bar vs an $18-tall 1m kline). The
                // backend is the single writer for these bubbles:
                // `fold_on_live_1m`/`trade_fold_on_live_trade`
                // (`mlc-backend/src/chart_type/lifecycle.rs`) already
                // appended/upserted the CORRECT chained bar(s) into
                // `bubble.bars` earlier in this SAME `tick_writers` call,
                // before this pass-through event was even pushed. Writing
                // the raw kline bar here too was landing it as a SECOND,
                // spurious full-height column per incoming 1m tick — one
                // extra bar right at the reseed/live seam, since the raw
                // kline's timestamp identity essentially never matches the
                // fold's own last bar identity (2026-07-10 "забор" bug:
                // 3-4 full-height columns at the history/live boundary on
                // 30R range_bar). This arm is now purely a repaint trigger +
                // viewport-bookkeeping signal for that family — see the
                // read-only `bu_is_new_bar` derivation below.
                //
                // TRADE-BUILT kline bubbles (owner incident 2026-07-16) join
                // this same repaint-trigger + viewport-bookkeeping role, for
                // a different reason: their writer is
                // `trade_built_kline_on_live_trade`
                // (`mlc-backend/src/chart_type/lifecycle.rs`), which folds
                // raw trades into `bubble.bars` earlier in the SAME
                // `tick_writers` drain — this arm never writes bars for them
                // either, it only reads the already-current length to derive
                // `is_new_bar_for_window` and drive the per-window viewport.

                for (cid_bu, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                    let (win_sym, win_at, is_derived, is_trade_built, win_exch) = match instrument_by_cid_bu.get(cid_bu) {
                        Some(t) => t,
                        None => continue,
                    };
                    // Trade-agg-bar (fold-owned) windows AND trade-built kline
                    // windows both reach viewport bookkeeping only through
                    // this arm — see the `is_trade_built` doc comment above.
                    // Every other window (native-TF kline, footprint) is
                    // driven by its own dedicated arm/passthrough.
                    if !*is_derived && !*is_trade_built {
                        continue;
                    }
                    // Exchange belongs to the instrument identity — without
                    // it a same-symbol window on a DIFFERENT venue consumed
                    // this event's viewport bookkeeping (same defect class as
                    // the trade-fold match below).
                    if win_sym.as_str() != bar_symbol
                        || win_exch.as_str() != _bar_exch.as_str()
                        || *win_at != bar_event_at
                    {
                        continue;
                    }
                    if window.pending_symbol_load {
                        window.pending_symbol_load = false;
                    }
                    // Resolved PER WINDOW (not once from an arbitrary shared
                    // instrument-wide bubble set): distinct windows on the
                    // same (symbol, account_type) can
                    // own distinct bubbles (different chart-type params, e.g.
                    // two range_bar windows with different range sizes, or
                    // range_bar + volume_bar both on the same instrument —
                    // `bubble_for_chart` resolves per-window via the tag
                    // system, not per-instrument). Broadcasting one bubble's
                    // array length to every window's viewport bookkeeping
                    // (cold-snap/autoscale/bar_count/advance) parked the
                    // viewport mid-history whenever the picked bubble's bar
                    // count diverged from a given window's own series.
                    let Some(bid_for_window) = self.core.tags().tag_for_chart(*cid_bu) else {
                        continue;
                    };
                    let bu_bubble_bars_owned: Vec<mlc_core::Bar> =
                        self.core.bubbles().bars(bid_for_window).to_vec();
                    // These windows are gated `is_derived` (trade-agg-bar
                    // family: range/volume/tick/dollar/imbalance/run) OR
                    // `is_trade_built` (trade-built kline TFs, e.g. bybit
                    // 15s) — in both cases the backend fold engine
                    // (`fold_on_live_1m`/`trade_fold_on_live_trade`/
                    // `trade_built_kline_on_live_trade`,
                    // `mlc-backend/src/chart_type/lifecycle.rs`) writes that
                    // series directly INTO bubble.bars, so it IS the kline
                    // array for this window (audit D8 resolution table) —
                    // plain `Bars` wrap is correct here. Footprint is NOT
                    // part of this arm (it reaches viewport bookkeeping via
                    // `FootprintBarAppended` instead, see the exclusion
                    // comment above).
                    let series = crate::active_series::ActiveSeries::Bars(&bu_bubble_bars_owned);
                    // Read-only "did a new bar land for THIS window" — no
                    // write happened above (see the no-write doc comment),
                    // so this compares the fold's already-current length
                    // against the window's own previously-tracked count
                    // instead of the write-path's former per-instrument
                    // append-vs-upsert bool.
                    let is_new_bar_for_window = series.len() > window.viewport().bar_count;
                    bu_is_new_bar = is_new_bar_for_window;
                    if window.needs_snap_after_backfill && window.viewport().chart_width > 0.0 {
                        window.consume_cold_snap(series);
                    }
                    window.set_bar_count(series.len());
                    if window.price_scale().scale_mode.is_auto_y() {
                        let _as_start = web_time::Instant::now();
                        window.calc_auto_scale(series);
                        self.last_auto_scale_us += _as_start.elapsed().as_micros() as u64;
                    }
                    // Live-tick follow: Follow mode snaps every tick; Auto mode
                    // advances view_start by one bar only when a new bar landed
                    // AND the prior last bar was already at the right edge.
                    window.advance_view_on_tick(is_new_bar_for_window);
                    bu_symbol_for_recalc = Some(bar_symbol.clone());
                }

                if let Some(ref sym) = bu_symbol_for_recalc {
                    let recalc_mode = self.core.indicators().recalc_mode();
                    match recalc_mode {
                        RecalcMode::PerTick => {
                            let _ri_start = web_time::Instant::now();
                            self.recalc_indicators_for_symbol(sym);
                            self.last_indicator_recalc_us += _ri_start.elapsed().as_micros() as u64;
                        }
                        RecalcMode::PerFrame => {
                            self.core.indicators_mut().mark_dirty(sym);
                        }
                        RecalcMode::PerBar => {
                            if bu_is_new_bar {
                                self.core.indicators_mut().mark_new_bar(sym);
                            } else {
                                self.core.indicators_mut().mark_dirty(sym);
                            }
                        }
                    }
                }
            }

            // ── TradeUpdatePassthrough ─────────────────────────────────────────
            BubbleEvent::TradeUpdatePassthrough { exchange_id, account_type, symbol, price, quantity, timestamp, .. } => {
                self.trade_count += 1;
                self.last_trade_event_count += 1;

                let _bar_apply_start = web_time::Instant::now();
                let trade_event_at: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();

                let instrument_by_cid_tu: std::collections::HashMap<mlc_core::state::ChartId, (String, mlc_core::state::Timeframe, String, mlc_core::instrument::AccountType)> = {
                    self.panel_app.panel_grid.containers().keys()
                        .filter_map(|cid| {
                            let sym = self.symbol_for_chart(*cid)?;
                            let tf = self.timeframe_for_chart(*cid)?;
                            let exch = self.exchange_for_chart(*cid)?;
                            let at = self.account_type_for_chart(*cid)?;
                            Some((*cid, (sym, tf, exch, at)))
                        })
                        .collect()
                };
                // Excludes trade-agg-bar windows (range/volume/tick/dollar/
                // imbalance/run bars — tick-TF kline windows are migrated to
                // chart_type="tick_bar" on hydration, see
                // `hydration_impl.rs`) — their bars come from the client-side
                // `TradeFoldRuntime` (`trade_fold_on_live_trade`, fired earlier
                // in `tick_writers` for the SAME live trade this event is a
                // passthrough of), never from this wall-clock candle builder.
                // Without this exclusion, raw trades would build a SECOND,
                // ordinary-OHLC candle series into the SAME `bubble.bars` the
                // fold's `BarUpdatePassthrough` output writes for these
                // windows (root cause fixed 2026-07-08, preserved by the
                // unified-data-plane Э5b cutover). `chart_type_is_trade_agg_bar`
                // is the same registry-driven check `BarUpdatePassthrough`'s
                // bars-ownership gate above uses — single source of truth.
                let derived_cids_tu: std::collections::HashSet<mlc_core::state::ChartId> = {
                    self.panel_app.panel_grid.containers().keys()
                        .filter(|cid| {
                            self.chart_type_for_chart(**cid)
                                .map(crate::active_series::chart_type_is_trade_agg_bar)
                                .unwrap_or(false)
                        })
                        .copied()
                        .collect()
                };

                let mut bubble_periods: std::collections::HashMap<mlc_core::ids::BubbleId, i64> = std::collections::HashMap::new();
                for (cid_tu, _window) in self.panel_app.panel_grid.containers().iter() {
                    if derived_cids_tu.contains(cid_tu) { continue; }
                    let (win_sym_tu, win_tf_tu, win_exch_tu, win_at_tu) = match instrument_by_cid_tu.get(cid_tu) {
                        Some(t) => t,
                        None => continue,
                    };
                    // Exchange is part of the instrument identity — matching
                    // on (symbol, account_type) alone folded a BTCUSDT:F
                    // trade from ANY venue into this window's live candle
                    // (every multi-venue tape and the LIQ roster subscribe
                    // the same base symbol across venues), so a foreign
                    // venue's tick moved a binance chart's close/volume.
                    if win_sym_tu.as_str() != symbol
                        || win_exch_tu.as_str() != exchange_id.as_str()
                        || *win_at_tu != trade_event_at
                    {
                        continue;
                    }
                    let period_secs = win_tf_tu.interval_ms / 1000;
                    if period_secs <= 0 { continue; }
                    if let Some(bid_tu) = self.bubble_for_chart(*cid_tu) {
                        bubble_periods.insert(bid_tu, period_secs);
                    }
                }

                let trade_ts_secs = timestamp / 1000;
                let mut is_new_bar = false;
                let mut needs_backfill = false;

                for (bid_tu, period_secs) in &bubble_periods {
                    let outcome = self.core.bubbles_mut().with_bubble_mut(*bid_tu, |bubble| {
                        // bars-ownership gate: only a `Kline`-owned bubble may
                        // have a wall-clock candle built from raw trades here.
                        // Footprint-owned bubbles get their OHLC from
                        // FootprintUpdate (its TF is a time bucket like "1m",
                        // never trade-agg-bar-excluded upstream — so this
                        // bars_source check is footprint's only protection in
                        // this arm). Trade-agg-bar bubbles are
                        // already excluded upstream via `derived_cids_tu`
                        // (`chart_type_is_trade_agg_bar`), but gating on "Kline
                        // owns" rather than naming FootprintOhlc alone makes
                        // this arm defend the invariant even if that upstream
                        // filter's set ever drifts from the ownership map.
                        if bubble.bars_source != mlc_backend::bubble::BarsSource::Kline {
                            return (false, false);
                        }
                        // TRADE-BUILT kline bubbles have their own single writer
                        // (`trade_built_kline_on_live_trade`, runs earlier in the SAME
                        // tick_writers drain) — this wall-clock builder double-writing was
                        // masked only by that ordering (it always saw the already-updated
                        // bucket), and its `needs_bf` re-request below is destructive for
                        // trade-built TFs (owner incident 2026-07-16: empty-replace +
                        // fresh-snap cascade pinned the viewport). Same exclusion class as
                        // the trade-agg-bar upstream filter.
                        if bubble.active_trade_built_kline.is_some() {
                            return (false, false);
                        }
                        if let Some(last_ts) = bubble.bars.last().map(|b| b.timestamp) {
                            let candle_end = last_ts + period_secs;
                            if trade_ts_secs >= candle_end {
                                let needs_bf = trade_ts_secs >= candle_end + period_secs;
                                let new_candle_start = (trade_ts_secs / period_secs) * period_secs;
                                bubble.append_bar(mlc_core::Bar {
                                    timestamp: new_candle_start,
                                    open: *price, high: *price, low: *price, close: *price, volume: *quantity,
                                    derived_ts_ms: 0,
                                });
                                (true, needs_bf)
                            } else {
                                if let Some(last) = bubble.last_bar_mut() {
                                    last.close = *price;
                                    if *price > last.high { last.high = *price; }
                                    if *price < last.low  { last.low  = *price; }
                                    last.volume += quantity;
                                }
                                (false, false)
                            }
                        } else {
                            let candle_start = (trade_ts_secs / period_secs) * period_secs;
                            bubble.append_bar(mlc_core::Bar {
                                timestamp: candle_start,
                                open: *price, high: *price, low: *price, close: *price, volume: *quantity,
                                derived_ts_ms: 0,
                            });
                            (true, false)
                        }
                    });
                    if let Some((new_bar, needs_bf)) = outcome {
                        if new_bar { is_new_bar = true; }
                        if needs_bf { needs_backfill = true; }
                    }
                }

                // Snapshot each affected window's active series ONCE before
                // the mutable iteration — the borrow-checker forbids
                // `active_series_for_chart(cid)` inside the mutable loop.
                let series_by_chart_tu: std::collections::HashMap<mlc_core::state::ChartId, crate::active_series::OwnedActiveSeries> =
                    instrument_by_cid_tu.iter()
                        .filter(|(cid, _)| !derived_cids_tu.contains(cid))
                        .filter(|(_, (s, _, _, at))| s == symbol && *at == trade_event_at)
                        .map(|(cid, _)| (*cid, self.active_series_for_chart(*cid)))
                        .collect();
                for (cid_tu, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                    if window.pending_symbol_load { continue; }
                    if derived_cids_tu.contains(cid_tu) { continue; }
                    let (win_sym_tu, _win_tf_tu, _win_exch_tu, win_at_tu) = match instrument_by_cid_tu.get(cid_tu) {
                        Some(t) => t,
                        None => continue,
                    };
                    if !(win_sym_tu.as_str() == symbol && *win_at_tu == trade_event_at) {
                        continue;
                    }
                    // Live tick Y-track for plain kline (candles/bars/line/…):
                    // owner doctrine 2026-07-06 — F Y-guard rides the tick.
                    // A-mode also refits so the whole visible range keeps in
                    // frame. Only F actually needs the tick-level precision
                    // (grow-only guard); A just keeps the existing behavior
                    // that the kline BarUpdate arm gets for derived windows.
                    if window.price_scale().scale_mode.is_auto_y() {
                        if let Some(owned) = series_by_chart_tu.get(cid_tu) {
                            window.calc_auto_scale(owned.as_active_series());
                        }
                    }
                    window.advance_view_on_tick(is_new_bar);
                }

                self.last_bar_apply_us += _bar_apply_start.elapsed().as_micros() as u64;

                if needs_backfill {
                    for cid in self.panel_app.panel_grid.containers().keys() {
                        // Footprint windows own their bar series — a kline
                        // REST refill would be dropped by the BarsLoaded
                        // ownership gate anyway; don't spend the request.
                        if self.chart_type_for_chart(*cid) == Some("footprint") { continue; }
                        let sym_match = self.symbol_for_chart(*cid).map_or(false, |s| s == *symbol);
                        let win_at = self.account_type_for_chart(*cid);
                        let at_match = win_at.map_or(false, |at| at == trade_event_at);
                        if sym_match && at_match {
                            let at_val = mlc_backend_data_bridge::account_type_to_dig3(win_at.unwrap_or_default());
                            if let Some(tf) = self.timeframe_for_chart(*cid) {
                                self.core.request_kline(*exchange_id, symbol, &tf, at_val, None, None, false);
                            }
                        }
                    }
                }

                // Panel ticks.
                let _trade_panel_start = web_time::Instant::now();
                for state in self.panels_store.big_trades.values_mut() { state.tick(); }
                for state in self.panels_store.volume_profile.values_mut() { state.tick(); }
                for state in self.panels_store.footprint.values_mut() { state.tick(); }
                for state in self.panels_store.trade_tape.values_mut() {
                    if state.shared_trades.is_none()
                        && !state.symbol.is_empty()
                        && !state.exchange.is_empty()
                    {
                        if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                            let at: mlc_core::instrument::AccountType = state.account_type.parse().unwrap_or_default();
                            let at = mlc_backend_data_bridge::account_type_to_dig3(at);
                            let h = self.core.subscribe_trades(eid, &state.symbol, at);
                            state.shared_trades = h;
                            state.last_seen_ts_ms = 0;
                        }
                    }
                    state.tick();
                }
                self.last_trade_panel_us += _trade_panel_start.elapsed().as_micros() as u64;

                let recalc_mode = self.core.indicators().recalc_mode();
                match recalc_mode {
                    RecalcMode::PerTick => {
                        let _ri_start = web_time::Instant::now();
                        self.recalc_indicators_for_symbol(symbol);
                        self.last_indicator_recalc_us += _ri_start.elapsed().as_micros() as u64;
                    }
                    RecalcMode::PerFrame => {
                        self.core.indicators_mut().mark_dirty(symbol);
                    }
                    RecalcMode::PerBar => {
                        if is_new_bar {
                            self.core.indicators_mut().mark_new_bar(symbol);
                        } else {
                            self.core.indicators_mut().mark_dirty(symbol);
                        }
                    }
                }
            }

            // ── ConnectorReady ─────────────────────────────────────────────────
            BubbleEvent::ConnectorReady { exchange_id } => {
                let eid_str = exchange_id.as_str();
                if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                    return;
                }
                let exchange_str = eid_str;
                // Feature: warm the watchlist (owner 2026-08-30) — "не
                // греются данные из списка вочлистов, а могли бы сразу
                // загружаться". A watchlist row click today always starts a
                // cold download; queue a background `request_kline` for
                // every row on this venue at the ACTIVE chart's timeframe
                // (falls back to the app default `Timeframe::h1()`, same
                // fallback used elsewhere in this file) so the row already
                // holds bars by the time it's clicked — the click still
                // force-fetches a fresh page on top (`panel_click.rs`'s
                // `apply_instrument_from_sidebar_click`, unchanged).
                // `drain_watchlist_warmup` (below) spends this queue against
                // a per-venue REST budget instead of firing the whole venue
                // at once.
                let warmup_tf = self.panel_app.panel_grid.active_container_id()
                    .and_then(|cid| self.timeframe_for_chart(cid))
                    .unwrap_or_else(mlc_core::state::Timeframe::h1);
                let wl_guard = self.core.watchlists();
                if let Some(wl) = wl_guard.find(self.watchlist_ui.active_list_id) {
                    for ws in wl.all_symbols() {
                        if ws.exchange == exchange_str {
                            let ws_at = mlc_backend_data_bridge::account_type_to_dig3(ws.account_type);
                            self.core.bridge_handle().subscribe_mini_ticker(*exchange_id, &ws.symbol, ws_at);
                            // `ConnectorReady` can refire for a venue that
                            // is already warm (reconnect) — only queue a
                            // key this session has never warmed. The
                            // bridge's own dedup is NOT a backstop here:
                            // REST-poll venues with no kline WS stream
                            // (dYdX, Gemini) never register a live
                            // subscriber, so a repeat non-force
                            // `request_kline` is not deduped for them and
                            // would otherwise re-fire forever.
                            if !self.watchlist_warmed.contains(&(ws.clone(), warmup_tf.clone())) {
                                self.watchlist_warmup_queue.push_back((*exchange_id, ws.clone(), warmup_tf.clone()));
                            }
                        }
                    }
                }
                drop(wl_guard);
                for (cid, _) in self.panel_app.panel_grid.containers().iter() {
                    // Footprint windows own their bar series (see BarsLoaded
                    // ownership gate) — skip the kline refetch.
                    if self.chart_type_for_chart(*cid) == Some("footprint") { continue; }
                    let sym = self.symbol_for_chart(*cid).unwrap_or_default();
                    if sym.is_empty() { continue; }
                    let exch = self.exchange_for_chart(*cid).unwrap_or_default();
                    let win_eid = digdigdig3::ExchangeId::from_str(&exch)
                        .unwrap_or(digdigdig3::ExchangeId::Binance);
                    if win_eid == *exchange_id {
                        let at: mlc_core::instrument::AccountType = self.account_type_for_chart(*cid).unwrap_or_default();
                        let at = mlc_backend_data_bridge::account_type_to_dig3(at);
                        if let Some(tf) = self.timeframe_for_chart(*cid) {
                            self.core.request_kline(*exchange_id, &sym, &tf, at, None, None, false);
                        }
                    }
                }
                if let Some(mut tm) = self.core.trading_mut() {
                    tm.on_connector_ready(*exchange_id);
                }
            }

            // ── ErrorReported ──────────────────────────────────────────────────
            BubbleEvent::ErrorReported { exchange_id, symbol, account_type, timeframe, message } => {
                // Two renderings of one classification (owner spec
                // 2026-08-30, revised): `status` is the short class label
                // alone for the narrow watchlist column; `chart_text` is
                // the class PLUS the literal cause for the chart's
                // empty-state text, which has room for a full sentence and
                // is where the user actually needs to see WHAT failed, not
                // just its category.
                let status = super::prepare::classify_watchlist_feed_error(message);
                let chart_text = super::prepare::classify_chart_feed_error(message);
                if !symbol.is_empty() {
                    let event_account_type: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
                    let error_instrument_map: std::collections::HashMap<mlc_core::state::ChartId, bool> = {
                        self.panel_app.panel_grid.containers().keys()
                            .filter_map(|cid| {
                                let win_exch = self.exchange_for_chart(*cid)?;
                                let win_sym = self.symbol_for_chart(*cid)?;
                                let win_at = self.account_type_for_chart(*cid)?;
                                let win_tf = self.timeframe_for_chart(*cid)?;
                                let matches = win_exch == exchange_id.as_str()
                                    && win_sym == *symbol
                                    && win_at == event_account_type
                                    && win_tf.name == *timeframe;
                                if matches { Some((*cid, true)) } else { None }
                            })
                            .collect()
                    };
                    let matched = !error_instrument_map.is_empty();
                    // Classified, not the raw dump — but the CHART gets the
                    // class-plus-cause text (`chart_text`), not the row's
                    // bare label; the row's `status` is deliberately
                    // shorter than what the chart can afford to show.
                    for (cid_err, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                        if error_instrument_map.contains_key(cid_err) {
                            window.data_error = Some(chart_text.clone());
                        }
                    }
                    let key = format!("{}:{}:{}", symbol, exchange_id.as_str(), account_type);
                    self.sidebar_state.feed_status.insert(key, status);
                    log::warn!(
                        "[data-error] {:?} {} at={} tf={}: {} (window_match={})",
                        exchange_id, symbol, account_type, timeframe, message, matched,
                    );
                } else {
                    // Connector/warmup-level error — no instrument to attach it
                    // to (empty symbol = venue-wide, e.g. connect_public /
                    // warmup failure). Stamp every watchlist row on this venue
                    // so the row can show the classified failure AND every
                    // open chart window on this venue, so a chart left with
                    // no bars explains itself instead of sitting silently
                    // empty (owner spec 2026-08-30 — this branch used to skip
                    // `window.data_error` entirely; only the per-instrument
                    // branch above touched it).
                    let exch = exchange_id.as_str();
                    let keys: Vec<String> = self.sidebar_state.watchlist_items.iter()
                        .filter(|it| it.exchange == exch)
                        .map(|it| format!("{}:{}:{}", it.symbol, it.exchange, it.account_type))
                        .collect();
                    for key in keys {
                        self.sidebar_state.feed_status.insert(key, status.clone());
                    }
                    let matching_windows: std::collections::HashSet<mlc_core::state::ChartId> = self
                        .panel_app.panel_grid.containers().keys()
                        .filter(|cid| self.exchange_for_chart(**cid).is_some_and(|e| e == exch))
                        .copied()
                        .collect();
                    if !matching_windows.is_empty() {
                        for (cid_err, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                            if matching_windows.contains(cid_err) {
                                window.data_error = Some(chart_text.clone());
                            }
                        }
                    }
                    log::warn!(
                        "[data-error] {:?} (connector-level, no instrument): {}",
                        exchange_id, message,
                    );
                }
            }

            // ── Derived chart types: snap + pending_symbol_load clear ──────────
            //
            // Audit D8 ("two arrays, one index"): this ONE shared arm used to
            // key bar_count for ALL SIX structurally distinct appended-series
            // events off `bubble.bars.len()` (the KLINE count) — wrong for
            // every peer window actually rendering renko/pnf/kagi/tlb (their
            // own array is a different length, so `visible_range()` computed
            // start>=end and the pane went blank until bars "popped in").
            // Each window now resolves its OWN active series from its OWN
            // chart_type (not from which event variant fired) — a window
            // displaying "candles" that happens to share this bubble with a
            // renko peer must NOT have its bar_count perturbed by a
            // RenkoBarAppended event that's irrelevant to what it renders.
            BubbleEvent::FootprintBarAppended { bid, exchange_id, symbol, account_type }
            | BubbleEvent::RenkoBarAppended { bid, exchange_id, symbol, account_type }
            | BubbleEvent::PnfBarAppended { bid, exchange_id, symbol, account_type }
            | BubbleEvent::KagiBarAppended { bid, exchange_id, symbol, account_type }
            | BubbleEvent::TpoSessionUpdated { bid, exchange_id, symbol, account_type }
            | BubbleEvent::TlbLineAppended { bid, exchange_id, symbol, account_type } => {
                let at_label = account_type.as_str();
                // Resolve target chart ids first (immutable borrow), then
                // snapshot each one's own active series before the mutable
                // containers_mut() loop.
                let target_cids: Vec<mlc_core::state::ChartId> = self.panel_app.panel_grid
                    .containers().iter()
                    .filter(|(_, w)| w.current_tag_id == *bid)
                    .map(|(cid, _)| *cid)
                    .collect();
                let series_by_chart: std::collections::HashMap<mlc_core::state::ChartId, crate::active_series::OwnedActiveSeries> =
                    target_cids.iter().map(|cid| (*cid, self.active_series_for_chart(*cid))).collect();
                let chart_type_by_chart: std::collections::HashMap<mlc_core::state::ChartId, &'static str> =
                    target_cids.iter().filter_map(|cid| self.chart_type_for_chart(*cid).map(|ct| (*cid, ct))).collect();
                for (cid_, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                    if window.current_tag_id != *bid { continue; }
                    let _ = (exchange_id, symbol, at_label);
                    let series = series_by_chart.get(cid_)
                        .map(|s| s.as_active_series())
                        .unwrap_or(crate::active_series::ActiveSeries::Bars(&[]));
                    // Keep the loading state up until the event that actually
                    // delivers bars — clearing on a zero-length notification
                    // kills the spinner while the pane is still blank.
                    if window.pending_symbol_load && !series.is_empty() {
                        window.pending_symbol_load = false;
                    }
                    // Cold snap (held armed until non-empty), prepend index
                    // compensation, right-edge follow — all inside the intent
                    // method (owner viewport doctrine 2026-07-05).
                    window.reconcile_derived_series(series);
                    // VPLOCKSTEP (viewport doctrine audit 2026-07-07,
                    // mechanism #2): footprint's viewport bookkeeping keys
                    // off `bubble.bars` while the renderer clamps against
                    // `footprint_bars` independently — diagnostic only, logs
                    // on desync so a future writer touching one array
                    // without the other is visible instead of surfacing as
                    // an unexplained jump.
                    //
                    // Excluded in hybrid mode (hybrid-data-modes-plan-2026-07-08,
                    // Step 3): a RecentOnly-venue footprint bubble stays on
                    // `BarsSource::Kline` by design — `footprint_bars` (the
                    // live-accumulated tail) is PERMANENTLY shorter than
                    // `bubble.bars` (the full kline backdrop) there, which is
                    // the intended hybrid shape, not a desync bug.
                    if chart_type_by_chart.get(cid_).copied() == Some("footprint")
                        && self.core.bubbles().bars_source(*bid) == mlc_backend::bubble::BarsSource::FootprintOhlc
                    {
                        // Bound first so the `bubbles()` read guard drops before the
                        // body runs (clippy::significant_drop_in_scrutinee).
                        let fp_bars_len = self.core.bubbles().with_bubble(*bid, |b| {
                            b.footprint_bars.as_ref().map(|fp| (fp.len(), b.bars.len()))
                        }).flatten();
                        if let Some((fp_len, bars_len)) = fp_bars_len {
                            if fp_len != bars_len {
                                log::warn!(
                                    "[VPLOCKSTEP] footprint desync bid={:?} footprint_bars.len()={fp_len} bubble.bars.len()={bars_len}",
                                    bid,
                                );
                            }
                        }
                    }
                    // Focus-mode Y-track: PriceScale::calc_auto_scale extends
                    // Y to include the last bar even when it exits the visible
                    // range (see mlc-core price_scale.rs Focus branch +
                    // chart_container.rs calc_auto_scale). Kline live-tick
                    // (BarUpdate arm ~676) drives this every tick; the derived
                    // Appended arm never called it, so Focus-Y stopped tracking
                    // last footprint/renko/pnf/kagi/tpo/tlb bars — the last
                    // bar's high/low drifted off-screen. Mirror the kline
                    // path's `is_auto_y` gate (covers both Auto and Focus).
                    if window.price_scale().scale_mode.is_auto_y() && !series.is_empty() {
                        window.calc_auto_scale(series);
                    }
                }
            }

            // ── CVD (demoted from chart_type — 2026-07-04 taxonomy fix) ────────
            //
            // `bubble.append_cvd` already ran upstream (`BackendCore::tick_writers`)
            // before this event fires. CVD needs no `pending_symbol_load`/
            // `consume_cold_snap`/`set_bar_count` bookkeeping of its own: its
            // viewport borrows kline bookkeeping (same as OI/Funding), which
            // the kline `BarUpdate` path already drives independently for
            // every candle-family window.
            //
            // What this arm DOES need to drive: the CVD indicator instance
            // (real compute-catalog "CVD", mli `BarIndicatorId::Cvd`) reads
            // its delta feed from `IndicatorsMut::recalc_bubble`'s
            // bubble-scoped delta builder (footprint_bars preferred,
            // cvd_line fallback) — a fresh sample just landed on `cvd_line`,
            // so the sub-pane must recalc or it never advances past the
            // last footprint-derived value.
            BubbleEvent::CvdSampleAppended { bid, .. } => {
                // Guard rule (mirrors the kline BarUpdate arms above): bind
                // facade reads BEFORE the match/if — a guard living in a
                // scrutinee spans the whole statement and aborts wasm when
                // the arm takes the same lock mutably (`indicators_mut`
                // co-acquires bubble.write). This arm was the one scrutinee
                // holdout; it only fires with a live cvd fold, which is why
                // it never tripped on the kline paths' cadence.
                let recalc_mode = self.core.indicators().recalc_mode();
                match recalc_mode {
                    RecalcMode::PerTick => {
                        let bars = self.core.bubbles().bars(*bid).to_vec();
                        self.core.indicators_mut().recalc_bubble(*bid, &bars);
                    }
                    RecalcMode::PerFrame | RecalcMode::PerBar => {
                        let sym = self.core.bubbles().symbol(*bid).map(|s| s.to_string());
                        if let Some(sym) = sym {
                            self.core.indicators_mut().mark_dirty(&sym);
                        }
                    }
                }
            }

            // ── OI / Funding pass-throughs ─────────────────────────────────────
            BubbleEvent::OiUpdatePassthrough { exchange_id: _, account_type, symbol, point } => {
                let event_account_type: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
                let target_cids: std::collections::HashSet<mlc_core::state::ChartId> =
                    self.panel_app.panel_grid.containers().iter()
                        .filter_map(|(cid, win)| {
                            if !self.overlay_instance_active(win.current_tag_id, Self::OI_DELTA_TYPE_ID) { return None; }
                            let s = self.symbol_for_chart(*cid)?;
                            let a = self.account_type_for_chart(*cid)?;
                            if s != *symbol || a != event_account_type { return None; }
                            Some(*cid)
                        }).collect();
                let bids: std::collections::HashSet<mlc_core::ids::BubbleId> =
                    target_cids.iter().filter_map(|cid| self.bubble_for_chart(*cid)).collect();
                for b in bids {
                    self.core.bubbles_mut().with_bubble_mut(b, |bubble| bubble.append_oi(*point));
                }
            }

            BubbleEvent::OiHistoryPassthrough { exchange_id, account_type: _, symbol, rows } => {
                // Account type is deliberately NOT matched: open interest is
                // a futures quantity, and a spot chart asking for the
                // projection wants that instrument's futures OI, not
                // nothing. Venue is the map key, so a fold sees every venue
                // that answered and a solo read picks its own.
                let venue = exchange_id.as_str().to_string();
                let bids: std::collections::HashSet<mlc_core::ids::BubbleId> = self
                    .panel_app
                    .panel_grid
                    .containers()
                    .keys()
                    .filter(|cid| self.symbol_for_chart(**cid).as_deref() == Some(symbol.as_str()))
                    .filter_map(|cid| self.bubble_for_chart(*cid))
                    .collect();
                for b in bids {
                    let venue = venue.clone();
                    let rows = rows.clone();
                    self.core.bubbles_mut().with_bubble_mut(b, |bubble| {
                        bubble.oi_hist.insert(venue, rows);
                    });
                }
            }

            BubbleEvent::FundingRatePassthrough { exchange_id: _, account_type, symbol, point } => {
                let event_account_type: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
                let target_cids: std::collections::HashSet<mlc_core::state::ChartId> =
                    self.panel_app.panel_grid.containers().iter()
                        .filter_map(|(cid, win)| {
                            if !self.overlay_instance_active(win.current_tag_id, Self::FUNDING_RATE_TYPE_ID) { return None; }
                            let s = self.symbol_for_chart(*cid)?;
                            let a = self.account_type_for_chart(*cid)?;
                            if s != *symbol || a != event_account_type { return None; }
                            Some(*cid)
                        }).collect();
                let bids: std::collections::HashSet<mlc_core::ids::BubbleId> =
                    target_cids.iter().filter_map(|cid| self.bubble_for_chart(*cid)).collect();
                for b in bids {
                    self.core.bubbles_mut().with_bubble_mut(b, |bubble| bubble.append_funding(*point));
                }
            }

            // ── DOM heatmap pass-throughs ──────────────────────────────────────
            //
            // Gate on `win.dom_heatmap_show` (chart-type → overlay
            // migration bool), NOT on `chart_type_for_chart` — the old
            // "dom_heatmap" chart_type id was removed from the registry
            // (`mlc-core/src/chart_type/registry.rs`), so a gate comparing
            // against it can never match (G1 fix, 2026-08-01: this arm
            // filtered every event away, silently starving the overlay's
            // ring buffer regardless of the toggle). Mirrors the OI/Funding
            // handlers above.
            BubbleEvent::DomHeatmapSnapshotPassthrough { exchange_id, account_type, symbol, ts_ms, bids, asks } => {
                let at_label = account_type.as_str();
                let event_account_type: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
                let target_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                    self.panel_app.panel_grid.containers().iter()
                        .filter_map(|(cid, win)| {
                            let _ = win;
                            if !self.dom_overlay_active_for_chart(*cid) { return None; }
                            let s = self.symbol_for_chart(*cid)?;
                            let a = self.account_type_for_chart(*cid)?;
                            if s != *symbol || a != event_account_type { return None; }
                            Some(*cid)
                        }).collect()
                };
                let bubble_ids: std::collections::HashSet<mlc_core::ids::BubbleId> =
                    target_cids.iter().filter_map(|cid| self.bubble_for_chart(*cid)).collect();
                // Grid geometry is (bar duration, row grouping): the bar comes
                // from the bubble's timeframe, the grouping from the overlay's
                // `row_ticks` param. Resolved before the write borrow — and a
                // change in either invalidates the grid, so the buffer is
                // rebuilt rather than fed cells of a second grain.
                #[allow(clippy::type_complexity)]
                let dom_geometry: Vec<(
                    mlc_core::ids::BubbleId,
                    i64,
                    i64,
                    f64,
                    bool,
                    Vec<digdigdig3::ExchangeId>,
                )> = bubble_ids
                    .iter()
                    .map(|b| {
                        let (row_ticks, band) = self.dom_heatmap_grid(*b);
                        let bar_ms = self
                            .core
                            .bubbles()
                            .timeframe(*b)
                            .map(|tf| tf.interval_ms)
                            .filter(|ms| *ms > 0)
                            .unwrap_or(60_000);
                        (
                            *b,
                            bar_ms,
                            row_ticks,
                            band,
                            self.dom_heatmap_aggregate(*b),
                            self.overlay_venues(*b, Self::DOM_HEATMAP_TYPE_ID),
                        )
                    })
                    .collect();
                // Aggregated source: fold the books of the venues THIS
                // overlay armed (frame_lifecycle/dom_books.rs). It used to
                // fold every book the workspace happened to have open, which
                // made the picture depend on what other panels were showing
                // and made the toggle a no-op with nothing else on screen.
                //
                // Keyed by the venue set, so two bubbles that fold the same
                // venues share one pass and two that do not each get their
                // own — the fold is per SET, not per event and not per
                // bubble.
                let mut folded_by_set: std::collections::HashMap<
                    String,
                    (Vec<(f64, f64)>, Vec<(f64, f64)>),
                > = std::collections::HashMap::new();
                {
                    use rust_decimal::prelude::ToPrimitive;
                    let at_dig3 = mlc_backend_data_bridge::account_type_to_dig3(event_account_type);
                    for (_, _, _, _, aggregate, venues) in dom_geometry.iter() {
                        if !*aggregate {
                            continue;
                        }
                        let key = venues.iter().map(|e| e.as_str()).collect::<Vec<_>>().join(",");
                        if folded_by_set.contains_key(&key) {
                            continue;
                        }
                        let mut sources = vec![(bids.clone(), asks.clone())];
                        for (venue, venue_at, tracker) in self.core.ob_trackers_for_symbol(symbol) {
                            // The host market is already in `sources` from the
                            // event itself — folding its tracker too would
                            // double every level it quotes. Both halves of the
                            // key matter: the same venue's spot and perp books
                            // are different markets and both belong in the fold.
                            if venue == *exchange_id && venue_at == at_dig3 {
                                continue;
                            }
                            if !venues.contains(&venue) {
                                continue;
                            }
                            let g = tracker.blocking_read();
                            let side = |v: Vec<(rust_decimal::Decimal, rust_decimal::Decimal)>| {
                                v.into_iter()
                                    .map(|(p, q)| (p.to_f64().unwrap_or(0.0), q.to_f64().unwrap_or(0.0)))
                                    .collect::<Vec<_>>()
                            };
                            sources.push((side(g.top_bids(500)), side(g.top_asks(500))));
                        }
                        folded_by_set.insert(key, mlc_core::chart::dom_heatmap::fold_books(&sources));
                    }
                }
                for (b, bar_ms, row_ticks, band, aggregate, venues) in dom_geometry {
                    let key = venues.iter().map(|e| e.as_str()).collect::<Vec<_>>().join(",");
                    let folded = folded_by_set.get(&key);
                    let (bids, asks): (&[(f64, f64)], &[(f64, f64)]) = match (aggregate, folded) {
                        (true, Some((fb, fa))) => (fb, fa),
                        _ => (bids, asks),
                    };
                    self.core.bubbles_mut().with_bubble_mut(b, |bubble| {
                        const DEFAULT_TICK: f64 = 0.5;
                        let slot = bubble.dom_heatmap_mut();
                        let stale = slot.as_ref().map_or(true, |buf| {
                            !buf.matches_geometry(bar_ms, row_ticks, band, aggregate)
                        });
                        if stale {
                            *slot = Some(mlc_core::chart::dom_heatmap::DomHeatmapBuffer::new(
                                DEFAULT_TICK, bar_ms, row_ticks, band, aggregate,
                            ));
                        }
                        if let Some(buf) = slot.as_mut() {
                            buf.push_snapshot(*ts_ms, bids, asks);
                        }
                    });
                }
                let dom_bubble_bars: Vec<mlc_core::Bar> = {
                    let mgr = self.core.bubbles();
                    mgr.bubble_for_instrument(exchange_id.as_str(), at_label, symbol)
                        .map(|b| mgr.bars(b).to_vec())
                        .unwrap_or_default()
                };
                for (cid_, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                    if !target_cids.contains(cid_) { continue; }
                    if window.pending_symbol_load { window.pending_symbol_load = false; }
                    if window.needs_snap_after_backfill && window.viewport().chart_width > 0.0 {
                        window.consume_cold_snap(crate::active_series::ActiveSeries::Bars(&dom_bubble_bars));
                    }
                }
            }

            // Gate on the overlay's indicator instance — see the snapshot arm above.
            BubbleEvent::DomTradeDotPassthrough { exchange_id: _, account_type, symbol, dot } => {
                let event_account_type: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
                let target_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                    self.panel_app.panel_grid.containers().iter()
                        .filter_map(|(cid, win)| {
                            let _ = win;
                            if !self.dom_overlay_active_for_chart(*cid) { return None; }
                            let s = self.symbol_for_chart(*cid)?;
                            let a = self.account_type_for_chart(*cid)?;
                            if s != *symbol || a != event_account_type { return None; }
                            Some(*cid)
                        }).collect()
                };
                let bids: std::collections::HashSet<mlc_core::ids::BubbleId> =
                    target_cids.iter().filter_map(|cid| self.bubble_for_chart(*cid)).collect();
                for b in bids {
                    self.core.bubbles_mut().with_bubble_mut(b, |bubble| {
                        const MAX_DOTS: usize = 2_000;
                        let dots = bubble.dom_trade_dots.get_or_insert_with(Vec::new);
                        if dots.len() >= MAX_DOTS { dots.remove(0); }
                        dots.push(*dot);
                    });
                }
            }


            // ── HistoryDepthReported: eternal-skeleton fix ──────────────────────
            //
            // hybrid-data-modes-plan-2026-07-08, Step 4. Traced behavior: a
            // batch-cut chart_type (tick_bar/volume_bar/dollar_bar/
            // tick_imbalance_bar/volume_imbalance_bar/run_bar) writes into
            // `bubble.bars` directly (same array as kline). On a `RecentOnly`
            // venue the cold seed (`backfill::trades_recent`, capped e.g. 60
            // trades on Bybit spot) can complete ZERO buckets for a chart_type
            // with a large threshold (tick_bar(100), volume_bar(1000)) — the
            // `TradeFoldRuntime::reseed` replay produces no output at all on a
            // zero-bucket seed, so no fresh-load-clearing event fires —
            // CONFIRMED HANG until enough live trades finally complete the
            // first bucket (could be arbitrarily long on a low-volume symbol).
            //
            // Fix: once the seed's depth is known to be a permanent ceiling
            // (`LiveOnly`) or a soft ceiling with nothing usable achieved yet
            // (`Capped { achieved: 0 }` — the REST seed ran and returned
            // nothing), AND the window is still waiting AND its own active
            // series is still empty, clear `pending_symbol_load` so the chart
            // shows the (empty) live-accumulation view instead of hanging.
            // `needs_snap_after_backfill` is deliberately left armed — it was
            // already set by `begin_fresh_load` at subscribe time and stays
            // armed through the empty period; the existing cold-snap
            // consumers (`BarUpdatePassthrough`'s `consume_cold_snap` call,
            // shared with kline) fire it on the first bar that actually
            // completes, so the viewport still snaps correctly the moment
            // live trades produce real data — this arm only stops the
            // spinner from lying about a fetch that structurally cannot
            // resolve via REST.
            BubbleEvent::HistoryDepthReported { bid, depth, .. } => {
                let seed_exhausted_empty = matches!(
                    depth,
                    mlc_backend::bubble::HistoryDepth::LiveOnly
                        | mlc_backend::bubble::HistoryDepth::Capped { achieved: 0 }
                );
                if seed_exhausted_empty {
                    let target_cids: Vec<mlc_core::state::ChartId> = self.panel_app.panel_grid
                        .containers().iter()
                        .filter(|(_, w)| w.current_tag_id == *bid && w.pending_symbol_load)
                        .map(|(cid, _)| *cid)
                        .collect();
                    for cid in target_cids {
                        if !self.active_series_for_chart(cid).as_active_series().is_empty() {
                            continue;
                        }
                        if let Some(window) = self.panel_app.panel_grid.containers_mut().get_mut(&cid) {
                            window.pending_symbol_load = false;
                        }
                    }
                }
            }

            BubbleEvent::MiniTickerUpdated { exchange_id, symbol, account_type, last_price } => {
                if *last_price > 0.0 {
                    let key = format!("{}:{}:{}", symbol, exchange_id.as_str(), account_type);
                    self.sidebar_state.feed_status.remove(&key);
                }
            }

            // ── Informational / no per-app reaction ────────────────────────────
            BubbleEvent::SymbolsLoaded { .. }
            | BubbleEvent::OrderbookSnapshotRest { .. }
            | BubbleEvent::OrderbookDelta { .. }
            | BubbleEvent::TradingUpdate { .. } => {
                // Handled by the inline accumulator logic in tick() before/after
                // calling this method. No additional window-level work needed here.
            }
        }
    }

    /// Drain `watchlist_warmup_queue` against each venue's token bucket in
    /// `watchlist_warmup_budget` (feature: "warm the watchlist", owner
    /// 2026-08-30). Called once per tick from `react_to_events`
    /// (`frame_lifecycle/tick.rs`), same cadence as `warmup_connectors`.
    ///
    /// Every queued row is checked every call — cheap (a hashmap lookup per
    /// row) — but only NOT-YET-WARMED rows whose venue currently has a
    /// spare token are actually sent; a row already recorded in
    /// `watchlist_warmed` is dropped here without spending a token (it can
    /// arrive stale from a duplicate enqueue — e.g. two `ConnectorReady`
    /// fires before the first copy drained). A row that IS still unwarmed
    /// but has no spare token is re-queued in its original order and
    /// retried on a later tick once the bucket refills — the "queue rather
    /// than drop" half of the rate limit, unaffected by the memory added
    /// here: a large watchlist on a tight venue (BitMEX ~30 req/min) still
    /// drains progressively instead of either blowing the venue's REST
    /// budget in one sweep or losing rows.
    ///
    /// `watchlist_warmed` is the stopping condition for "warm each row at
    /// most once" REGARDLESS of what the bridge does underneath — it is
    /// what stops THIS sweep from re-enqueuing/re-sending a key, not a
    /// side-effect of the bridge's own dedup. That distinction mattered in
    /// practice: venues that never register a live kline WS subscriber
    /// (dYdX, Gemini: REST-poll only) don't trip the bridge's subscriber-
    /// already-live skip, so a repeat call for them was a real network
    /// call every time — measured 2026-08-30, ~1.3 req/s sustained against
    /// dYdX with the app fully idle before this set existed.
    ///
    /// The send itself is `core.request_kline_once` (round 4 fix), not
    /// `request_kline(..., force=false)`: the ordinary path spawns a live
    /// `Stream::Kline` forwarder that stays subscribed forever, and for
    /// the same REST-poll venues that forwarder's own heal loop was
    /// re-fetching full candle history repeatedly (~1.5x/s), indefinitely,
    /// for rows the user never opened — `watchlist_warmed` stopped THIS
    /// sweep's repeats, but not the standing forwarder it had already
    /// started. `request_kline_once` fetches bars once and leaves nothing
    /// running.
    pub(crate) fn drain_watchlist_warmup(&mut self) {
        if self.watchlist_warmup_queue.is_empty() {
            return;
        }
        let now = web_time::Instant::now();
        let mut requeue: std::collections::VecDeque<(
            digdigdig3::ExchangeId,
            mlc_core::instrument::InstrumentAddress,
            mlc_core::state::Timeframe,
        )> = std::collections::VecDeque::new();
        while let Some((exchange_id, address, tf)) = self.watchlist_warmup_queue.pop_front() {
            let warm_key = (address, tf);
            if self.watchlist_warmed.contains(&warm_key) {
                continue;
            }
            let budget = self
                .watchlist_warmup_budget
                .entry(exchange_id)
                .or_insert_with(|| WarmupBudget::new(warmup_requests_per_minute(exchange_id), now));
            budget.refill(now);
            if budget.take() {
                let (address, tf) = &warm_key;
                let at = mlc_backend_data_bridge::account_type_to_dig3(address.account_type);
                self.core.ensure_connector(exchange_id);
                // One-shot fetch, not `request_kline(..., force=false)`
                // (owner spec 2026-08-30 round 4): the ordinary path spawns
                // a live `Stream::Kline` forwarder that stays subscribed
                // forever, and for REST-poll venues with no push stream
                // (dYdX, Gemini) that forwarder's heal loop was re-fetching
                // full candle history ~1.5x/s indefinitely for rows the
                // user never opened — `watchlist_warmed` stopped OUR
                // re-requests, but not the forwarder it had already
                // started. `request_kline_once` sends the same Level-1-
                // cache-then-Phase-A-REST `BarsLoaded` and leaves nothing
                // running (no-op skip if a live forwarder already covers
                // this key, e.g. an open chart on the same instrument). The
                // mini-ticker subscription above already carries live
                // price for the row; a warm row needs bars once, not a
                // standing subscription.
                self.core.request_kline_once(exchange_id, &address.symbol, tf, at);
                self.watchlist_warmed.insert(warm_key);
            } else {
                let (address, tf) = warm_key;
                requeue.push_back((exchange_id, address, tf));
            }
        }
        self.watchlist_warmup_queue = requeue;
    }
}

/// Per-venue REST budget for the watchlist warm-up sweep (rows/minute).
///
/// BitMEX rejects aggressively past roughly 30 requests/minute on its
/// public kline endpoint (owner-measured, 2026-08-30). Every other venue
/// defaults to a conservative generic ceiling until it, too, earns a
/// measured number — the point of a PER-venue budget (rather than one
/// shared number) is precisely so a tight venue like this one doesn't cap
/// every other venue's warm-up throughput.
fn warmup_requests_per_minute(exchange_id: digdigdig3::ExchangeId) -> u32 {
    match exchange_id {
        digdigdig3::ExchangeId::Bitmex => 30,
        _ => 60,
    }
}

// =============================================================================
// Regression: live-seam "забор" (fence) — BarUpdatePassthrough must not
// double-write fold-owned bubbles (2026-07-10)
// =============================================================================
//
// LIVE BUG: on a kline-fold chart type (range_bar/volume_bar/dollar_bar/
// tick_bar/tick_imbalance_bar/volume_imbalance_bar/run_bar), history renders
// as the correct chained staircase, but the live tail showed 3-4 FULL-HEIGHT
// vertical columns at the history/live seam — one per incoming 1m WS update.
//
// Root cause: `LiveUpdate::BarUpdate`'s `tick_writers` arm (mlc-backend
// lib.rs) unconditionally pushes a `BubbleEvent::BarUpdatePassthrough`
// carrying the RAW wall-clock 1m kline, in addition to the fold's OWN
// `fold_on_live_1m`-emitted event (which already appended/upserted the
// correctly chained bar(s) into `bubble.bars`). This arm used to ALSO write
// the raw kline bar into `bubble.bars` for every trade-agg-bar (fold-owned)
// bubble — landing it as a second, spurious, full-height bar per tick, since
// the raw kline's OHLC/timestamp identity has nothing to do with the fold's
// own chained-bar identity. Fixed by making this arm a pure repaint trigger
// + viewport-bookkeeping signal for that family — see the no-write doc
// comment on the `BarUpdatePassthrough` arm above.
#[cfg(test)]
mod live_seam_regression_tests {
    use super::*;
    use std::sync::Arc;

    use digdigdig3::core::types::{AccountType, ExchangeId};
    use mlc_backend_data_bridge::{DataBridge, LiveUpdate};
    use mlc_core::state::Timeframe;

    fn bar(ts_sec: i64, o: f64, h: f64, l: f64, c: f64, v: f64) -> mlc_core::Bar {
        mlc_core::Bar {
            timestamp: ts_sec,
            open: o, high: h, low: l, close: c, volume: v,
            derived_ts_ms: ts_sec * 1000,
        }
    }

    fn tf_m1() -> Timeframe { Timeframe::m1() }

    /// Build a `ChartApp` with one active range_bar window (bubble seeded,
    /// `active_fold` installed via the REAL `subscribe_fold_chart_type`
    /// entrypoint — same path `ChangeChartType` uses) reseeded over a
    /// synthetic "1m" layer trending up to `seed_close` (flat-ish, no
    /// crossings of its own so every assertion below is about the LIVE
    /// tail, not the seed replay).
    async fn build_range_bar_app(range: f64, seed_close: f64) -> (ChartApp, Arc<DataBridge>, mlc_core::ids::BubbleId) {
        let (bridge, _live_rx, _connector_ready_rx) = DataBridge::new().await;
        let bridge = Arc::new(bridge);

        let core = mlc_backend::BackendCore::new(bridge.clone(), None);
        let mut app = ChartApp::new_empty(core);
        app.seed_default_chart(ExchangeId::Binance, "BTCUSDT", tf_m1(), AccountType::Spot, 10);

        let active_cid = app.panel_app.panel_grid.active_container_id().expect("active container");
        let bid = app.bubble_for_chart(active_cid).expect("seeded bubble");

        // Seed the "1m" layer directly (no network) — a few perfectly flat
        // bars (no OHLC wobble — keeps the closed-bar height assertion
        // below unambiguous), then an in-progress bar sitting exactly at
        // `seed_close`.
        for i in 0..3i64 {
            let ts = (i + 1) * 60;
            app.core.bubbles_mut().kline_layer_push_live(
                bid, "1m", bar(ts, seed_close, seed_close, seed_close, seed_close, 1.0),
            );
        }
        let in_progress_ts = 4 * 60;
        app.core.bubbles_mut().kline_layer_push_live(
            bid, "1m", bar(in_progress_ts, seed_close, seed_close, seed_close, seed_close, 1.0),
        );

        app.core.bubbles_mut().set_chart_type(bid, "range_bar");
        let params = mlc_backend::bubble::FoldParams { scaled_units: range, ..Default::default() };
        let event = app.core.subscribe_fold_chart_type(
            bid, "range_bar", params, ExchangeId::Binance, "BTCUSDT", AccountType::Spot, Some(10),
        );
        if let Some(ev) = event {
            app.react_to_bubble_event(&ev, 0);
        }

        (app, bridge, bid)
    }

    fn now_ms() -> u64 {
        web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// The exact scenario the task calls for: reseed ends with an
    /// in-progress bar at price P; two live 1m updates land at P-10R (a
    /// single-leg gap-crossing delta big enough to chain through ~10 exact-R
    /// bars). Asserts `bubble.bars`' tail is the chain + one in-progress bar,
    /// no bar taller than R (excluding the organic in-progress one), and no
    /// duplicate identities — the exact шара the забор bug produced (extra
    /// full-height bars with colliding/foreign identities from the raw
    /// kline passthrough double-write).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn live_seam_chain_has_no_taller_than_range_bars_and_no_duplicate_identities() {
        const RANGE: f64 = 1.0;
        const SEED_CLOSE: f64 = 1000.0;
        let (mut app, bridge, bid) = build_range_bar_app(RANGE, SEED_CLOSE).await;

        let tx = bridge.debug_broadcast_tx();
        let in_progress_ts = 4 * 60;

        // Two live 1m updates for the SAME in-progress open_time, each
        // walking further down — together a P -> P-10R move. Mirrors the
        // task's "two live 1m updates at P-10R" scenario: split across two
        // ticks (not one single jump) so the fix is proven under the SAME
        // multi-event `tick_writers` drain shape the live bug hit (one
        // `BarUpdatePassthrough` raw-kline event PER incoming WS update).
        // `volume` is CUMULATIVE within the still-open bar (real WS kline
        // semantics — `KlineDeltaState::to_delta_trade` deltas against the
        // PREVIOUS call's volume, see digdigdig3-station derived.rs), so
        // each successive update carries a strictly larger running volume,
        // not a fresh per-tick amount.
        let mid = SEED_CLOSE - 5.0 * RANGE;
        let end = SEED_CLOSE - 10.0 * RANGE;
        for &(close, cum_volume) in &[(mid, 5.0), (end, 10.0)] {
            let _ = tx.send(LiveUpdate::BarUpdate {
                exchange_id: ExchangeId::Binance,
                account_type: AccountType::Spot,
                symbol: "BTCUSDT".to_string(),
                timeframe: "1m".to_string(),
                bar: bar(in_progress_ts, SEED_CLOSE, SEED_CLOSE, close, close, cum_volume),
            });
            app.tick(now_ms());
        }

        let bars = app.core.bubbles().bars(bid).to_vec();
        assert!(bars.len() >= 8, "expected a long chain of ~10 exact-R bars, got {} bars: {:?}", bars.len(), bars);

        // No bar taller than R, excluding the last (possibly still
        // in-progress, organically-widening) bar — this is exactly the
        // invariant the забор bug broke: a raw 1m kline bar landing in the
        // chain is $10+ tall at R=$1, i.e. 10x too tall.
        let (closed, _last) = bars.split_at(bars.len().saturating_sub(1));
        for (i, b) in closed.iter().enumerate() {
            let height = b.high - b.low;
            assert!(
                height <= RANGE + 1e-6,
                "closed bar #{i} has height {height} > R={RANGE} — забор regression \
                 (raw kline bar landed in the fold-owned chain): {b:?}",
            );
        }

        // No duplicate identities — every bar's (derived_ts_ms-or-timestamp)
        // identity must be distinct. The забор bug appended a SECOND bar per
        // tick with an identity that (almost always) never matched the
        // fold's own last-bar identity, inflating the array with
        // never-deduplicated extra columns.
        let mut seen = std::collections::HashSet::new();
        for b in &bars {
            let id = if b.derived_ts_ms != 0 { b.derived_ts_ms } else { b.timestamp };
            assert!(seen.insert(id), "duplicate bar identity {id} in tail — забор regression: {bars:?}");
        }
    }
}
