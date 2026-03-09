//! Chart-local modal state.
//!
//! Tracks which modal is currently open inside the chart sub-application.
//! The actual modal UI state (tabs, dragging, color pickers) still lives
//! in the host (core) crate — this enum is the chart's own record of
//! which dialog it requested.

/// Which chart-local modal is currently open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ChartOpenModal {
    /// No modal open.
    #[default]
    None,
    /// Chart settings (instrument, scales, appearance).
    ChartSettings,
    /// Indicator search / add indicator.
    IndicatorSearch,
    /// Primitive / drawing object settings.
    PrimitiveSettings,
    /// Indicator settings for a specific indicator.
    IndicatorSettings,
    /// Screenshot / export dialog.
    Screenshot,
    /// Overlay (leaf) layout and settings dialog.
    OverlaySettings,
    /// Tags & Tabs modal (unified panel-tree + sync-group manager).
    TagsTabs,
    /// Preset name input (Save As / Rename).
    PresetNameInput,
    /// Chart browser (Open Chart) dialog.
    ChartBrowser,
}

impl ChartOpenModal {
    /// Any modal open?
    pub fn is_open(self) -> bool {
        self != Self::None
    }

    /// Is this a search-style overlay?
    pub fn is_search(self) -> bool {
        matches!(self, Self::IndicatorSearch)
    }

    /// Is this a settings-style dialog?
    pub fn is_settings(self) -> bool {
        matches!(self, Self::ChartSettings | Self::PrimitiveSettings | Self::IndicatorSettings | Self::OverlaySettings | Self::TagsTabs | Self::PresetNameInput | Self::ChartBrowser)
    }
}
