//! Test scenarios. Each returns `Ok(())` or a `HarnessError` with the
//! specific assertion that failed. Public for reuse by the runner binary and
//! by any other consumer that wants to embed a subset.

use std::time::Duration;

use digdigdig3::{AccountType, ExchangeId};
use mlc_backend_core::BubbleEvent;
use mlc_core::state::chart_container_leaf::ChartPanelId;
use mlc_core::state::Timeframe;
use mlc_core::tag_manager::BubbleId;
use mlc_core::types::Bar;
use mlc_live_data::LiveUpdate;

use crate::{BackendHarness, DaemonGuard, HarnessError, HarnessResult};

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

pub async fn agent_state_apps_with_and_without_core(h: BackendHarness) -> HarnessResult<()> {
    use mlc_agentic_server::AgentState;
    use std::sync::Arc as StdArc;

    // Phase A: AgentState WITHOUT core attached.
    let state_no_core = StdArc::new(AgentState::new(h.bridge.clone(), "harness".to_string()));
    let port_a = pick_free_port()?;
    let rt = spawn_server_runtime()?;
    let _handle_a = mlc_agentic_server::start_server(state_no_core.clone(), rt, port_a);
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

fn pick_free_port() -> HarnessResult<u16> {
    let l = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| err("port-pick", format!("bind: {e}")))?;
    let port = l.local_addr().map_err(|e| err("port-pick", format!("addr: {e}")))?.port();
    drop(l);
    Ok(port)
}

async fn wait_for_health(port: u16, path: &str) -> HarnessResult<()> {
    let url = format!("http://127.0.0.1:{port}{path}");
    let client = reqwest::Client::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(r) = client.get(&url).send().await {
            if r.status().is_success() {
                return Ok(());
            }
        }
        if std::time::Instant::now() > deadline {
            return Err(err("wait-health", format!("{url} never returned 2xx within 5s")));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// ==========================================================================
// 12. Mode-2 daemon e2e: spawn binary, /health works, /v1/apps round-trips
// ==========================================================================

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
// Master list — runner iterates this. The async ones live in a parallel list.
// ==========================================================================

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
    ]
}

