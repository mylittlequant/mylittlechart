//! Approved-design concept data — a Rust transcription of
//! `sim/registry.js` (the approved 2026-08-10 modal concepts:
//! `designs/2026-08-10-*/*_v2.html`, already transcribed into JS data by
//! that file). This module carries CONTENT only — which tabs/groups/rows
//! exist, in what order, with what labels and default values — no styling,
//! no geometry, no rendering. A later task builds a renderer that walks
//! this tree; this one only has to describe it faithfully.
//!
//! Every type/tab/group/row here mirrors `registry.js` 1:1: same 14
//! `TYPES` in the same order, same tabs, same groups in the same order,
//! same rows in the same order, same labels, same default values, same
//! hints. `APP_MODALS` (9 application modals) and `templatesFor` are
//! ported the same way. This is the APPROVED DESIGN, not what
//! `mlc-core`'s primitives declare today (`sim/types.json`,
//! `primitive_settings_*.rs`) — the two deliberately differ; nothing here
//! was taken from the current code.
//!
//! # Where this diverges from a literal transcription
//!
//! - **Colours** travel as [`ThemeColor`], not CSS var strings — the JS
//!   source writes `var(--mlc-colors-accent)` etc.; a later renderer maps
//!   each variant onto the live `RuntimeTheme`.
//! - **`registry.js`'s own `t(table, key, fb)` calls** (its app-string
//!   lookup, backed by `window.SIM_I18N` in the browser sim) are replaced
//!   with the real `mlc_core::i18n` accessor for the matching key — see
//!   each call site below. One of the eight is a deliberate mismatch: the
//!   `EXTEND` group's title calls `t('CONFIG_KEY', 'Extend', 'Продление')`
//!   — the concept's own fallback word — but `ConfigKey::Extend`'s real Ru
//!   translation is "Продлить", not "Продление". The key match (English
//!   "Extend", same UI concept) is unambiguous, so this file calls the
//!   real key; the word that actually renders will be "Продлить".
//! - **Two more deliberate key reuses**, found during the i18n pass
//!   (2026-08-11): `types()`'s `elliott_impulse` entry calls
//!   `PrimitiveNameKey::ElliottImpulse`, whose real En/Ru wording is
//!   "Elliott Impulse Wave" / "Импульс Эллиотта" — a trailing "Wave" the
//!   concept's own English label "Elliott Impulse" drops; and
//!   `price_date_range` calls `PrimitiveNameKey::PriceDateRange`, whose
//!   real wording uses a slash ("Price/Date Range" / "Диапазон цена/дата")
//!   where the concept's own English label uses an ampersand ("Price &
//!   Date Range"). Same unambiguous key match as `Extend` above (identical
//!   UI concept, no other candidate key) — reused rather than duplicated
//!   into a new `ConceptKey`.
//! - **The "Точки" placement table's fake demo prices/times**
//!   (`fmtPrice`/`fmtTime` in `registry.js`) are ported as
//!   [`fmt_price`]/[`fmt_time`], reproducing the same arithmetic. One
//!   cosmetic choice: JS's `Intl`/`toLocaleString('ru-RU', ...)` groups
//!   thousands with a non-breaking space; this uses a plain space — same
//!   reading, ASCII-simpler source.
//! - **Table cells** that are plain strings in `registry.js` become
//!   [`Cell::Text`]; the two other JS cell shapes it uses —
//!   `{ t, cls }` (coloured value) and `{ sw }` (colour swatch) / `{ chk }`
//!   (inline checkbox, regression's own sigma table) — become
//!   [`Cell::Toned`], [`Cell::Swatch`], [`Cell::Check`] respectively.
//!   `cls: ''` (present but empty — the alerts list's "ждёt" row) is
//!   [`Tone::Neutral`], distinct from a `Row::Stat` with no `cls` key at
//!   all (that one carries `tone: None`, since a stat row is never wrapped
//!   in a cell shape to begin with).
//! - **User-visible strings are localized** through `mlc_core::i18n` — each
//!   row/group/tab label below calls the matching key from the app's
//!   existing vocabulary (`ConfigKey`, `SettingsKey`, `MenuKey`, `StyleKey`,
//!   `ToolbarMenuKey`, `ToolbarTooltipKey`, `ModalKey`, `ShellKey`,
//!   `WizardKey`, `ClockKey`, `UserSettingsKey`, `LabelPositionKey`,
//!   `PositionPointKey`, `PrimitiveNameKey`) or, where none already fits,
//!   this design's own `ConceptKey`. Left as bare literals: internal ids
//!   (`type_id`s, tab ids, icon names), proper nouns and technical tokens
//!   (`Gartley`, `Close`, `EMA`, `BINANCE`, `BTCUSDT`, point letters,
//!   segment names, notation samples), and numeric/timestamp captions.

use std::sync::OnceLock;

use mlc_core::i18n::{
    t, t_concept, t_config, t_indicator, t_label_pos, t_menu, t_modal, t_primitive_name,
    t_settings, t_shell, t_style, t_toolbar, t_toolbar_menu, t_user_settings, t_wave, t_wizard,
    ClockKey, ConceptKey, ConfigKey, IndicatorKey, LabelPositionKey, MenuKey, ModalKey,
    PositionPointKey, PrimitiveNameKey, SettingsKey, ShellKey, StyleKey, ToolbarMenuKey,
    ToolbarTooltipKey, UserSettingsKey, WaveDegreeKey, WizardKey,
};

// ─── Data shape ─────────────────────────────────────────────────────────────

/// One object-bound primitive type's settings-modal concept — one entry of
/// `registry.js`'s own `TYPES` array.
#[derive(Debug, Clone)]
pub struct ConceptType {
    pub id: &'static str,
    pub name: &'static str,
    pub tabs: Vec<ConceptTab>,
}

/// One tab within a type's (or a modal's) settings surface.
#[derive(Debug, Clone)]
pub struct ConceptTab {
    pub id: &'static str,
    pub label: String,
    /// Set only on `chart_settings`'s four tabs in `registry.js` — every
    /// other tab (object types and the other 8 app modals) has none.
    pub icon: Option<&'static str>,
    pub groups: Vec<ConceptGroup>,
}

/// One titled group of rows within a tab.
#[derive(Debug, Clone)]
pub struct ConceptGroup {
    pub title: String,
    pub hint: Option<String>,
    pub rows: Vec<Row>,
}

/// One application-level modal — `registry.js`'s own `APP_MODALS` entries.
/// Not object-bound: these hang off the toolbars rather than deriving
/// their tabs from a primitive type, so they carry their own metadata
/// (icon/size) a [`ConceptType`] never needs.
#[derive(Debug, Clone)]
pub struct ConceptModal {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    /// Set only on `chart_settings` — the one modal with an icon rail
    /// alongside its tab strip.
    pub rail: bool,
    pub width: f64,
    pub height: f64,
    pub tabs: Vec<ConceptTab>,
}

/// A ladder row's own value unit — decides how a value in [`Row::Ladder`]
/// reads (a ratio like `0.618`, or a Gann angle like `4`/`1`/`0.5`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LadderUnit {
    Ratio,
    Gann,
}

/// A value's semantic colour class — a table cell's `cls`, or a stat row's
/// `cls`. `Neutral` is an explicit "no colour" (`cls: ''`), distinct from
/// a [`Row::Stat`] with no `cls` at all (`tone: None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Good,
    Bad,
    Neutral,
}

/// A theme colour reference, in place of the CSS `var(--mlc-colors-*)`
/// strings `registry.js` carries — a later renderer maps each variant onto
/// the live `RuntimeTheme` rather than re-parsing a CSS var name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeColor {
    Accent,
    Success,
    Danger,
    Warning,
    TextPrimary,
    TextMuted,
    BorderLight,
    ToolbarBg,
    ModalBg,
}

/// One table/cell grid cell — the three shapes `registry.js`'s own table
/// rows use (a plain string, `{ t, cls }`, `{ sw }`, `{ chk }`).
#[derive(Debug, Clone)]
pub enum Cell {
    Text(String),
    Toned(String, Tone),
    Swatch(ThemeColor),
    Check(bool),
}

