//! Expanded watchlist overlay modal renderer.
//!
//! Shows a full-size modal with three tabs:
//!   - Overview: searchable, scrollable list of watchlist symbols with price data
//!   - Groups: placeholder for group management
//!   - Settings: placeholder for column settings

use crate::engine::render::RenderContext;
use crate::engine::render::draw_svg_icon;
use uzor::render::{TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;
use crate::ui::modal_settings::{WatchlistModalState, WatchlistModalTab, WatchlistGroupNameInputState, WatchlistGroupNameMode};
use crate::ui::toolbar_render::ToolbarTheme;
use crate::ui::widgets::{render_modal_frame_only, ModalTheme};
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig};
use crate::ui::widgets::types::WidgetState;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::layout::render_chart::FrameTheme;
use crate::ui::z_order::ZLayer;
use crate::ui::Icon;
use crate::ui::scroll_widget::{ScrollbarConfig, ScrollbarState, draw_scrollbar};

// =============================================================================
// Public data types for modal items
// =============================================================================

/// A single watchlist group (named preset/list) passed into the Groups tab.
///
/// The chart crate is isolated from higher-level crates, so we define a
/// minimal local type here instead of importing from `sidebar_content`.
pub struct WatchlistGroupInfo {
    /// Unique id of the watchlist list.
    pub id: u64,
    /// Display name of the list.
    pub name: String,
    /// Optional accent color (CSS hex string). Empty string = no color.
    pub color: String,
    /// Number of symbols contained in this list.
    pub symbol_count: usize,
    /// True when this is the currently active list.
    pub is_active: bool,
}

/// A single item in the watchlist, passed into the modal renderer.
///
/// The chart crate is isolated from higher-level crates, so we define a
/// minimal local type here instead of importing from `sidebar_content`.
pub struct WatchlistEntry {
    /// Ticker symbol, e.g. "BTCUSDT".
    pub symbol: String,
    /// Exchange name, e.g. "Binance".
    pub exchange: String,
    /// Last traded price.
    pub price: f64,
    /// Percentage change (positive = up, negative = down).
    pub change_pct: f64,
    /// Absolute price change.
    pub change_abs: f64,
    /// 24-hour high price.
    pub high_24h: f64,
    /// 24-hour low price.
    pub low_24h: f64,
    /// 24-hour volume (base asset).
    pub volume_24h: f64,
    /// Color flag hex string (e.g. "#ef5350"), empty if no flag.
    pub color_flag: String,
    /// Account type short label (e.g. "FC" for FuturesCross, "M" for Margin).
    ///
    /// Empty string for Spot (the common case).
    pub account_type: String,
}

// =============================================================================
// Result type
// =============================================================================

/// Render result from the watchlist modal.
#[derive(Clone, Debug, Default)]
pub struct WatchlistModalResult {
    /// The modal outer rectangle (for backdrop hit-testing).
    pub modal_rect: WidgetRect,
    /// Header rectangle — used for drag detection.
    pub header_rect: WidgetRect,
    /// Close (X) button rectangle.
    pub close_btn_rect: WidgetRect,
    /// Tab hit zones: (tab label string, rect).
    pub tab_rects: Vec<(String, WidgetRect)>,
    /// Per-item hit zones: (symbol string, row rect).
    pub item_rects: Vec<(String, WidgetRect)>,
    /// Per-item delete button hit zones: (symbol string, btn rect).
    pub delete_btn_rects: Vec<(String, WidgetRect)>,
    /// Scrollable list viewport rect (used for scroll event routing).
    pub list_viewport_rect: WidgetRect,
    /// Total pixel height of all rendered items (for clamping scroll).
    pub total_content_height: f64,
    /// X positions of each character boundary in the search input (for click-to-cursor).
    pub search_char_positions: Vec<f64>,
    /// Search input field rectangle (for drag-to-select hit testing).
    pub search_input_rect: WidgetRect,
    /// Scrollbar handle rectangle (for drag detection).
    pub scrollbar_handle_rect: Option<WidgetRect>,
    /// Scrollbar track rectangle (for track-click detection).
    pub scrollbar_track_rect: Option<WidgetRect>,
}

// =============================================================================
// Renderer
// =============================================================================

