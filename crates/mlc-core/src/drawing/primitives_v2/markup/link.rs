//! `MarkupLink` — the containment edge between two markup claims, and the
//! slot vocabulary each family names its own regions with.
//!
//! This is the general half of `elliott::count::WaveLink`, and the one
//! place where the shared layer genuinely could NOT reuse Elliott's shape
//! (`docs/mlc/plans/markup-engine-parity-2026-08-25.md` §2.2). An Elliott
//! slot names a **point span**: wave 3 of an impulse is the segment between
//! the parent's own point 2 and point 3, so `slot_span` can hand back two
//! `(timestamp, price)` pairs read straight off the parent. A smart-money
//! slot names a **region**: the premium half of a dealing range is every
//! price above equilibrium inside the range's own bars, and no pair of the
//! parent's own drawn points delimits it. So the lookup inverts — instead
//! of the parent handing out a span, the parent is ASKED whether a given
//! child anchor falls inside one of its slots
//! ([`super::super::Primitive::slot_containing`]).
//!
//! ## Why the slot is a token and not an enum
//!
//! The families' slot sets are disjoint: nothing that is `Premium` is ever
//! `Accumulation`, and order flow's future slots will share neither. A
//! single shared enum would therefore grow one variant per family forever
//! and force every `match` to carry arms it can never reach. A `&'static
//! str` token declared by the family that owns it costs one table lookup
//! for the localized name ([`slot_name_key`]) and nothing else.
//!
//! ## Why the link is persisted rather than recomputed on read
//!
//! Containment is derivable from coordinates, so a cache would do — but
//! S5's own auto-adoption writes a link the analyst can then keep across a
//! reload, and Elliott's `WaveLink` is persisted for exactly that reason.
//! The rule that keeps a persisted link from lying is in the maintaining
//! pass, not here: `mlc_backend::bubble::markup_guards` fills an empty
//! link, re-slots one whose half changed, and clears one whose parent no
//! longer contains the child — every refresh, so a stale edge cannot
//! survive a drag.

use serde::{Deserialize, Serialize};

use super::super::config::StatRow;
use crate::i18n::{current_language, ConceptKey, PrimitiveNameKey};

/// A slot token as the family that owns the parent declares it — see this
/// module's own doc for why this is a token rather than a shared enum.
///
/// Returned by [`super::super::Primitive::slot_containing`]; stored in
/// [`MarkupLink::slot`] as an owned `String` because the link is persisted
/// and a borrowed `&'static str` cannot come back out of `serde`.
pub type SlotId = &'static str;

/// Every slot token the shipped families declare, in one place so a
/// mistyped literal is a compile error rather than a link that silently
/// never matches.
pub mod slots {
    use super::SlotId;

    /// `amd_cycle`'s own three phases, in the order the cycle runs.
    pub const ACCUMULATION: SlotId = "accumulation";
    pub const MANIPULATION: SlotId = "manipulation";
    pub const DISTRIBUTION: SlotId = "distribution";

    /// `dealing_range`'s own two halves, plus the midpoint itself.
    ///
    /// [`EQUILIBRIUM`] exists because the two halves are STRICTLY above and
    /// strictly below (`DealingRange::is_premium`/`is_discount`), and a
    /// child anchored exactly on the midpoint is nonetheless inside the
    /// range. Without a third token it would report no slot at all, which
    /// would read as "not contained" — the one wrong answer.
    pub const PREMIUM: SlotId = "premium";
    pub const EQUILIBRIUM: SlotId = "equilibrium";
    pub const DISCOUNT: SlotId = "discount";
}

/// The localized name of a slot token, for the Stat row a linked child
/// shows. `None` for a token no shipped family declares — a persisted link
/// written by a newer build, which the row then renders by its raw token
/// rather than dropping.
pub fn slot_name_key(slot: &str) -> Option<ConceptKey> {
    match slot {
        slots::ACCUMULATION => Some(ConceptKey::AccumulationLabel),
        slots::MANIPULATION => Some(ConceptKey::ManipulationLabel),
        slots::DISTRIBUTION => Some(ConceptKey::DistributionLabel),
        slots::PREMIUM => Some(ConceptKey::DealingRangePremiumLabel),
        slots::EQUILIBRIUM => Some(ConceptKey::DealingRangeEquilibriumLabel),
        slots::DISCOUNT => Some(ConceptKey::DealingRangeDiscountLabel),
        _ => None,
    }
}

