//! Chart panel toolbar definitions
//!
//! Defines the drawing tools toolbar that belongs to the chart panel.
//! This is the chart crate's local toolbar — the chart knows its own tools.

use uzor::panel_api::{
    PanelToolbarDef, ToolbarSectionDef, ToolbarItemDef, DropdownItemDef,
    ToolbarIconId, SectionAlign,
};
use crate::i18n::{
    ToolbarTooltipKey as TK, t_toolbar,
    ToolbarMenuKey as MK, t_toolbar_menu,
    PrimitiveNameKey as PN,
    current_language,
};

// Re-export orientation type for callers
pub use uzor::panel_api::ToolbarOrientation;

/// Build the chart's left-side drawing tools toolbar (vertical, 50px wide)
pub fn left_toolbar() -> PanelToolbarDef {
    PanelToolbarDef::vertical(vec![
        cursor_section(),
        line_section(),
        fib_section(),
        pattern_section(),
        brush_section(),
        annotation_section(),
        icon_section(),
        projection_section(),
        magnet_section(),
        lock_section(),
        visibility_section(),
        delete_section(),
    ]).with_size(crate::types::LEFT_TOOLBAR_WIDTH)
}

/// Build the chart's right-side sidebar toolbar (vertical, 50px wide)
///
/// Full version: includes all sidebar toggle buttons (watchlist, alerts, object tree, signals).
/// Used by the terminal where these buttons are backed by terminal infrastructure.
pub fn right_toolbar() -> PanelToolbarDef {
    PanelToolbarDef::vertical(vec![
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::icon_button("watchlist", ToolbarIconId::new("List"))
                .with_tooltip(t_toolbar(TK::Watchlist)),
            ToolbarItemDef::icon_button("alerts", ToolbarIconId::new("Bell"))
                .with_tooltip(t_toolbar(TK::Alerts)),
            ToolbarItemDef::icon_button("object_tree", ToolbarIconId::new("TreePine"))
                .with_tooltip(t_toolbar(TK::ObjectTree)),
            ToolbarItemDef::icon_button("signals", ToolbarIconId::new("Zap"))
                .with_tooltip(t_toolbar(TK::Signals)),
            ToolbarItemDef::icon_button("connectors", ToolbarIconId::new("CircuitBoard"))
                .with_tooltip(t_toolbar(TK::Connectors)),
            ToolbarItemDef::icon_button("performance", ToolbarIconId::new("Activity"))
                .with_tooltip(t_toolbar(TK::Performance)),
            ToolbarItemDef::icon_button("agents", ToolbarIconId::new("Bot"))
                .with_tooltip(t_toolbar(TK::Agents)),
            ToolbarItemDef::icon_button("slot1", ToolbarIconId::new("slot1"))
                .with_tooltip("Slot 1"),
            ToolbarItemDef::icon_button("slot2", ToolbarIconId::new("slot2"))
                .with_tooltip("Slot 2"),
            ToolbarItemDef::icon_button("slot3", ToolbarIconId::new("slot3"))
                .with_tooltip("Slot 3"),
            ToolbarItemDef::icon_button("slot4", ToolbarIconId::new("slot4"))
                .with_tooltip("Slot 4"),
        ]),
    ]).with_size(crate::types::LEFT_TOOLBAR_WIDTH)
}

/// Build the right-side toolbar for standalone / chart-app mode.
///
/// Identical to `right_toolbar()` — includes watchlist, alerts, object_tree and signals.
/// Only the `main_menu` (hamburger) is omitted from the top toolbar in standalone mode.
pub fn standalone_right_toolbar() -> PanelToolbarDef {
    right_toolbar()
}

/// Build the top toolbar for standalone / chart-app mode.
///
/// Omits the `main_menu` (hamburger) section which opens a sidebar panel that
/// requires terminal infrastructure (account management, exchange connections).
pub fn standalone_top_toolbar() -> PanelToolbarDef {
    PanelToolbarDef::horizontal(vec![
        // 1. Symbol selector + Compare (no main_menu section)
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::dropdown("symbol_selector", vec![])
                .with_icon(ToolbarIconId::new("Search"))
                .with_text("BTCUSD")
                .with_min_width(150.0)
                .with_tooltip(t_toolbar(TK::SymbolSelector)),
            ToolbarItemDef::icon_button("compare", ToolbarIconId::new("Plus"))
                .with_tooltip(t_toolbar(TK::Compare)),
        ]),
        // 2. Timeframe selector
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::dropdown("timeframe_selector", vec![
                DropdownItemDef::action("tf_1m", "1m"),
                DropdownItemDef::action("tf_3m", "3m"),
                DropdownItemDef::action("tf_5m", "5m"),
                DropdownItemDef::action("tf_15m", "15m"),
                DropdownItemDef::action("tf_30m", "30m"),
                DropdownItemDef::action("tf_1h", "1H"),
                DropdownItemDef::action("tf_2h", "2H"),
                DropdownItemDef::action("tf_4h", "4H"),
                DropdownItemDef::action("tf_6h", "6H"),
                DropdownItemDef::action("tf_12h", "12H"),
                DropdownItemDef::action("tf_1d", "1D"),
                DropdownItemDef::action("tf_1w", "1W"),
                DropdownItemDef::action("tf_1M", "1M"),
            ]).with_icon(ToolbarIconId::new("Clock"))
              .with_text("1H")
              .with_min_width(56.0)
              .with_tooltip(t_toolbar(TK::TimeframeSelector)),
        ]).with_separator(),
        // 3. Chart type selector
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::dropdown("chart_type_selector", chart_type_items())
                .with_icon(ToolbarIconId::new("Candlestick"))
                .with_tooltip(t_toolbar(TK::ChartType)),
        ]).with_separator(),
        // 4. Indicators
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::icon_button("indicators", ToolbarIconId::new("Indicators"))
                .with_tooltip(t_toolbar(TK::Indicators)),
        ]),
        // 5. Settings (dropdown — matches terminal settings_menu content)
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::dropdown("settings_menu", settings_menu_items())
                .with_icon(ToolbarIconId::new("Settings"))
                .with_tooltip(t_toolbar(TK::Settings)),
        ]).with_separator(),
        // 6. Undo/Redo
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::icon_button("undo", ToolbarIconId::new("Undo"))
                .with_tooltip(t_toolbar(TK::Undo)),
            ToolbarItemDef::icon_button("redo", ToolbarIconId::new("Redo"))
                .with_tooltip(t_toolbar(TK::Redo)),
        ]).with_separator(),
        // 7. Layout
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::dropdown("layout_menu", layout_menu_items())
                .with_icon(ToolbarIconId::new("LayoutSingle"))
                .with_tooltip(t_toolbar(TK::Layout)),
        ]),
        // 8. Presets (dropdown — between layout and screenshot)
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::dropdown("presets_menu", vec![
                // Dynamic preset items built in find_dropdown_items()
            ]).with_icon(ToolbarIconId::new("Bookmark"))
              .with_tooltip(t_toolbar(TK::Presets)),
        ]),
        // 9. Screenshot
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::icon_button("screenshot", ToolbarIconId::new("Camera"))
                .with_tooltip(t_toolbar(TK::Screenshot)),
        ]),
    ]).with_size(crate::types::TOP_TOOLBAR_HEIGHT)
}

