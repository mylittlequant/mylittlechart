//! Command Pattern for Undo/Redo
//!
//! All state-changing operations are represented as commands that can be
//! executed, undone, and redone.
//!
//! # Integration with DrawingManager
//!
//! Commands work with primitive indices (as used by DrawingManager):
//! - `SetPrimitiveVisibility` - uses index
//! - `SetPrimitiveLock` - uses index
//! - `CreatePrimitive` - stores type_id, points, data for recreation
//! - `DeletePrimitive` - stores full primitive snapshot for undo
//! - `MovePrimitive` - stores before/after points
//! - `ViewportChange` - stores before/after viewport state
//!
//! The pre-`ChartContainer` object-ID-based command family (`ObjectInfo`,
//! `SetVisibility`, `SetLock`, `SetGlobalLock`, `SetCategoryVisibility`,
//! `DeleteObject`, `DeleteCategory`, `DeleteAll`, `CreateObject`,
//! `ModifyObject`, `MoveObject`, the legacy `SetTimeframe`, `PropertyValue`,
//! `Position`, `TimeframeVisibility`) was deleted 2026-07-02 (tail wave of
//! `docs/mlc/plans/doctrine-cutover-2026-06-27.md`) — audit confirmed zero
//! constructors anywhere in the workspace outside test placeholders.

use serde::{Serialize, Deserialize};
use crate::drawing::PrimitiveData;
use crate::chart::CompareSeries;
use crate::state::Timeframe;
use crate::alert::AlertItem;

// =============================================================================
// ObjectCategory - full terminal version (with all categories)
// =============================================================================

/// Categories for organizing chart objects (terminal variant with all categories)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObjectCategory {
    /// Drawing primitives (lines, rectangles, etc.)
    Drawing,
    /// Technical indicators
    Indicator,
    /// Trading signals and annotations
    Signal,
    /// Price alerts
    Alert,
    /// Positions and orders
    Position,
    /// Text annotations
    Text,
    /// Measurement tools
    Measurement,
    /// Compare overlay symbols
    Compare,
    /// Custom/user-defined
    Custom,
}

impl ObjectCategory {
    /// Get display name for the category
    pub fn display_name(&self) -> &'static str {
        match self {
            ObjectCategory::Drawing => "Drawings",
            ObjectCategory::Indicator => "Indicators",
            ObjectCategory::Signal => "Signals",
            ObjectCategory::Alert => "Alerts",
            ObjectCategory::Position => "Positions",
            ObjectCategory::Text => "Text",
            ObjectCategory::Measurement => "Measurements",
            ObjectCategory::Compare => "Compare",
            ObjectCategory::Custom => "Custom",
        }
    }

    /// Get all categories
    pub fn all() -> &'static [ObjectCategory] {
        &[
            ObjectCategory::Drawing,
            ObjectCategory::Indicator,
            ObjectCategory::Signal,
            ObjectCategory::Alert,
            ObjectCategory::Position,
            ObjectCategory::Text,
            ObjectCategory::Measurement,
            ObjectCategory::Compare,
            ObjectCategory::Custom,
        ]
    }
}

// =============================================================================
// Command enum
// =============================================================================

