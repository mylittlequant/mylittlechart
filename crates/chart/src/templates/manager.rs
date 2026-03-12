//! [`TemplateManager`] — in-memory registry with file-backed persistence.
//!
//! Holds all loaded templates in memory and provides CRUD operations plus
//! bulk save/load from a directory tree.

use std::path::Path;

use super::chart_template::ChartTemplate;
use super::compare_template::CompareTemplate;
use super::indicator_set::IndicatorSet;
use super::indicator_template::IndicatorTemplate;
use super::primitive_template::PrimitiveTemplate;
use super::set_manager::IndicatorSetManager;
use super::storage::{
    category_dir, delete_template, load_all_templates,
    save_template, templates_root, TemplateError,
};
use crate::vault::VaultKey;

// =============================================================================
// Category constants
// =============================================================================

const CAT_PRIMITIVES: &str = "primitives";
const CAT_INDICATORS: &str = "indicators";
const CAT_COMPARE: &str = "compare";
const CAT_CHART: &str = "chart";
const CAT_INDICATOR_SETS: &str = "indicator_sets";

// =============================================================================
// TemplateManager
// =============================================================================

/// Filename used for the serialized [`IndicatorSetManager`] state.
const INDICATOR_SET_MANAGER_FILE: &str = "indicator_set_manager.json";

/// Central registry for all template types.
///
/// Load from disk once at startup with [`TemplateManager::load_from_default_dir`], then
/// use the CRUD methods.  Call [`TemplateManager::save_to_default_dir`] to flush all
/// in-memory templates back to disk (e.g. before quitting).
///
/// Individual template adds/removes are also persisted immediately via the
/// per-item `save`/`delete` helpers.
#[derive(Debug, Default, Clone)]
pub struct TemplateManager {
    /// Drawing primitive style templates.
    pub primitive_templates: Vec<PrimitiveTemplate>,
    /// Indicator parameter + style templates.
    pub indicator_templates: Vec<IndicatorTemplate>,
    /// Compare overlay visual style templates.
    pub compare_templates: Vec<CompareTemplate>,
    /// Chart settings templates.
    pub chart_templates: Vec<ChartTemplate>,
    /// Indicator set (group) templates.
    pub indicator_sets: Vec<IndicatorSet>,
    /// Manager for named collections of indicator sets (active-set selection,
    /// ordering, add/remove/rename operations).
    pub indicator_set_manager: IndicatorSetManager,
    /// Encryption key for write-through persistence.  `None` means plaintext.
    pub vault_key: Option<VaultKey>,
}

impl TemplateManager {
    /// Create an empty manager (no templates loaded).
    pub fn new() -> Self {
        Self {
            primitive_templates: Vec::new(),
            indicator_templates: Vec::new(),
            compare_templates: Vec::new(),
            chart_templates: Vec::new(),
            indicator_sets: Vec::new(),
            indicator_set_manager: IndicatorSetManager::new(),
            vault_key: None,
        }
    }

    // =========================================================================
    // Load / Save (bulk)
    // =========================================================================

    /// Load all templates with optional encryption key.
    ///
    /// Handles both `.enc` and `.json` files in each category directory.
    pub fn load_from_default_dir(key: Option<&VaultKey>) -> Self {
        let root = templates_root();
        Self {
            primitive_templates: load_all_templates(&category_dir(CAT_PRIMITIVES), key),
            indicator_templates: load_all_templates(&category_dir(CAT_INDICATORS), key),
            compare_templates: load_all_templates(&category_dir(CAT_COMPARE), key),
            chart_templates: load_all_templates(&category_dir(CAT_CHART), key),
            indicator_sets: load_all_templates(&category_dir(CAT_INDICATOR_SETS), key),
            indicator_set_manager: load_indicator_set_manager(&root),
            vault_key: key.copied(),
        }
    }

