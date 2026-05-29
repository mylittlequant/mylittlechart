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
    MODAL_KEY_TABLE,
    INDICATOR_MODAL_KEY_TABLE,
    SIDEBAR_KEY_TABLE,
    PRIMITIVE_NAME_TABLE,
    PRIMITIVE_TOOLTIP_TABLE,
    TRADING_KEY_TABLE,
    TOOLBAR_MENU_KEY_TABLE,
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

    // Vault unlock screen (shown when profile is passphrase-protected)
    UnlockYourData        = 25,
    UnlockSubtitle        = 26,
    ForgotPassphrase      = 27,
}

impl WizardKey {
    pub const COUNT: usize = 28;

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

    // Offline mode confirmation panel
    OfflineModeTitle         = 25,
    OfflineModeBody1         = 26,
    OfflineModeBody2         = 27,
}

impl UserSettingsKey {
    pub const COUNT: usize = 28;

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

// =============================================================================
// Modal Keys  (shared across modals: alert, watchlist, tags_tabs, chart_browser,
//              template_name, overlay_settings, search_overlay, hotkeys)
// =============================================================================

/// Shared modal UI string keys.
///
/// Variant order is **frozen** — discriminant == row index in `MODAL_KEY_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum ModalKey {
    // ---- Alert Settings ----
    /// "Edit Alert"
    EditAlert              = 0,
    /// "Create Alert"
    CreateAlert            = 1,
    /// "Settings" (tab)
    AlertTabSettings       = 2,
    /// "Notifications" (tab)
    AlertTabNotifications  = 3,
    /// "Source"
    AlertSource            = 4,
    /// "Condition"
    AlertCondition         = 5,
    /// "Price"
    AlertPrice             = 6,
    /// "Price 2"
    AlertPrice2            = 7,
    /// "Percentage"
    AlertPercentage        = 8,
    /// "Trigger Mode"
    AlertTriggerMode       = 9,
    /// "Count"
    AlertCount             = 10,
    /// "Message"
    AlertMessage           = 11,
    /// "Signal Kind"
    AlertSignalKind        = 12,
    /// "Any"
    AlertAny               = 13,
    /// "Save" (alert edit button)
    AlertSave              = 14,
    /// "Create" (alert create button)
    AlertCreate            = 15,
    /// "No alerts"
    NoAlerts               = 16,
    /// "Subscribers"
    Subscribers            = 17,
    /// "Detect Users"
    DetectUsers            = 18,
    /// "Add"
    Add                    = 19,
    /// "Test Connection"
    TestConnection         = 20,
    /// "URL"
    Url                    = 21,

    // ---- Watchlist Modal ----
    /// "Watchlist"
    Watchlist              = 22,
    /// "Overview" (tab)
    WatchlistOverview      = 23,
    /// "Groups" (tab)
    WatchlistGroups        = 24,
    /// "SYMBOL" column header
    ColSymbol              = 25,
    /// "EXCHANGE" column header
    ColExchange            = 26,
    /// "LAST" column header
    ColLast                = 27,
    /// "CHG%" column header
    ColChgPct              = 28,
    /// "CHG" column header
    ColChg                 = 29,
    /// "HIGH" column header
    ColHigh                = 30,
    /// "LOW" column header
    ColLow                 = 31,
    /// "VOL" column header
    ColVol                 = 32,

    // ---- Tags & Tabs Modal ----
    /// "TAGS & TABS"
    TagsAndTabs            = 33,
    /// "TABS" (section label)
    SectionTabs            = 34,
    /// "TAGS" (section label)
    SectionTags            = 35,
    /// "Hidden:" (section label)
    Hidden                 = 36,

    // ---- Chart Browser ----
    /// "Charts"
    Charts                 = 37,
    /// "CHART NAME" column header
    ColChartName           = 38,
    /// "Search charts..." placeholder
    SearchChartsPlaceholder= 39,

    // ---- Template Name Modal ----
    /// "Сохранить шаблон как..." / "Save template as..."
    SaveTemplateAs         = 40,
    /// "Название шаблона..." / "Template name..." placeholder
    TemplateNamePlaceholder= 41,
    /// "Сохранить" / "Save"
    SaveTemplate           = 42,

    // ---- Overlay / Tags-Tabs shared panel states ----
    /// "No panels"
    NoPanels               = 43,
    /// "No hidden panels"
    NoHiddenPanels         = 44,
    /// "Restore"
    Restore                = 45,
    /// "Select a panel on the map"
    SelectPanelOnMap       = 46,
    /// "No panel data available"
    NoPanelData            = 47,
    /// "Delete" (panel delete button — same as TextKey::Delete but kept here
    /// so overlay_settings doesn't need to import keys_common)
    DeletePanel            = 48,

