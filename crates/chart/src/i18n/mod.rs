//! Internationalization (i18n) module
//!
//! Simple, zero-dependency localization system using compile-time checked keys.
//!
//! # Design
//!
//! - All text keys are typed enums (compile-time safety)
//! - Translations are static strings (no heap allocation)
//! - Language can be switched at runtime with zero cost
//! - English is the default/fallback language
//!
//! # Usage
//!
//! ```ignore
//! use zengeld_chart::i18n::{Language, t, TextKey};
//!
//! // Get translation for current global language
//! let text = t(TextKey::Delete);
//!
//! // Or get for specific language
//! let text_ru = TextKey::Delete.get(Language::Ru);
//! ```
//!
//! # Adding New Languages
//!
//! 1. Add variant to `Language` enum
//! 2. Add match arm in each `TextKey::get()` implementation
//! 3. Add translations for all keys

mod keys;
mod translations;

pub use keys::{
    TextKey,
    MenuKey,
    ConfigKey,
    WaveDegreeKey,
    StyleKey,
    LabelPositionKey,
    MonthKey,
    month_names_short,
};

use std::sync::atomic::{AtomicU8, Ordering};

/// Supported languages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Language {
    /// English (default)
    #[default]
    En = 0,
    /// Russian
    Ru = 1,
}

impl Language {
    /// Get language from u8 value
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Language::Ru,
            _ => Language::En,
        }
    }

    /// Get language code (ISO 639-1)
    pub fn code(&self) -> &'static str {
        match self {
            Language::En => "en",
            Language::Ru => "ru",
        }
    }

    /// Get language name in English
    pub fn name(&self) -> &'static str {
        match self {
            Language::En => "English",
            Language::Ru => "Russian",
        }
    }

    /// Get language name in native language
    pub fn native_name(&self) -> &'static str {
        match self {
            Language::En => "English",
            Language::Ru => "Русский",
        }
    }

    /// Get all available languages
    pub fn all() -> &'static [Language] {
        &[Language::En, Language::Ru]
    }
}

// Global language setting (atomic for thread safety)
static CURRENT_LANGUAGE: AtomicU8 = AtomicU8::new(0);

/// Get current global language
#[inline]
pub fn current_language() -> Language {
    Language::from_u8(CURRENT_LANGUAGE.load(Ordering::Relaxed))
}

/// Set global language
#[inline]
pub fn set_language(lang: Language) {
    CURRENT_LANGUAGE.store(lang as u8, Ordering::Relaxed);
}

/// Translate a text key using current global language
#[inline]
pub fn t(key: TextKey) -> &'static str {
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
        // Save current
        let prev = current_language();

        set_language(Language::Ru);
        assert_eq!(current_language(), Language::Ru);

        set_language(Language::En);
        assert_eq!(current_language(), Language::En);

        // Restore
        set_language(prev);
    }

    #[test]
    fn test_translation() {
        set_language(Language::En);
        assert_eq!(t(TextKey::Delete), "Delete");

        set_language(Language::Ru);
        assert_eq!(t(TextKey::Delete), "Удалить");

        // Reset to default
        set_language(Language::En);
    }
}
