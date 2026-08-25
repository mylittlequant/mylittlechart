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
    CITY_KEY_TABLE,
    HEAD_SHOULDERS_KEY_TABLE,
    CANDLE_PATTERN_KEY_TABLE,
    POSITION_POINT_KEY_TABLE,
    SHELL_KEY_TABLE,
    CONCEPT_KEY_TABLE,
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
    Lock            = 13,
    Unlock          = 14,
    Show            = 15,
    HideItem        = 16,
    ResetZoom       = 17,
    Screenshot      = 18,
    SymbolSearch    = 19,
    ResetCache      = 20,
    ResetStorage    = 21,
}

impl MenuKey {
    pub const COUNT: usize = 22;

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

    // Fill (closed shapes: rectangle, circle, ellipse, triangle, channels, …)
    FillOpacity     = 76,

    // Path smoothing (Catmull-Rom-style quadratic curve through points)
    Smooth          = 77,

    // Label backing plate (wave/pattern point labels + measurement rulers +
    // trading position price labels — label-plumbing pass)
    LabelBackgroundColor = 78,
    LabelPadding         = 79,
    LabelRadius          = 80,
    LabelBorder          = 81,
    LabelBorderColor     = 82,
    LabelBorderWidth     = 83,
    LabelOffset          = 84,

    // Trading position tools (long/short position)
    StopZoneColor   = 85,
    TargetZoneColor = 86,
    Quantity        = 87,
    ShowPnl         = 88,

    // Measurement rulers (price/date/price-date range)
    ShowPriceDiff   = 89,
    ShowBarCount    = 90,
    ShowDuration    = 91,

    // Fibonacci spiral geometry (rotations/direction/mirroring — previously
    // wired into the geometry but unreachable from any UI control)
    Rotations       = 92,
    Clockwise       = 93,
    FlipHorizontal  = 94,
    FlipVertical    = 95,

    // Volume profile / anchored VWAP (rows, POC, value area, deviation bands
    // — struct fields that drove rendering but had no style_properties()
    // override anywhere, so none of them was ever reachable from the UI)
    ShowPoc         = 96,
    ShowValueArea   = 97,
    ShowStdDev1     = 98,
    ShowStdDev2     = 99,
    SigmaMultiplier1 = 100,
    SigmaMultiplier2 = 101,

    // Cycle tools (cycle_lines/sine_wave/time_cycles repeat count, sine
    // amplitude and period count)
    Count           = 102,
    Amplitude       = 103,
    WaveCycles      = 104,

    // Bars Pattern projection geometry (mirror + price offset — real fields
    // read by render(), unreachable from any UI control)
    Mirror          = 105,
    PriceOffset     = 106,

    // Arrow Line arrowhead configuration
    ArrowStart      = 107,
    ArrowEnd        = 108,
    ArrowSize       = 109,

    // Time-axis companion to ShowPrice (vertical_line / cross_line)
    ShowTimeLabel   = 110,

    // Trend Angle's angle-arc toggle
    ShowArc         = 111,

    // primitive_settings modal Style tab base row labels — previously
    // hardcoded Russian literals that never localized
    LineColorLabel  = 112,
    WidthLabel      = 113,
    FillColorLabel  = 114,

    // primitive_settings modal Text tab — the two base row labels with no
    // existing equivalent (Content/FontSize/TextColor above already cover
    // the other three rows verbatim, so those are reused rather than
    // duplicated here).
    TextStyleLabel   = 115,
    PositionLabel    = 116,

    // Shared "(empty)" placeholder for an unset text field — previously two
    // separate hardcoded "(пусто)" literals at two call sites in the same
    // modal, now one key.
    EmptyPlaceholder = 117,

    // primitive_settings modal Coordinates tab — per-point row label
    // ("Point N") used when the primitive has no named point labels.
    PointLabel       = 118,

    // Long/Short Position live-price label toggle (the shading layer and
    // marker line always render; this gates the text label only).
    ShowLiveLabel    = 119,

    // Free-form shape closure (Polyline/Path): last point joins back to the
    // first, turning an open segment chain into a closed shape. Precondition
    // for a fill to paint anything — `render` gates fill on `closed && fill`.
    Closed           = 120,

    // ---- primitive_settings modal — approved tab design (Appearance /
    // Levels / Placement replacing Style/Text/Coordinates/Levels/Visibility,
    // 2026-08-08 port; Labels split out of Appearance into its own tab the
    // same day — see `TabVid`'s rename to `TabAppearance` below, its
    // discriminant/table row is unchanged, only the identifier followed the
    // tab's own rename) ----
    TabAppearance                = 121,
    TabPlacement                = 122,
    GroupLine                   = 123,
    LabelsGroupHint              = 124,
    BoldAbbrev                  = 125,
    ItalicAbbrev                 = 126,
    SearchSettingsPlaceholder    = 127,
    GlobalFillBetween            = 128,
    EmptyLadder                  = 129,
    AddLevel                     = 130,
    UpToLevel                    = 131,
    GroupPoints                  = 132,
    TimeModeTime                 = 133,
    TimeModeBarIndex             = 134,
    NotAvailable                 = 135,
    GroupVisibleOnTimeframes     = 136,

    // primitive_settings modal — Levels tab "Набор" (preset) selector +
    // palette cards, restored 2026-08-08. `LevelPillLabel` is the pill's
    // own compact word ("уровень") — `Levels` (id=1) stays the tab name
    // ("Уровни") and cannot be reused here.
    LevelPillLabel               = 137,
    SetLabel                     = 138,
    LevelSetStandard             = 139,
    LevelSetExtended             = 140,
    LevelSetKeyOnly              = 141,
    LevelSetCustom               = 142,
    // The Template footer menu's set-and-templates extension (2026-08-08
    // review): the default entry that seeds the ladder with the full
    // many-level catalogue (`ALL_LEVELS` in
    // `mlc-core::drawing::primitives_v2::fibonacci::retracement`).
    LevelSetAll                  = 143,

    // Levels tab palette card names (`primitive_settings_palettes::PALETTES`)
    // — previously bare Russian literals on the `Palette` struct itself,
    // never localized (i18n audit, 2026-08-08 review).
    PaletteClassic                = 144,
    PaletteWarm                   = 145,
    PaletteCold                   = 146,
    PaletteMono                   = 147,

    // Levels tab top section (2026-08-08 appearance-surface consolidation)
    // — the Моно/Палитра colour-mode segmented control that replaces the
    // per-primitive LINE/FILL split for a level-capable primitive.
    ColorModeMono                 = 148,
    ColorModePalette              = 149,

    // `LabelStyle::style_properties` — the "inherit the line's own colour"
    // toggle (2026-08-09 label-colour-inherit fix): `LabelStyle::color ==
    // None` means "no override, use the per-level/per-primitive stroke
    // colour"; this checkbox is the only UI path back to `None` once a
    // colour has been chosen — see `LabelStyle::apply_property`'s own doc.
    LabelColorInherit             = 150,

    // Elliott Correction's own `CorrectionType` (Zigzag/Flat/Expanded/
    // Running) — a real field with zero `ConfigProperty` representation
    // until 2026-08-09 (modal-surface-inventory audit). Own keys rather
    // than reusing `Expanded`/`Direction*` above: those already belong to
    // an unrelated UI context (bubble/callout expand-toggle, signpost
    // direction) and a shared key would let an edit to one silently retext
    // the other.
    CorrectionType                = 151,
    CorrectionZigzag              = 152,
    CorrectionFlat                = 153,
    CorrectionExpanded            = 154,
    CorrectionRunning             = 155,

    // Freehand brush/highlighter stroke thickness (`brush_size` — distinct
    // from the base `stroke_width` row: it also drives hit-test tolerance).
    // Was a bare hardcoded "Brush Size" literal on `Highlighter` only; moved
    // into this table 2026-08-09 alongside giving `Brush` the same control.
    BrushSize                     = 156,

    // Position's own trailing Appearance group (2026-08-09 modal-surface
    // pass): a read-only computed Risk/Target/R:R block —
    // `PropertyCategory::AppearanceStats` — below the editable rows on
    // `long_position`/`short_position`. `GroupResults` headers that group
    // the same way `GroupLine`/`Fill` head the Line/Fill groups above it.
    GroupResults                  = 157,
    PositionRiskLabel             = 158,
    PositionTargetLabel           = 159,
    PositionRiskRewardLabel       = 160,

    // Table's own derived Cells tab (2026-08-09 modal-surface pass) — a
    // live editable grid replacing the flat "Cell [r,c]" list the generic
    // Labels-tab renderer used to show for `cell_{row}_{col}` properties
    // (removed — see `Table::text_properties`'s own doc).
    TabCells                      = 161,

    // Elliott fibonacci-EXTENSION projection toggle (`elliott::projection`,
    // 2026-08-14) — the per-structure counterpart to `ShowRatios` above;
    // same settings-modal row family, own key because it labels a distinct
    // property (`show_projections`, not `show_ratios`).
    ShowProjections               = 162,

