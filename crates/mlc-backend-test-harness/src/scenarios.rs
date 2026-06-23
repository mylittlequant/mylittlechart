//! Test scenarios. Each returns `Ok(())` or a `HarnessError` with the
//! specific assertion that failed. Public for reuse by the runner binary and
//! by any other consumer that wants to embed a subset.
//!
//! On wasm32 the **bodies** (`body_*` functions taking `&mut BackendHarness`)
//! are reusable from `wasm_bindgen_test` drivers. The sync wrappers and async
//! HTTP / daemon scenarios are native-only because they spawn external
//! processes (the daemon binary) or depend on `tokio::runtime::Builder` which
//! is unavailable on wasm.

use digdigdig3::{AccountType, ExchangeId};
use mlc_backend_core::BubbleEvent;
use mlc_core::state::Timeframe;
use mlc_core::tag_manager::BubbleId;
use mlc_core::types::Bar;
use mlc_live_data::LiveUpdate;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use mlc_core::state::chart_container_leaf::ChartPanelId;
#[cfg(not(target_arch = "wasm32"))]
use crate::DaemonGuard;

use crate::{BackendHarness, HarnessError, HarnessResult};

fn make_bar(ts: i64, close: f64) -> Bar {
    Bar {
        timestamp: ts,
        open: close,
        high: close,
        low: close,
        close,
        volume: 1.0,
    }
}

fn err(scenario: &'static str, message: impl Into<String>) -> HarnessError {
    HarnessError {
        scenario,
        message: message.into(),
    }
}

// ==========================================================================
// 1. Smoke: BackendCore::new survives, tick_bridge_only runs idempotently
// ==========================================================================

#[cfg(not(target_arch = "wasm32"))]
pub fn smoke_backend_alive() -> HarnessResult<()> {
    let h = BackendHarness::fresh();
    for i in 0..100 {
        h.core.tick_bridge_only((i as u64) * 16);
    }
    if h.core.sessions_snapshot().len() != 0 {
        return Err(err("smoke", "fresh backend should have zero sessions"));
    }
    Ok(())
}

// ==========================================================================
// 2. Data plane: inject BarsLoaded against a registered bubble → bubble.bars
// populated + BarsLoaded event with correct bid emitted
// ==========================================================================

#[cfg(not(target_arch = "wasm32"))]
pub fn data_plane_bars_loaded() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();

    // Register a bubble for BTCUSDT spot on Binance.
    let bid = {
        let mut mgr = h.core.bubble_mut();
        let bid = mgr
            .bubble
            .create_group_auto([1.0, 0.0, 0.0, 1.0], "BTCUSDT".to_string(), Timeframe::h1());
        if let Some(b) = mgr.bubble.bubble_mut(bid) {
            // bubbles_for_instrument matches on the lowercase id string and the
            // short-label account type — these are what tick_writers passes in.
            b.exchange = ExchangeId::Binance.as_str().to_string();
            b.account_type = AccountType::Spot.short_label().to_string();
        }
        bid
    };

    let bars: Vec<Bar> = (0..3).map(|i| make_bar(1_700_000_000 + i * 3600, 50_000.0 + (i as f64))).collect();
    h.inject(LiveUpdate::BarsLoaded {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        timeframe: "1h".to_string(),
        bars: bars.clone(),
    })?;

    let events = h.tick(16);

    let matched = events.iter().filter_map(|e| match e {
        BubbleEvent::BarsLoaded { bid: ev_bid, bars: ev_bars, .. } => Some((*ev_bid, ev_bars.len())),
        _ => None,
    }).collect::<Vec<_>>();
    if matched.len() != 1 {
        return Err(err("data-plane", format!("expected exactly 1 BarsLoaded event, got {}", matched.len())));
    }
    if matched[0].0 != bid {
        return Err(err("data-plane", format!("event bid mismatch: emitted={:?} expected={:?}", matched[0].0, bid)));
    }
    if matched[0].1 != bars.len() {
        return Err(err("data-plane", format!("event bar count: {} != {}", matched[0].1, bars.len())));
    }

    // Verify bubble actually got the bars written.
    let store_bars_len = h.core
        .bubble()
        .bubble
        .bubble(bid)
        .map(|b| b.bars.len())
        .unwrap_or(0);
    if store_bars_len != bars.len() {
        return Err(err("data-plane", format!("bubble.bars: stored {} != injected {}", store_bars_len, bars.len())));
    }
    Ok(())
}

// ==========================================================================
// 3. Data plane: BarsLoaded with NO registered bubble — must still emit
// BarsLoaded event with bid=0 sentinel and not panic
// ==========================================================================

#[cfg(not(target_arch = "wasm32"))]
pub fn data_plane_bars_no_bubble() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    h.inject(LiveUpdate::BarsLoaded {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "ETHUSDT".to_string(),
        timeframe: "1h".to_string(),
        bars: vec![make_bar(1_700_000_000, 3_000.0)],
    })?;
    let events = h.tick(16);
    let saw_zero = events.iter().any(|e| matches!(e, BubbleEvent::BarsLoaded { bid: BubbleId(0), .. }));
    if !saw_zero {
        return Err(err("no-bubble", "expected BarsLoaded with bid=0 sentinel when no bubble registered"));
    }
    Ok(())
}

// ==========================================================================
// 4. Pass-through arms: BarUpdate produces BarUpdatePassthrough; backend
// itself does NOT touch the bubble (per-app reactor's job)
// ==========================================================================

#[cfg(not(target_arch = "wasm32"))]
pub fn data_plane_pass_through_bar_update() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    h.inject(LiveUpdate::BarUpdate {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        bar: make_bar(1_700_000_000, 50_000.0),
    })?;
    let events = h.tick(16);
    let n = events.iter().filter(|e| matches!(e, BubbleEvent::BarUpdatePassthrough { .. })).count();
    if n != 1 {
        return Err(err("pass-through", format!("BarUpdatePassthrough count: {}", n)));
    }
    Ok(())
}

// ==========================================================================
// 5. Mini-ticker cache write — DashMap entry created, MiniTickerUpdated event
// emitted
// ==========================================================================

#[cfg(not(target_arch = "wasm32"))]
pub fn mini_ticker_cache() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    h.inject(LiveUpdate::MiniTickerUpdate {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        last_price: 51_234.5,
        price_change_percent: Some(1.2),
        high_price: Some(52_000.0),
        low_price: Some(50_000.0),
        volume: Some(123.4),
    })?;
    let events = h.tick(16);
    let saw_event = events.iter().any(|e| matches!(e, BubbleEvent::MiniTickerUpdated { .. }));
    if !saw_event {
        return Err(err("mini-ticker", "MiniTickerUpdated event not emitted"));
    }
    let key = "BTCUSDT:binance:S";
    let entry = h.core.mini_ticker.get(key);
    let Some(entry) = entry else {
        return Err(err("mini-ticker", format!("cache key {key} not present")));
    };
    if (entry.last_price - 51_234.5).abs() > 0.001 {
        return Err(err("mini-ticker", format!("last_price: {} != 51234.5", entry.last_price)));
    }
    if (entry.price_change_percent - 1.2).abs() > 0.001 {
        return Err(err("mini-ticker", format!("price_change_percent: {} != 1.2", entry.price_change_percent)));
    }
    Ok(())
}

// ==========================================================================
// 6. Connector lifecycle: ConnectorReady passes through
// ==========================================================================

#[cfg(not(target_arch = "wasm32"))]
pub fn connector_ready_event() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    h.inject(LiveUpdate::ConnectorReady {
        exchange_id: ExchangeId::Binance,
    })?;
    let events = h.tick(16);
    if !events.iter().any(|e| matches!(e, BubbleEvent::ConnectorReady { .. })) {
        return Err(err("connector-ready", "no ConnectorReady event"));
    }
    Ok(())
}

// ==========================================================================
// 7. SessionRegistry: register → list → heartbeat → reap_idle → deregister
// ==========================================================================

