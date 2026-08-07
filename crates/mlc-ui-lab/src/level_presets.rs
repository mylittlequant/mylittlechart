//! Canned Fibonacci level sets for the "Набор" selector
//! (`crate::approved_levels`).
//!
//! Fib-specific on purpose: the набор concept ("which ratios exist") only
//! makes sense for Fibonacci-style level primitives, this lab hosts
//! exactly one primitive type (`fib_retracement`, see `crate::lib`'s
//! `LabState::new`), and `mlc_core`'s own Fibonacci module already
//! publishes the two real value lists — no parallel catalogue invented
//! here.

use mlc_core::drawing::primitives_v2::config::FibLevelConfig;
use mlc_core::drawing::primitives_v2::fibonacci::retracement::{EXTENDED_LEVELS, MAIN_LEVELS};
use mlc_core::drawing::primitives_v2::Primitive;

use crate::design_state::LevelSet;

/// **Ambiguous in the approved spec** (`fib_modal.html`'s answer #1 names
/// "Только ключевые" but never lists its values). Chosen as the five
/// retracement levels most commonly singled out on their own — both
/// endpoints plus the golden zone. Needs the owner's sign-off; flagged
/// again in the implementation report.
const KEY_ONLY_LEVELS: &[f64] = &[0.0, 0.382, 0.5, 0.618, 1.0];

const EPS: f64 = 1e-9;

/// The value list a preset stands for. `Custom` has none — it is not a
/// generator, see [`LevelSet`].
pub fn preset_values(set: LevelSet) -> &'static [f64] {
    match set {
        LevelSet::Standard => MAIN_LEVELS,
        LevelSet::Extended => EXTENDED_LEVELS,
        LevelSet::KeyOnly => KEY_ONLY_LEVELS,
        LevelSet::Custom => &[],
    }
}

/// Does `composition` (the ladder's own value list, `DesignUiState::composition`)
/// match `set` exactly?
pub fn matches_preset(composition: &[f64], set: LevelSet) -> bool {
    let target = preset_values(set);
    if composition.len() != target.len() {
        return false;
    }
    target.iter().all(|v| composition.iter().any(|c| (*c - v).abs() < EPS))
}

/// The набор currently in effect: the first preset whose value set still
/// matches `composition`, or `Custom` if none does.
pub fn current_level_set(composition: &[f64]) -> LevelSet {
    LevelSet::PRESETS.into_iter().find(|s| matches_preset(composition, *s)).unwrap_or(LevelSet::Custom)
}

/// Replace `composition` wholesale with `set`'s canonical value list, and
/// make sure every one of those values has a real `FibLevelConfig` on
/// `prim` to draw from: an existing entry keeps its colour/width/style/
/// fill (hand-set or not) and is simply made visible; a value with no
/// existing entry gets a fresh one. Entries for values LEAVING the
/// composition are left exactly as they are — not deleted, not hidden —
/// they just stop being in the list this function returns, which is what
/// the spec asks for: "смена набора не сбрасывает ручные цвета тех строк,
/// что остались".
pub fn apply_level_set(prim: &mut dyn Primitive, composition: &mut Vec<f64>, set: LevelSet) {
    let target = preset_values(set);
    *composition = target.to_vec();

    let Some(mut configs) = prim.level_configs() else { return };
    let mut changed = false;
    for &level in target {
        if let Some(cfg) = configs.iter_mut().find(|c| (c.level - level).abs() < EPS) {
            if !cfg.visible {
                cfg.visible = true;
                changed = true;
            }
        } else {
            configs.push(FibLevelConfig::new(level));
            changed = true;
        }
    }
    if changed {
        prim.set_level_configs(configs);
    }
}

/// Add one value to the composition (a no-op if it is already there,
/// within tolerance). Ensures a real, visible `FibLevelConfig` exists for
/// it, reusing whatever is already stored at that value (e.g. a level
/// that was in a preset before, then removed) rather than resetting it.
pub fn add_level(prim: &mut dyn Primitive, composition: &mut Vec<f64>, level: f64) {
    if composition.iter().any(|c| (*c - level).abs() < EPS) {
        return;
    }
    composition.push(level);

    let Some(mut configs) = prim.level_configs() else { return };
    if let Some(cfg) = configs.iter_mut().find(|c| (c.level - level).abs() < EPS) {
        cfg.visible = true;
    } else {
        configs.push(FibLevelConfig::new(level));
    }
    prim.set_level_configs(configs);
}

/// Remove one value from the composition. The underlying entry is kept
/// (colours survive a re-add) but its `visible` is cleared so it does not
/// keep drawing on the chart after leaving the list it is edited from.
pub fn remove_level(prim: &mut dyn Primitive, composition: &mut Vec<f64>, level: f64) {
    composition.retain(|v| (*v - level).abs() >= EPS);

    let Some(mut configs) = prim.level_configs() else { return };
    if let Some(cfg) = configs.iter_mut().find(|c| (c.level - level).abs() < EPS) {
        cfg.visible = false;
        prim.set_level_configs(configs);
    }
}