    // Elliott per-structure truncated-fifth escape hatch
    // (`ElliottImpulse::truncated_fifth`, owner ruling 2026-08-15) — same
    // settings-modal row family as `ShowProjections` right above it, own key
    // because it labels a distinct property.
    TruncatedFifth                = 163,

    // Elliott Impulse's own `ImpulseKind` select (the diagonal arc,
    // 2026-08-24) — `mli::ewa::grammar` carried `LEADING_DIAGONAL_SLOTS`/
    // `ENDING_DIAGONAL_SLOTS` for as long as the plain rule table existed,
    // with no control on the chart to say a drawn wave IS one. Mirrors
    // `CorrectionType`/`CorrectionZigzag`'s own group shape exactly, for
    // the OTHER Elliott type that carries a real shape-defining subtype.
    ImpulseKind                   = 164,
    ImpulseKindPlain               = 165,
    ImpulseKindLeadingDiagonal     = 166,
    ImpulseKindEndingDiagonal      = 167,
}

impl ConfigKey {
    pub const COUNT: usize = 168;

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
    // Line-dash style names for the primitive settings modal's Style tab
    // (line-style dual-button label + dropdown — previously hardcoded
    // Russian literals in both places).
    Solid       = 8,
    LargeDashed = 9,
    SparseDotted = 10,
}

impl StyleKey {
    pub const COUNT: usize = 11;

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
    PriceActionTool   = 10,
    BrushTool         = 11,
    AnnotationTool    = 12,
    IconTool          = 13,
    ProjectionTool    = 14,
    Lock              = 15,
    Eye               = 16,
    DeleteTool        = 17,

    // Actions (top toolbar)
    Undo              = 18,
    Redo              = 19,
    MagnetMode        = 20,
    StayInDrawingMode = 21,
    Snapshot          = 22,
    Bookmark          = 23,
    MeasureTool       = 24,
    Indicators        = 25,
    Settings          = 26,
    Compare           = 27,
    SymbolSelector    = 28,
    TimeframeSelector = 29,
    ChartType         = 30,
    Layout            = 31,
    Presets           = 32,
    Screenshot        = 33,
    Expand            = 34,
    MainMenu          = 35,

    // Right toolbar (sidebar panels)
    Watchlist         = 36,
    Alerts            = 37,
    ObjectTree        = 38,
    Templates         = 39,
    Signals           = 40,
    /// Merged Connectors + Performance panel (2026-07-18). Replaces the two
    /// former separate keys `Connectors`/`Performance` — the standalone
    /// `Connectors` key is gone (its old row was reused).
    Monitoring        = 41,
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
    TabInstrument               = 108,
    TabStatusLine               = 109,
    TabScalesLines              = 110,
    TabAppearance               = 111,

    // ---- Compare Settings modal (symbol-search-compare-inner arc, Wave 3
    // — owner spec 2026-07-16) ----
    // Tab names
    CompareTabStyle              = 112,
    CompareTabVisibility         = 113,
    CompareTabInfo                = 114,

    // Info tab row labels
    CompareInfoSymbol             = 115,
    CompareInfoName                = 116,
    CompareInfoBars                = 117,
    CompareInfoBase                = 118,

    // Visibility tab — timeframe-family row labels
    TfTicks                        = 119,
    TfSeconds                      = 120,
    TfMinutes                      = 121,
    TfHours                        = 122,
    TfDays                         = 123,
    TfWeeks                        = 124,
    TfMonths                       = 125,

    // ---- chart_settings modal — default template name prefix (was a
    // hardcoded Russian literal duplicated at 4 call sites across
    // mlc-app/src/input/{settings,overlay}.rs; one key now) ----
    DefaultTemplateNamePrefix      = 126,

    // ---- chart_settings modal ScalesLines tab — date-format dropdown
    // examples. The day/month/year letters are localized per language's own
    // convention (e.g. DD/TT/JJ/GG for day); "MMM" is kept as a deliberate
    // cross-locale token for "month abbreviation" (spreadsheet format-code
    // convention) rather than translated. The separator (., /, -, space)
    // and the concrete example digits are NOT localizable — they mirror the
    // literal `DateFormat` variants in `chart/types/time_scale.rs` exactly. ----
    DateFormatDayMonthYear         = 127,
    DateFormatMonthDayYear         = 128,
    DateFormatYearMonthDay         = 129,
    DateFormatDayMonthShort        = 130,

    // ---- primitive_settings modal — Placement tab "Виден на таймфреймах"
    // chip row (2026-08-08 3-tab port): the one class TfTicks..TfMonths
    // didn't cover.
    TfRanges                       = 131,
}

impl SettingsKey {
    pub const COUNT: usize = 132;

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

    // General tab — workspace toggle (workspace-panel arc, Stage 4)
    TradingPanelTopbarTakeover = 25,

    // General tab — "Control points" group (handle shape/size, 2026-08-08)
    SectionControlPoints           = 26,
    ControlPointShapeLabel         = 27,
    ControlPointSizeLabel          = 28,
    ControlPointShapeCircle        = 29,
    ControlPointShapeRoundedSquare = 30,
    ControlPointShapeSquare        = 31,
    ControlPointSizeSmall          = 32,
    ControlPointSizeMedium         = 33,
    ControlPointSizeLarge          = 34,
}

impl UserSettingsKey {
    pub const COUNT: usize = 35;

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

    // ShowRecoveryKey page — subtitle + buttons (never localized before;
    // four hardcoded Russian literals in `profile_manager.rs`)
    RecoveryKeyWriteDownWarning  = 24,
    RecoveryKeyPasswordLossNote  = 25,
    BtnCopyKey                   = 26,
    BtnRecoveryConfirmed         = 27,
}

impl ProfileKey {
    pub const COUNT: usize = 28;

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

    // ---- Search Overlay header toggles (symbol-search-compare-inner arc,
    // Wave 3 — owner spec 2026-07-16; 2026-07-17 rework: the third
    // "Inline input" toggle is deleted — the button-hosted editor IS
    // quick search) ----
    /// "Quick search" toggle label
    QuickSearchToggle      = 73,
    /// "Separate compare button" toggle label
    SeparateCompareToggle  = 74,
    /// "COMPARE" column header over the search row's trailing + — without it
    /// the icon reads as a second way to pick the symbol.
    ColCompare             = 75,
}

impl ModalKey {
    pub const COUNT: usize = 75;

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
    /// "Parameters" (Inputs tab label in the indicator settings modal —
    /// previously hardcoded Russian "Параметры" that never localized)
    TabInputs              = 27,
    /// "Outputs:" (Info tab row label, prefixed to the output count —
    /// previously a hardcoded Russian "Выходы: {}" format string)
    Outputs                = 28,
}

impl IndicatorKey {
    pub const COUNT: usize = 29;

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
    /// "T" — Tagged mode button (join active group)
    SourceTaggedBtn      = 12,
    /// "U" — Untagged mode button (solo group)
    SourceUntaggedBtn    = 13,
    /// Reserved (was SourceLinkedBtn) — kept for table index stability.
    _SourceLinkedReserved = 14,

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
    /// "Tagged — joins active chart group"
    SlotSourceTagged     = 38,
    /// "Untagged — own solo group (snapshot)"
    SlotSourceUntagged   = 39,
    /// Reserved (was SlotSourceLinked) — kept for table index stability.
    _SlotSourceLinkedReserved = 40,
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
    // Volume (VWAP) — appended after the initial 0-83 set rather than
    // inserted next to `FixedVolumeProfile`/`AnchoredVolumeProfile`
    // (75-76): variant order is FROZEN (discriminant == table row index),
    // so a genuinely new pair of primitives gets new discriminants at the
    // END, never a renumbering of the 84 that already ship (84-85).
    AnchoredVwap        = 84,
    FixedVwap           = 85,
    // Markup engines — candle patterns, smart money, AMD, order flow.
    // Appended at the END for the same reason the VWAP pair was: the
    // discriminant IS the table row index, so it is frozen (86-98).
    CandlePattern       = 86,
    CandlePatternScan   = 87,
    OrderBlock          = 88,
    FairValueGap        = 89,
    LiquiditySweep      = 90,
    StructureBreak      = 91,
    DealingRange        = 92,
    SmcScan             = 93,
    AmdCycle            = 94,
    ImbalanceZone       = 95,
    AbsorptionMark      = 96,
    DeltaDivergence     = 97,
    OrderFlowScan       = 98,
}

