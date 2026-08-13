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
//! - `CreatePrimitive` / `DeletePrimitive` - each other's inverse, both
//!   carrying a full `PrimitiveSnapshot` (never a `PrimitiveData` subset —
//!   see each variant's own doc for why) plus every primitive a cascading
//!   delete also removed
//! - `MovePrimitive` - stores before/after points
//! - `PromoteWaveChain` / `DemoteWaveChain` - collapse/restore a
//!   `follows`-linked Elliott chain into the degree above (E4)
//! - `ConvertPrimitiveKind` - change an Elliott structure's own registry
//!   type (and correction subtype) IN PLACE mid-count, remapping its own
//!   subdivisions by ordinal rather than by name (owner report 2026-08-14,
//!   see the variant's own doc)
//!
//! The pre-`ChartContainer` object-ID-based command family (`ObjectInfo`,
//! `SetVisibility`, `SetLock`, `SetGlobalLock`, `SetCategoryVisibility`,
//! `DeleteObject`, `DeleteCategory`, `DeleteAll`, `CreateObject`,
//! `ModifyObject`, `MoveObject`, the legacy `SetTimeframe`, `PropertyValue`,
//! `Position`, `TimeframeVisibility`) was deleted 2026-07-02 (tail wave of
//! `docs/mlc/plans/doctrine-cutover-2026-06-27.md`) — audit confirmed zero
//! constructors anywhere in the workspace outside test placeholders.

