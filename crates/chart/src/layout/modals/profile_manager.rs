//! Unified Profile Manager overlay.
//!
//! A single full-screen modal that replaces the old vault_unlock overlay and
//! vault_profile_picker with a coherent profile management flow.
//!
//! Pages:
//!   ProfileList       — List all profiles; select one to load or create a new one.
//!   UnlockPassphrase  — Enter passphrase to unlock a profile with vault.enc.
//!   CreatePassphrase  — Set a new passphrase for a profile that has no vault.
//!   CreateNew         — Enter name + mode for a brand-new profile.

use crate::engine::render::RenderContext;
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
/// This is a full-screen, non-closeable overlay that handles all profile-related
/// flows: listing profiles, unlocking encrypted profiles, setting up passphrase
/// for new profiles, and creating new profiles.
#[allow(clippy::too_many_arguments)]
pub fn render_profile_manager(
    ctx: &mut dyn RenderContext,
    window_w: f64,
    window_h: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    // ── Full-screen dimmer ────────────────────────────────────────────────────
    ctx.set_fill_color("rgba(0,0,0,0.72)");
    ctx.fill_rect(0.0, 0.0, window_w, window_h);

    // Push a high-z modal layer so the profile manager absorbs all input
    let layer_id = ZLayer::ModalOverlay.push_named(input_coordinator, "profile_manager");

    // Block all clicks on the dimmer (profile manager is non-closeable)
    input_coordinator.register_on_layer(
        "profile_manager:dimmer",
        WidgetRect::new(0.0, 0.0, window_w, window_h),
        Sense::CLICK,
        &layer_id,
    );

    match &state.profile_manager_page {
        ProfileManagerPage::ProfileList => render_page_profile_list(
            ctx, window_w, window_h, state, text_color, toolbar_theme,
            &layer_id, input_coordinator, result,
        ),
        ProfileManagerPage::UnlockPassphrase => render_page_unlock(
            ctx, window_w, window_h, state, text_color, toolbar_theme, frame_theme,
            current_time_ms, &layer_id, input_coordinator, result,
        ),
        ProfileManagerPage::CreatePassphrase => render_page_create_passphrase(
            ctx, window_w, window_h, state, text_color, toolbar_theme, frame_theme,
            current_time_ms, &layer_id, input_coordinator, result,
        ),
        ProfileManagerPage::CreateNew => render_page_create_new(
            ctx, window_w, window_h, state, text_color, toolbar_theme, frame_theme,
            current_time_ms, &layer_id, input_coordinator, result,
        ),
    }
}

