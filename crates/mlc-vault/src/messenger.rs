//! Messenger-specific secrets derived from the vault key: the AES-256-GCM
//! key sealing every record `mlc-messenger`'s own store persists
//! ([`MessengerSecrets::store_seal_key`]), and the key backing the
//! account's server-side Megolm key backup ([`MessengerSecrets::backup_key`],
//! M9 — not yet consumed by anything landed in this workspace).
//!
//! Plan: `docs/mlc/plans/messenger-proto-client-crates-2026-09-24.md` §3.3 +
//! §3.5, Manager decision #3 (M14).
//!
//! # Why the Olm account itself is NOT derived from the vault key
//!
//! Every OTHER identity this crate derives ([`crate::dm::DmIdentity`],
//! [`crate::channel::ChannelSigningIdentity`]) is deterministic: the same
//! vault key always yields the same keypair, on any device, with nothing to
//! generate or persist by hand. `mlc-messenger`'s own Olm account
//! (`crypto::account::OlmAccountState::load_or_create`) is the opposite by
//! design — generated ONCE from OS entropy on whichever device first needs
//! it, then persisted (sealed under [`MessengerSecrets::store_seal_key`]
//! below) rather than re-derived on every open. Two reasons this can never
//! be deterministic the way a DM/channel identity is: (1) the Olm/Megolm
//! protocol's own security model assumes each DEVICE carries its own
//! independent identity keypair — a user's two devices sharing one derived
//! keypair would make them cryptographically indistinguishable to every
//! peer they talk to, breaking per-device session establishment and
//! revocation entirely; (2) the underlying Olm account primitive exposes no
//! hook for injecting deterministic entropy in the first place — it always
//! draws from the OS. This module's two derived secrets exist BESIDE that
//! account, never instead of it: [`MessengerSecrets::store_seal_key`] is
//! what lets the account's own pickle (and every other record) be sealed at
//! rest at all; [`MessengerSecrets::backup_key`] is a separate,
//! deliberately account-wide (not per-device) secret for recovering Megolm
//! session history across devices — orthogonal to the account identity
//! itself.
//!
//! # Key flow
//!
//! [`MessengerSecrets::derive`] takes the profile's already-unlocked vault
//! key plus this device's own record-store-scoping device id (matching
//! `mlc-messenger`'s own `messenger/{device_id}/...` record-key layout) and
//! produces two independently domain-separated 32-byte keys via
//! HKDF-SHA256, no salt (the vault key is already uniformly random — same
//! reasoning [`crate::dm::DmIdentity::from_vault_key`]/
//! [`crate::channel::ChannelSigningIdentity::from_vault_key`] document):
//! `store_seal_key = HKDF(info = HKDF_MESSENGER_STORE_INFO || device_id)`
//! — per device, because `mlc-messenger`'s own record store is per device
//! too, and two devices of the same account must not be able to open each
//! other's sealed records even though both derive from the same vault key;
//! `backup_key = HKDF(info = HKDF_MESSENGER_BACKUP_INFO)` — the SAME on
//! every device of the account, no id folded in, because the whole point of
//! a key backup is that any device recovers it from the vault key alone.

use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::crypto;
use crate::vault::VaultKey;

/// Vault-derived secrets `mlc-messenger` needs at store-open time — see the
/// module doc for the two derivations and why the account identity itself
/// is never one of them. Does not implement `Debug`/`Clone`/`Serialize` —
/// same reasoning as [`crate::dm::DmIdentity`]: this key material is never
/// persisted, logged, or serialized, only held for as long as the messenger
/// core needs it.
pub struct MessengerSecrets {
    /// The AES-256-GCM key sealing every record this device's own
    /// `mlc-messenger` store persists. Per device — see the module doc.
    pub store_seal_key: Zeroizing<[u8; 32]>,
    /// The key backing the account's server-side Megolm key backup (M9,
    /// not yet consumed by anything landed in this workspace). Deliberately
    /// the SAME on every device of the account — see the module doc.
    pub backup_key: Zeroizing<[u8; 32]>,
}

