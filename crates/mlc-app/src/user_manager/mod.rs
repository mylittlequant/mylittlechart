//! User persistence: one-shot disk bootstrap + multi-profile orchestration.
//!
//! # Startup (headless / test-harness path)
//! Call [`bootstrap::load_user_state_from_disk`] once at application startup
//! to restore profile, templates, presets, and settings snapshots from disk.
//!
//! # Startup (shell-vello multi-window path)
//! Use [`ProfileManager`] — it owns the multi-profile index, vault key
//! derivation, and profile-switch flow. See its doc-comment and the step
//! 3.6c report (`docs/mlc/plans/doctrine-cutover-2026-06-27.md`) for why the
//! two paths are not unified: `ProfileManager` is legitimate app-side UI-flow
//! orchestration, not a duplicate of `BackendCore`'s `UserProfileManager`.
//!
//! # Shutdown / Save
//! `mlc-app-shell-vello::App::save_all()` persists everything for the
//! multi-window path.

pub mod bootstrap;
pub mod profile_manager;

pub use bootstrap::load_user_state_from_disk;
pub use profile_manager::{ProfileInfo, ProfileManager, SwitchData, MIN_PASSPHRASE_LENGTH};
