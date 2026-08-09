//! Primitive Configuration System
//!
//! Provides a unified way to configure primitives through:
//! - Inline toolbar (quick settings: color, width, style)
//! - Context menu (clone, delete, lock, visibility, layer order)
//! - Settings modal (full configuration with tabs: Style, Coordinates, Visibility)
//!
//! # Architecture
//!
//! Each primitive exposes its configurable properties through the `Primitive` trait's
//! `style_properties()` / `level_properties()` / `text_properties()` methods, applied
//! back via `DrawingManager::apply_property` (dispatched by `PropertyKind`).
//! Properties are described as `ConfigProperty` which includes:
//! - Property ID and display name
//! - Property type (color, number, boolean, select, levels, etc.)
//! - Current value
//! - Constraints (min/max, options, etc.)

use serde::{Deserialize, Serialize};
use crate::i18n::{Language, WaveDegreeKey, ConfigKey, LabelPositionKey, current_language};

// =============================================================================
// Elliott Wave Degree and Notation
// =============================================================================

/// Elliott Wave degree - determines the notation style for wave labels
/// From largest (multi-century) to smallest (minutes)
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum WaveDegree {
    /// Multi-century timeframe
    Supermillennium,
    /// Multi-century timeframe
    Millennium,
    /// Multi-century timeframe
    Submillennium,
    /// Multi-decade (50+ years)
    GrandSupercycle,
    /// Multi-decade (years to decades)
    Supercycle,
    /// One to several years
    Cycle,
    /// Months to a year
    Primary,
    /// Weeks to months
    #[default]
    Intermediate,
    /// Weeks
    Minor,
    /// Days
    Minute,
    /// Hours
    Minuette,
    /// Minutes
    Subminuette,
    /// Sub-minute
    Micro,
    /// Sub-minute
    Submicro,
    /// Smallest observable
    Miniscule,
}

impl WaveDegree {
    /// Get impulse wave labels (1-5) for this degree
    pub fn impulse_labels(&self) -> [&'static str; 5] {
        match self {
            WaveDegree::Supermillennium => ["(((I)))", "(((II)))", "(((III)))", "(((IV)))", "(((V)))"],
            WaveDegree::Millennium => ["((I))", "((II))", "((III))", "((IV))", "((V))"],
            WaveDegree::Submillennium => ["(I)", "(II)", "(III)", "(IV)", "(V)"],
            WaveDegree::GrandSupercycle => ["I", "II", "III", "IV", "V"],  // Large Roman, no brackets
            WaveDegree::Supercycle => ["(I)", "(II)", "(III)", "(IV)", "(V)"],  // Circled Roman
            WaveDegree::Cycle => ["I", "II", "III", "IV", "V"],  // Roman numerals
            WaveDegree::Primary => ["①", "②", "③", "④", "⑤"],  // Circled numbers
            WaveDegree::Intermediate => ["(1)", "(2)", "(3)", "(4)", "(5)"],  // Parenthesized
            WaveDegree::Minor => ["1", "2", "3", "4", "5"],  // Plain numbers
            WaveDegree::Minute => ["i", "ii", "iii", "iv", "v"],  // Lowercase Roman
            WaveDegree::Minuette => ["(i)", "(ii)", "(iii)", "(iv)", "(v)"],  // Parenthesized lowercase
            WaveDegree::Subminuette => ["((i))", "((ii))", "((iii))", "((iv))", "((v))"],  // Double parenthesized
            WaveDegree::Micro => ["i", "ii", "iii", "iv", "v"],  // Tiny lowercase Roman
            WaveDegree::Submicro => ["(i)", "(ii)", "(iii)", "(iv)", "(v)"],
            WaveDegree::Miniscule => ["((i))", "((ii))", "((iii))", "((iv))", "((v))"],
        }
    }

    /// Get corrective wave labels (A-B-C) for this degree
    pub fn corrective_labels(&self) -> [&'static str; 3] {
        match self {
            WaveDegree::Supermillennium => ["(((A)))", "(((B)))", "(((C)))"],
            WaveDegree::Millennium => ["((A))", "((B))", "((C))"],
            WaveDegree::Submillennium => ["(A)", "(B)", "(C)"],
            WaveDegree::GrandSupercycle => ["A", "B", "C"],  // Large letters
            WaveDegree::Supercycle => ["(A)", "(B)", "(C)"],  // Circled
            WaveDegree::Cycle => ["A", "B", "C"],  // Plain uppercase
            WaveDegree::Primary => ["Ⓐ", "Ⓑ", "Ⓒ"],  // Circled letters
            WaveDegree::Intermediate => ["(A)", "(B)", "(C)"],  // Parenthesized
            WaveDegree::Minor => ["A", "B", "C"],  // Plain uppercase
            WaveDegree::Minute => ["a", "b", "c"],  // Lowercase
            WaveDegree::Minuette => ["(a)", "(b)", "(c)"],  // Parenthesized lowercase
            WaveDegree::Subminuette => ["((a))", "((b))", "((c))"],  // Double parenthesized
            WaveDegree::Micro => ["a", "b", "c"],
            WaveDegree::Submicro => ["(a)", "(b)", "(c)"],
            WaveDegree::Miniscule => ["((a))", "((b))", "((c))"],
        }
    }

    /// Get triangle wave labels (A-E) for this degree
    pub fn triangle_labels(&self) -> [&'static str; 5] {
        match self {
            WaveDegree::Supermillennium => ["(((A)))", "(((B)))", "(((C)))", "(((D)))", "(((E)))"],
            WaveDegree::Millennium => ["((A))", "((B))", "((C))", "((D))", "((E))"],
            WaveDegree::Submillennium => ["(A)", "(B)", "(C)", "(D)", "(E)"],
            WaveDegree::GrandSupercycle => ["A", "B", "C", "D", "E"],
            WaveDegree::Supercycle => ["(A)", "(B)", "(C)", "(D)", "(E)"],
            WaveDegree::Cycle => ["A", "B", "C", "D", "E"],
            WaveDegree::Primary => ["Ⓐ", "Ⓑ", "Ⓒ", "Ⓓ", "Ⓔ"],
            WaveDegree::Intermediate => ["(A)", "(B)", "(C)", "(D)", "(E)"],
            WaveDegree::Minor => ["A", "B", "C", "D", "E"],
            WaveDegree::Minute => ["a", "b", "c", "d", "e"],
            WaveDegree::Minuette => ["(a)", "(b)", "(c)", "(d)", "(e)"],
            WaveDegree::Subminuette => ["((a))", "((b))", "((c))", "((d))", "((e))"],
            WaveDegree::Micro => ["a", "b", "c", "d", "e"],
            WaveDegree::Submicro => ["(a)", "(b)", "(c)", "(d)", "(e)"],
            WaveDegree::Miniscule => ["((a))", "((b))", "((c))", "((d))", "((e))"],
        }
    }

    /// Get combo wave labels (W-X-Y or W-X-Y-X-Z) for this degree
    pub fn combo_labels(&self) -> [&'static str; 5] {
        match self {
            WaveDegree::Supermillennium => ["(((W)))", "(((X)))", "(((Y)))", "(((X)))", "(((Z)))"],
            WaveDegree::Millennium => ["((W))", "((X))", "((Y))", "((X))", "((Z))"],
            WaveDegree::Submillennium => ["(W)", "(X)", "(Y)", "(X)", "(Z)"],
            WaveDegree::GrandSupercycle => ["W", "X", "Y", "X", "Z"],
            WaveDegree::Supercycle => ["(W)", "(X)", "(Y)", "(X)", "(Z)"],
            WaveDegree::Cycle => ["W", "X", "Y", "X", "Z"],
            WaveDegree::Primary => ["Ⓦ", "Ⓧ", "Ⓨ", "Ⓧ", "Ⓩ"],
            WaveDegree::Intermediate => ["(W)", "(X)", "(Y)", "(X)", "(Z)"],
            WaveDegree::Minor => ["W", "X", "Y", "X", "Z"],
            WaveDegree::Minute => ["w", "x", "y", "x", "z"],
            WaveDegree::Minuette => ["(w)", "(x)", "(y)", "(x)", "(z)"],
            WaveDegree::Subminuette => ["((w))", "((x))", "((y))", "((x))", "((z))"],
            WaveDegree::Micro => ["w", "x", "y", "x", "z"],
            WaveDegree::Submicro => ["(w)", "(x)", "(y)", "(x)", "(z)"],
            WaveDegree::Miniscule => ["((w))", "((x))", "((y))", "((x))", "((z))"],
        }
    }

