//! `mlc-ui-lab` — wasm UI laboratory.
//!
//! Hosts the REAL `mlc-app-layout` modal builders (primitive settings, to
//! start) and real uzor widgets, driven by a real `uzor::input::InputCoordinator`,
//! in a standalone browser page. No `mlc-app`, no `mlc-backend`, no
//! `digdigdig3` — see `README.md` for the feasibility claims this rests on
//! and `Cargo.toml` for the exact dependency list.
//!
//! No chart, no toolbars, no chrome: this lab exists to compare *modal
//! designs* against an empty background. `crate::variants` is the registry
//! of named "draw a settings surface given the same inputs" functions
//! (`baseline`/`approved`/`templates`/empty) — all four are drawn EVERY
//! frame, one per sector of a 2x2 grid (see `sector_grid` below), so the
//! owner can judge them side by side instead of switching between them.

#[cfg(target_arch = "wasm32")]
mod approved_design;
#[cfg(target_arch = "wasm32")]
mod approved_levels;
#[cfg(target_arch = "wasm32")]
mod approved_placement;
#[cfg(target_arch = "wasm32")]
mod approved_vid;
#[cfg(target_arch = "wasm32")]
mod design_state;
#[cfg(target_arch = "wasm32")]
mod dispatch;
#[cfg(target_arch = "wasm32")]
mod level_presets;
#[cfg(target_arch = "wasm32")]
mod palettes;
#[cfg(target_arch = "wasm32")]
mod render_context;
#[cfg(target_arch = "wasm32")]
mod shell_common;
#[cfg(target_arch = "wasm32")]
mod templates_variant;
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
use uzor::render::{Masking, Painter, ShapeHelpers, TextRenderer};
#[cfg(target_arch = "wasm32")]
use uzor::types::Rect;
#[cfg(target_arch = "wasm32")]
use uzor_window_web::WebWindowProvider;

#[cfg(target_arch = "wasm32")]
use design_state::DesignUiState;

// ─── Sector grid ────────────────────────────────────────────────────────────

/// Scoped-region id per grid sector, in the same order as
/// `variants::VARIANTS` (`baseline`, `approved`, `templates`, empty). The id
/// doubles as the `"{id}:"` prefix every response coming out of that
/// sector's scoped region carries (`InputCoordinator::end_frame`'s own
/// convention).
#[cfg(target_arch = "wasm32")]
const SECTOR_IDS: [&str; 4] = ["sector_baseline", "sector_approved", "sector_templates", "sector_empty"];

/// The 2x2 grid: one rect per sector, each already carved down to the area
/// BELOW its own label (label height reserved, not part of the rect the
/// modal renders/registers into). Order matches `SECTOR_IDS`/`variants::VARIANTS`:
/// top-left, top-right, bottom-left, bottom-right.
#[cfg(target_arch = "wasm32")]
fn sector_grid(screen_w: f64, screen_h: f64) -> [Rect; 4] {
    const OUTER_PAD: f64 = 8.0;
    const GAP: f64 = 10.0;
    const LABEL_H: f64 = 20.0;

    let top = theme::BAR_HEIGHT + OUTER_PAD;
    let grid_w = (screen_w - OUTER_PAD * 2.0).max(200.0);
    let grid_h = (screen_h - top - OUTER_PAD).max(200.0);
    let cell_w = (grid_w - GAP) / 2.0;
    let cell_h = (grid_h - GAP) / 2.0;
    let cols = [OUTER_PAD, OUTER_PAD + cell_w + GAP];
    let rows = [top, top + cell_h + GAP];
    let body_h = (cell_h - LABEL_H).max(40.0);
    [
        Rect::new(cols[0], rows[0] + LABEL_H, cell_w, body_h),
        Rect::new(cols[1], rows[0] + LABEL_H, cell_w, body_h),
        Rect::new(cols[0], rows[1] + LABEL_H, cell_w, body_h),
        Rect::new(cols[1], rows[1] + LABEL_H, cell_w, body_h),
    ]
}

