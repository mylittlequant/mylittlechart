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
mod app_modals;
#[cfg(target_arch = "wasm32")]
mod approved_design;
#[cfg(target_arch = "wasm32")]
pub mod concept;
#[cfg(target_arch = "wasm32")]
mod dispatch;
#[cfg(target_arch = "wasm32")]
mod render_context;
#[cfg(target_arch = "wasm32")]
mod shell;
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
use app_modals::{AppModalKind, AppModalsState};

#[cfg(target_arch = "wasm32")]
use concept::state::ConceptModalState;

#[cfg(target_arch = "wasm32")]
use mlc_app_layout::layout::modals::primitive_settings::{
    render_primitive_color_picker_popup, render_primitive_settings_modal,
};
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
/// text-edit focus, and — since the 2026-08-08 port folded it in — the
/// Levels/Placement tab's own session bookkeeping too), and the drag
/// geometry captured from its own most recently rendered frame. All four
/// sectors point at the SAME primitive (`LabState::primitives`) — only this
/// bookkeeping is per-sector, which is what keeps an edit in one sector
/// visible in the other three while selection/inspector/scroll stay
/// independent.
#[cfg(target_arch = "wasm32")]
struct Sector {
    state: PrimitiveSettingsState,
    drag_geometry: dispatch::DragGeometry,
}

#[cfg(target_arch = "wasm32")]
impl Sector {
    fn new(primitive_id: u64) -> Self {
        let mut state = PrimitiveSettingsState::new();
        state.open(primitive_id);
        state.active_tab = mlc_app_layout::ui::modal_settings::PrimitiveSettingsTab::Levels;
        Self { state, drag_geometry: dispatch::DragGeometry::default() }
    }
}

/// Concept-modal viewport geometry captured from the most recently
/// rendered frame — pane-LOCAL (the concept modal draws under the same
/// `ctx.translate(pane_rect.x, pane_rect.y)` the tree/app modal families
/// use, see their own render call sites' doc). `content_rect` is the
/// scroll viewport the wheel routing (`PlatformEvent::Scroll`, below)
/// hit-tests the pointer against; `total_content_height`/`viewport_height`
/// feed `ScrollState::handle_wheel` the same pair `dispatch::DragGeometry`
/// carries for the production modal. `modal_rect` is the outer frame, read
/// by the agent control plane (`lab_metrics`). Drag/resize are NOT wired
/// for the concept modal (out of this pass's scope — see the crate's own
/// report), so nothing here ever feeds `dispatch::on_drag_*`.
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Default)]
struct ConceptModalGeo {
    modal_rect: uzor::types::Rect,
    content_rect: uzor::types::Rect,
    total_content_height: f64,
    viewport_height: f64,
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
    /// The stub app shell (toolbars/rails/pane/Object Tree) drawn every
    /// frame, below whichever surfaces the lab currently overlays on top
    /// of it. See `shell::draw_shell`'s own doc.
    shell_state: shell::ShellState,
    /// Whether the primitive-settings modal comparison grid
    /// (`variants::VARIANTS`, drawn via the `sectors` loop below) is
    /// showing on top of the shell. Lives here rather than on
    /// `shell::ShellState` because it gates a whole DIFFERENT surface
    /// (`crate::variants`) this lab hosts, not a piece of shell chrome —
    /// toggled by the shell's own "Modals" button
    /// (`shell::MODALS_TOGGLE_ID`).
    show_variants: bool,
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

    // ─── Object Tree (wave 2) ───────────────────────────────────────────
    /// Object Tree's own primitives — one of each registered primitive
    /// type (`shell::object_tree::spawnable_types()`, ~121 as of this
    /// writing, wave 5 — was the curated 14 left-rail types), the tree's
    /// own spawn/delete surface thereafter (`spawn_tree_object`/
    /// `delete_tree_object`). Separate from `primitives` above (wave 1's
    /// own single Fib Retracement, still driving the four-sector
    /// design-comparison grid) — Object Tree rows operate on this list
    /// exclusively.
    tree_objects: Vec<Box<dyn Primitive>>,
    /// Selected Object Tree row — a `tree_objects` primitive id, or
    /// `None`. Drives the bottom toolbar's name label and the chart
    /// pane's schematic placeholder (see `shell::draw_shell`'s own
    /// `selected_name`).
    tree_selected: Option<u64>,
    /// Next id to stamp into a freshly spawned tree object's own
    /// `PrimitiveData::id`. `mlc-core` primitives default to id 0 — a real
    /// `DrawingManager` hands out ids on insert (out of this crate's
    /// reach, see crate contracts); the lab plays that same role for its
    /// own list.
    next_tree_id: u64,
    /// The Object Tree's own gear-triggered settings modal: the SAME real
    /// `render_primitive_settings_modal` the four-sector grid uses
    /// (`variants::VARIANTS[0].render`), opened full-screen (not
    /// sectored) for whichever tree row's gear — or the bottom toolbar's
    /// own settings icon — was clicked.
    tree_modal: PrimitiveSettingsState,
    /// Drag geometry captured from the tree modal's own most recently
    /// rendered frame — same role as `Sector::drag_geometry`, kept
    /// separate because the tree modal is not one of the four `sectors`.
    tree_modal_drag: dispatch::DragGeometry,
    /// Whether a drag (header/resize/sliders/scrollbar/colour-picker
    /// opacity) is in flight against the tree modal — same role as
    /// `active_drag` above, boolean because there is only one tree modal.
    tree_active_drag: bool,
    /// Whether the tree modal currently holds keyboard focus on its own
    /// inline text field — same role as `focused_sector` above, boolean
    /// for the same reason `tree_active_drag` is.
    tree_modal_focused: bool,

    // ─── Application modals + launcher menu (wave 3) ───────────────────
    /// State for every application-level modal (chart/indicator settings,
    /// watchlist, alerts, ...) plus which one is open — see
    /// `app_modals`'s own module doc.
    app_modals: AppModalsState,
    /// Whether the shell's "Modals" launcher dropdown
    /// (`shell::launcher_menu`) is currently open. Lives here rather than
    /// on `shell::ShellState` for the same reason `show_variants` does —
    /// it gates a whole separate surface this lab hosts, not a piece of
    /// shell chrome.
    launcher_open: bool,

    // ─── Concept modal (approved-design surface, 2026-08-10) ───────────
    /// State for the approved-design concept modal
    /// (`concept::render::render_concept_modal`) — the 14 object
    /// archetypes' and 9 application modals' own open/tab/row-value/scroll
    /// bookkeeping (`concept::spec`). Mutually exclusive with `tree_modal`
    /// (the production primitive-settings modal) and `app_modals` (the
    /// production application-modal family) — opening one closes the
    /// other two, the same one-modal-family-at-a-time contract
    /// `open_tree_modal`/`open_app_modal` already keep between themselves.
    concept_modal: ConceptModalState,
    /// This frame's concept-modal geometry, captured after each render —
    /// see [`ConceptModalGeo`]'s own doc.
    concept_modal_geo: ConceptModalGeo,

    // ─── Agent control plane (wave 4) ──────────────────────────────────
    /// This frame's chart pane rect (`pane_rect` in `render_one_frame`),
    /// mirrored here so `lab_metrics` (called from JS, between frames, with
    /// no `screen_w`/`screen_h` of its own) can convert the tree/app modal
    /// families' own captured rects — pane-LOCAL, since both are now drawn
    /// under `ctx.translate(pane_rect.x, pane_rect.y)`, see those render
    /// calls' own doc — back to viewport-absolute coordinates, matching
    /// `sim.js`'s own `getBoundingClientRect()`-sourced numbers.
    last_pane_rect: uzor::types::Rect,
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

        // Object Tree's own scoped region — always full screen (updated
        // every frame in `render_one_frame`, see `sync_sector_regions`'s
        // sibling call site), with its own text-field store so the shared
        // `PRIMITIVE_SETTINGS_FIELD` id cannot collide with the four
        // sectors' own copies (same reasoning as the loop above).
        let tree_modal_child = coordinator.push_scoped_region("objtree_modal", Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 });
        tree_modal_child.register_text_field(
            PRIMITIVE_SETTINGS_FIELD,
            uzor::types::Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 },
            TextFieldConfig::text(),
        );

        // The application-modals region (wave 3, `app_modals::REGION_ID`) —
        // one shared full-screen region for whichever of the 15 shell-based
        // app modals is open at a time (see that module's own doc for why
        // this is one region, not fifteen). Three of them own a text field
        // this crate must register up front, same reasoning as the loop
        // above and `tree_modal_child`.
        let app_modal_child = coordinator.push_scoped_region(app_modals::REGION_ID, Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 });
        for field_id in [app_modals::SEARCH_FIELD, app_modals::TEMPLATE_NAME_FIELD, app_modals::PRESET_NAME_FIELD] {
            app_modal_child.register_text_field(
                field_id,
                uzor::types::Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 },
                TextFieldConfig::text(),
            );
        }

        // The launcher menu's own region (wave 3, `shell::launcher_menu::
        // REGION_ID`) — pure clicks, no text field of its own.
        coordinator.push_scoped_region(shell::launcher_menu::REGION_ID, Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 });

        // The concept modal's own region (2026-08-10, `concept::REGION_ID`)
        // — pure clicks, no text field of its own (every concept-modal row
        // this task wires is a toggle/segment/stepper, never free text).
        coordinator.push_scoped_region(concept::REGION_ID, Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 });

        // One of every REGISTERED type (wave 5, 2026-08-10 — was the
        // curated 14 left-rail types) — mirrors `sim.js`'s own
        // `spawnAll()`: the Object Tree starts populated, not empty (see
        // that function's own doc for why). No points supplied (this lab
        // has no data plane); every registered factory defaults sensibly
        // on an empty slice (verified against `PrimitiveRegistry`'s own
        // `hydration`/`hit_test_safety_net` test modules, which construct
        // every registered type this same way).
        let mut tree_objects: Vec<Box<dyn Primitive>> = Vec::new();
        let mut next_tree_id = 1u64;
        {
            let registry = PrimitiveRegistry::global()
                .read()
                .map_err(|_| JsValue::from_str("primitive registry lock poisoned"))?;
            for type_id in shell::object_tree::spawnable_types() {
                let mut prim = registry
                    .create(type_id, &[], None)
                    .ok_or_else(|| JsValue::from_str(&format!("{type_id} not registered")))?;
                prim.data_mut().id = next_tree_id;
                next_tree_id += 1;
                tree_objects.push(prim);
            }
        }
        let tree_selected = tree_objects.first().map(|p| p.data().id);

        Ok(Self {
            canvas_id: "chart".to_string(),
            provider,
            event_processor: EventProcessor::new(),
            input_state: InputState::new(),
            coordinator,
            theme_manager: ThemeManager::new(),
            primitives: vec![fib],
            templates: Vec::new(),
            shell_state: shell::ShellState::default(),
            show_variants: false,
            sectors,
            active_drag: None,
            focused_sector: None,
            tree_objects,
            tree_selected,
            next_tree_id,
            tree_modal: PrimitiveSettingsState::new(),
            tree_modal_drag: dispatch::DragGeometry::default(),
            tree_active_drag: false,
            tree_modal_focused: false,
            app_modals: AppModalsState::new(),
            launcher_open: false,
            concept_modal: ConceptModalState::new(),
            concept_modal_geo: ConceptModalGeo::default(),
            last_pane_rect: uzor::types::Rect::default(),
        })
    }
}

