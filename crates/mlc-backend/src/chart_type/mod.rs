//! Chart-type data-plane engine — Step 8 of the chart-type registry arc.
//!
//! `SubscribeOps` composes `DataBridge` (subscribe/abort network calls) with
//! `DataBubble` (slot-clear bookkeeping) into one per-chart-type fn-pointer
//! table. Zero call sites migrated yet — see `engine.rs` module doc.

pub mod engine;

pub use engine::{
    SubscribeOps, SubscribeParams, ops_by_id,
    deep_trade_history_available, is_footprint_hybrid, clear_slot_footprint_hybrid_aware,
    fold_chart_types, is_fold_chart_type, fold_params_from_subscribe_params,
    trade_fold_chart_types, is_trade_fold_chart_type, trade_fold_params_from_subscribe_params,
    fold_params_for,
    DEFAULT_TPO_SOURCE,
    clear_slot_dom_heatmap, clear_slot_liq_heatmap,
};
