//! Centralized text-input manager for `ChartApp`.
//!
//! All text-field state (text, cursor, selection) is owned here.
//! Renderers register field geometry each frame; input handlers call
//! the manager's methods instead of touching scattered per-field state copies.

mod manager;
mod types;

pub use manager::TextInputManager;
pub use types::{FieldAction, FieldConfig, FieldId, InputCapability};