/// One settings row. One variant per `k` value in `registry.js`
/// (`chk, chk3, color, slider, dd, mode, ramps, fillmaster, ladder, note,
/// table, pills, area, num, stepper, cells, icons, stat, hr, range`),
/// carrying exactly the fields that `k`'s own row objects carry there.
#[derive(Debug, Clone)]
pub enum Row {
    Chk { label: String, on: bool },
    /// A trio of checkboxes sharing one row (`chk3` — always exactly three
    /// in `registry.js`, the ladder tab's own extend-left/extend-right/
    /// trendline set).
    Chk3 { items: Vec<(String, bool)> },
    Color { label: String, value: ThemeColor, dim: bool },
    Slider { label: String, pct: u8, value: String },
    /// `dd` — a dropdown/select row.
    Dropdown { label: String, value: String },
    /// A segmented mode switch (ladder tab's own Моно/Палитра toggle).
    Mode { label: String, options: Vec<String>, selected: usize },
    /// The ladder tab's own palette-ramp picker — no fields in
    /// `registry.js` (`{ k: 'ramps' }`), the ramp set itself is a fixed
    /// catalogue on the renderer side.
    Ramps,
    /// The ladder tab's own "fill to next level" master toggle + opacity.
    FillMaster { label: String, pct: u8, value: String, on: bool },
    /// The ladder's own level/angle list — `values` in the level's own
    /// unit (see [`LadderUnit`]).
    Ladder { values: Vec<f64>, unit: LadderUnit },
    Note { text: String },
    Table { head: Vec<String>, rows: Vec<Vec<Cell>> },
    /// A segmented-pill choice row. `label` is `None` for every occurrence
    /// except the two `registry.js` gives one (Text tab's "Начертание",
    /// user-settings' "Тема").
    Pills { label: Option<String>, options: Vec<String>, selected: usize },
    /// A free-text textarea's content.
    Area { value: String },
    /// A numeric input. `value` stays a display string (not a number) —
    /// `registry.js` itself mixes numeric literals (`v: 12`) and
    /// pre-formatted strings (`v: '1.00'`, `v: '65 400'`) for this row,
    /// and the trailing zeros/spacing are part of the concept.
    Num { label: String, value: String },
    /// A +/- stepper — always a plain integer in `registry.js`, unlike
    /// `num`.
    Stepper { label: String, value: i32 },
    /// The Table type's own editable cell grid (`cells`, distinct from
    /// `table` — this one holds plain strings only, no coloured/swatch/
    /// checkbox cells).
    Cells { cols: usize, rows: Vec<Vec<String>> },
    /// An icon/glyph picker row.
    Icons { options: Vec<String>, selected: usize },
    /// A read-only computed value (Position's Risk/Target/R:R, Regression's
    /// slope/R²). `tone` is `None` when `registry.js` gives the row no
    /// `cls` at all.
    Stat { label: String, value: String, tone: Option<Tone> },
    /// A plain divider line (`{ k: 'hr' }`).
    Hr,
    /// One timeframe-visibility range slider (Placement tab's own
    /// "Виден на таймфреймах" group).
    Range { label: String, from: i32, to: i32, cap: String },
}

// ─── Row/cell constructors ──────────────────────────────────────────────────
// One free function per `k` value, field order matching the row object's
// own key order in registry.js.

fn chk(label: &str, on: bool) -> Row {
    Row::Chk { label: label.to_string(), on }
}

fn chk3(labels: [&str; 3], on: [bool; 3]) -> Row {
    Row::Chk3 { items: labels.into_iter().zip(on).map(|(l, o)| (l.to_string(), o)).collect() }
}

fn color(label: &str, value: ThemeColor) -> Row {
    Row::Color { label: label.to_string(), value, dim: false }
}

fn color_dim(label: &str, value: ThemeColor) -> Row {
    Row::Color { label: label.to_string(), value, dim: true }
}

fn slider(label: &str, pct: u8, value: &str) -> Row {
    Row::Slider { label: label.to_string(), pct, value: value.to_string() }
}

fn dd(label: &str, value: &str) -> Row {
    Row::Dropdown { label: label.to_string(), value: value.to_string() }
}

fn mode(label: &str, opts: &[&str], selected: usize) -> Row {
    Row::Mode { label: label.to_string(), options: opts.iter().map(|s| s.to_string()).collect(), selected }
}

fn ramps() -> Row {
    Row::Ramps
}

fn fillmaster(label: &str, pct: u8, value: &str, on: bool) -> Row {
    Row::FillMaster { label: label.to_string(), pct, value: value.to_string(), on }
}

fn ladder(values: &[f64], unit: LadderUnit) -> Row {
    Row::Ladder { values: values.to_vec(), unit }
}

fn note(text: &str) -> Row {
    Row::Note { text: text.to_string() }
}

fn table(head: &[&str], rows: Vec<Vec<Cell>>) -> Row {
    Row::Table { head: head.iter().map(|s| s.to_string()).collect(), rows }
}

fn pills(label: Option<&str>, opts: &[&str], selected: usize) -> Row {
    Row::Pills {
        label: label.map(|s| s.to_string()),
        options: opts.iter().map(|s| s.to_string()).collect(),
        selected,
    }
}

fn area(value: &str) -> Row {
    Row::Area { value: value.to_string() }
}

fn num(label: &str, value: &str) -> Row {
    Row::Num { label: label.to_string(), value: value.to_string() }
}

fn stepper(label: &str, value: i32) -> Row {
    Row::Stepper { label: label.to_string(), value }
}

fn cells(cols: usize, rows: Vec<Vec<&str>>) -> Row {
    Row::Cells { cols, rows: rows.into_iter().map(|r| r.into_iter().map(|s| s.to_string()).collect()).collect() }
}

fn icons(opts: &[&str], selected: usize) -> Row {
    Row::Icons { options: opts.iter().map(|s| s.to_string()).collect(), selected }
}

fn stat(label: &str, value: &str, tone: Option<Tone>) -> Row {
    Row::Stat { label: label.to_string(), value: value.to_string(), tone }
}

fn hr() -> Row {
    Row::Hr
}

fn range(label: &str, from: i32, to: i32, cap: &str) -> Row {
    Row::Range { label: label.to_string(), from, to, cap: cap.to_string() }
}

fn txt(s: &str) -> Cell {
    Cell::Text(s.to_string())
}

fn toned(s: &str, tone: Tone) -> Cell {
    Cell::Toned(s.to_string(), tone)
}

fn swatch(c: ThemeColor) -> Cell {
    Cell::Swatch(c)
}

fn check(b: bool) -> Cell {
    Cell::Check(b)
}

fn group(title: &str, rows: Vec<Row>) -> ConceptGroup {
    ConceptGroup { title: title.to_string(), hint: None, rows }
}

fn group_hint(title: &str, hint: &str, rows: Vec<Row>) -> ConceptGroup {
    ConceptGroup { title: title.to_string(), hint: Some(hint.to_string()), rows }
}

fn tab(id: &'static str, label: &str, groups: Vec<ConceptGroup>) -> ConceptTab {
    ConceptTab { id, label: label.to_string(), icon: None, groups }
}

fn tab_icon(id: &'static str, label: &str, icon: &'static str, groups: Vec<ConceptGroup>) -> ConceptTab {
    ConceptTab { id, label: label.to_string(), icon: Some(icon), groups }
}

// ─── Fake demo price/time (points table) ────────────────────────────────────
// Mirrors registry.js's own fmtPrice/fmtTime exactly — plausible demo
// numbers for the Placement tab's "Точки" table, never live market data.

fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let len = digits.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

/// `v.toLocaleString('ru-RU', { minimumFractionDigits: 2, maximumFractionDigits: 2 })`
/// — always called with a positive value here (65100 + i*180), so no sign
/// handling. Groups thousands with a plain space rather than JS's
/// non-breaking space (see module doc).
fn fmt_price(v: f64) -> String {
    let cents = (v * 100.0).round() as u64;
    let whole = cents / 100;
    let frac = cents % 100;
    format!("{},{:02}", group_thousands(whole), frac)
}

/// ``` js
/// fmtTime(i) { return `1${i % 2 ? 0 : 1}.08 0${(i % 8) + 1}:00`; }
/// ```
fn fmt_time(i: usize) -> String {
    let day_digit = if i % 2 == 1 { 0 } else { 1 };
    let hour = (i % 8) + 1;
    format!("1{day_digit}.08 0{hour}:00")
}

/// The Elliott wave-degree ladder note (`wave_tabs`'s own "Subminuette ·
/// Minuette · … · Supercycle" list) — built from [`WaveDegreeKey`] rather
/// than carried as a literal, so it follows the UI language like every
/// other degree name in the app.
fn wave_degree_note() -> String {
    format!(
        "{} · {} · {} · {} · {} · {} · {} · {}",
        t_wave(WaveDegreeKey::Subminuette),
        t_wave(WaveDegreeKey::Minuette),
        t_wave(WaveDegreeKey::Minute),
        t_wave(WaveDegreeKey::Minor),
        t_wave(WaveDegreeKey::Intermediate),
        t_wave(WaveDegreeKey::Primary),
        t_wave(WaveDegreeKey::Cycle),
        t_wave(WaveDegreeKey::Supercycle),
    )
}

// ─── Shared groups/tabs ──────────────────────────────────────────────────────
// Mirrors registry.js's own TF / BEHAVIOUR / points / placement / labelRows
// / PLATE / LINE / FILL / EXTEND helpers.

/// Timeframe visibility: six numeric classes with range sliders — the
/// control the app actually ships, not chips.
fn tf_group() -> ConceptGroup {
    group(
        t_config(ConfigKey::GroupVisibleOnTimeframes),
        vec![
            chk(t_settings(SettingsKey::TfRanges), true),
            range(t_settings(SettingsKey::TfSeconds), 0, 100, t_concept(ConceptKey::SecondsRangeCap)),
            range(t_settings(SettingsKey::TfMinutes), 0, 100, t_concept(ConceptKey::MinutesRangeCap)),
            range(t_settings(SettingsKey::TfHours), 0, 100, t_concept(ConceptKey::HoursRangeCap)),
            range(t_settings(SettingsKey::TfDays), 0, 100, t_concept(ConceptKey::DaysRangeCap)),
            range(t_settings(SettingsKey::TfWeeks), 0, 100, t_concept(ConceptKey::WeeksRangeCap)),
            range(t_settings(SettingsKey::TfMonths), 0, 100, t_concept(ConceptKey::MonthsRangeCap)),
        ],
    )
}

