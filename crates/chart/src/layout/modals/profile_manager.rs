//! Unified Profile Manager overlay.
//!
//! A single full-screen modal that replaces the old vault_unlock overlay and
//! vault_profile_picker with a coherent profile management flow.
//!
//! Pages:
//!   ProfileList       — List all profiles; select one to load or create a new one.
//!   UnlockPassphrase  — Enter passphrase to unlock a profile with vault.enc.
//!   CreatePassphrase  — Set a new passphrase for a profile that has no vault.
//!   CreateNew         — Enter name for a brand-new profile (sync toggled in settings).

use crate::engine::render::{RenderContext, draw_svg_icon, draw_svg_multicolor};

const MASCOT_SVG: &str = include_str!("../../../../../assets/mascot/mascot.svg");
use crate::ui::Icon;
use uzor::render::{TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;
use uzor::input::sense::Sense;
use crate::ui::modal_settings::{UserSettingsState, ProfileManagerPage};
use crate::ui::toolbar_render::ToolbarTheme;
use crate::layout::render_frame::UserSettingsResult;
use crate::ui::z_order::ZLayer;
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig, InputType};
use crate::ui::widgets::types::WidgetState;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::layout::render_chart::FrameTheme;

/// Render the unified Profile Manager overlay.
///
/// This is a non-closeable overlay that handles all profile-related flows:
/// listing profiles, unlocking encrypted profiles, setting up passphrase for
/// new profiles, and creating new profiles.
///
/// `content_x/y` are the top-left origin of the content area (below the top
/// toolbar, right of the left toolbar).  `content_w/h` are its dimensions.
/// In skeleton mode these fill the area between all four toolbars; in
/// non-skeleton mode the caller may pass the full window dimensions.
#[allow(clippy::too_many_arguments)]
pub fn render_profile_manager(
    ctx: &mut dyn RenderContext,
    content_x: f64,
    content_y: f64,
    content_w: f64,
    content_h: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    // Push a high-z modal layer so the profile manager absorbs all input
    let layer_id = ZLayer::ModalOverlay.push_named(input_coordinator, "profile_manager");

    // Block all clicks on the dimmer (profile manager is non-closeable)
    input_coordinator.register_on_layer(
        "profile_manager:dimmer",
        WidgetRect::new(content_x, content_y, content_w, content_h),
        Sense::CLICK,
        &layer_id,
    );

    match &state.profile_manager_page {
        ProfileManagerPage::ProfileList => render_page_profile_list(
            ctx, content_x, content_y, content_w, content_h, state, text_color, toolbar_theme,
            &layer_id, input_coordinator, result,
        ),
        ProfileManagerPage::UnlockPassphrase => render_page_unlock(
            ctx, content_x, content_y, content_w, content_h, state, text_color, toolbar_theme,
            frame_theme, current_time_ms, &layer_id, input_coordinator, result,
        ),
        ProfileManagerPage::CreatePassphrase => render_page_create_passphrase(
            ctx, content_x, content_y, content_w, content_h, state, text_color, toolbar_theme,
            frame_theme, current_time_ms, &layer_id, input_coordinator, result,
        ),
        ProfileManagerPage::CreateNew => render_page_create_new(
            ctx, content_x, content_y, content_w, content_h, state, text_color, toolbar_theme,
            frame_theme, current_time_ms, &layer_id, input_coordinator, result,
        ),
        ProfileManagerPage::ShowRecoveryKey => render_page_show_recovery_key(
            ctx, content_x, content_y, content_w, content_h, state, text_color, toolbar_theme,
            frame_theme, current_time_ms, &layer_id, input_coordinator, result,
        ),
        ProfileManagerPage::UseRecoveryKey => render_page_use_recovery_key(
            ctx, content_x, content_y, content_w, content_h, state, text_color, toolbar_theme,
            frame_theme, current_time_ms, &layer_id, input_coordinator, result,
        ),
        ProfileManagerPage::SetNewPassphrase => render_page_set_new_passphrase(
            ctx, content_x, content_y, content_w, content_h, state, text_color, toolbar_theme,
            frame_theme, current_time_ms, &layer_id, input_coordinator, result,
        ),
    }
}

