//! End-to-end send-pipeline tests (M13b, part 2) — two or three
//! [`MessengerCore`]s driven through [`FakeServer`]'s own real-room model
//! (`Room`/`room_cursors`, this fake's own module doc), using ONLY the
//! kernel API on every side: [`MessengerCore::dispatch`],
//! [`MessengerCore::releasable_requests`], [`MessengerCore::on_response`],
//! [`MessengerCore::take_flush_batch`]/[`MessengerCore::ack_flush`],
//! [`MessengerCore::timeline`]/[`MessengerCore::room_state`]/etc. Never
//! reaches into `crypto`/`room`/`store` internals for a core under test —
//! `e2e_core.rs`'s own `PeerClient` is the one place in this crate that
//! still drives Olm/Megolm managers directly, and that is scoped to the
//! receive-path piece (M13a) this file does not touch.
//!
//! # Helper shape
//!
//! [`Device`] pairs one simulated client's identity (`user_id`/`device_id`)
//! with its own [`MessengerCore`], so a test reads as a sequence of
//! `alice.dispatch(...)`/`alice.drive_until(...)` calls rather than
//! threading four parameters through every helper call by hand.
//! [`Device::drive_until`] is the one open-ended ("keep ticking till X is
//! true") loop shape in this file — bounded at [`MAX_TICKS`], panicking
//! with the caller-supplied `loop_name` if the condition never becomes true,
//! per this piece's own working-rule ("every drive loop... has an iteration
//! cap that panics naming the loop"). [`Device::drive_n`] is the other
//! shape, used for bootstrap: a fixed, small tick count, already bounded by
//! construction.

#[allow(dead_code)]
mod support;

use mlc_messenger::store::InsecurePlainCodecForTests;
use mlc_messenger::wire::Membership;
use mlc_messenger::{
    CoreConfig, CreateRoomKind, DeviceId, ItemContent, Jitter, MessageKind, MessengerCommand, MessengerCore,
    MessengerEvent, OutgoingMessage, OutgoingRequestKind, RoomId, SendState, TimelineItem, UserId,
};
use support::fake_server::FakeServer;

type TestCore = MessengerCore<InsecurePlainCodecForTests>;

/// Bound on every open-ended ("keep ticking till X") drive loop in this
/// file's own helpers (module doc).
const MAX_TICKS: usize = 60;

struct FixedJitter(f64);

impl Jitter for FixedJitter {
    fn next_unit(&mut self) -> f64 {
        self.0
    }
}

fn core_config(user_id: &UserId, device_id: &DeviceId) -> CoreConfig {
    CoreConfig { user_id: user_id.clone(), device_id: device_id.clone(), server_name: "example.org".to_string() }
}

/// One simulated client: an identity plus its own [`MessengerCore`] — see
/// this file's own module doc.
struct Device {
    user_id: UserId,
    device_id: DeviceId,
    core: TestCore,
}

impl Device {
    fn new(user: &str, device: &str) -> Self {
        let user_id = UserId::parse(format!("@{user}:example.org")).expect("valid user id");
        let device_id = DeviceId::parse(device).expect("valid device id");
        let core = MessengerCore::open(Vec::new(), InsecurePlainCodecForTests, core_config(&user_id, &device_id), 0, Box::new(FixedJitter(0.0)))
            .expect("open succeeds on a brand-new device");
        Self { user_id, device_id, core }
    }

    fn flush_and_ack(&mut self) {
        while let Some(batch) = self.core.take_flush_batch() {
            self.core.ack_flush(batch.id);
        }
    }

    /// One full shell tick against `server` — see `e2e_core.rs`'s own
    /// `drive_tick` doc for why this calls
    /// [`MessengerCore::releasable_requests`] exactly once and always
    /// drives every request it returns through to a response.
    fn drive_one(&mut self, server: &mut FakeServer, now_ms: i64) -> Vec<MessengerEvent> {
        self.flush_and_ack();
        let requests = self.core.releasable_requests(now_ms);
        let mut events = Vec::new();
        for request in requests {
            let response = server.dispatch(&self.user_id, &self.device_id, &request);
            events.extend(self.core.on_response(request.id.clone(), response, now_ms));
        }
        self.flush_and_ack();
        events
    }