// ─── Object Tree mutation helpers (wave 2) ─────────────────────────────────

/// Create one more `type_id` primitive (mirrors `sim.js`'s own `spawn()`):
/// suffix its display name with a running count when it is not the type's
/// first instance ("Fib Retracement 2", ...), stamp it with the lab's own
/// next id (see `LabState::next_tree_id`'s doc), push it onto
/// `tree_objects`, and select it. A no-op if `type_id` is not registered.
#[cfg(target_arch = "wasm32")]
fn spawn_tree_object(lab: &mut LabState, type_id: &str) {
    let created = PrimitiveRegistry::global().read().ok().and_then(|reg| reg.create(type_id, &[], None));
    let Some(mut prim) = created else { return };

    let existing = lab.tree_objects.iter().filter(|p| p.type_id() == type_id).count();
    if existing > 0 {
        let base = prim.data().display_name.clone();
        prim.data_mut().display_name = format!("{base} {}", existing + 1);
    }

    let id = lab.next_tree_id;
    lab.next_tree_id += 1;
    prim.data_mut().id = id;

    lab.tree_objects.push(prim);
    lab.tree_selected = Some(id);
}

/// Remove one tree object by id (mirrors `sim.js`'s own row `del` action
/// and `#bot`'s own delete button) — clears selection and closes the tree
/// modal if either pointed at the removed object.
#[cfg(target_arch = "wasm32")]
fn delete_tree_object(lab: &mut LabState, id: u64) {
    lab.tree_objects.retain(|p| p.data().id != id);
    if lab.tree_selected == Some(id) {
        lab.tree_selected = None;
    }
    if lab.tree_modal.primitive_id == Some(id) {
        lab.tree_modal.close();
        lab.tree_modal_focused = false;
    }
}

/// Open the tree modal for one object (mirrors `sim.js`'s own `openModal`)
/// — selects it too, and defensively drops any stale focus/field state a
/// PREVIOUSLY open object might have left behind (`PrimitiveSettingsState::
/// open` does not clear `active_field_id` itself — see its own doc). Closes
/// any open application modal first — "одновременно открыта одна модалка"
/// (wave 3) applies across BOTH modal families, not just within each.
#[cfg(target_arch = "wasm32")]
fn open_tree_modal(lab: &mut LabState, id: u64) {
    lab.app_modals.close();
    lab.concept_modal.close();
    lab.tree_selected = Some(id);
    lab.tree_modal.active_field_id = None;
    lab.tree_modal_focused = false;
    lab.tree_modal.open(id);
}

/// Open one application-level modal, closing the tree modal first — see
/// `open_tree_modal`'s own doc for why the two families are mutually
/// exclusive.
#[cfg(target_arch = "wasm32")]
fn open_app_modal(lab: &mut LabState, kind: AppModalKind) {
    lab.tree_modal.close();
    lab.tree_modal_focused = false;
    lab.concept_modal.close();
    let Some(coord) = lab.coordinator.scoped_region_coordinator_mut(app_modals::REGION_ID) else { return };
    lab.app_modals.open_kind(kind, coord);
}

/// Open the concept-design modal (`concept::render::render_concept_modal`)
/// for `type_id` — the CONCEPT counterpart to [`open_tree_modal`], for the
/// approved-design surface instead of the live-`Primitive` one. Closes the
/// production tree modal and any open application modal first (only one
/// modal family open at a time, mirroring `open_tree_modal`/
/// `open_app_modal`'s own contract), and selects the Object Tree's own
/// matching row so the bottom bar and chart-pane placeholder stay in sync
/// with whichever archetype's concept modal is open. A no-op if `type_id`
/// does not name one of `concept::spec::types()`'s own 14 entries.
#[cfg(target_arch = "wasm32")]
fn open_concept_type_modal(lab: &mut LabState, type_id: &str) -> bool {
    let Some(concept_type) = concept::spec::types().iter().find(|t| t.id == type_id) else { return false };
    lab.tree_modal.close();
    lab.tree_modal_focused = false;
    lab.app_modals.close();
    lab.tree_selected = lab.tree_objects.iter().find(|p| p.type_id() == type_id).map(|p| p.data().id);
    lab.concept_modal.open_type(concept_type);
    true
}

/// Open the concept-design modal for one of the 9 application-level
/// archetypes (`concept::spec::app_modals()`) — the same surface
/// [`open_concept_type_modal`] opens for an object type, keyed by
/// `ConceptModal::id` instead. Not reachable from the shell UI in this
/// pass (no toolbar button routes here — see the crate's own report); only
/// the agent control plane (`lab_open_type`) calls this today.
#[cfg(target_arch = "wasm32")]
fn open_concept_app_modal(lab: &mut LabState, modal_id: &str) -> bool {
    let Some(modal_spec) = concept::spec::app_modals().iter().find(|m| m.id == modal_id) else { return false };
    lab.tree_modal.close();
    lab.tree_modal_focused = false;
    lab.app_modals.close();
    lab.concept_modal.open_modal(modal_spec);
    true
}

