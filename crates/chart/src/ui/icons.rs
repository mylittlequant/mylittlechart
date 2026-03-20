//! Chart icon registry
//!
//! Maps icon name strings (as used in `ToolbarIconId`) to SVG string constants.
//! Only includes icons used by the chart toolbar (`toolbar.rs`).
//!
//! SVG constants are copied from `crates/core/src/ui/icons.rs`.

// =============================================================================
// Icon enum (moved from zengeld-terminal-core so toolbar_state.rs can live here)
// =============================================================================

/// Icon identifier for lookup.
///
/// Each variant maps to an SVG constant. The `svg()` method is implemented in
/// `zengeld-terminal-core` (which holds the full SVG constant table) via a
/// re-export.  Within `zengeld-chart` the enum is used purely as a key for
/// `ToolbarState::quick_select_icons`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    // === Chart Types ===
    Candlestick,
    HollowCandles,
    HeikinAshi,
    LineChart,
    StepLine,
    LineWithMarkers,
    AreaChart,
    HlcArea,
    BarChart,
    Histogram,
    Columns,
    Baseline,

    // === Drawing Tools ===
    TrendLine,
    HorizontalLine,
    VerticalLine,
    Ray,
    ExtendedLine,
    ParallelChannel,
    HorizontalRay,
    CrossLine,
    InfoLine,
    TrendAngle,

    // === Channels ===
    RegressionTrend,
    FlatTopBottom,
    DisjointChannel,

    // === Pitchforks ===
    Pitchfork,
    SchiffPitchfork,
    ModifiedSchiff,
    InsidePitchfork,

    // === Fibonacci ===
    FibRetracement,
    FibExtension,
    FibChannel,
    FibCircle,
    FibSpiral,
    FibTimeZones,
    FibSpeedResistance,
    FibTrendTime,
    FibArcs,
    FibWedge,
    FibFan,

    // === Gann ===
    GannBox,
    GannSquare,
    GannFan,

    // === Patterns ===
    XabcdPattern,
    CypherPattern,
    HeadShoulders,
    AbcdPattern,
    TrianglePattern,
    ThreeDrives,

    // === Elliott ===
    ElliottImpulse,
    ElliottCorrection,
    ElliottTriangle,
    ElliottCombo,

    // === Cycles ===
    CycleLines,
    TimeCycles,
    SineWave,

    // === Shapes ===
    Rectangle,
    RotatedRectangle,
    Circle,
    Ellipse,
    Triangle,
    Arc,
    Polyline,
    Path,
    Curve,
    DoubleCurve,

    // === Arrows ===
    Arrow,
    ArrowUp,
    ArrowDown,

    // === Brushes ===
    Brush,
    Highlighter,

    // === Annotations ===
    Text,
    AnchoredText,
    Note,
    PriceNote,
    Signpost,
    Callout,
    Comment,
    PriceLabel,
    Sign,
    Flag,
    Diamond,
    Table,

    // === Icons ===
    Emoji,
    Image,

    // === Measurement ===
    PriceRange,
    DateRange,
    PriceDateRange,

    // === Volume ===
    VolumeProfile,

    // === Projection ===
    BarsPattern,
    PriceProjection,
    Projection,

    // === Tools ===
    Crosshair,
    Magnet,
    Cursor,
    Hand,
    Zoom,

    // === Actions ===
    Undo,
    Redo,
    Delete,
    Lock,
    Unlock,
    Eye,
    EyeOff,
    Copy,
    Settings,
    Close,
    /// Empty (outline) star — for watchlist toggle.
    Star,
    /// Filled (solid) star — for watchlist toggle (active state).
    StarFilled,

    // === Positions ===
    LongPosition,
    ShortPosition,

    // === Navigation ===
    ChevronUp,
    ChevronDown,
    ChevronRight,
    Plus,
    Minus,
    Grid,
    Layers,
    Indicators,
    Layout,

    // === UI Elements ===
    Search,
    Clock,
    Watermark,
    Legend,
    Tooltip,

    // === Panels ===
    Watchlist,
    Alert,
    Trading,
    Positions,
    PanelRight,
    PanelBottom,

    // === Theme ===
    Palette,

    // === Info ===
    Info,

    // === Signals ===
    Signal,

    // === Navigation/Menu ===
    Menu,

    // === Line Styles ===
    LineSolid,
    LineDashed,
    LineDotted,

    // === Primitive Toolbar ===
    Pencil,
    ColorFill,
    TextColor,
    LineWidth1,
    LineWidth2,
    LineWidth3,
    LineWidth4,
    MoreHorizontal,

    // === Window Layouts ===
    LayoutSingle,
    LayoutSplitH,
    LayoutSplitV,
    LayoutGrid2x2,
    Layout2Left1Right,
    Layout1Left2Right,
    Layout2Top1Bottom,
    Layout1Top2Bottom,
    Layout3Columns,
    Layout3Rows,
    Layout1Big3Small,

    // === Expand/Collapse ===
    Expand,
    Collapse,

    // === Move (viewport sync) ===
    Move,

    // === Object Tree / Sidebar ===
    ObjectTree,

    // === Zoom Controls ===
    ZoomIn,
    ZoomOut,
    ZoomReset,

    // === Screenshot ===
    Screenshot,

    // === Connectors ===
    CircuitBoard,

    // === Window Management ===
    NewWindow,

    // === User / Auth / Cloud ===
    Cloud,
    CloudDownload,
    User,
    LogIn,
    LogOut,
    ChevronLeft,
    Refresh,
    Shield,
    ShieldCheck,
    Globe,
    Key,
}

// =============================================================================
// Chart Type Icons
// =============================================================================

pub const ICON_CANDLESTICK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M7 4v3"/>
  <path d="M7 17v3"/>
  <rect x="5" y="7" width="4" height="10" rx="1"/>
  <path d="M17 6v2"/>
  <path d="M17 16v2"/>
  <rect x="15" y="8" width="4" height="8" rx="1" fill="currentColor"/>
</svg>"##;

pub const ICON_HOLLOW_CANDLES: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M7 4v3"/>
  <path d="M7 17v3"/>
  <rect x="5" y="7" width="4" height="10" rx="1"/>
  <path d="M17 6v2"/>
  <path d="M17 16v2"/>
  <rect x="15" y="8" width="4" height="8" rx="1"/>
</svg>"##;

pub const ICON_HEIKIN_ASHI: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M5 4v4"/>
  <path d="M5 16v4"/>
  <rect x="3" y="8" width="4" height="8" rx="1" fill="currentColor"/>
  <path d="M12 3v3"/>
  <path d="M12 15v6"/>
  <rect x="10" y="6" width="4" height="9" rx="1" fill="currentColor"/>
  <path d="M19 5v5"/>
  <path d="M19 17v2"/>
  <rect x="17" y="10" width="4" height="7" rx="1" fill="currentColor"/>
</svg>"##;

pub const ICON_LINE_CHART: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M3 17l6-6 4 4 8-8"/>
</svg>"##;

pub const ICON_AREA_CHART: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M3 17l6-6 4 4 8-8v11H3z" fill="currentColor" fill-opacity="0.3"/>
  <path d="M3 17l6-6 4 4 8-8"/>
</svg>"##;

pub const ICON_BAR_CHART: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M6 4v16"/>
  <path d="M4 8h2"/>
  <path d="M6 14h2"/>
  <path d="M12 6v12"/>
  <path d="M10 10h2"/>
  <path d="M12 14h2"/>
  <path d="M18 5v14"/>
  <path d="M16 8h2"/>
  <path d="M18 15h2"/>
</svg>"##;

pub const ICON_HISTOGRAM: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="3" y="12" width="4" height="8" fill="currentColor"/>
  <rect x="10" y="8" width="4" height="12" fill="currentColor"/>
  <rect x="17" y="4" width="4" height="16" fill="currentColor"/>
</svg>"##;

pub const ICON_BASELINE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M2 12h20"/>
  <path d="M3 8l5 8 4-10 5 6 5-4"/>
  <path d="M3 12L3 8l5 8V12" fill="currentColor" fill-opacity="0.3" stroke="none"/>
  <path d="M13 12v-2l5 6v-4" fill="currentColor" fill-opacity="0.3" stroke="none"/>
</svg>"##;

// =============================================================================
// Drawing Tool Icons
// =============================================================================

pub const ICON_TREND_LINE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 20L20 4"/>
  <circle cx="4" cy="20" r="2" fill="currentColor"/>
  <circle cx="20" cy="4" r="2" fill="currentColor"/>
</svg>"##;

pub const ICON_HORIZONTAL_LINE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M3 12h18"/>
  <circle cx="3" cy="12" r="2" fill="currentColor"/>
</svg>"##;

pub const ICON_VERTICAL_LINE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M12 3v18"/>
  <circle cx="12" cy="3" r="2" fill="currentColor"/>
</svg>"##;

pub const ICON_RAY: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 16L20 8"/>
  <circle cx="4" cy="16" r="2" fill="currentColor"/>
  <path d="M18 6l2 2-2 2"/>
</svg>"##;

pub const ICON_EXTENDED_LINE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M2 18L22 6"/>
  <circle cx="8" cy="14" r="2" fill="currentColor"/>
  <circle cx="16" cy="10" r="2" fill="currentColor"/>
</svg>"##;

pub const ICON_PARALLEL_CHANNEL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 18L20 10"/>
  <path d="M4 10L20 2"/>
  <circle cx="4" cy="18" r="1.5" fill="currentColor"/>
  <circle cx="4" cy="10" r="1.5" fill="currentColor"/>
</svg>"##;

pub const ICON_HORIZONTAL_RAY: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 12h17"/>
  <circle cx="4" cy="12" r="2" fill="currentColor"/>
  <path d="M19 9l3 3-3 3"/>
</svg>"##;

pub const ICON_CROSS_LINE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M12 3v18"/>
  <path d="M3 12h18"/>
  <circle cx="12" cy="12" r="2" fill="currentColor"/>
</svg>"##;

pub const ICON_INFO_LINE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 18L20 6"/>
  <circle cx="4" cy="18" r="2" fill="currentColor"/>
  <rect x="14" y="4" width="8" height="5" rx="1"/>
</svg>"##;

pub const ICON_TREND_ANGLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 18L18 6"/>
  <path d="M4 18h14"/>
  <path d="M8 18a4 4 0 0 0 2.5-3.5"/>
  <circle cx="4" cy="18" r="2" fill="currentColor"/>
  <circle cx="18" cy="6" r="2" fill="currentColor"/>
</svg>"##;

// =============================================================================
// Channel Icons
// =============================================================================

pub const ICON_REGRESSION_TREND: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M3 18L21 6"/><path d="M3 14L21 2" stroke-dasharray="2 2"/><path d="M3 22L21 10" stroke-dasharray="2 2"/>
</svg>"##;

pub const ICON_FLAT_TOP_BOTTOM: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M3 6h18"/><path d="M3 18h18"/><path d="M3 6L10 12L3 18"/>
</svg>"##;