    // ---- Search Overlay ----
    /// "Symbol Search"
    SymbolSearch           = 49,
    /// "Add Indicator"
    AddIndicator           = 50,
    /// "Compare Symbol"
    CompareSymbol          = 51,
    /// "Search symbol..." placeholder
    SearchSymbolPlaceholder= 52,
    /// "Search indicator..." placeholder
    SearchIndicatorPlaceholder = 53,
    /// "Search symbol to compare..." placeholder
    SearchCompareSymbolPlaceholder = 54,
    /// "No indicators in this category"
    NoIndicatorsInCategory = 55,
    /// "Type to search..."
    TypeToSearch           = 56,
    /// "Nothing found"
    NothingFound           = 57,
    /// "Save current indicators as set"
    SaveIndicatorSet       = 58,
    /// "No saved indicator sets yet"
    NoSavedIndicatorSets   = 59,

    // ---- Hotkeys Modal ----
    /// "Keyboard Shortcuts"
    KeyboardShortcuts      = 60,
    /// "Undo" hotkey description
    HkUndo                 = 61,
    /// "Redo"
    HkRedo                 = 62,
    /// "Save template"
    HkSaveTemplate         = 63,
    /// "Delete selected"
    HkDeleteSelected       = 64,
    /// "Deselect / Close modal"
    HkDeselect             = 65,
    /// "Play/Pause replay"
    HkPlayPause            = 66,
    /// "Search indicators"
    HkSearchIndicators     = 67,
    /// "Symbol search"
    HkSymbolSearch         = 68,
    /// "Copy"
    HkCopy                 = 69,
    /// "Paste"
    HkPaste                = 70,
    /// "Zoom in/out"
    HkZoom                 = 71,
    /// "Pan chart"
    HkPan                  = 72,
}

impl ModalKey {
    pub const COUNT: usize = 73;

    /// Get translation for this key.
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&MODAL_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for ModalKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&MODAL_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Indicator Settings Modal Keys
// =============================================================================

/// Indicator settings modal string keys.
///
/// Variant order is **frozen** — discriminant == row index in `INDICATOR_MODAL_KEY_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum IndicatorKey {
    /// "No configurable parameters"
    NoParams               = 0,
    /// "No configurable outputs"
    NoOutputs              = 1,
    /// "Enable signals"
    EnableSignals          = 2,
    /// "Auto-detects signals based on"
    SignalsAutoLine1       = 3,
    /// "indicator values (crossovers, levels, etc.)"
    SignalsAutoLine2       = 4,
    /// "Shape:"
    Shape                  = 5,
    /// "Bull color:"
    BullColor              = 6,
    /// "Bear color:"
    BearColor              = 7,
    /// "Size:"
    SignalSize             = 8,
    /// "Offset:"
    SignalOffset           = 9,
    /// "Short name:"
    ShortName              = 10,
    /// "Category:"
    Category               = 11,
    /// "Overlay:"
    Overlay                = 12,
    /// "Bounds:"
    Bounds                 = 13,
    /// "Description:"
    Description            = 14,
    /// "No description"
    NoDescription          = 15,
    /// "Indicator info"
    IndicatorInfo          = 16,
    /// "Metadata unavailable"
    MetadataUnavailable    = 17,
    /// "Template" (footer button)
    Template               = 18,
    /// "Save as..." (dropdown item)
    SaveAs                 = 19,
    /// "Apply default" (dropdown item)
    ApplyDefault           = 20,

    // ---- Primitive settings specific ----
    /// "This primitive does not support text"
    NoTextSupport          = 21,
    /// "This primitive does not support levels"
    NoLevelsSupport        = 22,
    /// "Price" (field label in primitive coordinate editor)
    PriceLabel             = 23,
    /// "Bar" (field label in primitive coordinate editor)
    BarLabel               = 24,
    /// Bold toggle marker ("B" / "Ж")
    TextBold               = 25,
    /// Italic toggle marker ("I" / "К")
    TextItalic             = 26,
}

impl IndicatorKey {
    pub const COUNT: usize = 27;