/// Select an existing Object Tree entry of `type_id` and open its
/// PRODUCTION settings modal (the live-`Primitive`-backed
/// `render_primitive_settings_modal`), or — if the owner deleted its only
/// instance — spawn a fresh one first. Backs the launcher menu's own
/// "Production modals (current code)" section (see
/// `shell::launcher_menu`'s own module doc for why select-first). The
/// CONCEPT counterpart is [`open_concept_type_modal`].
#[cfg(target_arch = "wasm32")]
fn open_object_modal_for_type(lab: &mut LabState, type_id: &str) {
    let existing = lab.tree_objects.iter().find(|p| p.type_id() == type_id).map(|p| p.data().id);
    let id = match existing {
        Some(id) => id,
        None => {
            spawn_tree_object(lab, type_id);
            match lab.tree_selected {
                Some(id) => id,
                None => return,
            }
        }
    };
    open_tree_modal(lab, id);
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
    // Publish the same `Rc` the render loop drives to the agent control
    // plane's thread-local — see `LAB_HANDLE`'s own doc.
    LAB_HANDLE.with(|handle| *handle.borrow_mut() = Some(state.clone()));

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

    // The chart pane's own rect — toolbars and (when open) the Object Tree
    // sidebar excluded, same geometry `shell::draw_shell` returns later
    // this frame (see `shell::chart_pane_rect`'s own doc for why this is
    // computed via that shared helper instead of re-derived by hand: the
    // tree-gear modal and every application modal (wave 4, owner report
    // 2026-08-10) are sized/centred INSIDE this rect rather than the full
    // screen, so they can never reach the sidebar's own pixels regardless
    // of viewport width — see the render calls below). Needed this early
    // (before `begin_frame`) only to size `objtree_modal`/`app_modal`'s own
    // scoped regions and to translate this frame's raw pointer events for
    // `objtree_modal`'s hand-driven drag routing — the render calls that
    // actually consume this geometry happen later, off `shell::draw_shell`'s
    // own return value (identical, since nothing mutates `shell_state`
    // in between).
    let modal_sidebar_open = lab.shell_state.active_right_panel == Some("object_tree");
    let modal_pane_rect =
        shell::chart_pane_rect(theme::BAR_HEIGHT, screen_w, (screen_h - theme::BAR_HEIGHT).max(0.0), modal_sidebar_open);

    // `objtree_modal`/`app_modal` scoped regions are sized to the chart
    // pane (not the full screen) so `InputCoordinator::begin_frame`'s own
    // per-region pointer translation lands in the SAME local coordinate
    // space the modal renderers below register their widgets in (they are
    // drawn under a matching `ctx.translate(modal_pane_rect.x,
    // modal_pane_rect.y)` — see the render calls' own doc). Updated every
    // frame for the same reason the four sectors are (`sync_sector_regions`'s
    // own doc): `begin_frame` only propagates into a region that already
    // has this frame's rect.
    lab.coordinator.push_scoped_region("objtree_modal", modal_pane_rect);
    lab.coordinator.push_scoped_region(app_modals::REGION_ID, modal_pane_rect);
    // The concept modal's own region — same pane-narrowing reasoning as
    // `objtree_modal`/`app_modal` above (see this frame-setup block's own
    // top-of-function doc); the concept modal draws inside the same
    // `pane_rect` those two do.
    lab.coordinator.push_scoped_region(concept::REGION_ID, modal_pane_rect);
    // The launcher menu is NOT narrowed — it is a top-toolbar dropdown
    // meant to draw over everything below it, including the sidebar (see
    // `shell::launcher_menu`'s own module doc), so it keeps the full-screen
    // region it always had.
    lab.coordinator.push_scoped_region(shell::launcher_menu::REGION_ID, Rect::new(0.0, 0.0, screen_w, screen_h));

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
                            dispatch::apply_char(ch, &mut sectors[sidx].state, prim, coord, templates);
                        }
                    }
                } else if lab.tree_modal_focused {
                    let LabState { coordinator, tree_modal, tree_objects, templates, .. } = &mut *lab;
                    if let (Some(coord), Some(prim)) = (
                        coordinator.scoped_region_coordinator_mut("objtree_modal"),
                        lab_active_primitive(tree_modal, tree_objects),
                    ) {
                        for ch in chars {
                            dispatch::apply_char(ch, tree_modal, prim, coord, templates);
                        }
                    }
                } else if lab.app_modals.is_open() {
                    // Application modals read text/cursor/selection LIVE off
                    // their own `TextFieldStore` inside their own render call
                    // (search overlay, template-name, preset-name — no
                    // render-time proxy struct the way the primitive-settings
                    // modal needs, see `app_modals`'s own module doc) — so
                    // forwarding raw chars into this region's own coordinator
                    // is the whole wiring needed, no commit step of ours.
                    if let Some(coord) = lab.coordinator.scoped_region_coordinator_mut(app_modals::REGION_ID) {
                        for ch in chars {
                            let _ = coord.on_char(ch);
                        }
                    }
                }
            }
            PlatformEvent::KeyDown { key, .. }
                if matches!(key, KeyCode::Escape)
                    && (lab.launcher_open
                        || lab.app_modals.is_open()
                        || lab.concept_modal.is_open()
                        || (lab.tree_modal.is_open() && !lab.tree_modal_focused)) =>
            {
                // Esc closes the topmost lab-owned overlay — the launcher
                // menu first (it draws on top of everything), then whichever
                // application modal is open, then the Object Tree's own
                // gear-triggered modal. The tree-modal branch is gated on
                // `!tree_modal_focused` on purpose: while one of its own text
                // fields/colour-picker/dropdowns has focus, the OTHER arm
                // below routes Escape into `dispatch::apply_key`, which
                // closes those one level at a time (hex input -> colour
                // picker -> template dropdown -> field blur) before a
                // second Escape reaches the modal itself — the same
                // progressive-Escape contract `dispatch::apply_key`
                // documents. Without this branch, a tree modal opened by
                // clicking its own row gear (never focusing a field) had NO
                // Escape handler at all: this arm's original guard only
                // named the launcher and app_modals, wave 3's own
                // "Esc closes any open modal" claim never covered it.
                if lab.launcher_open {
                    lab.launcher_open = false;
                } else if lab.app_modals.is_open() {
                    lab.app_modals.close();
                } else if lab.concept_modal.is_open() {
                    lab.concept_modal.close();
                } else {
                    lab.tree_modal.close();
                    lab.tree_modal_focused = false;
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
                            dispatch::apply_key(kp, &mut sectors[sidx].state, prim, coord, templates);
                        }
                    }
                } else if lab.tree_modal_focused {
                    let LabState { coordinator, tree_modal, tree_objects, templates, .. } = &mut *lab;
                    if let (Some(coord), Some(prim)) = (
                        coordinator.scoped_region_coordinator_mut("objtree_modal"),
                        lab_active_primitive(tree_modal, tree_objects),
                    ) {
                        if tree_modal.color_picker.hex_editing {
                            dispatch::apply_hex_key(key, tree_modal, prim);
                        } else if let Some(kp) = key_press_from(key) {
                            dispatch::apply_key(kp, tree_modal, prim, coord, templates);
                        }
                    }
                    // Keep this flag in sync with the field store it
                    // mirrors: `dispatch::apply_key`'s own Escape/Enter
                    // paths blur the field (`active_field_id = None`)
                    // without knowing about this crate's bookkeeping —
                    // without this, a stale `true` here would swallow the
                    // Escape meant to close the modal itself once the
                    // field it was covering is already gone (see the
                    // Escape arm's own doc above).
                    if tree_modal.active_field_id.is_none() {
                        lab.tree_modal_focused = false;
                    }
                } else if lab.app_modals.is_open() {
                    if let Some(coord) = lab.coordinator.scoped_region_coordinator_mut(app_modals::REGION_ID) {
                        if let Some(kp) = key_press_from(key) {
                            let _ = coord.on_key(kp);
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
                } else {
                    // Object Tree's own settings modal — region sized to
                    // `modal_pane_rect` (wave 4), so local coords are the
                    // pointer's position relative to the chart pane's own
                    // top-left, matching the LOCAL coordinate space the
                    // renderer registers its widgets in under a matching
                    // `ctx.translate` (see the render call's own doc).
                    // Harmless while closed: nothing is registered in this
                    // region that frame, so `process_drag_start` finds
                    // nothing.
                    let (lx, ly) = (*x - modal_pane_rect.x, *y - modal_pane_rect.y);
                    let LabState { coordinator, tree_modal, tree_modal_drag, .. } = &mut *lab;
                    if let Some(child) = coordinator.scoped_region_coordinator_mut("objtree_modal") {
                        if let Some(wid) = child.process_drag_start(lx, ly) {
                            dispatch::on_drag_start(wid.as_str(), lx, ly, tree_modal, tree_modal_drag);
                            lab.tree_active_drag = true;
                        }
                    }
                }
            }
            // The wheel over the Object Tree list. Routed here, off the live
            // pointer position and the sidebar's own rect, rather than through
            // a hit-tested SCROLL zone: `InputState::scroll_delta` is set by
            // the event processor and never cleared, so a zone-driven read
            // would keep scrolling on frames with no wheel at all.
            PlatformEvent::Scroll { dy, .. } => {
                let sidebar_open = lab.shell_state.active_right_panel == Some("object_tree");
                let inside = sidebar_open
                    && lab.input_state.pointer.pos.is_some_and(|(px, py)| {
                        let x0 = modal_pane_rect.x + modal_pane_rect.width;
                        px >= x0
                            && px < x0 + mlc_core::types::RIGHT_SIDEBAR_WIDTH
                            && py >= modal_pane_rect.y
                            && py < modal_pane_rect.y + modal_pane_rect.height
                    });
                if inside {
                    // uzor-window-web already flips the browser delta
                    // (`dy: -wheel.delta_y()`), and `handle_wheel` ADDS its
                    // argument to the offset — so passing `dy` straight through
                    // scrolls the list the wrong way. Flip it back here.
                    let geo = lab.shell_state.tree_list;
                    lab.shell_state.tree_scroll.handle_wheel(-*dy, geo.content_height, geo.viewport_height);
                } else if lab.concept_modal.is_open() {
                    // Same routing as the Object Tree above (pointer
                    // position, not a `Sense::SCROLL` response), narrowed
                    // to the concept modal's own pane-local viewport rect
                    // — see `ConceptModalGeo`'s own doc for why this reads
                    // pane-local coordinates. Same sign flip: the delta
                    // arrives already negated by `uzor-window-web`.
                    if let Some((px, py)) = lab.input_state.pointer.pos {
                        let geo = lab.concept_modal_geo;
                        let (lx, ly) = (px - modal_pane_rect.x, py - modal_pane_rect.y);
                        let r = geo.content_rect;
                        let inside_modal = lx >= r.x && lx < r.x + r.width && ly >= r.y && ly < r.y + r.height;
                        if inside_modal {
                            lab.concept_modal.scroll.handle_wheel(-*dy, geo.total_content_height, geo.viewport_height);
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
                } else if lab.tree_active_drag {
                    let (lx, ly) = (*x - modal_pane_rect.x, *y - modal_pane_rect.y);
                    let LabState { coordinator, tree_modal, tree_objects, tree_modal_drag, .. } = &mut *lab;
                    if let Some(child) = coordinator.scoped_region_coordinator_mut("objtree_modal") {
                        if let Some((wid, _, _)) = child.process_drag_move(lx, ly) {
                            if let Some(prim) = lab_active_primitive(tree_modal, tree_objects) {
                                dispatch::on_drag_move(wid.as_str(), lx, ly, tree_modal, prim, tree_modal_drag);
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
                                dispatch::on_drag_end(wid.as_str(), &mut sector.state, prim);
                            }
                        }
                    }
                } else if lab.tree_active_drag {
                    lab.tree_active_drag = false;
                    let LabState { coordinator, tree_modal, tree_objects, .. } = &mut *lab;
                    if let Some(child) = coordinator.scoped_region_coordinator_mut("objtree_modal") {
                        if let Some(wid) = child.process_drag_end() {
                            if let Some(prim) = lab_active_primitive(tree_modal, tree_objects) {
                                dispatch::on_drag_end(wid.as_str(), tree_modal, prim);
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
    theme::draw_lab_bar(&mut ctx, &mut lab.coordinator, screen_w, &lab.theme_manager, any_sector_closed, lab.show_variants);

    // Stub app shell — drawn every frame, below whichever surface the lab
    // currently overlays on top of it (tree modal / app modals / the
    // launcher menu / the design-comparison grid). See `shell::draw_shell`'s
    // own doc. `modals_active` now highlights the launcher-menu toggle
    // (wave 3) — see `shell::MODALS_TOGGLE_ID`'s own doc for why that is a
    // different flag than the wave-1 `show_variants` this button used to
    // drive.
    let pane_rect = {
        let launcher_open = lab.launcher_open;
        let LabState { coordinator, theme_manager, shell_state, tree_objects, tree_selected, .. } = &mut *lab;
        let rt = theme_manager.current();
        shell::draw_shell(
            &mut ctx,
            coordinator,
            shell_state,
            tree_objects,
            *tree_selected,
            rt,
            theme::BAR_HEIGHT,
            screen_w,
            (screen_h - theme::BAR_HEIGHT).max(0.0),
            launcher_open,
        )
    };
    // Mirrored onto `LabState` for `lab_metrics` — see that field's own
    // doc for why (called from JS, between frames, with no `screen_w`/
    // `screen_h` of its own).
    lab.last_pane_rect = pane_rect;

    // Owner report, 2026-08-10, first pass: clipping the tree-gear/app
    // modal draw calls to this rect stopped them cutting into the Object
    // Tree sidebar, but it cut the MODALS instead — their own centring
    // math still assumed the FULL screen width, so a dialog wide enough to
    // reach the old clip edge lost its own right side (body + OK button)
    // instead of never reaching there. Fixed at the source below: both
    // modal families are drawn under `ctx.translate(pane_rect.x,
    // pane_rect.y)` and handed `pane_rect.width`/`pane_rect.height` in
    // place of the full screen — the SAME shape `chart_settings.rs`'s own
    // `chart_area_x`/`screen_w` split already uses (mirrors the real app's
    // own `ChartModalLayout::chart_x`/`ind_screen_w`, narrowed here to the
    // pane instead of the window), so a modal sizes and centres itself
    // entirely WITHIN the pane and never has a reason to reach the sidebar
    // in the first place — nothing left to clip.

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

    // Object Tree's own gear-triggered settings modal — the SAME real
    // `render_primitive_settings_modal` the four-sector grid uses, drawn
    // full-screen (not sectored) for whichever tree row's gear was
    // clicked. Independent of `show_variants` below — this is the shell's
    // own modal system, not the design-comparison grid.
    let tree_color_picker_was_open = lab.tree_modal.is_color_picker_open();
    if lab.tree_modal.is_open() {
        // Render-time text proxy from TextFieldStore — same shape as the
        // sector loop below, against this modal's own child coordinator.
        {
            let LabState { coordinator, tree_modal, .. } = &mut *lab;
            if let Some(coord) = coordinator.scoped_region_coordinator_mut("objtree_modal") {
                let wid = uzor::WidgetId::from(PRIMITIVE_SETTINGS_FIELD);
                let tf = coord.text_fields();
                let cursor_blink = tf.cursor_visible(now_ms as u64);
                if tree_modal.active_field_id.is_some() {
                    tree_modal.active_render = ActiveFieldRender {
                        text: tf.text(&wid).to_string(),
                        cursor: tf.cursor(&wid),
                        selection_start: tf.selection_range(&wid).map(|(s, _)| s),
                        cursor_visible: cursor_blink,
                    };
                } else {
                    tree_modal.active_render = ActiveFieldRender::default();
                }
            }
        }

        // Templates are stored per-type, same as the sector grid's own
        // `templates_for_render` above — filtered by whichever tree
        // object's gear is currently open.
        let tree_active_type = lab
            .tree_modal
            .primitive_id
            .and_then(|id| lab.tree_objects.iter().find(|p| p.data().id == id))
            .map(|p| p.type_id());
        let tree_templates_for_render: Vec<PrimitiveTemplate> = match tree_active_type {
            Some(t) => lab.templates.iter().filter(|tm| tm.type_id == t).cloned().collect(),
            None => Vec::new(),
        };

        let LabState { coordinator, tree_modal, tree_objects, tree_modal_drag, .. } = &mut *lab;
        if let Some(coord) = coordinator.scoped_region_coordinator_mut("objtree_modal") {
            // Draw in the chart pane's own LOCAL coordinate space (origin
            // at its top-left) rather than the full screen's — see this
            // block's own top-of-function doc for why. `render_primitive_
            // settings_modal` has no `chart_area_x`/`chart_area_y`
            // parameters of its own (unlike the app-modal family below) to
            // hand an offset through, so the canvas transform carries it
            // instead: everything drawn between `save`/`restore` lands at
            // `pane_rect.x/y` + its own local coordinates, and the modal's
            // internal `(screen_w - width) / 2.0` centring — fed
            // `pane_rect.width` here instead of the full screen — now
            // centres within the pane for exactly the same reason. The
            // `objtree_modal` scoped region is sized to `modal_pane_rect`
            // (== `pane_rect`, see that binding's own doc) for the same
            // frame, so hit-testing already expects LOCAL coordinates here
            // — matches the `PointerDown`/`PointerMoved` translation above.
            ctx.save();
            ctx.translate(pane_rect.x, pane_rect.y);

            let result = render_primitive_settings_modal(
                &mut ctx,
                pane_rect.width,
                pane_rect.height,
                16.0,
                tree_modal,
                &tree_objects[..],
                &frame_theme,
                &toolbar_theme,
                &toolbar_state,
                now_ms as u64,
                coord,
                &tree_templates_for_render,
                &[],
            );
            dispatch::register_dropdown_option_patch(coord, &result);

            let mut color_opacity_slider_rect = None;
            if tree_modal.is_color_picker_open() {
                let cp_result = render_primitive_color_picker_popup(&mut ctx, tree_modal, &toolbar_theme, coord);
                color_opacity_slider_rect = cp_result
                    .l1_result
                    .as_ref()
                    .and_then(|l1| l1.opacity_slider_rect)
                    .or_else(|| cp_result.l2_result.as_ref().map(|l2| l2.opacity_slider_rect));
            }

            ctx.restore();

            *tree_modal_drag = dispatch::DragGeometry {
                modal_rect: Some(result.modal_rect),
                slider_tracks: result.slider_tracks.clone(),
                scrollbar_track: result.scrollbar_track_rect,
                total_content_height: result.total_content_height,
                viewport_height: result.viewport_height,
                color_opacity_slider_rect,
            };
        }
    }

    // The approved-design concept modal (2026-08-10) — the SAME pane-local
    // translate treatment as the tree modal above (see that block's own
    // top-of-function doc), against its own `concept::REGION_ID` region.
    // Mutually exclusive with the tree modal and every application modal
    // (`open_concept_type_modal`/`open_tree_modal`/`open_app_modal` all
    // close the other two families on open), so this never draws at the
    // same time as either.
    if lab.concept_modal.is_open() {
        let LabState { coordinator, concept_modal, concept_modal_geo, .. } = &mut *lab;
        if let Some(coord) = coordinator.scoped_region_coordinator_mut(concept::REGION_ID) {
            ctx.save();
            ctx.translate(pane_rect.x, pane_rect.y);
            let result = concept::render::render_concept_modal(
                &mut ctx,
                pane_rect.width,
                pane_rect.height,
                16.0,
                concept_modal,
                &frame_theme,
                &toolbar_theme,
                coord,
            );
            ctx.restore();
            *concept_modal_geo = ConceptModalGeo {
                modal_rect: result.modal_rect,
                content_rect: result.content_rect,
                total_content_height: result.total_content_height,
                viewport_height: result.viewport_height,
            };
        }
    }

    // Indicator-overlay chart-corner toggle (wave 3) — always drawn,
    // independent of any modal being open, straight on the MAIN
    // coordinator: see `AppModalsState::draw_indicator_overlay`'s own doc
    // for why it is not routed through the `app_modal` scoped region.
    lab.app_modals.draw_indicator_overlay(&mut ctx, &mut lab.coordinator, pane_rect, &frame_theme, &toolbar_theme);

    // Whichever application modal the launcher opened (wave 3) — same
    // shape as the tree-modal block above, against its own
    // `app_modals::REGION_ID` region.
    if lab.app_modals.is_open() {
        let LabState { coordinator, app_modals, theme_manager, .. } = &mut *lab;
        if let Some(coord) = coordinator.scoped_region_coordinator_mut(crate::app_modals::REGION_ID) {
            // Same translate-into-the-pane treatment as the tree modal
            // above: `pane_rect.width`/`pane_rect.height` take the place
            // of the full screen for every one of `app_modals::draw`'s own
            // sixteen match arms. Chart Settings / Indicator Settings
            // already had a `chart_area_x`/`chart_area_y` parameter pair
            // for exactly this (mirrors the real app's own
            // `ChartModalLayout::chart_x`/`chart_y`) — `0.0, 0.0` is now
            // correct for them too, since the translate already supplies
            // the pane's own offset. The `app_modal` scoped region is
            // sized to `modal_pane_rect` (== `pane_rect`) for this same
            // frame, so hit-testing already expects LOCAL coordinates.
            ctx.save();
            ctx.translate(pane_rect.x, pane_rect.y);
            app_modals.draw(&mut ctx, coord, pane_rect.width, pane_rect.height, &frame_theme, &toolbar_theme, theme_manager, now_ms as u64);
            ctx.restore();
        }
    }

    // The modal comparison grid (`variants::VARIANTS`) draws ON TOP of the
    // shell only while the lab dev bar's own "Design compare" button has it
    // toggled on — see `LabState::show_variants`'s own doc.
    if lab.show_variants {
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
                    &[],
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
    }

    // The launcher menu (wave 3) draws LAST — on top of everything else
    // this frame paints, including an open application/tree modal or the
    // design-comparison grid — see `shell::launcher_menu`'s own module doc.
    if lab.launcher_open {
        let anchor = (screen_w - 8.0, theme::BAR_HEIGHT + mlc_core::types::TOP_TOOLBAR_HEIGHT + 4.0);
        let LabState { coordinator, theme_manager, tree_objects, .. } = &mut *lab;
        let rt = theme_manager.current();
        if let Some(coord) = coordinator.scoped_region_coordinator_mut(shell::launcher_menu::REGION_ID) {
            shell::launcher_menu::draw(&mut ctx, coord, rt, anchor, screen_w, screen_h, tree_objects);
        }
    }

    let raw_click = lab.input_state.pointer.clicked.is_some();
    let responses = lab.coordinator.end_frame();
    let mut color_picker_interacted = [false; 4];
    let mut tree_color_picker_interacted = false;

    for (wid, resp) in &responses {
        if !resp.clicked && !resp.scrolled {
            continue;
        }
        let id = wid.as_str();

        if id == shell::MODALS_TOGGLE_ID {
            if resp.clicked {
                lab.launcher_open = !lab.launcher_open;
            }
            continue;
        }
        if id == "ind_overlay:open" || id == "ind_overlay:close" {
            if resp.clicked {
                lab.app_modals.apply_indicator_overlay_response(id);
            }
            continue;
        }
        // Shell buttons that own an application modal (wave 3) — checked
        // before the generic `"shell:"` prefix catch-all below so these
        // ids get BOTH behaviours where that makes sense (the three rail
        // buttons still flip their own selected-highlight via
        // `apply_shell_response`, in addition to opening their modal).
        if id == "shell:top:chart_settings" {
            if resp.clicked {
                open_app_modal(lab, AppModalKind::ChartSettings);
            }
            continue;
        }
        if id == "shell:top:indicators" {
            if resp.clicked {
                open_app_modal(lab, AppModalKind::IndicatorSettings);
            }
            continue;
        }
        if id == "shell:top:symbol" {
            if resp.clicked {
                open_app_modal(lab, AppModalKind::Search);
            }
            continue;
        }
        if id == "shell:top:template" {
            if resp.clicked {
                open_app_modal(lab, AppModalKind::TemplateName);
            }
            continue;
        }
        if id == shell::bottom_bar::CLOCK_ID {
            if resp.clicked {
                open_app_modal(lab, AppModalKind::Clock);
            }
            continue;
        }
        if id == "shell:right:panel:watchlist" || id == "shell:right:panel:alert" || id == "shell:right:panel:trading" {
            if resp.clicked {
                shell::apply_shell_response(id, &mut lab.shell_state);
                let kind = match id {
                    "shell:right:panel:watchlist" => AppModalKind::Watchlist,
                    "shell:right:panel:alert" => AppModalKind::Alerts,
                    _ => AppModalKind::TradingContainer,
                };
                open_app_modal(lab, kind);
            }
            continue;
        }
        if let Some(pid_str) = id.strip_prefix("shell:objtree:row:") {
            if resp.clicked {
                if let Ok(pid) = pid_str.parse::<u64>() {
                    lab.tree_selected = Some(pid);
                }
            }
            continue;
        }
        if let Some(rest) = id.strip_prefix("shell:objtree:act:") {
            if resp.clicked {
                if let Some((action, pid_str)) = rest.split_once(':') {
                    if let Ok(pid) = pid_str.parse::<u64>() {
                        match action {
                            // The gear opens the CONCEPT modal (2026-08-10)
                            // — the production modal moved to the
                            // launcher's own "Production modals" section
                            // (`shell::launcher_menu`).
                            "gear" => {
                                if let Some(type_id) = lab.tree_objects.iter().find(|p| p.data().id == pid).map(|p| p.type_id()) {
                                    open_concept_type_modal(lab, type_id);
                                }
                            }
                            "del" => delete_tree_object(lab, pid),
                            // unlock/eye/alert: registered for parity with
                            // `sim.js`'s own row actions, which likewise
                            // leave them unwired — no behaviour to mirror.
                            _ => {}
                        }
                    }
                }
            }
            continue;
        }
        if id == "shell:bottom:settings" {
            if resp.clicked {
                // Same redirect as the row gear above: the bottom
                // toolbar's own settings icon is "the gear" too.
                let type_id = lab
                    .tree_selected
                    .and_then(|pid| lab.tree_objects.iter().find(|p| p.data().id == pid))
                    .map(|p| p.type_id());
                if let Some(type_id) = type_id {
                    open_concept_type_modal(lab, type_id);
                }
            }
            continue;
        }
        if id == "shell:bottom:delete" {
            if resp.clicked {
                if let Some(pid) = lab.tree_selected {
                    delete_tree_object(lab, pid);
                }
            }
            continue;
        }
        if let Some(tool) = id.strip_prefix("shell:left:tool:") {
            if resp.clicked {
                shell::apply_shell_response(id, &mut lab.shell_state);
                if shell::object_tree::spawnable_types().contains(&tool) {
                    spawn_tree_object(lab, tool);
                }
            }
            continue;
        }
        if id.starts_with("shell:") {
            if resp.clicked {
                shell::apply_shell_response(id, &mut lab.shell_state);
            }
            continue;
        }
        if let Some(rest_id) = id.strip_prefix("objtree_modal:") {
            // LOCAL to the chart pane (wave 4) — the modal itself is drawn
            // under `ctx.translate(pane_rect.x, pane_rect.y)` (see that
            // render call's own doc), so every rect it registered/returned
            // (`tree_modal_drag`'s own fields, slider/scrollbar rects) is
            // pane-local too; the raw pointer position must match.
            let pointer = lab.input_state.pointer.pos.map(|(x, y)| (x - pane_rect.x, y - pane_rect.y));

            if let Some(cp_id) = rest_id.strip_prefix("color_picker_primitive:") {
                tree_color_picker_interacted = true;
                let LabState { tree_modal, tree_objects, .. } = &mut *lab;
                if let Some(prim) = lab_active_primitive(tree_modal, tree_objects) {
                    dispatch::apply_color_picker_response(cp_id, resp, tree_modal, prim, pointer, pane_rect.width, pane_rect.height);
                }
                continue;
            }

            if rest_id == "prim_settings:scroll_viewport" || rest_id == "prim_settings:scrollbar_track" {
                let geo = lab.tree_modal_drag.clone();
                dispatch::apply_scroll_response(
                    rest_id, resp, &mut lab.tree_modal, pointer, geo.total_content_height, geo.viewport_height, geo.scrollbar_track,
                );
                continue;
            }

            let routed_id = rest_id.strip_prefix("prim_settings:item:").unwrap_or(rest_id);

            if resp.clicked {
                if !lab.tree_modal_focused {
                    // A sector held focus — blur/commit its field before
                    // this click steals it (mirrors the sector-to-sector
                    // switch below; only one surface holds focus at once).
                    if let Some(prev_idx) = lab.focused_sector.take() {
                        let LabState { coordinator, sectors, primitives, templates, .. } = &mut *lab;
                        if let Some(prev_coord) = coordinator.scoped_region_coordinator_mut(SECTOR_IDS[prev_idx]) {
                            if let Some(prim) = lab_active_primitive(&sectors[prev_idx].state, primitives) {
                                dispatch::commit_active_field(&mut sectors[prev_idx].state, prim, prev_coord, templates);
                            }
                        }
                    }
                    lab.tree_modal_focused = true;
                }

                let LabState { coordinator, tree_modal, tree_objects, templates, .. } = &mut *lab;
                let Some(coord) = coordinator.scoped_region_coordinator_mut("objtree_modal") else { continue };
                if let Some(prim) = lab_active_primitive(tree_modal, tree_objects) {
                    let mut click_ctx =
                        dispatch::ClickCtx { anchor: resp.rect, screen_w: pane_rect.width, screen_h: pane_rect.height, templates };
                    let handled = dispatch::apply_approved_design_click(routed_id, tree_modal, prim, coord, &mut click_ctx);
                    if !handled {
                        dispatch::apply_primitive_settings_click(routed_id, tree_modal, prim, coord, &mut click_ctx);
                    }
                }
            }
            continue;
        }

        if let Some(rest_id) = id.strip_prefix("concept_modal_region:") {
            if resp.clicked {
                if rest_id == "concept_modal:close" || rest_id == "concept_modal:ok" {
                    lab.concept_modal.close();
                } else if let Some(tab_id) = rest_id.strip_prefix("concept_modal:tab:") {
                    if let Some(target) = lab.concept_modal.open {
                        if let Some(tabs) = concept::render::tabs_for(target) {
                            if let Some(tab) = tabs.iter().find(|t| t.id == tab_id) {
                                lab.concept_modal.set_active_tab(target.id(), tab.id);
                            }
                        }
                    }
                } else if let Some(item_id) = rest_id.strip_prefix("concept_modal:item:") {
                    if let Some(target) = lab.concept_modal.open {
                        if let Some(tabs) = concept::render::tabs_for(target) {
                            concept::click::apply_click(tabs, item_id, &mut lab.concept_modal);
                        }
                    }
                }
            }
            continue;
        }

        if let Some(preset) = id.strip_prefix("lab:theme:") {
            if resp.clicked {
                lab.theme_manager.set_preset(preset);
            }
            continue;
        }
        if id == theme::DESIGN_COMPARE_WIDGET_ID {
            if resp.clicked {
                lab.show_variants = !lab.show_variants;
            }
            continue;
        }
        if let Some(rest_id) = id.strip_prefix("launcher_menu:") {
            if resp.clicked {
                if rest_id == "bg" {
                    lab.launcher_open = false;
                } else if let Some(type_id) = rest_id.strip_prefix("object:") {
                    // Concept section (2026-08-10) — see
                    // `shell::launcher_menu`'s own doc for why this now
                    // opens the DESIGN modal, and where the production one
                    // moved to.
                    lab.launcher_open = false;
                    open_concept_type_modal(lab, type_id);
                } else if let Some(type_id) = rest_id.strip_prefix("object_prod:") {
                    lab.launcher_open = false;
                    open_object_modal_for_type(lab, type_id);
                } else if let Some(kind_id) = rest_id.strip_prefix("app:") {
                    lab.launcher_open = false;
                    if let Some(kind) = AppModalKind::from_id(kind_id) {
                        if kind == AppModalKind::IndicatorOverlay {
                            lab.app_modals.indicator_overlay.is_open = true;
                        } else {
                            open_app_modal(lab, kind);
                        }
                    }
                }
            }
            continue;
        }
        if let Some(rest_id) = id.strip_prefix("app_modal:") {
            lab.app_modals.apply_response(rest_id, resp.clicked);
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
                            dispatch::commit_active_field(&mut sectors[prev_idx].state, prim, prev_coord, templates);
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
                    routed_id, &mut sector.state, prim, coord, &mut click_ctx,
                );
                if !handled {
                    dispatch::apply_primitive_settings_click(routed_id, &mut sector.state, prim, coord, &mut click_ctx);
                }
            }
        }
    }

    for i in 0..4 {
        if color_picker_was_open[i] && raw_click && !color_picker_interacted[i] {
            lab.sectors[i].state.close_color_picker();
        }
    }
    if tree_color_picker_was_open && raw_click && !tree_color_picker_interacted {
        lab.tree_modal.close_color_picker();
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

// ─── Agent control plane (`window.__LAB`, wave 4) ──────────────────────────
//
// CDP-eval surface, mirroring `sim/sim.js`'s own `window.__SIM` (see that
// file's own doc: "the page never opens a socket and the dev server stays
// a dumb static one" — same rule here). Six `#[wasm_bindgen]` free
// functions, each returning a hand-built JSON `String` (no serde in this
// crate, and the field set below is small and fixed enough that a
// dependency would buy nothing). `index.html`'s own bootstrap script
// imports these by name and assembles `window.__LAB` from them — see its
// own comment for why that step lives in JS rather than here: this crate
// cannot poke a JS global itself without `js_sys::Reflect` ceremony, and a
// free `#[wasm_bindgen]` fn already gets wasm-bindgen's own ES-module
// export machinery for free.

#[cfg(target_arch = "wasm32")]
thread_local! {
    /// The one [`LabState`] the render loop drives, stashed here so these
    /// exported fns (called from JS, never from the render loop itself)
    /// can reach it. wasm is single-threaded — `thread_local!` is exactly
    /// the "one instance, JS-callable" idiom it exists for.
    static LAB_HANDLE: RefCell<Option<Rc<RefCell<LabState>>>> = RefCell::new(None);
}

/// Run `f` against the live [`LabState`], if the render loop has booted.
/// `None` only in the narrow window before [`start`]'s first frame — every
/// exported fn below turns that into an honest `{"ok":false,...}" rather
/// than panicking.
#[cfg(target_arch = "wasm32")]
fn with_lab<R>(f: impl FnOnce(&mut LabState) -> R) -> Option<R> {
    LAB_HANDLE.with(|handle| {
        let borrowed = handle.borrow();
        let rc = borrowed.as_ref()?;
        let mut guard = rc.borrow_mut();
        Some(f(&mut guard))
    })
}

/// Escape `s` for embedding inside a JSON string literal.
#[cfg(target_arch = "wasm32")]
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(target_arch = "wasm32")]
fn json_str(s: &str) -> String {
    format!("\"{}\"", json_escape(s))
}

#[cfg(target_arch = "wasm32")]
fn json_opt_u64(v: Option<u64>) -> String {
    v.map(|n| n.to_string()).unwrap_or_else(|| "null".to_string())
}

#[cfg(target_arch = "wasm32")]
const LAB_NOT_READY: &str = "{\"ok\":false,\"error\":\"lab not ready\"}";

/// `{ objects: [type_id, ...], application: [modal_id, ...],
/// concept_application: [modal_id, ...] }`.
///
/// `objects` is the 14 approved-design archetypes
/// (`shell::object_tree::spawnable_types()`, itself sourced from
/// `concept::spec::types()` since 2026-08-10) — every id [`lab_open_type`]
/// opens as the CONCEPT modal (`concept::render::render_concept_modal`).
/// `application` is unchanged — the 16 legacy `AppModalKind` ids
/// [`lab_open_app`] opens. `concept_application` is the 9 approved-design
/// APPLICATION archetypes (`concept::spec::app_modals()`) — also openable
/// through [`lab_open_type`] (it tries the 14 object types first, then
/// falls back to these 9), kept as a separate field rather than folded
/// into `application` because the two id spaces partly collide (e.g.
/// `"trading"` here vs `"trading_container"` there) and `application`'s
/// own ids must keep working with [`lab_open_app`] unchanged. Equivalent
/// to `__SIM.modals()`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn lab_modals() -> String {
    let objects: Vec<String> = shell::object_tree::spawnable_types().iter().map(|t| json_str(t)).collect();
    let application: Vec<String> = AppModalKind::ALL.iter().map(|k| json_str(k.id())).collect();
    let concept_application: Vec<String> = concept::spec::app_modals().iter().map(|m| json_str(m.id)).collect();
    format!(
        "{{\"objects\":[{}],\"application\":[{}],\"concept_application\":[{}]}}",
        objects.join(","),
        application.join(","),
        concept_application.join(","),
    )
}

/// Open the CONCEPT modal (`concept::render::render_concept_modal`) for
/// `id` — tried first against the 14 object archetypes
/// (`concept::spec::types()`), then against the 9 application archetypes
/// (`concept::spec::app_modals()`), optionally switching straight to `tab`
/// (one of that target's own tab ids — `concept::spec::ConceptTab::id`,
/// e.g. `"levels"`/`"labels"`/`"placement"`; the exact set varies per
/// target, see [`lab_modals`]'s own doc for where to read a target's
/// declared tabs — [`lab_snapshot`] reports them once open).
///
/// **2026-08-10, concept wiring**: this used to open the PRODUCTION
/// primitive-settings modal (`open_object_modal_for_type`) — that path is
/// still reachable through the shell's own launcher "Production modals"
/// section, just not through this control-plane entry point any more (the
/// function NAME is unchanged, per the launching brief: "extend, do not
/// rename"). `"id"` in the response is now the STRING target id
/// (`type_id`/`modal_id`), not a numeric primitive id — the concept
/// surface has no live `Primitive` behind it to number. Equivalent to
/// `__LAB.open_type(id, tab)`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn lab_open_type(type_id: &str, tab: Option<String>) -> String {
    with_lab(|lab| {
        let opened = open_concept_type_modal(lab, type_id) || open_concept_app_modal(lab, type_id);
        if !opened {
            return format!("{{\"ok\":false,\"error\":\"unknown type_id\",\"type_id\":{}}}", json_str(type_id));
        }
        let Some(target) = lab.concept_modal.open else {
            return format!("{{\"ok\":false,\"error\":\"failed to open\",\"type_id\":{}}}", json_str(type_id));
        };
        let tabs = concept::render::tabs_for(target).unwrap_or(&[]);
        if let Some(tab_id) = tab.as_deref() {
            if let Some(t) = tabs.iter().find(|t| t.id == tab_id) {
                lab.concept_modal.set_active_tab(target.id(), t.id);
            }
        }
        format!(
            "{{\"ok\":true,\"id\":{},\"type_id\":{},\"tab\":{}}}",
            json_str(target.id()),
            json_str(type_id),
            json_str(lab.concept_modal.active_tab_id(target.id(), tabs)),
        )
    })
    .unwrap_or_else(|| LAB_NOT_READY.to_string())
}

/// Switch the currently-open CONCEPT modal's active tab to `tab_id` — the
/// LIVE counterpart to [`lab_open_type`]'s own `tab` parameter (which only
/// applies at open time). Reports `ok:false` if no concept modal is open
/// or `tab_id` does not name one of its own declared tabs. Equivalent to
/// `__LAB.tab(id)`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn lab_tab(tab_id: &str) -> String {
    with_lab(|lab| {
        let Some(target) = lab.concept_modal.open else {
            return "{\"ok\":false,\"error\":\"no concept modal open\"}".to_string();
        };
        let Some(tabs) = concept::render::tabs_for(target) else {
            return format!("{{\"ok\":false,\"error\":\"unknown target\",\"id\":{}}}", json_str(target.id()));
        };
        let Some(tab) = tabs.iter().find(|t| t.id == tab_id) else {
            return format!("{{\"ok\":false,\"error\":\"unknown tab_id\",\"tab_id\":{}}}", json_str(tab_id));
        };
        lab.concept_modal.set_active_tab(target.id(), tab.id);
        format!("{{\"ok\":true,\"id\":{},\"tab\":{}}}", json_str(target.id()), json_str(tab.id))
    })
    .unwrap_or_else(|| LAB_NOT_READY.to_string())
}

/// Open the application-level modal named by `modal_id` (one of [`lab_modals`]'s
/// own `"application"` list). Equivalent to `__SIM.app`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn lab_open_app(modal_id: &str) -> String {
    with_lab(|lab| {
        let Some(kind) = AppModalKind::from_id(modal_id) else {
            return format!("{{\"ok\":false,\"error\":\"unknown modal_id\",\"modal_id\":{}}}", json_str(modal_id));
        };
        // Mirrors the launcher menu's own click handler exactly (`shell::
        // launcher_menu` app-row branch in `render_one_frame`): indicator
        // overlay is not a modal (no backdrop, no OK/close footer — see
        // `app_modals`'s own module doc), so it is not routed through
        // `open_app_modal`'s tree-modal-closing, single-slot path.
        if kind == AppModalKind::IndicatorOverlay {
            lab.app_modals.indicator_overlay.is_open = true;
        } else {
            open_app_modal(lab, kind);
        }
        format!("{{\"ok\":true,\"id\":{}}}", json_str(modal_id))
    })
    .unwrap_or_else(|| LAB_NOT_READY.to_string())
}

/// Close whichever lab-owned overlay is topmost — same priority order as
/// the Esc key handler in `render_one_frame` (launcher, then an
/// application modal, then the concept modal, then the Object Tree's own
/// production gear modal). Equivalent to `__SIM.close`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn lab_close() -> String {
    with_lab(|lab| {
        let closed = if lab.launcher_open {
            lab.launcher_open = false;
            "launcher"
        } else if lab.app_modals.is_open() {
            lab.app_modals.close();
            "app"
        } else if lab.concept_modal.is_open() {
            lab.concept_modal.close();
            "concept"
        } else if lab.tree_modal.is_open() {
            lab.tree_modal.close();
            lab.tree_modal_focused = false;
            "tree"
        } else {
            "none"
        };
        format!("{{\"ok\":true,\"closed\":{}}}", json_str(closed))
    })
    .unwrap_or_else(|| LAB_NOT_READY.to_string())
}

/// What the Object Tree holds, what is selected, and which modal (if any)
/// is open — its family, id, active tab, and declared tab list. Equivalent
/// to `__SIM.snapshot()`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn lab_snapshot() -> String {
    with_lab(|lab| {
        let objects: Vec<String> = lab
            .tree_objects
            .iter()
            .map(|p| {
                let data = p.data();
                format!(
                    "{{\"id\":{},\"type_id\":{},\"display_name\":{},\"selected\":{}}}",
                    data.id,
                    json_str(p.type_id()),
                    json_str(p.display_name()),
                    lab.tree_selected == Some(data.id),
                )
            })
            .collect();

        let modal = if let Some(target) = lab.concept_modal.open {
            let tabs = concept::render::tabs_for(target).unwrap_or(&[]);
            let tab_ids: Vec<String> = tabs.iter().map(|t| json_str(t.id)).collect();
            format!(
                "{{\"family\":\"concept\",\"id\":{},\"tab\":{},\"tabs\":[{}]}}",
                json_str(target.id()),
                json_str(lab.concept_modal.active_tab_id(target.id(), tabs)),
                tab_ids.join(","),
            )
        } else if lab.tree_modal.is_open() {
            let type_id = lab
                .tree_modal
                .primitive_id
                .and_then(|id| lab.tree_objects.iter().find(|p| p.data().id == id))
                .map(|p| p.type_id())
                .unwrap_or("");
            let tabs: Vec<String> = mlc_app_layout::ui::modal_settings::PrimitiveSettingsTab::all()
                .iter()
                .map(|t| json_str(t.id()))
                .collect();
            format!(
                "{{\"family\":\"tree\",\"id\":{},\"tab\":{},\"tabs\":[{}]}}",
                json_str(type_id),
                json_str(lab.tree_modal.active_tab.id()),
                tabs.join(","),
            )
        } else if let Some(kind) = lab.app_modals.open {
            let tabs: Vec<String> = AppModalsState::declared_tabs(kind).iter().map(|t| json_str(t)).collect();
            let tab = lab.app_modals.active_tab_id().map(json_str).unwrap_or_else(|| "null".to_string());
            format!(
                "{{\"family\":\"app\",\"id\":{},\"tab\":{},\"tabs\":[{}]}}",
                json_str(kind.id()),
                tab,
                tabs.join(","),
            )
        } else {
            "null".to_string()
        };

        format!(
            "{{\"objects\":[{}],\"selected\":{},\"sidebar_open\":{},\"launcher_open\":{},\"modal\":{}}}",
            objects.join(","),
            json_opt_u64(lab.tree_selected),
            lab.shell_state.active_right_panel == Some("object_tree"),
            lab.launcher_open,
            modal,
        )
    })
    .unwrap_or_else(|| LAB_NOT_READY.to_string())
}

