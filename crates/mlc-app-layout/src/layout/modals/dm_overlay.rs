//! Direct messages (paid, end-to-end encrypted) — what `NewsTab::Messages`
//! draws. Sibling of `cloud_overlay.rs` under `layout::modals`; reuses that
//! file's shared row/status/scroll helpers (`pub(crate)`). State lives in
//! `CloudOverlayState::dm` (`DmState`); see that type's own doc for the
//! plaintext-only contract (the shell decrypts before this crate ever sees
//! a message). Pure presentation — effects live in the shell driver; click
//! dispatch lives in `mlc-app`'s `panel_click.rs` (out of scope here).

use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::layout::render_chart::FrameTheme;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::ui::modal_settings::{CloudOverlayState, DmConversationUi, DmMessageUi, DmState, DmView};
use crate::ui::scroll_widget::ScrollableContainer;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig, SETTINGS_BUTTON_RADIUS};
use crate::ui::widgets::types::{WidgetState, WidgetTheme};
use crate::ui::Icon;

use super::cloud_overlay::{
    draw_action_row, draw_status, register_scrollbar_hit_zones, relative_time, scroll_config,
    wrap_text, CloudOverlayResult, FIELD_H, GOLD, ROW_GAP, ROW_H,
};

const CONV_ROW_H: f64 = 52.0;
/// Message bubble text wraps at this many characters — narrower than the
/// Forum body's own 62 (`forum_v2_overlay`'s `blocks`) since a bubble only
/// takes up [`BUBBLE_WIDTH_FRAC`] of the column, not the full width.
const BUBBLE_WRAP_COLS: usize = 40;
const BUBBLE_WIDTH_FRAC: f64 = 0.72;
const BUBBLE_MIN_W: f64 = 120.0;

// =============================================================================
// Entry points — mirror `forum_v2_overlay`'s own `height`/`draw` shape.
// =============================================================================

pub(crate) fn height(state: &CloudOverlayState) -> f64 {
    match &state.dm.view {
        DmView::List => list_h(&state.dm),
        DmView::Conversation { .. } => conversation_h(&state.dm),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    viewport_h: f64,
    state: &CloudOverlayState,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) {
    let drawn_h = height(state);
    let viewport_rect = WidgetRect::new(x, y, w, viewport_h);
    coord.register_on_layer("cloud_overlay:scroll_viewport", viewport_rect, Sense::SCROLL, layer_id);
    let scrollable = ScrollableContainer::new(viewport_rect, &state.scroll, Some(scroll_config()));
    scrollable.begin(ctx);
    let sx = x;
    let sy = scrollable.content_y();
    let sw = scrollable.content_width();

    match &state.dm.view {
        DmView::List => draw_list(ctx, coord, layer_id, sx, sy, sw, state, current_time_ms, tt),
        DmView::Conversation { id, messages, draft_field_id } => draw_conversation(
            ctx, coord, layer_id, sx, sy, sw, state, *id, messages, draft_field_id, ft, tt, current_time_ms, result,
        ),
    }

    let widget_theme = WidgetTheme::default();
    let scroll_result = scrollable.end(ctx, drawn_h, &widget_theme);
    result.total_content_height = Some(drawn_h);
    result.viewport_height = Some(viewport_h);
    result.scroll_viewport_rect = Some(viewport_rect);
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
    register_scrollbar_hit_zones(coord, layer_id, &scroll_result);
}

/// One clipped red error line — mirrors `forum_v2_overlay`'s own
/// `draw_error_line`; not shared because pulling one function up to
/// `cloud_overlay.rs` for two four-line call sites is not worth the extra
/// public surface.
fn draw_error_line(ctx: &mut dyn RenderContext, w: f64, x: f64, y: f64, msg: &str) -> f64 {
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color("#e5484d");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let m = crate::ui::widgets::fit_text_to_width(ctx, msg, w);
    ctx.fill_text(&m, x, y);
    20.0
}

fn draw_field(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    widget_id: &str,
    field_id: &str,
    rect: WidgetRect,
    placeholder: &str,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
) -> (WidgetRect, Vec<f64>) {
    let wid = WidgetId::from(field_id);
    let text = coord.text_fields().text(&wid).to_string();
    let cursor = coord.text_fields().cursor(&wid);
    let (sel_start, sel_end) = match coord.text_fields().selection_range(&wid) {
        Some((lo, hi)) => (Some(lo), Some(hi)),
        None => (None, None),
    };
    let engaged = coord.text_fields().is_engaged(&wid);
    let widget_theme = toolbar_to_widget_theme(tt, ft);
    let cfg = InputConfig::new(&text)
        .with_focused(engaged)
        .with_cursor(cursor)
        .with_selection(sel_start, sel_end);
    let cfg = if engaged || !text.is_empty() { cfg } else { cfg.with_placeholder(placeholder) };
    let input_result = draw_input(ctx, &cfg, WidgetState::Normal, rect, &widget_theme);
    if engaged && coord.text_fields().cursor_visible(current_time_ms) {
        draw_input_cursor(ctx, input_result.cursor_x, input_result.cursor_y, input_result.cursor_height, &tt.item_text);
    }
    coord.register_on_layer(widget_id, rect, Sense::CLICK, layer_id);
    (rect, input_result.char_x_positions)
}

// =============================================================================
// List view (`DmView::List`) — "Publish my key" (when no identity key has
// been published yet) + conversation rows with unread badges.
// =============================================================================

fn draw_publish_key_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    tt: &ToolbarTheme,
) {
    let widget_id = "cloud_overlay:dm:publish_key";
    let rect = WidgetRect::new(x, y, w, ROW_H - ROW_GAP);
    let hovered = coord.is_hovered(&WidgetId::from(widget_id));
    ctx.set_fill_color(GOLD);
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    }
    mlc_core::engine::render::draw_svg_icon(
        ctx, Icon::Key.svg(), rect.x + 10.0, rect.y + (rect.height - 16.0) / 2.0, 16.0, 16.0, "#101014",
    );
    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color("#101014");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Publish my key", rect.x + 34.0, rect.center_y());
    coord.register_on_layer(widget_id, rect, Sense::CLICK | Sense::HOVER, layer_id);
}