    /// A fixed, small number of ticks — bounded by construction, no
    /// condition to fail on (module doc). Used for bootstrap, where "enough
    /// ticks for `/keys/upload` to land" is a known small constant
    /// (`e2e_core.rs`'s own precedent).
    fn drive_n(&mut self, server: &mut FakeServer, now_ms: i64, n: usize) -> Vec<MessengerEvent> {
        let mut events = Vec::new();
        for _ in 0..n {
            events.extend(self.drive_one(server, now_ms));
        }
        events
    }

    /// Ticks until `condition` holds, capped at [`MAX_TICKS`] — panics
    /// naming `loop_name` if it never does (module doc).
    fn drive_until(
        &mut self,
        server: &mut FakeServer,
        now_ms: i64,
        loop_name: &str,
        mut condition: impl FnMut(&TestCore) -> bool,
    ) -> Vec<MessengerEvent> {
        let mut events = Vec::new();
        for _ in 0..MAX_TICKS {
            events.extend(self.drive_one(server, now_ms));
            if condition(&self.core) {
                return events;
            }
        }
        panic!("{loop_name}: condition not met after {MAX_TICKS} ticks");
    }

    /// This device's own bootstrap: enough ticks for `/keys/upload` to land
    /// on `server` (`e2e_core.rs`'s own "a handful of ticks" precedent).
    fn bootstrap(&mut self, server: &mut FakeServer) {
        self.drive_n(server, 0, 8);
    }
}

/// `true` once `room_id`'s timeline holds an item whose plaintext body is
/// `body` — the non-panicking check every [`Device::drive_until`] predicate
/// waiting on a RECEIVER's own delivery must use instead of [`find_by_body`]
/// (a receiver may not have a `room_id` timeline entry at all yet — no
/// timeline event has arrived for it — until the very message being waited
/// on lands; `find_by_body` panicking on that is correct for a
/// post-condition assertion, but wrong for a predicate that is expected to
/// return `false` on every early tick).
fn has_body(core: &TestCore, room_id: &RoomId, body: &str) -> bool {
    core.timeline(room_id).is_some_and(|timeline| timeline.items().iter().any(|item| item_body(item) == Some(body)))
}

/// The one item in `room_id`'s timeline whose plaintext body is `body` —
/// works for [`mlc_messenger::ItemContent::Text`]/`Notice`/`Emote` alike.
/// Panics if `room_id` is unknown or no such item exists — use
/// [`has_body`] instead inside a [`Device::drive_until`] predicate.
fn find_by_body<'a>(core: &'a TestCore, room_id: &RoomId, body: &str) -> &'a TimelineItem {
    core.timeline(room_id)
        .expect("room present")
        .items()
        .iter()
        .find(|item| item_body(item).is_some_and(|b| b == body))
        .unwrap_or_else(|| panic!("no timeline item in {room_id:?} with body {body:?}"))
}

fn item_body(item: &TimelineItem) -> Option<&str> {
    match &item.content {
        ItemContent::Text(inner) | ItemContent::Notice(inner) | ItemContent::Emote(inner) => Some(inner.body.as_str()),
        _ => None,
    }
}

fn text_message(body: &str) -> OutgoingMessage {
    OutgoingMessage { kind: MessageKind::Text, body: body.to_string(), reply_to: None, edit_of: None }
}

fn is_joined(core: &TestCore, room_id: &RoomId, user_id: &UserId) -> bool {
    core.room_state(room_id).and_then(|state| state.members.get(user_id)).map(|member| &member.membership)
        == Some(&Membership::Join)
}

