//! Lightweight projection of `mlc_core::preset::preset::ChartPreset` for the
//! layout layer.
//!
//! Layout lives BELOW mlc-app in the dep-graph, so it cannot speak about
//! the full `ChartPreset` type.  App projects each preset into one of these
//! and hands a slice to the preset-browser modal — same pattern used by
//! `TagView` for bubbles.

#[derive(Clone, Debug)]
pub struct PresetSummary {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    /// `(symbol, timeframe_name)` of the first window snapshot, if any.
    pub first_window: Option<(String, String)>,
}
