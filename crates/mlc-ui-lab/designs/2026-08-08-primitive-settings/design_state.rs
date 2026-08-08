//! Per-sector UI state for the approved 3-tab design (`Вид` · `Уровни` ·
//! `Размещение`, `crate::approved_design`). Baseline and the templates
//! panel (sector C) never touch it.
//!
//! `PrimitiveSettingsState` (mlc-app-layout, out of this crate's scope)
//! has no home for any of this — which of the approved design's own three
//! tabs is open, which canned level set is active, which level's inspector
//! is expanded, which levels were coloured by hand — because none of it
//! existed before this design. One [`DesignUiState`] is kept per grid
//! sector.

/// Which of the approved design's own three tabs is showing. Independent
/// of `PrimitiveSettingsState::active_tab` (baseline's five real tabs) —
/// the approved design never delegates through that enum (unlike the
/// earlier four-sector comparison lab), so it needs its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomTab {
    Vid,
    Levels,
    Placement,
}

impl CustomTab {
    pub const ALL: [CustomTab; 3] = [CustomTab::Vid, CustomTab::Levels, CustomTab::Placement];

    pub fn id(self) -> &'static str {
        match self {
            CustomTab::Vid => "vid",
            CustomTab::Levels => "levels",
            CustomTab::Placement => "placement",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CustomTab::Vid => "Вид",
            CustomTab::Levels => "Уровни",
            CustomTab::Placement => "Размещение",
        }
    }
}

/// Which canned level set is active. "Стандартный"/"Расширенный"/"Только
/// ключевые" name a fixed value list (`crate::level_presets`); `Custom`
/// is not a generator — it is what the selector shows once the level
/// VALUES on the primitive (not their colours) no longer match any of the
/// three, per the approved spec: "как только тронул что-то руками, набор
/// становится «Свой»".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LevelSet {
    Standard,
    Extended,
    KeyOnly,
    Custom,
}

impl LevelSet {
    pub const PRESETS: [LevelSet; 3] = [LevelSet::Standard, LevelSet::Extended, LevelSet::KeyOnly];

    pub fn label(self) -> &'static str {
        match self {
            LevelSet::Standard => "Стандартный",
            LevelSet::Extended => "Расширенный",
            LevelSet::KeyOnly => "Только ключевые",
            LevelSet::Custom => "Свой",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            LevelSet::Standard => "standard",
            LevelSet::Extended => "extended",
            LevelSet::KeyOnly => "key_only",
            LevelSet::Custom => "custom",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::PRESETS.into_iter().find(|s| s.id() == id)
    }
}

/// "время" (a formatted date/time, real) vs "индекс бара" (needs the
/// chart's own bar/timeframe data to convert a millisecond timestamp to a
/// bar index — this lab has no chart, no bar array, no bar spacing; see
/// `crate::approved_placement`). Kept as a real toggle anyway so the inert
/// side is visibly present, not silently dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeDisplayMode {
    Time,
    BarIndex,
}

/// One of these per grid sector. Only `crate::approved_design` reads or
/// writes it.
pub struct DesignUiState {
    pub custom_tab: CustomTab,
    /// Whether the "Набор" select is open.
    pub set_dropdown_open: bool,
    /// The level VALUES currently in the ladder — "the list is what is
    /// really enabled, not a catalogue" (the approved spec's own framing).
    /// Independent of each entry's own `FibLevelConfig::visible` (the
    /// checkbox: dims a row, keeps it in the list) — this is what leaves
    /// the list altogether: набор replaces it wholesale, "×" removes one
    /// value, "+ добавить уровень" appends one. The underlying
    /// `Primitive::level_configs()` stays a reservoir of every value ever
    /// touched (so colours survive leaving and re-entering the list); this
    /// is the view over it.
    pub composition: Vec<f64>,
    pub level_set: LevelSet,
    /// The last explicitly chosen (non-`Custom`) preset — what "вернуть"
    /// restores.
    pub last_preset: LevelSet,
    /// Index into [`crate::palettes::PALETTES`] for the chosen preset.
    pub palette: usize,
    /// Level VALUES marked "touched by hand" (colour only — composition
    /// changes are what flip `level_set` to `Custom`, not this). Keyed by
    /// value rather than position because the ladder is now a filtered,
    /// sorted VIEW over the primitive's own (unsorted, partly-hidden)
    /// level list — a positional index would point at a different level
    /// the moment the набор or the sort order changes it.
    pub overridden: Vec<f64>,
    pub global_fill_opacity: f64,
    /// Level value whose inspector is expanded, if any — value-keyed for
    /// the same reason `overridden` is.
    pub selected: Option<f64>,
    pub time_display_mode: TimeDisplayMode,
}

impl Default for DesignUiState {
    fn default() -> Self {
        Self {
            custom_tab: CustomTab::Levels,
            set_dropdown_open: false,
            composition: crate::level_presets::preset_values(LevelSet::Standard).to_vec(),
            level_set: LevelSet::Standard,
            last_preset: LevelSet::Standard,
            palette: 0,
            overridden: Vec::new(),
            global_fill_opacity: 0.14,
            selected: None,
            time_display_mode: TimeDisplayMode::Time,
        }
    }
}

const EPS: f64 = 1e-9;

impl DesignUiState {
    pub fn is_overridden(&self, level: f64) -> bool {
        self.overridden.iter().any(|v| (*v - level).abs() < EPS)
    }

    pub fn mark_overridden(&mut self, level: f64) {
        if !self.is_overridden(level) {
            self.overridden.push(level);
        }
    }

    /// Drop a value's override flag — called when that level is deleted,
    /// so a re-added level at the same value starts clean rather than
    /// inheriting a stale "touched by hand" mark from a row that no
    /// longer exists.
    pub fn clear_overridden(&mut self, level: f64) {
        self.overridden.retain(|v| (*v - level).abs() >= EPS);
    }

    pub fn is_selected(&self, level: f64) -> bool {
        self.selected.map(|s| (s - level).abs() < EPS).unwrap_or(false)
    }
}