/// Bootstraps both `a` and `b`, has `a` create a DM inviting `b`, and drives
/// both until `b` has joined — the shared setup every encrypted-DM
/// send-pipeline test in this file starts from. Both devices are bootstrapped
/// BEFORE the room is created so this fake server's own
/// `broadcast_membership_awareness` (fired at `/createRoom`/`/join` time)
/// has an already-registered device to notify on each side (`e2e_core.rs`'s
/// own module doc explains why this ordering matters: a device that does not
/// exist yet cannot be queued a `device_lists.changed` notification).
fn create_dm(a: &mut Device, b: &mut Device, server: &mut FakeServer) -> RoomId {
    a.bootstrap(server);
    b.bootstrap(server);

    a.core.dispatch(MessengerCommand::CreateRoom { kind: CreateRoomKind::Dm { peer: b.user_id.clone() } }, 0).expect("dispatch create_room");
    a.drive_until(server, 0, "create_dm: alice's own room appears", |core| core.room_ids().next().is_some());
    let room_id = a.core.room_ids().next().cloned().expect("room present after drive_until");

    b.drive_until(server, 0, "create_dm: bob sees the invite", |core| core.room_ids().any(|id| id == &room_id));
    b.core.dispatch(MessengerCommand::JoinRoom { room_id: room_id.clone() }, 0).expect("dispatch join_room");
    let b_user_id = b.user_id.clone();
    b.drive_until(server, 0, "create_dm: bob's own join lands", |core| is_joined(core, &room_id, &b_user_id));

    room_id
}

#[test]
fn e2e_two_cores_exchange_messages_in_an_encrypted_dm() {
    let mut server = FakeServer::new();
    let mut alice = Device::new("alice", "ALICE1");
    let mut bob = Device::new("bob", "BOB1");
    let room_id = create_dm(&mut alice, &mut bob, &mut server);
    assert!(alice.core.room_state(&room_id).expect("room present").encryption.is_some(), "a DM is always encrypted");

    alice.core.dispatch(MessengerCommand::SendMessage { room_id: room_id.clone(), message: text_message("hi from alice"), txn_id: None }, 0)
        .expect("dispatch send_message");
    alice.drive_until(&mut server, 0, "alice's send reaches Sent", |core| {
        find_by_body(core, &room_id, "hi from alice").send_state == SendState::Sent
    });
    bob.drive_until(&mut server, 0, "bob receives alice's message", |core| has_body(core, &room_id, "hi from alice"));
    assert_eq!(item_body(find_by_body(&bob.core, &room_id, "hi from alice")), Some("hi from alice"));

    bob.core.dispatch(MessengerCommand::SendMessage { room_id: room_id.clone(), message: text_message("hi from bob"), txn_id: None }, 0)
        .expect("dispatch send_message");
    bob.drive_until(&mut server, 0, "bob's send reaches Sent", |core| {
        find_by_body(core, &room_id, "hi from bob").send_state == SendState::Sent
    });
    alice.drive_until(&mut server, 0, "alice receives bob's message", |core| has_body(core, &room_id, "hi from bob"));
    assert_eq!(item_body(find_by_body(&alice.core, &room_id, "hi from bob")), Some("hi from bob"));
}

#[test]
fn e2e_room_key_is_shared_before_the_message_is_sent() {
    let mut server = FakeServer::new();
    let mut alice = Device::new("alice", "ALICE1");
    let mut bob = Device::new("bob", "BOB1");
    let room_id = create_dm(&mut alice, &mut bob, &mut server);

    alice
        .core
        .dispatch(MessengerCommand::SendMessage { room_id: room_id.clone(), message: text_message("secret"), txn_id: None }, 0)
        .expect("dispatch send_message");
    alice.drive_until(&mut server, 0, "alice's send reaches Sent", |core| {
        find_by_body(core, &room_id, "secret").send_state == SendState::Sent
    });

    let log = server.request_log();
    let share_idx = log
        .iter()
        .position(|kind| *kind == OutgoingRequestKind::SendToDevice)
        .expect("a room-key share happened before the first encrypted send");
    let send_idx =
        log.iter().position(|kind| *kind == OutgoingRequestKind::RoomSend).expect("the room event itself was sent");
    assert!(share_idx < send_idx, "the room-key share must land before the room event it unblocks");
}

