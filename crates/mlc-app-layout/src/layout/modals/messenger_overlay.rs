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
//! field or button painting a second time. Widget ids are mostly the
//! `cloud_overlay:dm:*` ids `dm_overlay.rs` already used (community-modal
//! arc W1/W2); this file's own additions, each with a matching `mlc-app`
//! `panel_click.rs`/`input/mod.rs` arm: `cloud_overlay:dm:jump_latest`
//! (UI-2a, the bottom-anchored feed's own "jump to latest" button),
//! `cloud_overlay:dm:chat_filter_input`/`cloud_overlay:dm:
//! new_conversation_toggle` (UI-2b, [`draw_chat_filter_row`]'s own name
//! filter + pencil toggle), plus the right-click context menus UI-2b adds
//! on a message bubble and a chat-list row (`ChartApp::
//! try_messenger_right_click`, `mlc-app`) — those dispatch through
//! `panel_app.context_menu_state`, not a `cloud_overlay:dm:*` widget id at
//! all.

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
use crate::ui::avatar_widget::{draw_avatar, AvatarColors, AvatarShape, CornerMarker};
use crate::ui::count_badge::{badge_width, draw_count_badge, BadgeColors, BadgeContent};
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
    draw_error_line, draw_field, draw_field_shaped, draw_key_changed_notice, draw_lock_glyph,
    draw_lookup_dropdown, draw_open_peer_row,
};

/// The folder rail's own width — messenger concept v1 `.rail{width:70px}`.
const RAIL_W: f64 = 70.0;
/// One rail item's own footprint — concept `.fold{width:62px}`, centred
/// inside [`RAIL_W`] (4px either side, same margin the active indicator's
/// own `left:-4px` walks back out to reach the rail's own left edge).
const RAIL_ITEM_W: f64 = 62.0;
const RAIL_ITEM_ICON_D: f64 = 22.0;
const RAIL_ITEM_PAD_TOP: f64 = 7.0;
const RAIL_ITEM_ICON_LABEL_GAP: f64 = 3.0;
const RAIL_ITEM_LABEL_H: f64 = 13.0;
const RAIL_ITEM_PAD_BOTTOM: f64 = 6.0;
const RAIL_ITEM_H: f64 =
    RAIL_ITEM_PAD_TOP + RAIL_ITEM_ICON_D + RAIL_ITEM_ICON_LABEL_GAP + RAIL_ITEM_LABEL_H + RAIL_ITEM_PAD_BOTTOM;
