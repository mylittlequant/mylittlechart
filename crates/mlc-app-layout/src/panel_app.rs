//! PanelApp implementation for the chart panel
//!
//! Makes the chart panel an autonomous application that owns its toolbar
//! and handles its own drawing tool state.

use uzor::panel_api::{
    PanelToolbarDef, DropdownItemDef, ToolbarItemDef, SectionAlign,
    ToolbarIconId,
};
use uzor::render::{RenderContext, draw_svg_icon};
use crate::layout::LayoutRect;
use crate::layout::ExtendedFrameLayout;
use mlc_core::scale_settings::ScaleSettings;
use crate::ui::toolbar_render::{
    ToolbarRect, ToolbarRenderResult,
};
use crate::ui::dropdown::{DropdownItem, DropdownConfig, DropdownTheme, draw_dropdown, GridDropdownConfig, draw_grid_dropdown};
use crate::ui::toolbar_core::{
    WidgetRect, IconId,
    ToolbarConfig as TcConfig, ToolbarSection, ToolbarItem as TcToolbarItem,
    ToolbarTheme as TcToolbarTheme, ToolbarOrientation as TcOrientation,
    SectionAlign as TcSectionAlign, ToolbarResult as TcToolbarResult,
    draw_toolbar_with_icons, calculate_section_width,
    apply_active_states, apply_toggle_icons, apply_quick_select_icons,
};
use mlc_core::state::selected_config::SelectedPrimitiveConfig;

// Types moved to state/toolbar_state.rs (Phase X.4 back-edge removal).
// Re-exported here so any `crate::panel_app::*` paths remain valid.
pub use mlc_core::state::toolbar_state::{
    ToolbarConfig,
    ChartToolbarState,
    ChartToolbarRenderResult,
    FloatingInlineBar,
    InlineDockEdge,
    InlineSlideContainer,
    DropdownRenderInfo,
    InlineDropdownResult,
    InlineConfigResult,
};

// =============================================================================
// Layout
// =============================================================================

/// Layout regions for a chart panel with local toolbars.
///
/// Computed by carving all four toolbars (top control strip, left drawing
/// toolbar, right sidebar toolbar, bottom toolbar) out of the full window
/// rect, leaving a content rect for the chart itself.
///
/// ```text
/// +------------------------------------------+
/// |          top_toolbar (40px)              |
/// +------+---------------------------+-------+
/// |      |                           |       |
/// | left |       content_rect        | right |
/// | 50px |                           | 50px  |
/// |      |                           |       |
/// +------+---------------------------+-------+
/// |          bottom_toolbar (40px)           |
/// +------------------------------------------+
/// ```
#[derive(Clone, Copy, Debug)]
pub struct ChartPanelLayout {
    /// Full window rect (original, before toolbar carving)
    pub full_rect: LayoutRect,
    /// Top toolbar area (40px tall, full window width)
    pub top_toolbar_rect: LayoutRect,
    /// Left drawing toolbar area (50px wide, full height from top toolbar to window bottom)
    pub left_toolbar_rect: LayoutRect,
    /// Right sidebar toolbar area (50px wide, full height from top toolbar to window bottom)
    pub right_toolbar_rect: LayoutRect,
    /// Bottom toolbar area (40px tall, between left and right toolbars)
    pub bottom_toolbar_rect: LayoutRect,
    /// Chart content area (grid, candles, scales)
    pub content_rect: LayoutRect,
}

impl ChartPanelLayout {
    /// Compute layout by carving all four toolbars out of `window_rect`.
    ///
    /// Each toolbar is only carved when its `toolbar_config` entry is `Some`.
    /// When all entries are `None` (e.g. `ToolbarConfig::minimal()`), all
    /// toolbar rects are zero-sized and `content_rect` equals the full
    /// `window_rect` (no space carved).
    pub fn compute(window_rect: &LayoutRect, toolbar_config: &ToolbarConfig) -> Self {
        let top_h    = toolbar_config.top_height();
        let left_w   = toolbar_config.left_width();
        let right_w  = toolbar_config.right_width();
        let bottom_h = toolbar_config.bottom_height();

        // П-shape: top full width, left/right full height below top, bottom between left/right
        let side_y      = window_rect.y + top_h;
        let side_height = (window_rect.height - top_h).max(0.0);

        let zero = LayoutRect::new(0.0, 0.0, 0.0, 0.0);

        Self {
            full_rect: *window_rect,
            // Top toolbar: full width
            top_toolbar_rect: if top_h > 0.0 {
                LayoutRect::new(window_rect.x, window_rect.y, window_rect.width, top_h)
            } else { zero },
            // Left toolbar: full height from top toolbar to window bottom
            left_toolbar_rect: if left_w > 0.0 {
                LayoutRect::new(window_rect.x, side_y, left_w, side_height)
            } else { zero },
            // Right toolbar: full height from top toolbar to window bottom
            right_toolbar_rect: if right_w > 0.0 {
                LayoutRect::new(window_rect.x + window_rect.width - right_w, side_y, right_w, side_height)
            } else { zero },
            // Bottom toolbar: between left and right toolbars only
            bottom_toolbar_rect: if bottom_h > 0.0 {
                LayoutRect::new(
                    window_rect.x + left_w,
                    window_rect.y + window_rect.height - bottom_h,
                    (window_rect.width - left_w - right_w).max(0.0),
                    bottom_h,
                )
            } else { zero },
            // Content: between all four toolbars
            content_rect: LayoutRect::new(
                window_rect.x + left_w,
                side_y,
                (window_rect.width - left_w - right_w).max(0.0),
                (window_rect.height - top_h - bottom_h).max(0.0),
            ),
        }
    }
}

// =============================================================================
// Chart internal layout
// =============================================================================

/// The chart's computed internal layout.
///
/// Contains both toolbar layout and chart content layout.
/// This is the SINGLE SOURCE OF TRUTH for chart coordinate computation.
#[derive(Clone, Debug)]
pub struct ChartInternalLayout {
    /// Toolbar layout (50px left drawing toolbar, 40px top control strip)
    pub panel_layout: ChartPanelLayout,
    /// Extended layout of chart content (main chart, sub-panes, scales)
    pub extended: ExtendedFrameLayout,
}

/// Compute the chart's internal layout from a full panel rect.
///
/// This is the CANONICAL way to compute chart layout.
/// It handles:
/// 1. Carving out toolbar areas (ChartPanelLayout)
/// 2. Computing ExtendedFrameLayout from the remaining content area
///
/// Both rendering and input handling MUST use this function
/// to ensure consistent coordinates.
pub fn compute_chart_internal_layout(
    full_panel_rect: &LayoutRect,
    toolbar_config: &ToolbarConfig,
    sub_pane_instance_ids: &[u64],
    scale_settings: &ScaleSettings,
    sub_proportions: &[f64],
    main_proportion: Option<f64>,
    separator_height: f64,
) -> ChartInternalLayout {
    let panel_layout = ChartPanelLayout::compute(full_panel_rect, toolbar_config);
    let extended = ExtendedFrameLayout::from_proportions(
        &panel_layout.content_rect,
        sub_pane_instance_ids,
        sub_proportions,
        main_proportion,
        scale_settings,
        separator_height,
        &std::collections::HashSet::new(),
    );
    ChartInternalLayout {
        panel_layout,
        extended,
    }
}

// =============================================================================
// FloatingInlineBar layout-dependent free functions
//
// FloatingInlineBar is defined in mlc_core (state/toolbar_state.rs), so inherent
// impl blocks for it here would violate the orphan rule. These are free functions
// that take a FloatingInlineBar by reference instead.
// =============================================================================

