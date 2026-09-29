//! End-to-end provider-seam test (P2): two [`MessengerCore`]s driven through
//! [`FakeServer`], each fronted by its own [`SnapshotProvider`] -- proving
//! the whole shell-driver loop [`provider`]'s own module doc describes
//! (`take_intents` -> `apply_intents` -> drive HTTP -> `build_view` ->
//! `publish`), not just the two halves in isolation. Every assertion below
//! reads through the PROVIDER (the `MessengerProvider` trait), never the
//! core directly -- that is the one thing this file exists to prove that
//! `provider.rs`'s own unit tests (which call `build_view`/`apply_intent`
//! directly) cannot.
//!
//! Reuses `e2e_send_pipeline.rs`'s own `Device`/`drive_until` shape
//! (module doc there): a fixed, small bootstrap tick count, an
//! iteration-capped "keep ticking till X" loop that panics naming the loop
//! if the condition never holds, and "never discard a non-empty
//! `releasable_requests` result" (`MessengerCore::releasable_requests`'s
//! own doc) enforced structurally by driving every request a tick returns
//! through to a response.

#[allow(dead_code)]
mod support;

use mlc_messenger::provider::{apply_intents, build_view};
use mlc_messenger::store::InsecurePlainCodecForTests;
use mlc_messenger::wire::Membership;
use mlc_messenger::{CoreConfig, CoreSecrets, DeviceId, Jitter, MessengerCore, RoomId, SnapshotProvider, UserId, SLICE_CAPABILITIES};
use mlc_messenger_model::{
    CreateRoomRequest, MessageContent, MessengerIntent, MessengerProvider, RoomKind, RoomVisibility,
    UserId as ModelUserId,
};
use support::fake_server::FakeServer;

type TestCore = MessengerCore<InsecurePlainCodecForTests>;

/// Bound on every open-ended ("keep ticking till X") drive loop in this
/// file (`e2e_send_pipeline.rs`'s own module doc, same rationale).
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

/// One simulated client: its own [`MessengerCore`] (the single writer) plus
/// the [`SnapshotProvider`] a UI would read -- every [`ProviderDevice::
/// drive_one`] tick runs the full shell-driver loop `provider`'s own module
/// doc describes, in order.
struct ProviderDevice {
    user_id: UserId,
    device_id: DeviceId,
    core: TestCore,
    provider: SnapshotProvider,
    last_seen_change_counter: u64,
}

impl ProviderDevice {
    fn new(user: &str, device: &str) -> Self {
        let user_id = UserId::parse(format!("@{user}:example.org")).expect("valid user id");
        let device_id = DeviceId::parse(device).expect("valid device id");
        let core = MessengerCore::open(
            Vec::new(),
            InsecurePlainCodecForTests,
            core_config(&user_id, &device_id),
            CoreSecrets::default(),
            0,
            Box::new(FixedJitter(0.0)),
        )
        .expect("open succeeds on a brand-new device");
        Self { user_id, device_id, core, provider: SnapshotProvider::new(SLICE_CAPABILITIES), last_seen_change_counter: 0 }
    }

    fn model_user_id(&self) -> ModelUserId {
        ModelUserId::from(self.user_id.as_str())
    }

    fn flush_and_ack(&mut self) {
        while let Some(batch) = self.core.take_flush_batch() {
            self.core.ack_flush(batch.id);
        }
    }

    /// One full shell tick: drain the UI's own intents into the core,
    /// drive HTTP against `server`, and publish a fresh view iff the
    /// core's own `change_counter` moved -- exactly the loop `provider`'s
    /// own module doc describes.
    fn drive_one(&mut self, server: &mut FakeServer, now_ms: i64) {
        let intents = self.provider.take_intents();
        if !intents.is_empty() {
            let last_error = apply_intents(&mut self.core, intents, now_ms);
            self.provider.set_last_error(last_error);
        }

        self.flush_and_ack();
        let requests = self.core.releasable_requests(now_ms);
        for request in requests {
            let response = server.dispatch(&self.user_id, &self.device_id, &request);
            self.core.on_response(request.id.clone(), response, now_ms);
        }
        self.flush_and_ack();

        let change_counter = self.core.change_counter();
        if change_counter != self.last_seen_change_counter {
            self.provider.publish(build_view(&self.core, now_ms));
            self.last_seen_change_counter = change_counter;
        }
    }

    fn drive_n(&mut self, server: &mut FakeServer, now_ms: i64, n: usize) {
        for _ in 0..n {
            self.drive_one(server, now_ms);
        }
    }

    fn drive_until(&mut self, server: &mut FakeServer, now_ms: i64, loop_name: &str, mut condition: impl FnMut(&SnapshotProvider) -> bool) {
        for _ in 0..MAX_TICKS {
            self.drive_one(server, now_ms);
            if condition(&self.provider) {
                return;
            }
        }
        panic!("{loop_name}: condition not met after {MAX_TICKS} ticks");
    }