    /// Get translation for this key.
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&INDICATOR_MODAL_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for IndicatorKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&INDICATOR_MODAL_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Sidebar Panel Keys  (connector details, agent panel, slot toolbar)
// =============================================================================

/// Sidebar UI string keys — connector detail rows, agent/slot panel UI.
///
/// Variant order is **frozen** — discriminant == row index in `SIDEBAR_KEY_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum SidebarKey {
    // ---- Connector detail row labels ----
    /// "REST:"
    RestLabel            = 0,
    /// "WS:"
    WsLabel              = 1,
    /// "Trading:"
    TradingLabel         = 2,
    /// "Acct:"
    AcctLabel            = 3,
    /// "Pos:"
    PosLabel             = 4,
    /// "Batch:"
    BatchLabel           = 5,
    /// "Aggregated:"
    AggregatedLabel      = 6,
    /// "Timeframes:"
    TimeframesLabel      = 7,
    /// "Metrics"
    MetricsLabel         = 8,
    /// "HTTP req/s"
    HttpReqsLabel        = 9,
    /// "REST lat."
    RestLatLabel         = 10,
    /// "WS ping"
    WsPingLabel          = 11,

    // ---- Slot source-mode toggle buttons ----
    /// "A" — Auto mode button
    SourceAutoBtn        = 12,
    /// "P" — Pinned mode button
    SourcePinnedBtn      = 13,
    /// "L" — Linked mode button
    SourceLinkedBtn      = 14,

    // ---- Agent panel empty/action state labels ----
    /// "Pick a CLI above to open a pane"
    PickCliPrompt        = 15,
    /// "Sessions"
    SessionsBtn          = 16,
    /// "No sessions yet"
    NoSessionsYet        = 17,
    /// "Start"
    StartBtn             = 18,
    /// "No messages yet"
    NoMessagesYet        = 19,

    // ---- Agent panel button tooltips ----
    /// "Terminal mode (PTY)"
    AgentModePty         = 20,
    /// "Chat mode"
    AgentModeChat        = 21,
    /// "Open Claude session"
    AgentSpawnClaude     = 22,
    /// "Open Codex session"
    AgentSpawnCodex      = 23,
    /// "Open Gemini session"
    AgentSpawnGemini     = 24,
    /// "Open OpenCode session"
    AgentSpawnOpencode   = 25,
    /// "Split horizontal"
    AgentSplitH          = 26,
    /// "Split vertical"
    AgentSplitV          = 27,
    /// "Replace focused pane"
    AgentSplitReplace    = 28,
    /// "Expand / Collapse"
    AgentExpandToggle    = 29,
    /// "Reset pane sizes"
    AgentResetSizes      = 30,
    /// "Close focused pane"
    AgentClosePane       = 31,

    // ---- Free slot toolbar tooltips ----
    /// "Close panel"
    SlotClosePanel       = 32,
    /// "Column visibility"
    SlotColConfig        = 33,
    /// "Toggle auto-center"
    SlotAutoCenter       = 34,
    /// "Cycle volume filter"
    SlotVolumeFilter     = 35,
    /// "Tick size"
    SlotTickSize         = 36,
    /// "Add panel"
    SlotAddPanel         = 37,
    /// "Auto symbol (follow chart)"
    SlotSourceAuto       = 38,
    /// "Pinned symbol (fixed)"
    SlotSourcePinned     = 39,
    /// "Linked symbol (bound to chart)"
    SlotSourceLinked     = 40,
    /// "Split side by side"
    SlotSplitH           = 41,
    /// "Split top and bottom"
    SlotSplitV           = 42,
    /// "Replace focused panel"
    SlotSplitReplace     = 43,
    /// "Expand / collapse focused panel"
    SlotExpandToggle     = 44,
    /// "Reset panel sizes"
    SlotResetSizes       = 45,

    // ---- Connector detail: draw_detail / draw_section labels ----
    /// "Auth:"
    AuthLabel            = 46,
    /// "Free tier:"
    FreeTierLabel        = 47,
    /// "Rate limits:"
    RateLimitsLabel      = 48,
    /// "Data Capabilities"
    DataCapabilities     = 49,
    /// "Kline Config"
    KlineConfig          = 50,
}

impl SidebarKey {
    pub const COUNT: usize = 51;

