//! Search overlay modal renderer (symbol/indicator search).

use crate::engine::render::RenderContext;
use crate::ui::toolbar_render::ToolbarTheme;
use uzor::types::Rect as WidgetRect;
use crate::layout::render_frame::ModalSearchResult;
use crate::layout::render_chart::FrameTheme;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::ui::modal_state::{ModalState, OpenModal, IndicatorCatalogItem, IndicatorCategoryFilter, SearchResult};
use crate::ui::modal_settings::ChartScreenArea;
use crate::ui::scroll_widget::{ScrollableContainer, ScrollableConfig};
use crate::ui::widgets::{render_modal_frame_only, ModalTheme};
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig, InputType};
use crate::ui::widgets::types::WidgetState;
use crate::ui::Icon;
use crate::ui::z_order::ZLayer;
use crate::templates::indicator_set::IndicatorSet;

/// Render search overlay (symbol/indicator search).
pub fn render_search_overlay(
    ctx: &mut dyn RenderContext,
    screen: ChartScreenArea,
    modal_state: &ModalState,
    indicator_items: &[IndicatorCatalogItem],
    indicator_sets: &[IndicatorSet],
    hovered_item_id: Option<&str>,
    theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> ModalSearchResult {
    use crate::render::{TextAlign, TextBaseline};
    use crate::engine::render::draw_svg_icon;

    let mut result = ModalSearchResult::default();

    let (title, placeholder, _icon) = match modal_state.current {
        OpenModal::SymbolSearch => ("Symbol Search", "Search symbol...", Icon::Search),
        OpenModal::IndicatorSearch => ("Add Indicator", "Search indicator...", Icon::Indicators),
        OpenModal::CompareSearch => ("Compare Symbol", "Search symbol to compare...", Icon::Search),
        _ => return result,
    };

    let is_indicator_search = modal_state.current == OpenModal::IndicatorSearch;

    let header_height = 36.0;
    let modal_padding = 12.0;
    let close_btn_size = 20.0;
    let input_height = 32.0;
    let scrollbar_width = 8.0;
    let sidebar_width = if is_indicator_search { 48.0 } else { 0.0 };

    let base_width = (screen.width * 0.5).max(400.0);
    let modal_width = if is_indicator_search { base_width + sidebar_width } else { base_width };
    let modal_height = (screen.height * 0.6).max(300.0);

    let screen_w = screen.x + screen.width;
    let screen_h = screen.y + screen.height;

    let (modal_x, modal_y) = modal_state.position.unwrap_or_else(|| {
        let x = screen.x + (screen.width - modal_width) / 2.0;
        let y = screen.y + (screen.height - modal_height) / 2.0;
        (x, y)
    });

    let modal_x = modal_x.max(0.0).min(screen_w - modal_width);
    let modal_y = modal_y.max(0.0).min(screen_h - modal_height);

    result.modal_rect = WidgetRect::new(modal_x, modal_y, modal_width, modal_height);

    let layer_id = ZLayer::Modal.push_named(input_coordinator, "modal_search");

    input_coordinator.register_on_layer(
        "modal_search:modal_bg",
        uzor::types::Rect::new(modal_x, modal_y, modal_width, modal_height),
        uzor::input::sense::Sense::CLICK,
        &layer_id,
    );

    let modal_theme = ModalTheme::from_frame_theme(
        &theme.toolbar_bg,
        &toolbar_theme.separator,
        &toolbar_theme.item_text,
        &toolbar_theme.item_text_hover,
        &toolbar_theme.separator,
    );
    render_modal_frame_only(ctx, result.modal_rect, &modal_theme, 0.0);

    result.header_rect = Some(WidgetRect::new(modal_x, modal_y, modal_width, header_height));

    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(title, modal_x + modal_padding, modal_y + header_height / 2.0);

    let close_x = modal_x + modal_width - modal_padding - close_btn_size;
    let close_y = modal_y + (header_height - close_btn_size) / 2.0;

    draw_svg_icon(
        ctx,
        Icon::Close.svg(),
        close_x + 2.0, close_y + 2.0,
        close_btn_size - 4.0, close_btn_size - 4.0,
        &toolbar_theme.item_text,
    );
    result.close_btn_rect = WidgetRect::new(close_x, close_y, close_btn_size, close_btn_size);

    input_coordinator.register_on_layer(
        "modal_search:close",
        uzor::types::Rect::new(close_x, close_y, close_btn_size, close_btn_size),
        uzor::input::sense::Sense::CLICK,
        &layer_id,
    );

    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.begin_path();
    ctx.move_to(modal_x, modal_y + header_height);
    ctx.line_to(modal_x + modal_width, modal_y + header_height);
    ctx.stroke();

    // Left sidebar (indicator category filters)
    let content_x = modal_x + sidebar_width;
    let content_width = modal_width - sidebar_width;

    if is_indicator_search {
        let sidebar_x = modal_x;
        let sidebar_y = modal_y + header_height;
        let sidebar_height = modal_height - header_height;

        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.begin_path();
        ctx.move_to(sidebar_x + sidebar_width, sidebar_y);
        ctx.line_to(sidebar_x + sidebar_width, sidebar_y + sidebar_height);
        ctx.stroke();

        result.sidebar_rect = Some(WidgetRect::new(sidebar_x, sidebar_y, sidebar_width, sidebar_height));

        let tab_icon_size = 20.0;
        let tab_button_height = 40.0;
        let active_filter = modal_state.indicator_category_filter;
        let show_sets = modal_state.show_indicator_sets;

        for (i, filter) in IndicatorCategoryFilter::all().iter().enumerate() {
            let tab_y = sidebar_y + i as f64 * tab_button_height;
            // Category tabs are active only if sets view is NOT shown
            let is_active = !show_sets && *filter == active_filter;

            if is_active {
                ctx.draw_sidebar_active_item(
                    sidebar_x, tab_y, sidebar_width, tab_button_height,
                    &toolbar_theme.accent, &toolbar_theme.item_bg_active, 3.0,
                );
            }

            let icon_x = sidebar_x + (sidebar_width - tab_icon_size) / 2.0;
            let icon_y = tab_y + (tab_button_height - tab_icon_size) / 2.0;
            let icon_color = if is_active { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text_muted };

            let icon = match filter {
                IndicatorCategoryFilter::All => Icon::Grid,
                IndicatorCategoryFilter::Trend => Icon::TrendLine,
                IndicatorCategoryFilter::Momentum => Icon::ArrowUp,
                IndicatorCategoryFilter::Volatility => Icon::FibChannel,
                IndicatorCategoryFilter::Volume => Icon::Histogram,
                IndicatorCategoryFilter::Oscillator => Icon::SineWave,
                IndicatorCategoryFilter::Average => Icon::LineChart,
                IndicatorCategoryFilter::Other => Icon::MoreHorizontal,
            };
            draw_svg_icon(ctx, icon.svg(), icon_x, icon_y, tab_icon_size, tab_icon_size, icon_color);
            result.category_rects.push((i, WidgetRect::new(sidebar_x, tab_y, sidebar_width, tab_button_height)));
            let cat_wid = format!("modal_search:category:{}", i);
            input_coordinator.register_on_layer(
                cat_wid.as_str(),
                uzor::types::Rect::new(sidebar_x, tab_y, sidebar_width, tab_button_height),
                uzor::input::sense::Sense::CLICK,
                &layer_id,
            );
        }

        // ── Indicator Sets button at bottom of sidebar ───────────────────────
        // Draw a separator line above the sets button.
        let sets_btn_y = sidebar_y + sidebar_height - tab_button_height;
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(sidebar_x + 6.0, sets_btn_y);
        ctx.line_to(sidebar_x + sidebar_width - 6.0, sets_btn_y);
        ctx.stroke();

        let is_sets_active = show_sets;
        if is_sets_active {
            ctx.draw_sidebar_active_item(
                sidebar_x, sets_btn_y, sidebar_width, tab_button_height,
                &toolbar_theme.accent, &toolbar_theme.item_bg_active, 3.0,
            );
        }
        let sets_icon_x = sidebar_x + (sidebar_width - tab_icon_size) / 2.0;
        let sets_icon_y = sets_btn_y + (tab_button_height - tab_icon_size) / 2.0;
        let sets_icon_color = if is_sets_active { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text_muted };
        draw_svg_icon(ctx, Icon::Layers.svg(), sets_icon_x, sets_icon_y, tab_icon_size, tab_icon_size, sets_icon_color);
        input_coordinator.register_on_layer(
            "ind_search:sets_tab",
            uzor::types::Rect::new(sidebar_x, sets_btn_y, sidebar_width, tab_button_height),
            uzor::input::sense::Sense::CLICK,
            &layer_id,
        );
    }

    // Search input
    let input_y = modal_y + header_height + modal_padding;
    let input_width = content_width - modal_padding * 2.0;
    let input_x = content_x + modal_padding;

    let is_editing = modal_state.editing_text.as_ref()
        .map(|e| e.field_id == "search_input")
        .unwrap_or(false);

    let (input_value, cursor_pos, selection_start, selection_end) = if let Some(ref edit) = modal_state.editing_text {
        if edit.field_id == "search_input" {
            (&edit.text, edit.cursor, edit.selection_start, Some(edit.cursor))
        } else {
            (&modal_state.search_query, modal_state.search_query.len(), None, None)
        }
    } else {
        (&modal_state.search_query, modal_state.search_query.len(), None, None)
    };

    let input_config = InputConfig::new(input_value)
        .with_placeholder(placeholder)
        .with_focused(is_editing)
        .with_type(InputType::Search)
        .with_font_size(13.0)
        .with_padding(28.0)
        .with_radius(0.0)
        .with_cursor(cursor_pos)
        .with_selection(selection_start, selection_end);

    let widget_theme = toolbar_to_widget_theme(toolbar_theme, theme);
    let widget_state = WidgetState::Normal;
    let input_rect = WidgetRect::new(input_x, input_y, input_width, input_height);
    let input_result = draw_input(ctx, &input_config, widget_state, input_rect, &widget_theme);
    result.input_rect = input_rect;
    result.search_char_positions = input_result.char_x_positions.clone();

    input_coordinator.register_on_layer(
        "modal_search:search_input",
        uzor::types::Rect::new(input_x, input_y, input_width, input_height),
        uzor::input::sense::Sense::CLICK,
        &layer_id,
    );

    let search_icon_size = 14.0;
    draw_svg_icon(
        ctx, Icon::Search.svg(),
        input_x + 8.0, input_y + (input_height - search_icon_size) / 2.0,
        search_icon_size, search_icon_size,
        &toolbar_theme.item_text_muted,
    );

    if is_editing {
        if let Some(ref edit) = modal_state.editing_text {
            if edit.field_id == "search_input" && edit.is_cursor_visible(current_time_ms) {
                draw_input_cursor(ctx, input_result.cursor_x, input_result.cursor_y, input_result.cursor_height, &toolbar_theme.item_text);
            }
        }
    }

    // Results area
    let results_y = input_y + input_height + modal_padding;
    let results_height = modal_y + modal_height - results_y - modal_padding;
    let results_width = content_width - modal_padding * 2.0 - scrollbar_width;

    result.results_rect = Some(WidgetRect::new(
        content_x + modal_padding, results_y,
        results_width + scrollbar_width, results_height,
    ));

    let viewport_rect = WidgetRect::new(content_x + modal_padding, results_y, results_width + scrollbar_width, results_height);
    let scroll_config = ScrollableConfig { scrollbar_width, scrollbar_padding: 0.0, always_show_scrollbar: false };
    let scrollable = ScrollableContainer::new(viewport_rect, &modal_state.scroll, Some(scroll_config));
    scrollable.begin(ctx);
    let scroll_offset = scrollable.scroll_offset();

    let total_content_height: f64;

    if modal_state.current == OpenModal::IndicatorSearch && modal_state.show_indicator_sets {
        // ── Indicator Sets view ──────────────────────────────────────────────
        let (rects, height) = render_indicator_sets_scrollable(
            ctx,
            content_x + modal_padding, results_y,
            results_width, results_height,
            indicator_sets,
            hovered_item_id,
            scroll_offset,
            theme,
            toolbar_theme,
            input_coordinator,
            &layer_id,
        );
        result.item_rects = rects;
        total_content_height = height;
        result.hovered_item_id = hovered_item_id.map(|s| s.to_string());
    } else if modal_state.current == OpenModal::IndicatorSearch && !indicator_items.is_empty() {
        let active_filter = modal_state.indicator_category_filter;
        let q = modal_state.search_query.to_lowercase();
        let filtered_items: Vec<&IndicatorCatalogItem> = indicator_items
            .iter()
            .filter(|item| active_filter.matches(&item.category))
            .filter(|item| {
                q.is_empty()
                    || item.short_name.to_lowercase().contains(&q)
                    || item.name.to_lowercase().contains(&q)
                    || item.description.to_lowercase().contains(&q)
            })
            .collect();

        if !filtered_items.is_empty() {
            let (rects, height) = render_indicator_search_results_scrollable_filtered(
                ctx, content_x + modal_padding, results_y, results_width, results_height,
                &filtered_items, hovered_item_id, scroll_offset, theme, toolbar_theme,
            );
            result.item_rects = rects;
            total_content_height = height;
            result.hovered_item_id = hovered_item_id.map(|s| s.to_string());
        } else {
            total_content_height = 0.0;
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(&theme.toolbar_border);
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.fill_text("No indicators in this category", content_x + content_width / 2.0, results_y + 20.0);
        }
    } else if (modal_state.current == OpenModal::SymbolSearch || modal_state.current == OpenModal::CompareSearch)
        && !modal_state.symbol_search_results.is_empty()
    {
        let (rects, star_rects, height) = render_symbol_search_results_scrollable(
            ctx, content_x + modal_padding, results_y, results_width, results_height,
            &modal_state.symbol_search_results, hovered_item_id, scroll_offset, theme, toolbar_theme,
        );
        result.item_rects = rects;
        result.star_rects = star_rects;
        total_content_height = height;
        result.hovered_item_id = hovered_item_id.map(|s| s.to_string());
    } else {
        total_content_height = 0.0;
        let center_x = content_x + content_width / 2.0;
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&theme.toolbar_border);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Top);
        if modal_state.search_query.is_empty() {
            ctx.fill_text("Type to search...", center_x, results_y + 20.0);
        } else {
            ctx.fill_text("Nothing found", center_x, results_y + 20.0);
        }
    }

    let widget_theme = crate::ui::widgets::types::WidgetTheme::default();
    let scroll_result = scrollable.end(ctx, total_content_height, &widget_theme);
    result.total_content_height = scroll_result.content_height;
    result.viewport_height = scroll_result.viewport_height;
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;

    if let Some(track_rect) = result.scrollbar_track_rect {
        input_coordinator.register_on_layer(
            "modal_search:scrollbar_track",
            uzor::types::Rect::new(track_rect.x, track_rect.y, track_rect.width, track_rect.height),
            uzor::input::sense::Sense::CLICK,
            &layer_id,
        );
    }

    // Register item row zones first (skip when indicator sets view is active —
    // set rows are already registered with the `ind_search:set:` prefix).
    if !modal_state.show_indicator_sets {
        for (item_id, item_rect) in &result.item_rects {
            input_coordinator.register_on_layer(
                format!("modal_search:item:{}", item_id),
                uzor::types::Rect::new(item_rect.x, item_rect.y, item_rect.width, item_rect.height),
                uzor::input::sense::Sense::CLICK,
                &layer_id,
            );
        }
    }

    // Register star zones AFTER item zones so they take priority on overlap
    // (later registration wins in the input coordinator hit-test order).
    for (symbol, star_rect) in &result.star_rects {
        input_coordinator.register_on_layer(
            format!("modal_search:star:{}", symbol),
            uzor::types::Rect::new(star_rect.x, star_rect.y, star_rect.width, star_rect.height),
            uzor::input::sense::Sense::CLICK,
            &layer_id,
        );
    }

    input_coordinator.pop_layer(&layer_id);
    result
}

