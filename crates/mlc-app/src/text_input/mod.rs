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

/// Numeric parameter field for derived bar-mode subscriptions.
///
/// - For `RangeBar`: price range (e.g. `"10.0"`).
/// - For `TickBar`: trade count (e.g. `"50"`).
/// - For `VolumeBar`: volume threshold (e.g. `"1.0"`).
/// - Not shown / ignored for `TimeBased` and `Footprint`.
///
/// The field is pre-filled with the current bar-mode parameter whenever the
/// bar-mode dropdown opens.  On `Enter` the value is parsed and
/// `ChartOutEvent::UpdateBarModeParam` is emitted.
pub const BAR_MODE_PARAM: &str = "bar_mode_param";

/// Symbol search overlay search input.
pub const SYMBOL_SEARCH: &str = "symbol_search";

/// Indicator search overlay search input.
pub const INDICATOR_SEARCH: &str = "indicator_search";

/// Compare search overlay search input.
pub const COMPARE_SEARCH: &str = "compare_search";

/// Chart browser modal search input.
pub const CHART_BROWSER_SEARCH: &str = "chart_browser_search";

/// Watchlist modal Overview-tab search input.
pub const WATCHLIST_SEARCH: &str = "watchlist_search";

/// Watchlist group name dialog input.
pub const WATCHLIST_GROUP_NAME: &str = "watchlist_group_name";

/// Preset name input modal text field.
pub const PRESET_NAME_INPUT: &str = "preset_name_input";
