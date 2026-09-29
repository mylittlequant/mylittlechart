//! Messenger UI-2a/2b — the Community modal's Messages tab body, drawn ONLY
//! from [`mlc_messenger_model::MessengerProvider`] (a `&dyn` trait object
//! the CALLER builds — `community.rs`'s real call site wraps `state.dm` in
//! `DmMessengerProviderView`, `mlc-ui-lab`'s own fixture wraps a hand-built
//! room/timeline set — see [`draw`]'s own doc) instead of reading `DmState`
//! fields ad hoc the way `dm_overlay.rs` still does. `Capabilities` gates
//! what draws: today's DM adapter reports every flag false except
//! `receipts`, so a DM-backed provider never exercises the group/channel
//! paths below — not because this file forgot them, but because that
//! provider's own rooms are all `RoomKind::Direct`.
//!
//! Two things the model genuinely cannot carry are read straight off
//! `state.dm`/`state.forum2` instead, each documented at its own read site
//! rather than silently papered over:
//! - the peer's short key FINGERPRINT STRING (`DmConversationUi::
//!   peer_fingerprint`) — [`Member::has_messaging_key`] says WHETHER a key
//!   exists, but the model carries no fingerprint text field at all; this is
//!   read only for `RoomKind::Direct` rooms, the only kind that has one;
//! - "which room is open" and the nick-required/nick-search flow
//!   (`DmState::view`, `ForumV2State::nick_required`, the lookup dropdown)
//!   — [`MessengerProvider`] has no navigation or account-ladder concept;
//!   picking a room and choosing a nick are UI-side facts, not provider
//!   reads, and searching for someone to message is a DM-specific server
//!   endpoint with no [`MessengerIntent`] equivalent yet.
//!
//! Reuses `dm_overlay.rs`'s own row/field/notice painters (widened to
//! `pub(crate)` for this file) rather than duplicating them — this file's
//! own job is the three-column shape (folder rail / chat list / chat pane),
//! group/channel-aware chrome, and reading through the provider trait, not
//! re-deriving text-field or button painting a second time.
//!
//! UI-2b (2026-09-25, messenger concept v1 screen "1 · Группа" parity):
//! folder rail now draws every built-in (All chats/Personal/Groups/
//! Channels, computed client-side from `RoomKind`/`archived` — folder.rs's
//! own doc) plus every CUSTOM folder the provider reports, with real
//! `Folder::matches`-derived counts, and Archive/Folders pinned to the
//! rail's own bottom. No folder besides "All chats" is ever selectable yet
//! — switching the active folder needs UI-side selection state this pass
//! does not add (same gap the previous pass already named for the rail).
//! Chat-list rows are now KIND-aware (circle/rounded-square avatar, a
//! presence dot or a kind-corner-badge, a ladder-rank chip for a Direct
//! peer, a mute glyph, and a single trailing marker chosen by priority
//! mention > unread-count > pinned — never more than one at once, matching
//! the concept's own row anatomy) instead of the DM-only row the previous
//! pass drew. The chat pane grows group/channel chrome too: a typing/
//! member-count subtitle instead of a fingerprint line, a pinned-message
//! bar, message GROUPING (consecutive same-sender bubbles share one avatar
//! and one name/rank header), a shared-chart-card attachment, a reply
//! quote, `@mention` token colouring, a reactions row, an "edited" badge,
//! an unread separator derived from `RoomSummary::notification_count`, and
//! a round jump-to-latest button with its own unread badge.
use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::i18n::{current_language, UserSettingsKey};
use mlc_messenger_model::{
    Folder, FolderIcon, MembershipChange, MembershipEvent, MessageContent, MessengerProvider, PreviewContent,
    RankBadge, ReactionSummary, ReplyPreview, RoomId, RoomKind, RoomSummary, SendState, SharedChartCard,
    Timestamp, UtdReason, VaultState,
};
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::layout::render_chart::FrameTheme;
use crate::ui::avatar_widget::{draw_avatar, AvatarColors, AvatarShape, CornerMarker};
use crate::ui::count_badge::{badge_width, draw_count_badge, BadgeColors, BadgeContent};
use crate::ui::modal_settings::{CommunityState, MessengerFolderSel};
use crate::ui::scroll_widget::{jump_button_visible, ScrollableContainer};
use crate::ui::selectable_text::{highlight_rect, LineHit, Selection};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::avatar::contrasting_text_color;
use crate::ui::widgets::{draw_avatar_placeholder, SETTINGS_BUTTON_RADIUS};

use super::cloud_overlay::{
    draw_action_row, draw_expand_row, draw_status, register_scrollbar_hit_zones, scroll_config,
    wrap_text, CloudOverlayResult, DM_CHAT_FILTER_FIELD, FIELD_H, LIST_COL_W, ROW_GAP, ROW_H,
};
use super::community::CommunityListPaneResult;
use super::dm_overlay::{
    draw_error_line, draw_field_shaped, draw_key_changed_notice, draw_lock_glyph, draw_lookup_dropdown,
    draw_open_peer_row,
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
/// Gap between consecutive rail rows — concept `.rail{gap:2px}`.
const RAIL_ROW_GAP: f64 = 2.0;
/// Bottom inset for the last pinned rail row (Archive/Folders) — concept
/// `.rail{padding:6px 0}` (same value as the rail's own top inset).
const RAIL_BOTTOM_PAD: f64 = 6.0;

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
const BUBBLE_TIME_H: f64 = 14.0;
/// A bubble's own inner padding — concept `.b{padding:6px 10px 5px}`.
const BUBBLE_PAD_V: f64 = 6.0;
const BUBBLE_PAD_H: f64 = 10.0;
/// A bubble's own corner radius, and the tighter radius its one "tail"
/// corner gets — concept `.b{border-radius:12px}` plus `.b.own`/`.b.peer`'s
/// own 4px corner (see [`bubble_corners_grouped`]).
const BUBBLE_RADIUS: f64 = 12.0;
const BUBBLE_TAIL_RADIUS: f64 = 4.0;
/// Gap BETWEEN two message groups (or a group and a day/unread separator) —
/// concept `.grp{margin-top:6px}`.
const BUBBLE_GAP: f64 = 6.0;
/// Gap between two bubbles WITHIN the same group — concept `.grp .col2{gap:2px}`.
const GROUP_INNER_GAP: f64 = 2.0;
const DAY_SEP_H: f64 = 24.0;
const UNREAD_SEP_H: f64 = 24.0;
const SYSTEM_NOTICE_H: f64 = 20.0;
const CIPHERTEXT_MAX_LINES: usize = 3;
/// A peer/own group's own name+rank header row — concept `.b .nm{font:600
/// 12px;margin-bottom:1px}`.
const NAME_HEADER_H: f64 = 16.0;
/// A reply-quote block's own footprint (sender line + one preview line) —
/// concept `.quote{padding:2px 8px;margin:1px 0 4px}`.
const QUOTE_H: f64 = 34.0;
const QUOTE_PAD_H: f64 = 8.0;
const QUOTE_PAD_V: f64 = 2.0;
const QUOTE_BAR_W: f64 = 2.0;
/// The reactions row's own footprint, added to a bubble's height only when
/// it carries at least one reaction — concept `.rx{margin-top:4px}`.
const REACTIONS_ROW_H: f64 = 22.0;
/// A shared-chart-card attachment's own footprint — concept `.card{margin:
/// 4px 0 2px;width:330px}` (chart 74px + a 1px divider + a 40px info row).
const CARD_W: f64 = 330.0;
const CARD_CHART_H: f64 = 74.0;
const CARD_INFO_H: f64 = 40.0;
const CARD_MARGIN_TOP: f64 = 4.0;
const CARD_MARGIN_BOTTOM: f64 = 2.0;
const CARD_RADIUS: f64 = 8.0;
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
/// always drawn, above the room rows themselves.
const CHAT_FILTER_ROW_H: f64 = SEARCH_FIELD_H + 12.0;
/// The filter field's own height/radius — concept `.search{height:30px;
/// border-radius:15px}` (fully rounded, radius = height / 2). A local,
/// messenger-only height rather than the shared [`FIELD_H`] every other
/// `cloud_overlay` text field uses, so this pane's own pill fields never
/// drag the forum's/DM's plain boxes along with them.
const SEARCH_FIELD_H: f64 = 30.0;
const SEARCH_FIELD_RADIUS: f64 = SEARCH_FIELD_H / 2.0;
/// Left text inset — wide enough to clear the magnifier glyph
/// [`draw_chat_filter_row`] draws first (concept `.search{padding:0 11px;
/// gap:7px}`: an icon, a 7px gap, THEN the text — not a plain left pad).
const SEARCH_FIELD_H_PADDING: f64 = 32.0;
/// The pencil button's own square footprint, to the filter field's right —
/// concept `.ib{width:30px;height:30px}`.
const CHAT_FILTER_PENCIL_W: f64 = 30.0;
const CHAT_FILTER_GAP: f64 = 8.0;
/// A chat header's own icon button (search/members/more) — concept `.ib`.
const HEADER_ICON_D: f64 = 30.0;
const HEADER_ICON_GAP: f64 = 2.0;
/// The docked composer's own ONE-row footprint (concept `.comp`/`.cin`:
/// a 36px draft field + round send button side by side, `padding:8px 10px
/// 9px`) — every non-nick-gated dock state (locked, peer-without-key,
/// normal) fits inside this same footprint, so the pane's scroll math never
/// has to change between them. The nick-required gate is the one exception
/// ([`NICK_REQUIRED_DOCK_H`], [`dock_h`]) — a ladder-mandate hint+button
/// pair unrelated to messaging itself, unchanged from before this pass.
const COMPOSER_PAD_TOP: f64 = 8.0;
const COMPOSER_PAD_BOTTOM: f64 = 9.0;
/// The draft field's own height — concept `.field{min-height:36px;
/// border-radius:18px}` (fully rounded).
const COMPOSER_FIELD_H: f64 = 36.0;
const COMPOSER_FIELD_RADIUS: f64 = COMPOSER_FIELD_H / 2.0;
const COMPOSER_GAP: f64 = 8.0;
/// The round accent send button's own diameter — concept `.send{width:36px;
/// height:36px;border-radius:50%}`.
const COMPOSER_SEND_D: f64 = 36.0;
const COMPOSER_H: f64 = COMPOSER_PAD_TOP + COMPOSER_FIELD_H + COMPOSER_PAD_BOTTOM;
/// The composer's own reply-preview banner — concept `.cbar`.
const REPLY_BANNER_H: f64 = 40.0;
/// A dock notice row's own compact button — concept `.btn{padding:8px
/// 16px;border-radius:4px}`, sized to its own text rather than the pane's
/// full width (the old dock stretched "Unlock vault" across the whole
/// composer; the concept never does).
const DOCK_BTN_PAD_V: f64 = 8.0;
const DOCK_BTN_PAD_H: f64 = 16.0;
const DOCK_BTN_TEXT_H: f64 = 13.0;
const DOCK_BTN_H: f64 = DOCK_BTN_TEXT_H + DOCK_BTN_PAD_V * 2.0;
/// The nick-required gate's own footprint — a hint line then a full-width
/// button, the SAME two-row shape this dock always used for it (a ladder
/// mandate, not a messaging-specific notice — see [`COMPOSER_H`]'s own doc
/// for why every other dock state got a new, single-row footprint this pass
/// and this one did not).
const NICK_REQUIRED_DOCK_H: f64 = 8.0 + FIELD_H + 8.0 + ROW_H;
/// The pin bar's own footprint — concept `.pinbar{height:40px}`.
const PINBAR_H: f64 = 40.0;
/// The round jump-to-latest button's own diameter — concept `.jump{width:
/// 38px;height:38px;border-radius:50%}`.
const JUMP_D: f64 = 38.0;
/// The composer's own attach/emoji icon buttons — concept `.ib{width:30px;
/// height:30px}` either side of the draft field (`.cin`'s own clip/smile
/// icons). Inert this pass (registered for hover only, same convention
/// [`draw_header_icons`]'s own doc already uses) — attach/emoji pickers are
/// their own slice (U3, sticker/emoji picker).
const COMPOSER_ICON_D: f64 = 30.0;

/// A `$SYMBOL` cashtag pill inline in message text (concept `.sym`) —
/// `padding:0 5px;border-radius:4px;font:12px mono`.
const SYMBOL_PILL_FONT: &str = "12px monospace";
const SYMBOL_PILL_PAD_H: f64 = 5.0;
const SYMBOL_PILL_RADIUS: f64 = 4.0;

/// The hovered-bubble quick-action bar (concept `.qbar`): four 26×24
/// buttons (Reply/React/Forward/More), floating at the bubble's own
/// top-right corner.
const QBAR_BTN_W: f64 = 26.0;
const QBAR_BTN_H: f64 = 24.0;
const QBAR_PAD: f64 = 2.0;
const QBAR_GAP: f64 = 1.0;
/// concept `.qbar{right:-6px;top:-14px}` — offsets past the bubble's own
/// top-right corner.
const QBAR_OFFSET_RIGHT: f64 = -6.0;
const QBAR_OFFSET_TOP: f64 = -14.0;

/// A Channel room's own composer gate — `None` for every non-channel room
/// (Direct/Group keep the normal locked/no-key/nick-gated dock below).
/// `Some` for a Channel room: `can_post == true` (an admin) still falls
/// through to the normal composer; `can_post == false` (a subscriber)
/// replaces the WHOLE dock with the concept's own wide Mute/Unmute row
/// (concept: "Подписчик не пишет: вместо поля — Mute/Unmute"). "Join
/// channel" (a channel not yet in this account's own room list) has no
/// reachable case here — [`MessengerProvider::rooms`] only ever returns
/// rooms this account already holds.
#[derive(Clone, Copy)]
struct ChannelGate {
    can_post: bool,
    muted: bool,
}

/// The wide, borderless Mute/Unmute row a Channel subscriber's own dock
/// shows in place of a draft field — concept `.cta{min-height:38px}`
/// `.btn.wide`.
const CHANNEL_CTA_H: f64 = 38.0;

/// The docked composer's own footprint for the CURRENT dock state — see
/// [`COMPOSER_H`]'s own doc. Read by both `draw_chat_pane` (to size the
/// feed above the dock) and `draw_composer` (to anchor the dock's own top),
/// so the two can never disagree about where the dock starts.
fn dock_h(state: &CommunityState, channel_gate: Option<ChannelGate>) -> f64 {
    if let Some(gate) = channel_gate {
        if !gate.can_post {
            return CHANNEL_CTA_H;
        }
    }
    if state.forum2.nick_required() {
        NICK_REQUIRED_DOCK_H
    } else if state.messenger_reply_preview.is_some() {
        COMPOSER_H + REPLY_BANNER_H
    } else {
        COMPOSER_H
    }
}
/// The chat pane's own fixed header — concept `.ch{height:52px}`, drawn
/// once above the scrollable feed rather than scrolling away with it (the
/// concept's `.chat` is a column of fixed-size siblings — header, an
/// optional pin bar, a scrolling `.feed`, and the docked composer — not one
/// long scrollable block with the header as its first row).
const CHAT_HEADER_H: f64 = 52.0;
const CHAT_HEADER_PAD_L: f64 = 14.0;

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

/// Draw the Messages tab's whole body — folder rail, chat list, chat pane —
/// against `provider`. The CALLER decides which provider backs this frame:
/// `community.rs`'s real call site wraps `state.dm` in a fresh
/// `DmMessengerProviderView` every frame (that adapter's own doc explains
/// why a read-only view, not `&mut`, is correct for a paint pass); a lab/
/// test fixture passes its own `MessengerProvider` impl straight through —
/// this file draws identically either way, by construction.
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
    provider: &dyn MessengerProvider,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
    list_result: &mut CommunityListPaneResult,
) {
    let caps = provider.capabilities();

    let list_x = x + RAIL_W;
    let list_w = LIST_COL_W;
    let pane_x = list_x + list_w + COL_GAP;
    let pane_w = (w - RAIL_W - list_w - COL_GAP).max(0.0);

    draw_folder_rail(ctx, coord, layer_id, x, y, RAIL_W, viewport_h, provider, &state.messenger_folder, tt);
    draw_soft_divider_v(ctx, x + RAIL_W, y, viewport_h, tt);
    draw_soft_divider_v(ctx, pane_x - COL_GAP / 2.0, y, viewport_h, tt);

    draw_chat_list(
        ctx, coord, layer_id, list_x, y, list_w, viewport_h, state, provider, ft, tt, current_time_ms,
        result, list_result,
    );

    draw_chat_pane(
        ctx, coord, layer_id, pane_x, y, pane_w, viewport_h, state, provider, caps, ft, tt,
        current_time_ms, result,
    );
}