pub const ICON_DISJOINT_CHANNEL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M3 18L12 10"/><path d="M12 14L21 6"/><path d="M3 10L12 2"/><path d="M12 6L21 14"/>
</svg>"##;

// =============================================================================
// Pitchfork Icons
// =============================================================================

pub const ICON_PITCHFORK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M12 20V8"/>
  <path d="M6 2v6c0 2 2 3 6 3s6-1 6-3V2"/>
  <path d="M6 2v4"/>
  <path d="M12 2v4"/>
  <path d="M18 2v4"/>
</svg>"##;

// =============================================================================
// Fibonacci Icons
// =============================================================================

pub const ICON_FIB_RETRACEMENT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 4h16"/>
  <path d="M4 9h16" stroke-dasharray="4 2"/>
  <path d="M4 14h16" stroke-dasharray="4 2"/>
  <path d="M4 20h16"/>
  <path d="M4 4v16"/>
</svg>"##;

pub const ICON_FIB_EXTENSION: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 20L12 4L20 12"/>
  <path d="M4 8h16" stroke-dasharray="2 2"/>
  <path d="M4 14h16" stroke-dasharray="2 2"/>
</svg>"##;

pub const ICON_FIB_CHANNEL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M2 20L22 8"/>
  <path d="M2 14L22 2"/>
  <path d="M6 12L22 5" stroke-dasharray="2 2"/>
</svg>"##;

pub const ICON_FIB_CIRCLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="12" cy="12" r="9"/>
  <circle cx="12" cy="12" r="6" stroke-dasharray="2 2"/>
  <circle cx="12" cy="12" r="3" stroke-dasharray="2 2"/>
  <circle cx="12" cy="12" r="1" fill="currentColor"/>
</svg>"##;

pub const ICON_FIB_SPIRAL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M12 12a5 5 0 0 1 5 5"/>
  <path d="M12 12a8 8 0 0 0-8 8"/>
  <path d="M12 12a3 3 0 0 1-3-3"/>
  <path d="M12 12a2 2 0 0 0 2-2"/>
  <circle cx="12" cy="12" r="1" fill="currentColor"/>
</svg>"##;

pub const ICON_FIB_TIME_ZONES: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 4v16"/><path d="M8 4v16"/><path d="M13 4v16" stroke-dasharray="2 2"/><path d="M20 4v16" stroke-dasharray="2 2"/>
</svg>"##;

pub const ICON_FIB_SPEED_RESISTANCE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 20L20 4"/><path d="M4 20L20 10" stroke-dasharray="2 2"/><path d="M4 20L20 16" stroke-dasharray="2 2"/>
  <circle cx="4" cy="20" r="2" fill="currentColor"/>
</svg>"##;

pub const ICON_FIB_TREND_TIME: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 18L12 6L20 14"/><path d="M4 18v-14"/><path d="M12 6v12" stroke-dasharray="2 2"/>
</svg>"##;

pub const ICON_FIB_ARCS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 20Q4 8 20 4"/><path d="M4 20Q4 12 14 8" stroke-dasharray="2 2"/><path d="M4 20Q4 16 10 14" stroke-dasharray="2 2"/>
  <circle cx="4" cy="20" r="2" fill="currentColor"/>
</svg>"##;

pub const ICON_FIB_WEDGE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 20L20 4"/><path d="M4 20L20 12"/><path d="M4 20L20 8" stroke-dasharray="2 2"/>
  <circle cx="4" cy="20" r="2" fill="currentColor"/>
</svg>"##;

pub const ICON_FIB_FAN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 20L20 4"/><path d="M4 20L20 8" stroke-dasharray="2 2"/><path d="M4 20L20 12" stroke-dasharray="2 2"/><path d="M4 20L20 16" stroke-dasharray="2 2"/>
  <circle cx="4" cy="20" r="2" fill="currentColor"/>
</svg>"##;

// =============================================================================
// Gann Icons
// =============================================================================

pub const ICON_GANN_BOX: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="4" y="4" width="16" height="16"/><path d="M4 4L20 20"/><path d="M20 4L4 20"/><path d="M12 4v16"/><path d="M4 12h16"/>
</svg>"##;

pub const ICON_GANN_SQUARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="4" y="4" width="16" height="16"/><circle cx="12" cy="12" r="6" stroke-dasharray="2 2"/>
  <path d="M4 4L20 20" stroke-dasharray="2 2"/><path d="M20 4L4 20" stroke-dasharray="2 2"/>
</svg>"##;

pub const ICON_GANN_FAN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 20L20 4"/><path d="M4 20L20 8"/><path d="M4 20L20 12"/><path d="M4 20L20 16"/><path d="M4 20h16"/>
  <circle cx="4" cy="20" r="2" fill="currentColor"/>
</svg>"##;

// =============================================================================
// Pattern Icons
// =============================================================================

pub const ICON_XABCD_PATTERN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M2 18L6 6L12 14L16 8L22 16"/>
  <circle cx="2" cy="18" r="1.5" fill="currentColor"/><circle cx="6" cy="6" r="1.5" fill="currentColor"/>
  <circle cx="12" cy="14" r="1.5" fill="currentColor"/><circle cx="16" cy="8" r="1.5" fill="currentColor"/>
  <circle cx="22" cy="16" r="1.5" fill="currentColor"/>
</svg>"##;

pub const ICON_HEAD_SHOULDERS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M2 16L6 10L10 14L12 4L14 14L18 10L22 16"/>
  <path d="M6 14h12" stroke-dasharray="2 2"/>
</svg>"##;

pub const ICON_ABCD_PATTERN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 18L8 6L16 14L20 6"/>
  <circle cx="4" cy="18" r="1.5" fill="currentColor"/><circle cx="8" cy="6" r="1.5" fill="currentColor"/>
  <circle cx="16" cy="14" r="1.5" fill="currentColor"/><circle cx="20" cy="6" r="1.5" fill="currentColor"/>
</svg>"##;

pub const ICON_TRIANGLE_PATTERN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 8L8 16L12 8L16 16L20 12"/>
  <path d="M4 6L20 10" stroke-dasharray="2 2"/><path d="M4 18L20 14" stroke-dasharray="2 2"/>
</svg>"##;

pub const ICON_THREE_DRIVES: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M2 20L6 8L10 14L14 6L18 12L22 4"/>
  <circle cx="6" cy="8" r="1.5" fill="currentColor"/><circle cx="14" cy="6" r="1.5" fill="currentColor"/><circle cx="22" cy="4" r="1.5" fill="currentColor"/>
</svg>"##;

// =============================================================================
// Elliott Wave Icons
// =============================================================================

pub const ICON_ELLIOTT_WAVE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M2 18L5 14L8 16L12 4L15 12L18 10L22 14"/>
  <circle cx="5" cy="14" r="1.5" fill="currentColor"/>
  <circle cx="12" cy="4" r="1.5" fill="currentColor"/>
  <circle cx="22" cy="14" r="1.5" fill="currentColor"/>
</svg>"##;

// =============================================================================
// Cycle Icons
// =============================================================================

pub const ICON_CYCLE_LINES: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 4v16"/><path d="M12 4v16"/><path d="M20 4v16"/>
  <path d="M4 20h16"/>
</svg>"##;

pub const ICON_TIME_CYCLES: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="8" cy="12" r="5"/><circle cx="16" cy="12" r="5"/>
</svg>"##;

pub const ICON_SINE_WAVE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M2 12c2-4 4-4 6 0s4 4 6 0 4-4 6 0 4 4 6 0"/>
</svg>"##;

// =============================================================================
// Shape Icons
// =============================================================================

pub const ICON_RECTANGLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="4" y="6" width="16" height="12" rx="1"/>
</svg>"##;

pub const ICON_ROTATED_RECTANGLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M6 4L20 8L18 20L4 16L6 4z"/>
</svg>"##;

pub const ICON_CIRCLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="12" cy="12" r="9"/>
</svg>"##;

pub const ICON_ELLIPSE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M5 12a7 4 0 1 0 14 0a7 4 0 1 0 -14 0"/>
</svg>"##;

pub const ICON_TRIANGLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M12 4L21 20H3L12 4z"/>
</svg>"##;

pub const ICON_ARC: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 18Q12 2 20 18"/>
  <circle cx="4" cy="18" r="2" fill="currentColor"/><circle cx="20" cy="18" r="2" fill="currentColor"/>
</svg>"##;

pub const ICON_POLYLINE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 18L8 8L14 14L20 6"/>
  <circle cx="4" cy="18" r="1.5" fill="currentColor"/><circle cx="8" cy="8" r="1.5" fill="currentColor"/>
  <circle cx="14" cy="14" r="1.5" fill="currentColor"/><circle cx="20" cy="6" r="1.5" fill="currentColor"/>
</svg>"##;

pub const ICON_PATH: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 18Q8 4 12 12T20 6"/>
</svg>"##;

pub const ICON_CURVE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 18Q12 2 20 18"/>
  <path d="M4 18L12 2" stroke-dasharray="2 2"/><path d="M12 2L20 18" stroke-dasharray="2 2"/>
  <circle cx="4" cy="18" r="1.5" fill="currentColor"/><circle cx="12" cy="2" r="1.5" fill="currentColor"/><circle cx="20" cy="18" r="1.5" fill="currentColor"/>
</svg>"##;

pub const ICON_DOUBLE_CURVE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 12Q8 4 12 12T20 12"/>
  <path d="M4 16Q8 8 12 16T20 16" stroke-dasharray="2 2"/>
</svg>"##;

// =============================================================================
// Arrow Icons
// =============================================================================

pub const ICON_ARROW: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M5 19L19 5"/>
  <path d="M19 5h-6"/>
  <path d="M19 5v6"/>
</svg>"##;

pub const ICON_ARROW_UP: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="1" stroke-linecap="round" stroke-linejoin="round">
  <path d="M12 4l8 14H4z"/>
</svg>"##;

pub const ICON_ARROW_DOWN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="1" stroke-linecap="round" stroke-linejoin="round">
  <path d="M12 20l8-14H4z"/>
</svg>"##;

// =============================================================================
// Brush Icons
// =============================================================================

pub const ICON_BRUSH: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M12 19l7-7 3 3-7 7-3-3z" fill="currentColor"/>
  <path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/>
  <path d="M2 2l7.586 7.586"/>
</svg>"##;

pub const ICON_HIGHLIGHTER: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M14 3l7 7-10 10H4v-7L14 3z" fill="currentColor" fill-opacity="0.3"/>
  <path d="M14 3l7 7-10 10H4v-7L14 3z"/>
</svg>"##;

// =============================================================================
// Annotation Icons
// =============================================================================

pub const ICON_TEXT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 6h16"/>
  <path d="M12 6v14"/>
  <path d="M8 20h8"/>
</svg>"##;

pub const ICON_ANCHORED_TEXT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 6h16"/><path d="M12 6v14"/><path d="M8 20h8"/>
  <circle cx="12" cy="6" r="2" fill="currentColor"/>
</svg>"##;

