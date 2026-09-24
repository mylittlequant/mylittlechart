//! EWA (Elliott Wave Analysis) engine configuration — one process-global,
//! user-level settings surface for the ENGINE itself, distinct from the
//! per-drawing settings every Elliott primitive already carries in its own
//! settings modal (owner ask 2026-08-14: "для настройки ЕВА движка можно
//! сделать отдельную настройку, ну вот прям модалку ЕВА движка" — per-
//! primitive settings already exist; the ENGINE had none).
//!
//! Same idiom as [`crate::drawing::primitives_v2::control_point_style`]:
//! read fresh on every use, never cached, mutated in place at pick time and
//! carried in the profile blob (`ProfileUiSettings::ewa_engine`) so it
//! survives a reload — see [`ewa_engine_config`]/[`set_ewa_engine_config`].
//!
//! Two of the fields below (`detect_higher_degree`, `show_projections`)
//! were knobs with no reader when this module was first written; both are
//! live now and both still start OFF, so an untouched config behaves
//! exactly as it did before either reader existed:
//! - `detect_higher_degree` gates the whole higher-cycle detector at
//!   creation time (`DrawingManager::adopt_higher_degree`, which sits
//!   behind one early return on this field) — the owner asked for
//!   recognition as an option, not a default;
//! - `show_projections` is the STARTING value of each Elliott structure's
//!   own per-drawing `show_projections` toggle, read once at construction
//!   the same way `ratio_rib_defaults` is, never retro-applied.

use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

use crate::drawing::primitives_v2::{LabelSeparationStyle, LineStyle};

// =============================================================================
// Ratio-rib defaults — applied at CONSTRUCTION time only
// =============================================================================

/// The ratio-rib colour/width/style a NEWLY CONSTRUCTED Elliott primitive
/// (`ElliottImpulse::new`/`ElliottCorrection::new`/`ElliottDoubleCombo::new`/
/// `ElliottTriangle::new`/`ElliottTripleCombo::new`) starts with — mirrors
/// the per-primitive `ratio_rib_color`/`ratio_rib_width`/`ratio_rib_style`
/// fields that already exist on each of those five structs. Read ONCE, at
/// construction, baked into the new primitive's own fields exactly the way
/// its base stroke colour is — never retro-applied to a primitive that
/// already exists (an existing drawing's own ratio-rib fields are a saved
/// user choice, not a live mirror of this config).
///
/// `color`/`width` default to `None` — "inherit from this structure's own
/// stroke colour/width" (`elliott::ratio::default_rib_color`/
/// `default_rib_width`), the EXACT fallback every rib used before these
/// fields existed. `style` defaults to [`LineStyle::Dotted`] — the fixed
/// literal every one of the five constructors already hardcoded. With
/// [`RatioRibDefaults::default`] unchanged, wiring the five constructors to
/// read this struct instead of their own hardcoded values changes nothing
/// about what a freshly drawn structure looks like.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RatioRibDefaults {
    /// `None` = inherit from the new primitive's own stroke colour.
    #[serde(default)]
    pub color: Option<String>,
    /// `None` = inherit from the new primitive's own stroke width.
    #[serde(default)]
    pub width: Option<f64>,
    #[serde(default = "default_ratio_rib_style")]
    pub style: LineStyle,
}

fn default_ratio_rib_style() -> LineStyle {
    LineStyle::Dotted
}

impl Default for RatioRibDefaults {
    fn default() -> Self {
        Self { color: None, width: None, style: LineStyle::Dotted }
    }
}

// =============================================================================
// Guard rule families — which classes of `elliott::guard` rule id the
// primitive-settings modal's own trailing "Rules" group is allowed to
// report. Gates EXISTING rule ids only — no new rule is invented here; see
// `elliott::guard::rule_family` for which id belongs to which family.
// =============================================================================

/// Which `elliott::guard` rule FAMILIES currently report a violation. All
/// three default ON — flipping one off is an explicit opt-out, not the
/// starting posture (the guard existed and reported everything before this
/// config did; this config must not silently go quiet on load).
///
/// - `geometry` — the point-position hard rules (wave-beyond-origin,
///   wave-overlap, wave-must-progress, wave-3-not-shortest, the B/C
///   retracement-vs-origin checks, the triangle contracting-boundaries
///   check, the wave-2/wave-4-must-retrace checks).
/// - `grammar` — the subdivision/typing rules (does the right KIND of
///   structure occupy a slot, does a subdivision carry the right DEGREE for
///   its slot, does a declared correction subtype actually match its
///   measured ratio, combo component/connector correctness, at most one
///   zigzag component, a triangle only in the final component).
/// - `chain` — the `follows`-chain-level rules (continuity, alternation,
///   shared degree).
///
/// See `elliott::guard::rule_family`'s own doc for the exact rule-id → family
/// table this struct's three switches gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardFamilies {
    #[serde(default = "default_true")]
    pub geometry: bool,
    #[serde(default = "default_true")]
    pub grammar: bool,
    #[serde(default = "default_true")]
    pub chain: bool,
}