    /// Get translation for this key.
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&SIDEBAR_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for SidebarKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&SIDEBAR_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Primitive Name Keys  (drawing toolbar / object tree / context menu names)
// =============================================================================

/// Localized display name keys for the 84 built-in drawing primitives.
///
/// Variant order is **frozen** — discriminant == row index in `PRIMITIVE_NAME_TABLE`.
/// Map a registry `type_id` string → this key via `PrimitiveNameKey::from_type_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum PrimitiveNameKey {
    // Lines (0-8)
    TrendLine           = 0,
    HorizontalLine      = 1,
    VerticalLine        = 2,
    Ray                 = 3,
    ExtendedLine        = 4,
    InfoLine            = 5,
    TrendAngle          = 6,
    HorizontalRay       = 7,
    CrossLine           = 8,
    // Channels (9-12)
    ParallelChannel     = 9,
    RegressionTrend     = 10,
    FlatTopBottom       = 11,
    DisjointChannel     = 12,
    // Shapes (13-22)
    Rectangle           = 13,
    Circle              = 14,
    Ellipse             = 15,
    Triangle            = 16,
    Arc                 = 17,
    Polyline            = 18,
    Path                = 19,
    RotatedRectangle    = 20,
    Curve               = 21,
    DoubleCurve         = 22,
    // Fibonacci (23-33)
    FibRetracement      = 23,
    FibExtension        = 24,
    FibChannel          = 25,
    FibTimeZones        = 26,
    FibSpeedResistance  = 27,
    FibTrendTime        = 28,
    FibCircles          = 29,
    FibSpiral           = 30,
    FibArcs             = 31,
    FibWedge            = 32,
    FibFan              = 33,
    // Pitchforks (34-37)
    Pitchfork           = 34,
    SchiffPitchfork     = 35,
    ModifiedSchiff      = 36,
    InsidePitchfork     = 37,
    // Gann (38-41)
    GannBox             = 38,
    GannSquareFixed     = 39,
    GannSquare          = 40,
    GannFan             = 41,
    // Arrows (42)
    ArrowLine           = 42,
    // Annotations (43-55)
    Text                = 43,
    AnchoredText        = 44,
    Note                = 45,
    PriceNote           = 46,
    Signpost            = 47,
    Callout             = 48,
    Comment             = 49,
    PriceLabel          = 50,
    Sign                = 51,
    Flag                = 52,
    Table               = 53,
    TriangleUp          = 54,
    TriangleDown        = 55,
    // Patterns (56-61)
    XabcdPattern        = 56,
    CypherPattern       = 57,
    HeadShoulders       = 58,
    AbcdPattern         = 59,
    TrianglePattern     = 60,
    ThreeDrives         = 61,
    // Elliott (62-66)
    ElliottImpulse      = 62,
    ElliottCorrection   = 63,
    ElliottTriangle     = 64,
    ElliottDoubleCombo  = 65,
    ElliottTripleCombo  = 66,
    // Cycles (67-69)
    CycleLines          = 67,
    TimeCycles          = 68,
    SineWave            = 69,
    // Projection (70-74)
    LongPosition        = 70,
    ShortPosition       = 71,
    BarsPattern         = 72,
    PriceProjection     = 73,
    Projection          = 74,
    // Volume (75-76)
    FixedVolumeProfile  = 75,
    AnchoredVolumeProfile = 76,
    // Measurement (77-79)
    PriceRange          = 77,
    DateRange           = 78,
    PriceDateRange      = 79,
    // Brushes (80-81)
    Brush               = 80,
    Highlighter         = 81,
    // Icons (82-83)
    Image               = 82,
    Sticker             = 83,
}

impl PrimitiveNameKey {
    pub const COUNT: usize = 84;

