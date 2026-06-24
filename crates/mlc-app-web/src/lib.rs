//! `mlc-app-web` — browser shell for the MLC chart application.
//!
//! This crate is a **spike** (Phase 4 of the wasm migration plan).  It is the
//! wasm32 sibling of `mlc-app-vello` (the native shell).  Everything in this
//! crate is cfg-gated under `#[cfg(target_arch = "wasm32")]` so that
//! `cargo check --workspace` (native) compiles it as an empty lib.
//!
//! # Build
//!
//! ```sh
//! cargo check -p mylittlechart-app-web --target wasm32-unknown-unknown
//! wasm-pack build crates/mlc-app-web --target web --out-dir ../../dist/mlc-app-web
//! ```
//!
//! See `README.md` for full build + serve instructions.
//!
//! # Architecture sketch
//!
//! ```text
//! DataBridge::new().await        → Arc<DataBridge>
//! BackendCore::new(bridge, None) → Arc<BackendCore>   (no trading mgr on wasm)
//! ChartApp::new_empty(bridge)    → ChartApp
//!
//! WebWindowProvider::from_id("chart")   ← DOM event source
//! Canvas2dChartRenderContext            ← render surface (mlc_core::RenderContext impl)
//!
//! requestAnimationFrame loop:
//!   provider.poll_events() → dispatch to ChartApp via mlc_input_shell::process_event
//!   canvas_ctx = get_canvas_context()
//!   chart_app.render(&mut ctx, now_ms, false)
//! ```

