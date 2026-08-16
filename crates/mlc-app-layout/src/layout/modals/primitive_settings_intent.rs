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
use mlc_core::drawing::primitives_v2::fibonacci::retracement::level_set_id_for_composition;
use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::drawing::{LineStyle, TimeframeVisibilityConfig};
use mlc_core::i18n::current_language;

use crate::layout::modals::primitive_settings_palettes::PALETTES;
use crate::ui::modal_settings::{PrimitiveSettingsState, TimeDisplayMode};

const EPS: f64 = 1e-9;

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
    /// `levels_palette:{i}` — recolour whichever axis/axes are currently in
    /// Палитра (see [`is_line_palette_mode`]/[`is_fill_palette_mode`]) from
    /// one of the four palette ramps. An axis sitting in Моно/"как линии"
    /// is left untouched — clicking a card while only the fill axis is on
    /// Палитра recolours bands and leaves the lines exactly alone.
    ApplyPalette(usize),
    /// `levels_line_mode:{mono,palette}` — the Levels tab's top-section
    /// "Линии" segmented control (2026-08-13 axis split of the old combined
    /// Моно/Палитра control): the LINE half. `true` = switching INTO
    /// Палитра: recolour every non-overridden level's own LINE colour
    /// (`cfg.color`) from the currently selected palette's ramp (same
    /// effect as [`ApplyPalette`] with `state.palette`, line half only).
    /// `false` = switching INTO Моно: clear every level's own `cfg.color`
    /// so the base colour (this tab's own "Base colour" row) is what every
    /// row's line draws in. Never touches `cfg.fill_color` — see
    /// [`SetFillColorMode`] for that independent axis. The mode itself is
    /// never stored — see [`is_line_palette_mode`], the same derivation the
    /// draw pass reads to decide which segment shows as active.
    SetLineColorMode(bool),
    /// `levels_fill_mode:{inherit,palette}` — the Levels tab's top-section
    /// "Заливка" segmented control (2026-08-13 axis split): the FILL half.
    /// `true` = switching INTO Палитра: recolour every non-overridden
    /// level's own BAND colour (`cfg.fill_color`) from the currently
    /// selected palette's ramp. `false` = switching INTO "как линии"
    /// (inherit): clear every level's own `cfg.fill_color` — `None` already
    /// means "take this level's own line colour" (the renderer falls back
    /// `fill_color` \u{2192} `color` \u{2192} base stroke), so there is
    /// nothing else to write for that state, no new field. Never touches
    /// `cfg.color` — see [`SetLineColorMode`] for that independent axis.
    /// Derived the same way, via [`is_fill_palette_mode`].
    SetFillColorMode(bool),
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
    if let Some(mode) = item_id.strip_prefix("levels_line_mode:") {
        return match mode {
            "mono" => Some(LevelsIntent::SetLineColorMode(false)),
            "palette" => Some(LevelsIntent::SetLineColorMode(true)),
            _ => None,
        };
    }
    if let Some(mode) = item_id.strip_prefix("levels_fill_mode:") {
        return match mode {
            "inherit" => Some(LevelsIntent::SetFillColorMode(false)),
            "palette" => Some(LevelsIntent::SetFillColorMode(true)),
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
        }
        LevelsIntent::SetStyle(level, style) => {
            set_level_config(configs, *level, |c| c.style = style.as_str().to_string());
            state.mark_level_overridden(*level);
        }
        LevelsIntent::AddValue(level) => add_level(configs, state, *level),
        LevelsIntent::StartValueEditing { .. } | LevelsIntent::StartAddEditing => {}
        LevelsIntent::ApplyPalette(idx) => {
            state.palette = *idx;
            // Read BOTH axes' current mode from `configs` BEFORE either
            // write below runs — the same data `apply_palette_lines`/
            // `apply_palette_fill` are about to mutate — so a card click
            // while only one axis is on Палитра recolours that axis alone
            // (see the intent's own doc).
            let recolor_lines = is_line_palette_mode(configs, state);
            let recolor_fill = is_fill_palette_mode(configs, state);
            if recolor_lines {
                apply_palette_lines(configs, state, *idx);
            }
            if recolor_fill {
                apply_palette_fill(configs, state, *idx);
            }
        }
        LevelsIntent::SetLineColorMode(to_palette) => {
            if *to_palette {
                apply_palette_lines(configs, state, state.palette);
            } else {
                // A level the user painted by hand is left alone, exactly as
                // `apply_palette_lines` leaves it alone: `is_level_overridden`
                // means that row's own style was edited deliberately, and a
                // global mode switch is not the place to discard it. Clears
                // ONLY `color` — `fill_color` is the other axis's own field,
                // see `SetFillColorMode`.
                for cfg in configs.iter_mut() {
                    if state.is_level_excluded(cfg.level) || state.is_level_overridden(cfg.level) {
                        continue;
                    }
                    cfg.color = None;
                }
            }
        }
        LevelsIntent::SetFillColorMode(to_palette) => {
            if *to_palette {
                apply_palette_fill(configs, state, state.palette);
            } else {
                // Clears ONLY `fill_color` — `None` already means "как
                // линии" (take this level's own line colour), so there is
                // nothing else to write. Same overridden/excluded guards as
                // `SetLineColorMode`.
                for cfg in configs.iter_mut() {
                    if state.is_level_excluded(cfg.level) || state.is_level_overridden(cfg.level) {
                        continue;
                    }
                    cfg.fill_color = None;
                }
            }
        }
    }
}

