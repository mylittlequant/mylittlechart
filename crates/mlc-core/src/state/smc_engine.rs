//! Smart-money-concepts engine configuration — the process-global,
//! user-level settings surface for the smart-money markup engine
//! (markup-engines wave, 2026-08-25 —
//! `docs/mlc/plans/markup-engines-2026-08-25.md`'s own "Engine config" row
//! of the five-part EWA template, system 2, "Smart money").
//!
//! Modelled exactly on [`crate::state::candle_pattern_engine`] — see that
//! module's own doc for the full rationale of the shape reused here
//! verbatim: RAM + profile only, `OnceLock<RwLock<_>>`, read fresh on every
//! use (never cached), mutated in place and carried in the profile blob
//! (`ProfileUiSettings::smc_engine`) so it survives a reload — see
//! [`smc_engine_config`]/[`set_smc_engine_config`].
//!
//! Carries the chart's own copy of every threshold
//! `mli::smc::config::SmcConfig` declares, INCLUDING
//! `amd_max_distribution_lag` — that field was left unread through the
//! order-block/FVG/sweep/structure-break wave (it belongs to the AMD
//! assembly, `docs/mlc/plans/markup-engines-2026-08-25.md` system 3, a
//! later wave at the time), and [`SmcEngineConfig::to_smc_config`] filled it
//! from `mli`'s own default rather than expose a "control over a field
//! nothing reads" defect. The AMD wave
//! (`drawing::primitives_v2::smc::amd_cycle`) is that later wave: the field
//! is now real (`amd_max_distribution_lag`, below) and
//! [`SmcEngineConfig::to_smc_config`] passes the config's own value through.
//!
//! Like the candle-pattern engine, this config has no settings-modal UI yet.
//! Its fields are read by every one of the six SMC claim primitives
//! (`drawing::primitives_v2::smc::{order_block, fair_value_gap,
//! liquidity_sweep, structure_break, dealing_range, amd_cycle}`) and the
//! survey (`smc_scan`), each through the shared
//! [`crate::drawing::primitives_v2::smc::smc_judge::judge_window`] helper —
//! a control over a field nothing reads is a defect, and every field below
//! already has its reader.
//!
//! `#[serde(default)]` on every field: a profile blob written before this
//! struct existed still loads, and lands on every default described below.

use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

/// The chart's own mirror of `mli::smc::config::OrderBlockZone`.
///
/// Re-declared here rather than storing `mli`'s own enum directly, and the
/// reason is a standing rule, not an accident of what happens to derive
/// what: the indicator family carries NO serde. Its types are strictly
/// typed structs and real enums, and every JSON boundary belongs to the
/// CONSUMER — which for this config is the chart's own profile blob.
/// Deriving `Serialize` upstream to save this mirror would push a wire
/// shape into an engine that is deliberately built not to know about one.
///
/// [`Self::to_mli`] is the ONE place the two enums map, so they cannot
/// silently drift — a new `mli` variant fails to compile here rather than
/// being quietly dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderBlockZoneMode {
    /// Mirrors `mli::smc::config::OrderBlockZone::FullRange` — Appendix A's
    /// primary definition, `[low, high]` of the candle.
    FullRange,
    /// Mirrors `mli::smc::config::OrderBlockZone::BodyOnly` — the
    /// configurable alternative Appendix A names as "also in use", not
    /// universal.
    BodyOnly,
}

impl OrderBlockZoneMode {
    /// The one place this mode maps to `mli`'s own enum.
    pub fn to_mli(self) -> mli::smc::OrderBlockZone {
        match self {
            Self::FullRange => mli::smc::OrderBlockZone::FullRange,
            Self::BodyOnly => mli::smc::OrderBlockZone::BodyOnly,
        }
    }
}

impl Default for OrderBlockZoneMode {
    /// Mirrors `mli::smc::config::SmcConfig::default().order_block_zone`.
    fn default() -> Self {
        Self::FullRange
    }
}

