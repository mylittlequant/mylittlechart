//! User settings modal renderer — tabbed settings dialog.
//!
//! Tabs:
//!   - General    : placeholder for future general settings
//!   - Performance: RecalcMode radio group selector

use crate::engine::render::RenderContext;
use crate::engine::render::draw_svg_icon;
use uzor::render::{TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;
use uzor::input::Sense;
use crate::i18n::{current_language, TextKey, UserSettingsKey};
use crate::ui::modal_settings::{UserSettingsState, UserSettingsTab};
use crate::ui::scroll_state::ScrollState;
use crate::ui::toolbar_render::ToolbarTheme;
use crate::ui::widgets::{render_modal_frame_only, ModalTheme, WidgetTheme, RadioOption, draw_radio_group};
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig};
use crate::ui::widgets::{render_single_slider, SliderConfig, SliderTrackInfo};
use crate::ui::widgets::types::WidgetState;
use crate::layout::render_chart::FrameTheme;
use crate::layout::render_frame::UserSettingsResult;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::ui::z_order::ZLayer;
use crate::ui::Icon;
use crate::ui::scroll_widget::ScrollableContainer;

/// Render the User Settings modal dialog.
pub fn render_user_settings_modal(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    state: &UserSettingsState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> UserSettingsResult {
    let mut result = UserSettingsResult::default();

    let modal_w = 540.0;
    let modal_h = 500.0;
    let header_h = 44.0;
    let sidebar_w = 48.0;
    let padding = 20.0;

    // Position (draggable, centered by default)
    let (modal_x, modal_y) = state.position.unwrap_or_else(|| {
        ((screen_w - modal_w) / 2.0, (screen_h - modal_h) / 2.0)
    });
    let modal_x = modal_x.max(0.0).min(screen_w - modal_w);
    let modal_y = modal_y.max(0.0).min(screen_h - modal_h);

    let modal_rect = WidgetRect::new(modal_x, modal_y, modal_w, modal_h);
    result.modal_rect = modal_rect;

    // Modal frame (shadow + background + border)
    let modal_theme = ModalTheme::from_frame_theme(
        &frame_theme.toolbar_bg,
        &toolbar_theme.separator,
        &toolbar_theme.item_text,
        &toolbar_theme.item_text_hover,
        &toolbar_theme.separator,
    );
    render_modal_frame_only(ctx, modal_rect, &modal_theme, 0.0);

    // InputCoordinator layer
    let layer_id = ZLayer::Modal.push_named(input_coordinator, "user_settings");

    // Register modal background (absorbs clicks so they don't fall through)
    input_coordinator.register_on_layer(
        "user_settings:modal_bg",
        uzor::types::Rect::new(modal_x, modal_y, modal_w, modal_h),
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    // =========================================================================
    // Header
    // =========================================================================
    let header_rect = WidgetRect::new(modal_x, modal_y, modal_w, header_h);
    result.header_rect = header_rect;

    input_coordinator.register_on_layer(
        "user_settings:header",
        uzor::types::Rect::new(modal_x, modal_y, modal_w, header_h),
        uzor::input::Sense::DRAG,
        &layer_id,
    );

    let text_color = &toolbar_theme.item_text;
    ctx.set_font("14px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(UserSettingsKey::Title.get(current_language()), modal_x + padding, modal_y + header_h / 2.0);

    // Close button (X) — right side of header
    let close_size = 18.0;
    let close_x = modal_x + modal_w - close_size - 12.0;
    let close_y = modal_y + (header_h - close_size) / 2.0;
    let close_rect = WidgetRect::new(close_x, close_y, close_size, close_size);
    result.close_btn_rect = close_rect;

    draw_svg_icon(ctx, Icon::Close.svg(), close_x, close_y, close_size, close_size, text_color);

    input_coordinator.register_on_layer(
        "user_settings:close",
        uzor::types::Rect::new(close_x, close_y, close_size, close_size),
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    // Header separator
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, modal_y + header_h);
    ctx.line_to(modal_x + modal_w, modal_y + header_h);
    ctx.stroke();

    // =========================================================================
    // Left sidebar (vertical icon tabs)
    // =========================================================================
    let sidebar_x = modal_x;
    let sidebar_y = modal_y + header_h;
    let content_h = modal_h - header_h;

    // Sidebar right border
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(sidebar_x + sidebar_w, sidebar_y);
    ctx.line_to(sidebar_x + sidebar_w, sidebar_y + content_h);
    ctx.stroke();

    let tab_button_h = 44.0;
    let tab_icon_size = 20.0;

    for tab in UserSettingsTab::all() {
        let tab_idx = UserSettingsTab::all().iter().position(|t| t == tab).unwrap_or(0);
        let tab_y = sidebar_y + tab_idx as f64 * tab_button_h;
        let is_active = *tab == state.active_tab;

        if is_active {
            ctx.draw_sidebar_active_item(
                sidebar_x, tab_y, sidebar_w, tab_button_h,
                &toolbar_theme.accent, &toolbar_theme.item_bg_active, 3.0,
            );
        }

        let icon_x = sidebar_x + (sidebar_w - tab_icon_size) / 2.0;
        let icon_y = tab_y + (tab_button_h - tab_icon_size) / 2.0;
        let icon_color = if is_active { &toolbar_theme.item_text_active } else { text_color };

        // Choose icon per tab — Settings for General, Lock for Sync, Grid for Performance, Layers for Server
        let icon = match tab {
            UserSettingsTab::General => Icon::Settings,
            UserSettingsTab::Sync => Icon::Lock,
            UserSettingsTab::Performance => Icon::Grid,
            UserSettingsTab::Server => Icon::Layers,
        };
        draw_svg_icon(ctx, icon.svg(), icon_x, icon_y, tab_icon_size, tab_icon_size, icon_color);

        let tab_rect = WidgetRect::new(sidebar_x, tab_y, sidebar_w, tab_button_h);
        result.tab_rects.push((tab.id().to_string(), tab_rect));

        let hit_id = format!("user_settings:tab:{}", tab.id());
        input_coordinator.register_on_layer(
            hit_id.as_str(),
            uzor::types::Rect::new(sidebar_x, tab_y, sidebar_w, tab_button_h),
            uzor::input::Sense::CLICK,
            &layer_id,
        );
    }

    // =========================================================================
    // Content area
    // =========================================================================
    let sidebar_gap = 6.0;
    let content_x = modal_x + sidebar_w + sidebar_gap;
    let content_y = modal_y + header_h;
    let content_w = modal_w - sidebar_w - sidebar_gap;

    // Tab title
    ctx.set_font("bold 14px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(state.active_tab.label(), content_x + padding, content_y + padding);

    let settings_y = content_y + padding + 30.0;
    let scroll_viewport_h = modal_h - header_h - padding - 30.0;

    // WidgetTheme used for scrollbar rendering in General / Sync tabs
    let scroll_widget_theme = WidgetTheme {
        bg_normal:      toolbar_theme.item_bg_hover.clone(),
        bg_hover:       toolbar_theme.item_bg_hover.clone(),
        bg_pressed:     toolbar_theme.item_bg_active.clone(),
        bg_disabled:    toolbar_theme.item_bg_hover.clone(),
        text_normal:    toolbar_theme.item_text.clone(),
        text_hover:     toolbar_theme.item_text_active.clone(),
        text_disabled:  toolbar_theme.item_text_muted.clone(),
        border_normal:  toolbar_theme.separator.clone(),
        border_hover:   toolbar_theme.separator.clone(),
        border_focused: toolbar_theme.item_bg_active.clone(),
        accent:         toolbar_theme.accent.clone(),
        accent_hover:   toolbar_theme.accent.clone(),
        success:        "#26a69a".to_string(),
        warning:        "#ff9800".to_string(),
        danger:         "#ef5350".to_string(),
    };

    match state.active_tab {
        UserSettingsTab::General => {
            let viewport_rect = WidgetRect::new(
                content_x + padding,
                settings_y,
                content_w - padding * 2.0,
                scroll_viewport_h,
            );
            render_general_tab(
                ctx,
                viewport_rect,
                state,
                text_color,
                toolbar_theme,
                frame_theme,
                &scroll_widget_theme,
                current_time_ms,
                input_coordinator,
                &layer_id,
                &mut result,
            );
        }
        UserSettingsTab::Sync => {}
        UserSettingsTab::Performance => {
            let viewport_rect = WidgetRect::new(
                content_x + padding,
                settings_y,
                content_w - padding * 2.0,
                scroll_viewport_h,
            );
            render_performance_tab(
                ctx,
                viewport_rect,
                state,
                toolbar_theme,
                &scroll_widget_theme,
                input_coordinator,
                &layer_id,
                &mut result,
            );
        }
        UserSettingsTab::Server => {
            let viewport_rect = WidgetRect::new(
                content_x + padding,
                settings_y,
                content_w - padding * 2.0,
                scroll_viewport_h,
            );
            render_server_tab(
                ctx,
                viewport_rect,
                state,
                toolbar_theme,
                frame_theme,
                &scroll_widget_theme,
                input_coordinator,
                &layer_id,
                &mut result,
            );
        }
    }

    result
}

// =============================================================================
// Tab content renderers
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_general_tab(
    ctx: &mut dyn RenderContext,
    viewport_rect: WidgetRect,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    scroll_widget_theme: &WidgetTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    let container = ScrollableContainer::new(
        viewport_rect,
        &state.general_tab_scroll,
        None,
    );
    container.begin(ctx);
    let x = viewport_rect.x;
    let available_w = container.content_width();
    let mut cy = container.content_y();

    // ── Section: PROFILE ─────────────────────────────────────────────────────
    ctx.set_font("600 11px sans-serif");
    ctx.set_fill_color("rgba(244,205,99,0.7)");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::SectionProfile.get(current_language()), x, cy);
    cy += 20.0;

    cy = render_profile_section(
        ctx, x, cy, available_w, state, text_color, toolbar_theme, frame_theme,
        current_time_ms, input_coordinator, layer_id, result,
    );
    cy += 16.0;

    // ── Section: LANGUAGE ────────────────────────────────────────────────────
    ctx.set_font("600 11px sans-serif");
    ctx.set_fill_color("rgba(244,205,99,0.7)");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::SectionLanguage.get(current_language()), x, cy);
    cy += 20.0;

    {
        let lang_options = [
            RadioOption {
                key: "en",
                label: "English",
                description: "",
            },
            RadioOption {
                key: "ru",
                label: "Русский",
                description: "",
            },
        ];

        let lang_selected = match state.language.as_str() {
            "ru" => 1,
            _    => 0,
        };

        let lang_radio_result = draw_radio_group(
            ctx,
            &lang_options,
            lang_selected,
            state.hovered_item_id.as_deref(),
            x,
            cy,
            available_w,
            scroll_widget_theme,
        );

        for (i, (rx, ry, rw, rh)) in lang_radio_result.option_rects.iter().enumerate() {
            let hit_id = format!("user_settings:language:{}", lang_options[i].key);
            result.content_items.push((
                format!("language:{}", lang_options[i].key),
                WidgetRect::new(*rx, *ry, *rw, *rh),
            ));
            input_coordinator.register_on_layer(
                hit_id.as_str(),
                uzor::types::Rect::new(*rx, *ry, *rw, *rh),
                Sense::CLICK | Sense::HOVER,
                layer_id,
            );
        }

        cy += lang_options.len() as f64 * (52.0 + 8.0) - 8.0 + 16.0;
    }

    // ── Version info (always shown at bottom) ─────────────────────────────────
    ctx.set_font("600 11px sans-serif");
    ctx.set_fill_color("rgba(244,205,99,0.7)");
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::SectionVersion.get(current_language()), x, cy);
    cy += 18.0;

    ctx.set_font("700 18px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.fill_text(&format!("v{}", env!("CARGO_PKG_VERSION")), x, cy);
    cy += 30.0;

    // "Show Welcome Wizard" debug button
    let btn_h = 24.0;
    let btn_w = available_w.min(180.0);
    let is_wizard_hovered = state.hovered_item_id.as_deref() == Some("show_wizard");
    let wizard_bg = if is_wizard_hovered { "rgba(255,255,255,0.10)" } else { &toolbar_theme.item_bg_hover };
    ctx.set_fill_color(wizard_bg);
    ctx.fill_rounded_rect(x, cy, btn_w, btn_h, 4.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, cy, btn_w, btn_h, 4.0);
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(if is_wizard_hovered { "rgba(254,255,238,0.80)" } else { "rgba(254,255,238,0.55)" });
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(UserSettingsKey::ShowWelcomeWizard.get(current_language()), x + btn_w / 2.0, cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    result.content_items.push(("show_wizard".to_string(), WidgetRect::new(x, cy, btn_w, btn_h)));
    input_coordinator.register_on_layer(
        "user_settings:show_wizard",
        uzor::types::Rect::new(x, cy, btn_w, btn_h),
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );
    cy += btn_h + 8.0;

    let total_content_h = cy - container.content_y();
    let scroll_result = container.end(ctx, total_content_h, scroll_widget_theme);
    result.scroll_viewport_rect = Some(viewport_rect);
    result.scroll_content_height = scroll_result.content_height;
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
    result.scroll_viewport_height = scroll_result.viewport_height;

    if let Some(ref hr) = result.scrollbar_handle_rect {
        let inflated = uzor::types::Rect::new(hr.x - 5.0, hr.y, hr.width + 10.0, hr.height);
        input_coordinator.register_on_layer(
            "user_settings:scrollbar_handle",
            inflated,
            uzor::input::Sense::DRAG,
            layer_id,
        );
    }
    if let Some(ref tr) = result.scrollbar_track_rect {
        input_coordinator.register_on_layer(
            "user_settings:scrollbar_track",
            uzor::types::Rect::new(tr.x, tr.y, tr.width, tr.height),
            uzor::input::Sense::CLICK,
            layer_id,
        );
    }
    if let Some(ref vp) = result.scroll_viewport_rect {
        input_coordinator.register_on_layer(
            "user_settings:scroll_viewport",
            uzor::types::Rect::new(vp.x, vp.y, vp.width, vp.height),
            uzor::input::Sense::SCROLL,
            layer_id,
        );
    }
}

// =============================================================================
// Profile section renderer
// =============================================================================

/// Maps an avatar key string to a short display tag used as a colored label.
fn avatar_tag(key: &str) -> &'static str {
    match key {
        "chart"  => "CH",
        "rocket" => "RK",
        "shield" => "SH",
        "fire"   => "FR",
        "star"   => "ST",
        "moon"   => "MN",
        "sun"    => "SN",
        "ghost"  => "GH",
        _        => "??",
    }
}

/// Returns a CSS-style hex color string for an avatar key.
fn avatar_color(key: &str) -> &'static str {
    match key {
        "chart"  => "#4a8fe7",
        "rocket" => "#f07030",
        "shield" => "#4caf50",
        "fire"   => "#e53935",
        "star"   => "#fdd835",
        "moon"   => "#9c6dd8",
        "sun"    => "#ffb300",
        "ghost"  => "#90a4ae",
        _        => "#888888",
    }
}

/// Renders a small colored dot (circle) for a profile avatar.
fn draw_avatar_dot(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_fill_color(color);
    ctx.begin_path();
    ctx.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
    ctx.fill();
}

#[allow(clippy::too_many_arguments)]
fn render_profile_section(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    available_w: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) -> f64 {
    let mut cy = y;

    // ── Unified profile list ───────────────────────────────────────────────────
    // Each row: [radio dot] [avatar dot] Name (mode) [Rename] [Avatar] -or- [Delete]
    // Active row shows Rename + Avatar buttons; inactive rows show Delete + are clickable.
    let profile_row_h = 32.0;
    let btn_h = 22.0;
    let btn_gap = 6.0;
    let small_btn_w = 46.0;

    for (id, name, avatar) in &state.available_profiles {
        // Use runtime_profile_id (the ACTUALLY loaded profile) so that buttons
        // remain on the correct row even after a pending profile_switch.
        let is_active = *id == state.runtime_profile_id;
        let is_renaming = state.profile_rename_mode
            && state.profile_rename_target_id.as_deref() == Some(id.as_str());
        let is_avatar_open = state.show_avatar_picker
            && state.profile_avatar_target_id.as_deref() == Some(id.as_str());

        let row_cy = cy + profile_row_h / 2.0;

        // ── Row hover highlight for inactive rows ──
        if !is_active {
            let row_hover_id = format!("profile_switch:{}", id);
            if state.hovered_item_id.as_deref() == Some(row_hover_id.as_str()) {
                ctx.set_fill_color("rgba(255,255,255,0.04)");
                ctx.fill_rounded_rect(x, cy, available_w, profile_row_h, 3.0);
            }
        }

        // ── Radio dot (filled = active, empty ring = inactive) ──
        let radio_r = 5.0;
        let radio_cx = x + radio_r + 2.0;
        let radio_cy = row_cy;
        if is_active {
            // Outer ring in accent color
            ctx.set_stroke_color(&toolbar_theme.accent);
            ctx.set_stroke_width(1.5);
            ctx.begin_path();
            ctx.arc(radio_cx, radio_cy, radio_r, 0.0, std::f64::consts::TAU);
            ctx.stroke();
            // Inner fill dot
            ctx.set_fill_color(&toolbar_theme.accent);
            ctx.begin_path();
            ctx.arc(radio_cx, radio_cy, radio_r - 2.5, 0.0, std::f64::consts::TAU);
            ctx.fill();
        } else {
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.5);
            ctx.begin_path();
            ctx.arc(radio_cx, radio_cy, radio_r, 0.0, std::f64::consts::TAU);
            ctx.stroke();
        }

        // ── Avatar dot ──
        let dot_r = 5.0;
        let dot_cx = radio_cx + radio_r + 8.0 + dot_r;
        let dot_cy = row_cy;
        draw_avatar_dot(ctx, dot_cx, dot_cy, dot_r, avatar_color(avatar));

        // ── Name + mode label ──
        let name_x = dot_cx + dot_r + 8.0;
        let name_alpha = if is_active { "1.0" } else { "0.65" };
        let row_name_color = format!("rgba(254,255,238,{})", name_alpha);

        // Right-side buttons layout (computed from right edge)
        // All rows: [Rename] [Avatar] [Delete]
        // Active row: Delete is hidden (can't delete the running profile).
        // Inactive row: all three buttons visible + whole-row click area for switch.
        let right_edge = x + available_w;
        let delete_btn_x = right_edge - small_btn_w;
        let avatar_btn_x = delete_btn_x - small_btn_w - btn_gap;
        let rename_btn_x = avatar_btn_x - small_btn_w - btn_gap;

        if is_renaming {
            // Inline rename input replaces name text
            let confirm_btn_x = rename_btn_x;
            let cancel_btn_x = avatar_btn_x;
            let input_w = confirm_btn_x - name_x - btn_gap;
            let input_h = 22.0;
            let input_y = cy + (profile_row_h - input_h) / 2.0;

            let input_rect = WidgetRect::new(name_x, input_y, input_w, input_h);
            let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);

            let (sel_start, sel_end) = if let Some((lo, hi)) = state.profile_rename_editing.selection_range() {
                (Some(lo), Some(hi))
            } else {
                (None, None)
            };
            let rename_input_config = InputConfig::new(&state.profile_rename_editing.text)
                .with_focused(state.profile_rename_focused)
                .with_cursor(state.profile_rename_editing.cursor)
                .with_placeholder("Profile name...")
                .with_selection(sel_start, sel_end);

            let rename_input_result = draw_input(ctx, &rename_input_config, WidgetState::Normal, input_rect, &widget_theme);

            // Register click target so mouse clicks focus this input
            input_coordinator.register_on_layer(
                "user_settings:profile_rename_input",
                uzor::types::Rect::new(name_x, input_y, input_w, input_h),
                Sense::CLICK,
                layer_id,
            );

            // Draw blinking cursor when focused
            if state.profile_rename_focused && state.profile_rename_editing.is_cursor_visible(current_time_ms) {
                draw_input_cursor(
                    ctx,
                    rename_input_result.cursor_x,
                    rename_input_result.cursor_y,
                    rename_input_result.cursor_height,
                    text_color,
                );
            }

            let btn_y = cy + (profile_row_h - btn_h) / 2.0;

            // Save button (green tint)
            ctx.set_fill_color("rgba(76,175,80,0.2)");
            ctx.fill_rounded_rect(confirm_btn_x, btn_y, small_btn_w, btn_h, 3.0);
            ctx.set_stroke_color("rgba(76,175,80,0.6)");
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(confirm_btn_x, btn_y, small_btn_w, btn_h, 3.0);
            ctx.set_font("11px sans-serif");
            ctx.set_fill_color("#81c784");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(TextKey::Save.get(current_language()), confirm_btn_x + small_btn_w / 2.0, btn_y + btn_h / 2.0);
            ctx.set_text_align(TextAlign::Left);

            result.content_items.push(("profile_rename_confirm".to_string(), WidgetRect::new(confirm_btn_x, btn_y, small_btn_w, btn_h)));
            input_coordinator.register_on_layer(
                "user_settings:profile_rename_confirm",
                uzor::types::Rect::new(confirm_btn_x, btn_y, small_btn_w, btn_h),
                Sense::CLICK | Sense::HOVER,
                layer_id,
            );

            // Cancel button
            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rounded_rect(cancel_btn_x, btn_y, small_btn_w, btn_h, 3.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(cancel_btn_x, btn_y, small_btn_w, btn_h, 3.0);
            ctx.set_font("11px sans-serif");
            ctx.set_fill_color("rgba(254,255,238,0.7)");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(TextKey::Cancel.get(current_language()), cancel_btn_x + small_btn_w / 2.0, btn_y + btn_h / 2.0);
            ctx.set_text_align(TextAlign::Left);

            result.content_items.push(("profile_rename_cancel".to_string(), WidgetRect::new(cancel_btn_x, btn_y, small_btn_w, btn_h)));
            input_coordinator.register_on_layer(
                "user_settings:profile_rename_cancel",
                uzor::types::Rect::new(cancel_btn_x, btn_y, small_btn_w, btn_h),
                Sense::CLICK | Sense::HOVER,
                layer_id,
            );
        } else {
            // Normal display: name + mode label
            ctx.set_font(if is_active { "600 13px sans-serif" } else { "13px sans-serif" });
            ctx.set_fill_color(row_name_color.as_str());
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(name.as_str(), name_x, row_cy);

            let btn_y = cy + (profile_row_h - btn_h) / 2.0;

            // ── Rename button (shown on every row) ──
            {
                let rename_hover_id = format!("profile_rename:{}", id);
                let is_rename_hovered = state.hovered_item_id.as_deref() == Some(rename_hover_id.as_str());
                let rename_bg = if is_rename_hovered { "rgba(255,255,255,0.12)" } else { &toolbar_theme.item_bg_hover };
                ctx.set_fill_color(rename_bg);
                ctx.fill_rounded_rect(rename_btn_x, btn_y, small_btn_w, btn_h, 3.0);
                ctx.set_stroke_color(&toolbar_theme.separator);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rounded_rect(rename_btn_x, btn_y, small_btn_w, btn_h, 3.0);
                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(if is_rename_hovered { "rgba(254,255,238,0.95)" } else { "rgba(254,255,238,0.7)" });
                ctx.set_text_align(TextAlign::Center);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(UserSettingsKey::BtnRename.get(current_language()), rename_btn_x + small_btn_w / 2.0, btn_y + btn_h / 2.0);
                ctx.set_text_align(TextAlign::Left);

                let rename_hit_id = format!("user_settings:profile_rename:{}", id);
                result.content_items.push((format!("profile_rename:{}", id), WidgetRect::new(rename_btn_x, btn_y, small_btn_w, btn_h)));
                input_coordinator.register_on_layer(
                    rename_hit_id.as_str(),
                    uzor::types::Rect::new(rename_btn_x, btn_y, small_btn_w, btn_h),
                    Sense::CLICK | Sense::HOVER,
                    layer_id,
                );
            }

            // ── Avatar button (shown on every row) ──
            {
                let avatar_toggle_hover_id = format!("profile_avatar_toggle:{}", id);
                let is_avatar_hovered = state.hovered_item_id.as_deref() == Some(avatar_toggle_hover_id.as_str());
                let avatar_bg = if is_avatar_hovered { "rgba(255,255,255,0.12)" } else { &toolbar_theme.item_bg_hover };
                ctx.set_fill_color(avatar_bg);
                ctx.fill_rounded_rect(avatar_btn_x, btn_y, small_btn_w, btn_h, 3.0);
                ctx.set_stroke_color(&toolbar_theme.separator);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rounded_rect(avatar_btn_x, btn_y, small_btn_w, btn_h, 3.0);
                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(if is_avatar_hovered { "rgba(254,255,238,0.95)" } else { "rgba(254,255,238,0.7)" });
                ctx.set_text_align(TextAlign::Center);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(UserSettingsKey::BtnAvatar.get(current_language()), avatar_btn_x + small_btn_w / 2.0, btn_y + btn_h / 2.0);
                ctx.set_text_align(TextAlign::Left);

                let avatar_hit_id = format!("user_settings:profile_avatar_toggle:{}", id);
                result.content_items.push((format!("profile_avatar_toggle:{}", id), WidgetRect::new(avatar_btn_x, btn_y, small_btn_w, btn_h)));
                input_coordinator.register_on_layer(
                    avatar_hit_id.as_str(),
                    uzor::types::Rect::new(avatar_btn_x, btn_y, small_btn_w, btn_h),
                    Sense::CLICK | Sense::HOVER,
                    layer_id,
                );
            }

            if !is_active {
                // ── Delete button (only on inactive rows — can't delete the running profile) ──
                let delete_hover_id = format!("profile_delete:{}", id);
                let is_delete_hovered = state.hovered_item_id.as_deref() == Some(delete_hover_id.as_str());
                let delete_bg = if is_delete_hovered { "rgba(229,57,53,0.2)" } else { &toolbar_theme.item_bg_hover };
                ctx.set_fill_color(delete_bg);
                ctx.fill_rounded_rect(delete_btn_x, btn_y, small_btn_w, btn_h, 3.0);
                ctx.set_stroke_color(if is_delete_hovered { "rgba(229,57,53,0.5)" } else { &toolbar_theme.separator });
                ctx.set_stroke_width(1.0);
                ctx.stroke_rounded_rect(delete_btn_x, btn_y, small_btn_w, btn_h, 3.0);
                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(if is_delete_hovered { "rgba(239,154,154,0.95)" } else { "rgba(254,255,238,0.5)" });
                ctx.set_text_align(TextAlign::Center);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(TextKey::Delete.get(current_language()), delete_btn_x + small_btn_w / 2.0, btn_y + btn_h / 2.0);
                ctx.set_text_align(TextAlign::Left);

                let delete_hit_id = format!("user_settings:profile_delete:{}", id);
                result.content_items.push((format!("profile_delete:{}", id), WidgetRect::new(delete_btn_x, btn_y, small_btn_w, btn_h)));
                input_coordinator.register_on_layer(
                    delete_hit_id.as_str(),
                    uzor::types::Rect::new(delete_btn_x, btn_y, small_btn_w, btn_h),
                    Sense::CLICK | Sense::HOVER,
                    layer_id,
                );

                // Whole-row click area for switching (excludes button area on the right)
                let row_click_w = rename_btn_x - x - btn_gap;
                let switch_hit_id = format!("user_settings:profile_switch:{}", id);
                result.content_items.push((format!("profile_switch:{}", id), WidgetRect::new(x, cy, row_click_w, profile_row_h)));
                input_coordinator.register_on_layer(
                    switch_hit_id.as_str(),
                    uzor::types::Rect::new(x, cy, row_click_w, profile_row_h),
                    Sense::CLICK | Sense::HOVER,
                    layer_id,
                );
            }
        }

        // ── Avatar picker popover (inline, below current row) ──
        if is_avatar_open {
            let avatars = [
                "chart", "rocket", "shield", "fire",
                "star",  "moon",   "sun",    "ghost",
            ];
            let cell_size = 28.0;
            let picker_cols = 8usize;
            let picker_w = cell_size * picker_cols as f64;
            let picker_h = cell_size + 8.0;
            let picker_x = x;
            let picker_y = cy + profile_row_h;

            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rounded_rect(picker_x, picker_y, picker_w, picker_h, 4.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(picker_x, picker_y, picker_w, picker_h, 4.0);

            // Determine current avatar for the target profile
            let current_avatar = state.profile_avatar_target_id.as_deref()
                .and_then(|tid| state.available_profiles.iter().find(|(pid, _, _)| pid == tid))
                .map(|(_, _, av)| av.as_str())
                .unwrap_or(state.profile_avatar.as_str());

            for (i, av) in avatars.iter().enumerate() {
                let cell_x = picker_x + i as f64 * cell_size;
                let cell_cx = cell_x + cell_size / 2.0;
                let cell_cy = picker_y + picker_h / 2.0;
                let is_selected = *av == current_avatar;

                if is_selected {
                    ctx.set_fill_color(&toolbar_theme.item_bg_active);
                    ctx.fill_rounded_rect(cell_x + 2.0, picker_y + 2.0, cell_size - 4.0, picker_h - 4.0, 3.0);
                }

                draw_avatar_dot(ctx, cell_cx, cell_cy, 7.0, avatar_color(av));

                let hit_id = format!("user_settings:profile_avatar:{}", av);
                result.content_items.push((format!("profile_avatar:{}", av), WidgetRect::new(cell_x, picker_y, cell_size, picker_h)));
                input_coordinator.register_on_layer(
                    hit_id.as_str(),
                    uzor::types::Rect::new(cell_x, picker_y, cell_size, picker_h),
                    Sense::CLICK | Sense::HOVER,
                    layer_id,
                );
            }
            cy += picker_h + 4.0;
        }

        cy += profile_row_h;
    }

    // ── Separator ─────────────────────────────────────────────────────────────
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(x, cy);
    ctx.line_to(x + available_w, cy);
    ctx.stroke();
    cy += 8.0;

    // ── "New Profile" dialog / button ─────────────────────────────────────────
    if state.show_new_profile_dialog {
        let input_h = 24.0;
        let input_w = available_w - 60.0 - 6.0;

        let new_name_input_rect = WidgetRect::new(x, cy, input_w, input_h);
        let widget_theme_new = toolbar_to_widget_theme(toolbar_theme, frame_theme);

        let (new_sel_start, new_sel_end) = if let Some((lo, hi)) = state.new_profile_name_editing.selection_range() {
            (Some(lo), Some(hi))
        } else {
            (None, None)
        };
        let new_name_input_config = InputConfig::new(&state.new_profile_name_editing.text)
            .with_focused(state.new_profile_name_focused)
            .with_cursor(state.new_profile_name_editing.cursor)
            .with_placeholder("Profile name...")
            .with_selection(new_sel_start, new_sel_end);

        let new_name_input_result = draw_input(ctx, &new_name_input_config, WidgetState::Normal, new_name_input_rect, &widget_theme_new);

        // Register click target so mouse clicks focus this input
        input_coordinator.register_on_layer(
            "user_settings:new_profile_name_input",
            uzor::types::Rect::new(x, cy, input_w, input_h),
            Sense::CLICK,
            layer_id,
        );

        // Draw blinking cursor when focused
        if state.new_profile_name_focused && state.new_profile_name_editing.is_cursor_visible(current_time_ms) {
            draw_input_cursor(
                ctx,
                new_name_input_result.cursor_x,
                new_name_input_result.cursor_y,
                new_name_input_result.cursor_height,
                text_color,
            );
        }

        // Cancel (X) button — on same row as name input
        let cancel_x = x + input_w + 6.0;
        if cancel_x + 24.0 <= x + available_w {
            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rounded_rect(cancel_x, cy, 24.0, input_h, 3.0);
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color("rgba(254,255,238,0.5)");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("✕", cancel_x + 12.0, cy + input_h / 2.0);
            ctx.set_text_align(TextAlign::Left);

            result.content_items.push(("profile_new_cancel".to_string(), WidgetRect::new(cancel_x, cy, 24.0, input_h)));
            input_coordinator.register_on_layer(
                "user_settings:profile_new_cancel",
                uzor::types::Rect::new(cancel_x, cy, 24.0, input_h),
                Sense::CLICK | Sense::HOVER,
                layer_id,
            );
        }

        cy += input_h + 4.0;

        // Create button — full row
        ctx.set_fill_color("rgba(76,175,80,0.2)");
        ctx.fill_rounded_rect(x, cy, available_w, input_h, 3.0);
        ctx.set_stroke_color("rgba(76,175,80,0.6)");
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(x, cy, available_w, input_h, 3.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color("#81c784");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(UserSettingsKey::BtnCreate.get(current_language()), x + available_w / 2.0, cy + input_h / 2.0);
        ctx.set_text_align(TextAlign::Left);

        result.content_items.push(("profile_new_confirm".to_string(), WidgetRect::new(x, cy, available_w, input_h)));
        input_coordinator.register_on_layer(
            "user_settings:profile_new_confirm",
            uzor::types::Rect::new(x, cy, available_w, input_h),
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );

        cy += input_h + 6.0;
    } else {
        // "+ New Profile" button
        let new_btn_h = 24.0;
        let new_btn_w = available_w.min(140.0);
        let is_new_profile_hovered = state.hovered_item_id.as_deref() == Some("profile_new");
        let new_profile_bg = if is_new_profile_hovered { "rgba(255,255,255,0.12)" } else { &toolbar_theme.item_bg_hover };
        ctx.set_fill_color(new_profile_bg);
        ctx.fill_rounded_rect(x, cy, new_btn_w, new_btn_h, 3.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(x, cy, new_btn_w, new_btn_h, 3.0);
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(if is_new_profile_hovered { "rgba(254,255,238,0.95)" } else { "rgba(254,255,238,0.7)" });
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(UserSettingsKey::BtnNewProfile.get(current_language()), x + new_btn_w / 2.0, cy + new_btn_h / 2.0);
        ctx.set_text_align(TextAlign::Left);

        result.content_items.push(("profile_new".to_string(), WidgetRect::new(x, cy, new_btn_w, new_btn_h)));
        input_coordinator.register_on_layer(
            "user_settings:profile_new",
            uzor::types::Rect::new(x, cy, new_btn_w, new_btn_h),
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );

        cy += new_btn_h;
    }

    // Suppress unused variable warning for avatar_tag helper (used for future text-only rendering).
    let _ = avatar_tag;

    cy
}

// =============================================================================
// Performance tab renderer
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_performance_tab(
    ctx: &mut dyn RenderContext,
    viewport_rect: WidgetRect,
    state: &UserSettingsState,
    toolbar_theme: &ToolbarTheme,
    scroll_widget_theme: &WidgetTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    let container = ScrollableContainer::new(
        viewport_rect,
        &state.performance_tab_scroll,
        None,
    );
    container.begin(ctx);
    let x = viewport_rect.x;
    let available_w = container.content_width();
    let mut cy = container.content_y();

    // Build WidgetTheme from toolbar colours
    let widget_theme = WidgetTheme {
        bg_normal:     toolbar_theme.item_bg_hover.clone(),
        bg_hover:      toolbar_theme.item_bg_hover.clone(),
        bg_pressed:    toolbar_theme.item_bg_active.clone(),
        bg_disabled:   toolbar_theme.item_bg_hover.clone(),
        text_normal:   toolbar_theme.item_text.clone(),
        text_hover:    toolbar_theme.item_text_active.clone(),
        text_disabled: toolbar_theme.item_text_muted.clone(),
        border_normal: toolbar_theme.separator.clone(),
        border_hover:  toolbar_theme.separator.clone(),
        border_focused: toolbar_theme.item_bg_active.clone(),
        accent:        toolbar_theme.accent.clone(),
        accent_hover:  toolbar_theme.accent.clone(),
        success:       "#26a69a".to_string(),
        warning:       "#ff9800".to_string(),
        danger:        "#ef5350".to_string(),
    };

    // Section label
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::SectionIndicatorRecalc.get(current_language()), x, cy);
    cy += 22.0;

    let options = [
        RadioOption {
            key: "PerFrame",
            label: "Per Frame",
            description: "Recalculate once per rendered frame. Best balance of accuracy and performance.",
        },
        RadioOption {
            key: "PerTick",
            label: "Per Tick",
            description: "Recalculate on every incoming trade. Most accurate, higher CPU. Best for agents.",
        },
        RadioOption {
            key: "PerBar",
            label: "Per Bar",
            description: "Recalculate only when a new bar closes. Minimal CPU, suitable for long timeframes.",
        },
    ];

    let selected_index = match state.recalc_mode_label.as_str() {
        "Per Tick" => 1,
        "Per Bar"  => 2,
        _          => 0, // Per Frame default
    };

    let radio_result = draw_radio_group(
        ctx,
        &options,
        selected_index,
        state.hovered_item_id.as_deref(),
        x,
        cy,
        available_w,
        &widget_theme,
    );

    // Register a hit zone for each radio option row
    for (i, (rx, ry, rw, rh)) in radio_result.option_rects.iter().enumerate() {
        let hit_id = format!("user_settings:recalc_mode:{}", options[i].key);
        result.content_items.push((
            format!("recalc_mode:{}", options[i].key),
            WidgetRect::new(*rx, *ry, *rw, *rh),
        ));
        input_coordinator.register_on_layer(
            hit_id.as_str(),
            uzor::types::Rect::new(*rx, *ry, *rw, *rh),
            uzor::input::Sense::CLICK | uzor::input::Sense::HOVER,
            layer_id,
        );
    }

    // Advance cy past the radio group
    cy += options.len() as f64 * (52.0 + 8.0) - 8.0 + 24.0;

    // ── Diagnostics toggle ────────────────────────────────────────────────────
    // Section label
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::SectionDiagnostics.get(current_language()), x, cy);

    let row_h = 24.0;
    let cb_y_offset = cy + 18.0;

    // Checkbox (16 × 16)
    let cb_size = 16.0;
    let cb_x = x;
    let cb_y = cb_y_offset + (row_h - cb_size) / 2.0;

    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(cb_x, cb_y, cb_size, cb_size, 2.0);

    if state.diagnostics_enabled {
        ctx.set_fill_color(&toolbar_theme.item_text_active);
        ctx.fill_rounded_rect(cb_x + 3.0, cb_y + 3.0, cb_size - 6.0, cb_size - 6.0, 1.0);
    }

    // Label + description
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(
        UserSettingsKey::EnableDiagnosticLogging.get(current_language()),
        cb_x + cb_size + 10.0,
        cb_y_offset + row_h / 2.0,
    );

    let desc_y = cb_y_offset + row_h + 2.0;
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(
        "Logs trade and recalculation counts every 5 seconds.",
        cb_x + cb_size + 10.0,
        desc_y,
    );

    // Hit zone covers the label row
    let toggle_rect = WidgetRect::new(cb_x, cb_y_offset, available_w, row_h);
    result.content_items.push(("diagnostics_toggle".to_string(), toggle_rect));
    input_coordinator.register_on_layer(
        "user_settings:diagnostics_toggle",
        uzor::types::Rect::new(cb_x, cb_y_offset, available_w, row_h),
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );

    cy = desc_y + 15.0;

    // ── DATA & CACHE ──────────────────────────────────────────────────────────
    cy += 24.0;
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::SectionDataCache.get(current_language()), x, cy);
    cy += 22.0;

    let content_w = available_w;
    let slider_h = 28.0;
    let desc_gap = 14.0;

    // Helper: display value is the committed value, unless a drag preview is active.
    let floating = state.data_slider_floating();

    // ── Slider 1: Background bars (300–10000) ──────────────────────────────
    let bg_bars_val = if let Some(("data_bg_bars", v)) = floating { v } else { state.data_bg_bars as f64 };
    let bg_bars_config = SliderConfig::new(300.0, 10000.0).with_step(100.0);
    let bg_bars_rect = WidgetRect::new(x, cy, content_w, slider_h);
    let bg_bars_result = render_single_slider(
        ctx,
        &bg_bars_config,
        bg_bars_val,
        bg_bars_rect,
        UserSettingsKey::SliderBgBars.get(current_language()),
        &widget_theme,
        false,
        None,
    );
    if let Some(track_info) = bg_bars_result.track_info {
        result.slider_tracks.push(SliderTrackInfo::new(
            "data_bg_bars",
            track_info.track_x,
            track_info.track_width,
            track_info.min_val,
            track_info.max_val,
        ));
        result.content_items.push(("data_bg_bars".to_string(), bg_bars_rect));
        input_coordinator.register_on_layer(
            "user_settings:data_bg_bars",
            uzor::types::Rect::new(x, cy, content_w, slider_h),
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
    }
    cy += slider_h;
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::DescBgBars.get(current_language()), x + 4.0, cy);
    cy += desc_gap;

    // ── Slider 2: Max bars in memory (0–50000) ────────────────────────────
    cy += 8.0;
    let max_bars_val = if let Some(("data_max_bars", v)) = floating { v } else { state.data_max_bars as f64 };
    let max_bars_config = SliderConfig::new(0.0, 50000.0).with_step(500.0);
    let max_bars_rect = WidgetRect::new(x, cy, content_w, slider_h);
    let max_bars_result = render_single_slider(
        ctx,
        &max_bars_config,
        max_bars_val,
        max_bars_rect,
        UserSettingsKey::SliderMaxBars.get(current_language()),
        &widget_theme,
        false,
        None,
    );
    if let Some(track_info) = max_bars_result.track_info {
        result.slider_tracks.push(SliderTrackInfo::new(
            "data_max_bars",
            track_info.track_x,
            track_info.track_width,
            track_info.min_val,
            track_info.max_val,
        ));
        result.content_items.push(("data_max_bars".to_string(), max_bars_rect));
        input_coordinator.register_on_layer(
            "user_settings:data_max_bars",
            uzor::types::Rect::new(x, cy, content_w, slider_h),
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
    }
    cy += slider_h;
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::DescMaxBars.get(current_language()), x + 4.0, cy);
    cy += desc_gap;

    // ── Slider 3: Cache size limit MB (50–5000) ───────────────────────────
    cy += 8.0;
    let store_mb_val = if let Some(("data_store_size_mb", v)) = floating { v } else { state.data_store_size_mb as f64 };
    let store_mb_config = SliderConfig::new(50.0, 5000.0).with_step(50.0);
    let store_mb_rect = WidgetRect::new(x, cy, content_w, slider_h);
    let store_mb_result = render_single_slider(
        ctx,
        &store_mb_config,
        store_mb_val,
        store_mb_rect,
        UserSettingsKey::SliderCacheSizeMb.get(current_language()),
        &widget_theme,
        false,
        None,
    );
    if let Some(track_info) = store_mb_result.track_info {
        result.slider_tracks.push(SliderTrackInfo::new(
            "data_store_size_mb",
            track_info.track_x,
            track_info.track_width,
            track_info.min_val,
            track_info.max_val,
        ));
        result.content_items.push(("data_store_size_mb".to_string(), store_mb_rect));
        input_coordinator.register_on_layer(
            "user_settings:data_store_size_mb",
            uzor::types::Rect::new(x, cy, content_w, slider_h),
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
    }
    cy += slider_h;
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::DescCacheSize.get(current_language()), x + 4.0, cy);
    cy += desc_gap;

    // ── Slider 4: Auto-cleanup days (1–365) ───────────────────────────────
    cy += 8.0;
    let cleanup_days_val = if let Some(("data_cleanup_days", v)) = floating { v } else { state.data_cleanup_days as f64 };
    let cleanup_days_config = SliderConfig::new(1.0, 365.0).with_step(1.0);
    let cleanup_days_rect = WidgetRect::new(x, cy, content_w, slider_h);
    let cleanup_days_result = render_single_slider(
        ctx,
        &cleanup_days_config,
        cleanup_days_val,
        cleanup_days_rect,
        UserSettingsKey::SliderAutoCleanupDays.get(current_language()),
        &widget_theme,
        false,
        None,
    );
    if let Some(track_info) = cleanup_days_result.track_info {
        result.slider_tracks.push(SliderTrackInfo::new(
            "data_cleanup_days",
            track_info.track_x,
            track_info.track_width,
            track_info.min_val,
            track_info.max_val,
        ));
        result.content_items.push(("data_cleanup_days".to_string(), cleanup_days_rect));
        input_coordinator.register_on_layer(
            "user_settings:data_cleanup_days",
            uzor::types::Rect::new(x, cy, content_w, slider_h),
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
    }
    cy += slider_h;
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::DescAutoCleanup.get(current_language()), x + 4.0, cy);
    cy += desc_gap;

    cy += 8.0;

    let total_content_h = cy - container.content_y();
    let scroll_result = container.end(ctx, total_content_h, scroll_widget_theme);
    result.scroll_viewport_rect = Some(viewport_rect);
    result.scroll_content_height = scroll_result.content_height;
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
    result.scroll_viewport_height = scroll_result.viewport_height;

    if let Some(ref hr) = result.scrollbar_handle_rect {
        let inflated = uzor::types::Rect::new(hr.x - 5.0, hr.y, hr.width + 10.0, hr.height);
        input_coordinator.register_on_layer(
            "user_settings:scrollbar_handle",
            inflated,
            uzor::input::Sense::DRAG,
            layer_id,
        );
    }
    if let Some(ref tr) = result.scrollbar_track_rect {
        input_coordinator.register_on_layer(
            "user_settings:scrollbar_track",
            uzor::types::Rect::new(tr.x, tr.y, tr.width, tr.height),
            uzor::input::Sense::CLICK,
            layer_id,
        );
    }
    if let Some(ref vp) = result.scroll_viewport_rect {
        input_coordinator.register_on_layer(
            "user_settings:scroll_viewport",
            uzor::types::Rect::new(vp.x, vp.y, vp.width, vp.height),
            uzor::input::Sense::SCROLL,
            layer_id,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_server_tab(
    ctx: &mut dyn RenderContext,
    viewport_rect: WidgetRect,
    state: &UserSettingsState,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    scroll_widget_theme: &WidgetTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    // Server tab content is short (status row + endpoint info), no scroll needed.
    let no_scroll = ScrollState::default();
    let container = ScrollableContainer::new(
        viewport_rect,
        &no_scroll,
        None,
    );
    container.begin(ctx);
    let x = viewport_rect.x;
    let available_w = container.content_width();
    let mut cy = container.content_y();

    let row_h = 24.0;
    let section_gap = 18.0;

    // ── Section: SERVER ───────────────────────────────────────────────────────
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::SectionServer.get(current_language()), x, cy);

    // Enable toggle checkbox row
    cy += section_gap;
    let cb_size = 16.0;
    let cb_x = x;
    let cb_y = cy + (row_h - cb_size) / 2.0;

    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(cb_x, cb_y, cb_size, cb_size, 2.0);

    if state.server_enabled {
        ctx.set_fill_color(&toolbar_theme.item_text_active);
        ctx.fill_rounded_rect(cb_x + 3.0, cb_y + 3.0, cb_size - 6.0, cb_size - 6.0, 1.0);
    }

    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(
        UserSettingsKey::EnableAgentApiServer.get(current_language()),
        cb_x + cb_size + 10.0,
        cy + row_h / 2.0,
    );

    let toggle_rect = WidgetRect::new(cb_x, cy, available_w, row_h);
    result.content_items.push(("server_toggle".to_string(), toggle_rect));
    input_coordinator.register_on_layer(
        "user_settings:server_toggle",
        uzor::types::Rect::new(cb_x, cy, available_w, row_h),
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );
    cy += row_h + 8.0;

    // Status indicator row
    let dot_r = 4.0;
    let dot_cx = x + dot_r;
    let dot_cy = cy + row_h / 2.0;

    let is_running = state.server_enabled && state.server_status == "running";
    let dot_color = if is_running { "#26a69a" } else { "#ef5350" };
    ctx.set_fill_color(dot_color);
    ctx.begin_path();
    ctx.arc(dot_cx, dot_cy, dot_r, 0.0, std::f64::consts::TAU);
    ctx.fill();

    let status_text = if is_running {
        format!("Running on :{}", state.server_port)
    } else {
        UserSettingsKey::ServerStopped.get(current_language()).to_string()
    };
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&status_text, dot_cx + dot_r + 6.0, dot_cy);
    cy += row_h + 16.0;

    // ── Access info (replaces the old API KEYS section) ──────────────────────
    // The OSS build serves the Agent API openly on 127.0.0.1, so there are no
    // user-managed keys. Tell the user where the API is reachable instead.
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(
        UserSettingsKey::ServerOpenAccess.get(current_language()),
        x,
        cy,
    );
    cy += 18.0;
    ctx.fill_text(
        &format!("Endpoint: http://127.0.0.1:{}/", state.server_port),
        x,
        cy,
    );
    let bottom = cy + 18.0;
    // Silence the now-unused parameter warning.
    let _ = frame_theme;
    let _ = available_w;

    let total_content_h = bottom - container.content_y();
    let scroll_result = container.end(ctx, total_content_h, scroll_widget_theme);
    result.scroll_viewport_rect = Some(viewport_rect);
    result.scroll_content_height = scroll_result.content_height;
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
    result.scroll_viewport_height = scroll_result.viewport_height;

    if let Some(ref hr) = result.scrollbar_handle_rect {
        let inflated = uzor::types::Rect::new(hr.x - 5.0, hr.y, hr.width + 10.0, hr.height);
        input_coordinator.register_on_layer(
            "user_settings:scrollbar_handle",
            inflated,
            uzor::input::Sense::DRAG,
            layer_id,
        );
    }
    if let Some(ref tr) = result.scrollbar_track_rect {
        input_coordinator.register_on_layer(
            "user_settings:scrollbar_track",
            uzor::types::Rect::new(tr.x, tr.y, tr.width, tr.height),
            uzor::input::Sense::CLICK,
            layer_id,
        );
    }
    if let Some(ref vp) = result.scroll_viewport_rect {
        input_coordinator.register_on_layer(
            "user_settings:scroll_viewport",
            uzor::types::Rect::new(vp.x, vp.y, vp.width, vp.height),
            uzor::input::Sense::SCROLL,
            layer_id,
        );
    }
}


// =============================================================================
// Mode-transition confirmation dialogs
// =============================================================================

/// Render the inline confirmation panel shown when the user clicks "Standalone"
/// from Connected mode (disconnect_pending = true).
///
/// Returns the new `cy` value after all rendered content.
#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
fn render_disconnect_dialog(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    available_w: f64,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) -> f64 {
    let mut cy = y;

    // Dialog box background
    ctx.set_fill_color("rgba(239,83,80,0.06)");
    ctx.fill_rounded_rect(x - 6.0, cy - 6.0, available_w + 12.0, 130.0, 6.0);
    ctx.set_stroke_color("rgba(239,83,80,0.25)");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x - 6.0, cy - 6.0, available_w + 12.0, 130.0, 6.0);

    // Title
    ctx.set_font("600 12px sans-serif");
    ctx.set_fill_color("rgba(239,83,80,0.9)");
    ctx.set_text_align(uzor::render::TextAlign::Left);
    ctx.set_text_baseline(uzor::render::TextBaseline::Top);
    ctx.fill_text("SWITCH TO OFFLINE MODE?", x, cy);
    cy += 22.0;

    // Body text
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.55)");
    ctx.fill_text("Your data stays on this machine.", x, cy);
    cy += 15.0;
    ctx.fill_text("Cloud sync will stop. You can reconnect anytime.", x, cy);
    cy += 22.0;

    let btn_h = 28.0;
    let half_w = (available_w - 8.0) / 2.0;

    // ── Button: Confirm Disconnect ────────────────────────────────────────────
    ctx.set_fill_color("rgba(239,83,80,0.15)");
    ctx.fill_rounded_rect(x, cy, half_w, btn_h, 4.0);
    ctx.set_stroke_color("rgba(239,83,80,0.5)");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, cy, half_w, btn_h, 4.0);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color("#ef5350");
    ctx.set_text_align(uzor::render::TextAlign::Center);
    ctx.set_text_baseline(uzor::render::TextBaseline::Middle);
    ctx.fill_text("Disconnect", x + half_w / 2.0, cy + btn_h / 2.0);
    ctx.set_text_align(uzor::render::TextAlign::Left);
    result.content_items.push(("disconnect_confirm".to_string(), WidgetRect::new(x, cy, half_w, btn_h)));
    input_coordinator.register_on_layer(
        "user_settings:disconnect_confirm",
        uzor::types::Rect::new(x, cy, half_w, btn_h),
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );

    // ── Button: Cancel ────────────────────────────────────────────────────────
    let cancel_x = x + half_w + 8.0;
    ctx.set_fill_color(&toolbar_theme.item_bg_hover);
    ctx.fill_rounded_rect(cancel_x, cy, half_w, btn_h, 4.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(cancel_x, cy, half_w, btn_h, 4.0);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(uzor::render::TextAlign::Center);
    ctx.set_text_baseline(uzor::render::TextBaseline::Middle);
    ctx.fill_text("Cancel", cancel_x + half_w / 2.0, cy + btn_h / 2.0);
    ctx.set_text_align(uzor::render::TextAlign::Left);
    result.content_items.push(("disconnect_cancel".to_string(), WidgetRect::new(cancel_x, cy, half_w, btn_h)));
    input_coordinator.register_on_layer(
        "user_settings:disconnect_cancel",
        uzor::types::Rect::new(cancel_x, cy, half_w, btn_h),
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );
    cy += btn_h;

    cy
}