    /// Get display name for this degree using current language
    pub fn display_name(&self) -> &'static str {
        self.display_name_for(current_language())
    }

    /// Get display name for this degree in specified language
    pub fn display_name_for(&self, lang: Language) -> &'static str {
        self.i18n_key().get(lang)
    }

    /// Get i18n key for this degree
    pub fn i18n_key(&self) -> WaveDegreeKey {
        match self {
            WaveDegree::Supermillennium => WaveDegreeKey::Supermillennium,
            WaveDegree::Millennium => WaveDegreeKey::Millennium,
            WaveDegree::Submillennium => WaveDegreeKey::Submillennium,
            WaveDegree::GrandSupercycle => WaveDegreeKey::GrandSupercycle,
            WaveDegree::Supercycle => WaveDegreeKey::Supercycle,
            WaveDegree::Cycle => WaveDegreeKey::Cycle,
            WaveDegree::Primary => WaveDegreeKey::Primary,
            WaveDegree::Intermediate => WaveDegreeKey::Intermediate,
            WaveDegree::Minor => WaveDegreeKey::Minor,
            WaveDegree::Minute => WaveDegreeKey::Minute,
            WaveDegree::Minuette => WaveDegreeKey::Minuette,
            WaveDegree::Subminuette => WaveDegreeKey::Subminuette,
            WaveDegree::Micro => WaveDegreeKey::Micro,
            WaveDegree::Submicro => WaveDegreeKey::Submicro,
            WaveDegree::Miniscule => WaveDegreeKey::Miniscule,
        }
    }

    /// Get all degrees in order from largest to smallest
    pub fn all() -> &'static [WaveDegree] {
        &[
            WaveDegree::Supermillennium,
            WaveDegree::Millennium,
            WaveDegree::Submillennium,
            WaveDegree::GrandSupercycle,
            WaveDegree::Supercycle,
            WaveDegree::Cycle,
            WaveDegree::Primary,
            WaveDegree::Intermediate,
            WaveDegree::Minor,
            WaveDegree::Minute,
            WaveDegree::Minuette,
            WaveDegree::Subminuette,
            WaveDegree::Micro,
            WaveDegree::Submicro,
            WaveDegree::Miniscule,
        ]
    }

    /// Get serialization value
    pub fn as_str(&self) -> &'static str {
        match self {
            WaveDegree::Supermillennium => "supermillennium",
            WaveDegree::Millennium => "millennium",
            WaveDegree::Submillennium => "submillennium",
            WaveDegree::GrandSupercycle => "grand_supercycle",
            WaveDegree::Supercycle => "supercycle",
            WaveDegree::Cycle => "cycle",
            WaveDegree::Primary => "primary",
            WaveDegree::Intermediate => "intermediate",
            WaveDegree::Minor => "minor",
            WaveDegree::Minute => "minute",
            WaveDegree::Minuette => "minuette",
            WaveDegree::Subminuette => "subminuette",
            WaveDegree::Micro => "micro",
            WaveDegree::Submicro => "submicro",
            WaveDegree::Miniscule => "miniscule",
        }
    }

    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "supermillennium" => Some(WaveDegree::Supermillennium),
            "millennium" => Some(WaveDegree::Millennium),
            "submillennium" => Some(WaveDegree::Submillennium),
            "grand_supercycle" => Some(WaveDegree::GrandSupercycle),
            "supercycle" => Some(WaveDegree::Supercycle),
            "cycle" => Some(WaveDegree::Cycle),
            "primary" => Some(WaveDegree::Primary),
            "intermediate" => Some(WaveDegree::Intermediate),
            "minor" => Some(WaveDegree::Minor),
            "minute" => Some(WaveDegree::Minute),
            "minuette" => Some(WaveDegree::Minuette),
            "subminuette" => Some(WaveDegree::Subminuette),
            "micro" => Some(WaveDegree::Micro),
            "submicro" => Some(WaveDegree::Submicro),
            "miniscule" => Some(WaveDegree::Miniscule),
            _ => None,
        }
    }
}

// =============================================================================
// Label Style Configuration
// =============================================================================

/// Style configuration for wave/pattern labels
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LabelStyle {
    /// Font size in pixels
    #[serde(default = "default_label_font_size")]
    pub font_size: f64,
    /// Font weight: normal or bold
    #[serde(default = "default_font_weight")]
    pub font_weight: String,
    /// Text color (if None, uses primitive stroke color)
    pub color: Option<String>,
    /// Background color for label (if None, no background). Defaults to a
    /// dark translucent plate — labels drawn straight onto candles with no
    /// backing were the single biggest readability defect in this arc; see
    /// `nemo/mylittlechart` label-plumbing pass. `#[serde(default = ...)]`
    /// (rather than only setting it in `Default for LabelStyle`) so JSON
    /// that genuinely lacks the key — pre-dating this field's existence —
    /// also resolves to a plate. Saved drawings that already serialized an
    /// explicit `null` here keep reading back as no background; only a
    /// fresh recreate of the primitive picks up the new default for those.
    #[serde(default = "default_label_background_color")]
    pub background_color: Option<String>,
    /// Background padding in pixels
    #[serde(default = "default_label_padding")]
    pub background_padding: f64,
    /// Background corner radius
    #[serde(default = "default_label_radius")]
    pub background_radius: f64,
    /// Whether the plate draws a border at all. A separate flag, NOT
    /// `border_color.is_some()` (2026-08-09): the UI row for the colour
    /// shows the RESOLVED value (falling back to the label colour when
    /// unset), so a template — which captures declared property values —
    /// wrote that resolved colour back on apply and silently turned the
    /// border ON for every primitive it touched. That is what put a
    /// coloured ring around every fib label and what kept "Save as…" lit
    /// after applying a system template: the applied state could never
    /// equal the template it came from. An `Option` that doubles as an
    /// on/off switch cannot round-trip through a UI that has to render
    /// SOMETHING when it is `None` — so the switch is its own field now.
    /// `#[serde(default)]` = false: drawings carrying a `border_color` from
    /// before this split load with the border off, which is the state their
    /// owner actually chose (nobody ever enabled it deliberately).
    #[serde(default)]
    pub border_enabled: bool,
    /// Border colour. `None` means "use the label's own colour" — a colour
    /// only, never an on/off switch (see [`Self::border_enabled`]).
    pub border_color: Option<String>,
    /// Border width
    #[serde(default = "default_border_width")]
    pub border_width: f64,
    /// Vertical offset from point (negative = above)
    #[serde(default = "default_label_offset")]
    pub offset_y: f64,
}

fn default_label_font_size() -> f64 { 12.0 }
fn default_font_weight() -> String { "normal".to_string() }
fn default_label_background_color() -> Option<String> { Some("#0d0f14cc".to_string()) }
fn default_label_padding() -> f64 { 4.0 }
fn default_label_radius() -> f64 { 3.0 }
fn default_border_width() -> f64 { 1.0 }
fn default_label_offset() -> f64 { 15.0 }

