//! [`SettingsSnapshots`] — runtime snapshots of the user's current settings.
//!
//! Moved here from `mlc-app::user_manager::manager` (step 3.6c) — value types
//! that persist to disk belong in the type dictionary, not the app-level
//! wrapper. Persisted wholesale to `settings_snapshots.json`
//! (`mlc_core::user_profile::storage::{save_json, load_json}`), always
//! plaintext (never encrypted — see doc-comment on each load site).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Runtime snapshots of user's current settings.
///
/// These capture the user's active configuration so it persists across sessions.
/// Distinct from developer defaults (factory reset) and from named templates.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SettingsSnapshots {
    /// Last known chart settings (instrument, scales, status line).
    /// Stored as `serde_json::Value` for forward-compatibility.
    #[serde(default)]
    pub chart_settings: Option<serde_json::Value>,

    /// Last known primitive settings, keyed by primitive `type_id`.
    /// e.g. `"trend_line"` → `{ color: "#ff0000", width: 2 }`
    #[serde(default)]
    pub primitive_settings: HashMap<String, serde_json::Value>,

    /// Last known indicator settings, keyed by indicator `type_id`.
    /// e.g. `"sma"` → `{ period: 20, color: "#2196F3" }`
    #[serde(default)]
    pub indicator_settings: HashMap<String, serde_json::Value>,

    /// Last known compare overlay settings.
    #[serde(default)]
    pub compare_settings: Option<serde_json::Value>,

    /// Last-used drawing style per primitive `type_id`.
    ///
    /// Persisted so that the next primitive of the same type is pre-populated
    /// with the user's last-used color, width, line style, and extended
    /// style properties.  Applies globally across all chart windows.
    ///
    /// Stored as `serde_json::Value` for forward-compatibility so that
    /// new `TemplateStyle` fields never break deserialization of older files.
    #[serde(default)]
    pub last_used_drawing_styles: HashMap<String, serde_json::Value>,
}
