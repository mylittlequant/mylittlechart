//! Primitive Configuration System
//!
//! Provides a unified way to configure primitives through:
//! - Inline toolbar (quick settings: color, width, style)
//! - Context menu (clone, delete, lock, visibility, layer order)
//! - Settings modal (full configuration with tabs: Style, Coordinates, Visibility)
//!
//! # Architecture
//!
//! Each primitive can expose its configurable properties through the `Configurable` trait.
//! Properties are described as `ConfigProperty` which includes:
//! - Property ID and display name
//! - Property type (color, number, boolean, select, levels, etc.)
//! - Current value
//! - Constraints (min/max, options, etc.)

use serde::{Deserialize, Serialize};
use crate::i18n::{Language, WaveDegreeKey, MenuKey, StyleKey, ConfigKey, LabelPositionKey, current_language};

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
    /// Background color for label (if None, no background)
    pub background_color: Option<String>,
    /// Background padding in pixels
    #[serde(default = "default_label_padding")]
    pub background_padding: f64,
    /// Background corner radius
    #[serde(default = "default_label_radius")]
    pub background_radius: f64,
    /// Border color (if None, no border)
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
            background_color: None,
            background_padding: 4.0,
            background_radius: 3.0,
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

fn default_fill_opacity() -> f64 { 0.1 }

