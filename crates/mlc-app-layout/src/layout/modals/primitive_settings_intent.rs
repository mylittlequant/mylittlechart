//! Pure `item_id -> intent` half of the primitive-settings modal's click
//! dispatch for the ids the approved 3-tab design introduces (`level_v*`,
//! `levels_*`, `tf_{class}_*`, `placement_time_mode:*` — the pre-port ids
//! `stroke_color`/`style_prop:*`/`text_*`/`coord_*`/`template_*`/
//! `line_style` are unchanged by the port and stay wherever they already
//! lived, on both hosts).
//!
//! Two independent hosts drive the same modal against two different
//! storage models: `mlc-app` (`ChartApp`) resolves a primitive through a
//! `BubbleId`-scoped `drawings_mut()` facade whose only writes are
//! plain-data setters (`set_level_configs(bid, id, Vec<FibLevelConfig>)`,
//! `set_data(bid, id, &PrimitiveData)`) — it never hands out a raw
//! `&mut dyn Primitive` — and follows every write with cross-window sync +
//! autosave; `mlc-ui-lab` holds one bare `Vec<Box<dyn Primitive>>` in
//! memory and calls the same `Primitive` trait setters directly. Before
//! this module existed those two re-derived the same "what does clicking
//! `level_v0.618_delete` mean" logic independently (`mlc-ui-lab/src/
//! dispatch.rs`'s `apply_approved_design_click` vs. hand-written blocks in
//! `mlc-app/src/input/settings.rs`) — they were always going to drift.
//!
//! Operating on plain data (`&mut Vec<FibLevelConfig>`,
//! `&mut TimeframeVisibilityConfig`) rather than `&mut dyn Primitive` is
//! what makes this actually shared: `mlc-app` can only ever offer the
//! former (see above), so a signature shaped around the latter would have
//! been unusable on the one side that most needed the code shared. Each
//! host reads its own copy out (`Primitive::level_configs()` for the lab,
//! `drawings().get_data()`+a `primitives()` read for the app), calls in
//! here, then writes the result back through its own setter.
//!
//! This crate is where it lives because it is the one place BOTH hosts
//! already depend on and that depends on NEITHER of them: `mlc-app-layout`
//! sits below `mlc-app` (`ChartApp`'s own kernel) and below `mlc-ui-lab`
//! (a leaf wasm-only crate that exists specifically to drive
//! `mlc-app-layout`'s real widget builders) in the crate graph, so putting
//! it here keeps the dependency arrow one-way — this module can never end
//! up depending back on either caller.
//!
//! `parse_*` is pure. `apply_*` mutates `state` (session-only Levels/
//! Placement bookkeeping) and the plain data passed in. What deliberately
//! stays OUT of this module: `active_field_id`/`InputCoordinator` text
//! focus (the two hosts do not share one `InputCoordinator` instance, so
//! `Start*Editing` intents carry the field id + seed text and the CALLER
//! focuses its own), and anything BubbleId/persistence-shaped
//! (`sync_drawing_back_to_group`, `autosave_snapshot`,
//! `snapshot_primitive_settings_to_user_manager` — real only on the
//! `mlc-app` side, meaningless in a lab with no data plane).

use mlc_core::drawing::primitives_v2::config::FibLevelConfig;
use mlc_core::drawing::primitives_v2::fibonacci::retracement::{
    ALL_LEVELS, EXTENDED_LEVELS, KEY_ONLY_LEVELS, MAIN_LEVELS,
};
use mlc_core::drawing::{LineStyle, TimeframeVisibilityConfig};

use crate::layout::modals::primitive_settings_palettes::PALETTES;
use crate::ui::modal_settings::{LevelSet, PrimitiveSettingsState, TimeDisplayMode};

const EPS: f64 = 1e-9;

