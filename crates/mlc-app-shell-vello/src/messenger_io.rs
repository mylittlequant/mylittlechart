//! `MessengerIo` — the native shell's I/O for `mlc_messenger::driver::
//! MessengerDriver`: HTTP over `reqwest` on the shell's own live-data
//! `tokio` runtime, and durable persistence under `<store>/messenger/
//! <device_id>/` on one dedicated writer thread.
//!
//! `MessengerDriver` itself performs no I/O (see its own module doc) — it
//! only ever hands out `DriverRequest`s and `FlushBatch`es and expects raw
//! results fed back in. This module is the thin glue that actually does
//! that I/O for the native (winit/vello) shell; the web shell has its own,
//! separate glue over `fetch`/IndexedDB.
//!
//! # Boot
//!
//! [`MessengerIo::start`] is non-blocking: it builds the driver and returns
//! immediately. The very first [`MessengerIo::tick`] call drives the
//! driver's own boot sequence (`whoami` -> load records -> running, see
//! `mlc_messenger::driver`'s own module doc). Once `whoami` resolves a
//! device id, this module spawns its one dedicated writer thread, which
//! itself loads `snapshot.bin` + `records.log` from
//! `<store>/messenger/<device_id>/`, replays them
//! ([`mlc_messenger::driver::persist_log::replay`]), truncates a torn tail,
//! compacts if the log has grown past 8 MiB, and hands the resulting
//! records back through the same completion channel every HTTP response
//! also uses.
//!
//! # Threading
//!
//! - HTTP: each `DriverRequest` is spawned as its own task on
//!   `MessengerIoConfig::runtime`; its `(id, Result<(status, body), _>)`
//!   lands in an `std::sync::mpsc` channel, and `wake()` is called so the
//!   shell knows to call [`MessengerIo::tick`] again soon.
//! - Persistence: one dedicated OS thread receives `FlushBatch`es in the
//!   order they were produced, appends each as one
//!   [`mlc_messenger::driver::persist_log::encode_batch`] frame to
//!   `records.log`, `sync_data`s, and reports back
//!   (`flush_done`) through the same completion channel. A failed write cuts
//!   the torn frame and retries once ([`persist_log::append_frame`]); a
//!   second failure puts the writer in a failed state (nothing more acked or
//!   written, `Completion::StorageFailed` -> `MessengerDriver::
//!   storage_failed`), so the messenger stalls instead of writing frames
//!   behind a torn one.
//!
//! # Wiring
//!
//! Not wired into `main.rs`/`ChartApp` here — the MLC coordinator does
//! that, choosing where a `MessengerIo` lives in `App`/`PerWindowState` and
//! how its `provider()` reaches the UI.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;

use zeroize::Zeroizing;

use mlc_messenger::driver::persist_log::{self, LogSink};
use mlc_messenger::driver::{DriverRequest, DriverRequestId, MessengerDriver};
use mlc_messenger::persist::{FlushBatch, RecordKey, SealedRecord};
use mlc_messenger::provider::SnapshotProvider;
use mlc_messenger::wire::HttpMethod;

/// `GET /_matrix/client/v3/sync`'s own long-poll path — the one request
/// this module gives a longer timeout (module doc).
const SYNC_PATH: &str = "/_matrix/client/v3/sync";
const SYNC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const DEFAULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Compact `records.log` into a fresh `snapshot.bin` once it grows past
/// this size, at boot (module doc).
const COMPACTION_THRESHOLD_BYTES: u64 = 8 * 1024 * 1024;

/// How `MessengerIo`'s HTTP calls authenticate to the homeserver.
pub enum MessengerAuth {
    /// `Authorization: Bearer <token>`.
    Bearer(String),
}

impl MessengerAuth {
    fn header_value(&self) -> String {
        match self {
            MessengerAuth::Bearer(token) => format!("Bearer {token}"),
        }
    }
}

/// Everything [`MessengerIo::start`] needs from the shell besides the vault
/// key and the auth token.
pub struct MessengerIoConfig {
    /// The `mylittlechart-server` origin (e.g. `https://chat.example.org`),
    /// with no trailing slash — every `DriverRequest::path` is appended to
    /// this verbatim.
    pub base_url: String,
    /// This profile's own store directory. Records live under
    /// `<store>/messenger/<device_id>/`.
    pub store: PathBuf,
    /// Called (from whichever thread produced new driver work) whenever the
    /// shell should call [`MessengerIo::tick`] again soon — typically wakes
    /// the UI event loop.
    pub wake: Arc<dyn Fn() + Send + Sync>,
    /// The shell's existing live-data `tokio` runtime — every HTTP request
    /// this module makes is spawned on it.
    pub runtime: tokio::runtime::Handle,
}

