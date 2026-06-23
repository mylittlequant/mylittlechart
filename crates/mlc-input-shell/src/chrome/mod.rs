//! Platform-neutral chrome (title bar) types and geometry logic.
//!
//! Render functions that require `dyn RenderContext` stay in `mlc-app-vello/src/chrome.rs`.

pub mod logic;
pub mod types;

pub use types::{
    CHROME_HEIGHT, ChromeHit, ChromeMenuAction, ChromeContextMenu, ChromeColors, ChromeState, Tab,
};
pub use logic::{hit_test, update_tooltip, context_menu_hit_test, context_menu_hover, new_tab_button_x};
