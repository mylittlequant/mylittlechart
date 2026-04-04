//! Command Pattern for Undo/Redo
//!
//! All state-changing operations are represented as commands that can be
//! executed, undone, and redone.
//!
//! # Integration with DrawingManager
//!
//! New commands work with primitive indices (as used by DrawingManager):
//! - `SetPrimitiveVisibility` - uses index
//! - `SetPrimitiveLock` - uses index
//! - `CreatePrimitive` - stores type_id, points, data for recreation
//! - `DeletePrimitive` - stores full primitive snapshot for undo
//! - `MovePrimitive` - stores before/after points
//! - `ViewportChange` - stores before/after viewport state
//!
//! Legacy commands work with object IDs (for backward compatibility):
//! - `SetVisibility` - uses object_id
//! - `SetLock` - uses object_id

use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use crate::drawing::PrimitiveData;
use crate::chart::CompareSeries;
use crate::state::Timeframe;

// =============================================================================
// TimeframeVisibility - controls on which timeframes an object is shown
// =============================================================================

/// Timeframe visibility configuration for chart objects
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub enum TimeframeVisibility {
    /// Visible on all timeframes
    #[default]
    All,
    /// Visible only on specific timeframes
    Specific(Vec<Timeframe>),
    /// Visible on timeframes >= specified weight
    MinimumWeight(u32),
    /// Visible on timeframes <= specified weight
    MaximumWeight(u32),
    /// Visible in a range of timeframe weights
    WeightRange { min: u32, max: u32 },
}

impl TimeframeVisibility {
    /// Check if visible on a specific timeframe
    pub fn is_visible_on(&self, tf: &Timeframe) -> bool {
        match self {
            TimeframeVisibility::All => true,
            TimeframeVisibility::Specific(tfs) => tfs.contains(tf),
            TimeframeVisibility::MinimumWeight(min) => tf.weight() >= *min,
            TimeframeVisibility::MaximumWeight(max) => tf.weight() <= *max,
            TimeframeVisibility::WeightRange { min, max } => {
                let w = tf.weight();
                w >= *min && w <= *max
            }
        }
    }
}


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
// ObjectInfo - information about a chart object (used by legacy commands)
// =============================================================================

/// Information about a chart object (used by legacy commands for undo/redo)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectInfo {
    /// Unique object ID
    pub id: u64,
    /// Display name
    pub name: String,
    /// Object category
    pub category: ObjectCategory,
    /// Sub-type (e.g., "TrendLine", "SMA", "BuySignal")
    pub object_type: String,
    /// Current visibility state
    pub visible: bool,
    /// Current lock state
    pub locked: bool,
    /// Timeframe visibility configuration
    pub timeframe_visibility: TimeframeVisibility,
    /// Position in the tree (for ordering)
    pub order: i32,
    /// Parent object ID (for grouping)
    pub parent_id: Option<u64>,
    /// Creation timestamp
    pub created_at: u64,
    /// Last modified timestamp
    pub modified_at: u64,
    /// Symbol this object is associated with (if any)
    pub symbol: Option<String>,
    /// Custom properties
    pub properties: HashMap<String, String>,
    /// Position (for movable objects)
    pub position: Option<Position>,
}

