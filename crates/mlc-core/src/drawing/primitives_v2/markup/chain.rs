//! Succession — the second relation between two markup claims, and the
//! grammar that says which claim may follow which.
//!
//! Containment ([`super::link`]) asks "what am I inside of". Succession asks
//! "what came before me in the story". Elliott keeps the two apart for the
//! same reason (`elliott::count`'s own module doc: `link` is subdivision,
//! `follows` is succession), and smart money needs both for the same
//! reason: an order block sits INSIDE a dealing range and FOLLOWS the
//! structure break it originated.
//!
//! ## The narrative is a grammar
//!
//! The smart-money setup is a sequence traders actually draw:
//!
//! ```text
//! liquidity sweep -> structure shift -> origin (order block / gap) -> mitigation
//! ```
//!
//! A sweep takes a pool; the shift says the sweep worked; the origin is
//! where the impulse that shifted structure began. [`successor_kinds`] is
//! that sequence as a table, and it is what makes a chain-level rule
//! possible at all — without it, "the claim before this one" is whatever
//! happens to be nearest, and a rule about the pair means nothing.
//!
//! ## Why succession is derived, not declared
//!
//! Elliott's `follows` is declared: the analyst builds a count by saying
//! this correction continues that impulse, and nothing but the analyst
//! knows which. Smart money is not like that. A break that lands eleven
//! bars after a sweep of the same pool IS the shift that sweep predicted —
//! there is nothing left for the analyst to declare, and asking for a
//! declaration would only be a way to get it wrong. So the maintaining pass
//! derives the edge the same way it derives containment: the nearest claim
//! the grammar allows, inside the engine's own chain window.
//!
//! One rule from the plan is deliberately NOT shipped because of that.
//! `smc_chain_continuity` — "the predecessor's bar is at or before this
//! claim's, within the chain window" — cannot fire when the pass only ever
//! links predecessors that are earlier and in-window. A rule that cannot
//! fire is not a safety net, it is a line of code that looks like one.
//!
//! ## What each family contributes
//!
//! The rules need facts the claims already resolved — a direction, a
//! BOS/CHoCH kind, the bar an origin says it came from. Those arrive as one
//! [`ChainFact`] per claim ([`super::super::Primitive::chain_fact`]) rather
//! than as a trait method per rule, so adding a rule is a change to this
//! file and nothing else.

use serde::{Deserialize, Serialize};

use super::super::config::StatRow;
use super::super::elliott::guard::Violation;
use crate::i18n::{current_language, ConceptKey, PrimitiveNameKey};

/// A claim's own resolved direction, as its detector reports it.
///
/// Geometric, not interpretive: [`MarkupBias::Up`] on a liquidity sweep
/// means a swing HIGH was taken, and on a structure break means a high was
/// broken. Whether taking a high is bullish or bearish is a reading, and
/// readings belong in the rules ([`chain_violations`]) where they can be
/// stated in full — not baked into the name of a value every claim has to
/// map itself onto.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarkupBias {
    Up,
    Down,
}

impl From<mli::smc::Direction> for MarkupBias {
    fn from(d: mli::smc::Direction) -> Self {
        match d {
            mli::smc::Direction::Up => Self::Up,
            mli::smc::Direction::Down => Self::Down,
        }
    }
}

/// What one claim contributes to its family's own chain rules.
///
/// One struct rather than a trait method per rule: every field here is
/// something the claim ALREADY resolved for its own render, so gathering
/// them costs nothing, and a new rule becomes a change to
/// [`chain_violations`] alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChainFact {
    /// The claim's own anchor bar, as a GLOBAL index.
    pub bar: usize,
    /// The direction the claim resolved, if it resolved one at all.
    pub bias: Option<MarkupBias>,
    /// A within-type discriminator the rules need — today only
    /// `structure_break`'s own [`kinds::BOS`]/[`kinds::CHOCH`].
    pub variant: Option<&'static str>,
    /// For a claim that says it ORIGINATED an earlier event: that event's
    /// own bar. An order block resolves the structure break its impulse
    /// caused; `order_block_outside_its_break` is the rule that checks the
    /// analyst chained it to that break and not another one.
    pub origin_event_bar: Option<usize>,
}

