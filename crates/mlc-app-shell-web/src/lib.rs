//! `mlc-app-shell-web` — browser shell for the MLC chart application.
//!
//! This crate is a **spike** (Phase 4 of the wasm migration plan).  It is the
//! wasm32 sibling of `mlc-app-shell-vello` (the native shell).  Everything in this
//! crate is cfg-gated under `#[cfg(target_arch = "wasm32")]` so that
//! `cargo check --workspace` (native) compiles it as an empty lib.
//!
//! # Build
//!
//! ```sh
//! cargo check -p mylittlechart-app-shell-web --target wasm32-unknown-unknown
//! wasm-pack build crates/mlc-app-shell-web --target web --out-dir ../../dist/mlc-app-shell-web
//! ```
//!
//! See `README.md` for full build + serve instructions.
//!
//! # Architecture sketch
//!
//! ```text
//! DataBridge::new().await               → Arc<DataBridge>
//! persistence::bootstrap_first_run()    ← default profile if localStorage is empty
//! ProfileManager::load(None)            ← profile + templates + presets + snapshots
//! BackendCore::new(bridge, None)        → Arc<BackendCore>   (no trading mgr on wasm)
//! ChartApp::new_window(core, ..)        → ChartApp  (seeds core from profile_manager,
//!                                          loads watchlists, restores/creates active preset)
//!
//! WebWindowProvider::from_id("chart")   ← DOM event source
//! Canvas2dChartRenderContext            ← render surface (mlc_core::RenderContext impl)
//!
//! requestAnimationFrame loop:
//!   provider.poll_events() → dispatch to ChartApp via mlc_app_input_shell::process_event
//!   chart_app.tick(now_ms) → flush_pending_autosave (active preset)
//!   persistence::save_persistent_state() when profile/watchlists/presets are dirty
//!   canvas_ctx = get_canvas_context()
//!   chart_app.render(&mut ctx, now_ms, false)
//!
//! visibilitychange (hidden) / beforeunload:
//!   persistence::save_persistent_state() + station.flush_persistence() (OPFS)
//! ```
//!
//! See `persistence.rs` for the full shell-persistence-duties writeup.

#[cfg(target_arch = "wasm32")]
mod context;
#[cfg(target_arch = "wasm32")]
mod persistence;

#[cfg(target_arch = "wasm32")]
use context::Canvas2dChartRenderContext;

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;
#[cfg(target_arch = "wasm32")]
use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;

#[cfg(target_arch = "wasm32")]
use uzor::layout::window::WindowProvider;
#[cfg(target_arch = "wasm32")]
use uzor_window_web::WebWindowProvider;

#[cfg(target_arch = "wasm32")]
use mlc_app::ChartApp;
#[cfg(target_arch = "wasm32")]
use mlc_backend_data_bridge::DataBridge;
#[cfg(target_arch = "wasm32")]
use mlc_app_input_shell::{ChromeFlavor, CHROME_HEIGHT};

// ─── Entry point ──────────────────────────────────────────────────────────────

/// WASM entry point — invoked automatically when the module is loaded.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() {
    std::panic::set_hook(Box::new(|info| {
        let loc = info.location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown location>".to_string());
        let msg = info.payload().downcast_ref::<&'static str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(|s| s.as_str()))
            .unwrap_or("<non-string payload>");
        let js_err = js_sys::Error::new("[panic-trace]");
        let stack = js_sys::Reflect::get(&js_err, &"stack".into())
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_else(|| "<no stack>".to_string());
        web_sys::console::error_1(
            &format!("[PANIC] at {} :: {}\n=== CALLER STACK ===\n{}", loc, msg, stack).into()
        );
        console_error_panic_hook::hook(info);
    }));
    // Route all `tracing::*` calls (dig3 connectors + station internals)
    // into the browser console. Without this the WS heal loops, mux churn,
    // and silence-timer fires are silent on wasm and the bridge appears
    // healthy when it's actually thrashing reconnects.
    // Bridge `log`-crate records (mlc-app layer) into tracing so they reach
    // the console alongside dig3's native tracing output.
    let _ = tracing_log::LogTracer::init();
    tracing_wasm::set_as_global_default_with_config(
        tracing_wasm::WASMLayerConfigBuilder::new()
            // TRACE so we see every per-frame dispatch (dig3::ws::frame),
            // not just connect/error events. Yes, this is loud — that's the
            // point. Filter in DevTools if needed.
            .set_max_level(tracing::Level::TRACE)
            .build(),
    );
    web_sys::console::log_1(&"[mlc-app-shell-web] booting".into());

    spawn_local(async {
        if let Err(e) = run().await {
            web_sys::console::error_1(&format!("[mlc-app-shell-web] fatal: {:?}", e).into());
        }
    });
}

// ─── Async boot sequence ───────────────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
async fn run() -> Result<(), JsValue> {
    let (bridge, live_rx, _connector_ready_rx) = DataBridge::new().await;
    // 2026-06-24: do NOT hold the first receiver alive — `broadcast::Sender`
    // measures "lag" per-receiver, and any unread receiver drags the slow-
    // consumer counter on the whole channel. With this rx idle, BarUpdate
    // frames piling up at 290/s overflow the 65536 buffer in <4 min and the
    // backend's properly-drained receiver silently gets `Lagged`-skipped on
    // every later try_recv. Drop it explicitly so the only live receiver is
    // the one BackendCore::new owns via `bridge.add_listener()`.
    drop(live_rx);
    let bridge = Arc::new(bridge);
    web_sys::console::log_1(&"[mlc-app-shell-web] bridge ready".into());

    // ── Shell persistence duties (parity with mlc-app-shell-vello/src/main.rs
    // ProfileManager::load / record_launch / save_profile boot sequence,
    // ~4928-4990) ────────────────────────────────────────────────────────────
    // First-run: no profile in localStorage yet — create a single "default"
    // profile (non-UI equivalent of the welcome wizard). Returning visits are
    // a no-op here; `ProfileManager::load` below resolves the existing one.
    persistence::bootstrap_first_run();

    let mut profile_manager = mlc_app::user_manager::ProfileManager::load(None);
    profile_manager.profile.record_launch(env!("CARGO_PKG_VERSION"));
    if let Err(e) = profile_manager.save_profile() {
        web_sys::console::error_1(
            &format!("[mlc-app-shell-web] failed to save profile at startup: {:?}", e).into(),
        );
    }

    // Best-effort: ask the browser not to evict localStorage under storage
    // pressure. Fire-and-forget — result is only logged.
    persistence::request_storage_persistence();

    // `ChartApp::new_window` seeds core.templates/core.presets/core.user from
    // `profile_manager` and calls `self.load_watchlists()` itself (reads
    // watchlists.json straight from `global_storage()`) — no manual
    // pre-seeding needed here, same division of labour as the native shell's
    // `App::new` → `mlc_app::ChartApp::new_window` call.
    let core = mlc_backend::BackendCore::new(bridge.clone(), None);
    let profile = profile_manager.profile.clone();
    // Restore = this shell's saved WindowState — the native shell passes
    // profile.windows[i] per OS window; the per-window state (active
    // preset id, open tabs, sidebar) lives THERE, not in the legacy
    // top-level profile fields. Passing None here made `new_window`
    // resolve an empty active_preset_id → `has_saved_preset == false` →
    // fresh default chart on EVERY boot (restore never ran).
    let restore = profile
        .ui
        .windows
        .iter()
        .find(|w| w.window_id == "win_web")
        .or_else(|| profile.ui.windows.first());
    let app = ChartApp::new_window(
        core,
        bridge.clone(),
        "win_web".to_string(),
        restore,
        &profile,
        &profile_manager,
        false,
    );
    web_sys::console::log_1(&"[mlc-app-shell-web] app ready".into());

    let provider = WebWindowProvider::from_id("chart")
        .map_err(|e| JsValue::from_str(&e))?;

    web_sys::console::log_1(&"[mlc-app-shell-web] render loop running".into());
    run_render_loop(app, provider, bridge)
}

