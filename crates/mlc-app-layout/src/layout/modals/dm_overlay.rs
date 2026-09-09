//! Direct messages (paid, end-to-end encrypted) — what the Community modal's
//! Messages tab draws (`crate::layout::modals::community`, community-modal
//! arc W1; moved out of the News overlay's own `NewsTab::Messages`, retired).
//! Sibling of `cloud_overlay.rs` under `layout::modals`; reuses that file's
//! shared row/status/scroll helpers (`pub(crate)`). State lives in
//! `CommunityState::dm` (`DmState`); see that type's own doc for the
//! plaintext-only contract (the shell decrypts before this crate ever sees
//! a message). Pure presentation — effects live in the shell driver; click
//! dispatch lives in `mlc-app`'s `panel_click.rs` (out of scope here). Widget
//! ids keep their pre-existing `cloud_overlay:dm:*` prefix unchanged — only
//! the Rust type hosting them moved.
//!
//! W2 of the community-modal-and-forum-ladder arc (`docs/mlc/plans/
//! community-modal-and-forum-ladder-2026-09-09.md`) turned this into a
//! "familiar messenger" TWO-COLUMN layout: a fixed-width, own-scroll LEFT
//! column (`draw_conversation_list` — the "start a conversation with
//! nick/public id" row, always at its top, plus the conversation rows) and a
//! RIGHT pane holding the selected conversation's messages, the "desktop
//! only"/"Setting up messages…" notices, or a landing placeholder. The send
//! box is DOCKED at the bottom of the right pane, with only the messages
//! above it scrolling — same split `forum_v2_overlay.rs`'s own Thread view
//! uses for its reply box.

use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::i18n::{current_language, UserSettingsKey};
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::layout::render_chart::FrameTheme;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::ui::modal_settings::{CommunityState, DmConversationUi, DmMessageUi, DmState, DmView};
use crate::ui::scroll_widget::ScrollableContainer;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig, SETTINGS_BUTTON_RADIUS};
use crate::ui::widgets::types::{WidgetState, WidgetTheme};

use super::cloud_overlay::{
    draw_action_row, draw_column_divider, draw_status, register_scrollbar_hit_zones,
    relative_time, scroll_config, wrap_text, CloudOverlayResult, COL_GAP,
    DM_PEER_FIELD, FIELD_H, GOLD, LIST_COL_W, ROW_GAP, ROW_H,
};
use super::community::CommunityListPaneResult;

const CONV_ROW_H: f64 = 52.0;
/// Message bubble text wraps at this many characters — narrower than the
/// Forum body's own 62 (`forum_v2_overlay`'s `blocks`) since a bubble only
/// takes up [`BUBBLE_WIDTH_FRAC`] of the column, not the full width.
const BUBBLE_WRAP_COLS: usize = 40;
const BUBBLE_WIDTH_FRAC: f64 = 0.72;
const BUBBLE_MIN_W: f64 = 120.0;
/// The docked send box's own height — draft field + gap + Send button.
const COMPOSER_H: f64 = 8.0 + FIELD_H + 8.0 + ROW_H;

const SETTING_UP_STATUS: &str = "Setting up messages…";

/// Recipient-search dropdown under the "start a conversation" field (owner
/// ruling 2026-09-09, community-modal-and-forum-ladder arc, piece 2 item b)
/// — same row-height/cap idiom `forum_v2_overlay.rs`'s own inline
/// quick-search dropdown uses (`QUICK_RESULT_ROW_H`/`QUICK_DROPDOWN_MAX_ROWS`
/// there), sized for two lines (label + `@nick`/public id) per row instead
/// of one.
const LOOKUP_RESULT_ROW_H: f64 = 40.0;
const LOOKUP_NO_RESULT_ROW_H: f64 = 24.0;
const LOOKUP_DROPDOWN_MAX_ROWS: usize = 8;

// =============================================================================
// Entry points — `list_height` sizes the LEFT column's own scroll container;
// the RIGHT pane's content height is computed inline by `draw` (see
// `forum_v2_overlay.rs`'s own top doc for why one shared number stopped
// being enough once the composer needed docking).
// =============================================================================

pub(crate) fn list_height(state: &CommunityState) -> f64 {
    list_pane_h(&state.dm, state.forum2.nick_required())
}