/// Build the chart's bottom toolbar (horizontal, 40px tall)
///
/// Left section: zoom controls.
/// Right section: expand button + clock label (right-aligned).
pub fn bottom_toolbar() -> PanelToolbarDef {
    PanelToolbarDef::horizontal(vec![
        {
            let mut s = ToolbarSectionDef::new(vec![
                ToolbarItemDef::icon_button("expand", ToolbarIconId::new("Expand"))
                    .with_tooltip(t_toolbar(TK::Expand)),
                ToolbarItemDef::button("clock").with_text("00:00:00")
                    .with_tooltip(t_toolbar(TK::ServerTime)),
            ]);
            s.align = SectionAlign::End;
            s
        },
    ]).with_size(crate::types::TOP_TOOLBAR_HEIGHT)
}

/// Build the chart's top toolbar (horizontal, 40px tall) — matches terminal toolbar exactly
pub fn top_toolbar() -> PanelToolbarDef {
    PanelToolbarDef::horizontal(vec![
        // 1. Menu button (min_width=41 so separator aligns with left toolbar edge)
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::button("main_menu")
                .with_icon(ToolbarIconId::new("Menu"))
                .with_min_width(41.0)
                .with_tooltip(t_toolbar(TK::MainMenu)),
        ]).with_separator(),
        // 2. Symbol selector + Compare
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::dropdown("symbol_selector", vec![])
                .with_icon(ToolbarIconId::new("Search"))
                .with_text("BTCUSD")
                .with_min_width(150.0)
                .with_tooltip(t_toolbar(TK::SymbolSelector)),
            ToolbarItemDef::icon_button("compare", ToolbarIconId::new("Plus"))
                .with_tooltip(t_toolbar(TK::Compare)),
        ]),
        // 3. Timeframe selector
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::dropdown("timeframe_selector", vec![
                DropdownItemDef::action("tf_1m", "1m"),
                DropdownItemDef::action("tf_3m", "3m"),
                DropdownItemDef::action("tf_5m", "5m"),
                DropdownItemDef::action("tf_15m", "15m"),
                DropdownItemDef::action("tf_30m", "30m"),
                DropdownItemDef::action("tf_1h", "1H"),
                DropdownItemDef::action("tf_2h", "2H"),
                DropdownItemDef::action("tf_4h", "4H"),
                DropdownItemDef::action("tf_6h", "6H"),
                DropdownItemDef::action("tf_12h", "12H"),
                DropdownItemDef::action("tf_1d", "1D"),
                DropdownItemDef::action("tf_1w", "1W"),
                DropdownItemDef::action("tf_1M", "1M"),
            ]).with_icon(ToolbarIconId::new("Clock"))
              .with_text("1H")
              .with_min_width(56.0)
              .with_tooltip(t_toolbar(TK::TimeframeSelector)),
        ]).with_separator(),
        // 4. Chart type selector (dropdown with chart type items)
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::dropdown("chart_type_selector", vec![
                DropdownItemDef::action("candles", "Candles").with_icon(ToolbarIconId::new("Candlestick")),
                DropdownItemDef::action("hollow_candles", "Hollow Candles").with_icon(ToolbarIconId::new("HollowCandles")),
                DropdownItemDef::action("heikin_ashi", "Heikin Ashi").with_icon(ToolbarIconId::new("HeikinAshi")),
                DropdownItemDef::action("bars", "Bars").with_icon(ToolbarIconId::new("BarChart")),
                DropdownItemDef::Separator,
                DropdownItemDef::action("line", "Line").with_icon(ToolbarIconId::new("LineChart")),
                DropdownItemDef::action("step_line", "Step Line").with_icon(ToolbarIconId::new("StepLine")),
                DropdownItemDef::action("line_markers", "Line with Markers").with_icon(ToolbarIconId::new("LineWithMarkers")),
                DropdownItemDef::action("area", "Area").with_icon(ToolbarIconId::new("AreaChart")),
                DropdownItemDef::Separator,
                DropdownItemDef::action("hlc_area", "HLC Area").with_icon(ToolbarIconId::new("HlcArea")),
                DropdownItemDef::action("baseline", "Baseline").with_icon(ToolbarIconId::new("Baseline")),
                DropdownItemDef::action("histogram", "Histogram").with_icon(ToolbarIconId::new("Histogram")),
                DropdownItemDef::action("columns", "Columns").with_icon(ToolbarIconId::new("Columns")),
            ]).with_icon(ToolbarIconId::new("Candlestick"))
              .with_tooltip(t_toolbar(TK::ChartType)),
        ]).with_separator(),
        // 5. Indicators
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::icon_button("indicators", ToolbarIconId::new("Indicators"))
                .with_tooltip(t_toolbar(TK::Indicators)),
        ]),
        // 6. Settings (dropdown — matches terminal settings_menu content)
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::dropdown("settings_menu", settings_menu_items())
                .with_icon(ToolbarIconId::new("Settings"))
                .with_tooltip(t_toolbar(TK::Settings)),
        ]).with_separator(),
        // 7. Undo/Redo
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::icon_button("undo", ToolbarIconId::new("Undo"))
                .with_tooltip(t_toolbar(TK::Undo)),
            ToolbarItemDef::icon_button("redo", ToolbarIconId::new("Redo"))
                .with_tooltip(t_toolbar(TK::Redo)),
        ]).with_separator(),
        // 8. Layout
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::dropdown("layout_menu", layout_menu_items())
                .with_icon(ToolbarIconId::new("LayoutSingle"))
                .with_tooltip(t_toolbar(TK::Layout)),
        ]),
        // 9. Presets (dropdown — between layout and screenshot)
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::dropdown("presets_menu", vec![
                // Dynamic preset items built in find_dropdown_items()
            ]).with_icon(ToolbarIconId::new("Bookmark"))
              .with_tooltip(t_toolbar(TK::Presets)),
        ]),
        // 10. Screenshot
        ToolbarSectionDef::new(vec![
            ToolbarItemDef::icon_button("screenshot", ToolbarIconId::new("Camera"))
                .with_tooltip(t_toolbar(TK::Screenshot)),
        ]),
    ]).with_size(crate::types::TOP_TOOLBAR_HEIGHT)
}

