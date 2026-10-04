//! Hand-off of an outside [`MessengerProvider`] to the Community modal's
//! Messages tab (cloud builds only).
//!
//! The shell owns the messenger I/O (the web shell's `MessengerIo`, over the
//! Matrix-shaped `mlc-messenger` engine) and gives the app the UI-facing half
//! of it — a `Send` [`MessengerProvider`] handle — through
//! [`ChartApp::set_messenger_provider`]. The shell never writes app fields
//! itself. While a provider is present:
//!
//! - the Messages tab paints from it (`render_community_modal`'s provider
//!   argument) instead of the legacy DM adapter over `DmState`;
//! - every click, key and menu action that used to arm a `DmRequest` enqueues
//!   a [`MessengerIntent`] into it instead;
//! - navigation (the open room) lives in `CommunityState::messenger_room`,
//!   because a provider's room ids are opaque strings, not DM numbers.
//!
//! `set_messenger_provider(None)` restores the legacy path unchanged. The
//! DM driver in the shell keeps running either way; while a provider is
//! present nothing reads the fields it fills.
//!
//! The logic lives on [`ChartPanelApp`] (which owns the modal state and the
//! provider slot) so it can be driven in tests without a whole `ChartApp`;
//! the few pieces that need the text-field store sit on [`ChartApp`].

use mlc_app_layout::ui::dm_messenger_provider::{enqueue_forward, enqueue_room_action, RoomAction};
use mlc_app_layout::ui::modal_settings::{CommunityTab, DmLookupHitUi};
use mlc_messenger_model::{
    CreateRoomRequest, EventId, MessengerIntent, MessengerProvider, RoomId, RoomKind, RoomVisibility, SendState,
    VaultState,
};
use uzor::WidgetId;

use crate::chart_panel_app::ChartPanelApp;
use crate::ChartApp;

/// The provider slot plus the small bookkeeping the pump needs. `Default` is
/// "no provider" — the legacy DM path.
#[derive(Default)]
pub(crate) struct MessengerBridge {
    provider: Option<Box<dyn MessengerProvider + Send>>,
    /// The newest confirmed event of the open room whose read receipt has
    /// been handled (sent, or not needed) — so each new message is receipted
    /// once, not every frame.
    read_marker: Option<(RoomId, EventId)>,
    /// The provider's `change_counter` the pump last looked at.
    pumped_counter: Option<u64>,
    /// `(change_counter, any_unread)` — the toolbar badge is stamped every
    /// frame, the provider's rooms are only re-read when it moved.
    unread_cache: Option<(u64, bool)>,
}

/// The newest event of `timeline` the server has confirmed — a local echo has
/// no event id of its own yet and cannot be receipted.
fn latest_confirmed_event(timeline: &[mlc_messenger_model::TimelineItem]) -> Option<EventId> {
    timeline.iter().rev().find(|item| item.send_state == SendState::Sent).map(|item| item.event_id.clone())
}

impl ChartPanelApp {
    /// Installs (`Some`) or removes (`None`) the outside provider backing the
    /// Messages tab — see the module doc. Switching drops the open room, the
    /// forward picker and any reply banner: ids of two backends never name
    /// the same room.
    pub fn set_messenger_provider(&mut self, provider: Option<Box<dyn MessengerProvider + Send>>) {
        let stickers = provider.as_ref().is_some_and(|provider| provider.capabilities().stickers);
        let chart_cards = provider.as_ref().is_some_and(|provider| provider.capabilities().chart_card_send);
        self.community_state.set_messenger_external(provider.is_some());
        self.community_state.messenger_stickers = stickers;
        self.community_state.messenger_chart_cards = chart_cards;
        self.messenger = MessengerBridge { provider, ..MessengerBridge::default() };
    }

    /// Whether an outside provider currently backs the Messages tab.
    pub fn has_messenger_provider(&self) -> bool {
        self.messenger.provider.is_some()
    }

