//! Chart State Module
//!
//! Core state types for the standalone chart library.
//!
//! This module provides:
//! - `ChartContainer` - the main chart state aggregate (multi-window)
//! - `SubPane` - sub-pane layout for indicator panels
//! - `Timeframe` - timeframe representation
//! - `VisibilityManager` / `LockManager`
//! - `Command` / `CommandHistory` - undo/redo command pattern
//!
//! The legacy single-window `Chart` struct was deleted — all consumers
//! flow through `ChartContainer`.

mod pane;
mod timeframe;
mod visibility;
mod lock;
pub mod geometry;
pub mod scale_corner;
pub mod selected_config;
pub mod command;
pub mod history;
pub mod app_state;
pub mod chart_container;
pub mod panel_grid;
pub mod chart_container_leaf;
pub mod inner_layout;
pub mod branch;
pub mod chart_panel_state;
pub mod panel;
pub mod sub_pane_overlay;
pub mod toolbar_state;
pub mod chart_settings_data;
pub mod icons;
pub mod scroll_state;
pub mod modal_state;
pub mod modal_data;
pub mod toolbar_geometry;

// Unified panel taxonomy (K1 — supersedes PanelLeaf + FreeItem)
pub use panel::{Panel, PanelId, PanelKind};

// Layout types
pub use pane::{
    SubPane,
    SubPaneId,
    generate_sub_pane_id,
    bump_sub_pane_id_past,
    Pane,
    PaneId,
    PaneManager,
    PaneGeometry,
    InteractionRegion,
    MAIN_PANE,
    coordinate_utils,
};

// Timeframe
pub use timeframe::{Timeframe, TimeframeManager};

// Visibility and lock management
pub use visibility::VisibilityManager;
pub use lock::LockManager;

// Selected primitive configuration
pub use selected_config::SelectedPrimitiveConfig;

// Command pattern (undo/redo) - exported from chart for use by core and terminal
pub use command::{
    Command, CommandResult, StateChange, ViewportState,
    PropertyValue, Position, ObjectCategory, ObjectInfo, TimeframeVisibility,
};
pub use history::CommandHistory;

// App state interface, window layout, window sync mode
pub use app_state::{AppState, is_action_active, WindowLayout, WindowSyncMode};

// `action_executor` (ActionResult, execute_chart_action[_internal],
// ChartExternalEvent, OpenModalRequest) was zengeld-terminal-core legacy API
// with zero callers in this workspace — deleted alongside the Step 3.1
// DrawingManager physical move (it took `&mut DrawingManager` by value,
// which no longer resolves from mlc-core).

// Phase 1 hardcut of the chart-container cut: the former `ChartWindow` is
// renamed in place to `ChartContainer`. It currently still owns ALL fields
// (canvas + container-level + sub-panes); Phase 2 carves a thin `ChartPanel`
// out of it for the canvas-only fields.
pub use chart_container::{
    ChartContainer,
    ChartId,
    ConnectionStatus,
    WindowRect,
    WINDOW_GAP,
    generate_chart_id,
    bump_chart_id_past,
    DEFAULT_SNAP_MARGIN,
    SubPaneYState,
};


// Inner-docking leaf payload for ChartContainer (Phase 1 of chart-container cut).
// `ChartContainerId` is an alias for `ChartId` (plan D1); the canvas gets its
// own `ChartPanelId`.
pub use chart_container_leaf::{
    ChartContainerId,
    ChartContainerLeaf,
    ChartPanelId,
    generate_chart_panel_id,
    bump_chart_panel_id_past,
};

// Inner 1D vertical stack layout (own math, no uzor docking).
pub use inner_layout::InnerLayout;

// Branch-level docking payload types.
pub use branch::InstanceId;

// Panel grid — input-aware layout manager
pub use panel_grid::{ChartPanelGrid, ChartInputTarget, ChartRightClickHit, ChartDragStartHit, SplitHitResult, FreehandCompleteResult, DrawingSession};

// Phase 4 S2.7 — per-canvas display-option + series-visibility state
pub use chart_panel_state::ChartPanelState;

// Pure geometry (no external deps — shared with layout/)
pub use geometry::{
    LayoutRect,
    ChartAreaLayout,
    ChartHitZone,
    FrameLayout,
    SubPaneLayout,
    ExtendedFrameLayout,
    Margins,
    LayoutConfig,
    LayoutManager,
};

// Scale-corner button state + hit zones (pure data)
pub use scale_corner::{ScaleCornerState, ScaleCornerHitZones, ScaleCornerButton};

// Sub-pane overlay hit-test rect cache
pub use sub_pane_overlay::SubPaneOverlayResult;

// Toolbar interaction state (pure data — no render logic)
// Also includes chart-toolbar types (moved from panel_app — Phase X.4 back-edge removal).
pub use toolbar_state::{
    ToolbarState, ToolbarClickResult, ToggleIconPair,
    ToolbarConfig,
    ChartToolbarState,
    ChartToolbarRenderResult,
    FloatingInlineBar,
    InlineDockEdge,
    InlineSlideContainer,
    DropdownRenderInfo,
    InlineDropdownResult,
    InlineConfigResult as ChartInlineConfigResult,
};

// Chart-settings data bags (pure serde — consumed by templates + app)
pub use chart_settings_data::{ChartSettingsData, InstrumentSettings, ScalesLinesSettings, StatusLineSettings};

// UI data types relocated from mlc-layout/src/ui/ — pure data consumed by backend
pub use icons::Icon;
pub use scroll_state::ScrollState;
pub use modal_state::{ModalState, OpenModal, SearchResult, IndicatorCategory, IndicatorCategoryFilter, IndicatorCatalogItem, ClockPopupState};
pub use modal_data::{SubPaneButton, SubPaneOverlayState, IndicatorDisplayInfo, IndicatorParamDef, IndicatorParamType, IndicatorOutputDef, IndicatorOutputType};
pub use toolbar_geometry::{WidgetRect, ToolbarRect, ToolbarRenderResult, PRESET_COLORS};
