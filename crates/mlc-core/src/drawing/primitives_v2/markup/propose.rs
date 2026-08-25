//! Proposal — the engine assembling a setup it found and offering it as a
//! set of claims to draw.
//!
//! # What this adds that the surveys do not
//!
//! `smc_scan` already detects. It paints every sweep, every break, every
//! block it can find over a range, and it paints them as one undifferentiated
//! wash: the analyst still has to read which break belongs to which sweep,
//! and still has to place each claim by hand to get anything the engine can
//! reason about. A survey is opinion-free by design — that is what makes it
//! a survey.
//!
//! What automation adds is the reading. This walks the same detector output
//! and assembles it into CHAINS — a sweep, the shift that confirms it, the
//! origin that shift came from — using the very same grammar
//! ([`super::chain::successor_kinds`]) and the same window
//! (`SmcEngineConfig::chain_window_bars`) the manual pass uses to derive an
//! edge. Materializing one puts three claims on the chart, and the
//! maintaining pass links them without being told to, because they were
//! built to the rule it derives edges by.
//!
//! # Why it proposes only setups that hold
//!
//! A proposal whose own links contradict each other would be the engine
//! asking the analyst to draw something it will immediately flag. So the
//! break must REVERSE the sweep — a sweep that took a high is followed by a
//! bearish shift ([`super::chain::RULE_SMC_CHAIN_DIRECTION`] states the
//! same thing from the other side), and an origin is only attached when the
//! block itself names that break as the one its impulse caused
//! (`RULE_ORDER_BLOCK_OUTSIDE_ITS_BREAK`). The rules and the proposals are
//! two readings of one grammar, and they are not allowed to disagree.
//!
//! # This is last on purpose
//!
//! `docs/mlc/plans/markup-engine-parity-2026-08-25.md` puts automation
//! after everything else, because it is worth nothing on its own: proposing
//! a chain means nothing unless a chain is a thing the engine understands,
//! and that is what S2–S6 built.

use mli::smc as mli_smc;

use crate::state::smc_engine::SmcEngineConfig;
use crate::types::Bar;

/// One claim in a proposal — what to create and on which bar.
///
/// A bar index rather than a timestamp because the whole proposal is
/// computed against one bar slice; the caller converts to timestamps at the
/// point it creates primitives, which is also the only place that knows
/// which series those indices belong to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProposedClaim {
    /// The registry `type_id` to create.
    pub type_id: &'static str,
    /// The GLOBAL bar index this claim anchors on.
    pub bar: usize,
    /// A within-type discriminator the constructor needs — today only
    /// `structure_break`'s own claimed kind
    /// ([`super::chain::kinds`]).
    pub variant: Option<&'static str>,
}

/// One assembled setup, in narrative order.
///
/// Always at least two links (a sweep and the shift that confirms it) and
/// at most three (plus the origin). A sweep with no shift behind it is not
/// a setup, so it is not proposed at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposedChain {
    pub links: Vec<ProposedClaim>,
}