/// Render indicator search results (non-scrollable, max 8 items).
fn render_indicator_search_results(
    ctx: &mut dyn RenderContext,
    x: f64, y: f64, width: f64, _height: f64,
    items: &[IndicatorCatalogItem],
    hovered_item_id: Option<&str>,
    _theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
) -> Vec<(String, WidgetRect)> {
    use crate::render::{TextAlign, TextBaseline};
    let item_height = 44.0;
    let mut current_y = y;
    let mut item_rects = Vec::new();
    for item in items.iter().take(8) {
        let is_hovered = hovered_item_id == Some(item.type_id.as_str());
        if is_hovered { ctx.draw_hover_rect(x, current_y, width, item_height, &toolbar_theme.item_bg_active); }
        item_rects.push((item.type_id.clone(), WidgetRect::new(x, current_y, width, item_height)));
        ctx.set_font("bold 13px sans-serif");
        ctx.set_fill_color(if is_hovered { "#ffffff" } else { &toolbar_theme.item_text });
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(&item.short_name, x + 12.0, current_y + 8.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(if is_hovered { &toolbar_theme.item_text } else { &toolbar_theme.item_text_muted });
        ctx.fill_text(&item.name, x + 12.0, current_y + 26.0);
        let category_text = if item.overlay {
            format!("[overlay] {}", item.category.display_name())
        } else {
            item.category.display_name().to_string()
        };
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Right);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&category_text, x + width - 12.0, current_y + item_height / 2.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(x, current_y + item_height);
        ctx.line_to(x + width, current_y + item_height);
        ctx.stroke();
        current_y += item_height;
    }
    item_rects
}

