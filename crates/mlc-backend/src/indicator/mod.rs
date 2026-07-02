//! Indicator manager + facade module.
//!
//! `IndicatorManager` (the stateful registry: instances, catalog, per-bubble
//! assignment, recalc scheduling) lives here per doctrine 3.7a — the pure
//! compute half (calculator, catalog bridge, `mli` bridge, `ComputeCache`)
//! stays in `mlc-backend-indicator-engine`, which this module calls into.
//! App code accesses the manager exclusively through the `Indicators` /
//! `IndicatorsMut` facade (see `BackendCore::indicators` / `indicators_mut`
//! in `crates/mlc-backend/src/lib.rs`).

mod facade;
mod manager;
pub use facade::{BubbleIndicatorView, IndicatorFullConfig, Indicators, IndicatorsMut};
pub use manager::IndicatorManager;