const RAIL_ITEM_RADIUS: f64 = 6.0;
/// The active indicator bar's own width — concept `.fold.on::before{width:3px}`.
const RAIL_ACTIVE_BAR_W: f64 = 3.0;

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
/// Chat-list row avatar — concept `.av{width:42px;height:42px}`.
const LIST_AVATAR_D: f64 = 42.0;
const LIST_AVATAR_GAP: f64 = 10.0;
const BUBBLE_LINE_H: f64 = 16.0;
const BUBBLE_NICK_H: f64 = 14.0;
const BUBBLE_TIME_H: f64 = 14.0;
const BUBBLE_PAD_V: f64 = 8.0;
const BUBBLE_GAP: f64 = 10.0;
const DAY_SEP_H: f64 = 24.0;
const SYSTEM_NOTICE_H: f64 = 20.0;
const CIPHERTEXT_MAX_LINES: usize = 3;
/// A chat-list row's own corner radius — concept `.row{border-radius:6px}`.
const LIST_ROW_RADIUS: f64 = 6.0;
/// A row's own vertical/horizontal padding — concept `.row{padding:7px 8px}`.
const LIST_ROW_PAD_V: f64 = 7.0;
const LIST_ROW_PAD_H: f64 = 8.0;
/// The row's own painted rect height — the avatar (the tallest child) plus
/// top/bottom padding; [`CONV_ROW_H`] adds [`ROW_GAP`] on top so rows keep
/// the same "painted rect, then a blank strip" idiom every other list in
/// this crate uses (see `draw_room_row`'s own doc for why the rect itself
/// stays `CONV_ROW_H - ROW_GAP` tall).
const CONV_ROW_RECT_H: f64 = LIST_AVATAR_D + LIST_ROW_PAD_V * 2.0;
const CONV_ROW_H: f64 = CONV_ROW_RECT_H + ROW_GAP;
/// The chat-list header row: the name-filter field plus its own pencil
/// button (concept v1's `.lhead`: `[Search][pencil]`) — one fixed row,
/// always drawn, above the (now conditional) "New message" recipient
/// search and the room rows themselves.
const CHAT_FILTER_ROW_H: f64 = SEARCH_FIELD_H + 12.0;
/// The filter field's own height/radius — concept `.search{height:30px;
/// border-radius:15px}` (fully rounded, radius = height / 2). A local,
/// messenger-only height rather than the shared [`FIELD_H`] every other
/// `cloud_overlay` text field uses, so this pane's own pill fields never
/// drag the forum's/DM's plain boxes along with them.
const SEARCH_FIELD_H: f64 = 30.0;
const SEARCH_FIELD_RADIUS: f64 = SEARCH_FIELD_H / 2.0;
const SEARCH_FIELD_H_PADDING: f64 = 14.0;
/// The pencil button's own square footprint, to the filter field's right —
/// concept `.ib{width:30px;height:30px}`.
const CHAT_FILTER_PENCIL_W: f64 = 30.0;
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

/// The rail's own single "All chats" item — the provider carries no folder
/// concept at all (today's DM adapter's `Capabilities::folders == false`,
/// this file's own top doc), so drawing the concept's Personal/Groups/
/// Channels/Trading-desk/Archive/Folders rows here would be inventing rooms
/// this crate cannot actually switch to. "All chats" is the one row every
/// conversation genuinely belongs to — always active (there is nothing else
/// to select), badged with the total unread count across every room.
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

    let unread: i64 = provider.rooms().iter().map(|r| r.notification_count as i64).sum();
    let rect = WidgetRect::new(x + (w - RAIL_ITEM_W) / 2.0, y + 6.0, RAIL_ITEM_W, RAIL_ITEM_H);

    // Active indicator — a bar flush with the RAIL's own left edge (concept
    // `.fold.on::before{left:-4px}` relative to the item, which sits inset
    // from the rail by that same 4px margin).
    ctx.set_fill_color(&tt.accent);
    ctx.fill_rect(x, rect.y + 9.0, RAIL_ACTIVE_BAR_W, (rect.height - 18.0).max(0.0));

    ctx.set_fill_color(&tt.button_bg);
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, RAIL_ITEM_RADIUS);

    let icon_cy = rect.y + RAIL_ITEM_PAD_TOP + RAIL_ITEM_ICON_D / 2.0;
    draw_all_chats_glyph(ctx, rect.center_x(), icon_cy, RAIL_ITEM_ICON_D / 2.0, &tt.item_text);

    ctx.set_font("600 10.5px sans-serif");
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    let label_y = rect.y + RAIL_ITEM_PAD_TOP + RAIL_ITEM_ICON_D + RAIL_ITEM_ICON_LABEL_GAP;
    let fit = crate::ui::widgets::fit_text_to_width(ctx, "All chats", rect.width - 4.0);
    ctx.fill_text(&fit, rect.center_x(), label_y);

    if unread > 0 {
        let label = if unread > 99 { "99+".to_string() } else { unread.to_string() };
        let badge_h = 16.0;
        let bw = badge_width(ctx, &BadgeContent::Count(&label), badge_h);
        let bx = rect.x + rect.width - 9.0 - bw;
        let by = rect.y + 2.0;
        draw_count_badge(
            ctx, bx, by, bw, badge_h, &BadgeContent::Count(&label),
            &BadgeColors { background: &tt.accent, text: contrasting_text_color(&tt.accent) },
        );
    }
}

