//! Process ChartOutEvent variants  -  propagates chart-internal events
//! (viewport, timeframe, symbol, indicator, drawing changes) to sync groups,
//! tag manager, autosave, and other downstream consumers.

use crate::{ChartApp, sglog};
use mlc_core::UIStyle;
use mlc_core::state::modal_state::OpenModal;

/// Wasm-diagnosable counters for the default-companion mechanic (exported
/// via MLC_DEBUG): how many times the all-windows sync ran this session,
/// and how many companion adds succeeded / silently failed.
pub static COMPANION_SYNC_RUNS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);
pub static COMPANION_ADD_OK: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);
pub static COMPANION_ADD_FAIL: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

impl ChartApp {
    /// Drain the cure work LoadPreset's warm path deferred. Called by the host
    /// AFTER the first frame following a preset swap has been presented so
    /// the user sees loading skeletons + spinners during the request_bars
    /// sweep instead of staring at the old chart while it runs synchronously.
    pub fn drain_pending_cure_after_preset_load(&mut self) {
        if self.pending_cure_after_preset_load.is_empty() { return; }
        let work = std::mem::take(&mut self.pending_cure_after_preset_load);
        let bar_count = self.core.user().profile().domain.bar_count as usize;
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
        // Refuse BEFORE anything is mutated if another window already
        // claims target_id — background-warm is a pure optimization
        // (pre-hydrate into cache for a snappier future switch), never
        // worth stealing a preset another window is actively showing.
        if !self.try_claim_preset_for_cold_load(&target_id) {
            eprintln!(
                "[ChartApp] background-warm SKIPPED '{}' — already claimed by another window",
                target_id,
            );
            return true; // counted as drained, nothing to do
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
                // Depth-gating guard (2026-07-04 TF-gating fix): re-check the
                // resolved TF against `allowed_timeframes` so the agent-API
                // bypass (`ChartApp::change_timeframe`, which routes through
                // this same handler) can't force a TF the toolbar wouldn't
                // let the user click. `None` (picker not applicable for this
                // chart_type) never blocks — those chart types don't reach
                // this dispatch path with a real TF change today, and a
                // future caller shouldn't be surprised by a silent no-op.
                let timeframe = timeframe.filter(|_| {
                    let Some(active_chart_id) = self.panel_app.panel_grid.active_container_id() else {
                        return true;
                    };
                    let chart_type = self.chart_type_for_chart(active_chart_id).unwrap_or("candles");
                    let active_series = self.active_series_for_chart(active_chart_id);
                    let series = active_series.as_active_series();
                    // Cold state: no bars yet means depth is not measurable.
                    // The user is choosing the FIRST TF for this chart —
                    // never block that path, or the initial subscription
                    // never happens and the chart stays empty forever
                    // (regression seen after wiring depth-gate 2026-07-04).
                    if series.is_empty() {
                        return true;
                    }
                    let now_ms = web_time::SystemTime::now()
                        .duration_since(web_time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as i64;
                    match crate::active_series::allowed_timeframes(series, chart_type, now_ms) {
                        None => true,
                        Some(allowed) => {
                            let ok = allowed.contains(timeframe_id.as_str());
                            if !ok {
                                log::warn!(
                                    "[ChartApp] ChangeTimeframe: {} not allowed for chart_type {} at current depth, ignoring",
                                    timeframe_id, chart_type,
                                );
                            }
                            ok
                        }
                    }
                });
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
                    let at_tf: mlc_core::instrument::AccountType = active_chart_id
                        .and_then(|cid| self.account_type_for_chart(cid))
                        .unwrap_or_default();
                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                        // Mark a fresh load so a stray TradeUpdate can't synthesize a
                        // bar before BarsLoaded arrives (which would strand view_start
                        // at 0 = oldest bar at the left edge).
                        window.begin_fresh_load();
                        // Snap to right edge once first batch lands — without this
                        // the last bar sits at view_start=0 (left edge) after TF switch.
                        window.needs_snap_after_backfill = true;
                    }
                    if let Some(chart_id) = active_chart_id {
                        if let Some(bid) = self.core.tags().tag_for_chart(chart_id) {
                            self.core.bubbles_mut().set_timeframe(bid, tf.clone());
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
                            let at = mlc_backend_data_bridge::account_type_to_dig3(at_tf);
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
                                self.core.bridge.request_bars(self.active_exchange, &symbol, &tf, at, None, Some(self.core.user().profile().domain.bar_count as usize), true);
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
                    // "histogram" removed 2026-07-05 — it drew VOLUME, not
                    // price (lightweight-charts series type mis-ported as a
                    // chart style); legacy saves fall through to "candles".
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
                    // "cvd" removed (2026-07-04 taxonomy fix) — CVD is now a
                    // real compute-catalog indicator instance (type_id
                    // "cvd"), not a chart_type routed through ChangeChartType.
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
                    // Captured BEFORE the mutation below so the D9 kline-forwarder
                    // refcount logic knows what family the window is LEAVING.
                    let previous_ct: Option<&'static str> = pid
                        .and_then(|chart_panel_id| self.panels_store.chart_panel.get(&chart_panel_id))
                        .map(|p| p.chart_type);
                    let (chart_param, chart_interval): (f64, &'static str) = if let Some(chart_panel_id) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(
                            &mut self.panels_store, chart_panel_id,
                        );
                        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                            window.set_chart_type_with_undo(panel, ct);
                            // Wave T1 commit 1: some chart types need more
                            // horizontal room per bar than whatever spacing
                            // the previous type left behind (footprint's
                            // text-legibility floor). Clamp up, right-edge
                            // anchored, so the switch shows real content
                            // instead of a permanent zoom-in placeholder.
                            // Switching AWAY never restores the old spacing.
                            if let Some(min_spacing) = min_usable_bar_spacing(ct) {
                                window.ensure_min_bar_spacing(min_spacing);
                            }
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
                        | "renko" | "pnf" | "kagi" | "tpo"
                        | "three_line_break"
                        | "dollar_bar" | "tick_imbalance_bar"
                        | "volume_imbalance_bar" | "run_bar"
                        | "dom_heatmap"
                        | "liquidation_heatmap"
                    );
                    if let Some(active_leaf) = self.panel_app.panel_grid.active_container() {
                        let cid = active_leaf.id;
                        let symbol_opt = self.symbol_for_chart(cid);
                        let at_opt: Option<mlc_core::instrument::AccountType> = self.account_type_for_chart(cid);
                        let tf_opt = self.timeframe_for_chart(cid);
                        if let (Some(symbol), Some(at), Some(tf)) = (symbol_opt, at_opt, tf_opt) {
                            if !symbol.is_empty() {
                                let eid_str = self.active_exchange.as_str();
                                let connector_enabled = self.sidebar_state.connector_enabled
                                    .get(eid_str).copied().unwrap_or(true);
                                if connector_enabled {
                                    let at = mlc_backend_data_bridge::account_type_to_dig3(at);
                                    let on_tick_tf = tf.tick_count().is_some();

                                    // Audit D9 step 2: release the outgoing kline
                                    // forwarder when leaving kline-family for a
                                    // derived type that doesn't need the kline
                                    // backdrop (footprint's cluster overlay reads
                                    // live kline data — never abort under it).
                                    // CVD used to be listed here too (its ts-domain
                                    // also reads live kline data) but it is no
                                    // longer a chart_type reachable via
                                    // ChangeChartType (2026-07-04 taxonomy fix —
                                    // demoted to a subpane overlay whose kline
                                    // dependency is now implicit: it only ever
                                    // renders alongside whatever kline-family
                                    // chart_type the window is already on). Only
                                    // if NO OTHER container in THIS ChartApp still
                                    // displays (exchange,symbol,tf,account) as
                                    // kline-family or footprint — a sibling
                                    // window/leaf sharing the same instrument must
                                    // not go dark.
                                    //
                                    // Scope note: this only enumerates containers
                                    // within the current ChartApp/process. Whether
                                    // another OS window (a second ChartApp
                                    // instance / daemon-mode session) displays the
                                    // same instrument as kline-family is NOT
                                    // reachable from here — `BackendCore::sessions`
                                    // is pure connection metadata (app_id/label/
                                    // heartbeat), it does not enumerate a peer
                                    // session's live containers. If that
                                    // cross-session case matters in practice, it
                                    // needs its own arc (a shared kline-forwarder
                                    // refcount on BackendCore, not per-ChartApp
                                    // enumeration).
                                    let was_kline_family = !matches!(
                                        previous_ct,
                                        Some("range_bar") | Some("volume_bar") | Some("tick_bar") | Some("footprint")
                                        | Some("renko") | Some("pnf") | Some("kagi") | Some("tpo")
                                        | Some("three_line_break")
                                        | Some("dollar_bar") | Some("tick_imbalance_bar")
                                        | Some("volume_imbalance_bar") | Some("run_bar")
                                        | Some("dom_heatmap") | Some("liquidation_heatmap")
                                    );
                                    // NOTE: footprint used to be exempted here via a
                                    // `target_needs_kline_backdrop` flag — stale since
                                    // 441d623d dropped the kline backdrop (footprint IS
                                    // its own OHLC series). It now releases the shared
                                    // kline forwarder like every other derived type.
                                    if was_kline_family && is_derived_target && !on_tick_tf {
                                        let still_needed = self.panel_app.panel_grid.containers().iter()
                                            .any(|(other_cid, _)| {
                                                if *other_cid == cid { return false; }
                                                let same_instrument = self.symbol_for_chart(*other_cid).as_deref() == Some(symbol.as_str())
                                                    && self.exchange_for_chart(*other_cid).as_deref() == Some(self.active_exchange.as_str())
                                                    && self.account_type_for_chart(*other_cid) == Some(
                                                        self.account_type_for_chart(cid).unwrap_or_default(),
                                                    )
                                                    && self.timeframe_for_chart(*other_cid).map(|t| t.name.clone()) == Some(tf.name.clone());
                                                if !same_instrument { return false; }
                                                let other_ct = self.chart_type_for_chart(*other_cid).unwrap_or("candles");
                                                !matches!(
                                                    other_ct,
                                                    "range_bar" | "volume_bar" | "tick_bar" | "footprint"
                                                    | "renko" | "pnf" | "kagi" | "tpo"
                                                    | "three_line_break"
                                                    | "dollar_bar" | "tick_imbalance_bar"
                                                    | "volume_imbalance_bar" | "run_bar"
                                                    | "dom_heatmap" | "liquidation_heatmap"
                                                )
                                            });
                                        if !still_needed {
                                            self.core.bridge.abort_kline_bars(self.active_exchange, &symbol, at, &tf.name);
                                        }
                                    }

                                    // Step 1: always abort any derived
                                    // subscription. The bridge.abort_* call is
                                    // idempotent when nothing's live.
                                    self.core.bridge.abort_derived_chart(
                                        self.active_exchange, &symbol, at,
                                    );

                                    // Steps 2-5: mutate the active window.
                                    // X axis is always index-packed (see
                                    // chart-types-xaxis-audit-2026-07-03.md).
                                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                                        window.begin_fresh_load();
                                        // Mark "snap to end on first data" so
                                        // the new stream lands at the right
                                        // edge of the viewport.
                                        window.needs_snap_after_backfill = true;
                                    }
                                    // Audit D9 (extended 2026-07-06 — owner
                                    // report: chart-type switch jumps the
                                    // viewport 50-500 bars into history):
                                    // clear EVERY derived slot, including the
                                    // TARGET type's own — `ChangeChartType`
                                    // always performs a full fresh
                                    // re-subscribe below (`begin_fresh_load`
                                    // + `needs_snap_after_backfill`), so a
                                    // "keep the target slot" exemption is
                                    // never correct here. Re-entering a
                                    // previously-visited derived type on the
                                    // same bubble (e.g. renko -> pnf -> renko)
                                    // used to keep the OLD renko_bars Vec
                                    // (`clear_derived_slots(Some(ct))` skipped
                                    // it) and the re-seed then pushed the
                                    // fresh replay bricks AFTER the stale
                                    // ones — `reconcile_derived_series`'s
                                    // prepend binary-search assumes ascending
                                    // timestamps, so the resulting
                                    // stale-then-fresh array broke that
                                    // invariant and stranded `view_start`
                                    // mid-history instead of at the right
                                    // edge. `UpdateChartTypeParam` (same-type
                                    // param refresh) already calls
                                    // `clear_derived_slots(None)` — this call
                                    // site now matches it.
                                    if let Some(bid) = self.bubble_for_chart(cid) {
                                        self.core.bubbles_mut().with_bubble_mut(bid, |bubble| {
                                            bubble.clear_derived_slots(None);
                                            // Single-writer ownership of bubble.bars:
                                            // footprint's own pipeline writes the bar
                                            // series; every other chart type returns
                                            // ownership to the kline family. Set here
                                            // (one choke point for every switch) so
                                            // the kernel/reactor write gates see the
                                            // right owner before the first event of
                                            // the new mode lands.
                                            bubble.bars_source = if ct == "footprint" {
                                                mlc_backend::bubble::BarsSource::FootprintOhlc
                                            } else {
                                                mlc_backend::bubble::BarsSource::Kline
                                            };
                                        });
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
                                            // Full TF flip for the footprint bucket
                                            // interval (2026-07-05 owner report:
                                            // switching to footprint on a 1H chart
                                            // left the toolbar showing "1H" while
                                            // clusters were bucketed at 1m; naively
                                            // updating just the bubble label left
                                            // stale 1H bars mixed with the incoming
                                            // 1m fetch — timelines couldn't align,
                                            // pane rendered empty).
                                            //
                                            // Order matters:
                                            //   1. abort the previous kline sub on
                                            //      the OLD TF (target_needs_kline_
                                            //      backdrop skipped this earlier),
                                            //   2. clear bubble.bars — 1H bars are
                                            //      semantically incompatible with
                                            //      the new 1m timeline,
                                            //   3. set the bubble timeframe to the
                                            //      footprint bucket interval,
                                            //   4. dispatch request_footprint (its
                                            //      internal REST + WS sub target
                                            //      the new interval automatically).
                                            // Unconditional: footprint owns its OHLC
                                            // timeline (441d623d dropped the kline
                                            // backdrop). The old `tf.name != iv`
                                            // guard left the live kline sub running
                                            // whenever the chart was ALREADY on the
                                            // bucket interval (1m chart → footprint
                                            // 1m) — that surviving sub was one of
                                            // the five writers interleaving a kline
                                            // timeline into bubble.bars.
                                            self.core.bridge.abort_kline_bars(
                                                self.active_exchange, &symbol, at, &tf.name,
                                            );
                                            let iv_ms = match iv {
                                                "1m" => 60_000, "3m" => 180_000, "5m" => 300_000,
                                                "15m" => 900_000, "30m" => 1_800_000,
                                                "1h" => 3_600_000, _ => 60_000,
                                            };
                                            let footprint_tf = mlc_core::state::Timeframe::new(iv, iv_ms);
                                            if let Some(active_bid) = self.panel_app.panel_grid.active_container()
                                                .map(|w| w.current_tag_id)
                                            {
                                                self.core.bubbles_mut().with_bubble_mut(active_bid, |bubble| {
                                                    bubble.clear_bars();
                                                    bubble.mark_footprint_pending();
                                                });
                                                self.core.bubbles_mut().set_timeframe(active_bid, footprint_tf);
                                            }
                                            self.core.bridge.request_footprint(self.active_exchange, &symbol, at, iv);
                                            // The CVD sub-pane companion for footprint
                                            // is now handled uniformly by
                                            // `sync_default_indicators_for_chart_type`
                                            // at the tail of Step 6 (adds the real
                                            // mli `CVD` indicator to the chart's
                                            // bubble, computed off the bar stream
                                            // like MACD/RSI — no bespoke sub, no
                                            // bespoke render path).
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
                                        // "cvd" removed (2026-07-04 taxonomy fix) —
                                        // CVD subscribe now happens when a CVD
                                        // indicator instance is added (indicator
                                        // picker / default-companion path), see
                                        // `ChartApp::subscribe_cvd_line_for_indicator_add`.
                                        "tpo" => {
                                            // Default 30-min letter buckets, Trade-source path (sub-tick precision, fast cold start).
                                            let freq = if chart_param > 0.0 { chart_param as u16 } else { 30 };
                                            self.core.bridge.request_tpo(
                                                self.active_exchange, &symbol, at, freq,
                                                mlc_backend_data_bridge::TpoSource::TradeBucket,
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
                                                    Some(self.core.user().profile().domain.bar_count as usize),
                                                    true,
                                                );
                                            }
                                        }
                                    }
                                    // Owner rule 2026-07-05: each chart_type
                                    // has a canonical companion indicator that
                                    // opens automatically alongside the base
                                    // series (footprint → CVD). Uses the real
                                    // mli catalog id (`CVD`), so the sub-pane
                                    // is populated by the same trubopipeline
                                    // that drives MACD/RSI — no bespoke
                                    // sub-scription, no bespoke render path.
                                    self.sync_default_indicators_for_chart_type(cid, ct);
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
                let pid = self.panel_app.panel_grid
                    .active_container()
                    .map(|w| w.chart_panel_id);
                // Route off the panel's own `chart_type` rather than the
                // toolbar-level `active_param_chart_type` field — that field
                // is only ever set by the dead inline-dropdown-input path
                // (removed, see dropdown.rs:590-613) and is permanently
                // `None`, so every param edit used to silently no-op. The
                // modal (and any other caller) already has the active panel
                // resolved when it fires this event, so `panel.chart_type`
                // is the correct, always-populated source of truth for
                // "which derived type is being parameterized".
                let active_ct: &'static str = pid
                    .and_then(|chart_panel_id| self.panels_store.chart_panel.get(&chart_panel_id))
                    .map(|p| p.chart_type)
                    .unwrap_or("candles");
                if let Some(chart_panel_id) = pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(
                        &mut self.panels_store, chart_panel_id,
                    );
                    panel.chart_type_param = value;
                }
                if let Some(active_leaf) = self.panel_app.panel_grid.active_container() {
                    let cid = active_leaf.id;
                    let symbol_opt = self.symbol_for_chart(cid);
                    let at_opt: Option<mlc_core::instrument::AccountType> = self.account_type_for_chart(cid);
                    if let (Some(symbol), Some(at)) = (symbol_opt, at_opt) {
                        if !symbol.is_empty() {
                            let eid_str = self.active_exchange.as_str();
                            if self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                                let at = mlc_backend_data_bridge::account_type_to_dig3(at);
                                let scaled = (value * 1e8) as u64;
                                // Audit D9: a param change (e.g. renko box=10 -> box=20)
                                // re-subscribes on the SAME chart_type — ChangeChartType's
                                // clear never fires. append_renko/append_pnf/... only ever
                                // push, so without this the new box size's bricks land
                                // after the stale old-box bricks in the same slot. Clear
                                // the target's own slot (keep=None) right before the
                                // re-subscribe so it starts from empty; the re-sub below
                                // repopulates it with fresh data at the new param.
                                if let Some(bid) = self.bubble_for_chart(cid) {
                                    self.core.bubbles_mut().with_bubble_mut(bid, |bubble| {
                                        bubble.clear_derived_slots(None);
                                    });
                                }
                                match active_ct {
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
                                    // Recon 2026-07-05: three previously-missing
                                    // arms — param edits on these types used to
                                    // silently no-op (fell through `_ => {}`
                                    // below), even though `ChangeChartType`'s
                                    // own switch already covers them.
                                    "three_line_break" => {
                                        let lines_back = if value > 0.0 { (value as u8).max(1) } else { 3u8 };
                                        self.core.bridge.request_three_line_break(self.active_exchange, &symbol, at, lines_back);
                                    }
                                    "tpo" => {
                                        let freq = if value > 0.0 { value as u16 } else { 30 };
                                        self.core.bridge.request_tpo(
                                            self.active_exchange, &symbol, at, freq,
                                            mlc_backend_data_bridge::TpoSource::TradeBucket,
                                        );
                                    }
                                    // tick_bar's param is a bar count, not a
                                    // price/volume threshold — it does not go
                                    // through the ×1e8 fixed-point scaling or
                                    // `abort_derived_chart`'s "derived" pool;
                                    // it reuses the same tick-count subscribe
                                    // path the Timeframe-driven tick TF switch
                                    // uses (`request_tick_bars` /
                                    // `abort_tick_bars`, both keyed into the
                                    // same `derived_subs` slot as every other
                                    // derived stream, so the abort above is
                                    // still correct — only the request differs).
                                    "tick_bar" => {
                                        let count = if value > 0.0 { value as u32 } else { 100 };
                                        self.core.bridge.request_tick_bars(self.active_exchange, &symbol, at, count);
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
                    let at_opt: Option<mlc_core::instrument::AccountType> = self.account_type_for_chart(cid);
                    if let (Some(symbol), Some(at)) = (symbol_opt, at_opt) {
                        if !symbol.is_empty() {
                            let eid_str = self.active_exchange.as_str();
                            if self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                                let at = mlc_backend_data_bridge::account_type_to_dig3(at);
                                // Bucket-interval change re-subscribes on the SAME
                                // chart_type — ChangeChartType's clear never fires
                                // (same class of bug as the D9 param-change fix).
                                // Old-interval clusters and bars are semantically
                                // incompatible with the new bucket timeline: clear
                                // both slots and re-stamp the bubble TF before the
                                // re-subscribe repopulates from scratch.
                                let iv_ms = match static_iv {
                                    "1m" => 60_000, "5m" => 300_000, "15m" => 900_000,
                                    "30m" => 1_800_000, "1h" => 3_600_000, "4h" => 14_400_000,
                                    _ => 60_000,
                                };
                                if let Some(bid) = self.bubble_for_chart(cid) {
                                    self.core.bubbles_mut().with_bubble_mut(bid, |bubble| {
                                        bubble.clear_footprint();
                                        bubble.clear_bars();
                                        bubble.mark_footprint_pending();
                                        bubble.bars_source = mlc_backend::bubble::BarsSource::FootprintOhlc;
                                    });
                                    self.core.bubbles_mut().set_timeframe(
                                        bid, mlc_core::state::Timeframe::new(static_iv, iv_ms),
                                    );
                                }
                                if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                                    window.begin_fresh_load();
                                    window.needs_snap_after_backfill = true;
                                }
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
                    crate::ChartApp::build_demo_symbol_results("", self.core.watchlists().find(self.watchlist_ui.active_list_id), self.core.exchange_symbols().as_map());
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

                let resolved_exchange = self.core.exchange_symbols()
                    .find_by_label(exchange.as_str())
                    .unwrap_or(self.active_exchange);

                let previous_symbol: String = self.symbol_for_chart(target_cid)
                    .map(String::from)
                    .unwrap_or_default();
                let old_trade_at: mlc_core::instrument::AccountType = self.account_type_for_chart(target_cid)
                    .unwrap_or_default();
                let old_at_search = old_trade_at.to_string();
                let old_trade_at = mlc_backend_data_bridge::account_type_to_dig3(old_trade_at);
                let old_trade_exchange = self.exchange_for_chart(target_cid)
                    .and_then(|s| self.core.exchange_symbols().find_by_label(s.as_str()))
                    .unwrap_or(self.active_exchange);

                let timeframe = self.timeframe_for_chart(target_cid)
                    .unwrap_or_default();

                let old_sym_search = previous_symbol.clone();
                let old_exch_search = self.exchange_for_chart(target_cid)
                    .map(String::from).unwrap_or_default();

                let target_leaf = self.panel_app.panel_grid.iter_containers()
                    .find(|(_, w)| w.id == target_cid)
                    .map(|(lid, _)| lid);
                let target_bid = self.panel_app.panel_grid.containers().values()
                    .find(|w| w.id == target_cid)
                    .map(|w| w.current_tag_id);

                if let Some(window) = self.panel_app.panel_grid.containers_mut().values_mut()
                    .find(|w| w.id == target_cid)
                {
                    if let Some(bid) = target_bid {
                        let restore_snaps = window.take_drawings_snapshot_for_symbol(&symbol, resolved_exchange.as_str(), &account_type);
                        let new_address = mlc_core::instrument::InstrumentAddress::new(
                            resolved_exchange.as_str(),
                            symbol.clone(),
                            account_type.parse().unwrap_or_default(),
                        );
                        let result = self.core.drawings_mut().swap_symbol(
                            bid, &new_address, restore_snaps.as_deref(),
                        );
                        let old_key = format!("{}:{}:{}", old_sym_search, old_exch_search, old_at_search);
                        if result.old_primitives.is_empty() {
                            window.symbol_drawings.remove(&old_key);
                        } else {
                            window.symbol_drawings.insert(old_key, result.old_primitives);
                        }
                    }
                    window.begin_fresh_load();
                }

                if let Some(bid) = self.bubble_for_chart(target_cid) {
                    self.core.bubbles_mut().set_bid_instrument_full(
                        bid,
                        symbol.clone(),
                        resolved_exchange.as_str().to_string(),
                        account_type.clone(),
                        timeframe.clone(),
                    );
                }

                self.active_exchange = resolved_exchange;
                if !previous_symbol.is_empty() {
                    self.core.bridge.unsubscribe_trades(old_trade_exchange, &previous_symbol, old_trade_at);
                }
                let eid_str = resolved_exchange.as_str();
                if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                    eprintln!("[ChartApp] ApplySymbol: exchange {} disabled, skipping connector call", eid_str);
                } else {
                    let at_typed: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
                    let at = mlc_backend_data_bridge::account_type_to_dig3(at_typed);
                    self.core.bridge.ensure_connector(resolved_exchange);
                    self.core.bridge.request_bars(
                        resolved_exchange, &symbol, &timeframe, at, None,
                        Some(self.core.user().profile().domain.bar_count as usize), false,
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
                if let Some(bid) = target_bid {
                    self.core.indicators_mut().recalc_bubble(bid, &bars);
                }
                self.sync_sub_panes_from_manager();
                self.sidebar_data_dirty = true;
                self.autosave_snapshot();
                state_mutated = true;
            }
            ChartOutEvent::OpenCompareSearch => {
                self.modal_state.open(OpenModal::CompareSearch);
                self.modal_state.symbol_search_results =
                    crate::ChartApp::build_demo_symbol_results("", self.core.watchlists().find(self.watchlist_ui.active_list_id), self.core.exchange_symbols().as_map());
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
                    let at_opt: Option<mlc_core::instrument::AccountType> = self.account_type_for_chart(cid);
                    if let (Some(symbol), Some(at)) = (symbol_opt, at_opt) {
                        if !symbol.is_empty() {
                            let eid_str = self.active_exchange.as_str();
                            let connector_enabled = self.sidebar_state.connector_enabled
                                .get(eid_str).copied().unwrap_or(true);
                            if connector_enabled {
                                let at = mlc_backend_data_bridge::account_type_to_dig3(at);
                                let oi_show_now = {
                                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                                        w.oi_show = !w.oi_show;
                                        eprintln!("[ChartApp] oi_show toggled -> {}", w.oi_show);
                                        w.oi_show
                                    } else { false }
                                };
                                // Phase X0 / C5c: oi_delta storage lives on bubble.
                                if let Some(bid) = self.bubble_for_chart(cid) {
                                    self.core.bubbles_mut().with_bubble_mut(bid, |bubble| bubble.clear_oi());
                                }
                                if oi_show_now {
                                    self.core.bridge.request_open_interest(
                                        self.active_exchange, &symbol, at,
                                    );
                                } else {
                                    self.core.bridge.unsubscribe_open_interest(
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
                    let at_opt: Option<mlc_core::instrument::AccountType> = self.account_type_for_chart(cid);
                    if let (Some(symbol), Some(at)) = (symbol_opt, at_opt) {
                        if !symbol.is_empty() {
                            let eid_str = self.active_exchange.as_str();
                            let connector_enabled = self.sidebar_state.connector_enabled
                                .get(eid_str).copied().unwrap_or(true);
                            if connector_enabled {
                                let at = mlc_backend_data_bridge::account_type_to_dig3(at);
                                let funding_show_now = {
                                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                                        w.funding_show = !w.funding_show;
                                        eprintln!("[ChartApp] funding_show toggled -> {}", w.funding_show);
                                        w.funding_show
                                    } else { false }
                                };
                                // Phase X0 / C5c: funding_rate storage lives on bubble.
                                if let Some(bid) = self.bubble_for_chart(cid) {
                                    self.core.bubbles_mut().with_bubble_mut(bid, |bubble| bubble.clear_funding());
                                }
                                if funding_show_now {
                                    self.core.bridge.request_funding_rate(
                                        self.active_exchange, &symbol, at,
                                    );
                                } else {
                                    self.core.bridge.unsubscribe_funding_rate(
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
                    mlc_app_sidebar_content::state::RightSidebarPanel::Watchlist,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Watchlist panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleAlerts => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_app_sidebar_content::state::RightSidebarPanel::Alerts,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Alerts panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleObjectTree => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_app_sidebar_content::state::RightSidebarPanel::ObjectTree,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Object Tree panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleSignals => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_app_sidebar_content::state::RightSidebarPanel::Signals,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Signals panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleConnectors => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_app_sidebar_content::state::RightSidebarPanel::Connectors,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Connectors panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::TogglePerformance => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_app_sidebar_content::state::RightSidebarPanel::Performance,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Performance panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleAgents => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_app_sidebar_content::state::RightSidebarPanel::Agents,
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
                let panel = mlc_app_sidebar_content::state::RightSidebarPanel::from_slot_index(idx);
                let _ = self.sidebar_state.toggle_right_panel(panel);
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            // Split layout events  -  wire to ChartPanelGrid split system.
            // After each split all new leaves receive a shared sync color tag so
            // that symbol / timeframe changes propagate across the split group.
            ChartOutEvent::InternalSplitHorizontal => { sglog!("[chart-out] InternalSplitHorizontal -> do_split"); self.do_split(mlc_core::SplitKind::SplitRight); state_mutated = true; }
            ChartOutEvent::InternalSplitVertical => { sglog!("[chart-out] InternalSplitVertical -> do_split"); self.do_split(mlc_core::SplitKind::SplitBottom); state_mutated = true; }
            ChartOutEvent::InternalSplitGrid2x2 => { sglog!("[chart-out] InternalSplitGrid2x2 -> do_split"); self.do_split(mlc_core::SplitKind::Grid2x2); state_mutated = true; }
            ChartOutEvent::InternalSplit2Left1Right => { sglog!("[chart-out] InternalSplit2Left1Right -> do_split"); self.do_split(mlc_core::SplitKind::TwoLeftOneRight); state_mutated = true; }
            ChartOutEvent::InternalSplit1Left2Right => { sglog!("[chart-out] InternalSplit1Left2Right -> do_split"); self.do_split(mlc_core::SplitKind::OneLeftTwoRight); state_mutated = true; }
            ChartOutEvent::InternalSplit2Top1Bottom => { sglog!("[chart-out] InternalSplit2Top1Bottom -> do_split"); self.do_split(mlc_core::SplitKind::TwoTopOneBottom); state_mutated = true; }
            ChartOutEvent::InternalSplit1Top2Bottom => { sglog!("[chart-out] InternalSplit1Top2Bottom -> do_split"); self.do_split(mlc_core::SplitKind::OneTopTwoBottom); state_mutated = true; }
            ChartOutEvent::InternalSplit3Columns => { sglog!("[chart-out] InternalSplit3Columns -> do_split"); self.do_split(mlc_core::SplitKind::ThreeColumns); state_mutated = true; }
            ChartOutEvent::InternalSplit3Rows => { sglog!("[chart-out] InternalSplit3Rows -> do_split"); self.do_split(mlc_core::SplitKind::ThreeRows); state_mutated = true; }
            ChartOutEvent::InternalSplit1Big3Small => { sglog!("[chart-out] InternalSplit1Big3Small -> do_split"); self.do_split(mlc_core::SplitKind::OneBig3Small); state_mutated = true; }
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

            ChartOutEvent::LoadPresetInActiveSlot { id } => {
                let prev_id = self.panel_app.active_preset_id.clone();
                let already_open = self.panel_app.open_tabs.iter().any(|t| t == &id);
                if already_open {
                    // Owner UX: id already has its own tab — pure switch to
                    // that tab. Source tab keeps its preset untouched: no
                    // park (the source state stays live under prev_id), no
                    // collect (we're not saving over anything), no rotation
                    // of open_tabs. Same effect as if the user had clicked
                    // the existing chrome tab directly.
                    sglog!(
                        "[LoadPresetInActiveSlot] id='{}' already in open_tabs={:?} prev='{}' — bare switch via LoadPreset",
                        id, self.panel_app.open_tabs, prev_id
                    );
                    self.process_chart_out_event(ChartOutEvent::LoadPreset { id });
                } else {
                    // id not yet open: replace the active slot in-place
                    // (rename the current tab's id to the new preset's id).
                    let prev_idx = self.panel_app.open_tabs.iter().position(|t| t == &prev_id);
                    match prev_idx {
                        Some(idx) => { self.panel_app.open_tabs[idx] = id.clone(); }
                        None => { self.panel_app.open_tabs.push(id.clone()); }
                    }
                    sglog!(
                        "[LoadPresetInActiveSlot] id='{}' replaced active slot (prev='{}'), open_tabs={:?}",
                        id, prev_id, self.panel_app.open_tabs
                    );
                    self.process_chart_out_event(ChartOutEvent::LoadPreset { id });
                }
            }

            ChartOutEvent::LoadPreset { id } => {
                let _lp_total = web_time::Instant::now();
                let prev_id = self.panel_app.active_preset_id.clone();
                sglog!(
                    "[LoadPreset] ENTER id='{}' prev_active='{}' open_tabs={:?} live_cache={:?} preset_files={:?}",
                    id, prev_id,
                    self.panel_app.open_tabs,
                    self.live_preset_cache.keys().cloned().collect::<Vec<_>>(),
                    self.core.presets().keys().cloned().collect::<Vec<_>>()
                );

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
                    sglog!("[LoadPreset] no-op: prev_id == id == '{}' and already_materialized=true", id);
                } else if prev_id != id && !self.try_claim_preset_for_cold_load(&id) {
                    // Refuse BEFORE anything is mutated (prev_id not yet
                    // parked, active_preset_id not yet changed) — a clean
                    // early-return, no rollback needed. Same-window
                    // re-selects (prev_id == id) skip this check entirely
                    // since try_claim_preset_for_cold_load would trivially
                    // succeed anyway (reclaiming your own preset), and the
                    // no-op branch above already handles that case.
                    sglog!("[LoadPreset] refused: '{}' already claimed by another window — staying on '{}'", id, prev_id);
                } else {
                sglog!(
                    "[LoadPreset] proceeding: already_materialized={} (prev_id != id ? {})",
                    already_materialized, prev_id != id
                );

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

                // `LoadPreset` is the "switch active to this id" path emitted
                // by chrome tab clicks.  It must NOT mutate `open_tabs` — the
                // user clicked an existing tab, the strip is already correct.
                // (`LoadPresetInActiveSlot` and `OpenTab` handle the
                // dropdown / chart-browser-load and chart-browser-new-tab
                // UX paths respectively and DO own `open_tabs` mutation.)
                //
                // Defensive append: if a caller wires LoadPreset for a preset
                // that is NOT currently in the strip (e.g. cold boot from a
                // profile snapshot where the active id was persisted without
                // its tab), the chrome strip would have no slot to show it
                // active.  Append in that case so the strip stays consistent.
                if !self.panel_app.open_tabs.iter().any(|t| t == &id) {
                    self.panel_app.open_tabs.push(id.clone());
                    sglog!("[LoadPreset] appended missing id '{}' to open_tabs (was bare-switch)", id);
                }
                self.panel_app.active_preset_id = id.clone();
                self.persist_profile();

                // Warm path: live cache hit  -  zero-flicker switch.
                let _lp_t_unpark = web_time::Instant::now();
                let unpark_hit = self.unpark_preset(&id);
                let lp_unpark_us = _lp_t_unpark.elapsed().as_micros();
                sglog!("[LoadPreset] unpark_preset('{}') -> {} (cache_keys_after={:?})",
                    id, if unpark_hit { "HIT" } else { "MISS, will cold-load" },
                    self.live_preset_cache.keys().cloned().collect::<Vec<_>>());
                {
                    let containers = self.panel_app.panel_grid.containers();
                    let active_cid = self.panel_app.panel_grid.active_container_id();
                    let summary: Vec<(u64, u64, u64)> = containers.iter()
                        .map(|(cid, w)| (cid.0, w.auto_tag_id.0, w.current_tag_id.0))
                        .collect();
                    let active_sym = active_cid
                        .and_then(|cid| self.symbol_for_chart(cid))
                        .unwrap_or_default();
                    sglog!(
                        "[LoadPreset] post-unpark active_cid={:?} sym='{}' containers(cid,auto_bid,cur_bid)={:?}",
                        active_cid, active_sym, summary
                    );
                }
                if unpark_hit {
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
                    let bar_count = self.core.user().profile().domain.bar_count as usize;
                    // Phase 4 S2.7.1c: read via branch_meta (B6 tuple-collect flip).
                    let window_bar_data: Vec<(String, String, mlc_core::state::Timeframe, mlc_core::instrument::AccountType, bool)> = self
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
                            let at: mlc_core::instrument::AccountType = self.account_type_for_chart(cid)
                                .unwrap_or_default();
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
                    for (sym, exch, tf, at, has_bars) in &window_bar_data {
                        let eid = digdigdig3::ExchangeId::from_str(exch)
                            .unwrap_or(digdigdig3::ExchangeId::Binance);
                        if !self.sidebar_state.connector_enabled.get(eid.as_str()).copied().unwrap_or(true) {
                            continue;
                        }
                        let at = mlc_backend_data_bridge::account_type_to_dig3(*at);
                        // A window can have bars but a dead kline forwarder (D4
                        // fixed the gate so this is now honestly observable) —
                        // force the refetch+resub in that case too, not only
                        // when the bubble came back empty (audit D5).
                        let kline_alive = self.core.bridge.is_kline_alive(eid, sym, at, &tf.name);
                        let force = !*has_bars || !kline_alive;
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
                // Release this window's claim (if any) — a deleted preset
                // can never be claimed again, so there is nothing left to
                // guard against a two-window collision on.
                self.core.release_preset(&id, &self.window_id);
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
                // Release this window's claim — the tab is fully closing in
                // this window, so the invariant "live in exactly one window"
                // (§5.3.5) no longer applies to it here. No-op if another
                // window's app_id holds it (release_preset only releases
                // your own claim).
                self.core.release_preset(&id, &self.window_id);
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
                    // Preset already open  -  pure redirect to that tab.
                    // Do NOT pre-set active_preset_id here: LoadPreset reads
                    // its own `prev_id` from active_preset_id and only parks
                    // when `prev != id`.  Pre-assigning makes prev == id, the
                    // park step is skipped, the source tab's live state is
                    // then dropped by the unpark mem::replace — exactly the
                    // "source tab broke" symptom.
                } else {
                    self.panel_app.open_tabs.push(id.clone());
                }
                self.process_chart_out_event(ChartOutEvent::LoadPreset { id });
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
                    if let Some(new_flag) = self.core.bubbles_mut().with_bubble_mut(gid, |group| {
                        group.sync_flags.sync_symbol = !group.sync_flags.sync_symbol;
                        group.sync_flags.sync_symbol
                    }) {
                        newly_enabled = new_flag;
                        eprintln!("[ChartApp] InternalToggleSyncSymbol: group={:?} sync_symbol={}", gid, new_flag);
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
                            .map(|at| at.to_string())
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
                    if let Some(new_flag) = self.core.bubbles_mut().with_bubble_mut(gid, |group| {
                        group.sync_flags.sync_timeframe = !group.sync_flags.sync_timeframe;
                        group.sync_flags.sync_timeframe
                    }) {
                        newly_enabled = new_flag;
                        eprintln!("[ChartApp] InternalToggleSyncTimeframe: group={:?} sync_timeframe={}", gid, new_flag);
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
                    if let Some(new_flag) = self.core.bubbles_mut().with_bubble_mut(gid, |group| {
                        group.sync_flags.sync_crosshair = !group.sync_flags.sync_crosshair;
                        group.sync_flags.sync_crosshair
                    }) {
                        newly_enabled = new_flag;
                        eprintln!("[ChartApp] InternalToggleSyncCrosshair: group={:?} sync_crosshair={}", gid, new_flag);
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
                    if let Some(was_on) = self.core.bubbles_mut().with_bubble_mut(gid, |group| {
                        let was_on = group.sync_flags.sync_viewport;
                        group.sync_flags.sync_viewport = !was_on;
                        was_on
                    }) {
                        newly_enabled = !was_on;
                        newly_disabled = was_on;
                        eprintln!("[ChartApp] InternalToggleSyncViewport: group={:?} sync_viewport={}", gid, !was_on);
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
                            .map(|w| w.price_scale().scale_mode);
                        if let Some(mode) = active_mode {
                            if let Some(gid) = gid {
                                self.core.bubbles_mut().set_scale_mode(gid, mode);
                            }
                        }
                        // Stash each peer's current mode and apply the group mode.
                        // propagate_viewport_to_sync_group applies mode to peers via the
                        // scale_mode argument; stash happens here before the call.
                        if let Some(gid) = gid {
                            let peer_leaves: Vec<mlc_core::LeafId> =
                                self.core.tags().chart_member_ids(gid)
                                    .into_iter()
                                    .filter_map(|cid| self.panel_app.panel_grid.leaf_for_container_id(cid))
                                    .filter(|&lid| lid != active_leaf)
                                    .collect();
                            for peer_leaf in peer_leaves {
                                if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(peer_leaf) {
                                    // Stash existing mode if not already stashed (preserve
                                    // the window's pre-join mode across multiple sync toggles).
                                    if window.stashed_scale_mode.is_none() {
                                        window.stashed_scale_mode = Some(window.price_scale().scale_mode);
                                    }
                                }
                            }
                        }
                        let viewport_state = self.panel_app
                            .panel_grid
                            .active_container()
                            .map(|w| (w.viewport().view_start, w.viewport().bar_spacing));
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
                            self.core.tags().chart_member_ids(gid)
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
                    let new_sync = match self.core.bubbles_mut().with_bubble_mut(gid, |group| {
                        group.sync_flags.sync_drawings = !group.sync_flags.sync_drawings;
                        group.sync_flags.sync_drawings
                    }) {
                        Some(new_sync) => {
                            eprintln!("[BubbleStore] sync_drawings toggled to {}", new_sync);
                            new_sync
                        }
                        None => return,
                    };

                    // Get auto_tag_id and current bubble id before any mutable borrows.
                    let auto_bid = chart_id.and_then(|cid| {
                        self.panel_app.panel_grid.containers().get(&cid).map(|w| w.auto_tag_id)
                    });
                    let active_bid = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);

                    // NOTE (Step 3.1): under the per-bubble drawing model, the
                    // window's `current_tag_id` is always the tag bubble
                    // once tagged — there is no separate per-window
                    // DrawingManager to stash/restore from any more.
                    // `sync_drawings` toggling the *drawing* dimension is now
                    // a no-op at this layer (the primitives always live on
                    // the bubble the window points at); the stash/restore
                    // dance below is preserved only for the auto-bubble
                    // bookkeeping other flags (indicators, viewport) still
                    // rely on. Flagged for product review in the handoff.
                    if new_sync {
                        if let (Some(bid), Some(active_bid)) = (auto_bid, active_bid) {
                            let local: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>> =
                                self.core.drawings().primitives(active_bid).iter()
                                    .filter(|p| p.data().origin_id.is_none())
                                    .map(|p| p.clone_box())
                                    .collect();
                            let stash_count = local.len();
                            self.core.bubbles_mut().with_bubble_mut(bid, |auto_bub| {
                                auto_bub.primitives = local;
                            });
                            eprintln!(
                                "[BubbleStore] sync_drawings ON: stashed {} primitives into auto-bubble {:?}",
                                stash_count, bid
                            );
                        }
                    } else if let (Some(bid), Some(active_bid)) = (auto_bid, active_bid) {
                        let restored: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>> =
                            self.with_bubble(bid, |bub| bub.primitives.iter().map(|p| p.clone_box()).collect())
                                .unwrap_or_default();
                        let count = restored.len();
                        self.core.drawings_mut().adopt(active_bid, restored);
                        eprintln!(
                            "[BubbleStore] sync_drawings OFF: restored {} primitives from auto-bubble {:?}",
                            count, bid
                        );
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
                    let new_sync = match self.core.bubbles_mut().with_bubble_mut(gid, |group| {
                        group.sync_flags.sync_indicators = !group.sync_flags.sync_indicators;
                        group.sync_flags.sync_indicators
                    }) {
                        Some(new_sync) => {
                            eprintln!("[BubbleStore] sync_indicators toggled to {}", new_sync);
                            new_sync
                        }
                        None => return,
                    };

                    let auto_bid_raw: Option<u64> = self.panel_app.panel_grid.containers()
                        .get(&chart_id)
                        .map(|w| w.auto_tag_id.0);
                    let pre_tag_ids: Vec<u64> = auto_bid_raw
                        .and_then(|bid| self.with_bubble(
                            mlc_core::ids::BubbleId(bid),
                            |bub| bub.pre_tag_indicator_ids.clone(),
                        ))
                        .unwrap_or_default();

                    if new_sync {
                        // OFF  ->  ON: hide home (auto-bubble) indicators that
                        // are listed in pre_tag_indicator_ids but not yet
                        // mirrored into the active group's indicator_configs.
                        let group_indicator_ids: std::collections::HashSet<u64> =
                            self.core.bubbles().indicator_configs(gid).iter().map(|c| c.id).collect();

                        let to_hide: Vec<u64> = {
                            let indicators_guard = self.core.indicators();
                            auto_bid_raw
                                .map(|b| indicators_guard.instances_for_bubble(mlc_core::ids::BubbleId(b)))
                                .unwrap_or_default()
                                .into_iter()
                                .filter(|i| {
                                    pre_tag_ids.contains(&i.id)
                                        && !group_indicator_ids.contains(&i.id)
                                })
                                .map(|i| i.id)
                                .collect()
                        };

                        for id in &to_hide {
                            self.core.indicators_mut().set_visible(*id, false);
                        }
                        eprintln!(
                            "[BubbleStore] sync_indicators ON: hid {} home indicators",
                            to_hide.len()
                        );
                    } else {
                        // ON  ->  OFF: unhide home (auto-bubble) indicators
                        // that were tucked away on tag join.
                        let to_show: Vec<u64> = {
                            let indicators_guard = self.core.indicators();
                            auto_bid_raw
                                .map(|b| indicators_guard.instances_for_bubble(mlc_core::ids::BubbleId(b)))
                                .unwrap_or_default()
                                .into_iter()
                                .filter(|i| pre_tag_ids.contains(&i.id) && !i.visible)
                                .map(|i| i.id)
                                .collect()
                        };

                        for id in &to_show {
                            self.core.indicators_mut().set_visible(*id, true);
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

    /// Ignite the footprint pipeline for every restored window whose
    /// chart_type is `footprint`.
    ///
    /// The startup / new-window post-load loops only fire kline
    /// `request_bars` — a hydrated footprint window never went through
    /// ChangeChartType's Step 6, so nothing subscribed its derived stream:
    /// the pane sat on the spinner forever while the kline seed painted a
    /// foreign timeline (cold-open hang, handoff 2026-07-05 §2.3). Also
    /// stamps `bars_source = FootprintOhlc` so the kline-family write gates
    /// drop the parallel kline seed for these bubbles.
    pub(crate) fn ignite_footprint_windows(&mut self) {
        let targets: Vec<(mlc_core::state::ChartId, String, String, mlc_core::instrument::AccountType, &'static str)> =
            self.panel_app.panel_grid.containers().iter()
                .filter_map(|(cid, w)| {
                    if self.chart_type_for_chart(*cid) != Some("footprint") { return None; }
                    let sym = self.symbol_for_chart(*cid)?;
                    if sym.is_empty() { return None; }
                    let exch = self.exchange_for_chart(*cid)?;
                    let at = self.account_type_for_chart(*cid)?;
                    let iv = self.panels_store.chart_panel.get(&w.chart_panel_id)
                        .map(|p| p.chart_type_interval)
                        .unwrap_or("1m");
                    Some((*cid, sym.to_string(), exch.to_string(), at, iv))
                })
                .collect();
        for (cid, sym, exch, at, iv) in targets {
            let eid = digdigdig3::ExchangeId::from_str(&exch)
                .unwrap_or(digdigdig3::ExchangeId::Binance);
            if !self.sidebar_state.connector_enabled.get(eid.as_str()).copied().unwrap_or(true) {
                continue;
            }
            let at = mlc_backend_data_bridge::account_type_to_dig3(at);
            if let Some(bid) = self.bubble_for_chart(cid) {
                self.core.bubbles_mut().with_bubble_mut(bid, |bubble| {
                    bubble.clear_bars();
                    bubble.mark_footprint_pending();
                    bubble.bars_source = mlc_backend::bubble::BarsSource::FootprintOhlc;
                });
            }
            self.core.bridge.ensure_connector(eid);
            self.core.bridge.request_footprint(eid, &sym, at, iv);
            eprintln!("[ChartApp] ignited footprint sub for restored window {:?} iv={}", cid, iv);
        }
    }

    /// Run the default-companion reconcile for EVERY open window.
    ///
    /// [`COMPANION_SYNC_RUNS`] / [`COMPANION_ADD_OK`] / [`COMPANION_ADD_FAIL`]
    /// are wasm-diagnosable counters (exported via MLC_DEBUG) — owner
    /// report 2026-07-05: VOLUME never auto-appears; these pin whether the
    /// sync ran at all vs the add silently failing.
    ///
    /// `sync_default_indicators_for_chart_type` only fires inside
    /// ChangeChartType — a restored window never passes through it, so a
    /// hydrated candles chart came up without its VOLUME companion and a
    /// hydrated footprint chart without CVD (owner report 2026-07-05 #2).
    /// Called from the startup / new-window / LoadPreset post-load paths,
    /// right after `ignite_footprint_windows`.
    pub(crate) fn sync_default_indicators_for_all_windows(&mut self) {
        COMPANION_SYNC_RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let targets: Vec<(mlc_core::state::ChartId, &'static str)> =
            self.panel_app.panel_grid.containers().keys()
                .map(|cid| (*cid, self.chart_type_for_chart(*cid).unwrap_or("candles")))
                .collect();
        for (cid, ct) in targets {
            self.sync_default_indicators_for_chart_type(cid, ct);
        }
    }

    /// Reconcile the chart's set of "default companion" indicators against
    /// what the new `chart_type` calls for.
    ///
    /// Owner rule (2026-07-05): each chart_type has one canonical companion
    /// indicator that opens alongside the base series (footprint → CVD;
    /// kline-render family → Volume). Add if missing, remove if the
    /// chart_type no longer wants it. Idempotent — safe to call on every
    /// ChangeChartType, including no-op switches.
    ///
    /// Uses the real mli catalog id (`CVD`, `VOLUME`, ...), so the sub-pane
    /// / overlay is populated by the same pipeline that drives MACD/RSI —
    /// no bespoke subscription, no bespoke renderer.
    pub(crate) fn sync_default_indicators_for_chart_type(
        &mut self,
        cid: mlc_core::ids::ChartId,
        chart_type: &str,
    ) {
        // Companion indicator per chart_type. Extend the table when adding
        // more auto-pairs (tpo → VolumeProfile, ...).
        let wanted: &[&str] = match chart_type {
            "footprint" => &["CVD"],
            // Kline-render family (candles + every OHLC-derived line/area
            // style that draws straight off `bubble.bars`, per the
            // `ChangeChartType` match arm above) — NOT the derived-seed
            // family (range_bar/tick_bar/volume_bar/dollar_bar family/
            // renko/pnf/kagi/three_line_break/footprint/tpo/heatmaps),
            // which already carry their own volume-shaped visualization or
            // no kline backdrop to pin a bottom histogram against.
            "candles" | "hollow_candles" | "heikin_ashi" | "bars"
            | "line" | "step_line" | "line_markers" | "area" | "hlc_area"
            | "baseline" | "columns" => &["VOLUME"],
            _ => &[],
        };

        let bid = match self.bubble_for_chart(cid) {
            Some(b) => b,
            None => return,
        };

        // Instance type_ids in the manager are lowercased on create
        // (indicator_bridge.rs). Compare on lowercase both sides.
        let existing_type_ids: Vec<String> = self
            .core
            .indicators()
            .instances_for_bubble(bid)
            .into_iter()
            .map(|inst| inst.type_id.to_lowercase())
            .collect();

        // ── Add missing companions ──────────────────────────────────
        let bars: Vec<mlc_core::Bar> = self
            .with_bubble(bid, |b| b.bars.clone())
            .unwrap_or_default();
        for id in wanted {
            let lower = id.to_lowercase();
            if existing_type_ids.iter().any(|t| *t == lower) {
                continue;
            }
            // `create_instance` looks definitions up CASE-SENSITIVELY and the
            // map is keyed by the lowercased catalog id (indicator_bridge.rs
            // `info.id().to_lowercase()`) — passing "CVD"/"VOLUME" verbatim
            // returns None silently. Always add by the lowercased id.
            let new_id_opt = self.core.indicators_mut().add(Some(bid), &lower);
            if new_id_opt.is_some() {
                COMPANION_ADD_OK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            } else {
                COMPANION_ADD_FAIL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if let Some(new_id) = new_id_opt {
                if !bars.is_empty() {
                    self.core.indicators_mut().recalc_bubble(bid, &bars);
                }
                self.push_undo_command(mlc_core::Command::AddIndicator {
                    instance_id: new_id,
                    type_id: lower.clone(),
                    params_json: String::new(),
                });
                eprintln!(
                    "[ChartApp] auto-added default indicator '{}' (id={}) for chart_type='{}'",
                    id, new_id, chart_type
                );
            }
        }

        // ── Remove companions no longer wanted for this chart_type ──
        // Set of default-companion type_ids across ALL chart_types — used
        // to decide "is this instance a default we own?" so we never touch
        // user-added indicators of the same type_id. Deliberately narrow:
        // only ids we ever auto-add belong here. "builtin_volume" is not
        // listed — no live `IndicatorInstance` can ever carry that type_id:
        // `IndicatorManager::create_instance{,_with_id}` always normalizes
        // to the DEFINITION's own `type_id` field ("volume"), even when
        // looked up through the migration alias (`register_legacy_aliases`).
        const AUTO_MANAGED: &[&str] = &["cvd", "volume"];
        let wanted_lower: Vec<String> = wanted.iter().map(|s| s.to_lowercase()).collect();
        let to_remove: Vec<u64> = self
            .core
            .indicators()
            .instances_for_bubble(bid)
            .into_iter()
            .filter(|inst| {
                let t = inst.type_id.to_lowercase();
                AUTO_MANAGED.contains(&t.as_str()) && !wanted_lower.contains(&t)
            })
            .map(|inst| inst.id)
            .collect();
        for id in to_remove {
            self.delete_indicator_instance(id);
            eprintln!(
                "[ChartApp] auto-removed default indicator id={} on switch to chart_type='{}'",
                id, chart_type
            );
        }

        self.sync_sub_panes_from_manager();
    }
}

/// Per-chart-type minimum-usable `bar_spacing` (px), enforced on
/// `ChangeChartType` (Wave T1 commit 1,
/// `docs/mlc/audits/series-types-ui-audit-2026-07-04.md`).
///
/// The default `Viewport::bar_spacing` is 8px (kline-tuned). Some
/// derived-chart renderers need much more horizontal room per bar before
/// their content is legible — below the threshold they show a
/// zoom-in placeholder instead of real content (see
/// `mlc-app-layout/src/chart_render/footprint.rs` `MIN_BAR_WIDTH_PX`).
/// Returns `None` when the type is fine with the inherited spacing.
///
/// Table is deliberately extensible — new entries only need a match arm.
pub fn min_usable_bar_spacing(chart_type: &str) -> Option<f64> {
    match chart_type {
        // Footprint clusters draw per-level bid/ask numbers inside the bar
        // column; `footprint.rs::MIN_BAR_WIDTH_PX = 40.0` is the hard floor
        // for text, `bar_width() = bar_spacing * bar_width_ratio` (default
        // ratio 0.8) — so `bar_spacing` must clear `40.0 / 0.8 = 50.0` for
        // the body alone to fit legible text, plus a little headroom for
        // the per-level number labels so they don't touch the column edges.
        "footprint" => Some(50.0),
        // P&F X/O shapes stretch to the box cell (`pnf.rs`); below ~12px of
        // spacing the glyph is a sliver and the column grid is unreadable.
        "pnf" => Some(12.0),
        _ => None,
    }
}