impl Default for LabelStyle {
    fn default() -> Self {
        Self {
            font_size: 12.0,
            font_weight: "normal".to_string(),
            color: None,
            background_color: default_label_background_color(),
            background_padding: 4.0,
            background_radius: 3.0,
            border_enabled: false,
            border_color: None,
            border_width: 1.0,
            offset_y: 15.0,
        }
    }
}

impl LabelStyle {
    /// Create a simple label style with just text color
    pub fn simple(color: Option<&str>) -> Self {
        Self {
            color: color.map(|c| c.to_string()),
            ..Default::default()
        }
    }

    /// Create a label with background
    pub fn with_background(bg_color: &str, text_color: Option<&str>) -> Self {
        Self {
            color: text_color.map(|c| c.to_string()),
            background_color: Some(bg_color.to_string()),
            ..Default::default()
        }
    }

    /// Create a circled/bordered label style (for circled numbers like ①②③)
    pub fn circled(border_color: &str) -> Self {
        Self {
            border_enabled: true,
            border_color: Some(border_color.to_string()),
            border_width: 1.5,
            background_radius: 10.0,
            background_padding: 2.0,
            ..Default::default()
        }
    }

    /// Get CSS-like font string
    pub fn font_string(&self) -> String {
        if self.font_weight == "bold" {
            format!("bold {}px sans-serif", self.font_size)
        } else {
            format!("{}px sans-serif", self.font_size)
        }
    }

    /// Config properties for the parts of the struct beyond `font_size`/
    /// `color` (those two predate this method — every call site already
    /// wires them at fixed orders 13/14 via `ConfigProperty::label_font_size`/
    /// `label_color`). This appends the "inherit line colour" toggle FIRST,
    /// at `order_start` (see [`Self::apply_property`]'s own doc for why it
    /// must be applied after each caller's own `"label_color"` write — this
    /// row's position immediately after that row, whatever order the caller
    /// used for it, is what makes that ordering hold), then the remaining 6
    /// fields — bold, offset, background on/off + color, padding, corner
    /// radius, border on/off + color + width — at `order_start + 1 ..=
    /// order_start + 9`. Callers `.extend()` this into their own
    /// `style_properties()`.
    pub fn style_properties(&self, fallback_color: &str, order_start: i32) -> Vec<ConfigProperty> {
        vec![
            ConfigProperty::boolean("label_color_inherit", ConfigKey::LabelColorInherit.get(current_language()), self.color.is_none())
                .with_category(PropertyCategory::LabelsText).with_order(order_start),
            ConfigProperty::boolean("label_bold", ConfigKey::Bold.get(current_language()), self.font_weight == "bold")
                .with_category(PropertyCategory::LabelsText).with_order(order_start + 1),
            ConfigProperty::number("label_offset", ConfigKey::LabelOffset.get(current_language()), self.offset_y, Some(0.0), Some(60.0))
                .with_category(PropertyCategory::LabelsText).with_order(order_start + 2),
            ConfigProperty::boolean("label_background", ConfigKey::ShowBackground.get(current_language()), self.background_color.is_some())
                .with_category(PropertyCategory::LabelsText).with_order(order_start + 3),
            ConfigProperty::color("label_background_color", ConfigKey::LabelBackgroundColor.get(current_language()), self.background_color.as_deref().unwrap_or(fallback_color))
                .with_category(PropertyCategory::LabelsText).with_order(order_start + 4),
            ConfigProperty::number("label_padding", ConfigKey::LabelPadding.get(current_language()), self.background_padding, Some(0.0), Some(20.0))
                .with_category(PropertyCategory::LabelsText).with_order(order_start + 5),
            ConfigProperty::number("label_radius", ConfigKey::LabelRadius.get(current_language()), self.background_radius, Some(0.0), Some(20.0))
                .with_category(PropertyCategory::LabelsText).with_order(order_start + 6),
            ConfigProperty::boolean("label_border", ConfigKey::LabelBorder.get(current_language()), self.border_enabled)
                .with_category(PropertyCategory::LabelsText).with_order(order_start + 7),
            ConfigProperty::color("label_border_color", ConfigKey::LabelBorderColor.get(current_language()), self.border_color.as_deref().unwrap_or(fallback_color))
                .with_category(PropertyCategory::LabelsText).with_order(order_start + 8),
            ConfigProperty::number("label_border_width", ConfigKey::LabelBorderWidth.get(current_language()), self.border_width, Some(0.5), Some(6.0))
                .with_category(PropertyCategory::LabelsText).with_order(order_start + 9),
        ]
    }

    /// Apply a property produced by `style_properties()`. Returns `true` if
    /// `id` was recognized. `font_size`/`color` are NOT handled here — they
    /// stay on the per-primitive `"label_font_size"`/`"label_color"` ids
    /// callers already had; this only owns the ids it mints above.
    ///
    /// `fallback_color` is the primitive's own stroke colour
    /// (`self.data.color.stroke` at every call site) — turning inherit OFF
    /// with no colour chosen yet has nowhere else to source a concrete
    /// value from, and leaving `self.color` at `None` would make the
    /// checkbox look like it did nothing. Turning inherit back ON always
    /// writes `None`, regardless of what colour was pinned before —
    /// `self.color.is_none()` is the checkbox's own read
    /// ([`Self::style_properties`]'s `"label_color_inherit"` row), so this
    /// is the only write that can ever uncheck it again.
    pub fn apply_property(&mut self, id: &str, value: &PropertyValue, fallback_color: &str) -> bool {
        match id {
            "label_color_inherit" => {
                if let Some(v) = value.as_bool() {
                    self.color = if v { None } else { Some(self.color.clone().unwrap_or_else(|| fallback_color.to_string())) };
                    return true;
                }
            }
            "label_bold" => {
                if let Some(v) = value.as_bool() {
                    self.font_weight = if v { "bold".to_string() } else { "normal".to_string() };
                    return true;
                }
            }
            "label_offset" => {
                if let Some(v) = value.as_number() { self.offset_y = v; return true; }
            }
            "label_background" => {
                if let Some(v) = value.as_bool() {
                    self.background_color = if v {
                        Some(self.background_color.clone().unwrap_or_else(|| default_label_background_color().unwrap_or_default()))
                    } else {
                        None
                    };
                    return true;
                }
            }
            "label_background_color" => {
                if let Some(c) = value.as_color() { self.background_color = Some(c.to_string()); return true; }
            }
            "label_padding" => {
                if let Some(v) = value.as_number() { self.background_padding = v.max(0.0); return true; }
            }
            "label_radius" => {
                if let Some(v) = value.as_number() { self.background_radius = v.max(0.0); return true; }
            }
            "label_border" => {
                if let Some(v) = value.as_bool() { self.border_enabled = v; return true; }
            }
            // Sets the COLOUR only. Turning the border on is
            // `label_border`'s job — writing this row must never enable it,
            // or applying any template re-enables the border on every
            // primitive it touches (see `border_enabled`'s own doc).
            "label_border_color" => {
                if let Some(c) = value.as_color() { self.border_color = Some(c.to_string()); return true; }
            }
            "label_border_width" => {
                if let Some(v) = value.as_number() { self.border_width = v.max(0.1); return true; }
            }
            _ => {}
        }
        false
    }
}