/// Flush both persistence layers on tab close/hide.
///
/// - OPFS-backed market-data cache (`station.flush_persistence()`, pre-
///   existing) — async, buffered dat/idx/blob writes.
/// - Profile/preset/watchlist/template/snapshot state
///   ([`persistence::save_persistent_state`], new) — sync, localStorage.
///
/// Drop can't await an async flush on wasm, so the browser tears down the
/// wasm instance while OPFS buffers still hold uncommitted records; the
/// localStorage-backed profile writes are synchronous and complete before
/// the handler returns, so they don't need the same care, but are run from
/// the same two listeners for one obvious flush point. Two listeners cover
/// both real teardown paths (visibilitychange="hidden" is the more-reliable
/// signal on modern browsers; beforeunload for legacy).
#[cfg(target_arch = "wasm32")]
fn install_persistence_flush_hooks(
    bridge: Arc<mlc_backend_data_bridge::DataBridge>,
    state: Rc<RefCell<WebAppState>>,
) {
    let Some(win) = web_sys::window() else { return; };

    // visibilitychange — fires when the tab is hidden (background/close).
    {
        let bridge = bridge.clone();
        let state = state.clone();
        let cb = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            let doc = match web_sys::window().and_then(|w| w.document()) {
                Some(d) => d,
                None => return,
            };
            if !doc.hidden() {
                return;
            }
            // Best-effort flush: the event can fire while the RAF tick (or
            // another handler) already holds the borrow — a hard borrow_mut
            // here is a guaranteed same-thread RefCell panic. Skip; the
            // per-frame dirty drain persists on the next tick anyway.
            // Also gated on `is_persist_safe` — same rationale as the
            // per-frame drain in `run_render_loop`: a tab hidden mid-cold-
            // load must not persist the pre-restore placeholder window.
            if let Ok(mut st) = state.try_borrow_mut() {
                if mlc_app::persistence::autosave::is_persist_safe(&st.chart.load_state) {
                    persistence::save_persistent_state(&mut st.chart);
                }
            }
            let station = bridge.station();
            spawn_local(async move {
                match station.flush_persistence().await {
                    Ok(()) => {
                        web_sys::console::log_1(&"[mlc-app-shell-web] persistence flushed (hidden)".into());
                    }
                    Err(errs) => {
                        web_sys::console::warn_1(&format!(
                            "[mlc-app-shell-web] persistence flush errors ({} kinds)", errs.len()
                        ).into());
                    }
                }
            });
        });
        let _ = win.add_event_listener_with_callback(
            "visibilitychange", cb.as_ref().unchecked_ref(),
        );
        // Leak the closure — it must outlive this scope for the lifetime of
        // the shell (there is only one), same pattern as WebWindowProvider's
        // long-lived DOM listeners.
        cb.forget();
    }

    // beforeunload — legacy path for browsers that skip visibilitychange on
    // real navigation-away. Same handler, no visibility gate.
    {
        let bridge = bridge.clone();
        let state = state.clone();
        let cb = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            // Best-effort — same try_borrow + is_persist_safe rationale as
            // the visibilitychange hook above.
            if let Ok(mut st) = state.try_borrow_mut() {
                if mlc_app::persistence::autosave::is_persist_safe(&st.chart.load_state) {
                    persistence::save_persistent_state(&mut st.chart);
                }
            }
            let station = bridge.station();
            spawn_local(async move {
                let _ = station.flush_persistence().await;
            });
        });
        let _ = win.add_event_listener_with_callback(
            "beforeunload", cb.as_ref().unchecked_ref(),
        );
        cb.forget();
    }
}

// ─── App state ────────────────────────────────────────────────────────────────

/// Per-session state for the web shell, replacing the old thread_local! PRESS_POS / MOUSE_PRESSED.
#[cfg(target_arch = "wasm32")]
struct WebAppState {
    chart:              ChartApp,
    input:              mlc_app_input_shell::InputState,
    chrome:             mlc_app_input_shell::ChromeState,
    dirty:              mlc_app_input_shell::DirtyFlags,
    out:                mlc_app_input_shell::DispatchOut,
    toolbar_tooltip:    mlc_app_input_shell::TooltipState,
    screenshot_pending: bool,
    /// DOM canvas element for cursor CSS updates.
    canvas_id:          String,
    /// Settle-debounce for `chart.resize` (audit D2).  `canvas.client_width()/
    /// client_height()` is measured every RAF frame (cheap), but a raw per-frame
    /// value can land mid-reflow (47px slivers observed) if it's applied straight
    /// into viewport geometry — native only resizes on discrete OS events, wasm
    /// has no such boundary since there's no dedicated resize callback wired to
    /// `chart.resize` (the DOM `resize` listener only feeds `poll_events`, and
    /// layout can also change without a `resize` event, e.g. sidebar toggle).
    /// `last_measured` holds the previous frame's raw reading; `last_applied`
    /// holds the size actually handed to `chart.resize`. The size is applied
    /// only once two consecutive frames agree, so a single sliver frame never
    /// reaches viewport geometry — cost is one frame (~16ms) of extra latency.
    last_measured:      Option<(u32, u32)>,
    last_applied:       Option<(u32, u32)>,
    /// Frame-time EMA for the Monitoring panel (wasm-native browsers have
    /// no sysinfo; fps/frame-time is the honest subset we can measure via
    /// `performance.now()`). `last_render_ms` holds the previous frame's
    /// `now_ms` at the top of the render callback; delta feeds `fps_ema`
    /// with the same 0.9/0.1 low-pass as `mlc-app-shell-vello`.
    fps_ema:            f64,
    last_frame_time_ms: f64,
    last_render_ms:     Option<f64>,
    /// DataBridge handle so the Monitoring/Performance tab can read live
    /// counts (ws_task_count_total, connector count) same as native.
    bridge:             Arc<mlc_backend_data_bridge::DataBridge>,
}

#[cfg(target_arch = "wasm32")]
impl WebAppState {
    fn new(chart: ChartApp, canvas_id: &str, bridge: Arc<mlc_backend_data_bridge::DataBridge>) -> Self {
        let mut input = mlc_app_input_shell::InputState::default();
        // Web shell renders chrome — offset chart by chrome height.
        input.chrome_height = CHROME_HEIGHT;
        let mut chrome = mlc_app_input_shell::ChromeState::new("");
        chrome.flavor = ChromeFlavor::Web;
        Self {
            chart,
            input,
            chrome,
            dirty:  mlc_app_input_shell::DirtyFlags::default(),
            out:    mlc_app_input_shell::DispatchOut::default(),
            toolbar_tooltip:    mlc_app_input_shell::TooltipState::new(),
            screenshot_pending: false,
            canvas_id:          canvas_id.to_string(),
            last_measured:      None,
            last_applied:       None,
            fps_ema:            60.0,
            last_frame_time_ms: 16.0,
            last_render_ms:     None,
            bridge,
        }
    }
}

