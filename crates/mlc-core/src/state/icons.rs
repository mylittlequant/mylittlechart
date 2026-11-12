//! Chart icon registry
//!
//! Maps icon name strings (as used in `ToolbarIconId`) to SVG string constants.
//! Only includes icons used by the chart toolbar (`toolbar.rs`).
//!
//! The SVG markup lives in `uzor::icons::{ui, chart}`; the `ICON_*` consts here
//! are aliases of uzor's. The few whose uzor namesake draws differently stay local.

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
    Baseline,

    // === Drawing Tools ===
    TrendLine,
    HorizontalLine,
    VerticalLine,
    Ray,
    RayLeft,
    ExtendedLine,
    ParallelChannel,
    HorizontalRay,
    HorizontalRayLeft,
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
    /// Pushpin outline — pinned-favorites-row toggle header.
    Pin,

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
    /// Clock face with a counter-clockwise arrow — system event log (bottom
    /// of the right toolbar + the `Events` sidebar panel header). Added
    /// 2026-09-05: "Bell" already means the Alerts panel and "List" already
    /// means Watchlist, so neither reads as a distinct third icon; no
    /// "Inbox"/"History" glyph existed in this registry before this one.
    History,
    /// Envelope — right-toolbar Community (Forum + Messages) button,
    /// directly above `History`'s own bottom-anchored slot (community-modal
    /// arc, `docs/mlc/plans/community-modal-and-forum-ladder-2026-09-09.md`
    /// W1). No prior envelope glyph existed in this registry.
    Mail,
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

    // === AI / Agents ===
    /// Bot / robot head icon — used for the Agents sidebar panel.
    Bot,

    // === Performance ===
    /// Gauge / speedometer — used for the Performance sidebar panel.
    /// Distinct from `Signal` (Zap bolt) — the two used to collide (both
    /// rendered the Zap icon before the typed-slot redesign 2026-07-11).
    Gauge,

    // === Typed sidebar slots (owner redesign 2026-07-11) ===
    /// Order-ticket / briefcase glyph — Trading slot (hosts `TradingContainer`).
    SlotTrading,
    /// Orderbook ladder (two columns of short rows) — DOM slot.
    SlotDom,
    /// Vertical tape with row ticks — Trade Tape slot.
    SlotTape,
    /// Ladder + tape hybrid (rows with depth bars) — L2 Tape slot.
    SlotL2,
    /// One oversized block among small rows — Big Trades slot.
    SlotBigTrades,
    /// "LIQ" text chip — Liquidations slot.
    SlotLiq,
    /// "SCR" text chip — Screener panel rail button (plan F, owner spec
    /// 2026-08-01). NOT a typed slot — same text-chip grammar as the slot
    /// icons above, reused for a plain sidebar panel per owner instruction.
    /// Provisional: owner will rename/re-icon later.
    Screener,
}

// =============================================================================
// Chart Type Icons
// =============================================================================

pub const ICON_CANDLESTICK: &str = uzor::icons::ui::ICON_CANDLESTICK;

pub const ICON_HOLLOW_CANDLES: &str = uzor::icons::ui::ICON_HOLLOW_CANDLES;

pub const ICON_HEIKIN_ASHI: &str = uzor::icons::ui::ICON_HEIKIN_ASHI;

pub const ICON_LINE_CHART: &str = uzor::icons::ui::ICON_LINE_CHART;

pub const ICON_AREA_CHART: &str = uzor::icons::ui::ICON_AREA_CHART;

pub const ICON_BAR_CHART: &str = uzor::icons::ui::ICON_BAR_CHART;

pub const ICON_HISTOGRAM: &str = uzor::icons::ui::ICON_HISTOGRAM;

pub const ICON_BASELINE: &str = uzor::icons::ui::ICON_BASELINE;

// =============================================================================
// Derived / Price-Action / Orderflow Chart Type Icons
// =============================================================================
// Generated 2026-07-10 (`docs/mlc/research/chart-type-icons-2026-07-10/`) —
// replaces the 13 stub arms in `icon_svg()` that previously fell back to
// ICON_BAR_CHART / ICON_HISTOGRAM / ICON_LINE_CHART / ICON_CANDLESTICK,
// leaving every derived chart type visually indistinguishable from its
// siblings in the chart-type dropdown. Same style contract as the block
// above: viewBox="0 0 24 24", stroke="currentColor", fill="none" root with
// selective fill="currentColor" for solid glyph parts.

pub const ICON_RANGE_BAR: &str = uzor::icons::chart::ICON_RANGE_BAR;

pub const ICON_VOLUME_BAR: &str = uzor::icons::chart::ICON_VOLUME_BAR;

pub const ICON_DOLLAR_BAR: &str = uzor::icons::chart::ICON_DOLLAR_BAR;

pub const ICON_TICK_BAR: &str = uzor::icons::chart::ICON_TICK_BAR;

pub const ICON_TICK_IMBALANCE_BAR: &str = uzor::icons::chart::ICON_TICK_IMBALANCE_BAR;

pub const ICON_VOLUME_IMBALANCE_BAR: &str = uzor::icons::chart::ICON_VOLUME_IMBALANCE_BAR;

pub const ICON_RUN_BAR: &str = uzor::icons::chart::ICON_RUN_BAR;

pub const ICON_RENKO: &str = uzor::icons::chart::ICON_RENKO;

pub const ICON_PNF: &str = uzor::icons::chart::ICON_PNF;

pub const ICON_KAGI: &str = uzor::icons::chart::ICON_KAGI;

pub const ICON_THREE_LINE_BREAK: &str = uzor::icons::chart::ICON_THREE_LINE_BREAK;

pub const ICON_FOOTPRINT_CELLS: &str = uzor::icons::chart::ICON_FOOTPRINT_CELLS;

pub const ICON_TPO: &str = uzor::icons::chart::ICON_TPO;

// =============================================================================
// Drawing Tool Icons
// =============================================================================

pub const ICON_TREND_LINE: &str = uzor::icons::ui::ICON_TREND_LINE;

pub const ICON_HORIZONTAL_LINE: &str = uzor::icons::ui::ICON_HORIZONTAL_LINE;

pub const ICON_VERTICAL_LINE: &str = uzor::icons::ui::ICON_VERTICAL_LINE;

pub const ICON_RAY: &str = uzor::icons::ui::ICON_RAY;

pub const ICON_RAY_LEFT: &str = uzor::icons::chart::ICON_RAY_LEFT;

pub const ICON_EXTENDED_LINE: &str = uzor::icons::ui::ICON_EXTENDED_LINE;

pub const ICON_PARALLEL_CHANNEL: &str = uzor::icons::ui::ICON_PARALLEL_CHANNEL;

pub const ICON_HORIZONTAL_RAY: &str = uzor::icons::ui::ICON_HORIZONTAL_RAY;

pub const ICON_HORIZONTAL_RAY_LEFT: &str = uzor::icons::chart::ICON_HORIZONTAL_RAY_LEFT;

pub const ICON_CROSS_LINE: &str = uzor::icons::ui::ICON_CROSS_LINE;

/// Simplified 2026-08-07 (icon audit item 5 — smudged at 24px): the info
/// box was small (8x5) and overlapped the diagonal's midpoint. Enlarged and
/// pushed into the corner (less overlap) with a simple internal tick
/// standing in for the readout text.
pub const ICON_INFO_LINE: &str = uzor::icons::chart::ICON_INFO_LINE;

pub const ICON_TREND_ANGLE: &str = uzor::icons::ui::ICON_TREND_ANGLE;

// =============================================================================
// Channel Icons
// =============================================================================

/// Simplified 2026-08-07 (icon audit item 5 — smudged at 24px): was 3
/// near-parallel diagonals (1 solid + 2 dashed) crowding the same corners.
/// Now ONE solid trend line plus two short perpendicular ticks marking the
/// channel width at each end — reads clean at rail size.
pub const ICON_REGRESSION_TREND: &str = uzor::icons::chart::ICON_REGRESSION_TREND;

/// Redrawn 2026-08-07 (icon audit item 4 — read as a mathematical sigma Σ):
/// top+bottom horizontal boundaries plus a chevron on the left edge drew
/// exactly the Σ glyph. Now one flat (horizontal) boundary and one sloped
/// boundary — the actual "channel with one flat side" semantic.
pub const ICON_FLAT_TOP_BOTTOM: &str = uzor::icons::chart::ICON_FLAT_TOP_BOTTOM;

/// Simplified 2026-08-07 (icon audit item 5 — smudged at 24px): was 4
/// disconnected diagonal segments that read as scattered fragments. Now 2
/// bent 2-segment polylines (top boundary, bottom boundary) — still reads
/// as "non-parallel channel" with half the strokes.
pub const ICON_DISJOINT_CHANNEL: &str = uzor::icons::chart::ICON_DISJOINT_CHANNEL;

// =============================================================================
// Pitchfork Icons
// =============================================================================
// Redrawn 2026-08-07 (icon audit item 2): all 4 pitchfork variants shared
// this one glyph, even though they are 4 distinct tools with different
// median-line construction. Shared family silhouette (3-tine fork) kept —
// they ARE a family — but the stem/handle treatment now differs per variant
// to match its actual construction difference.

/// Andrew's Pitchfork — the reference: straight median line down from the
/// tine midpoint. Every other variant's stem is a variation on this one.
pub const ICON_PITCHFORK: &str = uzor::icons::ui::ICON_PITCHFORK;

/// Schiff Pitchfork — median origin shifted to the midpoint of the first
/// two tines (marked dot), not the handle center.
pub const ICON_SCHIFF_PITCHFORK: &str = uzor::icons::chart::ICON_SCHIFF_PITCHFORK;

/// Modified Schiff — median origin shifted further, to the midpoint of the
/// last two tines (marked dot on the opposite side from Schiff).
pub const ICON_MODIFIED_SCHIFF: &str = uzor::icons::chart::ICON_MODIFIED_SCHIFF;

/// Inside Pitchfork — "projects backward through the handle": the median
/// continues (dashed) past the tine base instead of stopping there.
pub const ICON_INSIDE_PITCHFORK: &str = uzor::icons::chart::ICON_INSIDE_PITCHFORK;

// =============================================================================
// Fibonacci Icons
// =============================================================================

pub const ICON_FIB_RETRACEMENT: &str = uzor::icons::chart::ICON_FIB_RETRACEMENT;

/// Redrawn 2026-08-07 (icon audit item 4 — read as "disabled"): the level
/// guides used to span the full width and cross the trend leg at points
/// that read like a prohibition slash. Now a proper A-B-C leg (3 points,
/// matching "trend-based extension") with SHORT guides that don't cross it.
pub const ICON_FIB_EXTENSION: &str = uzor::icons::chart::ICON_FIB_EXTENSION;

pub const ICON_FIB_CHANNEL: &str = uzor::icons::chart::ICON_FIB_CHANNEL;

pub const ICON_FIB_CIRCLE: &str = uzor::icons::chart::ICON_FIB_CIRCLE;

/// Redrawn 2026-07-18 (owner-approved design round; see nemo/docs/mlc/research/icon-drafts/)
pub const ICON_FIB_SPIRAL: &str = uzor::icons::chart::ICON_FIB_SPIRAL;

pub const ICON_FIB_TIME_ZONES: &str = uzor::icons::chart::ICON_FIB_TIME_ZONES;

/// Simplified 2026-08-07 (icon audit item 5 — smudged at 24px): dropped the
/// dashed bounding square (competed with the diagonal for attention);
/// diagonal + 2 divider verticals + anchor dot IS the speed-resistance fan.
pub const ICON_FIB_SPEED_RESISTANCE: &str = uzor::icons::chart::ICON_FIB_SPEED_RESISTANCE;

pub const ICON_FIB_TREND_TIME: &str = uzor::icons::chart::ICON_FIB_TREND_TIME;

/// Redrawn 2026-08-07 (icon audit item 4 — read as "disabled"): the trend
/// leg used to run the full corner-to-corner diagonal, which combined with
/// the concentric arcs into a classic circle-slash "prohibited" glyph. The
/// leg now stops short of the far corner.
pub const ICON_FIB_ARCS: &str = uzor::icons::chart::ICON_FIB_ARCS;

/// Redrawn 2026-07-18 (owner-approved design round; see nemo/docs/mlc/research/icon-drafts/)
pub const ICON_FIB_WEDGE: &str = uzor::icons::chart::ICON_FIB_WEDGE;

pub const ICON_FIB_FAN: &str = uzor::icons::chart::ICON_FIB_FAN;

// =============================================================================
// Gann Icons
// =============================================================================

/// Simplified 2026-08-07 (icon audit item 5 — smudged at 24px): dropped one
/// of the two X diagonals (5 crossing strokes at full weight was a busy
/// asterisk at rail size); the remaining diagonal + cross midlines are
/// de-emphasized so the square border reads first.
pub const ICON_GANN_BOX: &str = uzor::icons::chart::ICON_GANN_BOX;