/// [`ChainFact::variant`] tokens, in one place for the same reason
/// [`super::link::slots`] exists.
pub mod kinds {
    pub const BOS: &str = "bos";
    pub const CHOCH: &str = "choch";
}

/// What may follow `type_id` in its own family's narrative — the
/// `successor_kinds` analogue for the non-Elliott families.
///
/// Empty for anything that opens nothing and continues nothing, which is
/// how a survey, a container and every drawing outside these families all
/// answer.
pub fn successor_kinds(type_id: &str) -> &'static [&'static str] {
    match type_id {
        // Smart money, in narrative order. A sweep predicts a shift; a
        // shift has an origin; an origin is either raided again or carried
        // further by the next break.
        "liquidity_sweep" => &["structure_break"],
        "structure_break" => &["order_block", "fair_value_gap"],
        "order_block" | "fair_value_gap" => &["liquidity_sweep", "structure_break"],
        // The cycle's own manipulation phase IS a sweep — the coarser claim
        // opens the finer narrative rather than sitting inside it.
        "amd_cycle" => &["liquidity_sweep"],

        // Order flow's exhaustion narrative, one scale down and two links
        // shorter: an imbalance is absorbed, and the effort/result split
        // shows up as a delta divergence after it.
        "imbalance_zone" => &["absorption_mark"],
        "absorption_mark" => &["delta_divergence"],

        _ => &[],
    }
}

/// Whether `next_type` may follow `prev_type`.
pub fn may_follow(prev_type: &str, next_type: &str) -> bool {
    successor_kinds(prev_type).contains(&next_type)
}

// =========================================================================
// Rule ids
// =========================================================================

/// A sweep and the structure break that follows it point the same way.
///
/// The setup says otherwise, and says it plainly: price wicks ABOVE a swing
/// high to take the stops resting there, then turns and breaks structure
/// DOWNWARD. That reversal is the whole claim. A sweep of a high followed
/// by a bullish break is not a weaker version of the setup — it is the
/// continuation the sweep was supposed to rule out.
pub const RULE_SMC_CHAIN_DIRECTION: &str = "smc_chain_direction";

/// A CHoCH claimed after a break that already went the same way.
///
/// A change of character is by definition the FIRST break against the
/// prevailing direction. A second one the same way is a BOS — a break of
/// structure continuing the move — and calling it a CHoCH loses the only
/// thing the distinction carries.
pub const RULE_CHOCH_AFTER_SAME_DIRECTION_BREAK: &str = "choch_after_same_direction_break";

/// An origin chained to a break it did not cause.
///
/// An order block is the last opposing candle before the impulse that broke
/// structure, so the block itself resolves WHICH break that was. If the
/// analyst chained it to a different one, one of the two is misplaced.
pub const RULE_ORDER_BLOCK_OUTSIDE_ITS_BREAK: &str = "order_block_outside_its_break";

// =========================================================================
// The rules
// =========================================================================