// =============================================================================
// Page: ProfileList
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page_profile_list(
    ctx: &mut dyn RenderContext,
    window_w: f64,
    window_h: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();

    // ── Modal geometry ────────────────────────────────────────────────────────
    let modal_w: f64 = 500.0;
    let profile_row_h: f64 = 52.0;
    let n = state.profiles_with_vault_status.len();
    let calculated_h: f64 = 30.0       // top pad
        + 28.0                          // title
        + 20.0                          // subtitle
        + 16.0                          // gap
        + n as f64 * (profile_row_h + 6.0) // rows + gaps
        + 12.0                          // gap before button
        + 36.0                          // create button
        + 24.0;                         // bottom pad
    let modal_h = calculated_h.clamp(160.0, window_h - 80.0);
    let modal_x = (window_w - modal_w) / 2.0;
    let modal_y = (window_h - modal_h) / 2.0;

    // Modal background + border
    ctx.set_fill_color("rgba(24,26,32,0.98)");
    ctx.fill_rounded_rect(modal_x, modal_y, modal_w, modal_h, 8.0);
    ctx.set_stroke_color("rgba(244,205,99,0.25)");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(modal_x, modal_y, modal_w, modal_h, 8.0);

    // Absorb modal background clicks
    input_coordinator.register_on_layer(
        "profile_manager:modal_bg",
        WidgetRect::new(modal_x, modal_y, modal_w, modal_h),
        Sense::CLICK,
        layer_id,
    );

    let padding = 28.0;
    let inner_x = modal_x + padding;
    let inner_w = modal_w - padding * 2.0;
    let mut cy = modal_y + 30.0;

    // Title
    ctx.set_font("bold 20px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Profiles", inner_x + inner_w / 2.0, cy);
    cy += 28.0;

    // Subtitle
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.50)");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Select a profile to load", inner_x + inner_w / 2.0, cy);
    ctx.set_text_align(TextAlign::Left);
    cy += 20.0 + 16.0;

    // Profile rows
    for (id, display_name, _avatar, client_mode, has_vault) in &state.profiles_with_vault_status {
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

        // Right-side badges
        // Vault status badge
        let vault_label = if *has_vault { "Encrypted" } else { "No encryption" };
        let vault_color = if *has_vault {
            "rgba(80,200,120,0.7)"
        } else {
            "rgba(255,180,80,0.7)"
        };
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(vault_color);
        ctx.set_text_align(TextAlign::Right);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(vault_label, inner_x + inner_w - 8.0, row_mid_y);

        // Client mode badge (slightly left of vault badge)
        let mode_label = match client_mode {
            crate::user_profile::profile::ClientMode::Connected => "Cloud",
            crate::user_profile::profile::ClientMode::Standalone => "Offline",
        };
        ctx.set_fill_color("rgba(254,255,238,0.30)");
        let vault_label_approx_w = if *has_vault { 62.0 } else { 90.0 };
        ctx.fill_text(mode_label, inner_x + inner_w - 8.0 - vault_label_approx_w - 8.0, row_mid_y);
        ctx.set_text_align(TextAlign::Left);

        // Register hit area
        let row_rect = WidgetRect::new(inner_x, cy, inner_w, profile_row_h);
        result.content_items.push((widget_id.clone(), row_rect));
        let hit_id = format!("user_settings:{}",widget_id);
        input_coordinator.register_on_layer(hit_id.as_str(), row_rect, Sense::CLICK, layer_id);

        cy += profile_row_h + 6.0;
    }

    cy += 12.0;

    // "Create New Profile" button
    let create_btn_h = 36.0;
    let is_create_hovered = hovered == Some("profile_mgr:create_new");
    let create_bg = if is_create_hovered {
        "rgba(255,255,255,0.92)"
    } else {
        toolbar_theme.accent.as_str()
    };
    ctx.set_fill_color(create_bg);
    ctx.fill_rounded_rect(inner_x, cy, inner_w, create_btn_h, 4.0);
    ctx.set_font("bold 12px sans-serif");
    ctx.set_fill_color("rgba(0,0,0,0.85)");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Create New Profile", inner_x + inner_w / 2.0, cy + create_btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    let create_rect = WidgetRect::new(inner_x, cy, inner_w, create_btn_h);
    let create_id = "profile_mgr:create_new";
    result.content_items.push((create_id.to_string(), create_rect));
    input_coordinator.register_on_layer(
        format!("user_settings:{}", create_id).as_str(),
        create_rect,
        Sense::CLICK,
        layer_id,
    );
}

// =============================================================================
// Page: UnlockPassphrase
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page_unlock(
    ctx: &mut dyn RenderContext,
    window_w: f64,
    window_h: f64,
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

    let modal_w: f64 = 460.0;
    let modal_h: f64 = if state.vault_unlock_error.is_some() { 310.0 } else { 280.0 };
    let modal_x = (window_w - modal_w) / 2.0;
    let modal_y = (window_h - modal_h) / 2.0;

    ctx.set_fill_color("rgba(24,26,32,0.98)");
    ctx.fill_rounded_rect(modal_x, modal_y, modal_w, modal_h, 8.0);
    ctx.set_stroke_color("rgba(244,205,99,0.25)");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(modal_x, modal_y, modal_w, modal_h, 8.0);

    input_coordinator.register_on_layer(
        "profile_manager:unlock_bg",
        WidgetRect::new(modal_x, modal_y, modal_w, modal_h),
        Sense::CLICK,
        layer_id,
    );

    let padding = 28.0;
    let inner_x = modal_x + padding;
    let inner_w = modal_w - padding * 2.0;
    let mut cy = modal_y + 20.0;

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
        input_coordinator.register_on_layer("user_settings:profile_mgr:unlock", btn_rect, Sense::CLICK, layer_id);
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
// Page: CreatePassphrase
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page_create_passphrase(
    ctx: &mut dyn RenderContext,
    window_w: f64,
    window_h: f64,
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

    let modal_w: f64 = 460.0;
    let modal_h: f64 = 320.0;
    let modal_x = (window_w - modal_w) / 2.0;
    let modal_y = (window_h - modal_h) / 2.0;

    ctx.set_fill_color("rgba(24,26,32,0.98)");
    ctx.fill_rounded_rect(modal_x, modal_y, modal_w, modal_h, 8.0);
    ctx.set_stroke_color("rgba(244,205,99,0.25)");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(modal_x, modal_y, modal_w, modal_h, 8.0);

    input_coordinator.register_on_layer(
        "profile_manager:create_pass_bg",
        WidgetRect::new(modal_x, modal_y, modal_w, modal_h),
        Sense::CLICK,
        layer_id,
    );

    let padding = 28.0;
    let inner_x = modal_x + padding;
    let inner_w = modal_w - padding * 2.0;
    let mut cy = modal_y + 20.0;

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

    // Passphrase input
    render_passphrase_input(
        ctx, inner_x, inner_w, &mut cy, state, text_color, toolbar_theme,
        frame_theme, current_time_ms, layer_id, input_coordinator, result,
    );

    // Encrypt button
    let encrypt_disabled = state.e2e_passphrase_editing.text.is_empty();
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
            Sense::CLICK,
            layer_id,
        );
    }
    cy += btn_h + 16.0;

    // Skip link
    let is_skip_hovered = hovered == Some("profile_mgr:skip_encryption");
    let skip_color = if is_skip_hovered {
        "rgba(254,255,238,0.60)"
    } else {
        "rgba(254,255,238,0.32)"
    };
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(skip_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Skip \u{2014} leave unencrypted", inner_x + inner_w / 2.0, cy);
    ctx.set_text_align(TextAlign::Left);

    let skip_w = 180.0;
    let skip_h = 16.0;
    let skip_rect = WidgetRect::new(inner_x + (inner_w - skip_w) / 2.0, cy, skip_w, skip_h);
    result.content_items.push(("profile_mgr:skip_encryption".to_string(), skip_rect));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:skip_encryption",
        skip_rect,
        Sense::CLICK,
        layer_id,
    );
}

