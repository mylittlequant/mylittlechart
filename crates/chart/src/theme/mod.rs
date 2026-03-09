//! Theme system - static presets and runtime configuration
//!
//! # Architecture
//!
//! This module provides chart-specific theming. Terminal-specific UI theming
//! (toolbars, buttons, modals) should be managed by the terminal crate.
//!
//! ## Chart-Specific (this crate)
//!
//! - **ChartTheme**: Complete chart theme (colors + series + fonts)
//! - **ChartColors**: Background, grid, scales, crosshair, legend, watermark
//! - **SeriesColors**: Candles, line, area, histogram, baseline, bars, volume
//! - **ChartFonts**: Price scale, time scale, legend, crosshair, watermark fonts
//! - **StyleParams**: Opacity and visual effects (subpane backgrounds, etc.)
//!
//! ## Legacy Types (for backwards compatibility)
//!
//! - **UITheme**, **UIColors**, **UIFonts**, **UISizing**, **UIEffects**
//! - These are terminal-specific and should eventually be moved to terminal crate
//! - Terminal should create its own `TerminalTheme` that composes `ChartTheme`
//!
//! ## Files
//!
//! - **preset.rs**: Static theme presets (ChartTheme::dark(), light(), etc.)
//! - **runtime.rs**: Runtime-configurable theme (RuntimeTheme)
//! - **manager.rs**: ThemeManager - single source of truth for theme operations
//! - **style.rs**: UI styles (Solid, Glass, FrostedGlass, LiquidGlass)
//!
//! Styles are orthogonal to themes: Theme defines colors, Style defines how
//! those colors are applied (opacity, blur, effects).

mod preset;
mod runtime;
mod manager;
mod style;

// =============================================================================
// CHART-SPECIFIC TYPES (recommended for new code)
// =============================================================================

// New chart-specific theme types
pub use preset::{
    ChartTheme, ChartColors, SeriesColors, ChartFonts,
};

// =============================================================================
// LEGACY TYPES (backwards compatibility - terminal should migrate)
// =============================================================================

// Legacy UI theme types (terminal-specific)
pub use preset::{
    UITheme, UIColors, UIFonts, UISizing, UIEffects,
};

// Re-export runtime types
pub use runtime::{
    RuntimeTheme, RuntimeUIColors, RuntimeChartColors, RuntimeSeriesColors,
    RuntimeFonts, RuntimeSizing, RuntimeEffects,
};

// Re-export manager and settings panel types
pub use manager::{
    ThemeManager,
    ThemeSettingsPanel, ThemeSettingsSection, ThemeColorField, ThemeColorPath,
};

// Re-export style types (chart uses StyleParams for subpane opacity)
pub use style::{
    UIStyle, StyleParams, OpacityType, GlassButtonStyle,
};