impl PrimitiveNameKey {
    pub const COUNT: usize = 99;

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
            "anchored_vwap"            => Some(Self::AnchoredVwap),
            "fixed_vwap"               => Some(Self::FixedVwap),
            "candle_pattern"           => Some(Self::CandlePattern),
            "candle_pattern_scan"      => Some(Self::CandlePatternScan),
            "order_block"              => Some(Self::OrderBlock),
            "fair_value_gap"           => Some(Self::FairValueGap),
            "liquidity_sweep"          => Some(Self::LiquiditySweep),
            "structure_break"          => Some(Self::StructureBreak),
            "dealing_range"            => Some(Self::DealingRange),
            "smc_scan"                 => Some(Self::SmcScan),
            "amd_cycle"                => Some(Self::AmdCycle),
            "imbalance_zone"           => Some(Self::ImbalanceZone),
            "absorption_mark"          => Some(Self::AbsorptionMark),
            "delta_divergence"         => Some(Self::DeltaDivergence),
            "order_flow_scan"          => Some(Self::OrderFlowScan),
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

    // Center panel — chart-embed honest empty state (previously hardcoded
    // Russian "Chart — подключается")
    ChartConnecting  = 28,
}

impl TradingKey {
    pub const COUNT: usize = 29;

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

    // Drawing tool section headers (appended — see the block above for why
    // a new header lands here rather than being inserted next to its
    // siblings: discriminant == table row index, frozen).
    HeaderCandles      = 75,
    // Smart-money group header (markup-engines wave, 2026-08-25) — same
    // append-only reason as `HeaderCandles` above.
    HeaderSmartMoney   = 76,
    // Order-flow group header (markup-engines wave, 2026-08-25) — same
    // append-only reason as `HeaderSmartMoney` above.
    HeaderOrderFlow    = 77,
}

impl ToolbarMenuKey {
    pub const COUNT: usize = 78;

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

// =============================================================================
// City Name Keys  (timezone city labels for clock popup + timezone dropdown)
// =============================================================================

/// Localized city names for the 25 UTC offsets -12..=+12.
///
/// Variant order is **frozen** — discriminant == row index in `CITY_KEY_TABLE`.
/// Use [`CityKey::from_offset`] to map an integer offset to the corresponding key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum CityKey {
    BakerIsland  =  0,  // UTC-12
    PagoPago     =  1,  // UTC-11
    Honolulu     =  2,  // UTC-10
    Alaska       =  3,  // UTC-9
    LosAngeles   =  4,  // UTC-8
    Denver       =  5,  // UTC-7
    Chicago      =  6,  // UTC-6
    NewYork      =  7,  // UTC-5
    Halifax      =  8,  // UTC-4
    BuenosAires  =  9,  // UTC-3
    MidAtlantic  = 10,  // UTC-2
    Azores       = 11,  // UTC-1
    London       = 12,  // UTC+0
    Berlin       = 13,  // UTC+1
    Kyiv         = 14,  // UTC+2
    Moscow       = 15,  // UTC+3
    Dubai        = 16,  // UTC+4
    Tashkent     = 17,  // UTC+5
    Almaty       = 18,  // UTC+6
    Bangkok      = 19,  // UTC+7
    Singapore    = 20,  // UTC+8
    Tokyo        = 21,  // UTC+9
    Sydney       = 22,  // UTC+10
    Magadan      = 23,  // UTC+11
    Auckland     = 24,  // UTC+12
}

impl CityKey {
    pub const COUNT: usize = 25;

    /// Map a UTC offset (hours) in -12..=+12 to the corresponding key.
    /// Returns `None` for offsets outside that range.
    #[inline]
    pub fn from_offset(offset: i32) -> Option<Self> {
        match offset {
            -12 => Some(Self::BakerIsland),
            -11 => Some(Self::PagoPago),
            -10 => Some(Self::Honolulu),
            -9  => Some(Self::Alaska),
            -8  => Some(Self::LosAngeles),
            -7  => Some(Self::Denver),
            -6  => Some(Self::Chicago),
            -5  => Some(Self::NewYork),
            -4  => Some(Self::Halifax),
            -3  => Some(Self::BuenosAires),
            -2  => Some(Self::MidAtlantic),
            -1  => Some(Self::Azores),
            0   => Some(Self::London),
            1   => Some(Self::Berlin),
            2   => Some(Self::Kyiv),
            3   => Some(Self::Moscow),
            4   => Some(Self::Dubai),
            5   => Some(Self::Tashkent),
            6   => Some(Self::Almaty),
            7   => Some(Self::Bangkok),
            8   => Some(Self::Singapore),
            9   => Some(Self::Tokyo),
            10  => Some(Self::Sydney),
            11  => Some(Self::Magadan),
            12  => Some(Self::Auckland),
            _   => None,
        }
    }

    /// Get the localized city name for this key.
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&CITY_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for CityKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&CITY_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Head & Shoulders Pattern Point Keys
// =============================================================================

/// Named point labels for the `head_shoulders` drawing primitive's Coordinates
/// tab (`drawing::get_point_labels`). Real per-language trading terminology,
/// not a word-for-word translation — e.g. the two trough points are named
/// after the neckline they define, reusing the same "neckline" vocabulary
/// already established by `ConfigKey::ShowNeckline`.
///
/// Variant order is **frozen** — discriminant == row index in
/// `HEAD_SHOULDERS_KEY_TABLE`, and also the point order the primitive stores
/// (left shoulder, head, right shoulder, then the two neckline troughs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum HeadShouldersKey {
    LeftShoulder  = 0,
    Head          = 1,
    RightShoulder = 2,
    Neckline1     = 3,
    Neckline2     = 4,
}

impl HeadShouldersKey {
    pub const COUNT: usize = 5;

    /// Get translation for this key.
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&HEAD_SHOULDERS_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for HeadShouldersKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&HEAD_SHOULDERS_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Position (Long/Short) Point Keys
// =============================================================================

/// Named point labels for the `long_position`/`short_position` drawing
/// primitives' Placement tab (`drawing::get_point_labels`). Only two —
/// `points()` for both types returns exactly `[(ts_ms, entry_price),
/// (end_ts_ms, take_profit)]` (2026-08-09 audit: `stop_loss` is a real field
/// but is not part of the points array either type exposes, driven instead
/// by its own `ControlPointType::Point2` drag handle and the
/// `stop_zone_color` style property — there is no third "Stop" row to name
/// here).
///
/// Variant order is **frozen** — discriminant == row index in
/// `POSITION_POINT_KEY_TABLE`, and also the point order both primitives
/// return from `points()` (entry, then target).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum PositionPointKey {
    Entry  = 0,
    Target = 1,
}

impl PositionPointKey {
    pub const COUNT: usize = 2;

