//! First-run Welcome Wizard modal.
//!
//! Shown when the user launches the app for the first time (no `profile.json`
//! found on disk).  The wizard is a full-screen dimmer + centered modal that
//! cannot be dismissed except by completing the setup flow.
//!
//! Pages:
//!   0 — Connection Mode  (Standalone / Connected)
//!   1 — Set Passphrase   (mandatory for both modes; Connected also shows sign-in)

use crate::engine::render::RenderContext;
use uzor::render::{TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;
use uzor::input::sense::Sense;
use crate::ui::modal_settings::UserSettingsState;
use crate::ui::toolbar_render::ToolbarTheme;
use crate::layout::render_frame::UserSettingsResult;
use crate::ui::z_order::ZLayer;

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
        _ => 380.0, // page 0
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
        1 => render_page1_passphrase(ctx, inner_x, inner_w, &mut cy, state, text_color, toolbar_theme, input_coordinator, &layer_id, result),
        _ => render_page0(ctx, inner_x, inner_w, &mut cy, state, text_color, toolbar_theme, input_coordinator, &layer_id, result),
    }
}

/// Compute the height for page 1 based on the selected connection mode.
fn page1_height(state: &UserSettingsState) -> f64 {
    if state.wizard_mode_standalone {
        320.0 // Standalone: passphrase only
    } else {
        420.0 // Connected: passphrase + sign-in section
    }
}

// =============================================================================
// Page 0 — Connection Mode selection
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
    *cy += 34.0;

    // Step indicator
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.35)");
    ctx.fill_text("Step 1 of 2 — Connection Mode", x + w / 2.0, *cy);
    ctx.set_text_align(TextAlign::Left);
    *cy += 32.0;

    // ── Card A: Standalone ────────────────────────────────────────────────────
    render_mode_card(
        ctx, x, *cy, w, toolbar_theme, layer_id, input_coordinator, result,
        "Standalone (Offline)",
        "No internet required. All data stays on this device.",
        "Choose Offline",
        "wizard_standalone",
        false,
        hovered,
    );
    *cy += 102.0;

    // ── Card B: Connected ─────────────────────────────────────────────────────
    render_mode_card(
        ctx, x, *cy, w, toolbar_theme, layer_id, input_coordinator, result,
        "Connected (Cloud Sync)",
        "Sync presets, watchlists across devices. Requires a free account.",
        "Choose Connected",
        "wizard_connected",
        true,
        hovered,
    );
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
    *cy = render_passphrase_input(ctx, x, w, cy, state, text_color, toolbar_theme, layer_id, input_coordinator, result);

    // ── Connected mode: sign-in section ──────────────────────────────────────
    if !state.wizard_mode_standalone {
        ctx.set_font("600 11px sans-serif");
        ctx.set_fill_color("rgba(244,205,99,0.7)");
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text("SIGN IN", x, *cy);
        *cy += 16.0;

        ctx.set_font("12px sans-serif");
        ctx.set_fill_color("rgba(254,255,238,0.55)");
        ctx.fill_text("Open chart.example/login to link your account.", x, *cy);
        *cy += 20.0;

        let btn_h = 28.0;
        let btn_w = w.min(160.0);
        let is_browser_hovered = hovered == Some("wizard_open_browser");
        let browser_bg = if is_browser_hovered { "rgba(255,255,255,0.92)" } else { toolbar_theme.accent.as_str() };
        ctx.set_fill_color(browser_bg);
        ctx.fill_rounded_rect(x, *cy, btn_w, btn_h, 4.0);
        ctx.set_font("bold 11px sans-serif");
        ctx.set_fill_color("rgba(0,0,0,0.85)");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Open Browser", x + btn_w / 2.0, *cy + btn_h / 2.0);
        ctx.set_text_align(TextAlign::Left);

        let open_rect = WidgetRect::new(x, *cy, btn_w, btn_h);
        result.content_items.push(("wizard_open_browser".to_string(), open_rect));
        input_coordinator.register_on_layer("user_settings:wizard_open_browser", open_rect, Sense::CLICK, layer_id);
        *cy += btn_h + 8.0;

        if !state.wizard_linking_status.is_empty() {
            let status_color = if state.wizard_linking_status.starts_with("Linked") { "#5cb85c" } else { "rgba(254,255,238,0.45)" };
            ctx.set_font("11px sans-serif");
            ctx.set_fill_color(status_color);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.fill_text(&state.wizard_linking_status, x, *cy);
            *cy += 16.0;
        }
        *cy += 12.0;
    }

    // ── Complete Setup button (disabled until passphrase is entered) ──────────
    let enable_disabled = state.e2e_passphrase.is_empty();
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

