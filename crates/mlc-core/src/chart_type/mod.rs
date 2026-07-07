//! Chart-type registry — data-only catalog of every `chart_type` id and its
//! static properties (grouping, render kind, series variant, TF/grid gating,
//! default companion indicators, parameter spec).
//!
//! Step 1 of the chart-type registry arc: this module is a pure-data
//! addition. No existing call site reads from it yet — the string-keyed
//! dispatch tables in `mlc-app`/`mlc-app-layout` remain the source of truth
//! until a later arc step migrates them over.

pub mod registry;

pub use registry::{
    all, by_id, BarsSourceKind, ChartTypeDef, ChartTypeGroup, ChartTypeKind,
    ChartTypeParamOption, ChartTypeParamSpec, DeepenKind, SeriesVariant,
};
