//! Messenger UI-2a — the Community modal's Messages tab body, drawn ONLY
//! from [`mlc_messenger_model::MessengerProvider`] (`DmMessengerProviderView`,
//! today's read-only view over `DmState` — see that type's own doc for why
//! this paint pass never holds `&mut DmState`) instead of reading `DmState`
//! fields ad hoc the way `dm_overlay.rs` still does. `Capabilities` gates
//! what draws: today's DM adapter reports every flag false except
//! `receipts` (`dm_messenger_provider::reads::capabilities`), so this file
//! never draws group/channel chrome, custom folders, reactions, edit
//! badges, reply previews or a typing indicator — not because this file
//! forgot them, but because the provider says they do not exist here.
//!
//! Two things the model genuinely cannot carry are read straight off
//! `state.dm`/`state.forum2` instead, each documented at its own read site
//! rather than silently papered over:
//! - the peer's short key FINGERPRINT STRING (`DmConversationUi::
//!   peer_fingerprint`) — [`Member::has_messaging_key`] says WHETHER a key
//!   exists, but the model carries no fingerprint text field at all;
//! - "which room is open" and the nick-required/nick-search flow
//!   (`DmState::view`, `ForumV2State::nick_required`, the lookup dropdown)
//!   — [`MessengerProvider`] has no navigation or account-ladder concept;
//!   picking a room and choosing a nick are UI-side facts, not provider
//!   reads, and searching for someone to message is a DM-specific server
//!   endpoint with no [`MessengerIntent`] equivalent yet.
//!
//! Reuses `dm_overlay.rs`'s own row/field/notice painters (widened to
//! `pub(crate)` for this file) rather than duplicating them — this file's
//! own job is the NEW three-column shape (folder rail / chat list / chat
//! pane) and reading through the provider trait, not re-deriving text-
//! field or button painting a second time. Widget ids are the exact
//! `cloud_overlay:dm:*` ids `dm_overlay.rs` already used (community-modal
//! arc W1/W2) plus exactly one new one this piece adds
//! (`cloud_overlay:dm:jump_latest`, the bottom-anchored feed's own
//! "jump to latest" button) — `mlc-app`'s `panel_click.rs` needs no other
//! new arm, so a click anywhere else in this tab keeps working unchanged.

use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::i18n::{current_language, UserSettingsKey};
use mlc_messenger_model::{
    MembershipChange, MembershipEvent, MessageContent, MessengerProvider, RoomId, SendState,
    Timestamp, UtdReason, VaultState,
};
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::layout::render_chart::FrameTheme;
use crate::ui::dm_messenger_provider::DmMessengerProviderView;
use crate::ui::modal_settings::CommunityState;
use crate::ui::scroll_widget::{jump_button_visible, ScrollableContainer};
use crate::ui::selectable_text::{highlight_rect, LineHit, Selection};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::avatar::contrasting_text_color;
use crate::ui::widgets::{draw_avatar_placeholder, SETTINGS_BUTTON_RADIUS};

use super::cloud_overlay::{
    draw_action_row, draw_column_divider, draw_expand_row, draw_status, register_scrollbar_hit_zones,
    scroll_config, wrap_text, CloudOverlayResult, COL_GAP, DM_CHAT_FILTER_FIELD, FIELD_H, LIST_COL_W,
    ROW_GAP, ROW_H,
};
use super::community::CommunityListPaneResult;
use super::dm_overlay::{
    draw_error_line, draw_field, draw_key_changed_notice, draw_lock_glyph, draw_lookup_dropdown,
    draw_open_peer_row,
};

const RAIL_W: f64 = 68.0;

// Bubble geometry — mirrors `dm_overlay.rs`'s own constants (same numeric
// values) so a conversation opened through either path reads identically;
// duplicated rather than imported since those are file-private consts over
// there, and re-deriving four numbers is cheaper than widening a whole
// geometry module's visibility for it.
const BUBBLE_WRAP_COLS: usize = 40;
const BUBBLE_WIDTH_FRAC: f64 = 0.72;
const BUBBLE_MIN_W: f64 = 120.0;
const BUBBLE_AVATAR_D: f64 = 28.0;
const BUBBLE_AVATAR_GAP: f64 = 8.0;
const LIST_AVATAR_D: f64 = 32.0;
const LIST_AVATAR_GAP: f64 = 8.0;
const BUBBLE_LINE_H: f64 = 16.0;
const BUBBLE_NICK_H: f64 = 14.0;
const BUBBLE_TIME_H: f64 = 14.0;
const BUBBLE_PAD_V: f64 = 8.0;
const BUBBLE_GAP: f64 = 10.0;
const DAY_SEP_H: f64 = 24.0;
const SYSTEM_NOTICE_H: f64 = 20.0;
const CIPHERTEXT_MAX_LINES: usize = 3;
const CONV_ROW_H: f64 = 52.0;
const RAIL_ROW_H: f64 = 40.0;
/// The chat-list header row: the name-filter field plus its own pencil
/// button (concept v1's `.lhead`: `[Search][pencil]`) — one fixed row,
/// always drawn, above the (now conditional) "New message" recipient
/// search and the room rows themselves.
const CHAT_FILTER_ROW_H: f64 = FIELD_H + 12.0;
/// The pencil button's own square footprint, to the filter field's right.
const CHAT_FILTER_PENCIL_W: f64 = 32.0;
const CHAT_FILTER_GAP: f64 = 8.0;
/// The docked send box's own height — draft field + gap + Send button, the
/// SAME footprint every dock state uses (locked/peer-without-key/nick-
/// required all fit inside it too — see [`draw_composer`]'s own doc) so
/// the pane's scroll math never has to change per dock state.
const COMPOSER_H: f64 = 8.0 + FIELD_H + 8.0 + ROW_H;