/// Resizable Gann square: bounding square + inscribed circle + dashed X —
/// the circle is what distinguishes it from the fixed-size variant below.
pub const ICON_GANN_SQUARE: &str = uzor::icons::chart::ICON_GANN_SQUARE;

/// Fixed-size Gann square (icon audit item 2 — was a byte-identical
/// duplicate of the resizable variant). A fixed 3x3 grid subdivision reads
/// as "equal fixed divisions", distinct from the resizable variant's
/// inscribed circle + X.
pub const ICON_GANN_SQUARE_FIXED: &str = uzor::icons::chart::ICON_GANN_SQUARE_FIXED;

/// Redrawn 2026-07-18 (owner-approved design round; see nemo/docs/mlc/research/icon-drafts/)
pub const ICON_GANN_FAN: &str = uzor::icons::chart::ICON_GANN_FAN;

// =============================================================================
// Pattern Icons
// =============================================================================

/// Redrawn 2026-08-07 (icon audit item 3 — indistinguishable zigzag column):
/// marked all 5 vertices (X-A-B-C-D) with dots — the point COUNT is the
/// distinguishing device, since XABCD and its 4/5-point siblings otherwise
/// share the same "zigzag line" silhouette.
pub const ICON_XABCD_PATTERN: &str = uzor::icons::chart::ICON_XABCD_PATTERN;

/// Redrawn 2026-08-07 (icon audit item 3): same 5-point count as XABCD (both
/// MultiPointFixed(5)) so dots alone don't distinguish them — the final
/// vertex (Cypher's defining C-D leg) is marked with a SQUARE instead of a
/// circle, an enclosing-form device rather than just a different squiggle.
pub const ICON_CYPHER_PATTERN: &str = uzor::icons::chart::ICON_CYPHER_PATTERN;

/// Redrawn 2026-08-07 (icon audit item 3): the neckline (already a good
/// enclosing-form device) plus the 3 marked peaks — left shoulder, head,
/// right shoulder — name the pattern in the glyph.
pub const ICON_HEAD_SHOULDERS: &str = uzor::icons::chart::ICON_HEAD_SHOULDERS;

/// Redrawn 2026-08-07 (icon audit item 3): 4 marked vertices (A-B-C-D) —
/// point count distinguishes it from the 5-point XABCD/Cypher siblings.
pub const ICON_ABCD_PATTERN: &str = uzor::icons::chart::ICON_ABCD_PATTERN;

/// Redrawn 2026-07-18 (owner-approved design round; see nemo/docs/mlc/research/icon-drafts/)
pub const ICON_TRIANGLE_PATTERN: &str = uzor::icons::chart::ICON_TRIANGLE_PATTERN;

/// Redrawn 2026-08-07 (icon audit item 3): the 3 ascending peaks — the
/// "three drives" — are marked, distinguishing device drawn directly from
/// the pattern's name rather than an arbitrary vertex count.
pub const ICON_THREE_DRIVES: &str = uzor::icons::chart::ICON_THREE_DRIVES;

// =============================================================================
// Harmonic preset icons (autodetectors A0, 2026-09-24)
// =============================================================================
//
// One glyph per named XABCD preset (`xabcd_pattern.rs`'s `metadata_*`
// factories, `docs/mlc/plans/autodetectors-a0-implementation-2026-09-24.md`
// piece 2/3) — bodies copied verbatim from `mlc-ui-lab/designs/
// 2026-09-23-toolbar-icons/icons_v4.json` batch B, wrapped the same way
// `ICON_SMC_SCAN` already is (viewBox 24x24, root stroke-width 1.25,
// currentColor, tints via opacity — house standard, not the design file's
// own 1.5 note).

pub const ICON_HARMONIC_GARTLEY: &str = uzor::icons::chart::ICON_HARMONIC_GARTLEY;

pub const ICON_HARMONIC_BAT: &str = uzor::icons::chart::ICON_HARMONIC_BAT;

pub const ICON_HARMONIC_ALT_BAT: &str = uzor::icons::chart::ICON_HARMONIC_ALT_BAT;

pub const ICON_HARMONIC_BUTTERFLY: &str = uzor::icons::chart::ICON_HARMONIC_BUTTERFLY;

pub const ICON_HARMONIC_CRAB: &str = uzor::icons::chart::ICON_HARMONIC_CRAB;

pub const ICON_HARMONIC_DEEP_CRAB: &str = uzor::icons::chart::ICON_HARMONIC_DEEP_CRAB;

pub const ICON_HARMONIC_SHARK: &str = uzor::icons::chart::ICON_HARMONIC_SHARK;

/// Bars scan icon swap (A0, piece 3): the range-bracket-motif replacement
/// for `candle_pattern_scan`'s previous icon, matching the "scan tools
/// share a bracket motif" design note (`ICON_SMC_SCAN`'s own doc) that
/// `ICON_CANDLE_PATTERN_SCAN` predates. Wired to the `candle_pattern_scan`
/// toolbar row in `toolbar.rs`'s `pattern_section()` (piece 5) — the icon
/// constant name follows the new `bars_scan` id, not the old primitive name.
pub const ICON_BARS_SCAN: &str = uzor::icons::chart::ICON_BARS_SCAN;

// =============================================================================
// Elliott Wave Icons
// =============================================================================

/// Redrawn 2026-07-18 (owner-approved design round; see nemo/docs/mlc/research/icon-drafts/)
pub const ICON_ELLIOTT_IMPULSE: &str = uzor::icons::chart::ICON_ELLIOTT_IMPULSE;

/// Redrawn 2026-07-18 (owner-approved design round; see nemo/docs/mlc/research/icon-drafts/)
pub const ICON_ELLIOTT_CORRECTION: &str = uzor::icons::chart::ICON_ELLIOTT_CORRECTION;

/// Redrawn 2026-07-18 (owner-approved design round; see nemo/docs/mlc/research/icon-drafts/)
pub const ICON_ELLIOTT_TRIANGLE: &str = uzor::icons::chart::ICON_ELLIOTT_TRIANGLE;

/// Redrawn 2026-07-18 (owner-approved design round; see nemo/docs/mlc/research/icon-drafts/).
/// Generic base — used only when no double/triple distinction applies.
pub const ICON_ELLIOTT_COMBO: &str = uzor::icons::chart::ICON_ELLIOTT_COMBO;

/// WXY double combination — same glyph as the generic combo base (3 marked
/// legs = W-X-Y).
pub const ICON_ELLIOTT_DOUBLE_COMBO: &str = ICON_ELLIOTT_COMBO;

/// WXYXZ triple combination (icon audit item 2 — was byte-identical to the
/// double combo). Busier zigzag with 5 marked legs (W-X-Y-X-Z) — MORE
/// direction changes than the double combo, matching the extra W/X/Y/X/Z
/// structure.
pub const ICON_ELLIOTT_TRIPLE_COMBO: &str = uzor::icons::chart::ICON_ELLIOTT_TRIPLE_COMBO;

// =============================================================================
// Cycle Icons
// =============================================================================

pub const ICON_CYCLE_LINES: &str = uzor::icons::ui::ICON_CYCLE_LINES;

pub const ICON_TIME_CYCLES: &str = uzor::icons::ui::ICON_TIME_CYCLES;

pub const ICON_SINE_WAVE: &str = uzor::icons::ui::ICON_SINE_WAVE;

// =============================================================================
// Shape Icons
// =============================================================================

pub const ICON_RECTANGLE: &str = uzor::icons::ui::ICON_RECTANGLE;

pub const ICON_ROTATED_RECTANGLE: &str = uzor::icons::ui::ICON_ROTATED_RECTANGLE;

pub const ICON_CIRCLE: &str = uzor::icons::ui::ICON_CIRCLE;

pub const ICON_ELLIPSE: &str = uzor::icons::ui::ICON_ELLIPSE;

pub const ICON_TRIANGLE: &str = uzor::icons::ui::ICON_TRIANGLE;

pub const ICON_ARC: &str = uzor::icons::ui::ICON_ARC;

pub const ICON_POLYLINE: &str = uzor::icons::ui::ICON_POLYLINE;

pub const ICON_PATH: &str = uzor::icons::ui::ICON_PATH;

/// Redrawn 2026-08-07 (icon audit item 4 — read as a capital A): the two
/// dashed straight legs from each endpoint up to the control point formed
/// exactly that triangle/crossbar silhouette. Dropped them; the control
/// point is now a dimmer, unconnected dot — distinguishes from `arc` (no
/// control-point marker at all) without reconstructing the "A".
pub const ICON_CURVE: &str = uzor::icons::chart::ICON_CURVE;

/// Simplified 2026-08-07 (icon audit item 5 — smudged at 24px): widened the
/// vertical gap between the two S-curves (was 4px apart, now 9px) so they
/// read as two distinct parallel waves instead of one blurred double line.
pub const ICON_DOUBLE_CURVE: &str = uzor::icons::chart::ICON_DOUBLE_CURVE;

// =============================================================================
// Arrow Icons
// =============================================================================

pub const ICON_ARROW: &str = uzor::icons::ui::ICON_ARROW;

/// Solid sub-grammar (icon audit item 6): `viewBox="0 0 24 24" fill="currentColor"
/// stroke="none"` — used for glyph/badge-style icons (triangle markers,
/// filled arrows) whose recognizability depends on a filled silhouette
/// rather than an outline. Normalized from the old `stroke-width="1"`
/// (an accidental third weight, neither the outline grammar's 2 nor a
/// deliberate solid choice).
// Kept local: uzor's const of this name draws differently (see uzor-gaps.md).
pub const ICON_ARROW_UP: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor" stroke="none" stroke-width="1.25">
  <path d="M12 4l8 14H4z"/>
</svg>"##;

/// Solid sub-grammar — see [`ICON_ARROW_UP`].
// Kept local: uzor's const of this name draws differently (see uzor-gaps.md).
pub const ICON_ARROW_DOWN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor" stroke="none" stroke-width="1.25">
  <path d="M12 20l8-14H4z"/>
</svg>"##;

// =============================================================================
// Brush Icons
// =============================================================================

pub const ICON_BRUSH: &str = uzor::icons::ui::ICON_BRUSH;

pub const ICON_HIGHLIGHTER: &str = uzor::icons::ui::ICON_HIGHLIGHTER;

// =============================================================================
// Annotation Icons
// =============================================================================

pub const ICON_TEXT: &str = uzor::icons::ui::ICON_TEXT;

pub const ICON_ANCHORED_TEXT: &str = uzor::icons::ui::ICON_ANCHORED_TEXT;

pub const ICON_NOTE: &str = uzor::icons::ui::ICON_NOTE;

pub const ICON_PRICE_NOTE: &str = uzor::icons::ui::ICON_PRICE_NOTE;

pub const ICON_SIGNPOST: &str = uzor::icons::ui::ICON_SIGNPOST;

pub const ICON_TABLE: &str = uzor::icons::ui::ICON_TABLE;

pub const ICON_CALLOUT: &str = uzor::icons::ui::ICON_CALLOUT;

pub const ICON_COMMENT: &str = uzor::icons::ui::ICON_COMMENT;

pub const ICON_PRICE_LABEL: &str = uzor::icons::ui::ICON_PRICE_LABEL;

pub const ICON_SIGN: &str = uzor::icons::ui::ICON_SIGN;

pub const ICON_FLAG: &str = uzor::icons::ui::ICON_FLAG;

pub const ICON_DIAMOND: &str = uzor::icons::ui::ICON_DIAMOND;

pub const ICON_EMOJI: &str = uzor::icons::ui::ICON_EMOJI;

pub const ICON_IMAGE: &str = uzor::icons::ui::ICON_IMAGE;

// =============================================================================
// Measurement Icons
// =============================================================================

pub const ICON_PRICE_RANGE: &str = uzor::icons::ui::ICON_PRICE_RANGE;

pub const ICON_DATE_RANGE: &str = uzor::icons::ui::ICON_DATE_RANGE;

pub const ICON_PRICE_DATE_RANGE: &str = uzor::icons::ui::ICON_PRICE_DATE_RANGE;

// =============================================================================
// Volume Icons
// =============================================================================

/// Generic base (bounded range) — see [`ICON_FIXED_VOLUME_PROFILE`] /
/// [`ICON_ANCHORED_VOLUME_PROFILE`] for the two registered primitives.
pub const ICON_VOLUME_PROFILE: &str = uzor::icons::ui::ICON_VOLUME_PROFILE;

/// Fixed volume profile: bars bounded by TWO vertical boundaries (left +
/// right) — a profile over a fixed, closed range.
pub const ICON_FIXED_VOLUME_PROFILE: &str = uzor::icons::chart::ICON_FIXED_VOLUME_PROFILE;

/// Anchored volume profile (icon audit item 2 — was byte-identical to the
/// fixed variant). ONE anchor point (marked dot + stem), bars growing
/// forward with no closing boundary — an open-ended profile from a single
/// anchor, not a bounded range.
pub const ICON_ANCHORED_VOLUME_PROFILE: &str = uzor::icons::chart::ICON_ANCHORED_VOLUME_PROFILE;