    /// Get translation for this key.
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&POSITION_POINT_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for PositionPointKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&POSITION_POINT_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Chart Shell Keys
// =============================================================================

/// UI strings for the chart shell frame (toolbars, panels) that are not tied
/// to a specific tool, tab, or property — the bottom toolbar's
/// selected-object label when nothing is selected, the raw-lab stub shell's
/// own "no data" watermark, its "Modals" launcher button, the launcher
/// menu's own two top-level section headers plus one sub-header per
/// `PrimitiveKind` under "Object modals" (wave 5, 2026-08-10 — the object
/// list grew from the curated 14 left-rail types to every registered
/// type, grouped by kind), the sixteen application-modal entries it lists
/// (wave 3, 2026-08-10 — `mlc-ui-lab`'s own launcher menu labels; the
/// modal SURFACES themselves reuse whatever i18n key they already carry
/// internally, this is only the one-line label the launcher's own list row
/// shows), and the lab dev-bar's own "Design compare" toggle (relocated off
/// the app-shell "Modals" button in the same wave — see
/// `mlc-ui-lab::theme::draw_lab_bar`'s doc for why).
///
/// Variant order is **frozen** — discriminant == row index in
/// `SHELL_KEY_TABLE`. Only En/Ru are filled (owner scope, 2026-08-10,
/// `mlc-ui-lab` stub-shell waves 1-5); the remaining 13 columns are empty
/// and fall back to En at runtime — an intentional, pre-existing pattern
/// (see `tables.rs`'s own module doc), not a shortcut. Deliberately NOT
/// part of `tables.rs`'s `every_chart_table_is_fully_translated` test for
/// that reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum ShellKey {
    /// Bottom toolbar's selected-object label when nothing is selected.
    NothingSelected = 0,
    /// Chart-pane watermark shown by the UI lab's stub shell (no data
    /// plane, no chart engine — this text says so explicitly).
    SimulatorWatermark = 1,
    /// Top toolbar's "Modals" button — opens the launcher menu (wave 3).
    ModalsButton = 2,
    /// Launcher menu top-level section header over every registered
    /// primitive type (grouped into `CategoryLine..CategorySignal`
    /// sub-headers below it, wave 5).
    LauncherObjectModals = 3,
    /// Launcher menu section header over the 16 application-level modals.
    LauncherAppModals = 4,
    /// Launcher menu row label + `chart_settings` modal.
    AppModalChartSettings = 5,
    /// Launcher menu row label + `indicator_settings` modal.
    AppModalIndicatorSettings = 6,
    /// Launcher menu row label + `watchlist_modal`.
    AppModalWatchlist = 7,
    /// Launcher menu row label + `alert_settings` modal.
    AppModalAlerts = 8,
    /// Launcher menu row label + `user_settings` modal.
    AppModalUserSettings = 9,
    /// Launcher menu row label + `tags_tabs_modal`.
    AppModalTagsTabs = 10,
    /// Launcher menu row label + `trading_container_modal`.
    AppModalTradingContainer = 11,
    /// Launcher menu row label + `search_overlay`.
    AppModalSearch = 12,
    /// Launcher menu row label + `template_name_modal`.
    AppModalTemplateName = 13,
    /// Launcher menu row label + `preset_name_input`.
    AppModalPresetName = 14,
    /// Launcher menu row label + `chart_browser`.
    AppModalChartBrowser = 15,
    /// Launcher menu row label + `compare_settings`.
    AppModalCompareSettings = 16,
    /// Launcher menu row label + `context_menu`.
    AppModalContextMenu = 17,
    /// Launcher menu row label + `clock_popup`.
    AppModalClock = 18,
    /// Launcher menu row label + `panel_settings_popup`.
    AppModalPanelSettings = 19,
    /// Launcher menu row label + `indicator_overlay`.
    AppModalIndicatorOverlay = 20,
    /// Lab dev-bar button that toggles the four-sector primitive-settings
    /// design-comparison grid (`crate::variants`) — relocated here off the
    /// app-shell "Modals" button in wave 3, since that button now opens the
    /// launcher menu instead.
    DesignCompareToggle = 21,
    /// Launcher menu sub-header over `PrimitiveKind::Line` rows (wave 5,
    /// 2026-08-10 — the "Object modals" section grew from the curated 14
    /// left-rail types to every registered type, grouped by
    /// `mlc_core::PrimitiveKind`; see `mlc-ui-lab`'s own
    /// `shell::launcher_menu::kind_label`).
    CategoryLine = 22,
    /// Launcher menu sub-header over `PrimitiveKind::Channel` rows.
    CategoryChannel = 23,
    /// Launcher menu sub-header over `PrimitiveKind::Shape` rows.
    CategoryShape = 24,
    /// Launcher menu sub-header over `PrimitiveKind::Fibonacci` rows.
    CategoryFibonacci = 25,
    /// Launcher menu sub-header over `PrimitiveKind::Gann` rows.
    CategoryGann = 26,
    /// Launcher menu sub-header over `PrimitiveKind::Pattern` rows.
    CategoryPattern = 27,
    /// Launcher menu sub-header over `PrimitiveKind::Annotation` rows.
    CategoryAnnotation = 28,
    /// Launcher menu sub-header over `PrimitiveKind::Measurement` rows.
    CategoryMeasurement = 29,
    /// Launcher menu sub-header over `PrimitiveKind::Trading` rows.
    CategoryTrading = 30,
    /// Launcher menu sub-header over `PrimitiveKind::Signal` rows.
    CategorySignal = 31,
    /// Launcher menu section header over the rows that open the PRODUCTION
    /// primitive-settings modal (the current code) rather than the approved
    /// concept — the two sit side by side in the lab on purpose.
    LauncherProductionModals = 32,
    /// Lab dev-bar button: every design-comparison sector is open.
    LabAllSectorsOpen = 33,
    /// Lab dev-bar button: at least one sector was closed, click to restore.
    LabReopenSectors = 34,
}

impl ShellKey {
    pub const COUNT: usize = 35;

    /// Get translation for this key.
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&SHELL_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for ShellKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&SHELL_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Concept Modal Keys (approved archetype-modal design — mlc-ui-lab::concept::spec)
// =============================================================================

/// UI strings for the approved archetype-modal design concept
/// (`mlc-ui-lab::concept::spec` — a Rust transcription of `sim/registry.js`,
/// the approved 2026-08-10 modal concepts). Covers every row/group/tab label,
/// hint, and demo value across the 14 object-bound archetypes (ladder / wave /
/// pattern / position / measure / text / table / shape / regression / brush /
/// icon) and the 9 application-level modals, that isn't already covered by an
/// existing key elsewhere in this module (`ConfigKey`, `SettingsKey`,
/// `MenuKey`, `StyleKey`, `ToolbarMenuKey`, `PositionPointKey`, `ShellKey`, …
/// — `spec.rs`'s own literals reuse those directly where the exact wording
/// already matches).
///
/// Variant order is **frozen** — discriminant == row index in
/// `CONCEPT_KEY_TABLE`. Only En/Ru are filled (owner scope, 2026-08-11,
/// following `ShellKey`'s own convention — see its doc above); the remaining
/// 13 columns are empty and fall back to En at runtime. Deliberately NOT part
/// of `tables.rs`'s `every_chart_table_is_fully_translated` test, for the
/// same reason as `ShellKey`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum ConceptKey {
    // Timeframes — `tf_group()`'s own range-slider captions (six numeric
    // visibility classes, the control the app actually ships).
    SecondsRangeCap = 0,
    MinutesRangeCap = 1,
    HoursRangeCap = 2,
    DaysRangeCap = 3,
    WeeksRangeCap = 4,
    MonthsRangeCap = 5,

    // Placement / behaviour — `placement_tab()`'s own shared tf/points/
    // behaviour groups, reused by every archetype's own Placement tab.
    BehaviourGroupTitle = 6,
    SyncAllChartsOfSymbol = 7,
    PointStart = 8,
    PointEnd = 9,
    Anchor = 10,
    AnchorToPriceAndBar = 11,
    AnchorToBar = 12,
    AnchorToScreen = 13,

    // Shared groups — `fill_group()`/`extend_group()`'s own short row
    // labels, and the `Тип` dropdown label reused by both the wave
    // correction-type dropdown and the trading-container account-type
    // dropdown.
    FillToggle = 14,
    ExtendLeftShort = 15,
    ExtendRightShort = 16,
    TypeDropdownLabel = 17,

    // Labels / plate — `label_rows()`/`plate_group()`, shared by every
    // archetype's own Labels tab.
    LabelColor = 18,
    Plate = 19,
    PlateBehindLabel = 20,
    PlateColor = 21,
    PlateRounding = 22,
    PlatePadding = 23,

    // Ladder — `ladder_tabs()` (Fib retracement / Gann fan / Pitchfork).
    LadderGeneralGroupTitle = 24,
    LadderPalettesGroupTitle = 25,
    FillToNextLevel = 26,
    LevelLabelsGroupTitle = 27,
    NoteLabelInheritsLevelColor = 28,
    Angles = 29,

    // Waves — `wave_tabs()` (Elliott impulse / correction).
    WavesTabLabel = 30,
    NoteWaveNamesFromDegree = 31,
    ShowDegree = 32,
    Degree = 33,
    Notation = 34,
    NoteWaveNotationCorrectionCombination = 35,
    CorrectionOnlyHint = 36,
    FillBetweenWaves = 37,
    WaveLabelsGroupTitle = 38,
    CustomLabelsGroupTitle = 39,
    OverrideHint = 40,
    WaveColumnHeader = 41,
    LabelColumnHeader = 42,

    // Patterns — `pattern_tabs()` (XABCD harmonic pattern).
    RatiosTabLabel = 43,
    Subtype = 44,
    PatternDropdownLabel = 45,
    NotePatternSubtypesList = 46,
    PatternBody = 47,
    TargetRatiosGroupTitle = 48,
    SegmentColumnHeader = 49,
    ActualColumnHeader = 50,
    ToleranceColumnHeader = 51,
    ScoringGroupTitle = 52,
    HitColor = 53,
    MissColor = 54,
    ColorLinesByHit = 55,
    PointLabelsGroupTitle = 56,
    NotePatternPointNames = 57,
    OnChartValuesGroupTitle = 58,
    ShowRatiosValues = 59,
    ShowSegmentLength = 60,
    DecimalPlaces = 61,
    SideOfSegment = 62,

    // Position — `position_tabs()` (long/short position).
    TradeTabLabel = 63,
    AccountSize = 64,
    RiskPerTradePct = 65,
    CalcSizeFromRisk = 66,
    ZonesGroupTitle = 67,
    StopZone = 68,
    TargetZone = 69,
    StopPointLabel = 70,
    DisplayGroupTitle = 71,
    Currency = 72,
    PercentUnit = 73,
    ShowPnlToggle = 74,
    ShowVolumeToggle = 75,
    ResultGroupTitle = 76,
    CalculatedHintSingular = 77,
    RiskRewardRatio = 78,
    NotePositionResultReadOnly = 79,
    NotePositionLabelsWhat = 80,
    WhatToWriteGroupTitle = 81,
    WritePointName = 82,
    WritePrice = 83,
    WriteDistancePct = 84,
    WriteRiskTargetSum = 85,

    // Measurement — `measure_tabs()` (price/date range).
    WhatToShowTabLabel = 86,
    ValuesGroupTitle = 87,
    PriceChange = 88,
    BarCount = 89,
    VolumeOverRange = 90,
    AveragePrice = 91,
    SlopeAngle = 92,
    FormatGroupTitle = 93,
    ThousandsSeparator = 94,
    VolumeUnit = 95,
    AutoKmUnit = 96,
    LineAndArrowGroupTitle = 97,
    ArrowheadAtEnd = 98,
    ColorBySign = 99,
    MeasureUp = 100,
    MeasureDown = 101,
    PlateVerticalMiddle = 102,
    PlateBackground = 103,
    BackgroundColor = 104,

