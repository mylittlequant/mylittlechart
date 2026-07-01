//! Platform-neutral chrome (title bar) types, geometry logic, and render functions.

pub mod logic;
pub mod render;
pub mod types;

pub use types::{
    CHROME_HEIGHT, ChromeFlavor, ChromeHit, ChromeMenuAction, ChromeContextMenu, ChromeColors,
    ChromeState, Tab,
};
pub use logic::{hit_test, update_tooltip, context_menu_hit_test, context_menu_hover, new_tab_button_x};
pub use render::{render, render_tooltip, render_tooltip_themed, render_context_menu, update_tab_widths};
