//! Chart UI rendering - self-contained toolbar and overlay rendering
pub mod widgets;
pub mod z_order;
pub mod avatar_widget;
pub mod chips_input;
pub mod count_badge;
pub mod toggle_widget;
pub mod toolbar_theme;
pub mod icons;
pub mod toolbar_core;
mod clock_glitch;
mod mascot_anim;
pub mod dropdown;
pub mod overlay_fill;
pub mod color_picker_state;
pub mod scroll_state;
pub mod scroll_widget;
pub mod selectable_text;
pub mod modal_settings;
pub mod modal_state;
pub mod context_menu;
pub mod sync_color_grid;
pub mod tags_tabs_state;
pub mod share_viewer_row;
pub mod toast;

pub use icons::Icon;
pub use tags_tabs_state::*;
pub use modal_state::{
    SearchResult,
    IndicatorCategory,
    OpenModal,
    ModalState,
    ClockPopupState,
    IndicatorCategoryFilter,
    IndicatorCatalogItem,
};