#[cfg(not(target_arch = "wasm32"))]
pub fn session_registry_lifecycle() -> HarnessResult<()> {
    let h = BackendHarness::fresh();
    let id_a = h.core.register_session(Some("alpha".to_string()), 1_000);
    let id_b = h.core.register_session(Some("beta".to_string()), 1_000);
    if id_a == id_b {
        return Err(err("sessions", "register_session produced duplicate id"));
    }

    let snap = h.core.sessions_snapshot();
    if snap.len() != 2 {
        return Err(err("sessions", format!("snapshot size: {} != 2", snap.len())));
    }

    // Heartbeat just one — bring alpha's last_heartbeat close to the reap time.
    if !h.core.heartbeat_session(&id_a, 1_990) {
        return Err(err("sessions", "heartbeat on existing id returned false"));
    }
    if h.core.heartbeat_session("nonexistent", 1_990) {
        return Err(err("sessions", "heartbeat on missing id returned true"));
    }

    // Reap at t=2000, idle threshold 60s.
    //   alpha: 2000-1990 = 10s stale → KEEP
    //   beta:  2000-1000 = 1000s stale → DROP
    let reaped = h.core.reap_idle_sessions(2_000, 60);
    if reaped != 1 {
        return Err(err("sessions", format!("reaped {} sessions, expected 1", reaped)));
    }

    let snap = h.core.sessions_snapshot();
    if snap.len() != 1 || snap[0].app_id != id_a {
        return Err(err("sessions", format!("expected only alpha (id_a={}) left after reap; got {} sessions", id_a, snap.len())));
    }

    if !h.core.deregister_session(&id_a) {
        return Err(err("sessions", "deregister of existing id returned false"));
    }
    if h.core.sessions_snapshot().len() != 0 {
        return Err(err("sessions", "deregister did not drop session"));
    }

    Ok(())
}

// ==========================================================================
// 8. DrawingRegistry: per-ChartPanelId insert/get/remove, drain_matching
// ==========================================================================

#[cfg(not(target_arch = "wasm32"))]
pub fn drawing_registry_basic() -> HarnessResult<()> {
    let h = BackendHarness::fresh();

    let panel_a = ChartPanelId(101);
    let panel_b = ChartPanelId(202);

    {
        let mut reg = h.core.drawings_mut();
        let _ = reg.entry_or_default(panel_a);
        let _ = reg.entry_or_default(panel_b);
    }

    {
        let reg = h.core.drawings();
        if reg.get(panel_a).is_none() {
            return Err(err("drawing-reg", "panel_a entry missing after entry_or_default"));
        }
        if reg.get(panel_b).is_none() {
            return Err(err("drawing-reg", "panel_b entry missing after entry_or_default"));
        }
        let count = reg.iter().count();
        if count != 2 {
            return Err(err("drawing-reg", format!("iter count: {} != 2", count)));
        }
    }

    // Remove panel_a only.
    {
        let mut reg = h.core.drawings_mut();
        let removed = reg.remove(panel_a);
        if removed.is_none() {
            return Err(err("drawing-reg", "remove returned None on present key"));
        }
    }
    {
        let reg = h.core.drawings();
        if reg.get(panel_a).is_some() {
            return Err(err("drawing-reg", "panel_a still present after remove"));
        }
        if reg.get(panel_b).is_none() {
            return Err(err("drawing-reg", "panel_b dropped by panel_a remove (cross-talk)"));
        }
    }

    // drain_matching — drop everything whose id > 200.
    {
        let mut reg = h.core.drawings_mut();
        reg.drain_matching(|pid| pid.0 > 200);
        if reg.get(panel_b).is_some() {
            return Err(err("drawing-reg", "drain_matching did not drop panel_b (id=202)"));
        }
    }

    Ok(())
}

// ==========================================================================
// 9. IndicatorManager AtomicU64 round-trip
// ==========================================================================

#[cfg(not(target_arch = "wasm32"))]
pub fn indicator_atomic_window_id() -> HarnessResult<()> {
    use std::sync::atomic::Ordering;

    fn decode(raw: u64) -> Option<u64> {
        match raw { 0 => None, n => Some(n - 1) }
    }

    let h = BackendHarness::fresh();
    {
        let im = h.core.indicators();
        let raw = im.current_render_window_id.load(Ordering::Relaxed);
        if decode(raw).is_some() {
            return Err(err("indicator-atomic", format!("fresh manager has current_render_window_id raw={raw}")));
        }
    }
    {
        let im = h.core.indicators();
        // store: encode Some(7) as 8
        im.current_render_window_id.store(7 + 1, Ordering::Relaxed);
    }
    {
        let im = h.core.indicators();
        let raw = im.current_render_window_id.load(Ordering::Relaxed);
        if decode(raw) != Some(7) {
            return Err(err("indicator-atomic", format!("after store Some(7): raw={raw} decoded={:?}", decode(raw))));
        }
    }
    {
        let im = h.core.indicators();
        im.current_render_window_id.store(0, Ordering::Relaxed);
    }
    {
        let im = h.core.indicators();
        if im.current_render_window_id.load(Ordering::Relaxed) != 0 {
            return Err(err("indicator-atomic", "after store(0): still nonzero"));
        }
    }
    Ok(())
}

// ==========================================================================
// 10. Lock discipline: many parallel reads + one writer — no deadlock,
// correct value
// ==========================================================================