/// Compute the absolute rect for the bar given the panel layout and the rendered width.
///
/// When docked, the bar slides within the slide container (free space inside the toolbar).
/// When free-floating, the bar is clamped to content_rect minus `sidebar_w` so it never
/// slides under an open sidebar.
pub fn floating_inline_absolute_rect(
    bar: &FloatingInlineBar,
    layout: &ChartPanelLayout,
    bar_width: f64,
    top_slide: &InlineSlideContainer,
    bottom_slide: &InlineSlideContainer,
    sidebar_w: f64,
) -> LayoutRect {
    let content = &layout.content_rect;
    match bar.dock_edge {
        InlineDockEdge::Bottom => {
            // Docked in the bottom toolbar — sidebar is below, doesn't restrict movement.
            let slide_right = (bottom_slide.x + bottom_slide.width - bar_width)
                .max(bottom_slide.x);
            let clamped_x = (content.x + bar.x)
                .clamp(bottom_slide.x, slide_right);
            LayoutRect::new(clamped_x, layout.bottom_toolbar_rect.y, bar_width, FloatingInlineBar::BAR_HEIGHT)
        }
        InlineDockEdge::Top => {
            // Docked in the top toolbar — sidebar is below, doesn't restrict movement.
            let slide_right = (top_slide.x + top_slide.width - bar_width)
                .max(top_slide.x);
            let clamped_x = (content.x + bar.x)
                .clamp(top_slide.x, slide_right);
            LayoutRect::new(clamped_x, layout.top_toolbar_rect.y, bar_width, FloatingInlineBar::BAR_HEIGHT)
        }
        InlineDockEdge::Free => {
            let available_w = (content.width - sidebar_w - bar_width).max(0.0);
            let clamped_x = bar.x.clamp(0.0, available_w);
            LayoutRect::new(content.x + clamped_x, content.y + bar.y, bar_width, FloatingInlineBar::BAR_HEIGHT)
        }
    }
}

/// Update position during drag using the full panel layout.
///
/// `sidebar_w` is the width of the open sidebar so free-mode clamping respects it.
pub fn floating_inline_update_drag(
    bar: &mut FloatingInlineBar,
    cursor_x: f64,
    cursor_y: f64,
    layout: &ChartPanelLayout,
    bar_width: f64,
    sidebar_w: f64,
) {
    if !bar.dragging { return; }

    let content = &layout.content_rect;
    let new_abs_x = cursor_x - bar.drag_offset_x;
    let new_abs_y = cursor_y - bar.drag_offset_y;

    // Store x as content-local offset (floating_inline_absolute_rect will re-clamp per dock edge)
    bar.x = new_abs_x - content.x;

    // Check magnetism — snap INTO toolbar when close enough or beyond edge
    let bar_bottom = new_abs_y + FloatingInlineBar::BAR_HEIGHT;
    let bar_top = new_abs_y;
    let top_toolbar_bottom = layout.top_toolbar_rect.y + layout.top_toolbar_rect.height;
    let bottom_toolbar_top = layout.bottom_toolbar_rect.y;

    // Snap to bottom: bar top is near or below the bottom toolbar top
    let dist_to_bottom = bottom_toolbar_top - bar_top;
    // Snap to top: bar bottom is near or above the top toolbar bottom
    let dist_to_top = bar_bottom - top_toolbar_bottom;

    if dist_to_bottom.abs() < FloatingInlineBar::SNAP_THRESHOLD || bar_top >= bottom_toolbar_top {
        bar.dock_edge = InlineDockEdge::Bottom;
    } else if dist_to_top.abs() < FloatingInlineBar::SNAP_THRESHOLD || bar_bottom <= top_toolbar_bottom {
        bar.dock_edge = InlineDockEdge::Top;
    } else {
        bar.dock_edge = InlineDockEdge::Free;
        // In free mode, clamp x to content area minus sidebar.
        // Y is allowed to go above the content area (negative = over top toolbar),
        // only clamped to the window top (0 absolute) and bottom of content.
        let available_w = (content.width - sidebar_w - bar_width).max(0.0);
        bar.x = bar.x.clamp(0.0, available_w);
        let min_y = -content.y; // allows bar to reach absolute y=0 (top of window)
        let max_y = (content.height - FloatingInlineBar::BAR_HEIGHT).max(0.0);
        bar.y = (new_abs_y - content.y).clamp(min_y, max_y);
    }
}

// =============================================================================
// ChartToolbarState render methods (struct + state methods in state/toolbar_state.rs)
// =============================================================================

