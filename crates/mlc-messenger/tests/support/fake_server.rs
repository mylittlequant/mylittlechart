//! An in-process, spec-shaped fake Matrix homeserver for `mlc-messenger`'s
//! own integration tests. Speaks [`OutgoingRequest`] -> [`HttpResponseDescriptor`]
//! directly -- no HTTP, no async, no sockets -- but every request/response
//! body is the real wire JSON shape the research doc's §3.2/§3.3 describe,
//! so this is a contract test harness every later piece (Megolm group
//! sessions, key backup, ...) can extend rather than a throwaway mock.
//!
//! Deliberately out of scope, and left for a later piece to add: room
//! endpoints (`/createRoom`, `/rooms/*`), cross-signing, key backup. A
//! request of any [`OutgoingRequestKind`] this fake does not implement gets
//! a clean `501` rather than a panic, so extending this file never requires
//! touching every call site that already works.

use mlc_messenger::wire::{HttpResponseDescriptor, OutgoingRequest, OutgoingRequestKind};
use mlc_messenger::{DeviceId, UserId};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, VecDeque};

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

/// An in-process fake homeserver. See the module doc.
#[derive(Default)]
pub struct FakeServer {
    devices: BTreeMap<(UserId, DeviceId), DeviceState>,
    to_device_queues: BTreeMap<(UserId, DeviceId), VecDeque<Value>>,
    pending_device_list_changes: BTreeMap<(UserId, DeviceId), Vec<UserId>>,
    next_batch: u64,
}

fn json_response(status: u16, value: &Value) -> HttpResponseDescriptor {
    HttpResponseDescriptor { status, body: serde_json::to_vec(value).expect("a serde_json::Value always serializes") }
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
            OutgoingRequestKind::Sync => self.handle_sync(as_user, as_device),
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
                self.to_device_queues.entry((user_id.clone(), device_id)).or_default().push_back(event);
            }
        }

        json_response(200, &serde_json::json!({}))
    }

    fn handle_sync(&mut self, as_user: &UserId, as_device: &DeviceId) -> HttpResponseDescriptor {
        self.next_batch += 1;
        let key = (as_user.clone(), as_device.clone());

        let events: Vec<Value> = self.to_device_queues.get_mut(&key).map(|queue| queue.drain(..).collect()).unwrap_or_default();
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
                "next_batch": format!("s{}", self.next_batch),
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