/// Real measured shell + open-modal geometry, for a numeric cross-check
/// against `__SIM.metrics()`'s own CSS-computed values.
///
/// `rails` mirrors `sim.js`'s `--mlc-layout-*` custom properties field for
/// field (same underlying `mlc_core::types` constants both ultimately
/// trace to). `modal` is `null` when nothing is open; otherwise its `rect`
/// is this frame's REAL measured frame rect (the tree family's own
/// `PrimitiveSettingsResult::modal_rect` / an app modal's own `*Result::
/// modal_rect`-shaped field — see `app_modals::AppModalsState::last_rect`'s
/// own doc), and `header_h`/`tab_bar_h`/`tab_rail_w`/`footer_h` are the
/// shared `mlc_app_layout::ui::widgets` shell constants for the "app"
/// family (every one of the sixteen modals that uses `ModalShellSpec`
/// inherits them) — `null` for the "tree" family, whose modal is a bespoke
/// shell (`layout::modals::primitive_settings`) that does not use them.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn lab_metrics() -> String {
    with_lab(|lab| {
        let sidebar_open = lab.shell_state.active_right_panel == Some("object_tree");
        let rails = format!(
            concat!(
                "{{\"top_toolbar_height\":{},\"left_toolbar_width\":{},",
                "\"right_toolbar_width\":{},\"bottom_toolbar_height\":{},",
                "\"right_sidebar_width\":{},\"lab_bar_height\":{}}}",
            ),
            mlc_core::types::TOP_TOOLBAR_HEIGHT,
            mlc_core::types::LEFT_TOOLBAR_WIDTH,
            mlc_core::types::RIGHT_TOOLBAR_WIDTH,
            mlc_core::types::BOTTOM_TOOLBAR_HEIGHT,
            if sidebar_open { mlc_core::types::RIGHT_SIDEBAR_WIDTH } else { 0.0 },
            theme::BAR_HEIGHT,
        );

        // Both modal families draw pane-LOCAL now (`ctx.translate(pane_rect.x,
        // pane_rect.y)` — see those render calls' own doc), so their own
        // captured rects are pane-local too; shift them back to
        // viewport-absolute here, matching `sim.js`'s own
        // `getBoundingClientRect()`-sourced numbers.
        let pane = lab.last_pane_rect;
        let rect_json = |r: uzor::types::Rect| {
            format!(
                "{{\"x\":{},\"y\":{},\"width\":{},\"height\":{}}}",
                r.x + pane.x,
                r.y + pane.y,
                r.width,
                r.height,
            )
        };

        let modal = if lab.concept_modal.is_open() {
            format!(
                "{{\"family\":\"concept\",\"rect\":{},\"header_h\":null,\"tab_bar_h\":null,\"tab_rail_w\":null,\"footer_h\":null}}",
                rect_json(lab.concept_modal_geo.modal_rect),
            )
        } else if lab.tree_modal.is_open() {
            match lab.tree_modal_drag.modal_rect {
                Some(r) => format!(
                    "{{\"family\":\"tree\",\"rect\":{},\"header_h\":null,\"tab_bar_h\":null,\"tab_rail_w\":null,\"footer_h\":null}}",
                    rect_json(r),
                ),
                None => "null".to_string(),
            }
        } else if lab.app_modals.is_open() {
            match lab.app_modals.last_rect() {
                Some(r) => format!(
                    "{{\"family\":\"app\",\"rect\":{},\"header_h\":{},\"tab_bar_h\":{},\"tab_rail_w\":{},\"footer_h\":{}}}",
                    rect_json(r),
                    mlc_app_layout::ui::widgets::HEADER_H,
                    mlc_app_layout::ui::widgets::TAB_BAR_H,
                    mlc_app_layout::ui::widgets::TAB_RAIL_W,
                    mlc_app_layout::ui::widgets::FOOTER_H,
                ),
                None => "null".to_string(),
            }
        } else {
            "null".to_string()
        };

        format!("{{\"rails\":{},\"modal\":{}}}", rails, modal)
    })
    .unwrap_or_else(|| LAB_NOT_READY.to_string())
}