/// One набор `prim`'s own TYPE offers, as the Levels tab's own selector
/// would read it: the id [`Primitive::level_set_presets`] names, paired
/// with its localized display name for the CURRENT language — resolved
/// here (a preset's `name` field is a bare `fn(Language) -> &'static str`,
/// see [`mlc_core::drawing::primitives_v2::config::LevelSetPreset`]'s own
/// doc) so a caller gets a ready-to-draw string, not a resolver to call
/// again itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelSetOption {
    pub id: &'static str,
    pub name: &'static str,
}

/// Which набор(ы) `prim`'s own TYPE offers, and which one (if any) its
/// LIVE composition currently matches — "the modal asks the primitive",
/// routed through EXACTLY the pair `mlc-backend`'s `DrawingManager::
/// initial_style_reference` stamps `StyleReference::System` with:
/// [`Primitive::level_set_presets`] for the catalogue,
/// [`level_set_id_for_composition`] for the match, over the SAME
/// VISIBLE-only composition that function reads (`level_configs().filter
/// (|c| c.visible)`) — so the persisted creation-time stamp and this live
/// read can never independently redefine "what a preset means" and drift
/// apart. Every family that owns a preset catalogue seeds its FULL own
/// value catalogue at construction with only the "standard" subset marked
/// `visible` (see e.g. `gann::gann_fan::default_gann_fan_configs`) — an
/// excluded-only filter (ignoring `visible`) reads a fresh instance of any
/// of them as "all", not "standard"; visible-only is what actually agrees
/// with the backend's own stamp.
///
/// Replaces the hardcoded four-entry Fibonacci list this function used to
/// carry (`preset_values`/`matches_preset`, keyed off a closed `LevelSet`
/// enum): that list narrowed EVERY level-having type's own composition
/// toward Fibonacci ratios it never had, so a freshly placed Gann fan
/// (etc.) could never read as its own "Standard" here — exactly the
/// second, independently-drifting definition of "what a preset means"
/// the owning [`Primitive::level_set_presets`] doc warns against.
///
/// Empty options and `None` current for a primitive with no level ladder
/// at all (`level_set_presets()` empty) — never a fabricated
/// Fibonacci-shaped answer for a type that never had one.
pub fn level_set_options_and_current(prim: &dyn Primitive) -> (Vec<LevelSetOption>, Option<&'static str>) {
    let lang = current_language();
    let presets = prim.level_set_presets();
    let options: Vec<LevelSetOption> =
        presets.iter().map(|p| LevelSetOption { id: p.id, name: (p.name)(lang) }).collect();
    let Some(levels) = prim.level_configs() else { return (options, None) };
    let composition: Vec<f64> = levels.iter().filter(|c| c.visible).map(|c| c.level).collect();
    let current = level_set_id_for_composition(presets, &composition);
    (options, current)
}

