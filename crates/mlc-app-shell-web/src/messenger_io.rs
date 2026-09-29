//! `MessengerIo` — the web shell's I/O for `mlc_messenger::driver::
//! MessengerDriver`: HTTP over `window.fetch`, durable records in IndexedDB,
//! and a Web Lock so exactly one tab per browser profile runs the core.
//!
//! `MessengerDriver` itself performs no I/O (see its own module doc) — it only
//! hands out `DriverRequest`s and `FlushBatch`es and expects raw results fed
//! back in. This module is the thin glue that does that I/O for the browser;
//! the native shell has its own twin over `reqwest` + a log file.
//!
//! # Single writer (Web Lock)
//!
//! [`MessengerIo::start`] asks `navigator.locks.request("mlc-messenger:" +
//! <store>, callback)` (through `js_sys::Reflect`: web-sys' `LockManager`
//! needs unstable cfgs). The callback returns a Promise this module keeps
//! pending; resolving it is how the lock is released, and the browser also
//! releases it if the tab dies. Until the callback runs the provider reports
//! `VaultState::ActiveElsewhere` and the driver is not ticked. A browser
//! without `navigator.locks` (or one that refuses the request) proceeds
//! without the lock after one console warning.
//!
//! # HTTP
//!
//! Each `DriverRequest` becomes one `fetch` on `wasm_bindgen_futures::
//! spawn_local`, with the bearer token, `cache: no-store`, and an
//! `AbortController` timeout (60 s for `/sync`, 30 s otherwise). Its result
//! lands in a shared completion queue and `wake()` is called; the next
//! [`MessengerIo::tick`] feeds it to the driver.
//!
//! # IndexedDB
//!
//! One database `<store>` with one object store `records`: out-of-line keys
//! (the record key string), values `Uint8Array`. There is no `persist_log`
//! here — an IndexedDB transaction is atomic, so each [`FlushBatch`] is exactly
//! one readwrite transaction (every put, every delete) created with
//! `durability: "strict"` so `oncomplete` means the bytes reached disk (the
//! flush-before-send barrier is what protects the ratchets).
//!
//! Batches are submitted ONE AT A TIME. IndexedDB runs overlapping readwrite
//! transactions in creation order, but an aborted transaction does not stop a
//! later one that was already created from committing — pipelining would let
//! batch N+1 land while batch N was lost, a gap the core cannot detect.
//! Serialising in Rust (the next batch is created only from the previous
//! one's `oncomplete`) makes the failure rule exact: the first
//! `onerror`/`onabort` calls `MessengerDriver::storage_failed`, drops every
//! queued batch, and nothing further is written or acked, so the messenger
//! stalls instead of sending ciphertext whose ratchet state never landed.
//!
//! # Boot, stop, sign-out
//!
//! Boot opens the database once the lock is held; after `whoami` resolves the
//! device the records in `[messenger/<device_id>/, messenger/<device_id>/
//! \u{10FFFF}]` are read (`getAllKeys` + `getAll`) and handed to
//! `MessengerDriver::records_loaded`. A database that cannot be opened or read
//! is a boot failure (`last_error`, vault stays locked) — never a silent
//! fresh start over history that failed to load.
//!
//! [`MessengerIo::stop`] queues the driver's leftover batches behind whatever
//! is already in flight and releases the lock (and closes the database) only
//! after the last `oncomplete`. [`MessengerIo::sign_out`] discards those
//! leftovers instead (they would be deleted anyway), lets the in-flight batch
//! finish, deletes the device key range in one transaction, then releases.
//! It can only delete records of a device it has already learned from
//! `whoami`; a tab that never got that far has nothing of its own to delete.
//!
//! # Wiring
//!
//! Not wired into `ChartApp` or the render loop here — the MLC coordinator does
//! that, choosing where a `MessengerIo` lives and how `wake` reaches
//! `requestAnimationFrame`.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    AbortController, IdbDatabase, IdbKeyRange, IdbObjectStore, IdbOpenDbRequest, IdbRequest, IdbTransaction,
    IdbTransactionMode, Request, RequestCache, RequestInit, RequestMode, Response, UrlSearchParams, Window,
};
use zeroize::Zeroizing;

use mlc_messenger::driver::{DriverRequest, DriverRequestId, MessengerDriver};
use mlc_messenger::persist::{FlushBatch, RecordKey, SealedRecord};
use mlc_messenger::provider::SnapshotProvider;
use mlc_messenger::wire::HttpMethod;
use mlc_messenger_model::VaultState;