/// Anchored VWAP: anchor point (marked) + the VWAP line + dashed ±1/±2σ
/// deviation bands (matches the tooltip). Was wrongly aliased to the plain
/// Line Chart glyph (icon audit item 1 — unresolved, no distinguishing icon
/// at all before this).
pub const ICON_ANCHORED_VWAP: &str = uzor::icons::chart::ICON_ANCHORED_VWAP;

/// Fixed VWAP: same VWAP line + dashed ±1/±2σ bands as
/// [`ICON_ANCHORED_VWAP`], but bounded by TWO vertical boundaries (left +
/// right) instead of one anchor point — the `ICON_FIXED_VOLUME_PROFILE` /
/// `ICON_ANCHORED_VOLUME_PROFILE` distinction, reapplied to VWAP.
pub const ICON_FIXED_VWAP: &str = uzor::icons::chart::ICON_FIXED_VWAP;

// =============================================================================
// Projection Icons
// =============================================================================

pub const ICON_BARS_PATTERN: &str = uzor::icons::ui::ICON_BARS_PATTERN;

pub const ICON_PRICE_PROJECTION: &str = uzor::icons::ui::ICON_PRICE_PROJECTION;

pub const ICON_PROJECTION: &str = uzor::icons::ui::ICON_PROJECTION;

// =============================================================================
// Markup Engine Icons  (candle patterns, smart money, AMD, order flow)
// =============================================================================
//
// One shared visual grammar, so a reader can tell the four engines apart at
// 24px: a ZONE is a rectangle with a 0.2-0.3 fill, a LEVEL is a dashed
// horizontal, a CLOSE past something is a solid body, a WICK past something
// is a bare line, and every SCANNER carries the same lens. The three
// scanners deliberately differ only in what sits under that lens.

/// A three-bar group with the middle bar picked out — the shape of a
/// claim about specific bars, which is what this primitive is.
pub const ICON_CANDLE_PATTERN: &str = uzor::icons::chart::ICON_CANDLE_PATTERN;

/// Candles under a lens — a survey of what the bars contain, as opposed
/// to [`ICON_CANDLE_PATTERN`]'s single named claim.
pub const ICON_CANDLE_PATTERN_SCAN: &str = uzor::icons::chart::ICON_CANDLE_PATTERN_SCAN;

/// The opposing candle that a displacement left behind, drawn as a zone
/// with the leg breaking away from it.
pub const ICON_ORDER_BLOCK: &str = uzor::icons::chart::ICON_ORDER_BLOCK;

/// Two bars with an untouched band between them — the gap is the subject,
/// so the band is what the icon fills, not the bars.
pub const ICON_FAIR_VALUE_GAP: &str = uzor::icons::chart::ICON_FAIR_VALUE_GAP;

/// A wick spiking through a level and a body closing back under it — the
/// whole distinction between a sweep and a break of structure in one shape.
pub const ICON_LIQUIDITY_SWEEP: &str = uzor::icons::chart::ICON_LIQUIDITY_SWEEP;

/// A level with a body closing through it — the CLOSE is the event, so the
/// icon shows a body past the line rather than a wick.
pub const ICON_STRUCTURE_BREAK: &str = uzor::icons::chart::ICON_STRUCTURE_BREAK;

/// A range halved at its equilibrium — the upper half shaded as premium,
/// the lower left open as discount.
pub const ICON_DEALING_RANGE: &str = uzor::icons::chart::ICON_DEALING_RANGE;

/// A zone and a level under a lens — the smart-money survey, sharing
/// [`ICON_CANDLE_PATTERN_SCAN`]'s lens so the two read as one family.
pub const ICON_SMC_SCAN: &str = uzor::icons::chart::ICON_SMC_SCAN;

/// The three phases in order: a flat accumulation range, a spike out of it
/// that closes back inside, and the leg away.
pub const ICON_AMD_CYCLE: &str = uzor::icons::chart::ICON_AMD_CYCLE;

/// A 5-wave impulse leg under the bracket scan-motif, five dotted pivots
/// marking each wave's own endpoint — `icons_v4.json` batch B's own
/// `elliott_scan` body (2026-09-23 toolbar icons, drawn ahead of A2, wired
/// here), sharing `ICON_SMC_SCAN`'s own bracket wrapper so every autodetector
/// reads as one family.
pub const ICON_ELLIOTT_SCAN: &str = uzor::icons::chart::ICON_ELLIOTT_SCAN;

/// The 6-point `0-X-A-B-C-D` zigzag Duddella's own "5-0" names, with the
/// pattern's own completion triangle tinted — `icons_v4.json` batch B's own
/// `harmonic_five_o` body (2026-09-23 toolbar icons, drawn ahead of A3,
/// wired here), the manual counterpart to `ICON_HARMONIC_SCAN` (A3.5).
pub const ICON_HARMONIC_FIVE_O: &str = uzor::icons::chart::ICON_HARMONIC_FIVE_O;

/// The bracket scan-motif around a 5-0-shaped leg, tinted — `icons_v4.json`
/// batch B's own `harmonic_scan` body, sharing `ICON_SMC_SCAN`'s/
/// `ICON_ELLIOTT_SCAN`'s own bracket wrapper so every autodetector reads as
/// one family.
pub const ICON_HARMONIC_SCAN: &str = uzor::icons::chart::ICON_HARMONIC_SCAN;

/// The bracket scan-motif around a stack of tinted horizontal Fib-level
/// bands — `icons_v4.json` batch B's own `fib_scan` body (2026-09-23
/// toolbar icons, drawn ahead of A4, wired here), sharing `ICON_SMC_SCAN`'s/
/// `ICON_ELLIOTT_SCAN`'s/`ICON_HARMONIC_SCAN`'s own bracket wrapper so every
/// autodetector reads as one family.
pub const ICON_FIB_SCAN: &str = uzor::icons::chart::ICON_FIB_SCAN;

/// Two columns of unequal cells facing each other across a bar's centre —
/// the bid-versus-ask reading a footprint cell carries.
pub const ICON_IMBALANCE_ZONE: &str = uzor::icons::chart::ICON_IMBALANCE_ZONE;

/// Volume arriving into a level that does not move — arrows into a wall,
/// which is what absorption is.
pub const ICON_ABSORPTION_MARK: &str = uzor::icons::chart::ICON_ABSORPTION_MARK;

/// Price and delta pointing opposite ways — two legs from one origin,
/// which is the entire claim the mark makes.
pub const ICON_DELTA_DIVERGENCE: &str = uzor::icons::chart::ICON_DELTA_DIVERGENCE;

/// A footprint grid under the family lens — same lens as the other two
/// scanners, different subject beneath it.
pub const ICON_ORDER_FLOW_SCAN: &str = uzor::icons::chart::ICON_ORDER_FLOW_SCAN;
// =============================================================================
// Tool Icons
// =============================================================================

pub const ICON_CROSSHAIR: &str = uzor::icons::ui::ICON_CROSSHAIR;

/// Terminal prompt icon (lucide `terminal`) — agents PTY mode.
pub const ICON_AGENT_TERMINAL: &str = uzor::icons::ui::ICON_AGENT_TERMINAL;

/// Chat bubble icon (lucide `message-square`) — agents inline/chat mode.
pub const ICON_AGENT_CHAT: &str = uzor::icons::ui::ICON_AGENT_CHAT;

pub const ICON_MAGNET: &str = uzor::icons::ui::ICON_MAGNET;

/// Strong magnet icon — same U-shape as ICON_MAGNET but with an electric discharge
/// (zigzag lightning bolt) between the two pole tips, indicating strong body-snap mode.
pub const ICON_MAGNET_STRONG: &str = uzor::icons::ui::ICON_MAGNET_STRONG;

pub const ICON_CURSOR: &str = uzor::icons::ui::ICON_CURSOR;

pub const ICON_HAND: &str = uzor::icons::ui::ICON_HAND;

// =============================================================================
// Action Icons
// =============================================================================

pub const ICON_UNDO: &str = uzor::icons::ui::ICON_UNDO;

pub const ICON_REDO: &str = uzor::icons::ui::ICON_REDO;

pub const ICON_DELETE: &str = uzor::icons::ui::ICON_DELETE;

/// Normalized 2026-08-07 (icon audit item 6): stroke-width 1.5 was an
/// accidental third weight outside the outline grammar's 2.
pub const ICON_LOCK: &str = uzor::icons::ui::ICON_LOCK;

pub const ICON_UNLOCK: &str = uzor::icons::ui::ICON_UNLOCK;

pub const ICON_EYE: &str = uzor::icons::ui::ICON_EYE;

/// THE system gear (owner-picked base, 2026-07-30): filled 8-tooth classic
/// (Material geometry) — crisp silhouette at small sizes. Every settings
/// gear in the app routes through this const (Icon::Settings / "settings" /
/// ToolbarIconId "Settings"); swap it to change the gear everywhere.
/// The owner-saved alternates live right below (ICON_SETTINGS_*).
pub const ICON_SETTINGS: &str = uzor::icons::ui::ICON_SETTINGS_FILLED;

// ── Gear collection (owner-saved pool, 2026-07-30) ──────────────────────────
// Alternates for ICON_SETTINGS; not wired anywhere — swap the const above to
// switch the base gear app-wide.

/// The pre-2026-07-30 gear (feather "settings") — tiny arcs, mushy small.
pub const ICON_SETTINGS_FEATHER: &str = uzor::icons::ui::ICON_SETTINGS_FEATHER;

/// Chunky ring + 8 stub teeth — reads best at the smallest sizes.
pub const ICON_SETTINGS_CHUNKY: &str = uzor::icons::ui::ICON_SETTINGS_CHUNKY;

/// Soft 8-lobe outline (tabler geometry) — cleaner outline than feather.
pub const ICON_SETTINGS_OUTLINE: &str = uzor::icons::ui::ICON_SETTINGS_OUTLINE;

/// Industrial: thick ring + 8 wide teeth — solid cog.
pub const ICON_SETTINGS_INDUSTRIAL: &str = uzor::icons::ui::ICON_SETTINGS_INDUSTRIAL;

pub const ICON_BOOKMARK: &str = uzor::icons::ui::ICON_BOOKMARK;

/// Owner-approved final design (2026-07-11) — Lucide `maximize-2`: two
/// diagonal outward-pointing arrows (top-right, bottom-left) with L-shaped
/// arrowheads anchored at the corners.
pub const ICON_EXPAND: &str = uzor::icons::ui::ICON_ZOOM_RESET;

pub const ICON_MOVE: &str = uzor::icons::ui::ICON_MOVE;

// =============================================================================
// Position Icons
// =============================================================================

/// Redrawn 2026-07-18 (owner-approved design round; see nemo/docs/mlc/research/icon-drafts/)
/// Redrawn 2026-08-07 (icon audit item 4 — read as the letter "L"): the
/// bracket polyline was a literal L-glyph. Now a reward/risk position box —
/// large reward zone above entry, small risk zone below, entry line, and an
/// up-pointing chevron for direction. A lettermark is not an icon; this is
/// the actual tool semantic (buy trade with stop/target).
/// Redrawn again 2026-08-07 (owner: the Long/Short arrows must be a true
/// mirror, not a same-band chevron flip): box moved to y=2..14 and the
/// stem+chevron assembly moved to y=17..22, a clear 3px gap below the box
/// so the arrow no longer crosses a rect edge. Long's arrow stays BELOW
/// the box, pointing up into it.
pub const ICON_LONG_POSITION: &str = uzor::icons::chart::ICON_LONG_POSITION;

/// Redrawn 2026-07-18 (owner-approved design round; see nemo/docs/mlc/research/icon-drafts/)
/// Redrawn 2026-08-07 (icon audit item 4 — read as the letter "S"): the
/// S-curve path was a literal S-glyph. Mirrors [`ICON_LONG_POSITION`]'s
/// reward/risk box — small risk zone above entry, large reward zone below
/// (price falling = profit), entry line, down-pointing chevron.
/// Redrawn again 2026-08-07 (owner: "в СП стрелочка должна быть сверху" —
/// a true mirror of Long, not the same y=14..20 band with only the chevron
/// endpoints flipped, which crossed the box's own bottom edge at y=17).
/// Box moved to y=10..22, arrow assembly moved to y=2..7 ABOVE the box,
/// pointing down into it.
pub const ICON_SHORT_POSITION: &str = uzor::icons::chart::ICON_SHORT_POSITION;

// =============================================================================
// Navigation Icons
// =============================================================================

pub const ICON_PLUS: &str = uzor::icons::ui::ICON_PLUS;

pub const ICON_INDICATORS: &str = uzor::icons::ui::ICON_INDICATORS;

pub const ICON_CHEVRON_DOWN: &str = uzor::icons::ui::ICON_CHEVRON_DOWN;

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