// =============================================================================
// Page: ProfileList
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page_profile_list(
    ctx: &mut dyn RenderContext,
    content_x: f64,
    content_y: f64,
    content_w: f64,
    content_h: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();

    // ── Full-area modal geometry ──────────────────────────────────────────────
    let modal_x = content_x;
    let modal_y = content_y;
    let modal_w = content_w;
    let modal_h = content_h;

    // Modal background
    ctx.set_fill_color(&toolbar_theme.background);
    ctx.fill_rect(modal_x, modal_y, modal_w, modal_h);

    // Absorb modal background clicks
    input_coordinator.register_on_layer(
        "profile_manager:modal_bg",
        WidgetRect::new(modal_x, modal_y, modal_w, modal_h),
        Sense::CLICK,
        layer_id,
    );

    // ── Split layout ─────────────────────────────────────────────────────────
    // Left column: account info, updates
    // Right column: profile list, create button, cloud profiles
    let left_col_w: f64 = 240.0;
    let divider_x = modal_x + left_col_w;
    let right_col_x = divider_x + 1.0;
    let right_col_w = modal_w - left_col_w - 1.0;

    // Vertical divider
    ctx.set_stroke_color("rgba(255,255,255,0.08)");
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(divider_x, modal_y + 20.0);
    ctx.line_to(divider_x, modal_y + modal_h - 20.0);
    ctx.stroke();

    // ── LEFT COLUMN: Account panel ────────────────────────────────────────────
    let left_pad = 20.0;
    let left_inner_x = modal_x + left_pad;
    let left_inner_w = left_col_w - left_pad * 2.0;
    let mut left_cy = modal_y + 28.0;

    // "ACCOUNT" section header
    ctx.set_font("bold 10px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.35)");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("ACCOUNT", left_inner_x, left_cy);
    left_cy += 20.0;

    if state.is_logged_in {
        // Display name
        ctx.set_font("bold 14px sans-serif");
        ctx.set_fill_color(text_color);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(&state.auth_display_name, left_inner_x, left_cy);
        left_cy += 20.0;

        // Provider label
        if !state.auth_provider.is_empty() {
            ctx.set_font("11px sans-serif");
            ctx.set_fill_color("rgba(254,255,238,0.45)");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.fill_text(&format!("via {}", state.auth_provider), left_inner_x, left_cy);
            left_cy += 18.0;
        }

        left_cy += 10.0;

        // "Log Out" button
        let logout_btn_h = 30.0;
        let logout_id = "profile_mgr:logout";
        let is_logout_hovered = hovered == Some(logout_id);
        let logout_bg = if is_logout_hovered {
            "rgba(255,80,80,0.35)"
        } else {
            "rgba(255,80,80,0.15)"
        };
        ctx.set_fill_color(logout_bg);
        ctx.fill_rounded_rect(left_inner_x, left_cy, left_inner_w, logout_btn_h, 4.0);
        let logout_text_color = if is_logout_hovered { "rgba(255,120,100,1.0)" } else { "rgba(255,120,100,0.75)" };
        draw_svg_icon(ctx, Icon::LogOut.svg(), left_inner_x + 10.0, left_cy + (logout_btn_h - 14.0) / 2.0, 14.0, 14.0, logout_text_color);
        ctx.set_font("bold 11px sans-serif");
        ctx.set_fill_color(logout_text_color);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Log Out", left_inner_x + left_inner_w / 2.0 + 7.0, left_cy + logout_btn_h / 2.0);
        ctx.set_text_align(TextAlign::Left);

        let logout_rect = WidgetRect::new(left_inner_x, left_cy, left_inner_w, logout_btn_h);
        result.content_items.push((logout_id.to_string(), logout_rect));
        input_coordinator.register_on_layer(
            format!("user_settings:{}", logout_id).as_str(),
            logout_rect,
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
        left_cy += logout_btn_h;
    } else {
        // Not logged in — brief description
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color("rgba(254,255,238,0.40)");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text("Sign in to enable cloud", left_inner_x, left_cy);
        left_cy += 16.0;
        ctx.fill_text("sync and profile backup.", left_inner_x, left_cy);
        left_cy += 24.0;

        // "Sign In via Browser" button
        let signin_btn_h = 32.0;
        let signin_id = "profile_mgr:sign_in";
        let is_signin_hovered = hovered == Some(signin_id);
        let signin_bg = if is_signin_hovered {
            "rgba(255,255,255,0.92)"
        } else {
            toolbar_theme.accent.as_str()
        };
        ctx.set_fill_color(signin_bg);
        ctx.fill_rounded_rect(left_inner_x, left_cy, left_inner_w, signin_btn_h, 4.0);
        draw_svg_icon(ctx, Icon::LogIn.svg(), left_inner_x + 10.0, left_cy + (signin_btn_h - 16.0) / 2.0, 16.0, 16.0, "rgba(0,0,0,0.85)");
        ctx.set_font("bold 11px sans-serif");
        ctx.set_fill_color("rgba(0,0,0,0.85)");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Sign In via Browser", left_inner_x + left_inner_w / 2.0 + 8.0, left_cy + signin_btn_h / 2.0);
        ctx.set_text_align(TextAlign::Left);

        let signin_rect = WidgetRect::new(left_inner_x, left_cy, left_inner_w, signin_btn_h);
        result.content_items.push((signin_id.to_string(), signin_rect));
        input_coordinator.register_on_layer(
            format!("user_settings:{}", signin_id).as_str(),
            signin_rect,
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
        left_cy += signin_btn_h;
    }

    // ── Separator ─────────────────────────────────────────────────────────────
    left_cy += 20.0;
    ctx.set_stroke_color("rgba(255,255,255,0.08)");
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(left_inner_x, left_cy);
    ctx.line_to(left_inner_x + left_inner_w, left_cy);
    ctx.stroke();
    left_cy += 16.0;

    // ── UPDATES section ────────────────────────────────────────────────────────
    ctx.set_font("bold 10px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.35)");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("UPDATES", left_inner_x, left_cy);
    left_cy += 18.0;

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.65)");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(&format!("v{}", env!("CARGO_PKG_VERSION")), left_inner_x, left_cy);
    left_cy += 22.0;

    // ── Device mode toggles ──────────────────────────────────────────────────
    // Connected / Standalone radio pair
    let radio_r = 5.0;
    let radio_row_h = 22.0;
    let radio_label_x = left_inner_x + radio_r * 2.0 + 7.0;

    // "Connected" row
    {
        let id = "profile_mgr:device_connected";
        let is_active = state.device_ota_enabled;
        let circle_cy = left_cy + radio_row_h / 2.0;
        let row_rect = WidgetRect::new(left_inner_x, left_cy, left_inner_w, radio_row_h);
        if is_active {
            ctx.set_fill_color("rgba(244,205,99,0.9)");
            ctx.begin_path();
            ctx.arc(left_inner_x + radio_r, circle_cy, radio_r, 0.0, std::f64::consts::TAU);
            ctx.fill();
        } else {
            ctx.set_stroke_color("rgba(254,255,238,0.35)");
            ctx.set_stroke_width(1.0);
            ctx.begin_path();
            ctx.arc(left_inner_x + radio_r, circle_cy, radio_r, 0.0, std::f64::consts::TAU);
            ctx.stroke();
        }
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(if is_active { "rgba(254,255,238,0.90)" } else { "rgba(254,255,238,0.45)" });
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Connected", radio_label_x, circle_cy);
        result.content_items.push((id.to_string(), row_rect));
        input_coordinator.register_on_layer(
            id,
            row_rect,
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
        left_cy += radio_row_h;
    }

    // "Standalone" row
    {
        let id = "profile_mgr:device_standalone";
        let is_active = !state.device_ota_enabled;
        let circle_cy = left_cy + radio_row_h / 2.0;
        let row_rect = WidgetRect::new(left_inner_x, left_cy, left_inner_w, radio_row_h);
        if is_active {
            ctx.set_fill_color("rgba(244,205,99,0.9)");
            ctx.begin_path();
            ctx.arc(left_inner_x + radio_r, circle_cy, radio_r, 0.0, std::f64::consts::TAU);
            ctx.fill();
        } else {
            ctx.set_stroke_color("rgba(254,255,238,0.35)");
            ctx.set_stroke_width(1.0);
            ctx.begin_path();
            ctx.arc(left_inner_x + radio_r, circle_cy, radio_r, 0.0, std::f64::consts::TAU);
            ctx.stroke();
        }
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(if is_active { "rgba(254,255,238,0.90)" } else { "rgba(254,255,238,0.45)" });
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Standalone", radio_label_x, circle_cy);
        result.content_items.push((id.to_string(), row_rect));
        input_coordinator.register_on_layer(
            id,
            row_rect,
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
        left_cy += radio_row_h;
    }

    // Channel selector — only shown in Connected mode
    if state.device_ota_enabled {
        left_cy += 8.0;
        ctx.set_font("bold 10px sans-serif");
        ctx.set_fill_color("rgba(254,255,238,0.30)");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text("CHANNEL", left_inner_x, left_cy);
        left_cy += 16.0;

        let channel_rows: &[(&str, &str, &str)] = &[
            ("stable", "Stable", "profile_mgr:channel_stable"),
            ("dev",    "Dev",    "profile_mgr:channel_dev"),
        ];
        for &(channel_key, label, id) in channel_rows {
            let is_active = state.device_update_channel == channel_key;
            let circle_cy = left_cy + radio_row_h / 2.0;
            let row_rect = WidgetRect::new(left_inner_x, left_cy, left_inner_w, radio_row_h);
            if is_active {
                ctx.set_fill_color("rgba(244,205,99,0.9)");
                ctx.begin_path();
                ctx.arc(left_inner_x + radio_r, circle_cy, radio_r, 0.0, std::f64::consts::TAU);
                ctx.fill();
            } else {
                ctx.set_stroke_color("rgba(254,255,238,0.35)");
                ctx.set_stroke_width(1.0);
                ctx.begin_path();
                ctx.arc(left_inner_x + radio_r, circle_cy, radio_r, 0.0, std::f64::consts::TAU);
                ctx.stroke();
            }
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(if is_active { "rgba(254,255,238,0.90)" } else { "rgba(254,255,238,0.45)" });
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(label, radio_label_x, circle_cy);
            result.content_items.push((id.to_string(), row_rect));
            input_coordinator.register_on_layer(
                id,
                row_rect,
                Sense::CLICK | Sense::HOVER,
                layer_id,
            );
            left_cy += radio_row_h;
        }
    }

    // Mascot at bottom of left panel — pre-rendered PNG, maintain aspect ratio
    let mascot_w = 140.0;
    let mascot_h = mascot_w * (251.0 / 197.0); // ~178px
    let mascot_x = left_inner_x + (left_inner_w - mascot_w) / 2.0;
    let mascot_y = modal_y + modal_h - mascot_h - 8.0;
    draw_svg_multicolor(ctx, MASCOT_SVG, mascot_x, mascot_y, mascot_w, mascot_h);

    // ── RIGHT COLUMN: Profile list ────────────────────────────────────────────
    let right_pad = 24.0;
    let inner_x = right_col_x + right_pad;
    let inner_w = right_col_w - right_pad * 2.0;
    let mut cy = modal_y + 28.0;

    let profile_row_h: f64 = 52.0;
    let cloud_row_h: f64 = 44.0;

    // Filter cloud profiles: exclude any that are already present locally.
    let local_ids: std::collections::HashSet<&str> = state
        .profiles_with_vault_status
        .iter()
        .map(|(id, _, _, _, _, _)| id.as_str())
        .collect();
    let cloud_profiles_to_show: Vec<&crate::ui::modal_settings::CloudProfileEntry> = state
        .cloud_profiles
        .iter()
        .filter(|cp| !local_ids.contains(cp.profile_id.as_str()))
        .collect();
    let has_cloud_section = !cloud_profiles_to_show.is_empty()
        || state.cloud_profiles_loading
        || !state.cloud_profiles_error.is_empty();

    // Close button (×) — only when a live profile is running AND not in skeleton lock
    if !state.runtime_profile_id.is_empty() && !state.needs_vault_unlock {
        let close_size = 28.0;
        let close_x = right_col_x + right_col_w - right_pad - close_size + 4.0;
        let close_y = modal_y + 8.0;
        let close_id = "user_settings:profile_mgr:close";
        let close_hovered = hovered == Some("profile_mgr:close");

        if close_hovered {
            ctx.set_fill_color("rgba(255,255,255,0.10)");
            ctx.fill_rounded_rect(close_x, close_y, close_size, close_size, 4.0);
        }

        draw_svg_icon(ctx, Icon::Close.svg(), close_x + 4.0, close_y + 4.0, 20.0, 20.0,
            if close_hovered { "rgba(255,255,255,0.9)" } else { "rgba(255,255,255,0.5)" });

        let close_rect = WidgetRect::new(close_x, close_y, close_size, close_size);
        result.content_items.push(("profile_mgr:close".to_string(), close_rect));
        input_coordinator.register_on_layer(
            close_id,
            close_rect,
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
    }

    // Title: "Profiles"
    ctx.set_font("bold 20px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Profiles", inner_x, cy);
    cy += 28.0;

    // ZT badge — same font size as title
    draw_svg_icon(ctx, Icon::ShieldCheck.svg(), inner_x, cy + 3.0, 16.0, 16.0, "rgba(80,200,120,0.50)");
    ctx.set_font("bold 20px sans-serif");
    ctx.set_fill_color("rgba(80,200,120,0.50)");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Zero-trust encrypted", inner_x + 22.0, cy);
    cy += 32.0;

    // "Create New Profile" button — full width, accent style
    let create_btn_h = 36.0;
    let create_id = "profile_mgr:create_new";
    let is_create_hovered = hovered == Some(create_id);
    let create_bg = if is_create_hovered {
        "rgba(255,255,255,0.12)"
    } else {
        "rgba(255,255,255,0.06)"
    };
    ctx.set_fill_color(create_bg);
    ctx.fill_rounded_rect(inner_x, cy, inner_w, create_btn_h, 4.0);

    // "+" icon + label
    let btn_center_y = cy + create_btn_h / 2.0;
    ctx.set_font("bold 18px sans-serif");
    ctx.set_fill_color(if is_create_hovered { "rgba(255,255,255,0.85)" } else { "rgba(255,255,255,0.50)" });
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("+ Create New Profile", inner_x + inner_w / 2.0, btn_center_y);
    ctx.set_text_align(TextAlign::Left);

    let create_rect = WidgetRect::new(inner_x, cy, inner_w, create_btn_h);
    result.content_items.push((create_id.to_string(), create_rect));
    input_coordinator.register_on_layer(
        format!("user_settings:{}", create_id).as_str(),
        create_rect,
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );

    cy += create_btn_h + 8.0;

    // ── Scrollable viewport for profile rows + cloud section ──────────────────
    // No bottom button — list fills all available space.
    let scroll_viewport_y = cy;
    let scroll_viewport_h = ((modal_y + modal_h) - scroll_viewport_y - 12.0)
        .max(0.0);

    // Measure total unclipped content height (profile rows + optional cloud section).
    let mut total_content_h: f64 = 0.0;
    for _ in &state.profiles_with_vault_status {
        total_content_h += profile_row_h + 6.0;
    }
    if has_cloud_section {
        total_content_h += 16.0; // gap before section header
        total_content_h += 24.0; // section header row
        if state.cloud_profiles_loading || !state.cloud_profiles_error.is_empty() {
            total_content_h += 24.0;
        }
        for _ in &cloud_profiles_to_show {
            total_content_h += cloud_row_h + 6.0;
        }
    }

    // Clamp scroll offset against the actual max.
    let max_scroll = (total_content_h - scroll_viewport_h).max(0.0);
    let scroll_offset = state.profile_list_scroll.offset.min(max_scroll);

    // Store viewport rect and total content height in result for the scroll handler.
    result.profile_list_viewport_rect = WidgetRect::new(
        inner_x,
        scroll_viewport_y,
        inner_w,
        scroll_viewport_h,
    );
    result.profile_list_total_content_h = total_content_h;

    // Apply clip rect — all profile rows and cloud section render inside.
    ctx.save();
    ctx.clip_rect(inner_x, scroll_viewport_y, inner_w, scroll_viewport_h);

    // Shift starting y by the scroll offset.
    cy = scroll_viewport_y - scroll_offset;

    // Profile rows
    for (id, display_name, _avatar, _client_mode, has_vault, _sync_level) in &state.profiles_with_vault_status {
        let widget_id = format!("profile_mgr:select:{}", id);
        let is_row_hovered = hovered == Some(widget_id.as_str());
        let is_active = *id == state.runtime_profile_id;

        // Row background
        let row_bg = if is_row_hovered {
            "rgba(255,255,255,0.08)"
        } else {
            "rgba(255,255,255,0.04)"
        };
        ctx.set_fill_color(row_bg);
        ctx.fill_rounded_rect(inner_x, cy, inner_w, profile_row_h, 4.0);

        // Active profile: left accent border
        if is_active {
            ctx.set_fill_color(&toolbar_theme.accent);
            ctx.fill_rounded_rect(inner_x, cy, 3.0, profile_row_h, 2.0);
        }

        let row_mid_y = cy + profile_row_h / 2.0;

        // Display name (14px bold, left)
        let name_x = if is_active { inner_x + 10.0 } else { inner_x + 8.0 };
        ctx.set_font("bold 14px sans-serif");
        ctx.set_fill_color(text_color);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(display_name.as_str(), name_x, row_mid_y);

        if *has_vault {
            // Encrypted profile: just the name, no right-side badges
            let row_rect = WidgetRect::new(inner_x, cy, inner_w, profile_row_h);
            result.content_items.push((widget_id.clone(), row_rect));
            let hit_id = format!("user_settings:{}", widget_id);
            input_coordinator.register_on_layer(hit_id.as_str(), row_rect, Sense::CLICK | Sense::HOVER, layer_id);
        } else {
            // Unencrypted: show warning + delete button (no mode_label)
            ctx.set_font("10px sans-serif");
            ctx.set_text_align(TextAlign::Right);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.set_fill_color("rgba(255,100,80,0.8)");
            ctx.fill_text("Unprotected", inner_x + inner_w - 70.0, row_mid_y);

            // Delete button
            let del_w = 54.0;
            let del_h = 22.0;
            let del_x = inner_x + inner_w - del_w - 6.0;
            let del_y = row_mid_y - del_h / 2.0;
            let del_id = format!("profile_delete:{}", id);
            let is_del_hovered = hovered == Some(del_id.as_str());
            let del_bg = if is_del_hovered { "rgba(255,60,60,0.6)" } else { "rgba(255,60,60,0.3)" };
            ctx.set_fill_color(del_bg);
            ctx.fill_rounded_rect(del_x, del_y, del_w, del_h, 3.0);
            ctx.set_font("bold 10px sans-serif");
            ctx.set_fill_color("rgba(255,255,255,0.9)");
            ctx.set_text_align(TextAlign::Center);
            ctx.fill_text("Delete", del_x + del_w / 2.0, row_mid_y);
            ctx.set_text_align(TextAlign::Left);

            // Register row hit area FIRST (lower priority)
            let row_rect = WidgetRect::new(inner_x, cy, inner_w, profile_row_h);
            result.content_items.push((widget_id.clone(), row_rect));
            let hit_id = format!("user_settings:{}", widget_id);
            input_coordinator.register_on_layer(hit_id.as_str(), row_rect, Sense::CLICK | Sense::HOVER, layer_id);

            // Register delete button SECOND (higher priority — on top of row)
            let del_rect = WidgetRect::new(del_x, del_y, del_w, del_h);
            result.content_items.push((del_id.clone(), del_rect));
            input_coordinator.register_on_layer(
                format!("user_settings:{}", del_id).as_str(),
                del_rect,
                Sense::CLICK | Sense::HOVER,
                layer_id,
            );
        }

        cy += profile_row_h + 6.0;
    }

    // ── CLOUD PROFILES section (inside clip) ──────────────────────────────────
    if has_cloud_section {
        cy += 16.0;

        // Section header row: "CLOUD PROFILES" label + "Refresh" link
        ctx.set_font("bold 10px sans-serif");
        ctx.set_fill_color("rgba(254,255,238,0.35)");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("CLOUD PROFILES", inner_x, cy + 10.0);

        // "Refresh" link on the right
        let refresh_id = "profile_mgr:refresh_cloud_profiles";
        let is_refresh_hovered = hovered == Some(refresh_id);
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(if is_refresh_hovered {
            "rgba(244,205,99,0.9)"
        } else {
            "rgba(244,205,99,0.55)"
        });
        ctx.set_text_align(TextAlign::Right);
        ctx.fill_text("Refresh", inner_x + inner_w, cy + 10.0);
        ctx.set_text_align(TextAlign::Left);

        let refresh_rect = WidgetRect::new(inner_x + inner_w - 50.0, cy, 50.0, 20.0);
        result.content_items.push((refresh_id.to_string(), refresh_rect));
        input_coordinator.register_on_layer(
            format!("user_settings:{}", refresh_id).as_str(),
            refresh_rect,
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
        cy += 24.0;

        // Loading / error state
        if state.cloud_profiles_loading {
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color("rgba(254,255,238,0.45)");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.fill_text("Loading\u{2026}", inner_x, cy);
            cy += 24.0;
        } else if !state.cloud_profiles_error.is_empty() {
            ctx.set_font("11px sans-serif");
            ctx.set_fill_color("rgba(255,100,80,0.85)");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.fill_text(&state.cloud_profiles_error, inner_x, cy);
            cy += 24.0;
        }

        // Cloud profile rows
        for cp in &cloud_profiles_to_show {
            let row_id = format!("profile_mgr:cloud_restore:{}", cp.profile_id);
            let is_row_hovered = hovered == Some(row_id.as_str());

            // Row background
            let row_bg = if is_row_hovered {
                "rgba(255,255,255,0.07)"
            } else {
                "rgba(255,255,255,0.03)"
            };
            ctx.set_fill_color(row_bg);
            ctx.fill_rounded_rect(inner_x, cy, inner_w, cloud_row_h, 4.0);

            let row_mid_y = cy + cloud_row_h / 2.0;

            // Lock icon for encrypted cloud profiles
            let label_x = if cp.has_vault {
                draw_svg_icon(ctx, Icon::Lock.svg(), inner_x + 6.0, row_mid_y - 7.0, 14.0, 14.0, "rgba(254,255,238,0.5)");
                inner_x + 26.0
            } else {
                inner_x + 8.0
            };
            // Display name (bold) + short profile ID (dimmer)
            let short_id: String = cp.profile_id.chars().take(8).collect();
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            let mut text_x = label_x;
            if let Some(ref name) = cp.display_name {
                ctx.set_font("bold 13px sans-serif");
                ctx.set_fill_color("rgba(254,255,238,0.85)");
                ctx.fill_text(name, text_x, row_mid_y);
                text_x += ctx.measure_text(name) + 6.0;
                ctx.set_font("11px sans-serif");
                ctx.set_fill_color("rgba(254,255,238,0.35)");
                ctx.fill_text(&format!("({})", short_id), text_x, row_mid_y);
            } else {
                ctx.set_font("bold 13px sans-serif");
                ctx.set_fill_color("rgba(254,255,238,0.75)");
                ctx.fill_text(&short_id, text_x, row_mid_y);
            }

            // Item count + size
            let size_str = if cp.total_bytes >= 1_048_576 {
                format!(
                    "{} items, {:.1} MB",
                    cp.item_count,
                    cp.total_bytes as f64 / 1_048_576.0
                )
            } else {
                format!(
                    "{} items, {} KB",
                    cp.item_count,
                    cp.total_bytes / 1024
                )
            };
            ctx.set_font("10px sans-serif");
            ctx.set_fill_color("rgba(254,255,238,0.35)");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(&size_str, inner_x + 8.0, row_mid_y + 13.0);

            // Restore button (right side)
            let btn_w = 60.0;
            let btn_h = 22.0;
            let btn_x = inner_x + inner_w - btn_w - 6.0;
            let btn_y = row_mid_y - btn_h / 2.0;

            let is_restoring = state.restoring_profile_id.as_deref() == Some(&cp.profile_id);
            let btn_label = if is_restoring { "Restoring\u{2026}" } else { "Restore" };
            let btn_bg = if is_restoring {
                "rgba(100,100,100,0.4)"
            } else if is_row_hovered {
                "rgba(244,205,99,0.6)"
            } else {
                "rgba(244,205,99,0.30)"
            };
            ctx.set_fill_color(btn_bg);
            ctx.fill_rounded_rect(btn_x, btn_y, btn_w, btn_h, 3.0);

            let btn_text_color = if is_restoring {
                "rgba(200,200,200,0.7)"
            } else {
                "rgba(0,0,0,0.85)"
            };
            ctx.set_font("bold 10px sans-serif");
            ctx.set_fill_color(btn_text_color);
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(btn_label, btn_x + btn_w / 2.0, row_mid_y);
            ctx.set_text_align(TextAlign::Left);

            // Register row click area (covers entire row including button)
            let row_rect = WidgetRect::new(inner_x, cy, inner_w, cloud_row_h);
            result.content_items.push((row_id.clone(), row_rect));
            if !is_restoring {
                input_coordinator.register_on_layer(
                    format!("user_settings:{}", row_id).as_str(),
                    row_rect,
                    Sense::CLICK | Sense::HOVER,
                    layer_id,
                );
            }

            cy += cloud_row_h + 6.0;
        }
    }

    // End clip — scrollbar and Create button are drawn outside.
    ctx.restore();

    // ── Scrollbar thumb (drawn outside clip, over the viewport) ───────────────
    if total_content_h > scroll_viewport_h {
        let scrollbar_w = 4.0;
        let track_x = inner_x + inner_w + 2.0;
        let ratio = (scroll_viewport_h / total_content_h).min(1.0);
        let thumb_h = (scroll_viewport_h * ratio).max(24.0);
        let scroll_ratio = if max_scroll > 0.0 { scroll_offset / max_scroll } else { 0.0 };
        let thumb_y = scroll_viewport_y + scroll_ratio * (scroll_viewport_h - thumb_h);
        ctx.set_fill_color("rgba(255,255,255,0.18)");
        ctx.fill_rounded_rect(track_x, thumb_y, scrollbar_w, thumb_h, 2.0);

        // Store rects for input handling (drag support)
        let track_rect = WidgetRect::new(track_x, scroll_viewport_y, scrollbar_w, scroll_viewport_h);
        let handle_rect = WidgetRect::new(track_x, thumb_y, scrollbar_w, thumb_h);
        result.profile_list_handle_rect = Some(handle_rect);
        result.profile_list_track_rect = Some(track_rect);
    }

}

// =============================================================================
// Page: UnlockPassphrase
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page_unlock(
    ctx: &mut dyn RenderContext,
    content_x: f64,
    content_y: f64,
    content_w: f64,
    content_h: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    current_time_ms: u64,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();

    let modal_x = content_x;
    let modal_y = content_y;
    let modal_w = content_w;
    let modal_h = content_h;

    ctx.set_fill_color(&toolbar_theme.background);
    ctx.fill_rect(modal_x, modal_y, modal_w, modal_h);

    input_coordinator.register_on_layer(
        "profile_manager:unlock_bg",
        WidgetRect::new(modal_x, modal_y, modal_w, modal_h),
        Sense::CLICK,
        layer_id,
    );

    let form_w: f64 = 460.0;
    let inner_x = modal_x + (modal_w - form_w) / 2.0;
    let inner_w = form_w;
    let mut cy = modal_y + (modal_h / 2.0) - 130.0;

    // Back button
    render_back_button(ctx, inner_x, &mut cy, toolbar_theme, layer_id, input_coordinator, result, hovered);
    cy += 10.0;

    // Title
    let title = if state.profile_manager_target_name.is_empty() {
        "Unlock Profile".to_string()
    } else {
        format!("Unlock {}", state.profile_manager_target_name)
    };
    ctx.set_font("bold 18px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(title.as_str(), inner_x + inner_w / 2.0, cy);
    cy += 26.0;

    // Subtitle
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.55)");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Enter your passphrase to decrypt", inner_x + inner_w / 2.0, cy);
    ctx.set_text_align(TextAlign::Left);
    cy += 22.0;

    // Passphrase input
    render_passphrase_input(
        ctx, inner_x, inner_w, &mut cy, state, text_color, toolbar_theme,
        frame_theme, current_time_ms, layer_id, input_coordinator, result,
    );

    // Unlock button
    let unlock_disabled = state.e2e_passphrase_editing.text.is_empty();
    let is_unlock_hovered = !unlock_disabled && hovered == Some("profile_mgr:unlock");
    let btn_bg = if unlock_disabled {
        "rgba(244,205,99,0.20)"
    } else if is_unlock_hovered {
        "rgba(255,255,255,0.92)"
    } else {
        toolbar_theme.accent.as_str()
    };
    let btn_text_col = if unlock_disabled { "rgba(0,0,0,0.35)" } else { "rgba(0,0,0,0.85)" };
    let btn_h = 32.0;
    let btn_w = inner_w.min(180.0);
    ctx.set_fill_color(btn_bg);
    ctx.fill_rounded_rect(inner_x, cy, btn_w, btn_h, 4.0);
    ctx.set_font("bold 12px sans-serif");
    ctx.set_fill_color(btn_text_col);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Unlock", inner_x + btn_w / 2.0, cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    if !unlock_disabled {
        let btn_rect = WidgetRect::new(inner_x, cy, btn_w, btn_h);
        result.content_items.push(("profile_mgr:unlock".to_string(), btn_rect));
        input_coordinator.register_on_layer("user_settings:profile_mgr:unlock", btn_rect, Sense::CLICK | Sense::HOVER, layer_id);
    }
    cy += btn_h + 10.0;

    // Error message
    if let Some(ref err_msg) = state.vault_unlock_error {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color("rgba(255,80,80,0.90)");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(err_msg.as_str(), inner_x + inner_w / 2.0, cy);
        ctx.set_text_align(TextAlign::Left);
        cy += 18.0;
    }

    // "Use recovery key" link
    let link_text = "Use recovery key";
    let is_link_hovered = hovered == Some("profile_mgr:use_recovery_key");
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(if is_link_hovered {
        "rgba(244,205,99,0.95)"
    } else {
        "rgba(244,205,99,0.55)"
    });
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(link_text, inner_x + inner_w / 2.0, cy);
    ctx.set_text_align(TextAlign::Left);
    let link_w = 140.0;
    let link_h = 16.0;
    let link_x = inner_x + (inner_w - link_w) / 2.0;
    let link_rect = WidgetRect::new(link_x, cy, link_w, link_h);
    result.content_items.push(("profile_mgr:use_recovery_key".to_string(), link_rect));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:use_recovery_key", link_rect, Sense::CLICK | Sense::HOVER, layer_id,
    );
}

// =============================================================================
// Page: CreatePassphrase
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page_create_passphrase(
    ctx: &mut dyn RenderContext,
    content_x: f64,
    content_y: f64,
    content_w: f64,
    content_h: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    current_time_ms: u64,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();

    let modal_x = content_x;
    let modal_y = content_y;
    let modal_w = content_w;
    let modal_h = content_h;

    ctx.set_fill_color(&toolbar_theme.background);
    ctx.fill_rect(modal_x, modal_y, modal_w, modal_h);

    input_coordinator.register_on_layer(
        "profile_manager:create_pass_bg",
        WidgetRect::new(modal_x, modal_y, modal_w, modal_h),
        Sense::CLICK,
        layer_id,
    );

    let form_w: f64 = 460.0;
    let inner_x = modal_x + (modal_w - form_w) / 2.0;
    let inner_w = form_w;
    let mut cy = modal_y + (modal_h / 2.0) - 110.0;

    // Back button
    render_back_button(ctx, inner_x, &mut cy, toolbar_theme, layer_id, input_coordinator, result, hovered);
    cy += 10.0;

    // Title
    let title = if state.profile_manager_target_name.is_empty() {
        "Set Up Encryption".to_string()
    } else {
        format!("Set Up Encryption for {}", state.profile_manager_target_name)
    };
    ctx.set_font("bold 18px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(title.as_str(), inner_x + inner_w / 2.0, cy);
    cy += 26.0;

    // Subtitle
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.55)");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Create a passphrase to protect your API keys", inner_x + inner_w / 2.0, cy);
    ctx.set_text_align(TextAlign::Left);
    cy += 22.0;

    // Minimum length hint
    if state.e2e_passphrase_editing.text.len() < crate::user_manager::profile_manager::MIN_PASSPHRASE_LENGTH {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color("rgba(254,255,238,0.35)");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(
            &format!("Minimum {} characters", crate::user_manager::profile_manager::MIN_PASSPHRASE_LENGTH),
            inner_x,
            cy,
        );
        cy += 16.0;
    } else {
        cy += 16.0; // keep spacing consistent
    }

    // Passphrase input
    render_passphrase_input(
        ctx, inner_x, inner_w, &mut cy, state, text_color, toolbar_theme,
        frame_theme, current_time_ms, layer_id, input_coordinator, result,
    );

    // Encrypt button (disabled until passphrase meets minimum length)
    let encrypt_disabled = state.e2e_passphrase_editing.text.len() < crate::user_manager::profile_manager::MIN_PASSPHRASE_LENGTH;
    let is_encrypt_hovered = !encrypt_disabled && hovered == Some("profile_mgr:create_passphrase");
    let btn_bg = if encrypt_disabled {
        "rgba(244,205,99,0.20)"
    } else if is_encrypt_hovered {
        "rgba(255,255,255,0.92)"
    } else {
        toolbar_theme.accent.as_str()
    };
    let btn_text_col = if encrypt_disabled { "rgba(0,0,0,0.35)" } else { "rgba(0,0,0,0.85)" };
    let btn_h = 32.0;
    let btn_w = inner_w.min(180.0);
    ctx.set_fill_color(btn_bg);
    ctx.fill_rounded_rect(inner_x, cy, btn_w, btn_h, 4.0);
    ctx.set_font("bold 12px sans-serif");
    ctx.set_fill_color(btn_text_col);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Encrypt", inner_x + btn_w / 2.0, cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    if !encrypt_disabled {
        let btn_rect = WidgetRect::new(inner_x, cy, btn_w, btn_h);
        result.content_items.push(("profile_mgr:create_passphrase".to_string(), btn_rect));
        input_coordinator.register_on_layer(
            "user_settings:profile_mgr:create_passphrase",
            btn_rect,
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
    }
}

// =============================================================================
// Page: ShowRecoveryKey
// =============================================================================

/// Render the recovery key display page.
///
/// Shown once after a successful vault creation.  The user must click
/// "Я записал" ("I have written it down") to proceed.  Until they do,
/// this overlay remains.
#[allow(clippy::too_many_arguments)]
fn render_page_show_recovery_key(
    ctx: &mut dyn RenderContext,
    content_x: f64,
    content_y: f64,
    content_w: f64,
    content_h: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    current_time_ms: u64,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();

    let modal_x = content_x;
    let modal_y = content_y;
    let modal_w = content_w;
    let modal_h = content_h;

    // Modal background
    ctx.set_fill_color(&toolbar_theme.background);
    ctx.fill_rect(modal_x, modal_y, modal_w, modal_h);

    // Absorb clicks on the modal background
    input_coordinator.register_on_layer(
        "profile_manager:recovery_key_bg",
        WidgetRect::new(modal_x, modal_y, modal_w, modal_h),
        Sense::CLICK,
        layer_id,
    );

    let form_w: f64 = 500.0;
    let inner_x = modal_x + (modal_w - form_w) / 2.0;
    let inner_w = form_w;
    let mut cy = modal_y + (modal_h / 2.0) - 140.0;

    // Title
    ctx.set_font("bold 18px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Recovery Key", inner_x + inner_w / 2.0, cy);
    cy += 28.0;

    // Warning subtitle
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color("rgba(244,205,99,0.9)");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(
        "Запишите и сохраните в безопасное место",
        inner_x + inner_w / 2.0,
        cy,
    );
    cy += 18.0;
    ctx.set_fill_color("rgba(254,255,238,0.55)");
    ctx.fill_text(
        "Если вы забудете пароль, этот ключ восстановит доступ",
        inner_x + inner_w / 2.0,
        cy,
    );
    ctx.set_text_align(TextAlign::Left);
    cy += 26.0;

    // Recovery key display box — rendered as a selectable read-only text input so the
    // user can click-to-cursor and drag-to-select the key text for manual copying.
    let key_text = state
        .recovery_key_display
        .as_deref()
        .unwrap_or("(key not available)");

    let key_box_h = 40.0;
    let key_display_rect = WidgetRect::new(inner_x, cy, inner_w, key_box_h);

    // Build a WidgetTheme that matches the gold recovery key color scheme.
    let key_widget_theme = crate::ui::widgets::types::WidgetTheme {
        bg_normal: "rgba(0,0,0,0.35)".to_string(),
        bg_hover: "rgba(0,0,0,0.40)".to_string(),
        bg_pressed: "rgba(0,0,0,0.45)".to_string(),
        bg_disabled: "rgba(0,0,0,0.20)".to_string(),
        text_normal: "rgba(244,205,99,1.0)".to_string(),
        text_hover: "rgba(244,205,99,1.0)".to_string(),
        text_disabled: "rgba(244,205,99,0.5)".to_string(),
        border_normal: "rgba(244,205,99,0.35)".to_string(),
        border_hover: "rgba(244,205,99,0.55)".to_string(),
        border_focused: "rgba(244,205,99,0.70)".to_string(),
        accent: "rgba(244,205,99,0.30)".to_string(),
        accent_hover: "rgba(244,205,99,0.40)".to_string(),
        success: "#26a69a".to_string(),
        warning: "#ff9800".to_string(),
        danger: "#ef5350".to_string(),
    };

    let editing = &state.recovery_key_display_editing;
    let (sel_start, sel_end) = if let Some((lo, hi)) = editing.selection_range() {
        (Some(lo), Some(hi))
    } else {
        (None, None)
    };
    let display_focused = state.recovery_key_display_focused;
    let key_display_config = InputConfig::new(key_text)
        .with_focused(display_focused)
        .with_cursor(editing.cursor)
        .with_type(InputType::Text)
        .with_font_size(13.0)
        .with_padding(10.0)
        .with_radius(4.0)
        .with_selection(sel_start, sel_end);

    let key_display_result = draw_input(ctx, &key_display_config, crate::ui::widgets::types::WidgetState::Normal, key_display_rect, &key_widget_theme);

    // Register the display box for click events (focus, cursor positioning) and
    // hover so the mouse cursor changes.
    result.content_items.push(("profile_mgr:recovery_key_display".to_string(), key_display_rect));
    result.input_char_positions.push(("profile_mgr:recovery_key_display".to_string(), key_display_result.char_x_positions.clone()));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:recovery_key_display",
        key_display_rect,
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );

    // Draw blinking cursor if the display is focused.
    let _ = frame_theme; // frame_theme reserved for future use (e.g. accent color)
    if display_focused && editing.is_cursor_visible(current_time_ms) {
        draw_input_cursor(
            ctx,
            key_display_result.cursor_x,
            key_display_result.cursor_y,
            key_display_result.cursor_height,
            "rgba(244,205,99,0.9)",
        );
    }

    cy += key_box_h + 10.0;

    // Copy button: copies the recovery key to clipboard
    let copy_btn_label = "Скопировать ключ";
    let is_copy_hovered = hovered == Some("profile_mgr:recovery_key_copy");
    let copy_btn_bg = if is_copy_hovered {
        "rgba(244,205,99,0.2)"
    } else {
        "rgba(244,205,99,0.08)"
    };
    let copy_btn_h = 28.0;
    let copy_btn_w = inner_w.min(200.0);
    let copy_btn_x = inner_x + (inner_w - copy_btn_w) / 2.0;
    ctx.set_fill_color(copy_btn_bg);
    ctx.fill_rounded_rect(copy_btn_x, cy, copy_btn_w, copy_btn_h, 4.0);
    ctx.set_stroke_color("rgba(244,205,99,0.4)");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(copy_btn_x, cy, copy_btn_w, copy_btn_h, 4.0);
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color("rgba(244,205,99,0.9)");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(copy_btn_label, copy_btn_x + copy_btn_w / 2.0, cy + copy_btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    let copy_btn_rect = WidgetRect::new(copy_btn_x, cy, copy_btn_w, copy_btn_h);
    result.content_items.push(("profile_mgr:recovery_key_copy".to_string(), copy_btn_rect));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:recovery_key_copy",
        copy_btn_rect,
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );

    cy += copy_btn_h + 12.0;

    // Confirm button: "Я записал"
    let btn_label = "Я записал — продолжить";
    let is_btn_hovered = hovered == Some("profile_mgr:recovery_key_confirm");
    let btn_bg = if is_btn_hovered {
        "rgba(255,255,255,0.92)"
    } else {
        toolbar_theme.accent.as_str()
    };
    let btn_text_col = "rgba(0,0,0,0.85)";
    let btn_h = 34.0;
    let btn_w = inner_w.min(260.0);
    let btn_x = inner_x + (inner_w - btn_w) / 2.0;
    ctx.set_fill_color(btn_bg);
    ctx.fill_rounded_rect(btn_x, cy, btn_w, btn_h, 4.0);
    ctx.set_font("bold 12px sans-serif");
    ctx.set_fill_color(btn_text_col);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(btn_label, btn_x + btn_w / 2.0, cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    let btn_rect = WidgetRect::new(btn_x, cy, btn_w, btn_h);
    result.content_items.push(("profile_mgr:recovery_key_confirm".to_string(), btn_rect));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:recovery_key_confirm",
        btn_rect,
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );
}


