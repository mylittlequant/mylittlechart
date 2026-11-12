//! UI strings: Russian by default, English as the fallback (an empty Russian cell
//! shows the English one). The language is uzor's global index: 0 English, 1 Russian.

use uzor::ui::i18n::{current_lang_index, set_lang_index, Translate};

/// Languages, by uzor's index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En = 0,
    Ru = 1,
}

/// Switch the UI language.
pub fn set_lang(l: Lang) {
    set_lang_index(l as u8);
}

/// The current UI language.
pub fn lang() -> Lang {
    if current_lang_index() == 1 { Lang::Ru } else { Lang::En }
}

/// Pick the language from a locale tag (`ru-RU`, `en_US`): Russian unless it is
/// clearly another language with an English UI.
pub fn lang_for_locale(tag: &str) -> Lang {
    let t = tag.trim().to_ascii_lowercase();
    if t.is_empty() || t.starts_with("ru") || t.starts_with("be") || t.starts_with("uk") || t.starts_with("kk") { Lang::Ru } else { Lang::En }
}

/// String keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum K {
    Markets,
    Demo,
    Live,
    Loading,
    Offline,
    Reset,
    Hint,
    Open,
    High,
    Low,
    Close,
    Volume,
    Timeframes,
    Chart,
}

const TABLE: [[&str; 2]; 14] = [
    ["Markets", "Рынки"],
    ["Demo data", "Демо-данные"],
    ["Live", "В реальном времени"],
    ["Loading…", "Загрузка…"],
    ["Offline: demo data", "Нет сети: демо-данные"],
    ["Reset", "Сброс"],
    ["Pinch to zoom · hold for crosshair · double-tap to reset", "Щипок — масштаб · удержание — перекрестие · двойной тап — сброс"],
    ["O", "О"],
    ["H", "В"],
    ["L", "Н"],
    ["C", "З"],
    ["Vol", "Об"],
    ["Timeframe", "Таймфрейм"],
    ["Chart", "График"],
];

impl Translate for K {
    fn translate(self, lang_index: usize) -> &'static str {
        let row: &[&str] = &TABLE[self as usize];
        uzor::table_lookup!(row, lang_index)
    }
}

/// `k` in the current language.
pub fn t(k: K) -> &'static str {
    uzor::ui::i18n::t(k)
}

/// Timeframe label (`1h` → `1ч`).
pub fn tf_label(code: &str) -> String {
    match lang() {
        Lang::En => code.to_string(),
        Lang::Ru => code.replace('m', "м").replace('h', "ч").replace('d', "д"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn russian_by_default_english_fallback() {
        assert_eq!(K::Demo.translate(1), "Демо-данные");
        assert_eq!(K::Demo.translate(0), "Demo data");
        assert_eq!(K::Demo.translate(7), "Demo data", "unknown index falls back to English");
        assert_eq!(lang_for_locale(""), Lang::Ru);
        assert_eq!(lang_for_locale("ru-RU"), Lang::Ru);
        assert_eq!(lang_for_locale("en_US"), Lang::En);
        for row in TABLE {
            assert!(!row[0].is_empty(), "every key has English");
        }
    }
}