// ─── Type declaration dumper (wave 5, `lab_dump_types`) ────────────────────
//
// One JSON row per registered primitive type (`shell::object_tree::
// spawnable_types()`'s own order — every registered type_id, alphabetically,
// see that function's own doc for why that is the one deterministic order
// available), built from a freshly-constructed instance of each
// (`registry.create(type_id, &[], None)`) reporting exactly what that
// instance declares about itself, nothing invented on top. The wasm-callable
// equivalent of `mlc-core`'s own native `registry::catalogue_dump::
// dump_catalogue` test (icon-resolution audit precedent, 2026-08-07) — see
// [`lab_dump_types`]'s own doc for the exact schema.

/// Which tabs the real primitive-settings modal
/// (`mlc-app-layout::layout::modals::primitive_settings::
/// render_primitive_settings_modal`) would show for `prim` — duplicates
/// that crate's own PRIVATE `available_tabs` filter (`layout/modals/
/// primitive_settings.rs`) arm-for-arm, built entirely from its own
/// already-`pub` building blocks (`primitive_settings_{vid,labels,cells}::
/// has_content`, `Primitive::level_configs`) rather than widening that
/// crate's own visibility for this one dump function. Chosen over exporting
/// a new `pub fn` from `mlc-app-layout` because every piece this needs was
/// ALREADY public — `available_tabs` itself is the only private link in the
/// chain, and it is a ten-line filter, not logic this crate would otherwise
/// have had to invent. Kept from drifting by construction: `PrimitiveSettingsTab::all()`
/// is the same fixed enum both sides iterate, and the three `has_content`
/// calls are the exact functions the real modal calls too.
#[cfg(target_arch = "wasm32")]
fn resolved_tabs(prim: &dyn Primitive) -> Vec<mlc_app_layout::ui::modal_settings::PrimitiveSettingsTab> {
    use mlc_app_layout::layout::modals::{primitive_settings_cells, primitive_settings_labels, primitive_settings_vid};
    use mlc_app_layout::ui::modal_settings::PrimitiveSettingsTab;
    PrimitiveSettingsTab::all()
        .iter()
        .copied()
        .filter(|tab| match tab {
            PrimitiveSettingsTab::Levels => prim.level_configs().is_some(),
            PrimitiveSettingsTab::Appearance => primitive_settings_vid::has_content(prim),
            PrimitiveSettingsTab::Labels => primitive_settings_labels::has_content(prim),
            PrimitiveSettingsTab::Cells => primitive_settings_cells::has_content(prim),
            PrimitiveSettingsTab::Placement => true,
        })
        .collect()
}

