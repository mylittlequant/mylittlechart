//! End-to-end legacy-DM migration test (M11), driven entirely through the
//! [`MessengerCore`] kernel API for the receiving side (alice) -- see
//! `e2e_core.rs`'s own module doc for why the SENDING peer below (bob)
//! still drives the managers directly for the Megolm half of this test
//! (`PeerClient`, a trimmed copy of that file's own type; each `tests/*.rs`
//! file is its own crate, so this cannot be shared without a
//! support-module change outside this piece's scope).

// See `e2e_olm_1to1.rs`'s own `mod support;` for why this allow is here --
// `tests/support` is shared across every integration-test binary in this
// crate, each compiled independently, so a helper this file needs but a
// sibling file's own tests don't (or vice versa) is `dead_code`'s known
// per-binary false positive, not actually unused code.
#[allow(dead_code)]
mod support;

use base64::Engine;
use mlc_messenger::store::InsecurePlainCodecForTests;
use mlc_messenger::wire::events::RoomEncryptionContent;
use mlc_messenger::wire::RoomEncryptedContent;
use mlc_messenger::{
    CoreConfig, CoreSecrets, DeviceId, DeviceTracker, GroupSessionManager, ItemContent, Jitter, MessengerCore,
    MessengerEvent, OlmAccountState, OlmSessionManager, OutgoingRequest, RequestId, RoomId, Store, StoredDevice,
    TxnId, UserId,
};
use mlc_vault::dm::{conversation_key, fingerprint, message_aad, seal_message, DmIdentity};
use serde_json::Value;
use std::collections::BTreeSet;
use support::fake_server::FakeServer;

type TestCore = MessengerCore<InsecurePlainCodecForTests>;

struct FixedJitter(f64);

impl Jitter for FixedJitter {
    fn next_unit(&mut self) -> f64 {
        self.0
    }
}

fn b64(bytes: impl AsRef<[u8]>) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn core_config(user_id: &UserId, device_id: &DeviceId) -> CoreConfig {
    CoreConfig { user_id: user_id.clone(), device_id: device_id.clone(), server_name: "example.org".to_string() }
}

fn flush_and_ack(core: &mut TestCore) {
    while let Some(batch) = core.take_flush_batch() {
        core.ack_flush(batch.id);
    }
}

fn drive_tick(core: &mut TestCore, server: &mut FakeServer, user_id: &UserId, device_id: &DeviceId, now_ms: i64) -> Vec<MessengerEvent> {
    flush_and_ack(core);
    let requests = core.releasable_requests(now_ms);
    let mut events = Vec::new();
    for request in requests {
        let response = server.dispatch(user_id, device_id, &request);
        events.extend(core.on_response(request.id.clone(), response, now_ms));
    }
    flush_and_ack(core);
    events
}

fn legacy_dm_key_event(state_key: &UserId, sender: &UserId, ts: i64, public: [u8; 32], conversation_id: i64) -> Value {
    serde_json::json!({
        "event_id": format!("$legacykey-{state_key}:example.org"),
        "type": "org.mylittlechart.legacy_dm_key",
        "sender": sender.as_str(),
        "origin_server_ts": ts,
        "state_key": state_key.as_str(),
        "content": { "public_key_b64": b64(public), "legacy_conversation_id": conversation_id },
    })
}

fn legacy_dm_event(event_id: &str, sender: &UserId, ts: i64, legacy_message_id: &str, conversation_id: i64, nonce: [u8; 12], ciphertext: Vec<u8>) -> Value {
    serde_json::json!({
        "event_id": event_id,
        "type": "org.mylittlechart.legacy_dm",
        "sender": sender.as_str(),
        "origin_server_ts": ts,
        "content": {
            "legacy_message_id": legacy_message_id,
            "legacy_conversation_id": conversation_id,
            "nonce_b64": b64(nonce),
            "ciphertext_b64": b64(ciphertext),
        },
    })
}

