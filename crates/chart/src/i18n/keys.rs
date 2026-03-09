//! Translation keys
//!
//! All translatable text has a typed key for compile-time safety.

use super::Language;

// =============================================================================
// General Text Keys
// =============================================================================

/// General text keys used across the application
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextKey {
    // Common actions
    Delete,
    Clone,
    Copy,
    Cancel,
    Apply,
    Save,
    Reset,
    Close,
    Ok,
    Yes,
    No,

    // Visibility/state
    Show,
    Hide,
    Lock,
    Unlock,
    Enable,
    Disable,

    // Common labels
    Settings,
    Properties,
    Color,
    Style,
    Width,
    Opacity,
    Background,
    Foreground,
    Border,
    Text,
    Font,
    Size,

    // Position
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

impl TextKey {
    /// Get translation for this key
    pub fn get(self, lang: Language) -> &'static str {
        match lang {
            Language::En => self.en(),
            Language::Ru => self.ru(),
        }
    }

    fn en(self) -> &'static str {
        match self {
            Self::Delete => "Delete",
            Self::Clone => "Clone",
            Self::Copy => "Copy",
            Self::Cancel => "Cancel",
            Self::Apply => "Apply",
            Self::Save => "Save",
            Self::Reset => "Reset",
            Self::Close => "Close",
            Self::Ok => "OK",
            Self::Yes => "Yes",
            Self::No => "No",
            Self::Show => "Show",
            Self::Hide => "Hide",
            Self::Lock => "Lock",
            Self::Unlock => "Unlock",
            Self::Enable => "Enable",
            Self::Disable => "Disable",
            Self::Settings => "Settings",
            Self::Properties => "Properties",
            Self::Color => "Color",
            Self::Style => "Style",
            Self::Width => "Width",
            Self::Opacity => "Opacity",
            Self::Background => "Background",
            Self::Foreground => "Foreground",
            Self::Border => "Border",
            Self::Text => "Text",
            Self::Font => "Font",
            Self::Size => "Size",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Top => "Top",
            Self::Bottom => "Bottom",
            Self::Center => "Center",
        }
    }

    fn ru(self) -> &'static str {
        match self {
            Self::Delete => "Удалить",
            Self::Clone => "Клонировать",
            Self::Copy => "Копировать",
            Self::Cancel => "Отмена",
            Self::Apply => "Применить",
            Self::Save => "Сохранить",
            Self::Reset => "Сбросить",
            Self::Close => "Закрыть",
            Self::Ok => "ОК",
            Self::Yes => "Да",
            Self::No => "Нет",
            Self::Show => "Показать",
            Self::Hide => "Скрыть",
            Self::Lock => "Заблокировать",
            Self::Unlock => "Разблокировать",
            Self::Enable => "Включить",
            Self::Disable => "Отключить",
            Self::Settings => "Настройки",
            Self::Properties => "Свойства",
            Self::Color => "Цвет",
            Self::Style => "Стиль",
            Self::Width => "Ширина",
            Self::Opacity => "Прозрачность",
            Self::Background => "Фон",
            Self::Foreground => "Передний план",
            Self::Border => "Граница",
            Self::Text => "Текст",
            Self::Font => "Шрифт",
            Self::Size => "Размер",
            Self::Left => "Слева",
            Self::Right => "Справа",
            Self::Top => "Сверху",
            Self::Bottom => "Снизу",
            Self::Center => "По центру",
        }
    }
}

// =============================================================================
// Context Menu Keys
// =============================================================================

/// Context menu action keys
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuKey {
    OpenSettings,
    Delete,
    Clone,
    Copy,
    LockUnlock,
    ShowHide,
    BringToFront,
    SendToBack,
    BringForward,
    SendBackward,
    SyncToAllCharts,
    SyncEverywhere,
    NoSync,
}

