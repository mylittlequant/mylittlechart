//! First-run Welcome Wizard modal.
//!
//! Shown when the user launches the app for the first time (no `profile.json`
//! found on disk).  The wizard is a full-screen dimmer + centered modal that
//! cannot be dismissed except by making a mode choice.
//!
//! Pages:
//!   0 — Mode selection (Standalone / Connected / Connected + E2E)
//!   1 — Link account (device code polling)
//!   2 — E2E setup (only when E2E was chosen on page 0)

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
        1 => 320.0,
        2 => 360.0,
        _ => 420.0, // page 0
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
        1 => render_page1(ctx, inner_x, inner_w, &mut cy, state, text_color, toolbar_theme, input_coordinator, &layer_id, result),
        2 => render_page2(ctx, inner_x, inner_w, &mut cy, state, text_color, toolbar_theme, input_coordinator, &layer_id, result),
        _ => render_page0(ctx, inner_x, inner_w, &mut cy, state, text_color, toolbar_theme, input_coordinator, &layer_id, result),
    }
}

// =============================================================================
// Page 0 — Mode selection
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page0(
    ctx: &mut dyn RenderContext,
    x: f64,
    w: f64,
    cy: &mut f64,
    _state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    // Title
    ctx.set_font("bold 22px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Welcome to Nemo", x + w / 2.0, *cy);
    *cy += 36.0;

    // Subtitle
    ctx.set_font("14px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.55)");
    ctx.fill_text("Choose how you want to use the terminal", x + w / 2.0, *cy);
    ctx.set_text_align(TextAlign::Left);
    *cy += 36.0;

    // ── Card A: Standalone ────────────────────────────────────────────────────
    render_mode_card(
        ctx, x, *cy, w, toolbar_theme, layer_id, input_coordinator, result,
        "Standalone (Offline)",
        "No internet required. All data stays on this device.",
        "Start Offline",
        "wizard_standalone",
        false,
    );
    *cy += 96.0;

    // ── Card B: Connected ─────────────────────────────────────────────────────
    render_mode_card(
        ctx, x, *cy, w, toolbar_theme, layer_id, input_coordinator, result,
        "Connected",
        "Sync presets, watchlists, and templates across devices. Requires a free account.",
        "Connect Account",
        "wizard_connected",
        true,
    );
    *cy += 96.0;

    // ── Card C: Connected + E2E ───────────────────────────────────────────────
    render_mode_card(
        ctx, x, *cy, w, toolbar_theme, layer_id, input_coordinator, result,
        "Zero-Trust (E2E Encrypted)",
        "Same as Connected, but data is encrypted before leaving your device. Server cannot read it.",
        "Set Up E2E",
        "wizard_e2e",
        true,
    );
}

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
) {
    let card_h = 84.0;
    let card_padding = 14.0;

    // Card background
    ctx.set_fill_color("rgba(255,255,255,0.04)");
    ctx.fill_rounded_rect(x, y, w, card_h, 6.0);
    ctx.set_stroke_color("rgba(244,205,99,0.18)");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, y, w, card_h, 6.0);

    // Title
    let title_color = if accent_title {
        &toolbar_theme.accent
    } else {
        &toolbar_theme.item_text
    };
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
    let btn_w = 120.0;
    let btn_h = 26.0;
    let btn_x = x + w - card_padding - btn_w;
    let btn_y = y + (card_h - btn_h) / 2.0;

    ctx.set_fill_color(&toolbar_theme.accent);
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

