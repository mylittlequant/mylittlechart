//! Small standalone types that do not earn their own file: time/power-level
//! newtypes, capability/vault flags, back-pagination state, room-creation
//! requests and the intent mailbox enum.

use crate::folder::Folder;
use crate::ids::{EventId, PaginationToken, RoomId, TagName, UserId};
use crate::member::RankBadge;
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
    React { room: RoomId, event_id: EventId, key: String },
    Unreact { room: RoomId, event_id: EventId },
    MarkRead { room: RoomId, event_id: EventId },
    SetTyping { room: RoomId, typing: bool },
    SetMuted { room: RoomId, muted: bool },
    SetPinned { room: RoomId, pinned: bool },
    SetArchived { room: RoomId, archived: bool },
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
    /// Join a room — a public channel picked from
    /// [`crate::MessengerProvider::public_rooms`], or an invite accepted.
    JoinRoom { room: RoomId },
    LeaveRoom { room: RoomId },
    Invite { room: RoomId, user: UserId },
    Kick { room: RoomId, user: UserId },
    /// Re-send a `SendState::Failed` item. `local_id` is the id the item
    /// carries in the timeline (its transaction id while no server event id
    /// exists); the provider re-sends the identical bytes.
    RetrySend { room: RoomId, local_id: EventId },
    /// Search the public-room directory; the latest result set is read back
    /// through [`crate::MessengerProvider::public_rooms`].
    SearchPublicRooms { term: String },
    /// Search the user directory; the latest result set is read back through
    /// [`crate::MessengerProvider::user_search`].
    SearchUsers { term: String },
}

/// One public-room directory hit — a row of the "Join a channel" list.
#[derive(Clone, Debug, PartialEq)]
pub struct PublicRoomEntry {
    pub room_id: RoomId,
    pub name: String,
    pub topic: Option<String>,
    pub member_count: u64,
    /// Whether this account is already a member.
    pub joined: bool,
}

/// One user-directory hit — a row of the "New message" / "Invite" pickers.
#[derive(Clone, Debug, PartialEq)]
pub struct UserEntry {
    pub user_id: UserId,
    pub display_name: String,
    pub rank: Option<RankBadge>,
}
