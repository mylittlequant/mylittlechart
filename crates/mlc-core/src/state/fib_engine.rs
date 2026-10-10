//! Fib engine configuration — the process-global, SEED-ONLY settings surface
//! `FibScan::new()` reads once, at construction. Modelled exactly on
//! [`crate::state::harmonic_engine`]/[`crate::state::ewa_engine`] — see those
//! modules' own doc for the full rationale of the shape reused here
//! verbatim: `OnceLock<RwLock<_>>`, read fresh on every use (never cached),
//! mutated in place.
//!
//! `docs/mlc/plans/autodetectors-a2-a4-implementation-2026-09-24.md`,
//! "Architecture decision" + "A4.1 — `FibEngineConfig`": `swing_n`/
//! `swing_mode`/`last_swings_n`/`direction`/`enabled_levels` stay genuinely
//! PER-DRAWING fields on `FibScan` after construction (arc §2's own explicit
//! ruling for the sibling EWA/harmonic fields, applied here identically) —
//! this struct only seeds their starting value, the exact same "DEFAULT a
//! freshly-placed scan seeds from once, at construction" contract
//! [`crate::state::candle_pattern_engine::CandlePatternEngineConfig::
//! default_enabled_kinds`] already establishes for `CandlePatternScan::new()`.
//!
//! No settings-modal row in A4 scope (same as `CandlePatternEngineConfig`'s/
//! `HarmonicEngineConfig`'s own precedent — this config exists so
//! `FibScan::new()` has somewhere to read its own starting values from ONCE;
//! it is not user-editable process-globally in this phase).
//!
//! `#[serde(default)]` on every field: a profile blob written before this
//! struct existed still loads, and lands on every default described below.

use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

use crate::drawing::primitives_v2::fibonacci::retracement::MAIN_LEVELS;

/// One process-wide Fib engine configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FibEngineConfig {
    /// A freshly placed `FibScan`'s own starting pivot sensitivity — same
    /// per-drawing ruling `ElliottScan::swing_n`'s/`HarmonicScan::swing_n`'s
    /// own doc gives. Default `2`, matching both siblings' own default.
    #[serde(default = "default_scan_swing_n")]
    pub scan_default_swing_n: usize,
    /// A freshly placed `FibScan`'s own starting `swing_mode` wire id
    /// (`"largest"` | `"last_n"`, mirrors `mli::fib::swing::
    /// FibSwingMode`). Default `"largest"` — "the biggest high↔low move in
    /// range," the simplest reading a freshly dragged scan should show.
    #[serde(default = "default_scan_swing_mode")]
    pub scan_default_swing_mode: String,
    /// A freshly placed `FibScan`'s own starting `last_swings_n` — only
    /// read when `scan_default_swing_mode == "last_n"`. Default `1`, the
    /// smallest of the `[1, 2, 3, 5]` pills set `FibScan::style_properties`
    /// offers for this field.
    #[serde(default = "default_scan_last_swings_n")]
    pub scan_default_last_swings_n: usize,
    /// A freshly placed `FibScan`'s own starting `direction` wire id
    /// (`"auto"` | `"up"` | `"down"`, mirrors `mli::fib::swing::
    /// FibSwingDirection`). Default `"auto"` — no filtering, so an untouched
    /// engine config changes nothing about what a freshly drawn scan shows.
    #[serde(default = "default_scan_direction")]
    pub scan_default_direction: String,
    /// A freshly placed `FibScan`'s own starting `enabled_levels` — the
    /// SAME 7-value `MAIN_LEVELS` ladder `fibonacci::retracement::
    /// default_level_configs` already seeds a freshly placed manual
    /// `FibRetracement` with, so a scan-drawn ladder and a hand-placed one
    /// start on the same levels.
    #[serde(default = "default_scan_enabled_levels")]
    pub scan_default_enabled_levels: Vec<String>,
}

fn default_scan_swing_n() -> usize {
    2
}