/// A command that can be executed and undone
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    // =========================================================================
    // New index-based commands (integrated with DrawingManager)
    // =========================================================================

    /// Set primitive visibility by stable id (delegates to `DrawingsMut::set_visibility`)
    SetPrimitiveVisibility {
        prim_id: u64,
        visible: bool,
        previous: bool,
    },

    /// Set primitive lock by stable id (delegates to `DrawingsMut::set_lock`)
    SetPrimitiveLock {
        prim_id: u64,
        locked: bool,
        previous: bool,
    },

    /// Delete primitive by stable id (delegates to `DrawingsMut::delete`)
    DeletePrimitive {
        prim_id: u64,
        /// Type ID for recreation via registry
        type_id: String,
        /// Coordinate points
        points: Vec<(i64, f64)>,
        /// Full primitive data (id, color, style, etc.)
        data: PrimitiveData,
    },

    /// Delete all primitives
    DeleteAllPrimitives {
        /// All primitives before deletion (for undo) - stores full data for recreation
        primitives: Vec<(String, Vec<(i64, f64)>, PrimitiveData)>, // (type_id, points, data)
    },

    /// Restore all primitives (inverse of DeleteAllPrimitives - used for undo)
    RestoreAllPrimitives {
        /// All primitives to restore - stores full data for recreation
        primitives: Vec<(String, Vec<(i64, f64)>, PrimitiveData)>, // (type_id, points, data)
    },

    /// Set strategy visibility.
    SetStrategyVisibility {
        strategy_tag: String,
        visible: bool,
        previous: bool,
    },

    /// Create primitive (for undo of delete, or recording creation)
    CreatePrimitive {
        /// Stable id of the primitive being created/restored.
        prim_id: u64,
        /// Type ID for recreation via registry
        type_id: String,
        /// Coordinate points
        points: Vec<(i64, f64)>,
        /// Full primitive data (id, color, style, etc.)
        data: PrimitiveData,
    },

    /// Move/reshape primitive (stores points before and after)
    MovePrimitive {
        prim_id: u64,
        /// Points before the move
        previous_points: Vec<(i64, f64)>,
        /// Points after the move
        new_points: Vec<(i64, f64)>,
    },

    /// Reorder primitive (bring to front / send to back)
    ReorderPrimitive {
        /// Primitive being reordered.
        prim_id: u64,
        /// Primitive it should now sit immediately before (`None` = back of z-order).
        before_prim_id: Option<u64>,
    },

    /// Viewport state change (pan, zoom)
    ViewportChange {
        /// Previous viewport state
        previous: ViewportState,
        /// New viewport state
        new: ViewportState,
    },

    /// Modify primitive data (color, width, style, text, etc.)
    ModifyPrimitiveData {
        prim_id: u64,
        /// Data before modification
        previous_data: PrimitiveData,
        /// Data after modification
        new_data: PrimitiveData,
    },

    /// Modify full primitive (including type-specific data like Fib levels)
    /// Uses JSON serialization to capture complete state
    ModifyPrimitiveFull {
        prim_id: u64,
        /// Type ID of the primitive
        type_id: String,
        /// Full JSON before modification
        previous_json: String,
        /// Full JSON after modification
        new_json: String,
    },

    // =========================================================================
    // Compare Overlay commands
    // =========================================================================

    /// Add a compare series to the overlay
    AddCompareSeries {
        /// The series that was added (for undo)
        series: CompareSeries,
    },

    /// Remove a compare series from the overlay
    RemoveCompareSeries {
        /// Symbol of the series to remove
        symbol: String,
        /// The full series data (for undo - to restore it)
        series: CompareSeries,
    },

    /// Set compare series visibility
    SetCompareSeriesVisibility {
        symbol: String,
        visible: bool,
        previous: bool,
    },

    /// Set compare series color
    SetCompareSeriesColor {
        symbol: String,
        color: String,
        previous_color: String,
    },

    /// Clear all compare series
    ClearAllCompareSeries {
        /// All series before clearing (for undo)
        series: Vec<CompareSeries>,
    },

    /// Restore all compare series (inverse of ClearAllCompareSeries)
    RestoreAllCompareSeries {
        /// All series to restore
        series: Vec<CompareSeries>,
    },

    // =========================================================================
    // Indicator commands
    // =========================================================================

    /// Add an indicator
    AddIndicator {
        /// Instance ID assigned to the indicator
        instance_id: u64,
        /// Type ID of the indicator (e.g., "macd", "rsi")
        type_id: String,
        /// Parameters as JSON string (for recreation)
        params_json: String,
    },

    /// Remove an indicator
    RemoveIndicator {
        /// Instance ID of the removed indicator
        instance_id: u64,
        /// Type ID of the indicator (for undo recreation)
        type_id: String,
        /// Parameters as JSON string (for undo recreation)
        params_json: String,
    },

    // =========================================================================
    // Alert commands
    // =========================================================================

    /// Add an alert (recorded at creation time; inverse = DeleteAlert)
    AddAlert {
        /// Full alert state captured immediately after creation
        snapshot: AlertItem,
    },

    /// Delete an alert (recorded at deletion time; inverse = AddAlert)
    DeleteAlert {
        /// Full alert state captured immediately before deletion
        snapshot: AlertItem,
    },

    // =========================================================================
    // Symbol/Timeframe/ChartType commands
    // =========================================================================

    /// Change symbol
    ChangeSymbol {
        /// Previous symbol
        previous_symbol: String,
        /// New symbol
        new_symbol: String,
    },

    /// Change timeframe
    ChangeTimeframe {
        /// Previous timeframe
        previous_timeframe: Timeframe,
        /// New timeframe
        new_timeframe: Timeframe,
    },

    /// Change chart type (candlestick, line, bar, etc.)
    ChangeChartType {
        /// Previous chart type
        previous_type: String,
        /// New chart type
        new_type: String,
    },

}