/// Textual summary of a `PropertyType` — includes the full option list for
/// `Select` and the numeric bounds for `Number`/`Integer`/`Text`, not just
/// the variant name (what the audit this feeds actually needs).
#[cfg(target_arch = "wasm32")]
fn property_type_label(pt: &mlc_core::drawing::primitives_v2::config::PropertyType) -> String {
    use mlc_core::drawing::primitives_v2::config::PropertyType;
    fn opt<T: std::fmt::Display>(v: Option<T>) -> String {
        v.map(|x| x.to_string()).unwrap_or_else(|| "none".to_string())
    }
    match pt {
        PropertyType::Color => "color".to_string(),
        PropertyType::Number { min, max, step } => {
            format!("number(min={},max={},step={})", opt(*min), opt(*max), opt(*step))
        }
        PropertyType::Integer { min, max } => format!("integer(min={},max={})", opt(*min), opt(*max)),
        PropertyType::Boolean => "boolean".to_string(),
        PropertyType::Select { options } => {
            let opts: Vec<String> = options.iter().map(|o| format!("{}:{}", o.value, o.label)).collect();
            format!("select({})", opts.join("|"))
        }
        PropertyType::LineStyle => "line_style".to_string(),
        PropertyType::Text { multiline, max_length } => {
            format!("text(multiline={},max_length={})", multiline, opt(*max_length))
        }
        PropertyType::FibLevels => "fib_levels".to_string(),
        PropertyType::Coordinate => "coordinate".to_string(),
        PropertyType::TimeframeVisibility => "timeframe_visibility".to_string(),
    }
}

