//! Harmonic engine configuration — the process-global, SEED-ONLY settings
//! surface `HarmonicScan::new()` reads once, at construction. Modelled
//! exactly on [`crate::state::ewa_engine`]/[`crate::state::
//! candle_pattern_engine`] — see those modules' own doc for the full
//! rationale of the shape reused here verbatim: `OnceLock<RwLock<_>>`, read
//! fresh on every use (never cached), mutated in place.
//!
//! `docs/mlc/plans/autodetectors-a2-a4-implementation-2026-09-24.md`,
//! "Architecture decision" + "A3.2 — `HarmonicEngineConfig`": `tolerance`/
//! `min_score`/`enabled_kinds` stay genuinely PER-DRAWING fields on
//! `HarmonicScan` after construction (arc §3's own explicit ruling — "mirrors
//! `XabcdPattern::harmonic_tolerance`'s own per-drawing field") — this
//! struct only seeds their starting value, the exact same "DEFAULT a
//! freshly-placed scan seeds from once, at construction" contract
//! [`crate::state::candle_pattern_engine::CandlePatternEngineConfig::
//! default_enabled_kinds`] already establishes for `CandlePatternScan::new()`.
//!
//! No settings-modal row in A3 scope (same as `CandlePatternEngineConfig`'s
//! own precedent — this config exists so `HarmonicScan::new()` has
//! somewhere to read its own starting values from ONCE; it is not
//! user-editable process-globally in this phase).
//!
//! `#[serde(default)]` on every field: a profile blob written before this
//! struct existed still loads, and lands on every default described below.

use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

use mli::ewa::harmonic::DEFAULT_HARMONIC_TOLERANCE;

use crate::drawing::primitives_v2::patterns::default_all_harmonic_kind_ids;

/// One process-wide harmonic engine configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HarmonicEngineConfig {
    /// A freshly placed `HarmonicScan`'s own starting `tolerance` (widening
    /// delta applied to every target leg's own band, `mli::ewa::harmonic::
    /// widen_band`). Default [`DEFAULT_HARMONIC_TOLERANCE`] — the SAME
    /// vocabulary-own default `XabcdPattern::harmonic_tolerance`/
    /// `FiveZeroPattern::harmonic_tolerance` already seed at construction,
    /// never a second tolerance constant of this crate's own.
    #[serde(default = "default_scan_tolerance")]
    pub scan_default_tolerance: f64,
    /// A freshly placed `HarmonicScan`'s own starting `enabled_kinds` — see
    /// [`default_all_harmonic_kind_ids`]'s own doc. Default: all 11
    /// catalogue ids.
    #[serde(default = "default_scan_enabled_kinds")]
    pub scan_default_enabled_kinds: Vec<String>,
    /// A freshly placed `HarmonicScan`'s own starting `min_score` post-filter
    /// — see `elliott_scan::ElliottScan::min_score`'s own doc for the same
    /// field on the Elliott side. Default `0.0` ("Any" — no filtering), so
    /// an untouched engine config changes nothing about what a freshly
    /// drawn scan shows.
    #[serde(default)]
    pub scan_default_min_score: f64,
}

fn default_scan_tolerance() -> f64 {
    DEFAULT_HARMONIC_TOLERANCE
}

fn default_scan_enabled_kinds() -> Vec<String> {
    default_all_harmonic_kind_ids()
}

impl Default for HarmonicEngineConfig {
    fn default() -> Self {
        Self {
            scan_default_tolerance: default_scan_tolerance(),
            scan_default_enabled_kinds: default_scan_enabled_kinds(),
            scan_default_min_score: 0.0,
        }
    }
}

static HARMONIC_ENGINE_CONFIG: OnceLock<RwLock<HarmonicEngineConfig>> = OnceLock::new();

/// Read the active harmonic engine configuration. Never fails — falls back
/// to [`HarmonicEngineConfig::default`] on a poisoned lock (a settings read
/// must never crash a render frame), mirroring [`crate::state::ewa_engine::
/// ewa_engine_config`]'s own contract.
pub fn harmonic_engine_config() -> HarmonicEngineConfig {
    HARMONIC_ENGINE_CONFIG
        .get_or_init(|| RwLock::new(HarmonicEngineConfig::default()))
        .read()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

/// Set the active harmonic engine configuration. Every reader picks it up on
/// the very next use because [`harmonic_engine_config`] is read fresh, never
/// cached, mirroring [`crate::state::ewa_engine::set_ewa_engine_config`]'s
/// own contract.
pub fn set_harmonic_engine_config(config: HarmonicEngineConfig) {
    let lock = HARMONIC_ENGINE_CONFIG.get_or_init(|| RwLock::new(HarmonicEngineConfig::default()));
    if let Ok(mut guard) = lock.write() {
        *guard = config;
    }
}

/// Serializes every test in this crate that reads or writes the shared
/// [`HARMONIC_ENGINE_CONFIG`] global — mirrors [`crate::state::
/// candle_pattern_engine::engine_config_test_lock`]'s own reason: `cargo
/// test`'s default parallel runner otherwise interleaves two such tests on
/// separate OS threads, and a bare `RwLock` swap gives no isolation between
/// them. Every test anywhere in this crate that touches this global
/// acquires this guard FIRST, as its very first statement.
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
    use crate::drawing::primitives_v2::patterns::HarmonicScanKind;

    #[test]
    fn default_config_enables_every_catalogue_kind() {
        let _guard = engine_config_test_lock();
        let cfg = HarmonicEngineConfig::default();
        assert_eq!(cfg.scan_default_enabled_kinds.len(), HarmonicScanKind::ALL.len());
        for kind in HarmonicScanKind::ALL {
            assert!(cfg.scan_default_enabled_kinds.contains(&kind.id().to_string()), "missing {}", kind.id());
        }
        assert_eq!(cfg.scan_default_tolerance, DEFAULT_HARMONIC_TOLERANCE);
        assert_eq!(cfg.scan_default_min_score, 0.0);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let _guard = engine_config_test_lock();
        let restored: HarmonicEngineConfig = serde_json::from_str("{}")
            .expect("empty object deserializes via field defaults");
        assert_eq!(restored, HarmonicEngineConfig::default());
    }

    #[test]
    fn set_then_read_round_trips() {
        let _guard = engine_config_test_lock();
        let mut cfg = HarmonicEngineConfig::default();
        cfg.scan_default_tolerance = 0.02;
        cfg.scan_default_min_score = 0.7;
        cfg.scan_default_enabled_kinds = vec!["gartley".to_string(), "five_zero".to_string()];
        set_harmonic_engine_config(cfg.clone());
        assert_eq!(harmonic_engine_config(), cfg);
        // Restore the default so other tests in this process observe the
        // documented starting state (the global is process-wide).
        set_harmonic_engine_config(HarmonicEngineConfig::default());
    }
}