/// Render both local toolbars (drawing toolbar on the left, control strip
/// on top) and return hit zones plus the content rect.
///
/// When `selected_primitive` is `Some`, an inline config toolbar is rendered
/// on the right side of the control strip for the selected drawing primitive.
///
/// `toolbar_config` controls which toolbars are rendered. Only toolbars with
/// `Some(def)` in the config are rendered; others are skipped entirely.
///
/// `toolbar_theme` overrides the default dark theme.  Pass `None` to keep
/// the hardcoded dark fallback (for call sites that do not have a theme).
/// `dropdown_theme` overrides colors used for dropdown/submenu popups.
/// Pass `None` to fall back to `DropdownTheme::default()`.
/// `ChartPanelApp::render_toolbars_with_theme` passes both themes from its ThemeManager.
///
/// `allowed_timeframes` gates the `timeframe_selector` control (writer:
/// `mlc-app::active_series::allowed_timeframes`, audit D8 follow-up
/// 2026-07-04). `None` means the TF picker is not applicable for the active
/// chart_type (renko/pnf/kagi/three_line_break, counted-bar family) — the
/// control is dropped from the control strip entirely instead of rendering
/// a picker with no effect. `Some(set)` means the picker applies; TF ids not
/// in `set` (insufficient backfilled depth) render disabled in the popup.
///
/// `chart_type_data_modes` marks the `chart_type_selector` popup items with
/// a "hybrid"/"live only" subtitle (writer: `ChartApp::chart_type_data_modes`,
/// hybrid-data-modes-plan-2026-07-08 Step 5). `None`/missing-entry means no
/// marker — items stay fully clickable either way (owner directive: no
/// chart type is ever disabled by data capability).
pub fn chart_toolbar_render_toolbars(
    tb: &ChartToolbarState,
    ctx: &mut dyn RenderContext,
    layout: &ChartPanelLayout,
    toolbar_config: &ToolbarConfig,
    selected_primitive: Option<&SelectedPrimitiveConfig>,
    toolbar_theme: Option<&crate::ui::toolbar_render::ToolbarTheme>,
    dropdown_theme: Option<&crate::ui::dropdown::DropdownTheme>,
    clock_time: Option<&str>,
    presets: &[mlc_core::PresetSummary],
    active_preset_id: &str,
    autosave_enabled: bool,
    sync_flags: Option<&mlc_core::SyncFlags>,
    is_expanded: bool,
    split_without_group: bool,
    is_mono_group: bool,
    active_symbol: Option<&str>,
    active_timeframe: Option<&str>,
    sidebar_w: f64,
    allowed_timeframes: Option<&std::collections::HashSet<&'static str>>,
    active_chart_type: Option<&str>,
    chart_type_data_modes: Option<&std::collections::HashMap<&'static str, mlc_core::chart_type::DataMode>>,
) -> ChartToolbarRenderResult {
        // Use provided theme or fall back to the hardcoded dark default.
        let theme: TcToolbarTheme = match toolbar_theme {
            Some(t) => TcToolbarTheme {
                background: t.background.clone(),
                separator: t.separator.clone(),
                item_bg_hover: t.item_bg_hover.clone(),
                item_bg_active: t.item_bg_active.clone(),
                item_text: t.item_text.clone(),
                item_text_muted: t.item_text_muted.clone(),
                item_text_hover: t.item_text_hover.clone(),
                item_text_active: t.item_text_active.clone(),
                accent: t.accent.clone(),
                sidebar_style: t.sidebar_style,
            },
            None => TcToolbarTheme::default(),
        };

        let _active_tool = tb.active_tool_id.as_deref();

        // --- Drawing toolbar (vertical, left side) ---
        let drawing_tc_result = if toolbar_config.left.is_some() {
            let drawing_def = toolbar_config.left.as_ref().unwrap();
            let mut drawing_sections = convert_toolbar_def_to_sections(drawing_def);

            // Apply active states
            drawing_sections = apply_active_states(drawing_sections, |id| {
                match id {
                    "magnet" => tb.magnet_enabled,
                    "lock" => tb.drawings_locked,
                    "eye" => !tb.drawings_visible,
                    _ => tb.is_active(id),
                }
            });

            // Apply toggle icon swaps (Lock/Unlock, Eye/EyeOff, MagnetStrong)
            drawing_sections = apply_toggle_icons(
                drawing_sections,
                |id| match id {
                    "magnet" => tb.magnet_enabled,
                    "lock" => tb.drawings_locked,
                    "eye" => !tb.drawings_visible,
                    _ => false,
                },
                |id| match id {
                    "magnet" => if tb.magnet_strong { Some("MagnetStrong") } else { None },
                    "lock" => if tb.drawings_locked { Some("Lock") } else { None },
                    "eye" => if !tb.drawings_visible { Some("EyeOff") } else { None },
                    _ => None,
                },
            );

            // Apply quick-select icon overrides
            let qs_memory = &tb.quick_select_memory;
            drawing_sections = apply_quick_select_icons(drawing_sections, |id| {
                qs_memory.get(id).map(|(_, icon)| IconId::new(icon))
            });

            let drawing_config = TcConfig {
                sections: drawing_sections,
                orientation: TcOrientation::Vertical,
                item_size: 42.0,
                icon_size: 28.0,
                spacing: 4.0,
                padding: 4.0,
                separator_size: 1.0,
                scroll_offset: tb.left_scroll_offset,
            };

            let drawing_rect = WidgetRect::new(
                layout.left_toolbar_rect.x,
                layout.left_toolbar_rect.y,
                layout.left_toolbar_rect.width,
                layout.left_toolbar_rect.height,
            );

            draw_toolbar_with_icons(
                ctx,
                &drawing_config,
                drawing_rect,
                &theme,
                tb.hovered_left_toolbar_id.as_deref(),
            )
        } else {
            TcToolbarResult::default()
        };

        // --- Control strip (horizontal, top) ---
        let control_tc_result = if toolbar_config.top.is_some() {
            let control_def = toolbar_config.top.as_ref().unwrap();
            let mut control_sections = convert_toolbar_def_to_sections(control_def);

            // Apply active states for control strip
            control_sections = apply_active_states(control_sections, |id| {
                tb.is_active(id)
            });

            // Override symbol_selector and timeframe_selector text with actual live values.
            if active_symbol.is_some() || active_timeframe.is_some() {
                for section in &mut control_sections {
                    for item in &mut section.items {
                        if let TcToolbarItem::Dropdown { id, text, .. } = item {
                            if id == "symbol_selector" {
                                if let Some(sym) = active_symbol {
                                    *text = Some(sym.to_string());
                                }
                            } else if id == "timeframe_selector" {
                                if let Some(tf) = active_timeframe {
                                    *text = Some(tf.to_string());
                                }
                            }
                        }
                    }
                }
            }

            // Override chart_type_selector icon with the active chart_type's icon.
            // The toolbar def hardcodes "Candlestick" as a fallback (see
            // `top_toolbar()` in mlc-core::toolbar), but at render time we know
            // the actual chart_type — resolve its icon via
            // `mlc_core::toolbar::chart_type_icon_id()`. §3.3 (handoff 2026-07-06).
            //
            // Note: the dropdown builder in panel_app.rs downgrades a
            // no-text Dropdown to `TcToolbarItem::IconButton` (see line ~1311),
            // so match both shapes here.
            if let Some(ct) = active_chart_type {
                let icon_name = mlc_core::toolbar::chart_type_icon_id(ct);
                for section in &mut control_sections {
                    for item in &mut section.items {
                        match item {
                            TcToolbarItem::IconButton { id, icon, .. }
                                if id == "chart_type_selector" =>
                            {
                                *icon = IconId::new(icon_name);
                            }
                            TcToolbarItem::Dropdown { id, icon, .. }
                                if id == "chart_type_selector" =>
                            {
                                *icon = Some(IconId::new(icon_name));
                            }
                            _ => {}
                        }
                    }
                }
            }

            // TF picker button always visible so its position in the toolbar
            // stays stable across chart-type switches. When the current
            // chart_type is TF-inert (renko/pnf/kagi/three_line_break,
            // counted-bar family), the popup content is what tells the story
            // via per-item disable (see `chart_toolbar_render_dropdown`);
            // the button itself is not hidden. Owner rule 2026-07-04.

            let control_config = TcConfig {
                sections: control_sections,
                orientation: TcOrientation::Horizontal,
                item_size: 28.0,
                icon_size: 24.0,
                spacing: 4.0,
                padding: 4.0,
                separator_size: 1.0,
                scroll_offset: tb.top_scroll_offset,
            };

            let control_rect = WidgetRect::new(
                layout.top_toolbar_rect.x,
                layout.top_toolbar_rect.y,
                layout.top_toolbar_rect.width,
                layout.top_toolbar_rect.height,
            );

            draw_toolbar_with_icons(
                ctx,
                &control_config,
                control_rect,
                &theme,
                tb.hovered_top_toolbar_id.as_deref(),
            )
        } else {
            TcToolbarResult::default()
        };

        // --- Right toolbar (vertical, right side, sidebar style) ---
        let right_tc_result = if toolbar_config.right.is_some() {
            let right_def = toolbar_config.right.as_ref().unwrap();
            let right_sections = convert_toolbar_def_to_sections(right_def);

            // Right toolbar uses sidebar_style so it renders with the sidebar variant.
            let right_theme = TcToolbarTheme {
                sidebar_style: true,
                ..theme.clone()
            };

            let right_config = TcConfig {
                sections: right_sections,
                orientation: TcOrientation::Vertical,
                item_size: 42.0,
                icon_size: 28.0,
                spacing: 4.0,
                padding: 4.0,
                separator_size: 1.0,
                scroll_offset: tb.right_scroll_offset,
            };

            let right_rect = WidgetRect::new(
                layout.right_toolbar_rect.x,
                layout.right_toolbar_rect.y,
                layout.right_toolbar_rect.width,
                layout.right_toolbar_rect.height,
            );

            draw_toolbar_with_icons(
                ctx,
                &right_config,
                right_rect,
                &right_theme,
                tb.hovered_right_toolbar_id.as_deref(),
            )
        } else {
            TcToolbarResult::default()
        };

        // --- Bottom toolbar (horizontal, bottom) ---
        let bottom_tc_result = if toolbar_config.bottom.is_some() {
            let bottom_def = toolbar_config.bottom.as_ref().unwrap();
            let mut bottom_sections = convert_toolbar_def_to_sections(bottom_def);

            // Update clock time if provided
            if let Some(time_str) = clock_time {
                for section in &mut bottom_sections {
                    for item in &mut section.items {
                        if let TcToolbarItem::Clock { time, .. } = item {
                            *time = time_str.to_string();
                        }
                    }
                }
            }

            // Update expand button icon: show Collapse icon when panel is expanded
            for section in &mut bottom_sections {
                for item in &mut section.items {
                    if let TcToolbarItem::IconButton { id, icon, active, .. } = item {
                        if id == "expand" {
                            *icon = if is_expanded {
                                IconId::new("Collapse")
                            } else {
                                IconId::new("Expand")
                            };
                            *active = false;
                        }
                    }
                }
            }

            let bottom_config = TcConfig {
                sections: bottom_sections,
                orientation: TcOrientation::Horizontal,
                item_size: 28.0,
                icon_size: 24.0,
                spacing: 4.0,
                padding: 4.0,
                separator_size: 1.0,
                scroll_offset: tb.bottom_scroll_offset,
            };

            let bottom_rect = WidgetRect::new(
                layout.bottom_toolbar_rect.x,
                layout.bottom_toolbar_rect.y,
                layout.bottom_toolbar_rect.width,
                layout.bottom_toolbar_rect.height,
            );

            draw_toolbar_with_icons(
                ctx,
                &bottom_config,
                bottom_rect,
                &theme,
                tb.hovered_bottom_toolbar_id.as_deref(),
            )
        } else {
            TcToolbarResult::default()
        };

        // Draw toolbar borders on top of the toolbar backgrounds.
        // The terminal draws these via draw_toolbar_borders(); the chart panel
        // draws them directly here using the same border colour (theme.separator).
        //
        // - Drawing toolbar (left, vertical):  border on the right edge.
        // - Control strip (top, horizontal):   border on the bottom edge.
        // - Right toolbar (right, vertical):   border on the left edge.
        // - Bottom toolbar (bottom, horizontal): border on the top edge.
        ctx.set_fill_color(&theme.separator);
        // Right border of drawing toolbar (only when visible)
        if toolbar_config.left.is_some() && layout.left_toolbar_rect.width > 0.0 {
            ctx.fill_rect(
                layout.left_toolbar_rect.right() - 1.0,
                layout.left_toolbar_rect.y,
                1.0,
                layout.left_toolbar_rect.height,
            );
        }
        // Bottom border of control strip (only when visible)
        if toolbar_config.top.is_some() && layout.top_toolbar_rect.height > 0.0 {
            ctx.fill_rect(
                layout.top_toolbar_rect.x,
                layout.top_toolbar_rect.bottom() - 1.0,
                layout.top_toolbar_rect.width,
                1.0,
            );
        }
        // Left border of right toolbar (only when visible)
        if toolbar_config.right.is_some() && layout.right_toolbar_rect.width > 0.0 {
            ctx.fill_rect(
                layout.right_toolbar_rect.x,
                layout.right_toolbar_rect.y,
                1.0,
                layout.right_toolbar_rect.height,
            );
        }
        // Top border of bottom toolbar (only when visible)
        if toolbar_config.bottom.is_some() && layout.bottom_toolbar_rect.height > 0.0 {
            ctx.fill_rect(
                layout.bottom_toolbar_rect.x,
                layout.bottom_toolbar_rect.y,
                layout.bottom_toolbar_rect.width,
                1.0,
            );
        }

        // Extract chevron rects and overflow info from raw TcToolbarResults before converting.
        let drawing_left_chev = drawing_tc_result.left_chevron_rect;
        let drawing_right_chev = drawing_tc_result.right_chevron_rect;
        let drawing_max_scroll = drawing_tc_result.max_scroll;
        let control_left_chev = control_tc_result.left_chevron_rect;
        let control_right_chev = control_tc_result.right_chevron_rect;
        let control_max_scroll = control_tc_result.max_scroll;
        let right_left_chev = right_tc_result.left_chevron_rect;
        let right_right_chev = right_tc_result.right_chevron_rect;
        let right_max_scroll = right_tc_result.max_scroll;
        let bottom_left_chev = bottom_tc_result.left_chevron_rect;
        let bottom_right_chev = bottom_tc_result.right_chevron_rect;
        let bottom_max_scroll = bottom_tc_result.max_scroll;

        // Convert ToolbarResult -> ToolbarRenderResult (for existing hit-test code)
        let drawing_result = tc_result_to_render_result(drawing_tc_result);

        // --- Compute slide containers for inline bar docking ---
        //
        // Top slide: free space starts after the last start-aligned button, extends to toolbar right edge.
        let top_slide = {
            let rightmost = control_tc_result.item_rects.iter()
                .map(|(_, wr)| wr.x + wr.width)
                .fold(layout.top_toolbar_rect.x, f64::max);
            let right_edge = layout.top_toolbar_rect.x + layout.top_toolbar_rect.width;
            InlineSlideContainer {
                x: rightmost + 4.0,
                width: (right_edge - rightmost - 4.0).max(0.0),
            }
        };
        // Bottom slide: free space from toolbar left edge to the first end-aligned button.
        let bottom_slide = {
            let leftmost = bottom_tc_result.item_rects.iter()
                .map(|(_, wr)| wr.x)
                .fold(layout.bottom_toolbar_rect.x + layout.bottom_toolbar_rect.width, f64::min);
            InlineSlideContainer {
                x: layout.bottom_toolbar_rect.x,
                width: (leftmost - layout.bottom_toolbar_rect.x - 4.0).max(0.0),
            }
        };

        // --- Floating inline config toolbar (overlay on content area) ---
        //
        // When a primitive is selected, render a standalone floating bar on top of
        // the content area. Position and dock edge are controlled by floating_inline_bar.
        let inline_config = if let Some(config) = selected_primitive {
            let inline_section = ToolbarSection::inline_config(
                &config.name,
                &config.color,
                config.text_color.as_deref(),
                config.supports_text,
                config.width as u32,
                &config.style,
                config.locked,
            );

            let inline_tc_config = TcConfig {
                sections: vec![inline_section],
                orientation: TcOrientation::Horizontal,
                item_size: 28.0,
                icon_size: 24.0,
                spacing: 4.0,
                padding: 4.0,
                separator_size: 1.0,
                scroll_offset: 0.0, // Inline bar never overflows (sized to fit)
            };

            // Pre-compute exact bar width from section items (single-pass, no measurement draw).
            let sections_width: f64 = inline_tc_config.sections.iter()
                .map(|s| calculate_section_width(s, &inline_tc_config))
                .sum();
            let bar_width = sections_width + inline_tc_config.padding * 2.0;
            let bar_rect = floating_inline_absolute_rect(&tb.floating_inline_bar, layout, bar_width, &top_slide, &bottom_slide, sidebar_w);

            // Single draw pass: draw_toolbar_with_icons draws background + items at exact width.
            let bar_wr = WidgetRect::new(bar_rect.x, bar_rect.y, bar_width, bar_rect.height);
            let inline_result = draw_toolbar_with_icons(
                ctx,
                &inline_tc_config,
                bar_wr,
                &theme,
                tb.hovered_inline_id.as_deref(),
            );

            // Border — suppress the edge that merges with the host toolbar when docked.
            ctx.set_fill_color(&theme.separator);
            let dock = tb.floating_inline_bar.dock_edge;
            // Top border: hide when docked to top (merges with control strip)
            if dock != InlineDockEdge::Top {
                ctx.fill_rect(bar_rect.x, bar_rect.y, bar_width, 1.0);
            }
            // Bottom border: hide when docked to bottom (merges with bottom toolbar)
            if dock != InlineDockEdge::Bottom {
                ctx.fill_rect(bar_rect.x, bar_rect.y + bar_rect.height - 1.0, bar_width, 1.0);
            }
            // Side borders always
            ctx.fill_rect(bar_rect.x, bar_rect.y, 1.0, bar_rect.height);
            ctx.fill_rect(bar_rect.x + bar_width - 1.0, bar_rect.y, 1.0, bar_rect.height);

            let inline_rects: Vec<(String, ToolbarRect)> = inline_result.item_rects.into_iter()
                .map(|(id, wr)| (id, ToolbarRect::new(wr.x, wr.y, wr.width, wr.height)))
                .collect();

            if !inline_rects.is_empty() {
                Some(InlineConfigResult {
                    item_rects: inline_rects,
                    bar_rect: LayoutRect::new(bar_rect.x, bar_rect.y, bar_width, bar_rect.height),
                })
            } else {
                None
            }
        } else {
            None
        };

        let control_result = tc_result_to_render_result(control_tc_result);
        let right_result = tc_result_to_render_result(right_tc_result);
        let bottom_result = tc_result_to_render_result(bottom_tc_result);

        // Build the effective dropdown theme (caller-supplied or default dark fallback).
        let effective_dropdown_theme;
        let dd_theme_ref: &crate::ui::dropdown::DropdownTheme = match dropdown_theme {
            Some(t) => t,
            None => {
                effective_dropdown_theme = crate::ui::dropdown::DropdownTheme::default();
                &effective_dropdown_theme
            }
        };

        // Render open dropdown if any
        let dropdown_result = if let Some(ref dropdown_id) = tb.open_dropdown_id {
            chart_toolbar_render_dropdown(tb, ctx, dropdown_id, toolbar_config, layout, &drawing_result, &control_result, dd_theme_ref, presets, active_preset_id, autosave_enabled, sync_flags, is_expanded, split_without_group, is_mono_group, allowed_timeframes, chart_type_data_modes)
        } else {
            None
        };

        // Render inline style/width dropdown popup if open
        let inline_dropdown_result = chart_toolbar_render_inline_dropdown(
            tb,
            ctx,
            &inline_config,
            dd_theme_ref,
            &theme,
            layout.full_rect.height,
        );

        ChartToolbarRenderResult {
            left_toolbar: drawing_result,
            top_toolbar: control_result,
            right_toolbar: right_result,
            bottom_toolbar: bottom_result,
            content_rect: layout.content_rect,
            dropdown_result,
            inline_config,
            inline_dropdown_result,
            // Scroll chevron rects — left/right for horizontal, up/down for vertical
            top_left_chevron: control_left_chev,
            top_right_chevron: control_right_chev,
            bottom_left_chevron: bottom_left_chev,
            bottom_right_chevron: bottom_right_chev,
            left_up_chevron: drawing_left_chev,
            left_down_chevron: drawing_right_chev,
            right_up_chevron: right_left_chev,
            right_down_chevron: right_right_chev,
            top_max_scroll: control_max_scroll,
            bottom_max_scroll,
            left_max_scroll: drawing_max_scroll,
            right_max_scroll,
        }
    }

