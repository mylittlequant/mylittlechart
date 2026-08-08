//! The approved 3-tab design — Вид · Уровни · Размещение — now that it has
//! been ported into `mlc-app-layout` as the REAL primitive-settings modal
//! (owner direction, 2026-08-07, after live review; hard-cutover port
//! 2026-08-08). This module is a thin wrapper matching
//! `crate::variants::RenderFn`'s signature so `crate::variants::VARIANTS`
//! can list it by name — it delegates the entire render to
//! `render_primitive_settings_modal`, which now IS the approved design,
//! not a fallback baseline sitting next to it. That is the whole reason
//! this crate hosts the real `mlc-app-layout` widget builders instead of
//! drawing its own approximation.

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

#[allow(clippy::too_many_arguments)]
pub fn render(
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
    template_order: &[String],
) -> PrimitiveSettingsResult {
    render_primitive_settings_modal(
        ctx, screen_w, screen_h, default_modal_y, state, primitives, frame_theme, toolbar_theme,
        toolbar_state, current_time_ms, coord, templates, template_order,
    )
}
