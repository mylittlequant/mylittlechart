//! Per-app reactor: consume [`BubbleEvent`]s emitted by [`BackendCore::tick_writers`]
//! and run per-window side-effects (viewport snap, `pending_symbol_load` clear,
//! indicator recalc trigger, persistence flush).
//!
//! The write half (inserting data into the shared bubble store) has already
//! happened inside `tick_writers`. This module is the read/react half.

use crate::ChartApp;
use crate::chart_container::DEFAULT_SNAP_MARGIN;
use mlc_backend::BubbleEvent;
use mlc_core::ScaleMode;
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
                            window.capture_view_anchor(prev_bars_for_anchor)
                        } else {
                            None
                        };
                        matched_windows.push((*chart_id_key, is_backfill, anchor));
                    }
                }

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
                            let bs = window.viewport.bar_spacing.max(1.0);
                            let visible_f = window.viewport.chart_width / bs;
                            let prev_count = window.viewport.bar_count as f64;
                            let right_edge_vs = (prev_count + DEFAULT_SNAP_MARGIN - visible_f).max(0.0);
                            (window.viewport.view_start - right_edge_vs).abs() < 1.5
                                && window.viewport.chart_width > 0.0
                                && prev_count > 0.0
                        };
                        window.reconcile_view_keep_anchor(new_bars, anchor);
                        if window.needs_snap_after_backfill
                            && !new_bars.is_empty()
                            && window.viewport.chart_width > 0.0
                        {
                            window.consume_cold_snap(new_bars);
                        } else if was_at_end && !new_bars.is_empty() {
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
                        window.price_scale.scale_mode = mlc_core::chart::types::price_scale::ScaleMode::Auto;
                        window.reconcile_view_fresh(new_bars);
                        window.pending_symbol_load = false;
                        if window.needs_snap_after_backfill
                            && !new_bars.is_empty()
                            && window.viewport.chart_width > 0.0
                        {
                            window.consume_cold_snap(new_bars);
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
                    self.core.bridge.request_background_backfill(*exchange_id, &sym, &tf, at, bg_target);
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
                                if let Some(window) = parked.panel_grid.containers_mut().get_mut(&cid) {
                                    let is_backfill = !window.pending_symbol_load
                                        && window.viewport.bar_count > 0;
                                    if is_backfill {
                                        window.reconcile_view_keep_anchor(new_bars, None);
                                    } else {
                                        window.reconcile_view_fresh(new_bars);
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

                // Subscribe trades for matched windows.
                if any_matched {
                    if self.sidebar_state.connector_enabled.get(exchange_id.as_str()).copied().unwrap_or(true) {
                        let at = mlc_backend_data_bridge::account_type_to_dig3(event_account_type);
                        self.core.bridge.subscribe_trades(*exchange_id, symbol, at);
                    }
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
                        let bs = window.viewport.bar_spacing.max(1.0);
                        let visible_f = window.viewport.chart_width / bs;
                        let prev_count = window.viewport.bar_count as f64;
                        let right_edge_vs = (prev_count + DEFAULT_SNAP_MARGIN - visible_f).max(0.0);
                        (window.viewport.view_start - right_edge_vs).abs() < 1.5
                            && window.viewport.chart_width > 0.0
                            && prev_count > 0.0
                    };
                    matched_anchors.push((*chart_id_key, window.capture_view_anchor(prev_bars_for_anchor), was_at_end));
                }

                for (chart_id_key, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                    let Some((_, anchor, was_at_end)) = matched_anchors.iter().find(|(c, _, _)| c == chart_id_key).copied() else { continue; };
                    window.reconcile_view_keep_anchor(new_bars, anchor);
                    if window.needs_snap_after_backfill {
                        window.consume_cold_snap(new_bars);
                    } else if was_at_end && !new_bars.is_empty() {
                        // User was at right edge before backfill prepended
                        // older bars — re-snap so the live tick stays visible.
                        window.snap_to_end(DEFAULT_SNAP_MARGIN);
                    } else if window.price_scale.scale_mode.is_auto_y() {
                        window.calc_auto_scale(new_bars);
                    }
                    prev_close_mirrors.push((*chart_id_key, new_bars.first().map(|b| b.open)));
                }

                for (chart_id, prev_close) in prev_close_mirrors {
                    if let Some(b) = self.bubble_for_chart(chart_id) {
                        self.core.bubbles_mut().set_prev_close(b, prev_close);
                    }
                }

                let matched_ids: Vec<u64> = self.panel_app.panel_grid.containers().iter()
                    .filter(|(cid, _)| {
                        self.symbol_for_chart(**cid).map_or(false, |s| s == *symbol)
                            && self.exchange_for_chart(**cid).map_or(false, |e| e == exchange_id.as_str())
                            && self.timeframe_for_chart(**cid).map_or(false, |tf| tf.name == *tf_name)
                            && self.account_type_for_chart(**cid).map_or(false, |at| at == event_account_type)
                    })
                    .map(|(cid, _)| cid.0)
                    .collect();
                for wid in &matched_ids {
                    self.core.indicators_mut().recalc_bubble(mlc_core::ids::BubbleId(*wid), new_bars);
                }
            }

            // ── ScrollBarsAppended ─────────────────────────────────────────────
            BubbleEvent::ScrollBarsAppended { bid: _, exchange_id, symbol, account_type, timeframe: tf_name, bars, .. } => {
                let new_bars = bars;
                let at_label = account_type.as_str();
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

                let prev_bars_for_anchor: Vec<mlc_core::Bar> = {
                    let mgr = self.core.bubbles();
                    mgr.bubble_for_instrument(exchange_id.as_str(), at_label, symbol)
                        .map(|b| mgr.bars(b).to_vec())
                        .unwrap_or_default()
                };

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
                    matched_anchors.push((*chart_id_key, window.capture_view_anchor(&prev_bars_for_anchor)));
                }

                for (chart_id_key, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                    let Some((_, anchor)) = matched_anchors.iter().find(|(c, _)| c == chart_id_key).copied() else { continue; };
                    window.scroll_fetch_in_flight = false;
                    window.scroll_fetch_started = None;
                    window.reconcile_view_keep_anchor(new_bars, anchor);
                    if window.price_scale.scale_mode.is_auto_y() {
                        window.calc_auto_scale(new_bars);
                    }
                    prev_close_mirrors.push((*chart_id_key, new_bars.first().map(|b| b.open)));
                }

                for (chart_id, prev_close) in prev_close_mirrors {
                    if let Some(b) = self.bubble_for_chart(chart_id) {
                        self.core.bubbles_mut().set_prev_close(b, prev_close);
                    }
                }

                if any_matched {
                    let matched_ids: Vec<u64> = self.panel_app.panel_grid.containers().iter()
                        .filter(|(cid, _)| {
                            self.symbol_for_chart(**cid).map_or(false, |s| s == *symbol)
                                && self.exchange_for_chart(**cid).map_or(false, |e| e == exchange_id.as_str())
                                && self.timeframe_for_chart(**cid).map_or(false, |tf| tf.name == *tf_name)
                        })
                        .map(|(cid, _)| cid.0)
                        .collect();
                    for wid in &matched_ids {
                        self.core.indicators_mut().recalc_bubble(mlc_core::ids::BubbleId(*wid), new_bars);
                    }
                }
            }

            // ── BarUpdatePassthrough ────────────────────────────────────────────
            BubbleEvent::BarUpdatePassthrough { exchange_id: _bar_exch, account_type: bar_account_type, symbol: bar_symbol, bar } => {
                let bar_event_at: mlc_core::instrument::AccountType = bar_account_type.parse().unwrap_or_default();

                let instrument_by_cid_bu: std::collections::HashMap<mlc_core::state::ChartId, (String, mlc_core::instrument::AccountType, bool)> = {
                    let panels_store = &self.panels_store;
                    self.panel_app.panel_grid.containers().iter()
                        .filter_map(|(cid, win)| {
                            let sym = self.symbol_for_chart(*cid)?;
                            let at = self.account_type_for_chart(*cid)?;
                            let is_tick_tf = self.timeframe_for_chart(*cid).map(|tf| tf.is_tick()).unwrap_or(false);
                            let derived_chart_type = panels_store.chart_panel
                                .get(&win.chart_panel_id)
                                .map(|p| matches!(p.chart_type,
                                    "range_bar" | "volume_bar" | "tick_bar" | "footprint"
                                    | "dollar_bar" | "tick_imbalance_bar"
                                    | "volume_imbalance_bar" | "run_bar"
                                ))
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

                for bid_bu in &target_bubbles_bu {
                    let is_new = self.core.bubbles_mut().with_bubble_mut(*bid_bu, |bubble| {
                        let last_ts = bubble.bars.last().map(|b| b.timestamp);
                        if last_ts == Some(bar.timestamp) {
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
                    if window.needs_snap_after_backfill && window.viewport.chart_width > 0.0 {
                        window.consume_cold_snap(&bu_bubble_bars);
                    }
                    window.viewport.bar_count = bu_bubble_bars.len();
                    if window.price_scale.scale_mode.is_auto_y() {
                        let _as_start = web_time::Instant::now();
                        window.calc_auto_scale(&bu_bubble_bars);
                        self.last_auto_scale_us += _as_start.elapsed().as_micros() as u64;
                    }
                    let count = bu_bubble_bars.len();
                    let visible_f = window.viewport.chart_width / window.viewport.bar_spacing;
                    if window.price_scale.scale_mode.is_follow() {
                        window.snap_to_end(DEFAULT_SNAP_MARGIN);
                    }
                    if bu_is_new_bar && window.price_scale.scale_mode == ScaleMode::Auto {
                        let right_edge_bar = window.viewport.view_start + visible_f;
                        let last_bar = count as f64;
                        if last_bar >= right_edge_bar {
                            window.viewport.view_start += 1.0;
                        }
                    }
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
                let derived_cids_tu: std::collections::HashSet<mlc_core::state::ChartId> = {
                    self.panel_app.panel_grid.containers().keys()
                        .filter(|cid| {
                            self.timeframe_for_chart(**cid).map(|tf| tf.is_tick()).unwrap_or(false)
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
                        if let Some(last_ts) = bubble.bars.last().map(|b| b.timestamp) {
                            let candle_end = last_ts + period_secs;
                            if trade_ts_secs >= candle_end {
                                let needs_bf = trade_ts_secs >= candle_end + period_secs;
                                let new_candle_start = (trade_ts_secs / period_secs) * period_secs;
                                bubble.append_bar(mlc_core::Bar {
                                    timestamp: new_candle_start,
                                    open: *price, high: *price, low: *price, close: *price, volume: *quantity,
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
                            });
                            (true, false)
                        }
                    });
                    if let Some((new_bar, needs_bf)) = outcome {
                        if new_bar { is_new_bar = true; }
                        if needs_bf { needs_backfill = true; }
                    }
                }

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
                    window.viewport.bar_count = window.viewport.bar_count.saturating_add(if is_new_bar { 1 } else { 0 });
                    let count = window.viewport.bar_count;
                    let visible_f = window.viewport.chart_width / window.viewport.bar_spacing;
                    if window.price_scale.scale_mode.is_follow() {
                        window.snap_to_end(DEFAULT_SNAP_MARGIN);
                    }
                    if is_new_bar && window.price_scale.scale_mode == ScaleMode::Auto {
                        let right_edge_bar = window.viewport.view_start + visible_f;
                        let last_bar = count as f64;
                        if last_bar >= right_edge_bar {
                            window.viewport.view_start += 1.0;
                        }
                    }
                }

                self.last_bar_apply_us += _bar_apply_start.elapsed().as_micros() as u64;

                if needs_backfill {
                    let bridge = self.core.bridge.clone();
                    for cid in self.panel_app.panel_grid.containers().keys() {
                        let sym_match = self.symbol_for_chart(*cid).map_or(false, |s| s == *symbol);
                        let win_at = self.account_type_for_chart(*cid);
                        let at_match = win_at.map_or(false, |at| at == trade_event_at);
                        if sym_match && at_match {
                            let at_val = mlc_backend_data_bridge::account_type_to_dig3(win_at.unwrap_or_default());
                            if let Some(tf) = self.timeframe_for_chart(*cid) {
                                bridge.request_bars(*exchange_id, symbol, &tf, at_val, None, None, false);
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
                            let h = self.core.bridge.subscribe_trades(eid, &state.symbol, at);
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
                let bridge = self.core.bridge.clone();
                let exchange_str = eid_str;
                let wl_guard = self.core.watchlists();
                if let Some(wl) = wl_guard.find(self.watchlist_ui.active_list_id) {
                    for ws in wl.all_symbols() {
                        if ws.exchange == exchange_str {
                            let ws_at = mlc_backend_data_bridge::account_type_to_dig3(ws.account_type);
                            bridge.subscribe_mini_ticker(*exchange_id, &ws.symbol, ws_at);
                        }
                    }
                }
                drop(wl_guard);
                for (cid, _) in self.panel_app.panel_grid.containers().iter() {
                    let sym = self.symbol_for_chart(*cid).unwrap_or_default();
                    if sym.is_empty() { continue; }
                    let exch = self.exchange_for_chart(*cid).unwrap_or_default();
                    let win_eid = digdigdig3::ExchangeId::from_str(&exch)
                        .unwrap_or(digdigdig3::ExchangeId::Binance);
                    if win_eid == *exchange_id {
                        let at: mlc_core::instrument::AccountType = self.account_type_for_chart(*cid).unwrap_or_default();
                        let at = mlc_backend_data_bridge::account_type_to_dig3(at);
                        if let Some(tf) = self.timeframe_for_chart(*cid) {
                            bridge.request_bars(*exchange_id, &sym, &tf, at, None, None, false);
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
                    for (cid_err, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                        if error_instrument_map.contains_key(cid_err) {
                            window.data_error = Some(message.clone());
                        }
                    }
                }
            }

            // ── Derived chart types: snap + pending_symbol_load clear ──────────
            BubbleEvent::FootprintBarAppended { bid, exchange_id, symbol, account_type }
            | BubbleEvent::RenkoBarAppended { bid, exchange_id, symbol, account_type }
            | BubbleEvent::PnfBarAppended { bid, exchange_id, symbol, account_type }
            | BubbleEvent::KagiBarAppended { bid, exchange_id, symbol, account_type }
            | BubbleEvent::CvdSampleAppended { bid, exchange_id, symbol, account_type }
            | BubbleEvent::TpoSessionUpdated { bid, exchange_id, symbol, account_type }
            | BubbleEvent::TlbLineAppended { bid, exchange_id, symbol, account_type } => {
                let bubble_bars: Vec<mlc_core::Bar> = self.with_bubble(*bid, |b| b.bars.clone()).unwrap_or_default();
                let at_label = account_type.as_str();
                for (_cid_, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                    if window.current_tag_id != *bid { continue; }
                    let _ = (exchange_id, symbol, at_label);
                    if window.pending_symbol_load { window.pending_symbol_load = false; }
                    if window.needs_snap_after_backfill && window.viewport.chart_width > 0.0 {
                        window.consume_cold_snap(&bubble_bars);
                    }
                    window.viewport.bar_count = bubble_bars.len();
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
                    let panels_store = &self.panels_store;
                    self.panel_app.panel_grid.containers().iter()
                        .filter_map(|(cid, win)| {
                            let s = self.symbol_for_chart(*cid)?;
                            let a = self.account_type_for_chart(*cid)?;
                            let is_dom = panels_store.chart_panel.get(&win.chart_panel_id)
                                .map(|p| p.chart_type == "dom_heatmap")
                                .unwrap_or(false);
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
                    if window.needs_snap_after_backfill && window.viewport.chart_width > 0.0 {
                        window.consume_cold_snap(&dom_bubble_bars);
                    }
                }
            }

            BubbleEvent::DomTradeDotPassthrough { exchange_id: _, account_type, symbol, dot } => {
                let event_account_type: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
                let target_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                    let panels_store = &self.panels_store;
                    self.panel_app.panel_grid.containers().iter()
                        .filter_map(|(cid, win)| {
                            let s = self.symbol_for_chart(*cid)?;
                            let a = self.account_type_for_chart(*cid)?;
                            let is_dom = panels_store.chart_panel.get(&win.chart_panel_id)
                                .map(|p| p.chart_type == "dom_heatmap")
                                .unwrap_or(false);
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
                    let panels_store = &self.panels_store;
                    self.panel_app.panel_grid.containers().iter()
                        .filter_map(|(cid, win)| {
                            let s = self.symbol_for_chart(*cid)?;
                            let a = self.account_type_for_chart(*cid)?;
                            let is_liq = panels_store.chart_panel.get(&win.chart_panel_id)
                                .map(|p| p.chart_type == "liquidation_heatmap")
                                .unwrap_or(false);
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
                    if window.needs_snap_after_backfill && window.viewport.chart_width > 0.0 {
                        window.consume_cold_snap(&liq_bubble_bars);
                    }
                }
            }

            BubbleEvent::LiquidationBucketPassthrough { exchange_id, account_type, symbol, ts_ms, long_liq_usd, short_liq_usd } => {
                let at_label = account_type.as_str();
                let event_account_type: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
                let target_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                    let panels_store = &self.panels_store;
                    self.panel_app.panel_grid.containers().iter()
                        .filter_map(|(cid, win)| {
                            let s = self.symbol_for_chart(*cid)?;
                            let a = self.account_type_for_chart(*cid)?;
                            let is_liq = panels_store.chart_panel.get(&win.chart_panel_id)
                                .map(|p| p.chart_type == "liquidation_heatmap")
                                .unwrap_or(false);
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