/// One process-wide smart-money engine configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SmcEngineConfig {
    /// Mirrors `mli::smc::config::SmcConfig::swing_n`. Appendix A default: 2.
    #[serde(default = "default_swing_n")]
    pub swing_n: usize,
    /// Mirrors `mli::smc::config::SmcConfig::atr_period`. Appendix A default: 14.
    #[serde(default = "default_atr_period")]
    pub atr_period: usize,
    /// Mirrors `mli::smc::config::SmcConfig::displacement_atr_mult`.
    /// Appendix A default: 2.0.
    #[serde(default = "default_displacement_atr_mult")]
    pub displacement_atr_mult: f64,
    /// Mirrors `mli::smc::config::SmcConfig::equal_level_atr_tol`. Appendix A
    /// default: 0.1.
    #[serde(default = "default_equal_level_atr_tol")]
    pub equal_level_atr_tol: f64,
    /// Mirrors `mli::smc::config::SmcConfig::sweep_recovery_bars`. Appendix A
    /// default: 1.
    #[serde(default = "default_sweep_recovery_bars")]
    pub sweep_recovery_bars: usize,
    /// Mirrors `mli::smc::config::SmcConfig::order_block_zone` — see
    /// [`OrderBlockZoneMode`]'s own doc for why this crate carries its own
    /// mirror enum rather than `mli`'s.
    #[serde(default)]
    pub order_block_zone: OrderBlockZoneMode,

    /// Whether an SMC claim (`order_block`/`fair_value_gap`/
    /// `liquidity_sweep`/`structure_break`) reports its own guard rule(s) at
    /// all. Default `true` — same posture and same rationale as
    /// [`crate::state::candle_pattern_engine::CandlePatternEngineConfig::judge_claims`].
    #[serde(default = "default_true")]
    pub judge_claims: bool,

    /// How many bars BEFORE a mark's own anchor bar the SMC detectors are
    /// given when judging that mark — the backward bound of the window
    /// [`crate::drawing::primitives_v2::smc::smc_judge::judge_window`]
    /// builds. The FORWARD bound is deliberately not a config field at all:
    /// it is simply every bar loaded from the anchor onward, because
    /// mitigation/breaker/recovery state all look FORWARD from the event
    /// that creates them, and capping that side would make a claim's own
    /// outcome depend on how much history happens to be loaded past it
    /// rather than on the market itself.
    ///
    /// This is a JUDGEMENT window, not a DISPLAY window — it bounds what the
    /// detectors are handed to decide whether a claim holds, never what a
    /// primitive draws on screen (`order_block`/`fair_value_gap` draw their
    /// own resolved zone out to their own `extend_right`-governed edge;
    /// `liquidity_sweep`/`structure_break` draw their own resolved level
    /// line; none of the four reads this field to decide where its own
    /// geometry stops). It exists because a mark's own judgement needs real
    /// structural history (an order block needs its own swing/BOS lineage
    /// behind it, a sweep needs the swing it wicked) but must not re-walk
    /// the ENTIRE loaded series for every mark on every bar refresh — the
    /// same cost concern `docs/mlc/plans/markup-engines-2026-08-25.md` names
    /// for the candle-pattern scan's own bounded window. Default 300.
    #[serde(default = "default_judge_lookback_bars")]
    pub judge_lookback_bars: usize,

    /// Mirrors `mli::smc::config::SmcConfig::amd_max_distribution_lag` — how
    /// many bars after a manipulation sweep [`crate::drawing::primitives_v2::
    /// smc::amd_cycle::AmdCycle`] will still accept a displacement leg as
    /// that cycle's distribution phase. `mli`'s own default: 10. See this
    /// module's own doc for why this field has a real reader now
    /// (`SmcEngineConfig::to_smc_config`) rather than being filled from
    /// `mli`'s default underneath a chart-side knob nothing read.
    #[serde(default = "default_amd_max_distribution_lag")]
    pub amd_max_distribution_lag: usize,
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

    /// How many bars back a claim may look for the claim it CONTINUES —
    /// the window `mlc_backend::bubble::markup_guards` derives
    /// `markup_follows` inside. Default 30, `0` disables chaining.
    ///
    /// A window rather than an unbounded search because succession here is
    /// derived, not declared (`markup::chain`'s own module doc): a break
    /// three hundred bars after a sweep is not the shift that sweep
    /// predicted, and linking them would put a chain rule's verdict on two
    /// claims that have nothing to do with each other.
    #[serde(default = "default_chain_window_bars")]
    pub chain_window_bars: usize,

}

fn default_true() -> bool {
    true
}

fn default_swing_n() -> usize {
    2
}

fn default_atr_period() -> usize {
    14
}

fn default_displacement_atr_mult() -> f64 {
    2.0
}

fn default_equal_level_atr_tol() -> f64 {
    0.1
}

fn default_sweep_recovery_bars() -> usize {
    1
}

fn default_judge_lookback_bars() -> usize {
    300
}

fn default_amd_max_distribution_lag() -> usize {
    10
}

fn default_claim_snap_bars() -> usize {
    3
}

fn default_chain_window_bars() -> usize {
    30
}

impl Default for SmcEngineConfig {
    fn default() -> Self {
        Self {
            swing_n: default_swing_n(),
            atr_period: default_atr_period(),
            displacement_atr_mult: default_displacement_atr_mult(),
            equal_level_atr_tol: default_equal_level_atr_tol(),
            sweep_recovery_bars: default_sweep_recovery_bars(),
            order_block_zone: OrderBlockZoneMode::default(),
            judge_claims: true,
            judge_lookback_bars: default_judge_lookback_bars(),
            amd_max_distribution_lag: default_amd_max_distribution_lag(),
            claim_snap_bars: default_claim_snap_bars(),
            chain_window_bars: default_chain_window_bars(),
        }
    }
}