// =============================================================================
// Page: CreateNew
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page_create_new(
    ctx: &mut dyn RenderContext,
    content_x: f64,
    content_y: f64,
    content_w: f64,
    content_h: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    current_time_ms: u64,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();

    let modal_x = content_x;
    let modal_y = content_y;
    let modal_w = content_w;
    let modal_h = content_h;

    ctx.set_fill_color(&toolbar_theme.background);
    ctx.fill_rect(modal_x, modal_y, modal_w, modal_h);

    input_coordinator.register_on_layer(
        "profile_manager:create_new_bg",
        WidgetRect::new(modal_x, modal_y, modal_w, modal_h),
        Sense::CLICK,
        layer_id,
    );

    let form_w: f64 = 460.0;
    let inner_x = modal_x + (modal_w - form_w) / 2.0;
    let inner_w = form_w;
    let mut cy = modal_y + (modal_h / 2.0) - 100.0;

    // Back button
    render_back_button(ctx, inner_x, &mut cy, toolbar_theme, layer_id, input_coordinator, result, hovered);
    cy += 10.0;

    // Title
    ctx.set_font("bold 18px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("New Profile", inner_x + inner_w / 2.0, cy);
    ctx.set_text_align(TextAlign::Left);
    cy += 28.0;

    // Profile name label
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Profile Name", inner_x, cy);
    cy += 18.0;

    // Profile name input
    let input_h = 32.0;
    let name_rect = WidgetRect::new(inner_x, cy, inner_w, input_h);
    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let editing = &state.new_profile_name_editing;
    let (sel_start, sel_end) = if let Some((lo, hi)) = editing.selection_range() {
        (Some(lo), Some(hi))
    } else {
        (None, None)
    };
    let name_config = InputConfig::new(&editing.text)
        .with_focused(state.new_profile_name_focused)
        .with_cursor(editing.cursor)
        .with_placeholder("Profile name\u{2026}")
        .with_type(InputType::Text)
        .with_selection(sel_start, sel_end);
    let name_result = draw_input(ctx, &name_config, WidgetState::Normal, name_rect, &widget_theme);

    result.content_items.push(("profile_mgr:name_input".to_string(), name_rect));
    result.input_char_positions.push(("profile_mgr:name_input".to_string(), name_result.char_x_positions.clone()));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:name_input",
        name_rect,
        Sense::CLICK,
        layer_id,
    );

    if state.new_profile_name_focused && editing.is_cursor_visible(current_time_ms) {
        draw_input_cursor(
            ctx,
            name_result.cursor_x,
            name_result.cursor_y,
            name_result.cursor_height,
            &toolbar_theme.item_text,
        );
    }
    cy += input_h + 16.0;

    // Create button
    let name_is_empty = state.new_profile_name_editing.text.trim().is_empty();
    let create_disabled = name_is_empty;
    let is_create_hovered = !create_disabled && hovered == Some("profile_mgr:create_confirm");
    let create_bg = if create_disabled {
        "rgba(244,205,99,0.20)"
    } else if is_create_hovered {
        "rgba(255,255,255,0.92)"
    } else {
        toolbar_theme.accent.as_str()
    };
    let create_text_col = if create_disabled { "rgba(0,0,0,0.35)" } else { "rgba(0,0,0,0.85)" };
    let create_btn_h = 32.0;
    let create_btn_w = inner_w.min(180.0);
    ctx.set_fill_color(create_bg);
    ctx.fill_rounded_rect(inner_x, cy, create_btn_w, create_btn_h, 4.0);
    ctx.set_font("bold 12px sans-serif");
    ctx.set_fill_color(create_text_col);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Create", inner_x + create_btn_w / 2.0, cy + create_btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    if !create_disabled {
        let create_rect = WidgetRect::new(inner_x, cy, create_btn_w, create_btn_h);
        result.content_items.push(("profile_mgr:create_confirm".to_string(), create_rect));
        input_coordinator.register_on_layer(
            "user_settings:profile_mgr:create_confirm",
            create_rect,
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
    }
}