    /// Serialize all in-memory templates with optional encryption.
    ///
    /// Uses `save_template` for each template so that encrypted installs
    /// write `.enc` files and plain installs write `.json` files.
    pub fn save_to_default_dir(&self, key: Option<&VaultKey>) -> Result<(), TemplateError> {
        let root = templates_root();
        let prim_dir = category_dir(CAT_PRIMITIVES);
        let ind_dir = category_dir(CAT_INDICATORS);
        let cmp_dir = category_dir(CAT_COMPARE);
        let chart_dir = category_dir(CAT_CHART);
        let sets_dir = category_dir(CAT_INDICATOR_SETS);

        for t in &self.primitive_templates {
            save_template(t, &t.id, &prim_dir, key)?;
        }
        for t in &self.indicator_templates {
            save_template(t, &t.id, &ind_dir, key)?;
        }
        for t in &self.compare_templates {
            save_template(t, &t.id, &cmp_dir, key)?;
        }
        for t in &self.chart_templates {
            save_template(t, &t.id, &chart_dir, key)?;
        }
        for t in &self.indicator_sets {
            save_template(t, &t.id, &sets_dir, key)?;
        }
        save_indicator_set_manager(&self.indicator_set_manager, &root)?;
        Ok(())
    }

    // =========================================================================
    // Primitive templates
    // =========================================================================

    /// Add a primitive template and immediately persist it to disk.
    pub fn add_primitive_template(
        &mut self,
        template: PrimitiveTemplate,
    ) -> Result<(), TemplateError> {
        let dir = category_dir(CAT_PRIMITIVES);
        save_template(&template, &template.id, &dir, self.vault_key.as_ref())?;
        self.primitive_templates.push(template);
        Ok(())
    }

