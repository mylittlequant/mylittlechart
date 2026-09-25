//! The messenger UI's whole data contract.

use crate::folder::Folder;
use crate::ids::RoomId;
use crate::member::Member;
use crate::misc::{
    BackPagination, Capabilities, MessengerIntent, PublicRoomEntry, UserEntry, VaultState,
};
use crate::room::RoomSummary;
use crate::timeline::TimelineItem;

/// Implemented once per backend — today's 1:1 DM adapter
/// (`mlc-app-layout`'s `DmMessengerProvider`), later the Matrix-shaped
/// provider `messenger-proto` builds. Reads are synchronous snapshots
/// (`&self`, cheap enough to call every frame — an implementation that
/// needs caching owns that caching itself, not this trait); mutation is a
/// single [`Self::enqueue`] call into a mailbox the provider drains on its
/// own schedule, never a per-verb method that blocks. The UI never awaits
/// anything (SWC: the UI core reacts to state, it does not drive I/O), and
/// this trait has no `async fn` so it stays implementable on wasm32
/// without pulling in a runtime.
pub trait MessengerProvider {
    /// Bumped by the provider every time a mutation lands that could change
    /// what any read below returns. The UI compares this against the value
    /// it saw last frame to decide whether it is even worth re-reading —
    /// cheap enough to check every frame, unlike diffing a snapshot.
    fn change_counter(&self) -> u64;

    fn capabilities(&self) -> Capabilities;
    fn vault_state(&self) -> VaultState;
    /// The provider's own last fetch/send failure, if any — one slot, the
    /// same "one mailbox, one error slot" shape `DmState::last_error`
    /// already has.
    fn last_error(&self) -> Option<String>;

    /// Every room this account currently holds (or, for a public channel,
    /// currently reads) — chat-list ORDER is the UI's own job (pinned,
    /// unread, recency), not this method's.
    fn rooms(&self) -> Vec<RoomSummary>;
    fn room(&self, room: &RoomId) -> Option<RoomSummary>;
    /// A room's currently-held timeline, oldest first — what has already
    /// been fetched/decrypted; older history is reached via
    /// [`Self::back_pagination`] plus a [`MessengerIntent::LoadOlder`].
    fn timeline(&self, room: &RoomId) -> Vec<TimelineItem>;
    fn back_pagination(&self, room: &RoomId) -> BackPagination;
    fn members(&self, room: &RoomId) -> Vec<Member>;
    /// Every folder this account has — built-ins (`id: None`) first, in
    /// display order, then custom folders by their own `order`.
    fn folders(&self) -> Vec<Folder>;

    /// The latest [`MessengerIntent::SearchPublicRooms`] result set — empty
    /// until one has landed, or when the backend has no directory.
    fn public_rooms(&self) -> Vec<PublicRoomEntry> {
        Vec::new()
    }
    /// The latest [`MessengerIntent::SearchUsers`] result set — empty until
    /// one has landed, or when the backend has no user directory.
    fn user_search(&self) -> Vec<UserEntry> {
        Vec::new()
    }

    /// Enqueue one user-triggered action for the provider to drain on its
    /// own schedule.
    fn enqueue(&mut self, intent: MessengerIntent);
}