/// Render a dropdown menu for the given dropdown_id.
///
/// Finds the dropdown's items from the toolbar definitions, converts them
/// to chart's DropdownItem format, and calls draw_dropdown. If a submenu
/// is open (`open_submenu_id` is set), renders the submenu grid to the
/// right of the parent dropdown and merges all item_rects into the result.
///
/// `theme` provides all colors for the popup.  Pass the theme built from the
/// chart's ThemeManager so dropdowns respect the active preset.
fn chart_toolbar_render_dropdown(
    tb: &ChartToolbarState,
    ctx: &mut dyn RenderContext,
    dropdown_id: &str,
    toolbar_config: &ToolbarConfig,
    _layout: &ChartPanelLayout,
    drawing_result: &ToolbarRenderResult,
    control_result: &ToolbarRenderResult,
    theme: &DropdownTheme,
    presets: &[mlc_core::PresetSummary],
    active_preset_id: &str,
    autosave_enabled: bool,
    sync_flags: Option<&mlc_core::SyncFlags>,
    _is_expanded: bool,
    split_without_group: bool,
    is_mono_group: bool,
    allowed_timeframes: Option<&std::collections::HashSet<&'static str>>,
    chart_type_data_modes: Option<&std::collections::HashMap<&'static str, mlc_core::chart_type::DataMode>>,
) -> Option<DropdownRenderInfo> {
        // 1. Find the button rect for this dropdown in the toolbar results.
        //    If open_dropdown_position is set (e.g. for new_tab_menu opened by chrome),
        //    skip the button-rect lookup and use the custom position directly.
        let origin = if let Some(pos) = tb.open_dropdown_position {
            // Custom position: caller provides exact (x, y) for the dropdown top-left.
            pos
        } else {
            let button_rect = drawing_result.item_rects.iter()
                .chain(control_result.item_rects.iter())
                .find(|(id, _)| id == dropdown_id)
                .map(|(_, r)| *r)?;

            // Determine position: right of button for vertical toolbar, below for horizontal
            let is_left_toolbar = drawing_result.item_rects.iter().any(|(id, _)| id == dropdown_id);
            if is_left_toolbar {
                // Vertical toolbar: dropdown opens to the right of the button
                (button_rect.right() + 2.0, button_rect.y)
            } else {
                // Horizontal toolbar: dropdown opens below the button
                (button_rect.x, button_rect.bottom() + 2.0)
            }
        };

        // 2. Find the dropdown items from toolbar definitions
        let items = chart_toolbar_find_dropdown_items(tb, dropdown_id, toolbar_config, presets, active_preset_id)?;

        // 3. Convert DropdownItemDef -> DropdownItem
        let active_preset_load_id = format!("preset_load:{}", active_preset_id);
        let dropdown_items: Vec<DropdownItem> = items.iter().map(|item| {
            let mut di = convert_dropdown_item(item);
            if dropdown_id == "presets_menu" {
                if let DropdownItem::Item { ref id, ref mut accent_color, ref mut toggle, ref mut disabled, .. } = di {
                    // Active preset accent bar
                    if id == &active_preset_load_id {
                        *accent_color = Some("#2962ff".to_string());
                    }
                    // Autosave toggle
                    if id == "preset_autosave" {
                        *toggle = Some(autosave_enabled);
                    }
                    // Save disabled when autosave is on
                    if id == "preset_save" && autosave_enabled {
                        *disabled = true;
                    }
                }
            }
            if dropdown_id == "layout_menu" {
                if let DropdownItem::Item { ref id, ref mut toggle, ref mut disabled, .. } = di {
                    if id == "split_untagged" {
                        *toggle = Some(split_without_group);
                    }
                    if let Some(flags) = sync_flags {
                        match id.as_str() {
                            "sync_symbol"     => *toggle = Some(flags.sync_symbol),
                            "sync_timeframe"  => *toggle = Some(flags.sync_timeframe),
                            "sync_crosshair"  => *toggle = Some(flags.sync_crosshair),
                            "sync_viewport"   => *toggle = Some(flags.sync_viewport),
                            "sync_drawings"   => *toggle = Some(flags.sync_drawings),
                            "sync_indicators" => *toggle = Some(flags.sync_indicators),
                            _ => {}
                        }
                    }
                    // Disable sync toggles for mono-groups (solo windows don't need sync)
                    if is_mono_group && id.starts_with("sync_") {
                        *disabled = true;
                    }
                }
            }
            if dropdown_id == "timeframe_selector" {
                if let DropdownItem::Item { ref id, ref mut disabled, .. } = di {
                    // Depth-gating (audit D8 follow-up, 2026-07-04): a TF not
                    // in the allowed set has insufficient backfilled depth —
                    // disable it rather than remove it, so the full TF list
                    // stays visible/discoverable (only reachability changes).
                    // `None` = TF picker is not applicable at all for the
                    // active chart_type (TF-inert families) — disable EVERY
                    // TF item; the button itself stays visible so its
                    // toolbar position stays stable across chart-type switches.
                    match allowed_timeframes {
                        None => *disabled = true,
                        Some(allowed) if !allowed.contains(id.as_str()) => *disabled = true,
                        _ => {}
                    }
                }
            }
            if dropdown_id == "chart_type_selector" {
                if let DropdownItem::Item { ref id, ref mut subtitle, ref mut accent_color, .. } = di {
                    // Data-availability marking (hybrid-data-modes-plan-
                    // 2026-07-08, Step 5): transparency-only — every chart
                    // type stays clickable (owner directive), `disabled` is
                    // never set here. `None`/`Unknown`/`Full` render no
                    // marker at all.
                    if let Some(marks) = chart_type_data_modes {
                        match marks.get(id.as_str()) {
                            Some(mlc_core::chart_type::DataMode::Hybrid) => {
                                *subtitle = Some("hybrid".to_string());
                                *accent_color = Some("#ffa726".to_string());
                            }
                            Some(mlc_core::chart_type::DataMode::LiveOnly) => {
                                *subtitle = Some("live only".to_string());
                                *accent_color = Some("#ef5350".to_string());
                            }
                            _ => {}
                        }
                    }
                }
            }
            di
        }).collect();

        if dropdown_items.is_empty() {
            return None;
        }

        // 5. Build config and render the main dropdown
        let mut config = DropdownConfig::new(dropdown_items);
        // Chart-type menu: two-column grid inside each section (owner spec
        // 2026-07-05) — headers/separators span the full row, items flow
        // 2 per row. Everything else keeps the classic vertical list.
        if dropdown_id == "chart_type_selector" {
            config.grid_columns = Some(2);
        }

        let result = draw_dropdown(
            ctx,
            &config,
            origin,
            theme,
            tb.hovered_dropdown_item.as_deref(),
            chart_icon_renderer,
        );

        // Capture hover-based submenu and hovered item from the draw_dropdown result
        // before consuming `result`.  These propagate out via DropdownRenderInfo so
        // callers can open submenus on hover without waiting for a click.
        let hover_open_submenu = result.open_submenu.clone();
        let hover_hovered = result.hovered.clone();

        let mut all_item_rects = result.item_rects;
        let mut final_menu_rect = result.menu_rect;

        // 6. Render submenu grid if one is open
        if let Some(ref submenu_id) = tb.open_submenu_id {
            // Find the submenu trigger item rect within the dropdown
            if let Some((_, submenu_trigger_rect)) = all_item_rects.iter().find(|(id, _)| id == submenu_id) {
                let submenu_trigger_rect = *submenu_trigger_rect;

                // Find submenu items from toolbar definitions
                if let Some((sub_items_def, grid_columns)) = chart_toolbar_find_submenu_items(tb, submenu_id, toolbar_config) {
                    let sub_items: Vec<DropdownItem> = sub_items_def.iter().map(|item| {
                        convert_dropdown_item(item)
                    }).collect();

                    if !sub_items.is_empty() {
                        // Position submenu to the right of the parent dropdown menu
                        let sub_origin = (
                            final_menu_rect.right() + 2.0,
                            submenu_trigger_rect.y,
                        );

                        // grid_columns: None → list-style submenu (draw_dropdown)
                        // grid_columns: Some(n) → icon-grid submenu (draw_grid_dropdown)
                        let sub_result = if let Some(columns) = grid_columns {
                            let grid_config = GridDropdownConfig::new(sub_items, columns);
                            draw_grid_dropdown(
                                ctx,
                                &grid_config,
                                sub_origin,
                                theme,
                                tb.hovered_dropdown_item.as_deref(),
                                chart_icon_renderer,
                            )
                        } else {
                            let list_config = DropdownConfig::new(sub_items);
                            draw_dropdown(
                                ctx,
                                &list_config,
                                sub_origin,
                                theme,
                                tb.hovered_dropdown_item.as_deref(),
                                chart_icon_renderer,
                            )
                        };

                        // Merge submenu rects into the result
                        all_item_rects.extend(sub_result.item_rects);

                        // Expand menu_rect to encompass both menus
                        let combined_right = final_menu_rect.right().max(sub_result.menu_rect.right());
                        let combined_bottom = final_menu_rect.bottom().max(sub_result.menu_rect.bottom());
                        let combined_x = final_menu_rect.x.min(sub_result.menu_rect.x);
                        let combined_y = final_menu_rect.y.min(sub_result.menu_rect.y);
                        final_menu_rect = WidgetRect::new(
                            combined_x,
                            combined_y,
                            combined_right - combined_x,
                            combined_bottom - combined_y,
                        );
                    }
                }
            }
        }

        Some(DropdownRenderInfo {
            dropdown_id: dropdown_id.to_string(),
            item_rects: all_item_rects,
            menu_rect: final_menu_rect,
            open_submenu: hover_open_submenu,
            hovered: hover_hovered,
        })
    }