// ─── RAF render loop ───────────────────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
fn run_render_loop(
    app: ChartApp,
    provider: WebWindowProvider,
    bridge: Arc<mlc_backend_data_bridge::DataBridge>,
) -> Result<(), JsValue> {
    let state = Rc::new(RefCell::new(WebAppState::new(app, "chart", bridge.clone())));

    // Wire Ctrl+C / Ctrl+X to read the active selection out of TextFieldStore.
    // The closure runs synchronously inside the DOM `copy` / `cut` listener in
    // uzor-window-web — only window allowed to write `clipboardData`.
    {
        let state_for_copy = state.clone();
        provider.set_copy_provider(move || {
            let st = state_for_copy.borrow();
            let coord = st.chart.input_coordinator.borrow();
            coord.text_fields().copy_selection()
        });
    }

    // Flush profile/preset/watchlist state (localStorage) + OPFS-backed
    // market-data cache on tab close/hide. Parity duty (see doc comment on
    // `install_persistence_flush_hooks`) — was already present for OPFS,
    // now covers user state too.
    install_persistence_flush_hooks(bridge, state.clone());

    let provider = Rc::new(RefCell::new(provider));

    let closure: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let closure_clone = closure.clone();

    let state_clone = state.clone();
    let provider_clone = provider.clone();

    // RAF-stall watchdog (incident 2026-07-16): overnight Chrome throttled
    // requestAnimationFrame to zero on a background tab — the frame closure
    // below never ran, `tick_writers`' 65536-cap broadcast overflowed, and
    // 112,652 `LiveUpdate`s dropped silently while the WS callbacks kept
    // delivering. Stamped at the TOP of the frame closure body (below); the
    // `set_interval` watchdog further down reads it to detect the stall.
    let last_frame_ms: Rc<std::cell::Cell<f64>> = Rc::new(std::cell::Cell::new(js_sys::Date::now()));
    let last_frame_ms_for_raf = last_frame_ms.clone();

    *closure.borrow_mut() = Some(Closure::wrap(Box::new(move || {
        // ── RAF-stall watchdog stamp ─────────────────────────────────────────
        last_frame_ms_for_raf.set(js_sys::Date::now());

        // ── Diag counter ─────────────────────────────────────────────────────
        if let Some(win) = web_sys::window() {
            let _ = js_sys::Reflect::set(
                &win,
                &"MLC_FRAMES".into(),
                &js_sys::Reflect::get(&win, &"MLC_FRAMES".into())
                    .map(|v| v.as_f64().unwrap_or(0.0) + 1.0)
                    .unwrap_or(1.0)
                    .into(),
            );
        }

        // ── Process DOM events ────────────────────────────────────────────────
        let events = provider_clone.borrow_mut().poll_events();
        {
            let mut st = state_clone.borrow_mut();
            let canvas_id = st.canvas_id.clone();
            for event in events {
                dispatch_event(&mut st, event, &canvas_id);
            }
        }

        // ── Tick ──────────────────────────────────────────────────────────────
        {
            let now_ms = web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let mut st = state_clone.borrow_mut();
            st.chart.tick(now_ms);
            // Drain queued WatchlistActions into core.watchlist_manager + bridge.
            // Native equivalent: mlc-app-shell-vello/main.rs:2191-2360 (which writes
            // into a parallel AppState cache). Wasm has one window — drive the
            // single source of truth (core.watchlist_manager) directly.
            // Without this, Symbol Search star clicks queue Toggle actions that
            // nobody consumes — symbols never reach the watchlist and the
            // mini-ticker subscription never starts.
            st.chart.drain_watchlist_actions();
        }

        // ── Drain deferred cold-preset apply ─────────────────────────────────
        // Native equivalent: mlc-app-shell-vello/main.rs:4462-4482, gated on
        // GpuDone (one frame after the promote). Wasm has no such fence — the
        // RAF callback already IS "one frame later" relative to whatever
        // dispatch_event call above set `pending_cold_apply` (e.g. a chrome
        // tab click routing through `LoadPreset`), so draining immediately
        // here is the correct wasm-shaped equivalent, not a shortcut.
        //
        // `pending_cold_apply` is the ONLY place that clears `load_state`
        // back to `Live` after `LoadPreset` stashes the cold-apply body and
        // flips to `Loading` (chart_out_events.rs:142). Before this was
        // wired, wasm never called this drain anywhere — any `LoadPreset`
        // (chrome tab click, preset load) would stash the body once and then
        // sit forever on the loading skeleton because nothing ever consumed
        // the stash. Parity-sweep finding, not cosmetic.
        //
        // MUST run before the persistence drain below: on the FIRST frame
        // after boot (and on every warm-miss chrome-tab switch),
        // `ChartApp::new_window` already called `process_chart_out_event
        // (LoadPreset)` synchronously during construction — that call sets
        // `active_preset_id`, calls `persist_profile()` (→ `profile_dirty =
        // true`), and then STASHES the actual restore body (chart_type,
        // bubbles, viewport snap-arming) into `pending_cold_apply` instead of
        // applying it inline (`chart_out_events.rs:1974-1991`). If the
        // persistence drain ran first on that frame, it would
        // `build_window_state()` + overwrite `profile.json` from a window
        // whose bubbles/chart_type have not been restored yet (still
        // whatever `new_window`'s placeholder container carried — "candles",
        // no attached bubble) while `load_state == Loading`. Draining the
        // cold apply first means `chart_type`/viewport-snap-arming land
        // before the persistence gate below even runs its `is_persist_safe`
        // check.
        {
            let mut st = state_clone.borrow_mut();
            st.chart.drain_pending_cold_apply();
        }

        // ── Persist dirty user state ──────────────────────────────────────────
        // Native equivalent: mlc-app-shell-vello/main.rs:2743-2864 (per-frame
        // dirty-flag drain inside `about_to_wait`). `profile_dirty` /
        // `profile_geometry_dirty` / `watchlists_dirty` are shared `ChartApp`
        // fields already set by shared input-handling code (chrome tab
        // clicks, preset rename/delete, watchlist edits, ...) — this was the
        // one piece with nobody on the wasm side ever reading them. Also
        // covers `PresetAction` → disk (native step 9 of `save_all`):
        // `flush_pending_autosave` (tick(), above) only stages preset
        // upserts into `preset_actions` / the in-memory core registry —
        // nothing wrote them to `global_storage()` before this.
        //
        // `is_persist_safe` gate (mirrors `flush_pending_autosave` /
        // `collect_state_into_preset`, both of which early-return under the
        // same `LoadState::Loading` check): while a cold apply is stashed
        // (`LoadPreset` set `profile_dirty = true` synchronously at
        // construction time, BEFORE the drain above could possibly have run
        // yet on the very first call inside `new_window`'s own constructor),
        // saving here would persist the pre-restore placeholder window state
        // — the disaster path `flush_pending_autosave`'s own doc comment
        // calls out by name (`7a9d1d2`). `drain_pending_cold_apply` above
        // already flips `load_state` back to `Live` once the real restore
        // finishes, so by the time this check runs post-drain the gate opens
        // on the SAME frame the cold apply completed.
        {
            let mut st = state_clone.borrow_mut();
            let persist_safe = mlc_app::persistence::autosave::is_persist_safe(&st.chart.load_state);
            if persist_safe
                && (st.chart.profile_dirty || st.chart.profile_geometry_dirty || st.chart.watchlists_dirty
                    || !st.chart.preset_actions.is_empty())
            {
                persistence::save_persistent_state(&mut st.chart);
            }
        }

        // ── Diag: expose active bubble's last bar timestamp + bars count + close
        //     to JS so `dev-attach --eval window.MLC_DEBUG` can see whether the
        //     live tick actually reaches the bubble (the chart reads from there).
        if let Some(win) = web_sys::window() {
            let st = state_clone.borrow();
            let active_cid = st.chart.panel_app.panel_grid.active_container_id();
            let active_bid = active_cid.and_then(|cid| st.chart.bubble_for_chart(cid));
            let (sym, count, last_ts, last_close) = active_bid
                .and_then(|bid| st.chart.with_bubble(bid, |b| {
                    let last = b.bars.last();
                    (
                        b.address.symbol.clone(),
                        b.bars.len() as f64,
                        last.map(|x| x.timestamp as f64).unwrap_or(0.0),
                        last.map(|x| x.close).unwrap_or(0.0),
                    )
                }))
                .unwrap_or_else(|| ("<none>".to_string(), 0.0, 0.0, 0.0));
            // Footprint-hang diag (handoff 2026-07-05 §2.1-2.3): expose BOTH
            // series' extents + the derived-seed flag so the cluster/grid
            // drift and the cold-open spinner can be diagnosed from live
            // numbers instead of code-guessing.
            let (first_ts_bubble, fp_len, fp_first_ts, fp_last_ts, fp_last_cum_delta) = active_bid
                .and_then(|bid| st.chart.with_bubble(bid, |b| {
                    let fp = b.footprint_bars.as_deref().unwrap_or(&[]);
                    (
                        b.bars.first().map(|x| x.timestamp as f64).unwrap_or(0.0),
                        fp.len() as f64,
                        fp.first().map(|x| x.open_time as f64).unwrap_or(0.0),
                        fp.last().map(|x| x.open_time as f64).unwrap_or(0.0),
                        fp.last().map(|x| x.cumulative_delta).unwrap_or(0.0),
                    )
                }))
                .unwrap_or((0.0, 0.0, 0.0, 0.0, 0.0));
            let chart_type = active_cid
                .and_then(|cid| st.chart.chart_type_for_chart(cid))
                .unwrap_or("<none>");
            let (view_start, bar_spacing, chart_width, bar_count_vp) = active_cid
                .and_then(|cid| st.chart.panel_app.panel_grid.containers().get(&cid))
                .map(|w| (
                    w.viewport().view_start,
                    w.viewport().bar_spacing,
                    w.viewport().chart_width,
                    w.viewport().bar_count as f64,
                ))
                .unwrap_or((0.0, 0.0, 0.0, 0.0));
            let dbg = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&dbg, &"symbol".into(), &sym.into());
            let _ = js_sys::Reflect::set(&dbg, &"bars".into(), &count.into());
            let _ = js_sys::Reflect::set(&dbg, &"last_ts".into(), &last_ts.into());
            let _ = js_sys::Reflect::set(&dbg, &"last_close".into(), &last_close.into());
            let _ = js_sys::Reflect::set(&dbg, &"view_start".into(), &view_start.into());
            let _ = js_sys::Reflect::set(&dbg, &"bar_spacing".into(), &bar_spacing.into());
            let _ = js_sys::Reflect::set(&dbg, &"chart_width".into(), &chart_width.into());
            let _ = js_sys::Reflect::set(&dbg, &"bar_count_vp".into(), &bar_count_vp.into());
            let _ = js_sys::Reflect::set(&dbg, &"chart_type".into(), &chart_type.into());
            let _ = js_sys::Reflect::set(&dbg, &"first_ts_bubble".into(), &first_ts_bubble.into());
            let _ = js_sys::Reflect::set(&dbg, &"fp_len".into(), &fp_len.into());
            let _ = js_sys::Reflect::set(&dbg, &"fp_first_ts".into(), &fp_first_ts.into());
            let _ = js_sys::Reflect::set(&dbg, &"fp_last_ts".into(), &fp_last_ts.into());
            let _ = js_sys::Reflect::set(&dbg, &"fp_last_cum_delta".into(), &fp_last_cum_delta.into());
            // Indicator introspection (owner report 2026-07-05: VOLUME
            // neither auto-added nor visible in the picker) — expose whether
            // the definition registered and what instances the active bubble
            // actually carries, so the failing hop is visible live.
            {
                let has_volume_def = st.chart.core.indicators()
                    .definitions()
                    .iter()
                    .any(|d| d.type_id.eq_ignore_ascii_case("volume"));
                let _ = js_sys::Reflect::set(&dbg, &"def_volume".into(), &has_volume_def.into());
                let inst_list: String = active_bid
                    .map(|bid| st.chart.core.indicators()
                        .instances_for_bubble(bid)
                        .iter()
                        .map(|i| format!("{}#{}", i.type_id, i.id))
                        .collect::<Vec<_>>()
                        .join(","))
                    .unwrap_or_default();
                let _ = js_sys::Reflect::set(&dbg, &"instances".into(), &inst_list.into());
                let def_count = st.chart.core.indicators().definitions().len() as f64;
                let _ = js_sys::Reflect::set(&dbg, &"def_count".into(), &def_count.into());
                use std::sync::atomic::Ordering::Relaxed;
                let _ = js_sys::Reflect::set(&dbg, &"comp_sync_runs".into(),
                    &(mlc_app::input::chart_out_events::COMPANION_SYNC_RUNS.load(Relaxed) as f64).into());
                let _ = js_sys::Reflect::set(&dbg, &"comp_add_ok".into(),
                    &(mlc_app::input::chart_out_events::COMPANION_ADD_OK.load(Relaxed) as f64).into());
                let _ = js_sys::Reflect::set(&dbg, &"comp_add_fail".into(),
                    &(mlc_app::input::chart_out_events::COMPANION_ADD_FAIL.load(Relaxed) as f64).into());
            }

            // Compare overlay diag — for debugging the compare-chart retry path.
            // Shows what compare series are currently mounted on the active
            // window and what's still queued in `pending_compare` waiting for
            // BarsLoaded.
            {
                let pending: Vec<String> = st.chart.pending_compare_diag();
                let pending_arr = js_sys::Array::new();
                for s in &pending {
                    pending_arr.push(&s.clone().into());
                }
                let _ = js_sys::Reflect::set(&dbg, &"pending_compare".into(), &pending_arr);

                let series: Vec<String> = active_cid
                    .and_then(|cid| st.chart.panel_app.panel_grid.containers().get(&cid))
                    .map(|w| w.compare_overlay.series.iter()
                        .map(|s| format!("{} bars={} vis={}", s.symbol, s.bars.len(), s.visible))
                        .collect())
                    .unwrap_or_default();
                let series_arr = js_sys::Array::new();
                for s in &series {
                    series_arr.push(&s.clone().into());
                }
                let _ = js_sys::Reflect::set(&dbg, &"compare_series".into(), &series_arr);

                let _ = js_sys::Reflect::set(&dbg, &"modal".into(), &st.chart.modal_state_diag().into());
                let _ = js_sys::Reflect::set(&dbg, &"cmp_attempts".into(), &(st.chart.compare_add_attempts() as f64).into());
                let _ = js_sys::Reflect::set(&dbg, &"cmp_success".into(), &(st.chart.compare_add_success() as f64).into());
                let _ = js_sys::Reflect::set(&dbg, &"cmp_queued".into(), &(st.chart.compare_add_queued() as f64).into());
            }

            let _ = js_sys::Reflect::set(&win, &"MLC_DEBUG".into(), &dbg);
        }

        // ── Render frame ──────────────────────────────────────────────────────
        let render_result = render_frame(state_clone.clone());
        if let Err(e) = render_result {
            web_sys::console::error_1(&format!("[mlc-app-shell-web] render error: {:?}", e).into());
        }

        // ── Screenshot capture ────────────────────────────────────────────────
        // Check AFTER render so the canvas contains the just-painted frame.
        // Two triggers feed the same path:
        //   1. `WebAppState.screenshot_pending` — set by the shell Ctrl+S
        //      branch (`mlc-app-input-shell/src/dispatch.rs:635`) via `&mut bool`.
        //   2. `ChartApp.drain_pending_screenshot()` — set by
        //      `ChartApp::request_screenshot()`, which is what the toolbar
        //      camera button reaches through
        //      `ChartOutEvent::Screenshot → process_chart_out_event` (same path
        //      native uses).  Without draining this on every frame the toolbar
        //      button silently misses.
        {
            let pending = {
                let mut st = state_clone.borrow_mut();
                let shell_flag = st.screenshot_pending;
                if shell_flag { st.screenshot_pending = false; }
                let chart_flag = st.chart.drain_pending_screenshot();
                shell_flag || chart_flag
            };
            if pending {
                trigger_screenshot_to_clipboard("chart");
            }
        }

        // ── Drain pending REST cure ───────────────────────────────────────────
        // Mirrors the native shell (mlc-app-shell-vello/src/main.rs:4372).  Any flow
        // that enqueues onto `pending_cure_after_preset_load` (LoadPreset warm
        // path, do_split, retag — anything that needs a fresh REST sweep)
        // relies on a per-frame drain on the host side, otherwise the queue
        // accumulates forever and the new chart sits on stale data.
        {
            let mut st = state_clone.borrow_mut();
            st.chart.drain_pending_cure_after_preset_load();
        }

        // ── Background-warm: pre-hydrate open tabs into live_preset_cache ─────
        // Native equivalent: mlc-app-shell-vello/main.rs:4491-4494. Pure
        // optimization (self-paced VecDeque, idempotent, exits when empty) —
        // without it every chrome tab click is a cold load instead of a
        // zero-flicker cache hit. Same per-frame-drain shape as the cure
        // drain above.
        {
            let mut st = state_clone.borrow_mut();
            st.chart.seed_background_warm_if_idle();
            st.chart.drain_one_background_warm();
        }

        // ── Schedule next frame ───────────────────────────────────────────────
        if let Some(win) = web_sys::window() {
            if let Some(cb) = closure_clone.borrow().as_ref() {
                let _ = win.request_animation_frame(cb.as_ref().unchecked_ref());
            }
        }
    }) as Box<dyn FnMut()>));

    if let Some(win) = web_sys::window() {
        if let Some(cb) = closure.borrow().as_ref() {
            let _ = win.request_animation_frame(cb.as_ref().unchecked_ref());
        }
    }

    std::mem::forget(closure);

    // RAF-stall fallback drain (incident 2026-07-16): `setInterval` keeps
    // firing at roughly its requested cadence even on a backgrounded tab
    // under Chrome's intensive-timer-throttling policy (clamped to ~1/min),
    // unlike `requestAnimationFrame`, which Chrome can throttle all the way
    // to zero. When the RAF frame closure hasn't stamped `last_frame_ms` in
    // over 15s, RAF is stalled — run a DATA-ONLY `ChartApp::tick` (the
    // sanctioned single-window `tick_writers` drain +
    // `react_to_events`, see `mlc-app/src/frame_lifecycle/tick.rs`) so the
    // 65536-cap broadcast channel keeps draining instead of overflowing.
    // Even a once-per-minute drain comfortably keeps up with the ~700
    // updates/min the incident's overflow was measured at (112,652 dropped
    // over ~4h). Deliberately does NOT render and does NOT reschedule RAF
    // (rendering into a backgrounded/invisible tab is wasted GPU work the
    // browser will throttle anyway) and does NOT stamp `last_frame_ms`
    // (which would silence the watchdog while RAF is still stalled).
    {
        let state_for_watchdog = state.clone();
        let watchdog_closure = Closure::wrap(Box::new(move || {
            if js_sys::Date::now() - last_frame_ms.get() > 15_000.0 {
                // Timestamp source matches the RAF frame closure's own
                // `tick` call above (`web_time::SystemTime`), not
                // `js_sys::Date::now()` (used only for the stall check
                // against `last_frame_ms`, which is stamped from the same
                // source) — keeps `ChartApp::tick`'s wall-clock reads
                // consistent regardless of which path drove them.
                let now_ms = web_time::SystemTime::now()
                    .duration_since(web_time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                if let Ok(mut st) = state_for_watchdog.try_borrow_mut() {
                    st.chart.tick(now_ms);
                }
            }
        }) as Box<dyn FnMut()>);
        if let Some(win) = web_sys::window() {
            let _ = win.set_interval_with_callback_and_timeout_and_arguments_0(
                watchdog_closure.as_ref().unchecked_ref(),
                5_000,
            );
        }
        std::mem::forget(watchdog_closure);
    }

    Ok(())
}