fn behaviour_group() -> ConceptGroup {
    group(
        t_concept(ConceptKey::BehaviourGroupTitle),
        vec![
            chk(t_menu(MenuKey::Lock), false),
            chk(t_menu(MenuKey::ShowHide), false),
            chk(t_concept(ConceptKey::SyncAllChartsOfSymbol), false),
        ],
    )
}

fn points_group(names: &[&str], note_text: Option<&str>) -> ConceptGroup {
    let table_rows: Vec<Vec<Cell>> = names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let price = fmt_price(65_100.0 + i as f64 * 180.0);
            let time = fmt_time(i);
            vec![txt(name), txt(&price), txt(&time)]
        })
        .collect();
    let mut rows =
        vec![table(&[t_config(ConfigKey::PointLabel), t_modal(ModalKey::AlertPrice), t_concept(ConceptKey::TimeGroupTitle)], table_rows)];
    if let Some(text) = note_text {
        rows.push(note(text));
    }
    group_hint(t_config(ConfigKey::GroupPoints), &names.len().to_string(), rows)
}

fn placement_tab(names: &[&str], note_text: Option<&str>) -> ConceptTab {
    tab("placement", t_config(ConfigKey::TabPlacement), vec![tf_group(), points_group(names, note_text), behaviour_group()])
}

fn label_rows(what: &str) -> Vec<Row> {
    vec![
        chk(t_settings(SettingsKey::Show), true),
        chk(t_config(ConfigKey::LabelColorInherit), true),
        color_dim(t_concept(ConceptKey::LabelColor), ThemeColor::Accent),
        num(t_user_settings(UserSettingsKey::ControlPointSizeLabel), "12"),
        chk(t_config(ConfigKey::Bold), false),
        note(what),
    ]
}

fn plate_group() -> ConceptGroup {
    group(
        t_concept(ConceptKey::Plate),
        vec![
            chk(t_concept(ConceptKey::PlateBehindLabel), true),
            color(t_concept(ConceptKey::PlateColor), ThemeColor::ModalBg),
            num(t_concept(ConceptKey::PlateRounding), "3"),
            num(t_concept(ConceptKey::PlatePadding), "4"),
        ],
    )
}

fn line_group(title: Option<&str>) -> ConceptGroup {
    let title = title.unwrap_or_else(|| t_toolbar_menu(ToolbarMenuKey::Line));
    group(
        title,
        vec![
            color(t_settings(SettingsKey::Color), ThemeColor::Accent),
            slider(t_config(ConfigKey::WidthLabel), 24, "1.0"),
            dd(t_config(ConfigKey::Style), t_style(StyleKey::Solid)),
        ],
    )
}

fn fill_group(title: Option<&str>, on: bool) -> ConceptGroup {
    let title = title.unwrap_or_else(|| t_config(ConfigKey::Fill));
    group(
        title,
        vec![
            chk(t_concept(ConceptKey::FillToggle), on),
            color(t_settings(SettingsKey::Color), ThemeColor::Accent),
            slider(t_config(ConfigKey::FillOpacity), 10, "10%"),
        ],
    )
}

fn extend_group() -> ConceptGroup {
    group(
        t_config(ConfigKey::Extend),
        vec![chk(t_concept(ConceptKey::ExtendLeftShort), false), chk(t_concept(ConceptKey::ExtendRightShort), false)],
    )
}

// ─── Archetypes ──────────────────────────────────────────────────────────────
// Mirrors registry.js's own A.ladder / A.wave / A.pattern / A.position /
// A.measure / A.text / A.table / A.shape / A.regression / A.brush / A.icon.

fn ladder_tabs(subject: &'static str, values: &[f64], unit: LadderUnit) -> Vec<ConceptTab> {
    vec![
        tab(
            "levels",
            subject,
            vec![
                group(
                    t_concept(ConceptKey::LadderGeneralGroupTitle),
                    vec![
                        // Two independent axes (2026-08-13 split of the old
                        // single combined Моно/Палитра control — see
                        // `mlc_app_layout::layout::modals::
                        // primitive_settings_levels::draw_top_section`'s own
                        // doc): "Линии" writes/clears a level's own line
                        // colour, "Заливка" writes/clears its band colour,
                        // independently.
                        mode(
                            t_concept(ConceptKey::LinesAxisLabel),
                            &[t_config(ConfigKey::ColorModeMono), t_config(ConfigKey::ColorModePalette)],
                            1,
                        ),
                        mode(
                            t_config(ConfigKey::Fill),
                            &[t_concept(ConceptKey::FillInheritLine), t_config(ConfigKey::ColorModePalette)],
                            1,
                        ),
                        slider(t_config(ConfigKey::WidthLabel), 24, "1.0"),
                        dd(t_concept(ConceptKey::TypeDropdownLabel), t_style(StyleKey::Solid)),
                        chk3(
                            [
                                t_settings(SettingsKey::ExtendLeft),
                                t_settings(SettingsKey::ExtendRight),
                                t_toolbar(ToolbarTooltipKey::TrendLine),
                            ],
                            [false, false, true],
                        ),
                    ],
                ),
                group(t_concept(ConceptKey::LadderPalettesGroupTitle), vec![ramps()]),
                group(
                    t_config(ConfigKey::Fill),
                    vec![fillmaster(t_concept(ConceptKey::FillToNextLevel), 10, "10%", true)],
                ),
                group_hint(subject, &values.len().to_string(), vec![ladder(values, unit)]),
            ],
        ),
        tab(
            "labels",
            t_config(ConfigKey::Labels),
            vec![
                group(
                    t_concept(ConceptKey::LevelLabelsGroupTitle),
                    label_rows(t_concept(ConceptKey::NoteLabelInheritsLevelColor)),
                ),
                plate_group(),
            ],
        ),
        placement_tab(&["1", "2"], None),
    ]
}

fn wave_tabs() -> Vec<ConceptTab> {
    let mut wave_label_rows = label_rows(t_concept(ConceptKey::NoteWaveNamesFromDegree));
    wave_label_rows.push(chk(t_concept(ConceptKey::ShowDegree), false));

    let degree_note = wave_degree_note();

    vec![
        tab(
            "waves",
            t_concept(ConceptKey::WavesTabLabel),
            vec![
                line_group(None),
                group(
                    t_concept(ConceptKey::Degree),
                    vec![dd(t_config(ConfigKey::WaveDegree), t_wave(WaveDegreeKey::Minor)), note(&degree_note)],
                ),
                group(
                    t_concept(ConceptKey::Notation),
                    vec![
                        pills(None, &["1 2 3 4 5", "I II III IV V", "① ② ③ ④ ⑤"], 0),
                        note(t_concept(ConceptKey::NoteWaveNotationCorrectionCombination)),
                    ],
                ),
                group_hint(
                    t_config(ConfigKey::CorrectionType),
                    t_concept(ConceptKey::CorrectionOnlyHint),
                    vec![dd(t_concept(ConceptKey::TypeDropdownLabel), t_config(ConfigKey::CorrectionZigzag))],
                ),
                fill_group(Some(t_concept(ConceptKey::FillBetweenWaves)), false),
            ],
        ),
        tab(
            "labels",
            t_config(ConfigKey::Labels),
            vec![
                group(t_concept(ConceptKey::WaveLabelsGroupTitle), wave_label_rows),
                plate_group(),
                group_hint(
                    t_concept(ConceptKey::CustomLabelsGroupTitle),
                    t_concept(ConceptKey::OverrideHint),
                    vec![table(
                        &[t_concept(ConceptKey::WaveColumnHeader), t_concept(ConceptKey::LabelColumnHeader)],
                        vec![
                            vec![txt("0"), txt("0")],
                            vec![txt("1"), txt("1")],
                            vec![txt("2"), txt("2")],
                            vec![txt("3"), txt("3")],
                            vec![txt("4"), txt("4")],
                            vec![txt("5"), txt("5")],
                        ],
                    )],
                ),
            ],
        ),
        placement_tab(&["0", "1", "2", "3", "4", "5"], None),
    ]
}

