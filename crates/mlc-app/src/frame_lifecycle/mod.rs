//! Frame lifecycle: tick (live update loop), prepare_frame, alert rendering.
pub(crate) mod alert_render;
pub(crate) mod dom_books;
pub(crate) mod liq_pump;
pub(crate) mod liq_projection;
pub(crate) mod subpane_series;
pub(crate) mod profile_klines;
pub(crate) mod prepare;
pub(crate) mod react;
pub(crate) mod tick;
