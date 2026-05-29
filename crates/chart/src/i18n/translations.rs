//! Translation utilities — chart-specific Translatable implementations

use super::lang::Language;
use super::keys_common::Translatable;
use super::keys::{MenuKey, ConfigKey, WaveDegreeKey, StyleKey, LabelPositionKey, WizardKey};

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

impl Translatable for WizardKey {
    fn display_name(&self, lang: Language) -> &'static str {
        self.get(lang)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translatable_trait() {
        let key = MenuKey::Delete;
        assert_eq!(key.display_name(Language::En), "Delete");
        assert_eq!(key.display_name(Language::Ru), "Удалить");
    }
}
