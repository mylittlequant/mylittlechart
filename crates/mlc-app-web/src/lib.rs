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
//!   provider.poll_events() → dispatch to ChartApp input methods
//!   canvas_ctx = get_canvas_context()
//!   chart_app.render(&mut ctx, now_ms, false)
//! ```
//!
//! # Swapping render backends
//!
//! The `run_render_loop` function is the single seam where the render backend is
//! selected.  To switch from canvas2d to a different backend:
//! 1. Uncomment the relevant `dep:uzor-render-*` line in `Cargo.toml`.
//! 2. Add the corresponding feature flag.
//! 3. Replace `Canvas2dChartRenderContext::new(...)` with the new context
//!    constructor — the interface is identical because all contexts implement
//!    `mlc_core::render::RenderContext`.

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

// ─── Entry point ──────────────────────────────────────────────────────────────

/// WASM entry point — invoked automatically when the module is loaded.
///
/// Boots the data plane, constructs `ChartApp`, and starts the RAF render loop.
/// All async work is driven by `wasm_bindgen_futures::spawn_local` — there is
/// no `tokio::Runtime` and no `block_on` anywhere in this path.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() {
    // Custom panic hook BEFORE console_error_panic_hook so panic_info.location()
    // (file:line of the actual `panic!()` call site) shows up in the browser
    // console. The default hook only shows the post-unwind stack which on wasm
    // collapses to console_error_panic_hook frames and tells us nothing about
    // who actually panicked. We chain to console_error_panic_hook afterwards
    // so its full Error.stack still arrives too.
    std::panic::set_hook(Box::new(|info| {
        let loc = info.location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown location>".to_string());
        let msg = info.payload().downcast_ref::<&'static str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(|s| s.as_str()))
            .unwrap_or("<non-string payload>");
        // Capture JS-side stack trace BEFORE we unwind. This is the only
        // way to see who actually called the panicking fn on wasm — the
        // post-unwind stack collapses to console_error_panic_hook frames.
        let js_err = js_sys::Error::new("[panic-trace]");
        let stack = js_sys::Reflect::get(&js_err, &"stack".into())
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_else(|| "<no stack>".to_string());
        web_sys::console::error_1(
            &format!("[PANIC] at {} :: {}\n=== CALLER STACK ===\n{}", loc, msg, stack).into()
        );
        // Chain to console_error_panic_hook for the full stack.
        console_error_panic_hook::hook(info);
    }));
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
    // 1. Build the data plane.  On wasm32, DataBridge::new() is a plain async
    //    fn backed by station's wasm-aware spawn_local paths — no Runtime needed.
    let (bridge, _live_rx, _connector_ready_rx) = DataBridge::new().await;
    let bridge = Arc::new(bridge);
    web_sys::console::log_1(&"[mlc-app-web] bridge ready".into());

    // 2. Construct ChartApp from the shared bridge.
    //    new_empty() is uncfg-gated — it works on both native and wasm32.
    //    It skips profile loading, welcome wizard, vault unlock — all native-only.
    let app = ChartApp::new_empty(bridge.clone());
    web_sys::console::log_1(&"[mlc-app-web] app ready".into());

    // 3. Mount the event provider (DOM canvas → PlatformEvents).
    let provider = WebWindowProvider::from_id("chart")
        .map_err(|e| JsValue::from_str(&e))?;

    // 4. Start the render loop.
    web_sys::console::log_1(&"[mlc-app-web] render loop running".into());
    run_render_loop(app, provider)
}

// ─── RAF render loop ───────────────────────────────────────────────────────────