/// A small filled speech-bubble: a rounded-rect body with a triangular tail
/// at its bottom-left corner — the rail's own "All chats" glyph (concept
/// `#i-all`). Plain vector shapes, same idiom `dm_overlay.rs`'s own
/// `draw_lock_glyph` uses, for the same reason: this crate sits below the
/// `IconId` SVG registry's own home (see that function's own doc).
fn draw_all_chats_glyph(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    let w = r * 1.8;
    let h = r * 1.3;
    let x = cx - w / 2.0;
    let y = cy - h / 2.0;
    ctx.set_fill_color(color);
    ctx.fill_rounded_rect(x, y, w, h, r * 0.35);
    ctx.begin_path();
    ctx.move_to(x + w * 0.22, y + h);
    ctx.line_to(x + w * 0.10, y + h + r * 0.4);
    ctx.line_to(x + w * 0.42, y + h);
    ctx.close_path();
    ctx.fill();
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
    let field_rect = WidgetRect::new(x, y, field_w, SEARCH_FIELD_H);
    let (rect, chars) = draw_field_shaped(
        ctx, coord, layer_id, "cloud_overlay:dm:chat_filter_input", DM_CHAT_FILTER_FIELD, field_rect,
        "Search", ft, tt, current_time_ms, SEARCH_FIELD_RADIUS, SEARCH_FIELD_H_PADDING,
    );
    result.dm_chat_filter_rect = Some(rect);
    result.dm_chat_filter_char_positions = chars;

    let pencil_rect = WidgetRect::new(x + field_w + CHAT_FILTER_GAP, y, pencil_w, SEARCH_FIELD_H);
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

/// Up to two initials (first letter of the first two words), uppercased —
/// [`draw_avatar`]'s own harvest-ready widget takes the initials string
/// pre-computed rather than deriving them itself.
fn initials(name: &str) -> String {
    name.split_whitespace().filter_map(|w| w.chars().next()).take(2).flat_map(|c| c.to_uppercase()).collect()
}

/// One chat-list row — concept v1 `.row`: a 42px avatar, then two text
/// lines (name + time; preview + unread pill). Padding 7×8, radius 6.
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
    let rect = WidgetRect::new(x, y, w, CONV_ROW_RECT_H);
    let hovered = coord.is_hovered(&WidgetId::from(widget_id.as_str()));
    let open = matches!(&state.dm.view, crate::ui::modal_settings::DmView::Conversation { id, .. } if id.to_string() == r.id.as_str());
    if open {
        ctx.set_fill_color(&tt.button_bg);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, LIST_ROW_RADIUS);
    } else if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, LIST_ROW_RADIUS);
    }

    let avatar_cx = x + LIST_ROW_PAD_H + LIST_AVATAR_D / 2.0;
    let avatar_cy = rect.y + rect.height / 2.0;
    draw_avatar(
        ctx, avatar_cx, avatar_cy, LIST_AVATAR_D / 2.0, AvatarShape::Circle, &initials(&r.name),
        &AvatarColors { background: &tt.item_bg_hover, text: contrasting_text_color(&tt.item_bg_hover), ring: &tt.background },
        CornerMarker::None,
    );
    let text_x = x + LIST_ROW_PAD_H + LIST_AVATAR_D + LIST_AVATAR_GAP;
    let text_right = x + w - LIST_ROW_PAD_H;

    // Line 1 — name (left) + time (mono, right-aligned, same line).
    let time_label = r.last_event.as_ref().map(|ev| row_time_label(ev.ts, current_time_ms));
    let time_w = time_label.as_ref().map(|t| {
        ctx.set_font("11px monospace");
        ctx.measure_text(t)
    });
    let line1_y = rect.y + (rect.height - 29.0) / 2.0;
    let name_right = time_w.map(|tw| text_right - tw - 6.0).unwrap_or(text_right);
    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color(if hovered || open { &tt.item_text_hover } else { &tt.item_text });
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let label = crate::ui::widgets::fit_text_to_width(ctx, &r.name, (name_right - text_x).max(0.0));
    ctx.fill_text(&label, text_x, line1_y);
    if let Some(t) = &time_label {
        ctx.set_font("11px monospace");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Right);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(t, text_right, line1_y);
    }

    // Line 2 — preview (left, ellipsised) + unread pill (right-aligned).
    let line2_y = line1_y + 15.0;
    let badge_h = 16.0;
    let mut preview_right = text_right;
    if r.notification_count > 0 {
        let badge_label = if r.notification_count > 99 { "99+".to_string() } else { r.notification_count.to_string() };
        let bw = badge_width(ctx, &BadgeContent::Count(&badge_label), badge_h);
        let bx = text_right - bw;
        let by = line2_y - 1.0;
        draw_count_badge(
            ctx, bx, by, bw, badge_h, &BadgeContent::Count(&badge_label),
            &BadgeColors { background: &tt.accent, text: contrasting_text_color(&tt.accent) },
        );
        preview_right = bx - 6.0;
    }
    // Preview line — only when the provider actually has one
    // (`RoomSummary::last_event`, honestly `None` for today's DM adapter;
    // see this file's own top doc). Nothing fabricated when it is absent.
    if let Some(ev) = &r.last_event {
        let (label, encrypted) = match &ev.content {
            mlc_messenger_model::PreviewContent::Text(t) => (t.clone(), false),
            mlc_messenger_model::PreviewContent::Encrypted => ("Encrypted message".to_string(), true),
            mlc_messenger_model::PreviewContent::Redacted => ("Message deleted".to_string(), false),
            mlc_messenger_model::PreviewContent::Notice(n) => (n.clone(), false),
        };
        ctx.set_font(if encrypted { "italic 12.5px sans-serif" } else { "12.5px sans-serif" });
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        let fit = crate::ui::widgets::fit_text_to_width(ctx, &label, (preview_right - text_x).max(0.0));
        ctx.fill_text(&fit, text_x, line2_y);
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

    #[test]
    fn rail_item_is_centred_inside_the_rail_with_a_4px_margin_either_side() {
        let margin = (RAIL_W - RAIL_ITEM_W) / 2.0;
        assert_eq!(margin, 4.0);
    }

    #[test]
    fn rail_active_bar_sits_flush_with_the_rails_own_left_edge() {
        // The item itself starts 4px in from the rail's left edge; the
        // active bar is drawn at the RAIL's own x (see `draw_folder_rail`),
        // i.e. exactly `margin` to the left of the item — same offset the
        // concept's own `left:-4px` (relative to the item) describes.
        let margin = (RAIL_W - RAIL_ITEM_W) / 2.0;
        let rail_x = 0.0;
        let item_x = rail_x + margin;
        let bar_x = rail_x;
        assert_eq!(item_x - bar_x, margin);
    }

    #[test]
    fn conv_row_rect_height_fits_the_42px_avatar_plus_7px_padding_top_and_bottom() {
        assert_eq!(CONV_ROW_RECT_H, LIST_AVATAR_D + 14.0);
        assert_eq!(CONV_ROW_RECT_H, 56.0);
    }

    #[test]
    fn conv_row_h_reserves_the_shared_row_gap_after_the_painted_rect() {
        assert_eq!(CONV_ROW_H - CONV_ROW_RECT_H, ROW_GAP);
    }

    #[test]
    fn search_field_is_fully_rounded() {
        // A fully-rounded pill's own radius is exactly half its height.
        assert_eq!(SEARCH_FIELD_RADIUS, SEARCH_FIELD_H / 2.0);
    }

    #[test]
    fn initials_takes_up_to_two_words_uppercased() {
        assert_eq!(initials("zeng3ld"), "Z");
        assert_eq!(initials("Trading desk"), "TD");
        assert_eq!(initials("kvant_42 extra words"), "KE");
        assert_eq!(initials(""), "");
    }
}