impl MenuKey {
    /// Get translation for this key
    pub fn get(self, lang: Language) -> &'static str {
        match lang {
            Language::En => self.en(),
            Language::Ru => self.ru(),
        }
    }

    fn en(self) -> &'static str {
        match self {
            Self::OpenSettings => "Settings",
            Self::Delete => "Delete",
            Self::Clone => "Clone",
            Self::Copy => "Copy",
            Self::LockUnlock => "Lock/Unlock",
            Self::ShowHide => "Show/Hide",
            Self::BringToFront => "Bring to Front",
            Self::SendToBack => "Send to Back",
            Self::BringForward => "Bring Forward",
            Self::SendBackward => "Send Backward",
            Self::SyncToAllCharts => "Sync to All Charts",
            Self::SyncEverywhere => "Sync Everywhere",
            Self::NoSync => "Don't Sync",
        }
    }

    fn ru(self) -> &'static str {
        match self {
            Self::OpenSettings => "Настройки",
            Self::Delete => "Удалить",
            Self::Clone => "Клонировать",
            Self::Copy => "Копировать",
            Self::LockUnlock => "Заблокировать",
            Self::ShowHide => "Скрыть",
            Self::BringToFront => "На передний план",
            Self::SendToBack => "На задний план",
            Self::BringForward => "Переместить вперёд",
            Self::SendBackward => "Переместить назад",
            Self::SyncToAllCharts => "Синхронизировать на всех графиках",
            Self::SyncEverywhere => "Синхр. везде",
            Self::NoSync => "Не синхронизировать",
        }
    }
}

// =============================================================================
// Config Section Keys
// =============================================================================

/// Configuration section/group keys
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigKey {
    // Common sections
    Labels,
    Levels,
    Percentages,
    LabelPosition,
    ExtendLines,
    Prices,
    Coordinates,
    Style,
    Appearance,
    Visibility,

    // Specific properties
    ShowLabels,
    ShowLevels,
    ShowPercentages,
    ShowPrices,
    ShowCoordinates,
    ShowNeckline,
    ShowBackground,
    ShowLines,
    ShowRatios,
    ShowTrendlines,
    ShowPrice,
    ShowLine,
    ShowHeader,
    ExtendLeft,
    ExtendRight,
    Reverse,
    LogScale,

    // Fibonacci specific
    FibLevels,
    CustomLevels,
    TrendBased,

    // Wave specific
    WaveDegree,
    WaveStyle,

    // Line/drawing specific
    TrendLine,
    Extend,
    FullCircle,
    Fill,

    // Pitchfork level modes
    LevelMode,
    AllLevels,
    BaseLevels,
    FibonacciLevels,

    // Elliott wave and label settings
    LabelFontSize,
    LabelColor,
    Inverted,

    // Triangle pattern types
    TriangleType,
    Symmetrical,
    Ascending,
    Descending,
    Expanding,

    // Annotation text settings
    FontSize,
    TextColor,
    HeaderColor,
    GridColor,
    HeaderTextColor,

    // Text formatting
    Content,
    Comment,
    Bold,
    Italic,
    BubbleWidth,
    BubbleHeight,
    Expanded,

    // Directions for signpost
    Direction,
    DirectionRight,
    DirectionLeft,
    DirectionUp,
    DirectionDown,

    // Table settings
    Rows,
    Columns,
    Header,
    Cell,

    // Text alignment
    HorizontalAlign,
    VerticalAlign,
    AlignLeft,
    AlignCenter,
    AlignRight,
    AlignTop,
    AlignBottom,
}

