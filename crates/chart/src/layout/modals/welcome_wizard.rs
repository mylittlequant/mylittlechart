//! First-run Welcome Wizard modal.
//!
//! Shown when the user launches the app for the first time (no `profile.json`
//! found on disk).  The wizard is a full-screen dimmer + centered modal that
//! cannot be dismissed except by completing the setup flow.
//!
//! Pages:
//!   0 — Welcome          (Get Started button)
//!   1 — Set Passphrase   (mandatory; Connected mode also shows sign-in section)

use crate::engine::render::RenderContext;
use uzor::render::{TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;
use uzor::input::sense::Sense;
use crate::ui::modal_settings::UserSettingsState;
use crate::ui::toolbar_render::ToolbarTheme;
use crate::layout::render_frame::UserSettingsResult;
use crate::ui::z_order::ZLayer;
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig, InputType};
use crate::ui::widgets::types::WidgetState;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::layout::render_chart::FrameTheme;

/// Render the Welcome Wizard overlay.
///
/// This is rendered independently of the settings modal — it replaces the
/// entire UI with a full-screen dimmer + centered modal until the user makes
/// a mode choice.
#[allow(clippy::too_many_arguments)]
pub fn render_welcome_wizard(
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

    // Push a high-z modal layer so the wizard absorbs all input
    let layer_id = ZLayer::ModalOverlay.push_named(input_coordinator, "welcome_wizard");

    // Block all clicks on the dimmer (wizard is non-closeable)
    input_coordinator.register_on_layer(
        "welcome_wizard:dimmer",
        WidgetRect::new(0.0, 0.0, window_w, window_h),
        Sense::CLICK,
        &layer_id,
    );

    // ── Modal dimensions ─────────────────────────────────────────────────────
    let modal_w: f64 = 580.0;
    let modal_h: f64 = match state.wizard_page {
        1 => page1_height(state),
        _ => 360.0, // page 0 — welcome + Get Started
    };
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
        "welcome_wizard:modal_bg",
        WidgetRect::new(modal_x, modal_y, modal_w, modal_h),
        Sense::CLICK,
        &layer_id,
    );

    let padding = 32.0;
    let inner_x = modal_x + padding;
    let inner_w = modal_w - padding * 2.0;
    let mut cy = modal_y + 32.0;

    match state.wizard_page {
        0 => render_page0(ctx, inner_x, inner_w, &mut cy, state, text_color, toolbar_theme, input_coordinator, &layer_id, result),
        1 => render_page1_passphrase(ctx, inner_x, inner_w, &mut cy, state, text_color, toolbar_theme, frame_theme, current_time_ms, input_coordinator, &layer_id, result),
        _ => render_page0(ctx, inner_x, inner_w, &mut cy, state, text_color, toolbar_theme, input_coordinator, &layer_id, result),
    }
}

/// Compute the height for page 1.
fn page1_height(_state: &UserSettingsState) -> f64 {
    320.0 // Passphrase only
}