// =============================================================================
// Page: SetNewPassphrase
// =============================================================================

/// Render the "Set New Passphrase" page.
///
/// Shown after a successful recovery key unlock. The user MUST set a new
/// passphrase before the vault re-key completes and the app becomes usable.
/// There is no back button — this step is mandatory.
#[allow(clippy::too_many_arguments)]
fn render_page_set_new_passphrase(
    ctx: &mut dyn RenderContext,
    content_x: f64,
    content_y: f64,
    content_w: f64,
    content_h: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    current_time_ms: u64,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();
    let has_error = !state.set_passphrase_error.is_empty();

    let modal_x = content_x;
    let modal_y = content_y;
    let modal_w = content_w;
    let modal_h = content_h;

    ctx.set_fill_color(&toolbar_theme.background);
    ctx.fill_rect(modal_x, modal_y, modal_w, modal_h);

    // Absorb clicks on the modal background
    input_coordinator.register_on_layer(
        "profile_manager:set_new_pass_bg",
        WidgetRect::new(modal_x, modal_y, modal_w, modal_h),
        Sense::CLICK,
        layer_id,
    );

    let form_w: f64 = 480.0;
    let inner_x = modal_x + (modal_w - form_w) / 2.0;
    let inner_w = form_w;
    let mut cy = modal_y + (modal_h / 2.0) - 150.0;

    // Title
    ctx.set_font("bold 18px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Set New Passphrase", inner_x + inner_w / 2.0, cy);
    cy += 28.0;

    // Subtitle
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.55)");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(
        "Your vault was unlocked with recovery key.",
        inner_x + inner_w / 2.0,
        cy,
    );
    cy += 16.0;
    ctx.fill_text(
        "Set a new passphrase to continue.",
        inner_x + inner_w / 2.0,
        cy,
    );
    ctx.set_text_align(TextAlign::Left);
    cy += 26.0;

    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let input_h = 32.0;

    // ── New Passphrase ────────────────────────────────────────────────────────
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("New Passphrase", inner_x, cy);
    cy += 18.0;

    let new_pass_rect = WidgetRect::new(inner_x, cy, inner_w, input_h);
    let editing = &state.new_passphrase_editing;
    let (sel_start, sel_end) = if let Some((lo, hi)) = editing.selection_range() {
        (Some(lo), Some(hi))
    } else {
        (None, None)
    };
    let new_pass_config = InputConfig::new(&editing.text)
        .with_focused(state.new_passphrase_focused)
        .with_cursor(editing.cursor)
        .with_placeholder("New passphrase\u{2026}")
        .with_type(InputType::Password)
        .with_selection(sel_start, sel_end);
    let new_pass_result = draw_input(ctx, &new_pass_config, WidgetState::Normal, new_pass_rect, &widget_theme);

    result.content_items.push(("profile_mgr:new_passphrase_input".to_string(), new_pass_rect));
    result.input_char_positions.push(("profile_mgr:new_passphrase_input".to_string(), new_pass_result.char_x_positions.clone()));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:new_passphrase_input",
        new_pass_rect,
        Sense::CLICK,
        layer_id,
    );

    if state.new_passphrase_focused && editing.is_cursor_visible(current_time_ms) {
        draw_input_cursor(
            ctx,
            new_pass_result.cursor_x,
            new_pass_result.cursor_y,
            new_pass_result.cursor_height,
            &toolbar_theme.item_text,
        );
    }
    cy += input_h + 14.0;

    // ── Confirm Passphrase ────────────────────────────────────────────────────
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Confirm Passphrase", inner_x, cy);
    cy += 18.0;

    let confirm_pass_rect = WidgetRect::new(inner_x, cy, inner_w, input_h);
    let editing2 = &state.confirm_passphrase_editing;
    let (sel_start2, sel_end2) = if let Some((lo, hi)) = editing2.selection_range() {
        (Some(lo), Some(hi))
    } else {
        (None, None)
    };
    let confirm_pass_config = InputConfig::new(&editing2.text)
        .with_focused(state.confirm_passphrase_focused)
        .with_cursor(editing2.cursor)
        .with_placeholder("Confirm passphrase\u{2026}")
        .with_type(InputType::Password)
        .with_selection(sel_start2, sel_end2);
    let confirm_pass_result = draw_input(ctx, &confirm_pass_config, WidgetState::Normal, confirm_pass_rect, &widget_theme);

    result.content_items.push(("profile_mgr:confirm_passphrase_input".to_string(), confirm_pass_rect));
    result.input_char_positions.push(("profile_mgr:confirm_passphrase_input".to_string(), confirm_pass_result.char_x_positions.clone()));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:confirm_passphrase_input",
        confirm_pass_rect,
        Sense::CLICK,
        layer_id,
    );

    if state.confirm_passphrase_focused && editing2.is_cursor_visible(current_time_ms) {
        draw_input_cursor(
            ctx,
            confirm_pass_result.cursor_x,
            confirm_pass_result.cursor_y,
            confirm_pass_result.cursor_height,
            &toolbar_theme.item_text,
        );
    }
    cy += input_h + 16.0;

    // ── Error message ─────────────────────────────────────────────────────────
    if has_error {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color("rgba(255,80,80,0.90)");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(state.set_passphrase_error.as_str(), inner_x + inner_w / 2.0, cy);
        ctx.set_text_align(TextAlign::Left);
        cy += 22.0;
    }

    // ── Save button ───────────────────────────────────────────────────────────
    let new_text = &state.new_passphrase_editing.text;
    let confirm_text = &state.confirm_passphrase_editing.text;
    let save_disabled = new_text.len() < crate::user_manager::profile_manager::MIN_PASSPHRASE_LENGTH
        || confirm_text.len() < crate::user_manager::profile_manager::MIN_PASSPHRASE_LENGTH
        || new_text != confirm_text;

    let is_save_hovered = !save_disabled && hovered == Some("profile_mgr:save_new_passphrase");
    let save_bg = if save_disabled {
        "rgba(244,205,99,0.20)"
    } else if is_save_hovered {
        "rgba(255,255,255,0.92)"
    } else {
        toolbar_theme.accent.as_str()
    };
    let save_text_col = if save_disabled { "rgba(0,0,0,0.35)" } else { "rgba(0,0,0,0.85)" };
    let btn_h = 32.0;
    let btn_w = inner_w.min(180.0);
    ctx.set_fill_color(save_bg);
    ctx.fill_rounded_rect(inner_x, cy, btn_w, btn_h, 4.0);
    ctx.set_font("bold 12px sans-serif");
    ctx.set_fill_color(save_text_col);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Save", inner_x + btn_w / 2.0, cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    if !save_disabled {
        let btn_rect = WidgetRect::new(inner_x, cy, btn_w, btn_h);
        result.content_items.push(("profile_mgr:save_new_passphrase".to_string(), btn_rect));
        input_coordinator.register_on_layer(
            "user_settings:profile_mgr:save_new_passphrase",
            btn_rect,
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
    }
}

