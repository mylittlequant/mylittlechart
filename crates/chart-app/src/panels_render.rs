//! Render dispatch for `FreeItem` leaves inside the free-slot sidebars.
//!
//! `render_free_item` is the single entry point: it receives a reference to
//! the panel store and a `FreeItem`, looks up the matching state, and
//! delegates to the `TradingPanel::render()` trait method.
//!
//! All 11 trading panels are wired through `get_panel()`.

use sidebar_content::free_slot::FreeItem;
use zengeld_chart::render::RenderContext;

use zengeld_panels::panel_theme::PanelTheme;
use zengeld_panels::trading::SymbolSource;

use crate::panels_store::TradingPanelsStore;

/// Info extracted from panel state for rendering the panel header bar.
pub struct PanelHeaderInfo {
    /// Display label for the SymbolSource mode.
    pub source_label: &'static str,
    /// Background color for the source pill.
    pub source_color: &'static str,
    /// Resolved symbol (e.g. "SOLUSDT").
    pub symbol: String,
    /// Whether this panel is a DOM (shows zoom/center controls).
    pub is_dom: bool,
}


/// Extract header info for a given FreeItem from the store.
pub fn panel_header_info(
    store: &TradingPanelsStore,
    item: &FreeItem,
) -> Option<PanelHeaderInfo> {
    fn source_label(s: &SymbolSource) -> &'static str {
        match s {
            SymbolSource::HyperFocus => "Auto",
            SymbolSource::Fixed { .. } => "Pinned",
            SymbolSource::BoundToChart { .. } => "Linked",
        }
    }
    fn source_color(s: &SymbolSource) -> &'static str {
        match s {
            SymbolSource::HyperFocus => "#374151",
            SymbolSource::Fixed { .. } => "#2563eb",
            SymbolSource::BoundToChart { .. } => "#16a34a",
        }
    }

    match item {
        FreeItem::Dom(id) => store.dom.get(id).map(|s| PanelHeaderInfo {
            source_label: source_label(&s.source),
            source_color: source_color(&s.source),
            symbol: s.symbol.clone(),
            is_dom: true,
        }),
        FreeItem::Footprint(id) => store.footprint.get(id).map(|s| PanelHeaderInfo {
            source_label: source_label(&s.source),
            source_color: source_color(&s.source),
            symbol: s.symbol.clone(),
            is_dom: false,
        }),
        FreeItem::VolumeProfile(id) => store.volume_profile.get(id).map(|s| PanelHeaderInfo {
            source_label: source_label(&s.source),
            source_color: source_color(&s.source),
            symbol: s.symbol.clone(),
            is_dom: false,
        }),
        FreeItem::LiquidityHeatmap(id) => store.liquidity_heatmap.get(id).map(|s| PanelHeaderInfo {
            source_label: source_label(&s.source),
            source_color: source_color(&s.source),
            symbol: s.symbol.clone(),
            is_dom: false,
        }),
        FreeItem::BigTrades(id) => store.big_trades.get(id).map(|s| PanelHeaderInfo {
            source_label: source_label(&s.source),
            source_color: source_color(&s.source),
            symbol: s.symbol.clone(),
            is_dom: false,
        }),
        FreeItem::L2Tape(id) => store.l2_tape.get(id).map(|s| PanelHeaderInfo {
            source_label: source_label(&s.source),
            source_color: source_color(&s.source),
            symbol: s.symbol.clone(),
            is_dom: false,
        }),
        FreeItem::OrderEntry(id) => store.order_entry.get(id).map(|s| PanelHeaderInfo {
            source_label: source_label(&s.source),
            source_color: source_color(&s.source),
            symbol: s.symbol.clone(),
            is_dom: false,
        }),
        FreeItem::TradingContainer(id) => store.trading_container.get(id).map(|s| PanelHeaderInfo {
            source_label: source_label(&s.source),
            source_color: source_color(&s.source),
            symbol: s.symbol.clone(),
            is_dom: false,
        }),
        // Panels without SymbolSource
        FreeItem::PositionManager(_) | FreeItem::TradeLog(_) | FreeItem::RiskCalculator(_) => None,
    }
}

