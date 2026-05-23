//! Slot inner separator hit-testing — mirrors `ChartPanelGrid::resolve_input`.
//!
//! Separators have priority over leaf bodies so that thin drag targets are
//! reachable even when they overlap a leaf rect edge.

use uzor::panels::SeparatorOrientation;
use crate::free_slot::FreeItem;

/// Result of hit-testing a slot's docking area.
#[derive(Debug, Clone, PartialEq)]
pub enum SlotInputTarget {
    /// Cursor is on an internal separator divider.
    Separator {
        sep_idx: usize,
        orientation: SeparatorOrientation,
    },
    /// Cursor is inside a leaf panel body.
    Leaf {
        leaf_id: uzor::panels::LeafId,
    },
    /// Cursor is in the slot area but not on any element.
    None,
}

/// Hit-test absolute screen coordinate `(x, y)` against the docking layout
/// of a free slot.
///
/// Mirrors `ChartPanelGrid::resolve_input` but for the slot's
/// `DockState<FreeItem>`.  Separators are checked first (they are thin
/// and must win over leaf body hits).
///
/// The separator positions returned by `docking.separators()` are in
/// **absolute screen coordinates** (the manager was laid out with absolute
/// `inner_x / inner_y`), so `x` and `y` are passed through directly without
/// origin subtraction.
pub fn slot_resolve_input(
    docking: &uzor::layout::DockState<FreeItem>,
    x: f64,
    y: f64,
) -> SlotInputTarget {
    let fx = x as f32;
    let fy = y as f32;

    // 1. Separators take priority — they are thin and easily missed otherwise.
    for (sep_idx, sep) in docking.separators().iter().enumerate() {
        if sep.hit_test(fx, fy) {
            return SlotInputTarget::Separator {
                sep_idx,
                orientation: sep.orientation,
            };
        }
    }

    // 2. Leaf bodies.
    for (&leaf_id, rect) in docking.panel_rects() {
        if fx >= rect.x
            && fx < rect.x + rect.width
            && fy >= rect.y
            && fy < rect.y + rect.height
        {
            return SlotInputTarget::Leaf { leaf_id };
        }
    }

    SlotInputTarget::None
}