// =============================================================================
// Page: UseRecoveryKey
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page_use_recovery_key(
    ctx: &mut dyn RenderContext,
    content_x: f64,
    content_y: f64,
    content_w: f64,
    content_h: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    current_time_ms: u64,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();

    let modal_x = content_x;
    let modal_y = content_y;
    let modal_w = content_w;
    let modal_h = content_h;

    ctx.set_fill_color(&toolbar_theme.background);
    ctx.fill_rect(modal_x, modal_y, modal_w, modal_h);

    input_coordinator.register_on_layer(
        "profile_manager:recovery_input_bg",
        WidgetRect::new(modal_x, modal_y, modal_w, modal_h),
        Sense::CLICK,
        layer_id,
    );

    let form_w: f64 = 520.0;
    let inner_x = modal_x + (modal_w - form_w) / 2.0;
    let inner_w = form_w;
    let mut cy = modal_y + (modal_h / 2.0) - 130.0;

    // Back button
    render_back_button(ctx, inner_x, &mut cy, toolbar_theme, layer_id, input_coordinator, result, hovered);
    cy += 10.0;

    // Title
    ctx.set_font("bold 18px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Recover with Recovery Key", inner_x + inner_w / 2.0, cy);
    cy += 26.0;

    // Subtitle
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.55)");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Enter the recovery key shown during vault setup", inner_x + inner_w / 2.0, cy);
    ctx.set_text_align(TextAlign::Left);
    cy += 22.0;

    // Recovery key input (plain text, not password)
    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let input_h = 32.0;
    let input_rect = WidgetRect::new(inner_x, cy, inner_w, input_h);
    let editing = &state.recovery_key_editing;
    let (sel_start, sel_end) = if let Some((lo, hi)) = editing.selection_range() {
        (Some(lo), Some(hi))
    } else {
        (None, None)
    };
    let input_config = InputConfig::new(&editing.text)
        .with_focused(state.recovery_key_focused)
        .with_cursor(editing.cursor)
        .with_placeholder("xxxx-xxxx-xxxx-xxxx-xxxx-xxxx-xxxx-xxxx-\u{2026}")
        .with_type(InputType::Text)
        .with_selection(sel_start, sel_end);

    let input_result = draw_input(ctx, &input_config, WidgetState::Normal, input_rect, &widget_theme);

    result.content_items.push(("profile_mgr:recovery_key_input".to_string(), input_rect));
    result.input_char_positions.push(("profile_mgr:recovery_key_input".to_string(), input_result.char_x_positions.clone()));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:recovery_key_input",
        input_rect,
        Sense::CLICK,
        layer_id,
    );

    if state.recovery_key_focused && editing.is_cursor_visible(current_time_ms) {
        draw_input_cursor(
            ctx,
            input_result.cursor_x,
            input_result.cursor_y,
            input_result.cursor_height,
            &toolbar_theme.item_text,
        );
    }
    cy += input_h + 14.0;

    // Recover button
    let recover_disabled = state.recovery_key_editing.text.len() < 40;
    let is_recover_hovered = !recover_disabled && hovered == Some("profile_mgr:recovery_unlock");
    let btn_bg = if recover_disabled {
        "rgba(244,205,99,0.20)"
    } else if is_recover_hovered {
        "rgba(255,255,255,0.92)"
    } else {
        toolbar_theme.accent.as_str()
    };
    let btn_text_col = if recover_disabled { "rgba(0,0,0,0.35)" } else { "rgba(0,0,0,0.85)" };
    let btn_h = 32.0;
    let btn_w = inner_w.min(180.0);
    ctx.set_fill_color(btn_bg);
    ctx.fill_rounded_rect(inner_x, cy, btn_w, btn_h, 4.0);
    ctx.set_font("bold 12px sans-serif");
    ctx.set_fill_color(btn_text_col);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Recover", inner_x + btn_w / 2.0, cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    if !recover_disabled {
        let btn_rect = WidgetRect::new(inner_x, cy, btn_w, btn_h);
        result.content_items.push(("profile_mgr:recovery_unlock".to_string(), btn_rect));
        input_coordinator.register_on_layer(
            "user_settings:profile_mgr:recovery_unlock", btn_rect, Sense::CLICK | Sense::HOVER, layer_id,
        );
    }
    cy += btn_h + 10.0;

    // Error message
    if let Some(ref err_msg) = state.vault_unlock_error {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color("rgba(255,80,80,0.90)");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(err_msg.as_str(), inner_x + inner_w / 2.0, cy);
        ctx.set_text_align(TextAlign::Left);
    }
}

