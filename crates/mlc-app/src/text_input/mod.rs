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

/// Template name input for the "Save as Template" overlay in primitive settings.
pub const PRIMITIVE_SETTINGS_TEMPLATE_NAME: &str = "prim_tmpl_name";

/// Template name input for the "Save as Template" overlay in indicator settings.
pub const INDICATOR_SETTINGS_TEMPLATE_NAME: &str = "ind_tmpl_name";

/// Template name input for the "Save as Template" overlay in compare settings.
pub const COMPARE_SETTINGS_TEMPLATE_NAME: &str = "cmp_tmpl_name";

/// Template name input for the "Save as Template" overlay in chart settings.
pub const CHART_SETTINGS_TEMPLATE_NAME: &str = "chart_tmpl_name";

/// Sub-field edit input in the Primitive Settings modal.
/// One WidgetId covers ALL inline sub-fields (stroke_width, text_content, level_N_value,
/// coord_N_price, coord_N_bar, tf_N_min/max, style_prop:*, text_prop:*).
/// The discriminant (which sub-field) lives in `PrimitiveSettingsState.active_field_id`.
pub const PRIMITIVE_SETTINGS_FIELD: &str = "prim_settings_field";

/// Sub-field edit input in the Indicator Settings modal.
pub const INDICATOR_SETTINGS_FIELD: &str = "ind_settings_field";

/// Sub-field edit input in the Chart Settings modal (watermark text, etc.).
pub const CHART_SETTINGS_FIELD: &str = "chart_settings_field";

/// Sub-field edit input in the Compare Settings modal (tf min/max fields).
pub const COMPARE_SETTINGS_FIELD: &str = "cmp_settings_field";

/// New profile name input (profile manager / welcome wizard / settings modal).
pub const NEW_PROFILE_NAME: &str = "new_profile_name";

/// Inline profile rename input (user settings profile list row).
pub const PROFILE_RENAME: &str = "profile_rename";

/// New passphrase input (profile manager / welcome wizard / settings sync tab / vault unlock).
/// Rendered with asterisk masking.
pub const NEW_PASSPHRASE: &str = "new_passphrase";

/// Confirm passphrase input (profile manager CreatePassphrase/SetNewPassphrase / wizard).
/// Rendered with asterisk masking.
pub const CONFIRM_PASSPHRASE: &str = "confirm_passphrase";

/// Recovery key display box (UseRecoveryKey page).
/// Long paste-friendly text field; read-only display on ShowRecoveryKey page.
pub const RECOVERY_KEY: &str = "recovery_key";

/// Telegram bot token input (alert settings notifications tab).
pub const ALERT_TG_TOKEN: &str = "alert_tg_token";
