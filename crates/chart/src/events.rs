//! Chart output events.
//!
//! Events emitted by the chart to the host application when it cannot
//! handle an action internally (e.g., symbol search needs app-level data,
//! sidebar toggles are app-level, window management is app-level).

/// Events emitted by a chart panel to the host application.
#[derive(Debug, Clone)]
pub enum ChartOutEvent {
    // === Symbol ===
    /// Request the app to open the symbol search modal.
    OpenSymbolSearch,
    /// Request the app to open the compare symbol search modal.
    OpenCompareSearch,
    /// Chart changed its symbol internally (for cross-window sync).
    SymbolChanged { symbol: String },

    // === Sidebar / Panel toggles (app-level) ===
    ToggleWatchlist,
    ToggleAlerts,
    ToggleObjectTree,
    ToggleSignals,
    ToggleConnectors,
    TogglePerformance,
    ToggleIndicators,
    ToggleTradingPanel,
    TogglePositions,
    ToggleLeftPanel,
    ToggleThemeSettings,

    // === Window management ===
    SpawnWindow,
    CloseWindow,

    // === Layout (terminal-level) ===
    ExpandPanel,
    SplitHorizontal,
    SplitVertical,

    // === Chart-internal split/expand (handled within the chart crate) ===
    /// Set layout to single panel (no splits).
    InternalSetLayoutSingle,
    /// Split the active sub-chart horizontally (left | right).
    InternalSplitHorizontal,
    /// Split the active sub-chart vertically (top / bottom).
    InternalSplitVertical,
    /// Split into 2x2 grid (4 panels).
    InternalSplitGrid2x2,
    /// Split into 2-left 1-right (3 panels).
    InternalSplit2Left1Right,
    /// Split into 1-left 2-right (3 panels).
    InternalSplit1Left2Right,
    /// Split into 2-top 1-bottom (3 panels).
    InternalSplit2Top1Bottom,
    /// Split into 1-top 2-bottom (3 panels).
    InternalSplit1Top2Bottom,
    /// Split into 3 vertical columns.
    InternalSplit3Columns,
    /// Split into 3 horizontal rows.
    InternalSplit3Rows,
    /// Split into 1-big 3-small (4 panels).
    InternalSplit1Big3Small,
    /// Toggle expand: maximise the active sub-chart or restore the split view.
    InternalToggleExpand,
    /// Close the active sub-chart panel.
    InternalClosePanel,
    /// Reset panel sizes to equal proportions.
    InternalResetSizes,

    // === Sync options (cross-panel sync) ===
    /// Toggle symbol synchronisation across panels.
    InternalToggleSyncSymbol,
    /// Toggle timeframe synchronisation across panels.
    InternalToggleSyncTimeframe,
    /// Toggle crosshair synchronisation across panels.
    InternalToggleSyncCrosshair,
    /// Toggle viewport (time range) synchronisation across panels.
    InternalToggleSyncViewport,

    // === Timeframe ===
    /// Request a timeframe change (e.g., from toolbar dropdown selection).
    ChangeTimeframe { timeframe_id: String },

    // === Chart Type ===
    /// Request a chart type change (e.g., from chart_type_selector dropdown).
    ChangeChartType { chart_type: String },

    // === Theme (app-global) ===
    SetTheme(&'static str),
    SetStyle(&'static str),
    OpenThemeSettings,

    // === Chart settings modal ===
    /// Request the app to open the chart settings modal.
    OpenChartSettings,

    // === User settings modal ===
    /// Request the app to open (or toggle) the user settings modal.
    OpenUserSettings,

    // === Quick-settings toggles (handled by chart-app) ===
    /// Toggle grid visibility on the active chart window.
    ToggleGrid,
    /// Toggle vertical grid lines on the active chart window.
    ToggleGridVertical,
    /// Toggle horizontal grid lines on the active chart window.
    ToggleGridHorizontal,
    /// Toggle crosshair visibility on the active chart window.
    ToggleCrosshair,
    /// Toggle legend visibility on the active chart window.
    ToggleLegend,
    /// Toggle OHLC display in the legend.
    ToggleLegendOHLC,
    /// Toggle change display in the legend.
    ToggleLegendChange,
    /// Toggle percent display in the legend.
    ToggleLegendPercent,
    /// Toggle tooltip visibility.
    ToggleTooltip,
    /// Toggle tooltip follow-cursor mode.
    ToggleTooltipFollow,
    /// Toggle watermark visibility on the active chart window.
    ToggleWatermark,
    /// Set watermark text.
    SetWatermarkText(&'static str),
    /// Set watermark position ("center", "bottom_left", "bottom_right").
    SetWatermarkPosition(&'static str),

    // === Presets ===
    /// Save the current workspace as a named preset.
    SavePreset { name: String },
    /// Load a preset by its id.
    LoadPreset { id: String },
    /// Delete a preset by its id.
    DeletePreset { id: String },
    /// Close a tab without deleting the preset.
    CloseTab { id: String },
    /// Open a preset as a new tab (used by + dropdown).
    OpenTab { id: String },
    /// Rename a preset.
    RenamePreset { id: String, new_name: String },
    /// Open the preset name input modal in "Save As" mode.
    OpenPresetSaveAs,
    /// Open the preset name input modal in "Rename" mode for the active preset.
    OpenPresetRename,
    /// Open the preset name input modal in "New Chart" mode (ask for name before clearing).
    OpenPresetNewChart,
    /// Re-save snapshot into the currently active preset.
    SaveCurrentPreset,
    /// Toggle autosave on/off.
    ToggleAutosave,
    /// Open the chart browser modal (list all presets).
    OpenChartBrowser,
    /// Open the chart browser modal with "open in new tab" flag set.
    /// When the user selects a preset, it opens as a new tab instead of replacing the active one.
    OpenChartBrowserInNewTab,
    /// Create a new blank chart (auto-save current if unsaved).
    NewChart,

    // === Consumed (chart handled it internally) ===
    /// The chart fully handled this action. No app-level work needed.
    Consumed,
}