/// The value list a preset stands for. `Custom` has none — it is not a
/// generator, see [`LevelSet`]. Sourced from
/// `mlc_core::drawing::primitives_v2::fibonacci::retracement` — the single
/// canonical home for these four lists (`KEY_ONLY_LEVELS`'s own "needs
/// owner sign-off" flag lives there now too) — so this crate's own
/// `current_level_set` (the Levels tab's own "which набор is this"
/// display), `mlc-backend`'s creation-time `StyleReference::System`
/// stamping, and `mlc_core::templates::PrimitiveTemplate::system_templates`
/// (the Template menu's system rows) can never read two different
/// definitions of the same preset.
fn preset_values(set: LevelSet) -> &'static [f64] {
    match set {
        LevelSet::Standard => MAIN_LEVELS,
        LevelSet::Extended => EXTENDED_LEVELS,
        LevelSet::KeyOnly => KEY_ONLY_LEVELS,
        LevelSet::All => ALL_LEVELS,
        LevelSet::Custom => &[],
    }
}

// =============================================================================
// Уровни
// =============================================================================

/// What a click on one of the Levels tab's own ids means. Ids not owned by
/// this tab (`level_v{key}_color`/`level_v{key}_fill_color`, which open the
/// colour picker through the same generic path baseline's own swatches
/// use) are not recognised here — see `parse_levels_click`. The ROW's own
/// second chip, `level_v{key}_fill`, IS owned here (`ToggleFillEnabled`):
/// it used to open the fill colour picker too, before the fill mechanic
/// was inverted (2026-08-08 follow-up) so the row chip is on/off and the
/// colour lives only in the inspector's own `level_v{key}_fill_color`.
#[derive(Debug, Clone, PartialEq)]
pub enum LevelsIntent {
    /// `levels_row_select:{key}` — expand/collapse that row's inspector.
    ToggleSelected(f64),
    /// `level_v{key}_visible` — the checkbox: dims the row, never removes
    /// it (composition \u{2260} visibility, see
    /// `PrimitiveSettingsState::levels_excluded`'s doc comment).
    ToggleVisible(f64),
    /// `level_v{key}_fill` — the ladder row's own second chip: on/off for
    /// that level's band (2026-08-08 follow-up — used to be the
    /// inspector's own separate checkbox, `level_v{key}_fill_toggle`, now
    /// removed; the row chip already rendered solid-when-on/hollow-when-off,
    /// so it took over the click too, and the inspector keeps only the
    /// fill colour, `level_v{key}_fill_color`).
    ToggleFillEnabled(f64),
    /// `level_v{key}_delete` — "\u{d7}": excludes the value from the
    /// ladder view AND hides it on the chart; the underlying
    /// `FibLevelConfig` entry (colour/width/style) is left exactly as is.
    Delete(f64),
    /// `level_v{key}_style:{id}` — direct-select line style for one level.
    SetStyle(f64, LineStyle),
    /// `levels_add_chip:{v}` — add one of the quick-add values.
    AddValue(f64),
    /// `level_v{key}_value` — start editing that row's own value field;
    /// the caller focuses its own text input with `seed`.
    StartValueEditing { level: f64, seed: String },
    /// `levels_add_value` — start editing the "+ добавить уровень" field.
    StartAddEditing,
    /// `levels_palette:{i}` — recolour every non-overridden level in the
    /// composition from one of the four palette ramps.
    ApplyPalette(usize),
    /// `levels_color_mode:{mono,palette}` — the Levels tab's top-section
    /// Моно/Палитра segmented control (2026-08-08 appearance-surface
    /// consolidation). `true` = switching INTO Палитра: apply the
    /// currently selected palette's ramp to the composition (same effect
    /// as [`ApplyPalette`] with `state.palette`). `false` = switching INTO
    /// Моно: clear every level's own colour so the base colour (Вид's old
    /// `stroke_color`, now this tab's own "Base colour" row) is what every
    /// row draws in. The mode itself is never stored — see
    /// [`is_palette_mode`], the same derivation the draw pass reads to
    /// decide which segment shows as active.
    SetColorMode(bool),
}

