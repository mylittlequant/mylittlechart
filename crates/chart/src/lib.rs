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

// Core types (remain at root)
pub mod types;

// Internationalization
pub mod i18n;

// Engine (rendering + input handling)
pub mod engine;

// Layout (platform-agnostic layout computation)
pub mod layout;

// Organized modules
pub mod chart;
pub mod demo;
pub mod drawing;
pub mod scale_settings;
pub mod state;
pub mod theme;

// Utility modules (platform-independent)
pub mod utils;

// Chart panel toolbar definitions
pub mod toolbar;

// Chart UI rendering (self-contained toolbars and overlays, no core dependency)
pub mod ui;

// Chart-local modal state
pub mod modal;

// PanelApp implementation for the chart panel
pub mod panel_app;

// Tag / sync-group manager (Step 1–2 of the TagManager architecture)
pub mod tag_manager;

// Chart preset system (snapshot types for persistence)
pub mod preset;

// Template system (reusable style configurations for primitives, indicators, compare)
pub mod templates;

// User profile system (aggregate persistent state — active selections, UI state)
pub mod user_profile;

// Unified user state manager (single entry point for all user persistence)
pub mod user_manager;

// Unified cryptography module (key derivation, encryption, domain separation)
pub mod crypto;

// Zero-trust local encryption vault for profile data
pub mod vault;

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
    crisp, crisp_rect, Bar, DragMode, Theme,
    find_bar_for_timestamp, bar_to_timestamp,
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

// Layout (platform-agnostic layout computation)
#[allow(deprecated)]
pub use layout::{
    // Rectangle types
    LayoutRect, ChartAreaLayout, FrameLayout, ChartHitZone,
    // Margins (space consumed by external UI)
    Margins,
    // Extended layout with sub-panes
    ExtendedFrameLayout, SubPaneLayout,
    // Sub-pane height helpers
    default_sub_pane_heights, sub_pane_heights_from_panes,
    // Configuration and manager
    LayoutConfig, LayoutManager,
    // Hit testing
    LayoutHitTester, ExtendedLayoutHitTester,
    // Rendering
    ChartRenderConfig, render_chart_area, draw_scale_corner, render_scales,
    render_chart_window, render_chart_splits,
    // Full panel rendering (chart-internal, replaces terminal-assembled path)
    ChartPanelRenderData, ChartPanelRenderResult, render_full_chart_panel,
    // Sub-pane overlay hit-test results (produced each frame, cached on ChartWindow)
    SubPaneOverlayResult,
    draw_content_borders, draw_frame_borders,
    // Scale corner with buttons
    ScaleCornerState, ScaleCornerHitZones, ScaleCornerButton,
    draw_scale_corner_with_buttons, render_chart_area_with_buttons,
    // Frame rendering
    FrameTheme, render_frame,
    // Render pass control (for multi-pass rendering with blur effects)
    RenderPass,
    // Toolbar state (moved from core to avoid circular dep on Icon)
    // Note: RenderThemes and build_render_themes stay in core (depend on core's types)
    ToolbarState, ToolbarClickResult, ToggleIconPair,
    // Frame render result types (moved from core to break circular dependency)
    ContextMenuResult,
    ColorPickerRenderResult,
    InlineConfigResult,
    SliderTrackInfo,
    PrimitiveSettingsResult,
    PanelTreeManagerResult,
    ModalSearchResult,
    RightSidebarResult,
    ChartSettingsModalResult,
    IndicatorSettingsModalResult,
    IndicatorRowResult,
    IndicatorOverlayResult,
    SingleChartPanelResult,
    MultiChartRenderResult,
    // Chart modal rendering (delegated from core to chart panel)
    ChartModalLayout,
    ChartModalRenderResult,
};

// Chart settings modal data types (moved from core to chart crate)
pub use layout::modals::chart_settings::{
    ChartSettingsData, InstrumentSettings, StatusLineSettings, ScalesLinesSettings,
};

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