/// Recolour every non-overridden level's own LINE colour (`cfg.color`) in
/// the (non-excluded) composition from `palette_idx`'s five stops, ranked
/// by ascending level value — the "Линии" axis's own half of what a single
/// combined `apply_palette` used to do to both fields at once (2026-08-13
/// axis split; see [`apply_palette_fill`] for the "Заливка" axis's half).
/// Palette changes never touch which levels exist — per the spec's "набор
/// и палитра — разные оси".
pub fn apply_palette_lines(configs: &mut [FibLevelConfig], state: &PrimitiveSettingsState, palette_idx: usize) {
    apply_palette_axis(configs, state, palette_idx, |cfg, color| cfg.color = Some(color));
}

/// Recolour every non-overridden level's own BAND colour (`cfg.fill_color`)
/// in the (non-excluded) composition — the "Заливка" axis's own half; see
/// [`apply_palette_lines`] for the "Линии" axis's half and for why the two
/// write different fields.
pub fn apply_palette_fill(configs: &mut [FibLevelConfig], state: &PrimitiveSettingsState, palette_idx: usize) {
    apply_palette_axis(configs, state, palette_idx, |cfg, color| cfg.fill_color = Some(color));
}

/// Shared ramp-ranking walk both axis-scoped palette writers use — `write`
/// is which single field ([`apply_palette_lines`]'s `cfg.color` or
/// [`apply_palette_fill`]'s `cfg.fill_color`) this call is scoped to; the
/// OTHER field on every `FibLevelConfig` is left completely untouched.
fn apply_palette_axis(
    configs: &mut [FibLevelConfig],
    state: &PrimitiveSettingsState,
    palette_idx: usize,
    write: impl Fn(&mut FibLevelConfig, String),
) {
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
        write(cfg, color);
    }
}

/// Whether the Levels tab's "Линии" axis (2026-08-13 axis split of the old
/// combined Моно/Палитра control) currently reads as Палитра (`true`) or
/// Моно (`false`) — DERIVED, never stored: Палитра exactly when any level
/// in the (non-excluded) composition has its own LINE colour set
/// (`cfg.color.is_some()`). The draw pass (`primitive_settings_levels`)
/// reads this same function to decide which segment shows as active, so
/// the control can never show a mode the underlying data does not actually
/// match. See [`is_fill_palette_mode`] for the independent "Заливка" axis.
pub fn is_line_palette_mode(configs: &[FibLevelConfig], state: &PrimitiveSettingsState) -> bool {
    configs
        .iter()
        .filter(|c| !state.is_level_excluded(c.level))
        .any(|c| c.color.is_some())
}

