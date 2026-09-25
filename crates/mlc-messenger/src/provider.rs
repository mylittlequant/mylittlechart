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

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use mlc_messenger_model::{
    BackPagination, Capabilities, Folder, Member, MessengerIntent, MessengerProvider, PublicRoomEntry, RoomId,
    RoomSummary, TimelineItem, UserEntry, VaultState,
};

use crate::core::MessengerCore;
use crate::error::MessengerError;
use crate::store::RecordCodec;

/// What the engine behind this provider can do in the first release slice
/// (owner reorder 2026-09-25: ship what works end to end, switch the rest
/// off). `folders` turns on once the tag mapping in [`build_view`] and
/// [`apply_intents`] lands.
pub const SLICE_CAPABILITIES: Capabilities = Capabilities {
    groups: true,
    channels: true,
    folders: false,
    reactions: true,
    edits: true,
    replies: true,
    typing: true,
    receipts: true,
};

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
/// only when [`MessengerCore::change_counter`] moved. The model mapping is
/// P2's job; until it lands this returns an empty view, so the I/O drivers
/// can be built and run against the real core in parallel.
pub fn build_view<C: RecordCodec>(_core: &MessengerCore<C>, _now_ms: i64) -> ProviderView {
    ProviderView::default()
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

/// One intent -> one or more core commands. P2 fills in every arm; until
/// then every intent is refused by name, never dropped silently.
fn apply_intent<C: RecordCodec>(
    _core: &mut MessengerCore<C>,
    intent: MessengerIntent,
    _now_ms: i64,
) -> Result<(), MessengerError> {
    Err(MessengerError::IntentRefused(format!("not wired yet: {intent:?}")))
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
}
