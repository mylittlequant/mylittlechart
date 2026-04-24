//! Overlay Settings modal renderer — 4-tab panel tree manager.
//!
//! Opens when the user clicks the gear icon on an overlay tab header.
//! Shows the chart's internal split-panel tree with tabs for:
//!   1. TreeView  — hierarchical tree with inline action buttons
//!   2. Eliminate — flat list with delete buttons
//!   3. Hidden    — list of hidden leaves with restore buttons
//!   4. Minimap   — scaled visual layout with clickable rects

use crate::engine::render::RenderContext;
use crate::engine::render::draw_svg_icon;
use crate::layout::render_chart::FrameTheme;
use crate::ui::modal_settings::{OverlaySettingsState, OverlayPanelTreeTab};
use crate::ui::toolbar_render::ToolbarTheme;
use crate::ui::widgets::{
    render_modal_frame_only, ModalTheme,
};
use crate::ui::Icon;
use crate::ui::z_order::ZLayer;
use crate::render::{TextAlign, TextBaseline};
use crate::state::panel_grid::ChartPanelGrid;
use crate::state::sub_panel::ChartSubPanel;
use crate::theme::ThemeManager;
use uzor::types::Rect as WidgetRect;
use uzor::input::Sense;
use uzor::panels::{LeafId, BranchId, PanelNode, Leaf};

// =============================================================================
// Result type
// =============================================================================

/// Hit-test rectangles returned by [`render_overlay_settings_modal`].
#[derive(Clone, Debug, Default)]
pub struct OverlaySettingsResult {
    /// The full modal frame (for click-outside detection).
    pub modal_rect: WidgetRect,
    /// The title-bar / header area (for drag initiation).
    pub header_rect: WidgetRect,
    /// The close button rect (for hit-testing).
    pub close_btn_rect: Option<WidgetRect>,
    // --- new fields ---
    /// Tab rects: (tab_id, rect) pairs for all visible tabs.
    pub tab_rects: Vec<(String, WidgetRect)>,
    /// Content area rect (below tab bar).
    pub content_rect: WidgetRect,
    /// All content item rects: (widget_id, rect) pairs.
    pub content_items: Vec<(String, WidgetRect)>,
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
// Helper: render tree nodes recursively (Tab 1 — TreeView)
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
    result: &mut OverlaySettingsResult,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
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
        let is_target   = child.leaf_id().map(|lid| Some(lid) == target_leaf_id).unwrap_or(false);
        let is_selected = Some(node_id) == selected_node_id;

        // Highlight background for selected/target row
        if is_target || is_selected {
            let color = if is_target { &toolbar_theme.accent } else { &toolbar_theme.item_bg_active };
            ctx.set_fill_color(color);
            ctx.fill_rounded_rect(content_left, *row_y, modal_width - modal_padding * 2.0, row_height, 3.0);
        }

        // Label
        let label = match child {
            PanelNode::Leaf(l) => {
                let title = l.active_panel().map(|p| p.title.clone()).unwrap_or_else(|| "Empty".to_string());
                let short = if title.chars().count() > 30 {
                    format!("{}...", &title[..title.char_indices().nth(27).map(|(b, _)| b).unwrap_or(title.len())])
                } else {
                    title
                };
                let hidden_mark = if l.hidden { " [hidden]" } else { "" };
                format!("├─ Leaf {} — {}{}", l.id.0, short, hidden_mark)
            }
            PanelNode::Branch(b) => {
                format!("├─ Branch {} [{} children, {:?}]", b.id.0, b.children.len(), b.layout)
            }
        };