// =============================================================================
// Page 1 — Link Account
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page1(
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
    // Title
    ctx.set_font("bold 20px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Link Your Account", x + w / 2.0, *cy);
    ctx.set_text_align(TextAlign::Left);
    *cy += 36.0;

    // Instructions
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.60)");
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Open chart.example/login in your browser, then enter this code:", x, *cy);
    *cy += 24.0;

    // Device code — large centered monospace display
    let code_display = if state.wizard_device_code.is_empty() {
        "--------".to_string()
    } else {
        state.wizard_device_code.clone()
    };

    ctx.set_font("bold 32px monospace");
    ctx.set_fill_color(&toolbar_theme.accent);
    ctx.set_text_align(TextAlign::Center);
    ctx.fill_text(&code_display, x + w / 2.0, *cy);
    ctx.set_text_align(TextAlign::Left);
    *cy += 56.0;

    // Status line
    let status_text = if state.wizard_linking_status.is_empty() {
        "Waiting for account link..."
    } else {
        state.wizard_linking_status.as_str()
    };
    let status_color = if state.wizard_linking_status.starts_with("Linked") {
        "#5cb85c"
    } else {
        "rgba(254,255,238,0.45)"
    };
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(status_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(status_text, x + w / 2.0, *cy);
    ctx.set_text_align(TextAlign::Left);
    *cy += 40.0;

    // Back button
    let btn_w = 80.0;
    let btn_h = 28.0;
    let btn_x = x;
    ctx.set_fill_color("rgba(255,255,255,0.06)");
    ctx.fill_rounded_rect(btn_x, *cy, btn_w, btn_h, 4.0);
    ctx.set_stroke_color("rgba(254,255,238,0.20)");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(btn_x, *cy, btn_w, btn_h, 4.0);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.55)");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Back", btn_x + btn_w / 2.0, *cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    let back_rect = WidgetRect::new(btn_x, *cy, btn_w, btn_h);
    result.content_items.push(("wizard_back".to_string(), back_rect));
    input_coordinator.register_on_layer(
        "user_settings:wizard_back",
        back_rect,
        Sense::CLICK,
        layer_id,
    );
}

// =============================================================================
// Page 2 — E2E Setup
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page2(
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
    // Title
    ctx.set_font("bold 20px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Set Up End-to-End Encryption", x + w / 2.0, *cy);
    ctx.set_text_align(TextAlign::Left);
    *cy += 36.0;

    // Warning box
    let warn_h = 64.0;
    ctx.set_fill_color("rgba(240,173,78,0.08)");
    ctx.fill_rounded_rect(x, *cy, w, warn_h, 4.0);
    ctx.set_stroke_color("rgba(240,173,78,0.30)");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, *cy, w, warn_h, 4.0);
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color("rgba(244,205,99,0.85)");
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Choose a strong passphrase. If you lose it, your cloud data", x + 10.0, *cy + 8.0);
    ctx.fill_text("cannot be recovered — there is no reset. Keep it somewhere safe.", x + 10.0, *cy + 26.0);
    ctx.fill_text("Your passphrase is never transmitted — only used locally.", x + 10.0, *cy + 44.0);
    *cy += warn_h + 16.0;

    // Passphrase label
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Passphrase", x, *cy);
    *cy += 18.0;

    // Passphrase input box
    let input_h = 28.0;
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
    input_coordinator.register_on_layer(
        "user_settings:e2e_passphrase_input",
        input_rect,
        Sense::CLICK,
        layer_id,
    );
    *cy += input_h + 16.0;

    // Action buttons row
    let btn_h = 30.0;
    let enable_btn_w = 160.0;
    let skip_btn_w = 100.0;
    let btn_gap = 12.0;

    // "Enable E2E" button — only active when passphrase is non-empty
    let enable_disabled = state.e2e_passphrase.is_empty();
    let enable_bg = if enable_disabled {
        "rgba(244,205,99,0.20)"
    } else {
        &toolbar_theme.accent
    };
    let enable_text = if enable_disabled {
        "rgba(0,0,0,0.35)"
    } else {
        "rgba(0,0,0,0.85)"
    };
    ctx.set_fill_color(enable_bg);
    ctx.fill_rounded_rect(x, *cy, enable_btn_w, btn_h, 4.0);
    ctx.set_font("bold 12px sans-serif");
    ctx.set_fill_color(enable_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Enable E2E Encryption", x + enable_btn_w / 2.0, *cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    if !enable_disabled {
        let enable_rect = WidgetRect::new(x, *cy, enable_btn_w, btn_h);
        result.content_items.push(("wizard_enable_e2e".to_string(), enable_rect));
        input_coordinator.register_on_layer(
            "user_settings:wizard_enable_e2e",
            enable_rect,
            Sense::CLICK,
            layer_id,
        );
    }

    // "Skip for now" link button
    let skip_x = x + enable_btn_w + btn_gap;
    ctx.set_fill_color("rgba(255,255,255,0.0)");
    ctx.fill_rect(skip_x, *cy, skip_btn_w, btn_h);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color("rgba(254,255,238,0.40)");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Skip for now", skip_x, *cy + btn_h / 2.0);

    let skip_rect = WidgetRect::new(skip_x, *cy, skip_btn_w, btn_h);
    result.content_items.push(("wizard_skip_e2e".to_string(), skip_rect));
    input_coordinator.register_on_layer(
        "user_settings:wizard_skip_e2e",
        skip_rect,
        Sense::CLICK,
        layer_id,
    );
}