/// Property writes a system набор (level-set preset) performs on every
/// primitive's `LabelStyle` when applied — the label-plate half of "шаблоны
/// это настройки в целом" (owner direction, 2026-08-09): a preset restores
/// the label plate to the product default and re-establishes label colour
/// INHERITANCE (`label_color_inherit = true`, i.e. `LabelStyle.color =
/// None` — "use the line's own colour"), not just the level value list.
///
/// Ids are exactly the ones [`LabelStyle::apply_property`] already owns —
/// this mints no new ones — and the numbers are read from the same
/// `default_label_*` functions `LabelStyle::default()` itself uses, so
/// there is one source for them, not a second copy. Does not touch
/// `LabelStyle::default()` or any `default_label_*` value.
pub fn level_set_label_defaults() -> Vec<(&'static str, PropertyValue)> {
    vec![
        ("label_color_inherit", PropertyValue::Boolean(true)),
        ("label_bold", PropertyValue::Boolean(false)),
        ("label_padding", PropertyValue::Number(default_label_padding())),
        ("label_radius", PropertyValue::Number(default_label_radius())),
        ("label_background", PropertyValue::Boolean(true)),
    ]
}

// =============================================================================
// Property Types
// =============================================================================

/// Type of configuration property
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum PropertyType {
    /// Color picker (hex string)
    Color,
    /// Numeric value with optional range
    Number {
        min: Option<f64>,
        max: Option<f64>,
        step: Option<f64>,
    },
    /// Integer value with optional range
    Integer {
        min: Option<i32>,
        max: Option<i32>,
    },
    /// Boolean toggle
    Boolean,
    /// Select from predefined options
    Select {
        options: Vec<SelectOption>,
    },
    /// Line style selector
    LineStyle,
    /// Text input
    Text {
        multiline: bool,
        max_length: Option<usize>,
    },
    /// Fibonacci levels (list of level configs)
    FibLevels,
    /// Coordinate (bar, price)
    Coordinate,
    /// Timeframe visibility settings
    TimeframeVisibility,
}

/// Option for Select property type
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

impl SelectOption {
    pub fn new(value: &str, label: &str) -> Self {
        Self {
            value: value.to_string(),
            label: label.to_string(),
        }
    }

    /// Create select options for label positions (left, center, right)
    pub fn label_positions() -> Vec<Self> {
        let lang = current_language();
        vec![
            Self::new("left", LabelPositionKey::Left.get(lang)),
            Self::new("center", LabelPositionKey::Center.get(lang)),
            Self::new("right", LabelPositionKey::Right.get(lang)),
        ]
    }

    /// Create select options for vertical positions (top, center, bottom)
    pub fn vertical_positions() -> Vec<Self> {
        let lang = current_language();
        vec![
            Self::new("top", LabelPositionKey::Top.get(lang)),
            Self::new("center", LabelPositionKey::Center.get(lang)),
            Self::new("bottom", LabelPositionKey::Bottom.get(lang)),
        ]
    }
}

// =============================================================================
// Property Values
// =============================================================================

/// Value of a configuration property
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum PropertyValue {
    Color(String),
    Number(f64),
    Integer(i32),
    Boolean(bool),
    String(String),
    LineStyle(String), // "solid", "dashed", "dotted"
    FibLevels(Vec<FibLevelConfig>),
    Coordinate { bar: f64, price: f64 },
    TimeframeVisibility(TimeframeVisibilityConfig),
}

/// Style / text / level property family.
///
/// Replaces the 3 separate `apply_{style,text,level}_property` manager
/// methods with one intent call (`DrawingManager::apply_property`). The
/// individual `prop_id` stays an open `&str` schema — properties within
/// each family are introspected per-primitive-type via
/// `Primitive::{style,text,level}_properties()` and are genuinely
/// unbounded (new primitive types plug in new properties without touching
/// central code). Only the 3-way family split is closed and stable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyKind {
    Style,
    Text,
    Level,
}

