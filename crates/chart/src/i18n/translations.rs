//! Translation utilities
//!
//! This module provides additional translation utilities and documentation
//! for extending the i18n system.

use super::{Language, TextKey, MenuKey, ConfigKey, WaveDegreeKey, StyleKey, LabelPositionKey};

/// Trait for types that can be translated
pub trait Translatable {
    /// Get the translated display name for this value
    fn display_name(&self, lang: Language) -> &'static str;

    /// Get display name using current global language
    fn display_name_current(&self) -> &'static str {
        self.display_name(super::current_language())
    }
}

// Implement Translatable for all key types
impl Translatable for TextKey {
    fn display_name(&self, lang: Language) -> &'static str {
        self.get(lang)
    }
}

impl Translatable for MenuKey {
    fn display_name(&self, lang: Language) -> &'static str {
        self.get(lang)
    }
}

impl Translatable for ConfigKey {
    fn display_name(&self, lang: Language) -> &'static str {
        self.get(lang)
    }
}

impl Translatable for WaveDegreeKey {
    fn display_name(&self, lang: Language) -> &'static str {
        self.get(lang)
    }
}

impl Translatable for StyleKey {
    fn display_name(&self, lang: Language) -> &'static str {
        self.get(lang)
    }
}

impl Translatable for LabelPositionKey {
    fn display_name(&self, lang: Language) -> &'static str {
        self.get(lang)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translatable_trait() {
        let key = TextKey::Delete;
        assert_eq!(key.display_name(Language::En), "Delete");
        assert_eq!(key.display_name(Language::Ru), "Удалить");
    }
}
