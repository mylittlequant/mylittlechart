//! `mlc-ui-lab` — wasm UI laboratory.
//!
//! Hosts the REAL `mlc-app-layout` modal builders (primitive settings, to
//! start) and real uzor widgets, driven by a real `uzor::input::InputCoordinator`,
//! in a standalone browser page. No `mlc-app`, no `mlc-backend`, no
//! `digdigdig3` — see `README.md` for the feasibility claims this rests on
//! and `Cargo.toml` for the exact dependency list.
//!
//! No chart, no toolbars, no chrome: this lab exists to compare *modal
//! designs* against an empty background. `crate::variants` is the scaffold
//! for that — a registry of named "draw a settings surface given the same
//! inputs" functions, switchable live from the lab bar, orthogonal to the
//! theme switcher.
//!
//! The entire crate body is cfg-gated under `#[cfg(target_arch = "wasm32")]`
//! so `cargo check --workspace` (native) compiles it as an empty lib — same
//! pattern as `mlc-app-shell-web`. Use
//! `cargo check -p mylittlechart-ui-lab --target wasm32-unknown-unknown` for
//! the real gate.

#[cfg(target_arch = "wasm32")]
mod dispatch;
#[cfg(target_arch = "wasm32")]
mod render_context;
#[cfg(target_arch = "wasm32")]
mod theme;
#[cfg(target_arch = "wasm32")]
mod variants;

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
use render_context::LabRenderContext;

#[cfg(target_arch = "wasm32")]
use mlc_app_layout::layout::modals::primitive_settings::render_primitive_color_picker_popup;
#[cfg(target_arch = "wasm32")]
use mlc_app_layout::layout::render_frame::PrimitiveSettingsResult;
#[cfg(target_arch = "wasm32")]
use mlc_app_layout::layout::toolbar_state::ToolbarState;
#[cfg(target_arch = "wasm32")]
use mlc_app_layout::ui::modal_settings::{ActiveFieldRender, PrimitiveSettingsState, PRIMITIVE_SETTINGS_FIELD};

#[cfg(target_arch = "wasm32")]
use mlc_core::drawing::primitives_v2::{Primitive, PrimitiveRegistry};
#[cfg(target_arch = "wasm32")]
use mlc_core::templates::PrimitiveTemplate;
#[cfg(target_arch = "wasm32")]
use mlc_core::theme::ThemeManager;

#[cfg(target_arch = "wasm32")]
use uzor::input::keyboard::events::KeyCode;
#[cfg(target_arch = "wasm32")]
use uzor::input::keyboard::keyboard::KeyPress;
#[cfg(target_arch = "wasm32")]
use uzor::input::{EventProcessor, InputCoordinator, InputState, MouseButton, PlatformEvent, TextFieldConfig};
#[cfg(target_arch = "wasm32")]
use uzor::layout::window::WindowProvider;
#[cfg(target_arch = "wasm32")]
use uzor::render::{Painter, ShapeHelpers};
#[cfg(target_arch = "wasm32")]
use uzor::WidgetId;
#[cfg(target_arch = "wasm32")]
use uzor_window_web::WebWindowProvider;

// ─── Entry point ──────────────────────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    web_sys::console::log_1(&"[mlc-ui-lab] booting".into());

    let lab = LabState::new()?;
    run_render_loop(lab)
}

// ─── Lab state ────────────────────────────────────────────────────────────────

/// Everything the lab owns. One Fib Retracement primitive (the richest
/// registered type — Style/Text/Coordinates/Levels/Visibility, see
/// `PrimitiveRegistry::global()` at `mlc-core/src/drawing/primitives_v2/registry.rs:64`),
/// its settings-modal state, a real theme manager, a real
/// `InputCoordinator` fed from real DOM events via `WebWindowProvider`, an
/// in-memory template store (no persistence layer exists in this crate), and
/// the index of the active design variant (`crate::variants::VARIANTS`).
#[cfg(target_arch = "wasm32")]
struct LabState {
    canvas_id: String,
    provider: WebWindowProvider,
    event_processor: EventProcessor,
    input_state: InputState,
    coordinator: InputCoordinator,
    theme_manager: ThemeManager,
    primitives: Vec<Box<dyn Primitive>>,
    primitive_settings_state: PrimitiveSettingsState,
    templates: Vec<PrimitiveTemplate>,
    active_variant: usize,
    /// Geometry the drag dispatch needs (modal frame / slider tracks /
    /// scrollbar track / colour-picker opacity slider rect), refreshed at
    /// the end of every frame that renders the modal. Read by the raw
    /// `PointerDown`/`PointerMoved`/`PointerUp` handlers below, which run
    /// BEFORE this frame's own render pass — see `dispatch::DragGeometry`.
    drag_geometry: dispatch::DragGeometry,
}

