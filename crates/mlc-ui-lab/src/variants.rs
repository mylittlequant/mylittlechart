//! Registry of primitive-settings modal design variants.
//!
//! A "variant" is a single function matching [`RenderFn`] — the real
//! `render_primitive_settings_modal` signature plus one trailing parameter,
//! `&mut DesignUiState`, for the lab-only per-sector UI state the approved
//! design and the templates panel need (which of their own tabs is open,
//! набор/palette/inspector selection, ...) and that has no home on
//! `PrimitiveSettingsState` itself (out of this crate's scope). Baseline
//! never reads it.
//!
//! Two-sector layout (owner direction, 2026-08-07, after live review of the
//! four-sector Levels-tab comparison): `baseline` next to the `approved`
//! design side by side is the whole before/after; the templates panel gets
//! its own third sector rather than being crammed into either; the fourth
//! sector is empty by design (`crate::lib`'s 2×2 grid stays 2×2, drawing
//! nothing in the unused cell is simpler and safer than reshaping the
//! grid). `crate::lib`'s sector loop draws all four every frame — there is
//! no "active variant" to switch between.

use mlc_app_layout::layout::modals::primitive_settings::render_primitive_settings_modal;
use mlc_app_layout::layout::render_frame::PrimitiveSettingsResult;
use mlc_app_layout::layout::toolbar_state::ToolbarState;
use mlc_app_layout::layout::FrameTheme;
use mlc_app_layout::ui::modal_settings::PrimitiveSettingsState;
use mlc_app_layout::ui::toolbar_theme::ToolbarTheme;
use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::render::RenderContext;
use mlc_core::templates::PrimitiveTemplate;
use uzor::input::InputCoordinator;

use crate::design_state::DesignUiState;

/// The real `render_primitive_settings_modal` signature plus one trailing
/// `&mut DesignUiState` for the lab-only per-sector state the approved
/// design and the templates panel need.
pub type RenderFn = fn(
    &mut dyn RenderContext,
    f64,
    f64,
    f64,
    &PrimitiveSettingsState,
    &[Box<dyn Primitive>],
    &FrameTheme,
    &ToolbarTheme,
    &ToolbarState,
    u64,
    &mut InputCoordinator,
    &[PrimitiveTemplate],
    &mut DesignUiState,
) -> PrimitiveSettingsResult;

/// One named design.
pub struct Variant {
    /// Shown as the sector label and in the debug readout.
    pub name: &'static str,
    pub render: RenderFn,
}

/// Adapter: the real baseline function ignores the trailing
/// `&mut DesignUiState` — it ships unmodified, byte-for-byte the shipped
/// `mlc-app-layout` renderer.
#[allow(clippy::too_many_arguments)]
fn render_baseline(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    default_modal_y: f64,
    state: &PrimitiveSettingsState,
    primitives: &[Box<dyn Primitive>],
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    toolbar_state: &ToolbarState,
    current_time_ms: u64,
    coord: &mut InputCoordinator,
    templates: &[PrimitiveTemplate],
    _design_ui: &mut DesignUiState,
) -> PrimitiveSettingsResult {
    render_primitive_settings_modal(
        ctx, screen_w, screen_h, default_modal_y, state, primitives, frame_theme, toolbar_theme,
        toolbar_state, current_time_ms, coord, templates,
    )
}

/// The fourth grid cell — deliberately empty (see the module doc).
#[allow(clippy::too_many_arguments)]
fn render_empty(
    _ctx: &mut dyn RenderContext,
    _screen_w: f64,
    _screen_h: f64,
    _default_modal_y: f64,
    _state: &PrimitiveSettingsState,
    _primitives: &[Box<dyn Primitive>],
    _frame_theme: &FrameTheme,
    _toolbar_theme: &ToolbarTheme,
    _toolbar_state: &ToolbarState,
    _current_time_ms: u64,
    _coord: &mut InputCoordinator,
    _templates: &[PrimitiveTemplate],
    _design_ui: &mut DesignUiState,
) -> PrimitiveSettingsResult {
    PrimitiveSettingsResult::default()
}

/// The registry: baseline (unmodified), the approved 3-tab design
/// (`crate::approved_design`), the templates panel (`crate::templates_variant`),
/// and an empty fourth cell.
pub const VARIANTS: &[Variant] = &[
    Variant { name: "baseline", render: render_baseline },
    Variant { name: "approved", render: crate::approved_design::render },
    Variant { name: "templates", render: crate::templates_variant::render_templates_panel },
    Variant { name: "\u{2014}", render: render_empty },
];
