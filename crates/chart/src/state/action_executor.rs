//! Chart-domain action executor
//!
//! Handles `ChartAction` variants that are pure chart concerns:
//! drawing tool selection, drawing manager operations, modal opens, toggles,
//! zoom, and display option mutations.
//!
//! The terminal-level `execute_action()` in `zengeld-terminal-core` calls
//! `execute_chart_action_internal()` first, then handles terminal-domain
//! actions (panel management, sidebar toggles, window layout, etc.).
//!
//! The standalone `execute_chart_action()` function operates directly on
//! `ChartWindow` and returns `Vec<ChartExternalEvent>` for any actions that
//! require terminal coordination (symbol change, theme change, etc.).

use crate::engine::input::actions::ChartAction;
use crate::ui::modal_state::{ModalState, OpenModal};
use crate::layout::ToolbarState;
use crate::drawing::DrawingManager;
use crate::state::chart_window::ChartWindow;

/// Result of executing a `ChartAction`.
///
/// Returned by both `execute_chart_action_internal()` (chart crate) and
/// `execute_action()` (core crate) so callers can react uniformly.
#[derive(Debug, Clone)]
pub enum ActionResult {
    /// Action was executed successfully; no UI repaint needed beyond normal.
    Handled,
    /// Action requires a UI update (e.g., a modal was opened).
    UIUpdate,
    /// Action was not recognised by this executor; the caller should try
    /// the next handler in the chain.
    NotHandled,
}

/// Events that chart cannot handle alone — the terminal must coordinate.
///
/// Returned by `execute_chart_action()` when an action requires app-level
/// context (data reload, theme switching, panel management, etc.).
#[derive(Debug, Clone)]
pub enum ChartExternalEvent {
    /// Symbol change requested; app should reload data and sync other windows.
    RequestSymbolChange(String),
    /// Timeframe change requested; app should reload data.
    RequestTimeframeChange(String),
    /// Chart type changed; app should update toolbar icon.
    ChartTypeChanged(String),
    /// Theme change requested.
    ThemeChangeRequested(String),
    /// Style change requested.
    StyleChangeRequested(String),
    /// A modal should be opened by the terminal.
    OpenModal(OpenModalRequest),
    /// Action not handled by chart — pass to terminal for further dispatch.
    NotHandled,
}

/// Which modal the terminal should open on behalf of chart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenModalRequest {
    IndicatorSearch,
    ChartSettings,
    GeneralSettings,
    SymbolSearch,
    SymbolHistory,
    Compare,
}