impl PropertyValue {
    pub fn as_color(&self) -> Option<&str> {
        match self {
            PropertyValue::Color(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            PropertyValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            PropertyValue::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&str> {
        match self {
            PropertyValue::String(s) => Some(s),
            PropertyValue::Color(s) => Some(s),
            PropertyValue::LineStyle(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_coordinate(&self) -> Option<(f64, f64)> {
        match self {
            PropertyValue::Coordinate { bar, price } => Some((*bar, *price)),
            _ => None,
        }
    }
}

/// Configuration for a single Fibonacci level
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FibLevelConfig {
    /// Level value (0.0, 0.236, 0.382, 0.5, 0.618, etc.)
    pub level: f64,
    /// Is this level visible
    pub visible: bool,
    /// Line color (if different from main color)
    pub color: Option<String>,
    /// Line width (if different from main width)
    pub width: Option<f64>,
    /// Line style
    pub style: String,
    /// Fill color for area below this level (to next level down)
    #[serde(default)]
    pub fill_color: Option<String>,
    /// Fill opacity (0.0 to 1.0)
    #[serde(default = "default_fill_opacity")]
    pub fill_opacity: f64,
    /// Whether fill is enabled for this level
    #[serde(default)]
    pub fill_enabled: bool,
    /// Group name for organizing levels (e.g., "base", "fib" for pitchforks)
    /// UI can show group headers and toggle entire groups
    #[serde(default)]
    pub group: Option<String>,
}

use super::fill_defaults::{BAND_FILL_OPACITY, default_band_fill_opacity as default_fill_opacity};

impl FibLevelConfig {
    pub fn new(level: f64) -> Self {
        Self {
            level,
            visible: true,
            color: None,
            width: None,
            style: "solid".to_string(),
            fill_color: None,
            fill_opacity: BAND_FILL_OPACITY,
            fill_enabled: false,
            group: None,
        }
    }

    pub fn with_style(level: f64, style: &str) -> Self {
        Self {
            level,
            visible: true,
            color: None,
            width: None,
            style: style.to_string(),
            fill_color: None,
            fill_opacity: BAND_FILL_OPACITY,
            fill_enabled: false,
            group: None,
        }
    }

    /// Create with fill enabled (for default preset with fills)
    pub fn with_fill(level: f64, fill_color: Option<String>, opacity: f64) -> Self {
        Self {
            level,
            visible: true,
            color: None,
            width: None,
            style: "solid".to_string(),
            fill_color,
            fill_opacity: opacity,
            fill_enabled: true,
            group: None,
        }
    }

    /// Create with group assignment (for pitchfork Base/Fib mode switching)
    pub fn with_group(level: f64, group: &str) -> Self {
        Self {
            level,
            visible: true,
            color: None,
            width: None,
            style: "solid".to_string(),
            fill_color: None,
            fill_opacity: BAND_FILL_OPACITY,
            fill_enabled: false,
            group: Some(group.to_string()),
        }
    }

    /// Set group on existing config
    pub fn in_group(mut self, group: &str) -> Self {
        self.group = Some(group.to_string());
        self
    }
}

/// Timeframe visibility configuration
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TimeframeVisibilityConfig {
    /// Show on tick charts
    pub ticks: bool,
    /// Show on second charts (range: 1-59)
    pub seconds: Option<(u32, u32)>,
    /// Show on minute charts (range: 1-59)
    pub minutes: Option<(u32, u32)>,
    /// Show on hour charts (range: 1-24)
    pub hours: Option<(u32, u32)>,
    /// Show on day charts (range: 1-366)
    pub days: Option<(u32, u32)>,
    /// Show on week charts (range: 1-52)
    pub weeks: Option<(u32, u32)>,
    /// Show on month charts (range: 1-12)
    pub months: Option<(u32, u32)>,
    /// Show on range charts
    pub ranges: bool,
}

impl TimeframeVisibilityConfig {
    /// Create config that shows on all timeframes
    pub fn all() -> Self {
        Self {
            ticks: true,
            seconds: Some((1, 59)),
            minutes: Some((1, 59)),
            hours: Some((1, 24)),
            days: Some((1, 366)),
            weeks: Some((1, 52)),
            months: Some((1, 12)),
            ranges: true,
        }
    }

    /// Check if primitive is visible on a specific timeframe
    pub fn is_visible_on(&self, timeframe: &str, value: u32) -> bool {
        match timeframe {
            "tick" | "ticks" => self.ticks,
            "second" | "seconds" | "s" => {
                self.seconds.is_some_and(|(min, max)| value >= min && value <= max)
            }
            "minute" | "minutes" | "m" => {
                self.minutes.is_some_and(|(min, max)| value >= min && value <= max)
            }
            "hour" | "hours" | "h" => {
                self.hours.is_some_and(|(min, max)| value >= min && value <= max)
            }
            "day" | "days" | "d" | "D" => {
                self.days.is_some_and(|(min, max)| value >= min && value <= max)
            }
            "week" | "weeks" | "w" | "W" => {
                self.weeks.is_some_and(|(min, max)| value >= min && value <= max)
            }
            "month" | "months" | "M" => {
                self.months.is_some_and(|(min, max)| value >= min && value <= max)
            }
            "range" | "ranges" => self.ranges,
            _ => true
        }
    }

    /// Check if this config shows on all timeframes
    pub fn is_all(&self) -> bool {
        self.ticks &&
        self.seconds == Some((1, 59)) &&
        self.minutes == Some((1, 59)) &&
        self.hours == Some((1, 24)) &&
        self.days == Some((1, 366)) &&
        self.weeks == Some((1, 52)) &&
        self.months == Some((1, 12)) &&
        self.ranges
    }

    /// Check visibility using a timeframe label string (e.g., "1H", "15m", "1D")
    /// Parses the label and calls is_visible_on() with appropriate type and value
    pub fn is_visible_on_label(&self, label: &str) -> bool {
        if let Some((tf_type, tf_value)) = Self::parse_timeframe_label(label) {
            self.is_visible_on(tf_type, tf_value)
        } else {
            true // Unknown format = show on all
        }
    }

    /// Parse timeframe label (e.g., "1H", "15m", "1D") into (type, value)
    /// Returns None if format is unrecognized
    pub fn parse_timeframe_label(label: &str) -> Option<(&'static str, u32)> {
        let label_lower = label.to_lowercase();
        match label_lower.as_str() {
            // Minutes
            "1m" => Some(("minutes", 1)),
            "3m" => Some(("minutes", 3)),
            "5m" => Some(("minutes", 5)),
            "15m" => Some(("minutes", 15)),
            "30m" => Some(("minutes", 30)),
            "45m" => Some(("minutes", 45)),
            // Hours
            "1h" => Some(("hours", 1)),
            "2h" => Some(("hours", 2)),
            "4h" => Some(("hours", 4)),
            "6h" => Some(("hours", 6)),
            "8h" => Some(("hours", 8)),
            "12h" => Some(("hours", 12)),
            // Days
            "1d" | "d" | "d1" => Some(("days", 1)),
            "3d" => Some(("days", 3)),
            // Weeks
            "1w" | "w" | "w1" => Some(("weeks", 1)),
            // Months
            "1mo" | "m1" => Some(("months", 1)),
            "3mo" => Some(("months", 3)),
            // Seconds
            "1s" => Some(("seconds", 1)),
            "5s" => Some(("seconds", 5)),
            "10s" => Some(("seconds", 10)),
            "15s" => Some(("seconds", 15)),
            "30s" => Some(("seconds", 30)),
            _ => {
                // Try parsing generic format: number + suffix
                let s = label_lower.trim();
                if s.is_empty() { return None; }

                // Find where digits end
                let digit_end = s.chars().take_while(|c| c.is_ascii_digit()).count();
                if digit_end == 0 { return None; }

                let num: u32 = s[..digit_end].parse().ok()?;
                let suffix = &s[digit_end..];

                match suffix {
                    "s" | "sec" | "second" | "seconds" => Some(("seconds", num)),
                    "m" | "min" | "minute" | "minutes" => Some(("minutes", num)),
                    "h" | "hr" | "hour" | "hours" => Some(("hours", num)),
                    "d" | "day" | "days" => Some(("days", num)),
                    "w" | "wk" | "week" | "weeks" => Some(("weeks", num)),
                    "mo" | "month" | "months" => Some(("months", num)),
                    _ => None,
                }
            }
        }
    }
}

// =============================================================================
// Config Property Definition
// =============================================================================

/// Where a property's control renders in the primitive-settings modal —
/// each variant names both a TAB (`Levels`/`Appearance`/`Labels`/
/// `Placement`, encoded by the variant's own name prefix) and a named GROUP
/// within that tab. A primitive declares this per property
/// (`ConfigProperty::category`); the modal buckets by it, sorts within a
/// bucket by `ConfigProperty::order`, and renders each control from its own
/// `PropertyType` — see `primitive_settings_vid`/`_labels`/`_levels`/
/// `_placement` in `mlc-app-layout`. A group nobody declared into renders
/// nothing (no header, no gap): the modal's layout follows the
/// declaration, not the other way around.
///
/// The TAB STRIP itself is derived, not fixed: `mlc-app-layout`'s
/// `available_tabs` builds it from the distinct tabs present among a
/// primitive's own declared properties, plus whichever tabs its bespoke
/// (non-`ConfigProperty`) collections force — a level ladder forces
/// `Levels`, the points table and timeframe chips force `Placement`
/// unconditionally. A tab with nothing in it (declared or bespoke) never
/// appears — a type with no level ladder shows no Levels tab, a type that
/// declares nothing into `AppearanceLine`/`AppearanceFill` and has no base
/// stroke to fall back to (i.e. it has levels, so that fallback lives on
/// the Levels tab instead) shows no Appearance tab either. Tab STRIP order
/// is the enum's own declaration order (`Levels`, `Appearance`, `Labels`,
/// `Placement`), never the order a primitive happened to declare its
/// properties in.
///
/// No variant carries a user-facing string — every group's header resolves
/// through an existing `ConfigKey`/`SettingsKey` at render time, the same
/// way every OTHER display string in this module already does. Replaces the
/// old five-tab vocabulary (`Style`/`Text`/`Coordinates`/`Levels`/
/// `Visibility`/`Inputs`), retired with the 3-tab cutover (2026-08-08); the
/// 3-tab set itself was in turn superseded the same day by this 4-tab,
/// declared-strip vocabulary (`Labels` split out of the old `Appearance`
/// catch-all into a tab of its own) — hard cutover both times, no
/// compatibility variants kept alongside these.
///
/// Extending this (a new group, or a 5th tab) never requires touching the
/// modal's own grouping/sorting/empty-group-suppression code — only
/// `available_tabs` and the tab module that owns the new variant need to
/// learn about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyCategory {
    /// Levels tab — the base stroke (colour/width/line style) every level
    /// falls back to when it has no override of its own, plus any other
    /// property that belongs with the ladder rather than the shape's own
    /// line (e.g. a Fibonacci retracement's `extend_left`/`extend_right`/
    /// `show_trend_line` — see its own `style_properties()` doc for why
    /// those moved here out of `AppearanceLine`).
    LevelsBase,
    /// Appearance tab — stroke colour / width / line style, extend
    /// left/right, trend line — everything about how the LINE itself
    /// looks (as opposed to a fill or a label).
    AppearanceLine,
    /// Appearance tab — fill on/off + opacity.
    AppearanceFill,
    /// Labels tab — content/position/font/style/pills and every other
    /// text-adjacent control (font size, colour, background, border,
    /// alignment). Its own tab since the 4-tab cutover (2026-08-08) — it
    /// used to share the Appearance tab (`AppearanceLabels`) with the line
    /// and fill groups above.
    LabelsText,
    /// Placement tab — per-point anchor properties, alongside the points
    /// table itself (which is a bespoke collection, not a declared
    /// property — see `primitive_settings_placement`'s own doc).
    PlacementPoints,
    /// Placement tab — timeframe-visibility properties, alongside the
    /// timeframe chip row itself (also bespoke).
    PlacementTimeframes,
}

/// A single configurable property
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigProperty {
    /// Unique identifier for this property
    pub id: String,
    /// Display name (localized)
    pub name: String,
    /// Property type
    pub prop_type: PropertyType,
    /// Current value
    pub value: PropertyValue,
    /// Category for UI grouping
    pub category: PropertyCategory,
    /// Order within category (lower = first)
    pub order: i32,
    /// Is property read-only
    pub readonly: bool,
    /// Help text / tooltip
    pub tooltip: Option<String>,
}

impl ConfigProperty {
    /// Create a color property
    pub fn color(id: &str, name: &str, value: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::Color,
            value: PropertyValue::Color(value.to_string()),
            category: PropertyCategory::AppearanceLine,
            order: 0,
            readonly: false,
            tooltip: None,
        }
    }

    /// Create a number property
    pub fn number(id: &str, name: &str, value: f64, min: Option<f64>, max: Option<f64>) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::Number { min, max, step: None },
            value: PropertyValue::Number(value),
            category: PropertyCategory::AppearanceLine,
            order: 0,
            readonly: false,
            tooltip: None,
        }
    }

    /// Create a boolean property
    pub fn boolean(id: &str, name: &str, value: bool) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::Boolean,
            value: PropertyValue::Boolean(value),
            category: PropertyCategory::AppearanceLine,
            order: 0,
            readonly: false,
            tooltip: None,
        }
    }

