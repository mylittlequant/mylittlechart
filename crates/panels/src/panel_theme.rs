//! Theme colors for all trading panels.
//!
//! `PanelTheme` is a self-contained struct of hex color strings. It lives in
//! `zengeld-panels` which does **not** depend on `zengeld-chart`, so the type
//! must carry no chart-side types. Chart-app constructs a `PanelTheme` from
//! the runtime theme and passes it into every `TradingPanel::render` call.

/// Theme colors for all trading panels.
///
/// All fields are hex color strings with alpha (e.g. `"#1a1d23ff"`).
/// Built from `RuntimeTheme` in chart-app and passed down at render time.
#[derive(Clone, Debug)]
pub struct PanelTheme {
    // === Common (shared across panels) ===
    /// Panel background
    pub panel_bg: String,
    /// Alternate row background (zebra striping)
    pub row_bg_alt: String,
    /// Header / section background
    pub header_bg: String,
    /// Separator / border lines
    pub separator: String,

    // Text
    pub text_primary: String,
    pub text_muted: String,
    pub text_header: String,

    // Buy/Sell semantic colors
    /// Green for bids
    pub buy: String,
    /// Bright green (best bid, fills)
    pub buy_bright: String,
    /// Red for asks
    pub sell: String,
    /// Bright red (best ask, fills)
    pub sell_bright: String,

    // Highlights
    /// Gold/yellow — current market price
    pub current_price: String,
    /// Hover row highlight
    pub hover: String,
    /// Selected row
    pub selected: String,
    /// Accent / blue
    pub accent: String,

    // === DOM-specific ===
    pub dom_spread_bg: String,
    pub dom_best_bid_bg: String,
    pub dom_best_ask_bg: String,
    pub dom_user_order: String,

    // === Footprint-specific ===
    pub fp_cell_text: String,
    pub fp_poc_marker: String,
    pub fp_poc_border: String,
    pub fp_bullish: String,

    // === Volume Profile ===
    pub vp_bar: String,
    pub vp_bar_poc: String,
    pub vp_poc_line: String,
    pub vp_vah_line: String,
    pub vp_val_line: String,
    pub vp_value_area: String,

    // === Liquidity Heatmap ===
    pub heatmap_price_line: String,

    // === Order Entry ===
    pub oe_tab_active: String,
    pub oe_tab_inactive: String,
    pub oe_input_bg: String,
    pub oe_input_border: String,
    pub oe_buy_button: String,
    pub oe_sell_button: String,
    pub oe_buy_button_text: String,
    pub oe_sell_button_text: String,

    // === Position Manager ===
    pub pm_pnl_positive: String,
    pub pm_pnl_negative: String,
    pub pm_pnl_neutral: String,
    pub pm_long: String,
    pub pm_short: String,
    pub pm_liquidation: String,
    pub pm_summary_bg: String,

    // === Trade Log ===
    pub tl_row_bg_alt: String,
    pub tl_profit: String,
    pub tl_loss: String,

    // === Risk Calculator ===
    pub rc_risk: String,
    pub rc_profit: String,
    pub rc_good_rr: String,
    pub rc_input_bg: String,

    // === Trading Container ===
    pub tc_bg: String,
    pub tc_inner_bg: String,
    pub tc_separator: String,
}

impl PanelTheme {
    /// Default dark theme constructed from the hardcoded color values
    /// that were previously scattered across all 11 panel files as `const`.
    pub fn dark_default() -> Self {
        Self {
            // Common
            panel_bg:     "#0d1117ff".to_string(),
            row_bg_alt:   "#10151bff".to_string(),
            header_bg:    "#161b22ff".to_string(),
            separator:    "#30363dff".to_string(),

            // Text
            text_primary: "#e0e0e0ff".to_string(),
            text_muted:   "#8b949eff".to_string(),
            text_header:  "#8091a5ff".to_string(),

            // Buy / Sell
            buy:          "#2ea043ff".to_string(),
            buy_bright:   "#00ff87ff".to_string(),
            sell:         "#cc2233ff".to_string(),
            sell_bright:  "#ff4466ff".to_string(),

            // Highlights
            current_price: "#ffde00ff".to_string(),
            hover:         "#2a2f40ff".to_string(),
            selected:      "#1e2538ff".to_string(),
            accent:        "#58a6ffff".to_string(),

            // DOM
            dom_spread_bg:    "#14141eff".to_string(),
            dom_best_bid_bg:  "#0a3520ff".to_string(),
            dom_best_ask_bg:  "#3a100aff".to_string(),
            dom_user_order:   "#58a6ffff".to_string(),

            // Footprint
            fp_cell_text:  "#e0e0e0ff".to_string(),
            fp_poc_marker: "#ffde00ff".to_string(),
            fp_poc_border: "#b8860bff".to_string(),
            fp_bullish:    "#2ea043ff".to_string(),

            // Volume Profile
            vp_bar:        "#6699cc80".to_string(),
            vp_bar_poc:    "#88bbffff".to_string(),
            vp_poc_line:   "#ffde00ff".to_string(),
            vp_vah_line:   "#da363380".to_string(),
            vp_val_line:   "#2ea04380".to_string(),
            vp_value_area: "#58a6ff20".to_string(),

            // Liquidity Heatmap
            heatmap_price_line: "#ffde00ff".to_string(),

            // Order Entry
            oe_tab_active:      "#58a6ffff".to_string(),
            oe_tab_inactive:    "#21262dff".to_string(),
            oe_input_bg:        "#0d1117ff".to_string(),
            oe_input_border:    "#30363dff".to_string(),
            oe_buy_button:      "#2ea043ff".to_string(),
            oe_sell_button:     "#cc2233ff".to_string(),
            oe_buy_button_text: "#ffffffff".to_string(),
            oe_sell_button_text:"#ffffffff".to_string(),

            // Position Manager
            pm_pnl_positive: "#3fb950ff".to_string(),
            pm_pnl_negative: "#f85149ff".to_string(),
            pm_pnl_neutral:  "#8b949eff".to_string(),
            pm_long:         "#2ea043ff".to_string(),
            pm_short:        "#cc2233ff".to_string(),
            pm_liquidation:  "#f0883eff".to_string(),
            pm_summary_bg:   "#161b22ff".to_string(),

            // Trade Log
            tl_row_bg_alt: "#161b22ff".to_string(),
            tl_profit:     "#3fb950ff".to_string(),
            tl_loss:       "#f85149ff".to_string(),

            // Risk Calculator
            rc_risk:     "#cc2233ff".to_string(),
            rc_profit:   "#2ea043ff".to_string(),
            rc_good_rr:  "#ffde00ff".to_string(),
            rc_input_bg: "#0d1117ff".to_string(),

            // Trading Container
            tc_bg:        "#0a0a0fff".to_string(),
            tc_inner_bg:  "#1c1c29ff".to_string(),
            tc_separator: "#33333fff".to_string(),
        }
    }
}