/// `GET /_matrix/client/v3/sync`'s own long-poll path — the one request this
/// module gives a longer timeout (module doc).
const SYNC_PATH: &str = "/_matrix/client/v3/sync";
const SYNC_TIMEOUT_MS: i32 = 60_000;
const DEFAULT_TIMEOUT_MS: i32 = 30_000;

/// The one object store inside the `<store>` database.
const STORE_NAME: &str = "records";
const DB_VERSION: u32 = 1;
/// Web Lock names are `LOCK_PREFIX + <store>`.
const LOCK_PREFIX: &str = "mlc-messenger:";

/// How `MessengerIo`'s HTTP calls authenticate to the homeserver.
pub enum MessengerAuth {
    /// `Authorization: Bearer <token>` — the same token `mlc_cloud::
    /// FetchTransport` holds.
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
    /// The server origin; `""` in production (same origin). Every
    /// `DriverRequest::path` is appended to this verbatim.
    pub base_url: String,
    /// The IndexedDB database name (e.g. `"mlc-messenger"`); also names the
    /// Web Lock.
    pub store: String,
    /// "Tick me soon" — called whenever background work finished (in the shell:
    /// schedule a `requestAnimationFrame` that calls [`MessengerIo::tick`]).
    pub wake: Rc<dyn Fn()>,
}

/// One piece of background work that finished: the Web Lock being granted,
/// the database opening, an HTTP result, the boot record load, or a durable
/// write landing (or failing).
enum Completion {
    /// The Web Lock is held (or there is none to take).
    LockAcquired,
    /// The database is open (`Shared::db` is set) — only wakes the shell.
    DbReady,
    DbFailed(String),
    Http { id: DriverRequestId, result: Result<(u16, Vec<u8>), String> },
    RecordsLoaded(Result<Vec<SealedRecord>, String>),
    FlushDone(u64),
    /// A batch could not be written; nothing further is written or acked.
    StorageFailed(String),
}

/// Where the Web Lock request stands.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LockPhase {
    /// Requested, callback not run yet.
    Waiting,
    /// Held (or bypassed after the browser refused the request).
    Granted,
    /// `stop`/`sign_out` released it (or will the moment the callback runs).
    Closed,
}

/// What to do once the flush pipe has drained (set by `stop`/`sign_out`).
enum Finish {
    Release,
    WipeThenRelease(String),
}

/// The serial flush pipeline (module doc, "IndexedDB").
#[derive(Default)]
struct FlushPipe {
    queue: VecDeque<FlushBatch>,
    in_flight: bool,
    /// Set by the first failed batch: nothing further is written.
    failed: bool,
    finish: Option<Finish>,
}

/// State shared between `MessengerIo` and the futures it spawns.
struct Shared {
    wake: Rc<dyn Fn()>,
    completions: RefCell<VecDeque<Completion>>,
    /// Set once `stop`/`sign_out` consumed the `MessengerIo`: nobody reads
    /// completions any more, so they are dropped (and `wake` stays quiet).
    detached: Cell<bool>,
    db: RefCell<Option<IdbDatabase>>,
    pipe: RefCell<FlushPipe>,
    lock_phase: Cell<LockPhase>,
    /// Resolves the Promise the Web Lock callback returned.
    release_lock: RefCell<Option<Function>>,
    inflight: RefCell<HashMap<u64, AbortController>>,
    next_http: Cell<u64>,
}

impl Shared {
    fn new(wake: Rc<dyn Fn()>) -> Self {
        Self {
            wake,
            completions: RefCell::new(VecDeque::new()),
            detached: Cell::new(false),
            db: RefCell::new(None),
            pipe: RefCell::new(FlushPipe::default()),
            lock_phase: Cell::new(LockPhase::Waiting),
            release_lock: RefCell::new(None),
            inflight: RefCell::new(HashMap::new()),
            next_http: Cell::new(0),
        }
    }

    /// Queues `completion` for the next tick and asks the shell for one.
    fn push(&self, completion: Completion) {
        if self.detached.get() {
            return;
        }
        self.completions.borrow_mut().push_back(completion);
        (self.wake)();
    }

    fn abort_inflight(&self) {
        let controllers: Vec<AbortController> = self.inflight.borrow_mut().drain().map(|(_, controller)| controller).collect();
        for controller in controllers {
            controller.abort();
        }
    }
}

// ── JS plumbing ─────────────────────────────────────────────────────────────

