//! End-to-end test for `mlc_messenger::driver::MessengerDriver` against the
//! shared fake homeserver: two independent drivers over the same fake
//! server -- a brand-new device's own full boot (`whoami` -> load records
//! -> running), then a SECOND driver restarted from exactly the records a
//! real shell's own `records.log` would hold after replaying the frames
//! [`persist_log::encode_batch`] produced -- proving the restart resumes
//! the same identity rather than silently starting over with a fresh one
//! (`driver`'s own module doc: never open an empty store over records that
//! merely failed to decrypt, and symmetrically here, never re-derive a
//! fresh identity when the real one replayed cleanly).

// See `e2e_olm_1to1.rs`'s own `mod support;` for why this allow is here --
// `tests/support` is shared across every integration-test binary in this
// crate, each compiled independently, so a helper this file needs but a
// sibling file's own tests don't (or vice versa) is `dead_code`'s known
// per-binary false positive, not actually unused code.
#[allow(dead_code)]
mod support;

use mlc_messenger::driver::{persist_log, DriverRequestId, MessengerDriver};
use mlc_messenger::persist::SealedRecord;
use mlc_messenger::wire::OutgoingRequest;
use mlc_messenger::{DeviceId, UserId};
use mlc_vault::VaultKey;
use support::fake_server::FakeServer;
use zeroize::Zeroizing;

fn alice() -> UserId {
    UserId::parse("@alice:example.org").expect("valid user id")
}

fn alice_device() -> DeviceId {
    DeviceId::parse("DEV1").expect("valid device id")
}

/// Drives `driver` for `ticks` steps against `server`: every emitted
/// `DriverRequest` is dispatched (the boot-time `whoami` probe -- carrying
/// no `OutgoingRequestKind` of its own -- via [`FakeServer::whoami`],
/// everything else via [`FakeServer::dispatch`]), and every produced
/// `FlushBatch` is encoded via [`persist_log::encode_batch`], appended to
/// `log`, and acked in the same step -- exactly the loop a real native
/// shell's own `MessengerIo` runs.
fn pump(driver: &mut MessengerDriver, server: &mut FakeServer, ticks: u32, log: &mut Vec<u8>) {
    for tick in 0..ticks {
        let now_ms = i64::from(tick) * 1000;
        let output = driver.tick(now_ms);
        for request in output.requests {
            let response = match &request.kind {
                None => server.whoami(&alice(), &alice_device()),
                Some(kind) => {
                    let DriverRequestId::Core(core_id) = &request.id else {
                        panic!("a request carrying an OutgoingRequestKind must be a Core request")
                    };
                    let outgoing = OutgoingRequest {
                        id: core_id.clone(),
                        method: request.method,
                        path: request.path.clone(),
                        query: request.query.clone(),
                        body: request.body.clone(),
                        kind: *kind,
                    };
                    server.dispatch(&alice(), &alice_device(), &outgoing)
                }
            };
            driver.on_response(request.id, response.status, response.body, now_ms);
        }
        for batch in output.flush {
            log.extend_from_slice(&persist_log::encode_batch(&batch));
            driver.flush_done(batch.id);
        }
    }
}

/// Boots `driver` through `whoami` (as `alice`/`DEV1`, against `server`)
/// and hands back `records` as this device's own stored records --
/// `Vec::new()` for a brand-new device, or [`persist_log::replay`]'s own
/// output turned back into `SealedRecord`s for a restart.
fn boot(driver: &mut MessengerDriver, server: &mut FakeServer, records: Vec<SealedRecord>) {
    let output = driver.tick(0);
    assert_eq!(output.requests.len(), 1, "boot emits exactly one whoami probe");
    let response = server.whoami(&alice(), &alice_device());
    driver.on_response(DriverRequestId::Whoami, response.status, response.body, 0);

    let (user_id, device_id) = driver.awaiting_records().expect("whoami resolved a device");
    assert_eq!(user_id, &alice());
    assert_eq!(device_id, &alice_device());

    driver.records_loaded(records, 0);
}

#[test]
fn e2e_driver_restart_resumes_from_its_own_replayed_log() {
    let mut server = FakeServer::new();
    let vault_key: VaultKey = [7u8; 32];

    // Driver A: a brand-new device, driven for a few ticks so its own
    // bootstrap (Olm account creation, `/keys/upload`, the first `/sync`)
    // produces more than the single initial flush batch.
    let mut driver_a = MessengerDriver::new(Zeroizing::new(vault_key));
    boot(&mut driver_a, &mut server, Vec::new());

    let mut log = Vec::new();
    pump(&mut driver_a, &mut server, 6, &mut log);

    // Everything still unflushed at "shutdown" rides in too -- exactly what
    // a real shell's own final synchronous write covers.
    for batch in driver_a.shutdown() {
        log.extend_from_slice(&persist_log::encode_batch(&batch));
    }
    assert!(!log.is_empty(), "a real boot sequence dirties at least the fresh Olm account record");

    // Replay the log exactly like a real shell's own boot would, and hand
    // the result to a SECOND driver -- "restart one from its replayed log;
    // the history is still there".
    let (replayed, valid_len) = persist_log::replay(&log);
    assert_eq!(valid_len, log.len(), "every frame this test wrote is well-formed");
    let records: Vec<SealedRecord> = replayed.into_iter().map(|(key, bytes)| SealedRecord { key, bytes }).collect();
    assert!(!records.is_empty(), "the replayed log carries at least the persisted Olm account");

    let mut driver_b = MessengerDriver::new(Zeroizing::new(vault_key));
    boot(&mut driver_b, &mut server, records);

    // The restarted driver reused the persisted identity rather than
    // silently regenerating a fresh one -- proven exactly like
    // `e2e_sealed_core.rs`'s own restart test: nothing is left to flush
    // immediately after opening.
    let batches_after_restart = driver_b.shutdown();
    assert!(batches_after_restart.is_empty(), "the persisted account was reused, not silently regenerated with a fresh identity");
}
