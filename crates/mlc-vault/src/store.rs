//! `SecretStore` — the RAM-only, TTL-bounded home for decrypted vault
//! secrets.
//!
//! `unlock()` derives the vault key, decrypts `vault.enc` once, and holds the
//! plaintext bytes in memory until `seal()` or the TTL lapses. This crate
//! must not depend on `mlc-core` (dependency direction is one-way upward:
//! types → engines → backend → app → shells, and this is the leaf), so the
//! store cannot know `VaultSecrets`' shape. It holds the decrypted JSON as
//! opaque bytes and hands a copy to the caller on [`SecretStore::get`], which
//! deserializes into whatever type it holds — a `SecretStore<T>` generic over
//! the payload would instead force every future payload type (and every type
//! nested inside it) to implement `Zeroize` just to satisfy this crate's
//! wipe guarantee, which is a much larger and more fragile surface than one
//! `serde_json::from_slice` call at the point of use.
//!
//! # A sealed store is a state, not an error
//!
//! [`SecretStore::get`] returns `Result<_, Sealed>`. `Sealed` carries no
//! detail beyond "there is nothing to read right now" — no ordinary code
//! path asks for a secret, so nothing prompts on its own; a caller that
//! wants to offer an unlock UI matches on `Sealed` and does so explicitly.
//!
//! # TTL: idle-based, with a hard cap
//!
//! [`IDLE_TTL`] resets on every successful [`SecretStore::get`] so an active
//! session does not die mid-flight. [`ABSOLUTE_TTL`] does not reset, so an
//! unattended machine does not stay unlocked all night even under constant
//! access. The store expires on whichever deadline is closer.
//!
//! [`SecretStore::get`] checks both deadlines on every call — expiry is
//! therefore enforced by access, not by a scheduler. [`SecretStore::get`]
//! and [`SecretStore::remaining`] both also wipe the held state the instant
//! either of them observes it past the deadline, so a background timer is
//! an optional eager-wipe optimisation, never a correctness dependency: if
//! nothing ever calls in again after expiry, the payload is unreachable
//! (sealed) but not physically wiped until the next call — a background
//! timer is what would make that wipe eager instead of lazy, and this crate
//! does not start one (this is a sync, leaf crate with no executor to spawn
//! it on).
//!
//! # What `zeroize` covers, and what it does not
//!
//! Covered: the decrypted secrets payload (wiped on `seal()`, on expiry, and
//! when a caller drops the `Zeroizing<Vec<u8>>` returned by `get()`).
//!
//! Not covered, and not coverable from safe Rust: the vault AES key itself
//! is derived fresh inside `unlock()` for the one `decrypt()` call it is
//! needed for, wrapped in `Zeroizing` for that scope only, and is never
//! stored in `Unlocked` — so it is wiped before `unlock()` returns rather
//! than lingering for the lifetime of the session. Nor covered: any
//! intermediate copies the `aes-gcm`/`pbkdf2`/`hkdf` crates make internally
//! while deriving or decrypting (outside this crate's control), or any
//! stack copy of a `Copy` value (e.g. the `[u8; 32]` key array) made before
//! it is wrapped in `Zeroizing` — Rust moves of `Copy` types do not erase
//! the source location, only the final `Zeroizing`-owned copy's own drop
//! does.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use web_time::Instant;
use zeroize::Zeroizing;

use crate::vault::{self, VaultError};

/// Idle timeout — the store wipes if no [`SecretStore::get`] call lands
/// within this window of the last one. Resets on every successful `get()`.
pub const IDLE_TTL: Duration = Duration::from_secs(15 * 60);

/// Absolute timeout — the store wipes this long after `unlock()` regardless
/// of activity. Does not reset.
pub const ABSOLUTE_TTL: Duration = Duration::from_secs(8 * 60 * 60);

/// Returned by [`SecretStore::get`] when the vault is locked or has expired.
///
/// A state, not an error condition to propagate and log — see the module
/// doc. Implements [`std::error::Error`] purely so it composes with `?` in
/// contexts that expect one; treat it as "offer an unlock", not "something
/// went wrong".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sealed;

impl std::fmt::Display for Sealed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "vault is sealed")
    }
}

impl std::error::Error for Sealed {}

/// The RAM-resident state while unlocked. Every field that carries secret
/// material is `Zeroizing` so it self-wipes on drop — dropping `Unlocked`
/// (via `seal()`, expiry, or replacement in `unlock()`) is the wipe.
struct Unlocked {
    /// Decrypted `vault.enc` plaintext (JSON bytes) — the caller's
    /// `serde_json::from_slice` target.
    payload: Zeroizing<Vec<u8>>,
    /// Stamped at `unlock()`; anchors [`ABSOLUTE_TTL`].
    unlocked_at: Instant,
    /// Stamped at `unlock()`, refreshed on every `get()`; anchors
    /// [`IDLE_TTL`].
    last_access: Instant,
}