/// Column gap between the chat list and the chat pane — same numeric value
/// `cloud_overlay::COL_GAP` uses (kept file-local so this file no longer
/// needs that shared constant for anything else).
const COL_GAP: f64 = 16.0;

/// A vertical 1px rule in the concept's own SOFT shade (`--soft`,
/// [`ToolbarTheme::divider_soft`]) — every internal divider in this pane
/// (rail/list/pane column rules, the chat header's own bottom rule, the pin
/// bar, the composer's top rule) uses this shade, never the stronger
/// [`ToolbarTheme::separator`] the outer modal frame uses.
fn draw_soft_divider_v(ctx: &mut dyn RenderContext, x: f64, y: f64, h: f64, tt: &ToolbarTheme) {
    ctx.set_stroke_color(&tt.divider_soft);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(x, y);
    ctx.line_to(x, y + h);
    ctx.stroke();
}

fn draw_soft_divider_h(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, tt: &ToolbarTheme) {
    ctx.set_stroke_color(&tt.divider_soft);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(x, y);
    ctx.line_to(x + w, y);
    ctx.stroke();
}

// =============================================================================
// Left rail — every built-in folder (All chats/Personal/Groups/Channels,
// computed client-side from `RoomKind` — folder.rs's own doc) plus every
// CUSTOM folder the provider reports (`id: Some(_)`), with real
// `Folder::matches`-derived counts; Archive and Folders (a settings action,
// not a real folder) are pinned to the rail's own bottom. The active folder
// is UI-side state (`CommunityState::messenger_folder`). The built-in Archive
// folder carries no include/exclude rule that could express "archived only"
// (`Folder::matches` cannot), so [`folder_includes`] filters it on
// `RoomSummary::archived` directly and keeps archived rooms out of EVERY
// other folder the same way.
// =============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RailIcon {
    All,
    Person,
    Group,
    Channel,
    Candles,
    Bookmark,
    Folder,
    Archive,
    Edit,
}

fn rail_icon_for(icon: FolderIcon) -> RailIcon {
    match icon {
        FolderIcon::Candles => RailIcon::Candles,
        FolderIcon::Folder => RailIcon::Folder,
        FolderIcon::Group => RailIcon::Group,
        FolderIcon::Channel => RailIcon::Channel,
        FolderIcon::Bookmark => RailIcon::Bookmark,
        FolderIcon::Person => RailIcon::Person,
    }
}

struct RailRow {
    icon: RailIcon,
    /// Widget-id suffix — [`MessengerFolderSel::widget_key`] for a selectable
    /// folder, `"folders"` for the settings row.
    key: String,
    label: String,
    /// `None` draws no badge at all (Archive/Folders never show a count,
    /// same as the concept).
    count: Option<String>,
    /// Muted/grey badge fill (concept `.bd.m`, the Channels folder only) —
    /// every other folder's badge is accent-filled (`.bd`).
    muted_badge: bool,
    active: bool,
}

/// Whether `room` is listed under the `sel` folder. Archived rooms are listed
/// ONLY under [`MessengerFolderSel::Archive`]; the built-in kind folders and
/// every custom folder skip them even when their own rules would match.
fn folder_includes(sel: &MessengerFolderSel, folders: &[Folder], room: &RoomSummary) -> bool {
    match sel {
        MessengerFolderSel::Archive => room.archived,
        _ if room.archived => false,
        MessengerFolderSel::All => true,
        MessengerFolderSel::Personal => room.kind == RoomKind::Direct,
        MessengerFolderSel::Groups => room.kind == RoomKind::Group,
        MessengerFolderSel::Channels => room.kind == RoomKind::Channel,
        MessengerFolderSel::Custom(tag) => folders
            .iter()
            .any(|f| f.id.as_ref().is_some_and(|id| id.as_str() == tag.as_str()) && f.matches(room)),
    }
}

/// Every rail row, split into the top (scrollable-in-spirit, but never
/// actually taller than the rail today) group and the bottom-pinned group —
/// see this section's own top doc.
fn rail_rows(provider: &dyn MessengerProvider, active: &MessengerFolderSel) -> (Vec<RailRow>, Vec<RailRow>) {
    let rooms = provider.rooms();
    let folders = provider.folders();
    let count = |sel: &MessengerFolderSel| rooms.iter().filter(|r| folder_includes(sel, &folders, r)).count();
    let row = |sel: MessengerFolderSel, icon: RailIcon, label: &str, muted_badge: bool| RailRow {
        icon,
        key: sel.widget_key(),
        label: label.to_string(),
        count: Some(count(&sel).to_string()),
        muted_badge,
        active: &sel == active,
    };
    let mut top = vec![
        row(MessengerFolderSel::All, RailIcon::All, "All chats", false),
        row(MessengerFolderSel::Personal, RailIcon::Person, "Personal", false),
        row(MessengerFolderSel::Groups, RailIcon::Group, "Groups", false),
        row(MessengerFolderSel::Channels, RailIcon::Channel, "Channels", true),
    ];
    for folder in &folders {
        // Built-ins are computed above, never round-tripped through this
        // method (folder.rs's own top doc) — only a genuinely custom folder
        // (its own tag identity) adds a row here.
        let Some(tag) = &folder.id else { continue };
        top.push(row(MessengerFolderSel::Custom(tag.as_str().to_string()), rail_icon_for(folder.icon), &folder.name, false));
    }
    let bottom = vec![
        RailRow {
            icon: RailIcon::Archive,
            key: MessengerFolderSel::Archive.widget_key(),
            label: "Archive".to_string(),
            count: None,
            muted_badge: false,
            active: *active == MessengerFolderSel::Archive,
        },
        RailRow {
            icon: RailIcon::Edit,
            key: "folders".to_string(),
            label: "Folders".to_string(),
            count: None,
            muted_badge: false,
            active: false,
        },
    ];
    (top, bottom)
}

fn draw_folder_rail(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    provider: &dyn MessengerProvider,
    active: &MessengerFolderSel,
    tt: &ToolbarTheme,
) {
    ctx.set_fill_color(&tt.dropdown_bg);
    ctx.fill_rect(x, y, w, h);

    let (top, bottom) = rail_rows(provider, active);

    let mut ry = y + RAIL_BOTTOM_PAD;
    for row in &top {
        draw_rail_row(ctx, coord, layer_id, x, ry, w, row, tt);
        ry += RAIL_ITEM_H + RAIL_ROW_GAP;
    }
    let bottom_h = bottom.len() as f64 * RAIL_ITEM_H + (bottom.len().saturating_sub(1)) as f64 * RAIL_ROW_GAP;
    let mut by = (y + h - RAIL_BOTTOM_PAD - bottom_h).max(ry);
    for row in &bottom {
        draw_rail_row(ctx, coord, layer_id, x, by, w, row, tt);
        by += RAIL_ITEM_H + RAIL_ROW_GAP;
    }
}

fn draw_rail_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    row: &RailRow,
    tt: &ToolbarTheme,
) {
    let rect = WidgetRect::new(x + (w - RAIL_ITEM_W) / 2.0, y, RAIL_ITEM_W, RAIL_ITEM_H);
    let id = format!("cloud_overlay:dm:folder:{}", row.key);
    let hovered = coord.is_hovered(&WidgetId::from(id.as_str()));

    if row.active {
        ctx.set_fill_color(&tt.accent);
        ctx.fill_rect(x, rect.y + 9.0, RAIL_ACTIVE_BAR_W, (rect.height - 18.0).max(0.0));
        ctx.set_fill_color(&tt.button_bg);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, RAIL_ITEM_RADIUS);
    } else if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, RAIL_ITEM_RADIUS);
    }

    let icon_color: &str = if row.active { tt.item_text.as_str() } else { tt.muted_ink.as_str() };
    let icon_cy = rect.y + RAIL_ITEM_PAD_TOP + RAIL_ITEM_ICON_D / 2.0;
    draw_rail_icon(ctx, row.icon, rect.center_x(), icon_cy, RAIL_ITEM_ICON_D / 2.0, icon_color);

    ctx.set_font("600 10.5px sans-serif");
    ctx.set_fill_color(if row.active { tt.item_text.as_str() } else { tt.muted_ink.as_str() });
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    let label_y = rect.y + RAIL_ITEM_PAD_TOP + RAIL_ITEM_ICON_D + RAIL_ITEM_ICON_LABEL_GAP;
    let fit = crate::ui::widgets::fit_text_to_width(ctx, &row.label, rect.width - 4.0);
    ctx.fill_text(&fit, rect.center_x(), label_y);

    if let Some(count) = &row.count {
        let badge_h = 16.0;
        let bw = badge_width(ctx, &BadgeContent::Count(count), badge_h);
        let bx = rect.x + rect.width - 9.0 - bw;
        let by = rect.y + 2.0;
        let (bg, fg): (&str, &str) = if row.muted_badge {
            (tt.muted_ink.as_str(), tt.background.as_str())
        } else {
            (tt.accent.as_str(), contrasting_text_color(&tt.accent))
        };
        draw_count_badge(ctx, bx, by, bw, badge_h, &BadgeContent::Count(count), &BadgeColors { background: bg, text: fg });
    }

    coord.register_on_layer(id.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
}

fn draw_rail_icon(ctx: &mut dyn RenderContext, icon: RailIcon, cx: f64, cy: f64, r: f64, color: &str) {
    match icon {
        RailIcon::All => draw_icon_all_chats(ctx, cx, cy, r, color),
        RailIcon::Person => draw_icon_person(ctx, cx, cy, r, color),
        RailIcon::Group => draw_icon_group(ctx, cx, cy, r, color),
        RailIcon::Channel => draw_icon_channel(ctx, cx, cy, r, color),
        RailIcon::Candles => draw_icon_candles(ctx, cx, cy, r, color),
        RailIcon::Bookmark => draw_icon_bookmark(ctx, cx, cy, r, color),
        RailIcon::Folder => draw_icon_folder(ctx, cx, cy, r, color),
        RailIcon::Archive => draw_icon_archive(ctx, cx, cy, r, color),
        RailIcon::Edit => draw_icon_sliders(ctx, cx, cy, r, color),
    }
}

// =============================================================================
// Small vector glyphs — plain shapes (rects/circles/paths), the same idiom
// every other icon in this crate below the `IconId` SVG registry's own home
// uses (see `dm_overlay.rs`'s `draw_lock_glyph`'s own doc). Schematic, not
// pixel copies of the concept's own SVG paths — recognisable at 22px, which
// is all a rail/header/avatar glyph needs to be.
// =============================================================================

const TAU: f64 = std::f64::consts::TAU;