pub const ICON_NOTE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/>
  <path d="M14 3v6h6"/>
</svg>"##;

pub const ICON_PRICE_NOTE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/>
  <path d="M14 3v6h6"/><path d="M8 13h8"/><path d="M8 17h4"/>
</svg>"##;

pub const ICON_SIGNPOST: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M12 3v18"/><path d="M6 7h10l2 2-2 2H6V7z"/>
</svg>"##;

pub const ICON_TABLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 10h18"/><path d="M3 16h18"/><path d="M10 4v16"/>
</svg>"##;

pub const ICON_CALLOUT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5z"/>
</svg>"##;

pub const ICON_COMMENT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>
</svg>"##;

pub const ICON_PRICE_LABEL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 9h12l4 3-4 3H4z"/>
  <path d="M4 9v6"/>
</svg>"##;

pub const ICON_SIGN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="3" y="6" width="18" height="12" rx="2"/><path d="M12 6v-3"/><path d="M12 21v-3"/>
</svg>"##;

pub const ICON_FLAG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M8 21V4"/><path d="M8 4l12 4-12 4"/>
</svg>"##;

pub const ICON_DIAMOND: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor" stroke="none">
  <path d="M12 2L22 12L12 22L2 12Z"/>
</svg>"##;

pub const ICON_EMOJI: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="12" cy="12" r="10"/><path d="M8 14s1.5 2 4 2 4-2 4-2"/><line x1="9" y1="9" x2="9.01" y2="9"/><line x1="15" y1="9" x2="15.01" y2="9"/>
</svg>"##;

pub const ICON_IMAGE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/>
</svg>"##;

// =============================================================================
// Measurement Icons
// =============================================================================

pub const ICON_PRICE_RANGE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M12 4v16"/>
  <path d="M8 8l4-4 4 4"/>
  <path d="M8 16l4 4 4-4"/>
  <path d="M4 12h4"/>
  <path d="M16 12h4"/>
</svg>"##;

pub const ICON_DATE_RANGE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 12h16"/>
  <path d="M8 8l-4 4 4 4"/>
  <path d="M16 8l4 4-4 4"/>
  <path d="M12 4v4"/>
  <path d="M12 16v4"/>
</svg>"##;

pub const ICON_PRICE_DATE_RANGE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="4" y="4" width="16" height="16" rx="1" stroke-dasharray="4 2"/>
  <path d="M4 4l16 16"/>
  <circle cx="4" cy="4" r="2" fill="currentColor"/>
  <circle cx="20" cy="20" r="2" fill="currentColor"/>
</svg>"##;

// =============================================================================
// Volume Icons
// =============================================================================

pub const ICON_VOLUME_PROFILE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="4" y="4" width="8" height="3" fill="currentColor" fill-opacity="0.5"/>
  <rect x="4" y="8" width="14" height="3" fill="currentColor"/>
  <rect x="4" y="12" width="10" height="3" fill="currentColor" fill-opacity="0.5"/>
  <rect x="4" y="16" width="6" height="3" fill="currentColor" fill-opacity="0.3"/>
  <path d="M20 4v16"/>
</svg>"##;

// =============================================================================
// Projection Icons
// =============================================================================

pub const ICON_BARS_PATTERN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="4" y="8" width="3" height="8"/>
  <rect x="9" y="6" width="3" height="10"/>
  <rect x="14" y="10" width="3" height="8" stroke-dasharray="2 2"/>
  <rect x="19" y="7" width="3" height="9" stroke-dasharray="2 2"/>
</svg>"##;

pub const ICON_PRICE_PROJECTION: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 18l6-8 4 4 6-10"/>
  <path d="M4 6h16" stroke-dasharray="4 2"/>
  <path d="M20 4v4h-4"/>
</svg>"##;

pub const ICON_PROJECTION: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M3 20l7-7"/>
  <path d="M10 13l4 4" stroke-dasharray="2 2"/>
  <path d="M14 17l4-8" stroke-dasharray="2 2"/>
  <circle cx="3" cy="20" r="2" fill="currentColor"/>
  <circle cx="10" cy="13" r="1.5" fill="currentColor"/>
</svg>"##;

// =============================================================================
// Tool Icons
// =============================================================================

pub const ICON_CROSSHAIR: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M12 3v18"/>
  <path d="M3 12h18"/>
</svg>"##;

pub const ICON_MAGNET: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 3h4v4H4z" fill="currentColor"/>
  <path d="M16 3h4v4h-4z" fill="currentColor"/>
  <path d="M4 7v5a8 8 0 0 0 16 0V7"/>
  <path d="M8 7v5a4 4 0 0 0 8 0V7"/>
</svg>"##;

/// Strong magnet icon — same U-shape as ICON_MAGNET but with an electric discharge
/// (zigzag lightning bolt) between the two pole tips, indicating strong body-snap mode.
pub const ICON_MAGNET_STRONG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 3h4v4H4z" fill="currentColor"/>
  <path d="M16 3h4v4h-4z" fill="currentColor"/>
  <path d="M4 7v5a8 8 0 0 0 16 0V7"/>
  <path d="M8 7v5a4 4 0 0 0 8 0V7"/>
  <polyline points="8,7 10,4 12,7 14,4 16,7" stroke-width="1.5"/>
</svg>"##;

pub const ICON_CURSOR: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 4l7 17 2-7 7-2L4 4z" fill="currentColor"/>
</svg>"##;

pub const ICON_HAND: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M18 11V6a2 2 0 0 0-4 0"/>
  <path d="M14 10V4a2 2 0 0 0-4 0v6"/>
  <path d="M10 10.5V6a2 2 0 0 0-4 0v8"/>
  <path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.9-5.9-2.4L3.3 16"/>
</svg>"##;

// =============================================================================
// Action Icons
// =============================================================================

pub const ICON_UNDO: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 7h10a5 5 0 0 1 5 5v0a5 5 0 0 1-5 5H9"/>
  <path d="M7 4l-4 3 4 3"/>
</svg>"##;

pub const ICON_REDO: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M20 7H10a5 5 0 0 0-5 5v0a5 5 0 0 0 5 5h5"/>
  <path d="M17 4l4 3-4 3"/>
</svg>"##;

pub const ICON_DELETE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M3 6h18"/>
  <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/>
  <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
</svg>"##;

pub const ICON_LOCK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
  <rect x="5" y="11" width="14" height="10" rx="2"/>
  <path d="M7 11V7a5 5 0 0 1 10 0v4"/>
</svg>"##;

pub const ICON_UNLOCK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="5" y="11" width="14" height="10" rx="2"/>
  <path d="M7 11V7a5 5 0 0 1 9.9-1"/>
</svg>"##;

pub const ICON_EYE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7z"/>
  <circle cx="12" cy="12" r="3"/>
  <path d="M7 7L5.5 4.5"/>
  <path d="M12 5V2"/>
  <path d="M17 7l1.5-2.5"/>
</svg>"##;

pub const ICON_SETTINGS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="12" cy="12" r="3"/>
  <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/>
</svg>"##;

pub const ICON_BOOKMARK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/>
</svg>"##;

pub const ICON_EXPAND: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M0 0l8 8"/>
  <path d="M0 0h8"/>
  <path d="M0 0v8"/>
  <path d="M24 0l-8 8"/>
  <path d="M24 0h-8"/>
  <path d="M24 0v8"/>
  <path d="M0 24l8-8"/>
  <path d="M0 24h8"/>
  <path d="M0 24v-8"/>
  <path d="M24 24l-8-8"/>
  <path d="M24 24h-8"/>
  <path d="M24 24v-8"/>
</svg>"##;

pub const ICON_MOVE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M5 9l-3 3 3 3"/>
  <path d="M9 5l3-3 3 3"/>
  <path d="M15 19l-3 3-3-3"/>
  <path d="M19 9l3 3-3 3"/>
  <line x1="2" y1="12" x2="22" y2="12"/>
  <line x1="12" y1="2" x2="12" y2="22"/>
</svg>"##;

// =============================================================================
// Position Icons
// =============================================================================

pub const ICON_LONG_POSITION: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M12 20V4"/>
  <path d="M5 11l7-7 7 7"/>
  <path d="M5 20h14"/>
</svg>"##;

pub const ICON_SHORT_POSITION: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M12 4v16"/>
  <path d="M5 13l7 7 7-7"/>
  <path d="M5 4h14"/>
</svg>"##;

// =============================================================================
// Navigation Icons
// =============================================================================

pub const ICON_PLUS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M12 5v14"/>
  <path d="M5 12h14"/>
</svg>"##;

pub const ICON_INDICATORS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M3 8l4-3 5 4 5-5 4 3"/>
  <rect x="4" y="14" width="3" height="7" fill="currentColor" stroke="none"/>
  <rect x="10" y="16" width="3" height="5" fill="currentColor" stroke="none"/>
  <rect x="16" y="12" width="3" height="9" fill="currentColor" stroke="none"/>
</svg>"##;

pub const ICON_CHEVRON_DOWN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M6 9l6 6 6-6"/>
</svg>"##;

// =============================================================================
// Icon lookup function
// =============================================================================

/// Resolve an icon name (as used in `ToolbarIconId`) to its SVG string.
///
/// The name is normalized to lowercase with hyphens replaced by underscores
/// before matching, so `"TrendLine"`, `"trend-line"`, and `"trend_line"` all
/// resolve to the same icon.
// =============================================================================
// Additional SVG constants not yet present in chart (moved from core)
// =============================================================================

pub const ICON_STEP_LINE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M3 18h4v-4h4v-2h4v-4h4v-2h2"/>
</svg>"##;

pub const ICON_LINE_MARKERS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M3 17l6-6 4 4 8-8"/>
  <circle cx="3" cy="17" r="2" fill="currentColor"/>
  <circle cx="9" cy="11" r="2" fill="currentColor"/>
  <circle cx="13" cy="15" r="2" fill="currentColor"/>
  <circle cx="21" cy="7" r="2" fill="currentColor"/>
</svg>"##;

pub const ICON_HLC_AREA: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M3 8l5 2 5-4 5 3 3-2v10l-3 2-5-3-5 4-5-2z" fill="currentColor" fill-opacity="0.3"/>
  <path d="M3 12l5 1 5-2 5 2 3-1"/>
</svg>"##;

pub const ICON_COLUMNS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="3" y="10" width="3" height="10" fill="currentColor"/>
  <rect x="8" y="6" width="3" height="14" fill="currentColor"/>
  <rect x="13" y="14" width="3" height="6" fill="currentColor"/>
  <rect x="18" y="8" width="3" height="12" fill="currentColor"/>
</svg>"##;

pub const ICON_ZOOM: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="10" cy="10" r="7"/>
  <path d="M21 21l-4.35-4.35"/>
  <path d="M10 7v6"/>
  <path d="M7 10h6"/>
</svg>"##;

pub const ICON_EYE_OFF: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7z"/>
  <path d="M7 17l-1.5 2.5"/>
  <path d="M12 19v3"/>
  <path d="M17 17l1.5 2.5"/>
</svg>"##;

