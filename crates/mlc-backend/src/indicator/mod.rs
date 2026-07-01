//! Indicator facade module.
//!
//! `IndicatorManager` itself lives in `mlc-backend-indicator-engine` (Audit-C verdict:
//! 7520 LOC compute-heavy crate, wrap rather than move —
//! `docs/mlc/plans/step-3.3-indicator-watchlist-design.md` §1's "Physical
//! location" discussion). App code accesses it exclusively through the
//! `Indicators` / `IndicatorsMut` facade (see `BackendCore::indicators` /
//! `indicators_mut` in `crates/mlc-backend/src/lib.rs`).

mod facade;
pub use facade::{BubbleIndicatorView, Indicators, IndicatorsMut};
