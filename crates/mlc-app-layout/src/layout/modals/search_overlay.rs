//! Search overlay modal renderer (symbol/indicator search).

use mlc_core::engine::render::RenderContext;
use crate::ui::toolbar_theme::ToolbarTheme;
use uzor::types::Rect as WidgetRect;
use crate::layout::render_frame::ModalSearchResult;
use crate::layout::render_chart::FrameTheme;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::ui::modal_state::{ModalState, OpenModal, IndicatorCatalogItem, IndicatorCategoryFilter, SearchResult};
use crate::ui::modal_settings::ChartScreenArea;
use crate::ui::scroll_widget::{ScrollableContainer, ScrollableConfig};
use crate::ui::widgets::{
    begin_modal_shell, end_modal_shell, ModalFooter, ModalPlacement, ModalShellSpec, ModalTheme,
};
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig, InputType};
use crate::ui::widgets::types::WidgetState;
use crate::ui::Icon;
use mlc_core::templates::indicator_set::IndicatorSet;
use mlc_core::i18n::{ModalKey, t_modal};

/// Render search overlay (symbol/indicator search).
///
/// `field_id` is the TextFieldStore key for the active search overlay — one of
/// `"symbol_search"`, `"indicator_search"`, or `"compare_search"`.
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
    field_id: &str,
    split_symbol_compare: bool,
    symbol_quick_search: bool,
) -> ModalSearchResult {
    use mlc_core::render::{TextAlign, TextBaseline};
    use mlc_core::engine::render::draw_svg_icon;

    let mut result = ModalSearchResult::default();

    let (title, placeholder, _icon) = match modal_state.current {
        OpenModal::SymbolSearch => (t_modal(ModalKey::SymbolSearch), t_modal(ModalKey::SearchSymbolPlaceholder), Icon::Search),
        OpenModal::IndicatorSearch => (t_modal(ModalKey::AddIndicator), t_modal(ModalKey::SearchIndicatorPlaceholder), Icon::Indicators),
        OpenModal::CompareSearch => (t_modal(ModalKey::CompareSymbol), t_modal(ModalKey::SearchCompareSymbolPlaceholder), Icon::Search),
        _ => return result,
    };

    let is_indicator_search = modal_state.current == OpenModal::IndicatorSearch;

    let header_height = 36.0;
    let modal_padding = 12.0;
    let input_height = 32.0;
    let scrollbar_width = 8.0;
    let sidebar_width = if is_indicator_search { 48.0 } else { 0.0 };

    let base_width = (screen.width * 0.5).max(400.0);
    let mut modal_width = if is_indicator_search { base_width + sidebar_width } else { base_width };
    let modal_height = (screen.height * 0.6).max(300.0);

    // The header carries a title, two toggles and the ✕ on the symbol and
    // compare variants. Give it the width it needs, or the title and the first
    // toggle label overlap — which they did.
    if !is_indicator_search {
        const TOGGLE_GAP: f64 = 10.0;
        const CLOSE_ZONE: f64 = 44.0;
        ctx.set_font("13px sans-serif");
        let needed = 16.0
            + ctx.measure_text(title)
            + TOGGLE_GAP
            + header_toggle_width(ctx, t_modal(ModalKey::QuickSearchToggle))
            + TOGGLE_GAP
            + header_toggle_width(ctx, t_modal(ModalKey::SeparateCompareToggle))
            + CLOSE_ZONE;
        modal_width = modal_width.max(needed).min(screen.width);
    }

    let screen_w = screen.x + screen.width;
    let screen_h = screen.y + screen.height;

    let modal_theme = ModalTheme::from_frame_theme(
        &theme.toolbar_bg,
        &toolbar_theme.separator,
        &toolbar_theme.item_text,
        &toolbar_theme.item_text_hover,
        &toolbar_theme.separator,
    );
    // A search list, not a settings surface: the shell brings the frame,
    // header, title and ✕; the list below is untouched. Fixed height, so the
    // ceiling equals what the body asks for.
    let body_h = modal_height - header_height;
    let spec = ModalShellSpec {
        padding: 0.0,
        header_height,
        placement: ModalPlacement::At(modal_state.position),
        ..ModalShellSpec::new("modal_search", title, modal_width, body_h)
            .with_footer(ModalFooter::None)
            .with_max_content_height(body_h)
    };
    let (shell, layer_id) = begin_modal_shell(
        ctx,
        input_coordinator,
        &spec,
        &modal_theme,
        (screen_w, screen_h),
    );

    result.modal_rect = shell.frame;
    result.header_rect = Some(shell.header);
    result.close_btn_rect = shell.close_btn.unwrap_or_default();

    // The body reads the frame the shell settled on.
    let (modal_x, modal_y) = (shell.frame.x, shell.frame.y);
    let modal_width = shell.frame.width;
    let modal_height = shell.frame.height;

    // ── Header toggles (2026-07-17 rework — TWO toggles only): "Quick
    // search" / "Separate compare button", the ONLY way back to the
    // button-hosted fast-search once the modal is open with
    // `symbol_quick_search == false`. Drawn on both SymbolSearch and
    // CompareSearch variants; skipped for IndicatorSearch (irrelevant
    // toggles for that modal). The former third "Inline input" toggle is
    // deleted (the inline behavior IS quick search now).
    //
    // They hang off where the shell put the ✕ rather than re-deriving it.
    if !is_indicator_search {
        let toggle_gap = 10.0;
        let mut toggle_right_edge = result.close_btn_rect.x - toggle_gap;
        toggle_right_edge = draw_header_toggle(
            ctx, input_coordinator, &layer_id,
            toggle_right_edge, modal_y, header_height,
            t_modal(ModalKey::SeparateCompareToggle), split_symbol_compare,
            "modal_search:toggle_split", toolbar_theme,
        );
        toggle_right_edge -= toggle_gap;
        let _ = draw_header_toggle(
            ctx, input_coordinator, &layer_id,
            toggle_right_edge, modal_y, header_height,
            t_modal(ModalKey::QuickSearchToggle), symbol_quick_search,
            "modal_search:toggle_quick", toolbar_theme,
        );
    }

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
                IndicatorCategoryFilter::TotalOverlay => Icon::VolumeProfile,
                IndicatorCategoryFilter::Other => Icon::MoreHorizontal,
            };
            draw_svg_icon(ctx, icon.svg(), icon_x, icon_y, tab_icon_size, tab_icon_size, icon_color);
            result.category_rects.push((i, WidgetRect::new(sidebar_x, tab_y, sidebar_width, tab_button_height)));
            let cat_wid = format!("modal_search:category:{}", i);
            input_coordinator.register_on_layer(
                cat_wid.as_str(),
                uzor::types::Rect::new(sidebar_x, tab_y, sidebar_width, tab_button_height),
                uzor::input::Sense::CLICK,
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
            uzor::input::Sense::CLICK,
            &layer_id,
        );
    }

    // Search input — reads text/cursor/selection from TextFieldStore.
    let input_y = modal_y + header_height + modal_padding;
    let input_width = content_width - modal_padding * 2.0;
    let input_x = content_x + modal_padding;

    let search_field_id = uzor::WidgetId::from(field_id);
    let is_focused = input_coordinator.text_fields().is_focused(&search_field_id);
    let input_text = input_coordinator.text_fields().text(&search_field_id).to_string();
    let cursor_pos = input_coordinator.text_fields().cursor(&search_field_id);
    let (selection_start, selection_end) = match input_coordinator.text_fields().selection_range(&search_field_id) {
        Some((lo, hi)) => (Some(lo), Some(hi)),
        None => (None, None),
    };

    // Use the store text when focused; fall back to modal_state.search_query for display.
    let display_text = if is_focused || !input_text.is_empty() {
        input_text.clone()
    } else {
        modal_state.search_query.clone()
    };
    let display_cursor = if is_focused { cursor_pos } else { display_text.len() };

    // Armed on open so the first keystroke lands, but the ring and the caret
    // wait for a click or a keystroke. The text fallback below still follows
    // FOCUS — what is displayed is a different question from what looks live.
    let is_engaged = input_coordinator.text_fields().is_engaged(&search_field_id);
    let input_config = InputConfig::new(&display_text)
        .with_placeholder(placeholder)
        .with_focused(is_engaged)
        .with_type(InputType::Search)
        .with_font_size(13.0)
        .with_padding(28.0)
        .with_radius(0.0)
        .with_cursor(display_cursor)
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
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    let search_icon_size = 14.0;
    draw_svg_icon(
        ctx, Icon::Search.svg(),
        input_x + 8.0, input_y + (input_height - search_icon_size) / 2.0,
        search_icon_size, search_icon_size,
        &toolbar_theme.item_text_muted,
    );

    if is_engaged && input_coordinator.text_fields().cursor_visible(current_time_ms) {
        draw_input_cursor(ctx, input_result.cursor_x, input_result.cursor_y, input_result.cursor_height, &toolbar_theme.item_text);
    }

    // Results area
    let mut results_y = input_y + input_height + modal_padding;
    let mut results_height = modal_y + modal_height - results_y - modal_padding;
    let results_width = content_width - modal_padding * 2.0 - scrollbar_width;

    // A header band, but only on the variant that carries the compare column.
    // The trailing + read as "add this symbol" — the same thing clicking the
    // row does — because nothing said otherwise. Naming the columns is what
    // separates "the row picks the instrument" from "the + compares it".
    let show_cmp_plus = modal_state.current == OpenModal::SymbolSearch && !split_symbol_compare;
    if show_cmp_plus {
        const HEADER_H: f64 = 22.0;
        let hx = content_x + modal_padding;
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_text_align(TextAlign::Left);
        ctx.fill_text(t_modal(ModalKey::ColSymbol), hx + 8.0, results_y + HEADER_H / 2.0);
        ctx.set_text_align(TextAlign::Right);
        ctx.fill_text(
            t_modal(ModalKey::ColCompare),
            hx + results_width - 4.0,
            results_y + HEADER_H / 2.0,
        );
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(hx, results_y + HEADER_H);
        ctx.line_to(hx + results_width, results_y + HEADER_H);
        ctx.stroke();
        results_y += HEADER_H;
        results_height -= HEADER_H;
    }

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
            ctx.fill_text(t_modal(ModalKey::NoIndicatorsInCategory), content_x + content_width / 2.0, results_y + 20.0);
        }
    } else if (modal_state.current == OpenModal::SymbolSearch || modal_state.current == OpenModal::CompareSearch)
        && !modal_state.symbol_search_results.is_empty()
    {
        // Merged-mode compare-plus icon (symbol-search-compare-inner arc,
        // Wave 2, кусок 5 — owner spec 2026-07-16): only on the SymbolSearch
        // modal (not CompareSearch, which is already the compare-add path
        // itself) and only when `split_symbol_compare == false` — the
        // separate `compare` toolbar button + its own CompareSearch modal
        // remain the compare path unchanged when the flag is `true`.
        let (rects, star_rects, cmp_rects, height) = render_symbol_search_results_scrollable(
            ctx, content_x + modal_padding, results_y, results_width, results_height,
            &modal_state.symbol_search_results, hovered_item_id, scroll_offset, theme, toolbar_theme,
            show_cmp_plus, input_coordinator,
        );
        result.item_rects = rects;
        result.star_rects = star_rects;
        result.cmp_rects = cmp_rects;
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
            ctx.fill_text(t_modal(ModalKey::TypeToSearch), center_x, results_y + 20.0);
        } else {
            ctx.fill_text(t_modal(ModalKey::NothingFound), center_x, results_y + 20.0);
        }
    }

    let widget_theme = crate::ui::widgets::types::WidgetTheme::default();
    let scroll_result = scrollable.end(ctx, total_content_height, &widget_theme);
    result.total_content_height = scroll_result.content_height;
    result.viewport_height = scroll_result.viewport_height;
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;

    if let Some(ref hr) = result.scrollbar_handle_rect {
        let inflated = uzor::types::Rect::new(hr.x - 4.0, hr.y, hr.width + 8.0, hr.height);
        input_coordinator.register_on_layer(
            "search_modal:scrollbar_handle",
            inflated,
            uzor::input::Sense::DRAG,
            &layer_id,
        );
    }
    if let Some(track_rect) = result.scrollbar_track_rect {
        input_coordinator.register_on_layer(
            "modal_search:scrollbar_track",
            uzor::types::Rect::new(track_rect.x, track_rect.y, track_rect.width, track_rect.height),
            uzor::input::Sense::CLICK,
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
                uzor::input::Sense::CLICK,
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
            uzor::input::Sense::CLICK,
            &layer_id,
        );
    }

    // Register compare-plus zones AFTER star zones (merged-mode, кусок 5) —
    // same overlap-priority reasoning as star zones above.
    for (key, cmp_rect) in &result.cmp_rects {
        input_coordinator.register_on_layer(
            format!("modal_cmp:{}", key),
            uzor::types::Rect::new(cmp_rect.x, cmp_rect.y, cmp_rect.width, cmp_rect.height),
            uzor::input::Sense::CLICK,
            &layer_id,
        );
    }

    end_modal_shell(ctx, input_coordinator, &spec, &modal_theme, &shell, &layer_id);
    result
}