    /// Create a line style property
    pub fn line_style(id: &str, name: &str, value: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::LineStyle,
            value: PropertyValue::LineStyle(value.to_string()),
            category: PropertyCategory::AppearanceLine,
            order: 0,
            readonly: false,
            tooltip: None,
        }
    }

    /// Create a coordinate property
    pub fn coordinate(id: &str, name: &str, bar: f64, price: f64) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::Coordinate,
            value: PropertyValue::Coordinate { bar, price },
            category: PropertyCategory::PlacementPoints,
            order: 0,
            readonly: false,
            tooltip: None,
        }
    }

    /// Create a text content property (multiline)
    pub fn text_content(id: &str, name: &str, value: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::Text { multiline: true, max_length: Some(1000) },
            value: PropertyValue::String(value.to_string()),
            category: PropertyCategory::LabelsText,
            order: 0,
            readonly: false,
            tooltip: None,
        }
    }

    /// Create a text property (single line)
    pub fn text(id: &str, name: &str, value: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::Text { multiline: false, max_length: Some(200) },
            value: PropertyValue::String(value.to_string()),
            category: PropertyCategory::LabelsText,
            order: 0,
            readonly: false,
            tooltip: None,
        }
    }

    /// Create a select property
    pub fn select(id: &str, name: &str, value: &str, options: Vec<SelectOption>) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::Select { options },
            value: PropertyValue::String(value.to_string()),
            category: PropertyCategory::AppearanceLine,
            order: 0,
            readonly: false,
            tooltip: None,
        }
    }

    /// Set category
    pub fn with_category(mut self, category: PropertyCategory) -> Self {
        self.category = category;
        self
    }

    /// Set order
    pub fn with_order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }

    /// Set tooltip
    pub fn with_tooltip(mut self, tooltip: &str) -> Self {
        self.tooltip = Some(tooltip.to_string());
        self
    }

    /// Set readonly
    pub fn readonly(mut self) -> Self {
        self.readonly = true;
        self
    }

    // =========================================================================
    // i18n-aware property constructors
    // =========================================================================

    /// Create "Show Prices" boolean property with i18n
    pub fn show_prices(value: bool) -> Self {
        Self::boolean("show_prices", ConfigKey::Prices.get(current_language()), value)
    }

    /// Create "Show Levels" boolean property with i18n
    pub fn show_levels(value: bool) -> Self {
        Self::boolean("show_percentages", ConfigKey::Levels.get(current_language()), value)
    }

    /// Create "Show as Percent" boolean property with i18n
    pub fn show_as_percent(value: bool) -> Self {
        Self::boolean("show_as_percent", ConfigKey::Percentages.get(current_language()), value)
    }

    /// Create "Label Position" select property with i18n
    pub fn label_position(value: &str) -> Self {
        Self::select(
            "label_position",
            ConfigKey::LabelPosition.get(current_language()),
            value,
            SelectOption::label_positions(),
        )
    }

    /// Create "Extend Left" boolean property with i18n
    pub fn extend_left(value: bool) -> Self {
        Self::boolean("extend_left", ConfigKey::ExtendLeft.get(current_language()), value)
    }

    /// Create "Extend Right" boolean property with i18n
    pub fn extend_right(value: bool) -> Self {
        Self::boolean("extend_right", ConfigKey::ExtendRight.get(current_language()), value)
    }

    /// Create "Show Trend Line" boolean property with i18n
    pub fn show_trend_line(value: bool) -> Self {
        Self::boolean("show_trend_line", ConfigKey::TrendLine.get(current_language()), value)
    }

    /// Create "Extend Lines" boolean property with i18n
    pub fn extend_lines(value: bool) -> Self {
        Self::boolean("extend_lines", ConfigKey::ExtendLines.get(current_language()), value)
    }

    /// Create "Show Labels" boolean property with i18n
    pub fn show_labels(value: bool) -> Self {
        Self::boolean("show_labels", ConfigKey::ShowLabels.get(current_language()), value)
    }

    /// Create "Show Lines" boolean property with i18n
    pub fn show_lines(value: bool) -> Self {
        Self::boolean("show_lines", ConfigKey::ShowLines.get(current_language()), value)
    }

    /// Create "Show Ratios" boolean property with i18n
    pub fn show_ratios(value: bool) -> Self {
        Self::boolean("show_ratios", ConfigKey::ShowRatios.get(current_language()), value)
    }

    /// Create "Show Trendlines" boolean property with i18n
    pub fn show_trendlines(value: bool) -> Self {
        Self::boolean("show_trendlines", ConfigKey::ShowTrendlines.get(current_language()), value)
    }

    /// Create "Show Price" boolean property with i18n
    pub fn show_price(value: bool) -> Self {
        Self::boolean("show_price", ConfigKey::ShowPrice.get(current_language()), value)
    }

    /// Create "Show Line" boolean property with i18n
    pub fn show_line(value: bool) -> Self {
        Self::boolean("show_line", ConfigKey::ShowLine.get(current_language()), value)
    }

    /// Create "Show Header" boolean property with i18n
    pub fn show_header(value: bool) -> Self {
        Self::boolean("show_header", ConfigKey::ShowHeader.get(current_language()), value)
    }

    /// Create "Show Neckline" boolean property with i18n
    pub fn show_neckline(value: bool) -> Self {
        Self::boolean("show_neckline", ConfigKey::ShowNeckline.get(current_language()), value)
    }

    /// Create "Show Background" boolean property with i18n
    pub fn show_background(value: bool) -> Self {
        Self::boolean("background", ConfigKey::ShowBackground.get(current_language()), value)
    }

    /// Create "Extend" boolean property with i18n
    pub fn extend(value: bool) -> Self {
        Self::boolean("extend", ConfigKey::Extend.get(current_language()), value)
    }

    /// Create "Levels" (show_percentages) boolean property with i18n
    pub fn levels(value: bool) -> Self {
        Self::boolean("show_percentages", ConfigKey::Levels.get(current_language()), value)
    }

    /// Create "Full Circle" boolean property with i18n
    pub fn full_circle(value: bool) -> Self {
        Self::boolean("full_circle", ConfigKey::FullCircle.get(current_language()), value)
    }

    /// Create "Reverse" boolean property with i18n
    pub fn reverse(value: bool) -> Self {
        Self::boolean("reverse", ConfigKey::Reverse.get(current_language()), value)
    }

    /// Create "Closed" boolean property with i18n (free-form shape closure —
    /// last point joins back to the first). Precondition for `fill` to paint
    /// anything on a free-form shape (Polyline/Path); order it above the
    /// "Fill" row in `style_properties()`.
    pub fn closed(value: bool) -> Self {
        Self::boolean("closed", ConfigKey::Closed.get(current_language()), value)
    }

    /// Create "Fill" boolean property with i18n
    pub fn fill(value: bool) -> Self {
        Self::boolean("fill", ConfigKey::Fill.get(current_language()), value)
    }

    /// Create "Fill Opacity" number property with i18n. `value_pct` is a
    /// percentage (0-100) — primitives store opacity as 0.0-1.0 internally,
    /// but the slider renders `{:.0}` with no decimals, so a 0.0-1.0 range
    /// would only ever show "0" or "1".
    pub fn fill_opacity(value_pct: f64) -> Self {
        Self::number("fill_opacity", ConfigKey::FillOpacity.get(current_language()), value_pct, Some(0.0), Some(100.0))
    }

    /// Create "Smooth" boolean property with i18n (path curve smoothing).
    pub fn smooth(value: bool) -> Self {
        Self::boolean("smooth", ConfigKey::Smooth.get(current_language()), value)
    }

    /// Create "Level Mode" select property with i18n for pitchforks
    pub fn level_mode(value: &str) -> Self {
        let lang = current_language();
        Self::select(
            "level_mode",
            ConfigKey::LevelMode.get(lang),
            value,
            vec![
                SelectOption::new("both", ConfigKey::AllLevels.get(lang)),
                SelectOption::new("base", ConfigKey::BaseLevels.get(lang)),
                SelectOption::new("fibonacci", ConfigKey::FibonacciLevels.get(lang)),
            ],
        )
    }

    /// Create "Label Font Size" number property with i18n
    pub fn label_font_size(value: f64) -> Self {
        Self::number("label_font_size", ConfigKey::LabelFontSize.get(current_language()), value, Some(8.0), Some(32.0))
    }

    /// Create "Label Color" color property with i18n
    pub fn label_color(value: &str) -> Self {
        Self::color("label_color", ConfigKey::LabelColor.get(current_language()), value)
    }

    /// Create "Label Background Color" color property with i18n
    pub fn label_background_color(value: &str) -> Self {
        Self::color("label_background_color", ConfigKey::LabelBackgroundColor.get(current_language()), value)
    }

    /// Create "Label Padding" number property with i18n
    pub fn label_padding(value: f64) -> Self {
        Self::number("label_padding", ConfigKey::LabelPadding.get(current_language()), value, Some(0.0), Some(20.0))
    }

    /// Create "Label Corner Radius" number property with i18n
    pub fn label_radius(value: f64) -> Self {
        Self::number("label_radius", ConfigKey::LabelRadius.get(current_language()), value, Some(0.0), Some(20.0))
    }

    /// Create "Label Border" boolean property with i18n
    pub fn label_border(value: bool) -> Self {
        Self::boolean("label_border", ConfigKey::LabelBorder.get(current_language()), value)
    }

    /// Create "Label Border Color" color property with i18n
    pub fn label_border_color(value: &str) -> Self {
        Self::color("label_border_color", ConfigKey::LabelBorderColor.get(current_language()), value)
    }

    /// Create "Label Border Width" number property with i18n
    pub fn label_border_width(value: f64) -> Self {
        Self::number("label_border_width", ConfigKey::LabelBorderWidth.get(current_language()), value, Some(0.5), Some(6.0))
    }

    /// Create "Label Offset" number property with i18n
    pub fn label_offset(value: f64) -> Self {
        Self::number("label_offset", ConfigKey::LabelOffset.get(current_language()), value, Some(0.0), Some(60.0))
    }

    /// Create "Quantity" number property with i18n (trading position size)
    pub fn quantity(value: f64) -> Self {
        Self::number("quantity", ConfigKey::Quantity.get(current_language()), value, Some(0.0), Some(1_000_000.0))
    }

    /// Create "Show P&L" boolean property with i18n
    pub fn show_pnl(value: bool) -> Self {
        Self::boolean("show_pnl", ConfigKey::ShowPnl.get(current_language()), value)
    }

    /// Create "Show Live Label" boolean property with i18n (long/short
    /// position — gates the live-price text label; the live shading layer
    /// and marker line always render regardless of this setting).
    pub fn show_live_label(value: bool) -> Self {
        Self::boolean("show_live_label", ConfigKey::ShowLiveLabel.get(current_language()), value)
    }

    /// Create "Stop Zone Color" color property with i18n (long/short position)
    pub fn stop_zone_color(value: &str) -> Self {
        Self::color("stop_color", ConfigKey::StopZoneColor.get(current_language()), value)
    }

    /// Create "Target Zone Color" color property with i18n (long/short position)
    pub fn target_zone_color(value: &str) -> Self {
        Self::color("target_color", ConfigKey::TargetZoneColor.get(current_language()), value)
    }

    /// Create "Show Price Diff" boolean property with i18n (measurement rulers).
    /// Property id stays `"show_pips"` to match the struct field it drives.
    pub fn show_price_diff(value: bool) -> Self {
        Self::boolean("show_pips", ConfigKey::ShowPriceDiff.get(current_language()), value)
    }

    /// Create "Show Bar Count" boolean property with i18n (measurement rulers)
    pub fn show_bar_count(value: bool) -> Self {
        Self::boolean("show_bars", ConfigKey::ShowBarCount.get(current_language()), value)
    }

    /// Create "Show Duration" boolean property with i18n (date range ruler)
    pub fn show_duration(value: bool) -> Self {
        Self::boolean("show_time", ConfigKey::ShowDuration.get(current_language()), value)
    }

    /// Create "Rotations" number property with i18n (Fibonacci spiral turn count)
    pub fn rotations(value: f64) -> Self {
        Self::number("rotations", ConfigKey::Rotations.get(current_language()), value, Some(0.5), Some(6.0))
    }

    /// Create "Clockwise" boolean property with i18n (Fibonacci spiral direction)
    pub fn clockwise(value: bool) -> Self {
        Self::boolean("clockwise", ConfigKey::Clockwise.get(current_language()), value)
    }

    /// Create "Flip Horizontal" boolean property with i18n (Fibonacci spiral mirroring)
    pub fn flip_horizontal(value: bool) -> Self {
        Self::boolean("flip_horizontal", ConfigKey::FlipHorizontal.get(current_language()), value)
    }

    /// Create "Flip Vertical" boolean property with i18n (Fibonacci spiral mirroring)
    pub fn flip_vertical(value: bool) -> Self {
        Self::boolean("flip_vertical", ConfigKey::FlipVertical.get(current_language()), value)
    }

    /// Create "Wave Degree" select property with i18n
    pub fn wave_degree(value: &str, options: Vec<SelectOption>) -> Self {
        Self::select("degree", ConfigKey::WaveDegree.get(current_language()), value, options)
    }

    /// Create "Inverted" boolean property with i18n
    pub fn inverted(value: bool) -> Self {
        Self::boolean("inverted", ConfigKey::Inverted.get(current_language()), value)
    }

    /// Create "Triangle Type" select property with i18n
    pub fn triangle_type(value: &str) -> Self {
        let lang = current_language();
        Self::select(
            "triangle_type",
            ConfigKey::TriangleType.get(lang),
            value,
            vec![
                SelectOption::new("symmetrical", ConfigKey::Symmetrical.get(lang)),
                SelectOption::new("ascending", ConfigKey::Ascending.get(lang)),
                SelectOption::new("descending", ConfigKey::Descending.get(lang)),
                SelectOption::new("expanding", ConfigKey::Expanding.get(lang)),
            ],
        )
    }

    /// Create "Correction Type" select property with i18n (Elliott
    /// Correction's `CorrectionType` — Zigzag/Flat/Expanded/Running).
    /// 2026-08-09: previously a real field with zero declared representation.
    pub fn correction_type(value: &str) -> Self {
        let lang = current_language();
        Self::select(
            "correction_type",
            ConfigKey::CorrectionType.get(lang),
            value,
            vec![
                SelectOption::new("zigzag", ConfigKey::CorrectionZigzag.get(lang)),
                SelectOption::new("flat", ConfigKey::CorrectionFlat.get(lang)),
                SelectOption::new("expanded", ConfigKey::CorrectionExpanded.get(lang)),
                SelectOption::new("running", ConfigKey::CorrectionRunning.get(lang)),
            ],
        )
    }

    /// Create "Font Size" number property with i18n
    pub fn font_size(value: f64) -> Self {
        Self::number("font_size", ConfigKey::FontSize.get(current_language()), value, Some(8.0), Some(72.0))
    }

    /// Create "Text Color" color property with i18n
    pub fn text_color(value: &str) -> Self {
        Self::color("text_color", ConfigKey::TextColor.get(current_language()), value)
    }

    /// Create "Header Color" color property with i18n
    pub fn header_color(value: &str) -> Self {
        Self::color("header_color", ConfigKey::HeaderColor.get(current_language()), value)
    }

    /// Create "Grid Color" color property with i18n
    pub fn grid_color(value: &str) -> Self {
        Self::color("grid_color", ConfigKey::GridColor.get(current_language()), value)
    }

    /// Create "Header Text Color" color property with i18n
    pub fn header_text_color(value: &str) -> Self {
        Self::color("header_text_color", ConfigKey::HeaderTextColor.get(current_language()), value)
    }

    /// Create "Content" text property with i18n
    pub fn content(value: &str) -> Self {
        Self::text("content", ConfigKey::Content.get(current_language()), value)
    }

    /// Create "Comment" multiline text property with i18n
    pub fn comment(value: &str) -> Self {
        Self::text_content("content", ConfigKey::Comment.get(current_language()), value)
    }

    /// Create "Bold" boolean property with i18n
    pub fn bold(value: bool) -> Self {
        Self::boolean("bold", ConfigKey::Bold.get(current_language()), value)
    }

    /// Create "Italic" boolean property with i18n
    pub fn italic(value: bool) -> Self {
        Self::boolean("italic", ConfigKey::Italic.get(current_language()), value)
    }

    /// Create "Bubble Width" number property with i18n
    pub fn bubble_width(value: f64) -> Self {
        Self::number("bubble_width", ConfigKey::BubbleWidth.get(current_language()), value, Some(50.0), Some(500.0))
    }

    /// Create "Bubble Height" number property with i18n
    pub fn bubble_height(value: f64) -> Self {
        Self::number("bubble_height", ConfigKey::BubbleHeight.get(current_language()), value, Some(30.0), Some(300.0))
    }

    /// Create "Expanded" boolean property with i18n
    pub fn expanded(value: bool) -> Self {
        Self::boolean("expanded", ConfigKey::Expanded.get(current_language()), value)
    }

    /// Create "Direction" select property with i18n for signpost
    pub fn direction(value: &str) -> Self {
        let lang = current_language();
        Self::select(
            "direction",
            ConfigKey::Direction.get(lang),
            value,
            vec![
                SelectOption::new("right", ConfigKey::DirectionRight.get(lang)),
                SelectOption::new("left", ConfigKey::DirectionLeft.get(lang)),
                SelectOption::new("up", ConfigKey::DirectionUp.get(lang)),
                SelectOption::new("down", ConfigKey::DirectionDown.get(lang)),
            ],
        )
    }

    /// Create "Rows" number property with i18n for table
    pub fn rows_count(value: f64) -> Self {
        Self::number("rows_count", ConfigKey::Rows.get(current_language()), value, Some(1.0), Some(20.0))
    }

    /// Create "Columns" number property with i18n for table
    pub fn columns_count(value: f64) -> Self {
        Self::number("cols_count", ConfigKey::Columns.get(current_language()), value, Some(1.0), Some(10.0))
    }

    /// Create "Horizontal Align" select property with i18n
    pub fn h_align(value: &str) -> Self {
        let lang = current_language();
        Self::select(
            "text_h_align",
            ConfigKey::HorizontalAlign.get(lang),
            value,
            vec![
                SelectOption::new("start", ConfigKey::AlignLeft.get(lang)),
                SelectOption::new("center", ConfigKey::AlignCenter.get(lang)),
                SelectOption::new("end", ConfigKey::AlignRight.get(lang)),
            ],
        )
    }

    /// Create "Vertical Align" select property with i18n
    pub fn v_align(value: &str) -> Self {
        let lang = current_language();
        Self::select(
            "text_v_align",
            ConfigKey::VerticalAlign.get(lang),
            value,
            vec![
                SelectOption::new("start", ConfigKey::AlignTop.get(lang)),
                SelectOption::new("center", ConfigKey::AlignCenter.get(lang)),
                SelectOption::new("end", ConfigKey::AlignBottom.get(lang)),
            ],
        )
    }
}

