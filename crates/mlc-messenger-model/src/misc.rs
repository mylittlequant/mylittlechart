//! Small standalone types that do not earn their own file: time/power-level
//! newtypes, capability/vault flags, back-pagination state, room-creation
//! requests and the intent mailbox enum.

use crate::folder::Folder;
use crate::ids::{EventId, PaginationToken, RoomId, TagName, UserId};
use crate::room::RoomKind;

/// Milliseconds since the Unix epoch — Matrix's own `origin_server_ts`
/// unit, used everywhere a moment in time crosses this crate's boundary. A
/// plain newtype, not `Duration`/`SystemTime`: this crate takes no `time`/
/// `chrono` dependency (none was needed for anything below).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub i64);

/// A member's raw power level (Matrix convention: 0..=100 in practice, but
/// the protocol allows any integer) — see [`crate::MemberRole::
/// from_power_level`] for the coarse role a UI actually draws from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PowerLevel(pub i64);

/// What the CURRENT provider can actually do — a DM-only adapter over
/// today's server reports every flag false except plain messaging; a full
/// Matrix-backed provider reports them all true. The UI reads this once per
/// frame to decide which concept screens/buttons even apply, rather than
/// hardcoding "does this feature exist" against one specific provider.
/// A flag that is `false` HIDES its control — the UI never draws a button
/// the provider would only refuse.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub groups: bool,
    pub channels: bool,
    pub folders: bool,
    pub reactions: bool,
    pub edits: bool,
    pub replies: bool,
    pub typing: bool,
    pub receipts: bool,
    /// [`MessengerIntent::Forward`] is carried — gates the bubble's Forward
    /// action and the forward chat picker.
    pub forward: bool,
    /// `m.sticker` events are sent and received ([`crate::MessageContent::Sticker`],
    /// [`MessengerIntent::SendSticker`]) — gates the composer's sticker tab
    /// and the sticker picker.
    pub stickers: bool,
    /// A chart can be sent into this room as a shared-chart card — gates the
    /// chart's "Send to chat" entry and the card attach action.
    pub chart_card_send: bool,
}

/// This account's own end-to-end device/vault state — gates which timeline
/// content a room can actually show (see `MessageContent::Encrypted`/
/// `Undecryptable`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultState {
    /// Sealed — ciphertext still renders (owner ruling 2026-09-10: a
    /// sealed vault is not an error state).
    Locked,
    /// No identity/device keys published yet for this account.
    NeedsSetup,
    /// Keys published, but this device has not completed cross-signing/
    /// verification — a real Matrix E2E state with no equivalent in
    /// today's DM model (that adapter never reports this variant).
    Unverified,
    /// The web client's own messenger core runs in exactly one browser tab
    /// at a time (holds a Web Lock); this tab is not it — the UI shows
    /// "open in another tab" and takes over once the lock frees. No
    /// equivalent on native (one process, one core) or in today's DM
    /// adapter.
    ActiveElsewhere,
    Ready,
}

/// One room's back-pagination state — an opaque cursor plus whether asking
/// for it plausibly returns anything more (see [`crate::MessengerProvider::
/// back_pagination`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BackPagination {
    pub token: Option<PaginationToken>,
    pub has_more: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomVisibility {
    Private,
    Public,
}

/// What "New message" / "New group" / "New channel" in the concept's pencil
/// menu submits.
#[derive(Clone, Debug, PartialEq)]
pub struct CreateRoomRequest {
    pub kind: RoomKind,
    pub title: Option<String>,
    pub description: Option<String>,
    pub member_ids: Vec<UserId>,
    pub members_can_add: bool,
    pub visibility: RoomVisibility,
}

/// One user-triggered action, handed to [`crate::MessengerProvider::
/// enqueue`] — the provider drains its own mailbox on its own schedule; the
/// UI never awaits a result, it only watches `change_counter()` move and
/// re-reads a snapshot on a later frame.
#[derive(Clone, Debug, PartialEq)]
pub enum MessengerIntent {
    SendText { room: RoomId, text: String, reply_to: Option<EventId> },
    Edit { room: RoomId, event_id: EventId, text: String },
    Redact { room: RoomId, event_id: EventId },
    /// Forward one event to another room — the provider re-sends the
    /// content with its own `org.mylittlechart.forwarded_from` attribution
    /// (event shape agreed with `messenger-proto`), NOT a redaction/move of
    /// the original; `from_room` is carried so a Matrix-shaped provider can
    /// resolve whether the source room was encrypted (→ [`crate::
    /// ForwardedFrom::Hidden`]) or a public channel (→ [`crate::
    /// ForwardedFrom::Channel`]) without a second round trip.
    Forward { from_room: RoomId, event_id: EventId, to_room: RoomId },
    React { room: RoomId, event_id: EventId, key: String },
    Unreact { room: RoomId, event_id: EventId },
    /// Send one sticker from a bundled pack (`m.sticker`, `url mlc-sticker://
    /// <pack>/<id>`) — the provider fills the event's `alt` from the pack's
    /// own table; the UI names the sticker by `(pack, id)` only.
    SendSticker { room: RoomId, pack: String, id: String },
    MarkRead { room: RoomId, event_id: EventId },
    SetTyping { room: RoomId, typing: bool },
    SetMuted { room: RoomId, muted: bool },
    SetPinned { room: RoomId, pinned: bool },
    SetArchived { room: RoomId, archived: bool },
    /// Leave a group/channel, or delete a 1:1 conversation — the provider
    /// decides which of the two "leaving" means for this room's own kind
    /// (concept row menu's own "Leave channel" / "Leave group" / "Delete
    /// chat", one action underneath).
    LeaveRoom { room: RoomId },
    LoadOlder { room: RoomId },
    CreateFolder(Folder),
    UpdateFolder(Folder),
    DeleteFolder(TagName),
    /// New `(tag, order)` for every reordered folder in one shot — mirrors
    /// how a drag-reorder in the concept's Folders screen lands (owner
    /// ruling: folders are tags with a Matrix-style `f64` order, so a
    /// reorder is just new order values, never a rewritten list).
    ReorderFolders(Vec<(TagName, f64)>),
    CreateRoom(CreateRoomRequest),
}