#[cfg(target_arch = "wasm32")]
fn sector_at(rects: &[Rect; 4], x: f64, y: f64) -> Option<usize> {
    rects.iter().position(|r| r.contains(x, y))
}

/// Push (or update) all four scoped regions on `coord` so this frame's
/// `begin_frame` propagates a correctly LOCAL pointer position into each —
/// must run before `coord.begin_frame(..)` for that propagation to see the
/// current rects (`InputCoordinator::begin_frame` only translates into
/// regions that already exist when it runs).
#[cfg(target_arch = "wasm32")]
fn sync_sector_regions(coord: &mut InputCoordinator, rects: &[Rect; 4]) {
    for (id, rect) in SECTOR_IDS.iter().zip(rects.iter()) {
        coord.push_scoped_region(id, *rect);
    }
}

// ─── Lab state ────────────────────────────────────────────────────────────────

/// Everything one grid sector owns: its own copy of the primitive-settings
/// modal state (open/closed, active tab, scroll position, colour picker,
/// text-edit focus, ...), the lab-only design UI state the approved design
/// and the templates panel need (`crate::design_state::DesignUiState`), and
/// the drag geometry captured from its own most recently rendered frame.
/// All four sectors point at the SAME primitive (`LabState::primitives`) —
/// only this bookkeeping is per-sector, which is what keeps an edit in one
/// sector visible in the other three while selection/inspector/palette/
/// scroll stay independent.
#[cfg(target_arch = "wasm32")]
struct Sector {
    state: PrimitiveSettingsState,
    design_ui: DesignUiState,
    drag_geometry: dispatch::DragGeometry,
}

#[cfg(target_arch = "wasm32")]
impl Sector {
    fn new(primitive_id: u64) -> Self {
        let mut state = PrimitiveSettingsState::new();
        state.open(primitive_id);
        state.active_tab = mlc_app_layout::ui::modal_settings::PrimitiveSettingsTab::Levels;
        Self { state, design_ui: DesignUiState::default(), drag_geometry: dispatch::DragGeometry::default() }
    }
}

/// One Fib Retracement primitive (the richest registered type —
/// Style/Text/Coordinates/Levels/Visibility, see
/// `PrimitiveRegistry::global()` at `mlc-core/src/drawing/primitives_v2/registry.rs:64`),
/// a real theme manager, a real `InputCoordinator` fed from real DOM events
/// via `WebWindowProvider`, an in-memory template store (no persistence
/// layer exists in this crate), and four [`Sector`]s — one per grid cell,
/// each hosting one of `variants::VARIANTS` against the SAME primitive.
#[cfg(target_arch = "wasm32")]
struct LabState {
    canvas_id: String,
    provider: WebWindowProvider,
    event_processor: EventProcessor,
    input_state: InputState,
    coordinator: InputCoordinator,
    theme_manager: ThemeManager,
    primitives: Vec<Box<dyn Primitive>>,
    templates: Vec<PrimitiveTemplate>,
    sectors: [Sector; 4],
    /// Which sector currently owns an in-flight drag (header/resize/sliders/
    /// scrollbar handle/colour-picker opacity slider) — drag continuation
    /// (`PointerMoved`/`PointerUp`) routes here regardless of where the
    /// pointer currently sits, exactly like the single-modal lab drove drag
    /// off `PointerDown`'s sector rather than re-resolving it every move.
    active_drag: Option<usize>,
    /// Which sector last received a click — used to blur/commit a text
    /// field left active in a DIFFERENT sector before routing a click to a
    /// new one (only one sector can hold keyboard focus at a time).
    focused_sector: Option<usize>,
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

        let mut coordinator = InputCoordinator::new();

