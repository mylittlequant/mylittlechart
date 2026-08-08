//! `Templates` / `TemplatesMut` — the only doctrinally-sanctioned access
//! path to `TemplateManager` from outside `mlc-backend`.
//!
//! `TemplateManager` never touches `DataBubble` — it owns the template
//! registry (primitives / indicators / compare / chart / indicator sets) as
//! its own state, with file-backed persistence. Like
//! `WatchlistManager`/`TagManager`/`PresetManager`, the facade holds exactly
//! one guard, no `BubbleStore` co-acquisition.
//!
//! See `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A.

use std::sync::{RwLockReadGuard, RwLockWriteGuard};

use mlc_core::templates::{ChartTemplate, CompareTemplate, IndicatorSet, IndicatorTemplate, PrimitiveTemplate, TemplateError};
use mlc_vault::vault::VaultKey;

use super::manager::TemplateManager;

/// Read-only facade over `TemplateManager`.
pub struct Templates<'a> {
    mgr: RwLockReadGuard<'a, TemplateManager>,
}

impl<'a> Templates<'a> {
    pub(crate) fn new(mgr: RwLockReadGuard<'a, TemplateManager>) -> Self {
        Self { mgr }
    }

    pub fn primitive_templates(&self) -> &[PrimitiveTemplate] {
        &self.mgr.primitive_templates
    }

    pub fn indicator_templates(&self) -> &[IndicatorTemplate] {
        &self.mgr.indicator_templates
    }

    pub fn compare_templates(&self) -> &[CompareTemplate] {
        &self.mgr.compare_templates
    }

    pub fn chart_templates(&self) -> &[ChartTemplate] {
        &self.mgr.chart_templates
    }

    pub fn indicator_sets(&self) -> &[IndicatorSet] {
        &self.mgr.indicator_sets
    }

    /// Merged display order for the primitive-settings Template menu's
    /// system-preset + user-template list — see `TemplateManager`'s own
    /// field doc for the key shape.
    pub fn primitive_template_order(&self) -> &[String] {
        &self.mgr.primitive_template_order
    }

    pub fn get_primitive_template(&self, id: &str) -> Option<&PrimitiveTemplate> {
        self.mgr.get_primitive_template(id)
    }

    pub fn get_primitive_templates_for_type(&self, type_id: &str) -> Vec<&PrimitiveTemplate> {
        self.mgr.get_primitive_templates_for_type(type_id)
    }

    pub fn get_indicator_template(&self, id: &str) -> Option<&IndicatorTemplate> {
        self.mgr.get_indicator_template(id)
    }

    pub fn get_indicator_templates_for_type(&self, type_id: &str) -> Vec<&IndicatorTemplate> {
        self.mgr.get_indicator_templates_for_type(type_id)
    }

    pub fn get_compare_template(&self, id: &str) -> Option<&CompareTemplate> {
        self.mgr.get_compare_template(id)
    }

    pub fn get_chart_template(&self, id: &str) -> Option<&ChartTemplate> {
        self.mgr.get_chart_template(id)
    }

    pub fn get_indicator_set(&self, id: &str) -> Option<&IndicatorSet> {
        self.mgr.get_indicator_set(id)
    }

    /// Persist the whole manager (all template categories + indicator-set
    /// manager) to the default templates directory.
    pub fn save_to_default_dir(&self, key: Option<&VaultKey>) -> Result<(), TemplateError> {
        self.mgr.save_to_default_dir(key)
    }
}

/// Mutable facade over `TemplateManager`.
pub struct TemplatesMut<'a> {
    mgr: RwLockWriteGuard<'a, TemplateManager>,
}

impl<'a> TemplatesMut<'a> {
    pub(crate) fn new(mgr: RwLockWriteGuard<'a, TemplateManager>) -> Self {
        Self { mgr }
    }

    /// Set the vault key used for encrypting/decrypting template files.
    /// Shared-BackendCore arc: there is one `TemplateManager` for the whole
    /// process now, so this only needs setting once per unlock, not per
    /// window (`docs/mlc/plans/shared-backend-core-2026-07-03.md` §2.5).
    pub fn set_vault_key(&mut self, key: Option<VaultKey>) {
        self.mgr.vault_key = key;
    }