pub const ICON_COPY: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="9" y="9" width="13" height="13" rx="2"/>
  <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
</svg>"##;

pub const ICON_CLOSE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <line x1="18" y1="6" x2="6" y2="18"/>
  <line x1="6" y1="6" x2="18" y2="18"/>
</svg>"##;

pub const ICON_CHEVRON_UP: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M18 15l-6-6-6 6"/>
</svg>"##;

pub const ICON_CHEVRON_RIGHT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M9 6l6 6-6 6"/>
</svg>"##;

pub const ICON_MINUS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M5 12h14"/>
</svg>"##;

pub const ICON_GRID: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="3" y="3" width="7" height="7"/>
  <rect x="14" y="3" width="7" height="7"/>
  <rect x="14" y="14" width="7" height="7"/>
  <rect x="3" y="14" width="7" height="7"/>
</svg>"##;

pub const ICON_LAYERS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <polygon points="12 2 2 7 12 12 22 7 12 2"/>
  <polyline points="2 17 12 22 22 17"/>
  <polyline points="2 12 12 17 22 12"/>
</svg>"##;

pub const ICON_LAYOUT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="3" y="3" width="18" height="18" rx="2" ry="2"/>
</svg>"##;

pub const ICON_SEARCH: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="11" cy="11" r="8"/>
  <path d="M21 21l-4.35-4.35"/>
</svg>"##;

pub const ICON_CLOCK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="12" cy="12" r="10"/>
  <path d="M12 6v6l4 2"/>
</svg>"##;

pub const ICON_WATERMARK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="3" y="3" width="18" height="18" rx="2"/>
  <path d="M7 12h10" stroke-opacity="0.5"/>
  <path d="M12 7v10" stroke-opacity="0.5"/>
</svg>"##;

pub const ICON_LEGEND: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="3" y="3" width="18" height="18" rx="2"/>
  <path d="M7 8h2"/>
  <path d="M11 8h6"/>
  <path d="M7 12h2"/>
  <path d="M11 12h6"/>
  <path d="M7 16h2"/>
  <path d="M11 16h6"/>
</svg>"##;

pub const ICON_TOOLTIP: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="12" cy="12" r="10"/>
  <path d="M12 16v-4"/>
  <circle cx="12" cy="8" r="0.5" fill="currentColor"/>
</svg>"##;

pub const ICON_WATCHLIST: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="3" y="3" width="18" height="18" rx="2"/>
  <path d="M7 7h6"/>
  <path d="M7 11h10"/>
  <path d="M7 15h8"/>
</svg>"##;

pub const ICON_ALERT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9"/>
  <path d="M13.73 21a2 2 0 0 1-3.46 0"/>
</svg>"##;

pub const ICON_TRADING: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M12 2v20"/>
  <path d="M17 5H9.5a3.5 3.5 0 0 0 0 7h5a3.5 3.5 0 0 1 0 7H6"/>
</svg>"##;

pub const ICON_POSITIONS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="2" y="7" width="20" height="14" rx="2"/>
  <path d="M16 7V5a2 2 0 0 0-2-2h-4a2 2 0 0 0-2 2v2"/>
  <path d="M12 12v4"/>
  <path d="M2 12h20"/>
</svg>"##;

pub const ICON_PANEL_RIGHT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="3" y="3" width="18" height="18" rx="2"/>
  <path d="M15 3v18"/>
</svg>"##;

pub const ICON_PANEL_BOTTOM: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="3" y="3" width="18" height="18" rx="2"/>
  <path d="M3 15h18"/>
</svg>"##;

pub const ICON_PALETTE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="13.5" cy="6.5" r="2"/>
  <circle cx="17.5" cy="10.5" r="2"/>
  <circle cx="8.5" cy="7.5" r="2"/>
  <circle cx="6.5" cy="12.5" r="2"/>
  <path d="M12 2C6.5 2 2 6.5 2 12s4.5 10 10 10c.926 0 1.648-.746 1.648-1.688 0-.437-.18-.835-.437-1.125-.29-.289-.438-.652-.438-1.125a1.64 1.64 0 0 1 1.668-1.668h1.996c3.051 0 5.555-2.503 5.555-5.555C21.965 6.012 17.461 2 12 2z"/>
</svg>"##;

pub const ICON_INFO: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="12" cy="12" r="10"/>
  <path d="M12 16v-4"/>
  <path d="M12 8h.01"/>
</svg>"##;

pub const ICON_SIGNAL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M3 16l4-6 5 8 5-12 4 6"/>
  <polygon points="3,19 1,23 5,23" fill="currentColor" stroke="none"/>
  <polygon points="17,4 15,0 19,0" fill="currentColor" stroke="none"/>
</svg>"##;

pub const ICON_OBJECT_TREE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <polygon points="12 2 2 7 12 12 22 7 12 2"/>
  <polyline points="2 17 12 22 22 17"/>
  <polyline points="2 12 12 17 22 12"/>
</svg>"##;

pub const ICON_ZOOM_IN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
<circle cx="11" cy="11" r="8"/>
<line x1="21" y1="21" x2="16.65" y2="16.65"/>
<line x1="11" y1="8" x2="11" y2="14"/>
<line x1="8" y1="11" x2="14" y2="11"/>
</svg>"##;

pub const ICON_ZOOM_OUT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
<circle cx="11" cy="11" r="8"/>
<line x1="21" y1="21" x2="16.65" y2="16.65"/>
<line x1="8" y1="11" x2="14" y2="11"/>
</svg>"##;

pub const ICON_ZOOM_RESET: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
<polyline points="15 3 21 3 21 9"/>
<polyline points="9 21 3 21 3 15"/>
<line x1="21" y1="3" x2="14" y2="10"/>
<line x1="3" y1="21" x2="10" y2="14"/>
</svg>"##;

pub const ICON_SCREENSHOT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
<path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/>
<circle cx="12" cy="13" r="4"/>
</svg>"##;

pub const ICON_CIRCUIT_BOARD: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="3" y="3" width="18" height="18" rx="2"/>
  <path d="M11 9h4a2 2 0 0 0 2-2V3"/>
  <circle cx="9" cy="9" r="2"/>
  <path d="M7 21v-4a2 2 0 0 1 2-2h4"/>
  <circle cx="15" cy="15" r="2"/>
</svg>"##;

/// CPU chip with pins — processor icon for Performance panel
pub const ICON_CPU: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="4" y="4" width="16" height="16" rx="2"/>
  <rect x="9" y="9" width="6" height="6" rx="1"/>
  <path d="M9 1v3"/><path d="M15 1v3"/>
  <path d="M9 20v3"/><path d="M15 20v3"/>
  <path d="M20 9h3"/><path d="M20 14h3"/>
  <path d="M1 9h3"/><path d="M1 14h3"/>
</svg>"##;

pub const ICON_NEW_WINDOW: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M15 3L21 3L21 9"/>
  <path d="M21 3L10 14"/>
  <path d="M18 13L18 19C18 20.1046 17.1046 21 16 21L5 21C3.89543 21 3 20.1046 3 19L3 8C3 6.89543 3.89543 6 5 6L11 6"/>
</svg>"##;

// =============================================================================
// User / Auth / Cloud Icons
// =============================================================================

pub const ICON_CLOUD: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 10h-1.26A8 8 0 1 0 9 20h9a5 5 0 0 0 0-10z"/></svg>"##;

pub const ICON_CLOUD_DOWNLOAD: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="8 17 12 21 16 17"/><line x1="12" y1="12" x2="12" y2="21"/><path d="M20.88 18.09A5 5 0 0 0 18 9h-1.26A8 8 0 1 0 3 16.29"/></svg>"##;

pub const ICON_USER: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>"##;

pub const ICON_LOG_IN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4"/><polyline points="10 17 15 12 10 7"/><line x1="15" y1="12" x2="3" y2="12"/></svg>"##;

pub const ICON_LOG_OUT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" y1="12" x2="9" y2="12"/></svg>"##;

pub const ICON_CHEVRON_LEFT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 18l-6-6 6-6"/></svg>"##;

pub const ICON_REFRESH: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/></svg>"##;

pub const ICON_SHIELD: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>"##;

pub const ICON_SHIELD_CHECK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><path d="M9 12l2 2 4-4"/></svg>"##;

pub const ICON_GLOBE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="2" y1="12" x2="22" y2="12"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/></svg>"##;

pub const ICON_KEY: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4"/></svg>"##;

pub const ICON_MENU: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 6h16"/>
  <path d="M4 12h16"/>
  <path d="M4 18h16"/>
</svg>"##;

pub const ICON_LINE_SOLID: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M3 12h18"/>
</svg>"##;

pub const ICON_LINE_DASHED: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M3 12h4"/>
  <path d="M10 12h4"/>
  <path d="M17 12h4"/>
</svg>"##;

pub const ICON_LINE_DOTTED: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="4" cy="12" r="1" fill="currentColor"/>
  <circle cx="8" cy="12" r="1" fill="currentColor"/>
  <circle cx="12" cy="12" r="1" fill="currentColor"/>
  <circle cx="16" cy="12" r="1" fill="currentColor"/>
  <circle cx="20" cy="12" r="1" fill="currentColor"/>
</svg>"##;

pub const ICON_PENCIL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M17 3a2.85 2.85 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"/>
  <path d="m15 5 4 4"/>
</svg>"##;

pub const ICON_COLOR_FILL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="m19 11-8-8-8.6 8.6a2 2 0 0 0 0 2.8l5.2 5.2c.8.8 2 .8 2.8 0L19 11Z"/>
  <path d="m5 2 5 5"/>
  <path d="M2 13h15"/>
  <path d="M22 20a2 2 0 1 1-4 0c0-1.6 1.7-2.4 2-4 .3 1.6 2 2.4 2 4Z"/>
</svg>"##;

pub const ICON_TEXT_COLOR: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M4 20h16"/>
  <path d="m6 16 6-12 6 12"/>
  <path d="M8 12h8"/>
</svg>"##;

pub const ICON_LINE_WIDTH_1: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round">
  <path d="M4 12h16" stroke-width="1"/>
</svg>"##;

pub const ICON_LINE_WIDTH_2: &str = r##"<svg xmlns="http://www.w3.org/2020/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round">
  <path d="M4 12h16" stroke-width="2"/>
</svg>"##;

pub const ICON_LINE_WIDTH_3: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round">
  <path d="M4 12h16" stroke-width="3"/>
</svg>"##;

pub const ICON_LINE_WIDTH_4: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round">
  <path d="M4 12h16" stroke-width="4"/>
</svg>"##;

pub const ICON_MORE_HORIZONTAL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor">
  <circle cx="5" cy="12" r="2"/>
  <circle cx="12" cy="12" r="2"/>
  <circle cx="19" cy="12" r="2"/>
</svg>"##;

pub const ICON_LAYOUT_SINGLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
  <rect x="3" y="3" width="18" height="18" rx="1"/>
</svg>"##;

