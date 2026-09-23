//! "Open shared chart…" overlay (native-share-viewer arc, 2026-09-06).
//!
//! Reached by selecting `mlc_core::toolbar::OPEN_SHARE_ACTION_ID` from the
//! `share_chart` (globe) toolbar button's camera-style double-click
//! dropdown (see that constant's own doc — the dropdown ROWS themselves
//! are a special case in `mlc-app-layout::panel_app::
//! chart_toolbar_find_dropdown_items`, mirroring `"screenshot"`/`"magnet"`).
//!
//! Pattern reused: `cloud_login.rs`'s redeem-code row — a `ModalFooter::
//! None` body drawing its OWN field + button(s) with explicit hit ids,
//! rather than the generic `ModalFooter::Creator`/`Editor` single
//! auto-button (neither can produce the two DISTINCT `share_open:submit` /
//! `share_open:cancel` ids this overlay needs). Non-draggable and
//! un-dimmed, like `preset_name_input.rs`: this is a quick "paste a link,
//! go" dialog, not a must-answer gate like sign-in.
//!
//! Signed-out users may open a share exactly like a signed-in one —
//! reading a share link is public (rights v1) — so, unlike `share_chart`'s
//! OWN single-click action, nothing here gates on `account_signed_in`.

use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::layout::render_chart::FrameTheme;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{
    draw_input, draw_input_cursor, render_modal_shell, InputConfig, ModalFooter, ModalShell,
    ModalShellSpec, ModalTheme, WidgetState, SETTINGS_BUTTON_RADIUS,
};

/// Text-field id for the share link/token box.
///
/// A literal, not a shared constant: this crate cannot depend on `mlc-app`,
/// where `text_input`'s ids live — see `user_settings.rs`'s own
/// `"account_redeem_input"` for the same split. `mlc_app::text_input::
/// SHARE_OPEN_INPUT` holds the other half of the pair.
const SHARE_OPEN_FIELD: &str = "share_open_input";

const MODAL_W: f64 = 380.0;
const FIELD_H: f64 = 32.0;
const BTN_H: f64 = 34.0;
const BTN_GAP: f64 = 10.0;
const ROW_GAP: f64 = 14.0;
const LABEL_H: f64 = 16.0;

fn content_height() -> f64 {
    LABEL_H + ROW_GAP + FIELD_H + ROW_GAP + BTN_H
}

/// Geometry the input side needs to drive the text field's caret/selection —
/// same pair every modal field in this crate hands back (see
/// `user_settings.rs`'s `render_account_nick_row` doc for why both are
/// required: without the rect+char-position pair reaching `TextFieldStore::
/// update_field`, a field has no `last_rect`, so click-to-cursor and
/// drag-select never work).
#[derive(Clone, Debug, Default)]
pub struct ShareOpenResult {
    /// The overlay's own frame — kept for a future click-outside check; the
    /// shell's auto `"share_open:modal_bg"` zone already absorbs clicks
    /// that land inside it but hit no child.
    pub modal_rect: WidgetRect,
    pub input_rect: WidgetRect,
    pub char_x_positions: Vec<f64>,
}

/// Render the "Open shared chart…" overlay.
pub fn render_share_open_overlay(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    current_time_ms: u64,
    input_coordinator: &mut InputCoordinator,
) -> ShareOpenResult {
    let theme = ModalTheme::from_toolbar(&frame_theme.modal_bg, &toolbar_theme.separator, toolbar_theme);
    let spec = ModalShellSpec {
        draggable: false,
        footer: ModalFooter::None,
        ..ModalShellSpec::new("share_open", "Open shared chart", MODAL_W, content_height())
    };

    let mut field: Option<(WidgetRect, Vec<f64>)> = None;
    let shell = render_modal_shell(
        ctx,
        input_coordinator,
        &spec,
        &theme,
        (screen_w, screen_h),
        |ctx, coord, layer_id, shell| {
            field = Some(draw_body(ctx, coord, layer_id, shell, toolbar_theme, frame_theme, &theme, current_time_ms));
        },
    );
    let (input_rect, char_x_positions) = field.unwrap_or_default();
    ShareOpenResult {
        modal_rect: shell.frame,
        input_rect,
        char_x_positions,
    }
}

