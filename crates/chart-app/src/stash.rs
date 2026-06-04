//! Stash helpers for per-chart primitive storage.
//!
//! Replaces the `stashed_primitives` helpers that previously lived in
//! `branch_meta_bridge.rs` and were keyed by `BranchId`.  The map is
//! now keyed by `ChartId` directly — the BranchId wrapper was a no-op
//! (`BranchId(chart_id.0)`).
//!
//! These helpers operate on `&mut HashMap<ChartId, Vec<Box<dyn Primitive>>>`.

use std::collections::HashMap;

use zengeld_chart::drawing::primitives_v2::Primitive;
use zengeld_chart::state::ChartId;

/// Replace the stashed primitives for `chart_id` with `prims`.
pub fn stash_primitives(
    stashed_primitives: &mut HashMap<ChartId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
    prims: Vec<Box<dyn Primitive>>,
) {
    stashed_primitives.insert(chart_id, prims);
}

/// Take ownership of the stashed primitives for `chart_id`, removing the
/// entry.  Returns `None` if there was no stash.
pub fn take_stashed_primitives(
    stashed_primitives: &mut HashMap<ChartId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
) -> Option<Vec<Box<dyn Primitive>>> {
    stashed_primitives.remove(&chart_id)
}

/// Remove the stash entry for `chart_id` without taking ownership.
pub fn clear_stashed_primitives(
    stashed_primitives: &mut HashMap<ChartId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
) {
    stashed_primitives.remove(&chart_id);
}

/// Returns `true` if there are any stashed primitives for `chart_id`.
pub fn has_stashed_primitives(
    stashed_primitives: &HashMap<ChartId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
) -> bool {
    stashed_primitives
        .get(&chart_id)
        .map_or(false, |v| !v.is_empty())
}

/// Read-only view of the stashed primitives for `chart_id`.
pub fn read_stashed_primitives(
    stashed_primitives: &HashMap<ChartId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
) -> Option<&Vec<Box<dyn Primitive>>> {
    stashed_primitives.get(&chart_id)
}

/// Mutable reference to the stashed primitives vec for `chart_id`.
pub fn stashed_primitives_mut(
    stashed_primitives: &mut HashMap<ChartId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
) -> Option<&mut Vec<Box<dyn Primitive>>> {
    stashed_primitives.get_mut(&chart_id)
}

/// Append a single primitive; lazy-inserts the entry if missing.
pub fn push_stashed_primitive(
    stashed_primitives: &mut HashMap<ChartId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
    prim: Box<dyn Primitive>,
) {
    stashed_primitives
        .entry(chart_id)
        .or_default()
        .push(prim);
}

/// Extend with multiple primitives; lazy-inserts the entry if missing.
pub fn extend_stashed_primitives(
    stashed_primitives: &mut HashMap<ChartId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
    prims: impl IntoIterator<Item = Box<dyn Primitive>>,
) {
    stashed_primitives
        .entry(chart_id)
        .or_default()
        .extend(prims);
}

/// Retain primitives by predicate.  Returns `true` if any were removed.
pub fn retain_stashed_primitives(
    stashed_primitives: &mut HashMap<ChartId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
    pred: impl FnMut(&Box<dyn Primitive>) -> bool,
) -> bool {
    if let Some(v) = stashed_primitives.get_mut(&chart_id) {
        let before = v.len();
        v.retain(pred);
        v.len() < before
    } else {
        false
    }
}

/// Clone all stashed primitives via `Primitive::clone_box`.
pub fn clone_stashed_primitives(
    stashed_primitives: &HashMap<ChartId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
) -> Vec<Box<dyn Primitive>> {
    stashed_primitives
        .get(&chart_id)
        .map(|v| v.iter().map(|p| p.clone_box()).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stashed_primitives_round_trip_take() {
        let mut sp: HashMap<ChartId, Vec<Box<dyn Primitive>>> = HashMap::new();
        stash_primitives(&mut sp, ChartId(19), Vec::new());
        let taken = take_stashed_primitives(&mut sp, ChartId(19));
        assert!(taken.is_some());
        assert!(taken.unwrap().is_empty());
        assert!(sp.is_empty(), "take should remove the entry");
    }

    #[test]
    fn take_stashed_primitives_returns_none_when_missing() {
        let mut sp: HashMap<ChartId, Vec<Box<dyn Primitive>>> = HashMap::new();
        let taken = take_stashed_primitives(&mut sp, ChartId(123));
        assert!(taken.is_none());
    }
}