/// `TimelineItem::sender`'s own opaque id for the signed-in caller's own
/// messages — see `dm_messenger_provider::ME`'s own doc: this is the same
/// fixed marker that adapter stamps onto every "mine" message, documented
/// there as "stable within one conversation, which is all a single-
/// provider UI ever compares it against". A future Matrix-shaped provider
/// stamps the caller's OWN Matrix user id instead — this file would need a
/// real `MessengerProvider::my_user_id()` method to keep working against
/// it, a gap noted for that provider's own author rather than guessed at
/// here.
const ME: &str = "me";

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
    let provider = DmMessengerProviderView::new(&state.dm);
    let caps = provider.capabilities();

    let list_x = x + RAIL_W;
    let list_w = LIST_COL_W;
    let pane_x = list_x + list_w + COL_GAP;
    let pane_w = (w - RAIL_W - list_w - COL_GAP).max(0.0);

    draw_folder_rail(ctx, x, y, RAIL_W, viewport_h, &provider, tt);
    draw_column_divider(ctx, pane_x - COL_GAP / 2.0, y, viewport_h, tt);

    draw_chat_list(
        ctx, coord, layer_id, list_x, y, list_w, viewport_h, state, &provider, ft, tt, current_time_ms,
        result, list_result,
    );

    draw_chat_pane(
        ctx, coord, layer_id, pane_x, y, pane_w, viewport_h, state, &provider, caps, ft, tt,
        current_time_ms, result,
    );
}

// =============================================================================
// Left rail — built-in folders only (`capabilities().folders == false`
// today: [`Folder`]'s own doc says built-ins draw regardless of that flag,
// it only gates CUSTOM folder CRUD — see this file's own top doc). Purely
// informational this pass: switching which folder is active needs UI-side
// selection state this piece does not add (no new dispatch arm beyond
// `jump_latest` — see top doc); "All chats" always reads as the active one.
// =============================================================================

fn draw_folder_rail(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    provider: &DmMessengerProviderView,
    tt: &ToolbarTheme,
) {
    ctx.set_fill_color(&tt.dropdown_bg);
    ctx.fill_rect(x, y, w, h);

    let rooms = provider.rooms();
    let personal_count: i64 = rooms.iter().map(|r| r.notification_count as i64).sum();
    let rows: [(&str, i64, bool); 2] = [("All chats", personal_count, true), ("Personal", personal_count, false)];

    let mut ry = y + 6.0;
    for (label, count, active) in rows {
        let rect = WidgetRect::new(x, ry, w, RAIL_ROW_H);
        if active {
            ctx.set_fill_color(&tt.accent);
            ctx.fill_rect(rect.x, rect.y + 4.0, 3.0, rect.height - 8.0);
        }
        ctx.set_font(if active { "600 11px sans-serif" } else { "11px sans-serif" });
        ctx.set_fill_color(if active { &tt.item_text } else { &tt.item_text_muted });
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Top);
        let fit = crate::ui::widgets::fit_text_to_width(ctx, label, w - 8.0);
        ctx.fill_text(&fit, rect.center_x(), rect.y + 6.0);
        if count > 0 {
            ctx.set_font("9px sans-serif");
            ctx.set_fill_color(&tt.item_text_muted);
            ctx.fill_text(&count.to_string(), rect.center_x(), rect.y + 22.0);
        }
        ry += RAIL_ROW_H;
    }
}

// =============================================================================
// Chat list column — the header's own name-filter row
// ([`draw_chat_filter_row`], UI-2b piece 2), the "New message" recipient
// search ([`draw_open_peer_row`], unchanged ids — now drawn only while
// [`mlc_app_layout::ui::modal_settings::DmState::new_conversation_open`],
// toggled behind the header's own pencil button), and one row per
// (filtered) [`RoomSummary`].
// =============================================================================

