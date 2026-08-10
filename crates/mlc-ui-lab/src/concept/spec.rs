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

use std::sync::OnceLock;

use mlc_core::i18n::{
    t_config, t_menu, t_settings, t_style, t_toolbar_menu, ConfigKey, MenuKey, SettingsKey, StyleKey, ToolbarMenuKey,
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

// ─── Shared groups/tabs ──────────────────────────────────────────────────────
// Mirrors registry.js's own TF / BEHAVIOUR / points / placement / labelRows
// / PLATE / LINE / FILL / EXTEND helpers.

/// Timeframe visibility: six numeric classes with range sliders — the
/// control the app actually ships, not chips.
fn tf_group() -> ConceptGroup {
    group(
        "Виден на таймфреймах",
        vec![
            chk("Диапазоны", true),
            range("Секунды", 0, 100, "1с–30с"),
            range("Минуты", 0, 100, "1м–45м"),
            range("Часы", 0, 100, "1Ч–12Ч"),
            range("Дни", 0, 100, "1Д–3Д"),
            range("Недели", 0, 100, "1Н"),
            range("Месяцы", 0, 100, "1М"),
        ],
    )
}

fn behaviour_group() -> ConceptGroup {
    group(
        "Поведение",
        vec![chk(t_menu(MenuKey::Lock), false), chk("Скрыть", false), chk("На всех графиках символа", false)],
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
    let mut rows = vec![table(&["Точка", "Цена", "Время"], table_rows)];
    if let Some(text) = note_text {
        rows.push(note(text));
    }
    group_hint("Точки", &names.len().to_string(), rows)
}

fn placement_tab(names: &[&str], note_text: Option<&str>) -> ConceptTab {
    tab("placement", "Размещение", vec![tf_group(), points_group(names, note_text), behaviour_group()])
}

fn label_rows(what: &str) -> Vec<Row> {
    vec![
        chk(t_settings(SettingsKey::Show), true),
        chk("Наследовать цвет линии", true),
        color_dim("Цвет подписи", ThemeColor::Accent),
        num("Размер", "12"),
        chk("Жирный", false),
        note(what),
    ]
}

fn plate_group() -> ConceptGroup {
    group(
        "Плашка",
        vec![
            chk("Плашка под подписью", true),
            color("Цвет плашки", ThemeColor::ModalBg),
            num("Скругление", "3"),
            num("Отступы", "4"),
        ],
    )
}

fn line_group(title: Option<&str>) -> ConceptGroup {
    let title = title.unwrap_or_else(|| t_toolbar_menu(ToolbarMenuKey::Line));
    group(
        title,
        vec![
            color(t_settings(SettingsKey::Color), ThemeColor::Accent),
            slider("Толщина", 24, "1.0"),
            dd(t_config(ConfigKey::Style), t_style(StyleKey::Solid)),
        ],
    )
}

fn fill_group(title: Option<&str>, on: bool) -> ConceptGroup {
    let title = title.unwrap_or_else(|| t_config(ConfigKey::Fill));
    group(
        title,
        vec![chk("Заливать", on), color(t_settings(SettingsKey::Color), ThemeColor::Accent), slider("Прозрачность", 10, "10%")],
    )
}

fn extend_group() -> ConceptGroup {
    group(t_config(ConfigKey::Extend), vec![chk("Влево", false), chk("Вправо", false)])
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
                    "Общее",
                    vec![
                        mode("", &["Моно", "Палитра"], 1),
                        slider("Толщина", 24, "1.0"),
                        dd("Стиль", "Сплошная"),
                        chk3(["Продлить влево", "Продлить вправо", "Трендовая линия"], [false, false, true]),
                    ],
                ),
                group("Палитры", vec![ramps()]),
                group("Заливка", vec![fillmaster("Заливка до следующего уровня", 10, "10%", true)]),
                group_hint(subject, &values.len().to_string(), vec![ladder(values, unit)]),
            ],
        ),
        tab(
            "labels",
            "Подписи",
            vec![
                group(
                    "Подписи уровней",
                    label_rows("Подпись берёт цвет своего уровня, пока включено наследование."),
                ),
                plate_group(),
            ],
        ),
        placement_tab(&["1", "2"], None),
    ]
}