/// Draw one compact labeled toggle switch in the search-overlay header
/// (symbol-search-compare-inner arc, Wave 3 — owner spec 2026-07-16),
/// right-aligned so the caller can chain multiple toggles leftward.
///
/// Layout: `[label] [pill]`, positioned so its RIGHT edge lands at
/// `right_edge`. Returns the toggle's own left edge (x) so the caller can
/// place the next toggle further left with its own gap.
///
/// The pill visual mirrors the dropdown-row toggle switch
/// (`crate::ui::dropdown`'s `DropdownItem::Item::toggle` rendering) scaled
/// down to fit the header's `header_height`.
#[allow(clippy::too_many_arguments)]
/// What a header toggle occupies, label included. Same measurement
/// `draw_header_toggle` makes, asked before the frame width is chosen — the
/// header cannot fit what nothing measured, and this one did not: the title
/// and the "Quick search" label overlapped by several pixels at every window
/// size, because the width came from the screen and the toggles were laid out
/// from the ✕ leftwards until they ran into whatever was there.
fn header_toggle_width(ctx: &mut dyn RenderContext, label: &str) -> f64 {
    const TRACK_W: f64 = 28.0;
    const LABEL_GAP: f64 = 6.0;
    ctx.set_font("11px sans-serif");
    ctx.measure_text(label) + LABEL_GAP + TRACK_W
}