/// Chart type dropdown items (localized).
fn chart_type_items() -> Vec<DropdownItemDef> {
    let lang = current_language();
    vec![
        DropdownItemDef::action("candles", MK::Candles.get(lang)).with_icon(ToolbarIconId::new("Candlestick")),
        DropdownItemDef::action("hollow_candles", MK::HollowCandles.get(lang)).with_icon(ToolbarIconId::new("HollowCandles")),
        DropdownItemDef::action("heikin_ashi", MK::HeikinAshi.get(lang)).with_icon(ToolbarIconId::new("HeikinAshi")),
        DropdownItemDef::action("bars", MK::Bars.get(lang)).with_icon(ToolbarIconId::new("BarChart")),
        DropdownItemDef::Separator,
        DropdownItemDef::action("line", MK::Line.get(lang)).with_icon(ToolbarIconId::new("LineChart")),
        DropdownItemDef::action("step_line", MK::StepLine.get(lang)).with_icon(ToolbarIconId::new("StepLine")),
        DropdownItemDef::action("line_markers", MK::LineWithMarkers.get(lang)).with_icon(ToolbarIconId::new("LineWithMarkers")),
        DropdownItemDef::action("area", MK::Area.get(lang)).with_icon(ToolbarIconId::new("AreaChart")),
        DropdownItemDef::Separator,
        DropdownItemDef::action("hlc_area", MK::HlcArea.get(lang)).with_icon(ToolbarIconId::new("HlcArea")),
        DropdownItemDef::action("baseline", MK::Baseline.get(lang)).with_icon(ToolbarIconId::new("Baseline")),
        DropdownItemDef::action("histogram", MK::Histogram.get(lang)).with_icon(ToolbarIconId::new("Histogram")),
        DropdownItemDef::action("columns", MK::Columns.get(lang)).with_icon(ToolbarIconId::new("Columns")),
    ]
}

/// Layout menu dropdown items (localized).
fn layout_menu_items() -> Vec<DropdownItemDef> {
    let lang = current_language();
    vec![
        // 1 panel
        DropdownItemDef::action("layout_single", "1").with_icon(ToolbarIconId::new("LayoutSingle")),
        DropdownItemDef::Separator,
        // 2 panels
        DropdownItemDef::action("layout_split_h", "2h").with_icon(ToolbarIconId::new("LayoutSplitH")),
        DropdownItemDef::action("layout_split_v", "2v").with_icon(ToolbarIconId::new("LayoutSplitV")),
        DropdownItemDef::Separator,
        // 3 panels
        DropdownItemDef::action("layout_2left_1right", "2L1R").with_icon(ToolbarIconId::new("Layout2Left1Right")),
        DropdownItemDef::action("layout_1left_2right", "1L2R").with_icon(ToolbarIconId::new("Layout1Left2Right")),
        DropdownItemDef::action("layout_2top_1bottom", "2T1B").with_icon(ToolbarIconId::new("Layout2Top1Bottom")),
        DropdownItemDef::action("layout_1top_2bottom", "1T2B").with_icon(ToolbarIconId::new("Layout1Top2Bottom")),
        DropdownItemDef::action("layout_3columns", "3col").with_icon(ToolbarIconId::new("Layout3Columns")),
        DropdownItemDef::action("layout_3rows", "3row").with_icon(ToolbarIconId::new("Layout3Rows")),
        DropdownItemDef::Separator,
        // 4 panels
        DropdownItemDef::action("layout_grid_2x2", "2x2").with_icon(ToolbarIconId::new("LayoutGrid2x2")),
        DropdownItemDef::action("layout_1big_3small", "1+3").with_icon(ToolbarIconId::new("Layout1Big3Small")),
        DropdownItemDef::Separator,
        // Panel management
        DropdownItemDef::action("panel_close", MK::ClosePanel.get(lang)),
        DropdownItemDef::action("panel_reset_sizes", MK::ResetSizes.get(lang)).with_icon(ToolbarIconId::new("ZoomReset")),
        DropdownItemDef::Separator,
        DropdownItemDef::action("split_untagged", MK::SplitWithoutGroup.get(lang)),
        DropdownItemDef::Separator,
        // Sync options
        DropdownItemDef::action("sync_symbol", MK::SyncSymbol.get(lang)).with_icon(ToolbarIconId::new("Search")),
        DropdownItemDef::action("sync_timeframe", MK::SyncTimeframe.get(lang)).with_icon(ToolbarIconId::new("Clock")),
        DropdownItemDef::action("sync_crosshair", MK::SyncCrosshair.get(lang)).with_icon(ToolbarIconId::new("Crosshair")),
        DropdownItemDef::action("sync_viewport", MK::SyncViewport.get(lang)).with_icon(ToolbarIconId::new("Move")),
        DropdownItemDef::action("sync_drawings", MK::SyncDrawings.get(lang)),
        DropdownItemDef::action("sync_indicators", MK::SyncIndicators.get(lang)),
    ]
}

