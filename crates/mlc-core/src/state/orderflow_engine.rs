//! Order-flow engine configuration — the process-global, user-level
//! settings surface for the order-flow markup engine (markup-engines wave,
//! 2026-08-25 — `docs/mlc/plans/markup-engines-2026-08-25.md`'s own
//! "Engine config" row of the five-part EWA template, system 4, "Order
//! flow").
//!
//! Modelled exactly on [`crate::state::smc_engine`] — see that module's own
//! doc for the full rationale of the shape reused here verbatim: RAM +
//! profile only, `OnceLock<RwLock<_>>`, read fresh on every use (never
//! cached), mutated in place and carried in the profile blob
//! (`ProfileUiSettings::orderflow_engine`) so it survives a reload — see
//! [`orderflow_engine_config`]/[`set_orderflow_engine_config`].
//!
//! Deliberately carries NO imbalance-ratio or absorption threshold: those
//! belong to `mlc_core::types::FootprintBar::compute_extras`, computed
//! upstream from the user's own footprint-display settings, and every order-
//! flow primitive READS the resulting flags — see this crate's own
//! `drawing::primitives_v2::orderflow` module doc for why a second,
//! independently-configured copy of those thresholds here would be exactly
//! the defect the plan keeps closing. The one thing this engine DOES
//! compute — delta divergence — is `mli`'s own vocabulary
//! (`mli::orderflow::delta_divergences`), and this config carries only the
//! swing width that call needs, plus the same judge-claims/lookback pair
//! every markup engine in this crate carries.
//!
//! `#[serde(default)]` on every field: a profile blob written before this
//! struct existed still loads, and lands on every default described below.

use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

/// One process-wide order-flow engine configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrderFlowEngineConfig {
    /// Whether an order-flow claim (`imbalance_zone`/`absorption_mark`/
    /// `delta_divergence`) reports its own guard rule(s) at all. Default
    /// `true` — same posture and same rationale as
    /// [`crate::state::smc_engine::SmcEngineConfig::judge_claims`].
    #[serde(default = "default_true")]
    pub judge_claims: bool,

    /// How many bars BEFORE a mark's own anchor bar the order-flow
    /// detectors are given when judging that mark — the backward bound of
    /// the window
    /// [`crate::drawing::primitives_v2::orderflow::orderflow_judge::footprint_judge_window`]
    /// builds. Same meaning as
    /// [`crate::state::smc_engine::SmcEngineConfig::judge_lookback_bars`]:
    /// the FORWARD bound is deliberately not a config field at all (swing
    /// confirmation looks forward from the event that creates it), and this
    /// bounds what `delta_divergence` is handed to resolve a claim, never
    /// what a primitive draws on screen. Default 300.
    #[serde(default = "default_judge_lookback_bars")]
    pub judge_lookback_bars: usize,

    /// The fractal half-width `mli::orderflow::delta_divergences` resolves
    /// its swing highs/lows with (`mli::orderflow::DeltaDivergenceConfig::
    /// swing_n`) — the ONE parameter that detector exposes beyond the
    /// series itself. Default 2, matching
    /// `mli::smc::SmcConfig::default().swing_n` (the same default
    /// `mli::orderflow::DeltaDivergenceConfig::default()` reproduces, since
    /// that module defines no swing rule of its own — see its own doc).
    #[serde(default = "default_delta_divergence_swing_n")]
    pub delta_divergence_swing_n: usize,

    /// How many CONSECUTIVE one-sided price levels a single-print run must
    /// span before `order_flow_scan` reports it. Default 3.
    ///
    /// `FootprintLevel::is_single_print` is a per-LEVEL flag, and on a fine
    /// price grid most levels carry it — measured on BTCUSDT 1m over 65
    /// footprint bars: 13,916 flagged levels, ~214 per bar. Reported one by
    /// one that is not a survey finding, it is the texture of the footprint,
    /// and it buried every other structure the survey drew. What traders
    /// actually read is a RUN of adjacent one-sided levels — a gap in the
    /// profile — so the survey collapses each run to one hit and drops runs
    /// shorter than this. 1 restores the old per-level behaviour.
    #[serde(default = "default_min_single_print_run")]
    pub min_single_print_run: usize,
    /// How many bars either side of a freshly placed claim its own detector
    /// may be searched for the exact bar it names, before the claim gives
    /// up and reports its violation where it was put. Default 3, `0`
    /// disables snapping entirely.
    ///
    /// See [`crate::drawing::primitives_v2::Primitive::snap_to_detected`]
    /// for why a claim needs this at all: every detector here answers about
    /// ONE exact bar, and hitting that bar by eye out of a screen full of
    /// candles is guesswork.
    #[serde(default = "default_claim_snap_bars")]
    pub claim_snap_bars: usize,

}

fn default_true() -> bool {
    true
}

fn default_judge_lookback_bars() -> usize {
    300
}

fn default_delta_divergence_swing_n() -> usize {
    2
}

