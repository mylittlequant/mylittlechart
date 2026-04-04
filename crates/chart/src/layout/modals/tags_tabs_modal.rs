//! Tags & Tabs modal renderer.
//!
//! A large modal with a left sidebar (TABS / TAGS) and a content area.
//! The TABS section embeds the overlay-settings panel tree UI.
//! The TAGS section shows sync groups with their members and sync flags.

use std::collections::HashMap;

use uzor::types::Rect as WidgetRect;
use uzor::input::sense::Sense;

use crate::engine::render::RenderContext;
use crate::engine::render::draw_svg_icon;
use crate::layout::render_chart::FrameTheme;
use crate::render::{TextAlign, TextBaseline};
use crate::tag_manager::TagManager;
use crate::theme::ThemeManager;
use crate::ui::modal_settings::{OverlaySettingsState, TagsTabsState, TagsTabsSidebar, TagsTabsTagsTab};
use crate::ui::scroll_state::ScrollState;
use crate::ui::scroll_widget::ScrollableContainer;
use crate::ui::toolbar_render::ToolbarTheme;
use crate::ui::widgets::{render_modal_frame_only, ModalTheme};
use crate::ui::widgets::types::WidgetTheme;
use crate::ui::Icon;
use crate::ui::z_order::ZLayer;
use uzor::panels::{LeafId, BranchId, PanelNode, Leaf};
use crate::state::panel_grid::ChartPanelGrid;
use crate::state::sub_panel::ChartSubPanel;

// =============================================================================
// Result type
// =============================================================================

/// Hit-test rectangles returned by [`render_tags_tabs_modal`].
#[derive(Clone, Debug, Default)]
pub struct TagsTabsResult {
    /// The full modal frame (for click-outside detection).
    pub modal_rect: WidgetRect,
    /// The title-bar / header area (for drag initiation).
    pub header_rect: WidgetRect,
    /// The close button rect (for hit-testing).
    pub close_btn_rect: Option<WidgetRect>,
    /// Left sidebar item rects: (sidebar_id, rect).
    pub sidebar_rects: Vec<(String, WidgetRect)>,
    /// Sub-tab rects: (tab_id, rect).
    pub sub_tab_rects: Vec<(String, WidgetRect)>,
    /// Content item rects: (widget_id, rect).
    pub content_items: Vec<(String, WidgetRect)>,
    /// Viewport rect of the scrollable content area (for wheel hit-test).
    pub scroll_viewport_rect: Option<WidgetRect>,
    /// Total rendered content height (for scroll range).
    pub scroll_content_height: f64,
    /// Scrollbar handle rect (for drag detection).
    pub scrollbar_handle_rect: Option<WidgetRect>,
    /// Scrollbar track rect (for drag calculation).
    pub scrollbar_track_rect: Option<WidgetRect>,
    /// Viewport height used for scroll clamping.
    pub scroll_viewport_height: f64,
}

// =============================================================================
// Layout constants
// =============================================================================

const MODAL_WIDTH: f64  = 520.0;
const MODAL_HEIGHT: f64 = 450.0;
const HEADER_HEIGHT: f64 = 36.0;
const SIDEBAR_WIDTH: f64 = 80.0;
const SUB_TAB_HEIGHT: f64 = 32.0;
const MODAL_PADDING: f64 = 12.0;
const CLOSE_BTN_SIZE: f64 = 20.0;
const ROW_HEIGHT: f64 = 28.0;
const ROW_GAP: f64 = 6.0;
const SWATCH_SIZE: f64 = 12.0;

// =============================================================================
// Renderer
// =============================================================================

/// Render the Tags & Tabs modal.
///
/// Returns hit-zone information used by the input handler for click dispatch.
#[allow(clippy::too_many_arguments)]
pub fn render_tags_tabs_modal(
    ctx: &mut dyn RenderContext,
    state: &TagsTabsState,
    overlay_state: &OverlaySettingsState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    panel_grid: &ChartPanelGrid,
    tag_manager: &TagManager,
    _leaf_color_tags: &HashMap<LeafId, [f32; 4]>,
    screen_w: f64,
    screen_h: f64,
    theme_manager: &ThemeManager,
    chart_area_w: f64,
    chart_area_h: f64,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> TagsTabsResult {
    let mut result = TagsTabsResult::default();

    // =========================================================================
    // Position calculation (draggable, centered by default)
    // =========================================================================
    let (modal_x, modal_y) = state.position.unwrap_or_else(|| {
        (
            (screen_w - MODAL_WIDTH) / 2.0,
            (screen_h - MODAL_HEIGHT) / 2.0,
        )
    });
    let modal_x = modal_x.max(0.0).min((screen_w - MODAL_WIDTH).max(0.0));
    let modal_y = modal_y.max(0.0).min((screen_h - MODAL_HEIGHT).max(0.0));

    result.modal_rect  = WidgetRect::new(modal_x, modal_y, MODAL_WIDTH, MODAL_HEIGHT);
    result.header_rect = WidgetRect::new(modal_x, modal_y, MODAL_WIDTH, HEADER_HEIGHT);

    // =========================================================================
    // InputCoordinator layer
    // =========================================================================
    let layer_id = ZLayer::Modal.push_named(input_coordinator, "tags_tabs");

    // Register modal background as click absorber
    input_coordinator.register_on_layer(
        "tags_tabs:modal_bg",
        WidgetRect::new(modal_x, modal_y, MODAL_WIDTH, MODAL_HEIGHT),
        Sense::CLICK,
        &layer_id,
    );

    // =========================================================================
    // 1. Modal frame
    // =========================================================================
    let modal_theme = ModalTheme::from_frame_theme(
        &frame_theme.toolbar_bg,
        &frame_theme.toolbar_border,
        &toolbar_theme.item_text,
        &toolbar_theme.item_text_active,
        &frame_theme.toolbar_border,
    );
    render_modal_frame_only(ctx, result.modal_rect, &modal_theme, 0.0);

    // =========================================================================
    // 2. Header
    // =========================================================================
    // Header background
    ctx.set_fill_color(&frame_theme.toolbar_bg);
    ctx.fill_rect(modal_x, modal_y, MODAL_WIDTH, HEADER_HEIGHT);

    // Title
    ctx.set_font("bold 13px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("TAGS & TABS", modal_x + 16.0, modal_y + HEADER_HEIGHT / 2.0);

    // Close button
    let close_x = modal_x + MODAL_WIDTH - CLOSE_BTN_SIZE - 12.0;
    let close_y = modal_y + (HEADER_HEIGHT - CLOSE_BTN_SIZE) / 2.0;
    let close_rect = WidgetRect::new(close_x, close_y, CLOSE_BTN_SIZE, CLOSE_BTN_SIZE);
    result.close_btn_rect = Some(close_rect);

    draw_svg_icon(
        ctx,
        Icon::Close.svg(),
        close_x, close_y,
        CLOSE_BTN_SIZE, CLOSE_BTN_SIZE,
        &toolbar_theme.item_text,
    );
    input_coordinator.register_on_layer(
        "tags_tabs:close",
        close_rect,
        Sense::CLICK,
        &layer_id,
    );

    // Header bottom border
    ctx.set_stroke_color(&frame_theme.toolbar_border);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, modal_y + HEADER_HEIGHT);
    ctx.line_to(modal_x + MODAL_WIDTH, modal_y + HEADER_HEIGHT);
    ctx.stroke();

    // =========================================================================
    // 3. Left sidebar (below header)
    // =========================================================================
    let sidebar_top    = modal_y + HEADER_HEIGHT;
    let sidebar_height = MODAL_HEIGHT - HEADER_HEIGHT;
    let sidebar_items  = [
        ("tabs",  "TABS",  TagsTabsSidebar::Tabs),
        ("tags",  "TAGS",  TagsTabsSidebar::Tags),
        ("map",   "MAP",   TagsTabsSidebar::Map),
    ];

    let sidebar_item_height = 40.0;
    let sidebar_items_total = sidebar_item_height * sidebar_items.len() as f64;
    let sidebar_start_y = sidebar_top + (sidebar_height - sidebar_items_total) / 2.0;
    let mut sb_y = sidebar_start_y;

    for (id, label, variant) in &sidebar_items {
        let is_active = state.sidebar == *variant;
        let item_rect = WidgetRect::new(modal_x, sb_y, SIDEBAR_WIDTH, sidebar_item_height);
        let sidebar_wid = format!("tags_tabs:sidebar:{}", id);
        let is_hovered = overlay_state.hovered_item_id.as_deref() == Some(sidebar_wid.as_str());

        // Active background
        if is_active {
            ctx.set_fill_color(&crate::apply_opacity(&toolbar_theme.accent, 0.20));
            ctx.fill_rounded_rect(item_rect.x + 4.0, item_rect.y + 2.0, SIDEBAR_WIDTH - 8.0, sidebar_item_height - 4.0, 4.0);
        } else if is_hovered {
            ctx.set_fill_color(&crate::apply_opacity(&toolbar_theme.item_text, 0.08));
            ctx.fill_rounded_rect(item_rect.x + 4.0, item_rect.y + 2.0, SIDEBAR_WIDTH - 8.0, sidebar_item_height - 4.0, 4.0);
        }

        // Label
        let text_color = if is_active {
            toolbar_theme.accent.as_str()
        } else {
            toolbar_theme.item_text.as_str()
        };
        ctx.set_font("bold 11px sans-serif");
        ctx.set_fill_color(text_color);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, modal_x + SIDEBAR_WIDTH / 2.0, sb_y + sidebar_item_height / 2.0);

        result.sidebar_rects.push((sidebar_wid.clone(), item_rect));
        input_coordinator.register_on_layer(
            sidebar_wid.as_str(),
            item_rect,
            Sense::CLICK,
            &layer_id,
        );

        sb_y += sidebar_item_height;
    }

    // Vertical separator between sidebar and content
    ctx.set_stroke_color(&frame_theme.toolbar_border);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x + SIDEBAR_WIDTH, sidebar_top);
    ctx.line_to(modal_x + SIDEBAR_WIDTH, modal_y + MODAL_HEIGHT);
    ctx.stroke();

    // =========================================================================
    // 4. Content area (right of sidebar, below header)
    // =========================================================================
    let content_x      = modal_x + SIDEBAR_WIDTH;
    let content_y      = sidebar_top;
    let content_width  = MODAL_WIDTH - SIDEBAR_WIDTH;
    let content_height = MODAL_HEIGHT - HEADER_HEIGHT;

    // WidgetTheme used for scrollbar rendering in TABS and TAGS sections
    let scroll_widget_theme = WidgetTheme {
        bg_normal:      toolbar_theme.item_bg_hover.clone(),
        bg_hover:       toolbar_theme.item_bg_hover.clone(),
        bg_pressed:     toolbar_theme.item_bg_active.clone(),
        bg_disabled:    toolbar_theme.item_bg_hover.clone(),
        text_normal:    toolbar_theme.item_text.clone(),
        text_hover:     toolbar_theme.item_text_active.clone(),
        text_disabled:  toolbar_theme.item_text_muted.clone(),
        border_normal:  toolbar_theme.separator.clone(),
        border_hover:   toolbar_theme.separator.clone(),
        border_focused: toolbar_theme.item_bg_active.clone(),
        accent:         toolbar_theme.accent.clone(),
        accent_hover:   toolbar_theme.accent.clone(),
        success:        "#26a69a".to_string(),
        warning:        "#ff9800".to_string(),
        danger:         "#ef5350".to_string(),
    };

    match state.sidebar {
        // =====================================================================
        // TABS sidebar
        // =====================================================================
        TagsTabsSidebar::Tabs => {
            render_tabs_section(
                ctx,
                overlay_state,
                frame_theme,
                toolbar_theme,
                theme_manager,
                panel_grid,
                chart_area_w,
                chart_area_h,
                modal_x,
                content_x,
                content_y,
                content_width,
                content_height,
                &state.tabs_scroll,
                &scroll_widget_theme,
                &layer_id,
                input_coordinator,
                &mut result,
            );
        }

        // =====================================================================
        // TAGS sidebar
        // =====================================================================
        TagsTabsSidebar::Tags => {
            let tags_scroll = match state.tags_tab {
                TagsTabsTagsTab::Groups  => &state.tags_groups_scroll,
                TagsTabsTagsTab::Details => &state.tags_details_scroll,
            };
            render_tags_section(
                ctx,
                state,
                overlay_state,
                frame_theme,
                toolbar_theme,
                theme_manager,
                tag_manager,
                content_x,
                content_y,
                content_width,
                content_height,
                tags_scroll,
                &scroll_widget_theme,
                &layer_id,
                input_coordinator,
                &mut result,
            );
        }

        // =====================================================================
        // MAP sidebar — unified minimap with tag-colored leaves
        // =====================================================================
        TagsTabsSidebar::Map => {
            render_map_section(
                ctx,
                overlay_state,
                frame_theme,
                toolbar_theme,
                theme_manager,
                panel_grid,
                tag_manager,
                _leaf_color_tags,
                chart_area_w,
                chart_area_h,
                modal_x,
                content_x,
                content_y,
                content_width,
                content_height,
                &layer_id,
                input_coordinator,
                &mut result,
            );
        }
    }

    input_coordinator.pop_layer(&layer_id);
    result
}

