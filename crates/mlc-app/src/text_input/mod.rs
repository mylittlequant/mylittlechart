//! String IDs for text fields managed by `InputCoordinator::text_fields`.
//!
//! These constants are the canonical identifiers passed to the uzor
//! `TextFieldStore` API.  They replace the former `FieldId` enum now that
//! `TextInputManager` has been removed in favour of the coordinator-owned
//! `TextFieldStore`.

/// Hex-color field in the L2 color-picker popup.
pub const HEX_COLOR: &str = "hex_color";

/// Raw PTY input field for the agent terminal pane.
pub const AGENT_PTY: &str = "agent_pty";

/// Text chat input field for the agent chat pane.
pub const AGENT_CHAT: &str = "agent_chat";