/// Viewport state snapshot for undo/redo
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ViewportState {
    /// Starting bar index
    pub view_start: f64,
    /// Pixels per bar (zoom level)
    pub bar_spacing: f64,
    /// Price scale minimum
    pub price_min: f64,
    /// Price scale maximum
    pub price_max: f64,
}

impl ViewportState {
    pub fn new(view_start: f64, bar_spacing: f64, price_min: f64, price_max: f64) -> Self {
        Self { view_start, bar_spacing, price_min, price_max }
    }
}

impl Command {
    /// Get the inverse command for undo
    pub fn inverse(&self) -> Command {
        match self {
            // New id-based commands
            Command::SetPrimitiveVisibility { prim_id, visible, previous } => {
                Command::SetPrimitiveVisibility {
                    prim_id: *prim_id,
                    visible: *previous,
                    previous: *visible,
                }
            }

            Command::SetPrimitiveLock { prim_id, locked, previous } => {
                Command::SetPrimitiveLock {
                    prim_id: *prim_id,
                    locked: *previous,
                    previous: *locked,
                }
            }

            Command::DeletePrimitive { prim_id, type_id, points, data } => {
                // Inverse of delete is create - recreate the primitive
                Command::CreatePrimitive {
                    prim_id: *prim_id,
                    type_id: type_id.clone(),
                    points: points.clone(),
                    data: data.clone(),
                }
            }

            Command::DeleteAllPrimitives { primitives } => {
                // Inverse of delete all is restore all
                Command::RestoreAllPrimitives {
                    primitives: primitives.clone(),
                }
            }

            Command::RestoreAllPrimitives { primitives } => {
                // Inverse of restore all is delete all
                Command::DeleteAllPrimitives {
                    primitives: primitives.clone(),
                }
            }

            Command::SetStrategyVisibility { strategy_tag, visible, previous } => {
                Command::SetStrategyVisibility {
                    strategy_tag: strategy_tag.clone(),
                    visible: *previous,
                    previous: *visible,
                }
            }

            Command::CreatePrimitive { prim_id, type_id, points, data } => {
                // Inverse of create is delete
                Command::DeletePrimitive {
                    prim_id: *prim_id,
                    type_id: type_id.clone(),
                    points: points.clone(),
                    data: data.clone(),
                }
            }

            Command::MovePrimitive { prim_id, previous_points, new_points } => {
                Command::MovePrimitive {
                    prim_id: *prim_id,
                    previous_points: new_points.clone(),
                    new_points: previous_points.clone(),
                }
            }

            Command::ReorderPrimitive { prim_id, before_prim_id } => {
                // Reorder has no clean self-inverse under relative (before_prim_id)
                // addressing — the caller (mlc-app) captures the pre-move
                // neighbor itself and pushes a fresh command for undo rather
                // than relying on `inverse()` for this variant.
                Command::ReorderPrimitive {
                    prim_id: *prim_id,
                    before_prim_id: *before_prim_id,
                }
            }

            Command::ViewportChange { previous, new } => {
                Command::ViewportChange {
                    previous: *new,
                    new: *previous,
                }
            }

            Command::ModifyPrimitiveData { prim_id, previous_data, new_data } => {
                Command::ModifyPrimitiveData {
                    prim_id: *prim_id,
                    previous_data: new_data.clone(),
                    new_data: previous_data.clone(),
                }
            }

            Command::ModifyPrimitiveFull { prim_id, type_id, previous_json, new_json } => {
                Command::ModifyPrimitiveFull {
                    prim_id: *prim_id,
                    type_id: type_id.clone(),
                    previous_json: new_json.clone(),
                    new_json: previous_json.clone(),
                }
            }

            // Compare commands
            Command::AddCompareSeries { series } => {
                Command::RemoveCompareSeries {
                    symbol: series.symbol.clone(),
                    series: series.without_bars(),
                }
            }

            Command::RemoveCompareSeries { symbol: _, series } => {
                Command::AddCompareSeries {
                    series: series.without_bars(),
                }
            }

            Command::SetCompareSeriesVisibility { symbol, visible, previous } => {
                Command::SetCompareSeriesVisibility {
                    symbol: symbol.clone(),
                    visible: *previous,
                    previous: *visible,
                }
            }

            Command::SetCompareSeriesColor { symbol, color, previous_color } => {
                Command::SetCompareSeriesColor {
                    symbol: symbol.clone(),
                    color: previous_color.clone(),
                    previous_color: color.clone(),
                }
            }

            Command::ClearAllCompareSeries { series } => {
                // Inverse of clear-all is to restore all cleared series
                Command::RestoreAllCompareSeries {
                    series: series.iter().map(|s| s.without_bars()).collect(),
                }
            }

            Command::RestoreAllCompareSeries { series } => {
                // Inverse of restore-all is to clear-all (storing them for further undo)
                Command::ClearAllCompareSeries {
                    series: series.iter().map(|s| s.without_bars()).collect(),
                }
            }

            // Indicator commands
            Command::AddIndicator { instance_id, type_id, params_json } => {
                Command::RemoveIndicator {
                    instance_id: *instance_id,
                    type_id: type_id.clone(),
                    params_json: params_json.clone(),
                }
            }

            Command::RemoveIndicator { instance_id, type_id, params_json } => {
                Command::AddIndicator {
                    instance_id: *instance_id,
                    type_id: type_id.clone(),
                    params_json: params_json.clone(),
                }
            }

            // Alert commands
            Command::AddAlert { snapshot } => {
                Command::DeleteAlert { snapshot: snapshot.clone() }
            }

            Command::DeleteAlert { snapshot } => {
                Command::AddAlert { snapshot: snapshot.clone() }
            }

            // Symbol/Timeframe/ChartType commands
            Command::ChangeSymbol { previous_symbol, new_symbol } => {
                Command::ChangeSymbol {
                    previous_symbol: new_symbol.clone(),
                    new_symbol: previous_symbol.clone(),
                }
            }

            Command::ChangeTimeframe { previous_timeframe, new_timeframe } => {
                Command::ChangeTimeframe {
                    previous_timeframe: new_timeframe.clone(),
                    new_timeframe: previous_timeframe.clone(),
                }
            }

            Command::ChangeChartType { previous_type, new_type } => {
                Command::ChangeChartType {
                    previous_type: new_type.clone(),
                    new_type: previous_type.clone(),
                }
            }
        }
    }