pub const ICON_LAYOUT_SPLIT_H: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
  <rect x="3" y="3" width="8" height="18" rx="1"/>
  <rect x="13" y="3" width="8" height="18" rx="1"/>
</svg>"##;

pub const ICON_LAYOUT_SPLIT_V: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
  <rect x="3" y="3" width="18" height="8" rx="1"/>
  <rect x="3" y="13" width="18" height="8" rx="1"/>
</svg>"##;

pub const ICON_LAYOUT_GRID_2X2: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
  <rect x="3" y="3" width="8" height="8" rx="1"/>
  <rect x="13" y="3" width="8" height="8" rx="1"/>
  <rect x="3" y="13" width="8" height="8" rx="1"/>
  <rect x="13" y="13" width="8" height="8" rx="1"/>
</svg>"##;

pub const ICON_LAYOUT_2LEFT_1RIGHT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
  <rect x="3" y="3" width="8" height="8" rx="1"/>
  <rect x="3" y="13" width="8" height="8" rx="1"/>
  <rect x="13" y="3" width="8" height="18" rx="1"/>
</svg>"##;

pub const ICON_LAYOUT_1LEFT_2RIGHT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
  <rect x="3" y="3" width="8" height="18" rx="1"/>
  <rect x="13" y="3" width="8" height="8" rx="1"/>
  <rect x="13" y="13" width="8" height="8" rx="1"/>
</svg>"##;

pub const ICON_LAYOUT_2TOP_1BOTTOM: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
  <rect x="3" y="3" width="8" height="8" rx="1"/>
  <rect x="13" y="3" width="8" height="8" rx="1"/>
  <rect x="3" y="13" width="18" height="8" rx="1"/>
</svg>"##;

pub const ICON_LAYOUT_1TOP_2BOTTOM: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
  <rect x="3" y="3" width="18" height="8" rx="1"/>
  <rect x="3" y="13" width="8" height="8" rx="1"/>
  <rect x="13" y="13" width="8" height="8" rx="1"/>
</svg>"##;

pub const ICON_LAYOUT_3COLUMNS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
  <rect x="3" y="3" width="5" height="18" rx="1"/>
  <rect x="9.5" y="3" width="5" height="18" rx="1"/>
  <rect x="16" y="3" width="5" height="18" rx="1"/>
</svg>"##;

pub const ICON_LAYOUT_3ROWS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
  <rect x="3" y="3" width="18" height="5" rx="1"/>
  <rect x="3" y="9.5" width="18" height="5" rx="1"/>
  <rect x="3" y="16" width="18" height="5" rx="1"/>
</svg>"##;

pub const ICON_LAYOUT_1BIG_3SMALL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
  <rect x="3" y="3" width="12" height="18" rx="1"/>
  <rect x="17" y="3" width="4" height="5" rx="1"/>
  <rect x="17" y="9.5" width="4" height="5" rx="1"/>
  <rect x="17" y="16" width="4" height="5" rx="1"/>
</svg>"##;

pub const ICON_COLLAPSE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M8 8L0 0"/>
  <path d="M8 8H0"/>
  <path d="M8 8V0"/>
  <path d="M16 8l8-8"/>
  <path d="M16 8h8"/>
  <path d="M16 8V0"/>
  <path d="M8 16L0 24"/>
  <path d="M8 16H0"/>
  <path d="M8 16v8"/>
  <path d="M16 16l8 8"/>
  <path d="M16 16h8"/>
  <path d="M16 16v8"/>
</svg>"##;

// =============================================================================
// Star Icons (Watchlist)
// =============================================================================

pub const ICON_STAR: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z"/></svg>"##;

pub const ICON_STAR_FILLED: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z"/></svg>"##;

// =============================================================================
// Icon enum implementation (svg lookup, from_name)
// =============================================================================

