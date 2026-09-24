//! An in-process, spec-shaped fake Matrix homeserver for `mlc-messenger`'s
//! own integration tests. Speaks [`OutgoingRequest`] -> [`HttpResponseDescriptor`]
//! directly -- no HTTP, no async, no sockets -- but every request/response
//! body is the real wire JSON shape the research doc's §3.2/§3.3 describe,
//! so this is a contract test harness every later piece (Megolm group
//! sessions, key backup, ...) can extend rather than a throwaway mock.
//!
//! Deliberately out of scope, and left for a later piece to add:
//! `/createRoom`, `/invite`, cross-signing, key backup -- room MEMBERSHIP
//! for the Megolm group tests (`e2e_megolm_group.rs`) is supplied directly
//! by the test as plain data, never fetched from this fake server. A
//! request of any [`OutgoingRequestKind`] this fake does not implement gets
//! a clean `501` rather than a panic, so extending this file never requires
//! touching every call site that already works.

use mlc_messenger::wire::{HttpResponseDescriptor, OutgoingRequest, OutgoingRequestKind};
use mlc_messenger::{DeviceId, RoomId, UserId};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// One device's server-side state: its published `device_keys`, its pool
/// of unclaimed regular one-time keys, and its current fallback key.
#[derive(Default, Clone)]
struct DeviceState {
    device_keys: Option<Value>,
    /// `signed_curve25519:{key_id}` -> the signed key object. Regular
    /// one-time keys are exactly-once: [`FakeServer::handle_keys_claim`]
    /// removes an entry the moment it is claimed.
    one_time_keys: BTreeMap<String, Value>,
    /// The current fallback key, if any -- unlike a regular one-time key,
    /// claiming it does not remove it (real Matrix semantics: a fallback
    /// key stays claimable, potentially by more than one claimant, until
    /// the device replaces it with a new upload). `fallback_used` only
    /// tracks whether `device_unused_fallback_key_types` should still
    /// report it as unused.
    fallback: Option<(String, Value)>,
    fallback_used: bool,
}

/// One device's `/sync` to-device mailbox: real Matrix since-token
/// semantics (spec, and this project's own server's P2 piece), not a
/// drain-on-read queue -- a `/sync` response is only ever safe to forget
/// once a LATER request names a `since` at or past it, because the
/// response naming that later `since` is the client's own proof it
/// durably received everything before it. Until then, the exact same
/// events must be redelivered verbatim on a retried `since` (a lost
/// response, a crashed client, ...).
///
/// `base_index` is the global position of `events[0]`; `events` holds
/// every event from `base_index` onward that has not yet been pruned.
/// `/sync`'s own `next_batch` token is always `"s{base_index + events.len()}"`.
#[derive(Default)]
struct SyncMailbox {
    base_index: usize,
    events: Vec<Value>,
}

fn parse_sync_token(token: &str) -> Option<usize> {
    token.strip_prefix('s').and_then(|rest| rest.parse().ok())
}

/// One room's server-stored timeline (`PUT /rooms/{roomId}/send/...`) --
/// the "bare minimum" the Megolm group tests need: append-only, each event
/// gets a server-assigned id, nothing else (no pagination, no state
/// events, no `/sync` room section -- a test reads it back directly via
/// [`FakeServer::room_timeline`]).
#[derive(Default)]
struct RoomTimeline {
    events: Vec<Value>,
    next_event_seq: u64,
}

fn json_response(status: u16, value: &Value) -> HttpResponseDescriptor {
    HttpResponseDescriptor { status, body: serde_json::to_vec(value).expect("a serde_json::Value always serializes") }
}

