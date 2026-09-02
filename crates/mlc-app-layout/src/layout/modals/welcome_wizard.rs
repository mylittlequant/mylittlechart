//! First-run Welcome Wizard modal.
//!
//! An ordinary dismissible modal over a live chart — same [`begin_modal_shell`]
//! every other modal uses. Owner's ruling 2026-08-31: theme first, then
//! language — the wizard is a first-run courtesy, not a gate. Dismissing
//! keeps whatever profile boot already created; it just skips the guided
//! tour. A tutorial is planned for this same slot later.
//!
//! Owner's ruling 2026-08-31 (second pass, same day): the passphrase page is
//! dropped from this flow entirely — the passphrase belongs to the vault,
//! which is opened later at the point of use (profile manager), not at
//! first-run setup. `profile_manager.rs` owns the vault/passphrase/recovery-key
//! UI on its own, separate render path — nothing here reaches into it, and
//! nothing there reached into this file either (each had its own private
//! `render_back_button`/`render_passphrase_input`), so dropping this file's
//! copies does not touch the profile manager's passphrase flow at all.
//!
//! Owner's identity ruling 2026-09-02: a profile is a local workspace whose
//! identity is its UUID, and an anonymous profile needs no user-facing name
//! — not even a default one. A nickname is an ACCOUNT attribute obtained
//! through OAuth, never a profile name. So the wizard's Profile Name page is
//! gone — there is nothing left to name at first run. The Language page is
//! now the last page; its forward button ends the wizard directly.
//!
//! Pages:
//!   0 — Theme         (`RuntimeTheme::PRESETS_SHORTLIST` — 6 presets, grid)
//!   1 — Language       (`Language::all()` — 15 languages, grid; no mascot —
//!                       dropped 2026-08-31, owner: it only ate space; last
//!                       page — its forward button ends the wizard)

use mlc_core::engine::render::{draw_svg_icon, RenderContext};
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
    begin_modal_shell, end_modal_shell, register_dimmer_zones,
    ModalFooter, ModalPlacement, ModalShellSpec, ModalTheme,
    theme_palette_tokens, theme_palette_width, draw_theme_palette,
};
use crate::layout::render_chart::FrameTheme;
use crate::ui::icons::Icon;

/// Widened 580→640 on 2026-09-02: each theme row now carries a
/// five-chip palette strip on its right edge (`THEME_PALETTE_*`), and at
/// 580 the strip and the longest label ("High Contrast Mono") fought for
/// the same pixels in a two-column cell.
const MODAL_W: f64 = 640.0;

/// Columns in the theme page's preset grid (page 0). `RuntimeTheme::
/// PRESETS_SHORTLIST` is 6 entries long — 2 columns keeps each cell wide
/// enough for the longest label ("High Contrast Mono") without measuring
/// text width at layout time, at 3 rows instead of 6.
const THEME_GRID_COLS: usize = 2;
const THEME_ROW_H: f64 = 40.0;
const THEME_ROW_GAP: f64 = 6.0;
const THEME_COL_GAP: f64 = 16.0;

/// Palette strip geometry for a theme row. The strip is right-aligned in the
/// cell, so the label keeps everything between the radio and the strip and
/// rows stay aligned with each other regardless of label length.
const THEME_CHIP: f64 = 12.0;
const THEME_CHIP_GAP: f64 = 4.0;
/// Radius of the selection radio drawn at the head of a theme row.
const THEME_RADIO_R: f64 = 7.0;
/// Left inset shared by the radio and the right-hand palette strip.
const THEME_ROW_PAD: f64 = 12.0;

/// Columns in the language page's grid (page 1). `Language::all()` is 15
/// entries, all single native-script words — 3 columns fits comfortably and
/// cuts 15 stacked rows down to 5.
const LANG_GRID_COLS: usize = 3;
const LANG_ROW_H: f64 = 32.0;
const LANG_ROW_GAP: f64 = 4.0;
const LANG_COL_GAP: f64 = 16.0;

/// Content height per page. The big in-body title each page used to draw is
/// dropped — the shell header carries it now. Each arm sums the exact cy
/// deltas the matching render fn below produces, so the modal frame is sized
/// to its content instead of guessed.
fn page_content_height(state: &UserSettingsState) -> f64 {
    match state.wizard_page {
        // Theme: subtitle(28) + rows*(40+6) + gap(16) + button(38), rows =
        // ceil(`RuntimeTheme::PRESETS_SHORTLIST.len()` / `THEME_GRID_COLS`).
        0 => theme_page_content_height(),
        // Language (last page — its button ends the wizard): back+gap(34) +
        // rows*(32+4) + gap(12) + button(38), rows =
        // ceil(`Language::all().len()` / `LANG_GRID_COLS`). No mascot term —
        // the mascot was dropped from this page (owner 2026-08-31: it only
        // ate space; the boot splash keeps its own, separate mascot).
        1 => {
            let lang_count = Language::all().len();
            let rows = lang_count.div_ceil(LANG_GRID_COLS) as f64;
            let header = 26.0 + 8.0;
            let langs = rows * (LANG_ROW_H + LANG_ROW_GAP);
            header + langs + 12.0 + 38.0
        }
        _ => theme_page_content_height(),
    }
}