// =============================================================================
// Helper: count tree nodes recursively
// =============================================================================

fn count_nodes(children: &[PanelNode<ChartSubPanel>]) -> usize {
    let mut count = 0;
    for child in children {
        count += 1;
        if let PanelNode::Branch(b) = child {
            count += count_nodes(&b.children);
        }
    }
    count
}

// =============================================================================
// Helper: render tree nodes recursively (TreeView tab)
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_tree_nodes(
    ctx: &mut dyn RenderContext,
    children: &[PanelNode<ChartSubPanel>],
    content_left: f64,
    row_y: &mut f64,
    row_height: f64,
    row_gap: f64,
    modal_width: f64,
    modal_padding: f64,
    indent: f64,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    theme_manager: &ThemeManager,
    overlay_state: &OverlaySettingsState,
    result: &mut TagsTabsResult,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::coordinator::LayerId,
    target_leaf_id: Option<LeafId>,
    selected_node_id: Option<u64>,
    expanded_leaf_id: Option<LeafId>,
    all_leaf_count: usize,
    visible_leaf_count: usize,
    modal_x: f64,
) {
    let rt = theme_manager.current();
    let colors = &rt.colors;
    for child in children {
        let x = content_left + indent;
        let node_id = child.raw_id();
        let is_selected = Some(node_id) == selected_node_id;
        // Show target highlight only when nothing is manually selected
        let is_target = selected_node_id.is_none()
            && child.leaf_id().map(|lid| Some(lid) == target_leaf_id).unwrap_or(false);

        // Left accent bar (selected takes priority over target)
        if is_selected || is_target {
            let bar_color = if is_selected { &toolbar_theme.accent } else { &toolbar_theme.item_bg_active };
            ctx.set_fill_color(bar_color);
            ctx.fill_rounded_rect(content_left, *row_y + 2.0, 3.0, row_height - 4.0, 1.5);
        }

        // Icon + Label
        let (label, node_icon) = match child {
            PanelNode::Leaf(l) => {
                let title = l.active_panel().map(|p| p.title.clone()).unwrap_or_else(|| "Empty".to_string());
                let short = if title.chars().count() > 30 {
                    format!("{}...", &title[..title.char_indices().nth(27).map(|(b, _)| b).unwrap_or(title.len())])
                } else {
                    title
                };
                let hidden_mark = if l.hidden { " [hidden]" } else { "" };
                (format!("Leaf {} - {}{}", l.id.0, short, hidden_mark), Icon::LayoutSingle)
            }
            PanelNode::Branch(b) => {
                (format!("Branch {} [{} ch., {:?}]", b.id.0, b.children.len(), b.layout), Icon::Grid)
            }
        };

        let text_color = if child.is_hidden() { &toolbar_theme.item_text_muted } else { &toolbar_theme.item_text };
        // Draw node type icon
        let icon_size = 12.0;
        let icon_y = *row_y + (row_height - icon_size) / 2.0;
        draw_svg_icon(ctx, node_icon.svg(), x, icon_y, icon_size, icon_size, text_color);
        // Draw label shifted right by icon width + gap
        ctx.set_fill_color(text_color);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&label, x + 16.0, *row_y + row_height / 2.0);

        // Row click-to-select
        let widget_id = format!("tags_tabs:select:{}", node_id);
        let item_rect = WidgetRect::new(content_left, *row_y, modal_width - modal_padding * 2.0, row_height);
        result.content_items.push((widget_id.clone(), item_rect));
        input_coordinator.register_on_layer(
            widget_id.as_str(),
            WidgetRect::new(content_left, *row_y, modal_width - modal_padding * 2.0, row_height),
            Sense::CLICK,
            layer_id,
        );

        // Inline action buttons for leaf nodes only
        if let PanelNode::Leaf(l) = child {
            let btn_size = 20.0;
            let btn_gap  = 4.0;
            let btn_y    = *row_y + (row_height - btn_size) / 2.0;
            let right_edge = modal_x + modal_width - modal_padding;
            let icon_size = 12.0;

            // Button 3 (rightmost): Eliminate [Close icon] — only if >1 leaf
            if all_leaf_count > 1 {
                let btn_x = right_edge - btn_size;
                let wid = format!("tags_tabs:eliminate:{}", l.id.0);
                let is_hovered = overlay_state.hovered_item_id.as_deref() == Some(wid.as_str());
                let bg = if is_hovered { colors.danger.as_str() } else { toolbar_theme.item_bg_hover.as_str() };
                ctx.set_fill_color(bg);
                ctx.fill_rounded_rect(btn_x, btn_y, btn_size, btn_size, 3.0);
                let icon_x = btn_x + (btn_size - icon_size) / 2.0;
                let icon_y = btn_y + (btn_size - icon_size) / 2.0;
                let icon_color = if is_hovered { &toolbar_theme.item_text_active } else { &colors.danger };
                draw_svg_icon(ctx, Icon::Close.svg(), icon_x, icon_y, icon_size, icon_size, icon_color);
                let r = WidgetRect::new(btn_x, btn_y, btn_size, btn_size);
                result.content_items.push((wid.clone(), r));
                input_coordinator.register_on_layer(wid.as_str(), r, Sense::CLICK, layer_id);
            }

            // Button 2: Hide/Show [EyeOff/Eye icon] — only if >1 visible or already hidden
            let can_hide = visible_leaf_count > 1 || l.hidden;
            if can_hide {
                let btn_x = right_edge - btn_size * 2.0 - btn_gap;
                let wid = if l.hidden {
                    format!("tags_tabs:show:{}", l.id.0)
                } else {
                    format!("tags_tabs:hide:{}", l.id.0)
                };
                let is_hovered = overlay_state.hovered_item_id.as_deref() == Some(wid.as_str());
                let bg = if l.hidden {
                    if is_hovered { colors.success.as_str() } else { toolbar_theme.item_bg_hover.as_str() }
                } else {
                    if is_hovered { toolbar_theme.item_bg_active.as_str() } else { toolbar_theme.accent.as_str() }
                };
                ctx.set_fill_color(bg);
                ctx.fill_rounded_rect(btn_x, btn_y, btn_size, btn_size, 3.0);
                let icon_color = if is_hovered { &toolbar_theme.item_text_active } else if l.hidden { &toolbar_theme.item_text_muted } else { &toolbar_theme.item_text_active };
                let icon_x = btn_x + (btn_size - icon_size) / 2.0;
                let icon_y = btn_y + (btn_size - icon_size) / 2.0;
                let icon = if l.hidden { Icon::EyeOff } else { Icon::Eye };
                draw_svg_icon(ctx, icon.svg(), icon_x, icon_y, icon_size, icon_size, icon_color);
                let r = WidgetRect::new(btn_x, btn_y, btn_size, btn_size);
                result.content_items.push((wid.clone(), r));
                input_coordinator.register_on_layer(wid.as_str(), r, Sense::CLICK, layer_id);
            }

            // Button 1 (leftmost): Expand/Suppress [Collapse/Expand icon] — only if not hidden
            if !l.hidden {
                let btn_x = right_edge - btn_size * 3.0 - btn_gap * 2.0;
                let is_exp = expanded_leaf_id == Some(l.id);
                let wid = format!("tags_tabs:expand:{}", l.id.0);
                let is_hovered = overlay_state.hovered_item_id.as_deref() == Some(wid.as_str());
                let expand_hover_bg = if is_exp {
                    colors.success.clone()
                } else {
                    crate::apply_opacity(&colors.success, 0.3)
                };
                let bg = if is_hovered {
                    expand_hover_bg.as_str()
                } else if is_exp {
                    toolbar_theme.item_bg_active.as_str()
                } else {
                    toolbar_theme.background.as_str()
                };
                ctx.set_fill_color(bg);
                ctx.fill_rounded_rect(btn_x, btn_y, btn_size, btn_size, 3.0);
                let icon_color = if is_hovered { &toolbar_theme.item_text_active } else if is_exp { &colors.success } else { &toolbar_theme.item_text_muted };
                let icon_x = btn_x + (btn_size - icon_size) / 2.0;
                let icon_y = btn_y + (btn_size - icon_size) / 2.0;
                let icon = if is_exp { Icon::Collapse } else { Icon::Expand };
                draw_svg_icon(ctx, icon.svg(), icon_x, icon_y, icon_size, icon_size, icon_color);
                let r = WidgetRect::new(btn_x, btn_y, btn_size, btn_size);
                result.content_items.push((wid.clone(), r));
                input_coordinator.register_on_layer(wid.as_str(), r, Sense::CLICK, layer_id);
            }
        }

        *row_y += row_height + row_gap;

        // Recurse into branch children
        if let PanelNode::Branch(b) = child {
            render_tree_nodes(
                ctx, &b.children, content_left, row_y, row_height, row_gap,
                modal_width, modal_padding, indent + 16.0,
                frame_theme, toolbar_theme, theme_manager, overlay_state, result, input_coordinator, layer_id,
                target_leaf_id, selected_node_id, expanded_leaf_id,
                all_leaf_count, visible_leaf_count, modal_x,
            );
        }
    }
}

