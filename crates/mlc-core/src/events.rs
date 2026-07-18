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

    /// Programmatic symbol change for a specific chart, bypassing the
    /// symbol-search modal. Emitted by headless drivers (test harness,
    /// agent HTTP API) so external clients can switch instrument without
    /// touching the modal flow. Handler updates the bubble, re-subscribes
    /// streams, propagates via sync group — same effect as user-driven
    /// symbol search confirmation.
    ApplySymbol {
        chart_id: u64,
        symbol: String,
        exchange: String,
        account_type: String,
    },

    /// Canonical compare-overlay-add entry point. Emitted by the compare
    /// search modal's item-click and reachable headlessly (agent HTTP API,
    /// test harness) the same way `ApplySymbol` is for the main symbol.
    /// Operates on the active chart container (same target resolution the
    /// compare toolbar button/search modal has always used — unlike
    /// `ApplySymbol` this does not take an explicit `chart_id`, since
    /// compare-add has never been window-addressable from outside the
    /// active-container context). Handler resolves the compare symbol's
    /// venue against `exchange`/`account_type`, attempts a cache-hit add
    /// (`ChartContainer::add_compare_symbol`) with undo recording on
    /// success, or queues a `PendingCompareAdd` retry + fires an explicit
    /// cross-exchange kline request on a cache miss — see the doc comment
    /// on the handler in `chart_out_events.rs` for the cross-exchange
    /// gotcha this replicates.
    AddCompareSymbol {
        symbol: String,
        exchange: String,
        account_type: String,
    },

    // === Sidebar / Panel toggles (app-level) ===
    ToggleWatchlist,
    ToggleAlerts,
    ToggleObjectTree,
    ToggleSignals,
    /// Toggle the merged Monitoring panel (former separate Connectors +
    /// Performance panels/buttons, merged 2026-07-18).
    ToggleMonitoring,
    ToggleAgents,
    /// Toggle a typed slot sidebar (0..=4: Trading, Dom, Tape, L2, BigTrades —
    /// see `mlc_app_sidebar_content::free_slot::SlotKind`). Each slot hosts a
    /// chart-preset-scoped `DockingManager<Panel>` locked to its own panel
    /// kind. The profile only tracks open/closed per slot; content lives in
    /// the preset. First open of an empty slot auto-seeds one default panel.
    ToggleSlot(u8),
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

    // === Split mode toggle ===
    /// Toggle "Split Without Group": when enabled, split operations create the
    /// new window as an independent auto-created group instead of sharing the
    /// source window's tag.
    InternalToggleSplitUntagged,

    // === Sync options (cross-panel sync) ===
    /// Toggle symbol synchronisation across panels.
    InternalToggleSyncSymbol,
    /// Toggle timeframe synchronisation across panels.
    InternalToggleSyncTimeframe,
    /// Toggle crosshair synchronisation across panels.
    InternalToggleSyncCrosshair,
    /// Toggle viewport synchronisation across panels.
    InternalToggleSyncViewport,
    /// Toggle drawings synchronisation across panels.
    InternalToggleSyncDrawings,
    /// Toggle indicators synchronisation across panels.
    InternalToggleSyncIndicators,

    // === Timeframe ===
    /// Request a timeframe change (e.g., from toolbar dropdown selection).
    ChangeTimeframe { timeframe_id: String },

    // === Chart Type ===
    /// Request a chart type change (e.g., from chart_type_selector dropdown).
    ChangeChartType { chart_type: String },

    /// Commit a numeric parameter for a derived chart type (range_bar /
    /// volume_bar). Emitted by the param-input popup on Enter; the handler
    /// stores the value on the panel and re-issues the bridge subscription
    /// with the new threshold.
    UpdateChartTypeParam { value: f64 },

    /// Switch renko/kagi's box-size computation to ATR(14) mode and
    /// recompute `chart_type_param` from the live ATR value (granularity
    /// arc, кусок 2). Emitted by the resolution dropdown's "ATR (14)" item
    /// (renko/kagi only); handler sets `chart_type_param_mode = Atr14`,
    /// computes the box via `BackendCore::atr14_box_ticks`, and — on
    /// success — re-subscribes through the same path `UpdateChartTypeParam`
    /// uses. See `mlc-app::input::chart_out_events`'s handler doc comment
    /// for the exact recompute-cadence contract (explicit pick / chart-type
    /// switch INTO the mode only, never continuous).
    SetChartTypeParamAtr14,

    /// Commit the kline interval for the `"footprint"` chart type. Emitted
    /// by the interval-picker popup; handler stores it on the panel and
    /// re-issues the footprint subscription.
    UpdateChartTypeInterval { interval: String },

    /// Commit a new row-grouping preset (0/2/5/10 ticks, canon-fix wave 2
    /// item 5) for the `"footprint"` chart type. Emitted by the Instrument
    /// tab's "Row grouping" dropdown; handler converts `ticks` to a price
    /// step via the instrument tick size and re-folds `footprint_bars`
    /// through a fresh grouped fold machine, zero network.
    UpdateFootprintGroupTicks { ticks: u32 },

    // === Granularity favorites (user-granularity-templates-inner arc,
    // кусок 2 — owner spec 2026-07-16) — CUSTOM values (user-typed TF names
    // / numeric params) and STARRED rows (builtin or custom), persisted on
    // `UserProfile::domain::granularity_favorites`. Replaces кусок 1's
    // toggle-by-retype `ToggleTimeframeFavorite`/`ToggleParamFavorite`. ===
    /// Add (or, in edit mode — see `ChartApp::granularity_edit` — replace) a
    /// CUSTOM granularity value and APPLY it (dispatches `ChangeTimeframe`
    /// for `Tf`, `UpdateChartTypeParam` for `Param`, same as clicking a
    /// builtin row). Committing an already-existing custom value just
    /// applies it — dedup, no delete (no toggle-by-retype).
    CommitGranularityCustom { target: crate::chart_type::GranularityTarget },
    /// Remove a CUSTOM granularity value. Also removes it from the starred
    /// list if present (a deleted custom can't stay starred). Does NOT
    /// apply/change the active TF or param.
    DeleteGranularityCustom { target: crate::chart_type::GranularityTarget },
    /// Flip star membership for a `timeframe_selector` row — builtin or
    /// custom, TF or param. Does NOT apply/change the active TF or param.
    ToggleGranularityStar { target: crate::chart_type::GranularityTarget },
    /// Flip `ProfileUiSettings::pin_granularity_favorites` — user-
    /// granularity-templates-inner arc, кусок 3 (owner spec 2026-07-16).
    /// Emitted by the pin icon on the `timeframe_selector` dropdown's
    /// "FAVORITES" header row. Pure presentation toggle — does not touch
    /// `granularity_favorites` itself.
    ToggleGranularityPin,
    /// Move one row within a granularity FAVORITES/CUSTOM list from
    /// `from_index` to `to_index` — drag-to-reorder (owner spec 2026-07-16
    /// items 2+3). Indices are scoped to the exact rendered subset
    /// `section`+`family` identifies: for `GranularityFamily::Param`, the
    /// handler filters `GranularityFavorites::{params,starred_params}` by
    /// `chart_type` FIRST, reorders within that filtered view, then writes
    /// the result back at the matching absolute positions in the flat vec —
    /// reordering one chart_type's params must never disturb another
    /// chart_type's entries interleaved in the same flat vec. Does NOT
    /// change the reordered value's applied/starred status.
    ReorderGranularity {
        family: crate::chart_type::GranularityFamily,
        section: crate::chart_type::GranularitySection,
        from_index: usize,
        to_index: usize,
    },

    // === Overlays (C10 Volume Profile family) ===
    /// Toggle the Volume Profile right-side overlay on the active chart.
    ToggleVolumeProfileOverlay,
    /// Cycle the Volume Profile aggregation mode
    /// (VisibleRange → Session → Composite → VisibleRange).
    CycleVolumeProfileMode,

    // === TPO Profile overlay (item D) — mirrors the Volume Profile toggle
    // shape above; block-histogram time-at-price profile, distinct from the
    // letter-based TPO Market Profile chart type. ===
    /// Toggle the TPO Profile right-side block-histogram overlay on the
    /// active chart.
    ToggleTpoProfileOverlay,

    // === C15: OI Delta sub-pane + Funding Rate strip ===
    /// Toggle the Open Interest Δ sub-pane on the active chart.
    ToggleOpenInterestOverlay,
    /// Toggle the Funding Rate strip at the bottom of the active chart.
    ToggleFundingRateStrip,

    // === DOM Heatmap (C12) / Liquidation Heatmap (C13) — chart-type →
    // overlay migration: both were `chart_type` registry entries, now
    // builtin overlay indicators that draw under the main series of ANY
    // chart type. Mirrors the OI Delta / Funding Rate toggle shape above. ===
    /// Toggle the DOM Heatmap background overlay on the active chart.
    ToggleDomHeatmapOverlay,
    /// Toggle the Liquidation Heatmap background overlay on the active chart.
    ToggleLiquidationHeatmapOverlay,

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
    /// Load a preset by its id INTO the currently-active tab's slot in
    /// `open_tabs` — the active preset id is replaced in place.  Used by the
    /// toolbar presets dropdown and ChartBrowser modal's "load" button:
    /// owner UX = top-toolbar actions act on the active chart slot, never
    /// spawn a new tab.  Distinct from `LoadPreset` (which is what chrome
    /// tab clicks emit — that path must not mutate `open_tabs`).
    LoadPresetInActiveSlot { id: String },
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

    // === Undo / Redo ===
    /// Perform one undo step (mirrors Ctrl+Z keyboard path).
    Undo,
    /// Perform one redo step (mirrors Ctrl+Y keyboard path).
    Redo,

    /// Capture a screenshot of the current chart on the next rendered frame
    /// (mirrors Ctrl+S keyboard path).
    Screenshot,

    // === Consumed (chart handled it internally) ===
    /// The chart fully handled this action. No app-level work needed.
    Consumed,
}