/// This claim's own edge upward: which parent claim contains it, and which
/// of that parent's slots it sits in.
///
/// `parent_type_id` is carried rather than looked up because the Stat row
/// that renders this link is built by the child alone
/// ([`link_stat_row`]) — a primitive cannot see its siblings, and giving
/// the config layer a bubble lookup purely to name a parent would push
/// collection state into the one place that has always been free of it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MarkupLink {
    /// The parent's own [`super::super::PrimitiveData::id`].
    pub parent_id: u64,
    /// The parent's own `type_id`, for naming it without a lookup.
    pub parent_type_id: String,
    /// One of the parent's own slot tokens — see [`slots`].
    pub slot: String,
}

impl MarkupLink {
    pub fn new(parent_id: u64, parent_type_id: &str, slot: SlotId) -> Self {
        Self { parent_id, parent_type_id: parent_type_id.to_string(), slot: slot.to_string() }
    }

    /// `"Dealing Range — Premium"`, both halves localized. Falls back to
    /// the raw `type_id`/token for anything a newer build wrote.
    pub fn label(&self) -> String {
        let lang = current_language();
        let parent = PrimitiveNameKey::from_type_id(&self.parent_type_id)
            .map(|k| k.get(lang).to_string())
            .unwrap_or_else(|| self.parent_type_id.clone());
        let slot = slot_name_key(&self.slot)
            .map(|k| k.get(lang).to_string())
            .unwrap_or_else(|| self.slot.clone());
        format!("{parent} \u{2014} {slot}")
    }
}

/// The Stat row a linked child prepends to its own result block — the
/// shared render for every family, so the answer to "which half am I in"
/// reads identically on an order block and on a gap.
pub fn link_stat_row(link: Option<&MarkupLink>) -> Option<StatRow> {
    let link = link?;
    Some(StatRow::new(
        ConceptKey::MarkupInsideLabel.get(current_language()),
        &link.label(),
        None,
    ))
}

/// `rows` with the containment row in front of it, or `rows` untouched
/// when there is no link.
///
/// The prepend order is the point: a claim's own result block answers
/// "what did I resolve to", and where the claim SITS frames every row
/// under it. Called at the `stat_block` site rather than inside each
/// family's own `result_stat_rows` so a claim that resolved to nothing
/// still reports its containment instead of returning an empty block.
pub fn with_link_row(link: Option<&MarkupLink>, rows: Vec<StatRow>) -> Vec<StatRow> {
    match link_stat_row(link) {
        Some(row) => std::iter::once(row).chain(rows).collect(),
        None => rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_declared_slot_has_a_localized_name() {
        for slot in [
            slots::ACCUMULATION,
            slots::MANIPULATION,
            slots::DISTRIBUTION,
            slots::PREMIUM,
            slots::EQUILIBRIUM,
            slots::DISCOUNT,
        ] {
            assert!(slot_name_key(slot).is_some(), "{slot} has no ConceptKey");
        }
    }

    #[test]
    fn an_unknown_token_falls_back_to_itself_rather_than_vanishing() {
        let link = MarkupLink { parent_id: 7, parent_type_id: "future_tool".into(), slot: "future_slot".into() };
        let label = link.label();
        assert!(label.contains("future_tool"), "{label}");
        assert!(label.contains("future_slot"), "{label}");
    }

    #[test]
    fn a_shipped_link_names_both_halves() {
        let link = MarkupLink::new(3, "dealing_range", slots::PREMIUM);
        let label = link.label();
        assert!(label.contains('\u{2014}'), "{label}");
        assert!(!label.contains("dealing_range"), "raw type_id leaked: {label}");
    }

    #[test]
    fn an_unresolved_claim_still_reports_where_it_sits() {
        let link = MarkupLink::new(2, "dealing_range", slots::DISCOUNT);
        let rows = with_link_row(Some(&link), Vec::new());
        assert_eq!(rows.len(), 1, "an empty result block must still carry the link");
        assert_eq!(with_link_row(None, Vec::new()).len(), 0);
    }

    #[test]
    fn no_stat_row_without_a_link() {
        assert!(link_stat_row(None).is_none());
        let link = MarkupLink::new(1, "amd_cycle", slots::MANIPULATION);
        assert!(link_stat_row(Some(&link)).is_some());
    }
}