pub const ICON_STEP_LINE: &str = uzor::icons::ui::ICON_STEP_LINE;

pub const ICON_LINE_MARKERS: &str = uzor::icons::ui::ICON_LINE_MARKERS;

pub const ICON_HLC_AREA: &str = uzor::icons::ui::ICON_HLC_AREA;

// Kept local: uzor's const of this name draws differently (see uzor-gaps.md).
pub const ICON_ZOOM: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="10" cy="10" r="7"/>
  <path d="M21 21l-4.35-4.35"/>
  <path d="M10 7v6"/>
  <path d="M7 10h6"/>
</svg>"##;

pub const ICON_EYE_OFF: &str = uzor::icons::ui::ICON_EYE_OFF;

pub const ICON_COPY: &str = uzor::icons::ui::ICON_COPY;

pub const ICON_CLOSE: &str = uzor::icons::ui::ICON_CLOSE;

pub const ICON_CHEVRON_UP: &str = uzor::icons::ui::ICON_CHEVRON_UP;

pub const ICON_CHEVRON_RIGHT: &str = uzor::icons::ui::ICON_CHEVRON_RIGHT;

pub const ICON_MINUS: &str = uzor::icons::ui::ICON_MINUS;

pub const ICON_GRID: &str = uzor::icons::ui::ICON_GRID;

pub const ICON_LAYERS: &str = uzor::icons::ui::ICON_LAYERS;

pub const ICON_LAYOUT: &str = uzor::icons::ui::ICON_LAYOUT;

pub const ICON_SEARCH: &str = uzor::icons::ui::ICON_SEARCH;

pub const ICON_CLOCK: &str = uzor::icons::ui::ICON_CLOCK;

/// System event log glyph (Lucide "History" silhouette) — clock face plus a
/// counter-clockwise sweep arrow, distinct from the plain `ICON_CLOCK` used
/// by the timeframe selector / bottom clock.
pub const ICON_HISTORY: &str = uzor::icons::ui::ICON_HISTORY;

/// Envelope glyph (Lucide "Mail" silhouette) — right-toolbar Community
/// (Forum + Messages) button.
pub const ICON_MAIL: &str = uzor::icons::ui::ICON_MAIL;

pub const ICON_WATERMARK: &str = uzor::icons::ui::ICON_WATERMARK;

pub const ICON_LEGEND: &str = uzor::icons::ui::ICON_LEGEND;

pub const ICON_TOOLTIP: &str = uzor::icons::ui::ICON_TOOLTIP;

pub const ICON_WATCHLIST: &str = uzor::icons::ui::ICON_WATCHLIST;

pub const ICON_ALERT: &str = uzor::icons::ui::ICON_ALERT;

pub const ICON_TRADING: &str = uzor::icons::ui::ICON_TRADING;

pub const ICON_POSITIONS: &str = uzor::icons::ui::ICON_POSITIONS;

pub const ICON_PANEL_RIGHT: &str = uzor::icons::ui::ICON_PANEL_RIGHT;

pub const ICON_PANEL_BOTTOM: &str = uzor::icons::ui::ICON_PANEL_BOTTOM;

pub const ICON_PALETTE: &str = uzor::icons::ui::ICON_PALETTE;

pub const ICON_INFO: &str = uzor::icons::ui::ICON_INFO;

pub const ICON_SIGNAL: &str = uzor::icons::ui::ICON_SIGNAL;

pub const ICON_OBJECT_TREE: &str = uzor::icons::ui::ICON_OBJECT_TREE;

pub const ICON_ZOOM_IN: &str = uzor::icons::ui::ICON_ZOOM_IN;

pub const ICON_ZOOM_OUT: &str = uzor::icons::ui::ICON_ZOOM_OUT;

pub const ICON_ZOOM_RESET: &str = uzor::icons::ui::ICON_ZOOM_RESET;

pub const ICON_SCREENSHOT: &str = uzor::icons::ui::ICON_SCREENSHOT;

/// Camcorder: body plus the side lens. Not the photo camera (`ICON_SCREENSHOT`).
pub const ICON_VIDEO: &str = uzor::icons::ui::ICON_VIDEO;

/// Camcorder while the live socket is being opened: three dots in the body.
pub const ICON_VIDEO_CONNECTING: &str = uzor::icons::ui::ICON_VIDEO_CONNECTING;

/// Camcorder while this tab publishes: a filled record dot in the body.
pub const ICON_VIDEO_LIVE: &str = uzor::icons::ui::ICON_VIDEO_LIVE;

/// Camcorder while this tab only receives: a play triangle in the body.
pub const ICON_VIDEO_VIEWING: &str = uzor::icons::ui::ICON_VIDEO_VIEWING;

/// Camcorder with a slash: the live socket keeps failing.
pub const ICON_VIDEO_ERROR: &str = uzor::icons::ui::ICON_VIDEO_ERROR;

pub const ICON_CIRCUIT_BOARD: &str = uzor::icons::ui::ICON_CIRCUIT_BOARD;

/// CPU chip with pins — processor icon for Performance panel
pub const ICON_CPU: &str = uzor::icons::ui::ICON_CPU;

pub const ICON_NEW_WINDOW: &str = uzor::icons::ui::ICON_NEW_WINDOW;

// =============================================================================
// User / Auth / Cloud Icons
// =============================================================================

pub const ICON_CLOUD: &str = uzor::icons::ui::ICON_CLOUD;

pub const ICON_CLOUD_DOWNLOAD: &str = uzor::icons::ui::ICON_CLOUD_DOWNLOAD;

pub const ICON_USER: &str = uzor::icons::ui::ICON_USER;

pub const ICON_LOG_IN: &str = uzor::icons::ui::ICON_LOG_IN;

pub const ICON_LOG_OUT: &str = uzor::icons::ui::ICON_LOG_OUT;

pub const ICON_CHEVRON_LEFT: &str = uzor::icons::ui::ICON_CHEVRON_LEFT;

pub const ICON_REFRESH: &str = uzor::icons::ui::ICON_REFRESH;

pub const ICON_SHIELD: &str = uzor::icons::ui::ICON_SHIELD;

pub const ICON_SHIELD_CHECK: &str = uzor::icons::ui::ICON_SHIELD_CHECK;

pub const ICON_GLOBE: &str = uzor::icons::ui::ICON_GLOBE;

pub const ICON_KEY: &str = uzor::icons::ui::ICON_KEY;

pub const ICON_BOT: &str = uzor::icons::ui::ICON_BOT;

// =============================================================================
// Typed sidebar slot icons (owner redesign 2026-07-11)
// =============================================================================
//
// Replaces the 4 generic aviation-icon slot buttons (from uzor's aviation
// set, visually unrelated to trading) with 5 type-specific glyphs matching
// the house stroke-2 24x24 style used across the rest of the toolbar.

/// Trading slot — briefcase glyph (hosts `TradingContainer`). Owner-approved
/// final design 2026-07-11.
pub const ICON_SLOT_TRADING: &str = uzor::icons::chart::ICON_SLOT_TRADING;

/// DOM slot glyph variant (redrawn 2026-07-18; kept as a variant per
/// owner order 2026-08-01 — the ACTIVE rail badge is the text chip
/// below, «текущие скины можно оставить просто как варианты»).
pub const ICON_SLOT_DOM_GLYPH: &str = uzor::icons::chart::ICON_SLOT_DOM_GLYPH;

/// DOM slot — text chip "DOM" (owner order 2026-08-01 «нужно написать
/// DOM TT L2 BT»; same chip grammar as the L2 badge).
pub const ICON_SLOT_DOM: &str = uzor::icons::chart::ICON_SLOT_DOM;

/// Trade Tape slot glyph variant (redrawn 2026-07-18; kept as a variant
/// per owner order 2026-08-01).
pub const ICON_SLOT_TAPE_GLYPH: &str = uzor::icons::chart::ICON_SLOT_TAPE_GLYPH;

/// Trade Tape slot — text chip "TT" (owner order 2026-08-01).
pub const ICON_SLOT_TAPE: &str = uzor::icons::chart::ICON_SLOT_TAPE;

/// L2 Tape slot — text-chip glyph. Redrawn 2026-07-18 (owner-picked
/// chip design; see nemo/docs/mlc/research/icon-drafts/); previous design
/// owner-picked (round 4, L-b) 2026-07-11.
pub const ICON_SLOT_L2: &str = uzor::icons::chart::ICON_SLOT_L2;

/// Big Trades slot glyph variant — sonar/ripple (owner-picked #12 of 24,
/// 2026-07-11; kept as a variant per owner order 2026-08-01).
pub const ICON_SLOT_BIG_TRADES_GLYPH: &str = uzor::icons::chart::ICON_SLOT_BIG_TRADES_GLYPH;

/// Big Trades slot — text chip "BT" (owner order 2026-08-01).
pub const ICON_SLOT_BIG_TRADES: &str = uzor::icons::chart::ICON_SLOT_BIG_TRADES;

/// Liquidations slot chip — "LIQ" in the slot text-chip grammar
/// (owner spec 2026-08-01, plan D: rail button under BT).
pub const ICON_SLOT_LIQ: &str = uzor::icons::chart::ICON_SLOT_LIQ;

/// Screener panel rail button — "SCR" text chip (plan F, owner spec
/// 2026-08-01: `docs/mlc/plans/orderflow-panels-arc2-2026-08-01.md` §F).
/// Same rect+stroked-letter-path chip grammar as the typed-slot text chips
/// above (copied from `ICON_SLOT_LIQ`'s grammar) even though Screener is a
/// plain sidebar panel, not a typed slot — provisional icon, owner will
/// rename/re-icon later.
pub const ICON_SCREENER: &str = uzor::icons::ui::ICON_SCREENER;

/// Performance panel — gauge / speedometer, distinct from `Signal` (Zap bolt).
/// The two used to collide (Performance reused `ICON_SIGNAL` before the
/// typed-slot redesign 2026-07-11).
pub const ICON_GAUGE: &str = uzor::icons::ui::ICON_GAUGE;

pub const ICON_MENU: &str = uzor::icons::ui::ICON_MENU;


pub const ICON_LINE_SOLID: &str = uzor::icons::ui::ICON_LINE_SOLID;

pub const ICON_LINE_DASHED: &str = uzor::icons::ui::ICON_LINE_DASHED;

pub const ICON_LINE_DOTTED: &str = uzor::icons::ui::ICON_LINE_DOTTED;

pub const ICON_PENCIL: &str = uzor::icons::ui::ICON_PENCIL;

pub const ICON_COLOR_FILL: &str = uzor::icons::ui::ICON_COLOR_FILL;

pub const ICON_TEXT_COLOR: &str = uzor::icons::ui::ICON_TEXT_COLOR;

pub const ICON_LINE_WIDTH_1: &str = uzor::icons::ui::ICON_LINE_WIDTH_1;

pub const ICON_LINE_WIDTH_2: &str = uzor::icons::ui::ICON_LINE_WIDTH_2;

pub const ICON_LINE_WIDTH_3: &str = uzor::icons::ui::ICON_LINE_WIDTH_3;

pub const ICON_LINE_WIDTH_4: &str = uzor::icons::ui::ICON_LINE_WIDTH_4;

pub const ICON_MORE_HORIZONTAL: &str = uzor::icons::ui::ICON_MORE_HORIZONTAL;

pub const ICON_LAYOUT_SINGLE: &str = uzor::icons::ui::ICON_LAYOUT_SINGLE;

pub const ICON_LAYOUT_SPLIT_H: &str = uzor::icons::ui::ICON_LAYOUT_SPLIT_H;

pub const ICON_LAYOUT_SPLIT_V: &str = uzor::icons::ui::ICON_LAYOUT_SPLIT_V;

pub const ICON_LAYOUT_GRID_2X2: &str = uzor::icons::ui::ICON_LAYOUT_GRID_2X2;

pub const ICON_LAYOUT_2LEFT_1RIGHT: &str = uzor::icons::ui::ICON_LAYOUT_2LEFT_1RIGHT;

pub const ICON_LAYOUT_1LEFT_2RIGHT: &str = uzor::icons::ui::ICON_LAYOUT_1LEFT_2RIGHT;

pub const ICON_LAYOUT_2TOP_1BOTTOM: &str = uzor::icons::ui::ICON_LAYOUT_2TOP_1BOTTOM;

pub const ICON_LAYOUT_1TOP_2BOTTOM: &str = uzor::icons::ui::ICON_LAYOUT_1TOP_2BOTTOM;

pub const ICON_LAYOUT_3COLUMNS: &str = uzor::icons::ui::ICON_LAYOUT_3COLUMNS;

pub const ICON_LAYOUT_3ROWS: &str = uzor::icons::ui::ICON_LAYOUT_3ROWS;

pub const ICON_LAYOUT_1BIG_3SMALL: &str = uzor::icons::ui::ICON_LAYOUT_1BIG_3SMALL;

