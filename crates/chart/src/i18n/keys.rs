//! Chart-specific translation keys
//!
//! General keys (TextKey, MonthKey, TooltipKey) are provided by uzor::i18n.
//! This module defines chart-specific keys only.

use uzor::i18n::Language;

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
// Toolbar Tooltip Keys (app-specific — NOT in uzor core)
// =============================================================================

/// Toolbar button tooltip keys — chart application specific.
///
/// Window chrome tooltips (CloseWindow, Minimize, etc.) live in `uzor::i18n::TooltipKey`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolbarTooltipKey {
    // Drawing tools (left toolbar)
    Crosshair,
    TrendLine,
    HorizontalLine,
    VerticalLine,
    FibRetracement,
    Rectangle,
    DrawingTools,
    LineTool,
    FibTool,
    PatternTool,
    BrushTool,
    AnnotationTool,
    IconTool,
    ProjectionTool,
    Lock,
    Eye,
    DeleteTool,

    // Actions (top toolbar)
    Undo,
    Redo,
    MagnetMode,
    StayInDrawingMode,
    Snapshot,
    Bookmark,
    MeasureTool,
    Indicators,
    Settings,
    Compare,
    SymbolSelector,
    TimeframeSelector,
    ChartType,
    Layout,
    Presets,
    Screenshot,
    Expand,
    MainMenu,

    // Right toolbar (sidebar panels)
    Watchlist,
    Alerts,
    ObjectTree,
    Templates,
    Signals,
    Connectors,
    Performance,

    // General
    Search,
    FullScreen,
    SplitView,
    ServerTime,
}

impl ToolbarTooltipKey {
    pub fn get(self, lang: Language) -> &'static str {
        match lang {
            Language::En => self.en(),
            Language::Ru => self.ru(),
        }
    }

    fn en(self) -> &'static str {
        match self {
            Self::Crosshair => "Crosshair",
            Self::TrendLine => "Trend Line",
            Self::HorizontalLine => "Horizontal Line",
            Self::VerticalLine => "Vertical Line",
            Self::FibRetracement => "Fibonacci Retracement",
            Self::Rectangle => "Rectangle",
            Self::DrawingTools => "Drawing Tools",
            Self::LineTool => "Line Tools",
            Self::FibTool => "Fibonacci Tools",
            Self::PatternTool => "Pattern Tools",
            Self::BrushTool => "Brush & Shapes",
            Self::AnnotationTool => "Annotations",
            Self::IconTool => "Icons & Images",
            Self::ProjectionTool => "Positions & Projections",
            Self::Lock => "Lock Drawings",
            Self::Eye => "Show/Hide Drawings",
            Self::DeleteTool => "Delete Tools",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::MagnetMode => "Magnet Mode",
            Self::StayInDrawingMode => "Stay in Drawing Mode",
            Self::Snapshot => "Take Snapshot",
            Self::Bookmark => "Bookmark",
            Self::MeasureTool => "Measure",
            Self::Indicators => "Indicators",
            Self::Settings => "Settings",
            Self::Compare => "Compare Symbol",
            Self::SymbolSelector => "Symbol Selector",
            Self::TimeframeSelector => "Timeframe",
            Self::ChartType => "Chart Type",
            Self::Layout => "Layout",
            Self::Presets => "Presets",
            Self::Screenshot => "Screenshot",
            Self::Expand => "Expand Chart",
            Self::MainMenu => "Main Menu",
            Self::Watchlist => "Watchlist",
            Self::Alerts => "Alerts",
            Self::ObjectTree => "Object Tree",
            Self::Templates => "Templates",
            Self::Signals => "Signals",
            Self::Connectors => "Connectors",
            Self::Performance => "Performance",
            Self::Search => "Search",
            Self::FullScreen => "Full Screen",
            Self::SplitView => "Split View",
            Self::ServerTime => "Server Time",
        }
    }

    fn ru(self) -> &'static str {
        match self {
            Self::Crosshair => "Перекрестие",
            Self::TrendLine => "Трендовая линия",
            Self::HorizontalLine => "Горизонтальная линия",
            Self::VerticalLine => "Вертикальная линия",
            Self::FibRetracement => "Уровни Фибоначчи",
            Self::Rectangle => "Прямоугольник",
            Self::DrawingTools => "Инструменты рисования",
            Self::LineTool => "Инструменты линий",
            Self::FibTool => "Инструменты Фибоначчи",
            Self::PatternTool => "Инструменты паттернов",
            Self::BrushTool => "Кисть и фигуры",
            Self::AnnotationTool => "Аннотации",
            Self::IconTool => "Иконки и изображения",
            Self::ProjectionTool => "Позиции и проекции",
            Self::Lock => "Заблокировать рисунки",
            Self::Eye => "Показать/скрыть рисунки",
            Self::DeleteTool => "Инструменты удаления",
            Self::Undo => "Отменить",
            Self::Redo => "Повторить",
            Self::MagnetMode => "Режим магнита",
            Self::StayInDrawingMode => "Оставаться в режиме рисования",
            Self::Snapshot => "Сделать снимок",
            Self::Bookmark => "Закладка",
            Self::MeasureTool => "Измерить",
            Self::Indicators => "Индикаторы",
            Self::Settings => "Настройки",
            Self::Compare => "Сравнить символ",
            Self::SymbolSelector => "Выбор символа",
            Self::TimeframeSelector => "Таймфрейм",
            Self::ChartType => "Тип графика",
            Self::Layout => "Макет",
            Self::Presets => "Пресеты",
            Self::Screenshot => "Снимок экрана",
            Self::Expand => "Развернуть график",
            Self::MainMenu => "Главное меню",
            Self::Watchlist => "Список наблюдения",
            Self::Alerts => "Оповещения",
            Self::ObjectTree => "Дерево объектов",
            Self::Templates => "Шаблоны",
            Self::Signals => "Сигналы",
            Self::Connectors => "Коннекторы",
            Self::Performance => "Производительность",
            Self::Search => "Поиск",
            Self::FullScreen => "Полный экран",
            Self::SplitView => "Разделить вид",
            Self::ServerTime => "Серверное время",
        }
    }
}