/// Content height of the theme page (0), shared with the `_` fallback arm
/// above (which also renders the theme page — see `render_welcome_wizard`).
fn theme_page_content_height() -> f64 {
    let theme_count = RuntimeTheme::PRESETS_SHORTLIST.len();
    let rows = theme_count.div_ceil(THEME_GRID_COLS) as f64;
    28.0 + rows * (THEME_ROW_H + THEME_ROW_GAP) + 16.0 + 38.0
}

fn page_title(state: &UserSettingsState) -> String {
    match state.wizard_page {
        0 => t_wizard(WizardKey::Theme).to_string(),
        1 => format!("{} mylittlechart", t_wizard(WizardKey::WelcomeTo)),
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
        // Draggable by its header like every other modal (owner 2026-09-02:
        // it would not move on native). Nothing behind it needs protecting —
        // the chart is live and the wizard is a courtesy, so letting the user
        // push it aside to look at what they are theming is the point.
        //
        // `At(state.wizard_position)`, not `Center`: `draggable: true` alone
        // only registers the header rect with `Sense::DRAG` — nothing then
        // STORES or APPLIES a dragged position, which is exactly why it used
        // to look draggable and never move. `wizard_position` is a field of
        // `UserSettingsState` separate from that struct's own
        // `position`/`is_dragging`/`drag_offset` (those back the User
        // Settings modal itself — see the field doc on `wizard_position`),
        // `None` until the user actually drags, which is what keeps the
        // wizard centred on open.
        draggable: true,
        placement: ModalPlacement::At(state.wizard_position),
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
    //
    // Grid, not a single stacked column (owner 2026-08-31: "themes read fine
    // as a compact grid") — `THEME_GRID_COLS` columns, filled row-major.
    // Swatch bumped 16→20 and squared up (see `theme_sample.rs`) per the
    // same review: the old 3-vertical-bar sample read as "narrow bars", not
    // a color sample.
    let row_h = THEME_ROW_H;
    let row_gap = THEME_ROW_GAP;
    let col_gap = THEME_COL_GAP;
    let col_w = (w - col_gap * (THEME_GRID_COLS - 1) as f64) / THEME_GRID_COLS as f64;
    let grid_top = *cy;

    for (i, key) in RuntimeTheme::PRESETS_SHORTLIST.iter().enumerate() {
        let col = i % THEME_GRID_COLS;
        let row = i / THEME_GRID_COLS;
        let cell_x = x + col as f64 * (col_w + col_gap);
        let cell_y = grid_top + row as f64 * (row_h + row_gap);

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
        ctx.fill_rounded_rect(cell_x, cell_y, col_w, row_h, 4.0);

        if is_active {
            ctx.set_fill_color(toolbar_theme.accent.as_str());
            ctx.fill_rounded_rect(cell_x, cell_y, 3.0, row_h, 2.0);
        }

        // Selection radio. Owner review 2026-09-02: the old head-of-row mark
        // was the colour sample itself — a rounded square with an accent dot
        // inset in it, which read as "a checkbox with a ball inside" and left
        // the row with no honest selected/unselected control at all (only an
        // accent stripe and slightly brighter text said which one was picked).
        // Same ring-plus-inner-dot shape `draw_radio_group` uses, so a
        // single-choice list looks like a single-choice list everywhere.
        let radio_cx = cell_x + THEME_ROW_PAD + THEME_RADIO_R;
        let radio_cy = cell_y + row_h / 2.0;
        ctx.begin_path();
        ctx.arc(radio_cx, radio_cy, THEME_RADIO_R, 0.0, std::f64::consts::TAU);
        ctx.set_stroke_color(if is_active {
            toolbar_theme.accent.as_str()
        } else {
            toolbar_theme.separator.as_str()
        });
        ctx.set_stroke_width(1.5);
        ctx.stroke();
        if is_active {
            ctx.begin_path();
            ctx.arc(radio_cx, radio_cy, THEME_RADIO_R - 3.5, 0.0, std::f64::consts::TAU);
            ctx.set_fill_color(toolbar_theme.accent.as_str());
            ctx.fill();
        }

        // The preset's own palette, right-aligned so the strips line up in a
        // column and can be compared down the list rather than read one row
        // at a time.
        let strip_w = theme_palette_width(THEME_CHIP, THEME_CHIP_GAP);
        let strip_x = cell_x + col_w - THEME_ROW_PAD - strip_w;
        let strip_y = cell_y + (row_h - THEME_CHIP) / 2.0;
        let palette = theme_palette_tokens(key);
        draw_theme_palette(
            ctx,
            strip_x,
            strip_y,
            THEME_CHIP,
            THEME_CHIP_GAP,
            toolbar_theme.separator.as_str(),
            &palette,
        );

        let text_alpha = if is_active { toolbar_theme.item_text.as_str() } else { toolbar_theme.item_text_muted.as_str() };
        ctx.set_font("14px sans-serif");
        ctx.set_fill_color(text_alpha);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, radio_cx + THEME_RADIO_R + 10.0, cell_y + row_h / 2.0);

        let row_rect = WidgetRect::new(cell_x, cell_y, col_w, row_h);
        result.content_items.push((widget_id.clone(), row_rect));
        let hit_id = format!("user_settings:{}", widget_id);
        input_coordinator.register_on_layer(hit_id.as_str(), row_rect, Sense::CLICK | Sense::HOVER, layer_id);
    }

    let grid_rows = RuntimeTheme::PRESETS_SHORTLIST.len().div_ceil(THEME_GRID_COLS) as f64;
    *cy = grid_top + grid_rows * (row_h + row_gap) + 16.0;

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
// Page 1 — Language (language grid, no mascot; last page — the forward
// button ends the wizard, see the identity ruling in the module doc above)
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

    // No mascot on this page (owner 2026-08-31: it only ate space — the boot
    // splash keeps its own, separate mascot; this page never reused that
    // asset, so dropping it here does not touch the splash).
    //
    // Language selection grid — `LANG_GRID_COLS` columns, filled row-major.
    // 15 single-word native names is the bulk of this page's height when
    // stacked one per row (owner 2026-08-31); a grid cuts it to 5 rows.
    let langs: Vec<(Language, String)> = Language::all().iter()
        .map(|l| (*l, format!("wizard_lang_{}", l.code())))
        .collect();

    let row_h = LANG_ROW_H;
    let row_gap = LANG_ROW_GAP;
    let col_gap = LANG_COL_GAP;
    let col_w = (w - col_gap * (LANG_GRID_COLS - 1) as f64) / LANG_GRID_COLS as f64;
    let grid_top = *cy;

    for (i, (lang, widget_id)) in langs.iter().enumerate() {
        let col = i % LANG_GRID_COLS;
        let row = i / LANG_GRID_COLS;
        let cell_x = x + col as f64 * (col_w + col_gap);
        let cell_y = grid_top + row as f64 * (row_h + row_gap);

        let is_active = active_lang == *lang;
        let is_row_hovered = hovered == Some(widget_id.as_str());

        let row_bg = if is_row_hovered {
            toolbar_theme.button_bg_hover.as_str()
        } else {
            toolbar_theme.button_bg.as_str()
        };
        ctx.set_fill_color(row_bg);
        ctx.fill_rounded_rect(cell_x, cell_y, col_w, row_h, 4.0);

        if is_active {
            ctx.set_fill_color(toolbar_theme.accent.as_str());
            ctx.fill_rounded_rect(cell_x, cell_y, 3.0, row_h, 2.0);
        }

        let text_alpha = if is_active { toolbar_theme.item_text.as_str() } else { toolbar_theme.item_text_muted.as_str() };
        ctx.set_font("14px sans-serif");
        ctx.set_fill_color(text_alpha);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(lang.native_name(), cell_x + 16.0, cell_y + row_h / 2.0);

        let row_rect = WidgetRect::new(cell_x, cell_y, col_w, row_h);
        result.content_items.push((widget_id.to_string(), row_rect));
        let hit_id = format!("user_settings:{}", widget_id);
        input_coordinator.register_on_layer(hit_id.as_str(), row_rect, Sense::CLICK | Sense::HOVER, layer_id);
    }

    let grid_rows = langs.len().div_ceil(LANG_GRID_COLS) as f64;
    *cy = grid_top + grid_rows * (row_h + row_gap) + 12.0;

    // "Get Started" button — last page now, so this is the finish action
    // the dropped Profile Name page's own finish button used to perform:
    // ends the wizard (theme + language stay applied, setup marked done).
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
    ctx.fill_text(t_wizard(WizardKey::GetStarted), btn_x + btn_w / 2.0, *cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    let btn_rect = WidgetRect::new(btn_x, *cy, btn_w, btn_h);
    result.content_items.push(("wizard_lang_next".to_string(), btn_rect));
    input_coordinator.register_on_layer("user_settings:wizard_lang_next", btn_rect, Sense::CLICK, layer_id);
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
