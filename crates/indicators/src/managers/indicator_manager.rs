//! Indicator System
//!
//! Provides a framework for technical indicators with:
//! - Configurable parameters
//! - Multiple outputs (e.g., MACD has signal line, histogram)
//! - Visibility/lock/timeframe integration
//! - Full indicator catalog (480+ indicators from zengeld-chart-indicators)

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use super::indicator_bridge::IndicatorBridge;
// Re-export BarIndicatorId for use in IndicatorDefinition
pub use crate::BarIndicatorId;
// Signal system imports
pub use crate::signals::SignalEvent;
// IndicatorCategory comes from zengeld-chart
pub use zengeld_chart::ui::modal_state::IndicatorCategory;

/// Controls when indicators are recalculated relative to incoming trade data.
///
/// Choosing a coarser mode (PerFrame, PerBar) reduces CPU usage at the cost
/// of indicator values being updated less frequently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RecalcMode {
    /// Recalculate on every incoming trade (highest accuracy, highest CPU).
    PerTick,
    /// Recalculate once per render frame, batching all trades that arrived
    /// since the last frame (default — good balance of accuracy and CPU).
    PerFrame,
    /// Recalculate only when a new bar forms (lowest CPU usage).
    PerBar,
}

impl Default for RecalcMode {
    fn default() -> Self {
        RecalcMode::PerFrame
    }
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

    /// Convert to display string for UI
    pub fn to_display_string(&self) -> String {
        match self {
            IndicatorValue::Int(v) => v.to_string(),
            IndicatorValue::Float(v) => format!("{:.2}", v),
            IndicatorValue::Bool(v) => v.to_string(),
            IndicatorValue::String(v) | IndicatorValue::Color(v) => v.clone(),
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
    pub timeframe_visibility: Option<zengeld_chart::drawing::TimeframeVisibilityConfig>,
    /// Symbol this indicator is on
    pub symbol: String,
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
    /// Window ID (for multi-window support)
    pub window_id: Option<u64>,
    /// Origin instance ID — set when this instance was cloned for a sync group peer.
    /// None means this is an original (not a clone).
    pub origin_id: Option<u64>,
    /// Generated signals (populated by calculation) — runtime only, not persisted
    #[serde(skip)]
    pub signals: Vec<SignalEvent>,
    /// Whether signal generation is enabled for this instance
    pub signals_enabled: bool,
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
    pub fn new(id: u64, definition: &IndicatorDefinition, symbol: &str) -> Self {
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
            symbol: symbol.to_string(),
            pane: if definition.overlay { 0 } else { 1 },
            order: 0,
            values: Arc::new(HashMap::new()),
            window_id: None,
            origin_id: None,
            signals: Vec::new(),
            signals_enabled: true, // Enabled by default
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
        for (_, value) in &self.params {
            if let IndicatorValue::Int(v) = value {
                return format!("{}({})", self.name, v);
            }
        }
        self.name.clone()
    }

    /// Set timeframe visibility config
    pub fn set_timeframe_visibility(&mut self, config: zengeld_chart::drawing::TimeframeVisibilityConfig) {
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
            };
            parts.push(format!("\"{}\":{}", key, val_str));
        }
        format!("{{{}}}", parts.join(","))
    }
}

/// Manages indicator definitions and instances
pub struct IndicatorManager {
    /// Available indicator definitions
    definitions: HashMap<String, IndicatorDefinition>,
    /// Active indicator instances
    instances: HashMap<u64, IndicatorInstance>,
    /// Instances by symbol
    by_symbol: HashMap<String, Vec<u64>>,
    /// Next instance ID
    next_id: u64,
    /// Transient: set before a render call to scope queries to a specific window.
    /// When Some(x), get_instances_for_symbol and get_render_instances_for_symbol
    /// only return instances where window_id == Some(x).
    /// When None, all instances are returned (legacy single-window behavior).
    ///
    /// Uses `Cell` so that `render_to_scene(&self)` can set/clear this field
    /// via interior mutability without requiring `&mut self`.
    pub current_render_window_id: Cell<Option<u64>>,

    // ── Recalculation scheduling ──────────────────────────────────────────────

    /// Current recalculation mode (controls when indicator values are refreshed).
    pub recalc_mode: RecalcMode,
    /// Symbols that received at least one trade update since the last flush.
    /// Used by PerFrame mode — all dirty symbols are recalculated once per frame.
    dirty_symbols: HashSet<String>,
    /// Symbols where a new bar formed since the last flush.
    /// Used by PerBar mode — only these symbols trigger recalculation.
    new_bar_symbols: HashSet<String>,
}

impl IndicatorManager {
    /// Create a new indicator manager with the full catalog (480+ indicators)
    pub fn new() -> Self {
        let mut manager = Self {
            definitions: HashMap::new(),
            instances: HashMap::new(),
            by_symbol: HashMap::new(),
            next_id: 1,
            current_render_window_id: Cell::new(None),
            recalc_mode: RecalcMode::default(),
            dirty_symbols: HashSet::new(),
            new_bar_symbols: HashSet::new(),
        };
        manager.register_from_catalog();
        manager
    }

    /// Remove all indicator instances. Used during preset restore.
    ///
    /// Clears active instances and per-symbol indices, then resets the ID
    /// counter to 1 so freshly inserted indicators receive predictable IDs.
    /// Catalog definitions are preserved — they are static and do not need
    /// to be rebuilt after a clear.
    pub fn clear_all(&mut self) {
        self.instances.clear();
        self.by_symbol.clear();
        self.next_id = 1;
        self.current_render_window_id.set(None);
        self.dirty_symbols.clear();
        self.new_bar_symbols.clear();
    }

