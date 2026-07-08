//! Chart-type registry — data-only catalog of every `chart_type` id and its
//! static properties (grouping, render kind, series variant, TF/grid gating,
//! default companion indicators, parameter spec).
//!
//! Step 1 of the chart-type registry arc was a pure-data addition with no
//! call sites. First live consumer (derived-prepend-fold-plan-2026-07-07,
//! Step 5): `mlc-app::active_series::chart_type_deepen_is_teardown` reads
//! `ChartTypeDef::deepen_kind` to route scroll-left deepen between the
//! in-place prepend-fold path and the legacy teardown+respawn path. The
//! remaining string-keyed dispatch tables in `mlc-app`/`mlc-app-layout` are
//! still the source of truth for everything else until a later arc step
//! migrates them over.

pub mod registry;

pub use registry::{
    all, by_id, BarsSourceKind, ChartTypeDef, ChartTypeGroup, ChartTypeKind,
    ChartTypeParamOption, ChartTypeParamSpec, DataMode, DataRequirement,
    DeepenKind, ResolutionKind, SeriesVariant, RESOLUTION_PARAM_ITEM_PREFIX,
};
