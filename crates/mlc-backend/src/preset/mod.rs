//! Preset manager + facade module.
//!
//! `PresetManager` wraps the in-memory chart-preset registry (previously a
//! bare `HashMap<String, ChartPreset>` field on `BackendCore`). App code
//! accesses it exclusively through the `Presets` / `PresetsMut` facade (see
//! `BackendCore::presets` / `presets_mut` in `crates/mlc-backend/src/lib.rs`).
//! `ChartPreset` itself stays a value type in `mlc-core`.

mod manager;
mod facade;
pub use manager::PresetManager;
pub use facade::{Presets, PresetsMut};