/// Whether the Levels tab's "Заливка" axis currently reads as Палитра —
/// same derivation as [`is_line_palette_mode`], keyed on
/// `cfg.fill_color.is_some()` instead. `fill_color: None` is "как линии"
/// (inherit the line's own colour) — never a stored inherit flag, see
/// `LevelsIntent::SetFillColorMode`'s own doc.
pub fn is_fill_palette_mode(configs: &[FibLevelConfig], state: &PrimitiveSettingsState) -> bool {
    configs
        .iter()
        .filter(|c| !state.is_level_excluded(c.level))
        .any(|c| c.fill_color.is_some())
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
/// Unlike [`apply_palette_lines`]/[`apply_palette_fill`], this does not consult
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

#[cfg(test)]
mod tests {
    use super::*;
    use mlc_core::drawing::primitives_v2::config::LevelSetPreset;
    use mlc_core::drawing::primitives_v2::fibonacci::retracement::{
        LEVEL_SET_PRESETS as FIB_LEVEL_SET_PRESETS, MAIN_LEVELS,
    };
    use mlc_core::drawing::primitives_v2::fibonacci::time_zones::LEVEL_SET_PRESETS as TIME_ZONES_LEVEL_SET_PRESETS;
    use mlc_core::drawing::primitives_v2::fibonacci::{FibRetracement, FibTimeZones};
    use mlc_core::drawing::primitives_v2::gann::gann_box::LEVEL_SET_PRESETS as GANN_BOX_LEVEL_SET_PRESETS;
    use mlc_core::drawing::primitives_v2::gann::gann_fan::LEVEL_SET_PRESETS as GANN_FAN_LEVEL_SET_PRESETS;
    use mlc_core::drawing::primitives_v2::gann::gann_square::LEVEL_SET_PRESETS as GANN_SQUARE_LEVEL_SET_PRESETS;
    use mlc_core::drawing::primitives_v2::gann::{GannBox, GannFan, GannSquare};
    use mlc_core::drawing::primitives_v2::lines::TrendLine;
    use mlc_core::drawing::primitives_v2::pitchforks::pitchfork::LEVEL_SET_PRESETS as PITCHFORK_LEVEL_SET_PRESETS;
    use mlc_core::drawing::primitives_v2::pitchforks::Pitchfork;

    /// `options` (the Levels tab's own set LIST for this primitive) must be
    /// exactly `presets` (`Primitive::level_set_presets()`'s own catalogue)
    /// — same ids, in the same order, each paired with that preset's own
    /// localized name — never a hardcoded stand-in list.
    fn assert_options_match_presets(options: &[LevelSetOption], presets: &'static [LevelSetPreset], label: &str) {
        let lang = current_language();
        assert_eq!(options.len(), presets.len(), "{label}: option count");
        for (opt, preset) in options.iter().zip(presets.iter()) {
            assert_eq!(opt.id, preset.id, "{label}: id");
            assert_eq!(opt.name, (preset.name)(lang), "{label}: name");
        }
    }

    /// A fibonacci retracement's Levels tab must keep listing exactly its
    /// four sets, with the same names, and still mark "standard" as current
    /// for a freshly placed instance — unchanged by this pass.
    #[test]
    fn fib_retracement_lists_its_four_sets_and_marks_standard_current() {
        let fib = FibRetracement::new(0, 100.0, 1_000, 200.0, "#F7B93E");
        let (options, current) = level_set_options_and_current(&fib);
        assert_eq!(options.len(), 4);
        assert_options_match_presets(&options, FIB_LEVEL_SET_PRESETS, "fib_retracement");
        assert_eq!(current, Some("standard"));
    }

    /// Hiding one of a fresh fib retracement's own levels (unchecking its
    /// row — `FibLevelConfig::visible = false`) must read as a custom
    /// composition, not silently keep matching "standard" — this is a LIVE
    /// read of the primitive's own current levels, not a one-time creation
    /// snapshot.
    #[test]
    fn hiding_a_level_reads_as_custom() {
        let mut fib = FibRetracement::new(0, 100.0, 1_000, 200.0, "#F7B93E");
        let mut configs = fib.level_configs().expect("fib retracement has a level ladder");
        let cfg = configs.iter_mut().find(|c| (c.level - MAIN_LEVELS[3]).abs() < 1e-9); // 0.5
        cfg.expect("0.5 is a member of MAIN_LEVELS").visible = false;
        fib.set_level_configs(configs);
        let (_, current) = level_set_options_and_current(&fib);
        assert_eq!(current, None);
    }

    /// A Gann fan now lists its OWN angle presets and marks its OWN
    /// "standard" as current when freshly placed — the exact defect this
    /// pass closes: the deleted hardcoded fib-only matcher would have
    /// compared this composition against `MAIN_LEVELS` and always landed
    /// on "Custom".
    #[test]
    fn gann_fan_lists_its_own_sets_and_marks_standard_current() {
        let gann = GannFan::new(0, 100.0, 1_000, 200.0, "#00ff00");
        let (options, current) = level_set_options_and_current(&gann);
        assert_options_match_presets(&options, GANN_FAN_LEVEL_SET_PRESETS, "gann_fan");
        assert_eq!(current, Some("standard"));
        // Regression guard: a Gann fan's own angles are not Fibonacci price
        // ratios — under the deleted matcher this composition could never
        // equal `MAIN_LEVELS`, which is exactly why it always read "Custom".
        for preset in GANN_FAN_LEVEL_SET_PRESETS {
            assert_ne!(preset.values, MAIN_LEVELS, "gann_fan must not borrow retracement's ratios");
        }
    }

    /// A Gann box lists its own fraction presets and marks its own
    /// "standard" as current when freshly placed.
    #[test]
    fn gann_box_lists_its_own_sets_and_marks_standard_current() {
        let gann = GannBox::new(0, 100.0, 1_000, 50.0, "#00ff00");
        let (options, current) = level_set_options_and_current(&gann);
        assert_options_match_presets(&options, GANN_BOX_LEVEL_SET_PRESETS, "gann_box");
        assert_eq!(current, Some("standard"));
    }

    /// A Gann square lists its own ring presets and marks its own
    /// "standard" as current when freshly placed.
    #[test]
    fn gann_square_lists_its_own_sets_and_marks_standard_current() {
        let sq = GannSquare::new(0, 100.0, 1_000, 50.0, "#00ff00");
        let (options, current) = level_set_options_and_current(&sq);
        assert_options_match_presets(&options, GANN_SQUARE_LEVEL_SET_PRESETS, "gann_square");
        assert_eq!(current, Some("standard"));
    }

    /// A pitchfork lists its own quarter/Fibonacci-ratio presets and marks
    /// its own "standard" as current when freshly placed.
    #[test]
    fn pitchfork_lists_its_own_sets_and_marks_standard_current() {
        let pf = Pitchfork::new(0, 100.0, 1_000_000, 150.0, 1_000_000, 50.0, "#F7B93E");
        let (options, current) = level_set_options_and_current(&pf);
        assert_options_match_presets(&options, PITCHFORK_LEVEL_SET_PRESETS, "pitchfork");
        assert_eq!(current, Some("standard"));
    }

    /// A Fibonacci Time Zones instance lists its own bar-count presets —
    /// never `retracement`'s price ratios, despite living in the same
    /// module — and marks its own "standard" as current when freshly
    /// placed.
    #[test]
    fn fib_time_zones_lists_its_own_sets_and_marks_standard_current() {
        let tz = FibTimeZones::new(0, 100.0, "#F7B93E");
        let (options, current) = level_set_options_and_current(&tz);
        assert_options_match_presets(&options, TIME_ZONES_LEVEL_SET_PRESETS, "fib_time_zones");
        assert_eq!(current, Some("standard"));
        for preset in TIME_ZONES_LEVEL_SET_PRESETS {
            assert_ne!(preset.values, MAIN_LEVELS, "time zones must not borrow retracement's ratios");
        }
    }

    /// A primitive with no level ladder at all shows no set list and no
    /// current id — never a fabricated (previously: empty Fibonacci) list.
    #[test]
    fn no_ladder_primitive_offers_no_sets() {
        let trend_line = TrendLine::new(0, 100.0, 1_000, 200.0, "#2962ff");
        let (options, current) = level_set_options_and_current(&trend_line);
        assert!(options.is_empty());
        assert_eq!(current, None);
    }
}