// =============================================================================
// Shared helpers
// =============================================================================

/// Render a "← Back to profiles" button. Advances `cy` by button height.
fn render_back_button(
    ctx: &mut dyn RenderContext,
    x: f64,
    cy: &mut f64,
    toolbar_theme: &ToolbarTheme,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
    hovered_item_id: Option<&str>,
) {
    let btn_w = 120.0;
    let btn_h = 24.0;
    let is_hovered = hovered_item_id == Some("profile_mgr:back");
    let btn_bg = if is_hovered { "rgba(255,255,255,0.12)" } else { "rgba(255,255,255,0.06)" };
    ctx.set_fill_color(btn_bg);
    ctx.fill_rounded_rect(x, *cy, btn_w, btn_h, 4.0);
    let stroke_color = if is_hovered { "rgba(254,255,238,0.40)" } else { "rgba(254,255,238,0.15)" };
    ctx.set_stroke_color(stroke_color);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, *cy, btn_w, btn_h, 4.0);
    // Draw chevron-left icon (small, 14x14) at left of button
    draw_svg_icon(ctx, Icon::ChevronLeft.svg(), x + 4.0, *cy + (btn_h - 14.0) / 2.0, 14.0, 14.0, &toolbar_theme.item_text);
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Back to profiles", x + 22.0, *cy + btn_h / 2.0);

    let back_rect = WidgetRect::new(x, *cy, btn_w, btn_h);
    result.content_items.push(("profile_mgr:back".to_string(), back_rect));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:back",
        back_rect,
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );
    *cy += btn_h;
}

