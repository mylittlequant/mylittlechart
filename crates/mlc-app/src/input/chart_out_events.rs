//! Process ChartOutEvent variants  -  propagates chart-internal events
//! (viewport, timeframe, symbol, indicator, drawing changes) to sync groups,
//! tag manager, autosave, and other downstream consumers.

use crate::ChartApp;
use mlc_core::UIStyle;
use mlc_core::state::modal_state::OpenModal;

impl ChartApp {
    /// Drain the cure work LoadPreset's warm path deferred. Called by the host
    /// AFTER the first frame following a preset swap has been presented so
    /// the user sees loading skeletons + spinners during the request_bars
    /// sweep instead of staring at the old chart while it runs synchronously.
    pub fn drain_pending_cure_after_preset_load(&mut self) {
        if self.pending_cure_after_preset_load.is_empty() { return; }
        let work = std::mem::take(&mut self.pending_cure_after_preset_load);
        let bar_count = self.panel_app.user_manager.profile.bar_count as usize;
        let drain_t0 = web_time::Instant::now();
        let n = work.len();
        for (eid, sym, tf, at, force) in work {
            self.core.bridge.ensure_connector(eid);
            self.core.bridge.request_bars(eid, &sym, &tf, at, None, Some(bar_count), force);
        }
        eprintln!("[ChartApp] drained {} cure jobs in {}us", n, drain_t0.elapsed().as_micros());
    }

    /// Seed `pending_background_warm` once with every `open_tabs` entry
    /// that is NOT the active preset and NOT already in
    /// `live_preset_cache`. Called by the host on every frame; idempotent
    /// after the first successful seed (guarded by `background_warm_seeded`).
    pub fn seed_background_warm_if_idle(&mut self) {
        if self.background_warm_seeded { return; }
        // Only seed once we're fully Live — seeding during a cold load
        // means we'd race against drain_pending_cold_apply.
        if self.load_state.is_loading() { return; }
        if self.pending_cold_apply.is_some() { return; }
        // Need an active preset materialised first. Otherwise the
        // background-warm dance has no "current" state to park/unpark
        // against.
        if self.panel_app.active_preset_id.is_empty() { return; }
        let active = self.panel_app.active_preset_id.clone();
        if !self
            .panel_app
            .panel_grid
            .iter_containers()
            .any(|(_, w)| self.bubble_for_chart(w.id).is_some())
        {
            // Active slot not yet materialised (still showing the
            // promote-skeleton). Defer to the next frame.
            return;
        }
        for tab_id in self.panel_app.open_tabs.clone().into_iter() {
            if tab_id == active { continue; }
            if tab_id == "__default__" { continue; }
            if self.live_preset_cache.contains_key(&tab_id) { continue; }
            if !self.core.presets().contains_key(&tab_id) { continue; }
            self.pending_background_warm.push_back(tab_id);
        }
        self.background_warm_seeded = true;
        if !self.pending_background_warm.is_empty() {
            eprintln!(
                "[ChartApp] background-warm seeded: {} open_tab preset(s) queued for pre-hydration",
                self.pending_background_warm.len(),
            );
        }
    }

    /// Hydrate ONE queued background-warm preset and park it directly into
    /// `live_preset_cache`. Called by the host once per frame from
    /// `about_to_wait`. Each call: park current active → cold-apply target
    /// → park target → unpark original. All within one event-loop tick so
    /// no intermediate frame paints — owner sees zero flicker.
    ///
    /// Bails out if a cold apply is already in flight or load_state is
    /// not Live. Returns true if it processed an entry, false otherwise.
    pub fn drain_one_background_warm(&mut self) -> bool {
        if self.load_state.is_loading() { return false; }
        if self.pending_cold_apply.is_some() { return false; }
        let Some(target_id) = self.pending_background_warm.pop_front() else {
            return false;
        };
        // Skip if it got into the cache via a user click in the meantime.
        if self.live_preset_cache.contains_key(&target_id) {
            return true; // counted as drained, just nothing to do
        }
        let Some(preset) = self.core.presets().get(&target_id).cloned() else {
            return true; // gone — silently drop
        };
        let active = self.panel_app.active_preset_id.clone();
        if active.is_empty() {
            // No active preset to swap around — re-queue and bail.
            self.pending_background_warm.push_front(target_id);
            return false;
        }

        let t0 = web_time::Instant::now();
        // 1. Park current active into live_preset_cache.
        self.park_active_preset(&active);
        // 2. Cold-apply the target preset into the now-placeholder slot.
        //    Use synthetic zero timings — these are diagnostic-only logs.
        let lp_total = web_time::Instant::now();
        self.panel_app.active_preset_id = target_id.clone();
        self.apply_preset_cold(preset, lp_total, 0, 0, 0, 0, 0);
        // 3. Park target into live_preset_cache.
        self.park_active_preset(&target_id);
        // 4. Unpark original active.
        let unpark_ok = self.unpark_preset(&active);
        self.panel_app.active_preset_id = active.clone();
        if !unpark_ok {
            eprintln!(
                "[ChartApp] background-warm WARNING: failed to unpark active '{}' after warming '{}' — active slot left as placeholder",
                active, target_id,
            );
        }
        eprintln!(
            "[ChartApp] background-warm processed '{}' in {}us (queue {} remaining)",
            target_id,
            t0.elapsed().as_micros(),
            self.pending_background_warm.len(),
        );
        true
    }

    /// Drain a cold-path preset that was stashed by `LoadPreset` one frame
    /// ago. The host calls this AFTER the GpuDone for the skeleton frame
    /// (so the user has already seen "loading skeleton + spinner") and
    /// then runs `apply_preset_cold` synchronously on the main thread.
    ///
    /// While the stash is in flight, `load_state == Loading`. Cleared back
    /// to `Live` here once apply finishes.
    pub fn drain_pending_cold_apply(&mut self) {
        let Some(stash) = self.pending_cold_apply.take() else { return; };
        let waited_us = stash.stashed_at.elapsed().as_micros();
        eprintln!(
            "[ChartApp] draining pending cold apply for preset '{}' (waited {}us for skeleton frame)",
            stash.preset.name, waited_us,
        );
        self.apply_preset_cold(
            stash.preset,
            stash.timings.lp_total,
            stash.timings.lp_materialized_us,
            stash.timings.lp_autosave_us,
            stash.timings.lp_park_us,
            stash.timings.lp_unpark_us,
            stash.timings.lp_clone_us,
        );
        // Cold body finished — autosave + collect_state_into_preset are
        // safe again. Future flush_pending_autosave runs will persist the
        // newly materialised state to disk normally.
        self.load_state = crate::persistence::LoadState::Live;
    }

    /// Process a `ChartOutEvent`  -  route high-level outcomes back to state.
    pub(crate) fn process_chart_out_event(&mut self, event: mlc_core::events::ChartOutEvent) {
        use mlc_core::events::ChartOutEvent;
        use mlc_core::state::Timeframe;

        // Tracks whether this event mutated chart state (triggers autosave).
        let mut state_mutated = false;

        match event {
            ChartOutEvent::ChangeTimeframe { timeframe_id } => {
                eprintln!("[ChartApp] ChangeTimeframe: {}", timeframe_id);
                let timeframe = match timeframe_id.as_str() {
                    "tf_1t"    => Some(Timeframe::t1()),
                    "tf_10t"   => Some(Timeframe::t10()),
                    "tf_100t"  => Some(Timeframe::t100()),
                    "tf_1000t" => Some(Timeframe::t1000()),
                    "tf_1m"  => Some(Timeframe::m1()),
                    "tf_3m"  => Some(Timeframe::new("3m", 3 * 60_000)),
                    "tf_5m"  => Some(Timeframe::m5()),
                    "tf_15m" => Some(Timeframe::m15()),
                    "tf_30m" => Some(Timeframe::m30()),
                    "tf_1h"  => Some(Timeframe::h1()),
                    "tf_2h"  => Some(Timeframe::new("2H", 2 * 3_600_000)),
                    "tf_4h"  => Some(Timeframe::h4()),
                    "tf_6h"  => Some(Timeframe::new("6H", 6 * 3_600_000)),
                    "tf_12h" => Some(Timeframe::new("12H", 12 * 3_600_000)),
                    "tf_1d"  => Some(Timeframe::d1()),
                    "tf_1w"  => Some(Timeframe::w1()),
                    "tf_1M"  => Some(Timeframe::mn1()),
                    _ => {
                        eprintln!("[ChartApp] unknown timeframe_id: {}", timeframe_id);
                        None
                    }
                };
                if let Some(tf) = timeframe {
                    // Phase 4 S2.2: capture chart_id BEFORE mutating block.
                    // Phase 4 S2.7.1c: reads via branch_meta (B1 flip).
                    let active_chart_id = self.panel_app.panel_grid.active_container_id();
                    // Capture previous timeframe and active leaf BEFORE changing.
                    let previous_timeframe = active_chart_id
                        .and_then(|cid| self.timeframe_for_chart(cid));
                    let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
                    // Extract whether timeframe actually changed before mutating.
                    // prev_tf_opt: Some(prev) means there was a window; None means no window.
                    let prev_tf_opt = previous_timeframe;
                    let tf_changed = match prev_tf_opt {
                        Some(ref prev_tf) if *prev_tf != tf => {
                            self.push_undo_command(mlc_core::Command::ChangeTimeframe {
                                previous_timeframe: prev_tf.clone(),
                                new_timeframe: tf.clone(),
                            });
                            eprintln!("[ChartApp] Recorded ChangeTimeframe {} -> {}", prev_tf.name, tf.name);
                            true
                        }
                        Some(_) => false,
                        None => true,
                    };
                    // Set timeframe, clear bars, and request new data asynchronously.
                    // (change_timeframe tries a sync cache lookup which fails for live data.)
                    let symbol = active_chart_id
                        .and_then(|cid| self.symbol_for_chart(cid))
                        .map(String::from)
                        .unwrap_or_default();
                    let at_label_tf = active_chart_id
                        .and_then(|cid| self.account_type_for_chart(cid))
                        .map(String::from)
                        .unwrap_or_else(|| "S".to_string());
                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                        {};
                        window.viewport.bar_count = 0;
                        window.viewport.view_start = 0.0;
                        // Mark a fresh load so a stray TradeUpdate can't synthesize a
                        // bar before BarsLoaded arrives (which would strand view_start
                        // at 0 = oldest bar at the left edge).
                        window.pending_symbol_load = true;
                        // Snap to right edge once first batch lands — without this
                        // the last bar sits at view_start=0 (left edge) after TF switch.
                        window.needs_snap_after_backfill = true;
                        // NOTE: the X-axis kind (index vs time) is reconciled centrally
                        // in prepare_frame from tf.is_tick() + real bar timestamps — do
                        // NOT set time_axis here (it has no bars to anchor from yet).
                    }
                    if let Some(chart_id) = active_chart_id {
                        if let Some(bid) = self.bubble_for_chart(chart_id) {
                            if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(bid) {
                                bubble.timeframe = tf.clone();
                            }
                        }
                    }
                    // Fetch data for the new timeframe. A tick TF subscribes to the
                    // dig3 TickBar stream (N trades/bar); a time TF fetches klines.
                    // Either way switch ABORTS any prior tick subscription so a
                    // stale tick forwarder can't keep feeding the window.
                    if !symbol.is_empty() {
                        let eid_str = self.active_exchange.as_str();
                        if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                            eprintln!("[ChartApp] Exchange {} is disabled, skipping data fetch (timeframe change)", eid_str);
                        } else {
                            let at = crate::account_type_from_label(&at_label_tf);
                            // Drop the OLD time-TF kline forwarder so it stops piping
                            // 1h BarUpdates to the broadcast after the user switched
                            // to 1m. abort_kline_bars is a no-op for tick TFs (the
                            // prior tick subscription is owned by abort_tick_bars).
                            if let Some(ref prev_tf) = prev_tf_opt {
                                if prev_tf.tick_count().is_none() {
                                    self.core.bridge.abort_kline_bars(self.active_exchange, &symbol, at, &prev_tf.name);
                                }
                            }
                            if let Some(count) = tf.tick_count() {
                                self.core.bridge.request_tick_bars(self.active_exchange, &symbol, at, count);
                            } else {
                                self.core.bridge.abort_tick_bars(self.active_exchange, &symbol, at);
                                self.core.bridge.request_bars(self.active_exchange, &symbol, &tf, at, None, Some(self.panel_app.user_manager.profile.bar_count as usize), true);
                            }
                        }
                    }
                    // Propagate new timeframe to all leaves in the same sync group.
                    if tf_changed {
                        if let Some(leaf) = active_leaf {
                            self.propagate_timeframe_to_sync_group(leaf, tf);
                        }
                        self.sidebar_data_dirty = true;
                        state_mutated = true;
                    }
                }
            }