/// Render symbol search results (non-scrollable, max 10 items).
fn render_symbol_search_results(
    ctx: &mut dyn RenderContext,
    x: f64, y: f64, width: f64, _height: f64,
    items: &[SearchResult],
    hovered_item_id: Option<&str>,
    _theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
) -> Vec<(String, WidgetRect)> {
    use crate::render::{TextAlign, TextBaseline};
    let item_height = 44.0;
    let mut current_y = y;
    let mut item_rects = Vec::new();
    for item in items.iter().take(10) {
        let item_key = format!("{}:{}", item.symbol, item.exchange_id);
        let is_hovered = hovered_item_id == Some(item_key.as_str());
        if is_hovered { ctx.draw_hover_rect(x, current_y, width, item_height, &toolbar_theme.item_bg_active); }
        item_rects.push((item_key, WidgetRect::new(x, current_y, width, item_height)));
        ctx.set_font("bold 13px sans-serif");
        ctx.set_fill_color(if is_hovered { "#ffffff" } else { &toolbar_theme.item_text });
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(&item.symbol, x + 12.0, current_y + 8.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(if is_hovered { &toolbar_theme.item_text } else { &toolbar_theme.item_text_muted });
        ctx.fill_text(&item.name, x + 12.0, current_y + 26.0);
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Right);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&item.exchange, x + width - 12.0, current_y + item_height / 2.0);
        ctx.set_font("12px sans-serif");
        ctx.fill_text(&item.category_icon, x + width - 70.0, current_y + item_height / 2.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(x, current_y + item_height);
        ctx.line_to(x + width, current_y + item_height);
        ctx.stroke();
        current_y += item_height;
    }
    item_rects
}

