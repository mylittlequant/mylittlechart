//! What the three markup engines share: the rule-family switches, and one
//! place to read them from whichever family a claim belongs to.
//!
//! # Why a shared struct and not three copies
//!
//! Elliott's [`super::ewa_engine::GuardFamilies`] classes its own rules by
//! what they are ABOUT — geometry, grammar, chain. The other three families
//! split the same way, and they split identically, because the split is not
//! about Elliott: a rule is either about the bars under one claim, about a
//! neighbour, or about the claim before it in the story. Three copies of
//! two booleans would only be three places to forget.
//!
//! # Why two switches and not three
//!
//! The third class — a claim's own detector rules, "the bars disagree with
//! what you asserted here" — already has a switch. Every one of these
//! engines shipped with `judge_claims` long before this struct existed, and
//! it gates exactly that class and nothing else. Adding a `detector` field
//! beside it would be a second switch for one thing, and two switches for
//! one thing is how they end up disagreeing.
//!
//! So: `judge_claims` is the detector family, under an older name.
//! [`MarkupGuardFamilies`] carries the two that had no switch at all.

use serde::{Deserialize, Serialize};

use crate::drawing::primitives_v2::markup::MarkupFamily;

/// The two cross-primitive rule classes a markup engine can silence
/// independently.
///
/// Both default on: a rule that was never asked about should report, and a
/// saved profile from a build before these existed reads as "everything
/// on", which is what that build did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkupGuardFamilies {
    /// Rules about a claim's NEIGHBOURS — two claims of one kind on one
    /// bar (`overlapping_claim`) and anything else that compares a claim
    /// with the markup drawn beside it.
    #[serde(default = "default_true")]
    pub neighbour: bool,
    /// Rules about the claim BEFORE this one in its family's narrative —
    /// `smc_chain_direction`, `choch_after_same_direction_break`,
    /// `order_block_outside_its_break` (see
    /// `drawing::primitives_v2::markup::chain`).
    #[serde(default = "default_true")]
    pub chain: bool,
}

fn default_true() -> bool {
    true
}

impl Default for MarkupGuardFamilies {
    fn default() -> Self {
        Self { neighbour: true, chain: true }
    }
}

/// The switches currently in force for `family` — read FRESH on every use,
/// never cached, the same contract `ewa_engine::guard_families` readers
/// hold to. Each family answers from its own engine config, so silencing
/// order flow's chain rules leaves smart money's alone.
pub fn guard_families_for(family: MarkupFamily) -> MarkupGuardFamilies {
    match family {
        MarkupFamily::SmartMoney => super::smc_engine::smc_engine_config().guard_families,
        MarkupFamily::OrderFlow => super::orderflow_engine::orderflow_engine_config().guard_families,
        MarkupFamily::CandlePattern => {
            super::candle_pattern_engine::candle_pattern_engine_config().guard_families
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_reports_until_it_is_switched_off() {
        let d = MarkupGuardFamilies::default();
        assert!(d.neighbour);
        assert!(d.chain);
    }

    /// A profile written before these switches existed carries neither
    /// field, and must read as the behaviour that build actually had.
    #[test]
    fn an_older_profile_reads_as_everything_on() {
        let parsed: MarkupGuardFamilies = serde_json::from_str("{}").expect("empty object parses");
        assert_eq!(parsed, MarkupGuardFamilies::default());
    }

    #[test]
    fn each_family_answers_from_its_own_engine() {
        // Defaults across the board; the point of the assertion is that
        // every family resolves to a config at all rather than panicking or
        // borrowing another family's.
        for family in [MarkupFamily::SmartMoney, MarkupFamily::OrderFlow, MarkupFamily::CandlePattern] {
            let f = guard_families_for(family);
            assert!(f.neighbour && f.chain, "{family:?} starts with both classes reporting");
        }
    }
}
