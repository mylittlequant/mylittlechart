// Types moved to `mlc_core::state::toolbar_state` (Phase X.3 back-edge removal).
// Re-exported here to keep `crate::layout::ToolbarState` (and friends) valid
// for all downstream call sites outside `state/`.
pub use mlc_core::state::toolbar_state::{ToolbarState, ToolbarClickResult, ToggleIconPair};