    // Text — `text_tabs()` (callout).
    ContentGroupTitle = 105,
    NoteCalloutDemoText = 106,
    NoteCalloutPlaceholders = 107,
    FontGroupTitle = 108,
    Typeface = 109,
    StrikethroughAbbrev = 110,
    LayoutGroupTitle = 111,
    AlignLeftShort = 112,
    AlignRightShort = 113,
    WordWrap = 114,
    WidthFitsContent = 115,
    MaxWidth = 116,
    Border = 117,
    CalloutBorderColor = 118,
    Background = 119,
    Shadow = 120,
    CalloutHint = 121,
    TailToPoint = 122,
    Tip = 123,
    ArrowTipOption = 124,

    // Table — `table_tabs()` (Table archetype's own editable cell grid).
    RowsCount = 125,
    ColumnsCount = 126,
    LevelColumnHeader = 127,
    DemoResistance = 128,
    DemoDailyHigh = 129,
    DemoOnRetest = 130,
    DemoBelowLow = 131,
    ColumnsGroupTitle = 132,
    ColumnColumnHeader = 133,
    AutoWidthValue = 134,
    GridAndHeaderGroupTitle = 135,
    LineColor = 136,
    FirstRowIsHeader = 137,
    AlternateRows = 138,
    TextAndBackgroundGroupTitle = 139,
    TableBackground = 140,
    AnchorTopLeft = 141,
    AnchorTopRight = 142,
    AnchorBottomLeft = 143,
    AnchorBottomRight = 144,

    // Shape — `shape_tabs()` (rectangle / parallel channel).
    Outline = 145,
    TextOnShape = 146,
    NoteShapePolylinePointsList = 147,
    ShapeSubject = 148,
    ChannelSubject = 149,
    MiddleLine = 150,
    DashedShort = 151,

    // Regression — `regression_tabs()` (regression trend).
    DeviationsTabLabel = 152,
    CalculationGroupTitle = 153,
    Period = 154,
    ByPoints = 155,
    SigmasGroupTitle = 156,
    Multiplier = 157,
    FillOpacityLong = 158,
    MetricsGroupTitle = 159,
    CalculatedHintPlural = 160,
    ShowSlope = 161,
    ShowRSquared = 162,
    Slope = 163,
    DemoSlopeValue = 164,

    // Brush — `brush_tabs()`.
    StrokeGroupTitle = 165,
    SmoothingStrength = 166,
    RoundEndCaps = 167,
    CloseOutline = 168,
    FillClosedOutline = 169,
    NoteBrushPointsNotEditable = 170,

    // Icon — `icon_tabs()` (flag).
    IconTabLabel = 171,
    Rendering = 172,
    IconOutlineToggle = 173,
    LabelNextToIcon = 174,
    DemoTrendEntryLabel = 175,
    Side = 176,

    // Application modals — `app_modals()` (the 9 application-level modals:
    // chart_settings, indicator_settings, watchlist, alerts, user_settings,
    // tags_tabs, trading, search, template_name).
    Instrument = 177,
    UpBody = 178,
    DownBody = 179,
    Wicks = 180,
    WickColor = 181,
    Other = 182,
    LastPrice = 183,
    PreviousClose = 184,
    HighAndLow = 185,
    StatusLineTabLabel = 186,
    Ticker = 187,
    OhlcValues = 188,
    Change = 189,
    BackgroundOpacity = 190,
    Vertical = 191,
    Horizontal = 192,
    InvertedScale = 193,
    ShowSeconds = 194,
    ShowDate = 195,
    Crosshair = 196,
    SolidBackground = 197,
    Gradient = 198,
    Margins = 199,
    RightMarginBars = 200,
    SmoothingPeriod = 201,
    Upper = 202,
    Value = 203,
    Lower = 204,
    MainLine = 205,
    BetweenLevels = 206,
    Digits = 207,
    DefaultValue = 208,
    ConditionsPlural = 209,
    CrossesUp = 210,
    CrossesDown = 211,
    ExitsZone = 212,
    Markers = 213,
    Instruments = 214,
    ChangeAbbrev = 215,
    Crypto = 216,
    Forex = 217,
    UsStocks = 218,
    SyncWithChart = 219,
    Spread = 220,
    PriceCross = 221,
    Up = 222,
    Expiry = 223,
    OneTime = 224,
    Expires = 225,
    After24h = 226,
    Sound = 227,
    Popup = 228,
    Telegram = 229,
    Webhook = 230,
    NoteAlertMessageTemplate = 231,
    List = 232,
    Active = 233,
    Status = 234,
    AlertStatusWaiting = 235,
    AlertStatusTriggered = 236,
    GeneralTabLabel = 237,
    Interface = 238,
    Language = 239,
    RussianLanguageName = 240,
    CompactMode = 241,
    TimeGroupTitle = 242,
    Performance = 243,
    FrameRate = 244,
    ShowFrameCounter = 245,
    Data = 246,
    HistoryDepthBars = 247,
    CacheToDisk = 248,
    TabsLabel = 249,
    ChartTabs = 250,
    Name = 251,
    TfAbbrev = 252,
    DemoChartTabScalp = 253,
    DemoTimeframe5m = 254,
    DemoChartTabUntitled = 255,
    DemoChartTabTimeframe1h = 256,
    Tags = 257,
    TagIdea = 258,
    TagWork = 259,
    TagArchive = 260,
    TagColor = 261,
    SyncGroups = 262,
    TagColorRed = 263,
    TagColorBlue = 264,
    Panels = 265,
    Tape = 266,
    ShowHeaders = 267,
    Account = 268,
    Exchange = 269,
    Leverage = 270,
    StopRequired = 271,
    TemplateNameGroupTitle = 272,
    MakeDefaultTemplate = 273,

    // Ring/circle Fibonacci tools (Fib Circles / Fib Arcs) — not sampled by
    // the approved design's 14 representative types (`spec.rs` only
    // samples fib_retracement/gann_fan/pitchfork from the Ladder
    // archetype), but named as a hard requirement by the archetype-modals
    // rollout plan (W1,
    // `docs/mlc/plans/archetype-modals-into-the-app-2026-08-11.md`): a
    // ring/circle fib is `Ladder`-archetype (it has a level ladder) but is
    // not called "Levels" — added here rather than reused from elsewhere
    // since neither `ConfigKey` nor the rest of `ConceptKey` names it.
    Rings = 274,
    /// Subject-tab label for the line family (trend line, ray, extended
    /// line, horizontal/vertical line, info line, trend angle, arrow).
    LineSubject = 275,

    // Levels tab top section (2026-08-13 axis split): the leading axis
    // label for the "Линии" segmented control (Моно/Палитра — those two
    // segment words are `ConfigKey::ColorModeMono`/`ColorModePalette`,
    // already in that table; this key is the row's own caption, which the
    // old single combined control never needed — see
    // `primitive_settings_levels::draw_top_section`'s own doc).
    LinesAxisLabel = 276,
    /// The "Заливка" axis's own non-Палитра segment word ("как линии" —
    /// inherit the line's own colour, `cfg.fill_color: None`). The axis's
    /// leading row label reuses `ConfigKey::Fill` ("Заливка") — no new key
    /// needed for that half.
    FillInheritLine = 277,

    // ---- Elliott ratio-rib style properties (2026-08-13) — next to
    // `ConfigKey::ShowRatios` in every Elliott type's own `style_properties`,
    // see `elliott::ratio`'s own module doc.
    RatioRibColorLabel = 278,
    RatioRibWidthLabel = 279,
    RatioRibStyleLabel = 280,

    // ---- Elliott hard-rule guard messages (2026-08-13) — one per
    // `elliott::guard::Violation::message`, see that module's own doc for
    // the predicate each message reports on.
    RuleWave2BeyondOrigin = 281,
    RuleWave4OverlapsWave1 = 282,
    RuleWave3Shortest = 283,
    RuleCorrectionBRule = 284,
    RuleCorrectionCRule = 285,
    RuleTriangleNotContracting = 286,

    // ---- Elliott child-kind picker labels (2026-08-13) — one per
    // `elliott::count::child_kinds` candidate id, see that function's own
    // doc for the fixed eight-candidate order these name.
    WaveKindImpulse = 287,
    WaveKindZigzag = 288,
    WaveKindRegularFlat = 289,
    WaveKindExpandedFlat = 290,
    WaveKindRunningFlat = 291,
    WaveKindTriangle = 292,
    WaveKindDoubleCombo = 293,
    WaveKindTripleCombo = 294,

