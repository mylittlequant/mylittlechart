//! Template for indicator parameters and output style configuration.
//!
//! An [`IndicatorTemplate`] saves the full configuration of an indicator
//! instance — calculation parameters and per-output visual style — so the
//! same setup can be applied to new indicator instances without manual
//! re-entry.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::preset::preset::unix_timestamp_parts;

// =============================================================================
// OutputStyleConfig
// =============================================================================

/// Visual style overrides for a single indicator output (plot/histogram/signal).
///
/// All fields are optional — `None` means "use the indicator's own default".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputStyleConfig {
    /// Hex color string (e.g. `"#2196F3"`).
    pub color: Option<String>,
    /// Line width in pixels.
    pub line_width: Option<f32>,
    /// Whether this output is visible.
    pub visible: Option<bool>,
}

impl OutputStyleConfig {
    /// Create an empty style config (all `None`).
    pub fn empty() -> Self {
        Self {
            color: None,
            line_width: None,
            visible: None,
        }
    }

    /// Create a style config with a color override only.
    pub fn with_color(color: impl Into<String>) -> Self {
        Self {
            color: Some(color.into()),
            line_width: None,
            visible: None,
        }
    }

    /// Returns `true` if all fields are `None` (no overrides).
    pub fn is_empty(&self) -> bool {
        self.color.is_none() && self.line_width.is_none() && self.visible.is_none()
    }
}

// =============================================================================
// IndicatorTemplate
// =============================================================================

/// Saved configuration for an indicator instance.
///
/// Captures:
/// - The indicator type identifier (matches the indicator registry key).
/// - Calculation parameters (period, source, thresholds, etc.) as a flexible
///   JSON value map.
/// - Per-output visual style overrides keyed by output name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorTemplate {
    /// Unique identifier generated at creation time.
    /// Format: `"itmpl_{unix_secs}_{nanos_suffix}"`.
    pub id: String,
    /// User-visible display name for this template.
    pub name: String,
    /// Indicator type identifier (e.g. `"sma"`, `"rsi"`, `"macd"`).
    pub type_id: String,
    /// Instance-level visibility (whether the indicator is shown at all).
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Calculation parameters keyed by parameter name.
    ///
    /// Values are stored as [`serde_json::Value`] to support heterogeneous
    /// parameter types (integers, floats, strings, booleans) without
    /// a rigid schema.
    #[serde(default)]
    pub params: HashMap<String, serde_json::Value>,
    /// Visual style overrides per output, keyed by output name.
    ///
    /// Example keys: `"line"`, `"signal"`, `"histogram"`.
    #[serde(default)]
    pub outputs: HashMap<String, OutputStyleConfig>,
}

fn default_true() -> bool { true }

impl IndicatorTemplate {
    /// Create a template with sensible defaults for the given indicator `type_id`.
    pub fn new_with_defaults(type_id: &str, name: &str) -> Self {
        let (secs, nanos) = unix_timestamp_parts();
        Self {
            id: format!("itmpl_{}_{}", secs, nanos),
            name: name.to_string(),
            type_id: type_id.to_string(),
            visible: true,
            params: HashMap::new(),
            outputs: HashMap::new(),
        }
    }

    /// Create a template from explicit params and output styles.
    pub fn new(
        type_id: &str,
        name: &str,
        visible: bool,
        params: HashMap<String, serde_json::Value>,
        outputs: HashMap<String, OutputStyleConfig>,
    ) -> Self {
        let (secs, nanos) = unix_timestamp_parts();
        Self {
            id: format!("itmpl_{}_{}", secs, nanos),
            name: name.to_string(),
            type_id: type_id.to_string(),
            visible,
            params,
            outputs,
        }
    }

    /// Set a single parameter value.
    pub fn set_param(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.params.insert(key.into(), value);
    }

    /// Set a single output style config.
    pub fn set_output_style(&mut self, output_name: impl Into<String>, style: OutputStyleConfig) {
        self.outputs.insert(output_name.into(), style);
    }

    /// Returns `true` if this template has any parameter overrides.
    pub fn has_params(&self) -> bool {
        !self.params.is_empty()
    }

    /// Returns `true` if this template has any output style overrides.
    pub fn has_output_styles(&self) -> bool {
        !self.outputs.is_empty()
    }
}