/// A small, local, test-only percent-decoder for path segments this fake
/// server needs to recover from a request path (currently just a room id
/// out of a `/rooms/{room_id}/send/...` path). `mlc_messenger::wire`'s own
/// implementation is crate-private, and this fake server compiles as a
/// separate integration-test crate, so it cannot reuse it.
fn percent_decode_minimal(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&segment[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_default()
}

/// Extracts `(room_id, event_type)` from
/// `/_matrix/client/v3/rooms/{room_id}/send/{event_type}/{txn_id}`.
fn parse_room_send_path(path: &str) -> Option<(RoomId, String)> {
    let segments: Vec<&str> = path.split('/').collect();
    let rooms_idx = segments.iter().position(|segment| *segment == "rooms")?;
    let send_idx = segments.iter().position(|segment| *segment == "send")?;
    let room_id_raw = segments.get(rooms_idx + 1)?;
    let event_type = (*segments.get(send_idx + 1)?).to_string();
    let room_id = RoomId::parse(percent_decode_minimal(room_id_raw)).ok()?;
    Some((room_id, event_type))
}

/// An in-process fake homeserver. See the module doc.
#[derive(Default)]
pub struct FakeServer {
    devices: BTreeMap<(UserId, DeviceId), DeviceState>,
    to_device_mailboxes: BTreeMap<(UserId, DeviceId), SyncMailbox>,
    pending_device_list_changes: BTreeMap<(UserId, DeviceId), Vec<UserId>>,
    room_timelines: BTreeMap<RoomId, RoomTimeline>,
}

/// Extracts the `{event_type}` path component from
/// `/_matrix/client/v3/sendToDevice/{event_type}/{txn_id}`. Every event
/// type this crate's own builders ever put there
/// (`percent_encode_segment`'s unreserved set includes `.`) survives
/// unescaped, so a plain path split is enough for this fake.
fn event_type_from_send_to_device_path(path: &str) -> String {
    let segments: Vec<&str> = path.split('/').collect();
    let index = segments.iter().position(|segment| *segment == "sendToDevice").expect("a sendToDevice path segment");
    segments[index + 1].to_string()
}

impl FakeServer {
    /// Builds an empty fake server: no users, no devices, nothing queued.
    pub fn new() -> Self {
        Self::default()
    }

    fn device_mut(&mut self, user_id: &UserId, device_id: &DeviceId) -> &mut DeviceState {
        self.devices.entry((user_id.clone(), device_id.clone())).or_default()
    }

    /// Dispatches one [`OutgoingRequest`] as if authenticated as
    /// `(as_user, as_device)` (this fake has no access-token concept --
    /// the caller supplies the authenticated identity directly).
    pub fn dispatch(&mut self, as_user: &UserId, as_device: &DeviceId, request: &OutgoingRequest) -> HttpResponseDescriptor {
        match request.kind {
            OutgoingRequestKind::KeysUpload => self.handle_keys_upload(as_user, as_device, request),
            OutgoingRequestKind::KeysQuery => self.handle_keys_query(request),
            OutgoingRequestKind::KeysClaim => self.handle_keys_claim(request),
            OutgoingRequestKind::SendToDevice => self.handle_send_to_device(as_user, request),
            OutgoingRequestKind::Sync => self.handle_sync(as_user, as_device, request),
            OutgoingRequestKind::RoomSend => self.handle_room_send(as_user, request),
            _ => json_response(
                501,
                &serde_json::json!({ "errcode": "M_UNRECOGNIZED", "error": "fake server: unsupported request kind" }),
            ),
        }
    }

    /// Queues a `device_lists.changed` entry for `(for_user, for_device)`'s
    /// next `/sync` -- lets a test drive device-list re-tracking through
    /// the real `/sync` -> `DeviceTracker::on_device_lists` path instead of
    /// poking the store directly.
    pub fn note_device_list_changed(&mut self, for_user: &UserId, for_device: &DeviceId, changed_user: &UserId) {
        self.pending_device_list_changes
            .entry((for_user.clone(), for_device.clone()))
            .or_default()
            .push(changed_user.clone());
    }

    /// Corrupts the signature of one of `(user_id, device_id)`'s currently
    /// unclaimed regular one-time keys in place -- simulates a corrupted
    /// upload or an in-flight tamper, for a test that must prove a
    /// mis-signed claimed key is refused rather than trusted.
    pub fn corrupt_one_time_key_signature(&mut self, user_id: &UserId, device_id: &DeviceId) {
        let Some(state) = self.devices.get_mut(&(user_id.clone(), device_id.clone())) else { return };
        let Some((_, value)) = state.one_time_keys.iter_mut().next() else { return };
        corrupt_first_signature(value, user_id);
    }

    fn handle_keys_upload(&mut self, as_user: &UserId, as_device: &DeviceId, request: &OutgoingRequest) -> HttpResponseDescriptor {
        let body = request.body.clone().unwrap_or_else(|| Value::Object(Map::new()));
        let state = self.device_mut(as_user, as_device);

        if let Some(device_keys) = body.get("device_keys") {
            state.device_keys = Some(device_keys.clone());
        }
        if let Some(one_time_keys) = body.get("one_time_keys").and_then(Value::as_object) {
            for (key_id, value) in one_time_keys {
                state.one_time_keys.insert(key_id.clone(), value.clone());
            }
        }
        if let Some(fallback_keys) = body.get("fallback_keys").and_then(Value::as_object) {
            if let Some((key_id, value)) = fallback_keys.iter().next() {
                state.fallback = Some((key_id.clone(), value.clone()));
                state.fallback_used = false;
            }
        }

        let count = state.one_time_keys.len() as u64;
        json_response(200, &serde_json::json!({ "one_time_key_counts": { "signed_curve25519": count } }))
    }

    fn handle_keys_query(&self, request: &OutgoingRequest) -> HttpResponseDescriptor {
        let body = request.body.clone().unwrap_or_else(|| Value::Object(Map::new()));
        let requested_users = body.get("device_keys").and_then(Value::as_object).cloned().unwrap_or_default();

        let mut device_keys_out = Map::new();
        for user_key in requested_users.keys() {
            let mut by_device = Map::new();
            for ((user_id, device_id), state) in &self.devices {
                if user_id.as_str() == user_key.as_str() {
                    if let Some(device_keys) = &state.device_keys {
                        by_device.insert(device_id.as_str().to_string(), device_keys.clone());
                    }
                }
            }
            device_keys_out.insert(user_key.clone(), Value::Object(by_device));
        }

        json_response(200, &serde_json::json!({ "device_keys": device_keys_out, "failures": {} }))
    }

    fn handle_keys_claim(&mut self, request: &OutgoingRequest) -> HttpResponseDescriptor {
        let body = request.body.clone().unwrap_or_else(|| Value::Object(Map::new()));
        let requested = body.get("one_time_keys").and_then(Value::as_object).cloned().unwrap_or_default();

        let mut claimed_out = Map::new();
        for (user_str, by_device_value) in &requested {
            let Ok(user_id) = UserId::parse(user_str.clone()) else { continue };
            let Some(by_device) = by_device_value.as_object() else { continue };

            let mut claimed_devices = Map::new();
            for device_str in by_device.keys() {
                let Ok(device_id) = DeviceId::parse(device_str.clone()) else { continue };
                let Some(state) = self.devices.get_mut(&(user_id.clone(), device_id)) else { continue };

                let claimed = if let Some((key_id, value)) = state.one_time_keys.pop_first() {
                    Some((key_id, value))
                } else if let Some((key_id, value)) = &state.fallback {
                    state.fallback_used = true;
                    Some((key_id.clone(), value.clone()))
                } else {
                    None
                };

                if let Some((key_id, value)) = claimed {
                    let mut one = Map::new();
                    one.insert(key_id, value);
                    claimed_devices.insert(device_str.clone(), Value::Object(one));
                }
            }

            if !claimed_devices.is_empty() {
                claimed_out.insert(user_str.clone(), Value::Object(claimed_devices));
            }
        }

        json_response(200, &serde_json::json!({ "one_time_keys": claimed_out }))
    }

    fn handle_send_to_device(&mut self, as_user: &UserId, request: &OutgoingRequest) -> HttpResponseDescriptor {
        let event_type = event_type_from_send_to_device_path(&request.path);
        let body = request.body.clone().unwrap_or_else(|| Value::Object(Map::new()));
        let messages = body.get("messages").and_then(Value::as_object).cloned().unwrap_or_default();

        for (user_str, by_device_value) in &messages {
            let Ok(user_id) = UserId::parse(user_str.clone()) else { continue };
            let Some(by_device) = by_device_value.as_object() else { continue };
            for (device_str, content) in by_device {
                let Ok(device_id) = DeviceId::parse(device_str.clone()) else { continue };
                let event = serde_json::json!({ "sender": as_user.as_str(), "type": event_type, "content": content });
                self.to_device_mailboxes.entry((user_id.clone(), device_id)).or_default().events.push(event);
            }
        }

        json_response(200, &serde_json::json!({}))
    }

    /// `PUT /rooms/{roomId}/send/{eventType}/{txnId}` -- appends `body` to
    /// `room_id`'s server-side timeline under a fresh, server-assigned
    /// event id (module doc's "bare minimum" for the Megolm group tests).
    /// No idempotency-by-`txnId` handling here (unlike the real spec):
    /// none of this crate's own tests retries a room send, so a second
    /// call with the same `txnId` simply appends a second event, matching
    /// this fake's overall "spec-shaped, not spec-complete" scope.
    fn handle_room_send(&mut self, as_user: &UserId, request: &OutgoingRequest) -> HttpResponseDescriptor {
        let Some((room_id, event_type)) = parse_room_send_path(&request.path) else {
            return json_response(
                400,
                &serde_json::json!({ "errcode": "M_UNKNOWN", "error": "fake server: malformed room send path" }),
            );
        };
        let body = request.body.clone().unwrap_or_else(|| Value::Object(Map::new()));
        let timeline = self.room_timelines.entry(room_id).or_default();
        timeline.next_event_seq += 1;
        let event_id = format!("$evt{}:example.org", timeline.next_event_seq);
        let event = serde_json::json!({
            "event_id": event_id,
            "type": event_type,
            "sender": as_user.as_str(),
            "origin_server_ts": timeline.next_event_seq as i64,
            "content": body,
        });
        timeline.events.push(event);

        json_response(200, &serde_json::json!({ "event_id": event_id }))
    }

    /// Every event currently stored in `room_id`'s server-side timeline,
    /// oldest first, in the full `{event_id, type, sender,
    /// origin_server_ts, content}` shape [`FakeServer::handle_room_send`]
    /// stored it under.
    pub fn room_timeline(&self, room_id: &RoomId) -> Vec<Value> {
        self.room_timelines.get(room_id).map(|timeline| timeline.events.clone()).unwrap_or_default()
    }

    fn handle_sync(&mut self, as_user: &UserId, as_device: &DeviceId, request: &OutgoingRequest) -> HttpResponseDescriptor {
        let key = (as_user.clone(), as_device.clone());
        let since_index =
            request.query.iter().find(|(name, _)| name == "since").and_then(|(_, value)| parse_sync_token(value));

        let mailbox = self.to_device_mailboxes.entry(key.clone()).or_default();
        // The client naming `since_index` is its own proof it already
        // durably has everything before that position -- only now is it
        // safe to forget those events. A request that never advances
        // `since` (a lost response, a retry) re-prunes nothing new and so
        // redelivers the exact same remaining events, verbatim.
        if let Some(since_index) = since_index {
            if since_index > mailbox.base_index {
                let drop_count = (since_index - mailbox.base_index).min(mailbox.events.len());
                mailbox.events.drain(0..drop_count);
                mailbox.base_index += drop_count;
            }
        }
        let events = mailbox.events.clone();
        let next_batch_index = mailbox.base_index + mailbox.events.len();

        let changed: Vec<Value> = self
            .pending_device_list_changes
            .remove(&key)
            .unwrap_or_default()
            .into_iter()
            .map(|user_id| Value::String(user_id.as_str().to_string()))
            .collect();

        let (otk_count, fallback_unused) = match self.devices.get(&key) {
            Some(state) => (state.one_time_keys.len() as u64, state.fallback.is_some() && !state.fallback_used),
            None => (0, false),
        };
        let fallback_types: Vec<&str> = if fallback_unused { vec!["signed_curve25519"] } else { Vec::new() };

        json_response(
            200,
            &serde_json::json!({
                "next_batch": format!("s{next_batch_index}"),
                "to_device": { "events": events },
                "device_lists": { "changed": changed, "left": [] },
                "device_one_time_keys_count": { "signed_curve25519": otk_count },
                "device_unused_fallback_key_types": fallback_types,
            }),
        )
    }
}

/// Flips the first byte of whatever string sits under
/// `value["signatures"][user_id][<first key>]` -- used only to corrupt an
/// already-signed key object for a test, never for anything this crate
/// actually trusts.
fn corrupt_first_signature(value: &mut Value, user_id: &UserId) {
    let Some(by_user) = value.get_mut("signatures").and_then(|sigs| sigs.get_mut(user_id.as_str())) else { return };
    let Some(by_user_map) = by_user.as_object_mut() else { return };
    let Some(key_id) = by_user_map.keys().next().cloned() else { return };
    let Some(signature) = by_user_map.get(&key_id).and_then(Value::as_str) else { return };
    let mut bytes = signature.as_bytes().to_vec();
    bytes[0] ^= 0xFF;
    by_user_map.insert(key_id, Value::String(mlc_vodozemac::base64_encode(bytes)));
}