/// Build a `PanelTheme` from the active `RuntimeTheme`.
///
/// Common semantic colors (success, danger, warning, text, accent) are pulled
/// from the runtime theme. Panel-specific colors come from `rt.trading`.
pub fn panel_theme_from_runtime(rt: &zengeld_chart::theme::RuntimeTheme) -> zengeld_panels::panel_theme::PanelTheme {
    let t = &rt.trading;
    zengeld_panels::panel_theme::PanelTheme {
        // Common — mapped from RuntimeTheme base colors
        panel_bg:      t.panel_bg.clone(),
        row_bg_alt:    t.row_bg_alt.clone(),
        header_bg:     t.header_bg.clone(),
        separator:     rt.colors.divider.clone(),

        text_primary:  rt.colors.text_primary.clone(),
        text_muted:    rt.colors.text_muted.clone(),
        text_header:   rt.colors.text_secondary.clone(),

        buy:           rt.colors.success.clone(),
        buy_bright:    t.buy_bright.clone(),
        sell:          rt.colors.danger.clone(),
        sell_bright:   t.sell_bright.clone(),

        current_price: rt.colors.warning.clone(),
        hover:         t.hover.clone(),
        selected:      t.selected.clone(),
        accent:        rt.colors.accent.clone(),

        // Panel-specific — direct from trading.*
        dom_spread_bg:    t.dom_spread_bg.clone(),
        dom_best_bid_bg:  t.dom_best_bid_bg.clone(),
        dom_best_ask_bg:  t.dom_best_ask_bg.clone(),
        dom_user_order:   t.dom_user_order.clone(),

        fp_cell_text:  t.fp_cell_text.clone(),
        fp_poc_marker: t.fp_poc_marker.clone(),
        fp_poc_border: t.fp_poc_border.clone(),
        fp_bullish:    t.fp_bullish.clone(),

        vp_bar:        t.vp_bar.clone(),
        vp_bar_poc:    t.vp_bar_poc.clone(),
        vp_poc_line:   t.vp_poc_line.clone(),
        vp_vah_line:   t.vp_vah_line.clone(),
        vp_val_line:   t.vp_val_line.clone(),
        vp_value_area: t.vp_value_area.clone(),

        heatmap_price_line: t.heatmap_price_line.clone(),

        oe_tab_active:       t.oe_tab_active.clone(),
        oe_tab_inactive:     t.oe_tab_inactive.clone(),
        oe_input_bg:         t.oe_input_bg.clone(),
        oe_input_border:     t.oe_input_border.clone(),
        oe_buy_button:       t.oe_buy_button.clone(),
        oe_sell_button:      t.oe_sell_button.clone(),
        oe_buy_button_text:  t.oe_buy_button_text.clone(),
        oe_sell_button_text: t.oe_sell_button_text.clone(),

        pm_pnl_positive: t.pm_pnl_positive.clone(),
        pm_pnl_negative: t.pm_pnl_negative.clone(),
        pm_pnl_neutral:  t.pm_pnl_neutral.clone(),
        pm_long:         t.pm_long.clone(),
        pm_short:        t.pm_short.clone(),
        pm_liquidation:  t.pm_liquidation.clone(),
        pm_summary_bg:   t.pm_summary_bg.clone(),

        tl_row_bg_alt: t.tl_row_bg_alt.clone(),
        tl_profit:     t.tl_profit.clone(),
        tl_loss:       t.tl_loss.clone(),

        rc_risk:     t.rc_risk.clone(),
        rc_profit:   t.rc_profit.clone(),
        rc_good_rr:  t.rc_good_rr.clone(),
        rc_input_bg: t.rc_input_bg.clone(),

        tc_bg:        t.tc_bg.clone(),
        tc_inner_bg:  t.tc_inner_bg.clone(),
        tc_separator: t.tc_separator.clone(),
    }
}

/// Render the content of a `FreeItem` leaf into `(x, y, w, h)`.
///
/// If the item's state is missing from the store (e.g. the panel was removed
/// between frames), the slot is left blank. The function never panics.
pub fn render_free_item(
    store: &TradingPanelsStore,
    item: &FreeItem,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    ctx: &mut dyn RenderContext,
    theme: &PanelTheme,
) {
    if let Some(panel) = store.get_panel(item) {
        panel.render(ctx, x, y, w, h, theme);
    }
}