impl FibLevelConfig {
    pub fn new(level: f64) -> Self {
        Self {
            level,
            visible: true,
            color: None,
            width: None,
            style: "solid".to_string(),
            fill_color: None,
            fill_opacity: 0.1,
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
            fill_opacity: 0.1,
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
            fill_opacity: 0.1,
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

/// Category for grouping properties in UI
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyCategory {
    /// Style tab - colors, line styles, fills
    Style,
    /// Text tab - text content, font, alignment
    Text,
    /// Coordinates tab - points, bars, prices
    Coordinates,
    /// Levels tab - level mode, level configs
    Levels,
    /// Visibility tab - timeframe visibility
    Visibility,
    /// Inputs tab - specific parameters (like Fib levels)
    Inputs,
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
            category: PropertyCategory::Style,
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
            category: PropertyCategory::Style,
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
            category: PropertyCategory::Style,
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
            category: PropertyCategory::Style,
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
            category: PropertyCategory::Coordinates,
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
            category: PropertyCategory::Text,
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
            category: PropertyCategory::Text,
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
            category: PropertyCategory::Style,
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

    /// Create "Fill" boolean property with i18n
    pub fn fill(value: bool) -> Self {
        Self::boolean("fill", ConfigKey::Fill.get(current_language()), value)
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
// Context Menu Actions
// =============================================================================

/// Actions available in primitive context menu
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextMenuAction {
    /// Open settings modal
    OpenSettings,
    /// Delete primitive
    Delete,
    /// Clone primitive
    Clone,
    /// Copy to clipboard
    Copy,
    /// Lock/unlock editing
    ToggleLock,
    /// Hide/show primitive
    ToggleVisibility,
    /// Bring to front
    BringToFront,
    /// Send to back
    SendToBack,
    /// Bring forward one layer
    BringForward,
    /// Send backward one layer
    SendBackward,
    /// Apply to all charts (sync)
    SyncToAllCharts,
    /// Apply everywhere
    SyncEverywhere,
    /// Don't sync
    NoSync,
}

impl ContextMenuAction {
    /// Get label using current language
    pub fn label(&self) -> &'static str {
        self.label_for(current_language())
    }

    /// Get label for specified language
    pub fn label_for(&self, lang: Language) -> &'static str {
        self.i18n_key().get(lang)
    }

    /// Get i18n key for this action
    pub fn i18n_key(&self) -> MenuKey {
        match self {
            Self::OpenSettings => MenuKey::OpenSettings,
            Self::Delete => MenuKey::Delete,
            Self::Clone => MenuKey::Clone,
            Self::Copy => MenuKey::Copy,
            Self::ToggleLock => MenuKey::LockUnlock,
            Self::ToggleVisibility => MenuKey::ShowHide,
            Self::BringToFront => MenuKey::BringToFront,
            Self::SendToBack => MenuKey::SendToBack,
            Self::BringForward => MenuKey::BringForward,
            Self::SendBackward => MenuKey::SendBackward,
            Self::SyncToAllCharts => MenuKey::SyncToAllCharts,
            Self::SyncEverywhere => MenuKey::SyncEverywhere,
            Self::NoSync => MenuKey::NoSync,
        }
    }
}

// =============================================================================
// Configurable Trait
// =============================================================================

/// Trait for primitives that expose configuration properties
pub trait Configurable {
    /// Get all configurable properties
    fn get_properties(&self) -> Vec<ConfigProperty>;

    /// Set a property value by ID
    /// Returns true if property was found and updated
    fn set_property(&mut self, id: &str, value: PropertyValue) -> bool;

    /// Get available context menu actions
    fn context_menu_actions(&self) -> Vec<ContextMenuAction> {
        vec![
            ContextMenuAction::OpenSettings,
            ContextMenuAction::Delete,
            ContextMenuAction::Clone,
            ContextMenuAction::Copy,
            ContextMenuAction::ToggleLock,
            ContextMenuAction::ToggleVisibility,
            ContextMenuAction::BringToFront,
            ContextMenuAction::SendToBack,
        ]
    }

    /// Get timeframe visibility config (if supported)
    fn timeframe_visibility(&self) -> Option<&TimeframeVisibilityConfig> {
        None
    }

    /// Set timeframe visibility config
    fn set_timeframe_visibility(&mut self, _config: TimeframeVisibilityConfig) {
        // Default: do nothing (primitive doesn't support timeframe visibility)
    }
}

// =============================================================================
// Full Config Structure (for serialization to UI)
// =============================================================================

/// Full primitive configuration for UI
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PrimitiveFullConfig {
    /// Primitive ID
    pub id: u64,
    /// Type ID (e.g., "trend_line", "fib_retracement")
    pub type_id: String,
    /// Display name
    pub display_name: String,
    /// Is locked
    pub locked: bool,
    /// Is visible
    pub visible: bool,
    /// All properties grouped by category
    pub properties: Vec<ConfigProperty>,
    /// Available context menu actions
    pub actions: Vec<ContextMenuAction>,
}

impl PrimitiveFullConfig {
    /// Get properties by category
    pub fn properties_by_category(&self, category: PropertyCategory) -> Vec<&ConfigProperty> {
        self.properties
            .iter()
            .filter(|p| p.category == category)
            .collect()
    }

    /// Convert to JSON string
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

// =============================================================================
// Blanket Implementation for all Primitives
// =============================================================================

use super::Primitive;

/// Blanket implementation of Configurable for all Primitive types
/// This provides base configuration support (color, width, style, coordinates)
/// Individual primitives can override by implementing Configurable directly
impl<T: Primitive> Configurable for T {
    fn get_properties(&self) -> Vec<ConfigProperty> {
        let data = self.data();
        let mut props = data.base_properties();

        // Add primitive-specific style properties (show_labels, degree, etc.)
        props.extend(self.style_properties());

        // Add text properties if primitive has text
        props.extend(data.text_properties());

        // Add coordinate properties from points()
        let points = self.points();
        for (i, (bar, price)) in points.iter().enumerate() {
            props.push(
                ConfigProperty::coordinate(
                    &format!("point{}", i + 1),
                    &format!("Point {}", i + 1),
                    *bar,
                    *price,
                ).with_order(100 + i as i32)
            );
        }

        props
    }

    fn set_property(&mut self, id: &str, value: PropertyValue) -> bool {
        // Handle base properties
        if self.data_mut().apply_property(id, &value) {
            return true;
        }

        // Handle primitive-specific style properties
        if self.apply_style_property(id, &value) {
            return true;
        }

        // Handle coordinate properties (point1, point2, etc.)
        if id.starts_with("point") {
            if let Some((bar, price)) = value.as_coordinate() {
                if let Ok(idx) = id[5..].parse::<usize>() {
                    let idx = idx.saturating_sub(1); // point1 -> index 0
                    let mut points = self.points();
                    if idx < points.len() {
                        points[idx] = (bar, price);
                        self.set_points(&points);
                        return true;
                    }
                }
            }
        }

        false
    }

    fn timeframe_visibility(&self) -> Option<&TimeframeVisibilityConfig> {
        self.data().timeframe_visibility.as_ref()
    }

    fn set_timeframe_visibility(&mut self, config: TimeframeVisibilityConfig) {
        self.data_mut().timeframe_visibility = Some(config);
    }
}

// =============================================================================
// Settings Templates System
// =============================================================================

/// A saved template of primitive settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SettingsTemplate {
    /// Unique ID
    pub id: String,
    /// User-assigned name
    pub name: String,
    /// Type of primitive this applies to (e.g., "fib_retracement", "trend_line", or "*" for all)
    pub primitive_type: String,
    /// Style properties (color, width, line_style)
    pub style: TemplateStyle,
    /// Fib-specific settings (only for Fib primitives)
    pub fib_levels: Option<Vec<FibLevelConfig>>,
    /// Timeframe visibility (optional)
    pub timeframe_visibility: Option<TimeframeVisibilityConfig>,
    /// Is this a built-in template (non-deletable)
    pub builtin: bool,
    /// Creation timestamp
    pub created_at: u64,
}

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
}


impl SettingsTemplate {
    /// Create a new template with given name and type
    pub fn new(id: &str, name: &str, primitive_type: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            primitive_type: primitive_type.to_string(),
            style: TemplateStyle::default(),
            fib_levels: None,
            timeframe_visibility: None,
            builtin: false,
            created_at: 0,
        }
    }

    /// Create from primitive JSON
    pub fn from_primitive_json(id: &str, name: &str, primitive_type: &str, json: &str) -> Option<Self> {
        // Parse JSON to extract style properties
        let value: serde_json::Value = serde_json::from_str(json).ok()?;

        let mut template = Self::new(id, name, primitive_type);

        // Extract style from data.color (or direct color field)
        if let Some(data) = value.get("data") {
            if let Some(color) = data.get("color") {
                if let Some(stroke) = color.get("stroke").and_then(|s| s.as_str()) {
                    template.style.color = Some(stroke.to_string());
                }
            }
            if let Some(width) = data.get("width").and_then(|w| w.as_f64()) {
                template.style.width = Some(width);
            }
            if let Some(line_style) = data.get("line_style").and_then(|s| s.as_str()) {
                template.style.line_style = Some(line_style.to_string());
            }
            if let Some(show_labels) = data.get("show_labels").and_then(|s| s.as_bool()) {
                template.style.show_labels = Some(show_labels);
            }
            if let Some(show_prices) = data.get("show_prices").and_then(|s| s.as_bool()) {
                template.style.show_prices = Some(show_prices);
            }
        }

        // Extract Fib levels if present
        if let Some(levels) = value.get("level_configs") {
            if let Ok(fib_levels) = serde_json::from_value::<Vec<FibLevelConfig>>(levels.clone()) {
                template.fib_levels = Some(fib_levels);
            }
        }

        // Extract timeframe visibility
        if let Some(data) = value.get("data") {
            if let Some(tfv) = data.get("timeframe_visibility") {
                if let Ok(config) = serde_json::from_value::<TimeframeVisibilityConfig>(tfv.clone()) {
                    template.timeframe_visibility = Some(config);
                }
            }
        }

        Some(template)
    }

    /// Get builtin templates for a primitive type
    pub fn builtin_templates(primitive_type: &str) -> Vec<Self> {
        match primitive_type {
            "fib_retracement" => vec![
                Self::fib_standard(),
                Self::fib_extended(),
                Self::fib_colored_fills(),
            ],
            "trend_line" => vec![
                Self::line_standard(),
                Self::line_thick(),
                Self::line_dashed(),
            ],
            _ => vec![],
        }
    }

    // Built-in Fibonacci templates
    fn fib_standard() -> Self {
        use super::fibonacci::retracement::default_level_configs;
        Self {
            id: "fib_standard".to_string(),
            name: StyleKey::Standard.get(current_language()).to_string(),
            primitive_type: "fib_retracement".to_string(),
            style: TemplateStyle {
                color: Some("#787b86".to_string()),
                width: Some(1.0),
                line_style: Some("solid".to_string()),
                ..Default::default()
            },
            fib_levels: Some(default_level_configs()),
            timeframe_visibility: None,
            builtin: true,
            created_at: 0,
        }
    }

    fn fib_extended() -> Self {
        use super::fibonacci::retracement::extended_level_configs;
        Self {
            id: "fib_extended".to_string(),
            name: StyleKey::Extended.get(current_language()).to_string(),
            primitive_type: "fib_retracement".to_string(),
            style: TemplateStyle {
                color: Some("#787b86".to_string()),
                width: Some(1.0),
                line_style: Some("solid".to_string()),
                ..Default::default()
            },
            fib_levels: Some(extended_level_configs()),
            timeframe_visibility: None,
            builtin: true,
            created_at: 0,
        }
    }

    fn fib_colored_fills() -> Self {
        use super::fibonacci::retracement::filled_level_configs;
        Self {
            id: "fib_filled".to_string(),
            name: StyleKey::Filled.get(current_language()).to_string(),
            primitive_type: "fib_retracement".to_string(),
            style: TemplateStyle {
                color: Some("#787b86".to_string()),
                width: Some(1.0),
                line_style: Some("solid".to_string()),
                ..Default::default()
            },
            fib_levels: Some(filled_level_configs()),
            timeframe_visibility: None,
            builtin: true,
            created_at: 0,
        }
    }

    // Built-in line templates
    fn line_standard() -> Self {
        Self {
            id: "line_standard".to_string(),
            name: StyleKey::Standard.get(current_language()).to_string(),
            primitive_type: "trend_line".to_string(),
            style: TemplateStyle {
                color: Some("#2962ff".to_string()),
                width: Some(1.0),
                line_style: Some("solid".to_string()),
                ..Default::default()
            },
            fib_levels: None,
            timeframe_visibility: None,
            builtin: true,
            created_at: 0,
        }
    }

    fn line_thick() -> Self {
        Self {
            id: "line_thick".to_string(),
            name: StyleKey::Thick.get(current_language()).to_string(),
            primitive_type: "trend_line".to_string(),
            style: TemplateStyle {
                color: Some("#2962ff".to_string()),
                width: Some(3.0),
                line_style: Some("solid".to_string()),
                ..Default::default()
            },
            fib_levels: None,
            timeframe_visibility: None,
            builtin: true,
            created_at: 0,
        }
    }

    fn line_dashed() -> Self {
        Self {
            id: "line_dashed".to_string(),
            name: StyleKey::Dashed.get(current_language()).to_string(),
            primitive_type: "trend_line".to_string(),
            style: TemplateStyle {
                color: Some("#787b86".to_string()),
                width: Some(1.0),
                line_style: Some("dashed".to_string()),
                ..Default::default()
            },
            fib_levels: None,
            timeframe_visibility: None,
            builtin: true,
            created_at: 0,
        }
    }

    /// Convert to JSON
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Parse from JSON
    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }
}