#[test]
fn e2e_three_cores_group_kick_excludes_leaver_from_new_messages() {
    let mut server = FakeServer::new();
    let mut alice = Device::new("alice", "ALICE1");
    let mut bob = Device::new("bob", "BOB1");
    let mut carol = Device::new("carol", "CAROL1");
    alice.bootstrap(&mut server);
    bob.bootstrap(&mut server);
    carol.bootstrap(&mut server);

    let kind = CreateRoomKind::Group {
        name: "trio".to_string(),
        invite: vec![bob.user_id.clone(), carol.user_id.clone()],
        members_can_invite: true,
    };
    alice.core.dispatch(MessengerCommand::CreateRoom { kind }, 0).expect("dispatch create_room");
    alice.drive_until(&mut server, 0, "alice's own room appears", |core| core.room_ids().next().is_some());
    let room_id = alice.core.room_ids().next().cloned().expect("room present");

    for member in [&mut bob, &mut carol] {
        member.drive_until(&mut server, 0, "member sees the invite", |core| core.room_ids().any(|id| id == &room_id));
        member.core.dispatch(MessengerCommand::JoinRoom { room_id: room_id.clone() }, 0).expect("dispatch join_room");
        let member_user_id = member.user_id.clone();
        member.drive_until(&mut server, 0, "member's own join lands", |core| is_joined(core, &room_id, &member_user_id));
    }

    alice
        .core
        .dispatch(MessengerCommand::SendMessage { room_id: room_id.clone(), message: text_message("hello all"), txn_id: None }, 0)
        .expect("dispatch send_message");
    alice.drive_until(&mut server, 0, "alice's first send reaches Sent", |core| {
        find_by_body(core, &room_id, "hello all").send_state == SendState::Sent
    });
    bob.drive_until(&mut server, 0, "bob decrypts the first message", |core| has_body(core, &room_id, "hello all"));
    carol.drive_until(&mut server, 0, "carol decrypts the first message", |core| has_body(core, &room_id, "hello all"));

    alice.core.dispatch(MessengerCommand::Kick { room_id: room_id.clone(), user_id: carol.user_id.clone(), reason: None }, 0)
        .expect("dispatch kick");
    let carol_user_id = carol.user_id.clone();
    alice.drive_until(&mut server, 0, "alice observes carol's own leave", |core| {
        core.room_state(&room_id).and_then(|state| state.members.get(&carol_user_id)).map(|member| &member.membership)
            == Some(&Membership::Leave)
    });

    alice
        .core
        .dispatch(MessengerCommand::SendMessage { room_id: room_id.clone(), message: text_message("bye carol"), txn_id: None }, 0)
        .expect("dispatch send_message");
    alice.drive_until(&mut server, 0, "alice's second send reaches Sent", |core| {
        find_by_body(core, &room_id, "bye carol").send_state == SendState::Sent
    });
    bob.drive_until(&mut server, 0, "bob decrypts the second message", |core| has_body(core, &room_id, "bye carol"));

    // Carol never receives it at all: once kicked, this fake server's own
    // `/sync` never again delivers this room's `join`/`invite` section to
    // her (`FakeServer`'s own module doc's `handle_sync` -- a `Leave`
    // membership falls into the match's `_ => {}` arm). A handful of ticks
    // is enough to prove absence without an open-ended wait.
    carol.drive_n(&mut server, 0, 6);
    assert!(
        carol.core.timeline(&room_id).is_none_or(|timeline| !timeline.items().iter().any(|item| item_body(item) == Some("bye carol"))),
        "the kicked member must never receive a message sent after her own kick"
    );
}