    /// Remove a primitive template by `id` and delete its file from disk.
    ///
    /// Returns `true` if the template was found and removed.
    pub fn remove_primitive_template(&mut self, id: &str) -> Result<bool, TemplateError> {
        let pos = self.primitive_templates.iter().position(|t| t.id == id);
        if let Some(i) = pos {
            self.primitive_templates.remove(i);
            let dir = category_dir(CAT_PRIMITIVES);
            delete_template(id, &dir)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Return all primitive templates whose `type_id` matches the given string.
    pub fn get_primitive_templates_for_type(&self, type_id: &str) -> Vec<&PrimitiveTemplate> {
        self.primitive_templates
            .iter()
            .filter(|t| t.type_id == type_id)
            .collect()
    }

    /// Find a primitive template by `id`.
    pub fn get_primitive_template(&self, id: &str) -> Option<&PrimitiveTemplate> {
        self.primitive_templates.iter().find(|t| t.id == id)
    }

    // =========================================================================
    // Indicator templates
    // =========================================================================

    /// Add an indicator template and immediately persist it to disk.
    pub fn add_indicator_template(
        &mut self,
        template: IndicatorTemplate,
    ) -> Result<(), TemplateError> {
        let dir = category_dir(CAT_INDICATORS);
        save_template(&template, &template.id, &dir, self.vault_key.as_ref())?;
        self.indicator_templates.push(template);
        Ok(())
    }

    /// Remove an indicator template by `id` and delete its file from disk.
    ///
    /// Returns `true` if the template was found and removed.
    pub fn remove_indicator_template(&mut self, id: &str) -> Result<bool, TemplateError> {
        let pos = self.indicator_templates.iter().position(|t| t.id == id);
        if let Some(i) = pos {
            self.indicator_templates.remove(i);
            let dir = category_dir(CAT_INDICATORS);
            delete_template(id, &dir)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Return all indicator templates whose `type_id` matches the given string.
    pub fn get_indicator_templates_for_type(&self, type_id: &str) -> Vec<&IndicatorTemplate> {
        self.indicator_templates
            .iter()
            .filter(|t| t.type_id == type_id)
            .collect()
    }

    /// Find an indicator template by `id`.
    pub fn get_indicator_template(&self, id: &str) -> Option<&IndicatorTemplate> {
        self.indicator_templates.iter().find(|t| t.id == id)
    }

    // =========================================================================
    // Compare templates
    // =========================================================================

    /// Add a compare template and immediately persist it to disk.
    pub fn add_compare_template(
        &mut self,
        template: CompareTemplate,
    ) -> Result<(), TemplateError> {
        let dir = category_dir(CAT_COMPARE);
        save_template(&template, &template.id, &dir, self.vault_key.as_ref())?;
        self.compare_templates.push(template);
        Ok(())
    }

    /// Remove a compare template by `id` and delete its file from disk.
    ///
    /// Returns `true` if the template was found and removed.
    pub fn remove_compare_template(&mut self, id: &str) -> Result<bool, TemplateError> {
        let pos = self.compare_templates.iter().position(|t| t.id == id);
        if let Some(i) = pos {
            self.compare_templates.remove(i);
            let dir = category_dir(CAT_COMPARE);
            delete_template(id, &dir)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Find a compare template by `id`.
    pub fn get_compare_template(&self, id: &str) -> Option<&CompareTemplate> {
        self.compare_templates.iter().find(|t| t.id == id)
    }

    // =========================================================================
    // Chart templates
    // =========================================================================

    /// Add a chart template and immediately persist it to disk.
    pub fn add_chart_template(
        &mut self,
        template: ChartTemplate,
    ) -> Result<(), TemplateError> {
        let dir = category_dir(CAT_CHART);
        save_template(&template, &template.id, &dir, self.vault_key.as_ref())?;
        self.chart_templates.push(template);
        Ok(())
    }

    /// Remove a chart template by `id` and delete its file from disk.
    ///
    /// Returns `true` if the template was found and removed.
    pub fn remove_chart_template(&mut self, id: &str) -> Result<bool, TemplateError> {
        let pos = self.chart_templates.iter().position(|t| t.id == id);
        if let Some(i) = pos {
            self.chart_templates.remove(i);
            let dir = category_dir(CAT_CHART);
            delete_template(id, &dir)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Find a chart template by `id`.
    pub fn get_chart_template(&self, id: &str) -> Option<&ChartTemplate> {
        self.chart_templates.iter().find(|t| t.id == id)
    }

    // =========================================================================
    // Indicator sets
    // =========================================================================

    /// Add an indicator set and immediately persist it to disk.
    pub fn add_indicator_set(&mut self, set: IndicatorSet) -> Result<(), TemplateError> {
        let dir = category_dir(CAT_INDICATOR_SETS);
        save_template(&set, &set.id, &dir, self.vault_key.as_ref())?;
        self.indicator_sets.push(set);
        Ok(())
    }

    /// Remove an indicator set by `id` and delete its file from disk.
    ///
    /// Returns `true` if the set was found and removed.
    pub fn remove_indicator_set(&mut self, id: &str) -> Result<bool, TemplateError> {
        let pos = self.indicator_sets.iter().position(|s| s.id == id);
        if let Some(i) = pos {
            self.indicator_sets.remove(i);
            let dir = category_dir(CAT_INDICATOR_SETS);
            delete_template(id, &dir)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Find an indicator set by `id`.
    pub fn get_indicator_set(&self, id: &str) -> Option<&IndicatorSet> {
        self.indicator_sets.iter().find(|s| s.id == id)
    }

    // =========================================================================
    // IndicatorSetManager persistence helpers (public convenience wrappers)
    // =========================================================================

    /// Persist the in-memory [`IndicatorSetManager`] to the default templates
    /// root immediately.
    ///
    /// Call this after any mutation to keep the on-disk state in sync.
    pub fn save_indicator_set_manager(&self) -> Result<(), TemplateError> {
        save_indicator_set_manager(&self.indicator_set_manager, &templates_root())
    }

}

// =============================================================================
// Module-private helpers for IndicatorSetManager single-file I/O
// =============================================================================

/// Load an [`IndicatorSetManager`] from `{dir}/indicator_set_manager.json`.
///
/// Returns a default (empty) manager if the file does not exist or cannot be
/// parsed, so startup always succeeds.
fn load_indicator_set_manager(dir: &Path) -> IndicatorSetManager {
    let path = dir.join(INDICATOR_SET_MANAGER_FILE);
    if !path.exists() {
        return IndicatorSetManager::new();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Serialize `manager` to `{dir}/indicator_set_manager.json`.
fn save_indicator_set_manager(
    manager: &IndicatorSetManager,
    dir: &Path,
) -> Result<(), TemplateError> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(INDICATOR_SET_MANAGER_FILE);
    let json = serde_json::to_string_pretty(manager)?;
    std::fs::write(&path, json)?;
    Ok(())
}
