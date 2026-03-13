//! Chart modal rendering functions.
//!
//! Each modal lives in its own sub-module for clear separation of concerns.

pub mod indicator_settings;
pub mod context_menu;
pub mod primitive_settings;
pub mod indicator_color_picker;
pub mod chart_settings_color_picker;
pub mod indicator_overlay_dropdown;
pub mod indicator_overlay;
pub mod panel_color_tag;
pub mod simple_modal;
pub mod hotkeys;
pub mod search_overlay;
pub mod chart_settings;
pub mod overlay_settings;
pub mod preset_name_input;
pub mod chart_browser;
pub mod tags_tabs_modal;
pub mod watchlist_modal;
pub mod alert_settings;
pub use alert_settings::*;
pub mod compare_settings;
pub use compare_settings::{render_compare_settings_modal, CompareSettingsResult};
pub mod template_name_modal;
pub use template_name_modal::{render_template_name_modal, TemplateNameModalResult};
pub mod compare_color_picker;
pub use compare_color_picker::render_compare_color_picker_popup;
pub use chart_browser::{render_chart_browser, ChartBrowserResult};
pub mod user_settings;
pub use user_settings::render_user_settings_modal;
pub mod welcome_wizard;
pub use welcome_wizard::render_welcome_wizard;
pub use welcome_wizard::render_vault_unlock;
pub mod profile_manager;
pub use profile_manager::render_profile_manager;

pub use indicator_settings::*;
pub use context_menu::*;
pub use primitive_settings::*;
pub use indicator_color_picker::*;
pub use chart_settings_color_picker::*;
pub use indicator_overlay_dropdown::*;
pub use indicator_overlay::*;
pub use panel_color_tag::*;
pub use simple_modal::*;
pub use hotkeys::*;
pub use search_overlay::*;
pub use chart_settings::{
    render_settings_modal, ChartSettingsData,
    InstrumentSettings, StatusLineSettings, ScalesLinesSettings,
};
pub use overlay_settings::{render_overlay_settings_modal, OverlaySettingsResult};
pub use tags_tabs_modal::{render_tags_tabs_modal, TagsTabsResult};
pub use watchlist_modal::{
    render_wl_group_name_input, WlGroupNameInputResult,
};