// =============================================================================
// Welcome Wizard Keys
// =============================================================================

/// Welcome Wizard UI string keys
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WizardKey {
    // Page 0 — Welcome + Language
    WelcomeTo,
    GetStarted,

    // Page 1 — Theme
    Theme,
    ChooseTheme,

    // Page 2 — Profile + Passphrase
    ProfileAndSecurity,
    ProfileName,
    Passphrase,
    PassphrasePlaceholder,
    MinPassphraseHint,
    ZtInfo1,
    ZtInfo2,
    ZtInfo3,
    GenerateRecoveryPhrase,

    // Shared
    Back,
    Next,
    Step2of3,
    Step3of3,
}

impl WizardKey {
    /// Get translation for this key
    pub fn get(self, lang: Language) -> &'static str {
        match lang {
            Language::En => self.en(),
            Language::Ru => self.ru(),
        }
    }

    fn en(self) -> &'static str {
        match self {
            Self::WelcomeTo => "Welcome to",
            Self::GetStarted => "Get Started",
            Self::Theme => "Theme",
            Self::ChooseTheme => "Choose your visual theme",
            Self::ProfileAndSecurity => "Profile & Security",
            Self::ProfileName => "Profile Name",
            Self::Passphrase => "Passphrase",
            Self::PassphrasePlaceholder => "Click to type passphrase\u{2026}",
            Self::MinPassphraseHint => "Minimum 8 characters",
            Self::ZtInfo1 => "Your passphrase creates a Zero-trust container",
            Self::ZtInfo2 => "for API keys, indicators, strategies and agent prompts.",
            Self::ZtInfo3 => "It is never stored on any server.",
            Self::GenerateRecoveryPhrase => "Generate Recovery Phrase",
            Self::Back => "Back",
            Self::Next => "Next",
            Self::Step2of3 => "Step 2 of 3",
            Self::Step3of3 => "Step 3 of 3",
        }
    }

    fn ru(self) -> &'static str {
        match self {
            Self::WelcomeTo => "Добро пожаловать в",
            Self::GetStarted => "Начать",
            Self::Theme => "Тема",
            Self::ChooseTheme => "Выберите визуальную тему",
            Self::ProfileAndSecurity => "Профиль и безопасность",
            Self::ProfileName => "Имя профиля",
            Self::Passphrase => "Пароль",
            Self::PassphrasePlaceholder => "Нажмите, чтобы ввести пароль\u{2026}",
            Self::MinPassphraseHint => "Минимум 8 символов",
            Self::ZtInfo1 => "Ваш пароль создаёт Zero-trust контейнер",
            Self::ZtInfo2 => "для API-ключей, индикаторов, стратегий и агент-промптов.",
            Self::ZtInfo3 => "Он никогда не хранится на сервере.",
            Self::GenerateRecoveryPhrase => "Сгенерировать фразу восстановления",
            Self::Back => "Назад",
            Self::Next => "Далее",
            Self::Step2of3 => "Шаг 2 из 3",
            Self::Step3of3 => "Шаг 3 из 3",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