impl ConfigKey {
    /// Get translation for this key
    pub fn get(self, lang: Language) -> &'static str {
        match lang {
            Language::En => self.en(),
            Language::Ru => self.ru(),
        }
    }

    fn en(self) -> &'static str {
        match self {
            Self::Labels => "Labels",
            Self::Levels => "Levels",
            Self::Percentages => "Percentages",
            Self::LabelPosition => "Label Position",
            Self::ExtendLines => "Extend Lines",
            Self::Prices => "Prices",
            Self::Coordinates => "Coordinates",
            Self::Style => "Style",
            Self::Appearance => "Appearance",
            Self::Visibility => "Visibility",
            Self::ShowLabels => "Show Labels",
            Self::ShowLevels => "Show Levels",
            Self::ShowPercentages => "Show Percentages",
            Self::ShowPrices => "Show Prices",
            Self::ShowCoordinates => "Show Coordinates",
            Self::ShowNeckline => "Show Neckline",
            Self::ShowBackground => "Show Background",
            Self::ShowLines => "Show Lines",
            Self::ShowRatios => "Show Ratios",
            Self::ShowTrendlines => "Show Trendlines",
            Self::ShowPrice => "Show Price",
            Self::ShowLine => "Show Line",
            Self::ShowHeader => "Show Header",
            Self::ExtendLeft => "Extend Left",
            Self::ExtendRight => "Extend Right",
            Self::Reverse => "Reverse",
            Self::LogScale => "Log Scale",
            Self::FibLevels => "Fib Levels",
            Self::CustomLevels => "Custom Levels",
            Self::TrendBased => "Trend Based",
            Self::WaveDegree => "Wave Degree",
            Self::WaveStyle => "Wave Style",
            Self::TrendLine => "Trend Line",
            Self::Extend => "Extend",
            Self::FullCircle => "Full Circle",
            Self::Fill => "Fill",
            Self::LevelMode => "Level Mode",
            Self::AllLevels => "All",
            Self::BaseLevels => "Base (0.25, 0.5, ...)",
            Self::FibonacciLevels => "Fibonacci (0.236, 0.382, ...)",
            Self::LabelFontSize => "Label Font Size",
            Self::LabelColor => "Label Color",
            Self::Inverted => "Inverted",
            Self::TriangleType => "Triangle Type",
            Self::Symmetrical => "Symmetrical",
            Self::Ascending => "Ascending",
            Self::Descending => "Descending",
            Self::Expanding => "Expanding",
            Self::FontSize => "Font Size",
            Self::TextColor => "Text Color",
            Self::HeaderColor => "Header Color",
            Self::GridColor => "Grid Color",
            Self::HeaderTextColor => "Header Text Color",
            Self::Content => "Text",
            Self::Comment => "Comment",
            Self::Bold => "Bold",
            Self::Italic => "Italic",
            Self::BubbleWidth => "Width",
            Self::BubbleHeight => "Height",
            Self::Expanded => "Expanded",
            Self::Direction => "Direction",
            Self::DirectionRight => "Right →",
            Self::DirectionLeft => "Left ←",
            Self::DirectionUp => "Up ↑",
            Self::DirectionDown => "Down ↓",
            Self::Rows => "Rows",
            Self::Columns => "Columns",
            Self::Header => "Header",
            Self::Cell => "Cell",
            Self::HorizontalAlign => "Horizontal Align",
            Self::VerticalAlign => "Vertical Align",
            Self::AlignLeft => "Left",
            Self::AlignCenter => "Center",
            Self::AlignRight => "Right",
            Self::AlignTop => "Top",
            Self::AlignBottom => "Bottom",
        }
    }

    fn ru(self) -> &'static str {
        match self {
            Self::Labels => "Метки",
            Self::Levels => "Уровни",
            Self::Percentages => "В процентах",
            Self::LabelPosition => "Позиция меток",
            Self::ExtendLines => "Продлить линии",
            Self::Prices => "Цены",
            Self::Coordinates => "Координаты",
            Self::Style => "Стиль",
            Self::Appearance => "Внешний вид",
            Self::Visibility => "Видимость",
            Self::ShowLabels => "Показать метки",
            Self::ShowLevels => "Показать уровни",
            Self::ShowPercentages => "Показать проценты",
            Self::ShowPrices => "Показать цены",
            Self::ShowCoordinates => "Показать координаты",
            Self::ShowNeckline => "Показывать линию шеи",
            Self::ShowBackground => "Показать фон",
            Self::ShowLines => "Показывать линии",
            Self::ShowRatios => "Показывать соотношения",
            Self::ShowTrendlines => "Показывать трендлинии",
            Self::ShowPrice => "Показать цену",
            Self::ShowLine => "Показать линию",
            Self::ShowHeader => "Показать заголовок",
            Self::ExtendLeft => "Продлить влево",
            Self::ExtendRight => "Продлить вправо",
            Self::Reverse => "Инвертировать",
            Self::LogScale => "Логарифмическая шкала",
            Self::FibLevels => "Уровни Фибоначчи",
            Self::CustomLevels => "Пользовательские уровни",
            Self::TrendBased => "На основе тренда",
            Self::WaveDegree => "Степень волны",
            Self::WaveStyle => "Стиль волны",
            Self::TrendLine => "Трендовая линия",
            Self::Extend => "Продлить",
            Self::FullCircle => "Полный круг",
            Self::Fill => "Заливка",
            Self::LevelMode => "Режим уровней",
            Self::AllLevels => "Все",
            Self::BaseLevels => "Базовые (0.25, 0.5, ...)",
            Self::FibonacciLevels => "Фибоначчи (0.236, 0.382, ...)",
            Self::LabelFontSize => "Размер шрифта меток",
            Self::LabelColor => "Цвет меток",
            Self::Inverted => "Инвертировать",
            Self::TriangleType => "Тип треугольника",
            Self::Symmetrical => "Симметричный",
            Self::Ascending => "Восходящий",
            Self::Descending => "Нисходящий",
            Self::Expanding => "Расширяющийся",
            Self::FontSize => "Размер шрифта",
            Self::TextColor => "Цвет текста",
            Self::HeaderColor => "Цвет заголовка",
            Self::GridColor => "Цвет сетки",
            Self::HeaderTextColor => "Цвет текста заголовка",
            Self::Content => "Текст",
            Self::Comment => "Комментарий",
            Self::Bold => "Жирный",
            Self::Italic => "Курсив",
            Self::BubbleWidth => "Ширина",
            Self::BubbleHeight => "Высота",
            Self::Expanded => "Развёрнута",
            Self::Direction => "Направление",
            Self::DirectionRight => "Вправо →",
            Self::DirectionLeft => "Влево ←",
            Self::DirectionUp => "Вверх ↑",
            Self::DirectionDown => "Вниз ↓",
            Self::Rows => "Строки",
            Self::Columns => "Столбцы",
            Self::Header => "Заголовок",
            Self::Cell => "Ячейка",
            Self::HorizontalAlign => "Горизонтальное выравнивание",
            Self::VerticalAlign => "Вертикальное выравнивание",
            Self::AlignLeft => "Слева",
            Self::AlignCenter => "По центру",
            Self::AlignRight => "Справа",
            Self::AlignTop => "Сверху",
            Self::AlignBottom => "Снизу",
        }
    }
}