#[cfg(target_arch = "wasm32")]
impl LabState {
    fn new() -> Result<Self, JsValue> {
        let provider = WebWindowProvider::from_id("chart").map_err(|e| JsValue::from_str(&e))?;

        // Two swing points a screen-worth apart — plausible input, not
        // meaningful market data (the lab never touches a data plane).
        let now_ms = js_sys::Date::now() as i64;
        let points: [(i64, f64); 2] = [(now_ms - 3_600_000, 42_000.0), (now_ms, 38_500.0)];
        let registry = PrimitiveRegistry::global()
            .read()
            .map_err(|_| JsValue::from_str("primitive registry lock poisoned"))?;
        let fib = registry
            .create("fib_retracement", &points, None)
            .ok_or_else(|| JsValue::from_str("fib_retracement not registered"))?;
        drop(registry);
        let primitive_id = fib.data().id;

        let mut primitive_settings_state = PrimitiveSettingsState::new();
        primitive_settings_state.open(primitive_id);

        let mut coordinator = InputCoordinator::new();
        // One shared text field for every inline sub-field edit in the
        // modal — same identity the real app uses
        // (`mlc-app-layout/src/ui/modal_settings.rs:103`). The rect is a
        // placeholder: the lab drives focus explicitly (`dispatch::start_editing`)
        // rather than relying on the coordinator's own click-to-focus path.
        coordinator.register_text_field(
            PRIMITIVE_SETTINGS_FIELD,
            uzor::types::Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 },
            TextFieldConfig::text(),
        );

        Ok(Self {
            canvas_id: "chart".to_string(),
            provider,
            event_processor: EventProcessor::new(),
            input_state: InputState::new(),
            coordinator,
            theme_manager: ThemeManager::new(),
            primitives: vec![fib],
            primitive_settings_state,
            templates: Vec::new(),
            active_variant: 0,
            drag_geometry: dispatch::DragGeometry::default(),
        })
    }

}

/// Map a DOM `KeyCode` to the `TextFieldStore`'s named-key vocabulary.
/// Printable characters arrive separately as `PlatformEvent::TextInput`
/// (see `uzor-window-web`'s `keydown` listener) — this only covers the
/// control keys text editing needs.
#[cfg(target_arch = "wasm32")]
fn key_press_from(key: &KeyCode) -> Option<KeyPress> {
    Some(match key {
        KeyCode::ArrowLeft => KeyPress::ArrowLeft,
        KeyCode::ArrowRight => KeyPress::ArrowRight,
        KeyCode::ArrowUp => KeyPress::ArrowUp,
        KeyCode::ArrowDown => KeyPress::ArrowDown,
        KeyCode::Home => KeyPress::Home,
        KeyCode::End => KeyPress::End,
        KeyCode::PageUp => KeyPress::PageUp,
        KeyCode::PageDown => KeyPress::PageDown,
        KeyCode::Delete => KeyPress::Delete,
        KeyCode::Backspace => KeyPress::Backspace,
        KeyCode::Enter => KeyPress::Enter,
        KeyCode::Escape => KeyPress::Escape,
        KeyCode::Tab => KeyPress::Tab,
        _ => return None,
    })
}

