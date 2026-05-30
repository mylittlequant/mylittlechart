//! Live update tick loop: drains broadcast channels (bars, trades, mini-ticker,
//! connector status), processes alert checks, and updates per-frame state.

use crate::ChartApp;
use crate::{account_type_from_label, parse_timeframe_name};
use live_data::LiveUpdate;
use zengeld_chart::{ChartId, ScaleMode, timestamp_ms_to_bar_f64};
use zengeld_terminal_indicators::RecalcMode;

impl ChartApp {
    /// Called every frame with the current wall-clock time in milliseconds.
    ///
    /// Drains the async `LiveUpdate` channel (bar loads, WebSocket bar updates,
    /// connector-ready events) and runs the alert crossing checker.
    pub fn tick(&mut self, current_time_ms: u64, bar_svc: &mut bar_service::BarService) {
        let _ = current_time_ms;
        let tick_start = std::time::Instant::now();

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
        let mut trading_updates: Vec<LiveUpdate> = Vec::new();
        let events_start = std::time::Instant::now();
        loop {
            let update = match self.live_update_rx.try_recv() {
                Ok(u) => { _drain_count += 1; u },
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                    self.lag_event_count += 1;
                    eprintln!("[ChartApp:{}] broadcast LAGGED — skipped {} messages (total lag events: {})",
                        self.panel_app.panel_grid.windows().values().next()
                            .map(|w| w.symbol.as_str()).unwrap_or("?"),
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
            {
                use sidebar_content::state::RightSidebarPanel;
                match self.sidebar_state.right_panel {
                    RightSidebarPanel::Watchlist | RightSidebarPanel::Connectors => {
                        self.sidebar_data_dirty = true;
                    }
                    _ => {}
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
                _ => n_other += 1,
            }
            match update {
                LiveUpdate::BarsLoaded { exchange_id, symbol, timeframe: tf_name, bars, account_type } => {
                    let loaded_tf = parse_timeframe_name(&tf_name);
                    eprintln!("[ChartApp] BarsLoaded: {:?} {} tf={} bars={} first_ts={} last_ts={}",
                        exchange_id, symbol, tf_name, bars.len(),
                        bars.first().map(|b| b.timestamp).unwrap_or(0),
                        bars.last().map(|b| b.timestamp).unwrap_or(0));

                    // Obtain/update TrackedSeriesHandle for matched windows.
                    {
                        let period_secs = loaded_tf.as_ref().map_or(60, |tf| tf.minutes as i64) * 60;
                        let bs_key = bar_service::BarSeriesKey::new(exchange_id, account_type, symbol.clone(), tf_name.clone());
                        let matched_cids: Vec<u64> = self.panel_app.panel_grid.windows().iter()
                            .filter(|(_cid, window)| {
                                window.symbol == symbol
                                    && window.exchange == exchange_id.as_str()
                                    && window.timeframe.name == tf_name
                                    && window.account_type == account_type.short_label()
                            })
                            .map(|(cid, _window)| cid.0)
                            .collect();
                        for cid_val in matched_cids {
                            let handle_key = (cid_val, bs_key.clone());
                            self.series_handles.entry(handle_key).or_insert_with(|| {
                                let arc = bar_svc.get_or_create(bs_key.clone(), period_secs);
                                bar_service::TrackedSeriesHandle::new(arc)
                            });
                        }
                    }

                    let mut any_matched = false;
                    // Collect (symbol, timeframe, account_type) for windows that received
                    // an initial load so we can trigger background backfill after the loop
                    // (can't borrow self.bridge while windows_mut() is held).
                    let mut backfill_requests: Vec<(String, zengeld_chart::state::Timeframe, digdigdig3::AccountType)> = Vec::new();
                    for window in self.panel_app.panel_grid.windows_mut().values_mut() {
                        let tf_matches = window.timeframe.name == tf_name;
                        let matched = window.symbol == symbol
                            && window.exchange == exchange_id.as_str()
                            && tf_matches
                            && window.account_type == account_type.short_label();
                        if matched {
                            any_matched = true;
                            // Use update_bars for backfill (preserves viewport),
                            // set_bars for initial load (resets viewport to end).
                            // pending_symbol_load forces the initial-load path even if a
                            // stray TradeUpdate inserted a synthetic bar before bars arrived.
                            let is_backfill = if window.pending_symbol_load {
                                false // force initial-load path, ignore any stray bars
                            } else {
                                !window.bars.is_empty()
                            };
                            eprintln!("[ChartApp]   -> window matched: sym={} exch={} tf={} is_backfill={} bars_len={} pending_sym={}",
                                window.symbol, window.exchange, window.timeframe.name,
                                is_backfill, window.bars.len(), window.pending_symbol_load);
                            if is_backfill {
                                window.update_bars(bars.clone());
                                // Also schedule backfill for cached windows that haven't
                                // reached the target bar count yet.
                                let target = self.panel_app.user_manager.profile.data_load.background_bar_count;
                                if target > 0 && (window.bars.len() as u32) < target {
                                    backfill_requests.push((
                                        window.symbol.clone(),
                                        window.timeframe.clone(),
                                        account_type_from_label(&window.account_type),
                                    ));
                                }
                            } else {
                                // Apply scale mode BEFORE set_bars so calc_auto_scale()
                                // runs with the correct mode (not stale Manual from previous symbol).
                                window.price_scale.scale_mode = self.default_scale_mode;
                                window.set_bars(bars.clone());
                                window.pending_symbol_load = false;
                                // Schedule a Layer 2 background backfill to extend history
                                // beyond the initial 300 bars.
                                let target = self.panel_app.user_manager.profile.data_load.background_bar_count;
                                if target > 300 {
                                    backfill_requests.push((
                                        window.symbol.clone(),
                                        window.timeframe.clone(),
                                        account_type_from_label(&window.account_type),
                                    ));
                                }
                            }
                            eprintln!("[BarsLoaded] after set_bars: view_start={} chart_width={} bar_spacing={}",
                                window.viewport.view_start, window.viewport.chart_width, window.viewport.bar_spacing);
                        }
                    }

                    // Trigger Layer 2 background backfill for initial loads.
                    // Done after the window loop to avoid borrow conflicts with self.bridge.
                    let bg_target = self.panel_app.user_manager.profile.data_load.background_bar_count;
                    for (sym, tf, at) in backfill_requests {
                        eprintln!("[ChartApp] Scheduling background backfill: {} {} tf={} target={}", exchange_id.as_str(), sym, tf.name, bg_target);
                        self.bridge.request_background_backfill(exchange_id, &sym, &tf, at, bg_target);
                    }

                    // Populate data_provider cache so future LoadPreset calls
                    // can serve bars synchronously via get_bars().
                    if any_matched {
                        if let Some(w) = self.panel_app.panel_grid.windows().values()
                            .find(|w| w.symbol == symbol && w.exchange == exchange_id.as_str() && w.timeframe.name == tf_name && w.account_type == account_type.short_label())
                        {
                            w.data_provider.insert_bars(&symbol, &tf_name, bars.clone());
                        }
                    }

                    // Recalculate indicators only for windows that match this
                    // BarsLoaded event (symbol + exchange + timeframe).  Using
                    // calculate_for_window instead of calculate_all_for_symbol
                    // prevents leaking bars from one TF into another window's
                    // indicators.
                    let matched_ids: Vec<(u64, Vec<zengeld_chart::Bar>)> = self
                        .panel_app
                        .panel_grid
                        .windows()
                        .iter()
                        .filter(|(_, w)| {
                            w.symbol == symbol
                                && w.exchange == exchange_id.as_str()
                                && w.timeframe.name == tf_name
                                && w.account_type == account_type.short_label()
                        })
                        .map(|(cid, w)| (cid.0, w.bars.clone()))
                        .collect();
                    for (wid, bars_for_window) in &matched_ids {
                        self.indicator_manager.calculate_for_window(*wid, bars_for_window);
                    }

                    // Only autosave and subscribe trades if at least one window matched.
                    if any_matched {
                        // Auto-subscribe to WebSocket trade stream for live updates after bars load.
                        if self.sidebar_state.connector_enabled.get(exchange_id.as_str()).copied().unwrap_or(true) {
                            self.bridge.subscribe_trades(exchange_id, &symbol, account_type);
                        }

                        // Bars are kept in-memory (window.bars) for tab-switch UX.
                        // No disk write or sync needed — bars are re-fetchable cache.
                    }
                }
                LiveUpdate::BackfillComplete { exchange_id, account_type, symbol, timeframe: tf_name, bars } => {
                    eprintln!("[ChartApp] BackfillComplete: {} {} tf={} bars={}", exchange_id.as_str(), symbol, tf_name, bars.len());
                    for window in self.panel_app.panel_grid.windows_mut().values_mut() {
                        let tf_matches = window.timeframe.name == tf_name;
                        if !(window.symbol == symbol
                            && window.exchange == exchange_id.as_str()
                            && tf_matches
                            && window.account_type == account_type.short_label())
                        {
                            continue;
                        }
                        // Backfill always uses update_bars — viewport is never reset.
                        window.update_bars(bars.clone());
                        if window.price_scale.scale_mode.is_auto_y() {
                            window.calc_auto_scale();
                        }
                    }

                    // Recalculate indicators for matched windows.
                    let matched_ids: Vec<(u64, Vec<zengeld_chart::Bar>)> = self
                        .panel_app
                        .panel_grid
                        .windows()
                        .iter()
                        .filter(|(_, w)| {
                            w.symbol == symbol
                                && w.exchange == exchange_id.as_str()
                                && w.timeframe.name == tf_name
                                && w.account_type == account_type.short_label()
                        })
                        .map(|(cid, w)| (cid.0, w.bars.clone()))
                        .collect();
                    for (wid, bars_for_window) in &matched_ids {
                        self.indicator_manager.calculate_for_window(*wid, bars_for_window);
                    }
                    // Backfill wrote new bars into the bridge cache — mark for disk flush.
                    self.bars_cache_dirty = true;
                }
                LiveUpdate::ScrollBarsLoaded { exchange_id, account_type, symbol, timeframe: tf_name, bars, prepend_count } => {
                    eprintln!("[ChartApp] ScrollBarsLoaded: {} {} tf={} bars={} prepend={}",
                        exchange_id.as_str(), symbol, tf_name, bars.len(), prepend_count);

                    let at_label = account_type.short_label();
                    let mut any_matched = false;

                    for window in self.panel_app.panel_grid.windows_mut().values_mut() {
                        let tf_matches = window.timeframe.name == tf_name;
                        if !(window.symbol == symbol
                            && window.exchange == exchange_id.as_str()
                            && tf_matches
                            && window.account_type == at_label)
                        {
                            continue;
                        }

                        any_matched = true;
                        window.scroll_fetch_in_flight = false;
                        window.scroll_fetch_started = None;

                        // Viewport shift: prepending N bars pushes all existing indices up by N.
                        window.viewport.view_start += prepend_count as f64;

                        // Replace bars with the full merged set from the bridge.
                        window.update_bars(bars.clone());

                        // Enforce max_loaded_bars: evict oldest bars if over limit.
                        let max = self.panel_app.user_manager.profile.data_load.max_loaded_bars as usize;
                        if max > 0 && window.bars.len() > max {
                            let excess = window.bars.len() - max;
                            window.bars.drain(..excess);
                            window.viewport.view_start = (window.viewport.view_start - excess as f64).max(0.0);
                            window.viewport.bar_count = window.bars.len();
                        }

                        if window.price_scale.scale_mode.is_auto_y() {
                            window.calc_auto_scale();
                        }
                    }

                    if any_matched {
                        let matched_ids: Vec<(u64, Vec<zengeld_chart::Bar>)> = self
                            .panel_app
                            .panel_grid
                            .windows()
                            .iter()
                            .filter(|(_, w)| {
                                w.symbol == symbol
                                    && w.exchange == exchange_id.as_str()
                                    && w.timeframe.name == tf_name
                            })
                            .map(|(cid, w)| (cid.0, w.bars.clone()))
                            .collect();
                        for (wid, bars_for_window) in &matched_ids {
                            self.indicator_manager.calculate_for_window(*wid, bars_for_window);
                        }
                    }
                    // Scroll-load wrote new bars into the bridge cache — mark for disk flush.
                    if any_matched {
                        self.bars_cache_dirty = true;
                    }
                }
                LiveUpdate::BarUpdate { .. } => {
                    // BarUpdate is superseded by TradeUpdate — no-op.
                }
                LiveUpdate::TradeUpdate { exchange_id, symbol, price, quantity, timestamp, account_type, is_buyer_maker } => {
                    self.trade_count += 1;
                    had_trade_update = true;
                    // Track whether any window formed a new bar for this symbol.
                    let mut is_new_bar = false;
                    // Track whether a multi-bar gap was detected (needs REST backfill).
                    let mut needs_backfill = false;

                    // Feed trade into BarService for each active timeframe.
                    {
                        let mut seen_tfs: Vec<String> = Vec::new();
                        for window in self.panel_app.panel_grid.windows().values() {
                            if window.pending_symbol_load { continue; }
                            if window.symbol == symbol && window.account_type == account_type.short_label() {
                                let tf_name = &window.timeframe.name;
                                if !seen_tfs.contains(tf_name) {
                                    seen_tfs.push(tf_name.clone());
                                    let key = bar_service::BarSeriesKey::new(exchange_id, account_type, symbol.clone(), tf_name.clone());
                                    bar_svc.apply_trade(&key, price, quantity, timestamp);
                                }
                            }
                        }
                    }

                    // Update the last bar of every window matching this symbol.
                    for window in self.panel_app.panel_grid.windows_mut().values_mut() {
                        if window.pending_symbol_load {
                            // Skip trade updates while waiting for initial bars —
                            // otherwise a stray bar inserted here would cause BarsLoaded
                            // to treat the load as a backfill and skip viewport repositioning.
                            continue;
                        }
                        if window.symbol == symbol && window.account_type == account_type.short_label() {
                            // Period in seconds derived from minutes field of Timeframe.
                            let period_secs = (window.timeframe.minutes as i64) * 60;
                            let trade_ts_secs = timestamp / 1000;

                            if let Some(last_ts) = window.bars.last().map(|b| b.timestamp) {
                                let candle_end = last_ts + period_secs;

                                if trade_ts_secs >= candle_end {
                                    // Detect multi-bar gap (>1 bar skipped → need REST backfill).
                                    if trade_ts_secs >= candle_end + period_secs {
                                        needs_backfill = true;
                                    }
                                    // Trade belongs to a new candle — push a fresh bar.
                                    let new_candle_start = (trade_ts_secs / period_secs) * period_secs;
                                    window.bars.push(zengeld_chart::Bar {
                                        timestamp: new_candle_start,
                                        open: price,
                                        high: price,
                                        low: price,
                                        close: price,
                                        volume: quantity,
                                    });
                                    is_new_bar = true;
                                } else if let Some(last) = window.bars.last_mut() {
                                    // Same candle — update OHLCV in-place.
                                    last.close = price;
                                    if price > last.high { last.high = price; }
                                    if price < last.low { last.low = price; }
                                    last.volume += quantity;
                                }
                            } else {
                                // No bars yet — create first bar from trade.
                                let candle_start = (trade_ts_secs / period_secs) * period_secs;
                                window.bars.push(zengeld_chart::Bar {
                                    timestamp: candle_start,
                                    open: price,
                                    high: price,
                                    low: price,
                                    close: price,
                                    volume: quantity,
                                });
                                is_new_bar = true;
                            }

                            // Update bar count.
                            window.viewport.bar_count = window.bars.len();

                            // Auto-scale if enabled.
                            if window.price_scale.scale_mode.is_auto_y() {
                                let _as_start = std::time::Instant::now();
                                window.calc_auto_scale();
                                self.last_auto_scale_us += _as_start.elapsed().as_micros() as u64;
                            }

                            let count = window.bars.len();
                            let visible_f = window.viewport.chart_width / window.viewport.bar_spacing;

                            // Follow mode: keep last bar visible with standard margin.
                            if window.price_scale.scale_mode.is_follow() {
                                window.snap_to_end(zengeld_chart::DEFAULT_SNAP_MARGIN);
                            }

                            // Auto mode guard: if a new bar appeared and it would
                            // be off-screen or at the very edge, nudge viewport by
                            // exactly 1 bar so it stays visible.  No margin — A-mode
                            // just keeps the last bar in view without adding space.
                            // If the user scrolled far away, don't disturb.
                            if is_new_bar && window.price_scale.scale_mode == ScaleMode::Auto {
                                let right_edge_bar = window.viewport.view_start + visible_f;
                                let last_bar = count as f64; // one past last bar index
                                // Nudge if the new bar is at or beyond the right edge
                                if last_bar >= right_edge_bar {
                                    window.viewport.view_start += 1.0;
                                }
                            }
                            // Manual mode: no viewport adjustments.

                        }
                    }

                    // Multi-bar gap detected — trigger REST backfill to fill missing candles.
                    if needs_backfill {
                        eprintln!("[ChartApp] Multi-bar gap detected for {} — requesting REST backfill", symbol);
                        let bridge = self.bridge.clone();
                        for window in self.panel_app.panel_grid.windows().values() {
                            if window.symbol == symbol && window.account_type == account_type.short_label() {
                                let at = account_type_from_label(&window.account_type);
                                bridge.request_bars(exchange_id, &window.symbol, &window.timeframe, at, None, None, false);
                            }
                        }
                    }

                    // Write trade into the shared TradeSeries ring so that
                    // BigTrades (and future panels) can pull from it via tick().
                    {
                        let trade_key = trade_service::TradeKey::new(
                            exchange_id,
                            account_type,
                            symbol.clone(),
                        );
                        if let Ok(map) = self.trade_map.read() {
                            if let Some(series_arc) = map.get(&trade_key) {
                                if let Ok(mut series) = series_arc.write() {
                                    let trade = trade_service::Trade {
                                        timestamp_ms: timestamp,
                                        price,
                                        quantity,
                                        trade_id: 0,
                                        is_buyer_maker: if is_buyer_maker { 1 } else { 0 },
                                        _pad: [0u8; 7],
                                    };
                                    series.trades.push_back(trade);
                                    series.version += 1;
                                    series.dirty = true;
                                    if timestamp > series.last_ts_ms {
                                        series.last_ts_ms = timestamp;
                                    }
                                    // Enforce ring buffer capacity (simple eviction, no disk flush here).
                                    if series.trades.len() > series.capacity {
                                        series.trades.pop_front();
                                    }
                                }
                            }
                        }
                    }

                    // Pull new trades from the shared ring into all order-flow panels.
                    // All four panel kinds now read from shared_trades via tick().
                    for state in self.panels_store.big_trades.values_mut() {
                        state.tick();
                    }
                    for state in self.panels_store.volume_profile.values_mut() {
                        state.tick();
                    }
                    for state in self.panels_store.footprint.values_mut() {
                        state.tick();
                    }
                    for state in self.panels_store.trade_tape.values_mut() {
                        state.tick();
                    }

                    // Schedule indicator recalculation according to the current mode.
                    match self.indicator_manager.recalc_mode {
                        RecalcMode::PerTick => {
                            // Immediate recalc — pull bars from ALL windows with this symbol
                            // (fixes the bug where only the active window was considered).
                            let _ri_start = std::time::Instant::now();
                            self.recalc_indicators_for_symbol(&symbol);
                            self.last_indicator_recalc_us += _ri_start.elapsed().as_micros() as u64;
                        }
                        RecalcMode::PerFrame => {
                            // Defer to end-of-tick flush; all trades in this frame are batched.
                            self.indicator_manager.mark_dirty(&symbol);
                        }
                        RecalcMode::PerBar => {
                            if is_new_bar {
                                eprintln!("[ChartApp] PerBar: new bar detected for {}", symbol);
                                self.indicator_manager.mark_new_bar(&symbol);
                            } else {
                                // Still mark dirty so the flag exists; drain_pending_recalc
                                // will ignore it in PerBar mode unless a new bar formed.
                                self.indicator_manager.mark_dirty(&symbol);
                            }
                        }
                    }

                    // Update orderbook last_trade_price so the ghost-level filter
                    // has an authoritative mid to work with.
                    {
                        let ob_key = orderbook_service::OrderbookKey::new(exchange_id, account_type, &symbol);
                        let series_arc = {
                            let ob_map = self.bridge.orderbook_map();
                            ob_map.read().ok().and_then(|map| map.get(&ob_key).cloned())
                        };
                        if let Some(arc) = series_arc {
                            if let Ok(mut s) = arc.write() {
                                s.set_last_trade_price(price);
                            }
                        }
                    }
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
                    let entry = self.mini_ticker_cache
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

                    // Mirror last_price into the matching OrderbookSeries so the ghost
                    // filter has an authoritative mid even on slow markets with no trades.
                    {
                        let ob_key = orderbook_service::OrderbookKey::new(exchange_id, account_type, &symbol);
                        let series_arc = {
                            let ob_map = self.bridge.orderbook_map();
                            ob_map.read().ok().and_then(|map| map.get(&ob_key).cloned())
                        };
                        if let Some(arc) = series_arc {
                            if let Ok(mut s) = arc.write() {
                                s.set_last_trade_price(last_price);
                            }
                        }
                    }
                }
                LiveUpdate::ConnectorReady { exchange_id } => {
                    let eid_str = exchange_id.as_str();
                    if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                        eprintln!("[ChartApp] ConnectorReady for disabled {}, ignoring", eid_str);
                        continue;
                    }
                    // Always load symbols for any connector that becomes ready.
                    let bridge = self.bridge.clone();
                    bridge.request_symbols(exchange_id);

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
                    for window in self.panel_app.panel_grid.windows().values() {
                        if window.symbol.is_empty() { continue; }
                        let win_eid = digdigdig3::ExchangeId::from_str(&window.exchange)
                            .unwrap_or(digdigdig3::ExchangeId::Binance);
                        if win_eid == exchange_id {
                            let at = account_type_from_label(&window.account_type);
                            bridge.request_bars(exchange_id, &window.symbol, &window.timeframe, at, None, None, true);
                        }
                    }
                    if let Some(tm) = &mut self.trading_manager {
                        tm.on_connector_ready(exchange_id);
                    }
                }
                LiveUpdate::SymbolsLoaded { exchange_id, symbols } => {
                    self.exchange_symbols.insert(exchange_id, symbols);
                }
                LiveUpdate::Error { exchange_id, message } => {
                    eprintln!("[ChartApp] live-data error ({:?}): {}", exchange_id, message);
                }
                LiveUpdate::OrderbookSnapshot { exchange_id, account_type, symbol, bids: _, asks: _, timestamp: _, source: _ } => {
                    let ex_str = exchange_id.as_str();
                    let at_str = account_type.short_label();
                    // All orderbook panels now read from the shared OrderbookSeries via tick().
                    // The bridge already wrote the new data into the shared series before
                    // broadcasting this LiveUpdate, so tick() will see the version bump.
                    for state in self.panels_store.dom.values_mut() {
                        if state.symbol == symbol && state.exchange == ex_str && state.account_type == at_str {
                            state.tick();
                        }
                    }
                    for state in self.panels_store.l2_tape.values_mut() {
                        if state.symbol == symbol && state.exchange == ex_str && state.account_type == at_str {
                            state.tick();
                            state.prune_flash();
                        }
                    }
                    for state in self.panels_store.liquidity_heatmap.values_mut() {
                        if state.symbol == symbol && state.exchange == ex_str && state.account_type == at_str {
                            state.tick();
                        }
                    }
                }
                LiveUpdate::OrderbookDelta { exchange_id, account_type, symbol, bids: _, asks: _, timestamp: _ } => {
                    let ex_str = exchange_id.as_str();
                    let at_str = account_type.short_label();
                    // All orderbook panels read from shared OrderbookSeries via tick().
                    for state in self.panels_store.dom.values_mut() {
                        if state.symbol == symbol && state.exchange == ex_str && state.account_type == at_str {
                            state.tick();
                        }
                    }
                    for state in self.panels_store.l2_tape.values_mut() {
                        if state.symbol == symbol && state.exchange == ex_str && state.account_type == at_str {
                            state.tick();
                            state.prune_flash();
                        }
                    }
                    for state in self.panels_store.liquidity_heatmap.values_mut() {
                        if state.symbol == symbol && state.exchange == ex_str && state.account_type == at_str {
                            state.tick();
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
            }
        }
        self.last_event_process_us = events_start.elapsed().as_micros() as u64;

        if !trading_updates.is_empty() {
            if let Some(tm) = &mut self.trading_manager {
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
                drawing_points: Vec<(u64, Vec<(f64, f64)>, alerts::DrawingExtendMode)>,
            }
            let window_data: Vec<WindowAlertData> = self.panel_app.panel_grid.windows()
                .values()
                .filter_map(|window| {
                    let triple = (window.symbol.clone(), window.exchange.clone(), window.account_type.clone());
                    if seen_pairs.contains(&triple) {
                        return None;
                    }
                    seen_pairs.insert(triple);
                    let current_price = window.bars.last().map(|b| b.close).unwrap_or(0.0);
                    let current_bar = window.bars.len().saturating_sub(1) as f64;
                    let bars = &window.bars;
                    let drawing_points: Vec<(u64, Vec<(f64, f64)>, alerts::DrawingExtendMode)> = window
                        .drawing_manager
                        .primitives()
                        .iter()
                        .map(|p| {
                            let pts_bar: Vec<(f64, f64)> = p.points()
                                .into_iter()
                                .map(|(ts_ms, price)| (timestamp_ms_to_bar_f64(bars, ts_ms), price))
                                .collect();
                            (p.data().id, pts_bar, alerts::DrawingExtendMode::from_u8(p.extend_mode_raw()))
                        })
                        .collect();
                    Some(WindowAlertData {
                        symbol: window.symbol.clone(),
                        exchange: window.exchange.clone(),
                        account_type: window.account_type.clone(),
                        current_price,
                        current_bar,
                        drawing_points,
                    })
                })
                .collect();

            let indicator_values = Self::build_indicator_values_for_alerts(
                &self.alert_manager,
                &self.indicator_manager,
            );

            let mut all_triggered_ids: Vec<u64> = Vec::new();
            for wd in &window_data {
                let triggered = self.alert_manager.check_crossings_dynamic(
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
                use zengeld_terminal_indicators::signals::signal::BarConfirmation;

                let signal_batch: Vec<(u64, usize, i8, u8, String)> = self
                    .indicator_manager
                    .instances_iter()
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
                    let triggered = self.alert_manager.check_signal_alerts(
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
            let current_price = self.panel_app.panel_grid.active_window()
                .and_then(|w| w.bars.last())
                .map(|b| b.close)
                .unwrap_or(0.0);

            // Build delivery events for triggered alerts.
            if !triggered_ids.is_empty() {
                let symbol = self.panel_app.panel_grid.active_window()
                    .map(|w| w.symbol.clone())
                    .unwrap_or_default();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);

                for id in &triggered_ids {
                    if let Some(alert) = self.alert_manager.get(*id) {
                        self.pending_delivery_events.push(alert_delivery::DeliveryEvent {
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
        let pending = self.indicator_manager.drain_pending_recalc();
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
                    .windows()
                    .iter()
                    .filter(|(_id, w)| w.symbol == *symbol)
                    .map(|(id, _w)| id.0)
                    .collect();

                // Split-borrow: `panel_app.panel_grid` and `indicator_manager`
                // are distinct struct fields, so both can be used simultaneously.
                for window_id in matching_ids {
                    let chart_id = ChartId(window_id);
                    if let Some(w) = self.panel_app.panel_grid.windows().get(&chart_id) {
                        self.indicator_manager.calculate_for_window(window_id, &w.bars);
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
            let mode = match self.indicator_manager.recalc_mode {
                RecalcMode::PerTick => "PerTick",
                RecalcMode::PerFrame => "PerFrame",
                RecalcMode::PerBar => "PerBar",
            };
            eprintln!(
                "[ChartApp] RecalcMode={} | trades={} recalcs={} in 5s",
                mode, self.trade_count, self.recalc_count
            );
            self.trade_count = 0;
            self.recalc_count = 0;
            self.recalc_log_timer = std::time::Instant::now();
        }

        // ── Layer 3: trigger scroll-fetch when viewport approaches left edge ───
        // Collect (symbol, exchange, tf_name, at_label) for windows that need a
        // historical extension fetch.  Two-pass to avoid a mutable/immutable
        // borrow conflict: first collect while mutating `scroll_fetch_in_flight`,
        // then re-borrow immutably to read `oldest_ts` for each request.
        {
            let mut scroll_requests: Vec<(String, String, String, String)> = Vec::new();

            let max_loaded = self.panel_app.user_manager.profile.data_load.max_loaded_bars;

            for window in self.panel_app.panel_grid.windows_mut().values_mut() {
                if window.scroll_fetch_in_flight {
                    if let Some(started) = window.scroll_fetch_started {
                        if started.elapsed() > std::time::Duration::from_secs(10) {
                            eprintln!("[ChartApp] scroll_fetch_in_flight timeout, resetting for {}", window.symbol);
                            window.scroll_fetch_in_flight = false;
                            window.scroll_fetch_started = None;
                        }
                    }
                    if window.scroll_fetch_in_flight { continue; }
                }
                if window.bars.is_empty() { continue; }
                if window.pending_symbol_load { continue; }

                let visible = window.viewport.visible_bars() as f64;
                let threshold = (visible * 0.20).max(5.0);
                if window.viewport.view_start > threshold { continue; }

                if max_loaded > 0 && window.bars.len() >= max_loaded as usize { continue; }

                window.scroll_fetch_in_flight = true;
                window.scroll_fetch_started = Some(std::time::Instant::now());
                scroll_requests.push((
                    window.symbol.clone(),
                    window.exchange.clone(),
                    window.timeframe.name.clone(),
                    window.account_type.clone(),
                ));
            }

            for (symbol, exchange, tf_name, at_label) in scroll_requests {
                let oldest_ts = self.panel_app.panel_grid.windows()
                    .values()
                    .find(|w| {
                        w.symbol == symbol
                            && w.exchange == exchange
                            && w.timeframe.name == tf_name
                            && w.account_type == at_label
                    })
                    .and_then(|w| w.bars.first().map(|b| b.timestamp))
                    .unwrap_or(0);

                if oldest_ts == 0 { continue; }

                let eid = digdigdig3::ExchangeId::from_str(&exchange)
                    .unwrap_or(digdigdig3::ExchangeId::Binance);
                let at = account_type_from_label(&at_label);

                if let Some(tf) = parse_timeframe_name(&tf_name) {
                    self.bridge.request_scroll_bars(eid, &symbol, &tf, at, oldest_ts, 500);
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
        self.last_tick_us = tick_start.elapsed().as_micros() as u64;

        // ── Leak / backpressure census ───────────────────────────────────────
        // Snapshot growable structures so the [PERF] line can show whether they
        // grow unbounded over time (the suspected "leak" / load-storm).
        self.diag_queue_len = self.live_update_rx.len();
        self.diag_series_handles = self.series_handles.len();
        self.diag_mini_ticker = self.mini_ticker_cache.len();

        // ── Tick spike log ───────────────────────────────────────────────────
        // Print a one-line breakdown ONLY when this tick blew past budget, so we
        // can attribute random multi-ms/multi-second tick explosions to the exact
        // events that flooded the queue this frame. Gated by MLC_PERF_LOG so it's
        // silent in normal runs.
        if self.last_tick_us > 3000 && std::env::var("MLC_PERF_LOG").is_ok() {
            eprintln!(
                "[TICK-SPIKE] tick={}us events={}us recalc={}us autoscale={}us | drained={} (bars={} backfill={} scroll={} trade={} ticker={} conn={} other={}) lag_events={}",
                self.last_tick_us,
                self.last_event_process_us,
                self.last_indicator_recalc_us,
                self.last_auto_scale_us,
                _drain_count, n_bars, n_backfill, n_scroll, n_trade, n_ticker, n_connector, n_other,
                self.lag_event_count,
            );
        }
    }
}
