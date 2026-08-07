//! The approved 3-tab design — Вид · Уровни · Размещение — replacing the
//! earlier four-sector Levels-tab-only comparison (owner direction,
//! 2026-08-07, after live review of the merged B design). Source of
//! truth: `crates/mlc-app-shell-web/pkg/fib_modal.html`.
//!
//! Unlike the earlier variants, this one does NOT delegate any tab to
//! `render_primitive_settings_modal` — the approved design replaces the
//! whole five-tab structure (Style/Text/Coordinates/Levels/Visibility)
//! with its own three, so there is no baseline tab left to fall through
//! to. `state.active_tab` (`PrimitiveSettingsState`, baseline's own five
//! tabs) is therefore irrelevant here; `design_ui.custom_tab` is the real
//! tab state, and the tab strip this module builds is its own
//! (`crate::shell_common::begin_tab_body_shell` takes caller-built
//! `ModalTab`s precisely so this can happen without touching baseline's
//! `PrimitiveSettingsTab` enum).

use mlc_app_layout::layout::render_frame::PrimitiveSettingsResult;
use mlc_app_layout::layout::toolbar_state::ToolbarState;
use mlc_app_layout::layout::FrameTheme;
use mlc_app_layout::ui::modal_settings::PrimitiveSettingsState;
use mlc_app_layout::ui::toolbar_theme::ToolbarTheme;
use mlc_app_layout::ui::widgets::ModalTab;
use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::render::RenderContext;
use mlc_core::templates::PrimitiveTemplate;
use uzor::input::InputCoordinator;

use crate::design_state::{CustomTab, DesignUiState};
use crate::shell_common;
use crate::{approved_levels, approved_placement, approved_vid};

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
    _toolbar_state: &ToolbarState,
    _current_time_ms: u64,
    coord: &mut InputCoordinator,
    templates: &[PrimitiveTemplate],
    design_ui: &mut DesignUiState,
) -> PrimitiveSettingsResult {
    let mut result = PrimitiveSettingsResult::default();
    let Some(prim_id) = state.primitive_id else { return result };
    let Some(prim) = primitives.iter().find(|p| p.data().id == prim_id) else { return result };
    let prim = prim.as_ref();

    let tab_items: Vec<ModalTab<'_>> = CustomTab::ALL.iter().map(|t| ModalTab::new(t.id(), t.label())).collect();
    let active_idx = CustomTab::ALL.iter().position(|t| *t == design_ui.custom_tab).unwrap_or(0);

    let content_height = match design_ui.custom_tab {
        CustomTab::Vid => approved_vid::content_height(),
        CustomTab::Levels => approved_levels::content_height(design_ui),
        CustomTab::Placement => approved_placement::content_height(prim),
    };

    let body = shell_common::begin_tab_body_shell(
        ctx, coord, screen_w, screen_h, default_modal_y, state, prim.display_name(), &tab_items, active_idx,
        frame_theme, toolbar_theme, content_height, &mut result,
    );

    let x0 = body.content_left;
    let x1 = body.content_right;
    let mut row_y = body.row_y;

    match design_ui.custom_tab {
        CustomTab::Vid => {
            approved_vid::draw(ctx, toolbar_theme, &body.widget_theme, state, &mut result, x0, x1, &mut row_y, prim);
        }
        CustomTab::Levels => {
            approved_levels::draw(ctx, toolbar_theme, &body.widget_theme, state, &mut result, x0, x1, &mut row_y, prim, design_ui);
        }
        CustomTab::Placement => {
            approved_placement::draw(ctx, toolbar_theme, &body.widget_theme, state, &mut result, x0, x1, &mut row_y, prim, design_ui);
        }
    }

    shell_common::end_tab_body_shell(ctx, coord, body, &mut result, content_height, state, templates, toolbar_theme);

    result
}