fn default_scan_swing_mode() -> String {
    "largest".to_string()
}

fn default_scan_last_swings_n() -> usize {
    1
}

fn default_scan_direction() -> String {
    "auto".to_string()
}

fn default_scan_enabled_levels() -> Vec<String> {
    MAIN_LEVELS.iter().map(|v| format!("{v}")).collect()
}

impl Default for FibEngineConfig {
    fn default() -> Self {
        Self {
            scan_default_swing_n: default_scan_swing_n(),
            scan_default_swing_mode: default_scan_swing_mode(),
            scan_default_last_swings_n: default_scan_last_swings_n(),
            scan_default_direction: default_scan_direction(),
            scan_default_enabled_levels: default_scan_enabled_levels(),
        }
    }
}

static FIB_ENGINE_CONFIG: OnceLock<RwLock<FibEngineConfig>> = OnceLock::new();

/// Read the active Fib engine configuration. Never fails — falls back to
/// [`FibEngineConfig::default`] on a poisoned lock (a settings read must
/// never crash a render frame), mirroring [`crate::state::harmonic_engine::
/// harmonic_engine_config`]'s own contract.
pub fn fib_engine_config() -> FibEngineConfig {
    FIB_ENGINE_CONFIG
        .get_or_init(|| RwLock::new(FibEngineConfig::default()))
        .read()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

/// Set the active Fib engine configuration. Every reader picks it up on the
/// very next use because [`fib_engine_config`] is read fresh, never cached,
/// mirroring [`crate::state::harmonic_engine::set_harmonic_engine_config`]'s
/// own contract.
pub fn set_fib_engine_config(config: FibEngineConfig) {
    let lock = FIB_ENGINE_CONFIG.get_or_init(|| RwLock::new(FibEngineConfig::default()));
    if let Ok(mut guard) = lock.write() {
        *guard = config;
    }
}

/// Serializes every test in this crate that reads or writes the shared
/// [`FIB_ENGINE_CONFIG`] global — mirrors [`crate::state::harmonic_engine::
/// engine_config_test_lock`]'s own reason: `cargo test`'s default parallel
/// runner otherwise interleaves two such tests on separate OS threads, and a
/// bare `RwLock` swap gives no isolation between them. Every test anywhere in
/// this crate that touches this global acquires this guard FIRST, as its
/// very first statement.
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
    fn default_config_seeds_main_levels() {
        let _guard = engine_config_test_lock();
        let cfg = FibEngineConfig::default();
        assert_eq!(cfg.scan_default_swing_n, 2);
        assert_eq!(cfg.scan_default_swing_mode, "largest");
        assert_eq!(cfg.scan_default_last_swings_n, 1);
        assert_eq!(cfg.scan_default_direction, "auto");
        assert_eq!(cfg.scan_default_enabled_levels.len(), MAIN_LEVELS.len());
        for level in MAIN_LEVELS {
            assert!(cfg.scan_default_enabled_levels.contains(&format!("{level}")), "missing {level}");
        }
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let _guard = engine_config_test_lock();
        let restored: FibEngineConfig = serde_json::from_str("{}")
            .expect("empty object deserializes via field defaults");
        assert_eq!(restored, FibEngineConfig::default());
    }

    #[test]
    fn set_then_read_round_trips() {
        let _guard = engine_config_test_lock();
        let mut cfg = FibEngineConfig::default();
        cfg.scan_default_swing_n = 5;
        cfg.scan_default_swing_mode = "last_n".to_string();
        cfg.scan_default_last_swings_n = 3;
        cfg.scan_default_direction = "up".to_string();
        cfg.scan_default_enabled_levels = vec!["0".to_string(), "1".to_string()];
        set_fib_engine_config(cfg.clone());
        assert_eq!(fib_engine_config(), cfg);
        // Restore the default so other tests in this process observe the
        // documented starting state (the global is process-wide).
        set_fib_engine_config(FibEngineConfig::default());
    }
}