/// Every chain violation `own` carries because of the claim it follows.
///
/// Pairwise where the relation is pairwise, and reaching further back only
/// where the rule genuinely is about a further-back claim:
/// `choch_after_same_direction_break` compares two BREAKS, which in a full
/// narrative are two links apart (break -> origin -> break), so it is given
/// the chain's own earlier breaks rather than only the immediate
/// predecessor.
///
/// `prior_breaks` is every `structure_break` earlier in this claim's own
/// chain, nearest last. Empty for any claim that is not a break itself.
pub fn chain_violations(
    own_type: &str,
    own: &ChainFact,
    prev_type: &str,
    prev: &ChainFact,
    prior_breaks: &[ChainFact],
) -> Vec<Violation> {
    let mut out = Vec::new();

    if own_type == "structure_break" && prev_type == "liquidity_sweep" {
        if let (Some(a), Some(b)) = (own.bias, prev.bias) {
            if a == b {
                out.push(Violation::whole(
                    RULE_SMC_CHAIN_DIRECTION,
                    ConceptKey::RuleSmcChainDirection,
                ));
            }
        }
    }

    if own_type == "structure_break" && own.variant == Some(kinds::CHOCH) {
        if let (Some(bias), Some(last)) = (own.bias, prior_breaks.last()) {
            if last.bias == Some(bias) {
                out.push(Violation::whole(
                    RULE_CHOCH_AFTER_SAME_DIRECTION_BREAK,
                    ConceptKey::RuleChochAfterSameDirectionBreak,
                ));
            }
        }
    }

    if own_type == "order_block" && prev_type == "structure_break" {
        if let Some(origin) = own.origin_event_bar {
            if origin != prev.bar {
                out.push(Violation::whole(
                    RULE_ORDER_BLOCK_OUTSIDE_ITS_BREAK,
                    ConceptKey::RuleOrderBlockOutsideItsBreak,
                ));
            }
        }
    }

    out
}

/// This claim's own edge backward: the claim it continues.
///
/// Carries the predecessor's `type_id` for the same reason
/// [`super::link::MarkupLink`] carries the parent's — the Stat row that
/// renders it is built by the claim alone, and a primitive cannot see its
/// siblings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MarkupFollows {
    /// The predecessor's own [`super::super::PrimitiveData::id`].
    pub prev_id: u64,
    /// The predecessor's own `type_id`, for naming it without a lookup.
    pub prev_type_id: String,
}

impl MarkupFollows {
    pub fn new(prev_id: u64, prev_type_id: &str) -> Self {
        Self { prev_id, prev_type_id: prev_type_id.to_string() }
    }
}

/// The Stat row a chained claim shows — the succession counterpart of
/// [`super::link::link_stat_row`], and named for what the relation IS:
/// this claim continues that one.
pub fn follows_stat_row(follows: Option<&MarkupFollows>) -> Option<StatRow> {
    let lang = current_language();
    let follows = follows?;
    let name = PrimitiveNameKey::from_type_id(&follows.prev_type_id)
        .map(|k| k.get(lang).to_string())
        .unwrap_or_else(|| follows.prev_type_id.clone());
    Some(StatRow::new(ConceptKey::MarkupFollowsLabel.get(lang), &name, None))
}

