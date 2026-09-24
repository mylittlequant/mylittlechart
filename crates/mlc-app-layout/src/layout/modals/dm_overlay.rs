//! Direct messages (paid, end-to-end encrypted) — sizing helpers and shared
//! row/field/notice painters for the Community modal's Messages tab.
//!
//! Messenger UI-2a moved the tab's own PAINT (folder rail, chat list, chat
//! pane, bubbles) to `messenger_overlay.rs`, which reads through
//! `mlc_messenger_model::MessengerProvider` instead of `DmState` directly
//! — see that file's own top doc. This file keeps: the frame-sizing entry
//! points `community.rs` still calls ([`list_height`]/[`height`], plus the
//! `conversation_scroll_h`/`message_h`/`message_lines` chain they sit on)
//! and the small paint helpers `messenger_overlay.rs` reuses rather than
//! duplicating ([`draw_field`], [`draw_open_peer_row`],
//! [`draw_lookup_dropdown`], [`draw_vault_locked_header`],
//! [`draw_key_changed_notice`], [`draw_lock_glyph`], [`draw_error_line`]).
//! State lives in `CommunityState::dm` (`DmState`); see that type's own doc
//! for the decryption-stage contract (`DmMessageBody`) — the shell decrypts
//! what it can before this crate ever sees a message, but a sealed vault
//! (or an unresolved peer key) means it hands over raw ciphertext instead
//! of refusing to hand over anything at all. Widget ids stay the pre-
//! existing `cloud_overlay:dm:*` prefix (community-modal arc W1/W2) —
//! `messenger_overlay.rs` draws under the SAME ids, so `mlc-app`'s
//! `panel_click.rs` needed no id rename for this move.

use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::i18n::{current_language, UserSettingsKey};
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::layout::render_chart::FrameTheme;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::ui::modal_settings::{CommunityState, DmMessageBody, DmMessageUi, DmState, DmUnavailableReason, DmView};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig, SETTINGS_BUTTON_RADIUS};
use crate::ui::widgets::types::WidgetState;

use super::cloud_overlay::{
    draw_action_row, draw_status, parse_rfc3339_unix_secs, wrap_text, CloudOverlayResult, DM_PEER_FIELD,
    FIELD_H, ROW_GAP, ROW_H,
};

const CONV_ROW_H: f64 = 52.0;
/// Message bubble text wraps at this many characters — narrower than the
/// Forum body's own 62 (`forum_v2_overlay`'s `blocks`) since a bubble only
/// takes up a fraction of the column, not the full width (see
/// `messenger_overlay.rs`'s own `BUBBLE_WIDTH_FRAC`).
const BUBBLE_WRAP_COLS: usize = 40;
/// The docked send box's own height — draft field + gap + Send button.
const COMPOSER_H: f64 = 8.0 + FIELD_H + 8.0 + ROW_H;

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
// Bubble-height geometry (owner ruling 2026-09-10) — see this file's own
// top doc; the matching PAINT constants now live in `messenger_overlay.rs`.
// =============================================================================

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


/// One clipped error line, in the theme's own `danger` tone — mirrors
/// `forum_v2_overlay`'s own `draw_error_line`; not shared because pulling
/// one function up to `cloud_overlay.rs` for two four-line call sites is
/// not worth the extra public surface.
pub(crate) fn draw_error_line(ctx: &mut dyn RenderContext, tt: &ToolbarTheme, w: f64, x: f64, y: f64, msg: &str) -> f64 {
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
pub(crate) fn draw_lock_glyph(ctx: &mut dyn RenderContext, cx: f64, cy: f64, size: f64, color: &str) {
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
pub(crate) fn draw_vault_locked_header(
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

pub(crate) fn draw_field(
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
pub(crate) fn draw_open_peer_row(
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
pub(crate) fn draw_lookup_dropdown(
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

/// Wrapped line count for one message's own bubble — the only part of the
/// old per-frame `MessageVisual` (draw-time paint styling: mono/lock-glyph,
/// nick/body colour) that survives now that `messenger_overlay.rs` owns
/// the actual paint (see this file's own top doc). [`height`]/
/// [`conversation_scroll_h`] still need the LINE COUNT for sizing, so this
/// stays the one place that decides how a `DmMessageBody` wraps.
///
/// `key_changed` — whether THIS message's conversation has a detected
/// peer-key change ([`DmState::has_peer_key_changed`]) — only changes the
/// `Undecryptable` branch's own wording (owner review 2026-09-23,
/// `forum-messenger-realtime.md` item 2): a bubble sealed under a key the
/// peer has since rotated away from reads differently from a generic
/// decrypt failure.
fn message_lines(m: &DmMessageUi, available: bool, key_changed: bool) -> Vec<String> {
    match &m.body {
        DmMessageBody::Plain(text) => wrap_text(text, BUBBLE_WRAP_COLS),
        DmMessageBody::Undecryptable => {
            let text = if key_changed {
                UserSettingsKey::DmUndecryptableKeyChanged.get(current_language()).to_string()
            } else {
                "<cannot decrypt>".to_string()
            };
            vec![text]
        }
        DmMessageBody::Ciphertext { ciphertext_b64, .. } => {
            if available {
                // Not vault-locked — a peer-key backfill just has not
                // landed for this conversation yet (rare, transient): the
                // shell's own per-frame redecrypt-in-place retry clears
                // this the moment it does, no user action needed, so this
                // reads as ordinary bubble text rather than the locked
                // ciphertext block.
                vec!["Decrypting…".to_string()]
            } else {
                let mut lines = wrap_text(ciphertext_b64, BUBBLE_WRAP_COLS);
                if lines.len() > CIPHERTEXT_MAX_LINES {
                    lines.truncate(CIPHERTEXT_MAX_LINES);
                    if let Some(last) = lines.last_mut() {
                        last.push('…');
                    }
                }
                lines
            }
        }
    }
}

fn message_h(m: &DmMessageUi, available: bool, key_changed: bool) -> f64 {
    let line_count = message_lines(m, available, key_changed).len().max(1);
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
pub(crate) fn draw_key_changed_notice(ctx: &mut dyn RenderContext, tt: &ToolbarTheme, w: f64, x: f64, y: f64) -> f64 {
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&tt.danger);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let msg = crate::ui::widgets::fit_text_to_width(ctx, UserSettingsKey::DmPeerKeyChanged.get(current_language()), w);
    ctx.fill_text(&msg, x, y);
    KEY_CHANGED_NOTICE_H
}