use serde::{Serialize, Deserialize};
use crate::drawing::{PrimitiveData, PrimitiveSnapshot};
use crate::drawing::primitives_v2::elliott::WaveSlot;
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

    /// Delete primitive by stable id (delegates to `DrawingsMut::delete`).
    ///
    /// Carries the WHOLE primitive, not a `PrimitiveData` subset — a
    /// snapshot of its own serialized form (`PrimitiveSnapshot`), so undo
    /// restores every field the concrete type carries, including the ones
    /// outside `PrimitiveData` entirely (an Elliott wave's own
    /// `degree`/`link`/`follows` — see `drawing::primitives_v2::elliott::
    /// count`'s own module doc). `points` travels separately, not parsed
    /// out of `snapshot.json`, because `Command::primitive_identity` must
    /// hand back a borrowed slice for cold-load id matching.
    ///
    /// `cascaded` snapshots every OTHER primitive `DrawingManager::delete`'s
    /// own subdivision cascade also removed (e.g. Elliott children nested
    /// under `prim_id`), captured BEFORE the cascade ran — empty for a
    /// primitive with no children. Undo (`inverse` -> `CreatePrimitive`)
    /// restores the primary primitive AND every cascaded one together, so a
    /// single Ctrl+Z brings back the whole subtree, not just the primitive
    /// the user clicked delete on.
    DeletePrimitive {
        prim_id: u64,
        points: Vec<(i64, f64)>,
        snapshot: PrimitiveSnapshot,
        cascaded: Vec<PrimitiveSnapshot>,
    },

    /// Delete all primitives.
    DeleteAllPrimitives {
        /// Every primitive before deletion (for undo) — full snapshots, not
        /// a `PrimitiveData` subset (see `DeletePrimitive`'s own doc for
        /// why).
        primitives: Vec<PrimitiveSnapshot>,
    },

    /// Restore all primitives (inverse of DeleteAllPrimitives - used for undo)
    RestoreAllPrimitives {
        /// Every primitive to restore — full snapshots (see
        /// `DeletePrimitive`'s own doc).
        primitives: Vec<PrimitiveSnapshot>,
    },

    /// Create primitive (for undo of delete, or recording creation). See
    /// `DeletePrimitive`'s own doc — same snapshot shape, same
    /// `cascaded` meaning (populated when this is the `inverse()` of a
    /// cascading `DeletePrimitive`; empty for an ordinary creation).
    CreatePrimitive {
        /// Stable id of the primitive being created/restored.
        prim_id: u64,
        points: Vec<(i64, f64)>,
        snapshot: PrimitiveSnapshot,
        cascaded: Vec<PrimitiveSnapshot>,
    },

    /// Move/reshape primitive (stores points before and after)
    MovePrimitive {
        prim_id: u64,
        /// Points before the move
        previous_points: Vec<(i64, f64)>,
        /// Points after the move
        new_points: Vec<(i64, f64)>,
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

    /// Collapse a `follows`-linked same-degree chain into one coarser
    /// structure (E4 — see `drawing::primitives_v2::elliott::count::
    /// promote_chain_shape`, `DrawingManager::promote_wave_chain`).
    ///
    /// `parent_snapshot` is the newly built parent's own full serialized
    /// state (its id is `parent_id`, also embedded in the snapshot's own
    /// JSON). `members` is every chain member's id paired with the slot it
    /// was stamped into — redo re-stamps `WaveLink { parent_id, slot }` on
    /// each; undo (`inverse` -> `DemoteWaveChain`) clears it back to `None`
    /// on each and removes the parent. The chain members THEMSELVES are
    /// never snapshotted here — they already existed before promotion and
    /// are untouched by it beyond gaining/losing their own `link`.
    PromoteWaveChain {
        parent_id: u64,
        parent_snapshot: PrimitiveSnapshot,
        members: Vec<(u64, WaveSlot)>,
    },

    /// Inverse of [`Command::PromoteWaveChain`] — same fields, opposite
    /// application: clear `link` back to `None` on every member, then
    /// remove the parent (safe to do in that order only — a member still
    /// pointing at the parent would otherwise fall into the parent's own
    /// subdivision-cascade delete, which is exactly the DESTRUCTIVE
    /// behaviour a demote must not have: the chain members pre-date the
    /// promotion and must survive it being undone).
    DemoteWaveChain {
        parent_id: u64,
        parent_snapshot: PrimitiveSnapshot,
        members: Vec<(u64, WaveSlot)>,
    },

    /// Change WHAT an Elliott structure fundamentally IS — its own registry
    /// `type_id` (and, for a correction, its own subtype) — while keeping
    /// the polyline the user actually drew (owner report 2026-08-14: "я
    /// создал АБЦ как зигзаг, но в процессе понял, что хочу сделать его
    /// плоскостью или ещё чем-то — не получается переменить весь отрезок
    /// ... допустим я выбрал зигзаг, а в процессе понял, что это скорее
    /// двойной зигзаг — тоже не могу прямо в процессе разметки переменить
    /// глобальное состояние отрезка"). [`Command::ModifyPrimitiveFull`]
    /// cannot carry this: it holds exactly ONE `type_id`, and a conversion
    /// is precisely the case where the type BEFORE and the type AFTER
    /// differ.
    ///
    /// `before`/`after` are full serialized snapshots — same
    /// [`PrimitiveSnapshot`] shape [`Command::DeletePrimitive`]'s own doc
    /// explains, never a `PrimitiveData` subset — captured on either side
    /// of the conversion: `before.type_id` is the structure's OLD registry
    /// type, `after.type_id` is the NEW one. Both carry `prim_id`'s own
    /// point array (already resampled to the new pivot count by
    /// `drawing::primitives_v2::elliott::count::resample_polyline`, which
    /// keeps the drawn shape and the structure's own welded endpoints — see
    /// that function's own doc), so undo/redo never re-derives the shape,
    /// only replays a snapshot exactly like every other primitive command
    /// here does.
    ///
    /// `children` carries every subdivision nested inside `prim_id` at the
    /// moment of conversion, remapped by ORDINAL — see
    /// `drawing::primitives_v2::elliott::count::{slot_ordinal,
    /// slot_at_ordinal}`'s own doc for why ordinal, not name, is the
    /// mapping a child survives a parent's own type change by:
    /// `(child_id, slot_before, slot_after)`. `slot_after` is `None` when
    /// the child's own ordinal has no match on the new type (the child
    /// loses its own link rather than being deleted); `slot_before` is
    /// `None` only for the symmetric case an undo of THIS command produces
    /// — a child that gained a slot the OLD type never had (the conversion
    /// added waves the old shape did not carry).
    ///
    /// Apply logic lives in `mlc-app`, not here — this variant only carries
    /// what undo/redo needs to reverse the edit (same division of labour
    /// [`Command::PromoteWaveChain`]'s own doc already draws).
    ConvertPrimitiveKind {
        prim_id: u64,
        before: PrimitiveSnapshot,
        after: PrimitiveSnapshot,
        children: Vec<(u64, Option<WaveSlot>, Option<WaveSlot>)>,
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

            Command::DeletePrimitive { prim_id, points, snapshot, cascaded } => {
                // Inverse of delete is create - recreate the primitive
                Command::CreatePrimitive {
                    prim_id: *prim_id,
                    points: points.clone(),
                    snapshot: snapshot.clone(),
                    cascaded: cascaded.clone(),
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

            Command::CreatePrimitive { prim_id, points, snapshot, cascaded } => {
                // Inverse of create is delete
                Command::DeletePrimitive {
                    prim_id: *prim_id,
                    points: points.clone(),
                    snapshot: snapshot.clone(),
                    cascaded: cascaded.clone(),
                }
            }

            Command::PromoteWaveChain { parent_id, parent_snapshot, members } => {
                Command::DemoteWaveChain {
                    parent_id: *parent_id,
                    parent_snapshot: parent_snapshot.clone(),
                    members: members.clone(),
                }
            }

            Command::DemoteWaveChain { parent_id, parent_snapshot, members } => {
                Command::PromoteWaveChain {
                    parent_id: *parent_id,
                    parent_snapshot: parent_snapshot.clone(),
                    members: members.clone(),
                }
            }

            Command::ConvertPrimitiveKind { prim_id, before, after, children } => {
                Command::ConvertPrimitiveKind {
                    prim_id: *prim_id,
                    before: after.clone(),
                    after: before.clone(),
                    children: children.iter().map(|&(id, before_slot, after_slot)| (id, after_slot, before_slot)).collect(),
                }
            }

            Command::MovePrimitive { prim_id, previous_points, new_points } => {
                Command::MovePrimitive {
                    prim_id: *prim_id,
                    previous_points: new_points.clone(),
                    new_points: previous_points.clone(),
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
    ///
    /// `PromoteWaveChain`/`DemoteWaveChain`/`ConvertPrimitiveKind` act on
    /// MULTIPLE primitives (a parent/converted primitive plus every chain
    /// member/subdivided child) and are deliberately excluded here, same as
    /// `DeleteAllPrimitives`/`RestoreAllPrimitives` — the single-id remap
    /// machinery below (`set_prim_id`/`primitive_identity`) only ever
    /// covers a command with exactly one primitive of its own.
    pub fn prim_id(&self) -> Option<u64> {
        match self {
            Command::SetPrimitiveVisibility { prim_id, .. }
            | Command::SetPrimitiveLock { prim_id, .. }
            | Command::DeletePrimitive { prim_id, .. }
            | Command::CreatePrimitive { prim_id, .. }
            | Command::MovePrimitive { prim_id, .. }
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
    ///
    /// `DeletePrimitive`/`CreatePrimitive` patch their own `snapshot`'s
    /// embedded id too (`PrimitiveSnapshot::with_id`) — the id lives twice
    /// now (the command's own `prim_id` field, for cheap lookup, and inside
    /// the serialized snapshot, which is what actually gets deserialized on
    /// apply), and both copies must agree or the recreated primitive would
    /// carry the OLD id despite the command claiming the new one.
    /// `cascaded` snapshots are left untouched — a cascade-deleted child's
    /// own id is not what this command is "about" for remap purposes, only
    /// the primary `prim_id` is (same scope `primitive_identity` covers).
    pub fn set_prim_id(&mut self, new_id: u64) {
        match self {
            Command::SetPrimitiveVisibility { prim_id, .. }
            | Command::SetPrimitiveLock { prim_id, .. }
            | Command::MovePrimitive { prim_id, .. }
            | Command::ModifyPrimitiveFull { prim_id, .. } => *prim_id = new_id,
            Command::DeletePrimitive { prim_id, snapshot, .. }
            | Command::CreatePrimitive { prim_id, snapshot, .. } => {
                *snapshot = snapshot.with_id(new_id);
                *prim_id = new_id;
            }
            Command::ModifyPrimitiveData { prim_id, new_data, .. } => {
                *prim_id = new_id;
                new_data.id = new_id;
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
            Command::CreatePrimitive { snapshot, points, .. }
            | Command::DeletePrimitive { snapshot, points, .. } => {
                Some((snapshot.type_id.as_str(), points.as_slice()))
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
            Command::ModifyPrimitiveData { .. } => "Modify Primitive",
            Command::ModifyPrimitiveFull { .. } => "Modify Primitive Settings",
            Command::PromoteWaveChain { .. } => "Promote Wave Chain",
            Command::DemoteWaveChain { .. } => "Demote Wave Chain",
            Command::ConvertPrimitiveKind { .. } => "Convert Wave Kind",

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