    pub fn primitive_templates(&self) -> &[PrimitiveTemplate] {
        &self.mgr.primitive_templates
    }

    pub fn indicator_templates(&self) -> &[IndicatorTemplate] {
        &self.mgr.indicator_templates
    }

    pub fn compare_templates(&self) -> &[CompareTemplate] {
        &self.mgr.compare_templates
    }

    pub fn chart_templates(&self) -> &[ChartTemplate] {
        &self.mgr.chart_templates
    }

    pub fn indicator_sets(&self) -> &[IndicatorSet] {
        &self.mgr.indicator_sets
    }

    pub fn get_primitive_template(&self, id: &str) -> Option<&PrimitiveTemplate> {
        self.mgr.get_primitive_template(id)
    }

    pub fn get_indicator_template(&self, id: &str) -> Option<&IndicatorTemplate> {
        self.mgr.get_indicator_template(id)
    }

    pub fn get_compare_template(&self, id: &str) -> Option<&CompareTemplate> {
        self.mgr.get_compare_template(id)
    }

    pub fn get_chart_template(&self, id: &str) -> Option<&ChartTemplate> {
        self.mgr.get_chart_template(id)
    }

    pub fn get_indicator_set(&self, id: &str) -> Option<&IndicatorSet> {
        self.mgr.get_indicator_set(id)
    }

    pub fn add_primitive_template(&mut self, template: PrimitiveTemplate) -> Result<(), TemplateError> {
        self.mgr.add_primitive_template(template)
    }

    pub fn remove_primitive_template(&mut self, id: &str) -> Result<bool, TemplateError> {
        self.mgr.remove_primitive_template(id)
    }

    pub fn rename_primitive_template(&mut self, id: &str, new_name: &str) -> Result<bool, TemplateError> {
        self.mgr.rename_primitive_template(id, new_name)
    }

    pub fn primitive_template_order(&self) -> &[String] {
        &self.mgr.primitive_template_order
    }

    pub fn set_primitive_template_order(&mut self, order: Vec<String>) -> Result<(), TemplateError> {
        self.mgr.set_primitive_template_order(order)
    }

    pub fn add_indicator_template(&mut self, template: IndicatorTemplate) -> Result<(), TemplateError> {
        self.mgr.add_indicator_template(template)
    }

    pub fn remove_indicator_template(&mut self, id: &str) -> Result<bool, TemplateError> {
        self.mgr.remove_indicator_template(id)
    }

    pub fn add_compare_template(&mut self, template: CompareTemplate) -> Result<(), TemplateError> {
        self.mgr.add_compare_template(template)
    }

    pub fn remove_compare_template(&mut self, id: &str) -> Result<bool, TemplateError> {
        self.mgr.remove_compare_template(id)
    }

    pub fn add_chart_template(&mut self, template: ChartTemplate) -> Result<(), TemplateError> {
        self.mgr.add_chart_template(template)
    }

    pub fn remove_chart_template(&mut self, id: &str) -> Result<bool, TemplateError> {
        self.mgr.remove_chart_template(id)
    }

    pub fn add_indicator_set(&mut self, set: IndicatorSet) -> Result<(), TemplateError> {
        self.mgr.add_indicator_set(set)
    }

    pub fn remove_indicator_set(&mut self, id: &str) -> Result<bool, TemplateError> {
        self.mgr.remove_indicator_set(id)
    }

    pub fn save_indicator_set_manager(&self) -> Result<(), TemplateError> {
        self.mgr.save_indicator_set_manager()
    }

    /// Persist the whole manager (all template categories + indicator-set
    /// manager) to the default templates directory.
    pub fn save_to_default_dir(&self, key: Option<&VaultKey>) -> Result<(), TemplateError> {
        self.mgr.save_to_default_dir(key)
    }

    /// Replace the whole manager wholesale (startup load, cross-window sync).
    pub fn replace_all(&mut self, manager: TemplateManager) {
        *self.mgr = manager;
    }
}