// ─── WebPlatform ──────────────────────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
struct WebPlatform<'a> {
    canvas_id: &'a str,
}

#[cfg(target_arch = "wasm32")]
fn cursor_style_to_css(style: mlc_core::CursorStyle) -> &'static str {
    match style {
        mlc_core::CursorStyle::Default    => "default",
        mlc_core::CursorStyle::Pointer    => "pointer",
        mlc_core::CursorStyle::Grab       => "grab",
        mlc_core::CursorStyle::Grabbing   => "grabbing",
        mlc_core::CursorStyle::Move       => "move",
        mlc_core::CursorStyle::NsResize   => "ns-resize",
        mlc_core::CursorStyle::EwResize   => "ew-resize",
        mlc_core::CursorStyle::NeswResize => "nesw-resize",
        mlc_core::CursorStyle::NwseResize => "nwse-resize",
        mlc_core::CursorStyle::Crosshair  => "crosshair",
        mlc_core::CursorStyle::NotAllowed => "not-allowed",
        mlc_core::CursorStyle::None       => "none",
    }
}

#[cfg(target_arch = "wasm32")]
fn get_canvas(canvas_id: &str) -> Option<web_sys::HtmlCanvasElement> {
    use wasm_bindgen::JsCast;
    let doc = web_sys::window()?.document()?;
    doc.get_element_by_id(canvas_id)?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .ok()
}