        // One scoped region per sector, each with its OWN text-field store —
        // so the shared `PRIMITIVE_SETTINGS_FIELD` id used by every inline
        // sub-field edit never collides across sectors, with zero
        // namespacing needed. Placeholder rects: `sector_grid` overwrites
        // them every frame before anything reads pointer position through
        // them; the lab drives focus explicitly (`dispatch::start_editing_raw`
        // equivalents below) rather than click-to-focus.
        for id in SECTOR_IDS {
            let child = coordinator.push_scoped_region(id, Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 });
            child.register_text_field(
                PRIMITIVE_SETTINGS_FIELD,
                uzor::types::Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 },
                TextFieldConfig::text(),
            );
        }

        let sectors = [Sector::new(primitive_id), Sector::new(primitive_id), Sector::new(primitive_id), Sector::new(primitive_id)];

        Ok(Self {
            canvas_id: "chart".to_string(),
            provider,
            event_processor: EventProcessor::new(),
            input_state: InputState::new(),
            coordinator,
            theme_manager: ThemeManager::new(),
            primitives: vec![fib],
            templates: Vec::new(),
            sectors,
            active_drag: None,
            focused_sector: None,
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
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    web_sys::console::log_1(&"[mlc-ui-lab] booting".into());

    let lab = LabState::new()?;
    run_render_loop(lab)
}

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
    // Rebind to a plain `&mut LabState` up front — same reasoning as the
    // single-modal version of this file: disjoint-field borrow-splitting
    // below needs an ordinary struct place, not a fresh `RefMut::deref_mut`
    // at every access.
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

    let bg = lab.theme_manager.current().chart.background.clone();
    ctx.set_fill_color(&bg);
    ctx.fill_rect(0.0, 0.0, screen_w, screen_h);

    let now_ms = win.performance().map(|p| p.now()).unwrap_or(0.0);
    let time_s = now_ms / 1000.0;

    // This frame's sector layout — computed before any input handling so
    // every pointer-coordinate translation below (scoped-region propagation
    // AND the raw-pointer drag routing) uses the SAME rects the paint pass
    // will use later in this function. See the report's "registration
    // follows the paint" note for how this keeps the two in step.
    let sector_rects = sector_grid(screen_w, screen_h);
    sync_sector_regions(&mut lab.coordinator, &sector_rects);

    // ── Drain DOM events ────────────────────────────────────────────────────
    let events = lab.provider.poll_events();
    for event in &events {
        lab.event_processor.process(event, &mut lab.input_state, time_s);
        match event {
            PlatformEvent::TextInput { text } => {
                let chars: Vec<char> = text.chars().collect();
                if let Some(sidx) = lab.focused_sector {
                    let LabState { sectors, coordinator, primitives, templates, .. } = &mut *lab;
                    if let (Some(coord), Some(prim)) = (
                        coordinator.scoped_region_coordinator_mut(SECTOR_IDS[sidx]),
                        lab_active_primitive(&sectors[sidx].state, primitives),
                    ) {
                        for ch in chars {
                            dispatch::apply_char(ch, &mut sectors[sidx].state, prim, coord, templates, &mut sectors[sidx].design_ui);
                        }
                    }
                }
            }
            PlatformEvent::KeyDown { key, .. } => {
                if let Some(sidx) = lab.focused_sector {
                    let LabState { sectors, coordinator, primitives, templates, .. } = &mut *lab;
                    if let (Some(coord), Some(prim)) = (
                        coordinator.scoped_region_coordinator_mut(SECTOR_IDS[sidx]),
                        lab_active_primitive(&sectors[sidx].state, primitives),
                    ) {
                        if sectors[sidx].state.color_picker.hex_editing {
                            dispatch::apply_hex_key(key, &mut sectors[sidx].state, prim);
                        } else if let Some(kp) = key_press_from(key) {
                            dispatch::apply_key(kp, &mut sectors[sidx].state, prim, coord, templates, &mut sectors[sidx].design_ui);
                        }
                    }
                }
            }
            // Drag is driven directly off the raw pointer events, straight
            // against the OWNING sector's own child coordinator (never the
            // parent `lab.coordinator`) — see the module doc + report for
            // why: `InputCoordinator::process_drag_move`/`process_drag_end`
            // do not delegate into scoped regions (only `process_drag_start`
            // does), so driving these through the parent would silently drop
            // every drag after its first frame. Calling them directly on the
            // sector's own coordinator sidesteps that gap entirely.
            PlatformEvent::PointerDown { x, y, button } if *button == MouseButton::Left => {
                if let Some(sidx) = sector_at(&sector_rects, *x, *y) {
                    let (lx, ly) = (*x - sector_rects[sidx].x, *y - sector_rects[sidx].y);
                    let LabState { coordinator, sectors, .. } = &mut *lab;
                    if let Some(child) = coordinator.scoped_region_coordinator_mut(SECTOR_IDS[sidx]) {
                        if let Some(wid) = child.process_drag_start(lx, ly) {
                            let sector = &mut sectors[sidx];
                            dispatch::on_drag_start(wid.as_str(), lx, ly, &mut sector.state, &mut sector.drag_geometry);
                            lab.active_drag = Some(sidx);
                        }
                    }
                }
            }
            PlatformEvent::PointerMoved { x, y } => {
                if let Some(sidx) = lab.active_drag {
                    let (lx, ly) = (*x - sector_rects[sidx].x, *y - sector_rects[sidx].y);
                    let LabState { coordinator, sectors, primitives, .. } = &mut *lab;
                    if let Some(child) = coordinator.scoped_region_coordinator_mut(SECTOR_IDS[sidx]) {
                        if let Some((wid, _, _)) = child.process_drag_move(lx, ly) {
                            let sector = &mut sectors[sidx];
                            if let Some(prim) = lab_active_primitive(&sector.state, primitives) {
                                dispatch::on_drag_move(wid.as_str(), lx, ly, &mut sector.state, prim, &sector.drag_geometry);
                            }
                        }
                    }
                }
            }
            PlatformEvent::PointerUp { button, .. } if *button == MouseButton::Left => {
                if let Some(sidx) = lab.active_drag.take() {
                    let LabState { coordinator, sectors, primitives, .. } = &mut *lab;
                    if let Some(child) = coordinator.scoped_region_coordinator_mut(SECTOR_IDS[sidx]) {
                        if let Some(wid) = child.process_drag_end() {
                            let sector = &mut sectors[sidx];
                            if let Some(prim) = lab_active_primitive(&sector.state, primitives) {
                                dispatch::on_drag_end(wid.as_str(), &mut sector.state, prim, &mut sector.design_ui);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    lab.coordinator.begin_frame(lab.input_state.clone());

    let any_sector_closed = lab.sectors.iter().any(|s| !s.state.is_open());
    theme::draw_lab_bar(&mut ctx, &mut lab.coordinator, screen_w, &lab.theme_manager, any_sector_closed);

    // Templates are stored per-type; every sector shows the same primitive,
    // so this is computed once rather than per sector.
    let active_type_id = lab.primitives.first().map(|p| p.type_id());
    let templates_for_render: Vec<PrimitiveTemplate> = match active_type_id {
        Some(t) => lab.templates.iter().filter(|tm| tm.type_id == t).cloned().collect(),
        None => Vec::new(),
    };

    let frame_theme = theme::frame_theme(&lab.theme_manager);
    let toolbar_theme = theme::toolbar_theme(&lab.theme_manager);
    let toolbar_state = ToolbarState::default();

    // Captured before this frame's clicks are applied — the colour-picker
    // outside-click-closes check below needs the PRE-click state per sector.
    let color_picker_was_open: [bool; 4] = std::array::from_fn(|i| lab.sectors[i].state.is_color_picker_open());

    for (i, sector_id) in SECTOR_IDS.iter().enumerate() {
        let region = sector_rects[i];
        let label_y = region.y - 14.0;
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(mlc_core::render::TextAlign::Left);
        ctx.set_text_baseline(mlc_core::render::TextBaseline::Bottom);
        ctx.fill_text(variants::VARIANTS[i].name, region.x + 2.0, label_y + 12.0);

        ctx.save();
        ctx.begin_path();
        ctx.rect(region.x, region.y, region.width, region.height);
        ctx.clip();
        ctx.translate(region.x, region.y);

        if lab.sectors[i].state.is_open() {
            // Render-time text proxy from TextFieldStore, mirroring
            // `ChartApp::render_modals`'s caller — same shape as the
            // single-modal lab, resolved against THIS sector's own child
            // coordinator (its own, unshared, `TextFieldStore`).
            {
                let LabState { coordinator, sectors, .. } = &mut *lab;
                if let Some(coord) = coordinator.scoped_region_coordinator_mut(sector_id) {
                    let wid = uzor::WidgetId::from(PRIMITIVE_SETTINGS_FIELD);
                    let tf = coord.text_fields();
                    let cursor_blink = tf.cursor_visible(now_ms as u64);
                    let sector_state = &mut sectors[i].state;
                    if sector_state.active_field_id.is_some() {
                        sector_state.active_render = ActiveFieldRender {
                            text: tf.text(&wid).to_string(),
                            cursor: tf.cursor(&wid),
                            selection_start: tf.selection_range(&wid).map(|(s, _)| s),
                            cursor_visible: cursor_blink,
                        };
                    } else {
                        sector_state.active_render = ActiveFieldRender::default();
                    }
                }
            }

            let LabState { coordinator, sectors, primitives, .. } = &mut *lab;
            let Some(coord) = coordinator.scoped_region_coordinator_mut(sector_id) else {
                ctx.restore();
                continue;
            };
            let sector = &mut sectors[i];

            let variant = &variants::VARIANTS[i];
            let result = (variant.render)(
                &mut ctx,
                region.width,
                region.height,
                8.0,
                &sector.state,
                &primitives[..],
                &frame_theme,
                &toolbar_theme,
                &toolbar_state,
                now_ms as u64,
                coord,
                &templates_for_render,
                &mut sector.design_ui,
            );

            dispatch::register_dropdown_option_patch(coord, &result);

            let mut color_opacity_slider_rect = None;
            if sector.state.is_color_picker_open() {
                let cp_result = render_primitive_color_picker_popup(&mut ctx, &sector.state, &toolbar_theme, coord);
                color_opacity_slider_rect = cp_result
                    .l1_result
                    .as_ref()
                    .and_then(|l1| l1.opacity_slider_rect)
                    .or_else(|| cp_result.l2_result.as_ref().map(|l2| l2.opacity_slider_rect));
            }

            sector.drag_geometry = dispatch::DragGeometry {
                modal_rect: Some(result.modal_rect),
                slider_tracks: result.slider_tracks.clone(),
                scrollbar_track: result.scrollbar_track_rect,
                total_content_height: result.total_content_height,
                viewport_height: result.viewport_height,
                color_opacity_slider_rect,
            };
        }

        ctx.restore();
    }

    let hovered_sector = lab
        .input_state
        .pointer
        .pos
        .and_then(|(x, y)| sector_at(&sector_rects, x, y))
        .map(|i| variants::VARIANTS[i].name);
    draw_debug_readout(&mut ctx, lab, screen_h, hovered_sector);

    let raw_click = lab.input_state.pointer.clicked.is_some();
    let responses = lab.coordinator.end_frame();
    let mut color_picker_interacted = [false; 4];

    for (wid, resp) in &responses {
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
        if id == theme::REOPEN_WIDGET_ID {
            if resp.clicked {
                if let Some(pid) = lab.primitives.first().map(|p| p.data().id) {
                    for sector in &mut lab.sectors {
                        if !sector.state.is_open() {
                            sector.state.open(pid);
                            sector.state.active_tab = mlc_app_layout::ui::modal_settings::PrimitiveSettingsTab::Levels;
                        }
                    }
                }
            }
            continue;
        }

        let Some((sector_prefix, rest_id)) = id.split_once(':').and_then(|(p, r)| {
            SECTOR_IDS.iter().position(|s| *s == p).map(|i| (i, r))
        }) else {
            continue;
        };
        let sidx = sector_prefix;
        let pointer = lab
            .input_state
            .pointer
            .pos
            .map(|(x, y)| (x - sector_rects[sidx].x, y - sector_rects[sidx].y));

        if let Some(cp_id) = rest_id.strip_prefix("color_picker_primitive:") {
            color_picker_interacted[sidx] = true;
            let LabState { sectors, primitives, .. } = &mut *lab;
            let sector = &mut sectors[sidx];
            if let Some(prim) = lab_active_primitive(&sector.state, primitives) {
                dispatch::apply_color_picker_response(
                    cp_id, resp, &mut sector.state, prim, pointer, sector_rects[sidx].width, sector_rects[sidx].height,
                );
            }
            continue;
        }

        if rest_id == "prim_settings:scroll_viewport" || rest_id == "prim_settings:scrollbar_track" {
            let sector = &mut lab.sectors[sidx];
            let geo = sector.drag_geometry.clone();
            dispatch::apply_scroll_response(rest_id, resp, &mut sector.state, pointer, geo.total_content_height, geo.viewport_height, geo.scrollbar_track);
            continue;
        }

        let routed_id = rest_id.strip_prefix("prim_settings:item:").unwrap_or(rest_id);

        if resp.clicked {
            if lab.focused_sector != Some(sidx) {
                if let Some(prev_idx) = lab.focused_sector.take() {
                    let LabState { coordinator, sectors, primitives, templates, .. } = &mut *lab;
                    if let Some(prev_coord) = coordinator.scoped_region_coordinator_mut(SECTOR_IDS[prev_idx]) {
                        if let Some(prim) = lab_active_primitive(&sectors[prev_idx].state, primitives) {
                            dispatch::commit_active_field(&mut sectors[prev_idx].state, prim, prev_coord, templates, &mut sectors[prev_idx].design_ui);
                        }
                    }
                }
                lab.focused_sector = Some(sidx);
            }

            let LabState { coordinator, sectors, primitives, templates, .. } = &mut *lab;
            let Some(coord) = coordinator.scoped_region_coordinator_mut(SECTOR_IDS[sidx]) else { continue };
            let sector = &mut sectors[sidx];
            if let Some(prim) = lab_active_primitive(&sector.state, primitives) {
                let mut click_ctx = dispatch::ClickCtx {
                    anchor: resp.rect,
                    screen_w: sector_rects[sidx].width,
                    screen_h: sector_rects[sidx].height,
                    templates,
                };
                let handled = dispatch::apply_approved_design_click(
                    routed_id, &mut sector.state, prim, coord, &mut click_ctx, &mut sector.design_ui,
                );
                if !handled {
                    dispatch::apply_primitive_settings_click(routed_id, &mut sector.state, prim, coord, &mut click_ctx, &mut sector.design_ui);
                }
            }
        }
    }

    for i in 0..4 {
        if color_picker_was_open[i] && raw_click && !color_picker_interacted[i] {
            lab.sectors[i].state.close_color_picker();
        }
    }

    lab.input_state.scroll_delta = (0.0, 0.0);
    lab.input_state.end_frame();

    Ok(())
}

/// Borrow-splitting helper: look up the primitive a sector's state points
/// at, by id, inside the shared `primitives` list.
#[cfg(target_arch = "wasm32")]
fn lab_active_primitive<'a>(
    state: &PrimitiveSettingsState,
    primitives: &'a mut [Box<dyn Primitive>],
) -> Option<&'a mut (dyn Primitive + 'static)> {
    let id = state.primitive_id?;
    primitives.iter_mut().find(|p| p.data().id == id).map(|p| p.as_mut())
}

/// Small live-state readout under the grid so a checkbox toggle / text edit
/// is verifiable even without a chart underneath to draw the primitive on.
/// Names whichever sector the pointer is currently over instead of a single
/// "active variant" — there is no such thing any more, all four render
/// every frame.
#[cfg(target_arch = "wasm32")]
fn draw_debug_readout(ctx: &mut dyn mlc_core::render::RenderContext, lab: &LabState, screen_h: f64, hovered_sector: Option<&str>) {
    use mlc_core::render::{TextAlign, TextBaseline};
    let Some(prim) = lab.primitives.first() else { return };
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
        "live: width={:.2}  show_prices={}  level[0].visible={}  theme={}  sector={}",
        data.width,
        show_prices,
        level0_visible,
        lab.theme_manager.preset_name(),
        hovered_sector.unwrap_or("-"),
    );
    ctx.set_font("12px monospace");
    ctx.set_fill_color("#9598a1");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Bottom);
    ctx.fill_text(&line, 8.0, screen_h - 8.0);
}