/// Render the expanded watchlist overlay modal.
///
/// Returns hit-zone information for click dispatch, drag, and scroll handling.
pub fn render_watchlist_modal(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    state: &WatchlistModalState,
    items: &[WatchlistEntry],
    groups: &[WatchlistGroupInfo],
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> WatchlistModalResult {
    let mut result = WatchlistModalResult::default();

    // -------------------------------------------------------------------------
    // Layout constants
    // -------------------------------------------------------------------------
    let modal_w = (screen_w * 0.65).clamp(380.0, 600.0);
    let modal_h = (screen_h * 0.8).clamp(300.0, 600.0);
    let header_h = 44.0;
    let tab_bar_h = 32.0;
    let search_h = 36.0;
    let col_header_h = 24.0;
    let item_h = 28.0;
    let padding = 16.0;
    let input_h = 26.0;
    let icon_size = 16.0;
    let icon_btn_size = 22.0;

    // -------------------------------------------------------------------------
    // Position (draggable; centered by default)
    // -------------------------------------------------------------------------
    let (modal_x, modal_y) = state.position.unwrap_or_else(|| {
        ((screen_w - modal_w) / 2.0, (screen_h - modal_h) / 2.0)
    });
    let modal_x = modal_x.max(0.0).min(screen_w - modal_w);
    let modal_y = modal_y.max(0.0).min(screen_h - modal_h);

    let modal_rect = WidgetRect::new(modal_x, modal_y, modal_w, modal_h);
    result.modal_rect = modal_rect;

    // -------------------------------------------------------------------------
    // Modal frame (blur + background + border)
    // -------------------------------------------------------------------------
    let modal_theme = ModalTheme::from_frame_theme(
        &frame_theme.toolbar_bg,
        &toolbar_theme.separator,
        &toolbar_theme.item_text,
        &toolbar_theme.item_text_hover,
        &toolbar_theme.separator,
    );
    render_modal_frame_only(ctx, modal_rect, &modal_theme, 0.0);

    // -------------------------------------------------------------------------
    // InputCoordinator layer
    // -------------------------------------------------------------------------
    let layer_id = ZLayer::Modal.push_named(input_coordinator, "watchlist_modal");

    // Modal background absorbs clicks that don't hit any widget.
    input_coordinator.register_on_layer(
        "wl_modal:modal_bg",
        uzor::types::Rect::new(modal_x, modal_y, modal_w, modal_h),
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    // ==========================================================================
    // Header
    // ==========================================================================

    let header_rect = WidgetRect::new(modal_x, modal_y, modal_w, header_h);
    result.header_rect = header_rect;

    // Header background
    ctx.set_fill_color(&frame_theme.toolbar_bg);
    ctx.fill_rect(modal_x, modal_y, modal_w, header_h);

    // Title
    ctx.set_font("bold 14px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Watchlist", modal_x + padding, modal_y + header_h / 2.0);

    // Close button
    let close_x = modal_x + modal_w - icon_size - 14.0;
    let close_y = modal_y + (header_h - icon_size) / 2.0;
    let close_rect = WidgetRect::new(close_x, close_y, icon_size, icon_size);
    result.close_btn_rect = close_rect;

    let close_hovered = state.hovered_widget.as_deref() == Some("wl_modal:close");
    let close_color = if close_hovered {
        &toolbar_theme.item_text
    } else {
        &toolbar_theme.item_text_muted
    };
    draw_svg_icon(
        ctx, Icon::Close.svg(),
        close_x, close_y,
        icon_size, icon_size,
        close_color,
    );

    input_coordinator.register_on_layer(
        "wl_modal:close",
        uzor::types::Rect::new(close_x, close_y, icon_size, icon_size),
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    // Header separator
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, modal_y + header_h);
    ctx.line_to(modal_x + modal_w, modal_y + header_h);
    ctx.stroke();

    // Register header drag zone
    input_coordinator.register_on_layer(
        "wl_modal:header_drag",
        uzor::types::Rect::new(modal_x, modal_y, modal_w - icon_size - 14.0, header_h),
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    // ==========================================================================
    // Tab bar
    // ==========================================================================

    let tab_bar_y = modal_y + header_h;
    // Only Overview and Groups tabs — Settings tab removed.
    let tabs: &[(&str, &str, WatchlistModalTab)] = &[
        ("Overview", "wl_modal:tab:overview", WatchlistModalTab::Overview),
        ("Groups",   "wl_modal:tab:groups",   WatchlistModalTab::Groups),
    ];
    let tab_w = modal_w / tabs.len() as f64;

    for (_i, (label, widget_id, this_tab)) in tabs.iter().enumerate() {
        let tab_x = modal_x + _i as f64 * tab_w;
        let tab_rect = WidgetRect::new(tab_x, tab_bar_y, tab_w, tab_bar_h);

        let is_active = state.active_tab == *this_tab;
        let is_tab_hovered = state.hovered_widget.as_deref() == Some(*widget_id);

        if is_active {
            ctx.set_fill_color(&toolbar_theme.item_bg_active);
            ctx.fill_rect(tab_x, tab_bar_y, tab_w, tab_bar_h);
        } else if is_tab_hovered {
            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rect(tab_x, tab_bar_y, tab_w, tab_bar_h);
        }

        let text_color = if is_active {
            &toolbar_theme.item_text_active
        } else if is_tab_hovered {
            &toolbar_theme.item_text
        } else {
            &toolbar_theme.item_text_muted
        };
        ctx.set_fill_color(text_color);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, tab_x + tab_w / 2.0, tab_bar_y + tab_bar_h / 2.0);

        result.tab_rects.push((label.to_string(), tab_rect));
        input_coordinator.register_on_layer(
            *widget_id,
            uzor::types::Rect::new(tab_x, tab_bar_y, tab_w, tab_bar_h),
            uzor::input::Sense::CLICK,
            &layer_id,
        );
    }

    // Tab bar bottom separator
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, tab_bar_y + tab_bar_h);
    ctx.line_to(modal_x + modal_w, tab_bar_y + tab_bar_h);
    ctx.stroke();

    // ==========================================================================
    // Content area — dispatch by active tab
    // ==========================================================================

    let content_top = modal_y + header_h + tab_bar_h;
    let content_h = modal_h - header_h - tab_bar_h;

    match state.active_tab {
        WatchlistModalTab::Overview => {
            render_overview_tab(
                ctx,
                modal_x, modal_y,
                modal_w, modal_h,
                content_top, content_h,
                header_h, tab_bar_h,
                search_h, col_header_h, item_h,
                padding, input_h, icon_size, icon_btn_size,
                state,
                items,
                frame_theme,
                toolbar_theme,
                current_time_ms,
                input_coordinator,
                &layer_id,
                &mut result,
            );
        }
        WatchlistModalTab::Groups => {
            render_groups_tab(
                ctx,
                modal_x, content_top, modal_w, content_h,
                padding, icon_size,
                state,
                groups,
                toolbar_theme,
                input_coordinator,
                &layer_id,
            );
        }
        WatchlistModalTab::Settings => {
            render_placeholder_tab(
                ctx,
                modal_x, content_top, modal_w, content_h,
                "Column settings",
                toolbar_theme,
            );
        }
    }

    input_coordinator.pop_layer(&layer_id);
    result
}

// =============================================================================
// Overview tab
// =============================================================================

/// Column widths as fractions of `list_w` (the usable list area width after
/// subtracting the flag stripe).  Must sum to <= 1.0.
struct ColLayout {
    /// Left edge of each column relative to `list_content_x` (after flag).
    sym_x: f64,
    exch_x: f64,
    /// Right boundary of the LAST column (used as the right edge for CHG% column).
    chg_pct_x: f64,
    /// Right boundary of the CHG% column (used as right edge for CHG ABS column).
    chg_abs_x: f64,
    /// Right boundary of the CHG ABS column (used as right edge for HIGH column).
    high_x: f64,
    /// Right boundary of the HIGH column (used as right edge for LOW column).
    low_x: f64,
    /// Right boundary of the LOW column (used as right edge for VOL column).
    vol_x: f64,
    /// Start of the delete zone (right boundary of the VOL column).
    del_x: f64,
}

impl ColLayout {
    fn compute(list_x: f64, list_w: f64) -> Self {
        // Fixed left stripe for color flag
        let flag_w = 6.0;
        let available = list_w - flag_w;

        // Proportional column widths (fractions of `available`)
        let sym_frac   = 0.18_f64;
        let exch_frac  = 0.12_f64;
        let last_frac  = 0.12_f64;
        let pct_frac   = 0.10_f64;
        let abs_frac   = 0.10_f64;
        let high_frac  = 0.10_f64;
        let low_frac   = 0.10_f64;
        let vol_frac   = 0.10_f64;
        // Delete zone gets the remainder (~0.18)

        let sym_w   = available * sym_frac;
        let exch_w  = available * exch_frac;
        let last_w  = available * last_frac;
        let pct_w   = available * pct_frac;
        let abs_w   = available * abs_frac;
        let high_w  = available * high_frac;
        let low_w   = available * low_frac;
        let vol_w   = available * vol_frac;

        // Compute right edges (used for right-aligned columns)
        let sym_x     = list_x + flag_w;
        let exch_x    = sym_x + sym_w;
        let chg_pct_x = exch_x + exch_w + last_w;  // right edge after LAST col
        let chg_abs_x = chg_pct_x + pct_w;
        let high_x    = chg_abs_x + abs_w;
        let low_x     = high_x + high_w;
        let vol_x     = low_x + low_w;
        let del_x     = vol_x + vol_w;

        Self {
            sym_x,
            exch_x,
            chg_pct_x,
            chg_abs_x,
            high_x,
            low_x,
            vol_x,
            del_x,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_overview_tab(
    ctx: &mut dyn RenderContext,
    modal_x: f64,
    _modal_y: f64,
    modal_w: f64,
    _modal_h: f64,
    content_top: f64,
    content_h: f64,
    _header_h: f64,
    _tab_bar_h: f64,
    search_h: f64,
    col_header_h: f64,
    item_h: f64,
    padding: f64,
    input_h: f64,
    icon_size: f64,
    icon_btn_size: f64,
    state: &WatchlistModalState,
    items: &[WatchlistEntry],
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut WatchlistModalResult,
) {
    // -------------------------------------------------------------------------
    // Search input row
    // -------------------------------------------------------------------------
    let search_row_y = content_top;
    let search_input_x = modal_x + padding;
    let search_input_w = modal_w - padding * 2.0;
    let search_input_y = search_row_y + (search_h - input_h) / 2.0;
    let search_input_rect = WidgetRect::new(search_input_x, search_input_y, search_input_w, input_h);

    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let input_config = InputConfig::new(&state.search_editing.text)
        .with_focused(true)
        .with_cursor(state.search_editing.cursor)
        .with_placeholder("Search symbols...")
        .with_selection(state.search_editing.selection_start, Some(state.search_editing.cursor))
        .with_padding(26.0);

    let search_input_result = draw_input(ctx, &input_config, WidgetState::Normal, search_input_rect, &widget_theme);
    result.search_char_positions = search_input_result.char_x_positions.clone();
    result.search_input_rect = search_input_rect;

    // Search icon inside the input
    let search_icon_size = 13.0;
    draw_svg_icon(
        ctx, Icon::Search.svg(),
        search_input_x + 7.0,
        search_input_y + (input_h - search_icon_size) / 2.0,
        search_icon_size, search_icon_size,
        &toolbar_theme.item_text_muted,
    );

    // Blinking cursor for search input
    if state.search_editing.is_cursor_visible(current_time_ms) {
        draw_input_cursor(
            ctx,
            search_input_result.cursor_x,
            search_input_result.cursor_y,
            search_input_result.cursor_height,
            &toolbar_theme.item_text,
        );
    }

    input_coordinator.register_on_layer(
        "wl_modal:search_input",
        uzor::types::Rect::new(search_input_x, search_input_y, search_input_w, input_h),
        uzor::input::Sense::CLICK,
        layer_id,
    );

    // Search row bottom separator
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, search_row_y + search_h);
    ctx.line_to(modal_x + modal_w, search_row_y + search_h);
    ctx.stroke();

    // -------------------------------------------------------------------------
    // Column layout
    // -------------------------------------------------------------------------
    let list_x = modal_x;
    let list_w = modal_w;
    let cols = ColLayout::compute(list_x, list_w);

    // -------------------------------------------------------------------------
    // Column header row
    // -------------------------------------------------------------------------
    let col_header_y = search_row_y + search_h;
    let header_mid_y = col_header_y + col_header_h / 2.0;

    ctx.set_font("bold 10px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_baseline(TextBaseline::Middle);

    // SYMBOL — left-aligned from sym_x
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text("SYMBOL", cols.sym_x, header_mid_y);

    // EXCHANGE — left-aligned from exch_x
    ctx.fill_text("EXCHANGE", cols.exch_x, header_mid_y);

    // LAST — right-aligned, right edge at chg_pct_x
    ctx.set_text_align(TextAlign::Right);
    ctx.fill_text("LAST", cols.chg_pct_x - 4.0, header_mid_y);

    // CHG% — right-aligned, right edge at chg_abs_x
    ctx.fill_text("CHG%", cols.chg_abs_x - 4.0, header_mid_y);

    // CHG — right-aligned, right edge at high_x
    ctx.fill_text("CHG", cols.high_x - 4.0, header_mid_y);

    // HIGH — right-aligned, right edge at low_x
    ctx.fill_text("HIGH", cols.low_x - 4.0, header_mid_y);

    // LOW — right-aligned, right edge at vol_x
    ctx.fill_text("LOW", cols.vol_x - 4.0, header_mid_y);

    // VOL — right-aligned, right edge at del_x
    ctx.fill_text("VOL", cols.del_x - 4.0, header_mid_y);

    // Column header separator
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, col_header_y + col_header_h);
    ctx.line_to(modal_x + modal_w, col_header_y + col_header_h);
    ctx.stroke();

    // -------------------------------------------------------------------------
    // Scrollable item list
    // -------------------------------------------------------------------------
    let list_top = col_header_y + col_header_h;
    let list_h = content_top + content_h - list_top;
    let list_viewport_rect = WidgetRect::new(modal_x, list_top, modal_w, list_h);
    result.list_viewport_rect = list_viewport_rect;

    // Filter items by search query
    let query = state.search_query.to_lowercase();
    let filtered: Vec<&WatchlistEntry> = items
        .iter()
        .filter(|e| {
            if query.is_empty() {
                true
            } else {
                e.symbol.to_lowercase().contains(&query)
            }
        })
        .collect();

    let total_h = filtered.len() as f64 * item_h;
    result.total_content_height = total_h;

    // Clip rendering to the viewport
    ctx.save();
    ctx.clip_rect(modal_x, list_top, modal_w, list_h);

    let scroll = state.scroll.offset;
    let mut current_y = list_top - scroll;

    for entry in &filtered {
        let item_rect = WidgetRect::new(modal_x, current_y, modal_w, item_h);

        // Delete button position (always computed for hit testing)
        let del_center_x = cols.del_x + (list_w - cols.del_x + list_x) / 2.0;
        let delete_btn_w = icon_btn_size;
        let delete_x = (del_center_x - delete_btn_w / 2.0).min(modal_x + modal_w - delete_btn_w - 4.0);
        let delete_y = current_y + (item_h - icon_size) / 2.0;
        let delete_rect = WidgetRect::new(delete_x, delete_y, icon_btn_size, icon_btn_size);

        // Only draw visible rows
        if current_y + item_h >= list_top && current_y <= list_top + list_h {
            let entry_key = format!("{}:{}:{}", entry.symbol, entry.exchange, entry.account_type);
            let is_hovered = state.hovered_item_id.as_deref() == Some(entry_key.as_str());
            let row_mid_y = current_y + item_h / 2.0;

            if is_hovered {
                ctx.draw_hover_rect(modal_x, current_y, modal_w, item_h, &toolbar_theme.item_bg_hover);
            }

            // Color flag stripe — 4px wide on the far left of the row
            if !entry.color_flag.is_empty() {
                ctx.set_fill_color(&entry.color_flag);
                ctx.fill_rect(modal_x, current_y + 2.0, 4.0, item_h - 4.0);
            }

            // Symbol name (12px bold, left-aligned)
            ctx.set_font("bold 12px sans-serif");
            ctx.set_fill_color(&toolbar_theme.item_text);
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(&entry.symbol, cols.sym_x, row_mid_y);

            // Exchange (10px, muted, left-aligned)
            ctx.set_font("10px sans-serif");
            ctx.set_fill_color(&toolbar_theme.item_text_muted);
            ctx.fill_text(&entry.exchange, cols.exch_x, row_mid_y);

            // Last price (11px, item_text color, right-aligned)
            ctx.set_font("11px sans-serif");
            ctx.set_fill_color(&toolbar_theme.item_text);
            ctx.set_text_align(TextAlign::Right);
            let price_str = format_price(entry.price);
            ctx.fill_text(&price_str, cols.chg_pct_x - 4.0, row_mid_y);

            // Change % (11px, green/red, right-aligned)
            let change_color: &str = if entry.change_pct >= 0.0 {
                "#26a69a"
            } else {
                "#ef5350"
            };
            ctx.set_fill_color(change_color);
            let chg_pct_str = format!("{:+.2}%", entry.change_pct);
            ctx.fill_text(&chg_pct_str, cols.chg_abs_x - 4.0, row_mid_y);

            // Change absolute (11px, green/red, right-aligned)
            let chg_abs_str = if entry.change_abs >= 0.0 {
                format!("+{}", format_price(entry.change_abs))
            } else {
                format!("-{}", format_price(entry.change_abs.abs()))
            };
            ctx.fill_text(&chg_abs_str, cols.high_x - 4.0, row_mid_y);

            // High 24h (10px, muted, right-aligned)
            ctx.set_font("10px sans-serif");
            ctx.set_fill_color(&toolbar_theme.item_text_muted);
            let high_str = format_price(entry.high_24h);
            ctx.fill_text(&high_str, cols.low_x - 4.0, row_mid_y);

            // Low 24h (10px, muted, right-aligned)
            let low_str = format_price(entry.low_24h);
            ctx.fill_text(&low_str, cols.vol_x - 4.0, row_mid_y);

            // Volume (10px, muted, right-aligned)
            let vol_str = format_volume(entry.volume_24h);
            ctx.fill_text(&vol_str, cols.del_x - 4.0, row_mid_y);

            // Delete icon — only on hover, after all column text
            if is_hovered {
                let delete_widget_id = format!("wl_modal:delete:{}:{}:{}", entry.symbol, entry.exchange, entry.account_type);
                let is_delete_hovered = state.hovered_widget.as_deref() == Some(delete_widget_id.as_str());
                let icon_inner = 14.0;
                let icon_off_x = delete_x + (icon_btn_size - icon_inner) / 2.0;
                let icon_off_y = delete_y + (icon_btn_size - icon_inner) / 2.0;
                if is_delete_hovered {
                    // Highlight delete button with a red background when hovered.
                    ctx.set_fill_color("#ef535033");
                    ctx.fill_rect(delete_x, delete_y, icon_btn_size, icon_btn_size);
                }
                draw_svg_icon(
                    ctx, Icon::Close.svg(),
                    icon_off_x,
                    icon_off_y,
                    icon_inner, icon_inner,
                    if is_delete_hovered { "#ef5350" } else { &toolbar_theme.item_text_muted },
                );
            }

            // Row separator
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.begin_path();
            ctx.move_to(modal_x, current_y + item_h);
            ctx.line_to(modal_x + modal_w, current_y + item_h);
            ctx.stroke();
        }

        let composite_key = format!("{}:{}:{}", entry.symbol, entry.exchange, entry.account_type);
        result.item_rects.push((composite_key.clone(), item_rect));
        result.delete_btn_rects.push((composite_key, delete_rect));

        current_y += item_h;
    }

    // Empty state
    if filtered.is_empty() {
        let center_x = modal_x + modal_w / 2.0;
        let center_y = list_top + list_h / 2.0;
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        let msg = if state.search_query.is_empty() {
            "No symbols in watchlist"
        } else {
            "No symbols match your search"
        };
        ctx.fill_text(msg, center_x, center_y);
    }

    // -------------------------------------------------------------------------
    // Drag-reorder visual: dragged-row highlight and drop indicator line.
    // -------------------------------------------------------------------------
    if let Some((drag_idx, _drag_y)) = state.drag_reorder {
        // Highlight the dragged row with a semi-transparent accent overlay.
        let drag_row_y = list_top - state.scroll.offset + drag_idx as f64 * item_h;
        ctx.set_fill_color("#4a9eff33"); // accent blue ~20% opacity
        ctx.fill_rect(modal_x, drag_row_y, modal_w, item_h);

        // Drop indicator: a 2 px horizontal line at the drop position.
        if let Some(drop_idx) = state.drop_index {
            let drop_line_y = list_top - state.scroll.offset + drop_idx as f64 * item_h;
            ctx.set_stroke_color("#4a9eff");
            ctx.set_stroke_width(2.0);
            ctx.begin_path();
            ctx.move_to(modal_x, drop_line_y);
            ctx.line_to(modal_x + modal_w, drop_line_y);
            ctx.stroke();
        }
    }

    ctx.restore();

    // -------------------------------------------------------------------------
    // Scrollbar
    // -------------------------------------------------------------------------
    let scrollbar_w = 6.0;
    let needs_scrollbar = total_h > list_h;
    if needs_scrollbar {
        let sb_x = modal_x + modal_w - scrollbar_w - 2.0;
        let sb_rect = WidgetRect::new(sb_x, list_top, scrollbar_w, list_h);
        let sb_config = ScrollbarConfig::new(total_h, list_h, state.scroll.offset);
        let sb_state = if state.scroll.is_dragging {
            ScrollbarState::Dragging
        } else {
            ScrollbarState::Active
        };
        let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
        let sb_result = draw_scrollbar(ctx, &sb_config, sb_state, sb_rect, &widget_theme, None);
        result.scrollbar_handle_rect = Some(sb_result.handle_rect);
        result.scrollbar_track_rect = Some(sb_result.track_rect);
    }

    // -------------------------------------------------------------------------
    // Register item hit zones with InputCoordinator
    // -------------------------------------------------------------------------
    // Scroll viewport registered FIRST — base layer for wheel/drag-scroll events.
    // Items and delete buttons are registered on top so hits.last() picks them.
    input_coordinator.register_on_layer(
        "wl_modal:list_scroll",
        uzor::types::Rect::new(modal_x, list_top, modal_w, list_h),
        uzor::input::Sense::DRAG,
        layer_id,
    );

    for (symbol, item_rect) in &result.item_rects {
        let item_id = format!("wl_modal:item:{}", symbol);
        input_coordinator.register_on_layer(
            item_id.as_str(),
            uzor::types::Rect::new(item_rect.x, item_rect.y, item_rect.width, item_rect.height),
            uzor::input::Sense::CLICK_AND_DRAG,
            layer_id,
        );
    }

    for (symbol, delete_rect) in &result.delete_btn_rects {
        let delete_id = format!("wl_modal:delete:{}", symbol);
        input_coordinator.register_on_layer(
            delete_id.as_str(),
            uzor::types::Rect::new(delete_rect.x, delete_rect.y, delete_rect.width, delete_rect.height),
            uzor::input::Sense::CLICK,
            layer_id,
        );
    }
}

// =============================================================================
// Groups tab
// =============================================================================

/// Render the Groups management tab content.
///
/// Shows a "New Group" button at the top and a scrollable list of watchlist
/// presets, each with a colored dot, name, symbol count, and delete button.
#[allow(clippy::too_many_arguments)]
fn render_groups_tab(
    ctx: &mut dyn RenderContext,
    modal_x: f64,
    content_top: f64,
    modal_w: f64,
    content_h: f64,
    padding: f64,
    icon_size: f64,
    state: &WatchlistModalState,
    groups: &[WatchlistGroupInfo],
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
) {
    // -------------------------------------------------------------------------
    // "New Group" button row at the top
    // -------------------------------------------------------------------------
    let btn_h = 28.0;
    let btn_top = content_top + padding / 2.0;
    let btn_x = modal_x + padding;
    let btn_w = 110.0;

    // Button background (subtle: separator-colored border + transparent fill)
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    let group_add_hovered = state.hovered_widget.as_deref() == Some("wl_modal:group_add");
    // Draw a rounded-looking rect via plain rect for now
    let btn_bg = if group_add_hovered {
        &toolbar_theme.item_bg_active
    } else {
        &toolbar_theme.item_bg_hover
    };
    ctx.set_fill_color(btn_bg);
    ctx.fill_rect(btn_x, btn_top, btn_w, btn_h);

    ctx.set_font("bold 11px sans-serif");
    let btn_text_color = if group_add_hovered {
        &toolbar_theme.item_text_active
    } else {
        &toolbar_theme.item_text
    };
    ctx.set_fill_color(btn_text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    // "+" prefix symbol
    ctx.fill_text("+ New Watchlist", btn_x + 8.0, btn_top + btn_h / 2.0);

    input_coordinator.register_on_layer(
        "wl_modal:group_add",
        uzor::types::Rect::new(btn_x, btn_top, btn_w, btn_h),
        uzor::input::Sense::CLICK,
        layer_id,
    );

    // Separator below the button row
    let separator_y = btn_top + btn_h + padding / 2.0;
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, separator_y);
    ctx.line_to(modal_x + modal_w, separator_y);
    ctx.stroke();

    // -------------------------------------------------------------------------
    // Groups list
    // -------------------------------------------------------------------------
    let row_h = 32.0;
    let list_top = separator_y;
    let list_bottom = content_top + content_h;
    let name_x_off = padding;                      // left edge of name text (no dot)
    let del_right = modal_x + modal_w - padding;   // right edge of delete icon

    // Clip to the list area
    ctx.save();
    ctx.clip_rect(modal_x, list_top, modal_w, list_bottom - list_top);

    let mut row_y = list_top;

    for group in groups {
        let row_mid_y = row_y + row_h / 2.0;

        // Compute icon positions first so we know the clipping boundary.
        let del_icon_x = del_right - icon_size;
        let del_icon_y = row_mid_y - icon_size / 2.0;
        let rename_icon_x = del_icon_x - icon_size - 6.0;
        let rename_icon_y = del_icon_y;

        // Widget id strings for hover detection.
        let rename_id = format!("wl_modal:group_rename:{}", group.id);
        let del_id = format!("wl_modal:group_delete:{}", group.id);
        let row_id = format!("wl_modal:group:{}", group.id);
        let rename_hovered = state.hovered_widget.as_deref() == Some(rename_id.as_str());
        let del_hovered = state.hovered_widget.as_deref() == Some(del_id.as_str());
        let row_hovered = state.hovered_widget.as_deref() == Some(row_id.as_str());

        // Active group: 3px accent stripe on the left edge (no full-row background).
        if group.is_active {
            ctx.set_fill_color(&toolbar_theme.accent);
            ctx.fill_rect(modal_x, row_y, 3.0, row_h);
        } else if row_hovered {
            ctx.draw_hover_rect(modal_x, row_y, modal_w, row_h, &toolbar_theme.item_bg_hover);
        }

        // Group name — bold when active, clipped to avoid overlapping icons.
        // Reserve space for rename + delete icons (approx 50-60px on the right).
        let icon_reserve = icon_size * 2.0 + 6.0 + 12.0;
        let name_x = modal_x + name_x_off;
        let name_clip_right = del_right - icon_reserve;
        let name_clip_w = (name_clip_right - name_x).max(0.0);

        if group.is_active {
            ctx.set_font("bold 12px sans-serif");
            ctx.set_fill_color(&toolbar_theme.item_text_active);
        } else {
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(&toolbar_theme.item_text);
        }
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.save();
        ctx.clip_rect(name_x, row_y, name_clip_w, row_h);
        ctx.fill_text(&group.name, name_x, row_mid_y);
        ctx.restore();

        // Symbol count — muted, right-aligned before the rename icon.
        let count_str = format!("{} symbols", group.symbol_count);
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Right);
        let count_right_x = rename_icon_x - 8.0;
        if count_right_x > name_clip_right {
            ctx.fill_text(&count_str, count_right_x, row_mid_y);
        }

        // Rename (pencil) icon — always visible, brighter on hover.
        let rename_color = if rename_hovered {
            &toolbar_theme.item_text
        } else {
            &toolbar_theme.item_text_muted
        };
        draw_svg_icon(
            ctx, Icon::Pencil.svg(),
            rename_icon_x,
            rename_icon_y,
            icon_size, icon_size,
            rename_color,
        );
        input_coordinator.register_on_layer(
            rename_id.as_str(),
            uzor::types::Rect::new(rename_icon_x - 4.0, row_y, icon_size + 8.0, row_h),
            uzor::input::Sense::CLICK,
            layer_id,
        );

        // Delete icon (x) -- only if not the last group, red on hover.
        if groups.len() > 1 {
            let del_color = if del_hovered { "#ef5350" } else { &toolbar_theme.item_text_muted };
            draw_svg_icon(
                ctx, Icon::Close.svg(),
                del_icon_x,
                del_icon_y,
                icon_size, icon_size,
                del_color,
            );
            input_coordinator.register_on_layer(
                del_id.as_str(),
                uzor::types::Rect::new(del_icon_x - 4.0, row_y, icon_size + 8.0, row_h),
                uzor::input::Sense::CLICK,
                layer_id,
            );
        }

        // Row click zone -- switch active watchlist.
        // Exclude the rename and delete icon areas from the row click zone.
        let row_clickable_w = rename_icon_x - 4.0 - modal_x;
        input_coordinator.register_on_layer(
            row_id.as_str(),
            uzor::types::Rect::new(modal_x, row_y, row_clickable_w.max(0.0), row_h),
            uzor::input::Sense::CLICK,
            layer_id,
        );

        // Row separator
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(modal_x, row_y + row_h);
        ctx.line_to(modal_x + modal_w, row_y + row_h);
        ctx.stroke();

        row_y += row_h;
    }

    // Empty state
    if groups.is_empty() {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(
            "No watchlists",
            modal_x + modal_w / 2.0,
            list_top + (list_bottom - list_top) / 2.0,
        );
    }

    ctx.restore();
}

// =============================================================================
// Placeholder tab
// =============================================================================

fn render_placeholder_tab(
    ctx: &mut dyn RenderContext,
    modal_x: f64,
    content_top: f64,
    modal_w: f64,
    content_h: f64,
    message: &str,
    toolbar_theme: &ToolbarTheme,
) {
    let center_x = modal_x + modal_w / 2.0;
    let center_y = content_top + content_h / 2.0;
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(message, center_x, center_y);
}

// =============================================================================
// Helpers
// =============================================================================

/// Format a price value for display.
fn format_price(p: f64) -> String {
    if p >= 1000.0 {
        format!("{:.2}", p)
    } else if p >= 1.0 {
        format!("{:.2}", p)
    } else if p >= 0.01 {
        format!("{:.4}", p)
    } else {
        format!("{:.6}", p)
    }
}

/// Format a volume value with K/M/B suffixes.
fn format_volume(v: f64) -> String {
    if v >= 1_000_000_000.0 {
        format!("{:.1}B", v / 1_000_000_000.0)
    } else if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.1}K", v / 1_000.0)
    } else {
        format!("{:.0}", v)
    }
}