// =============================================================================
// Last-used style snapshot
// =============================================================================
//
// `TemplateStyle` is NOT part of the (now-deleted) legacy `SettingsTemplate`
// system — it is the bubble's per-type "last used" style, captured every
// time a primitive of that type is finalized and replayed onto the NEXT one
// drawn (`DrawingManager::save_last_style_for_primitive` /
// `apply_last_style_snapshot` in `mlc-backend`). Unrelated to
// `mlc_core::templates::PrimitiveTemplate` (a named, user-saved or system
// row in the Template menu) — this has no id, is never shown as a menu row,
// and is overwritten unconditionally on every finalize.

/// Style portion of a template
#[derive(Clone, Debug, Serialize, Deserialize)]
#[derive(Default)]
pub struct TemplateStyle {
    /// Main color
    pub color: Option<String>,
    /// Line width
    pub width: Option<f64>,
    /// Line style
    pub line_style: Option<String>,
    /// Fill color
    pub fill_color: Option<String>,
    /// Fill opacity
    pub fill_opacity: Option<f64>,
    /// Show labels
    pub show_labels: Option<bool>,
    /// Show prices
    pub show_prices: Option<bool>,
    /// Extended per-primitive style properties (e.g. show_labels, line_extend,
    /// label_font_size, etc.) captured via `style_properties()` and restored via
    /// `apply_style_property()` on the next creation of the same primitive type.
    #[serde(default)]
    pub style_properties: Vec<(String, PropertyValue)>,
    /// Full level ladder captured via `Primitive::level_configs()`, restored
    /// via `Primitive::set_level_configs()` on the next primitive of the same
    /// type — the last-used-style analogue of `PrimitiveTemplate::level_configs`.
    /// Empty means either "this primitive type has no ladder" or "captured
    /// before this field existed" (`#[serde(default)]`); both read the same
    /// way on apply: the freshly-created primitive's own default ladder is
    /// left alone rather than cleared.
    #[serde(default)]
    pub level_configs: Vec<FibLevelConfig>,
    /// Per-type text properties captured via `Primitive::text_properties()`,
    /// restored via `Primitive::apply_text_property()` — the last-used-style
    /// analogue of `PrimitiveTemplate::custom_text_properties`. Distinct from
    /// `style_properties` above, which only covers `Primitive::style_properties()`.
    #[serde(default)]
    pub text_properties: Vec<(String, PropertyValue)>,
}