// =============================================================================
// Elliott Wave Degree Keys
// =============================================================================

/// Elliott Wave degree names
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WaveDegreeKey {
    Supermillennium,
    Millennium,
    Submillennium,
    GrandSupercycle,
    Supercycle,
    Cycle,
    Primary,
    Intermediate,
    Minor,
    Minute,
    Minuette,
    Subminuette,
    Micro,
    Submicro,
    Miniscule,
}

impl WaveDegreeKey {
    /// Get translation for this key
    pub fn get(self, lang: Language) -> &'static str {
        match lang {
            Language::En => self.en(),
            Language::Ru => self.ru(),
        }
    }

    fn en(self) -> &'static str {
        match self {
            Self::Supermillennium => "Supermillennium",
            Self::Millennium => "Millennium",
            Self::Submillennium => "Submillennium",
            Self::GrandSupercycle => "Grand Supercycle",
            Self::Supercycle => "Supercycle",
            Self::Cycle => "Cycle",
            Self::Primary => "Primary",
            Self::Intermediate => "Intermediate",
            Self::Minor => "Minor",
            Self::Minute => "Minute",
            Self::Minuette => "Minuette",
            Self::Subminuette => "Subminuette",
            Self::Micro => "Micro",
            Self::Submicro => "Submicro",
            Self::Miniscule => "Miniscule",
        }
    }

    fn ru(self) -> &'static str {
        match self {
            Self::Supermillennium => "Супермиллениум",
            Self::Millennium => "Миллениум",
            Self::Submillennium => "Субмиллениум",
            Self::GrandSupercycle => "Гранд Суперцикл",
            Self::Supercycle => "Суперцикл",
            Self::Cycle => "Цикл",
            Self::Primary => "Первичная",
            Self::Intermediate => "Промежуточная",
            Self::Minor => "Второстепенная",
            Self::Minute => "Минута",
            Self::Minuette => "Минуэт",
            Self::Subminuette => "Субминуэт",
            Self::Micro => "Микро",
            Self::Submicro => "Субмикро",
            Self::Miniscule => "Минускул",
        }
    }
}