/// The header's own filter field: a plain, always-visible client-side
/// substring match against each already-loaded [`RoomSummary::name`] —
/// unlike [`draw_open_peer_row`]'s recipient search, this never touches
/// the server (concept v1's own `.lhead` distinguishes the two: `Search`
/// filters what's already on screen, the pencil is what starts something
/// new). Registers `cloud_overlay:dm:chat_filter_input` (a real
/// `TextFieldStore` field, `mlc_app::text_input::DM_CHAT_FILTER` — see
/// `cloud_overlay::DM_CHAT_FILTER_FIELD`'s own doc for why the literal is
/// spelled twice) and `cloud_overlay:dm:new_conversation_toggle` (the
/// pencil button; `mlc-app`'s `panel_click.rs` flips `new_conversation_
/// open`). Returns the row's own height.
#[allow(clippy::too_many_arguments)]
fn draw_chat_filter_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    new_conversation_open: bool,
    result: &mut CloudOverlayResult,
) -> f64 {
    let pencil_w = CHAT_FILTER_PENCIL_W;
    let field_w = (w - pencil_w - CHAT_FILTER_GAP).max(60.0);
    let field_rect = WidgetRect::new(x, y, field_w, FIELD_H);
    let (rect, chars) = draw_field(
        ctx, coord, layer_id, "cloud_overlay:dm:chat_filter_input", DM_CHAT_FILTER_FIELD, field_rect,
        "Search", ft, tt, current_time_ms,
    );
    result.dm_chat_filter_rect = Some(rect);
    result.dm_chat_filter_char_positions = chars;

    let pencil_rect = WidgetRect::new(x + field_w + CHAT_FILTER_GAP, y, pencil_w, FIELD_H);
    let pencil_id = WidgetId::from("cloud_overlay:dm:new_conversation_toggle");
    let hovered = coord.is_hovered(&pencil_id);
    if hovered || new_conversation_open {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(pencil_rect.x, pencil_rect.y, pencil_rect.width, pencil_rect.height, SETTINGS_BUTTON_RADIUS);
    }
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(if new_conversation_open { &tt.accent } else { &tt.item_text_muted });
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("\u{270E}", pencil_rect.center_x(), pencil_rect.center_y());
    coord.register_on_layer(pencil_id.as_str(), pencil_rect, Sense::CLICK | Sense::HOVER, layer_id);

    CHAT_FILTER_ROW_H
}

#[allow(clippy::too_many_arguments)]
fn draw_chat_list(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    viewport_h: f64,
    state: &CommunityState,
    provider: &DmMessengerProviderView,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
    list_result: &mut CommunityListPaneResult,
) {
    let mut rooms = provider.rooms();
    let had_any_room = !rooms.is_empty();
    let filter_query = coord.text_fields().text(&WidgetId::from(DM_CHAT_FILTER_FIELD)).trim().to_lowercase();
    if !filter_query.is_empty() {
        rooms.retain(|r| r.name.to_lowercase().contains(&filter_query));
    }
    let list_drawn_h = list_content_h(state, &rooms);
    let viewport_rect = WidgetRect::new(x, y, w, viewport_h);
    coord.register_on_layer("cloud_overlay:scroll_viewport", viewport_rect, Sense::SCROLL, layer_id);
    let scrollable = ScrollableContainer::new(viewport_rect, &state.list_scroll, Some(scroll_config()));
    scrollable.begin(ctx);
    let lx = x;
    let mut ly = scrollable.content_y();
    let lw = scrollable.content_width();

    ly += draw_chat_filter_row(
        ctx, coord, layer_id, lx, ly, lw, ft, tt, current_time_ms, state.dm.new_conversation_open, result,
    );

    if state.dm.new_conversation_open {
        ly += draw_open_peer_row(
            ctx, coord, layer_id, lx, ly, lw, ft, tt, current_time_ms, state.forum2.nick_required(), result,
        );
        draw_lookup_dropdown(ctx, coord, layer_id, result.dm_peer_rect, &state.dm, tt);
    }

    if rooms.is_empty() {
        let msg = if had_any_room { "No matches." } else { "No conversations yet." };
        draw_status(ctx, tt, lx, ly, lw, msg, None);
    } else {
        for r in &rooms {
            ly += draw_room_row(ctx, coord, layer_id, lx, ly, lw, r, state, current_time_ms, tt);
        }
    }

    let widget_theme = crate::ui::widgets::types::WidgetTheme::default();
    let scroll_result = scrollable.end(ctx, list_drawn_h, &widget_theme);
    list_result.viewport_rect = Some(viewport_rect);
    list_result.total_content_height = Some(list_drawn_h);
    list_result.viewport_height = Some(viewport_h.min(list_drawn_h));
    list_result.scrollbar_handle_rect = scroll_result.handle_rect;
    list_result.scrollbar_track_rect = scroll_result.track_rect;
    register_scrollbar_hit_zones(coord, layer_id, &scroll_result);
}

fn open_peer_row_h(nick_required: bool) -> f64 {
    if nick_required {
        20.0 + (ROW_H - ROW_GAP) + 12.0
    } else {
        FIELD_H + 12.0
    }
}

/// `rooms` is the caller's own ALREADY-FILTERED slice (see
/// `draw_chat_list`'s own filter step) — sizing off the filtered count is
/// correct here: the scroll container only ever needs room for what this
/// frame actually draws below the header.
fn list_content_h(state: &CommunityState, rooms: &[mlc_messenger_model::RoomSummary]) -> f64 {
    let open_h = if state.dm.new_conversation_open { open_peer_row_h(state.forum2.nick_required()) } else { 0.0 };
    let rows_h = if rooms.is_empty() { 24.0 } else { rooms.len() as f64 * CONV_ROW_H };
    CHAT_FILTER_ROW_H + open_h + rows_h
}