/// Render indicator search results with scroll support.
fn render_indicator_search_results_scrollable(
    ctx: &mut dyn RenderContext,
    x: f64, y: f64, width: f64, viewport_height: f64,
    items: &[IndicatorCatalogItem],
    hovered_item_id: Option<&str>,
    scroll_offset: f64,
    _theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
) -> (Vec<(String, WidgetRect)>, f64) {
    use crate::render::{TextAlign, TextBaseline};
    let item_height = 48.0;
    let total_height = items.len() as f64 * item_height;
    let mut current_y = y - scroll_offset;
    let mut item_rects = Vec::new();
    for item in items.iter() {
        if current_y + item_height < y || current_y > y + viewport_height {
            item_rects.push((item.type_id.clone(), WidgetRect::new(x, current_y, width, item_height)));
            current_y += item_height;
            continue;
        }
        let is_hovered = hovered_item_id == Some(item.type_id.as_str());
        if is_hovered { ctx.draw_hover_rect(x, current_y, width, item_height, &toolbar_theme.item_bg_active); }
        item_rects.push((item.type_id.clone(), WidgetRect::new(x, current_y, width, item_height)));
        ctx.set_font("bold 13px sans-serif");
        ctx.set_fill_color(if is_hovered { "#ffffff" } else { &toolbar_theme.item_text });
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(&item.short_name, x + 12.0, current_y + 10.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(if is_hovered { &toolbar_theme.item_text } else { &toolbar_theme.item_text_muted });
        ctx.fill_text(&item.name, x + 12.0, current_y + 28.0);
        let category_text = if item.overlay {
            format!("[overlay] {}", item.category.display_name())
        } else {
            item.category.display_name().to_string()
        };
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Right);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&category_text, x + width - 12.0, current_y + item_height / 2.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(x, current_y + item_height);
        ctx.line_to(x + width, current_y + item_height);
        ctx.stroke();
        current_y += item_height;
    }
    (item_rects, total_height)
}