/// Every smart-money setup the detectors support over `bars`.
///
/// Driven by the SHIFTS, not the sweeps, because that is the direction the
/// maintaining pass resolves in: a break takes the nearest earlier sweep,
/// so a proposal that paired it with a further one would be overruled the
/// moment it was drawn. Everything this refuses, it refuses for that one
/// reason — the engine must not draw a chain it would then read
/// differently.
///
/// Refused, and each is a case that actually arises:
///
/// - a shift whose nearest earlier sweep runs the SAME way it does. That is
///   the continuation the sweep was supposed to rule out
///   ([`super::chain::RULE_SMC_CHAIN_DIRECTION`]), and proposing it would be
///   the engine asking for a claim it immediately flags.
/// - a shift whose nearest sweep is already the subject of an accepted
///   chain, or has an accepted claim standing between them. The pass picks
///   by proximity among the claims that EXIST, so a proposal is only honest
///   if nothing it has already accepted would come between.
///
/// The origin is attached only when the block itself names this break as
/// the one its impulse caused — the same fact
/// `RULE_ORDER_BLOCK_OUTSIDE_ITS_BREAK` judges by — and it lands BEHIND the
/// break in time, which is what [`super::chain::Reach::Backward`] is for.
pub fn propose_smc_chains(bars: &[Bar], engine: &SmcEngineConfig) -> Vec<ProposedChain> {
    if bars.is_empty() || engine.chain_window_bars == 0 {
        return Vec::new();
    }
    let window: Vec<mli_smc::SmcBar> = bars
        .iter()
        .map(|b| mli_smc::SmcBar { open: b.open, high: b.high, low: b.low, close: b.close })
        .collect();
    let cfg = engine.to_smc_config();
    let reach = engine.chain_window_bars;

    let sweeps = mli_smc::liquidity_sweeps(&window, &cfg);
    let breaks = mli_smc::structure(&window, &cfg);
    let blocks = mli_smc::order_blocks(&window, &cfg);

    // Every claim accepted so far, by bar — what the pass will actually see
    // when it resolves edges over the drawn set.
    let mut accepted: Vec<usize> = Vec::new();
    let mut out = Vec::new();

    for event in &breaks {
        // The nearest earlier sweep that REVERSES this shift — the setup.
        //
        // Filtered by direction BEFORE taking the nearest, not after. The
        // pass resolves against the claims that EXIST, and a same-direction
        // sweep sitting in between is one this engine simply does not draw,
        // so it cannot come between anything. Testing the nearest sweep
        // overall was the first attempt and it proposed nothing at all on a
        // real daily chart, where sweeps cluster inside the push they
        // belong to.
        let Some(sweep) = sweeps
            .iter()
            .filter(|s| {
                s.index < event.index
                    && event.index - s.index <= reach
                    && s.direction != event.direction
            })
            .max_by_key(|s| s.index)
        else {
            continue;
        };
        if accepted.contains(&sweep.index) {
            continue;
        }
        if accepted.iter().any(|&bar| bar > sweep.index && bar < event.index) {
            continue;
        }

        let mut links = vec![
            ProposedClaim { type_id: "liquidity_sweep", bar: sweep.index, variant: None },
            ProposedClaim {
                type_id: "structure_break",
                bar: event.index,
                variant: Some(match event.kind {
                    mli_smc::StructureEventKind::Choch => super::chain::kinds::CHOCH,
                    mli_smc::StructureEventKind::Bos => super::chain::kinds::BOS,
                }),
            },
        ];
        accepted.push(sweep.index);
        accepted.push(event.index);

        if let Some(block) = blocks
            .iter()
            .find(|b| b.broke_structure_at == event.index && b.index < event.index && !accepted.contains(&b.index))
        {
            links.push(ProposedClaim { type_id: "order_block", bar: block.index, variant: None });
            accepted.push(block.index);
        }

        out.push(ProposedChain { links });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing::primitives_v2::markup::chain;

    fn bar(i: i64, o: f64, h: f64, l: f64, c: f64) -> Bar {
        Bar::new(1_000 + i * 60, o, h, l, c)
    }

    /// A series built to contain exactly one setup, spelled out bar by bar
    /// rather than generated.
    ///
    /// The generated staircase this replaced produced ZERO swings, so every
    /// property test here passed vacuously — they all quantify over
    /// "whatever was found", and nothing was. A fixture for a detector has
    /// to be checked against the detector, which is what
    /// [`a_series_with_setups_yields_at_least_one`] is for.
    ///
    /// The shape, with `swing_n` = 2:
    /// - bar 10 is a swing HIGH at 120 (two lower bars each side)
    /// - bar 12 is a swing LOW at 104
    /// - bar 15 wicks to 123, above the swing high, and closes back under
    ///   it — a liquidity sweep of that high, direction Up
    /// - bar 17 closes at 103, under the swing low — a bearish shift,
    ///   direction Down, which REVERSES the sweep
    fn shaped_bars() -> Vec<Bar> {
        // (high, low, close) per bar; open is the previous close.
        let path: [(f64, f64, f64); 25] = [
            (101.0, 99.0, 100.0),
            (106.0, 100.0, 105.0),
            (111.0, 104.0, 110.0),
            (110.0, 103.0, 104.0),
            (105.0, 97.0, 98.0),
            (107.0, 98.0, 106.0),
            (115.0, 105.0, 114.0),
            (114.0, 107.0, 108.0),
            (108.0, 100.0, 101.0),
            (113.0, 101.0, 112.0),
            (120.0, 111.0, 119.0), // 10 — swing high at 120
            (119.0, 112.0, 113.0),
            (113.0, 104.0, 106.0), // 12 — swing low at 104
            (112.0, 106.0, 111.0),
            (119.0, 109.0, 118.0),
            (123.0, 115.0, 116.0), // 15 — wicks 123 over the high, closes back under
            (116.0, 109.0, 110.0),
            (110.0, 102.0, 103.0), // 17 — closes under 105, the shift
            (104.0, 96.0, 97.0),
            (98.0, 90.0, 91.0),
            (92.0, 86.0, 87.0),
            (88.0, 82.0, 83.0),
            (84.0, 79.0, 80.0),
            (81.0, 76.0, 77.0),
            (78.0, 73.0, 74.0),
        ];
        let mut out = Vec::new();
        let mut open = 100.0;
        for (i, &(high, low, close)) in path.iter().enumerate() {
            out.push(bar(i as i64, open, high, low, close));
            open = close;
        }
        out
    }

    #[test]
    fn a_series_with_setups_yields_at_least_one() {
        let bars = shaped_bars();
        let engine = SmcEngineConfig::default();
        let window: Vec<mli_smc::SmcBar> = bars
            .iter()
            .map(|b| mli_smc::SmcBar { open: b.open, high: b.high, low: b.low, close: b.close })
            .collect();
        let cfg = engine.to_smc_config();
        let sweeps = mli_smc::liquidity_sweeps(&window, &cfg).len();
        let breaks = mli_smc::structure(&window, &cfg).len();
        let proposals = propose_smc_chains(&bars, &engine).len();
        assert!(
            proposals > 0,
            "sweeps={sweeps} breaks={breaks} proposals={proposals} - the engine found nothing to offer",
        );
    }

    #[test]
    fn no_bars_no_proposals() {
        assert!(propose_smc_chains(&[], &SmcEngineConfig::default()).is_empty());
    }

    /// The window is the same switch the manual pass reads, so turning
    /// chaining off must turn proposals off — otherwise the engine would
    /// offer a chain it would then refuse to link.
    #[test]
    fn a_zero_chain_window_proposes_nothing() {
        let cfg = SmcEngineConfig { chain_window_bars: 0, ..SmcEngineConfig::default() };
        assert!(propose_smc_chains(&shaped_bars(), &cfg).is_empty());
    }

    /// Every proposal must be legal under the SAME grammar the maintaining
    /// pass derives edges by — otherwise materializing one would produce
    /// claims that refuse to chain.
    /// Every proposal must be legal under the SAME grammar the maintaining
    /// pass derives edges by, INCLUDING which side of its predecessor each
    /// link sits on — a chain that ran the wrong way in time would be drawn
    /// and then read differently.
    #[test]
    fn every_proposed_link_may_follow_the_one_before_it() {
        for proposal in propose_smc_chains(&shaped_bars(), &SmcEngineConfig::default()) {
            assert!(proposal.links.len() >= 2, "a sweep alone is not a setup");
            for pair in proposal.links.windows(2) {
                let reach = chain::may_follow(pair[0].type_id, pair[1].type_id).unwrap_or_else(|| {
                    panic!("{} cannot be followed by {}", pair[0].type_id, pair[1].type_id)
                });
                match reach {
                    chain::Reach::Forward => assert!(pair[0].bar < pair[1].bar),
                    chain::Reach::Backward => assert!(pair[1].bar < pair[0].bar),
                }
            }
        }
    }

    /// The rule that caught this engine out on the stand: a shift running
    /// the same way as the sweep before it is the continuation that sweep
    /// was supposed to rule out, and the engine must never propose one.
    #[test]
    fn no_proposal_contradicts_the_direction_rule() {
        let bars = shaped_bars();
        let engine = SmcEngineConfig::default();
        let sweeps = {
            let window: Vec<mli_smc::SmcBar> = bars
                .iter()
                .map(|b| mli_smc::SmcBar { open: b.open, high: b.high, low: b.low, close: b.close })
                .collect();
            mli_smc::liquidity_sweeps(&window, &engine.to_smc_config())
        };
        let breaks = {
            let window: Vec<mli_smc::SmcBar> = bars
                .iter()
                .map(|b| mli_smc::SmcBar { open: b.open, high: b.high, low: b.low, close: b.close })
                .collect();
            mli_smc::structure(&window, &engine.to_smc_config())
        };
        for proposal in propose_smc_chains(&bars, &engine) {
            let sweep_bar = proposal.links[0].bar;
            let break_bar = proposal.links[1].bar;
            let sweep = sweeps.iter().find(|s| s.index == sweep_bar).expect("proposed sweep is real");
            let event = breaks.iter().find(|e| e.index == break_bar).expect("proposed break is real");
            assert_ne!(sweep.direction, event.direction, "a proposal must not self-flag");
        }
    }

    /// Two proposals sharing a break would put two claims on one bar, which
    /// the neighbour rule flags as a duplicate the moment they are drawn.
    #[test]
    fn no_two_proposals_share_a_link() {
        let proposals = propose_smc_chains(&shaped_bars(), &SmcEngineConfig::default());
        let mut seen: Vec<(&str, usize)> = Vec::new();
        for p in &proposals {
            for link in &p.links {
                let key = (link.type_id, link.bar);
                assert!(!seen.contains(&key), "{} at bar {} proposed twice", key.0, key.1);
                seen.push(key);
            }
        }
    }

    /// A break is only proposed with the kind its own detector reported —
    /// the claim the analyst would otherwise have to pick by hand, and the
    /// one `choch_after_same_direction_break` judges.
    #[test]
    fn a_proposed_break_carries_a_claimed_kind() {
        for p in propose_smc_chains(&shaped_bars(), &SmcEngineConfig::default()) {
            for link in p.links.iter().filter(|l| l.type_id == "structure_break") {
                assert!(
                    matches!(link.variant, Some(chain::kinds::BOS) | Some(chain::kinds::CHOCH)),
                    "a break must name its kind, got {:?}",
                    link.variant,
                );
            }
        }
    }

    /// The proposal window is bounded by the engine's own setting, so a
    /// wider window can only ever find at least as much as a narrower one.
    #[test]
    fn a_wider_window_never_finds_less() {
        let bars = shaped_bars();
        let narrow = SmcEngineConfig { chain_window_bars: 3, ..SmcEngineConfig::default() };
        let wide = SmcEngineConfig { chain_window_bars: 30, ..SmcEngineConfig::default() };
        assert!(propose_smc_chains(&bars, &wide).len() >= propose_smc_chains(&bars, &narrow).len());
    }
}