/// Textual value of a `PropertyValue` — the current value read straight off
/// the fresh instance, not re-derived.
#[cfg(target_arch = "wasm32")]
fn property_value_label(v: &mlc_core::drawing::primitives_v2::config::PropertyValue) -> String {
    use mlc_core::drawing::primitives_v2::config::PropertyValue;
    match v {
        PropertyValue::Color(s) | PropertyValue::String(s) | PropertyValue::LineStyle(s) => s.clone(),
        PropertyValue::Number(n) => n.to_string(),
        PropertyValue::Integer(n) => n.to_string(),
        PropertyValue::Boolean(b) => b.to_string(),
        PropertyValue::FibLevels(levels) => format!("{} levels", levels.len()),
        PropertyValue::Coordinate { bar, price } => format!("bar={bar},price={price}"),
        PropertyValue::TimeframeVisibility(cfg) => format!("{cfg:?}"),
    }
}

/// One `ConfigProperty` as `{id, name, prop_type, value, category, order,
/// readonly}` — every field the task's own audit asked for, nothing more.
#[cfg(target_arch = "wasm32")]
fn config_property_json(p: &mlc_core::drawing::primitives_v2::config::ConfigProperty) -> String {
    format!(
        "{{\"id\":{},\"name\":{},\"prop_type\":{},\"value\":{},\"category\":{},\"order\":{},\"readonly\":{}}}",
        json_str(&p.id),
        json_str(&p.name),
        json_str(&property_type_label(&p.prop_type)),
        json_str(&property_value_label(&p.value)),
        json_str(&format!("{:?}", p.category)),
        p.order,
        p.readonly,
    )
}