// === Drawing tool sections ===

fn cursor_section() -> ToolbarSectionDef {
    ToolbarSectionDef::new(vec![
        ToolbarItemDef::quick_select("cursor_tools", vec![
            DropdownItemDef::action("crosshair", t_toolbar_menu(MK::SubCrosshair)).with_icon(ToolbarIconId::new("Crosshair")),
            DropdownItemDef::action("hand", t_toolbar_menu(MK::Pan)).with_icon(ToolbarIconId::new("Hand")),
        ]).with_icon(ToolbarIconId::new("Crosshair"))
          .with_tooltip(t_toolbar(TK::Crosshair)),
    ])
}

fn line_section() -> ToolbarSectionDef {
    let lang = current_language();
    ToolbarSectionDef::new(vec![
        ToolbarItemDef::quick_select("line_tools", vec![
            // Lines
            DropdownItemDef::Header { label: MK::HeaderLines.get(lang).to_string() },
            DropdownItemDef::action("trend_line", PN::TrendLine.get(lang)).with_icon(ToolbarIconId::new("TrendLine")),
            DropdownItemDef::action("ray", PN::Ray.get(lang)).with_icon(ToolbarIconId::new("Ray")),
            DropdownItemDef::action("info_line", PN::InfoLine.get(lang)).with_icon(ToolbarIconId::new("InfoLine")),
            DropdownItemDef::action("extended_line", PN::ExtendedLine.get(lang)).with_icon(ToolbarIconId::new("ExtendedLine")),
            DropdownItemDef::action("trend_angle", PN::TrendAngle.get(lang)).with_icon(ToolbarIconId::new("TrendAngle")),
            DropdownItemDef::action("horizontal_line", PN::HorizontalLine.get(lang)).with_icon(ToolbarIconId::new("HorizontalLine")),
            DropdownItemDef::action("horizontal_ray", PN::HorizontalRay.get(lang)).with_icon(ToolbarIconId::new("HorizontalRay")),
            DropdownItemDef::action("vertical_line", PN::VerticalLine.get(lang)).with_icon(ToolbarIconId::new("VerticalLine")),
            DropdownItemDef::action("cross_line", PN::CrossLine.get(lang)).with_icon(ToolbarIconId::new("CrossLine")),
            // Channels
            DropdownItemDef::Separator,
            DropdownItemDef::Header { label: MK::HeaderChannels.get(lang).to_string() },
            DropdownItemDef::action("parallel_channel", PN::ParallelChannel.get(lang)).with_icon(ToolbarIconId::new("ParallelChannel")),
            DropdownItemDef::action("regression_trend", PN::RegressionTrend.get(lang)).with_icon(ToolbarIconId::new("RegressionTrend")),
            DropdownItemDef::action("flat_top_bottom", PN::FlatTopBottom.get(lang)).with_icon(ToolbarIconId::new("FlatTopBottom")),
            DropdownItemDef::action("disjoint_channel", PN::DisjointChannel.get(lang)).with_icon(ToolbarIconId::new("DisjointChannel")),
            // Pitchforks
            DropdownItemDef::Separator,
            DropdownItemDef::Header { label: MK::HeaderPitchforks.get(lang).to_string() },
            DropdownItemDef::action("pitchfork", PN::Pitchfork.get(lang)).with_icon(ToolbarIconId::new("Pitchfork")),
            DropdownItemDef::action("schiff_pitchfork", PN::SchiffPitchfork.get(lang)).with_icon(ToolbarIconId::new("SchiffPitchfork")),
            DropdownItemDef::action("modified_schiff", PN::ModifiedSchiff.get(lang)).with_icon(ToolbarIconId::new("ModifiedSchiff")),
            DropdownItemDef::action("inside_pitchfork", PN::InsidePitchfork.get(lang)).with_icon(ToolbarIconId::new("InsidePitchfork")),
        ]).with_icon(ToolbarIconId::new("TrendLine"))
          .with_tooltip(t_toolbar(TK::LineTool)),
    ])
}

fn fib_section() -> ToolbarSectionDef {
    let lang = current_language();
    ToolbarSectionDef::new(vec![
        ToolbarItemDef::quick_select("fib_tools", vec![
            // Fibonacci
            DropdownItemDef::Header { label: MK::HeaderFibonacci.get(lang).to_string() },
            DropdownItemDef::action("fib_retracement", PN::FibRetracement.get(lang)).with_icon(ToolbarIconId::new("FibRetracement")),
            DropdownItemDef::action("fib_trend_extension", PN::FibExtension.get(lang)).with_icon(ToolbarIconId::new("FibExtension")),
            DropdownItemDef::action("fib_channel", PN::FibChannel.get(lang)).with_icon(ToolbarIconId::new("FibChannel")),
            DropdownItemDef::action("fib_time_zones", PN::FibTimeZones.get(lang)).with_icon(ToolbarIconId::new("FibTimeZones")),
            DropdownItemDef::action("fib_speed_resistance", PN::FibSpeedResistance.get(lang)).with_icon(ToolbarIconId::new("FibSpeedResistance")),
            DropdownItemDef::action("fib_trend_time", PN::FibTrendTime.get(lang)).with_icon(ToolbarIconId::new("FibTrendTime")),
            DropdownItemDef::action("fib_circles", PN::FibCircles.get(lang)).with_icon(ToolbarIconId::new("FibCircle")),
            DropdownItemDef::action("fib_spiral", PN::FibSpiral.get(lang)).with_icon(ToolbarIconId::new("FibSpiral")),
            DropdownItemDef::action("fib_arcs", PN::FibArcs.get(lang)).with_icon(ToolbarIconId::new("FibArcs")),
            DropdownItemDef::action("fib_wedge", PN::FibWedge.get(lang)).with_icon(ToolbarIconId::new("FibWedge")),
            DropdownItemDef::action("fib_fan", PN::FibFan.get(lang)).with_icon(ToolbarIconId::new("FibFan")),
            // Gann
            DropdownItemDef::Separator,
            DropdownItemDef::Header { label: MK::HeaderGann.get(lang).to_string() },
            DropdownItemDef::action("gann_box", PN::GannBox.get(lang)).with_icon(ToolbarIconId::new("GannBox")),
            DropdownItemDef::action("gann_square_fixed", PN::GannSquareFixed.get(lang)).with_icon(ToolbarIconId::new("GannSquare")),
            DropdownItemDef::action("gann_square", PN::GannSquare.get(lang)).with_icon(ToolbarIconId::new("GannSquare")),
            DropdownItemDef::action("gann_fan", PN::GannFan.get(lang)).with_icon(ToolbarIconId::new("GannFan")),
        ]).with_icon(ToolbarIconId::new("FibRetracement"))
          .with_tooltip(t_toolbar(TK::FibTool)),
    ])
}