/// Parse one of the Levels tab's own ids.
pub fn parse_levels_click(item_id: &str) -> Option<LevelsIntent> {
    if let Some(key) = item_id.strip_prefix("levels_row_select:") {
        return key.parse::<f64>().ok().map(LevelsIntent::ToggleSelected);
    }
    if let Some(v) = item_id.strip_prefix("levels_add_chip:") {
        return v.parse::<f64>().ok().map(LevelsIntent::AddValue);
    }
    if item_id == "levels_add_value" {
        return Some(LevelsIntent::StartAddEditing);
    }
    if let Some(idx_str) = item_id.strip_prefix("levels_palette:") {
        return idx_str.parse::<usize>().ok().map(LevelsIntent::ApplyPalette);
    }
    if let Some(mode) = item_id.strip_prefix("levels_color_mode:") {
        return match mode {
            "mono" => Some(LevelsIntent::SetColorMode(false)),
            "palette" => Some(LevelsIntent::SetColorMode(true)),
            _ => None,
        };
    }
    let rest = item_id.strip_prefix("level_v")?;
    let (key, suffix) = rest.split_once('_')?;
    let level = key.parse::<f64>().ok()?;
    match suffix {
        "visible" => Some(LevelsIntent::ToggleVisible(level)),
        "fill" => Some(LevelsIntent::ToggleFillEnabled(level)),
        "delete" => Some(LevelsIntent::Delete(level)),
        "value" => Some(LevelsIntent::StartValueEditing { level, seed: format!("{level:.3}") }),
        _ if suffix.starts_with("style:") => {
            let style_id = &suffix["style:".len()..];
            Some(LevelsIntent::SetStyle(level, LineStyle::from_str(style_id)))
        }
        _ => None,
    }
}

/// Apply a parsed intent to `state` (session-only Levels-tab bookkeeping)
/// and `configs` (the primitive's own `level_configs()`, read by the
/// caller before this call and written back after it — this function
/// always mutates `configs` in place, even when the net effect is a
/// no-op, so the caller never needs to track "did anything change").
/// `Start*Editing` intents leave `configs` untouched — see the module doc
/// for why.
pub fn apply_levels_intent(intent: &LevelsIntent, state: &mut PrimitiveSettingsState, configs: &mut Vec<FibLevelConfig>) {
    match intent {
        LevelsIntent::ToggleSelected(level) => {
            state.selected_level = if state.is_level_selected(*level) { None } else { Some(*level) };
        }
        LevelsIntent::ToggleVisible(level) => set_level_config(configs, *level, |c| c.visible = !c.visible),
        LevelsIntent::ToggleFillEnabled(level) => set_level_config(configs, *level, |c| c.fill_enabled = !c.fill_enabled),
        LevelsIntent::Delete(level) => {
            state.exclude_level(*level);
            state.clear_level_overridden(*level);
            if state.is_level_selected(*level) {
                state.selected_level = None;
            }
            set_level_config(configs, *level, |c| c.visible = false);
            state.level_set = current_level_set(configs, state);
        }
        LevelsIntent::SetStyle(level, style) => {
            set_level_config(configs, *level, |c| c.style = style.as_str().to_string());
            state.mark_level_overridden(*level);
        }
        LevelsIntent::AddValue(level) => add_level(configs, state, *level),
        LevelsIntent::StartValueEditing { .. } | LevelsIntent::StartAddEditing => {}
        LevelsIntent::ApplyPalette(idx) => {
            state.palette = *idx;
            apply_palette(configs, state, *idx);
        }
        LevelsIntent::SetColorMode(to_palette) => {
            if *to_palette {
                apply_palette(configs, state, state.palette);
            } else {
                // Clear BOTH colours the palette wrote. Clearing only
                // `color` left every band still painted in the palette's own
                // per-level colour — the renderer reads `fill_color` first
                // and only falls back to `color` — so one Палитра→Моно cycle
                // produced mono LINES over palette BANDS, permanently, with
                // no control anywhere that could undo it.
                //
                // A level the user painted by hand is left alone, exactly as
                // `apply_palette` leaves it alone: `is_level_overridden`
                // means that row's own style was edited deliberately, and a
                // global mode switch is not the place to discard it.
                for cfg in configs.iter_mut() {
                    if state.is_level_excluded(cfg.level) || state.is_level_overridden(cfg.level) {
                        continue;
                    }
                    cfg.color = None;
                    cfg.fill_color = None;
                }
            }
        }
    }
}

