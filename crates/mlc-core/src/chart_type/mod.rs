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
pub mod identity;
pub mod generation;
pub mod resolver;
pub mod interval_intern;

pub use registry::{
    all, by_id, BarsSourceKind, ChartTypeDef, ChartTypeGroup, ChartTypeKind,
    ChartTypeParamOption, ChartTypeParamSpec, DataMode, DataRequirement,
    DeepenKind, FixedConsts, GranularityEditRender, GranularityTarget,
    IdentityKey, MachineKind,
    ParamMode, RenderPolicy, ResolutionKind, SeriesVariant, WickPolicy,
    RESOLUTION_PARAM_ATR14_ITEM_ID, RESOLUTION_PARAM_ITEM_PREFIX,
    TF_FAVORITE_CUSTOM_INPUT_ITEM_ID, PARAM_FAVORITE_CUSTOM_INPUT_ITEM_ID,
    INTERVAL_FAVORITE_CUSTOM_INPUT_ITEM_ID, GRANULARITY_INTERVAL_ID_PREFIX,
    GRANULARITY_STAR_ACTION_PREFIX, GRANULARITY_EDIT_ACTION_PREFIX,
    GRANULARITY_DELETE_ACTION_PREFIX, GRANULARITY_PIN_TOGGLE_ACTION_ID,
    GRANULARITY_QUICK_PICK_PREFIX, GRANULARITY_FAV_ID_PREFIX,
};
pub use identity::{
    bar_identity, bar_identity_key_fn, cvd_identity, cvd_identity_key_fn,
    footprint_identity, footprint_identity_key_fn, kagi_identity,
    kagi_identity_key_fn, merge_last_wins, pnf_identity, pnf_identity_key_fn,
    renko_identity, renko_identity_key_fn, three_line_break_identity,
    three_line_break_identity_key_fn, tpo_identity, tpo_identity_key_fn,
};
pub use generation::BarsGeneration;
pub use resolver::resolve_instrument_relative_default;
pub use interval_intern::intern_interval;