/// A small filled speech-bubble: a rounded-rect body with a triangular tail
/// at its bottom-left corner — the rail's own "All chats" glyph (concept
/// `#i-all`).
fn draw_icon_all_chats(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
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

fn draw_icon_person(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_fill_color(color);
    ctx.begin_path();
    ctx.arc(cx, cy - r * 0.35, r * 0.32, 0.0, TAU);
    ctx.fill();
    let sw = r * 0.85;
    let sh = r * 0.6;
    let sy = cy + r * 0.1;
    ctx.begin_path();
    ctx.move_to(cx - sw, sy + sh);
    ctx.line_to(cx - sw, sy + sh * 0.3);
    ctx.quadratic_curve_to(cx - sw, sy, cx, sy);
    ctx.quadratic_curve_to(cx + sw, sy, cx + sw, sy + sh * 0.3);
    ctx.line_to(cx + sw, sy + sh);
    ctx.close_path();
    ctx.fill();
}

fn draw_icon_group(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_global_alpha(0.55);
    draw_icon_person(ctx, cx - r * 0.32, cy + r * 0.08, r * 0.8, color);
    ctx.set_global_alpha(1.0);
    draw_icon_person(ctx, cx + r * 0.2, cy - r * 0.05, r * 0.8, color);
}

/// A schematic "broadcast" glyph — a flag/pennant shape plus a small
/// quarter-arc "wave" to its right (concept `#i-channel`'s own megaphone).
fn draw_icon_channel(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_fill_color(color);
    ctx.begin_path();
    ctx.move_to(cx - r * 0.55, cy - r * 0.4);
    ctx.line_to(cx - r * 0.55, cy + r * 0.4);
    ctx.line_to(cx + r * 0.2, cy + r * 0.72);
    ctx.line_to(cx + r * 0.2, cy - r * 0.72);
    ctx.close_path();
    ctx.fill();
    ctx.set_stroke_color(color);
    ctx.set_stroke_width(1.3);
    ctx.set_line_cap("round");
    ctx.begin_path();
    ctx.arc(cx + r * 0.2, cy, r * 0.55, -std::f64::consts::FRAC_PI_3, std::f64::consts::FRAC_PI_3);
    ctx.stroke();
}

/// Three candlestick bars of different heights — the "Trading desk" custom
/// folder's own icon (concept `#i-candles`).
fn draw_icon_candles(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_stroke_color(color);
    ctx.set_stroke_width(1.2);
    ctx.set_fill_color(color);
    let bars: [(f64, f64, f64); 3] = [(-0.55, 0.5, 0.9), (0.0, 0.8, 0.5), (0.55, 0.35, 0.75)];
    for (xo, bh, wk) in bars {
        let bx = cx + xo * r;
        let bw = (r * 0.32).max(1.5);
        let body_h = r * bh;
        ctx.begin_path();
        ctx.move_to(bx, cy - body_h / 2.0 - r * wk * 0.35);
        ctx.line_to(bx, cy + body_h / 2.0 + r * wk * 0.35);
        ctx.stroke();
        ctx.fill_rect(bx - bw / 2.0, cy - body_h / 2.0, bw, body_h);
    }
}

/// A box with a lid and a short handle line — Archive (concept `#i-archive`).
fn draw_icon_archive(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_fill_color(color);
    let w = r * 1.6;
    ctx.fill_rounded_rect(cx - w / 2.0, cy - r * 0.85, w, r * 0.45, r * 0.15);
    ctx.set_stroke_color(color);
    ctx.set_stroke_width(1.3);
    ctx.begin_path();
    ctx.rect(cx - w * 0.42, cy - r * 0.2, w * 0.84, r * 0.95);
    ctx.stroke();
    ctx.begin_path();
    ctx.move_to(cx - r * 0.25, cy + r * 0.25);
    ctx.line_to(cx + r * 0.25, cy + r * 0.25);
    ctx.stroke();
}

/// Two horizontal slider rows with small handle dots — the "Folders"
/// settings action (concept `#i-edit`).
fn draw_icon_sliders(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_stroke_color(color);
    ctx.set_stroke_width(1.3);
    ctx.begin_path();
    ctx.move_to(cx - r * 0.7, cy - r * 0.35);
    ctx.line_to(cx + r * 0.7, cy - r * 0.35);
    ctx.stroke();
    ctx.begin_path();
    ctx.move_to(cx - r * 0.7, cy + r * 0.35);
    ctx.line_to(cx + r * 0.7, cy + r * 0.35);
    ctx.stroke();
    ctx.set_fill_color(color);
    ctx.begin_path();
    ctx.arc(cx + r * 0.3, cy - r * 0.35, r * 0.16, 0.0, TAU);
    ctx.fill();
    ctx.begin_path();
    ctx.arc(cx - r * 0.25, cy + r * 0.35, r * 0.16, 0.0, TAU);
    ctx.fill();
}

/// A plain folder tab shape — the generic custom-folder icon (concept
/// `#i-folder`), used only if a custom folder ever picks [`FolderIcon::Folder`].
fn draw_icon_folder(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_fill_color(color);
    ctx.fill_rounded_rect(cx - r * 0.75, cy - r * 0.35, r * 1.5, r * 0.9, r * 0.15);
}

/// A bookmark ribbon (a rect with a V-notch cut from its bottom edge) —
/// "Saved messages"'s own avatar glyph and [`FolderIcon::Bookmark`]
/// (concept `#i-bookmark`).
fn draw_icon_bookmark(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_fill_color(color);
    ctx.begin_path();
    ctx.move_to(cx - r * 0.45, cy - r * 0.65);
    ctx.line_to(cx + r * 0.45, cy - r * 0.65);
    ctx.line_to(cx + r * 0.45, cy + r * 0.65);
    ctx.line_to(cx, cy + r * 0.2);
    ctx.line_to(cx - r * 0.45, cy + r * 0.65);
    ctx.close_path();
    ctx.fill();
}

/// A speaker shape with a diagonal slash — the mute glyph (concept
/// `#i-mute`).
fn draw_icon_mute(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_fill_color(color);
    ctx.begin_path();
    ctx.move_to(cx - r * 0.7, cy - r * 0.25);
    ctx.line_to(cx - r * 0.25, cy - r * 0.25);
    ctx.line_to(cx + r * 0.15, cy - r * 0.6);
    ctx.line_to(cx + r * 0.15, cy + r * 0.6);
    ctx.line_to(cx - r * 0.25, cy + r * 0.25);
    ctx.line_to(cx - r * 0.7, cy + r * 0.25);
    ctx.close_path();
    ctx.fill();
    ctx.set_stroke_color(color);
    ctx.set_stroke_width(1.3);
    ctx.begin_path();
    ctx.move_to(cx + r * 0.35, cy - r * 0.5);
    ctx.line_to(cx + r * 0.85, cy + r * 0.5);
    ctx.stroke();
}

/// A thumbtack pin (a circle head, a triangular point) — concept `#i-pin`.
fn draw_icon_pin(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_fill_color(color);
    ctx.begin_path();
    ctx.arc(cx, cy - r * 0.15, r * 0.5, 0.0, TAU);
    ctx.fill();
    ctx.begin_path();
    ctx.move_to(cx - r * 0.18, cy + r * 0.2);
    ctx.line_to(cx + r * 0.18, cy + r * 0.2);
    ctx.line_to(cx, cy + r * 0.75);
    ctx.close_path();
    ctx.fill();
}

/// A plain checkmark — the verified-channel corner badge (concept
/// `#i-verified`, simplified to just its own check).
fn draw_icon_check(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_stroke_color(color);
    ctx.set_stroke_width(1.4);
    ctx.set_line_cap("round");
    ctx.set_line_join("round");
    ctx.begin_path();
    ctx.move_to(cx - r * 0.5, cy);
    ctx.line_to(cx - r * 0.1, cy + r * 0.4);
    ctx.line_to(cx + r * 0.55, cy - r * 0.4);
    ctx.stroke();
}

fn draw_icon_search(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_stroke_color(color);
    ctx.set_stroke_width(1.4);
    ctx.set_line_cap("round");
    ctx.begin_path();
    ctx.arc(cx - r * 0.15, cy - r * 0.15, r * 0.45, 0.0, TAU);
    ctx.stroke();
    ctx.begin_path();
    ctx.move_to(cx + r * 0.2, cy + r * 0.2);
    ctx.line_to(cx + r * 0.65, cy + r * 0.65);
    ctx.stroke();
}

fn draw_icon_more(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_fill_color(color);
    for dy in [-0.5, 0.0, 0.5] {
        ctx.begin_path();
        ctx.arc(cx, cy + dy * r, r * 0.12, 0.0, TAU);
        ctx.fill();
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
    let field_rect = WidgetRect::new(x, y, field_w, SEARCH_FIELD_H);
    let (rect, chars) = draw_field_shaped(
        ctx, coord, layer_id, "cloud_overlay:dm:chat_filter_input", DM_CHAT_FILTER_FIELD, field_rect,
        "Search", ft, tt, current_time_ms, SEARCH_FIELD_RADIUS, SEARCH_FIELD_H_PADDING,
    );
    // The magnifier glyph — drawn AFTER the field so it sits on top of its
    // own placeholder/caret paint, left-inset the same amount the field's
    // own text padding already reserves.
    draw_icon_search(ctx, rect.x + 15.0, rect.center_y(), 7.0, &tt.muted_ink);
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
    provider: &dyn MessengerProvider,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
    list_result: &mut CommunityListPaneResult,
) {
    let mut rooms = provider.rooms();
    let folders = provider.folders();
    rooms.retain(|r| folder_includes(&state.messenger_folder, &folders, r));
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
        let msg = if had_any_room {
            "No matches."
        } else {
            match state.messenger_folder {
                MessengerFolderSel::All => "No conversations yet.",
                MessengerFolderSel::Archive => "No archived chats.",
                _ => "No chats in this folder.",
            }
        };
        draw_status(ctx, tt, lx, ly, lw, msg, None);
    } else {
        for r in &rooms {
            let is_open = matches!(&state.dm.view, crate::ui::modal_settings::DmView::Conversation { id, .. } if id.to_string() == r.id.as_str());
            ly += draw_room_row(ctx, coord, layer_id, lx, ly, lw, r, provider, is_open, current_time_ms, tt);
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
fn list_content_h(state: &CommunityState, rooms: &[RoomSummary]) -> f64 {
    let open_h = if state.dm.new_conversation_open { open_peer_row_h(state.forum2.nick_required()) } else { 0.0 };
    let rows_h = if rooms.is_empty() { 24.0 } else { rooms.len() as f64 * CONV_ROW_H };
    CHAT_FILTER_ROW_H + open_h + rows_h
}

/// Whether a room's row/reactions-adjacent badges should render in the
/// MUTED/grey style (concept `.ub.m`/`.bd.m`) rather than accent — either
/// the room itself is individually muted, or it is a Channel (channels read
/// as lower-urgency by convention, same as the rail's own Channels badge).
fn unread_badge_muted(room: &RoomSummary) -> bool {
    room.muted || matches!(room.kind, RoomKind::Channel)
}

/// A room's own chat-list preview line — the last-event text (or a typing
/// override), plus an optional bold "author: " prefix for a group/channel
/// room or an own last message (concept `.rp b`).
struct RoomPreview {
    prefix: Option<String>,
    text: String,
    encrypted: bool,
    typing: bool,
}

fn room_preview(r: &RoomSummary, is_self_chat: bool) -> Option<RoomPreview> {
    if !r.typing_user_ids.is_empty() {
        return Some(RoomPreview {
            prefix: None,
            text: format!("{} is typing…", r.typing_user_ids[0].as_str()),
            encrypted: false,
            typing: true,
        });
    }
    let ev = r.last_event.as_ref()?;
    let (text, encrypted) = match &ev.content {
        PreviewContent::Text(t) => (t.clone(), false),
        PreviewContent::Encrypted => ("Encrypted message".to_string(), true),
        PreviewContent::Redacted => ("Message deleted".to_string(), false),
        PreviewContent::Notice(n) => (n.clone(), false),
    };
    let prefix = if is_self_chat {
        None
    } else if matches!(r.kind, RoomKind::Group) {
        // Channels are single-broadcaster-shaped in the concept's own row
        // anatomy (screen 2's own channel rows never prefix an author) —
        // only a multi-sender GROUP row gets one.
        Some(format!("{}: ", ev.sender.as_str()))
    } else if ev.sender.as_str() == ME {
        Some("You: ".to_string())
    } else {
        None
    };
    Some(RoomPreview { prefix, text, encrypted, typing: false })
}

/// The row's own single trailing marker (concept: at most ONE of a pin
/// icon / an unread pill / an `@` mention pill ever shows per row). The
/// currently OPEN row suppresses its own unread/mention marker (you are
/// already looking at it) and shows only its pin state, if any.
enum Trailing {
    None,
    Pin,
    Unread(u64, bool),
    Mention,
}

fn trailing_marker(r: &RoomSummary, is_open: bool) -> Trailing {
    if is_open {
        return if r.pinned { Trailing::Pin } else { Trailing::None };
    }
    if r.highlight_count > 0 {
        Trailing::Mention
    } else if r.notification_count > 0 {
        Trailing::Unread(r.notification_count, unread_badge_muted(r))
    } else if r.pinned {
        Trailing::Pin
    } else {
        Trailing::None
    }
}

fn rank_label(rank: RankBadge) -> &'static str {
    match rank {
        RankBadge::Paid => "PAID",
        RankBadge::Trusted => "TRUSTED",
        RankBadge::Moderator => "MOD",
        RankBadge::Admin => "ADMIN",
    }
}

/// One chat-list row — concept v1 `.row`: a 42px avatar (circle for a
/// person, rounded square with a kind badge for a group/channel), then two
/// text lines (name [+ rank chip] [+ mute glyph] + time; preview [+ author
/// prefix] + one trailing marker). Padding 7×8, radius 6.
#[allow(clippy::too_many_arguments)]
fn draw_room_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    r: &RoomSummary,
    provider: &dyn MessengerProvider,
    is_open: bool,
    current_time_ms: u64,
    tt: &ToolbarTheme,
) -> f64 {
    let widget_id = format!("cloud_overlay:dm:conversation:{}", r.id.as_str());
    let rect = WidgetRect::new(x, y, w, CONV_ROW_RECT_H);
    let hovered = coord.is_hovered(&WidgetId::from(widget_id.as_str()));
    if is_open {
        ctx.set_fill_color(&tt.button_bg);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, LIST_ROW_RADIUS);
    } else if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, LIST_ROW_RADIUS);
    }

    // "Saved messages" is a self-chat — no clean model signal exists for
    // "this Direct room's only other member is me" (see `RoomSummary`'s own
    // doc: no peer-user-id field), so this one name is matched literally, a
    // narrow, documented special case rather than a general rule.
    let is_self_chat = r.name == "Saved messages";

    let avatar_cx = x + LIST_ROW_PAD_H + LIST_AVATAR_D / 2.0;
    let avatar_cy = rect.y + rect.height / 2.0;
    let avatar_bg: &str = if matches!(r.kind, RoomKind::Channel) { tt.button_bg.as_str() } else { tt.item_bg_hover.as_str() };
    let (shape, corner) = match r.kind {
        RoomKind::Direct => {
            let online = !is_self_chat
                && r.peer_last_active.map(|t| (current_time_ms as i64 - t.0).abs() < 5 * 60_000).unwrap_or(false);
            (AvatarShape::Circle, if online { CornerMarker::Presence { color: &tt.success } } else { CornerMarker::None })
        }
        RoomKind::Group => (AvatarShape::RoundedSquare, CornerMarker::Kind { background: avatar_bg }),
        RoomKind::Channel => (AvatarShape::RoundedSquare, CornerMarker::Kind { background: avatar_bg }),
    };
    let marker = draw_avatar(
        ctx, avatar_cx, avatar_cy, LIST_AVATAR_D / 2.0, shape,
        if is_self_chat { "" } else { &r.initials },
        &AvatarColors { background: avatar_bg, text: contrasting_text_color(avatar_bg), ring: &tt.dropdown_bg },
        corner,
    );
    if is_self_chat {
        draw_icon_bookmark(ctx, avatar_cx, avatar_cy, LIST_AVATAR_D * 0.28, &tt.item_text);
    }
    if let Some((bx, by, br)) = marker {
        match r.kind {
            RoomKind::Group => draw_icon_group(ctx, bx, by, br * 0.85, &tt.item_text),
            RoomKind::Channel if r.is_public => draw_icon_check(ctx, bx, by, br * 0.85, &tt.item_text),
            RoomKind::Channel => draw_icon_channel(ctx, bx, by, br * 0.7, &tt.item_text),
            RoomKind::Direct => {}
        }
    }

    let text_x = x + LIST_ROW_PAD_H + LIST_AVATAR_D + LIST_AVATAR_GAP;
    let text_right = x + w - LIST_ROW_PAD_H;

    // Line 1 — name [+ rank chip] (left) + [mute glyph] + time (right).
    let peer_rank = if matches!(r.kind, RoomKind::Direct) && !is_self_chat {
        provider.members(&r.id).into_iter().find(|m| m.user_id.as_str() != ME).and_then(|m| m.rank)
    } else {
        None
    };
    let time_label = row_time_label_for(r, current_time_ms);
    let time_w = time_label.as_ref().map(|t| {
        ctx.set_font("11px monospace");
        ctx.measure_text(t)
    });
    let mute_w = if r.muted { 18.0 } else { 0.0 };
    let line1_y = rect.y + (rect.height - 29.0) / 2.0;
    let name_right = text_right - time_w.unwrap_or(0.0) - mute_w - if time_label.is_some() { 6.0 } else { 0.0 };
    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color(if hovered || is_open { &tt.item_text_hover } else { &tt.item_text });
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let name_budget = (name_right - text_x - if peer_rank.is_some() { 34.0 } else { 0.0 }).max(0.0);
    let label = crate::ui::widgets::fit_text_to_width(ctx, &r.name, name_budget);
    ctx.fill_text(&label, text_x, line1_y);
    let name_w = ctx.measure_text(&label);
    if let Some(rank) = peer_rank {
        draw_rank_chip(ctx, text_x + name_w + 6.0, line1_y, name_right, rank_label(rank), tt);
    }
    if r.muted {
        draw_icon_mute(ctx, text_right - (time_w.unwrap_or(0.0) + 6.0) - 8.0, line1_y + 6.5, 7.0, &tt.muted_ink);
    }
    if let Some(t) = &time_label {
        ctx.set_font("11px monospace");
        ctx.set_fill_color(&tt.muted_ink);
        ctx.set_text_align(TextAlign::Right);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(t, text_right, line1_y);
    }

    // Line 2 — preview [+ author prefix] (left) + trailing marker (right).
    let line2_y = line1_y + 15.0;
    let trailing = trailing_marker(r, is_open);
    let badge_h = 16.0;
    let (trailing_w, trailing_draw): (f64, Box<dyn FnOnce(&mut dyn RenderContext, f64)>) = match trailing {
        Trailing::None => (0.0, Box::new(|_, _| {})),
        Trailing::Pin => (14.0, Box::new(move |ctx: &mut dyn RenderContext, tx: f64| {
            draw_icon_pin(ctx, tx + 7.0, line2_y + 6.5, 7.0, &tt.muted_ink);
        })),
        Trailing::Mention => {
            let bw = badge_h;
            (bw, Box::new(move |ctx: &mut dyn RenderContext, tx: f64| {
                draw_count_badge(
                    ctx, tx, line2_y - 1.0, bw, badge_h, &BadgeContent::Mention,
                    &BadgeColors { background: &tt.accent, text: contrasting_text_color(&tt.accent) },
                );
            }))
        }
        Trailing::Unread(count, muted) => {
            let label = if count > 99 { "99+".to_string() } else { count.to_string() };
            let bw = badge_width(ctx, &BadgeContent::Count(&label), badge_h);
            let (bg, fg): (&str, &str) = if muted {
                (tt.item_bg_hover.as_str(), tt.item_text_muted.as_str())
            } else {
                (tt.accent.as_str(), contrasting_text_color(&tt.accent))
            };
            (bw, Box::new(move |ctx: &mut dyn RenderContext, tx: f64| {
                draw_count_badge(ctx, tx, line2_y - 1.0, bw, badge_h, &BadgeContent::Count(&label), &BadgeColors { background: bg, text: fg });
            }))
        }
    };
    let trailing_x = text_right - trailing_w;
    trailing_draw(ctx, trailing_x);
    let preview_right = if trailing_w > 0.0 { trailing_x - 6.0 } else { text_right };

    if let Some(p) = room_preview(r, is_self_chat) {
        let mut px = text_x;
        if let Some(prefix) = &p.prefix {
            ctx.set_font("500 12.5px sans-serif");
            ctx.set_fill_color(&tt.item_text);
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Top);
            let fit = crate::ui::widgets::fit_text_to_width(ctx, prefix, (preview_right - px).max(0.0));
            ctx.fill_text(&fit, px, line2_y);
            px += ctx.measure_text(&fit);
        }
        let (font, color): (&str, &str) = if p.typing {
            ("12.5px sans-serif", tt.accent.as_str())
        } else if p.encrypted {
            ("italic 12.5px sans-serif", tt.muted_ink.as_str())
        } else {
            ("12.5px sans-serif", tt.item_text_muted.as_str())
        };
        ctx.set_font(font);
        ctx.set_fill_color(color);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        let fit = crate::ui::widgets::fit_text_to_width(ctx, &p.text, (preview_right - px).max(0.0));
        ctx.fill_text(&fit, px, line2_y);
    }

    // `RIGHT_CLICK` alongside `CLICK` — the messenger UI-2b chat-row context
    // menu (`ChartApp::try_messenger_right_click`, `mlc-app`) hit-tests this
    // SAME registered widget through `InputCoordinator::process_right_click`
    // rather than a second, parallel rect list.
    coord.register_on_layer(widget_id.as_str(), rect, Sense::CLICK | Sense::HOVER | Sense::RIGHT_CLICK, layer_id);
    CONV_ROW_H
}

/// A ladder-rank chip (concept `.rank`: small, bordered, monospace,
/// uppercase) — drawn only up to `max_x`, so a long name never pushes it
/// past its own row/header's right edge (silently skipped rather than
/// clipped mid-glyph).
fn draw_rank_chip(ctx: &mut dyn RenderContext, x: f64, name_top_y: f64, max_x: f64, rank: &str, tt: &ToolbarTheme) {
    let label = rank.to_uppercase();
    ctx.set_font("600 9px monospace");
    let text_w = ctx.measure_text(&label);
    let chip_h = 13.0;
    let chip_w = text_w + 8.0;
    if x + chip_w > max_x {
        return;
    }
    let chip_y = name_top_y + 2.0;
    ctx.set_fill_color(&tt.item_bg_hover);
    ctx.fill_rounded_rect(x, chip_y, chip_w, chip_h, 3.0);
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&label, x + chip_w / 2.0, chip_y + chip_h / 2.0 + 0.5);
}

