//! First-run Welcome Wizard modal.
//!
//! Shown when the user launches the app for the first time (no `profile.json`
//! found on disk).  The wizard is a full-screen dimmer + centered modal that
//! cannot be dismissed except by completing the setup flow.
//!
//! Pages:
//!   0 — Welcome + Language   (mascot, lang selection, Get Started)
//!   1 — Theme                (5 presets)
//!   2 — Profile + Passphrase (name input, passphrase, ZT info, Generate Recovery Phrase)

use crate::engine::render::{draw_svg_icon, draw_svg_multicolor, RenderContext};
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
use crate::i18n::{Language, current_language, WizardKey, t_wizard};
use crate::ui::icons::Icon;

const MINI_MASCOT_SVG: &str = include_str!("../../../../../assets/mascot/mini_mascot.svg");

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
        0 => 400.0,  // welcome + language + mascot
        1 => 420.0,  // theme
        2 => 440.0,  // profile + passphrase (extra height for passphrase hint)
        _ => 400.0,
    };
    let modal_x = (window_w - modal_w) / 2.0;
    let modal_y = (window_h - modal_h) / 2.0;

    // Modal background + border
    ctx.set_fill_color(toolbar_theme.background.as_str());
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
        1 => render_page1_theme(ctx, inner_x, inner_w, &mut cy, state, text_color, toolbar_theme, input_coordinator, &layer_id, result),
        2 => render_page2_profile(ctx, inner_x, inner_w, &mut cy, state, text_color, toolbar_theme, frame_theme, current_time_ms, input_coordinator, &layer_id, result),
        _ => render_page0(ctx, inner_x, inner_w, &mut cy, state, text_color, toolbar_theme, input_coordinator, &layer_id, result),
    }
}

