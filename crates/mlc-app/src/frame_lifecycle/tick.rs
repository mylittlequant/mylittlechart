//! Live update tick loop: drives `BackendCore::tick_writers` to drain the
//! broadcast channel, then runs per-app reactions on the emitted `BubbleEvent`s.
#[cfg(target_arch = "wasm32")]
use crate::agent as gate4agent;


use crate::ChartApp;
use crate::parse_timeframe_name;
use mlc_backend::BubbleEvent;
use mlc_backend_data_bridge::LiveUpdate;
use mlc_core::{ChartId, ScaleMode, timestamp_ms_to_bar_f64};
use mlc_backend_indicator_engine::RecalcMode;

impl ChartApp {
    /// Called every frame with the current wall-clock time in milliseconds.
    ///
    /// Drives `BackendCore::tick_writers` to drain the broadcast channel and
    /// write incoming data into the shared bubble store (bars, derived series,
    /// mini-ticker, exchange-symbols). Returns one `BubbleEvent` per write so
    /// per-app reactions can run viewport/indicator/panel side-effects.
    pub fn tick(&mut self, current_time_ms: u64) {
        let _ = current_time_ms;
        let tick_start = web_time::Instant::now();

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

        // ── Live data: drive backend writers ─────────────────────────────────
        // BackendCore::tick_writers drains the broadcast channel and writes
        // incoming data into the shared bubble store (bars, derived series,
        // mini-ticker, exchange-symbols). It returns one BubbleEvent per write
        // so per-app reactors can run viewport/indicator/panel side-effects.
        let mut had_trade_update = false;
        // had_ticker_update — same idea but for MiniTickerUpdated. Watchlist /
        // sidebar prices come straight out of `core.mini_ticker` (DashMap) at
        // render time; without this flag the sidebar render is skipped until
        // some other input triggers a redraw (a click, an alert, an agent
        // command). Owner regression 2026-06-20: "watchlist values don't
        // refresh unless I click the chart".
        let mut had_ticker_update = false;
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
        // Event routing:
        //   OrderbookSnapshotRest → force_rebuild = true for DOM
        //   OrderbookDelta{bids,asks} → push ObDeltaEvent preserving per-event ts
        /// One WS delta message with its original wire timestamp.
        struct ObDeltaEvent {
            ts_ms: i64,
            bids: Vec<(f64, f64)>,
            asks: Vec<(f64, f64)>,
        }
        struct ObDeltaEntry {
            /// Per-event raw deltas in arrival order. Each entry preserves its own ts
            /// so L2-tape rows get per-event ts instead of a frame-batch ts.
            events: Vec<ObDeltaEvent>,
            /// True when a REST snapshot arrived this frame → DOM must full-rebuild.
            force_rebuild: bool,
        }
        let mut ob_dirty: std::collections::HashMap<(String, String, String), ObDeltaEntry> =
            std::collections::HashMap::new();
        // Coalesce last-trade-price → orderbook ghost-filter updates.  Calling
        // set_last_trade_price per trade is O(book): it sweeps ghost levels
        // (~1000) AND rebuilds the published view (full ~1000×2 clone) every time.
        // On a burst (172 trades/frame) that was the dominant ~21ms tick spike.
        // Only the LAST price per symbol matters for the ghost filter, so record
        // it here and apply once after the drain loop.
        let mut last_trade_price_by_symbol: std::collections::HashMap<(String, String, String), f64> =
            std::collections::HashMap::new();

        let events_start = web_time::Instant::now();

        let events = self.core.tick_writers(current_time_ms);
        _drain_count = events.len() as u32;

        // ── Per-event reactor loop ────────────────────────────────────────────
        for event in &events {
            // Census.
            match event {
                BubbleEvent::BarsLoaded { .. } => n_bars += 1,
                BubbleEvent::BackfillCompleted { .. } => n_backfill += 1,
                BubbleEvent::ScrollBarsAppended { .. } => n_scroll += 1,
                BubbleEvent::TradeUpdatePassthrough { .. } | BubbleEvent::BarUpdatePassthrough { .. } => {
                    n_trade += 1;
                }
                BubbleEvent::MiniTickerUpdated { .. } => n_ticker += 1,
                BubbleEvent::ConnectorReady { .. } => n_connector += 1,
                BubbleEvent::SymbolsLoaded { count, .. } => {
                    let t0 = web_time::Instant::now();
                    n_symbols += 1;
                    n_symbols_total += count;
                    symbols_insert_us += t0.elapsed().as_micros() as u64;
                }
                _ => n_other += 1,
            }

            // Inline accumulator updates for items consumed after the loop.
            match event {
                BubbleEvent::TradeUpdatePassthrough { exchange_id, account_type, symbol, price, .. } => {
                    had_trade_update = true;
                    last_trade_price_by_symbol.insert(
                        (symbol.clone(), exchange_id.as_str().to_string(), account_type.clone()),
                        *price,
                    );
                }
                BubbleEvent::MiniTickerUpdated { exchange_id, symbol, account_type, last_price } => {
                    last_trade_price_by_symbol.insert(
                        (symbol.clone(), exchange_id.as_str().to_string(), account_type.clone()),
                        *last_price,
                    );
                    had_ticker_update = true;
                }
                BubbleEvent::OrderbookSnapshotRest { exchange_id, symbol, account_type } => {
                    self.last_ob_event_count += 1;
                    let key = (symbol.clone(), exchange_id.as_str().to_string(), account_type.clone());
                    let entry = ob_dirty.entry(key).or_insert_with(|| ObDeltaEntry {
                        events: Vec::new(),
                        force_rebuild: false,
                    });
                    entry.force_rebuild = true;
                }
                BubbleEvent::OrderbookDelta { exchange_id, symbol, account_type, bids, asks, ts_ms } => {
                    self.last_ob_event_count += 1;
                    let key = (symbol.clone(), exchange_id.as_str().to_string(), account_type.clone());
                    let entry = ob_dirty.entry(key).or_insert_with(|| ObDeltaEntry {
                        events: Vec::new(),
                        force_rebuild: false,
                    });
                    entry.events.push(ObDeltaEvent { ts_ms: *ts_ms, bids: bids.clone(), asks: asks.clone() });
                }
                BubbleEvent::OrderbookDeltaForMultiDom { exchange_id, symbol, bids, asks, .. } => {
                    let ex_label = exchange_id.as_str().to_string();
                    let target_ids: Vec<mlc_core::state::PanelId> = crate::standalone_panel::StandalonePanelRegistry::multi_dom_ids_for_symbol(
                        &self.panels_store.multi_dom, symbol.as_str(),
                    ).collect();
                    for pid in target_ids {
                        let Some(state) = self.panels_store.multi_dom.get_mut(&pid) else { continue };
                        if let Some(idx) = state.exchange_idx(&ex_label) {
                            let bids_v: Vec<(f64, f64)> = bids.iter().map(|(p, q)| (*p, *q)).collect();
                            let asks_v: Vec<(f64, f64)> = asks.iter().map(|(p, q)| (*p, *q)).collect();
                            state.apply_snapshot(idx, bids_v.into_iter(), asks_v.into_iter());
                            state.rebuild_merged();
                        }
                    }
                }
                BubbleEvent::TradingUpdate { update } => {
                    trading_updates.push(update.clone());
                }
                _ => {}
            }

            // Per-window / indicator / panel side-effects.
            self.react_to_bubble_event(event, current_time_ms);
        }

        // Keep census variables live so the spike / startup-wave logs at the end
        // of tick() can reference them without compiler warnings.
        let _ = last_trade_price_by_symbol;
        let _ = symbols_insert_us;
        let _ = n_scroll;
        let _ = ScaleMode::Auto; // silence import

        self.last_event_process_us = events_start.elapsed().as_micros() as u64;

        // ── Pending compare deadline countdown ───────────────────────────────
        // Evict entries that timed out without a BarsLoaded response (~10 s at 60 fps).
        self.pending_compare.retain_mut(|p| {
            if p.deadline_frames == 0 {
                eprintln!(
                    "[Compare] Timed out waiting for bars: symbol={} exchange={} at={}",
                    p.symbol,
                    p.exchange.as_str(),
                    p.account_type_label
                );
                false
            } else {
                p.deadline_frames = p.deadline_frames.saturating_sub(1);
                true
            }
        });

        // ── Coalesced orderbook panel re-aggregation ─────────────────────────
        // DOM + heatmap: tick once per frame with the merged level changes (they
        // track current book state, not history — per-event ts is irrelevant).
        // L2 tape: iterate the per-event list so each tape row carries its own
        // original wire timestamp instead of a single SystemTime::now() for the
        // whole batch.
        if !ob_dirty.is_empty() {
            let ob_panel_start = web_time::Instant::now();
            for ((symbol, ex_str, at_str), entry) in &ob_dirty {
                // ── DOM: incremental or full-rebuild (bulk — level state only) ──
                let dom_t0 = web_time::Instant::now();
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
                let l2_t0 = web_time::Instant::now();
                for state in self.panels_store.l2_tape.values_mut() {
                    if &state.symbol == symbol && &state.exchange == ex_str && &state.account_type == at_str {
                        if entry.force_rebuild || entry.events.is_empty() {
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
                let heat_t0 = web_time::Instant::now();
                for state in self.panels_store.liquidity_heatmap.values_mut() {
                    if &state.symbol == symbol && &state.exchange == ex_str && &state.account_type == at_str {
                        state.tick();
                    }
                }
                self.last_heatmap_panel_us += heat_t0.elapsed().as_micros() as u64;
            }
            self.last_orderbook_panel_us += ob_panel_start.elapsed().as_micros() as u64;
        }

        if !trading_updates.is_empty() {
            if let Some(mut tm) = self.core.trading_mut() {
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

        // ── Watchlist refresh trigger ─────────────────────────────────────────
        // mini_ticker writes are passthrough at the BackendCore layer (DashMap,
        // lock-free) — the sidebar render path reads them on demand. But render
        // only rebuilds when sidebar_data_dirty=true, so without an explicit
        // flip here the watchlist freezes between user clicks. Pre-cutover the
        // tick.rs MiniTickerUpdate arm did the equivalent.
        //
        // Rate-limit because the tick can carry 20+ MiniTicker events per
        // frame (one per watchlist symbol at 1Hz, fanned in 16ms windows by
        // the broadcast channel). Mark once per ~250ms wall-clock so we still
        // get ~4 sidebar refreshes per second without rebuilding 20× per tick.
        if had_ticker_update {
            const MARK_INTERVAL_MS: u128 = 250;
            let now = web_time::Instant::now();
            let bump = match self.sidebar_ticker_mark {
                Some(prev) => now.duration_since(prev).as_millis() >= MARK_INTERVAL_MS,
                None => true,
            };
            if bump {
                self.sidebar_data_dirty = true;
                self.sidebar_ticker_mark = Some(now);
            }
        }

        // ── Alert checker: detect price crossings for every visible bubble ────
        // Skip entirely when no trade arrived this tick — nothing changed.
        if had_trade_update {
            // Collect per-bubble alert data: bubble_id, current_price, current_bar, drawing_points.
            // De-duplicate by bubble_id so shared bubbles are only checked once.
            let mut seen_bids: std::collections::HashSet<mlc_core::ids::BubbleId> = std::collections::HashSet::new();
            struct BubbleAlertData {
                bid: mlc_core::ids::BubbleId,
                current_price: f64,
                current_bar: f64,
                drawing_points: Vec<(u64, Vec<(f64, f64)>, mlc_core::alert::DrawingExtendMode)>,
            }
            let bubble_data: Vec<BubbleAlertData> = self.panel_app.panel_grid.containers()
                .iter()
                .filter_map(|(_cid, window)| {
                    let bid = window.current_bubble_id;
                    if !seen_bids.insert(bid) {
                        return None;
                    }
                    let bubble_bars_alert: Vec<mlc_core::Bar> = self.with_bubble(bid, |b| b.bars.clone()).unwrap_or_default();
                    let current_price = bubble_bars_alert.last().map(|b| b.close).unwrap_or(0.0);
                    let current_bar = bubble_bars_alert.len().saturating_sub(1) as f64;
                    let drawing_points: Vec<(u64, Vec<(f64, f64)>, mlc_core::alert::DrawingExtendMode)> = {
                        let guard = self.core.drawings();
                        guard.primitives(bid).iter()
                            .map(|p| {
                                let pts_bar: Vec<(f64, f64)> = p.points()
                                    .into_iter()
                                    .map(|(ts_ms, price)| (timestamp_ms_to_bar_f64(&bubble_bars_alert, ts_ms), price))
                                    .collect();
                                (p.data().id, pts_bar, mlc_core::alert::DrawingExtendMode::from_u8(p.extend_mode_raw()))
                            })
                            .collect()
                    };
                    Some(BubbleAlertData {
                        bid,
                        current_price,
                        current_bar,
                        drawing_points,
                    })
                })
                .collect();

            // Build indicator values from all bubbles' alerts combined.
            let all_alerts_snapshot: Vec<mlc_core::alert::AlertItem> = self.core.alerts().items_all();
            let indicator_values = Self::build_indicator_values_for_alerts(
                &all_alerts_snapshot,
                &self.core.indicators(),
            );

            let mut all_triggered: Vec<(u64, mlc_core::ids::BubbleId)> = Vec::new();
            for bd in &bubble_data {
                let triggered = self.core.alerts_mut().on_tick(
                    bd.bid,
                    bd.current_price,
                    bd.current_bar,
                    &bd.drawing_points,
                    &indicator_values,
                );
                for id in triggered {
                    all_triggered.push((id, bd.bid));
                }
            }

            // ── Signal alert checker ───────────────────────────────────────────────
            {
                use mlc_backend_indicator_engine::signals::signal::BarConfirmation;

                let signal_batch: Vec<(u64, usize, i8, u8, String)> = self.core.indicators().all_instances()
                    .into_iter()
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

                for bd in &bubble_data {
                    let triggered = self.core.alerts_mut().on_signals(bd.bid, &signal_batch);
                    for id in triggered {
                        all_triggered.push((id, bd.bid));
                    }
                }
            }

            // Deduplicate by alert id.
            let mut seen_alert_ids: std::collections::HashSet<u64> = std::collections::HashSet::new();
            let triggered_ids: Vec<u64> = all_triggered.into_iter()
                .filter(|(id, _)| seen_alert_ids.insert(*id))
                .map(|(id, _)| id)
                .collect();

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
                let now = web_time::SystemTime::now()
                    .duration_since(web_time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);

                // Snapshot triggered alerts from all bubbles for delivery.
                let triggered_alerts: Vec<mlc_core::alert::AlertItem> = {
                    let alerts = self.core.alerts();
                    triggered_ids.iter().filter_map(|&id| {
                        alerts.find_owner(id).map(|(_, a)| a)
                    }).collect()
                };
                for alert in &triggered_alerts {
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

                self.pending_alert_screenshot = true;
                self.sidebar_data_dirty = true;
            }
        }

        // ── Deferred indicator recalculation flush (PerFrame / PerBar) ────────
        // For PerTick mode drain_pending_recalc returns an empty Vec — no-op.
        let pending = self.core.indicators_mut().drain_pending_recalc();
        if !pending.is_empty() {
            let deferred_start = web_time::Instant::now();
            for symbol in &pending {
                let matching_ids: Vec<u64> = self
                    .panel_app
                    .panel_grid
                    .containers()
                    .keys()
                    .filter(|cid| self.symbol_for_chart(**cid)
                        .map_or(false, |s| s == *symbol))
                    .map(|cid| cid.0)
                    .collect();

                for window_id in matching_ids {
                    let chart_id = ChartId(window_id);
                    let bid_opt = self.panel_app.panel_grid.containers().get(&chart_id).map(|w| w.current_bubble_id);
                    if let Some(bid) = bid_opt {
                        let bars_recalc: Vec<mlc_core::Bar> = self.with_bubble(bid, |b| b.bars.clone()).unwrap_or_default();
                        self.core.indicators_mut().recalc_bubble(bid, &bars_recalc);
                    }
                }
                let _ = symbol; // recalc_bubble filters by bid, not symbol
                self.recalc_count += 1;
            }
            self.last_indicator_recalc_us += deferred_start.elapsed().as_micros() as u64;
            self.sync_sub_panes_from_manager();
        }

        // ── Periodic RecalcMode diagnostic log (every 5 seconds) ─────────────
        if self.diagnostics_enabled
            && self.recalc_log_timer.elapsed() >= std::time::Duration::from_secs(5)
        {
            let mode = match self.core.indicators().recalc_mode() {
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
            self.recalc_log_timer = web_time::Instant::now();
        }

        // ── Layer 3: trigger scroll-fetch when viewport approaches left edge ───
        {
            let mut scroll_requests: Vec<(mlc_core::state::ChartId, String, String, String, mlc_core::instrument::AccountType)> = Vec::new();
            let max_loaded = self.core.user().profile().domain.data_load.max_loaded_bars;

            let instrument_by_cid_scroll: std::collections::HashMap<mlc_core::state::ChartId, (String, mlc_core::state::Timeframe, String, mlc_core::instrument::AccountType)> = {
                self.panel_app.panel_grid.containers().keys()
                    .filter_map(|cid| {
                        let sym = self.symbol_for_chart(*cid)?;
                        let tf = self.timeframe_for_chart(*cid)?;
                        let exch = self.exchange_for_chart(*cid)?;
                        let at: mlc_core::instrument::AccountType = self.account_type_for_chart(*cid)?.parse().unwrap_or_default();
                        Some((*cid, (sym, tf, exch, at)))
                    })
                    .collect()
            };

            let bar_counts_by_chart: std::collections::HashMap<mlc_core::state::ChartId, usize> =
                self.panel_app.panel_grid.containers().iter()
                    .map(|(cid, w)| (*cid, self.core.bubbles().bars(w.current_bubble_id).len()))
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
                window.scroll_fetch_started = Some(web_time::Instant::now());
                scroll_requests.push((
                    *cid_scroll,
                    win_sym_sc.clone(),
                    win_exch_sc.clone(),
                    win_tf_sc.name.clone(),
                    *win_at_sc,
                ));
            }

            for (cid_req, symbol, exchange, tf_name, account_type) in scroll_requests {
                // Read the OLDEST bar's ts from the active bubble for this chart.
                // The pre-cutover code path stored bars on the window directly;
                // it now lives on the bubble. The dangling `None::<&Bar>` here
                // made `oldest_ts` always 0, which made the next `continue` skip
                // EVERY scroll-fetch — left/scroll backfill silently no-op'd.
                let oldest_ts = self.panel_app.panel_grid.containers()
                    .get(&cid_req)
                    .and_then(|w| self.core.bubbles().bars(w.current_bubble_id).first().map(|bar| bar.timestamp))
                    .unwrap_or(0);

                if oldest_ts == 0 { continue; }

                let eid = digdigdig3::ExchangeId::from_str(&exchange)
                    .unwrap_or(digdigdig3::ExchangeId::Binance);
                let at = mlc_backend_data_bridge::account_type_to_dig3(account_type);

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

        self.flush_pending_autosave();

        self.last_tick_us = tick_start.elapsed().as_micros() as u64;

        // broadcast_rx is now owned by BackendCore — direct len() not accessible.
        self.diag_queue_len = 0;
        self.diag_mini_ticker = self.core.mini_ticker.len();

        // ── Tick spike log ───────────────────────────────────────────────────
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

        // ── Startup-wave log ─────────────────────────────────────────────────
        if (n_connector > 0 || n_symbols > 0) && std::env::var("MLC_PERF_LOG").is_ok() {
            eprintln!(
                "[STARTUP-WAVE] conn_ready={} symbols_loaded={}(total_syms={} insert={}us) tick={}us events={}us drained={} | skeleton_window={}",
                n_connector, n_symbols, n_symbols_total, symbols_insert_us,
                self.last_tick_us, self.last_event_process_us, _drain_count,
                self.panel_app.panel_grid.containers().is_empty(),
            );
        }

        // ── Indicator heal ───────────────────────────────────────────────────
        let any_unhealed = {
            let attempted = &self.indicator_heal_attempted;
            let im = self.core.indicators();
            self.panel_app.panel_grid.iter_containers().any(|(_, w)| {
                im.instances_for_bubble(w.current_bubble_id)
                    .iter()
                    .any(|i| i.values.is_empty() && !attempted.contains(&i.id))
            })
        };
        if any_unhealed {
            let window_ids: Vec<(u64, mlc_core::state::ChartId)> = self
                .panel_app
                .panel_grid
                .iter_containers()
                .map(|(_, w)| (w.current_bubble_id.0, w.id))
                .collect();
            for (bid, cid) in window_ids {
                let bubble_id = mlc_core::ids::BubbleId(bid);
                let needs_heal: Vec<u64> = self.core.indicators().instances_for_bubble(bubble_id)
                    .iter()
                    .filter(|i| i.values.is_empty() && !self.indicator_heal_attempted.contains(&i.id))
                    .map(|i| i.id)
                    .collect();
                if !needs_heal.is_empty() {
                    let bars = self.bars_for_chart(cid);
                    for id in &needs_heal {
                        self.indicator_heal_attempted.insert(*id);
                    }
                    eprintln!(
                        "[ChartApp] indicator heal: bubble {} has {} bars, {} never-computed instance(s) {:?} — recalculating",
                        bid, bars.len(), needs_heal.len(), needs_heal
                    );
                    self.core.indicators_mut().recalc_bubble(bubble_id, &bars);
                }
            }
        }
    }
}