fn default_true() -> bool {
    true
}

impl Default for GuardFamilies {
    fn default() -> Self {
        Self { geometry: true, grammar: true, chain: true }
    }
}

// =============================================================================
// EwaEngineConfig — the whole engine-level settings surface
// =============================================================================

/// One process-wide EWA engine configuration — the settings surface the
/// owner's "EWA engine settings" modal edits (owner ask 2026-08-14). RAM +
/// profile only, exactly like `ControlPointStyle`: there is no per-drawing
/// counterpart and no OK/Cancel diff to manage, because every field here
/// governs either how the engine draws/labels EVERY structure (read fresh,
/// every use) or what a NEWLY CONSTRUCTED structure starts with (read once,
/// at construction) — never a value copied onto an existing drawing.
///
/// `#[serde(default)]` on every field: a profile blob written before this
/// struct existed still loads, and lands on every default described below.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EwaEngineConfig {
    /// How two same-pivot wave labels separate once they collide — see
    /// [`LabelSeparationStyle`]'s own doc for the three styles and the
    /// geometry each implements. Default [`LabelSeparationStyle::Stacked`]
    /// — the ONLY behaviour that existed before this config did, so the
    /// default config changes nothing about how an existing chart renders.
    #[serde(default)]
    pub label_separation: LabelSeparationStyle,

    /// Recognize higher-degree cycles automatically. Default `false` — the
    /// owner asked for this as an OPTION, not a default. Read at creation
    /// time by `DrawingManager::finalize_new_primitive`, which runs the
    /// whole detector (`elliott::count::adoptable_children` +
    /// `DrawingManager::adopt_higher_degree`) behind one early return on
    /// this field, so an untouched config leaves creation untouched.
    #[serde(default)]
    pub detect_higher_degree: bool,

    /// Show fibonacci projections by default on a newly drawn structure.
    /// Default `false`. Read ONCE, at construction, by each Elliott type
    /// that has a projection table of its own (impulse, correction, both
    /// combos — not the triangle, see `elliott::projection`'s module doc),
    /// seeding that structure's own per-drawing `show_projections` toggle
    /// exactly the way `ratio_rib_defaults` seeds its rib style; never
    /// retro-applied to a structure that already exists.
    #[serde(default)]
    pub show_projections: bool,

    /// Which `elliott::guard` rule families currently report a violation —
    /// see [`GuardFamilies`]'s own doc. All three ON by default.
    #[serde(default)]
    pub guard_families: GuardFamilies,

    /// The ratio-rib colour/width/style a newly constructed Elliott
    /// primitive starts with — see [`RatioRibDefaults`]'s own doc. Never
    /// retro-applied to a primitive that already exists.
    #[serde(default)]
    pub ratio_rib_defaults: RatioRibDefaults,

    // -------------------------------------------------------------------
    // Autodetector A2 (2026-09-24) — `ElliottScan`'s own SEED-ONLY
    // defaults. `docs/mlc/plans/autodetectors-a2-a4-implementation-2026-09-24.md`,
    // "Architecture decision": these three fields exist so a freshly
    // constructed `ElliottScan` has somewhere to read its own starting
    // `swing_n`/`min_score`/`enabled_kinds` from ONCE, at construction
    // (`ElliottScan::new`, mirroring `CandlePatternEngineConfig::
    // default_enabled_kinds` → `CandlePatternScan::new()` exactly) — never
    // retro-applied to a scan that already exists, and never a settings-
    // modal row in A2 scope (same as `CandlePatternEngineConfig`'s own
    // precedent). `swing_n`/`min_score` stay genuinely PER-DRAWING fields
    // on `ElliottScan` after construction (arc §2's own explicit ruling);
    // this struct only seeds their starting value.
    // -------------------------------------------------------------------
    /// A freshly placed `ElliottScan`'s own starting pivot sensitivity —
    /// see `ElliottScan::swing_n`'s own doc. Default `2`, matching
    /// `SmcEngineConfig::swing_n`'s own default (Appendix A) — the same
    /// pivot-sensitivity reading this workspace already treats as a
    /// sensible starting point for a swing-based detector.
    #[serde(default = "default_scan_swing_n")]
    pub scan_default_swing_n: usize,
    /// A freshly placed `ElliottScan`'s own starting `min_score` post-
    /// filter. Default `0.0` ("Any" — no filtering), so an untouched
    /// engine config changes nothing about what a freshly drawn scan
    /// shows.
    #[serde(default)]
    pub scan_default_min_score: f64,
    /// A freshly placed `ElliottScan`'s own starting `enabled_kinds` — see
    /// `crate::drawing::primitives_v2::elliott::default_all_kind_ids`'s own
    /// doc. Default: all 8 catalogue ids.
    #[serde(default = "default_scan_enabled_kinds")]
    pub scan_default_enabled_kinds: Vec<String>,
}