fn pattern_tabs() -> Vec<ConceptTab> {
    vec![
        tab(
            "ratios",
            t_concept(ConceptKey::RatiosTabLabel),
            vec![
                group(
                    t_concept(ConceptKey::Subtype),
                    vec![
                        dd(t_concept(ConceptKey::PatternDropdownLabel), "Gartley"),
                        note(t_concept(ConceptKey::NotePatternSubtypesList)),
                    ],
                ),
                line_group(Some(t_toolbar_menu(ToolbarMenuKey::HeaderLines))),
                fill_group(Some(t_concept(ConceptKey::PatternBody)), true),
                group(
                    t_concept(ConceptKey::TargetRatiosGroupTitle),
                    vec![table(
                        &[
                            t_concept(ConceptKey::SegmentColumnHeader),
                            t_config(ConfigKey::PositionTargetLabel),
                            t_concept(ConceptKey::ActualColumnHeader),
                            t_concept(ConceptKey::ToleranceColumnHeader),
                        ],
                        vec![
                            vec![txt("XA→AB"), txt("0.618"), toned("0.621", Tone::Good), txt("3%")],
                            vec![txt("AB→BC"), txt("0.382"), toned("0.394", Tone::Good), txt("5%")],
                            vec![txt("BC→CD"), txt("1.272"), toned("1.512", Tone::Bad), txt("5%")],
                            vec![txt("XA→AD"), txt("0.786"), toned("0.780", Tone::Good), txt("3%")],
                        ],
                    )],
                ),
                group(
                    t_concept(ConceptKey::ScoringGroupTitle),
                    vec![
                        color(t_concept(ConceptKey::HitColor), ThemeColor::Success),
                        color(t_concept(ConceptKey::MissColor), ThemeColor::Danger),
                        chk(t_concept(ConceptKey::ColorLinesByHit), false),
                    ],
                ),
            ],
        ),
        tab(
            "labels",
            t_config(ConfigKey::Labels),
            vec![
                group(t_concept(ConceptKey::PointLabelsGroupTitle), label_rows(t_concept(ConceptKey::NotePatternPointNames))),
                group(
                    t_concept(ConceptKey::OnChartValuesGroupTitle),
                    vec![
                        chk(t_concept(ConceptKey::ShowRatiosValues), true),
                        chk(t_concept(ConceptKey::ShowSegmentLength), false),
                        num(t_concept(ConceptKey::DecimalPlaces), "3"),
                        dd(t_concept(ConceptKey::SideOfSegment), t_label_pos(LabelPositionKey::Outside)),
                    ],
                ),
                plate_group(),
            ],
        ),
        placement_tab(&["X", "A", "B", "C", "D"], None),
    ]
}

fn position_tabs() -> Vec<ConceptTab> {
    vec![
        tab(
            "trade",
            t_concept(ConceptKey::TradeTabLabel),
            vec![
                group(
                    t_toolbar_menu(ToolbarMenuKey::HeaderVolume),
                    vec![
                        num(t_config(ConfigKey::Quantity), "1.00"),
                        num(t_concept(ConceptKey::AccountSize), "10000"),
                        num(t_concept(ConceptKey::RiskPerTradePct), "1.0"),
                        chk(t_concept(ConceptKey::CalcSizeFromRisk), false),
                    ],
                ),
                group(
                    t_concept(ConceptKey::ZonesGroupTitle),
                    vec![
                        color(t_concept(ConceptKey::StopZone), ThemeColor::Danger),
                        color(t_concept(ConceptKey::TargetZone), ThemeColor::Success),
                        slider(t_config(ConfigKey::FillOpacity), 18, "18%"),
                    ],
                ),
                group(
                    t_toolbar_menu(ToolbarMenuKey::HeaderLines),
                    vec![
                        color(t(PositionPointKey::Entry), ThemeColor::Accent),
                        color(t_concept(ConceptKey::StopPointLabel), ThemeColor::Danger),
                        color(t_config(ConfigKey::PositionTargetLabel), ThemeColor::Success),
                        dd(t_config(ConfigKey::Style), t_style(StyleKey::Solid)),
                    ],
                ),
                group(
                    t_concept(ConceptKey::DisplayGroupTitle),
                    vec![
                        pills(
                            None,
                            &[t_concept(ConceptKey::Currency), t_settings(SettingsKey::TfTicks), t_concept(ConceptKey::PercentUnit)],
                            0,
                        ),
                        chk(t_concept(ConceptKey::ShowPnlToggle), true),
                        chk(t_concept(ConceptKey::ShowVolumeToggle), true),
                    ],
                ),
                group_hint(
                    t_concept(ConceptKey::ResultGroupTitle),
                    t_concept(ConceptKey::CalculatedHintSingular),
                    vec![
                        stat(t_config(ConfigKey::PositionRiskLabel), "−437.59", Some(Tone::Bad)),
                        stat(t_config(ConfigKey::PositionTargetLabel), "+875.19", Some(Tone::Good)),
                        hr(),
                        stat(t_concept(ConceptKey::RiskRewardRatio), "1 : 2.00", None),
                        note(t_concept(ConceptKey::NotePositionResultReadOnly)),
                    ],
                ),
            ],
        ),
        tab(
            "labels",
            t_config(ConfigKey::Labels),
            vec![
                group(t_config(ConfigKey::Labels), label_rows(t_concept(ConceptKey::NotePositionLabelsWhat))),
                group(
                    t_concept(ConceptKey::WhatToWriteGroupTitle),
                    vec![
                        chk(t_concept(ConceptKey::WritePointName), true),
                        chk(t_concept(ConceptKey::WritePrice), true),
                        chk(t_concept(ConceptKey::WriteDistancePct), true),
                        chk(t_concept(ConceptKey::WriteRiskTargetSum), false),
                        num(t_concept(ConceptKey::DecimalPlaces), "2"),
                    ],
                ),
                plate_group(),
            ],
        ),
        placement_tab(&[t(PositionPointKey::Entry), t_concept(ConceptKey::StopPointLabel), t_config(ConfigKey::PositionTargetLabel)], None),
    ]
}

fn measure_tabs() -> Vec<ConceptTab> {
    vec![
        tab(
            "show",
            t_concept(ConceptKey::WhatToShowTabLabel),
            vec![
                group(
                    t_concept(ConceptKey::ValuesGroupTitle),
                    vec![
                        chk(t_concept(ConceptKey::PriceChange), true),
                        chk(t_concept(ConceptKey::PercentUnit), true),
                        chk(t_concept(ConceptKey::BarCount), true),
                        chk(t_concept(ConceptKey::TimeGroupTitle), false),
                        chk(t_concept(ConceptKey::VolumeOverRange), false),
                        chk(t_concept(ConceptKey::AveragePrice), false),
                        chk(t_concept(ConceptKey::SlopeAngle), false),
                    ],
                ),
                group(
                    t_concept(ConceptKey::FormatGroupTitle),
                    vec![
                        num(t_concept(ConceptKey::DecimalPlaces), "2"),
                        chk(t_concept(ConceptKey::ThousandsSeparator), true),
                        dd(t_concept(ConceptKey::VolumeUnit), t_concept(ConceptKey::AutoKmUnit)),
                    ],
                ),
                group(
                    t_concept(ConceptKey::LineAndArrowGroupTitle),
                    vec![
                        color(t_settings(SettingsKey::Color), ThemeColor::Accent),
                        slider(t_config(ConfigKey::WidthLabel), 24, "1.0"),
                        chk(t_concept(ConceptKey::ArrowheadAtEnd), true),
                    ],
                ),
                group(
                    t_toolbar_menu(ToolbarMenuKey::Area),
                    vec![
                        chk(t_concept(ConceptKey::FillToggle), true),
                        slider(t_config(ConfigKey::FillOpacity), 15, "15%"),
                        chk(t_concept(ConceptKey::ColorBySign), true),
                        color(t_concept(ConceptKey::MeasureUp), ThemeColor::Success),
                        color(t_concept(ConceptKey::MeasureDown), ThemeColor::Danger),
                    ],
                ),
            ],
        ),
        tab(
            "labels",
            t_config(ConfigKey::Labels),
            vec![
                group(
                    t_settings(SettingsKey::Text),
                    vec![
                        num(t_user_settings(UserSettingsKey::ControlPointSizeLabel), "12"),
                        color(t_settings(SettingsKey::Color), ThemeColor::TextPrimary),
                        chk(t_config(ConfigKey::LabelColorInherit), false),
                    ],
                ),
                group(
                    t_concept(ConceptKey::Plate),
                    vec![
                        pills(
                            None,
                            &[t_config(ConfigKey::AlignTop), t_concept(ConceptKey::PlateVerticalMiddle), t_config(ConfigKey::AlignBottom)],
                            1,
                        ),
                        chk(t_concept(ConceptKey::PlateBackground), true),
                        color(t_concept(ConceptKey::BackgroundColor), ThemeColor::ModalBg),
                    ],
                ),
            ],
        ),
        placement_tab(&[t_concept(ConceptKey::PointStart), t_concept(ConceptKey::PointEnd)], None),
    ]
}

