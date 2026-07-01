//! Platform-neutral input pipeline shared by vello and web shells.
//!
//! This crate contains no winit, no arboard, no wasm-bindgen, no vello.
//! It depends only on mlc-app (ChartApp + KeyPress), mlc-core, mlc-app-sidebar-content,
//! uzor (WidgetId), and web-time.

pub mod chrome;
pub mod events;
pub mod platform;
pub mod state;
pub mod tooltip;
pub mod dispatch;

pub use chrome::{
    ChromeFlavor, ChromeHit, ChromeMenuAction, ChromeContextMenu, ChromeColors, ChromeState, Tab,
    CHROME_HEIGHT,
    render, render_tooltip, render_tooltip_themed, render_context_menu, update_tab_widths,
};
pub use events::{ShellEvent, NormalizedModifiers, MouseBtn, LogicalKey, PhysicalKey, ResizeDir};
pub use platform::{Platform, DispatchOut};
pub use state::{InputState, DirtyFlags};
pub use tooltip::{TooltipState, TooltipRequest, calculate_tooltip_position};
pub use dispatch::process_event;
