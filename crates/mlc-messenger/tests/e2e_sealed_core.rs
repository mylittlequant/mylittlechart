//! End-to-end test for M14: [`MessengerCore::open_sealed`] +
//! [`CoreSecrets::from_vault`] +
//! [`mlc_messenger::SealedRecordCodec`] wired together, driven entirely
//! through the kernel API -- a fresh device derives its secrets from a
//! vault key, persists sealed records, and a later "restart" continues from
//! exactly those records under the same secrets; a restart under a
//! DIFFERENT vault key must fail to open loudly, not silently start over
//! with a fresh identity.

use mlc_messenger::{CoreConfig, CoreSecrets, DeviceId, Jitter, MessengerCore, RecordKey, SealedRecord, UserId};
use mlc_vault::VaultKey;

struct FixedJitter(f64);

impl Jitter for FixedJitter {
    fn next_unit(&mut self) -> f64 {
        self.0
    }
}

fn core_config() -> CoreConfig {
    CoreConfig {
        user_id: UserId::parse("@alice:example.org").expect("valid user id"),
        device_id: DeviceId::parse("DEV1").expect("valid device id"),
        server_name: "example.org".to_string(),
    }
}

#[test]
fn e2e_sealed_core_restart_resumes_from_sealed_records() {
    let vault_key: VaultKey = [7u8; 32];
    let device_id = "DEV1";

    let mut core = MessengerCore::open_sealed(
        Vec::new(),
        CoreSecrets::from_vault(&vault_key, device_id),
        core_config(),
        0,
        Box::new(FixedJitter(0.0)),
    )
    .expect("open_sealed succeeds on a brand-new device");

    // A fresh device's own Olm account bootstrap dirties at least the
    // account record -- persist it (a real shell's own first flush) and
    // confirm nothing else is left pending.
    let first_batch = core
        .take_flush_batch()
        .expect("opening a fresh device dirties at least its own Olm account record");
    assert!(!first_batch.records.is_empty());
    core.ack_flush(first_batch.id);
    assert!(core.take_flush_batch().is_none(), "nothing else pending right after the initial bootstrap flush");

    let records: Vec<SealedRecord> = first_batch.records.clone();

    // Restart under the SAME vault key and device id: the persisted
    // account is decrypted and reused, never silently regenerated -- proven
    // by there being nothing left to flush immediately after reopening.
    let mut restarted = MessengerCore::open_sealed(
        records.clone(),
        CoreSecrets::from_vault(&vault_key, device_id),
        core_config(),
        0,
        Box::new(FixedJitter(0.0)),
    )
    .expect("reopen under the same vault key and device id succeeds");
    assert!(
        restarted.take_flush_batch().is_none(),
        "the persisted account was reused, not silently regenerated with a fresh identity"
    );

    // A DIFFERENT vault key derives a different `store_seal_key`
    // (`mlc_vault::messenger::MessengerSecrets::derive`'s own guarantee) --
    // every sealed record then fails its own AES-256-GCM authentication
    // check, and this must fail to open loudly rather than fabricate a
    // brand-new identity in its place.
    let other_vault_key: VaultKey = [9u8; 32];
    let err = MessengerCore::open_sealed(
        records,
        CoreSecrets::from_vault(&other_vault_key, device_id),
        core_config(),
        0,
        Box::new(FixedJitter(0.0)),
    )
    .err()
    .expect("a different vault key must fail to open, not silently start over");
    let message = err.to_string();
    assert!(
        message.contains("codec rejected"),
        "the error names the codec-open failure clearly, not a generic decode error: {message}"
    );
}

#[test]
fn e2e_sealed_core_open_sealed_without_store_seal_key_fails_clearly() {
    let err = MessengerCore::open_sealed(
        Vec::<SealedRecord>::new(),
        CoreSecrets::default(),
        core_config(),
        0,
        Box::new(FixedJitter(0.0)),
    )
    .err()
    .expect("open_sealed with no store_seal_key must not silently proceed unsealed");
    let message = err.to_string();
    assert!(
        message.contains("CoreSecrets::from_vault"),
        "the error tells the caller exactly which constructor to use instead: {message}"
    );
}

#[test]
fn e2e_sealed_core_records_are_illegible_without_the_codec() {
    // The sealed bytes a shell actually writes to disk must never contain
    // the plaintext account pickle -- this is the whole point of M14, and
    // the cheapest possible regression guard for it.
    let vault_key: VaultKey = [11u8; 32];
    let mut core = MessengerCore::open_sealed(
        Vec::new(),
        CoreSecrets::from_vault(&vault_key, "DEV1"),
        core_config(),
        0,
        Box::new(FixedJitter(0.0)),
    )
    .expect("open_sealed succeeds");

    let batch = core.take_flush_batch().expect("the fresh account bootstrap dirtied a record");
    let account_key = RecordKey::new("messenger/DEV1/account");
    let account_record = batch
        .records
        .iter()
        .find(|record| record.key == account_key)
        .expect("the fresh account was persisted under its own key");
    // The unsealed account record is plain JSON (`crypto::account`'s own
    // `serde_json::to_vec(&AccountRecord { .. })`) -- the sealed bytes a
    // shell actually writes must not parse as that JSON at all.
    assert!(
        serde_json::from_slice::<serde_json::Value>(&account_record.bytes).is_err(),
        "sealed bytes must not be readable as the plaintext account record"
    );
}