fn pattern_section() -> ToolbarSectionDef {
    let lang = current_language();
    ToolbarSectionDef::new(vec![
        ToolbarItemDef::quick_select("pattern_tools", vec![
            // Patterns
            DropdownItemDef::Header { label: MK::HeaderPatterns.get(lang).to_string() },
            DropdownItemDef::action("xabcd_pattern", PN::XabcdPattern.get(lang)).with_icon(ToolbarIconId::new("XabcdPattern")),
            DropdownItemDef::action("cypher_pattern", PN::CypherPattern.get(lang)).with_icon(ToolbarIconId::new("CypherPattern")),
            DropdownItemDef::action("head_shoulders", PN::HeadShoulders.get(lang)).with_icon(ToolbarIconId::new("HeadShoulders")),
            DropdownItemDef::action("abcd_pattern", PN::AbcdPattern.get(lang)).with_icon(ToolbarIconId::new("AbcdPattern")),
            DropdownItemDef::action("triangle_pattern", PN::TrianglePattern.get(lang)).with_icon(ToolbarIconId::new("TrianglePattern")),
            DropdownItemDef::action("three_drives", PN::ThreeDrives.get(lang)).with_icon(ToolbarIconId::new("ThreeDrives")),
            // Elliott Waves
            DropdownItemDef::Separator,
            DropdownItemDef::Header { label: MK::HeaderElliottWaves.get(lang).to_string() },
            DropdownItemDef::action("elliott_impulse", PN::ElliottImpulse.get(lang)).with_icon(ToolbarIconId::new("ElliottImpulse")),
            DropdownItemDef::action("elliott_correction", PN::ElliottCorrection.get(lang)).with_icon(ToolbarIconId::new("ElliottCorrection")),
            DropdownItemDef::action("elliott_triangle", PN::ElliottTriangle.get(lang)).with_icon(ToolbarIconId::new("ElliottTriangle")),
            DropdownItemDef::action("elliott_double_combo", PN::ElliottDoubleCombo.get(lang)).with_icon(ToolbarIconId::new("ElliottCombo")),
            DropdownItemDef::action("elliott_triple_combo", PN::ElliottTripleCombo.get(lang)).with_icon(ToolbarIconId::new("ElliottCombo")),
            // Cycles
            DropdownItemDef::Separator,
            DropdownItemDef::Header { label: MK::HeaderCycles.get(lang).to_string() },
            DropdownItemDef::action("cycle_lines", PN::CycleLines.get(lang)).with_icon(ToolbarIconId::new("CycleLines")),
            DropdownItemDef::action("time_cycles", PN::TimeCycles.get(lang)).with_icon(ToolbarIconId::new("TimeCycles")),
            DropdownItemDef::action("sine_wave", PN::SineWave.get(lang)).with_icon(ToolbarIconId::new("SineWave")),
        ]).with_icon(ToolbarIconId::new("XabcdPattern"))
          .with_tooltip(t_toolbar(TK::PatternTool)),
    ])
}

fn brush_section() -> ToolbarSectionDef {
    let lang = current_language();
    ToolbarSectionDef::new(vec![
        ToolbarItemDef::quick_select("brush_tools", vec![
            // Brushes
            DropdownItemDef::Header { label: MK::HeaderBrushes.get(lang).to_string() },
            DropdownItemDef::action("brush", PN::Brush.get(lang)).with_icon(ToolbarIconId::new("Brush")),
            DropdownItemDef::action("highlighter", PN::Highlighter.get(lang)).with_icon(ToolbarIconId::new("Highlighter")),
            // Shapes
            DropdownItemDef::Separator,
            DropdownItemDef::Header { label: MK::HeaderShapes.get(lang).to_string() },
            DropdownItemDef::action("rectangle", PN::Rectangle.get(lang)).with_icon(ToolbarIconId::new("Rectangle")),
            DropdownItemDef::action("rotated_rectangle", PN::RotatedRectangle.get(lang)).with_icon(ToolbarIconId::new("RotatedRectangle")),
            DropdownItemDef::action("circle", PN::Circle.get(lang)).with_icon(ToolbarIconId::new("Circle")),
            DropdownItemDef::action("ellipse", PN::Ellipse.get(lang)).with_icon(ToolbarIconId::new("Ellipse")),
            DropdownItemDef::action("triangle", PN::Triangle.get(lang)).with_icon(ToolbarIconId::new("Triangle")),
            DropdownItemDef::action("arc", PN::Arc.get(lang)).with_icon(ToolbarIconId::new("Arc")),
            DropdownItemDef::action("polyline", PN::Polyline.get(lang)).with_icon(ToolbarIconId::new("Polyline")),
            DropdownItemDef::action("path", PN::Path.get(lang)).with_icon(ToolbarIconId::new("Path")),
            DropdownItemDef::action("curve", PN::Curve.get(lang)).with_icon(ToolbarIconId::new("Curve")),
            DropdownItemDef::action("double_curve", PN::DoubleCurve.get(lang)).with_icon(ToolbarIconId::new("DoubleCurve")),
            // Arrows
            DropdownItemDef::Separator,
            DropdownItemDef::action("arrow_line", PN::ArrowLine.get(lang)).with_icon(ToolbarIconId::new("Arrow")),
        ]).with_icon(ToolbarIconId::new("Brush"))
          .with_tooltip(t_toolbar(TK::BrushTool)),
    ])
}