fn text_tabs() -> Vec<ConceptTab> {
    vec![
        tab(
            "text",
            t_settings(SettingsKey::Text),
            vec![
                group(
                    t_concept(ConceptKey::ContentGroupTitle),
                    vec![
                        area(t_concept(ConceptKey::NoteCalloutDemoText)),
                        note(t_concept(ConceptKey::NoteCalloutPlaceholders)),
                    ],
                ),
                group(
                    t_concept(ConceptKey::FontGroupTitle),
                    vec![
                        dd(t_concept(ConceptKey::Typeface), "Inter"),
                        num(t_user_settings(UserSettingsKey::ControlPointSizeLabel), "13"),
                        pills(
                            Some(t_config(ConfigKey::TextStyleLabel)),
                            &[t_config(ConfigKey::BoldAbbrev), t_config(ConfigKey::ItalicAbbrev), t_concept(ConceptKey::StrikethroughAbbrev)],
                            0,
                        ),
                        color(t_settings(SettingsKey::Color), ThemeColor::TextPrimary),
                    ],
                ),
                group(
                    t_concept(ConceptKey::LayoutGroupTitle),
                    vec![
                        pills(
                            None,
                            &[
                                t_concept(ConceptKey::AlignLeftShort),
                                t_config(ConfigKey::AlignCenter),
                                t_concept(ConceptKey::AlignRightShort),
                            ],
                            0,
                        ),
                        chk(t_concept(ConceptKey::WordWrap), true),
                        chk(t_concept(ConceptKey::WidthFitsContent), true),
                        num(t_concept(ConceptKey::MaxWidth), "240"),
                    ],
                ),
                group(
                    t_concept(ConceptKey::Plate),
                    vec![
                        chk(t_concept(ConceptKey::Border), true),
                        color(t_concept(ConceptKey::CalloutBorderColor), ThemeColor::Accent),
                        chk(t_concept(ConceptKey::Background), true),
                        color(t_concept(ConceptKey::BackgroundColor), ThemeColor::ModalBg),
                        num(t_concept(ConceptKey::PlateRounding), "4"),
                        num(t_concept(ConceptKey::PlatePadding), "6"),
                        chk(t_concept(ConceptKey::Shadow), false),
                    ],
                ),
                group_hint(
                    t_primitive_name(PrimitiveNameKey::Signpost),
                    t_concept(ConceptKey::CalloutHint),
                    vec![chk(t_concept(ConceptKey::TailToPoint), true), dd(t_concept(ConceptKey::Tip), t_concept(ConceptKey::ArrowTipOption))],
                ),
            ],
        ),
        tab(
            "placement",
            t_config(ConfigKey::TabPlacement),
            vec![
                group(
                    t_concept(ConceptKey::Anchor),
                    vec![
                        pills(
                            None,
                            &[
                                t_concept(ConceptKey::AnchorToPriceAndBar),
                                t_concept(ConceptKey::AnchorToBar),
                                t_concept(ConceptKey::AnchorToScreen),
                            ],
                            0,
                        ),
                        pills(
                            None,
                            &[t_config(ConfigKey::AlignTop), t_config(ConfigKey::AlignRight), t_config(ConfigKey::AlignBottom), t_config(ConfigKey::AlignLeft)],
                            1,
                        ),
                    ],
                ),
                tf_group(),
                points_group(&[t_concept(ConceptKey::Anchor), t_concept(ConceptKey::Plate)], None),
                behaviour_group(),
            ],
        ),
    ]
}

fn table_tabs() -> Vec<ConceptTab> {
    vec![
        tab(
            "cells",
            t_config(ConfigKey::TabCells),
            vec![
                group(
                    t_user_settings(UserSettingsKey::ControlPointSizeLabel),
                    vec![stepper(t_concept(ConceptKey::RowsCount), 4), stepper(t_concept(ConceptKey::ColumnsCount), 3)],
                ),
                group_hint(
                    t_concept(ConceptKey::ContentGroupTitle),
                    "4 × 3",
                    vec![cells(
                        3,
                        vec![
                            vec![t_concept(ConceptKey::LevelColumnHeader), t_modal(ModalKey::AlertPrice), t_config(ConfigKey::Comment)],
                            vec![t_concept(ConceptKey::DemoResistance), "65 400", t_concept(ConceptKey::DemoDailyHigh)],
                            vec![t(PositionPointKey::Entry), "65 100", t_concept(ConceptKey::DemoOnRetest)],
                            vec![t_concept(ConceptKey::StopPointLabel), "64 660", t_concept(ConceptKey::DemoBelowLow)],
                        ],
                    )],
                ),
                group(
                    t_concept(ConceptKey::ColumnsGroupTitle),
                    vec![table(
                        &[t_concept(ConceptKey::ColumnColumnHeader), t_config(ConfigKey::PositionLabel), t_config(ConfigKey::BubbleWidth)],
                        vec![
                            vec![txt("1"), txt(t_concept(ConceptKey::AlignLeftShort)), txt(t_concept(ConceptKey::AutoWidthValue))],
                            vec![txt("2"), txt(t_concept(ConceptKey::AlignRightShort)), txt("70")],
                            vec![txt("3"), txt(t_concept(ConceptKey::AlignLeftShort)), txt(t_concept(ConceptKey::AutoWidthValue))],
                        ],
                    )],
                ),
                group(
                    t_concept(ConceptKey::GridAndHeaderGroupTitle),
                    vec![
                        chk(t_config(ConfigKey::ShowLines), true),
                        color(t_concept(ConceptKey::LineColor), ThemeColor::BorderLight),
                        chk(t_concept(ConceptKey::FirstRowIsHeader), true),
                        chk(t_concept(ConceptKey::AlternateRows), false),
                    ],
                ),
                group(
                    t_concept(ConceptKey::TextAndBackgroundGroupTitle),
                    vec![
                        num(t_user_settings(UserSettingsKey::ControlPointSizeLabel), "12"),
                        color(t_config(ConfigKey::TextColor), ThemeColor::TextPrimary),
                        color(t_concept(ConceptKey::TableBackground), ThemeColor::ToolbarBg),
                        slider(t_config(ConfigKey::FillOpacity), 90, "90%"),
                    ],
                ),
            ],
        ),
        tab(
            "placement",
            t_config(ConfigKey::TabPlacement),
            vec![
                group(
                    t_concept(ConceptKey::Anchor),
                    vec![
                        pills(None, &[t_concept(ConceptKey::AnchorToPriceAndBar), t_concept(ConceptKey::AnchorToScreen)], 1),
                        pills(
                            None,
                            &[
                                t_concept(ConceptKey::AnchorTopLeft),
                                t_concept(ConceptKey::AnchorTopRight),
                                t_concept(ConceptKey::AnchorBottomLeft),
                                t_concept(ConceptKey::AnchorBottomRight),
                            ],
                            1,
                        ),
                    ],
                ),
                tf_group(),
                behaviour_group(),
            ],
        ),
    ]
}

fn shape_tabs(subject: &'static str, extra: Vec<ConceptGroup>) -> Vec<ConceptTab> {
    let mut groups = vec![line_group(Some(t_concept(ConceptKey::Outline)))];
    groups.extend(extra);
    groups.push(fill_group(Some(t_config(ConfigKey::Fill)), true));
    groups.push(extend_group());
    groups.push(group(
        t_concept(ConceptKey::TextOnShape),
        vec![
            chk(t_settings(SettingsKey::Show), false),
            num(t_user_settings(UserSettingsKey::ControlPointSizeLabel), "12"),
            dd(t_config(ConfigKey::LabelPosition), t_config(ConfigKey::AlignCenter)),
        ],
    ));
    vec![
        tab("shape", subject, groups),
        placement_tab(&["1", "2"], Some(t_concept(ConceptKey::NoteShapePolylinePointsList))),
    ]
}

fn regression_tabs() -> Vec<ConceptTab> {
    vec![
        tab(
            "sigma",
            t_concept(ConceptKey::DeviationsTabLabel),
            vec![
                group(
                    t_concept(ConceptKey::CalculationGroupTitle),
                    vec![
                        dd(t_modal(ModalKey::AlertSource), "Close"),
                        dd(t_concept(ConceptKey::Period), t_concept(ConceptKey::ByPoints)),
                        num(t_concept(ConceptKey::BarCount), "120"),
                    ],
                ),
                group(
                    t_concept(ConceptKey::SigmasGroupTitle),
                    vec![
                        table(
                            &["", t_concept(ConceptKey::Multiplier), t_settings(SettingsKey::Color), t_config(ConfigKey::Fill)],
                            vec![
                                vec![txt("±1σ"), txt("1.0"), swatch(ThemeColor::Accent), check(true)],
                                vec![txt("±2σ"), txt("2.0"), swatch(ThemeColor::Warning), check(true)],
                                vec![txt("±3σ"), txt("3.0"), swatch(ThemeColor::Danger), check(false)],
                            ],
                        ),
                        slider(t_concept(ConceptKey::FillOpacityLong), 10, "10%"),
                    ],
                ),
                group(
                    t_toolbar_menu(ToolbarMenuKey::Baseline),
                    vec![
                        chk(t_settings(SettingsKey::Show), true),
                        color(t_settings(SettingsKey::Color), ThemeColor::Accent),
                        slider(t_config(ConfigKey::WidthLabel), 30, "1.5"),
                    ],
                ),
                extend_group(),
                group_hint(
                    t_concept(ConceptKey::MetricsGroupTitle),
                    t_concept(ConceptKey::CalculatedHintPlural),
                    vec![
                        chk(t_concept(ConceptKey::ShowSlope), false),
                        chk(t_concept(ConceptKey::ShowRSquared), true),
                        stat(t_concept(ConceptKey::Slope), t_concept(ConceptKey::DemoSlopeValue), None),
                        stat("R²", "0.87", None),
                    ],
                ),
            ],
        ),
        placement_tab(&[t_concept(ConceptKey::PointStart), t_concept(ConceptKey::PointEnd)], None),
    ]
}

