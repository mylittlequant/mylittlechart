//! Chart-specific translation keys
//!
//! General keys (TextKey, MonthKey, TooltipKey) are in keys_common.
//! This module defines chart-specific keys only.

use super::lang::Language;
use super::tables::{
    MENU_KEY_TABLE,
    CONFIG_KEY_TABLE,
    WAVE_DEGREE_KEY_TABLE,
    STYLE_KEY_TABLE,
    LABEL_POSITION_KEY_TABLE,
    TOOLBAR_TOOLTIP_KEY_TABLE,
    WIZARD_KEY_TABLE,
    CLOCK_KEY_TABLE,
    SETTINGS_KEY_TABLE,
    USER_SETTINGS_KEY_TABLE,
    PROFILE_KEY_TABLE,
};

// =============================================================================
// Context Menu Keys
// =============================================================================

/// Context menu action keys
///
/// Variant order is **frozen** — discriminant == row index in `MENU_KEY_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum MenuKey {
    OpenSettings    = 0,
    Delete          = 1,
    Clone           = 2,
    Copy            = 3,
    LockUnlock      = 4,
    ShowHide        = 5,
    BringToFront    = 6,
    SendToBack      = 7,
    BringForward    = 8,
    SendBackward    = 9,
    SyncToAllCharts = 10,
    SyncEverywhere  = 11,
    NoSync          = 12,
}

impl MenuKey {
    pub const COUNT: usize = 13;

    /// Get translation for this key
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&MENU_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for MenuKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&MENU_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Config Section Keys
// =============================================================================

/// Configuration section/group keys
///
/// Variant order is **frozen** — discriminant == row index in `CONFIG_KEY_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum ConfigKey {
    // Common sections
    Labels          = 0,
    Levels          = 1,
    Percentages     = 2,
    LabelPosition   = 3,
    ExtendLines     = 4,
    Prices          = 5,
    Coordinates     = 6,
    Style           = 7,
    Appearance      = 8,
    Visibility      = 9,

    // Specific properties
    ShowLabels      = 10,
    ShowLevels      = 11,
    ShowPercentages = 12,
    ShowPrices      = 13,
    ShowCoordinates = 14,
    ShowNeckline    = 15,
    ShowBackground  = 16,
    ShowLines       = 17,
    ShowRatios      = 18,
    ShowTrendlines  = 19,
    ShowPrice       = 20,
    ShowLine        = 21,
    ShowHeader      = 22,
    ExtendLeft      = 23,
    ExtendRight     = 24,
    Reverse         = 25,
    LogScale        = 26,

    // Fibonacci specific
    FibLevels       = 27,
    CustomLevels    = 28,
    TrendBased      = 29,

    // Wave specific
    WaveDegree      = 30,
    WaveStyle       = 31,

    // Line/drawing specific
    TrendLine       = 32,
    Extend          = 33,
    FullCircle      = 34,
    Fill            = 35,

    // Pitchfork level modes
    LevelMode       = 36,
    AllLevels       = 37,
    BaseLevels      = 38,
    FibonacciLevels = 39,

    // Elliott wave and label settings
    LabelFontSize   = 40,
    LabelColor      = 41,
    Inverted        = 42,

    // Triangle pattern types
    TriangleType    = 43,
    Symmetrical     = 44,
    Ascending       = 45,
    Descending      = 46,
    Expanding       = 47,

    // Annotation text settings
    FontSize        = 48,
    TextColor       = 49,
    HeaderColor     = 50,
    GridColor       = 51,
    HeaderTextColor = 52,

    // Text formatting
    Content         = 53,
    Comment         = 54,
    Bold            = 55,
    Italic          = 56,
    BubbleWidth     = 57,
    BubbleHeight    = 58,
    Expanded        = 59,

    // Directions for signpost
    Direction       = 60,
    DirectionRight  = 61,
    DirectionLeft   = 62,
    DirectionUp     = 63,
    DirectionDown   = 64,

    // Table settings
    Rows            = 65,
    Columns         = 66,
    Header          = 67,
    Cell            = 68,

    // Text alignment
    HorizontalAlign = 69,
    VerticalAlign   = 70,
    AlignLeft       = 71,
    AlignCenter     = 72,
    AlignRight      = 73,
    AlignTop        = 74,
    AlignBottom     = 75,
}

impl ConfigKey {
    pub const COUNT: usize = 76;