impl ObjectInfo {
    /// Create a new object info
    pub fn new(id: u64, name: &str, category: ObjectCategory) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            id,
            name: name.to_string(),
            category,
            object_type: String::new(),
            visible: true,
            locked: false,
            timeframe_visibility: TimeframeVisibility::All,
            order: 0,
            parent_id: None,
            created_at: now,
            modified_at: now,
            symbol: None,
            properties: HashMap::new(),
            position: None,
        }
    }

    /// Set object type
    pub fn with_type(mut self, object_type: &str) -> Self {
        self.object_type = object_type.to_string();
        self
    }

    /// Set timeframe visibility
    pub fn with_timeframe_visibility(mut self, visibility: TimeframeVisibility) -> Self {
        self.timeframe_visibility = visibility;
        self
    }

    /// Set symbol
    pub fn with_symbol(mut self, symbol: &str) -> Self {
        self.symbol = Some(symbol.to_string());
        self
    }

    /// Check if object should be visible on a specific timeframe
    pub fn is_visible_on_timeframe(&self, tf: &Timeframe) -> bool {
        self.timeframe_visibility.is_visible_on(tf)
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

    /// Set primitive visibility by index (delegates to DrawingManager.set_visibility_at)
    SetPrimitiveVisibility {
        index: usize,
        visible: bool,
        previous: bool,
    },

    /// Set primitive lock by index (delegates to DrawingManager.set_lock_at)
    SetPrimitiveLock {
        index: usize,
        locked: bool,
        previous: bool,
    },

    /// Delete primitive by index (delegates to DrawingManager.delete_at)
    DeletePrimitive {
        index: usize,
        /// Type ID for recreation via registry
        type_id: String,
        /// Coordinate points
        points: Vec<(f64, f64)>,
        /// Full primitive data (id, color, style, etc.)
        data: PrimitiveData,
    },

    /// Delete all primitives
    DeleteAllPrimitives {
        /// All primitives before deletion (for undo) - stores full data for recreation
        primitives: Vec<(String, Vec<(f64, f64)>, PrimitiveData)>, // (type_id, points, data)
    },

    /// Restore all primitives (inverse of DeleteAllPrimitives - used for undo)
    RestoreAllPrimitives {
        /// All primitives to restore - stores full data for recreation
        primitives: Vec<(String, Vec<(f64, f64)>, PrimitiveData)>, // (type_id, points, data)
    },

    /// Set strategy visibility (delegates to SignalManager.set_strategy_visible)
    SetStrategyVisibility {
        strategy_tag: String,
        visible: bool,
        previous: bool,
    },

    /// Create primitive (for undo of delete, or recording creation)
    CreatePrimitive {
        /// Index where primitive was/will be inserted
        index: usize,
        /// Type ID for recreation via registry
        type_id: String,
        /// Coordinate points
        points: Vec<(f64, f64)>,
        /// Full primitive data (id, color, style, etc.)
        data: PrimitiveData,
    },

    /// Move/reshape primitive (stores points before and after)
    MovePrimitive {
        index: usize,
        /// Points before the move
        previous_points: Vec<(f64, f64)>,
        /// Points after the move
        new_points: Vec<(f64, f64)>,
    },

    /// Reorder primitive (bring to front / send to back)
    ReorderPrimitive {
        /// Original index before reorder
        old_index: usize,
        /// New index after reorder
        new_index: usize,
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
        index: usize,
        /// Data before modification
        previous_data: PrimitiveData,
        /// Data after modification
        new_data: PrimitiveData,
    },

    /// Modify full primitive (including type-specific data like Fib levels)
    /// Uses JSON serialization to capture complete state
    ModifyPrimitiveFull {
        index: usize,
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

    // =========================================================================
    // Legacy ID-based commands (for backward compatibility)
    // =========================================================================

    /// Set visibility by object ID
    SetVisibility {
        object_id: u64,
        visible: bool,
        previous: bool,
    },

    /// Set category visibility
    SetCategoryVisibility {
        category: ObjectCategory,
        visible: bool,
        previous_states: Vec<(u64, bool)>,
    },

    /// Set lock by object ID
    SetLock {
        object_id: u64,
        locked: bool,
        previous: bool,
    },

    /// Set global lock state
    SetGlobalLock {
        locked: bool,
        previous: bool,
    },

    /// Change timeframe (legacy - uses state::Timeframe)
    SetTimeframe {
        timeframe: Timeframe,
        previous: Timeframe,
    },

    /// Delete object by ID (legacy)
    DeleteObject {
        object_id: u64,
        object_info: ObjectInfo,
    },

    /// Delete all objects in a category
    DeleteCategory {
        category: ObjectCategory,
        objects: Vec<ObjectInfo>,
    },

    /// Delete all objects
    DeleteAll {
        objects: Vec<ObjectInfo>,
    },

    /// Create object (for undo of delete)
    CreateObject {
        object_info: ObjectInfo,
    },

    /// Modify object properties
    ModifyObject {
        object_id: u64,
        changes: HashMap<String, PropertyValue>,
        previous: HashMap<String, PropertyValue>,
    },

    /// Move object
    MoveObject {
        object_id: u64,
        new_position: Position,
        previous_position: Position,
    },
}

/// Property value for generic object modifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropertyValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Color(u32), // RGBA as u32
}