fn brush_tabs() -> Vec<ConceptTab> {
    vec![
        tab(
            "stroke",
            t_concept(ConceptKey::StrokeGroupTitle),
            vec![
                group(
                    t_concept(ConceptKey::StrokeGroupTitle),
                    vec![
                        color(t_settings(SettingsKey::Color), ThemeColor::Accent),
                        slider(t_user_settings(UserSettingsKey::ControlPointSizeLabel), 35, "3.0"),
                        slider(t_config(ConfigKey::FillOpacity), 100, "100%"),
                        dd(t_settings(SettingsKey::LineStyle), t_style(StyleKey::Solid)),
                    ],
                ),
                group(
                    t_user_settings(UserSettingsKey::ControlPointShapeLabel),
                    vec![
                        chk(t_config(ConfigKey::Smooth), true),
                        slider(t_concept(ConceptKey::SmoothingStrength), 50, "50%"),
                        chk(t_concept(ConceptKey::RoundEndCaps), true),
                        chk(t_concept(ConceptKey::CloseOutline), false),
                    ],
                ),
                fill_group(Some(t_concept(ConceptKey::FillClosedOutline)), false),
            ],
        ),
        placement_tab(
            &[t_concept(ConceptKey::PointStart), t_concept(ConceptKey::PointEnd)],
            Some(t_concept(ConceptKey::NoteBrushPointsNotEditable)),
        ),
    ]
}

fn icon_tabs() -> Vec<ConceptTab> {
    vec![
        tab(
            "icon",
            t_concept(ConceptKey::IconTabLabel),
            vec![
                group(
                    t_settings(SettingsKey::CompareInfoSymbol),
                    vec![icons(
                        &["▲", "▼", "⚑", "★", "●", "◆", "✕", "✓", "❗", "❓", "💡", "🔒", "📌", "🔔", "👁", "💬"],
                        0,
                    )],
                ),
                group(
                    t_concept(ConceptKey::Rendering),
                    vec![
                        color(t_settings(SettingsKey::Color), ThemeColor::Success),
                        slider(t_user_settings(UserSettingsKey::ControlPointSizeLabel), 40, "18"),
                        chk(t_concept(ConceptKey::IconOutlineToggle), false),
                    ],
                ),
                group(
                    t_concept(ConceptKey::LabelNextToIcon),
                    vec![
                        chk(t_settings(SettingsKey::Show), true),
                        dd(t_settings(SettingsKey::Text), t_concept(ConceptKey::DemoTrendEntryLabel)),
                        num(t_user_settings(UserSettingsKey::ControlPointSizeLabel), "12"),
                        dd(t_concept(ConceptKey::Side), t_config(ConfigKey::AlignRight)),
                    ],
                ),
            ],
        ),
        placement_tab(&[t_config(ConfigKey::PointLabel)], None),
    ]
}

// ─── Registry accessors ──────────────────────────────────────────────────────

/// The 14 object-bound primitive types, in `registry.js`'s own `TYPES`
/// order. Built once and cached — this data is fixed for the process
/// lifetime.
pub fn types() -> &'static [ConceptType] {
    static TYPES: OnceLock<Vec<ConceptType>> = OnceLock::new();
    TYPES.get_or_init(|| {
        const FIB_LEVELS: [f64; 7] = [0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0];
        const GANN_ANGLES: [f64; 7] = [8.0, 4.0, 3.0, 2.0, 1.0, 0.5, 0.333];
        const PITCHFORK_LEVELS: [f64; 4] = [0.25, 0.5, 0.75, 1.0];

        vec![
            ConceptType {
                id: "fib_retracement",
                name: t_primitive_name(PrimitiveNameKey::FibRetracement),
                tabs: ladder_tabs(t_config(ConfigKey::Levels), &FIB_LEVELS, LadderUnit::Ratio),
            },
            ConceptType {
                id: "gann_fan",
                name: t_primitive_name(PrimitiveNameKey::GannFan),
                tabs: ladder_tabs(t_concept(ConceptKey::Angles), &GANN_ANGLES, LadderUnit::Gann),
            },
            ConceptType {
                id: "pitchfork",
                name: t_primitive_name(PrimitiveNameKey::Pitchfork),
                tabs: ladder_tabs(t_config(ConfigKey::Levels), &PITCHFORK_LEVELS, LadderUnit::Ratio),
            },
            ConceptType { id: "elliott_impulse", name: t_primitive_name(PrimitiveNameKey::ElliottImpulse), tabs: wave_tabs() },
            ConceptType { id: "xabcd_pattern", name: t_primitive_name(PrimitiveNameKey::XabcdPattern), tabs: pattern_tabs() },
            ConceptType { id: "long_position", name: t_primitive_name(PrimitiveNameKey::LongPosition), tabs: position_tabs() },
            ConceptType {
                id: "price_date_range",
                name: t_primitive_name(PrimitiveNameKey::PriceDateRange),
                tabs: measure_tabs(),
            },
            ConceptType { id: "callout", name: t_primitive_name(PrimitiveNameKey::Callout), tabs: text_tabs() },
            ConceptType { id: "table", name: t_primitive_name(PrimitiveNameKey::Table), tabs: table_tabs() },
            ConceptType {
                id: "rectangle",
                name: t_primitive_name(PrimitiveNameKey::Rectangle),
                tabs: shape_tabs(t_concept(ConceptKey::ShapeSubject), Vec::new()),
            },
            ConceptType {
                id: "parallel_channel",
                name: t_primitive_name(PrimitiveNameKey::ParallelChannel),
                tabs: shape_tabs(
                    t_concept(ConceptKey::ChannelSubject),
                    vec![group(
                        t_concept(ConceptKey::MiddleLine),
                        vec![
                            chk(t_settings(SettingsKey::Show), true),
                            color(t_settings(SettingsKey::Color), ThemeColor::TextMuted),
                            dd(t_config(ConfigKey::Style), t_concept(ConceptKey::DashedShort)),
                        ],
                    )],
                ),
            },
            ConceptType { id: "regression_trend", name: t_primitive_name(PrimitiveNameKey::RegressionTrend), tabs: regression_tabs() },
            ConceptType { id: "brush", name: t_primitive_name(PrimitiveNameKey::Brush), tabs: brush_tabs() },
            ConceptType { id: "flag", name: t_primitive_name(PrimitiveNameKey::Flag), tabs: icon_tabs() },
        ]
    })
}