/// `rows` with the succession row in front of it, or `rows` untouched when
/// this claim continues nothing — the counterpart of
/// [`super::link::with_link_row`], and applied AFTER it so a claim showing
/// both reads "inside X, follows Y" top-down.
pub fn with_follows_row(follows: Option<&MarkupFollows>, rows: Vec<StatRow>) -> Vec<StatRow> {
    match follows_stat_row(follows) {
        Some(row) => std::iter::once(row).chain(rows).collect(),
        None => rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(bar: usize, bias: Option<MarkupBias>) -> ChainFact {
        ChainFact { bar, bias, variant: None, origin_event_bar: None }
    }

    fn ids(v: &[Violation]) -> Vec<&'static str> {
        v.iter().map(|x| x.rule_id).collect()
    }

    #[test]
    fn the_grammar_is_the_setup_read_forwards() {
        assert!(may_follow("liquidity_sweep", "structure_break"));
        assert!(may_follow("structure_break", "order_block"));
        assert!(may_follow("order_block", "liquidity_sweep"));
        assert!(!may_follow("liquidity_sweep", "order_block"), "the shift is not optional");
        assert!(!may_follow("order_block", "order_block"));
    }

    #[test]
    fn a_survey_opens_no_narrative() {
        assert!(successor_kinds("smc_scan").is_empty());
        assert!(successor_kinds("trend_line").is_empty());
    }

    /// A sweep of a HIGH must be followed by a break DOWN.
    #[test]
    fn a_break_agreeing_with_the_sweep_it_follows_is_flagged() {
        let v = chain_violations(
            "structure_break",
            &fact(20, Some(MarkupBias::Up)),
            "liquidity_sweep",
            &fact(14, Some(MarkupBias::Up)),
            &[],
        );
        assert!(ids(&v).contains(&RULE_SMC_CHAIN_DIRECTION));
    }

    #[test]
    fn a_break_reversing_the_sweep_it_follows_is_the_setup() {
        let v = chain_violations(
            "structure_break",
            &fact(20, Some(MarkupBias::Down)),
            "liquidity_sweep",
            &fact(14, Some(MarkupBias::Up)),
            &[],
        );
        assert!(v.is_empty(), "{:?}", ids(&v));
    }

    /// Nothing resolved means nothing to disagree about — an unresolved
    /// claim already carries its own guard violation, and stacking a chain
    /// rule on top would report the same ignorance twice.
    #[test]
    fn an_unresolved_claim_reports_no_chain_disagreement() {
        let v = chain_violations(
            "structure_break",
            &fact(20, None),
            "liquidity_sweep",
            &fact(14, Some(MarkupBias::Up)),
            &[],
        );
        assert!(v.is_empty());
    }

    #[test]
    fn a_choch_repeating_the_previous_breaks_direction_is_flagged() {
        let mut own = fact(30, Some(MarkupBias::Down));
        own.variant = Some(kinds::CHOCH);
        let v = chain_violations(
            "structure_break",
            &own,
            "order_block",
            &fact(24, Some(MarkupBias::Down)),
            &[fact(18, Some(MarkupBias::Down))],
        );
        assert!(ids(&v).contains(&RULE_CHOCH_AFTER_SAME_DIRECTION_BREAK));
    }

    #[test]
    fn a_choch_against_the_previous_break_is_exactly_what_a_choch_is() {
        let mut own = fact(30, Some(MarkupBias::Up));
        own.variant = Some(kinds::CHOCH);
        let v = chain_violations(
            "structure_break",
            &own,
            "order_block",
            &fact(24, Some(MarkupBias::Up)),
            &[fact(18, Some(MarkupBias::Down))],
        );
        assert!(v.is_empty(), "{:?}", ids(&v));
    }

    /// A BOS repeating the direction is the definition of a BOS, not a
    /// mistake — the rule is about the CLAIMED kind, never the geometry.
    #[test]
    fn a_bos_repeating_the_previous_breaks_direction_is_not_flagged() {
        let mut own = fact(30, Some(MarkupBias::Down));
        own.variant = Some(kinds::BOS);
        let v = chain_violations(
            "structure_break",
            &own,
            "order_block",
            &fact(24, Some(MarkupBias::Down)),
            &[fact(18, Some(MarkupBias::Down))],
        );
        assert!(v.is_empty(), "{:?}", ids(&v));
    }

    #[test]
    fn an_origin_chained_to_a_break_it_did_not_cause_is_flagged() {
        let mut own = fact(25, Some(MarkupBias::Up));
        own.origin_event_bar = Some(31);
        let v = chain_violations("order_block", &own, "structure_break", &fact(28, Some(MarkupBias::Up)), &[]);
        assert!(ids(&v).contains(&RULE_ORDER_BLOCK_OUTSIDE_ITS_BREAK));
    }

    #[test]
    fn an_origin_chained_to_its_own_break_is_silent() {
        let mut own = fact(25, Some(MarkupBias::Up));
        own.origin_event_bar = Some(28);
        let v = chain_violations("order_block", &own, "structure_break", &fact(28, Some(MarkupBias::Up)), &[]);
        assert!(v.is_empty(), "{:?}", ids(&v));
    }

    #[test]
    fn a_chained_claim_names_what_it_continues() {
        assert!(follows_stat_row(None).is_none());
        let follows = MarkupFollows::new(4, "liquidity_sweep");
        let row = follows_stat_row(Some(&follows)).expect("a chained claim has a row");
        assert!(!row.value.contains("liquidity_sweep"), "raw type_id leaked: {}", row.value);
        assert_eq!(with_follows_row(Some(&follows), Vec::new()).len(), 1);
        assert_eq!(with_follows_row(None, Vec::new()).len(), 0);
    }
}