/// Human-readable text for whatever a rejected Promise / thrown call carried:
/// an `Event` (its target's `error`, e.g. a `DOMException`), an `Error`, or a
/// bare string.
fn describe_failure(reason: &JsValue) -> String {
    if let Some(text) = reason.as_string() {
        return text;
    }
    let source = Reflect::get(reason, &JsValue::from_str("target"))
        .and_then(|target| Reflect::get(&target, &JsValue::from_str("error")))
        .ok()
        .filter(|error| !error.is_null() && !error.is_undefined())
        .unwrap_or_else(|| reason.clone());
    let field = |name: &str| Reflect::get(&source, &JsValue::from_str(name)).ok().and_then(|value| value.as_string()).filter(|text| !text.is_empty());
    match (field("name"), field("message")) {
        (Some(name), Some(message)) => format!("{name}: {message}"),
        (Some(name), None) => name,
        (None, Some(message)) => message,
        (None, None) => field("type").map(|kind| format!("{kind} event")).unwrap_or_else(|| format!("{reason:?}")),
    }
}

/// A Promise that resolves on the request's `success` and rejects on its
/// `error` — built from the Promise executor's own `resolve`/`reject`
/// functions, so no Rust closure is left behind whichever event fires.
fn request_promise(request: &IdbRequest) -> Promise {
    Promise::new(&mut |resolve: Function, reject: Function| {
        request.set_onsuccess(Some(&resolve));
        request.set_onerror(Some(&reject));
    })
}

/// A Promise that resolves on the transaction's `complete` and rejects on its
/// `error`/`abort`. Attach it only once every request has been issued: a
/// rejected Promise nobody awaits is reported as unhandled.
fn transaction_promise(transaction: &IdbTransaction) -> Promise {
    Promise::new(&mut |resolve: Function, reject: Function| {
        transaction.set_oncomplete(Some(&resolve));
        transaction.set_onerror(Some(&reject));
        transaction.set_onabort(Some(&reject));
    })
}

// ── IndexedDB ───────────────────────────────────────────────────────────────

/// Creates the `records` object store on first open (`upgradeneeded`).
fn create_schema(event: &web_sys::Event) -> Result<(), String> {
    let target = event.target().ok_or_else(|| "upgrade event has no target".to_string())?;
    let request: IdbOpenDbRequest = target.dyn_into().map_err(|_| "upgrade event target is not an open request".to_string())?;
    let db: IdbDatabase = request
        .result()
        .map_err(|error| describe_failure(&error))?
        .dyn_into()
        .map_err(|_| "open request result is not a database".to_string())?;
    if !db.object_store_names().contains(STORE_NAME) {
        db.create_object_store(STORE_NAME).map_err(|error| format!("create object store: {}", describe_failure(&error)))?;
    }
    Ok(())
}

/// Opens (creating on first use) the `name` database.
async fn open_database(name: &str) -> Result<IdbDatabase, String> {
    let window = web_sys::window().ok_or_else(|| "no window".to_string())?;
    let factory = window
        .indexed_db()
        .map_err(|error| describe_failure(&error))?
        .ok_or_else(|| "IndexedDB is unavailable".to_string())?;
    let request = factory.open_with_u32(name, DB_VERSION).map_err(|error| format!("open: {}", describe_failure(&error)))?;

    let upgrade_error: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let on_upgrade = {
        let upgrade_error = Rc::clone(&upgrade_error);
        Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            if let Err(reason) = create_schema(&event) {
                *upgrade_error.borrow_mut() = Some(reason);
            }
        })
    };
    request.set_onupgradeneeded(Some(on_upgrade.as_ref().unchecked_ref()));
    let outcome = JsFuture::from(request_promise(&request)).await;
    request.set_onupgradeneeded(None);
    outcome.map_err(|error| format!("open: {}", describe_failure(&error)))?;

    let db: IdbDatabase = request
        .result()
        .map_err(|error| describe_failure(&error))?
        .dyn_into()
        .map_err(|_| "open request result is not a database".to_string())?;
    let schema_error = upgrade_error.borrow_mut().take();
    if let Some(reason) = schema_error {
        db.close();
        return Err(reason);
    }
    Ok(db)
}

/// `[messenger/<device_id>/, messenger/<device_id>/\u{10FFFF}]` — every record
/// key this device owns (`mlc_messenger::store`'s layout).
fn device_key_range(device_id: &str) -> Result<IdbKeyRange, String> {
    let lower = format!("messenger/{device_id}/");
    let upper = format!("{lower}\u{10FFFF}");
    IdbKeyRange::bound(&JsValue::from_str(&lower), &JsValue::from_str(&upper)).map_err(|error| format!("key range: {}", describe_failure(&error)))
}