fn annotation_section() -> ToolbarSectionDef {
    let lang = current_language();
    ToolbarSectionDef::new(vec![
        ToolbarItemDef::quick_select("annotation_tools", vec![
            DropdownItemDef::action("text", PN::Text.get(lang)).with_icon(ToolbarIconId::new("Text")),
            DropdownItemDef::action("anchored_text", PN::AnchoredText.get(lang)).with_icon(ToolbarIconId::new("AnchoredText")),
            DropdownItemDef::action("note", PN::Note.get(lang)).with_icon(ToolbarIconId::new("Note")),
            DropdownItemDef::action("price_note", PN::PriceNote.get(lang)).with_icon(ToolbarIconId::new("PriceNote")),
            DropdownItemDef::action("signpost", PN::Signpost.get(lang)).with_icon(ToolbarIconId::new("Signpost")),
            DropdownItemDef::action("table", PN::Table.get(lang)).with_icon(ToolbarIconId::new("Table")),
            DropdownItemDef::action("callout", PN::Callout.get(lang)).with_icon(ToolbarIconId::new("Callout")),
            DropdownItemDef::action("comment", PN::Comment.get(lang)).with_icon(ToolbarIconId::new("Comment")),
            DropdownItemDef::action("price_label", PN::PriceLabel.get(lang)).with_icon(ToolbarIconId::new("PriceLabel")),
            DropdownItemDef::action("sign", PN::Sign.get(lang)).with_icon(ToolbarIconId::new("Sign")),
            DropdownItemDef::action("flag", PN::Flag.get(lang)).with_icon(ToolbarIconId::new("Flag")),
            DropdownItemDef::action("triangle_up", PN::TriangleUp.get(lang)).with_icon(ToolbarIconId::new("ArrowUp")),
            DropdownItemDef::action("triangle_down", PN::TriangleDown.get(lang)).with_icon(ToolbarIconId::new("ArrowDown")),
        ]).with_icon(ToolbarIconId::new("Text"))
          .with_tooltip(t_toolbar(TK::AnnotationTool)),
    ])
}

fn icon_section() -> ToolbarSectionDef {
    let lang = current_language();
    ToolbarSectionDef::new(vec![
        ToolbarItemDef::dropdown("icon_tools", vec![
            DropdownItemDef::Submenu {
                id: "emoji_submenu".to_string(),
                label: MK::HeaderEmoji.get(lang).to_string(),
                icon: Some(ToolbarIconId::new("Emoji")),
                items: emoji_items(),
                grid_columns: Some(6),
            },
            DropdownItemDef::Separator,
            DropdownItemDef::action("image", PN::Image.get(lang)).with_icon(ToolbarIconId::new("Image")),
        ]).with_icon(ToolbarIconId::new("Emoji"))
          .with_tooltip(t_toolbar(TK::IconTool)),
    ])
}

fn emoji_items() -> Vec<DropdownItemDef> {
    let lang = current_language();
    vec![
        // Signals (emoji labels are icon descriptors — kept as EN, universal)
        DropdownItemDef::Header { label: MK::HeaderSignals.get(lang).to_string() },
        DropdownItemDef::action("emoji_target", "Target").with_icon(ToolbarIconId::new("EmojiTarget")),
        DropdownItemDef::action("emoji_flag", "Flag").with_icon(ToolbarIconId::new("EmojiFlag")),
        DropdownItemDef::action("emoji_check", "Check").with_icon(ToolbarIconId::new("EmojiCheck")),
        DropdownItemDef::action("emoji_cross", "Cross").with_icon(ToolbarIconId::new("EmojiCross")),
        DropdownItemDef::action("emoji_warning", "Warning").with_icon(ToolbarIconId::new("EmojiWarning")),
        DropdownItemDef::action("emoji_dollar", "Dollar").with_icon(ToolbarIconId::new("EmojiDollar")),
        DropdownItemDef::action("emoji_lightning", "Lightning").with_icon(ToolbarIconId::new("EmojiLightning")),
        DropdownItemDef::action("emoji_lock", "Lock").with_icon(ToolbarIconId::new("EmojiLock")),
        DropdownItemDef::action("emoji_unlock", "Unlock").with_icon(ToolbarIconId::new("EmojiUnlock")),
        DropdownItemDef::action("emoji_bell", "Bell").with_icon(ToolbarIconId::new("EmojiBell")),
        DropdownItemDef::action("emoji_eye", "Eye").with_icon(ToolbarIconId::new("EmojiEye")),
        DropdownItemDef::action("emoji_clock", "Clock").with_icon(ToolbarIconId::new("EmojiClock")),
        // Markers
        DropdownItemDef::Separator,
        DropdownItemDef::Header { label: MK::HeaderMarkers.get(lang).to_string() },
        DropdownItemDef::action("emoji_star", "Star").with_icon(ToolbarIconId::new("EmojiStar")),
        DropdownItemDef::action("emoji_heart", "Heart").with_icon(ToolbarIconId::new("EmojiHeart")),
        DropdownItemDef::action("emoji_circle", "Circle").with_icon(ToolbarIconId::new("EmojiCircle")),
        DropdownItemDef::action("emoji_diamond", "Diamond").with_icon(ToolbarIconId::new("EmojiDiamond")),
        DropdownItemDef::action("emoji_square", "Square").with_icon(ToolbarIconId::new("EmojiSquare")),
        DropdownItemDef::action("emoji_triangle", "Triangle").with_icon(ToolbarIconId::new("EmojiTriangle")),
        DropdownItemDef::action("emoji_plus", "Plus").with_icon(ToolbarIconId::new("EmojiPlus")),
        DropdownItemDef::action("emoji_minus", "Minus").with_icon(ToolbarIconId::new("EmojiMinus")),
        DropdownItemDef::action("emoji_question", "Question").with_icon(ToolbarIconId::new("EmojiQuestion")),
        DropdownItemDef::action("emoji_info", "Info").with_icon(ToolbarIconId::new("EmojiInfo")),
        // Emotions
        DropdownItemDef::Separator,
        DropdownItemDef::Header { label: MK::HeaderEmotions.get(lang).to_string() },
        DropdownItemDef::action("emoji_thumbs_up", "Thumbs Up").with_icon(ToolbarIconId::new("EmojiThumbsUp")),
        DropdownItemDef::action("emoji_thumbs_down", "Thumbs Down").with_icon(ToolbarIconId::new("EmojiThumbsDown")),
        DropdownItemDef::action("emoji_fire", "Fire").with_icon(ToolbarIconId::new("EmojiFire")),
        DropdownItemDef::action("emoji_rocket", "Rocket").with_icon(ToolbarIconId::new("EmojiRocket")),
        DropdownItemDef::action("emoji_skull", "Skull").with_icon(ToolbarIconId::new("EmojiSkull")),
        DropdownItemDef::action("emoji_crown", "Crown").with_icon(ToolbarIconId::new("EmojiCrown")),
        DropdownItemDef::action("emoji_gem", "Gem").with_icon(ToolbarIconId::new("EmojiGem")),
        DropdownItemDef::action("emoji_poop", "Poop").with_icon(ToolbarIconId::new("EmojiPoop")),
        DropdownItemDef::action("emoji_frog", "Frog").with_icon(ToolbarIconId::new("EmojiFrog")),
        DropdownItemDef::action("emoji_frogger", "Frogger").with_icon(ToolbarIconId::new("EmojiFrogger")),
        // Arrows
        DropdownItemDef::Separator,
        DropdownItemDef::Header { label: MK::HeaderArrows.get(lang).to_string() },
        DropdownItemDef::action("emoji_arrow_up", "Arrow Up").with_icon(ToolbarIconId::new("EmojiArrowUp")),
        DropdownItemDef::action("emoji_arrow_down", "Arrow Down").with_icon(ToolbarIconId::new("EmojiArrowDown")),
        DropdownItemDef::action("emoji_arrow_left", "Arrow Left").with_icon(ToolbarIconId::new("EmojiArrowLeft")),
        DropdownItemDef::action("emoji_arrow_right", "Arrow Right").with_icon(ToolbarIconId::new("EmojiArrowRight")),
    ]
}