impl MessengerSecrets {
    /// Derives both secrets from `vault_key` and this device's own
    /// `device_id` — see the module doc for the exact HKDF derivations.
    pub fn derive(vault_key: &VaultKey, device_id: &str) -> Self {
        Self {
            store_seal_key: derive_store_seal_key(vault_key, device_id),
            backup_key: derive_backup_key(vault_key),
        }
    }
}

fn derive_store_seal_key(vault_key: &VaultKey, device_id: &str) -> Zeroizing<[u8; 32]> {
    let mut info = Vec::with_capacity(crypto::HKDF_MESSENGER_STORE_INFO.len() + device_id.len());
    info.extend_from_slice(crypto::HKDF_MESSENGER_STORE_INFO);
    info.extend_from_slice(device_id.as_bytes());
    hkdf_expand(vault_key, &info)
}

fn derive_backup_key(vault_key: &VaultKey) -> Zeroizing<[u8; 32]> {
    hkdf_expand(vault_key, crypto::HKDF_MESSENGER_BACKUP_INFO)
}

fn hkdf_expand(vault_key: &VaultKey, info: &[u8]) -> Zeroizing<[u8; 32]> {
    let hk = Hkdf::<Sha256>::new(None, vault_key);
    let mut key = [0u8; 32];
    hk.expand(info, &mut key).expect("HKDF expand should never fail for 32-byte output");
    Zeroizing::new(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dm::DmIdentity;

    #[test]
    fn store_seal_key_differs_per_device_backup_key_same_per_user() {
        let vault_key: VaultKey = [3u8; 32];
        let a = MessengerSecrets::derive(&vault_key, "DEV1");
        let b = MessengerSecrets::derive(&vault_key, "DEV2");
        assert_ne!(*a.store_seal_key, *b.store_seal_key, "each device seals its own store under its own key");
        assert_eq!(*a.backup_key, *b.backup_key, "the backup key is the same across every device of the account");
    }

    #[test]
    fn derive_is_deterministic() {
        let vault_key: VaultKey = [4u8; 32];
        let a = MessengerSecrets::derive(&vault_key, "DEV1");
        let b = MessengerSecrets::derive(&vault_key, "DEV1");
        assert_eq!(*a.store_seal_key, *b.store_seal_key);
        assert_eq!(*a.backup_key, *b.backup_key);
    }

    #[test]
    fn different_vault_keys_diverge() {
        let a = MessengerSecrets::derive(&[5u8; 32], "DEV1");
        let b = MessengerSecrets::derive(&[6u8; 32], "DEV1");
        assert_ne!(*a.store_seal_key, *b.store_seal_key);
        assert_ne!(*a.backup_key, *b.backup_key);
    }

    #[test]
    fn derived_keys_differ_from_dm_identity_material() {
        let vault_key: VaultKey = [8u8; 32];
        let secrets = MessengerSecrets::derive(&vault_key, "DEV1");
        let dm_identity = DmIdentity::from_vault_key(&vault_key);
        assert_ne!(
            secrets.store_seal_key.as_slice(),
            dm_identity.secret.as_slice(),
            "store seal key must not collide with the DM identity secret scalar"
        );
        assert_ne!(
            secrets.backup_key.as_slice(),
            dm_identity.secret.as_slice(),
            "backup key must not collide with the DM identity secret scalar"
        );
    }

    #[test]
    fn hkdf_info_strings_are_distinct() {
        let infos: Vec<&[u8]> = vec![
            crypto::HKDF_VAULT_INFO,
            crypto::HKDF_SYNC_INFO,
            crypto::HKDF_RECOVERY_INFO,
            crypto::HKDF_DM_INFO,
            crypto::HKDF_DM_IDENTITY_INFO,
            crypto::HKDF_GROUP_WRAP_INFO,
            crypto::HKDF_CHANNEL_SIGNING_INFO,
            crypto::HKDF_MESSENGER_STORE_INFO,
            crypto::HKDF_MESSENGER_BACKUP_INFO,
        ];
        for i in 0..infos.len() {
            for j in (i + 1)..infos.len() {
                assert_ne!(
                    infos[i], infos[j],
                    "HKDF info strings at index {i} and {j} collide: {:?} vs {:?}",
                    infos[i], infos[j]
                );
            }
        }
    }
}