/// Reads every stored record of `device_id`. Any malformed row fails the
/// whole load: opening the core over a partial view would silently drop the
/// missing ratchets' history.
async fn load_records(db: &IdbDatabase, device_id: &str) -> Result<Vec<SealedRecord>, String> {
    let transaction = db
        .transaction_with_str_and_mode(STORE_NAME, IdbTransactionMode::Readonly)
        .map_err(|error| format!("read transaction: {}", describe_failure(&error)))?;
    let store = transaction.object_store(STORE_NAME).map_err(|error| describe_failure(&error))?;
    let range = device_key_range(device_id)?;
    let keys_request = store.get_all_keys_with_key(&range).map_err(|error| describe_failure(&error))?;
    let values_request = store.get_all_with_key(&range).map_err(|error| describe_failure(&error))?;

    let both = Promise::all(&Array::of2(&request_promise(&keys_request), &request_promise(&values_request)));
    JsFuture::from(both).await.map_err(|error| describe_failure(&error))?;

    let keys: Array = keys_request
        .result()
        .map_err(|error| describe_failure(&error))?
        .dyn_into()
        .map_err(|_| "getAllKeys did not return an array".to_string())?;
    let values: Array = values_request
        .result()
        .map_err(|error| describe_failure(&error))?
        .dyn_into()
        .map_err(|_| "getAll did not return an array".to_string())?;
    if keys.length() != values.length() {
        return Err(format!("record keys and values out of step ({} vs {})", keys.length(), values.length()));
    }
    let mut records = Vec::with_capacity(keys.length() as usize);
    for index in 0..keys.length() {
        let key = keys.get(index).as_string().ok_or_else(|| format!("record #{index} has a non-string key"))?;
        let bytes = values
            .get(index)
            .dyn_into::<Uint8Array>()
            .map_err(|_| format!("record {key} is not a Uint8Array"))?
            .to_vec();
        records.push(SealedRecord { key: RecordKey::new(key), bytes });
    }
    Ok(records)
}

/// A readwrite transaction on the records store with `durability: "strict"`
/// (`oncomplete` = on disk). web-sys gates `IdbTransactionOptions` behind
/// `web_sys_unstable_apis`, so the three-argument `transaction()` call goes
/// through `Reflect`. Engines that predate the option ignore it.
fn readwrite_transaction(db: &IdbDatabase) -> Result<IdbTransaction, String> {
    let db_value: &JsValue = db.as_ref();
    let transaction_fn: Function = Reflect::get(db_value, &JsValue::from_str("transaction"))
        .map_err(|error| describe_failure(&error))?
        .dyn_into()
        .map_err(|_| "IDBDatabase.transaction is not a function".to_string())?;
    let options = Object::new();
    Reflect::set(&options, &JsValue::from_str("durability"), &JsValue::from_str("strict")).map_err(|error| describe_failure(&error))?;
    let names = Array::of1(&JsValue::from_str(STORE_NAME));
    transaction_fn
        .call3(db_value, &names, &JsValue::from_str("readwrite"), &options)
        .map_err(|error| format!("write transaction: {}", describe_failure(&error)))?
        .dyn_into::<IdbTransaction>()
        .map_err(|_| "IDBDatabase.transaction did not return a transaction".to_string())
}

/// Issues every put and delete of `batch` on `store`.
fn queue_batch(store: &IdbObjectStore, batch: &FlushBatch) -> Result<(), String> {
    for record in &batch.records {
        let value = Uint8Array::from(record.bytes.as_slice());
        store
            .put_with_key(&value, &JsValue::from_str(record.key.as_str()))
            .map_err(|error| format!("put {}: {}", record.key, describe_failure(&error)))?;
    }
    for key in &batch.deletes {
        store.delete(&JsValue::from_str(key.as_str())).map_err(|error| format!("delete {key}: {}", describe_failure(&error)))?;
    }
    Ok(())
}

