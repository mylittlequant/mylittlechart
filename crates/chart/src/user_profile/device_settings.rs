//! Device-level settings persisted at `%APPDATA%\zengeld\device_settings.json`.
//!
//! Unlike `UserProfile`, these settings are **not tied to any profile** — they
//! apply to the device as a whole and persist across profile switches.

use std::path::PathBuf;

use super::storage::app_data_dir;

/// Available render backends for the chart application.
///
/// Stored in `device_settings.json` so the user's choice survives restarts.
/// `None` (the default) means auto-detect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderBackend {
    VelloGpu,
    InstancedWgpu,
    VelloCpu,
    VelloHybrid,
    TinySkia,
}

fn default_fps_limit() -> u32 {
    120
}

fn default_msaa_samples() -> u8 {
    8
}

fn default_max_bars() -> usize {
    0
}

fn default_recalc_mode() -> String {
    "per_frame".to_string()
}

/// Device-level settings that persist across profile switches.
///
/// Stored at `{app_data_dir()}/device_settings.json`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DeviceSettings {
    /// Preferred render backend.
    ///
    /// `None` (default) means auto-detect: the app picks the best available
    /// backend at startup.  Set to a specific variant to force a backend.
    #[serde(default)]
    pub render_backend: Option<RenderBackend>,

    /// Frame-rate cap in frames per second.  `0` means unlimited.
    #[serde(default = "default_fps_limit")]
    pub fps_limit: u32,

    /// MSAA sample count.  `0` = off (Area AA), `8` = 8x, `16` = 16x.
    #[serde(default = "default_msaa_samples")]
    pub msaa_samples: u8,

    /// Maximum bars to keep per window.  `0` = unlimited.
    #[serde(default = "default_max_bars")]
    pub max_bars: usize,

    /// Indicator recalculation mode: `"per_frame"`, `"PerBar"`, or `"PerTick"`.
    #[serde(default = "default_recalc_mode")]
    pub recalc_mode: String,
}

impl Default for DeviceSettings {
    fn default() -> Self {
        Self {
            render_backend: None,
            fps_limit: default_fps_limit(),
            msaa_samples: default_msaa_samples(),
            max_bars: default_max_bars(),
            recalc_mode: default_recalc_mode(),
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