    /// Get translation for this key.
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&PRIMITIVE_NAME_TABLE[self as usize], lang as usize)
    }

    /// Map a registry `type_id` string to the corresponding key.
    /// Returns `None` for unknown or emoji_* variant type_ids.
    pub fn from_type_id(type_id: &str) -> Option<Self> {
        match type_id {
            "trend_line"               => Some(Self::TrendLine),
            "horizontal_line"          => Some(Self::HorizontalLine),
            "vertical_line"            => Some(Self::VerticalLine),
            "ray"                      => Some(Self::Ray),
            "extended_line"            => Some(Self::ExtendedLine),
            "info_line"                => Some(Self::InfoLine),
            "trend_angle"              => Some(Self::TrendAngle),
            "horizontal_ray"           => Some(Self::HorizontalRay),
            "cross_line"               => Some(Self::CrossLine),
            "parallel_channel"         => Some(Self::ParallelChannel),
            "regression_trend"         => Some(Self::RegressionTrend),
            "flat_top_bottom"          => Some(Self::FlatTopBottom),
            "disjoint_channel"         => Some(Self::DisjointChannel),
            "rectangle"                => Some(Self::Rectangle),
            "circle"                   => Some(Self::Circle),
            "ellipse"                  => Some(Self::Ellipse),
            "triangle"                 => Some(Self::Triangle),
            "arc"                      => Some(Self::Arc),
            "polyline"                 => Some(Self::Polyline),
            "path"                     => Some(Self::Path),
            "rotated_rectangle"        => Some(Self::RotatedRectangle),
            "curve"                    => Some(Self::Curve),
            "double_curve"             => Some(Self::DoubleCurve),
            "fib_retracement"          => Some(Self::FibRetracement),
            "fib_trend_extension"      => Some(Self::FibExtension),
            "fib_channel"              => Some(Self::FibChannel),
            "fib_time_zones"           => Some(Self::FibTimeZones),
            "fib_speed_resistance"     => Some(Self::FibSpeedResistance),
            "fib_trend_time"           => Some(Self::FibTrendTime),
            "fib_circles"              => Some(Self::FibCircles),
            "fib_spiral"               => Some(Self::FibSpiral),
            "fib_arcs"                 => Some(Self::FibArcs),
            "fib_wedge"                => Some(Self::FibWedge),
            "fib_fan"                  => Some(Self::FibFan),
            "pitchfork"                => Some(Self::Pitchfork),
            "schiff_pitchfork"         => Some(Self::SchiffPitchfork),
            "modified_schiff"          => Some(Self::ModifiedSchiff),
            "inside_pitchfork"         => Some(Self::InsidePitchfork),
            "gann_box"                 => Some(Self::GannBox),
            "gann_square_fixed"        => Some(Self::GannSquareFixed),
            "gann_square"              => Some(Self::GannSquare),
            "gann_fan"                 => Some(Self::GannFan),
            "arrow_line"               => Some(Self::ArrowLine),
            "text"                     => Some(Self::Text),
            "anchored_text"            => Some(Self::AnchoredText),
            "note"                     => Some(Self::Note),
            "price_note"               => Some(Self::PriceNote),
            "signpost"                 => Some(Self::Signpost),
            "callout"                  => Some(Self::Callout),
            "comment"                  => Some(Self::Comment),
            "price_label"              => Some(Self::PriceLabel),
            "sign"                     => Some(Self::Sign),
            "flag"                     => Some(Self::Flag),
            "table"                    => Some(Self::Table),
            "triangle_up"              => Some(Self::TriangleUp),
            "triangle_down"            => Some(Self::TriangleDown),
            "xabcd_pattern"            => Some(Self::XabcdPattern),
            "cypher_pattern"           => Some(Self::CypherPattern),
            "head_shoulders"           => Some(Self::HeadShoulders),
            "abcd_pattern"             => Some(Self::AbcdPattern),
            "triangle_pattern"         => Some(Self::TrianglePattern),
            "three_drives"             => Some(Self::ThreeDrives),
            "elliott_impulse"          => Some(Self::ElliottImpulse),
            "elliott_correction"       => Some(Self::ElliottCorrection),
            "elliott_triangle"         => Some(Self::ElliottTriangle),
            "elliott_double_combo"     => Some(Self::ElliottDoubleCombo),
            "elliott_triple_combo"     => Some(Self::ElliottTripleCombo),
            "cycle_lines"              => Some(Self::CycleLines),
            "time_cycles"              => Some(Self::TimeCycles),
            "sine_wave"                => Some(Self::SineWave),
            "long_position"            => Some(Self::LongPosition),
            "short_position"           => Some(Self::ShortPosition),
            "bars_pattern"             => Some(Self::BarsPattern),
            "price_projection"         => Some(Self::PriceProjection),
            "projection"               => Some(Self::Projection),
            "fixed_volume_profile"     => Some(Self::FixedVolumeProfile),
            "anchored_volume_profile"  => Some(Self::AnchoredVolumeProfile),
            "price_range"              => Some(Self::PriceRange),
            "date_range"               => Some(Self::DateRange),
            "price_date_range"         => Some(Self::PriceDateRange),
            "brush"                    => Some(Self::Brush),
            "highlighter"              => Some(Self::Highlighter),
            "image"                    => Some(Self::Image),
            "emoji"                    => Some(Self::Sticker),
            _                          => None,
        }
    }
}

impl uzor::i18n::Translate for PrimitiveNameKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&PRIMITIVE_NAME_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Primitive Tooltip Keys  (drawing toolbar hover tooltips)
// =============================================================================

