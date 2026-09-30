//! [`MessengerDriver`] — the sans-I/O state machine a shell pumps to run a
//! [`crate::core::MessengerCore`] end to end, shared verbatim between the
//! native and web shells (this crate's own "one core, many shells" seam,
//! alongside [`crate::provider::SnapshotProvider`]). No network socket, no
//! filesystem call, no clock read, no thread spawn lives in this file (this
//! crate's own "no I/O of any kind" contract, crate doc) — every side
//! effect crosses out as a [`DriverRequest`]/[`FlushBatch`] the shell
//! executes, and every result crosses back in through [`MessengerDriver::
//! on_response`]/[`MessengerDriver::on_transport_error`]/
//! [`MessengerDriver::flush_done`].
//!
//! # Boot phases
//!
//! A fresh [`MessengerDriver`] does not hold a [`crate::core::MessengerCore`]
//! yet — it does not even know this token's `user_id`/`device_id` until the
//! server tells it. Three phases, in order:
//!
//! 1. **`whoami`** — [`MessengerDriver::tick`]'s very first call emits
//!    exactly one `GET /_matrix/client/v3/account/whoami` [`DriverRequest`]
//!    (tagged [`DriverRequestId::Whoami`] — never a
//!    [`crate::core::MessengerCore`]-minted request, since no core exists
//!    yet). The shell executes it with whatever bearer token it was
//!    started with and feeds the raw status/body back via
//!    [`MessengerDriver::on_response`].
//! 2. **Awaiting records** — once `whoami` resolves to a `user_id`/
//!    `device_id`, [`MessengerDriver::awaiting_records`] starts returning
//!    `Some((user_id, device_id))`. The shell loads every record it has
//!    stored under that device (`messenger/{device_id}/...`,
//!    `crate::store`'s own layout) and hands them back via
//!    [`MessengerDriver::records_loaded`].
//! 3. **Running** — [`MessengerDriver::records_loaded`] opens the core with
//!    [`crate::core::MessengerCore::open_sealed`] (`server_name` taken as
//!    the part of `user_id` after its first `:`, matching
//!    [`crate::ids::UserId`]'s own `@localpart:server_name` grammar). From
//!    here on [`MessengerDriver::tick`] drives the ordinary pump loop (see
//!    below).
//!
//! A `whoami` transport error, `5xx`, or `429` is treated as TRANSIENT (a
//! flaky connection or a rate limit, not a bad credential): it reports
//! through [`crate::provider::SnapshotProvider::set_last_error`] as a
//! "connecting…" message and reschedules another `whoami` probe after a
//! jittered, doubling backoff (base 1s, capped at 30s — the same
//! [`SeededJitter`] shape as [`MessengerCore::open_sealed`]'s own retry
//! jitter), with [`MessengerDriver::tick`] re-emitting it once `now_ms`
//! reaches that schedule. Every other boot failure — a `401`/`403`/other
//! `4xx` `whoami` response, an undecodable body, or
//! [`crate::core::MessengerCore::open_sealed`] itself failing to open the
//! records the shell handed back (wrong vault key, a corrupted record) — is
//! TERMINAL: it reports the same way but leaves
//! [`crate::provider::SnapshotProvider::vault_state`] at
//! [`VaultState::Locked`] permanently for this driver instance, and never
//! falls back to opening an empty store instead, because that is exactly
//! how a wrong vault key would silently destroy this device's own history
//! rather than refuse loudly. A shell that wants to retry a terminal
//! failure (e.g. after the user re-authenticates) builds a fresh
//! [`MessengerDriver`].
//!
//! # The pump loop
//!
//! Once running, every [`MessengerDriver::tick`] call:
//!
//! 1. drains [`crate::provider::SnapshotProvider::take_intents`] and
//!    applies them via [`crate::provider::apply_intents`], surfacing any
//!    failure through [`crate::provider::SnapshotProvider::set_last_error`];
//! 2. collects [`crate::core::MessengerCore::releasable_requests`];
//! 3. drains [`crate::core::MessengerCore::take_flush_batch`] until `None`;
//! 4. re-publishes the view ([`crate::provider::build_view`] +
//!    [`crate::provider::SnapshotProvider::publish`]) only if
//!    [`crate::core::MessengerCore::change_counter`] moved since the last
//!    publish — a tick where nothing changed publishes nothing new;
//! 5. marks [`VaultState::Ready`].
//!
//! [`MessengerDriver::shutdown`] drains whatever [`FlushBatch`]es are still
//! unflushed for one final synchronous write, then drops every secret this
//! driver held — the vault key and the core's own [`crate::core::CoreSecrets`]
//! are all [`zeroize::Zeroizing`]-wrapped already, so this needs no `Drop`
//! impl of its own.

