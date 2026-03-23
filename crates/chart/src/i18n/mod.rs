//! Internationalization (i18n) module
//!
//! Core i18n types (Language, TextKey, MonthKey, Translatable) are provided by uzor.
//! This module re-exports them and adds chart-specific translation keys.

mod keys;
mod translations;

// Re-export core i18n from uzor
pub use uzor::i18n::{
    Language, current_language, set_language,
    TextKey, MonthKey, TooltipKey,
    Translatable, month_names_short,
};

// Re-export chart-specific keys
pub use keys::{
    MenuKey,
    ConfigKey,
    WaveDegreeKey,
    StyleKey,
    LabelPositionKey,
};

// Chart-specific convenience functions (use uzor's Language)

/// Translate a text key using current global language (delegates to uzor)
#[inline]
pub fn t(key: TextKey) -> &'static str {
    key.get(current_language())
}

/// Translate a tooltip key using current global language
#[inline]
pub fn t_tooltip(key: TooltipKey) -> &'static str {
    key.get(current_language())
}

/// Translate a menu key using current global language
#[inline]
pub fn t_menu(key: MenuKey) -> &'static str {
    key.get(current_language())
}

/// Translate a config key using current global language
#[inline]
pub fn t_config(key: ConfigKey) -> &'static str {
    key.get(current_language())
}

/// Translate a wave degree key using current global language
#[inline]
pub fn t_wave(key: WaveDegreeKey) -> &'static str {
    key.get(current_language())
}

/// Translate a style key using current global language
#[inline]
pub fn t_style(key: StyleKey) -> &'static str {
    key.get(current_language())
}

/// Translate a label position key using current global language
#[inline]
pub fn t_label_pos(key: LabelPositionKey) -> &'static str {
    key.get(current_language())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_default() {
        assert_eq!(Language::default(), Language::En);
    }

    #[test]
    fn test_language_codes() {
        assert_eq!(Language::En.code(), "en");
        assert_eq!(Language::Ru.code(), "ru");
    }

    #[test]
    fn test_language_names() {
        assert_eq!(Language::En.name(), "English");
        assert_eq!(Language::Ru.name(), "Russian");
        assert_eq!(Language::Ru.native_name(), "Русский");
    }

    #[test]
    fn test_global_language() {
        let prev = current_language();
        set_language(Language::Ru);
        assert_eq!(current_language(), Language::Ru);
        set_language(Language::En);
        assert_eq!(current_language(), Language::En);
        set_language(prev);
    }

    #[test]
    fn test_translation() {
        set_language(Language::En);
        assert_eq!(t(TextKey::Delete), "Delete");
        set_language(Language::Ru);
        assert_eq!(t(TextKey::Delete), "Удалить");
        set_language(Language::En);
    }
}
