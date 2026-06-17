//! Lifecycle marker for the currently-active preset slot on a window.
//!
//! Set by [`super::hydration`] dispatch and cleared once a hydrated state
//! has been swapped in. While in `Loading`:
//!
//! - `flush_pending_autosave` early-returns (a placeholder grid + the new
//!   active_preset_id together would otherwise be serialised to disk).
//! - `PresetAction::Upsert` host drain skips this id.
//! - `render_full_chart_panel` short-circuits to the loading skeleton even
//!   if `bars` happens to be non-empty.
//! - `park_active_preset` is never called.
//!
//! The previous preset's `LivePresetState` is parked in `live_preset_cache`
//! BEFORE the state flips to `Loading`, so a fast back-click during cold
//! load still has a warm cache entry to unpark.

use std::time::Instant;

#[derive(Debug, Clone)]
pub enum LoadState {
    /// Window is showing a fully materialised preset. Autosave is enabled,
    /// renderers behave normally.
    Live,
    /// A cold-path hydrate worker is in flight for `target_id`. Autosave is
    /// disabled. Renderer must draw the loading skeleton.
    Loading {
        target_id: String,
        started_at: Instant,
    },
}

impl Default for LoadState {
    fn default() -> Self {
        LoadState::Live
    }
}

impl LoadState {
    #[inline]
    pub fn is_loading(&self) -> bool {
        matches!(self, LoadState::Loading { .. })
    }

    #[inline]
    pub fn target(&self) -> Option<&str> {
        match self {
            LoadState::Loading { target_id, .. } => Some(target_id.as_str()),
            LoadState::Live => None,
        }
    }
}