            ChartOutEvent::ChangeChartType { chart_type } => {
                eprintln!("[ChartApp] ChangeChartType: {}", chart_type);
                // Map the incoming String to a &'static str used by ChartContainer.
                let static_type: Option<&'static str> = match chart_type.as_str() {
                    "candles"       => Some("candles"),
                    "hollow_candles" => Some("hollow_candles"),
                    "heikin_ashi"   => Some("heikin_ashi"),
                    "bars"          => Some("bars"),
                    "line"          => Some("line"),
                    "step_line"     => Some("step_line"),
                    "line_markers"  => Some("line_markers"),
                    "area"          => Some("area"),
                    "hlc_area"      => Some("hlc_area"),
                    "baseline"      => Some("baseline"),
                    "histogram"     => Some("histogram"),
                    "columns"       => Some("columns"),
                    // Phase 2C: derived bar streams from dig3-station — own
                    // subscription, no per-frame folding. Bars arrive via
                    // LiveUpdate::BarUpdate from the station's
                    // TradeToRangeBarDerived / TradeToVolumeBarDerived /
                    // TradeToFootprintDerived forwarders.
                    "range_bar"     => Some("range_bar"),
                    "volume_bar"    => Some("volume_bar"),
                    "tick_bar"      => Some("tick_bar"),
                    "footprint"     => Some("footprint"),
                    "renko"         => Some("renko"),
                    "pnf"           => Some("pnf"),
                    "kagi"          => Some("kagi"),
                    "cvd"           => Some("cvd"),
                    "tpo"           => Some("tpo"),
                    "three_line_break" => Some("three_line_break"),
                    // ── López de Prado quant bars (C9) ────────────────────────
                    "dollar_bar"           => Some("dollar_bar"),
                    "tick_imbalance_bar"   => Some("tick_imbalance_bar"),
                    "volume_imbalance_bar" => Some("volume_imbalance_bar"),
                    "run_bar"              => Some("run_bar"),
                    // ── DOM Heatmap (C12) ──────────────────────────────────────
                    "dom_heatmap"          => Some("dom_heatmap"),
                    // ── Liquidation Heatmap (C13) ──────────────────────────────
                    "liquidation_heatmap"  => Some("liquidation_heatmap"),
                    _ => {
                        eprintln!("[ChartApp] unknown chart_type: {}", chart_type);
                        None
                    }
                };
                if let Some(ct) = static_type {
                    let pid = self.panel_app.panel_grid
                        .active_container()
                        .map(|w| w.chart_panel_id);
                    let (chart_param, chart_interval): (f64, &'static str) = if let Some(chart_panel_id) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(
                            &mut self.panels_store, chart_panel_id,
                        );
                        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                            window.set_chart_type_with_undo(panel, ct);
                        }
                        (panel.chart_type_param, panel.chart_type_interval)
                    } else {
                        (0.0, "1m")
                    };

                    // ── Layer 1 (derived-bars audit 2026-06-17): clean state ──
                    //
                    // Every chart_type switch must:
                    //   1. abort the previous derived subscription (kline-OR-
                    //      derived → any). If we don't, switching range_bar →
                    //      volume_bar leaves the range_bar forwarder running,
                    //      which keeps pushing BarUpdate (range timestamps)
                    //      into a window that just became volume_bar.
                    //   2. clear `window.bars` — the old timestamp grid is
                    //      semantically incompatible with the new stream
                    //      (kline 1H ts every 3600s vs range bars ts at
                    //      crossing trade ms; mixing both = the "crossing
                    //      timestamps" visual owner described).
                    //   3. flip viewport axis mode to match: derived bar
                    //      streams are non-uniform in time → time_axis.
                    //      Time-uniform candles (kline) → index axis.
                    //   4. reset view_start so the next batch of bars
                    //      doesn't land off-screen.
                    //   5. set `pending_symbol_load = true` so renderer
                    //      shows the loading skeleton until the first new
                    //      bar arrives.
                    let is_derived_target = matches!(
                        ct,
                        "range_bar" | "volume_bar" | "tick_bar" | "footprint"
                        | "renko" | "pnf" | "kagi" | "cvd" | "tpo"
                        | "three_line_break"
                        | "dollar_bar" | "tick_imbalance_bar"
                        | "volume_imbalance_bar" | "run_bar"
                        | "dom_heatmap"
                        | "liquidation_heatmap"
                    );
                    if let Some(active_leaf) = self.panel_app.panel_grid.active_container() {
                        let cid = active_leaf.id;
                        let symbol_opt = self.symbol_for_chart(cid);
                        let at_label_opt = self.account_type_for_chart(cid);
                        let tf_opt = self.timeframe_for_chart(cid);
                        if let (Some(symbol), Some(at_label), Some(tf)) = (symbol_opt, at_label_opt, tf_opt) {
                            if !symbol.is_empty() {
                                let eid_str = self.active_exchange.as_str();
                                let connector_enabled = self.sidebar_state.connector_enabled
                                    .get(eid_str).copied().unwrap_or(true);
                                if connector_enabled {
                                    let at = crate::account_type_from_label(&at_label);
                                    let on_tick_tf = tf.tick_count().is_some();

                                    // Step 1: always abort any derived
                                    // subscription. The bridge.abort_* call is
                                    // idempotent when nothing's live.
                                    self.core.bridge.abort_derived_chart(
                                        self.active_exchange, &symbol, at,
                                    );

                                    // Steps 2-5: mutate the active window.
                                    // NOTE: axis mode (time vs index) is
                                    // reconciled centrally in prepare_frame
                                    // from chart_type + tf.is_tick(). DON'T
                                    // toggle viewport.time_axis here — without
                                    // real bar timestamps the anchor would be
                                    // wrong, and the next prepare_frame would
                                    // overwrite it anyway.
                                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                                        {};
                                        window.viewport.bar_count = 0;
                                        window.viewport.view_start = 0.0;
                                        window.pending_symbol_load = true;
                                        // Mark "snap to end on first data" so
                                        // the new stream lands at the right
                                        // edge of the viewport.
                                        window.needs_snap_after_backfill = true;
                                        // Phase X0 / C5c: DOM / liquidation heatmap storage now lives on
                                        // the bubble. Cleanup on chart-type switch deferred: bubble
                                        // can hold stale buffers, but the renderer only reads them when
                                        // a chart_type matching dom_heatmap / liquidation_heatmap is
                                        // active, so the bleed never reaches the user. If a future
                                        // cleanup is desired, use bubble.clear_dom_heatmap() etc.
                                    }
                                    let _ = is_derived_target; // axis flip done in prepare_frame

                                    // Step 6: fire the appropriate
                                    // subscription for the new mode.
                                    match ct {
                                        "range_bar" => {
                                            let units = if chart_param > 0.0 { chart_param } else { 10.0 };
                                            let scaled = (units * 1e8) as u64;
                                            self.core.bridge.request_range_bars(self.active_exchange, &symbol, at, scaled);
                                        }
                                        "volume_bar" => {
                                            let units = if chart_param > 0.0 { chart_param } else { 1000.0 };
                                            let scaled = (units * 1e8) as u64;
                                            self.core.bridge.request_volume_bars(self.active_exchange, &symbol, at, scaled);
                                        }
                                        "footprint" => {
                                            let iv = if !chart_interval.is_empty() { chart_interval } else { "1m" };
                                            self.core.bridge.request_footprint(self.active_exchange, &symbol, at, iv);
                                        }
                                        "renko" => {
                                            // Default: 0.5 % of last close for the box size; reversal = 2 (TradeStation classic).
                                            let units = if chart_param > 0.0 { chart_param } else { 10.0 };
                                            let scaled = (units * 1e8) as u64;
                                            self.core.bridge.request_renko_bars(self.active_exchange, &symbol, at, scaled, 2);
                                        }
                                        "pnf" => {
                                            // Default reversal = 3 (TradeStation classic).
                                            let units = if chart_param > 0.0 { chart_param } else { 10.0 };
                                            let scaled = (units * 1e8) as u64;
                                            self.core.bridge.request_pnf_bars(self.active_exchange, &symbol, at, scaled, 3);
                                        }
                                        "kagi" => {
                                            // Default reversal = 1 % of current price expressed as absolute units.
                                            let units = if chart_param > 0.0 { chart_param } else { 10.0 };
                                            let scaled = (units * 1e8) as u64;
                                            self.core.bridge.request_kagi_bars(self.active_exchange, &symbol, at, scaled);
                                        }
                                        "cvd" => {
                                            self.core.bridge.request_cvd_line(self.active_exchange, &symbol, at);
                                        }
                                        "tpo" => {
                                            // Default 30-min letter buckets, Trade-source path (sub-tick precision, fast cold start).
                                            let freq = if chart_param > 0.0 { chart_param as u16 } else { 30 };
                                            self.core.bridge.request_tpo(
                                                self.active_exchange, &symbol, at, freq,
                                                mlc_live_data::TpoSource::TradeBucket,
                                            );
                                        }
                                        "three_line_break" => {
                                            // Default N=3 (industry standard).
                                            let lines_back = if chart_param > 0.0 {
                                                (chart_param as u8).max(1)
                                            } else {
                                                3u8
                                            };
                                            self.core.bridge.request_three_line_break(
                                                self.active_exchange, &symbol, at, lines_back,
                                            );
                                        }
                                        // ── López de Prado quant bars (C9) ────
                                        "dollar_bar" => {
                                            // Default threshold = $1,000,000 (typical liquid futures).
                                            let threshold = if chart_param > 0.0 {
                                                chart_param as u64
                                            } else {
                                                1_000_000u64
                                            };
                                            self.core.bridge.request_dollar_bars(
                                                self.active_exchange, &symbol, at, threshold,
                                            );
                                        }
                                        "tick_imbalance_bar" => {
                                            // alpha=0.20 (20%), min_ticks=50.
                                            let min_ticks = if chart_param > 0.0 {
                                                chart_param as u32
                                            } else {
                                                50u32
                                            };
                                            self.core.bridge.request_tick_imbalance_bars(
                                                self.active_exchange, &symbol, at, 20, min_ticks,
                                            );
                                        }
                                        "volume_imbalance_bar" => {
                                            let min_ticks = if chart_param > 0.0 {
                                                chart_param as u32
                                            } else {
                                                50u32
                                            };
                                            self.core.bridge.request_volume_imbalance_bars(
                                                self.active_exchange, &symbol, at, 20, min_ticks,
                                            );
                                        }
                                        "run_bar" => {
                                            let min_ticks = if chart_param > 0.0 {
                                                chart_param as u32
                                            } else {
                                                50u32
                                            };
                                            self.core.bridge.request_run_bars(
                                                self.active_exchange, &symbol, at, 20, min_ticks,
                                            );
                                        }
                                        // ── DOM Heatmap (C12) ─────────────────
                                        "dom_heatmap" => {
                                            self.core.bridge.request_dom_heatmap(
                                                self.active_exchange, &symbol, at,
                                            );
                                        }
                                        // ── Liquidation Heatmap (C13) ─────────
                                        "liquidation_heatmap" => {
                                            self.core.bridge.request_liquidation_heatmap(
                                                self.active_exchange, &symbol, at,
                                            );
                                        }
                                        _ => {
                                            // Plain render type (candle /
                                            // bars / line / ...). Kline
                                            // request only when NOT on a
                                            // tick TF (tick TF flips into
                                            // derived path above).
                                            if !on_tick_tf {
                                                self.core.bridge.request_bars(
                                                    self.active_exchange, &symbol, &tf, at,
                                                    None,
                                                    Some(self.panel_app.user_manager.profile.bar_count as usize),
                                                    true,
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    state_mutated = true;
                }
            }

            ChartOutEvent::UpdateChartTypeParam { value } => {
                eprintln!("[ChartApp] UpdateChartTypeParam: {}", value);
                if value <= 0.0 {
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    return;
                }
                let active_ct = self.panel_app.toolbar_state.active_param_chart_type
                    .clone()
                    .unwrap_or_default();
                let pid = self.panel_app.panel_grid
                    .active_container()
                    .map(|w| w.chart_panel_id);
                if let Some(chart_panel_id) = pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(
                        &mut self.panels_store, chart_panel_id,
                    );
                    panel.chart_type_param = value;
                }
                if let Some(active_leaf) = self.panel_app.panel_grid.active_container() {
                    let cid = active_leaf.id;
                    let symbol_opt = self.symbol_for_chart(cid);
                    let at_label_opt = self.account_type_for_chart(cid);
                    if let (Some(symbol), Some(at_label)) = (symbol_opt, at_label_opt) {
                        if !symbol.is_empty() {
                            let eid_str = self.active_exchange.as_str();
                            if self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                                let at = crate::account_type_from_label(&at_label);
                                let scaled = (value * 1e8) as u64;
                                match active_ct.as_str() {
                                    "range_bar"  => self.core.bridge.request_range_bars(self.active_exchange, &symbol, at, scaled),
                                    "volume_bar" => self.core.bridge.request_volume_bars(self.active_exchange, &symbol, at, scaled),
                                    "renko"      => self.core.bridge.request_renko_bars(self.active_exchange, &symbol, at, scaled, 2),
                                    "pnf"        => self.core.bridge.request_pnf_bars(self.active_exchange, &symbol, at, scaled, 3),
                                    "kagi"       => self.core.bridge.request_kagi_bars(self.active_exchange, &symbol, at, scaled),
                                    // Quant bars (C9): param is reinterpreted per bar type.
                                    // dollar_bar: param = dollar threshold (whole dollars).
                                    "dollar_bar" => {
                                        let threshold = if value > 0.0 { value as u64 } else { 1_000_000u64 };
                                        self.core.bridge.request_dollar_bars(self.active_exchange, &symbol, at, threshold);
                                    }
                                    // imbalance/run bars: param = min_ticks floor.
                                    "tick_imbalance_bar" => {
                                        let min_ticks = if value > 0.0 { value as u32 } else { 50 };
                                        self.core.bridge.request_tick_imbalance_bars(self.active_exchange, &symbol, at, 20, min_ticks);
                                    }
                                    "volume_imbalance_bar" => {
                                        let min_ticks = if value > 0.0 { value as u32 } else { 50 };
                                        self.core.bridge.request_volume_imbalance_bars(self.active_exchange, &symbol, at, 20, min_ticks);
                                    }
                                    "run_bar" => {
                                        let min_ticks = if value > 0.0 { value as u32 } else { 50 };
                                        self.core.bridge.request_run_bars(self.active_exchange, &symbol, at, 20, min_ticks);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                state_mutated = true;
            }

            ChartOutEvent::UpdateChartTypeInterval { interval } => {
                eprintln!("[ChartApp] UpdateChartTypeInterval: {}", interval);
                let static_iv: &'static str = match interval.trim() {
                    "1m"  => "1m",
                    "5m"  => "5m",
                    "15m" => "15m",
                    "30m" => "30m",
                    "1h"  => "1h",
                    "4h"  => "4h",
                    _ => {
                        self.input_coordinator.borrow_mut().text_fields_mut().blur();
                        return;
                    }
                };
                let pid = self.panel_app.panel_grid
                    .active_container()
                    .map(|w| w.chart_panel_id);
                if let Some(chart_panel_id) = pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(
                        &mut self.panels_store, chart_panel_id,
                    );
                    panel.chart_type_interval = static_iv;
                }
                if let Some(active_leaf) = self.panel_app.panel_grid.active_container() {
                    let cid = active_leaf.id;
                    let symbol_opt = self.symbol_for_chart(cid);
                    let at_label_opt = self.account_type_for_chart(cid);
                    if let (Some(symbol), Some(at_label)) = (symbol_opt, at_label_opt) {
                        if !symbol.is_empty() {
                            let eid_str = self.active_exchange.as_str();
                            if self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                                let at = crate::account_type_from_label(&at_label);
                                self.core.bridge.request_footprint(self.active_exchange, &symbol, at, static_iv);
                            }
                        }
                    }
                }
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                state_mutated = true;
            }
            ChartOutEvent::Consumed => {}
            ChartOutEvent::ToggleIndicators => {
                self.modal_state.toggle(OpenModal::IndicatorSearch);
                if self.modal_state.current == OpenModal::IndicatorSearch {
                    let ind_id = uzor::WidgetId::from(crate::text_input::INDICATOR_SEARCH);
                    let q = self.modal_state.search_query.clone();
                    self.input_coordinator.borrow_mut().text_fields_mut().set_text(&ind_id, &q);
                    self.input_coordinator.borrow_mut().text_fields_mut().begin_edit(&ind_id);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::INDICATOR_SEARCH);
                } else {
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                }
                eprintln!("[ChartApp] indicator search modal toggled: {:?}", self.modal_state.current);
            }
            ChartOutEvent::OpenSymbolSearch => {
                self.modal_state.open(OpenModal::SymbolSearch);
                self.modal_state.symbol_search_results =
                    crate::ChartApp::build_demo_symbol_results("", &self.core.watchlist(), &self.core.exchange_symbols_read());
                {
                    let sym_id = uzor::WidgetId::from(crate::text_input::SYMBOL_SEARCH);
                    self.input_coordinator.borrow_mut().text_fields_mut().clear(&sym_id);
                    self.input_coordinator.borrow_mut().text_fields_mut().begin_edit(&sym_id);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::SYMBOL_SEARCH);
                }
                eprintln!("[ChartApp] symbol search modal opened");
            }
            ChartOutEvent::ApplySymbol { chart_id, symbol, exchange, account_type } => {
                use mlc_core::state::ChartId;
                let target_cid = ChartId(chart_id);

                let resolved_exchange = self.core.exchange_symbols_read()
                    .keys()
                    .find(|eid| eid.as_str() == exchange.as_str())
                    .copied()
                    .unwrap_or(self.active_exchange);

                let previous_symbol: String = self.symbol_for_chart(target_cid)
                    .map(String::from)
                    .unwrap_or_default();
                let old_trade_at = self.account_type_for_chart(target_cid)
                    .map(|s| crate::account_type_from_label(&s))
                    .unwrap_or(digdigdig3::AccountType::Spot);
                let old_trade_exchange = self.exchange_for_chart(target_cid)
                    .and_then(|s| self.core.exchange_symbols_read().keys().find(|eid| eid.as_str() == s.as_str()).copied())
                    .unwrap_or(self.active_exchange);

                let timeframe = self.timeframe_for_chart(target_cid)
                    .unwrap_or_default();

                let old_sym_search = previous_symbol.clone();
                let old_exch_search = self.exchange_for_chart(target_cid)
                    .map(String::from).unwrap_or_default();
                let old_at_search = self.account_type_for_chart(target_cid)
                    .map(String::from).unwrap_or_else(|| "S".to_string());

                let target_leaf = self.panel_app.panel_grid.iter_containers()
                    .find(|(_, w)| w.id == target_cid)
                    .map(|(lid, _)| lid);
                let target_pid = self.panel_app.panel_grid.containers().values()
                    .find(|w| w.id == target_cid)
                    .map(|w| w.chart_panel_id);

                if let Some(window) = self.panel_app.panel_grid.containers_mut().values_mut()
                    .find(|w| w.id == target_cid)
                {
                    if let Some(pid) = target_pid {
                        let mut guard = self.core.drawings_mut();
                        let dm = guard.entry_or_default(pid);
                        window.snapshot_drawings_for_symbol(&old_sym_search, &old_exch_search, &old_at_search, dm);
                        dm.set_current_symbol_key(&symbol, resolved_exchange.as_str(), &account_type);
                        dm.clear_all_primitives();
                        window.restore_drawings_for_symbol(&symbol, resolved_exchange.as_str(), &account_type, dm);
                    }
                    window.viewport.bar_count = 0;
                    window.viewport.view_start = 0.0;
                    window.pending_symbol_load = true;
                }

                if let Some(bid) = self.bubble_for_chart(target_cid) {
                    if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(bid) {
                        bubble.symbol = symbol.clone();
                        bubble.timeframe = timeframe.clone();
                        bubble.exchange = resolved_exchange.as_str().to_string();
                        bubble.account_type = account_type.clone();
                    }
                }

                self.active_exchange = resolved_exchange;
                if !previous_symbol.is_empty() {
                    self.core.bridge.unsubscribe_trades(old_trade_exchange, &previous_symbol, old_trade_at);
                }
                let eid_str = resolved_exchange.as_str();
                if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                    eprintln!("[ChartApp] ApplySymbol: exchange {} disabled, skipping connector call", eid_str);
                } else {
                    let at = crate::account_type_from_label(&account_type);
                    self.core.bridge.ensure_connector(resolved_exchange);
                    self.core.bridge.request_bars(
                        resolved_exchange, &symbol, &timeframe, at, None,
                        Some(self.panel_app.user_manager.profile.bar_count as usize), false,
                    );
                }

                if previous_symbol != symbol {
                    self.push_undo_command(mlc_core::Command::ChangeSymbol {
                        previous_symbol: previous_symbol.clone(),
                        new_symbol: symbol.clone(),
                    });
                    eprintln!("[ChartApp] ApplySymbol {} -> {} (chart {})", previous_symbol, symbol, chart_id);
                    if let Some(leaf) = target_leaf {
                        self.propagate_symbol_to_sync_group(leaf, &symbol, resolved_exchange.as_str(), &account_type);
                    }
                }

                let bars: Vec<mlc_core::Bar> = self.bubble_for_chart(target_cid)
                    .and_then(|bid| self.with_bubble(bid, |b| b.bars.clone()))
                    .unwrap_or_default();
                self.core.indicators_mut().calculate_all_for_window(chart_id, &bars);
                self.sync_sub_panes_from_manager();
                self.sidebar_data_dirty = true;
                self.autosave_snapshot();
                state_mutated = true;
            }
            ChartOutEvent::OpenCompareSearch => {
                self.modal_state.open(OpenModal::CompareSearch);
                self.modal_state.symbol_search_results =
                    crate::ChartApp::build_demo_symbol_results("", &self.core.watchlist(), &self.core.exchange_symbols_read());
                {
                    let cmp_id = uzor::WidgetId::from(crate::text_input::COMPARE_SEARCH);
                    self.input_coordinator.borrow_mut().text_fields_mut().clear(&cmp_id);
                    self.input_coordinator.borrow_mut().text_fields_mut().begin_edit(&cmp_id);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::COMPARE_SEARCH);
                }
                eprintln!("[ChartApp] compare search modal opened");
            }
            // Chart settings modal  -  opened by gear dropdown "Chart Settings..." item.
            ChartOutEvent::OpenChartSettings => {
                self.panel_app.chart_settings_state.toggle();
                eprintln!("[ChartApp] chart_settings modal toggled via settings dropdown: {}", self.panel_app.chart_settings_state.is_open);
            }

            // User settings modal  -  opened by the chrome gear button.
            ChartOutEvent::OpenUserSettings => {
                self.panel_app.user_settings_state.toggle();
                eprintln!("[ChartApp] user_settings modal toggled: {}", self.panel_app.user_settings_state.is_open);
            }

            // Quick-settings toggles from the settings gear dropdown.
            ChartOutEvent::ToggleGrid => {
                // Collect active panel id  -  drops grid borrow before panels_store borrow.
                let active_pid = self.panel_app.panel_grid.active_container()
                    .map(|w| w.chart_panel_id);
                let new_val = if let Some(pid) = active_pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(
                        &mut self.panels_store, pid,
                    );
                    let v = !panel.grid_options.vert_lines.visible;
                    panel.grid_options.vert_lines.visible = v;
                    panel.grid_options.horz_lines.visible = v;
                    Some((v, v))
                } else { None };
                if let Some((v, h)) = new_val {
                    // Broadcast to all panel states.
                    let all_pids: Vec<_> = self.panel_app.panel_grid.containers()
                        .values().map(|w| w.chart_panel_id).collect();
                    for pid in all_pids {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(
                            &mut self.panels_store, pid,
                        );
                        panel.grid_options.vert_lines.visible = v;
                        panel.grid_options.horz_lines.visible = h;
                    }
                }
                eprintln!("[ChartApp] grid toggled (all windows)");
                state_mutated = true;
            }
            ChartOutEvent::ToggleVolumeProfileOverlay => {
                let new_val = if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                    w.vp_show = !w.vp_show;
                    if !w.vp_show {
                        w.vp_overlay = None;
                        w.vp_overlay_stamp = None;
                    }
                    Some(w.vp_show)
                } else { None };
                eprintln!("[ChartApp] vp_show toggled -> {:?}", new_val);
                state_mutated = true;
            }
            ChartOutEvent::CycleVolumeProfileMode => {
                use mlc_core::chart::volume_profile::VpMode;
                if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                    w.vp_mode = match w.vp_mode {
                        VpMode::VisibleRange   => VpMode::Session,
                        VpMode::Session        => VpMode::Composite,
                        VpMode::Composite      => VpMode::VisibleRange,
                        VpMode::FixedRange { .. } => VpMode::VisibleRange,
                    };
                    w.vp_overlay = None;
                    w.vp_overlay_stamp = None;
                    eprintln!("[ChartApp] vp_mode cycled -> {:?}", w.vp_mode);
                }
                state_mutated = true;
            }
            // === C15: OI Delta sub-pane ===
            ChartOutEvent::ToggleOpenInterestOverlay => {
                let cid_opt = self.panel_app.panel_grid.active_container().map(|w| w.id);
                if let Some(cid) = cid_opt {
                    let symbol_opt = self.symbol_for_chart(cid);
                    let at_label_opt = self.account_type_for_chart(cid);
                    if let (Some(symbol), Some(at_label)) = (symbol_opt, at_label_opt) {
                        if !symbol.is_empty() {
                            let eid_str = self.active_exchange.as_str();
                            let connector_enabled = self.sidebar_state.connector_enabled
                                .get(eid_str).copied().unwrap_or(true);
                            if connector_enabled {
                                let at = crate::account_type_from_label(&at_label);
                                let oi_show_now = {
                                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                                        w.oi_show = !w.oi_show;
                                        eprintln!("[ChartApp] oi_show toggled -> {}", w.oi_show);
                                        w.oi_show
                                    } else { false }
                                };
                                // Phase X0 / C5c: oi_delta storage lives on bubble.
                                if let Some(bid) = self.bubble_for_chart(cid) {
                                    if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(bid) {
                                        bubble.clear_oi();
                                    }
                                }
                                if oi_show_now {
                                    self.core.bridge.request_open_interest(
                                        self.active_exchange, &symbol, at,
                                    );
                                }
                            }
                        }
                    }
                }
                state_mutated = true;
            }
            // === C15: Funding Rate strip ===
            ChartOutEvent::ToggleFundingRateStrip => {
                let cid_opt = self.panel_app.panel_grid.active_container().map(|w| w.id);
                if let Some(cid) = cid_opt {
                    let symbol_opt = self.symbol_for_chart(cid);
                    let at_label_opt = self.account_type_for_chart(cid);
                    if let (Some(symbol), Some(at_label)) = (symbol_opt, at_label_opt) {
                        if !symbol.is_empty() {
                            let eid_str = self.active_exchange.as_str();
                            let connector_enabled = self.sidebar_state.connector_enabled
                                .get(eid_str).copied().unwrap_or(true);
                            if connector_enabled {
                                let at = crate::account_type_from_label(&at_label);
                                let funding_show_now = {
                                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                                        w.funding_show = !w.funding_show;
                                        eprintln!("[ChartApp] funding_show toggled -> {}", w.funding_show);
                                        w.funding_show
                                    } else { false }
                                };
                                // Phase X0 / C5c: funding_rate storage lives on bubble.
                                if let Some(bid) = self.bubble_for_chart(cid) {
                                    if let Some(bubble) = self.core.bubble_mut().bubble.bubble_mut(bid) {
                                        bubble.clear_funding();
                                    }
                                }
                                if funding_show_now {
                                    self.core.bridge.request_funding_rate(
                                        self.active_exchange, &symbol, at,
                                    );
                                }
                            }
                        }
                    }
                }
                state_mutated = true;
            }
            ChartOutEvent::ToggleCrosshair => {
                let new_val = if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                    w.toggle_crosshair();
                    Some(w.crosshair.enabled)
                } else { None };
                if let Some(val) = new_val {
                    for w in self.panel_app.panel_grid.containers_mut().values_mut() {
                        w.crosshair.enabled = val;
                    }
                }
                eprintln!("[ChartApp] crosshair toggled (all windows)");
                state_mutated = true;
            }
            ChartOutEvent::ToggleLegend => {
                let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                if let Some(pid) = pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    panel.legend.visible = !panel.legend.visible;
                    eprintln!("[ChartApp] legend toggled -> {}", panel.legend.visible);
                }
                state_mutated = true;
            }
            ChartOutEvent::ToggleWatermark => {
                let active_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                let new_val = if let Some(pid) = active_pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    match &mut panel.watermark {
                        Some(wm) => { wm.visible = !wm.visible; Some(wm.visible) }
                        None => { panel.watermark = Some(mlc_core::Watermark::default()); Some(true) }
                    }
                } else { None };
                if let Some(val) = new_val {
                    let all_pids: Vec<_> = self.panel_app.panel_grid.containers().values().map(|w| w.chart_panel_id).collect();
                    for pid in all_pids {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        if let Some(ref mut wm) = panel.watermark { wm.visible = val; }
                    }
                }
                eprintln!("[ChartApp] watermark toggled (all windows)");
                state_mutated = true;
            }

            // Left panel  -  no sidebar in standalone mode.
            ChartOutEvent::ToggleLeftPanel => {
                eprintln!("[ChartApp] Not available in standalone mode: {:?}", event);
            }

            // Right sidebar panels  -  handled via SidebarState.
            // toggle_right_panel returns Option<(bool, f64)>:
            //   Some((true,  w))  ->  sidebar opened   ->  compensate viewport rightward
            //   Some((false, w))  ->  sidebar closed   ->  compensate viewport leftward
            //   None              ->  panel switched   ->  no viewport change needed
            ChartOutEvent::ToggleWatchlist => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_sidebar_content::state::RightSidebarPanel::Watchlist,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Watchlist panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleAlerts => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_sidebar_content::state::RightSidebarPanel::Alerts,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Alerts panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleObjectTree => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_sidebar_content::state::RightSidebarPanel::ObjectTree,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Object Tree panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleSignals => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_sidebar_content::state::RightSidebarPanel::Signals,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Signals panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleConnectors => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_sidebar_content::state::RightSidebarPanel::Connectors,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Connectors panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::TogglePerformance => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_sidebar_content::state::RightSidebarPanel::Performance,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Performance panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleAgents => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_sidebar_content::state::RightSidebarPanel::Agents,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Agents panel: {}", if opening { "opened" } else { "closed" });
                    // Spawn-on-demand: PTY only starts when user explicitly clicks [Start].
                    let _ = opening;
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleSlot(idx) => {
                let panel = mlc_sidebar_content::state::RightSidebarPanel::from_slot_index(idx);
                let _ = self.sidebar_state.toggle_right_panel(panel);
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            // Split layout events  -  wire to ChartPanelGrid split system.
            // After each split all new leaves receive a shared sync color tag so
            // that symbol / timeframe changes propagate across the split group.
            ChartOutEvent::InternalSplitHorizontal => { self.do_split(mlc_core::SplitKind::SplitRight); state_mutated = true; }
            ChartOutEvent::InternalSplitVertical => { self.do_split(mlc_core::SplitKind::SplitBottom); state_mutated = true; }
            ChartOutEvent::InternalSplitGrid2x2 => { self.do_split(mlc_core::SplitKind::Grid2x2); state_mutated = true; }
            ChartOutEvent::InternalSplit2Left1Right => { self.do_split(mlc_core::SplitKind::TwoLeftOneRight); state_mutated = true; }
            ChartOutEvent::InternalSplit1Left2Right => { self.do_split(mlc_core::SplitKind::OneLeftTwoRight); state_mutated = true; }
            ChartOutEvent::InternalSplit2Top1Bottom => { self.do_split(mlc_core::SplitKind::TwoTopOneBottom); state_mutated = true; }
            ChartOutEvent::InternalSplit1Top2Bottom => { self.do_split(mlc_core::SplitKind::OneTopTwoBottom); state_mutated = true; }
            ChartOutEvent::InternalSplit3Columns => { self.do_split(mlc_core::SplitKind::ThreeColumns); state_mutated = true; }
            ChartOutEvent::InternalSplit3Rows => { self.do_split(mlc_core::SplitKind::ThreeRows); state_mutated = true; }
            ChartOutEvent::InternalSplit1Big3Small => { self.do_split(mlc_core::SplitKind::OneBig3Small); state_mutated = true; }
            ChartOutEvent::InternalSetLayoutSingle => {
                self.panel_app.panel_grid.set_layout_single();
                state_mutated = true;
            }
            ChartOutEvent::InternalToggleExpand => {
                self.panel_app.panel_grid.toggle_expand();
                state_mutated = true;
            }
            ChartOutEvent::InternalClosePanel => {
                if let Some(leaf_id) = self.panel_app.panel_grid.docking().active_leaf() {
                    self.panel_app.panel_grid.close_leaf(leaf_id);
                }
                state_mutated = true;
            }
            ChartOutEvent::InternalResetSizes => {
                self.panel_app.panel_grid.reset_sizes();
                state_mutated = true;
            }
            ChartOutEvent::ToggleGridVertical => {
                let active_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                let new_val = if let Some(pid) = active_pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    panel.grid_options.vert_lines.visible = !panel.grid_options.vert_lines.visible;
                    Some(panel.grid_options.vert_lines.visible)
                } else { None };
                if let Some(val) = new_val {
                    let all_pids: Vec<_> = self.panel_app.panel_grid.containers().values().map(|w| w.chart_panel_id).collect();
                    for pid in all_pids {
                        crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid)
                            .grid_options.vert_lines.visible = val;
                    }
                }
                eprintln!("[ChartApp] grid vertical toggled (all windows)");
                state_mutated = true;
            }
            ChartOutEvent::ToggleGridHorizontal => {
                let active_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                let new_val = if let Some(pid) = active_pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    panel.grid_options.horz_lines.visible = !panel.grid_options.horz_lines.visible;
                    Some(panel.grid_options.horz_lines.visible)
                } else { None };
                if let Some(val) = new_val {
                    let all_pids: Vec<_> = self.panel_app.panel_grid.containers().values().map(|w| w.chart_panel_id).collect();
                    for pid in all_pids {
                        crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid)
                            .grid_options.horz_lines.visible = val;
                    }
                }
                eprintln!("[ChartApp] grid horizontal toggled (all windows)");
                state_mutated = true;
            }
            ChartOutEvent::ToggleLegendOHLC => {
                let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                if let Some(pid) = pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    panel.legend.show_ohlc = !panel.legend.show_ohlc;
                    eprintln!("[ChartApp] legend OHLC toggled -> {}", panel.legend.show_ohlc);
                }
                state_mutated = true;
            }
            ChartOutEvent::ToggleLegendChange => {
                let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                if let Some(pid) = pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    panel.legend.show_change = !panel.legend.show_change;
                    eprintln!("[ChartApp] legend change toggled -> {}", panel.legend.show_change);
                }
                state_mutated = true;
            }
            ChartOutEvent::ToggleLegendPercent => {
                let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                if let Some(pid) = pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    panel.legend.show_percent = !panel.legend.show_percent;
                    eprintln!("[ChartApp] legend percent toggled -> {}", panel.legend.show_percent);
                }
                state_mutated = true;
            }
            ChartOutEvent::ToggleTooltip => {
                let active_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                let new_val = if let Some(pid) = active_pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    panel.tooltip.visible = !panel.tooltip.visible;
                    Some(panel.tooltip.visible)
                } else { None };
                if let Some(val) = new_val {
                    let all_pids: Vec<_> = self.panel_app.panel_grid.containers().values().map(|w| w.chart_panel_id).collect();
                    for pid in all_pids {
                        crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid).tooltip.visible = val;
                    }
                }
                eprintln!("[ChartApp] tooltip toggled (all windows)");
                state_mutated = true;
            }
            ChartOutEvent::ToggleTooltipFollow => {
                let active_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                let new_val = if let Some(pid) = active_pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    panel.tooltip.follow_cursor = !panel.tooltip.follow_cursor;
                    Some(panel.tooltip.follow_cursor)
                } else { None };
                if let Some(val) = new_val {
                    let all_pids: Vec<_> = self.panel_app.panel_grid.containers().values().map(|w| w.chart_panel_id).collect();
                    for pid in all_pids {
                        crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid).tooltip.follow_cursor = val;
                    }
                }
                eprintln!("[ChartApp] tooltip follow toggled (all windows)");
                state_mutated = true;
            }
            ChartOutEvent::SetWatermarkText(text) => {
                let all_pids: Vec<_> = self.panel_app.panel_grid.containers().values().map(|w| w.chart_panel_id).collect();
                for pid in all_pids {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    match &mut panel.watermark {
                        Some(wm) => wm.set_text(text.to_string()),
                        None => { let mut wm = mlc_core::Watermark::default(); wm.set_text(text.to_string()); panel.watermark = Some(wm); }
                    }
                }
                eprintln!("[ChartApp] watermark text set to {} (all windows)", text);
                state_mutated = true;
            }
            ChartOutEvent::SetWatermarkPosition(pos) => {
                use mlc_core::{HorzAlign, VertAlign};
                let (horz, vert) = match pos.as_ref() {
                    "center"       => (HorzAlign::Center, VertAlign::Center),
                    "bottom_left"  => (HorzAlign::Left,   VertAlign::Bottom),
                    "bottom_right" => (HorzAlign::Right,  VertAlign::Bottom),
                    "top_left"     => (HorzAlign::Left,   VertAlign::Top),
                    "top_right"    => (HorzAlign::Right,  VertAlign::Top),
                    _              => (HorzAlign::Left,   VertAlign::Bottom),
                };
                let all_pids: Vec<_> = self.panel_app.panel_grid.containers().values().map(|w| w.chart_panel_id).collect();
                for pid in all_pids {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    match &mut panel.watermark {
                        Some(wm) => { wm.horz_align = horz; wm.vert_align = vert; }
                        None => { panel.watermark = Some(mlc_core::Watermark { horz_align: horz, vert_align: vert, ..mlc_core::Watermark::default() }); }
                    }
                }
                eprintln!("[ChartApp] watermark position set to {} (all windows)", pos);
                state_mutated = true;
            }
            // === Presets (in-memory HashMap) ===
            ChartOutEvent::SavePreset { name } => {
                // "Save As"  -  create a new named preset from current state.
                use mlc_core::preset::preset::unix_timestamp_parts;
                let (secs, nanos) = unix_timestamp_parts();
                let id = format!("preset_{}_{}", secs, nanos);
                eprintln!("[ChartApp] save-as preset '{}'  ->  id={}", name, id);
                self.collect_state_into_preset(&id, &name);
                self.panel_app.active_preset_id = id.clone();
                // Add the new preset to open_tabs.
                if !self.panel_app.open_tabs.contains(&id) {
                    self.panel_app.open_tabs.push(id.clone());
                }
                if let Some(preset) = self.core.presets().get(&id).cloned() {
                    self.preset_actions.push(crate::PresetAction::Upsert(preset));
                }
                self.persist_profile();
                if let Some(p) = self.core.presets().get(&id) {
                    eprintln!(
                        "[ChartApp] preset '{}' saved ({} windows, {} groups, {} indicators)",
                        id, p.windows.len(), p.bubbles.len(), p.indicators.len()
                    );
                }
            }

            ChartOutEvent::LoadPreset { id } => {
                let _lp_total = web_time::Instant::now();
                eprintln!("[ChartApp] load preset: {}", id);
                let prev_id = self.panel_app.active_preset_id.clone();

                // Is the CURRENT live state actually a materialized preset?
                // At startup active_preset_id is already set from the profile
                // BEFORE the restore runs, so a LoadPreset for that same id
                // must NOT be treated as a no-op — the windows have no bubble
                // membership yet. Treating it as a no-op both skipped the real
                // restore (eternal "Loading...", empty toolbar) and let the
                // trailing autosave bake the EMPTY state into the preset file.
                let _lp_t_materialized = web_time::Instant::now();
                let already_materialized = self
                    .panel_app
                    .panel_grid
                    .iter_containers()
                    .any(|(_, w)| self.bubble_for_chart(w.id).is_some());
                let lp_materialized_us = _lp_t_materialized.elapsed().as_micros();

                // Same tab AND live state real  -  no-op (and nothing changed,
                // so no autosave either).
                if prev_id == id && already_materialized {
                    eprintln!("[ChartApp] LoadPreset: already active, skipping (materialized_check={}us)", lp_materialized_us);
                } else {

                // Save the outgoing preset (with bars) before switching away —
                // but never snapshot a non-materialized placeholder state over
                // a real preset file.
                //
                // CRITICAL: must run SYNCHRONOUSLY before `active_preset_id`
                // is changed. The debounced `autosave_snapshot()` path can't
                // catch this — by the time the end-of-tick flush runs,
                // `active_preset_id` is already the NEW id, and the captured
                // target_id != active_id check in flush_pending_autosave will
                // intentionally skip. We need the outgoing preset persisted
                // RIGHT NOW with the still-materialised outgoing panel_grid.
                let _lp_t_autosave = web_time::Instant::now();
                if !prev_id.is_empty() && prev_id != "__default__" && already_materialized {
                    let prev_name = self.core.presets().get(&prev_id)
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| prev_id.clone());
                    // Drop any earlier pending request — we're handling
                    // outgoing-state persistence here synchronously.
                    self.pending_autosave = None;
                    self.collect_state_into_preset(&prev_id, &prev_name);
                    if let Some(preset) = self.core.presets().get(&prev_id).cloned() {
                        self.preset_actions.push(crate::PresetAction::Upsert(preset));
                    }
                    eprintln!(
                        "[ChartApp] LoadPreset: persisted outgoing preset '{}' synchronously before switch (active='{}')",
                        prev_id, self.panel_app.active_preset_id,
                    );
                }
                let lp_autosave_us = _lp_t_autosave.elapsed().as_micros();

                // Park outgoing state into live cache — only a materialized
                // state. Parking the startup placeholder under the SAME id we
                // are about to load would make unpark return the placeholder
                // and skip the cold restore entirely.
                let _lp_t_park = web_time::Instant::now();
                if !prev_id.is_empty() && already_materialized {
                    self.park_active_preset(&prev_id);
                }
                let lp_park_us = _lp_t_park.elapsed().as_micros();

                self.panel_app.active_preset_id = id.clone();
                self.persist_profile();

                // Warm path: live cache hit  -  zero-flicker switch.
                let _lp_t_unpark = web_time::Instant::now();
                let unpark_hit = self.unpark_preset(&id);
                let lp_unpark_us = _lp_t_unpark.elapsed().as_micros();
                if unpark_hit {
                    eprintln!("[ChartApp] LoadPreset '{}': live cache hit  -  zero-flicker switch (materialized={}us autosave={}us park={}us unpark={}us)",
                        id, lp_materialized_us, lp_autosave_us, lp_park_us, lp_unpark_us);
                    let _lp_t_cure = web_time::Instant::now();

                    // Per-window cure: only force=true when the unparked window
                    // came back EMPTY (bars never reached it before the park, or
                    // the live forwarder died). Otherwise call force=false — the
                    // bridge's dedup-L2 sees an already-live Stream::Kline
                    // subscriber and skips the REST + bg-backfill round-trip,
                    // and the WS BarUpdate stream that has been piping since the
                    // park keeps the dataset fresh. This collapses the
                    // 6×Phase-A + 6×2000-bar-backfill storm that froze the UI
                    // on every preset tab swap to zero network work in the
                    // common "I'm flipping between presets I just looked at"
                    // case. The handful of empty-window cases still resync
                    // properly via the force=true fallback.
                    let bar_count = self.panel_app.user_manager.profile.bar_count as usize;
                    // Phase 4 S2.7.1c: read via branch_meta (B6 tuple-collect flip).
                    let window_bar_data: Vec<(String, String, mlc_core::state::Timeframe, String, bool)> = self
                        .panel_app
                        .panel_grid
                        .iter_containers()
                        .map(|(_, w)| {
                            let cid = w.id;
                            let sym = self.symbol_for_chart(cid)
                                .map(String::from).unwrap_or_default();
                            let exch = self.exchange_for_chart(cid)
                                .map(String::from).unwrap_or_default();
                            let tf = self.timeframe_for_chart(cid).unwrap_or_else(mlc_core::state::Timeframe::h1);
                            let at = self.account_type_for_chart(cid)
                                .map(String::from).unwrap_or_default();
                            // Was `let has_bars = !true;` — cutover sweep stub that
                            // pinned `force=true` on the cure path always (every
                            // preset switch re-issued Phase A even on warm cache).
                            // Real check: any bars on the chart's bubble?
                            let has_bars = !self.bars_for_chart(cid).is_empty();
                            (sym, exch, tf, at, has_bars)
                        })
                        .collect();
                    // Deferred cure: stash the per-window work and bail out.
                    // The host drains `pending_cure_after_preset_load` AFTER
                    // the first post-swap frame is presented, so the user
                    // sees loading skeletons + spinners DURING the 100-220ms
                    // request_bars sweep instead of staring at the old chart.
                    let mut bars_requested_force: usize = 0;
                    let mut bars_requested_lazy: usize = 0;
                    for (sym, exch, tf, at_label, has_bars) in &window_bar_data {
                        let eid = digdigdig3::ExchangeId::from_str(&exch)
                            .unwrap_or(digdigdig3::ExchangeId::Binance);
                        if !self.sidebar_state.connector_enabled.get(eid.as_str()).copied().unwrap_or(true) {
                            continue;
                        }
                        let at = crate::account_type_from_label(at_label);
                        let force = !*has_bars;
                        self.pending_cure_after_preset_load.push((eid, sym.clone(), tf.clone(), at, force));
                        if force { bars_requested_force += 1; } else { bars_requested_lazy += 1; }
                    }
                    let _ = bar_count; // bar_count moves to drain_pending_cure
                    let lp_cure_us = _lp_t_cure.elapsed().as_micros();
                    eprintln!(
                        "[ChartApp] LoadPreset (cache): cure requested force={} lazy={} (cure={}us total={}us)",
                        bars_requested_force, bars_requested_lazy,
                        lp_cure_us, _lp_total.elapsed().as_micros(),
                    );

                    self.sidebar_data_dirty = true;
                    state_mutated = true;
                } else {
                    let _lp_t_clone = web_time::Instant::now();
                    let preset_opt = self.core.presets().get(&id).cloned();
                    let lp_clone_us = _lp_t_clone.elapsed().as_micros();
                    if let Some(preset) = preset_opt {
                        // Defer the 1.3-1.8s `apply_preset_cold` body by ONE
                        // frame so the renderer has a chance to paint a
                        // skeleton + spinner on the parked placeholder grid
                        // before the main thread blocks on the cold body.
                        // The host drains `pending_cold_apply` from
                        // `about_to_wait` AFTER GpuDone for the skeleton
                        // frame.
                        //
                        // While the stash is in flight, `load_state ==
                        // Loading { target_id }`. flush_pending_autosave
                        // and collect_state_into_preset both early-return
                        // under that gate, so a placeholder grid cannot
                        // leak to disk under the new `active_preset_id`.
                        self.load_state = crate::persistence::LoadState::Loading {
                            target_id: id.clone(),
                            started_at: web_time::Instant::now(),
                        };
                        self.pending_cold_apply = Some(crate::PendingColdApply {
                            preset,
                            stashed_at: web_time::Instant::now(),
                            timings: crate::ColdApplyTimings {
                                lp_total: _lp_total,
                                lp_materialized_us,
                                lp_autosave_us,
                                lp_park_us,
                                lp_unpark_us,
                                lp_clone_us,
                            },
                        });
                        eprintln!(
                            "[ChartApp] LoadPreset '{}' COLD path: stashed for deferred apply (skeleton-frame-first, lp_handler={}us)",
                            id, _lp_total.elapsed().as_micros(),
                        );
                        // Do NOT set state_mutated — autosave is gated by
                        // LoadState anyway, and "Loading" is not a state
                        // worth persisting.
                    } else {
                        eprintln!("[ChartApp] preset '{}' not found in memory (clone_attempt={}us total={}us)",
                            id, lp_clone_us, _lp_total.elapsed().as_micros());
                    }
                }

                } // end else (prev_id != id)
            }

            ChartOutEvent::DeletePreset { id } => {
                eprintln!("[ChartApp] delete preset: {}", id);
                // Also remove from open_tabs when deleting.
                self.panel_app.open_tabs.retain(|t| t != &id);
                if self.core.presets_mut().remove(&id).is_some() {
                    self.preset_actions.push(crate::PresetAction::Delete { id: id.clone() });
                    self.persist_profile();
                    eprintln!("[ChartApp] preset '{}' removed from memory", id);
                } else {
                    eprintln!("[ChartApp] preset '{}' not found", id);
                }
            }

            ChartOutEvent::CloseTab { id } => {
                eprintln!("[ChartApp] close tab: {}", id);
                // Autosave the closing tab's state before removing.
                if id == self.panel_app.active_preset_id {
                    self.autosave_snapshot();
                }
                self.panel_app.open_tabs.retain(|t| t != &id);
                // Drop live cache entry for the closed tab.
                self.live_preset_cache.remove(&id);
                // If the closed tab was active, switch to another.
                if self.panel_app.active_preset_id == id {
                    if let Some(next) = self.panel_app.open_tabs.last().cloned() {
                        self.process_chart_out_event(ChartOutEvent::LoadPreset { id: next });
                    } else {
                        // No tabs left  -  fall back to __default__.
                        self.panel_app.active_preset_id = "__default__".to_string();
                    }
                }
                self.persist_profile();
            }

            ChartOutEvent::OpenTab { id } => {
                eprintln!("[ChartApp] open tab: {}", id);
                if self.panel_app.open_tabs.contains(&id) {
                    // Preset already open  -  just switch to it.
                    self.panel_app.active_preset_id = id.clone();
                    self.process_chart_out_event(ChartOutEvent::LoadPreset { id });
                } else {
                    self.panel_app.open_tabs.push(id.clone());
                    self.process_chart_out_event(ChartOutEvent::LoadPreset { id });
                }
                self.persist_profile();
            }

            ChartOutEvent::RenamePreset { id, new_name } => {
                // Resolve the target preset: empty id means "active preset".
                let target_id = if id.is_empty() {
                    self.panel_app.active_preset_id.clone()
                } else {
                    id
                };

                if target_id == "__default__" || target_id.is_empty() {
                    eprintln!("[ChartApp] no active named preset to rename");
                } else {
                    // Generate a name if none was supplied by the caller.
                    let final_name = if new_name.is_empty() {
                        if let Some(preset) = self.core.presets().get(&target_id) {
                            format!("{} (renamed)", preset.name)
                        } else {
                            eprintln!("[ChartApp] preset '{}' not found for rename", target_id);
                            return;
                        }
                    } else {
                        new_name
                    };

                    if let Some(preset) = self.core.presets_mut().get_mut(&target_id) {
                        eprintln!(
                            "[ChartApp] renamed preset '{}' -> '{}'",
                            preset.name, final_name
                        );
                        preset.name = final_name.clone();
                        self.preset_actions.push(crate::PresetAction::Rename {
                            id: target_id.clone(),
                            new_name: final_name,
                        });
                    } else {
                        eprintln!("[ChartApp] preset '{}' not found for rename", target_id);
                    }
                }
            }

            ChartOutEvent::OpenPresetSaveAs => {
                // Open the preset name input modal in "Save As" mode.
                // Find the next free "Untitled N" number.
                let max_n = self.core.presets().values()
                    .filter_map(|p| p.name.strip_prefix("Untitled "))
                    .filter_map(|s| s.parse::<u32>().ok())
                    .max()
                    .unwrap_or(0);
                let default_name = format!("Untitled {}", max_n + 1);
                self.panel_app.preset_name_input.open_save_as(&default_name, 0);
                self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::PresetNameInput;
                {
                    let pni_id = uzor::WidgetId::from(crate::text_input::PRESET_NAME_INPUT);
                    self.input_coordinator.borrow_mut().text_fields_mut().set_text(&pni_id, &default_name);
                    self.input_coordinator.borrow_mut().text_fields_mut().begin_edit(&pni_id);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::PRESET_NAME_INPUT);
                }
                eprintln!("[ChartApp] opened preset name input (save-as), default='{}'", default_name);
            }

            ChartOutEvent::OpenPresetRename => {
                // Open the preset name input modal in "Rename" mode for the active preset.
                let target_id = self.panel_app.active_preset_id.clone();
                if target_id == "__default__" || target_id.is_empty() {
                    eprintln!("[ChartApp] no active named preset to rename");
                } else if let Some(preset) = self.core.presets().get(&target_id) {
                    let current_name = preset.name.clone();
                    self.panel_app.preset_name_input.open_rename(&target_id, &current_name, 0);
                    self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::PresetNameInput;
                    {
                        let pni_id = uzor::WidgetId::from(crate::text_input::PRESET_NAME_INPUT);
                        self.input_coordinator.borrow_mut().text_fields_mut().set_text(&pni_id, &current_name);
                        self.input_coordinator.borrow_mut().text_fields_mut().begin_edit(&pni_id);
                        self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::PRESET_NAME_INPUT);
                    }
                    eprintln!("[ChartApp] opened preset name input (rename), preset='{}'", target_id);
                } else {
                    eprintln!("[ChartApp] preset '{}' not found for rename modal", target_id);
                }
            }

            ChartOutEvent::SaveCurrentPreset => {
                // Re-save snapshot into the currently active preset
                let id = self.panel_app.active_preset_id.clone();
                if id != "__default__" && !id.is_empty() {
                    let name_opt = self.core.presets().get(&id).map(|p| p.name.clone());
                    if let Some(name) = name_opt {
                        self.collect_state_into_preset(&id, &name);
                        if let Some(preset) = self.core.presets().get(&id).cloned() {
                            self.preset_actions.push(crate::PresetAction::Upsert(preset));
                        }
                        eprintln!("[ChartApp] saved snapshot into preset '{}' (id={})", name, id);
                    }
                } else {
                    // No active named preset  -  treat as Save As
                    self.process_chart_out_event(ChartOutEvent::OpenPresetSaveAs);
                }
            }

            ChartOutEvent::ToggleAutosave => {
                self.panel_app.autosave_enabled = !self.panel_app.autosave_enabled;
                eprintln!("[ChartApp] autosave = {}", self.panel_app.autosave_enabled);
            }

            ChartOutEvent::OpenChartBrowser => {
                self.panel_app.chart_browser.open(0);
                self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::ChartBrowser;
                {
                    let cb_id = uzor::WidgetId::from(crate::text_input::CHART_BROWSER_SEARCH);
                    self.input_coordinator.borrow_mut().text_fields_mut().clear(&cb_id);
                    self.input_coordinator.borrow_mut().text_fields_mut().begin_edit(&cb_id);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::CHART_BROWSER_SEARCH);
                }
                eprintln!("[ChartApp] chart browser opened");
            }

            ChartOutEvent::OpenChartBrowserInNewTab => {
                self.panel_app.chart_browser.open(0);
                self.panel_app.chart_browser.open_in_new_tab = true;
                self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::ChartBrowser;
                {
                    let cb_id = uzor::WidgetId::from(crate::text_input::CHART_BROWSER_SEARCH);
                    self.input_coordinator.borrow_mut().text_fields_mut().clear(&cb_id);
                    self.input_coordinator.borrow_mut().text_fields_mut().begin_edit(&cb_id);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::CHART_BROWSER_SEARCH);
                }
                eprintln!("[ChartApp] chart browser opened (new-tab mode)");
            }

            ChartOutEvent::NewChart => {
                // Delegate to OpenPresetNewChart so the user is asked for a name first.
                self.process_chart_out_event(ChartOutEvent::OpenPresetNewChart);
            }

            ChartOutEvent::OpenPresetNewChart => {
                // If current chart is __default__, auto-save it first so the name
                // counter sees it and avoids collisions.
                if self.panel_app.active_preset_id == "__default__" {
                    use mlc_core::preset::preset::unix_timestamp_parts;
                    let max_n = self.core.presets().values()
                        .filter_map(|p| p.name.strip_prefix("Untitled "))
                        .filter_map(|s| s.parse::<u32>().ok())
                        .max()
                        .unwrap_or(0);
                    let autosave_name = format!("Untitled {}", max_n + 1);
                    let (secs, nanos) = unix_timestamp_parts();
                    let autosave_id = format!("preset_{}_{}", secs, nanos);
                    self.collect_state_into_preset(&autosave_id, &autosave_name);
                    self.panel_app.active_preset_id = autosave_id.clone();
                    eprintln!("[ChartApp] auto-saved __default__ as '{}' (id={})", autosave_name, autosave_id);
                }

                // Now compute the next "Untitled N"  -  the auto-saved one is already counted.
                let max_n = self.core.presets().values()
                    .filter_map(|p| p.name.strip_prefix("Untitled "))
                    .filter_map(|s| s.parse::<u32>().ok())
                    .max()
                    .unwrap_or(0);
                let default_name = format!("Untitled {}", max_n + 1);
                self.panel_app.preset_name_input.open_new_chart(&default_name, 0);
                self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::PresetNameInput;
                {
                    let pni_id = uzor::WidgetId::from(crate::text_input::PRESET_NAME_INPUT);
                    self.input_coordinator.borrow_mut().text_fields_mut().set_text(&pni_id, &default_name);
                    self.input_coordinator.borrow_mut().text_fields_mut().begin_edit(&pni_id);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::PRESET_NAME_INPUT);
                }
                eprintln!("[ChartApp] opened preset name input (new-chart), default='{}'", default_name);
            }

            ChartOutEvent::SetTheme(name) => {
                self.panel_app.theme_manager.set_preset(name);
                // Signal the App-level coordinator to propagate this change to all windows.
                self.theme_changed = Some(name.to_string());
                eprintln!("[ChartApp] SetTheme: preset={}", name);
            }

            ChartOutEvent::SetStyle(name) => {
                if let Some(style) = UIStyle::from_name(name) {
                    self.panel_app.theme_manager.current_mut().set_style(style);
                    eprintln!("[ChartApp] SetStyle: style={:?} (from name={})", style, name);
                } else {
                    eprintln!("[ChartApp] SetStyle: unknown style name '{}'", name);
                }
            }

            ChartOutEvent::InternalToggleSyncSymbol => {
                let gid = self.panel_app.panel_grid.active_container_id().and_then(|cid| self.bubble_for_chart(cid));
                let mut newly_enabled = false;
                if let Some(gid) = gid {
                    if let Some(group) = self.core.bubble_mut().bubble.bubble_mut(gid) {
                        group.sync_flags.sync_symbol = !group.sync_flags.sync_symbol;
                        newly_enabled = group.sync_flags.sync_symbol;
                        eprintln!("[ChartApp] InternalToggleSyncSymbol: group={:?} sync_symbol={}", gid, group.sync_flags.sync_symbol);
                    }
                } else {
                    eprintln!("[ChartApp] InternalToggleSyncSymbol: no active group");
                }
                // When sync is turned ON, immediately propagate active leaf's instrument key to peers.
                // Phase 4 S2.7.1c: read via branch_meta (B1 flip).
                if newly_enabled {
                    if let Some(leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                        let active_cid = self.panel_app.panel_grid.active_container_id();
                        let symbol = active_cid
                            .and_then(|cid| self.symbol_for_chart(cid))
                            .map(String::from)
                            .unwrap_or_default();
                        let exchange = active_cid
                            .and_then(|cid| self.exchange_for_chart(cid))
                            .map(String::from)
                            .unwrap_or_default();
                        let account_type = active_cid
                            .and_then(|cid| self.account_type_for_chart(cid))
                            .map(String::from)
                            .unwrap_or_default();
                        if !symbol.is_empty() {
                            self.propagate_symbol_to_sync_group(leaf, &symbol, &exchange, &account_type);
                        }
                    }
                }
                state_mutated = true;
            }

            ChartOutEvent::InternalToggleSyncTimeframe => {
                let gid = self.panel_app.panel_grid.active_container_id().and_then(|cid| self.bubble_for_chart(cid));
                let mut newly_enabled = false;
                if let Some(gid) = gid {
                    if let Some(group) = self.core.bubble_mut().bubble.bubble_mut(gid) {
                        group.sync_flags.sync_timeframe = !group.sync_flags.sync_timeframe;
                        newly_enabled = group.sync_flags.sync_timeframe;
                        eprintln!("[ChartApp] InternalToggleSyncTimeframe: group={:?} sync_timeframe={}", gid, group.sync_flags.sync_timeframe);
                    }
                } else {
                    eprintln!("[ChartApp] InternalToggleSyncTimeframe: no active group");
                }
                // When sync is turned ON, immediately propagate active leaf's TF to peers.
                // Phase 4 S2.7.1c: read via branch_meta (B1 flip).
                if newly_enabled {
                    if let Some(leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                        let tf = self.panel_app.panel_grid.active_container_id()
                            .and_then(|cid| self.timeframe_for_chart(cid));
                        if let Some(tf) = tf {
                            self.propagate_timeframe_to_sync_group(leaf, tf);
                        }
                    }
                }
                state_mutated = true;
            }

            ChartOutEvent::InternalToggleSyncCrosshair => {
                let gid = self.panel_app.panel_grid.active_container_id().and_then(|cid| self.bubble_for_chart(cid));
                let mut newly_enabled = false;
                if let Some(gid) = gid {
                    if let Some(group) = self.core.bubble_mut().bubble.bubble_mut(gid) {
                        group.sync_flags.sync_crosshair = !group.sync_flags.sync_crosshair;
                        newly_enabled = group.sync_flags.sync_crosshair;
                        eprintln!("[ChartApp] InternalToggleSyncCrosshair: group={:?} sync_crosshair={}", gid, group.sync_flags.sync_crosshair);
                    }
                } else {
                    eprintln!("[ChartApp] InternalToggleSyncCrosshair: no active group");
                }
                // When sync is turned ON, immediately propagate active window's crosshair to peers
                // so that peer price ranges are re-synced and the horizontal line becomes visible.
                if newly_enabled {
                    if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                        let active_bars_cs: Vec<mlc_core::Bar> = self.active_chart_bars();
                        let (timestamp, price, crosshair_visible, pane_index) = self.panel_app
                            .panel_grid
                            .active_container()
                            .map(|w| {
                                let bar_f64 = w.crosshair.bar_f64;
                                let timestamp = mlc_core::bar_f64_to_timestamp_ms(&active_bars_cs[..], bar_f64);
                                (timestamp, w.crosshair.price, w.crosshair.visible, w.crosshair.pane_index)
                            })
                            .unwrap_or((0, 0.0, false, None));
                        self.propagate_crosshair_to_sync_group(active_leaf, timestamp, price, crosshair_visible, pane_index);
                    }
                }
                state_mutated = true;
            }

            ChartOutEvent::InternalToggleSyncViewport => {
                let gid = self.panel_app.panel_grid.active_container_id().and_then(|cid| self.bubble_for_chart(cid));
                let mut newly_enabled = false;
                let mut newly_disabled = false;
                if let Some(gid) = gid {
                    if let Some(group) = self.core.bubble_mut().bubble.bubble_mut(gid) {
                        let was_on = group.sync_flags.sync_viewport;
                        group.sync_flags.sync_viewport = !was_on;
                        newly_enabled = !was_on;
                        newly_disabled = was_on;
                        eprintln!("[ChartApp] InternalToggleSyncViewport: group={:?} sync_viewport={}", gid, group.sync_flags.sync_viewport);
                    }
                } else {
                    eprintln!("[ChartApp] InternalToggleSyncViewport: no active group");
                }
                // OFF -> ON: adopt the active leaf's current scale_mode as the group mode,
                // stash each peer's prior mode, then push mode + viewport to all peers.
                if newly_enabled {
                    let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
                    if let Some(active_leaf) = active_leaf {
                        // Capture the active window's current scale_mode as the new group mode.
                        let active_mode = self.panel_app.panel_grid
                            .active_container()
                            .map(|w| w.price_scale.scale_mode);
                        if let Some(mode) = active_mode {
                            if let Some(gid) = gid {
                                if let Some(group) = self.core.bubble_mut().bubble.bubble_mut(gid) {
                                    group.scale_mode = mode;
                                }
                            }
                        }
                        // Stash each peer's current mode and apply the group mode.
                        // propagate_viewport_to_sync_group applies mode to peers via the
                        // scale_mode argument; stash happens here before the call.
                        if let Some(gid) = gid {
                            let peer_leaves: Vec<mlc_core::LeafId> =
                                self.core.bubble().bubble.chart_member_ids(gid)
                                    .into_iter()
                                    .filter_map(|cid| self.panel_app.panel_grid.leaf_for_container_id(cid))
                                    .filter(|&lid| lid != active_leaf)
                                    .collect();
                            for peer_leaf in peer_leaves {
                                if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(peer_leaf) {
                                    // Stash existing mode if not already stashed (preserve
                                    // the window's pre-join mode across multiple sync toggles).
                                    if window.stashed_scale_mode.is_none() {
                                        window.stashed_scale_mode = Some(window.price_scale.scale_mode);
                                    }
                                }
                            }
                        }
                        let viewport_state = self.panel_app
                            .panel_grid
                            .active_container()
                            .map(|w| (w.viewport.view_start, w.viewport.bar_spacing));
                        if let Some((view_start, bar_spacing)) = viewport_state {
                            self.propagate_viewport_to_sync_group(
                                active_leaf, view_start, bar_spacing,
                                active_mode,
                            );
                        }
                    }
                }
                // ON -> OFF: clear stashed_scale_mode on all peers (they keep whatever
                // mode they currently display, which was the group mode).
                if newly_disabled {
                    if let Some(gid) = gid {
                        let all_leaves: Vec<mlc_core::LeafId> =
                            self.core.bubble().bubble.chart_member_ids(gid)
                                .into_iter()
                                .filter_map(|cid| self.panel_app.panel_grid.leaf_for_container_id(cid))
                                .collect();
                        for leaf in all_leaves {
                            if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf) {
                                window.stashed_scale_mode = None;
                            }
                        }
                    }
                }
                state_mutated = true;
            }

            ChartOutEvent::InternalToggleSyncDrawings => {
                let gid = self.panel_app.panel_grid.active_container_id().and_then(|cid| self.bubble_for_chart(cid));
                let chart_id = self.panel_app.panel_grid.active_container().map(|w| w.id);
                if let Some(gid) = gid {
                    // Toggle the flag and capture the new value.
                    let new_sync = if let Some(group) = self.core.bubble_mut().bubble.bubble_mut(gid) {
                        group.sync_flags.sync_drawings = !group.sync_flags.sync_drawings;
                        eprintln!("[BubbleStore] sync_drawings toggled to {}", group.sync_flags.sync_drawings);
                        group.sync_flags.sync_drawings
                    } else {
                        return;
                    };

                    // Get auto_bubble_id and chart_panel_id before any mutable borrows.
                    let auto_bid = chart_id.and_then(|cid| {
                        self.panel_app.panel_grid.containers().get(&cid).map(|w| w.auto_bubble_id)
                    });
                    let active_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);

                    if new_sync {
                        // sync_drawings OFF -> ON: the chart was showing its own drawings (from
                        // auto_bubble); now it will show the color-group drawings via per-frame
                        // sync.  Save current drawing_manager locals into auto_bubble.primitives
                        // (they are the "home" state; auto_bubble must hold them for desync).
                        if let (Some(bid), Some(pid)) = (auto_bid, active_pid) {
                            let local: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>> =
                                self.core.drawings().get(pid)
                                    .map(|dm| dm.primitives().iter()
                                        .filter(|p| p.data().origin_id.is_none())
                                        .map(|p| p.clone_box())
                                        .collect())
                                    .unwrap_or_default();
                            let stash_count = local.len();
                            if let Some(auto_bub) = self.core.bubble_mut().bubble.bubble_mut(bid) {
                                auto_bub.primitives = local;
                            }
                            self.core.drawings_mut().entry_or_default(pid).clear_all_primitives();
                            eprintln!(
                                "[BubbleStore] sync_drawings ON: moved {} window-local primitives into auto-bubble {:?}",
                                stash_count, bid
                            );
                        }
                    } else {
                        // sync_drawings ON -> OFF: restore home drawings from auto_bubble into
                        // drawing_manager so the chart shows its own drawings again.
                        if let (Some(bid), Some(pid)) = (auto_bid, active_pid) {
                            let restored: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>> =
                                self.core.bubble().bubble.bubble(bid)
                                    .map(|bub| bub.primitives.iter().map(|p| p.clone_box()).collect())
                                    .unwrap_or_default();
                            let count = restored.len();
                            let mut guard = self.core.drawings_mut();
                            let dm = guard.entry_or_default(pid);
                            dm.clear_all_primitives();
                            dm.add_synced_primitives(restored);
                            eprintln!(
                                "[BubbleStore] sync_drawings OFF: restored {} primitives from auto-bubble {:?}",
                                count, bid
                            );
                        }
                    }
                } else {
                    eprintln!("[ChartApp] InternalToggleSyncDrawings: no active group");
                }
                let _ = chart_id; // chart_id reserved for future per-chart filtering
                state_mutated = true;
            }

            ChartOutEvent::InternalToggleSyncIndicators => {
                let gid = self.panel_app.panel_grid.active_container_id().and_then(|cid| self.bubble_for_chart(cid));
                let chart_id = self.panel_app.panel_grid.active_container().map(|w| w.id);
                if let (Some(gid), Some(chart_id)) = (gid, chart_id) {
                    // Toggle the flag and capture the new value.
                    let new_sync = if let Some(group) = self.core.bubble_mut().bubble.bubble_mut(gid) {
                        group.sync_flags.sync_indicators = !group.sync_flags.sync_indicators;
                        eprintln!("[BubbleStore] sync_indicators toggled to {}", group.sync_flags.sync_indicators);
                        group.sync_flags.sync_indicators
                    } else {
                        return;
                    };

                    let pre_tag_ids: Vec<u64> = {
                        let auto_bid = self.panel_app.panel_grid.containers()
                            .get(&chart_id)
                            .map(|w| w.auto_bubble_id);
                        auto_bid
                            .and_then(|bid| self.with_bubble(bid, |bub| bub.pre_tag_indicator_ids.clone()))
                            .unwrap_or_default()
                    };

                    if new_sync {
                        // OFF  ->  ON: hide window-local indicators (in pre_tag_indicator_ids
                        // but not yet in the group's indicator_configs).
                        let group_indicator_ids: std::collections::HashSet<u64> = self
                            .core.bubble().bubble
                            .bubble(gid)
                            .map(|g| g.indicator_configs.iter().map(|c| c.id).collect())
                            .unwrap_or_default();

                        let to_hide: Vec<u64> = self.core.indicators().instances_iter()
                            .filter(|i| {
                                i.window_id == Some(chart_id.0)
                                    && pre_tag_ids.contains(&i.id)
                                    && !group_indicator_ids.contains(&i.id)
                            })
                            .map(|i| i.id)
                            .collect();

                        for id in &to_hide {
                            if let Some(inst) = self.core.indicators_mut().get_instance_mut(*id) {
                                inst.visible = false;
                            }
                        }
                        eprintln!(
                            "[BubbleStore] sync_indicators ON: hid {} window-local indicators",
                            to_hide.len()
                        );
                    } else {
                        // ON  ->  OFF: unhide window-local (pre_tag) indicators.
                        let to_show: Vec<u64> = self.core.indicators().instances_iter()
                            .filter(|i| {
                                i.window_id == Some(chart_id.0)
                                    && pre_tag_ids.contains(&i.id)
                                    && !i.visible
                            })
                            .map(|i| i.id)
                            .collect();

                        for id in &to_show {
                            if let Some(inst) = self.core.indicators_mut().get_instance_mut(*id) {
                                inst.visible = true;
                            }
                        }
                        eprintln!(
                            "[BubbleStore] sync_indicators OFF: unhid {} window-local indicators",
                            to_show.len()
                        );
                    }
                } else {
                    eprintln!("[ChartApp] InternalToggleSyncIndicators: no active group");
                }
                state_mutated = true;
            }

            ChartOutEvent::InternalToggleSplitUntagged => {
                self.split_without_group = !self.split_without_group;
                eprintln!("[ChartApp] split_without_group toggled to {}", self.split_without_group);
            }

            ChartOutEvent::Undo => {
                self.perform_undo_with_group();
            }

            ChartOutEvent::Redo => {
                self.perform_redo_with_group();
            }

            ChartOutEvent::Screenshot => {
                self.request_screenshot();
            }

            ref other => {
                eprintln!("[ChartApp] unhandled event: {:?}", other);
            }
        }

        if state_mutated {
            self.autosave_snapshot();
        }
    }

}