    /// Get translation for this key
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&CONFIG_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for ConfigKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&CONFIG_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Elliott Wave Degree Keys
// =============================================================================

/// Elliott Wave degree names
///
/// Variant order is **frozen** — discriminant == row index in `WAVE_DEGREE_KEY_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum WaveDegreeKey {
    Supermillennium = 0,
    Millennium      = 1,
    Submillennium   = 2,
    GrandSupercycle = 3,
    Supercycle      = 4,
    Cycle           = 5,
    Primary         = 6,
    Intermediate    = 7,
    Minor           = 8,
    Minute          = 9,
    Minuette        = 10,
    Subminuette     = 11,
    Micro           = 12,
    Submicro        = 13,
    Miniscule       = 14,
}

impl WaveDegreeKey {
    pub const COUNT: usize = 15;

    /// Get translation for this key
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&WAVE_DEGREE_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for WaveDegreeKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&WAVE_DEGREE_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Style Keys
// =============================================================================

/// Style name keys (for line styles, presets, etc.)
///
/// Variant order is **frozen** — discriminant == row index in `STYLE_KEY_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum StyleKey {
    Standard = 0,
    Extended = 1,
    Filled   = 2,
    Thick    = 3,
    Dashed   = 4,
    Dotted   = 5,
    Thin     = 6,
    Bold     = 7,
}

impl StyleKey {
    pub const COUNT: usize = 8;

    /// Get translation for this key
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&STYLE_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for StyleKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&STYLE_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Label Position Keys
// =============================================================================

/// Label position keys
///
/// Variant order is **frozen** — discriminant == row index in `LABEL_POSITION_KEY_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum LabelPositionKey {
    Left    = 0,
    Right   = 1,
    Center  = 2,
    Top     = 3,
    Bottom  = 4,
    Inside  = 5,
    Outside = 6,
    Above   = 7,
    Below   = 8,
}

impl LabelPositionKey {
    pub const COUNT: usize = 9;

    /// Get translation for this key
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&LABEL_POSITION_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for LabelPositionKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&LABEL_POSITION_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Toolbar Tooltip Keys (app-specific — NOT in uzor core)
// =============================================================================

/// Toolbar button tooltip keys — chart application specific.
///
/// Window chrome tooltips (CloseWindow, Minimize, etc.) live in `crate::i18n::TooltipKey`.
/// Variant order is **frozen** — discriminant == row index in `TOOLBAR_TOOLTIP_KEY_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum ToolbarTooltipKey {
    // Drawing tools (left toolbar)
    Crosshair         = 0,
    TrendLine         = 1,
    HorizontalLine    = 2,
    VerticalLine      = 3,
    FibRetracement    = 4,
    Rectangle         = 5,
    DrawingTools      = 6,
    LineTool          = 7,
    FibTool           = 8,
    PatternTool       = 9,
    BrushTool         = 10,
    AnnotationTool    = 11,
    IconTool          = 12,
    ProjectionTool    = 13,
    Lock              = 14,
    Eye               = 15,
    DeleteTool        = 16,

    // Actions (top toolbar)
    Undo              = 17,
    Redo              = 18,
    MagnetMode        = 19,
    StayInDrawingMode = 20,
    Snapshot          = 21,
    Bookmark          = 22,
    MeasureTool       = 23,
    Indicators        = 24,
    Settings          = 25,
    Compare           = 26,
    SymbolSelector    = 27,
    TimeframeSelector = 28,
    ChartType         = 29,
    Layout            = 30,
    Presets           = 31,
    Screenshot        = 32,
    Expand            = 33,
    MainMenu          = 34,

    // Right toolbar (sidebar panels)
    Watchlist         = 35,
    Alerts            = 36,
    ObjectTree        = 37,
    Templates         = 38,
    Signals           = 39,
    Connectors        = 40,
    Performance       = 41,
    Agents            = 42,

    // General
    Search            = 43,
    FullScreen        = 44,
    SplitView         = 45,
    ServerTime        = 46,
}

impl ToolbarTooltipKey {
    pub const COUNT: usize = 47;

    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&TOOLBAR_TOOLTIP_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for ToolbarTooltipKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&TOOLBAR_TOOLTIP_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Welcome Wizard Keys
// =============================================================================

/// Welcome Wizard UI string keys
///
/// Variant order is **frozen** — discriminant == row index in `WIZARD_KEY_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum WizardKey {
    // Page 0 — Welcome + Language
    WelcomeTo             = 0,
    GetStarted            = 1,

    // Page 1 — Theme
    Theme                 = 2,
    ChooseTheme           = 3,

    // Page 2 — Profile + Passphrase
    ProfileAndSecurity    = 4,
    ProfileName           = 5,
    Passphrase            = 6,
    PassphrasePlaceholder = 7,
    MinPassphraseHint     = 8,
    ConfirmPassphrase     = 9,
    PassphraseMismatch    = 10,
    ZtInfo1               = 11,
    ZtInfo2               = 12,
    ZtInfo3               = 13,
    GenerateRecoveryPhrase = 14,