/// One row of [`lab_dump_types`]'s own array — a freshly-constructed
/// `type_id` instance, reporting exactly what it declares about itself.
#[cfg(target_arch = "wasm32")]
fn dump_one_type(registry: &PrimitiveRegistry, type_id: &str) -> String {
    let Some(prim) = registry.create(type_id, &[], None) else {
        return format!("{{\"type_id\":{},\"error\":{}}}", json_str(type_id), json_str("factory returned None"));
    };
    // `supports_text` has no `Primitive` trait accessor — it lives only on
    // the registry's own `PrimitiveMetadata`, alongside `type_id`/
    // `display_name`/`kind` (which this reads off the INSTANCE instead,
    // via `prim.type_id()`/`prim.display_name()`/`prim.kind()` — the same
    // three that can legitimately differ from static metadata for a
    // primitive whose behaviour depends on its own state).
    let supports_text = registry.get(type_id).map(|m| m.supports_text).unwrap_or(false);

    let points = prim.points();
    let point_labels = prim
        .point_labels()
        .unwrap_or_else(|| mlc_core::drawing::get_point_labels(prim.type_id(), points.len()));
    let point_labels_json: Vec<String> = point_labels.iter().map(|s| json_str(s)).collect();

    let level_configs = prim.level_configs();
    let has_levels = level_configs.is_some();
    let levels_json: Vec<String> = level_configs
        .unwrap_or_default()
        .iter()
        .map(|lvl| {
            format!(
                "{{\"value\":{},\"visible\":{},\"label\":{}}}",
                lvl.level,
                lvl.visible,
                json_str(&prim.level_value_label(lvl.level)),
            )
        })
        .collect();

    let style_properties: Vec<String> = prim.style_properties().iter().map(config_property_json).collect();
    let text_properties_json = match prim.text_properties() {
        Some(list) => format!("[{}]", list.iter().map(config_property_json).collect::<Vec<_>>().join(",")),
        None => "null".to_string(),
    };
    let level_properties: Vec<String> = prim.level_properties().iter().map(config_property_json).collect();

    let tabs: Vec<String> = resolved_tabs(prim.as_ref()).iter().map(|t| json_str(t.id())).collect();

    format!(
        concat!(
            "{{\"type_id\":{},\"display_name\":{},\"kind\":{},\"supports_text\":{},",
            "\"points_count\":{},\"point_labels\":[{}],\"has_levels\":{},\"levels\":[{}],",
            "\"style_properties\":[{}],\"text_properties\":{},\"level_properties\":[{}],\"tabs\":[{}]}}",
        ),
        json_str(prim.type_id()),
        json_str(prim.display_name()),
        json_str(&format!("{:?}", prim.kind())),
        supports_text,
        points.len(),
        point_labels_json.join(","),
        has_levels,
        levels_json.join(","),
        style_properties.join(","),
        text_properties_json,
        level_properties.join(","),
        tabs.join(","),
    )
}

/// One JSON object per registered primitive type (`shell::object_tree::
/// spawnable_types()`'s own order): `type_id`, `display_name`, `kind`,
/// `supports_text`, `points_count`, `point_labels` (resolved the SAME way
/// the Placement tab does — `Primitive::point_labels()` first, falling back
/// to `mlc_core::drawing::get_point_labels`), `has_levels` +
/// `levels: [{value, visible, label}]` (via `Primitive::level_configs`/
/// `level_value_label`), `style_properties`/`text_properties`
/// (nullable)/`level_properties` (each a `ConfigProperty` array —
/// `{id, name, prop_type, value, category, order, readonly}`), and `tabs`
/// (the tab strip [`resolved_tabs`] says the real settings modal would show
/// for this type). A type whose factory returns `None` (should not happen —
/// every id here came from the registry itself) reports `{type_id, error}`
/// instead of panicking.
///
/// The machine half of a primitive audit — mirrors `mlc-core`'s own native
/// `registry::catalogue_dump::dump_catalogue` test (icon-resolution audit
/// precedent, 2026-08-07), as the wasm-callable, JSON-string equivalent for
/// this lab's own agent control plane.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn lab_dump_types() -> String {
    let Ok(registry) = PrimitiveRegistry::global().read() else {
        return "[]".to_string();
    };
    let rows: Vec<String> = shell::object_tree::spawnable_types()
        .iter()
        .map(|type_id| dump_one_type(&registry, type_id))
        .collect();
    format!("[{}]", rows.join(","))
}