    /// The provider as a paint-time read handle, `None` on the legacy path.
    pub(crate) fn messenger_view(&self) -> Option<&dyn MessengerProvider> {
        self.messenger.provider.as_deref().map(|provider| provider as &dyn MessengerProvider)
    }

    /// Enqueues `intent` into the provider; `false` (nothing enqueued) when
    /// there is none.
    pub(crate) fn messenger_enqueue(&mut self, intent: MessengerIntent) -> bool {
        match self.messenger.provider.as_deref_mut() {
            Some(provider) => {
                provider.enqueue(intent);
                true
            }
            None => false,
        }
    }

    /// Sends `text` to the open room. `false` when there is no provider (the
    /// caller then runs the legacy path); `true` once handled, which includes
    /// "nothing to send" (empty text, no room open) — the draft is then left
    /// alone by the caller.
    pub(crate) fn messenger_send_text(&mut self, text: &str) -> MessengerSend {
        if self.messenger.provider.is_none() {
            return MessengerSend::NoProvider;
        }
        let text = text.trim();
        let Some(room) = self.community_state.messenger_room.clone() else { return MessengerSend::Nothing };
        if text.is_empty() {
            return MessengerSend::Nothing;
        }
        self.messenger_enqueue(MessengerIntent::SendText { room, text: text.to_string(), reply_to: None });
        // A reply banner belongs to the message just sent.
        self.community_state.messenger_reply_preview = None;
        MessengerSend::Sent
    }

    /// Routes a chat-row / channel-footer toggle to the provider; `false`
    /// when there is none.
    pub(crate) fn messenger_room_action(&mut self, room_id: &str, action: RoomAction) -> bool {
        match self.messenger.provider.as_deref_mut() {
            Some(provider) => {
                enqueue_room_action(provider, room_id, action);
                true
            }
            None => false,
        }
    }

    /// Routes a picked forward target to the provider; `false` when there is
    /// none.
    pub(crate) fn messenger_forward(&mut self, from_room: &str, event_id: &str, to_room: &str) -> bool {
        match self.messenger.provider.as_deref_mut() {
            Some(provider) => {
                enqueue_forward(provider, from_room, event_id, to_room);
                true
            }
            None => false,
        }
    }

    /// The event a "mark as read" on `room_id` would receipt — the newest
    /// confirmed one the provider holds for that room.
    fn messenger_read_target(&self, room_id: &str) -> Option<EventId> {
        let provider = self.messenger.provider.as_deref()?;
        latest_confirmed_event(&provider.timeline(&RoomId::from(room_id)))
    }

    /// Whether the chat-row menu's "Mark as read" has anything to receipt.
    pub(crate) fn messenger_can_mark_read(&self, room_id: &str) -> bool {
        self.messenger_read_target(room_id).is_some()
    }

    /// Receipts the newest confirmed event of `room_id`; `false` when there
    /// is no provider or nothing to receipt.
    pub(crate) fn messenger_mark_room_read(&mut self, room_id: &str) -> bool {
        let Some(event_id) = self.messenger_read_target(room_id) else { return false };
        self.messenger_enqueue(MessengerIntent::MarkRead { room: RoomId::from(room_id), event_id })
    }

    /// Whether any room the provider holds has unread messages worth a badge
    /// on the toolbar's envelope: a muted or archived room does not light it.
    /// `None` on the legacy path (the caller reads the DM list instead).
    pub(crate) fn messenger_any_unread(&mut self) -> Option<bool> {
        let provider = self.messenger.provider.as_deref()?;
        let counter = provider.change_counter();
        if let Some((seen, value)) = self.messenger.unread_cache {
            if seen == counter {
                return Some(value);
            }
        }
        let value = provider.rooms().iter().any(|room| room.notification_count > 0 && !room.muted && !room.archived);
        self.messenger.unread_cache = Some((counter, value));
        Some(value)
    }

    /// The provider's own capabilities; `None` on the legacy path.
    pub(crate) fn messenger_capabilities(&self) -> Option<mlc_messenger_model::Capabilities> {
        self.messenger.provider.as_deref().map(|provider| provider.capabilities())
    }