/// Creates ONE readwrite transaction holding all of `batch` and returns the
/// Promise of its outcome. Synchronous on purpose: every request must be
/// issued in the task that created the transaction, and creation order is
/// what orders the batches. A failure part-way aborts the transaction so half
/// a batch can never commit.
fn begin_flush_transaction(db: &IdbDatabase, batch: &FlushBatch) -> Result<Promise, String> {
    let transaction = readwrite_transaction(db)?;
    let store = transaction.object_store(STORE_NAME).map_err(|error| describe_failure(&error))?;
    if let Err(reason) = queue_batch(&store, batch) {
        let _ = transaction.abort();
        return Err(reason);
    }
    Ok(transaction_promise(&transaction))
}

/// Deletes every record of `device_id` in one transaction (sign-out).
async fn wipe_device(db: &IdbDatabase, device_id: &str) -> Result<(), String> {
    let transaction = readwrite_transaction(db)?;
    let store = transaction.object_store(STORE_NAME).map_err(|error| describe_failure(&error))?;
    let range = device_key_range(device_id)?;
    if let Err(error) = store.delete(&range) {
        let _ = transaction.abort();
        return Err(describe_failure(&error));
    }
    JsFuture::from(transaction_promise(&transaction)).await.map(|_| ()).map_err(|error| describe_failure(&error))
}

// ── Flush pipeline ──────────────────────────────────────────────────────────

/// Starts the next queued batch if none is in flight; runs the `finish`
/// action once the pipe is empty. Called after every enqueue and every
/// completion, so batches chain without waiting for a `tick`.
fn pump_flush(shared: &Rc<Shared>) {
    enum Next {
        Idle,
        Start(FlushBatch, IdbDatabase),
        Fail(String),
        Finish(Finish),
    }
    let next = {
        let mut pipe = shared.pipe.borrow_mut();
        if pipe.in_flight {
            Next::Idle
        } else {
            if pipe.failed {
                pipe.queue.clear();
            }
            match pipe.queue.pop_front() {
                Some(batch) => {
                    let db = shared.db.borrow().clone();
                    match db {
                        Some(db) => {
                            pipe.in_flight = true;
                            Next::Start(batch, db)
                        }
                        None => Next::Fail("IndexedDB is not open".to_string()),
                    }
                }
                None => pipe.finish.take().map_or(Next::Idle, Next::Finish),
            }
        }
    };
    match next {
        Next::Idle => {}
        Next::Fail(reason) => fail_flush(shared, reason),
        Next::Finish(finish) => run_finish(shared, finish),
        Next::Start(batch, db) => match begin_flush_transaction(&db, &batch) {
            Ok(done) => {
                let shared = Rc::clone(shared);
                let batch_id = batch.id;
                spawn_local(async move {
                    match JsFuture::from(done).await {
                        Ok(_) => {
                            shared.pipe.borrow_mut().in_flight = false;
                            shared.push(Completion::FlushDone(batch_id));
                            pump_flush(&shared);
                        }
                        Err(reason) => fail_flush(&shared, describe_failure(&reason)),
                    }
                });
            }
            Err(reason) => fail_flush(shared, reason),
        },
    }
}

/// The first failed batch: nothing further is written, queued batches are
/// dropped unwritten (a later batch landing without an earlier one is exactly
/// what must never happen), and the driver is told.
fn fail_flush(shared: &Rc<Shared>, reason: String) {
    {
        let mut pipe = shared.pipe.borrow_mut();
        pipe.failed = true;
        pipe.in_flight = false;
        pipe.queue.clear();
    }
    shared.push(Completion::StorageFailed(reason));
    pump_flush(shared);
}

/// Closes the database and releases the Web Lock.
fn close_and_release(shared: &Shared) {
    let db = shared.db.borrow_mut().take();
    if let Some(db) = db {
        db.close();
    }
    shared.lock_phase.set(LockPhase::Closed);
    let release = shared.release_lock.borrow_mut().take();
    if let Some(release) = release {
        let _ = release.call0(&JsValue::UNDEFINED);
    }
}

fn run_finish(shared: &Rc<Shared>, finish: Finish) {
    match finish {
        Finish::Release => close_and_release(shared),
        Finish::WipeThenRelease(device_id) => {
            let db = shared.db.borrow().clone();
            let shared = Rc::clone(shared);
            spawn_local(async move {
                if let Some(db) = db {
                    if let Err(reason) = wipe_device(&db, &device_id).await {
                        web_sys::console::warn_1(&JsValue::from_str(&format!("messenger sign-out: stored records were not deleted: {reason}")));
                    }
                }
                close_and_release(&shared);
            });
        }
    }
}

// ── Web Lock ────────────────────────────────────────────────────────────────

