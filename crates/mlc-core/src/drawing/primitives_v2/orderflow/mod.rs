//! Order flow — markup engine #4 (`docs/mlc/plans/markup-engines-2026-08-25.md`,
//! Appendix B). Three CLAIM primitives, each anchored on a bar/bars the
//! analyst marks and reading ONLY per-bar facts
//! [`mlc_core::types::FootprintBar`] already carries (computed upstream by
//! `FootprintBar::compute_extras` from the user's own configured
//! thresholds — never recomputed here), plus the survey (`order_flow_scan`).
//!
//! `delta_divergence` is the one exception that computes anything at all:
//! it reads `mli::orderflow::delta_divergences` over each footprint bar's
//! own `cumulative_delta` — see that primitive's own module doc.
//!
//! **Honest degradation is the family's own defining rule** (Appendix B):
//! a series with no footprint data (`DataBubble::footprint_bars == None`,
//! the normal case for a plain kline chart) makes every primitive here
//! report `"order_flow_data_missing"` — never an empty zone, never a
//! bar-direction guess. See [`orderflow_judge::MISSING_VIOLATION`]'s own
//! doc for how that is reported even though
//! [`crate::drawing::primitives_v2::Primitive::refresh_footprint_derived_cache`]
//! is never CALLED at all in that case.

mod orderflow_judge;
pub mod imbalance_zone;
pub mod absorption_mark;
pub mod delta_divergence;
pub mod order_flow_scan;

pub use imbalance_zone::ImbalanceZone;
pub use absorption_mark::AbsorptionMark;
pub use delta_divergence::DeltaDivergence;
pub use order_flow_scan::OrderFlowScan;
