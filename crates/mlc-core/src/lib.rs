//! zengeld-chart: Standalone charting library
//!
//! This crate provides core chart rendering functionality.
//! It can be used standalone or as a foundation for full-featured
//! chart applications.
//!
//! # Architecture
//!
//! The library is organized into several key modules:
//!
//! ## Engine (`engine`)
//! Core coordinate systems and viewport management:
//! - `viewport` - Chart viewport/coordinate system
//! - `price_scale` - Price axis calculations (nice number algorithm)
//! - `time_scale` - Time axis calculations (weight-based ticks)
//! - `kinetic` - Kinetic scrolling physics
//!
//! ## Layout (`layout`)
//! Platform-agnostic layout computation:
//! - `LayoutRect` - Simple rectangle type
//! - `ChartAreaLayout` - Chart + scales subdivision
//! - `FrameLayout` - Full frame layout (toolbars, sidebars, chart)
//! - `LayoutManager` - Convenience wrapper for layout computation
//!
//! ## Input (`input`)
//! Unified input handling:
//! - `drag_mode` - What is being dragged (DragMode)
//! - `action` - Semantic input actions (ChartInputAction)
//! - `handler` - Traits for hit testing and action handling
//!
//! ## Overlays (`overlays`)
//! Visual UI elements drawn on top of the chart:
//! - `crosshair` - Cursor tracking with magnet snap
//! - `grid` - Horizontal and vertical grid lines
//! - `legend` - OHLC value display
//! - `tooltip` - Detailed bar information
//! - `watermark` - Chart branding
//!
//!
//! ## Annotations (`annotations`)
//! Data-point markers and price lines:
//! - `markers` - Chart markers (buy/sell signals, etc.)
//! - `price_line` - Horizontal price levels
//!
//! ## Primitives (`primitives`)
//! Drawing tools:
//! - Trend lines, rectangles, horizontal/vertical lines
//! - Primitive manager and traits
//!
//! ## Series (`series`)
//! Data series types:
//! - Candlestick, line, area, histogram, baseline, bar
//! - Series options and data structures
//!
//! ## Core (`types`)
//! Fundamental types and constants:
//! - Theme, Bar, DragMode
//! - Pixel-perfect helpers (crisp, crisp_rect)

// =============================================================================
// Module Declarations
// =============================================================================

// Platform-agnostic storage trait + global handle (defined in mlc-vault to
// avoid an upward dependency cycle; re-exported here for convenience).
pub mod storage {
    pub use mlc_vault::storage::{
        Storage, FilesystemStorage, global_storage, set_global_storage,
    };
}
pub use storage::{Storage, global_storage, set_global_storage};

// Core types (remain at root)
pub mod types;

// Internationalization
pub mod i18n;

// Engine (rendering + input handling)
pub mod engine;

// Organized modules
pub mod chart;
pub mod drawing;
pub mod scale_settings;
pub mod state;
pub mod theme;

// Utility modules (platform-independent)
pub mod utils;

// Chart panel toolbar definitions
pub mod toolbar;


// Chart-local modal state
pub mod modal;

// Shared identifier + sync types — vocabulary used by both backend
// (mlc-backend::bubble) and app (mlc-app::tag).
pub mod ids;
pub use ids::{BubbleId, GroupView, MemberSyncOverride, SyncFlags, SyncMemberId};

// Pure value-types for indicators shared between backend and app.
pub mod indicator_types;

// Pure value-types for alerts shared between backend and app.
pub mod alert;

// Canonical instrument key (exchange, symbol, account_type) — see
// docs/mlc/plans/doctrine-cutover-2026-06-27.md §1C.
pub mod instrument;
pub use instrument::{AccountType, InstrumentAddress};

// Watchlist domain value types (no UI/presentation fields) — see
// docs/mlc/plans/doctrine-cutover-2026-06-27.md §1B.
pub mod watchlist;
pub use watchlist::{WatchlistGroup, WatchlistList};

// Tag domain value type (no UI/presentation fields) — see
// docs/mlc/plans/doctrine-cutover-2026-06-27.md §1B.
pub mod tag;
pub use tag::{Tag, TagId};