impl Icon {
    /// Get the SVG content for this icon.
    pub fn svg(&self) -> &'static str {
        match self {
            Icon::Candlestick => ICON_CANDLESTICK,
            Icon::HollowCandles => ICON_HOLLOW_CANDLES,
            Icon::HeikinAshi => ICON_HEIKIN_ASHI,
            Icon::LineChart => ICON_LINE_CHART,
            Icon::StepLine => ICON_STEP_LINE,
            Icon::LineWithMarkers => ICON_LINE_MARKERS,
            Icon::AreaChart => ICON_AREA_CHART,
            Icon::HlcArea => ICON_HLC_AREA,
            Icon::BarChart => ICON_BAR_CHART,
            Icon::Histogram => ICON_HISTOGRAM,
            Icon::Columns => ICON_COLUMNS,
            Icon::Baseline => ICON_BASELINE,
            Icon::TrendLine => ICON_TREND_LINE,
            Icon::HorizontalLine => ICON_HORIZONTAL_LINE,
            Icon::VerticalLine => ICON_VERTICAL_LINE,
            Icon::Ray => ICON_RAY,
            Icon::ExtendedLine => ICON_EXTENDED_LINE,
            Icon::ParallelChannel => ICON_PARALLEL_CHANNEL,
            Icon::HorizontalRay => ICON_HORIZONTAL_RAY,
            Icon::CrossLine => ICON_CROSS_LINE,
            Icon::InfoLine => ICON_INFO_LINE,
            Icon::TrendAngle => ICON_TREND_ANGLE,
            Icon::RegressionTrend => ICON_REGRESSION_TREND,
            Icon::FlatTopBottom => ICON_FLAT_TOP_BOTTOM,
            Icon::DisjointChannel => ICON_DISJOINT_CHANNEL,
            Icon::Pitchfork => ICON_PITCHFORK,
            Icon::SchiffPitchfork => ICON_PITCHFORK,
            Icon::ModifiedSchiff => ICON_PITCHFORK,
            Icon::InsidePitchfork => ICON_PITCHFORK,
            Icon::FibRetracement => ICON_FIB_RETRACEMENT,
            Icon::FibExtension => ICON_FIB_EXTENSION,
            Icon::FibChannel => ICON_FIB_CHANNEL,
            Icon::FibCircle => ICON_FIB_CIRCLE,
            Icon::FibSpiral => ICON_FIB_SPIRAL,
            Icon::FibTimeZones => ICON_FIB_TIME_ZONES,
            Icon::FibSpeedResistance => ICON_FIB_SPEED_RESISTANCE,
            Icon::FibTrendTime => ICON_FIB_TREND_TIME,
            Icon::FibArcs => ICON_FIB_ARCS,
            Icon::FibWedge => ICON_FIB_WEDGE,
            Icon::FibFan => ICON_FIB_FAN,
            Icon::GannBox => ICON_GANN_BOX,
            Icon::GannSquare => ICON_GANN_SQUARE,
            Icon::GannFan => ICON_GANN_FAN,
            Icon::XabcdPattern => ICON_XABCD_PATTERN,
            Icon::CypherPattern => ICON_XABCD_PATTERN,
            Icon::HeadShoulders => ICON_HEAD_SHOULDERS,
            Icon::AbcdPattern => ICON_ABCD_PATTERN,
            Icon::TrianglePattern => ICON_TRIANGLE_PATTERN,
            Icon::ThreeDrives => ICON_THREE_DRIVES,
            Icon::ElliottImpulse => ICON_ELLIOTT_WAVE,
            Icon::ElliottCorrection => ICON_ELLIOTT_WAVE,
            Icon::ElliottTriangle => ICON_ELLIOTT_WAVE,
            Icon::ElliottCombo => ICON_ELLIOTT_WAVE,
            Icon::CycleLines => ICON_CYCLE_LINES,
            Icon::TimeCycles => ICON_TIME_CYCLES,
            Icon::SineWave => ICON_SINE_WAVE,
            Icon::Rectangle => ICON_RECTANGLE,
            Icon::RotatedRectangle => ICON_ROTATED_RECTANGLE,
            Icon::Circle => ICON_CIRCLE,
            Icon::Ellipse => ICON_ELLIPSE,
            Icon::Triangle => ICON_TRIANGLE,
            Icon::Arc => ICON_ARC,
            Icon::Polyline => ICON_POLYLINE,
            Icon::Path => ICON_PATH,
            Icon::Curve => ICON_CURVE,
            Icon::DoubleCurve => ICON_DOUBLE_CURVE,
            Icon::Arrow => ICON_ARROW,
            Icon::ArrowUp => ICON_ARROW_UP,
            Icon::ArrowDown => ICON_ARROW_DOWN,
            Icon::Brush => ICON_BRUSH,
            Icon::Highlighter => ICON_HIGHLIGHTER,
            Icon::Text => ICON_TEXT,
            Icon::AnchoredText => ICON_ANCHORED_TEXT,
            Icon::Note => ICON_NOTE,
            Icon::PriceNote => ICON_PRICE_NOTE,
            Icon::Signpost => ICON_SIGNPOST,
            Icon::Callout => ICON_CALLOUT,
            Icon::Comment => ICON_COMMENT,
            Icon::PriceLabel => ICON_PRICE_LABEL,
            Icon::Sign => ICON_SIGN,
            Icon::Flag => ICON_FLAG,
            Icon::Diamond => ICON_DIAMOND,
            Icon::Table => ICON_TABLE,
            Icon::Emoji => ICON_EMOJI,
            Icon::Image => ICON_IMAGE,
            Icon::PriceRange => ICON_PRICE_RANGE,
            Icon::DateRange => ICON_DATE_RANGE,
            Icon::PriceDateRange => ICON_PRICE_DATE_RANGE,
            Icon::VolumeProfile => ICON_VOLUME_PROFILE,
            Icon::BarsPattern => ICON_BARS_PATTERN,
            Icon::PriceProjection => ICON_PRICE_PROJECTION,
            Icon::Projection => ICON_PROJECTION,
            Icon::Crosshair => ICON_CROSSHAIR,
            Icon::Magnet => ICON_MAGNET,
            Icon::Cursor => ICON_CURSOR,
            Icon::Hand => ICON_HAND,
            Icon::Zoom => ICON_ZOOM,
            Icon::Undo => ICON_UNDO,
            Icon::Redo => ICON_REDO,
            Icon::Delete => ICON_DELETE,
            Icon::Lock => ICON_LOCK,
            Icon::Unlock => ICON_UNLOCK,
            Icon::Eye => ICON_EYE,
            Icon::EyeOff => ICON_EYE_OFF,
            Icon::Copy => ICON_COPY,
            Icon::Settings => ICON_SETTINGS,
            Icon::Close => ICON_CLOSE,
            Icon::Star => ICON_STAR,
            Icon::StarFilled => ICON_STAR_FILLED,
            Icon::LongPosition => ICON_LONG_POSITION,
            Icon::ShortPosition => ICON_SHORT_POSITION,
            Icon::ChevronUp => ICON_CHEVRON_UP,
            Icon::ChevronDown => ICON_CHEVRON_DOWN,
            Icon::ChevronRight => ICON_CHEVRON_RIGHT,
            Icon::Plus => ICON_PLUS,
            Icon::Minus => ICON_MINUS,
            Icon::Grid => ICON_GRID,
            Icon::Layers => ICON_LAYERS,
            Icon::Indicators => ICON_INDICATORS,
            Icon::Layout => ICON_LAYOUT,
            Icon::Search => ICON_SEARCH,
            Icon::Clock => ICON_CLOCK,
            Icon::Watermark => ICON_WATERMARK,
            Icon::Legend => ICON_LEGEND,
            Icon::Tooltip => ICON_TOOLTIP,
            Icon::Watchlist => ICON_WATCHLIST,
            Icon::Alert => ICON_ALERT,
            Icon::Trading => ICON_TRADING,
            Icon::Positions => ICON_POSITIONS,
            Icon::PanelRight => ICON_PANEL_RIGHT,
            Icon::PanelBottom => ICON_PANEL_BOTTOM,
            Icon::Palette => ICON_PALETTE,
            Icon::Info => ICON_INFO,
            Icon::Signal => ICON_SIGNAL,
            Icon::Menu => ICON_MENU,
            Icon::LineSolid => ICON_LINE_SOLID,
            Icon::LineDashed => ICON_LINE_DASHED,
            Icon::LineDotted => ICON_LINE_DOTTED,
            Icon::Pencil => ICON_PENCIL,
            Icon::ColorFill => ICON_COLOR_FILL,
            Icon::TextColor => ICON_TEXT_COLOR,
            Icon::LineWidth1 => ICON_LINE_WIDTH_1,
            Icon::LineWidth2 => ICON_LINE_WIDTH_2,
            Icon::LineWidth3 => ICON_LINE_WIDTH_3,
            Icon::LineWidth4 => ICON_LINE_WIDTH_4,
            Icon::MoreHorizontal => ICON_MORE_HORIZONTAL,
            Icon::LayoutSingle => ICON_LAYOUT_SINGLE,
            Icon::LayoutSplitH => ICON_LAYOUT_SPLIT_H,
            Icon::LayoutSplitV => ICON_LAYOUT_SPLIT_V,
            Icon::LayoutGrid2x2 => ICON_LAYOUT_GRID_2X2,
            Icon::Layout2Left1Right => ICON_LAYOUT_2LEFT_1RIGHT,
            Icon::Layout1Left2Right => ICON_LAYOUT_1LEFT_2RIGHT,
            Icon::Layout2Top1Bottom => ICON_LAYOUT_2TOP_1BOTTOM,
            Icon::Layout1Top2Bottom => ICON_LAYOUT_1TOP_2BOTTOM,
            Icon::Layout3Columns => ICON_LAYOUT_3COLUMNS,
            Icon::Layout3Rows => ICON_LAYOUT_3ROWS,
            Icon::Layout1Big3Small => ICON_LAYOUT_1BIG_3SMALL,
            Icon::Expand => ICON_EXPAND,
            Icon::Collapse => ICON_COLLAPSE,
            Icon::Move => ICON_MOVE,
            Icon::ObjectTree => ICON_OBJECT_TREE,
            Icon::ZoomIn => ICON_ZOOM_IN,
            Icon::ZoomOut => ICON_ZOOM_OUT,
            Icon::ZoomReset => ICON_ZOOM_RESET,
            Icon::Screenshot => ICON_SCREENSHOT,
            Icon::CircuitBoard => ICON_CIRCUIT_BOARD,
            Icon::NewWindow => ICON_NEW_WINDOW,
            Icon::Cloud => ICON_CLOUD,
            Icon::CloudDownload => ICON_CLOUD_DOWNLOAD,
            Icon::User => ICON_USER,
            Icon::LogIn => ICON_LOG_IN,
            Icon::LogOut => ICON_LOG_OUT,
            Icon::ChevronLeft => ICON_CHEVRON_LEFT,
            Icon::Refresh => ICON_REFRESH,
            Icon::Shield => ICON_SHIELD,
            Icon::ShieldCheck => ICON_SHIELD_CHECK,
            Icon::Globe => ICON_GLOBE,
            Icon::Key => ICON_KEY,
        }
    }

    /// Get just the inner paths (without svg wrapper) for embedding.
    pub fn paths(&self) -> &'static str {
        let svg = self.svg();
        if let Some(start) = svg.find('>') {
            if let Some(end) = svg.rfind("</svg>") {
                return &svg[start + 1..end];
            }
        }
        svg
    }

    /// Get icon by name (case-insensitive, supports snake_case and kebab-case).
    pub fn from_name(name: &str) -> Option<Self> {
        let name_lower = name.to_lowercase().replace('-', "_");
        match name_lower.as_str() {
            "candlestick" | "candles" => Some(Icon::Candlestick),
            "hollow_candles" | "hollowcandles" => Some(Icon::HollowCandles),
            "heikin_ashi" | "heikinashi" => Some(Icon::HeikinAshi),
            "line_chart" | "linechart" | "line" => Some(Icon::LineChart),
            "step_line" | "stepline" => Some(Icon::StepLine),
            "line_markers" | "linemarkers" | "line_with_markers" => Some(Icon::LineWithMarkers),
            "area_chart" | "areachart" | "area" => Some(Icon::AreaChart),
            "hlc_area" | "hlcarea" => Some(Icon::HlcArea),
            "bar_chart" | "barchart" | "bars" => Some(Icon::BarChart),
            "histogram" => Some(Icon::Histogram),
            "columns" => Some(Icon::Columns),
            "baseline" => Some(Icon::Baseline),
            "trend_line" | "trendline" => Some(Icon::TrendLine),
            "horizontal_line" | "horizontalline" => Some(Icon::HorizontalLine),
            "vertical_line" | "verticalline" => Some(Icon::VerticalLine),
            "ray" => Some(Icon::Ray),
            "extended_line" | "extendedline" => Some(Icon::ExtendedLine),
            "parallel_channel" | "parallelchannel" => Some(Icon::ParallelChannel),
            "horizontal_ray" | "horizontalray" => Some(Icon::HorizontalRay),
            "cross_line" | "crossline" => Some(Icon::CrossLine),
            "info_line" | "infoline" => Some(Icon::InfoLine),
            "trend_angle" | "trendangle" => Some(Icon::TrendAngle),
            "regression_trend" | "regressiontrend" => Some(Icon::RegressionTrend),
            "flat_top_bottom" | "flattopbottom" => Some(Icon::FlatTopBottom),
            "disjoint_channel" | "disjointchannel" => Some(Icon::DisjointChannel),
            "rectangle" => Some(Icon::Rectangle),
            "rotated_rectangle" | "rotatedrectangle" => Some(Icon::RotatedRectangle),
            "circle" => Some(Icon::Circle),
            "ellipse" => Some(Icon::Ellipse),
            "triangle" => Some(Icon::Triangle),
            "arc" => Some(Icon::Arc),
            "polyline" => Some(Icon::Polyline),
            "path" => Some(Icon::Path),
            "curve" => Some(Icon::Curve),
            "double_curve" | "doublecurve" => Some(Icon::DoubleCurve),
            "arrow" | "arrow_line" => Some(Icon::Arrow),
            "arrow_up" | "arrowup" | "triangle_up" => Some(Icon::ArrowUp),
            "arrow_down" | "arrowdown" | "triangle_down" => Some(Icon::ArrowDown),
            "brush" => Some(Icon::Brush),
            "highlighter" => Some(Icon::Highlighter),
            "text" => Some(Icon::Text),
            "anchored_text" | "anchoredtext" => Some(Icon::AnchoredText),
            "note" => Some(Icon::Note),
            "price_note" | "pricenote" => Some(Icon::PriceNote),
            "signpost" => Some(Icon::Signpost),
            "callout" => Some(Icon::Callout),
            "comment" => Some(Icon::Comment),
            "price_label" | "pricelabel" => Some(Icon::PriceLabel),
            "sign" => Some(Icon::Sign),
            "flag" => Some(Icon::Flag),
            "table" => Some(Icon::Table),
            "emoji" => Some(Icon::Emoji),
            "image" => Some(Icon::Image),
            "crosshair" => Some(Icon::Crosshair),
            "magnet" => Some(Icon::Magnet),
            "cursor" => Some(Icon::Cursor),
            "hand" => Some(Icon::Hand),
            "zoom" => Some(Icon::Zoom),
            "undo" => Some(Icon::Undo),
            "redo" => Some(Icon::Redo),
            "delete" | "trash" => Some(Icon::Delete),
            "lock" => Some(Icon::Lock),
            "unlock" => Some(Icon::Unlock),
            "eye" | "visible" => Some(Icon::Eye),
            "eye_off" | "eyeoff" | "hidden" => Some(Icon::EyeOff),
            "copy" => Some(Icon::Copy),
            "settings" => Some(Icon::Settings),
            "close" => Some(Icon::Close),
            "star" => Some(Icon::Star),
            "star_filled" | "starfilled" => Some(Icon::StarFilled),
            "long_position" | "longposition" => Some(Icon::LongPosition),
            "short_position" | "shortposition" => Some(Icon::ShortPosition),
            "chevron_up" | "chevronup" => Some(Icon::ChevronUp),
            "chevron_down" | "chevrondown" => Some(Icon::ChevronDown),
            "chevron_right" | "chevronright" => Some(Icon::ChevronRight),
            "plus" => Some(Icon::Plus),
            "minus" => Some(Icon::Minus),
            "grid" => Some(Icon::Grid),
            "layers" => Some(Icon::Layers),
            "indicators" => Some(Icon::Indicators),
            "layout" => Some(Icon::Layout),
            "layout_single" | "layoutsingle" => Some(Icon::LayoutSingle),
            "layout_split_h" | "layoutsplith" => Some(Icon::LayoutSplitH),
            "layout_split_v" | "layoutsplitv" => Some(Icon::LayoutSplitV),
            "layout_grid_2x2" | "layoutgrid2x2" => Some(Icon::LayoutGrid2x2),
            "layout_2left_1right" | "layout2left1right" => Some(Icon::Layout2Left1Right),
            "layout_1left_2right" | "layout1left2right" => Some(Icon::Layout1Left2Right),
            "layout_2top_1bottom" | "layout2top1bottom" => Some(Icon::Layout2Top1Bottom),
            "layout_1top_2bottom" | "layout1top2bottom" => Some(Icon::Layout1Top2Bottom),
            "layout_3columns" | "layout3columns" => Some(Icon::Layout3Columns),
            "layout_3rows" | "layout3rows" => Some(Icon::Layout3Rows),
            "layout_1big_3small" | "layout1big3small" => Some(Icon::Layout1Big3Small),
            "expand" => Some(Icon::Expand),
            "collapse" => Some(Icon::Collapse),
            "move" => Some(Icon::Move),
            "object_tree" | "objecttree" | "tree_pine" | "treepine" => Some(Icon::ObjectTree),
            "zoom_in" | "zoomin" => Some(Icon::ZoomIn),
            "zoom_out" | "zoomout" => Some(Icon::ZoomOut),
            "zoom_reset" | "zoomreset" | "maximize2" => Some(Icon::ZoomReset),
            "screenshot" | "camera" => Some(Icon::Screenshot),
            "watchlist" | "list" => Some(Icon::Watchlist),
            "alert" | "alerts" | "bell" => Some(Icon::Alert),
            "signal" | "signals" | "zap" => Some(Icon::Signal),
            "trading" | "dollarsign" | "dollar_sign" => Some(Icon::Trading),
            "palette" | "theme_settings" => Some(Icon::Palette),
            "search" => Some(Icon::Search),
            "clock" => Some(Icon::Clock),
            "watermark" => Some(Icon::Watermark),
            "legend" => Some(Icon::Legend),
            "tooltip" => Some(Icon::Tooltip),
            "positions" => Some(Icon::Positions),
            "panel_right" | "panelright" => Some(Icon::PanelRight),
            "panel_bottom" | "panelbottom" => Some(Icon::PanelBottom),
            "info" => Some(Icon::Info),
            "menu" => Some(Icon::Menu),
            "line_solid" | "linesolid" => Some(Icon::LineSolid),
            "line_dashed" | "linedashed" => Some(Icon::LineDashed),
            "line_dotted" | "linedotted" => Some(Icon::LineDotted),
            "pencil" => Some(Icon::Pencil),
            "color_fill" | "colorfill" => Some(Icon::ColorFill),
            "text_color" | "textcolor" => Some(Icon::TextColor),
            "line_width_1" | "linewidth1" => Some(Icon::LineWidth1),
            "line_width_2" | "linewidth2" => Some(Icon::LineWidth2),
            "line_width_3" | "linewidth3" => Some(Icon::LineWidth3),
            "line_width_4" | "linewidth4" => Some(Icon::LineWidth4),
            "more_horizontal" | "morehorizontal" => Some(Icon::MoreHorizontal),
            "fib_retracement" | "fibretracement" => Some(Icon::FibRetracement),
            "fib_extension" | "fibextension" | "fib_trend_extension" => Some(Icon::FibExtension),
            "fib_channel" | "fibchannel" => Some(Icon::FibChannel),
            "fib_circle" | "fibcircle" | "fib_circles" => Some(Icon::FibCircle),
            "fib_spiral" | "fibspiral" => Some(Icon::FibSpiral),
            "fib_time_zones" | "fibtimezones" => Some(Icon::FibTimeZones),
            "fib_speed_resistance" | "fibspeedresistance" => Some(Icon::FibSpeedResistance),
            "fib_trend_time" | "fibtrendtime" => Some(Icon::FibTrendTime),
            "fib_arcs" | "fibarcs" => Some(Icon::FibArcs),
            "fib_wedge" | "fibwedge" => Some(Icon::FibWedge),
            "fib_fan" | "fibfan" => Some(Icon::FibFan),
            "gann_box" | "gannbox" => Some(Icon::GannBox),
            "gann_square" | "gannsquare" | "gann_square_fixed" => Some(Icon::GannSquare),
            "gann_fan" | "gannfan" => Some(Icon::GannFan),
            "xabcd_pattern" | "xabcdpattern" => Some(Icon::XabcdPattern),
            "cypher_pattern" | "cypherpattern" => Some(Icon::CypherPattern),
            "head_shoulders" | "headshoulders" => Some(Icon::HeadShoulders),
            "abcd_pattern" | "abcdpattern" => Some(Icon::AbcdPattern),
            "triangle_pattern" | "trianglepattern" => Some(Icon::TrianglePattern),
            "three_drives" | "threedrives" => Some(Icon::ThreeDrives),
            "elliott_impulse" | "elliottimpulse" => Some(Icon::ElliottImpulse),
            "elliott_correction" | "elliottcorrection" => Some(Icon::ElliottCorrection),
            "elliott_triangle" | "elliotttriangle" => Some(Icon::ElliottTriangle),
            "elliott_combo" | "elliottcombo" | "elliott_double_combo" | "elliott_triple_combo" => Some(Icon::ElliottCombo),
            "cycle_lines" | "cyclelines" => Some(Icon::CycleLines),
            "time_cycles" | "timecycles" => Some(Icon::TimeCycles),
            "sine_wave" | "sinewave" => Some(Icon::SineWave),
            "pitchfork" => Some(Icon::Pitchfork),
            "schiff_pitchfork" | "schiffpitchfork" => Some(Icon::SchiffPitchfork),
            "modified_schiff" | "modifiedschiff" => Some(Icon::ModifiedSchiff),
            "inside_pitchfork" | "insidepitchfork" => Some(Icon::InsidePitchfork),
            "price_range" | "pricerange" => Some(Icon::PriceRange),
            "date_range" | "daterange" => Some(Icon::DateRange),
            "price_date_range" | "pricedaterange" => Some(Icon::PriceDateRange),
            "volume_profile" | "volumeprofile" | "fixed_volume_profile" | "anchored_volume_profile" => Some(Icon::VolumeProfile),
            "bars_pattern" | "barspattern" => Some(Icon::BarsPattern),
            "price_projection" | "priceprojection" => Some(Icon::PriceProjection),
            "projection" => Some(Icon::Projection),
            "circuit_board" | "circuitboard" | "connectors" => Some(Icon::CircuitBoard),
            "new_window" | "newwindow" | "external_link" | "open_new" => Some(Icon::NewWindow),
            "cloud" => Some(Icon::Cloud),
            "cloud_download" | "clouddownload" => Some(Icon::CloudDownload),
            "user" => Some(Icon::User),
            "log_in" | "login" => Some(Icon::LogIn),
            "log_out" | "logout" => Some(Icon::LogOut),
            "chevron_left" | "chevronleft" => Some(Icon::ChevronLeft),
            "refresh" | "refresh_cw" | "refreshcw" => Some(Icon::Refresh),
            "shield" => Some(Icon::Shield),
            "shield_check" | "shieldcheck" => Some(Icon::ShieldCheck),
            "globe" => Some(Icon::Globe),
            "key" => Some(Icon::Key),
            _ => None,
        }
    }

    /// Get all available icon names.
    pub fn all_names() -> Vec<&'static str> {
        vec![
            "candlestick", "line_chart", "area_chart", "bar_chart", "histogram", "baseline",
            "trend_line", "horizontal_line", "vertical_line", "ray", "extended_line",
            "parallel_channel", "horizontal_ray", "cross_line", "info_line", "trend_angle",
            "rectangle", "rotated_rectangle", "circle", "ellipse", "triangle", "arc",
            "polyline", "path", "curve", "double_curve",
            "arrow", "arrow_up", "arrow_down",
            "brush", "highlighter",
            "text", "anchored_text", "note", "price_note", "signpost", "callout",
            "comment", "price_label", "sign", "flag", "table",
            "emoji", "image",
            "crosshair", "magnet", "cursor", "hand", "zoom",
            "undo", "redo", "delete", "lock", "unlock", "eye", "eye_off", "copy", "settings", "close",
            "long_position", "short_position",
            "chevron_up", "chevron_down", "chevron_right", "plus", "minus", "grid", "layers", "indicators",
            "search", "clock", "watermark", "legend", "tooltip",
            "watchlist", "alert", "trading", "positions", "panel_right", "panel_bottom",
            "palette", "signal", "menu",
            "line_solid", "line_dashed", "line_dotted",
            "pencil", "color_fill", "text_color",
            "line_width_1", "line_width_2", "line_width_3", "line_width_4", "more_horizontal",
        ]
    }
}

