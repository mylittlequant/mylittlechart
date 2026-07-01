//! Layout computation - platform-agnostic layout for charts
//!
//! All geometry types and computation impls now live in `mlc_core::state::geometry`.
//! This module re-exports them so that `crate::layout::Margins` etc. continue to
//! resolve inside mlc-app-layout without any changes to callers within this crate.

pub use mlc_core::state::geometry::{
    Margins,
    LayoutConfig,
    LayoutManager,
};
