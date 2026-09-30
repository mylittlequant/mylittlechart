//! Platform-neutral chrome (title bar) types, geometry logic, and render functions.

pub mod logic;
pub mod render;
pub mod types;

pub use types::{
    CHROME_HEIGHT, ChromeFlavor, ChromeHit, ChromeMenuAction, ChromeContextMenu, ChromeColors,
    ChromeState, Tab, TabDrag, is_veiled_hit,
    ChromeUpdate,
};
pub use logic::{hit_test, update_tooltip, context_menu_hit_test, context_menu_hover, new_tab_button_x,
    tabs_right_limit, max_tab_scroll, scroll_tab_strip, scroll_tab_into_view,
    tab_left_x, tab_drop_index,
    tab_list_rect, tab_list_hit_test, tab_list_hover, tab_list_should_close, MIN_TAB_WIDTH,
    AccountMenuItem, ACCOUNT_MENU_WIDTH, ACCOUNT_MENU_ITEM_HEIGHT, ACCOUNT_MENU_PADDING};
#[cfg(feature = "cloud")]
pub use logic::{account_menu_rect, account_menu_hit_test, account_menu_hover};
pub use render::{
    render, render_backdrop_dim, render_tooltip, render_tooltip_themed, render_context_menu,
    render_tab_list, update_tab_widths, SYSTEM_BUTTONS_WIDTH,
};