fn matches_preset(composition: &[f64], set: LevelSet) -> bool {
    let target = preset_values(set);
    if composition.len() != target.len() {
        return false;
    }
    target.iter().all(|v| composition.iter().any(|c| (*c - v).abs() < EPS))
}

/// The набор currently in effect: the first preset whose value set still
/// matches the (non-excluded) composition, or `Custom` if none does.
pub fn current_level_set(configs: &[FibLevelConfig], state: &PrimitiveSettingsState) -> LevelSet {
    let composition: Vec<f64> =
        configs.iter().map(|c| c.level).filter(|level| !state.is_level_excluded(*level)).collect();
    LevelSet::PRESETS.into_iter().find(|s| matches_preset(&composition, *s)).unwrap_or(LevelSet::Custom)
}

/// Recolour every non-overridden level in the (non-excluded) composition
/// from `palette_idx`'s five stops, ranked by ascending level value.
/// Palette changes never touch which levels exist — per the spec's "набор
/// и палитра — разные оси".
pub fn apply_palette(configs: &mut [FibLevelConfig], state: &PrimitiveSettingsState, palette_idx: usize) {
    let palette = &PALETTES[palette_idx.min(PALETTES.len().saturating_sub(1))];
    let mut sorted_vals: Vec<f64> =
        configs.iter().map(|c| c.level).filter(|level| !state.is_level_excluded(*level)).collect();
    sorted_vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let denom = sorted_vals.len().saturating_sub(1).max(1) as f64;
    for cfg in configs.iter_mut() {
        let Some(rank) = sorted_vals.iter().position(|v| (*v - cfg.level).abs() < EPS) else { continue };
        if state.is_level_overridden(cfg.level) {
            continue;
        }
        let t = rank as f64 / denom;
        let color = crate::layout::modals::primitive_settings_palettes::palette_color_at(&palette.colors, t);
        cfg.color = Some(color.clone());
        cfg.fill_color = Some(color);
    }
}

/// Whether the Levels tab's top-section colour-mode control currently reads
/// as Палитра (`true`) or Моно (`false`) — DERIVED, never stored (2026-08-08
/// appearance-surface consolidation): Палитра exactly when any level in the
/// (non-excluded) composition has its own colour set. The draw pass
/// (`primitive_settings_levels`) reads this same function to decide which
/// segment shows as active, so the control can never show a mode the
/// underlying data does not actually match.
pub fn is_palette_mode(configs: &[FibLevelConfig], state: &PrimitiveSettingsState) -> bool {
    configs
        .iter()
        .filter(|c| !state.is_level_excluded(c.level))
        .any(|c| c.color.is_some())
}

/// Write `width` into every level's own `cfg.width` in the (non-excluded)
/// composition (item 3, 2026-08-09) — the global Width control's own
/// writer. Called directly by each host's `"stroke_width"` handler
/// (`mlc-app`'s `input/sliders.rs` drag-commit + `input/settings.rs` typed-
/// value commit, `mlc-ui-lab`'s `dispatch.rs` mirror of both) alongside its
/// existing `PrimitiveData.width` write — never through `LevelsIntent`/
/// `parse_levels_click`: `"stroke_width"` is a bare id shared with every
/// non-level primitive's own Appearance tab, exactly like `"stroke_color"`/
/// `"line_style"` (see `field_id()` in `primitive_settings_property`), the
/// same reason the global fill toggle/opacity row also bypasses
/// `LevelsIntent` and calls `apply_style_property` directly.
///
/// Unlike [`apply_palette`], this does not consult
/// `state.is_level_overridden` (that flag tracks a level's own per-row
/// STYLE edit specifically, a narrower, unrelated axis) — the global
/// control always overwrites every level currently shown; a per-row width
/// edit made through the inspector's own slider afterwards still applies
/// normally, right up until the global control is touched again.
pub fn apply_global_width(configs: &mut [FibLevelConfig], state: &PrimitiveSettingsState, width: f64) {
    for cfg in configs.iter_mut() {
        if state.is_level_excluded(cfg.level) {
            continue;
        }
        cfg.width = Some(width);
    }
}

