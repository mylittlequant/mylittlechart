//! Candle pattern engine configuration — the process-global, user-level
//! settings surface for the candle-pattern markup engine (markup-engines
//! wave, 2026-08-25 — `docs/mlc/plans/markup-engines-2026-08-25.md`'s own
//! "Engine config" row of the five-part EWA template).
//!
//! Modelled exactly on [`crate::state::ewa_engine`] — see that module's own
//! doc for the full rationale of the shape reused here verbatim: RAM +
//! profile only, `OnceLock<RwLock<_>>`, read fresh on every use (never
//! cached), mutated in place and carried in the profile blob
//! (`ProfileUiSettings::candle_pattern_engine`) so it survives a reload —
//! see [`candle_pattern_engine_config`]/[`set_candle_pattern_engine_config`].
//!
//! Unlike the EWA engine, this config has no settings-modal UI yet — no
//! `candle_pattern_engine_settings_state` exists to mirror it into. Its two
//! fields are read directly by the two primitives that need them
//! ([`crate::drawing::primitives_v2::patterns::candle_pattern_scan::CandlePatternScan::new`]
//! for [`CandlePatternEngineConfig::default_enabled_kinds`],
//! [`crate::drawing::primitives_v2::patterns::candle_pattern::CandlePattern`]'s
//! own `refresh_bar_derived_cache` for
//! [`CandlePatternEngineConfig::judge_claims`]) — a control over a field
//! nothing reads is a defect, and both fields already have their reader in
//! this same change.
//!
//! `#[serde(default)]` on every field: a profile blob written before this
//! struct existed still loads, and lands on every default described below.

use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

use mli::events::candle_pattern::CandlePatternKind;

/// One process-wide candle-pattern engine configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CandlePatternEngineConfig {
    /// The `mli` wire ids a NEWLY CREATED
    /// [`crate::drawing::primitives_v2::patterns::candle_pattern_scan::CandlePatternScan`]
    /// starts with. Read ONCE, at construction
    /// (`CandlePatternScan::new`), baked into the new primitive's own
    /// `enabled_kinds` field exactly the way [`crate::state::ewa_engine::
    /// RatioRibDefaults`] seeds a new Elliott structure's own rib style —
    /// never retro-applied to a scan that already exists (an existing
    /// scan's own `enabled_kinds` is a saved user choice, not a live
    /// mirror of this config).
    ///
    /// Default: every one of the 34 catalogue ids
    /// (`CandlePatternKind::all()`) — the exact set a scan with no
    /// enabled-kind config at all would already survey, so an untouched
    /// engine config changes nothing about what a freshly drawn scan
    /// shows.
    #[serde(default = "default_enabled_kinds")]
    pub default_enabled_kinds: Vec<String>,

    /// Whether a [`crate::drawing::primitives_v2::patterns::candle_pattern::CandlePattern`]
    /// claim reports its two guard rules
    /// (`"candle_pattern_window_incomplete"` / `"candle_pattern_not_formed"`)
    /// at all. Default `true` — the guard existing IS the point of the
    /// claim primitive (see the plan's own "the mark is a CLAIM the guard
    /// judges" framing); `false` is an explicit opt-out an analyst reaches
    /// for, not the starting posture, mirroring
    /// [`crate::state::ewa_engine::GuardFamilies`]'s own all-on default for
    /// the exact same reason.
    #[serde(default = "default_true")]
    pub judge_claims: bool,
}

fn default_true() -> bool {
    true
}

fn default_enabled_kinds() -> Vec<String> {
    CandlePatternKind::all().iter().map(|k| k.id().to_string()).collect()
}

impl Default for CandlePatternEngineConfig {
    fn default() -> Self {
        Self {
            default_enabled_kinds: default_enabled_kinds(),
            judge_claims: true,
        }
    }
}

static CANDLE_PATTERN_ENGINE_CONFIG: OnceLock<RwLock<CandlePatternEngineConfig>> = OnceLock::new();

/// Read the active candle-pattern engine configuration. Never fails — falls
/// back to [`CandlePatternEngineConfig::default`] on a poisoned lock (a
/// settings read must never crash a render frame), mirroring
/// [`crate::state::ewa_engine::ewa_engine_config`]'s own contract.
pub fn candle_pattern_engine_config() -> CandlePatternEngineConfig {
    CANDLE_PATTERN_ENGINE_CONFIG
        .get_or_init(|| RwLock::new(CandlePatternEngineConfig::default()))
        .read()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

/// Set the active candle-pattern engine configuration. Called once at
/// startup (profile load, `ChartApp::apply_profile_state`) — every reader
/// picks it up on the very next use because [`candle_pattern_engine_config`]
/// is read fresh, never cached, mirroring
/// [`crate::state::ewa_engine::set_ewa_engine_config`]'s own contract.
pub fn set_candle_pattern_engine_config(config: CandlePatternEngineConfig) {
    let lock = CANDLE_PATTERN_ENGINE_CONFIG.get_or_init(|| RwLock::new(CandlePatternEngineConfig::default()));
    if let Ok(mut guard) = lock.write() {
        *guard = config;
    }
}

/// Serializes every test in this crate that reads or writes the shared
/// [`CANDLE_PATTERN_ENGINE_CONFIG`] global — `cargo test`'s default
/// parallel runner otherwise interleaves two such tests on separate OS
/// threads, and a bare `RwLock` swap gives no isolation between them.
/// Exposure is real, not theoretical: this config is also read by
/// `CandlePattern::refresh_bar_derived_cache`/`CandlePatternScan::new` in
/// `drawing::primitives_v2::patterns`'s own test modules — every test
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
    fn default_config_enables_every_catalogue_pattern() {
        let cfg = CandlePatternEngineConfig::default();
        assert_eq!(cfg.default_enabled_kinds.len(), CandlePatternKind::all().len());
        for kind in CandlePatternKind::all() {
            assert!(cfg.default_enabled_kinds.contains(&kind.id().to_string()), "missing {}", kind.id());
        }
        assert!(cfg.judge_claims);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let restored: CandlePatternEngineConfig = serde_json::from_str("{}")
            .expect("empty object deserializes via field defaults");
        assert_eq!(restored, CandlePatternEngineConfig::default());
    }

    #[test]
    fn set_then_read_round_trips() {
        let _guard = engine_config_test_lock();
        let mut cfg = CandlePatternEngineConfig::default();
        cfg.judge_claims = false;
        cfg.default_enabled_kinds = vec!["doji".to_string()];
        set_candle_pattern_engine_config(cfg.clone());
        assert_eq!(candle_pattern_engine_config(), cfg);
        // Restore the default so other tests in this process observe the
        // documented starting state (the global is process-wide).
        set_candle_pattern_engine_config(CandlePatternEngineConfig::default());
    }
}