/// Render the inline style or width dropdown popup if one is open.
///
/// Looks up the button rect from `inline_config`, then calls `draw_dropdown` to
/// draw the popup below (or above) the button.  Returns `None` if no inline dropdown
/// is open or if the button rect cannot be found.
///
/// When there is not enough space below the button (button bottom + dropdown height
/// exceeds `panel_height`), the dropdown opens upward and the chevron icon on the
/// button is redrawn pointing up.
fn chart_toolbar_render_inline_dropdown(
    tb: &ChartToolbarState,
    ctx: &mut dyn RenderContext,
    inline_config: &Option<InlineConfigResult>,
    theme: &DropdownTheme,
    toolbar_theme: &TcToolbarTheme,
    panel_height: f64,
) -> Option<InlineDropdownResult> {
    // Determine which inline dropdown is open (style takes priority)
    let (dropdown_id, button_id) = if tb.open_inline_style_dropdown {
        ("inline:style", "inline:style")
    } else if tb.open_inline_width_dropdown {
        ("inline:width", "inline:width")
    } else {
        return None;
    };

        // Find the button rect from the inline toolbar result.
        // We look for the main button area (button_id) to get the button's vertical position,
        // and the chevron area (button_id + "_menu") for the upward-chevron overdraw.
        let inline_cfg = inline_config.as_ref()?;
        let button_rect = inline_cfg.item_rects.iter()
            .find(|(id, _)| id == button_id)
            .map(|(_, r)| *r)?;
        let chev_menu_id = format!("{}_menu", button_id);
        let chevron_rect = inline_cfg.item_rects.iter()
            .find(|(id, _)| id == chev_menu_id.as_str())
            .map(|(_, r)| *r);

        // Build dropdown items
        let items: Vec<crate::ui::dropdown::DropdownItem> = if dropdown_id == "inline:style" {
            vec![
                crate::ui::dropdown::DropdownItem::item("inline:style_option:solid", "Solid")
                    .with_icon(crate::ui::toolbar_core::IconId::new("LineSolid")),
                crate::ui::dropdown::DropdownItem::item("inline:style_option:dashed", "Dashed")
                    .with_icon(crate::ui::toolbar_core::IconId::new("LineDashed")),
                crate::ui::dropdown::DropdownItem::item("inline:style_option:dotted", "Dotted")
                    .with_icon(crate::ui::toolbar_core::IconId::new("LineDotted")),
                crate::ui::dropdown::DropdownItem::item("inline:style_option:large_dashed", "Large Dashed")
                    .with_icon(crate::ui::toolbar_core::IconId::new("LineDashed")),
                crate::ui::dropdown::DropdownItem::item("inline:style_option:sparse_dotted", "Sparse Dotted")
                    .with_icon(crate::ui::toolbar_core::IconId::new("LineDotted")),
            ]
        } else {
            // Width options 1–5
            (1u32..=5).map(|w| {
                crate::ui::dropdown::DropdownItem::item(
                    &format!("inline:width_option:{}", w),
                    &format!("{} px", w),
                )
            }).collect()
        };

        let config = crate::ui::dropdown::DropdownConfig::new(items);

        // Determine whether there is enough room below the button.
        // If not, open the dropdown upward above the button instead.
        let dropdown_height = config.calculate_height();
        let button_bottom = button_rect.y + button_rect.height;
        let space_below = panel_height - button_bottom;
        let open_upward = space_below < dropdown_height + 4.0;

        let origin = if open_upward {
            (button_rect.x, button_rect.y - dropdown_height - 2.0)
        } else {
            (button_rect.x, button_bottom + 2.0)
        };

        let result = crate::ui::dropdown::draw_dropdown(
            ctx,
            &config,
            origin,
            theme,
            tb.hovered_inline_dropdown_item.as_deref(),
            chart_icon_renderer,
        );

        // When opening upward, overdraw the chevron area to flip the arrow direction.
        // The toolbar has already been drawn, so we paint on top of the existing chevron.
        if open_upward {
            if let Some(chev) = chevron_rect {
                // Erase old downward chevron by filling with active button background.
                ctx.set_fill_color(&toolbar_theme.item_bg_active);
                ctx.fill_rect(chev.x, chev.y, chev.width, chev.height);

                // Draw upward-pointing chevron (inverted V shape).
                let cx = chev.x + chev.width / 2.0;
                let cy = chev.y + chev.height / 2.0;
                let cs = 2.5_f64;
                ctx.set_stroke_color(&toolbar_theme.item_text_active);
                ctx.set_stroke_width(1.2);
                ctx.begin_path();
                ctx.move_to(cx - cs, cy + cs / 2.0);
                ctx.line_to(cx, cy - cs / 2.0);
                ctx.line_to(cx + cs, cy + cs / 2.0);
                ctx.stroke();
            }
        }

        Some(InlineDropdownResult {
            dropdown_id: dropdown_id.to_string(),
            item_rects: result.item_rects,
            menu_rect: result.menu_rect,
        })
    }