#[cfg(target_arch = "wasm32")]
impl mlc_app_input_shell::Platform for WebPlatform<'_> {
    fn set_cursor(&self, style: mlc_core::CursorStyle) {
        if let Some(canvas) = get_canvas(self.canvas_id) {
            let _ = canvas.style().set_property("cursor", cursor_style_to_css(style));
        }
    }

    fn set_cursor_visible(&self, visible: bool) {
        if let Some(canvas) = get_canvas(self.canvas_id) {
            let css = if visible { "" } else { "none" };
            let _ = canvas.style().set_property("cursor", css);
        }
    }

    fn drag_window(&self) {}
    fn drag_resize_window(&self, _: mlc_app_input_shell::ResizeDir) {}
    fn set_minimized(&self, _: bool) {}
    fn set_maximized(&self, _: bool) {}
    fn is_maximized(&self) -> bool { false }

    fn window_size(&self) -> (u32, u32) {
        get_canvas(self.canvas_id)
            .map(|c| (c.client_width() as u32, c.client_height() as u32))
            .unwrap_or((800, 600))
    }

    fn clipboard_write(&self, _text: &str) {
        // Async clipboard API — stub until wasm clipboard PR.
    }

    fn clipboard_read(&self) -> Option<String> {
        None
    }

    fn request_redraw(&self) {
        // RAF loop drives itself.
    }
}

