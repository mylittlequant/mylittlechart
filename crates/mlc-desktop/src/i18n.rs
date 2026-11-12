//! Desktop strings: Russian by default, English fallback, on uzor's global language
//! index (the same switch as `mlc_chart_view::i18n`).

use uzor::ui::i18n::Translate;

/// String keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum K {
    Watchlist,
    Search,
    NoMatches,
    Reset,
    Live,
    Demo,
    Loading,
    Offline,
    Timeframe,
    Chart,
}

const TABLE: [[&str; 2]; 10] = [
    ["Watchlist", "Список"],
    ["Search", "Поиск"],
    ["No matches", "Ничего не найдено"],
    ["Reset view", "Сбросить вид"],
    ["Live", "В реальном времени"],
    ["Demo data", "Демо-данные"],
    ["Loading…", "Загрузка…"],
    ["Offline: demo data", "Нет сети: демо-данные"],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn russian_first_english_fallback() {
        assert_eq!(K::Watchlist.translate(1), "Список");
        assert_eq!(K::Watchlist.translate(0), "Watchlist");
        assert_eq!(K::Search.translate(9), "Search");
        assert!(TABLE.iter().all(|r| !r[0].is_empty() && !r[1].is_empty()));
    }
}