    // ---- Elliott declared-subtype-vs-measured-ratio guard messages
    // (2026-08-13, owner ask: "есть ли проверка на плоскость? что плоскость
    // >0.9 если меньше это не плоскость") — see
    // `elliott::guard::correction_violations`'s own doc.
    RuleFlatRequiresBNearAOrigin = 295,
    RuleZigzagRequiresShallowB = 296,
    RuleExpandedFlatRequiresBBeyondAOrigin = 297,

    // ---- Elliott CONTEXT-aware guard messages (2026-08-13) — one per
    // `elliott::guard::violations_in` rule, see that function's own doc.
    RuleSubdivisionClass = 298,
    RuleComboXNotCorrective = 299,
    RuleComboWyzNotCorrective = 300,
    RuleComboAtMostOneZigzag = 301,
    RuleComboTriangleOnlyFinal = 302,
    RuleChainContinuityGap = 303,

    // ---- Elliott context-violations modal group (2026-08-13) — the
    // primitive settings modal's own Subject-tab "Rules" group (owner
    // ask: the violation as text, "правило — факт", not only a
    // recoloured segment on the chart) and the E2 two-step wave picker's
    // own back-row caption.
    RulesGroupTitle = 304,
    WaveBackLabel = 305,

    // ---- Elliott CHAIN guard messages (2026-08-13, owner ask: "ну и
    // правила внутри цепочек") — one per `elliott::guard::chain_violations`
    // rule that exists ONLY at chain level (the family rules it re-runs
    // reuse the existing `Rule*` keys above, e.g. `RuleWave2BeyondOrigin`).
    RuleChainAlternationBroken = 306,
    RuleChainDegreeMismatch = 307,

    // ---- Wave-gesture menu labels (2026-08-13) — the second control
    // surface's own two popups (`mlc_core::state::gestures`,
    // `mlc-app-layout::ui::context_menu::{build_wave_menu,
    // build_successor_menu}`): a double-click (or right-click, once the
    // owner picks) on a wave segment or a structure's own terminal point.
    // `WaveKind*`/`WaveBackLabel` above are reused verbatim for the rows
    // these two popups share with the inline E2/E3 pickers; these three
    // are the ones unique to the popup's own child-management rows and
    // its promote row.
    SelectSubdivisionLabel = 308,
    RemoveSubdivisionLabel = 309,
    PromoteChainLabel = 310,

    // ---- Elliott motive/retracement guard messages (2026-08-14, owner
    // report: an impulse's wave 4 drawn to CONTINUE past wave 3 in the
    // trend direction instead of retracing against it, and nothing
    // flagged it — "видно, что 4 волна ниже, чем 3, но ведь так не
    // может быть по правилам, почему она тут не подсвечивается как
    // ошибочная?"). See `elliott::guard::impulse_violations`'s own
    // `"wave3_exceeds_wave1"` check and `elliott::guard::violations_in`'s
    // own `"wave2_retraces_wave1"`/`"wave4_retraces_wave3"` checks.
    RuleWave2NotRetracingWave1 = 311,
    RuleWave4NotRetracingWave3 = 312,
    RuleWave3NotExceedingWave1 = 313,

    // ---- EWA Engine Settings modal (2026-08-14, owner ask: "для настройки
    // ЕВА движка можно сделать отдельную настройку, ну вот прям модалку ЕВА
    // движка") — `mlc-app-layout`'s `layout::modals::ewa_engine_settings`,
    // reached from the wave-menu popups' own trailing row and from the Wave
    // archetype's own Subject tab in the primitive settings modal.
    EwaEngineSettingsTitle = 314,
    EwaEngineSettingsMenuLabel = 315,
    EwaLabelSeparationGroupTitle = 316,
    EwaLabelSeparationStacked = 317,
    EwaLabelSeparationParallel = 318,
    EwaLabelSeparationTriangle = 319,
    EwaDetectHigherDegreeLabel = 320,
    EwaShowProjectionsLabel = 321,
    EwaGuardFamiliesGroupTitle = 322,
    EwaGuardGeometryLabel = 323,
    EwaGuardGrammarLabel = 324,
    EwaGuardChainLabel = 325,
    EwaRatioRibDefaultsGroupTitle = 326,

    // ---- Elliott subdivision-degree guard message (2026-08-14,
    // coordinator review of higher-degree auto-adoption: `elliott::count::
    // adopted_parent_degree`'s own ceiling case — `WaveDegree::coarser`
    // saturates at `Supermillennium`, so adopting children already at that
    // degree leaves the new parent sharing its own children's degree
    // instead of sitting one step above them). See
    // `elliott::guard::violations_in`'s own `"subdivision_degree"` check —
    // a CONTEXT rule like `RuleSubdivisionClass` above, just checked from
    // the CHILD side (the primitive carrying the `WaveLink`) rather than
    // the parent, so it can name both degrees.
    RuleSubdivisionDegree = 327,

    // ---- Elliott truncated-fifth guard message (2026-08-15, owner ruling:
    // a fifth that fails to exceed wave 3 is a real Elliott shape — a
    // truncation — not automatically a mistake, so the rule gained a
    // per-structure excuse rather than staying unimplemented). See
    // `elliott::guard::impulse_violations`'s own `"wave5_beyond_wave3"`
    // check and `ElliottImpulse::truncated_fifth`'s own field doc.
    RuleWave5BeyondWave3 = 328,

    // ---- Wave-5 context-menu checkable row (2026-08-15, second entrance
    // for `ElliottImpulse::truncated_fifth` — the first is the structure's
    // own settings modal, free via `style_properties()`). Two labels, not
    // one dynamic string: `mlc-app-layout`'s `build_wave_menu` mirrors the
    // existing lock/visibility toggle idiom (`MenuKey::Lock`/`MenuKey::
    // Unlock`) — the label names the ACTION a click performs, which reads
    // back as the row's current state.
    MarkTruncatedFifthLabel = 329,
    ClearTruncatedFifthLabel = 330,

    // ---- Elliott declared-truncated-vs-measured-shape guard message
    // (2026-08-15, owner correction: `truncated_fifth` is a CLAIM about the
    // shape, not permission to stop checking it — the GRAMMAR-family
    // counterpart of `RuleWave5BeyondWave3` above, mirroring
    // `RuleFlatRequiresBNearAOrigin`'s own "declared subtype vs measured
    // shape" precedent). See `elliott::guard::impulse_violations`'s own
    // `"wave5_not_truncated"` check.
    RuleWave5NotTruncated = 331,

    // ---- Gann family's own undeclared toggles (2026-08-16, defect pass:
    // real fields `render`/`hit_test` already read, with no
    // `ConfigProperty` anywhere to reach them from the UI). Declared into
    // each type's own `level_properties()`, `PropertyGroup::General` — the
    // ladder archetype's own General group, mirroring `pitchfork.rs`'s
    // `extend`/`show_labels` (the design has no dedicated Gann-toggle slot,
    // so this is the closest honest home). `gann_fan`'s own `extend` reuses
    // the existing `ConfigKey::Extend` — only `upward` needs a new label.
    ShowGrid = 332,
    ShowDiagonals = 333,
    ShowCardinal = 334,
    ShowOrdinal = 335,
    Upward = 336,

    // ---- Comment's own `author`/`timestamp` (2026-08-16, defect pass):
    // both were declared, constructed and read by nothing — see
    // `Comment::render`'s own doc. Gated behind this single toggle,
    // defaulting off so no existing drawing changes.
    ShowAuthorTimestamp = 337,

    // ---- Measurement's own computed-stat block (W4, 2026-08-17) — the
    // elapsed-time row `DateRange`/`PriceDateRange` show in their own
    // read-only Result block. `PriceChange`/`PercentUnit` (86/73) already
    // cover the other two rows; only the bare "Duration" noun was missing
    // (`ShowDuration` = 91 is the CHECKBOX label, "Show Duration" — reusing
    // it for the row itself would read as a control, not a value).
    Duration = 338,

    // ---- Regression trend real fit (2026-08-23, "the defect" pass —
    // `RegressionSource`/`RegressionPeriodMode` in
    // `channels/regression_trend.rs`) — the Calculation group's own
    // source-dropdown option labels, same seven-value vocabulary
    // `mlc-backend-indicator-engine`'s `IndicatorParamType::source_options()`
    // gives an indicator's own source picker (`close`/`open`/`high`/`low`/
    // `hl2`/`hlc3`/`ohlc4`) — `mlc-core` cannot depend on that engine crate
    // (types → engines in the crate contract), so the vocabulary is
    // reproduced, not imported. Plus the Period dropdown's second option —
    // `ByPoints` (155) already names the first.
    SourceClose = 339,
    SourceOpen = 340,
    SourceHigh = 341,
    SourceLow = 342,
    SourceHl2 = 343,
    SourceHlc3 = 344,
    SourceOhlc4 = 345,
    ByBarCount = 346,