// =============================================================================
// TABS section renderer
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_tabs_section(
    ctx: &mut dyn RenderContext,
    overlay_state: &OverlaySettingsState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    theme_manager: &ThemeManager,
    panel_grid: &ChartPanelGrid,
    _chart_area_w: f64,
    _chart_area_h: f64,
    modal_x: f64,
    content_x: f64,
    content_y: f64,
    content_width: f64,
    content_height: f64,
    tabs_scroll: &ScrollState,
    scroll_widget_theme: &WidgetTheme,
    layer_id: &uzor::input::coordinator::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut TagsTabsResult,
) {
    use crate::ui::modal_settings::OverlayPanelTreeTab;

    let rt = theme_manager.current();
    let colors = &rt.colors;

    // Sub-tab bar
    let tab_padding_h = 12.0;
    let tab_gap       = 2.0;
    ctx.set_font("12px sans-serif");
    let tabs = OverlayPanelTreeTab::all();
    let tab_widths: Vec<f64> = tabs
        .iter()
        .map(|t| ctx.measure_text(t.label()) + tab_padding_h * 2.0)
        .collect();

    let mut tab_x = content_x + MODAL_PADDING;
    for (i, tab) in tabs.iter().enumerate() {
        let tab_w    = tab_widths[i];
        let tab_rect = WidgetRect::new(tab_x, content_y, tab_w, SUB_TAB_HEIGHT);
        let is_active = overlay_state.active_tab == *tab;
        let wid = format!("tags_tabs:tab:{}", tab.id());
        let is_hovered = overlay_state.hovered_item_id.as_deref() == Some(wid.as_str());

        if is_active {
            ctx.set_fill_color(&toolbar_theme.accent);
            ctx.fill_rounded_rect(tab_rect.x, tab_rect.y, tab_rect.width, tab_rect.height, 3.0);
        } else if is_hovered {
            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rounded_rect(tab_rect.x, tab_rect.y, tab_rect.width, tab_rect.height, 3.0);
        }

        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(
            if is_active { &toolbar_theme.item_text_active }
            else if is_hovered { &toolbar_theme.item_text_hover }
            else { &toolbar_theme.item_text },
        );
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(
            tab.label(),
            tab_rect.x + tab_rect.width / 2.0,
            tab_rect.y + tab_rect.height / 2.0,
        );

        result.sub_tab_rects.push((wid.clone(), tab_rect));
        input_coordinator.register_on_layer(wid.as_str(), tab_rect, Sense::CLICK, layer_id);

        tab_x += tab_w + tab_gap;
    }

    // Sub-tab bar bottom border
    ctx.set_stroke_color(&frame_theme.toolbar_border);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(content_x, content_y + SUB_TAB_HEIGHT);
    ctx.line_to(content_x + content_width, content_y + SUB_TAB_HEIGHT);
    ctx.stroke();

    // =========================================================================
    // Data gathering from panel_grid
    // =========================================================================
    let tree = panel_grid.docking().tree();
    let all_leaves: Vec<&Leaf<ChartSubPanel>> = tree.leaves();
    let hidden_leaves: Vec<&Leaf<ChartSubPanel>> = all_leaves.iter()
        .filter(|l| l.hidden)
        .copied()
        .collect();

    let expanded_leaf_id: Option<LeafId> = if panel_grid.is_expanded() {
        panel_grid.docking().active_leaf()
    } else {
        None
    };

    let root = tree.root();
    let tree_node_count = count_nodes(&root.children).max(1);
    let _ = tree_node_count; // used implicitly by count_nodes above

    // Content area begins below the sub-tab bar
    let content_top = content_y + SUB_TAB_HEIGHT;

    // Scrollable viewport covers the area below the sub-tab bar
    let scroll_viewport_h = content_height - SUB_TAB_HEIGHT;
    let viewport_rect = WidgetRect::new(content_x, content_top, content_width, scroll_viewport_h);

    let container = ScrollableContainer::new(viewport_rect, tabs_scroll, None);
    container.begin(ctx);
    let content_left = content_x + MODAL_PADDING;
    let mut row_y    = container.content_y() + MODAL_PADDING;

    // Set default text style
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);

    match overlay_state.active_tab {
        // =================================================================
        // Tab 1: TreeView
        // =================================================================
        OverlayPanelTreeTab::TreeView => {
            // Root row
            let root_text = format!(
                "Root [{}] - {} children, layout: {:?}",
                root.id.0, root.children.len(), root.layout
            );
            ctx.set_fill_color(&toolbar_theme.item_text);
            // Draw object-tree icon before Root label
            let icon_size_root = 12.0;
            let icon_y_root = row_y + (ROW_HEIGHT - icon_size_root) / 2.0;
            draw_svg_icon(ctx, Icon::ObjectTree.svg(), content_left, icon_y_root, icon_size_root, icon_size_root, &toolbar_theme.item_text);
            ctx.set_font("12px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(&root_text, content_left + 16.0, row_y + ROW_HEIGHT / 2.0);

            // Register root row as selectable
            let root_wid = format!("tags_tabs:select:{}", root.id.0);
            let root_rect = WidgetRect::new(content_left, row_y, content_width - MODAL_PADDING * 2.0, ROW_HEIGHT);
            result.content_items.push((root_wid.clone(), root_rect));
            input_coordinator.register_on_layer(root_wid.as_str(), root_rect, Sense::CLICK, layer_id);
            row_y += ROW_HEIGHT + ROW_GAP;

            // Recursive tree rendering
            render_tree_nodes(
                ctx, &root.children.clone(), content_left, &mut row_y,
                ROW_HEIGHT, ROW_GAP, content_width, MODAL_PADDING,
                16.0,
                frame_theme, toolbar_theme, theme_manager, overlay_state, result, input_coordinator, layer_id,
                overlay_state.target_leaf_id, overlay_state.selected_node_id,
                expanded_leaf_id,
                all_leaves.len(),
                tree.visible_leaf_count(),
                modal_x,
            );
        }

        // =================================================================
        // Tab 2: Eliminate
        // =================================================================
        OverlayPanelTreeTab::Eliminate => {
            let can_delete = all_leaves.len() > 1;

            for leaf in &all_leaves {
                let leaf_id_val = leaf.id.0;
                let title = leaf.active_panel().map(|p| p.title.clone()).unwrap_or_else(|| "Empty".to_string());
                let short = if title.chars().count() > 25 {
                    format!("{}...", &title[..title.char_indices().nth(22).map(|(b, _)| b).unwrap_or(title.len())])
                } else {
                    title
                };
                let is_target = Some(leaf.id) == overlay_state.target_leaf_id;

                // Left accent bar for target leaf row (no full-width fill)
                if is_target {
                    ctx.set_fill_color(&toolbar_theme.accent);
                    ctx.fill_rounded_rect(content_left, row_y + 2.0, 3.0, ROW_HEIGHT - 4.0, 1.5);
                }

                // Icon + Label
                let text_color = if leaf.hidden { &toolbar_theme.item_text_muted } else { &toolbar_theme.item_text };
                let icon_size_el = 12.0;
                let icon_y_el = row_y + (ROW_HEIGHT - icon_size_el) / 2.0;
                draw_svg_icon(ctx, Icon::LayoutSingle.svg(), content_left, icon_y_el, icon_size_el, icon_size_el, text_color);
                ctx.set_fill_color(text_color);
                ctx.set_font("12px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(
                    &format!("Leaf {} - {}", leaf_id_val, short),
                    content_left + 16.0,
                    row_y + ROW_HEIGHT / 2.0,
                );

                // Delete button
                if can_delete {
                    let btn_w = 60.0;
                    let btn_h = ROW_HEIGHT - 6.0;
                    let btn_x = content_x + content_width - MODAL_PADDING - btn_w;
                    let btn_y_inner = row_y + 3.0;

                    let wid_str = format!("tags_tabs:eliminate:{}", leaf_id_val);
                    let is_hovered = overlay_state.hovered_item_id.as_deref() == Some(wid_str.as_str());
                    let btn_bg = if is_hovered { &colors.danger } else { &toolbar_theme.item_bg_hover };
                    ctx.set_fill_color(btn_bg);
                    ctx.fill_rounded_rect(btn_x, btn_y_inner, btn_w, btn_h, 3.0);
                    ctx.set_fill_color(&toolbar_theme.item_text_active);
                    ctx.set_font("12px sans-serif");
                    ctx.set_text_align(TextAlign::Center);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text("Delete", btn_x + btn_w / 2.0, btn_y_inner + btn_h / 2.0);

                    let r = WidgetRect::new(btn_x, btn_y_inner, btn_w, btn_h);
                    result.content_items.push((wid_str.clone(), r));
                    input_coordinator.register_on_layer(wid_str.as_str(), r, Sense::CLICK, layer_id);
                }

                row_y += ROW_HEIGHT + ROW_GAP;
            }

            // Empty state
            if all_leaves.is_empty() {
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.set_font("12px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("No panels", content_left, row_y + ROW_HEIGHT / 2.0);
            }
        }

        // =================================================================
        // Tab 3: Hidden
        // =================================================================
        OverlayPanelTreeTab::Hidden => {
            if hidden_leaves.is_empty() {
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.set_font("12px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("No hidden panels", content_left, row_y + ROW_HEIGHT / 2.0);
            } else {
                for leaf in &hidden_leaves {
                    let leaf_id_val = leaf.id.0;
                    let title = leaf.active_panel().map(|p| p.title.clone()).unwrap_or_else(|| "Empty".to_string());
                    let short = if title.chars().count() > 25 {
                        format!("{}...", &title[..title.char_indices().nth(22).map(|(b, _)| b).unwrap_or(title.len())])
                    } else {
                        title
                    };

                    let icon_size_hid = 12.0;
                    let icon_y_hid = row_y + (ROW_HEIGHT - icon_size_hid) / 2.0;
                    draw_svg_icon(ctx, Icon::LayoutSingle.svg(), content_left, icon_y_hid, icon_size_hid, icon_size_hid, &toolbar_theme.item_text_muted);
                    ctx.set_fill_color(&toolbar_theme.item_text_muted);
                    ctx.set_font("12px sans-serif");
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(
                        &format!("Leaf {} - {}", leaf_id_val, short),
                        content_left + 16.0,
                        row_y + ROW_HEIGHT / 2.0,
                    );

                    // Restore button
                    let btn_w = 80.0;
                    let btn_h = ROW_HEIGHT - 6.0;
                    let btn_x = content_x + content_width - MODAL_PADDING - btn_w;
                    let btn_y_inner = row_y + 3.0;

                    let wid_str = format!("tags_tabs:restore:{}", leaf_id_val);
                    let is_hovered = overlay_state.hovered_item_id.as_deref() == Some(wid_str.as_str());
                    let btn_bg = if is_hovered { &colors.success } else { &toolbar_theme.item_bg_hover };
                    ctx.set_fill_color(btn_bg);
                    ctx.fill_rounded_rect(btn_x, btn_y_inner, btn_w, btn_h, 3.0);
                    ctx.set_fill_color(&toolbar_theme.item_text_active);
                    ctx.set_font("12px sans-serif");
                    ctx.set_text_align(TextAlign::Center);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text("Restore", btn_x + btn_w / 2.0, btn_y_inner + btn_h / 2.0);

                    let r = WidgetRect::new(btn_x, btn_y_inner, btn_w, btn_h);
                    result.content_items.push((wid_str.clone(), r));
                    input_coordinator.register_on_layer(wid_str.as_str(), r, Sense::CLICK, layer_id);

                    row_y += ROW_HEIGHT + ROW_GAP;
                }
            }
        }

        // =================================================================
        // Tab 4: Minimap — moved to standalone MAP sidebar section
        // =================================================================
        OverlayPanelTreeTab::Minimap => {
            // Minimap moved to standalone MAP sidebar section
        }
    }

    // Close the scrollable container and write scroll results
    let content_y_start = container.content_y();
    let total_content_h = row_y - content_y_start;
    let scroll_result = container.end(ctx, total_content_h, scroll_widget_theme);
    result.scroll_viewport_rect    = Some(viewport_rect);
    result.scroll_content_height   = scroll_result.content_height;
    result.scrollbar_handle_rect   = scroll_result.handle_rect;
    result.scrollbar_track_rect    = scroll_result.track_rect;
    result.scroll_viewport_height  = scroll_result.viewport_height;
}

// =============================================================================
// MAP section renderer — unified minimap with tag-colored leaves
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_map_section(
    ctx: &mut dyn RenderContext,
    overlay_state: &OverlaySettingsState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    theme_manager: &ThemeManager,
    panel_grid: &ChartPanelGrid,
    tag_manager: &TagManager,
    leaf_color_tags: &HashMap<LeafId, [f32; 4]>,
    chart_area_w: f64,
    chart_area_h: f64,
    _modal_x: f64,
    content_x: f64,
    content_y: f64,
    content_width: f64,
    _content_height: f64,
    layer_id: &uzor::input::coordinator::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut TagsTabsResult,
) {
    let rt = theme_manager.current();
    let colors = &rt.colors;

    let content_left = content_x + MODAL_PADDING;
    let mut row_y = content_y + MODAL_PADDING;

    let tree = panel_grid.docking().tree();
    let all_leaves: Vec<&Leaf<ChartSubPanel>> = tree.leaves();
    let expanded_leaf_id: Option<LeafId> = if panel_grid.is_expanded() {
        panel_grid.docking().active_leaf()
    } else {
        None
    };

    let panel_w = chart_area_w as f32;
    let panel_h = chart_area_h as f32;

    if panel_w > 0.0 && panel_h > 0.0 {
        let minimap_max_w = content_width - MODAL_PADDING * 2.0;
        let minimap_max_h = 190.0;
        let aspect = panel_w as f64 / panel_h as f64;
        let (minimap_w, minimap_h) = if minimap_max_w / minimap_max_h > aspect {
            (minimap_max_h * aspect, minimap_max_h)
        } else {
            (minimap_max_w, minimap_max_w / aspect)
        };
        let minimap_x = content_x + (content_width - minimap_w) / 2.0;
        let minimap_y = row_y;
        let scale_x = minimap_w / panel_w as f64;
        let scale_y = minimap_h / panel_h as f64;

        // Minimap background
        ctx.set_fill_color(&frame_theme.toolbar_bg);
        ctx.fill_rounded_rect(minimap_x - 2.0, minimap_y - 2.0, minimap_w + 4.0, minimap_h + 4.0, 4.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(minimap_x - 2.0, minimap_y - 2.0, minimap_w + 4.0, minimap_h + 4.0, 4.0);

        // Leaf rectangles
        let leaf_rects = tree.layout_rects(panel_w, panel_h);

        for (_i, (leaf_id, rect)) in leaf_rects.iter().enumerate() {
            let mx = minimap_x + rect.x as f64 * scale_x;
            let my = minimap_y + rect.y as f64 * scale_y;
            let mw = (rect.width as f64 * scale_x).max(2.0);
            let mh = (rect.height as f64 * scale_y).max(2.0);

            let is_selected = overlay_state.selected_node_id == Some(leaf_id.0);
            // Show target highlight only when nothing is manually selected
            let is_target = overlay_state.selected_node_id.is_none()
                && overlay_state.target_leaf_id == Some(*leaf_id);
            let is_expanded = expanded_leaf_id == Some(*leaf_id);
            let wid_hover = format!("tags_tabs:minimap_leaf:{}", leaf_id.0);
            let is_hovered  = overlay_state.hovered_item_id.as_deref() == Some(wid_hover.as_str());

            // Use tag group color if the leaf is tagged, else neutral
            let leaf_color = if let Some(tag_color) = leaf_color_tags.get(leaf_id) {
                let [r, g, b, _a] = tag_color;
                let alpha = if is_selected || is_target { 0.9 } else { 0.6 };
                format!("rgba({},{},{},{:.2})", (*r * 255.0) as u8, (*g * 255.0) as u8, (*b * 255.0) as u8, alpha)
            } else {
                let alpha = if is_selected || is_target { 0.7 } else { 0.3 };
                format!("rgba(100,100,100,{:.2})", alpha)
            };

            ctx.set_fill_color(&leaf_color);
            ctx.fill_rounded_rect(mx, my, mw, mh, 2.0);

            // Border
            if is_selected || is_target {
                ctx.set_stroke_color(&toolbar_theme.item_text_active);
                ctx.set_stroke_width(2.0);
            } else if is_hovered {
                ctx.set_stroke_color(&toolbar_theme.item_text_muted);
                ctx.set_stroke_width(1.5);
            } else {
                ctx.set_stroke_color(&toolbar_theme.separator);
                ctx.set_stroke_width(1.0);
            }
            ctx.stroke_rounded_rect(mx, my, mw, mh, 2.0);

            // Leaf ID label (only if rect is large enough)
            if mw > 30.0 && mh > 16.0 {
                ctx.set_font("10px sans-serif");
                ctx.set_fill_color(if is_selected || is_target { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text });
                ctx.set_text_align(TextAlign::Center);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(&format!("{}", leaf_id.0), mx + mw / 2.0, my + mh / 2.0);
            }

            // Expand indicator (success-colored icon in corner)
            if is_expanded {
                let ind_size = 8.0;
                let ind_x = mx + mw - ind_size - 2.0;
                let ind_y = my + 1.0;
                draw_svg_icon(ctx, Icon::Expand.svg(), ind_x, ind_y, ind_size, ind_size, &colors.success);
            }

            // Register clickable zone
            let r = WidgetRect::new(mx, my, mw, mh);
            result.content_items.push((wid_hover.clone(), r));
            input_coordinator.register_on_layer(wid_hover.as_str(), r, Sense::CLICK, layer_id);
        }

        // Branch border overlays
        let branch_rects_data = tree.branch_rects(panel_w, panel_h);

        // Determine which branch is parent of the hovered leaf
        let active_branch_id: Option<BranchId> = overlay_state.hovered_item_id.as_deref()
            .and_then(|hid| hid.strip_prefix("tags_tabs:minimap_leaf:"))
            .and_then(|id_str| id_str.parse::<u64>().ok())
            .and_then(|lid| {
                tree.find_parent_of_leaf(LeafId(lid)).map(|b| b.id)
            });

        let branch_colors: &[&str] = &["#60a5fa", "#a78bfa", "#34d399", "#fbbf24"];

        for (branch_id, rect, depth) in &branch_rects_data {
            let bx = minimap_x + rect.x as f64 * scale_x;
            let by = minimap_y + rect.y as f64 * scale_y;
            let bw = (rect.width as f64 * scale_x).max(2.0);
            let bh = (rect.height as f64 * scale_y).max(2.0);

            let color_idx = (depth.saturating_sub(1)) % branch_colors.len();
            let color = branch_colors[color_idx];
            let is_active = active_branch_id == Some(*branch_id);

            if is_active {
                ctx.set_stroke_color(color);
                ctx.set_stroke_width(2.0);
            } else {
                ctx.set_stroke_color(&format!("{}66", color));
                ctx.set_stroke_width(1.0);
            }
            ctx.stroke_rounded_rect(bx - 1.0, by - 1.0, bw + 2.0, bh + 2.0, 3.0);

            if bw > 20.0 && bh > 18.0 {
                ctx.set_font("10px sans-serif");
                let label_color = if is_active {
                    color.to_string()
                } else {
                    format!("{}99", color)
                };
                ctx.set_fill_color(&label_color);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Bottom);
                ctx.fill_text(&format!("{}", branch_id.0), bx + 2.0, by + bh - 2.0);
            }
        }

        row_y += minimap_h + MODAL_PADDING;

        // ── Legend — show group colors ────────────────────────────────────
        let mut groups: Vec<_> = tag_manager.groups().collect();
        groups.sort_by_key(|g| g.id.0);
        if !groups.is_empty() {
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            let mut legend_x = content_left;
            for group in &groups {
                let [r, g, b, a] = group.color;
                let color_str = format!("rgba({},{},{},{:.2})", (r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, a);
                // Swatch
                ctx.set_fill_color(&color_str);
                ctx.fill_rounded_rect(legend_x, row_y, 10.0, 10.0, 2.0);
                // Label
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                let label = format!("G{} ({})", group.id.0, group.members.len());
                ctx.fill_text(&label, legend_x + 13.0, row_y + 5.0);
                let label_w = ctx.measure_text(&label);
                legend_x += label_w + 22.0;
                // Wrap to next line if too wide
                if legend_x > content_x + content_width - MODAL_PADDING {
                    legend_x = content_left;
                    row_y += 16.0;
                }
            }
            row_y += 16.0;
        }

        // ── Hidden panels list ──────────────────────────────────────────────
        let hidden_leaves_map: Vec<&Leaf<ChartSubPanel>> = all_leaves.iter()
            .filter(|l| l.hidden)
            .copied()
            .collect();
        if !hidden_leaves_map.is_empty() {
            ctx.set_font("11px sans-serif");
            ctx.set_fill_color(&toolbar_theme.item_text_muted);
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("Hidden:", content_left, row_y + 10.0);
            row_y += 20.0;

            for leaf in &hidden_leaves_map {
                let leaf_id_val = leaf.id.0;
                let title = leaf.active_panel().map(|p| p.title.clone()).unwrap_or_else(|| "Empty".to_string());
                let short = if title.chars().count() > 20 {
                    format!("{}...", &title[..title.char_indices().nth(17).map(|(b, _)| b).unwrap_or(title.len())])
                } else {
                    title
                };

                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(&format!("Leaf {} - {}", leaf_id_val, short), content_left, row_y + 10.0);

                // Show button
                let btn_label = "Show";
                ctx.set_font("11px sans-serif");
                let btn_w = ctx.measure_text(btn_label) + 12.0;
                let btn_h = 18.0;
                let btn_x = content_x + content_width - MODAL_PADDING - btn_w;
                let btn_y = row_y + 1.0;
                let wid = format!("tags_tabs:show:{}", leaf_id_val);
                let is_hovered = overlay_state.hovered_item_id.as_deref() == Some(wid.as_str());
                let btn_bg = if is_hovered { colors.success.as_str() } else { toolbar_theme.item_bg_hover.as_str() };
                let btn_border = if is_hovered { colors.success.as_str() } else { toolbar_theme.separator.as_str() };
                let btn_text_color = if is_hovered { toolbar_theme.item_text_active.as_str() } else { toolbar_theme.item_text.as_str() };
                ctx.set_fill_color(btn_bg);
                ctx.fill_rounded_rect(btn_x, btn_y, btn_w, btn_h, 3.0);
                ctx.set_stroke_color(btn_border);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rounded_rect(btn_x, btn_y, btn_w, btn_h, 3.0);
                ctx.set_fill_color(btn_text_color);
                ctx.set_text_align(TextAlign::Center);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(btn_label, btn_x + btn_w / 2.0, btn_y + btn_h / 2.0);

                let r = WidgetRect::new(btn_x, btn_y, btn_w, btn_h);
                result.content_items.push((wid.clone(), r));
                input_coordinator.register_on_layer(wid.as_str(), r, Sense::CLICK, layer_id);

                row_y += 22.0;
            }
        }

        // ── Action buttons for selected leaf ──────────────────────────────
        if let Some(selected_id) = overlay_state.selected_node_id {
            let leaf_id = LeafId(selected_id);
            if let Some(leaf) = tree.leaf(leaf_id) {
                let is_hidden       = leaf.hidden;
                let is_expanded_sel = expanded_leaf_id == Some(leaf_id);
                let can_delete      = all_leaves.len() > 1;
                let can_hide        = tree.visible_leaf_count() > 1 || is_hidden;

                // Info row
                let title = leaf.active_panel().map(|p| p.title.clone()).unwrap_or_else(|| "-".to_string());
                let short = if title.chars().count() > 30 {
                    format!("{}...", &title[..title.char_indices().nth(27).map(|(b, _)| b).unwrap_or(title.len())])
                } else {
                    title
                };
                ctx.set_fill_color(&toolbar_theme.item_text);
                ctx.set_font("12px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(
                    &format!("Leaf {} - {}", selected_id, short),
                    content_left,
                    row_y + ROW_HEIGHT / 2.0,
                );
                row_y += ROW_HEIGHT + 4.0;

                // ── Row 1: Panel management buttons ──
                let btn_h = 24.0;
                let btn_gap = 6.0;

                // "TABS" section label
                ctx.set_font("10px sans-serif");
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("TABS", content_left, row_y + 6.0);
                row_y += 14.0;

                let mut btn_x = content_left;

                // Expand/Collapse button
                if !is_hidden {
                    let btn_label = if is_expanded_sel { "Collapse" } else { "Expand" };
                    ctx.set_font("12px sans-serif");
                    let btn_w = ctx.measure_text(btn_label) + 16.0;
                    let wid = format!("tags_tabs:expand:{}", selected_id);
                    let is_hovered = overlay_state.hovered_item_id.as_deref() == Some(wid.as_str());
                    // Expand: keep special expanded styling (success color) regardless of hover
                    let bg = if is_expanded_sel {
                        if is_hovered { colors.success.as_str() } else { toolbar_theme.item_bg_active.as_str() }
                    } else if is_hovered {
                        toolbar_theme.item_bg_hover.as_str()
                    } else {
                        toolbar_theme.background.as_str()
                    };
                    let border = if is_expanded_sel { colors.success.as_str() } else { toolbar_theme.separator.as_str() };
                    let text_col = if is_expanded_sel {
                        if is_hovered { toolbar_theme.item_text_active.as_str() } else { colors.success.as_str() }
                    } else {
                        toolbar_theme.item_text.as_str()
                    };
                    ctx.set_fill_color(bg);
                    ctx.fill_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                    ctx.set_stroke_color(border);
                    ctx.set_stroke_width(1.0);
                    ctx.stroke_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                    ctx.set_fill_color(text_col);
                    ctx.set_text_align(TextAlign::Center);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(btn_label, btn_x + btn_w / 2.0, row_y + btn_h / 2.0);

                    let r = WidgetRect::new(btn_x, row_y, btn_w, btn_h);
                    result.content_items.push((wid.clone(), r));
                    input_coordinator.register_on_layer(wid.as_str(), r, Sense::CLICK, layer_id);
                    btn_x += btn_w + btn_gap;
                }

                // Hide/Show button
                if can_hide {
                    let btn_label = if is_hidden { "Show" } else { "Hide" };
                    ctx.set_font("12px sans-serif");
                    let btn_w = ctx.measure_text(btn_label) + 16.0;
                    let wid = if is_hidden {
                        format!("tags_tabs:show:{}", selected_id)
                    } else {
                        format!("tags_tabs:hide:{}", selected_id)
                    };
                    let is_hovered = overlay_state.hovered_item_id.as_deref() == Some(wid.as_str());
                    let bg = if is_hovered { toolbar_theme.item_bg_hover.as_str() } else { toolbar_theme.background.as_str() };
                    ctx.set_fill_color(bg);
                    ctx.fill_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                    ctx.set_stroke_color(&toolbar_theme.separator);
                    ctx.set_stroke_width(1.0);
                    ctx.stroke_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                    ctx.set_fill_color(&toolbar_theme.item_text);
                    ctx.set_text_align(TextAlign::Center);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(btn_label, btn_x + btn_w / 2.0, row_y + btn_h / 2.0);

                    let r = WidgetRect::new(btn_x, row_y, btn_w, btn_h);
                    result.content_items.push((wid.clone(), r));
                    input_coordinator.register_on_layer(wid.as_str(), r, Sense::CLICK, layer_id);
                    btn_x += btn_w + btn_gap;
                }

                // Split H button (only if not hidden)
                if !is_hidden {
                    let btn_label = "Split H";
                    ctx.set_font("12px sans-serif");
                    let btn_w = ctx.measure_text(btn_label) + 16.0;
                    let wid = format!("tags_tabs:split_h:{}", selected_id);
                    let is_hovered = overlay_state.hovered_item_id.as_deref() == Some(wid.as_str());
                    let bg = if is_hovered { toolbar_theme.item_bg_hover.as_str() } else { toolbar_theme.background.as_str() };
                    ctx.set_fill_color(bg);
                    ctx.fill_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                    ctx.set_stroke_color(&toolbar_theme.separator);
                    ctx.set_stroke_width(1.0);
                    ctx.stroke_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                    ctx.set_fill_color(&toolbar_theme.item_text);
                    ctx.set_text_align(TextAlign::Center);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(btn_label, btn_x + btn_w / 2.0, row_y + btn_h / 2.0);

                    let r = WidgetRect::new(btn_x, row_y, btn_w, btn_h);
                    result.content_items.push((wid.clone(), r));
                    input_coordinator.register_on_layer(wid.as_str(), r, Sense::CLICK, layer_id);
                    btn_x += btn_w + btn_gap;
                }

                // Split V button (only if not hidden)
                if !is_hidden {
                    let btn_label = "Split V";
                    ctx.set_font("12px sans-serif");
                    let btn_w = ctx.measure_text(btn_label) + 16.0;
                    let wid = format!("tags_tabs:split_v:{}", selected_id);
                    let is_hovered = overlay_state.hovered_item_id.as_deref() == Some(wid.as_str());
                    let bg = if is_hovered { toolbar_theme.item_bg_hover.as_str() } else { toolbar_theme.background.as_str() };
                    ctx.set_fill_color(bg);
                    ctx.fill_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                    ctx.set_stroke_color(&toolbar_theme.separator);
                    ctx.set_stroke_width(1.0);
                    ctx.stroke_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                    ctx.set_fill_color(&toolbar_theme.item_text);
                    ctx.set_text_align(TextAlign::Center);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(btn_label, btn_x + btn_w / 2.0, row_y + btn_h / 2.0);

                    let r = WidgetRect::new(btn_x, row_y, btn_w, btn_h);
                    result.content_items.push((wid.clone(), r));
                    input_coordinator.register_on_layer(wid.as_str(), r, Sense::CLICK, layer_id);
                    btn_x += btn_w + btn_gap;
                }

                // Delete button — LAST in row, only Delete hovers red
                if can_delete {
                    let btn_label = "Delete";
                    ctx.set_font("12px sans-serif");
                    let btn_w = ctx.measure_text(btn_label) + 16.0;
                    let wid = format!("tags_tabs:eliminate:{}", selected_id);
                    let is_hovered = overlay_state.hovered_item_id.as_deref() == Some(wid.as_str());
                    let bg = if is_hovered { colors.danger.as_str() } else { toolbar_theme.background.as_str() };
                    let border = if is_hovered { colors.danger.as_str() } else { toolbar_theme.separator.as_str() };
                    let text_col = if is_hovered { toolbar_theme.item_text_active.as_str() } else { toolbar_theme.item_text.as_str() };
                    ctx.set_fill_color(bg);
                    ctx.fill_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                    ctx.set_stroke_color(border);
                    ctx.set_stroke_width(1.0);
                    ctx.stroke_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                    ctx.set_fill_color(text_col);
                    ctx.set_text_align(TextAlign::Center);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(btn_label, btn_x + btn_w / 2.0, row_y + btn_h / 2.0);

                    let r = WidgetRect::new(btn_x, row_y, btn_w, btn_h);
                    result.content_items.push((wid.clone(), r));
                    input_coordinator.register_on_layer(wid.as_str(), r, Sense::CLICK, layer_id);
                }

                row_y += btn_h + btn_gap;

                // ── Row 2: Tag management buttons ──
                // "TAGS" section label
                ctx.set_font("10px sans-serif");
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("TAGS", content_left, row_y + 6.0);
                row_y += 14.0;

                btn_x = content_left;

                // Tag button — open sync color grid for this leaf
                {
                    let btn_label = "Tag";
                    ctx.set_font("12px sans-serif");
                    let btn_w = ctx.measure_text(btn_label) + 16.0;
                    let wid = format!("tags_tabs:tag:{}", selected_id);
                    let is_hovered = overlay_state.hovered_item_id.as_deref() == Some(wid.as_str());
                    let bg = if is_hovered { toolbar_theme.item_bg_hover.as_str() } else { toolbar_theme.background.as_str() };
                    let border = &toolbar_theme.separator;
                    let text_col = toolbar_theme.item_text.as_str();
                    ctx.set_fill_color(bg);
                    ctx.fill_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                    ctx.set_stroke_color(border);
                    ctx.set_stroke_width(1.0);
                    ctx.stroke_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                    ctx.set_fill_color(text_col);
                    ctx.set_text_align(TextAlign::Center);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(btn_label, btn_x + btn_w / 2.0, row_y + btn_h / 2.0);

                    let r = WidgetRect::new(btn_x, row_y, btn_w, btn_h);
                    result.content_items.push((wid.clone(), r));
                    input_coordinator.register_on_layer(wid.as_str(), r, Sense::CLICK, layer_id);
                    btn_x += btn_w + btn_gap;
                }

                // Untag button — only if this leaf has a color tag
                if leaf_color_tags.contains_key(&leaf_id) {
                    let btn_label = "Untag";
                    ctx.set_font("12px sans-serif");
                    let btn_w = ctx.measure_text(btn_label) + 16.0;
                    let wid = format!("tags_tabs:untag:{}", selected_id);
                    let is_hovered = overlay_state.hovered_item_id.as_deref() == Some(wid.as_str());
                    let bg = if is_hovered { toolbar_theme.item_bg_hover.as_str() } else { toolbar_theme.background.as_str() };
                    let border = &toolbar_theme.separator;
                    let text_col = toolbar_theme.item_text.as_str();
                    ctx.set_fill_color(bg);
                    ctx.fill_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                    ctx.set_stroke_color(border);
                    ctx.set_stroke_width(1.0);
                    ctx.stroke_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                    ctx.set_fill_color(text_col);
                    ctx.set_text_align(TextAlign::Center);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(btn_label, btn_x + btn_w / 2.0, row_y + btn_h / 2.0);

                    let r = WidgetRect::new(btn_x, row_y, btn_w, btn_h);
                    result.content_items.push((wid.clone(), r));
                    input_coordinator.register_on_layer(wid.as_str(), r, Sense::CLICK, layer_id);
                }
            }
        } else {
            // No selection — show hint
            ctx.set_fill_color(&toolbar_theme.item_text_muted);
            ctx.set_font("12px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("Select a panel on the map", content_left, row_y + ROW_HEIGHT / 2.0);
        }
    } else {
        // No valid chart area dimensions
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("No panel data available", content_left, row_y + ROW_HEIGHT / 2.0);
    }
}

// =============================================================================
// TAGS section renderer
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_tags_section(
    ctx: &mut dyn RenderContext,
    state: &TagsTabsState,
    overlay_state: &OverlaySettingsState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    theme_manager: &ThemeManager,
    tag_manager: &TagManager,
    content_x: f64,
    content_y: f64,
    content_width: f64,
    content_height: f64,
    tags_scroll: &ScrollState,
    scroll_widget_theme: &WidgetTheme,
    layer_id: &uzor::input::coordinator::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut TagsTabsResult,
) {
    let rt = theme_manager.current();
    let colors = &rt.colors;
    // ---- Sub-tab bar ----
    let sub_tabs: &[(&str, &str, TagsTabsTagsTab)] = &[
        ("groups",  "Groups",  TagsTabsTagsTab::Groups),
        ("details", "Details", TagsTabsTagsTab::Details),
    ];

    let tab_padding_h = 12.0;
    let tab_gap       = 2.0;
    ctx.set_font("12px sans-serif");
    let tab_widths: Vec<f64> = sub_tabs
        .iter()
        .map(|(_, label, _)| ctx.measure_text(label) + tab_padding_h * 2.0)
        .collect();

    let mut tab_x = content_x + MODAL_PADDING;
    for (i, (id, label, variant)) in sub_tabs.iter().enumerate() {
        let tab_w    = tab_widths[i];
        let tab_rect = WidgetRect::new(tab_x, content_y, tab_w, SUB_TAB_HEIGHT);
        let is_active = state.tags_tab == *variant;

        if is_active {
            ctx.set_fill_color(&toolbar_theme.accent);
            ctx.fill_rounded_rect(tab_rect.x, tab_rect.y, tab_rect.width, tab_rect.height, 3.0);
        }

        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(
            if is_active { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text },
        );
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(
            label,
            tab_rect.x + tab_rect.width / 2.0,
            tab_rect.y + tab_rect.height / 2.0,
        );

        let wid = format!("tags_tabs:tags_tab:{}", id);
        result.sub_tab_rects.push((wid.clone(), tab_rect));
        input_coordinator.register_on_layer(wid.as_str(), tab_rect, Sense::CLICK, layer_id);

        tab_x += tab_w + tab_gap;
    }

    // Sub-tab bar bottom border
    ctx.set_stroke_color(&frame_theme.toolbar_border);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(content_x, content_y + SUB_TAB_HEIGHT);
    ctx.line_to(content_x + content_width, content_y + SUB_TAB_HEIGHT);
    ctx.stroke();

    let inner_top = content_y + SUB_TAB_HEIGHT;
    let inner_left = content_x + MODAL_PADDING;

    // Scrollable viewport covers the area below the sub-tab bar
    let scroll_viewport_h = content_height - SUB_TAB_HEIGHT;
    let viewport_rect = WidgetRect::new(content_x, inner_top, content_width, scroll_viewport_h);

    let container = ScrollableContainer::new(viewport_rect, tags_scroll, None);
    container.begin(ctx);
    let content_y_start = container.content_y();
    let mut row_y = content_y_start + MODAL_PADDING;

    match state.tags_tab {
        // ------------------------------------------------------------------ //
        // Groups tab
        // ------------------------------------------------------------------ //
        TagsTabsTagsTab::Groups => {
            // Collect groups — sort by id for stable ordering
            let mut groups: Vec<_> = tag_manager.groups().collect();
            groups.sort_by_key(|g| g.id.0);

            if groups.is_empty() {
                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(
                    "No sync groups",
                    inner_left,
                    row_y + ROW_HEIGHT / 2.0,
                );
            } else {

                for group in &groups {
                    let gid = group.id.0;
                    let member_count = group.members.len();

                    // Group indicators AND primitives by symbol
                    let symbol_counts: Vec<(String, usize, usize)> = {
                        // (symbol) -> (ind_count, prim_count)
                        let mut map: std::collections::HashMap<String, (usize, usize)> = std::collections::HashMap::new();
                        // Always include current group symbol
                        if !group.symbol.is_empty() {
                            map.insert(group.symbol.clone(), (0, 0));
                        }
                        for cfg in &group.indicator_configs {
                            map.entry(cfg.symbol.clone()).or_insert((0, 0)).0 += 1;
                        }
                        for prim in &group.primitives {
                            let sym = &prim.data().symbol;
                            map.entry(sym.clone()).or_insert((0, 0)).1 += 1;
                        }
                        let mut pairs: Vec<_> = map.into_iter()
                            .map(|(sym, (ic, pc))| (sym, ic, pc))
                            .collect();
                        // Current symbol first, then alphabetical
                        pairs.sort_by(|a, b| {
                            let a_current = a.0 == group.symbol;
                            let b_current = b.0 == group.symbol;
                            b_current.cmp(&a_current).then(a.0.cmp(&b.0))
                        });
                        pairs
                    };

                    let row_h = ROW_HEIGHT * 2.0 + 4.0; // Two-line row
                    let row_rect = WidgetRect::new(content_x, row_y, content_width, row_h);

                    // Row background — highlight if selected
                    let is_selected = state.selected_group_id == Some(group.id);
                    if is_selected {
                        ctx.set_fill_color(&crate::apply_opacity(&toolbar_theme.accent, 0.15));
                        ctx.fill_rounded_rect(content_x + 2.0, row_y, content_width - 4.0, row_h, 3.0);
                    }

                    // Color swatch
                    let swatch_x = inner_left;
                    let swatch_y = row_y + 8.0;
                    let [r, g, b, a] = group.color;
                    let swatch_color = format!(
                        "rgba({},{},{},{:.2})",
                        (r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, a,
                    );
                    ctx.set_fill_color(&swatch_color);
                    ctx.fill_rounded_rect(swatch_x, swatch_y, SWATCH_SIZE, SWATCH_SIZE, 2.0);
                    ctx.set_stroke_color(&toolbar_theme.separator);
                    ctx.set_stroke_width(1.0);
                    ctx.stroke_rounded_rect(swatch_x, swatch_y, SWATCH_SIZE, SWATCH_SIZE, 2.0);

                    // Line 1: Group ID + members
                    ctx.set_font("bold 12px sans-serif");
                    ctx.set_fill_color(&toolbar_theme.item_text);
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(
                        &format!("Group {}   {} members", gid, member_count),
                        swatch_x + SWATCH_SIZE + 6.0,
                        row_y + ROW_HEIGHT / 2.0,
                    );

                    // Line 2: Per-symbol indicator + primitive breakdown
                    ctx.set_font("11px sans-serif");
                    ctx.set_fill_color(&toolbar_theme.item_text_muted);
                    let symbol_parts: Vec<String> = symbol_counts.iter()
                        .map(|(sym, ind_c, prim_c)| {
                            let s = if sym.is_empty() { "-" } else { sym.as_str() };
                            format!("{}: {} inds {} prims", s, ind_c, prim_c)
                        })
                        .collect();
                    let line2 = if symbol_parts.is_empty() {
                        "No data".to_string()
                    } else {
                        symbol_parts.join("  ")
                    };
                    ctx.fill_text(
                        &line2,
                        swatch_x + SWATCH_SIZE + 6.0,
                        row_y + ROW_HEIGHT + ROW_HEIGHT / 2.0 - 2.0,
                    );

                    // Delete button (position to right, vertically centered in the 2-line row)
                    let del_w = 52.0;
                    let del_h = 22.0;
                    let del_x = content_x + content_width - MODAL_PADDING - del_w;
                    let del_y = row_y + (row_h - del_h) / 2.0;
                    let del_wid = format!("tags_tabs:tags:delete_group:{}", gid);
                    let is_del_hovered = overlay_state.hovered_item_id.as_deref() == Some(del_wid.as_str());
                    let del_bg = if is_del_hovered { colors.danger.as_str() } else { toolbar_theme.item_bg_hover.as_str() };
                    ctx.set_fill_color(del_bg);
                    ctx.fill_rounded_rect(del_x, del_y, del_w, del_h, 3.0);
                    ctx.set_font("11px sans-serif");
                    let del_text_color = if is_del_hovered { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text_muted };
                    ctx.set_fill_color(del_text_color);
                    ctx.set_text_align(TextAlign::Center);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text("Delete", del_x + del_w / 2.0, del_y + del_h / 2.0);
                    let del_rect = WidgetRect::new(del_x, del_y, del_w, del_h);
                    result.content_items.push((del_wid.clone(), del_rect));
                    input_coordinator.register_on_layer(del_wid.as_str(), del_rect, Sense::CLICK, layer_id);

                    // Row click → select group
                    let select_wid = format!("tags_tabs:tags:select_group:{}", gid);
                    result.content_items.push((select_wid.clone(), row_rect));
                    input_coordinator.register_on_layer(select_wid.as_str(), row_rect, Sense::CLICK, layer_id);

                    row_y += row_h + ROW_GAP;
                }
            }
        }

        // ------------------------------------------------------------------ //
        // Details tab
        // ------------------------------------------------------------------ //
        TagsTabsTagsTab::Details => {
            if let Some(group_id) = state.selected_group_id {
                if let Some(group) = tag_manager.group(group_id) {

                    // Group title + color swatch
                    let [r, g, b, a] = group.color;
                    let swatch_color = format!(
                        "rgba({},{},{},{:.2})",
                        (r * 255.0) as u8,
                        (g * 255.0) as u8,
                        (b * 255.0) as u8,
                        a,
                    );
                    let swatch_x = inner_left;
                    let swatch_y = row_y + (ROW_HEIGHT - SWATCH_SIZE) / 2.0;
                    ctx.set_fill_color(&swatch_color);
                    ctx.fill_rounded_rect(swatch_x, swatch_y, SWATCH_SIZE, SWATCH_SIZE, 2.0);
                    ctx.set_stroke_color(&toolbar_theme.separator);
                    ctx.set_stroke_width(1.0);
                    ctx.stroke_rounded_rect(swatch_x, swatch_y, SWATCH_SIZE, SWATCH_SIZE, 2.0);

                    let symbol_str_det = if group.symbol.is_empty() { "-" } else { &group.symbol };
                    let tf_name = &group.timeframe.name;
                    ctx.set_font("bold 12px sans-serif");
                    ctx.set_fill_color(&toolbar_theme.item_text);
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(
                        &format!("Group {}  [{}]  {}  ({} members)", group.id.0, symbol_str_det, tf_name, group.members.len()),
                        swatch_x + SWATCH_SIZE + 6.0,
                        row_y + ROW_HEIGHT / 2.0,
                    );
                    row_y += ROW_HEIGHT + ROW_GAP;

                    // Sync flags
                    let gid = group.id.0;
                    let flags = [
                        ("sync_crosshair", "Crosshair",  group.sync_flags.sync_crosshair),
                        ("sync_viewport",  "Viewport",   group.sync_flags.sync_viewport),
                        ("sync_symbol",    "Symbol",     group.sync_flags.sync_symbol),
                        ("sync_timeframe", "Timeframe",  group.sync_flags.sync_timeframe),
                        ("sync_drawings",  "Drawings",   group.sync_flags.sync_drawings),
                        ("sync_indicators","Indicators", group.sync_flags.sync_indicators),
                    ];

                    for (flag_id, flag_label, flag_val) in &flags {
                        // Label
                        ctx.set_font("12px sans-serif");
                        ctx.set_fill_color(&toolbar_theme.item_text);
                        ctx.set_text_align(TextAlign::Left);
                        ctx.set_text_baseline(TextBaseline::Middle);
                        ctx.fill_text(flag_label, inner_left, row_y + ROW_HEIGHT / 2.0);

                        // Toggle button
                        let toggle_w = 44.0;
                        let toggle_h = 20.0;
                        let toggle_x = content_x + content_width - MODAL_PADDING - toggle_w;
                        let toggle_y = row_y + (ROW_HEIGHT - toggle_h) / 2.0;
                        let toggle_bg_str;
                        let toggle_bg = if *flag_val { toggle_bg_str = toolbar_theme.accent.clone(); toggle_bg_str.as_str() } else { toolbar_theme.item_bg_hover.as_str() };
                        ctx.set_fill_color(toggle_bg);
                        ctx.fill_rounded_rect(toggle_x, toggle_y, toggle_w, toggle_h, toggle_h / 2.0);

                        // Toggle knob
                        let knob_r = toggle_h / 2.0 - 2.0;
                        let knob_x = if *flag_val {
                            toggle_x + toggle_w - knob_r - 3.0
                        } else {
                            toggle_x + knob_r + 3.0
                        };
                        let knob_y = toggle_y + toggle_h / 2.0;
                        ctx.set_fill_color(&toolbar_theme.item_text_active);
                        ctx.begin_path();
                        ctx.arc(knob_x, knob_y, knob_r, 0.0, std::f64::consts::TAU);
                        ctx.fill();

                        // Register toggle interaction
                        let wid = format!("tags_tabs:tags:toggle_flag:{}:{}", gid, flag_id);
                        let toggle_rect = WidgetRect::new(toggle_x, toggle_y, toggle_w, toggle_h);
                        result.content_items.push((wid.clone(), toggle_rect));
                        input_coordinator.register_on_layer(
                            wid.as_str(),
                            toggle_rect,
                            Sense::CLICK,
                            layer_id,
                        );

                        row_y += ROW_HEIGHT + ROW_GAP;
                    }
                } else {
                    // Group was deleted between render and state update
                    ctx.set_font("12px sans-serif");
                    ctx.set_fill_color(&toolbar_theme.item_text_muted);
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(
                        "Group not found",
                        inner_left,
                        row_y + ROW_HEIGHT / 2.0,
                    );
                }
            } else {
                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(
                    "Select a group from Groups tab",
                    inner_left,
                    row_y + ROW_HEIGHT / 2.0,
                );
            }
        }
    }

    // Close the scrollable container and write scroll results
    let total_content_h = row_y - content_y_start;
    let scroll_result = container.end(ctx, total_content_h, scroll_widget_theme);
    result.scroll_viewport_rect    = Some(viewport_rect);
    result.scroll_content_height   = scroll_result.content_height;
    result.scrollbar_handle_rect   = scroll_result.handle_rect;
    result.scrollbar_track_rect    = scroll_result.track_rect;
    result.scroll_viewport_height  = scroll_result.viewport_height;
}