/// Render indicator search results with scroll support (filtered).
fn render_indicator_search_results_scrollable_filtered(
    ctx: &mut dyn RenderContext,
    x: f64, y: f64, width: f64, viewport_height: f64,
    items: &[&IndicatorCatalogItem],
    hovered_item_id: Option<&str>,
    scroll_offset: f64,
    _theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
) -> (Vec<(String, WidgetRect)>, f64) {
    use crate::render::{TextAlign, TextBaseline};
    let item_height = 48.0;
    let total_height = items.len() as f64 * item_height;
    let mut current_y = y - scroll_offset;
    let mut item_rects = Vec::new();
    for item in items.iter() {
        if current_y + item_height < y || current_y > y + viewport_height {
            item_rects.push((item.type_id.clone(), WidgetRect::new(x, current_y, width, item_height)));
            current_y += item_height;
            continue;
        }
        let is_hovered = hovered_item_id == Some(item.type_id.as_str());
        if is_hovered { ctx.draw_hover_rect(x, current_y, width, item_height, &toolbar_theme.item_bg_active); }
        item_rects.push((item.type_id.clone(), WidgetRect::new(x, current_y, width, item_height)));
        ctx.set_font("bold 13px sans-serif");
        ctx.set_fill_color(if is_hovered { "#ffffff" } else { &toolbar_theme.item_text });
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(&item.short_name, x + 12.0, current_y + 10.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(if is_hovered { &toolbar_theme.item_text } else { &toolbar_theme.item_text_muted });
        ctx.fill_text(&item.name, x + 12.0, current_y + 28.0);
        let category_text = if item.overlay {
            format!("[overlay] {}", item.category.display_name())
        } else {
            item.category.display_name().to_string()
        };
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Right);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&category_text, x + width - 12.0, current_y + item_height / 2.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(x, current_y + item_height);
        ctx.line_to(x + width, current_y + item_height);
        ctx.stroke();
        current_y += item_height;
    }
    (item_rects, total_height)
}