/// Find the DropdownItemDef list for a given dropdown_id from toolbar definitions.
///
/// Searches the active toolbar definitions from `toolbar_config` (left and top),
/// falling back to the default definitions when a slot is `None`.
///
/// Special case: "magnet" is an icon_button (not a dropdown in the toolbar def),
/// so its dropdown items are returned directly here when opened via double-click.
fn chart_toolbar_find_dropdown_items(
    _tb: &ChartToolbarState,
    dropdown_id: &str,
    toolbar_config: &ToolbarConfig,
    presets: &[mlc_core::PresetSummary],
    active_preset_id: &str,
) -> Option<Vec<DropdownItemDef>> {
        // Special case: magnet is an icon_button that opens a dropdown on double-click.
        // Its items are not in the toolbar definition — return them directly.
        if dropdown_id == "magnet" {
            return Some(vec![
                DropdownItemDef::action("magnet_ohlc", "Weak Magnet (OHLC)").with_icon(ToolbarIconId::new("Magnet")),
                DropdownItemDef::action("magnet_strong", "Strong Magnet (Body)").with_icon(ToolbarIconId::new("MagnetStrong")),
            ]);
        }

        // Special case: presets_menu is built dynamically from in-memory presets.
        if dropdown_id == "presets_menu" {
            let mut items = Vec::new();

            let mut named_presets: Vec<_> = presets.iter()
                .filter(|p| p.id != "__default__")
                .collect();
            named_presets.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            let has_active_named = named_presets.iter().any(|p| p.id == active_preset_id);

            // --- Group 1: New + Save ---
            items.push(DropdownItemDef::action("preset_new_chart", "New Chart").with_icon(ToolbarIconId::new("Plus")));
            items.push(DropdownItemDef::action("preset_autosave", "Autosave"));
            items.push(DropdownItemDef::action("preset_save", "Save"));
            items.push(DropdownItemDef::action("preset_save_as", "Save As").with_icon(ToolbarIconId::new("Copy")));

            // --- Group 2: Recent presets ---
            items.push(DropdownItemDef::Separator);
            items.push(DropdownItemDef::Header { label: "RECENT".to_string() });

            let recent_presets: Vec<_> = named_presets.iter().take(4).cloned().collect();

            if recent_presets.is_empty() {
                items.push(DropdownItemDef::action("preset_noop", "No saved charts"));
            } else {
                for preset in &recent_presets {
                    let load_id = format!("preset_load:{}", preset.id);
                    let summary = preset.first_window.as_ref()
                        .map(|(s, t)| format!("{}, {}", s, t))
                        .unwrap_or_default();
                    let mut item = DropdownItemDef::action(load_id, &preset.name);
                    if !summary.is_empty() {
                        item = item.with_shortcut(summary);
                    }
                    items.push(item);
                }
            }

            // --- Group 3: Rename ---
            if has_active_named {
                items.push(DropdownItemDef::Separator);
                items.push(DropdownItemDef::action("preset_rename", "Rename").with_icon(ToolbarIconId::new("Pencil")));
            }

            // --- Group 4: Open Chart browser ---
            items.push(DropdownItemDef::Separator);
            items.push(DropdownItemDef::action("preset_open_chart", "Open Chart").with_icon(ToolbarIconId::new("Watchlist")));

            return Some(items);
        }

        // Special case: new_tab_menu is built dynamically — shows closed presets
        // and commands for opening a new tab.  This dropdown is opened by the
        // chrome "+" button (which passes an explicit position via open_dropdown_position)
        // rather than by a toolbar button.
        if dropdown_id == "new_tab_menu" {
            let mut items: Vec<DropdownItemDef> = Vec::new();

            // "New Chart" — always at the top
            items.push(DropdownItemDef::action("new_tab:new_chart", "New Chart").with_icon(ToolbarIconId::new("Plus")));

            // Closed presets (presets not in open_tabs, not __default__), newest first, up to 5.
            // NOTE: `presets` and `open_tabs` are available through the ChartPanelApp render
            // context.  However find_dropdown_items only has access to `presets` and
            // `active_preset_id`.  The open_tabs list lives on ChartPanelApp, not here.
            // We work around this by passing the full presets map; callers must ensure
            // the items list is filtered at render time via handle_dropdown_select_with_chart.
            // For now we include ALL named presets (sorted newest-first, up to 5) as candidates;
            // the item ids carry the "new_tab:open:{id}" prefix so the click handler opens them
            // as a new tab via ChartOutEvent::OpenTab rather than LoadPreset.
            let mut named: Vec<_> = presets.iter()
                .filter(|p| p.id != "__default__")
                .collect();
            named.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            let max = named.len().min(5);

            if max > 0 {
                items.push(DropdownItemDef::Separator);
                items.push(DropdownItemDef::Header { label: "RECENT".to_string() });
                for preset in &named[..max] {
                    let load_id = format!("new_tab:open:{}", preset.id);
                    let summary = preset.first_window.as_ref()
                        .map(|(s, t)| format!("{}, {}", s, t))
                        .unwrap_or_default();
                    let mut item = DropdownItemDef::action(load_id, &preset.name);
                    if !summary.is_empty() {
                        item = item.with_shortcut(summary);
                    }
                    items.push(item);
                }
            }

            // "Open Chart" browser — always at the bottom
            items.push(DropdownItemDef::Separator);
            items.push(DropdownItemDef::action("new_tab:browser", "Open Chart").with_icon(ToolbarIconId::new("Watchlist")));

            return Some(items);
        }

        // Search in left toolbar
        let drawing_def = toolbar_config.left.as_ref().cloned()
            .unwrap_or_else(mlc_core::toolbar::left_toolbar);
        if let Some(items) = find_items_in_def(&drawing_def, dropdown_id) {
            return Some(items);
        }
        // Search in top toolbar
        let control_def = toolbar_config.top.as_ref().cloned()
            .unwrap_or_else(mlc_core::toolbar::top_toolbar);
        find_items_in_def(&control_def, dropdown_id)
    }

