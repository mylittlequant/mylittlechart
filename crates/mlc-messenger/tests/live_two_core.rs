//! Live two-core run: real `MessengerDriver`s (plus the `provider` UI view)
//! pumped over real HTTP against a LOCAL `mylittlechart-server` stand. Every
//! test here is `#[ignore]` -- the stand is a throwaway server on this
//! machine, never part of the normal gate. Run with:
//!
//! ```text
//! cargo test --release -p mylittlechart-messenger --test live_two_core \
//!     -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! The stand's URL, bearer tokens and account ids are read from the env file
//! named by `MLC_LIVE_STAND_ENV` (default
//! `E:\Data\mylittlechart\live-run\stand.env`); no credential is ever
//! printed, logged or committed.
//!
//! # Pump
//!
//! [`LiveCore`] is the minimal blocking twin of the native shell's
//! `MessengerIo`: [`LiveCore::step`] applies finished HTTP completions,
//! ticks the driver, spawns each released request on its own std thread
//! (`/sync` long-polls, so requests must run concurrently), and appends every
//! flush batch to an in-memory record log via
//! `driver::persist_log::encode_batch` before acking it. [`wait_until`] steps
//! every core in play every 10 ms until a condition on the providers holds
//! (the shell wakes on completion, so a coarser step would only add noise to
//! the timings), or panics after 20 s with each core's last provider state.
//!
//! Every core sends its own `X-Forwarded-For` (a loopback peer's header is
//! trusted by the server's per-IP rate limiter) so no two cores share a
//! bucket.
//!
//! # Devices
//!
//! The server refuses to let a device's identity keys change once published
//! (`keys.rs` `decide_device_keys_upload`), and a stand token is one device
//! for good. So each account's long-lived device (`Stand::core`) keeps its
//! record log on disk between tests and runs, like a real shell, and resumes
//! the same Olm account every time. A "second device" scenario needs a device
//! the server has never seen, so `Stand::fresh_core` mints a throwaway token
//! straight into the stand's identity DB (`mlc-dbadmin exec`, the documented
//! seeding path) and logs that device out again when the core is dropped.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use mlc_messenger::driver::{persist_log, DriverRequest, DriverRequestId, MessengerDriver};
use mlc_messenger::persist::SealedRecord;
use mlc_messenger::wire::HttpMethod;
use mlc_messenger::SnapshotProvider;
use mlc_messenger_model::{
    CreateRoomRequest, Folder, FolderExclude, FolderIcon, FolderInclude, LegacyContent, MemberRole, MessageContent,
    MessengerIntent, MessengerProvider, RoomId, RoomKind, RoomSummary, RoomVisibility, SendState, TagName,
    TimelineItem, UserId, UtdReason, VaultState,
};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const DEFAULT_STAND_ENV: &str = r"E:\Data\mylittlechart\live-run\stand.env";
const WAIT_TIMEOUT: Duration = Duration::from_secs(20);
const STEP_INTERVAL: Duration = Duration::from_millis(10);
const SYNC_PATH: &str = "/_matrix/client/v3/sync";
const SYNC_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// How many failed (non-2xx / transport) requests a core remembers for its
/// timeout diagnostics.
const FAILURE_TAIL: usize = 8;
const THUMBS_UP: &str = "\u{1F44D}";

// ---------------------------------------------------------------------
// Stand env + identities
// ---------------------------------------------------------------------

/// The three stand accounts. Each has one long-lived device (its stand
/// token, with a record log persisted on disk exactly like a real shell's:
/// the server refuses to let a device's identity keys change, so a device
/// must resume its Olm account on every run) and can mint throwaway extra
/// devices ([`Stand::fresh_core`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Who {
    Alice,
    Bob,
    Carol,
}

impl Who {
    fn label(self) -> &'static str {
        match self {
            Who::Alice => "alice",
            Who::Bob => "bob",
            Who::Carol => "carol",
        }
    }

    /// The stand token of the account's long-lived device.
    fn token_var(self) -> &'static str {
        match self {
            Who::Alice => "MLC_LIVE_ALICE1_TOKEN",
            Who::Bob => "MLC_LIVE_BOB_TOKEN",
            Who::Carol => "MLC_LIVE_CAROL_TOKEN",
        }
    }

    fn account_prefix(self) -> &'static str {
        match self {
            Who::Alice => "MLC_LIVE_ALICE",
            Who::Bob => "MLC_LIVE_BOB",
            Who::Carol => "MLC_LIVE_CAROL",
        }
    }

    /// Fixed test vault keys (never real key material). Every device of an
    /// account shares its key.
    fn vault_key(self) -> [u8; 32] {
        match self {
            Who::Alice => [0x11; 32],
            Who::Bob => [0x22; 32],
            Who::Carol => [0x33; 32],
        }
    }
}

/// The parsed stand env file. Deliberately no `Debug`: it holds tokens.
struct Stand {
    /// The stand env file; its directory also holds `bin/`, `data/` and the
    /// key copy `mlc-dbadmin` needs.
    env_path: PathBuf,
    vars: BTreeMap<String, String>,
}

impl Stand {
    fn load() -> Self {
        let path = std::env::var("MLC_LIVE_STAND_ENV").unwrap_or_else(|_| DEFAULT_STAND_ENV.to_string());
        let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read the stand env file {path}: {error}"));
        let vars = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
            .collect();
        let stand = Self { env_path: PathBuf::from(path), vars };
        stand.assert_server_up();
        stand
    }

    fn stand_dir(&self) -> PathBuf {
        self.env_path.parent().map(Path::to_path_buf).unwrap_or_default()
    }

    /// The value of `key`; a missing key panics naming the KEY only.
    fn var(&self, key: &str) -> &str {
        self.vars.get(key).map(String::as_str).unwrap_or_else(|| panic!("stand env has no {key}"))
    }

    fn base_url(&self) -> String {
        self.var("MLC_LIVE_BASE_URL").trim_end_matches('/').to_string()
    }

    fn public_id(&self, who: Who) -> String {
        self.var(&format!("{}_PUBLIC_ID", who.account_prefix())).to_string()
    }

    fn mxid(&self, who: Who) -> UserId {
        UserId::from(self.var(&format!("{}_MXID", who.account_prefix())))
    }

    fn assert_server_up(&self) {
        let url = format!("{}/_matrix/client/versions", self.base_url());
        let up = reqwest::blocking::Client::new().get(&url).timeout(Duration::from_secs(5)).send().map(|r| r.status().is_success()).unwrap_or(false);
        assert!(up, "the stand server is not answering {url}; run: bash E:/Data/mylittlechart/live-run/start-server.sh");
    }

    /// Where each long-lived device keeps its record log between tests and
    /// runs (`MLC_LIVE_LOG_DIR`, default `<stand dir>/core-logs`).
    fn log_path(&self, who: Who) -> PathBuf {
        let dir = std::env::var("MLC_LIVE_LOG_DIR").map(PathBuf::from).unwrap_or_else(|_| self.stand_dir().join("core-logs"));
        fs::create_dir_all(&dir).unwrap_or_else(|error| panic!("cannot create the core log dir {}: {error}", dir.display()));
        dir.join(format!("{}.log", who.label()))
    }

    /// Boots `who`'s long-lived device, resuming its persisted record log
    /// (empty on the very first run). Every acked batch is mirrored to the
    /// file, so a later test -- or run -- resumes the same Olm account.
    fn core(&self, who: Who) -> LiveCore {
        let path = self.log_path(who);
        let bytes = fs::read(&path).unwrap_or_default();
        self.core_with_log(who, bytes)
    }