fn projection_section() -> ToolbarSectionDef {
    let lang = current_language();
    ToolbarSectionDef::new(vec![
        ToolbarItemDef::quick_select("projection_tools", vec![
            // Positions
            DropdownItemDef::Header { label: MK::HeaderPositions.get(lang).to_string() },
            DropdownItemDef::action("long_position", PN::LongPosition.get(lang)).with_icon(ToolbarIconId::new("LongPosition")),
            DropdownItemDef::action("short_position", PN::ShortPosition.get(lang)).with_icon(ToolbarIconId::new("ShortPosition")),
            // Forecast
            DropdownItemDef::Separator,
            DropdownItemDef::Header { label: MK::HeaderForecast.get(lang).to_string() },
            DropdownItemDef::action("bars_pattern", PN::BarsPattern.get(lang)).with_icon(ToolbarIconId::new("BarsPattern")),
            DropdownItemDef::action("price_projection", PN::PriceProjection.get(lang)).with_icon(ToolbarIconId::new("PriceProjection")),
            DropdownItemDef::action("projection", PN::Projection.get(lang)).with_icon(ToolbarIconId::new("Projection")),
            // Volume
            DropdownItemDef::Separator,
            DropdownItemDef::Header { label: MK::HeaderVolume.get(lang).to_string() },
            DropdownItemDef::action("fixed_volume_profile", PN::FixedVolumeProfile.get(lang)).with_icon(ToolbarIconId::new("VolumeProfile")),
            DropdownItemDef::action("anchored_volume_profile", PN::AnchoredVolumeProfile.get(lang)).with_icon(ToolbarIconId::new("VolumeProfile")),
            // Measurement
            DropdownItemDef::Separator,
            DropdownItemDef::Header { label: MK::HeaderMeasurement.get(lang).to_string() },
            DropdownItemDef::action("price_range", PN::PriceRange.get(lang)).with_icon(ToolbarIconId::new("PriceRange")),
            DropdownItemDef::action("date_range", PN::DateRange.get(lang)).with_icon(ToolbarIconId::new("DateRange")),
            DropdownItemDef::action("price_date_range", PN::PriceDateRange.get(lang)).with_icon(ToolbarIconId::new("PriceDateRange")),
        ]).with_icon(ToolbarIconId::new("LongPosition"))
          .with_tooltip(t_toolbar(TK::ProjectionTool)),
    ])
}