    /// Get a human-readable description of the command
    pub fn description(&self) -> &'static str {
        match self {
            // New commands
            Command::SetPrimitiveVisibility { visible, .. } => {
                if *visible { "Show Primitive" } else { "Hide Primitive" }
            }
            Command::SetPrimitiveLock { locked, .. } => {
                if *locked { "Lock Primitive" } else { "Unlock Primitive" }
            }
            Command::DeletePrimitive { .. } => "Delete Primitive",
            Command::DeleteAllPrimitives { .. } => "Delete All Primitives",
            Command::RestoreAllPrimitives { .. } => "Restore All Primitives",
            Command::SetStrategyVisibility { visible, .. } => {
                if *visible { "Show Strategy" } else { "Hide Strategy" }
            }
            Command::CreatePrimitive { .. } => "Create Primitive",
            Command::MovePrimitive { .. } => "Move Primitive",
            Command::ReorderPrimitive { .. } => "Reorder Primitive",
            Command::ViewportChange { .. } => "Viewport Change",
            Command::ModifyPrimitiveData { .. } => "Modify Primitive",
            Command::ModifyPrimitiveFull { .. } => "Modify Primitive Settings",

            // Compare commands
            Command::AddCompareSeries { .. } => "Add Compare Symbol",
            Command::RemoveCompareSeries { .. } => "Remove Compare Symbol",
            Command::SetCompareSeriesVisibility { visible, .. } => {
                if *visible { "Show Compare" } else { "Hide Compare" }
            }
            Command::SetCompareSeriesColor { .. } => "Change Compare Color",
            Command::ClearAllCompareSeries { .. } => "Clear All Compare",
            Command::RestoreAllCompareSeries { .. } => "Restore All Compare",

            // Indicator commands
            Command::AddIndicator { .. } => "Add Indicator",
            Command::RemoveIndicator { .. } => "Remove Indicator",

            // Alert commands
            Command::AddAlert { .. } => "Add Alert",
            Command::DeleteAlert { .. } => "Delete Alert",

            // Symbol/Timeframe/ChartType commands
            Command::ChangeSymbol { .. } => "Change Symbol",
            Command::ChangeTimeframe { .. } => "Change Timeframe",
            Command::ChangeChartType { .. } => "Change Chart Type",
        }
    }

    /// Whether this command is a viewport change (pan/zoom).
    pub fn is_viewport_change(&self) -> bool {
        matches!(self, Command::ViewportChange { .. })
    }
}