/// Render the passphrase input box. Advances `cy` past the input field.
#[allow(clippy::too_many_arguments)]
fn render_passphrase_input(
    ctx: &mut dyn RenderContext,
    x: f64,
    w: f64,
    cy: &mut f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    current_time_ms: u64,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    // Label
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Passphrase", x, *cy);
    *cy += 18.0;

    // Input box
    let input_h = 32.0;
    let input_rect = WidgetRect::new(x, *cy, w, input_h);
    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let editing = &state.e2e_passphrase_editing;
    let (sel_start, sel_end) = if let Some((lo, hi)) = editing.selection_range() {
        (Some(lo), Some(hi))
    } else {
        (None, None)
    };
    let input_config = InputConfig::new(&editing.text)
        .with_focused(state.e2e_passphrase_focused)
        .with_cursor(editing.cursor)
        .with_placeholder("Click to type passphrase\u{2026}")
        .with_type(InputType::Password)
        .with_selection(sel_start, sel_end);

    let input_result = draw_input(ctx, &input_config, WidgetState::Normal, input_rect, &widget_theme);

    result.content_items.push(("e2e_passphrase_input".to_string(), input_rect));
    result.input_char_positions.push(("e2e_passphrase_input".to_string(), input_result.char_x_positions.clone()));
    input_coordinator.register_on_layer(
        "user_settings:e2e_passphrase_input",
        input_rect,
        Sense::CLICK,
        layer_id,
    );

    if state.e2e_passphrase_focused && editing.is_cursor_visible(current_time_ms) {
        draw_input_cursor(
            ctx,
            input_result.cursor_x,
            input_result.cursor_y,
            input_result.cursor_height,
            &toolbar_theme.item_text,
        );
    }

    *cy += input_h + 16.0;
}
