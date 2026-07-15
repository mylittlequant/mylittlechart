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
            self.core.ensure_connector(eid);
            self.core.request_kline(eid, &sym, &tf, at, None, Some(bar_count), force);
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
                // Generic parse (consolidation, user-granularity-templates-
                // inner arc, кусок 1): strips the `tf_` dropdown-id prefix
                // and hands the bare name to `Timeframe::parse`, which
                // reproduces the former 17-arm closed match for every
                // builtin AND additionally resolves custom favorite ids
                // minted as `tf_{name}` (e.g. `tf_7m`) — same dispatch path,
                // zero extra wiring for favorites.
                let timeframe = timeframe_id.strip_prefix("tf_").and_then(Timeframe::parse);
                let timeframe = match timeframe {
                    Some(tf) => Some(tf),
                    None => {
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
                            // Granularity favorites (кусок 1/2): `allowed` is
                            // built over the builtin `ALL_TF_IDS` and can
                            // never contain a custom TF id (`tf_7m`). The
                            // toolbar DOES offer customs and starred builtins
                            // now, so membership in EITHER `time_tfs`
                            // (customs) or `starred_tfs` (a starred builtin,
                            // or a starred custom — belt-and-suspenders in
                            // case a builtin gets un-starred while still
                            // referenced) is as legitimate as a builtin —
                            // exempt it from the membership check (the guard
                            // keeps blocking arbitrary non-custom/non-starred
                            // ids forced through the agent-API bypass).
                            let user_guard = self.core.user();
                            let favs = &user_guard.profile().domain.granularity_favorites;
                            let is_favorite = timeframe_id
                                .strip_prefix("tf_")
                                .map(|n| {
                                    favs.time_tfs.iter().any(|f| f == n)
                                        || favs.starred_tfs.iter().any(|f| f == n)
                                })
                                .unwrap_or(false);
                            drop(user_guard);
                            let ok = is_favorite || allowed.contains(timeframe_id.as_str());
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
                    // `ResolutionKind::Interval` chart_types (footprint today)
                    // surface the SAME `timeframe_selector` dropdown as
                    // `Time` types (owner doctrine 2026-07-08 — "the TF
                    // button IS the resolution picker"), but a bucket-
                    // interval change is not a plain kline TF switch: it must
                    // re-subscribe the footprint stream (`request_footprint`)
                    // and stamp `chart_type_interval`, not fetch klines at
                    // the clicked TF. Route through the same helper the
                    // toolbar's inline text field uses
                    // (`apply_footprint_interval`) instead of falling into
                    // the generic kline/tick branch below — that branch has
                    // never known about footprint and previously left a
                    // dropdown-clicked bucket interval (e.g. 15s) applied
                    // only to the label, not the live subscription.
                    let is_interval_resolution = active_chart_id
                        .and_then(|cid| self.chart_type_for_chart(cid))
                        .and_then(mlc_core::chart_type::by_id)
                        .is_some_and(|def| matches!(def.resolution, mlc_core::chart_type::ResolutionKind::Interval));
                    if is_interval_resolution {
                        // `tf` was resolved from a `tf_whitelist`-gated
                        // dropdown id (footprint's whitelist and
                        // `footprint_interval_static`'s table are kept in
                        // sync — see `mlc-core::chart_type::registry`'s
                        // `FOOTPRINT_TF_WHITELIST`), so this always resolves
                        // in practice; `None` degrades to a no-op rather
                        // than panicking on a future whitelist/table drift.
                        if let Some(static_iv) = Self::footprint_interval_static(&tf.name) {
                            self.apply_footprint_interval(static_iv, tf.interval_ms);
                            self.sidebar_data_dirty = true;
                            state_mutated = true;
                        }
                    } else {
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
                        // Fetch klines for the new timeframe. Tick TFs are no
                        // longer reachable through this handler (removed from
                        // the TF dropdown 2026-07-08 — they now live as the
                        // `tick_bar` chart_type's own `chart_type_param`, see
                        // `hydration_impl.rs`'s legacy-preset migration), so
                        // this is unconditionally the kline path.
                        if !symbol.is_empty() {
                            // Per-container venue, NOT the app-global
                            // `active_exchange` (2026-07-15 wrong-venue bug —
                            // see `exchange_id_for_chart`'s doc).
                            let chart_eid = active_chart_id
                                .map(|cid| self.exchange_id_for_chart(cid))
                                .unwrap_or(self.active_exchange);
                            let eid_str = chart_eid.as_str();
                            if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                                eprintln!("[ChartApp] Exchange {} is disabled, skipping data fetch (timeframe change)", eid_str);
                            } else {
                                let at = mlc_backend_data_bridge::account_type_to_dig3(at_tf);
                                // Drop the OLD kline forwarder so it stops piping
                                // 1h BarUpdates to the broadcast after the user
                                // switched to 1m.
                                if let Some(ref prev_tf) = prev_tf_opt {
                                    self.core.abort_kline_bars(chart_eid, &symbol, at, &prev_tf.name);
                                }
                                self.core.request_kline(chart_eid, &symbol, &tf, at, None, Some(self.core.user().profile().domain.bar_count as usize), true);
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
                    // "columns" fully deleted 2026-07-06 (chart-type registry
                    // arc Step 13) — falls into the `_` arm below and is
                    // rejected like any other unknown chart_type id. Legacy
                    // preset restore still resolves it to "candles" via
                    // `hydration_impl.rs::static_ct`'s `by_id(...).unwrap_or`.
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
                    // "dom_heatmap"/"liquidation_heatmap" REMOVED (chart-type →
                    // overlay migration) — both are now builtin overlay
                    // indicators (ChartOutEvent::Toggle{Dom,Liquidation}HeatmapOverlay
                    // above), not chart_type switch targets. Legacy preset ids
                    // fall into the `_` arm below and hydrate to "candles" via
                    // `hydration_impl.rs`'s `by_id(...).unwrap_or("candles")`.
                    _ => {
                        eprintln!("[ChartApp] unknown chart_type: {}", chart_type);
                        None
                    }
                };
                if let Some(ct) = static_type {
                    let active_cid = self.panel_app.panel_grid.active_container_id();
                    let bid = active_cid.and_then(|cid| self.bubble_for_chart(cid));
                    // Captured BEFORE the mutation below so the D9 kline-forwarder
                    // refcount logic knows what family the window is LEAVING.
                    let previous_ct: Option<&'static str> = bid
                        .and_then(|bid| self.core.bubbles().chart_type(bid));
                    // Last bar close of the OUTGOING series, captured BEFORE
                    // any clear runs below (`clear_slot_trade_agg_bars` wipes
                    // `bubble.bars` a few steps down) — the instrument-
                    // relative default resolver (granularity arc, кусок 2)
                    // needs this as its live-price input; a price read AFTER
                    // the clear would always see an empty array.
                    let pre_switch_last_price: Option<f64> = bid
                        .and_then(|bid| self.core.bubbles().bars(bid).last().map(|b| b.close))
                        .filter(|p| *p > 0.0);
                    // Box-size mode carried by the OUTGOING chart_type,
                    // captured BEFORE `set_chart_type` resets it to `Fixed`
                    // below (granularity arc, кусок 2) — lets a switch back
                    // INTO renko/kagi on a bubble that was last in ATR(14)
                    // mode on this SAME type recompute instead of silently
                    // reverting to Fixed (e.g. renko[Atr14] -> candles ->
                    // renko again). A genuine cross-family switch (candles ->
                    // renko for the first time) reads `Fixed` here, same as
                    // today.
                    let pre_switch_param_mode: Option<mlc_core::chart_type::ParamMode> = bid
                        .and_then(|bid| self.core.bubbles().chart_type_param_mode(bid));
                    // Render-only switch (both previous and target are plain
                    // render styles over the same kline bars) — computed here,
                    // early, so the min-spacing enforcement right below does
                    // NOT depend on the symbol/account-type/timeframe/connector
                    // guard chain further down (that chain gates subscription/
                    // abort bookkeeping, which spacing has nothing to do with —
                    // see the 2026-07-11 owner report: a live switch left
                    // bar_spacing at the inherited 8px because ONE of those
                    // unrelated guards — e.g. `connector_enabled` false, or the
                    // window's account_type/timeframe not yet resolved — failed
                    // and silently skipped the nested `ensure_min_bar_spacing`
                    // call along with it).
                    let is_render_family = |t: Option<&'static str>| matches!(
                        t,
                        Some("candles") | Some("bars") | Some("hollow_candles")
                        | Some("heikin_ashi") | Some("line") | Some("step_line")
                        | Some("line_markers") | Some("area") | Some("hlc_area")
                        | Some("baseline")
                    );
                    let render_only_switch = is_render_family(previous_ct) && is_render_family(Some(ct));
                    let (mut chart_param, chart_interval): (f64, &'static str) = if let Some(bid) = bid {
                        let previous_type = previous_ct.unwrap_or("candles").to_string();
                        self.core.bubbles_mut().set_chart_type(bid, ct);
                        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                            window.command_history.push(mlc_core::state::Command::ChangeChartType {
                                previous_type,
                                new_type: ct.to_string(),
                            });
                            // F6 (viewport doctrine audit 2026-07-07) intent
                            // preserved — skip on a pure render-only switch so
                            // `view_start` never moves for candles↔line. Fixed
                            // 2026-07-11: applied HERE, unconditional on
                            // symbol/account_type/timeframe/connector state
                            // (previously nested three guards deep below,
                            // where any one of those failing silently dropped
                            // the clamp — see comment above).
                            if !render_only_switch {
                                if let Some(min_spacing) = min_usable_bar_spacing(ct) {
                                    window.ensure_min_bar_spacing(min_spacing);
                                }
                            }
                        }
                        let bubbles = self.core.bubbles();
                        (bubbles.chart_type_param(bid).unwrap_or(0.0), bubbles.chart_type_interval(bid).unwrap_or("1m"))
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
                    let is_derived_target = crate::active_series::chart_type_is_derived_seed(ct);
                    if let Some(active_leaf) = self.panel_app.panel_grid.active_container() {
                        let cid = active_leaf.id;
                        let symbol_opt = self.symbol_for_chart(cid);
                        let at_opt: Option<mlc_core::instrument::AccountType> = self.account_type_for_chart(cid);
                        let tf_opt = self.timeframe_for_chart(cid);
                        if let (Some(symbol), Some(at), Some(tf)) = (symbol_opt, at_opt, tf_opt) {
                            if !symbol.is_empty() {
                                // Per-container venue, NOT the app-global
                                // `active_exchange` (2026-07-15 wrong-venue
                                // bug: on a container whose exchange differs
                                // from the global, every subscribe below went
                                // to the wrong venue → data landed in a layer
                                // no bubble reads → empty chart forever).
                                let chart_eid = self.exchange_id_for_chart(cid);
                                let eid_str = chart_eid.as_str();
                                let connector_enabled = self.sidebar_state.connector_enabled
                                    .get(eid_str).copied().unwrap_or(true);
                                if connector_enabled {
                                    let at = mlc_backend_data_bridge::account_type_to_dig3(at);

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
                                    let was_kline_family = previous_ct.map_or(true, |ct| {
                                        !crate::active_series::chart_type_is_derived_seed(ct)
                                    });
                                    // NOTE: footprint used to be exempted here via a
                                    // `target_needs_kline_backdrop` flag — stale since
                                    // 441d623d dropped the kline backdrop (footprint IS
                                    // its own OHLC series). It now releases the shared
                                    // kline forwarder like every other derived type.
                                    if was_kline_family && is_derived_target {
                                        let still_needed = self.panel_app.panel_grid.containers().iter()
                                            .any(|(other_cid, _)| {
                                                if *other_cid == cid { return false; }
                                                let same_instrument = self.symbol_for_chart(*other_cid).as_deref() == Some(symbol.as_str())
                                                    && self.exchange_for_chart(*other_cid).as_deref() == Some(eid_str)
                                                    && self.account_type_for_chart(*other_cid) == Some(
                                                        self.account_type_for_chart(cid).unwrap_or_default(),
                                                    )
                                                    && self.timeframe_for_chart(*other_cid).map(|t| t.name.clone()) == Some(tf.name.clone());
                                                if !same_instrument { return false; }
                                                let other_ct = self.chart_type_for_chart(*other_cid).unwrap_or("candles");
                                                !crate::active_series::chart_type_is_derived_seed(other_ct)
                                            });
                                        if !still_needed {
                                            self.core.abort_kline_bars(chart_eid, &symbol, at, &tf.name);
                                        }
                                    }

                                    // Render-only switch (both previous and target are
                                    // plain render styles over the same kline bars):
                                    // preserve the viewport. `begin_fresh_load` /
                                    // `needs_snap_after_backfill` / `request_bars(force)`
                                    // are ONLY needed when the underlying series changes
                                    // (kline↔derived, or between two derived families).
                                    // For candles→line the bars are identical, just
                                    // rendered differently — resetting `view_start=0` +
                                    // snap-to-end throws the user out of any scroll-back
                                    // position they were holding. Owner report 2026-07-06.
                                    // `render_only_switch` itself is computed earlier now
                                    // (see the min-spacing fix above) — reused here as-is.

                                    // Step 1: always abort any derived
                                    // subscription. The bridge.abort_* call is
                                    // idempotent when nothing's live.
                                    if !render_only_switch {
                                        self.core.bridge_handle().abort_derived_chart(
                                            chart_eid, &symbol, at,
                                        );
                                    }

                                    // Steps 2-5: mutate the active window.
                                    // X axis is always index-packed (see
                                    // chart-types-xaxis-audit-2026-07-03.md).
                                    // Skipped for render-only switches (see above).
                                    if !render_only_switch {
                                        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                                            window.begin_fresh_load();
                                            // Mark "snap to end on first data" so
                                            // the new stream lands at the right
                                            // edge of the viewport.
                                            window.needs_snap_after_backfill = true;
                                            // `ensure_min_bar_spacing` now applied
                                            // unconditionally right after
                                            // `set_chart_type` above (2026-07-11
                                            // fix) — not re-applied here, this
                                            // block is gated behind
                                            // symbol/account_type/timeframe/
                                            // connector state that has nothing to
                                            // do with spacing.
                                        }
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
                                    // Full wipe of every derived slot (defensive —
                                    // we're switching AWAY from a possibly-unknown
                                    // previous derived type) followed by the
                                    // registry's own type-specific clear (stamps
                                    // `bars_source` + owns footprint's
                                    // clear_bars/mark_footprint_pending). Some
                                    // slots get cleared twice; harmless.
                                    //
                                    // Footprint hybrid (hybrid-data-modes-plan-
                                    // 2026-07-08, Step 3): resolved once here —
                                    // reused below both to bypass the generic
                                    // `ops.clear_slot` table entry (its
                                    // fn-pointer type carries no capability
                                    // info, so only a call site with
                                    // `(exchange_id, account_type)` in scope can
                                    // resolve it) and to decide the backdrop-TF
                                    // realignment after the TF flip.
                                    let fp_hybrid_backdrop = ct == "footprint"
                                        && mlc_backend::chart_type::is_footprint_hybrid(
                                            self.core.bridge_handle(), chart_eid, at,
                                        );
                                    if !render_only_switch {
                                        if let Some(bid) = self.bubble_for_chart(cid) {
                                            self.core.bubbles_mut().with_bubble_mut(bid, |bubble| {
                                                bubble.clear_derived_slots(None);
                                                if ct == "footprint" {
                                                    mlc_backend::chart_type::clear_slot_footprint_hybrid_aware(bubble, fp_hybrid_backdrop);
                                                } else if let Some(ops) = mlc_backend::chart_type::ops_by_id(ct) {
                                                    (ops.clear_slot)(bubble);
                                                }
                                            });
                                        }
                                    }
                                    let _ = is_derived_target; // axis flip done in prepare_frame

                                    // Step 6 (chart-type registry arc, Step 10):
                                    // fire the appropriate subscription for the
                                    // new mode through the `SubscribeOps` engine
                                    // (Step 8). Footprint's TF flip (bucket
                                    // interval → bubble timeframe) still needs
                                    // its own bespoke handling here — the engine's
                                    // `subscribe_footprint` only touches
                                    // `DataBridge`, not the bubble's `Timeframe`.
                                    if ct == "footprint" {
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
                                        let iv_ms = match iv {
                                            "1m" => 60_000, "3m" => 180_000, "5m" => 300_000,
                                            "15m" => 900_000, "30m" => 1_800_000,
                                            "1h" => 3_600_000, _ => 60_000,
                                        };
                                        let footprint_tf = mlc_core::state::Timeframe::new(iv, iv_ms);
                                        if let Some(active_bid) = self.panel_app.panel_grid.active_container()
                                            .map(|w| w.current_tag_id)
                                        {
                                            self.core.bubbles_mut().set_timeframe(active_bid, footprint_tf.clone());
                                        }
                                        // Hybrid backdrop TF alignment (Step 3,
                                        // plan risk note): the kline sub left
                                        // running by `clear_slot_footprint_hybrid_aware`
                                        // is whatever the PREVIOUS chart_type was
                                        // subscribed at (`tf`, e.g. "1h") — that
                                        // can differ from the footprint bucket
                                        // interval (`footprint_tf`, e.g. "1m").
                                        // A mismatched backdrop TF desyncs the
                                        // index axis between `bubble.bars` (kline
                                        // backdrop) and `footprint_bars` (bucket
                                        // grid). Simplest fix per the plan:
                                        // backdrop TF == footprint interval,
                                        // ALWAYS, in hybrid mode — abort the old-TF
                                        // kline sub and re-request kline bars
                                        // fresh at the bucket interval before
                                        // `subscribe_footprint` fires below.
                                        if fp_hybrid_backdrop && tf.name != iv {
                                            self.core.bridge_handle().abort_kline_bars(
                                                chart_eid, &symbol, at, &tf.name,
                                            );
                                            let bar_count = self.core.user().profile().domain.bar_count as usize;
                                            self.core.bridge_handle().request_bars(
                                                chart_eid, &symbol, &footprint_tf, at,
                                                None, Some(bar_count), true,
                                            );
                                        }
                                    }
                                    if ct == "tpo" {
                                        // TPO sessions are UTC-daily by
                                        // construction — the display TF is the
                                        // 1D INVARIANT (registry tf_whitelist
                                        // is ["tf_1d"]); flip the bubble label
                                        // so the toolbar clock reads 1D, not
                                        // the previous chart's leftover TF.
                                        let daily = mlc_core::state::Timeframe::new("1D", 86_400_000);
                                        if let Some(active_bid) = self.panel_app.panel_grid.active_container()
                                            .map(|w| w.current_tag_id)
                                        {
                                            self.core.bubbles_mut().set_timeframe(active_bid, daily);
                                        }
                                    }
                                    if let Some(ops) = mlc_backend::chart_type::ops_by_id(ct) {
                                        // Render-family (candles/bars/line/...) shares
                                        // `subscribe_kline_bars` with the legacy `_`
                                        // fallback arm below, which only fired when
                                        // NOT a render-only switch (bars already
                                        // loaded — re-requesting would trigger the
                                        // fresh-load viewport jump this arc is
                                        // fixing). Every other (derived) chart_type
                                        // always fires, matching the per-arm
                                        // behavior this table replaces.
                                        let skip = is_render_family(Some(ct)) && render_only_switch;
                                        if !skip {
                                            let bar_count = self.core.user().profile().domain.bar_count as usize;
                                            let tick_size = self.resolve_tick_size(eid_str, &symbol, bid);
                                            // Instrument-relative default resolution (granularity
                                            // arc, кусок 2): the sentinel (`chart_param <= 0.0`,
                                            // i.e. the user hasn't picked a value on THIS bubble
                                            // yet) resolves through the live resolver first —
                                            // range_bar/volume_bar get an instrument-relative
                                            // value instead of the registry's static fallback.
                                            // Written back via `set_chart_type_param` BEFORE
                                            // `build_subscribe_params` runs, so the toolbar button
                                            // and settings modal show the REAL resolved number
                                            // (e.g. "500R") and it persists like any user pick.
                                            if chart_param <= 0.0 {
                                                if let Some(def) = mlc_core::chart_type::by_id(ct) {
                                                    // `pre_switch_last_price` was captured before
                                                    // this fn's own clear_slot ran, above; fall back
                                                    // to mini_ticker if the outgoing series had no
                                                    // bars yet (cold subscribe on this bubble).
                                                    let last_price = pre_switch_last_price.or_else(|| {
                                                        self.resolve_last_price(eid_str, &symbol, at.short_label(), None)
                                                    });
                                                    if let Some(resolved) = mlc_core::chart_type::resolve_instrument_relative_default(
                                                        def, tick_size, last_price.unwrap_or(0.0),
                                                    ) {
                                                        chart_param = resolved;
                                                        if let Some(bid) = bid {
                                                            self.core.bubbles_mut().set_chart_type_param(bid, resolved);
                                                        }
                                                    }
                                                }
                                            }
                                            // ATR(14) recompute-on-switch (granularity arc,
                                            // кусок 2): a switch INTO renko/kagi whose
                                            // bubble carried `Atr14` mode BEFORE
                                            // `set_chart_type` reset it (captured as
                                            // `pre_switch_param_mode` at the top of this
                                            // handler — e.g. renko[Atr14] -> candles ->
                                            // renko again) recomputes the box from the
                                            // CURRENT instrument instead of reverting to
                                            // Fixed silently. Re-applies the mode too (the
                                            // reset above flipped it to Fixed).
                                            if matches!(ct, "renko" | "kagi")
                                                && pre_switch_param_mode == Some(mlc_core::chart_type::ParamMode::Atr14)
                                            {
                                                if let Some(bid) = bid {
                                                    if let Some(ticks) = self.core.atr14_box_ticks(bid, tick_size) {
                                                        chart_param = ticks;
                                                        self.core.bubbles_mut().set_chart_type_param(bid, ticks);
                                                    }
                                                    self.core.bubbles_mut().set_chart_type_param_mode(bid, mlc_core::chart_type::ParamMode::Atr14);
                                                }
                                            }
                                            let params = build_subscribe_params(ct, chart_param, chart_interval, bar_count, tick_size);
                                            // Unified-data-plane Э3: the 7 kline-sufficient
                                            // fold types install/reseed a client-side
                                            // `FoldRuntime` INSTEAD of `ops.subscribe`
                                            // opening a station-side derived sub — that
                                            // fn-pointer slot is a documented no-op for
                                            // these 7 (see `subscribe_fold_noop`).
                                            if mlc_backend::chart_type::is_fold_chart_type(ct) {
                                                if let (Some(bid), Some(fold_params)) = (
                                                    self.bubble_for_chart(cid),
                                                    mlc_backend::chart_type::fold_params_from_subscribe_params(ct, params),
                                                ) {
                                                    let derived_target = self.core.user().profile().domain.data_load.derived_kline_target;
                                                    if let Some(ev) = self.core.subscribe_fold_chart_type(
                                                        bid, ct, fold_params, chart_eid, &symbol, at, Some(derived_target),
                                                    ) {
                                                        self.react_to_bubble_event(&ev, now_ms());
                                                    }
                                                }
                                            } else if mlc_backend::chart_type::is_trade_fold_chart_type(ct) {
                                                // Unified-data-plane Э4: the 6 trade-fed
                                                // chart types install/reseed a client-side
                                                // `TradeFoldRuntime` over the bubble's own
                                                // trade layer INSTEAD of `ops.subscribe`
                                                // opening a station-side derived sub —
                                                // mirrors the Э3 branch above one layer
                                                // over (trades, not "1m" klines). Footprint's
                                                // `bars_source` was already stamped by
                                                // `clear_slot_footprint_hybrid_aware` above
                                                // (or `ops.clear_slot` for every other trade-
                                                // fold type) BEFORE this fires — the fold
                                                // reseed reads it to decide the OHLC mirror.
                                                if let (Some(bid), Some(mut fold_params)) = (
                                                    self.bubble_for_chart(cid),
                                                    mlc_backend::chart_type::trade_fold_params_from_subscribe_params(ct, params),
                                                ) {
                                                    // Footprint row-grouping preset (canon-fix
                                                    // wave 2, item 5) — persisted per-panel, not
                                                    // carried by `SubscribeParams`; apply it here
                                                    // so switching TO footprint on a panel with a
                                                    // saved grouping preset seeds grouped from the
                                                    // first fold, same as `ignite_derived_windows`'s
                                                    // cold-restore path and the live-toggle path
                                                    // (`apply_footprint_group_ticks`).
                                                    if ct == "footprint" {
                                                        let group_ticks = self.panel_app.panel_grid.containers().get(&cid)
                                                            .and_then(|w| self.panels_store.chart_panel.get(&w.chart_panel_id))
                                                            .map(|p| p.footprint_display.group_ticks)
                                                            .unwrap_or(0);
                                                        if group_ticks > 0 {
                                                            fold_params.group_step = group_ticks as f64 * tick_size;
                                                        }
                                                    }
                                                    let derived_target = self.core.user().profile().domain.data_load.derived_trade_target;
                                                    if let Some(ev) = self.core.subscribe_trade_fold_chart_type(
                                                        bid, ct, fold_params, mlc_backend::chart_type::DEFAULT_TPO_SOURCE,
                                                        chart_eid, &symbol, at, Some(derived_target),
                                                    ) {
                                                        self.react_to_bubble_event(&ev, now_ms());
                                                    }
                                                }
                                            } else if let Some(subscribe) = ops.subscribe {
                                                subscribe(self.core.bridge_handle(), chart_eid, &symbol, at, &tf, params);
                                            }
                                        }
                                        // The CVD sub-pane companion for footprint
                                        // is handled uniformly by
                                        // `sync_default_indicators_for_chart_type`
                                        // below (adds the real mli `CVD` indicator
                                        // to the chart's bubble, computed off the
                                        // bar stream like MACD/RSI — no bespoke
                                        // sub, no bespoke render path).
                                    } else {
                                        // Unknown chart_type (legacy preset) — fall
                                        // through to plain kline.
                                        if !render_only_switch {
                                            self.core.request_kline(
                                                chart_eid, &symbol, &tf, at,
                                                None,
                                                Some(self.core.user().profile().domain.bar_count as usize),
                                                true,
                                            );
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
                let active_cid = self.panel_app.panel_grid.active_container_id();
                let bid = active_cid.and_then(|cid| self.bubble_for_chart(cid));
                // Route off the bubble's own `chart_type` rather than the
                // toolbar-level `active_param_chart_type` field — that field
                // is only ever set by the dead inline-dropdown-input path
                // (removed, see dropdown.rs:590-613) and is permanently
                // `None`, so every param edit used to silently no-op. The
                // modal (and any other caller) already has the active
                // bubble resolved when it fires this event, so
                // `bubble.chart_type` is the correct, always-populated
                // source of truth for "which derived type is being
                // parameterized".
                let active_ct: &'static str = bid
                    .and_then(|bid| self.core.bubbles().chart_type(bid))
                    .unwrap_or("candles");
                if let Some(bid) = bid {
                    self.core.bubbles_mut().set_chart_type_param(bid, value);
                    // Any explicit numeric pick is a Fixed value by
                    // definition — resets an ATR(14) mode left over from a
                    // prior `SetChartTypeParamAtr14` on this bubble
                    // (granularity arc, кусок 2).
                    self.core.bubbles_mut().set_chart_type_param_mode(bid, mlc_core::chart_type::ParamMode::Fixed);
                }
                if let Some(active_leaf) = self.panel_app.panel_grid.active_container() {
                    let cid = active_leaf.id;
                    let symbol_opt = self.symbol_for_chart(cid);
                    let at_opt: Option<mlc_core::instrument::AccountType> = self.account_type_for_chart(cid);
                    if let (Some(symbol), Some(at)) = (symbol_opt, at_opt) {
                        if !symbol.is_empty() {
                            // Per-container venue (2026-07-15 wrong-venue bug).
                            let chart_eid = self.exchange_id_for_chart(cid);
                            let eid_str = chart_eid.as_str();
                            if self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                                let at = mlc_backend_data_bridge::account_type_to_dig3(at);
                                let tf = self.timeframe_for_chart(cid).unwrap_or_default();
                                let chart_interval: &'static str = bid
                                    .and_then(|bid| self.core.bubbles().chart_type_interval(bid))
                                    .unwrap_or("1m");
                                let bar_count = self.core.user().profile().domain.bar_count as usize;
                                // Audit D9: a param change (e.g. renko box=10 -> box=20)
                                // re-subscribes on the SAME chart_type — ChangeChartType's
                                // clear never fires. append_renko/append_pnf/... only ever
                                // push, so without this the new box size's bricks land
                                // after the stale old-box bricks in the same slot. Clear
                                // the target's own slot (keep=None) right before the
                                // re-subscribe so it starts from empty; the re-sub below
                                // repopulates it with fresh data at the new param.
                                //
                                // `clear_derived_slots(None)` alone never touches
                                // `bubble.bars` (see its doc — kline/trade-agg-family
                                // storage is deliberately excluded there), so on
                                // trade-agg param changes (e.g. 1V -> 1000V) the OLD
                                // bars lingered, `has_any_series_data()` stayed true,
                                // and the loading skeleton never showed during the
                                // multi-second reseed. Also invoke the active type's
                                // own `ops.clear_slot` (mirrors ChangeChartType's
                                // arm at chart_out_events.rs:651-660) so trade-agg/
                                // renko/pnf/etc. bars are wiped too. Footprint routes
                                // through the hybrid-aware clear directly (same
                                // reasoning as the ChangeChartType arm — only call
                                // sites with `(exchange_id, account_type)` in scope
                                // can resolve `is_footprint_hybrid`); every other
                                // registered type uses the generic table entry.
                                if let Some(bid) = self.bubble_for_chart(cid) {
                                    self.core.bubbles_mut().with_bubble_mut(bid, |bubble| {
                                        bubble.clear_derived_slots(None);
                                        if active_ct == "footprint" {
                                            let hybrid = mlc_backend::chart_type::is_footprint_hybrid(
                                                self.core.bridge_handle(), chart_eid, at,
                                            );
                                            mlc_backend::chart_type::clear_slot_footprint_hybrid_aware(bubble, hybrid);
                                        } else if let Some(ops) = mlc_backend::chart_type::ops_by_id(active_ct) {
                                            (ops.clear_slot)(bubble);
                                        }
                                    });
                                }
                                // P0 V1 (viewport doctrine audit 2026-07-07): param
                                // change = new series identity = same cold-load
                                // contract as UpdateChartTypeInterval below.
                                if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                                    window.begin_fresh_load();
                                    window.needs_snap_after_backfill = true;
                                }
                                let tick_size = self.resolve_tick_size(eid_str, &symbol, bid);
                                let params = build_subscribe_params(active_ct, value, chart_interval, bar_count, tick_size);
                                // Unified-data-plane Э3, param-change path: for
                                // the 7 fold types this is a pure re-fold — new
                                // params → fresh machine → reseed from the SAME
                                // "1m" layer, zero network. See the
                                // `ChangeChartType` arm above for the matching
                                // chart-type-switch cutover.
                                if mlc_backend::chart_type::is_fold_chart_type(active_ct) {
                                    if let (Some(bid), Some(fold_params)) = (
                                        bid,
                                        mlc_backend::chart_type::fold_params_from_subscribe_params(active_ct, params),
                                    ) {
                                        let derived_target = self.core.user().profile().domain.data_load.derived_kline_target;
                                        if let Some(ev) = self.core.subscribe_fold_chart_type(
                                            bid, active_ct, fold_params, chart_eid, &symbol, at, Some(derived_target),
                                        ) {
                                            self.react_to_bubble_event(&ev, now_ms());
                                        }
                                    }
                                } else if mlc_backend::chart_type::is_trade_fold_chart_type(active_ct) {
                                    // Unified-data-plane Э4, param-change path: for
                                    // the 6 trade-fed types this is a full refold —
                                    // new params → fresh machine → replay the SAME
                                    // trade layer, zero network. Mirrors the Э3
                                    // branch above one layer over.
                                    if let (Some(bid), Some(fold_params)) = (
                                        bid,
                                        mlc_backend::chart_type::trade_fold_params_from_subscribe_params(active_ct, params),
                                    ) {
                                        let derived_target = self.core.user().profile().domain.data_load.derived_trade_target;
                                        if let Some(ev) = self.core.subscribe_trade_fold_chart_type(
                                            bid, active_ct, fold_params, mlc_backend::chart_type::DEFAULT_TPO_SOURCE,
                                            chart_eid, &symbol, at, Some(derived_target),
                                        ) {
                                            self.react_to_bubble_event(&ev, now_ms());
                                        }
                                    }
                                } else if let Some(subscribe) = mlc_backend::chart_type::ops_by_id(active_ct).and_then(|ops| ops.subscribe) {
                                    subscribe(self.core.bridge_handle(), chart_eid, &symbol, at, &tf, params);
                                }
                            }
                        }
                    }
                }
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                state_mutated = true;
            }

            // ATR(14) box-size mode (granularity arc, кусок 2) — renko/kagi
            // only. Recompute cadence contract: this event fires ONLY at
            // explicit pick time (this handler) or at a chart-type switch
            // INTO renko/kagi when the bubble's mode was already `Atr14`
            // (the `ChangeChartType`/`ignite_derived_window_target` recompute
            // blocks, which call `atr14_box_ticks` directly rather than
            // re-dispatching this event). There is deliberately NO live/
            // continuous recompute (no timer, no re-fire on every tick) —
            // out of scope for this arc; a future "keep ATR fresh" mode would
            // need its own deferred-recompute machine, not built here.
            ChartOutEvent::SetChartTypeParamAtr14 => {
                eprintln!("[ChartApp] SetChartTypeParamAtr14");
                let Some(cid) = self.panel_app.panel_grid.active_container_id() else {
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    return;
                };
                let Some(bid) = self.bubble_for_chart(cid) else {
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    return;
                };
                let active_ct: &'static str = self.core.bubbles().chart_type(bid).unwrap_or("candles");
                if !matches!(active_ct, "renko" | "kagi") {
                    eprintln!("[ChartApp] SetChartTypeParamAtr14: ignored, active chart_type={active_ct} is not renko/kagi");
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    return;
                }
                // 1. Set mode = Atr14 up front (bubble-level, survives even
                //    if the compute below returns None — the task spec: "on
                //    None: leave the current param untouched, log, mode
                //    stays Atr14").
                self.core.bubbles_mut().set_chart_type_param_mode(bid, mlc_core::chart_type::ParamMode::Atr14);
                // 2. Compute the box.
                let Some(symbol) = self.symbol_for_chart(cid).filter(|s| !s.is_empty()) else {
                    eprintln!("[ChartApp] SetChartTypeParamAtr14: no active symbol, leaving param untouched");
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    return;
                };
                let chart_eid = self.exchange_id_for_chart(cid);
                let eid_str = chart_eid.as_str();
                let tick_size = self.resolve_tick_size(eid_str, &symbol, Some(bid));
                match self.core.atr14_box_ticks(bid, tick_size) {
                    Some(ticks) => {
                        // 3. Re-subscribe through the EXACT same path
                        //    `UpdateChartTypeParam` uses (clear slot + fold
                        //    re-install) — reused, not duplicated. That
                        //    handler resets mode to `Fixed`, so re-stamp
                        //    `Atr14` immediately after it returns.
                        self.process_chart_out_event(ChartOutEvent::UpdateChartTypeParam { value: ticks });
                        self.core.bubbles_mut().set_chart_type_param_mode(bid, mlc_core::chart_type::ParamMode::Atr14);
                        return;
                    }
                    None => {
                        eprintln!(
                            "[ChartApp] SetChartTypeParamAtr14: fewer than 15 completed hourly bars (or tick_size<=0) for {symbol}, leaving param untouched"
                        );
                    }
                }
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }

            ChartOutEvent::UpdateChartTypeInterval { interval } => {
                eprintln!("[ChartApp] UpdateChartTypeInterval: {}", interval);
                // Free-text inline field (footprint's toolbar text input) —
                // only wall-clock-minute-and-up values are accepted here;
                // the dropdown path (`ChangeTimeframe`, whitelist-gated by
                // `tf_whitelist`) reuses `apply_footprint_interval` directly
                // with an already-validated interval/ms pair, including the
                // sub-minute (5s/15s/30s) buckets this text field rejects.
                let Some(static_iv) = Self::footprint_interval_static(interval.trim())
                    .filter(|iv| !matches!(*iv, "5s" | "15s" | "30s"))
                else {
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    return;
                };
                // `static_iv` already passed `footprint_interval_static`'s
                // validation (builtin literal or a parsed+clamped custom),
                // so `Timeframe::parse` here cannot fail in practice —
                // derive `iv_ms` from it directly instead of a second
                // closed literal match (which used to default unknown
                // customs to a wrong 60_000, per-family scoping arc
                // 2026-07-16 fix). `unwrap_or(60_000)` only guards a
                // theoretical future drift between the two functions.
                let iv_ms: i64 = Timeframe::parse(static_iv)
                    .map(|tf| tf.interval_ms)
                    .unwrap_or(60_000);
                self.apply_footprint_interval(static_iv, iv_ms);
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                state_mutated = true;
            }
            ChartOutEvent::UpdateFootprintGroupTicks { ticks } => {
                eprintln!("[ChartApp] UpdateFootprintGroupTicks: {}", ticks);
                self.apply_footprint_group_ticks(ticks);
                state_mutated = true;
            }
            ChartOutEvent::CommitGranularityCustom { target } => {
                eprintln!("[ChartApp] CommitGranularityCustom: {:?}", target);
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                // Edit mode: remove the original custom (+ its star, if any)
                // BEFORE adding the new value — a rename keeps its star
                // because the star is re-added under the new value below
                // when it was present on the original.
                let editing_original = match self.granularity_edit.take() {
                    Some(crate::GranularityEdit { kind: crate::GranularityEditKind::EditTf { original }, .. }) => {
                        Some(mlc_core::chart_type::GranularityTarget::Tf { name: original })
                    }
                    Some(crate::GranularityEdit { kind: crate::GranularityEditKind::EditParam { original }, .. }) => {
                        let chart_type = match &target {
                            mlc_core::chart_type::GranularityTarget::Param { chart_type, .. } => chart_type.clone(),
                            mlc_core::chart_type::GranularityTarget::Tf { .. }
                            | mlc_core::chart_type::GranularityTarget::Interval { .. } => {
                                eprintln!("[ChartApp] CommitGranularityCustom: EditParam target mismatch, ignored");
                                return;
                            }
                        };
                        Some(mlc_core::chart_type::GranularityTarget::Param { chart_type, value: original })
                    }
                    Some(crate::GranularityEdit { kind: crate::GranularityEditKind::EditInterval { original }, .. }) => {
                        Some(mlc_core::chart_type::GranularityTarget::Interval { name: original })
                    }
                    _ => None,
                };
                let mut was_starred = false;
                if let Some(orig_target) = editing_original {
                    let mut user_mut = self.core.user_mut();
                    let favs = &mut user_mut.profile_mut().domain.granularity_favorites;
                    match &orig_target {
                        mlc_core::chart_type::GranularityTarget::Tf { name } => {
                            was_starred = favs.starred_tfs.iter().any(|n| n == name);
                            favs.time_tfs.retain(|n| n != name);
                            favs.starred_tfs.retain(|n| n != name);
                        }
                        mlc_core::chart_type::GranularityTarget::Param { chart_type, value } => {
                            was_starred = favs.starred_params.iter()
                                .any(|f| &f.chart_type == chart_type && f.value == *value);
                            favs.params.retain(|f| !(&f.chart_type == chart_type && f.value == *value));
                            favs.starred_params.retain(|f| !(&f.chart_type == chart_type && f.value == *value));
                        }
                        mlc_core::chart_type::GranularityTarget::Interval { name } => {
                            was_starred = favs.starred_intervals.iter().any(|n| n == name);
                            favs.interval_tfs.retain(|n| n != name);
                            favs.starred_intervals.retain(|n| n != name);
                        }
                    }
                }
                match &target {
                    mlc_core::chart_type::GranularityTarget::Tf { name } => {
                        if Timeframe::parse(name).is_none() {
                            eprintln!("[ChartApp] CommitGranularityCustom: invalid TF name, ignored: {}", name);
                            return;
                        }
                        let mut user_mut = self.core.user_mut();
                        let favs = &mut user_mut.profile_mut().domain.granularity_favorites;
                        if !favs.time_tfs.iter().any(|n| n == name) {
                            if favs.time_tfs.len() >= 12 {
                                favs.time_tfs.remove(0);
                            }
                            favs.time_tfs.push(name.clone());
                        }
                        if was_starred && !favs.starred_tfs.iter().any(|n| n == name) {
                            favs.starred_tfs.push(name.clone());
                        }
                        drop(user_mut);
                        self.autosave_snapshot();
                        self.process_chart_out_event(ChartOutEvent::ChangeTimeframe { timeframe_id: format!("tf_{}", name) });
                        return;
                    }
                    mlc_core::chart_type::GranularityTarget::Param { chart_type, value } => {
                        if *value <= 0.0 {
                            eprintln!("[ChartApp] CommitGranularityCustom: non-positive value, ignored: {}", value);
                            return;
                        }
                        let mut user_mut = self.core.user_mut();
                        let favs = &mut user_mut.profile_mut().domain.granularity_favorites;
                        let already = favs.params.iter()
                            .any(|f| &f.chart_type == chart_type && f.value == *value);
                        if !already {
                            let cap_hit = favs.params.iter().filter(|f| &f.chart_type == chart_type).count() >= 12;
                            if cap_hit {
                                if let Some(idx) = favs.params.iter().position(|f| &f.chart_type == chart_type) {
                                    favs.params.remove(idx);
                                }
                            }
                            favs.params.push(mlc_core::user_profile::ParamFavorite {
                                chart_type: chart_type.clone(), value: *value,
                            });
                        }
                        if was_starred && !favs.starred_params.iter()
                            .any(|f| &f.chart_type == chart_type && f.value == *value)
                        {
                            favs.starred_params.push(mlc_core::user_profile::ParamFavorite {
                                chart_type: chart_type.clone(), value: *value,
                            });
                        }
                        drop(user_mut);
                        self.autosave_snapshot();
                        self.process_chart_out_event(ChartOutEvent::UpdateChartTypeParam { value: *value });
                        return;
                    }
                    mlc_core::chart_type::GranularityTarget::Interval { name } => {
                        // Same validate-first contract as the `Tf` arm —
                        // `footprint_interval_static` runs `Timeframe::parse`
                        // + the 1s..=1h clamp before ever touching the
                        // profile, so an invalid/out-of-range custom bucket
                        // never gets persisted.
                        let Some(static_iv) = Self::footprint_interval_static(name) else {
                            eprintln!("[ChartApp] CommitGranularityCustom: invalid/out-of-range interval, ignored: {}", name);
                            return;
                        };
                        let mut user_mut = self.core.user_mut();
                        let favs = &mut user_mut.profile_mut().domain.granularity_favorites;
                        if !favs.interval_tfs.iter().any(|n| n == static_iv) {
                            if favs.interval_tfs.len() >= 12 {
                                favs.interval_tfs.remove(0);
                            }
                            favs.interval_tfs.push(static_iv.to_string());
                        }
                        if was_starred && !favs.starred_intervals.iter().any(|n| n == static_iv) {
                            favs.starred_intervals.push(static_iv.to_string());
                        }
                        drop(user_mut);
                        self.autosave_snapshot();
                        let iv_ms = mlc_core::state::Timeframe::parse(static_iv)
                            .map(|tf| tf.interval_ms)
                            .unwrap_or(60_000);
                        self.apply_footprint_interval(static_iv, iv_ms);
                        self.sidebar_data_dirty = true;
                        return;
                    }
                }
            }
            ChartOutEvent::DeleteGranularityCustom { target } => {
                eprintln!("[ChartApp] DeleteGranularityCustom: {:?}", target);
                let mut user_mut = self.core.user_mut();
                let favs = &mut user_mut.profile_mut().domain.granularity_favorites;
                match &target {
                    mlc_core::chart_type::GranularityTarget::Tf { name } => {
                        favs.time_tfs.retain(|n| n != name);
                        favs.starred_tfs.retain(|n| n != name);
                    }
                    mlc_core::chart_type::GranularityTarget::Param { chart_type, value } => {
                        favs.params.retain(|f| !(&f.chart_type == chart_type && f.value == *value));
                        favs.starred_params.retain(|f| !(&f.chart_type == chart_type && f.value == *value));
                    }
                    mlc_core::chart_type::GranularityTarget::Interval { name } => {
                        favs.interval_tfs.retain(|n| n != name);
                        favs.starred_intervals.retain(|n| n != name);
                    }
                }
                drop(user_mut);
                self.autosave_snapshot();
                return;
            }
            ChartOutEvent::ToggleGranularityStar { target } => {
                eprintln!("[ChartApp] ToggleGranularityStar: {:?}", target);
                let mut user_mut = self.core.user_mut();
                let favs = &mut user_mut.profile_mut().domain.granularity_favorites;
                match &target {
                    mlc_core::chart_type::GranularityTarget::Tf { name } => {
                        if favs.starred_tfs.iter().any(|n| n == name) {
                            favs.starred_tfs.retain(|n| n != name);
                        } else {
                            favs.starred_tfs.push(name.clone());
                        }
                    }
                    mlc_core::chart_type::GranularityTarget::Param { chart_type, value } => {
                        let starred = favs.starred_params.iter()
                            .any(|f| &f.chart_type == chart_type && f.value == *value);
                        if starred {
                            favs.starred_params.retain(|f| !(&f.chart_type == chart_type && f.value == *value));
                        } else {
                            favs.starred_params.push(mlc_core::user_profile::ParamFavorite {
                                chart_type: chart_type.clone(), value: *value,
                            });
                        }
                    }
                    mlc_core::chart_type::GranularityTarget::Interval { name } => {
                        if favs.starred_intervals.iter().any(|n| n == name) {
                            favs.starred_intervals.retain(|n| n != name);
                        } else {
                            favs.starred_intervals.push(name.clone());
                        }
                    }
                }
                drop(user_mut);
                self.autosave_snapshot();
                return;
            }
            ChartOutEvent::ToggleGranularityPin => {
                let mut user_mut = self.core.user_mut();
                let ui = &mut user_mut.profile_mut().ui;
                ui.pin_granularity_favorites = !ui.pin_granularity_favorites;
                eprintln!("[ChartApp] ToggleGranularityPin: {}", ui.pin_granularity_favorites);
                drop(user_mut);
                self.autosave_snapshot();
                return;
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
                // Teardown, deliverable 2 (2026-07-13 live-bug fix): only
                // drop the OLD instrument's trade stream once NO remaining
                // bubble still needs it — `unsubscribe_trades` decrements a
                // real per-key forwarder now (see `DataBridge::unsubscribe_
                // trades`'s doc comment), so calling it while a sibling
                // window/leaf still displays the old instrument as a
                // trade-fold chart type or carries a CVD companion would
                // kill their live feed too. `bubbles_for_instrument` no
                // longer contains `bid` itself at this point (its instrument
                // was already reassigned above by `set_bid_instrument_full`),
                // so any survivor found here is a genuine sibling. Gated on
                // the FULL instrument key (not just symbol) — an
                // exchange-only or account-type-only switch also leaves the
                // old (exchange, symbol, account_type) triplet behind.
                let old_instrument_key = (old_exch_search.as_str(), old_at_search.as_str(), previous_symbol.as_str());
                let new_instrument_key = (resolved_exchange.as_str(), account_type.as_str(), symbol.as_str());
                if !previous_symbol.is_empty() && old_instrument_key != new_instrument_key {
                    let old_instrument_still_needs_trades = self.core.bubbles()
                        .bubbles_for_instrument(old_exch_search.as_str(), &old_at_search, &previous_symbol)
                        .into_iter()
                        .any(|other_bid| {
                            let other_ct = self.core.bubbles().chart_type(other_bid).unwrap_or("candles");
                            if mlc_backend::chart_type::is_trade_fold_chart_type(other_ct) {
                                return true;
                            }
                            self.core.indicators().instances_for_bubble(other_bid)
                                .into_iter()
                                .any(|inst| inst.type_id == "cvd")
                        });
                    if !old_instrument_still_needs_trades {
                        self.core.unsubscribe_trades(old_trade_exchange, &previous_symbol, old_trade_at);
                    }
                }
                let eid_str = resolved_exchange.as_str();
                if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                    eprintln!("[ChartApp] ApplySymbol: exchange {} disabled, skipping connector call", eid_str);
                } else {
                    let at_typed: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
                    let at = mlc_backend_data_bridge::account_type_to_dig3(at_typed);
                    self.core.ensure_connector(resolved_exchange);
                    self.core.request_kline(
                        resolved_exchange, &symbol, &timeframe, at, None,
                        Some(self.core.user().profile().domain.bar_count as usize), false,
                    );
                    // Re-ignite, deliverable 1 (2026-07-13 live-bug fix):
                    // `set_bid_instrument_full` above rebinds the bubble's
                    // (symbol, exchange, account_type) key, but nothing
                    // re-fires the trade-fed data plane for the NEW
                    // instrument — the bubble's `active_trade_fold`/
                    // `active_cvd_fold` runtimes stay installed for the OLD
                    // one, and no fresh `subscribe_trades`/backfill was ever
                    // requested for the new key. Boot (`ignite_derived_
                    // windows`) and `ChangeChartType`'s Step 6 both already
                    // run this ignition; `ApplySymbol` never did, which is
                    // why a full reload (which always goes through
                    // hydration's `ignite_derived_windows`) "fixed" the
                    // empty panel but a live symbol switch did not. Gated on
                    // the registry's `is_trade_fold_chart_type` (chart-type
                    // MachineKind contract) — never a chart_type string
                    // match. `clear_derived_slots`/`ops.clear_slot` inside
                    // `ignite_derived_window_target` already drop the OLD
                    // fold before installing the new one, so no stale
                    // runtime survives the switch.
                    if let Some(ct) = self.chart_type_for_chart(target_cid) {
                        if mlc_backend::chart_type::is_trade_fold_chart_type(ct) {
                            let iv = self.chart_type_interval_for_chart(target_cid).unwrap_or("1m");
                            let param = self.chart_type_param_for_chart(target_cid).unwrap_or(0.0);
                            let group_ticks = self.panel_app.panel_grid.containers().get(&target_cid)
                                .and_then(|w| self.panels_store.chart_panel.get(&w.chart_panel_id))
                                .map(|p| p.footprint_display.group_ticks)
                                .unwrap_or(0);
                            self.ignite_derived_window_target(
                                target_cid, symbol.clone(), resolved_exchange.as_str().to_string(),
                                at_typed, ct, iv, param, timeframe.clone(), group_ticks,
                            );
                        }
                    }
                    // CVD companion re-ignite — mirrors `subscribe_cvd_line_
                    // for_indicator_add`'s own exemption (footprint already
                    // carries real per-bar buy/sell, a CVD fold there would
                    // be redundant) and its `chart_tf_ms` resolution.
                    let has_cvd_instance = self.bubble_for_chart(target_cid)
                        .map(|bid| self.core.indicators().instances_for_bubble(bid)
                            .into_iter().any(|inst| inst.type_id == "cvd"))
                        .unwrap_or(false);
                    if has_cvd_instance && self.chart_type_for_chart(target_cid) != Some("footprint") {
                        if let Some(bid) = self.bubble_for_chart(target_cid) {
                            let chart_tf_ms = Some(timeframe.clone())
                                .filter(|tf| !tf.is_tick())
                                .map(|tf| tf.interval_ms);
                            if let Some(ev) = self.core.subscribe_cvd_fold(bid, resolved_exchange, &symbol, at, chart_tf_ms) {
                                self.react_to_bubble_event(&ev, now_ms());
                            }
                        }
                    }
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
            ChartOutEvent::ToggleTpoProfileOverlay => {
                let new_val = if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                    w.tpo_profile_show = !w.tpo_profile_show;
                    if !w.tpo_profile_show {
                        w.tpo_profile_overlay = None;
                        w.tpo_profile_overlay_stamp = None;
                    }
                    Some(w.tpo_profile_show)
                } else { None };
                eprintln!("[ChartApp] tpo_profile_show toggled -> {:?}", new_val);
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
                            // Per-container venue (2026-07-15 wrong-venue bug).
                            let chart_eid = self.exchange_id_for_chart(cid);
                            let eid_str = chart_eid.as_str();
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
                                    self.core.subscribe_open_interest(
                                        chart_eid, &symbol, at,
                                    );
                                } else {
                                    self.core.unsubscribe_open_interest(
                                        chart_eid, &symbol, at,
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
                            // Per-container venue (2026-07-15 wrong-venue bug).
                            let chart_eid = self.exchange_id_for_chart(cid);
                            let eid_str = chart_eid.as_str();
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
                                    self.core.subscribe_funding_rate(
                                        chart_eid, &symbol, at,
                                    );
                                } else {
                                    self.core.unsubscribe_funding_rate(
                                        chart_eid, &symbol, at,
                                    );
                                }
                            }
                        }
                    }
                }
                state_mutated = true;
            }
            // === DOM Heatmap overlay (C12; chart-type → overlay migration) ===
            // Mirrors ToggleOpenInterestOverlay/ToggleFundingRateStrip above —
            // same per-window bool flag + subscribe/unsubscribe pair, but the
            // ring-buffer state (`bubble.dom_heatmap`/`dom_trade_dots`) and
            // subscribe/clear-slot fns are the SAME ones `dom_heatmap` used
            // as a chart_type; only the trigger moved.
            ChartOutEvent::ToggleDomHeatmapOverlay => {
                let cid_opt = self.panel_app.panel_grid.active_container().map(|w| w.id);
                if let Some(cid) = cid_opt {
                    let symbol_opt = self.symbol_for_chart(cid);
                    let at_opt: Option<mlc_core::instrument::AccountType> = self.account_type_for_chart(cid);
                    if let (Some(symbol), Some(at)) = (symbol_opt, at_opt) {
                        if !symbol.is_empty() {
                            // Per-container venue (2026-07-15 wrong-venue bug).
                            let chart_eid = self.exchange_id_for_chart(cid);
                            let eid_str = chart_eid.as_str();
                            let connector_enabled = self.sidebar_state.connector_enabled
                                .get(eid_str).copied().unwrap_or(true);
                            if connector_enabled {
                                let at = mlc_backend_data_bridge::account_type_to_dig3(at);
                                let dom_show_now = {
                                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                                        w.dom_heatmap_show = !w.dom_heatmap_show;
                                        eprintln!("[ChartApp] dom_heatmap_show toggled -> {}", w.dom_heatmap_show);
                                        w.dom_heatmap_show
                                    } else { false }
                                };
                                if let Some(bid) = self.bubble_for_chart(cid) {
                                    self.core.bubbles_mut().with_bubble_mut(bid, |bubble| {
                                        mlc_backend::chart_type::clear_slot_dom_heatmap(bubble);
                                    });
                                }
                                if dom_show_now {
                                    self.core.subscribe_dom_heatmap(
                                        chart_eid, &symbol, at,
                                    );
                                } else {
                                    self.core.unsubscribe_dom_heatmap(
                                        chart_eid, &symbol, at,
                                    );
                                }
                            }
                        }
                    }
                }
                state_mutated = true;
            }
            // === Liquidation Heatmap overlay (C13; chart-type → overlay migration) ===
            // Mirrors ToggleDomHeatmapOverlay above.
            ChartOutEvent::ToggleLiquidationHeatmapOverlay => {
                let cid_opt = self.panel_app.panel_grid.active_container().map(|w| w.id);
                if let Some(cid) = cid_opt {
                    let symbol_opt = self.symbol_for_chart(cid);
                    let at_opt: Option<mlc_core::instrument::AccountType> = self.account_type_for_chart(cid);
                    if let (Some(symbol), Some(at)) = (symbol_opt, at_opt) {
                        if !symbol.is_empty() {
                            // Per-container venue (2026-07-15 wrong-venue bug).
                            let chart_eid = self.exchange_id_for_chart(cid);
                            let eid_str = chart_eid.as_str();
                            let connector_enabled = self.sidebar_state.connector_enabled
                                .get(eid_str).copied().unwrap_or(true);
                            if connector_enabled {
                                let at = mlc_backend_data_bridge::account_type_to_dig3(at);
                                let liq_show_now = {
                                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                                        w.liq_heatmap_show = !w.liq_heatmap_show;
                                        eprintln!("[ChartApp] liq_heatmap_show toggled -> {}", w.liq_heatmap_show);
                                        w.liq_heatmap_show
                                    } else { false }
                                };
                                if let Some(bid) = self.bubble_for_chart(cid) {
                                    self.core.bubbles_mut().with_bubble_mut(bid, |bubble| {
                                        mlc_backend::chart_type::clear_slot_liq_heatmap(bubble);
                                    });
                                }
                                if liq_show_now {
                                    self.core.subscribe_liquidation_heatmap(
                                        chart_eid, &symbol, at,
                                    );
                                } else {
                                    self.core.unsubscribe_liquidation_heatmap(
                                        chart_eid, &symbol, at,
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
                let was_open = self.sidebar_state.right_panel == panel;
                let _ = self.sidebar_state.toggle_right_panel(panel);
                // Auto-seed: first open of an EMPTY typed slot spawns one
                // default panel of that slot's kind via the same spawn path
                // the [+] dropdown uses (owner redesign 2026-07-11).
                if !was_open {
                    let idx_usize = idx as usize;
                    let is_empty = self.sidebar_state.slot_dockings
                        .get(idx_usize)
                        .map(|mgr| mgr.inner().tree().leaves().is_empty())
                        .unwrap_or(false);
                    if is_empty {
                        if let Some(slot_kind) = mlc_app_sidebar_content::free_slot::SlotKind::from_index(idx_usize) {
                            self.spawn_panel_in_slot(idx_usize, slot_kind.default_panel_kind());
                        }
                    }
                }
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
            ChartOutEvent::ExpandPanel => {
                // Bottom-toolbar "expand" button ("expand" / "expand_chart" item
                // ids in toolbar_state.rs) — previously dead-wired (fell through
                // to the `ref other =>` logger, no-op). Same fullscreen-the-
                // active-chart-panel semantics as `InternalToggleExpand`: hide
                // every sibling leaf in the split, keeping only the active one
                // visible; toggling again restores the split.
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
                        let kline_alive = self.core.bridge_handle().is_kline_alive(eid, sym, at, &tf.name);
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

    /// Canonical `&'static str` form of a footprint bucket-interval label,
    /// covering every value reachable from any interval-change entry point:
    /// the toolbar's free-text field (minute-and-up), the TF dropdown
    /// (whitelist-gated by `tf_whitelist`, includes 5s/15s/30s), and now a
    /// user-typed CUSTOM bucket interval (per-family scoping arc
    /// 2026-07-16 — `GranularityFavorites::interval_tfs`/
    /// `starred_intervals`). `set_chart_type_interval`/`request_footprint`
    /// key off this stable string, not the caller's owned
    /// `String`/`Timeframe::name`.
    ///
    /// The 10 literal arms stay first (fast path, no parse/lock — every
    /// builtin/whitelisted interval hits this). Anything else is validated
    /// via `Timeframe::parse` (rejects garbage) and clamped to 1s..=1h
    /// (footprint buckets beyond 1h are senseless — a hand-typed hour-plus
    /// bucket would produce single-digit clusters per bar, defeating the
    /// point of the footprint view), then resolved to a `'static` string
    /// via `mlc_core::chart_type::intern_interval` — the leak-intern
    /// registry that makes an arbitrary user-typed value usable where the
    /// rest of the bubble/fold pipeline expects `&'static str`.
    fn footprint_interval_static(interval: &str) -> Option<&'static str> {
        match interval {
            "5s"  => Some("5s"),
            "15s" => Some("15s"),
            "30s" => Some("30s"),
            "1m"  => Some("1m"),
            "3m"  => Some("3m"),
            "5m"  => Some("5m"),
            "15m" => Some("15m"),
            "30m" => Some("30m"),
            "1h"  => Some("1h"),
            "4h"  => Some("4h"),
            other => {
                let tf = mlc_core::state::Timeframe::parse(other)?;
                if tf.interval_ms < 1_000 || tf.interval_ms > 3_600_000 {
                    return None;
                }
                Some(mlc_core::chart_type::intern_interval(&tf.name))
            }
        }
    }

    /// Apply a new footprint bucket interval to the active window — re-stamps
    /// `bubble.timeframe` to `(interval, interval_ms)` and re-subscribes the
    /// footprint stream at the new bucket size.
    ///
    /// Shared by both footprint interval-change entry points: the toolbar
    /// inline text field (`UpdateChartTypeInterval`, minute-and-up only) and
    /// the TF dropdown (`ChangeTimeframe`, whitelist-gated by
    /// `tf_whitelist` — includes 5s/15s/30s). The caller is responsible for
    /// validating `interval`/`interval_ms` before calling (via
    /// `footprint_interval_static`); this method does not re-derive
    /// `interval_ms` from a hardcoded table, so any interval string the
    /// caller already resolved a duration for (including sub-minute
    /// buckets) survives unclamped into the dig3 subscribe call —
    /// `interval_to_ms` on the station side already parses "5s"-style
    /// strings.
    fn apply_footprint_interval(&mut self, interval: &'static str, interval_ms: i64) {
        let active_cid = self.panel_app.panel_grid.active_container_id();
        if let Some(bid) = active_cid.and_then(|cid| self.bubble_for_chart(cid)) {
            self.core.bubbles_mut().set_chart_type_interval(bid, interval);
        }
        let Some(active_leaf) = self.panel_app.panel_grid.active_container() else { return };
        let cid = active_leaf.id;
        let symbol_opt = self.symbol_for_chart(cid);
        let at_opt: Option<mlc_core::instrument::AccountType> = self.account_type_for_chart(cid);
        let (Some(symbol), Some(at)) = (symbol_opt, at_opt) else { return };
        if symbol.is_empty() {
            return;
        }
        // Per-container venue (2026-07-15 wrong-venue bug).
        let chart_eid = self.exchange_id_for_chart(cid);
        let eid_str = chart_eid.as_str();
        if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
            return;
        }
        let at = mlc_backend_data_bridge::account_type_to_dig3(at);
        // Bucket-interval change re-subscribes on the SAME chart_type —
        // ChangeChartType's clear never fires (same class of bug as the D9
        // param-change fix). Old-interval clusters and bars are semantically
        // incompatible with the new bucket timeline: clear both slots and
        // re-stamp the bubble TF before the re-subscribe repopulates from
        // scratch.
        //
        // Hybrid (hybrid-data-modes-plan-2026-07-08, Step 3): on a
        // RecentOnly venue `bubble.bars` is the kline backdrop, not
        // footprint's own OHLC mirror — clearing it here would blank the
        // backdrop for no reason (the kline sub keeps running under a
        // bucket-interval change same as any other TF change) and flipping
        // `bars_source` away from `Kline` would make the FootprintUpdate
        // mirror-write guard start writing into it, creating a second
        // writer. Only `footprint_bars` needs a reset (old-interval clusters
        // are foreign to the new bucket grid); the kline backdrop is
        // re-requested fresh at the new interval below so it stays aligned
        // to the bucket grid.
        let fp_hybrid_backdrop = mlc_backend::chart_type::is_footprint_hybrid(
            self.core.bridge_handle(), chart_eid, at,
        );
        // Old backdrop TF name — in hybrid mode the kline backdrop always
        // rides the PREVIOUS bucket interval (Step 3 invariant: backdrop TF
        // == footprint interval), so this is the exact key `abort_kline_bars`
        // needs. Captured before `set_timeframe` overwrites the bubble's
        // label.
        let old_tf_name = self.timeframe_for_chart(cid).map(|tf| tf.name.clone());
        if let Some(bid) = self.bubble_for_chart(cid) {
            self.core.bubbles_mut().with_bubble_mut(bid, |bubble| {
                bubble.clear_footprint();
                mlc_backend::chart_type::clear_slot_footprint_hybrid_aware(bubble, fp_hybrid_backdrop);
            });
            self.core.bubbles_mut().set_timeframe(
                bid, mlc_core::state::Timeframe::new(interval, interval_ms),
            );
        }
        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
            window.begin_fresh_load();
            window.needs_snap_after_backfill = true;
        }
        if fp_hybrid_backdrop {
            if let Some(old_tf_name) = old_tf_name {
                self.core.bridge_handle().abort_kline_bars(
                    chart_eid, &symbol, at, &old_tf_name,
                );
            }
            let bar_count = self.core.user().profile().domain.bar_count as usize;
            let backdrop_tf = mlc_core::state::Timeframe::new(interval, interval_ms);
            self.core.bridge_handle().request_bars(
                chart_eid, &symbol, &backdrop_tf, at,
                None, Some(bar_count), true,
            );
        }
        // Unified-data-plane Э4: footprint's bucket-interval change is a
        // new fold machine (`TradeToFootprintDerived::new(new_interval_ms)`)
        // + full refold over the SAME trade layer, zero network for the
        // clusters themselves — mirrors `ChangeChartType`'s trade-fold
        // branch. The hybrid kline backdrop re-request above is unchanged
        // (still real network, still gated by `fp_hybrid_backdrop`).
        if let Some(bid) = self.bubble_for_chart(cid) {
            let derived_target = self.core.user().profile().domain.data_load.derived_trade_target;
            let fold_params = mlc_backend::bubble::FoldParams { interval_ms, ..Default::default() };
            if let Some(ev) = self.core.subscribe_trade_fold_chart_type(
                bid, "footprint", fold_params, mlc_backend::chart_type::DEFAULT_TPO_SOURCE,
                chart_eid, &symbol, at, Some(derived_target),
            ) {
                self.react_to_bubble_event(&ev, now_ms());
            }
        }
    }

    /// Apply a new footprint row-grouping preset (canon-fix wave 2, item 5)
    /// to the active window — converts `ticks` to a price step via the
    /// instrument's tick size (mirrors renko/pnf's ticks→price conversion at
    /// `build_subscribe_params`), then re-folds the SAME trade layer through
    /// a fresh `TradeToFootprintDerived(interval_ms, group_step)` machine,
    /// zero network — narrower sibling of [`Self::apply_footprint_interval`]:
    /// grouping is a row-binning change, not a time-bucket-boundary change,
    /// so the bucket `interval_ms`/`bubble.timeframe` and the hybrid kline
    /// backdrop are left untouched; only `footprint_bars` is cleared and
    /// refolded.
    fn apply_footprint_group_ticks(&mut self, ticks: u32) {
        let Some(active_leaf) = self.panel_app.panel_grid.active_container() else { return };
        let cid = active_leaf.id;
        let symbol_opt = self.symbol_for_chart(cid);
        let at_opt: Option<mlc_core::instrument::AccountType> = self.account_type_for_chart(cid);
        let (Some(symbol), Some(at)) = (symbol_opt, at_opt) else { return };
        if symbol.is_empty() {
            return;
        }
        // Per-container venue (2026-07-15 wrong-venue bug).
        let chart_eid = self.exchange_id_for_chart(cid);
        let eid_str = chart_eid.as_str();
        if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
            return;
        }
        let at = mlc_backend_data_bridge::account_type_to_dig3(at);
        let bid = self.bubble_for_chart(cid);
        let interval_ms = self.timeframe_for_chart(cid).map(|tf| tf.interval_ms).unwrap_or(60_000);
        let tick_size = self.resolve_tick_size(eid_str, &symbol, bid);
        let group_step = if ticks > 0 && tick_size > 0.0 { ticks as f64 * tick_size } else { 0.0 };
        if let Some(bid) = bid {
            self.core.bubbles_mut().with_bubble_mut(bid, |bubble| {
                bubble.clear_footprint();
            });
        }
        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
            window.begin_fresh_load();
            window.needs_snap_after_backfill = true;
        }
        if let Some(bid) = bid {
            let derived_target = self.core.user().profile().domain.data_load.derived_trade_target;
            let fold_params = mlc_backend::bubble::FoldParams { interval_ms, group_step, ..Default::default() };
            if let Some(ev) = self.core.subscribe_trade_fold_chart_type(
                bid, "footprint", fold_params, mlc_backend::chart_type::DEFAULT_TPO_SOURCE,
                chart_eid, &symbol, at, Some(derived_target),
            ) {
                self.react_to_bubble_event(&ev, now_ms());
            }
        }
    }

    /// Ignite the derived-pipeline for every restored window whose chart_type
    /// is derived (footprint / renko / pnf / kagi / three_line_break / tpo /
    /// range_bar / volume_bar / tick_bar / dollar_bar / *_imbalance_bar /
    /// run_bar). DOM Heatmap / Liquidation Heatmap are NOT chart_types
    /// anymore (chart-type → overlay migration) — their restore-time
    /// re-ignition is handled by the hydration path's
    /// `dom_heatmap_show`/`liq_heatmap_show` mapping instead (see
    /// `persistence/hydration_impl.rs`), not this fn.
    ///
    /// The startup / new-window post-load loops only fire kline `request_bars`;
    /// a hydrated derived window never went through `ChangeChartType`'s Step 6,
    /// so nothing subscribed its derived stream — the pane sat on the spinner
    /// forever while the kline seed painted a foreign timeline (cold-open hang,
    /// handoff 2026-07-05 §2.3 for footprint; §3.4 handoff 2026-07-06 widens
    /// this to the whole derived family).
    ///
    /// Mirrors `ChangeChartType`'s Step 6 dispatch — `bars_source` stamp,
    /// `clear_derived_slots(None)` on the target bubble, then the matching
    /// bridge `request_*`. `chart_param`/`chart_type_interval` defaults are
    /// the same as the runtime switch so a preset lands on the SAME live
    /// pipeline as if the user had just clicked the picker.
    pub(crate) fn ignite_derived_windows(&mut self) {
        let targets: Vec<(mlc_core::state::ChartId, String, String, mlc_core::instrument::AccountType, &'static str, &'static str, f64, mlc_core::state::Timeframe, u32)> =
            self.panel_app.panel_grid.containers().iter()
                .filter_map(|(cid, w)| {
                    let ct = self.chart_type_for_chart(*cid)?;
                    let is_derived = crate::active_series::chart_type_is_derived_seed(ct);
                    if !is_derived { return None; }
                    let sym = self.symbol_for_chart(*cid)?;
                    if sym.is_empty() { return None; }
                    let exch = self.exchange_for_chart(*cid)?;
                    let at = self.account_type_for_chart(*cid)?;
                    let iv = self.chart_type_interval_for_chart(*cid).unwrap_or("1m");
                    let param = self.chart_type_param_for_chart(*cid).unwrap_or(0.0);
                    let tf = self.timeframe_for_chart(*cid).unwrap_or_default();
                    // Footprint row-grouping preset (canon-fix wave 2, item
                    // 5) — persisted on the panel, not the bubble, so it
                    // survives a cold restore the same way `bar_family_wicks`
                    // does; read here so the cold-boot fold matches whatever
                    // the user last set in the settings modal.
                    let group_ticks = self.panels_store.chart_panel.get(&w.chart_panel_id)
                        .map(|p| p.footprint_display.group_ticks)
                        .unwrap_or(0);
                    Some((*cid, sym.to_string(), exch.to_string(), at, ct, iv, param, tf, group_ticks))
                })
                .collect();
        for (cid, sym, exch, at, ct, iv, param, tf, group_ticks) in targets {
            self.ignite_derived_window_target(cid, sym, exch, at, ct, iv, param, tf, group_ticks);
        }
    }

    /// Ignite (or re-ignite) the derived/trade-fold/fold pipeline for ONE
    /// window — the per-target body factored out of [`Self::ignite_derived_windows`]'s
    /// loop (§1 of the ChangeSymbol re-ignite fix, 2026-07-13) so the SAME
    /// logic drives both the cold-restore sweep (all windows) and a single
    /// window's instrument switch (`ApplySymbol`). See that fn's doc comment
    /// for the "why" of every step — this is a pure extraction, behavior
    /// unchanged for the cold-restore caller.
    #[allow(clippy::too_many_arguments)]
    fn ignite_derived_window_target(
        &mut self,
        cid: mlc_core::state::ChartId,
        sym: String,
        exch: String,
        at: mlc_core::instrument::AccountType,
        ct: &'static str,
        iv: &'static str,
        param: f64,
        tf: mlc_core::state::Timeframe,
        group_ticks: u32,
    ) {
        let eid = digdigdig3::ExchangeId::from_str(&exch)
            .unwrap_or(digdigdig3::ExchangeId::Binance);
        if !self.sidebar_state.connector_enabled.get(eid.as_str()).copied().unwrap_or(true) {
            return;
        }
        let at = mlc_backend_data_bridge::account_type_to_dig3(at);
        let bid = self.bubble_for_chart(cid);
        // Instrument-relative default resolution (granularity arc, кусок 2):
        // captured BEFORE `ops.clear_slot`/`clear_derived_slots` below wipe
        // `bubble.bars` (`clear_slot_trade_agg_bars` clears the very array
        // this reads) — a cold-restore ignite is the ONE call site where the
        // clear and the read would otherwise race in the wrong order.
        let mut param = param;
        if param <= 0.0 {
            if let Some(def) = mlc_core::chart_type::by_id(ct) {
                let last_price = self.resolve_last_price(eid.as_str(), &sym, at.short_label(), bid);
                let tick_size_for_resolve = self.resolve_tick_size(eid.as_str(), &sym, bid);
                if let Some(resolved) = mlc_core::chart_type::resolve_instrument_relative_default(
                    def, tick_size_for_resolve, last_price.unwrap_or(0.0),
                ) {
                    param = resolved;
                    if let Some(bid) = bid {
                        self.core.bubbles_mut().set_chart_type_param(bid, resolved);
                    }
                }
            }
        }
        // ATR(14) recompute-on-cold-restore (granularity arc, кусок 2): the
        // ONE non-explicit-pick recompute trigger this arc allows (see
        // `atr14_box_ticks`'s doc comment) — a persisted-Atr14 preset
        // restoring INTO renko/kagi needs a fresh box size for the CURRENT
        // instrument, not whatever raw number was last saved. Read the mode
        // BEFORE the clear below (mode isn't touched by any clear, but
        // reading early keeps this block adjacent to its sibling above).
        // `atr14_box_ticks` reads the bubble's "1m" kline layer, which is
        // typically still cold at this exact point in a fresh restore — a
        // `None` here is expected and falls back to the persisted `param`
        // untouched (graceful, matches the handler's own None contract).
        if matches!(ct, "renko" | "kagi") {
            let mode = bid.and_then(|bid| self.core.bubbles().chart_type_param_mode(bid));
            if mode == Some(mlc_core::chart_type::ParamMode::Atr14) {
                if let Some(bid) = bid {
                    let tick_size_for_atr = self.resolve_tick_size(eid.as_str(), &sym, Some(bid));
                    if let Some(ticks) = self.core.atr14_box_ticks(bid, tick_size_for_atr) {
                        param = ticks;
                        self.core.bubbles_mut().set_chart_type_param(bid, ticks);
                    }
                }
            }
        }
        if let Some(bid) = bid {
            self.core.bubbles_mut().with_bubble_mut(bid, |bubble| {
                bubble.clear_derived_slots(None);
                if let Some(ops) = mlc_backend::chart_type::ops_by_id(ct) {
                    (ops.clear_slot)(bubble);
                }
            });
        }
        self.core.ensure_connector(eid);
        let bar_count = self.core.user().profile().domain.bar_count as usize;
        let tick_size = self.resolve_tick_size(eid.as_str(), &sym, bid);
        let params = build_subscribe_params(ct, param, iv, bar_count, tick_size);
        // Unified-data-plane Э3: restored windows on one of the 7 fold
        // types install/reseed a `FoldRuntime` instead of an
        // `ops.subscribe` station derived sub — mirrors the
        // `ChangeChartType`/`UpdateChartTypeParam` cutover above.
        if mlc_backend::chart_type::is_fold_chart_type(ct) {
            if let (Some(bid), Some(fold_params)) = (
                bid,
                mlc_backend::chart_type::fold_params_from_subscribe_params(ct, params),
            ) {
                let derived_target = self.core.user().profile().domain.data_load.derived_kline_target;
                if let Some(ev) = self.core.subscribe_fold_chart_type(
                    bid, ct, fold_params, eid, &sym, at, Some(derived_target),
                ) {
                    self.react_to_bubble_event(&ev, now_ms());
                }
            }
        } else if mlc_backend::chart_type::is_trade_fold_chart_type(ct) {
            // Unified-data-plane Э4: restored windows on one of the 6
            // trade-fold types install/reseed a `TradeFoldRuntime`
            // instead of an `ops.subscribe` station derived sub —
            // mirrors the `ChangeChartType`/`UpdateChartTypeParam`
            // cutover above.
            if let (Some(bid), Some(mut fold_params)) = (
                bid,
                mlc_backend::chart_type::trade_fold_params_from_subscribe_params(ct, params),
            ) {
                // Footprint row-grouping preset (canon-fix wave 2, item
                // 5) — `SubscribeParams`/`build_subscribe_params` don't
                // carry a group-ticks axis (footprint's `param_spec` is
                // `None`), so the panel-persisted value is converted and
                // applied directly onto the resolved `FoldParams` here,
                // same as `apply_footprint_group_ticks`'s live-toggle path.
                if ct == "footprint" && group_ticks > 0 {
                    fold_params.group_step = group_ticks as f64 * tick_size;
                }
                let derived_target = self.core.user().profile().domain.data_load.derived_trade_target;
                if let Some(ev) = self.core.subscribe_trade_fold_chart_type(
                    bid, ct, fold_params, mlc_backend::chart_type::DEFAULT_TPO_SOURCE,
                    eid, &sym, at, Some(derived_target),
                ) {
                    self.react_to_bubble_event(&ev, now_ms());
                }
            }
        } else if let Some(subscribe) = mlc_backend::chart_type::ops_by_id(ct).and_then(|ops| ops.subscribe) {
            subscribe(self.core.bridge_handle(), eid, &sym, at, &tf, params);
        }
        eprintln!("[ChartApp] ignited derived sub for window {:?} ct={} iv={} param={}", cid, ct, iv, param);
    }

    /// Ignite the live overlay subscription for every restored window whose
    /// `dom_heatmap_show`/`liq_heatmap_show` flag came back `true` — either
    /// a genuine user preference (never persisted today, see the doctrine
    /// note on `oi_show`/`funding_show`; RAM-only view state always
    /// defaults to `false`) or the chart-type → overlay migration's
    /// hydration mapping (`persistence/hydration_impl.rs`), which flips the
    /// flag `true` for a legacy `chart_type = "dom_heatmap"`/
    /// `"liquidation_heatmap"` snapshot so the overlay comes back visually
    /// equivalent to the old chart_type. Sibling of `ignite_derived_windows`
    /// — same startup/new-window post-load timing, called right after it.
    pub(crate) fn ignite_legacy_heatmap_overlays(&mut self) {
        let dom_targets: Vec<(mlc_core::state::ChartId, String, String, mlc_core::instrument::AccountType)> =
            self.panel_app.panel_grid.containers().iter()
                .filter_map(|(cid, w)| {
                    if !w.dom_heatmap_show { return None; }
                    let sym = self.symbol_for_chart(*cid)?;
                    if sym.is_empty() { return None; }
                    let exch = self.exchange_for_chart(*cid)?;
                    let at = self.account_type_for_chart(*cid)?;
                    Some((*cid, sym.to_string(), exch.to_string(), at))
                })
                .collect();
        for (cid, sym, exch, at) in dom_targets {
            let eid = digdigdig3::ExchangeId::from_str(&exch)
                .unwrap_or(digdigdig3::ExchangeId::Binance);
            if !self.sidebar_state.connector_enabled.get(eid.as_str()).copied().unwrap_or(true) {
                continue;
            }
            let at_dig3 = mlc_backend_data_bridge::account_type_to_dig3(at);
            self.core.ensure_connector(eid);
            self.core.subscribe_dom_heatmap(eid, &sym, at_dig3);
            eprintln!("[ChartApp] ignited DOM heatmap overlay for restored window {:?}", cid);
        }

        let liq_targets: Vec<(mlc_core::state::ChartId, String, String, mlc_core::instrument::AccountType)> =
            self.panel_app.panel_grid.containers().iter()
                .filter_map(|(cid, w)| {
                    if !w.liq_heatmap_show { return None; }
                    let sym = self.symbol_for_chart(*cid)?;
                    if sym.is_empty() { return None; }
                    let exch = self.exchange_for_chart(*cid)?;
                    let at = self.account_type_for_chart(*cid)?;
                    Some((*cid, sym.to_string(), exch.to_string(), at))
                })
                .collect();
        for (cid, sym, exch, at) in liq_targets {
            let eid = digdigdig3::ExchangeId::from_str(&exch)
                .unwrap_or(digdigdig3::ExchangeId::Binance);
            if !self.sidebar_state.connector_enabled.get(eid.as_str()).copied().unwrap_or(true) {
                continue;
            }
            let at_dig3 = mlc_backend_data_bridge::account_type_to_dig3(at);
            self.core.ensure_connector(eid);
            self.core.subscribe_liquidation_heatmap(eid, &sym, at_dig3);
            eprintln!("[ChartApp] ignited Liquidation heatmap overlay for restored window {:?}", cid);
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
        // Companion indicator per chart_type, sourced from the chart-type
        // registry (`mlc_core::chart_type::ChartTypeDef::default_indicators`)
        // — the registry row IS the table now; extend it there when adding
        // more auto-pairs (tpo → VolumeProfile, ...).
        let wanted: &[&str] = mlc_core::chart_type::by_id(chart_type)
            .map(|d| d.default_indicators)
            .unwrap_or(&[]);

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

    /// Resolve the instrument tick size used to convert a TICKS-denominated
    /// `chart_type_param` (range_bar/renko/pnf/kagi — owner doctrine
    /// 2026-07-08 resolution-axes rework) into absolute price units at
    /// subscribe time.
    ///
    /// Fallback chain (each step logged via `dlog!` so a wrong pick is
    /// diagnosable):
    /// 1. Exchange symbol catalogue (`BackendCore::exchange_symbols`) —
    ///    authoritative venue-reported tick size, when the catalogue has
    ///    loaded for this instrument.
    /// 2. Recent bars on the bubble — mirrors footprint's
    ///    `infer_tick_size` precedent (`mlc-app-layout/src/chart_render/
    ///    footprint.rs`), but computes the actual minimum observed price
    ///    step rather than a display-density row target (footprint's
    ///    ladder is tuned for legible cluster rows, not real tick size).
    /// 3. Price-magnitude heuristic: `10^(floor(log10(price)) - 4)` — a
    ///    coarse guess (~4 significant digits of quantization) used only
    ///    when neither of the above has any data (e.g. cold subscribe
    ///    before the first bar ever arrived).
    pub(crate) fn resolve_tick_size(
        &self,
        exchange_str: &str,
        symbol: &str,
        bid: Option<mlc_core::ids::BubbleId>,
    ) -> f64 {
        if let Some(t) = self.core.exchange_symbols().tick_size_for(exchange_str, symbol).filter(|t| *t > 0.0) {
            crate::dlog!(
                "[ChartApp] resolve_tick_size: {}/{} = {} (source=catalogue)",
                exchange_str, symbol, t
            );
            return t;
        }
        if let Some(bid) = bid {
            let bubbles = self.core.bubbles();
            let bars = bubbles.bars(bid);
            let mut min_step = f64::INFINITY;
            let mut last_close: Option<f64> = None;
            for bar in bars.iter().rev().take(200) {
                if let Some(prev) = last_close {
                    let step = (prev - bar.close).abs();
                    if step > 0.0 && step < min_step {
                        min_step = step;
                    }
                }
                last_close = Some(bar.close);
            }
            if min_step.is_finite() && min_step > 0.0 {
                crate::dlog!(
                    "[ChartApp] resolve_tick_size: {}/{} = {} (source=recent_bars)",
                    exchange_str, symbol, min_step
                );
                return min_step;
            }
            if let Some(price) = bars.last().map(|b| b.close).filter(|p| *p > 0.0) {
                let heuristic = 10f64.powf(price.log10().floor() - 4.0);
                crate::dlog!(
                    "[ChartApp] resolve_tick_size: {}/{} = {} (source=price_magnitude_heuristic, price={})",
                    exchange_str, symbol, heuristic, price
                );
                return heuristic;
            }
        }
        crate::dlog!(
            "[ChartApp] resolve_tick_size: {}/{} = 0.01 (source=fallback_default, no catalogue/bars available)",
            exchange_str, symbol
        );
        0.01
    }

    /// Resolve the active instrument's last traded price for `bid` — the
    /// live-price input [`mlc_core::chart_type::resolve_instrument_relative_default`]
    /// needs at subscribe time (granularity arc, кусок 2). Two sources, in
    /// order:
    /// 1. The bubble's own last bar close (`bars(bid).last()`) — cheapest,
    ///    already resident, and captured here BEFORE any clear the caller
    ///    might run right after.
    /// 2. `BackendCore::mini_ticker`, keyed `"{symbol}:{exchange}:{account}"`
    ///    (same key format `MiniTickerUpdate`'s writer and the watchlist
    ///    reader already use) — covers a cold subscribe where no bar has
    ///    landed yet but a ticker tick already has.
    ///
    /// `None` when neither source has data — callers pass that straight
    /// through to the resolver, which requires a positive price and
    /// therefore returns `None` too, falling back to the registry's static
    /// default exactly as before this resolver existed.
    pub(crate) fn resolve_last_price(
        &self,
        exchange_str: &str,
        symbol: &str,
        account_type_label: &str,
        bid: Option<mlc_core::ids::BubbleId>,
    ) -> Option<f64> {
        if let Some(bid) = bid {
            if let Some(price) = self.core.bubbles().bars(bid).last().map(|b| b.close).filter(|p| *p > 0.0) {
                return Some(price);
            }
        }
        let key = format!("{}:{}:{}", symbol, exchange_str, account_type_label);
        self.core.mini_ticker.get(&key).map(|t| t.last_price).filter(|p| *p > 0.0)
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
/// Sourced from `mlc_core::chart_type::registry` — footprint clusters draw
/// per-level bid/ask numbers inside the bar column (`footprint.rs::
/// MIN_BAR_WIDTH_PX = 40.0` is the hard floor for text, `bar_width() =
/// bar_spacing * bar_width_ratio` with default ratio 0.8, so `bar_spacing`
/// must clear `40.0 / 0.8 = 50.0` for the body alone to fit legible text
/// plus headroom for the per-level number labels). P&F X/O shapes stretch to
/// the box cell (`pnf.rs`) — below ~12px of spacing the glyph is a sliver
/// and the column grid is unreadable.
pub fn min_usable_bar_spacing(chart_type: &str) -> Option<f64> {
    mlc_core::chart_type::by_id(chart_type).and_then(|d| d.min_bar_spacing)
}

/// Current wall-clock time in milliseconds — the same
/// `tick.rs`/`frame_lifecycle`-style clock source `react_to_bubble_event`
/// expects for its `now_ms` parameter (used to timestamp alert-crossing
/// checks and similar tick-relative bookkeeping the reactor runs per
/// event). SWC audit fix: the 3 direct `react_to_bubble_event(&ev, 0)` call
/// sites in this file (`ChangeChartType`/`UpdateChartTypeParam`/
/// `ignite_derived_windows`'s fold-install paths) used to pass a hardcoded
/// `0` instead of the real tick time — those call sites fire OUTSIDE the
/// normal `tick()`/`react_to_events` drain (they're synchronous side
/// effects of a user-triggered chart-type switch), so there is no
/// `current_time_ms` already in scope to thread through; this helper
/// resolves it the same way `chart_out_events.rs`'s own TF-gate check
/// already does a few hundred lines up (`allowed_timeframes`'s `now_ms`
/// local).
fn now_ms() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Resolve the code-side default for a `ResolutionKind::Param` chart_type's
/// numeric parameter — thin forward to the single canonical resolver
/// [`mlc_core::chart_type::ChartTypeDef::default_param_value`] (canon-fix
/// wave 2, item 1: this used to be an independent copy of the
/// `inline_default`-or-`options.first()` rule; now every reader — legend,
/// toolbar button label, settings-modal dropdown, and this subscribe-time
/// fallback — calls the SAME registry method, so they can never diverge
/// again (bug found 2026-07-10: tick_bar showed "1T" pre-first-pick but
/// subscribed count=100 — `TICK_BAR_OPTIONS.first() == "1"` vs the old
/// hardcoded `100` fallback here). `None` when the type has no `param_spec`
/// (not a Param chart_type, or a registry drift) — callers keep their own
/// last-resort literal for that defensive case.
fn default_chart_type_param(ct: &str) -> Option<f64> {
    mlc_core::chart_type::by_id(ct)?.default_param_value()
}

/// Map a `chart_type` id + its raw UI param/interval into the
/// `SubscribeParams` shape the [`mlc_backend::chart_type::SubscribeOps`]
/// table expects (chart-type registry arc, Step 10). One choke point for
/// the same per-type scaling formulas previously duplicated across
/// `ChangeChartType`, `UpdateChartTypeParam`, and `ignite_derived_windows`.
///
/// `tick_size` is the resolved instrument tick size (see
/// [`ChartApp::resolve_tick_size`]) — used ONLY by the price-quantized
/// price-action family (range_bar/kagi/renko/pnf). Owner doctrine
/// (2026-07-08 resolution-axes rework): `param` for those four chart types
/// is now expressed in INSTRUMENT TICKS, not absolute price. The conversion
/// `price_units = ticks * tick_size` happens here, at the last choke point
/// before the value leaves the UI layer for the engine — every other
/// chart_type's `param` is unaffected and keeps its pre-existing unit
/// (contract count / volume / dollar notional / trade count).
///
/// Every `param <= 0.0` (pre-first-pick sentinel) branch below falls back to
/// [`default_chart_type_param`] — the registry's first preset — so the
/// toolbar/modal's displayed default and the value actually subscribed can
/// never diverge (see that fn's doc comment for the bug this closes). The
/// literal after `.unwrap_or(..)` is a defensive last resort only, hit when
/// the registry entry has no `param_spec` at all (should not happen for
/// these ids — guarded by an `Э4`/`Э3` regression test).
fn build_subscribe_params(
    ct: &str,
    param: f64,
    interval: &'static str,
    bar_count: usize,
    tick_size: f64,
) -> mlc_backend::chart_type::SubscribeParams {
    use mlc_backend::chart_type::SubscribeParams;
    let default_param = || default_chart_type_param(ct);
    // Fixed non-editable constants (renko/pnf reversal, imbalance/run alpha)
    // read from the registry row (chart-type-engine-consolidation B3, S3) —
    // no longer independently hardcoded per arm below. `unwrap_or` literals
    // are the registry's own canonical defaults (mirrors `FixedConsts::NONE`
    // for any type without a fixed constant), not an independent source.
    let fixed_consts = mlc_core::chart_type::by_id(ct).map(|d| d.fixed_consts).unwrap_or(mlc_core::chart_type::FixedConsts::NONE);
    match ct {
        "range_bar" | "kagi" => {
            let ticks = if param > 0.0 { param } else { default_param().unwrap_or(10.0) };
            let units = ticks * tick_size;
            SubscribeParams::ScaledUnits { scaled_u64: (units * 1e8) as u64 }
        }
        "volume_bar" => {
            let units = if param > 0.0 { param } else { default_param().unwrap_or(1000.0) };
            SubscribeParams::ScaledUnits { scaled_u64: (units * 1e8) as u64 }
        }
        "renko" => {
            let ticks = if param > 0.0 { param } else { default_param().unwrap_or(10.0) };
            let units = ticks * tick_size;
            SubscribeParams::ScaledUnitsWithReversal { scaled_u64: (units * 1e8) as u64, reversal: fixed_consts.reversal.unwrap_or(2) }
        }
        "pnf" => {
            let ticks = if param > 0.0 { param } else { default_param().unwrap_or(10.0) };
            let units = ticks * tick_size;
            SubscribeParams::ScaledUnitsWithReversal { scaled_u64: (units * 1e8) as u64, reversal: fixed_consts.reversal.unwrap_or(3) }
        }
        "dollar_bar" => {
            let threshold = if param > 0.0 { param } else { default_param().unwrap_or(1_000_000.0) };
            SubscribeParams::Threshold { threshold_u64: threshold as u64 }
        }
        "tick_imbalance_bar" | "volume_imbalance_bar" | "run_bar" => {
            let min_ticks = if param > 0.0 { param } else { default_param().unwrap_or(50.0) };
            SubscribeParams::MinTicks {
                alpha_x100: fixed_consts.alpha_x100.unwrap_or(20),
                min_ticks: min_ticks as u32,
            }
        }
        "tick_bar" => {
            let count = if param > 0.0 { param } else { default_param().unwrap_or(100.0) };
            SubscribeParams::Count { count_u32: count as u32 }
        }
        "three_line_break" => {
            let lines_back = if param > 0.0 { param } else { default_param().unwrap_or(3.0) };
            SubscribeParams::LinesBack {
                lines_back: (lines_back as u8).max(1),
            }
        }
        "tpo" => {
            let freq = if param > 0.0 { param } else { default_param().unwrap_or(30.0) };
            SubscribeParams::Freq { freq_u16: freq as u16 }
        }
        "footprint" => {
            let iv = if !interval.is_empty() { interval } else { "1m" };
            SubscribeParams::Interval { interval: iv }
        }
        _ => SubscribeParams::Kline { bar_count },
    }
}

// =============================================================================
// Regression tests — UI param value -> SubscribeParams -> FoldParams
// =============================================================================
//
// Closes the 2026-07-10 bug: toolbar "1T" selected for tick_bar subscribed
// at count=100 instead of count=1. Covers every `ResolutionKind::Param`
// chart_type on both the trade-fold (Э4) and kline-fold (Э3) families —
// the UI-selected value must survive `build_subscribe_params` into the
// matching `SubscribeParams` variant, and from there into `FoldParams`
// unmangled. Also asserts the `param <= 0.0` sentinel default now equals
// the registry's own first preset (`default_chart_type_param`), so the
// toolbar's displayed default and the backend's subscribed default can
// never diverge again.
#[cfg(test)]
mod subscribe_params_regression_tests {
    use super::{build_subscribe_params, default_chart_type_param};
    use mlc_backend::chart_type::{
        fold_params_from_subscribe_params, trade_fold_params_from_subscribe_params,
        SubscribeParams,
    };

    const BAR_COUNT: usize = 300;
    const TICK_SIZE: f64 = 1.0;

    /// tick_bar: "1T"/"500T" (and every other registry preset) must carry
    /// through as the EXACT `FoldParams::count`, not the old hardcoded 100
    /// fallback.
    #[test]
    fn tick_bar_param_carries_selected_count() {
        for &(picked, expected_count) in &[(1.0, 1u32), (10.0, 10), (100.0, 100), (500.0, 500)] {
            let params = build_subscribe_params("tick_bar", picked, "1m", BAR_COUNT, TICK_SIZE);
            assert!(
                matches!(params, SubscribeParams::Count { count_u32 } if count_u32 == expected_count),
                "tick_bar param {picked} did not produce SubscribeParams::Count{{{expected_count}}}: {params:?}",
            );
            let fold = trade_fold_params_from_subscribe_params("tick_bar", params)
                .expect("tick_bar is a registered trade-fold chart_type");
            assert_eq!(
                fold.count, expected_count,
                "tick_bar param {picked} did not reach FoldParams::count",
            );
        }
    }

    /// tick_bar's `param <= 0.0` sentinel (pre-first-pick) must fall back to
    /// the registry's `inline_default` ("100"), the SAME value the toolbar
    /// button label shows for that sentinel — not an independently
    /// hardcoded literal, and not `TICK_BAR_OPTIONS.first()` ("1") either.
    #[test]
    fn tick_bar_sentinel_default_matches_registry_inline_default() {
        let registry_default = default_chart_type_param("tick_bar").expect("tick_bar has a param_spec");
        assert_eq!(registry_default, 100.0, "tick_bar's inline_default drifted from \"100\"");

        let params = build_subscribe_params("tick_bar", 0.0, "1m", BAR_COUNT, TICK_SIZE);
        let fold = trade_fold_params_from_subscribe_params("tick_bar", params)
            .expect("tick_bar is a registered trade-fold chart_type");
        assert_eq!(
            fold.count, registry_default as u32,
            "tick_bar sentinel default diverged from the registry's inline_default (the exact 2026-07-10 bug)",
        );
    }

    /// tick_imbalance_bar / volume_imbalance_bar / run_bar: the UI-selected
    /// `min_ticks` must carry through, not silently default to (alpha=20,
    /// min_ticks=50).
    #[test]
    fn imbalance_and_run_family_param_carries_selected_min_ticks() {
        for ct in ["tick_imbalance_bar", "volume_imbalance_bar", "run_bar"] {
            for &(picked, expected_min_ticks) in &[(10.0, 10u32), (25.0, 25), (250.0, 250)] {
                let params = build_subscribe_params(ct, picked, "1m", BAR_COUNT, TICK_SIZE);
                assert!(
                    matches!(
                        params,
                        SubscribeParams::MinTicks { alpha_x100: 20, min_ticks } if min_ticks == expected_min_ticks
                    ),
                    "{ct} param {picked} did not produce SubscribeParams::MinTicks{{20, {expected_min_ticks}}}: {params:?}",
                );
                let fold = trade_fold_params_from_subscribe_params(ct, params)
                    .unwrap_or_else(|| panic!("{ct} is a registered trade-fold chart_type"));
                assert_eq!(
                    fold.min_ticks, expected_min_ticks,
                    "{ct} param {picked} did not reach FoldParams::min_ticks",
                );
                assert_eq!(fold.alpha, 0.20, "{ct} alpha should stay the fixed 0.20 (no UI control for it)");
            }
        }
    }

    /// Imbalance/run family sentinel default (`param <= 0.0`) must match
    /// the registry's `inline_default` ("50"), NOT `IMBALANCE_RUN_OPTIONS
    /// .first() == "10"`.
    #[test]
    fn imbalance_and_run_family_sentinel_default_matches_registry_inline_default() {
        for ct in ["tick_imbalance_bar", "volume_imbalance_bar", "run_bar"] {
            let registry_default = default_chart_type_param(ct).unwrap_or_else(|| panic!("{ct} has a param_spec"));
            assert_eq!(registry_default, 50.0, "{ct}'s inline_default drifted from \"50\"");

            let params = build_subscribe_params(ct, 0.0, "1m", BAR_COUNT, TICK_SIZE);
            let fold = trade_fold_params_from_subscribe_params(ct, params)
                .unwrap_or_else(|| panic!("{ct} is a registered trade-fold chart_type"));
            assert_eq!(
                fold.min_ticks, registry_default as u32,
                "{ct} sentinel default diverged from the registry's inline_default",
            );
        }
    }

    /// footprint (control case — the probe confirmed this family already
    /// worked): interval string must carry through unmangled into
    /// `FoldParams::interval_ms`.
    #[test]
    fn footprint_interval_carries_selected_bucket() {
        for &(picked, expected_ms) in &[("1m", 60_000i64), ("5m", 300_000), ("1h", 3_600_000)] {
            let params = build_subscribe_params("footprint", 0.0, picked, BAR_COUNT, TICK_SIZE);
            assert!(
                matches!(params, SubscribeParams::Interval { interval } if interval == picked),
                "footprint interval {picked} did not produce SubscribeParams::Interval: {params:?}",
            );
            let fold = trade_fold_params_from_subscribe_params("footprint", params)
                .expect("footprint is a registered trade-fold chart_type");
            assert_eq!(fold.interval_ms, expected_ms, "footprint interval {picked} did not reach FoldParams::interval_ms");
        }
    }

    /// tpo: freq_minutes must carry the UI-selected value, not silently
    /// default to 30.
    #[test]
    fn tpo_param_carries_selected_freq() {
        for &(picked, expected_freq) in &[(5.0, 5u16), (15.0, 15), (60.0, 60)] {
            let params = build_subscribe_params("tpo", picked, "1m", BAR_COUNT, TICK_SIZE);
            let fold = trade_fold_params_from_subscribe_params("tpo", params)
                .expect("tpo is a registered trade-fold chart_type");
            assert_eq!(fold.freq_minutes, expected_freq, "tpo param {picked} did not reach FoldParams::freq_minutes");
        }
    }

    /// range_bar / kagi: ticks * tick_size must carry through into
    /// `FoldParams::scaled_units` (kline-fold Э3 family, the twin to Э4
    /// above). Uses a non-1.0 tick_size to also pin the multiplication.
    #[test]
    fn range_bar_and_kagi_param_carries_selected_ticks_scaled() {
        let tick_size = 0.5;
        for ct in ["range_bar", "kagi"] {
            for &picked_ticks in &[1.0, 10.0, 50.0] {
                let params = build_subscribe_params(ct, picked_ticks, "1m", BAR_COUNT, tick_size);
                let fold = fold_params_from_subscribe_params(ct, params)
                    .unwrap_or_else(|| panic!("{ct} is a registered kline-fold chart_type"));
                let expected_units = picked_ticks * tick_size;
                assert!(
                    (fold.scaled_units - expected_units).abs() < 1e-6,
                    "{ct} ticks {picked_ticks} * tick_size {tick_size} did not reach FoldParams::scaled_units \
                     (got {}, expected {expected_units})",
                    fold.scaled_units,
                );
            }
        }
    }

    /// renko / pnf: same ticks*tick_size carry as range_bar/kagi, plus a
    /// fixed reversal count baked at the UI seam (not user-editable today).
    #[test]
    fn renko_and_pnf_param_carries_selected_ticks_scaled() {
        let tick_size = 2.0;
        for (ct, expected_reversal) in [("renko", 2u8), ("pnf", 3u8)] {
            let picked_ticks = 20.0;
            let params = build_subscribe_params(ct, picked_ticks, "1m", BAR_COUNT, tick_size);
            let fold = fold_params_from_subscribe_params(ct, params)
                .unwrap_or_else(|| panic!("{ct} is a registered kline-fold chart_type"));
            let expected_units = picked_ticks * tick_size;
            assert!(
                (fold.scaled_units - expected_units).abs() < 1e-6,
                "{ct} ticks {picked_ticks} * tick_size {tick_size} did not reach FoldParams::scaled_units",
            );
            assert_eq!(fold.reversal, expected_reversal, "{ct} reversal did not match its fixed UI-seam constant");
        }
    }

    /// volume_bar: UI param carries straight through as `scaled_units`
    /// (no tick_size scaling — volume is contract count, not price).
    #[test]
    fn volume_bar_param_carries_selected_units() {
        for &picked in &[0.1, 5.0, 1000.0] {
            let params = build_subscribe_params("volume_bar", picked, "1m", BAR_COUNT, TICK_SIZE);
            let fold = fold_params_from_subscribe_params("volume_bar", params)
                .expect("volume_bar is a registered kline-fold chart_type");
            assert!(
                (fold.scaled_units - picked).abs() < 1e-6,
                "volume_bar param {picked} did not reach FoldParams::scaled_units (got {})",
                fold.scaled_units,
            );
        }
    }

    /// dollar_bar: UI param carries straight through as `threshold`.
    #[test]
    fn dollar_bar_param_carries_selected_threshold() {
        for &picked in &[1_000.0, 100_000.0, 10_000_000.0] {
            let params = build_subscribe_params("dollar_bar", picked, "1m", BAR_COUNT, TICK_SIZE);
            let fold = fold_params_from_subscribe_params("dollar_bar", params)
                .expect("dollar_bar is a registered kline-fold chart_type");
            assert!(
                (fold.threshold - picked).abs() < 1e-6,
                "dollar_bar param {picked} did not reach FoldParams::threshold (got {})",
                fold.threshold,
            );
        }
    }

    /// three_line_break: UI param carries through as `lines_back`.
    #[test]
    fn three_line_break_param_carries_selected_lines_back() {
        for &(picked, expected) in &[(2.0, 2u8), (4.0, 4), (6.0, 6)] {
            let params = build_subscribe_params("three_line_break", picked, "1m", BAR_COUNT, TICK_SIZE);
            let fold = fold_params_from_subscribe_params("three_line_break", params)
                .expect("three_line_break is a registered kline-fold chart_type");
            assert_eq!(fold.lines_back, expected, "three_line_break param {picked} did not reach FoldParams::lines_back");
        }
    }

    /// Every kline-fold family's sentinel default must match its registry's
    /// `inline_default` (when set) or first preset otherwise (same class of
    /// fix as the trade-fold assertions above). range_bar/kagi/three_line_break
    /// each have an `inline_default` that differs from `options.first()` —
    /// this pins that `inline_default` wins.
    #[test]
    fn kline_fold_family_sentinel_defaults_match_registry_inline_default() {
        let cases: &[(&str, f64)] = &[
            ("range_bar", 8.0),
            ("kagi", 50.0),
            ("renko", 10.0),
            ("pnf", 10.0),
            ("volume_bar", 10.0),
            ("dollar_bar", 100_000.0),
            ("three_line_break", 3.0),
        ];
        for &(ct, expected_default) in cases {
            let registry_default = default_chart_type_param(ct).unwrap_or_else(|| panic!("{ct} has a param_spec"));
            assert_eq!(
                registry_default, expected_default,
                "{ct}'s inline_default drifted",
            );
        }
    }

    /// Extract the single numeric "quantity" a fold machine's `FoldParams`
    /// carries for its own `MachineKind` — the shape-specific counterpart to
    /// [`default_chart_type_param`]'s label-side value, so the two can be
    /// compared numerically without hand-listing which `FoldParams` field
    /// belongs to which chart_type (chart-type-engine-consolidation B4,
    /// T2.3). `renko`/`pnf` compare `scaled_units` (ticks BEFORE the
    /// `tick_size` multiplication — divided back out by the caller), not
    /// `reversal` (fixed, not user-editable, already covered by
    /// `renko_and_pnf_param_carries_selected_ticks_scaled`).
    fn fold_params_numeric_quantity(machine: mlc_core::chart_type::MachineKind, fold: mlc_backend::bubble::FoldParams) -> Option<f64> {
        use mlc_core::chart_type::MachineKind;
        match machine {
            MachineKind::RangeBar | MachineKind::VolumeBar | MachineKind::Renko | MachineKind::Pnf | MachineKind::Kagi => Some(fold.scaled_units),
            MachineKind::DollarBar => Some(fold.threshold),
            MachineKind::ThreeLineBreak => Some(fold.lines_back as f64),
            MachineKind::TickBar => Some(fold.count as f64),
            MachineKind::TickImbalance | MachineKind::VolumeImbalance | MachineKind::RunBar => Some(fold.min_ticks as f64),
            MachineKind::Tpo => Some(fold.freq_minutes as f64),
            // footprint's "quantity" is the interval string, not a scalar —
            // covered separately by `footprint_interval_carries_selected_bucket`.
            MachineKind::Footprint | MachineKind::None => None,
        }
    }

    /// Label == subscribe coherence, generalized over the WHOLE registry
    /// (chart-type-engine-consolidation B4, T2.3): for every row with a
    /// `param_spec` (the toolbar/modal numeric-param surface), the default
    /// the toolbar button LABEL shows ([`default_chart_type_param`]) must
    /// numerically equal the value the sentinel (`param <= 0.0`, "no pick
    /// yet") subscribe path actually derives via `build_subscribe_params` ->
    /// `fold_params_for`/`fold_params_for` (kline-fold and trade-fold both
    /// route through the same `fold_params_for`, dispatched by `def.machine`
    /// — see `mlc-backend::chart_type::engine`). Replaces hand-listing every
    /// chart_type id with one loop over `mlc_core::chart_type::all()`; the
    /// hand-written per-family tests above stay as focused documentation of
    /// WHICH `SubscribeParams`/`FoldParams` field each family uses, this
    /// test is the drift guard that catches a FUTURE registry row silently
    /// diverging from its own default.
    ///
    /// `range_bar`/`kagi`/`renko`/`pnf` divide the multiplied result back by
    /// `tick_size` before comparing — their `param_spec` is denominated in
    /// TICKS, `build_subscribe_params` converts to absolute price units
    /// before it ever reaches `fold_params_for` (see that fn's doc comment).
    #[test]
    fn every_param_spec_row_label_default_matches_subscribe_default() {
        let tick_size = 1.0;
        for def in mlc_core::chart_type::all() {
            if def.param_spec.is_none() {
                continue;
            }
            let label_default = default_chart_type_param(def.id)
                .unwrap_or_else(|| panic!("{}: has param_spec but no parseable default", def.id));

            let params = build_subscribe_params(def.id, 0.0, "1m", BAR_COUNT, tick_size);
            let fold = mlc_backend::chart_type::fold_params_for(def, params);
            let Some(fold) = fold else {
                // footprint: param_spec is None for it (see registry doc
                // comment), so this branch is unreachable for it — every
                // OTHER param_spec row has a concrete `machine`.
                panic!("{}: has param_spec but fold_params_for returned None", def.id);
            };
            let Some(subscribed_default) = fold_params_numeric_quantity(def.machine, fold) else {
                continue; // footprint-shaped output, no single scalar to compare
            };

            let expected = if matches!(def.id, "range_bar" | "kagi" | "renko" | "pnf") {
                label_default * tick_size
            } else {
                label_default
            };
            assert!(
                (subscribed_default - expected).abs() < 1e-6,
                "{}: toolbar label default ({label_default}) disagrees with sentinel subscribe default \
                 (derived {subscribed_default}, expected {expected})", def.id,
            );
        }
    }

    /// End-to-end batched-bar param LEAK regression (owner live report,
    /// 2026-07-10): dial volume_bar to 50V, switch to dollar_bar — the
    /// toolbar/subscribe must NOT show "$50" (the leaked volume_bar param),
    /// they must land on dollar_bar's OWN registry default (100000).
    ///
    /// Exercises the real seam `ChangeChartType` relies on —
    /// `BubbleStore::set_chart_type` (the actual reset, not a hand-picked
    /// literal) followed by a `chart_type_param(bid)` read and
    /// `build_subscribe_params` — instead of calling
    /// `build_subscribe_params` in isolation like the tests above. Also
    /// covers the inverse (re-selecting the SAME type keeps the dialed
    /// value) and the toolbar label path via `default_chart_type_param`,
    /// which must agree with the subscribed value (label/subscribe
    /// coherence, same invariant as `every_param_spec_row_label_default_matches_subscribe_default`).
    #[test]
    fn chart_type_switch_does_not_leak_batched_bar_param_across_types() {
        let mut store = mlc_backend::bubble::BubbleStore::new();
        let bid = store.create_bubble("BTCUSDT".to_string(), mlc_core::state::Timeframe::m1());

        // Dial volume_bar to 50V — mirrors the user picking "50V" from the
        // TF-button param ladder (`UpdateChartTypeParam`'s
        // `set_chart_type_param` call).
        store.set_chart_type(bid, "volume_bar");
        store.set_chart_type_param(bid, 50.0);
        assert_eq!(store.chart_type_param(bid), Some(50.0));

        // Switch to dollar_bar — mirrors `ChangeChartType`'s
        // `set_chart_type` call (chart_out_events.rs's `ChangeChartType`
        // arm) followed by its post-reset `chart_type_param` read.
        store.set_chart_type(bid, "dollar_bar");
        let leaked_param = store.chart_type_param(bid).unwrap_or(0.0);
        assert_eq!(
            leaked_param, 0.0,
            "set_chart_type must reset chart_type_param to the pre-first-pick sentinel on a type change",
        );

        // The subscribe built from the POST-SWITCH read must land on
        // dollar_bar's OWN registry default, not volume_bar's leftover 50.
        let params = build_subscribe_params("dollar_bar", leaked_param, "1m", BAR_COUNT, TICK_SIZE);
        let fold = fold_params_from_subscribe_params("dollar_bar", params)
            .expect("dollar_bar is a registered kline-fold chart_type");
        assert_eq!(
            fold.threshold, 100_000.0,
            "dollar_bar must subscribe at its OWN default (100000) after switching away from volume_bar(50), not the leaked 50",
        );

        // Toolbar label must agree with what actually got subscribed —
        // same coherence rule the registry-wide test above pins.
        let label_default = default_chart_type_param("dollar_bar").expect("dollar_bar has a param_spec");
        assert_eq!(label_default, fold.threshold, "toolbar label default must match the subscribed default");

        // Re-selecting the SAME type (no-op switch) must keep the dialed
        // value — only an actual family change resets the param.
        store.set_chart_type(bid, "dollar_bar");
        store.set_chart_type_param(bid, 250_000.0);
        store.set_chart_type(bid, "dollar_bar");
        assert_eq!(
            store.chart_type_param(bid), Some(250_000.0),
            "re-selecting the SAME chart_type must not reset the user's dialed-in param",
        );
    }
}
