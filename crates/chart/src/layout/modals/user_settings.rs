//! User settings modal renderer — tabbed settings dialog.
//!
//! Tabs:
//!   - General    : placeholder for future general settings
//!   - Performance: RecalcMode radio group selector

use crate::engine::render::RenderContext;
use crate::engine::render::draw_svg_icon;
use uzor::render::{TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;
use uzor::input::sense::Sense;
use crate::ui::modal_settings::{UserSettingsState, UserSettingsTab};
use crate::ui::toolbar_render::ToolbarTheme;
use crate::ui::widgets::{render_modal_frame_only, ModalTheme, WidgetTheme, RadioOption, draw_radio_group};
use crate::layout::render_chart::FrameTheme;
use crate::layout::render_frame::UserSettingsResult;
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
    _current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> UserSettingsResult {
    let mut result = UserSettingsResult::default();

    let modal_w = 540.0;
    let modal_h = 400.0;
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
        uzor::input::sense::Sense::CLICK,
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
        uzor::input::sense::Sense::DRAG,
        &layer_id,
    );

    let text_color = &toolbar_theme.item_text;
    ctx.set_font("14px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("User Settings", modal_x + padding, modal_y + header_h / 2.0);

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
        uzor::input::sense::Sense::CLICK,
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
            uzor::input::sense::Sense::CLICK,
            &layer_id,
        );
    }

    // =========================================================================
    // Content area
    // =========================================================================
    let content_x = modal_x + sidebar_w;
    let content_y = modal_y + header_h;
    let content_w = modal_w - sidebar_w;

    // Content background
    ctx.set_fill_color(&frame_theme.toolbar_bg);
    ctx.fill_rect(content_x, content_y, content_w, content_h);

    // Tab title
    ctx.set_font("bold 14px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(state.active_tab.label(), content_x + padding, content_y + padding);

    let settings_y = content_y + padding + 30.0;

    match state.active_tab {
        UserSettingsTab::General => {
            render_general_tab(
                ctx,
                content_x + padding,
                settings_y,
                content_w - padding * 2.0,
                state,
                text_color,
                toolbar_theme,
                input_coordinator,
                &layer_id,
                &mut result,
            );
        }
        UserSettingsTab::Sync => {
            render_sync_tab(
                ctx,
                content_x + padding,
                settings_y,
                content_w - padding * 2.0,
                state,
                text_color,
                toolbar_theme,
                input_coordinator,
                &layer_id,
                &mut result,
            );
        }
        UserSettingsTab::Performance => {
            render_performance_tab(
                ctx,
                content_x + padding,
                settings_y,
                content_w - padding * 2.0,
                state,
                text_color,
                toolbar_theme,
                frame_theme,
                input_coordinator,
                &layer_id,
                &mut result,
            );
        }
        UserSettingsTab::Server => {
            render_server_tab(
                ctx,
                content_x + padding,
                settings_y,
                content_w - padding * 2.0,
                state,
                text_color,
                toolbar_theme,
                frame_theme,
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
    x: f64,
    y: f64,
    available_w: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    let mut cy = y;

    // ── Section: CONNECTION MODE ──────────────────────────────────────────────
    ctx.set_font("600 11px sans-serif");
    ctx.set_fill_color("rgba(244,205,99,0.7)");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("CONNECTION MODE", x, cy);
    cy += 20.0;

    if state.sync_transition_pending {
        // ── Confirmation: Standalone → Connected ─────────────────────────────
        cy = render_sync_connect_dialog(
            ctx, x, cy, available_w, toolbar_theme,
            input_coordinator, layer_id, result,
        );
        cy += 16.0;
    } else if state.disconnect_pending {
        // ── Confirmation: Connected → Standalone ─────────────────────────────
        cy = render_disconnect_dialog(
            ctx, x, cy, available_w, toolbar_theme,
            input_coordinator, layer_id, result,
        );
        cy += 16.0;
    } else {
        // ── Normal radio buttons ──────────────────────────────────────────────
        let mode_row_h = 44.0;
        let mode_row_gap = 6.0;
        let dot_size = 14.0;
        let dot_r = dot_size / 2.0;

        // Option: Connected
        let connected_y = cy;
        if state.client_mode_connected {
            ctx.set_fill_color(&toolbar_theme.item_bg_active);
            ctx.fill_rounded_rect(x - 6.0, connected_y - 4.0, available_w + 12.0, mode_row_h, 4.0);
        }

        let dot_cx = x + dot_r;
        let dot_cy = connected_y + 10.0 + dot_r;
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.5);
        ctx.begin_path();
        ctx.arc(dot_cx, dot_cy, dot_r, 0.0, std::f64::consts::TAU);
        ctx.stroke();
        if state.client_mode_connected {
            ctx.set_fill_color(&toolbar_theme.accent);
            ctx.begin_path();
            ctx.arc(dot_cx, dot_cy, dot_r - 4.0, 0.0, std::f64::consts::TAU);
            ctx.fill();
        }

        let text_x = x + dot_size + 10.0;
        ctx.set_fill_color(toolbar_theme.item_text.as_str());
        ctx.set_font("13px sans-serif");
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text("Connected to chart.example", text_x, connected_y + 6.0);
        ctx.set_fill_color("rgba(254,255,238,0.45)");
        ctx.set_font("11px sans-serif");
        ctx.fill_text("OTA updates, cloud sync, centralized API keys", text_x, connected_y + 24.0);

        let connected_rect = WidgetRect::new(x, connected_y, available_w, mode_row_h);
        result.content_items.push(("mode_connected".to_string(), connected_rect));
        input_coordinator.register_on_layer(
            "user_settings:mode_connected",
            uzor::types::Rect::new(x, connected_y, available_w, mode_row_h),
            Sense::CLICK,
            layer_id,
        );
        cy += mode_row_h + mode_row_gap;

        // Option: Standalone
        let standalone_y = cy;
        if !state.client_mode_connected {
            ctx.set_fill_color(&toolbar_theme.item_bg_active);
            ctx.fill_rounded_rect(x - 6.0, standalone_y - 4.0, available_w + 12.0, mode_row_h, 4.0);
        }

        let dot_cy2 = standalone_y + 10.0 + dot_r;
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.5);
        ctx.begin_path();
        ctx.arc(dot_cx, dot_cy2, dot_r, 0.0, std::f64::consts::TAU);
        ctx.stroke();
        if !state.client_mode_connected {
            ctx.set_fill_color(&toolbar_theme.accent);
            ctx.begin_path();
            ctx.arc(dot_cx, dot_cy2, dot_r - 4.0, 0.0, std::f64::consts::TAU);
            ctx.fill();
        }

        ctx.set_fill_color(toolbar_theme.item_text.as_str());
        ctx.set_font("13px sans-serif");
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text("Standalone (offline)", text_x, standalone_y + 6.0);
        ctx.set_fill_color("rgba(254,255,238,0.45)");
        ctx.set_font("11px sans-serif");
        ctx.fill_text("No server communication, all data stays local", text_x, standalone_y + 24.0);

        let standalone_rect = WidgetRect::new(x, standalone_y, available_w, mode_row_h);
        result.content_items.push(("mode_standalone".to_string(), standalone_rect));
        input_coordinator.register_on_layer(
            "user_settings:mode_standalone",
            uzor::types::Rect::new(x, standalone_y, available_w, mode_row_h),
            Sense::CLICK,
            layer_id,
        );
        cy += mode_row_h + 16.0;
    }

    // ── Section: ACCOUNT ─────────────────────────────────────────────────────
    ctx.set_font("600 11px sans-serif");
    ctx.set_fill_color("rgba(244,205,99,0.7)");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("ACCOUNT", x, cy);
    cy += 20.0;

    if state.is_logged_in {
        // ── Logged in state ───────────────────────────────────────────────────
        // Display name — slightly muted when in Standalone mode (info only, no action).
        let name_alpha = if state.client_mode_connected { "1.0" } else { "0.5" };
        let name_color = format!("rgba(254,255,238,{})", name_alpha);
        ctx.set_font("700 18px sans-serif");
        ctx.set_fill_color(name_color.as_str());
        ctx.fill_text(&state.auth_display_name, x, cy);
        cy += 26.0;

        // "Signed in via {provider}"
        let provider_text = if state.auth_provider.is_empty() {
            "Signed in".to_string()
        } else {
            format!("Signed in via {}", state.auth_provider)
        };
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color("rgba(254,255,238,0.5)");
        ctx.fill_text(&provider_text, x, cy);
        cy += 30.0;

        // Only show interactive buttons when in Connected mode.
        if state.client_mode_connected {
            // "Open Dashboard" button
            let btn_h = 28.0;
            let btn_w = available_w.min(180.0);
            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rounded_rect(x, cy, btn_w, btn_h, 4.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(x, cy, btn_w, btn_h, 4.0);
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(text_color);
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("Open Dashboard", x + btn_w / 2.0, cy + btn_h / 2.0);
            ctx.set_text_align(TextAlign::Left);

            result.content_items.push(("sign_in".to_string(), WidgetRect::new(x, cy, btn_w, btn_h)));
            input_coordinator.register_on_layer(
                "user_settings:open_dashboard",
                uzor::types::Rect::new(x, cy, btn_w, btn_h),
                Sense::CLICK,
                layer_id,
            );
            cy += btn_h + 8.0;

            // "Sign Out" button
            ctx.set_fill_color("rgba(239,83,80,0.15)");
            ctx.fill_rounded_rect(x, cy, btn_w, btn_h, 4.0);
            ctx.set_stroke_color("rgba(239,83,80,0.5)");
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(x, cy, btn_w, btn_h, 4.0);
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color("#ef5350");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("Sign Out", x + btn_w / 2.0, cy + btn_h / 2.0);
            ctx.set_text_align(TextAlign::Left);

            result.content_items.push(("sign_out".to_string(), WidgetRect::new(x, cy, btn_w, btn_h)));
            input_coordinator.register_on_layer(
                "user_settings:sign_out",
                uzor::types::Rect::new(x, cy, btn_w, btn_h),
                Sense::CLICK,
                layer_id,
            );
            cy += btn_h + 24.0;
        } else {
            // Standalone while logged in — show grayed note
            ctx.set_font("11px sans-serif");
            ctx.set_fill_color("rgba(254,255,238,0.35)");
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.fill_text("Switch to Connected mode to access account actions.", x, cy);
            cy += 20.0;
        }
    } else if !state.client_mode_connected {
        // ── Standalone, not logged in ─────────────────────────────────────────
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color("rgba(254,255,238,0.35)");
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text("Sign in is available in Connected mode.", x, cy);
        cy += 28.0;
    } else {
        // ── Connected, not logged in ──────────────────────────────────────────
        ctx.set_font("13px sans-serif");
        ctx.set_fill_color("rgba(254,255,238,0.5)");
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text("Sign in to sync your settings and link devices.", x, cy);
        cy += 28.0;

        // "Sign In via Browser" button
        let btn_h = 28.0;
        let btn_w = available_w.min(200.0);
        ctx.set_fill_color(&toolbar_theme.item_bg_hover);
        ctx.fill_rounded_rect(x, cy, btn_w, btn_h, 4.0);
        ctx.set_stroke_color(&toolbar_theme.accent);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(x, cy, btn_w, btn_h, 4.0);
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&toolbar_theme.accent);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Sign In via Browser", x + btn_w / 2.0, cy + btn_h / 2.0);
        ctx.set_text_align(TextAlign::Left);

        result.content_items.push(("sign_in".to_string(), WidgetRect::new(x, cy, btn_w, btn_h)));
        input_coordinator.register_on_layer(
            "user_settings:sign_in",
            uzor::types::Rect::new(x, cy, btn_w, btn_h),
            Sense::CLICK,
            layer_id,
        );
        cy += btn_h + 24.0;
    }

    // ── Section: PRIVACY ─────────────────────────────────────────────────────
    ctx.set_font("600 11px sans-serif");
    ctx.set_fill_color("rgba(244,205,99,0.7)");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("PRIVACY", x, cy);
    cy += 20.0;

    // Telemetry toggle row
    {
        let toggle_w = 32.0;
        let toggle_h = 18.0;
        let toggle_x = x + available_w - toggle_w;
        let toggle_y = cy + 1.0;
        let is_on = state.telemetry_enabled;

        // Track
        let track_color = if is_on { &toolbar_theme.accent } else { &toolbar_theme.separator };
        ctx.set_fill_color(track_color);
        ctx.fill_rounded_rect(toggle_x, toggle_y, toggle_w, toggle_h, toggle_h / 2.0);

        // Thumb
        let thumb_r = toggle_h / 2.0 - 2.0;
        let thumb_cx = if is_on {
            toggle_x + toggle_w - thumb_r - 3.0
        } else {
            toggle_x + thumb_r + 3.0
        };
        ctx.set_fill_color("rgba(255,255,255,0.95)");
        ctx.begin_path();
        ctx.arc(thumb_cx, toggle_y + toggle_h / 2.0, thumb_r, 0.0, std::f64::consts::TAU);
        ctx.fill();

        // Label
        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(text_color);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text("Send anonymous usage data", x, cy);
        cy += 20.0;
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color("rgba(254,255,238,0.45)");
        ctx.fill_text("Heartbeat & metrics sent to chart.example (Connected mode only)", x, cy);
        cy += 20.0;

        let row_rect = uzor::types::Rect::new(x, toggle_y - 2.0, available_w, toggle_h + 4.0);
        result.content_items.push(("telemetry_toggle".to_string(), uzor::types::Rect::new(toggle_x, toggle_y, toggle_w, toggle_h)));
        input_coordinator.register_on_layer(
            "user_settings:telemetry_toggle",
            row_rect,
            Sense::CLICK,
            layer_id,
        );
    }
    cy += 8.0;

    // ── Version info (always shown at bottom) ─────────────────────────────────
    ctx.set_font("600 11px sans-serif");
    ctx.set_fill_color("rgba(244,205,99,0.7)");
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("VERSION", x, cy);
    cy += 18.0;

    ctx.set_font("700 18px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.fill_text(&format!("v{}", env!("CARGO_PKG_VERSION")), x, cy);
    let _ = cy; // suppress unused variable warning
}

// =============================================================================
// Helper: render a single toggle row (track + thumb + label)
// Returns the toggle track rect for hit-testing.
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_toggle_row(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    available_w: f64,
    label: &str,
    is_on: bool,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
) -> uzor::types::Rect {
    let toggle_w = 32.0;
    let toggle_h = 18.0;
    let toggle_x = x + available_w - toggle_w;
    let toggle_y = y + 1.0;

    let track_color = if is_on { &toolbar_theme.accent } else { &toolbar_theme.separator };
    ctx.set_fill_color(track_color);
    ctx.fill_rounded_rect(toggle_x, toggle_y, toggle_w, toggle_h, toggle_h / 2.0);

    let thumb_r = toggle_h / 2.0 - 2.0;
    let thumb_cx = if is_on {
        toggle_x + toggle_w - thumb_r - 3.0
    } else {
        toggle_x + thumb_r + 3.0
    };
    ctx.set_fill_color("rgba(255,255,255,0.95)");
    ctx.begin_path();
    ctx.arc(thumb_cx, toggle_y + toggle_h / 2.0, thumb_r, 0.0, std::f64::consts::TAU);
    ctx.fill();

    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(uzor::render::TextAlign::Left);
    ctx.set_text_baseline(uzor::render::TextBaseline::Top);
    ctx.fill_text(label, x, y);

    uzor::types::Rect::new(x, toggle_y - 2.0, available_w, toggle_h + 4.0)
}

// =============================================================================
// Helper: render a checkbox row
// Returns the row rect for hit-testing.
// =============================================================================

fn render_checkbox_row(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    label: &str,
    is_checked: bool,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
) -> uzor::types::Rect {
    let cb_size = 14.0;
    let cb_y = y + 1.0;

    // Box
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.set_fill_color(if is_checked { &toolbar_theme.accent } else { "transparent" });
    ctx.begin_path();
    ctx.move_to(x, cb_y);
    ctx.line_to(x + cb_size, cb_y);
    ctx.line_to(x + cb_size, cb_y + cb_size);
    ctx.line_to(x, cb_y + cb_size);
    ctx.close_path();
    ctx.fill();
    ctx.stroke();

    // Checkmark
    if is_checked {
        ctx.set_stroke_color("rgba(255,255,255,0.95)");
        ctx.set_stroke_width(1.5);
        ctx.begin_path();
        ctx.move_to(x + 2.5, cb_y + cb_size / 2.0);
        ctx.line_to(x + cb_size / 2.0 - 1.0, cb_y + cb_size - 3.0);
        ctx.line_to(x + cb_size - 2.0, cb_y + 2.5);
        ctx.stroke();
    }

    // Label
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(uzor::render::TextAlign::Left);
    ctx.set_text_baseline(uzor::render::TextBaseline::Top);
    ctx.fill_text(label, x + cb_size + 8.0, y);

    uzor::types::Rect::new(x, y, 200.0, 20.0)
}

// =============================================================================
// Sync tab renderer
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_sync_tab(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    available_w: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    let mut cy = y;
    let section_gap = 20.0;
    let row_gap = 26.0;

    // ── Gate: Standalone Mode / Unofficial Build / Attestation Rejected ───────
    let sync_tab_locked = !state.client_mode_connected || state.is_unofficial_build;

    // Effective text color — dimmed when the sync tab is locked
    let effective_text_color: &str = if sync_tab_locked { "#666666" } else { text_color };

    // Banner: Offline / Standalone mode
    if !state.client_mode_connected {
        let banner_h = 34.0;
        ctx.set_fill_color("rgba(244,205,99,0.08)");
        ctx.fill_rounded_rect(x - 4.0, cy - 4.0, available_w + 8.0, banner_h, 4.0);
        ctx.set_stroke_color("rgba(244,205,99,0.25)");
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(x - 4.0, cy - 4.0, available_w + 8.0, banner_h, 4.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color("rgba(244,205,99,0.75)");
        ctx.set_text_align(uzor::render::TextAlign::Left);
        ctx.set_text_baseline(uzor::render::TextBaseline::Top);
        ctx.fill_text("Offline mode — sync disabled. Switch to Connected in General tab.", x, cy + 4.0);
        ctx.fill_text("", x, cy + 4.0); // keep text baseline state clean
        cy += banner_h + 8.0;
    } else if state.is_unofficial_build {
        // Banner: development / unofficial build
        let banner_h = 34.0;
        ctx.set_fill_color("rgba(244,205,99,0.08)");
        ctx.fill_rounded_rect(x - 4.0, cy - 4.0, available_w + 8.0, banner_h, 4.0);
        ctx.set_stroke_color("rgba(244,205,99,0.25)");
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(x - 4.0, cy - 4.0, available_w + 8.0, banner_h, 4.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color("rgba(244,205,99,0.75)");
        ctx.set_text_align(uzor::render::TextAlign::Left);
        ctx.set_text_baseline(uzor::render::TextBaseline::Top);
        ctx.fill_text("Development build — cloud sync disabled.", x, cy + 4.0);
        cy += banner_h + 8.0;
    }

    // Banner: attestation rejected by server
    if state.attestation_rejected {
        let banner_h = 34.0;
        ctx.set_fill_color("rgba(239,83,80,0.08)");
        ctx.fill_rounded_rect(x - 4.0, cy - 4.0, available_w + 8.0, banner_h, 4.0);
        ctx.set_stroke_color("rgba(239,83,80,0.25)");
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(x - 4.0, cy - 4.0, available_w + 8.0, banner_h, 4.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color("rgba(239,83,80,0.85)");
        ctx.set_text_align(uzor::render::TextAlign::Left);
        ctx.set_text_baseline(uzor::render::TextBaseline::Top);
        ctx.fill_text("Server rejected this build. Only official releases can sync.", x, cy + 4.0);
        cy += banner_h + 8.0;
    }

    // ── Section: CLOUD SYNC ───────────────────────────────────────────────────
    ctx.set_font("600 11px sans-serif");
    ctx.set_fill_color("rgba(244,205,99,0.7)");
    ctx.set_text_align(uzor::render::TextAlign::Left);
    ctx.set_text_baseline(uzor::render::TextBaseline::Top);
    ctx.fill_text("CLOUD SYNC", x, cy);
    cy += section_gap;

    // Toggle: Enable Cloud Sync
    {
        let row_rect = render_toggle_row(ctx, x, cy, available_w, "Enable Cloud Sync", state.sync_enabled, effective_text_color, toolbar_theme);
        result.content_items.push(("sync_toggle".to_string(), row_rect));
        if !sync_tab_locked {
            input_coordinator.register_on_layer(
                "user_settings:sync_toggle",
                row_rect,
                uzor::input::sense::Sense::CLICK,
                layer_id,
            );
        }
    }
    cy += row_gap;

    // ── Live Sync Status Row ──────────────────────────────────────────────────
    {
        // Colored status dot (●) + status label
        let dot_char = "\u{25CF}"; // filled circle ●
        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(&state.sync_status_color);
        ctx.set_text_align(uzor::render::TextAlign::Left);
        ctx.set_text_baseline(uzor::render::TextBaseline::Top);
        ctx.fill_text(dot_char, x, cy);

        ctx.set_fill_color(effective_text_color);
        ctx.fill_text(&state.sync_status_label, x + 14.0, cy);
        cy += 16.0;

        // Below: muted "Last synced: X" relative time
        let ts_str = if state.last_sync_timestamp > 0 {
            let secs = state.last_sync_timestamp as u64;
            let mins = secs / 60;
            let hours = mins / 60;
            let days = hours / 24;
            format!("Last synced: {}d {}h ago", days, hours % 24)
        } else {
            "Last synced: Never".to_string()
        };
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color("rgba(254,255,238,0.45)");
        ctx.fill_text(&ts_str, x, cy);
        cy += 18.0;
    }

    // Status: quota usage
    {
        let quota_mb = state.quota_used_bytes as f64 / (1024.0 * 1024.0);
        let quota_str = format!("Storage: {:.1} MB / 50 MB", quota_mb);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color("rgba(254,255,238,0.45)");
        ctx.fill_text(&quota_str, x, cy);
        cy += 24.0;
    }

    // ── "Sync Now" Button ─────────────────────────────────────────────────────
    {
        let btn_w = available_w;
        let btn_h = 28.0;
        let is_syncing = state.sync_is_active;
        let btn_label = if is_syncing { "Syncing\u{2026}" } else { "Sync Now" };
        let btn_disabled = sync_tab_locked || is_syncing;

        let btn_bg = if btn_disabled {
            "rgba(254,255,238,0.05)"
        } else {
            &toolbar_theme.item_bg_hover
        };
        let btn_stroke = if btn_disabled {
            "rgba(254,255,238,0.12)"
        } else {
            &toolbar_theme.separator
        };
        let btn_text = if btn_disabled { "#555555" } else { effective_text_color };

        ctx.set_fill_color(btn_bg);
        ctx.fill_rounded_rect(x, cy, btn_w, btn_h, 4.0);
        ctx.set_stroke_color(btn_stroke);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(x, cy, btn_w, btn_h, 4.0);
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(btn_text);
        ctx.set_text_align(uzor::render::TextAlign::Center);
        ctx.set_text_baseline(uzor::render::TextBaseline::Middle);
        ctx.fill_text(btn_label, x + btn_w / 2.0, cy + btn_h / 2.0);
        ctx.set_text_align(uzor::render::TextAlign::Left);

        let btn_rect = uzor::types::Rect::new(x, cy, btn_w, btn_h);
        result.content_items.push(("force_sync".to_string(), btn_rect));
        if !btn_disabled {
            input_coordinator.register_on_layer(
                "user_settings:force_sync",
                btn_rect,
                uzor::input::sense::Sense::CLICK,
                layer_id,
            );
        }
        cy += btn_h + 8.0;
    }

    // ── NeedsSetup Prompt ─────────────────────────────────────────────────────
    if state.sync_needs_setup {
        let box_h = 110.0;
        ctx.set_fill_color("rgba(244,205,99,0.08)");
        ctx.fill_rounded_rect(x - 4.0, cy - 4.0, available_w + 8.0, box_h, 4.0);
        ctx.set_stroke_color("rgba(244,205,99,0.3)");
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(x - 4.0, cy - 4.0, available_w + 8.0, box_h, 4.0);

        ctx.set_font("11px sans-serif");
        ctx.set_fill_color("rgba(244,205,99,0.85)");
        ctx.set_text_align(uzor::render::TextAlign::Left);
        ctx.set_text_baseline(uzor::render::TextBaseline::Top);
        ctx.fill_text("Cloud data found for this account. Choose how to proceed:", x, cy);
        cy += 18.0;

        let btn_h = 24.0;
        let third_w = (available_w - 8.0) / 2.0;

        // "Upload Local Data" button
        ctx.set_fill_color(&toolbar_theme.item_bg_hover);
        ctx.fill_rounded_rect(x, cy, third_w, btn_h, 4.0);
        ctx.set_stroke_color(&toolbar_theme.accent);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(x, cy, third_w, btn_h, 4.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&toolbar_theme.accent);
        ctx.set_text_align(uzor::render::TextAlign::Center);
        ctx.set_text_baseline(uzor::render::TextBaseline::Middle);
        ctx.fill_text("Upload Local Data", x + third_w / 2.0, cy + btn_h / 2.0);
        ctx.set_text_align(uzor::render::TextAlign::Left);
        let upload_rect = uzor::types::Rect::new(x, cy, third_w, btn_h);
        result.content_items.push(("needs_setup_upload".to_string(), upload_rect));
        if !sync_tab_locked {
            input_coordinator.register_on_layer(
                "user_settings:needs_setup_upload",
                upload_rect,
                uzor::input::sense::Sense::CLICK,
                layer_id,
            );
        }

        // "Download Cloud Data" button
        let dl_x = x + third_w + 8.0;
        ctx.set_fill_color(&toolbar_theme.item_bg_hover);
        ctx.fill_rounded_rect(dl_x, cy, third_w, btn_h, 4.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(dl_x, cy, third_w, btn_h, 4.0);
        ctx.set_fill_color(text_color);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(uzor::render::TextAlign::Center);
        ctx.set_text_baseline(uzor::render::TextBaseline::Middle);
        ctx.fill_text("Download Cloud Data", dl_x + third_w / 2.0, cy + btn_h / 2.0);
        ctx.set_text_align(uzor::render::TextAlign::Left);
        let download_rect = uzor::types::Rect::new(dl_x, cy, third_w, btn_h);
        result.content_items.push(("needs_setup_download".to_string(), download_rect));
        if !sync_tab_locked {
            input_coordinator.register_on_layer(
                "user_settings:needs_setup_download",
                download_rect,
                uzor::input::sense::Sense::CLICK,
                layer_id,
            );
        }
        cy += btn_h + 8.0;

        // "Dismiss" link
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color("rgba(254,255,238,0.4)");
        ctx.set_text_baseline(uzor::render::TextBaseline::Top);
        ctx.fill_text("Dismiss", x, cy);
        let dismiss_rect = uzor::types::Rect::new(x, cy, 50.0, 16.0);
        result.content_items.push(("needs_setup_dismiss".to_string(), dismiss_rect));
        if !sync_tab_locked {
            input_coordinator.register_on_layer(
                "user_settings:needs_setup_dismiss",
                dismiss_rect,
                uzor::input::sense::Sense::CLICK,
                layer_id,
            );
        }
        cy += 24.0;
    }

    // ── Section: ENCRYPTION (only when sync enabled) ──────────────────────────
    if state.sync_enabled {
        ctx.set_font("600 11px sans-serif");
        ctx.set_fill_color("rgba(244,205,99,0.7)");
        ctx.set_text_baseline(uzor::render::TextBaseline::Top);
        ctx.fill_text("ENCRYPTION", x, cy);
        cy += section_gap;

        // Toggle: End-to-End Encryption
        {
            let row_rect = render_toggle_row(ctx, x, cy, available_w, "End-to-End Encryption", state.e2e_enabled, effective_text_color, toolbar_theme);
            result.content_items.push(("e2e_toggle".to_string(), row_rect));
            if !sync_tab_locked {
                input_coordinator.register_on_layer(
                    "user_settings:e2e_toggle",
                    row_rect,
                    uzor::input::sense::Sense::CLICK,
                    layer_id,
                );
            }
        }
        cy += row_gap;

        if state.e2e_enabled {
            // E2E Active — no passphrase input needed
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color("#5cb85c");
            ctx.set_text_baseline(uzor::render::TextBaseline::Top);
            ctx.fill_text("E2E Active", x, cy);
            cy += 18.0;

            // Note text
            ctx.set_font("10px sans-serif");
            ctx.set_fill_color("rgba(254,255,238,0.35)");
            ctx.fill_text("Your passphrase is never stored or transmitted. Keep it safe.", x, cy);
            cy += 18.0;
        } else if state.e2e_restore_mode {
            // ── E2E Restore Flow ──────────────────────────────────────────────
            ctx.set_font("600 11px sans-serif");
            ctx.set_fill_color("rgba(244,205,99,0.85)");
            ctx.set_text_baseline(uzor::render::TextBaseline::Top);
            ctx.fill_text("E2E RESTORE", x, cy);
            cy += 18.0;

            ctx.set_font("11px sans-serif");
            ctx.set_fill_color("rgba(254,255,238,0.55)");
            ctx.fill_text("This account has E2E encryption. Enter your passphrase to decrypt.", x, cy);
            cy += 18.0;

            // Passphrase label
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(effective_text_color);
            ctx.fill_text("Passphrase", x, cy);
            cy += 18.0;

            // Passphrase input box
            let input_h = 24.0;
            let masked: String = if state.e2e_passphrase.is_empty() {
                "Click to type passphrase...".to_string()
            } else {
                "*".repeat(state.e2e_passphrase.chars().count().min(20))
            };
            let input_color = if state.e2e_passphrase.is_empty() {
                "rgba(254,255,238,0.25)"
            } else {
                effective_text_color
            };
            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rounded_rect(x, cy, available_w, input_h, 3.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.begin_path();
            ctx.move_to(x, cy);
            ctx.line_to(x + available_w, cy);
            ctx.line_to(x + available_w, cy + input_h);
            ctx.line_to(x, cy + input_h);
            ctx.close_path();
            ctx.stroke();
            ctx.set_font("13px sans-serif");
            ctx.set_fill_color(input_color);
            ctx.set_text_baseline(uzor::render::TextBaseline::Middle);
            ctx.fill_text(&masked, x + 8.0, cy + input_h / 2.0);

            let input_rect = uzor::types::Rect::new(x, cy, available_w, input_h);
            result.content_items.push(("e2e_passphrase_input".to_string(), input_rect));
            if !sync_tab_locked {
                input_coordinator.register_on_layer(
                    "user_settings:e2e_passphrase_input",
                    input_rect,
                    uzor::input::sense::Sense::CLICK,
                    layer_id,
                );
            }
            cy += input_h + 8.0;

            // "Restore E2E" button (only when passphrase is non-empty)
            if !state.e2e_passphrase.is_empty() && !sync_tab_locked {
                let btn_w = 120.0;
                let btn_h = 26.0;
                ctx.set_fill_color(&toolbar_theme.accent);
                ctx.fill_rounded_rect(x, cy, btn_w, btn_h, 4.0);
                ctx.set_font("bold 12px sans-serif");
                ctx.set_fill_color("rgba(0,0,0,0.85)");
                ctx.set_text_align(uzor::render::TextAlign::Center);
                ctx.set_text_baseline(uzor::render::TextBaseline::Middle);
                ctx.fill_text("Restore E2E", x + btn_w / 2.0, cy + btn_h / 2.0);
                ctx.set_text_align(uzor::render::TextAlign::Left);

                let btn_rect = uzor::types::Rect::new(x, cy, btn_w, btn_h);
                result.content_items.push(("e2e_restore".to_string(), btn_rect));
                input_coordinator.register_on_layer(
                    "user_settings:e2e_restore",
                    btn_rect,
                    uzor::input::sense::Sense::CLICK,
                    layer_id,
                );
                cy += btn_h + 8.0;
            }
        } else {
            // ── Normal E2E Setup Flow ─────────────────────────────────────────
            // Passphrase label
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(effective_text_color);
            ctx.set_text_baseline(uzor::render::TextBaseline::Top);
            ctx.fill_text("Passphrase", x, cy);
            cy += 18.0;

            // Passphrase input box (read-only display)
            let input_h = 24.0;
            let masked: String = if state.e2e_passphrase.is_empty() {
                "Click to type passphrase...".to_string()
            } else {
                "*".repeat(state.e2e_passphrase.chars().count().min(20))
            };
            let input_color = if state.e2e_passphrase.is_empty() {
                "rgba(254,255,238,0.25)"
            } else {
                effective_text_color
            };
            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rounded_rect(x, cy, available_w, input_h, 3.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.begin_path();
            ctx.move_to(x, cy);
            ctx.line_to(x + available_w, cy);
            ctx.line_to(x + available_w, cy + input_h);
            ctx.line_to(x, cy + input_h);
            ctx.close_path();
            ctx.stroke();
            ctx.set_font("13px sans-serif");
            ctx.set_fill_color(input_color);
            ctx.set_text_baseline(uzor::render::TextBaseline::Middle);
            ctx.fill_text(&masked, x + 8.0, cy + input_h / 2.0);

            let input_rect = uzor::types::Rect::new(x, cy, available_w, input_h);
            result.content_items.push(("e2e_passphrase_input".to_string(), input_rect));
            if !sync_tab_locked {
                input_coordinator.register_on_layer(
                    "user_settings:e2e_passphrase_input",
                    input_rect,
                    uzor::input::sense::Sense::CLICK,
                    layer_id,
                );
            }
            cy += input_h + 8.0;

            // Setup E2E button (only when passphrase is non-empty and not locked)
            if !state.e2e_passphrase.is_empty() && !sync_tab_locked {
                let btn_w = 100.0;
                let btn_h = 26.0;
                ctx.set_fill_color(&toolbar_theme.accent);
                ctx.fill_rounded_rect(x, cy, btn_w, btn_h, 4.0);
                ctx.set_font("bold 12px sans-serif");
                ctx.set_fill_color("rgba(0,0,0,0.85)");
                ctx.set_text_align(uzor::render::TextAlign::Center);
                ctx.set_text_baseline(uzor::render::TextBaseline::Middle);
                ctx.fill_text("Setup E2E", x + btn_w / 2.0, cy + btn_h / 2.0);
                ctx.set_text_align(uzor::render::TextAlign::Left);

                let btn_rect = uzor::types::Rect::new(x, cy, btn_w, btn_h);
                result.content_items.push(("e2e_setup".to_string(), btn_rect));
                input_coordinator.register_on_layer(
                    "user_settings:e2e_setup",
                    btn_rect,
                    uzor::input::sense::Sense::CLICK,
                    layer_id,
                );
                cy += btn_h + 8.0;
            }

            // Note text
            ctx.set_font("10px sans-serif");
            ctx.set_fill_color("rgba(254,255,238,0.35)");
            ctx.set_text_baseline(uzor::render::TextBaseline::Top);
            ctx.fill_text("Your passphrase is never stored or transmitted. Keep it safe.", x, cy);
            cy += 18.0;
        }

        cy += 8.0;

        // ── Section: SYNC CATEGORIES ──────────────────────────────────────────
        ctx.set_font("600 11px sans-serif");
        ctx.set_fill_color("rgba(244,205,99,0.7)");
        ctx.set_text_baseline(uzor::render::TextBaseline::Top);
        ctx.fill_text("SYNC CATEGORIES", x, cy);
        cy += section_gap;

        let categories: &[(&str, &str, bool)] = &[
            ("sync_presets_toggle", "Presets", state.sync_presets),
            ("sync_watchlists_toggle", "Watchlists", state.sync_watchlists),
            ("sync_templates_toggle", "Templates", state.sync_templates),
            ("sync_snapshots_toggle", "Settings Snapshots", state.sync_snapshots),
        ];

        for (action_id, label, is_checked) in categories {
            let row_rect = render_checkbox_row(ctx, x, cy, label, *is_checked, effective_text_color, toolbar_theme);
            result.content_items.push((action_id.to_string(), row_rect));
            if !sync_tab_locked {
                let hit_id = format!("user_settings:{}", action_id);
                input_coordinator.register_on_layer(
                    hit_id.as_str(),
                    row_rect,
                    uzor::input::sense::Sense::CLICK,
                    layer_id,
                );
            }
            cy += row_gap - 6.0;
        }
    }

    let _ = cy;
}

#[allow(clippy::too_many_arguments)]
fn render_performance_tab(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    available_w: f64,
    state: &UserSettingsState,
    _text_color: &str,
    toolbar_theme: &ToolbarTheme,
    _frame_theme: &FrameTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    // Section label
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("INDICATOR RECALCULATION", x, y);

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
        border_focused: toolbar_theme.accent.clone(),
        accent:        toolbar_theme.accent.clone(),
        accent_hover:  toolbar_theme.accent.clone(),
        success:       "#26a69a".to_string(),
        warning:       "#ff9800".to_string(),
        danger:        "#ef5350".to_string(),
    };

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

    let radio_y = y + 22.0;
    let radio_result = draw_radio_group(
        ctx,
        &options,
        selected_index,
        state.hovered_item_id.as_deref(),
        x,
        radio_y,
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
            uzor::input::sense::Sense::CLICK,
            layer_id,
        );
    }

    // ── Diagnostics toggle ────────────────────────────────────────────────────
    // Placed below the radio group: 3 options × (52 + 8) px − last gap = 172 px
    let diag_section_y = radio_y + options.len() as f64 * (52.0 + 8.0) - 8.0 + 24.0;

    // Section label
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("DIAGNOSTICS", x, diag_section_y);

    let row_h = 24.0;
    let cb_y_offset = diag_section_y + 18.0;

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
        "Enable diagnostic logging",
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
        Sense::CLICK,
        layer_id,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_server_tab(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    available_w: f64,
    state: &UserSettingsState,
    _text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    let row_h = 24.0;
    let section_gap = 18.0;

    // ── Section: SERVER ───────────────────────────────────────────────────────
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("SERVER", x, y);

    // Enable toggle checkbox row
    let cb_y_offset = y + section_gap;
    let cb_size = 16.0;
    let cb_x = x;
    let cb_y = cb_y_offset + (row_h - cb_size) / 2.0;

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
        "Enable Agent API Server",
        cb_x + cb_size + 10.0,
        cb_y_offset + row_h / 2.0,
    );

    let toggle_rect = WidgetRect::new(cb_x, cb_y_offset, available_w, row_h);
    result.content_items.push(("server_toggle".to_string(), toggle_rect));
    input_coordinator.register_on_layer(
        "user_settings:server_toggle",
        uzor::types::Rect::new(cb_x, cb_y_offset, available_w, row_h),
        Sense::CLICK,
        layer_id,
    );

    // Status indicator row
    let status_row_y = cb_y_offset + row_h + 8.0;
    let dot_r = 4.0;
    let dot_cx = x + dot_r;
    let dot_cy = status_row_y + row_h / 2.0;

    let is_running = state.server_enabled && state.server_status == "running";
    let dot_color = if is_running { "#26a69a" } else { "#ef5350" };
    ctx.set_fill_color(dot_color);
    ctx.begin_path();
    ctx.arc(dot_cx, dot_cy, dot_r, 0.0, std::f64::consts::TAU);
    ctx.fill();

    let status_text = if is_running {
        format!("Running on :{}", state.server_port)
    } else {
        "Stopped".to_string()
    };
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&status_text, dot_cx + dot_r + 6.0, dot_cy);

    // ── Section: API KEYS ─────────────────────────────────────────────────────
    // This unified section replaces the old "API KEY" + "MANAGED KEYS" sections.
    let api_keys_y = status_row_y + row_h + 16.0;

    render_api_keys_section(
        ctx,
        x,
        api_keys_y,
        available_w,
        state,
        toolbar_theme,
        frame_theme,
        input_coordinator,
        layer_id,
        result,
    );
}

/// Render the unified API KEYS section:
///   1. "New key" row (label input + tier toggle + Create button)
///   2. "Created key" one-time reveal box (if last_created_key is Some)
///   3. "Registered keys" scrollable list with Delete buttons
#[allow(clippy::too_many_arguments)]
fn render_api_keys_section(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    available_w: f64,
    state: &UserSettingsState,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    let row_h = 24.0;
    let section_gap = 18.0;

    // Section header
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("API KEYS", x, y);

    let mut cursor_y = y + section_gap;

    // ── "New key" sub-label ──────────────────────────────────────────────────
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("New key:", x, cursor_y);
    cursor_y += 16.0;

    // ── Create form row ───────────────────────────────────────────────────────
    let form_row_h = 26.0;
    let tier_btn_w = 84.0;
    let create_btn_w = 64.0;
    let input_gap = 6.0;
    let label_input_w = available_w - tier_btn_w - create_btn_w - input_gap * 2.0;

    // Label text input box
    let label_input_x = x;
    let is_label_focused = state.new_key_label_focused;

    ctx.set_fill_color(&frame_theme.toolbar_bg);
    ctx.fill_rounded_rect(label_input_x, cursor_y, label_input_w, form_row_h, 4.0);
    let border_color = if is_label_focused {
        &toolbar_theme.accent
    } else {
        &toolbar_theme.separator
    };
    ctx.set_stroke_color(border_color);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(label_input_x, cursor_y, label_input_w, form_row_h, 4.0);

    if state.new_key_label.is_empty() && !is_label_focused {
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Label…", label_input_x + 6.0, cursor_y + form_row_h / 2.0);
    } else {
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(
            &state.new_key_label,
            label_input_x + 6.0,
            cursor_y + form_row_h / 2.0,
        );
    }

    let label_input_rect = WidgetRect::new(label_input_x, cursor_y, label_input_w, form_row_h);
    result.content_items.push(("server_key_label_input".to_string(), label_input_rect));
    input_coordinator.register_on_layer(
        "user_settings:server_key_label_input",
        uzor::types::Rect::new(label_input_x, cursor_y, label_input_w, form_row_h),
        Sense::CLICK,
        layer_id,
    );

    // Tier toggle button (read_only / read_write)
    let tier_btn_x = label_input_x + label_input_w + input_gap;
    let tier_label = match state.new_key_tier.as_str() {
        "read_write" => "read-write",
        "admin" => "admin",
        _ => "read-only",
    };
    let tier_color = match state.new_key_tier.as_str() {
        "read_write" => "#2196f3",
        "admin" => "#e74c3c",
        _ => "#26a69a",
    };
    ctx.set_stroke_color(tier_color);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(tier_btn_x, cursor_y, tier_btn_w, form_row_h, 4.0);
    ctx.set_fill_color(tier_color);
    ctx.set_font("10px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(tier_label, tier_btn_x + tier_btn_w / 2.0, cursor_y + form_row_h / 2.0);

    let tier_rect = WidgetRect::new(tier_btn_x, cursor_y, tier_btn_w, form_row_h);
    result.content_items.push(("server_key_tier_toggle".to_string(), tier_rect));
    input_coordinator.register_on_layer(
        "user_settings:server_key_tier_toggle",
        uzor::types::Rect::new(tier_btn_x, cursor_y, tier_btn_w, form_row_h),
        Sense::CLICK,
        layer_id,
    );

    // Create button
    let create_btn_x = tier_btn_x + tier_btn_w + input_gap;
    let label_is_empty = state.new_key_label.trim().is_empty();
    let create_color = if label_is_empty {
        &toolbar_theme.item_text_muted
    } else {
        &toolbar_theme.accent
    };
    ctx.set_stroke_color(create_color);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(create_btn_x, cursor_y, create_btn_w, form_row_h, 4.0);
    ctx.set_fill_color(create_color);
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(
        "Create",
        create_btn_x + create_btn_w / 2.0,
        cursor_y + form_row_h / 2.0,
    );

    let create_rect = WidgetRect::new(create_btn_x, cursor_y, create_btn_w, form_row_h);
    result.content_items.push(("server_key_create".to_string(), create_rect));
    input_coordinator.register_on_layer(
        "user_settings:server_key_create",
        uzor::types::Rect::new(create_btn_x, cursor_y, create_btn_w, form_row_h),
        Sense::CLICK,
        layer_id,
    );

    cursor_y += form_row_h + 10.0;

    // ── Last-created key reveal box ───────────────────────────────────────────
    if let Some(ref raw_key) = state.last_created_key {
        let box_h = 52.0;
        ctx.set_fill_color("#0d2a1a");
        ctx.fill_rounded_rect(x, cursor_y, available_w, box_h, 4.0);
        ctx.set_stroke_color("#26a69a");
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(x, cursor_y, available_w, box_h, 4.0);

        ctx.set_fill_color("#26a69a");
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(
            "Copy now — won't be shown again:",
            x + 8.0,
            cursor_y + 6.0,
        );

        // Show full raw key in monospace (it's already visible in the box)
        let display = if raw_key.len() > 32 {
            format!("{}…", &raw_key[..32])
        } else {
            raw_key.clone()
        };
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_font("11px monospace");
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(&display, x + 8.0, cursor_y + 22.0);

        // Copy button — right side, vertically centered
        let copy_w = 56.0;
        let copy_x = x + available_w - copy_w - 4.0;
        let copy_y = cursor_y + (box_h - row_h) / 2.0;
        ctx.set_stroke_color("#26a69a");
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(copy_x, copy_y, copy_w, row_h, 4.0);
        ctx.set_fill_color("#26a69a");
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Copy", copy_x + copy_w / 2.0, copy_y + row_h / 2.0);

        let copy_rect = WidgetRect::new(copy_x, copy_y, copy_w, row_h);
        result.content_items.push(("server_key_copy_new".to_string(), copy_rect));
        input_coordinator.register_on_layer(
            "user_settings:server_key_copy_new",
            uzor::types::Rect::new(copy_x, copy_y, copy_w, row_h),
            Sense::CLICK,
            layer_id,
        );

        cursor_y += box_h + 10.0;
    }

    // ── "Registered keys" separator + label ──────────────────────────────────
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(x, cursor_y + 7.0);
    ctx.line_to(x + available_w, cursor_y + 7.0);
    ctx.stroke();

    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Registered keys", x, cursor_y + 7.0);
    cursor_y += 20.0;

    // ── Keys list (scrollable when more than 3 keys) ──────────────────────────
    let item_h = 28.0;
    let num_keys = state.managed_keys.len();
    let total_keys_h = if num_keys == 0 {
        row_h + 4.0 // "No keys" placeholder
    } else {
        num_keys as f64 * (item_h + 4.0)
    };

    // Viewport height: show up to 3 keys; scroll if more
    let max_visible_keys = 3usize;
    let viewport_h = (item_h + 4.0) * max_visible_keys.min(num_keys.max(1)) as f64;

    let keys_viewport = uzor::types::Rect::new(x, cursor_y, available_w, viewport_h);

    // Build WidgetTheme for the scrollbar
    let widget_theme = WidgetTheme {
        bg_normal:      toolbar_theme.item_bg_hover.clone(),
        bg_hover:       toolbar_theme.item_bg_hover.clone(),
        bg_pressed:     toolbar_theme.item_bg_active.clone(),
        bg_disabled:    toolbar_theme.item_bg_hover.clone(),
        text_normal:    toolbar_theme.item_text.clone(),
        text_hover:     toolbar_theme.item_text_active.clone(),
        text_disabled:  toolbar_theme.item_text_muted.clone(),
        border_normal:  toolbar_theme.separator.clone(),
        border_hover:   toolbar_theme.separator.clone(),
        border_focused: toolbar_theme.accent.clone(),
        accent:         toolbar_theme.accent.clone(),
        accent_hover:   toolbar_theme.accent.clone(),
        success:        "#26a69a".to_string(),
        warning:        "#ff9800".to_string(),
        danger:         "#ef5350".to_string(),
    };

    let container = ScrollableContainer::new(
        keys_viewport,
        &state.server_keys_scroll,
        None,
    );
    container.begin(ctx);

    let content_start_y = container.content_y();
    let content_w = container.content_width();

    if state.managed_keys.is_empty() {
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("No keys yet.", x, content_start_y + row_h / 2.0);
    } else {
        for (idx, key_info) in state.managed_keys.iter().enumerate() {
            let item_y = content_start_y + idx as f64 * (item_h + 4.0);
            let delete_btn_w = 24.0;
            let delete_btn_x = x + content_w - delete_btn_w;

            // Row hover highlight
            let item_id = format!("server_key_row_{}", key_info.label);
            let is_hovered = state.hovered_item_id.as_deref() == Some(&item_id);
            if is_hovered {
                ctx.set_fill_color(&frame_theme.toolbar_bg);
                ctx.fill_rounded_rect(x, item_y, content_w, item_h, 2.0);
            }

            // Tier badge
            let badge_color = match key_info.tier.as_str() {
                "read_write" => "#2196f3",
                "admin" => "#ef5350",
                _ => "#26a69a",
            };
            let badge_w = 72.0;
            let badge_h = 18.0;
            let badge_y = item_y + (item_h - badge_h) / 2.0;
            ctx.set_fill_color(badge_color);
            ctx.fill_rounded_rect(x, badge_y, badge_w, badge_h, 9.0);
            ctx.set_fill_color("#ffffff");
            ctx.set_font("10px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            let tier_label = match key_info.tier.as_str() {
                "read_write" => "read-write",
                "admin" => "admin",
                _ => "read-only",
            };
            ctx.fill_text(tier_label, x + badge_w / 2.0, badge_y + badge_h / 2.0);

            // Label text (truncate if needed)
            ctx.set_fill_color(&toolbar_theme.item_text);
            ctx.set_font("12px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            let label_x = x + badge_w + 8.0;
            let label_max_w = delete_btn_x - label_x - 4.0;
            let display_label = if ctx.measure_text(&key_info.label) > label_max_w {
                let mut truncated = key_info.label.clone();
                while !truncated.is_empty()
                    && ctx.measure_text(&format!("{}…", truncated)) > label_max_w
                {
                    truncated.pop();
                }
                format!("{}…", truncated)
            } else {
                key_info.label.clone()
            };
            ctx.fill_text(&display_label, label_x, item_y + item_h / 2.0);

            // Delete [×] button — keyed by label (not index) for robustness
            let del_btn_y = item_y + (item_h - row_h) / 2.0;
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(delete_btn_x, del_btn_y, delete_btn_w, row_h, 4.0);
            ctx.set_fill_color("#ef5350");
            ctx.set_font("12px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("×", delete_btn_x + delete_btn_w / 2.0, item_y + item_h / 2.0);

            // Register hit zone using the key's label (not index)
            let del_item_id = format!("server_key_delete_{}", key_info.label);
            let del_rect = WidgetRect::new(delete_btn_x, del_btn_y, delete_btn_w, row_h);
            result.content_items.push((del_item_id.clone(), del_rect));
            input_coordinator.register_on_layer(
                format!("user_settings:{}", del_item_id).as_str(),
                uzor::types::Rect::new(delete_btn_x, del_btn_y, delete_btn_w, row_h),
                Sense::CLICK,
                layer_id,
            );
        }
    }

    let _scroll_result = container.end(ctx, total_keys_h, &widget_theme);
}

// =============================================================================
// Mode-transition confirmation dialogs
// =============================================================================

/// Render the inline confirmation panel shown when the user clicks "Connected"
/// from Standalone mode (sync_transition_pending = true).
///
/// Returns the new `cy` value after all rendered content.
#[allow(clippy::too_many_arguments)]
fn render_sync_connect_dialog(
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
    ctx.set_fill_color("rgba(244,205,99,0.06)");
    ctx.fill_rounded_rect(x - 6.0, cy - 6.0, available_w + 12.0, 220.0, 6.0);
    ctx.set_stroke_color("rgba(244,205,99,0.25)");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x - 6.0, cy - 6.0, available_w + 12.0, 220.0, 6.0);

    // Title
    ctx.set_font("600 12px sans-serif");
    ctx.set_fill_color("rgba(244,205,99,0.9)");
    ctx.set_text_align(uzor::render::TextAlign::Left);
    ctx.set_text_baseline(uzor::render::TextBaseline::Top);
    ctx.fill_text("CONNECT TO MYLITTLECHART.ORG?", x, cy);
    cy += 22.0;

    // Body text
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.55)");
    ctx.fill_text("Your local data stays on this machine.", x, cy);
    cy += 16.0;
    ctx.fill_text("Choose how to handle cloud synchronization:", x, cy);
    cy += 20.0;

    let btn_h = 28.0;
    let btn_gap = 8.0;
    let btn_w = available_w;

    // ── Button: Upload Local Data ─────────────────────────────────────────────
    ctx.set_fill_color(&toolbar_theme.item_bg_hover);
    ctx.fill_rounded_rect(x, cy, btn_w, btn_h, 4.0);
    ctx.set_stroke_color(&toolbar_theme.accent);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, cy, btn_w, btn_h, 4.0);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.accent);
    ctx.set_text_align(uzor::render::TextAlign::Center);
    ctx.set_text_baseline(uzor::render::TextBaseline::Middle);
    ctx.fill_text("Upload Local Data", x + btn_w / 2.0, cy + btn_h / 2.0);
    ctx.set_text_align(uzor::render::TextAlign::Left);
    result.content_items.push(("sync_upload".to_string(), WidgetRect::new(x, cy, btn_w, btn_h)));
    input_coordinator.register_on_layer(
        "user_settings:sync_upload",
        uzor::types::Rect::new(x, cy, btn_w, btn_h),
        Sense::CLICK,
        layer_id,
    );
    cy += btn_h + btn_gap;

    // Sub-label for Upload
    ctx.set_font("10px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.35)");
    ctx.set_text_baseline(uzor::render::TextBaseline::Top);
    ctx.fill_text("Push your presets and settings to the cloud.", x, cy);
    cy += 16.0;

    // ── Button: Download Cloud Data ───────────────────────────────────────────
    ctx.set_fill_color(&toolbar_theme.item_bg_hover);
    ctx.fill_rounded_rect(x, cy, btn_w, btn_h, 4.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, cy, btn_w, btn_h, 4.0);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(uzor::render::TextAlign::Center);
    ctx.set_text_baseline(uzor::render::TextBaseline::Middle);
    ctx.fill_text("Download Cloud Data", x + btn_w / 2.0, cy + btn_h / 2.0);
    ctx.set_text_align(uzor::render::TextAlign::Left);
    result.content_items.push(("sync_download".to_string(), WidgetRect::new(x, cy, btn_w, btn_h)));
    input_coordinator.register_on_layer(
        "user_settings:sync_download",
        uzor::types::Rect::new(x, cy, btn_w, btn_h),
        Sense::CLICK,
        layer_id,
    );
    cy += btn_h + btn_gap;

    // Sub-label for Download
    ctx.set_font("10px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.35)");
    ctx.set_text_baseline(uzor::render::TextBaseline::Top);
    ctx.fill_text("Replace local syncable data with your cloud version.", x, cy);
    cy += 16.0;

    // ── Button: Start Fresh ───────────────────────────────────────────────────
    ctx.set_fill_color(&toolbar_theme.item_bg_hover);
    ctx.fill_rounded_rect(x, cy, btn_w, btn_h, 4.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, cy, btn_w, btn_h, 4.0);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(uzor::render::TextAlign::Center);
    ctx.set_text_baseline(uzor::render::TextBaseline::Middle);
    ctx.fill_text("Start Fresh", x + btn_w / 2.0, cy + btn_h / 2.0);
    ctx.set_text_align(uzor::render::TextAlign::Left);
    result.content_items.push(("sync_fresh".to_string(), WidgetRect::new(x, cy, btn_w, btn_h)));
    input_coordinator.register_on_layer(
        "user_settings:sync_fresh",
        uzor::types::Rect::new(x, cy, btn_w, btn_h),
        Sense::CLICK,
        layer_id,
    );
    cy += btn_h + btn_gap;

    // Sub-label for Fresh
    ctx.set_font("10px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.35)");
    ctx.set_text_baseline(uzor::render::TextBaseline::Top);
    ctx.fill_text("Don't upload anything. Start with an empty cloud profile.", x, cy);
    cy += 16.0;

    // ── Button: Cancel ────────────────────────────────────────────────────────
    ctx.set_fill_color("rgba(239,83,80,0.10)");
    ctx.fill_rounded_rect(x, cy, btn_w, btn_h, 4.0);
    ctx.set_stroke_color("rgba(239,83,80,0.35)");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, cy, btn_w, btn_h, 4.0);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color("rgba(239,83,80,0.8)");
    ctx.set_text_align(uzor::render::TextAlign::Center);
    ctx.set_text_baseline(uzor::render::TextBaseline::Middle);
    ctx.fill_text("Cancel — Stay Offline", x + btn_w / 2.0, cy + btn_h / 2.0);
    ctx.set_text_align(uzor::render::TextAlign::Left);
    result.content_items.push(("sync_cancel".to_string(), WidgetRect::new(x, cy, btn_w, btn_h)));
    input_coordinator.register_on_layer(
        "user_settings:sync_cancel",
        uzor::types::Rect::new(x, cy, btn_w, btn_h),
        Sense::CLICK,
        layer_id,
    );
    cy += btn_h;

    cy
}

/// Render the inline confirmation panel shown when the user clicks "Standalone"
/// from Connected mode (disconnect_pending = true).
///
/// Returns the new `cy` value after all rendered content.
#[allow(clippy::too_many_arguments)]
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
        Sense::CLICK,
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
        Sense::CLICK,
        layer_id,
    );
    cy += btn_h;

    cy
}