/// Find submenu items (and grid_columns) for a submenu id nested inside a dropdown.
///
/// Searches the active toolbar definitions from `toolbar_config` (left and top),
/// falling back to the default definitions when a slot is `None`.
fn chart_toolbar_find_submenu_items(_tb: &ChartToolbarState, submenu_id: &str, toolbar_config: &ToolbarConfig) -> Option<(Vec<DropdownItemDef>, Option<u8>)> {
    let drawing_def = toolbar_config.left.as_ref().cloned()
        .unwrap_or_else(mlc_core::toolbar::left_toolbar);
    if let Some(result) = find_submenu_in_def(&drawing_def, submenu_id) {
        return Some(result);
    }
    let control_def = toolbar_config.top.as_ref().cloned()
        .unwrap_or_else(mlc_core::toolbar::top_toolbar);
    find_submenu_in_def(&control_def, submenu_id)
}

/// Convert a `PanelToolbarDef` (uzor-panel-api) into `Vec<ToolbarSection>` (toolbar_core).
///
/// Maps each section and item to the toolbar_core equivalents so that
/// `draw_toolbar_with_icons` can render the chart's toolbar definitions.
fn convert_toolbar_def_to_sections(def: &PanelToolbarDef) -> Vec<ToolbarSection> {
    def.sections.iter().map(|section| {
        let items = section.items.iter().map(|item| {
            match item {
                ToolbarItemDef::Separator => TcToolbarItem::Separator,
                ToolbarItemDef::Spacer => TcToolbarItem::Spacer,
                ToolbarItemDef::IconButton { id, icon, .. } => {
                    TcToolbarItem::icon_button(id, IconId::new(icon.name()))
                }
                ToolbarItemDef::Button { id, icon, text, min_width, .. } => {
                    if *id == "clock" {
                        // Clock buttons are converted to the Clock toolbar item type
                        // which renders with monospace font and right alignment.
                        // The actual time text is updated by the consumer before rendering.
                        TcToolbarItem::Clock {
                            id: id.to_string(),
                            time: text.clone().unwrap_or_else(|| "00:00:00".to_string()),
                        }
                    } else {
                        TcToolbarItem::Button {
                            id: id.to_string(),
                            icon: icon.as_ref().map(|i| IconId::new(i.name())),
                            text: text.clone(),
                            active: false,
                            disabled: false,
                            min_width: *min_width,
                        }
                    }
                }
                ToolbarItemDef::Dropdown { id, icon, text, show_chevron: _, quick_select: _, min_width, .. } => {
                    // Dropdowns without text render as IconButton (matches terminal)
                    if text.is_none() {
                        TcToolbarItem::IconButton {
                            id: id.to_string(),
                            icon: icon.as_ref().map(|i| IconId::new(i.name()))
                                .unwrap_or_else(|| IconId::new("Unknown")),
                            active: false,
                            disabled: false,
                            min_width: 0.0,
                        }
                    } else {
                        TcToolbarItem::Dropdown {
                            id: id.to_string(),
                            icon: icon.as_ref().map(|i| IconId::new(i.name())),
                            text: text.clone(),
                            active: false,
                            show_chevron: false, // Toolbar dropdowns don't show chevrons (matches terminal)
                            min_width: *min_width,
                        }
                    }
                }
            }
        }).collect();

        let align = match section.align {
            SectionAlign::End => TcSectionAlign::End,
            SectionAlign::Start => TcSectionAlign::Start,
        };

        ToolbarSection {
            items,
            show_separator: section.show_separator,
            align,
        }
    }).collect()
}