    // ---- Harmonic pattern kind names (`pattern_kind`'s own `Select`
    // options, `xabcd_pattern.rs`) — Scott Carney's own published names for
    // the five classifiable shapes (`mli::ewa::harmonic`'s own doc: "Scott
    // Carney's published ratio definitions"). Proper nouns, not phrases —
    // every language column carries the same Latin spelling, the same
    // treatment `NotePatternSubtypesList` (46) already gives them in its
    // own hint sentence; the `Unclassified` option reuses `LevelSetCustom`
    // (142, "Custom") rather than adding a sixth near-duplicate here.
    HarmonicKindGartley = 347,
    HarmonicKindBat = 348,
    HarmonicKindButterfly = 349,
    HarmonicKindCrab = 350,
    HarmonicKindShark = 351,

    // ---- Elliott diagonal child-kind picker labels (the diagonal arc,
    // 2026-08-24) — `elliott::count::CHILD_KIND_CANDIDATES`'s own two new
    // entries (`"elliott_impulse:leading_diagonal"`/`"elliott_impulse:
    // ending_diagonal"`), same row family as `WaveKind*` above (287-294),
    // read by the same `elliott::count::kind_label` picker function.
    WaveKindLeadingDiagonal = 352,
    WaveKindEndingDiagonal = 353,

    // ---- Candle pattern engine (markup-engines wave, 2026-08-25) — the
    // `patterns::candle_pattern`/`candle_pattern_scan` primitives' own
    // guard-rule messages, Stat-block rows, and scan-table column headers.
    // `RuleCandlePatternWindowIncomplete`/`RuleCandlePatternNotFormed` are
    // `elliott::guard::Violation::whole` messages, same naming convention as
    // every other `Rule*` key above (e.g. `RuleWave2BeyondOrigin` = 281).
    RuleCandlePatternWindowIncomplete = 354,
    RuleCandlePatternNotFormed = 355,
    /// `CandlePattern`'s own Stat block: catalogue bias row label.
    PatternBiasLabel = 356,
    PatternBiasBullish = 357,
    PatternBiasBearish = 358,
    PatternBiasIndecision = 359,
    /// `CandlePattern`'s own Stat block: "do the bars currently agree with
    /// the claim" row label — value is `YesValue`/`NoValue`.
    PatternMatchesBarsLabel = 360,
    YesValue = 361,
    NoValue = 362,
    /// `CandlePatternScan`'s own per-pattern table: hit-count column header.
    ScanHitsColumnHeader = 363,
    /// `CandlePatternScan`'s own Stat block: total-hit-count row label.
    ScanTotalHitsLabel = 364,

    // ---- Smart money (markup-engines wave, 2026-08-25) — the
    // `drawing::primitives_v2::smc::{order_block, fair_value_gap,
    // liquidity_sweep, structure_break}` primitives' own guard-rule
    // messages, Stat-block rows, and Pills-row labels. Rule keys follow the
    // same `elliott::guard::Violation::whole` naming convention as
    // `RuleCandlePattern*` above.
    RuleOrderBlockNoStructureBreak = 365,
    RuleFvgNotFormed = 366,
    RuleLiquiditySweepNotFormed = 367,
    RuleStructureBreakNotFormed = 368,
    RuleStructureBreakKindMismatch = 369,
    /// Shared Stat-block row label: direction (Up/Down), value drawn from
    /// `PatternBiasBullish`/`PatternBiasBearish` above — `order_block`/
    /// `fair_value_gap` both use this.
    SmcDirectionLabel = 370,
    /// Shared Stat-block row label: zone bounds (`"{low} - {high}"`) —
    /// `order_block`/`fair_value_gap` both use this.
    SmcZoneLabel = 371,
    /// `OrderBlock`'s own Stat block: the structure-break bar index.
    OrderBlockBreakFollowsLabel = 372,
    /// `OrderBlock`'s own Stat block: the breaker-transition bar index, or
    /// `NoneValue` when the block has not flipped.
    BreakerBarLabel = 373,
    /// `FairValueGap`'s own Stat block: mitigation bar index, or
    /// `NoneValue`.
    ZoneMitigatedLabel = 374,
    /// `FairValueGap`'s own Stat block: fill bar index, or `NoneValue`.
    ZoneFilledLabel = 375,
    /// `LiquiditySweep`'s own Stat block: the swept swing's own price.
    SweptLevelLabel = 376,
    /// `LiquiditySweep`'s own Stat block: bars between the wick and the
    /// close that recovered back inside the level.
    RecoveryBarsLabel = 377,
    /// `StructureBreak`'s own Stat block: the broken level's own price.
    StructureBreakLevelLabel = 378,
    /// `StructureBreak`'s own Stat block: the broken swing's own bar index.
    StructureBreakSwingLabel = 379,
    /// `StructureBreak`'s own Stat block: prevailing trend at the break bar.
    TrendAtBarLabel = 380,
    TrendValueUp = 381,
    TrendValueDown = 382,
    /// Appendix A: "anything else is unresolved and must be reported as
    /// unresolved, not defaulted to a direction" — the value this trend
    /// Stat row prints instead of guessing.
    TrendValueUnresolved = 383,
    /// `StructureBreak`'s own Pills row label — which kind (BOS/CHoCH) the
    /// analyst claims this break is.
    StructureBreakKindLabel = 384,
    StructureBreakKindBos = 385,
    StructureBreakKindChoch = 386,
    /// Generic "nothing resolved yet" Stat value — `OrderBlock`'s own
    /// breaker bar, `FairValueGap`'s own mitigated/filled bars, before any
    /// of the three has happened.
    NoneValue = 387,

    // ---- Dealing range / AMD cycle / SMC scan (markup-engines wave,
    // 2026-08-25 tail) — `drawing::primitives_v2::smc::{dealing_range,
    // amd_cycle, smc_scan}`'s own guard-rule message, Pills-row labels,
    // Stat-block rows, and structure-survey table labels. Appended after
    // `NoneValue` for the same append-only reason every block above states.
    /// `DealingRange`'s own guard rule — `FromStructure` mode only; `Manual`
    /// mode never reports it (see that primitive's own doc).
    RuleDealingRangeNoStructure = 388,
    /// `DealingRange`'s own Pills row: which mode supplies the range bounds.
    DealingRangeSourceLabel = 389,
    DealingRangeSourceManual = 390,
    DealingRangeSourceFromStructure = 391,
    /// `DealingRange`'s own OTE-band toggle label.
    DealingRangeShowOteLabel = 392,
    /// `DealingRange`'s own Stat block rows: high, low, equilibrium, size.
    DealingRangeHighLabel = 393,
    DealingRangeLowLabel = 394,
    DealingRangeEquilibriumLabel = 395,
    DealingRangeSizeLabel = 396,
    /// `AmdCycle`'s own guard rule.
    RuleAmdCycleNotFormed = 397,
    /// Shared phase label — `AmdCycle`'s own on-chart phase labels AND Stat
    /// block rows, and `SmcScan`'s own structure-table row name for the
    /// `amd_cycle` hit kind's manipulation/accumulation/distribution
    /// sub-shapes reuse the same three keys below rather than declaring a
    /// second set.
    AccumulationLabel = 398,
    ManipulationLabel = 399,
    DistributionLabel = 400,
    /// `SmcScan`'s own structure-table row names for the two surveyed
    /// structures that carry no standalone claim primitive (and therefore
    /// no `PrimitiveNameKey` of their own): equal highs/lows and
    /// displacement legs.
    EqualLevelsLabel = 401,
    DisplacementLegLabel = 402,
    /// `SmcScan`'s own structure-table title.
    SmcScanStructuresLabel = 403,

