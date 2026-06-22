//! Device-level settings persisted at `{app_data_dir}/device_settings.json`.
//!
//! Unlike `UserProfile`, these settings are **not tied to any profile** — they
//! apply to the device as a whole and persist across profile switches.

use std::path::PathBuf;

use super::storage::app_data_dir;
use mlc_crypto::global_storage;

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
    /// Load `DeviceSettings` from storage, returning `Default` on any error.
    pub fn load() -> Self {
        let key = Self::storage_key();
        global_storage()
            .read(&key)
            .ok()
            .flatten()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    /// Persist `DeviceSettings` to storage.  Errors are silently ignored.
    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = global_storage().write(&Self::storage_key(), json.as_bytes());
        }
    }

    /// Absolute path to `device_settings.json` (for callers that need the path).
    pub fn file_path() -> PathBuf {
        app_data_dir().join("device_settings.json")
    }

    fn storage_key() -> String {
        // Key is relative to app_data_dir — just the filename.
        "device_settings.json".to_string()
    }
}