/// Render a single mode-selection card.
#[allow(clippy::too_many_arguments)]
fn render_mode_card(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    w: f64,
    toolbar_theme: &ToolbarTheme,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
    title: &str,
    description: &str,
    btn_label: &str,
    action: &str,
    accent_title: bool,
    hovered_item_id: Option<&str>,
) {
    let card_h = 90.0;
    let card_padding = 14.0;
    let is_btn_hovered = hovered_item_id == Some(action);

    // Card background — subtle highlight when button is hovered
    let card_bg = if is_btn_hovered { "rgba(255,255,255,0.07)" } else { "rgba(255,255,255,0.04)" };
    ctx.set_fill_color(card_bg);
    ctx.fill_rounded_rect(x, y, w, card_h, 6.0);
    let card_stroke = if is_btn_hovered { "rgba(244,205,99,0.32)" } else { "rgba(244,205,99,0.18)" };
    ctx.set_stroke_color(card_stroke);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, y, w, card_h, 6.0);

    // Title
    let title_color = if accent_title { toolbar_theme.accent.as_str() } else { toolbar_theme.item_text.as_str() };
    ctx.set_font("bold 13px sans-serif");
    ctx.set_fill_color(title_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(title, x + card_padding, y + card_padding);

    // Description
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.50)");
    ctx.fill_text(description, x + card_padding, y + card_padding + 20.0);

    // Button (right side, vertically centered)
    let btn_w = 130.0;
    let btn_h = 28.0;
    let btn_x = x + w - card_padding - btn_w;
    let btn_y = y + (card_h - btn_h) / 2.0;

    // Slightly brighten button on hover
    let btn_bg = if is_btn_hovered {
        // Blend accent with white for a lighter shade on hover
        "rgba(255,255,255,0.92)"
    } else {
        toolbar_theme.accent.as_str()
    };
    ctx.set_fill_color(btn_bg);
    ctx.fill_rounded_rect(btn_x, btn_y, btn_w, btn_h, 4.0);
    ctx.set_font("bold 11px sans-serif");
    ctx.set_fill_color("rgba(0,0,0,0.85)");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(btn_label, btn_x + btn_w / 2.0, btn_y + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    let hit_rect = WidgetRect::new(btn_x, btn_y, btn_w, btn_h);
    let action_id = format!("user_settings:{}", action);
    result.content_items.push((action.to_string(), hit_rect));
    input_coordinator.register_on_layer(
        action_id.as_str(),
        hit_rect,
        Sense::CLICK,
        layer_id,
    );
}

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

/// Render the passphrase input box. Returns new cy after the input.
#[allow(clippy::too_many_arguments)]
fn render_passphrase_input(
    ctx: &mut dyn RenderContext,
    x: f64,
    w: f64,
    cy: &mut f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
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

    // Input box
    let input_h = 30.0;
    let masked: String = if state.e2e_passphrase.is_empty() {
        "Click to type passphrase\u{2026}".to_string()
    } else {
        "\u{2022}".repeat(state.e2e_passphrase.chars().count().min(24))
    };
    let input_text_color = if state.e2e_passphrase.is_empty() {
        "rgba(254,255,238,0.25)"
    } else {
        text_color
    };

    ctx.set_fill_color(&toolbar_theme.item_bg_hover);
    ctx.fill_rounded_rect(x, *cy, w, input_h, 3.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, *cy, w, input_h, 3.0);
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(input_text_color);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&masked, x + 8.0, *cy + input_h / 2.0);

    let input_rect = WidgetRect::new(x, *cy, w, input_h);
    result.content_items.push(("e2e_passphrase_input".to_string(), input_rect));
    input_coordinator.register_on_layer("user_settings:e2e_passphrase_input", input_rect, Sense::CLICK, layer_id);
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
    let modal_h: f64 = 260.0;
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
    cy = render_passphrase_input(ctx, inner_x, inner_w, &mut cy, state, text_color, toolbar_theme, &layer_id, input_coordinator, result);

    // Unlock button (disabled until passphrase is entered)
    let unlock_disabled = state.e2e_passphrase.is_empty();
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
