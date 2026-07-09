//! Per-app reactor: consume [`BubbleEvent`]s emitted by [`BackendCore::tick_writers`]
//! and run per-window side-effects (viewport snap, `pending_symbol_load` clear,
//! indicator recalc trigger, persistence flush).
//!
//! The write half (inserting data into the shared bubble store) has already
//! happened inside `tick_writers`. This module is the read/react half.

use crate::ChartApp;
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
                            // coordinates — indexing it into the #2 snapshot
                            // clamps to the newest bar and teleports the view
                            // to the live edge. If the snapshot length doesn't
                            // match the window's current bookkeeping, skip the
                            // anchor math entirely (count-adopt only).
                            let wc = window.viewport().bar_count;
                            if wc != 0 && prev_bars_for_anchor.len().abs_diff(wc) > 2 {
                                log::warn!(
                                    "[VPANCHOR stale] BarsLoaded: snapshot len {} vs window bar_count {} — skipping anchor reconcile",
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
                        window.pending_symbol_load = false;
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
                    let anchor = {
                        let wc = window.viewport().bar_count;
                        if wc != 0 && prev_bars_for_anchor.len().abs_diff(wc) > 2 {
                            log::warn!(
                                "[VPANCHOR stale] BackfillCompleted: snapshot len {} vs window bar_count {} — skipping anchor reconcile",
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
                    if window.needs_snap_after_backfill {
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

                // Recalc by the window's BUBBLE id — instances hang off
                // `current_tag_id`, and ChartId/BubbleId come from separate
                // counters. The old `BubbleId(cid.0)` cast recalced a
                // usually-nonexistent bubble, leaving indicator values stale
                // (sized for the PRE-replace series) until the next
                // trade-tick recalc — visible as the volume histogram
                // vanishing for a beat after every bars reload (owner
                // report 2026-07-05).
                let matched_bids: Vec<mlc_core::ids::BubbleId> = self.panel_app.panel_grid.containers().iter()
                    .filter(|(cid, _)| {
                        self.symbol_for_chart(**cid).map_or(false, |s| s == *symbol)
                            && self.exchange_for_chart(**cid).map_or(false, |e| e == exchange_id.as_str())
                            && self.timeframe_for_chart(**cid).map_or(false, |tf| tf.name == *tf_name)
                            && self.account_type_for_chart(**cid).map_or(false, |at| at == event_account_type)
                    })
                    .filter_map(|(cid, _)| self.bubble_for_chart(*cid))
                    .collect();
                for b in &matched_bids {
                    self.core.indicators_mut().recalc_bubble(*b, new_bars);
                }
            }

            // ── ScrollBarsAppended ─────────────────────────────────────────────
            BubbleEvent::ScrollBarsAppended { bid, exchange_id, symbol, account_type, timeframe: tf_name, bars, prev_bars, prepend_count } => {
                let new_bars = bars;
                // F4 (viewport doctrine audit 2026-07-07): the in-flight
                // guard lives on the bubble now, not the window — clear it
                // once for the bubble this event actually landed on.
                //
                // Task 2 (derived-prepend-fold-plan-2026-07-07, "one batch
                // now, one batch lazy"): this event is shared by TWO
                // producers — kline's `ScrollBarsLoaded` REST path (sets
                // `scroll_fetch_in_flight`) and the trade-agg family's
                // `DerivedBarsPrepended` prepend-fold (driven by
                // `derived_seed_in_flight` instead, never touches this
                // flag). Reading `scroll_fetch_in_flight` BEFORE clearing it
                // is exactly how we tell "this was the kline scroll-left
                // page landing" from "this was a derived-family prepend
                // that happens to reuse the same merge/anchor machinery" —
                // the lazy follow-up prefetch below is kline-only by design
                // (Task 2 scope).
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
                // BEFORE tick_writers' merge_scroll_bars ran. `capture_view_anchor`
                // must index `view_start` into this OLD coordinate space (pre-
                // prepend), otherwise the anchor collapses onto a bar
                // `prepend_count` positions further back in time than what was
                // actually visible before the merge. See BubbleEvent::
                // ScrollBarsAppended doc comment (mlc-backend/src/lib.rs).
                let prev_bars_for_anchor: &[mlc_core::Bar] = prev_bars;

                let mut matched_anchors: Vec<(mlc_core::state::ChartId, Option<(i64, f64)>)> = Vec::new();
                for (chart_id_key, window) in self.panel_app.panel_grid.containers().iter() {
                    let (win_sym, win_tf, win_exch, win_at) = match instrument_by_cid.get(chart_id_key) {
                        Some(t) => t,
                        None => continue,
                    };
                    if !(win_sym.as_str() == symbol.as_str()
                        && win_exch.as_str() == exchange_id.as_str()
                        && win_tf.name == *tf_name
                        && *win_at == event_account_type)
                    {
                        continue;
                    }
                    any_matched = true;
                    // Stale-snapshot guard — see BarsLoaded branch.
                    let anchor = {
                        let wc = window.viewport().bar_count;
                        if wc != 0 && prev_bars_for_anchor.len().abs_diff(wc) > 2 {
                            log::warn!(
                                "[VPANCHOR stale] ScrollBarsAppended: snapshot len {} vs window bar_count {} — skipping anchor reconcile",
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
                    // Same BubbleId-not-ChartId fix as the BarsLoaded arm above.
                    let matched_bids: Vec<mlc_core::ids::BubbleId> = self.panel_app.panel_grid.containers().iter()
                        .filter(|(cid, _)| {
                            self.symbol_for_chart(**cid).map_or(false, |s| s == *symbol)
                                && self.exchange_for_chart(**cid).map_or(false, |e| e == exchange_id.as_str())
                                && self.timeframe_for_chart(**cid).map_or(false, |tf| tf.name == *tf_name)
                        })
                        .filter_map(|(cid, _)| self.bubble_for_chart(*cid))
                        .collect();
                    for b in &matched_bids {
                        self.core.indicators_mut().recalc_bubble(*b, new_bars);
                    }
                }
            }

            // ── BarUpdatePassthrough ────────────────────────────────────────────
            BubbleEvent::BarUpdatePassthrough { exchange_id: _bar_exch, account_type: bar_account_type, symbol: bar_symbol, bar } => {
                let bar_event_at: mlc_core::instrument::AccountType = bar_account_type.parse().unwrap_or_default();

                let instrument_by_cid_bu: std::collections::HashMap<mlc_core::state::ChartId, (String, mlc_core::instrument::AccountType, bool)> = {
                    self.panel_app.panel_grid.containers().keys()
                        .filter_map(|cid| {
                            let sym = self.symbol_for_chart(*cid)?;
                            let at = self.account_type_for_chart(*cid)?;
                            let is_tick_tf = self.timeframe_for_chart(*cid).map(|tf| tf.is_tick()).unwrap_or(false);
                            // NOT footprint: its bars come from FootprintUpdate
                            // and its viewport bookkeeping is driven exclusively
                            // by the FootprintBarAppended arm — letting a live
                            // kline BarUpdate through here could consume the
                            // one-shot cold snap against an empty/foreign series
                            // before the first footprint bar ever lands.
                            let derived_chart_type = self.chart_type_for_chart(*cid)
                                .map(crate::active_series::chart_type_is_trade_agg_bar)
                                .unwrap_or(false);
                            let is_derived = is_tick_tf || derived_chart_type;
                            Some((*cid, (sym, at, is_derived)))
                        })
                        .collect()
                };

                let mut bu_symbol_for_recalc: Option<String> = None;
                let mut bu_is_new_bar = false;

                // Bubble writes for derived windows.
                let target_bubbles_bu: std::collections::HashSet<mlc_core::ids::BubbleId> =
                    instrument_by_cid_bu.iter()
                        .filter(|(_, (sym, at, is_derived))| *is_derived && sym == bar_symbol && *at == bar_event_at)
                        .filter_map(|(cid, _)| self.bubble_for_chart(*cid))
                        .collect();

                // Trade-agg-bar-owned subset of `target_bubbles_bu` (excludes
                // tick-TF kline windows) — used below to defensively stamp
                // `bars_source` on the first derived bar that arrives for a
                // bubble the subscribe-time `clear_slot_trade_agg_bars` stamp
                // somehow missed (stale hydration path, future caller that
                // bypasses the `SubscribeOps` table). Belt-and-suspenders:
                // the OPS table stamp is the single source of truth in the
                // normal path, this only guards against that invariant
                // silently regressing.
                let trade_agg_bids_bu: std::collections::HashSet<mlc_core::ids::BubbleId> =
                    instrument_by_cid_bu.iter()
                        .filter(|(cid, (sym, at, _))| {
                            sym == bar_symbol && *at == bar_event_at
                                && self.chart_type_for_chart(**cid)
                                    .map(crate::active_series::chart_type_is_trade_agg_bar)
                                    .unwrap_or(false)
                        })
                        .filter_map(|(cid, _)| self.bubble_for_chart(*cid))
                        .collect();

                // Caterpillar-bug fix (2026-07-08): `bar.timestamp` is seconds-
                // truncated from dig3's ms-native `open_time` — the trade-agg
                // family (range/volume/tick/dollar bars, tick TFs) can close
                // more than once inside the same wall-clock second, so a
                // seconds-only same-bar test collapses genuinely distinct
                // bars into one (mutate instead of append) or vice versa.
                // Every window hitting this arm is `is_derived` (see the
                // filter above), so compare by `derived_ts_ms` whenever the
                // incoming bar actually carries one; kline's own `BarUpdate`
                // never reaches this arm (routed to the recalc path instead
                // via the non-derived branch), so this does not touch the
                // seconds-keyed kline path.
                let bar_ts_identity: Option<i64> = if bar.derived_ts_ms != 0 {
                    Some(bar.derived_ts_ms)
                } else {
                    Some(bar.timestamp)
                };

                for bid_bu in &target_bubbles_bu {
                    let is_trade_agg_owner = trade_agg_bids_bu.contains(bid_bu);
                    let is_new = self.core.bubbles_mut().with_bubble_mut(*bid_bu, |bubble| {
                        // bars-ownership gate: `BarUpdate` is a dual-purpose
                        // writer here — live kline ticks for tick-TF windows
                        // (`Kline`-owned) AND live trade-agg-bar-family ticks
                        // for range/volume/tick/dollar/imbalance/run windows
                        // (`TradeAggBars`-owned, per-bar station output) both
                        // legitimately land through this same event type, so
                        // only `FootprintOhlc` is blocked — footprint gets its
                        // bars from `FootprintUpdate` instead and must never
                        // have a live kline/derived `BarUpdate` interleave.
                        if bubble.bars_source == mlc_backend::bubble::BarsSource::FootprintOhlc {
                            return false;
                        }
                        // Defensive stamp: the first derived bar for a
                        // trade-agg-bar window should always find
                        // `bars_source` already `TradeAggBars` (stamped by
                        // `clear_slot_trade_agg_bars` at subscribe time) —
                        // if it still reads `Kline` here, stamp it now and
                        // drop any leftover kline bars before the append so
                        // this bubble never mixes two timelines.
                        if is_trade_agg_owner && bubble.bars_source == mlc_backend::bubble::BarsSource::Kline {
                            bubble.clear_bars();
                            bubble.bars_source = mlc_backend::bubble::BarsSource::TradeAggBars;
                        }
                        let last_ts_identity = bubble.bars.last().map(|b| {
                            if b.derived_ts_ms != 0 { b.derived_ts_ms } else { b.timestamp }
                        });
                        if last_ts_identity == bar_ts_identity {
                            if let Some(last) = bubble.last_bar_mut() {
                                last.open   = bar.open;
                                last.high   = bar.high;
                                last.low    = bar.low;
                                last.close  = bar.close;
                                last.volume = bar.volume;
                            }
                            false
                        } else {
                            bubble.append_bar(bar.clone());
                            true
                        }
                    });
                    if let Some(is_new) = is_new {
                        bu_is_new_bar = is_new;
                    }
                }

                let bu_bubble_bars: Vec<mlc_core::Bar> = target_bubbles_bu.iter().next()
                    .map(|b| self.core.bubbles().bars(*b).to_vec())
                    .unwrap_or_default();

                for (cid_bu, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                    let (win_sym, win_at, is_derived) = match instrument_by_cid_bu.get(cid_bu) {
                        Some(t) => t,
                        None => continue,
                    };
                    if !is_derived {
                        continue;
                    }
                    if win_sym.as_str() != bar_symbol || *win_at != bar_event_at {
                        continue;
                    }
                    if window.pending_symbol_load {
                        window.pending_symbol_load = false;
                    }
                    // These windows are gated `is_derived` (range/volume/tick/
                    // footprint/dollar/imbalance/run) — the station writes
                    // that series directly INTO bubble.bars, so it IS the
                    // kline array for this chart_type family (audit D8
                    // resolution table) — plain `Bars` wrap is correct here.
                    let series = crate::active_series::ActiveSeries::Bars(&bu_bubble_bars);
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
                    window.advance_view_on_tick(bu_is_new_bar);
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
                // Excludes tick-TF kline windows (their real live source IS raw
                // trades) AND trade-agg-bar windows (range/volume/tick/dollar/
                // imbalance/run bars) — the latter's `bubble.timeframe` stays an
                // ordinary wall-clock value (e.g. "15m", never re-stamped on
                // chart-type switch, see `chart_out_events.rs:690-694`), so an
                // `is_tick()`-only gate let raw trades build a SECOND, ordinary-
                // OHLC candle series into the SAME `bubble.bars` the station's
                // `BarUpdatePassthrough` writes for these windows (root cause
                // fixed 2026-07-08). `chart_type_is_trade_agg_bar` is the same
                // registry-driven check `BarUpdatePassthrough`'s bars-ownership
                // gate above uses — single source of truth.
                let derived_cids_tu: std::collections::HashSet<mlc_core::state::ChartId> = {
                    self.panel_app.panel_grid.containers().keys()
                        .filter(|cid| {
                            let is_tick_tf = self.timeframe_for_chart(**cid).map(|tf| tf.is_tick()).unwrap_or(false);
                            let is_trade_agg_bar = self.chart_type_for_chart(**cid)
                                .map(crate::active_series::chart_type_is_trade_agg_bar)
                                .unwrap_or(false);
                            is_tick_tf || is_trade_agg_bar
                        })
                        .copied()
                        .collect()
                };

                let mut bubble_periods: std::collections::HashMap<mlc_core::ids::BubbleId, i64> = std::collections::HashMap::new();
                for (cid_tu, _window) in self.panel_app.panel_grid.containers().iter() {
                    if derived_cids_tu.contains(cid_tu) { continue; }
                    let (win_sym_tu, win_tf_tu, _win_exch_tu, win_at_tu) = match instrument_by_cid_tu.get(cid_tu) {
                        Some(t) => t,
                        None => continue,
                    };
                    if win_sym_tu.as_str() != symbol || *win_at_tu != trade_event_at {
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
                        // FootprintUpdate (the tick-TF exclusion above never
                        // fires for footprint — its TF is a time bucket like
                        // "1m" — so this bars_source check is footprint's only
                        // protection in this arm). Trade-agg-bar bubbles are
                        // already excluded upstream via `derived_cids_tu`
                        // (`chart_type_is_trade_agg_bar`), but gating on "Kline
                        // owns" rather than naming FootprintOhlc alone makes
                        // this arm defend the invariant even if that upstream
                        // filter's set ever drifts from the ownership map.
                        if bubble.bars_source != mlc_backend::bubble::BarsSource::Kline {
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
                let wl_guard = self.core.watchlists();
                if let Some(wl) = wl_guard.find(self.watchlist_ui.active_list_id) {
                    for ws in wl.all_symbols() {
                        if ws.exchange == exchange_str {
                            let ws_at = mlc_backend_data_bridge::account_type_to_dig3(ws.account_type);
                            self.core.bridge_handle().subscribe_mini_ticker(*exchange_id, &ws.symbol, ws_at);
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
                    for (cid_err, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                        if error_instrument_map.contains_key(cid_err) {
                            window.data_error = Some(message.clone());
                        }
                    }
                    log::warn!(
                        "[data-error] {:?} {} at={} tf={}: {} (window_match={})",
                        exchange_id, symbol, account_type, timeframe, message, matched,
                    );
                } else {
                    // Connector/warmup-level error — no instrument to attach it
                    // to (empty symbol = venue-wide, e.g. connect_public
                    // failure). No window field to set; still surface to the log
                    // so it isn't silently dropped (audit D6).
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
                        if let Some((fp_len, bars_len)) = self.core.bubbles().with_bubble(*bid, |b| {
                            b.footprint_bars.as_ref().map(|fp| (fp.len(), b.bars.len()))
                        }).flatten() {
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
                match self.core.indicators().recalc_mode() {
                    RecalcMode::PerTick => {
                        let bars = self.core.bubbles().bars(*bid).to_vec();
                        self.core.indicators_mut().recalc_bubble(*bid, &bars);
                    }
                    RecalcMode::PerFrame | RecalcMode::PerBar => {
                        if let Some(sym) = self.core.bubbles().symbol(*bid) {
                            self.core.indicators_mut().mark_dirty(sym);
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
                            if !win.oi_show { return None; }
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

            BubbleEvent::FundingRatePassthrough { exchange_id: _, account_type, symbol, point } => {
                let event_account_type: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
                let target_cids: std::collections::HashSet<mlc_core::state::ChartId> =
                    self.panel_app.panel_grid.containers().iter()
                        .filter_map(|(cid, win)| {
                            if !win.funding_show { return None; }
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
            BubbleEvent::DomHeatmapSnapshotPassthrough { exchange_id, account_type, symbol, ts_ms, bids, asks } => {
                let at_label = account_type.as_str();
                let event_account_type: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
                let target_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                    self.panel_app.panel_grid.containers().keys()
                        .filter_map(|cid| {
                            let s = self.symbol_for_chart(*cid)?;
                            let a = self.account_type_for_chart(*cid)?;
                            let is_dom = self.chart_type_for_chart(*cid) == Some("dom_heatmap");
                            if !is_dom || s != *symbol || a != event_account_type { return None; }
                            Some(*cid)
                        }).collect()
                };
                let bubble_ids: std::collections::HashSet<mlc_core::ids::BubbleId> =
                    target_cids.iter().filter_map(|cid| self.bubble_for_chart(*cid)).collect();
                for b in &bubble_ids {
                    self.core.bubbles_mut().with_bubble_mut(*b, |bubble| {
                        const DEFAULT_TICK: f64 = 0.5;
                        bubble.dom_heatmap_mut()
                            .get_or_insert_with(|| mlc_core::chart::dom_heatmap::DomHeatmapBuffer::new(DEFAULT_TICK))
                            .push_snapshot(*ts_ms, bids, asks);
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

            BubbleEvent::DomTradeDotPassthrough { exchange_id: _, account_type, symbol, dot } => {
                let event_account_type: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
                let target_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                    self.panel_app.panel_grid.containers().keys()
                        .filter_map(|cid| {
                            let s = self.symbol_for_chart(*cid)?;
                            let a = self.account_type_for_chart(*cid)?;
                            let is_dom = self.chart_type_for_chart(*cid) == Some("dom_heatmap");
                            if !is_dom || s != *symbol || a != event_account_type { return None; }
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

            // ── Liquidation pass-throughs ──────────────────────────────────────
            BubbleEvent::LiquidationEventPassthrough { exchange_id, account_type, symbol, ts_ms, price, value_usd, side } => {
                let at_label = account_type.as_str();
                let event_account_type: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
                let target_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                    self.panel_app.panel_grid.containers().keys()
                        .filter_map(|cid| {
                            let s = self.symbol_for_chart(*cid)?;
                            let a = self.account_type_for_chart(*cid)?;
                            let is_liq = self.chart_type_for_chart(*cid) == Some("liquidation_heatmap");
                            if !is_liq || s != *symbol || a != event_account_type { return None; }
                            Some(*cid)
                        }).collect()
                };
                let bubble_ids: std::collections::HashSet<mlc_core::ids::BubbleId> =
                    target_cids.iter().filter_map(|cid| self.bubble_for_chart(*cid)).collect();
                for b in &bubble_ids {
                    self.core.bubbles_mut().with_bubble_mut(*b, |bubble| {
                        const DEFAULT_TICK: f64 = 0.5;
                        bubble.liq_heatmap_mut()
                            .get_or_insert_with(|| mlc_core::chart::liquidation_heatmap::LiquidationHeatmapBuffer::new(DEFAULT_TICK))
                            .push_liquidation(*ts_ms, *price, *value_usd, *side);
                        const MAX_DOTS: usize = 2_000;
                        let dots = bubble.liq_dots.get_or_insert_with(Vec::new);
                        if dots.len() >= MAX_DOTS { dots.remove(0); }
                        dots.push(mlc_core::chart::liquidation_heatmap::LiquidationDot {
                            ts_ms: *ts_ms, price: *price,
                            value_usd: if value_usd.is_finite() { *value_usd } else { 0.0 },
                            side: *side,
                        });
                    });
                }
                let liq_bubble_bars: Vec<mlc_core::Bar> = {
                    let mgr = self.core.bubbles();
                    mgr.bubble_for_instrument(exchange_id.as_str(), at_label, symbol)
                        .map(|b| mgr.bars(b).to_vec())
                        .unwrap_or_default()
                };
                for (cid_, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                    if !target_cids.contains(cid_) { continue; }
                    if window.pending_symbol_load { window.pending_symbol_load = false; }
                    if window.needs_snap_after_backfill && window.viewport().chart_width > 0.0 {
                        window.consume_cold_snap(crate::active_series::ActiveSeries::Bars(&liq_bubble_bars));
                    }
                }
            }

            BubbleEvent::LiquidationBucketPassthrough { exchange_id, account_type, symbol, ts_ms, long_liq_usd, short_liq_usd } => {
                let at_label = account_type.as_str();
                let event_account_type: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
                let target_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                    self.panel_app.panel_grid.containers().keys()
                        .filter_map(|cid| {
                            let s = self.symbol_for_chart(*cid)?;
                            let a = self.account_type_for_chart(*cid)?;
                            let is_liq = self.chart_type_for_chart(*cid) == Some("liquidation_heatmap");
                            if !is_liq || s != *symbol || a != event_account_type { return None; }
                            Some(*cid)
                        }).collect()
                };
                let ticker_key = format!("{}:{}:{}", symbol, exchange_id.as_str(), at_label);
                let mark_price_opt = self.core.mini_ticker.get(&ticker_key).map(|t| t.last_price);
                let bubble_ids: std::collections::HashSet<mlc_core::ids::BubbleId> =
                    target_cids.iter().filter_map(|cid| self.bubble_for_chart(*cid)).collect();
                for b in bubble_ids {
                    self.core.bubbles_mut().with_bubble_mut(b, |bubble| {
                        const DEFAULT_TICK: f64 = 0.5;
                        const SPREAD_HALF_TICKS: i64 = 5;
                        let mark_opt = mark_price_opt.or_else(|| bubble.bars.last().map(|b| b.close));
                        let buf = bubble.liq_heatmap_mut()
                            .get_or_insert_with(|| mlc_core::chart::liquidation_heatmap::LiquidationHeatmapBuffer::new(DEFAULT_TICK));
                        if let Some(mark) = mark_opt {
                            buf.push_bucket_spread(*ts_ms, *long_liq_usd, *short_liq_usd, mark, SPREAD_HALF_TICKS);
                        }
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
            // `derived_bar_sub_body` ring-peek only sends `LiveUpdate::
            // BarsLoaded` `if !points.is_empty()` (bridge.rs), so on a
            // zero-bucket seed that event NEVER fires and `pending_symbol_load`
            // is never cleared by the normal `BarsLoaded` fresh-load path —
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

            // ── Informational / no per-app reaction ────────────────────────────
            BubbleEvent::MiniTickerUpdated { .. }
            | BubbleEvent::SymbolsLoaded { .. }
            | BubbleEvent::OrderbookSnapshotRest { .. }
            | BubbleEvent::OrderbookDelta { .. }
            | BubbleEvent::OrderbookDeltaForMultiDom { .. }
            | BubbleEvent::TradingUpdate { .. } => {
                // Handled by the inline accumulator logic in tick() before/after
                // calling this method. No additional window-level work needed here.
            }
        }
    }
}