/// Write `style` into every level's own `cfg.style` in the (non-excluded)
/// composition (item 3, 2026-08-09) — the global Style control's own
/// writer, called the same way [`apply_global_width`] is (see its own doc
/// for why this is a plain function, not a `LevelsIntent`, and why it does
/// not consult `is_level_overridden`). `FibLevelConfig::style` is a plain
/// always-populated `String`, never `Option` — unlike width/colour it has
/// no fallback to fall back to, so before this writer existed the global
/// Style control changed nothing any level's own render ever read.
pub fn apply_global_style(configs: &mut [FibLevelConfig], state: &PrimitiveSettingsState, style: LineStyle) {
    let style_str = style.as_str().to_string();
    for cfg in configs.iter_mut() {
        if state.is_level_excluded(cfg.level) {
            continue;
        }
        cfg.style = style_str.clone();
    }
}

fn set_level_config(configs: &mut [FibLevelConfig], level: f64, f: impl FnOnce(&mut FibLevelConfig)) {
    let Some(cfg) = configs.iter_mut().find(|c| (c.level - level).abs() < EPS) else { return };
    f(cfg);
}

/// Ensure a real, visible `FibLevelConfig` exists at `level` in `configs` —
/// reusing whatever is already stored there (a value that was excluded,
/// then re-added, keeps its colour) — and drop its exclusion in `state` if
/// it had one.
pub fn add_level(configs: &mut Vec<FibLevelConfig>, state: &mut PrimitiveSettingsState, level: f64) {
    state.unexclude_level(level);
    if let Some(cfg) = configs.iter_mut().find(|c| (c.level - level).abs() < EPS) {
        cfg.visible = true;
    } else {
        configs.push(FibLevelConfig::new(level));
    }
    state.level_set = current_level_set(configs, state);
}

/// Change a level's own VALUE — the ladder's "double-click to edit"
/// affordance, committed from the `level_v{old}_value` text field.
/// Silently rejected if the new value already exists in the (non-excluded)
/// composition — a small deliberate simplification, flagged in the port
/// report. `state`'s value-keyed bookkeeping moves to the new value so
/// nothing keyed by the old one goes stale.
pub fn rename_level(configs: &mut [FibLevelConfig], state: &mut PrimitiveSettingsState, old_level: f64, new_level: f64) {
    if (old_level - new_level).abs() < EPS {
        return;
    }
    let collides = configs.iter().any(|c| (c.level - new_level).abs() < EPS && !state.is_level_excluded(c.level));
    if collides {
        return;
    }
    let Some(cfg) = configs.iter_mut().find(|c| (c.level - old_level).abs() < EPS) else { return };
    cfg.level = new_level;

    if state.is_level_excluded(old_level) {
        state.unexclude_level(old_level);
        state.exclude_level(new_level);
    }
    if state.is_level_overridden(old_level) {
        state.clear_level_overridden(old_level);
        state.mark_level_overridden(new_level);
    }
    if state.is_level_selected(old_level) {
        state.selected_level = Some(new_level);
    }
    state.level_set = current_level_set(configs, state);
}

// =============================================================================
// Размещение
// =============================================================================

/// What a click on one of the Placement tab's own ids means.
#[derive(Debug, Clone, PartialEq)]
pub enum PlacementIntent {
    SetTimeMode(TimeDisplayMode),
    /// `tf_{class}_toggle` — `class` is one of `seconds`/`minutes`/`hours`/
    /// `days`/`weeks`/`months`/`ranges`.
    ToggleTfClass(String),
    /// `tf_{class}_min`/`tf_{class}_max` — start editing that bound; the
    /// caller focuses its own text input with `seed`.
    StartTfBoundEditing { field_id: String, seed: String },
}