    /// Boots `who`'s long-lived device over `log` (its current record log,
    /// e.g. what `LiveCore::shutdown` returned): a restart.
    fn core_with_log(&self, who: Who, log: Vec<u8>) -> LiveCore {
        let path = self.log_path(who);
        let (_, valid_len) = persist_log::replay(&log);
        let file = OpenOptions::new().create(true).write(true).truncate(false).open(&path).unwrap_or_else(|error| panic!("cannot open {}: {error}", path.display()));
        // Cut a torn tail (a crash mid-write) exactly as a real shell does at boot.
        file.set_len(valid_len as u64).unwrap_or_else(|error| panic!("cannot trim {}: {error}", path.display()));
        drop(file);
        let file = OpenOptions::new().append(true).open(&path).unwrap_or_else(|error| panic!("cannot open {} for append: {error}", path.display()));
        let mut core = LiveCore::new(who.label(), self.base_url(), self.var(who.token_var()).to_string(), who.vault_key(), log[..valid_len].to_vec());
        core.log_file = Some(file);
        core
    }

    /// Boots a brand-new device of `who`'s account: a throwaway bearer token
    /// is minted straight into the stand's identity DB (`mlc-dbadmin`, the
    /// documented seeding path), so the server sees a never-before-seen
    /// device. The device is logged out again when the core is dropped.
    fn fresh_core(&self, who: Who, name: &'static str) -> LiveCore {
        let token = self.mint_token(who);
        let mut core = LiveCore::new(name, self.base_url(), token.clone(), who.vault_key(), Vec::new());
        core.revoke_on_drop = Some(RevokeOnDrop { base_url: self.base_url(), token });
        core
    }

    /// Inserts a new bearer-token row for `who`'s account and returns the raw
    /// token (only its SHA-256 goes into the DB). Never printed.
    fn mint_token(&self, who: Who) -> String {
        let raw = random_token();
        let hash = sha256_hex(&raw);
        let sql = format!(
            "INSERT INTO auth_tokens (token_hash, user_id, created_at, expires_at)              SELECT '{hash}', id, '2026-09-29T00:00:00+00:00', '2099-01-01T00:00:00+00:00'              FROM users WHERE provider = 'dev' AND provider_user_id = '{}';",
            who.label()
        );
        let dir = self.stand_dir();
        let key_copy = ["dbkey.seedcopy", "dbkey.keep"].iter().map(|name| dir.join(name)).find(|path| path.exists()).unwrap_or_else(|| panic!("no dbkey copy in {}", dir.display()));
        let output = Command::new(dir.join("bin").join("mlc-dbadmin.exe"))
            .arg("exec")
            .arg(dir.join("data").join("mylittlechart.db"))
            .arg(&sql)
            .env("MLC_DBKEY_PATH", key_copy)
            .output()
            .unwrap_or_else(|error| panic!("cannot run mlc-dbadmin: {error}"));
        assert!(output.status.success(), "mlc-dbadmin exec failed: {}", String::from_utf8_lossy(&output.stderr));
        raw
    }

    /// A plain authenticated HTTP handle for `who` (the legacy `/api/dm/*`
    /// calls).
    fn api(&self, who: Who) -> Api {
        Api { client: reqwest::blocking::Client::new(), base_url: self.base_url(), token: self.var(who.token_var()).to_string(), forwarded_for: next_forwarded_for() }
    }
}

/// A throwaway bearer token: SHA-256 over OS-seeded hasher output, the clock
/// and a counter. Test-only credential for a loopback stand.
fn random_token() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let mut material = Vec::new();
    material.extend_from_slice(&now_ms().to_le_bytes());
    material.extend_from_slice(&COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    for _ in 0..4 {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_nanos() as u64).unwrap_or(0));
        material.extend_from_slice(&hasher.finish().to_le_bytes());
    }
    sha256_hex_bytes(&material)
}

fn sha256_hex(text: &str) -> String {
    sha256_hex_bytes(text.as_bytes())
}

fn sha256_hex_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_millis() as i64).unwrap_or(0)
}

/// A per-core fake client IP for `X-Forwarded-For`, distinct across cores and
/// across runs (the server's rate-limit buckets outlive one test run).
fn next_forwarded_for() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let salt = SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_nanos() as u64).unwrap_or(0);
    format!("10.{}.{}.{}", (salt >> 20) & 0xff, (salt >> 12) & 0xff, n & 0xff)
}

/// A per-run suffix so names the test creates never collide with what an
/// earlier run left on the (persistent) stand.
fn nonce() -> String {
    now_ms().to_string()
}

// ---------------------------------------------------------------------
// Request log (shared handle, so wait conditions can read it)
// ---------------------------------------------------------------------

struct LoggedRequest {
    method: &'static str,
    path: String,
    /// The query string as `k=v&k=v` (for tracing only).
    query: String,
    body: Option<serde_json::Value>,
    /// `None` while in flight, `Some(0)` for a transport failure.
    status: Option<u16>,
    /// Set for a non-2xx answer or transport failure: status plus the start
    /// of the body (server error bodies carry no credentials).
    failure: Option<String>,
}

/// Every request one core issued, in order. Cloning shares the same log.
#[derive(Clone, Default)]
struct RequestLog(Arc<Mutex<Vec<LoggedRequest>>>);

impl RequestLog {
    fn with<R>(&self, read: impl FnOnce(&mut Vec<LoggedRequest>) -> R) -> R {
        let mut guard = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        read(&mut guard)
    }

    fn push(&self, entry: LoggedRequest) -> usize {
        self.with(|entries| {
            let index = entries.len();
            entries.push(entry);
            index
        })
    }

    fn finish(&self, index: usize, status: u16, body: &[u8]) {
        self.with(|entries| {
            if let Some(entry) = entries.get_mut(index) {
                entry.status = Some(status);
                if !(200..300).contains(&status) {
                    let text = String::from_utf8_lossy(body);
                    entry.failure = Some(format!("{} {} -> {status} {}", entry.method, entry.path, text.chars().take(160).collect::<String>()));
                }
            }
        });
    }

    fn fail_transport(&self, index: usize, reason: &str) {
        self.with(|entries| {
            if let Some(entry) = entries.get_mut(index) {
                entry.status = Some(0);
                entry.failure = Some(format!("{} {} -> transport error: {reason}", entry.method, entry.path));
            }
        });
    }

    fn len(&self) -> usize {
        self.with(|entries| entries.len())
    }

    /// `METHOD path?query` of entry `index`, for the trace.
    fn describe(&self, index: usize) -> String {
        self.with(|entries| entries.get(index).map(|entry| format!("{} {}?{}", entry.method, entry.path, entry.query)).unwrap_or_default())
    }

    /// How many requests at index `since` or later matched `method` and a
    /// path containing `path_part` and were answered 2xx.
    fn ok_since(&self, since: usize, method: &str, path_part: &str) -> usize {
        self.with(|entries| {
            entries
                .iter()
                .skip(since)
                .filter(|entry| entry.method == method && entry.path.contains(path_part) && entry.status.is_some_and(|status| (200..300).contains(&status)))
                .count()
        })
    }

    /// Whether any request at index `since` or later to a path containing
    /// `path_part` carried the top-level JSON key `key` in its body.
    fn body_has_key_since(&self, since: usize, path_part: &str, key: &str) -> bool {
        self.with(|entries| {
            entries.iter().skip(since).any(|entry| entry.path.contains(path_part) && entry.body.as_ref().is_some_and(|body| body.get(key).is_some()))
        })
    }

    fn failures(&self) -> Vec<String> {
        self.with(|entries| entries.iter().filter_map(|entry| entry.failure.clone()).rev().take(FAILURE_TAIL).collect())
    }

