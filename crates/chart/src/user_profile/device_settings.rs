//! Device-level settings persisted at `%APPDATA%\zengeld\device_settings.json`.
//!
//! Unlike `UserProfile`, these settings are **not tied to any profile** — they
//! apply to the device as a whole and persist across profile switches.
//!
//! # Fields
//! - `ota_enabled` — Connected (`true`, default) vs Standalone (`false`) mode.
//!   When `false` the updater makes no network calls (OTA + telemetry off).
//! - `update_channel` — OTA channel: `"stable"` (default) or `"dev"`.
//!   Only meaningful when `ota_enabled` is `true`.

use std::path::PathBuf;

use super::storage::app_data_dir;

fn default_true() -> bool {
    true
}

fn default_stable() -> String {
    "stable".to_string()
}

/// Device-level settings that persist across profile switches.
///
/// Stored at `{app_data_dir()}/device_settings.json`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DeviceSettings {
    /// Whether OTA updates and telemetry are active.
    ///
    /// `true` = Connected mode (default): updater polls for updates and sends
    /// anonymous metrics.  `false` = Standalone mode: no network activity.
    #[serde(default = "default_true")]
    pub ota_enabled: bool,

    /// OTA update channel: `"stable"` or `"dev"`.
    ///
    /// Controls the polling interval and which release track the updater
    /// checks.  `"stable"` uses a 4-hour interval; `"dev"` uses 2 minutes.
    /// Only meaningful when `ota_enabled` is `true`.
    #[serde(default = "default_stable")]
    pub update_channel: String,
}

impl Default for DeviceSettings {
    fn default() -> Self {
        Self {
            ota_enabled: true,
            update_channel: "stable".to_string(),
        }
    }
}

impl DeviceSettings {
    /// Load `DeviceSettings` from disk, returning `Default` on any error.
    pub fn load() -> Self {
        let path = Self::file_path();
        match std::fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Persist `DeviceSettings` to disk.  Errors are silently ignored.
    pub fn save(&self) {
        let path = Self::file_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, json);
        }
    }

    /// Absolute path to `device_settings.json`.
    pub fn file_path() -> PathBuf {
        app_data_dir().join("device_settings.json")
    }
}