    // Order flow (markup-engines wave, 2026-08-25 tail) —
    // `drawing::primitives_v2::orderflow::{imbalance_zone, absorption_mark,
    // delta_divergence, order_flow_scan}`'s own guard-rule messages,
    // Pills-row labels, Stat-block rows, and structure-survey table labels.
    // Appended after `SmcScanStructuresLabel` for the same append-only
    // reason every block above states.
    /// The shared "this series carries no footprint data at all" rule every
    /// order-flow primitive checks FIRST — see that module's own doc.
    RuleOrderFlowDataMissing = 404,
    /// `ImbalanceZone`'s own guard rule — the anchor bar carries no stacked
    /// imbalance run on either side.
    RuleImbalanceZoneNotPresent = 405,
    /// `AbsorptionMark`'s own guard rule — the anchor bar's own
    /// `is_absorption` flag is false.
    RuleAbsorptionNotPresent = 406,
    /// `DeltaDivergence`'s own guard rule — `mli::orderflow::
    /// delta_divergences` reports no divergence at all at this bar.
    RuleDeltaDivergenceNotFormed = 407,
    /// `DeltaDivergence`'s own second guard rule — a divergence IS resolved
    /// at this bar, but of the OTHER kind than claimed. Kept separate from
    /// `RuleDeltaDivergenceNotFormed` for the same reason
    /// `RuleStructureBreakKindMismatch` is kept separate from
    /// `RuleStructureBreakNotFormed`.
    RuleDeltaDivergenceKindMismatch = 408,
    /// `ImbalanceZone`'s own Stat block row — the resolved run's own length
    /// (level count).
    ImbalanceZoneRunLengthLabel = 409,
    /// `AbsorptionMark`'s own Stat block rows: bar delta, bar volume, and
    /// the ratio between them.
    AbsorptionDeltaLabel = 410,
    AbsorptionVolumeLabel = 411,
    AbsorptionRatioLabel = 412,
    /// `DeltaDivergence`'s own Pills row: which kind is claimed.
    DeltaDivergenceKindLabel = 413,
    DeltaDivergenceKindRegular = 414,
    DeltaDivergenceKindHidden = 415,
    /// `DeltaDivergence`'s own Stat block rows: the two prices compared, the
    /// two series (delta) values compared, and the earlier swing's own bar.
    DeltaDivergencePriceFromLabel = 416,
    DeltaDivergencePriceToLabel = 417,
    DeltaDivergenceValueFromLabel = 418,
    DeltaDivergenceValueToLabel = 419,
    DeltaDivergenceEarlierSwingLabel = 420,
    /// `OrderFlowScan`'s own structure-table title.
    OrderFlowScanStructuresLabel = 421,
    /// `OrderFlowScan`'s own structure-table row names for the two surveyed
    /// structures that carry no standalone claim primitive of their own
    /// (and therefore no `PrimitiveNameKey`): single-print levels and
    /// unfinished-business levels.
    SinglePrintLabel = 422,
    UnfinishedBusinessLabel = 423,
    /// Every survey primitive's own coverage row: does it survey the range
    /// its two points bracket, or every loaded bar, recomputing as bars
    /// arrive.
    ScanCoverageLabel = 424,
    ScanCoverageRange = 425,
    ScanCoverageAllBars = 426,
    /// `SmcScan`'s own switch for leaving already-filled gaps out.
    SmcScanHideFilledGapsLabel = 427,
    /// The shared markup layer's own first cross-primitive rule: two claims
    /// of the same kind anchored on the same bar.
    RuleOverlappingClaim = 428,
    /// The shared markup layer's own containment row: which parent claim
    /// this one sits inside, and in which of that parent's slots.
    MarkupInsideLabel = 429,
    /// `DealingRange`'s own two halves, named for a linked child's own
    /// containment row. The midpoint reuses `DealingRangeEquilibriumLabel`.
    DealingRangePremiumLabel = 430,
    DealingRangeDiscountLabel = 431,
}

impl ConceptKey {
    pub const COUNT: usize = 432;

    /// Get translation for this key.
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&CONCEPT_KEY_TABLE[self as usize], lang as usize)
    }
}

impl uzor::i18n::Translate for ConceptKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&CONCEPT_KEY_TABLE[self as usize], lang_index)
    }
}

// =============================================================================
// Candle Pattern Keys  (the 34-pattern catalogue's own display names)
// =============================================================================

/// Localized display names for `mli::events::candle_pattern::
/// CandlePatternKind`'s 34 patterns.
///
/// The catalogue itself — which patterns exist, what identifies one on the
/// wire, how many bars it needs, which way it leans — belongs to `mli` and
/// is not duplicated here. What lives here is the one thing `mli` has no
/// business owning: how each pattern's name is SPELLED for a reader. `mli`
/// carries an English `label()` as its own canon; this table is what the
/// chart shows, and [`CandlePatternKey::from_id`] is the single bridge
/// between the two, keyed on `CandlePatternKind::id()` — the stable
/// snake_case wire identifier, never on the display string.
///
/// En/Ru filled only, the same owner-approved scope [`ShellKey`] carries:
/// the remaining columns stay empty and fall back to English at runtime.
/// Deliberately NOT listed in `tables.rs`'s `every_chart_table_is_fully_
/// translated` assertion for that reason — a partially filled row fails it
/// by design, and claiming thirteen translations nobody wrote would be
/// worse than falling back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CandlePatternKey {
    Doji                    = 0,
    GravestoneDoji          = 1,
    DragonflyDoji           = 2,
    LongLeggedDoji          = 3,
    Hammer                  = 4,
    InvertedHammer          = 5,
    ShootingStar            = 6,
    HangingMan              = 7,
    Marubozu                = 8,
    WhiteMarubozu           = 9,
    BlackMarubozu           = 10,
    SpinningTop             = 11,
    BullishEngulfing        = 12,
    BearishEngulfing        = 13,
    BullishHarami           = 14,
    BearishHarami           = 15,
    PiercingPattern         = 16,
    DarkCloudCover          = 17,
    TweezerTop              = 18,
    TweezerBottom           = 19,
    MorningStar             = 20,
    EveningStar             = 21,
    MorningDojiStar         = 22,
    EveningDojiStar         = 23,
    ThreeWhiteSoldiers      = 24,
    ThreeBlackCrows         = 25,
    ThreeInsideUp           = 26,
    ThreeInsideDown         = 27,
    ThreeOutsideUp          = 28,
    ThreeOutsideDown        = 29,
    RisingThreeMethods      = 30,
    FallingThreeMethods     = 31,
    UpsideGapTwoCrows       = 32,
    DownsideGapThreeMethods = 33,
}

impl CandlePatternKey {
    pub const COUNT: usize = 34;

    /// Get translation for this key.
    #[inline]
    pub fn get(self, lang: Language) -> &'static str {
        uzor::table_lookup!(&CANDLE_PATTERN_KEY_TABLE[self as usize], lang as usize)
    }

    /// The key for `mli`'s own wire id (`CandlePatternKind::id()`).
    ///
    /// `None` for an unknown id rather than a placeholder key: an id this
    /// table does not know is a catalogue that grew in `mli` without its
    /// names following, and a caller must be able to see that rather than
    /// render someone else's pattern name.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "doji"                    => Some(Self::Doji),
            "gravestone_doji"         => Some(Self::GravestoneDoji),
            "dragonfly_doji"          => Some(Self::DragonflyDoji),
            "long_legged_doji"        => Some(Self::LongLeggedDoji),
            "hammer"                  => Some(Self::Hammer),
            "inverted_hammer"         => Some(Self::InvertedHammer),
            "shooting_star"           => Some(Self::ShootingStar),
            "hanging_man"             => Some(Self::HangingMan),
            "marubozu"                => Some(Self::Marubozu),
            "white_marubozu"          => Some(Self::WhiteMarubozu),
            "black_marubozu"          => Some(Self::BlackMarubozu),
            "spinning_top"            => Some(Self::SpinningTop),
            "bullish_engulfing"       => Some(Self::BullishEngulfing),
            "bearish_engulfing"       => Some(Self::BearishEngulfing),
            "bullish_harami"          => Some(Self::BullishHarami),
            "bearish_harami"          => Some(Self::BearishHarami),
            "piercing_pattern"        => Some(Self::PiercingPattern),
            "dark_cloud_cover"        => Some(Self::DarkCloudCover),
            "tweezer_top"             => Some(Self::TweezerTop),
            "tweezer_bottom"          => Some(Self::TweezerBottom),
            "morning_star"            => Some(Self::MorningStar),
            "evening_star"            => Some(Self::EveningStar),
            "morning_doji_star"       => Some(Self::MorningDojiStar),
            "evening_doji_star"       => Some(Self::EveningDojiStar),
            "three_white_soldiers"    => Some(Self::ThreeWhiteSoldiers),
            "three_black_crows"       => Some(Self::ThreeBlackCrows),
            "three_inside_up"         => Some(Self::ThreeInsideUp),
            "three_inside_down"       => Some(Self::ThreeInsideDown),
            "three_outside_up"        => Some(Self::ThreeOutsideUp),
            "three_outside_down"      => Some(Self::ThreeOutsideDown),
            "rising_three_methods"    => Some(Self::RisingThreeMethods),
            "falling_three_methods"   => Some(Self::FallingThreeMethods),
            "upside_gap_two_crows"    => Some(Self::UpsideGapTwoCrows),
            "downside_gap_three_methods" => Some(Self::DownsideGapThreeMethods),
            _ => None,
        }
    }
}

impl uzor::i18n::Translate for CandlePatternKey {
    #[inline]
    fn translate(self, lang_index: usize) -> &'static str {
        uzor::table_lookup!(&CANDLE_PATTERN_KEY_TABLE[self as usize], lang_index)
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
        assert_eq!(ConfigKey::Labels.get(Language::Ru), "Подписи");
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

    #[test]
    fn test_head_shoulders_keys() {
        assert_eq!(HeadShouldersKey::Head.get(Language::En), "Head");
        assert_eq!(HeadShouldersKey::Head.get(Language::Ru), "Голова");
    }

    #[test]
    fn test_position_point_keys() {
        assert_eq!(PositionPointKey::Entry.get(Language::En), "Entry");
        assert_eq!(PositionPointKey::Entry.get(Language::Ru), "Вход");
        assert_eq!(PositionPointKey::Target.get(Language::En), "Target");
        assert_eq!(PositionPointKey::Target.get(Language::Ru), "Цель");
    }
}