/// A chat-list row's own last-message/typing time label — `HH:MM` for
/// today, a short weekday name within the last week, else `D Mon` (concept
/// v1's own mix: `14:32`/`Mon`/`Tue`/`Sun`/`12 Sep`).
fn row_time_label_for(r: &RoomSummary, now_ms: u64) -> Option<String> {
    let ev = r.last_event.as_ref()?;
    Some(row_time_label(ev.ts, now_ms))
}

fn row_time_label(ts: Timestamp, now_ms: u64) -> String {
    let day = day_key(ts);
    let today = day_key(Timestamp(now_ms as i64));
    let delta = today - day;
    if delta == 0 {
        clock_hm(ts)
    } else if (1..7).contains(&delta) {
        weekday_short(day).to_string()
    } else {
        let (_, m, d) = civil_from_days(day);
        format!("{d} {}", month_short(m))
    }
}

// =============================================================================
// Chat pane — a fixed header ([`draw_chat_header`]/[`draw_chat_header_group`]),
// an optional pin bar, day/unread separators + message GROUPS from
// [`MessengerProvider::timeline`] in a bottom-anchored scrollable feed with a
// jump-to-latest button, and the docked composer ([`draw_composer`]): the
// single place every reason the composer cannot send yet shows up, one calm
// line at a time — no separate pane-top notice.
// =============================================================================

/// The chat pane's own fixed 1:1 header (concept `.ch`) — the peer's name
/// (plus their ladder rank chip, when [`DmConversationUi::peer_rank`] has
/// resolved one) on line 1, a short key-fingerprint line on line 2. Never
/// scrolls with the feed (see this section's own top doc).
#[allow(clippy::too_many_arguments)]
fn draw_chat_header(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    w: f64,
    peer_label: &str,
    peer_rank: Option<&str>,
    fingerprint: Option<&str>,
    has_key: bool,
    key_changed: bool,
    tt: &ToolbarTheme,
) {
    draw_soft_divider_h(ctx, x, y + CHAT_HEADER_H, w, tt);

    let text_x = x + CHAT_HEADER_PAD_L;
    let text_right = (x + w - 8.0).max(text_x);
    let line1_y = y + (CHAT_HEADER_H - 25.5) / 2.0;
    let line2_y = line1_y + 15.0;

    ctx.set_font("600 13.5px sans-serif");
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let name_budget = (text_right - text_x).max(0.0);
    let name_label = crate::ui::widgets::fit_text_to_width(ctx, peer_label, name_budget);
    let name_w = ctx.measure_text(&name_label).min(name_budget);
    ctx.fill_text(&name_label, text_x, line1_y);

    if let Some(rank) = peer_rank.filter(|r| !r.is_empty()) {
        draw_rank_chip(ctx, text_x + name_w + 6.0, line1_y, text_right, rank, tt);
    }

    ctx.set_font("11px monospace");
    ctx.set_fill_color(if key_changed { &tt.danger } else { &tt.item_text_muted });
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let key_line = match fingerprint {
        Some(fp) => format!("key {}", short_fingerprint(fp)),
        None if has_key => "key not published yet".to_string(),
        None => "no messaging key yet".to_string(),
    };
    let fit = crate::ui::widgets::fit_text_to_width(ctx, &key_line, name_budget);
    ctx.fill_text(&fit, text_x, line2_y);
}

/// The chat pane's own fixed GROUP/CHANNEL header — name on line 1,
/// a typing indicator (accent) or a member/subscriber count (muted) on
/// line 2 (concept `.ch .n`/`.ch .s`/`.ch .s.typing`).
fn draw_chat_header_group(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, name: &str, subtitle: &str, subtitle_accent: bool, tt: &ToolbarTheme) {
    draw_soft_divider_h(ctx, x, y + CHAT_HEADER_H, w, tt);

    let text_x = x + CHAT_HEADER_PAD_L;
    let text_right = (x + w - 8.0).max(text_x);
    let line1_y = y + (CHAT_HEADER_H - 25.5) / 2.0;
    let line2_y = line1_y + 15.0;

    ctx.set_font("600 13.5px sans-serif");
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let name_budget = (text_right - text_x).max(0.0);
    let fit = crate::ui::widgets::fit_text_to_width(ctx, name, name_budget);
    ctx.fill_text(&fit, text_x, line1_y);

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(if subtitle_accent { &tt.accent } else { &tt.item_text_muted });
    let fit_sub = crate::ui::widgets::fit_text_to_width(ctx, subtitle, name_budget);
    ctx.fill_text(&fit_sub, text_x, line2_y);
}

/// The three header icon buttons (search/members/more, concept `.ch`'s own
/// trailing `.ib` row) — inert this pass (no `MessengerIntent`/context-menu
/// equivalent wired yet), registered for hover only so the chrome reads
/// correctly; a later click-dispatch piece gives them a real action.
fn draw_header_icons(ctx: &mut dyn RenderContext, coord: &mut InputCoordinator, layer_id: &LayerId, x: f64, y: f64, w: f64, tt: &ToolbarTheme) {
    let names = ["search", "members", "more"];
    let mut bx = x + w - 8.0 - HEADER_ICON_D;
    for name in names.iter().rev() {
        let id = format!("cloud_overlay:dm:header_{name}");
        let rect = WidgetRect::new(bx, y + (CHAT_HEADER_H - HEADER_ICON_D) / 2.0, HEADER_ICON_D, HEADER_ICON_D);
        let hovered = coord.is_hovered(&WidgetId::from(id.as_str()));
        if hovered {
            ctx.set_fill_color(&tt.item_bg_hover);
            ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
        }
        match *name {
            "search" => draw_icon_search(ctx, rect.center_x(), rect.center_y(), 8.0, &tt.item_text_muted),
            "members" => draw_icon_group(ctx, rect.center_x(), rect.center_y(), 9.0, &tt.item_text_muted),
            _ => draw_icon_more(ctx, rect.center_x(), rect.center_y(), 8.0, &tt.item_text_muted),
        }
        coord.register_on_layer(id.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
        bx -= HEADER_ICON_D + HEADER_ICON_GAP;
    }
}

/// A group/channel room's own header subtitle — a typing indicator when
/// anyone is typing, else a plain member/subscriber count (never fabricated
/// — `members.len()` is exactly what [`MessengerProvider::members`]
/// reported). Returns `(text, is_accent)`.
fn group_subtitle(room: &RoomSummary, member_count: usize) -> (String, bool) {
    if let Some(who) = room.typing_user_ids.first() {
        return (format!("{} is typing…", who.as_str()), true);
    }
    let noun = if matches!(room.kind, RoomKind::Channel) { "subscribers" } else { "members" };
    (format!("{member_count} {noun}"), false)
}

/// A ladder-rank chip (concept `.rank`: small, bordered, monospace,
/// uppercase) — drawn only up to `max_x`, so a long name never pushes it
/// past the header's own right edge (silently skipped rather than clipped
/// mid-glyph, since a half-drawn chip reads worse than none at all).
fn short_fingerprint(fp: &str) -> String {
    let hex: Vec<char> = fp.chars().filter(|c| c.is_ascii_hexdigit()).map(|c| c.to_ascii_uppercase()).take(16).collect();
    hex.chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(" ")
}

/// The pinned-message bar (concept `.pinbar`) — drawn only when the open
/// room's currently-held timeline carries a [`TimelineItem::pinned`] item
/// (the FIRST one found; a real Matrix room can only ever have one message
/// pinned to this exact bar at a time in the concept's own design). Returns
/// the bar's own height, `0.0` when nothing is pinned.
fn draw_pin_bar(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, item: &mlc_messenger_model::TimelineItem, vault_state: VaultState, tt: &ToolbarTheme) -> f64 {
    draw_soft_divider_h(ctx, x, y + PINBAR_H, w, tt);
    let bar_x = x + CHAT_HEADER_PAD_L;
    let bar_h = 26.0;
    let bar_y = y + (PINBAR_H - bar_h) / 2.0;
    ctx.set_fill_color(&tt.accent);
    ctx.fill_rounded_rect(bar_x, bar_y, 2.0, bar_h, 1.0);

    let text_x = bar_x + 10.0;
    let icon_w = 24.0;
    let text_right = (x + w - 10.0 - icon_w).max(text_x);
    let line1_y = y + (PINBAR_H - 26.0) / 2.0;
    ctx.set_font("600 11.5px sans-serif");
    ctx.set_fill_color(&tt.accent);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Pinned message", text_x, line1_y);

    let visual = content_visual(&item.content, vault_state);
    let preview = visual.lines.join(" ");
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    let fit = crate::ui::widgets::fit_text_to_width(ctx, &preview, (text_right - text_x).max(0.0));
    ctx.fill_text(&fit, text_x, line1_y + 14.0);

    draw_icon_pin(ctx, x + w - 10.0 - icon_w / 2.0, y + PINBAR_H / 2.0, 8.0, &tt.muted_ink);
    PINBAR_H
}

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
    provider: &dyn MessengerProvider,
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
    let Some(room) = provider.room(&room_id) else {
        draw_status(ctx, tt, x, body_y, w, "Conversation not found.", None);
        return;
    };
    let members = provider.members(&room_id);
    let timeline = provider.timeline(&room_id);
    let is_direct = matches!(room.kind, RoomKind::Direct);

    let peer = members.iter().find(|m| m.user_id.as_str() != ME);
    let conversation = if is_direct { state.dm.conversations.iter().find(|c| c.id == *id) } else { None };
    let fingerprint = conversation.and_then(|c| c.peer_fingerprint.as_deref());
    let peer_rank = conversation.and_then(|c| c.peer_rank.as_deref());
    let has_key = if is_direct { peer.map(|m| m.has_messaging_key).unwrap_or(false) } else { true };
    let key_changed = is_direct && state.dm.has_peer_key_changed(*id);

    // Fixed 52px header (concept `.ch`) — drawn once, above the scrollable
    // feed, rather than as the feed's own first scrolling row.
    if is_direct {
        draw_chat_header(ctx, x, body_y, w, &room.name, peer_rank, fingerprint, has_key, key_changed, tt);
    } else {
        let (subtitle, accent) = group_subtitle(&room, members.len());
        draw_chat_header_group(ctx, x, body_y, w, &room.name, &subtitle, accent, tt);
    }
    draw_header_icons(ctx, coord, layer_id, x, body_y, w, tt);

    let mut feed_top = body_y + CHAT_HEADER_H;
    let pinned_item = timeline.iter().find(|i| i.pinned);
    if let Some(item) = pinned_item {
        feed_top += draw_pin_bar(ctx, x, feed_top, w, item, provider.vault_state(), tt);
    }

    let feed_y = feed_top;
    let channel_gate = matches!(room.kind, RoomKind::Channel)
        .then(|| ChannelGate { can_post: room.can_post, muted: room.muted });
    let feed_h = (body_viewport_h - (feed_top - body_y) - dock_h(state, channel_gate)).max(0.0);
    let viewport_rect = WidgetRect::new(x, feed_y, w, feed_h);
    coord.register_on_layer("cloud_overlay:scroll_viewport", viewport_rect, Sense::SCROLL, layer_id);
    let scrollable = ScrollableContainer::new(viewport_rect, &state.scroll, Some(scroll_config()));
    scrollable.begin(ctx);
    let sx = x;
    let mut sy = scrollable.content_y();
    let sw = scrollable.content_width();
    let content_top = sy;

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
    } else if matches!(room.kind, RoomKind::Channel) {
        sy += draw_channel_feed(ctx, sx, sy, sw, &timeline, &members, current_time_ms, tt);
    } else {
        sy += draw_timeline(
            ctx, coord, layer_id, sx, sy, sw, &timeline, &room, &members, is_direct, provider.vault_state(), caps,
            current_time_ms, state.dm.selection, tt, result,
        );
    }
    let content_h = sy - content_top;

    let widget_theme = crate::ui::widgets::types::WidgetTheme::default();
    let scroll_result = scrollable.end(ctx, content_h, &widget_theme);
    result.total_content_height = Some(content_h);
    result.viewport_height = Some(feed_h);
    result.scroll_viewport_rect = Some(viewport_rect);
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
    register_scrollbar_hit_zones(coord, layer_id, &scroll_result);

    // Floating "jump to latest" — visible only while scrolled away from
    // the bottom (`jump_button_visible`, read-only: see this file's own
    // top doc for why LANDING at the bottom on open/new-message is not
    // wired this pass). Its own click is the one new widget id this piece
    // adds (`panel_click.rs`'s `"jump_latest"` arm).
    if jump_button_visible(&state.scroll, content_h, feed_h) {
        draw_jump_to_latest(ctx, coord, layer_id, x, feed_y, w, feed_h, room.notification_count, tt);
    }

    draw_composer(
        ctx, coord, layer_id, x, body_y, w, body_viewport_h, state, locked, has_key, channel_gate, tt, ft,
        current_time_ms, result,
    );

    // The forward flow's own chat picker (concept: "bubble action → chat
    // picker → `MessengerIntent::Forward`") — floats above the whole pane,
    // drawn LAST so it paints over the composer/feed underneath it.
    if let Some(picker) = &state.forward_target_picker {
        draw_forward_picker(ctx, coord, layer_id, x, body_y, w, body_viewport_h, provider, &picker.from_room, tt);
    }
}

