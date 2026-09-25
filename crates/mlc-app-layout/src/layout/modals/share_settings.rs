//! Share settings overlay. Opened from the share/screenshot button's hover
//! menu. Click on those buttons still fires their current action.

use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::layout::render_chart::FrameTheme;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::ui::modal_settings::ShareSettingsState;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{
    draw_input, draw_input_cursor, render_modal_shell, InputConfig, ModalFooter, ModalPlacement,
    ModalShell, ModalShellSpec, ModalTheme, WidgetState, SETTINGS_BUTTON_RADIUS,
};

const FIELD: &str = "share_settings_nick";
const MODAL_W: f64 = 420.0;
const FIELD_H: f64 = 32.0;
const BTN_H: f64 = 30.0;
const GAP: f64 = 8.0;
const LABEL_H: f64 = 16.0;
const HIT_H: f64 = 28.0;

fn content_height(hits: usize, show_people: bool) -> f64 {
    let mut h = LABEL_H + GAP + 3.0 * (BTN_H + GAP) + LABEL_H + GAP;
    if show_people {
        let hits = hits.min(6) as f64;
        h += FIELD_H + GAP + hits * (HIT_H + 4.0);
    }
    h + LABEL_H + GAP + BTN_H
}

#[derive(Clone, Debug, Default)]
pub struct ShareSettingsResult {
    pub input_rect: WidgetRect,
    pub char_x_positions: Vec<f64>,
    pub header_rect: WidgetRect,
}

pub fn render_share_settings_overlay(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    state: &ShareSettingsState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    current_time_ms: u64,
    input_coordinator: &mut InputCoordinator,
) -> ShareSettingsResult {
    let theme = ModalTheme::from_toolbar(&frame_theme.modal_bg, &toolbar_theme.separator, toolbar_theme);
    let show_people = state.rights_preset == "private";
    let spec = ModalShellSpec {
        placement: ModalPlacement::At(state.position),
        footer: ModalFooter::None,
        ..ModalShellSpec::new(
            "share_settings",
            "Share settings",
            MODAL_W,
            content_height(state.hits.len(), show_people),
        )
    };
    let mut field: Option<(WidgetRect, Vec<f64>)> = None;
    let _shell = render_modal_shell(
        ctx,
        input_coordinator,
        &spec,
        &theme,
        (screen_w, screen_h),
        |ctx, coord, layer_id, shell| {
            field = Some(draw_body(
                ctx,
                coord,
                layer_id,
                shell,
                state,
                show_people,
                toolbar_theme,
                frame_theme,
                current_time_ms,
            ));
        },
    );
    let (input_rect, char_x_positions) = field.unwrap_or_default();
    ShareSettingsResult {
        input_rect,
        char_x_positions,
        header_rect: _shell.header,
    }
}

fn draw_body(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    shell: &ModalShell,
    state: &ShareSettingsState,
    show_people: bool,
    tt: &ToolbarTheme,
    ft: &FrameTheme,
    current_time_ms: u64,
) -> (WidgetRect, Vec<f64>) {
    let x = shell.content.x;
    let mut y = shell.content.y;
    let w = shell.content.width;

    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.fill_text("Rights for the next share. No edit.", x, y);
    y += LABEL_H + GAP;

    let presets = [
        ("public_clone", "Public + clone"),
        ("view_only", "Public, view only"),
        ("private", "Private"),
    ];
    for (id, label) in presets {
        let rect = WidgetRect::new(x, y, w, BTN_H);
        draw_secondary_button(ctx, coord, layer_id, &format!("share_settings:preset:{id}"), rect, label, tt);
        y += BTN_H + GAP;
    }

    let (field_rect, char_x_positions) = if show_people {
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.fill_text("Who can open this private link. View or clone, not edit.", x, y);
        y += LABEL_H + GAP;

        let field_rect = WidgetRect::new(x, y, w, FIELD_H);
        let wid = WidgetId::from(FIELD);
        if !coord.text_fields().is_focused(&wid) {
            let fields = coord.text_fields_mut();
            fields.begin_edit(&wid);
            fields.focus(wid.clone());
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
            .with_placeholder("Nick")
            .with_selection(sel_start, sel_end);
        let drawn = draw_input(ctx, &config, WidgetState::Normal, field_rect, &widget_theme);
        coord.register_on_layer("share_settings:input", field_rect, Sense::CLICK | Sense::DRAG, layer_id);
        if engaged && coord.text_fields().cursor_visible(current_time_ms) {
            draw_input_cursor(ctx, drawn.cursor_x, drawn.cursor_y, drawn.cursor_height, &tt.item_text);
        }
        y += FIELD_H + GAP;
        (field_rect, drawn.char_x_positions)
    } else {
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.fill_text("People are only for a private link.", x, y);
        y += LABEL_H + GAP;
        (WidgetRect::default(), Vec::new())
    };

    if show_people {
    for (idx, (nick, label)) in state.hits.iter().take(6).enumerate() {
        let name_w = w - 150.0;
        let name = WidgetRect::new(x, y, name_w, HIT_H);
        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(&tt.item_text);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        let shown = if label.is_empty() || label == nick { nick.clone() } else { format!("{nick}  {label}") };
        ctx.fill_text(&shown, name.x + 6.0, name.center_y());
        let view = WidgetRect::new(x + name_w + 6.0, y, 64.0, HIT_H);
        let clone = WidgetRect::new(x + name_w + 76.0, y, 70.0, HIT_H);
        draw_secondary_button(ctx, coord, layer_id, &format!("share_settings:grant:{idx}:view"), view, "View", tt);
        draw_secondary_button(ctx, coord, layer_id, &format!("share_settings:grant:{idx}:clone"), clone, "Clone", tt);
        y += HIT_H + 4.0;
    }
    }

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(&state.status, x, y);
    y += LABEL_H + GAP;

    let close = WidgetRect::new(x, y, w, BTN_H);
    draw_secondary_button(ctx, coord, layer_id, "share_settings:close", close, "Close", tt);

    (field_rect, char_x_positions)
}

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