// =============================================================================
// Page: CreateNew
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page_create_new(
    ctx: &mut dyn RenderContext,
    window_w: f64,
    window_h: f64,
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

    let modal_w: f64 = 460.0;
    let modal_h: f64 = 340.0;
    let modal_x = (window_w - modal_w) / 2.0;
    let modal_y = (window_h - modal_h) / 2.0;

    ctx.set_fill_color("rgba(24,26,32,0.98)");
    ctx.fill_rounded_rect(modal_x, modal_y, modal_w, modal_h, 8.0);
    ctx.set_stroke_color("rgba(244,205,99,0.25)");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(modal_x, modal_y, modal_w, modal_h, 8.0);

    input_coordinator.register_on_layer(
        "profile_manager:create_new_bg",
        WidgetRect::new(modal_x, modal_y, modal_w, modal_h),
        Sense::CLICK,
        layer_id,
    );

    let padding = 28.0;
    let inner_x = modal_x + padding;
    let inner_w = modal_w - padding * 2.0;
    let mut cy = modal_y + 20.0;

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

    // Mode selection label
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Mode", inner_x, cy);
    cy += 18.0;

    // Mode rows
    let mode_row_h = 36.0;
    let mode_rows = [
        ("profile_mgr:mode_standalone", "Standalone (Offline)", state.new_profile_standalone),
        ("profile_mgr:mode_connected", "Connected (Cloud Sync)", !state.new_profile_standalone),
    ];

    for (mode_id, mode_label, is_selected) in &mode_rows {
        let is_hovered = hovered == Some(*mode_id);
        let row_bg = if *is_selected {
            "rgba(255,255,255,0.08)"
        } else if is_hovered {
            "rgba(255,255,255,0.05)"
        } else {
            "rgba(255,255,255,0.03)"
        };
        ctx.set_fill_color(row_bg);
        ctx.fill_rounded_rect(inner_x, cy, inner_w, mode_row_h, 4.0);

        if *is_selected {
            ctx.set_stroke_color("rgba(244,205,99,0.35)");
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(inner_x, cy, inner_w, mode_row_h, 4.0);
        }

        let row_mid_y = cy + mode_row_h / 2.0;

        // Radio circle indicator
        let indicator = if *is_selected { "\u{25CF}" } else { "\u{25CB}" }; // ● or ○
        let ind_color = if *is_selected { toolbar_theme.accent.as_str() } else { "rgba(254,255,238,0.40)" };
        ctx.set_font("14px sans-serif");
        ctx.set_fill_color(ind_color);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(indicator, inner_x + 10.0, row_mid_y);

        // Mode label
        let label_color = if *is_selected { text_color } else { "rgba(254,255,238,0.65)" };
        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(label_color);
        ctx.fill_text(*mode_label, inner_x + 30.0, row_mid_y);

        let mode_rect = WidgetRect::new(inner_x, cy, inner_w, mode_row_h);
        result.content_items.push((mode_id.to_string(), mode_rect));
        let hit_id = format!("user_settings:{}",mode_id);
        input_coordinator.register_on_layer(hit_id.as_str(), mode_rect, Sense::CLICK, layer_id);

        cy += mode_row_h + 6.0;
    }

    cy += 12.0;

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
            Sense::CLICK,
            layer_id,
        );
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
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("\u{2190} Back to profiles", x + btn_w / 2.0, *cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    let back_rect = WidgetRect::new(x, *cy, btn_w, btn_h);
    result.content_items.push(("profile_mgr:back".to_string(), back_rect));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:back",
        back_rect,
        Sense::CLICK,
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
