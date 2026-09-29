//! [`SnapshotProvider`] -- the one [`mlc_messenger_model::MessengerProvider`]
//! this crate ships, and the seam between a shell's I/O driver and the UI.
//!
//! # Shape (single-writer core: snapshot out, intents in)
//!
//! The UI holds a clone of [`SnapshotProvider`] and only ever reads the
//! latest published [`ProviderView`] and enqueues intents. The shell's I/O
//! driver owns the [`MessengerCore`] (the single writer) and, on every tick:
//!
//! 1. [`SnapshotProvider::take_intents`] -> [`apply_intents`] into the core;
//! 2. drives HTTP and the flush barrier (the driver's own job);
//! 3. when [`MessengerCore::change_counter`] moved, [`build_view`] ->
//!    [`SnapshotProvider::publish`].
//!
//! Both halves share one `Arc<Mutex<..>>`. A lock is held only for a clone,
//! a push or a swap, never across I/O or across [`build_view`] itself, so the
//! UI thread never waits on the network. The type is `Clone + Send + Sync`,
//! so one type serves both the native (multi-threaded) and the web
//! (single-threaded) shell.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use mlc_messenger_model::{
    initials_from_name, BackPagination, Capabilities, CreateRoomRequest, EventId, Folder, FolderExclude, FolderIcon,
    FolderInclude, LastEventPreview, LegacyContent, Member, MemberRole, MessageContent, MessengerIntent,
    MessengerProvider, MembershipChange, MembershipEvent, PowerLevel, PreviewContent, PublicRoomEntry,
    ReactionSummary, ReplyPreview, RoomId, RoomKind, RoomSummary, RoomVisibility, SendState, Tag, TagName, Timestamp,
    TimelineItem, UserEntry, UserId, UtdReason, VaultState,
};
use serde::{Deserialize, Serialize};

use crate::core::{CreateRoomKind, MessageKind, MessengerCommand, MessengerCore, OutgoingMessage};
use crate::error::MessengerError;
use crate::room::timeline::{
    ItemContent, SendState as CoreSendState, StateChangeSummary, Timeline, TimelineItem as CoreTimelineItem,
};
use crate::room::{MemberRole as CoreMemberRole, RoomKind as CoreRoomKind, RoomState};
use crate::store::RecordCodec;
use crate::wire::events::{JoinRule, Membership, RelatesTo, RoomMessageContent, TagContent};

/// What the engine behind this provider can do in the first release slice
/// (owner reorder 2026-09-25: ship what works end to end, switch the rest
/// off). `folders` turns on once the tag mapping in [`build_view`] and
/// [`apply_intents`] lands.
pub const SLICE_CAPABILITIES: Capabilities = Capabilities {
    groups: true,
    channels: true,
    folders: true,
    reactions: true,
    edits: true,
    replies: true,
    typing: true,
    receipts: true,
    forward: false,
    stickers: false,
    chart_card_send: false,
};

/// `m.tag`'s pinned-room tag name (client-server API convention).
const FAVOURITE_TAG: &str = "m.favourite";
/// This slice's own tag name for "archived" (built-in folder), per P1/P2's
/// design ruling: archived is a room's own tag, not membership state.
const ARCHIVED_TAG: &str = "u.mlc.archived";
/// This slice's own room-scoped account-data type for "muted"
/// (`{"muted": bool}`).
const MUTED_ACCOUNT_DATA_TYPE: &str = "org.mylittlechart.muted";
/// This slice's own GLOBAL account-data type carrying every custom folder
/// this account has defined (`{"folders": [...]}` -- [`FoldersAccountData`]'s
/// own doc).
const FOLDERS_ACCOUNT_DATA_TYPE: &str = "org.mylittlechart.folders";

/// Everything the UI reads, built in one pass from the core by
/// [`build_view`] and swapped in whole by [`SnapshotProvider::publish`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProviderView {
    /// Every room this account holds, in no particular order (chat-list
    /// order is the UI's job).
    pub rooms: Vec<RoomSummary>,
    /// Each room's currently-held timeline, oldest first.
    pub timelines: BTreeMap<RoomId, Vec<TimelineItem>>,
    /// Each room's back-pagination state.
    pub back_pagination: BTreeMap<RoomId, BackPagination>,
    /// Each room's members.
    pub members: BTreeMap<RoomId, Vec<Member>>,
    /// Built-in folders first, in display order, then custom folders by
    /// their own `order`.
    pub folders: Vec<Folder>,
    /// The latest public-room directory result set.
    pub public_rooms: Vec<PublicRoomEntry>,
    /// The latest user-directory result set.
    pub user_search: Vec<UserEntry>,
}

struct Shared {
    view: Arc<ProviderView>,
    change_counter: u64,
    capabilities: Capabilities,
    vault_state: VaultState,
    last_error: Option<String>,
    intents: Vec<MessengerIntent>,
}

/// The UI-facing half (implements [`MessengerProvider`]) and the driver-facing
/// half ([`Self::take_intents`], [`Self::publish`], [`Self::set_vault_state`],
/// [`Self::set_last_error`]) of one shared slot. Cloning is cheap and every
/// clone sees the same state.
#[derive(Clone)]
pub struct SnapshotProvider {
    shared: Arc<Mutex<Shared>>,
}

impl SnapshotProvider {
    /// A provider with an empty view, [`VaultState::Locked`] (the state the UI
    /// shows while the driver is still booting) and no error.
    pub fn new(capabilities: Capabilities) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                view: Arc::new(ProviderView::default()),
                change_counter: 0,
                capabilities,
                vault_state: VaultState::Locked,
                last_error: None,
                intents: Vec::new(),
            })),
        }
    }

    /// A poisoned lock only means some thread panicked while holding it;
    /// every write here is a single assignment or push, so the data is
    /// still consistent and the UI keeps working.
    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn view(&self) -> Arc<ProviderView> {
        Arc::clone(&self.lock().view)
    }

    /// Driver half: every intent the UI enqueued since the last call, in
    /// enqueue order.
    pub fn take_intents(&self) -> Vec<MessengerIntent> {
        std::mem::take(&mut self.lock().intents)
    }

    /// Driver half: swaps in a freshly built view and bumps the change
    /// counter.
    pub fn publish(&self, view: ProviderView) {
        let mut shared = self.lock();
        shared.view = Arc::new(view);
        shared.change_counter += 1;
    }

    /// Driver half: boot progress / lock state. Bumps the change counter
    /// only when the state actually changes.
    pub fn set_vault_state(&self, state: VaultState) {
        let mut shared = self.lock();
        if shared.vault_state != state {
            shared.vault_state = state;
            shared.change_counter += 1;
        }
    }

    /// Driver half: the one error slot. Bumps the change counter only when
    /// the value actually changes.
    pub fn set_last_error(&self, error: Option<String>) {
        let mut shared = self.lock();
        if shared.last_error != error {
            shared.last_error = error;
            shared.change_counter += 1;
        }
    }
}

impl MessengerProvider for SnapshotProvider {
    fn change_counter(&self) -> u64 {
        self.lock().change_counter
    }

    fn capabilities(&self) -> Capabilities {
        self.lock().capabilities
    }

    fn vault_state(&self) -> VaultState {
        self.lock().vault_state
    }

    fn last_error(&self) -> Option<String> {
        self.lock().last_error.clone()
    }

    fn rooms(&self) -> Vec<RoomSummary> {
        self.view().rooms.clone()
    }

    fn room(&self, room: &RoomId) -> Option<RoomSummary> {
        self.view().rooms.iter().find(|summary| &summary.id == room).cloned()
    }

    fn timeline(&self, room: &RoomId) -> Vec<TimelineItem> {
        self.view().timelines.get(room).cloned().unwrap_or_default()
    }

    fn back_pagination(&self, room: &RoomId) -> BackPagination {
        self.view().back_pagination.get(room).cloned().unwrap_or_default()
    }

    fn members(&self, room: &RoomId) -> Vec<Member> {
        self.view().members.get(room).cloned().unwrap_or_default()
    }

    fn folders(&self) -> Vec<Folder> {
        self.view().folders.clone()
    }

    fn public_rooms(&self) -> Vec<PublicRoomEntry> {
        self.view().public_rooms.clone()
    }

    fn user_search(&self) -> Vec<UserEntry> {
        self.view().user_search.clone()
    }

    fn enqueue(&mut self, intent: MessengerIntent) {
        self.lock().intents.push(intent);
    }
}

/// Builds the UI view from the core's current state -- called by the driver
/// only when [`MessengerCore::change_counter`] moved. `now_ms` stays
/// unread: this slice reports no presence at all ([`RoomSummary::
/// peer_last_active`] is always `None`), so nothing here is time-sensitive
/// (the same "accepted for API symmetry, not read" shape [`MessengerCore::
/// open`]'s own `_now_ms` already has).
pub fn build_view<C: RecordCodec>(core: &MessengerCore<C>, _now_ms: i64) -> ProviderView {
    let self_user_id = core.user_id().clone();
    let room_ids: Vec<crate::ids::RoomId> = core.room_ids().cloned().collect();

    let mut rooms = Vec::with_capacity(room_ids.len());
    let mut timelines = BTreeMap::new();
    let mut back_pagination = BTreeMap::new();
    let mut members = BTreeMap::new();

    for room_id in &room_ids {
        if let Some(summary) = build_room_summary(core, room_id, &self_user_id) {
            rooms.push(summary);
        }
        let model_room_id = RoomId::from(room_id.as_str());
        if let Some(timeline) = core.timeline(room_id) {
            timelines.insert(model_room_id.clone(), build_timeline_items(core, room_id, timeline, &self_user_id));
            back_pagination.insert(model_room_id.clone(), build_back_pagination(timeline));
        }
        if let Some(state) = core.room_state(room_id) {
            members.insert(model_room_id, build_members(state, core));
        }
    }

    ProviderView {
        rooms,
        timelines,
        back_pagination,
        members,
        folders: build_folders(core),
        public_rooms: build_public_rooms(core),
        user_search: build_user_search(core),
    }
}