        let text_color = if child.is_hidden() { &toolbar_theme.item_text_muted } else { &toolbar_theme.item_text };
        ctx.set_fill_color(text_color);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&label, x, *row_y + row_height / 2.0);

        // Row click-to-select
        let widget_id = format!("overlay_settings:select:{}", node_id);
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
                ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                ctx.fill_rounded_rect(btn_x, btn_y, btn_size, btn_size, 3.0);
                let icon_x = btn_x + (btn_size - icon_size) / 2.0;
                let icon_y = btn_y + (btn_size - icon_size) / 2.0;
                draw_svg_icon(ctx, Icon::Close.svg(), icon_x, icon_y, icon_size, icon_size, &colors.danger);
                let wid = format!("overlay_settings:eliminate:{}", l.id.0);
                let r = WidgetRect::new(btn_x, btn_y, btn_size, btn_size);
                result.content_items.push((wid.clone(), r));
                input_coordinator.register_on_layer(wid.as_str(), r, Sense::CLICK, layer_id);
            }

            // Button 2: Hide/Show [EyeOff/Eye icon] — only if >1 visible or already hidden
            let can_hide = visible_leaf_count > 1 || l.hidden;
            if can_hide {
                let btn_x = right_edge - btn_size * 2.0 - btn_gap;
                let bg = if l.hidden { &toolbar_theme.item_bg_hover } else { &toolbar_theme.accent };
                ctx.set_fill_color(bg);
                ctx.fill_rounded_rect(btn_x, btn_y, btn_size, btn_size, 3.0);
                let icon_color = if l.hidden { &toolbar_theme.item_text_muted } else { &toolbar_theme.item_text_active };
                let icon_x = btn_x + (btn_size - icon_size) / 2.0;
                let icon_y = btn_y + (btn_size - icon_size) / 2.0;
                let icon = if l.hidden { Icon::EyeOff } else { Icon::Eye };
                draw_svg_icon(ctx, icon.svg(), icon_x, icon_y, icon_size, icon_size, icon_color);
                let wid = if l.hidden {
                    format!("overlay_settings:show:{}", l.id.0)
                } else {
                    format!("overlay_settings:hide:{}", l.id.0)
                };
                let r = WidgetRect::new(btn_x, btn_y, btn_size, btn_size);
                result.content_items.push((wid.clone(), r));
                input_coordinator.register_on_layer(wid.as_str(), r, Sense::CLICK, layer_id);
            }

            // Button 1 (leftmost): Expand/Suppress [Collapse/Expand icon] — only if not hidden
            if !l.hidden {
                let btn_x = right_edge - btn_size * 3.0 - btn_gap * 2.0;
                let is_exp = expanded_leaf_id == Some(l.id);
                let bg = if is_exp { &toolbar_theme.item_bg_active } else { &toolbar_theme.background };
                ctx.set_fill_color(bg);
                ctx.fill_rounded_rect(btn_x, btn_y, btn_size, btn_size, 3.0);
                let icon_color = if is_exp { &colors.success } else { &toolbar_theme.item_text_muted };
                let icon_x = btn_x + (btn_size - icon_size) / 2.0;
                let icon_y = btn_y + (btn_size - icon_size) / 2.0;
                let icon = if is_exp { Icon::Collapse } else { Icon::Expand };
                draw_svg_icon(ctx, icon.svg(), icon_x, icon_y, icon_size, icon_size, icon_color);
                let wid = format!("overlay_settings:expand:{}", l.id.0);
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
                frame_theme, toolbar_theme, theme_manager, result, input_coordinator, layer_id,
                target_leaf_id, selected_node_id, expanded_leaf_id,
                all_leaf_count, visible_leaf_count, modal_x,
            );
        }
    }
}

// =============================================================================
// Renderer
// =============================================================================

