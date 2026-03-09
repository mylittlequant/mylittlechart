//! Chart actions - all possible UI interactions
//!
//! Actions are the "what" - they describe intent without knowing how it's triggered.

use crate::{CrosshairMode, HorzAlign, LegendPosition, LineStyle, VertAlign};

/// All possible chart actions that can be triggered from UI
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChartAction {
    // === Series / Chart Type ===
    ToggleCandles,
    ToggleLine,
    ToggleArea,
    ToggleHistogram,
    ToggleBaseline,
    SetChartType(&'static str),  // "candles", "line", "area", "bars", "histogram", "baseline"

    // === Overlays ===
    ToggleLegend,
    ToggleTooltip,
    ToggleWatermark,
    ToggleGrid,
    ToggleCrosshair,

    // === Legend options ===
    SetLegendPosition(LegendPosition),
    ToggleLegendOHLC,
    ToggleLegendChange,
    ToggleLegendPercent,

    // === Tooltip options ===
    ToggleTooltipFollow,

    // === Watermark options ===
    SetWatermarkPosition(HorzAlign, VertAlign),
    SetWatermarkColor(&'static str),
    SetWatermarkText(&'static str),

    // === Grid ===
    SetGridStyle(LineStyle),
    SetGridHorzVisible(bool),
    SetGridVertVisible(bool),
    ToggleGridVertical,
    ToggleGridHorizontal,

    // === Crosshair ===
    SetCrosshairMode(CrosshairMode),
    SetCrosshairStyle(LineStyle),
    ToggleMagnet,
    ToggleCrosshairVertLine,
    ToggleCrosshairHorzLine,
    ToggleCrosshairVertLabel,
    ToggleCrosshairHorzLabel,

    // === Price lines ===
    AddPriceLine,
    ClearPriceLines,

    // === Markers ===
    AddMarker,
    ClearMarkers,

    // === Data ===
    RegenerateData,

    // === View / Zoom ===
    ResetZoom,
    FitContent,
    ZoomIn,
    ZoomOut,

    // === Drawing Tools ===
    SelectTool(&'static str),  // "cursor", "crosshair", "trend_line", "rectangle", etc.
    ToggleLockDrawings,        // Global lock - prevents all editing
    ToggleLockSelected,        // Lock/unlock selected primitive
    ToggleDrawingsVisible,
    DeleteSelected,
    DeleteAll,

    // === Symbol / Timeframe ===
    OpenSymbolSearch,
    OpenSymbolHistory,           // Opens dropdown with last 5 symbols
    SetSymbol(&'static str),     // Set symbol by ticker (e.g., "BTCUSD", "AAPL")
    SetTimeframe(&'static str),  // "1m", "5m", "15m", "1h", "4h", "1d", "1w"

    // === Dialogs ===
    OpenIndicators,
    OpenCompare,
    OpenSettings,
    ToggleObjectTree,
    /// Toggle signals panel (strategy-generated markers)
    ToggleSignals,
    /// Toggle indicators panel (right sidebar)
    ToggleIndicators,

    // === Panels / Sidebars ===
    ToggleLeftPanel,       // Main menu panel (account, settings, etc.)
    ToggleRightPanel,      // Watchlist/alerts panel
    ToggleBottomPanel,     // Trading panel
    ToggleWatchlist,       // Watchlist sub-panel
    ToggleAlerts,          // Alerts sub-panel
    ToggleTradingPanel,    // Trading/orders panel
    TogglePositions,       // Positions sub-panel

    // === History ===
    Undo,
    Redo,

    // === Theme ===
    SetTheme(&'static str),  // "dark", "light", "high_contrast", "high_contrast_mono", "cypherpunk"
    OpenThemeSettings,       // Open theme configuration panel

    // === UI Style ===
    SetStyle(&'static str),  // "solid", "glass", "liquid_glass"

    // === Custom action with string ID (for extensibility) ===
    Custom(&'static str),
}

impl ChartAction {
    /// Get action ID for serialization/lookup
    pub fn id(&self) -> &'static str {
        match self {
            // Series
            Self::ToggleCandles => "toggle_candles",
            Self::ToggleLine => "toggle_line",
            Self::ToggleArea => "toggle_area",
            Self::ToggleHistogram => "toggle_histogram",
            Self::ToggleBaseline => "toggle_baseline",
            Self::SetChartType(t) => t,

            // Overlays
            Self::ToggleLegend => "toggle_legend",
            Self::ToggleTooltip => "toggle_tooltip",
            Self::ToggleWatermark => "toggle_watermark",
            Self::ToggleGrid => "toggle_grid",
            Self::ToggleCrosshair => "toggle_crosshair",

            // Legend options
            Self::ToggleLegendOHLC => "toggle_legend_ohlc",
            Self::ToggleLegendChange => "toggle_legend_change",
            Self::ToggleLegendPercent => "toggle_legend_percent",

            // Tooltip options
            Self::ToggleTooltipFollow => "toggle_tooltip_follow",

            // Watermark options
            Self::SetWatermarkPosition(horz, vert) => match (horz, vert) {
                (HorzAlign::Center, VertAlign::Center) => "watermark_pos_center",
                (HorzAlign::Left, VertAlign::Bottom) => "watermark_pos_bl",
                (HorzAlign::Right, VertAlign::Bottom) => "watermark_pos_br",
                (HorzAlign::Left, VertAlign::Top) => "watermark_pos_tl",
                (HorzAlign::Right, VertAlign::Top) => "watermark_pos_tr",
                _ => "watermark_pos_custom",
            },
            Self::SetWatermarkColor(color) => color,
            Self::SetWatermarkText(_) => "set_watermark_text",

            // Legend position
            Self::SetLegendPosition(pos) => match pos {
                LegendPosition::TopLeft => "legend_pos_tl",
                LegendPosition::TopRight => "legend_pos_tr",
                LegendPosition::BottomLeft => "legend_pos_bl",
                LegendPosition::BottomRight => "legend_pos_br",
            },

            // Grid
            Self::SetGridStyle(style) => match style {
                LineStyle::Solid => "grid_style_solid",
                LineStyle::Dashed => "grid_style_dashed",
                LineStyle::Dotted => "grid_style_dotted",
                LineStyle::LargeDashed => "grid_style_large_dashed",
                LineStyle::SparseDotted => "grid_style_sparse_dotted",
            },
            Self::SetGridHorzVisible(_) => "grid_horz_visible",
            Self::SetGridVertVisible(_) => "grid_vert_visible",
            Self::ToggleGridVertical => "toggle_grid_vertical",
            Self::ToggleGridHorizontal => "toggle_grid_horizontal",

            // Crosshair
            Self::SetCrosshairMode(mode) => match mode {
                CrosshairMode::Normal => "crosshair_normal",
                CrosshairMode::Magnet => "crosshair_magnet",
                CrosshairMode::MagnetOHLC => "crosshair_magnet_ohlc",
                CrosshairMode::Hidden => "crosshair_hidden",
            },
            Self::SetCrosshairStyle(style) => match style {
                LineStyle::Solid => "crosshair_style_solid",
                LineStyle::Dashed => "crosshair_style_dashed",
                LineStyle::Dotted => "crosshair_style_dotted",
                LineStyle::LargeDashed => "crosshair_style_large_dashed",
                LineStyle::SparseDotted => "crosshair_style_sparse_dotted",
            },
            Self::ToggleMagnet => "toggle_magnet",
            Self::ToggleCrosshairVertLine => "toggle_crosshair_vert_line",
            Self::ToggleCrosshairHorzLine => "toggle_crosshair_horz_line",
            Self::ToggleCrosshairVertLabel => "toggle_crosshair_vert_label",
            Self::ToggleCrosshairHorzLabel => "toggle_crosshair_horz_label",

            // Price lines / Markers
            Self::AddPriceLine => "add_priceline",
            Self::ClearPriceLines => "clear_pricelines",
            Self::AddMarker => "add_marker",
            Self::ClearMarkers => "clear_markers",

            // Data
            Self::RegenerateData => "regenerate_data",

            // View
            Self::ResetZoom => "reset_zoom",
            Self::FitContent => "fit_content",
            Self::ZoomIn => "zoom_in",
            Self::ZoomOut => "zoom_out",

            // Drawing tools
            Self::SelectTool(tool) => tool,
            Self::ToggleLockDrawings => "toggle_lock_drawings",
            Self::ToggleLockSelected => "toggle_lock_selected",
            Self::ToggleDrawingsVisible => "toggle_drawings_visible",
            Self::DeleteSelected => "delete_selected",
            Self::DeleteAll => "delete_all",

            // Symbol / Timeframe
            Self::OpenSymbolSearch => "open_symbol_search",
            Self::OpenSymbolHistory => "open_symbol_history",
            Self::SetSymbol(symbol) => symbol,
            Self::SetTimeframe(tf) => tf,

            // Dialogs
            Self::OpenIndicators => "open_indicators",
            Self::OpenCompare => "open_compare",
            Self::OpenSettings => "open_settings",
            Self::ToggleObjectTree => "toggle_object_tree",
            Self::ToggleSignals => "toggle_signals",
            Self::ToggleIndicators => "toggle_indicators",

            // Panels / Sidebars
            Self::ToggleLeftPanel => "toggle_left_panel",
            Self::ToggleRightPanel => "toggle_right_panel",
            Self::ToggleBottomPanel => "toggle_bottom_panel",
            Self::ToggleWatchlist => "toggle_watchlist",
            Self::ToggleAlerts => "toggle_alerts",
            Self::ToggleTradingPanel => "toggle_trading_panel",
            Self::TogglePositions => "toggle_positions",

            // History
            Self::Undo => "undo",
            Self::Redo => "redo",

            // Theme
            Self::SetTheme(theme) => theme,
            Self::OpenThemeSettings => "open_theme_settings",

            // UI Style
            Self::SetStyle(style) => style,

            // Custom
            Self::Custom(id) => id,
        }
    }

    /// Parse action from string ID
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            // Series
            "toggle_candles" => Self::ToggleCandles,
            "toggle_line" => Self::ToggleLine,
            "toggle_area" => Self::ToggleArea,
            "toggle_histogram" => Self::ToggleHistogram,
            "toggle_baseline" => Self::ToggleBaseline,

            // Overlays
            "toggle_legend" => Self::ToggleLegend,
            "toggle_tooltip" => Self::ToggleTooltip,
            "toggle_watermark" => Self::ToggleWatermark,
            "toggle_grid" => Self::ToggleGrid,
            "toggle_crosshair" => Self::ToggleCrosshair,

            // Legend
            "legend_pos_tl" => Self::SetLegendPosition(LegendPosition::TopLeft),
            "legend_pos_tr" => Self::SetLegendPosition(LegendPosition::TopRight),
            "legend_pos_bl" => Self::SetLegendPosition(LegendPosition::BottomLeft),
            "legend_pos_br" => Self::SetLegendPosition(LegendPosition::BottomRight),

            // Legend options
            "toggle_legend_ohlc" => Self::ToggleLegendOHLC,
            "toggle_legend_change" => Self::ToggleLegendChange,
            "toggle_legend_percent" => Self::ToggleLegendPercent,

            // Tooltip options
            "toggle_tooltip_follow" => Self::ToggleTooltipFollow,

            // Watermark options
            "watermark_pos_center" => Self::SetWatermarkPosition(HorzAlign::Center, VertAlign::Center),
            "watermark_pos_bl" => Self::SetWatermarkPosition(HorzAlign::Left, VertAlign::Bottom),
            "watermark_pos_br" => Self::SetWatermarkPosition(HorzAlign::Right, VertAlign::Bottom),
            "watermark_pos_tl" => Self::SetWatermarkPosition(HorzAlign::Left, VertAlign::Top),
            "watermark_pos_tr" => Self::SetWatermarkPosition(HorzAlign::Right, VertAlign::Top),

            // Grid
            "grid_style_solid" => Self::SetGridStyle(LineStyle::Solid),
            "grid_style_dashed" => Self::SetGridStyle(LineStyle::Dashed),
            "grid_style_dotted" => Self::SetGridStyle(LineStyle::Dotted),
            "grid_style_large_dashed" => Self::SetGridStyle(LineStyle::LargeDashed),
            "toggle_grid_vertical" => Self::ToggleGridVertical,
            "toggle_grid_horizontal" => Self::ToggleGridHorizontal,

            // Crosshair
            "crosshair_normal" => Self::SetCrosshairMode(CrosshairMode::Normal),
            "crosshair_magnet" => Self::SetCrosshairMode(CrosshairMode::Magnet),
            "crosshair_magnet_ohlc" => Self::SetCrosshairMode(CrosshairMode::MagnetOHLC),
            "crosshair_hidden" => Self::SetCrosshairMode(CrosshairMode::Hidden),
            "crosshair_style_solid" => Self::SetCrosshairStyle(LineStyle::Solid),
            "crosshair_style_dashed" => Self::SetCrosshairStyle(LineStyle::Dashed),
            "crosshair_style_dotted" => Self::SetCrosshairStyle(LineStyle::Dotted),
            "crosshair_style_large_dashed" => Self::SetCrosshairStyle(LineStyle::LargeDashed),
            "toggle_magnet" => Self::ToggleMagnet,
            "toggle_crosshair_vert_line" => Self::ToggleCrosshairVertLine,
            "toggle_crosshair_horz_line" => Self::ToggleCrosshairHorzLine,
            "toggle_crosshair_vert_label" => Self::ToggleCrosshairVertLabel,
            "toggle_crosshair_horz_label" => Self::ToggleCrosshairHorzLabel,

            // Price lines / Markers
            "add_priceline" => Self::AddPriceLine,
            "clear_pricelines" => Self::ClearPriceLines,
            "add_marker" => Self::AddMarker,
            "clear_markers" => Self::ClearMarkers,

            // Data
            "regenerate_data" => Self::RegenerateData,

            // View
            "reset_zoom" => Self::ResetZoom,
            "fit_content" => Self::FitContent,
            "zoom_in" => Self::ZoomIn,
            "zoom_out" => Self::ZoomOut,

            // Drawing tools
            "toggle_lock_drawings" => Self::ToggleLockDrawings,
            "toggle_lock_selected" => Self::ToggleLockSelected,
            "toggle_drawings_visible" => Self::ToggleDrawingsVisible,
            "delete_selected" => Self::DeleteSelected,
            "delete_all" => Self::DeleteAll,

            // Dialogs
            "open_symbol_search" => Self::OpenSymbolSearch,
            "open_symbol_history" => Self::OpenSymbolHistory,
            "open_indicators" => Self::OpenIndicators,
            "open_compare" => Self::OpenCompare,
            "open_settings" => Self::OpenSettings,
            "toggle_object_tree" => Self::ToggleObjectTree,
            "toggle_signals" => Self::ToggleSignals,
            "toggle_indicators" => Self::ToggleIndicators,

            // Panels / Sidebars
            "toggle_left_panel" => Self::ToggleLeftPanel,
            "toggle_right_panel" => Self::ToggleRightPanel,
            "toggle_bottom_panel" => Self::ToggleBottomPanel,
            "toggle_watchlist" => Self::ToggleWatchlist,
            "toggle_alerts" => Self::ToggleAlerts,
            "toggle_trading_panel" => Self::ToggleTradingPanel,
            "toggle_positions" => Self::TogglePositions,

            // History
            "undo" => Self::Undo,
            "redo" => Self::Redo,

            // Theme
            "theme_dark" => Self::SetTheme("dark"),
            "theme_light" => Self::SetTheme("light"),
            "theme_high_contrast" => Self::SetTheme("high_contrast"),
            "theme_high_contrast_mono" => Self::SetTheme("high_contrast_mono"),
            "theme_cypherpunk" => Self::SetTheme("cypherpunk"),
            "open_theme_settings" => Self::OpenThemeSettings,

            // UI Style
            "style_solid" => Self::SetStyle("solid"),
            "style_glass" => Self::SetStyle("glass"),
            "style_frosted_glass_flat" => Self::SetStyle("frosted_glass_flat"),
            "style_frosted_glass_3d" => Self::SetStyle("frosted_glass_3d"),
            "style_liquid_glass_flat" => Self::SetStyle("liquid_glass_flat"),
            "style_liquid_glass_3d" => Self::SetStyle("liquid_glass_3d"),

            _ => return None,
        })
    }

    /// Check if this is a tool selection action
    pub fn is_tool_selection(&self) -> bool {
        matches!(self, Self::SelectTool(_))
    }

    /// Check if this is a toggle action
    pub fn is_toggle(&self) -> bool {
        matches!(self,
            Self::ToggleCandles | Self::ToggleLine | Self::ToggleArea |
            Self::ToggleHistogram | Self::ToggleBaseline | Self::ToggleLegend |
            Self::ToggleTooltip | Self::ToggleWatermark | Self::ToggleGrid |
            Self::ToggleCrosshair | Self::ToggleMagnet | Self::ToggleLockDrawings |
            Self::ToggleLockSelected | Self::ToggleDrawingsVisible | Self::ToggleObjectTree |
            Self::ToggleSignals | Self::ToggleIndicators | Self::ToggleLegendOHLC | Self::ToggleLegendChange |
            Self::ToggleLegendPercent | Self::ToggleTooltipFollow | Self::ToggleGridVertical |
            Self::ToggleGridHorizontal | Self::ToggleCrosshairVertLine | Self::ToggleCrosshairHorzLine |
            Self::ToggleCrosshairVertLabel | Self::ToggleCrosshairHorzLabel |
            Self::ToggleRightPanel | Self::ToggleBottomPanel | Self::ToggleWatchlist |
            Self::ToggleAlerts | Self::ToggleTradingPanel | Self::TogglePositions
        )
    }
}

/// Keyboard shortcut definition
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub key: char,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Shortcut {
    pub const fn key(key: char) -> Self {
        Self {
            key,
            ctrl: false,
            shift: false,
            alt: false,
        }
    }

    pub const fn ctrl(key: char) -> Self {
        Self {
            key,
            ctrl: true,
            shift: false,
            alt: false,
        }
    }

    pub const fn ctrl_shift(key: char) -> Self {
        Self {
            key,
            ctrl: true,
            shift: true,
            alt: false,
        }
    }

    pub const fn shift(key: char) -> Self {
        Self {
            key,
            ctrl: false,
            shift: true,
            alt: false,
        }
    }

    pub const fn alt(key: char) -> Self {
        Self {
            key,
            ctrl: false,
            shift: false,
            alt: true,
        }
    }

    /// Format for display (e.g., "Ctrl+S", "M")
    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.alt {
            parts.push("Alt");
        }
        let key_str = match self.key {
            '\u{7F}' => "Del".to_string(),
            '\u{1B}' => "Esc".to_string(),
            '\t' => "Tab".to_string(),
            ' ' => "Space".to_string(),
            c => c.to_uppercase().to_string(),
        };
        parts.push(&key_str);
        parts.join("+")
    }

    /// Check if shortcut matches key event
    pub fn matches(&self, key: char, ctrl: bool, shift: bool, alt: bool) -> bool {
        self.key.to_ascii_lowercase() == key.to_ascii_lowercase()
            && self.ctrl == ctrl
            && self.shift == shift
            && self.alt == alt
    }
}
