//! Re-export shim — Icon and all SVG consts now live in mlc-core.
//!
//! mlc-app-layout widget code that uses `crate::ui::icons::Icon` or
//! `crate::ui::icons::icon_svg` continues to resolve via this re-export.
pub use mlc_core::state::icons::*;