/// Localized tooltip keys for the 84 built-in drawing primitives.
///
/// Variant order is **frozen** — discriminant == row index in `PRIMITIVE_TOOLTIP_TABLE`.
/// Map a registry `type_id` string → this key via `PrimitiveTooltipKey::from_type_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum PrimitiveTooltipKey {
    // Lines (0-8)
    TrendLine           = 0,
    HorizontalLine      = 1,
    VerticalLine        = 2,
    Ray                 = 3,
    ExtendedLine        = 4,
    InfoLine            = 5,
    TrendAngle          = 6,
    HorizontalRay       = 7,
    CrossLine           = 8,
    // Channels (9-12)
    ParallelChannel     = 9,
    RegressionTrend     = 10,
    FlatTopBottom       = 11,
    DisjointChannel     = 12,
    // Shapes (13-22)
    Rectangle           = 13,
    Circle              = 14,
    Ellipse             = 15,
    Triangle            = 16,
    Arc                 = 17,
    Polyline            = 18,
    Path                = 19,
    RotatedRectangle    = 20,
    Curve               = 21,
    DoubleCurve         = 22,
    // Fibonacci (23-33)
    FibRetracement      = 23,
    FibExtension        = 24,
    FibChannel          = 25,
    FibTimeZones        = 26,
    FibSpeedResistance  = 27,
    FibTrendTime        = 28,
    FibCircles          = 29,
    FibSpiral           = 30,
    FibArcs             = 31,
    FibWedge            = 32,
    FibFan              = 33,
    // Pitchforks (34-37)
    Pitchfork           = 34,
    SchiffPitchfork     = 35,
    ModifiedSchiff      = 36,
    InsidePitchfork     = 37,
    // Gann (38-41)
    GannBox             = 38,
    GannSquareFixed     = 39,
    GannSquare          = 40,
    GannFan             = 41,
    // Arrows (42)
    ArrowLine           = 42,
    // Annotations (43-55)
    Text                = 43,
    AnchoredText        = 44,
    Note                = 45,
    PriceNote           = 46,
    Signpost            = 47,
    Callout             = 48,
    Comment             = 49,
    PriceLabel          = 50,
    Sign                = 51,
    Flag                = 52,
    Table               = 53,
    TriangleUp          = 54,
    TriangleDown        = 55,
    // Patterns (56-61)
    XabcdPattern        = 56,
    CypherPattern       = 57,
    HeadShoulders       = 58,
    AbcdPattern         = 59,
    TrianglePattern     = 60,
    ThreeDrives         = 61,
    // Elliott (62-66)
    ElliottImpulse      = 62,
    ElliottCorrection   = 63,
    ElliottTriangle     = 64,
    ElliottDoubleCombo  = 65,
    ElliottTripleCombo  = 66,
    // Cycles (67-69)
    CycleLines          = 67,
    TimeCycles          = 68,
    SineWave            = 69,
    // Projection (70-74)
    LongPosition        = 70,
    ShortPosition       = 71,
    BarsPattern         = 72,
    PriceProjection     = 73,
    Projection          = 74,
    // Volume (75-76)
    FixedVolumeProfile  = 75,
    AnchoredVolumeProfile = 76,
    // Measurement (77-79)
    PriceRange          = 77,
    DateRange           = 78,
    PriceDateRange      = 79,
    // Brushes (80-81)
    Brush               = 80,
    Highlighter         = 81,
    // Icons (82-83)
    Image               = 82,
    Sticker             = 83,
}

impl PrimitiveTooltipKey {
    pub const COUNT: usize = 84;