/// Requests the Web Lock `mlc-messenger:<store>`. `Ok` means the request is
/// queued and `Completion::LockAcquired` will follow; `Err` means the browser
/// has no usable Web Locks and the caller proceeds without one.
fn request_lock(shared: &Rc<Shared>, store: &str) -> Result<(), String> {
    let window = web_sys::window().ok_or_else(|| "no window".to_string())?;
    let navigator: JsValue = window.navigator().into();
    let locks = Reflect::get(&navigator, &JsValue::from_str("locks")).map_err(|error| describe_failure(&error))?;
    if locks.is_undefined() || locks.is_null() {
        return Err("navigator.locks is unavailable".to_string());
    }
    let request: Function = Reflect::get(&locks, &JsValue::from_str("request"))
        .map_err(|error| describe_failure(&error))?
        .dyn_into()
        .map_err(|_| "navigator.locks.request is not a function".to_string())?;

    let granted = Rc::clone(shared);
    let callback = Closure::once_into_js(move |_lock: JsValue| -> JsValue {
        if granted.lock_phase.get() == LockPhase::Closed {
            // Stopped while waiting: hand the lock straight back.
            return Promise::resolve(&JsValue::UNDEFINED).into();
        }
        granted.lock_phase.set(LockPhase::Granted);
        let held = Promise::new(&mut |resolve: Function, _reject: Function| {
            *granted.release_lock.borrow_mut() = Some(resolve);
        });
        granted.push(Completion::LockAcquired);
        held.into()
    });
    let returned: Promise = request
        .call2(&locks, &JsValue::from_str(&format!("{LOCK_PREFIX}{store}")), &callback)
        .map_err(|error| describe_failure(&error))?
        .dyn_into()
        .map_err(|_| "navigator.locks.request did not return a Promise".to_string())?;

    // The request Promise settles when the lock is released; a rejection
    // before the grant means the browser refused it outright (opaque origin,
    // ...), which is treated like a browser without Web Locks.
    let watched = Rc::clone(shared);
    spawn_local(async move {
        if let Err(reason) = JsFuture::from(returned).await {
            if watched.lock_phase.get() == LockPhase::Waiting {
                warn_no_lock(&describe_failure(&reason));
                watched.lock_phase.set(LockPhase::Granted);
                watched.push(Completion::LockAcquired);
            }
        }
    });
    Ok(())
}

fn warn_no_lock(reason: &str) {
    web_sys::console::warn_1(&JsValue::from_str(&format!(
        "messenger: running without a Web Lock ({reason}); two tabs on this profile could both write the messenger store"
    )));
}

// ── HTTP ────────────────────────────────────────────────────────────────────

/// Sends `request` and reads the whole body. Only a network/JS failure is an
/// `Err`; every HTTP status is an `Ok`.
async fn fetch_bytes(window: &Window, request: &Request) -> Result<(u16, Vec<u8>), String> {
    let response: Response = JsFuture::from(window.fetch_with_request(request))
        .await
        .map_err(|error| format!("fetch: {}", describe_failure(&error)))?
        .dyn_into()
        .map_err(|_| "fetch did not return a Response".to_string())?;
    let status = response.status();
    let body = response.array_buffer().map_err(|error| format!("body: {}", describe_failure(&error)))?;
    let buffer = JsFuture::from(body).await.map_err(|error| format!("read body: {}", describe_failure(&error)))?;
    Ok((status, Uint8Array::new(&buffer).to_vec()))
}

/// [`fetch_bytes`] under an `AbortController` timeout that also covers reading
/// the body.
async fn run_fetch(request: &Request, controller: &AbortController, timeout_ms: i32) -> Result<(u16, Vec<u8>), String> {
    let window = web_sys::window().ok_or_else(|| "no window".to_string())?;
    let to_abort = controller.clone();
    let on_timeout = Closure::once(move || to_abort.abort());
    let timer = window
        .set_timeout_with_callback_and_timeout_and_arguments_0(on_timeout.as_ref().unchecked_ref(), timeout_ms)
        .map_err(|error| format!("timer: {}", describe_failure(&error)))?;
    let result = fetch_bytes(&window, request).await;
    // `on_timeout` stays alive to here so the timer can never call a freed closure.
    window.clear_timeout_with_handle(timer);
    result
}

// ── MessengerIo ─────────────────────────────────────────────────────────────

