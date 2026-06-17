//! Autosave gating helpers.
//!
//! The actual `autosave_snapshot` / `flush_pending_autosave` /
//! `collect_state_into_preset` functions live on `ChartApp` in
//! `crate::input::mod` (100+ callsites of `autosave_snapshot` make a hard
//! relocation pointless). What lives here is the **gate**: a single rule
//! that says "is the current `ChartApp` state safe to serialise to disk?".
//!
//! Today that rule is "`LoadState::Live` only". A future refactor can pull
//! the full autosave body in here once the gate proves stable.

use super::load_state::LoadState;

/// Returns `true` when it is safe to run `collect_state_into_preset` and
/// push a `PresetAction::Upsert` for the current active preset.
///
/// Returns `false` while the window is mid cold-hydrate: the `panel_grid`
/// has been replaced with `ChartPanelGrid::placeholder()` by `park`, but
/// `active_preset_id` already points at the NEW id. A snapshot here would
/// overwrite the real preset file on disk with placeholder content (this
/// was the `7a9d1d2` disaster path).
#[inline]
pub fn is_persist_safe(load_state: &LoadState) -> bool {
    matches!(load_state, LoadState::Live)
}