// =============================================================================
// Page 0 — Welcome
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page0(
    ctx: &mut dyn RenderContext,
    x: f64,
    w: f64,
    cy: &mut f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();

    // Title
    ctx.set_font("bold 22px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Welcome to Nemo", x + w / 2.0, *cy);
    *cy += 40.0;

    // Subtitle
    ctx.set_font("14px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.65)");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Professional trading charts and analytics.", x + w / 2.0, *cy);
    *cy += 24.0;

    ctx.set_font("14px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.65)");
    ctx.fill_text("Click \u{201C}Get Started\u{201D} to set up your profile.", x + w / 2.0, *cy);
    ctx.set_text_align(TextAlign::Left);
    *cy += 60.0;

    // Info note about passphrase
    let note_h = 56.0;
    ctx.set_fill_color("rgba(240,173,78,0.07)");
    ctx.fill_rounded_rect(x, *cy, w, note_h, 4.0);
    ctx.set_stroke_color("rgba(240,173,78,0.25)");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, *cy, w, note_h, 4.0);
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color("rgba(244,205,99,0.75)");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Next: you\u{2019}ll create a passphrase to protect your data.", x + 12.0, *cy + 10.0);
    ctx.fill_text("Your passphrase is never stored on any server.", x + 12.0, *cy + 28.0);
    *cy += note_h + 36.0;

    // "Get Started" button
    let btn_h = 38.0;
    let btn_w = w.min(200.0);
    let btn_x = x + (w - btn_w) / 2.0;
    let is_btn_hovered = hovered == Some("wizard_get_started");
    let btn_bg = if is_btn_hovered { "rgba(255,255,255,0.92)" } else { toolbar_theme.accent.as_str() };
    ctx.set_fill_color(btn_bg);
    ctx.fill_rounded_rect(btn_x, *cy, btn_w, btn_h, 4.0);
    ctx.set_font("bold 13px sans-serif");
    ctx.set_fill_color("rgba(0,0,0,0.85)");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Get Started", btn_x + btn_w / 2.0, *cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    let btn_rect = WidgetRect::new(btn_x, *cy, btn_w, btn_h);
    result.content_items.push(("wizard_get_started".to_string(), btn_rect));
    input_coordinator.register_on_layer("user_settings:wizard_get_started", btn_rect, Sense::CLICK, layer_id);
}

// =============================================================================
// Page 1 — Set Passphrase (mandatory; Connected mode also shows sign-in)
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page1_passphrase(
    ctx: &mut dyn RenderContext,
    x: f64,
    w: f64,
    cy: &mut f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();

    // Back arrow
    render_back_button(ctx, x, cy, toolbar_theme, layer_id, input_coordinator, result, hovered);
    *cy += 8.0;

    // Title
    ctx.set_font("bold 20px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Set Passphrase", x + w / 2.0, *cy);
    ctx.set_text_align(TextAlign::Left);
    *cy += 26.0;

    // Step indicator
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.35)");
    ctx.set_text_align(TextAlign::Center);
    ctx.fill_text("Step 2 of 2 — Set Passphrase", x + w / 2.0, *cy);
    ctx.set_text_align(TextAlign::Left);
    *cy += 24.0;

    // Warning box
    let warn_h = 52.0;
    ctx.set_fill_color("rgba(240,173,78,0.08)");
    ctx.fill_rounded_rect(x, *cy, w, warn_h, 4.0);
    ctx.set_stroke_color("rgba(240,173,78,0.30)");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, *cy, w, warn_h, 4.0);
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color("rgba(244,205,99,0.85)");
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("This passphrase encrypts your local data. If you forget it,", x + 10.0, *cy + 8.0);
    ctx.fill_text("data cannot be recovered. Keep it somewhere safe.", x + 10.0, *cy + 24.0);
    *cy += warn_h + 16.0;

    // Passphrase input (mandatory)
    *cy = render_passphrase_input(ctx, x, w, cy, state, text_color, toolbar_theme, frame_theme, current_time_ms, layer_id, input_coordinator, result);

    // ── Complete Setup button (disabled until passphrase meets minimum length) ──
    let enable_disabled = state.e2e_passphrase_editing.text.len() < crate::user_manager::MIN_PASSPHRASE_LENGTH;
    let is_e2e_hovered = !enable_disabled && hovered == Some("wizard_enable_e2e");
    let enable_bg = if enable_disabled {
        "rgba(244,205,99,0.20)"
    } else if is_e2e_hovered {
        "rgba(255,255,255,0.92)"
    } else {
        toolbar_theme.accent.as_str()
    };
    let enable_text_col = if enable_disabled { "rgba(0,0,0,0.35)" } else { "rgba(0,0,0,0.85)" };
    let cbtn_h = 32.0;
    let cbtn_w = w.min(200.0);
    ctx.set_fill_color(enable_bg);
    ctx.fill_rounded_rect(x, *cy, cbtn_w, cbtn_h, 4.0);
    ctx.set_font("bold 12px sans-serif");
    ctx.set_fill_color(enable_text_col);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Complete Setup", x + cbtn_w / 2.0, *cy + cbtn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    if !enable_disabled {
        let btn_rect = WidgetRect::new(x, *cy, cbtn_w, cbtn_h);
        result.content_items.push(("wizard_enable_e2e".to_string(), btn_rect));
        input_coordinator.register_on_layer("user_settings:wizard_enable_e2e", btn_rect, Sense::CLICK, layer_id);
    }
}