// ─── RAF render loop ────────────────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
fn run_render_loop(lab: LabState) -> Result<(), JsValue> {
    let state = Rc::new(RefCell::new(lab));

    let closure: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let closure_clone = closure.clone();
    let state_clone = state.clone();

    *closure.borrow_mut() = Some(Closure::wrap(Box::new(move || {
        if let Err(e) = render_one_frame(&state_clone) {
            web_sys::console::error_1(&format!("[mlc-ui-lab] frame error: {:?}", e).into());
        }
        if let Some(win) = web_sys::window() {
            let _ = win.request_animation_frame(
                closure_clone.borrow().as_ref().unwrap().as_ref().unchecked_ref(),
            );
        }
    }) as Box<dyn FnMut()>));

    web_sys::window()
        .ok_or_else(|| JsValue::from_str("no window"))?
        .request_animation_frame(closure.borrow().as_ref().unwrap().as_ref().unchecked_ref())?;

    web_sys::console::log_1(&"[mlc-ui-lab] render loop running".into());
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn render_one_frame(state: &Rc<RefCell<LabState>>) -> Result<(), JsValue> {
    use wasm_bindgen::JsCast;

    let win = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let doc = win.document().ok_or_else(|| JsValue::from_str("no document"))?;

    let mut lab_guard = state.borrow_mut();
    // Rebind to a plain `&mut LabState` (one `DerefMut` up front) so the
    // borrow checker sees ordinary disjoint struct-field places for the
    // rest of this function instead of a fresh `RefMut::deref_mut` call at
    // every `lab.field` access — the latter cannot prove two field borrows
    // taken in the same expression (e.g. `lab.a.f(&mut lab.b)`) are disjoint.
    let lab: &mut LabState = &mut lab_guard;
    let canvas = doc
        .get_element_by_id(&lab.canvas_id)
        .ok_or_else(|| JsValue::from_str("canvas not found"))?
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
        .ok_or_else(|| JsValue::from_str("no 2d context"))?
        .dyn_into::<web_sys::CanvasRenderingContext2d>()?;
    raw_ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0)?;
    let mut ctx = LabRenderContext::new(raw_ctx, dpr, css_w, css_h);
    let screen_w = css_w as f64;
    let screen_h = css_h as f64;

    // Bug fix (owner report, 2026-08-07): nothing painted the full canvas
    // each frame. `theme::draw_lab_bar` only fills its own [0, BAR_HEIGHT)
    // strip and `render_primitive_settings_modal` only fills its own modal
    // frame rect — neither the uzor `Painter` trait nor `Canvas2dRenderContext`
    // exposes a `clear`/`clearRect` (checked: no such method in
    // `uzor/uzor/src/core/render/{painter,retained}.rs`), so whatever the
    // PREVIOUS frame painted outside this frame's shapes simply survived —
    // every pixel not repainted this frame was still the previous frame's
    // pixel. Switching from a taller layout (e.g. Style, with its stroke
    // colour/width/line-style/fill rows) to a shorter one (e.g. Levels
    // before it grows its own rows) left the taller frame's tail — and its
    // footer — visible below the new, shorter one; the debug line at a FIXED
    // y got two frames' text drawn on top of each other because it is
    // itself never cleared between draws. A full-surface fill up front,
    // using the theme's own chart background (so a theme switch repaints
    // the whole canvas, not just the widget chrome), fixes both.
    let bg = lab.theme_manager.current().chart.background.clone();
    ctx.set_fill_color(&bg);
    ctx.fill_rect(0.0, 0.0, screen_w, screen_h);

    let now_ms = win.performance().map(|p| p.now()).unwrap_or(0.0);
    let time_s = now_ms / 1000.0;

    // ── Drain DOM events ────────────────────────────────────────────────────
    let events = lab.provider.poll_events();
    for event in &events {
        lab.event_processor.process(event, &mut lab.input_state, time_s);
        match event {
            PlatformEvent::TextInput { text } => {
                let chars: Vec<char> = text.chars().collect();
                let LabState { primitive_settings_state, coordinator, primitives, .. } = &mut *lab;
                if let Some(prim) = lab_active_primitive(primitive_settings_state, primitives) {
                    for ch in chars {
                        dispatch::apply_char(ch, primitive_settings_state, prim, coordinator);
                    }
                }
            }
            PlatformEvent::KeyDown { key, .. } => {
                let LabState { primitive_settings_state, coordinator, primitives, .. } = &mut *lab;
                if let Some(prim) = lab_active_primitive(primitive_settings_state, primitives) {
                    if primitive_settings_state.color_picker.hex_editing {
                        dispatch::apply_hex_key(key, primitive_settings_state, prim);
                    } else if let Some(kp) = key_press_from(key) {
                        dispatch::apply_key(kp, primitive_settings_state, prim, coordinator);
                    }
                }
            }
            // Drag is driven directly off the raw pointer events, not off
            // `InputCoordinator::end_frame()`'s per-widget response flags —
            // see `dispatch`'s "Drag" section doc comment for why: those
            // flags never persist a drag's start across frames on this
            // `uzor` version, so every drag observed through them is a
            // same-frame no-op. `InputCoordinator::process_drag_start`
            // hit-tests itself (against the previous frame's registrations,
            // same one-frame lag every hit test here already has) and is
            // the documented call for exactly this: "call this from a
            // mouse-down handler".
            PlatformEvent::PointerDown { x, y, button } if *button == MouseButton::Left => {
                if let Some(wid) = lab.coordinator.process_drag_start(*x, *y) {
                    let LabState { primitive_settings_state, drag_geometry, .. } = &mut *lab;
                    dispatch::on_drag_start(wid.as_str(), *x, *y, primitive_settings_state, drag_geometry);
                }
            }
            PlatformEvent::PointerMoved { x, y } => {
                if let Some((wid, _, _)) = lab.coordinator.process_drag_move(*x, *y) {
                    let LabState { primitive_settings_state, primitives, drag_geometry, .. } = &mut *lab;
                    if let Some(prim) = lab_active_primitive(primitive_settings_state, primitives) {
                        dispatch::on_drag_move(wid.as_str(), *x, *y, primitive_settings_state, prim, drag_geometry);
                    }
                }
            }
            PlatformEvent::PointerUp { button, .. } if *button == MouseButton::Left => {
                if let Some(wid) = lab.coordinator.process_drag_end() {
                    let LabState { primitive_settings_state, primitives, .. } = &mut *lab;
                    if let Some(prim) = lab_active_primitive(primitive_settings_state, primitives) {
                        dispatch::on_drag_end(wid.as_str(), primitive_settings_state, prim);
                    }
                }
            }
            _ => {}
        }
    }

    lab.coordinator.begin_frame(lab.input_state.clone());

    theme::draw_lab_bar(
        &mut ctx,
        &mut lab.coordinator,
        screen_w,
        &lab.theme_manager,
        lab.primitive_settings_state.is_open(),
        variants::VARIANTS,
        lab.active_variant,
    );

    // The geometry the render call settled on this frame — `None` while the
    // modal is closed. Kept as a local (not a `LabState` field) so the
    // response loop below can borrow it immutably at the same time it takes
    // disjoint `&mut` borrows of `lab`'s fields.
    let mut prim_result: Option<PrimitiveSettingsResult> = None;

    if lab.primitive_settings_state.is_open() {
        // Populate the render-time text proxy from TextFieldStore — mirrors
        // `ChartApp::render_modals`'s caller in `mlc-app/src/lib.rs:8932-8954`.
        {
            let wid = WidgetId::from(PRIMITIVE_SETTINGS_FIELD);
            let tf = lab.coordinator.text_fields();
            let cursor_blink = tf.cursor_visible(now_ms as u64);
            if lab.primitive_settings_state.active_field_id.is_some() {
                lab.primitive_settings_state.active_render = ActiveFieldRender {
                    text: tf.text(&wid).to_string(),
                    cursor: tf.cursor(&wid),
                    selection_start: tf.selection_range(&wid).map(|(s, _)| s),
                    cursor_visible: cursor_blink,
                };
            } else {
                lab.primitive_settings_state.active_render = ActiveFieldRender::default();
            }
        }

        let frame_theme = theme::frame_theme(&lab.theme_manager);
        let toolbar_theme = theme::toolbar_theme(&lab.theme_manager);
        let toolbar_state = ToolbarState::default();

        // Templates are stored per-type; the render call only ever sees the
        // ones that apply to the primitive currently open.
        let active_type_id = lab
            .primitive_settings_state
            .primitive_id
            .and_then(|id| lab.primitives.iter().find(|p| p.data().id == id))
            .map(|p| p.type_id());
        let templates_for_render: Vec<PrimitiveTemplate> = match active_type_id {
            Some(t) => lab.templates.iter().filter(|tm| tm.type_id == t).cloned().collect(),
            None => Vec::new(),
        };

        let variant = variants::get(lab.active_variant);

        let LabState { primitive_settings_state, primitives, coordinator, drag_geometry, .. } = &mut *lab;
        let result = (variant.render)(
            &mut ctx,
            screen_w,
            screen_h,
            theme::BAR_HEIGHT + 20.0,
            primitive_settings_state,
            &primitives[..],
            &frame_theme,
            &toolbar_theme,
            &toolbar_state,
            now_ms as u64,
            coordinator,
            &templates_for_render,
        );

        // See `dispatch::register_dropdown_option_patch`: the line-style and
        // select-property dropdown OPTION rows this call just drew are not
        // registered as hit regions by `render_primitive_settings_modal`
        // itself — this closes that gap without touching mlc-app-layout.
        dispatch::register_dropdown_option_patch(coordinator, &result);

        // Every drag id (header/resize/sliders/scrollbar handle/colour-picker
        // opacity slider) is driven off raw pointer events, not off this
        // frame's responses — see `dispatch`'s "Drag" section. Those handlers
        // need this frame's geometry to seed the NEXT frame's drag, since
        // `PointerDown` for that drag arrives before the frame after this one
        // has rendered anything of its own.
        let mut color_opacity_slider_rect = None;
        if primitive_settings_state.is_color_picker_open() {
            let cp_result = render_primitive_color_picker_popup(&mut ctx, primitive_settings_state, &toolbar_theme, coordinator);
            color_opacity_slider_rect = cp_result
                .l1_result
                .as_ref()
                .and_then(|l1| l1.opacity_slider_rect)
                .or_else(|| cp_result.l2_result.as_ref().map(|l2| l2.opacity_slider_rect));
        }
        *drag_geometry = dispatch::DragGeometry {
            modal_rect: Some(result.modal_rect),
            slider_tracks: result.slider_tracks.clone(),
            scrollbar_track: result.scrollbar_track_rect,
            total_content_height: result.total_content_height,
            viewport_height: result.viewport_height,
            color_opacity_slider_rect,
        };

        prim_result = Some(result);
    }

    draw_debug_readout(&mut ctx, &*lab, screen_h);

    let color_picker_was_open = lab.primitive_settings_state.is_color_picker_open();
    let pointer = lab.input_state.pointer_pos();
    let raw_click = lab.input_state.pointer.clicked.is_some();
    let responses = lab.coordinator.end_frame();
    let mut color_picker_interacted = false;

    for (wid, resp) in &responses {
        // Drag ids are handled entirely off the raw pointer events above,
        // not off these responses — see `dispatch`'s "Drag" section — so
        // this loop only ever acts on `clicked` / `scrolled`. (`resp.dragged`
        // / `drag_started` / `drag_stopped` are read nowhere below; not
        // gating on them here too is deliberate, not an oversight — see the
        // crate's implementation report for the root cause.)
        if !resp.clicked && !resp.scrolled {
            continue;
        }
        let id = wid.as_str();

        if let Some(preset) = id.strip_prefix("lab:theme:") {
            if resp.clicked {
                lab.theme_manager.set_preset(preset);
            }
            continue;
        }
        if let Some(name) = id.strip_prefix("lab:variant:") {
            if resp.clicked {
                if let Some(idx) = variants::VARIANTS.iter().position(|v| v.name == name) {
                    lab.active_variant = idx;
                }
            }
            continue;
        }
        if id == theme::REOPEN_WIDGET_ID {
            if resp.clicked {
                if let Some(pid) = lab.primitives.first().map(|p| p.data().id) {
                    lab.primitive_settings_state.open(pid);
                }
            }
            continue;
        }

        if let Some(cp_id) = id.strip_prefix("color_picker_primitive:") {
            color_picker_interacted = true;
            let LabState { primitive_settings_state, primitives, .. } = &mut *lab;
            if let Some(prim) = lab_active_primitive(primitive_settings_state, primitives) {
                dispatch::apply_color_picker_response(cp_id, resp, primitive_settings_state, prim, pointer, screen_w, screen_h);
            }
            continue;
        }

        // Header drag, resize grips, the scrollbar HANDLE drag and slider
        // drags are NOT handled here — see the `PointerDown`/`PointerMoved`/
        // `PointerUp` arms above and `dispatch`'s "Drag" section doc comment.
        // `end_frame()`'s `dragged`/`drag_started`/`drag_stopped` fields for
        // those ids may still show up in `responses` (harmlessly — they
        // carry no usable state on this `uzor` version) and fall through
        // the checks below with no match, same as any other unrecognised id.
        if id == "prim_settings:scroll_viewport" || id == "prim_settings:scrollbar_track" {
            if let Some(ref result) = prim_result {
                dispatch::apply_scroll_response(
                    id,
                    resp,
                    &mut lab.primitive_settings_state,
                    pointer,
                    result.total_content_height,
                    result.viewport_height,
                    result.scrollbar_track_rect,
                );
            }
            continue;
        }

        let routed_id = id.strip_prefix("prim_settings:item:").unwrap_or(id);

        if resp.clicked {
            let LabState { primitive_settings_state, primitives, coordinator, templates, .. } = &mut *lab;
            if let Some(prim) = lab_active_primitive(primitive_settings_state, primitives) {
                let mut click_ctx = dispatch::ClickCtx { anchor: resp.rect, screen_w, screen_h, templates };
                dispatch::apply_primitive_settings_click(routed_id, primitive_settings_state, prim, coordinator, &mut click_ctx);
            }
        }
    }

    // The colour picker sits on its own modal-flagged layer (`ZLayer::ColorPicker`):
    // while it is open, a click that lands outside its own widgets hits
    // nothing at all (the modal barrier stops the search there — see
    // `theme.rs`'s `LAB_BAR_Z` doc comment for the same rule applied to the
    // lab bar). That is otherwise unrecoverable from inside a click handler,
    // since no widget response is ever generated for it — the raw pointer
    // click flag is the only signal left. Guarded by `color_picker_was_open`
    // (captured before this frame's clicks were applied) so the very click
    // that OPENS the picker cannot immediately close it again.
    if color_picker_was_open && raw_click && !color_picker_interacted {
        lab.primitive_settings_state.close_color_picker();
    }

    lab.input_state.scroll_delta = (0.0, 0.0);
    lab.input_state.end_frame();

    Ok(())
}

