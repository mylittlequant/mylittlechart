//! Direct messages (paid, end-to-end encrypted) — what the Community modal's
//! Messages tab draws (`crate::layout::modals::community`, community-modal
//! arc W1; moved out of the News overlay's own `NewsTab::Messages`, retired).
//! Sibling of `cloud_overlay.rs` under `layout::modals`; reuses that file's
//! shared row/status/scroll helpers (`pub(crate)`). State lives in
//! `CommunityState::dm` (`DmState`); see that type's own doc for the
//! decryption-stage contract (`DmMessageBody`) — the shell decrypts what it
//! can before this crate ever sees a message, but a sealed vault (or an
//! unresolved peer key) means it hands over raw ciphertext instead of
//! refusing to hand over anything at all. Pure presentation — effects live
//! in the shell driver; click dispatch lives in `mlc-app`'s
//! `panel_click.rs` (out of scope here). Widget ids keep their pre-existing
//! `cloud_overlay:dm:*` prefix unchanged — only the Rust type hosting them
//! moved.
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
//!
//! Owner ruling 2026-09-10 (Telegram-style bubbles + pre-unlock ciphertext):
//! the right pane renders even while the vault is sealed — a header notice
//! ([`draw_vault_locked_header`]) plus every message as a dim, clipped,
//! lock-glyphed block of raw base64 ([`message_visual`]'s `Ciphertext`
//! branch) — so a caller sees end-to-end encryption is real before they
//! ever unlock anything. Bubbles carry an avatar (peer side only), a nick
//! line, wrapped body text, a bottom-right clock, and day separators
//! between bubbles on different calendar days. Every color in this file
//! comes from `ToolbarTheme`/theme-derived helpers (`mlc_core::mix_colors`,
//! `contrasting_text_color`) — accent is used ONLY to tint the "mine"
//! bubble so sender/recipient read apart at a glance, never as a plain
//! decorative fill.
//!
//! Owner ruling 2026-09-10 (text selection, item 2): message bubble text is
//! drag-selectable and Ctrl+C-able across wrapped lines AND across
//! messages, via the SAME host-side line-rect mechanism the Agents chat
//! panel uses for its own selection (`mlc_app_sidebar_content::render::
//! render_agents_chat_bubbles`'s `line_rects_out`/`ChatSelection`) — see
//! [`draw_message`]'s own doc. This replaced a one-`TextFieldStore`-field-
//! per-wrapped-line approach that could only ever select inside a single
//! line at a time.

use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::i18n::{current_language, UserSettingsKey};
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::layout::render_chart::FrameTheme;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::ui::modal_settings::{
    CommunityState, DmConversationUi, DmMessageBody, DmMessageSelection, DmMessageUi, DmState,
    DmUnavailableReason, DmView,
};
use crate::ui::scroll_widget::ScrollableContainer;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::avatar::contrasting_text_color;
use crate::ui::widgets::{
    draw_avatar_placeholder, draw_input, draw_input_cursor, InputConfig, SETTINGS_BUTTON_RADIUS,
};
use crate::ui::widgets::types::{WidgetState, WidgetTheme};