/// The right pane's own "wanted" content height — used only for
/// `ModalShellSpec`'s inert `content_height` (see `forum_v2_overlay.rs`'s
/// own `height` doc for why that number is inert).
pub(crate) fn height(state: &CommunityState) -> f64 {
    if !state.dm.available {
        return if state.dm.last_error.is_some() { 20.0 } else { 0.0 };
    }
    if state.dm.identity_fingerprint.is_none() {
        return 24.0;
    }
    match &state.dm.view {
        DmView::List => 24.0,
        DmView::Conversation { messages, .. } => conversation_scroll_h(&state.dm, messages) + COMPOSER_H,
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
    state: &CommunityState,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
    list_result: &mut CommunityListPaneResult,
) {
    let right_x = x + LIST_COL_W + COL_GAP;
    let right_w = (w - LIST_COL_W - COL_GAP).max(0.0);
    draw_column_divider(ctx, right_x - COL_GAP / 2.0, y, viewport_h, tt);

    // --- Left column: "open a conversation" row + conversation list. Drawn
    // unconditionally (not gated on `available`/identity — see this file's
    // own top doc): a click on either still reaches `panel_click.rs`'s own
    // `dm:` arm, which already re-gates on `available` itself.
    let list_drawn_h = list_pane_h(&state.dm, state.forum2.nick_required());
    let list_viewport_rect = WidgetRect::new(x, y, LIST_COL_W, viewport_h);
    coord.register_on_layer("cloud_overlay:scroll_viewport", list_viewport_rect, Sense::SCROLL, layer_id);
    let list_scrollable = ScrollableContainer::new(list_viewport_rect, &state.list_scroll, Some(scroll_config()));
    list_scrollable.begin(ctx);
    let lx = x;
    let mut ly = list_scrollable.content_y();
    let lw = list_scrollable.content_width();
    ly += draw_open_peer_row(
        ctx, coord, layer_id, lx, ly, lw, ft, tt, current_time_ms,
        state.forum2.nick_required(), result,
    );
    if state.dm.conversations.is_empty() {
        draw_status(ctx, tt, lx, ly, lw, "No conversations yet.", None);
    } else {
        for c in &state.dm.conversations {
            ly += draw_conversation_row(ctx, coord, layer_id, lx, ly, lw, c, current_time_ms, tt);
        }
    }
    draw_lookup_dropdown(ctx, coord, layer_id, result.dm_peer_rect, &state.dm, tt);
    let widget_theme = WidgetTheme::default();
    let list_scroll_result = list_scrollable.end(ctx, list_drawn_h, &widget_theme);
    list_result.viewport_rect = Some(list_viewport_rect);
    list_result.total_content_height = Some(list_drawn_h);
    list_result.viewport_height = Some(viewport_h.min(list_drawn_h));
    list_result.scrollbar_handle_rect = list_scroll_result.handle_rect;
    list_result.scrollbar_track_rect = list_scroll_result.track_rect;
    register_scrollbar_hit_zones(coord, layer_id, &list_scroll_result);

    // --- Right pane ---
    if !state.dm.available {
        if let Some(err) = &state.dm.last_error {
            draw_error_line(ctx, right_w, right_x, y, err);
        }
        return;
    }
    if state.dm.identity_fingerprint.is_none() {
        draw_status(ctx, tt, right_x, y, right_w, SETTING_UP_STATUS, None);
        return;
    }
    match &state.dm.view {
        DmView::List => {
            draw_status(
                ctx, tt, right_x, y, right_w,
                "Select a conversation from the list on the left, or start a new one above.", None,
            );
        }
        DmView::Conversation { id, messages, draft_field_id } => draw_right_conversation(
            ctx, coord, layer_id, right_x, y, right_w, viewport_h, state, *id, messages,
            draft_field_id, ft, tt, current_time_ms, result,
        ),
    }
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
// Left column — "start a conversation" row + conversation rows. Drawn
// unconditionally (see this file's own top doc); `list_pane_h` sizes it the
// same way regardless of `DmState::available`/`identity_fingerprint`.
// =============================================================================

/// "Start a conversation" row: a fuzzy nick/public-id search field —
/// replaces the old plain field + "Open" button (owner ruling 2026-09-09:
/// search-as-you-type with a results dropdown, [`draw_lookup_dropdown`]).
/// Always at the top of the left column — publication of the identity key
/// to the server runs in the background (see `DmRequest`'s own doc) and is
/// never a precondition the user has to clear first; click dispatch lives
/// in `mlc-app`'s `panel_click.rs` (`cloud_overlay:dm:peer_input`).
#[allow(clippy::too_many_arguments)]
fn draw_open_peer_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    nick_required: bool,
    result: &mut CloudOverlayResult,
) -> f64 {
    // Disabled (owner mandate 2026-09-09): a signed-in caller with no
    // nickname yet cannot open a conversation — a hint line + the same
    // "Choose a nick" trigger the composer ([`draw_right_conversation`])
    // uses, instead of the search field.
    if nick_required {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(UserSettingsKey::AccountChooseNickHint.get(current_language()), x, y);
        let hint_h = 20.0;
        let btn_h = ROW_H - ROW_GAP;
        draw_action_row(
            ctx, coord, layer_id, "cloud_overlay:dm:choose_nick",
            WidgetRect::new(x, y + hint_h, w, btn_h),
            UserSettingsKey::AccountChooseNickBtn.get(current_language()), tt,
        );
        return open_peer_row_h(true);
    }
    let (rect, chars) = draw_field(
        ctx, coord, layer_id, "cloud_overlay:dm:peer_input", DM_PEER_FIELD,
        WidgetRect::new(x, y, w, FIELD_H), "Search by nick or public id…", ft, tt, current_time_ms,
    );
    result.dm_peer_rect = Some(rect);
    result.dm_peer_char_positions = chars;
    open_peer_row_h(false)
}

/// Drawn LAST in the left column's own scroll body (after the conversation
/// rows), so it paints over them — same "drawn last, floating over the
/// content beneath it" idiom `forum_v2_overlay.rs`'s own
/// `draw_quick_search_dropdown` uses for its inline search box. Anchored
/// under `field_rect` (the "start a conversation" field's own rect,
/// `None`/no-op while the field never drew). Only shows once a lookup has
/// actually been asked (`DmState::lookup_query.is_some()`) — an empty field
/// draws nothing, same "field cleared ⇒ dropdown gone" rule the forum box
/// uses.
fn draw_lookup_dropdown(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    field_rect: Option<WidgetRect>,
    d: &DmState,
    tt: &ToolbarTheme,
) {
    let Some(anchor) = field_rect else { return };
    if d.lookup_query.is_none() {
        return;
    }
    let n = d.lookup_results.len().min(LOOKUP_DROPDOWN_MAX_ROWS);
    let box_h = if n == 0 { LOOKUP_NO_RESULT_ROW_H + 8.0 } else { n as f64 * LOOKUP_RESULT_ROW_H + 8.0 };
    let box_x = anchor.x;
    let box_y = anchor.y + anchor.height + 4.0;

    ctx.set_fill_color(&tt.dropdown_bg);
    ctx.fill_rounded_rect(box_x, box_y, anchor.width, box_h, SETTINGS_BUTTON_RADIUS);

    if n == 0 {
        draw_status(ctx, tt, box_x, box_y + 4.0, anchor.width, "No one found.", None);
        return;
    }
    let mut ry = box_y + 4.0;
    for (i, hit) in d.lookup_results.iter().take(n).enumerate() {
        let widget_id = format!("cloud_overlay:dm:lookup_result:{i}");
        let rect = WidgetRect::new(box_x + 4.0, ry, anchor.width - 8.0, LOOKUP_RESULT_ROW_H - 4.0);
        let hovered = coord.is_hovered(&WidgetId::from(widget_id.as_str()));
        if hovered {
            ctx.set_fill_color(&tt.item_bg_hover);
            ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
        }

        ctx.set_font("600 12px sans-serif");
        ctx.set_fill_color(if hovered { GOLD } else { &tt.item_text });
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        let label = crate::ui::widgets::fit_text_to_width(ctx, &hit.label, rect.width - 12.0);
        ctx.fill_text(&label, rect.x + 6.0, rect.y + 2.0);

        ctx.set_font("11px monospace");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        let sub = match &hit.nick {
            Some(nick) => format!("@{nick}  ·  {}", short_public_id(&hit.public_id)),
            None => short_public_id(&hit.public_id),
        };
        let sub = crate::ui::widgets::fit_text_to_width(ctx, &sub, rect.width - 12.0);
        ctx.fill_text(&sub, rect.x + 6.0, rect.y + 18.0);

        coord.register_on_layer(widget_id.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
        ry += LOOKUP_RESULT_ROW_H;
    }
}

/// Shorten a public id for the dropdown's second line — a click carries the
/// full value from `DmState::lookup_results` directly (never re-typed), so
/// only enough to tell two rows apart at a glance is needed here.
/// Char-based (not byte slicing) so a non-ASCII id can never panic on a
/// mid-character boundary.
fn short_public_id(id: &str) -> String {
    if id.chars().count() <= 12 {
        id.to_string()
    } else {
        let head: String = id.chars().take(12).collect();
        format!("{head}…")
    }
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

/// [`draw_open_peer_row`]'s own height, factored out so [`list_pane_h`]'s
/// sizing can never drift from what that function actually draws — the
/// disabled (`nick_required`) shape is taller (hint line + button) than
/// the plain search field.
fn open_peer_row_h(nick_required: bool) -> f64 {
    if nick_required {
        let hint_h = 20.0;
        let btn_h = ROW_H - ROW_GAP;
        hint_h + btn_h + 12.0
    } else {
        FIELD_H + 12.0
    }
}

fn list_pane_h(d: &DmState, nick_required: bool) -> f64 {
    let open_peer_h = open_peer_row_h(nick_required);
    let list_h = if d.conversations.is_empty() { 24.0 } else { d.conversations.len() as f64 * CONV_ROW_H };
    open_peer_h + list_h
}

// =============================================================================
// Right pane — Conversation view (`DmView::Conversation`): header (peer
// label + fingerprint, read from the matching `DmState::conversations` row),
// message bubbles (mine right, theirs left) scrolling ABOVE a DOCKED
// send box (draft field + Send) fixed to the bottom of the pane — W2 split,
// same shape `forum_v2_overlay.rs`'s own Thread reply box uses. No "←
// Messages" row (W2): the left column's permanent conversation list already
// reaches this view — same reasoning `forum_v2_overlay.rs`'s own doc gives
// for dropping its `BoardThreads`/`Thread` back rows.
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

fn conversation_scroll_h(d: &DmState, messages: &[DmMessageUi]) -> f64 {
    let header_h = 36.0;
    let err_h = if d.last_error.is_some() { 20.0 } else { 0.0 };
    let msgs_h = if messages.is_empty() { 24.0 } else { messages.iter().map(message_h).sum() };
    header_h + err_h + msgs_h
}

#[allow(clippy::too_many_arguments)]
fn draw_right_conversation(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    viewport_h: f64,
    state: &CommunityState,
    id: i64,
    messages: &[DmMessageUi],
    draft_field_id: &str,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) {
    let scroll_h = (viewport_h - COMPOSER_H).max(0.0);
    let content_h = conversation_scroll_h(&state.dm, messages);

    let viewport_rect = WidgetRect::new(x, y, w, scroll_h);
    coord.register_on_layer("cloud_overlay:scroll_viewport", viewport_rect, Sense::SCROLL, layer_id);
    let scrollable = ScrollableContainer::new(viewport_rect, &state.scroll, Some(scroll_config()));
    scrollable.begin(ctx);
    let sx = x;
    let mut sy = scrollable.content_y();
    let sw = scrollable.content_width();

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
    ctx.fill_text(peer_label, sx, sy);
    sy += 18.0;
    ctx.set_font("10px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.fill_text(&format!("fingerprint: {fingerprint}"), sx, sy);
    sy += 18.0;

    if let Some(err) = &state.dm.last_error {
        sy += draw_error_line(ctx, sw, sx, sy, err);
    }

    if messages.is_empty() {
        draw_status(ctx, tt, sx, sy, sw, "No messages yet.", None);
    } else {
        for m in messages {
            sy += draw_message(ctx, sx, sy, sw, m, current_time_ms, tt);
        }
    }

    let widget_theme = WidgetTheme::default();
    let scroll_result = scrollable.end(ctx, content_h, &widget_theme);
    result.total_content_height = Some(content_h);
    result.viewport_height = Some(scroll_h);
    result.scroll_viewport_rect = Some(viewport_rect);
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
    register_scrollbar_hit_zones(coord, layer_id, &scroll_result);

    // Docked send box — fixed to the bottom of the pane, outside the
    // scrollable clip above. Disabled (owner mandate 2026-09-09,
    // `nick_required`) trades the draft field for a hint line and swaps
    // "Send" for the same "Choose a nick" trigger the recipient-search row
    // ([`draw_open_peer_row`]) uses — same `COMPOSER_H` footprint either
    // way, so the scroll math above never has to know which is showing.
    let composer_y = y + (viewport_h - COMPOSER_H).max(0.0);
    if state.forum2.nick_required() {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(
            UserSettingsKey::AccountChooseNickHint.get(current_language()),
            x, composer_y + 8.0,
        );
        let btn_y = composer_y + 8.0 + FIELD_H + 8.0;
        draw_action_row(
            ctx, coord, layer_id, "cloud_overlay:dm:choose_nick",
            WidgetRect::new(x, btn_y, w, ROW_H - ROW_GAP),
            UserSettingsKey::AccountChooseNickBtn.get(current_language()), tt,
        );
        return;
    }
    let field_rect = WidgetRect::new(x, composer_y + 8.0, w, FIELD_H);
    let (rect, chars) = draw_field(
        ctx, coord, layer_id, "cloud_overlay:dm:draft_input", draft_field_id, field_rect,
        "Write a message…", ft, tt, current_time_ms,
    );
    result.dm_draft_rect = Some(rect);
    result.dm_draft_char_positions = chars;
    let btn_y = composer_y + 8.0 + FIELD_H + 8.0;
    draw_action_row(ctx, coord, layer_id, "cloud_overlay:dm:send", WidgetRect::new(x, btn_y, w, ROW_H - ROW_GAP), "Send", tt);
}