// =============================================================================
// Style Keys
// =============================================================================

/// Style name keys (for line styles, presets, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StyleKey {
    Standard,
    Extended,
    Filled,
    Thick,
    Dashed,
    Dotted,
    Thin,
    Bold,
}

impl StyleKey {
    /// Get translation for this key
    pub fn get(self, lang: Language) -> &'static str {
        match lang {
            Language::En => self.en(),
            Language::Ru => self.ru(),
        }
    }

    fn en(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Extended => "Extended",
            Self::Filled => "Filled",
            Self::Thick => "Thick",
            Self::Dashed => "Dashed",
            Self::Dotted => "Dotted",
            Self::Thin => "Thin",
            Self::Bold => "Bold",
        }
    }

    fn ru(self) -> &'static str {
        match self {
            Self::Standard => "Стандарт",
            Self::Extended => "Расширенный",
            Self::Filled => "С заливкой",
            Self::Thick => "Толстая",
            Self::Dashed => "Пунктирная",
            Self::Dotted => "Точечная",
            Self::Thin => "Тонкая",
            Self::Bold => "Жирная",
        }
    }
}

// =============================================================================
// Label Position Keys
// =============================================================================

/// Label position keys
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LabelPositionKey {
    Left,
    Right,
    Center,
    Top,
    Bottom,
    Inside,
    Outside,
    Above,
    Below,
}

impl LabelPositionKey {
    /// Get translation for this key
    pub fn get(self, lang: Language) -> &'static str {
        match lang {
            Language::En => self.en(),
            Language::Ru => self.ru(),
        }
    }

    fn en(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Center => "Center",
            Self::Top => "Top",
            Self::Bottom => "Bottom",
            Self::Inside => "Inside",
            Self::Outside => "Outside",
            Self::Above => "Above",
            Self::Below => "Below",
        }
    }

    fn ru(self) -> &'static str {
        match self {
            Self::Left => "Слева",
            Self::Right => "Справа",
            Self::Center => "По центру",
            Self::Top => "Сверху",
            Self::Bottom => "Снизу",
            Self::Inside => "Внутри",
            Self::Outside => "Снаружи",
            Self::Above => "Над",
            Self::Below => "Под",
        }
    }
}

// =============================================================================
// Month Names (for TimeScale)
// =============================================================================

/// Month name keys for time axis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MonthKey {
    January,
    February,
    March,
    April,
    May,
    June,
    July,
    August,
    September,
    October,
    November,
    December,
}

