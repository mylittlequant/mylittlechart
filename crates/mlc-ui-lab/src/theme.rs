//! Theme plumbing + the lab's own preset switcher bar.
//!
//! `FrameTheme` / `ToolbarTheme` construction below is a field-for-field
//! mirror of `ChartPanelApp::frame_theme_for_render` /
//! `::toolbar_theme_for_render` in
//! `mlc-app/src/chart_panel_app.rs:334-405`. Those are methods on
//! `ChartPanelApp` (mlc-app crate) — out of scope for this lab's dependency
//! list — but the mapping itself touches only `mlc_core::theme::ThemeManager`
//! / `RuntimeTheme` (mlc-core) and `FrameTheme` / `ToolbarTheme`
//! (mlc-app-layout), both of which the lab already depends on. There is no
//! hand-written hex colour here: every value is read off
//! `theme_manager.current()`, exactly as the app does it.

use mlc_core::theme::ThemeManager;
use mlc_app_layout::layout::FrameTheme;
use mlc_app_layout::ui::toolbar_theme::ToolbarTheme;

/// Mirrors `ChartPanelApp::frame_theme_for_render` (mlc-app/src/chart_panel_app.rs:334).
pub fn frame_theme(theme_manager: &ThemeManager) -> FrameTheme {
    let rt = theme_manager.current();
    FrameTheme {
        toolbar_bg: rt.colors.toolbar_bg.clone(),
        modal_bg: rt.modal_bg_styled(),
        toolbar_border: rt.colors.ui_border.clone(),
        chart_border: rt.chart.chart_border.clone(),
        frame_border: rt.chart.frame_border.clone(),
        show_scale_separators: true,
    }
}

/// Mirrors `ChartPanelApp::toolbar_theme_for_render` (mlc-app/src/chart_panel_app.rs:372).
pub fn toolbar_theme(theme_manager: &ThemeManager) -> ToolbarTheme {
    let rt = theme_manager.current();
    ToolbarTheme {
        background: rt.colors.toolbar_bg.clone(),
        dropdown_bg: rt.menu_bg_styled(),
        separator: rt.colors.toolbar_divider.clone(),
        item_bg_hover: rt.colors.button_bg_hover.clone(),
        item_bg_active: rt.colors.button_bg_active.clone(),
        button_bg: rt.colors.button_bg.clone(),
        button_bg_hover: rt.colors.button_bg_hover.clone(),
        item_text: rt.colors.text_primary.clone(),
        item_text_muted: rt.colors.text_secondary.clone(),
        item_text_faint: rt.colors.text_muted.clone(),
        item_text_hidden: mlc_core::apply_opacity(&rt.colors.text_primary, 0.5),
        item_text_hover: rt.colors.text_primary.clone(),
        item_text_active: rt.colors.text_active.clone(),
        accent: rt.colors.accent.clone(),
        accent_hover: rt.colors.accent_hover.clone(),
        success: rt.colors.success.clone(),
        danger: rt.colors.danger.clone(),
        warning: rt.colors.warning.clone(),
        sidebar_style: rt.style_params.toolbar_sidebar_style,
        terminal_bg: rt.colors.toolbar_bg.clone(),
        pane_header_focused: rt.colors.accent.clone(),
        pane_header_hover: rt.colors.button_bg_hover.clone(),
        pane_header_idle: rt.colors.toolbar_bg.clone(),
        bubble_user_bg: rt.colors.bubble_user_bg.clone(),
        bubble_user_border: rt.colors.accent.clone(),
        bubble_user_text: rt.colors.text_primary.clone(),
        bubble_tool_bg: rt.colors.toolbar_bg.clone(),
        bubble_tool_text: rt.colors.text_secondary.clone(),
        selection: rt.colors.text_selection.clone(),
        danger_hover_bg: mlc_core::apply_opacity(&rt.colors.danger, 0.25),
    }
}

/// Height of the lab's own control bar (theme presets + reopen button),
/// drawn above the modal harness. Not part of mlc-app-layout — this strip
/// is lab-only chrome, the equivalent of a devtools toolbar.
pub const BAR_HEIGHT: f64 = 40.0;

/// Widget id prefix for a preset button: `"lab:theme:{preset_name}"`.
pub fn preset_widget_id(preset: &str) -> String {
    format!("lab:theme:{preset}")
}

/// Widget id for the "reopen primitive settings" button.
pub const REOPEN_WIDGET_ID: &str = "lab:reopen_primitive_settings";

/// Widget id for the dev bar's "Design compare" toggle — the four-sector
/// primitive-settings design-comparison grid (`crate::variants`),
/// relocated here off the app-shell "Modals" button in wave 3 (that
/// button now opens `crate::shell::launcher_menu` — see
/// `crate::shell::MODALS_TOGGLE_ID`'s own doc for why). Lives in the dev
/// bar rather than the shell because it toggles a lab-only debugging
/// surface, not app chrome.
pub const DESIGN_COMPARE_WIDGET_ID: &str = "lab:design_compare";