    /// Runs a user-directory search for the "start a conversation" field and
    /// arms the dropdown that shows its results. The same term is not asked
    /// twice in a row.
    pub(crate) fn messenger_search_users(&mut self, term: &str) {
        let term = term.trim();
        if term.is_empty() {
            self.community_state.dm.close_lookup_dropdown();
            return;
        }
        if self.community_state.dm.lookup_query.as_deref() == Some(term) {
            return;
        }
        self.community_state.dm.lookup_query = Some(term.to_string());
        self.messenger_enqueue(MessengerIntent::SearchUsers { term: term.to_string() });
    }

    /// Starts a direct message with the `index`-th directory hit; `false`
    /// when there is no such hit.
    pub(crate) fn messenger_start_dm_with_hit(&mut self, index: usize) -> bool {
        let Some(entry) = self.messenger.provider.as_deref().and_then(|provider| provider.user_search().get(index).cloned())
        else {
            return false;
        };
        self.community_state.dm.close_lookup_dropdown();
        self.community_state.dm.new_conversation_open = false;
        self.messenger_enqueue(MessengerIntent::CreateRoom(CreateRoomRequest {
            kind: RoomKind::Direct,
            title: None,
            description: None,
            member_ids: vec![entry.user_id],
            members_can_add: false,
            visibility: RoomVisibility::Private,
        }))
    }

    /// Per-frame upkeep while a provider is present. `messenger_stickers` is
    /// refreshed every frame (and cleared when there is no provider); rooms
    /// and receipts are re-read only when the provider's change counter moved:
    ///
    /// - shows the user-directory hits in the recipient dropdown;
    /// - drops an open room the provider no longer holds (left, removed);
    /// - receipts the newest message of the room the user is looking at, once
    ///   per new message, so the unread badge clears.
    pub(crate) fn pump_messenger(&mut self) {
        // Before the vault/counter early-returns below: the sticker tab is a
        // per-frame capability read, including while the driver is not Ready.
        let stickers = self.messenger.provider.as_deref().is_some_and(|provider| provider.capabilities().stickers);
        let chart_cards = self.messenger.provider.as_deref().is_some_and(|provider| provider.capabilities().chart_card_send);
        self.community_state.messenger_stickers = stickers;
        self.community_state.messenger_chart_cards = chart_cards;
        let Some(provider) = self.messenger.provider.as_deref() else { return };

        if self.community_state.dm.lookup_query.is_some() {
            let hits: Vec<DmLookupHitUi> = provider
                .user_search()
                .into_iter()
                .map(|entry| DmLookupHitUi { public_id: entry.user_id.to_string(), nick: None, label: entry.display_name })
                .collect();
            if self.community_state.dm.lookup_results != hits {
                self.community_state.dm.lookup_results = hits;
            }
        }

        // Nothing is published (no rooms exist) until the driver is Ready.
        if provider.vault_state() != VaultState::Ready {
            return;
        }
        let counter = provider.change_counter();
        let open = self.community_state.messenger_room.clone();
        let marker_room = self.messenger.read_marker.as_ref().map(|(room, _)| room);
        if self.messenger.pumped_counter == Some(counter) && marker_room == open.as_ref() {
            return;
        }
        self.messenger.pumped_counter = Some(counter);

        let Some(room) = open else {
            self.messenger.read_marker = None;
            return;
        };
        let Some(summary) = provider.room(&room) else {
            self.community_state.close_messenger_room();
            self.messenger.read_marker = None;
            return;
        };
        let looking = self.community_state.is_open && self.community_state.tab == CommunityTab::Messages;
        if !looking {
            return;
        }
        let Some(latest) = latest_confirmed_event(&provider.timeline(&room)) else { return };
        let handled = self.messenger.read_marker.as_ref().is_some_and(|(r, e)| *r == room && *e == latest);
        if handled {
            return;
        }
        let unread = summary.notification_count > 0 || summary.highlight_count > 0;
        self.messenger.read_marker = Some((room.clone(), latest.clone()));
        if unread {
            self.messenger_enqueue(MessengerIntent::MarkRead { room, event_id: latest });
        }
    }
}