fn default_scan_swing_n() -> usize {
    2
}

fn default_scan_enabled_kinds() -> Vec<String> {
    crate::drawing::primitives_v2::elliott::default_all_kind_ids()
}

impl Default for EwaEngineConfig {
    fn default() -> Self {
        Self {
            label_separation: LabelSeparationStyle::default(),
            detect_higher_degree: false,
            show_projections: false,
            guard_families: GuardFamilies::default(),
            ratio_rib_defaults: RatioRibDefaults::default(),
            scan_default_swing_n: default_scan_swing_n(),
            scan_default_min_score: 0.0,
            scan_default_enabled_kinds: default_scan_enabled_kinds(),
        }
    }
}

static EWA_ENGINE_CONFIG: OnceLock<RwLock<EwaEngineConfig>> = OnceLock::new();

/// Read the active EWA engine configuration. Never fails — falls back to
/// [`EwaEngineConfig::default`] on a poisoned lock (a settings read must
/// never crash a render frame), mirroring
/// [`crate::drawing::primitives_v2::control_point_style`]'s own contract.
pub fn ewa_engine_config() -> EwaEngineConfig {
    EWA_ENGINE_CONFIG
        .get_or_init(|| RwLock::new(EwaEngineConfig::default()))
        .read()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

/// Set the active EWA engine configuration. Called once at startup (profile
/// load, `ChartApp::apply_profile_state`) and again whenever the user edits
/// the EWA Engine Settings modal — every reader picks it up on the very next
/// use because [`ewa_engine_config`] is read fresh, never cached, mirroring
/// [`crate::drawing::primitives_v2::set_control_point_style`]'s own
/// contract.
pub fn set_ewa_engine_config(config: EwaEngineConfig) {
    let lock = EWA_ENGINE_CONFIG.get_or_init(|| RwLock::new(EwaEngineConfig::default()));
    if let Ok(mut guard) = lock.write() {
        *guard = config;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_matches_pre_existing_behaviour() {
        let cfg = EwaEngineConfig::default();
        assert_eq!(cfg.label_separation, LabelSeparationStyle::Stacked);
        assert!(!cfg.detect_higher_degree);
        assert!(!cfg.show_projections);
        assert!(cfg.guard_families.geometry);
        assert!(cfg.guard_families.grammar);
        assert!(cfg.guard_families.chain);
        assert_eq!(cfg.ratio_rib_defaults.color, None);
        assert_eq!(cfg.ratio_rib_defaults.width, None);
        assert_eq!(cfg.ratio_rib_defaults.style, LineStyle::Dotted);
        assert_eq!(cfg.scan_default_swing_n, 2);
        assert_eq!(cfg.scan_default_min_score, 0.0);
        assert_eq!(
            cfg.scan_default_enabled_kinds,
            crate::drawing::primitives_v2::elliott::default_all_kind_ids(),
        );
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let restored: EwaEngineConfig = serde_json::from_str("{}")
            .expect("empty object deserializes via field defaults");
        assert_eq!(restored, EwaEngineConfig::default());
    }

    #[test]
    fn set_then_read_round_trips() {
        let mut cfg = EwaEngineConfig::default();
        cfg.detect_higher_degree = true;
        cfg.guard_families.chain = false;
        cfg.scan_default_swing_n = 5;
        cfg.scan_default_min_score = 0.7;
        cfg.scan_default_enabled_kinds = vec!["impulse".to_string(), "zigzag".to_string()];
        set_ewa_engine_config(cfg.clone());
        assert_eq!(ewa_engine_config(), cfg);
        // Restore the default so other tests in this process observe the
        // documented starting state (the global is process-wide).
        set_ewa_engine_config(EwaEngineConfig::default());
    }
}