fn draw_body(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    shell: &ModalShell,
    tt: &ToolbarTheme,
    ft: &FrameTheme,
    theme: &ModalTheme,
    current_time_ms: u64,
) -> (WidgetRect, Vec<f64>) {
    let x = shell.content.x;
    let y = shell.content.y;
    let w = shell.content.width;

    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.fill_text("Share link or token", x, y);

    let field_y = y + LABEL_H + ROW_GAP;
    let field_rect = WidgetRect::new(x, field_y, w, FIELD_H);
    let wid = WidgetId::from(SHARE_OPEN_FIELD);

    // Own text focus for as long as the overlay is open — same reassertion
    // `template_name_modal.rs` runs, so a stray click elsewhere between the
    // opening dropdown-select and the first keystroke can never leave the
    // field silently untypable.
    if !coord.text_fields().is_focused(&wid) {
        coord.text_fields_mut().focus(wid.clone());
    }

    let text = coord.text_fields().text(&wid).to_string();
    let cursor = coord.text_fields().cursor(&wid);
    let (sel_start, sel_end) = match coord.text_fields().selection_range(&wid) {
        Some((lo, hi)) => (Some(lo), Some(hi)),
        None => (None, None),
    };
    let engaged = coord.text_fields().is_engaged(&wid);
    let widget_theme = toolbar_to_widget_theme(tt, ft);
    let config = InputConfig::new(&text)
        .with_focused(engaged)
        .with_cursor(cursor)
        .with_placeholder("Paste a link or token…")
        .with_selection(sel_start, sel_end);
    let drawn = draw_input(ctx, &config, WidgetState::Normal, field_rect, &widget_theme);
    coord.register_on_layer("share_open:input", field_rect, Sense::CLICK, layer_id);
    if engaged && coord.text_fields().cursor_visible(current_time_ms) {
        draw_input_cursor(ctx, drawn.cursor_x, drawn.cursor_y, drawn.cursor_height, &tt.item_text);
    }

    let btn_y = field_y + FIELD_H + ROW_GAP;
    let btn_w = (w - BTN_GAP) / 2.0;
    let cancel_rect = WidgetRect::new(x, btn_y, btn_w, BTN_H);
    let submit_rect = WidgetRect::new(x + btn_w + BTN_GAP, btn_y, btn_w, BTN_H);

    draw_secondary_button(ctx, coord, layer_id, "share_open:cancel", cancel_rect, "Cancel", tt);
    draw_primary_button(ctx, coord, layer_id, "share_open:submit", submit_rect, "Open", theme);

    (field_rect, drawn.char_x_positions)
}

/// Bordered, theme-neutral button — the redeem dialog's `BtnStyle::Secondary`.
fn draw_secondary_button(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    widget_id: &str,
    rect: WidgetRect,
    label: &str,
    tt: &ToolbarTheme,
) {
    let hovered = coord.is_hovered(&WidgetId::from(widget_id));
    let bg = if hovered { tt.button_bg_hover.as_str() } else { tt.dropdown_bg.as_str() };
    ctx.set_fill_color(bg);
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    ctx.set_stroke_color(&tt.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, rect.center_x(), rect.center_y());
    coord.register_on_layer(widget_id, rect, Sense::CLICK | Sense::HOVER, layer_id);
}

/// Accent-filled CTA button — the redeem dialog's `BtnStyle::Primary`, using
/// the modal theme's own accent instead of the sign-in flow's brand gold
/// (this overlay is an ordinary app dialog, not the sign-in gate).
fn draw_primary_button(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    widget_id: &str,
    rect: WidgetRect,
    label: &str,
    theme: &ModalTheme,
) {
    let hovered = coord.is_hovered(&WidgetId::from(widget_id));
    let bg = if hovered { &theme.accent_hover } else { &theme.accent };
    ctx.set_fill_color(bg);
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color(&theme.accent_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, rect.center_x(), rect.center_y());
    coord.register_on_layer(widget_id, rect, Sense::CLICK | Sense::HOVER, layer_id);
}
