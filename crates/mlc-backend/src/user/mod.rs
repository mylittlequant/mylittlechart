//! User profile manager + facade module.
//!
//! `UserProfileManager` (profile + settings snapshots + vault key) promoted
//! here from `mlc-app::user_manager` / `mlc-app-shell-vello::AppState`
//! (step 3.6c, doctrine 1B). App code accesses it exclusively through the
//! `User` / `UserMut` facade (see `BackendCore::user` / `user_mut` in
//! `crates/mlc-backend/src/lib.rs`).

mod manager;
mod facade;
pub use manager::UserProfileManager;
pub use facade::{User, UserMut};