/// Collection of templates
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TemplateCollection {
    /// User-created templates
    pub templates: Vec<SettingsTemplate>,
}

impl TemplateCollection {
    /// Create empty collection
    pub fn new() -> Self {
        Self { templates: Vec::new() }
    }

    /// Add a template
    pub fn add(&mut self, template: SettingsTemplate) {
        // Remove existing template with same ID
        self.templates.retain(|t| t.id != template.id);
        self.templates.push(template);
    }

    /// Remove a template by ID
    pub fn remove(&mut self, id: &str) -> bool {
        let len_before = self.templates.len();
        self.templates.retain(|t| t.id != id || t.builtin);
        self.templates.len() < len_before
    }

    /// Get template by ID
    pub fn get(&self, id: &str) -> Option<&SettingsTemplate> {
        self.templates.iter().find(|t| t.id == id)
    }

    /// Get all templates for a primitive type (including built-in)
    pub fn templates_for_type(&self, primitive_type: &str) -> Vec<&SettingsTemplate> {
        self.templates
            .iter()
            .filter(|t| t.primitive_type == primitive_type || t.primitive_type == "*")
            .collect()
    }

    /// Get combined list of builtin + user templates for a type
    pub fn all_templates_for_type(&self, primitive_type: &str) -> Vec<SettingsTemplate> {
        let mut result = SettingsTemplate::builtin_templates(primitive_type);
        for t in &self.templates {
            if t.primitive_type == primitive_type || t.primitive_type == "*" {
                result.push(t.clone());
            }
        }
        result
    }

    /// To JSON
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// From JSON
    pub fn from_json(json: &str) -> Self {
        serde_json::from_str(json).unwrap_or_default()
    }
}