    /// Same shape as [`Self::drive_until`], gated on the underlying core
    /// instead of the published view -- see [`is_joined`]'s own doc for
    /// why one orchestration gate in this file reads the core directly.
    fn drive_until_core(&mut self, server: &mut FakeServer, now_ms: i64, loop_name: &str, mut condition: impl FnMut(&TestCore) -> bool) {
        for _ in 0..MAX_TICKS {
            self.drive_one(server, now_ms);
            if condition(&self.core) {
                return;
            }
        }
        panic!("{loop_name}: condition not met after {MAX_TICKS} ticks");
    }

    fn bootstrap(&mut self, server: &mut FakeServer) {
        self.drive_n(server, 0, 8);
    }
}

fn core_room_id(model_room_id: &mlc_messenger_model::RoomId) -> RoomId {
    RoomId::parse(model_room_id.as_str()).expect("a provider-issued room id is always well-formed")
}

/// Whether `user_id` currently holds a `Join` membership in `room_id`,
/// checked directly against the core -- used only to gate a drive loop
/// (the same "the core, not the provider, is the orchestration signal for
/// timing a drive_until" split `e2e_send_pipeline.rs`'s own `is_joined`
/// makes; every user-visible assertion in this file still reads through
/// the provider).
fn is_joined(core: &TestCore, room_id: &RoomId, user_id: &UserId) -> bool {
    core.room_state(room_id).and_then(|state| state.members.get(user_id)).map(|member| &member.membership) == Some(&Membership::Join)
}

#[test]
fn e2e_dm_send_and_read_receipt_through_the_provider_seam() {
    let mut server = FakeServer::new();
    let mut alice = ProviderDevice::new("alice", "ALICE1");
    let mut bob = ProviderDevice::new("bob", "BOB1");
    alice.bootstrap(&mut server);
    bob.bootstrap(&mut server);

    // Alice creates a DM with Bob -- entirely through the intent seam.
    let bob_model_id = bob.model_user_id();
    alice.provider.enqueue(MessengerIntent::CreateRoom(CreateRoomRequest {
        kind: RoomKind::Direct,
        title: None,
        description: None,
        member_ids: vec![bob_model_id],
        members_can_add: false,
        visibility: RoomVisibility::Private,
    }));
    alice.drive_until(&mut server, 0, "alice's own DM appears in her provider view", |provider| !provider.rooms().is_empty());
    let room_id = alice.provider.rooms().first().expect("a room after drive_until").id.clone();

    // Bob sees the invite in HIS OWN provider view, then joins.
    bob.drive_until(&mut server, 0, "bob sees the DM invite in his provider view", |provider| provider.room(&room_id).is_some());
    bob.provider.enqueue(MessengerIntent::JoinRoom { room: room_id.clone() });
    let bob_user_id = bob.user_id.clone();
    let bob_room_id = core_room_id(&room_id);
    bob.drive_until_core(&mut server, 0, "bob's join lands", |core| is_joined(core, &bob_room_id, &bob_user_id));

    // Alice sends -- read back through HER OWN provider once Sent.
    alice.provider.enqueue(MessengerIntent::SendText { room: room_id.clone(), text: "hi bob".to_string(), reply_to: None });
    alice.drive_until(&mut server, 0, "alice's send reaches Sent through her own provider", |provider| {
        provider.timeline(&room_id).iter().any(|item| {
            item.content == MessageContent::Text("hi bob".to_string()) && item.send_state == mlc_messenger_model::SendState::Sent
        })
    });

    // Bob's provider view shows the room and the DECRYPTED text -- the
    // seam's whole point: Bob never touched a core method directly.
    bob.drive_until(&mut server, 0, "bob's provider view shows alice's decrypted message", |provider| {
        provider.timeline(&room_id).iter().any(|item| item.content == MessageContent::Text("hi bob".to_string()))
    });
    let bob_summary = bob.provider.room(&room_id).expect("room present in bob's own view");
    assert_eq!(bob_summary.kind, RoomKind::Direct);
    assert!(bob_summary.is_encrypted, "a DM is always encrypted");

    // Alice marks her own sent message read -- Bob's provider view must
    // then show ALICE's own read receipt against it.
    let sent_event_id = alice
        .provider
        .timeline(&room_id)
        .into_iter()
        .find(|item| item.content == MessageContent::Text("hi bob".to_string()))
        .expect("alice's own confirmed send")
        .event_id;
    alice.provider.enqueue(MessengerIntent::MarkRead { room: room_id.clone(), event_id: sent_event_id });
    // `MarkRead` mutates nothing locally (no optimistic state, this
    // crate's own doc on `MessengerCommand::MarkRead`) -- a fixed, small
    // tick count is enough to hand the `/read_markers` PUT to the server,
    // the same "known small constant" bootstrap convention `Device::
    // bootstrap` already uses.
    alice.drive_n(&mut server, 0, 4);
    assert!(alice.provider.last_error().is_none(), "the read marker was accepted");

    let alice_model_id = alice.model_user_id();
    bob.drive_until(&mut server, 0, "bob's provider view shows alice's own read receipt", |provider| {
        provider
            .timeline(&room_id)
            .into_iter()
            .find(|item| item.content == MessageContent::Text("hi bob".to_string()))
            .is_some_and(|item| item.read_by.contains(&alice_model_id))
    });
}