    fn tail(&self, count: usize) -> Vec<String> {
        self.with(|entries| {
            entries
                .iter()
                .rev()
                .take(count)
                .map(|entry| format!("{} {} {}", entry.method, entry.path, entry.status.map_or("...".to_string(), |status| status.to_string())))
                .collect()
        })
    }
}

// ---------------------------------------------------------------------
// The blocking pump
// ---------------------------------------------------------------------

/// One finished HTTP call, sent back from its request thread.
struct Completion {
    index: usize,
    id: DriverRequestId,
    /// How long the HTTP call took.
    took: Duration,
    result: Result<(u16, Vec<u8>), String>,
}

impl Completion {
    /// `status size ms` (or the transport error) for the trace line.
    fn summary(&self) -> String {
        match &self.result {
            Ok((status, body)) => format!("{status} {}B {}ms", body.len(), self.took.as_millis()),
            Err(reason) => format!("transport error after {}ms: {reason}", self.took.as_millis()),
        }
    }
}

/// `MLC_LIVE_TRACE=1` prints one line per finished HTTP call.
fn trace_enabled() -> bool {
    std::env::var_os("MLC_LIVE_TRACE").is_some()
}

/// A `MessengerDriver` plus its blocking HTTP pump and in-memory record log.
/// Deliberately no `Debug`: it holds a bearer token.
struct LiveCore {
    name: &'static str,
    base_url: String,
    token: String,
    forwarded_for: String,
    client: reqwest::blocking::Client,
    driver: MessengerDriver,
    provider: SnapshotProvider,
    log: Vec<u8>,
    /// The on-disk mirror of `log` (long-lived devices only).
    log_file: Option<File>,
    /// Whether this core resumed a non-empty record log (its Olm account
    /// already exists, so it will not publish device keys again).
    resumed: bool,
    /// Logs a throwaway device out when this core is dropped.
    revoke_on_drop: Option<RevokeOnDrop>,
    tx: Sender<Completion>,
    rx: Receiver<Completion>,
    requests: RequestLog,
}

/// Logs the device behind `token` out (`DELETE /devices/{id}`), which also
/// revokes the throwaway token itself. Best effort: a stand that is gone
/// leaves nothing to clean.
struct RevokeOnDrop {
    base_url: String,
    token: String,
}

impl Drop for RevokeOnDrop {
    fn drop(&mut self) {
        let client = reqwest::blocking::Client::new();
        let authorize = |builder: reqwest::blocking::RequestBuilder| {
            builder.timeout(Duration::from_secs(10)).header("Authorization", format!("Bearer {}", self.token)).header("X-Forwarded-For", next_forwarded_for())
        };
        let whoami = authorize(client.get(format!("{}/_matrix/client/v3/account/whoami", self.base_url))).send();
        let device_id = whoami.ok().and_then(|response| response.json::<serde_json::Value>().ok()).and_then(|body| body.get("device_id").and_then(serde_json::Value::as_str).map(str::to_string));
        if let Some(device_id) = device_id {
            let _ = authorize(client.delete(format!("{}/_matrix/client/v3/devices/{device_id}", self.base_url))).send();
        }
    }
}

impl LiveCore {
    fn new(name: &'static str, base_url: String, token: String, vault_key: [u8; 32], log: Vec<u8>) -> Self {
        let driver = MessengerDriver::new(Zeroizing::new(vault_key));
        let provider = driver.provider();
        let (tx, rx) = mpsc::channel();
        let resumed = !log.is_empty();
        Self {
            name,
            base_url,
            token,
            forwarded_for: next_forwarded_for(),
            client: reqwest::blocking::Client::new(),
            driver,
            provider,
            log,
            log_file: None,
            resumed,
            revoke_on_drop: None,
            tx,
            rx,
            requests: RequestLog::default(),
        }
    }

    fn provider(&self) -> SnapshotProvider {
        self.provider.clone()
    }

    fn requests(&self) -> RequestLog {
        self.requests.clone()
    }

    /// One pump step: completions in, boot records, tick, requests out,
    /// flush batches appended to the log and acked.
    fn step(&mut self) {
        let now = now_ms();
        while let Ok(done) = self.rx.try_recv() {
            if trace_enabled() {
                println!("TRACE [{}] {} -> {}", self.name, self.requests.describe(done.index), done.summary());
            }
            match done.result {
                Ok((status, body)) => {
                    self.requests.finish(done.index, status, &body);
                    self.driver.on_response(done.id, status, body, now);
                }
                Err(reason) => {
                    self.requests.fail_transport(done.index, &reason);
                    self.driver.on_transport_error(&done.id, now);
                }
            }
        }

        if self.driver.awaiting_records().is_some() {
            let (map, _valid_len) = persist_log::replay(&self.log);
            let records = map.into_iter().map(|(key, bytes)| SealedRecord { key, bytes }).collect();
            self.driver.records_loaded(records, now);
        }

        let output = self.driver.tick(now);
        for request in output.requests {
            self.spawn(request);
        }
        for batch in output.flush {
            self.append(&persist_log::encode_batch(&batch));
            self.driver.flush_done(batch.id);
        }
    }

    /// Appends one record-log frame to the in-memory log and its file mirror
    /// (before the batch is acked, as the flush-before-send barrier requires).
    fn append(&mut self, frame: &[u8]) {
        write_frame(self.name, &mut self.log, &mut self.log_file, frame);
    }

    fn spawn(&self, request: DriverRequest) {
        let (method, label) = match request.method {
            HttpMethod::Get => (reqwest::Method::GET, "GET"),
            HttpMethod::Post => (reqwest::Method::POST, "POST"),
            HttpMethod::Put => (reqwest::Method::PUT, "PUT"),
        };
        let query = request.query.iter().map(|(key, value)| format!("{key}={value}")).collect::<Vec<_>>().join("&");
        let index = self.requests.push(LoggedRequest { method: label, path: request.path.clone(), query, body: request.body.clone(), status: None, failure: None });
        let timeout = if request.path == SYNC_PATH { SYNC_TIMEOUT } else { DEFAULT_TIMEOUT };
        let mut builder = self
            .client
            .request(method, format!("{}{}", self.base_url, request.path))
            .timeout(timeout)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("X-Forwarded-For", &self.forwarded_for);
        if !request.query.is_empty() {
            builder = builder.query(&request.query);
        }
        if let Some(body) = &request.body {
            builder = builder.json(body);
        }
        let tx = self.tx.clone();
        let id = request.id;
        std::thread::spawn(move || {
            let started = Instant::now();
            let result = match builder.send() {
                Ok(response) => {
                    let status = response.status().as_u16();
                    response.bytes().map(|bytes| (status, bytes.to_vec())).map_err(|error| error.to_string())
                }
                Err(error) => Err(error.to_string()),
            };
            let _ = tx.send(Completion { index, id, took: started.elapsed(), result });
        });
    }

    /// A restart-style shutdown: drains the driver's unflushed batches into
    /// the log and returns the log bytes.
    fn shutdown(self) -> Vec<u8> {
        let LiveCore { name, driver, mut log, mut log_file, .. } = self;
        for batch in driver.shutdown() {
            write_frame(name, &mut log, &mut log_file, &persist_log::encode_batch(&batch));
        }
        log
    }