// =============================================================================
// Shared helper widgets
// =============================================================================


/// Render the back arrow button. Returns the new cy after the button.
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
    let btn_w = 70.0;
    let btn_h = 26.0;
    let is_hovered = hovered_item_id == Some("wizard_back");
    let btn_bg = if is_hovered { "rgba(255,255,255,0.12)" } else { "rgba(255,255,255,0.06)" };
    ctx.set_fill_color(btn_bg);
    ctx.fill_rounded_rect(x, *cy, btn_w, btn_h, 4.0);
    let stroke_color = if is_hovered { "rgba(254,255,238,0.40)" } else { "rgba(254,255,238,0.20)" };
    ctx.set_stroke_color(stroke_color);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, *cy, btn_w, btn_h, 4.0);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("← Back", x + btn_w / 2.0, *cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    let back_rect = WidgetRect::new(x, *cy, btn_w, btn_h);
    result.content_items.push(("wizard_back".to_string(), back_rect));
    input_coordinator.register_on_layer("user_settings:wizard_back", back_rect, Sense::CLICK, layer_id);
    *cy += btn_h;
}

/// Render the passphrase input box using the canonical `draw_input` widget. Returns new cy after the input.
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
) -> f64 {
    // Label
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Passphrase", x, *cy);
    *cy += 18.0;

    // Input box using canonical draw_input
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

    // Register for click-to-focus
    result.content_items.push(("e2e_passphrase_input".to_string(), input_rect));
    input_coordinator.register_on_layer("user_settings:e2e_passphrase_input", input_rect, Sense::CLICK, layer_id);

    // Blinking cursor (only when focused)
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
    *cy
}

/// Render the Vault Unlock overlay.
///
/// Shown at startup when the profile is encrypted (salt.hex exists) but no vault key has
/// been derived yet.  The user must enter their passphrase to unlock their data.
/// Like the Welcome Wizard this is a full-screen, non-closeable overlay.
#[allow(clippy::too_many_arguments)]
pub fn render_vault_unlock(
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

    // Push a high-z modal layer so the dialog absorbs all input
    let layer_id = ZLayer::ModalOverlay.push_named(input_coordinator, "vault_unlock");

    // Block all clicks on the dimmer (dialog is non-closeable)
    input_coordinator.register_on_layer(
        "vault_unlock:dimmer",
        WidgetRect::new(0.0, 0.0, window_w, window_h),
        Sense::CLICK,
        &layer_id,
    );

    // ── Modal dimensions ─────────────────────────────────────────────────────
    let modal_w: f64 = 480.0;
    // Expand the modal to fit: error message adds height, and after 3 attempts the
    // "Forgot passphrase?" link requires additional space.
    let modal_h: f64 = if state.vault_unlock_attempts >= 3 {
        340.0
    } else if state.vault_unlock_error.is_some() {
        296.0
    } else {
        260.0
    };
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
        "vault_unlock:modal_bg",
        WidgetRect::new(modal_x, modal_y, modal_w, modal_h),
        Sense::CLICK,
        &layer_id,
    );

    let padding = 32.0;
    let inner_x = modal_x + padding;
    let inner_w = modal_w - padding * 2.0;
    let mut cy = modal_y + 30.0;

    // Title
    ctx.set_font("bold 20px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Unlock Your Data", inner_x + inner_w / 2.0, cy);
    ctx.set_text_align(TextAlign::Left);
    cy += 28.0;

    // Subtitle
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.55)");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Enter your passphrase to decrypt your profile", inner_x + inner_w / 2.0, cy);
    ctx.set_text_align(TextAlign::Left);
    cy += 24.0;

    // Passphrase input (reuse the shared helper)
    cy = render_passphrase_input(ctx, inner_x, inner_w, &mut cy, state, text_color, toolbar_theme, frame_theme, current_time_ms, &layer_id, input_coordinator, result);

    // Unlock button (disabled until passphrase is entered; vault passphrase can be any length)
    let unlock_disabled = state.e2e_passphrase_editing.text.is_empty();
    let hovered = state.hovered_item_id.as_deref();
    let is_unlock_hovered = !unlock_disabled && hovered == Some("vault_unlock_btn");
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
        result.content_items.push(("vault_unlock_btn".to_string(), btn_rect));
        input_coordinator.register_on_layer("user_settings:vault_unlock_btn", btn_rect, Sense::CLICK, &layer_id);
    }
    cy += btn_h + 10.0;

    // ── Error message ────────────────────────────────────────────────────────
    if let Some(ref err_msg) = state.vault_unlock_error {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color("rgba(255,80,80,0.90)");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(err_msg.as_str(), inner_x + inner_w / 2.0, cy);
        ctx.set_text_align(TextAlign::Left);
        cy += 20.0;
    }

    // ── "Forgot passphrase?" link — shown after 3 failed attempts ────────────
    if state.vault_unlock_attempts >= 3 {
        let is_hovered = hovered == Some("vault_unlock_new_profile");
        let link_color = if is_hovered {
            "rgba(254,255,238,0.70)"
        } else {
            "rgba(254,255,238,0.38)"
        };
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(link_color);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text("Forgot passphrase? Switch or create profile", inner_x + inner_w / 2.0, cy);
        ctx.set_text_align(TextAlign::Left);

        let link_w = 260.0;
        let link_h = 18.0;
        let link_rect = WidgetRect::new(inner_x + (inner_w - link_w) / 2.0, cy, link_w, link_h);
        result.content_items.push(("vault_unlock_new_profile".to_string(), link_rect));
        input_coordinator.register_on_layer(
            "user_settings:vault_unlock_new_profile",
            link_rect,
            Sense::CLICK,
            &layer_id,
        );
    }
}