fn draw_conversation_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    c: &DmConversationUi,
    current_time_ms: u64,
    tt: &ToolbarTheme,
) -> f64 {
    let widget_id = format!("cloud_overlay:dm:conversation:{}", c.id);
    let rect = WidgetRect::new(x, y, w, CONV_ROW_H - ROW_GAP);
    let hovered = coord.is_hovered(&WidgetId::from(widget_id.as_str()));
    if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    }

    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color(if hovered { GOLD } else { &tt.item_text });
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let label = crate::ui::widgets::fit_text_to_width(ctx, &c.peer_label, w - 60.0);
    ctx.fill_text(&label, x + 8.0, y + 4.0);

    if c.unread_count > 0 {
        let badge_label = if c.unread_count > 99 { "99+".to_string() } else { c.unread_count.to_string() };
        let badge_w = 16.0 + badge_label.len() as f64 * 6.0;
        let badge_rect = WidgetRect::new(x + w - badge_w - 8.0, y + 4.0, badge_w, 18.0);
        ctx.set_fill_color(GOLD);
        ctx.fill_rounded_rect(badge_rect.x, badge_rect.y, badge_rect.width, badge_rect.height, 9.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color("#101014");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&badge_label, badge_rect.center_x(), badge_rect.center_y());
    }

    ctx.set_font("11px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let last = c
        .last_message_at
        .as_deref()
        .map(|t| relative_time(t, current_time_ms))
        .unwrap_or_else(|| "no messages yet".to_string());
    ctx.fill_text(&last, x + 8.0, y + 24.0);

    coord.register_on_layer(widget_id.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
    CONV_ROW_H
}

fn list_h(d: &DmState) -> f64 {
    let publish_h = if d.identity_fingerprint.is_none() { ROW_H + 8.0 } else { 0.0 };
    let err_h = if d.last_error.is_some() { 20.0 } else { 0.0 };
    let list_h = if d.conversations.is_empty() { 24.0 } else { d.conversations.len() as f64 * CONV_ROW_H };
    publish_h + err_h + list_h
}

fn draw_list(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    state: &CloudOverlayState,
    current_time_ms: u64,
    tt: &ToolbarTheme,
) {
    let mut ly = y;
    if state.dm.identity_fingerprint.is_none() {
        draw_publish_key_row(ctx, coord, layer_id, x, ly, w, tt);
        ly += ROW_H + 8.0;
    }
    if let Some(err) = &state.dm.last_error {
        ly += draw_error_line(ctx, w, x, ly, err);
    }
    if state.dm.conversations.is_empty() {
        draw_status(ctx, tt, x, ly, w, "No conversations yet.", None);
    } else {
        for c in &state.dm.conversations {
            ly += draw_conversation_row(ctx, coord, layer_id, x, ly, w, c, current_time_ms, tt);
        }
    }
}

// =============================================================================
// Conversation view (`DmView::Conversation`) — header (peer label +
// fingerprint, read from the matching `DmState::conversations` row),
// message bubbles (mine right, theirs left), draft field, Send.
// =============================================================================

fn message_h(m: &DmMessageUi) -> f64 {
    let lines = wrap_text(&m.text, BUBBLE_WRAP_COLS).len().max(1);
    16.0 + lines as f64 * 16.0 + 16.0 + 10.0
}

fn draw_message(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    w: f64,
    m: &DmMessageUi,
    current_time_ms: u64,
    tt: &ToolbarTheme,
) -> f64 {
    let lines = wrap_text(&m.text, BUBBLE_WRAP_COLS);
    let line_count = lines.len().max(1);
    let bubble_w = (w * BUBBLE_WIDTH_FRAC).max(BUBBLE_MIN_W);
    let bubble_h = line_count as f64 * 16.0 + 16.0;
    let bx = if m.mine { x + w - bubble_w } else { x };
    let mut ly = y;

    ctx.set_font("10px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(if m.mine { TextAlign::Right } else { TextAlign::Left });
    ctx.set_text_baseline(TextBaseline::Top);
    let label_x = if m.mine { x + w } else { x };
    ctx.fill_text(&relative_time(&m.created_at, current_time_ms), label_x, ly);
    ly += 16.0;

    ctx.set_fill_color(if m.mine { GOLD } else { &tt.dropdown_bg });
    ctx.fill_rounded_rect(bx, ly, bubble_w, bubble_h, SETTINGS_BUTTON_RADIUS);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(if m.mine { "#101014" } else { &tt.item_text });
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    for (i, line) in lines.iter().enumerate() {
        ctx.fill_text(line, bx + 10.0, ly + 8.0 + i as f64 * 16.0);
    }
    ly += bubble_h + 10.0;

    ly - y
}

fn conversation_h(d: &DmState) -> f64 {
    let DmView::Conversation { messages, .. } = &d.view else { return 0.0 };
    let back_h = 36.0;
    let header_h = 36.0;
    let err_h = if d.last_error.is_some() { 20.0 } else { 0.0 };
    let msgs_h = if messages.is_empty() { 24.0 } else { messages.iter().map(message_h).sum() };
    let draft_h = 8.0 + FIELD_H + 8.0 + ROW_H;
    back_h + header_h + err_h + msgs_h + draft_h
}

#[allow(clippy::too_many_arguments)]
fn draw_conversation(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    state: &CloudOverlayState,
    id: i64,
    messages: &[DmMessageUi],
    draft_field_id: &str,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) {
    let mut ly = y;

    let back_rect = WidgetRect::new(x, ly, w, 28.0);
    let hovered = coord.is_hovered(&WidgetId::from("cloud_overlay:dm:back"));
    if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(back_rect.x, back_rect.y, back_rect.width, back_rect.height, SETTINGS_BUTTON_RADIUS);
    }
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(if hovered { &tt.item_text } else { &tt.item_text_muted });
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("← Messages", x + 8.0, ly + 14.0);
    coord.register_on_layer("cloud_overlay:dm:back", back_rect, Sense::CLICK | Sense::HOVER, layer_id);
    ly += 36.0;

    // Peer label/fingerprint are read from the matching conversation-list
    // row by id (`DmState::open_peer`'s own lookup, inlined here since we
    // already have `id` and `state.dm.conversations`), never duplicated
    // onto the view itself — see `DmView::Conversation`'s own doc.
    let peer = state.dm.conversations.iter().find(|c| c.id == id);
    let peer_label = peer.map(|c| c.peer_label.as_str()).unwrap_or("Unknown");
    let fingerprint = peer.and_then(|c| c.peer_fingerprint.as_deref()).unwrap_or("no key published");
    ctx.set_font("600 14px sans-serif");
    ctx.set_fill_color(GOLD);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(peer_label, x, ly);
    ly += 18.0;
    ctx.set_font("10px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.fill_text(&format!("fingerprint: {fingerprint}"), x, ly);
    ly += 18.0;

    if let Some(err) = &state.dm.last_error {
        ly += draw_error_line(ctx, w, x, ly, err);
    }

    if messages.is_empty() {
        draw_status(ctx, tt, x, ly, w, "No messages yet.", None);
        ly += 24.0;
    } else {
        for m in messages {
            ly += draw_message(ctx, x, ly, w, m, current_time_ms, tt);
        }
    }

    ly += 8.0;
    let field_rect = WidgetRect::new(x, ly, w, FIELD_H);
    let (rect, chars) = draw_field(
        ctx, coord, layer_id, "cloud_overlay:dm:draft_input", draft_field_id, field_rect,
        "Write a message…", ft, tt, current_time_ms,
    );
    result.dm_draft_rect = Some(rect);
    result.dm_draft_char_positions = chars;
    ly += FIELD_H + 8.0;

    draw_action_row(ctx, coord, layer_id, "cloud_overlay:dm:send", WidgetRect::new(x, ly, w, ROW_H - ROW_GAP), "Send", tt);
}