// Chart preset system (snapshot types for persistence)
pub mod preset;

// `MIN_PASSPHRASE_LENGTH` declared at the crate root so the layout layer can
// validate the passphrase form without depending on `mlc-app::user_manager`.
//
// `PresetSummary` projection — also for the layout layer (preset-browser
// modal); the app builds these from `ChartPreset` and hands a slice down.

/// Minimum profile-passphrase length (UI form validation).
pub const MIN_PASSPHRASE_LENGTH: usize = 8;

pub mod preset_view;
pub use preset_view::PresetSummary;

// Template system (reusable style configurations for primitives, indicators, compare)
pub mod templates;

// User profile system (aggregate persistent state — active selections, UI state)
pub mod user_profile;

// Indicator source trait (abstraction for indicator data access)
pub mod indicator_source;

// Data provider trait (abstraction for OHLC data loading)
pub mod data_provider;

// Chart output events (emitted to the host application)
pub mod events;

// Backwards compatibility re-exports
pub use engine::input as input;
pub use engine::render as render;

// =============================================================================
// Re-exports for convenient access
// =============================================================================

// Utility functions (platform-independent)
pub use utils::{parse_css_color, apply_opacity, rgba_to_hex};
pub use utils::format_indicator_value;
pub use utils::catmull_rom_spline;

// Core Types (from types.rs)
pub use types::{
    crisp, crisp_rect, Bar, FootprintBar, FootprintLevel, DragMode, Theme,
    find_bar_for_timestamp, bar_to_timestamp,
    find_bar_for_timestamp_ms, timestamp_ms_to_bar_f64, bar_f64_to_timestamp_ms, bar_interval_seconds,
    // Price scale constants
    PRICE_SCALE_BORDER_SIZE, PRICE_SCALE_FONT, PRICE_SCALE_FONT_SIZE,
    PRICE_SCALE_FONT_SIZE_MAX, PRICE_SCALE_FONT_SIZE_MIN,
    PRICE_SCALE_LABEL_OFFSET, PRICE_SCALE_MIN_WIDTH, PRICE_SCALE_WIDTH,
    PRICE_SCALE_PADDING_INNER, PRICE_SCALE_PADDING_OUTER, PRICE_SCALE_TICK_LENGTH,
    TIME_SCALE_HEIGHT, TIME_SCALE_FONT_SIZE,
    // Sidebar & toolbar constants
    LEFT_SIDEBAR_WIDTH, RIGHT_SIDEBAR_WIDTH, BOTTOM_SIDEBAR_HEIGHT,
    RIGHT_TOOLBAR_WIDTH, LEFT_TOOLBAR_WIDTH,
    BOTTOM_TOOLBAR_HEIGHT, TOP_TOOLBAR_HEIGHT, STATUS_BAR_HEIGHT,
};

// Layout geometry + interaction state (backend data types).
//
// These pure-data types were lifted out of the frontend `layout/` module into
// `state/` during the mlc-app-layout split (Phase X + V1). They are re-exported here
// from `state::` so `mlc_core::LayoutRect` etc keep working. The frontend
// rendering symbols that used to be re-exported here (render_*, LayoutManager,
// {Extended,}LayoutHitTester, FrameTheme, ChartRenderConfig, the *Result render
// outputs, ChartModal*) now live in the `mlc-app-layout` crate — consumers import
// those as `mlc_app_layout::X`.
pub use state::geometry::{
    LayoutRect, ChartAreaLayout, FrameLayout, ChartHitZone,
    ExtendedFrameLayout, SubPaneLayout,
    Margins, LayoutConfig, LayoutManager,
};
pub use state::scale_corner::{ScaleCornerState, ScaleCornerHitZones, ScaleCornerButton};
pub use state::sub_pane_overlay::SubPaneOverlayResult;
pub use state::toolbar_state::{ToolbarState, ToolbarClickResult, ToggleIconPair};
pub use state::chart_settings_data::{
    ChartSettingsData, InstrumentSettings, StatusLineSettings, ScalesLinesSettings,
};