#[allow(clippy::too_many_arguments)]
fn draw_room_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    r: &mlc_messenger_model::RoomSummary,
    state: &CommunityState,
    current_time_ms: u64,
    tt: &ToolbarTheme,
) -> f64 {
    let widget_id = format!("cloud_overlay:dm:conversation:{}", r.id.as_str());
    let rect = WidgetRect::new(x, y, w, CONV_ROW_H - ROW_GAP);
    let hovered = coord.is_hovered(&WidgetId::from(widget_id.as_str()));
    let open = matches!(&state.dm.view, crate::ui::modal_settings::DmView::Conversation { id, .. } if id.to_string() == r.id.as_str());
    if open {
        ctx.set_fill_color(&tt.item_bg_active);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    } else if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    }

    let avatar_cx = x + 8.0 + LIST_AVATAR_D / 2.0;
    let avatar_cy = rect.y + rect.height / 2.0;
    draw_avatar_placeholder(ctx, avatar_cx, avatar_cy, LIST_AVATAR_D / 2.0, &r.name, &tt.item_bg_active);
    let text_x = x + 8.0 + LIST_AVATAR_D + LIST_AVATAR_GAP;

    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color(if hovered || open { &tt.item_text_hover } else { &tt.item_text });
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let label = crate::ui::widgets::fit_text_to_width(ctx, &r.name, (x + w - text_x - 52.0).max(0.0));
    ctx.fill_text(&label, text_x, y + 4.0);

    if r.notification_count > 0 {
        let badge_label = if r.notification_count > 99 { "99+".to_string() } else { r.notification_count.to_string() };
        let badge_w = 16.0 + badge_label.len() as f64 * 6.0;
        let badge_rect = WidgetRect::new(x + w - badge_w - 8.0, y + 4.0, badge_w, 18.0);
        ctx.set_fill_color(&tt.accent);
        ctx.fill_rounded_rect(badge_rect.x, badge_rect.y, badge_rect.width, badge_rect.height, 9.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(contrasting_text_color(&tt.accent));
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&badge_label, badge_rect.center_x(), badge_rect.center_y());
    } else if let Some(ev) = &r.last_event {
        // The unread badge and the last-message time share the row's
        // top-right corner — an unread count is the more urgent fact, so
        // the time only draws when there is no badge to show instead.
        ctx.set_font("10px monospace");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Right);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(&row_time_label(ev.ts, current_time_ms), x + w - 8.0, y + 4.0);
    }

    // Preview line — only when the provider actually has one
    // (`RoomSummary::last_event`, honestly `None` for today's DM adapter;
    // see this file's own top doc). Nothing fabricated when it is absent.
    if let Some(ev) = &r.last_event {
        ctx.set_font("11px monospace");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        let (label, locked) = match &ev.content {
            mlc_messenger_model::PreviewContent::Text(t) => (t.clone(), false),
            mlc_messenger_model::PreviewContent::Encrypted => ("Encrypted message".to_string(), true),
            mlc_messenger_model::PreviewContent::Redacted => ("Message deleted".to_string(), false),
            mlc_messenger_model::PreviewContent::Notice(n) => (n.clone(), false),
        };
        let mut px = text_x;
        if locked {
            draw_lock_glyph(ctx, px + 4.0, y + 24.0 + 4.0, 8.0, &tt.item_text_muted);
            px += 12.0;
        }
        let fit = crate::ui::widgets::fit_text_to_width(ctx, &label, (x + w - px - 8.0).max(0.0));
        ctx.fill_text(&fit, px, y + 24.0);
    }

    // `RIGHT_CLICK` alongside `CLICK` — the messenger UI-2b chat-row context
    // menu (`ChartApp::try_messenger_right_click`, `mlc-app`) hit-tests this
    // SAME registered widget through `InputCoordinator::process_right_click`
    // rather than a second, parallel rect list.
    coord.register_on_layer(widget_id.as_str(), rect, Sense::CLICK | Sense::HOVER | Sense::RIGHT_CLICK, layer_id);
    CONV_ROW_H
}

/// A chat-list row's own last-message time — `HH:MM` for something that
/// landed today, the same Today/Yesterday/date label [`draw_day_separator`]
/// uses otherwise (today's DM adapter never actually reaches the "today"
/// branch here in practice, since that's exactly what `clock_hm` already
/// covers — kept as one shared rule with the timeline's own day labels
/// rather than a second, narrower one).
fn row_time_label(ts: Timestamp, now_ms: u64) -> String {
    if day_key(ts) == day_key(Timestamp(now_ms as i64)) {
        clock_hm(ts)
    } else {
        day_label(ts, now_ms)
    }
}

