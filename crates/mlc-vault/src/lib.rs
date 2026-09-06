//! mylittlechart-vault — zero-trust blob primitives.
//!
//! Five modules:
//! - `crypto`  — AES-GCM, HKDF, PBKDF2 key derivation, salt management.
//! - `vault`   — sealed-blob encrypt/decrypt helpers + passphrase validation.
//! - `storage` — platform-agnostic byte-blob [`Storage`] trait + global handle.
//! - `store`   — [`SecretStore`], the RAM-only TTL-bounded holder built on
//!   top of the other three.
//! - `dm`      — end-to-end DM primitives: X25519 identity keypairs,
//!   per-conversation key derivation, AES-256-GCM sealed messages.
//!
//! This crate is a pure leaf: depends only on `aes-gcm`, `pbkdf2`, `hkdf`,
//! `rand`, `sha2`, `hex`, `serde`, `zeroize`, `web-time`, `subtle`,
//! `curve25519-dalek`. No workspace deps.

pub mod crypto;
pub mod dm;
pub mod storage;
pub mod store;
pub mod vault;

pub use crypto::{MasterKey, derive_master_key, load_or_create_salt, generate_passphrase};
pub use storage::{Storage, global_storage, set_global_storage, FilesystemStorage};
pub use store::{SecretStore, Sealed, ABSOLUTE_TTL, IDLE_TTL};
pub use vault::{VaultKey, VaultError, derive_key, encrypt_json, decrypt_json,
                load_encrypted, load_encrypted_raw, save_encrypted, validate_passphrase_at};