#[cfg(not(target_arch = "wasm32"))]
pub fn lock_discipline_concurrent_reads() -> HarnessResult<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc as StdArc;
    use std::thread;

    let h = BackendHarness::fresh();
    let core = h.core.clone();

    // Seed: register one session so reads have something to see.
    let id = core.register_session(Some("seed".to_string()), 0);

    let stop = StdArc::new(std::sync::atomic::AtomicBool::new(false));
    let read_count = StdArc::new(AtomicUsize::new(0));
    let write_count = StdArc::new(AtomicUsize::new(0));

    let mut readers = Vec::new();
    for _ in 0..8 {
        let core_r = core.clone();
        let stop_r = stop.clone();
        let read_count_r = read_count.clone();
        readers.push(thread::spawn(move || {
            while !stop_r.load(Ordering::Relaxed) {
                let _snap = core_r.sessions_snapshot();
                let _bubbles_n = core_r.bubble().bubble.bubbles().count();
                let _ind_w = core_r.indicators().current_render_window_id.load(std::sync::atomic::Ordering::Relaxed);
                read_count_r.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    let core_w = core.clone();
    let stop_w = stop.clone();
    let write_count_w = write_count.clone();
    let writer = thread::spawn(move || {
        let mut hb = 1u64;
        while !stop_w.load(Ordering::Relaxed) {
            core_w.heartbeat_session(&id, hb);
            hb += 1;
            write_count_w.fetch_add(1, Ordering::Relaxed);
        }
    });

    thread::sleep(Duration::from_millis(150));
    stop.store(true, Ordering::Relaxed);
    for r in readers {
        r.join().map_err(|_| err("lock-discipline", "reader thread panicked"))?;
    }
    writer.join().map_err(|_| err("lock-discipline", "writer thread panicked"))?;

    let r = read_count.load(Ordering::Relaxed);
    let w = write_count.load(Ordering::Relaxed);
    if r == 0 {
        return Err(err("lock-discipline", "0 reads completed under contention"));
    }
    if w == 0 {
        return Err(err("lock-discipline", "0 writes completed under contention"));
    }
    Ok(())
}

// ==========================================================================
// 11. AgentState /api/v1/apps surface — without core attached returns 503;
// with core attached returns 201 / 200 / 204
// ==========================================================================

/// Spin a multi-threaded runtime for the agent server in a parked worker
/// thread and return a `&'static` borrow. We can't call `Runtime::new()` from
/// inside `block_on` (nested-runtime panic), so a parked thread owns it. The
/// runtime is `Send`, so the channel hand-off is safe. Returned as `&'static`
/// (via `Box::leak`) because dropping a `Runtime` from within an async
/// context itself panics — leaking for the lifetime of the test process is
/// the cheapest correct option.
#[cfg(not(target_arch = "wasm32"))]
fn spawn_server_runtime() -> HarnessResult<&'static tokio::runtime::Runtime> {
    let (tx, rx) = std::sync::mpsc::channel::<Result<tokio::runtime::Runtime, String>>();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .build()
            .map_err(|e| format!("build runtime: {e}"));
        let _ = tx.send(rt);
        loop {
            std::thread::park();
        }
    });
    let rt = rx
        .recv()
        .map_err(|e| err("spawn-server-rt", format!("recv: {e}")))?
        .map_err(|e| err("spawn-server-rt", e))?;
    Ok(Box::leak(Box::new(rt)))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn agent_state_apps_with_and_without_core(h: BackendHarness) -> HarnessResult<()> {
    use mlc_agentic_server::AgentState;
    use std::sync::Arc as StdArc;

    // Phase A: AgentState WITHOUT core attached.
    let state_no_core = StdArc::new(AgentState::new(h.bridge.clone(), "harness".to_string()));
    let port_a = pick_free_port()?;
    let rt = spawn_server_runtime()?;
    let _handle_a = mlc_agentic_server::start_server(state_no_core.clone(), rt.handle(), port_a);
    wait_for_health(port_a, "/api/v1/health").await?;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{port_a}/api/v1/apps"))
        .json(&serde_json::json!({ "label": "no-core" }))
        .send()
        .await
        .map_err(|e| err("agent-apps", format!("POST no-core: {e}")))?;
    if resp.status().as_u16() != 503 {
        return Err(err("agent-apps", format!("expected 503 without core, got {}", resp.status())));
    }

    // Phase B: attach a core; subsequent requests succeed.
    state_no_core.attach_core(h.core.clone());

    let resp = client
        .post(format!("http://127.0.0.1:{port_a}/api/v1/apps"))
        .json(&serde_json::json!({ "label": "with-core" }))
        .send()
        .await
        .map_err(|e| err("agent-apps", format!("POST with-core: {e}")))?;
    if resp.status().as_u16() != 201 {
        return Err(err("agent-apps", format!("expected 201 with core, got {}", resp.status())));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| err("agent-apps", format!("json: {e}")))?;
    let app_id = body
        .get("app_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| err("agent-apps", format!("missing app_id in {body:?}")))?
        .to_string();

    // List
    let resp = client
        .get(format!("http://127.0.0.1:{port_a}/api/v1/apps"))
        .send()
        .await
        .map_err(|e| err("agent-apps", format!("GET: {e}")))?;
    if !resp.status().is_success() {
        return Err(err("agent-apps", format!("GET status: {}", resp.status())));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| err("agent-apps", format!("list json: {e}")))?;
    let apps = body.get("apps").and_then(|v| v.as_array()).ok_or_else(|| err("agent-apps", "no apps array"))?;
    if apps.is_empty() {
        return Err(err("agent-apps", "apps list empty after register"));
    }

    // Heartbeat
    let resp = client
        .post(format!("http://127.0.0.1:{port_a}/api/v1/apps/{app_id}/heartbeat"))
        .send()
        .await
        .map_err(|e| err("agent-apps", format!("heartbeat: {e}")))?;
    if resp.status().as_u16() != 200 {
        return Err(err("agent-apps", format!("heartbeat status: {}", resp.status())));
    }

    // Deregister
    let resp = client
        .delete(format!("http://127.0.0.1:{port_a}/api/v1/apps/{app_id}"))
        .send()
        .await
        .map_err(|e| err("agent-apps", format!("delete: {e}")))?;
    if resp.status().as_u16() != 204 {
        return Err(err("agent-apps", format!("delete status: {}", resp.status())));
    }

    // Already gone — second delete is 404.
    let resp = client
        .delete(format!("http://127.0.0.1:{port_a}/api/v1/apps/{app_id}"))
        .send()
        .await
        .map_err(|e| err("agent-apps", format!("delete2: {e}")))?;
    if resp.status().as_u16() != 404 {
        return Err(err("agent-apps", format!("second delete status: {}", resp.status())));
    }

    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_free_port() -> HarnessResult<u16> {
    let l = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| err("port-pick", format!("bind: {e}")))?;
    let port = l.local_addr().map_err(|e| err("port-pick", format!("addr: {e}")))?.port();
    drop(l);
    Ok(port)
}

#[cfg(not(target_arch = "wasm32"))]
async fn wait_for_health(port: u16, path: &str) -> HarnessResult<()> {
    let url = format!("http://127.0.0.1:{port}{path}");
    let client = reqwest::Client::new();
    let deadline = web_time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(r) = client.get(&url).send().await {
            if r.status().is_success() {
                return Ok(());
            }
        }
        if web_time::Instant::now() > deadline {
            return Err(err("wait-health", format!("{url} never returned 2xx within 5s")));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// ==========================================================================
// 12. Mode-2 daemon e2e: spawn binary, /health works, /v1/apps round-trips
// ==========================================================================

#[cfg(not(target_arch = "wasm32"))]
pub async fn daemon_e2e() -> HarnessResult<()> {
    let port = pick_free_port()?;
    let _guard = DaemonGuard::spawn_with_port(port).await?;
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();

    let h = client.get(format!("{base}/health")).send().await.map_err(|e| err("daemon-e2e", format!("health: {e}")))?;
    if !h.status().is_success() {
        return Err(err("daemon-e2e", format!("/health status: {}", h.status())));
    }
    let h_body: serde_json::Value = h.json().await.map_err(|e| err("daemon-e2e", format!("health json: {e}")))?;
    if h_body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return Err(err("daemon-e2e", format!("/health ok != true: {h_body:?}")));
    }

    // Register a session via the daemon, list it, heartbeat, deregister.
    let r = client.post(format!("{base}/v1/apps")).json(&serde_json::json!({"label":"daemon-e2e"})).send().await.map_err(|e| err("daemon-e2e", format!("register: {e}")))?;
    if r.status().as_u16() != 201 {
        return Err(err("daemon-e2e", format!("register: {}", r.status())));
    }
    let app_id = r.json::<serde_json::Value>().await.map_err(|e| err("daemon-e2e", format!("register json: {e}")))?
        .get("app_id").and_then(|v| v.as_str()).map(str::to_string)
        .ok_or_else(|| err("daemon-e2e", "no app_id in register response"))?;

    let r = client.get(format!("{base}/v1/apps")).send().await.map_err(|e| err("daemon-e2e", format!("list: {e}")))?;
    if !r.status().is_success() {
        return Err(err("daemon-e2e", format!("list: {}", r.status())));
    }
    let list: serde_json::Value = r.json().await.map_err(|e| err("daemon-e2e", format!("list json: {e}")))?;
    let apps = list.get("apps").and_then(|v| v.as_array()).ok_or_else(|| err("daemon-e2e", "no apps array"))?;
    if !apps.iter().any(|a| a.get("app_id").and_then(|v| v.as_str()) == Some(app_id.as_str())) {
        return Err(err("daemon-e2e", format!("listed apps did not contain {app_id}")));
    }

    let r = client.post(format!("{base}/v1/apps/{app_id}/heartbeat")).send().await.map_err(|e| err("daemon-e2e", format!("hb: {e}")))?;
    if r.status().as_u16() != 200 {
        return Err(err("daemon-e2e", format!("hb: {}", r.status())));
    }

    let r = client.delete(format!("{base}/v1/apps/{app_id}")).send().await.map_err(|e| err("daemon-e2e", format!("delete: {e}")))?;
    if r.status().as_u16() != 204 {
        return Err(err("daemon-e2e", format!("delete: {}", r.status())));
    }

    // /v1/bubbles works against an empty backend (empty list, 200).
    let r = client.get(format!("{base}/v1/bubbles")).send().await.map_err(|e| err("daemon-e2e", format!("bubbles: {e}")))?;
    if !r.status().is_success() {
        return Err(err("daemon-e2e", format!("/v1/bubbles: {}", r.status())));
    }
    let body: serde_json::Value = r.json().await.map_err(|e| err("daemon-e2e", format!("bubbles json: {e}")))?;
    if body.get("bubble_ids").and_then(|v| v.as_array()).map(|a| a.len()) != Some(0) {
        return Err(err("daemon-e2e", format!("expected empty bubble_ids, got {body:?}")));
    }

    Ok(())
}

// ==========================================================================
// 13. Daemon extended endpoints e2e — exercises the wider control plane
// (bubbles CRUD, presets, templates, watchlists, drawings, indicators,
// mini_ticker, exchanges, /v1/apps/_reap, /v1/bridge/request-bars)
// ==========================================================================

#[cfg(not(target_arch = "wasm32"))]
pub async fn daemon_extended_e2e() -> HarnessResult<()> {
    let port = pick_free_port()?;
    let _guard = DaemonGuard::spawn_with_port(port).await?;
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();

    // Each non-2xx is a failure.
    let must_200 = |path: &'static str| {
        let url = format!("{base}{path}");
        let client = client.clone();
        async move {
            let r = client.get(&url).send().await.map_err(|e| err("daemon-ext", format!("GET {path}: {e}")))?;
            if !r.status().is_success() {
                return Err(err("daemon-ext", format!("GET {path}: HTTP {}", r.status())));
            }
            r.json::<serde_json::Value>().await.map_err(|e| err("daemon-ext", format!("decode {path}: {e}")))
        }
    };

    // 1. Empty-state reads must all return 200.
    for path in [
        "/v1/bubbles", "/v1/presets", "/v1/templates/summary",
        "/v1/watchlists", "/v1/drawings", "/v1/indicators",
        "/v1/mini-ticker", "/v1/exchanges", "/v1/trading/connectors",
    ] {
        let _ = must_200(path).await?;
    }

    // 2. Bubble CRUD: create → get → bars(empty) → delete → 404
    let r = client.post(format!("{base}/v1/bubbles"))
        .json(&serde_json::json!({
            "symbol": "BTCUSDT", "exchange": "binance",
            "account_type": "S", "timeframe": "1m"
        }))
        .send().await.map_err(|e| err("daemon-ext", format!("create: {e}")))?;
    if r.status().as_u16() != 201 {
        return Err(err("daemon-ext", format!("create bubble: HTTP {}", r.status())));
    }
    let body: serde_json::Value = r.json().await.map_err(|e| err("daemon-ext", format!("create json: {e}")))?;
    let bid = body.get("bid").and_then(|v| v.as_u64())
        .ok_or_else(|| err("daemon-ext", format!("no bid in create response: {body:?}")))?;

    let info_path: &'static str = Box::leak(format!("/v1/bubbles/{bid}").into_boxed_str());
    let info = must_200(info_path).await?;
    let info_sym = info.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
    if info_sym != "BTCUSDT" {
        return Err(err("daemon-ext", format!("bubble info symbol={info_sym}")));
    }
    let info_exch = info.get("exchange").and_then(|v| v.as_str()).unwrap_or("");
    if info_exch != "binance" {
        return Err(err("daemon-ext", format!("bubble info exchange={info_exch}")));
    }

    let bars_url = format!("{base}/v1/bubbles/{bid}/bars?limit=5");
    let r = client.get(&bars_url).send().await.map_err(|e| err("daemon-ext", format!("bars: {e}")))?;
    if !r.status().is_success() {
        return Err(err("daemon-ext", format!("/bubbles/{bid}/bars: HTTP {}", r.status())));
    }

    let r = client.delete(format!("{base}/v1/bubbles/{bid}"))
        .send().await.map_err(|e| err("daemon-ext", format!("delete: {e}")))?;
    if r.status().as_u16() != 204 {
        return Err(err("daemon-ext", format!("delete bubble: HTTP {}", r.status())));
    }

    let r = client.get(format!("{base}/v1/bubbles/{bid}"))
        .send().await.map_err(|e| err("daemon-ext", format!("post-delete: {e}")))?;
    if r.status().as_u16() != 404 {
        return Err(err("daemon-ext", format!("post-delete: expected 404, got {}", r.status())));
    }

    // 3. /v1/apps/_reap
    let id_a = client.post(format!("{base}/v1/apps"))
        .json(&serde_json::json!({"label":"reap-test"}))
        .send().await.map_err(|e| err("daemon-ext", format!("reg: {e}")))?
        .json::<serde_json::Value>().await
        .ok().and_then(|v| v.get("app_id").and_then(|x| x.as_str()).map(str::to_string))
        .ok_or_else(|| err("daemon-ext", "no app_id"))?;

    let r = client.post(format!("{base}/v1/apps/_reap"))
        .json(&serde_json::json!({"idle_secs": 0}))
        .send().await.map_err(|e| err("daemon-ext", format!("reap: {e}")))?;
    if !r.status().is_success() {
        return Err(err("daemon-ext", format!("reap: HTTP {}", r.status())));
    }
    let body: serde_json::Value = r.json().await.map_err(|e| err("daemon-ext", format!("reap json: {e}")))?;
    let reaped = body.get("reaped").and_then(|v| v.as_u64()).unwrap_or(0);
    if reaped < 1 {
        return Err(err("daemon-ext", format!("expected reaped>=1, got {reaped}")));
    }
    // Idempotency: the now-gone session can no longer be heartbeated.
    let r = client.post(format!("{base}/v1/apps/{id_a}/heartbeat"))
        .send().await.map_err(|e| err("daemon-ext", format!("post-reap hb: {e}")))?;
    if r.status().as_u16() != 404 {
        return Err(err("daemon-ext", format!("post-reap hb: expected 404, got {}", r.status())));
    }

    // 4. /v1/bridge/request-bars accepts the call (does NOT verify data arrives —
    // that's a network-dependent test; the binance fetch is exercised in the
    // manual e2e). We just verify the endpoint round-trips.
    let r = client.post(format!("{base}/v1/bridge/request-bars"))
        .json(&serde_json::json!({
            "symbol":"BTCUSDT","exchange":"binance",
            "account_type":"S","timeframe":"1m"
        }))
        .send().await.map_err(|e| err("daemon-ext", format!("req-bars: {e}")))?;
    if !r.status().is_success() {
        return Err(err("daemon-ext", format!("req-bars: HTTP {}", r.status())));
    }

    Ok(())
}

// ==========================================================================
// 14. CLI binary smoke — spawn daemon, run several `mlc` subcommands against
// it, verify exit codes + output shape
// ==========================================================================

#[cfg(not(target_arch = "wasm32"))]
pub async fn cli_binary_smoke() -> HarnessResult<()> {
    let port = pick_free_port()?;
    let _guard = DaemonGuard::spawn_with_port(port).await?;
    let remote = format!("http://127.0.0.1:{port}");

    let exe = locate_cli_binary()?;

    let run = |args: Vec<&'static str>| {
        let exe = exe.clone();
        let remote = remote.clone();
        async move {
            let mut full_args: Vec<String> = vec![
                "--remote".to_string(), remote.clone(),
                "--json".to_string(),
            ];
            for a in args { full_args.push(a.to_string()); }
            let out = tokio::process::Command::new(&exe)
                .args(&full_args)
                .output()
                .await
                .map_err(|e| err("cli-smoke", format!("spawn mlc {full_args:?}: {e}")))?;
            if !out.status.success() {
                return Err(err("cli-smoke", format!(
                    "mlc {full_args:?} exit={} stderr={}",
                    out.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&out.stderr),
                )));
            }
            Ok(String::from_utf8_lossy(&out.stdout).to_string())
        }
    };

    // mlc health — JSON must have ok=true
    let out = run(vec!["health"]).await?;
    let v: serde_json::Value = serde_json::from_str(out.trim()).map_err(|e| err("cli-smoke", format!("health decode: {e}\n{out}")))?;
    if v.get("ok").and_then(|x| x.as_bool()) != Some(true) {
        return Err(err("cli-smoke", format!("health ok != true: {v}")));
    }

    // mlc session register --label cli-smoke → returns app_id
    let out = run(vec!["session", "register", "--label", "cli-smoke"]).await?;
    let v: serde_json::Value = serde_json::from_str(out.trim()).map_err(|e| err("cli-smoke", format!("session register decode: {e}\n{out}")))?;
    let app_id = v.get("app_id").and_then(|x| x.as_str()).map(str::to_string)
        .ok_or_else(|| err("cli-smoke", format!("no app_id in: {v}")))?;

    // mlc session ls — must contain our app_id
    let out = run(vec!["session", "ls"]).await?;
    if !out.contains(&app_id) {
        return Err(err("cli-smoke", format!("session ls missing {app_id}: {out}")));
    }

    // mlc bubble create BTCUSDT
    let out = run(vec!["bubble", "create", "BTCUSDT", "--tf", "1m"]).await?;
    let v: serde_json::Value = serde_json::from_str(out.trim()).map_err(|e| err("cli-smoke", format!("bubble create decode: {e}\n{out}")))?;
    let bid = v.get("bid").and_then(|x| x.as_u64()).ok_or_else(|| err("cli-smoke", format!("no bid: {v}")))?;

    // mlc bubble ls — at least 1 entry
    let out = run(vec!["bubble", "ls"]).await?;
    let v: serde_json::Value = serde_json::from_str(out.trim()).map_err(|e| err("cli-smoke", format!("bubble ls decode: {e}\n{out}")))?;
    let count = v.get("bubble_ids").and_then(|x| x.as_array()).map(|a| a.len()).unwrap_or(0);
    if count < 1 {
        return Err(err("cli-smoke", format!("bubble ls empty after create: {v}")));
    }

    // Read existence path: this verifies /v1/bubbles/:bid wiring
    let bid_str: &'static str = Box::leak(bid.to_string().into_boxed_str());
    let _ = run(vec!["bubble", "info", bid_str]).await?;

    Ok(())
}

fn locate_cli_binary() -> HarnessResult<std::path::PathBuf> {
    let exe_name = if cfg!(windows) { "mlc.exe" } else { "mlc" };
    let cwd = std::env::current_dir().map_err(|e| err("cli-smoke", format!("cwd: {e}")))?;
    let debug = cwd.join("target/debug").join(exe_name);
    let release = cwd.join("target/release").join(exe_name);
    if debug.exists() { Ok(debug) }
    else if release.exists() { Ok(release) }
    else {
        Err(err("cli-smoke", format!(
            "mlc CLI binary not found at {debug:?} or {release:?} — run cargo build -p mylittlechart-cli first"
        )))
    }
}

// ==========================================================================
// Master list — runner iterates this. The async ones live in a parallel list.
// ==========================================================================

// ==========================================================================
// Stream coverage extension: every LiveUpdate variant the bridge emits in
// production. Each scenario injects a synthetic event, ticks the backend,
// and asserts the matching BubbleEvent (passthrough or write) lands.
// ==========================================================================

pub fn register_bubble(h: &BackendHarness, exchange: ExchangeId, account: AccountType, symbol: &str, tf: Timeframe) -> BubbleId {
    let mut mgr = h.core.bubble_mut();
    let bid = mgr.bubble.create_group_auto([1.0, 0.0, 0.0, 1.0], symbol.to_string(), tf);
    if let Some(b) = mgr.bubble.bubble_mut(bid) {
        b.exchange = exchange.as_str().to_string();
        b.account_type = account.short_label().to_string();
    }
    bid
}

// All extended scenarios are split into two layers:
//  - `body_*` takes `&mut BackendHarness` and runs the actual assertions.
//    Same body is reused by native sync runner and the wasm
//    `wasm_bindgen_test` driver (which builds the harness async).
//  - `pub fn <scenario>()` keeps the legacy sync API for the runner binary.
//    It just builds a fresh harness and delegates.

pub fn body_stream_backfill_complete_writes_bubble(h: &mut BackendHarness) -> HarnessResult<()> {
    let bid = register_bubble(h, ExchangeId::Binance, AccountType::Spot, "BTCUSDT", Timeframe::h1());
    let bars: Vec<Bar> = (0..5).map(|i| make_bar(1_700_000_000 + i * 3600, 50_000.0 + i as f64)).collect();
    h.inject(LiveUpdate::BackfillComplete {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        timeframe: "1h".to_string(),
        bars: bars.clone(),
    })?;
    let events = h.tick(16);
    let n = events.iter().filter(|e| matches!(e, BubbleEvent::BackfillCompleted { bid: ev_bid, .. } if *ev_bid == bid)).count();
    if n != 1 {
        return Err(err("backfill", format!("expected 1 BackfillCompleted, got {}", n)));
    }
    let stored = h.core.bubble().bubble.bubble(bid).map(|b| b.bars.len()).unwrap_or(0);
    if stored != bars.len() {
        return Err(err("backfill", format!("bubble.bars: {} != {}", stored, bars.len())));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn stream_backfill_complete_writes_bubble() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_stream_backfill_complete_writes_bubble(&mut h)
}

pub fn body_stream_scroll_bars_merges_with_live_tail(h: &mut BackendHarness) -> HarnessResult<()> {
    let bid = register_bubble(h, ExchangeId::Binance, AccountType::Spot, "BTCUSDT", Timeframe::h1());
    // Seed live tail: bars [1000..1010), the "right edge" the chart is showing.
    let live_tail: Vec<Bar> = (1000..1010).map(|i| make_bar(i * 3600, 50_000.0 + i as f64)).collect();
    h.inject(LiveUpdate::BarsLoaded {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        timeframe: "1h".to_string(),
        bars: live_tail.clone(),
    })?;
    let _ = h.tick(16);
    // Now simulate scroll-left: REST returned an OLDER page [990..1000).
    let older_page: Vec<Bar> = (990..1000).map(|i| make_bar(i * 3600, 49_000.0 + i as f64)).collect();
    h.inject(LiveUpdate::ScrollBarsLoaded {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        timeframe: "1h".to_string(),
        bars: older_page.clone(),
        prepend_count: older_page.len(),
    })?;
    let events = h.tick(16);
    let n = events.iter().filter(|e| matches!(e, BubbleEvent::ScrollBarsAppended { bid: ev_bid, .. } if *ev_bid == bid)).count();
    if n != 1 {
        return Err(err("scroll", format!("expected 1 ScrollBarsAppended, got {}", n)));
    }
    // The bubble must now hold BOTH ranges merged by timestamp (older + live tail),
    // not just the 10-bar scroll page. This is the regression fix from
    // backend-core ScrollBarsLoaded arm.
    let stored: Vec<Bar> = h.core.bubble().bubble.bubble(bid).map(|b| b.bars.clone()).unwrap_or_default();
    if stored.len() != older_page.len() + live_tail.len() {
        return Err(err("scroll", format!("merge count: {} != {}+{}", stored.len(), older_page.len(), live_tail.len())));
    }
    // First bar must be the OLDEST from the scroll page.
    if stored.first().map(|b| b.timestamp) != Some(older_page[0].timestamp) {
        return Err(err("scroll", "merge does not start with oldest scroll bar"));
    }
    // Last bar must be the live tail's last bar (preserved through merge).
    if stored.last().map(|b| b.timestamp) != Some(live_tail.last().unwrap().timestamp) {
        return Err(err("scroll", "merge dropped live-tail right edge"));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn stream_scroll_bars_merges_with_live_tail() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_stream_scroll_bars_merges_with_live_tail(&mut h)
}

pub fn body_stream_trade_passthrough(h: &mut BackendHarness) -> HarnessResult<()> {
    h.inject(LiveUpdate::TradeUpdate {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        price: 52_345.67,
        quantity: 0.001,
        timestamp: 1_700_000_000_000,
        is_buyer_maker: false,
    })?;
    let events = h.tick(16);
    let hit = events.iter().any(|e| matches!(e,
        BubbleEvent::TradeUpdatePassthrough { price, symbol, .. }
            if (*price - 52_345.67).abs() < 1e-6 && symbol == "BTCUSDT"));
    if !hit {
        return Err(err("trade", "no TradeUpdatePassthrough emitted"));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn stream_trade_passthrough() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_stream_trade_passthrough(&mut h)
}

pub fn body_stream_orderbook_snapshot_ws_emits_event(h: &mut BackendHarness) -> HarnessResult<()> {
    let bids = vec![(50_000.0, 1.0), (49_999.5, 2.0)];
    let asks = vec![(50_001.0, 1.0), (50_001.5, 2.0)];
    h.inject(LiveUpdate::OrderbookSnapshot {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        bids: bids.clone(),
        asks: asks.clone(),
        timestamp: 1_700_000_000_000,
        source: mlc_live_data::OrderbookSource::Rest,
    })?;
    let events = h.tick(16);
    // REST snapshot → backend emits OrderbookSnapshotRest (signals DOM rebuild).
    let hit = events.iter().any(|e| matches!(e,
        BubbleEvent::OrderbookSnapshotRest { symbol, .. } if symbol == "BTCUSDT"));
    if !hit {
        return Err(err("ob-snap", "no OrderbookSnapshotRest emitted"));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn stream_orderbook_snapshot_ws_emits_event() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_stream_orderbook_snapshot_ws_emits_event(&mut h)
}

pub fn body_stream_orderbook_delta_emits_event(h: &mut BackendHarness) -> HarnessResult<()> {
    let bids = vec![(50_000.0, 1.5)];
    let asks = vec![(50_001.0, 0.5)];
    h.inject(LiveUpdate::OrderbookDelta {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        bids: bids.clone(),
        asks: asks.clone(),
        timestamp: 1_700_000_000_500,
    })?;
    let events = h.tick(16);
    let hit = events.iter().any(|e| matches!(e,
        BubbleEvent::OrderbookDelta { symbol, bids: ev_b, asks: ev_a, .. }
            if symbol == "BTCUSDT" && ev_b.len() == bids.len() && ev_a.len() == asks.len()));
    if !hit {
        return Err(err("ob-delta", "no OrderbookDelta event"));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn stream_orderbook_delta_emits_event() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_stream_orderbook_delta_emits_event(&mut h)
}

pub fn body_stream_open_interest_passthrough(h: &mut BackendHarness) -> HarnessResult<()> {
    h.inject(LiveUpdate::OpenInterestUpdate {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::FuturesCross,
        symbol: "BTCUSDT".to_string(),
        point: mlc_core::types::OiDeltaBar { ts_ms: 1_700_000_000_000, oi: 12345.0, delta: 5.0 },
    })?;
    let events = h.tick(16);
    let hit = events.iter().any(|e| matches!(e,
        BubbleEvent::OiUpdatePassthrough { symbol, point, .. }
            if symbol == "BTCUSDT" && (point.oi - 12345.0).abs() < 1e-6));
    if !hit {
        return Err(err("oi", "no OiUpdatePassthrough emitted"));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn stream_open_interest_passthrough() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_stream_open_interest_passthrough(&mut h)
}

pub fn body_stream_funding_rate_passthrough(h: &mut BackendHarness) -> HarnessResult<()> {
    h.inject(LiveUpdate::FundingRateUpdate {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::FuturesCross,
        symbol: "BTCUSDT".to_string(),
        point: mlc_core::types::FundingRateCell { ts_ms: 1_700_000_000_000, rate: 0.0001, interval_hours: 8 },
    })?;
    let events = h.tick(16);
    let hit = events.iter().any(|e| matches!(e,
        BubbleEvent::FundingRatePassthrough { symbol, point, .. }
            if symbol == "BTCUSDT" && (point.rate - 0.0001).abs() < 1e-9));
    if !hit {
        return Err(err("funding", "no FundingRatePassthrough emitted"));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn stream_funding_rate_passthrough() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_stream_funding_rate_passthrough(&mut h)
}

pub fn body_stream_liquidation_event_passthrough(h: &mut BackendHarness) -> HarnessResult<()> {
    h.inject(LiveUpdate::LiquidationEvent {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::FuturesCross,
        symbol: "BTCUSDT".to_string(),
        ts_ms: 1_700_000_000_000,
        price: 50_000.0,
        value_usd: 250_000.0,
        side: 1,
    })?;
    let events = h.tick(16);
    let hit = events.iter().any(|e| matches!(e,
        BubbleEvent::LiquidationEventPassthrough { symbol, value_usd, side, .. }
            if symbol == "BTCUSDT" && (*value_usd - 250_000.0).abs() < 1e-3 && *side == 1));
    if !hit {
        return Err(err("liq", "no LiquidationEventPassthrough emitted"));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn stream_liquidation_event_passthrough() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_stream_liquidation_event_passthrough(&mut h)
}

pub fn body_stream_liquidation_bucket_passthrough(h: &mut BackendHarness) -> HarnessResult<()> {
    h.inject(LiveUpdate::LiquidationBucketUpdate {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::FuturesCross,
        symbol: "BTCUSDT".to_string(),
        ts_ms: 1_700_000_000_000,
        long_liq_usd: 100_000.0,
        short_liq_usd: 50_000.0,
    })?;
    let events = h.tick(16);
    let hit = events.iter().any(|e| matches!(e,
        BubbleEvent::LiquidationBucketPassthrough { symbol, long_liq_usd, short_liq_usd, .. }
            if symbol == "BTCUSDT"
            && (*long_liq_usd - 100_000.0).abs() < 1e-3
            && (*short_liq_usd - 50_000.0).abs() < 1e-3));
    if !hit {
        return Err(err("liq-bucket", "no LiquidationBucketPassthrough emitted"));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn stream_liquidation_bucket_passthrough() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_stream_liquidation_bucket_passthrough(&mut h)
}

pub fn body_stream_dom_heatmap_snapshot_passthrough(h: &mut BackendHarness) -> HarnessResult<()> {
    let bids = vec![(50_000.0, 1.0)];
    let asks = vec![(50_001.0, 1.0)];
    h.inject(LiveUpdate::DomHeatmapSnapshot {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        ts_ms: 1_700_000_000_000,
        bids,
        asks,
    })?;
    let events = h.tick(16);
    let hit = events.iter().any(|e| matches!(e,
        BubbleEvent::DomHeatmapSnapshotPassthrough { symbol, .. } if symbol == "BTCUSDT"));
    if !hit {
        return Err(err("dom-heat", "no DomHeatmapSnapshotPassthrough emitted"));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn stream_dom_heatmap_snapshot_passthrough() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_stream_dom_heatmap_snapshot_passthrough(&mut h)
}

// ==========================================================================
// Multi-symbol isolation: two independent bubbles, two BarsLoaded events,
// each must write into its OWN bubble (no cross-contamination).
// ==========================================================================

pub fn body_multi_symbol_bars_isolation(h: &mut BackendHarness) -> HarnessResult<()> {
    let bid_btc = register_bubble(h, ExchangeId::Binance, AccountType::Spot, "BTCUSDT", Timeframe::h1());
    let bid_eth = register_bubble(h, ExchangeId::Binance, AccountType::Spot, "ETHUSDT", Timeframe::h1());
    if bid_btc == bid_eth {
        return Err(err("multi-sym", "the two bubbles must have different ids"));
    }

    let bars_btc: Vec<Bar> = (0..3).map(|i| make_bar(1_700_000_000 + i * 3600, 50_000.0 + i as f64)).collect();
    let bars_eth: Vec<Bar> = (0..5).map(|i| make_bar(1_700_000_000 + i * 3600, 3_000.0 + i as f64)).collect();

    h.inject(LiveUpdate::BarsLoaded {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        timeframe: "1h".to_string(),
        bars: bars_btc.clone(),
    })?;
    h.inject(LiveUpdate::BarsLoaded {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "ETHUSDT".to_string(),
        timeframe: "1h".to_string(),
        bars: bars_eth.clone(),
    })?;
    let _ = h.tick(16);

    let btc_len = h.core.bubble().bubble.bubble(bid_btc).map(|b| b.bars.len()).unwrap_or(0);
    let eth_len = h.core.bubble().bubble.bubble(bid_eth).map(|b| b.bars.len()).unwrap_or(0);
    if btc_len != bars_btc.len() {
        return Err(err("multi-sym", format!("BTC bubble: {} != {}", btc_len, bars_btc.len())));
    }
    if eth_len != bars_eth.len() {
        return Err(err("multi-sym", format!("ETH bubble: {} != {}", eth_len, bars_eth.len())));
    }
    // Cross-check: BTC bubble's bars must be the BTC payload (prices 50k+),
    // not the ETH one (3k+).
    let btc_close0 = h.core.bubble().bubble.bubble(bid_btc).and_then(|b| b.bars.first().map(|x| x.close)).unwrap_or(0.0);
    if btc_close0 < 49_000.0 {
        return Err(err("multi-sym", format!("BTC bubble got wrong payload: close={}", btc_close0)));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn multi_symbol_bars_isolation() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_multi_symbol_bars_isolation(&mut h)
}

pub fn body_multi_symbol_trade_passthrough_independent(h: &mut BackendHarness) -> HarnessResult<()> {
    // Two trades on two different symbols arrive in the same tick window.
    h.inject(LiveUpdate::TradeUpdate {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        price: 50_000.0,
        quantity: 0.01,
        timestamp: 1_700_000_000_000,
        is_buyer_maker: false,
    })?;
    h.inject(LiveUpdate::TradeUpdate {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "ETHUSDT".to_string(),
        price: 3_000.0,
        quantity: 0.5,
        timestamp: 1_700_000_000_500,
        is_buyer_maker: true,
    })?;
    let events = h.tick(16);
    let btc = events.iter().any(|e| matches!(e,
        BubbleEvent::TradeUpdatePassthrough { symbol, price, .. }
            if symbol == "BTCUSDT" && (*price - 50_000.0).abs() < 1e-6));
    let eth = events.iter().any(|e| matches!(e,
        BubbleEvent::TradeUpdatePassthrough { symbol, price, .. }
            if symbol == "ETHUSDT" && (*price - 3_000.0).abs() < 1e-6));
    if !(btc && eth) {
        return Err(err("multi-sym-trade",
            format!("expected both BTC + ETH TradeUpdatePassthrough; btc={} eth={}", btc, eth)));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn multi_symbol_trade_passthrough_independent() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_multi_symbol_trade_passthrough_independent(&mut h)
}

pub fn body_multi_symbol_mini_ticker_cache_keys(h: &mut BackendHarness) -> HarnessResult<()> {
    for (sym, last) in [("BTCUSDT", 50_000.0_f64), ("ETHUSDT", 3_000.0_f64), ("SOLUSDT", 150.0_f64)] {
        h.inject(LiveUpdate::MiniTickerUpdate {
            exchange_id: ExchangeId::Binance,
            account_type: AccountType::Spot,
            symbol: sym.to_string(),
            last_price: last,
            price_change_percent: Some(0.1),
            high_price: Some(last * 1.01),
            low_price: Some(last * 0.99),
            volume: Some(100.0),
        })?;
    }
    let _ = h.tick(16);
    for (sym, expected) in [("BTCUSDT", 50_000.0_f64), ("ETHUSDT", 3_000.0_f64), ("SOLUSDT", 150.0_f64)] {
        let key = format!("{sym}:binance:S");
        let entry = h.core.mini_ticker.get(&key).map(|e| e.last_price);
        match entry {
            Some(v) if (v - expected).abs() < 1e-6 => {}
            Some(v) => return Err(err("multi-sym-ticker", format!("{sym}: cached {} != {}", v, expected))),
            None => return Err(err("multi-sym-ticker", format!("{sym}: cache key missing ({key})"))),
        }
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn multi_symbol_mini_ticker_cache_keys() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_multi_symbol_mini_ticker_cache_keys(&mut h)
}

// ==========================================================================
// Alert delivery: two alerts straddling the current tick price (±0.25%) —
// the next mini-ticker MUST trigger exactly one of them on the price-source
// alert path. Builds via AlertManager directly so we don't depend on the
// per-app reactor (this is the backend-level pre-condition for the per-app
// react.rs delivery check).
// ==========================================================================

// ==========================================================================
// Derived bars stream coverage: Footprint / Renko / PnF / Kagi / CVD / TPO /
// ThreeLineBreak. All seven derive from aggTrades upstream of the station,
// but at the broadcast layer they arrive as LiveUpdate::<Variant>{ point }
// — bridge already re-emits the station Event so we can inject directly.
// Each scenario asserts the matching *Appended BubbleEvent and verifies the
// per-derived store on the bubble actually grew.
// ==========================================================================

pub fn body_stream_footprint_update_writes_bubble(h: &mut BackendHarness) -> HarnessResult<()> {
    let bid = register_bubble(h, ExchangeId::Binance, AccountType::Spot, "BTCUSDT", Timeframe::h1());
    let point = digdigdig3_station::data::FootprintPoint {
        open_time: 1_700_000_000_000,
        open: 50_000.0, high: 50_050.0, low: 49_900.0, close: 50_025.0,
        volume: 12.5,
        // (price, buy_vol, sell_vol)
        levels: vec![
            (50_000.0, 1.5, 0.5),
            (50_025.0, 2.0, 1.0),
            (50_050.0, 0.5, 1.0),
        ],
    };
    h.inject(LiveUpdate::FootprintUpdate {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        point,
    })?;
    let events = h.tick(16);
    let hit = events.iter().any(|e| matches!(e,
        BubbleEvent::FootprintBarAppended { bid: ev_bid, .. } if *ev_bid == bid));
    if !hit {
        return Err(err("footprint", "no FootprintBarAppended emitted"));
    }
    let fp_len = h.core.bubble().bubble.bubble(bid)
        .and_then(|b| b.footprint_bars.as_ref().map(|v| v.len()))
        .unwrap_or(0);
    if fp_len != 1 {
        return Err(err("footprint", format!("bubble.footprint_bars: {} != 1", fp_len)));
    }
    // The arm also mirrors OHLC into bubble.bars so the standard candle layer
    // can render historical context — sanity check that grew too.
    let bars_len = h.core.bubble().bubble.bubble(bid).map(|b| b.bars.len()).unwrap_or(0);
    if bars_len != 1 {
        return Err(err("footprint", format!("bubble.bars mirror: {} != 1", bars_len)));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn stream_footprint_update_writes_bubble() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_stream_footprint_update_writes_bubble(&mut h)
}

pub fn body_stream_renko_brick_writes_bubble(h: &mut BackendHarness) -> HarnessResult<()> {
    let bid = register_bubble(h, ExchangeId::Binance, AccountType::Spot, "BTCUSDT", Timeframe::h1());
    let point = digdigdig3_station::data::RenkoBrickPoint {
        open_time: 1_700_000_000_000,
        bottom: 49_900.0,
        top: 50_000.0,
        up: true,
        volume: 5.5,
        trades_count: 12,
    };
    h.inject(LiveUpdate::RenkoBarUpdate {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        point,
    })?;
    let events = h.tick(16);
    let hit = events.iter().any(|e| matches!(e,
        BubbleEvent::RenkoBarAppended { bid: ev_bid, .. } if *ev_bid == bid));
    if !hit {
        return Err(err("renko", "no RenkoBarAppended emitted"));
    }
    let renko_len = h.core.bubble().bubble.bubble(bid)
        .and_then(|b| b.renko_bars.as_ref().map(|v| v.len()))
        .unwrap_or(0);
    if renko_len != 1 {
        return Err(err("renko", format!("bubble.renko_bars: {} != 1", renko_len)));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn stream_renko_brick_writes_bubble() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_stream_renko_brick_writes_bubble(&mut h)
}

pub fn body_stream_pnf_column_writes_bubble(h: &mut BackendHarness) -> HarnessResult<()> {
    let bid = register_bubble(h, ExchangeId::Binance, AccountType::Spot, "BTCUSDT", Timeframe::h1());
    let point = digdigdig3_station::data::PnfColumnPoint {
        open_time: 1_700_000_000_000,
        column_id: 1,
        is_x: true,
        bottom: 50_000.0,
        top: 50_300.0,
        volume: 10.0,
        trades_count: 50,
    };
    h.inject(LiveUpdate::PnfBarUpdate {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        point,
        box_size: 100.0,
    })?;
    let events = h.tick(16);
    let hit = events.iter().any(|e| matches!(e,
        BubbleEvent::PnfBarAppended { bid: ev_bid, .. } if *ev_bid == bid));
    if !hit {
        return Err(err("pnf", "no PnfBarAppended emitted"));
    }
    let pnf_len = h.core.bubble().bubble.bubble(bid)
        .and_then(|b| b.pnf_columns.as_ref().map(|v| v.len()))
        .unwrap_or(0);
    if pnf_len != 1 {
        return Err(err("pnf", format!("bubble.pnf_columns: {} != 1", pnf_len)));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn stream_pnf_column_writes_bubble() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_stream_pnf_column_writes_bubble(&mut h)
}

pub fn body_stream_kagi_segment_writes_bubble(h: &mut BackendHarness) -> HarnessResult<()> {
    let bid = register_bubble(h, ExchangeId::Binance, AccountType::Spot, "BTCUSDT", Timeframe::h1());
    let point = digdigdig3_station::data::KagiSegmentPoint {
        open_time: 1_700_000_000_000,
        start_price: 50_000.0,
        end_price: 50_300.0,
        up: true,
        yang: true,
        is_connector: false,
    };
    h.inject(LiveUpdate::KagiBarUpdate {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        point,
    })?;
    let events = h.tick(16);
    let hit = events.iter().any(|e| matches!(e,
        BubbleEvent::KagiBarAppended { bid: ev_bid, .. } if *ev_bid == bid));
    if !hit {
        return Err(err("kagi", "no KagiBarAppended emitted"));
    }
    let kagi_len = h.core.bubble().bubble.bubble(bid)
        .and_then(|b| b.kagi_segments.as_ref().map(|v| v.len()))
        .unwrap_or(0);
    if kagi_len != 1 {
        return Err(err("kagi", format!("bubble.kagi_segments: {} != 1", kagi_len)));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn stream_kagi_segment_writes_bubble() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_stream_kagi_segment_writes_bubble(&mut h)
}

pub fn body_stream_cvd_scalar_writes_bubble(h: &mut BackendHarness) -> HarnessResult<()> {
    let bid = register_bubble(h, ExchangeId::Binance, AccountType::Spot, "BTCUSDT", Timeframe::h1());
    let point = digdigdig3_station::data::ScalarBarPoint {
        ts_ms: 1_700_000_000_000,
        value: 123.45,
    };
    h.inject(LiveUpdate::CvdLineUpdate {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        point,
    })?;
    let events = h.tick(16);
    let hit = events.iter().any(|e| matches!(e,
        BubbleEvent::CvdSampleAppended { bid: ev_bid, .. } if *ev_bid == bid));
    if !hit {
        return Err(err("cvd", "no CvdSampleAppended emitted"));
    }
    let cvd_len = h.core.bubble().bubble.bubble(bid)
        .and_then(|b| b.cvd_line.as_ref().map(|v| v.len()))
        .unwrap_or(0);
    if cvd_len != 1 {
        return Err(err("cvd", format!("bubble.cvd_line: {} != 1", cvd_len)));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn stream_cvd_scalar_writes_bubble() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_stream_cvd_scalar_writes_bubble(&mut h)
}

pub fn body_stream_tpo_session_writes_bubble(h: &mut BackendHarness) -> HarnessResult<()> {
    let bid = register_bubble(h, ExchangeId::Binance, AccountType::Spot, "BTCUSDT", Timeframe::h1());
    let point = digdigdig3_station::data::TpoSessionPoint {
        open_time: 1_700_000_000_000,
        tick_size: 1.0,
        session_high: 50_500.0,
        session_low: 49_500.0,
        poc_price: 50_000.0,
        vah_price: 50_300.0,
        val_price: 49_700.0,
        rows: vec![
            (49_500.0, vec!['A']),
            (50_000.0, vec!['A', 'B', 'C']),
            (50_500.0, vec!['C']),
        ],
    };
    h.inject(LiveUpdate::TpoProfileUpdate {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        point,
    })?;
    let events = h.tick(16);
    let hit = events.iter().any(|e| matches!(e,
        BubbleEvent::TpoSessionUpdated { bid: ev_bid, .. } if *ev_bid == bid));
    if !hit {
        return Err(err("tpo", "no TpoSessionUpdated emitted"));
    }
    let tpo_len = h.core.bubble().bubble.bubble(bid)
        .and_then(|b| b.tpo_sessions.as_ref().map(|v| v.len()))
        .unwrap_or(0);
    if tpo_len != 1 {
        return Err(err("tpo", format!("bubble.tpo_sessions: {} != 1", tpo_len)));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn stream_tpo_session_writes_bubble() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_stream_tpo_session_writes_bubble(&mut h)
}

pub fn body_stream_three_line_break_writes_bubble(h: &mut BackendHarness) -> HarnessResult<()> {
    let bid = register_bubble(h, ExchangeId::Binance, AccountType::Spot, "BTCUSDT", Timeframe::h1());
    let point = digdigdig3_station::data::ThreeLineBreakLinePoint {
        ts_open: 1_700_000_000_000,
        ts_close: 1_700_000_060_000,
        open: 50_000.0,
        close: 50_200.0,
        volume: 8.0,
        direction: 0,
    };
    h.inject(LiveUpdate::ThreeLineBreakUpdate {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        point,
    })?;
    let events = h.tick(16);
    let hit = events.iter().any(|e| matches!(e,
        BubbleEvent::TlbLineAppended { bid: ev_bid, .. } if *ev_bid == bid));
    if !hit {
        return Err(err("tlb", "no TlbLineAppended emitted"));
    }
    let tlb_len = h.core.bubble().bubble.bubble(bid)
        .and_then(|b| b.three_line_break.as_ref().map(|v| v.len()))
        .unwrap_or(0);
    if tlb_len != 1 {
        return Err(err("tlb", format!("bubble.three_line_break: {} != 1", tlb_len)));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn stream_three_line_break_writes_bubble() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_stream_three_line_break_writes_bubble(&mut h)
}

pub fn body_stream_dom_trade_dot_passthrough(h: &mut BackendHarness) -> HarnessResult<()> {
    let dot = mlc_core::chart::dom_heatmap::DomTradeDot {
        ts_ms: 1_700_000_000_000,
        price: 50_000.0,
        quantity: 0.25,
        side: 1,
    };
    h.inject(LiveUpdate::DomTradeDot {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        dot,
    })?;
    let events = h.tick(16);
    let hit = events.iter().any(|e| matches!(e,
        BubbleEvent::DomTradeDotPassthrough { symbol, dot, .. }
            if symbol == "BTCUSDT" && (dot.price - 50_000.0).abs() < 1e-6));
    if !hit {
        return Err(err("dom-dot", "no DomTradeDotPassthrough emitted"));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn stream_dom_trade_dot_passthrough() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_stream_dom_trade_dot_passthrough(&mut h)
}

pub fn body_alerts_straddle_fires_on_cross(h: &mut BackendHarness) -> HarnessResult<()> {
    use mlc_alerts::{AlertCondition, AlertSource};
    let pivot: f64 = 50_000.0;
    let target_up = pivot * 1.0025;   // +0.25 %
    let target_dn = pivot * 0.9975;   // -0.25 %

    let (id_up, id_dn) = {
        let mut mgr = h.core.bubble_mut();
        let id_up = mgr.alert.create(
            AlertSource::Price { symbol: "BTCUSDT".to_string() },
            "+0.25 BTC",
            target_up,
            AlertCondition::CrossingUp,
        );
        let id_dn = mgr.alert.create(
            AlertSource::Price { symbol: "BTCUSDT".to_string() },
            "-0.25 BTC",
            target_dn,
            AlertCondition::CrossingDown,
        );
        (id_up, id_dn)
    };

    // Seed prior price = pivot so the FIRST tick establishes a baseline.
    // Then push a price that crosses the upper target.
    let prior_tick: f64 = pivot;
    let new_tick: f64 = pivot * 1.0050; // jumps past target_up

    let triggered_up;
    let triggered_dn;
    {
        let mut mgr = h.core.bubble_mut();
        let _ = mgr.alert.check_crossings_dynamic(
            prior_tick, 0.0, "BTCUSDT", "binance", "S", &[], &[],
        );
        let fired = mgr.alert.check_crossings_dynamic(
            new_tick, 1.0, "BTCUSDT", "binance", "S", &[], &[],
        );
        triggered_up = fired.contains(&id_up);
        triggered_dn = fired.contains(&id_dn);
    }
    if !triggered_up {
        return Err(err("alert", format!("upper +0.25% alert (id={}) did NOT fire on price jump {} -> {}", id_up, prior_tick, new_tick)));
    }
    if triggered_dn {
        return Err(err("alert", format!("lower -0.25% alert (id={}) fired spuriously on upward jump", id_dn)));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn alerts_straddle_fires_on_cross() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_alerts_straddle_fires_on_cross(&mut h)
}

pub fn body_alerts_straddle_fires_on_drop(h: &mut BackendHarness) -> HarnessResult<()> {
    use mlc_alerts::{AlertCondition, AlertSource};
    let pivot: f64 = 50_000.0;
    let target_up = pivot * 1.0025;
    let target_dn = pivot * 0.9975;
    let (id_up, id_dn) = {
        let mut mgr = h.core.bubble_mut();
        let id_up = mgr.alert.create(
            AlertSource::Price { symbol: "BTCUSDT".to_string() },
            "+0.25 BTC",
            target_up,
            AlertCondition::CrossingUp,
        );
        let id_dn = mgr.alert.create(
            AlertSource::Price { symbol: "BTCUSDT".to_string() },
            "-0.25 BTC",
            target_dn,
            AlertCondition::CrossingDown,
        );
        (id_up, id_dn)
    };
    let prior_tick: f64 = pivot;
    let new_tick: f64 = pivot * 0.9950; // drops past target_dn

    let triggered_up;
    let triggered_dn;
    {
        let mut mgr = h.core.bubble_mut();
        let _ = mgr.alert.check_crossings_dynamic(
            prior_tick, 0.0, "BTCUSDT", "binance", "S", &[], &[],
        );
        let fired = mgr.alert.check_crossings_dynamic(
            new_tick, 1.0, "BTCUSDT", "binance", "S", &[], &[],
        );
        triggered_up = fired.contains(&id_up);
        triggered_dn = fired.contains(&id_dn);
    }
    if !triggered_dn {
        return Err(err("alert", format!("lower -0.25% alert (id={}) did NOT fire on drop {} -> {}", id_dn, prior_tick, new_tick)));
    }
    if triggered_up {
        return Err(err("alert", format!("upper +0.25% alert (id={}) fired spuriously on downward drop", id_up)));
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn alerts_straddle_fires_on_drop() -> HarnessResult<()> {
    let mut h = BackendHarness::fresh();
    body_alerts_straddle_fires_on_drop(&mut h)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn sync_scenarios() -> Vec<(&'static str, fn() -> HarnessResult<()>)> {
    vec![
        ("smoke_backend_alive", smoke_backend_alive),
        ("data_plane_bars_loaded", data_plane_bars_loaded),
        ("data_plane_bars_no_bubble", data_plane_bars_no_bubble),
        ("data_plane_pass_through_bar_update", data_plane_pass_through_bar_update),
        ("mini_ticker_cache", mini_ticker_cache),
        ("connector_ready_event", connector_ready_event),
        ("session_registry_lifecycle", session_registry_lifecycle),
        ("drawing_registry_basic", drawing_registry_basic),
        ("indicator_atomic_window_id", indicator_atomic_window_id),
        ("lock_discipline_concurrent_reads", lock_discipline_concurrent_reads),
        // Extended stream coverage
        ("stream_backfill_complete_writes_bubble", stream_backfill_complete_writes_bubble),
        ("stream_scroll_bars_merges_with_live_tail", stream_scroll_bars_merges_with_live_tail),
        ("stream_trade_passthrough", stream_trade_passthrough),
        ("stream_orderbook_snapshot_ws_emits_event", stream_orderbook_snapshot_ws_emits_event),
        ("stream_orderbook_delta_emits_event", stream_orderbook_delta_emits_event),
        ("stream_open_interest_passthrough", stream_open_interest_passthrough),
        ("stream_funding_rate_passthrough", stream_funding_rate_passthrough),
        ("stream_liquidation_event_passthrough", stream_liquidation_event_passthrough),
        ("stream_liquidation_bucket_passthrough", stream_liquidation_bucket_passthrough),
        ("stream_dom_heatmap_snapshot_passthrough", stream_dom_heatmap_snapshot_passthrough),
        // Multi-symbol isolation
        ("multi_symbol_bars_isolation", multi_symbol_bars_isolation),
        ("multi_symbol_trade_passthrough_independent", multi_symbol_trade_passthrough_independent),
        ("multi_symbol_mini_ticker_cache_keys", multi_symbol_mini_ticker_cache_keys),
        // Derived bars (aggTrades-fueled) — backend re-emits to the bubble store
        ("stream_footprint_update_writes_bubble", stream_footprint_update_writes_bubble),
        ("stream_renko_brick_writes_bubble", stream_renko_brick_writes_bubble),
        ("stream_pnf_column_writes_bubble", stream_pnf_column_writes_bubble),
        ("stream_kagi_segment_writes_bubble", stream_kagi_segment_writes_bubble),
        ("stream_cvd_scalar_writes_bubble", stream_cvd_scalar_writes_bubble),
        ("stream_tpo_session_writes_bubble", stream_tpo_session_writes_bubble),
        ("stream_three_line_break_writes_bubble", stream_three_line_break_writes_bubble),
        ("stream_dom_trade_dot_passthrough", stream_dom_trade_dot_passthrough),
        // Alert delivery on cross
        ("alerts_straddle_fires_on_cross", alerts_straddle_fires_on_cross),
        ("alerts_straddle_fires_on_drop", alerts_straddle_fires_on_drop),
    ]
}