// =============================================================================
// Chat pane — header, day separators + bubbles/system notices from
// [`MessengerProvider::timeline`], a bottom-anchored feed with a
// jump-to-latest button, and the docked composer ([`draw_composer`], UI-2a
// piece 3): the single place every reason the composer cannot send yet
// shows up, one calm line at a time — no separate pane-top notice.
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn draw_chat_pane(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    viewport_h: f64,
    state: &CommunityState,
    provider: &DmMessengerProviderView,
    caps: mlc_messenger_model::Capabilities,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) {
    use crate::ui::modal_settings::{DmUnavailableReason, DmView};

    // Access-ladder gate — not a `VaultState` fact (see
    // `dm_messenger_provider::reads::vault_state`'s own doc), read
    // straight off `state.dm` same as `dm_overlay.rs` does today.
    if matches!(state.dm.unavailable_reason, DmUnavailableReason::NotPaid) {
        if let Some(err) = &state.dm.last_error {
            draw_error_line(ctx, tt, w, x, y, err);
        }
        return;
    }

    let locked = matches!(provider.vault_state(), VaultState::Locked);
    let body_y = y;
    let body_viewport_h = viewport_h;

    let DmView::Conversation { id, has_older, .. } = &state.dm.view else {
        draw_status(
            ctx, tt, x, body_y, w,
            "Select a conversation from the list on the left, or start a new one above.", None,
        );
        return;
    };
    if !state.dm.available && !locked {
        if let Some(err) = &state.dm.last_error {
            draw_error_line(ctx, tt, w, x, body_y, err);
        }
        return;
    }

    let room_id = RoomId::from(id.to_string());
    let room = provider.room(&room_id);
    let members = provider.members(&room_id);
    let timeline = provider.timeline(&room_id);
    let peer = members.iter().find(|m| m.user_id.as_str() != ME);
    let peer_label = room.as_ref().map(|r| r.name.clone()).unwrap_or_else(|| "Unknown".to_string());
    // Fingerprint STRING — the model has no field for it (see this file's
    // own top doc); [`Member::has_messaging_key`] backs the "no messaging
    // key yet" fallback, the fingerprint text itself is read off
    // `DmConversationUi` by id, same lookup `dm_overlay.rs` already did.
    let fingerprint = state.dm.conversations.iter().find(|c| c.id == *id).and_then(|c| c.peer_fingerprint.as_deref());
    let has_key = peer.map(|m| m.has_messaging_key).unwrap_or(false);
    let key_changed = state.dm.has_peer_key_changed(*id);

    let scroll_h = (body_viewport_h - COMPOSER_H).max(0.0);
    let viewport_rect = WidgetRect::new(x, body_y, w, scroll_h);
    coord.register_on_layer("cloud_overlay:scroll_viewport", viewport_rect, Sense::SCROLL, layer_id);
    let scrollable = ScrollableContainer::new(viewport_rect, &state.scroll, Some(scroll_config()));
    scrollable.begin(ctx);
    let sx = x;
    let mut sy = scrollable.content_y();
    let sw = scrollable.content_width();
    let content_top = sy;

    ctx.set_font("600 14px sans-serif");
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(&peer_label, sx, sy);
    sy += 18.0;
    ctx.set_font("10px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    let key_line = match fingerprint {
        Some(fp) => format!("fingerprint: {fp}"),
        None if has_key => "fingerprint: (published)".to_string(),
        None => "no messaging key yet".to_string(),
    };
    ctx.fill_text(&key_line, sx, sy);
    sy += 18.0;

    if key_changed {
        sy += draw_key_changed_notice(ctx, tt, sw, sx, sy);
    }
    if let Some(err) = &state.dm.last_error {
        sy += draw_error_line(ctx, tt, sw, sx, sy, err);
    }

    let has_older = provider.back_pagination(&room_id).has_more || *has_older;
    if has_older {
        draw_expand_row(
            ctx, coord, layer_id, "cloud_overlay:dm:load_older",
            WidgetRect::new(sx, sy, sw, ROW_H - ROW_GAP),
            UserSettingsKey::DmLoadOlder.get(current_language()), false, tt,
        );
        sy += ROW_H;
    }

    if timeline.is_empty() {
        draw_status(ctx, tt, sx, sy, sw, "No messages yet.", None);
    } else {
        let mut prev_day: Option<i64> = None;
        let mut msg_idx: u16 = 0;
        for item in &timeline {
            let day = day_key(item.ts);
            if prev_day != Some(day) {
                let label = day_label(item.ts, current_time_ms);
                sy += draw_day_separator(ctx, sx, sy, sw, &label, tt);
                prev_day = Some(day);
            }
            if let Some(notice) = system_notice_text(&item.content) {
                sy += draw_system_notice(ctx, sx, sy, sw, &notice, tt);
                continue;
            }
            let mine = item.sender.as_str() == ME;
            let peer_name = peer.map(|m| m.display_name.as_str()).unwrap_or(peer_label.as_str());
            sy += draw_bubble(
                ctx, sx, sy, sw, msg_idx, item, mine, peer_name, provider.vault_state(), caps,
                state.dm.selection, tt, result,
            );
            msg_idx += 1;
        }
    }
    let content_h = sy - content_top;

    let widget_theme = crate::ui::widgets::types::WidgetTheme::default();
    let scroll_result = scrollable.end(ctx, content_h, &widget_theme);
    result.total_content_height = Some(content_h);
    result.viewport_height = Some(scroll_h);
    result.scroll_viewport_rect = Some(viewport_rect);
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
    register_scrollbar_hit_zones(coord, layer_id, &scroll_result);

    // Floating "jump to latest" — visible only while scrolled away from
    // the bottom (`jump_button_visible`, read-only: see this file's own
    // top doc for why LANDING at the bottom on open/new-message is not
    // wired this pass). Its own click is the one new widget id this piece
    // adds (`panel_click.rs`'s `"jump_latest"` arm).
    if jump_button_visible(&state.scroll, content_h, scroll_h) {
        draw_jump_to_latest(ctx, coord, layer_id, x, body_y, w, scroll_h, tt);
    }

    draw_composer(
        ctx, coord, layer_id, x, body_y, w, body_viewport_h, state, locked, has_key, tt, ft, current_time_ms,
        result,
    );
}