    // Page 3 — Recovery Key
    RecoveryKey           = 15,
    RecoveryWarning1      = 16,
    RecoveryWarning2      = 17,
    CopyKey               = 18,
    SavedAndContinue      = 19,

    // Shared
    Back                  = 20,
    Next                  = 21,
    Step2of4              = 22,
    Step3of4              = 23,
    Step4of4              = 24,
}

impl WizardKey {
    pub const COUNT: usize = 25;

    /// Get translation for this key
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&WIZARD_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for WizardKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&WIZARD_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Clock Popup Keys
// =============================================================================

/// Clock popup and time format setting keys
///
/// Variant order is **frozen** — discriminant == row index in `CLOCK_KEY_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum ClockKey {
    Timezone      = 0,
    Use24h        = 1,
    ShowUtcPrefix = 2,
    DateFormat    = 3,
    DayOfWeek     = 4,
}

impl ClockKey {
    pub const COUNT: usize = 5;

    /// Get translation for this key
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&CLOCK_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for ClockKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&CLOCK_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Settings Modal Keys
// =============================================================================

/// Chart settings modal string keys
///
/// Variant order is **frozen** — discriminant == row index in `SETTINGS_KEY_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum SettingsKey {
    // Header / footer
    Title                       = 0,
    ButtonTemplate              = 1,
    ButtonOk                    = 2,
    ButtonCancel                = 3,
    SaveAsTemplate              = 4,
    ApplyDefault                = 5,
    NoTemplates                 = 6,

    // Tab: Instrument — section headers
    SectionCandles              = 7,
    SectionDataConfig           = 8,
    SectionPriceTick            = 9,

    // Tab: Instrument — row labels
    BodyColorPrevClose          = 10,
    Body                        = 11,
    Borders                     = 12,
    Wick                        = 13,
    CountdownToClose            = 14,
    ExtendRight                 = 15,
    ExtendLeft                  = 16,
    LineStyle                   = 17,
    Precision                   = 18,

    // Precision dropdown
    PrecisionAuto               = 19,

    // Timezone dropdown values
    TimezoneUtc                 = 20,
    TimezoneMoscow              = 21,
    TimezoneLondon              = 22,
    TimezoneNewYork             = 23,
    TimezoneChicago             = 24,
    TimezoneLosAngeles          = 25,
    TimezoneTokyo               = 26,
    TimezoneHongKong            = 27,
    TimezoneSingapore           = 28,
    TimezoneSydney              = 29,

    // Tab: Appearance — section headers
    SectionPresets              = 30,
    SectionStyle                = 31,
    SectionStyleSettings        = 32,

    // Tab: Appearance — theme labels
    ThemeDark                   = 33,
    ThemeLight                  = 34,
    ThemeHighContrast           = 35,
    ThemeHighContrastMono       = 36,
    ThemeWizardHat              = 37,

    // Tab: Appearance — opacity slider labels
    ToolbarOpacity              = 38,
    ModalOpacity                = 39,
    SidebarOpacity              = 40,
    MenuOpacity                 = 41,
    ScaleOpacity                = 42,
    HoverOpacity                = 43,
    CrosshairLabelOpacity       = 44,
    BlurRadius                  = 45,

    // Tab: ScalesLines — section headers
    SectionGrid                 = 46,
    SectionPriceScale           = 47,
    SectionTimeScale            = 48,
    SectionPriceLines           = 49,
    SectionCrosshair            = 50,
    SectionScalePosition        = 51,
    SectionScaleSize            = 52,
    SectionTimeFormat           = 53,

    // Tab: ScalesLines — row labels
    ShowGrid                    = 54,
    VerticalLines               = 55,
    HorizontalLines             = 56,
    ShowPriceScaleRight         = 57,
    AutoScale                   = 58,
    ShowTimeScaleBottom         = 59,
    PrevDayClosePrice           = 60,
    CrosshairMode               = 61,
    CrosshairLineStyle          = 62,
    CrosshairLineWidth          = 63,
    CrosshairLineColor          = 64,
    PriceScalePosition          = 65,
    TimeScalePosition           = 66,
    CornerButtons               = 67,
    PriceScaleWidth             = 68,
    TimeScaleHeight             = 69,

    // Crosshair mode dropdown values
    CrosshairNormal             = 70,
    CrosshairMagnetStrong       = 71,
    CrosshairMagnetLight        = 72,
    CrosshairHidden             = 73,