/// Position for move operations
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
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
            // New index-based commands
            Command::SetPrimitiveVisibility { index, visible, previous } => {
                Command::SetPrimitiveVisibility {
                    index: *index,
                    visible: *previous,
                    previous: *visible,
                }
            }

            Command::SetPrimitiveLock { index, locked, previous } => {
                Command::SetPrimitiveLock {
                    index: *index,
                    locked: *previous,
                    previous: *locked,
                }
            }

            Command::DeletePrimitive { index, type_id, points, data } => {
                // Inverse of delete is create - recreate the primitive
                Command::CreatePrimitive {
                    index: *index,
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

            Command::CreatePrimitive { index, type_id, points, data } => {
                // Inverse of create is delete
                Command::DeletePrimitive {
                    index: *index,
                    type_id: type_id.clone(),
                    points: points.clone(),
                    data: data.clone(),
                }
            }

            Command::MovePrimitive { index, previous_points, new_points } => {
                Command::MovePrimitive {
                    index: *index,
                    previous_points: new_points.clone(),
                    new_points: previous_points.clone(),
                }
            }

            Command::ReorderPrimitive { old_index, new_index } => {
                // Inverse of reorder swaps old and new indices
                Command::ReorderPrimitive {
                    old_index: *new_index,
                    new_index: *old_index,
                }
            }

            Command::ViewportChange { previous, new } => {
                Command::ViewportChange {
                    previous: *new,
                    new: *previous,
                }
            }

            Command::ModifyPrimitiveData { index, previous_data, new_data } => {
                Command::ModifyPrimitiveData {
                    index: *index,
                    previous_data: new_data.clone(),
                    new_data: previous_data.clone(),
                }
            }

            Command::ModifyPrimitiveFull { index, type_id, previous_json, new_json } => {
                Command::ModifyPrimitiveFull {
                    index: *index,
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

            // Legacy commands
            Command::SetVisibility { object_id, visible, previous } => {
                Command::SetVisibility {
                    object_id: *object_id,
                    visible: *previous,
                    previous: *visible,
                }
            }

            Command::SetCategoryVisibility { category, visible, previous_states } => {
                Command::SetCategoryVisibility {
                    category: *category,
                    visible: !visible,
                    previous_states: previous_states.clone(),
                }
            }

            Command::SetLock { object_id, locked, previous } => {
                Command::SetLock {
                    object_id: *object_id,
                    locked: *previous,
                    previous: *locked,
                }
            }

            Command::SetGlobalLock { locked, previous } => {
                Command::SetGlobalLock {
                    locked: *previous,
                    previous: *locked,
                }
            }

            Command::SetTimeframe { timeframe, previous } => {
                Command::SetTimeframe {
                    timeframe: previous.clone(),
                    previous: timeframe.clone(),
                }
            }

            Command::DeleteObject { object_id: _, object_info } => {
                Command::CreateObject {
                    object_info: object_info.clone(),
                }
            }

            Command::DeleteCategory { category, objects } => {
                Command::DeleteCategory {
                    category: *category,
                    objects: objects.clone(),
                }
            }

            Command::DeleteAll { objects } => {
                Command::DeleteAll {
                    objects: objects.clone(),
                }
            }

            Command::CreateObject { object_info } => {
                Command::DeleteObject {
                    object_id: object_info.id,
                    object_info: object_info.clone(),
                }
            }

            Command::ModifyObject { object_id, changes, previous } => {
                Command::ModifyObject {
                    object_id: *object_id,
                    changes: previous.clone(),
                    previous: changes.clone(),
                }
            }

            Command::MoveObject { object_id, new_position, previous_position } => {
                Command::MoveObject {
                    object_id: *object_id,
                    new_position: *previous_position,
                    previous_position: *new_position,
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
            Command::ReorderPrimitive { new_index, old_index } => {
                if *new_index > *old_index { "Bring to Front" } else { "Send to Back" }
            }
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

            // Symbol/Timeframe/ChartType commands
            Command::ChangeSymbol { .. } => "Change Symbol",
            Command::ChangeTimeframe { .. } => "Change Timeframe",
            Command::ChangeChartType { .. } => "Change Chart Type",

            // Legacy commands
            Command::SetVisibility { visible, .. } => {
                if *visible { "Show" } else { "Hide" }
            }
            Command::SetCategoryVisibility { visible, .. } => {
                if *visible { "Show Category" } else { "Hide Category" }
            }
            Command::SetLock { locked, .. } => {
                if *locked { "Lock" } else { "Unlock" }
            }
            Command::SetGlobalLock { locked, .. } => {
                if *locked { "Lock All" } else { "Unlock All" }
            }
            Command::SetTimeframe { .. } => "Change Timeframe",
            Command::DeleteObject { .. } => "Delete",
            Command::DeleteCategory { .. } => "Delete Category",
            Command::DeleteAll { .. } => "Delete All",
            Command::CreateObject { .. } => "Create",
            Command::ModifyObject { .. } => "Modify",
            Command::MoveObject { .. } => "Move",
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