fn draw_header_toggle(
    ctx: &mut dyn RenderContext,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    right_edge: f64,
    modal_y: f64,
    header_height: f64,
    label: &str,
    is_on: bool,
    widget_id: &str,
    toolbar_theme: &ToolbarTheme,
) -> f64 {
    use mlc_core::render::{TextAlign, TextBaseline};

    let track_w = 28.0;
    let track_h = 14.0;
    let track_r = track_h / 2.0;
    let label_gap = 6.0;

    let track_x = right_edge - track_w;
    let track_y = modal_y + (header_height - track_h) / 2.0;

    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_text_baseline(TextBaseline::Middle);
    let label_width = ctx.measure_text(label);
    let label_x = track_x - label_gap;
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.fill_text(label, label_x, modal_y + header_height / 2.0);

    // Track background
    ctx.set_fill_color(if is_on { &toolbar_theme.accent } else { &toolbar_theme.item_text_muted });
    ctx.fill_rounded_rect(track_x, track_y, track_w, track_h, track_r);

    // Thumb circle (10px diameter, 2px inset)
    let thumb_d = 10.0;
    let thumb_x = if is_on { track_x + track_w - thumb_d - 2.0 } else { track_x + 2.0 };
    let thumb_y = track_y + (track_h - thumb_d) / 2.0;
    ctx.set_fill_color(&toolbar_theme.item_text_active);
    ctx.fill_rounded_rect(thumb_x, thumb_y, thumb_d, thumb_d, thumb_d / 2.0);

    let hit_x = label_x - label_width;
    let hit_rect = WidgetRect::new(hit_x, modal_y, right_edge - hit_x, header_height);
    input_coordinator.register_on_layer(
        widget_id,
        uzor::types::Rect::new(hit_rect.x, hit_rect.y, hit_rect.width, hit_rect.height),
        uzor::input::Sense::CLICK,
        layer_id,
    );

    hit_x
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
    use mlc_core::render::{TextAlign, TextBaseline};
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
        ctx.set_fill_color(if is_hovered { &toolbar_theme.item_text_hover } else { &toolbar_theme.item_text });
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(&item.short_name, x + 12.0, current_y + 10.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(if is_hovered { &toolbar_theme.item_text } else { &toolbar_theme.item_text_muted });
        ctx.fill_text(&item.name, x + 12.0, current_y + 28.0);
        // The badge states the VISUALISATION SPACE and the text after it the
        // category — they are different axes. `item.overlay` is exactly that
        // space (pane 0 vs a pane of its own), so a sub-pane entry simply
        // prints its category with no badge.
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
/// Returns `(item_rects, star_rects, cmp_rects, total_height)`.  The caller
/// must register item zones first, star zones second, compare-plus zones
/// third so that overlapping icons take priority over the row's own hit
/// zone. `show_cmp_plus` gates the compare-plus icon (merged-mode
/// SymbolSearch only — кусок 5); `cmp_rects` is always empty when `false`.
fn render_symbol_search_results_scrollable(
    ctx: &mut dyn RenderContext,
    x: f64, y: f64, width: f64, viewport_height: f64,
    items: &[SearchResult],
    hovered_item_id: Option<&str>,
    scroll_offset: f64,
    _theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    show_cmp_plus: bool,
    coord: &uzor::input::InputCoordinator,
) -> (Vec<(String, WidgetRect)>, Vec<(String, WidgetRect)>, Vec<(String, WidgetRect)>, f64) {
    use mlc_core::render::{TextAlign, TextBaseline};
    use mlc_core::engine::render::draw_svg_icon;

    let item_height = 48.0;
    let star_size = 16.0;
    // Horizontal padding before the star icon.
    let star_pad_x = 8.0;
    // Space the star occupies (star_pad_x + star_size + gap after star).
    let star_col_w = star_pad_x + star_size + 4.0;
    // Compare-plus icon (merged-mode, кусок 5) — right side, its own hover
    // rect like the star, positioned left of the exchange/category text.
    let cmp_size = 16.0;
    let cmp_pad_right = 8.0;

    let total_height = items.len() as f64 * item_height;
    let mut current_y = y - scroll_offset;
    let mut item_rects = Vec::new();
    // Star rects returned separately so they can be registered AFTER items,
    // giving them higher input priority despite the overlapping row hit zone.
    let mut star_rects: Vec<(String, WidgetRect)> = Vec::new();
    let mut cmp_rects: Vec<(String, WidgetRect)> = Vec::new();
    for item in items.iter() {
        // Composite key: "BTC-USDT:binance:S" — unique even when the same ticker
        // appears under multiple account types on the same exchange.
        let item_key = format!("{}:{}:{}", item.symbol, item.exchange_id, item.account_type);

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
            (crate::ui::Icon::StarFilled, toolbar_theme.warning.as_str())
        } else {
            (crate::ui::Icon::Star, toolbar_theme.item_text_muted.as_str())
        };
        draw_svg_icon(ctx, star_icon.svg(), star_x, star_y, star_size, star_size, star_color);
        // Collect star rect — registration happens after all item rects so star wins.
        star_rects.push((item_key.clone(), WidgetRect::new(star_x, star_y, star_size, star_size)));

        // --- Compare-plus icon (right side, merged-mode only — кусок 5) ---
        // It hovers on its OWN, with its own pad, because it does a different
        // thing from the row it sits in: the row picks the instrument, the +
        // compares it. Sharing the row's highlight is what made it read as a
        // second way to do the same thing.
        if show_cmp_plus {
            const CMP_PAD: f64 = 5.0;
            let cmp_x = x + width - cmp_pad_right - cmp_size;
            let cmp_y = current_y + (item_height - cmp_size) / 2.0;
            let cmp_hovered = coord.is_hovered(&uzor::types::WidgetId::from(
                format!("modal_cmp:{item_key}").as_str(),
            ));
            if cmp_hovered {
                ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                ctx.fill_rounded_rect(
                    cmp_x - CMP_PAD,
                    cmp_y - CMP_PAD,
                    cmp_size + CMP_PAD * 2.0,
                    cmp_size + CMP_PAD * 2.0,
                    4.0,
                );
            }
            let cmp_color = if cmp_hovered {
                toolbar_theme.accent.as_str()
            } else {
                toolbar_theme.item_text_muted.as_str()
            };
            draw_svg_icon(ctx, crate::ui::Icon::Plus.svg(), cmp_x, cmp_y, cmp_size, cmp_size, cmp_color);
            cmp_rects.push((
                item_key.clone(),
                WidgetRect::new(
                    cmp_x - CMP_PAD,
                    cmp_y - CMP_PAD,
                    cmp_size + CMP_PAD * 2.0,
                    cmp_size + CMP_PAD * 2.0,
                ),
            ));
        }

        // --- Symbol text (shifted right to make room for star) ---
        let text_x = x + star_col_w;
        ctx.set_font("bold 13px sans-serif");
        ctx.set_fill_color(if is_hovered { &toolbar_theme.item_text_hover } else { &toolbar_theme.item_text });
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(&item.symbol, text_x, current_y + 10.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(if is_hovered { &toolbar_theme.item_text } else { &toolbar_theme.item_text_muted });
        ctx.fill_text(&item.name, text_x, current_y + 28.0);

        // --- Exchange / category (right side) --- shifted left when the
        // compare-plus icon occupies the row's right edge (merged-mode,
        // кусок 5) so the two never overlap.
        let right_edge = if show_cmp_plus { x + width - cmp_pad_right - cmp_size - 8.0 } else { x + width - 12.0 };
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Right);
        ctx.set_text_baseline(TextBaseline::Middle);
        // Exchange labels render UPPERCASE everywhere (display-only, owner
        // rule 2026-07-17 — keys/queries keep the canonical lowercase id).
        // Right-aligned, so a long venue grows LEFT — straight into the
        // category icon 58px away, which is what "KUCOIN" and "GATEIO" were
        // doing. Shorten to the budget instead of letting two labels share
        // pixels.
        const VENUE_BUDGET: f64 = 50.0;
        let venue = ellipsize(ctx, &item.exchange.to_uppercase(), VENUE_BUDGET);
        ctx.fill_text(&venue, right_edge, current_y + item_height / 2.0);
        ctx.set_font("12px sans-serif");
        ctx.fill_text(&item.category_icon, right_edge - 58.0, current_y + item_height / 2.0);

        // Account type badge (S/F) — same style as category icon
        if !item.account_type.is_empty() {
            ctx.fill_text(&item.account_type, right_edge - 78.0, current_y + item_height / 2.0);
        }

        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(x, current_y + item_height);
        ctx.line_to(x + width, current_y + item_height);
        ctx.stroke();
        current_y += item_height;
    }
    (item_rects, star_rects, cmp_rects, total_height)
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
    use mlc_core::render::{TextAlign, TextBaseline};
    use mlc_core::engine::render::draw_svg_icon;

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
        ctx.fill_text(t_modal(ModalKey::SaveIndicatorSet), x + 10.0 + btn_icon_size + 6.0, btn_y + create_btn_height / 2.0);
    }

    input_coordinator.register_on_layer(
        "ind_search:set_create",
        uzor::types::Rect::new(x, y - scroll_offset, width, create_btn_height),
        uzor::input::Sense::CLICK,
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
            ctx.fill_text(t_modal(ModalKey::NoSavedIndicatorSets), x + width / 2.0, msg_y);
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
        ctx.set_fill_color(if is_hovered { &toolbar_theme.item_text_hover } else { &toolbar_theme.item_text });
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
        let delete_color = if is_delete_hovered { &toolbar_theme.danger } else { &toolbar_theme.separator };
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
            uzor::input::Sense::CLICK,
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
            uzor::input::Sense::CLICK,
            layer_id,
        );
    }

    (item_rects, total_height)
}

/// Shorten `text` with a trailing ellipsis until it fits `budget` pixels.
///
/// Assumes the caller has already set the font it will draw with.
fn ellipsize(ctx: &mut dyn RenderContext, text: &str, budget: f64) -> String {
    if ctx.measure_text(text) <= budget {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    for keep in (1..chars.len()).rev() {
        let candidate: String = chars[..keep].iter().collect::<String>() + "…";
        if ctx.measure_text(&candidate) <= budget {
            return candidate;
        }
    }
    "…".to_string()
}
