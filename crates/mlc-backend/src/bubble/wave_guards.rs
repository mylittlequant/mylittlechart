//! Elliott wave-link integrity sweep + guard-cache refresh, scoped to a
//! single [`DataBubble`] — no [`crate::drawing::DrawingManager`] instance,
//! no [`crate::bubble::BubbleStore`] lookup, just `&mut DataBubble`.
//!
//! # Why this lives here and not on `DrawingManager` (2026-08-14 report,
//! second attempt)
//!
//! The first attempt at this fix (commit `d36e6ae5`) called
//! `DrawingManager::{clear_dangling_wave_links, refresh_wave_guards}` from
//! ONE call site — a sweep added to `mlc-app`'s `apply_preset_cold`, right
//! after `BubbleStore::restore_from_snapshots`. The owner measured it still
//! broken on a plain wasm page reload and asked for the path that
//! `apply_preset_cold`'s sweep was missing. Rather than chase that specific
//! path, this moves the sweep DOWN into [`super::store::BubbleStore::
//! restore_from_snapshots`] itself — the doc on that function is explicit
//! that it is "Private — the only caller is `Self::restore_from_snapshots`.
//! External callers use that instead" for its own `insert_bubble_raw`
//! helper, and grepping the whole workspace for `restore_from_snapshots`
//! confirms it: turning a `Vec<DataBubbleSnapshot>` (the on-disk/on-wire
//! shape) back into live bubbles has exactly one production entry point.
//! Any restore path — `apply_preset_cold` today, a worker-thread
//! `hydrate_preset` tomorrow (the scaffold in `mlc-app/src/persistence/
//! hydration.rs`), or anything else that ever needs to rebuild a
//! `BubbleStore` from snapshots — is REQUIRED to call THIS function to get
//! primitives back at all, so a sweep placed inside it cannot be skipped by
//! a caller that doesn't know to run a separate app-level step. That is why
//! this logic is bubble-scoped rather than manager-scoped: `DrawingManager`
//! is exactly the thing `restore_from_snapshots` never touches (see that
//! function's own doc — it "builds bubbles straight on `BubbleStore`,
//! never through `DrawingManager`").
//!
//! `DrawingManager::{clear_dangling_wave_links, refresh_wave_guards}` still
//! exist, as thin `bid`-resolving wrappers around the two functions here —
//! every live-editing call site (`add_external`, `end_drag`, `insert_at`,
//! …) already holds a `bid` + `&mut BubbleStore`, not a bare
//! `&mut DataBubble`, so those wrappers stay the right shape for them.
//!
//! `find_index_by_id`/`chain_head_id`/`chain_member_ids` below are
//! deliberately NOT the same functions `DrawingManager` uses internally
//! (tens of call sites there, unrelated to this sweep) — small, stable,
//! self-contained copies, kept here so this module never has to reach back
//! into `drawing::manager` and re-introduce the same one-call-site fragility
//! this file exists to close.

use std::collections::HashSet;

use mlc_core::drawing::primitives_v2::elliott::{
    chain_violations, label_collision_indices, violations_in, Violation, WaveSlot,
};
use mlc_core::drawing::primitives_v2::Primitive;

use super::data_bubble::DataBubble;

fn find_index_by_id(b: &DataBubble, prim_id: u64) -> Option<usize> {
    b.primitives.iter().position(|p| p.data().id == prim_id)
}

/// Walk `wave_follows` BACKWARD from `prim_id` to the chain's own head —
/// mirrors `DrawingManager::chain_head_id`'s own doc.
fn chain_head_id(b: &DataBubble, prim_id: u64) -> u64 {
    let mut current = prim_id;
    let mut visited: HashSet<u64> = HashSet::new();
    while visited.insert(current) {
        let Some(p) = b.primitives.iter().find(|p| p.data().id == current) else { break };
        match p.wave_follows() {
            Some(prev_id) if b.primitives.iter().any(|q| q.data().id == prev_id) => current = prev_id,
            _ => break,
        }
    }
    current
}

/// Walk `wave_follows` FORWARD from `first_id` — mirrors
/// `DrawingManager::chain_member_ids`'s own doc.
fn chain_member_ids(b: &DataBubble, first_id: u64) -> Vec<u64> {
    let mut ids = Vec::new();
    let mut current = first_id;
    let mut visited: HashSet<u64> = HashSet::new();
    while visited.insert(current) {
        if find_index_by_id(b, current).is_none() {
            break;
        }
        ids.push(current);
        let next = b.primitives.iter()
            .find(|p| p.data().id != current && p.wave_follows() == Some(current))
            .map(|p| p.data().id);
        match next {
            Some(next_id) => current = next_id,
            None => break,
        }
    }
    ids
}