// ─── Input dispatch ────────────────────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
fn map_platform_event_to_shell(
    event: uzor::input::core::event_processor::PlatformEvent,
    state: &mlc_app_input_shell::InputState,
) -> Option<mlc_app_input_shell::ShellEvent> {
    use uzor::input::core::event_processor::PlatformEvent;
    use uzor::input::keyboard::events::KeyCode;
    use uzor::input::pointer::state::MouseButton;
    use mlc_app_input_shell::{ShellEvent, MouseBtn, NormalizedModifiers, LogicalKey, PhysicalKey};

    match event {
        PlatformEvent::PointerDown { x, y, button: MouseButton::Left } => {
            Some(ShellEvent::MouseInput { button: MouseBtn::Left, pressed: true, x, y })
        }
        PlatformEvent::PointerUp { x, y, button: MouseButton::Left } => {
            Some(ShellEvent::MouseInput { button: MouseBtn::Left, pressed: false, x, y })
        }
        PlatformEvent::PointerMoved { x, y } => {
            Some(ShellEvent::CursorMoved { x, y })
        }
        PlatformEvent::PointerLeft => {
            Some(ShellEvent::CursorLeft)
        }
        PlatformEvent::Scroll { dx, dy } => {
            Some(ShellEvent::Scroll { dx, dy })
        }
        PlatformEvent::WindowResized { width, height } => {
            Some(ShellEvent::Resized { w: width, h: height })
        }
        PlatformEvent::PointerDown { x: _, y: _, button: MouseButton::Right } => {
            Some(ShellEvent::MouseInput { button: MouseBtn::Right, pressed: true,
                x: state.last_mouse_pos.0, y: state.last_mouse_pos.1 })
        }
        PlatformEvent::PointerUp { x: _, y: _, button: MouseButton::Right } => {
            Some(ShellEvent::MouseInput { button: MouseBtn::Right, pressed: false,
                x: state.last_mouse_pos.0, y: state.last_mouse_pos.1 })
        }

        // Keyboard.  Mirrors mlc-app-shell-vello/src/window/event_handler.rs:101-150 —
        // KeyDown only (no KeyUp), explicit Logical/Physical mapping so modals
        // and text fields see Escape / Enter / Backspace / character input.
        PlatformEvent::ModifiersChanged { modifiers } => {
            Some(ShellEvent::ModifiersChanged(NormalizedModifiers {
                ctrl:  modifiers.ctrl,
                shift: modifiers.shift,
                alt:   modifiers.alt,
            }))
        }
        PlatformEvent::KeyDown { key, .. } => {
            let logical = match key {
                KeyCode::Escape    => LogicalKey::Escape,
                KeyCode::Backspace => LogicalKey::Backspace,
                KeyCode::Enter     => LogicalKey::Enter,
                KeyCode::Space     => LogicalKey::Space,
                KeyCode::Tab       => LogicalKey::Tab,
                KeyCode::Delete    => LogicalKey::Delete,
                KeyCode::ArrowLeft  => LogicalKey::ArrowLeft,
                KeyCode::ArrowRight => LogicalKey::ArrowRight,
                KeyCode::ArrowUp    => LogicalKey::ArrowUp,
                KeyCode::ArrowDown  => LogicalKey::ArrowDown,
                KeyCode::Home       => LogicalKey::Home,
                KeyCode::End        => LogicalKey::End,
                KeyCode::PageUp     => LogicalKey::PageUp,
                KeyCode::PageDown   => LogicalKey::PageDown,
                _                   => LogicalKey::Other,
            };
            let physical = match key {
                KeyCode::A => PhysicalKey::KeyA,
                KeyCode::B => PhysicalKey::KeyB,
                KeyCode::C => PhysicalKey::KeyC,
                KeyCode::S => PhysicalKey::KeyS,
                KeyCode::V => PhysicalKey::KeyV,
                KeyCode::X => PhysicalKey::KeyX,
                KeyCode::Y => PhysicalKey::KeyY,
                KeyCode::Z => PhysicalKey::KeyZ,
                _          => PhysicalKey::Other,
            };
            Some(ShellEvent::KeyPressed { logical, physical })
        }
        PlatformEvent::TextInput { text } => {
            // Treat as IME commit so text-fields ingest typed characters.
            Some(ShellEvent::ImeCommit(text))
        }
        PlatformEvent::ClipboardPaste { text } => {
            // Ctrl+V routed via the `paste` DOM listener in uzor-window-web.
            // ImeCommit feeds the whole string into the focused TextFieldStore
            // field at once — matches winit Ime::Commit on native.
            Some(ShellEvent::ImeCommit(text))
        }
        PlatformEvent::ClipboardCut => {
            // Ctrl+X: the `cut` DOM listener has already written the selection
            // to the OS clipboard.  Now we synthesize a Backspace into the
            // focused field; TextFieldStore's Backspace handler deletes the
            // current selection if one is active (store.rs:454).
            Some(ShellEvent::KeyPressed {
                logical:  LogicalKey::Backspace,
                physical: PhysicalKey::Other,
            })
        }

        _ => None,
    }
}

#[cfg(target_arch = "wasm32")]
fn dispatch_event(
    st: &mut WebAppState,
    event: uzor::input::core::event_processor::PlatformEvent,
    canvas_id: &str,
) {
    if let Some(shell_ev) = map_platform_event_to_shell(event, &st.input) {
        let platform = WebPlatform { canvas_id };
        mlc_app_input_shell::process_event(
            &mut st.input,
            &mut st.chrome,
            &mut st.dirty,
            &mut st.chart,
            &platform,
            shell_ev,
            &mut st.toolbar_tooltip,
            &mut st.screenshot_pending,
            &mut st.out,
        );
        // Reset one-shot flags.
        st.out = mlc_app_input_shell::DispatchOut::default();
    }
}

