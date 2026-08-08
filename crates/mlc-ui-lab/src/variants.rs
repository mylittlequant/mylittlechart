//! Registry of primitive-settings modal design variants.
//!
//! A "variant" is a single function matching [`RenderFn`] — exactly the
//! real `render_primitive_settings_modal` signature. Before the 2026-08-08
//! port this carried one extra trailing parameter, `&mut DesignUiState`,
//! for the lab-only per-sector UI state the approved design needed and
//! that had no home on `PrimitiveSettingsState` — that state is folded
//! into `PrimitiveSettingsState` itself now (the app owns it, it is the
//! real modal's state), so every variant here is a direct, unmodified
//! `render_primitive_settings_modal` call — `approved` included, see
//! `crate::approved_design`'s own module doc.
//!
//! Two-sector layout (owner direction, 2026-08-07, after live review of the
//! four-sector Levels-tab comparison): `baseline` next to the `approved`
//! design side by side is the whole before/after; the templates panel gets
//! its own third sector rather than being crammed into either; the fourth
//! sector is empty by design (`crate::lib`'s 2×2 grid stays 2×2, drawing
//! nothing in the unused cell is simpler and safer than reshaping the
//! grid). `crate::lib`'s sector loop draws all four every frame — there is
//! no "active variant" to switch between.
//!
//! Since the hard-cutover port, `baseline` and `approved` call the exact
//! SAME function and so render identically — the earlier five-tab layout
//! no longer exists anywhere in the codebase to compare against. Both
//! sectors are kept (rather than collapsed to one) because the point of
//! this registry is comparing named variants, and removing `baseline`
//! would be a design decision about the LAB, not something this port asked
//! for.

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

/// The real `render_primitive_settings_modal` signature.
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
    &[String],
) -> PrimitiveSettingsResult;

/// One named design.
pub struct Variant {
    /// Shown as the sector label and in the debug readout.
    pub name: &'static str,
    pub render: RenderFn,
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
    _template_order: &[String],
) -> PrimitiveSettingsResult {
    PrimitiveSettingsResult::default()
}

/// The registry: baseline (== the real modal, unmodified), the approved
/// design (`crate::approved_design` — also the real modal, see its own
/// module doc for why), the templates panel
/// (`crate::templates_variant::render_templates_panel`), and an empty
/// fourth cell.
pub const VARIANTS: &[Variant] = &[
    Variant { name: "baseline", render: render_primitive_settings_modal },
    Variant { name: "approved", render: crate::approved_design::render },
    Variant { name: "templates", render: crate::templates_variant::render_templates_panel },
    Variant { name: "\u{2014}", render: render_empty },
];