/// Clear any `WaveLink`/`follows` reference in `b`'s own primitive list that
/// no longer resolves within that same collection.
///
/// `WaveLink::parent_id` and `wave_follows` name a sibling in the SAME
/// bubble — never a cross-bubble or cross-store reference (see
/// `mlc_core::drawing::primitives_v2::elliott::WaveLink`'s own doc: "a
/// primitive cannot see its siblings, so it stores only this reference,
/// never the parent itself"). A restored primitive normally keeps that
/// reference intact — every external-insertion path either preserves the
/// incoming id (`DrawingManager::add_external`/`swap_symbol`) or, when ids
/// MUST change on a copy, is responsible for rewriting `link`/`follows`
/// through the old→new map itself
/// (`mlc_core::drawing::reassign_primitive_ids_remapped`). A file saved
/// before that doctrine existed, or hand-edited, can still carry an id
/// nothing in this bubble owns any more — that reference is not
/// recoverable by guessing which primitive it used to name, so it is
/// DROPPED here rather than left dangling: the alternative is a
/// subdivision/chain link [`refresh_wave_guards`] (which resolves children
/// by `wave_link().parent_id`) silently skips forever, holding a parent's
/// own leg drawn under a subdivision that no longer claims it (owner report
/// 2026-08-14, WaveLink cold-restore audit).
///
/// Returns the number of primitives whose `link` and/or `follows` was
/// cleared.
pub(crate) fn clear_dangling_wave_links(b: &mut DataBubble) -> u32 {
    let ids: HashSet<u64> = b.primitives.iter().map(|p| p.data().id).collect();
    let mut cleared = 0u32;
    for p in b.primitives.iter_mut() {
        let mut touched = false;
        if let Some(link) = p.wave_link() {
            if !ids.contains(&link.parent_id) {
                p.set_wave_link(None);
                touched = true;
            }
        }
        if let Some(follows) = p.wave_follows() {
            if !ids.contains(&follows) {
                p.set_wave_follows(None);
                touched = true;
            }
        }
        if touched {
            cleared += 1;
        }
    }
    cleared
}

/// Recompute every Elliott primitive's own SIBLING-dependent violations,
/// subdivided-slot list, and label-collision list for the whole bubble and
/// cache them — mirrors `DrawingManager::refresh_wave_guards`'s own doc
/// (E2/guard follow-up, 2026-08-13): two passes on purpose (immutable
/// compute, then mutable write-back — `elliott::guard::violations_in` needs
/// to see every sibling at once), every Elliott primitive is written even
/// when the result is now empty (a cured violation must clear its stale
/// cache, not keep the last non-empty one forever).
pub(crate) fn refresh_wave_guards(b: &mut DataBubble) {
    let mut updates: Vec<(usize, Vec<Violation>)> = {
        let refs: Vec<&dyn Primitive> = b.primitives.iter().map(|p| p.as_ref()).collect();
        refs.iter()
            .enumerate()
            .filter(|(_, p)| p.wave_degree().is_some())
            .map(|(i, &p)| (i, violations_in(p, &refs)))
            .collect()
    };
    // The chain's OWN rules, merged into the same cache — see
    // `DrawingManager::refresh_wave_guards`'s own doc for the full
    // rationale (a `follows` chain is one coarser structure written as
    // separate primitives; `ChainViolation` carries a member index because
    // the violation is reported ON the member that owns the offending
    // wave).
    let chain_updates: Vec<(usize, Violation)> = {
        let mut out = Vec::new();
        let mut seen_heads: HashSet<u64> = HashSet::new();
        let member_ids: Vec<u64> = b
            .primitives
            .iter()
            .filter(|p| p.wave_degree().is_some())
            .map(|p| p.data().id)
            .collect();
        for id in member_ids {
            let head = chain_head_id(b, id);
            if !seen_heads.insert(head) {
                continue;
            }
            let ids = chain_member_ids(b, head);
            if ids.len() < 2 {
                continue;
            }
            let idxs: Vec<usize> = ids.iter().filter_map(|id| find_index_by_id(b, *id)).collect();
            if idxs.len() != ids.len() {
                continue;
            }
            let chain: Vec<&dyn Primitive> = idxs.iter().map(|&i| b.primitives[i].as_ref()).collect();
            for v in chain_violations(&chain) {
                if let Some(&idx) = idxs.get(v.member_index) {
                    out.push((idx, Violation { rule_id: v.rule_id, wave: Some(v.wave), message: v.message }));
                }
            }
        }
        out
    };
    for (idx, violation) in chain_updates {
        if let Some(slot) = updates.iter_mut().find(|(i, _)| *i == idx) {
            slot.1.push(violation);
        }
    }
    for (i, violations) in updates {
        b.primitives[i].set_context_violations(violations);
    }

    // Which of each structure's own waves are subdivided — the render side
    // reads this to stop drawing the leg a subdivision replaced.
    let subdivided: Vec<(usize, Vec<WaveSlot>)> = {
        let ids: Vec<(usize, u64)> = b
            .primitives
            .iter()
            .enumerate()
            .filter(|(_, p)| p.wave_degree().is_some())
            .map(|(i, p)| (i, p.data().id))
            .collect();
        ids.into_iter()
            .map(|(i, id)| {
                let slots = b
                    .primitives
                    .iter()
                    .filter_map(|p| p.wave_link().filter(|l| l.parent_id == id).map(|l| l.slot))
                    .collect();
                (i, slots)
            })
            .collect()
    };
    // Where another degree's mark really lands on this structure's own
    // pivots — see `DrawingManager::refresh_wave_guards`'s own doc (owner
    // report 2026-08-14) for why this is computed here rather than by the
    // parent or the child alone.
    let collisions: Vec<(usize, Vec<usize>)> = {
        let ids: Vec<(usize, u64)> = b
            .primitives
            .iter()
            .enumerate()
            .filter(|(_, p)| p.wave_degree().is_some())
            .map(|(i, p)| (i, p.data().id))
            .collect();
        ids.into_iter()
            .map(|(i, id)| {
                let children: Vec<(&dyn Primitive, WaveSlot)> = b
                    .primitives
                    .iter()
                    .filter_map(|p| {
                        p.wave_link()
                            .filter(|l| l.parent_id == id)
                            .map(|l| (p.as_ref(), l.slot))
                    })
                    .collect();
                (i, label_collision_indices(b.primitives[i].as_ref(), &children))
            })
            .collect()
    };
    for (i, indices) in collisions {
        b.primitives[i].set_label_collisions(indices);
    }

    for (i, slots) in subdivided {
        b.primitives[i].set_subdivided_slots(slots);
    }
}