    /// This core's last provider state, for a timeout panic.
    fn describe(&self) -> String {
        let mut out = format!("[{}] vault={:?} last_error={:?}\n", self.name, self.provider.vault_state(), self.provider.last_error());
        let mut rooms = self.provider.rooms();
        rooms.sort_by_key(|room| std::cmp::Reverse(room.last_event.as_ref().map(|event| event.ts)));
        out.push_str(&format!("  rooms held: {}\n", rooms.len()));
        for room in rooms.iter().take(4) {
            out.push_str(&format!(
                "  room {} {:?} name={:?} enc={} public={} can_post={} role={:?} notif={}\n",
                room.id, room.kind, room.name, room.is_encrypted, room.is_public, room.can_post, room.my_role, room.notification_count
            ));
            let timeline = self.provider.timeline(&room.id);
            for item in timeline.iter().rev().take(4).rev() {
                out.push_str(&format!(
                    "    item {} from {} {:?} redacted={} edited={} state={:?} read_by={} reactions={}\n",
                    item.event_id,
                    item.sender,
                    item.content,
                    item.redacted,
                    item.edited,
                    item.send_state,
                    item.read_by.len(),
                    item.reactions.len()
                ));
            }
        }
        out.push_str(&format!("  last requests: {}\n", self.requests.tail(6).join(" | ")));
        out.push_str(&format!("  failures (newest first): {}\n", self.requests.failures().join(" | ")));
        out
    }
}

/// Appends `frame` to the in-memory log and, if there is one, its file mirror.
fn write_frame(name: &str, log: &mut Vec<u8>, file: &mut Option<File>, frame: &[u8]) {
    log.extend_from_slice(frame);
    if let Some(file) = file {
        file.write_all(frame).and_then(|()| file.sync_data()).unwrap_or_else(|error| panic!("[{name}] cannot persist the record log: {error}"));
    }
}

/// Steps every core in `cores` every [`STEP_INTERVAL`] until `cond` holds,
/// returning how long that took, or `None` if `timeout` ran out first. A core
/// that cannot boot (terminal `whoami` failure) panics immediately.
fn try_wait_until(cores: &mut [&mut LiveCore], timeout: Duration, mut cond: impl FnMut() -> bool) -> Option<Duration> {
    let started = Instant::now();
    loop {
        for core in cores.iter_mut() {
            core.step();
        }
        if cond() {
            return Some(started.elapsed());
        }
        for core in cores.iter() {
            let error = core.provider.last_error().unwrap_or_default();
            assert!(!error.starts_with("whoami failed"), "core {} could not boot: {error}", core.name);
        }
        if started.elapsed() > timeout {
            return None;
        }
        std::thread::sleep(STEP_INTERVAL);
    }
}

/// [`try_wait_until`] with [`WAIT_TIMEOUT`]; a timeout panics naming `label`
/// and dumping each core's last provider state.
fn wait_until(cores: &mut [&mut LiveCore], label: &str, cond: impl FnMut() -> bool) -> Duration {
    match try_wait_until(cores, WAIT_TIMEOUT, cond) {
        Some(took) => took,
        None => {
            let dump: String = cores.iter().map(|core| core.describe()).collect();
            panic!("timed out after {WAIT_TIMEOUT:?} waiting for: {label}\n{dump}");
        }
    }
}

/// Keeps every core pumping for `duration` (a negative check needs the
/// server round-trips to have had time to happen).
fn settle(cores: &mut [&mut LiveCore], duration: Duration) {
    let started = Instant::now();
    while started.elapsed() < duration {
        for core in cores.iter_mut() {
            core.step();
        }
        std::thread::sleep(STEP_INTERVAL);
    }
}

/// `true` once a core is running, has completed a first `/sync` and (unless
/// it resumed an account that published its keys long ago) has published its
/// device keys.
fn ready_probe(core: &LiveCore) -> impl Fn() -> bool {
    let provider = core.provider();
    let requests = core.requests();
    let resumed = core.resumed;
    move || {
        provider.vault_state() == VaultState::Ready
            && requests.ok_since(0, "GET", "/sync") >= 1
            && (resumed || requests.ok_since(0, "POST", "/keys/upload") >= 1)
    }
}

/// Waits until every core is ready (see [`ready_probe`]).
fn boot_all(cores: &mut [&mut LiveCore]) -> Duration {
    let probes: Vec<Box<dyn Fn() -> bool>> = cores.iter().map(|core| Box::new(ready_probe(core)) as Box<dyn Fn() -> bool>).collect();
    wait_until(cores, "every core booted (Ready, first /sync, keys published)", || probes.iter().all(|probe| probe()))
}

/// A blocking, authenticated HTTP handle (the legacy `/api/dm/*` calls).
/// No `Debug`: it holds a bearer token.
struct Api {
    client: reqwest::blocking::Client,
    base_url: String,
    token: String,
    forwarded_for: String,
}

impl Api {
    /// Sends `body` (if any) with `method` to `path`; returns the status and
    /// the parsed JSON body (`Null` if it was not JSON).
    fn call(&self, method: reqwest::Method, path: &str, body: Option<serde_json::Value>) -> (u16, serde_json::Value) {
        let mut builder = self
            .client
            .request(method, format!("{}{path}", self.base_url))
            .timeout(DEFAULT_TIMEOUT)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("X-Forwarded-For", &self.forwarded_for);
        if let Some(body) = &body {
            builder = builder.json(body);
        }
        let response = builder.send().unwrap_or_else(|error| panic!("{path}: transport error: {error}"));
        let status = response.status().as_u16();
        let bytes = response.bytes().unwrap_or_default();
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }
}

// ---------------------------------------------------------------------
// Scenario helpers
// ---------------------------------------------------------------------

/// Prints one timing/finding line (`--nocapture` shows them).
fn report(scenario: &str, label: &str, took: Duration) {
    println!("LIVE {scenario} | {label}: {} ms", took.as_millis());
}

fn note(scenario: &str, text: &str) {
    println!("LIVE {scenario} | {text}");
}

/// The shell-side `enqueue`: the driver drains it on its next tick.
fn enqueue(provider: &SnapshotProvider, intent: MessengerIntent) {
    let mut handle = provider.clone();
    handle.enqueue(intent);
}

fn room_ids(provider: &SnapshotProvider) -> BTreeSet<RoomId> {
    provider.rooms().into_iter().map(|room| room.id).collect()
}

/// A room `provider` holds that is not in `before`.
fn new_room(provider: &SnapshotProvider, before: &BTreeSet<RoomId>) -> Option<RoomSummary> {
    provider.rooms().into_iter().find(|room| !before.contains(&room.id))
}

/// The timeline item in `room` whose content is exactly `Text(text)`.
fn text_item(provider: &SnapshotProvider, room: &RoomId, text: &str) -> Option<TimelineItem> {
    provider.timeline(room).into_iter().find(|item| item.content == MessageContent::Text(text.to_string()))
}

fn send_text(provider: &SnapshotProvider, room: &RoomId, text: &str) {
    enqueue(provider, MessengerIntent::SendText { room: room.clone(), text: text.to_string(), reply_to: None });
}

/// Whether `provider`'s core tracks at least one device of `user` in `room`
/// (`Member::has_messaging_key`) -- i.e. it can already encrypt for them.
fn member_has_key(provider: &SnapshotProvider, room: &RoomId, user: &UserId) -> bool {
    provider.members(room).iter().any(|member| &member.user_id == user && member.has_messaging_key)
}

fn create_request(kind: RoomKind, title: Option<String>, members: Vec<UserId>, visibility: RoomVisibility) -> MessengerIntent {
    MessengerIntent::CreateRoom(CreateRoomRequest { kind, title, description: None, member_ids: members, members_can_add: false, visibility })
}