#[cfg(target_arch = "wasm32")]
mod context;

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
use mlc_live_data::DataBridge;
#[cfg(target_arch = "wasm32")]
use mlc_core::state::Timeframe;
#[cfg(target_arch = "wasm32")]
use mlc_input_shell::{ChromeFlavor, CHROME_HEIGHT};

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
    tracing_wasm::set_as_global_default_with_config(
        tracing_wasm::WASMLayerConfigBuilder::new()
            // TRACE so we see every per-frame dispatch (dig3::ws::frame),
            // not just connect/error events. Yes, this is loud — that's the
            // point. Filter in DevTools if needed.
            .set_max_level(tracing::Level::TRACE)
            .build(),
    );
    web_sys::console::log_1(&"[mlc-app-web] booting".into());

    spawn_local(async {
        if let Err(e) = run().await {
            web_sys::console::error_1(&format!("[mlc-app-web] fatal: {:?}", e).into());
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
    web_sys::console::log_1(&"[mlc-app-web] bridge ready".into());

    let mut app = ChartApp::new_empty(bridge.clone());
    app.seed_default_chart(
        digdigdig3::ExchangeId::Binance,
        "BTCUSDT",
        Timeframe::h1(),
        digdigdig3::AccountType::Spot,
        500,
    );
    web_sys::console::log_1(&"[mlc-app-web] app ready".into());

    let provider = WebWindowProvider::from_id("chart")
        .map_err(|e| JsValue::from_str(&e))?;

    web_sys::console::log_1(&"[mlc-app-web] render loop running".into());
    run_render_loop(app, provider)
}

// ─── App state ────────────────────────────────────────────────────────────────

/// Per-session state for the web shell, replacing the old thread_local! PRESS_POS / MOUSE_PRESSED.
#[cfg(target_arch = "wasm32")]
struct WebAppState {
    chart:              ChartApp,
    input:              mlc_input_shell::InputState,
    chrome:             mlc_input_shell::ChromeState,
    dirty:              mlc_input_shell::DirtyFlags,
    out:                mlc_input_shell::DispatchOut,
    toolbar_tooltip:    mlc_input_shell::TooltipState,
    screenshot_pending: bool,
    /// DOM canvas element for cursor CSS updates.
    canvas_id:          String,
}

#[cfg(target_arch = "wasm32")]
impl WebAppState {
    fn new(chart: ChartApp, canvas_id: &str) -> Self {
        let mut input = mlc_input_shell::InputState::default();
        // Web shell renders chrome — offset chart by chrome height.
        input.chrome_height = CHROME_HEIGHT;
        let mut chrome = mlc_input_shell::ChromeState::new("");
        chrome.flavor = ChromeFlavor::Web;
        Self {
            chart,
            input,
            chrome,
            dirty:  mlc_input_shell::DirtyFlags::default(),
            out:    mlc_input_shell::DispatchOut::default(),
            toolbar_tooltip:    mlc_input_shell::TooltipState::new(),
            screenshot_pending: false,
            canvas_id:          canvas_id.to_string(),
        }
    }
}

// ─── RAF render loop ───────────────────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
fn run_render_loop(
    app: ChartApp,
    provider: WebWindowProvider,
) -> Result<(), JsValue> {
    let state = Rc::new(RefCell::new(WebAppState::new(app, "chart")));
    let provider = Rc::new(RefCell::new(provider));

    let closure: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let closure_clone = closure.clone();

    let state_clone = state.clone();
    let provider_clone = provider.clone();

    *closure.borrow_mut() = Some(Closure::wrap(Box::new(move || {
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
            state_clone.borrow_mut().chart.tick(now_ms);
        }

        // ── Diag: expose active bubble's last bar timestamp + bars count + close
        //     to JS so `dev-attach --eval window.MLC_DEBUG` can see whether the
        //     live tick actually reaches the bubble (the chart reads from there).
        if let Some(win) = web_sys::window() {
            let st = state_clone.borrow();
            let active_cid = st.chart.panel_app.panel_grid.active_container_id();
            let active_bid = active_cid.and_then(|cid| st.chart.bubble_for_chart(cid));
            let mgr = st.chart.core.bubble();
            let bubble = active_bid.and_then(|bid| mgr.bubble.bubble(bid));
            let (sym, count, last_ts, last_close) = bubble
                .map(|b| {
                    let last = b.bars.last();
                    (
                        b.symbol.clone(),
                        b.bars.len() as f64,
                        last.map(|x| x.timestamp as f64).unwrap_or(0.0),
                        last.map(|x| x.close).unwrap_or(0.0),
                    )
                })
                .unwrap_or_else(|| ("<none>".to_string(), 0.0, 0.0, 0.0));
            let (view_start, bar_spacing, chart_width, bar_count_vp) = active_cid
                .and_then(|cid| st.chart.panel_app.panel_grid.containers().get(&cid))
                .map(|w| (
                    w.viewport.view_start,
                    w.viewport.bar_spacing,
                    w.viewport.chart_width,
                    w.viewport.bar_count as f64,
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
            let _ = js_sys::Reflect::set(&win, &"MLC_DEBUG".into(), &dbg);
        }

        // ── Render frame ──────────────────────────────────────────────────────
        let render_result = render_frame(state_clone.clone());
        if let Err(e) = render_result {
            web_sys::console::error_1(&format!("[mlc-app-web] render error: {:?}", e).into());
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
impl mlc_input_shell::Platform for WebPlatform<'_> {
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
    fn drag_resize_window(&self, _: mlc_input_shell::ResizeDir) {}
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
    state: &mlc_input_shell::InputState,
) -> Option<mlc_input_shell::ShellEvent> {
    use uzor::input::core::event_processor::PlatformEvent;
    use uzor::input::keyboard::events::KeyCode;
    use uzor::input::pointer::state::MouseButton;
    use mlc_input_shell::{ShellEvent, MouseBtn, NormalizedModifiers, LogicalKey, PhysicalKey};

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

        // Keyboard.  Mirrors mlc-app-vello/src/window/event_handler.rs:101-150 —
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
        mlc_input_shell::process_event(
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
        st.out = mlc_input_shell::DispatchOut::default();
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

    // Chart owns the area BELOW the chrome strip — shrink by CHROME_HEIGHT so
    // the chart's panel/toolbar layout doesn't draw under the chrome and the
    // bottom edge stays visible.
    let chart_h = css_h.saturating_sub(CHROME_HEIGHT as u32);
    {
        let mut st = state.borrow_mut();
        st.chart.resize(css_w, chart_h);
    }

    let mut ctx = Canvas2dChartRenderContext::new(raw_ctx, dpr, css_w, css_h);

    let now_ms = win
        .performance()
        .map(|p| p.now() as u64)
        .unwrap_or(0);

    // ── Tab sync (mirror of scene_builder.rs:63-94 for the web shell) ─────────
    {
        let mut st = state.borrow_mut();
        let open_tabs = &st.chart.panel_app.open_tabs;
        let active_id = st.chart.panel_app.active_preset_id.clone();
        let presets_guard = st.chart.core.presets();
        let mut tabs: Vec<mlc_input_shell::Tab> = open_tabs
            .iter()
            .filter_map(|tab_id| {
                presets_guard.get(tab_id).map(|p| mlc_input_shell::Tab {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    active: p.id == active_id,
                })
            })
            .collect();
        drop(presets_guard);
        if tabs.is_empty() {
            tabs.push(mlc_input_shell::Tab {
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
    mlc_input_shell::update_tab_widths(&mut ctx, &mut state.borrow_mut().chrome);

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
            st.chart.render_modal_layer(&mut ctx, now_ms);
        }
        ctx.restore();
    }

    // ── Chrome render ─────────────────────────────────────────────────────────
    {
        let st = state.borrow();
        mlc_input_shell::render(&mut ctx, &st.chrome, css_w as f64, false);
        mlc_input_shell::render_tooltip(&mut ctx, &st.chrome, css_w as f64, css_h as f64);
    }

    Ok(())
}