    /// Register all indicators from the unified catalog (480+ indicators)
    fn register_from_catalog(&mut self) {
        let bridge = IndicatorBridge::new();
        let definitions = bridge.get_all_definitions();

        for definition in definitions {
            self.definitions.insert(definition.type_id.clone(), definition);
        }
    }

    /// Register a new indicator definition
    pub fn register_definition(&mut self, definition: IndicatorDefinition) {
        self.definitions.insert(definition.type_id.clone(), definition);
    }

    /// Get all available definitions
    pub fn get_definitions(&self) -> Vec<&IndicatorDefinition> {
        self.definitions.values().collect()
    }

    /// Get definition by type ID
    pub fn get_definition(&self, type_id: &str) -> Option<&IndicatorDefinition> {
        self.definitions.get(type_id)
    }

    /// Get definitions by category
    pub fn get_by_category(&self, category: IndicatorCategory) -> Vec<&IndicatorDefinition> {
        self.definitions
            .values()
            .filter(|d| d.category == category)
            .collect()
    }

    /// Create a new indicator instance
    pub fn create_instance(&mut self, type_id: &str, symbol: &str) -> Option<u64> {
        let definition = self.definitions.get(type_id)?.clone();
        let id = self.next_id;
        self.next_id += 1;

        let instance = IndicatorInstance::new(id, &definition, symbol);

        self.instances.insert(id, instance);
        self.by_symbol
            .entry(symbol.to_string())
            .or_insert_with(Vec::new)
            .push(id);

        Some(id)
    }

    /// Create instance with a specific ID (for undo/redo)
    /// Returns true if created successfully, false if definition not found or ID already exists
    pub fn create_instance_with_id(&mut self, id: u64, type_id: &str, symbol: &str) -> bool {
        // Check if ID already exists
        if self.instances.contains_key(&id) {
            return false;
        }

        let definition = match self.definitions.get(type_id) {
            Some(d) => d.clone(),
            None => return false,
        };

        let instance = IndicatorInstance::new(id, &definition, symbol);

        self.instances.insert(id, instance);
        self.by_symbol
            .entry(symbol.to_string())
            .or_insert_with(Vec::new)
            .push(id);

        // Update next_id if needed
        if id >= self.next_id {
            self.next_id = id + 1;
        }

        true
    }

    /// Remove an instance
    pub fn remove_instance(&mut self, id: u64) -> Option<IndicatorInstance> {
        if let Some(instance) = self.instances.remove(&id) {
            if let Some(ids) = self.by_symbol.get_mut(&instance.symbol) {
                ids.retain(|&i| i != id);
            }
            Some(instance)
        } else {
            None
        }
    }

    /// Remove an instance (alias for remove_instance)
    pub fn remove(&mut self, id: u64) -> Option<IndicatorInstance> {
        self.remove_instance(id)
    }

    /// Remove all cloned indicator instances for a specific window (used on desync).
    /// DEPRECATED: Only used by legacy origin_id-based sync. TagManager uses
    /// pre_tag_indicator_ids filtering instead.
    pub fn purge_synced_instances_for_window(&mut self, target_window_id: u64) {
        let to_remove: Vec<u64> = self.instances.values()
            .filter(|inst| inst.origin_id.is_some() && inst.window_id == Some(target_window_id))
            .map(|inst| inst.id)
            .collect();

        for id in to_remove {
            if let Some(inst) = self.instances.remove(&id) {
                if let Some(ids) = self.by_symbol.get_mut(&inst.symbol) {
                    ids.retain(|&i| i != id);
                }
            }
        }
    }

    /// Remove only indicator instances whose type_id is in `group_type_ids` for the given window.
    /// Used when disconnecting from a TagManager group — removes group-owned indicators
    /// but keeps any pre-existing indicators the window had before joining.
    pub fn purge_group_indicators_for_window(&mut self, target_window_id: u64, group_type_ids: &[String]) {
        let to_remove: Vec<u64> = self.instances.values()
            .filter(|inst| {
                inst.window_id == Some(target_window_id)
                    && group_type_ids.contains(&inst.type_id)
            })
            .map(|inst| inst.id)
            .collect();

        let count = to_remove.len();
        for id in to_remove {
            if let Some(inst) = self.instances.remove(&id) {
                if let Some(ids) = self.by_symbol.get_mut(&inst.symbol) {
                    ids.retain(|&i| i != id);
                }
            }
        }
        if count > 0 {
            eprintln!("[IndicatorManager] Purged {} group indicators for window {}", count, target_window_id);
        }
    }

    /// Remove ALL indicator instances belonging to a window (regardless of origin_id).
    /// Used when disconnecting from a TagManager group — the group owns the indicators.
    pub fn purge_all_instances_for_window(&mut self, target_window_id: u64) {
        let to_remove: Vec<u64> = self.instances.values()
            .filter(|inst| inst.window_id == Some(target_window_id))
            .map(|inst| inst.id)
            .collect();

        let count = to_remove.len();
        for id in to_remove {
            if let Some(inst) = self.instances.remove(&id) {
                if let Some(ids) = self.by_symbol.get_mut(&inst.symbol) {
                    ids.retain(|&i| i != id);
                }
            }
        }
        if count > 0 {
            eprintln!("[IndicatorManager] Purged all {} instances for window {}", count, target_window_id);
        }
    }

    /// Toggle visibility of an instance
    /// Returns the new visibility state or None if instance not found
    pub fn toggle_visibility(&mut self, id: u64) -> Option<bool> {
        if let Some(instance) = self.instances.get_mut(&id) {
            instance.visible = !instance.visible;
            Some(instance.visible)
        } else {
            None
        }
    }