impl Unlocked {
    /// Past either deadline.
    fn expired(&self, now: Instant) -> bool {
        // `checked_duration_since` over `duration_since`: never panics
        // regardless of clock-source quirks on a given platform, and a
        // `None` (now earlier than the stamp) is honestly "no time has
        // passed yet", not an error — `unwrap_or_default()` here is exactly
        // that reading, not a suppressed failure.
        let idle_elapsed = now.checked_duration_since(self.last_access).unwrap_or_default();
        let total_elapsed = now.checked_duration_since(self.unlocked_at).unwrap_or_default();
        idle_elapsed >= IDLE_TTL || total_elapsed >= ABSOLUTE_TTL
    }
}

/// RAM-only, TTL-bounded holder for the decrypted vault secrets.
///
/// `Arc`-shared, `Send + Sync` (via `Mutex`) — one instance per profile,
/// handed out as `Arc<SecretStore>` to whatever eventually needs it (a
/// trading engine, an alert-delivery engine, …) without those crates ever
/// depending on `mlc-core` or on each other.
///
/// Construction takes the two paths `mlc-vault`'s existing `vault::`
/// functions already expect (`salt.hex`, `vault.enc`) — this crate does not
/// know about profile directory layout, the caller (today: `mlc-core` /
/// `ProfileManager`) does.
pub struct SecretStore {
    salt_path: PathBuf,
    vault_path: PathBuf,
    state: Mutex<Option<Unlocked>>,
}

impl SecretStore {
    /// Build a store bound to a given profile's `salt.hex` / `vault.enc`
    /// paths. Starts sealed — nothing is read from disk until `unlock()`.
    pub fn new(salt_path: PathBuf, vault_path: PathBuf) -> Self {
        Self {
            salt_path,
            vault_path,
            state: Mutex::new(None),
        }
    }

    /// Derive the vault key from `passphrase`, decrypt `vault.enc`, populate
    /// RAM, and stamp both deadlines from now.
    ///
    /// A missing `vault.enc` (a fresh profile that has never saved a secret)
    /// is not a failed unlock — it succeeds with an empty JSON object as the
    /// payload, which `#[serde(default)]`-shaped payload types (e.g.
    /// `VaultSecrets` in `mlc-core`) deserialize to their default value.
    /// Any other failure (wrong passphrase, corrupted blob, I/O error) is
    /// returned and the store is left exactly as it was — an unlock attempt
    /// never seals an already-unlocked store.
    pub fn unlock(&self, passphrase: &str) -> Result<(), VaultError> {
        let salt = vault::load_or_create_salt(&self.salt_path)?;

        let payload = {
            // The vault AES key lives only for this block — one decrypt
            // call — and is never stored on `Unlocked`. See the module doc
            // "what zeroize does not cover".
            let vault_key = Zeroizing::new(vault::derive_key(passphrase, &salt));
            match vault::load_encrypted_raw(&vault_key, &self.vault_path) {
                Ok(bytes) => bytes,
                Err(VaultError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                    b"{}".to_vec()
                }
                Err(e) => return Err(e),
            }
        };

        let now = Instant::now();
        let mut guard = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some(Unlocked {
            payload: Zeroizing::new(payload),
            unlocked_at: now,
            last_access: now,
        });
        Ok(())
    }

    /// Wipe now. Idempotent — sealing an already-sealed store is a no-op.
    pub fn seal(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = None; // Unlocked::drop → Zeroizing::drop wipes `payload`.
    }

    /// Return a copy of the decrypted payload if the store is unlocked and
    /// neither deadline has passed; otherwise [`Sealed`].
    ///
    /// Checks the deadline on every call — this is the enforcement point;
    /// a background timer, if one is ever added by an owning shell, is
    /// strictly an eager-wipe optimisation on top of this, never a
    /// substitute for it. A successful call refreshes [`IDLE_TTL`].
    ///
    /// The returned bytes are `Zeroizing` so the caller's copy also wipes
    /// on drop — decode it (`serde_json::from_slice::<VaultSecrets>(&bytes)`
    /// in `mlc-core`) and let it go out of scope rather than holding it.
    pub fn get(&self) -> Result<Zeroizing<Vec<u8>>, Sealed> {
        let mut guard = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        match guard.as_mut() {
            Some(u) if !u.expired(now) => {
                u.last_access = now;
                Ok(Zeroizing::new(u.payload.to_vec()))
            }
            _ => {
                // Either never unlocked (no-op) or past the deadline
                // (eager wipe right here, rather than waiting for the next
                // touch).
                *guard = None;
                Err(Sealed)
            }
        }
    }

