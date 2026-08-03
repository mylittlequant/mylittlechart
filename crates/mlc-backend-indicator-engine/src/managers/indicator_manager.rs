//! Indicator value types + compute cache.
//!
//! Compute-only half of the indicator system (doctrine 3.7a split): pure
//! data describing an indicator's catalog metadata (`IndicatorDefinition`),
//! a configured instance (`IndicatorInstance`), and the incremental compute
//! scratch state (`ComputeCache`). The stateful registry that owns instances
//! and drives recalculation — `IndicatorManager` — lives in
//! `mlc_backend::indicator::manager` (see
//! `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A/§1B).
//!
//! - Configurable parameters
//! - Multiple outputs (e.g., MACD has signal line, histogram)
//! - Visibility/lock/timeframe integration
//! - Full indicator catalog (480+ indicators from zengeld-chart-indicators)

use mli::data_loader::stream_kind::StreamKind;
use std::collections::HashMap;
use std::sync::Arc;
use serde::{Serialize, Deserialize};
// Re-export BarIndicatorId for use in IndicatorDefinition
pub use crate::BarIndicatorId;
// Signal system imports
pub use crate::signals::signal::Signal;
// IndicatorCategory comes from zengeld-chart
pub use mlc_core::state::modal_state::IndicatorCategory;

/// Controls when indicators are recalculated relative to incoming trade data.
///
/// Choosing a coarser mode (PerFrame, PerBar) reduces CPU usage at the cost
/// of indicator values being updated less frequently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[derive(Default)]
pub enum RecalcMode {
    /// Recalculate on every incoming trade (highest accuracy, highest CPU).
    PerTick,
    /// Recalculate once per render frame, batching all trades that arrived
    /// since the last frame (default — good balance of accuracy and CPU).
    #[default]
    PerFrame,
    /// Recalculate only when a new bar forms (lowest CPU usage).
    PerBar,
}


/// How histogram bars are rendered
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HistogramStyle {
    /// Bars grow from bottom (default for volume)
    #[default]
    FromBottom,
    /// Bars centered on zero line (MACD style)
    Centered,
}

/// Indicator output type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndicatorOutputType {
    /// Single line plot
    Line,
    /// Histogram bars
    Histogram,
    /// Area fill
    Area,
    /// Band (upper + lower)
    Band,
    /// Dots/markers
    Dots,
    /// Background color zones
    Background,
}

impl IndicatorOutputType {
    /// Get string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Line => "Line",
            Self::Histogram => "Histogram",
            Self::Area => "Area",
            Self::Band => "Band",
            Self::Dots => "Dots",
            Self::Background => "Background",
        }
    }
}

/// Indicator parameter type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IndicatorParamType {
    /// Integer value
    Int { min: i32, max: i32, step: i32 },
    /// Float value
    Float { min: f64, max: f64, step: f64 },
    /// Boolean toggle
    Bool,
    /// Selection from options
    Select { options: Vec<String> },
    /// Color picker
    Color,
    /// Source selection (close, open, high, low, hl2, etc.)
    Source,
    /// A SET of exchanges, chosen from what the host can actually serve for
    /// this indicator's declared stream. Distinct from `Select`: multiple
    /// values, and the options come from the host at open time rather than
    /// from the definition — a venue list baked into a definition is stale
    /// the moment a connector gains or loses an endpoint.
    Venues,
}

impl IndicatorParamType {
    /// Get dropdown options for Select and Source parameter types
    /// Returns empty slice for non-dropdown types
    pub fn get_dropdown_options(&self) -> &[String] {
        match self {
            IndicatorParamType::Select { options } => options.as_slice(),
            _ => &[],
        }
    }

    /// Get static source options for Source parameter type
    /// These are the canonical price source options
    pub fn source_options() -> &'static [&'static str] {
        &["close", "open", "high", "low", "hl2", "hlc3", "ohlc4"]
    }
}

/// Indicator parameter definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorParam {
    pub name: String,
    pub display_name: String,
    pub param_type: IndicatorParamType,
    pub default_value: IndicatorValue,
    pub group: Option<String>,
}

impl IndicatorParam {
    pub fn int(name: &str, display: &str, default: i32, min: i32, max: i32) -> Self {
        Self {
            name: name.to_string(),
            display_name: display.to_string(),
            param_type: IndicatorParamType::Int { min, max, step: 1 },
            default_value: IndicatorValue::Int(default),
            group: None,
        }
    }

    pub fn float(name: &str, display: &str, default: f64, min: f64, max: f64) -> Self {
        Self {
            name: name.to_string(),
            display_name: display.to_string(),
            param_type: IndicatorParamType::Float { min, max, step: 0.1 },
            default_value: IndicatorValue::Float(default),
            group: None,
        }
    }