/// alice creates a DM with bob, bob joins, and both cores know each other's
/// devices (so the next message can be encrypted for the peer).
fn establish_dm(scenario: &str, alice: &mut LiveCore, bob: &mut LiveCore, stand: &Stand) -> RoomId {
    let (ap, bp) = (alice.provider(), bob.provider());
    let (alice_user, bob_user) = (stand.mxid(Who::Alice), stand.mxid(Who::Bob));
    let before = room_ids(&ap);
    enqueue(&ap, create_request(RoomKind::Direct, None, vec![bob_user.clone()], RoomVisibility::Private));
    let took = wait_until(&mut [&mut *alice, &mut *bob], "alice's provider shows the DM room she created", || new_room(&ap, &before).is_some());
    report(scenario, "alice CreateRoom(Direct) -> room in her provider", took);
    let room = new_room(&ap, &before).map(|summary| summary.id).expect("the wait above proved the room exists");

    let took = wait_until(&mut [&mut *alice, &mut *bob], "bob's provider shows the DM invite", || bp.room(&room).is_some());
    report(scenario, "room in alice's provider -> invite visible on bob", took);
    enqueue(&bp, MessengerIntent::JoinRoom { room: room.clone() });
    let took = wait_until(&mut [&mut *alice, &mut *bob], "bob's JoinRoom completes (can_post)", || bp.room(&room).is_some_and(|summary| summary.can_post));
    report(scenario, "bob JoinRoom -> joined", took);

    let took = wait_until(&mut [&mut *alice, &mut *bob], "both cores track each other's devices", || {
        member_has_key(&ap, &room, &bob_user) && member_has_key(&bp, &room, &alice_user)
    });
    report(scenario, "bob joined -> both sides hold the peer's device keys", took);
    room
}

// ---------------------------------------------------------------------
// S1 -- direct message lifecycle
// ---------------------------------------------------------------------

#[test]
#[ignore = "live: needs the local stand server (MLC_LIVE_STAND_ENV)"]
fn s1_direct_message_lifecycle() {
    let stand = Stand::load();
    let mut alice = stand.core(Who::Alice);
    let mut bob = stand.core(Who::Bob);
    let (ap, bp) = (alice.provider(), bob.provider());
    let bob_user = stand.mxid(Who::Bob);

    let took = boot_all(&mut [&mut alice, &mut bob]);
    report("S1", "boot both cores (whoami -> keys uploaded -> first /sync)", took);
    let room = establish_dm("S1", &mut alice, &mut bob, &stand);

    send_text(&ap, &room, "hello");
    let took = wait_until(&mut [&mut alice, &mut bob], "bob's timeline shows Text(hello)", || text_item(&bp, &room, "hello").is_some());
    report("S1", "alice send -> bob sees decrypted Text", took);
    let summary = bp.room(&room).expect("bob holds the DM room");
    assert!(summary.is_encrypted, "the DM room must be encrypted");
    note("S1", &format!("bob's DM room: kind={:?} name={:?} encrypted={} can_post={}", summary.kind, summary.name, summary.is_encrypted, summary.can_post));

    let took = wait_until(&mut [&mut alice, &mut bob], "alice's item is Sent with a server event id", || {
        text_item(&ap, &room, "hello").is_some_and(|item| item.send_state == SendState::Sent && item.event_id.as_str().starts_with('$'))
    });
    report("S1", "alice send -> her own item Sent", took);
    let alice_item = text_item(&ap, &room, "hello").expect("alice's item exists");
    let bob_item = text_item(&bp, &room, "hello").expect("bob's item exists");
    assert_eq!(alice_item.event_id, bob_item.event_id, "both cores agree on the server event id");

    enqueue(&bp, MessengerIntent::MarkRead { room: room.clone(), event_id: bob_item.event_id.clone() });
    let took = wait_until(&mut [&mut alice, &mut bob], "alice's item read_by contains bob", || {
        text_item(&ap, &room, "hello").is_some_and(|item| item.read_by.contains(&bob_user))
    });
    report("S1", "bob MarkRead -> alice read_by", took);

    enqueue(&bp, MessengerIntent::React { room: room.clone(), event_id: bob_item.event_id.clone(), key: THUMBS_UP.to_string() });
    let took = wait_until(&mut [&mut alice, &mut bob], "alice's item shows bob's reaction", || {
        text_item(&ap, &room, "hello").is_some_and(|item| item.reactions.iter().any(|r| r.key == THUMBS_UP && r.count == 1 && !r.by_me))
    });
    report("S1", "bob React -> alice sees it", took);

    enqueue(&ap, MessengerIntent::Edit { room: room.clone(), event_id: alice_item.event_id.clone(), text: "hello (edited)".to_string() });
    let took = wait_until(&mut [&mut alice, &mut bob], "bob's item shows the edited text", || {
        bp.timeline(&room).iter().any(|item| item.event_id == bob_item.event_id && item.edited && item.content == MessageContent::Text("hello (edited)".to_string()))
    });
    report("S1", "alice Edit -> bob sees edited text", took);

    enqueue(&ap, MessengerIntent::Redact { room: room.clone(), event_id: alice_item.event_id.clone() });
    let took = wait_until(&mut [&mut alice, &mut bob], "bob's item is redacted", || {
        bp.timeline(&room).iter().any(|item| item.event_id == bob_item.event_id && item.redacted)
    });
    report("S1", "alice Redact -> bob sees redacted", took);
}

// ---------------------------------------------------------------------
// S2 -- group of three, then one leaves
// ---------------------------------------------------------------------

#[test]
#[ignore = "live: needs the local stand server (MLC_LIVE_STAND_ENV)"]
fn s2_group_with_leave() {
    let stand = Stand::load();
    let mut alice = stand.core(Who::Alice);
    let mut bob = stand.core(Who::Bob);
    let mut carol = stand.core(Who::Carol);
    let (ap, bp, cp) = (alice.provider(), bob.provider(), carol.provider());
    let (alice_user, bob_user, carol_user) = (stand.mxid(Who::Alice), stand.mxid(Who::Bob), stand.mxid(Who::Carol));

    let took = boot_all(&mut [&mut alice, &mut bob, &mut carol]);
    report("S2", "boot three cores", took);

    let before = room_ids(&ap);
    let title = format!("live-group-{}", nonce());
    enqueue(&ap, create_request(RoomKind::Group, Some(title), vec![bob_user.clone(), carol_user.clone()], RoomVisibility::Private));
    let took = wait_until(&mut [&mut alice, &mut bob, &mut carol], "alice's provider shows the group she created", || new_room(&ap, &before).is_some());
    report("S2", "alice CreateRoom(Group) -> room in her provider", took);
    let room = new_room(&ap, &before).map(|summary| summary.id).expect("the wait above proved the room exists");

    let took = wait_until(&mut [&mut alice, &mut bob, &mut carol], "bob and carol see the invite", || bp.room(&room).is_some() && cp.room(&room).is_some());
    report("S2", "room in alice's provider -> invites visible on bob and carol", took);
    enqueue(&bp, MessengerIntent::JoinRoom { room: room.clone() });
    enqueue(&cp, MessengerIntent::JoinRoom { room: room.clone() });
    let took = wait_until(&mut [&mut alice, &mut bob, &mut carol], "bob and carol joined (can_post)", || {
        bp.room(&room).is_some_and(|s| s.can_post) && cp.room(&room).is_some_and(|s| s.can_post)
    });
    report("S2", "both JoinRoom -> joined", took);
    let took = wait_until(&mut [&mut alice, &mut bob, &mut carol], "every member tracks every other member's devices", || {
        member_has_key(&ap, &room, &bob_user)
            && member_has_key(&ap, &room, &carol_user)
            && member_has_key(&bp, &room, &alice_user)
            && member_has_key(&bp, &room, &carol_user)
            && member_has_key(&cp, &room, &alice_user)
            && member_has_key(&cp, &room, &bob_user)
    });
    report("S2", "joined -> all six device-key views populated", took);

    send_text(&bp, &room, "hello group (from bob)");
    let started = Instant::now();
    let (mut alice_at, mut carol_at) = (None, None);
    wait_until(&mut [&mut alice, &mut bob, &mut carol], "alice and carol both decrypt bob's group message", || {
        if alice_at.is_none() && text_item(&ap, &room, "hello group (from bob)").is_some() {
            alice_at = Some(started.elapsed());
        }
        if carol_at.is_none() && text_item(&cp, &room, "hello group (from bob)").is_some() {
            carol_at = Some(started.elapsed());
        }
        alice_at.is_some() && carol_at.is_some()
    });
    report("S2", "bob send -> alice decrypts", alice_at.unwrap_or_default());
    report("S2", "bob send -> carol decrypts", carol_at.unwrap_or_default());
    assert!(ap.room(&room).is_some_and(|s| s.is_encrypted), "the group must be encrypted");

    enqueue(&cp, MessengerIntent::LeaveRoom { room: room.clone() });
    let took = wait_until(&mut [&mut alice, &mut bob, &mut carol], "carol no longer holds the room as joined", || cp.room(&room).is_none_or(|s| !s.can_post));
    report("S2", "carol LeaveRoom -> carol's provider drops it as joined", took);
    match cp.room(&room) {
        Some(summary) => note("S2", &format!("carol's provider still lists the room after leaving: can_post={} role={:?}", summary.can_post, summary.my_role)),
        None => note("S2", "carol's provider no longer lists the room at all"),
    }
    let took = wait_until(&mut [&mut alice, &mut bob, &mut carol], "alice and bob no longer list carol as a member", || {
        !ap.members(&room).iter().any(|m| m.user_id == carol_user) && !bp.members(&room).iter().any(|m| m.user_id == carol_user)
    });
    report("S2", "carol left -> alice and bob's member lists updated", took);

    send_text(&ap, &room, "after carol left");
    let took = wait_until(&mut [&mut alice, &mut bob, &mut carol], "bob decrypts alice's post-leave message", || text_item(&bp, &room, "after carol left").is_some());
    report("S2", "alice send (carol gone) -> bob decrypts", took);
    settle(&mut [&mut alice, &mut bob, &mut carol], Duration::from_millis(1500));
    assert!(text_item(&cp, &room, "after carol left").is_none(), "carol must never see a message sent after she left");
}