    /// Set timeframe visibility config for an instance
    pub fn set_timeframe_visibility(&mut self, id: u64, config: zengeld_chart::drawing::TimeframeVisibilityConfig) {
        if let Some(instance) = self.instances.get_mut(&id) {
            instance.set_timeframe_visibility(config);
        }
    }

    /// Get instance by ID
    pub fn get_instance(&self, id: u64) -> Option<&IndicatorInstance> {
        self.instances.get(&id)
    }

    /// Get mutable instance by ID
    pub fn get_instance_mut(&mut self, id: u64) -> Option<&mut IndicatorInstance> {
        self.instances.get_mut(&id)
    }

    /// Get all instances for a symbol.
    /// When `current_render_window_id` is set, only returns instances that
    /// belong strictly to that window — instances with `window_id = None` are
    /// excluded so they do not bleed across tabs in split-window mode.
    /// When `current_render_window_id` is `None` (single-window / legacy mode)
    /// all instances for the symbol are returned for backwards compatibility.
    pub fn get_instances_for_symbol(&self, symbol: &str) -> Vec<&IndicatorInstance> {
        self.by_symbol
            .get(symbol)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.instances.get(id))
                    .filter(|inst| match self.current_render_window_id.get() {
                        // Strict match: only show instances that belong to this window.
                        Some(wid) => inst.window_id == Some(wid),
                        // No render window set — show all (single-window / legacy).
                        None => true,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get indicator instances for a symbol scoped to a specific window.
    /// Only returns instances whose `window_id` exactly matches `window_id`.
    /// Instances with `window_id = None` are NOT included to prevent cross-tab leakage.
    pub fn get_instances_for_symbol_in_window(&self, symbol: &str, window_id: u64) -> Vec<&IndicatorInstance> {
        self.by_symbol
            .get(symbol)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.instances.get(id))
                    .filter(|inst| inst.window_id == Some(window_id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all instances
    pub fn get_all_instances(&self) -> Vec<&IndicatorInstance> {
        self.instances.values().collect()
    }

    /// Get count of instances
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    /// Search definitions by name
    pub fn search_definitions(&self, query: &str) -> Vec<&IndicatorDefinition> {
        let query_lower = query.to_lowercase();
        self.definitions
            .values()
            .filter(|d| {
                d.name.to_lowercase().contains(&query_lower) ||
                d.short_name.to_lowercase().contains(&query_lower)
            })
            .collect()
    }

    /// Calculate indicator values for an instance using bar data
    ///
    /// Uses the full indicator catalog from zengeld-chart-indicators for computation.
    /// Also generates signals if signals_enabled is true.
    ///
    /// Accepts `zengeld_chart::Bar` (which has `timestamp`) and converts internally.
    pub fn calculate(&mut self, id: u64, bars: &[zengeld_chart::Bar]) {
        use super::indicator_calculator::IndicatorCalculator;
        // Convert zengeld_chart::Bar to indicators Bar
        let bars: Vec<crate::Bar> = bars.iter().map(|b| crate::Bar {
            time: b.timestamp,
            open: b.open,
            high: b.high,
            low: b.low,
            close: b.close,
            volume: b.volume,
        }).collect();
        let bars: &[crate::Bar] = &bars;

        // Get machine_id and signals_enabled from definition/instance
        let (machine_id, signals_enabled) = {
            let instance = match self.instances.get(&id) {
                Some(i) => i,
                None => return,
            };
            let definition = match self.definitions.get(&instance.type_id) {
                Some(d) => d,
                None => return,
            };
            (definition.machine_id, instance.signals_enabled)
        };

        let instance = match self.instances.get_mut(&id) {
            Some(i) => i,
            None => return,
        };

        // Use machine_id directly (no string parsing!)
        if let Some(indicator_id) = machine_id {
            if signals_enabled {
                // Calculate with signals
                if let Some(result) = IndicatorCalculator::calculate_with_signals(indicator_id, &instance.params, bars) {
                    for (output_name, output_values) in result.values {
                        instance.set_values(&output_name, output_values);
                    }
                    instance.signals = result.signals;
                }
            } else {
                // Calculate without signals (faster)
                if let Some(values) = IndicatorCalculator::calculate_with_id(indicator_id, &instance.params, bars) {
                    for (output_name, output_values) in values {
                        instance.set_values(&output_name, output_values);
                    }
                }
            }
        }
    }

    /// Calculate all instances for a symbol in parallel using rayon.
    ///
    /// Each indicator computation is independent (reads params, writes values),
    /// so all instances can be computed concurrently on the rayon thread pool.
    /// Results are written back sequentially after the parallel phase.
    pub fn calculate_all_for_symbol(&mut self, symbol: &str, bars: &[zengeld_chart::Bar]) {
        use rayon::prelude::*;
        use super::indicator_calculator::IndicatorCalculator;

        let ids: Vec<u64> = self
            .by_symbol
            .get(symbol)
            .cloned()
            .unwrap_or_default();

        if ids.is_empty() {
            return;
        }

        // Convert bars once, shared across all parallel tasks.
        let converted_bars: Vec<crate::Bar> = bars.iter().map(|b| crate::Bar {
            time: b.timestamp,
            open: b.open,
            high: b.high,
            low: b.low,
            close: b.close,
            volume: b.volume,
        }).collect();

        // Collect all inputs needed for parallel computation (cheap clones of params).
        // This phase is sequential and cheap — just field reads and small HashMap clones.
        let tasks: Vec<(u64, Option<crate::BarIndicatorId>, bool, HashMap<String, IndicatorValue>)> = ids
            .iter()
            .filter_map(|&id| {
                let inst = self.instances.get(&id)?;
                let def = self.definitions.get(&inst.type_id)?;
                Some((id, def.machine_id, inst.signals_enabled, inst.params.clone()))
            })
            .collect();

        // Parallel computation phase — no shared mutable state.
        // Each task produces an (id, values, signals) result.
        type TaskResult = (u64, Option<HashMap<String, Vec<f64>>>, Vec<crate::signals::SignalEvent>);
        let results: Vec<TaskResult> = tasks
            .into_par_iter()
            .map(|(id, machine_id, signals_enabled, params)| {
                let bars: &[crate::Bar] = &converted_bars;
                if let Some(indicator_id) = machine_id {
                    if signals_enabled {
                        if let Some(result) = IndicatorCalculator::calculate_with_signals(indicator_id, &params, bars) {
                            return (id, Some(result.values), result.signals);
                        }
                    } else if let Some(values) = IndicatorCalculator::calculate_with_id(indicator_id, &params, bars) {
                        return (id, Some(values), Vec::new());
                    }
                }
                (id, None, Vec::new())
            })
            .collect();

        // Write-back phase — sequential, updates self.instances with computed results.
        for (id, values_opt, signals) in results {
            if let Some(instance) = self.instances.get_mut(&id) {
                if let Some(values) = values_opt {
                    for (output_name, output_values) in values {
                        instance.set_values(&output_name, output_values);
                    }
                }
                if instance.signals_enabled {
                    instance.signals = signals;
                }
            }
        }
    }

    /// Calculate only the instances for a symbol that belong to a specific window,
    /// using rayon to compute multiple indicators in parallel.
    ///
    /// This is the correct method to use when multiple windows show the same symbol
    /// on different timeframes — each window provides its own bars slice, so only
    /// instances scoped to that window (via `window_id`) should be recalculated
    /// against those bars.
    ///
    /// Instances whose `window_id` is `None` (unscoped / legacy) are skipped;
    /// they should be handled by `calculate_all_for_symbol` when appropriate.
    pub fn calculate_for_window(&mut self, symbol: &str, window_id: u64, bars: &[zengeld_chart::Bar]) {
        use rayon::prelude::*;
        use super::indicator_calculator::IndicatorCalculator;

        let ids: Vec<u64> = self
            .by_symbol
            .get(symbol)
            .cloned()
            .unwrap_or_default();

        if ids.is_empty() {
            return;
        }

        // Convert bars once, shared across all parallel tasks.
        let converted_bars: Vec<crate::Bar> = bars.iter().map(|b| crate::Bar {
            time: b.timestamp,
            open: b.open,
            high: b.high,
            low: b.low,
            close: b.close,
            volume: b.volume,
        }).collect();

        // Collect inputs for instances belonging to this window only.
        let tasks: Vec<(u64, Option<crate::BarIndicatorId>, bool, HashMap<String, IndicatorValue>)> = ids
            .iter()
            .filter_map(|&id| {
                let inst = self.instances.get(&id)?;
                // Only include instances explicitly scoped to this window.
                if inst.window_id != Some(window_id) {
                    return None;
                }
                let def = self.definitions.get(&inst.type_id)?;
                Some((id, def.machine_id, inst.signals_enabled, inst.params.clone()))
            })
            .collect();

        if tasks.is_empty() {
            return;
        }

        // Parallel computation phase.
        type TaskResult = (u64, Option<HashMap<String, Vec<f64>>>, Vec<crate::signals::SignalEvent>);
        let results: Vec<TaskResult> = tasks
            .into_par_iter()
            .map(|(id, machine_id, signals_enabled, params)| {
                let bars: &[crate::Bar] = &converted_bars;
                if let Some(indicator_id) = machine_id {
                    if signals_enabled {
                        if let Some(result) = IndicatorCalculator::calculate_with_signals(indicator_id, &params, bars) {
                            return (id, Some(result.values), result.signals);
                        }
                    } else if let Some(values) = IndicatorCalculator::calculate_with_id(indicator_id, &params, bars) {
                        return (id, Some(values), Vec::new());
                    }
                }
                (id, None, Vec::new())
            })
            .collect();

        // Write-back phase.
        for (id, values_opt, signals) in results {
            if let Some(instance) = self.instances.get_mut(&id) {
                if let Some(values) = values_opt {
                    for (output_name, output_values) in values {
                        instance.set_values(&output_name, output_values);
                    }
                }
                if instance.signals_enabled {
                    instance.signals = signals;
                }
            }
        }
    }

    // ── Recalculation scheduling helpers ─────────────────────────────────────

    /// Mark a symbol as needing indicator recalculation.
    ///
    /// In PerTick mode this is a no-op (callers handle recalc inline).
    /// In PerFrame and PerBar modes, the symbol is queued for the next flush.
    pub fn mark_dirty(&mut self, symbol: &str) {
        self.dirty_symbols.insert(symbol.to_string());
    }

    /// Mark a symbol as having formed a new bar, and also mark it dirty.
    ///
    /// In PerBar mode, only symbols marked via this method are recalculated
    /// during the next flush.
    pub fn mark_new_bar(&mut self, symbol: &str) {
        self.new_bar_symbols.insert(symbol.to_string());
        self.dirty_symbols.insert(symbol.to_string());
    }

    /// Clear all pending dirty flags without triggering recalculation.
    ///
    /// Call this when the recalc mode changes so that stale queue entries from
    /// the previous mode do not leak into the first flush of the new mode.
    pub fn clear_pending(&mut self) {
        self.dirty_symbols.clear();
        self.new_bar_symbols.clear();
    }

    /// Drain pending recalculation requests and return the symbols to recalculate.
    ///
    /// Behaviour depends on the current `recalc_mode`:
    /// - `PerTick`: returns an empty list (callers handle recalc inline, not here).
    /// - `PerFrame`: returns all dirty symbols and clears the dirty set.
    /// - `PerBar`: returns only symbols that formed a new bar; also clears dirty set.
    ///
    /// After calling this, both internal sets are empty.
    pub fn drain_pending_recalc(&mut self) -> Vec<String> {
        let result = match self.recalc_mode {
            RecalcMode::PerTick => {
                // PerTick is handled inline — nothing to drain.
                Vec::new()
            }
            RecalcMode::PerFrame => {
                self.dirty_symbols.drain().collect()
            }
            RecalcMode::PerBar => {
                let new_bars: Vec<String> = self.new_bar_symbols.drain().collect();
                self.dirty_symbols.clear();
                new_bars
            }
        };
        // Always clear both sets so stale entries never accumulate.
        self.new_bar_symbols.clear();
        result
    }

    /// Calculate the Y-axis range (min, max) for an indicator instance.
    /// This centralizes the range calculation logic that was previously in native.
    ///
    /// Returns `(min, max)` tuple, with appropriate padding applied.
    /// For bounded indicators (0-100), returns fixed bounds.
    /// For unbounded indicators, calculates from visible data.
    pub fn calculate_pane_range(
        &self,
        id: u64,
        visible_start: usize,
        visible_end: usize,
    ) -> Option<(f64, f64)> {
        let instance = self.instances.get(&id)?;
        let definition = self.definitions.get(&instance.type_id)?;

        // Check if this indicator has fixed bounds (e.g., RSI/Stoch 0-100)
        if let Some(bounds) = definition.bounds {
            return Some(bounds);
        }

        // Calculate range from visible data
        let mut min_val = f64::INFINITY;
        let mut max_val = f64::NEG_INFINITY;

        // Iterate through all output values
        for output in &definition.outputs {
            if let Some(values) = instance.values.get(&output.name) {
                let start = visible_start.min(values.len().saturating_sub(1));
                let end = visible_end.min(values.len());

                for i in start..end {
                    let v = values[i];
                    if !v.is_nan() && !v.is_infinite() {
                        min_val = min_val.min(v);
                        max_val = max_val.max(v);
                    }
                }
            }
        }

        if min_val.is_infinite() || max_val.is_infinite() || min_val >= max_val {
            // No valid data, return default range
            return Some((-1.0, 1.0));
        }

        // Add padding (5%)
        let range = max_val - min_val;
        let padding = range * 0.05;

        // Check if indicator has zero baseline (extends Y-range to include zero)
        if definition.zero_baseline && min_val > 0.0 {
            // Extend down to zero for histogram-type indicators
            min_val = 0.0;
        }

        Some((min_val - padding, max_val + padding))
    }

    /// Get the preferred pane index for a new indicator instance.
    /// Overlay indicators go to pane 0 (main chart), others get their own pane.
    pub fn get_preferred_pane(&self, type_id: &str) -> usize {
        if let Some(def) = self.definitions.get(type_id) {
            if def.overlay { 0 } else { 1 }
        } else {
            1
        }
    }

    /// Check if indicator type is an overlay (renders on main chart)
    pub fn is_overlay(&self, type_id: &str) -> bool {
        self.definitions.get(type_id).map(|d| d.overlay).unwrap_or(false)
    }

    /// Check if indicator type uses a bounded range (has fixed bounds)
    pub fn is_bounded(&self, type_id: &str) -> bool {
        self.definitions.get(type_id).map(|d| d.bounds.is_some()).unwrap_or(false)
    }

    /// Hit test for overlay indicators (pane == 0).
    /// Returns the indicator instance ID if the point (screen_x, screen_y) is close to any indicator line.
    pub fn hit_test_overlay(
        &self,
        screen_x: f64,
        screen_y: f64,
        symbol: &str,
        viewport: &zengeld_chart::Viewport,
        price_scale: &zengeld_chart::PriceScale,
        chart_height: f64,
        hit_distance: f64,
    ) -> Option<u64> {
        let instances = self.get_instances_for_symbol(symbol);

        // Convert screen X to bar index
        let bar_idx = viewport.x_to_bar_f64(screen_x);
        let bar_int = bar_idx.round() as i64;

        // Convert screen Y to price
        let price_range = price_scale.price_max - price_scale.price_min;
        let _screen_price = if chart_height > 0.0 && price_range > 0.0 {
            price_scale.price_max - (screen_y / chart_height) * price_range
        } else {
            return None;
        };

        // Check each overlay indicator
        for instance in instances {
            // Only check overlay indicators (pane == 0) that are visible
            if instance.pane != 0 || !instance.visible {
                continue;
            }

            // Check each output line
            for (_output_name, values) in instance.values.iter() {
                if values.is_empty() {
                    continue;
                }

                // Get the value at the current bar position
                if bar_int < 0 || bar_int as usize >= values.len() {
                    continue;
                }

                let value = values[bar_int as usize];
                if value.is_nan() || value.is_infinite() {
                    continue;
                }

                // Convert indicator value to screen Y
                let indicator_screen_y = if price_range > 0.0 {
                    ((price_scale.price_max - value) / price_range) * chart_height
                } else {
                    continue;
                };

                // Check if cursor is close to this indicator line
                let y_distance = (screen_y - indicator_screen_y).abs();
                if y_distance <= hit_distance {
                    return Some(instance.id);
                }

                // Also check interpolated value between bars for smoother hit detection
                // Check previous bar if available
                if bar_int > 0 {
                    let prev_idx = (bar_int - 1) as usize;
                    if prev_idx < values.len() {
                        let prev_value = values[prev_idx];
                        if !prev_value.is_nan() && !prev_value.is_infinite() {
                            // Linear interpolation
                            let frac = bar_idx - bar_idx.floor();
                            let interp_value = prev_value + (value - prev_value) * frac;
                            let interp_screen_y = ((price_scale.price_max - interp_value) / price_range) * chart_height;
                            let interp_distance = (screen_y - interp_screen_y).abs();
                            if interp_distance <= hit_distance {
                                return Some(instance.id);
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Hit test for sub-pane indicators (pane > 0).
    /// Returns the indicator instance ID if the point is within a sub-pane.
    pub fn hit_test_sub_pane(
        &self,
        instance_id: u64,
        screen_x: f64,
        screen_y: f64,
        viewport: &zengeld_chart::Viewport,
        price_min: f64,
        price_max: f64,
        pane_height: f64,
        hit_distance: f64,
    ) -> bool {
        let instance = match self.instances.get(&instance_id) {
            Some(i) => i,
            None => return false,
        };

        if !instance.visible {
            return false;
        }

        // Convert screen X to bar index
        let bar_idx = viewport.x_to_bar_f64(screen_x);
        let bar_int = bar_idx.round() as i64;

        // Convert screen Y to indicator value
        let price_range = price_max - price_min;
        let _screen_value = if pane_height > 0.0 && price_range > 0.0 {
            price_max - (screen_y / pane_height) * price_range
        } else {
            return false;
        };

        // Check each output line
        for (_, values) in instance.values.iter() {
            if values.is_empty() {
                continue;
            }

            if bar_int < 0 || bar_int as usize >= values.len() {
                continue;
            }

            let value = values[bar_int as usize];
            if value.is_nan() || value.is_infinite() {
                continue;
            }

            // Convert indicator value to screen Y for comparison
            let indicator_screen_y = if price_range > 0.0 {
                ((price_max - value) / price_range) * pane_height
            } else {
                continue;
            };

            let y_distance = (screen_y - indicator_screen_y).abs();
            if y_distance <= hit_distance {
                return true;
            }
        }

        false
    }

    /// Get display name for an indicator instance (e.g., "RSI (14)")
    pub fn get_instance_display_name(&self, instance: &IndicatorInstance) -> String {
        // Get the definition to find the main param
        if let Some(def) = self.definitions.get(&instance.type_id) {
            // Try to find a "period" or "length" param to include in name
            let main_param = instance.params.get("period")
                .or_else(|| instance.params.get("length"))
                .or_else(|| instance.params.get("Period"))
                .or_else(|| instance.params.get("Length"));

            if let Some(param_val) = main_param {
                let param_str = match param_val {
                    IndicatorValue::Int(v) => v.to_string(),
                    IndicatorValue::Float(v) => format!("{:.0}", v),
                    _ => String::new(),
                };
                if !param_str.is_empty() {
                    return format!("{} ({})", def.short_name, param_str);
                }
            }

            def.short_name.clone()
        } else {
            instance.name.clone()
        }
    }

    /// Expose instances map for use by core-side extensions
    pub fn instances_iter(&self) -> impl Iterator<Item = &IndicatorInstance> {
        self.instances.values()
    }
}

impl Default for IndicatorManager {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// IndicatorSource implementation for IndicatorManager
// =============================================================================

impl zengeld_chart::indicator_source::IndicatorSource for IndicatorManager {
    fn get_instances_for_symbol(&self, symbol: &str) -> Vec<zengeld_chart::indicator_source::IndicatorInfo> {
        self.get_instances_for_symbol(symbol)
            .into_iter()
            .map(|inst| zengeld_chart::indicator_source::IndicatorInfo {
                id: inst.id,
                name: inst.name.clone(),
                pane_index: inst.pane,
                visible: inst.visible,
            })
            .collect()
    }

    fn calculate_pane_range(
        &self,
        instance_id: u64,
        visible_start: usize,
        visible_end: usize,
    ) -> Option<(f64, f64)> {
        self.calculate_pane_range(instance_id, visible_start, visible_end)
    }

    fn get_render_instances_for_symbol(
        &self,
        symbol: &str,
    ) -> Vec<zengeld_chart::indicator_source::IndicatorRenderInstance> {
        self.get_instances_for_symbol(symbol)
            .into_iter()
            .filter_map(|inst| self.build_render_instance(inst))
            .collect()
    }

    fn get_render_instance(
        &self,
        instance_id: u64,
    ) -> Option<zengeld_chart::indicator_source::IndicatorRenderInstance> {
        let inst = self.get_instance(instance_id)?;
        self.build_render_instance(inst)
    }

    fn get_settings_data(
        &self,
        instance_id: u64,
    ) -> Option<zengeld_chart::indicator_source::IndicatorSettingsData> {
        use zengeld_chart::indicator_source::IndicatorSettingsData;
        use zengeld_chart::ui::modal_settings::{
            IndicatorDisplayInfo, IndicatorParamDef,
            IndicatorParamType as ChartParamType,
            IndicatorOutputDef, IndicatorOutputType as ChartOutputType,
        };

        let inst = self.get_instance(instance_id)?;
        let definition = self.get_definition(&inst.type_id);

        // Convert params to (name, value) pairs using definition order for stable UI ordering
        let params: Vec<(String, String)> = if let Some(def) = definition.as_ref() {
            def.params.iter()
                .filter_map(|param_def| {
                    let value = inst.params.get(&param_def.name)
                        .unwrap_or(&param_def.default_value);
                    let value_str = match value {
                        IndicatorValue::Int(i) => i.to_string(),
                        IndicatorValue::Float(f) => format!("{:.2}", f),
                        IndicatorValue::Bool(b) => b.to_string(),
                        IndicatorValue::String(s) => s.clone(),
                        IndicatorValue::Color(c) => c.clone(),
                    };
                    Some((param_def.name.clone(), value_str))
                })
                .collect()
        } else {
            // Fallback: no definition, use HashMap order (unstable but better than nothing)
            inst.params.iter()
                .map(|(k, v)| {
                    let value_str = match v {
                        IndicatorValue::Int(i) => i.to_string(),
                        IndicatorValue::Float(f) => format!("{:.2}", f),
                        IndicatorValue::Bool(b) => b.to_string(),
                        IndicatorValue::String(s) => s.clone(),
                        IndicatorValue::Color(c) => c.clone(),
                    };
                    (k.clone(), value_str)
                })
                .collect()
        };

        // Convert outputs to (name, color) pairs using definition order for stable ordering
        let outputs: Vec<(String, String)> = if let Some(def) = definition.as_ref() {
            def.outputs.iter()
                .map(|output_def| {
                    let color = inst.outputs.get(&output_def.name)
                        .and_then(|cfg| cfg.color.clone())
                        .unwrap_or_else(|| output_def.color.clone());
                    (output_def.name.clone(), color)
                })
                .collect()
        } else {
            inst.outputs.iter()
                .map(|(k, v)| {
                    let color = v.color.clone().unwrap_or_else(|| "#2962ff".to_string());
                    (k.clone(), color)
                })
                .collect()
        };

        // Convert IndicatorDefinition → IndicatorDisplayInfo (chart-owned type)
        let display_info: Option<IndicatorDisplayInfo> = definition.map(|def| {
            IndicatorDisplayInfo {
                name: def.name.clone(),
                short_name: def.short_name.clone(),
                description: def.description.clone(),
                overlay: def.overlay,
                bounds: def.bounds,
                category_name: def.category.display_name().to_string(),
                params: def.params.iter().map(|p| IndicatorParamDef {
                    name: p.name.clone(),
                    param_type: match &p.param_type {
                        IndicatorParamType::Int { .. } => ChartParamType::Int,
                        IndicatorParamType::Float { .. } => ChartParamType::Float,
                        IndicatorParamType::Bool => ChartParamType::Bool,
                        IndicatorParamType::Source => ChartParamType::Source,
                        IndicatorParamType::Select { options } => ChartParamType::Select { options: options.clone() },
                        IndicatorParamType::Color => ChartParamType::Color,
                    },
                }).collect(),
                outputs: def.outputs.iter().map(|o| IndicatorOutputDef {
                    display_name: o.display_name.clone(),
                    output_type: match o.output_type {
                        IndicatorOutputType::Line => ChartOutputType::Line,
                        IndicatorOutputType::Histogram => ChartOutputType::Histogram,
                        IndicatorOutputType::Area => ChartOutputType::Other("Area".to_string()),
                        IndicatorOutputType::Band => ChartOutputType::Other("Band".to_string()),
                        IndicatorOutputType::Dots => ChartOutputType::Other("Dots".to_string()),
                        IndicatorOutputType::Background => ChartOutputType::Other("Background".to_string()),
                    },
                }).collect(),
            }
        });

        Some(IndicatorSettingsData {
            name: inst.name.clone(),
            params,
            outputs,
            display_info,
            signals_enabled: inst.signals_enabled,
            timeframe_visibility: inst.timeframe_visibility.clone(),
        })
    }
}

impl IndicatorManager {
    /// Convert an `IndicatorInstance` + its `IndicatorDefinition` into a chart-side
    /// `IndicatorRenderInstance`. Returns `None` if the definition is missing.
    pub fn build_render_instance(
        &self,
        inst: &IndicatorInstance,
    ) -> Option<zengeld_chart::indicator_source::IndicatorRenderInstance> {
        use zengeld_chart::indicator_source::{
            IndicatorRenderInstance, IndicatorOutputRenderDef, IndicatorOutputRenderType,
            OutputRenderConfig, HistogramStyle as ChartHistogramStyle, SignalRenderData,
        };

        let def = self.get_definition(&inst.type_id)?;

        let output_defs: Vec<IndicatorOutputRenderDef> = def.outputs.iter().map(|o| {
            let render_type = match o.output_type {
                IndicatorOutputType::Line       => IndicatorOutputRenderType::Line,
                IndicatorOutputType::Histogram  => IndicatorOutputRenderType::Histogram,
                IndicatorOutputType::Band       => IndicatorOutputRenderType::Band,
                IndicatorOutputType::Area       => IndicatorOutputRenderType::Area,
                IndicatorOutputType::Dots       => IndicatorOutputRenderType::Dots,
                IndicatorOutputType::Background => IndicatorOutputRenderType::Background,
            };
            IndicatorOutputRenderDef {
                name: o.name.clone(),
                output_type: render_type,
                color: o.color.clone(),
                line_width: o.line_width,
            }
        }).collect();

        let output_configs: std::collections::HashMap<String, OutputRenderConfig> = inst.outputs
            .iter()
            .map(|(name, cfg)| {
                (name.clone(), OutputRenderConfig {
                    visible: cfg.visible,
                    color: cfg.color.clone(),
                    line_width: cfg.line_width,
                })
            })
            .collect();

        let histogram_style = match def.histogram_style {
            HistogramStyle::FromBottom => ChartHistogramStyle::FromBottom,
            HistogramStyle::Centered   => ChartHistogramStyle::Centered,
        };

        let signals: Vec<SignalRenderData> = inst.signals.iter().map(|s| SignalRenderData {
            bar_index: s.bar_index,
            direction: s.kind.direction() as i32,
            price: s.price,
        }).collect();

        // Collect color_params and bool_params from indicator params
        let mut color_params = std::collections::HashMap::new();
        let mut bool_params = std::collections::HashMap::new();

        for (key, value) in &inst.params {
            match value {
                IndicatorValue::Color(c) => { color_params.insert(key.clone(), c.clone()); }
                IndicatorValue::Bool(b)  => { bool_params.insert(key.clone(), *b); }
                _ => {}
            }
        }

        // Special case: volume indicator color params
        if inst.type_id == "volume" || inst.type_id == "vol" {
            color_params.entry("up_color".to_string())
                .or_insert_with(|| "#26A69A80".to_string());
            color_params.entry("down_color".to_string())
                .or_insert_with(|| "#EF535080".to_string());
            bool_params.entry("color_by_direction".to_string())
                .or_insert(true);
        }

        Some(IndicatorRenderInstance {
            id: inst.id,
            type_id: inst.type_id.clone(),
            pane: inst.pane,
            visible: inst.visible,
            title: inst.title(),
            output_defs,
            output_configs,
            // Arc::clone bumps the reference count (O(1)) instead of deep-copying
            // potentially millions of f64 values on every render frame.
            values: Arc::clone(&inst.values),
            histogram_style,
            signals,
            signals_enabled: inst.signals_enabled,
            color_params,
            bool_params,
            timeframe_visibility: inst.timeframe_visibility.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_instance() {
        let mut manager = IndicatorManager::new();

        let id = manager.create_instance("sma", "BTCUSD").unwrap();
        let instance = manager.get_instance(id).unwrap();

        assert_eq!(instance.type_id, "sma");
        assert_eq!(instance.symbol, "BTCUSD");
        assert!(instance.visible);
    }

    #[test]
    fn test_modify_params() {
        let mut manager = IndicatorManager::new();

        let id = manager.create_instance("rsi", "AAPL").unwrap();
        let instance = manager.get_instance_mut(id).unwrap();

        instance.set_param("period", IndicatorValue::Int(21));

        let period = instance.get_param("period").unwrap().as_int().unwrap();
        assert_eq!(period, 21);
    }

    #[test]
    fn test_full_catalog() {
        let manager = IndicatorManager::new();

        // Should have 400+ indicators from catalog
        let all_defs = manager.get_definitions();
        assert!(all_defs.len() > 400, "Expected 400+ indicators, got {}", all_defs.len());

        // Check some specific indicators exist
        assert!(manager.get_definition("sma").is_some());
        assert!(manager.get_definition("ema").is_some());
        assert!(manager.get_definition("rsi").is_some());
        assert!(manager.get_definition("macd").is_some());
        assert!(manager.get_definition("bb").is_some());
        assert!(manager.get_definition("atr").is_some());

        // Check overlay vs sub-pane
        let sma = manager.get_definition("sma").unwrap();
        assert!(sma.overlay, "SMA should be overlay");

        let rsi = manager.get_definition("rsi").unwrap();
        assert!(!rsi.overlay, "RSI should not be overlay");
        assert_eq!(rsi.bounds, Some((0.0, 100.0)), "RSI should have 0-100 bounds");
    }

    #[test]
    fn test_category_filter() {
        let manager = IndicatorManager::new();

        let trend = manager.get_by_category(IndicatorCategory::Trend);
        assert!(trend.iter().any(|d| d.type_id == "sma"));
        assert!(trend.iter().any(|d| d.type_id == "ema"));

        let momentum = manager.get_by_category(IndicatorCategory::Momentum);
        assert!(momentum.iter().any(|d| d.type_id == "rsi"));
        assert!(momentum.iter().any(|d| d.type_id == "macd"));
    }

    #[test]
    fn test_instance_title() {
        let mut manager = IndicatorManager::new();

        let id = manager.create_instance("sma", "TEST").unwrap();
        let instance = manager.get_instance(id).unwrap();

        // Title format depends on params from catalog
        assert!(instance.title().contains("SMA"));
    }

    #[test]
    fn test_calculate_rsi_values() {
        let mut manager = IndicatorManager::new();

        // Create RSI instance
        let id = manager.create_instance("rsi", "TEST").unwrap();

        // Create test bars (using zengeld_chart::Bar which has `timestamp`)
        let bars: Vec<zengeld_chart::Bar> = (0..100).map(|i| zengeld_chart::Bar {
            timestamp: i as i64,
            open: 100.0 + (i as f64 * 0.1).sin() * 5.0,
            high: 101.0 + (i as f64 * 0.1).sin() * 5.0,
            low: 99.0 + (i as f64 * 0.1).sin() * 5.0,
            close: 100.0 + (i as f64 * 0.1).sin() * 5.0,
            volume: 1000.0,
        }).collect();

        // Calculate
        manager.calculate(id, &bars);

        // Check values were populated
        let instance = manager.get_instance(id).unwrap();
        let definition = manager.get_definition("rsi").unwrap();

        // RSI should have "rsi" output from rendering catalog
        assert!(!instance.values.is_empty(), "RSI values should not be empty");

        // Check the output name matches between definition and values
        for output in &definition.outputs {
            assert!(instance.values.contains_key(&output.name),
                    "Instance should have values for output '{}', but has keys: {:?}",
                    output.name, instance.values.keys().collect::<Vec<_>>());
        }
    }
}