    pub fn bool(name: &str, display: &str, default: bool) -> Self {
        Self {
            name: name.to_string(),
            display_name: display.to_string(),
            param_type: IndicatorParamType::Bool,
            default_value: IndicatorValue::Bool(default),
            group: None,
        }
    }

    pub fn color(name: &str, display: &str, default: &str) -> Self {
        Self {
            name: name.to_string(),
            display_name: display.to_string(),
            param_type: IndicatorParamType::Color,
            default_value: IndicatorValue::Color(default.to_string()),
            group: None,
        }
    }

    pub fn source(name: &str, display: &str) -> Self {
        Self {
            name: name.to_string(),
            display_name: display.to_string(),
            param_type: IndicatorParamType::Source,
            default_value: IndicatorValue::String("close".to_string()),
            group: None,
        }
    }

    /// A set of exchanges this layer folds — see
    /// [`IndicatorParamType::Venues`]. The options are NOT stored on the
    /// definition: they depend on which connectors serve this indicator's
    /// declared stream, which is host state and changes at runtime.
    pub fn venues(name: &str, display: &str) -> Self {
        Self {
            name: name.to_string(),
            display_name: display.to_string(),
            param_type: IndicatorParamType::Venues,
            default_value: IndicatorValue::StringList(Vec::new()),
            group: None,
        }
    }

    pub fn select(name: &str, display: &str, options: Vec<String>, default_index: usize) -> Self {
        let default_value = options.get(default_index).cloned().unwrap_or_else(|| options[0].clone());
        Self {
            name: name.to_string(),
            display_name: display.to_string(),
            param_type: IndicatorParamType::Select { options },
            default_value: IndicatorValue::String(default_value),
            group: None,
        }
    }

    pub fn with_group(mut self, group: &str) -> Self {
        self.group = Some(group.to_string());
        self
    }

    /// Get the list of available options for dropdown parameters
    /// Returns the options for Select params or source options for Source params
    pub fn get_options_as_strings(&self) -> Vec<String> {
        match &self.param_type {
            IndicatorParamType::Select { options } => options.clone(),
            IndicatorParamType::Source => {
                IndicatorParamType::source_options()
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            }
            _ => Vec::new(),
        }
    }
}

/// Indicator parameter value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IndicatorValue {
    Int(i32),
    Float(f64),
    Bool(bool),
    String(String),
    Color(String),
    /// A set of names — today the venues an aggregating overlay folds. Empty
    /// means "everything that serves this layer's stream", not "nothing":
    /// narrowing is a deliberate act.
    StringList(Vec<String>),
}

