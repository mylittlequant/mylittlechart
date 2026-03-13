//! User profile system — aggregate persistent state for a user session.
//!
//! A [`UserProfile`] is the top-level metadata record that tracks active
//! selections and UI state.  Heavy data (chart presets, templates, watchlists)
//! are stored in their own files under the OS application data directory,
//! using the generic [`storage`] helpers.
//!
//! # Directory layout
//!
//! ```text
//! {APP_DATA_DIR}/zengeld/
//!   profile.json        — UserProfile (active selections, UI state)
//!   presets/            — ChartPreset files (managed by preset::storage)
//!   templates/          — Template files (managed by templates::storage)
//!   watchlists.json     — WatchlistManager snapshot
//! ```

pub mod profile;
pub mod storage;

pub use profile::{UserProfile, VaultSecrets, WindowState, StoredApiKey, ProfileMeta, ProfileIndex};
pub use storage::{
    ProfileError,
    app_data_dir,
    get_user_data_dir,
    save_profile,
    load_profile,
    save_json,
    load_json,
    profiles_dir,
    load_profile_index,
    save_profile_index,
    active_profile_data_dir,
    migrate_legacy_profile_if_needed,
    create_profile,
    delete_profile,
    watchlists_path,
};