// =============================================================================
// Conversions
// =============================================================================

/// Convert an `Icon` to a `uzor::types::IconId`.
///
/// This impl is here (in the crate that defines `Icon`) to satisfy Rust's
/// orphan rules — `uzor::IconId` is defined outside this crate, so we
/// are only allowed to impl `From<Icon>` for it in the crate that owns `Icon`.
impl From<Icon> for uzor::types::IconId {
    fn from(icon: Icon) -> Self {
        // Use Debug format to get the enum variant name, then convert to snake_case
        let name = format!("{:?}", icon);
        let snake_case: String = name
            .chars()
            .enumerate()
            .flat_map(|(i, c)| {
                if c.is_uppercase() && i > 0 {
                    vec!['_', c.to_ascii_lowercase()]
                } else {
                    vec![c.to_ascii_lowercase()]
                }
            })
            .collect();
        Self::new(&snake_case)
    }
}

pub fn icon_svg(name: &str) -> Option<&'static str> {
    let normalized = name.to_lowercase().replace('-', "_");
    match normalized.as_str() {
        // Chart types
        "candlestick" | "candles" => Some(ICON_CANDLESTICK),
        "hollow_candles" | "hollowcandles" => Some(ICON_HOLLOW_CANDLES),
        "heikin_ashi" | "heikinashi" => Some(ICON_HEIKIN_ASHI),
        "line_chart" | "linechart" | "line" => Some(ICON_LINE_CHART),
        "area_chart" | "areachart" | "area" => Some(ICON_AREA_CHART),
        "bar_chart" | "barchart" | "bars" => Some(ICON_BAR_CHART),
        "histogram" => Some(ICON_HISTOGRAM),
        "baseline" => Some(ICON_BASELINE),
        "step_line" | "stepline" => Some(ICON_STEP_LINE),
        "line_markers" | "linemarkers" | "line_with_markers" | "linewithmarkers" => Some(ICON_LINE_MARKERS),
        "hlc_area" | "hlcarea" => Some(ICON_HLC_AREA),
        "columns" => Some(ICON_COLUMNS),
        // Renko uses candlestick as fallback
        "renko" => Some(ICON_CANDLESTICK),

        // Drawing tools
        "trend_line" | "trendline" => Some(ICON_TREND_LINE),
        "horizontal_line" | "horizontalline" => Some(ICON_HORIZONTAL_LINE),
        "vertical_line" | "verticalline" => Some(ICON_VERTICAL_LINE),
        "ray" => Some(ICON_RAY),
        "extended_line" | "extendedline" => Some(ICON_EXTENDED_LINE),
        "parallel_channel" | "parallelchannel" => Some(ICON_PARALLEL_CHANNEL),
        "horizontal_ray" | "horizontalray" => Some(ICON_HORIZONTAL_RAY),
        "cross_line" | "crossline" => Some(ICON_CROSS_LINE),
        "info_line" | "infoline" => Some(ICON_INFO_LINE),
        "trend_angle" | "trendangle" => Some(ICON_TREND_ANGLE),

        // Channels
        "regression_trend" | "regressiontrend" => Some(ICON_REGRESSION_TREND),
        "flat_top_bottom" | "flattopbottom" => Some(ICON_FLAT_TOP_BOTTOM),
        "disjoint_channel" | "disjointchannel" => Some(ICON_DISJOINT_CHANNEL),

        // Pitchforks (all variants share the same icon)
        "pitchfork" | "schiff_pitchfork" | "schiffpitchfork"
        | "modified_schiff" | "modifiedschiff"
        | "inside_pitchfork" | "insidepitchfork" => Some(ICON_PITCHFORK),

        // Fibonacci
        "fib_retracement" | "fibretracement" => Some(ICON_FIB_RETRACEMENT),
        "fib_extension" | "fibextension" | "fib_trend_extension" => Some(ICON_FIB_EXTENSION),
        "fib_channel" | "fibchannel" => Some(ICON_FIB_CHANNEL),
        "fib_circle" | "fibcircle" | "fib_circles" => Some(ICON_FIB_CIRCLE),
        "fib_spiral" | "fibspiral" => Some(ICON_FIB_SPIRAL),
        "fib_time_zones" | "fibtimezones" => Some(ICON_FIB_TIME_ZONES),
        "fib_speed_resistance" | "fibspeedresistance" => Some(ICON_FIB_SPEED_RESISTANCE),
        "fib_trend_time" | "fibtrendtime" => Some(ICON_FIB_TREND_TIME),
        "fib_arcs" | "fibarcs" => Some(ICON_FIB_ARCS),
        "fib_wedge" | "fibwedge" => Some(ICON_FIB_WEDGE),
        "fib_fan" | "fibfan" => Some(ICON_FIB_FAN),

        // Gann
        "gann_box" | "gannbox" => Some(ICON_GANN_BOX),
        "gann_square" | "gannsquare" | "gann_square_fixed" => Some(ICON_GANN_SQUARE),
        "gann_fan" | "gannfan" => Some(ICON_GANN_FAN),

        // Patterns
        "xabcd_pattern" | "xabcdpattern" => Some(ICON_XABCD_PATTERN),
        "cypher_pattern" | "cypherpattern" => Some(ICON_XABCD_PATTERN),
        "head_shoulders" | "headshoulders" => Some(ICON_HEAD_SHOULDERS),
        "abcd_pattern" | "abcdpattern" => Some(ICON_ABCD_PATTERN),
        "triangle_pattern" | "trianglepattern" => Some(ICON_TRIANGLE_PATTERN),
        "three_drives" | "threedrives" => Some(ICON_THREE_DRIVES),

        // Elliott waves (all share same icon)
        "elliott_impulse" | "elliottimpulse"
        | "elliott_correction" | "elliottcorrection"
        | "elliott_triangle" | "elliotttriangle"
        | "elliott_combo" | "elliottcombo"
        | "elliott_double_combo" | "elliott_triple_combo" => Some(ICON_ELLIOTT_WAVE),

        // Cycles
        "cycle_lines" | "cyclelines" => Some(ICON_CYCLE_LINES),
        "time_cycles" | "timecycles" => Some(ICON_TIME_CYCLES),
        "sine_wave" | "sinewave" => Some(ICON_SINE_WAVE),

        // Shapes
        "rectangle" => Some(ICON_RECTANGLE),
        "rotated_rectangle" | "rotatedrectangle" => Some(ICON_ROTATED_RECTANGLE),
        "circle" => Some(ICON_CIRCLE),
        "ellipse" => Some(ICON_ELLIPSE),
        "triangle" => Some(ICON_TRIANGLE),
        "arc" => Some(ICON_ARC),
        "polyline" => Some(ICON_POLYLINE),
        "path" => Some(ICON_PATH),
        "curve" => Some(ICON_CURVE),
        "double_curve" | "doublecurve" => Some(ICON_DOUBLE_CURVE),

        // Arrows
        "arrow" | "arrow_line" => Some(ICON_ARROW),
        "arrow_up" | "arrowup" | "triangle_up" => Some(ICON_ARROW_UP),
        "arrow_down" | "arrowdown" | "triangle_down" => Some(ICON_ARROW_DOWN),

        // Brushes
        "brush" => Some(ICON_BRUSH),
        "highlighter" => Some(ICON_HIGHLIGHTER),

        // Annotations
        "text" => Some(ICON_TEXT),
        "anchored_text" | "anchoredtext" => Some(ICON_ANCHORED_TEXT),
        "note" => Some(ICON_NOTE),
        "price_note" | "pricenote" => Some(ICON_PRICE_NOTE),
        "signpost" => Some(ICON_SIGNPOST),
        "table" => Some(ICON_TABLE),
        "callout" => Some(ICON_CALLOUT),
        "comment" => Some(ICON_COMMENT),
        "price_label" | "pricelabel" => Some(ICON_PRICE_LABEL),
        "sign" => Some(ICON_SIGN),
        "flag" => Some(ICON_FLAG),
        "emoji" => Some(ICON_EMOJI),
        "image" => Some(ICON_IMAGE),

        // Measurement
        "price_range" | "pricerange" => Some(ICON_PRICE_RANGE),
        "date_range" | "daterange" => Some(ICON_DATE_RANGE),
        "price_date_range" | "pricedaterange" => Some(ICON_PRICE_DATE_RANGE),

        // Volume / projection
        "volume_profile" | "volumeprofile"
        | "fixed_volume_profile" | "anchored_volume_profile" => Some(ICON_VOLUME_PROFILE),
        "bars_pattern" | "barspattern" => Some(ICON_BARS_PATTERN),
        "price_projection" | "priceprojection" => Some(ICON_PRICE_PROJECTION),
        "projection" => Some(ICON_PROJECTION),

        // Positions
        "long_position" | "longposition" => Some(ICON_LONG_POSITION),
        "short_position" | "shortposition" => Some(ICON_SHORT_POSITION),

        // Tools
        "crosshair" => Some(ICON_CROSSHAIR),
        "magnet" => Some(ICON_MAGNET),
        "magnet_strong" | "magnetstrong" => Some(ICON_MAGNET_STRONG),
        "cursor" => Some(ICON_CURSOR),
        "hand" => Some(ICON_HAND),

        // Inline config toolbar icons
        "color_fill" | "colorfill" => Some(ICON_COLOR_FILL),
        "text_color" | "textcolor" => Some(ICON_TEXT_COLOR),
        "line_solid" | "linesolid" => Some(ICON_LINE_SOLID),
        "line_dashed" | "linedashed" => Some(ICON_LINE_DASHED),
        "line_dotted" | "linedotted" => Some(ICON_LINE_DOTTED),
        "more_horizontal" | "morehorizontal" => Some(ICON_MORE_HORIZONTAL),
        "alert" | "alerts" | "bell" => Some(ICON_ALERT),

        // Right toolbar sidebar buttons
        "watchlist" | "list" => Some(ICON_WATCHLIST),
        "trading" | "dollarsign" | "dollar_sign" => Some(ICON_TRADING),
        "palette" | "theme_settings" => Some(ICON_PALETTE),
        "signal" | "signals" | "zap" => Some(ICON_SIGNAL),
        "object_tree" | "objecttree" | "tree_pine" | "treepine" | "layers" => Some(ICON_OBJECT_TREE),
        "circuit_board" | "circuitboard" | "connectors" => Some(ICON_CIRCUIT_BOARD),
        "cpu" | "activity" | "performance" => Some(ICON_CPU),
        "new_window" | "newwindow" | "external_link" | "open_new" => Some(ICON_NEW_WINDOW),
        "cloud" => Some(ICON_CLOUD),
        "cloud_download" | "clouddownload" => Some(ICON_CLOUD_DOWNLOAD),
        "user" => Some(ICON_USER),
        "log_in" | "login" => Some(ICON_LOG_IN),
        "log_out" | "logout" => Some(ICON_LOG_OUT),
        "chevron_left" | "chevronleft" => Some(ICON_CHEVRON_LEFT),
        "refresh" | "refresh_cw" | "refreshcw" => Some(ICON_REFRESH),
        "shield" => Some(ICON_SHIELD),
        "shield_check" | "shieldcheck" => Some(ICON_SHIELD_CHECK),
        "globe" => Some(ICON_GLOBE),
        "key" => Some(ICON_KEY),

        // Bottom toolbar zoom/screenshot buttons
        "zoom_in" | "zoomin" => Some(ICON_ZOOM_IN),
        "zoom_out" | "zoomout" => Some(ICON_ZOOM_OUT),
        "zoom_reset" | "zoomreset" | "maximize2" => Some(ICON_ZOOM_RESET),
        "screenshot" | "camera" => Some(ICON_SCREENSHOT),

        // Actions
        "undo" => Some(ICON_UNDO),
        "redo" => Some(ICON_REDO),
        "delete" | "trash" => Some(ICON_DELETE),
        "lock" => Some(ICON_LOCK),
        "unlock" => Some(ICON_UNLOCK),
        "eye" | "visible" => Some(ICON_EYE),
        "eye_off" | "eyeoff" | "hidden" => Some(ICON_EYE_OFF),
        "settings" => Some(ICON_SETTINGS),
        "copy" => Some(ICON_COPY),
        "close" => Some(ICON_CLOSE),
        "pencil" | "edit" | "rename" => Some(ICON_PENCIL),
        "bookmark" => Some(ICON_BOOKMARK),
        "expand" => Some(ICON_EXPAND),
        "collapse" => Some(ICON_COLLAPSE),
        "move" => Some(ICON_MOVE),

        // Navigation/UI
        "plus" => Some(ICON_PLUS),
        "minus" => Some(ICON_MINUS),
        "indicators" => Some(ICON_INDICATORS),
        "chevron_down" | "chevrondown" => Some(ICON_CHEVRON_DOWN),
        "menu" => Some(ICON_MENU),
        "search" => Some(ICON_SEARCH),
        "clock" => Some(ICON_CLOCK),
        "layout_single" | "layoutsingle" => Some(ICON_LAYOUT_SINGLE),
        "layout_split_h" | "layoutsplith" => Some(ICON_LAYOUT_SPLIT_H),
        "layout_split_v" | "layoutsplitv" => Some(ICON_LAYOUT_SPLIT_V),
        "layout_grid_2x2" | "layoutgrid2x2" => Some(ICON_LAYOUT_GRID_2X2),
        "layout_2left_1right" | "layout2left1right" => Some(ICON_LAYOUT_2LEFT_1RIGHT),
        "layout_1left_2right" | "layout1left2right" => Some(ICON_LAYOUT_1LEFT_2RIGHT),
        "layout_2top_1bottom" | "layout2top1bottom" => Some(ICON_LAYOUT_2TOP_1BOTTOM),
        "layout_1top_2bottom" | "layout1top2bottom" => Some(ICON_LAYOUT_1TOP_2BOTTOM),
        "layout_3columns" | "layout3columns" => Some(ICON_LAYOUT_3COLUMNS),
        "layout_3rows" | "layout3rows" => Some(ICON_LAYOUT_3ROWS),
        "layout_1big_3small" | "layout1big3small" => Some(ICON_LAYOUT_1BIG_3SMALL),
        "grid" => Some(ICON_GRID),

        // Emoji items — use per-type SVG from primitives, fallback to generic
        n if n.starts_with("emoji") => {
            // Strip "emoji_" prefix if present, then look up EmojiType
            let key = n.strip_prefix("emoji_").unwrap_or(n.strip_prefix("emoji").unwrap_or(n));
            if let Some(et) = crate::drawing::primitives_v2::icons::emoji::EmojiType::from_id(key) {
                Some(et.svg())
            } else {
                Some(ICON_EMOJI)
            }
        }

        _ => None,
    }
}