#[path = "persist_log.rs"]
pub mod persist_log;

use serde::Deserialize;
use zeroize::Zeroizing;

use crate::core::{CoreConfig, CoreSecrets, MessengerCore};
use crate::ids::{DeviceId, RequestId, UserId};
use crate::outgoing_queue::Jitter;
use crate::persist::{FlushBatch, SealedRecord};
use crate::provider::{apply_intents, build_view, SnapshotProvider, SLICE_CAPABILITIES};
use crate::store::sealed::SealedRecordCodec;
use crate::wire::{HttpMethod, OutgoingRequest, OutgoingRequestKind};
use mlc_messenger_model::VaultState;

/// `GET /_matrix/client/v3/account/whoami` — this driver's own boot-time
/// probe (module doc's "Boot phases").
const WHOAMI_PATH: &str = "/_matrix/client/v3/account/whoami";

/// A transient `whoami` failure's own starting backoff (module doc).
const WHOAMI_RETRY_BASE_MS: u64 = 1_000;
/// A transient `whoami` failure's own backoff ceiling (module doc).
const WHOAMI_RETRY_CAP_MS: u64 = 30_000;

/// Computes `min(30_000, 1000 * 2^attempt)` milliseconds, then jitters the
/// result to `[50%, 100%]` of that value using `jitter_unit` (clamped to
/// `[0.0, 1.0]`), then re-caps — the exact same shape as
/// [`crate::outgoing_queue`]'s own `capped_exponential_backoff_ms`, just
/// with this module's own, tighter ceiling (module doc: 30s, not 60s).
/// `attempt` is capped at `5` internally: `1000 * 2^5 = 32_000` already
/// exceeds [`WHOAMI_RETRY_CAP_MS`], so every attempt from `5` onward
/// computes the same, fully-capped value.
fn whoami_retry_backoff_ms(attempt: u32, jitter_unit: f64) -> u64 {
    let shift = attempt.min(5);
    let base = WHOAMI_RETRY_BASE_MS.saturating_mul(1u64 << shift).min(WHOAMI_RETRY_CAP_MS);
    let unit = jitter_unit.clamp(0.0, 1.0);
    let jittered = (base as f64 * (0.5 + 0.5 * unit)).round() as u64;
    jittered.min(WHOAMI_RETRY_CAP_MS)
}

/// The subset of `whoami`'s response body this driver reads. `device_id` is
/// optional per the Client-Server API, but a token that does not identify a
/// device is useless to this crate (every record it stores is scoped under
/// `messenger/{device_id}/...`) — a response missing it fails boot exactly
/// like a non-200 status (module doc).
#[derive(Deserialize)]
struct WhoamiResponseBody {
    user_id: String,
    device_id: Option<String>,
}

/// Where [`MessengerDriver`]'s boot sequence currently stands — see the
/// module doc's "Boot phases".
enum BootPhase {
    /// Waiting on the one `whoami` probe [`MessengerDriver::tick`] emits on
    /// its first call.
    AwaitingWhoami,
    /// `whoami` resolved this token to `user_id`/`device_id`; waiting on
    /// the shell to load every stored record for `device_id` and hand them
    /// back via [`MessengerDriver::records_loaded`].
    AwaitingRecords {
        /// This account's own user id, as `whoami` reported it.
        user_id: UserId,
        /// This session's own device id, as `whoami` reported it.
        device_id: DeviceId,
    },
    /// Boot failed outright — terminal. The reason is already in
    /// [`crate::provider::SnapshotProvider::last_error`]; this driver
    /// instance never recovers on its own (module doc).
    Failed,
}