/// Z-order the lab bar's own layer is pushed at.
///
/// Bug fix (owner report, 2026-08-07): the bar's buttons used to register on
/// `InputCoordinator`'s default layer (`LayerId::main()`, z=0, non-modal).
/// `begin_modal_shell` pushes the primitive-settings modal on
/// `ZLayer::Modal` (`mlc-app-layout/src/ui/z_order.rs:33,56` → z=3,
/// `is_modal() == true`). `InputCoordinator::hit_test_at_filtered`
/// (`uzor/uzor/src/input/core/coordinator.rs:657-693`) walks layers highest
/// z-order first and, the moment it reaches a layer flagged `modal` that did
/// NOT contain a matching widget at the click point, returns `None`
/// unconditionally — it does not fall through to lower layers, regardless of
/// where on screen the click landed. With the bar on z=0 and the modal on
/// z=3, every click anywhere outside the modal's own widgets (including the
/// theme chips, which sit above the modal at y<40) hit the modal layer
/// first, missed every modal widget, and the search aborted there — the
/// click never reached the bar's z=0 registrations at all. `ThemeManager::
/// set_preset` was never unreachable code; it was simply never called,
/// because `resp.clicked` was never true for `lab:theme:*` ids while any
/// modal layer was open. Pushing the bar at a z-order above every layer the
/// real modal can use (`ZLayer::ColorPicker` = 6 is the highest, see
/// `z_order.rs:59`) makes the bar the first layer checked, so its own
/// buttons resolve before the modal barrier is ever consulted.
const LAB_BAR_Z: u32 = 100;

fn lab_bar_layer(coord: &mut uzor::input::InputCoordinator) -> uzor::input::LayerId {
    let id = uzor::input::LayerId::new("lab_bar");
    coord.push_layer(id.clone(), LAB_BAR_Z, false);
    id
}

/// Draw the lab's control bar: one button per theme preset (`ThemeManager::presets()`,
/// the exact list the app ships) — applies to all four grid sectors at
/// once — plus a button that reopens any sector whose modal was closed via
/// its own close button.
///
/// Registers hit regions on the coordinator's main layer under
/// `lab:theme:{preset}` / `REOPEN_WIDGET_ID` — read back in
/// `crate::render_one_frame`'s response loop.
pub fn draw_lab_bar(
    ctx: &mut dyn mlc_core::render::RenderContext,
    coord: &mut uzor::input::InputCoordinator,
    screen_w: f64,
    theme_manager: &ThemeManager,
    any_sector_closed: bool,
    show_variants: bool,
) {
    use mlc_core::render::{TextAlign, TextBaseline};
    use uzor::input::Sense;
    use uzor::types::Rect;

    let layer = lab_bar_layer(coord);

    let ft = frame_theme(theme_manager);
    let tt = toolbar_theme(theme_manager);

    ctx.set_fill_color(&ft.toolbar_bg);
    ctx.fill_rect(0.0, 0.0, screen_w, BAR_HEIGHT);
    ctx.set_stroke_color(&ft.toolbar_border);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(0.0, BAR_HEIGHT);
    ctx.line_to(screen_w, BAR_HEIGHT);
    ctx.stroke();

    ctx.set_font("12px sans-serif");
    let mut x = 8.0;
    let active_preset = theme_manager.preset_name().to_string();
    for preset in ThemeManager::presets() {
        let label_w = ctx.measure_text(preset).max(40.0) + 16.0;
        let rect = Rect { x, y: 8.0, width: label_w, height: BAR_HEIGHT - 16.0 };
        let is_active = *preset == active_preset;
        let bg = if is_active { &tt.item_bg_active } else { &tt.button_bg };
        ctx.set_fill_color(bg);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
        ctx.set_fill_color(if is_active { &tt.item_text_active } else { &tt.item_text });
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(preset, rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        coord.register_on_layer(preset_widget_id(preset), rect, Sense::CLICK, &layer);
        x += label_w + 6.0;
    }

    let reopen_label = mlc_core::i18n::t_shell(if any_sector_closed {
        mlc_core::i18n::ShellKey::LabReopenSectors
    } else {
        mlc_core::i18n::ShellKey::LabAllSectorsOpen
    });
    let reopen_w = ctx.measure_text(reopen_label) + 16.0;
    let reopen_rect = Rect { x: screen_w - reopen_w - 8.0, y: 8.0, width: reopen_w, height: BAR_HEIGHT - 16.0 };
    ctx.set_fill_color(if any_sector_closed { &tt.accent } else { &tt.button_bg });
    ctx.fill_rounded_rect(reopen_rect.x, reopen_rect.y, reopen_rect.width, reopen_rect.height, 4.0);
    ctx.set_fill_color(&tt.item_text_active);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(reopen_label, reopen_rect.x + reopen_rect.width / 2.0, reopen_rect.y + reopen_rect.height / 2.0);
    if any_sector_closed {
        coord.register_on_layer(REOPEN_WIDGET_ID, reopen_rect, Sense::CLICK, &layer);
    }

    // Relocated off the app-shell "Modals" button in wave 3 — see
    // `DESIGN_COMPARE_WIDGET_ID`'s own doc for why it lives here now.
    let dc_label = mlc_core::i18n::t_shell(mlc_core::i18n::ShellKey::DesignCompareToggle);
    let dc_w = ctx.measure_text(dc_label) + 16.0;
    let dc_rect = Rect { x: reopen_rect.x - dc_w - 6.0, y: 8.0, width: dc_w, height: BAR_HEIGHT - 16.0 };
    ctx.set_fill_color(if show_variants { &tt.item_bg_active } else { &tt.button_bg });
    ctx.fill_rounded_rect(dc_rect.x, dc_rect.y, dc_rect.width, dc_rect.height, 4.0);
    ctx.set_fill_color(if show_variants { &tt.item_text_active } else { &tt.item_text });
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(dc_label, dc_rect.x + dc_rect.width / 2.0, dc_rect.y + dc_rect.height / 2.0);
    coord.register_on_layer(DESIGN_COMPARE_WIDGET_ID, dc_rect, Sense::CLICK, &layer);
}