// =============================================================================
// Page 0 — Welcome + Language (merged)
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
    let active_lang = current_language();

    // Title
    ctx.set_font("bold 22px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    let title = format!("{} mylittlechart", t_wizard(WizardKey::WelcomeTo));
    ctx.fill_text(&title, x + w / 2.0, *cy);
    *cy += 28.0;

    // Mini mascot centered
    let mascot_size = 80.0;
    let mascot_x = x + (w - mascot_size) / 2.0;
    draw_svg_multicolor(ctx, MINI_MASCOT_SVG, mascot_x, *cy, mascot_size, mascot_size);
    *cy += mascot_size + 20.0;

    // Language selection rows
    let langs: &[(Language, &str)] = &[
        (Language::En, "wizard_lang_en"),
        (Language::Ru, "wizard_lang_ru"),
    ];

    let row_h = 44.0;
    let row_gap = 8.0;

    for (lang, widget_id) in langs {
        let is_active = active_lang == *lang;
        let is_row_hovered = hovered == Some(widget_id);

        let row_bg = if is_row_hovered {
            toolbar_theme.button_bg_hover.as_str()
        } else {
            toolbar_theme.button_bg.as_str()
        };
        ctx.set_fill_color(row_bg);
        ctx.fill_rounded_rect(x, *cy, w, row_h, 4.0);

        // Active accent left border
        if is_active {
            ctx.set_fill_color(toolbar_theme.accent.as_str());
            ctx.fill_rounded_rect(x, *cy, 3.0, row_h, 2.0);
        }

        // Language name
        let text_alpha = if is_active { toolbar_theme.item_text.as_str() } else { toolbar_theme.item_text_muted.as_str() };
        ctx.set_font("14px sans-serif");
        ctx.set_fill_color(text_alpha);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(lang.native_name(), x + 16.0, *cy + row_h / 2.0);

        let row_rect = WidgetRect::new(x, *cy, w, row_h);
        result.content_items.push((widget_id.to_string(), row_rect));
        let hit_id = format!("user_settings:{}", widget_id);
        input_coordinator.register_on_layer(hit_id.as_str(), row_rect, Sense::CLICK | Sense::HOVER, layer_id);

        *cy += row_h + row_gap;
    }

    *cy += 12.0;

    // "Get Started" button
    let btn_h = 38.0;
    let btn_w = w.min(200.0);
    let btn_x = x + (w - btn_w) / 2.0;
    let is_btn_hovered = hovered == Some("wizard_get_started");
    let btn_bg = if is_btn_hovered { toolbar_theme.button_bg_hover.as_str() } else { toolbar_theme.button_bg.as_str() };
    let btn_text = if is_btn_hovered { toolbar_theme.item_text_hover.as_str() } else { toolbar_theme.item_text.as_str() };
    ctx.set_fill_color(btn_bg);
    ctx.fill_rounded_rect(btn_x, *cy, btn_w, btn_h, 4.0);
    ctx.set_font("bold 13px sans-serif");
    ctx.set_fill_color(btn_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_wizard(WizardKey::GetStarted), btn_x + btn_w / 2.0, *cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    let btn_rect = WidgetRect::new(btn_x, *cy, btn_w, btn_h);
    result.content_items.push(("wizard_get_started".to_string(), btn_rect));
    input_coordinator.register_on_layer("user_settings:wizard_get_started", btn_rect, Sense::CLICK, layer_id);
}

// =============================================================================
// Page 1 — Theme Selection
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page1_theme(
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

    // Determine active theme — fallback to "dark" if not yet selected
    let active_theme = if state.wizard_selected_theme.is_empty() {
        "dark"
    } else {
        state.wizard_selected_theme.as_str()
    };

    // Back button
    render_back_button(ctx, x, w, cy, toolbar_theme, layer_id, input_coordinator, result, hovered);
    *cy += 8.0;

    // Step indicator (top-right)
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
    ctx.set_text_align(TextAlign::Right);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(t_wizard(WizardKey::Step2of3), x + w, *cy - 20.0);

    // Title
    ctx.set_font("bold 22px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(t_wizard(WizardKey::Theme), x + w / 2.0, *cy);
    *cy += 34.0;

    // Subtitle
    ctx.set_font("14px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
    ctx.set_text_align(TextAlign::Center);
    ctx.fill_text(t_wizard(WizardKey::ChooseTheme), x + w / 2.0, *cy);
    ctx.set_text_align(TextAlign::Left);
    *cy += 28.0;

    // Theme rows: (key, display label, widget_id)
    let themes: &[(&str, &str, &str)] = &[
        ("dark",                  "Dark",                "wizard_theme_dark"),
        ("light",                 "Light",               "wizard_theme_light"),
        ("high_contrast",         "High Contrast",       "wizard_theme_high_contrast"),
        ("high_contrast_mono",    "High Contrast Mono",  "wizard_theme_high_contrast_mono"),
        ("cypherpunk",            "Cypherpunk",          "wizard_theme_cypherpunk"),
    ];

    let row_h = 40.0;
    let row_gap = 6.0;

    for (key, label, widget_id) in themes {
        let is_active = active_theme == *key;
        let is_row_hovered = hovered == Some(widget_id);

        let row_bg = if is_row_hovered {
            toolbar_theme.button_bg_hover.as_str()
        } else {
            toolbar_theme.button_bg.as_str()
        };
        ctx.set_fill_color(row_bg);
        ctx.fill_rounded_rect(x, *cy, w, row_h, 4.0);

        // Active accent left border
        if is_active {
            ctx.set_fill_color(toolbar_theme.accent.as_str());
            ctx.fill_rounded_rect(x, *cy, 3.0, row_h, 2.0);
        }

        let text_alpha = if is_active { toolbar_theme.item_text.as_str() } else { toolbar_theme.item_text_muted.as_str() };
        ctx.set_font("14px sans-serif");
        ctx.set_fill_color(text_alpha);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, x + 16.0, *cy + row_h / 2.0);

        let row_rect = WidgetRect::new(x, *cy, w, row_h);
        result.content_items.push((widget_id.to_string(), row_rect));
        let hit_id = format!("user_settings:{}", widget_id);
        input_coordinator.register_on_layer(hit_id.as_str(), row_rect, Sense::CLICK | Sense::HOVER, layer_id);

        *cy += row_h + row_gap;
    }

    *cy += 16.0;

    // "Next" button (goes to profile+passphrase page)
    let btn_h = 38.0;
    let btn_w = w.min(200.0);
    let btn_x = x + (w - btn_w) / 2.0;
    let is_btn_hovered = hovered == Some("wizard_theme_next");
    let btn_bg = if is_btn_hovered { toolbar_theme.button_bg_hover.as_str() } else { toolbar_theme.button_bg.as_str() };
    let btn_text = if is_btn_hovered { toolbar_theme.item_text_hover.as_str() } else { toolbar_theme.item_text.as_str() };
    ctx.set_fill_color(btn_bg);
    ctx.fill_rounded_rect(btn_x, *cy, btn_w, btn_h, 4.0);
    ctx.set_font("bold 13px sans-serif");
    ctx.set_fill_color(btn_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_wizard(WizardKey::Next), btn_x + btn_w / 2.0, *cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    let btn_rect = WidgetRect::new(btn_x, *cy, btn_w, btn_h);
    result.content_items.push(("wizard_theme_next".to_string(), btn_rect));
    input_coordinator.register_on_layer("user_settings:wizard_theme_next", btn_rect, Sense::CLICK, layer_id);
}

// =============================================================================
// Page 2 — Profile + Passphrase (last page)
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page2_profile(
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

    // Back button
    render_back_button(ctx, x, w, cy, toolbar_theme, layer_id, input_coordinator, result, hovered);
    *cy += 8.0;

    // Step indicator (top-right)
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
    ctx.set_text_align(TextAlign::Right);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(t_wizard(WizardKey::Step3of3), x + w, *cy - 20.0);

    // Title
    ctx.set_font("bold 20px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(t_wizard(WizardKey::ProfileAndSecurity), x + w / 2.0, *cy);
    ctx.set_text_align(TextAlign::Left);
    *cy += 30.0;

    // ── Profile Name input ────────────────────────────────────────────────────
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(t_wizard(WizardKey::ProfileName), x, *cy);
    *cy += 18.0;

    let input_h = 32.0;
    let name_rect = WidgetRect::new(x, *cy, w, input_h);
    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let name_editing = &state.new_profile_name_editing;
    let (name_sel_start, name_sel_end) = if let Some((lo, hi)) = name_editing.selection_range() {
        (Some(lo), Some(hi))
    } else {
        (None, None)
    };
    let name_config = InputConfig::new(&name_editing.text)
        .with_focused(state.new_profile_name_focused)
        .with_cursor(name_editing.cursor)
        .with_placeholder("Default")
        .with_type(InputType::Text)
        .with_selection(name_sel_start, name_sel_end);

    let name_input_result = draw_input(ctx, &name_config, WidgetState::Normal, name_rect, &widget_theme);

    result.content_items.push(("wizard_profile_name_input".to_string(), name_rect));
    result.input_char_positions.push(("wizard_profile_name_input".to_string(), name_input_result.char_x_positions.clone()));
    input_coordinator.register_on_layer("user_settings:wizard_profile_name_input", name_rect, Sense::CLICK, layer_id);

    if state.new_profile_name_focused && name_editing.is_cursor_visible(current_time_ms) {
        draw_input_cursor(
            ctx,
            name_input_result.cursor_x,
            name_input_result.cursor_y,
            name_input_result.cursor_height,
            &toolbar_theme.item_text,
        );
    }

    *cy += input_h + 14.0;

    // ── Passphrase input ──────────────────────────────────────────────────────
    *cy = render_passphrase_input(
        ctx, x, w, cy, state, text_color, toolbar_theme, frame_theme,
        current_time_ms, layer_id, input_coordinator, result,
    );

    // ── ZT container info plashka ─────────────────────────────────────────────
    let note_h = 58.0;
    ctx.set_fill_color("rgba(240,173,78,0.07)");
    ctx.fill_rounded_rect(x, *cy, w, note_h, 4.0);
    ctx.set_stroke_color("rgba(240,173,78,0.25)");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, *cy, w, note_h, 4.0);
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color("rgba(244,205,99,0.75)");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(t_wizard(WizardKey::ZtInfo1), x + 12.0, *cy + 8.0);
    ctx.fill_text(t_wizard(WizardKey::ZtInfo2), x + 12.0, *cy + 24.0);
    ctx.fill_text(t_wizard(WizardKey::ZtInfo3), x + 12.0, *cy + 40.0);
    *cy += note_h + 14.0;

    // ── Complete Setup button ─────────────────────────────────────────────────
    let profile_name = state.new_profile_name_editing.text.trim().to_string();
    let profile_name_ok = !profile_name.is_empty();
    let passphrase_ok = state.e2e_passphrase_editing.text.len() >= crate::user_manager::MIN_PASSPHRASE_LENGTH;
    let finish_disabled = !passphrase_ok || !profile_name_ok;

    let is_finish_hovered = !finish_disabled && hovered == Some("wizard_finish");
    let finish_bg = if finish_disabled {
        "rgba(244,205,99,0.20)"
    } else if is_finish_hovered {
        toolbar_theme.button_bg_hover.as_str()
    } else {
        toolbar_theme.button_bg.as_str()
    };
    let finish_text_col = if finish_disabled {
        "rgba(0,0,0,0.35)"
    } else if is_finish_hovered {
        toolbar_theme.item_text_hover.as_str()
    } else {
        toolbar_theme.item_text.as_str()
    };

    let btn_h = 36.0;
    let btn_w = w.min(220.0);
    let btn_x = x + (w - btn_w) / 2.0;
    ctx.set_fill_color(finish_bg);
    ctx.fill_rounded_rect(btn_x, *cy, btn_w, btn_h, 4.0);
    ctx.set_font("bold 13px sans-serif");
    ctx.set_fill_color(finish_text_col);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_wizard(WizardKey::GenerateRecoveryPhrase), btn_x + btn_w / 2.0, *cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    if !finish_disabled {
        let btn_rect = WidgetRect::new(btn_x, *cy, btn_w, btn_h);
        result.content_items.push(("wizard_finish".to_string(), btn_rect));
        input_coordinator.register_on_layer("user_settings:wizard_finish", btn_rect, Sense::CLICK, layer_id);
    }
}

// =============================================================================
// Shared helper widgets
// =============================================================================

/// Render the back button with SVG chevron icon.
fn render_back_button(
    ctx: &mut dyn RenderContext,
    x: f64,
    _w: f64,
    cy: &mut f64,
    toolbar_theme: &ToolbarTheme,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
    hovered_item_id: Option<&str>,
) {
    let btn_w = 76.0;
    let btn_h = 26.0;
    let is_hovered = hovered_item_id == Some("wizard_back");
    let btn_bg = if is_hovered { toolbar_theme.button_bg_hover.as_str() } else { toolbar_theme.button_bg.as_str() };
    ctx.set_fill_color(btn_bg);
    ctx.fill_rounded_rect(x, *cy, btn_w, btn_h, 4.0);
    let stroke_color = if is_hovered { toolbar_theme.item_text_muted.as_str() } else { toolbar_theme.separator.as_str() };
    ctx.set_stroke_color(stroke_color);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, *cy, btn_w, btn_h, 4.0);

    let icon_size = 12.0;
    let icon_x = x + 8.0;
    let icon_y = *cy + (btn_h - icon_size) / 2.0;
    let icon_color = if is_hovered { toolbar_theme.item_text_hover.as_str() } else { toolbar_theme.item_text.as_str() };
    draw_svg_icon(ctx, Icon::ChevronLeft.svg(), icon_x, icon_y, icon_size, icon_size, icon_color);

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(icon_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_wizard(WizardKey::Back), x + 8.0 + icon_size + 4.0, *cy + btn_h / 2.0);
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
    _text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    current_time_ms: u64,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) -> f64 {
    // Label
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(t_wizard(WizardKey::Passphrase), x, *cy);
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
        .with_placeholder(t_wizard(WizardKey::PassphrasePlaceholder))
        .with_type(InputType::Password)
        .with_selection(sel_start, sel_end);

    let input_result = draw_input(ctx, &input_config, WidgetState::Normal, input_rect, &widget_theme);

    // Register for click-to-focus
    result.content_items.push(("e2e_passphrase_input".to_string(), input_rect));
    result.input_char_positions.push(("e2e_passphrase_input".to_string(), input_result.char_x_positions.clone()));
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

    *cy += input_h + 4.0;

    // Passphrase length hint
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(t_wizard(WizardKey::MinPassphraseHint), x, *cy);

    *cy += 18.0;
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

    // Modal background + border (vault unlock)
    ctx.set_fill_color(toolbar_theme.background.as_str());
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
    ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Enter your passphrase to decrypt your profile", inner_x + inner_w / 2.0, cy);
    ctx.set_text_align(TextAlign::Left);
    cy += 24.0;

    // Passphrase input (reuse the shared helper)
    cy = render_passphrase_input(ctx, inner_x, inner_w, &mut cy, state, text_color, toolbar_theme, frame_theme, current_time_ms, &layer_id, input_coordinator, result);

    // Unlock button (disabled until passphrase is entered)
    let unlock_disabled = state.e2e_passphrase_editing.text.is_empty();
    let hovered = state.hovered_item_id.as_deref();
    let is_unlock_hovered = !unlock_disabled && hovered == Some("vault_unlock_btn");
    let btn_bg = if unlock_disabled {
        "rgba(244,205,99,0.20)"
    } else if is_unlock_hovered {
        toolbar_theme.button_bg_hover.as_str()
    } else {
        toolbar_theme.button_bg.as_str()
    };
    let btn_text_col = if unlock_disabled {
        "rgba(0,0,0,0.35)"
    } else if is_unlock_hovered {
        toolbar_theme.item_text_hover.as_str()
    } else {
        toolbar_theme.item_text.as_str()
    };
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
            toolbar_theme.item_text.as_str()
        } else {
            toolbar_theme.item_text_muted.as_str()
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
