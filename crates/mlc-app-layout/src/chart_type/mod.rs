//! Chart-type render-plane dispatch tables.
//!
//! Step 9 of the chart-type registry arc — see `draw_ops` for the
//! fn-pointer table that replaced the `match chart_type` block in
//! `layout::render_chart::draw_series`.

pub mod draw_ops;

pub use draw_ops::{DrawOps, draw_ops_by_id};
