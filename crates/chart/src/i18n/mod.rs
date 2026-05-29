//! Internationalization (i18n) module.
//!
//! # Architecture
//!
//! - The **mechanism** lives in `uzor`: a generic `Translate` trait
//!   (`fn translate(self, lang_index: usize) -> &'static str`), a global
//!   language-index (`current_lang_index` / `set_lang_index`), and the
//!   `uzor::table_lookup!` macro (returns the cell, falling back to column 0
//!   when empty). uzor knows nothing about concrete languages or strings.
//! - The **data** lives here: the [`Language`] enum (15 langs), every key enum
//!   (`TextKey`, `MenuKey`, `ConfigKey`, `PrimitiveNameKey`, …) and their
//!   `[[&str; N_LANG]; COUNT]` translation tables in `tables*.rs`. Each key enum
//!   `impl uzor::i18n::Translate` by indexing its table.
//!
//! UI code is immediate-mode: render calls `Key::X.get(current_language())`
//! every frame, so switching language is picked up automatically. The only
//! things that must be rebuilt on a language change are values cached outside
//! the render loop (e.g. `ToolbarConfig`, rebuilt in chart-app-vello).
//!
//! # Adding a translation key
//!
//! 1. Append a variant to the relevant enum in `keys.rs` / `keys_common.rs`
//!    (order is **frozen** — discriminant == row index; only add at the end).
//! 2. Bump that enum's `COUNT`.
//! 3. Append a row to its table in `tables.rs` / `tables_common.rs` with all
//!    `N_LANG` columns filled (column 0 = En is mandatory).
//! 4. Use it: `fill_text(MyKey::Variant.get(current_language()), …)`.
//!
//! # Adding a language
//!
//! 1. Add a variant to [`Language`] + a `LANG_META` row in `lang.rs`, bump `N_LANG`.
//! 2. Add one column to every table (empty `""` falls back to En until filled).
//! 3. Expose it in the language pickers — they already iterate `Language::all()`.
//! 4. Non-Latin scripts need a font in the uzor fallback chain (see uzor-fonts).
//!
//! The `tests::every_key_translated_in_all_languages` test fails the build if a
//! cell is left empty, so coverage can't silently regress — it is the source of
//! truth, not a hand-maintained doc.

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
    PrimitiveNameKey,
    PrimitiveTooltipKey,
    TradingKey,
    ToolbarMenuKey,
    CityKey,
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

/// Translate a primitive name key using current global language
#[inline]
pub fn t_primitive_name(key: PrimitiveNameKey) -> &'static str {
    key.get(current_language())
}

/// Translate a primitive tooltip key using current global language
#[inline]
pub fn t_primitive_tooltip(key: PrimitiveTooltipKey) -> &'static str {
    key.get(current_language())
}

/// Translate a trading panel key using current global language
#[inline]
pub fn t_trading(key: TradingKey) -> &'static str {
    key.get(current_language())
}

/// Translate a toolbar menu key using current global language
#[inline]
pub fn t_toolbar_menu(key: ToolbarMenuKey) -> &'static str {
    key.get(current_language())
}

/// Localize a primitive display name by type_id, returning an owned `String`.
/// Falls back to `raw_name.to_string()` for unrecognized type_ids (e.g. individual emoji_* variants).
#[inline]
pub fn localize_primitive_name(type_id: &str, raw_name: &str) -> String {
    PrimitiveNameKey::from_type_id(type_id)
        .map(|k| k.get(current_language()).to_string())
        .unwrap_or_else(|| raw_name.to_string())
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