/// Owner-approved final design (2026-07-11) — Lucide `minimize-2`: two
/// diagonal inward-pointing arrows (mirrors `ICON_EXPAND`'s outward
/// brackets), rendered by the bottom-toolbar expand button when the panel
/// IS expanded (see `render_chart.rs`: `if is_maximized { Icon::Collapse }
/// else { Icon::Expand }`).
// Kept local: uzor's const of this name draws differently (see uzor-gaps.md).
pub const ICON_COLLAPSE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round">
  <polyline points="4 14 10 14 10 20"/>
  <polyline points="20 10 14 10 14 4"/>
  <line x1="14" y1="10" x2="21" y2="3"/>
  <line x1="3" y1="21" x2="10" y2="14"/>
</svg>"##;

// =============================================================================
// Star Icons (Watchlist)
// =============================================================================

pub const ICON_STAR: &str = uzor::icons::ui::ICON_STAR;

// `stroke="currentColor" stroke-width="2"` matches [`ICON_STAR`]'s outline
// exactly (same path, same stroke width) so the filled and outline glyphs
// occupy identical visual bounds — without it the filled star has no stroke
// outset and renders visibly smaller than its outline sibling (owner report
// 2026-07-16: "star icon shrinks when filled").
// Kept local: uzor's const of this name draws differently (see uzor-gaps.md).
pub const ICON_STAR_FILLED: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="1.25"><path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z"/></svg>"##;

// =============================================================================
// Pin Icon (pinned-favorites-row toggle header)
// =============================================================================

/// Pushpin outline — same 24x24 stroke style as [`ICON_STAR`] et al. Used
/// for the `timeframe_selector` dropdown's FAVORITES-header pin toggle
/// (user-granularity-templates-inner arc, кусок 3 — defect fix 2026-07-16
/// replaced the 📌 text glyph with this native SVG). Color alone conveys
/// ON/OFF state (accent vs theme secondary) — no separate filled variant.
pub const ICON_PIN: &str = uzor::icons::ui::ICON_PIN;

// =============================================================================
// Autodetector Rail Icon Families (2026-09-23, owner-approved icon set)
//
// Registered ahead of the autodetector rail UI itself (a later, separate
// phase) -- these six constants + their `icon_svg()` ids exist so the rail
// can wire directly to them once built; nothing in the toolbar resolves
// these ids yet.
// =============================================================================

pub const ICON_FAMILY_EWA: &str = uzor::icons::chart::ICON_FAMILY_EWA;

pub const ICON_FAMILY_BARS: &str = uzor::icons::chart::ICON_FAMILY_BARS;

pub const ICON_FAMILY_SMC: &str = uzor::icons::chart::ICON_FAMILY_SMC;

pub const ICON_FAMILY_ORDER_FLOW: &str = uzor::icons::chart::ICON_FAMILY_ORDER_FLOW;

pub const ICON_FAMILY_HARMONICS: &str = uzor::icons::chart::ICON_FAMILY_HARMONICS;

pub const ICON_FAMILY_CLASSIC: &str = uzor::icons::chart::ICON_FAMILY_CLASSIC;

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
            Icon::Baseline => ICON_BASELINE,
            Icon::TrendLine => ICON_TREND_LINE,
            Icon::HorizontalLine => ICON_HORIZONTAL_LINE,
            Icon::VerticalLine => ICON_VERTICAL_LINE,
            Icon::Ray => ICON_RAY,
            Icon::RayLeft => ICON_RAY_LEFT,
            Icon::ExtendedLine => ICON_EXTENDED_LINE,
            Icon::ParallelChannel => ICON_PARALLEL_CHANNEL,
            Icon::HorizontalRay => ICON_HORIZONTAL_RAY,
            Icon::HorizontalRayLeft => ICON_HORIZONTAL_RAY_LEFT,
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
            Icon::CypherPattern => ICON_CYPHER_PATTERN,
            Icon::HeadShoulders => ICON_HEAD_SHOULDERS,
            Icon::AbcdPattern => ICON_ABCD_PATTERN,
            Icon::TrianglePattern => ICON_TRIANGLE_PATTERN,
            Icon::ThreeDrives => ICON_THREE_DRIVES,
            Icon::ElliottImpulse => ICON_ELLIOTT_IMPULSE,
            Icon::ElliottCorrection => ICON_ELLIOTT_CORRECTION,
            Icon::ElliottTriangle => ICON_ELLIOTT_TRIANGLE,
            Icon::ElliottCombo => ICON_ELLIOTT_COMBO,
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
            Icon::Pin => ICON_PIN,
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
            Icon::History => ICON_HISTORY,
            Icon::Mail => ICON_MAIL,
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
            Icon::Bot => ICON_BOT,
            Icon::Gauge => ICON_GAUGE,
            Icon::SlotTrading => ICON_SLOT_TRADING,
            Icon::SlotDom => ICON_SLOT_DOM,
            Icon::SlotTape => ICON_SLOT_TAPE,
            Icon::SlotL2 => ICON_SLOT_L2,
            Icon::SlotBigTrades => ICON_SLOT_BIG_TRADES,
            Icon::SlotLiq => ICON_SLOT_LIQ,
            Icon::Screener => ICON_SCREENER,
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
            "baseline" => Some(Icon::Baseline),
            "trend_line" | "trendline" => Some(Icon::TrendLine),
            "horizontal_line" | "horizontalline" => Some(Icon::HorizontalLine),
            "vertical_line" | "verticalline" => Some(Icon::VerticalLine),
            "ray" => Some(Icon::Ray),
            "ray_left" | "rayleft" => Some(Icon::RayLeft),
            "extended_line" | "extendedline" => Some(Icon::ExtendedLine),
            "parallel_channel" | "parallelchannel" => Some(Icon::ParallelChannel),
            "horizontal_ray" | "horizontalray" => Some(Icon::HorizontalRay),
            "horizontal_ray_left" | "horizontalrayleft" => Some(Icon::HorizontalRayLeft),
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
            "pin" => Some(Icon::Pin),
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
            "history" | "events" => Some(Icon::History),
            "mail" | "envelope" | "community" => Some(Icon::Mail),
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
            "bot" => Some(Icon::Bot),
            "gauge" => Some(Icon::Gauge),
            "slot_trading" | "slottrading" => Some(Icon::SlotTrading),
            "slot_dom" | "slotdom" => Some(Icon::SlotDom),
            "slot_tape" | "slottape" => Some(Icon::SlotTape),
            "slot_l2" | "slotl2" => Some(Icon::SlotL2),
            "slot_big_trades" | "slotbigtrades" => Some(Icon::SlotBigTrades),
            "slot_liq" | "slotliq" => Some(Icon::SlotLiq),
            "screener" => Some(Icon::Screener),
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
        // Derived / price-action / orderflow chart types — each resolves to
        // its own dedicated glyph (2026-07-10,
        // `docs/mlc/research/chart-type-icons-2026-07-10/`); previously all
        // 13 fell back to shared ICON_BAR_CHART/ICON_HISTOGRAM/ICON_LINE_CHART
        // /ICON_CANDLESTICK arms, making every derived type visually
        // indistinguishable from its siblings in the dropdown.
        "range_bar" | "rangebar" => Some(ICON_RANGE_BAR),
        "volume_bar" | "volumebar" => Some(ICON_VOLUME_BAR),
        "tick_bar" | "tickbar" | "tick" => Some(ICON_TICK_BAR),
        "footprint" => Some(ICON_FOOTPRINT_CELLS),
        "pnf" | "point_and_figure" | "pointandfigure" => Some(ICON_PNF),
        "kagi" => Some(ICON_KAGI),
        "renko" => Some(ICON_RENKO),
        "three_line_break" | "threelinebreak" | "3lb" => Some(ICON_THREE_LINE_BREAK),
        // "cvd" removed (2026-07-04 taxonomy fix) — no longer a chart_type
        // dropdown entry (demoted to a subpane overlay toggle).
        "tpo" | "market_profile" => Some(ICON_TPO),
        "dollar_bar" | "dollarbar" => Some(ICON_DOLLAR_BAR),
        "tick_imbalance_bar" | "tickimbalancebar" => Some(ICON_TICK_IMBALANCE_BAR),
        "volume_imbalance_bar" | "volumeimbalancebar" => Some(ICON_VOLUME_IMBALANCE_BAR),
        "run_bar" | "runbar" => Some(ICON_RUN_BAR),
        // "dom_heatmap"/"liquidation_heatmap" REMOVED (chart-type → overlay
        // migration) — both are now indicator-picker overlay entries whose
        // icon resolves via the generic "histogram"/"activity" arms below.

        // Drawing tools
        "trend_line" | "trendline" => Some(ICON_TREND_LINE),
        "horizontal_line" | "horizontalline" => Some(ICON_HORIZONTAL_LINE),
        "vertical_line" | "verticalline" => Some(ICON_VERTICAL_LINE),
        "ray" => Some(ICON_RAY),
        "ray_left" | "rayleft" => Some(ICON_RAY_LEFT),
        "extended_line" | "extendedline" => Some(ICON_EXTENDED_LINE),
        "parallel_channel" | "parallelchannel" => Some(ICON_PARALLEL_CHANNEL),
        "horizontal_ray" | "horizontalray" => Some(ICON_HORIZONTAL_RAY),
        "horizontal_ray_left" | "horizontalrayleft" => Some(ICON_HORIZONTAL_RAY_LEFT),
        "cross_line" | "crossline" => Some(ICON_CROSS_LINE),
        "info_line" | "infoline" => Some(ICON_INFO_LINE),
        "trend_angle" | "trendangle" => Some(ICON_TREND_ANGLE),

        // Channels
        "regression_trend" | "regressiontrend" => Some(ICON_REGRESSION_TREND),
        "flat_top_bottom" | "flattopbottom" => Some(ICON_FLAT_TOP_BOTTOM),
        "disjoint_channel" | "disjointchannel" => Some(ICON_DISJOINT_CHANNEL),

        // Pitchforks — 4 distinct tools, 4 distinct glyphs (2026-08-07; used
        // to collapse onto one shared ICON_PITCHFORK).
        "pitchfork" => Some(ICON_PITCHFORK),
        "schiff_pitchfork" | "schiffpitchfork" => Some(ICON_SCHIFF_PITCHFORK),
        "modified_schiff" | "modifiedschiff" => Some(ICON_MODIFIED_SCHIFF),
        "inside_pitchfork" | "insidepitchfork" => Some(ICON_INSIDE_PITCHFORK),

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
        "gann_square" | "gannsquare" => Some(ICON_GANN_SQUARE),
        "gann_square_fixed" | "gannsquarefixed" => Some(ICON_GANN_SQUARE_FIXED),
        "gann_fan" | "gannfan" => Some(ICON_GANN_FAN),

        // Patterns
        "xabcd_pattern" | "xabcdpattern" => Some(ICON_XABCD_PATTERN),
        "cypher_pattern" | "cypherpattern" => Some(ICON_CYPHER_PATTERN),
        "head_shoulders" | "headshoulders" => Some(ICON_HEAD_SHOULDERS),
        "abcd_pattern" | "abcdpattern" => Some(ICON_ABCD_PATTERN),
        "triangle_pattern" | "trianglepattern" => Some(ICON_TRIANGLE_PATTERN),
        "three_drives" | "threedrives" => Some(ICON_THREE_DRIVES),
        "harmonic_five_o" | "harmonicfiveo" => Some(ICON_HARMONIC_FIVE_O),

        // Elliott waves
        "elliott_impulse" | "elliottimpulse" => Some(ICON_ELLIOTT_IMPULSE),
        "elliott_correction" | "elliottcorrection" => Some(ICON_ELLIOTT_CORRECTION),
        "elliott_triangle" | "elliotttriangle" => Some(ICON_ELLIOTT_TRIANGLE),
        "elliott_combo" | "elliottcombo" => Some(ICON_ELLIOTT_COMBO),
        "elliott_double_combo" | "elliottdoublecombo" => Some(ICON_ELLIOTT_DOUBLE_COMBO),
        "elliott_triple_combo" | "elliotttriplecombo" => Some(ICON_ELLIOTT_TRIPLE_COMBO),

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
        "volume_profile" | "volumeprofile" => Some(ICON_VOLUME_PROFILE),
        "fixed_volume_profile" | "fixedvolumeprofile" => Some(ICON_FIXED_VOLUME_PROFILE),
        "anchored_volume_profile" | "anchoredvolumeprofile" => Some(ICON_ANCHORED_VOLUME_PROFILE),
        "anchored_vwap" | "anchoredvwap" => Some(ICON_ANCHORED_VWAP),
        "fixed_vwap" | "fixedvwap" => Some(ICON_FIXED_VWAP),
        "bars_pattern" | "barspattern" => Some(ICON_BARS_PATTERN),

        // Markup engines (candle patterns, smart money, AMD, order flow)
        "candle_pattern" | "candlepattern" => Some(ICON_CANDLE_PATTERN),
        "candle_pattern_scan" | "candlepatternscan" => Some(ICON_CANDLE_PATTERN_SCAN),
        "bars_scan" | "barsscan" => Some(ICON_BARS_SCAN),

        // Named harmonic toolbar presets (autodetectors A0, 2026-09-24)
        "harmonic_gartley" | "harmonicgartley" => Some(ICON_HARMONIC_GARTLEY),
        "harmonic_bat" | "harmonicbat" => Some(ICON_HARMONIC_BAT),
        "harmonic_alt_bat" | "harmonicaltbat" => Some(ICON_HARMONIC_ALT_BAT),
        "harmonic_butterfly" | "harmonicbutterfly" => Some(ICON_HARMONIC_BUTTERFLY),
        "harmonic_crab" | "harmoniccrab" => Some(ICON_HARMONIC_CRAB),
        "harmonic_deep_crab" | "harmonicdeepcrab" => Some(ICON_HARMONIC_DEEP_CRAB),
        "harmonic_shark" | "harmonicshark" => Some(ICON_HARMONIC_SHARK),
        "order_block" | "orderblock" => Some(ICON_ORDER_BLOCK),
        "fair_value_gap" | "fairvaluegap" => Some(ICON_FAIR_VALUE_GAP),
        "liquidity_sweep" | "liquiditysweep" => Some(ICON_LIQUIDITY_SWEEP),
        "structure_break" | "structurebreak" => Some(ICON_STRUCTURE_BREAK),
        "dealing_range" | "dealingrange" => Some(ICON_DEALING_RANGE),
        "smc_scan" | "smcscan" => Some(ICON_SMC_SCAN),
        "amd_cycle" | "amdcycle" => Some(ICON_AMD_CYCLE),
        "imbalance_zone" | "imbalancezone" => Some(ICON_IMBALANCE_ZONE),
        "absorption_mark" | "absorptionmark" => Some(ICON_ABSORPTION_MARK),
        "delta_divergence" | "deltadivergence" => Some(ICON_DELTA_DIVERGENCE),
        "order_flow_scan" | "orderflowscan" => Some(ICON_ORDER_FLOW_SCAN),

        // Autodetector A2, Elliott (2026-09-24)
        "elliott_scan" | "elliottscan" => Some(ICON_ELLIOTT_SCAN),

        // Autodetector A3, Harmonics (2026-09-24)
        "harmonic_scan" | "harmonicscan" => Some(ICON_HARMONIC_SCAN),

        // Autodetector A4, Fib (2026-09-24)
        "fib_scan" | "fibscan" => Some(ICON_FIB_SCAN),

        // Autodetector rail families (2026-09-23, registered ahead of the rail UI)
        "family_ewa" => Some(ICON_FAMILY_EWA),
        "family_bars" => Some(ICON_FAMILY_BARS),
        "family_smc" => Some(ICON_FAMILY_SMC),
        "family_order_flow" | "familyorderflow" => Some(ICON_FAMILY_ORDER_FLOW),
        "family_harmonics" => Some(ICON_FAMILY_HARMONICS),
        "family_classic" => Some(ICON_FAMILY_CLASSIC),
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
        "cpu" | "activity" => Some(ICON_CPU),
        "gauge" | "performance" => Some(ICON_GAUGE),
        "terminal" | "pty" => Some(ICON_AGENT_TERMINAL),
        "chat_bubble" | "chatbubble" | "chat" => Some(ICON_AGENT_CHAT),
        "new_window" | "newwindow" | "external_link" | "open_new" => Some(ICON_NEW_WINDOW),
        "cloud" => Some(ICON_CLOUD),
        "cloud_download" | "clouddownload" => Some(ICON_CLOUD_DOWNLOAD),
        "user" => Some(ICON_USER),
        "log_in" | "login" => Some(ICON_LOG_IN),
        "log_out" | "logout" => Some(ICON_LOG_OUT),
        // The chevrons resolve by name. Only `chevron_left` and `chevron_down` did until
        // 2026-08-13: right and up existed as consts and as `Icon`
        // variants but had no arm here, so any caller that names an icon by
        // STRING — every `IconId::new("…")` in a toolbar — got `None` and
        // drew an empty button. Two of the Elliott inline controls shipped
        // blank for exactly that reason.
        "chevron_left" | "chevronleft" => Some(ICON_CHEVRON_LEFT),
        "chevron_right" | "chevronright" => Some(ICON_CHEVRON_RIGHT),
        "chevron_up" | "chevronup" => Some(ICON_CHEVRON_UP),
        "refresh" | "refresh_cw" | "refreshcw" => Some(ICON_REFRESH),
        "shield" => Some(ICON_SHIELD),
        "shield_check" | "shieldcheck" => Some(ICON_SHIELD_CHECK),
        "globe" => Some(ICON_GLOBE),
        "key" => Some(ICON_KEY),
        "bot" => Some(ICON_BOT),
        "slot_trading" | "slottrading" => Some(ICON_SLOT_TRADING),
        "slot_dom" | "slotdom" => Some(ICON_SLOT_DOM),
        "slot_tape" | "slottape" => Some(ICON_SLOT_TAPE),
        "slot_l2" | "slotl2" => Some(ICON_SLOT_L2),
        "slot_big_trades" | "slotbigtrades" => Some(ICON_SLOT_BIG_TRADES),
        "slot_liq" | "slotliq" => Some(ICON_SLOT_LIQ),
        "screener" => Some(ICON_SCREENER),
        // System event log button (bottom of the right toolbar, owner scope
        // §8, 2026-09-05) — "bell"/"list" already mean Alerts/Watchlist.
        "history" | "events" => Some(ICON_HISTORY),
        // Community (Forum + Messages) envelope button, directly above
        // `events` on the right toolbar (community-modal arc W1/W2). The
        // generic toolbar rail resolves `ToolbarIconId` through THIS
        // function (`toolbar_core.rs`'s own `icon_svg(icon_id.name())`),
        // never through `Icon::from_id`/`Icon::svg()` (the OTHER "mail"
        // match, above, which callers holding an already-resolved `Icon`
        // value use instead — `draw_svg_icon(ctx, Icon::X.svg(), ...)`
        // call sites like `forum_v2_overlay.rs`'s lock/pin glyphs). Adding
        // `ICON_MAIL` to the enum's own match was not enough: this arm was
        // still missing, so the toolbar button rendered with no glyph at
        // all — found in review of the shipped W1 build (owner, 2026-09-09).
        "mail" | "envelope" | "community" => Some(ICON_MAIL),

        // Bottom toolbar zoom/screenshot buttons
        "zoom_in" | "zoomin" => Some(ICON_ZOOM_IN),
        "zoom_out" | "zoomout" => Some(ICON_ZOOM_OUT),
        "zoom_reset" | "zoomreset" | "maximize2" => Some(ICON_ZOOM_RESET),
        "screenshot" | "camera" => Some(ICON_SCREENSHOT),
        "video" | "video_camera" | "videocam" => Some(ICON_VIDEO),
        "video_connecting" | "videoconnecting" => Some(ICON_VIDEO_CONNECTING),
        "video_live" | "videolive" => Some(ICON_VIDEO_LIVE),
        "video_viewing" | "videoviewing" => Some(ICON_VIDEO_VIEWING),
        "video_error" | "videoerror" => Some(ICON_VIDEO_ERROR),

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
        "star" => Some(ICON_STAR),
        "star_filled" | "starfilled" => Some(ICON_STAR_FILLED),
        "pin" => Some(ICON_PIN),
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