/// Convert a `TcToolbarResult` (toolbar_core) into `ToolbarRenderResult` (toolbar_render).
///
/// Needed because the hit-test code and dropdown rendering use `ToolbarRenderResult`
/// with `ToolbarRect`, while `draw_toolbar_with_icons` returns `ToolbarResult` with `WidgetRect`.
fn tc_result_to_render_result(result: TcToolbarResult) -> ToolbarRenderResult {
    ToolbarRenderResult {
        item_rects: result.item_rects.into_iter().map(|(id, wr)| {
            (id, ToolbarRect::new(wr.x, wr.y, wr.width, wr.height))
        }).collect(),
    }
}

/// Search a PanelToolbarDef for a dropdown with the given id and return its items.
fn find_items_in_def(def: &PanelToolbarDef, dropdown_id: &str) -> Option<Vec<DropdownItemDef>> {
    for section in &def.sections {
        for item in &section.items {
            if let uzor::panel_api::ToolbarItemDef::Dropdown { id, items, .. } = item {
                if *id == dropdown_id {
                    return Some(items.clone());
                }
            }
        }
    }
    None
}

/// Search a PanelToolbarDef for a Submenu with the given id inside any Dropdown,
/// and return its items and grid_columns.
fn find_submenu_in_def(def: &PanelToolbarDef, submenu_id: &str) -> Option<(Vec<DropdownItemDef>, Option<u8>)> {
    for section in &def.sections {
        for item in &section.items {
            if let uzor::panel_api::ToolbarItemDef::Dropdown { items, .. } = item {
                for dd_item in items {
                    if let DropdownItemDef::Submenu { id, items: sub_items, grid_columns, .. } = dd_item {
                        if *id == submenu_id {
                            return Some((sub_items.clone(), *grid_columns));
                        }
                    }
                }
            }
        }
    }
    None
}

/// Draw an icon using the chart's own SVG icon registry.
///
/// Used as the icon renderer callback for both the main dropdown and submenu grids.
fn chart_icon_renderer(ctx: &mut dyn RenderContext, icon_id: &IconId, rect: WidgetRect, color: &str) {
    if let Some(svg) = crate::ui::icons::icon_svg(icon_id.name()) {
        draw_svg_icon(ctx, svg, rect.x.floor(), rect.y.floor(), rect.width, rect.height, color);
    }
}

/// Convert a DropdownItemDef (from uzor-panel-api) to a DropdownItem (chart's own type).
fn convert_dropdown_item(item: &DropdownItemDef) -> DropdownItem {
    match item {
        DropdownItemDef::Action { id, label, icon, shortcut } => {
            let mut di = DropdownItem::item(id, label);
            if let Some(icon_id) = icon {
                di = di.with_icon(IconId::new(icon_id.name()));
            }
            if let Some(sc) = shortcut {
                di = di.with_shortcut(sc);
            }
            di
        }
        DropdownItemDef::ActionWithInput { id, label, icon, field_id, default_text } => {
            let mut di = DropdownItem::item(id, label)
                .with_param_field(field_id.clone(), default_text.clone());
            if let Some(icon_id) = icon {
                di = di.with_icon(IconId::new(icon_id.name()));
            }
            di
        }
        DropdownItemDef::Header { label } => {
            DropdownItem::header(label)
        }
        DropdownItemDef::Separator => {
            DropdownItem::separator()
        }
        DropdownItemDef::Submenu { id, label, .. } => {
            DropdownItem::submenu(id, label)
        }
    }
}

// `ChartPanelApp` moved to `mlc-app::chart_panel_app` — it is squarely
// app-level state (UserManager + ChartPanelGrid + modal states + active
// preset id).  This file keeps only the pure rendering helpers.