impl IndicatorValue {
    pub fn as_int(&self) -> Option<i32> {
        match self {
            IndicatorValue::Int(v) => Some(*v),
            IndicatorValue::Float(v) => Some(*v as i32),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            IndicatorValue::Float(v) => Some(*v),
            IndicatorValue::Int(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            IndicatorValue::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&str> {
        match self {
            IndicatorValue::String(v) | IndicatorValue::Color(v) => Some(v),
            _ => None,
        }
    }

    /// The chosen names, for a set-valued parameter.
    pub fn as_string_list(&self) -> Option<&[String]> {
        match self {
            IndicatorValue::StringList(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    /// Convert to display string for UI
    pub fn to_display_string(&self) -> String {
        match self {
            IndicatorValue::Int(v) => v.to_string(),
            IndicatorValue::Float(v) => format!("{:.2}", v),
            IndicatorValue::Bool(v) => v.to_string(),
            IndicatorValue::String(v) | IndicatorValue::Color(v) => v.clone(),
            // Comma-joined for display ONLY — the stored value stays a list.
            // Empty reads as the word, not as blank, because blank looks like
            // a control that failed to load.
            IndicatorValue::StringList(v) if v.is_empty() => "All".to_string(),
            IndicatorValue::StringList(v) => v.join(", "),
        }
    }
}

/// Indicator output definition
#[derive(Debug, Clone)]
pub struct IndicatorOutput {
    pub name: String,
    pub display_name: String,
    pub output_type: IndicatorOutputType,
    pub color: String,
    pub line_width: f32,
    pub visible: bool,
}

impl IndicatorOutput {
    pub fn line(name: &str, display: &str, color: &str) -> Self {
        Self {
            name: name.to_string(),
            display_name: display.to_string(),
            output_type: IndicatorOutputType::Line,
            color: color.to_string(),
            line_width: 1.0,
            visible: true,
        }
    }

    pub fn histogram(name: &str, display: &str, color: &str) -> Self {
        Self {
            name: name.to_string(),
            display_name: display.to_string(),
            output_type: IndicatorOutputType::Histogram,
            color: color.to_string(),
            line_width: 1.0,
            visible: true,
        }
    }

    pub fn band(name: &str, display: &str, color: &str) -> Self {
        Self {
            name: name.to_string(),
            display_name: display.to_string(),
            output_type: IndicatorOutputType::Band,
            color: color.to_string(),
            line_width: 1.0,
            visible: true,
        }
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.line_width = width;
        self
    }
}

/// Indicator definition (metadata)
#[derive(Debug, Clone)]
pub struct IndicatorDefinition {
    /// Unique type ID
    pub type_id: String,
    /// Display name
    pub name: String,
    /// Short name for display
    pub short_name: String,
    /// Description
    pub description: String,
    /// Category
    pub category: IndicatorCategory,
    /// Parameter definitions
    pub params: Vec<IndicatorParam>,
    /// Output definitions
    pub outputs: Vec<IndicatorOutput>,
    /// Whether this indicator is overlay (drawn on price) or separate pane
    pub overlay: bool,
    /// Precision for value display
    pub precision: u32,
    /// Fixed Y-axis bounds (e.g., Some((0.0, 100.0)) for RSI/Stoch)
    /// None means auto-scale based on data
    pub bounds: Option<(f64, f64)>,
    /// Whether to extend Y range to include zero (for MACD, volume)
    pub zero_baseline: bool,
    /// How histogram bars are rendered
    pub histogram_style: HistogramStyle,
    /// Machine ID for fast factory access (typed enum instead of string parsing)
    pub machine_id: Option<BarIndicatorId>,
    /// Data streams this indicator DECLARES it consumes, carried verbatim
    /// from its mli catalog signature (`input_stream` first, then
    /// `aux_streams`). The host arms live subscriptions from this instead
    /// of matching indicator ids — an overlay that needs the order book
    /// says so in its signature, and nothing in the app has to know its
    /// name. Empty only for definitions built outside the catalog.
    pub streams: Vec<StreamKind>,
}

/// An instantiated indicator with configured parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorInstance {
    /// Unique instance ID
    pub id: u64,
    /// Type ID (references IndicatorDefinition)
    pub type_id: String,
    /// Display name (can be customized)
    pub name: String,
    /// Configured parameter values
    pub params: HashMap<String, IndicatorValue>,
    /// Output configurations (visibility, color overrides)
    pub outputs: HashMap<String, OutputConfig>,
    /// Whether indicator is visible
    pub visible: bool,
    /// Whether indicator is locked
    pub locked: bool,
    /// Timeframe visibility config (same as primitives)
    pub timeframe_visibility: Option<mlc_core::drawing::TimeframeVisibilityConfig>,
    /// Pane index (0 = main chart, 1+ = separate panes)
    pub pane: usize,
    /// Order within pane
    pub order: i32,
    /// Computed values (populated by calculation) — runtime only, not persisted.
    /// Wrapped in `Arc` so that cloning a render instance shares the buffer
    /// instead of deep-copying it (O(1) ref-count bump vs O(n) data copy).
    /// On each recalculation a fresh `Arc::new(...)` is swapped in, so readers
    /// holding an old `Arc` clone see a stable snapshot while new data arrives.
    #[serde(skip)]
    pub values: Arc<HashMap<String, Vec<f64>>>,
    /// Bubble id the indicator lives in (i.e. the color tag / sync group /
    /// home auto-bubble).  Indicator ownership is by bubble, NOT by chart
    /// window — every chart whose `current_tag_id` matches this value
    /// renders this indicator with its own per-chart render rules
    /// (overlay vs sub-pane).  When a chart switches tags, its render set
    /// switches with it without touching indicator state.
    ///
    /// Persisted form keeps the legacy `window_id` JSON key as a serde
    /// alias so older preset files still deserialize during the cutover.
    #[serde(alias = "window_id")]
    pub bubble_id: Option<u64>,
    /// Origin instance ID — set when this instance was cloned for a sync group peer.
    /// None means this is an original (not a clone).
    pub origin_id: Option<u64>,
    /// Generated signals (populated by calculation) — runtime only, not persisted
    #[serde(skip)]
    pub signals: Vec<Signal>,
    /// Whether signal generation is enabled for this instance
    pub signals_enabled: bool,
    /// Visual configuration for signal markers on chart
    #[serde(default)]
    pub signal_display: mlc_core::indicator_source::SignalDisplayConfig,
}

/// Output configuration for an instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub visible: bool,
    pub color: Option<String>,
    pub line_width: Option<f32>,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            visible: true,
            color: None,
            line_width: None,
        }
    }
}

impl IndicatorInstance {
    /// Create a new indicator instance
    pub fn new(id: u64, definition: &IndicatorDefinition) -> Self {
        let mut params = HashMap::new();
        for param in &definition.params {
            params.insert(param.name.clone(), param.default_value.clone());
        }

        let mut outputs = HashMap::new();
        for output in &definition.outputs {
            outputs.insert(output.name.clone(), OutputConfig::default());
        }

        Self {
            id,
            type_id: definition.type_id.clone(),
            name: definition.short_name.clone(),
            params,
            outputs,
            visible: true,
            locked: false,
            timeframe_visibility: None, // None = visible on all timeframes
            pane: if definition.overlay { 0 } else { 1 },
            order: 0,
            values: Arc::new(HashMap::new()),
            bubble_id: None,
            origin_id: None,
            signals: Vec::new(),
            signals_enabled: true, // Enabled by default
            signal_display: Default::default(),
        }
    }

    /// Get parameter value
    pub fn get_param(&self, name: &str) -> Option<&IndicatorValue> {
        self.params.get(name)
    }

    /// Set parameter value
    pub fn set_param(&mut self, name: &str, value: IndicatorValue) {
        self.params.insert(name.to_string(), value);
    }

    /// Get computed values for an output
    pub fn get_values(&self, output: &str) -> Option<&Vec<f64>> {
        self.values.get(output)
    }

    /// Set computed values for one output key.
    ///
    /// Mutably borrows the inner `HashMap` via `Arc::make_mut`, which performs a
    /// clone of the map only if another `Arc` clone currently holds a reference
    /// (copy-on-write).  In the common recalc path the `Arc` is not shared during
    /// the write, so this is a direct in-place mutation.
    pub fn set_values(&mut self, output: &str, values: Vec<f64>) {
        Arc::make_mut(&mut self.values).insert(output.to_string(), values);
    }

    /// Get formatted title with params (e.g., "SMA(20)")
    pub fn title(&self) -> String {
        // Simplified - just return name with first int param
        for value in self.params.values() {
            if let IndicatorValue::Int(v) = value {
                return format!("{}({})", self.name, v);
            }
        }
        self.name.clone()
    }

    /// Set timeframe visibility config
    pub fn set_timeframe_visibility(&mut self, config: mlc_core::drawing::TimeframeVisibilityConfig) {
        self.timeframe_visibility = Some(config);
    }

    /// Check if indicator is visible on current timeframe
    pub fn is_visible_on_timeframe(&self, timeframe_label: &str) -> bool {
        match &self.timeframe_visibility {
            Some(config) => config.is_visible_on_label(timeframe_label),
            None => true, // No config = visible on all
        }
    }

    /// Serialize params to JSON string for undo/redo
    pub fn params_json(&self) -> String {
        // Simple serialization of params to JSON
        let mut parts = Vec::new();
        for (key, value) in &self.params {
            let val_str = match value {
                IndicatorValue::Int(v) => v.to_string(),
                IndicatorValue::Float(v) => v.to_string(),
                IndicatorValue::Bool(v) => v.to_string(),
                IndicatorValue::String(v) => format!("\"{}\"", v),
                IndicatorValue::Color(v) => format!("\"{}\"", v),
                // A real JSON array — this string is the undo/redo
                // fingerprint, so it must change when the SET changes.
                IndicatorValue::StringList(v) => {
                    let items: Vec<String> = v.iter().map(|s| format!("\"{}\"", s)).collect();
                    format!("[{}]", items.join(","))
                }
            };
            parts.push(format!("\"{}\":{}", key, val_str));
        }
        format!("{{{}}}", parts.join(","))
    }
}

/// Runtime stateful compute cache for one indicator on one window.
///
/// Lives in `IndicatorManager::compute_caches` (keyed by `IndicatorInstance.id`),
/// NOT inside `IndicatorInstance`, so `IndicatorInstance` stays `Clone + Serialize`.
///
/// Dropped whenever the indicator is removed, params change, or bars are reset.
/// `pub` (not `pub(crate)`) because `IndicatorManager` — the sole owner of
/// `compute_caches` — lives in `mlc-backend` since the 3.7a manager lift.
pub struct ComputeCache {
    /// The live stateful compute engine (480-variant enum, Clone).
    pub instance: crate::IndicatorInstance,
    /// Snapshot of `instance` at N-1 bars (before the last bar was fed).
    /// Used to avoid full O(N) recalc when the live bar is updated by a new tick:
    /// restore from checkpoint, feed the updated bar, replace last output value.
    pub checkpoint: Option<crate::IndicatorInstance>,
    /// How many bars were fed into `instance` so far.
    pub fed_bar_count: usize,
    /// FNV-1a fingerprint of (indicator_id, params) at cache creation time.
    /// A mismatch triggers a full rebuild.
    pub config_fingerprint: u64,
    /// Snapshot of the last bar fed, used to detect last-bar updates.
    pub last_fed_bar: Option<crate::Bar>,
    /// Signal engine for incremental signal generation; present only when
    /// `signals_enabled == true` and a default signal profile exists.
    pub signal_engine: Option<crate::signals::rules::SignalEngine>,
}