fn wave_tabs() -> Vec<ConceptTab> {
    let mut wave_label_rows = label_rows("Имена берутся из степени волны.");
    wave_label_rows.push(chk("Показывать степень", false));

    vec![
        tab(
            "waves",
            "Волны",
            vec![
                line_group(None),
                group(
                    "Степень",
                    vec![
                        dd("Степень волны", "Minor"),
                        note("Subminuette · Minuette · Minute · Minor · Intermediate · Primary · Cycle · Supercycle"),
                    ],
                ),
                group(
                    "Нотация",
                    vec![
                        pills(None, &["1 2 3 4 5", "I II III IV V", "① ② ③ ④ ⑤"], 0),
                        note("Для коррекции — A B C, для комбинаций — W X Y (Z)."),
                    ],
                ),
                group_hint("Тип коррекции", "только у коррекции", vec![dd("Тип", "Зигзаг")]),
                fill_group(Some("Заливка между волнами"), false),
            ],
        ),
        tab(
            "labels",
            "Подписи",
            vec![
                group("Подписи волн", wave_label_rows),
                plate_group(),
                group_hint(
                    "Свои подписи",
                    "переопределение",
                    vec![table(
                        &["Волна", "Подпись"],
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
            "Пропорции",
            vec![
                group("Подтип", vec![dd("Паттерн", "Gartley"), note("Gartley · Bat · Butterfly · Crab · Shark · Свой")]),
                line_group(Some("Линии")),
                fill_group(Some("Тело паттерна"), true),
                group(
                    "Целевые отношения",
                    vec![table(
                        &["Отрезок", "Цель", "Факт", "Допуск"],
                        vec![
                            vec![txt("XA→AB"), txt("0.618"), toned("0.621", Tone::Good), txt("3%")],
                            vec![txt("AB→BC"), txt("0.382"), toned("0.394", Tone::Good), txt("5%")],
                            vec![txt("BC→CD"), txt("1.272"), toned("1.512", Tone::Bad), txt("5%")],
                            vec![txt("XA→AD"), txt("0.786"), toned("0.780", Tone::Good), txt("3%")],
                        ],
                    )],
                ),
                group(
                    "Оценка",
                    vec![
                        color("Цвет попадания", ThemeColor::Success),
                        color("Цвет промаха", ThemeColor::Danger),
                        chk("Красить линии по попаданию", false),
                    ],
                ),
            ],
        ),
        tab(
            "labels",
            "Подписи",
            vec![
                group("Подписи точек", label_rows("Имена точек — X A B C D.")),
                group(
                    "Значения на графике",
                    vec![
                        chk("Показывать отношения", true),
                        chk("Показывать длину отрезков", false),
                        num("Знаков после запятой", "3"),
                        dd("Сторона от отрезка", "Снаружи"),
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
            "Сделка",
            vec![
                group(
                    "Объём",
                    vec![
                        num("Количество", "1.00"),
                        num("Размер счёта", "10000"),
                        num("Риск на сделку, %", "1.0"),
                        chk("Считать объём от риска", false),
                    ],
                ),
                group(
                    "Зоны",
                    vec![
                        color("Зона стопа", ThemeColor::Danger),
                        color("Зона цели", ThemeColor::Success),
                        slider("Прозрачность", 18, "18%"),
                    ],
                ),
                group(
                    "Линии",
                    vec![
                        color("Вход", ThemeColor::Accent),
                        color("Стоп", ThemeColor::Danger),
                        color("Цель", ThemeColor::Success),
                        dd("Стиль", "Сплошная"),
                    ],
                ),
                group(
                    "Отображение",
                    vec![
                        pills(None, &["Валюта", "Тики", "Проценты"], 0),
                        chk("Показывать P&L", true),
                        chk("Показывать объём", true),
                    ],
                ),
                group_hint(
                    "Результат",
                    "считается",
                    vec![
                        stat("Риск", "−437.59", Some(Tone::Bad)),
                        stat("Цель", "+875.19", Some(Tone::Good)),
                        hr(),
                        stat("Отношение", "1 : 2.00", None),
                        note("Не редактируется — считается от точек входа, стопа и цели."),
                    ],
                ),
            ],
        ),
        tab(
            "labels",
            "Подписи",
            vec![
                group("Подписи", label_rows("Подписи входа, стопа и цели.")),
                group(
                    "Что писать",
                    vec![
                        chk("Имя точки", true),
                        chk("Цену", true),
                        chk("Расстояние в %", true),
                        chk("Сумму риска и цели", false),
                        num("Знаков после запятой", "2"),
                    ],
                ),
                plate_group(),
            ],
        ),
        placement_tab(&["Вход", "Стоп", "Цель"], None),
    ]
}

fn measure_tabs() -> Vec<ConceptTab> {
    vec![
        tab(
            "show",
            "Что показывать",
            vec![
                group(
                    "Величины",
                    vec![
                        chk("Изменение цены", true),
                        chk("Проценты", true),
                        chk("Число баров", true),
                        chk("Время", false),
                        chk("Объём за диапазон", false),
                        chk("Средняя цена", false),
                        chk("Угол наклона", false),
                    ],
                ),
                group(
                    "Формат",
                    vec![
                        num("Знаков после запятой", "2"),
                        chk("Разделитель разрядов", true),
                        dd("Единица объёма", "Авто (K / M)"),
                    ],
                ),
                group(
                    "Линия и стрелка",
                    vec![color("Цвет", ThemeColor::Accent), slider("Толщина", 24, "1.0"), chk("Стрелка на конце", true)],
                ),
                group(
                    "Область",
                    vec![
                        chk("Заливать", true),
                        slider("Прозрачность", 15, "15%"),
                        chk("Красить по знаку", true),
                        color("Рост", ThemeColor::Success),
                        color("Падение", ThemeColor::Danger),
                    ],
                ),
            ],
        ),
        tab(
            "labels",
            "Подписи",
            vec![
                group(
                    "Текст",
                    vec![num("Размер", "12"), color("Цвет", ThemeColor::TextPrimary), chk("Наследовать цвет линии", false)],
                ),
                group(
                    "Плашка",
                    vec![
                        pills(None, &["Сверху", "В центре", "Снизу"], 1),
                        chk("Фон плашки", true),
                        color("Цвет фона", ThemeColor::ModalBg),
                    ],
                ),
            ],
        ),
        placement_tab(&["Начало", "Конец"], None),
    ]
}

fn text_tabs() -> Vec<ConceptTab> {
    vec![
        tab(
            "text",
            "Текст",
            vec![
                group(
                    "Содержимое",
                    vec![
                        area("Пробой уровня, ждём ретест"),
                        note("Подстановки: {цена} {время} {символ} {таймфрейм}"),
                    ],
                ),
                group(
                    "Шрифт",
                    vec![
                        dd("Гарнитура", "Inter"),
                        num("Размер", "13"),
                        pills(Some("Начертание"), &["Ж", "К", "Ч"], 0),
                        color("Цвет", ThemeColor::TextPrimary),
                    ],
                ),
                group(
                    "Раскладка",
                    vec![
                        pills(None, &["По левому", "По центру", "По правому"], 0),
                        chk("Перенос по словам", true),
                        chk("Ширина по содержимому", true),
                        num("Максимальная ширина", "240"),
                    ],
                ),
                group(
                    "Плашка",
                    vec![
                        chk("Рамка", true),
                        color("Цвет рамки", ThemeColor::Accent),
                        chk("Фон", true),
                        color("Цвет фона", ThemeColor::ModalBg),
                        num("Скругление", "4"),
                        num("Отступы", "6"),
                        chk("Тень", false),
                    ],
                ),
                group_hint(
                    "Указатель",
                    "выноска, ценовая заметка",
                    vec![chk("Хвостик к точке", true), dd("Наконечник", "Стрелка")],
                ),
            ],
        ),
        tab(
            "placement",
            "Размещение",
            vec![
                group(
                    "Якорь",
                    vec![
                        pills(None, &["К цене и бару", "К бару", "К экрану"], 0),
                        pills(None, &["Сверху", "Справа", "Снизу", "Слева"], 1),
                    ],
                ),
                tf_group(),
                points_group(&["Якорь", "Плашка"], None),
                behaviour_group(),
            ],
        ),
    ]
}

fn table_tabs() -> Vec<ConceptTab> {
    vec![
        tab(
            "cells",
            "Ячейки",
            vec![
                group("Размер", vec![stepper("Строк", 4), stepper("Колонок", 3)]),
                group_hint(
                    "Содержимое",
                    "4 × 3",
                    vec![cells(
                        3,
                        vec![
                            vec!["Уровень", "Цена", "Комментарий"],
                            vec!["Сопротивление", "65 400", "дневной максимум"],
                            vec!["Вход", "65 100", "по ретесту"],
                            vec!["Стоп", "64 660", "под лоем"],
                        ],
                    )],
                ),
                group(
                    "Колонки",
                    vec![table(
                        &["Колонка", "Выравнивание", "Ширина"],
                        vec![
                            vec![txt("1"), txt("По левому"), txt("авто")],
                            vec![txt("2"), txt("По правому"), txt("70")],
                            vec![txt("3"), txt("По левому"), txt("авто")],
                        ],
                    )],
                ),
                group(
                    "Сетка и шапка",
                    vec![
                        chk("Показывать линии", true),
                        color("Цвет линий", ThemeColor::BorderLight),
                        chk("Первая строка — шапка", true),
                        chk("Чередовать строки", false),
                    ],
                ),
                group(
                    "Текст и фон",
                    vec![
                        num("Размер", "12"),
                        color("Цвет текста", ThemeColor::TextPrimary),
                        color("Фон таблицы", ThemeColor::ToolbarBg),
                        slider("Прозрачность", 90, "90%"),
                    ],
                ),
            ],
        ),
        tab(
            "placement",
            "Размещение",
            vec![
                group(
                    "Якорь",
                    vec![
                        pills(None, &["К цене и бару", "К экрану"], 1),
                        pills(None, &["Слева сверху", "Справа сверху", "Слева снизу", "Справа снизу"], 1),
                    ],
                ),
                tf_group(),
                behaviour_group(),
            ],
        ),
    ]
}

fn shape_tabs(subject: &'static str, extra: Vec<ConceptGroup>) -> Vec<ConceptTab> {
    let mut groups = vec![line_group(Some("Контур"))];
    groups.extend(extra);
    groups.push(fill_group(Some("Заливка"), true));
    groups.push(extend_group());
    groups.push(group(
        "Текст на фигуре",
        vec![chk("Показывать", false), num("Размер", "12"), dd("Положение", "По центру")],
    ));
    vec![
        tab("shape", subject, groups),
        placement_tab(
            &["1", "2"],
            Some("У полилинии список растёт по числу точек — та же таблица со своей прокруткой."),
        ),
    ]
}

fn regression_tabs() -> Vec<ConceptTab> {
    vec![
        tab(
            "sigma",
            "Отклонения",
            vec![
                group("Расчёт", vec![dd("Источник", "Close"), dd("Период", "По точкам"), num("Число баров", "120")]),
                group(
                    "Сигмы",
                    vec![
                        table(
                            &["", "Множитель", "Цвет", "Заливка"],
                            vec![
                                vec![txt("±1σ"), txt("1.0"), swatch(ThemeColor::Accent), check(true)],
                                vec![txt("±2σ"), txt("2.0"), swatch(ThemeColor::Warning), check(true)],
                                vec![txt("±3σ"), txt("3.0"), swatch(ThemeColor::Danger), check(false)],
                            ],
                        ),
                        slider("Прозрачность заливки", 10, "10%"),
                    ],
                ),
                group(
                    "Базовая линия",
                    vec![chk("Показывать", true), color("Цвет", ThemeColor::Accent), slider("Толщина", 30, "1.5")],
                ),
                extend_group(),
                group_hint(
                    "Показатели",
                    "считаются",
                    vec![
                        chk("Показывать наклон", false),
                        chk("Показывать R²", true),
                        stat("Наклон", "+18.4 / бар", None),
                        stat("R²", "0.87", None),
                    ],
                ),
            ],
        ),
        placement_tab(&["Начало", "Конец"], None),
    ]
}

fn brush_tabs() -> Vec<ConceptTab> {
    vec![
        tab(
            "stroke",
            "Мазок",
            vec![
                group(
                    "Мазок",
                    vec![
                        color("Цвет", ThemeColor::Accent),
                        slider("Размер", 35, "3.0"),
                        slider("Прозрачность", 100, "100%"),
                        dd("Стиль линии", "Сплошная"),
                    ],
                ),
                group(
                    "Форма",
                    vec![
                        chk("Сглаживание", true),
                        slider("Сила сглаживания", 50, "50%"),
                        chk("Скруглять концы", true),
                        chk("Замыкать контур", false),
                    ],
                ),
                fill_group(Some("Заливка у замкнутого контура"), false),
            ],
        ),
        placement_tab(
            &["Начало", "Конец"],
            Some("У кисти точки не правятся поштучно — показываются рамка мазка и число точек."),
        ),
    ]
}

fn icon_tabs() -> Vec<ConceptTab> {
    vec![
        tab(
            "icon",
            "Значок",
            vec![
                group(
                    "Символ",
                    vec![icons(
                        &["▲", "▼", "⚑", "★", "●", "◆", "✕", "✓", "❗", "❓", "💡", "🔒", "📌", "🔔", "👁", "💬"],
                        0,
                    )],
                ),
                group(
                    "Отрисовка",
                    vec![color("Цвет", ThemeColor::Success), slider("Размер", 40, "18"), chk("Обводка", false)],
                ),
                group(
                    "Подпись рядом",
                    vec![
                        chk("Показывать", true),
                        dd("Текст", "Вход по тренду"),
                        num("Размер", "12"),
                        dd("Сторона", "Справа"),
                    ],
                ),
            ],
        ),
        placement_tab(&["Точка"], None),
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
                name: "Fib Retracement",
                tabs: ladder_tabs("Уровни", &FIB_LEVELS, LadderUnit::Ratio),
            },
            ConceptType { id: "gann_fan", name: "Gann Fan", tabs: ladder_tabs("Углы", &GANN_ANGLES, LadderUnit::Gann) },
            ConceptType {
                id: "pitchfork",
                name: "Pitchfork",
                tabs: ladder_tabs("Уровни", &PITCHFORK_LEVELS, LadderUnit::Ratio),
            },
            ConceptType { id: "elliott_impulse", name: "Elliott Impulse", tabs: wave_tabs() },
            ConceptType { id: "xabcd_pattern", name: "XABCD Pattern", tabs: pattern_tabs() },
            ConceptType { id: "long_position", name: "Long Position", tabs: position_tabs() },
            ConceptType { id: "price_date_range", name: "Price & Date Range", tabs: measure_tabs() },
            ConceptType { id: "callout", name: "Callout", tabs: text_tabs() },
            ConceptType { id: "table", name: "Table", tabs: table_tabs() },
            ConceptType { id: "rectangle", name: "Rectangle", tabs: shape_tabs("Фигура", Vec::new()) },
            ConceptType {
                id: "parallel_channel",
                name: "Parallel Channel",
                tabs: shape_tabs(
                    "Канал",
                    vec![group(
                        "Средняя линия",
                        vec![
                            chk("Показывать", true),
                            color("Цвет", ThemeColor::TextMuted),
                            dd("Стиль", "Пунктир"),
                        ],
                    )],
                ),
            },
            ConceptType { id: "regression_trend", name: "Regression Trend", tabs: regression_tabs() },
            ConceptType { id: "brush", name: "Brush", tabs: brush_tabs() },
            ConceptType { id: "flag", name: "Flag", tabs: icon_tabs() },
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
                name: "Настройки графика",
                icon: "settings",
                rail: true,
                width: 620.0,
                height: 560.0,
                tabs: vec![
                    tab_icon(
                        "instrument",
                        "Инструмент",
                        "candlestick",
                        vec![
                            group(
                                "Свечи",
                                vec![
                                    color("Тело роста", ThemeColor::Success),
                                    color("Тело падения", ThemeColor::Danger),
                                    chk("Полые свечи", false),
                                    chk("Фитили", true),
                                    color("Цвет фитиля", ThemeColor::TextMuted),
                                ],
                            ),
                            group("Границы", vec![chk("Показывать", true), slider("Толщина", 20, "1.0")]),
                            group(
                                "Прочее",
                                vec![
                                    chk("Последняя цена", true),
                                    chk("Предыдущее закрытие", false),
                                    chk("Высокая и низкая", false),
                                ],
                            ),
                        ],
                    ),
                    tab_icon(
                        "status",
                        "Статус-строка",
                        "legend",
                        vec![
                            group(
                                "Показывать",
                                vec![
                                    chk("Тикер", true),
                                    chk("Значения OHLC", true),
                                    chk("Изменение", true),
                                    chk("Объём", true),
                                    chk("Индикаторы", true),
                                ],
                            ),
                            group(
                                "Оформление",
                                vec![num("Размер", "12"), dd("Положение", "Слева сверху"), slider("Прозрачность фона", 60, "60%")],
                            ),
                        ],
                    ),
                    tab_icon(
                        "scales",
                        "Шкалы и линии",
                        "grid",
                        vec![
                            group(
                                "Сетка",
                                vec![
                                    chk("Вертикальные", true),
                                    chk("Горизонтальные", true),
                                    color("Цвет", ThemeColor::BorderLight),
                                    dd("Стиль", "Сплошная"),
                                ],
                            ),
                            group(
                                "Ценовая шкала",
                                vec![dd("Режим", "Авто"), num("Ширина", "70"), chk("Проценты", false), chk("Обратная", false)],
                            ),
                            group(
                                "Временная шкала",
                                vec![chk("Показывать секунды", false), chk("Показывать дату", true), num("Высота", "30")],
                            ),
                            group("Перекрестье", vec![color("Цвет", ThemeColor::TextMuted), dd("Стиль", "Пунктир")]),
                        ],
                    ),
                    tab_icon(
                        "look",
                        "Оформление",
                        "palette",
                        vec![
                            group("Фон", vec![pills(None, &["Сплошной", "Градиент"], 0), color("Цвет", ThemeColor::ToolbarBg)]),
                            group(
                                "Водяной знак",
                                vec![chk("Показывать", true), slider("Прозрачность", 20, "20%"), num("Размер", "52")],
                            ),
                            group(
                                "Поля",
                                vec![
                                    slider("Сверху", 10, "10%"),
                                    slider("Снизу", 8, "8%"),
                                    num("Отступ справа, баров", "10"),
                                ],
                            ),
                        ],
                    ),
                ],
            },
            ConceptModal {
                id: "indicator_settings",
                name: "Настройки индикатора",
                icon: "indicators",
                rail: false,
                width: 520.0,
                height: 470.0,
                tabs: vec![
                    tab(
                        "inputs",
                        "Параметры",
                        vec![
                            group(
                                "Вход",
                                vec![
                                    dd("Источник", "Close"),
                                    num("Период", "14"),
                                    dd("Сглаживание", "EMA"),
                                    num("Период сглаживания", "9"),
                                ],
                            ),
                            group(
                                "Уровни",
                                vec![chk("Верхний", true), num("Значение", "70"), chk("Нижний", true), num("Значение", "30")],
                            ),
                        ],
                    ),
                    tab(
                        "style",
                        "Стиль",
                        vec![
                            group(
                                "Линии",
                                vec![color("Основная", ThemeColor::Accent), slider("Толщина", 24, "1.0"), dd("Стиль", "Сплошная")],
                            ),
                            group("Заливка", vec![chk("Между уровнями", true), slider("Прозрачность", 12, "12%")]),
                            group("Точность", vec![dd("Знаков", "По умолчанию")]),
                        ],
                    ),
                    tab(
                        "signals",
                        "Сигналы",
                        vec![
                            group(
                                "Условия",
                                vec![
                                    chk("Пересечение вверх", true),
                                    chk("Пересечение вниз", true),
                                    chk("Выход из зоны", false),
                                ],
                            ),
                            group("Отметки", vec![dd("Форма", "Треугольник"), num("Размер", "10")]),
                        ],
                    ),
                    tab(
                        "visibility",
                        "Видимость",
                        vec![group(
                            "Виден на таймфреймах",
                            vec![
                                chk("Диапазоны", true),
                                range("Минуты", 0, 100, "1м–45м"),
                                range("Часы", 0, 100, "1Ч–12Ч"),
                                range("Дни", 0, 100, "1Д–3Д"),
                            ],
                        )],
                    ),
                ],
            },
            ConceptModal {
                id: "watchlist",
                name: "Список наблюдения",
                icon: "watchlist",
                rail: false,
                width: 520.0,
                height: 470.0,
                tabs: vec![
                    tab(
                        "overview",
                        "Обзор",
                        vec![group(
                            "Инструменты",
                            vec![table(
                                &["Символ", "Цена", "Изм."],
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
                        "Группы",
                        vec![group(
                            "Группы",
                            vec![pills(None, &["Крипта", "Форекс", "Акции США"], 0), chk("Синхронизировать с графиком", true)],
                        )],
                    ),
                    tab(
                        "settings",
                        "Настройки",
                        vec![group(
                            "Колонки",
                            vec![chk("Цена", true), chk("Изменение", true), chk("Объём", false), chk("Спред", false)],
                        )],
                    ),
                ],
            },
            ConceptModal {
                id: "alerts",
                name: "Алерты",
                icon: "alert",
                rail: false,
                width: 520.0,
                height: 470.0,
                tabs: vec![
                    tab(
                        "settings",
                        "Настройки",
                        vec![
                            group(
                                "Условие",
                                vec![dd("Тип", "Пересечение цены"), num("Значение", "65 400"), dd("Направление", "Вверх")],
                            ),
                            group("Срок", vec![chk("Однократный", true), dd("Истекает", "Через сутки")]),
                        ],
                    ),
                    tab(
                        "notify",
                        "Уведомления",
                        vec![
                            group(
                                "Каналы",
                                vec![
                                    chk("Звук", true),
                                    chk("Всплывающее окно", true),
                                    chk("Телеграм", false),
                                    chk("Вебхук", false),
                                ],
                            ),
                            group("Сообщение", vec![area("{символ} пробил {значение} на {таймфрейм}")]),
                        ],
                    ),
                    tab(
                        "list",
                        "Список",
                        vec![group(
                            "Активные",
                            vec![table(
                                &["Символ", "Условие", "Статус"],
                                vec![
                                    vec![txt("BTCUSDT"), txt("> 65 400"), toned("ждёт", Tone::Neutral)],
                                    vec![txt("ETHUSDT"), txt("< 3 300"), toned("сработал", Tone::Good)],
                                ],
                            )],
                        )],
                    ),
                ],
            },
            ConceptModal {
                id: "user_settings",
                name: "Настройки приложения",
                icon: "user",
                rail: false,
                width: 520.0,
                height: 440.0,
                tabs: vec![
                    tab(
                        "general",
                        "Общие",
                        vec![
                            group(
                                "Интерфейс",
                                vec![
                                    dd("Язык", "Русский"),
                                    pills(Some("Тема"), &["Тёмная", "Светлая"], 0),
                                    chk("Компактный режим", false),
                                ],
                            ),
                            group("Время", vec![dd("Часовой пояс", "UTC+0"), chk("24-часовой формат", true)]),
                        ],
                    ),
                    tab(
                        "perf",
                        "Производительность",
                        vec![
                            group(
                                "Отрисовка",
                                vec![
                                    dd("Частота кадров", "Авто"),
                                    chk("Сглаживание", true),
                                    chk("Показывать счётчик кадров", false),
                                ],
                            ),
                            group(
                                "Данные",
                                vec![num("Глубина истории, баров", "5000"), chk("Кешировать на диск", true)],
                            ),
                        ],
                    ),
                ],
            },
            ConceptModal {
                id: "tags_tabs",
                name: "Вкладки и метки",
                icon: "layers",
                rail: false,
                width: 520.0,
                height: 440.0,
                tabs: vec![
                    tab(
                        "tabs",
                        "Вкладки",
                        vec![group(
                            "Вкладки графиков",
                            vec![table(
                                &["Имя", "Символ", "ТФ"],
                                vec![
                                    vec![txt("Untitled 1"), txt("BTCUSDT"), txt("1H")],
                                    vec![txt("Скальп"), txt("ETHUSDT"), txt("5м")],
                                ],
                            )],
                        )],
                    ),
                    tab(
                        "tags",
                        "Метки",
                        vec![
                            group(
                                "Метки",
                                vec![pills(None, &["Идея", "Работа", "Архив"], 0), color("Цвет метки", ThemeColor::Accent)],
                            ),
                            group("Группы синхронизации", vec![pills(None, &["Нет", "Красная", "Синяя"], 1)]),
                        ],
                    ),
                ],
            },
            ConceptModal {
                id: "trading",
                name: "Торговый контейнер",
                icon: "trading",
                rail: false,
                width: 520.0,
                height: 520.0,
                tabs: vec![
                    tab(
                        "map",
                        "MAP",
                        vec![group(
                            "Панели",
                            vec![pills(None, &["DOM", "Лента", "L2", "Позиции"], 0), chk("Показывать заголовки", true)],
                        )],
                    ),
                    tab(
                        "setup",
                        "SETUP",
                        vec![
                            group("Счёт", vec![dd("Биржа", "BINANCE"), dd("Тип", "Spot"), num("Плечо", "1")]),
                            group("Риск", vec![num("Риск на сделку, %", "1.0"), chk("Стоп обязателен", true)]),
                        ],
                    ),
                ],
            },
            ConceptModal {
                id: "search",
                name: "Поиск инструмента",
                icon: "search",
                rail: false,
                width: 520.0,
                height: 420.0,
                tabs: vec![tab(
                    "symbols",
                    "Инструменты",
                    vec![
                        group("Поиск", vec![area("BTC")]),
                        group(
                            "Результаты",
                            vec![table(
                                &["Символ", "Биржа", "Тип"],
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
                name: "Сохранить шаблон",
                icon: "bookmark",
                rail: false,
                width: 380.0,
                height: 220.0,
                tabs: vec![tab(
                    "name",
                    "Имя",
                    vec![group(
                        "Название шаблона",
                        vec![area("Мой шаблон"), chk("Сделать шаблоном по умолчанию", false)],
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