/// Drive the app via `requestAnimationFrame`.
///
/// Shared state is wrapped in `Rc<RefCell<_>>` — wasm32 is single-threaded so
/// `Arc<Mutex<_>>` would be overkill and would require `tokio::sync::Mutex` to
/// avoid holding across `.await`.  Here there is no `.await` inside the RAF
/// callback, so `RefCell` is the correct primitive.
#[cfg(target_arch = "wasm32")]
fn run_render_loop(
    app: ChartApp,
    provider: WebWindowProvider,
) -> Result<(), JsValue> {
    let app = Rc::new(RefCell::new(app));
    let provider = Rc::new(RefCell::new(provider));

    // The RAF callback needs to schedule itself for the next frame.
    // We use the classic Rc<RefCell<Option<Closure<…>>>> self-referential trick.
    let closure: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let closure_clone = closure.clone();

    let app_clone = app.clone();
    let provider_clone = provider.clone();

    *closure.borrow_mut() = Some(Closure::wrap(Box::new(move || {
        // ── Diag counter — bump window.MLC_FRAMES each frame so we can verify
        //     externally that the RAF loop is alive.
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
            let mut chart = app_clone.borrow_mut();
            for event in events {
                dispatch_event(&mut chart, event);
            }
        }

        // ── Tick: drain LiveUpdate channel into BubbleStore + run lifecycle.
        // Without this, bars fetched by `request_bars` sit in the broadcast
        // channel forever — render reads from BubbleStore which stays empty
        // and the chart is blank despite the status bar showing the instrument.
        {
            let now_ms = web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            app_clone.borrow_mut().tick(now_ms);
        }

        // ── Render frame ──────────────────────────────────────────────────────
        let render_result = render_frame(app_clone.clone());
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

    // Kick off the first frame.
    if let Some(win) = web_sys::window() {
        if let Some(cb) = closure.borrow().as_ref() {
            let _ = win.request_animation_frame(cb.as_ref().unchecked_ref());
        }
    }

    // The closure must stay alive for the loop to continue. We intentionally
    // leak the Rc here — the loop runs for the entire page lifetime.
    std::mem::forget(closure);

    Ok(())
}

// ─── Input dispatch ────────────────────────────────────────────────────────────

/// Translate a uzor [`PlatformEvent`] into the corresponding `ChartApp` input
/// method call.
///
/// Mirrors the vello shell's mouse pipeline (see
/// `mlc-app-vello/src/window/event_handler.rs`):
///   PointerDown(Left)  → `on_drag_start`           (always, unconditional)
///   PointerUp(Left)    → if movement < 5px:  `on_click`  THEN `on_drag_end`
///                        else:                          `on_drag_end`
///   PointerMoved       → `on_mouse_move`
/// The slop logic from the spike's first version was wrong — it withheld
/// `on_drag_start` until the pointer had moved 4px, which broke every
/// toolbar/sidebar handler that listens for drag_start. The vello shell
/// fires drag_start on every press and emits `on_click` IN ADDITION to
/// `on_drag_end` when the press never moved, so click handlers in mlc-app
/// see both events.
#[cfg(target_arch = "wasm32")]
const CLICK_SLOP_PX: f64 = 5.0;

#[cfg(target_arch = "wasm32")]
thread_local! {
    static PRESS_POS: std::cell::Cell<Option<(f64, f64)>> = const { std::cell::Cell::new(None) };
    static MOUSE_PRESSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(target_arch = "wasm32")]
fn dispatch_event(chart: &mut ChartApp, event: uzor::input::core::event_processor::PlatformEvent) {
    use uzor::input::core::event_processor::PlatformEvent;
    use uzor::input::pointer::state::MouseButton;

    match event {
        PlatformEvent::PointerDown { x, y, button: MouseButton::Left } => {
            MOUSE_PRESSED.with(|p| p.set(true));
            PRESS_POS.with(|p| p.set(Some((x, y))));
            let dismissed = chart.on_drag_start(x, y);
            if dismissed {
                // Popup-dismiss: synthetic drag-end to clean ui_drag_active.
                chart.on_drag_end(x, y);
                MOUSE_PRESSED.with(|p| p.set(false));
                PRESS_POS.with(|p| p.set(None));
            }
        }
        PlatformEvent::PointerUp { x, y, button: MouseButton::Left } => {
            if MOUSE_PRESSED.with(|p| p.replace(false)) {
                if let Some((sx, sy)) = PRESS_POS.with(|p| p.take()) {
                    let dx = x - sx;
                    let dy = y - sy;
                    if dx * dx + dy * dy < CLICK_SLOP_PX * CLICK_SLOP_PX {
                        // Click + drag_end (vello shell's dual dispatch).
                        chart.on_click(x, y);
                    }
                }
                chart.on_drag_end(x, y);
            }
        }
        PlatformEvent::PointerMoved { x, y } => {
            chart.on_mouse_move(x, y);
        }
        PlatformEvent::PointerLeft => {
            MOUSE_PRESSED.with(|p| p.set(false));
            PRESS_POS.with(|p| p.set(None));
            chart.on_mouse_leave();
        }
        PlatformEvent::Scroll { dx, dy } => {
            chart.on_scroll(0.0, 0.0, dx, dy, false);
        }
        PlatformEvent::WindowResized { width, height } => {
            chart.resize(width, height);
        }
        // Right-click, keyboard, touch, focus — spikes do not need to handle
        // these exhaustively.  They are silently dropped here; add arms as
        // needed during productionisation.
        _ => {}
    }
}

// ─── Render frame ──────────────────────────────────────────────────────────────

/// Obtain the canvas 2D context, build a `Canvas2dChartRenderContext`, and
/// call `ChartApp::render`.
///
/// This is the single backend-selection point: to swap to vello-cpu, replace
/// `Canvas2dChartRenderContext::from_canvas_id` with the vello-cpu equivalent
/// and update the feature flag in `Cargo.toml`.
#[cfg(target_arch = "wasm32")]
fn render_frame(app: Rc<RefCell<ChartApp>>) -> Result<(), JsValue> {
    use wasm_bindgen::JsCast;

    let win = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let doc = win.document().ok_or_else(|| JsValue::from_str("no document"))?;
    let canvas = doc
        .get_element_by_id("chart")
        .ok_or_else(|| JsValue::from_str("canvas#chart not found"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()?;

    let dpr = win.device_pixel_ratio();

    // Resize the backing buffer to match CSS size × dpr.
    let css_w = canvas.client_width() as u32;
    let css_h = canvas.client_height() as u32;
    let phys_w = (css_w as f64 * dpr) as u32;
    let phys_h = (css_h as f64 * dpr) as u32;
    if canvas.width() != phys_w { canvas.set_width(phys_w); }
    if canvas.height() != phys_h { canvas.set_height(phys_h); }

    // Obtain the raw 2D context object.
    let raw_ctx = canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("could not get 2d context"))?
        .dyn_into::<web_sys::CanvasRenderingContext2d>()?;

    // Apply DPR scale so all drawing coordinates are in CSS pixels.
    raw_ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0)?;

    // Resize ChartApp to match CSS logical pixels.
    {
        let mut chart = app.borrow_mut();
        chart.resize(css_w, css_h);
    }

    // Build the chart render context wrapping the 2D context.
    let mut ctx = Canvas2dChartRenderContext::new(raw_ctx, dpr, css_w, css_h);

    // Current time for animation-dependent rendering (loading spinners etc.).
    let now_ms = win
        .performance()
        .map(|p| p.now() as u64)
        .unwrap_or(0);

    // Drive the frame.
    app.borrow_mut().render(&mut ctx, now_ms, false);

    Ok(())
}