/// See the module doc.
pub struct MessengerIo {
    driver: MessengerDriver,
    /// The `Authorization` header value, built once from [`MessengerAuth`].
    authorization: String,
    base_url: String,
    /// The IndexedDB database name.
    store: String,
    shared: Rc<Shared>,
    /// The Web Lock is held (or bypassed) and the database open is underway.
    active: bool,
    /// The boot record load has been started.
    load_started: bool,
    /// The device `whoami` resolved — what `sign_out` deletes.
    device_id: Option<String>,
    /// Storage could not be opened or read: terminal, the driver is no longer
    /// ticked (`last_error` says why).
    boot_failed: bool,
}

impl MessengerIo {
    /// Builds a driver for `vault_key` and returns immediately — nothing
    /// blocks. Until the Web Lock is granted the provider reports
    /// `VaultState::ActiveElsewhere` and [`MessengerIo::tick`] does not run
    /// the driver.
    pub fn start(vault_key: &mlc_vault::vault::VaultKey, auth: MessengerAuth, config: MessengerIoConfig) -> Self {
        let mut io = Self {
            driver: MessengerDriver::new(Zeroizing::new(*vault_key)),
            authorization: auth.header_value(),
            base_url: config.base_url,
            store: config.store,
            shared: Rc::new(Shared::new(config.wake)),
            active: false,
            load_started: false,
            device_id: None,
            boot_failed: false,
        };
        io.driver.provider().set_vault_state(VaultState::ActiveElsewhere);
        if let Err(reason) = request_lock(&io.shared, &io.store) {
            warn_no_lock(&reason);
            io.shared.lock_phase.set(LockPhase::Granted);
            io.activate();
        }
        io
    }

    /// A clone of this driver's own [`SnapshotProvider`] — the UI-facing half
    /// of the seam (`mlc_messenger::provider`'s own module doc).
    pub fn provider(&self) -> SnapshotProvider {
        self.driver.provider()
    }

    /// The lock is ours: leave `ActiveElsewhere` and open the database.
    fn activate(&mut self) {
        if self.active {
            return;
        }
        self.active = true;
        self.driver.provider().set_vault_state(VaultState::Locked);
        let shared = Rc::clone(&self.shared);
        let name = self.store.clone();
        spawn_local(async move {
            match open_database(&name).await {
                Ok(db) if shared.lock_phase.get() == LockPhase::Closed => db.close(),
                Ok(db) => {
                    *shared.db.borrow_mut() = Some(db);
                    shared.push(Completion::DbReady);
                }
                Err(reason) => shared.push(Completion::DbFailed(reason)),
            }
        });
    }

    fn fail_boot(&mut self, reason: String) {
        self.boot_failed = true;
        self.driver.provider().set_last_error(Some(reason));
    }

    /// Drains whatever finished in the background since the last call, feeds
    /// it to the driver, then advances the driver by one step — see
    /// `mlc_messenger::driver`'s own module doc. Never blocks; call it once
    /// per frame and again whenever `MessengerIoConfig::wake` fires. Does
    /// nothing to the driver while another tab holds the Web Lock.
    pub fn tick(&mut self, now_ms: i64) {
        loop {
            let next = self.shared.completions.borrow_mut().pop_front();
            let Some(completion) = next else { break };
            match completion {
                Completion::LockAcquired => self.activate(),
                Completion::DbReady => {}
                Completion::DbFailed(reason) => self.fail_boot(format!("messenger storage unavailable: {reason}")),
                Completion::Http { id, result } => match result {
                    Ok((status, body)) => self.driver.on_response(id, status, body, now_ms),
                    Err(_) => self.driver.on_transport_error(&id, now_ms),
                },
                Completion::RecordsLoaded(Ok(records)) => self.driver.records_loaded(records, now_ms),
                Completion::RecordsLoaded(Err(reason)) => self.fail_boot(format!("messenger storage read failed: {reason}")),
                Completion::FlushDone(id) => self.driver.flush_done(id),
                Completion::StorageFailed(reason) => self.driver.storage_failed(&reason),
            }
        }
        if !self.active || self.boot_failed {
            return;
        }

        self.start_record_load_if_due();
        let output = self.driver.tick(now_ms);
        for request in output.requests {
            self.spawn_http(request);
        }
        if !output.flush.is_empty() {
            self.shared.pipe.borrow_mut().queue.extend(output.flush);
            pump_flush(&self.shared);
        }
    }