    // Line style dropdown values
    LineStyleSolid              = 74,
    LineStyleDashed             = 75,
    LineStyleDotted             = 76,
    LineStyleLargeDashed        = 77,
    LineStyleSparseDotted       = 78,

    // Tick line style values (compact)
    TickStyleDash               = 79,
    TickStyleLine               = 80,
    TickStyleDots               = 81,

    // Scale position dropdown values
    ScalePosLeft                = 82,
    ScalePosRight               = 83,
    ScalePosHidden              = 84,
    ScalePosTop                 = 85,
    ScalePosBottom              = 86,

    // Corner visibility dropdown values
    CornerAlways                = 87,
    CornerOnHover               = 88,
    CornerNever                 = 89,

    // Tab: Status Line — section headers
    SectionLegend               = 90,
    SectionTooltip              = 91,
    SectionWatermark            = 92,
    SectionIndicators           = 93,

    // Tab: Status Line — row labels
    Position                    = 94,
    ShowOhlc                    = 95,
    ShowChange                  = 96,
    ShowPercent                 = 97,
    Show                        = 98,
    FollowCursor                = 99,
    Color                       = 100,
    Text                        = 101,
    ShowIndicatorPanel          = 102,

    // Legend position dropdown values
    LegendTopLeft               = 103,
    LegendTopRight              = 104,
    LegendBottomLeft            = 105,
    LegendBottomRight           = 106,
    LegendCenter                = 107,
}

impl SettingsKey {
    pub const COUNT: usize = 108;

    /// Get translation for this key
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&SETTINGS_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for SettingsKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&SETTINGS_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// User Settings Modal Keys
// =============================================================================

/// User settings modal string keys.
///
/// Variant order is **frozen** — discriminant == row index in `USER_SETTINGS_KEY_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum UserSettingsKey {
    // Header
    Title                    = 0,

    // General tab — section headers
    SectionProfile           = 1,
    SectionLanguage          = 2,
    SectionVersion           = 3,

    // General tab — buttons
    ShowWelcomeWizard        = 4,
    BtnRename                = 5,
    BtnAvatar                = 6,
    BtnNewProfile            = 7,
    BtnCreate                = 8,

    // Performance tab — section headers
    SectionIndicatorRecalc   = 9,
    SectionDiagnostics       = 10,
    SectionDataCache         = 11,

    // Performance tab — labels
    EnableDiagnosticLogging  = 12,
    SliderBgBars             = 13,
    SliderMaxBars            = 14,
    SliderCacheSizeMb        = 15,
    SliderAutoCleanupDays    = 16,

    // Server tab — section header + labels
    SectionServer            = 17,
    EnableAgentApiServer     = 18,
    ServerStopped            = 19,
    ServerOpenAccess         = 20,

    // Performance tab — slider description captions
    DescBgBars               = 21,
    DescMaxBars              = 22,
    DescCacheSize            = 23,
    DescAutoCleanup          = 24,
}

impl UserSettingsKey {
    pub const COUNT: usize = 25;

    /// Get translation for this key.
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&USER_SETTINGS_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for UserSettingsKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&USER_SETTINGS_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Profile Manager Modal Keys
// =============================================================================

/// Profile manager modal string keys.
///
/// Variant order is **frozen** — discriminant == row index in `PROFILE_KEY_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum ProfileKey {
    // ProfileList page
    ZeroTrust                = 0,
    Profiles                 = 1,
    CreateNewProfile         = 2,
    Unprotected              = 3,

    // UnlockPassphrase page
    EnterPassphraseToDecrypt = 4,
    BtnUnlock                = 5,
    LinkUseRecoveryKey       = 6,

    // CreatePassphrase page
    CreatePassphraseForKeys  = 7,
    ConfirmPassphrase        = 8,
    PassphrasesMismatch      = 9,
    BtnEncrypt               = 10,

    // CreateNew page
    NewProfile               = 11,
    ProfileName              = 12,
    BtnCreate                = 13,

    // SetNewPassphrase page
    SetNewPassphrase         = 14,
    VaultUnlockedWithKey     = 15,
    SetPassphraseToContinue  = 16,
    NewPassphrase            = 17,

    // UseRecoveryKey page
    RecoverWithKey           = 18,
    EnterRecoveryKeyShown    = 19,
    BtnRecover               = 20,

    // Shared helpers
    BackToProfiles           = 21,
    Passphrase               = 22,

    // ShowRecoveryKey page title
    RecoveryKeyTitle         = 23,
}

impl ProfileKey {
    pub const COUNT: usize = 24;

    /// Get translation for this key.
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&PROFILE_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for ProfileKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&PROFILE_KEY_TABLE[self as usize], lang_index)
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