// Frontend rendering symbols (LayoutHitTester, ExtendedLayoutHitTester,
// ChartRenderConfig, render_*, FrameTheme, RenderPass, the *Result render
// outputs, ChartModal*) moved to the `mlc-app-layout` crate in V1. Consumers now
// import them as `mlc_app_layout::X`. They are NOT re-exported from mlc-core.

// Input (unified input handling - events, actions, handlers, objects)
pub use input::{
    // Events (low-level)
    ChartInputAction, DragMode as InputDragMode, KeyCode, Modifiers, MouseButton,
    // Actions (high-level commands)
    ChartAction, Shortcut,
    // Handler
    ChartHitTester, ChartInputHandler, ChartInputState, ChartOutputAction,
    DefaultChartInputHandler, HitResult, InputHandlerConfig, UndoAction,
    // Objects (interaction)
    ChartObject, Configurable, ConfigProperty, ConfigPropertyType, CoordinateHelper, CursorStyle,
    DefaultStyles, DragAxis, DragConstraints, DragManager, DragState, Draggable, DraggableObject,
    FontStyleType, FontWeight, HitTestResult, ObjectCapabilities, ObjectEntry, ObjectRegistry,
    ObjectState, ObjectType, StyleSet, Styleable, UnifiedLineStyle, ZOrder, DRAG_THRESHOLD,
};

// Chart types (viewport, scales, overlays)
pub use chart::{
    // Viewport
    Viewport,
    // Price scale
    format_price, nice_number, nice_price_step, price_precision,
    PriceScale, PriceScaleMode, ScaleMode, NICE_MULTIPLIERS,
    // Time scale
    format_time_by_weight, format_time_full, format_time_by_weight_with_settings,
    format_time_full_with_settings, TickMarkWeight, TimeScale, TimeTick,
    DAY, HOUR, MINUTE,
    // Kinetic scrolling
    KineticState, KINETIC_DAMPING, KINETIC_FRICTION, KINETIC_MIN_VELOCITY,
    // Crosshair
    Crosshair, CrosshairLineOptions, CrosshairMode, CrosshairOptions,
    // Grid
    GridLineOptions, GridOptions,
    // Legend
    Legend, LegendData, LegendPosition,
    // Tooltip
    Tooltip, TooltipContent,
    // Watermark
    FontStyle, HorzAlign, VertAlign, Watermark, WatermarkLine,
    // Compare overlay
    CompareOverlay, CompareSeries, COMPARE_COLORS, get_compare_color,
};

// DOM Heatmap types (C12)
pub use chart::dom_heatmap::{DomHeatmapBuffer, DomHeatmapCell, DomTradeDot};

// Annotations (from chart module) — Marker family deleted; only price
// lines remain.
pub use chart::annotations::{LineStyle, PriceLine, PriceLineOptions};

// Series (from chart module)
pub use chart::series::{
    AreaData, AreaSeriesOptions, AreaStyleOptions, BarData, BarSeriesOptions, BarStyleOptions,
    BaselineData, BaselineSeriesOptions, BaselineStyleOptions, CandlestickData,
    CandlestickSeriesOptions, CandlestickStyleOptions, HistogramData, HistogramSeriesOptions,
    HistogramStyleOptions, LineData, LineSeriesOptions, LineStyleOptions, LineType,
    PriceLineSource, SeriesData, SeriesOptions, SeriesOptionsCommon, SeriesType, SingleValue,
};


// Theme System - from theme/
pub use theme::{
    // Chart-specific theme types (recommended for new code)
    ChartTheme, ChartColors, SeriesColors, ChartFonts,
    // Legacy UI theme types (terminal should migrate these to its own crate)
    UITheme, UIColors, UIFonts, UISizing, UIEffects,
    // Runtime Theme - dynamic configuration
    RuntimeTheme, RuntimeUIColors, RuntimeChartColors, RuntimeSeriesColors,
    RuntimeFonts, RuntimeSizing, RuntimeEffects,
    // Theme Manager - single source of truth
    ThemeManager,
    // Theme Settings Panel - definition for UI (terminal concern)
    ThemeSettingsPanel, ThemeSettingsSection, ThemeColorField, ThemeColorPath,
    // UI Styles (Solid, Glass, FrostedGlass, LiquidGlass) - orthogonal to themes
    UIStyle, StyleParams, OpacityType, GlassButtonStyle,
};