/// Build the full settings_menu dropdown items, matching the terminal's settings_menu content.
///
/// Sections: Chart Settings, Grid, Crosshair, Legend, Tooltip, Watermark, Theme, UI Style.
/// Each visual group is a `DropdownItemDef::Submenu` that opens a list-style flyout
/// (`grid_columns: None`) positioned to the right of the parent menu.
pub fn settings_menu_items() -> Vec<DropdownItemDef> {
    let lang = current_language();
    vec![
        // Direct action — opens the chart settings modal
        DropdownItemDef::action("chart_settings", MK::ChartSettings.get(lang)),
        DropdownItemDef::Separator,

        // Grid submenu
        DropdownItemDef::Submenu {
            id: "grid_submenu".to_string(),
            label: MK::SubGrid.get(lang).to_string(),
            icon: Some(ToolbarIconId::new("Grid")),
            items: vec![
                DropdownItemDef::action("grid_toggle", MK::ToggleGrid.get(lang)).with_icon(ToolbarIconId::new("Grid")),
                DropdownItemDef::Separator,
                DropdownItemDef::action("grid_vert", MK::VerticalLines.get(lang)),
                DropdownItemDef::action("grid_horz", MK::HorizontalLines.get(lang)),
            ],
            grid_columns: None,
        },

        // Crosshair submenu
        DropdownItemDef::Submenu {
            id: "crosshair_submenu".to_string(),
            label: MK::SubCrosshair.get(lang).to_string(),
            icon: Some(ToolbarIconId::new("Crosshair")),
            items: vec![
                DropdownItemDef::action("crosshair_toggle", MK::ToggleCrosshair.get(lang)).with_icon(ToolbarIconId::new("Crosshair")),
                DropdownItemDef::Separator,
                DropdownItemDef::action("ch_normal", MK::NormalMode.get(lang)),
                DropdownItemDef::action("ch_magnet", MK::MagnetClose.get(lang)),
                DropdownItemDef::action("ch_magnet_ohlc", MK::MagnetOhlc.get(lang)),
            ],
            grid_columns: None,
        },

        // Tooltip submenu
        DropdownItemDef::Submenu {
            id: "tooltip_submenu".to_string(),
            label: MK::SubTooltip.get(lang).to_string(),
            icon: None,
            items: vec![
                DropdownItemDef::action("tooltip_toggle", MK::ToggleTooltip.get(lang)),
                DropdownItemDef::Separator,
                DropdownItemDef::action("tooltip_follow", MK::FollowCursor.get(lang)),
            ],
            grid_columns: None,
        },

        // Watermark submenu
        DropdownItemDef::Submenu {
            id: "watermark_submenu".to_string(),
            label: MK::SubWatermark.get(lang).to_string(),
            icon: None,
            items: vec![
                DropdownItemDef::action("watermark_toggle", MK::ToggleWatermark.get(lang)),
                DropdownItemDef::Separator,
                DropdownItemDef::action("watermark_text_seeyou", MK::WatermarkSeeyou.get(lang)),
                DropdownItemDef::action("watermark_text_demo", MK::WatermarkDemo.get(lang)),
                DropdownItemDef::action("watermark_text_paper", MK::WatermarkPaper.get(lang)),
                DropdownItemDef::action("watermark_text_live", MK::WatermarkLive.get(lang)),
                DropdownItemDef::Separator,
                DropdownItemDef::action("watermark_pos_center", MK::WatermarkCenter.get(lang)),
                DropdownItemDef::action("watermark_pos_bl", MK::WatermarkBl.get(lang)),
                DropdownItemDef::action("watermark_pos_br", MK::WatermarkBr.get(lang)),
            ],
            grid_columns: None,
        },

        DropdownItemDef::Separator,

        // Theme submenu
        DropdownItemDef::Submenu {
            id: "theme_submenu".to_string(),
            label: MK::SubTheme.get(lang).to_string(),
            icon: None,
            items: vec![
                DropdownItemDef::action("theme_dark", MK::ThemeDark.get(lang)),
                DropdownItemDef::action("theme_light", MK::ThemeLight.get(lang)),
                DropdownItemDef::action("theme_high_contrast", MK::ThemeHighContrast.get(lang)),
                DropdownItemDef::action("theme_high_contrast_mono", MK::ThemeHcMono.get(lang)),
                DropdownItemDef::action("theme_mascot", MK::ThemeWizardHat.get(lang)),
            ],
            grid_columns: None,
        },

        // UI Style submenu
        DropdownItemDef::Submenu {
            id: "ui_style_submenu".to_string(),
            label: MK::SubUiStyle.get(lang).to_string(),
            icon: None,
            items: vec![
                DropdownItemDef::action("style_solid", MK::StyleSolid.get(lang)),
                DropdownItemDef::action("style_glass", MK::StyleGlass.get(lang)),
                DropdownItemDef::action("style_frosted_glass_flat", MK::StyleFrostedGlass.get(lang)),
            ],
            grid_columns: None,
        },
    ]
}

fn magnet_section() -> ToolbarSectionDef {
    // Single-click: toggle magnet ON/OFF (like lock/eye buttons).
    // Double-click: opens dropdown with magnet mode selection (handled in panel_app).
    ToolbarSectionDef::new(vec![
        ToolbarItemDef::icon_button("magnet", ToolbarIconId::new("Magnet"))
            .with_tooltip(t_toolbar(TK::MagnetMode)),
    ])
}

fn lock_section() -> ToolbarSectionDef {
    ToolbarSectionDef::new(vec![
        ToolbarItemDef::icon_button("lock", ToolbarIconId::new("Unlock"))
            .with_tooltip(t_toolbar(TK::Lock)),
    ])
}

fn visibility_section() -> ToolbarSectionDef {
    ToolbarSectionDef::new(vec![
        ToolbarItemDef::icon_button("eye", ToolbarIconId::new("Eye"))
            .with_tooltip(t_toolbar(TK::Eye)),
    ])
}

fn delete_section() -> ToolbarSectionDef {
    let lang = current_language();
    ToolbarSectionDef::new(vec![
        ToolbarItemDef::quick_select("delete_tools", vec![
            DropdownItemDef::action("delete_selected", MK::DeleteSelected.get(lang)).with_icon(ToolbarIconId::new("Delete")),
            DropdownItemDef::action("delete_all", MK::DeleteAll.get(lang)).with_icon(ToolbarIconId::new("Delete")),
        ]).with_icon(ToolbarIconId::new("Delete"))
          .with_tooltip(t_toolbar(TK::DeleteTool)),
    ])
}

/// Look up the tooltip text for a toolbar button by its item ID.
///
/// Searches all toolbar definitions (left, right, standalone top, top, bottom) for a
/// matching item and returns its tooltip text if one is configured.
pub fn find_toolbar_tooltip(button_id: &str) -> Option<&'static str> {
    let toolbars = [
        left_toolbar(),
        right_toolbar(),
        standalone_top_toolbar(),
        top_toolbar(),
        bottom_toolbar(),
    ];
    for toolbar in &toolbars {
        for section in &toolbar.sections {
            for item in &section.items {
                if item.id() == button_id {
                    return item.tooltip();
                }
            }
        }
    }
    None
}


