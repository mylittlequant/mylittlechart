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
//! itself opens `records.db` (SQLCipher) at
//! `<store>/messenger/<device_id>/`. The file key is the vault key, raw,
//! `PRAGMA key = "x'<64 hex>'"`. Rows are the engine's sealed records.
//! This module never decrypts them. A directory that still has
//! `snapshot.bin` / `records.log` and an empty database is imported once
//! from [`mlc_messenger::driver::persist_log::replay`], then those files
//! are removed. The loaded records go back through the same completion
//! channel every HTTP response also uses.
//!
//! # Threading
//!
//! - HTTP: each `DriverRequest` is spawned as its own task on
//!   `MessengerIoConfig::runtime`; its `(id, Result<(status, body), _>)`
//!   lands in an `std::sync::mpsc` channel, and `wake()` is called so the
//!   shell knows to call [`MessengerIo::tick`] again soon.
//! - Persistence: one dedicated OS thread receives `FlushBatch`es in the
//!   order they were produced and applies each as one SQLCipher
//!   transaction (upserts and deletes together). Commit is the durability
//!   barrier. A failed transaction is rolled back and the writer stops
//!   (`Completion::StorageFailed` -> `MessengerDriver::storage_failed`):
//!   nothing later is acked or written, so the messenger stalls instead of
//!   continuing past a batch that did not reach disk.
//!
//! # Wiring
//!
//! Not wired into `main.rs`/`ChartApp` here — the MLC coordinator does
//! that, choosing where a `MessengerIo` lives in `App`/`PerWindowState` and
//! how its `provider()` reaches the UI.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;

use zeroize::Zeroizing;

use mlc_messenger::driver::persist_log;
use mlc_messenger::driver::{DriverRequest, DriverRequestId, MessengerDriver};
use mlc_messenger::persist::{FlushBatch, RecordKey, SealedRecord};
use mlc_messenger::provider::SnapshotProvider;
use mlc_messenger::wire::HttpMethod;

/// `GET /_matrix/client/v3/sync`'s own long-poll path — the one request
/// this module gives a longer timeout (module doc).
const SYNC_PATH: &str = "/_matrix/client/v3/sync";
const SYNC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const DEFAULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// One SQLCipher file per device. The vault key is the file key.
const STORE_FILE: &str = "records.db";

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

/// One job the writer thread executes, in the order it receives them.
enum WriterJob {
    Flush(FlushBatch),
}

/// `PRAGMA key` text for the raw 32-byte vault key. SQLCipher reads this
/// before any other statement. The string is zeroized on drop. It is not
/// logged.
fn key_pragma(key: &[u8; 32]) -> Zeroizing<String> {
    let hex_key = Zeroizing::new(hex::encode(key));
    Zeroizing::new(format!("PRAGMA key = \"x'{}'\";", hex_key.as_str()))
}

fn open_store(path: &Path, key: &[u8; 32]) -> Result<rusqlite::Connection, String> {
    let conn = rusqlite::Connection::open(path).map_err(|error| error.to_string())?;
    // The key statement has to be first. A wrong key fails on the next
    // read, not on this statement. Do not include the pragma text in the
    // error: it contains the key.
    conn.execute_batch(key_pragma(key).as_str()).map_err(|_| "sqlcipher key statement failed".to_string())?;
    conn.execute_batch("PRAGMA journal_mode=WAL;").map_err(|_| "sqlcipher file did not open with this key".to_string())?;
    conn.execute_batch("PRAGMA busy_timeout=5000;").map_err(|error| error.to_string())?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sealed (\
            key TEXT PRIMARY KEY NOT NULL,\
            bytes BLOB NOT NULL\
         );",
    )
    .map_err(|error| error.to_string())?;
    Ok(conn)
}

fn load_records(conn: &rusqlite::Connection) -> Result<Vec<SealedRecord>, String> {
    let mut stmt = conn.prepare("SELECT key, bytes FROM sealed").map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(SealedRecord { key: RecordKey::new(row.get::<_, String>(0)?), bytes: row.get::<_, Vec<u8>>(1)? })
        })
        .map_err(|error| error.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|error| error.to_string())?);
    }
    Ok(out)
}

/// One transaction. Upserts and deletes from the same batch commit
/// together or not at all. The blob is the sealed record, untouched.
fn apply_batch(conn: &mut rusqlite::Connection, batch: &FlushBatch) -> Result<(), String> {
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    {
        let mut upsert = tx
            .prepare(
                "INSERT INTO sealed(key, bytes) VALUES (?1, ?2) \
                 ON CONFLICT(key) DO UPDATE SET bytes = excluded.bytes",
            )
            .map_err(|error| error.to_string())?;
        for record in &batch.records {
            upsert
                .execute(rusqlite::params![record.key.as_str(), record.bytes.as_slice()])
                .map_err(|error| error.to_string())?;
        }
        drop(upsert);
        let mut delete = tx.prepare("DELETE FROM sealed WHERE key = ?1").map_err(|error| error.to_string())?;
        for key in &batch.deletes {
            delete.execute(rusqlite::params![key.as_str()]).map_err(|error| error.to_string())?;
        }
    }
    tx.commit().map_err(|error| error.to_string())
}