/// One simulated manager-level peer -- a trimmed copy of `e2e_core.rs`'s
/// own `PeerClient` (this file's own module doc explains why).
struct PeerClient {
    user_id: UserId,
    device_id: DeviceId,
    store: Store<InsecurePlainCodecForTests>,
    account: OlmAccountState,
    request_seq: u64,
}

impl PeerClient {
    fn new(user: &str, device: &str) -> Self {
        let user_id = UserId::parse(format!("@{user}:example.org")).expect("valid user id");
        let device_id = DeviceId::parse(device).expect("valid device id");
        let mut store = Store::new(device_id.clone(), InsecurePlainCodecForTests);
        let account = OlmAccountState::load_or_create(&mut store).expect("create the device's Olm account");
        Self { user_id, device_id, store, account, request_seq: 0 }
    }

    fn next_request_id(&mut self) -> RequestId {
        self.request_seq += 1;
        RequestId::next(self.request_seq)
    }

    fn next_txn_id(&mut self) -> TxnId {
        self.request_seq += 1;
        TxnId::new(self.request_seq)
    }

    fn bootstrap(&mut self, server: &mut FakeServer) {
        self.account.on_sync_counts(&mut self.store, 0, &[]).expect("generate OTKs and a fallback key");
        let request_id = self.next_request_id();
        let Some(request) =
            self.account.keys_upload_request(request_id, &self.user_id, &self.device_id).expect("build the request")
        else {
            return;
        };
        let response = server.dispatch(&self.user_id, &self.device_id, &request);
        assert_eq!(response.status, 200, "keys/upload succeeds");
        self.account.on_keys_upload_response(&mut self.store).expect("mark keys published");
    }

    fn query_devices(&mut self, server: &mut FakeServer, peer: &UserId) {
        DeviceTracker::on_device_lists(&mut self.store, std::slice::from_ref(peer), &[]).expect("mark peer outdated");
        let request_id = self.next_request_id();
        let Some(request) = DeviceTracker::keys_query_request(&self.store, request_id).expect("build the request") else {
            return;
        };
        let response = server.dispatch(&self.user_id, &self.device_id, &request);
        assert_eq!(response.status, 200, "keys/query succeeds");
        DeviceTracker::on_keys_query_response(&mut self.store, &response.body).expect("apply the response");
    }

    fn devices_of(&self, peer: &UserId) -> Vec<StoredDevice> {
        DeviceTracker::devices_for_user(&self.store, peer).expect("no store error")
    }

    fn claim(&mut self, server: &mut FakeServer, devices: &[&StoredDevice]) {
        let request_id = self.next_request_id();
        let Some(request) = OlmSessionManager::keys_claim_request(request_id, devices) else { return };
        let response = server.dispatch(&self.user_id, &self.device_id, &request);
        assert_eq!(response.status, 200, "keys/claim succeeds");
        OlmSessionManager::on_keys_claim_response(&mut self.store, &self.account, devices, &response.body)
            .expect("apply the response");
    }

