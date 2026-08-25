//! Adoption — a coarser claim recognising markup that is already on the
//! chart as part of itself.
//!
//! ## Half of this arrived with containment, and that is the design
//!
//! The plan's own S5 asks for two things: an AMD cycle drawn over an
//! existing sweep should ADOPT it, and the cycle's phases should WELD onto
//! that sweep's exact anchor.
//!
//! The first half needs no code here. Containment
//! ([`super::link`]) is derived from geometry on every pass rather than
//! declared once at creation, so a cycle drawn over an existing sweep
//! already reports that sweep as its manipulation phase, and the sweep
//! already reports `Inside: AMD Cycle — Manipulation`. Elliott has to
//! detect adoption explicitly (`adoptable_children`) because its own link
//! is an assertion the analyst makes; deriving the link instead makes
//! adoption the default rather than a feature.
//!
//! What is left is the welding, and it is a real gap: containment answers
//! "the sweep is inside the cycle" while leaving the cycle anchored one or
//! two bars off the sweep it is talking about. Two claims about the SAME
//! event that name different bars disagree, however politely.
//!
//! ## What welds onto what
//!
//! Only where two claims are about one event, never merely nearby:
//!
//! - **`amd_cycle` onto `liquidity_sweep`** — the cycle's manipulation
//!   phase IS a sweep (`docs/mlc/plans/markup-engine-parity-2026-08-25.md`
//!   §2.2), and the cycle anchors on its manipulation bar. Same event, two
//!   scales.
//!
//! That is the whole table today, and it is short on purpose. An order
//! block near a gap is two readings of one area, not one event seen twice,
//! and welding them would fabricate an agreement the method does not make.

use super::SlotId;

/// The claim type `container_type` should anchor onto when one is already
/// drawn within reach — `None` for a type that welds onto nothing.
///
/// See this module's own doc for why the table is one row.
pub fn welds_onto(container_type: &str) -> Option<&'static str> {
    match container_type {
        "amd_cycle" => Some("liquidity_sweep"),
        _ => None,
    }
}

/// Which of `container_type`'s own slots the welded claim occupies — the
/// phase the two claims agree they are both talking about.
///
/// Returned for documentation and for a caller that wants to name the
/// relation; the LINK itself is not written from here. Containment is
/// derived every pass from geometry, and a second writer stamping the same
/// edge by hand is how the two get to disagree.
pub fn welded_slot(container_type: &str) -> Option<SlotId> {
    match container_type {
        "amd_cycle" => Some(super::slots::MANIPULATION),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cycle_welds_onto_the_sweep_it_is_about() {
        assert_eq!(welds_onto("amd_cycle"), Some("liquidity_sweep"));
        assert_eq!(welded_slot("amd_cycle"), Some(super::super::slots::MANIPULATION));
    }

    /// Nearby is not the same event — see the module doc.
    #[test]
    fn nothing_else_welds() {
        for t in ["order_block", "fair_value_gap", "structure_break", "dealing_range", "trend_line"] {
            assert!(welds_onto(t).is_none(), "{t} must not weld");
            assert!(welded_slot(t).is_none(), "{t} has no welded slot");
        }
    }

    /// The two tables answer for the same set of types — a container that
    /// welds must name the slot it welds into.
    #[test]
    fn the_two_tables_agree_on_which_types_weld() {
        for t in ["amd_cycle", "order_block", "liquidity_sweep", "trend_line"] {
            assert_eq!(
                welds_onto(t).is_some(),
                welded_slot(t).is_some(),
                "{t} answers one table and not the other",
            );
        }
    }
}
