//! Platform-neutral chrome (title bar) types, geometry logic, and render functions.

pub mod logic;
pub mod render;
pub mod types;

pub use types::{
    CHROME_HEIGHT, ChromeFlavor, ChromeHit, ChromeMenuAction, ChromeContextMenu, ChromeColors,
    ChromeState, Tab, TabDrag,
};
pub use logic::{hit_test, update_tooltip, context_menu_hit_test, context_menu_hover, new_tab_button_x,
    tabs_right_limit, max_tab_scroll, scroll_tab_strip, scroll_tab_into_view,
    tab_left_x, tab_drop_index,
    tab_list_rect, tab_list_hit_test, tab_list_hover, MIN_TAB_WIDTH};
pub use render::{render, render_tooltip, render_tooltip_themed, render_context_menu, render_tab_list, update_tab_widths};