    #[allow(clippy::too_many_arguments)]
    fn share_and_send(
        &mut self,
        server: &mut FakeServer,
        room_id: &RoomId,
        member_user_ids: &BTreeSet<UserId>,
        known_devices: &[StoredDevice],
        encryption: &RoomEncryptionContent,
        now_ms: i64,
        content: Value,
    ) -> String {
        let update = GroupSessionManager::ensure_outbound_session(
            &mut self.store,
            room_id,
            &self.user_id,
            &self.device_id,
            encryption,
            member_user_ids,
            known_devices,
            now_ms,
        )
        .expect("ensure outbound session");

        if let Some(room_key_content) = &update.room_key_content {
            self.claim(server, &update.new_recipients.iter().collect::<Vec<_>>());
            for chunk in GroupSessionManager::chunk_recipients_for_send_to_device(&update.new_recipients) {
                let body = GroupSessionManager::build_room_key_send_to_device_body(
                    &mut self.store,
                    &self.account,
                    &self.user_id,
                    &self.device_id,
                    chunk,
                    room_key_content,
                )
                .expect("build the room_key to-device body");
                let request_id = self.next_request_id();
                let txn_id = self.next_txn_id();
                let request = OutgoingRequest::send_to_device(request_id, "m.room.encrypted", &txn_id, body);
                let response = server.dispatch(&self.user_id, &self.device_id, &request);
                assert_eq!(response.status, 200, "sendToDevice(m.room_key) succeeds");
            }
        }

        let sender_curve = self.account.identity_keys().curve25519.to_base64();
        let encrypted = GroupSessionManager::encrypt_event(
            &mut self.store,
            room_id,
            &sender_curve,
            &self.device_id,
            "m.room.message",
            content,
            None,
        )
        .expect("encrypt the room event");
        let wire_content = serde_json::to_value(RoomEncryptedContent::Megolm(encrypted)).expect("serializes");
        let request_id = self.next_request_id();
        let txn_id = self.next_txn_id();
        let request = OutgoingRequest::room_send(request_id, room_id, "m.room.encrypted", &txn_id, wire_content);
        let response = server.dispatch(&self.user_id, &self.device_id, &request);
        assert_eq!(response.status, 200, "room send succeeds");
        let body: Value = serde_json::from_slice(&response.body).expect("valid JSON body");
        body["event_id"].as_str().expect("event_id present").to_string()
    }
}

