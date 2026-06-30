//! Preset persistence + live-state hydration.
//!
//! All loader / saver / cache logic for chart presets lives here. Outside
//! this module, `ChartApp` only sees:
//!
//! - `park_active_preset(id)` / `unpark_preset(id)` — warm cache pair (cheap
//!   `mem::replace` swap). Hot-path preset switch between already-open tabs.
//! - `hydrate_preset(...)` — pure `Preset -> HydratedPresetState`. Cold-path
//!   builder. `Send + 'static`, designed to run on a worker thread off the
//!   main loop. No `bridge.*` calls, no `autosave_snapshot`.
//! - `ignite_subscriptions(...)` — bridge subscribe + request_bars seeds.
//!   Runs on the main thread immediately after a hydrated state is swapped
//!   in.
//! - `LoadState` — `Live | Loading { target_id, .. }`. Gates `autosave` and
//!   `PresetAction::Upsert` so a placeholder grid is never serialised to
//!   disk (the `7a9d1d2` disaster path).
//! - `autosave_snapshot` / `flush_pending_autosave` / `collect_state_into_preset`
//!   — debounced once-per-frame writer. Skips when `LoadState::Loading`.
//! - profile helpers (`persist_profile`, `build_user_profile`,
//!   `load_user_profile`, etc.) — unchanged signatures, just relocated.

pub mod autosave;
pub mod hydration;
pub mod hydration_impl;
pub mod live_cache;
pub mod load_state;
pub mod profile;

pub use live_cache::LivePresetState;
pub use load_state::LoadState;
