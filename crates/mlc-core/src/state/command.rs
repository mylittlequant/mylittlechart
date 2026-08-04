//! Command Pattern for Undo/Redo
//!
//! # What belongs in the history (owner ruling, 2026-08-04)
//!
//! Undo/redo covers **chart objects and nothing else**: primitives,
//! indicators and compare series — created, moved, restyled, deleted.
//!
//! Explicitly NOT in the history, and not to be re-added:
//! - **viewport** (pan / zoom) — navigation is not an edit. It also poisoned
//!   the stack: a pan recorded a command, so Ctrl+Z undid the pan instead of
//!   the edit, and any pan between an undo and a redo cleared the redo stack.
//! - **symbol / timeframe / chart type** — same reason: choosing what to look
//!   at is navigation, not an edit of the thing being looked at.
//! Alerts ARE in: an alert is an object the user created, often attached to
//! a drawing, so an accidental delete must be takeable back. Only its
//! CREATION and DELETION though — firing and expiry are events, not edits.
//!
//! # Integration with DrawingManager
//!
//! Commands work with primitive ids (as used by DrawingManager):
//! - `SetPrimitiveVisibility` / `SetPrimitiveLock` - by stable id
//! - `CreatePrimitive` - stores type_id, points, data for recreation
//! - `DeletePrimitive` - stores full primitive snapshot for undo
//! - `MovePrimitive` - stores before/after points
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
use crate::chart::{CompareSeries, CompareSeriesStyle};
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

    /// Set the whole presentation of a compare series.
    ///
    /// A snapshot rather than one command per field, because the edits arrive
    /// in GROUPS: "reset to defaults" and "apply template" rewrite colour,
    /// width, style and chart type together, and one Ctrl+Z should put all
    /// four back rather than four.
    SetCompareSeriesStyle {
        symbol: String,
        previous: CompareSeriesStyle,
        new: CompareSeriesStyle,
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

    /// Change an indicator's parameters / outputs.
    ///
    /// Carries the whole instance before and after rather than one field:
    /// parameter edits arrive in groups (reset to defaults, apply template),
    /// and one Ctrl+Z should put the whole set back.
    ModifyIndicator {
        instance_id: u64,
        previous_json: String,
        new_json: String,
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

            Command::SetCompareSeriesStyle { symbol, previous, new } => {
                Command::SetCompareSeriesStyle {
                    symbol: symbol.clone(),
                    previous: new.clone(),
                    new: previous.clone(),
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

            Command::ModifyIndicator { instance_id, previous_json, new_json } => {
                Command::ModifyIndicator {
                    instance_id: *instance_id,
                    previous_json: new_json.clone(),
                    new_json: previous_json.clone(),
                }
            }

            // Alert commands
            Command::AddAlert { snapshot } => {
                Command::DeleteAlert { snapshot: snapshot.clone() }
            }

            Command::DeleteAlert { snapshot } => {
                Command::AddAlert { snapshot: snapshot.clone() }
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

        }
    }

    /// The primitive this command acts on, when it acts on one.
    pub fn prim_id(&self) -> Option<u64> {
        match self {
            Command::SetPrimitiveVisibility { prim_id, .. }
            | Command::SetPrimitiveLock { prim_id, .. }
            | Command::DeletePrimitive { prim_id, .. }
            | Command::CreatePrimitive { prim_id, .. }
            | Command::MovePrimitive { prim_id, .. }
            | Command::ReorderPrimitive { prim_id, .. }
            | Command::ModifyPrimitiveData { prim_id, .. }
            | Command::ModifyPrimitiveFull { prim_id, .. } => Some(*prim_id),
            _ => None,
        }
    }

    /// Point this command at a different primitive id.
    ///
    /// A cold load re-numbers primitives (collision and duplicate sweeps), so
    /// a restored history refers to ids that no longer exist. Rewriting is
    /// what keeps it usable — without it undo silently matches nothing and
    /// redo re-creates a second copy of a drawing that is already there.
    pub fn set_prim_id(&mut self, new_id: u64) {
        match self {
            Command::SetPrimitiveVisibility { prim_id, .. }
            | Command::SetPrimitiveLock { prim_id, .. }
            | Command::MovePrimitive { prim_id, .. }
            | Command::ReorderPrimitive { prim_id, .. }
            | Command::ModifyPrimitiveFull { prim_id, .. } => *prim_id = new_id,
            Command::DeletePrimitive { prim_id, data, .. }
            | Command::CreatePrimitive { prim_id, data, .. }
            | Command::ModifyPrimitiveData { prim_id, new_data: data, .. } => {
                *prim_id = new_id;
                data.id = new_id;
            }
            _ => {}
        }
        if let Command::ModifyPrimitiveData { previous_data, .. } = self {
            previous_data.id = new_id;
        }
    }

    /// What this command says the primitive IS — its type and geometry.
    ///
    /// The only identity that survives a re-numbering, and therefore the key
    /// a restored history is matched against the live drawings by.
    pub fn primitive_identity(&self) -> Option<(&str, &[(i64, f64)])> {
        match self {
            Command::CreatePrimitive { type_id, points, .. }
            | Command::DeletePrimitive { type_id, points, .. } => {
                Some((type_id.as_str(), points.as_slice()))
            }
            _ => None,
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
            Command::CreatePrimitive { .. } => "Create Primitive",
            Command::MovePrimitive { .. } => "Move Primitive",
            Command::ReorderPrimitive { .. } => "Reorder Primitive",
            Command::ModifyPrimitiveData { .. } => "Modify Primitive",
            Command::ModifyPrimitiveFull { .. } => "Modify Primitive Settings",

            // Compare commands
            Command::AddCompareSeries { .. } => "Add Compare Symbol",
            Command::RemoveCompareSeries { .. } => "Remove Compare Symbol",
            Command::SetCompareSeriesVisibility { visible, .. } => {
                if *visible { "Show Compare" } else { "Hide Compare" }
            }
            Command::SetCompareSeriesStyle { .. } => "Change Compare Style",
            Command::ClearAllCompareSeries { .. } => "Clear All Compare",
            Command::RestoreAllCompareSeries { .. } => "Restore All Compare",

            // Indicator commands
            Command::AddIndicator { .. } => "Add Indicator",
            Command::RemoveIndicator { .. } => "Remove Indicator",
            Command::ModifyIndicator { .. } => "Change Indicator Settings",

            // Alert commands
            Command::AddAlert { .. } => "Add Alert",
            Command::DeleteAlert { .. } => "Delete Alert",
        }
    }
}