// ─── Render frame ──────────────────────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
fn render_frame(state: Rc<RefCell<WebAppState>>) -> Result<(), JsValue> {
    use wasm_bindgen::JsCast;

    let win = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let doc = win.document().ok_or_else(|| JsValue::from_str("no document"))?;
    let canvas = doc
        .get_element_by_id("chart")
        .ok_or_else(|| JsValue::from_str("canvas#chart not found"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()?;

    let dpr = win.device_pixel_ratio();

    let css_w = canvas.client_width() as u32;
    let css_h = canvas.client_height() as u32;
    let phys_w = (css_w as f64 * dpr) as u32;
    let phys_h = (css_h as f64 * dpr) as u32;
    if canvas.width() != phys_w { canvas.set_width(phys_w); }
    if canvas.height() != phys_h { canvas.set_height(phys_h); }

    let raw_ctx = canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("could not get 2d context"))?
        .dyn_into::<web_sys::CanvasRenderingContext2d>()?;

    raw_ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0)?;

    // ── Settle-debounced resize (audit D2) ─────────────────────────────────
    // `canvas.client_width()/client_height()` is measured every RAF frame
    // above (cheap DOM read), but a raw per-frame value can land mid-reflow —
    // 47px slivers observed — if fed straight into `chart.resize`. Native only
    // resizes on discrete OS `WindowResized` events; wasm's DOM `resize`
    // listener alone is not enough because layout can also change without a
    // `resize` event firing (sidebar toggle, flex/CSS changes). Apply only
    // once two consecutive frames agree on the same (css_w, css_h) AND that
    // value differs from what's already applied — one-frame latency (~16ms)
    // is the cost, and the DOM-reflow sliver class stops reaching viewport
    // geometry entirely. First frame ever (no previous measurement) applies
    // immediately so the chart isn't left at 0×0 for a frame.
    {
        let mut st = state.borrow_mut();
        let measured = (css_w, css_h);
        let is_first_frame = st.last_measured.is_none() && st.last_applied.is_none();
        let settled = st.last_measured == Some(measured);
        let already_applied = st.last_applied == Some(measured);
        if (is_first_frame || settled) && !already_applied {
            let chart_h = measured.1.saturating_sub(CHROME_HEIGHT as u32);
            st.chart.resize(measured.0, chart_h);
            st.last_applied = Some(measured);
        }
        st.last_measured = Some(measured);
    }

    // Rendering uses the RAW per-frame css size (not the debounced-applied
    // size) purely for canvas2d transform/fill extents
    // (`Canvas2dChartRenderContext::canvas_width/height`) — chart layout
    // itself reads `chart.width`/`chart.height` (set by the applied resize
    // above), not from `ctx`, via `prepare_frame`. `set_coordinate_space`
    // overwrites `chart_width`/`chart_height` on `ctx` per sub-pane before any
    // primitive draws, so the two dimensions never fight over chart geometry —
    // the only consumers of the raw ctx dimensions are `canvas_width()`/
    // `canvas_height()`, used as a cosmetic fallback bound in a couple of
    // drawing primitives (cycle_lines/time_cycles), not layout-critical.
    let mut ctx = Canvas2dChartRenderContext::new(raw_ctx, dpr, css_w, css_h);

    let now_ms = win
        .performance()
        .map(|p| p.now() as u64)
        .unwrap_or(0);

    // ── Perf sample (fps + frame_time) — same EMA as mlc-app-shell-vello ──────
    // Only fields the browser can honestly give us: fps + frame_time from
    // `performance.now()` deltas, RAM via performance.memory (Chromium-only,
    // best-effort). Everything sysinfo-only (per-core CPU, RAM total, GPU
    // name/driver) stays 0 / empty under wasm — the Monitoring panel already
    // no-ops those tiles when the source is absent.
    let now_hires = win.performance().map(|p| p.now()).unwrap_or(0.0);
    {
        let mut st = state.borrow_mut();
        if let Some(prev) = st.last_render_ms {
            let dt_ms = (now_hires - prev).max(0.001);
            let instant_fps = 1000.0 / dt_ms;
            st.fps_ema = st.fps_ema * 0.9 + instant_fps * 0.1;
            st.last_frame_time_ms = dt_ms;
        }
        st.last_render_ms = Some(now_hires);

        // Write perf tiles only when the Monitoring panel is open.
        use mlc_app_sidebar_content::state::RightSidebarPanel;
        let monitoring_open = st.chart.sidebar_state.is_right_open()
            && st.chart.sidebar_state.right_panel == RightSidebarPanel::Monitoring;
        if monitoring_open {
            let fps = st.fps_ema;
            let frame_time_ms = st.last_frame_time_ms;
            // performance.memory is Chromium-only, non-standard, quantized to
            // 100KB — best-effort, browsers may return None.
            let ram_mb = js_sys::Reflect::get(&win.performance().unwrap(), &"memory".into())
                .ok()
                .and_then(|mem| js_sys::Reflect::get(&mem, &"usedJSHeapSize".into()).ok())
                .and_then(|v| v.as_f64())
                .map(|b| b / (1024.0 * 1024.0))
                .unwrap_or(0.0);

            // Cross-cutting counts (same source as mlc-app-shell-vello).
            let ws_connections = st.bridge.ws_task_count_total();
            let active_connectors = st.chart.core.agent_metrics_snapshot().len();
            let lag_events = st.chart.lag_event_count;
            let total_bars: usize = st.chart.panel_app.panel_grid.containers()
                .values()
                .map(|w| st.chart.core.bubbles().bars(w.current_tag_id).len())
                .sum();
            // Chart-app internal profiling — same fields shell-vello reads.
            let tick_us = st.chart.last_tick_us;
            let event_process_us = st.chart.last_event_process_us;
            let auto_scale_us = st.chart.last_auto_scale_us;
            let indicator_recalc_us = st.chart.last_indicator_recalc_us;
            let indicator_recalc_count = st.chart.core.indicators().count() as u32;
            let (indicator_incremental_count, indicator_full_count) =
                st.chart.core.indicators().recalc_stats();
            let orderbook_panel_us = st.chart.last_orderbook_panel_us;
            let dom_panel_us = st.chart.last_dom_panel_us;
            let l2_panel_us = st.chart.last_l2_panel_us;
            let heatmap_panel_us = st.chart.last_heatmap_panel_us;
            let trade_panel_us = st.chart.last_trade_panel_us;
            let bar_apply_us = st.chart.last_bar_apply_us;
            let ob_event_count = st.chart.last_ob_event_count;
            let trade_event_count = st.chart.last_trade_event_count;
            let composite_us = st.chart.last_composite_us;

            let perf = &mut st.chart.sidebar_state.performance_data;
            perf.fps = fps;
            perf.frame_time_ms = frame_time_ms;
            perf.ram_mb = ram_mb;
            perf.window_count = 1; // wasm shell is single-window by design
            perf.total_bars = total_bars;
            perf.ws_connections = ws_connections;
            perf.active_connectors = active_connectors;
            perf.lag_events = lag_events;
            perf.tick_us = tick_us;
            perf.event_process_us = event_process_us;
            perf.auto_scale_us = auto_scale_us;
            perf.indicator_recalc_us = indicator_recalc_us;
            perf.indicator_recalc_count = indicator_recalc_count;
            perf.indicator_incremental_count = indicator_incremental_count;
            perf.indicator_full_count = indicator_full_count;
            perf.orderbook_panel_us = orderbook_panel_us;
            perf.dom_panel_us = dom_panel_us;
            perf.l2_panel_us = l2_panel_us;
            perf.heatmap_panel_us = heatmap_panel_us;
            perf.trade_panel_us = trade_panel_us;
            perf.bar_apply_us = bar_apply_us;
            perf.ob_event_count = ob_event_count;
            perf.trade_event_count = trade_event_count;
            perf.composite_us = composite_us;
            perf.render_backend = mlc_app_sidebar_content::state::RenderBackend::Canvas2d;
            // Sysinfo-only fields (per_core_cpu, gpu_name/driver, ram_total_mb,
            // system CPU, process CPU) stay at their defaults — browsers do
            // not expose those. msaa_samples, fps_limit, scene_build_us,
            // gpu_render_us also stay at defaults (no wgpu/native-render
            // pipeline in canvas2d).
        }
    }

    // ── Tab sync (mirror of scene_builder.rs:63-94 for the web shell) ─────────
    {
        let mut st = state.borrow_mut();
        let open_tabs = &st.chart.panel_app.open_tabs;
        let active_id = st.chart.panel_app.active_preset_id.clone();
        let presets_guard = st.chart.core.presets();
        let mut tabs: Vec<mlc_app_input_shell::Tab> = open_tabs
            .iter()
            .filter_map(|tab_id| {
                presets_guard.get(tab_id).map(|p| mlc_app_input_shell::Tab {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    active: p.id == active_id,
                })
            })
            .collect();
        drop(presets_guard);
        if tabs.is_empty() {
            tabs.push(mlc_app_input_shell::Tab {
                id: "__fallback__".to_string(),
                name: "Untitled".to_string(),
                active: true,
            });
        } else if !tabs.iter().any(|t| t.active) {
            if let Some(first) = tabs.first_mut() {
                first.active = true;
            }
        }
        st.chrome.tabs = tabs;
    }

    // ── Measure tab widths ────────────────────────────────────────────────────
    mlc_app_input_shell::update_tab_widths(&mut ctx, &mut state.borrow_mut().chrome);

    // ── Chart render ──────────────────────────────────────────────────────────
    // Translate by CHROME_HEIGHT so chart's local (0,0) is just below the chrome
    // strip — mirrors native scene_builder.rs:131-138 (chart_ctx y_offset =
    // CHROME_HEIGHT). Dispatch already subtracts CHROME_HEIGHT from mouse Y
    // (via `input.chrome_height`) so click coords stay consistent.
    //
    // render_modal_layer is rendered immediately after — it paints toolbar
    // dropdowns, the preset-name dialog, indicator/symbol search modals, the
    // settings modal, etc. Without it ALL modals are state-only: the flags
    // flip but zero pixels paint. Native composites these via
    // scene_builder.rs:280-284 with the same CHROME_HEIGHT offset.
    {
        use uzor::render::Painter;
        ctx.save();
        ctx.translate(0.0, CHROME_HEIGHT);
        {
            let mut st = state.borrow_mut();
            st.chart.render(&mut ctx, now_ms, false);
            // Overlay pass (crosshair, cursor labels, drawing previews) — a
            // separate draw call by contract (native invokes it via its own
            // overlay_scene, scene_builder.rs:186); canvas2d is immediate-mode
            // so painting into the same ctx right after the base pass is
            // equivalent compositing.
            st.chart.render_overlay(&mut ctx);
            st.chart.render_modal_layer(&mut ctx, now_ms);
        }
        ctx.restore();
    }

    // ── Chrome render ─────────────────────────────────────────────────────────
    {
        let st = state.borrow();
        mlc_app_input_shell::render(&mut ctx, &st.chrome, css_w as f64, false);
        mlc_app_input_shell::render_tooltip(&mut ctx, &st.chrome, css_w as f64, css_h as f64);
    }

    Ok(())
}