fn draw_jump_to_latest(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    scroll_h: f64,
    tt: &ToolbarTheme,
) {
    let label = "\u{2193} Jump to latest";
    ctx.set_font("600 11px sans-serif");
    let text_w = ctx.measure_text(label);
    let pill_w = text_w + 24.0;
    let pill_h = 26.0;
    let rect = WidgetRect::new(x + (w - pill_w) / 2.0, y + scroll_h - pill_h - 10.0, pill_w, pill_h);
    let hovered = coord.is_hovered(&WidgetId::from("cloud_overlay:dm:jump_latest"));
    ctx.set_fill_color(if hovered { &tt.accent_hover } else { &tt.accent });
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, rect.height / 2.0);
    ctx.set_fill_color(contrasting_text_color(&tt.accent));
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, rect.center_x(), rect.center_y());
    coord.register_on_layer("cloud_overlay:dm:jump_latest", rect, Sense::CLICK | Sense::HOVER, layer_id);
}

/// The dock — the ONE place every reason the composer cannot send yet
/// shows up (UI-2a piece 3, owner spec): a nick-required hint keeps its
/// own pre-existing hint+button shape (a ladder-mandate gate, orthogonal
/// to messaging itself); vault-locked and peer-without-key each get
/// exactly the notice the owner asked for, one line (plus a button for
/// the vault case) — replacing the three separate old notices this piece
/// retires: the pane-top vault-locked header, the vault-locked composer's
/// own OLD hint-with-no-button, and the header's old "no key
/// published"/fingerprint-substitution line (`dm_overlay.rs`'s former
/// `draw_right_conversation`, all three gone from this crate now — see
/// this file's own top doc).
#[allow(clippy::too_many_arguments)]
fn draw_composer(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    viewport_h: f64,
    state: &CommunityState,
    locked: bool,
    has_key: bool,
    tt: &ToolbarTheme,
    ft: &FrameTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) {
    let composer_y = y + (viewport_h - COMPOSER_H).max(0.0);
    if state.forum2.nick_required() {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(UserSettingsKey::AccountChooseNickHint.get(current_language()), x, composer_y + 8.0);
        let btn_y = composer_y + 8.0 + FIELD_H + 8.0;
        draw_action_row(
            ctx, coord, layer_id, "cloud_overlay:dm:choose_nick",
            WidgetRect::new(x, btn_y, w, ROW_H - ROW_GAP),
            UserSettingsKey::AccountChooseNickBtn.get(current_language()), tt,
        );
        return;
    }
    // Vault locked (owner spec, UI-2a piece 3): ONE line + ONE accent
    // button, replacing the old pane-top header notice AND the old
    // button-less composer hint together.
    if locked {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        let hint = crate::ui::widgets::fit_text_to_width(
            ctx, "Vault locked. Messages stay encrypted until you unlock it.", w,
        );
        ctx.fill_text(&hint, x, composer_y + 8.0);
        let btn_y = composer_y + 8.0 + FIELD_H + 8.0;
        draw_action_row(
            ctx, coord, layer_id, "cloud_overlay:dm:unlock_vault",
            WidgetRect::new(x, btn_y, w, ROW_H - ROW_GAP), "Unlock vault", tt,
        );
        return;
    }
    // Peer has no messaging key published yet — a calm, no-button line
    // (owner spec): there is nothing to click here, only something to
    // wait for.
    if !has_key {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        let hint = crate::ui::widgets::fit_text_to_width(
            ctx, "This person hasn't set up messaging yet.", w,
        );
        ctx.fill_text(&hint, x, composer_y + 8.0);
        return;
    }
    let crate::ui::modal_settings::DmView::Conversation { draft_field_id, .. } = &state.dm.view else { return };
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

// =============================================================================
// Timeline content -> visual mapping (own equivalent of `dm_overlay.rs`'s
// `message_visual`, over `MessageContent`/`TimelineItem` instead of
// `DmMessageBody`/`DmMessageUi` — the two are different types, so this is
// new code, not a duplicate of an existing function).
// =============================================================================

struct MessageVisual {
    lines: Vec<String>,
    mono: bool,
}

fn content_visual(content: &MessageContent, vault_state: VaultState) -> MessageVisual {
    match content {
        MessageContent::Text(text) => MessageVisual { lines: wrap_text(text, BUBBLE_WRAP_COLS), mono: false },
        MessageContent::Legacy(mlc_messenger_model::LegacyContent::Text(text)) => {
            MessageVisual { lines: wrap_text(text, BUBBLE_WRAP_COLS), mono: false }
        }
        MessageContent::Encrypted { ciphertext_b64 }
        | MessageContent::Legacy(mlc_messenger_model::LegacyContent::Encrypted { ciphertext_b64 }) => {
            if matches!(vault_state, VaultState::Locked) {
                let mut lines = wrap_text(ciphertext_b64, BUBBLE_WRAP_COLS);
                if lines.len() > CIPHERTEXT_MAX_LINES {
                    lines.truncate(CIPHERTEXT_MAX_LINES);
                    if let Some(last) = lines.last_mut() {
                        last.push('…');
                    }
                }
                MessageVisual { lines, mono: true }
            } else {
                MessageVisual { lines: vec!["Decrypting…".to_string()], mono: false }
            }
        }
        MessageContent::Undecryptable(reason) => MessageVisual { lines: vec![undecryptable_text(*reason)], mono: false },
        // System-notice content never reaches this function — see
        // `system_notice_text`, checked by every caller first.
        MessageContent::Notice(_) | MessageContent::Membership(_) | MessageContent::KeyChanged => {
            MessageVisual { lines: Vec::new(), mono: false }
        }
    }
}

fn undecryptable_text(reason: UtdReason) -> String {
    match reason {
        UtdReason::MissingSession => "Waiting for the encryption key…".to_string(),
        UtdReason::WithheldBlacklisted => "This device was blocked from this message.".to_string(),
        UtdReason::WithheldUnverified => "Withheld — this device is not verified.".to_string(),
        UtdReason::WithheldUnauthorised => "Withheld — not a member of this conversation.".to_string(),
        UtdReason::SenderIdentityNotVerified => "Sender identity changed — re-verify to read.".to_string(),
        UtdReason::Unknown => "<cannot decrypt>".to_string(),
    }
}

fn system_notice_text(content: &MessageContent) -> Option<String> {
    match content {
        MessageContent::Notice(s) => Some(s.clone()),
        MessageContent::Membership(ev) => Some(membership_text(ev)),
        MessageContent::KeyChanged => Some("Encryption key changed".to_string()),
        _ => None,
    }
}

fn membership_text(ev: &MembershipEvent) -> String {
    let who = ev.user.as_str();
    match ev.change {
        MembershipChange::Joined => format!("{who} joined"),
        MembershipChange::Left => format!("{who} left"),
        MembershipChange::Invited => format!("{who} was invited"),
        MembershipChange::Removed => format!("{who} was removed"),
        MembershipChange::RoleChanged(_) => format!("{who}'s role changed"),
    }
}

fn draw_system_notice(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, label: &str, tt: &ToolbarTheme) -> f64 {
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    let fit = crate::ui::widgets::fit_text_to_width(ctx, label, w);
    ctx.fill_text(&fit, x + w / 2.0, y + SYSTEM_NOTICE_H / 2.0);
    SYSTEM_NOTICE_H
}

fn draw_day_separator(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, label: &str, tt: &ToolbarTheme) -> f64 {
    ctx.set_font("600 10px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, x + w / 2.0, y + DAY_SEP_H / 2.0);
    DAY_SEP_H
}

#[allow(clippy::too_many_arguments)]
fn draw_bubble(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    w: f64,
    msg_idx: u16,
    item: &mlc_messenger_model::TimelineItem,
    mine: bool,
    peer_label: &str,
    vault_state: VaultState,
    caps: mlc_messenger_model::Capabilities,
    selection: Option<Selection>,
    tt: &ToolbarTheme,
    result: &mut CloudOverlayResult,
) -> f64 {
    let visual = content_visual(&item.content, vault_state);
    let line_count = visual.lines.len().max(1);
    let bubble_inner_h = BUBBLE_NICK_H + line_count as f64 * BUBBLE_LINE_H + BUBBLE_TIME_H;
    let bubble_h = bubble_inner_h + BUBBLE_PAD_V * 2.0;

    let has_avatar = !mine;
    let avatar_reserved = if has_avatar { BUBBLE_AVATAR_D + BUBBLE_AVATAR_GAP } else { 0.0 };
    let avail_w = (w - avatar_reserved).max(0.0);
    let bubble_w = (avail_w * BUBBLE_WIDTH_FRAC).max(BUBBLE_MIN_W);
    let bx = if mine { x + w - bubble_w } else { x + avatar_reserved };

    if has_avatar {
        let avatar_cx = x + BUBBLE_AVATAR_D / 2.0;
        let avatar_cy = y + bubble_h / 2.0;
        draw_avatar_placeholder(ctx, avatar_cx, avatar_cy, BUBBLE_AVATAR_D / 2.0, peer_label, &tt.item_bg_active);
    }

    let mine_bg = mlc_core::mix_colors(&tt.dropdown_bg, &tt.accent, 0.25);
    let mine_text = contrasting_text_color(&mine_bg);
    ctx.set_fill_color(if mine { &mine_bg } else { &tt.dropdown_bg });
    ctx.fill_rounded_rect(bx, y, bubble_w, bubble_h, SETTINGS_BUTTON_RADIUS);

    if visual.mono {
        draw_lock_glyph(ctx, bx + bubble_w - 14.0, y + BUBBLE_PAD_V + 6.0, 10.0, &tt.item_text_muted);
    }

    let nick_color: &str = if mine { mine_text } else { &tt.item_text_muted };
    let nick_label = if mine { UserSettingsKey::DmYou.get(current_language()) } else { peer_label };
    ctx.set_font("600 10px sans-serif");
    ctx.set_fill_color(nick_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let nick_fit = crate::ui::widgets::fit_text_to_width(ctx, nick_label, bubble_w - 24.0);
    ctx.fill_text(&nick_fit, bx + 10.0, y + BUBBLE_PAD_V);

    let (body_font, body_color): (&str, &str) = if visual.mono {
        ("11px monospace", tt.item_text_muted.as_str())
    } else if mine {
        ("12px sans-serif", mine_text)
    } else {
        ("12px sans-serif", tt.item_text.as_str())
    };
    ctx.set_font(body_font);
    ctx.set_fill_color(body_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let line_x = bx + 10.0;
    let block = msg_idx as i64;
    let hit_start = result.dm_message_line_rects.len();
    for (i, line) in visual.lines.iter().enumerate() {
        let line_y = y + BUBBLE_PAD_V + BUBBLE_NICK_H + i as f64 * BUBBLE_LINE_H;
        ctx.fill_text(line, line_x, line_y);
        result.dm_message_line_rects.push(LineHit {
            block,
            line: i as u16,
            y_top: line_y,
            y_bot: line_y + BUBBLE_LINE_H,
            text: line.clone(),
            text_x: line_x,
            width: ctx.measure_text(line),
            mono: visual.mono,
        });
    }

    if let Some(sel) = selection.filter(|s| !s.is_empty()) {
        let ((lo_block, lo_line, lo_char), (hi_block, hi_line, hi_char)) = sel.ordered();
        ctx.set_font(body_font);
        let highlight_color: &str = if mine { mine_text } else { &tt.selection };
        ctx.set_fill_color(highlight_color);
        ctx.set_global_alpha(0.35);
        for line in &result.dm_message_line_rects[hit_start..] {
            let pos = (line.block, line.line);
            if pos < (lo_block, lo_line) || pos > (hi_block, hi_line) {
                continue;
            }
            let is_first = pos == (lo_block, lo_line);
            let is_last = pos == (hi_block, hi_line);
            let prefix_width = |n: u16| -> f64 {
                let prefix: String = line.text.chars().take(n as usize).collect();
                ctx.measure_text(&prefix)
            };
            let (rx, ry, rw, rh) = highlight_rect(line, lo_char, hi_char, is_first, is_last, prefix_width);
            if rw > 0.0 {
                ctx.fill_rect(rx, ry, rw, rh);
            }
        }
        ctx.set_global_alpha(1.0);
    }

    // Clock + send-state tick — bottom-right inside the bubble, own
    // messages only (a peer's own message has no send-state to show the
    // reader). A double tick for "read" only draws when the provider
    // actually reports read receipts (`Capabilities::receipts`) AND
    // `read_by` is non-empty — a provider that never sets `receipts: true`
    // never gets a double tick even if `read_by` happened to carry
    // something, honouring the flag rather than the raw data alone.
    ctx.set_font("10px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Right);
    ctx.set_text_baseline(TextBaseline::Top);
    let time_y = y + BUBBLE_PAD_V + BUBBLE_NICK_H + line_count as f64 * BUBBLE_LINE_H;
    let clock = clock_hm(item.ts);
    let label = if mine {
        let tick = match &item.send_state {
            SendState::LocalEcho | SendState::Sending => "\u{2022}",
            SendState::Failed { .. } => "!",
            SendState::Sent if caps.receipts && !item.read_by.is_empty() => "\u{2713}\u{2713}",
            SendState::Sent => "\u{2713}",
        };
        format!("{clock} {tick}")
    } else {
        clock
    };
    ctx.fill_text(&label, bx + bubble_w - 8.0, time_y);

    bubble_h + BUBBLE_GAP
}

// =============================================================================
// `Timestamp` (epoch ms) -> calendar-day math — pure integer arithmetic
// (Howard Hinnant's `civil_from_days`, public-domain algorithm), so day
// separators/clock labels work directly off the model's own numeric
// timestamp with no date/timezone crate dependency (this crate has none;
// `dm_overlay.rs`'s own `clock_hm`/`day_separator_label` instead slice an
// rfc3339 STRING, which `Timestamp` does not carry — see this file's own
// top doc, this is genuinely new code, not a duplicate).
// =============================================================================

fn day_key(ts: Timestamp) -> i64 {
    ts.0.div_euclid(86_400_000)
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn day_label(ts: Timestamp, now_ms: u64) -> String {
    let day = day_key(ts);
    let today = day_key(Timestamp(now_ms as i64));
    match today - day {
        0 => UserSettingsKey::DmDayToday.get(current_language()).to_string(),
        1 => UserSettingsKey::DmDayYesterday.get(current_language()).to_string(),
        _ => {
            let (y, m, d) = civil_from_days(day);
            format!("{y:04}-{m:02}-{d:02}")
        }
    }
}

fn clock_hm(ts: Timestamp) -> String {
    let secs = ts.0.div_euclid(1000);
    let hh = secs.div_euclid(3600).rem_euclid(24);
    let mm = secs.div_euclid(60).rem_euclid(60);
    format!("{hh:02}:{mm:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_matches_the_unix_epoch() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn civil_from_days_matches_a_known_later_date() {
        // 2024-03-01T00:00:00Z is unix timestamp 1_709_251_200, i.e. day
        // 19_783 since the epoch (1_709_251_200 / 86_400).
        assert_eq!(civil_from_days(19_783), (2024, 3, 1));
    }

    #[test]
    fn day_key_groups_timestamps_within_the_same_utc_day() {
        let a = Timestamp(1_700_000_000_000);
        let b = Timestamp(a.0 + 3600_000);
        assert_eq!(day_key(a), day_key(b));
        let c = Timestamp(a.0 + 86_400_000);
        assert_ne!(day_key(a), day_key(c));
    }

    #[test]
    fn clock_hm_formats_hours_and_minutes_zero_padded() {
        // 2024-01-01T03:05:00Z.
        assert_eq!(clock_hm(Timestamp(1_704_078_300_000)), "03:05");
    }
}