#[cfg(test)]
mod chart_type_icon_tests {
    //! Coverage for the 2026-07-10 chart-type icon integration
    //! (`docs/mlc/research/chart-type-icons-2026-07-10/`) — every registry
    //! row must resolve a real icon, and the 13 previously-stubbed derived
    //! types must no longer share identical SVG data with each other (the
    //! whole point of replacing the shared BarChart/Histogram/LineChart
    //! fallback arms).
    use super::*;

    /// The 13 chart_type ids the task named as newly-covered (previously
    /// stubbed onto ICON_BAR_CHART / ICON_HISTOGRAM / ICON_LINE_CHART /
    /// ICON_CANDLESTICK — see the research README §2 audit table).
    const NEWLY_COVERED: &[&str] = &[
        "range_bar", "volume_bar", "dollar_bar", "tick_bar",
        "tick_imbalance_bar", "volume_imbalance_bar", "run_bar",
        "renko", "pnf", "kagi", "three_line_break", "footprint", "tpo",
    ];

    #[test]
    fn every_registry_row_resolves_a_real_icon() {
        for def in crate::chart_type::all() {
            let resolved = icon_svg(def.icon_id);
            assert!(
                resolved.is_some(),
                "chart_type id={} icon_id={} must resolve a real icon",
                def.id, def.icon_id,
            );
        }
    }

    #[test]
    fn every_registry_row_resolves_via_its_own_chart_type_id() {
        // Same check keyed on the chart_type `id` string directly (what
        // `icon_svg` actually dispatches on in practice via
        // `ToolbarIconId::new(def.icon_id)` → normalized lowercase, which
        // for every one of these rows collapses to the same string as the
        // chart_type id itself) — belt-and-suspenders against a future
        // `icon_id` drifting out of sync with the `icon_svg` match arms.
        for def in crate::chart_type::all() {
            let resolved = icon_svg(def.id);
            assert!(resolved.is_some(), "chart_type id={} must resolve via its own id string", def.id);
        }
    }

    #[test]
    fn newly_covered_chart_types_have_distinct_non_shared_icons() {
        let mut seen: Vec<(&str, *const u8, usize)> = Vec::new();
        for &ct in NEWLY_COVERED {
            let svg = icon_svg(ct).unwrap_or_else(|| panic!("{ct} must resolve an icon"));
            let ptr = svg.as_ptr();
            let len = svg.len();
            for &(other_ct, other_ptr, other_len) in &seen {
                // Compare by CONTENT, not just pointer identity — two
                // distinct `&'static str` constants with byte-identical
                // SVG text would still be "shared" in the sense this test
                // guards against (a copy-paste that forgot to change the
                // glyph). Pointer/len fast-path first, full content compare
                // as the actual assertion.
                let other_svg = icon_svg(other_ct).unwrap_or_else(|| panic!("{other_ct} must resolve an icon"));
                assert_ne!(
                    svg, other_svg,
                    "{ct} and {other_ct} must NOT share identical SVG data (both newly-covered types)",
                );
                let _ = (other_ptr, other_len);
            }
            seen.push((ct, ptr, len));
        }
    }

    #[test]
    fn newly_covered_chart_types_no_longer_fall_back_to_legacy_shared_glyphs() {
        // Direct regression pin for the exact bug this task fixed: none of
        // the 13 previously-stubbed types may resolve to the OLD shared
        // fallback constants anymore.
        let legacy_shared = [ICON_BAR_CHART, ICON_HISTOGRAM, ICON_LINE_CHART, ICON_CANDLESTICK];
        for &ct in NEWLY_COVERED {
            let svg = icon_svg(ct).unwrap_or_else(|| panic!("{ct} must resolve an icon"));
            assert!(
                !legacy_shared.contains(&svg),
                "{ct} must not resolve to a legacy shared fallback glyph anymore",
            );
        }
    }
}

#[cfg(test)]
mod typed_slot_icon_tests {
    //! Coverage for the typed-slot sidebar redesign (2026-07-11): 5 new
    //! slot icons replacing the generic aviation glyphs, Performance getting
    //! its own Gauge glyph (previously collided with Signals' Zap bolt), and
    //! the owner-approved final `ICON_EXPAND`/`ICON_COLLAPSE` pair (Lucide
    //! `maximize-2`/`minimize-2`).
    use super::*;

    /// The 5 typed-slot `Icon` enum variants, in canonical slot index order
    /// (matches `mlc_app_sidebar_content::free_slot::SlotKind::ALL`).
    const SLOT_ICONS: &[Icon] = &[
        Icon::SlotTrading,
        Icon::SlotDom,
        Icon::SlotTape,
        Icon::SlotL2,
        Icon::SlotBigTrades,
        Icon::SlotLiq,
    ];

    /// Every typed-slot icon resolves to non-empty, well-formed SVG.
    #[test]
    fn slot_icons_resolve_nonempty_svg() {
        for icon in SLOT_ICONS {
            let svg = icon.svg();
            assert!(!svg.is_empty(), "{:?} must resolve non-empty SVG", icon);
            assert!(svg.starts_with("<svg"), "{:?} SVG must start with <svg tag", icon);
            assert!(svg.trim_end().ends_with("</svg>"), "{:?} SVG must end with </svg>", icon);
        }
    }

