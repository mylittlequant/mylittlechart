//! [`UserManager`] — unified runtime wrapper for all user persistence.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::preset::preset::ChartPreset;
use crate::preset::storage::{list_presets, load_preset};
use crate::templates::TemplateManager;
use crate::user_profile::storage::{get_user_data_dir, load_json, load_profile, save_profile};
use crate::user_profile::UserProfile;

// =============================================================================
// SettingsSnapshots
// =============================================================================

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
}

// =============================================================================
// UserManager
// =============================================================================

/// Unified user state manager.
///
/// Owns all persistent user data loaded from disk at startup: profile metadata,
/// templates, presets, and runtime settings snapshots.
///
/// # Lifecycle
///
/// ```ignore
/// // At startup — load once, then move data into AppState / per-window ChartApp:
/// let user_manager = UserManager::load();
/// ```
///
/// Saving is handled by `App::save_all()` in `main.rs`, which coordinates all
/// windows before writing.  `UserManager` only exposes `save_profile()` for the
/// device-identity write that must happen immediately on first launch.
pub struct UserManager {
    /// User profile — active selections, sidebar state, theme, device identity.
    pub profile: UserProfile,

    /// All template types (primitives, indicators, compare, chart, indicator sets).
    /// Transferred into `AppState` and per-window `ChartApp` at startup.
    pub template_manager: TemplateManager,

    /// Chart presets in memory. Keys are preset IDs.
    /// Transferred into `AppState` and per-window `ChartApp` at startup.
    pub presets: HashMap<String, ChartPreset>,

    /// Runtime settings snapshots — user's last-used settings per category.
    /// Transferred into `AppState` at startup; per-window copies are synced
    /// each frame from `AppState`.
    pub snapshots: SettingsSnapshots,
}

impl Clone for UserManager {
    fn clone(&self) -> Self {
        Self {
            profile: self.profile.clone(),
            template_manager: self.template_manager.clone(),
            presets: self.presets.clone(),
            snapshots: self.snapshots.clone(),
        }
    }
}

impl UserManager {
    /// Create a new empty `UserManager` (no disk I/O).
    pub fn new() -> Self {
        Self {
            profile: UserProfile::new(),
            template_manager: TemplateManager::new(),
            presets: HashMap::new(),
            snapshots: SettingsSnapshots::default(),
        }
    }

    /// Load all user state from disk. Call at application startup.
    ///
    /// This loads: `profile.json`, all templates, all presets, settings
    /// snapshots. Errors are logged but not fatal — missing data results in
    /// defaults.
    pub fn load() -> Self {
        let data_dir = crate::user_profile::storage::app_data_dir();
        eprintln!("[UserManager] data directory: {}", data_dir.display());

        let profile = match load_profile() {
            Ok(p) => {
                eprintln!(
                    "[UserManager] loaded profile (active_preset={})",
                    p.active_preset_id
                );
                p
            }
            Err(e) => {
                eprintln!(
                    "[UserManager] failed to load profile: {}, using defaults",
                    e
                );
                UserProfile::new()
            }
        };

        let template_manager = {
            let tm = TemplateManager::load_from_default_dir();
            eprintln!(
                "[UserManager] loaded templates: {} prim, {} ind, {} cmp, {} chart, {} sets",
                tm.primitive_templates.len(),
                tm.indicator_templates.len(),
                tm.compare_templates.len(),
                tm.chart_templates.len(),
                tm.indicator_sets.len(),
            );
            tm
        };

        // Load presets from disk.
        let mut presets = HashMap::new();
        match list_presets() {
            Ok(metas) => {
                for meta in &metas {
                    match load_preset(&meta.id) {
                        Ok(preset) => {
                            presets.insert(meta.id.clone(), preset);
                        }
                        Err(e) => {
                            eprintln!(
                                "[UserManager] failed to load preset {}: {}",
                                meta.id, e
                            );
                        }
                    }
                }
                eprintln!("[UserManager] loaded {} presets", presets.len());
            }
            Err(e) => {
                eprintln!("[UserManager] failed to list presets: {}", e);
            }
        }

        // Load settings snapshots.
        let snapshots_path = get_user_data_dir().join("settings_snapshots.json");
        let snapshots = match load_json::<SettingsSnapshots>(&snapshots_path) {
            Ok(s) => {
                eprintln!("[UserManager] loaded settings snapshots");
                s
            }
            Err(_) => {
                eprintln!("[UserManager] no settings snapshots found, using defaults");
                SettingsSnapshots::default()
            }
        };

        Self {
            profile,
            template_manager,
            presets,
            snapshots,
        }
    }

    // =========================================================================
    // Save methods
    // =========================================================================

    /// Save just the user profile to disk.
    ///
    /// Called at startup immediately after `load()` to persist a newly-generated
    /// device identity (`device_id`).  All other saves are handled by
    /// `App::save_all()` in `main.rs`, which has full multi-window context.
    pub fn save_profile(&self) {
        if let Err(e) = save_profile(&self.profile) {
            eprintln!("[UserManager] failed to save profile: {}", e);
        }
    }
}

impl Default for UserManager {
    fn default() -> Self {
        Self::new()
    }
}
