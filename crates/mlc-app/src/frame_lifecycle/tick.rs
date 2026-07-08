//! Live update tick loop: `BackendCore::tick_writers` drains the broadcast
//! channel (called once per frame by the host — see [`ChartApp::tick`] /
//! [`ChartApp::react_to_events`]), then each window runs its own per-app
//! reactions on the emitted `BubbleEvent`s.
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
    /// Single-window convenience wrapper: drains `BackendCore::tick_writers`
    /// itself, then reacts to the drained events. Only correct when exactly
    /// one `ChartApp` reads this `core` (wasm shell, single-window native
    /// test harnesses). Multi-window hosts (vello shell) MUST call
    /// `core.tick_writers` once per frame themselves and fan the same
    /// `&[BubbleEvent]` slice into every open window's
    /// [`Self::react_to_events`] instead — calling `tick` per window would
    /// drain the shared broadcast channel N times, each window only seeing
    /// its own fraction of the events.
    pub fn tick(&mut self, current_time_ms: u64) {
        let events = self.core.tick_writers(current_time_ms);
        self.react_to_events(&events, current_time_ms);
    }

    /// Per-window reaction pass over an already-drained `BubbleEvent` slice.
    ///
    /// Caller (the host — `App::about_to_wait` in the vello shell, or
    /// [`Self::tick`] for single-window hosts) drains `core.tick_writers`
    /// ONCE per frame and passes the same slice to every open window's
    /// `ChartApp`. This runs every per-window side-effect that used to live
    /// inline in the drain loop (viewport/indicator/panel reactions, alert
    /// checking, scroll-fetch, connector warm-up, agent autostart, etc) —
    /// everything from the old `tick` body except the drain itself.
    pub fn react_to_events(&mut self, events: &[BubbleEvent], current_time_ms: u64) {
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

        // ── Live data: react to backend writers ──────────────────────────────
        // `events` was drained once this frame by the host's single
        // `core.tick_writers` call (one BubbleEvent per write into the shared
        // bubble store — bars, derived series, mini-ticker, exchange-symbols)
        // and handed to every open window's `react_to_events`. What follows
        // is this window's own reaction pass over that shared slice.
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

        _drain_count = events.len() as u32;

        // ── Per-event reactor loop ────────────────────────────────────────────
        for event in events {
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
                    let bid = window.current_tag_id;
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
                .map(|w| w.current_tag_id)
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
                    let bid_opt = self.panel_app.panel_grid.containers().get(&chart_id).map(|w| w.current_tag_id);
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
        //
        // Routes by chart_type family (2026-07-04 derived-seed fix — see
        // `nemo/docs/mlc/plans/dig3-station-0325-derived-seeding-2026-07-04.md`,
        // corrected 2026-07-07 — derived-prepend-fold-plan-2026-07-07.md):
        // - Kline family (candles/line/area/.../histogram/columns) →
        //   `request_scroll_bars` (existing kline machinery, untouched).
        // - Everything `chart_type_is_derived_seed` (renko/pnf/kagi/
        //   three_line_break/range_bar/tick_bar/volume_bar/dollar_bar/
        //   tick_imbalance_bar/volume_imbalance_bar/run_bar/footprint/tpo) →
        //   `request_deeper_derived_seed` via `request_deeper_seed_for_active`.
        //   `footprint` IS in this branch (its `ChartTypeKind` is
        //   `OrderflowSlotted`, not `RenderOnly`) even though its rendered
        //   series still piggybacks `bubble.bars` — its bridge-side deepen
        //   (`request_deeper_footprint_seed`) has been in-place since the
        //   `rewarm_footprint` cutover. Below, `chart_type_deepen_is_teardown`
        //   further splits this branch: only the 3 EMA kinds (tick_imbalance_
        //   bar/volume_imbalance_bar/run_bar) still clear+reset for a full
        //   teardown+respawn — every other kind here (including footprint)
        //   rides `Station::rewarm_derived`/`rewarm_footprint` in place with
        //   no clear.
        //
        // The leftmost-rendered-bar read now goes through `active_series_for_chart`
        // (`ts_ms_at(0)` / `len()`) instead of hardcoding `bubble.bars` — the old
        // code read `bubble.bars.first().timestamp` unconditionally and always
        // called `request_scroll_bars`, which silently extended klines even for
        // chart types that don't render them (renko/pnf/kagi/... kept re-fetching
        // kline history nobody looked at, and NEVER deepened their own series).
        {
            let mut scroll_requests: Vec<(mlc_core::state::ChartId, String, String, String, mlc_core::instrument::AccountType)> = Vec::new();
            let mut derived_seed_requests: Vec<(mlc_core::state::ChartId, &'static str, String, String, mlc_core::instrument::AccountType)> = Vec::new();
            let max_loaded = self.core.user().profile().domain.data_load.max_loaded_bars;

            let instrument_by_cid_scroll: std::collections::HashMap<mlc_core::state::ChartId, (String, mlc_core::state::Timeframe, String, mlc_core::instrument::AccountType)> = {
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

            // Resolve each chart's chart_type + rendered-series (bar_count,
            // leftmost-ts) ONCE, before the mutable containers_mut() loop —
            // `active_series_for_chart` takes an owned snapshot precisely so
            // the bubble lock is released before this loop mutates windows.
            let active_series_by_chart: std::collections::HashMap<mlc_core::state::ChartId, crate::active_series::OwnedActiveSeries> =
                self.panel_app.panel_grid.containers().keys()
                    .map(|cid| (*cid, self.active_series_for_chart(*cid)))
                    .collect();
            let chart_type_by_chart: std::collections::HashMap<mlc_core::state::ChartId, &'static str> =
                self.panel_app.panel_grid.containers().keys()
                    .filter_map(|cid| self.chart_type_for_chart(*cid).map(|ct| (*cid, ct)))
                    .collect();

            // F4 (viewport doctrine audit 2026-07-07): the in-flight guards
            // live on the BUBBLE, not the window — the resource they guard
            // (a scroll-left backfill / derived reseed fetch) is per-bubble.
            // Two windows sharing one bubble used to carry independent
            // per-window flags, so each fired its own backfill for the same
            // bubble; the second event's `prev_bars` snapshot went stale
            // w.r.t. the first event's merge (double/under-shift). Resolve
            // each chart's bubble id up front (immutable borrow), same
            // access pattern as `ops.clear_slot` below.
            let bid_by_chart: std::collections::HashMap<mlc_core::state::ChartId, mlc_core::ids::BubbleId> =
                self.panel_app.panel_grid.containers().keys()
                    .filter_map(|cid| self.bubble_for_chart(*cid).map(|bid| (*cid, bid)))
                    .collect();

            for (cid_scroll, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                let chart_type = chart_type_by_chart.get(cid_scroll).copied().unwrap_or("candles");
                let is_derived = crate::active_series::chart_type_is_derived_seed(chart_type);
                let Some(bid_scroll) = bid_by_chart.get(cid_scroll).copied() else { continue; };

                // hybrid-data-modes-plan-2026-07-08, Step 2: a `LiveOnly`
                // bubble (RecentOnly venue — Bybit 60/1000, KuCoin 100, ...)
                // has no venue-side history at all, ever, for this
                // instrument+chart_type. Skip BEFORE arming
                // `derived_seed_in_flight` (10s timeout bookkeeping) or
                // firing the one wasted rewarm/trades_recent REST call every
                // scroll-left crossing would otherwise cost. `Capped` still
                // attempts to deepen (there may be more history up to the
                // wall); `Unknown` (seed not yet reported, or a chart_type
                // that never populates `history_depth`) never blocks —
                // default behavior preserved.
                if is_derived {
                    let depth = self.core.bubbles().history_depth(bid_scroll);
                    if depth.is_deepen_blocked() {
                        continue;
                    }
                }

                // Per-chart-type-family in-flight guard: kline scroll and
                // derived reseed are mutually exclusive per bubble but need
                // independent flags — see `derived_seed_in_flight`'s doc
                // comment on `DataBubble`.
                if is_derived {
                    let in_flight = self.core.bubbles().with_bubble(bid_scroll, |b| b.derived_seed_in_flight).flatten();
                    if let Some((started, _)) = in_flight {
                        if started.elapsed() > std::time::Duration::from_secs(10) {
                            crate::dlog!("[ChartApp] derived_seed_in_flight timeout, resetting for chart {:?}", cid_scroll);
                            self.core.bubbles_mut().with_bubble_mut(bid_scroll, |b| b.derived_seed_in_flight = None);
                            // P0 V2 follow-up: a failed/short reseed must not
                            // leave `reseed_clamp_guard` armed forever — that
                            // would permanently disable `resize_geometry`'s
                            // end-clamp for this window. The in-flight flag
                            // and the clamp guard both track the same reseed
                            // attempt, so they're released together.
                            window.reseed_clamp_guard = None;
                        } else {
                            continue;
                        }
                    }
                } else {
                    let in_flight = self.core.bubbles().with_bubble(bid_scroll, |b| b.scroll_fetch_in_flight).unwrap_or(false);
                    if in_flight {
                        let started = self.core.bubbles().with_bubble(bid_scroll, |b| b.scroll_fetch_started).flatten();
                        if let Some(started) = started {
                            if started.elapsed() > std::time::Duration::from_secs(10) {
                                crate::dlog!("[ChartApp] scroll_fetch_in_flight timeout, resetting for chart {:?}", cid_scroll);
                                self.core.bubbles_mut().with_bubble_mut(bid_scroll, |b| {
                                    b.scroll_fetch_in_flight = false;
                                    b.scroll_fetch_started = None;
                                });
                                continue;
                            }
                        }
                        continue;
                    }
                }

                let series = match active_series_by_chart.get(cid_scroll) {
                    Some(s) => s.as_active_series(),
                    None => continue,
                };
                let bar_count_scroll = series.len();
                if bar_count_scroll == 0 { continue; }
                if window.pending_symbol_load { continue; }

                // Scroll-pressure trigger (unified-data-plane Э2 doctrine:
                // fire at ~80% of LOADED history toward the oldest edge).
                // This check stays in VISIBLE-bars terms rather than
                // recomputing against `bar_count_scroll` (total loaded) —
                // `view_start <= visible_bars * 0.20` means the viewport's
                // oldest visible bar is within the last 20% of one screen
                // width from the oldest LOADED bar. That is the same
                // "within 20% of loaded history" rule restated in the units
                // this call site already tracks, and is exactly equivalent
                // to it whenever the loaded series is not dramatically
                // larger than one screen (the common case right after a
                // fresh open or a scroll-driven ramp) — see
                // `BackfillEngine::on_kline_scroll_pressure`'s doc comment
                // for the full mapping note. The call below now feeds a
                // scroll-PRESSURE signal into `BackfillEngine`'s ramp
                // instead of a one-shot fixed-size page fetch.
                let visible = window.viewport().visible_bars() as f64;
                let threshold = (visible * 0.20).max(5.0);
                if window.viewport().view_start > threshold { continue; }

                if max_loaded > 0 && bar_count_scroll >= max_loaded as usize { continue; }

                let (win_sym_sc, win_tf_sc, win_exch_sc, win_at_sc) = match instrument_by_cid_scroll.get(cid_scroll) {
                    Some(t) => t,
                    None => continue,
                };

                if is_derived {
                    self.core.bubbles_mut().with_bubble_mut(bid_scroll, |b| {
                        b.derived_seed_in_flight = Some((web_time::Instant::now(), bar_count_scroll));
                    });
                    derived_seed_requests.push((
                        *cid_scroll,
                        chart_type,
                        win_sym_sc.clone(),
                        win_exch_sc.clone(),
                        *win_at_sc,
                    ));
                } else {
                    self.core.bubbles_mut().with_bubble_mut(bid_scroll, |b| {
                        b.scroll_fetch_in_flight = true;
                        b.scroll_fetch_started = Some(web_time::Instant::now());
                        // New user-triggered crossing — reset the lazy
                        // prefetch guard (Task 2, derived-prepend-fold-plan-
                        // 2026-07-07) so this crossing gets its own "one
                        // batch now, one batch lazy" pair instead of
                        // inheriting the previous crossing's `done` flag.
                        b.scroll_prefetch_done = false;
                    });
                    scroll_requests.push((
                        *cid_scroll,
                        win_sym_sc.clone(),
                        win_exch_sc.clone(),
                        win_tf_sc.name.clone(),
                        *win_at_sc,
                    ));
                }
            }

            for (cid_req, symbol, exchange, tf_name, account_type) in scroll_requests {
                // Read the OLDEST bar's ts from the active bubble for this chart.
                // The pre-cutover code path stored bars on the window directly;
                // it now lives on the bubble. The dangling `None::<&Bar>` here
                // made `oldest_ts` always 0, which made the next `continue` skip
                // EVERY scroll-fetch — left/scroll backfill silently no-op'd.
                let oldest_ts = self.panel_app.panel_grid.containers()
                    .get(&cid_req)
                    .and_then(|w| self.core.bubbles().bars(w.current_tag_id).first().map(|bar| bar.timestamp))
                    .unwrap_or(0);

                if oldest_ts == 0 { continue; }

                let eid = digdigdig3::ExchangeId::from_str(&exchange)
                    .unwrap_or(digdigdig3::ExchangeId::Binance);
                let at = mlc_backend_data_bridge::account_type_to_dig3(account_type);

                if let Some(tf) = parse_timeframe_name(&tf_name) {
                    self.core.request_scroll_backfill(eid, &symbol, &tf, at, oldest_ts, 500);
                }
            }

            for (cid_req, chart_type, symbol, exchange, account_type) in derived_seed_requests {
                // derived-prepend-fold-plan-2026-07-07, Step 5: only the 3
                // EMA kinds (tick_imbalance_bar/volume_imbalance_bar/run_bar)
                // still teardown+respawn — their fold state cannot be
                // reconstructed from emitted output (see
                // `Station::rewarm_derived`'s `RewarmUnsupported` doc
                // comment). Every other derived chart_type now rides
                // `Station::rewarm_derived` in place: the live station
                // subscription never stops, older points are spliced in
                // FRONT of the existing rendered slot, and arrive as ONE
                // batched `LiveUpdate::*HistoryPrepended` event that
                // `reconcile_derived_series`'s stamp-based prepend-shift
                // already handles correctly — clearing the slot first would
                // throw away exactly the data being extended.
                if crate::active_series::chart_type_deepen_is_teardown(chart_type) {
                    // Drop the stale (shorter) rendered slot BEFORE the reseed
                    // lands — the station-side ring-peek replay in `spawn_*_sub`
                    // (see `derived_bar_sub_body`/`renko_sub_body`/etc.) always
                    // replays the FULL newly-seeded ring on respawn, so clearing
                    // here and letting the replay repopulate is correct and
                    // avoids ever concatenating the old (shallow) series with the
                    // new (deep) one. Bar-family kinds (range/tick/volume/dollar/
                    // imbalance/run) share `bubble.bars` with kline — cleared via
                    // `clear_bars()`, same as `resolve_active_series`'s mapping
                    // for those chart_types.
                    //
                    // In LOCKSTEP: reset the requesting window's derived-series
                    // bookkeeping (`bar_count` + `derived_series_stamp`). Without
                    // this, the first post-clear `reconcile_derived_series` call
                    // (fired by the first `LiveUpdate::*BarUpdate` from the
                    // replay) sees `new_len=1` against the STALE
                    // `(old_head, old_last)` stamp, computes a garbage
                    // `prepended=1` shift, and stomps `bar_count` from its stale
                    // count to 1 — producing the one-frame viewport jump the
                    // owner reports on zoom-out lazy-deepen. Preserving
                    // `view_start` here is deliberate: zoom-out means the user
                    // is mid-history and expects the position to hold, NOT to
                    // snap to end (unlike `ChangeChartType` which resets to
                    // fresh right-edge). See
                    // `ChartContainer::reset_derived_stamp_for_deepen`.
                    // "cvd" never reaches this branch: it isn't registered
                    // in `mlc_core::chart_type` (demoted from chart_type to
                    // overlay), so `chart_type_deepen_is_teardown("cvd")`
                    // resolves via `by_id(..).unwrap_or(false)` to `false` —
                    // CVD always takes the rewarm-in-place `else` path below
                    // (derived-prepend-fold-plan-2026-07-07, Task 3: removed
                    // the dead `chart_type == "cvd"` clear_cvd fallback that
                    // could never execute).
                    if let Some(bid) = self.bubble_for_chart(cid_req) {
                        self.core.bubbles_mut().with_bubble_mut(bid, |bubble| {
                            if let Some(ops) = mlc_backend::chart_type::ops_by_id(chart_type) {
                                (ops.clear_slot)(bubble);
                            }
                        });
                    }
                    if let Some(window) = self.panel_app.panel_grid.containers_mut().get_mut(&cid_req) {
                        window.reset_derived_stamp_for_deepen();
                    }
                }

                let eid = digdigdig3::ExchangeId::from_str(&exchange)
                    .unwrap_or(digdigdig3::ExchangeId::Binance);
                let at = mlc_backend_data_bridge::account_type_to_dig3(account_type);
                self.core.bridge_handle().request_deeper_seed_for_active(eid, &symbol, at);
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
                im.instances_for_bubble(w.current_tag_id)
                    .iter()
                    .any(|i| i.values.is_empty() && !attempted.contains(&i.id))
            })
        };
        if any_unhealed {
            let window_ids: Vec<(u64, mlc_core::state::ChartId)> = self
                .panel_app
                .panel_grid
                .iter_containers()
                .map(|(_, w)| (w.current_tag_id.0, w.id))
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
