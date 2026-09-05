//! String IDs for text fields managed by `InputCoordinator::text_fields`.
//!
//! See also the [`dispatch`] submodule for the centralised text-input dispatcher.
//!
//! These constants are the canonical identifiers passed to the uzor
//! `TextFieldStore` API.  They replace the former `FieldId` enum now that
//! `TextInputManager` has been removed in favour of the coordinator-owned
//! `TextFieldStore`.

pub mod dispatch;

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

/// Custom watermark text in the settings-gear submenu.
pub const WATERMARK_TEXT: &str = "watermark_text";

/// Per-panel DOM depth-bounds band, in percent. Dynamic id — the panel id is
/// appended, so the field belongs to the ladder being edited and survives the
/// modal being reopened on another panel.
pub const DOM_DEPTH_PCT_PREFIX: &str = "dom_depth_pct:";

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
/// coord_N_price, coord_N_bar, tf_N_min/max, style_prop:*, text_prop:*, level_prop:*).
/// The discriminant (which sub-field) lives in `PrimitiveSettingsState.active_field_id`.
pub const PRIMITIVE_SETTINGS_FIELD: &str = "prim_settings_field";

/// Sub-field edit input in the Indicator Settings modal.
pub const INDICATOR_SETTINGS_FIELD: &str = "ind_settings_field";

/// Sub-field edit input in the Chart Settings modal (watermark text, etc.).
pub const CHART_SETTINGS_FIELD: &str = "chart_settings_field";

/// Sub-field edit input in the Compare Settings modal (tf min/max fields).
pub const COMPARE_SETTINGS_FIELD: &str = "cmp_settings_field";

/// Inline on-chart text edit of a placed text primitive (inline-prim-text
/// arc, owner ask 2026-08-26: double-click a Text/Note/AnchoredText/
/// Callout/Comment/Signpost/PriceNote → edit its text in place, no modal).
/// One edit can be active at a time, so a fixed id suffices — the edited
/// primitive's identity lives in `ChartApp::inline_prim_text_edit`, not in
/// the id (the `PRIMITIVE_SETTINGS_FIELD` shared-id pattern, not the
/// per-instance `"{slot}:oe_field"` one).
pub const PRIM_INLINE_TEXT: &str = "prim_inline_text";

/// Inline chrome tab rename field (tab-rename arc, owner ask 2026-08-26:
/// double-click a chart tab → edit its name in place inside the tab strip,
/// no modal). One rename can be active at a time, so a fixed id suffices —
/// the renamed tab's identity lives in `ChromeState::renaming_tab`
/// (mlc-app-input-shell), not in the id (same shared-id pattern as
/// `PRIM_INLINE_TEXT`). Enter/Escape are intercepted by the input-shell's
/// own `KeyPressed` arm, so this field only needs the late-key admission
/// below for arrows/Backspace/Delete/Home/End.
pub const CHROME_TAB_RENAME: &str = "chrome_tab_rename";

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

/// Inline account-nickname edit input (user settings single-profile row,
/// `UserSettingsState::nick_edit_mode`). Sets the ACCOUNT's nickname via
/// `POST /api/account/nick` — never `PROFILE_RENAME`'s local `profile_meta`
/// display name, even though both edit the same row for a single-profile
/// identity. Not `cfg(feature = "cloud")`-gated, unlike `REDEEM_CODE`: the
/// row that owns this field compiles in every build.
pub const ACCOUNT_NICK: &str = "account_nick";

/// Telegram bot token input (alert settings notifications tab).
pub const ALERT_TG_TOKEN: &str = "alert_tg_token";

/// Gift/trial code input in the account overlay (cloud builds).
///
/// The modal that draws it lives in `mlc-app-layout`, which cannot depend on
/// this crate, so the id is spelled there as a literal too
/// (`cloud_login.rs`'s `REDEEM_FIELD`) — the same split every other modal
/// field in that crate already has. Enter submits, which is what a
/// ten-character code typed from a message wants.
pub const REDEEM_CODE: &str = "redeem_code";

/// Webhook URL input (credentials modal). Replaces the decorative field of
/// the same name that used to live in the alert-settings Notifications tab
/// (`alert_set:item:webhook_url`, whose click handler was a literal no-op) —
/// the value may embed its own auth token, so it is vault-gated exactly
/// like `ALERT_TG_TOKEN`.
pub const CRED_WEBHOOK_URL: &str = "cred_webhook_url";

/// Exchange id input in the credentials modal's "add key" form. Free-form
/// lowercase string (e.g. `"binance"`) — there is no canonical exchange
/// registry in this crate to draw a dropdown from, and `StoredExchangeKey::
/// exchange_id` is itself a free string.
pub const CRED_ADD_EXCHANGE: &str = "cred_add_exchange";

/// Label input in the credentials modal's "add key" form.
pub const CRED_ADD_LABEL: &str = "cred_add_label";

/// Public API key input in the credentials modal's "add key" form.
pub const CRED_ADD_API_KEY: &str = "cred_add_api_key";

/// API secret input in the credentials modal's "add key" form.
pub const CRED_ADD_API_SECRET: &str = "cred_add_api_secret";

/// Passphrase input in the credentials modal's "add key" form (OKX/KuCoin
/// style venues only — optional).
pub const CRED_ADD_PASSPHRASE: &str = "cred_add_passphrase";

/// Visible inline custom-timeframe editor field, shown in the "+ Add" row
/// slot of the `timeframe_selector` dropdown (user-granularity-templates-
/// inner arc, кусок 2 — owner spec 2026-07-16 replaced the invisible
/// "Custom…" input row with a visible inline editor; see
/// `ChartApp::granularity_edit`). Accepts a `<count><unit>` string (e.g.
/// `"7m"`, `"90m"`), validated via `mlc_core::state::Timeframe::parse` on
/// commit — see `ChartOutEvent::CommitGranularityCustom`'s doc comment.
/// Reused for both add (empty prefill) and rename (prefilled with the
/// original name) — the mode lives in `granularity_edit`, not the field id.
pub const TF_FAVORITE_INPUT: &str = "tf_favorite_input";

/// Visible inline custom numeric-param editor field, shown in the "+ Add"
/// row slot of the `timeframe_selector` dropdown for `ResolutionKind::Param`
/// chart types (user-granularity-templates-inner arc, кусок 2). Accepts a
/// positive `f64`, validated on commit — see
/// `ChartOutEvent::CommitGranularityCustom`'s doc comment. Reused for both
/// add and edit, same as `TF_FAVORITE_INPUT`.
pub const PARAM_FAVORITE_INPUT: &str = "param_favorite_input";

/// Inline fast-search text field hosted in the `symbol_selector` toolbar
/// dropdown (symbol-search-compare-inner arc, Wave 2, кусок 4 — owner spec
/// 2026-07-16, `ProfileUiSettings::symbol_quick_search`). Mirrors
/// `TF_FAVORITE_INPUT`'s plumbing exactly: same `draw_input`/
/// `draw_input_cursor` widget hosted over a dropdown row, same
/// `DropdownRenderInfo`-threaded geometry channel, same
/// `update_text_field_geometry` wiring. Live text drives
/// `ChartApp::symbol_quick_edit`'s per-frame result rebuild (no commit
/// validation grammar — any text is a valid substring query, unlike
/// `TF_FAVORITE_INPUT`'s `Timeframe::parse` gate).
pub const SYMBOL_QUICK_SEARCH: &str = "symbol_quick_search";