/// The forward flow's own chat picker — a centred panel listing every
/// OTHER room `provider.rooms()` reports, click-to-forward. Cancel closes
/// it without enqueuing anything.
#[allow(clippy::too_many_arguments)]
fn draw_forward_picker(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    provider: &dyn MessengerProvider,
    from_room: &str,
    tt: &ToolbarTheme,
) {
    let targets: Vec<RoomSummary> = provider.rooms().into_iter().filter(|r| r.id.as_str() != from_room).collect();
    let panel_w = 280.0f64.min(w - 24.0);
    let row_h = 44.0;
    let header_h = 34.0;
    let panel_h = (header_h + targets.len() as f64 * row_h + 8.0).min(h - 24.0).max(header_h + 8.0);
    let panel_x = x + (w - panel_w) / 2.0;
    let panel_y = y + (h - panel_h) / 2.0;

    ctx.set_global_alpha(0.45);
    ctx.set_fill_color("#000000");
    ctx.fill_rect(x, y, w, h);
    ctx.set_global_alpha(1.0);

    ctx.set_fill_color(&tt.dropdown_bg);
    ctx.fill_rounded_rect(panel_x, panel_y, panel_w, panel_h, 8.0);
    ctx.set_stroke_color(&tt.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(panel_x, panel_y, panel_w, panel_h, 8.0);

    ctx.set_font("600 12.5px sans-serif");
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Forward to…", panel_x + 12.0, panel_y + header_h / 2.0);

    let close_rect = WidgetRect::new(panel_x + panel_w - 28.0, panel_y + 4.0, 24.0, 24.0);
    let close_hovered = coord.is_hovered(&WidgetId::from("cloud_overlay:dm:forward_cancel"));
    ctx.set_fill_color(if close_hovered { &tt.item_text } else { &tt.item_text_muted });
    ctx.set_text_align(TextAlign::Center);
    ctx.fill_text("\u{2715}", close_rect.center_x(), close_rect.center_y());
    coord.register_on_layer("cloud_overlay:dm:forward_cancel", close_rect, Sense::CLICK | Sense::HOVER, layer_id);

    let mut ry = panel_y + header_h;
    for room in &targets {
        let rect = WidgetRect::new(panel_x + 4.0, ry, panel_w - 8.0, row_h - 2.0);
        let id = format!("cloud_overlay:dm:forward_to:{}", room.id.as_str());
        let hovered = coord.is_hovered(&WidgetId::from(id.as_str()));
        if hovered {
            ctx.set_fill_color(&tt.item_bg_hover);
            ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 6.0);
        }
        ctx.set_font("500 12.5px sans-serif");
        ctx.set_fill_color(&tt.item_text);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        let fit = crate::ui::widgets::fit_text_to_width(ctx, &room.name, rect.width - 16.0);
        ctx.fill_text(&fit, rect.x + 8.0, rect.center_y());
        coord.register_on_layer(id.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
        ry += row_h;
    }
}

/// A Channel room's own post feed (concept screen "2 · Канал") — each
/// timeline item draws as a standalone, full-width post (no avatar, no
/// author-grouping: the concept's own channel post markup carries neither),
/// author-signed at its own bottom, day-separated the same way the group/
/// direct feed is. Views/comment-thread rows are NOT drawn — the model
/// carries no view-count or comment-thread concept yet (a gap for a later
/// pass with its own [`mlc_messenger_model`] additions, not invented here).
fn draw_channel_feed(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    w: f64,
    timeline: &[mlc_messenger_model::TimelineItem],
    members: &[mlc_messenger_model::Member],
    current_time_ms: u64,
    tt: &ToolbarTheme,
) -> f64 {
    let top = y;
    let mut sy = y;
    let mut prev_day: Option<i64> = None;
    for item in timeline {
        let day = day_key(item.ts);
        if prev_day != Some(day) {
            let label = day_label(Timestamp(day * 86_400_000), current_time_ms);
            if sy > top {
                sy += BUBBLE_GAP;
            }
            sy += draw_day_separator(ctx, x, sy, w, &label, tt);
            prev_day = Some(day);
        } else {
            sy += BUBBLE_GAP;
        }
        let sender_display = members.iter().find(|m| m.user_id == item.sender).map(|m| m.display_name.as_str()).unwrap_or(item.sender.as_str());
        sy += draw_channel_post(ctx, x, sy, w, item, sender_display, tt);
    }
    sy - y
}

/// One channel post (concept `.b.peer{max-width:88%}`, no avatar/name
/// header — the author signs at the post's own bottom instead, alongside
/// the timestamp).
fn draw_channel_post(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    w: f64,
    item: &mlc_messenger_model::TimelineItem,
    sender_display: &str,
    tt: &ToolbarTheme,
) -> f64 {
    let visual = content_visual(&item.content, VaultState::Ready);
    let line_count = visual.lines.len().max(1);
    let signature_h = 16.0;
    let forwarded_h = if item.forwarded.is_some() { FORWARDED_LABEL_H } else { 0.0 };
    let reactions_h = if item.reactions.is_empty() { 0.0 } else { REACTIONS_ROW_H };
    let bubble_w = (w * 0.88).max(BUBBLE_MIN_W).min(w);
    let inner_h = forwarded_h + line_count as f64 * BUBBLE_LINE_H + signature_h + reactions_h;
    let bubble_h = inner_h + BUBBLE_PAD_V * 2.0;

    ctx.set_fill_color(&tt.button_bg);
    fill_rect_corners(ctx, x, y, bubble_w, bubble_h, BUBBLE_RADIUS, BUBBLE_RADIUS, BUBBLE_RADIUS, BUBBLE_TAIL_RADIUS);

    let text_left = x + BUBBLE_PAD_H;
    let text_right = x + bubble_w - BUBBLE_PAD_H;
    let mut cy = y + BUBBLE_PAD_V;

    if let Some(f) = &item.forwarded {
        ctx.set_font("italic 11px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        let label = forwarded_label(f);
        let fit = crate::ui::widgets::fit_text_to_width(ctx, &label, (text_right - text_left).max(0.0));
        ctx.fill_text(&fit, text_left, cy);
        cy += FORWARDED_LABEL_H;
    }

    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    for (i, line) in visual.lines.iter().enumerate() {
        let line_y = cy + i as f64 * BUBBLE_LINE_H;
        ctx.set_fill_color(tt.item_text.as_str());
        ctx.fill_text(line, text_left, line_y);
    }
    cy += line_count as f64 * BUBBLE_LINE_H;

    ctx.set_font("11px monospace");
    ctx.set_fill_color(&tt.muted_ink);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(&format!("— {sender_display}"), text_left, cy);
    ctx.set_text_align(TextAlign::Right);
    ctx.fill_text(&clock_hm(item.ts), text_right, cy);

    if !item.reactions.is_empty() {
        draw_reactions_row(ctx, text_left, cy + signature_h, &item.reactions, tt);
    }

    bubble_h + BUBBLE_GAP
}

/// One contiguous run of same-sender, same-day, non-system-notice timeline
/// items — the concept's own `.grp`: they share ONE avatar (drawn once, on
/// the LAST bubble) and ONE name/rank header (drawn once, on the FIRST).
/// System-notice items are never grouped (each is its own one-item "group",
/// tagged `is_notice`).
struct BubbleGroup<'a> {
    day: i64,
    sender: &'a str,
    items: Vec<&'a mlc_messenger_model::TimelineItem>,
    is_notice: bool,
}

fn group_timeline(timeline: &[mlc_messenger_model::TimelineItem]) -> Vec<BubbleGroup<'_>> {
    let mut groups: Vec<BubbleGroup> = Vec::new();
    for item in timeline {
        let day = day_key(item.ts);
        if system_notice_text(&item.content).is_some() {
            groups.push(BubbleGroup { day, sender: item.sender.as_str(), items: vec![item], is_notice: true });
            continue;
        }
        if let Some(last) = groups.last_mut() {
            if !last.is_notice && last.day == day && last.sender == item.sender.as_str() {
                last.items.push(item);
                continue;
            }
        }
        groups.push(BubbleGroup { day, sender: item.sender.as_str(), items: vec![item], is_notice: false });
    }
    groups
}

/// Position in `timeline` where the unread run starts: the `unread` newest
/// items that are real messages. System notices (membership, key change,
/// notices) never count toward a notification total, and the timeline is not
/// gapless (name/topic changes and unknown events produce no item at all),
/// so the run is counted over message items only, never as
/// `len - unread`.
fn unread_start(timeline: &[mlc_messenger_model::TimelineItem], unread: u64) -> usize {
    let mut remaining = unread;
    let mut start = timeline.len();
    for (i, item) in timeline.iter().enumerate().rev() {
        if remaining == 0 {
            break;
        }
        if system_notice_text(&item.content).is_none() {
            remaining -= 1;
            start = i;
        }
    }
    start
}

/// Draw the whole feed body: day separators, an unread separator derived
/// from `RoomSummary::notification_count` (the newest N message items read
/// as "unread" — see [`unread_start`]; no per-item read-receipt field
/// exists to place it more precisely), and one call to
/// [`draw_message_group`] per [`BubbleGroup`]. Returns the total height
/// drawn.
#[allow(clippy::too_many_arguments)]
fn draw_timeline(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    timeline: &[mlc_messenger_model::TimelineItem],
    room: &RoomSummary,
    members: &[mlc_messenger_model::Member],
    is_direct: bool,
    vault_state: VaultState,
    caps: mlc_messenger_model::Capabilities,
    current_time_ms: u64,
    selection: Option<Selection>,
    tt: &ToolbarTheme,
    result: &mut CloudOverlayResult,
) -> f64 {
    let groups = group_timeline(timeline);
    let unread_from = unread_start(timeline, room.notification_count);
    let mut sy = y;
    let top = y;
    let mut prev_day: Option<i64> = None;
    let mut consumed: usize = 0;
    let mut unread_drawn = false;
    let mut msg_idx: u16 = 0;
    for group in &groups {
        if prev_day != Some(group.day) {
            let label = day_label(Timestamp(group.day * 86_400_000), current_time_ms);
            if sy > top {
                sy += BUBBLE_GAP;
            }
            sy += draw_day_separator(ctx, x, sy, w, &label, tt);
            prev_day = Some(group.day);
        } else if sy > top {
            sy += BUBBLE_GAP;
        }
        if !unread_drawn && room.notification_count > 0 && consumed >= unread_from {
            sy += draw_unread_separator(ctx, x, sy, w, tt);
            unread_drawn = true;
        }
        if group.is_notice {
            let notice = system_notice_text(&group.items[0].content).unwrap_or_default();
            sy += draw_system_notice(ctx, x, sy, w, &notice, tt);
            consumed += 1;
            msg_idx += 1;
            continue;
        }
        let mine = group.sender == ME;
        let member = members.iter().find(|m| m.user_id.as_str() == group.sender);
        let sender_display = member.map(|m| m.display_name.as_str()).unwrap_or(group.sender);
        let sender_rank = member.and_then(|m| m.rank);
        let show_header = !mine && !is_direct;
        let group_h = draw_message_group(
            ctx, coord, layer_id, x, sy, w, msg_idx, group, mine, show_header, sender_display, sender_rank,
            vault_state, caps, selection, tt, result,
        );
        sy += group_h;
        consumed += group.items.len();
        msg_idx += group.items.len() as u16;
    }
    sy - y
}

#[allow(clippy::too_many_arguments)]
fn draw_message_group(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    start_idx: u16,
    group: &BubbleGroup,
    mine: bool,
    show_name_header: bool,
    sender_display: &str,
    sender_rank: Option<RankBadge>,
    vault_state: VaultState,
    caps: mlc_messenger_model::Capabilities,
    selection: Option<Selection>,
    tt: &ToolbarTheme,
    result: &mut CloudOverlayResult,
) -> f64 {
    let n = group.items.len();
    let mut cy = y;
    for (i, item) in group.items.iter().enumerate() {
        let is_first = i == 0;
        let is_last = i == n - 1;
        let h = draw_bubble(
            ctx, coord, layer_id, x, cy, w, start_idx + i as u16, item, mine, is_first, is_last,
            show_name_header && is_first, sender_display, sender_rank.filter(|_| show_name_header && is_first),
            vault_state, caps, selection, tt, result,
        );
        cy += h;
        if !is_last {
            cy += GROUP_INNER_GAP;
        }
    }
    cy - y
}

fn draw_jump_to_latest(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    scroll_h: f64,
    unread: u64,
    tt: &ToolbarTheme,
) {
    let rect = WidgetRect::new(x + w - JUMP_D - 16.0, y + scroll_h - JUMP_D - 12.0, JUMP_D, JUMP_D);
    let hovered = coord.is_hovered(&WidgetId::from("cloud_overlay:dm:jump_latest"));
    ctx.set_fill_color(&tt.dropdown_bg);
    ctx.begin_path();
    ctx.arc(rect.center_x(), rect.center_y(), JUMP_D / 2.0, 0.0, TAU);
    ctx.fill();
    ctx.set_stroke_color(&tt.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.arc(rect.center_x(), rect.center_y(), JUMP_D / 2.0 - 0.5, 0.0, TAU);
    ctx.stroke();
    if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.set_global_alpha(0.4);
        ctx.begin_path();
        ctx.arc(rect.center_x(), rect.center_y(), JUMP_D / 2.0, 0.0, TAU);
        ctx.fill();
        ctx.set_global_alpha(1.0);
    }
    ctx.set_stroke_color(&tt.item_text_muted);
    ctx.set_stroke_width(1.6);
    ctx.set_line_cap("round");
    ctx.begin_path();
    ctx.move_to(rect.center_x(), rect.center_y() - 6.0);
    ctx.line_to(rect.center_x(), rect.center_y() + 6.0);
    ctx.move_to(rect.center_x() - 5.0, rect.center_y() + 1.0);
    ctx.line_to(rect.center_x(), rect.center_y() + 6.0);
    ctx.line_to(rect.center_x() + 5.0, rect.center_y() + 1.0);
    ctx.stroke();

    if unread > 0 {
        let label = if unread > 99 { "99+".to_string() } else { unread.to_string() };
        let badge_h = 16.0;
        let bw = badge_width(ctx, &BadgeContent::Count(&label), badge_h);
        let bx = rect.x + rect.width - 4.0 - bw / 2.0;
        let by = rect.y - 7.0;
        draw_count_badge(
            ctx, bx, by, bw, badge_h, &BadgeContent::Count(&label),
            &BadgeColors { background: &tt.accent, text: contrasting_text_color(&tt.accent) },
        );
    }
    coord.register_on_layer("cloud_overlay:dm:jump_latest", rect, Sense::CLICK | Sense::HOVER, layer_id);
}

/// A Channel subscriber's own dock (concept `.cta` `.btn.wide`) — a
/// full-width, borderless Mute/Unmute row replacing the WHOLE composer
/// (this room's `RoomSummary::can_post` is `false`, so there is no draft
/// field to show at all).
fn draw_channel_cta(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    muted: bool,
    tt: &ToolbarTheme,
) {
    let rect = WidgetRect::new(x, y, w, CHANNEL_CTA_H);
    let id = "cloud_overlay:dm:channel_mute";
    let hovered = coord.is_hovered(&WidgetId::from(id));
    if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rect(rect.x, rect.y, rect.width, rect.height);
    }
    ctx.set_font("600 12px sans-serif");
    ctx.set_fill_color(&tt.accent);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(if muted { "Unmute" } else { "Mute" }, rect.center_x(), rect.center_y());
    coord.register_on_layer(id, rect, Sense::CLICK | Sense::HOVER, layer_id);
}

/// The dock — the ONE place every reason the composer cannot send yet
/// shows up (owner spec): a nick-required hint keeps its own pre-existing
/// hint+button shape (a ladder-mandate gate, orthogonal to messaging
/// itself); vault-locked and peer-without-key each get exactly the notice
/// the owner asked for, one line (plus a button for the vault case); a
/// pending reply (`CommunityState::messenger_reply_preview`) adds its own
/// banner above the draft field (concept `.cbar`).
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
    channel_gate: Option<ChannelGate>,
    tt: &ToolbarTheme,
    ft: &FrameTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) {
    let composer_y = y + (viewport_h - dock_h(state, channel_gate)).max(0.0);
    if let Some(gate) = channel_gate {
        if !gate.can_post {
            draw_channel_cta(ctx, coord, layer_id, x, composer_y, w, gate.muted, tt);
            return;
        }
    }
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

    let mut row_y = composer_y;
    if let Some(reply) = &state.messenger_reply_preview {
        draw_soft_divider_h(ctx, x, composer_y, w, tt);
        row_y += draw_reply_banner(ctx, coord, layer_id, x, composer_y, w, reply, tt);
    }
    let row_cy = row_y + COMPOSER_PAD_TOP + COMPOSER_FIELD_H / 2.0;
    row_y += COMPOSER_PAD_TOP;
    // Vault locked (owner spec, UI-2a piece 3): ONE row — a lock glyph, the
    // notice, and a COMPACT accent button hugging its own text, right-
    // aligned (not the pane's full width — see [`DOCK_BTN_H`]'s own doc).
    if locked {
        let label = "Unlock vault";
        ctx.set_font("600 13px sans-serif");
        let btn_w = ctx.measure_text(label) + DOCK_BTN_PAD_H * 2.0;
        let btn_rect = WidgetRect::new(x + w - btn_w, row_cy - DOCK_BTN_H / 2.0, btn_w, DOCK_BTN_H);

        let icon_cx = x + 9.0;
        draw_lock_glyph(ctx, icon_cx, row_cy, 14.0, &tt.item_text_muted);

        let text_x = x + 22.0;
        let text_right = (btn_rect.x - 10.0).max(text_x);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        let hint = crate::ui::widgets::fit_text_to_width(
            ctx, "Vault locked. Messages stay encrypted until you unlock it.", (text_right - text_x).max(0.0),
        );
        ctx.fill_text(&hint, text_x, row_cy);

        draw_action_row(ctx, coord, layer_id, "cloud_overlay:dm:unlock_vault", btn_rect, label, tt);
        return;
    }
    // Peer has no messaging key published yet — a calm, no-button line
    // (owner spec): there is nothing to click here, only something to
    // wait for.
    if !has_key {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        let hint = crate::ui::widgets::fit_text_to_width(
            ctx, "This person hasn't set up messaging yet.", w,
        );
        ctx.fill_text(&hint, x, row_cy);
        return;
    }
    // Normal — one row: attach + a fully-rounded draft field + emoji + a
    // round accent send button (concept `.cin`: `.ib` + `.field` + `.ib` +
    // `.send`, `gap:4px`). Attach/emoji are inert this pass — see
    // [`COMPOSER_ICON_D`]'s own doc.
    let crate::ui::modal_settings::DmView::Conversation { draft_field_id, .. } = &state.dm.view else { return };
    let icon_y = row_y + (COMPOSER_FIELD_H - COMPOSER_ICON_D) / 2.0;
    let attach_rect = WidgetRect::new(x, icon_y, COMPOSER_ICON_D, COMPOSER_ICON_D);
    draw_composer_icon_button(ctx, coord, layer_id, "cloud_overlay:dm:attach", attach_rect, ComposerIcon::Attach, tt);

    let field_x = x + COMPOSER_ICON_D + COMPOSER_GAP;
    let field_w = (w - COMPOSER_ICON_D * 2.0 - COMPOSER_GAP * 3.0 - COMPOSER_SEND_D).max(60.0);
    let field_rect = WidgetRect::new(field_x, row_y, field_w, COMPOSER_FIELD_H);
    let (rect, chars) = draw_field_shaped(
        ctx, coord, layer_id, "cloud_overlay:dm:draft_input", draft_field_id, field_rect,
        "Write a message…", ft, tt, current_time_ms, COMPOSER_FIELD_RADIUS, 14.0,
    );
    result.dm_draft_rect = Some(rect);
    result.dm_draft_char_positions = chars;

    let emoji_rect = WidgetRect::new(field_x + field_w + COMPOSER_GAP, icon_y, COMPOSER_ICON_D, COMPOSER_ICON_D);
    draw_composer_icon_button(ctx, coord, layer_id, "cloud_overlay:dm:emoji", emoji_rect, ComposerIcon::Emoji, tt);

    let send_rect = WidgetRect::new(emoji_rect.x + COMPOSER_ICON_D + COMPOSER_GAP, row_y, COMPOSER_SEND_D, COMPOSER_SEND_D);
    draw_send_button(ctx, coord, layer_id, send_rect, tt);
}

