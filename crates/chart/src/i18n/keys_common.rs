//! Common translation keys (TextKey, TooltipKey, MonthKey).
//!
//! These were formerly in uzor::i18n; now owned by mlc.
//! Tables live in `tables_common.rs`.

use super::lang::Language;
use super::tables_common::{TEXT_KEY_TABLE, TOOLTIP_KEY_TABLE, MONTH_TABLE_SHORT, MONTH_TABLE_FULL};

// =============================================================================
// General Text Keys
// =============================================================================

/// General text keys used across the application.
///
/// Variant order is **frozen** — discriminant == row index in `TEXT_KEY_TABLE`.
/// New variants must be appended at the end; `COUNT` must be updated accordingly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum TextKey {
    // Common actions
    Delete     = 0,
    Clone      = 1,
    Copy       = 2,
    Cancel     = 3,
    Apply      = 4,
    Save       = 5,
    Reset      = 6,
    Close      = 7,
    Ok         = 8,
    Yes        = 9,
    No         = 10,

    // Visibility/state
    Show       = 11,
    Hide       = 12,
    Lock       = 13,
    Unlock     = 14,
    Enable     = 15,
    Disable    = 16,

    // Common labels
    Settings   = 17,
    Properties = 18,
    Color      = 19,
    Style      = 20,
    Width      = 21,
    Opacity    = 22,
    Background = 23,
    Foreground = 24,
    Border     = 25,
    Text       = 26,
    Font       = 27,
    Size       = 28,

    // Position
    Left       = 29,
    Right      = 30,
    Top        = 31,
    Bottom     = 32,
    Center     = 33,

    // Navigation / actions
    Back       = 34,
    Add        = 35,
    Loading    = 36,
    Disconnect = 37,
}

impl TextKey {
    /// Number of variants. Must equal the number of rows in `TEXT_KEY_TABLE`.
    pub const COUNT: usize = 38;

    /// Get translation for this key, with En fallback for empty cells.
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&TEXT_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for TextKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&TEXT_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Month Names (for TimeScale)
// =============================================================================

/// Month name keys for time axis.
///
/// Variant order is **frozen** (January=0 .. December=11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum MonthKey {
    January   = 0,
    February  = 1,
    March     = 2,
    April     = 3,
    May       = 4,
    June      = 5,
    July      = 6,
    August    = 7,
    September = 8,
    October   = 9,
    November  = 10,
    December  = 11,
}

impl MonthKey {
    /// Get short month name (3 letters), En fallback for empty cells.
    #[inline]
    pub fn short(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&MONTH_TABLE_SHORT[self as usize], lang as usize)
    }

    /// Get full month name, En fallback for empty cells.
    #[inline]
    pub fn full(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&MONTH_TABLE_FULL[self as usize], lang as usize)
    }

    /// Get MonthKey from month number (1-12). Returns `January` for out-of-range.
    pub fn from_month(month: u32) -> Self {
        match month {
            1  => Self::January,
            2  => Self::February,
            3  => Self::March,
            4  => Self::April,
            5  => Self::May,
            6  => Self::June,
            7  => Self::July,
            8  => Self::August,
            9  => Self::September,
            10 => Self::October,
            11 => Self::November,
            12 => Self::December,
            _  => Self::January,
        }
    }
}

impl uzor::i18n::Translate for MonthKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&MONTH_TABLE_SHORT[self as usize], lang_index)
    }
}

/// Get localized short month names array.
pub fn month_names_short(lang: Language) -> [&'static str; 12] {
    std::array::from_fn(|i| {
        uzor::table_lookup!(&MONTH_TABLE_SHORT[i], lang as usize)
    })
}

// =============================================================================
// Tooltip Keys
// =============================================================================

/// Window chrome tooltip keys — generic desktop UI controls.
///
/// App-specific tooltip keys (toolbar buttons, sidebar panels, etc.)
/// should be defined in the application's own i18n module.
///
/// Variant order is **frozen** — discriminant == row index in `TOOLTIP_KEY_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum TooltipKey {
    /// "Close window" / "Закрыть окно"
    CloseWindow = 0,
    /// "Quit application" / "Закрыть приложение"
    CloseApp    = 1,
    /// "Minimize" / "Свернуть"
    Minimize    = 2,
    /// "Maximize" / "Развернуть"
    Maximize    = 3,
    /// "Restore" / "Восстановить"
    Restore     = 4,
    /// "New window" / "Новое окно"
    NewWindow   = 5,
    /// "Menu" / "Меню"
    Menu        = 6,
    /// "New tab" / "Новая вкладка"
    NewTab      = 7,
    /// "Close tab" / "Закрыть вкладку"
    CloseTab    = 8,
    /// "Undo" / "Отменить"
    Undo        = 9,
}

impl TooltipKey {
    /// Number of variants. Must equal the number of rows in `TOOLTIP_KEY_TABLE`.
    pub const COUNT: usize = 10;

    /// Get translation for this key, with En fallback for empty cells.
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&TOOLTIP_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for TooltipKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&TOOLTIP_KEY_TABLE[self as usize], lang_index)
    }
}

/// Trait for types that can produce a translated display name.
pub trait Translatable {
    /// Get the translated display name for this value.
    fn display_name(&self, lang: Language) -> &'static str;

    /// Get display name using current global language.
    fn display_name_current(&self) -> &'static str {
        self.display_name(super::current_language())
    }
}

impl Translatable for TextKey {
    fn display_name(&self, lang: Language) -> &'static str {
        self.get(lang)
    }
}

impl Translatable for MonthKey {
    /// Returns the short month name for the given language.
    fn display_name(&self, lang: Language) -> &'static str {
        self.short(lang)
    }
}

impl Translatable for TooltipKey {
    fn display_name(&self, lang: Language) -> &'static str {
        self.get(lang)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_keys() {
        assert_eq!(TextKey::Delete.get(Language::En), "Delete");
        assert_eq!(TextKey::Delete.get(Language::Ru), "Удалить");
    }

    #[test]
    fn test_tooltip_keys() {
        assert_eq!(TooltipKey::CloseWindow.get(Language::En), "Close window");
        assert_eq!(TooltipKey::CloseWindow.get(Language::Ru), "Закрыть окно");
    }

    #[test]
    fn test_month_keys() {
        assert_eq!(MonthKey::January.short(Language::En), "Jan");
        assert_eq!(MonthKey::January.short(Language::Ru), "Янв");
        assert_eq!(MonthKey::January.full(Language::En), "January");
        assert_eq!(MonthKey::January.full(Language::Ru), "Январь");
    }

    #[test]
    fn test_month_names_short() {
        let names = month_names_short(Language::Ru);
        assert_eq!(names[0], "Янв");
        assert_eq!(names[11], "Дек");
    }
}