/// Render symbol search results with scroll support.
///
/// Returns `(item_rects, star_rects, total_height)`.  The caller must register
/// item zones first and star zones second so that stars take priority on overlap.
fn render_symbol_search_results_scrollable(
    ctx: &mut dyn RenderContext,
    x: f64, y: f64, width: f64, viewport_height: f64,
    items: &[SearchResult],
    hovered_item_id: Option<&str>,
    scroll_offset: f64,
    _theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
) -> (Vec<(String, WidgetRect)>, Vec<(String, WidgetRect)>, f64) {
    use crate::render::{TextAlign, TextBaseline};
    use crate::engine::render::draw_svg_icon;

    let item_height = 48.0;
    let star_size = 16.0;
    // Horizontal padding before the star icon.
    let star_pad_x = 8.0;
    // Space the star occupies (star_pad_x + star_size + gap after star).
    let star_col_w = star_pad_x + star_size + 4.0;

    let total_height = items.len() as f64 * item_height;
    let mut current_y = y - scroll_offset;
    let mut item_rects = Vec::new();
    // Star rects returned separately so they can be registered AFTER items,
    // giving them higher input priority despite the overlapping row hit zone.
    let mut star_rects: Vec<(String, WidgetRect)> = Vec::new();
    for item in items.iter() {
        // Composite key: "BTC-USDT:binance" — unique even when multiple exchanges
        // list the same ticker symbol.
        let item_key = format!("{}:{}", item.symbol, item.exchange_id);

        if current_y + item_height < y || current_y > y + viewport_height {
            item_rects.push((item_key.clone(), WidgetRect::new(x, current_y, width, item_height)));
            current_y += item_height;
            continue;
        }
        let is_hovered = hovered_item_id == Some(item_key.as_str());
        if is_hovered { ctx.draw_hover_rect(x, current_y, width, item_height, &toolbar_theme.item_bg_active); }
        item_rects.push((item_key.clone(), WidgetRect::new(x, current_y, width, item_height)));

        // --- Star icon (left side) ---
        let star_x = x + star_pad_x;
        let star_y = current_y + (item_height - star_size) / 2.0;
        let (star_icon, star_color) = if item.in_watchlist {
            (crate::ui::Icon::StarFilled, "#FFD700")
        } else {
            (crate::ui::Icon::Star, toolbar_theme.item_text_muted.as_str())
        };
        draw_svg_icon(ctx, star_icon.svg(), star_x, star_y, star_size, star_size, star_color);
        // Collect star rect — registration happens after all item rects so star wins.
        star_rects.push((item_key.clone(), WidgetRect::new(star_x, star_y, star_size, star_size)));

        // --- Symbol text (shifted right to make room for star) ---
        let text_x = x + star_col_w;
        ctx.set_font("bold 13px sans-serif");
        ctx.set_fill_color(if is_hovered { "#ffffff" } else { &toolbar_theme.item_text });
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(&item.symbol, text_x, current_y + 10.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(if is_hovered { &toolbar_theme.item_text } else { &toolbar_theme.item_text_muted });
        ctx.fill_text(&item.name, text_x, current_y + 28.0);

        // --- Exchange / category (right side — unchanged) ---
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Right);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&item.exchange, x + width - 12.0, current_y + item_height / 2.0);
        ctx.set_font("12px sans-serif");
        ctx.fill_text(&item.category_icon, x + width - 70.0, current_y + item_height / 2.0);

        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(x, current_y + item_height);
        ctx.line_to(x + width, current_y + item_height);
        ctx.stroke();
        current_y += item_height;
    }
    (item_rects, star_rects, total_height)
}