/// What [`ChartPanelApp::messenger_send_text`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MessengerSend {
    /// No provider — the legacy DM send path applies.
    NoProvider,
    /// A provider is present but there was nothing to send.
    Nothing,
    /// A `SendText` intent was enqueued.
    Sent,
}

impl ChartApp {
    /// Hands the app the UI-facing half of the shell's messenger I/O (`Some`)
    /// or takes it away (`None`, back to the legacy DM path) — see the module
    /// doc. The one entry point a shell uses; it never touches app fields.
    pub fn set_messenger_provider(&mut self, provider: Option<Box<dyn MessengerProvider + Send>>) {
        self.panel_app.set_messenger_provider(provider);
    }

    /// Whether an outside provider currently backs the Messages tab.
    pub fn has_messenger_provider(&self) -> bool {
        self.panel_app.has_messenger_provider()
    }

    /// Sends the draft box's text to the open room through the provider.
    /// `false` when there is no provider (the legacy path applies); `true`
    /// once handled — the draft is cleared only when a message was sent.
    pub(crate) fn messenger_send_draft(&mut self) -> bool {
        let wid = WidgetId::from(crate::text_input::DM_DRAFT);
        let text = self.input_coordinator.borrow().text_fields().text(&wid).to_string();
        match self.panel_app.messenger_send_text(&text) {
            MessengerSend::NoProvider => false,
            MessengerSend::Nothing => true,
            MessengerSend::Sent => {
                self.input_coordinator.borrow_mut().text_fields_mut().clear(&wid);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use mlc_messenger_model::{
        BackPagination, Capabilities, Folder, LastEventPreview, Member, MemberRole, MessageContent, PowerLevel,
        PreviewContent, PublicRoomEntry, RoomSummary, Timestamp, TimelineItem, UserEntry, UserId,
    };

    use super::*;

    /// A hand-built provider: fixed rooms/timelines/search hits, every intent
    /// recorded so a test can read what the app enqueued.
    #[derive(Clone, Default)]
    struct FakeProvider {
        rooms: Arc<Mutex<Vec<RoomSummary>>>,
        timelines: Arc<Mutex<Vec<(RoomId, Vec<TimelineItem>)>>>,
        hits: Arc<Mutex<Vec<UserEntry>>>,
        intents: Arc<Mutex<Vec<MessengerIntent>>>,
        counter: Arc<Mutex<u64>>,
        ready: bool,
    }

    impl FakeProvider {
        fn ready() -> Self {
            Self { ready: true, ..Self::default() }
        }
        fn bump(&self) {
            *self.counter.lock().unwrap() += 1;
        }
        fn add_room(&self, id: &str, unread: u64) {
            self.rooms.lock().unwrap().push(room(id, unread));
            self.bump();
        }
        fn set_timeline(&self, id: &str, items: Vec<TimelineItem>) {
            let mut all = self.timelines.lock().unwrap();
            all.retain(|(room, _)| room.as_str() != id);
            all.push((RoomId::from(id), items));
            self.bump();
        }
        fn intents(&self) -> Vec<MessengerIntent> {
            self.intents.lock().unwrap().clone()
        }
    }

    fn room(id: &str, unread: u64) -> RoomSummary {
        RoomSummary {
            id: RoomId::from(id),
            kind: RoomKind::Group,
            name: id.to_string(),
            initials: "R".to_string(),
            is_encrypted: false,
            is_public: false,
            notification_count: unread,
            highlight_count: 0,
            last_event: Some(LastEventPreview {
                sender: UserId::from("@bob:x"),
                content: PreviewContent::Text("hi".to_string()),
                ts: Timestamp(1),
            }),
            muted: false,
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

    fn item(event: &str, state: SendState) -> TimelineItem {
        TimelineItem {
            event_id: EventId::from(event),
            sender: UserId::from("@bob:x"),
            sender_rank: None,
            ts: Timestamp(1),
            content: MessageContent::Text("hi".to_string()),
            reply_to: None,
            edited: false,
            reactions: Vec::new(),
            redacted: false,
            send_state: state,
            read_by: Vec::new(),
            pinned: false,
            shared_chart: None,
            mentions_me: false,
            forwarded: None,
        }
    }

    impl MessengerProvider for FakeProvider {
        fn change_counter(&self) -> u64 {
            *self.counter.lock().unwrap()
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities { forward: true, ..Capabilities::default() }
        }
        fn vault_state(&self) -> VaultState {
            if self.ready {
                VaultState::Ready
            } else {
                VaultState::Locked
            }
        }
        fn last_error(&self) -> Option<String> {
            None
        }
        fn rooms(&self) -> Vec<RoomSummary> {
            self.rooms.lock().unwrap().clone()
        }
        fn room(&self, id: &RoomId) -> Option<RoomSummary> {
            self.rooms.lock().unwrap().iter().find(|r| &r.id == id).cloned()
        }
        fn timeline(&self, id: &RoomId) -> Vec<TimelineItem> {
            self.timelines.lock().unwrap().iter().find(|(r, _)| r == id).map(|(_, t)| t.clone()).unwrap_or_default()
        }
        fn back_pagination(&self, _room: &RoomId) -> BackPagination {
            BackPagination::default()
        }
        fn members(&self, _room: &RoomId) -> Vec<Member> {
            Vec::new()
        }
        fn folders(&self) -> Vec<Folder> {
            Vec::new()
        }
        fn public_rooms(&self) -> Vec<PublicRoomEntry> {
            Vec::new()
        }
        fn user_search(&self) -> Vec<UserEntry> {
            self.hits.lock().unwrap().clone()
        }
        fn enqueue(&mut self, intent: MessengerIntent) {
            self.intents.lock().unwrap().push(intent);
        }
    }

    fn panel_with(provider: &FakeProvider) -> ChartPanelApp {
        let mut panel = ChartPanelApp::new("");
        panel.set_messenger_provider(Some(Box::new(provider.clone())));
        panel
    }

    #[test]
    fn handing_over_a_provider_switches_the_messages_tab_to_it_and_none_switches_back() {
        let provider = FakeProvider::ready();
        let mut panel = ChartPanelApp::new("");
        assert!(!panel.has_messenger_provider());
        assert!(!panel.community_state.messenger_external);
        assert!(panel.messenger_view().is_none());

        panel.set_messenger_provider(Some(Box::new(provider.clone())));
        assert!(panel.has_messenger_provider());
        assert!(panel.community_state.messenger_external);
        assert!(panel.messenger_view().is_some());

        panel.community_state.open_messenger_room(RoomId::from("!r:x"));
        assert_eq!(panel.community_state.messenger_open_room(), Some(RoomId::from("!r:x")));

        panel.set_messenger_provider(None);
        assert!(!panel.has_messenger_provider());
        assert!(!panel.community_state.messenger_external);
        assert_eq!(panel.community_state.messenger_open_room(), None, "the legacy list view is showing again");
    }

    #[test]
    fn without_a_provider_nothing_is_routed_and_the_legacy_path_is_told_so() {
        let mut panel = ChartPanelApp::new("");
        assert_eq!(panel.messenger_send_text("hello"), MessengerSend::NoProvider);
        assert!(!panel.messenger_enqueue(MessengerIntent::LoadOlder { room: RoomId::from("!r:x") }));
        assert!(!panel.messenger_room_action("!r:x", RoomAction::TogglePin));
        assert!(!panel.messenger_forward("!a:x", "$e", "!b:x"));
        assert!(!panel.messenger_mark_room_read("!r:x"));
        panel.pump_messenger();
    }

    #[test]
    fn send_text_enqueues_a_send_text_intent_for_the_open_room() {
        let provider = FakeProvider::ready();
        provider.add_room("!r:x", 0);
        let mut panel = panel_with(&provider);

        assert_eq!(panel.messenger_send_text("hello"), MessengerSend::Nothing, "no room open yet");
        panel.community_state.open_messenger_room(RoomId::from("!r:x"));
        assert_eq!(panel.messenger_send_text("   "), MessengerSend::Nothing, "blank text sends nothing");
        assert_eq!(panel.messenger_send_text("  hello  "), MessengerSend::Sent);
        assert_eq!(
            provider.intents(),
            vec![MessengerIntent::SendText { room: RoomId::from("!r:x"), text: "hello".to_string(), reply_to: None }]
        );
    }

    #[test]
    fn room_actions_and_forward_reach_the_provider() {
        let provider = FakeProvider::ready();
        provider.add_room("!r:x", 0);
        let mut panel = panel_with(&provider);

        assert!(panel.messenger_room_action("!r:x", RoomAction::TogglePin));
        assert!(panel.messenger_room_action("!r:x", RoomAction::Leave));
        assert!(panel.messenger_forward("!r:x", "$e1", "!other:x"));
        assert_eq!(
            provider.intents(),
            vec![
                MessengerIntent::SetPinned { room: RoomId::from("!r:x"), pinned: true },
                MessengerIntent::LeaveRoom { room: RoomId::from("!r:x") },
                MessengerIntent::Forward {
                    from_room: RoomId::from("!r:x"),
                    event_id: EventId::from("$e1"),
                    to_room: RoomId::from("!other:x"),
                },
            ]
        );
    }

    #[test]
    fn mark_room_read_receipts_the_newest_confirmed_event_not_a_local_echo() {
        let provider = FakeProvider::ready();
        provider.add_room("!r:x", 2);
        provider.set_timeline(
            "!r:x",
            vec![item("$old", SendState::Sent), item("$new", SendState::Sent), item("m.txn.1", SendState::Sending)],
        );
        let mut panel = panel_with(&provider);

        assert!(panel.messenger_can_mark_read("!r:x"));
        assert!(panel.messenger_mark_room_read("!r:x"));
        assert_eq!(
            provider.intents(),
            vec![MessengerIntent::MarkRead { room: RoomId::from("!r:x"), event_id: EventId::from("$new") }]
        );
        assert!(!panel.messenger_can_mark_read("!empty:x"), "a room with no confirmed event has nothing to receipt");
    }

    #[test]
    fn pump_receipts_each_new_message_of_the_open_room_once() {
        let provider = FakeProvider::ready();
        provider.add_room("!r:x", 1);
        provider.set_timeline("!r:x", vec![item("$a", SendState::Sent)]);
        let mut panel = panel_with(&provider);
        panel.community_state.is_open = true;
        panel.community_state.tab = CommunityTab::Messages;
        panel.community_state.open_messenger_room(RoomId::from("!r:x"));

        panel.pump_messenger();
        panel.pump_messenger();
        assert_eq!(
            provider.intents(),
            vec![MessengerIntent::MarkRead { room: RoomId::from("!r:x"), event_id: EventId::from("$a") }],
            "one receipt however many frames pass"
        );

        provider.set_timeline("!r:x", vec![item("$a", SendState::Sent), item("$b", SendState::Sent)]);
        panel.pump_messenger();
        assert_eq!(provider.intents().len(), 2, "a new message is receipted again");
        assert_eq!(
            provider.intents()[1],
            MessengerIntent::MarkRead { room: RoomId::from("!r:x"), event_id: EventId::from("$b") }
        );
    }

    #[test]
    fn pump_does_not_receipt_while_the_messages_tab_is_not_showing_or_nothing_is_unread() {
        let provider = FakeProvider::ready();
        provider.add_room("!r:x", 3);
        provider.add_room("!quiet:x", 0);
        provider.set_timeline("!r:x", vec![item("$a", SendState::Sent)]);
        provider.set_timeline("!quiet:x", vec![item("$q", SendState::Sent)]);
        let mut panel = panel_with(&provider);
        panel.community_state.open_messenger_room(RoomId::from("!r:x"));

        panel.community_state.is_open = false;
        panel.pump_messenger();
        assert!(provider.intents().is_empty(), "the modal is closed: nobody is looking");

        panel.community_state.is_open = true;
        panel.community_state.tab = CommunityTab::Messages;
        panel.community_state.open_messenger_room(RoomId::from("!quiet:x"));
        panel.pump_messenger();
        assert!(provider.intents().is_empty(), "a room with nothing unread needs no receipt");
    }

    #[test]
    fn pump_closes_an_open_room_the_provider_no_longer_holds() {
        let provider = FakeProvider::ready();
        provider.add_room("!r:x", 0);
        let mut panel = panel_with(&provider);
        panel.community_state.open_messenger_room(RoomId::from("!gone:x"));

        panel.pump_messenger();
        assert_eq!(panel.community_state.messenger_open_room(), None);
    }

    #[test]
    fn pump_waits_for_ready_before_judging_the_open_room() {
        let provider = FakeProvider::default();
        let mut panel = panel_with(&provider);
        panel.community_state.open_messenger_room(RoomId::from("!r:x"));

        panel.pump_messenger();
        assert_eq!(
            panel.community_state.messenger_open_room(),
            Some(RoomId::from("!r:x")),
            "no rooms exist before the driver is Ready — not a reason to close the pane"
        );
    }

    #[test]
    fn user_search_arms_the_dropdown_and_a_hit_starts_a_direct_message() {
        let provider = FakeProvider::ready();
        provider.hits.lock().unwrap().push(UserEntry {
            user_id: UserId::from("@carol:x"),
            display_name: "Carol".to_string(),
            rank: None,
        });
        let mut panel = panel_with(&provider);

        panel.messenger_search_users("car");
        panel.messenger_search_users("car");
        assert_eq!(
            provider.intents(),
            vec![MessengerIntent::SearchUsers { term: "car".to_string() }],
            "the same term is not asked twice"
        );
        assert_eq!(panel.community_state.dm.lookup_query.as_deref(), Some("car"));

        panel.pump_messenger();
        assert_eq!(panel.community_state.dm.lookup_results.len(), 1);
        assert_eq!(panel.community_state.dm.lookup_results[0].label, "Carol");

        assert!(panel.messenger_start_dm_with_hit(0));
        assert!(!panel.messenger_start_dm_with_hit(5), "no such hit");
        let intents = provider.intents();
        assert_eq!(
            intents[1],
            MessengerIntent::CreateRoom(CreateRoomRequest {
                kind: RoomKind::Direct,
                title: None,
                description: None,
                member_ids: vec![UserId::from("@carol:x")],
                members_can_add: false,
                visibility: RoomVisibility::Private,
            })
        );
        assert!(panel.community_state.dm.lookup_query.is_none(), "the dropdown closes once a hit is picked");
    }

    #[test]
    fn the_toolbar_badge_lights_for_an_unread_room_that_is_neither_muted_nor_archived() {
        let provider = FakeProvider::ready();
        let mut panel = panel_with(&provider);
        assert_eq!(panel.messenger_any_unread(), Some(false), "no rooms, nothing unread");

        provider.add_room("!quiet:x", 4);
        provider.rooms.lock().unwrap()[0].muted = true;
        assert_eq!(panel.messenger_any_unread(), Some(false), "a muted room does not light the envelope");

        provider.add_room("!old:x", 2);
        provider.rooms.lock().unwrap()[1].archived = true;
        assert_eq!(panel.messenger_any_unread(), Some(false), "nor does an archived one");

        provider.add_room("!loud:x", 1);
        assert_eq!(panel.messenger_any_unread(), Some(true));

        assert_eq!(ChartPanelApp::new("").messenger_any_unread(), None, "the legacy path reads the DM list instead");
    }

    #[test]
    fn capabilities_come_from_the_provider() {
        let provider = FakeProvider::ready();
        let panel = panel_with(&provider);
        assert!(panel.messenger_capabilities().is_some_and(|caps| caps.forward));
        assert!(ChartPanelApp::new("").messenger_capabilities().is_none());
    }
}