    /// All 5 typed-slot icons are visually distinct from each other (no two
    /// slots silently share a glyph — each was hand-designed per owner spec:
    /// Trading=order-ticket, DOM=ladder, Tape=vertical-tape, L2=ladder+tape
    /// hybrid, BigTrades=oversized-block-among-rows).
    #[test]
    fn slot_icons_are_pairwise_distinct() {
        for i in 0..SLOT_ICONS.len() {
            for j in (i + 1)..SLOT_ICONS.len() {
                assert_ne!(
                    SLOT_ICONS[i].svg(), SLOT_ICONS[j].svg(),
                    "{:?} and {:?} must not share identical SVG data",
                    SLOT_ICONS[i], SLOT_ICONS[j],
                );
            }
        }
    }

    /// None of the 5 new slot icons reuse the deleted aviation glyphs'
    /// silhouette accidentally by resolving to an unrelated pre-existing
    /// icon constant (defence against a copy-paste that forgot to swap in
    /// the new path data).
    #[test]
    fn slot_icons_are_distinct_from_unrelated_existing_icons() {
        let unrelated = [Icon::Bot, Icon::Watchlist, Icon::Alert, Icon::CircuitBoard, Icon::Gauge];
        for slot_icon in SLOT_ICONS {
            for other in &unrelated {
                assert_ne!(
                    slot_icon.svg(), other.svg(),
                    "{:?} must not collide with unrelated icon {:?}",
                    slot_icon, other,
                );
            }
        }
    }

    /// Performance now has its own Gauge glyph, distinct from Signals' Zap
    /// bolt — the two collided (`render.rs` used `Icon::Signal` for both
    /// panel titles) before this redesign.
    #[test]
    fn performance_icon_distinct_from_signals_icon() {
        assert_ne!(
            Icon::Gauge.svg(), Icon::Signal.svg(),
            "Performance (Gauge) must not collide with Signals (Signal/Zap)",
        );
    }

    /// The right-toolbar rail's icon-id strings resolve to the SAME SVG data
    /// as the corresponding `Icon` enum variant, across both lookup tables
    /// (`Icon::svg()` used by sidebar panel headers, `icon_svg()` used by
    /// `ToolbarIconId` rail buttons) — the two tables must never drift.
    #[test]
    fn slot_toolbar_icon_ids_match_icon_enum_svg() {
        let pairs: &[(&str, Icon)] = &[
            ("slot_trading", Icon::SlotTrading),
            ("slot_dom", Icon::SlotDom),
            ("slot_tape", Icon::SlotTape),
            ("slot_l2", Icon::SlotL2),
            ("slot_big_trades", Icon::SlotBigTrades),
            ("slot_liq", Icon::SlotLiq),
            ("gauge", Icon::Gauge),
        ];
        for (name, icon) in pairs {
            let by_name = icon_svg(name).unwrap_or_else(|| panic!("icon_svg(\"{name}\") must resolve"));
            assert_eq!(
                by_name, icon.svg(),
                "icon_svg(\"{}\") must match {:?}::svg()", name, icon,
            );
        }
    }

    /// `performance` toolbar icon-id resolves to Gauge, NOT the old
    /// `Activity`/`ICON_CPU` alias it collided with pre-redesign.
    #[test]
    fn performance_toolbar_icon_id_resolves_to_gauge() {
        let performance_svg = icon_svg("performance").expect("\"performance\" must resolve");
        assert_eq!(performance_svg, Icon::Gauge.svg());
        assert_ne!(performance_svg, ICON_CPU, "\"performance\" must not resolve to the old CPU/Activity glyph");
    }

    /// `ICON_EXPAND` owner-approved final design (2026-07-11, Lucide
    /// `maximize-2`): two `<polyline>` brackets (top-right, bottom-left)
    /// plus two diagonal `<line>` arrows pointing outward — no `<path>`
    /// elements (distinguishes it from the earlier stroke-bracket redraw).
    #[test]
    fn icon_expand_matches_lucide_maximize2_shape() {
        let svg = ICON_EXPAND;
        assert_eq!(svg.matches("<polyline").count(), 2, "ICON_EXPAND must have exactly 2 polylines: {svg}");
        assert_eq!(svg.matches("<line").count(), 2, "ICON_EXPAND must have exactly 2 diagonal lines: {svg}");
        assert_eq!(svg.matches("<path").count(), 0, "ICON_EXPAND (Lucide maximize-2) must not use <path>: {svg}");
    }

    /// `ICON_COLLAPSE` owner-approved final design (2026-07-11, Lucide
    /// `minimize-2`): mirrors `ICON_EXPAND`'s shape (2 polylines + 2
    /// diagonal lines, inward instead of outward) and remains visually
    /// distinct from it.
    #[test]
    fn icon_collapse_matches_lucide_minimize2_shape() {
        let svg = ICON_COLLAPSE;
        assert_eq!(svg.matches("<polyline").count(), 2, "ICON_COLLAPSE must have exactly 2 polylines: {svg}");
        assert_eq!(svg.matches("<line").count(), 2, "ICON_COLLAPSE must have exactly 2 diagonal lines: {svg}");
        assert_ne!(ICON_COLLAPSE, ICON_EXPAND, "ICON_COLLAPSE must remain visually distinct from ICON_EXPAND");
    }
}

#[cfg(test)]
mod icon_stroke_width_guard {
    //! Guard against a Lucide-default paste (`stroke-width="2"`) slipping
    //! back into the icon set: uzor's SVG renderer honors the ROOT
    //! `stroke-width` directly now (no more fixed-1.5-ignore-root formula,
    //! see uzor commit f11176c) so every icon here must carry an explicit,
    //! thin root value. House standard is 1.25 for toolbar/chrome glyphs;
    //! emoji stickers (a separate family, drawn on the chart itself --
    //! `drawing/primitives_v2/icons/emoji.rs`) are a different guard, not
    //! this one (root pinned to 1.5 there to preserve pre-fix weight).
    use super::*;