/// Execute chart-domain actions operating directly on a `ChartWindow`.
///
/// Handles the full set of `ChartAction` variants that belong to the chart
/// crate: series type, toggles, setters, zoom, drawing tool mutations.
///
/// Returns a `Vec<ChartExternalEvent>`:
/// - Empty vec — action was fully handled by this function.
/// - One or more events — the terminal must take further action (data reload,
///   modal open, theme switch, window management, etc.).
///
/// Actions that are purely terminal-domain (panel management, sidebar
/// toggles, window spawn/close, sync settings) return
/// `vec![ChartExternalEvent::NotHandled]`.
pub fn execute_chart_action(
    action: &ChartAction,
    window: &mut ChartWindow,
) -> Vec<ChartExternalEvent> {
    match action {
        // =====================================================================
        // Series / Chart Type
        // =====================================================================
        ChartAction::SetChartType(ct) => {
            let type_name = ct.to_string();
            window.set_chart_type_with_undo(ct);
            eprintln!("[Chart] Chart type set to {}", ct);
            vec![ChartExternalEvent::ChartTypeChanged(type_name)]
        }

        ChartAction::ToggleCandles => {
            window.set_chart_type_with_undo("candles");
            eprintln!("[Chart] Chart type -> candles");
            vec![ChartExternalEvent::ChartTypeChanged("candles".to_string())]
        }
        ChartAction::ToggleLine => {
            window.set_chart_type_with_undo("line");
            eprintln!("[Chart] Chart type -> line");
            vec![ChartExternalEvent::ChartTypeChanged("line".to_string())]
        }
        ChartAction::ToggleArea => {
            window.set_chart_type_with_undo("area");
            eprintln!("[Chart] Chart type -> area");
            vec![ChartExternalEvent::ChartTypeChanged("area".to_string())]
        }
        ChartAction::ToggleHistogram => {
            window.set_chart_type_with_undo("histogram");
            eprintln!("[Chart] Chart type -> histogram");
            vec![ChartExternalEvent::ChartTypeChanged("histogram".to_string())]
        }
        ChartAction::ToggleBaseline => {
            window.set_chart_type_with_undo("baseline");
            eprintln!("[Chart] Chart type -> baseline");
            vec![ChartExternalEvent::ChartTypeChanged("baseline".to_string())]
        }

        // =====================================================================
        // Overlay Toggles
        // =====================================================================
        ChartAction::ToggleLegend => {
            window.toggle_legend();
            eprintln!("[Chart] Legend toggled -> {}", window.legend.visible);
            vec![]
        }
        ChartAction::ToggleGrid => {
            window.toggle_grid();
            eprintln!("[Chart] Grid toggled");
            vec![]
        }
        ChartAction::ToggleCrosshair => {
            window.toggle_crosshair();
            eprintln!("[Chart] Crosshair toggled -> {}", window.crosshair.enabled);
            vec![]
        }
        ChartAction::ToggleMagnet => {
            window.crosshair.toggle_magnet();
            eprintln!("[Chart] Magnet toggled");
            vec![]
        }
        ChartAction::ToggleWatermark => {
            window.toggle_watermark();
            eprintln!("[Chart] Watermark toggled");
            vec![]
        }
        ChartAction::ToggleGridVertical => {
            window.toggle_grid_vertical();
            eprintln!("[Chart] Grid vertical toggled -> {}", window.grid_options.vert_lines.visible);
            vec![]
        }
        ChartAction::ToggleGridHorizontal => {
            window.toggle_grid_horizontal();
            eprintln!("[Chart] Grid horizontal toggled -> {}", window.grid_options.horz_lines.visible);
            vec![]
        }
        ChartAction::ToggleTooltip => {
            window.toggle_tooltip();
            eprintln!("[Chart] Tooltip toggled -> {}", window.tooltip.visible);
            vec![]
        }
        ChartAction::ToggleTooltipFollow => {
            window.toggle_tooltip_follow();
            eprintln!("[Chart] Tooltip follow toggled -> {}", window.tooltip.follow_cursor);
            vec![]
        }
        ChartAction::ToggleCrosshairVertLine => {
            window.toggle_crosshair_vert_line();
            eprintln!("[Chart] Crosshair vert line toggled");
            vec![]
        }
        ChartAction::ToggleCrosshairHorzLine => {
            window.toggle_crosshair_horz_line();
            eprintln!("[Chart] Crosshair horz line toggled");
            vec![]
        }
        ChartAction::ToggleLegendOHLC => {
            window.toggle_legend_ohlc();
            eprintln!("[Chart] Legend OHLC toggled -> {}", window.legend.show_ohlc);
            vec![]
        }
        ChartAction::ToggleLegendChange => {
            window.toggle_legend_change();
            eprintln!("[Chart] Legend change toggled -> {}", window.legend.show_change);
            vec![]
        }
        ChartAction::ToggleLegendPercent => {
            window.toggle_legend_percent();
            eprintln!("[Chart] Legend percent toggled -> {}", window.legend.show_percent);
            vec![]
        }

        // Crosshair label toggles — toggle the options flags directly.
        ChartAction::ToggleCrosshairVertLabel => {
            window.crosshair_options.vert_line.label_visible =
                !window.crosshair_options.vert_line.label_visible;
            eprintln!("[Chart] Crosshair vert label toggled -> {}",
                window.crosshair_options.vert_line.label_visible);
            vec![]
        }
        ChartAction::ToggleCrosshairHorzLabel => {
            window.crosshair_options.horz_line.label_visible =
                !window.crosshair_options.horz_line.label_visible;
            eprintln!("[Chart] Crosshair horz label toggled -> {}",
                window.crosshair_options.horz_line.label_visible);
            vec![]
        }

        // =====================================================================
        // Grid Visibility Setters
        // =====================================================================
        ChartAction::SetGridHorzVisible(v) => {
            window.grid_options.horz_lines.visible = *v;
            eprintln!("[Chart] Grid horz visible -> {}", v);
            vec![]
        }
        ChartAction::SetGridVertVisible(v) => {
            window.grid_options.vert_lines.visible = *v;
            eprintln!("[Chart] Grid vert visible -> {}", v);
            vec![]
        }

        // =====================================================================
        // Setters
        // =====================================================================
        ChartAction::SetGridStyle(s) => {
            window.set_grid_style(*s);
            eprintln!("[Chart] Grid style set");
            vec![]
        }
        ChartAction::SetCrosshairMode(m) => {
            window.set_crosshair_mode(*m);
            eprintln!("[Chart] Crosshair mode set to {:?}", m);
            vec![]
        }
        ChartAction::SetCrosshairStyle(s) => {
            window.set_crosshair_style(*s);
            eprintln!("[Chart] Crosshair style set");
            vec![]
        }
        ChartAction::SetLegendPosition(p) => {
            window.set_legend_position(*p);
            eprintln!("[Chart] Legend position set to {:?}", p);
            vec![]
        }
        ChartAction::SetWatermarkPosition(h, v) => {
            window.set_watermark_position(*h, *v);
            eprintln!("[Chart] Watermark position set");
            vec![]
        }
        ChartAction::SetWatermarkColor(c) => {
            window.set_watermark_color(c);
            eprintln!("[Chart] Watermark color set to {}", c);
            vec![]
        }
        ChartAction::SetWatermarkText(t) => {
            window.set_watermark_text(t);
            eprintln!("[Chart] Watermark text set to {}", t);
            vec![]
        }

        // =====================================================================
        // Zoom / Viewport
        // =====================================================================
        ChartAction::ZoomIn => {
            window.zoom_in();
            eprintln!("[Chart] Zoom in");
            vec![]
        }
        ChartAction::ZoomOut => {
            window.zoom_out();
            eprintln!("[Chart] Zoom out");
            vec![]
        }
        ChartAction::FitContent => {
            window.fit_content();
            eprintln!("[Chart] Fit content");
            vec![]
        }
        ChartAction::ResetZoom => {
            window.reset_zoom();
            eprintln!("[Chart] Reset zoom");
            vec![]
        }

        // =====================================================================
        // Drawing Tools
        // =====================================================================
        ChartAction::SelectTool(tool) => {
            window.drawing_manager.set_tool(Some(tool));
            let is_cursor_tool = matches!(*tool, "crosshair" | "hand" | "cursor" | "none");
            if is_cursor_tool {
                eprintln!("[Chart] Switched to cursor mode: {}", tool);
            } else {
                eprintln!("[Chart] DrawingManager tool set: {}", tool);
            }
            vec![]
        }
        ChartAction::ToggleLockDrawings => {
            window.drawing_manager.toggle_lock();
            eprintln!("[Chart] DrawingManager lock toggled");
            vec![]
        }
        ChartAction::ToggleLockSelected => {
            // Lock/unlock the currently selected primitive (if any)
            if let Some(idx) = window.drawing_manager.selected() {
                window.drawing_manager.toggle_lock_primitive(idx);
                eprintln!("[Chart] DrawingManager toggled lock for selected primitive at {}", idx);
            }
            vec![]
        }
        ChartAction::ToggleDrawingsVisible => {
            window.drawing_manager.set_visible(!window.drawing_manager.is_visible());
            eprintln!("[Chart] DrawingManager visibility toggled");
            vec![]
        }
        ChartAction::DeleteSelected => {
            if let Some(idx) = window.drawing_manager.selected() {
                window.drawing_manager.delete_at(idx);
                eprintln!("[Chart] DrawingManager deleted primitive at {}", idx);
            }
            vec![]
        }
        ChartAction::DeleteAll => {
            window.drawing_manager.clear();
            eprintln!("[Chart] DrawingManager cleared all primitives");
            vec![]
        }

        // =====================================================================
        // Price Lines / Markers
        // =====================================================================
        ChartAction::AddPriceLine => {
            // Price line creation needs user-supplied price — no-op here, terminal handles
            eprintln!("[Chart] AddPriceLine - delegating to terminal");
            vec![ChartExternalEvent::NotHandled]
        }
        ChartAction::ClearPriceLines => {
            window.price_lines.clear();
            eprintln!("[Chart] Cleared all price lines");
            vec![]
        }
        ChartAction::AddMarker => {
            // Marker creation needs user interaction — terminal handles
            eprintln!("[Chart] AddMarker - delegating to terminal");
            vec![ChartExternalEvent::NotHandled]
        }
        ChartAction::ClearMarkers => {
            window.marker_manager.clear();
            eprintln!("[Chart] Cleared all markers");
            vec![]
        }

        // =====================================================================
        // Data
        // =====================================================================
        ChartAction::RegenerateData => {
            // Data regeneration requires app context (data provider, async)
            eprintln!("[Chart] RegenerateData - delegating to terminal");
            vec![ChartExternalEvent::NotHandled]
        }

        // =====================================================================
        // Undo / Redo
        // =====================================================================
        ChartAction::Undo => {
            // Command application stays at app level — just signal intent.
            // The terminal reads window.command_history directly.
            eprintln!("[Chart] Undo requested (can_undo: {})", window.command_history.can_undo());
            vec![ChartExternalEvent::NotHandled]
        }
        ChartAction::Redo => {
            eprintln!("[Chart] Redo requested (can_redo: {})", window.command_history.can_redo());
            vec![ChartExternalEvent::NotHandled]
        }

        // =====================================================================
        // Symbol / Timeframe — require terminal data reload
        // =====================================================================
        ChartAction::SetSymbol(sym) => {
            eprintln!("[Chart] Symbol change requested: {}", sym);
            vec![ChartExternalEvent::RequestSymbolChange(sym.to_string())]
        }
        ChartAction::SetTimeframe(tf) => {
            eprintln!("[Chart] Timeframe change requested: {}", tf);
            vec![ChartExternalEvent::RequestTimeframeChange(tf.to_string())]
        }

        // =====================================================================
        // Dialogs / Modals — terminal opens these
        // =====================================================================
        ChartAction::OpenIndicators => {
            eprintln!("[Chart] Open IndicatorSearch modal (delegating)");
            vec![ChartExternalEvent::OpenModal(OpenModalRequest::IndicatorSearch)]
        }
        ChartAction::OpenCompare => {
            eprintln!("[Chart] Open Compare (delegating)");
            vec![ChartExternalEvent::OpenModal(OpenModalRequest::Compare)]
        }
        ChartAction::OpenSettings => {
            eprintln!("[Chart] Open ChartSettings modal (delegating)");
            vec![ChartExternalEvent::OpenModal(OpenModalRequest::ChartSettings)]
        }
        ChartAction::OpenThemeSettings => {
            eprintln!("[Chart] Open GeneralSettings modal (delegating)");
            vec![ChartExternalEvent::OpenModal(OpenModalRequest::GeneralSettings)]
        }
        ChartAction::OpenSymbolSearch => {
            eprintln!("[Chart] Open SymbolSearch (delegating)");
            vec![ChartExternalEvent::OpenModal(OpenModalRequest::SymbolSearch)]
        }
        ChartAction::OpenSymbolHistory => {
            eprintln!("[Chart] Open SymbolHistory (delegating)");
            vec![ChartExternalEvent::OpenModal(OpenModalRequest::SymbolHistory)]
        }

        // =====================================================================
        // Theme / Style — require terminal theme manager
        // =====================================================================
        ChartAction::SetTheme(theme) => {
            eprintln!("[Chart] Theme change requested: {}", theme);
            vec![ChartExternalEvent::ThemeChangeRequested(theme.to_string())]
        }
        ChartAction::SetStyle(style) => {
            eprintln!("[Chart] Style change requested: {}", style);
            vec![ChartExternalEvent::StyleChangeRequested(style.to_string())]
        }

        // =====================================================================
        // Terminal-only actions — no chart state to mutate
        // =====================================================================
        ChartAction::ToggleObjectTree
        | ChartAction::ToggleSignals
        | ChartAction::ToggleIndicators
        | ChartAction::ToggleLeftPanel
        | ChartAction::ToggleRightPanel
        | ChartAction::ToggleBottomPanel
        | ChartAction::ToggleWatchlist
        | ChartAction::ToggleAlerts
        | ChartAction::ToggleTradingPanel
        | ChartAction::TogglePositions
        | ChartAction::Custom(_) => {
            eprintln!("[Chart] Action {:?} is terminal-domain, passing through", action);
            vec![ChartExternalEvent::NotHandled]
        }
    }
}

