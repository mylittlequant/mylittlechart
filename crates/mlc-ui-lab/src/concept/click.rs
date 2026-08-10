//! Click dispatch for the concept modal (`super::render::render_concept_modal`).
//!
//! Parses one routed widget id — the `"concept_modal:item:"` prefix already
//! stripped by the caller (`crate::lib`'s own response loop, same shape as
//! its `"objtree_modal:"`/`"app_modal:"` handling) — back into the [`Row`]
//! it was drawn from, and applies the matching `ConceptModalState` mutator.
//! Scoped narrowly to the interactive subset the launching brief named:
//! [`Row::Chk`]/[`Row::Chk3`] toggle, [`Row::Mode`]/[`Row::Pills`]/
//! [`Row::Icons`] segment select, the ladder's own per-level checkbox
//! ([`Row::Ladder`]), the Table type's own per-cell [`Cell::Check`]
//! ([`Row::Table`]), and the stepper's `-`/`+` ([`Row::Stepper`]) — every
//! other `Row` variant stays inert (see `super::render`'s own "Known
//! simplifications" doc), so a click that resolves to one of THOSE rows
//! falls through and [`apply_click`] returns `false`.
//!
//! Tab switching and close/OK are simple enough to stay inline in
//! `crate::lib`'s own response loop (same shape as its `shell:*`/
//! `app_modal:*` handling) — this module is only the part that needs to
//! know a row's own SHAPE to act on it.

use super::spec::{Cell, ConceptTab, Row};
use super::state::ConceptModalState;

/// Split `item_id` into its base row key (`"{target}:{tab}:{group}:{row}"`,
/// see [`super::state::row_key`]) and, when present, the sub-widget suffix
/// after `"::"` (`"chk3:{i}"`, `"mode:{i}"`, `"pills:{i}"`, `"icons:{i}"`,
/// `"level:{i}"`, `"cell:{row}:{col}"`, `"stepper:dec"`/`"stepper:inc"`) —
/// the exact shapes `super::render`/`super::render::render_collections`
/// register their own sub-widgets under.
fn split_item_id(item_id: &str) -> (&str, Option<&str>) {
    match item_id.find("::") {
        Some(idx) => (&item_id[..idx], Some(&item_id[idx + 2..])),
        None => (item_id, None),
    }
}

/// Parse a row key's own `"{tab}:{group}:{row}"` tail — the target-id
/// segment is not needed here (the caller already resolved `tabs` from the
/// currently-open target). Every part is a plain identifier (tab ids are
/// snake_case, group/row indices are decimal) — none of the four ever
/// contains a colon of its own, so `rsplitn(4, ':')` recovers the tail
/// without ambiguity.
fn parse_row_key(key: &str) -> Option<(&str, usize, usize)> {
    let mut it = key.rsplitn(4, ':');
    let row_idx = it.next()?.parse().ok()?;
    let group_idx = it.next()?.parse().ok()?;
    let tab_id = it.next()?;
    let _target_id = it.next()?;
    Some((tab_id, group_idx, row_idx))
}

/// Walk `tabs` down to the exact [`Row`] a base key names.
fn row_for_key<'a>(tabs: &'a [ConceptTab], key: &str) -> Option<&'a Row> {
    let (tab_id, group_idx, row_idx) = parse_row_key(key)?;
    let tab = tabs.iter().find(|t| t.id == tab_id)?;
    let group = tab.groups.get(group_idx)?;
    group.rows.get(row_idx)
}

/// Apply one click against `state`. `tabs` is the currently-open target's
/// own tab list (the caller already resolved `state.open` via
/// `super::render::tabs_for`). Returns `true` if `item_id` matched a known
/// interactive row shape.
pub fn apply_click(tabs: &[ConceptTab], item_id: &str, state: &mut ConceptModalState) -> bool {
    let (base_key, suffix) = split_item_id(item_id);
    let Some(row) = row_for_key(tabs, base_key) else { return false };

    match row {
        Row::Chk { on, .. } => {
            state.toggle_chk(base_key, *on);
            true
        }
        Row::Chk3 { items } => {
            let Some(idx) = suffix.and_then(|s| s.strip_prefix("chk3:")).and_then(|s| s.parse::<usize>().ok()) else {
                return false;
            };
            state.toggle_chk3(base_key, idx, items);
            true
        }
        Row::Mode { .. } => {
            let Some(idx) = suffix.and_then(|s| s.strip_prefix("mode:")).and_then(|s| s.parse::<usize>().ok()) else {
                return false;
            };
            state.set_segment(base_key, idx);
            true
        }
        Row::Pills { .. } => {
            let Some(idx) = suffix.and_then(|s| s.strip_prefix("pills:")).and_then(|s| s.parse::<usize>().ok()) else {
                return false;
            };
            state.set_segment(base_key, idx);
            true
        }
        Row::Icons { .. } => {
            let Some(idx) = suffix.and_then(|s| s.strip_prefix("icons:")).and_then(|s| s.parse::<usize>().ok()) else {
                return false;
            };
            state.set_segment(base_key, idx);
            true
        }
        Row::Ladder { .. } => {
            let Some(idx) = suffix.and_then(|s| s.strip_prefix("level:")).and_then(|s| s.parse::<usize>().ok()) else {
                return false;
            };
            let level_key = format!("{base_key}::level:{idx}");
            // Mirrors `render_collections::draw_ladder_row`'s own default
            // (`state.chk(&level_key, true)`) — every level starts visible.
            state.toggle_chk(&level_key, true);
            true
        }
        Row::Table { rows, .. } => {
            let Some(rest) = suffix.and_then(|s| s.strip_prefix("cell:")) else { return false };
            let Some((row_s, col_s)) = rest.split_once(':') else { return false };
            let (Ok(row_i), Ok(col_i)) = (row_s.parse::<usize>(), col_s.parse::<usize>()) else { return false };
            let default_on = rows
                .get(row_i)
                .and_then(|r| r.get(col_i))
                .and_then(|c| match c {
                    Cell::Check(on) => Some(*on),
                    _ => None,
                })
                .unwrap_or(false);
            let cell_key = format!("{base_key}::cell:{row_i}:{col_i}");
            state.toggle_chk(&cell_key, default_on);
            true
        }
        Row::Stepper { value, .. } => match suffix {
            Some("stepper:dec") => {
                state.bump_stepper(base_key, *value, -1);
                true
            }
            Some("stepper:inc") => {
                state.bump_stepper(base_key, *value, 1);
                true
            }
            _ => false,
        },
        _ => false,
    }
}
