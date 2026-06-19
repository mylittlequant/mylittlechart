//! Live update tick loop: drains broadcast channels (bars, trades, mini-ticker,
//! connector status), processes alert checks, and updates per-frame state.

use crate::ChartApp;
use crate::{account_type_from_label, parse_timeframe_name};
use mlc_live_data::LiveUpdate;
use mlc_core::{ChartId, ScaleMode, timestamp_ms_to_bar_f64};
use mlc_indicators::RecalcMode;

impl ChartApp {
    /// Called every frame with the current wall-clock time in milliseconds.
    ///
    /// Drains the async `LiveUpdate` channel (bar loads, WebSocket bar updates,
    /// connector-ready events) and runs the alert crossing checker.
    pub fn tick(&mut self, current_time_ms: u64) {
        let _ = current_time_ms;
        let tick_start = std::time::Instant::now();

        // Connector warm-up: dial all enabled connectors once (bridge-guarded).
        // Fires on the first tick of the skeleton/loading window (concurrent, off
        // the UI thread); the bridge's one-shot flag makes every later call —
        // including the promoted live window's — a no-op. Cheap to call every tick.
        self.warmup_connectors();

        // Load chat history for any restored chat leaves on the very first tick.
        // PTY sessions spawn-on-demand only (user must click [Start]).
        if !self.agent_autostarted {
            self.agent_autostarted = true;
            // Load chat history per leaf — use the persisted session_id when available
            // so each leaf resumes its own conversation, not just the latest one.
            let chat_leaves: Vec<(gate4agent::InstanceId, Option<String>)> = self.sidebar_state.agent_leaves
                .values()
                .filter(|d| d.mode == gate4agent::InstanceMode::Chat)
                .map(|d| (d.instance_id, d.chat_session_id.clone()))
                .collect();
            for (id, session_id) in chat_leaves {
                if let Some(ref sid) = session_id {
                    self.agent.load_history_instance(id, sid);
                } else {
                    self.agent.load_latest_history_instance(id);
                }
            }
        }

        // Reset per-tick accumulators for profiling.
        self.last_auto_scale_us = 0;
        self.last_indicator_recalc_us = 0;
        self.last_orderbook_panel_us = 0;
        self.last_dom_panel_us = 0;
        self.last_l2_panel_us = 0;
        self.last_heatmap_panel_us = 0;
        self.last_trade_panel_us = 0;
        self.last_bar_apply_us = 0;
        self.last_ob_event_count = 0;
        self.last_trade_event_count = 0;

        // ── Live data: drain the async update channel ─────────────────────
        // The channel is a broadcast — handle Lagged by continuing to drain.
        // Track whether at least one trade arrived this tick so the alert
        // crossing checker can be skipped on quiet (no-trade) frames.
        let mut had_trade_update = false;
        let mut _drain_count = 0u32;
        // Per-tick drain census by event type — printed only when this tick
        // explodes (see the spike log at the end of tick), so we can see WHAT
        // flooded the queue this frame (trade burst? backfill? reconnect?).
        let (mut n_bars, mut n_trade, mut n_ticker, mut n_connector, mut n_scroll, mut n_backfill, mut n_other) =
            (0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
        // Startup-freeze diagnostics: count SymbolsLoaded events this tick, total
        // symbols moved, and microseconds spent in the exchange_symbols.insert
        // path. The startup symbol flood (thousands of symbols × N exchanges) is
        // a freeze suspect — this attributes it precisely.
        let (mut n_symbols, mut n_symbols_total, mut symbols_insert_us) = (0u32, 0usize, 0u64);
        let mut trading_updates: Vec<LiveUpdate> = Vec::new();
        // Coalesce orderbook panel updates: collect per-(symbol,exchange,account_type)
        // delta events for L2-tape (which needs per-event timestamps) and force_rebuild
        // flags for DOM (which reads the tracker for full rebuilds).
        //
        // Phase 2B step 3: the bridge station forwarder writes directly into the
        // per-key `OrderBookTracker` BEFORE emitting the broadcast. By the time this
        // drain runs, the tracker is already up-to-date. DOM reads the tracker via
        // `apply_ob_update`; heatmap reads via its own `tick()`. Only L2-tape
        // continues to receive raw delta events from the broadcast (it needs per-event
        // wire timestamps for tape row rendering).
        //
        // Event routing:
        //   OrderbookSnapshot{source:Rest}  → force_rebuild = true for DOM
        //   OrderbookSnapshot{source:Ws}    → SKIP
        //   OrderbookDelta{bids, asks}       → push ObDeltaEvent preserving per-event ts
        /// One WS delta message with its original wire timestamp.
        struct ObDeltaEvent {
            ts_ms: i64,
            bids: Vec<(f64, f64)>,
            asks: Vec<(f64, f64)>,
        }
        struct ObDeltaEntry {
            /// Per-event raw deltas in arrival order. Each entry preserves its own ts
            /// so L2-tape rows get per-event ts instead of one SystemTime::now() per
            /// render-frame batch.
            events: Vec<ObDeltaEvent>,
            /// True when a REST snapshot arrived this frame → DOM must full-rebuild
            /// from the tracker.
            force_rebuild: bool,
        }
        let mut ob_dirty: std::collections::HashMap<(String, String, String), ObDeltaEntry> =
            std::collections::HashMap::new();
        // Coalesce last-trade-price → orderbook ghost-filter updates.  Calling
        // `set_last_trade_price` per trade is O(book): it sweeps ghost levels
        // (~1000) AND rebuilds the published view (full ~1000×2 clone) every time.
        // On a burst (172 trades/frame) that was the dominant ~21ms tick spike.
        // Only the LAST price per symbol matters for the ghost filter, so record
        // it here and apply once after the drain loop.
        let mut last_trade_price_by_symbol: std::collections::HashMap<(String, String, String), f64> =
            std::collections::HashMap::new();
        // Listened-instrument set: every (symbol, exchange, tf, account_type)
        // that SOME live container or parked preset cares about. Built once per
        // tick (cheap N-window walk) so the BarsLoaded arm can short-circuit a
        // fully-unmatched event BEFORE the expensive matching path (per-event
        // container filtering + bubble lookups + bar_cache.write lock).
        //
        // Why this matters: warm_background_tabs fires ~11 series during the
        // promote worker phase; their BarsLoaded (plus any other-symbol feed)
        // pile up in the broadcast channel and, on the first live tick, run
        // the full matching only to resolve to no-match — measured as the bulk
        // of the ~228ms first-frame event drain. A key not in this set means
        // neither a live window nor a parked preset wants the bars and
        // data_provider.insert_bars is gated on any_matched anyway, so the
        // whole arm is pure waste — skip it.
        let listened_instruments: std::collections::HashSet<(String, String, String, String)> = {
            let mut set = std::collections::HashSet::new();
            for cid in self.panel_app.panel_grid.containers().keys() {
                if let (Some(s), Some(e), Some(tf), Some(at)) = (
                    self.symbol_for_chart(*cid),
                    self.exchange_for_chart(*cid),
                    self.timeframe_for_chart(*cid),
                    self.account_type_for_chart(*cid),
                ) {
                    set.insert((s.to_string(), e.to_string(), tf.name.clone(), at.to_string()));
                }
            }
            for parked in self.live_preset_cache.values() {
                for cid in parked.panel_grid.containers().keys() {
                    if let (Some(s), Some(e), Some(tf), Some(at)) = (
                        parked.tag_manager.symbol_for_chart(*cid),
                        parked.tag_manager.exchange_for_chart(*cid),
                        parked.tag_manager.timeframe_for_chart(*cid),
                        parked.tag_manager.account_type_for_chart(*cid),
                    ) {
                        set.insert((s.to_string(), e.to_string(), tf.name.clone(), at.to_string()));
                    }
                }
            }
            set
        };
        let events_start = std::time::Instant::now();
        // Drain budget: cap the per-tick event processing so a backlog never
        // blocks a single frame. The worst case is the FIRST live tick right
        // after the skeleton→live promote: the broadcast channel accumulated
        // ~2.4s of bars/trades/symbols while the worker built the ChartApp,
        // and draining the whole backlog in one tick measured ~1.07s on the
        // main thread — the "spinner gone, chart not yet" freeze. Stop after
        // the budget and leave the rest for the next about_to_wait pass (which
        // fires immediately); the post-loop coalesced work (ghost filter,
        // trading updates) is idempotent across partial drains.
        const DRAIN_BUDGET_MS: u128 = 8;
        loop {
            // Check budget before pulling the next event (not after applying it,
            // so a single heavy event can still complete). Always process at
            // least one event per tick to guarantee forward progress.
            if _drain_count > 0 && events_start.elapsed().as_millis() >= DRAIN_BUDGET_MS {
                break;
            }
            let update = match self.live_update_rx.try_recv() {
                Ok(u) => { _drain_count += 1; u },
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                    self.lag_event_count += 1;
                    crate::dlog!("[ChartApp:{}] broadcast LAGGED — skipped {} messages (total lag events: {})",
                        self.core.bubble().bubble.bubbles()
                            .find(|b| !b.symbol.is_empty())
                            .map(|b| b.symbol.clone())
                            .unwrap_or_else(|| "?".to_string()),
                        n, self.lag_event_count);
                    // Always trigger backfill — the receiver has jumped forward
                    // and missed Trade updates are permanently lost.
                    if self.last_backfill_time.elapsed() > std::time::Duration::from_millis(500) {
                        self.last_backfill_time = std::time::Instant::now();
                        self.trigger_backfill_after_lag();
                    }
                    continue;
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                    eprintln!("[ChartApp] broadcast receiver CLOSED — no more updates possible!");
                    break;
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
            };
            // Only mark sidebar dirty for panels that display live trade data.
            // Performance uses its own 1-second timer; Alerts, ObjectTree, and
            // Signals are not affected by individual price ticks.
            //
            // Rate-limited: a burst of live events (startup connector flood, or
            // a busy tape) would otherwise re-mark the sidebar dirty on every
            // single event, forcing a full ~30ms rebuild every frame. We mark at
            // most once per 100ms here; the per-second prepare-timer guarantees
            // the panel still refreshes if no mark slips through. Input clicks
            // set sidebar_data_dirty directly elsewhere and are not gated.
            {
                use mlc_sidebar_content::state::RightSidebarPanel;
                if matches!(
                    self.sidebar_state.right_panel,
                    RightSidebarPanel::Watchlist | RightSidebarPanel::Connectors
                ) {
                    let due = self
                        .last_sidebar_data_mark
                        .map(|t| t.elapsed() >= std::time::Duration::from_millis(100))
                        .unwrap_or(true);
                    if due {
                        self.sidebar_data_dirty = true;
                        self.last_sidebar_data_mark = Some(std::time::Instant::now());
                    }
                }
            }
            // Census this drained event by type (for the spike log).
            match &update {
                LiveUpdate::BarsLoaded { .. } => n_bars += 1,
                LiveUpdate::BackfillComplete { .. } => n_backfill += 1,
                LiveUpdate::ScrollBarsLoaded { .. } => n_scroll += 1,
                LiveUpdate::TradeUpdate { .. } | LiveUpdate::BarUpdate { .. } => n_trade += 1,
                LiveUpdate::MiniTickerUpdate { .. } => n_ticker += 1,
                LiveUpdate::ConnectorReady { .. } => n_connector += 1,
                LiveUpdate::SymbolsLoaded { symbols, .. } => {
                    n_symbols += 1;
                    n_symbols_total += symbols.len();
                }
                _ => n_other += 1,
            }
            match update {
                LiveUpdate::BarsLoaded { exchange_id, symbol, timeframe: tf_name, bars, account_type } => {
                    eprintln!(
                        "[tick::BarsLoaded recv] {:?} {} at={:?} tf={} bars={} first_ts={} last_ts={}",
                        exchange_id, symbol, account_type, tf_name, bars.len(),
                        bars.first().map(|b| b.timestamp).unwrap_or(0),
                        bars.last().map(|b| b.timestamp).unwrap_or(0),
                    );
                    // Short-circuit fully-unmatched bars before the expensive
                    // matching path. If no live window or parked preset listens
                    // to this instrument, the entire arm resolves to no-match
                    // (insert_bars is any_matched-gated), so the series_handle
                    // setup, container filtering, and bar_cache.write lock are
                    // all wasted. Skip straight away.
                    let listen_key = (
                        symbol.clone(),
                        exchange_id.as_str().to_string(),
                        tf_name.clone(),
                        account_type.short_label().to_string(),
                    );
                    if !listened_instruments.contains(&listen_key) {
                        eprintln!("[tick::BarsLoaded] no-listener, skipping");
                        continue;
                    }
                    let _loaded_tf = parse_timeframe_name(&tf_name);
                    crate::dlog!("[ChartApp] BarsLoaded: {:?} {} tf={} bars={} first_ts={} last_ts={}",
                        exchange_id, symbol, tf_name, bars.len(),
                        bars.first().map(|b| b.timestamp).unwrap_or(0),
                        bars.last().map(|b| b.timestamp).unwrap_or(0));

                    let mut any_matched = false;
                    // Collect (symbol, timeframe, account_type) for windows that received
                    // an initial load so we can trigger background backfill after the loop.
                    let mut backfill_requests: Vec<(String, mlc_core::state::Timeframe, digdigdig3::AccountType)> = Vec::new();
                    // Collect (chart_id, prev_close_price) for matched windows so we can
                    // write to the bubble after the panel_grid mut borrow drops.
                    let mut prev_close_mirrors: Vec<(mlc_core::state::ChartId, Option<f64>)> = Vec::new();

                    // Pre-collect instrument map from BubbleStore so symbol/tf/exchange/
                    // account_type are available inside the containers_mut() loop without
                    // an extra borrow.
                    let instrument_by_cid: std::collections::HashMap<mlc_core::state::ChartId, (String, mlc_core::state::Timeframe, String, String)> = {
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

                    // Phase X0 / C5c: capture per-window anchor BEFORE bubble write,
                    // determine is_backfill (was the window already showing data?).
                    // Then write bubble.bars ONCE, then loop windows for reconcile.
                    let mut matched_windows: Vec<(mlc_core::state::ChartId, bool, Option<(i64, f64)>)> = Vec::new();
                    let prev_bars_for_anchor: Vec<mlc_core::Bar> = {
                        let m = self.core.bubble();
                        m.bubble.bubble_for_instrument(exchange_id.as_str(), account_type.short_label(), &symbol)
                            .and_then(|bid| m.bubble.bubble(bid))
                            .map(|b| b.bars.clone())
                            .unwrap_or_default()
                    };
                    for (chart_id_key, window) in self.panel_app.panel_grid.containers().iter() {
                        let (win_sym, win_tf, win_exch, win_at) = match instrument_by_cid.get(chart_id_key) {
                            Some(t) => t,
                            None => continue,
                        };
                        let tf_matches = win_tf.name == tf_name;
                        let matched = win_sym.as_str() == symbol
                            && win_exch.as_str() == exchange_id.as_str()
                            && tf_matches
                            && win_at.as_str() == account_type.short_label();
                        if matched {
                            any_matched = true;
                            let is_backfill = !window.pending_symbol_load && !prev_bars_for_anchor.is_empty();
                            let anchor = if is_backfill {
                                window.capture_view_anchor(&prev_bars_for_anchor)
                            } else {
                                None
                            };
                            matched_windows.push((*chart_id_key, is_backfill, anchor));
                        }
                    }

                    // Write bubble.bars into EVERY bubble that ANY matched chart is
                    // attached to (resolved via `bubble_for_chart`). The renderer
                    // reads via `window.current_bubble_id`, so this MUST be the
                    // source of truth for routing — `bubble_for_instrument` can
                    // return a DIFFERENT bubble (e.g. an instrument-tagged search
                    // hit) than the chart's `current_bubble_id`, which would write
                    // into a phantom bubble while the live chart's bubble stays
                    // empty (the renderer would then see 0 bars and spinner-loop).
                    if any_matched {
                        let target_bubbles: std::collections::HashSet<mlc_core::tag_manager::BubbleId> =
                            matched_windows.iter()
                                .filter_map(|(cid, _, _)| self.bubble_for_chart(*cid))
                                .collect();
                        for bid in target_bubbles {
                            if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(bid) {
                                bubble.bars = bars.clone();
                            }
                        }
                    }

                    // Window-side reconcile loop — viewport snap, anchor restore, post-effects.
                    let new_bars_clone = bars.clone();
                    for (chart_id_key, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                        let Some((_, is_backfill, anchor)) = matched_windows.iter().find(|(cid, _, _)| cid == chart_id_key).cloned()
                            .map(|t| Some(t)).unwrap_or(None) else { continue; };
                        let win_at = instrument_by_cid.get(chart_id_key).map(|(_, _, _, at)| at.clone()).unwrap_or_default();
                        let win_sym = instrument_by_cid.get(chart_id_key).map(|(s, _, _, _)| s.clone()).unwrap_or_default();
                        let win_tf = instrument_by_cid.get(chart_id_key).map(|(_, tf, _, _)| tf.clone()).unwrap_or_else(mlc_core::state::Timeframe::h1);
                        if is_backfill {
                            window.reconcile_view_keep_anchor(&new_bars_clone, anchor);
                            if window.needs_snap_after_backfill
                                && !new_bars_clone.is_empty()
                                && window.viewport.chart_width > 0.0
                            {
                                window.consume_cold_snap(&new_bars_clone);
                            }
                            let target = self.panel_app.user_manager.profile.data_load.background_bar_count;
                            if target > 0 && (new_bars_clone.len() as u32) < target {
                                backfill_requests.push((
                                    win_sym,
                                    win_tf,
                                    account_type_from_label(&win_at),
                                ));
                            }
                        } else {
                            window.price_scale.scale_mode = self.default_scale_mode;
                            window.reconcile_view_fresh(&new_bars_clone);
                            window.pending_symbol_load = false;
                            if window.needs_snap_after_backfill
                                && !new_bars_clone.is_empty()
                                && window.viewport.chart_width > 0.0
                            {
                                window.consume_cold_snap(&new_bars_clone);
                            }
                            let target = self.panel_app.user_manager.profile.data_load.background_bar_count;
                            if target > 300 {
                                backfill_requests.push((
                                    win_sym,
                                    win_tf,
                                    account_type_from_label(&win_at),
                                ));
                            }
                        }
                        prev_close_mirrors.push((*chart_id_key, new_bars_clone.first().map(|b| b.open)));
                    }
                    for (chart_id, prev_close) in prev_close_mirrors {
                        if let Some(bid) = self.bubble_for_chart(chart_id) {
                            if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(bid) {
                                bubble.prev_close_price = prev_close;
                            }
                        }
                    }

                    // C5c: bubble write already done above (no second write needed).

                    // Trigger Layer 2 background backfill for initial loads.
                    // Done after the window loop to avoid borrow conflicts with self.core.bridge.
                    let bg_target = self.panel_app.user_manager.profile.data_load.background_bar_count;
                    for (sym, tf, at) in backfill_requests {
                        crate::dlog!("[ChartApp] Scheduling background backfill: {} {} tf={} target={}", exchange_id.as_str(), sym, tf.name, bg_target);
                        self.core.bridge.request_background_backfill(exchange_id, &sym, &tf, at, bg_target);
                    }

                    // Before diagnosing a true no-match, try to deliver bars to
                    // any parked (tab-switched-away) preset whose windows match
                    // this event.  Parked presets have their own panel_grid and
                    // tag_manager stored in live_preset_cache.  Bars delivered
                    // here are held in-memory so that unparking the preset
                    // renders immediately without re-fetching.
                    let mut parked_matched = false;
                    {
                        // Collect matching (preset_key, Vec<ChartId>) pairs first
                        // to avoid borrowing live_preset_cache and its inner fields
                        // simultaneously with the mutable containers pass.
                        let matching_parked: Vec<(String, Vec<mlc_core::state::ChartId>)> =
                            self.live_preset_cache
                                .iter()
                                .map(|(preset_key, parked)| {
                                    let cids: Vec<mlc_core::state::ChartId> = parked
                                        .panel_grid
                                        .containers()
                                        .keys()
                                        .filter(|cid| {
                                            parked
                                                .tag_manager
                                                .symbol_for_chart(**cid)
                                                .map_or(false, |s| s == symbol)
                                                && parked
                                                    .tag_manager
                                                    .exchange_for_chart(**cid)
                                                    .map_or(false, |e| e == exchange_id.as_str())
                                                && parked
                                                    .tag_manager
                                                    .timeframe_for_chart(**cid)
                                                    .map_or(false, |tf| tf.name == tf_name)
                                                && parked
                                                    .tag_manager
                                                    .account_type_for_chart(**cid)
                                                    .map_or(false, |at| at == account_type.short_label())
                                        })
                                        .copied()
                                        .collect();
                                    (preset_key.clone(), cids)
                                })
                                .filter(|(_, cids)| !cids.is_empty())
                                .collect();

                        for (preset_key, cids) in matching_parked {
                            if let Some(parked) = self.live_preset_cache.get_mut(&preset_key) {
                                // Phase X0 / C5c: write bubble.bars for each parked window's bubble FIRST.
                                let parked_bubble_ids: Vec<mlc_core::tag_manager::BubbleId> = cids.iter()
                                    .filter_map(|cid| parked.tag_manager.bubble_for_chart(*cid))
                                    .collect::<std::collections::HashSet<_>>()
                                    .into_iter()
                                    .collect();
                                for bid in &parked_bubble_ids {
                                    if let Some(bubble) = parked.tag_manager.bubble_mut(*bid) {
                                        bubble.bars = bars.clone();
                                    }
                                }
                                for cid in cids {
                                    if let Some(window) = parked.panel_grid.containers_mut().get_mut(&cid) {
                                        let is_backfill = !window.pending_symbol_load
                                            && window.viewport.bar_count > 0;
                                        if is_backfill {
                                            window.reconcile_view_keep_anchor(&bars, None);
                                        } else {
                                            window.reconcile_view_fresh(&bars);
                                            window.pending_symbol_load = false;
                                        }
                                        parked_matched = true;
                                        crate::dlog!("[BarsLoaded] delivered to parked preset '{}' cid={:?} is_backfill={}", preset_key, cid, is_backfill);
                                    }
                                }
                            }
                        }
                    }

                    // No-match diagnostic: log only when neither live nor parked windows matched.
                    if !any_matched && !parked_matched {
                        let window_keys: Vec<String> = self.panel_app.panel_grid.containers().keys()
                            .map(|cid| {
                                let sym = self.symbol_for_chart(*cid).unwrap_or_else(|| "?".to_string());
                                let exch = self.exchange_for_chart(*cid).unwrap_or_else(|| "?".to_string());
                                let at = self.account_type_for_chart(*cid).unwrap_or_else(|| "?".to_string());
                                let tf = self.timeframe_for_chart(*cid)
                                    .map(|t| t.name.clone()).unwrap_or_else(|| "?".to_string());
                                format!("{}@{}:{}/{}", sym, exch, at, tf)
                            })
                            .collect();
                        eprintln!(
                            "[BarsLoaded] no-match: event={}@{}:{}/{} windows=[{}]",
                            symbol, exchange_id.as_str(), account_type.short_label(), tf_name,
                            window_keys.join(", ")
                        );
                    }

                    // Populate data_provider cache so future LoadPreset calls
                    // can serve bars synchronously via get_bars().
                    if any_matched {
                        if let Some(w) = self.panel_app.panel_grid.containers().iter()
                            .find(|(cid, _w)| {
                
                                self.symbol_for_chart(**cid).map_or(false, |s| s == symbol)
                                    && self.exchange_for_chart(**cid).map_or(false, |e| e == exchange_id.as_str())
                                    && self.timeframe_for_chart(**cid).map_or(false, |tf| tf.name == tf_name)
                                    && self.account_type_for_chart(**cid).map_or(false, |at| at == account_type.short_label())
                            })
                            .map(|(_cid, w)| w)
                        {
                            w.data_provider.insert_bars(&symbol, &tf_name, bars.clone());
                        }
                    }

                    // Recalculate indicators only for windows that match this
                    // BarsLoaded event (symbol + exchange + timeframe).  Using
                    // calculate_for_window instead of calculate_all_for_symbol
                    // prevents leaking bars from one TF into another window's
                    // indicators.
                    // Phase X0 / C5c: bars come from bridge once; all matched windows share them.
                    let matched_ids: Vec<u64> = self
                        .panel_app
                        .panel_grid
                        .containers()
                        .iter()
                        .filter(|(cid, _w)| {
                            self.symbol_for_chart(**cid).map_or(false, |s| s == symbol)
                                && self.exchange_for_chart(**cid).map_or(false, |e| e == exchange_id.as_str())
                                && self.timeframe_for_chart(**cid).map_or(false, |tf| tf.name == tf_name)
                                && self.account_type_for_chart(**cid).map_or(false, |at| at == account_type.short_label())
                        })
                        .map(|(cid, _w)| cid.0)
                        .collect();
                    for wid in &matched_ids {
                        self.core.indicators_mut().calculate_for_window(*wid, &bars);
                    }

                    if any_matched {
                        if self.sidebar_state.connector_enabled.get(exchange_id.as_str()).copied().unwrap_or(true) {
                            self.core.bridge.subscribe_trades(exchange_id, &symbol, account_type);
                        }
                    }
                }
                LiveUpdate::BackfillComplete { exchange_id, account_type, symbol, timeframe: tf_name, bars } => {
                    eprintln!(
                        "[tick::BackfillComplete recv] {:?} {} at={:?} tf={} bars={} first_ts={} last_ts={}",
                        exchange_id, symbol, account_type, tf_name, bars.len(),
                        bars.first().map(|b| b.timestamp).unwrap_or(0),
                        bars.last().map(|b| b.timestamp).unwrap_or(0),
                    );
                    crate::dlog!("[ChartApp] BackfillComplete: {} {} tf={} bars={}", exchange_id.as_str(), symbol, tf_name, bars.len());
                    let mut prev_close_mirrors: Vec<(mlc_core::state::ChartId, Option<f64>)> = Vec::new();

                    let instrument_by_cid_bc: std::collections::HashMap<mlc_core::state::ChartId, (String, mlc_core::state::Timeframe, String, String)> = {
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

                    // Phase X0 / C5c: capture anchors before bubble write.
                    let prev_bars_for_anchor: Vec<mlc_core::Bar> = {
                        let mgr = self.core.bubble();
                        mgr.bubble.bubble_for_instrument(exchange_id.as_str(), account_type.short_label(), &symbol)
                            .and_then(|bid| mgr.bubble.bubble(bid))
                            .map(|b| b.bars.clone())
                            .unwrap_or_default()
                    };
                    let mut matched_anchors: Vec<(mlc_core::state::ChartId, Option<(i64, f64)>)> = Vec::new();
                    for (chart_id_key, window) in self.panel_app.panel_grid.containers().iter() {
                        let (win_sym, win_tf, win_exch, win_at) = match instrument_by_cid_bc.get(chart_id_key) {
                            Some(t) => t,
                            None => continue,
                        };
                        if !(win_sym.as_str() == symbol
                            && win_exch.as_str() == exchange_id.as_str()
                            && win_tf.name == tf_name
                            && win_at.as_str() == account_type.short_label())
                        {
                            continue;
                        }
                        matched_anchors.push((*chart_id_key, window.capture_view_anchor(&prev_bars_for_anchor)));
                    }

                    // Write bubble.bars into each matched chart's own bubble
                    // (resolved via bubble_for_chart — same source of truth the
                    // renderer reads).
                    let target_bubbles: std::collections::HashSet<mlc_core::tag_manager::BubbleId> =
                        matched_anchors.iter()
                            .filter_map(|(cid, _)| self.bubble_for_chart(*cid))
                            .collect();
                    for bid in target_bubbles {
                        if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(bid) {
                            bubble.bars = bars.clone();
                        }
                    }

                    for (chart_id_key, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                        let Some((_, anchor)) = matched_anchors.iter().find(|(c, _)| c == chart_id_key).copied() else { continue; };
                        window.reconcile_view_keep_anchor(&bars, anchor);
                        if window.needs_snap_after_backfill {
                            window.consume_cold_snap(&bars);
                        } else if window.price_scale.scale_mode.is_auto_y() {
                            window.calc_auto_scale(&bars);
                        }
                        prev_close_mirrors.push((*chart_id_key, bars.first().map(|b| b.open)));
                    }
                    for (chart_id, prev_close) in prev_close_mirrors {
                        if let Some(bid) = self.bubble_for_chart(chart_id) {
                            if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(bid) {
                                bubble.prev_close_price = prev_close;
                            }
                        }
                    }

                    // Recalculate indicators for matched windows.
                    let matched_ids: Vec<u64> = self
                        .panel_app
                        .panel_grid
                        .containers()
                        .iter()
                        .filter(|(cid, _w)| {
                            self.symbol_for_chart(**cid).map_or(false, |s| s == symbol)
                                && self.exchange_for_chart(**cid).map_or(false, |e| e == exchange_id.as_str())
                                && self.timeframe_for_chart(**cid).map_or(false, |tf| tf.name == tf_name)
                                && self.account_type_for_chart(**cid).map_or(false, |at| at == account_type.short_label())
                        })
                        .map(|(cid, _w)| cid.0)
                        .collect();
                    for wid in &matched_ids {
                        self.core.indicators_mut().calculate_for_window(*wid, &bars);
                    }
                }
                LiveUpdate::ScrollBarsLoaded { exchange_id, account_type, symbol, timeframe: tf_name, bars, prepend_count } => {
                    crate::dlog!("[ChartApp] ScrollBarsLoaded: {} {} tf={} bars={} prepend={}",
                        exchange_id.as_str(), symbol, tf_name, bars.len(), prepend_count);

                    let at_label = account_type.short_label();
                    let mut any_matched = false;
                    let mut prev_close_mirrors: Vec<(mlc_core::state::ChartId, Option<f64>)> = Vec::new();

                    let instrument_by_cid_sb: std::collections::HashMap<mlc_core::state::ChartId, (String, mlc_core::state::Timeframe, String, String)> = {
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

                    // Phase X0 / C5c: capture anchors before bubble write.
                    let prev_bars_for_anchor: Vec<mlc_core::Bar> = {
                        let mgr = self.core.bubble();
                        mgr.bubble.bubble_for_instrument(exchange_id.as_str(), at_label, &symbol)
                            .and_then(|bid| mgr.bubble.bubble(bid))
                            .map(|b| b.bars.clone())
                            .unwrap_or_default()
                    };
                    let mut matched_anchors: Vec<(mlc_core::state::ChartId, Option<(i64, f64)>)> = Vec::new();
                    for (chart_id_key, window) in self.panel_app.panel_grid.containers().iter() {
                        let (win_sym, win_tf, win_exch, win_at) = match instrument_by_cid_sb.get(chart_id_key) {
                            Some(t) => t,
                            None => continue,
                        };
                        if !(win_sym.as_str() == symbol
                            && win_exch.as_str() == exchange_id.as_str()
                            && win_tf.name == tf_name
                            && win_at.as_str() == at_label)
                        {
                            continue;
                        }
                        any_matched = true;
                        matched_anchors.push((*chart_id_key, window.capture_view_anchor(&prev_bars_for_anchor)));
                    }

                    let target_bubbles: std::collections::HashSet<mlc_core::tag_manager::BubbleId> =
                        matched_anchors.iter()
                            .filter_map(|(cid, _)| self.bubble_for_chart(*cid))
                            .collect();
                    for bid in target_bubbles {
                        if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(bid) {
                            bubble.bars = bars.clone();
                        }
                    }

                    for (chart_id_key, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                        let Some((_, anchor)) = matched_anchors.iter().find(|(c, _)| c == chart_id_key).copied() else { continue; };
                        window.scroll_fetch_in_flight = false;
                        window.scroll_fetch_started = None;
                        window.reconcile_view_keep_anchor(&bars, anchor);
                        if window.price_scale.scale_mode.is_auto_y() {
                            window.calc_auto_scale(&bars);
                        }
                        prev_close_mirrors.push((*chart_id_key, bars.first().map(|b| b.open)));
                    }
                    for (chart_id, prev_close) in prev_close_mirrors {
                        if let Some(bid) = self.bubble_for_chart(chart_id) {
                            if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(bid) {
                                bubble.prev_close_price = prev_close;
                            }
                        }
                    }

                    if any_matched {
                        let matched_ids: Vec<u64> = self
                            .panel_app
                            .panel_grid
                            .containers()
                            .iter()
                            .filter(|(cid, _w)| {
                                self.symbol_for_chart(**cid).map_or(false, |s| s == symbol)
                                    && self.exchange_for_chart(**cid).map_or(false, |e| e == exchange_id.as_str())
                                    && self.timeframe_for_chart(**cid).map_or(false, |tf| tf.name == tf_name)
                            })
                            .map(|(cid, _w)| cid.0)
                            .collect();
                        for wid in &matched_ids {
                            self.core.indicators_mut().calculate_for_window(*wid, &bars);
                        }
                    }
                }
                LiveUpdate::BarUpdate { exchange_id: bar_exch, account_type: bar_account_type, symbol: bar_symbol, bar } => {
                    // Derived bar modes (Range/Tick/Volume/Footprint) receive bars
                    // exclusively via BarUpdate from dig3. TimeBased windows still
                    // aggregate from TradeUpdate; this handler is a no-op for them.

                    // Pre-collect (symbol, account_label, is_derived) per cid to
                    // avoid holding &self.managers.bubble while iterating containers_mut().
                    //
                    // is_derived is true when EITHER:
                    //   - the timeframe is a tick TF (1t/10t/100t/1000t — N trades/bar),
                    //   - OR the chart_type is one of range_bar/volume_bar/footprint
                    //     (the time TF stays, but the bar stream comes from station's
                    //     RangeBar/VolumeBar/Footprint derived stream, not from local
                    //     time-slice aggregation).
                    let instrument_by_cid_bu: std::collections::HashMap<mlc_core::state::ChartId, (String, String, bool)> = {
                        let panels_store = &self.panels_store;
                        self.panel_app.panel_grid.containers().iter()
                            .filter_map(|(cid, win)| {
                                let sym = self.symbol_for_chart(*cid)?;
                                let at = self.account_type_for_chart(*cid)?;
                                let is_tick_tf = self.timeframe_for_chart(*cid)
                                    .map(|tf| tf.is_tick())
                                    .unwrap_or(false);
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

                    // Phase X0 / C5c: write every bubble whose chart matches this
                    // instrument AND is a derived-bar chart (upsert by ts).
                    let bar_at_label = bar_account_type.short_label();
                    let target_bubbles_bu: std::collections::HashSet<mlc_core::tag_manager::BubbleId> =
                        instrument_by_cid_bu.iter()
                            .filter(|(_, (sym, at, is_derived))|
                                *is_derived && sym == bar_symbol.as_str() && at == bar_at_label
                            )
                            .filter_map(|(cid, _)| self.bubble_for_chart(*cid))
                            .collect();
                    for bid in &target_bubbles_bu {
                        if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(*bid) {
                            let last_ts = bubble.bars.last().map(|b| b.timestamp);
                            if last_ts == Some(bar.timestamp) {
                                if let Some(last) = bubble.last_bar_mut() {
                                    last.open   = bar.open;
                                    last.high   = bar.high;
                                    last.low    = bar.low;
                                    last.close  = bar.close;
                                    last.volume = bar.volume;
                                }
                                bu_is_new_bar = false;
                            } else {
                                bubble.append_bar(bar.clone());
                                bu_is_new_bar = true;
                            }
                        }
                    }
                    // Snapshot bars from any one of the target bubbles for the
                    // window reconcile loop below.
                    let bu_bubble_bars: Vec<mlc_core::Bar> = target_bubbles_bu.iter().next()
                        .and_then(|bid| self.with_bubble(*bid, |b| b.bars.clone()))
                        .unwrap_or_default();

                    for (cid_bu, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                        let (win_sym, win_at, is_derived) = match instrument_by_cid_bu.get(cid_bu) {
                            Some(t) => t,
                            None => continue,
                        };
                        if !is_derived {
                            if window.pending_symbol_load { continue; }
                            continue;
                        }
                        if win_sym.as_str() != bar_symbol || win_at.as_str() != bar_account_type.short_label() {
                            continue;
                        }
                        if window.pending_symbol_load {
                            window.pending_symbol_load = false;
                        }
                        if window.needs_snap_after_backfill
                            && window.viewport.chart_width > 0.0
                        {
                            window.consume_cold_snap(&bu_bubble_bars);
                        }

                        window.viewport.bar_count = bu_bubble_bars.len();

                        if window.price_scale.scale_mode.is_auto_y() {
                            let _as_start = std::time::Instant::now();
                            window.calc_auto_scale(&bu_bubble_bars);
                            self.last_auto_scale_us += _as_start.elapsed().as_micros() as u64;
                        }

                        let count = bu_bubble_bars.len();
                        let visible_f = window.viewport.chart_width / window.viewport.bar_spacing;

                        if window.price_scale.scale_mode.is_follow() {
                            window.snap_to_end(mlc_core::DEFAULT_SNAP_MARGIN);
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

                    // Trigger indicator recompute and repaint when at least one derived
                    // window applied this bar — mirrors the TradeUpdate recalc path.
                    if let Some(ref sym) = bu_symbol_for_recalc {
                        had_trade_update = true;
                        let recalc_mode = self.core.indicators().recalc_mode;
                        match recalc_mode {
                            RecalcMode::PerTick => {
                                let _ri_start = std::time::Instant::now();
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
                    // C5c: bubble write done at the top of the arm.
                }
                LiveUpdate::TradeUpdate { exchange_id, symbol, price, quantity, timestamp, account_type, is_buyer_maker: _ } => {
                    self.trade_count += 1;
                    self.last_trade_event_count += 1;
                    had_trade_update = true;
                    // Track whether any window formed a new bar for this symbol.
                    let mut is_new_bar = false;
                    // Track whether a multi-bar gap was detected (needs REST backfill).
                    let mut needs_backfill = false;

                    let _bar_apply_start = std::time::Instant::now();

                    // Phase 4 B: pre-collect instrument map from branch_meta for TradeUpdate.
                    // Also pre-collect `is_derived` flag per cid: derived-mode windows (Range/Tick/Volume/FP)
                    // are owned by BarUpdate and must be skipped here to avoid double-aggregation.
                    let instrument_by_cid_tu: std::collections::HashMap<mlc_core::state::ChartId, (String, mlc_core::state::Timeframe, String, String)> = {
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
                    // Pre-collect derived cids separately (same borrow scope as above).
                    // Derived = tick TF; gets bars via BarUpdate and must skip
                    // local time-slice aggregation here.
                    let derived_cids_tu: std::collections::HashSet<mlc_core::state::ChartId> = {
                        self.panel_app.panel_grid.containers().keys()
                            .filter(|cid| {
                                self.timeframe_for_chart(**cid)
                                    .map(|tf| tf.is_tick())
                                    .unwrap_or(false)
                            })
                            .copied()
                            .collect()
                    };

                    // Phase X0 / C5c: per-bubble aggregation by tf — write bubble FIRST, then loop windows.
                    // Aggregation period comes from any matched window's tf (all share-symbol windows
                    // bound to the same bubble share the same tf via bubble identity).
                    let mut bubble_periods: std::collections::HashMap<mlc_core::tag_manager::BubbleId, i64> = std::collections::HashMap::new();
                    for (cid_tu, _window) in self.panel_app.panel_grid.containers().iter() {
                        if derived_cids_tu.contains(cid_tu) { continue; }
                        let (win_sym_tu, win_tf_tu, _win_exch_tu, win_at_tu) = match instrument_by_cid_tu.get(cid_tu) {
                            Some(t) => t,
                            None => continue,
                        };
                        if win_sym_tu.as_str() != symbol || win_at_tu.as_str() != account_type.short_label() {
                            continue;
                        }
                        let period_secs = win_tf_tu.interval_ms / 1000;
                        if period_secs <= 0 { continue; }
                        if let Some(bid) = self.bubble_for_chart(*cid_tu) {
                            bubble_periods.insert(bid, period_secs);
                        }
                    }
                    // Apply trade to each affected bubble's bars.
                    for (bid, period_secs) in &bubble_periods {
                        if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(*bid) {
                            let trade_ts_secs = timestamp / 1000;
                            if let Some(last_ts) = bubble.bars.last().map(|b| b.timestamp) {
                                let candle_end = last_ts + period_secs;
                                if trade_ts_secs >= candle_end {
                                    if trade_ts_secs >= candle_end + period_secs {
                                        needs_backfill = true;
                                    }
                                    let new_candle_start = (trade_ts_secs / period_secs) * period_secs;
                                    bubble.append_bar(mlc_core::Bar {
                                        timestamp: new_candle_start,
                                        open: price, high: price, low: price, close: price, volume: quantity,
                                    });
                                    is_new_bar = true;
                                } else if let Some(last) = bubble.last_bar_mut() {
                                    last.close = price;
                                    if price > last.high { last.high = price; }
                                    if price < last.low { last.low = price; }
                                    last.volume += quantity;
                                }
                            } else {
                                let candle_start = (trade_ts_secs / period_secs) * period_secs;
                                bubble.append_bar(mlc_core::Bar {
                                    timestamp: candle_start,
                                    open: price, high: price, low: price, close: price, volume: quantity,
                                });
                                is_new_bar = true;
                            }
                        }
                    }

                    // Window-side viewport reconcile loop.
                    for (cid_tu, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                        if window.pending_symbol_load { continue; }
                        if derived_cids_tu.contains(cid_tu) { continue; }
                        let (win_sym_tu, _win_tf_tu, _win_exch_tu, win_at_tu) = match instrument_by_cid_tu.get(cid_tu) {
                            Some(t) => t,
                            None => continue,
                        };
                        if !(win_sym_tu.as_str() == symbol && win_at_tu.as_str() == account_type.short_label()) {
                            continue;
                        }
                        // Snapshot bubble bars for this window's bubble.
                        // (Window's bubble was already updated above.)
                        // bubble_bars borrowed via separate immutable read at the start of each iter.
                        // To avoid double-borrow, we don't re-resolve; window does only post-effects
                        // that need bar_count + bars slice for auto_scale/snap.
                        // Use bar_count update from is_new_bar tracking; auto_scale needs slice.

                        // We deliberately skip per-window auto_scale here — it ran on the bubble
                        // path via reconcile in prepare_frame next frame. For follow-mode snap we
                        // need viewport.bar_count post-write, so update it now.
                        window.viewport.bar_count = window.viewport.bar_count.saturating_add(if is_new_bar { 1 } else { 0 });
                        let count = window.viewport.bar_count;
                        let visible_f = window.viewport.chart_width / window.viewport.bar_spacing;

                        if window.price_scale.scale_mode.is_follow() {
                            window.snap_to_end(mlc_core::DEFAULT_SNAP_MARGIN);
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

                    // Multi-bar gap detected — trigger REST backfill to fill missing candles.
                    if needs_backfill {
                        crate::dlog!("[ChartApp] Multi-bar gap detected for {} — requesting REST backfill", symbol);
                        let bridge = self.core.bridge.clone();
                        for cid in self.panel_app.panel_grid.containers().keys() {
                            let sym_match = self.symbol_for_chart(*cid).map_or(false, |s| s == symbol);
                            let at_match = self.account_type_for_chart(*cid).map_or(false, |at| at == account_type.short_label());
                            if sym_match && at_match {
                                let at_label = self.account_type_for_chart(*cid).unwrap_or_default();
                                let at = account_type_from_label(&at_label);
                                if let Some(tf) = self.timeframe_for_chart(*cid) {
                                    bridge.request_bars(exchange_id, &symbol, &tf, at, None, None, false);
                                }
                            }
                        }
                    }

                    // (Phase 2B step 2 — trades) The station's Series<TradePoint>
                    // forwarder owns the trade ring + DiskStore now; panels read
                    // from station.series::<TradePoint>(&key) directly through
                    // the handle stored in their `shared_trades` field. The
                    // bridge no longer maintains a mirror ring, and tick.rs no
                    // longer needs to push the freshly-drained TradeUpdate into
                    // any shared structure — the station has already written
                    // the same point milliseconds earlier.

                    // Time-axis (tick) windows: keep the newest trade near the
                    // right edge while in follow/auto mode. The bar-aggregation
                    // path above skips derived windows, so the time-axis viewport
                    // is advanced here off the live trade timestamp.
                    for (cid_tu, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                        if !window.viewport.is_time_axis() {
                            continue;
                        }
                        let follow = window.price_scale.scale_mode.is_follow()
                            || window.price_scale.scale_mode == ScaleMode::Auto;
                        if !follow {
                            continue;
                        }
                        if let Some((win_sym, _, _, win_at)) = instrument_by_cid_tu.get(cid_tu) {
                            if win_sym.as_str() == symbol && win_at.as_str() == account_type.short_label() {
                                window.viewport.snap_time_to_end(timestamp, 50.0);
                            }
                        }
                    }

                    // Pull new trades from the shared ring into all order-flow panels.
                    // All four panel kinds now read from shared_trades via tick().
                    let _trade_panel_start = std::time::Instant::now();
                    for state in self.panels_store.big_trades.values_mut() {
                        state.tick();
                    }
                    for state in self.panels_store.volume_profile.values_mut() {
                        state.tick();
                    }
                    for state in self.panels_store.footprint.values_mut() {
                        state.tick();
                    }
                    // Lazy self-heal for TradeTape after preset restore:
                    // shared_trades is a runtime Arc handle that is NOT persisted —
                    // after restore state.symbol is correct but the handle is None.
                    // Sync group apply_key_to_panels_in_group will set it on the next
                    // active-chart-change or symbol-change, but in the meantime
                    // (first frames after load) the panel would render empty. This
                    // loop subscribes proactively whenever the handle is None and
                    // we have a non-empty (symbol, exchange) pair.
                    for state in self.panels_store.trade_tape.values_mut() {
                        if state.shared_trades.is_none()
                            && !state.symbol.is_empty()
                            && !state.exchange.is_empty()
                        {
                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                let at = account_type_from_label(&state.account_type);
                                let h = self.core.bridge.subscribe_trades(eid, &state.symbol, at);
                                state.shared_trades = h;
                                state.last_seen_ts_ms = 0;
                            }
                        }
                        state.tick();
                    }
                    self.last_trade_panel_us += _trade_panel_start.elapsed().as_micros() as u64;

                    // Schedule indicator recalculation according to the current mode.
                    let recalc_mode = self.core.indicators().recalc_mode;
                    match recalc_mode {
                        RecalcMode::PerTick => {
                            // Immediate recalc — pull bars from ALL windows with this symbol
                            // (fixes the bug where only the active window was considered).
                            let _ri_start = std::time::Instant::now();
                            self.recalc_indicators_for_symbol(&symbol);
                            self.last_indicator_recalc_us += _ri_start.elapsed().as_micros() as u64;
                        }
                        RecalcMode::PerFrame => {
                            // Defer to end-of-tick flush; all trades in this frame are batched.
                            self.core.indicators_mut().mark_dirty(&symbol);
                        }
                        RecalcMode::PerBar => {
                            if is_new_bar {
                                crate::dlog!("[ChartApp] PerBar: new bar detected for {}", symbol);
                                self.core.indicators_mut().mark_new_bar(&symbol);
                            } else {
                                // Still mark dirty so the flag exists; drain_pending_recalc
                                // will ignore it in PerBar mode unless a new bar formed.
                                self.core.indicators_mut().mark_dirty(&symbol);
                            }
                        }
                    }

                    // Record the latest trade price for the ghost-level filter.
                    // Applied ONCE per symbol after the drain loop (coalesced) so a
                    // 172-trade burst does not trigger 172 full book sweeps + view
                    // republishes (the old ~21ms spike).
                    last_trade_price_by_symbol.insert(
                        (symbol.clone(), exchange_id.as_str().to_string(), account_type.short_label().to_string()),
                        price,
                    );
                    // Phase X0 / C2: mirror first matched window's bars into bubble storage.
                    // TradeUpdate aggregates into the open bar (in-place) or rolls a new one,
                    // C5c: bubble aggregation done at the top of the arm.
                }
                LiveUpdate::MiniTickerUpdate { exchange_id, symbol, last_price, price_change_percent, high_price, low_price, volume, account_type } => {
                    // Cache the 24h ticker stats keyed by symbol:exchange:account_type so that
                    // the same symbol on different exchanges or account types gets separate entries.
                    //
                    // Stats fields (price_change_percent, high, low, volume) are
                    // Option: BBO-only events (e.g. KuCoin `trade.ticker`) carry
                    // None for those fields and must not overwrite the values that
                    // a prior full-snapshot event already wrote into the cache.
                    let cache_key = format!("{}:{}:{}", symbol, exchange_id.as_str(), account_type.short_label());
                    let mut entry = self.core.mini_ticker
                        .entry(cache_key)
                        .or_insert(crate::MiniTickerData {
                            last_price,
                            price_change_percent: 0.0,
                            high_price: 0.0,
                            low_price: 0.0,
                            volume: 0.0,
                        });
                    // Always update last_price — it is always present.
                    entry.last_price = last_price;
                    // Only update stats fields when the event carries them.
                    if let Some(v) = price_change_percent { entry.price_change_percent = v; }
                    if let Some(v) = high_price           { entry.high_price = v; }
                    if let Some(v) = low_price            { entry.low_price = v; }
                    if let Some(v) = volume               { entry.volume = v; }

                    // Mirror last_price into the ghost filter — coalesced with the
                    // trade-price map (applied once per symbol after drain), so a
                    // burst of BBO ticker events does not trigger per-event book
                    // sweeps + view republishes.
                    last_trade_price_by_symbol.insert(
                        (symbol.clone(), exchange_id.as_str().to_string(), account_type.short_label().to_string()),
                        last_price,
                    );
                }
                LiveUpdate::ConnectorReady { exchange_id } => {
                    let eid_str = exchange_id.as_str();
                    if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                        crate::dlog!("[ChartApp] ConnectorReady for disabled {}, ignoring", eid_str);
                        continue;
                    }
                    // Symbols are requested at the APP level (App::tick_app_state via
                    // the dedicated mpsc ConnectorReady channel) — exactly once per
                    // exchange, regardless of window count. Calling it here too made
                    // every window race the app for the same first NET-FETCH, doubling
                    // the Spot+Futures REST work on the bridge's 2-worker runtime
                    // during the startup wave. Cache fills on the first response;
                    // any later window reading symbols hits the cache anyway.
                    let bridge = self.core.bridge.clone();

                    // Subscribe mini-tickers for watchlist symbols on this exchange.
                    let exchange_str = exchange_id.as_str();
                    if let Some(wl) = self.sidebar_state.watchlist_manager.active_list() {
                        for ws in wl.all_symbols() {
                            if ws.exchange == exchange_str {
                                let ws_at = account_type_from_label(&ws.account_type);
                                bridge.subscribe_mini_ticker(exchange_id, &ws.symbol, ws_at);
                            }
                        }
                    }

                    // Backfill bars for ALL windows on this exchange (covers reconnect gaps).
                    // force=true bypasses the cache_is_fresh guard so a reconnect always
                    // fetches fresh data even if the last cached bar is recent.
                    for (cid, _window) in self.panel_app.panel_grid.containers().iter() {
                        let sym = self.symbol_for_chart(*cid).unwrap_or_default();
                        if sym.is_empty() { continue; }
                        let exch = self.exchange_for_chart(*cid).unwrap_or_default();
                        let win_eid = digdigdig3::ExchangeId::from_str(&exch)
                            .unwrap_or(digdigdig3::ExchangeId::Binance);
                        if win_eid == exchange_id {
                            let at_label = self.account_type_for_chart(*cid).unwrap_or_default();
                            let at = account_type_from_label(&at_label);
                            if let Some(tf) = self.timeframe_for_chart(*cid) {
                                // ConnectorReady fires on initial connect AND reconnects.
                                // force=false lets the bridge skip if a Stream::Kline
                                // forwarder is already piping live ticks for this key —
                                // otherwise every reconnect would re-trigger Phase A,
                                // overwrite window.bars with a slightly-different fresh
                                // 300-bar window, and bounce the viewport. The cold
                                // first-connect case still spawns the subscriber +
                                // emits the initial Phase A BarsLoaded.
                                eprintln!(
                                    "[tick::ConnectorReady -> request_bars] {:?} {} at={:?} tf={} force=false",
                                    exchange_id, sym, at, tf.name,
                                );
                                bridge.request_bars(exchange_id, &sym, &tf, at, None, None, false);
                            }
                        }
                    }
                    if let Some(mut tm) = self.core.trading_lock() {
                        tm.on_connector_ready(exchange_id);
                    }
                }
                LiveUpdate::SymbolsLoaded { exchange_id, symbols } => {
                    let _t = std::time::Instant::now();
                    self.core.exchange_symbols_mut().insert(exchange_id, symbols);
                    symbols_insert_us += _t.elapsed().as_micros() as u64;
                    // Preload bridge's wire_symbol cache for this exchange.
                    // Trades one O(N) scan now for ~180ms saved on the FIRST
                    // request_bars / subscribe_* per (symbol, account_type)
                    // pair later — turns the first cold tab click into a
                    // pure UI swap instead of a 180ms freeze.
                    self.core.bridge.prewarm_wire_symbol_cache(exchange_id);
                }
                LiveUpdate::Error { exchange_id, account_type, symbol, timeframe: tf_name_err, message } => {
                    eprintln!("[ChartApp] live-data error ({:?}): {}", exchange_id, message);
                    // Propagate to matching windows so the UI shows the error
                    // instead of a perpetual "Loading..." label.
                    if !symbol.is_empty() {
                        let at_label = account_type.short_label();
                        // Pre-collect matching chart ids to avoid double-borrow
                        // (bubble is immutable, panel_grid needs mutable for data_error).
                        let error_instrument_map: std::collections::HashMap<mlc_core::state::ChartId, bool> = {
                            self.panel_app.panel_grid.containers().keys()
                                .filter_map(|cid| {
                                    let win_exch = self.exchange_for_chart(*cid)?;
                                    let win_sym = self.symbol_for_chart(*cid)?;
                                    let win_at = self.account_type_for_chart(*cid)?;
                                    let win_tf = self.timeframe_for_chart(*cid)?;
                                    let matches = win_exch == exchange_id.as_str()
                                        && win_sym == symbol
                                        && win_at == at_label
                                        && win_tf.name == tf_name_err;
                                    if matches { Some((*cid, true)) } else { None }
                                })
                                .collect()
                        };
                        for (cid_err, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                            if error_instrument_map.contains_key(cid_err) && true {
                                window.data_error = Some(message.clone());
                            }
                        }
                    }
                }
                LiveUpdate::OrderbookSnapshot { exchange_id, account_type, symbol, source, .. } => {
                    self.last_ob_event_count += 1;
                    match source {
                        mlc_live_data::OrderbookSource::Rest => {
                            // REST bootstrap — force DOM to full-rebuild from the tracker
                            // (the bridge station forwarder already wrote the snapshot into it).
                            let key = (symbol, exchange_id.as_str().to_string(), account_type.short_label().to_string());
                            let entry = ob_dirty.entry(key).or_insert_with(|| ObDeltaEntry {
                                events: Vec::new(),
                                force_rebuild: false,
                            });
                            entry.force_rebuild = true;
                        }
                        mlc_live_data::OrderbookSource::Ws => {
                            // WS snapshot always precedes a Delta event. Skip here;
                            // the tracker is already updated by the bridge task.
                        }
                    }
                }
                LiveUpdate::OrderbookDelta { exchange_id, account_type, symbol, bids, asks, timestamp } => {
                    self.last_ob_event_count += 1;
                    // Push each delta event with its original wire timestamp so
                    // L2-tape rows get per-event ts instead of one SystemTime::now()
                    // for the whole per-frame batch.
                    let key = (symbol.clone(), exchange_id.as_str().to_string(), account_type.short_label().to_string());
                    let entry = ob_dirty.entry(key).or_insert_with(|| ObDeltaEntry {
                        events: Vec::new(),
                        force_rebuild: false,
                    });
                    entry.events.push(ObDeltaEvent { ts_ms: timestamp, bids: bids.clone(), asks: asks.clone() });

                    // Multi-DOM fan-out: route the delta into every
                    // MultiDomState whose `(symbol, exchange in config)`
                    // matches this update. Delta replaces the per-exchange
                    // snapshot (single-shot rebuild model — finer-grain
                    // delta-merge is a Phase 7.3 followup).
                    let ex_label = exchange_id.as_str().to_string();
                    let sym_ref = symbol.as_str();
                    for state in self.panels_store.multi_dom.values_mut() {
                        if state.symbol != sym_ref { continue; }
                        if let Some(idx) = state.exchange_idx(&ex_label) {
                            let bids_iter = bids.iter().map(|(p, q)| (*p, *q));
                            let asks_iter = asks.iter().map(|(p, q)| (*p, *q));
                            state.apply_snapshot(idx, bids_iter, asks_iter);
                            state.rebuild_merged();
                        }
                    }
                }
                LiveUpdate::ConnectorMetrics { .. } => {
                    // Metrics snapshots are collected on-demand by the metrics panel.
                    // No action needed in the main update loop.
                }
                LiveUpdate::OrderUpdate { .. } => {
                    trading_updates.push(update.clone());
                }
                LiveUpdate::BalanceUpdate { .. } => {
                    trading_updates.push(update.clone());
                }
                LiveUpdate::PositionUpdate { .. } => {
                    trading_updates.push(update.clone());
                }
                // Footprint cluster grid (C7): convert the station-derived
                // `FootprintPoint` into our local `FootprintBar` and route into
                // every window whose chart_type == "footprint" and whose
                // (symbol, account_type) matches this update. The renderer
                // (`chart_render::footprint::draw_footprint_cluster`) reads
                // `window.footprint_bars` and paints Bid × Ask per level.
                LiveUpdate::FootprintUpdate { exchange_id, account_type: fp_at, symbol: fp_sym, point } => {
                    let fp_at_label = fp_at.short_label();
                    // Pre-collect (symbol, account_label, is_footprint) per cid
                    // to avoid holding &self.managers while mutating containers.
                    let footprint_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                        let panels_store = &self.panels_store;
                        self.panel_app.panel_grid.containers().iter()
                            .filter_map(|(cid, win)| {
                                let sym = self.symbol_for_chart(*cid)?;
                                let at = self.account_type_for_chart(*cid)?;
                                let is_footprint = panels_store.chart_panel
                                    .get(&win.chart_panel_id)
                                    .map(|p| p.chart_type == "footprint")
                                    .unwrap_or(false);
                                if !is_footprint { return None; }
                                if sym != fp_sym.as_str() { return None; }
                                if at != fp_at_label { return None; }
                                Some(*cid)
                            })
                            .collect()
                    };
                    if footprint_cids.is_empty() {
                        continue;
                    }
                    // Build the local FootprintBar once; clone per matching window.
                    let new_bar = mlc_core::FootprintBar {
                        open_time: point.open_time,
                        open: point.open,
                        high: point.high,
                        low: point.low,
                        close: point.close,
                        volume: point.volume,
                        levels: point.levels.iter()
                            .map(|(price, buy_vol, sell_vol)| mlc_core::FootprintLevel {
                                price: *price,
                                buy_vol: *buy_vol,
                                sell_vol: *sell_vol,
                                is_single_print: false,
                                is_unfinished: false,
                            })
                            .collect(),
                        cumulative_delta: 0.0,
                        stacked_ask_start: None,
                        stacked_ask_len: None,
                        stacked_bid_start: None,
                        stacked_bid_len: None,
                        is_absorption: false,
                    };
                    // Infer tick_size from the smallest positive level step
                    // on this bar (fallback: 1.0). Industry default
                    // imbalance ratio = 3.0×, stack count = 3, absorption
                    // ratio = 0.15, single-print share = 0.05.
                    let tick_size = {
                        let mut step = f64::INFINITY;
                        for w in new_bar.levels.windows(2) {
                            let d = (w[1].price - w[0].price).abs();
                            if d > 0.0 && d < step { step = d; }
                        }
                        if step.is_finite() { step } else { 1.0 }
                    };
                    // Compute F1-F4 static extras ONCE per event. CVD is
                    // window-scoped (per-store prior bar) so it's set in
                    // the loop below.
                    let mut new_bar_with_static = new_bar.clone();
                    new_bar_with_static.compute_static_extras(3.0, 3, 0.15, 0.05, tick_size);
                    // Phase X0 / C5c: write footprint + OHLC bars to bubble FIRST.
                    let ohlc_ts = new_bar_with_static.open_time / 1000;
                    let ohlc_bar = mlc_core::Bar::with_volume(
                        ohlc_ts,
                        new_bar_with_static.open,
                        new_bar_with_static.high,
                        new_bar_with_static.low,
                        new_bar_with_static.close,
                        new_bar_with_static.volume,
                    );
                    let fp_target_bubbles: std::collections::HashSet<mlc_core::tag_manager::BubbleId> =
                        footprint_cids.iter()
                            .filter_map(|cid| self.bubble_for_chart(*cid))
                            .collect();
                    for bid in &fp_target_bubbles {
                        if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(*bid) {
                            // Footprint upsert with CVD continuation.
                            let store = bubble.footprint_bars.get_or_insert_with(Vec::new);
                            let prev_cvd = match store.last() {
                                Some(last) if last.open_time != new_bar_with_static.open_time => last.cumulative_delta,
                                Some(_) if store.len() >= 2 => store[store.len() - 2].cumulative_delta,
                                _ => 0.0,
                            };
                            let mut bar = new_bar_with_static.clone();
                            bar.set_cumulative_delta(prev_cvd);
                            match store.last_mut() {
                                Some(last) if last.open_time == bar.open_time => { *last = bar; }
                                _ => store.push(bar),
                            }
                            // OHLC skeleton mirror in bubble.bars.
                            let last_ohlc_ts = bubble.bars.last().map(|b| b.timestamp);
                            if last_ohlc_ts == Some(ohlc_ts) {
                                if let Some(last) = bubble.last_bar_mut() {
                                    last.open   = ohlc_bar.open;
                                    last.high   = ohlc_bar.high;
                                    last.low    = ohlc_bar.low;
                                    last.close  = ohlc_bar.close;
                                    last.volume = ohlc_bar.volume;
                                }
                            } else {
                                bubble.append_bar(ohlc_bar.clone());
                            }
                        }
                    }
                    // Snapshot updated bubble bars for window post-effects.
                    let fp_bubble_bars: Vec<mlc_core::Bar> = {
                        let mgr = self.core.bubble();
                        mgr.bubble.bubble_for_instrument(exchange_id.as_str(), fp_at_label, fp_sym.as_str())
                            .and_then(|bid| mgr.bubble.bubble(bid))
                            .map(|b| b.bars.clone())
                            .unwrap_or_default()
                    };

                    for (cid_fp, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                        if !footprint_cids.contains(cid_fp) { continue; }
                        if window.pending_symbol_load { window.pending_symbol_load = false; }
                        if window.needs_snap_after_backfill && window.viewport.chart_width > 0.0 {
                            window.consume_cold_snap(&fp_bubble_bars);
                        }
                        window.viewport.bar_count = fp_bubble_bars.len();
                    }
                    let _ = new_bar;
                }
                // Renko brick stream — each emit is one closed brick.
                // Append to `window.renko_bars` for every window whose
                // chart_type == "renko" and (symbol, account) matches.
                LiveUpdate::RenkoBarUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    let at_label = at.short_label();
                    let target_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                        let panels_store = &self.panels_store;
                        self.panel_app.panel_grid.containers().iter()
                            .filter_map(|(cid, win)| {
                                let s = self.symbol_for_chart(*cid)?;
                                let a = self.account_type_for_chart(*cid)?;
                                let is_renko = panels_store.chart_panel
                                    .get(&win.chart_panel_id)
                                    .map(|p| p.chart_type == "renko")
                                    .unwrap_or(false);
                                if !is_renko || s != sym.as_str() || a != at_label { return None; }
                                Some(*cid)
                            }).collect()
                    };
                    if target_cids.is_empty() { continue; }
                    let brick = mlc_core::types::RenkoBrick {
                        open_time: point.open_time,
                        bottom: point.bottom,
                        top: point.top,
                        up: point.up,
                        volume: point.volume,
                        trades_count: point.trades_count,
                    };
                    // Phase X0 / C5c: write every bubble whose chart is in target_cids.
                    let renko_targets: std::collections::HashSet<mlc_core::tag_manager::BubbleId> =
                        target_cids.iter()
                            .filter_map(|cid| self.bubble_for_chart(*cid))
                            .collect();
                    for bid in &renko_targets {
                        if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(*bid) {
                            bubble.append_renko(brick.clone());
                        }
                    }
                    let renko_bubble_bars: Vec<mlc_core::Bar> = {
                        let mgr = self.core.bubble();
                        mgr.bubble.bubble_for_instrument(exchange_id.as_str(), at_label, sym.as_str())
                            .and_then(|bid| mgr.bubble.bubble(bid))
                            .map(|b| b.bars.clone())
                            .unwrap_or_default()
                    };
                    for (cid_, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                        if !target_cids.contains(cid_) { continue; }
                        if window.pending_symbol_load { window.pending_symbol_load = false; }
                        if window.needs_snap_after_backfill && window.viewport.chart_width > 0.0 {
                            window.consume_cold_snap(&renko_bubble_bars);
                        }
                    }
                }
                // P&F column stream — emit may update the current open
                // column (same column_id) or roll into a new one.
                LiveUpdate::PnfBarUpdate { exchange_id, account_type: at, symbol: sym, point, box_size: bs } => {
                    let at_label = at.short_label();
                    let target_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                        let panels_store = &self.panels_store;
                        self.panel_app.panel_grid.containers().iter()
                            .filter_map(|(cid, win)| {
                                let s = self.symbol_for_chart(*cid)?;
                                let a = self.account_type_for_chart(*cid)?;
                                let is_pnf = panels_store.chart_panel
                                    .get(&win.chart_panel_id)
                                    .map(|p| p.chart_type == "pnf")
                                    .unwrap_or(false);
                                if !is_pnf || s != sym.as_str() || a != at_label { return None; }
                                Some(*cid)
                            }).collect()
                    };
                    if target_cids.is_empty() { continue; }
                    let col = mlc_core::types::PnfColumn {
                        open_time: point.open_time,
                        column_id: point.column_id,
                        is_x: point.is_x,
                        bottom: point.bottom,
                        top: point.top,
                        volume: point.volume,
                        trades_count: point.trades_count,
                        box_size: bs,
                    };
                    // Phase X0 / C5c: write every bubble whose chart is in target_cids.
                    let pnf_targets: std::collections::HashSet<mlc_core::tag_manager::BubbleId> =
                        target_cids.iter()
                            .filter_map(|cid| self.bubble_for_chart(*cid))
                            .collect();
                    for bid in &pnf_targets {
                        if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(*bid) {
                            let store = bubble.pnf_columns.get_or_insert_with(Vec::new);
                            match store.last_mut() {
                                Some(last) if last.column_id == col.column_id => *last = col.clone(),
                                _ => store.push(col.clone()),
                            }
                        }
                    }
                    let pnf_bubble_bars: Vec<mlc_core::Bar> = {
                        let mgr = self.core.bubble();
                        mgr.bubble.bubble_for_instrument(exchange_id.as_str(), at_label, sym.as_str())
                            .and_then(|bid| mgr.bubble.bubble(bid))
                            .map(|b| b.bars.clone())
                            .unwrap_or_default()
                    };
                    for (cid_, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                        if !target_cids.contains(cid_) { continue; }
                        if window.pending_symbol_load { window.pending_symbol_load = false; }
                        if window.needs_snap_after_backfill && window.viewport.chart_width > 0.0 {
                            window.consume_cold_snap(&pnf_bubble_bars);
                        }
                    }
                }
                // Kagi segment stream — append-only, one event per
                // closed segment.
                LiveUpdate::KagiBarUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    let at_label = at.short_label();
                    let target_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                        let panels_store = &self.panels_store;
                        self.panel_app.panel_grid.containers().iter()
                            .filter_map(|(cid, win)| {
                                let s = self.symbol_for_chart(*cid)?;
                                let a = self.account_type_for_chart(*cid)?;
                                let is_kagi = panels_store.chart_panel
                                    .get(&win.chart_panel_id)
                                    .map(|p| p.chart_type == "kagi")
                                    .unwrap_or(false);
                                if !is_kagi || s != sym.as_str() || a != at_label { return None; }
                                Some(*cid)
                            }).collect()
                    };
                    if target_cids.is_empty() { continue; }
                    let seg = mlc_core::types::KagiSegment {
                        open_time: point.open_time,
                        start_price: point.start_price,
                        end_price: point.end_price,
                        up: point.up,
                        yang: point.yang,
                        is_connector: point.is_connector,
                    };
                    let kagi_targets: std::collections::HashSet<mlc_core::tag_manager::BubbleId> =
                        target_cids.iter()
                            .filter_map(|cid| self.bubble_for_chart(*cid))
                            .collect();
                    for bid in &kagi_targets {
                        if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(*bid) {
                            bubble.append_kagi(seg.clone());
                        }
                    }
                    let kagi_bubble_bars: Vec<mlc_core::Bar> = {
                        let mgr = self.core.bubble();
                        mgr.bubble.bubble_for_instrument(exchange_id.as_str(), at_label, sym.as_str())
                            .and_then(|bid| mgr.bubble.bubble(bid))
                            .map(|b| b.bars.clone())
                            .unwrap_or_default()
                    };
                    for (cid_, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                        if !target_cids.contains(cid_) { continue; }
                        if window.pending_symbol_load { window.pending_symbol_load = false; }
                        if window.needs_snap_after_backfill && window.viewport.chart_width > 0.0 {
                            window.consume_cold_snap(&kagi_bubble_bars);
                        }
                    }
                }
                // CVD line update — append a scalar sample.
                LiveUpdate::CvdLineUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    let at_label = at.short_label();
                    let target_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                        let panels_store = &self.panels_store;
                        self.panel_app.panel_grid.containers().iter()
                            .filter_map(|(cid, win)| {
                                let s = self.symbol_for_chart(*cid)?;
                                let a = self.account_type_for_chart(*cid)?;
                                let is_cvd = panels_store.chart_panel
                                    .get(&win.chart_panel_id)
                                    .map(|p| p.chart_type == "cvd")
                                    .unwrap_or(false);
                                if !is_cvd || s != sym.as_str() || a != at_label { return None; }
                                Some(*cid)
                            }).collect()
                    };
                    if target_cids.is_empty() { continue; }
                    let sample = mlc_core::types::CvdSample {
                        ts_ms: point.ts_ms,
                        value: point.value,
                    };
                    let cvd_targets: std::collections::HashSet<mlc_core::tag_manager::BubbleId> =
                        target_cids.iter()
                            .filter_map(|cid| self.bubble_for_chart(*cid))
                            .collect();
                    for bid in &cvd_targets {
                        if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(*bid) {
                            bubble.append_cvd(sample);
                        }
                    }
                    let cvd_bubble_bars: Vec<mlc_core::Bar> = {
                        let mgr = self.core.bubble();
                        mgr.bubble.bubble_for_instrument(exchange_id.as_str(), at_label, sym.as_str())
                            .and_then(|bid| mgr.bubble.bubble(bid))
                            .map(|b| b.bars.clone())
                            .unwrap_or_default()
                    };
                    for (cid_, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                        if !target_cids.contains(cid_) { continue; }
                        if window.pending_symbol_load { window.pending_symbol_load = false; }
                        if window.needs_snap_after_backfill && window.viewport.chart_width > 0.0 {
                            window.consume_cold_snap(&cvd_bubble_bars);
                        }
                    }
                }
                // TPO session snapshot — upsert in place by session_date
                // (the last entry IS the current session; new date appends).
                LiveUpdate::TpoProfileUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    let at_label = at.short_label();
                    let target_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                        let panels_store = &self.panels_store;
                        self.panel_app.panel_grid.containers().iter()
                            .filter_map(|(cid, win)| {
                                let s = self.symbol_for_chart(*cid)?;
                                let a = self.account_type_for_chart(*cid)?;
                                let is_tpo = panels_store.chart_panel
                                    .get(&win.chart_panel_id)
                                    .map(|p| p.chart_type == "tpo")
                                    .unwrap_or(false);
                                if !is_tpo || s != sym.as_str() || a != at_label { return None; }
                                Some(*cid)
                            }).collect()
                    };
                    if target_cids.is_empty() { continue; }
                    let session = mlc_core::types::TpoSession {
                        open_time: point.open_time,
                        tick_size: point.tick_size,
                        session_high: point.session_high,
                        session_low: point.session_low,
                        poc_price: point.poc_price,
                        vah_price: point.vah_price,
                        val_price: point.val_price,
                        rows: point.rows.clone(),
                    };
                    let tpo_targets: std::collections::HashSet<mlc_core::tag_manager::BubbleId> =
                        target_cids.iter()
                            .filter_map(|cid| self.bubble_for_chart(*cid))
                            .collect();
                    for bid in &tpo_targets {
                        if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(*bid) {
                            let store = bubble.tpo_sessions.get_or_insert_with(Vec::new);
                            match store.last_mut() {
                                Some(last) if last.open_time == session.open_time => *last = session.clone(),
                                _ => {
                                    store.push(session.clone());
                                    const MAX_TPO_SESSIONS: usize = 11;
                                    while store.len() > MAX_TPO_SESSIONS {
                                        store.remove(0);
                                    }
                                }
                            }
                        }
                    }
                    let tpo_bubble_bars: Vec<mlc_core::Bar> = {
                        let mgr = self.core.bubble();
                        mgr.bubble.bubble_for_instrument(exchange_id.as_str(), at_label, sym.as_str())
                            .and_then(|bid| mgr.bubble.bubble(bid))
                            .map(|b| b.bars.clone())
                            .unwrap_or_default()
                    };
                    for (cid_, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                        if !target_cids.contains(cid_) { continue; }
                        if window.pending_symbol_load { window.pending_symbol_load = false; }
                        if window.needs_snap_after_backfill && window.viewport.chart_width > 0.0 {
                            window.consume_cold_snap(&tpo_bubble_bars);
                        }
                    }
                }
                // Three Line Break line stream — each emit is one closed line.
                LiveUpdate::ThreeLineBreakUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    let at_label = at.short_label();
                    let target_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                        let panels_store = &self.panels_store;
                        self.panel_app.panel_grid.containers().iter()
                            .filter_map(|(cid, win)| {
                                let s = self.symbol_for_chart(*cid)?;
                                let a = self.account_type_for_chart(*cid)?;
                                let is_tlb = panels_store.chart_panel
                                    .get(&win.chart_panel_id)
                                    .map(|p| p.chart_type == "three_line_break")
                                    .unwrap_or(false);
                                if !is_tlb || s != sym.as_str() || a != at_label { return None; }
                                Some(*cid)
                            }).collect()
                    };
                    if target_cids.is_empty() { continue; }
                    let line = mlc_core::types::ThreeLineBreakLine {
                        ts_open: point.ts_open,
                        ts_close: point.ts_close,
                        price_open: point.open,
                        price_close: point.close,
                        volume: point.volume,
                        direction: point.direction,
                    };
                    let tlb_targets: std::collections::HashSet<mlc_core::tag_manager::BubbleId> =
                        target_cids.iter()
                            .filter_map(|cid| self.bubble_for_chart(*cid))
                            .collect();
                    for bid in &tlb_targets {
                        if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(*bid) {
                            bubble.append_three_line_break(line.clone());
                        }
                    }
                    let tlb_bubble_bars: Vec<mlc_core::Bar> = {
                        let mgr = self.core.bubble();
                        mgr.bubble.bubble_for_instrument(exchange_id.as_str(), at_label, sym.as_str())
                            .and_then(|bid| mgr.bubble.bubble(bid))
                            .map(|b| b.bars.clone())
                            .unwrap_or_default()
                    };
                    for (cid_, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                        if !target_cids.contains(cid_) { continue; }
                        if window.pending_symbol_load { window.pending_symbol_load = false; }
                        if window.needs_snap_after_backfill && window.viewport.chart_width > 0.0 {
                            window.consume_cold_snap(&tlb_bubble_bars);
                        }
                    }
                }
                // Open Interest Δ bar — append to all oi_show windows for the
                // matching symbol + account. Mirrors the CVD/TLB two-pass pattern.
                LiveUpdate::OpenInterestUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    let at_label = at.short_label();
                    let target_cids: std::collections::HashSet<mlc_core::state::ChartId> =
                        self.panel_app.panel_grid.containers().iter()
                            .filter_map(|(cid, win)| {
                                if !win.oi_show { return None; }
                                let s = self.symbol_for_chart(*cid)?;
                                let a = self.account_type_for_chart(*cid)?;
                                if s != sym.as_str() || a != at_label { return None; }
                                Some(*cid)
                            }).collect();
                    if target_cids.is_empty() { continue; }
                    let oi_targets: std::collections::HashSet<mlc_core::tag_manager::BubbleId> =
                        target_cids.iter()
                            .filter_map(|cid| self.bubble_for_chart(*cid))
                            .collect();
                    for bid in &oi_targets {
                        if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(*bid) {
                            bubble.append_oi(point);
                        }
                    }
                    // No window-side post-effects for OI (no cold-snap on overlay).
                    let _ = target_cids;
                }
                // Funding Rate cell — append to all funding_show windows for the
                // matching symbol + account. Same two-pass pattern as OI above.
                LiveUpdate::FundingRateUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    let at_label = at.short_label();
                    let target_cids: std::collections::HashSet<mlc_core::state::ChartId> =
                        self.panel_app.panel_grid.containers().iter()
                            .filter_map(|(cid, win)| {
                                if !win.funding_show { return None; }
                                let s = self.symbol_for_chart(*cid)?;
                                let a = self.account_type_for_chart(*cid)?;
                                if s != sym.as_str() || a != at_label { return None; }
                                Some(*cid)
                            }).collect();
                    if target_cids.is_empty() { continue; }
                    let funding_targets: std::collections::HashSet<mlc_core::tag_manager::BubbleId> =
                        target_cids.iter()
                            .filter_map(|cid| self.bubble_for_chart(*cid))
                            .collect();
                    for bid in &funding_targets {
                        if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(*bid) {
                            bubble.append_funding(point);
                        }
                    }
                    let _ = target_cids;
                }
                // DOM heatmap snapshot (C12) — push into dom_heatmap buffer on
                // all chart_type=="dom_heatmap" windows that match symbol+account.
                LiveUpdate::DomHeatmapSnapshot { exchange_id, account_type: at, symbol: sym, ts_ms, bids, asks } => {
                    let at_label = at.short_label();
                    let target_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                        let panels_store = &self.panels_store;
                        self.panel_app.panel_grid.containers().iter()
                            .filter_map(|(cid, win)| {
                                let s = self.symbol_for_chart(*cid)?;
                                let a = self.account_type_for_chart(*cid)?;
                                let is_dom = panels_store.chart_panel
                                    .get(&win.chart_panel_id)
                                    .map(|p| p.chart_type == "dom_heatmap")
                                    .unwrap_or(false);
                                if !is_dom || s != sym.as_str() || a != at_label { return None; }
                                Some(*cid)
                            }).collect()
                    };
                    if target_cids.is_empty() { continue; }
                    let dom_targets: std::collections::HashSet<mlc_core::tag_manager::BubbleId> =
                        target_cids.iter()
                            .filter_map(|cid| self.bubble_for_chart(*cid))
                            .collect();
                    for bid in &dom_targets {
                        if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(*bid) {
                            const DEFAULT_TICK: f64 = 0.5;
                            bubble.dom_heatmap_mut()
                                .get_or_insert_with(|| mlc_core::chart::dom_heatmap::DomHeatmapBuffer::new(DEFAULT_TICK))
                                .push_snapshot(ts_ms, &bids, &asks);
                        }
                    }
                    let dom_bubble_bars: Vec<mlc_core::Bar> = {
                        let mgr = self.core.bubble();
                        mgr.bubble.bubble_for_instrument(exchange_id.as_str(), at_label, sym.as_str())
                            .and_then(|bid| mgr.bubble.bubble(bid))
                            .map(|b| b.bars.clone())
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
                // DOM heatmap trade dot (C12) — append to dom_trade_dots ring.
                LiveUpdate::DomTradeDot { exchange_id, account_type: at, symbol: sym, dot } => {
                    let at_label = at.short_label();
                    let target_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                        let panels_store = &self.panels_store;
                        self.panel_app.panel_grid.containers().iter()
                            .filter_map(|(cid, win)| {
                                let s = self.symbol_for_chart(*cid)?;
                                let a = self.account_type_for_chart(*cid)?;
                                let is_dom = panels_store.chart_panel
                                    .get(&win.chart_panel_id)
                                    .map(|p| p.chart_type == "dom_heatmap")
                                    .unwrap_or(false);
                                if !is_dom || s != sym.as_str() || a != at_label { return None; }
                                Some(*cid)
                            }).collect()
                    };
                    if target_cids.is_empty() { continue; }
                    let domdot_targets: std::collections::HashSet<mlc_core::tag_manager::BubbleId> =
                        target_cids.iter()
                            .filter_map(|cid| self.bubble_for_chart(*cid))
                            .collect();
                    for bid in &domdot_targets {
                        if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(*bid) {
                            const MAX_DOTS: usize = 2_000;
                            let dots = bubble.dom_trade_dots.get_or_insert_with(Vec::new);
                            if dots.len() >= MAX_DOTS { dots.remove(0); }
                            dots.push(dot);
                        }
                    }
                    let _ = target_cids;
                }
                // Liquidation per-event (C13) — push into liq_heatmap buffer AND dot ring.
                LiveUpdate::LiquidationEvent { exchange_id, account_type: at, symbol: sym, ts_ms, price, value_usd, side } => {
                    let at_label = at.short_label();
                    let target_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                        let panels_store = &self.panels_store;
                        self.panel_app.panel_grid.containers().iter()
                            .filter_map(|(cid, win)| {
                                let s = self.symbol_for_chart(*cid)?;
                                let a = self.account_type_for_chart(*cid)?;
                                let is_liq = panels_store.chart_panel
                                    .get(&win.chart_panel_id)
                                    .map(|p| p.chart_type == "liquidation_heatmap")
                                    .unwrap_or(false);
                                if !is_liq || s != sym.as_str() || a != at_label { return None; }
                                Some(*cid)
                            }).collect()
                    };
                    if target_cids.is_empty() { continue; }
                    let liqev_targets: std::collections::HashSet<mlc_core::tag_manager::BubbleId> =
                        target_cids.iter()
                            .filter_map(|cid| self.bubble_for_chart(*cid))
                            .collect();
                    for bid in &liqev_targets {
                        if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(*bid) {
                            const DEFAULT_TICK: f64 = 0.5;
                            bubble.liq_heatmap_mut()
                                .get_or_insert_with(|| mlc_core::chart::liquidation_heatmap::LiquidationHeatmapBuffer::new(DEFAULT_TICK))
                                .push_liquidation(ts_ms, price, value_usd, side);
                            const MAX_DOTS: usize = 2_000;
                            let dots = bubble.liq_dots.get_or_insert_with(Vec::new);
                            if dots.len() >= MAX_DOTS { dots.remove(0); }
                            dots.push(mlc_core::chart::liquidation_heatmap::LiquidationDot {
                                ts_ms, price,
                                value_usd: if value_usd.is_finite() { value_usd } else { 0.0 },
                                side,
                            });
                        }
                    }
                    let liq_bubble_bars: Vec<mlc_core::Bar> = {
                        let mgr = self.core.bubble();
                        mgr.bubble.bubble_for_instrument(exchange_id.as_str(), at_label, sym.as_str())
                            .and_then(|bid| mgr.bubble.bubble(bid))
                            .map(|b| b.bars.clone())
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
                // Liquidation bucket (C13) — push aggregate into liq_heatmap strip.
                LiveUpdate::LiquidationBucketUpdate { exchange_id: ex_id, account_type: at, symbol: sym, ts_ms, long_liq_usd, short_liq_usd, .. } => {
                    let at_label = at.short_label();
                    let target_cids: std::collections::HashSet<mlc_core::state::ChartId> = {
                        let panels_store = &self.panels_store;
                        self.panel_app.panel_grid.containers().iter()
                            .filter_map(|(cid, win)| {
                                let s = self.symbol_for_chart(*cid)?;
                                let a = self.account_type_for_chart(*cid)?;
                                let is_liq = panels_store.chart_panel
                                    .get(&win.chart_panel_id)
                                    .map(|p| p.chart_type == "liquidation_heatmap")
                                    .unwrap_or(false);
                                if !is_liq || s != sym.as_str() || a != at_label { return None; }
                                Some(*cid)
                            }).collect()
                    };
                    if target_cids.is_empty() { continue; }
                    // Option A: spread bucket aggregate across ±5 ticks around mark price.
                    // Mark price sourced from mini_ticker_cache (last-trade/BBO price).
                    // Key format mirrors MiniTickerUpdate: "symbol:exchange:account_type".
                    let ticker_key = format!("{}:{}:{}", sym, ex_id.as_str(), at_label);
                    let mark_price_opt = self.core.mini_ticker
                        .get(&ticker_key)
                        .map(|t| t.last_price);
                    let liqbk_targets: std::collections::HashSet<mlc_core::tag_manager::BubbleId> =
                        target_cids.iter()
                            .filter_map(|cid| self.bubble_for_chart(*cid))
                            .collect();
                    for bid in &liqbk_targets {
                        if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(*bid) {
                            const DEFAULT_TICK: f64 = 0.5;
                            const SPREAD_HALF_TICKS: i64 = 5;
                            let mark_opt = mark_price_opt.or_else(|| bubble.bars.last().map(|b| b.close));
                            let buf = bubble.liq_heatmap_mut()
                                .get_or_insert_with(|| mlc_core::chart::liquidation_heatmap::LiquidationHeatmapBuffer::new(DEFAULT_TICK));
                            if let Some(mark) = mark_opt {
                                buf.push_bucket_spread(ts_ms, long_liq_usd, short_liq_usd, mark, SPREAD_HALF_TICKS);
                            }
                        }
                    }
                    let _ = target_cids;
                }
            }
        }
        self.last_event_process_us = events_start.elapsed().as_micros() as u64;

        // ── Coalesced orderbook panel re-aggregation ─────────────────────────
        // DOM + heatmap: tick once per frame with the merged level changes (they
        // track current book state, not history — per-event ts is irrelevant).
        // L2 tape: iterate the per-event list so each tape row carries its own
        // original wire timestamp instead of a single SystemTime::now() for the
        // whole batch.
        if !ob_dirty.is_empty() {
            let _ob_panel_start = std::time::Instant::now();
            for ((symbol, ex_str, at_str), entry) in &ob_dirty {
                // ── DOM: incremental or full-rebuild (bulk — level state only) ──
                // DOM shows current snapshot depth, not tape history. Passing the
                // last event's merged changes is correct; per-event ts not needed.
                let dom_t0 = std::time::Instant::now();
                // Flatten all events into a single delta slice for DOM (level state).
                let (bulk_bids, bulk_asks): (Vec<(f64, f64)>, Vec<(f64, f64)>) = {
                    let mut b: Vec<(f64, f64)> = Vec::new();
                    let mut a: Vec<(f64, f64)> = Vec::new();
                    for ev in &entry.events {
                        b.extend_from_slice(&ev.bids);
                        a.extend_from_slice(&ev.asks);
                    }
                    (b, a)
                };
                for state in self.panels_store.dom.values_mut() {
                    if &state.symbol == symbol && &state.exchange == ex_str && &state.account_type == at_str {
                        state.apply_ob_update(
                            &bulk_bids,
                            &bulk_asks,
                            entry.force_rebuild,
                        );
                    }
                }
                self.last_dom_panel_us += dom_t0.elapsed().as_micros() as u64;

                // ── L2 Tape: per-event calls with original wire ts ─────────────
                // Each ObDeltaEvent carries its own ts_ms from the WS message so
                // tape rows show real tick-level timing, not a frame-batch ts.
                let l2_t0 = std::time::Instant::now();
                for state in self.panels_store.l2_tape.values_mut() {
                    if &state.symbol == symbol && &state.exchange == ex_str && &state.account_type == at_str {
                        if entry.force_rebuild || entry.events.is_empty() {
                            // Full rebuild or snapshot-only frame: no delta events.
                            state.apply_ob_update(
                                &[],
                                &[],
                                entry.force_rebuild,
                            );
                        } else {
                            for ev in &entry.events {
                                state.apply_ob_update_with_ts(
                                    &ev.bids,
                                    &ev.asks,
                                    ev.ts_ms,
                                );
                            }
                        }
                        state.prune_flash();
                    }
                }
                self.last_l2_panel_us += l2_t0.elapsed().as_micros() as u64;

                // ── Heatmap: view-based tick (rate-limited, untouched path) ───
                let heat_t0 = std::time::Instant::now();
                for state in self.panels_store.liquidity_heatmap.values_mut() {
                    if &state.symbol == symbol && &state.exchange == ex_str && &state.account_type == at_str {
                        state.tick();
                    }
                }
                self.last_heatmap_panel_us += heat_t0.elapsed().as_micros() as u64;
            }
            self.last_orderbook_panel_us += _ob_panel_start.elapsed().as_micros() as u64;
        }

        if !trading_updates.is_empty() {
            if let Some(mut tm) = self.core.trading_lock() {
                tm.tick(&trading_updates);
            }
            for state in self.panels_store.order_entry.values_mut() {
                state.sync_from_snapshot();
            }
            for state in self.panels_store.position_manager.values_mut() {
                state.sync_from_snapshot();
            }
            for state in self.panels_store.trade_log.values_mut() {
                state.sync_from_snapshot();
            }
        }

        // ── Alert checker: detect price crossings for every visible symbol ────
        // Skip entirely when no trade arrived this tick — nothing changed.
        if had_trade_update {
            // Collect one entry per unique (symbol, exchange, account_type) triple across all windows.
            // Multiple windows on the same triple share the same bar data, so one check
            // per triple is sufficient.
            let mut seen_pairs: std::collections::HashSet<(String, String, String)> = std::collections::HashSet::new();
            struct WindowAlertData {
                symbol: String,
                exchange: String,
                account_type: String,
                current_price: f64,
                current_bar: f64,
                drawing_points: Vec<(u64, Vec<(f64, f64)>, mlc_alerts::DrawingExtendMode)>,
            }
            let window_data: Vec<WindowAlertData> = self.panel_app.panel_grid.containers()
                .iter()
                .filter_map(|(cid, window)| {
                    let sym = self.symbol_for_chart(*cid).unwrap_or_default();
                    let exch = self.exchange_for_chart(*cid).unwrap_or_default();
                    let at = self.account_type_for_chart(*cid).unwrap_or_default();
                    let triple = (sym.clone(), exch.clone(), at.clone());
                    if seen_pairs.contains(&triple) {
                        return None;
                    }
                    seen_pairs.insert(triple);
                    let bubble_bars_alert: Vec<mlc_core::Bar> = self.with_bubble(window.current_bubble_id, |b| b.bars.clone()).unwrap_or_default();
                    let current_price = bubble_bars_alert.last().map(|b| b.close).unwrap_or(0.0);
                    let current_bar = bubble_bars_alert.len().saturating_sub(1) as f64;
                    let drawing_points: Vec<(u64, Vec<(f64, f64)>, mlc_alerts::DrawingExtendMode)> =
                        self.panels_store.chart_panel.get(&window.chart_panel_id)
                            .map(|panel| panel.drawing_manager.primitives().iter()
                                .map(|p| {
                                    let pts_bar: Vec<(f64, f64)> = p.points()
                                        .into_iter()
                                        .map(|(ts_ms, price)| (timestamp_ms_to_bar_f64(&bubble_bars_alert, ts_ms), price))
                                        .collect();
                                    (p.data().id, pts_bar, mlc_alerts::DrawingExtendMode::from_u8(p.extend_mode_raw()))
                                })
                                .collect())
                            .unwrap_or_default();
                    Some(WindowAlertData {
                        symbol: sym,
                        exchange: exch,
                        account_type: at,
                        current_price,
                        current_bar,
                        drawing_points,
                    })
                })
                .collect();

            let indicator_values = Self::build_indicator_values_for_alerts(
                &self.core.bubble().alert,
                &self.core.indicators(),
            );

            let mut all_triggered_ids: Vec<u64> = Vec::new();
            for wd in &window_data {
                let triggered = self.core.bubble_mut().alert.check_crossings_dynamic(
                    wd.current_price,
                    wd.current_bar,
                    &wd.symbol,
                    &wd.exchange,
                    &wd.account_type,
                    &wd.drawing_points,
                    &indicator_values,
                );
                all_triggered_ids.extend(triggered);
            }

            // ── Signal alert checker ───────────────────────────────────────────────
            // Gather all signals from all indicator instances, then check signal alerts
            // per window (symbol/exchange/account_type context).
            {
                use mlc_indicators::signals::signal::BarConfirmation;

                let signal_batch: Vec<(u64, usize, i8, u8, String)> = self.core.indicators().instances_iter()
                    .flat_map(|inst| {
                        let ind_id = inst.id;
                        inst.signals.iter().map(move |s| {
                            let conf_u8 = match s.confirmation {
                                BarConfirmation::Pending => 0u8,
                                BarConfirmation::Closed => 1u8,
                                BarConfirmation::WickOnly => 2u8,
                            };
                            (ind_id, s.bar_index, s.direction.as_i8(), conf_u8, s.kind.description().to_string())
                        })
                    })
                    .collect();

                for wd in &window_data {
                    let triggered = self.core.bubble_mut().alert.check_signal_alerts(
                        &wd.symbol,
                        &wd.exchange,
                        &wd.account_type,
                        &signal_batch,
                    );
                    all_triggered_ids.extend(triggered);
                }
            }

            // Deduplicate in case the same alert matched multiple windows.
            all_triggered_ids.sort_unstable();
            all_triggered_ids.dedup();
            let triggered_ids = all_triggered_ids;

            // Use the active window's price for delivery event messages.
            let current_price = self.panel_app.panel_grid.active_container()
                .map(|w| w.current_bubble_id)
                .and_then(|bid| self.with_bubble(bid, |b| b.bars.last().map(|bar| bar.close)).flatten())
                .unwrap_or(0.0);

            // Build delivery events for triggered alerts.
            if !triggered_ids.is_empty() {
                let symbol = self.panel_app.panel_grid.active_container_id()
                    .and_then(|cid| self.symbol_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_default();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);

                for id in &triggered_ids {
                    if let Some(alert) = self.core.bubble().alert.get(*id) {
                        self.pending_delivery_events.push(mlc_alert_delivery::DeliveryEvent {
                            alert_name: alert.name.clone(),
                            symbol: symbol.clone(),
                            message: format!("{} {} @ {:.8}",
                                alert.source.display_name(),
                                alert.condition.display_name(),
                                current_price),
                            price: current_price,
                            timestamp: now,
                            screenshot: None,
                        });
                    }
                }

                // Request a screenshot capture from the render layer.
                // The renderer will attach PNG bytes to all pending_delivery_events
                // before they are drained and dispatched.
                self.pending_alert_screenshot = true;

                // Sidebar alert list needs to reflect the new Triggered status.
                self.sidebar_data_dirty = true;
            }
        }

        // ── Deferred indicator recalculation flush (PerFrame / PerBar) ────────
        // For PerTick mode drain_pending_recalc returns an empty Vec, so this
        // block is a no-op — PerTick was already handled inline above.
        let pending = self.core.indicators_mut().drain_pending_recalc();
        if !pending.is_empty() {
            let _deferred_start = std::time::Instant::now();
            for symbol in &pending {
                // Collect only the ChartId values (cheap u64 copies) for every
                // window showing this symbol.  Each window may have different
                // bars (different timeframes), so indicator instances are
                // per-window.  Bars are borrowed by reference below — no clone.
                let matching_ids: Vec<u64> = self
                    .panel_app
                    .panel_grid
                    .containers()
                    .keys()
                    .filter(|cid| self.symbol_for_chart(**cid)
                        .map_or(false, |s| s == *symbol))
                    .map(|cid| cid.0)
                    .collect();

                // Split-borrow: `panel_app.panel_grid` and `indicator_manager`
                // are distinct struct fields, so both can be used simultaneously.
                for window_id in matching_ids {
                    let chart_id = ChartId(window_id);
                    let bid_opt = self.panel_app.panel_grid.containers().get(&chart_id).map(|w| w.current_bubble_id);
                    if let Some(bid) = bid_opt {
                        let bars_recalc: Vec<mlc_core::Bar> = self.with_bubble(bid, |b| b.bars.clone()).unwrap_or_default();
                        self.core.indicators_mut().calculate_for_window(window_id, &bars_recalc);
                    }
                }
                let _ = symbol; // legacy — `calculate_for_window` filters by window_id, no longer by symbol
                // Count one recalc per symbol (regardless of window count).
                self.recalc_count += 1;
            }
            self.last_indicator_recalc_us += _deferred_start.elapsed().as_micros() as u64;
            self.sync_sub_panes_from_manager();
        }

        // ── Periodic RecalcMode diagnostic log (every 5 seconds) ─────────────
        if self.diagnostics_enabled
            && self.recalc_log_timer.elapsed() >= std::time::Duration::from_secs(5)
        {
            let mode = match self.core.indicators().recalc_mode {
                RecalcMode::PerTick => "PerTick",
                RecalcMode::PerFrame => "PerFrame",
                RecalcMode::PerBar => "PerBar",
            };
            crate::dlog!(
                "[ChartApp] RecalcMode={} | trades={} recalcs={} in 5s",
                mode, self.trade_count, self.recalc_count
            );
            self.trade_count = 0;
            self.recalc_count = 0;
            self.recalc_log_timer = std::time::Instant::now();
        }

        // ── Layer 3: trigger scroll-fetch when viewport approaches left edge ───
        {
            let mut scroll_requests: Vec<(mlc_core::state::ChartId, String, String, String, String)> = Vec::new();
            let max_loaded = self.panel_app.user_manager.profile.data_load.max_loaded_bars;

            // Phase 4 B: pre-collect instrument map from branch_meta.
            let instrument_by_cid_scroll: std::collections::HashMap<mlc_core::state::ChartId, (String, mlc_core::state::Timeframe, String, String)> = {
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

            // Snapshot per-chart bar counts up-front; the scroll loop needs to
            // know whether bars exist (was `w.bars.is_empty()`) and how many
            // bars are currently loaded (was `w.bars.len() >= max_loaded`).
            let bar_counts_by_chart: std::collections::HashMap<mlc_core::state::ChartId, usize> =
                self.panel_app.panel_grid.containers().iter()
                    .map(|(cid, w)| {
                        let n = self.core.bubble().bubble.bubble(w.current_bubble_id)
                            .map(|b| b.bars.len())
                            .unwrap_or(0);
                        (*cid, n)
                    })
                    .collect();
            for (cid_scroll, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                if window.scroll_fetch_in_flight {
                    if let Some(started) = window.scroll_fetch_started {
                        if started.elapsed() > std::time::Duration::from_secs(10) {
                            crate::dlog!("[ChartApp] scroll_fetch_in_flight timeout, resetting for chart {:?}", cid_scroll);
                            window.scroll_fetch_in_flight = false;
                            window.scroll_fetch_started = None;
                        }
                    }
                    if window.scroll_fetch_in_flight { continue; }
                }
                let bar_count_scroll = bar_counts_by_chart.get(cid_scroll).copied().unwrap_or(0);
                if bar_count_scroll == 0 { continue; }
                if window.pending_symbol_load { continue; }

                let visible = window.viewport.visible_bars() as f64;
                let threshold = (visible * 0.20).max(5.0);
                if window.viewport.view_start > threshold { continue; }

                if max_loaded > 0 && bar_count_scroll >= max_loaded as usize { continue; }

                let (win_sym_sc, win_tf_sc, win_exch_sc, win_at_sc) = match instrument_by_cid_scroll.get(cid_scroll) {
                    Some(t) => t,
                    None => continue,
                };

                window.scroll_fetch_in_flight = true;
                window.scroll_fetch_started = Some(std::time::Instant::now());
                scroll_requests.push((
                    *cid_scroll,
                    win_sym_sc.clone(),
                    win_exch_sc.clone(),
                    win_tf_sc.name.clone(),
                    win_at_sc.clone(),
                ));
            }

            for (cid_req, symbol, exchange, tf_name, at_label) in scroll_requests {
                // Read oldest_ts from the window's bars directly.
                let oldest_ts = self.panel_app.panel_grid.containers()
                    .get(&cid_req)
                    .and_then(|w| None::<&mlc_core::Bar>.map(|b| b.timestamp))
                    .unwrap_or(0);

                if oldest_ts == 0 { continue; }

                let eid = digdigdig3::ExchangeId::from_str(&exchange)
                    .unwrap_or(digdigdig3::ExchangeId::Binance);
                let at = account_type_from_label(&at_label);

                if let Some(tf) = parse_timeframe_name(&tf_name) {
                    self.core.bridge.request_scroll_bars(eid, &symbol, &tf, at, oldest_ts, 500);
                }
            }
        }

        if self.agent.drain_events() {
            self.sidebar_data_dirty = true;
        }
        // Sync pipe_session_id from gate4agent into sidebar descriptors for persistence.
        {
            let updates: Vec<(uzor::panels::LeafId, Option<String>)> = self
                .sidebar_state
                .agent_leaves
                .iter()
                .filter(|(_, desc)| desc.mode == gate4agent::InstanceMode::Chat)
                .filter_map(|(&leaf_id, desc)| {
                    self.agent
                        .snapshot_instance(desc.instance_id)
                        .and_then(|snap| {
                            if snap.pipe_session_id != desc.chat_session_id {
                                Some((leaf_id, snap.pipe_session_id))
                            } else {
                                None
                            }
                        })
                })
                .collect();
            for (leaf_id, session_id) in updates {
                if let Some(desc) = self.sidebar_state.agent_leaves.get_mut(&leaf_id) {
                    desc.chat_session_id = session_id;
                }
                self.profile_dirty = true;
            }
        }

        // Drain a pending autosave once per tick — collapses the 5-7
        // autosave_snapshot calls fired during a single preset switch
        // (sync-group propagation + park + load + command history all
        // ask for one in the same frame) into a single deep collect_state.
        self.flush_pending_autosave();

        self.last_tick_us = tick_start.elapsed().as_micros() as u64;

        // ── Leak / backpressure census ───────────────────────────────────────
        // Snapshot growable structures so the [PERF] line can show whether they
        // grow unbounded over time (the suspected "leak" / load-storm).
        self.diag_queue_len = self.live_update_rx.len();
        self.diag_mini_ticker = self.core.mini_ticker.len();

        // ── Tick spike log ───────────────────────────────────────────────────
        // Print a one-line breakdown ONLY when this tick blew past budget, so we
        // can attribute random multi-ms/multi-second tick explosions to the exact
        // events that flooded the queue this frame. Gated by MLC_PERF_LOG so it's
        // silent in normal runs.
        if self.last_tick_us > 3000 && std::env::var("MLC_PERF_LOG").is_ok() {
            eprintln!(
                "[TICK-SPIKE] tick={}us events={}us recalc={}us autoscale={}us obpanel={}us(n={}) dom={}us l2={}us heat={}us trpanel={}us(n={}) barapply={}us | drained={} (bars={} backfill={} scroll={} trade={} ticker={} conn={} other={}) lag_events={}",
                self.last_tick_us,
                self.last_event_process_us,
                self.last_indicator_recalc_us,
                self.last_auto_scale_us,
                self.last_orderbook_panel_us,
                self.last_ob_event_count,
                self.last_dom_panel_us,
                self.last_l2_panel_us,
                self.last_heatmap_panel_us,
                self.last_trade_panel_us,
                self.last_trade_event_count,
                self.last_bar_apply_us,
                _drain_count, n_bars, n_backfill, n_scroll, n_trade, n_ticker, n_connector, n_other,
                self.lag_event_count,
            );
        }

        // ── Startup-wave log (connector / symbol storm) ──────────────────────
        // Independent of the tick-budget gate above: a tick can drain a huge
        // connect/symbol wave WITHOUT itself exceeding 3ms (the cost is the
        // async REST work off-thread; the main-thread stall is the loop being
        // starved BETWEEN frames while connect IO completes). So whenever this
        // tick saw ANY connector-ready or symbols-loaded event, print a line
        // tagging exactly how much of the startup wave landed here and what it
        // cost on the main thread. Correlate the timestamps across ticks to see
        // the wave colliding with the skeleton→live promote.
        if (n_connector > 0 || n_symbols > 0) && std::env::var("MLC_PERF_LOG").is_ok() {
            eprintln!(
                "[STARTUP-WAVE] conn_ready={} symbols_loaded={}(total_syms={} insert={}us) tick={}us events={}us drained={} queue_depth={} | skeleton_window={}",
                n_connector, n_symbols, n_symbols_total, symbols_insert_us,
                self.last_tick_us, self.last_event_process_us, _drain_count,
                self.live_update_rx.len(),
                self.panel_app.panel_grid.containers().is_empty(),
            );
        }

        // ── Indicator heal: one-shot recalc for never-computed instances ────
        // Bars can reach a window through paths that never emit BarsLoaded
        // (Phase A cache-fresh skip returns without sending; preset restore
        // serves bars synchronously from the data_provider cache). Restored
        // instances then sit with empty `values` until something else forces
        // a recalc (the owner-visible bug: indicators absent after reload,
        // appearing only after a new indicator is added). Heal: when a window
        // HAS bars and hosts instances that were never computed, run one
        // calculate_for_window. One attempt per instance per session — an
        // indicator that legitimately yields no values doesn't retrigger.
        // Cheap pre-check WITHOUT cloning bars: only enter the heal path when
        // some window actually hosts a never-computed instance. The previous
        // version cloned every window's bar vec on every single tick just to
        // discover there was nothing to heal — pure waste on the steady state
        // and part of the first-live-frame cost.
        let any_unhealed = {
            let attempted = &self.indicator_heal_attempted;
            let im = self.core.indicators();
            self.panel_app.panel_grid.iter_containers().any(|(_, w)| {
                !true
                    && im.get_instances_for_window(w.id.0)
                        .iter()
                        .any(|i| i.values.is_empty() && !attempted.contains(&i.id))
            })
        };
        if any_unhealed {
            let window_bars: Vec<(u64, Vec<mlc_core::Bar>)> = self
                .panel_app
                .panel_grid
                .iter_containers()
                .filter(|(_, w)| !true)
                .map(|(_, w)| (w.id.0, Vec::<mlc_core::Bar>::new()))
                .collect();
            for (wid, bars) in window_bars {
                let needs_heal: Vec<u64> = self.core.indicators().get_instances_for_window(wid)
                    .iter()
                    .filter(|i| i.values.is_empty() && !self.indicator_heal_attempted.contains(&i.id))
                    .map(|i| i.id)
                    .collect();
                if !needs_heal.is_empty() {
                    for id in &needs_heal {
                        self.indicator_heal_attempted.insert(*id);
                    }
                    eprintln!(
                        "[ChartApp] indicator heal: window {} has {} bars, {} never-computed instance(s) {:?} — recalculating",
                        wid, bars.len(), needs_heal.len(), needs_heal
                    );
                    self.core.indicators_mut().calculate_for_window(wid, &bars);
                }
            }
        }
    }
}
