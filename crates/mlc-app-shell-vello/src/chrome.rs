//! Chrome re-export shim for the native (vello) shell.
//!
//! All render functions and types have been moved to `mlc_app_input_shell::chrome`
//! so they can be shared with the web shell. This file is a thin re-export
//! shim that keeps existing `use crate::chrome::…` imports in the vello shell
//! working without modification.

pub use mlc_app_input_shell::chrome::{
    CHROME_HEIGHT, ChromeState, Tab,
    update_tooltip, update_tab_widths, render, render_backdrop_dim, render_tooltip,
    render_tooltip_themed, render_context_menu, render_tab_list,
};