// ---------------------------------------------------------------------
// S3 -- public channel
// ---------------------------------------------------------------------

#[test]
#[ignore = "live: needs the local stand server (MLC_LIVE_STAND_ENV)"]
fn s3_public_channel() {
    let stand = Stand::load();
    let mut alice = stand.core(Who::Alice);
    let mut bob = stand.core(Who::Bob);
    let (ap, bp) = (alice.provider(), bob.provider());

    let took = boot_all(&mut [&mut alice, &mut bob]);
    report("S3", "boot both cores", took);

    let before = room_ids(&ap);
    let title = format!("live-channel-{}", nonce());
    enqueue(
        &ap,
        MessengerIntent::CreateRoom(CreateRoomRequest {
            kind: RoomKind::Channel,
            title: Some(title.clone()),
            description: Some("live run channel".to_string()),
            member_ids: Vec::new(),
            members_can_add: false,
            visibility: RoomVisibility::Public,
        }),
    );
    let took = wait_until(&mut [&mut alice, &mut bob], "alice's provider shows the channel she created", || new_room(&ap, &before).is_some());
    report("S3", "alice CreateRoom(Channel) -> room in her provider", took);
    let alice_view = new_room(&ap, &before).expect("the wait above proved the room exists");
    note("S3", &format!("alice's channel: kind={:?} public={} encrypted={} role={:?} can_post={}", alice_view.kind, alice_view.is_public, alice_view.is_encrypted, alice_view.my_role, alice_view.can_post));

    enqueue(&bp, MessengerIntent::SearchPublicRooms { term: title.clone() });
    let took = wait_until(&mut [&mut alice, &mut bob], "bob's directory search returns the channel", || bp.public_rooms().iter().any(|entry| entry.name == title));
    report("S3", "bob SearchPublicRooms -> channel listed", took);
    let entry = bp.public_rooms().into_iter().find(|entry| entry.name == title).expect("the wait above proved the entry exists");
    assert_eq!(entry.room_id, alice_view.id, "the directory lists the room alice created");
    let room = entry.room_id;

    enqueue(&bp, MessengerIntent::JoinRoom { room: room.clone() });
    let took = wait_until(&mut [&mut alice, &mut bob], "bob's JoinRoom on the channel completes", || {
        bp.room(&room).is_some_and(|summary| summary.my_role != MemberRole::Subscriber)
    });
    report("S3", "bob JoinRoom(channel) -> joined", took);

    send_text(&ap, &room, "channel post from alice");
    let took = wait_until(&mut [&mut alice, &mut bob], "bob's timeline shows the channel post", || text_item(&bp, &room, "channel post from alice").is_some());
    report("S3", "alice post -> bob sees it", took);
    let bob_view = bp.room(&room).expect("bob holds the channel");
    assert!(!bob_view.is_encrypted, "a public channel is not encrypted");
    assert!(bob_view.is_public, "the channel reports itself public");
    note("S3", &format!("bob in the channel: can_post={} role={:?} power_level={}", bob_view.can_post, bob_view.my_role, bob_view.my_power_level.0));
}

// ---------------------------------------------------------------------
// S4 -- folders + pinned + muted reach a fresh second device
// ---------------------------------------------------------------------

#[test]
#[ignore = "live: needs the local stand server (MLC_LIVE_STAND_ENV)"]
fn s4_folders_reach_a_second_device() {
    let stand = Stand::load();
    let mut alice = stand.core(Who::Alice);
    let mut bob = stand.core(Who::Bob);
    let ap = alice.provider();
    let alice_log = alice.requests();

    let took = boot_all(&mut [&mut alice, &mut bob]);
    report("S4", "boot both cores", took);
    let room = establish_dm("S4", &mut alice, &mut bob, &stand);

    let marker = alice_log.len();
    let folder_tag = TagName::from("u.live");
    enqueue(
        &ap,
        MessengerIntent::CreateFolder(Folder {
            id: Some(folder_tag.clone()),
            name: "Live".to_string(),
            icon: FolderIcon::Folder,
            include: FolderInclude { rooms: vec![room.clone()], ..FolderInclude::default() },
            exclude: FolderExclude::default(),
            order: 1.0,
        }),
    );
    let took = wait_until(&mut [&mut alice, &mut bob], "alice's own view lists the u.live folder", || {
        ap.folders().iter().any(|folder| folder.id.as_ref() == Some(&folder_tag))
    });
    report("S4", "alice CreateFolder -> visible in her provider", took);
    enqueue(&ap, MessengerIntent::SetPinned { room: room.clone(), pinned: true });
    let took = wait_until(&mut [&mut alice, &mut bob], "alice's own view shows the room pinned", || ap.room(&room).is_some_and(|s| s.pinned));
    report("S4", "alice SetPinned -> visible in her provider", took);
    enqueue(&ap, MessengerIntent::SetMuted { room: room.clone(), muted: true });
    let took = wait_until(&mut [&mut alice, &mut bob], "alice's own view shows the room muted", || ap.room(&room).is_some_and(|s| s.muted));
    report("S4", "alice SetMuted -> visible in her provider", took);

    // Four account-data writes: two m.tag (folder tag, favourite), the
    // folders list, the muted flag. The second device must boot AFTER they
    // reached the server, or it would rightly see the old state.
    let took = wait_until(&mut [&mut alice, &mut bob], "alice's four account-data writes reached the server", || {
        alice_log.ok_since(marker, "PUT", "/account_data/") >= 4
    });
    report("S4", "SetMuted -> all account-data writes acknowledged by the server", took);

    let mut alice2 = stand.fresh_core(Who::Alice, "alice2");
    let a2p = alice2.provider();
    let took = wait_until(&mut [&mut alice, &mut bob, &mut alice2], "alice2 shows the folder, the pin and the mute from account data", || {
        a2p.folders().iter().any(|folder| folder.id.as_ref() == Some(&folder_tag) && folder.include.rooms.contains(&room))
            && a2p.room(&room).is_some_and(|s| s.pinned && s.muted && s.tags.iter().any(|tag| tag.name == folder_tag))
    });
    report("S4", "alice2 fresh boot -> folder + pinned + muted visible", took);
    let folder = a2p.folders().into_iter().find(|folder| folder.id.as_ref() == Some(&folder_tag)).expect("the wait above proved the folder exists");
    assert_eq!(folder.name, "Live");
    assert_eq!(folder.icon, FolderIcon::Folder);
}

