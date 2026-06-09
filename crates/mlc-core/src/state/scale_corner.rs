//! Scale-corner button state + hit zones (pure data, consumed by ChartContainer).

use crate::chart::types::price_scale::ScaleMode;
use crate::state::geometry::LayoutRect;

/// State needed for rendering scale corner with buttons
#[derive(Clone, Debug)]
pub struct ScaleCornerState {
    /// Current scale corner mode (Manual / Auto / Focus)
    pub scale_mode: ScaleMode,
    /// Mode label (e.g., "lin", "log", "%")
    pub mode_label: String,
    /// Whether mouse is hovering over A/M button
    pub am_hovered: bool,
    /// Whether mouse is hovering over mode button
    pub mode_hovered: bool,
}

impl Default for ScaleCornerState {
    fn default() -> Self {
        Self {
            scale_mode: ScaleMode::Auto,
            mode_label: "lin".to_string(),
            am_hovered: false,
            mode_hovered: false,
        }
    }
}

/// Hit zones for scale corner buttons
#[derive(Clone, Debug, Default)]
pub struct ScaleCornerHitZones {
    /// A/M button rect
    pub am_button: LayoutRect,
    /// Mode button rect
    pub mode_button: LayoutRect,
}

impl ScaleCornerHitZones {
    /// Check if a point hits the A/M button
    pub fn hits_am(&self, x: f64, y: f64) -> bool {
        self.am_button.contains(x, y)
    }

    /// Check if a point hits the mode button
    pub fn hits_mode(&self, x: f64, y: f64) -> bool {
        self.mode_button.contains(x, y)
    }

    /// Check what button (if any) is hit
    pub fn hit_test(&self, x: f64, y: f64) -> ScaleCornerButton {
        if self.hits_am(x, y) {
            ScaleCornerButton::AutoManual
        } else if self.hits_mode(x, y) {
            ScaleCornerButton::Mode
        } else {
            ScaleCornerButton::None
        }
    }
}

/// Which button in scale corner was clicked
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleCornerButton {
    /// A/M toggle button
    AutoManual,
    /// Mode cycle button (lin/log/%)
    Mode,
    /// No button
    None,
}