/// Execute chart-domain actions (legacy API).
///
/// Handles the subset of `ChartAction` variants that belong purely to the
/// chart crate: drawing tool selection, drawing manager mutations, and
/// modal open/close operations.
///
/// Returns `ActionResult::NotHandled` for any action that is not a
/// chart-domain concern, allowing the terminal layer to handle it.
pub fn execute_chart_action_internal(
    action: &ChartAction,
    drawing_manager: &mut DrawingManager,
    modal_state: &mut ModalState,
    toolbar_state: &mut ToolbarState,
) -> ActionResult {
    match action {
        // =====================================================================
        // Drawing Tools -> DrawingManager + ToolbarState
        // =====================================================================
        ChartAction::SelectTool(tool) => {
            drawing_manager.set_tool(Some(tool));

            // Cursor tools (crosshair, hand) are navigation modes, not drawing tools.
            // They clear primed/active state instead of becoming the active tool.
            let is_cursor_tool = matches!(*tool, "crosshair" | "hand" | "cursor" | "none");
            if is_cursor_tool {
                toolbar_state.active_tool_id = None;
                toolbar_state.primed_id = None;
                eprintln!("[Chart] Switched to cursor mode: {}", tool);
            } else {
                toolbar_state.active_tool_id = Some(tool.to_string());
                eprintln!("[Chart] DrawingManager tool set: {}", tool);
            }
            ActionResult::Handled
        }

        ChartAction::ToggleLockDrawings => {
            drawing_manager.toggle_lock();
            eprintln!("[Chart] DrawingManager lock toggled");
            ActionResult::Handled
        }

        ChartAction::ToggleDrawingsVisible => {
            drawing_manager.set_visible(!drawing_manager.is_visible());
            eprintln!("[Chart] DrawingManager visibility toggled");
            ActionResult::Handled
        }

        ChartAction::DeleteSelected => {
            if let Some(idx) = drawing_manager.selected() {
                drawing_manager.delete_at(idx);
                eprintln!("[Chart] DrawingManager deleted primitive at {}", idx);
            }
            ActionResult::Handled
        }

        ChartAction::DeleteAll => {
            drawing_manager.clear();
            eprintln!("[Chart] DrawingManager cleared all primitives");
            ActionResult::Handled
        }

        // =====================================================================
        // Modals -> ModalState
        // =====================================================================
        ChartAction::OpenIndicators => {
            modal_state.open(OpenModal::IndicatorSearch);
            eprintln!("[Chart] Opened IndicatorSearch modal");
            ActionResult::UIUpdate
        }

        ChartAction::OpenCompare => {
            // Compare symbol search is not fully implemented yet.
            eprintln!("[Chart] OpenCompare - not implemented yet");
            ActionResult::NotHandled
        }

        ChartAction::OpenSettings => {
            // chart_settings_state is not accessible from execute_action's context.
            // These actions are fully handled by execute_with_events which emits
            // ChartExternalEvent::OpenModal(ChartSettings) -> app's chart_settings_state.toggle().
            // This branch is unreachable when execute_with_events runs first, but return
            // NotHandled as a safety net to avoid the stale modal_state path.
            ActionResult::NotHandled
        }

        ChartAction::OpenThemeSettings => {
            // Same as OpenSettings: handled by execute_with_events -> OpenModal(GeneralSettings).
            ActionResult::NotHandled
        }

        // OpenSymbolSearch requires populating search results from SymbolSearch,
        // which is a terminal-level type.  Return NotHandled so the terminal
        // executor can handle it with full context.
        ChartAction::OpenSymbolSearch => ActionResult::NotHandled,

        // All other actions are not chart-domain concerns.
        _ => ActionResult::NotHandled,
    }
}