// ---------------------------------------------------------------------
// S5 -- a second device: old history sealed, new messages readable
// ---------------------------------------------------------------------

#[test]
#[ignore = "live: needs the local stand server (MLC_LIVE_STAND_ENV)"]
fn s5_second_device_gets_new_keys_not_old_history() {
    let stand = Stand::load();
    let mut alice = stand.core(Who::Alice);
    let mut bob = stand.core(Who::Bob);
    let (ap, bp) = (alice.provider(), bob.provider());
    let bob_log = bob.requests();

    let took = boot_all(&mut [&mut alice, &mut bob]);
    report("S5", "boot both cores", took);
    let room = establish_dm("S5", &mut alice, &mut bob, &stand);

    send_text(&bp, &room, "before alice2");
    let took = wait_until(&mut [&mut alice, &mut bob], "alice1 decrypts the message sent before alice2 existed", || text_item(&ap, &room, "before alice2").is_some());
    report("S5", "bob send -> alice1 decrypts (alice2 not booted yet)", took);
    let before_id = text_item(&bp, &room, "before alice2").expect("bob's own item exists").event_id;

    let bob_marker = bob_log.len();
    let mut alice2 = stand.fresh_core(Who::Alice, "alice2");
    let a2p = alice2.provider();
    let ready2 = ready_probe(&alice2);
    let sealed = MessageContent::Undecryptable(UtdReason::MissingSession);
    let took = wait_until(&mut [&mut alice, &mut bob, &mut alice2], "alice2 is ready and shows the earlier message as Undecryptable(MissingSession)", || {
        ready2() && a2p.timeline(&room).iter().any(|item| item.event_id == before_id && item.content == sealed)
    });
    report("S5", "alice2 fresh boot -> earlier message shown as Undecryptable(MissingSession)", took);
    note("S5", "expected: key backup is deferred, so history from before this device existed stays sealed");

    let learned = try_wait_until(&mut [&mut alice, &mut bob, &mut alice2], Duration::from_secs(10), || bob_log.ok_since(bob_marker, "POST", "/keys/query") >= 1);
    match learned {
        Some(took) => report("S5", "alice2 online -> bob issued a /keys/query (learned the new device)", took),
        None => note("S5", "bob issued no /keys/query within 10 s of alice2 coming online (no device-list wake reached him)"),
    }

    send_text(&bp, &room, "after alice2 joined");
    let started = Instant::now();
    let (mut alice1_at, mut alice2_at) = (None, None);
    wait_until(&mut [&mut alice, &mut bob, &mut alice2], "alice1 and alice2 both decrypt bob's new message", || {
        if alice1_at.is_none() && text_item(&ap, &room, "after alice2 joined").is_some() {
            alice1_at = Some(started.elapsed());
        }
        if alice2_at.is_none() && text_item(&a2p, &room, "after alice2 joined").is_some() {
            alice2_at = Some(started.elapsed());
        }
        alice1_at.is_some() && alice2_at.is_some()
    });
    report("S5", "bob send -> alice1 decrypts", alice1_at.unwrap_or_default());
    report("S5", "bob send -> alice2 decrypts (room key shared to the new device)", alice2_at.unwrap_or_default());

    let old_item = a2p.timeline(&room).into_iter().find(|item| item.event_id == before_id).expect("alice2 still lists the earlier message");
    assert_eq!(old_item.content, MessageContent::Undecryptable(UtdReason::MissingSession), "the pre-alice2 message stays sealed on alice2");
}

// ---------------------------------------------------------------------
// S6 -- legacy DM bridged into Matrix
// ---------------------------------------------------------------------

/// The room in `provider` whose timeline holds a decrypted legacy item with
/// exactly this plaintext.
fn legacy_room(provider: &SnapshotProvider, text: &str) -> Option<RoomId> {
    let wanted = MessageContent::Legacy(LegacyContent::Text(text.to_string()));
    provider.rooms().into_iter().map(|room| room.id).find(|id| provider.timeline(id).iter().any(|item| item.content == wanted))
}

/// Seals `text` exactly as the old shells did (`conversation_key` +
/// `seal_message`, AAD = `message_aad(conversation id, sender fingerprint)`)
/// and posts it to the legacy `/api/dm` route as `sender`.
fn legacy_send(api: &Api, conv_id: i64, sender: &mlc_vault::dm::DmIdentity, peer_public: &[u8; 32], text: &str) {
    let key = mlc_vault::dm::conversation_key(&sender.secret, peer_public, &conv_id.to_string());
    let aad = mlc_vault::dm::message_aad(conv_id, &mlc_vault::dm::fingerprint(&sender.public));
    let sealed = mlc_vault::dm::seal_message(&key, text.as_bytes(), &aad);
    let engine = base64::engine::general_purpose::STANDARD;
    let body = serde_json::json!({ "nonce_b64": engine.encode(sealed.nonce), "ciphertext_b64": engine.encode(&sealed.ciphertext) });
    let (status, response) = api.call(reqwest::Method::POST, &format!("/api/dm/conversations/{conv_id}/messages"), Some(body));
    assert!((200..300).contains(&status), "POST /api/dm/conversations/{conv_id}/messages -> {status} {response}");
}