#[test]
fn e2e_migrated_dm_room_shows_old_messages_then_new_megolm_messages() {
    let mut server = FakeServer::new();
    let our_user = UserId::parse("@alice:example.org").expect("valid user id");
    let our_device = DeviceId::parse("ALICECORE").expect("valid device id");
    let room_id = RoomId::parse("!r:example.org").expect("valid room id");

    // Alice's own vault-derived legacy DM identity -- the SAME shape a
    // shell derives via `DmIdentity::from_vault_key` and hands to
    // `MessengerCore::open` as `CoreSecrets::legacy_dm_identity`.
    let our_identity = DmIdentity::from_vault_key(&[9u8; 32]);
    // Bob's own legacy identity is simulated as an already-published,
    // unrelated keypair (this test never needs bob's own vault).
    let peer_identity = DmIdentity::generate();
    let legacy_conversation_id = 100;

    // Seed a pre-migration message from bob to alice, sealed exactly the
    // way a pre-migration shell would have (`mlc_vault::dm::seal_message` +
    // `message_aad`, bound to the SENDER's own fingerprint -- bob's here).
    let key = conversation_key(&peer_identity.secret, &our_identity.public, &legacy_conversation_id.to_string());
    let aad = message_aad(legacy_conversation_id, &fingerprint(&peer_identity.public));
    let sealed = seal_message(&key, b"hey, this is from before the matrix migration", &aad);

    let peer = UserId::parse("@bob:example.org").expect("valid user id");
    server.queue_room_state(
        &our_user,
        &our_device,
        &room_id,
        serde_json::json!({
            "event_id": "$create:example.org", "type": "m.room.create", "sender": our_user.as_str(),
            "origin_server_ts": 1, "state_key": "", "content": { "creator": our_user.as_str() },
        }),
    );
    for (member, ts) in [(&our_user, 2), (&peer, 3)] {
        server.queue_room_state(
            &our_user,
            &our_device,
            &room_id,
            serde_json::json!({
                "event_id": format!("$member-{member}:example.org"), "type": "m.room.member", "sender": our_user.as_str(),
                "origin_server_ts": ts, "state_key": member.as_str(),
                "content": { "membership": "join", "is_direct": true },
            }),
        );
    }
    server.queue_room_state(
        &our_user,
        &our_device,
        &room_id,
        legacy_dm_key_event(&our_user, &our_user, 4, our_identity.public, legacy_conversation_id),
    );
    server.queue_room_state(
        &our_user,
        &our_device,
        &room_id,
        legacy_dm_key_event(&peer, &peer, 5, peer_identity.public, legacy_conversation_id),
    );
    server.queue_room_state(
        &our_user,
        &our_device,
        &room_id,
        serde_json::json!({
            "event_id": "$encryption:example.org", "type": "m.room.encryption", "sender": our_user.as_str(),
            "origin_server_ts": 6, "state_key": "",
            "content": { "algorithm": "m.megolm.v1.aes-sha2" },
        }),
    );
    server.queue_room_timeline(
        &our_user,
        &our_device,
        &room_id,
        legacy_dm_event("$legacy1:example.org", &peer, 7, "legacy-msg-1", legacy_conversation_id, sealed.nonce, sealed.ciphertext),
    );

    let mut core = MessengerCore::open(
        Vec::new(),
        InsecurePlainCodecForTests,
        core_config(&our_user, &our_device),
        CoreSecrets { legacy_dm_identity: Some(our_identity), ..CoreSecrets::default() },
        0,
        Box::new(FixedJitter(0.0)),
    )
    .expect("open succeeds on a brand-new device");

    for _ in 0..6 {
        drive_tick(&mut core, &mut server, &our_user, &our_device, 0);
    }

    let timeline = core.timeline(&room_id).expect("room joined via the initial sync");
    assert_eq!(timeline.items().len(), 1, "the migrated legacy message landed");
    match &timeline.items()[0].content {
        ItemContent::LegacyPlain(text) => assert_eq!(text, "hey, this is from before the matrix migration"),
        other => panic!("expected the legacy message to decrypt, got {other:?}"),
    }

    // Now bob sends a brand-new, ordinary post-migration Megolm message
    // into the very same room.
    let mut bob = PeerClient::new("bob", "BOBDEV");
    bob.bootstrap(&mut server);
    bob.query_devices(&mut server, &our_user);
    let our_devices = bob.devices_of(&our_user);
    assert_eq!(our_devices.len(), 1, "bob discovered alice's own published device");

    server.note_device_list_changed(&our_user, &our_device, &bob.user_id);
    for _ in 0..3 {
        drive_tick(&mut core, &mut server, &our_user, &our_device, 0);
    }

    let members: BTreeSet<UserId> = [bob.user_id.clone(), our_user.clone()].into_iter().collect();
    let encryption =
        RoomEncryptionContent { algorithm: "m.megolm.v1.aes-sha2".to_string(), rotation_period_ms: 604_800_000, rotation_period_msgs: 100 };
    let event_id = bob.share_and_send(
        &mut server,
        &room_id,
        &members,
        &our_devices,
        &encryption,
        0,
        serde_json::json!({ "msgtype": "m.text", "body": "hi via matrix now" }),
    );

    for event in server.room_timeline(&room_id) {
        server.queue_room_timeline(&our_user, &our_device, &room_id, event);
    }
    for _ in 0..6 {
        drive_tick(&mut core, &mut server, &our_user, &our_device, 0);
    }

    let timeline = core.timeline(&room_id).expect("room still present");
    assert_eq!(timeline.items().len(), 2, "the legacy message plus the new Megolm message");
    match &timeline.items()[0].content {
        ItemContent::LegacyPlain(text) => assert_eq!(text, "hey, this is from before the matrix migration"),
        other => panic!("expected the legacy message to still be decrypted, got {other:?}"),
    }
    let new_item =
        timeline.item_by_event_id(&mlc_messenger::EventId::parse(event_id).expect("valid event id")).expect("bob's new message is present");
    match &new_item.content {
        ItemContent::Text(text) => assert_eq!(text.body, "hi via matrix now"),
        other => panic!("expected the new message to decrypt as ordinary Megolm text, got {other:?}"),
    }
}
