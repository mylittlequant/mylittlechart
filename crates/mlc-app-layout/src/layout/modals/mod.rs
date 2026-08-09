//! Chart modal rendering functions.
//!
//! Each modal lives in its own sub-module for clear separation of concerns.

/// Where a settings row puts its control: after the label column, running to
/// the row's right edge.
///
/// A resizable modal that keeps its controls at a fixed width just grows a
/// field of empty space to the right of them, which is the same as not
/// resizing at all. Controls that show a VALUE — dropdowns, text inputs,
/// sliders — take the room; checkboxes and colour swatches keep their size and
/// do not go through here.
///
/// `min_w` is the width the control needs to read at all — usually its widest
/// label. There is deliberately no ceiling: a ceiling puts the empty space
/// back, which is the complaint that started this. A user who wants a narrow
/// control narrows the modal.
///
/// `width` must be the width the body may actually PAINT — inside a scrolling
/// body that is `ScrollableContainer::content_width()`, not the viewport's.
/// The container clips to the narrower of the two, and a control laid out to
/// the wider one has its right end, chevron and all, sliced off at the clip.
pub fn control_column(x: f64, width: f64, label_col: f64, min_w: f64) -> (f64, f64) {
    let cx = x + label_col;
    let w = (x + width - cx - SCROLLBAR_GAP).max(min_w);
    (cx, w)
}

/// Breathing room between a stretched control and the scrollbar beside it.
const SCROLLBAR_GAP: f64 = 6.0;

pub mod indicator_settings;
pub mod context_menu;
pub mod primitive_settings;
pub mod primitive_settings_vid;
pub mod primitive_settings_labels;
pub mod primitive_settings_levels;
pub mod primitive_settings_cells;
pub mod primitive_settings_placement;
pub mod primitive_settings_palettes;
pub mod primitive_settings_intent;
pub mod primitive_settings_property;
pub mod indicator_color_picker;
pub mod chart_settings_color_picker;
pub mod indicator_overlay_dropdown;
pub mod indicator_overlay;
pub mod panel_color_tag;
pub mod search_overlay;
pub mod chart_settings;
pub mod preset_name_input;
pub mod chart_browser;
pub mod tags_tabs_modal;
pub mod panel_settings_popup;
pub mod trading_container_modal;
pub use panel_settings_popup::{
    render_panel_settings_popup, PanelSettingRow, PanelSettingsPopupResult, PanelSettingsView,
};
pub mod watchlist_modal;
pub mod clock_popup;
pub use clock_popup::{render_clock_popup, ClockPopupResult};
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
pub use search_overlay::*;
pub use chart_settings::{
    render_settings_modal, ChartSettingsData,
    InstrumentSettings, StatusLineSettings, ScalesLinesSettings,
};
pub use tags_tabs_modal::{render_tags_tabs_modal, TagsTabsResult};
pub use trading_container_modal::{
    render_trading_container_modal, TcMapPanel, TcModalResult, TcModalSection,
    TcSetupData, TradingContainerModalState,
};
pub use watchlist_modal::{
    render_wl_group_name_input, WlGroupNameInputResult,
};