#[test]
#[ignore = "live: needs the local stand server (MLC_LIVE_STAND_ENV)"]
fn s6_legacy_dm_bridges_into_a_matrix_room() {
    let stand = Stand::load();
    let mut alice = stand.core(Who::Alice);
    let mut bob = stand.core(Who::Bob);
    let (ap, bp) = (alice.provider(), bob.provider());

    let took = boot_all(&mut [&mut alice, &mut bob]);
    report("S6", "boot both cores", took);
    // A native DM between the same two accounts, to answer "same room or not".
    let native_room = establish_dm("S6", &mut alice, &mut bob, &stand);

    let alice_identity = mlc_vault::dm::DmIdentity::from_vault_key(&Who::Alice.vault_key());
    let bob_identity = mlc_vault::dm::DmIdentity::from_vault_key(&Who::Bob.vault_key());
    let (alice_api, bob_api) = (stand.api(Who::Alice), stand.api(Who::Bob));
    let engine = base64::engine::general_purpose::STANDARD;
    for (who, api, identity) in [("alice", &alice_api, &alice_identity), ("bob", &bob_api, &bob_identity)] {
        let (status, body) = api.call(reqwest::Method::PUT, "/api/dm/identity", Some(serde_json::json!({ "public_key_b64": engine.encode(identity.public) })));
        assert!((200..300).contains(&status), "{who}: PUT /api/dm/identity -> {status} {body}");
    }
    let (status, body) = alice_api.call(reqwest::Method::POST, "/api/dm/conversations", Some(serde_json::json!({ "with": stand.public_id(Who::Bob) })));
    assert!((200..300).contains(&status), "POST /api/dm/conversations -> {status} {body}");
    let conv_id = body.get("id").and_then(serde_json::Value::as_i64).unwrap_or_else(|| panic!("no numeric id in the conversation response: {body}"));
    note("S6", &format!("legacy conversation id {conv_id}"));

    let tag = nonce();
    let (one, two, reply) = (format!("legacy one {tag}"), format!("legacy two {tag}"), format!("legacy reply from bob {tag}"));
    legacy_send(&alice_api, conv_id, &alice_identity, &bob_identity.public, &one);
    legacy_send(&alice_api, conv_id, &alice_identity, &bob_identity.public, &two);
    let started = Instant::now();
    let (mut alice_at, mut bob_at) = (None, None);
    wait_until(&mut [&mut alice, &mut bob], "both providers show both legacy messages as Legacy(Text)", || {
        if alice_at.is_none() && legacy_room(&ap, &one).is_some() && legacy_room(&ap, &two).is_some() {
            alice_at = Some(started.elapsed());
        }
        if bob_at.is_none() && legacy_room(&bp, &one).is_some() && legacy_room(&bp, &two).is_some() {
            bob_at = Some(started.elapsed());
        }
        alice_at.is_some() && bob_at.is_some()
    });
    report("S6", "legacy POST (alice) -> alice's provider shows Legacy(Text)", alice_at.unwrap_or_default());
    report("S6", "legacy POST (alice) -> bob's provider shows Legacy(Text)", bob_at.unwrap_or_default());
    let alice_room = legacy_room(&ap, &one).expect("alice shows the legacy room");
    assert_eq!(Some(alice_room.clone()), legacy_room(&bp, &one), "alice and bob see the same bridged room");
    assert_eq!(Some(alice_room.clone()), legacy_room(&ap, &two), "both legacy messages land in one room");

    legacy_send(&bob_api, conv_id, &bob_identity, &alice_identity.public, &reply);
    let took = wait_until(&mut [&mut alice, &mut bob], "the reply sealed by bob shows as Legacy(Text) on both", || {
        legacy_room(&ap, &reply) == Some(alice_room.clone()) && legacy_room(&bp, &reply) == Some(alice_room.clone())
    });
    report("S6", "legacy POST (bob's reply, bob's fingerprint in the AAD) -> visible on both", took);

    let bridged = ap.room(&alice_room).expect("alice holds the bridged room");
    let native = ap.room(&native_room).expect("alice holds the native DM");
    note("S6", &format!("bridged room same as the native DM room: {}", alice_room == native_room));
    note("S6", &format!("bridged room in the UI: kind={:?} name={:?} encrypted={} can_post={} last_event={:?}", bridged.kind, bridged.name, bridged.is_encrypted, bridged.can_post, bridged.last_event.as_ref().map(|e| &e.content)));
    note("S6", &format!("native DM in the UI:    kind={:?} name={:?} encrypted={} can_post={}", native.kind, native.name, native.is_encrypted, native.can_post));
    let direct_with_bob = ap.rooms().iter().filter(|room| room.kind == RoomKind::Direct && room.name == bridged.name).count();
    note("S6", &format!("alice's chat list holds {direct_with_bob} Direct room(s) named {:?} in total", bridged.name));
}

// ---------------------------------------------------------------------
// S7 -- restart from the persisted record log
// ---------------------------------------------------------------------

#[test]
#[ignore = "live: needs the local stand server (MLC_LIVE_STAND_ENV)"]
fn s7_restart_resumes_from_the_log() {
    let stand = Stand::load();
    let mut alice = stand.core(Who::Alice);
    let mut bob = stand.core(Who::Bob);
    let (ap, bp) = (alice.provider(), bob.provider());

    let took = boot_all(&mut [&mut alice, &mut bob]);
    report("S7", "boot both cores", took);
    let room = establish_dm("S7", &mut alice, &mut bob, &stand);

    let history = ["s7 history one", "s7 history two", "s7 history three (bob)"];
    send_text(&ap, &room, history[0]);
    send_text(&ap, &room, history[1]);
    send_text(&bp, &room, history[2]);
    wait_until(&mut [&mut alice, &mut bob], "the whole S1-style history is visible on both cores", || {
        text_item(&bp, &room, history[0]).is_some() && text_item(&bp, &room, history[1]).is_some() && text_item(&ap, &room, history[2]).is_some()
    });

    let bob_log = bob.shutdown();
    note("S7", &format!("bob shut down; his record log is {} bytes", bob_log.len()));
    let (replayed, valid_len) = persist_log::replay(&bob_log);
    assert_eq!(valid_len, bob_log.len(), "every frame of bob's log replays");
    note("S7", &format!("replay yields {} records", replayed.len()));

    let mut bob2 = stand.core_with_log(Who::Bob, bob_log);
    let bp2 = bob2.provider();
    let bob2_log = bob2.requests();
    let mut problems: Vec<String> = Vec::new();

    let took = try_wait_until(&mut [&mut alice, &mut bob2], WAIT_TIMEOUT, || bp2.vault_state() == VaultState::Ready).expect("the restarted core reaches Ready");
    report("S7", "restart -> Ready (whoami + records replay)", took);
    let visible_at_boot = history.iter().filter(|text| text_item(&bp2, &room, text).is_some()).count();
    note("S7", &format!("at Ready, before any /sync answer: room known={} timeline items={} history messages visible={visible_at_boot}/3", bp2.room(&room).is_some(), bp2.timeline(&room).len()));
    if visible_at_boot != history.len() {
        problems.push(format!("only {visible_at_boot}/3 history messages are visible right after boot"));
    }

    let after_sync = try_wait_until(&mut [&mut alice, &mut bob2], Duration::from_secs(5), || history.iter().all(|text| text_item(&bp2, &room, text).is_some()));
    match after_sync {
        Some(took) => report("S7", "Ready -> full history visible after the first /sync", took),
        None => {
            note("S7", "history still incomplete 5 s after Ready; asking for older events (LoadOlder)");
            enqueue(&bp2, MessengerIntent::LoadOlder { room: room.clone() });
            match try_wait_until(&mut [&mut alice, &mut bob2], WAIT_TIMEOUT, || history.iter().all(|text| text_item(&bp2, &room, text).is_some())) {
                Some(took) => report("S7", "LoadOlder -> full history visible", took),
                None => problems.push("history never came back, not even after LoadOlder".to_string()),
            }
        }
    }

    assert!(
        !bob2_log.body_has_key_since(0, "/keys/upload", "device_keys"),
        "the restarted core re-published its device keys instead of reusing the persisted Olm account"
    );
    note("S7", "no device_keys in any /keys/upload after the restart (Olm account reused)");

    send_text(&ap, &room, "s7 after restart (alice)");
    let took = wait_until(&mut [&mut alice, &mut bob2], "the restarted bob decrypts alice's new message", || text_item(&bp2, &room, "s7 after restart (alice)").is_some());
    report("S7", "alice send -> restarted bob decrypts", took);
    send_text(&bp2, &room, "s7 after restart (bob)");
    let took = wait_until(&mut [&mut alice, &mut bob2], "alice decrypts the restarted bob's message", || text_item(&ap, &room, "s7 after restart (bob)").is_some());
    report("S7", "restarted bob send -> alice decrypts", took);

    assert!(problems.is_empty(), "S7 findings: {}", problems.join("; "));
}