impl SmcEngineConfig {
    /// The ONE place this chart's own config maps to
    /// `mli::smc::config::SmcConfig` — every SMC primitive calls this rather
    /// than building its own `mli::smc::SmcConfig` literal, so the mapping
    /// can never drift between primitives.
    pub fn to_smc_config(&self) -> mli::smc::SmcConfig {
        mli::smc::SmcConfig {
            swing_n: self.swing_n,
            atr_period: self.atr_period,
            displacement_atr_mult: self.displacement_atr_mult,
            equal_level_atr_tol: self.equal_level_atr_tol,
            sweep_recovery_bars: self.sweep_recovery_bars,
            order_block_zone: self.order_block_zone.to_mli(),
            amd_max_distribution_lag: self.amd_max_distribution_lag,
        }
    }
}

static SMC_ENGINE_CONFIG: OnceLock<RwLock<SmcEngineConfig>> = OnceLock::new();

/// Read the active smart-money engine configuration. Never fails — falls
/// back to [`SmcEngineConfig::default`] on a poisoned lock (a settings read
/// must never crash a render frame), mirroring
/// [`crate::state::candle_pattern_engine::candle_pattern_engine_config`]'s
/// own contract.
pub fn smc_engine_config() -> SmcEngineConfig {
    SMC_ENGINE_CONFIG
        .get_or_init(|| RwLock::new(SmcEngineConfig::default()))
        .read()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

/// Set the active smart-money engine configuration. Called once at startup
/// (profile load, `ChartApp::apply_profile_state`) — every reader picks it
/// up on the very next use because [`smc_engine_config`] is read fresh,
/// never cached, mirroring
/// [`crate::state::candle_pattern_engine::set_candle_pattern_engine_config`]'s
/// own contract.
pub fn set_smc_engine_config(config: SmcEngineConfig) {
    let lock = SMC_ENGINE_CONFIG.get_or_init(|| RwLock::new(SmcEngineConfig::default()));
    if let Ok(mut guard) = lock.write() {
        *guard = config;
    }
}

/// Serializes every test in this crate that reads or writes the shared
/// [`SMC_ENGINE_CONFIG`] global — `cargo test`'s default parallel runner
/// otherwise interleaves two such tests on separate OS threads, and a bare
/// `RwLock` swap gives no isolation between them. Same contract as
/// [`crate::state::candle_pattern_engine::engine_config_test_lock`] — every
/// test anywhere in this crate that touches this global acquires this guard
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
        let cfg = SmcEngineConfig::default();
        let mli_default = mli::smc::SmcConfig::default();
        let mapped = cfg.to_smc_config();
        assert_eq!(mapped.swing_n, mli_default.swing_n);
        assert_eq!(mapped.atr_period, mli_default.atr_period);
        assert_eq!(mapped.displacement_atr_mult, mli_default.displacement_atr_mult);
        assert_eq!(mapped.equal_level_atr_tol, mli_default.equal_level_atr_tol);
        assert_eq!(mapped.sweep_recovery_bars, mli_default.sweep_recovery_bars);
        assert_eq!(mapped.order_block_zone, mli_default.order_block_zone);
        assert_eq!(mapped.amd_max_distribution_lag, mli_default.amd_max_distribution_lag);
        assert!(cfg.judge_claims);
        assert_eq!(cfg.judge_lookback_bars, 300);
    }

    /// `amd_max_distribution_lag` is a real, read field — `to_smc_config`
    /// passes the config's OWN value through rather than always filling
    /// `mli`'s default, unlike the earlier wave (see this module's own
    /// doc). `amd_cycle`'s own test suite confirms the value actually
    /// changes which cycles resolve; this test only confirms the mapping
    /// itself is not silently pinned to a constant.
    #[test]
    fn amd_max_distribution_lag_passes_through_unpinned() {
        let cfg = SmcEngineConfig { amd_max_distribution_lag: 0, ..SmcEngineConfig::default() };
        assert_eq!(cfg.to_smc_config().amd_max_distribution_lag, 0);
        let cfg = SmcEngineConfig { amd_max_distribution_lag: 42, ..SmcEngineConfig::default() };
        assert_eq!(cfg.to_smc_config().amd_max_distribution_lag, 42);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let restored: SmcEngineConfig =
            serde_json::from_str("{}").expect("empty object deserializes via field defaults");
        assert_eq!(restored, SmcEngineConfig::default());
    }

    #[test]
    fn set_then_read_round_trips() {
        let _guard = engine_config_test_lock();
        let mut cfg = SmcEngineConfig::default();
        cfg.judge_claims = false;
        cfg.judge_lookback_bars = 50;
        cfg.order_block_zone = OrderBlockZoneMode::BodyOnly;
        set_smc_engine_config(cfg.clone());
        assert_eq!(smc_engine_config(), cfg);
        // Restore the default so other tests in this process observe the
        // documented starting state (the global is process-wide).
        set_smc_engine_config(SmcEngineConfig::default());
    }
}