/// One piece of work finished in the background: an HTTP response (or
/// transport failure), a durable write completing, or the writer thread's
/// own boot-time record load finishing.
enum Completion {
    Http { id: DriverRequestId, result: Result<(u16, Vec<u8>), String> },
    FlushDone(u64),
    RecordsLoaded(Vec<SealedRecord>),
    /// The writer could not persist a batch even after cutting the torn
    /// frame and retrying once; it acks nothing further (module doc).
    StorageFailed(String),
}

/// `records.log` opened for `persist_log::append_frame`. Deliberately NOT
/// `OpenOptions::append`: on Windows an append-only handle lacks the write
/// access `set_len` needs to cut a torn frame, so the file is opened for
/// plain write and every append seeks to the end first.
struct LogFile(File);

impl LogSink for LogFile {
    fn byte_len(&mut self) -> std::io::Result<u64> {
        self.0.metadata().map(|metadata| metadata.len())
    }

    fn append_synced(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.0.seek(SeekFrom::End(0))?;
        self.0.write_all(bytes)?;
        self.0.sync_data()
    }

    fn truncate(&mut self, len: u64) -> std::io::Result<()> {
        self.0.set_len(len)?;
        self.0.seek(SeekFrom::End(0)).map(|_| ())
    }
}

fn open_log(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().create(true).write(true).truncate(false).open(path)
}

/// One job the writer thread executes, in the order it receives them.
enum WriterJob {
    Flush(FlushBatch),
}

/// Reads `snapshot.bin` + `records.log` (either or both may not exist yet),
/// replays them as one combined byte stream, truncates `records.log` to
/// drop a torn tail left by an earlier crash, compacts into a fresh
/// `snapshot.bin` if the log has grown past
/// [`COMPACTION_THRESHOLD_BYTES`], and returns the resulting records plus
/// an open, append-mode handle to `records.log` for the writer thread to
/// keep using.
fn boot_load(device_dir: &Path) -> (Vec<SealedRecord>, Option<File>) {
    let _ = fs::create_dir_all(device_dir);
    let snapshot_path = device_dir.join("snapshot.bin");
    let log_path = device_dir.join("records.log");

    let snapshot_bytes = fs::read(&snapshot_path).unwrap_or_default();
    let log_bytes = fs::read(&log_path).unwrap_or_default();

    let mut combined = Vec::with_capacity(snapshot_bytes.len() + log_bytes.len());
    combined.extend_from_slice(&snapshot_bytes);
    combined.extend_from_slice(&log_bytes);
    let (map, valid_len) = persist_log::replay(&combined);

    // A crash mid-write/mid-sync can only ever leave a torn tail inside
    // `records.log` itself -- `snapshot.bin` is only ever produced by an
    // atomic rename (`compact`, below), so it is either fully valid or
    // fully absent.
    let valid_log_len = valid_len.saturating_sub(snapshot_bytes.len());
    if valid_log_len < log_bytes.len() {
        if let Ok(file) = OpenOptions::new().write(true).open(&log_path) {
            let _ = file.set_len(valid_log_len as u64);
        }
    }

    if log_bytes.len() as u64 > COMPACTION_THRESHOLD_BYTES {
        // Folds every currently-live record into a fresh `snapshot.bin` and
        // empties `records.log` -- `map` itself (which records are live)
        // is unaffected, only where they live on disk.
        compact(device_dir, &map);
    }

    let records: Vec<SealedRecord> = map.into_iter().map(|(key, bytes)| SealedRecord { key, bytes }).collect();
    let log_file = open_log(&log_path).ok();
    (records, log_file)
}

/// Writes `map` as a fresh `snapshot.bin` (via a `.tmp` file + `fsync` +
/// atomic rename, so a crash mid-compaction never leaves a half-written
/// `snapshot.bin` in place) and, only once that succeeds, truncates
/// `records.log` to empty — the compaction [`boot_load`] triggers past
/// [`COMPACTION_THRESHOLD_BYTES`].
fn compact(device_dir: &Path, map: &BTreeMap<RecordKey, Vec<u8>>) {
    let snapshot_path = device_dir.join("snapshot.bin");
    let tmp_path = device_dir.join("snapshot.bin.tmp");
    let log_path = device_dir.join("records.log");

    let bytes = persist_log::encode_snapshot(map);
    let Ok(mut tmp) = File::create(&tmp_path) else { return };
    if tmp.write_all(&bytes).is_err() || tmp.sync_all().is_err() {
        return;
    }
    drop(tmp);
    if fs::rename(&tmp_path, &snapshot_path).is_err() {
        return;
    }
    if let Ok(log_file) = OpenOptions::new().write(true).open(&log_path) {
        let _ = log_file.set_len(0);
    }
}