/// This driver's own top-level state.
enum DriverPhase {
    /// Not yet running a core — see [`BootPhase`].
    Booting(BootPhase),
    /// A live [`crate::core::MessengerCore`] is open and being pumped.
    Running,
}

/// Correlates one [`DriverRequest`] to whatever eventually answers it —
/// either this driver's own boot-time probe, or a request
/// [`crate::core::MessengerCore`] itself minted (in which case this wraps
/// that request's own [`RequestId`], the id
/// [`crate::core::MessengerCore::on_response`]/`on_transport_error` expect
/// back).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DriverRequestId {
    /// The one boot-time `whoami` probe (module doc) — never reused, since
    /// a driver's boot sequence issues exactly one over its lifetime.
    Whoami,
    /// A request [`crate::core::MessengerCore`] itself minted.
    Core(RequestId),
}

/// One HTTP call a shell must perform — either [`MessengerDriver`]'s own
/// boot-time probe or a request [`crate::core::MessengerCore`] minted,
/// flattened to the same shape so a shell's HTTP dispatch never needs to
/// branch on which ([`method`](Self::method)/[`path`](Self::path)/
/// [`query`](Self::query)/[`body`](Self::body) are already everything an
/// HTTP client needs).
#[derive(Clone, Debug, PartialEq)]
pub struct DriverRequest {
    /// Correlates this request to the response a shell later hands back.
    pub id: DriverRequestId,
    /// The HTTP method to use.
    pub method: HttpMethod,
    /// The request path, rooted at `/_matrix/client/v3/...`.
    pub path: String,
    /// Query-string parameters, not yet percent-encoded (same convention as
    /// [`crate::wire::OutgoingRequest::query`]).
    pub query: Vec<(String, String)>,
    /// The JSON request body, if this call has one.
    pub body: Option<serde_json::Value>,
    /// The original [`OutgoingRequestKind`], `None` for the boot-time
    /// `whoami` probe (which never reaches
    /// [`crate::core::MessengerCore`] at all). A shell's own HTTP dispatch
    /// never needs this (`method`/`path`/`query`/`body` already fully
    /// describe the call); it rides along so a test harness that already
    /// routes by kind (`tests/support/fake_server`) can keep doing so
    /// instead of re-deriving it from `path`.
    pub kind: Option<OutgoingRequestKind>,
}

impl DriverRequest {
    fn whoami() -> Self {
        Self { id: DriverRequestId::Whoami, method: HttpMethod::Get, path: WHOAMI_PATH.to_string(), query: Vec::new(), body: None, kind: None }
    }

    fn from_core(request: OutgoingRequest) -> Self {
        Self {
            id: DriverRequestId::Core(request.id),
            method: request.method,
            path: request.path,
            query: request.query,
            body: request.body,
            kind: Some(request.kind),
        }
    }
}

/// What one [`MessengerDriver::tick`] call produced: every HTTP call a
/// shell must now perform, and every [`FlushBatch`] it must now write
/// durably (in order — see [`crate::persist`]'s own module doc for why
/// batches must be acked in the order they were produced).
#[derive(Debug, Default)]
pub struct DriverOutput {
    /// Requests to execute.
    pub requests: Vec<DriverRequest>,
    /// Batches to write durably, then ack via [`MessengerDriver::flush_done`].
    pub flush: Vec<FlushBatch>,
}

/// A small, dependency-free xorshift64* PRNG (Vigna 2014), seeded from a
/// caller-supplied timestamp rather than reading a system clock (this
/// crate's own "no clock inside" rule, `crate::core`'s module doc) —
/// [`crate::outgoing_queue::Jitter`]'s own retry backoff only needs to
/// spread concurrent clients apart, not resist prediction, so this is
/// deliberately not a cryptographic RNG and pulls in no new dependency
/// beyond what this crate's `Cargo.lock` already carries.
struct SeededJitter {
    state: u64,
}