/// Render the Indicator Sets list in the content area.
///
/// Shows a "Create Set" button at the top and then each set as a row with its
/// name and indicator count.  Returns `(item_rects, total_height)` where each
/// entry in `item_rects` has the set's `id` as the key.
///
/// The caller is responsible for registering input zones:
/// - `"ind_search:set_create"` for the create button
/// - `"ind_search:set:{id}"` for each set row
#[allow(clippy::too_many_arguments)]
fn render_indicator_sets_scrollable(
    ctx: &mut dyn RenderContext,
    x: f64, y: f64, width: f64, viewport_height: f64,
    sets: &[IndicatorSet],
    hovered_item_id: Option<&str>,
    scroll_offset: f64,
    _theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
) -> (Vec<(String, WidgetRect)>, f64) {
    use crate::render::{TextAlign, TextBaseline};
    use crate::engine::render::draw_svg_icon;

    let create_btn_height = 36.0;
    let create_btn_margin = 8.0;
    let item_height = 48.0;

    // Total scrollable height: create button + items
    let total_height = create_btn_height + create_btn_margin + sets.len() as f64 * item_height;

    // ── "Create Set" button ──────────────────────────────────────────────────
    let btn_y = y - scroll_offset;
    let btn_icon_size = 14.0;
    let is_btn_hovered = hovered_item_id == Some("set_create");

    if btn_y + create_btn_height >= y && btn_y <= y + viewport_height {
        if is_btn_hovered {
            ctx.draw_hover_rect(x, btn_y, width, create_btn_height, &toolbar_theme.item_bg_active);
        }

        // Border
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(x, btn_y + create_btn_height);
        ctx.line_to(x + width, btn_y + create_btn_height);
        ctx.stroke();

        // Plus icon
        let icon_x = x + 10.0;
        let icon_y = btn_y + (create_btn_height - btn_icon_size) / 2.0;
        draw_svg_icon(
            ctx, Icon::Plus.svg(),
            icon_x, icon_y, btn_icon_size, btn_icon_size,
            if is_btn_hovered { &toolbar_theme.item_text_active } else { &toolbar_theme.accent },
        );

        // Label
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(if is_btn_hovered { &toolbar_theme.item_text_active } else { &toolbar_theme.accent });
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Save current indicators as set", x + 10.0 + btn_icon_size + 6.0, btn_y + create_btn_height / 2.0);
    }

    input_coordinator.register_on_layer(
        "ind_search:set_create",
        uzor::types::Rect::new(x, y - scroll_offset, width, create_btn_height),
        uzor::input::sense::Sense::CLICK,
        layer_id,
    );

    // ── Set rows ─────────────────────────────────────────────────────────────
    let mut item_rects = Vec::new();
    let rows_start_y = y - scroll_offset + create_btn_height + create_btn_margin;

    if sets.is_empty() {
        // Empty state message
        let msg_y = rows_start_y + 20.0;
        if msg_y >= y && msg_y <= y + viewport_height {
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(&toolbar_theme.item_text_muted);
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.fill_text("No saved indicator sets yet", x + width / 2.0, msg_y);
        }
    }

    for set in sets.iter() {
        let row_y_abs = rows_start_y + item_rects.len() as f64 * item_height;

        // Always record the rect for hit-test purposes, even if off-screen.
        item_rects.push((set.id.clone(), WidgetRect::new(x, row_y_abs, width, item_height)));

        // Skip drawing if outside viewport.
        if row_y_abs + item_height < y || row_y_abs > y + viewport_height {
            continue;
        }

        let hover_id = format!("set:{}", set.id);
        let delete_hover_id = format!("set_delete:{}", set.id);
        let is_delete_hovered = hovered_item_id == Some(delete_hover_id.as_str());
        let is_hovered = hovered_item_id == Some(hover_id.as_str());
        // Only highlight row background when the row itself (not the delete button) is hovered.
        if is_hovered {
            ctx.draw_hover_rect(x, row_y_abs, width, item_height, &toolbar_theme.item_bg_active);
        }

        // Delete button dimensions (positioned on the right side of the row)
        let icon_size = 16.0;
        let delete_btn_size = 24.0;
        let delete_btn_x = x + width - 8.0 - delete_btn_size;
        let delete_btn_y = row_y_abs + (item_height - delete_btn_size) / 2.0;
        let delete_icon_x = delete_btn_x + (delete_btn_size - icon_size) / 2.0;
        let delete_icon_y = delete_btn_y + (delete_btn_size - icon_size) / 2.0;

        // Set name
        ctx.set_font("bold 13px sans-serif");
        ctx.set_fill_color(if is_hovered { "#ffffff" } else { &toolbar_theme.item_text });
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(&set.name, x + 12.0, row_y_abs + 10.0);

        // Indicator count subtitle
        let count_label = match set.len() {
            1 => "1 indicator".to_string(),
            n => format!("{} indicators", n),
        };
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(if is_hovered { &toolbar_theme.item_text } else { &toolbar_theme.item_text_muted });
        ctx.fill_text(&count_label, x + 12.0, row_y_abs + 28.0);

        // Delete (X) icon — red when hovered directly, dim otherwise
        let delete_color = if is_delete_hovered { "#EF5350" } else { &toolbar_theme.separator };
        draw_svg_icon(
            ctx, Icon::Close.svg(),
            delete_icon_x, delete_icon_y, icon_size, icon_size,
            delete_color,
        );

        // Separator line
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(x, row_y_abs + item_height);
        ctx.line_to(x + width, row_y_abs + item_height);
        ctx.stroke();
    }

    // Register input zones for set rows and their delete buttons.
    // Delete buttons are registered AFTER row zones so they take priority on overlap.
    for (set_id, rect) in &item_rects {
        let zone_id = format!("ind_search:set:{}", set_id);
        input_coordinator.register_on_layer(
            zone_id.as_str(),
            uzor::types::Rect::new(rect.x, rect.y, rect.width, rect.height),
            uzor::input::sense::Sense::CLICK,
            layer_id,
        );
    }
    for (set_id, rect) in &item_rects {
        let delete_btn_size = 24.0;
        let delete_btn_x = rect.x + rect.width - 8.0 - delete_btn_size;
        let delete_btn_y = rect.y + (rect.height - delete_btn_size) / 2.0;
        let delete_zone_id = format!("ind_search:set_delete:{}", set_id);
        input_coordinator.register_on_layer(
            delete_zone_id.as_str(),
            uzor::types::Rect::new(delete_btn_x, delete_btn_y, delete_btn_size, delete_btn_size),
            uzor::input::sense::Sense::CLICK,
            layer_id,
        );
    }

    (item_rects, total_height)
}