/// Result of executing a command
#[derive(Debug)]
pub struct CommandResult {
    /// Whether the command was successful
    pub success: bool,
    /// What changed as a result
    pub change: StateChange,
    /// Error message if failed
    pub error: Option<String>,
}

impl CommandResult {
    pub fn success(change: StateChange) -> Self {
        Self {
            success: true,
            change,
            error: None,
        }
    }

    pub fn failure(error: &str) -> Self {
        Self {
            success: false,
            change: StateChange::None,
            error: Some(error.to_string()),
        }
    }
}

/// Describes what changed after a command execution
#[derive(Debug, Clone)]
pub enum StateChange {
    None,
    /// Legacy: visibility changed by object ID
    Visibility { object_id: u64 },
    /// New: primitive visibility changed by index
    PrimitiveVisibility { index: usize },
    /// Legacy: category visibility changed
    CategoryVisibility { category: ObjectCategory },
    /// Legacy: lock changed by object ID
    Lock { object_id: u64 },
    /// New: primitive lock changed by index
    PrimitiveLock { index: usize },
    /// Global lock state changed
    GlobalLock,
    /// Strategy visibility changed
    StrategyVisibility { strategy_tag: String },
    /// Timeframe changed
    Timeframe,
    /// Legacy: object deleted by ID
    ObjectDeleted { object_id: u64 },
    /// New: primitive deleted by index
    PrimitiveDeleted { index: usize },
    /// All primitives deleted
    AllPrimitivesDeleted,
    /// All primitives restored
    AllPrimitivesRestored,
    /// Category deleted
    CategoryDeleted { category: ObjectCategory },
    /// All objects deleted (legacy)
    AllDeleted,
    /// Object created
    ObjectCreated { object_id: u64 },
    /// Object modified
    ObjectModified { object_id: u64 },
    /// Object moved
    ObjectMoved { object_id: u64 },
    /// Primitive created
    PrimitiveCreated { index: usize },
    /// Primitive moved/reshaped
    PrimitiveMoved { index: usize },
    /// Primitive data modified (color, width, style, text)
    PrimitiveDataModified { index: usize },
    /// Viewport changed (pan/zoom)
    ViewportChanged,
    /// Compare series added
    CompareSeriesAdded { symbol: String },
    /// Compare series removed
    CompareSeriesRemoved { symbol: String },
    /// Compare series visibility changed
    CompareSeriesVisibility { symbol: String },
    /// Compare series color changed
    CompareSeriesColor { symbol: String },
    /// All compare series cleared
    AllCompareSeriesCleared,
    /// All compare series restored
    AllCompareSeriesRestored,
}