/// The writer thread's own body: loads records once at start, reports them
/// back, then services [`WriterJob`]s until its channel closes.
fn writer_thread_main(device_dir: PathBuf, jobs_rx: mpsc::Receiver<WriterJob>, completions_tx: mpsc::Sender<Completion>, wake: Arc<dyn Fn() + Send + Sync>) {
    let (records, log_file) = boot_load(&device_dir);
    let _ = completions_tx.send(Completion::RecordsLoaded(records));
    wake();

    let mut log = log_file.map(LogFile);
    // Once storage has failed this thread acks nothing more and drops every
    // later job unwritten: a frame written behind a possibly-torn one would
    // be lost on the next boot (`persist_log::append_frame`'s own doc), and
    // its ratchet state is what already-sent ciphertext depends on. The
    // unacked batches keep the core from releasing any request that
    // depends on them, so the messenger stalls instead of diverging.
    let mut failed = false;
    let fail = |reason: String| {
        let _ = completions_tx.send(Completion::StorageFailed(reason));
        wake();
    };
    if log.is_none() {
        failed = true;
        fail("cannot open records.log for writing".to_string());
    }

    while let Ok(WriterJob::Flush(batch)) = jobs_rx.recv() {
        if failed {
            continue;
        }
        let Some(sink) = log.as_mut() else { continue };
        match persist_log::append_frame(sink, &persist_log::encode_batch(&batch)) {
            Ok(()) => {
                let _ = completions_tx.send(Completion::FlushDone(batch.id));
                wake();
            }
            Err(error) => {
                failed = true;
                fail(error.to_string());
            }
        }
    }
}

/// Writes `batches` to `<device_dir>/records.log` synchronously, on
/// whichever thread calls this — [`MessengerIo::stop`]'s own final flush,
/// after the writer thread has already been drained and joined (so this is
/// never racing that thread for the same file).
fn write_batches_sync(device_dir: &Path, batches: &[FlushBatch]) {
    if batches.is_empty() {
        return;
    }
    let log_path = device_dir.join("records.log");
    let Ok(file) = open_log(&log_path) else {
        tracing::warn!("messenger stop: cannot open records.log; {} final batch(es) not written", batches.len());
        return;
    };
    let mut log = LogFile(file);
    // Same rule as the writer thread: cut and retry once, and on the first
    // failure stop -- never write a later batch behind a failed one.
    for batch in batches {
        if let Err(error) = persist_log::append_frame(&mut log, &persist_log::encode_batch(batch)) {
            tracing::warn!("messenger stop: final write failed at batch {}: {error}", batch.id);
            return;
        }
    }
}

/// See the module doc.
pub struct MessengerIo {
    driver: MessengerDriver,
    auth: MessengerAuth,
    base_url: String,
    store: PathBuf,
    wake: Arc<dyn Fn() + Send + Sync>,
    runtime: tokio::runtime::Handle,
    client: reqwest::Client,
    completions_tx: mpsc::Sender<Completion>,
    completions_rx: mpsc::Receiver<Completion>,
    writer: Option<mpsc::Sender<WriterJob>>,
    writer_join: Option<JoinHandle<()>>,
    device_dir: Option<PathBuf>,
    /// Set once this driver's `whoami` resolved a device and this module
    /// spawned the writer for it — guards against spawning it twice.
    writer_spawned: bool,
    /// Set once the writer reported `Completion::StorageFailed`: no more
    /// batches are sent to it, and `stop` writes nothing further.
    storage_failed: bool,
}

impl MessengerIo {
    /// Builds a driver for `vault_key` and returns immediately -- nothing
    /// blocks, nothing is sent yet. The first [`MessengerIo::tick`] call
    /// starts the boot sequence.
    pub fn start(vault_key: &mlc_vault::vault::VaultKey, auth: MessengerAuth, config: MessengerIoConfig) -> Self {
        let driver = MessengerDriver::new(Zeroizing::new(*vault_key));
        let client = reqwest::Client::builder().build().unwrap_or_default();
        let (completions_tx, completions_rx) = mpsc::channel();
        Self {
            driver,
            auth,
            base_url: config.base_url,
            store: config.store,
            wake: config.wake,
            runtime: config.runtime,
            client,
            completions_tx,
            completions_rx,
            writer: None,
            writer_join: None,
            device_dir: None,
            writer_spawned: false,
            storage_failed: false,
        }
    }

    /// A clone of this driver's own [`SnapshotProvider`] -- the UI-facing
    /// half of the seam (`mlc_messenger::provider`'s own module doc).
    pub fn provider(&self) -> SnapshotProvider {
        self.driver.provider()
    }

