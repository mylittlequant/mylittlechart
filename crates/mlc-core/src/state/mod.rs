//! Chart State Module
//!
//! Core state types for the standalone chart library.
//!
//! This module provides:
//! - `Timeframe` - timeframe representation
//! - `Command` / `CommandHistory` - undo/redo command pattern
//!
//! `ChartContainer` (the main chart state aggregate), `ChartPanelGrid`,
//! `ChartContainerLeaf`, and `ChartPanelState` moved to `mlc-app` in Step 4
//! (state lift, see `docs/mlc/plans/doctrine-cutover-2026-06-27.md`) — they
//! are UI-window state, not type-dictionary values.  `mlc-backend` (which
//! reads `ChartId`/`ConnectionStatus` directly) and `mlc-core::preset`
//! (which builds `ChartWindowSnapshot`) were decoupled from `ChartContainer`
//! first so the move doesn't create an upward dependency.
//!
//! The legacy single-window `Chart` struct, `Pane`/`PaneManager`/`SubPane`
//! (pre-`ChartContainerLeaf`/`inner_docking` API), `VisibilityManager`/
//! `LockManager`/`AppState` (pre-`ChartContainer` object-ID API), and
//! `branch::InstanceId` (superseded by plain `u64` instance ids on
//! `ChartContainerLeaf::SubPane`) were dead code with zero consumers
//! outside this crate's own re-export chain — deleted during Step 4.

mod timeframe;
pub mod geometry;
pub mod scale_corner;
pub mod selected_config;
pub mod command;
pub mod history;
pub mod inner_layout;
pub mod panel;
pub mod drawing_session;
pub mod sub_pane_overlay;
pub mod sub_pane_y_state;
pub mod toolbar_state;
pub mod chart_settings_data;
pub mod icons;
pub mod scroll_state;
pub mod modal_state;
pub mod modal_data;
pub mod toolbar_geometry;
pub mod gestures;
pub mod ewa_engine;

// Unified panel taxonomy (K1 — supersedes PanelLeaf + FreeItem)
pub use panel::{Panel, PanelId, PanelKind};

// Timeframe
pub use timeframe::Timeframe;

// Selected primitive configuration
pub use selected_config::SelectedPrimitiveConfig;

// Command pattern (undo/redo) - exported from chart for use by core and terminal
pub use command::{Command, ObjectCategory};
pub use history::CommandHistory;

// `action_executor` (ActionResult, execute_chart_action[_internal],
// ChartExternalEvent, OpenModalRequest) was zengeld-terminal-core legacy API
// with zero callers in this workspace — deleted alongside the Step 3.1
// DrawingManager physical move (it took `&mut DrawingManager` by value,
// which no longer resolves from mlc-core).

// ID types (ChartId, ConnectionStatus, ChartPanelId) live in `mlc-core::ids` —
// referenced directly by `mlc-backend` (Tag routing / DataBubble field) and
// by `mlc-app-layout`/`mlc-app-panels` (TradingPanelsStore keying), neither
// of which can depend on `mlc-app` (where `ChartContainer` now lives).
pub use crate::ids::{
    ChartId, ConnectionStatus, ChartPanelId,
    generate_chart_id, bump_chart_id_past,
    generate_chart_panel_id, bump_chart_panel_id_past,
};

// Per-instance Y-axis state for indicator sub-panes — pure value type,
// referenced by `mlc-app-layout` render code (see sub_pane_y_state.rs doc).
pub use sub_pane_y_state::SubPaneYState;

// Inner 1D vertical stack layout (own math, no uzor docking).
pub use inner_layout::InnerLayout;

// DrawingSession — shared interface trait, see drawing_session.rs doc.
pub use drawing_session::DrawingSession;

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

// Elliott wave chart-gesture bindings (which physical gesture opens which
// wave-editing menu) — pure data, see gestures.rs's own module doc.
pub use gestures::{ChartGesture, GestureBindings, GestureTarget, GestureTrigger};

// EWA (Elliott Wave Analysis) engine configuration — process-global,
// user-level settings surface for the engine itself (owner ask 2026-08-14).
// See ewa_engine.rs's own module doc.
pub use ewa_engine::{EwaEngineConfig, GuardFamilies, RatioRibDefaults, ewa_engine_config, set_ewa_engine_config};

// Toolbar interaction state (pure data — no render logic)
// Also includes chart-toolbar types (moved from panel_app — Phase X.4 back-edge removal).
//
// STAYS IN mlc-core (Step 4 audit, deliberately NOT moved to mlc-app):
// ChartToolbarState/ToolbarConfig/ChartToolbarRenderResult are read
// read-only (`tb: &ChartToolbarState`) by 5 render function signatures in
// `mlc-app-layout::panel_app` (below mlc-app in the dep graph — cannot
// depend on it). Zero backend consumers. Converting those 5 signatures to
// plain-data/closure params (the option-(a) pattern used for ChartPanelGrid
// in Step 4 cluster 4) would require decomposing a large toolbar-render
// function's internals — out of this step's mechanical-sweep scope.
// Honest stay per doctrine's option (b): shared UI-model type.
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

// UI data types relocated from mlc-app-layout/src/ui/ — pure data consumed by backend
pub use icons::Icon;
pub use scroll_state::ScrollState;

// ModalState/OpenModal/SearchResult/IndicatorCategory(modal)/
// IndicatorCategoryFilter/ClockPopupState: STAYS IN mlc-core, same
// rationale as ChartToolbarState above — read-only by
// `mlc-app-layout::layout::modals::search_overlay::render_search_overlay`
// (`modal_state: &ModalState`), zero backend consumers, and the render fn
// reads many fields across ~300 lines (not a narrow 2-accessor case like
// ChartPanelGrid's `.docking()`/`.is_expanded()`).
pub use modal_state::{ModalState, OpenModal, SearchResult, IndicatorCategory, IndicatorCategoryFilter, IndicatorCatalogItem, ClockPopupState, SymbolQuickSearchRender};

// SubPaneButton/SubPaneOverlayState/IndicatorDisplayInfo/IndicatorParamDef/
// IndicatorParamType/IndicatorOutputDef/IndicatorOutputType: STAYS IN
// mlc-core — genuine backend consumer.
// `mlc-backend::indicator::manager::IndicatorManager::get_settings_data`
// imports `mlc_core::state::modal_data::{IndicatorDisplayInfo, ...}`
// directly to build the indicator-settings-modal payload. This is the
// doctrine §1A "CommandHistory-style" case: a type physically living in
// mlc-core because a backend manager needs it, not because it's UI state.
pub use modal_data::{SubPaneButton, SubPaneOverlayState, IndicatorDisplayInfo, IndicatorParamDef, IndicatorParamType, IndicatorOutputDef, IndicatorOutputType};

// PRESET_COLORS: STAYS IN mlc-core — genuine backend consumer
// (`mlc-backend::tag::manager` uses it for auto-assigned tag colors).
pub use toolbar_geometry::{WidgetRect, ToolbarRect, ToolbarRenderResult, PRESET_COLORS};
