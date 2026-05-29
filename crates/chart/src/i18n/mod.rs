//! Internationalization (i18n) module
//!
//! Language, TextKey, MonthKey, TooltipKey, Translatable are defined locally in mlc.
//! uzor provides only the generic Translate trait + global lang-index storage.
//! This module re-exports everything and provides chart-specific convenience functions.

mod lang;
mod keys_common;
mod tables_common;
mod keys;
mod tables;
mod translations;

// Local Language + N_LANG
pub use lang::{Language, N_LANG};

// Common keys (formerly in uzor)
pub use keys_common::{
    TextKey,
    TooltipKey,
    MonthKey,
    Translatable,
    month_names_short,
};

// Chart-specific keys
pub use keys::{
    MenuKey,
    ConfigKey,
    WaveDegreeKey,
    StyleKey,
    LabelPositionKey,
    ToolbarTooltipKey,
    WizardKey,
    ClockKey,
    SettingsKey,
    UserSettingsKey,
    ProfileKey,
    ModalKey,
    IndicatorKey,
    SidebarKey,
};

/// Set the active language. Updates uzor global lang-index.
#[inline]
pub fn set_language(lang: Language) {
    uzor::i18n::set_lang_index(lang as u8);
}

/// Get the active language from uzor global lang-index.
#[inline]
pub fn current_language() -> Language {
    Language::from_u8(uzor::i18n::current_lang_index() as u8)
}

/// Translate a key using current global language.
#[inline]
pub fn t<K: uzor::i18n::Translate>(key: K) -> &'static str {
    uzor::i18n::t(key)
}

/// Translate a text key using current global language
#[inline]
pub fn t_text(key: TextKey) -> &'static str {
    key.get(current_language())
}

/// Translate a tooltip key using current global language
#[inline]
pub fn t_tooltip(key: TooltipKey) -> &'static str {
    key.get(current_language())
}

/// Translate a toolbar tooltip key using current global language
#[inline]
pub fn t_toolbar(key: ToolbarTooltipKey) -> &'static str {
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

/// Translate a wizard key using current global language
#[inline]
pub fn t_wizard(key: WizardKey) -> &'static str {
    key.get(current_language())
}

/// Translate a settings modal key using current global language
#[inline]
pub fn t_settings(key: SettingsKey) -> &'static str {
    key.get(current_language())
}

/// Translate a user settings modal key using current global language
#[inline]
pub fn t_user_settings(key: UserSettingsKey) -> &'static str {
    key.get(current_language())
}

/// Translate a profile manager key using current global language
#[inline]
pub fn t_profile(key: ProfileKey) -> &'static str {
    key.get(current_language())
}

/// Translate a shared modal key using current global language
#[inline]
pub fn t_modal(key: ModalKey) -> &'static str {
    key.get(current_language())
}

/// Translate an indicator settings modal key using current global language
#[inline]
pub fn t_indicator(key: IndicatorKey) -> &'static str {
    key.get(current_language())
}

/// Translate a sidebar panel key using current global language
#[inline]
pub fn t_sidebar(key: SidebarKey) -> &'static str {
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
