//! Chart-type data-plane engine — Step 8 of the chart-type registry arc.
//!
//! `SubscribeOps` composes `DataBridge` (subscribe/abort network calls) with
//! `DataBubble` (slot-clear bookkeeping) into one per-chart-type fn-pointer
//! table. Zero call sites migrated yet — see `engine.rs` module doc.

pub mod engine;

pub use engine::{
    SubscribeOps, SubscribeParams, ops_by_id,
    is_footprint_hybrid, clear_slot_footprint_hybrid_aware,
    FOLD_CHART_TYPES, is_fold_chart_type, fold_params_from_subscribe_params,
    TRADE_FOLD_CHART_TYPES, is_trade_fold_chart_type, trade_fold_params_from_subscribe_params,
    DEFAULT_TPO_SOURCE,
};