fn default_min_single_print_run() -> usize {
    3
}

fn default_claim_snap_bars() -> usize {
    3
}

impl Default for OrderFlowEngineConfig {
    fn default() -> Self {
        Self {
            judge_claims: true,
            judge_lookback_bars: default_judge_lookback_bars(),
            delta_divergence_swing_n: default_delta_divergence_swing_n(),
            min_single_print_run: default_min_single_print_run(),
            claim_snap_bars: default_claim_snap_bars(),
        }
    }
}

impl OrderFlowEngineConfig {
    /// The ONE place this chart's own config maps to
    /// `mli::orderflow::DeltaDivergenceConfig` — every order-flow primitive
    /// that calls `mli::orderflow::delta_divergences` calls this rather than
    /// building its own config literal, so the mapping can never drift
    /// between the claim and the survey. Both `DivergenceKind`s are always
    /// requested: a claim/survey needs to see BOTH to tell "not formed" from
    /// "the other kind formed instead" (see
    /// `drawing::primitives_v2::orderflow::delta_divergence`'s own doc), and
    /// this crate carries no per-kind toggle of its own to filter that.
    pub fn to_delta_divergence_config(&self) -> mli::orderflow::DeltaDivergenceConfig {
        mli::orderflow::DeltaDivergenceConfig {
            swing_n: self.delta_divergence_swing_n,
            kinds: vec![
                mli::events::divergence::DivergenceKind::Regular,
                mli::events::divergence::DivergenceKind::Hidden,
            ],
        }
    }
}

static ORDERFLOW_ENGINE_CONFIG: OnceLock<RwLock<OrderFlowEngineConfig>> = OnceLock::new();

/// Read the active order-flow engine configuration. Never fails — falls back
/// to [`OrderFlowEngineConfig::default`] on a poisoned lock (a settings read
/// must never crash a render frame), mirroring
/// [`crate::state::smc_engine::smc_engine_config`]'s own contract.
pub fn orderflow_engine_config() -> OrderFlowEngineConfig {
    ORDERFLOW_ENGINE_CONFIG
        .get_or_init(|| RwLock::new(OrderFlowEngineConfig::default()))
        .read()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

/// Set the active order-flow engine configuration. Called once at startup
/// (profile load, `ChartApp::apply_profile_state`) — every reader picks it
/// up on the very next use because [`orderflow_engine_config`] is read
/// fresh, never cached, mirroring
/// [`crate::state::smc_engine::set_smc_engine_config`]'s own contract.
pub fn set_orderflow_engine_config(config: OrderFlowEngineConfig) {
    let lock = ORDERFLOW_ENGINE_CONFIG.get_or_init(|| RwLock::new(OrderFlowEngineConfig::default()));
    if let Ok(mut guard) = lock.write() {
        *guard = config;
    }
}

/// Serializes every test in this crate that reads or writes the shared
/// [`ORDERFLOW_ENGINE_CONFIG`] global — `cargo test`'s default parallel
/// runner otherwise interleaves two such tests on separate OS threads, and a
/// bare `RwLock` swap gives no isolation between them. Same contract as
/// [`crate::state::smc_engine::engine_config_test_lock`] — every test
/// anywhere in this crate that touches this global acquires this guard
/// FIRST, as its very first statement.
#[cfg(test)]
pub(crate) fn engine_config_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_maps_to_mli_defaults() {
        let cfg = OrderFlowEngineConfig::default();
        assert!(cfg.judge_claims);
        assert_eq!(cfg.judge_lookback_bars, 300);
        assert_eq!(cfg.delta_divergence_swing_n, 2);
        let mapped = cfg.to_delta_divergence_config();
        assert_eq!(mapped.swing_n, 2);
        assert_eq!(
            mapped.kinds,
            vec![
                mli::events::divergence::DivergenceKind::Regular,
                mli::events::divergence::DivergenceKind::Hidden,
            ]
        );
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let restored: OrderFlowEngineConfig =
            serde_json::from_str("{}").expect("empty object deserializes via field defaults");
        assert_eq!(restored, OrderFlowEngineConfig::default());
    }

    #[test]
    fn set_then_read_round_trips() {
        let _guard = engine_config_test_lock();
        let mut cfg = OrderFlowEngineConfig::default();
        cfg.judge_claims = false;
        cfg.judge_lookback_bars = 50;
        cfg.delta_divergence_swing_n = 5;
        set_orderflow_engine_config(cfg.clone());
        assert_eq!(orderflow_engine_config(), cfg);
        // Restore the default so other tests in this process observe the
        // documented starting state (the global is process-wide).
        set_orderflow_engine_config(OrderFlowEngineConfig::default());
    }

    #[test]
    fn swing_n_passes_through_unpinned() {
        let cfg = OrderFlowEngineConfig { delta_divergence_swing_n: 7, ..OrderFlowEngineConfig::default() };
        assert_eq!(cfg.to_delta_divergence_config().swing_n, 7);
    }
}