    /// Once `whoami` has named the device and the database is open, reads the
    /// device's records and hands them back as `Completion::RecordsLoaded`.
    fn start_record_load_if_due(&mut self) {
        if self.load_started {
            return;
        }
        let device_id = self.driver.awaiting_records().map(|(_, device_id)| device_id.as_str().to_string());
        let Some(device_id) = device_id else { return };
        self.device_id = Some(device_id.clone());
        let db = self.shared.db.borrow().clone();
        let Some(db) = db else { return };
        self.load_started = true;
        let shared = Rc::clone(&self.shared);
        spawn_local(async move {
            let result = load_records(&db, &device_id).await;
            shared.push(Completion::RecordsLoaded(result));
        });
    }

    /// Builds the `fetch` [`Request`] for `request` plus the controller that
    /// aborts it.
    fn prepare_fetch(&self, request: &DriverRequest) -> Result<(Request, AbortController), String> {
        let mut url = format!("{}{}", self.base_url.trim_end_matches('/'), request.path);
        if !request.query.is_empty() {
            let params = UrlSearchParams::new().map_err(|error| describe_failure(&error))?;
            for (name, value) in &request.query {
                params.append(name, value);
            }
            url.push('?');
            url.push_str(&String::from(params.to_string()));
        }
        let body = request.body.as_ref().map(serde_json::to_string).transpose().map_err(|error| format!("encode body: {error}"))?;

        let controller = AbortController::new().map_err(|error| describe_failure(&error))?;
        let options = RequestInit::new();
        options.set_method(match request.method {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
        });
        options.set_mode(RequestMode::Cors);
        options.set_cache(RequestCache::NoStore);
        options.set_signal(Some(&controller.signal()));
        if let Some(body) = &body {
            options.set_body(&JsValue::from_str(body));
        }
        let built = Request::new_with_str_and_init(&url, &options).map_err(|error| format!("build request: {}", describe_failure(&error)))?;
        let headers = built.headers();
        headers.set("Authorization", &self.authorization).map_err(|error| format!("set header: {}", describe_failure(&error)))?;
        if body.is_some() {
            headers.set("Content-Type", "application/json").map_err(|error| format!("set header: {}", describe_failure(&error)))?;
        }
        Ok((built, controller))
    }

    fn spawn_http(&self, request: DriverRequest) {
        let (built, controller) = match self.prepare_fetch(&request) {
            Ok(prepared) => prepared,
            Err(reason) => return self.shared.push(Completion::Http { id: request.id, result: Err(reason) }),
        };
        let timeout_ms = if request.path == SYNC_PATH { SYNC_TIMEOUT_MS } else { DEFAULT_TIMEOUT_MS };
        let key = self.shared.next_http.get();
        self.shared.next_http.set(key.wrapping_add(1));
        self.shared.inflight.borrow_mut().insert(key, controller.clone());

        let shared = Rc::clone(&self.shared);
        let id = request.id;
        spawn_local(async move {
            let result = run_fetch(&built, &controller, timeout_ms).await;
            shared.inflight.borrow_mut().remove(&key);
            shared.push(Completion::Http { id, result });
        });
    }

    /// Ends this session: the driver's leftover batches are queued behind
    /// whatever is already being written, and the Web Lock is released (and the
    /// database closed) only after the last of them completed — a
    /// failed store gets no further write at all. Returns immediately; the
    /// writes finish asynchronously. Keys are zeroized on drop
    /// (`MessengerDriver::shutdown`'s own doc).
    pub fn stop(self) {
        self.end(false);
    }

    /// [`MessengerIo::stop`], then deletes every record this device stored (in
    /// one transaction) — a real account sign-out, not just closing the tab.
    /// The leftover batches are not written first: they would be deleted
    /// straight away.
    pub fn sign_out(self) {
        self.end(true);
    }

    fn end(self, wipe: bool) {
        let Self { driver, shared, device_id, active, .. } = self;
        let leftovers = driver.shutdown();
        shared.detached.set(true);
        shared.abort_inflight();
        if !active {
            // Lock still pending (its callback will hand it straight back) or
            // granted before the first tick saw it: no database to flush.
            close_and_release(&shared);
            return;
        }
        {
            let mut pipe = shared.pipe.borrow_mut();
            match (wipe, device_id) {
                (true, Some(device_id)) => {
                    pipe.queue.clear();
                    pipe.finish = Some(Finish::WipeThenRelease(device_id));
                }
                (true, None) => {
                    pipe.queue.clear();
                    pipe.finish = Some(Finish::Release);
                }
                (false, _) => {
                    pipe.queue.extend(leftovers);
                    pipe.finish = Some(Finish::Release);
                }
            }
        }
        pump_flush(&shared);
    }
}