    /// Get translation for this key.
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&PRIMITIVE_TOOLTIP_TABLE[self as usize], lang as usize)
    }

    /// Map a registry `type_id` string to the corresponding tooltip key.
    pub fn from_type_id(type_id: &str) -> Option<Self> {
        match type_id {
            "trend_line"               => Some(Self::TrendLine),
            "horizontal_line"          => Some(Self::HorizontalLine),
            "vertical_line"            => Some(Self::VerticalLine),
            "ray"                      => Some(Self::Ray),
            "extended_line"            => Some(Self::ExtendedLine),
            "info_line"                => Some(Self::InfoLine),
            "trend_angle"              => Some(Self::TrendAngle),
            "horizontal_ray"           => Some(Self::HorizontalRay),
            "cross_line"               => Some(Self::CrossLine),
            "parallel_channel"         => Some(Self::ParallelChannel),
            "regression_trend"         => Some(Self::RegressionTrend),
            "flat_top_bottom"          => Some(Self::FlatTopBottom),
            "disjoint_channel"         => Some(Self::DisjointChannel),
            "rectangle"                => Some(Self::Rectangle),
            "circle"                   => Some(Self::Circle),
            "ellipse"                  => Some(Self::Ellipse),
            "triangle"                 => Some(Self::Triangle),
            "arc"                      => Some(Self::Arc),
            "polyline"                 => Some(Self::Polyline),
            "path"                     => Some(Self::Path),
            "rotated_rectangle"        => Some(Self::RotatedRectangle),
            "curve"                    => Some(Self::Curve),
            "double_curve"             => Some(Self::DoubleCurve),
            "fib_retracement"          => Some(Self::FibRetracement),
            "fib_trend_extension"      => Some(Self::FibExtension),
            "fib_channel"              => Some(Self::FibChannel),
            "fib_time_zones"           => Some(Self::FibTimeZones),
            "fib_speed_resistance"     => Some(Self::FibSpeedResistance),
            "fib_trend_time"           => Some(Self::FibTrendTime),
            "fib_circles"              => Some(Self::FibCircles),
            "fib_spiral"               => Some(Self::FibSpiral),
            "fib_arcs"                 => Some(Self::FibArcs),
            "fib_wedge"                => Some(Self::FibWedge),
            "fib_fan"                  => Some(Self::FibFan),
            "pitchfork"                => Some(Self::Pitchfork),
            "schiff_pitchfork"         => Some(Self::SchiffPitchfork),
            "modified_schiff"          => Some(Self::ModifiedSchiff),
            "inside_pitchfork"         => Some(Self::InsidePitchfork),
            "gann_box"                 => Some(Self::GannBox),
            "gann_square_fixed"        => Some(Self::GannSquareFixed),
            "gann_square"              => Some(Self::GannSquare),
            "gann_fan"                 => Some(Self::GannFan),
            "arrow_line"               => Some(Self::ArrowLine),
            "text"                     => Some(Self::Text),
            "anchored_text"            => Some(Self::AnchoredText),
            "note"                     => Some(Self::Note),
            "price_note"               => Some(Self::PriceNote),
            "signpost"                 => Some(Self::Signpost),
            "callout"                  => Some(Self::Callout),
            "comment"                  => Some(Self::Comment),
            "price_label"              => Some(Self::PriceLabel),
            "sign"                     => Some(Self::Sign),
            "flag"                     => Some(Self::Flag),
            "table"                    => Some(Self::Table),
            "triangle_up"              => Some(Self::TriangleUp),
            "triangle_down"            => Some(Self::TriangleDown),
            "xabcd_pattern"            => Some(Self::XabcdPattern),
            "cypher_pattern"           => Some(Self::CypherPattern),
            "head_shoulders"           => Some(Self::HeadShoulders),
            "abcd_pattern"             => Some(Self::AbcdPattern),
            "triangle_pattern"         => Some(Self::TrianglePattern),
            "three_drives"             => Some(Self::ThreeDrives),
            "elliott_impulse"          => Some(Self::ElliottImpulse),
            "elliott_correction"       => Some(Self::ElliottCorrection),
            "elliott_triangle"         => Some(Self::ElliottTriangle),
            "elliott_double_combo"     => Some(Self::ElliottDoubleCombo),
            "elliott_triple_combo"     => Some(Self::ElliottTripleCombo),
            "cycle_lines"              => Some(Self::CycleLines),
            "time_cycles"              => Some(Self::TimeCycles),
            "sine_wave"                => Some(Self::SineWave),
            "long_position"            => Some(Self::LongPosition),
            "short_position"           => Some(Self::ShortPosition),
            "bars_pattern"             => Some(Self::BarsPattern),
            "price_projection"         => Some(Self::PriceProjection),
            "projection"               => Some(Self::Projection),
            "fixed_volume_profile"     => Some(Self::FixedVolumeProfile),
            "anchored_volume_profile"  => Some(Self::AnchoredVolumeProfile),
            "price_range"              => Some(Self::PriceRange),
            "date_range"               => Some(Self::DateRange),
            "price_date_range"         => Some(Self::PriceDateRange),
            "brush"                    => Some(Self::Brush),
            "highlighter"              => Some(Self::Highlighter),
            "image"                    => Some(Self::Image),
            "emoji"                    => Some(Self::Sticker),
            _                          => None,
        }
    }
}

impl uzor::i18n::Translate for PrimitiveTooltipKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&PRIMITIVE_TOOLTIP_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Trading Panel Keys  (DOM, tape, order entry, position manager, etc.)
// =============================================================================