/// Borrow-splitting helper: `LabState`'s destructure pattern above already
/// gives disjoint `&mut` borrows of `primitive_settings_state` /
/// `coordinator`, but `primitives` still needs its own lookup by the id the
/// state carries — pulled out so every call site does it the same way.
#[cfg(target_arch = "wasm32")]
fn lab_active_primitive<'a>(
    state: &PrimitiveSettingsState,
    primitives: &'a mut [Box<dyn Primitive>],
) -> Option<&'a mut (dyn Primitive + 'static)> {
    let id = state.primitive_id?;
    primitives.iter_mut().find(|p| p.data().id == id).map(|p| p.as_mut())
}

/// Small live-state readout under the modal so a checkbox toggle / text edit
/// is verifiable even without a chart underneath to draw the primitive on.
#[cfg(target_arch = "wasm32")]
fn draw_debug_readout(ctx: &mut dyn mlc_core::render::RenderContext, lab: &LabState, screen_h: f64) {
    use mlc_core::render::{TextAlign, TextBaseline};
    let Some(prim) = lab
        .primitive_settings_state
        .primitive_id
        .and_then(|id| lab.primitives.iter().find(|p| p.data().id == id))
    else {
        return;
    };
    let data = prim.data();
    let show_prices = prim
        .style_properties()
        .into_iter()
        .find(|p| p.id == "show_prices")
        .and_then(|p| p.value.as_bool())
        .unwrap_or(false);
    let level0_visible = prim
        .level_configs()
        .and_then(|c| c.first().map(|l| l.visible))
        .unwrap_or(false);
    let line = format!(
        "live: width={:.2}  show_prices={}  level[0].visible={}  active_tab={}  theme={}  variant={}",
        data.width,
        show_prices,
        level0_visible,
        lab.primitive_settings_state.active_tab.id(),
        lab.theme_manager.preset_name(),
        variants::get(lab.active_variant).name,
    );
    ctx.set_font("12px monospace");
    ctx.set_fill_color("#9598a1");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Bottom);
    ctx.fill_text(&line, 8.0, screen_h - 8.0);
}
