//! Shared "which wave is under this click" resolution for
//! `Primitive::wave_at` (owner ask 2026-08-13: double-click a wave opens
//! "subdivide this wave into…", double-click a structure's own last point
//! opens "what continues this" — the input layer needs a single answer to
//! "which wave did this click land on", combining every part of a wave a
//! user actually perceives as belonging to it). One shared body, called by
//! each of the five Elliott types' own `wave_at` override rather than five
//! copies of the same three-way candidate search.

use super::count::{label_slot_for_index, slot_span, WaveSlot};
use super::ratio::{self, Rib};
use super::super::config::LabelStyle;
use super::super::{estimate_text_width, label_plate_rect, wave_label_offset, HIT_TOLERANCE};

/// The [`WaveSlot`] under `(sx, sy)` in screen space, or `None` — combining
/// all three things a user perceives as "this wave":
///
/// - its own BODY SPAN — [`slot_span`]'s own two anchors for the slot,
///   tested as ONE straight cord `screen[a]..screen[b]` regardless of how
///   many interior legs that span covers. For a combination's own
///   multi-leg component slot (W/Y/Z) this is the FULL span, never a
///   single `points[i]..points[i+1]` leg — `wave_slots` names every slot to
///   check (the SAME list [`crate::drawing::primitives_v2::Primitive::wave_slots`]
///   already returns per type, never a second copy of it), [`slot_span`]
///   names each one's own `(start, end)` pair, exactly the lookup every
///   Elliott type's own `render`/`hit_test` already use for the same
///   segments.
/// - its own LABEL PLATE — the SAME rectangle `render`'s own label loop
///   draws ([`label_plate_rect`], fed [`wave_label_offset`]'s own anchor,
///   computed from the SAME `label_collisions` cache the structure renders
///   with — see `elliott::count::label_collision_indices`'s own doc for
///   the rule that fills it — and [`estimate_text_width`]'s own estimate,
///   see those functions' own docs for why the estimate exists), resolved
///   back to a slot via [`label_slot_for_index`].
/// - (while `show_ratios` is on) a RATIO-RIB segment — `ratio_ribs`'s own
///   drawn same-parity polyline segments, each resolved to the wave its
///   LATER endpoint measures: an EXACT match (the endpoint IS that slot's
///   own [`slot_span`] end) wins over an INTERIOR match (the endpoint
///   merely falls inside that slot's own span) — see [`rib_segment_wave`]'s
///   own doc. A rib segment's own two endpoints are almost never a single
///   wave's own two anchors, since a rib threads same-parity vertices
///   across MULTIPLE waves (`elliott::ratio`'s own module doc explains why
///   it is drawn that way at all) — this is genuinely separate geometry
///   from the body-span candidate above, not a second test of the same
///   line.
///
/// When more than one candidate matches, the NEAREST one wins. A
/// label-plate hit (the click landing literally inside its own rectangle)
/// always counts as distance `0.0`, so it outranks a merely-nearby line —
/// being INSIDE a label is a stronger signal than being close to one.
///
/// `realized` gates every candidate to the primitive's own REALIZED prefix
/// (see `Primitive::realized_point_count`'s own doc) — a not-yet-drawn wave
/// is not clickable. Tolerance is the SAME [`HIT_TOLERANCE`] every Elliott
/// type's own `hit_test` already uses for its line checks.
pub(super) fn wave_slot_at(
    sx: f64,
    sy: f64,
    type_id: &str,
    screen: &[(f64, f64)],
    realized: usize,
    wave_slots: &[WaveSlot],
    labels: &[&str],
    label_style: &LabelStyle,
    label_collisions: &[usize],
    show_labels: bool,
    show_ratios: bool,
    ratio_ribs: &[Rib],
) -> Option<WaveSlot> {
    let mut best: Option<(f64, WaveSlot)> = None;
    let mut offer = |dist: f64, slot: WaveSlot| {
        if dist <= HIT_TOLERANCE && best.is_none_or(|(d, _)| dist < d) {
            best = Some((dist, slot));
        }
    };

    // a) Each wave's own body span.
    for &slot in wave_slots {
        if let Some((a, b)) = slot_span(type_id, slot) {
            if b < realized {
                if let (Some(&(x1, y1)), Some(&(x2, y2))) = (screen.get(a), screen.get(b)) {
                    offer(ratio::point_to_line_dist(sx, sy, x1, y1, x2, y2), slot);
                }
            }
        }
    }

    // b) Each wave's own label plate.
    if show_labels {
        for (i, &(x, y)) in screen.iter().enumerate() {
            if i >= realized || i >= labels.len() || labels[i].is_empty() {
                continue;
            }
            let Some(slot) = label_slot_for_index(type_id, i) else { continue };
            let collides = label_collisions.contains(&i);
            let (dx, dy) = wave_label_offset(screen, i, label_style, collides);
            let (label_x, label_y) = (x + dx, y + dy);
            let width = estimate_text_width(labels[i], label_style.font_size);
            let (bg_x, bg_y, bg_w, bg_h) =
                label_plate_rect(label_x, label_y, width, label_style.font_size, label_style.background_padding);
            if sx >= bg_x && sx <= bg_x + bg_w && sy >= bg_y && sy <= bg_y + bg_h {
                offer(0.0, slot);
            }
        }
    }

    // c) Ratio-rib segments — only while `show_ratios` is on (see this
    // function's own doc).
    if show_ratios {
        for rib in ratio_ribs {
            for pair in rib.windows(2) {
                let (Some(&(x1, y1)), Some(&(x2, y2))) = (screen.get(pair[0]), screen.get(pair[1])) else {
                    continue;
                };
                let dist = ratio::point_to_line_dist(sx, sy, x1, y1, x2, y2);
                if dist > HIT_TOLERANCE {
                    continue;
                }
                if let Some(slot) = rib_segment_wave(type_id, wave_slots, pair[1]) {
                    offer(dist, slot);
                }
            }
        }
    }

    best.map(|(_, slot)| slot)
}

/// The [`WaveSlot`] a ratio-rib segment ENDING at point index `j` measures
/// — see [`wave_slot_at`]'s own doc for the two-pass rule this implements:
/// an EXACT match (some slot's own [`slot_span`] ends exactly at `j`) is
/// tried first and always preferred; only when nothing ends exactly at `j`
/// does an INTERIOR match (some slot's own span strictly CONTAINS `j`, i.e.
/// `j` is one of that slot's own interior subwave pivots — the case a
/// combination's own multi-leg component slot produces) apply. `None` when
/// neither rule finds a slot, which cannot happen for any `j` a real rib
/// table ever names, but is handled rather than assumed.
fn rib_segment_wave(type_id: &str, wave_slots: &[WaveSlot], j: usize) -> Option<WaveSlot> {
    for &slot in wave_slots {
        if slot_span(type_id, slot).is_some_and(|(_, end)| end == j) {
            return Some(slot);
        }
    }
    for &slot in wave_slots {
        if slot_span(type_id, slot).is_some_and(|(start, end)| start < j && j < end) {
            return Some(slot);
        }
    }
    None
}