/// The 9 application-level modals, in `registry.js`'s own `APP_MODALS`
/// order.
pub fn app_modals() -> &'static [ConceptModal] {
    static MODALS: OnceLock<Vec<ConceptModal>> = OnceLock::new();
    MODALS.get_or_init(|| {
        vec![
            ConceptModal {
                id: "chart_settings",
                name: t_shell(ShellKey::AppModalChartSettings),
                icon: "settings",
                rail: true,
                width: 620.0,
                height: 560.0,
                tabs: vec![
                    tab_icon(
                        "instrument",
                        t_concept(ConceptKey::Instrument),
                        "candlestick",
                        vec![
                            group(
                                t_toolbar_menu(ToolbarMenuKey::Candles),
                                vec![
                                    color(t_concept(ConceptKey::UpBody), ThemeColor::Success),
                                    color(t_concept(ConceptKey::DownBody), ThemeColor::Danger),
                                    chk(t_toolbar_menu(ToolbarMenuKey::HollowCandles), false),
                                    chk(t_concept(ConceptKey::Wicks), true),
                                    color(t_concept(ConceptKey::WickColor), ThemeColor::TextMuted),
                                ],
                            ),
                            group(
                                t_settings(SettingsKey::Borders),
                                vec![chk(t_settings(SettingsKey::Show), true), slider(t_config(ConfigKey::WidthLabel), 20, "1.0")],
                            ),
                            group(
                                t_concept(ConceptKey::Other),
                                vec![
                                    chk(t_concept(ConceptKey::LastPrice), true),
                                    chk(t_concept(ConceptKey::PreviousClose), false),
                                    chk(t_concept(ConceptKey::HighAndLow), false),
                                ],
                            ),
                        ],
                    ),
                    tab_icon(
                        "status",
                        t_concept(ConceptKey::StatusLineTabLabel),
                        "legend",
                        vec![
                            group(
                                t_settings(SettingsKey::Show),
                                vec![
                                    chk(t_concept(ConceptKey::Ticker), true),
                                    chk(t_concept(ConceptKey::OhlcValues), true),
                                    chk(t_concept(ConceptKey::Change), true),
                                    chk(t_toolbar_menu(ToolbarMenuKey::HeaderVolume), true),
                                    chk(t_toolbar(ToolbarTooltipKey::Indicators), true),
                                ],
                            ),
                            group(
                                t_settings(SettingsKey::TabAppearance),
                                vec![
                                    num(t_user_settings(UserSettingsKey::ControlPointSizeLabel), "12"),
                                    dd(t_config(ConfigKey::LabelPosition), t_concept(ConceptKey::AnchorTopLeft)),
                                    slider(t_concept(ConceptKey::BackgroundOpacity), 60, "60%"),
                                ],
                            ),
                        ],
                    ),
                    tab_icon(
                        "scales",
                        t_settings(SettingsKey::TabScalesLines),
                        "grid",
                        vec![
                            group(
                                t_toolbar_menu(ToolbarMenuKey::SubGrid),
                                vec![
                                    chk(t_concept(ConceptKey::Vertical), true),
                                    chk(t_concept(ConceptKey::Horizontal), true),
                                    color(t_settings(SettingsKey::Color), ThemeColor::BorderLight),
                                    dd(t_config(ConfigKey::Style), t_style(StyleKey::Solid)),
                                ],
                            ),
                            group(
                                t_settings(SettingsKey::PriceScalePosition),
                                vec![
                                    dd(t_settings(SettingsKey::CrosshairMode), t_settings(SettingsKey::PrecisionAuto)),
                                    num(t_config(ConfigKey::BubbleWidth), "70"),
                                    chk(t_concept(ConceptKey::PercentUnit), false),
                                    chk(t_concept(ConceptKey::InvertedScale), false),
                                ],
                            ),
                            group(
                                t_settings(SettingsKey::TimeScalePosition),
                                vec![
                                    chk(t_concept(ConceptKey::ShowSeconds), false),
                                    chk(t_concept(ConceptKey::ShowDate), true),
                                    num(t_config(ConfigKey::BubbleHeight), "30"),
                                ],
                            ),
                            group(
                                t_concept(ConceptKey::Crosshair),
                                vec![
                                    color(t_settings(SettingsKey::Color), ThemeColor::TextMuted),
                                    dd(t_config(ConfigKey::Style), t_concept(ConceptKey::DashedShort)),
                                ],
                            ),
                        ],
                    ),
                    tab_icon(
                        "look",
                        t_settings(SettingsKey::TabAppearance),
                        "palette",
                        vec![
                            group(
                                t_concept(ConceptKey::Background),
                                vec![
                                    pills(None, &[t_concept(ConceptKey::SolidBackground), t_concept(ConceptKey::Gradient)], 0),
                                    color(t_settings(SettingsKey::Color), ThemeColor::ToolbarBg),
                                ],
                            ),
                            group(
                                t_toolbar_menu(ToolbarMenuKey::SubWatermark),
                                vec![
                                    chk(t_settings(SettingsKey::Show), true),
                                    slider(t_config(ConfigKey::FillOpacity), 20, "20%"),
                                    num(t_user_settings(UserSettingsKey::ControlPointSizeLabel), "52"),
                                ],
                            ),
                            group(
                                t_concept(ConceptKey::Margins),
                                vec![
                                    slider(t_config(ConfigKey::AlignTop), 10, "10%"),
                                    slider(t_config(ConfigKey::AlignBottom), 8, "8%"),
                                    num(t_concept(ConceptKey::RightMarginBars), "10"),
                                ],
                            ),
                        ],
                    ),
                ],
            },
            ConceptModal {
                id: "indicator_settings",
                name: t_shell(ShellKey::AppModalIndicatorSettings),
                icon: "indicators",
                rail: false,
                width: 520.0,
                height: 470.0,
                tabs: vec![
                    tab(
                        "inputs",
                        t_indicator(IndicatorKey::TabInputs),
                        vec![
                            group(
                                t(PositionPointKey::Entry),
                                vec![
                                    dd(t_modal(ModalKey::AlertSource), "Close"),
                                    num(t_concept(ConceptKey::Period), "14"),
                                    dd(t_config(ConfigKey::Smooth), "EMA"),
                                    num(t_concept(ConceptKey::SmoothingPeriod), "9"),
                                ],
                            ),
                            group(
                                t_config(ConfigKey::Levels),
                                vec![
                                    chk(t_concept(ConceptKey::Upper), true),
                                    num(t_concept(ConceptKey::Value), "70"),
                                    chk(t_concept(ConceptKey::Lower), true),
                                    num(t_concept(ConceptKey::Value), "30"),
                                ],
                            ),
                        ],
                    ),
                    tab(
                        "style",
                        t_config(ConfigKey::Style),
                        vec![
                            group(
                                t_toolbar_menu(ToolbarMenuKey::HeaderLines),
                                vec![
                                    color(t_concept(ConceptKey::MainLine), ThemeColor::Accent),
                                    slider(t_config(ConfigKey::WidthLabel), 24, "1.0"),
                                    dd(t_config(ConfigKey::Style), t_style(StyleKey::Solid)),
                                ],
                            ),
                            group(
                                t_config(ConfigKey::Fill),
                                vec![chk(t_concept(ConceptKey::BetweenLevels), true), slider(t_config(ConfigKey::FillOpacity), 12, "12%")],
                            ),
                            group(
                                t_settings(SettingsKey::Precision),
                                vec![dd(t_concept(ConceptKey::Digits), t_concept(ConceptKey::DefaultValue))],
                            ),
                        ],
                    ),
                    tab(
                        "signals",
                        t_toolbar(ToolbarTooltipKey::Signals),
                        vec![
                            group(
                                t_concept(ConceptKey::ConditionsPlural),
                                vec![
                                    chk(t_concept(ConceptKey::CrossesUp), true),
                                    chk(t_concept(ConceptKey::CrossesDown), true),
                                    chk(t_concept(ConceptKey::ExitsZone), false),
                                ],
                            ),
                            group(
                                t_concept(ConceptKey::Markers),
                                vec![
                                    dd(t_user_settings(UserSettingsKey::ControlPointShapeLabel), t_primitive_name(PrimitiveNameKey::Triangle)),
                                    num(t_user_settings(UserSettingsKey::ControlPointSizeLabel), "10"),
                                ],
                            ),
                        ],
                    ),
                    tab(
                        "visibility",
                        t_config(ConfigKey::Visibility),
                        vec![group(
                            t_config(ConfigKey::GroupVisibleOnTimeframes),
                            vec![
                                chk(t_settings(SettingsKey::TfRanges), true),
                                range(t_settings(SettingsKey::TfMinutes), 0, 100, t_concept(ConceptKey::MinutesRangeCap)),
                                range(t_settings(SettingsKey::TfHours), 0, 100, t_concept(ConceptKey::HoursRangeCap)),
                                range(t_settings(SettingsKey::TfDays), 0, 100, t_concept(ConceptKey::DaysRangeCap)),
                            ],
                        )],
                    ),
                ],
            },
            ConceptModal {
                id: "watchlist",
                name: t_shell(ShellKey::AppModalWatchlist),
                icon: "watchlist",
                rail: false,
                width: 520.0,
                height: 470.0,
                tabs: vec![
                    tab(
                        "overview",
                        t_modal(ModalKey::WatchlistOverview),
                        vec![group(
                            t_concept(ConceptKey::Instruments),
                            vec![table(
                                &[t_settings(SettingsKey::CompareInfoSymbol), t_modal(ModalKey::AlertPrice), t_concept(ConceptKey::ChangeAbbrev)],
                                vec![
                                    vec![txt("BTCUSDT"), txt("65 100.00"), toned("+1.24%", Tone::Good)],
                                    vec![txt("ETHUSDT"), txt("3 420.55"), toned("−0.42%", Tone::Bad)],
                                    vec![txt("SOLUSDT"), txt("182.30"), toned("+3.10%", Tone::Good)],
                                ],
                            )],
                        )],
                    ),
                    tab(
                        "groups",
                        t_modal(ModalKey::WatchlistGroups),
                        vec![group(
                            t_modal(ModalKey::WatchlistGroups),
                            vec![
                                pills(None, &[t_concept(ConceptKey::Crypto), t_concept(ConceptKey::Forex), t_concept(ConceptKey::UsStocks)], 0),
                                chk(t_concept(ConceptKey::SyncWithChart), true),
                            ],
                        )],
                    ),
                    tab(
                        "settings",
                        t_menu(MenuKey::OpenSettings),
                        vec![group(
                            t_concept(ConceptKey::ColumnsGroupTitle),
                            vec![
                                chk(t_modal(ModalKey::AlertPrice), true),
                                chk(t_concept(ConceptKey::Change), true),
                                chk(t_toolbar_menu(ToolbarMenuKey::HeaderVolume), false),
                                chk(t_concept(ConceptKey::Spread), false),
                            ],
                        )],
                    ),
                ],
            },
            ConceptModal {
                id: "alerts",
                name: t_shell(ShellKey::AppModalAlerts),
                icon: "alert",
                rail: false,
                width: 520.0,
                height: 470.0,
                tabs: vec![
                    tab(
                        "settings",
                        t_menu(MenuKey::OpenSettings),
                        vec![
                            group(
                                t_modal(ModalKey::AlertCondition),
                                vec![
                                    dd(t_concept(ConceptKey::TypeDropdownLabel), t_concept(ConceptKey::PriceCross)),
                                    num(t_concept(ConceptKey::Value), "65 400"),
                                    dd(t_config(ConfigKey::Direction), t_concept(ConceptKey::Up)),
                                ],
                            ),
                            group(
                                t_concept(ConceptKey::Expiry),
                                vec![chk(t_concept(ConceptKey::OneTime), true), dd(t_concept(ConceptKey::Expires), t_concept(ConceptKey::After24h))],
                            ),
                        ],
                    ),
                    tab(
                        "notify",
                        t_modal(ModalKey::AlertTabNotifications),
                        vec![
                            group(
                                t_toolbar_menu(ToolbarMenuKey::HeaderChannels),
                                vec![
                                    chk(t_concept(ConceptKey::Sound), true),
                                    chk(t_concept(ConceptKey::Popup), true),
                                    chk(t_concept(ConceptKey::Telegram), false),
                                    chk(t_concept(ConceptKey::Webhook), false),
                                ],
                            ),
                            group(t_modal(ModalKey::AlertMessage), vec![area(t_concept(ConceptKey::NoteAlertMessageTemplate))]),
                        ],
                    ),
                    tab(
                        "list",
                        t_concept(ConceptKey::List),
                        vec![group(
                            t_concept(ConceptKey::Active),
                            vec![table(
                                &[t_settings(SettingsKey::CompareInfoSymbol), t_modal(ModalKey::AlertCondition), t_concept(ConceptKey::Status)],
                                vec![
                                    vec![txt("BTCUSDT"), txt("> 65 400"), toned(t_concept(ConceptKey::AlertStatusWaiting), Tone::Neutral)],
                                    vec![txt("ETHUSDT"), txt("< 3 300"), toned(t_concept(ConceptKey::AlertStatusTriggered), Tone::Good)],
                                ],
                            )],
                        )],
                    ),
                ],
            },
            ConceptModal {
                id: "user_settings",
                name: t_shell(ShellKey::AppModalUserSettings),
                icon: "user",
                rail: false,
                width: 520.0,
                height: 440.0,
                tabs: vec![
                    tab(
                        "general",
                        t_concept(ConceptKey::GeneralTabLabel),
                        vec![
                            group(
                                t_concept(ConceptKey::Interface),
                                vec![
                                    dd(t_concept(ConceptKey::Language), t_concept(ConceptKey::RussianLanguageName)),
                                    pills(
                                        Some(t_wizard(WizardKey::Theme)),
                                        &[t_settings(SettingsKey::ThemeDark), t_settings(SettingsKey::ThemeLight)],
                                        0,
                                    ),
                                    chk(t_concept(ConceptKey::CompactMode), false),
                                ],
                            ),
                            group(t_concept(ConceptKey::TimeGroupTitle), vec![dd(t(ClockKey::Timezone), "UTC+0"), chk(t(ClockKey::Use24h), true)]),
                        ],
                    ),
                    tab(
                        "perf",
                        t_concept(ConceptKey::Performance),
                        vec![
                            group(
                                t_concept(ConceptKey::Rendering),
                                vec![
                                    dd(t_concept(ConceptKey::FrameRate), t_settings(SettingsKey::PrecisionAuto)),
                                    chk(t_config(ConfigKey::Smooth), true),
                                    chk(t_concept(ConceptKey::ShowFrameCounter), false),
                                ],
                            ),
                            group(
                                t_concept(ConceptKey::Data),
                                vec![num(t_concept(ConceptKey::HistoryDepthBars), "5000"), chk(t_concept(ConceptKey::CacheToDisk), true)],
                            ),
                        ],
                    ),
                ],
            },
            ConceptModal {
                id: "tags_tabs",
                name: t_shell(ShellKey::AppModalTagsTabs),
                icon: "layers",
                rail: false,
                width: 520.0,
                height: 440.0,
                tabs: vec![
                    tab(
                        "tabs",
                        t_concept(ConceptKey::TabsLabel),
                        vec![group(
                            t_concept(ConceptKey::ChartTabs),
                            vec![table(
                                &[t_concept(ConceptKey::Name), t_settings(SettingsKey::CompareInfoSymbol), t_concept(ConceptKey::TfAbbrev)],
                                vec![
                                    vec![txt("Untitled 1"), txt("BTCUSDT"), txt("1H")],
                                    vec![txt(t_concept(ConceptKey::DemoChartTabScalp)), txt("ETHUSDT"), txt(t_concept(ConceptKey::DemoTimeframe5m))],
                                ],
                            )],
                        )],
                    ),
                    tab(
                        "tags",
                        t_concept(ConceptKey::Tags),
                        vec![
                            group(
                                t_concept(ConceptKey::Tags),
                                vec![
                                    pills(
                                        None,
                                        &[t_concept(ConceptKey::TagIdea), t_concept(ConceptKey::TagWork), t_concept(ConceptKey::TagArchive)],
                                        0,
                                    ),
                                    color(t_concept(ConceptKey::TagColor), ThemeColor::Accent),
                                ],
                            ),
                            group(
                                t_concept(ConceptKey::SyncGroups),
                                vec![pills(
                                    None,
                                    &[t_settings(SettingsKey::ScalePosHidden), t_concept(ConceptKey::TagColorRed), t_concept(ConceptKey::TagColorBlue)],
                                    1,
                                )],
                            ),
                        ],
                    ),
                ],
            },
            ConceptModal {
                id: "trading",
                name: t_shell(ShellKey::AppModalTradingContainer),
                icon: "trading",
                rail: false,
                width: 520.0,
                height: 520.0,
                tabs: vec![
                    tab(
                        "map",
                        "MAP",
                        vec![group(
                            t_concept(ConceptKey::Panels),
                            vec![
                                pills(None, &["DOM", t_concept(ConceptKey::Tape), "L2", t_toolbar_menu(ToolbarMenuKey::HeaderPositions)], 0),
                                chk(t_concept(ConceptKey::ShowHeaders), true),
                            ],
                        )],
                    ),
                    tab(
                        "setup",
                        "SETUP",
                        vec![
                            group(
                                t_concept(ConceptKey::Account),
                                vec![
                                    dd(t_concept(ConceptKey::Exchange), "BINANCE"),
                                    dd(t_concept(ConceptKey::TypeDropdownLabel), "Spot"),
                                    num(t_concept(ConceptKey::Leverage), "1"),
                                ],
                            ),
                            group(
                                t_config(ConfigKey::PositionRiskLabel),
                                vec![num(t_concept(ConceptKey::RiskPerTradePct), "1.0"), chk(t_concept(ConceptKey::StopRequired), true)],
                            ),
                        ],
                    ),
                ],
            },
            ConceptModal {
                id: "search",
                name: t_shell(ShellKey::AppModalSearch),
                icon: "search",
                rail: false,
                width: 520.0,
                height: 420.0,
                tabs: vec![tab(
                    "symbols",
                    t_concept(ConceptKey::Instruments),
                    vec![
                        group(t_toolbar(ToolbarTooltipKey::Search), vec![area("BTC")]),
                        group(
                            t_config(ConfigKey::GroupResults),
                            vec![table(
                                &[t_settings(SettingsKey::CompareInfoSymbol), t_concept(ConceptKey::Exchange), t_concept(ConceptKey::TypeDropdownLabel)],
                                vec![
                                    vec![txt("BTCUSDT"), txt("BINANCE"), txt("spot")],
                                    vec![txt("BTCUSDT.P"), txt("BINANCE"), txt("perp")],
                                    vec![txt("BTCUSD"), txt("BYBIT"), txt("spot")],
                                ],
                            )],
                        ),
                    ],
                )],
            },
            ConceptModal {
                id: "template_name",
                name: t_shell(ShellKey::AppModalTemplateName),
                icon: "bookmark",
                rail: false,
                width: 380.0,
                height: 220.0,
                tabs: vec![tab(
                    "name",
                    t_concept(ConceptKey::Name),
                    vec![group(
                        t_concept(ConceptKey::TemplateNameGroupTitle),
                        vec![area(t_settings(SettingsKey::DefaultTemplateNamePrefix)), chk(t_concept(ConceptKey::MakeDefaultTemplate), false)],
                    )],
                )],
            },
        ]
    })
}

/// System templates a type offers, mirroring `registry.js`'s own
/// `templatesFor` (in turn mirroring `PrimitiveTemplate::system_templates`):
/// a ladder type (any type whose first tab carries a [`Row::Ladder`] row)
/// gets the four named sets, everything else a single "Standard".
pub fn templates_for(t: &ConceptType) -> &'static [&'static str] {
    const LADDER_SETS: &[&str] = &["Standard", "Extended", "Key levels only", "All levels"];
    const STANDARD_ONLY: &[&str] = &["Standard"];

    let has_ladder = t
        .tabs
        .first()
        .is_some_and(|tab| tab.groups.iter().any(|g| g.rows.iter().any(|r| matches!(r, Row::Ladder { .. }))));

    if has_ladder {
        LADDER_SETS
    } else {
        STANDARD_ONLY
    }
}
