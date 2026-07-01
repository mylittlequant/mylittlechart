//! Tag manager + facade module.
//!
//! `TagManager` promoted here from `mlc-app::tag::TagStore` per doctrine 1B
//! — bubble grouping / member routing / synced-panel set is domain, a
//! headless daemon UI cannot sync panels without it. App code accesses it
//! exclusively through the `Tags` / `TagsMut` facade (see
//! `BackendCore::tags` / `tags_mut` in `crates/mlc-backend/src/lib.rs`).

mod manager;
mod facade;
pub use manager::{TagManager, TagManagerError};
pub use facade::{Tags, TagsMut};
