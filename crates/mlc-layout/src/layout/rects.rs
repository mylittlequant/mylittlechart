//! Layout rect types - platform-agnostic rectangles
//!
//! All types live in `state::geometry`. Re-exported here so all existing
//! `layout::` consumers keep working without import changes.

pub use mlc_core::state::geometry::{
    LayoutRect,
    ChartAreaLayout,
    ChartHitZone,
    FrameLayout,
    SubPaneLayout,
    ExtendedFrameLayout,
};