// Scale Settings (configuration for price/time scales)
pub use scale_settings::{
    ScaleSettings,
    PriceScalePosition,
    TimeScalePosition,
    ScaleCornerVisibility,
    DateFormat,
    TimeFormatSettings,
    DEFAULT_PRICE_SCALE_WIDTH,
    DEFAULT_TIME_SCALE_HEIGHT,
    MIN_PRICE_SCALE_WIDTH,
    MAX_PRICE_SCALE_WIDTH,
    MIN_TIME_SCALE_HEIGHT,
    MAX_TIME_SCALE_HEIGHT,
    cycle_precision,
    precision_label,
};

// State Management - Base chart and layout types
pub use state::{
    // (Legacy single-window `Chart` struct deleted — use `ChartContainer`.)
    // Visibility (standalone manager)
    VisibilityManager,
    // Lock (standalone manager)
    LockManager,
    // Object category (full terminal version with all categories)
    ObjectCategory,
    // Object info (legacy command support)
    ObjectInfo,
    // Timeframe
    TimeframeManager, Timeframe,
    // Pane management (multi-pane charts)
    PaneManager, Pane, PaneId, PaneGeometry, InteractionRegion, MAIN_PANE,
    // Sub-pane (unified geometry + Y-axis state for indicator panes)
    SubPane, SubPaneId, generate_sub_pane_id, bump_sub_pane_id_past,
    // Coordinate utilities (clamping, boundary policy)
    coordinate_utils,
    // Command pattern (undo/redo)
    Command, CommandResult, StateChange, ViewportState,
    PropertyValue, Position, CommandHistory,
    // Timeframe visibility for object tree
    TimeframeVisibility,
    // ChartContainer - the main chart state aggregate (formerly ChartWindow,
    // renamed in Phase 1 of the chart-container cut)
    ChartContainer, ChartId, ConnectionStatus, WindowRect, WINDOW_GAP,
    // Phase 4 S2.7 — per-canvas display-option + series-visibility state
    ChartPanelState,
    // Snap-to-end constant (canonical margin used by ChartContainer::snap_to_end)
    DEFAULT_SNAP_MARGIN,
    // Chart-internal split/expand system
    ChartPanelGrid, SplitHitResult, ChartInputTarget, ChartRightClickHit,
    ChartDragStartHit, FreehandCompleteResult, DrawingSession,
    // ChartId for the chart crate's ChartWindow
    generate_chart_id, bump_chart_id_past,
};

// Re-export uzor-panels types so callers don't need a direct dependency.
pub use uzor::panels::SplitKind;
pub use uzor::panels::SeparatorOrientation;
pub use uzor::panels::PanelRect;
pub use uzor::panels::LeafId;

// Overlay tab headers (render_leaf_tab, LeafTabHoverZone, LeafTabHitZones)
// moved to mlc-app-layout in V1 — consumers import them as mlc_app_layout::X.

// Drawing System (v2 - trait-based primitives)
//
// `DrawingManager` / `DrawingState` / `DragType` moved to
// `mlc-backend::drawing` (Step 3.1 physical move) — they are no
// longer re-exported here. Consumers import `mlc_backend::drawing::*`.
pub use drawing::{
    // Render-path read view (implemented by mlc-backend::drawing::BubbleDrawingView)
    DrawingRenderView,
    // Primitive trait and registry
    PrimitiveTrait, PrimitiveRegistry, PrimitiveMetadata, PrimitiveKind, ClickBehavior,
    PrimitiveData, PrimitiveFactory,
    // Hit testing and control points
    HitTestResult as DrawingHitTestResult, ControlPoint, ControlPointType, ControlPointCursor,
    // Styling
    PrimitiveColor, LineStyle as DrawingLineStyle, PrimitiveText, TextAlign, ExtendMode,
    // Geometry helpers
    point_to_line_distance, HIT_TOLERANCE,
    CONTROL_POINT_RADIUS, CONTROL_POINT_HIT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    // Rendering
    RenderContext, RenderOp, RenderOps, TextBaseline,
    render_crisp, render_crisp_rect, execute_ops, draw_control_points,
    render_primitive_text, render_primitive_text_rotated, render_text_with_background, TextAnchor,
    // Icons
    EmojiType,
    // System signals (strategy-generated markers) - types only
    SystemSignal, SignalType, SignalPrimitive, StrategySignalConfig,
    // Level configuration
    FibLevelConfig,
    // (SignalManager / TradeManager / Trade / TradeDirection deleted — see
    // `mlc-core::drawing` for the rationale.)
};