fn sealed_count(conn: &rusqlite::Connection) -> Result<i64, String> {
    conn.query_row("SELECT COUNT(*) FROM sealed", [], |row| row.get(0)).map_err(|error| error.to_string())
}

/// Reads a pre-SQLCipher `snapshot.bin` + `records.log` into an empty
/// database, then removes those files. A torn log with no valid record is
/// left in place. A database that already has rows is left alone.
fn import_legacy(conn: &mut rusqlite::Connection, device_dir: &Path) -> Result<(), String> {
    let snapshot_path = device_dir.join("snapshot.bin");
    let log_path = device_dir.join("records.log");
    let snapshot_exists = snapshot_path.is_file();
    let log_exists = log_path.is_file();
    if !snapshot_exists && !log_exists {
        return Ok(());
    }
    if sealed_count(conn)? > 0 {
        return Ok(());
    }
    let snapshot_bytes = if snapshot_exists { fs::read(&snapshot_path).map_err(|error| error.to_string())? } else { Vec::new() };
    let log_bytes = if log_exists { fs::read(&log_path).map_err(|error| error.to_string())? } else { Vec::new() };
    let mut combined = Vec::with_capacity(snapshot_bytes.len() + log_bytes.len());
    combined.extend_from_slice(&snapshot_bytes);
    combined.extend_from_slice(&log_bytes);
    let (map, valid_len) = persist_log::replay(&combined);
    if map.is_empty() && valid_len < combined.len() {
        return Ok(());
    }
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    {
        let mut upsert = tx.prepare("INSERT INTO sealed(key, bytes) VALUES (?1, ?2)").map_err(|error| error.to_string())?;
        for (key, bytes) in &map {
            upsert.execute(rusqlite::params![key.as_str(), bytes.as_slice()]).map_err(|error| error.to_string())?;
        }
    }
    tx.commit().map_err(|error| error.to_string())?;
    if snapshot_exists {
        let _ = fs::remove_file(&snapshot_path);
    }
    if log_exists {
        let _ = fs::remove_file(&log_path);
    }
    let _ = fs::remove_file(device_dir.join("snapshot.bin.tmp"));
    Ok(())
}

fn boot_store(device_dir: &Path, key: &[u8; 32]) -> Result<(Vec<SealedRecord>, rusqlite::Connection), String> {
    fs::create_dir_all(device_dir).map_err(|error| error.to_string())?;
    let mut conn = open_store(&device_dir.join(STORE_FILE), key)?;
    import_legacy(&mut conn, device_dir)?;
    let records = load_records(&conn)?;
    Ok((records, conn))
}

/// The writer thread's own body: loads records once at start, reports them
/// back, then services [`WriterJob`]s until its channel closes.
fn writer_thread_main(
    device_dir: PathBuf,
    key: Zeroizing<[u8; 32]>,
    jobs_rx: mpsc::Receiver<WriterJob>,
    completions_tx: mpsc::Sender<Completion>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let opened = boot_store(&device_dir, &key);
    let (records, mut conn) = match opened {
        Ok(opened) => opened,
        Err(error) => {
            // No `RecordsLoaded`: an empty load would open the core on a
            // blank history while the file sits unread.
            let _ = completions_tx.send(Completion::StorageFailed(error));
            wake();
            while jobs_rx.recv().is_ok() {}
            return;
        }
    };
    let _ = completions_tx.send(Completion::RecordsLoaded(records));
    wake();

    // Once storage has failed this thread acks nothing more and drops every
    // later job unwritten. The unacked batches keep the core from releasing
    // any request that depends on them, so the messenger stalls instead of
    // diverging.
    let mut failed = false;
    while let Ok(WriterJob::Flush(batch)) = jobs_rx.recv() {
        if failed {
            continue;
        }
        match apply_batch(&mut conn, &batch) {
            Ok(()) => {
                let _ = completions_tx.send(Completion::FlushDone(batch.id));
                wake();
            }
            Err(error) => {
                failed = true;
                let _ = completions_tx.send(Completion::StorageFailed(error));
                wake();
            }
        }
    }
}

/// Writes `batches` to `records.db` synchronously, on whichever thread
/// calls this — [`MessengerIo::stop`]'s own final flush, after the writer
/// thread has already been drained and joined (so this is never racing
/// that thread for the same file).
fn write_batches_sync(device_dir: &Path, key: &[u8; 32], batches: &[FlushBatch]) {
    if batches.is_empty() {
        return;
    }
    let mut conn = match boot_store(device_dir, key) {
        Ok((_, conn)) => conn,
        Err(error) => {
            tracing::warn!("messenger stop: cannot open records.db; {} final batch(es) not written: {error}", batches.len());
            return;
        }
    };
    for batch in batches {
        if let Err(error) = apply_batch(&mut conn, batch) {
            tracing::warn!("messenger stop: final write failed at batch {}: {error}", batch.id);
            return;
        }
    }
}