impl SeededJitter {
    fn new(seed_ms: i64) -> Self {
        // A zero state is xorshift64*'s one fixed point (it would output
        // zero forever) -- folding in an odd constant keeps every seed,
        // including `0`, productive.
        let seed = (seed_ms as u64) ^ 0x9E37_79B9_7F4A_7C15;
        Self { state: if seed == 0 { 1 } else { seed } }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

impl Jitter for SeededJitter {
    fn next_unit(&mut self) -> f64 {
        // Top 53 bits -> [0.0, 1.0), matching an f64 mantissa's precision.
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

/// The part of `user_id` after its first `:` — [`crate::ids::UserId`]'s own
/// `@localpart:server_name` grammar, matching how
/// [`crate::ids`]'s own `split_sigil_id` already splits it.
fn device_server_name(user_id: &UserId) -> String {
    user_id.as_str().split_once(':').map(|(_, server)| server.to_string()).unwrap_or_default()
}

/// See the module doc.
pub struct MessengerDriver {
    phase: DriverPhase,
    vault_key: Option<Zeroizing<[u8; 32]>>,
    provider: SnapshotProvider,
    core: Option<MessengerCore<SealedRecordCodec>>,
    whoami_sent: bool,
    /// How many transient `whoami` failures have happened in a row since
    /// the last successful send — [`whoami_retry_backoff_ms`]'s own
    /// `attempt` (module doc's "connecting…" retry).
    whoami_attempt: u32,
    /// `Some(t)` while waiting out a transient `whoami` failure's own
    /// backoff — [`MessengerDriver::tick`] re-sends once `now_ms >= t`.
    /// `None` means either never failed yet, or a probe is already
    /// in flight awaiting its response.
    whoami_next_attempt_at: Option<i64>,
    /// The core's `change_counter` at the last publish. `u64::MAX` until the
    /// first running tick, so a resumed session publishes the rooms its store
    /// restored even though nothing has changed yet.
    last_published_change_counter: u64,
}

impl MessengerDriver {
    /// Builds a driver in its very first boot phase — nothing has been sent
    /// or opened yet. `vault_key` is held only until
    /// [`MessengerDriver::records_loaded`] opens the core (module doc's
    /// "Boot phases"), then dropped.
    pub fn new(vault_key: Zeroizing<[u8; 32]>) -> Self {
        Self {
            phase: DriverPhase::Booting(BootPhase::AwaitingWhoami),
            vault_key: Some(vault_key),
            provider: SnapshotProvider::new(SLICE_CAPABILITIES),
            core: None,
            whoami_sent: false,
            whoami_attempt: 0,
            whoami_next_attempt_at: None,
            last_published_change_counter: u64::MAX,
        }
    }

    /// A clone of this driver's own [`SnapshotProvider`] — the UI-facing
    /// half of the seam (`crate::provider`'s own module doc). Cheap: it is
    /// a shared, reference-counted slot.
    pub fn provider(&self) -> SnapshotProvider {
        self.provider.clone()
    }

    /// `Some((user_id, device_id))` while this driver is waiting on
    /// [`MessengerDriver::records_loaded`] (module doc's boot phase 2),
    /// `None` at every other point in this driver's lifetime.
    pub fn awaiting_records(&self) -> Option<(&UserId, &DeviceId)> {
        match &self.phase {
            DriverPhase::Booting(BootPhase::AwaitingRecords { user_id, device_id }) => Some((user_id, device_id)),
            _ => None,
        }
    }

    fn fail_boot(&mut self, reason: String) {
        self.phase = DriverPhase::Booting(BootPhase::Failed);
        self.vault_key = None;
        self.provider.set_last_error(Some(reason));
    }

    /// A transient `whoami` failure (transport error, `5xx`, `429`, module
    /// doc): schedules a retry after a jittered, doubling backoff instead
    /// of failing boot outright, and reports it as "connecting…" rather
    /// than a hard error.
    fn schedule_whoami_retry(&mut self, now_ms: i64, reason: String) {
        let attempt = self.whoami_attempt;
        self.whoami_attempt = self.whoami_attempt.saturating_add(1);
        let mut jitter = SeededJitter::new(now_ms.wrapping_add(i64::from(attempt)));
        let backoff = whoami_retry_backoff_ms(attempt, jitter.next_unit());
        self.whoami_next_attempt_at = Some(now_ms.saturating_add(backoff as i64));
        self.whoami_sent = false;
        self.provider.set_last_error(Some(format!("connecting… ({reason})")));
    }

    /// Advances the pump loop by one step — see the module doc's "The pump
    /// loop" (once [`DriverPhase::Running`]) or "Boot phases" (before then).
    /// Never blocks; a shell calls this as often as it likes (e.g. once per
    /// UI frame, or whenever [`crate::provider::SnapshotProvider::enqueue`]
    /// woke it).
    pub fn tick(&mut self, now_ms: i64) -> DriverOutput {
        match &self.phase {
            DriverPhase::Booting(BootPhase::AwaitingWhoami) => {
                if let Some(next_attempt_at) = self.whoami_next_attempt_at {
                    if now_ms < next_attempt_at {
                        return DriverOutput::default();
                    }
                } else if self.whoami_sent {
                    // Already sent once and still awaiting its response --
                    // no retry scheduled (yet).
                    return DriverOutput::default();
                }
                self.whoami_sent = true;
                self.whoami_next_attempt_at = None;
                DriverOutput { requests: vec![DriverRequest::whoami()], flush: Vec::new() }
            }
            DriverPhase::Booting(_) => DriverOutput::default(),
            DriverPhase::Running => self.tick_running(now_ms),
        }
    }

    fn tick_running(&mut self, now_ms: i64) -> DriverOutput {
        let Some(core) = self.core.as_mut() else { return DriverOutput::default() };

        // A response the core could not ingest is otherwise invisible (the
        // `/sync` loop just never advances): surface it through the one error
        // slot.
        if let Some(error) = core.take_ingest_error() {
            self.provider.set_last_error(Some(error));
        }

        let intents = self.provider.take_intents();
        if !intents.is_empty() {
            if let Some(error) = apply_intents(core, intents, now_ms) {
                self.provider.set_last_error(Some(error));
            }
        }

        let requests = core.releasable_requests(now_ms).into_iter().map(DriverRequest::from_core).collect();

        let mut flush = Vec::new();
        while let Some(batch) = core.take_flush_batch() {
            flush.push(batch);
        }

        let change_counter = core.change_counter();
        if change_counter != self.last_published_change_counter {
            self.last_published_change_counter = change_counter;
            self.provider.publish(build_view(core, now_ms));
        }

        self.provider.set_vault_state(VaultState::Ready);

        DriverOutput { requests, flush }
    }

    /// `true` for a status this driver treats as a transient `whoami`
    /// failure (module doc) — a rate limit or a server-side hiccup, not a
    /// bad credential.
    fn is_transient_whoami_status(status: u16) -> bool {
        status == 429 || (500..=599).contains(&status)
    }

    fn on_boot_whoami_response(&mut self, status: u16, body: Vec<u8>, now_ms: i64) {
        if Self::is_transient_whoami_status(status) {
            self.schedule_whoami_retry(now_ms, format!("HTTP {status}"));
            return;
        }
        if status != 200 {
            self.fail_boot(format!("whoami failed: HTTP {status}"));
            return;
        }
        let parsed: WhoamiResponseBody = match serde_json::from_slice(&body) {
            Ok(parsed) => parsed,
            Err(error) => return self.fail_boot(format!("whoami response was not valid JSON: {error}")),
        };
        let user_id = match UserId::parse(parsed.user_id) {
            Ok(user_id) => user_id,
            Err(error) => return self.fail_boot(format!("whoami returned an invalid user_id: {error}")),
        };
        let Some(device_id) = parsed.device_id else {
            return self.fail_boot("whoami response carried no device_id".to_string());
        };
        let device_id = match DeviceId::parse(device_id) {
            Ok(device_id) => device_id,
            Err(error) => return self.fail_boot(format!("whoami returned an invalid device_id: {error}")),
        };
        // Recovered from any earlier "connecting…" retry -- the UI should
        // not keep showing it once boot is actually making progress again.
        self.provider.set_last_error(None);
        self.phase = DriverPhase::Booting(BootPhase::AwaitingRecords { user_id, device_id });
    }

    /// Feeds one HTTP response back in, keyed by the [`DriverRequestId`] the
    /// matching [`DriverRequest`] carried.
    pub fn on_response(&mut self, id: DriverRequestId, status: u16, body: Vec<u8>, now_ms: i64) {
        match id {
            DriverRequestId::Whoami => self.on_boot_whoami_response(status, body, now_ms),
            DriverRequestId::Core(request_id) => {
                if let Some(core) = self.core.as_mut() {
                    let _ = core.on_response(request_id, crate::wire::HttpResponseDescriptor { status, body }, now_ms);
                }
            }
        }
    }

    /// Feeds one transport-level failure (no HTTP response at all) back in.
    /// A `whoami` transport error is always transient (module doc) -- it
    /// schedules a retry rather than failing boot outright.
    pub fn on_transport_error(&mut self, id: &DriverRequestId, now_ms: i64) {
        match id {
            DriverRequestId::Whoami => self.schedule_whoami_retry(now_ms, "transport error".to_string()),
            DriverRequestId::Core(request_id) => {
                if let Some(core) = self.core.as_mut() {
                    core.on_transport_error(request_id, now_ms);
                }
            }
        }
    }

    /// Hands back every record a shell loaded for the device
    /// [`MessengerDriver::awaiting_records`] named — a no-op if this driver
    /// is not currently in that boot phase. Opens the core
    /// ([`crate::core::MessengerCore::open_sealed`]) and, on success,
    /// advances to [`DriverPhase::Running`]; on failure (wrong vault key, a
    /// corrupted record), fails boot instead of retrying with an empty
    /// store (module doc's own load-bearing rule: never silently discard a
    /// device's history because it failed to decrypt).
    pub fn records_loaded(&mut self, records: Vec<SealedRecord>, now_ms: i64) {
        let (user_id, device_id) = match &self.phase {
            DriverPhase::Booting(BootPhase::AwaitingRecords { user_id, device_id }) => (user_id.clone(), device_id.clone()),
            _ => return,
        };
        let Some(vault_key) = self.vault_key.take() else {
            self.fail_boot("records_loaded called with no vault key held (already consumed)".to_string());
            return;
        };

        let secrets = CoreSecrets::from_vault(&vault_key, device_id.as_str());
        drop(vault_key);
        let config = CoreConfig { server_name: device_server_name(&user_id), user_id, device_id };
        let jitter: Box<dyn Jitter> = Box::new(SeededJitter::new(now_ms));

        match MessengerCore::open_sealed(records, secrets, config, now_ms, jitter) {
            Ok(core) => {
                self.core = Some(core);
                self.phase = DriverPhase::Running;
            }
            Err(error) => self.fail_boot(error.to_string()),
        }
    }

    /// A shell's durable write failed and could not be recovered (for the
    /// native shell: `persist_log::append_frame` gave up after its one
    /// retry). Surfaces through the error slot this driver owns. The shell
    /// must ack nothing further after this: unacked batches keep the core
    /// from releasing any request that depends on them, so the messenger
    /// stalls instead of sending ciphertext whose ratchet state never
    /// reached disk.
    pub fn storage_failed(&mut self, reason: &str) {
        self.provider.set_last_error(Some(format!("messenger storage write failed: {reason}")));
    }

    /// Acknowledges that batch `batch_id` has been durably written.
    pub fn flush_done(&mut self, batch_id: u64) {
        if let Some(core) = self.core.as_mut() {
            core.ack_flush(batch_id);
        }
    }

    /// Consumes this driver, draining every [`FlushBatch`] still unflushed
    /// for one final synchronous write. Every secret this driver held — the
    /// vault key and, if it reached [`DriverPhase::Running`], the core's own
    /// [`crate::core::CoreSecrets`] — is already [`Zeroizing`]-wrapped, so
    /// it self-wipes the moment this call returns and `self` drops; no
    /// explicit `Drop` impl is needed.
    pub fn shutdown(mut self) -> Vec<FlushBatch> {
        let mut batches = Vec::new();
        if let Some(core) = self.core.as_mut() {
            while let Some(batch) = core.take_flush_batch() {
                batches.push(batch);
            }
        }
        batches
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{MessageKind, MessengerCommand, OutgoingMessage};
    use crate::ids::RoomId;
    use mlc_messenger_model::MessengerProvider;

    fn whoami_body(user: &str, device: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({ "user_id": user, "device_id": device })).expect("serialize")
    }

    fn boot_to_running() -> MessengerDriver {
        let mut driver = MessengerDriver::new(Zeroizing::new([7u8; 32]));
        driver.tick(0);
        driver.on_response(DriverRequestId::Whoami, 200, whoami_body("@alice:example.org", "DEV1"), 0);
        driver.records_loaded(Vec::new(), 0);
        driver
    }

    #[test]
    fn boot_emits_whoami_then_waits_for_records_then_runs() {
        let mut driver = MessengerDriver::new(Zeroizing::new([7u8; 32]));

        let output = driver.tick(0);
        assert_eq!(output.requests.len(), 1);
        assert_eq!(output.requests[0].id, DriverRequestId::Whoami);
        assert_eq!(output.requests[0].method, HttpMethod::Get);
        assert_eq!(output.requests[0].path, WHOAMI_PATH);

        assert!(driver.tick(0).requests.is_empty(), "whoami is only ever sent once");
        assert!(driver.awaiting_records().is_none());

        driver.on_response(DriverRequestId::Whoami, 200, whoami_body("@alice:example.org", "DEV1"), 0);

        let (user_id, device_id) = driver.awaiting_records().expect("whoami resolved a device");
        assert_eq!(user_id.as_str(), "@alice:example.org");
        assert_eq!(device_id.as_str(), "DEV1");
        assert_eq!(driver.provider().vault_state(), VaultState::Locked);

        driver.records_loaded(Vec::new(), 0);
        assert!(driver.awaiting_records().is_none(), "boot is done -- nothing left to load");

        // Running now -- the fresh device's own bootstrap flush shows up on
        // the very first tick, and the vault reports Ready.
        let output = driver.tick(0);
        assert!(!output.flush.is_empty(), "opening a fresh device dirties its own Olm account record");
        assert_eq!(driver.provider().vault_state(), VaultState::Ready);
    }

    #[test]
    fn boot_with_undecryptable_records_stays_locked_and_reports() {
        let mut driver = MessengerDriver::new(Zeroizing::new([7u8; 32]));
        driver.tick(0);
        driver.on_response(DriverRequestId::Whoami, 200, whoami_body("@alice:example.org", "DEV1"), 0);

        // Sealed under a DIFFERENT key (or simply malformed) -- fails the
        // codec's own authentication check on open, exactly the "wrong
        // key" case this driver must never paper over by silently starting
        // fresh (module doc).
        let bogus = SealedRecord { key: crate::persist::RecordKey::new("messenger/DEV1/account"), bytes: vec![9u8; 64] };
        driver.records_loaded(vec![bogus], 0);

        assert_eq!(driver.provider().vault_state(), VaultState::Locked);
        assert!(driver.provider().last_error().is_some());

        // Boot stays failed permanently -- further ticks raise nothing.
        assert!(driver.tick(0).requests.is_empty());
        assert!(driver.tick(0).flush.is_empty());
    }

    #[test]
    fn whoami_transport_error_retries_with_backoff() {
        let mut driver = MessengerDriver::new(Zeroizing::new([7u8; 32]));
        assert_eq!(driver.tick(0).requests.len(), 1, "first tick sends whoami");

        driver.on_transport_error(&DriverRequestId::Whoami, 0);
        assert!(driver.provider().last_error().is_some(), "a transient failure still surfaces as a 'connecting' message");
        assert_eq!(driver.provider().vault_state(), VaultState::Locked, "not terminal -- still booting");

        // Attempt 0's own backoff window is [500ms, 1000ms] -- 100ms is
        // never enough, 1500ms always is, regardless of jitter.
        assert!(driver.tick(100).requests.is_empty(), "backoff has not elapsed yet");
        let output = driver.tick(1_500);
        assert_eq!(output.requests.len(), 1, "whoami is retried once the first backoff window elapsed");
        assert_eq!(output.requests[0].id, DriverRequestId::Whoami);

        // A second failure grows the backoff (attempt 1: [1000ms, 2000ms]
        // measured from now_ms=1_500, so due somewhere in [2_500, 3_500]).
        driver.on_transport_error(&DriverRequestId::Whoami, 1_500);
        assert!(driver.tick(2_000).requests.is_empty(), "the second, larger backoff window has not elapsed yet");
        let output = driver.tick(4_000);
        assert_eq!(output.requests.len(), 1, "the growing backoff still eventually retries -- never terminal on its own");

        // Recovery still completes boot normally.
        driver.on_response(DriverRequestId::Whoami, 200, whoami_body("@alice:example.org", "DEV1"), 4_000);
        let (user_id, device_id) = driver.awaiting_records().expect("recovered and resolved a device");
        assert_eq!(user_id.as_str(), "@alice:example.org");
        assert_eq!(device_id.as_str(), "DEV1");
        assert!(driver.provider().last_error().is_none(), "the 'connecting…' message clears once boot is making progress again");
    }

    #[test]
    fn whoami_401_is_terminal() {
        let mut driver = MessengerDriver::new(Zeroizing::new([7u8; 32]));
        driver.tick(0);
        driver.on_response(DriverRequestId::Whoami, 401, Vec::new(), 0);

        assert_eq!(driver.provider().vault_state(), VaultState::Locked);
        assert!(driver.provider().last_error().is_some());
        assert!(driver.awaiting_records().is_none());

        // Terminal -- further ticks never retry, even long after any
        // transient backoff window would have elapsed.
        assert!(driver.tick(60_000).requests.is_empty());
        assert!(driver.tick(60_000).flush.is_empty());
    }

    #[test]
    fn tick_publishes_only_when_change_counter_moves() {
        let mut driver = boot_to_running();
        let provider = driver.provider();

        // The first running tick publishes once (a resumed session must show
        // the rooms its store restored); the fresh account's own bootstrap
        // dirtied records but emitted no `MessengerEvent`, so nothing after
        // that first tick may publish again.
        driver.tick(0);
        let after_bootstrap = provider.change_counter();
        assert!(after_bootstrap > 0, "the first running tick published the (empty) view and flipped the vault state");
        driver.tick(0);
        assert_eq!(provider.change_counter(), after_bootstrap, "nothing moved -- no republish");

        // Drive a real state change directly through the core (bypassing
        // the still-stubbed `apply_intent`, a different piece's own job) --
        // `SendMessage` always emits `TimelineChanged`.
        let room_id = RoomId::parse("!r:example.org").expect("valid room id");
        driver
            .core
            .as_mut()
            .expect("Running")
            .dispatch(
                MessengerCommand::SendMessage {
                    room_id,
                    message: OutgoingMessage { kind: MessageKind::Text, body: "hi".to_string(), reply_to: None, edit_of: None },
                    txn_id: None,
                },
                0,
            )
            .expect("dispatch succeeds");

        driver.tick(0);
        assert_eq!(provider.change_counter(), after_bootstrap + 1, "the core's own change_counter moved -- exactly one new view was published");

        driver.tick(0);
        assert_eq!(provider.change_counter(), after_bootstrap + 1, "ticking again with nothing new does not republish");
    }

    #[test]
    fn an_undecodable_sync_response_reaches_the_error_slot() {
        let mut driver = boot_to_running();
        let mut sync_id = None;
        for _ in 0..4 {
            let output = driver.tick(0);
            for batch in &output.flush {
                driver.flush_done(batch.id);
            }
            if let Some(request) = output.requests.iter().find(|request| request.kind == Some(OutgoingRequestKind::Sync)) {
                sync_id = Some(request.id.clone());
                break;
            }
        }
        let sync_id = sync_id.expect("a sync request was released");
        assert!(driver.provider().last_error().is_none());

        driver.on_response(sync_id, 200, b"{ not a sync response".to_vec(), 0);
        driver.tick(0);
        let error = driver.provider().last_error().expect("the ingest failure was surfaced");
        assert!(error.contains("not ingested"), "unexpected error text: {error}");
    }

    #[test]
    fn storage_failed_reports_through_the_error_slot() {
        let mut driver = boot_to_running();
        assert!(driver.provider().last_error().is_none());
        driver.storage_failed("disk full");
        let error = driver.provider().last_error().expect("error set");
        assert!(error.contains("messenger storage write failed"));
        assert!(error.contains("disk full"));
    }

    #[test]
    fn shutdown_returns_unflushed_batches() {
        let driver = boot_to_running();
        let batches = driver.shutdown();
        assert!(!batches.is_empty(), "the fresh account's own bootstrap write was never handed to a shell tick, so shutdown must not lose it");
    }
}