/// The composer's own attach/emoji icon (concept `.ib`) — inert (hover
/// only), same convention [`draw_header_icons`]'s own doc uses.
enum ComposerIcon {
    Attach,
    Emoji,
}

fn draw_composer_icon_button(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    id: &str,
    rect: WidgetRect,
    icon: ComposerIcon,
    tt: &ToolbarTheme,
) {
    let hovered = coord.is_hovered(&WidgetId::from(id));
    if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    }
    let color = &tt.item_text_muted;
    match icon {
        ComposerIcon::Attach => draw_icon_clip(ctx, rect.center_x(), rect.center_y(), 8.0, color),
        ComposerIcon::Emoji => draw_icon_smile(ctx, rect.center_x(), rect.center_y(), 8.0, color),
    }
    coord.register_on_layer(id, rect, Sense::CLICK | Sense::HOVER, layer_id);
}

/// A paperclip glyph — the composer's own attach button (concept `#i-clip`).
fn draw_icon_clip(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_stroke_color(color);
    ctx.set_stroke_width(1.4);
    ctx.set_line_cap("round");
    ctx.begin_path();
    ctx.move_to(cx + r * 0.5, cy - r * 0.6);
    ctx.quadratic_curve_to(cx + r * 0.9, cy - r * 0.2, cx + r * 0.1, cy + r * 0.6);
    ctx.quadratic_curve_to(cx - r * 0.6, cy + r * 1.0, cx - r * 0.7, cy + r * 0.2);
    ctx.quadratic_curve_to(cx - r * 0.8, cy - r * 0.5, cx - r * 0.1, cy - r * 0.5);
    ctx.line_to(cx + r * 0.3, cy + r * 0.35);
    ctx.stroke();
}

/// A round smiley glyph — the composer's own emoji button and the hovered-
/// bubble quick-action bar's own React icon (concept `#i-smile`).
fn draw_icon_smile(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_stroke_color(color);
    ctx.set_stroke_width(1.4);
    ctx.set_line_cap("round");
    ctx.begin_path();
    ctx.arc(cx, cy, r, 0.0, TAU);
    ctx.stroke();
    ctx.begin_path();
    ctx.arc(cx, cy + r * 0.05, r * 0.55, 0.25, std::f64::consts::PI - 0.25);
    ctx.stroke();
    ctx.set_fill_color(color);
    ctx.begin_path();
    ctx.arc(cx - r * 0.32, cy - r * 0.25, r * 0.1, 0.0, TAU);
    ctx.fill();
    ctx.begin_path();
    ctx.arc(cx + r * 0.32, cy - r * 0.25, r * 0.1, 0.0, TAU);
    ctx.fill();
}

/// A reply arrow (bent, pointing left) — the hovered-bubble quick-action
/// bar's own Reply icon (concept `#i-reply`).
fn draw_icon_reply(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_stroke_color(color);
    ctx.set_stroke_width(1.4);
    ctx.set_line_cap("round");
    ctx.set_line_join("round");
    ctx.begin_path();
    ctx.move_to(cx + r * 0.15, cy - r * 0.55);
    ctx.line_to(cx - r * 0.55, cy);
    ctx.line_to(cx + r * 0.15, cy + r * 0.55);
    ctx.move_to(cx - r * 0.55, cy);
    ctx.line_to(cx + r * 0.35, cy);
    ctx.quadratic_curve_to(cx + r * 0.7, cy, cx + r * 0.7, cy + r * 0.5);
    ctx.stroke();
}

/// A forward arrow (bent, pointing right) — the hovered-bubble quick-action
/// bar's own Forward icon (concept `#i-fwd`).
fn draw_icon_forward(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_stroke_color(color);
    ctx.set_stroke_width(1.4);
    ctx.set_line_cap("round");
    ctx.set_line_join("round");
    ctx.begin_path();
    ctx.move_to(cx - r * 0.15, cy - r * 0.55);
    ctx.line_to(cx + r * 0.55, cy);
    ctx.line_to(cx - r * 0.15, cy + r * 0.55);
    ctx.move_to(cx + r * 0.55, cy);
    ctx.line_to(cx - r * 0.35, cy);
    ctx.quadratic_curve_to(cx - r * 0.7, cy, cx - r * 0.7, cy + r * 0.5);
    ctx.stroke();
}

/// The composer's own "replying to" banner (concept `.cbar`) — see
/// [`crate::ui::modal_settings::MessengerReplyPreview`]'s own doc for why
/// its own ✕ is registered but unwired this pass. Returns the banner's own
/// height.
fn draw_reply_banner(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    reply: &crate::ui::modal_settings::MessengerReplyPreview,
    tt: &ToolbarTheme,
) -> f64 {
    let bar_h = 28.0;
    let bar_y = y + (REPLY_BANNER_H - bar_h) / 2.0;
    ctx.set_fill_color(&tt.accent);
    ctx.fill_rounded_rect(x + 2.0, bar_y, 2.0, bar_h, 1.0);

    let close_w = 20.0;
    let text_x = x + 14.0;
    let text_right = x + w - 8.0 - close_w;
    ctx.set_font("600 11.5px sans-serif");
    ctx.set_fill_color(&tt.accent);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let label = format!("Reply to {}", reply.sender_label);
    let fit_label = crate::ui::widgets::fit_text_to_width(ctx, &label, (text_right - text_x).max(0.0));
    ctx.fill_text(&fit_label, text_x, bar_y);

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    let fit_preview = crate::ui::widgets::fit_text_to_width(ctx, &reply.preview, (text_right - text_x).max(0.0));
    ctx.fill_text(&fit_preview, text_x, bar_y + 14.0);

    let close_rect = WidgetRect::new(x + w - 8.0 - close_w, bar_y - 2.0, close_w, close_w);
    let hovered = coord.is_hovered(&WidgetId::from("cloud_overlay:dm:cancel_reply"));
    ctx.set_stroke_color(if hovered { &tt.item_text } else { &tt.item_text_muted });
    ctx.set_stroke_width(1.4);
    ctx.set_line_cap("round");
    ctx.begin_path();
    ctx.move_to(close_rect.x + 5.0, close_rect.y + 5.0);
    ctx.line_to(close_rect.x + close_rect.width - 5.0, close_rect.y + close_rect.height - 5.0);
    ctx.move_to(close_rect.x + close_rect.width - 5.0, close_rect.y + 5.0);
    ctx.line_to(close_rect.x + 5.0, close_rect.y + close_rect.height - 5.0);
    ctx.stroke();
    coord.register_on_layer("cloud_overlay:dm:cancel_reply", close_rect, Sense::CLICK | Sense::HOVER, layer_id);

    REPLY_BANNER_H
}

/// The round accent send button (concept `.send`) — a filled circle with a
/// plain glyph, same "font-glyph icon" idiom [`draw_chat_filter_row`]'s own
/// pencil button already uses rather than a vector path (this crate sits
/// below the `IconId` SVG registry's own home — see `draw_lock_glyph`'s own
/// doc).
fn draw_send_button(ctx: &mut dyn RenderContext, coord: &mut InputCoordinator, layer_id: &LayerId, rect: WidgetRect, tt: &ToolbarTheme) {
    let widget_id = "cloud_overlay:dm:send";
    let hovered = coord.is_hovered(&WidgetId::from(widget_id));
    let cx = rect.center_x();
    let cy = rect.center_y();
    let r = rect.width.min(rect.height) / 2.0;
    ctx.set_fill_color(&tt.accent);
    ctx.begin_path();
    ctx.arc(cx, cy, r, 0.0, TAU);
    ctx.fill();
    if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.set_global_alpha(0.3);
        ctx.begin_path();
        ctx.arc(cx, cy, r, 0.0, TAU);
        ctx.fill();
        ctx.set_global_alpha(1.0);
    }
    ctx.set_fill_color(contrasting_text_color(&tt.accent));
    ctx.set_font("600 15px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("\u{27A4}", cx, cy + 0.5);
    coord.register_on_layer(widget_id, rect, Sense::CLICK | Sense::HOVER, layer_id);
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

/// The day separator — a small rounded pill, centred (concept `.day`).
fn draw_day_separator(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, label: &str, tt: &ToolbarTheme) -> f64 {
    ctx.set_font("600 10px sans-serif");
    let text_w = ctx.measure_text(label);
    let pill_h = 16.0;
    let pill_w = text_w + 20.0;
    let pill_x = x + (w - pill_w) / 2.0;
    let pill_y = y + (DAY_SEP_H - pill_h) / 2.0;
    ctx.set_fill_color(&tt.button_bg);
    ctx.fill_rounded_rect(pill_x, pill_y, pill_w, pill_h, pill_h / 2.0);
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, x + w / 2.0, y + DAY_SEP_H / 2.0);
    DAY_SEP_H
}

/// The unread separator bar (concept `.unreadbar`) — full-width, centred
/// label.
fn draw_unread_separator(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, tt: &ToolbarTheme) -> f64 {
    ctx.set_fill_color(&tt.button_bg);
    ctx.fill_rect(x, y, w, UNREAD_SEP_H);
    ctx.set_font("11.5px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Unread messages", x + w / 2.0, y + UNREAD_SEP_H / 2.0);
    UNREAD_SEP_H
}

/// A forwarded item's own attribution line (concept's own forward marker —
/// no dedicated CSS class in v1, drawn the same small-muted-line idiom the
/// day separator/system notice already use) — see [`crate::
/// ForwardedFrom`]'s own doc for the attribution rule this string must
/// never weaken.
const FORWARDED_LABEL_H: f64 = 15.0;

fn forwarded_label(f: &mlc_messenger_model::ForwardedFrom) -> String {
    match f {
        mlc_messenger_model::ForwardedFrom::Hidden => "Forwarded".to_string(),
        mlc_messenger_model::ForwardedFrom::Channel { room_name } => format!("Forwarded from {room_name}"),
    }
}

/// A bubble's own content-block heights, BEFORE the trailing meta row —
/// the forwarded marker, name/rank header, reply quote, shared-chart card,
/// and the wrapped text itself — shared by the caller's height math and
/// [`draw_bubble`]'s own drawing pass so the two can never disagree.
fn bubble_content_h(line_count: usize, has_forwarded: bool, has_header: bool, has_quote: bool, has_card: bool) -> f64 {
    let mut h = 0.0;
    if has_forwarded {
        h += FORWARDED_LABEL_H;
    }
    if has_header {
        h += NAME_HEADER_H;
    }
    if has_quote {
        h += QUOTE_H;
    }
    if has_card {
        h += chart_card_height();
    }
    h += line_count as f64 * BUBBLE_LINE_H;
    h
}

fn chart_card_height() -> f64 {
    CARD_MARGIN_TOP + CARD_CHART_H + 1.0 + CARD_INFO_H + CARD_MARGIN_BOTTOM
}