/// Parse one of the Placement tab's own ids. `tf` is read only, to seed
/// `StartTfBoundEditing` with the field's current value.
pub fn parse_placement_click(item_id: &str, tf: &TimeframeVisibilityConfig) -> Option<PlacementIntent> {
    if item_id == "placement_time_mode:time" {
        return Some(PlacementIntent::SetTimeMode(TimeDisplayMode::Time));
    }
    if item_id == "placement_time_mode:bar_index" {
        return Some(PlacementIntent::SetTimeMode(TimeDisplayMode::BarIndex));
    }
    if let Some(id) = item_id.strip_prefix("tf_").and_then(|s| s.strip_suffix("_toggle")) {
        return Some(PlacementIntent::ToggleTfClass(id.to_string()));
    }
    if let Some(id) = item_id.strip_prefix("tf_").and_then(|s| s.strip_suffix("_min")) {
        let current = tf_bound(tf, id).map(|(min, _)| min).unwrap_or(1);
        return Some(PlacementIntent::StartTfBoundEditing { field_id: item_id.to_string(), seed: current.to_string() });
    }
    if let Some(id) = item_id.strip_prefix("tf_").and_then(|s| s.strip_suffix("_max")) {
        let current = tf_bound(tf, id).map(|(_, max)| max).unwrap_or(1);
        return Some(PlacementIntent::StartTfBoundEditing { field_id: item_id.to_string(), seed: current.to_string() });
    }
    None
}

/// Apply a parsed intent to `state` (the время/индекс-бара toggle) and
/// `tf`. Returns `true` if `tf` was modified — `SetTimeMode` and
/// `StartTfBoundEditing` only ever touch `state`, so the caller can skip
/// writing `tf` back for those.
pub fn apply_placement_intent(intent: &PlacementIntent, state: &mut PrimitiveSettingsState, tf: &mut TimeframeVisibilityConfig) -> bool {
    match intent {
        PlacementIntent::SetTimeMode(mode) => {
            state.time_display_mode = *mode;
            false
        }
        PlacementIntent::ToggleTfClass(id) => {
            toggle_tf_class(tf, id);
            true
        }
        PlacementIntent::StartTfBoundEditing { .. } => false,
    }
}

/// Commit a typed `tf_{class}_min`/`tf_{class}_max` value — called from
/// each host's own "commit active field" path.
pub fn apply_tf_bound(tf: &mut TimeframeVisibilityConfig, field_id: &str, value: u32) {
    let is_min = field_id.ends_with("_min");
    let Some(class_id) = field_id
        .strip_prefix("tf_")
        .and_then(|s| if is_min { s.strip_suffix("_min") } else { s.strip_suffix("_max") })
    else {
        return;
    };
    let Some(slot) = tf_slot_mut(tf, class_id) else { return };
    if let Some((min, max)) = slot {
        if is_min {
            *min = value.min(*max).max(1);
        } else {
            *max = value.max(*min);
        }
    }
}

fn tf_bound(tf: &TimeframeVisibilityConfig, class_id: &str) -> Option<(u32, u32)> {
    match class_id {
        "seconds" => tf.seconds,
        "minutes" => tf.minutes,
        "hours" => tf.hours,
        "days" => tf.days,
        "weeks" => tf.weeks,
        "months" => tf.months,
        _ => None,
    }
}

fn tf_slot_mut<'a>(tf: &'a mut TimeframeVisibilityConfig, class_id: &str) -> Option<&'a mut Option<(u32, u32)>> {
    match class_id {
        "seconds" => Some(&mut tf.seconds),
        "minutes" => Some(&mut tf.minutes),
        "hours" => Some(&mut tf.hours),
        "days" => Some(&mut tf.days),
        "weeks" => Some(&mut tf.weeks),
        "months" => Some(&mut tf.months),
        _ => None,
    }
}

fn tf_class_default_range(class_id: &str) -> (u32, u32) {
    match class_id {
        "seconds" | "minutes" => (1, 59),
        "hours" => (1, 24),
        "days" => (1, 366),
        "weeks" => (1, 52),
        "months" => (1, 12),
        _ => (1, 1),
    }
}

fn toggle_tf_class(tf: &mut TimeframeVisibilityConfig, class_id: &str) {
    if class_id == "ranges" {
        tf.ranges = !tf.ranges;
        return;
    }
    let default_range = tf_class_default_range(class_id);
    let Some(slot) = tf_slot_mut(tf, class_id) else { return };
    *slot = if slot.is_some() { None } else { Some(default_range) };
}

