//! Ported collection of trading / info / visual panel state + renderers from
//! zengeld-terminal. Copied wholesale in Phase 4-new of the sidebar docking
//! refactor so FreeItem slots can host real panels instead of placeholder
//! stubs. Not all panels are wired into the chart yet — this crate is the
//! source of truth and the wiring happens incrementally.
//!
//! - [`trading`]  — order flow, DOM, market data, trade log, etc.
//! - [`info`]     — analytics, calendar, news, options, portfolio, reference, utility
//! - [`visual`]   — financial, scientific, realtime, hierarchy, network, specialized
//!                  (NOTE: geospatial intentionally dropped — no map crate in chart)
//!
//! Renderers live in [`renderers`] and all use [`zengeld_chart::render::RenderContext`].

pub mod trading;
pub mod info;
pub mod visual;
pub mod renderers;

// Re-export RenderContext under the old `crate::render` path so the copied
// renderer files can keep `use crate::render::{RenderContext, ...};` unchanged.
pub mod render {
    pub use zengeld_chart::render::{RenderContext, TextAlign, TextBaseline};
}