// Annotations (from chart module)
pub use chart::annotations::{
    LineStyle, Marker, MarkerCoordinates, MarkerManager, MarkerPosition, MarkerShape, PriceLine,
    PriceLineOptions,
};

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
    // Base chart struct
    Chart,
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
    SubPane,
    // Coordinate utilities (clamping, boundary policy)
    coordinate_utils,
    // Command pattern (undo/redo)
    Command, CommandResult, StateChange, ViewportState,
    PropertyValue, Position, CommandHistory,
    // Timeframe visibility for object tree
    TimeframeVisibility,
    // ChartWindow - the main chart state aggregate
    ChartWindow, ChartId, ConnectionStatus, WindowRect, WINDOW_GAP,
    // Snap-to-end constant (canonical margin used by ChartWindow::snap_to_end)
    DEFAULT_SNAP_MARGIN,
    // Chart-internal split/expand system
    ChartPanelGrid, ChartSubPanel, SplitHitResult, ChartInputTarget,
    // ChartId for the chart crate's ChartWindow
    generate_chart_id, bump_chart_id_past,
    // Unified chart action executor (operates on ChartWindow, returns external events)
    execute_chart_action, ChartExternalEvent, OpenModalRequest,
};

// Re-export uzor-panels types so callers don't need a direct dependency.
pub use uzor::panels::SplitKind;
pub use uzor::panels::SeparatorOrientation;
pub use uzor::panels::PanelRect;
pub use uzor::panels::LeafId;

// Overlay tab headers for split panel leaves.
pub use layout::panel_overlay::{render_leaf_tab, LeafTabHoverZone, LeafTabHitZones};

// Drawing System (v2 - trait-based primitives)
pub use drawing::{
    // Manager
    DrawingManager, DrawingState, DragType,
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
    // Signal and trade managers
    SignalManager, Trade, TradeDirection, TradeManager,
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

// Panel layout and toolbar rendering
pub use panel_app::{
    ChartPanelLayout, ChartToolbarRenderResult,
    ChartInternalLayout, compute_chart_internal_layout,
    ToolbarConfig,
    InlineDockEdge,
};
pub use ui::toolbar_render::{ToolbarRect, ToolbarTheme, ToolbarRenderResult};
pub use ui::dropdown::DropdownTheme;
pub use ui::modal_settings::ManagedKeyInfo;
/// New preferred name for the key display info type.
pub use ui::modal_settings::ManagedKeyInfo as LocalAgentKeyInfo;

// Demo Data (for testing - can be replaced with real API)
pub use demo::{
    // Symbols
    DemoSymbol, demo_symbols, get_demo_symbol, demo_symbol_tickers,
    // Data Generation
    DemoTimeframe, DemoDataGenerator, generate_demo_bars, generate_all_timeframes,
    // Demo Indicator Calculations (stubs - real indicators from nemo library)
    calculate_sma, calculate_ema, calculate_rsi, calculate_bollinger,
    calculate_volume, calculate_macd, calculate_atr, calculate_stochastic,
};

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
    TextKey, TooltipKey, MenuKey, ConfigKey, WaveDegreeKey, StyleKey, LabelPositionKey,
    MonthKey, month_names_short,
    Translatable,
};

// User profile (aggregate persistent state — active selections, UI state)
pub use user_profile::{
    UserProfile,
    VaultSecrets,
    WindowState,
    StoredLocalAgentKey,
    StoredApiKey,  // backward-compat alias for StoredLocalAgentKey
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
    set_profile_cloud_enabled,
    set_profile_sync_level,
    delete_profile,
    PersistedAgentLeaf,
    PersistedAgentCli,
    PersistedInstanceMode,
};

// Unified user state manager
pub use user_manager::UserManager;
pub use user_manager::{ProfileInfo, ProfileManager, SwitchData, MIN_PASSPHRASE_LENGTH};

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