    const ALL_ICON_CONSTS: &[(&str, &str)] = &[
        ("ICON_CANDLESTICK", ICON_CANDLESTICK),
        ("ICON_HOLLOW_CANDLES", ICON_HOLLOW_CANDLES),
        ("ICON_HEIKIN_ASHI", ICON_HEIKIN_ASHI),
        ("ICON_LINE_CHART", ICON_LINE_CHART),
        ("ICON_AREA_CHART", ICON_AREA_CHART),
        ("ICON_BAR_CHART", ICON_BAR_CHART),
        ("ICON_HISTOGRAM", ICON_HISTOGRAM),
        ("ICON_BASELINE", ICON_BASELINE),
        ("ICON_RANGE_BAR", ICON_RANGE_BAR),
        ("ICON_VOLUME_BAR", ICON_VOLUME_BAR),
        ("ICON_DOLLAR_BAR", ICON_DOLLAR_BAR),
        ("ICON_TICK_BAR", ICON_TICK_BAR),
        ("ICON_TICK_IMBALANCE_BAR", ICON_TICK_IMBALANCE_BAR),
        ("ICON_VOLUME_IMBALANCE_BAR", ICON_VOLUME_IMBALANCE_BAR),
        ("ICON_RUN_BAR", ICON_RUN_BAR),
        ("ICON_RENKO", ICON_RENKO),
        ("ICON_PNF", ICON_PNF),
        ("ICON_KAGI", ICON_KAGI),
        ("ICON_THREE_LINE_BREAK", ICON_THREE_LINE_BREAK),
        ("ICON_FOOTPRINT_CELLS", ICON_FOOTPRINT_CELLS),
        ("ICON_TPO", ICON_TPO),
        ("ICON_TREND_LINE", ICON_TREND_LINE),
        ("ICON_HORIZONTAL_LINE", ICON_HORIZONTAL_LINE),
        ("ICON_VERTICAL_LINE", ICON_VERTICAL_LINE),
        ("ICON_RAY", ICON_RAY),
        ("ICON_RAY_LEFT", ICON_RAY_LEFT),
        ("ICON_EXTENDED_LINE", ICON_EXTENDED_LINE),
        ("ICON_PARALLEL_CHANNEL", ICON_PARALLEL_CHANNEL),
        ("ICON_HORIZONTAL_RAY", ICON_HORIZONTAL_RAY),
        ("ICON_HORIZONTAL_RAY_LEFT", ICON_HORIZONTAL_RAY_LEFT),
        ("ICON_CROSS_LINE", ICON_CROSS_LINE),
        ("ICON_INFO_LINE", ICON_INFO_LINE),
        ("ICON_TREND_ANGLE", ICON_TREND_ANGLE),
        ("ICON_REGRESSION_TREND", ICON_REGRESSION_TREND),
        ("ICON_FLAT_TOP_BOTTOM", ICON_FLAT_TOP_BOTTOM),
        ("ICON_DISJOINT_CHANNEL", ICON_DISJOINT_CHANNEL),
        ("ICON_PITCHFORK", ICON_PITCHFORK),
        ("ICON_SCHIFF_PITCHFORK", ICON_SCHIFF_PITCHFORK),
        ("ICON_MODIFIED_SCHIFF", ICON_MODIFIED_SCHIFF),
        ("ICON_INSIDE_PITCHFORK", ICON_INSIDE_PITCHFORK),
        ("ICON_FIB_RETRACEMENT", ICON_FIB_RETRACEMENT),
        ("ICON_FIB_EXTENSION", ICON_FIB_EXTENSION),
        ("ICON_FIB_CHANNEL", ICON_FIB_CHANNEL),
        ("ICON_FIB_CIRCLE", ICON_FIB_CIRCLE),
        ("ICON_FIB_SPIRAL", ICON_FIB_SPIRAL),
        ("ICON_FIB_TIME_ZONES", ICON_FIB_TIME_ZONES),
        ("ICON_FIB_SPEED_RESISTANCE", ICON_FIB_SPEED_RESISTANCE),
        ("ICON_FIB_TREND_TIME", ICON_FIB_TREND_TIME),
        ("ICON_FIB_ARCS", ICON_FIB_ARCS),
        ("ICON_FIB_WEDGE", ICON_FIB_WEDGE),
        ("ICON_FIB_FAN", ICON_FIB_FAN),
        ("ICON_GANN_BOX", ICON_GANN_BOX),
        ("ICON_GANN_SQUARE", ICON_GANN_SQUARE),
        ("ICON_GANN_SQUARE_FIXED", ICON_GANN_SQUARE_FIXED),
        ("ICON_GANN_FAN", ICON_GANN_FAN),
        ("ICON_XABCD_PATTERN", ICON_XABCD_PATTERN),
        ("ICON_CYPHER_PATTERN", ICON_CYPHER_PATTERN),
        ("ICON_HEAD_SHOULDERS", ICON_HEAD_SHOULDERS),
        ("ICON_ABCD_PATTERN", ICON_ABCD_PATTERN),
        ("ICON_TRIANGLE_PATTERN", ICON_TRIANGLE_PATTERN),
        ("ICON_THREE_DRIVES", ICON_THREE_DRIVES),
        ("ICON_ELLIOTT_IMPULSE", ICON_ELLIOTT_IMPULSE),
        ("ICON_ELLIOTT_CORRECTION", ICON_ELLIOTT_CORRECTION),
        ("ICON_ELLIOTT_TRIANGLE", ICON_ELLIOTT_TRIANGLE),
        ("ICON_ELLIOTT_COMBO", ICON_ELLIOTT_COMBO),
        ("ICON_ELLIOTT_TRIPLE_COMBO", ICON_ELLIOTT_TRIPLE_COMBO),
        ("ICON_CYCLE_LINES", ICON_CYCLE_LINES),
        ("ICON_TIME_CYCLES", ICON_TIME_CYCLES),
        ("ICON_SINE_WAVE", ICON_SINE_WAVE),
        ("ICON_RECTANGLE", ICON_RECTANGLE),
        ("ICON_ROTATED_RECTANGLE", ICON_ROTATED_RECTANGLE),
        ("ICON_CIRCLE", ICON_CIRCLE),
        ("ICON_ELLIPSE", ICON_ELLIPSE),
        ("ICON_TRIANGLE", ICON_TRIANGLE),
        ("ICON_ARC", ICON_ARC),
        ("ICON_POLYLINE", ICON_POLYLINE),
        ("ICON_PATH", ICON_PATH),
        ("ICON_CURVE", ICON_CURVE),
        ("ICON_DOUBLE_CURVE", ICON_DOUBLE_CURVE),
        ("ICON_ARROW", ICON_ARROW),
        ("ICON_ARROW_UP", ICON_ARROW_UP),
        ("ICON_ARROW_DOWN", ICON_ARROW_DOWN),
        ("ICON_BRUSH", ICON_BRUSH),
        ("ICON_HIGHLIGHTER", ICON_HIGHLIGHTER),
        ("ICON_TEXT", ICON_TEXT),
        ("ICON_ANCHORED_TEXT", ICON_ANCHORED_TEXT),
        ("ICON_NOTE", ICON_NOTE),
        ("ICON_PRICE_NOTE", ICON_PRICE_NOTE),
        ("ICON_SIGNPOST", ICON_SIGNPOST),
        ("ICON_TABLE", ICON_TABLE),
        ("ICON_CALLOUT", ICON_CALLOUT),
        ("ICON_COMMENT", ICON_COMMENT),
        ("ICON_PRICE_LABEL", ICON_PRICE_LABEL),
        ("ICON_SIGN", ICON_SIGN),
        ("ICON_FLAG", ICON_FLAG),
        ("ICON_DIAMOND", ICON_DIAMOND),
        ("ICON_EMOJI", ICON_EMOJI),
        ("ICON_IMAGE", ICON_IMAGE),
        ("ICON_PRICE_RANGE", ICON_PRICE_RANGE),
        ("ICON_DATE_RANGE", ICON_DATE_RANGE),
        ("ICON_PRICE_DATE_RANGE", ICON_PRICE_DATE_RANGE),
        ("ICON_VOLUME_PROFILE", ICON_VOLUME_PROFILE),
        ("ICON_FIXED_VOLUME_PROFILE", ICON_FIXED_VOLUME_PROFILE),
        ("ICON_ANCHORED_VOLUME_PROFILE", ICON_ANCHORED_VOLUME_PROFILE),
        ("ICON_ANCHORED_VWAP", ICON_ANCHORED_VWAP),
        ("ICON_FIXED_VWAP", ICON_FIXED_VWAP),
        ("ICON_BARS_PATTERN", ICON_BARS_PATTERN),
        ("ICON_PRICE_PROJECTION", ICON_PRICE_PROJECTION),
        ("ICON_PROJECTION", ICON_PROJECTION),
        ("ICON_CANDLE_PATTERN", ICON_CANDLE_PATTERN),
        ("ICON_CANDLE_PATTERN_SCAN", ICON_CANDLE_PATTERN_SCAN),
        ("ICON_BARS_SCAN", ICON_BARS_SCAN),
        ("ICON_HARMONIC_GARTLEY", ICON_HARMONIC_GARTLEY),
        ("ICON_HARMONIC_BAT", ICON_HARMONIC_BAT),
        ("ICON_HARMONIC_ALT_BAT", ICON_HARMONIC_ALT_BAT),
        ("ICON_HARMONIC_BUTTERFLY", ICON_HARMONIC_BUTTERFLY),
        ("ICON_HARMONIC_CRAB", ICON_HARMONIC_CRAB),
        ("ICON_HARMONIC_DEEP_CRAB", ICON_HARMONIC_DEEP_CRAB),
        ("ICON_HARMONIC_SHARK", ICON_HARMONIC_SHARK),
        ("ICON_ORDER_BLOCK", ICON_ORDER_BLOCK),
        ("ICON_FAIR_VALUE_GAP", ICON_FAIR_VALUE_GAP),
        ("ICON_LIQUIDITY_SWEEP", ICON_LIQUIDITY_SWEEP),
        ("ICON_STRUCTURE_BREAK", ICON_STRUCTURE_BREAK),
        ("ICON_DEALING_RANGE", ICON_DEALING_RANGE),
        ("ICON_SMC_SCAN", ICON_SMC_SCAN),
        ("ICON_AMD_CYCLE", ICON_AMD_CYCLE),
        ("ICON_IMBALANCE_ZONE", ICON_IMBALANCE_ZONE),
        ("ICON_ABSORPTION_MARK", ICON_ABSORPTION_MARK),
        ("ICON_DELTA_DIVERGENCE", ICON_DELTA_DIVERGENCE),
        ("ICON_ORDER_FLOW_SCAN", ICON_ORDER_FLOW_SCAN),
        ("ICON_ELLIOTT_SCAN", ICON_ELLIOTT_SCAN),
        ("ICON_HARMONIC_FIVE_O", ICON_HARMONIC_FIVE_O),
        ("ICON_HARMONIC_SCAN", ICON_HARMONIC_SCAN),
        ("ICON_FIB_SCAN", ICON_FIB_SCAN),
        ("ICON_CROSSHAIR", ICON_CROSSHAIR),
        ("ICON_AGENT_TERMINAL", ICON_AGENT_TERMINAL),
        ("ICON_AGENT_CHAT", ICON_AGENT_CHAT),
        ("ICON_MAGNET", ICON_MAGNET),
        ("ICON_MAGNET_STRONG", ICON_MAGNET_STRONG),
        ("ICON_CURSOR", ICON_CURSOR),
        ("ICON_HAND", ICON_HAND),
        ("ICON_UNDO", ICON_UNDO),
        ("ICON_REDO", ICON_REDO),
        ("ICON_DELETE", ICON_DELETE),
        ("ICON_LOCK", ICON_LOCK),
        ("ICON_UNLOCK", ICON_UNLOCK),
        ("ICON_EYE", ICON_EYE),
        ("ICON_SETTINGS", ICON_SETTINGS),
        ("ICON_SETTINGS_FEATHER", ICON_SETTINGS_FEATHER),
        ("ICON_SETTINGS_CHUNKY", ICON_SETTINGS_CHUNKY),
        ("ICON_SETTINGS_OUTLINE", ICON_SETTINGS_OUTLINE),
        ("ICON_SETTINGS_INDUSTRIAL", ICON_SETTINGS_INDUSTRIAL),
        ("ICON_BOOKMARK", ICON_BOOKMARK),
        ("ICON_EXPAND", ICON_EXPAND),
        ("ICON_MOVE", ICON_MOVE),
        ("ICON_LONG_POSITION", ICON_LONG_POSITION),
        ("ICON_SHORT_POSITION", ICON_SHORT_POSITION),
        ("ICON_PLUS", ICON_PLUS),
        ("ICON_INDICATORS", ICON_INDICATORS),
        ("ICON_CHEVRON_DOWN", ICON_CHEVRON_DOWN),
        ("ICON_STEP_LINE", ICON_STEP_LINE),
        ("ICON_LINE_MARKERS", ICON_LINE_MARKERS),
        ("ICON_HLC_AREA", ICON_HLC_AREA),
        ("ICON_EYE_OFF", ICON_EYE_OFF),
        ("ICON_COPY", ICON_COPY),
        ("ICON_CLOSE", ICON_CLOSE),
        ("ICON_CHEVRON_UP", ICON_CHEVRON_UP),
        ("ICON_CHEVRON_RIGHT", ICON_CHEVRON_RIGHT),
        ("ICON_MINUS", ICON_MINUS),
        ("ICON_GRID", ICON_GRID),
        ("ICON_LAYERS", ICON_LAYERS),
        ("ICON_LAYOUT", ICON_LAYOUT),
        ("ICON_SEARCH", ICON_SEARCH),
        ("ICON_CLOCK", ICON_CLOCK),
        ("ICON_HISTORY", ICON_HISTORY),
        ("ICON_MAIL", ICON_MAIL),
        ("ICON_WATERMARK", ICON_WATERMARK),
        ("ICON_LEGEND", ICON_LEGEND),
        ("ICON_TOOLTIP", ICON_TOOLTIP),
        ("ICON_WATCHLIST", ICON_WATCHLIST),
        ("ICON_ALERT", ICON_ALERT),
        ("ICON_TRADING", ICON_TRADING),
        ("ICON_POSITIONS", ICON_POSITIONS),
        ("ICON_PANEL_RIGHT", ICON_PANEL_RIGHT),
        ("ICON_PANEL_BOTTOM", ICON_PANEL_BOTTOM),
        ("ICON_PALETTE", ICON_PALETTE),
        ("ICON_INFO", ICON_INFO),
        ("ICON_SIGNAL", ICON_SIGNAL),
        ("ICON_OBJECT_TREE", ICON_OBJECT_TREE),
        ("ICON_ZOOM_IN", ICON_ZOOM_IN),
        ("ICON_ZOOM_OUT", ICON_ZOOM_OUT),
        ("ICON_ZOOM_RESET", ICON_ZOOM_RESET),
        ("ICON_SCREENSHOT", ICON_SCREENSHOT),
        ("ICON_CIRCUIT_BOARD", ICON_CIRCUIT_BOARD),
        ("ICON_CPU", ICON_CPU),
        ("ICON_NEW_WINDOW", ICON_NEW_WINDOW),
        ("ICON_CLOUD", ICON_CLOUD),
        ("ICON_CLOUD_DOWNLOAD", ICON_CLOUD_DOWNLOAD),
        ("ICON_USER", ICON_USER),
        ("ICON_LOG_IN", ICON_LOG_IN),
        ("ICON_LOG_OUT", ICON_LOG_OUT),
        ("ICON_CHEVRON_LEFT", ICON_CHEVRON_LEFT),
        ("ICON_REFRESH", ICON_REFRESH),
        ("ICON_SHIELD", ICON_SHIELD),
        ("ICON_SHIELD_CHECK", ICON_SHIELD_CHECK),
        ("ICON_GLOBE", ICON_GLOBE),
        ("ICON_KEY", ICON_KEY),
        ("ICON_BOT", ICON_BOT),
        ("ICON_SLOT_TRADING", ICON_SLOT_TRADING),
        ("ICON_SLOT_DOM_GLYPH", ICON_SLOT_DOM_GLYPH),
        ("ICON_SLOT_DOM", ICON_SLOT_DOM),
        ("ICON_SLOT_TAPE_GLYPH", ICON_SLOT_TAPE_GLYPH),
        ("ICON_SLOT_TAPE", ICON_SLOT_TAPE),
        ("ICON_SLOT_L2", ICON_SLOT_L2),
        ("ICON_SLOT_BIG_TRADES_GLYPH", ICON_SLOT_BIG_TRADES_GLYPH),
        ("ICON_SLOT_BIG_TRADES", ICON_SLOT_BIG_TRADES),
        ("ICON_SLOT_LIQ", ICON_SLOT_LIQ),
        ("ICON_SCREENER", ICON_SCREENER),
        ("ICON_GAUGE", ICON_GAUGE),
        ("ICON_MENU", ICON_MENU),
        ("ICON_LINE_SOLID", ICON_LINE_SOLID),
        ("ICON_LINE_DASHED", ICON_LINE_DASHED),
        ("ICON_LINE_DOTTED", ICON_LINE_DOTTED),
        ("ICON_PENCIL", ICON_PENCIL),
        ("ICON_COLOR_FILL", ICON_COLOR_FILL),
        ("ICON_TEXT_COLOR", ICON_TEXT_COLOR),
        ("ICON_LINE_WIDTH_1", ICON_LINE_WIDTH_1),
        ("ICON_LINE_WIDTH_2", ICON_LINE_WIDTH_2),
        ("ICON_LINE_WIDTH_3", ICON_LINE_WIDTH_3),
        ("ICON_LINE_WIDTH_4", ICON_LINE_WIDTH_4),
        ("ICON_MORE_HORIZONTAL", ICON_MORE_HORIZONTAL),
        ("ICON_LAYOUT_SINGLE", ICON_LAYOUT_SINGLE),
        ("ICON_LAYOUT_SPLIT_H", ICON_LAYOUT_SPLIT_H),
        ("ICON_LAYOUT_SPLIT_V", ICON_LAYOUT_SPLIT_V),
        ("ICON_LAYOUT_GRID_2X2", ICON_LAYOUT_GRID_2X2),
        ("ICON_LAYOUT_2LEFT_1RIGHT", ICON_LAYOUT_2LEFT_1RIGHT),
        ("ICON_LAYOUT_1LEFT_2RIGHT", ICON_LAYOUT_1LEFT_2RIGHT),
        ("ICON_LAYOUT_2TOP_1BOTTOM", ICON_LAYOUT_2TOP_1BOTTOM),
        ("ICON_LAYOUT_1TOP_2BOTTOM", ICON_LAYOUT_1TOP_2BOTTOM),
        ("ICON_LAYOUT_3COLUMNS", ICON_LAYOUT_3COLUMNS),
        ("ICON_LAYOUT_3ROWS", ICON_LAYOUT_3ROWS),
        ("ICON_LAYOUT_1BIG_3SMALL", ICON_LAYOUT_1BIG_3SMALL),
        ("ICON_COLLAPSE", ICON_COLLAPSE),
        ("ICON_STAR", ICON_STAR),
        ("ICON_STAR_FILLED", ICON_STAR_FILLED),
        ("ICON_PIN", ICON_PIN),
    ];

    #[test]
    fn every_icon_const_root_stroke_width_is_at_most_1_5() {
        for (name, svg) in ALL_ICON_CONSTS {
            let open = svg.find('>').unwrap_or_else(|| panic!("{name}: no closing '>' on root <svg> tag"));
            let root_tag = &svg[..=open];
            let width = root_tag
                .split("stroke-width=\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .and_then(|w| w.parse::<f64>().ok());
            // uzor's copies drop the root width where no root-level stroke
            // is drawn (fill-only icons, per-path widths), so only the ceiling holds.
            if let Some(w) = width {
                assert!(
                    w <= 1.5,
                    "{name}: root stroke-width {w} exceeds the 1.5 house ceiling -- looks like an unnormalized Lucide paste"
                );
            }
        }
    }
}