    fn spawn_writer(&mut self, device_id: &str) {
        let device_dir = self.store.join("messenger").join(device_id);
        let completions_tx = self.completions_tx.clone();
        let wake = Arc::clone(&self.wake);
        let (jobs_tx, jobs_rx) = mpsc::channel::<WriterJob>();
        let thread_device_dir = device_dir.clone();
        // A spawn failure (OS resource exhaustion) degrades to "no
        // persistence this session" rather than panicking -- `writer`
        // stays `Some` so a flush send is a harmless no-op (the receiver
        // was dropped with the never-started closure) instead of a crash.
        self.writer_join = std::thread::Builder::new()
            .name("mlc-messenger-writer".to_string())
            .spawn(move || writer_thread_main(thread_device_dir, jobs_rx, completions_tx, wake))
            .ok();
        self.writer = Some(jobs_tx);
        self.device_dir = Some(device_dir);
    }

    fn spawn_http(&mut self, request: DriverRequest) {
        let url = format!("{}{}", self.base_url, request.path);
        let method = match request.method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
        };
        let timeout = if request.path == SYNC_PATH { SYNC_TIMEOUT } else { DEFAULT_TIMEOUT };

        let mut builder = self.client.request(method, url).timeout(timeout).header("Authorization", self.auth.header_value());
        if !request.query.is_empty() {
            builder = builder.query(&request.query);
        }
        if let Some(body) = &request.body {
            builder = builder.json(body);
        }

        let id = request.id;
        let completions_tx = self.completions_tx.clone();
        let wake = Arc::clone(&self.wake);
        self.runtime.spawn(async move {
            let result = match builder.send().await {
                Ok(response) => {
                    let status = response.status().as_u16();
                    match response.bytes().await {
                        Ok(bytes) => Ok((status, bytes.to_vec())),
                        Err(error) => Err(error.to_string()),
                    }
                }
                Err(error) => Err(error.to_string()),
            };
            let _ = completions_tx.send(Completion::Http { id, result });
            wake();
        });
    }

    /// Drains whatever finished in the background since the last call,
    /// feeds it into the driver, then advances the driver's own pump loop
    /// by one step -- see `mlc_messenger::driver`'s own module doc. Never
    /// blocks; call this once per UI frame and again whenever
    /// `MessengerIoConfig::wake` fires.
    pub fn tick(&mut self, now_ms: i64) {
        while let Ok(completion) = self.completions_rx.try_recv() {
            match completion {
                Completion::Http { id, result } => match result {
                    Ok((status, body)) => self.driver.on_response(id, status, body, now_ms),
                    Err(_) => self.driver.on_transport_error(&id, now_ms),
                },
                Completion::FlushDone(id) => self.driver.flush_done(id),
                Completion::RecordsLoaded(records) => self.driver.records_loaded(records, now_ms),
                Completion::StorageFailed(reason) => {
                    self.storage_failed = true;
                    self.driver.storage_failed(&reason);
                }
            }
        }

        let awaiting_device = self.driver.awaiting_records().map(|(_, device_id)| device_id.as_str().to_string());
        if !self.writer_spawned {
            if let Some(device_id) = awaiting_device {
                self.writer_spawned = true;
                self.spawn_writer(&device_id);
            }
        }

        let output = self.driver.tick(now_ms);
        for request in output.requests {
            self.spawn_http(request);
        }
        if !self.storage_failed {
            if let Some(writer) = &self.writer {
                for batch in output.flush {
                    let _ = writer.send(WriterJob::Flush(batch));
                }
            }
        }
    }

    /// Final synchronous flush: drains whatever the driver still had
    /// unflushed, drains and joins the writer thread FIRST (so its own
    /// already-queued writes land before this call's, preserving flush
    /// order), then writes the driver's leftover batches directly on the
    /// calling thread. Keys are zeroized on drop (`MessengerDriver`'s own
    /// doc); every record already written stays on disk.
    pub fn stop(mut self) {
        let batches = self.driver.shutdown();
        let device_dir = self.device_dir.take();

        // Dropping the sender closes the writer's channel, so its own
        // `recv()` loop ends once it has drained whatever was already
        // queued -- joining then waits for exactly that, never longer.
        self.writer = None;
        if let Some(join) = self.writer_join.take() {
            let _ = join.join();
        }

        // The writer may have failed after the last `tick` drained its
        // completions; a leftover batch written behind a dropped one would
        // leave a gap, so a failed store gets no final write at all.
        while let Ok(completion) = self.completions_rx.try_recv() {
            if matches!(completion, Completion::StorageFailed(_)) {
                self.storage_failed = true;
            }
        }
        if self.storage_failed {
            return;
        }

        if let Some(device_dir) = device_dir {
            write_batches_sync(&device_dir, &batches);
        }
    }

    /// [`MessengerIo::stop`], then deletes every record this device ever
    /// stored -- a real account sign-out, not just closing this session.
    pub fn sign_out(self) {
        let device_dir = self.device_dir.clone();
        self.stop();
        if let Some(device_dir) = device_dir {
            let _ = fs::remove_dir_all(&device_dir);
        }
    }
}