// ---------------------------------------------------------------------
// RoomSummary
// ---------------------------------------------------------------------

fn build_room_summary<C: RecordCodec>(
    core: &MessengerCore<C>,
    room_id: &crate::ids::RoomId,
    self_user_id: &crate::ids::UserId,
) -> Option<RoomSummary> {
    let state = core.room_state(room_id)?;
    let kind = core.room_kind(room_id).unwrap_or(CoreRoomKind::Group);

    let name = room_display_name(state, kind, self_user_id);
    let initials = initials_from_name(&name);

    let is_encrypted = state.encryption.is_some();
    let is_public = matches!(state.join_rules.as_ref().map(|rules| &rules.join_rule), Some(JoinRule::Public));

    let last_event = compute_last_event(core.timeline(room_id));

    let muted = core
        .room_account_data(room_id, MUTED_ACCOUNT_DATA_TYPE)
        .and_then(|value| value.get("muted"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    let all_tags = current_tags(core, room_id);
    let pinned = all_tags.iter().any(|tag| tag.name.as_str() == FAVOURITE_TAG);
    let archived = all_tags.iter().any(|tag| tag.name.as_str() == ARCHIVED_TAG);
    let tags: Vec<Tag> =
        all_tags.into_iter().filter(|tag| tag.name.as_str().starts_with("u.") && tag.name.as_str() != ARCHIVED_TAG).collect();

    let typing_user_ids: Vec<UserId> = core
        .typing_users(room_id)
        .iter()
        .filter(|user| *user != self_user_id)
        .map(|user| UserId::from(user.as_str()))
        .collect();

    let is_joined =
        state.members.get(self_user_id).map(|member| member.membership == Membership::Join).unwrap_or(false);
    let my_role = if kind == CoreRoomKind::Channel && !is_joined {
        MemberRole::Subscriber
    } else {
        map_member_role(state.member_role(self_user_id))
    };
    let my_power_level = PowerLevel(state.power_level_for(self_user_id));
    let post_level = state.power_levels.events.get("m.room.message").copied().unwrap_or(state.power_levels.events_default);
    // Only a joined member can post at all (an invite, or a not-yet-joined
    // channel, cannot), whatever the power-level table says.
    let can_post = is_joined && my_power_level.0 >= post_level;

    Some(RoomSummary {
        id: RoomId::from(room_id.as_str()),
        kind: map_room_kind(kind),
        name,
        initials,
        is_encrypted,
        is_public,
        notification_count: state.unread_notifications.notification_count,
        highlight_count: state.unread_notifications.highlight_count,
        last_event,
        muted,
        pinned,
        archived,
        tags,
        typing_user_ids,
        peer_last_active: None,
        my_role,
        my_power_level,
        can_post,
    })
}

fn current_tags<C: RecordCodec>(core: &MessengerCore<C>, room_id: &crate::ids::RoomId) -> Vec<Tag> {
    core.room_account_data(room_id, "m.tag")
        .and_then(|value| serde_json::from_value::<TagContent>(value.clone()).ok())
        .map(|content| {
            content.tags.into_iter().map(|(name, info)| Tag { name: TagName::from(name), order: info.order.unwrap_or(0.0) }).collect()
        })
        .unwrap_or_default()
}

/// How many other members' names an unnamed group/channel lists before
/// collapsing the rest into " +N".
const UNNAMED_ROOM_NAME_MEMBERS: usize = 3;

fn room_display_name(state: &RoomState, kind: CoreRoomKind, self_user_id: &crate::ids::UserId) -> String {
    if let Some(name) = &state.name {
        return name.clone();
    }
    if kind == CoreRoomKind::Dm {
        if let Some((peer_id, peer_state)) = state.members.iter().find(|(user_id, _)| *user_id != self_user_id) {
            return peer_state.displayname.clone().unwrap_or_else(|| localpart(peer_id));
        }
        return "Direct".to_string();
    }
    // No `m.room.name`: the joined members' own names, self excluded.
    let others: Vec<String> = state
        .members
        .iter()
        .filter(|(user_id, member)| *user_id != self_user_id && member.membership == Membership::Join)
        .map(|(user_id, member)| member.displayname.clone().unwrap_or_else(|| localpart(user_id)))
        .collect();
    if others.is_empty() {
        return match kind {
            CoreRoomKind::Channel => "Channel",
            _ => "Group",
        }
        .to_string();
    }
    let mut name = others.iter().take(UNNAMED_ROOM_NAME_MEMBERS).cloned().collect::<Vec<_>>().join(", ");
    if others.len() > UNNAMED_ROOM_NAME_MEMBERS {
        name.push_str(&format!(" +{}", others.len() - UNNAMED_ROOM_NAME_MEMBERS));
    }
    name
}

fn map_room_kind(kind: CoreRoomKind) -> RoomKind {
    match kind {
        CoreRoomKind::Dm => RoomKind::Direct,
        CoreRoomKind::Group => RoomKind::Group,
        CoreRoomKind::Channel => RoomKind::Channel,
    }
}

fn map_member_role(role: CoreMemberRole) -> MemberRole {
    match role {
        CoreMemberRole::Owner => MemberRole::Owner,
        CoreMemberRole::Admin => MemberRole::Admin,
        CoreMemberRole::Member => MemberRole::Member,
    }
}

fn localpart(user_id: &crate::ids::UserId) -> String {
    user_id
        .as_str()
        .strip_prefix('@')
        .and_then(|rest| rest.split_once(':'))
        .map(|(local, _)| local.to_string())
        .unwrap_or_else(|| user_id.as_str().to_string())
}

fn compute_last_event(timeline: Option<&Timeline>) -> Option<LastEventPreview> {
    let timeline = timeline?;
    timeline.items().iter().rev().find_map(|item| {
        message_preview_content(&item.content).map(|content| LastEventPreview {
            sender: UserId::from(item.sender.as_str()),
            content,
            ts: Timestamp(item.origin_server_ts),
        })
    })
}

fn message_preview_content(content: &ItemContent) -> Option<PreviewContent> {
    match content {
        ItemContent::Text(inner) => Some(PreviewContent::Text(inner.body.clone())),
        ItemContent::Notice(inner) => Some(PreviewContent::Notice(inner.body.clone())),
        ItemContent::Emote(inner) => Some(PreviewContent::Text(format!("/me {}", inner.body))),
        ItemContent::Sticker { alt, .. } => Some(PreviewContent::Text(alt.clone())),
        ItemContent::ChartCard { body, .. } => Some(PreviewContent::Text(body.clone())),
        ItemContent::LegacyPlain(text) => Some(PreviewContent::Text(text.clone())),
        ItemContent::Encrypted { .. }
        | ItemContent::Undecryptable { .. }
        | ItemContent::LegacyDm { .. }
        | ItemContent::LegacyDmUndecryptable { .. } => Some(PreviewContent::Encrypted),
        ItemContent::Redacted => Some(PreviewContent::Redacted),
        ItemContent::StateChange(_) | ItemContent::Unknown => None,
    }
}

// ---------------------------------------------------------------------
// Members
// ---------------------------------------------------------------------

fn build_members<C: RecordCodec>(state: &RoomState, core: &MessengerCore<C>) -> Vec<Member> {
    state
        .members
        .iter()
        .filter(|(_, member_state)| matches!(member_state.membership, Membership::Join | Membership::Invite))
        .map(|(user_id, member_state)| {
            let display_name = member_state.displayname.clone().unwrap_or_else(|| localpart(user_id));
            Member {
                user_id: UserId::from(user_id.as_str()),
                initials: initials_from_name(&display_name),
                display_name,
                rank: None,
                role: map_member_role(state.member_role(user_id)),
                power_level: PowerLevel(state.power_level_for(user_id)),
                online: false,
                last_active: None,
                has_messaging_key: core.has_tracked_device(user_id),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------
// Timeline
// ---------------------------------------------------------------------

fn build_timeline_items<C: RecordCodec>(
    core: &MessengerCore<C>,
    room_id: &crate::ids::RoomId,
    timeline: &Timeline,
    self_user_id: &crate::ids::UserId,
) -> Vec<TimelineItem> {
    let read_by_map = compute_read_by(core, room_id, timeline);
    timeline
        .items()
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| map_timeline_item(core, timeline, item, self_user_id, idx, &read_by_map))
        .collect()
}

/// For every event id this room has a `m.read` receipt naming, every
/// timeline index at or before that receipt's own position gets that
/// user in its own `read_by` set -- see [`crate::MessengerCore::receipts`]'s
/// own doc for the wire shape this reads.
fn compute_read_by<C: RecordCodec>(
    core: &MessengerCore<C>,
    room_id: &crate::ids::RoomId,
    timeline: &Timeline,
) -> BTreeMap<usize, Vec<crate::ids::UserId>> {
    let Some(receipts) = core.receipts(room_id) else { return BTreeMap::new() };
    let mut positions: BTreeMap<crate::ids::EventId, usize> = BTreeMap::new();
    for (idx, item) in timeline.items().iter().enumerate() {
        if let Some(event_id) = &item.event_id {
            positions.insert(event_id.clone(), idx);
        }
    }
    let mut result: BTreeMap<usize, BTreeSet<crate::ids::UserId>> = BTreeMap::new();
    for (target_event_id, by_type) in receipts.0.iter() {
        let Some(readers) = by_type.get("m.read") else { continue };
        let Some(&target_idx) = positions.get(target_event_id) else { continue };
        for user_id in readers.keys() {
            for idx in 0..=target_idx {
                result.entry(idx).or_default().insert(user_id.clone());
            }
        }
    }
    result.into_iter().map(|(idx, set)| (idx, set.into_iter().collect())).collect()
}

fn map_timeline_item<C: RecordCodec>(
    core: &MessengerCore<C>,
    timeline: &Timeline,
    item: &CoreTimelineItem,
    self_user_id: &crate::ids::UserId,
    item_index: usize,
    read_by_map: &BTreeMap<usize, Vec<crate::ids::UserId>>,
) -> Option<TimelineItem> {
    let (content, edited) = match &item.relations.latest_edit {
        Some(edit) => (room_message_content_to_model(&edit.content)?, true),
        None => (item_content_to_model(&item.content)?, false),
    };

    let event_id = match (&item.event_id, &item.txn_id) {
        (Some(confirmed), _) => EventId::from(confirmed.as_str()),
        (None, Some(txn)) => EventId::from(txn.as_str()),
        (None, None) => return None,
    };

    let reactions: Vec<ReactionSummary> = item
        .relations
        .reactions
        .iter()
        .map(|(key, users)| ReactionSummary { key: key.clone(), count: users.len() as u32, by_me: users.contains(self_user_id) })
        .collect();

    let reply_to = item.relations.reply_to.as_ref().map(|target| build_reply_preview(timeline, target));

    let read_by: Vec<UserId> = read_by_map
        .get(&item_index)
        .map(|users| users.iter().map(|user| UserId::from(user.as_str())).collect())
        .unwrap_or_default();

    Some(TimelineItem {
        event_id,
        sender: UserId::from(item.sender.as_str()),
        sender_rank: None,
        ts: Timestamp(item.origin_server_ts),
        content,
        reply_to,
        edited,
        reactions,
        redacted: item.redacted,
        send_state: map_send_state(core, item),
        read_by,
        pinned: false,
        shared_chart: None,
        mentions_me: false,
        forwarded: None,
    })
}

fn build_reply_preview(timeline: &Timeline, target: &crate::ids::EventId) -> ReplyPreview {
    match timeline.item_by_event_id(target) {
        Some(target_item) => {
            let preview =
                item_content_to_model(&target_item.content).and_then(|content| first_line_preview(&content)).unwrap_or_else(|| "…".to_string());
            ReplyPreview { event_id: EventId::from(target.as_str()), sender: UserId::from(target_item.sender.as_str()), preview }
        }
        None => ReplyPreview { event_id: EventId::from(target.as_str()), sender: UserId::from(""), preview: "…".to_string() },
    }
}

fn first_line_preview(content: &MessageContent) -> Option<String> {
    let body = match content {
        MessageContent::Text(body) | MessageContent::Notice(body) => body.as_str(),
        MessageContent::Legacy(LegacyContent::Text(body)) => body.as_str(),
        _ => return None,
    };
    Some(body.lines().next().unwrap_or_default().to_string())
}

fn map_send_state<C: RecordCodec>(core: &MessengerCore<C>, item: &CoreTimelineItem) -> SendState {
    match &item.send_state {
        CoreSendState::LocalEcho => SendState::LocalEcho,
        CoreSendState::Sending => SendState::Sending,
        CoreSendState::Sent => SendState::Sent,
        CoreSendState::Failed { reason } => {
            let resolved = item.txn_id.as_ref().and_then(|txn| core.send_failure_reason(txn)).map(str::to_string).unwrap_or_else(|| reason.clone());
            SendState::Failed { reason: resolved }
        }
    }
}

fn room_message_content_to_model(content: &RoomMessageContent) -> Option<MessageContent> {
    match content {
        RoomMessageContent::Text(inner) => Some(MessageContent::Text(inner.body.clone())),
        RoomMessageContent::Notice(inner) => Some(MessageContent::Notice(inner.body.clone())),
        RoomMessageContent::Emote(inner) => Some(MessageContent::Text(format!("/me {}", inner.body))),
        RoomMessageContent::Unknown(_) => None,
    }
}

fn item_content_to_model(content: &ItemContent) -> Option<MessageContent> {
    match content {
        ItemContent::Text(inner) => Some(MessageContent::Text(inner.body.clone())),
        ItemContent::Notice(inner) => Some(MessageContent::Notice(inner.body.clone())),
        ItemContent::Emote(inner) => Some(MessageContent::Text(format!("/me {}", inner.body))),
        ItemContent::Sticker { pack, id, alt } => {
            Some(MessageContent::Sticker { pack: pack.clone(), id: id.clone(), alt: alt.clone() })
        }
        ItemContent::ChartCard { body, .. } => Some(MessageContent::Text(body.clone())),
        ItemContent::Encrypted { ciphertext_b64, .. } => Some(MessageContent::Encrypted { ciphertext_b64: ciphertext_b64.clone() }),
        ItemContent::Undecryptable { reason } => Some(MessageContent::Undecryptable(map_utd_reason(reason))),
        ItemContent::LegacyDm { ciphertext_b64, .. } | ItemContent::LegacyDmUndecryptable { ciphertext_b64, .. } => {
            Some(MessageContent::Legacy(LegacyContent::Encrypted { ciphertext_b64: ciphertext_b64.clone() }))
        }
        ItemContent::LegacyPlain(text) => Some(MessageContent::Legacy(LegacyContent::Text(text.clone()))),
        ItemContent::Redacted => Some(MessageContent::Text(String::new())),
        ItemContent::StateChange(StateChangeSummary::Member { user_id, membership, .. }) => {
            map_membership_change(membership).map(|change| MessageContent::Membership(MembershipEvent { user: UserId::from(user_id.as_str()), change }))
        }
        ItemContent::StateChange(_) | ItemContent::Unknown => None,
    }
}

fn map_utd_reason(reason: &str) -> UtdReason {
    match reason {
        "MissingSession" => UtdReason::MissingSession,
        "WithheldBlacklisted" => UtdReason::WithheldBlacklisted,
        "WithheldUnverified" => UtdReason::WithheldUnverified,
        "WithheldUnauthorised" => UtdReason::WithheldUnauthorised,
        "SenderIdentityNotVerified" => UtdReason::SenderIdentityNotVerified,
        _ => UtdReason::Unknown,
    }
}

fn map_membership_change(membership: &Membership) -> Option<MembershipChange> {
    match membership {
        Membership::Join => Some(MembershipChange::Joined),
        Membership::Leave => Some(MembershipChange::Left),
        Membership::Invite => Some(MembershipChange::Invited),
        Membership::Ban => Some(MembershipChange::Removed),
        Membership::Knock | Membership::Unknown => None,
    }
}

fn build_back_pagination(timeline: &Timeline) -> BackPagination {
    let token = timeline
        .older_token()
        .map(mlc_messenger_model::PaginationToken::from)
        .or_else(|| timeline.gap().and_then(|gap| gap.prev_batch.as_deref()).map(mlc_messenger_model::PaginationToken::from));
    let has_more = token.is_some();
    BackPagination { token, has_more }
}

// ---------------------------------------------------------------------
// Folders
// ---------------------------------------------------------------------

/// Mirrors `org.mylittlechart.folders` global account data
/// (`{"folders": [...]}`) -- `mlc-messenger-model` stays a dependency-free
/// data crate (its own module doc), so this crate defines its own serde
/// shape for the wire representation rather than deriving `Serialize`/
/// `Deserialize` on the model's own [`Folder`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct FoldersAccountData {
    #[serde(default)]
    folders: Vec<CustomFolderData>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CustomFolderData {
    id: String,
    name: String,
    #[serde(default = "default_folder_icon")]
    icon: String,
    #[serde(default)]
    include: CustomFolderIncludeData,
    #[serde(default)]
    exclude: CustomFolderExcludeData,
    order: f64,
}

fn default_folder_icon() -> String {
    "folder".to_string()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct CustomFolderIncludeData {
    #[serde(default)]
    all_dms: bool,
    #[serde(default)]
    all_groups: bool,
    #[serde(default)]
    all_channels: bool,
    #[serde(default)]
    rooms: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct CustomFolderExcludeData {
    #[serde(default)]
    muted: bool,
    #[serde(default)]
    read: bool,
    #[serde(default)]
    archived: bool,
}

fn folder_icon_to_str(icon: FolderIcon) -> &'static str {
    match icon {
        FolderIcon::Candles => "candles",
        FolderIcon::Folder => "folder",
        FolderIcon::Group => "group",
        FolderIcon::Channel => "channel",
        FolderIcon::Bookmark => "bookmark",
        FolderIcon::Person => "person",
    }
}

fn folder_icon_from_str(value: &str) -> FolderIcon {
    match value {
        "candles" => FolderIcon::Candles,
        "group" => FolderIcon::Group,
        "channel" => FolderIcon::Channel,
        "bookmark" => FolderIcon::Bookmark,
        "person" => FolderIcon::Person,
        _ => FolderIcon::Folder,
    }
}

fn custom_folder_from_data(data: &CustomFolderData) -> Folder {
    Folder {
        id: Some(TagName::from(data.id.as_str())),
        name: data.name.clone(),
        icon: folder_icon_from_str(&data.icon),
        include: FolderInclude {
            all_dms: data.include.all_dms,
            all_groups: data.include.all_groups,
            all_channels: data.include.all_channels,
            rooms: data.include.rooms.iter().map(|room_id| RoomId::from(room_id.as_str())).collect(),
        },
        exclude: FolderExclude { muted: data.exclude.muted, read: data.exclude.read, archived: data.exclude.archived },
        order: data.order,
    }
}

fn custom_folder_to_data(folder: &Folder, tag: &TagName) -> CustomFolderData {
    CustomFolderData {
        id: tag.as_str().to_string(),
        name: folder.name.clone(),
        icon: folder_icon_to_str(folder.icon).to_string(),
        include: CustomFolderIncludeData {
            all_dms: folder.include.all_dms,
            all_groups: folder.include.all_groups,
            all_channels: folder.include.all_channels,
            rooms: folder.include.rooms.iter().map(|room_id| room_id.as_str().to_string()).collect(),
        },
        exclude: CustomFolderExcludeData { muted: folder.exclude.muted, read: folder.exclude.read, archived: folder.exclude.archived },
        order: folder.order,
    }
}

fn build_folders<C: RecordCodec>(core: &MessengerCore<C>) -> Vec<Folder> {
    let mut folders = vec![
        Folder {
            id: None,
            name: "All chats".to_string(),
            icon: FolderIcon::Folder,
            include: FolderInclude { all_dms: true, all_groups: true, all_channels: true, rooms: Vec::new() },
            exclude: FolderExclude { muted: false, read: false, archived: true },
            order: 0.0,
        },
        Folder {
            id: None,
            name: "Personal".to_string(),
            icon: FolderIcon::Person,
            include: FolderInclude { all_dms: true, ..Default::default() },
            exclude: FolderExclude::default(),
            order: 1.0,
        },
        Folder {
            id: None,
            name: "Groups".to_string(),
            icon: FolderIcon::Group,
            include: FolderInclude { all_groups: true, ..Default::default() },
            exclude: FolderExclude::default(),
            order: 2.0,
        },
        Folder {
            id: None,
            name: "Channels".to_string(),
            icon: FolderIcon::Channel,
            include: FolderInclude { all_channels: true, ..Default::default() },
            exclude: FolderExclude::default(),
            order: 3.0,
        },
        // `Folder::matches` cannot express "every archived room, of any
        // kind" -- no `include` rule pulls in every kind at once except by
        // also pulling in everything else, and no `exclude` rule can
        // FORCE-include an archived room. This row is a placeholder the
        // UI must special-case (filter directly on `RoomSummary::archived`)
        // rather than calling `Folder::matches` for it -- see this crate's
        // own P2 handoff report.
        Folder {
            id: None,
            name: "Archive".to_string(),
            icon: FolderIcon::Bookmark,
            include: FolderInclude::default(),
            exclude: FolderExclude::default(),
            order: 4.0,
        },
    ];

    if let Some(value) = core.global_account_data(FOLDERS_ACCOUNT_DATA_TYPE) {
        if let Ok(data) = serde_json::from_value::<FoldersAccountData>(value.clone()) {
            folders.extend(data.folders.iter().map(custom_folder_from_data));
        }
    }
    folders
}

fn current_folders_data<C: RecordCodec>(core: &MessengerCore<C>) -> FoldersAccountData {
    core.global_account_data(FOLDERS_ACCOUNT_DATA_TYPE).and_then(|value| serde_json::from_value(value.clone()).ok()).unwrap_or_default()
}

fn write_folders_data<C: RecordCodec>(core: &mut MessengerCore<C>, data: FoldersAccountData, now_ms: i64) -> Result<(), MessengerError> {
    let value = serde_json::to_value(&data)?;
    core.dispatch(MessengerCommand::SetAccountData { event_type: FOLDERS_ACCOUNT_DATA_TYPE.to_string(), content: value }, now_ms)
}

fn validate_custom_folder_tag(tag: &TagName) -> Result<(), MessengerError> {
    let name = tag.as_str();
    if !name.starts_with("u.") || name == ARCHIVED_TAG {
        return Err(MessengerError::IntentRefused(format!("invalid folder id: {name}")));
    }
    Ok(())
}

fn room_has_tag<C: RecordCodec>(core: &MessengerCore<C>, room_id: &crate::ids::RoomId, tag: &str) -> bool {
    core.room_account_data(room_id, "m.tag")
        .and_then(|value| serde_json::from_value::<TagContent>(value.clone()).ok())
        .map(|content| content.tags.contains_key(tag))
        .unwrap_or(false)
}

/// Shared body of `MessengerIntent::CreateFolder`/`UpdateFolder`: a
/// read-modify-write of `org.mylittlechart.folders`, plus tagging every
/// room newly named in `folder.include.rooms` with this folder's own tag
/// (untagging every room dropped from that list since the previous
/// version, if any) -- "creating one is tagging the rooms it should hold"
/// ([`mlc_messenger_model::Folder`]'s own doc), and the reason
/// [`MessengerIntent::DeleteFolder`] can later find every room to untag
/// purely from the rooms' own `m.tag` state.
fn apply_upsert_folder<C: RecordCodec>(core: &mut MessengerCore<C>, folder: &Folder, now_ms: i64) -> Result<(), MessengerError> {
    let tag = folder.id.as_ref().ok_or_else(|| MessengerError::IntentRefused("folder must have an id".to_string()))?;
    validate_custom_folder_tag(tag)?;

    let mut data = current_folders_data(core);
    let previous_rooms: Vec<String> =
        data.folders.iter().find(|entry| entry.id == tag.as_str()).map(|entry| entry.include.rooms.clone()).unwrap_or_default();

    let new_entry = custom_folder_to_data(folder, tag);
    if let Some(existing) = data.folders.iter_mut().find(|entry| entry.id == tag.as_str()) {
        *existing = new_entry.clone();
    } else {
        data.folders.push(new_entry.clone());
    }

    for room_id_str in &new_entry.include.rooms {
        if !previous_rooms.contains(room_id_str) {
            let room_id = parse_room_id_str(room_id_str)?;
            core.dispatch(MessengerCommand::SetTag { room_id, tag: tag.as_str().to_string(), order: None }, now_ms)?;
        }
    }
    for room_id_str in &previous_rooms {
        if !new_entry.include.rooms.contains(room_id_str) {
            let room_id = parse_room_id_str(room_id_str)?;
            core.dispatch(MessengerCommand::RemoveTag { room_id, tag: tag.as_str().to_string() }, now_ms)?;
        }
    }

    write_folders_data(core, data, now_ms)
}

fn apply_delete_folder<C: RecordCodec>(core: &mut MessengerCore<C>, tag: &TagName, now_ms: i64) -> Result<(), MessengerError> {
    validate_custom_folder_tag(tag)?;
    let mut data = current_folders_data(core);
    data.folders.retain(|entry| entry.id != tag.as_str());
    write_folders_data(core, data, now_ms)?;

    let room_ids: Vec<crate::ids::RoomId> = core.room_ids().cloned().collect();
    for room_id in room_ids {
        if room_has_tag(core, &room_id, tag.as_str()) {
            core.dispatch(MessengerCommand::RemoveTag { room_id, tag: tag.as_str().to_string() }, now_ms)?;
        }
    }
    Ok(())
}

fn apply_reorder_folders<C: RecordCodec>(core: &mut MessengerCore<C>, pairs: &[(TagName, f64)], now_ms: i64) -> Result<(), MessengerError> {
    let mut data = current_folders_data(core);
    for (tag, order) in pairs {
        if let Some(entry) = data.folders.iter_mut().find(|entry| entry.id == tag.as_str()) {
            entry.order = *order;
        }
    }
    write_folders_data(core, data, now_ms)
}

// ---------------------------------------------------------------------
// Public-room / user-directory search results
// ---------------------------------------------------------------------

fn build_public_rooms<C: RecordCodec>(core: &MessengerCore<C>) -> Vec<PublicRoomEntry> {
    core.public_rooms_result()
        .iter()
        .map(|entry| PublicRoomEntry {
            room_id: RoomId::from(entry.room_id.as_str()),
            name: entry.name.clone().unwrap_or_else(|| entry.room_id.as_str().to_string()),
            topic: entry.topic.clone(),
            member_count: entry.num_joined_members,
            joined: core
                .room_state(&entry.room_id)
                .and_then(|state| state.members.get(core.user_id()))
                .map(|member| member.membership == Membership::Join)
                .unwrap_or(false),
        })
        .collect()
}

fn build_user_search<C: RecordCodec>(core: &MessengerCore<C>) -> Vec<UserEntry> {
    core.user_search_result()
        .iter()
        .map(|entry| UserEntry { user_id: UserId::from(entry.user_id.as_str()), display_name: entry.display_name.clone().unwrap_or_else(|| localpart(&entry.user_id)), rank: None })
        .collect()
}

/// Applies every drained intent to the core, in order. Returns the last
/// failure's message for [`SnapshotProvider::set_last_error`], or `None`
/// when every intent applied.
pub fn apply_intents<C: RecordCodec>(
    core: &mut MessengerCore<C>,
    intents: Vec<MessengerIntent>,
    now_ms: i64,
) -> Option<String> {
    let mut last_error = None;
    for intent in intents {
        if let Err(error) = apply_intent(core, intent, now_ms) {
            last_error = Some(error.to_string());
        }
    }
    last_error
}

/// One intent -> one or more core commands.
fn apply_intent<C: RecordCodec>(core: &mut MessengerCore<C>, intent: MessengerIntent, now_ms: i64) -> Result<(), MessengerError> {
    match intent {
        MessengerIntent::SendText { room, text, reply_to } => {
            let room_id = require_room(core, &room)?;
            let reply_to = reply_to.as_ref().map(parse_event_id).transpose()?;
            let message = OutgoingMessage { kind: MessageKind::Text, body: text, reply_to, edit_of: None };
            core.dispatch(MessengerCommand::SendMessage { room_id, message, txn_id: None }, now_ms)
        }
        MessengerIntent::Edit { room, event_id, text } => {
            let room_id = require_room(core, &room)?;
            let edit_of = parse_event_id(&event_id)?;
            let message = OutgoingMessage { kind: MessageKind::Text, body: text, reply_to: None, edit_of: Some(edit_of) };
            core.dispatch(MessengerCommand::SendMessage { room_id, message, txn_id: None }, now_ms)
        }
        MessengerIntent::Redact { room, event_id } => {
            let room_id = require_room(core, &room)?;
            let target = parse_event_id(&event_id)?;
            core.dispatch(MessengerCommand::Redact { room_id, target, reason: None }, now_ms)
        }
        MessengerIntent::React { room, event_id, key } => {
            let room_id = require_room(core, &room)?;
            let target = parse_event_id(&event_id)?;
            core.dispatch(MessengerCommand::React { room_id, target, key }, now_ms)
        }
        MessengerIntent::Unreact { room, event_id } => apply_unreact(core, &room, &event_id, now_ms),
        MessengerIntent::MarkRead { room, event_id } => {
            let room_id = require_room(core, &room)?;
            let event_id = parse_event_id(&event_id)?;
            core.dispatch(MessengerCommand::MarkRead { room_id, event_id }, now_ms)
        }
        MessengerIntent::SetTyping { room, typing } => {
            let room_id = require_room(core, &room)?;
            core.dispatch(MessengerCommand::SetTyping { room_id, typing }, now_ms)
        }
        MessengerIntent::SetMuted { room, muted } => {
            let room_id = require_room(core, &room)?;
            core.dispatch(
                MessengerCommand::SetRoomAccountData {
                    room_id,
                    event_type: MUTED_ACCOUNT_DATA_TYPE.to_string(),
                    content: serde_json::json!({ "muted": muted }),
                },
                now_ms,
            )
        }
        MessengerIntent::SetPinned { room, pinned } => {
            let room_id = require_room(core, &room)?;
            let cmd = if pinned {
                MessengerCommand::SetTag { room_id, tag: FAVOURITE_TAG.to_string(), order: None }
            } else {
                MessengerCommand::RemoveTag { room_id, tag: FAVOURITE_TAG.to_string() }
            };
            core.dispatch(cmd, now_ms)
        }
        MessengerIntent::SetArchived { room, archived } => {
            let room_id = require_room(core, &room)?;
            let cmd = if archived {
                MessengerCommand::SetTag { room_id, tag: ARCHIVED_TAG.to_string(), order: None }
            } else {
                MessengerCommand::RemoveTag { room_id, tag: ARCHIVED_TAG.to_string() }
            };
            core.dispatch(cmd, now_ms)
        }
        MessengerIntent::LoadOlder { room } => {
            let room_id = require_room(core, &room)?;
            core.dispatch(MessengerCommand::LoadOlder { room_id }, now_ms)
        }
        MessengerIntent::CreateFolder(folder) => apply_upsert_folder(core, &folder, now_ms),
        MessengerIntent::UpdateFolder(folder) => apply_upsert_folder(core, &folder, now_ms),
        MessengerIntent::DeleteFolder(tag) => apply_delete_folder(core, &tag, now_ms),
        MessengerIntent::ReorderFolders(pairs) => apply_reorder_folders(core, &pairs, now_ms),
        MessengerIntent::CreateRoom(request) => apply_create_room(core, request, now_ms),
        MessengerIntent::JoinRoom { room } => {
            let room_id = parse_room_id(&room)?;
            core.dispatch(MessengerCommand::JoinRoom { room_id }, now_ms)
        }
        MessengerIntent::LeaveRoom { room } => {
            let room_id = require_room(core, &room)?;
            core.dispatch(MessengerCommand::LeaveRoom { room_id }, now_ms)
        }
        MessengerIntent::Invite { room, user } => {
            let room_id = require_room(core, &room)?;
            let user_id = parse_user_id(&user)?;
            core.dispatch(MessengerCommand::Invite { room_id, user_id }, now_ms)
        }
        MessengerIntent::Kick { room, user } => {
            let room_id = require_room(core, &room)?;
            let user_id = parse_user_id(&user)?;
            core.dispatch(MessengerCommand::Kick { room_id, user_id, reason: None }, now_ms)
        }
        MessengerIntent::RetrySend { room, local_id } => {
            let room_id = require_room(core, &room)?;
            let txn_id = crate::ids::TxnId::from(local_id.as_str().to_string());
            core.dispatch(MessengerCommand::RetrySend { room_id, txn_id }, now_ms)
        }
        MessengerIntent::SearchPublicRooms { term } => core.dispatch(MessengerCommand::SearchPublicRooms { term }, now_ms),
        MessengerIntent::SearchUsers { term } => core.dispatch(MessengerCommand::SearchUsers { term }, now_ms),
        MessengerIntent::Forward { .. } | MessengerIntent::SendSticker { .. } => {
            Err(MessengerError::IntentRefused("not wired in this branch".to_string()))
        }
    }
}

fn parse_room_id_str(value: &str) -> Result<crate::ids::RoomId, MessengerError> {
    crate::ids::RoomId::parse(value).map_err(|_| MessengerError::IntentRefused(format!("invalid room id: {value}")))
}

fn parse_room_id(room: &RoomId) -> Result<crate::ids::RoomId, MessengerError> {
    parse_room_id_str(room.as_str())
}

fn parse_event_id(event: &EventId) -> Result<crate::ids::EventId, MessengerError> {
    crate::ids::EventId::parse(event.as_str()).map_err(|_| MessengerError::IntentRefused(format!("invalid event id: {}", event.as_str())))
}

fn parse_user_id(user: &UserId) -> Result<crate::ids::UserId, MessengerError> {
    crate::ids::UserId::parse(user.as_str()).map_err(|_| MessengerError::IntentRefused(format!("invalid user id: {}", user.as_str())))
}

/// Parses `room` and requires the core to already hold state for it --
/// every room-scoped intent except [`MessengerIntent::JoinRoom`] (the very
/// way a room BECOMES known) needs an already-known room to act on.
fn require_room<C: RecordCodec>(core: &MessengerCore<C>, room: &RoomId) -> Result<crate::ids::RoomId, MessengerError> {
    let room_id = parse_room_id(room)?;
    if core.room_state(&room_id).is_none() {
        return Err(MessengerError::IntentRefused(format!("unknown room: {}", room.as_str())));
    }
    Ok(room_id)
}

/// `MessengerIntent::Unreact` has no direct core command of its own (this
/// crate's own doc on [`crate::room::timeline::Timeline::relating_events_for`]):
/// find this account's own `m.reaction` event(s) on `target` via the
/// room's relation aggregation, then redact each. A target with no
/// reaction from this account at all is a no-op, not an error (already
/// unreacted).
fn apply_unreact<C: RecordCodec>(core: &mut MessengerCore<C>, room: &RoomId, event_id: &EventId, now_ms: i64) -> Result<(), MessengerError> {
    let room_id = require_room(core, room)?;
    let target = parse_event_id(event_id)?;
    let self_user_id = core.user_id().clone();
    let my_reactions: Vec<crate::ids::EventId> = core
        .timeline(&room_id)
        .map(|timeline| {
            timeline
                .relating_events_for(&target)
                .iter()
                .filter(|relating| relating.sender == self_user_id && matches!(relating.relation, RelatesTo::Annotation { .. }))
                .map(|relating| relating.event_id.clone())
                .collect()
        })
        .unwrap_or_default();
    for reaction_event_id in my_reactions {
        core.dispatch(MessengerCommand::Redact { room_id: room_id.clone(), target: reaction_event_id, reason: None }, now_ms)?;
    }
    Ok(())
}

fn apply_create_room<C: RecordCodec>(core: &mut MessengerCore<C>, request: CreateRoomRequest, now_ms: i64) -> Result<(), MessengerError> {
    let kind = match request.kind {
        RoomKind::Direct => {
            if request.member_ids.len() != 1 {
                return Err(MessengerError::IntentRefused("CreateRoom Direct requires exactly one member".to_string()));
            }
            let peer = parse_user_id(&request.member_ids[0])?;
            CreateRoomKind::Dm { peer }
        }
        RoomKind::Group => {
            let mut invite = Vec::with_capacity(request.member_ids.len());
            for member in &request.member_ids {
                invite.push(parse_user_id(member)?);
            }
            CreateRoomKind::Group { name: request.title.clone().unwrap_or_else(|| "Group".to_string()), invite, members_can_invite: request.members_can_add }
        }
        RoomKind::Channel => {
            if request.visibility != RoomVisibility::Public {
                return Err(MessengerError::IntentRefused("CreateRoom Channel requires Public visibility".to_string()));
            }
            let name = request.title.clone().ok_or_else(|| MessengerError::IntentRefused("CreateRoom Channel requires a title".to_string()))?;
            CreateRoomKind::Channel { name, topic: request.description.clone() }
        }
    };
    core.dispatch(MessengerCommand::CreateRoom { kind }, now_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_then_take_returns_intents_in_order_once() {
        let mut provider = SnapshotProvider::new(SLICE_CAPABILITIES);
        let room = RoomId::from("!r:x");
        provider.enqueue(MessengerIntent::LoadOlder { room: room.clone() });
        provider.enqueue(MessengerIntent::SetTyping { room: room.clone(), typing: true });
        let taken = provider.take_intents();
        assert_eq!(taken.len(), 2);
        assert_eq!(taken[0], MessengerIntent::LoadOlder { room });
        assert!(provider.take_intents().is_empty());
    }

    #[test]
    fn publish_is_seen_by_every_clone_and_bumps_the_counter() {
        let driver = SnapshotProvider::new(SLICE_CAPABILITIES);
        let ui = driver.clone();
        let before = ui.change_counter();
        let room = RoomId::from("!r:x");
        let mut view = ProviderView::default();
        view.timelines.insert(room.clone(), Vec::new());
        view.back_pagination.insert(room.clone(), BackPagination { token: None, has_more: true });
        driver.publish(view);
        assert_eq!(ui.change_counter(), before + 1);
        assert!(ui.back_pagination(&room).has_more);
        assert!(ui.rooms().is_empty());
    }

    #[test]
    fn status_setters_bump_the_counter_only_on_change() {
        let provider = SnapshotProvider::new(SLICE_CAPABILITIES);
        assert_eq!(provider.vault_state(), VaultState::Locked);
        let c0 = provider.change_counter();
        provider.set_vault_state(VaultState::Locked);
        provider.set_last_error(None);
        assert_eq!(provider.change_counter(), c0);
        provider.set_vault_state(VaultState::Ready);
        provider.set_last_error(Some("boom".to_string()));
        assert_eq!(provider.change_counter(), c0 + 2);
        assert_eq!(provider.last_error().as_deref(), Some("boom"));
    }

    // -------------------------------------------------------------
    // build_view / apply_intent
    // -------------------------------------------------------------

    use crate::core::{CoreConfig, CoreSecrets};
    use crate::outgoing_queue::Jitter;
    use crate::store::InsecurePlainCodecForTests;
    use crate::wire::{HttpResponseDescriptor, OutgoingRequest, OutgoingRequestKind};

    struct FixedJitter(f64);
    impl Jitter for FixedJitter {
        fn next_unit(&mut self) -> f64 {
            self.0
        }
    }

    fn core_user(value: &str) -> crate::ids::UserId {
        crate::ids::UserId::parse(value).expect("valid user id")
    }

    fn open_core(user_id: crate::ids::UserId, device: &str) -> MessengerCore<InsecurePlainCodecForTests> {
        let config = CoreConfig {
            user_id,
            device_id: crate::ids::DeviceId::parse(device).expect("valid device id"),
            server_name: "example.org".to_string(),
        };
        MessengerCore::open(Vec::new(), InsecurePlainCodecForTests, config, CoreSecrets::default(), 0, Box::new(FixedJitter(0.0)))
            .expect("open succeeds on a brand-new device")
    }

    fn flush_and_ack(core: &mut MessengerCore<InsecurePlainCodecForTests>) {
        while let Some(batch) = core.take_flush_batch() {
            core.ack_flush(batch.id);
        }
    }

    /// Feeds `body` back as the response to the currently outstanding
    /// `/sync` request, minting one first if none is outstanding yet (a
    /// fresh mint needs one more "flush it, then ask again" round before it
    /// is releasable -- `crate::core`'s own `sync_token_persisted_after_
    /// everything_it_covers` test does the same). Every OTHER request a
    /// call returns is answered with a bare `200 {}`: dropping it would
    /// leave it in flight forever, and the account-data lane is single
    /// flight, so one stranded write would block every later one
    /// (`MessengerCore::releasable_requests`'s own "never discard a
    /// non-empty result" doc).
    fn sync_with_body(core: &mut MessengerCore<InsecurePlainCodecForTests>, body: &serde_json::Value) {
        for _ in 0..3 {
            flush_and_ack(core);
            let mut sync_request = None;
            for request in core.releasable_requests(0) {
                if request.kind == OutgoingRequestKind::Sync && sync_request.is_none() {
                    sync_request = Some(request);
                } else {
                    core.on_response(request.id.clone(), HttpResponseDescriptor { status: 200, body: b"{}".to_vec() }, 0);
                }
            }
            if let Some(sync_request) = sync_request {
                core.on_response(
                    sync_request.id,
                    HttpResponseDescriptor { status: 200, body: serde_json::to_vec(body).expect("valid JSON") },
                    0,
                );
                flush_and_ack(core);
                return;
            }
        }
        panic!("no sync request became releasable");
    }

    /// [`MessengerCore::releasable_requests`] must never have a non-empty
    /// result thrown away (that call's own doc: doing so strands every
    /// request it returned permanently in flight) -- this crate's own
    /// `/sync` loop mints a fresh `Sync` request lazily, as a side effect
    /// of THIS very call, the moment none is outstanding, so a test that
    /// only wants the room-scoped request(s) a mutation produced must still
    /// answer any `Sync` request bundled alongside them (with a harmless
    /// no-op bump) rather than dropping it. Returns everything else.
    fn releasable_non_sync(core: &mut MessengerCore<InsecurePlainCodecForTests>) -> Vec<OutgoingRequest> {
        let mut rest = Vec::new();
        for request in core.releasable_requests(0) {
            if request.kind == OutgoingRequestKind::Sync {
                let body = serde_json::json!({ "next_batch": "s-drain" });
                core.on_response(request.id, HttpResponseDescriptor { status: 200, body: serde_json::to_vec(&body).expect("valid JSON") }, 0);
            } else {
                rest.push(request);
            }
        }
        rest
    }

    /// Releases and answers requests (bundled `Sync` requests via
    /// [`releasable_non_sync`]) until one of `kind` shows up, and returns
    /// it UNANSWERED. Every other request is answered with a bare
    /// `200 {}` so nothing is left stranded in flight: `Lane::Other` caps
    /// concurrency and the account-data lane is single flight, so a
    /// forgotten request would starve a LATER assertion for no reason
    /// relevant to what it tests.
    fn take_and_answer_rest(core: &mut MessengerCore<InsecurePlainCodecForTests>, kind: OutgoingRequestKind) -> OutgoingRequest {
        for _ in 0..8 {
            flush_and_ack(core);
            let mut target = None;
            for request in releasable_non_sync(core) {
                if target.is_none() && request.kind == kind {
                    target = Some(request);
                } else {
                    core.on_response(request.id.clone(), HttpResponseDescriptor { status: 200, body: b"{}".to_vec() }, 0);
                }
            }
            if let Some(target) = target {
                return target;
            }
        }
        panic!("no releasable request of kind {kind:?}");
    }

    fn join_body(room_id: &str, room_value: serde_json::Value, next_batch: &str) -> serde_json::Value {
        let mut join = serde_json::Map::new();
        join.insert(room_id.to_string(), room_value);
        serde_json::json!({ "next_batch": next_batch, "rooms": { "join": Value::Object(join) } })
    }

    use serde_json::Value;

    /// A DM between `self_id` and `peer_id`, `peer_id` carrying `displayname`
    /// (if given), plus one still-sealed Megolm timeline event from the
    /// peer (no session ever shared, so it stays `Undecryptable`).
    fn dm_sync_body(self_id: &str, peer_id: &str, peer_displayname: Option<&str>) -> serde_json::Value {
        let mut peer_member_content = serde_json::json!({ "membership": "join", "is_direct": true });
        if let Some(name) = peer_displayname {
            peer_member_content["displayname"] = Value::String(name.to_string());
        }
        let state = vec![
            serde_json::json!({
                "event_id": "$c1:example.org", "type": "m.room.create", "sender": self_id,
                "origin_server_ts": 1, "state_key": "", "content": { "creator": self_id }
            }),
            serde_json::json!({
                "event_id": "$m1:example.org", "type": "m.room.member", "sender": self_id,
                "origin_server_ts": 1, "state_key": self_id, "content": { "membership": "join" }
            }),
            serde_json::json!({
                "event_id": "$m2:example.org", "type": "m.room.member", "sender": self_id,
                "origin_server_ts": 1, "state_key": peer_id, "content": peer_member_content
            }),
            serde_json::json!({
                "event_id": "$e1:example.org", "type": "m.room.encryption", "sender": self_id,
                "origin_server_ts": 1, "state_key": "", "content": { "algorithm": "m.megolm.v1.aes-sha2" }
            }),
        ];
        let timeline = vec![serde_json::json!({
            "event_id": "$msg1:example.org", "type": "m.room.encrypted", "sender": peer_id, "origin_server_ts": 100,
            "content": { "algorithm": "m.megolm.v1.aes-sha2", "ciphertext": "sealed-ciphertext==", "session_id": "sess1" }
        })];
        join_body("!dm:example.org", serde_json::json!({ "state": { "events": state }, "timeline": { "events": timeline } }), "s1")
    }

    /// An ordinary, unencrypted group room with only `self_id` as a member
    /// -- enough for `require_room` to accept intents against it.
    fn group_sync_body(self_id: &str, room_id: &str, next_batch: &str) -> serde_json::Value {
        let state = vec![
            serde_json::json!({
                "event_id": "$c1:example.org", "type": "m.room.create", "sender": self_id,
                "origin_server_ts": 1, "state_key": "", "content": { "creator": self_id }
            }),
            serde_json::json!({
                "event_id": "$m1:example.org", "type": "m.room.member", "sender": self_id,
                "origin_server_ts": 1, "state_key": self_id, "content": { "membership": "join" }
            }),
        ];
        join_body(room_id, serde_json::json!({ "state": { "events": state } }), next_batch)
    }

    #[test]
    fn dm_room_summary_uses_peer_name_and_encrypted_preview() {
        let alice = core_user("@alice:example.org");
        let mut core = open_core(alice, "ALICEDEV");
        sync_with_body(&mut core, &dm_sync_body("@alice:example.org", "@bob:example.org", Some("Bob Trader")));

        let view = build_view(&core, 0);
        let summary = view.rooms.iter().find(|room| room.id.as_str() == "!dm:example.org").expect("room present");
        assert_eq!(summary.kind, RoomKind::Direct);
        assert_eq!(summary.name, "Bob Trader");
        assert_eq!(summary.initials, "BT");
        assert!(summary.is_encrypted);
        let preview = summary.last_event.as_ref().expect("a last_event preview");
        assert_eq!(preview.content, PreviewContent::Encrypted);
        assert!(summary.can_post, "a joined member of an ordinary room can post");
    }

    #[test]
    fn local_echo_item_carries_txn_id_and_retry_maps_back() {
        let alice = core_user("@alice:example.org");
        let mut core = open_core(alice, "ALICEDEV");
        sync_with_body(&mut core, &group_sync_body("@alice:example.org", "!r:example.org", "s1"));

        let room = RoomId::from("!r:example.org");
        apply_intent(&mut core, MessengerIntent::SendText { room: room.clone(), text: "hi".to_string(), reply_to: None }, 0)
            .expect("send applies");

        // Fail the send so its local echo settles on a stable, inspectable
        // `SendState::Failed` rather than racing this test against the
        // pipeline's own further (synchronous) steps.
        flush_and_ack(&mut core);
        let requests = releasable_non_sync(&mut core);
        let send_request = requests.into_iter().find(|r| r.kind == OutgoingRequestKind::RoomSend).expect("a RoomSend request");
        core.on_response(
            send_request.id,
            HttpResponseDescriptor { status: 400, body: br#"{"errcode":"M_UNKNOWN","error":"boom"}"#.to_vec() },
            0,
        );

        let view = build_view(&core, 0);
        let items = view.timelines.get(&room).expect("timeline present");
        let item = items.last().expect("the local echo is present");
        let SendState::Failed { .. } = &item.send_state else { panic!("expected a failed send, got {:?}", item.send_state) };
        let local_id = item.event_id.clone();

        // The event id an unconfirmed item carries is exactly its own
        // `TxnId` string -- `RetrySend::local_id` must round-trip back
        // into the SAME transaction the core is tracking.
        assert!(local_id.as_str().starts_with("m.txn."));
        assert!(core.send_failure_reason(&crate::ids::TxnId::from(local_id.as_str().to_string())).is_some());

        apply_intent(&mut core, MessengerIntent::RetrySend { room, local_id }, 0).expect("retry applies");
        assert!(
            core.send_failure_reason(&crate::ids::TxnId::from(item.event_id.as_str().to_string())).is_none(),
            "a successful retry moves the send out of the Failed phase"
        );
    }

    #[test]
    fn pinned_archived_muted_round_trip_through_intents() {
        let alice = core_user("@alice:example.org");
        let mut core = open_core(alice, "ALICEDEV");
        sync_with_body(&mut core, &group_sync_body("@alice:example.org", "!r:example.org", "s1"));
        let room = RoomId::from("!r:example.org");

        apply_intent(&mut core, MessengerIntent::SetPinned { room: room.clone(), pinned: true }, 0).expect("pin");
        flush_and_ack(&mut core);
        let pin_request = take_and_answer_rest(&mut core, OutgoingRequestKind::RoomAccountData);
        core.on_response(pin_request.id, HttpResponseDescriptor { status: 200, body: b"{}".to_vec() }, 0);
        sync_with_body(
            &mut core,
            &join_body(
                "!r:example.org",
                serde_json::json!({ "account_data": { "events": [{ "type": "m.tag", "content": { "tags": { "m.favourite": {} } } }] } }),
                "s2",
            ),
        );

        apply_intent(&mut core, MessengerIntent::SetArchived { room: room.clone(), archived: true }, 0).expect("archive");
        flush_and_ack(&mut core);
        let archive_request = take_and_answer_rest(&mut core, OutgoingRequestKind::RoomAccountData);
        core.on_response(archive_request.id, HttpResponseDescriptor { status: 200, body: b"{}".to_vec() }, 0);
        sync_with_body(
            &mut core,
            &join_body(
                "!r:example.org",
                serde_json::json!({ "account_data": { "events": [{ "type": "m.tag", "content": { "tags": { "m.favourite": {}, "u.mlc.archived": {} } } }] } }),
                "s3",
            ),
        );

        apply_intent(&mut core, MessengerIntent::SetMuted { room: room.clone(), muted: true }, 0).expect("mute");
        flush_and_ack(&mut core);
        let mute_request = take_and_answer_rest(&mut core, OutgoingRequestKind::RoomAccountData);
        core.on_response(mute_request.id, HttpResponseDescriptor { status: 200, body: b"{}".to_vec() }, 0);

        let view = build_view(&core, 0);
        let summary = view.rooms.iter().find(|r| r.id == room).expect("room present");
        assert!(summary.pinned, "pinned reflects the m.favourite tag once it round-trips back");
        assert!(summary.archived, "archived reflects the u.mlc.archived tag once it round-trips back");
        assert!(summary.muted, "muted is optimistic -- visible immediately, no round trip needed");
    }

    #[test]
    fn custom_folder_create_reorder_delete_rewrites_account_data_and_tags() {
        let alice = core_user("@alice:example.org");
        let mut core = open_core(alice, "ALICEDEV");
        sync_with_body(&mut core, &group_sync_body("@alice:example.org", "!r:example.org", "s1"));

        let folder = Folder {
            id: Some(TagName::from("u.trading-desk")),
            name: "Trading desk".to_string(),
            icon: FolderIcon::Candles,
            include: FolderInclude { rooms: vec![RoomId::from("!r:example.org")], ..Default::default() },
            exclude: FolderExclude::default(),
            order: 1.0,
        };
        apply_intent(&mut core, MessengerIntent::CreateFolder(folder), 0).expect("create folder");

        // The folder's own account data is optimistic -- visible without a
        // round trip.
        let view = build_view(&core, 0);
        let created =
            view.folders.iter().find(|f| f.id.as_ref().map(TagName::as_str) == Some("u.trading-desk")).expect("custom folder present");
        assert_eq!(created.order, 1.0);

        // Complete the round trip for the SetTag it also issued (creating
        // a folder tags every room it names), so DeleteFolder later finds
        // a real tag to remove.
        flush_and_ack(&mut core);
        let tag_request = take_and_answer_rest(&mut core, OutgoingRequestKind::RoomAccountData);
        core.on_response(tag_request.id, HttpResponseDescriptor { status: 200, body: b"{}".to_vec() }, 0);
        sync_with_body(
            &mut core,
            &join_body(
                "!r:example.org",
                serde_json::json!({ "account_data": { "events": [{ "type": "m.tag", "content": { "tags": { "u.trading-desk": {} } } }] } }),
                "s2",
            ),
        );

        apply_intent(
            &mut core,
            MessengerIntent::ReorderFolders(vec![(TagName::from("u.trading-desk"), 9.5)]),
            0,
        )
        .expect("reorder");
        let view = build_view(&core, 0);
        let reordered =
            view.folders.iter().find(|f| f.id.as_ref().map(TagName::as_str) == Some("u.trading-desk")).expect("still present");
        assert_eq!(reordered.order, 9.5);

        apply_intent(&mut core, MessengerIntent::DeleteFolder(TagName::from("u.trading-desk")), 0).expect("delete");
        let view = build_view(&core, 0);
        assert!(
            view.folders.iter().all(|f| f.id.as_ref().map(TagName::as_str) != Some("u.trading-desk")),
            "folder gone immediately (optimistic)"
        );

        // Deleting also removes the tag from every room carrying it --
        // asserted at the wire level (the removal itself is not
        // optimistic, same as every other `m.tag` write in this crate).
        flush_and_ack(&mut core);
        let untag_request = take_and_answer_rest(&mut core, OutgoingRequestKind::RoomAccountData);
        let body = untag_request.body.expect("a body");
        assert!(!body.to_string().contains("u.trading-desk"), "the tag is gone from the rewritten m.tag content");
    }

    #[test]
    fn unreact_redacts_only_my_reaction() {
        let alice = core_user("@alice:example.org");
        let mut core = open_core(alice, "ALICEDEV");
        let state = vec![
            serde_json::json!({
                "event_id": "$c1:example.org", "type": "m.room.create", "sender": "@alice:example.org",
                "origin_server_ts": 1, "state_key": "", "content": { "creator": "@alice:example.org" }
            }),
            serde_json::json!({
                "event_id": "$m1:example.org", "type": "m.room.member", "sender": "@alice:example.org",
                "origin_server_ts": 1, "state_key": "@alice:example.org", "content": { "membership": "join" }
            }),
            serde_json::json!({
                "event_id": "$m2:example.org", "type": "m.room.member", "sender": "@alice:example.org",
                "origin_server_ts": 1, "state_key": "@bob:example.org", "content": { "membership": "join" }
            }),
        ];
        let timeline = vec![
            serde_json::json!({
                "event_id": "$msg:example.org", "type": "m.room.message", "sender": "@bob:example.org", "origin_server_ts": 10,
                "content": { "msgtype": "m.text", "body": "hi" }
            }),
            serde_json::json!({
                "event_id": "$reactAlice:example.org", "type": "m.reaction", "sender": "@alice:example.org", "origin_server_ts": 11,
                "content": { "m.relates_to": { "rel_type": "m.annotation", "event_id": "$msg:example.org", "key": "👍" } }
            }),
            serde_json::json!({
                "event_id": "$reactBob:example.org", "type": "m.reaction", "sender": "@bob:example.org", "origin_server_ts": 12,
                "content": { "m.relates_to": { "rel_type": "m.annotation", "event_id": "$msg:example.org", "key": "👍" } }
            }),
        ];
        sync_with_body(
            &mut core,
            &join_body("!r:example.org", serde_json::json!({ "state": { "events": state }, "timeline": { "events": timeline } }), "s1"),
        );

        apply_intent(
            &mut core,
            MessengerIntent::Unreact { room: RoomId::from("!r:example.org"), event_id: EventId::from("$msg:example.org") },
            0,
        )
        .expect("unreact applies");

        flush_and_ack(&mut core);
        let redacts: Vec<OutgoingRequest> =
            releasable_non_sync(&mut core).into_iter().filter(|r| r.kind == OutgoingRequestKind::RoomRedact).collect();
        assert_eq!(redacts.len(), 1, "only alice's own reaction is redacted");
        assert!(redacts[0].path.contains("reactAlice"));
        assert!(!redacts[0].path.contains("reactBob"));
    }

    #[test]
    fn channel_not_joined_maps_to_subscriber_role() {
        let alice = core_user("@alice:example.org");
        let mut core = open_core(alice, "ALICEDEV");
        // A public, announcement-shaped channel where alice never sent (or
        // received) her own `m.room.member` -- a public-directory preview
        // shape, not an ordinary joined room.
        let state = vec![
            serde_json::json!({
                "event_id": "$c1:example.org", "type": "m.room.create", "sender": "@bob:example.org",
                "origin_server_ts": 1, "state_key": "", "content": { "creator": "@bob:example.org" }
            }),
            serde_json::json!({
                "event_id": "$m1:example.org", "type": "m.room.member", "sender": "@bob:example.org",
                "origin_server_ts": 1, "state_key": "@bob:example.org", "content": { "membership": "join" }
            }),
            serde_json::json!({
                "event_id": "$j1:example.org", "type": "m.room.join_rules", "sender": "@bob:example.org",
                "origin_server_ts": 1, "state_key": "", "content": { "join_rule": "public" }
            }),
            serde_json::json!({
                "event_id": "$p1:example.org", "type": "m.room.power_levels", "sender": "@bob:example.org",
                "origin_server_ts": 1, "state_key": "", "content": { "events_default": 50, "users_default": 0 }
            }),
        ];
        sync_with_body(&mut core, &join_body("!ch:example.org", serde_json::json!({ "state": { "events": state } }), "s1"));

        let view = build_view(&core, 0);
        let summary = view.rooms.iter().find(|r| r.id.as_str() == "!ch:example.org").expect("room present");
        assert_eq!(summary.kind, RoomKind::Channel);
        assert_eq!(summary.my_role, MemberRole::Subscriber);
        assert!(!summary.can_post, "a channel this account has not joined cannot be posted to");
    }

    #[test]
    fn unknown_room_id_intent_reports_an_error_naming_it() {
        let alice = core_user("@alice:example.org");
        let mut core = open_core(alice, "ALICEDEV");
        let err = apply_intent(
            &mut core,
            MessengerIntent::SendText { room: RoomId::from("!missing:example.org"), text: "hi".to_string(), reply_to: None },
            0,
        )
        .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("!missing:example.org"), "error should name the offending room id: {message}");
    }

    fn member_event(user: &str, membership: &str, displayname: Option<&str>) -> Value {
        let mut content = serde_json::json!({ "membership": membership });
        if let Some(name) = displayname {
            content["displayname"] = Value::String(name.to_string());
        }
        serde_json::json!({
            "event_id": format!("$m-{user}"), "type": "m.room.member", "sender": user,
            "origin_server_ts": 1, "state_key": user, "content": content
        })
    }

    #[test]
    fn a_member_who_is_only_invited_cannot_post() {
        let mut core = open_core(core_user("@alice:example.org"), "ALICEDEV");
        let state = vec![member_event("@alice:example.org", "invite", None), member_event("@bob:example.org", "join", None)];
        sync_with_body(&mut core, &join_body("!g:example.org", serde_json::json!({ "state": { "events": state } }), "s1"));

        let view = build_view(&core, 0);
        let summary = view.rooms.iter().find(|r| r.id.as_str() == "!g:example.org").expect("room present");
        assert!(!summary.can_post, "an invite is not a membership that may post, whatever the power levels say");
    }

    #[test]
    fn unnamed_room_name_falls_back_to_joined_member_names() {
        let mut core = open_core(core_user("@alice:example.org"), "ALICEDEV");
        let state = vec![
            member_event("@alice:example.org", "join", None),
            member_event("@bob:example.org", "join", Some("Bob Trader")),
            member_event("@carol:example.org", "join", None),
            member_event("@dave:example.org", "join", None),
            member_event("@erin:example.org", "join", None),
            member_event("@frank:example.org", "invite", Some("Frank")),
        ];
        sync_with_body(&mut core, &join_body("!g:example.org", serde_json::json!({ "state": { "events": state } }), "s1"));
        // Only this account: nobody else to name.
        sync_with_body(&mut core, &group_sync_body("@alice:example.org", "!solo:example.org", "s2"));
        // A channel (public, announcement-shaped) with nobody else joined.
        let channel_state = vec![
            member_event("@alice:example.org", "join", None),
            serde_json::json!({
                "event_id": "$j1:example.org", "type": "m.room.join_rules", "sender": "@alice:example.org",
                "origin_server_ts": 1, "state_key": "", "content": { "join_rule": "public" }
            }),
            serde_json::json!({
                "event_id": "$p1:example.org", "type": "m.room.power_levels", "sender": "@alice:example.org",
                "origin_server_ts": 1, "state_key": "", "content": { "events_default": 50, "users_default": 0 }
            }),
        ];
        sync_with_body(&mut core, &join_body("!ch:example.org", serde_json::json!({ "state": { "events": channel_state } }), "s3"));

        let view = build_view(&core, 0);
        let name_of = |id: &str| view.rooms.iter().find(|r| r.id.as_str() == id).expect("room present").clone();
        let group = name_of("!g:example.org");
        assert_eq!(group.name, "Bob Trader, carol, dave +1", "up to three joined members, self and invitees excluded, then +N");
        assert_eq!(group.initials, "BT");
        assert_eq!(name_of("!solo:example.org").name, "Group");
        let channel = name_of("!ch:example.org");
        assert_eq!(channel.kind, RoomKind::Channel);
        assert_eq!(channel.name, "Channel");
    }

    #[test]
    fn reply_to_unloaded_target_keeps_ellipsis_with_an_empty_sender() {
        let mut core = open_core(core_user("@alice:example.org"), "ALICEDEV");
        let state = vec![member_event("@alice:example.org", "join", None)];
        let timeline = vec![serde_json::json!({
            "event_id": "$reply:example.org", "type": "m.room.message", "sender": "@alice:example.org", "origin_server_ts": 5,
            "content": {
                "msgtype": "m.text", "body": "re",
                "m.relates_to": { "m.in_reply_to": { "event_id": "$gone:example.org" } }
            }
        })];
        sync_with_body(
            &mut core,
            &join_body("!g:example.org", serde_json::json!({ "state": { "events": state }, "timeline": { "events": timeline } }), "s1"),
        );

        let view = build_view(&core, 0);
        let items = view.timelines.get(&RoomId::from("!g:example.org")).expect("timeline present");
        let reply = items[0].reply_to.as_ref().expect("a reply preview");
        assert_eq!(reply.event_id.as_str(), "$gone:example.org");
        assert_eq!(reply.preview, "…");
        assert_eq!(reply.sender.as_str(), "", "the model requires a UserId; an unloaded target's sender is unknown, so it is empty");
    }
}
