//! Template manager + facade module.
//!
//! `TemplateManager` promoted here from `mlc-core::templates::manager` per
//! doctrine 1A — the manager body is backend-plane state; the value types it
//! holds stay in `mlc-core::templates` as the type dictionary. App code
//! accesses it exclusively through the `Templates` / `TemplatesMut` facade
//! (see `BackendCore::templates` / `templates_mut` in
//! `crates/mlc-backend/src/lib.rs`).

mod manager;
mod facade;
pub use manager::TemplateManager;
pub use facade::{Templates, TemplatesMut};
