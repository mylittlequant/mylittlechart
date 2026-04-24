//! Chart State Module
//!
//! Core state types for the standalone chart library.
//!
//! This module provides:
//! - `Chart` - base chart struct with data, viewport, scales, and display options
//! - `SubPane` - sub-pane layout for indicator panels
//! - `Timeframe` - timeframe representation
//! - `VisibilityManager` - visibility tracking
//! - `LockManager` - lock state management
//! - `Command` / `CommandHistory` - undo/redo command pattern
//!
//! Terminal-specific features (indicators, alerts, signals, trades,
//! multi-window management) are NOT included here - they belong in the terminal crate.

mod chart;
mod pane;
mod timeframe;
mod visibility;
mod lock;
pub mod selected_config;
pub mod command;
pub mod history;
pub mod app_state;
pub mod action_executor;
pub mod chart_window;
pub mod sub_panel;
pub mod panel_grid;

// Core chart struct
pub use chart::*;

// Layout types
pub use pane::{
    SubPane,
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

// Chart-domain action executor, result type, and external events
pub use action_executor::{
    ActionResult,
    execute_chart_action_internal,
    execute_chart_action,
    ChartExternalEvent,
    OpenModalRequest,
};

// ChartWindow - the main chart state aggregate
pub use chart_window::{ChartWindow, ChartId, ConnectionStatus, WindowRect, WINDOW_GAP, generate_chart_id, bump_chart_id_past, DEFAULT_SNAP_MARGIN};

// Chart-internal split/expand system (uzor-panels integration)
pub use sub_panel::ChartSubPanel;

// Panel grid — input-aware layout manager
pub use panel_grid::{ChartPanelGrid, ChartInputTarget, ChartRightClickHit, ChartDragStartHit, SplitHitResult, FreehandCompleteResult};