    /// Time left before the store seals itself, or `None` if it already has
    /// (or was never unlocked). Read-only in intent — unlike `get()`, this
    /// does not refresh [`IDLE_TTL`]; a UI polling it to show a countdown
    /// must not be able to keep the vault open indefinitely just by asking.
    pub fn remaining(&self) -> Option<Duration> {
        let mut guard = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        match guard.as_ref() {
            Some(u) if !u.expired(now) => {
                let idle_left = IDLE_TTL.saturating_sub(now.checked_duration_since(u.last_access).unwrap_or_default());
                let total_left = ABSOLUTE_TTL.saturating_sub(now.checked_duration_since(u.unlocked_at).unwrap_or_default());
                Some(idle_left.min(total_left))
            }
            _ => {
                *guard = None; // same eager-wipe-on-touch as get()
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_at(dir: &std::path::Path) -> SecretStore {
        SecretStore::new(dir.join("salt.hex"), dir.join("vault.enc"))
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| {
            panic!("test setup: create_dir_all({dir:?}) failed: {e}")
        });
        dir
    }

    #[test]
    fn sealed_before_unlock() {
        let dir = temp_dir("secret_store_sealed_before_unlock");
        let store = store_at(&dir);
        assert_eq!(store.get(), Err(Sealed));
        assert_eq!(store.remaining(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unlock_with_no_vault_file_yields_empty_object() {
        let dir = temp_dir("secret_store_fresh_profile");
        let store = store_at(&dir);
        store.unlock("passphrase").expect("fresh profile unlock should succeed");
        let bytes = store.get().expect("should be unlocked");
        assert_eq!(&*bytes, b"{}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unlock_decrypts_saved_vault() {
        let dir = temp_dir("secret_store_roundtrip");
        let salt_path = dir.join("salt.hex");
        let vault_path = dir.join("vault.enc");

        let salt = vault::load_or_create_salt(&salt_path).unwrap();
        let key = vault::derive_key("correct horse", &salt);
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct Payload {
            token: String,
        }
        vault::save_encrypted(&key, &vault_path, &Payload { token: "shh".into() }).unwrap();

        let store = store_at(&dir);
        store.unlock("correct horse").expect("unlock with the right passphrase");
        let bytes = store.get().expect("should be unlocked");
        let decoded: Payload = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, Payload { token: "shh".into() });

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wrong_passphrase_does_not_unlock() {
        let dir = temp_dir("secret_store_wrong_passphrase");
        let salt_path = dir.join("salt.hex");
        let vault_path = dir.join("vault.enc");

        let salt = vault::load_or_create_salt(&salt_path).unwrap();
        let key = vault::derive_key("correct", &salt);
        vault::save_encrypted(&key, &vault_path, &"secret").unwrap();

        let store = store_at(&dir);
        assert!(store.unlock("wrong").is_err());
        assert_eq!(store.get(), Err(Sealed));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn seal_wipes_immediately() {
        let dir = temp_dir("secret_store_seal");
        let store = store_at(&dir);
        store.unlock("pw").unwrap();
        assert!(store.get().is_ok());
        store.seal();
        assert_eq!(store.get(), Err(Sealed));
        assert_eq!(store.remaining(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remaining_reports_within_bounds_after_unlock() {
        let dir = temp_dir("secret_store_remaining");
        let store = store_at(&dir);
        store.unlock("pw").unwrap();
        let left = store.remaining().expect("freshly unlocked store has time left");
        assert!(left <= IDLE_TTL);
        assert!(left <= ABSOLUTE_TTL);
        assert!(left > Duration::from_secs(0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn expired_idle_seals_on_next_get() {
        let dir = temp_dir("secret_store_idle_expiry");
        let store = store_at(&dir);
        store.unlock("pw").unwrap();
        {
            // Force the idle deadline into the past without sleeping.
            let mut guard = store.state.lock().unwrap();
            if let Some(u) = guard.as_mut() {
                u.last_access = Instant::now() - IDLE_TTL - Duration::from_secs(1);
            }
        }
        assert_eq!(store.get(), Err(Sealed));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn expired_absolute_seals_even_with_recent_access() {
        let dir = temp_dir("secret_store_absolute_expiry");
        let store = store_at(&dir);
        store.unlock("pw").unwrap();
        {
            // Recent idle access, but the absolute cap has already passed.
            let mut guard = store.state.lock().unwrap();
            if let Some(u) = guard.as_mut() {
                u.unlocked_at = Instant::now() - ABSOLUTE_TTL - Duration::from_secs(1);
                u.last_access = Instant::now();
            }
        }
        assert_eq!(store.get(), Err(Sealed));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