/// See the module doc.
pub struct MessengerIo {
    driver: MessengerDriver,
    /// Copy for the writer thread and for [`MessengerIo::stop`]'s final
    /// flush. The driver holds its own and zeroizes it on shutdown.
    vault_key: Zeroizing<[u8; 32]>,
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
            vault_key: Zeroizing::new(*vault_key),
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
        let key = self.vault_key.clone();
        let (jobs_tx, jobs_rx) = mpsc::channel::<WriterJob>();
        let thread_device_dir = device_dir.clone();
        // A spawn failure (OS resource exhaustion) degrades to "no
        // persistence this session" rather than panicking -- `writer`
        // stays `Some` so a flush send is a harmless no-op (the receiver
        // was dropped with the never-started closure) instead of a crash.
        self.writer_join = std::thread::Builder::new()
            .name("mlc-messenger-writer".to_string())
            .spawn(move || writer_thread_main(thread_device_dir, key, jobs_rx, completions_tx, wake))
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
            write_batches_sync(&device_dir, &self.vault_key, &batches);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mlc-messenger-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn key_byte(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    fn batch(id: u64, records: Vec<(&str, &[u8])>, deletes: &[&str]) -> FlushBatch {
        FlushBatch {
            id,
            records: records
                .into_iter()
                .map(|(key, bytes)| SealedRecord { key: RecordKey::new(key), bytes: bytes.to_vec() })
                .collect(),
            deletes: deletes.iter().copied().map(RecordKey::new).collect(),
        }
    }

    #[test]
    fn sealed_bytes_roundtrip_and_stay_off_the_raw_pages() {
        let dir = test_dir("round");
        let key = key_byte(0x11);
        let payload = b"sealed-not-plaintext";
        let mut conn = open_store(&dir.join(STORE_FILE), &key).unwrap();
        apply_batch(&mut conn, &batch(1, vec![("room/1", payload)], &[])).unwrap();
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
        drop(conn);
        let raw = fs::read(dir.join(STORE_FILE)).unwrap();
        assert!(!raw.windows(payload.len()).any(|window| window == payload));
        let conn = open_store(&dir.join(STORE_FILE), &key).unwrap();
        let loaded = load_records(&conn).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].key.as_str(), "room/1");
        assert_eq!(loaded[0].bytes, payload);
        drop(conn);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wrong_key_does_not_open_the_file() {
        let dir = test_dir("wrong");
        let path = dir.join(STORE_FILE);
        let mut conn = open_store(&path, &key_byte(1)).unwrap();
        apply_batch(&mut conn, &batch(1, vec![("k", b"abc")], &[])).unwrap();
        drop(conn);
        assert!(open_store(&path, &key_byte(2)).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_later_delete_removes_the_sealed_row() {
        let dir = test_dir("delete");
        let key = key_byte(4);
        let mut conn = open_store(&dir.join(STORE_FILE), &key).unwrap();
        apply_batch(&mut conn, &batch(1, vec![("keep", b"aaa"), ("drop", b"bbb")], &[])).unwrap();
        apply_batch(&mut conn, &batch(2, vec![], &["drop"])).unwrap();
        let loaded = load_records(&conn).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].key.as_str(), "keep");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_connection_without_a_key_still_reads_plaintext() {
        // SQLCipher with no key still opens a plaintext file. The messenger
        // file always sets a key; this checks the library did not make
        // every connection encrypted.
        let dir = test_dir("plain");
        let path = dir.join("plain.sqlite");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE t(v TEXT); INSERT INTO t(v) VALUES ('visible');").unwrap();
        drop(conn);
        let conn = rusqlite::Connection::open(&path).unwrap();
        let value: String = conn.query_row("SELECT v FROM t", [], |row| row.get(0)).unwrap();
        assert_eq!(value, "visible");
        drop(conn);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_log_imports_once_and_is_removed() {
        let dir = test_dir("legacy");
        let mut map = BTreeMap::new();
        map.insert(RecordKey::new("k"), b"sealed-legacy".to_vec());
        fs::write(dir.join("snapshot.bin"), persist_log::encode_snapshot(&map)).unwrap();
        let (records, conn) = boot_store(&dir, &key_byte(3)).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].bytes, b"sealed-legacy");
        assert!(!dir.join("snapshot.bin").exists());
        drop(conn);
        let (records, conn) = boot_store(&dir, &key_byte(3)).unwrap();
        drop(conn);
        assert_eq!(records[0].bytes, b"sealed-legacy");
        let _ = fs::remove_dir_all(&dir);
    }
}
