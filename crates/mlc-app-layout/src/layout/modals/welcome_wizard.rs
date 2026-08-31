//! First-run Welcome Wizard modal.
//!
//! An ordinary dismissible modal over a live chart — same [`begin_modal_shell`]
//! every other modal uses. Owner's ruling 2026-08-31: theme first, then
//! language, then a name — the wizard is a first-run courtesy, not a gate.
//! Dismissing keeps whatever profile boot already created; it just skips the
//! guided tour. A tutorial is planned for this same slot later.
//!
//! Owner's ruling 2026-08-31 (second pass, same day): the passphrase page is
//! dropped from this flow entirely — the passphrase belongs to the vault,
//! which is opened later at the point of use (profile manager), not at
//! first-run setup. The wizard now ends the moment a name is picked; that is
//! the whole flow. `profile_manager.rs` owns the vault/passphrase/recovery-key
//! UI on its own, separate render path — nothing here reaches into it, and
//! nothing there reached into this file either (each had its own private
//! `render_back_button`/`render_passphrase_input`), so dropping this file's
//! copies does not touch the profile manager's passphrase flow at all.
//!
//! Pages:
//!   0 — Theme         (`RuntimeTheme::PRESETS_SHORTLIST` — 6 presets)
//!   1 — Language       (mascot, lang selection)
//!   2 — Profile Name   (name input only — ends the wizard)