// =============================================================================
// Watchlist Group Name Input Modal
// =============================================================================

/// Render result from the watchlist group name input modal.
#[derive(Clone, Debug, Default)]
pub struct WlGroupNameInputResult {
    /// The modal outer rectangle (for backdrop hit-testing).
    pub modal_rect: WidgetRect,
    /// Header rectangle (title + close button row) — used for drag detection.
    pub header_rect: WidgetRect,
    /// Close (X) button rectangle.
    pub close_btn_rect: WidgetRect,
    /// "Save" button rectangle.
    pub save_btn_rect: WidgetRect,
    /// "Cancel" button rectangle.
    pub cancel_btn_rect: WidgetRect,
    /// Text input field rectangle (for click-to-focus).
    pub input_rect: WidgetRect,
    /// Text area inside the input (inset by padding) — used for click-to-cursor.
    pub input_text_rect: WidgetRect,
    /// Character X positions for click-to-cursor.
    pub char_x_positions: Vec<f64>,
}

/// Render the watchlist group name input modal dialog.
///
/// Appears on top of the watchlist modal when creating a new watchlist group
/// or renaming an existing one.
pub fn render_wl_group_name_input(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    state: &WatchlistGroupNameInputState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> WlGroupNameInputResult {
    let mut result = WlGroupNameInputResult::default();

    let modal_w = 400.0;
    let modal_h = 170.0;
    let header_h = 44.0;
    let footer_h = 52.0;
    let padding = 16.0;
    let input_h = 32.0;
    let btn_h = 32.0;
    let btn_w = 80.0;
    let btn_gap = 8.0;

    // Position (draggable, centered by default)
    let (modal_x, modal_y) = state.position.unwrap_or_else(|| {
        ((screen_w - modal_w) / 2.0, (screen_h - modal_h) / 2.0)
    });
    let modal_x = modal_x.max(0.0).min(screen_w - modal_w);
    let modal_y = modal_y.max(0.0).min(screen_h - modal_h);

    let modal_rect = WidgetRect::new(modal_x, modal_y, modal_w, modal_h);
    result.modal_rect = modal_rect;

    // Modal frame (blur + background + border)
    let modal_theme = ModalTheme::from_frame_theme(
        &frame_theme.toolbar_bg,
        &toolbar_theme.separator,
        &toolbar_theme.item_text,
        &toolbar_theme.item_text_hover,
        &toolbar_theme.separator,
    );
    render_modal_frame_only(ctx, modal_rect, &modal_theme, 0.0);

    // InputCoordinator layer — use ModalOverlay (z=4) so this modal is processed
    // before the watchlist modal (z=3) in the same hit-test pass, preventing the
    // watchlist's wl_modal:modal_bg from eating clicks on Save/Cancel.
    let layer_id = ZLayer::ModalOverlay.push_named(input_coordinator, "wl_group_name_input");

    // Register modal background (absorbs clicks)
    input_coordinator.register_on_layer(
        "wl_group_name:modal_bg",
        uzor::types::Rect::new(modal_x, modal_y, modal_w, modal_h),
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    // Header
    let header_rect = WidgetRect::new(modal_x, modal_y, modal_w, header_h);
    result.header_rect = header_rect;

    let title = match state.mode {
        WatchlistGroupNameMode::CreateNew => "New Watchlist",
        WatchlistGroupNameMode::Rename(_) => "Rename Watchlist",
    };
    ctx.set_font("bold 14px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(title, modal_x + padding, modal_y + header_h / 2.0);

    // Close button (X) — right side of header
    let close_size = 18.0;
    let close_x = modal_x + modal_w - close_size - 12.0;
    let close_y = modal_y + (header_h - close_size) / 2.0;
    let close_rect = WidgetRect::new(close_x, close_y, close_size, close_size);
    result.close_btn_rect = close_rect;

    draw_svg_icon(ctx, Icon::Close.svg(), close_x, close_y, close_size, close_size, &toolbar_theme.item_text);

    input_coordinator.register_on_layer(
        "wl_group_name:close",
        uzor::types::Rect::new(close_x, close_y, close_size, close_size),
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    // Header separator
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, modal_y + header_h);
    ctx.line_to(modal_x + modal_w, modal_y + header_h);
    ctx.stroke();

    // Content area — text input field
    let content_y = modal_y + header_h + padding;
    let input_rect = WidgetRect::new(
        modal_x + padding,
        content_y,
        modal_w - padding * 2.0,
        input_h,
    );
    result.input_rect = input_rect;

    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let (sel_start, sel_end) = if let Some((lo, hi)) = state.editing.selection_range() {
        (Some(lo), Some(hi))
    } else {
        (None, None)
    };
    let input_config = InputConfig::new(&state.editing.text)
        .with_focused(true)
        .with_cursor(state.editing.cursor)
        .with_placeholder("Watchlist name...")
        .with_selection(sel_start, sel_end);

    let input_result = draw_input(ctx, &input_config, WidgetState::Normal, input_rect, &widget_theme);
    result.input_text_rect = input_result.text_rect;
    result.char_x_positions = input_result.char_x_positions;

    // Register text input area for click-to-cursor
    input_coordinator.register_on_layer(
        "wl_group_name:input",
        uzor::types::Rect::new(input_rect.x, input_rect.y, input_rect.width, input_rect.height),
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    // Blinking cursor
    if state.editing.is_cursor_visible(current_time_ms) {
        draw_input_cursor(
            ctx,
            input_result.cursor_x,
            input_result.cursor_y,
            input_result.cursor_height,
            &toolbar_theme.item_text,
        );
    }

    // Footer separator
    let footer_y = modal_y + modal_h - footer_h;
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, footer_y);
    ctx.line_to(modal_x + modal_w, footer_y);
    ctx.stroke();

    // Footer buttons — right-aligned
    let btns_y = footer_y + (footer_h - btn_h) / 2.0;
    let cancel_x = modal_x + modal_w - padding - btn_w;
    let save_x = cancel_x - btn_gap - btn_w;

    // "Save" button (primary)
    let save_rect = WidgetRect::new(save_x, btns_y, btn_w, btn_h);
    result.save_btn_rect = save_rect;
    ctx.set_fill_color("#2962ff");
    ctx.fill_rounded_rect(save_rect.x, save_rect.y, save_rect.width, save_rect.height, 4.0);
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color("#ffffff");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Save", save_rect.center_x(), save_rect.center_y());

    input_coordinator.register_on_layer(
        "wl_group_name:save",
        uzor::types::Rect::new(save_rect.x, save_rect.y, save_rect.width, save_rect.height),
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    // "Cancel" button (secondary — themed border + text)
    let cancel_rect = WidgetRect::new(cancel_x, btns_y, btn_w, btn_h);
    result.cancel_btn_rect = cancel_rect;
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(cancel_rect.x, cancel_rect.y, cancel_rect.width, cancel_rect.height, 4.0);
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Cancel", cancel_rect.center_x(), cancel_rect.center_y());

    input_coordinator.register_on_layer(
        "wl_group_name:cancel",
        uzor::types::Rect::new(cancel_rect.x, cancel_rect.y, cancel_rect.width, cancel_rect.height),
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    input_coordinator.pop_layer(&layer_id);
    result
}