// Indicator Source (abstraction for indicator data access)
pub use indicator_source::{
    IndicatorInfo, IndicatorSource, NullIndicatorSource,
    // Rich render types
    HistogramStyle, IndicatorOutputRenderType, OutputRenderConfig,
    IndicatorOutputRenderDef, SignalRenderData, IndicatorRenderInstance,
    AlertRenderData, AlertRenderStatus,
    // Settings modal data
    IndicatorSettingsData,
};

// Data Provider (abstraction for OHLC data loading)
pub use data_provider::{DataProvider, SharedDataProvider, NullDataProvider};

// Chart output events
pub use events::ChartOutEvent;

// Chart-local modal state
pub use modal::ChartOpenModal;

// Toolbar interaction state + geometry (backend data). The frontend rendering
// pieces — ChartPanelLayout, ChartInternalLayout, compute_chart_internal_layout
// (panel_app), ToolbarTheme, DropdownTheme (ui widgets) — moved to mlc-app-layout
// in V1; consumers import those as mlc_app_layout::X.
pub use state::toolbar_state::{ChartToolbarRenderResult, ToolbarConfig, InlineDockEdge};
pub use state::toolbar_geometry::{ToolbarRect, ToolbarRenderResult};

// Render Module (new - platform-agnostic rendering foundation)
pub use render::{
    // RenderContext trait and helpers
    RenderContext as RenderContextTrait,
    TextAlign as RenderTextAlign,
    TextBaseline as RenderTextBaseline,
    crisp as render_crisp_val,
    crisp_rect as render_crisp_rect_val,
    RenderOp as RenderOperation,
    RenderOps as RenderOperations,
    execute_ops as execute_render_ops,
    // Input state
    InputState as RenderInputState,
    MouseButton as RenderMouseButton,
    ModifierKeys as RenderModifierKeys,
    PointerState as RenderPointerState,
    DragState as RenderDragState,
    Rect as RenderRect,
    // Frame result
    FrameResult, CursorIcon as RenderCursorIcon, RenderAction,
};

// Internationalization (i18n)
pub use i18n::{
    Language, current_language, set_language,
    t, t_tooltip, t_menu, t_config, t_wave, t_style, t_label_pos,
    t_primitive_name, t_primitive_tooltip, localize_primitive_name,
    TextKey, TooltipKey, MenuKey, ConfigKey, WaveDegreeKey, StyleKey, LabelPositionKey,
    MonthKey, month_names_short,
    Translatable,
    PrimitiveNameKey, PrimitiveTooltipKey,
};

// User profile (aggregate persistent state — active selections, UI state)
pub use user_profile::{
    UserProfile,
    VaultSecrets,
    WindowState,
    ProfileError,
    ProfileMeta,
    ProfileIndex,
    app_data_dir,
    get_user_data_dir,
    active_profile_data_dir,
    migrate_legacy_profile_if_needed,
    save_profile,
    load_profile,
    save_json,
    load_json,
    profiles_dir,
    load_profile_index,
    save_profile_index,
    create_profile,
    delete_profile,
    PersistedAgentLeaf,
    PersistedAgentCli,
    PersistedInstanceMode,
};

// `UserManager`, `ProfileInfo`, `ProfileManager`, `SwitchData`,
// `MIN_PASSPHRASE_LENGTH` moved to `mlc-app::user_manager`.

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overlays_integration() {
        let watermark = Watermark::simple("Test");
        assert!(watermark.visible);

        let legend = Legend::default();
        assert!(legend.visible);

        let tooltip = Tooltip::default();
        assert!(!tooltip.visible);

        let grid = GridOptions::default();
        assert!(grid.vert_lines.visible);
    }
}