impl MonthKey {
    /// Get short month name (3 letters)
    pub fn short(self, lang: Language) -> &'static str {
        match lang {
            Language::En => self.short_en(),
            Language::Ru => self.short_ru(),
        }
    }

    /// Get full month name
    pub fn full(self, lang: Language) -> &'static str {
        match lang {
            Language::En => self.full_en(),
            Language::Ru => self.full_ru(),
        }
    }

    fn short_en(self) -> &'static str {
        match self {
            Self::January => "Jan",
            Self::February => "Feb",
            Self::March => "Mar",
            Self::April => "Apr",
            Self::May => "May",
            Self::June => "Jun",
            Self::July => "Jul",
            Self::August => "Aug",
            Self::September => "Sep",
            Self::October => "Oct",
            Self::November => "Nov",
            Self::December => "Dec",
        }
    }

    fn short_ru(self) -> &'static str {
        match self {
            Self::January => "Янв",
            Self::February => "Фев",
            Self::March => "Мар",
            Self::April => "Апр",
            Self::May => "Май",
            Self::June => "Июн",
            Self::July => "Июл",
            Self::August => "Авг",
            Self::September => "Сен",
            Self::October => "Окт",
            Self::November => "Ноя",
            Self::December => "Дек",
        }
    }

    fn full_en(self) -> &'static str {
        match self {
            Self::January => "January",
            Self::February => "February",
            Self::March => "March",
            Self::April => "April",
            Self::May => "May",
            Self::June => "June",
            Self::July => "July",
            Self::August => "August",
            Self::September => "September",
            Self::October => "October",
            Self::November => "November",
            Self::December => "December",
        }
    }

    fn full_ru(self) -> &'static str {
        match self {
            Self::January => "Январь",
            Self::February => "Февраль",
            Self::March => "Март",
            Self::April => "Апрель",
            Self::May => "Май",
            Self::June => "Июнь",
            Self::July => "Июль",
            Self::August => "Август",
            Self::September => "Сентябрь",
            Self::October => "Октябрь",
            Self::November => "Ноябрь",
            Self::December => "Декабрь",
        }
    }

    /// Get MonthKey from month number (1-12)
    pub fn from_month(month: u32) -> Self {
        match month {
            1 => Self::January,
            2 => Self::February,
            3 => Self::March,
            4 => Self::April,
            5 => Self::May,
            6 => Self::June,
            7 => Self::July,
            8 => Self::August,
            9 => Self::September,
            10 => Self::October,
            11 => Self::November,
            12 => Self::December,
            _ => Self::January, // fallback
        }
    }
}

/// Get localized short month names array
pub fn month_names_short(lang: Language) -> [&'static str; 12] {
    [
        MonthKey::January.short(lang),
        MonthKey::February.short(lang),
        MonthKey::March.short(lang),
        MonthKey::April.short(lang),
        MonthKey::May.short(lang),
        MonthKey::June.short(lang),
        MonthKey::July.short(lang),
        MonthKey::August.short(lang),
        MonthKey::September.short(lang),
        MonthKey::October.short(lang),
        MonthKey::November.short(lang),
        MonthKey::December.short(lang),
    ]
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
    fn test_month_keys() {
        assert_eq!(MonthKey::January.short(Language::En), "Jan");
        assert_eq!(MonthKey::January.short(Language::Ru), "Янв");
        assert_eq!(MonthKey::December.full(Language::Ru), "Декабрь");
    }

    #[test]
    fn test_menu_keys() {
        assert_eq!(MenuKey::BringToFront.get(Language::En), "Bring to Front");
        assert_eq!(MenuKey::BringToFront.get(Language::Ru), "На передний план");
    }

    #[test]
    fn test_config_keys() {
        assert_eq!(ConfigKey::Labels.get(Language::En), "Labels");
        assert_eq!(ConfigKey::Labels.get(Language::Ru), "Метки");
    }

    #[test]
    fn test_wave_degree_keys() {
        assert_eq!(WaveDegreeKey::Cycle.get(Language::En), "Cycle");
        assert_eq!(WaveDegreeKey::Cycle.get(Language::Ru), "Цикл");
    }

    #[test]
    fn test_style_keys() {
        assert_eq!(StyleKey::Standard.get(Language::En), "Standard");
        assert_eq!(StyleKey::Standard.get(Language::Ru), "Стандарт");
    }
}