use super::cloud_overlay::{
    draw_action_row, draw_column_divider, draw_expand_row, draw_status, parse_rfc3339_unix_secs,
    register_scrollbar_hit_zones, relative_time, scroll_config, wrap_text, CloudOverlayResult, COL_GAP,
    DM_PEER_FIELD, FIELD_H, LIST_COL_W, ROW_GAP, ROW_H,
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
// Telegram-style bubble geometry (owner ruling 2026-09-10) — see this file's
// own top doc.
// =============================================================================

/// Bubble avatar diameter — peer messages only. Own messages draw no
/// avatar at all ("pick left-only like Telegram", the owner's own wording).
const BUBBLE_AVATAR_D: f64 = 28.0;
const BUBBLE_AVATAR_GAP: f64 = 8.0;
/// Conversation-list row avatar diameter.
const LIST_AVATAR_D: f64 = 32.0;
const LIST_AVATAR_GAP: f64 = 8.0;
/// Height of one wrapped line inside a bubble's body.
const BUBBLE_LINE_H: f64 = 16.0;
/// Height of the small nick line at the top of a bubble.
const BUBBLE_NICK_H: f64 = 14.0;
/// Height of the small clock line at the bottom of a bubble.
const BUBBLE_TIME_H: f64 = 14.0;
const BUBBLE_PAD_V: f64 = 8.0;
/// Gap between one message block and the next.
const BUBBLE_GAP: f64 = 10.0;
/// Day-separator row height ("Today" / "Yesterday" / date, centered,
/// between bubbles on different calendar days).
const DAY_SEP_H: f64 = 24.0;
/// The peer-key-changed notice's own height (owner review 2026-09-23,
/// `forum-messenger-realtime.md` item 2) — one clipped line under the
/// fingerprint line.
const KEY_CHANGED_NOTICE_H: f64 = 18.0;
/// A locked (pre-unlock) ciphertext block never shows more than this many
/// wrapped lines of base64 — owner ruling 2026-09-10: "clipped to ~3 lines
/// with '…'".
const CIPHERTEXT_MAX_LINES: usize = 3;

// =============================================================================
// Entry points — `list_height` sizes the LEFT column's own scroll container;
// the RIGHT pane's content height is computed inline by `draw` (see
// `forum_v2_overlay.rs`'s own top doc for why one shared number stopped
// being enough once the composer needed docking).
// =============================================================================

pub(crate) fn list_height(state: &CommunityState) -> f64 {
    list_pane_h(&state.dm, state.forum2.nick_required())
}

/// [`draw_vault_locked_header`]'s own height, factored out so [`height`]'s
/// sizing can never drift from what that function actually draws — same
/// "one shared number" rule [`open_peer_row_h`]/[`list_pane_h`] already
/// follow for the left column.
fn vault_locked_header_h() -> f64 {
    18.0 + 8.0 + (ROW_H - ROW_GAP) + 10.0
}

/// Whether `id`'s peer-key backfill has landed a different fingerprint than
/// this session held before it — see [`DmState::has_peer_key_changed`]'s own
/// doc. Threaded into every sizing/drawing call below so the header notice
/// (owner review 2026-09-23, `forum-messenger-realtime.md` item 2) and the
/// Undecryptable explanation it unlocks never disagree about which
/// conversation's messages get the explanation.
fn conversation_key_changed(dm: &DmState, id: i64) -> bool {
    dm.has_peer_key_changed(id)
}

/// The right pane's own "wanted" content height — used only for
/// `ModalShellSpec`'s inert `content_height` (see `forum_v2_overlay.rs`'s
/// own `height` doc for why that number is inert).
pub(crate) fn height(state: &CommunityState) -> f64 {
    match state.dm.unavailable_reason {
        DmUnavailableReason::NotPaid => {
            return if state.dm.last_error.is_some() { 20.0 } else { 0.0 };
        }
        DmUnavailableReason::VaultLocked => {
            let body_h = match &state.dm.view {
                DmView::List => 24.0,
                DmView::Conversation { id, messages, has_older, .. } => {
                    let key_changed = conversation_key_changed(&state.dm, *id);
                    conversation_scroll_h(state.dm.available, state.dm.last_error.is_some(), key_changed, *has_older, messages)
                        + COMPOSER_H
                }
            };
            return vault_locked_header_h() + body_h;
        }
        DmUnavailableReason::None => {}
    }
    if !state.dm.available {
        return if state.dm.last_error.is_some() { 20.0 } else { 0.0 };
    }
    if state.dm.identity_fingerprint.is_none() {
        return 24.0;
    }
    match &state.dm.view {
        DmView::List => 24.0,
        DmView::Conversation { id, messages, has_older, .. } => {
            let key_changed = conversation_key_changed(&state.dm, *id);
            conversation_scroll_h(state.dm.available, state.dm.last_error.is_some(), key_changed, *has_older, messages)
                + COMPOSER_H
        }
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
    // `dm:` arm, which already re-gates on `available` itself (navigation —
    // opening a row, going back — is exempt; see that arm's own doc).
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
    match state.dm.unavailable_reason {
        DmUnavailableReason::NotPaid => {
            if let Some(err) = &state.dm.last_error {
                draw_error_line(ctx, tt, right_w, right_x, y, err);
            }
            return;
        }
        // Paid, vault sealed (owner ruling 2026-09-10): the header notice +
        // button draw ABOVE whatever `state.dm.view` already shows — the
        // list placeholder, or the open conversation's messages as
        // ciphertext — never REPLACING it, so a conversation opened while
        // locked stays open once the vault unlocks (no navigation reset).
        DmUnavailableReason::VaultLocked => {
            let header_h = draw_vault_locked_header(ctx, coord, layer_id, right_x, y, right_w, tt);
            let body_y = y + header_h;
            let body_viewport_h = (viewport_h - header_h).max(0.0);
            match &state.dm.view {
                DmView::List => {
                    draw_status(
                        ctx, tt, right_x, body_y, right_w,
                        "Select a conversation from the list on the left, or start a new one above.", None,
                    );
                }
                DmView::Conversation { id, messages, draft_field_id, has_older } => draw_right_conversation(
                    ctx, coord, layer_id, right_x, body_y, right_w, body_viewport_h, state, *id, messages,
                    draft_field_id, *has_older, ft, tt, current_time_ms, result,
                ),
            }
            return;
        }
        DmUnavailableReason::None => {}
    }
    if !state.dm.available {
        // Defensive fallback — `available` and `unavailable_reason` are
        // always stamped together by both shells, so this arm should be
        // unreachable; if it ever is, degrade to the old single-line notice
        // rather than draw controls the caller cannot use.
        if let Some(err) = &state.dm.last_error {
            draw_error_line(ctx, tt, right_w, right_x, y, err);
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
        DmView::Conversation { id, messages, draft_field_id, has_older } => draw_right_conversation(
            ctx, coord, layer_id, right_x, y, right_w, viewport_h, state, *id, messages,
            draft_field_id, *has_older, ft, tt, current_time_ms, result,
        ),
    }
}

/// One clipped error line, in the theme's own `danger` tone — mirrors
/// `forum_v2_overlay`'s own `draw_error_line`; not shared because pulling
/// one function up to `cloud_overlay.rs` for two four-line call sites is
/// not worth the extra public surface.
fn draw_error_line(ctx: &mut dyn RenderContext, tt: &ToolbarTheme, w: f64, x: f64, y: f64, msg: &str) -> f64 {
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&tt.danger);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let m = crate::ui::widgets::fit_text_to_width(ctx, msg, w);
    ctx.fill_text(&m, x, y);
    20.0
}

/// A small padlock: an arched shackle (stroke) over a rounded-rect body
/// (fill) — plain vector shapes, not the `IconId` SVG registry (this
/// crate sits below that registry's own home, and a locked-ciphertext
/// glyph is one tiny, constant shape, not worth wiring a lookup for).
fn draw_lock_glyph(ctx: &mut dyn RenderContext, cx: f64, cy: f64, size: f64, color: &str) {
    let body_w = size;
    let body_h = size * 0.72;
    let body_x = cx - body_w / 2.0;
    let body_y = cy - body_h / 2.0 + size * 0.16;
    let shackle_r = size * 0.30;
    ctx.set_stroke_color(color);
    ctx.set_stroke_width((size * 0.16).max(1.0));
    ctx.begin_path();
    ctx.arc(cx, body_y, shackle_r, std::f64::consts::PI, std::f64::consts::TAU);
    ctx.stroke();
    ctx.set_fill_color(color);
    ctx.fill_rounded_rect(body_x, body_y, body_w, body_h, size * 0.16);
}

/// The right pane's header while the vault is sealed but the account is
/// paid (owner ruling 2026-09-10): one line saying so, and a button that
/// opens the Vault tab — reuses the Account tab's own `account_open_vault`
/// handler (`"cloud_overlay:dm:unlock_vault"`, re-gated in `panel_click.rs`
/// BEFORE the `dm.available` check so it keeps working precisely while this
/// draws).
fn draw_vault_locked_header(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    tt: &ToolbarTheme,
) -> f64 {
    draw_lock_glyph(ctx, x + 8.0, y + 9.0, 14.0, &tt.item_text_muted);
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let hint = crate::ui::widgets::fit_text_to_width(
        ctx, UserSettingsKey::DmUnlockVaultHint.get(current_language()), w - 24.0,
    );
    ctx.fill_text(&hint, x + 22.0, y + 2.0);
    let btn_y = y + 18.0 + 8.0;
    draw_action_row(
        ctx, coord, layer_id, "cloud_overlay:dm:unlock_vault",
        WidgetRect::new(x, btn_y, w, ROW_H - ROW_GAP),
        UserSettingsKey::AccountOpenVault.get(current_language()), tt,
    );
    vault_locked_header_h()
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
        ctx.set_fill_color(if hovered { &tt.item_text_hover } else { &tt.item_text });
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

    let avatar_cx = x + 8.0 + LIST_AVATAR_D / 2.0;
    let avatar_cy = rect.y + rect.height / 2.0;
    // Peer identity marker — a neutral "active surface" tone, never the
    // accent: accent is reserved for telling sender/recipient bubbles apart
    // inside an open conversation (owner ruling 2026-09-10), and every row
    // here is a peer, never "me".
    draw_avatar_placeholder(ctx, avatar_cx, avatar_cy, LIST_AVATAR_D / 2.0, &c.peer_label, &tt.item_bg_active);
    let text_x = x + 8.0 + LIST_AVATAR_D + LIST_AVATAR_GAP;

    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color(if hovered { &tt.item_text_hover } else { &tt.item_text });
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let label = crate::ui::widgets::fit_text_to_width(ctx, &c.peer_label, (x + w - text_x - 52.0).max(0.0));
    ctx.fill_text(&label, text_x, y + 4.0);

    if c.unread_count > 0 {
        let badge_label = if c.unread_count > 99 { "99+".to_string() } else { c.unread_count.to_string() };
        let badge_w = 16.0 + badge_label.len() as f64 * 6.0;
        let badge_rect = WidgetRect::new(x + w - badge_w - 8.0, y + 4.0, badge_w, 18.0);
        // Unread-count pill — the same accent "active indicator" the
        // envelope/events toolbar buttons already paint their own unread
        // dot with (`ToolbarTheme::accent`'s own doc), not a sender/
        // recipient distinction, so it stays outside that rule.
        ctx.set_fill_color(&tt.accent);
        ctx.fill_rounded_rect(badge_rect.x, badge_rect.y, badge_rect.width, badge_rect.height, 9.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(contrasting_text_color(&tt.accent));
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
    ctx.fill_text(&last, text_x, y + 24.0);

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
//
// Owner ruling 2026-09-10: this view now also renders with the vault
// SEALED (`DmUnavailableReason::VaultLocked`, hosted under the header
// notice `draw_vault_locked_header` draws) — every message renders
// whatever `DmMessageBody` variant it actually holds ([`message_visual`]),
// and the composer swaps its live draft field for a muted hint
// (`DmState::available` gates that, read directly off `state.dm` — no
// separate "locked" parameter needed since this function already takes
// `state: &CommunityState`).
// =============================================================================

/// What one bubble actually shows, and how — computed once per message so
/// [`message_h`] (frame sizing) and [`draw_message`] (paint) can never
/// disagree about the line count.
struct MessageVisual {
    lines: Vec<String>,
    /// Dim monospace treatment + lock glyph (the raw-ciphertext-before-
    /// unlock look) vs the ordinary sans-serif bubble text.
    is_ciphertext_style: bool,
}

/// `key_changed` — whether THIS message's conversation has a detected
/// peer-key change ([`DmState::has_peer_key_changed`]) — only changes the
/// `Undecryptable` branch's own wording (owner review 2026-09-23,
/// `forum-messenger-realtime.md` item 2): a bubble sealed under a key the
/// peer has since rotated away from reads differently from a generic
/// decrypt failure.
fn message_visual(m: &DmMessageUi, available: bool, key_changed: bool) -> MessageVisual {
    match &m.body {
        DmMessageBody::Plain(text) => {
            MessageVisual { lines: wrap_text(text, BUBBLE_WRAP_COLS), is_ciphertext_style: false }
        }
        DmMessageBody::Undecryptable => {
            let text = if key_changed {
                UserSettingsKey::DmUndecryptableKeyChanged.get(current_language()).to_string()
            } else {
                "<cannot decrypt>".to_string()
            };
            MessageVisual { lines: vec![text], is_ciphertext_style: false }
        }
        DmMessageBody::Ciphertext { ciphertext_b64, .. } => {
            if available {
                // Not vault-locked — a peer-key backfill just has not
                // landed for this conversation yet (rare, transient): the
                // shell's own per-frame redecrypt-in-place retry clears
                // this the moment it does, no user action needed, so this
                // reads as ordinary bubble text rather than the locked
                // ciphertext block.
                MessageVisual { lines: vec!["Decrypting…".to_string()], is_ciphertext_style: false }
            } else {
                let mut lines = wrap_text(ciphertext_b64, BUBBLE_WRAP_COLS);
                if lines.len() > CIPHERTEXT_MAX_LINES {
                    lines.truncate(CIPHERTEXT_MAX_LINES);
                    if let Some(last) = lines.last_mut() {
                        last.push('…');
                    }
                }
                MessageVisual { lines, is_ciphertext_style: true }
            }
        }
    }
}

fn message_h(m: &DmMessageUi, available: bool, key_changed: bool) -> f64 {
    let visual = message_visual(m, available, key_changed);
    let line_count = visual.lines.len().max(1);
    BUBBLE_PAD_V * 2.0 + BUBBLE_NICK_H + line_count as f64 * BUBBLE_LINE_H + BUBBLE_TIME_H + BUBBLE_GAP
}

/// Two messages fall on the same calendar day (UTC — see [`clock_hm`]'s own
/// doc for why this crate has no local-timezone conversion yet).
fn same_day(a: &str, b: &str) -> bool {
    match (parse_rfc3339_unix_secs(a), parse_rfc3339_unix_secs(b)) {
        (Some(sa), Some(sb)) => sa.div_euclid(86400) == sb.div_euclid(86400),
        _ => false,
    }
}

/// Whether a day separator belongs immediately before `messages[i]` — the
/// very first message always gets one (Telegram shows a date header even
/// for a conversation's opening line), every later one only when its day
/// differs from the message right before it.
fn day_separator_before(messages: &[DmMessageUi], i: usize) -> bool {
    i == 0 || !same_day(&messages[i - 1].created_at, &messages[i].created_at)
}

/// "Today" / "Yesterday" / the plain date, compared against `now_ms`'s own
/// calendar day (UTC).
fn day_separator_label(created_at: &str, now_ms: u64) -> String {
    let fallback = || created_at.split('T').next().unwrap_or(created_at).to_string();
    let Some(secs) = parse_rfc3339_unix_secs(created_at) else { return fallback() };
    let day = secs.div_euclid(86400);
    let today = ((now_ms / 1000) as i64).div_euclid(86400);
    match today - day {
        0 => UserSettingsKey::DmDayToday.get(current_language()).to_string(),
        1 => UserSettingsKey::DmDayYesterday.get(current_language()).to_string(),
        _ => fallback(),
    }
}

fn draw_day_separator(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, label: &str, tt: &ToolbarTheme) -> f64 {
    ctx.set_font("600 10px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, x + w / 2.0, y + DAY_SEP_H / 2.0);
    DAY_SEP_H
}

/// `created_at`'s own `HH:MM` substring, verbatim off the wire ISO-8601
/// string (server clock, i.e. UTC) — this crate has no bridge to the
/// viewer's actual local timezone yet (wasm needs the browser's `Date`,
/// native would need a `chrono`/`time`-zone-database dependency; neither
/// exists here today), so "local" is a named gap rather than a silently
/// wrong conversion. `"--:--"` for anything shorter than an ISO timestamp.
fn clock_hm(created_at: &str) -> &str {
    created_at.get(11..16).unwrap_or("--:--")
}

/// One message bubble: an avatar (peer side only), a nick line ("You" for
/// `m.mine`, the peer's own label otherwise), the wrapped body, and a
/// bottom-right clock — plain bubbles for [`DmMessageBody::Plain`], a dim
/// monospace lock-glyphed block for [`DmMessageBody::Ciphertext`] while the
/// vault is sealed (`!available`).
///
/// Records one [`crate::ui::selectable_text::LineHit`] per WRAPPED LINE
/// into `result.dm_message_line_rects` — HOST-SIDE text selection (the same
/// mechanism the Agents chat panel uses for its own, owner ruling
/// 2026-09-10), not a per-line `TextFieldStore` field: that store tracks a
/// single `(rect, char_positions)` pair per id with no row axis at all, so
/// it could only ever select inside ONE wrapped line at a time. `selection`
/// is the whole open conversation's current selection (`DmState::
/// selection`), read here so each message can paint the highlight over
/// whichever of ITS OWN lines the ordered range covers.
#[allow(clippy::too_many_arguments)]
fn draw_message(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    w: f64,
    msg_idx: u16,
    m: &DmMessageUi,
    peer_label: &str,
    available: bool,
    key_changed: bool,
    selection: Option<DmMessageSelection>,
    tt: &ToolbarTheme,
    result: &mut CloudOverlayResult,
) -> f64 {
    let visual = message_visual(m, available, key_changed);
    let line_count = visual.lines.len().max(1);
    let bubble_inner_h = BUBBLE_NICK_H + line_count as f64 * BUBBLE_LINE_H + BUBBLE_TIME_H;
    let bubble_h = bubble_inner_h + BUBBLE_PAD_V * 2.0;

    let has_avatar = !m.mine;
    let avatar_reserved = if has_avatar { BUBBLE_AVATAR_D + BUBBLE_AVATAR_GAP } else { 0.0 };
    let avail_w = (w - avatar_reserved).max(0.0);
    let bubble_w = (avail_w * BUBBLE_WIDTH_FRAC).max(BUBBLE_MIN_W);
    let bx = if m.mine { x + w - bubble_w } else { x + avatar_reserved };

    if has_avatar {
        // Peer identity marker — a neutral "active surface" tone, never the
        // accent (see `draw_conversation_row`'s own comment: accent is
        // reserved for telling sender/recipient bubbles apart, and this is
        // always a peer, never "me").
        let avatar_cx = x + BUBBLE_AVATAR_D / 2.0;
        let avatar_cy = y + bubble_h / 2.0;
        draw_avatar_placeholder(ctx, avatar_cx, avatar_cy, BUBBLE_AVATAR_D / 2.0, peer_label, &tt.item_bg_active);
    }

    // Own bubble = the pane's own secondary surface tinted toward the
    // theme's accent; peer bubble = the plain secondary surface — accent
    // is used ONLY to tell sender and recipient apart (owner ruling
    // 2026-09-10), never as a decorative fill on its own. The text color
    // is derived from whatever that tinted surface actually renders as
    // (auto black/white contrast) rather than a fixed light/dark guess, so
    // every preset — including ones with a light or a very saturated
    // accent — stays legible.
    let mine_bg = mlc_core::mix_colors(&tt.dropdown_bg, &tt.accent, 0.25);
    let mine_text = contrasting_text_color(&mine_bg);
    ctx.set_fill_color(if m.mine { &mine_bg } else { &tt.dropdown_bg });
    ctx.fill_rounded_rect(bx, y, bubble_w, bubble_h, SETTINGS_BUTTON_RADIUS);

    if visual.is_ciphertext_style {
        draw_lock_glyph(ctx, bx + bubble_w - 14.0, y + BUBBLE_PAD_V + 6.0, 10.0, &tt.item_text_muted);
    }

    // Nick line — "You" for an own message, the peer's label otherwise.
    let nick_color: &str = if m.mine { mine_text } else { &tt.item_text_muted };
    let nick = if m.mine { UserSettingsKey::DmYou.get(current_language()) } else { peer_label };
    ctx.set_font("600 10px sans-serif");
    ctx.set_fill_color(nick_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let nick_fit = crate::ui::widgets::fit_text_to_width(ctx, nick, bubble_w - 24.0);
    ctx.fill_text(&nick_fit, bx + 10.0, y + BUBBLE_PAD_V);

    // Body lines — plain fill_text; the selection highlight (below) paints
    // a translucent overlay on top, same paint order the Agents chat's own
    // selection uses.
    let (body_font, body_color, mono): (&str, &str, bool) = if visual.is_ciphertext_style {
        ("11px monospace", tt.item_text_muted.as_str(), true)
    } else if m.mine {
        ("12px sans-serif", mine_text, false)
    } else {
        ("12px sans-serif", tt.item_text.as_str(), false)
    };
    ctx.set_font(body_font);
    ctx.set_fill_color(body_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let line_x = bx + 10.0;
    let block = msg_idx as i64;
    for (i, line) in visual.lines.iter().enumerate() {
        let line_y = y + BUBBLE_PAD_V + BUBBLE_NICK_H + i as f64 * BUBBLE_LINE_H;
        ctx.fill_text(line, line_x, line_y);
        result.dm_message_line_rects.push(crate::ui::selectable_text::LineHit {
            block,
            line: i as u16,
            y_top: line_y,
            y_bot: line_y + BUBBLE_LINE_H,
            text: line.clone(),
            text_x: line_x,
            width: ctx.measure_text(line),
            mono,
        });
    }

    // Selection highlight — see this function's own doc.
    if let Some(sel) = selection.filter(|s| !s.is_empty()) {
        let ((lo_block, lo_line, lo_char), (hi_block, hi_line, hi_char)) = sel.ordered();
        ctx.set_font(body_font);
        ctx.set_fill_color(&tt.selection);
        ctx.set_global_alpha(0.35);
        for (i, line) in visual.lines.iter().enumerate() {
            let line_idx = i as u16;
            let pos = (block, line_idx);
            if pos < (lo_block, lo_line) || pos > (hi_block, hi_line) {
                continue;
            }
            let line_y = y + BUBBLE_PAD_V + BUBBLE_NICK_H + i as f64 * BUBBLE_LINE_H;
            let is_first = pos == (lo_block, lo_line);
            let is_last = pos == (hi_block, hi_line);
            let line_width = ctx.measure_text(line);
            let prefix_width = |n: u16| -> f64 {
                let prefix: String = line.chars().take(n as usize).collect();
                ctx.measure_text(&prefix)
            };
            let (sel_x, sel_w) = crate::ui::selectable_text::highlight_span(
                line_x, line_width, lo_char, hi_char, is_first, is_last, prefix_width,
            );
            if sel_w > 0.0 {
                ctx.fill_rect(sel_x, line_y, sel_w, BUBBLE_LINE_H);
            }
        }
        ctx.set_global_alpha(1.0);
    }

    // Clock — bottom-right inside the bubble.
    ctx.set_font("10px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Right);
    ctx.set_text_baseline(TextBaseline::Top);
    let time_y = y + BUBBLE_PAD_V + BUBBLE_NICK_H + line_count as f64 * BUBBLE_LINE_H;
    ctx.fill_text(clock_hm(&m.created_at), bx + bubble_w - 8.0, time_y);

    bubble_h + BUBBLE_GAP
}

/// Total scrollable content height for a conversation's right pane —
/// header (peer name + fingerprint line, plus the peer-key-changed notice
/// when `key_changed`), the error line, the "Load older" row when
/// `has_older`, and every message bubble/day-separator. `pub` (unlike this
/// file's other geometry helpers) so the shell driver can size the "keep
/// the viewport anchored" scroll adjustment after a "Load older" page
/// prepends new messages, without duplicating this file's own bubble
/// geometry a second time (owner review 2026-09-23, `forum-messenger-
/// realtime.md` item 1). Takes plain scalars rather than `&DmState` so the
/// caller can measure BEFORE and AFTER mutating `DmView::Conversation::
/// messages` without two conflicting borrows of the same state.
pub fn conversation_scroll_h(
    available: bool,
    has_error: bool,
    key_changed: bool,
    has_older: bool,
    messages: &[DmMessageUi],
) -> f64 {
    let header_h = 36.0 + if key_changed { KEY_CHANGED_NOTICE_H } else { 0.0 };
    let err_h = if has_error { 20.0 } else { 0.0 };
    let more_h = if has_older { ROW_H } else { 0.0 };
    let msgs_h = if messages.is_empty() {
        24.0
    } else {
        messages
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let sep_h = if day_separator_before(messages, i) { DAY_SEP_H } else { 0.0 };
                sep_h + message_h(m, available, key_changed)
            })
            .sum()
    };
    header_h + err_h + more_h + msgs_h
}

/// The peer-key-changed header notice (owner review 2026-09-23,
/// `forum-messenger-realtime.md` item 2) — one clipped line under the
/// fingerprint line, in the theme's `danger` tone (a security-relevant
/// fact, same tone `draw_error_line` already uses for refusals).
fn draw_key_changed_notice(ctx: &mut dyn RenderContext, tt: &ToolbarTheme, w: f64, x: f64, y: f64) -> f64 {
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&tt.danger);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let msg = crate::ui::widgets::fit_text_to_width(ctx, UserSettingsKey::DmPeerKeyChanged.get(current_language()), w);
    ctx.fill_text(&msg, x, y);
    KEY_CHANGED_NOTICE_H
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
    has_older: bool,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) {
    let key_changed = conversation_key_changed(&state.dm, id);
    let scroll_h = (viewport_h - COMPOSER_H).max(0.0);
    let content_h =
        conversation_scroll_h(state.dm.available, state.dm.last_error.is_some(), key_changed, has_older, messages);

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
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(peer_label, sx, sy);
    sy += 18.0;
    ctx.set_font("10px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.fill_text(&format!("fingerprint: {fingerprint}"), sx, sy);
    sy += 18.0;

    if key_changed {
        sy += draw_key_changed_notice(ctx, tt, sw, sx, sy);
    }

    if let Some(err) = &state.dm.last_error {
        sy += draw_error_line(ctx, tt, sw, sx, sy, err);
    }

    // "Load older" row at the top of the message list (owner review
    // 2026-09-23, `forum-messenger-realtime.md` item 1) — same shape
    // `forum_v2_overlay.rs`'s own "Load more" row uses.
    if has_older {
        draw_expand_row(
            ctx, coord, layer_id, "cloud_overlay:dm:load_older",
            WidgetRect::new(sx, sy, sw, ROW_H - ROW_GAP),
            UserSettingsKey::DmLoadOlder.get(current_language()), false, tt,
        );
        sy += ROW_H;
    }

    if messages.is_empty() {
        draw_status(ctx, tt, sx, sy, sw, "No messages yet.", None);
    } else {
        for (i, m) in messages.iter().enumerate() {
            if day_separator_before(messages, i) {
                let label = day_separator_label(&m.created_at, current_time_ms);
                sy += draw_day_separator(ctx, sx, sy, sw, &label, tt);
            }
            sy += draw_message(
                ctx, sx, sy, sw, i as u16, m, peer_label, state.dm.available, key_changed, state.dm.selection, tt,
                result,
            );
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
    // scrollable clip above.
    let composer_y = y + (viewport_h - COMPOSER_H).max(0.0);
    if !state.dm.available {
        // Vault-locked composer (owner ruling 2026-09-10): a muted hint
        // only — the actual "Open Vault" button already lives once in the
        // pane's own header (`draw_vault_locked_header`), not repeated per
        // conversation.
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(UserSettingsKey::DmUnlockVaultHint.get(current_language()), x, composer_y + 8.0);
        return;
    }
    // Disabled (owner mandate 2026-09-09, `nick_required`) trades the
    // draft field for a hint line and swaps "Send" for the same
    // "Choose a nick" trigger the recipient-search row
    // ([`draw_open_peer_row`]) uses — same `COMPOSER_H` footprint either
    // way, so the scroll math above never has to know which is showing.
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