use mlc_core::engine::render::{draw_svg_icon, draw_svg_multicolor, RenderContext};
use mlc_core::i18n::{Language, WizardKey, current_language, t_wizard};
use mlc_core::theme::RuntimeTheme;
use uzor::render::{TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;
use uzor::input::Sense;
use crate::ui::modal_settings::UserSettingsState;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::layout::render_frame::UserSettingsResult;
use crate::ui::z_order::ZLayer;
use crate::ui::widgets::{
    begin_modal_shell, end_modal_shell, register_dimmer_zones, draw_input, draw_input_cursor,
    InputConfig, InputType, ModalFooter, ModalPlacement, ModalShellSpec, ModalTheme,
    theme_sample_tokens, draw_theme_sample,
};
use crate::ui::widgets::types::WidgetState;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::layout::render_chart::FrameTheme;
use crate::ui::icons::Icon;

const MINI_MASCOT_SVG: &str = include_str!("../../../../../assets/mascot/mini_mascot.svg");

const MODAL_W: f64 = 580.0;

/// Content height per page. The big in-body title each page used to draw is
/// dropped — the shell header carries it now. Each arm sums the exact cy
/// deltas the matching render fn below produces, so the modal frame is sized
/// to its content instead of guessed.
fn page_content_height(state: &UserSettingsState) -> f64 {
    match state.wizard_page {
        // Theme: subtitle(28) + N rows*(40+6) + gap(16) + button(38), N =
        // `RuntimeTheme::PRESETS_SHORTLIST.len()`.
        0 => theme_page_content_height(),
        // Language: back+gap(34) + mascot(100) + N lang rows + gap(12) + button(38).
        1 => {
            let lang_count = Language::all().len() as f64;
            let header = 26.0 + 8.0;
            let mascot = 80.0 + 20.0;
            let langs = lang_count * (32.0 + 4.0);
            header + mascot + langs + 12.0 + 38.0
        }
        // Profile Name: back+gap(34) + pre-input gap(12) + input(32) + gap(24) + button(38).
        2 => 140.0,
        _ => theme_page_content_height(),
    }
}

/// Content height of the theme page (0), shared with the `_` fallback arm
/// above (which also renders the theme page — see `render_welcome_wizard`).
fn theme_page_content_height() -> f64 {
    let theme_count = RuntimeTheme::PRESETS_SHORTLIST.len() as f64;
    28.0 + theme_count * (40.0 + 6.0) + 16.0 + 38.0
}

fn page_title(state: &UserSettingsState) -> String {
    match state.wizard_page {
        0 => t_wizard(WizardKey::Theme).to_string(),
        1 => format!("{} mylittlechart", t_wizard(WizardKey::WelcomeTo)),
        2 => t_wizard(WizardKey::ProfileName).to_string(),
        _ => t_wizard(WizardKey::Theme).to_string(),
    }
}

/// Render the Welcome Wizard modal.
///
/// `window_w/window_h` are the full window dimensions — the shell centers
/// and clamps the frame within them, same as every other modal.
#[allow(clippy::too_many_arguments)]
pub fn render_welcome_wizard(
    ctx: &mut dyn RenderContext,
    window_w: f64,
    window_h: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    // Unused since the passphrase/recovery pages (the only ones with a
    // blinking-cursor timing need) left this file — kept in the signature
    // because this is a `pub fn` called from `chart_panel_app.rs` alongside
    // every other modal on a uniform argument list.
    _current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    let title = page_title(state);
    let content_h = page_content_height(state);

    let theme = ModalTheme::from_toolbar(&frame_theme.modal_bg, &toolbar_theme.separator, toolbar_theme);
    let spec = ModalShellSpec {
        dim_backdrop: true,
        footer: ModalFooter::None,
        // A first-run courtesy, not a gate — always dismissible, unlike
        // `profile_manager`'s mandatory-migration exception.
        show_close: true,
        draggable: false,
        placement: ModalPlacement::Center,
        layer: ZLayer::ModalOverlay,
        ..ModalShellSpec::new("welcome_wizard", &title, MODAL_W, content_h)
    };
    let (shell, layer_id) =
        begin_modal_shell(ctx, input_coordinator, &spec, &theme, (window_w, window_h));
    register_dimmer_zones(input_coordinator, &layer_id, "welcome_wizard", shell.frame, (window_w, window_h));

    result.modal_rect = shell.frame;
    result.header_rect = shell.header;
    result.close_btn_rect = shell.close_btn.unwrap_or_default();

    let (x, w) = (shell.content.x, shell.content.width);
    let mut cy = shell.content.y;

    match state.wizard_page {
        0 => render_page_theme(ctx, x, w, &mut cy, state, text_color, toolbar_theme, input_coordinator, &layer_id, result),
        1 => render_page_language(ctx, x, w, &mut cy, state, text_color, toolbar_theme, input_coordinator, &layer_id, result),
        2 => render_page_profile_name(ctx, x, w, &mut cy, state, text_color, toolbar_theme, frame_theme, input_coordinator, &layer_id, result),
        _ => render_page_theme(ctx, x, w, &mut cy, state, text_color, toolbar_theme, input_coordinator, &layer_id, result),
    }

    end_modal_shell(ctx, input_coordinator, &spec, &theme, &shell, &layer_id, &[]);
}

// =============================================================================
// Page 0 — Theme (first page: no back button, no step indicator)
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page_theme(
    ctx: &mut dyn RenderContext,
    x: f64,
    w: f64,
    cy: &mut f64,
    state: &UserSettingsState,
    _text_color: &str,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();

    let active_theme = if state.wizard_selected_theme.is_empty() {
        "dark"
    } else {
        state.wizard_selected_theme.as_str()
    };

    // Subtitle (the title lives in the shell header now). First page — no
    // back button, no step indicator, matching the wizard's own convention
    // that the entry page never shows either.
    ctx.set_font("14px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(t_wizard(WizardKey::ChooseTheme), x + w / 2.0, *cy);
    ctx.set_text_align(TextAlign::Left);
    *cy += 28.0;

    // Reads the shared shortlist (`RuntimeTheme::PRESETS_SHORTLIST`) instead
    // of a hand-written id list — owner review 2026-08-31: `vaporwave`/
    // `terminal` are ugly and must not show here (or in the toolbar's Theme
    // dropdown), but `surf` stays visible everywhere, so both surfaces read
    // from the same source instead of drifting.
    let row_h = 40.0;
    let row_gap = 6.0;
    let swatch_size = 16.0;

    for key in RuntimeTheme::PRESETS_SHORTLIST {
        let label = wizard_theme_label(key);
        let widget_id = format!("wizard_theme_{key}");
        let is_active = active_theme == *key;
        let is_row_hovered = hovered == Some(widget_id.as_str());

        let row_bg = if is_row_hovered {
            toolbar_theme.button_bg_hover.as_str()
        } else {
            toolbar_theme.button_bg.as_str()
        };
        ctx.set_fill_color(row_bg);
        ctx.fill_rounded_rect(x, *cy, w, row_h, 4.0);

        if is_active {
            ctx.set_fill_color(toolbar_theme.accent.as_str());
            ctx.fill_rounded_rect(x, *cy, 3.0, row_h, 2.0);
        }

        // Background / accent / text sample, pulled from the preset itself
        // so it actually tells presets apart before clicking.
        let swatch_x = x + 12.0;
        let swatch_y = *cy + (row_h - swatch_size) / 2.0;
        let sample_tokens = theme_sample_tokens(key);
        draw_theme_sample(ctx, swatch_x, swatch_y, swatch_size, &sample_tokens);

        let text_alpha = if is_active { toolbar_theme.item_text.as_str() } else { toolbar_theme.item_text_muted.as_str() };
        ctx.set_font("14px sans-serif");
        ctx.set_fill_color(text_alpha);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, swatch_x + swatch_size + 10.0, *cy + row_h / 2.0);

        let row_rect = WidgetRect::new(x, *cy, w, row_h);
        result.content_items.push((widget_id.clone(), row_rect));
        let hit_id = format!("user_settings:{}", widget_id);
        input_coordinator.register_on_layer(hit_id.as_str(), row_rect, Sense::CLICK | Sense::HOVER, layer_id);

        *cy += row_h + row_gap;
    }

    *cy += 16.0;

    // "Next" button (goes to the language page)
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

/// Display label for a `RuntimeTheme::PRESETS_SHORTLIST` entry on the
/// wizard's theme page. English literals, matching this page's pre-existing
/// convention (unlike the rest of the wizard, this page never routed its
/// theme names through `t_wizard`/`t_settings`) — not something this change
/// widens the scope of.
fn wizard_theme_label(preset_id: &str) -> &'static str {
    match preset_id {
        "dark" => "Dark",
        "light" => "Light",
        "high_contrast" => "High Contrast",
        "high_contrast_mono" => "High Contrast Mono",
        "mascot" => "Wizard Hat",
        // 8-token skin preset — proper noun, not translated.
        "surf" => "Surf",
        // Defensive: PRESETS_SHORTLIST only ever names the ids above.
        other => {
            debug_assert!(false, "wizard_theme_label: no label for preset id {other:?}");
            "Theme"
        }
    }
}

// =============================================================================
// Page 1 — Language (mascot + language list)
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page_language(
    ctx: &mut dyn RenderContext,
    x: f64,
    w: f64,
    cy: &mut f64,
    state: &UserSettingsState,
    _text_color: &str,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();
    let active_lang = current_language();

    render_back_button(ctx, x, w, cy, toolbar_theme, layer_id, input_coordinator, result, hovered);
    *cy += 8.0;

    // Step indicator (top-right — shares the back button's row).
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
    ctx.set_text_align(TextAlign::Right);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(t_wizard(WizardKey::Step2of3), x + w, *cy - 20.0);
    ctx.set_text_align(TextAlign::Left);

    // Mini mascot centered.
    let mascot_size = 80.0;
    let mascot_x = x + (w - mascot_size) / 2.0;
    draw_svg_multicolor(ctx, MINI_MASCOT_SVG, mascot_x, *cy, mascot_size, mascot_size);
    *cy += mascot_size + 20.0;

    // Language selection rows
    let langs: Vec<(Language, String)> = Language::all().iter()
        .map(|l| (*l, format!("wizard_lang_{}", l.code())))
        .collect();

    let row_h = 32.0;
    let row_gap = 4.0;

    for (lang, widget_id) in &langs {
        let is_active = active_lang == *lang;
        let is_row_hovered = hovered == Some(widget_id.as_str());

        let row_bg = if is_row_hovered {
            toolbar_theme.button_bg_hover.as_str()
        } else {
            toolbar_theme.button_bg.as_str()
        };
        ctx.set_fill_color(row_bg);
        ctx.fill_rounded_rect(x, *cy, w, row_h, 4.0);

        if is_active {
            ctx.set_fill_color(toolbar_theme.accent.as_str());
            ctx.fill_rounded_rect(x, *cy, 3.0, row_h, 2.0);
        }

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

    // "Next" button (goes to the profile-name page)
    let btn_h = 38.0;
    let btn_w = w.min(200.0);
    let btn_x = x + (w - btn_w) / 2.0;
    let is_btn_hovered = hovered == Some("wizard_lang_next");
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
    result.content_items.push(("wizard_lang_next".to_string(), btn_rect));
    input_coordinator.register_on_layer("user_settings:wizard_lang_next", btn_rect, Sense::CLICK, layer_id);
}

// =============================================================================
// Page 2 — Profile Name (last page — picking a name ends the wizard)
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page_profile_name(
    ctx: &mut dyn RenderContext,
    x: f64,
    w: f64,
    cy: &mut f64,
    state: &UserSettingsState,
    _text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();

    render_back_button(ctx, x, w, cy, toolbar_theme, layer_id, input_coordinator, result, hovered);
    *cy += 8.0;

    // Step indicator (top-right — shares the back button's row).
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
    ctx.set_text_align(TextAlign::Right);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(t_wizard(WizardKey::Step3of3), x + w, *cy - 20.0);
    ctx.set_text_align(TextAlign::Left);
    *cy += 12.0;

    // Profile Name input — the shell header already reads "Profile Name",
    // so the field itself carries no separate label, only the placeholder.
    let input_h = 32.0;
    let name_rect = WidgetRect::new(x, *cy, w, input_h);
    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let wz_npn_render = &state.new_profile_name_render;
    let (name_sel_start, name_sel_end) = if let Some((lo, hi)) = wz_npn_render.selection_range() {
        (Some(lo), Some(hi))
    } else {
        (None, None)
    };
    let name_config = InputConfig::new(&wz_npn_render.text)
        .with_focused(state.new_profile_name_focused)
        .with_cursor(wz_npn_render.cursor)
        .with_placeholder("Default")
        .with_type(InputType::Text)
        .with_selection(name_sel_start, name_sel_end);

    let name_input_result = draw_input(ctx, &name_config, WidgetState::Normal, name_rect, &widget_theme);

    result.content_items.push(("wizard_profile_name_input".to_string(), name_rect));
    result.input_char_positions.push(("wizard_profile_name_input".to_string(), name_input_result.char_x_positions.clone()));
    input_coordinator.register_on_layer("user_settings:wizard_profile_name_input", name_rect, Sense::CLICK, layer_id);

    if state.new_profile_name_focused && wz_npn_render.cursor_visible {
        draw_input_cursor(
            ctx,
            name_input_result.cursor_x,
            name_input_result.cursor_y,
            name_input_result.cursor_height,
            &toolbar_theme.item_text,
        );
    }

    *cy += input_h + 24.0;

    // "Get Started" button — the whole flow: name the profile and start
    // using the app. No passphrase step here (owner's ruling 2026-08-31):
    // the vault opens later, at the point of use, via the profile manager.
    let profile_name = state.new_profile_name_render.text.trim().to_string();
    let finish_disabled = profile_name.is_empty();

    let is_finish_hovered = !finish_disabled && hovered == Some("wizard_finish");
    let finish_bg = if is_finish_hovered { toolbar_theme.button_bg_hover.as_str() } else { toolbar_theme.button_bg.as_str() };
    let finish_text_col = if finish_disabled {
        toolbar_theme.item_text_muted.as_str()
    } else if is_finish_hovered {
        toolbar_theme.item_text_hover.as_str()
    } else {
        toolbar_theme.item_text.as_str()
    };

    let btn_h = 38.0;
    let btn_w = w.min(200.0);
    let btn_x = x + (w - btn_w) / 2.0;
    ctx.set_fill_color(finish_bg);
    ctx.fill_rounded_rect(btn_x, *cy, btn_w, btn_h, 4.0);
    ctx.set_font("bold 13px sans-serif");
    ctx.set_fill_color(finish_text_col);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_wizard(WizardKey::GetStarted), btn_x + btn_w / 2.0, *cy + btn_h / 2.0);
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