#[allow(clippy::too_many_arguments)]
fn draw_bubble(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    msg_idx: u16,
    item: &mlc_messenger_model::TimelineItem,
    mine: bool,
    is_first_in_group: bool,
    is_last_in_group: bool,
    show_header: bool,
    sender_display: &str,
    sender_rank: Option<RankBadge>,
    vault_state: VaultState,
    caps: mlc_messenger_model::Capabilities,
    selection: Option<Selection>,
    tt: &ToolbarTheme,
    result: &mut CloudOverlayResult,
) -> f64 {
    let visual = content_visual(&item.content, vault_state);
    let line_count = visual.lines.len().max(1);
    let quote = item.reply_to.as_ref();
    let card = item.shared_chart.as_ref();

    let content_h = bubble_content_h(line_count, item.forwarded.is_some(), show_header, quote.is_some(), card.is_some());
    let reactions_h = if item.reactions.is_empty() { 0.0 } else { REACTIONS_ROW_H };
    let bubble_inner_h = content_h + BUBBLE_TIME_H + reactions_h;
    let bubble_h = bubble_inner_h + BUBBLE_PAD_V * 2.0;

    let reserves_avatar = !mine;
    let avatar_reserved = if reserves_avatar { BUBBLE_AVATAR_D + BUBBLE_AVATAR_GAP } else { 0.0 };
    let card_w = card.map(|_| CARD_W + BUBBLE_PAD_H * 2.0);
    let bubble_w = card_w
        .map(|cw| cw.min((w - avatar_reserved).max(BUBBLE_MIN_W)).max(BUBBLE_MIN_W))
        .unwrap_or_else(|| bubble_width_for((w - avatar_reserved).max(0.0)));
    let bx = if mine { x + w - bubble_w } else { x + avatar_reserved };

    if !mine && is_last_in_group {
        let avatar_cx = x + BUBBLE_AVATAR_D / 2.0;
        let avatar_cy = y + bubble_h / 2.0;
        draw_avatar_placeholder(ctx, avatar_cx, avatar_cy, BUBBLE_AVATAR_D / 2.0, sender_display, &tt.item_bg_active);
    }

    let mine_text = contrasting_text_color(&tt.bubble_user_bg);
    ctx.set_fill_color(if mine { &tt.bubble_user_bg } else { &tt.button_bg });
    let (tl, tr, br, bl) = bubble_corners_grouped(mine, is_first_in_group);
    fill_rect_corners(ctx, bx, y, bubble_w, bubble_h, tl, tr, br, bl);
    if item.mentions_me {
        ctx.set_stroke_color(&tt.accent);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect_corners(bx, y, bubble_w, bubble_h, tl, tr, br, bl);
    }

    let text_left = bx + BUBBLE_PAD_H;
    let text_right = bx + bubble_w - BUBBLE_PAD_H;
    let mut cy = y + BUBBLE_PAD_V;

    if let Some(f) = &item.forwarded {
        ctx.set_font("italic 11px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        let label = forwarded_label(f);
        let fit = crate::ui::widgets::fit_text_to_width(ctx, &label, (text_right - text_left).max(0.0));
        ctx.fill_text(&fit, text_left, cy);
        cy += FORWARDED_LABEL_H;
    }

    if show_header {
        ctx.set_font("600 12px sans-serif");
        ctx.set_fill_color(&tt.item_text);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        let fit = crate::ui::widgets::fit_text_to_width(ctx, sender_display, (text_right - text_left).max(0.0));
        ctx.fill_text(&fit, text_left, cy);
        if let Some(rank) = sender_rank {
            let name_w = ctx.measure_text(&fit);
            draw_rank_chip(ctx, text_left + name_w + 6.0, cy, text_right, rank_label(rank), tt);
        }
        cy += NAME_HEADER_H;
    }

    if let Some(q) = quote {
        draw_quote(ctx, text_left, cy, text_right - text_left, q, tt);
        cy += QUOTE_H;
    }

    if let Some(c) = card {
        draw_chart_card(ctx, text_left, cy, (text_right - text_left).min(CARD_W), c, tt);
        cy += chart_card_height();
    }

    let (body_font, body_color): (&str, &str) = if visual.mono {
        ("11px monospace", tt.item_text_muted.as_str())
    } else if mine {
        ("12px sans-serif", mine_text)
    } else {
        ("12px sans-serif", tt.item_text.as_str())
    };
    ctx.set_font(body_font);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let block = msg_idx as i64;
    let hit_start = result.dm_message_line_rects.len();
    for (i, line) in visual.lines.iter().enumerate() {
        let line_y = cy + i as f64 * BUBBLE_LINE_H;
        if visual.mono || mine {
            ctx.set_fill_color(body_color);
            ctx.fill_text(line, text_left, line_y);
        } else {
            draw_line_with_tokens(
                ctx, coord, layer_id, line, text_left, line_y, msg_idx, i as u16, body_font, body_color, &tt.accent, tt,
            );
        }
        result.dm_message_line_rects.push(LineHit {
            block,
            line: i as u16,
            y_top: line_y,
            y_bot: line_y + BUBBLE_LINE_H,
            text: line.clone(),
            text_x: text_left,
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

    // Clock + send-state tick (own messages) or a lock glyph (locked
    // ciphertext) — bottom-right INSIDE the bubble, 10.5px mono (owner
    // spec). A double tick for "read" only draws when the provider
    // actually reports read receipts (`Capabilities::receipts`) AND
    // `read_by` is non-empty.
    ctx.set_font("10.5px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    let time_y = cy + line_count as f64 * BUBBLE_LINE_H;
    let clock = clock_hm(item.ts);
    let mut label = if item.edited { format!("edited {clock}") } else { clock };
    if mine && !visual.mono {
        let tick = match &item.send_state {
            SendState::LocalEcho | SendState::Sending => "\u{2022}",
            SendState::Failed { .. } => "!",
            SendState::Sent if caps.receipts && !item.read_by.is_empty() => "\u{2713}\u{2713}",
            SendState::Sent => "\u{2713}",
        };
        label = format!("{label} {tick}");
    }
    let time_x = bx + bubble_w - BUBBLE_PAD_H;
    ctx.set_text_align(TextAlign::Right);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(&label, time_x, time_y);
    if visual.mono {
        let label_w = ctx.measure_text(&label);
        draw_lock_glyph(ctx, time_x - label_w - 7.0, time_y + 5.0, 8.0, &tt.item_text_muted);
    }

    if !item.reactions.is_empty() {
        draw_reactions_row(ctx, text_left, time_y + BUBBLE_TIME_H, &item.reactions, tt);
    }

    // Hovered-bubble quick-action bar (concept `.qbar`) — same "read
    // `is_hovered` against last frame, THEN register this frame's own
    // rect" order every other hoverable row in this file already uses
    // (e.g. `draw_rail_row`). `redacted`/`Legacy` items (own "nothing left
    // to act on" content — the concept never draws hover chrome over a
    // deleted/pre-migration message) never get one, same "system content,
    // not a live message" rule the day/unread separators already draw
    // from.
    if !item.redacted && !matches!(item.content, MessageContent::Legacy(_)) {
        let bubble_rect = WidgetRect::new(bx, y, bubble_w, bubble_h);
        let bubble_id = format!("cloud_overlay:dm:bubble:{}", item.event_id.as_str());
        if coord.is_hovered(&WidgetId::from(bubble_id.as_str())) {
            let can_forward = card.map(|c| c.allow_forward).unwrap_or(true);
            draw_quick_action_bar(ctx, coord, layer_id, bx, y, bubble_w, item.event_id.as_str(), can_forward, tt);
        }
        coord.register_on_layer(bubble_id.as_str(), bubble_rect, Sense::HOVER, layer_id);
    }

    bubble_h + BUBBLE_GAP
}

/// The hovered-bubble quick-action bar (concept `.qbar`) — Reply/React/
/// Forward/More, floating at the bubble's own top-right corner. React and
/// More are inert this pass (registered for hover only, same convention
/// [`draw_header_icons`]'s own doc uses) — a reaction picker is its own
/// slice (U3), and "more" has no defined action set in the concept beyond
/// what the row/header context menu already covers. Forward is omitted
/// entirely (not just disabled) when `can_forward` is false — a share
/// link's own `allow_forward == false` means there is no forward action to
/// show at all, matching [`crate::SharedChartCard::allow_forward`]'s own
/// doc.
#[allow(clippy::too_many_arguments)]
fn draw_quick_action_bar(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    bubble_x: f64,
    bubble_y: f64,
    bubble_w: f64,
    event_id: &str,
    can_forward: bool,
    tt: &ToolbarTheme,
) {
    let buttons: &[&str] = if can_forward { &["reply", "react", "forward", "more"] } else { &["reply", "react", "more"] };
    let bar_w = buttons.len() as f64 * QBAR_BTN_W + (buttons.len().saturating_sub(1)) as f64 * QBAR_GAP + QBAR_PAD * 2.0;
    let bar_h = QBAR_BTN_H + QBAR_PAD * 2.0;
    let bar_x = bubble_x + bubble_w - bar_w + QBAR_OFFSET_RIGHT;
    let bar_y = bubble_y + QBAR_OFFSET_TOP;

    ctx.set_fill_color(&tt.dropdown_bg);
    ctx.fill_rounded_rect(bar_x, bar_y, bar_w, bar_h, 6.0);
    ctx.set_stroke_color(&tt.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(bar_x, bar_y, bar_w, bar_h, 6.0);

    let mut bx = bar_x + QBAR_PAD;
    for name in buttons {
        let rect = WidgetRect::new(bx, bar_y + QBAR_PAD, QBAR_BTN_W, QBAR_BTN_H);
        let id = format!("cloud_overlay:dm:{name}:{event_id}");
        let hovered = coord.is_hovered(&WidgetId::from(id.as_str()));
        if hovered {
            ctx.set_fill_color(&tt.item_bg_hover);
            ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
        }
        let color = &tt.item_text_muted;
        match *name {
            "reply" => draw_icon_reply(ctx, rect.center_x(), rect.center_y(), 8.0, color),
            "react" => draw_icon_smile(ctx, rect.center_x(), rect.center_y(), 7.5, color),
            "forward" => draw_icon_forward(ctx, rect.center_x(), rect.center_y(), 8.0, color),
            _ => draw_icon_more(ctx, rect.center_x(), rect.center_y(), 7.0, color),
        }
        coord.register_on_layer(id.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
        bx += QBAR_BTN_W + QBAR_GAP;
    }
}

/// A reply-quote block inside a bubble (concept `.quote`) — the quoted
/// sender's own display name (resolved by the caller; a raw opaque id when
/// no matching member exists) and its preview text, single line, elided.
fn draw_quote(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, quote: &ReplyPreview, tt: &ToolbarTheme) -> f64 {
    ctx.set_fill_color(&tt.item_bg_hover);
    ctx.fill_rounded_rect(x, y, w, QUOTE_H - 4.0, 4.0);
    ctx.set_fill_color(&tt.accent);
    ctx.fill_rect(x, y, QUOTE_BAR_W, QUOTE_H - 4.0);

    let text_x = x + QUOTE_BAR_W + QUOTE_PAD_H;
    let text_w = (w - QUOTE_BAR_W - QUOTE_PAD_H * 2.0).max(0.0);
    ctx.set_font("600 11.5px sans-serif");
    ctx.set_fill_color(&tt.accent);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let fit_sender = crate::ui::widgets::fit_text_to_width(ctx, quote.sender.as_str(), text_w);
    ctx.fill_text(&fit_sender, text_x, y + QUOTE_PAD_V);

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    let fit_preview = crate::ui::widgets::fit_text_to_width(ctx, &quote.preview, text_w);
    ctx.fill_text(&fit_preview, text_x, y + QUOTE_PAD_V + 13.0);

    QUOTE_H
}

/// The shared-chart-card attachment (concept `.card`) — a bordered card
/// with a schematic candlestick pattern, a symbol/timeframe/venue title,
/// the sender's own note, and Open/Clone buttons. `--up`/`--down` have no
/// dedicated [`ToolbarTheme`] field (those live on the chart-specific theme
/// this file never otherwise touches) — [`ToolbarTheme::success`]/`danger`
/// stand in, the same teal/red pairing the Dark preset already uses for
/// both concepts.
fn draw_chart_card(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, card: &SharedChartCard, tt: &ToolbarTheme) -> f64 {
    let top = y + CARD_MARGIN_TOP;
    let total_h = CARD_CHART_H + 1.0 + CARD_INFO_H;
    ctx.set_fill_color(&tt.background);
    ctx.fill_rounded_rect(x, top, w, total_h, CARD_RADIUS);
    ctx.set_stroke_color(&tt.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, top, w, total_h, CARD_RADIUS);

    draw_chart_card_candles(ctx, x, top, w, CARD_CHART_H, tt);

    let info_y = top + CARD_CHART_H + 1.0;
    draw_soft_divider_h(ctx, x, info_y, w, tt);

    let label_open = "Open";
    let label_clone = "Clone";
    ctx.set_font("600 11.5px sans-serif");
    let open_w = ctx.measure_text(label_open) + 16.0;
    let clone_w = ctx.measure_text(label_clone) + 16.0;
    let btn_h = 24.0;
    let btn_y = info_y + (CARD_INFO_H - btn_h) / 2.0;
    let clone_rect = WidgetRect::new(x + w - 9.0 - clone_w, btn_y, clone_w, btn_h);
    let open_rect = WidgetRect::new(clone_rect.x - 6.0 - open_w, btn_y, open_w, btn_h);

    ctx.set_fill_color(&tt.button_bg);
    ctx.fill_rounded_rect(open_rect.x, open_rect.y, open_rect.width, open_rect.height, 4.0);
    ctx.set_stroke_color(&tt.divider_soft);
    ctx.stroke_rounded_rect(open_rect.x, open_rect.y, open_rect.width, open_rect.height, 4.0);
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label_open, open_rect.center_x(), open_rect.center_y());

    ctx.set_fill_color(&tt.accent);
    ctx.fill_rounded_rect(clone_rect.x, clone_rect.y, clone_rect.width, clone_rect.height, 4.0);
    ctx.set_fill_color(contrasting_text_color(&tt.accent));
    ctx.fill_text(label_clone, clone_rect.center_x(), clone_rect.center_y());

    let text_x = x + 9.0;
    let text_right = (open_rect.x - 8.0).max(text_x);
    ctx.set_font("600 12px sans-serif");
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let title = format!("{} · {} · {}", card.symbol, card.timeframe, card.venue);
    let fit_title = crate::ui::widgets::fit_text_to_width(ctx, &title, (text_right - text_x).max(0.0));
    ctx.fill_text(&fit_title, text_x, info_y + 6.0);

    ctx.set_font("11px monospace");
    ctx.set_fill_color(&tt.muted_ink);
    let fit_note = crate::ui::widgets::fit_text_to_width(ctx, &card.note, (text_right - text_x).max(0.0));
    ctx.fill_text(&fit_note, text_x, info_y + 21.0);

    chart_card_height()
}

/// A deterministic, schematic candlestick pattern — illustrative content
/// only (this is not real market data), a pure function of the slot index
/// so it never allocates a `rand` dependency this crate does not otherwise
/// need.
fn draw_chart_card_candles(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, h: f64, tt: &ToolbarTheme) {
    let n = 18;
    let slot_w = w / n as f64;
    for i in 0..n {
        let phase = i as f64 * 0.62;
        let seed = phase.sin();
        let up = seed >= 0.0;
        let body_h = (h * 0.16) + seed.abs() * (h * 0.32);
        let cy = y + h * 0.55 - (phase * 0.7).sin() * (h * 0.18);
        let bx = x + slot_w * i as f64 + slot_w * 0.3;
        let bw = (slot_w * 0.4).max(2.0);
        let color: &str = if up { tt.success.as_str() } else { tt.danger.as_str() };
        ctx.set_stroke_color(color);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(bx + bw / 2.0, cy - body_h / 2.0 - 3.0);
        ctx.line_to(bx + bw / 2.0, cy + body_h / 2.0 + 3.0);
        ctx.stroke();
        ctx.set_fill_color(color);
        ctx.fill_rect(bx, cy - body_h / 2.0, bw, body_h);
    }
}

/// A single reactions row (concept `.rx`) — one small pill per reaction,
/// accent-outlined and filled with [`ToolbarTheme::selection`] when
/// [`ReactionSummary::by_me`], plain-bordered otherwise.
fn draw_reactions_row(ctx: &mut dyn RenderContext, x: f64, y: f64, reactions: &[ReactionSummary], tt: &ToolbarTheme) -> f64 {
    let h = 18.0;
    let mut cx = x;
    for r in reactions {
        let label = format!("{} {}", r.key, r.count);
        ctx.set_font("11.5px sans-serif");
        let text_w = ctx.measure_text(&label);
        let pill_w = text_w + 14.0;
        if r.by_me {
            ctx.set_fill_color(&tt.selection);
            ctx.fill_rounded_rect(cx, y, pill_w, h, h / 2.0);
            ctx.set_stroke_color(&tt.accent);
        } else {
            ctx.set_fill_color(&tt.background);
            ctx.fill_rounded_rect(cx, y, pill_w, h, h / 2.0);
            ctx.set_stroke_color(&tt.divider_soft);
        }
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(cx, y, pill_w, h, h / 2.0);
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&label, cx + pill_w / 2.0, y + h / 2.0 + 0.5);
        cx += pill_w + 4.0;
    }
    h + 4.0
}

/// One classified run of `line`'s own text — a plain span, an `@mention`
/// token, or a `$SYMBOL` cashtag token (concept `.sym`'s own inline ticker
/// pill, owner order 2026-09-25: "the ticker is a clickable pill") — each
/// carrying its EXACT source text (including the leading `@`/`$`), so
/// re-joining every token's own text reproduces `line` verbatim.
#[derive(Clone, Debug, PartialEq)]
enum TextToken {
    Plain(String),
    Mention(String),
    /// A `$SYMBOL` cashtag — only a `$` immediately followed by an ASCII
    /// UPPERCASE letter starts one, so a dollar amount like `$64,150`
    /// (always digits right after the `$`) is never mistaken for a ticker.
    Symbol(String),
}

impl TextToken {
    fn text(&self) -> &str {
        match self {
            TextToken::Plain(s) | TextToken::Mention(s) | TextToken::Symbol(s) => s,
        }
    }
}

/// Split `line` into [`TextToken`]s — a generalisation of the old
/// `mention_segments` (same round-trip guarantee, same word-boundary rule)
/// that also recognises `$SYMBOL` cashtags. Pure, so [`draw_line_with_tokens`]
/// and its own unit tests share one definition of "what counts as a token".
fn text_tokens(line: &str) -> Vec<TextToken> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mode {
        Plain,
        Mention,
        Symbol,
    }
    let chars: Vec<char> = line.chars().collect();
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut mode = Mode::Plain;
    let mut at_word_start = true;
    for (i, &c) in chars.iter().enumerate() {
        if mode == Mode::Plain && at_word_start && c == '@' {
            if !current.is_empty() {
                tokens.push(TextToken::Plain(std::mem::take(&mut current)));
            }
            mode = Mode::Mention;
        } else if mode == Mode::Plain
            && at_word_start
            && c == '$'
            && chars.get(i + 1).is_some_and(|n| n.is_ascii_uppercase())
        {
            if !current.is_empty() {
                tokens.push(TextToken::Plain(std::mem::take(&mut current)));
            }
            mode = Mode::Symbol;
        } else if c.is_whitespace() && mode != Mode::Plain {
            let finished = std::mem::take(&mut current);
            tokens.push(match mode {
                Mode::Mention => TextToken::Mention(finished),
                Mode::Symbol => TextToken::Symbol(finished),
                Mode::Plain => unreachable!(),
            });
            mode = Mode::Plain;
        }
        current.push(c);
        at_word_start = c.is_whitespace();
    }
    if !current.is_empty() {
        tokens.push(match mode {
            Mode::Plain => TextToken::Plain(current),
            Mode::Mention => TextToken::Mention(current),
            Mode::Symbol => TextToken::Symbol(current),
        });
    }
    tokens
}

/// A `$SYMBOL` token's own display/click text — the source text with its
/// leading `$` stripped (the pill shows `BTCUSDT`, never `$BTCUSDT`, same
/// as the concept's own `.sym` span content).
fn symbol_display(token_text: &str) -> &str {
    token_text.strip_prefix('$').unwrap_or(token_text)
}

/// Draw `line` left-aligned at `(x, y)`, colouring `@mention` tokens in
/// `accent_color` and rendering `$SYMBOL` tokens as a clickable pill
/// (concept `.sym`) — plain-text bubble lines only (mono/ciphertext lines
/// never call this; own messages never call this either — see this
/// function's own call site in `draw_bubble`). `msg_idx`/`line_idx`
/// disambiguate the SAME symbol appearing more than once on screen — the
/// symbol itself is always the registered widget id's own trailing
/// segment, parsed back by `mlc-app`'s `panel_click.rs` `"symbol:"` arm.
#[allow(clippy::too_many_arguments)]
fn draw_line_with_tokens(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    line: &str,
    x: f64,
    y: f64,
    msg_idx: u16,
    line_idx: u16,
    body_font: &str,
    normal_color: &str,
    accent_color: &str,
    tt: &ToolbarTheme,
) {
    let mut cx = x;
    ctx.set_font(body_font);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    for tok in text_tokens(line) {
        match tok {
            TextToken::Plain(_) => {
                ctx.set_font(body_font);
                ctx.set_fill_color(normal_color);
                ctx.fill_text(tok.text(), cx, y);
                cx += ctx.measure_text(tok.text());
            }
            TextToken::Mention(_) => {
                ctx.set_font(body_font);
                ctx.set_fill_color(accent_color);
                ctx.fill_text(tok.text(), cx, y);
                cx += ctx.measure_text(tok.text());
            }
            TextToken::Symbol(_) => {
                let symbol = symbol_display(tok.text());
                ctx.set_font(SYMBOL_PILL_FONT);
                let text_w = ctx.measure_text(symbol);
                let pill_w = text_w + SYMBOL_PILL_PAD_H * 2.0;
                let pill_h = BUBBLE_LINE_H - 2.0;
                let pill_y = y + 1.0;
                let widget_id = format!("cloud_overlay:dm:symbol:{msg_idx}:{line_idx}:{symbol}");
                let hovered = coord.is_hovered(&WidgetId::from(widget_id.as_str()));
                ctx.set_fill_color(&tt.item_bg_hover);
                ctx.fill_rounded_rect(cx, pill_y, pill_w, pill_h, SYMBOL_PILL_RADIUS);
                if hovered {
                    ctx.set_stroke_color(&tt.accent);
                    ctx.set_stroke_width(1.0);
                    ctx.stroke_rounded_rect(cx, pill_y, pill_w, pill_h, SYMBOL_PILL_RADIUS);
                }
                ctx.set_fill_color(&tt.item_text);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Top);
                ctx.fill_text(symbol, cx + SYMBOL_PILL_PAD_H, y);
                let rect = WidgetRect::new(cx, pill_y, pill_w, pill_h);
                coord.register_on_layer(widget_id.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
                cx += pill_w;
                ctx.set_font(body_font);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Top);
            }
        }
    }
}

/// A bubble's own max width — 72% of whatever width is left after an
/// avatar reservation (own messages reserve none, see [`draw_bubble`]'s own
/// call site), never narrower than [`BUBBLE_MIN_W`]. Pulled out as a pure
/// function so geometry tests can prove it never overflows the pane at any
/// width in the target range without needing a `RenderContext`.
fn bubble_width_for(available_w: f64) -> f64 {
    (available_w * BUBBLE_WIDTH_FRAC).max(BUBBLE_MIN_W)
}

/// A bubble's own corner radii `(tl, tr, br, bl)` — [`BUBBLE_RADIUS`]
/// everywhere except the "tail" corner nearest its own avatar-less edge
/// (bottom-right for an own message, bottom-left for a peer's — concept
/// `.b.own`/`.b.peer`), and ALSO the corner nearest the group's own
/// PREVIOUS bubble when this one is not the first in its group (concept
/// `.b.own.cont`/`.b.peer.mid`: the top corner on that same side flattens
/// too, so consecutive bubbles read as one continuous shape).
fn bubble_corners_grouped(mine: bool, is_first_in_group: bool) -> (f64, f64, f64, f64) {
    if mine {
        let tr = if is_first_in_group { BUBBLE_RADIUS } else { BUBBLE_TAIL_RADIUS };
        (BUBBLE_RADIUS, tr, BUBBLE_TAIL_RADIUS, BUBBLE_RADIUS)
    } else {
        let tl = if is_first_in_group { BUBBLE_RADIUS } else { BUBBLE_TAIL_RADIUS };
        (tl, BUBBLE_RADIUS, BUBBLE_RADIUS, BUBBLE_TAIL_RADIUS)
    }
}

/// Fill a rect with four independent corner radii — `rounded_rect_corners`
/// is a path primitive (like `rect`/`arc`), not a one-shot fill helper the
/// way `fill_rounded_rect` is, so this wraps the same
/// `begin_path`/`…`/`fill` sequence every other multi-step shape in this
/// file already uses (e.g. [`draw_icon_all_chats`]).
#[allow(clippy::too_many_arguments)]
fn fill_rect_corners(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, h: f64, tl: f64, tr: f64, br: f64, bl: f64) {
    ctx.begin_path();
    ctx.rounded_rect_corners(x, y, w, h, tl, tr, br, bl);
    ctx.fill();
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

/// A short weekday name for `day` (days since the Unix epoch) — 1970-01-01
/// (day 0) was a Thursday, so the table starts there and cycles every 7.
fn weekday_short(day: i64) -> &'static str {
    const NAMES: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    NAMES[day.rem_euclid(7) as usize]
}

fn month_short(m: i64) -> &'static str {
    const NAMES: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    NAMES[(m - 1).rem_euclid(12) as usize]
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
    fn weekday_short_of_the_epoch_is_thursday() {
        assert_eq!(weekday_short(0), "Thu");
        assert_eq!(weekday_short(7), "Thu");
        assert_eq!(weekday_short(3), "Sun");
    }

    #[test]
    fn row_time_label_picks_clock_this_week_or_date_by_age() {
        let now = 10 * 86_400_000_u64 + 12 * 3_600_000; // day 10, noon (clear of the midnight boundary).
        let recent = Timestamp(now as i64 - 3_600_000); // same day, 1h ago.
        assert_eq!(row_time_label(recent, now), clock_hm(recent));
        // 2 days ago -> a short weekday name (day 8).
        assert_eq!(row_time_label(Timestamp(8 * 86_400_000), now), weekday_short(8));
        // Beyond the "this week" window -> a date (day -10, well before day 10).
        let (_, m, d) = civil_from_days(-10);
        assert_eq!(row_time_label(Timestamp(-10 * 86_400_000), now), format!("{d} {}", month_short(m)));
    }

    #[test]
    fn rail_item_is_centred_inside_the_rail_with_a_4px_margin_either_side() {
        let margin = (RAIL_W - RAIL_ITEM_W) / 2.0;
        assert_eq!(margin, 4.0);
    }

    #[test]
    fn rail_active_bar_sits_flush_with_the_rails_own_left_edge() {
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
        assert_eq!(SEARCH_FIELD_RADIUS, SEARCH_FIELD_H / 2.0);
    }

    #[test]
    fn bubble_corners_first_in_group_matches_the_plain_own_peer_shape() {
        assert_eq!(bubble_corners_grouped(true, true), (BUBBLE_RADIUS, BUBBLE_RADIUS, BUBBLE_TAIL_RADIUS, BUBBLE_RADIUS));
        assert_eq!(bubble_corners_grouped(false, true), (BUBBLE_RADIUS, BUBBLE_RADIUS, BUBBLE_RADIUS, BUBBLE_TAIL_RADIUS));
    }

    #[test]
    fn bubble_corners_continuation_flattens_the_near_top_corner() {
        assert_eq!(bubble_corners_grouped(true, false), (BUBBLE_RADIUS, BUBBLE_TAIL_RADIUS, BUBBLE_TAIL_RADIUS, BUBBLE_RADIUS));
        assert_eq!(bubble_corners_grouped(false, false), (BUBBLE_TAIL_RADIUS, BUBBLE_RADIUS, BUBBLE_RADIUS, BUBBLE_TAIL_RADIUS));
    }

    #[test]
    fn bubble_never_overflows_the_pane_across_the_target_width_range() {
        for w in (470..=700).step_by(10) {
            let w = w as f64;
            let own_w = bubble_width_for(w);
            assert!(own_w <= w, "own bubble {own_w} overflows pane width {w}");

            let avatar_reserved = BUBBLE_AVATAR_D + BUBBLE_AVATAR_GAP;
            let peer_w = bubble_width_for((w - avatar_reserved).max(0.0));
            assert!(
                avatar_reserved + peer_w <= w,
                "peer bubble {peer_w} + avatar {avatar_reserved} overflows pane width {w}"
            );
        }
    }

    #[test]
    fn bubble_width_never_collapses_below_the_minimum() {
        assert!(bubble_width_for(0.0) >= BUBBLE_MIN_W);
        assert!(bubble_width_for(50.0) >= BUBBLE_MIN_W);
    }

    #[test]
    fn short_fingerprint_takes_16_hex_chars_grouped_in_4s() {
        assert_eq!(short_fingerprint("3f9d5445e06b50e3"), "3F9D 5445 E06B 50E3");
        assert_eq!(short_fingerprint("3f:9d:54:45:e0:6b:50:e3:ff"), "3F9D 5445 E06B 50E3");
        assert_eq!(short_fingerprint("abcd"), "ABCD");
    }

    #[test]
    fn chat_header_height_matches_the_concept() {
        assert_eq!(CHAT_HEADER_H, 52.0);
    }

    #[test]
    fn composer_h_is_padding_plus_one_36px_row() {
        assert_eq!(COMPOSER_H, COMPOSER_PAD_TOP + COMPOSER_FIELD_H + COMPOSER_PAD_BOTTOM);
        assert_eq!(COMPOSER_H, 53.0);
    }

    #[test]
    fn composer_field_and_send_button_share_the_same_row_height() {
        assert_eq!(COMPOSER_FIELD_H, COMPOSER_SEND_D);
    }

    #[test]
    fn composer_field_is_fully_rounded() {
        assert_eq!(COMPOSER_FIELD_RADIUS, COMPOSER_FIELD_H / 2.0);
    }

    #[test]
    fn dock_h_uses_the_compact_row_footprint_when_no_nick_gate_applies() {
        let state = CommunityState::default();
        assert_eq!(dock_h(&state, None), COMPOSER_H);
    }

    #[test]
    fn dock_h_is_the_channel_cta_row_when_the_viewer_cannot_post() {
        let state = CommunityState::default();
        let gate = ChannelGate { can_post: false, muted: false };
        assert_eq!(dock_h(&state, Some(gate)), CHANNEL_CTA_H);
    }

    #[test]
    fn dock_h_falls_through_to_the_normal_composer_when_a_channel_admin_can_post() {
        let state = CommunityState::default();
        let gate = ChannelGate { can_post: true, muted: false };
        assert_eq!(dock_h(&state, Some(gate)), COMPOSER_H);
    }

    #[test]
    fn dock_btn_is_compact_not_a_full_row() {
        assert!(DOCK_BTN_H < ROW_H);
    }

    #[test]
    fn text_tokens_round_trip_to_the_original_line() {
        let line = "@admin закрепил правила — глянь";
        let tokens = text_tokens(line);
        let rebuilt: String = tokens.iter().map(TextToken::text).collect();
        assert_eq!(rebuilt, line);
        assert!(matches!(tokens[0], TextToken::Mention(_)), "first token must be the @mention token");
    }

    #[test]
    fn text_tokens_ignores_an_email_like_token_mid_word() {
        // "@" only starts a mention at a WORD boundary — mid-word it is just
        // a literal character, same as the concept's own `.mention` rule
        // (only ever applied to a leading `@handle`).
        let line = "user@name text";
        let tokens = text_tokens(line);
        assert!(tokens.iter().all(|t| !matches!(t, TextToken::Mention(_))));
    }

    #[test]
    fn text_tokens_recognises_a_cashtag_but_not_a_dollar_amount() {
        let line = "на 4h ордерблок $BTCUSDT 64 150–64 420";
        let tokens = text_tokens(line);
        let rebuilt: String = tokens.iter().map(TextToken::text).collect();
        assert_eq!(rebuilt, line);
        assert!(tokens.iter().any(|t| matches!(t, TextToken::Symbol(s) if s == "$BTCUSDT")));

        // A dollar AMOUNT (digit right after `$`) is never a cashtag.
        let price_line = "target $64,150 today";
        let price_tokens = text_tokens(price_line);
        assert!(price_tokens.iter().all(|t| !matches!(t, TextToken::Symbol(_))));
    }

    #[test]
    fn symbol_display_strips_the_leading_dollar_sign() {
        assert_eq!(symbol_display("$BTCUSDT"), "BTCUSDT");
        assert_eq!(symbol_display("BTCUSDT"), "BTCUSDT");
    }

    #[test]
    fn unread_badge_muted_is_true_for_a_muted_room_or_any_channel() {
        let mut r = test_room(RoomKind::Group, false, 0);
        assert!(!unread_badge_muted(&r));
        r.muted = true;
        assert!(unread_badge_muted(&r));
        let ch = test_room(RoomKind::Channel, false, 0);
        assert!(unread_badge_muted(&ch));
    }

    #[test]
    fn trailing_marker_on_the_open_row_never_shows_unread_or_mention() {
        let mut r = test_room(RoomKind::Group, false, 5);
        r.highlight_count = 1;
        r.pinned = true;
        assert!(matches!(trailing_marker(&r, true), Trailing::Pin));
        assert!(matches!(trailing_marker(&r, false), Trailing::Mention));
    }

    #[test]
    fn chart_card_height_is_the_sum_of_its_own_blocks() {
        assert_eq!(chart_card_height(), CARD_MARGIN_TOP + CARD_CHART_H + 1.0 + CARD_INFO_H + CARD_MARGIN_BOTTOM);
    }

    fn test_room(kind: RoomKind, muted: bool, notification_count: u64) -> RoomSummary {
        use mlc_messenger_model::{MemberRole, PowerLevel};
        RoomSummary {
            id: RoomId::from("!r:x"),
            kind,
            name: "x".to_string(),
            initials: "X".to_string(),
            is_encrypted: true,
            is_public: false,
            notification_count,
            highlight_count: 0,
            last_event: None,
            muted,
            pinned: false,
            archived: false,
            tags: Vec::new(),
            typing_user_ids: Vec::new(),
            peer_last_active: None,
            my_role: MemberRole::Member,
            my_power_level: PowerLevel(0),
            can_post: true,
        }
    }

    fn test_item(id: &str, content: MessageContent) -> mlc_messenger_model::TimelineItem {
        use mlc_messenger_model::{EventId, UserId};
        mlc_messenger_model::TimelineItem {
            event_id: EventId::from(id),
            sender: UserId::from("u"),
            sender_rank: None,
            ts: Timestamp(0),
            content,
            reply_to: None,
            edited: false,
            reactions: Vec::new(),
            redacted: false,
            send_state: SendState::Sent,
            read_by: Vec::new(),
            pinned: false,
            shared_chart: None,
            mentions_me: false,
            forwarded: None,
        }
    }

    #[test]
    fn unread_start_counts_message_items_only_and_skips_system_notices() {
        let timeline = vec![
            test_item("a", MessageContent::Text("old".to_string())),
            test_item("b", MessageContent::Text("unread 1".to_string())),
            test_item("c", MessageContent::KeyChanged),
            test_item("d", MessageContent::Text("unread 2".to_string())),
        ];
        // Two unread messages: the run starts at "b", the notice between the
        // two messages is inside the run and does not eat into the count.
        assert_eq!(unread_start(&timeline, 2), 1);
        assert_eq!(unread_start(&timeline, 1), 3);
        assert_eq!(unread_start(&timeline, 0), timeline.len());
        // More unread than items held: the run starts at the top.
        assert_eq!(unread_start(&timeline, 99), 0);
    }

    #[test]
    fn archived_rooms_are_listed_only_under_the_archive_folder() {
        let mut archived = test_room(RoomKind::Group, false, 0);
        archived.archived = true;
        let live = test_room(RoomKind::Group, false, 0);
        for sel in [
            MessengerFolderSel::All,
            MessengerFolderSel::Personal,
            MessengerFolderSel::Groups,
            MessengerFolderSel::Channels,
            MessengerFolderSel::Custom("u.trading-desk".to_string()),
        ] {
            assert!(!folder_includes(&sel, &[], &archived), "{sel:?} must hide an archived room");
        }
        assert!(folder_includes(&MessengerFolderSel::Archive, &[], &archived));
        assert!(!folder_includes(&MessengerFolderSel::Archive, &[], &live));
        assert!(folder_includes(&MessengerFolderSel::All, &[], &live));
        assert!(folder_includes(&MessengerFolderSel::Groups, &[], &live));
        assert!(!folder_includes(&MessengerFolderSel::Channels, &[], &live));
    }

    #[test]
    fn a_custom_folder_lists_a_room_through_its_own_tag_rule() {
        use mlc_messenger_model::{FolderExclude, FolderInclude, Tag, TagName};
        let folder = Folder {
            id: Some(TagName::from("u.trading-desk")),
            name: "Trading desk".to_string(),
            icon: FolderIcon::Candles,
            include: FolderInclude::default(),
            exclude: FolderExclude::default(),
            order: 1.0,
        };
        let sel = MessengerFolderSel::Custom("u.trading-desk".to_string());
        let mut room = test_room(RoomKind::Direct, false, 0);
        assert!(!folder_includes(&sel, std::slice::from_ref(&folder), &room));
        room.tags.push(Tag { name: TagName::from("u.trading-desk"), order: 1.0 });
        assert!(folder_includes(&sel, std::slice::from_ref(&folder), &room));
    }

    #[test]
    fn folder_selection_round_trips_through_its_widget_key() {
        for sel in [
            MessengerFolderSel::All,
            MessengerFolderSel::Personal,
            MessengerFolderSel::Groups,
            MessengerFolderSel::Channels,
            MessengerFolderSel::Archive,
            MessengerFolderSel::Custom("u.research".to_string()),
        ] {
            assert_eq!(MessengerFolderSel::from_widget_key(&sel.widget_key()), Some(sel));
        }
        assert_eq!(MessengerFolderSel::from_widget_key("folders"), None);
        assert_eq!(MessengerFolderSel::from_widget_key("tag:"), None);
    }
}
