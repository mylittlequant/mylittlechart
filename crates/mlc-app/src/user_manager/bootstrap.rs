//! One-shot disk-load bootstrap for the headless / test-harness entry point.
//!
//! Replaces the former `UserManager` struct (step 3.6c) — after the
//! preset/template/profile duplication kill, the only surviving caller was
//! `ChartPanelApp::load_user_state()`, and it only ever called
//! `UserManager::load()` once. A struct with one live constructor and no
//! other callers is not a manager, it is a load function wearing a
//! constructor — dissolved into [`load_user_state_from_disk`].
//!
//! `mlc-app-shell-vello` does NOT use this path — its bootstrap goes through
//! `user_manager::ProfileManager::load`, which remains a real struct (owns
//! the multi-profile index, vault key derivation, and profile-switch flow;
//! see `docs/mlc/plans/doctrine-cutover-2026-06-27.md` step 3.6c report for
//! the full classification).

use std::collections::HashMap;

use mlc_core::preset::preset::ChartPreset;
use mlc_core::preset::storage::{list_presets, load_preset};
use mlc_backend::template::TemplateManager;
use mlc_core::user_profile::storage::{active_profile_data_dir, load_json, load_profile};
use mlc_core::user_profile::{SettingsSnapshots, UserProfile};
use mlc_vault::vault::VaultKey;

/// Load all user state from disk: `profile.json`, all templates, all
/// presets, settings snapshots. Errors are logged but not fatal — missing
/// data results in defaults.
///
/// Pass `Some(key)` for encrypted installs, `None` for plaintext /
/// migration / first-run. Tries `.enc` files first and falls back to
/// `.json` automatically.
pub fn load_user_state_from_disk(
    key: Option<VaultKey>,
) -> (
    HashMap<String, ChartPreset>,
    TemplateManager,
    UserProfile,
    SettingsSnapshots,
    Option<VaultKey>,
) {
    let data_dir = active_profile_data_dir();
    log::info!("[user_manager] profile data directory: {}", data_dir.display());

    let key_ref = key.as_ref();

    let profile = match load_profile(key_ref) {
        Ok(p) => {
            log::info!(
                "[user_manager] loaded profile (active_preset={})",
                p.ui.active_preset_id
            );
            p
        }
        Err(e) => {
            if key_ref.is_some() {
                log::error!(
                    "[user_manager] WARNING: decryption failed with provided key, falling back to defaults \
                    — this should not happen after passphrase validation: {}",
                    e
                );
            } else {
                log::warn!("[user_manager] failed to load profile: {}, using defaults", e);
            }
            let mut fallback = UserProfile::new();
            if let Some(dir_name) = data_dir.file_name().and_then(|n| n.to_str()) {
                if !dir_name.is_empty() {
                    fallback.domain.profile_id = dir_name.to_string();
                    fallback.ui.display_name = dir_name.to_string();
                }
            }
            fallback
        }
    };

    // Templates — pass key so encrypted files are loaded correctly.
    let template_manager = {
        let tm = TemplateManager::load_from_default_dir(key_ref);
        log::info!(
            "[user_manager] loaded templates: {} prim, {} ind, {} cmp, {} chart, {} sets",
            tm.primitive_templates.len(),
            tm.indicator_templates.len(),
            tm.compare_templates.len(),
            tm.chart_templates.len(),
            tm.indicator_sets.len(),
        );
        tm
    };

    // Presets.
    let mut presets = HashMap::new();
    match list_presets(key_ref) {
        Ok(metas) => {
            for meta in &metas {
                match load_preset(&meta.id, key_ref) {
                    Ok(preset) => {
                        presets.insert(meta.id.clone(), preset);
                    }
                    Err(e) => {
                        log::warn!("[user_manager] failed to load preset {}: {}", meta.id, e);
                    }
                }
            }
            log::info!("[user_manager] loaded {} presets", presets.len());
        }
        Err(e) => log::warn!("[user_manager] failed to list presets: {}", e),
    }

    // Settings snapshots — always plaintext, never encrypted.
    let snapshots_path = active_profile_data_dir().join("settings_snapshots.json");
    let snapshots = match load_json::<SettingsSnapshots>(&snapshots_path, None) {
        Ok(s) => {
            log::info!("[user_manager] loaded settings snapshots");
            s
        }
        Err(_) => {
            log::info!("[user_manager] no settings snapshots found, using defaults");
            SettingsSnapshots::default()
        }
    };

    (presets, template_manager, profile, snapshots, key)
}