/// Render the vault profile picker overlay.
///
/// Shown after the user clicks "Forgot passphrase?" on the vault unlock screen.
/// Allows the user to switch to another profile or create a new one.
#[allow(clippy::too_many_arguments)]
fn render_vault_profile_picker(
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

    // Collect profiles excluding the currently locked one.
    let other_profiles: Vec<&(String, String, String, String)> = state
        .available_profiles
        .iter()
        .filter(|(id, _, _, _)| id != &state.profile_id)
        .collect();

    // ── Modal height calculation ──────────────────────────────────────────────
    let profile_row_h: f64 = 48.0;
    let n = other_profiles.len();
    let calculated_h: f64 = 30.0   // top pad
        + 28.0                      // title
        + 24.0                      // subtitle
        + 12.0                      // gap
        + n as f64 * profile_row_h  // profile rows
        + 12.0                      // gap
        + 1.0                       // separator
        + 12.0                      // gap
        + 36.0                      // create button
        + 12.0                      // gap
        + 18.0                      // back link
        + 20.0;                     // bottom pad
    let modal_h = calculated_h.min(window_h - 80.0);
    let modal_w: f64 = 480.0;
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
        "vault_picker:modal_bg",
        WidgetRect::new(modal_x, modal_y, modal_w, modal_h),
        Sense::CLICK,
        layer_id,
    );

    let padding = 32.0;
    let inner_x = modal_x + padding;
    let inner_w = modal_w - padding * 2.0;
    let mut cy = modal_y + 30.0;

    // ── Title ─────────────────────────────────────────────────────────────────
    ctx.set_font("bold 20px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Switch Profile", inner_x + inner_w / 2.0, cy);
    ctx.set_text_align(TextAlign::Left);
    cy += 28.0;

    // ── Subtitle ─────────────────────────────────────────────────────────────
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.55)");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Choose a profile or create a new one", inner_x + inner_w / 2.0, cy);
    ctx.set_text_align(TextAlign::Left);
    cy += 24.0 + 12.0; // subtitle + gap

    // ── Profile rows ─────────────────────────────────────────────────────────
    for (id, name, _avatar, sync_level) in &other_profiles {
        let widget_id = format!("vault_picker_profile:{}", id);
        let is_row_hovered = hovered == Some(widget_id.as_str());

        let row_bg = if is_row_hovered {
            "rgba(255,255,255,0.08)"
        } else {
            "rgba(255,255,255,0.04)"
        };
        ctx.set_fill_color(row_bg);
        ctx.fill_rounded_rect(inner_x, cy, inner_w, profile_row_h - 4.0, 4.0);

        // Display name (bold 14px, left)
        let row_mid_y = cy + (profile_row_h - 4.0) / 2.0;
        ctx.set_font("bold 14px sans-serif");
        ctx.set_fill_color(text_color);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(name.as_str(), inner_x + 12.0, row_mid_y);

        // Client mode badge (10px, dimmed, right-aligned)
        let badge_label = match sync_level.as_str() {
            "cloud_zt" => "Cloud ZT",
            "cloud" => "Cloud",
            "connected" => "Connected",
            _ => "Local",
        };
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color("rgba(254,255,238,0.35)");
        ctx.set_text_align(TextAlign::Right);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(badge_label, inner_x + inner_w - 12.0, row_mid_y);
        ctx.set_text_align(TextAlign::Left);

        // Register click
        let row_rect = WidgetRect::new(inner_x, cy, inner_w, profile_row_h - 4.0);
        result.content_items.push((widget_id.clone(), row_rect));
        let hit_id = format!("user_settings:{}", widget_id);
        input_coordinator.register_on_layer(hit_id.as_str(), row_rect, Sense::CLICK, layer_id);

        cy += profile_row_h;
    }

    cy += 12.0; // gap before separator

    // ── Separator line ────────────────────────────────────────────────────────
    ctx.set_fill_color("rgba(255,255,255,0.08)");
    ctx.fill_rect(inner_x, cy, inner_w, 1.0);
    cy += 1.0 + 12.0; // separator + gap

    // ── "Create New Profile" button ───────────────────────────────────────────
    let create_btn_h: f64 = 36.0;
    let is_create_hovered = hovered == Some("vault_picker_create_new");
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
    result.content_items.push(("vault_picker_create_new".to_string(), create_rect));
    input_coordinator.register_on_layer(
        "user_settings:vault_picker_create_new",
        create_rect,
        Sense::CLICK,
        layer_id,
    );
    cy += create_btn_h + 12.0; // button + gap

    // ── "Back" link ───────────────────────────────────────────────────────────
    let is_back_hovered = hovered == Some("vault_picker_back");
    let back_color = if is_back_hovered {
        "rgba(254,255,238,0.70)"
    } else {
        "rgba(254,255,238,0.38)"
    };
    let back_h: f64 = 18.0;
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(back_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Back to passphrase entry", inner_x + inner_w / 2.0, cy);
    ctx.set_text_align(TextAlign::Left);

    let back_w = 200.0;
    let back_rect = WidgetRect::new(inner_x + (inner_w - back_w) / 2.0, cy, back_w, back_h);
    result.content_items.push(("vault_picker_back".to_string(), back_rect));
    input_coordinator.register_on_layer(
        "user_settings:vault_picker_back",
        back_rect,
        Sense::CLICK,
        layer_id,
    );
}

/// Render a "Skip for now" link button.
///
/// Not used in the main wizard flow (passphrase is mandatory) but kept as a
/// helper in case it is needed by other UI paths.
#[allow(dead_code)]
fn render_skip_link(
    ctx: &mut dyn RenderContext,
    x: f64,
    cy: &mut f64,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
    hovered_item_id: Option<&str>,
) {
    let skip_btn_w = 120.0;
    let skip_btn_h = 24.0;
    let is_hovered = hovered_item_id == Some("wizard_skip");
    let link_color = if is_hovered { "rgba(254,255,238,0.70)" } else { "rgba(254,255,238,0.40)" };
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(link_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Skip for now", x, *cy + skip_btn_h / 2.0);

    let skip_rect = WidgetRect::new(x, *cy, skip_btn_w, skip_btn_h);
    result.content_items.push(("wizard_skip".to_string(), skip_rect));
    input_coordinator.register_on_layer("user_settings:wizard_skip", skip_rect, Sense::CLICK, layer_id);
    *cy += skip_btn_h;
}