// ─── Screenshot via canvas.toBlob → navigator.clipboard.write ────────────────

/// Copies the current canvas contents as a PNG to the OS clipboard.
///
/// Flow: `canvas.to_blob_with_type("image/png")` → callback receives `Blob` →
/// `ClipboardItem({"image/png": blob})` → `navigator.clipboard.write([item])`.
///
/// The `Closure` is stored in an `Rc<RefCell<Option<…>>>` so the JS GC cannot
/// collect it before the callback fires, then it self-destructs.
#[cfg(target_arch = "wasm32")]
fn trigger_screenshot_to_clipboard(canvas_id: &str) {
    use wasm_bindgen::JsCast;

    let canvas = match get_canvas(canvas_id) {
        Some(c) => c,
        None => {
            web_sys::console::warn_1(&"[screenshot] canvas not found".into());
            return;
        }
    };

    // Keep the closure alive until the browser calls it once, then self-drop.
    let closure_slot: Rc<RefCell<Option<Closure<dyn FnMut(web_sys::Blob)>>>> =
        Rc::new(RefCell::new(None));
    let closure_slot_inner = closure_slot.clone();

    let cb = Closure::once(move |blob: web_sys::Blob| {
        let _ = closure_slot_inner.borrow_mut().take();

        let result: Result<(), JsValue> = (|| {
            // Build ClipboardItem: { "image/png": blob }
            let record = js_sys::Object::new();
            js_sys::Reflect::set(&record, &"image/png".into(), &blob)?;
            let item =
                web_sys::ClipboardItem::new_with_record_from_str_to_blob_promise(&record)?;
            let items = js_sys::Array::of1(&item);

            let win = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
            let clipboard = win.navigator().clipboard();
            let promise = clipboard.write(items.as_ref());

            // Drive the promise to completion in a detached microtask.
            wasm_bindgen_futures::spawn_local(async move {
                if let Err(e) = wasm_bindgen_futures::JsFuture::from(promise).await {
                    web_sys::console::error_1(
                        &format!("[screenshot] clipboard.write failed: {:?}", e).into(),
                    );
                } else {
                    web_sys::console::log_1(&"[screenshot] copied to clipboard".into());
                }
            });

            Ok(())
        })();

        if let Err(e) = result {
            web_sys::console::error_1(
                &format!("[screenshot] clipboard setup failed: {:?}", e).into(),
            );
        }
    });

    *closure_slot.borrow_mut() = Some(cb);

    let cb_ref = closure_slot
        .borrow()
        .as_ref()
        .map(|c| c.as_ref().unchecked_ref::<js_sys::Function>().clone());

    if let Some(cb_fn) = cb_ref {
        if let Err(e) = canvas.to_blob_with_type(&cb_fn, "image/png") {
            web_sys::console::error_1(
                &format!("[screenshot] to_blob_with_type failed: {:?}", e).into(),
            );
            let _ = closure_slot.borrow_mut().take();
        }
    }
}