/// Render the Overlay Settings / Panel Tree Manager modal.
///
/// # Parameters
/// - `ctx` — render context.
/// - `screen_w` / `screen_h` — full usable screen dimensions (for centering).
/// - `screen_x` / `screen_y` — top-left origin of the usable screen area.
/// - `state` — current overlay settings state (position, drag, tab, selection).
/// - `frame_theme` — chart frame theme (toolbar_bg, toolbar_border, etc.).
/// - `toolbar_theme` — toolbar theme (item colors, accent, background, etc.).
/// - `theme_manager` — theme manager for RuntimeUIColors (danger, success, etc.).
/// - `panel_grid` — the chart panel grid for querying tree structure.
/// - `chart_area_w` / `chart_area_h` — chart content area dimensions (for minimap).
/// - `input_coordinator` — input coordinator for registering interactive zones.
pub fn render_overlay_settings_modal(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    screen_x: f64,
    screen_y: f64,
    state: &OverlaySettingsState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    theme_manager: &ThemeManager,
    panel_grid: &ChartPanelGrid,
    chart_area_w: f64,
    chart_area_h: f64,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> OverlaySettingsResult {
    let rt = theme_manager.current();
    let colors = &rt.colors;
    // =========================================================================
    // Layout constants
    // =========================================================================
    let header_height  = 36.0;
    let tab_height     = 32.0;
    let tab_padding_h  = 12.0;
    let tab_gap        = 2.0;
    let modal_padding  = 12.0;
    let close_btn_size = 20.0;
    let row_height     = 28.0;
    let row_gap        = 6.0;

    // =========================================================================
    // Data gathering
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

    // =========================================================================
    // Dynamic content height
    // =========================================================================
    let content_height = (match state.active_tab {
        OverlayPanelTreeTab::TreeView => {
            (tree_node_count + 1) as f64 * (row_height + row_gap) + modal_padding * 2.0
        }
        OverlayPanelTreeTab::Eliminate => {
            all_leaves.len().max(1) as f64 * (row_height + row_gap) + modal_padding * 2.0
        }
        OverlayPanelTreeTab::Hidden => {
            hidden_leaves.len().max(1) as f64 * (row_height + row_gap) + modal_padding * 2.0
        }
        OverlayPanelTreeTab::Minimap => {
            200.0 + 40.0 + modal_padding * 3.0
        }
    }).clamp(120.0, 400.0);

    // =========================================================================
    // Tab width measurement and modal sizing
    // =========================================================================
    ctx.set_font("13px sans-serif");
    let tab_widths: Vec<f64> = OverlayPanelTreeTab::all().iter()
        .map(|t| ctx.measure_text(t.label()) + tab_padding_h * 2.0)
        .collect();

    let tabs_row_width = tab_widths.iter().sum::<f64>()
        + tab_gap * (OverlayPanelTreeTab::all().len().saturating_sub(1)) as f64
        + modal_padding * 2.0;

    let min_header_width = ctx.measure_text("PANELS SETTINGS") + 16.0 + close_btn_size + 16.0 + 20.0;
    let modal_width = min_header_width.max(tabs_row_width).max(420.0);
    let modal_height = header_height + tab_height + content_height;

    // =========================================================================
    // Position calculation
    // =========================================================================
    let (modal_x, modal_y) = state.position.unwrap_or_else(|| {
        let x = screen_x + (screen_w - modal_width) / 2.0;
        let y = screen_y + (screen_h - modal_height) / 2.0;
        (x, y)
    });

    // Clamp to screen bounds.
    let modal_x = modal_x.max(0.0).min((screen_x + screen_w - modal_width).max(0.0));
    let modal_y = modal_y.max(0.0).min((screen_y + screen_h - modal_height).max(0.0));

    let mut result = OverlaySettingsResult {
        modal_rect: WidgetRect::new(modal_x, modal_y, modal_width, modal_height),
        header_rect: WidgetRect::new(modal_x, modal_y, modal_width, header_height),
        ..OverlaySettingsResult::default()
    };

    // =========================================================================
    // InputCoordinator layer
    // =========================================================================
    let layer_id = ZLayer::Modal.push_named(input_coordinator, "overlay_settings");

    // Register modal background catch-all
    input_coordinator.register_on_layer(
        "overlay_settings:modal_bg",
        WidgetRect::new(modal_x, modal_y, modal_width, modal_height),
        Sense::CLICK,
        &layer_id,
    );

    // Register header drag zone
    input_coordinator.register_on_layer(
        "overlay_settings:header",
        WidgetRect::new(modal_x, modal_y, modal_width, header_height),
        Sense::DRAG,
        &layer_id,
    );

    // =========================================================================
    // Modal frame
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
    // Header
    // =========================================================================
    let title = "PANELS SETTINGS";
    ctx.set_font("14px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(title, modal_x + 16.0, modal_y + header_height / 2.0);

    // Close button
    let close_x = modal_x + modal_width - close_btn_size - 12.0;
    let close_y = modal_y + (header_height - close_btn_size) / 2.0;
    result.close_btn_rect = Some(WidgetRect::new(close_x, close_y, close_btn_size, close_btn_size));
    draw_svg_icon(ctx, Icon::Close.svg(), close_x, close_y, close_btn_size, close_btn_size, &toolbar_theme.item_text);
    input_coordinator.register_on_layer(
        "overlay_settings:close",
        WidgetRect::new(close_x, close_y, close_btn_size, close_btn_size),
        Sense::CLICK,
        &layer_id,
    );

    // Header bottom border
    ctx.set_stroke_color(&frame_theme.toolbar_border);
    ctx.begin_path();
    ctx.move_to(modal_x, modal_y + header_height);
    ctx.line_to(modal_x + modal_width, modal_y + header_height);
    ctx.stroke();

    // =========================================================================
    // Tab bar
    // =========================================================================
    let tab_y = modal_y + header_height;
    let mut tab_x = modal_x + modal_padding;

    for (i, tab) in OverlayPanelTreeTab::all().iter().enumerate() {
        let tab_width = tab_widths[i];
        let tab_rect = WidgetRect::new(tab_x, tab_y, tab_width, tab_height);
        let is_active = state.active_tab == *tab;

        if is_active {
            ctx.set_fill_color(&toolbar_theme.accent);
            ctx.fill_rounded_rect(tab_rect.x, tab_rect.y, tab_rect.width, tab_rect.height, 3.0);
        }

        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(if is_active { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text });
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(tab.label(), tab_rect.x + tab_rect.width / 2.0, tab_rect.y + tab_rect.height / 2.0);

        result.tab_rects.push((tab.id().to_string(), tab_rect));
        let wid = format!("overlay_settings:tab:{}", tab.id());
        input_coordinator.register_on_layer(
            wid.as_str(),
            tab_rect,
            Sense::CLICK,
            &layer_id,
        );

        tab_x += tab_width + tab_gap;
    }

    // Tab bar bottom border
    ctx.set_stroke_color(&frame_theme.toolbar_border);
    ctx.begin_path();
    ctx.move_to(modal_x, tab_y + tab_height);
    ctx.line_to(modal_x + modal_width, tab_y + tab_height);
    ctx.stroke();

    // =========================================================================
    // Content area
    // =========================================================================
    let content_top = modal_y + header_height + tab_height;
    let content_left = modal_x + modal_padding;
    let mut row_y = content_top + modal_padding;

    result.content_rect = WidgetRect::new(modal_x, content_top, modal_width, content_height);

    // Set default text style for content
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);

    match state.active_tab {
        // =================================================================
        // Tab 1: TreeView
        // =================================================================
        OverlayPanelTreeTab::TreeView => {
            // Root row
            let root_text = format!(
                "Root [{}] — {} children, layout: {:?}",
                root.id.0, root.children.len(), root.layout
            );
            ctx.set_fill_color(&toolbar_theme.item_text);
            ctx.set_font("12px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(&root_text, content_left, row_y + row_height / 2.0);

            // Register root row as selectable
            let root_wid = format!("overlay_settings:select:{}", root.id.0);
            let root_rect = WidgetRect::new(content_left, row_y, modal_width - modal_padding * 2.0, row_height);
            result.content_items.push((root_wid.clone(), root_rect));
            input_coordinator.register_on_layer(root_wid.as_str(), root_rect, Sense::CLICK, &layer_id);
            row_y += row_height + row_gap;

            // Recursive tree rendering
            render_tree_nodes(
                ctx, &root.children.clone(), content_left, &mut row_y,
                row_height, row_gap, modal_width, modal_padding,
                16.0,
                frame_theme, toolbar_theme, theme_manager, &mut result, input_coordinator, &layer_id,
                state.target_leaf_id, state.selected_node_id,
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
                let is_target = Some(leaf.id) == state.target_leaf_id;

                // Highlight target leaf row
                if is_target {
                    ctx.set_fill_color(&toolbar_theme.accent);
                    ctx.fill_rounded_rect(content_left, row_y, modal_width - modal_padding * 2.0, row_height, 3.0);
                }

                // Label
                let text_color = if leaf.hidden { &toolbar_theme.item_text_muted } else { &toolbar_theme.item_text };
                ctx.set_fill_color(text_color);
                ctx.set_font("12px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(
                    &format!("Leaf {} — {}", leaf_id_val, short),
                    content_left,
                    row_y + row_height / 2.0,
                );

                // Delete button
                if can_delete {
                    let btn_w = 60.0;
                    let btn_h = row_height - 6.0;
                    let btn_x = modal_x + modal_width - modal_padding - btn_w;
                    let btn_y_inner = row_y + 3.0;

                    let wid_str = format!("overlay_settings:eliminate:{}", leaf_id_val);
                    let is_hovered = state.hovered_item_id.as_deref() == Some(wid_str.as_str());
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
                    input_coordinator.register_on_layer(wid_str.as_str(), r, Sense::CLICK, &layer_id);
                }

                row_y += row_height + row_gap;
            }

            // Empty state
            if all_leaves.is_empty() {
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.set_font("12px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("No panels", content_left, row_y + row_height / 2.0);
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
                ctx.fill_text("No hidden panels", content_left, row_y + row_height / 2.0);
            } else {
                for leaf in &hidden_leaves {
                    let leaf_id_val = leaf.id.0;
                    let title = leaf.active_panel().map(|p| p.title.clone()).unwrap_or_else(|| "Empty".to_string());
                    let short = if title.chars().count() > 25 {
                        format!("{}...", &title[..title.char_indices().nth(22).map(|(b, _)| b).unwrap_or(title.len())])
                    } else {
                        title
                    };

                    ctx.set_fill_color(&toolbar_theme.item_text_muted);
                    ctx.set_font("12px sans-serif");
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(
                        &format!("Leaf {} — {}", leaf_id_val, short),
                        content_left,
                        row_y + row_height / 2.0,
                    );

                    // Restore button
                    let btn_w = 80.0;
                    let btn_h = row_height - 6.0;
                    let btn_x = modal_x + modal_width - modal_padding - btn_w;
                    let btn_y_inner = row_y + 3.0;

                    let wid_str = format!("overlay_settings:restore:{}", leaf_id_val);
                    let is_hovered = state.hovered_item_id.as_deref() == Some(wid_str.as_str());
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
                    input_coordinator.register_on_layer(wid_str.as_str(), r, Sense::CLICK, &layer_id);

                    row_y += row_height + row_gap;
                }
            }
        }

        // =================================================================
        // Tab 4: Minimap
        // =================================================================
        OverlayPanelTreeTab::Minimap => {
            let panel_w = chart_area_w as f32;
            let panel_h = chart_area_h as f32;

            if panel_w > 0.0 && panel_h > 0.0 {
                let minimap_max_w = modal_width - modal_padding * 2.0;
                let minimap_max_h = 190.0;
                let aspect = panel_w as f64 / panel_h as f64;
                let (minimap_w, minimap_h) = if minimap_max_w / minimap_max_h > aspect {
                    (minimap_max_h * aspect, minimap_max_h)
                } else {
                    (minimap_max_w, minimap_max_w / aspect)
                };
                let minimap_x = modal_x + (modal_width - minimap_w) / 2.0;
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

                for (i, (leaf_id, rect)) in leaf_rects.iter().enumerate() {
                    let mx = minimap_x + rect.x as f64 * scale_x;
                    let my = minimap_y + rect.y as f64 * scale_y;
                    let mw = (rect.width as f64 * scale_x).max(2.0);
                    let mh = (rect.height as f64 * scale_y).max(2.0);

                    let is_selected = state.selected_node_id == Some(leaf_id.0);
                    let is_target   = state.target_leaf_id == Some(*leaf_id);
                    let is_expanded = expanded_leaf_id == Some(*leaf_id);
                    let wid_hover = format!("overlay_settings:minimap_leaf:{}", leaf_id.0);
                    let is_hovered  = state.hovered_item_id.as_deref() == Some(wid_hover.as_str());

                    let alpha = if is_selected || is_target { 0.9 } else { 0.5 };
                    let color_idx = i % 6;
                    let leaf_color = match color_idx {
                        0 => format!("rgba(30,90,200,{:.2})", alpha),
                        1 => format!("rgba(120,40,180,{:.2})", alpha),
                        2 => format!("rgba(20,150,100,{:.2})", alpha),
                        3 => format!("rgba(200,120,20,{:.2})", alpha),
                        4 => format!("rgba(180,40,60,{:.2})", alpha),
                        _ => format!("rgba(20,140,180,{:.2})", alpha),
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
                    input_coordinator.register_on_layer(wid_hover.as_str(), r, Sense::CLICK, &layer_id);
                }

                // Branch border overlays
                let branch_rects_data = tree.branch_rects(panel_w, panel_h);

                // Determine which branch is parent of the hovered leaf
                let active_branch_id: Option<BranchId> = state.hovered_item_id.as_deref()
                    .and_then(|hid| hid.strip_prefix("overlay_settings:minimap_leaf:"))
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

                row_y += minimap_h + modal_padding;

                // ── Action buttons for selected leaf ──────────────────────
                if let Some(selected_id) = state.selected_node_id {
                    let leaf_id = LeafId(selected_id);
                    if let Some(leaf) = tree.leaf(leaf_id) {
                        let is_hidden   = leaf.hidden;
                        let is_expanded_sel = expanded_leaf_id == Some(leaf_id);
                        let can_delete  = all_leaves.len() > 1;
                        let can_hide    = tree.visible_leaf_count() > 1 || is_hidden;

                        // Info row
                        let title = leaf.active_panel().map(|p| p.title.clone()).unwrap_or_else(|| "—".to_string());
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
                            &format!("Leaf {} — {}", selected_id, short),
                            content_left,
                            row_y + row_height / 2.0,
                        );
                        row_y += row_height + 4.0;

                        // Action buttons row
                        let btn_h = 24.0;
                        let btn_gap = 6.0;
                        let mut btn_x = content_left;

                        // Expand/Suppress button
                        if !is_hidden {
                            let btn_label = if is_expanded_sel { "Collapse" } else { "Expand" };
                            ctx.set_font("12px sans-serif");
                            let btn_w = ctx.measure_text(btn_label) + 16.0;
                            let bg = if is_expanded_sel { &toolbar_theme.item_bg_active } else { &toolbar_theme.background };
                            ctx.set_fill_color(bg);
                            ctx.fill_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                            ctx.set_stroke_color(if is_expanded_sel { &colors.success } else { &toolbar_theme.separator });
                            ctx.set_stroke_width(1.0);
                            ctx.stroke_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                            ctx.set_fill_color(if is_expanded_sel { &colors.success } else { &toolbar_theme.item_text_muted });
                            ctx.set_text_align(TextAlign::Center);
                            ctx.set_text_baseline(TextBaseline::Middle);
                            ctx.fill_text(btn_label, btn_x + btn_w / 2.0, row_y + btn_h / 2.0);

                            let wid = format!("overlay_settings:expand:{}", selected_id);
                            let r = WidgetRect::new(btn_x, row_y, btn_w, btn_h);
                            result.content_items.push((wid.clone(), r));
                            input_coordinator.register_on_layer(wid.as_str(), r, Sense::CLICK, &layer_id);
                            btn_x += btn_w + btn_gap;
                        }

                        // Hide/Show button
                        if can_hide {
                            let btn_label = if is_hidden { "Show" } else { "Hide" };
                            ctx.set_font("12px sans-serif");
                            let btn_w = ctx.measure_text(btn_label) + 16.0;
                            let bg = if is_hidden { &toolbar_theme.item_bg_hover } else { &toolbar_theme.accent };
                            ctx.set_fill_color(bg);
                            ctx.fill_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                            ctx.set_stroke_color(if is_hidden { &toolbar_theme.item_text_muted } else { &toolbar_theme.accent });
                            ctx.set_stroke_width(1.0);
                            ctx.stroke_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                            ctx.set_fill_color(if is_hidden { &toolbar_theme.item_text_muted } else { &toolbar_theme.accent });
                            ctx.set_text_align(TextAlign::Center);
                            ctx.set_text_baseline(TextBaseline::Middle);
                            ctx.fill_text(btn_label, btn_x + btn_w / 2.0, row_y + btn_h / 2.0);

                            let wid = if is_hidden {
                                format!("overlay_settings:show:{}", selected_id)
                            } else {
                                format!("overlay_settings:hide:{}", selected_id)
                            };
                            let r = WidgetRect::new(btn_x, row_y, btn_w, btn_h);
                            result.content_items.push((wid.clone(), r));
                            input_coordinator.register_on_layer(wid.as_str(), r, Sense::CLICK, &layer_id);
                            btn_x += btn_w + btn_gap;
                        }

                        // Delete button
                        if can_delete {
                            let btn_label = "Delete";
                            ctx.set_font("12px sans-serif");
                            let btn_w = ctx.measure_text(btn_label) + 16.0;
                            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                            ctx.fill_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                            ctx.set_stroke_color(&colors.danger);
                            ctx.set_stroke_width(1.0);
                            ctx.stroke_rounded_rect(btn_x, row_y, btn_w, btn_h, 3.0);
                            ctx.set_fill_color(&colors.danger);
                            ctx.set_text_align(TextAlign::Center);
                            ctx.set_text_baseline(TextBaseline::Middle);
                            ctx.fill_text(btn_label, btn_x + btn_w / 2.0, row_y + btn_h / 2.0);

                            let wid = format!("overlay_settings:eliminate:{}", selected_id);
                            let r = WidgetRect::new(btn_x, row_y, btn_w, btn_h);
                            result.content_items.push((wid.clone(), r));
                            input_coordinator.register_on_layer(wid.as_str(), r, Sense::CLICK, &layer_id);
                        }
                    }
                } else {
                    // No selection — show hint
                    ctx.set_fill_color(&toolbar_theme.item_text_muted);
                    ctx.set_font("12px sans-serif");
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text("Select a panel on the map", content_left, row_y + row_height / 2.0);
                }
            } else {
                // No valid chart area dimensions
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.set_font("12px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("No panel data available", content_left, row_y + row_height / 2.0);
            }
        }
    }

    result
}
