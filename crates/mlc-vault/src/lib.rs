//! mylittlechart-vault — zero-trust blob primitives.
//!
//! Four modules:
//! - `crypto`  — AES-GCM, HKDF, PBKDF2 key derivation, salt management.
//! - `vault`   — sealed-blob encrypt/decrypt helpers + passphrase validation.
//! - `storage` — platform-agnostic byte-blob [`Storage`] trait + global handle.
//! - `store`   — [`SecretStore`], the RAM-only TTL-bounded holder built on
//!   top of the other three.
//!
//! This crate is a pure leaf: depends only on `aes-gcm`, `pbkdf2`, `hkdf`,
//! `rand`, `sha2`, `hex`, `serde`, `zeroize`, `web-time`. No workspace deps.

pub mod crypto;
pub mod storage;
pub mod store;
pub mod vault;

pub use crypto::{MasterKey, derive_master_key, load_or_create_salt};
pub use storage::{Storage, global_storage, set_global_storage, FilesystemStorage};
pub use store::{SecretStore, Sealed, ABSOLUTE_TTL, IDLE_TTL};
pub use vault::{VaultKey, VaultError, derive_key, encrypt_json, decrypt_json,
                load_encrypted, load_encrypted_raw, save_encrypted, validate_passphrase_at};
