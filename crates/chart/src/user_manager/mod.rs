//! Unified user state manager — owns all persistent user data.
//!
//! `UserManager` is the single entry point for all user persistence:
//! templates, presets, profile settings, and runtime settings snapshots.
//!
//! # Startup
//! Call `UserManager::load()` at application startup to restore all user state
//! from disk.
//!
//! # Shutdown / Save
//! Call `save_all()` to persist everything, or use the granular save methods.

pub mod manager;
pub mod profile_manager;

pub use manager::UserManager;
pub use profile_manager::{ProfileInfo, ProfileManager, SwitchData};