/// Column header, section title, and button label keys for trading panels.
///
/// Variant order is **frozen** — discriminant == row index in `TRADING_KEY_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum TradingKey {
    // DOM column headers
    Bid              = 0,
    Ask              = 1,
    Price            = 2,
    Buy              = 3,
    Sell             = 4,

    // L2 tape column headers
    Time             = 5,
    Type             = 6,
    Side             = 7,
    Qty              = 8,
    Paused           = 9,

    // Volume profile labels
    Poc              = 10,
    Mkt              = 11,
    Vah              = 12,
    Val              = 13,

    // Order entry
    OrderEntry       = 14,

    // Trade tape
    NoTrades         = 15,

    // Position manager column headers
    Symbol           = 16,
    Entry            = 17,
    Mark             = 18,
    Pnl              = 19,
    Liq              = 20,
    Lev              = 21,
    NoOpenPositions  = 22,
    TotalPnl         = 23,

    // Risk calculator
    RiskCalculator   = 24,

    // Trade log column headers
    Fee              = 25,
    NoTradesLog      = 26,
    TotalPnlLog      = 27,
}

impl TradingKey {
    pub const COUNT: usize = 28;

    /// Get translation for this key.
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&TRADING_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for TradingKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&TRADING_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Toolbar Menu Keys  (dropdown item labels in chart toolbars)
// =============================================================================

/// Localized labels for dropdown items in chart toolbar menus.
///
/// Variant order is **frozen** — discriminant == row index in `TOOLBAR_MENU_KEY_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum ToolbarMenuKey {
    // Chart type names
    Candles            = 0,
    HollowCandles      = 1,
    HeikinAshi         = 2,
    Bars               = 3,
    Line               = 4,
    StepLine           = 5,
    LineWithMarkers    = 6,
    Area               = 7,
    HlcArea            = 8,
    Baseline           = 9,
    Histogram          = 10,
    Columns            = 11,

    // Panel management
    ClosePanel         = 12,
    ResetSizes         = 13,
    SplitWithoutGroup  = 14,

    // Sync options
    SyncSymbol         = 15,
    SyncTimeframe      = 16,
    SyncCrosshair      = 17,
    SyncViewport       = 18,
    SyncDrawings       = 19,
    SyncIndicators     = 20,

    // Drawing tool section headers
    HeaderLines        = 21,
    HeaderChannels     = 22,
    HeaderPitchforks   = 23,
    HeaderFibonacci    = 24,
    HeaderGann         = 25,
    HeaderPatterns     = 26,
    HeaderElliottWaves = 27,
    HeaderCycles       = 28,
    HeaderBrushes      = 29,
    HeaderShapes       = 30,
    HeaderPositions    = 31,
    HeaderForecast     = 32,
    HeaderVolume       = 33,
    HeaderMeasurement  = 34,
    HeaderArrows       = 35,
    HeaderSignals      = 36,
    HeaderMarkers      = 37,
    HeaderEmotions     = 38,
    HeaderEmoji        = 39,

    // Cursor tools
    Pan                = 40,

    // Delete tools
    DeleteSelected     = 41,
    DeleteAll          = 42,

    // Settings menu items
    ChartSettings      = 43,
    ToggleGrid         = 44,
    VerticalLines      = 45,
    HorizontalLines    = 46,
    ToggleCrosshair    = 47,
    NormalMode         = 48,
    MagnetClose        = 49,
    MagnetOhlc         = 50,
    ToggleTooltip      = 51,
    FollowCursor       = 52,
    ToggleWatermark    = 53,
    WatermarkSeeyou    = 54,
    WatermarkDemo      = 55,
    WatermarkPaper     = 56,
    WatermarkLive      = 57,
    WatermarkCenter    = 58,
    WatermarkBl        = 59,
    WatermarkBr        = 60,
    ThemeDark          = 61,
    ThemeLight         = 62,
    ThemeHighContrast  = 63,
    ThemeHcMono        = 64,
    ThemeWizardHat     = 65,
    StyleSolid         = 66,
    StyleGlass         = 67,
    StyleFrostedGlass  = 68,

    // Settings submenus
    SubGrid            = 69,
    SubCrosshair       = 70,
    SubTooltip         = 71,
    SubWatermark       = 72,
    SubTheme           = 73,
    SubUiStyle         = 74,
}

impl ToolbarMenuKey {
    pub const COUNT: usize = 75;

    /// Get translation for this key.
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&TOOLBAR_MENU_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for ToolbarMenuKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&TOOLBAR_MENU_KEY_TABLE[self as usize], lang_index)
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
