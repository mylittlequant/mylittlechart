//! PanelApp implementation for the chart panel
//!
//! Makes the chart panel an autonomous application that owns its toolbar
//! and handles its own drawing tool state.

use uzor::panel_api::{
    PanelApp, PanelToolbarDef, PanelRect, PanelInput, PanelTheme,
    HitZone, ToolbarPosition, DropdownItemDef, ToolbarItemDef, SectionAlign,
    ToolbarIconId,
};
use uzor::render::{RenderContext, draw_svg_icon};
use crate::layout::LayoutRect;
use crate::layout::{ChartAreaLayout, ChartRenderConfig, ScaleCornerState, render_chart_window, ExtendedFrameLayout, FrameTheme};
use crate::chart_render::{ScaleTheme, ChartRenderState, ChartTheme, ChartRect};
use mlc_core::state::{ChartPanelGrid, ChartContainer, ChartPanelState, Timeframe};
use mlc_core::scale_settings::ScaleSettings;
use mlc_core::toolbar;
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
use mlc_core::modal::ChartOpenModal;
use mlc_core::state::selected_config::SelectedPrimitiveConfig;
use crate::ui::modal_settings::{PrimitiveSettingsState, IndicatorSettingsState, ChartSettingsState, IndicatorOverlayState, OverlaySettingsState, TagsTabsState, PresetNameInputState, ChartBrowserState, AlertSettingsState, CompareSettingsState};
use mlc_core::templates::TemplateManager;
use mlc_core::user_manager::UserManager;
use crate::ui::context_menu::ContextMenuState;
use crate::layout::render_frame::{ChartModalRenderResult, ChartModalLayout};
use mlc_core::indicator_source::IndicatorSource;
use crate::layout::modals::{
    render_primitive_settings_modal,
    render_primitive_color_picker_popup,
    render_indicator_settings_modal,
    render_indicator_color_picker_popup,
    render_chart_settings_color_picker_popup,
    render_settings_modal, ChartSettingsData,
    render_overlay_settings_modal,
    render_panel_color_tag_picker_popup,
    render_tags_tabs_modal,
};

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
pub fn chart_toolbar_render_toolbars(
    tb: &ChartToolbarState,
    ctx: &mut dyn RenderContext,
    layout: &ChartPanelLayout,
    toolbar_config: &ToolbarConfig,
    selected_primitive: Option<&SelectedPrimitiveConfig>,
    toolbar_theme: Option<&crate::ui::toolbar_render::ToolbarTheme>,
    dropdown_theme: Option<&crate::ui::dropdown::DropdownTheme>,
    clock_time: Option<&str>,
    presets: &std::collections::HashMap<String, mlc_core::preset::preset::ChartPreset>,
    active_preset_id: &str,
    autosave_enabled: bool,
    sync_flags: Option<&mlc_core::tag_manager::SyncFlags>,
    is_expanded: bool,
    split_without_group: bool,
    is_mono_group: bool,
    active_symbol: Option<&str>,
    active_timeframe: Option<&str>,
    sidebar_w: f64,
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
            chart_toolbar_render_dropdown(tb, ctx, dropdown_id, toolbar_config, layout, &drawing_result, &control_result, dd_theme_ref, presets, active_preset_id, autosave_enabled, sync_flags, is_expanded, split_without_group, is_mono_group)
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
    presets: &std::collections::HashMap<String, mlc_core::preset::preset::ChartPreset>,
    active_preset_id: &str,
    autosave_enabled: bool,
    sync_flags: Option<&mlc_core::tag_manager::SyncFlags>,
    _is_expanded: bool,
    split_without_group: bool,
    is_mono_group: bool,
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
            di
        }).collect();

        if dropdown_items.is_empty() {
            return None;
        }

        // 5. Build config and render the main dropdown
        let config = DropdownConfig::new(dropdown_items);

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
    presets: &std::collections::HashMap<String, mlc_core::preset::preset::ChartPreset>,
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

            let mut named_presets: Vec<_> = presets.values()
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
                    let summary = preset.windows.first()
                        .map(|w| format!("{}, {}", w.symbol, w.timeframe.name))
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
            let mut named: Vec<_> = presets.values()
                .filter(|p| p.id != "__default__")
                .collect();
            named.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            let max = named.len().min(5);

            if max > 0 {
                items.push(DropdownItemDef::Separator);
                items.push(DropdownItemDef::Header { label: "RECENT".to_string() });
                for preset in &named[..max] {
                    let load_id = format!("new_tab:open:{}", preset.id);
                    let summary = preset.windows.first()
                        .map(|w| format!("{}, {}", w.symbol, w.timeframe.name))
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

/// Chart panel — autonomous application implementing PanelApp.
///
/// Owns both the toolbar state and the chart data (via ChartPanelGrid).
/// This makes the chart panel fully self-contained: it can render its own
/// content (including split sub-charts) without depending on core.
pub struct ChartPanelApp {
    pub toolbar_state: ChartToolbarState,
    title: String,
    /// Per-toolbar visibility and definition configuration for this panel.
    pub toolbar_config: ToolbarConfig,
    /// Which chart-local modal is currently open.
    pub active_modal: ChartOpenModal,
    /// Chart data and split layout manager.
    ///
    /// Holds all ChartContainer instances (one per split leaf) and the
    /// DockingManager that computes each sub-chart's rectangle.
    pub panel_grid: ChartPanelGrid,
    /// Chart's local theme manager — subordinate to terminal's ThemeManager.
    ///
    /// The terminal calls `apply_terminal_theme` to command this manager.
    /// All chart rendering (splits, toolbars, scales, candles) derives
    /// colors from this manager.
    pub theme_manager: mlc_core::theme::ThemeManager,

    // Modal states — owned per chart panel so each chart has independent modal UI.
    /// Primitive (drawing) settings modal state.
    pub primitive_settings_state: PrimitiveSettingsState,
    /// Chart settings modal state.
    pub chart_settings_state: ChartSettingsState,
    /// Indicator settings modal state.
    pub indicator_settings_state: IndicatorSettingsState,
    /// Indicator overlay state (used in single-window mode).
    pub indicator_overlay_state: IndicatorOverlayState,
    /// Per-leaf indicator overlay states (used in split mode).
    pub indicator_overlay_states: std::collections::HashMap<uzor::panels::LeafId, IndicatorOverlayState>,
    /// Context menu state (right-click menu).
    pub context_menu_state: ContextMenuState,
    /// Overlay (leaf) settings modal state.
    pub overlay_settings_state: OverlaySettingsState,
    /// Tags & Tabs modal state (unified panel-tree + sync-group manager).
    pub tags_tabs_state: TagsTabsState,
    /// Alert settings modal state.
    pub alert_settings_state: AlertSettingsState,
    /// Compare settings modal state.
    pub compare_settings_state: CompareSettingsState,
    /// User settings modal state.
    pub user_settings_state: crate::ui::modal_settings::UserSettingsState,
    /// Clock popup state (timezone selector + 24h toggle).
    pub clock_popup_state: crate::ui::ClockPopupState,

    // Panel color tag picker — opened when the user clicks the colored square
    // on an overlay tab header.
    /// Color picker state for the panel color tag picker.
    pub panel_color_picker: crate::ui::color_picker_state::ColorPickerState,
    /// Which leaf's color tag is being edited.
    pub panel_color_picker_leaf: Option<uzor::panels::LeafId>,
    /// Per-leaf color tags (RGBA, 0.0–1.0 each channel).
    pub leaf_color_tags: std::collections::HashMap<uzor::panels::LeafId, [f32; 4]>,

    // Sync color grid popup — lightweight preset grid that replaces the full
    // L1/L2 picker for panel color-tag assignment.
    /// State for the sync color grid popup.
    pub sync_color_grid: crate::ui::sync_color_grid::SyncColorGridState,
    /// Draw result from the last rendered sync color grid frame (used for hit testing).
    pub sync_color_grid_draw: crate::ui::sync_color_grid::SyncColorGridDrawResult,

    // Chart presets — in-memory store. Persist layer binds to this later.
    pub presets: std::collections::HashMap<String, mlc_core::preset::preset::ChartPreset>,
    /// ID of the currently active preset. Defaults to `"__default__"`.
    /// Updated when a preset is loaded or saved-as.
    pub active_preset_id: String,
    /// Ordered list of preset IDs that are currently open as tabs.
    /// A preset can exist in `self.presets` without being open.
    pub open_tabs: Vec<String>,
    /// Preset name input modal state (Save As / Rename).
    pub preset_name_input: PresetNameInputState,
    /// Chart browser modal state (Open Chart).
    pub chart_browser: ChartBrowserState,
    /// Whether autosave is enabled for presets.
    pub autosave_enabled: bool,

    /// In-memory template manager (drawing primitive / indicator / compare templates).
    pub template_manager: TemplateManager,

    /// Unified user state manager — owns profile and settings snapshots.
    ///
    /// Templates and presets are the source of truth in `template_manager` and
    /// `presets` respectively. `user_manager` holds the remainder: profile
    /// metadata (`active_preset_id`, sidebar state) and `SettingsSnapshots`.
    pub user_manager: UserManager,
}

impl ChartPanelApp {
    pub fn new(title: &str) -> Self {
        let initial_window = ChartContainer::new(title, Timeframe::h1());
        Self {
            toolbar_state: ChartToolbarState::new(),
            title: title.to_string(),
            toolbar_config: ToolbarConfig::standalone(),
            active_modal: ChartOpenModal::None,
            panel_grid: ChartPanelGrid::new(initial_window),
            theme_manager: mlc_core::theme::ThemeManager::new(),
            primitive_settings_state: PrimitiveSettingsState::new(),
            chart_settings_state: ChartSettingsState::new(),
            indicator_settings_state: IndicatorSettingsState::new(),
            indicator_overlay_state: IndicatorOverlayState::default(),
            indicator_overlay_states: std::collections::HashMap::new(),
            context_menu_state: ContextMenuState::new(),
            overlay_settings_state: OverlaySettingsState::new(),
            tags_tabs_state: TagsTabsState::default(),
            alert_settings_state: AlertSettingsState::new(),
            compare_settings_state: CompareSettingsState::new(),
            user_settings_state: crate::ui::modal_settings::UserSettingsState::default(),
            clock_popup_state: crate::ui::ClockPopupState::default(),
            panel_color_picker: crate::ui::color_picker_state::ColorPickerState::new(),
            panel_color_picker_leaf: None,
            leaf_color_tags: std::collections::HashMap::new(),
            sync_color_grid: crate::ui::sync_color_grid::SyncColorGridState::new(),
            sync_color_grid_draw: crate::ui::sync_color_grid::SyncColorGridDrawResult::default(),
            presets: std::collections::HashMap::new(),
            active_preset_id: "__default__".to_string(),
            open_tabs: Vec::new(),
            preset_name_input: PresetNameInputState::default(),
            chart_browser: ChartBrowserState::default(),
            autosave_enabled: true,
            template_manager: TemplateManager::new(),
            user_manager: UserManager::new(),
        }
    }

    /// Load all user state from disk and populate the in-memory fields.
    ///
    /// Call once at startup after construction. On success, `template_manager`,
    /// `presets`, and `active_preset_id` are populated from disk. The
    /// `user_manager` field retains the profile and settings snapshots.
    ///
    /// All errors are logged but non-fatal: missing files produce defaults.
    pub fn load_user_state(&mut self) {
        let um = UserManager::load();

        // Move templates and presets into the canonical in-memory fields.
        self.template_manager = um.template_manager;
        self.presets = um.presets;
        if !um.profile.active_preset_id.is_empty() {
            self.active_preset_id = um.profile.active_preset_id.clone();
        }

        // Restore open_tabs and filter out any stale IDs (presets no longer on disk).
        self.open_tabs = um.profile.open_tabs.clone();
        self.open_tabs.retain(|id| self.presets.contains_key(id));

        // Migration: old data has no open_tabs — open only the active preset.
        if self.open_tabs.is_empty() && !self.presets.is_empty() {
            if !self.active_preset_id.is_empty()
                && self.presets.contains_key(&self.active_preset_id)
            {
                self.open_tabs = vec![self.active_preset_id.clone()];
            } else {
                let mut all: Vec<_> = self.presets.values().collect();
                all.sort_by_key(|p| std::cmp::Reverse(p.created_at));
                if let Some(newest) = all.first() {
                    self.open_tabs = vec![newest.id.clone()];
                }
            }
        }

        // Keep profile + snapshots in user_manager.
        self.user_manager.profile = um.profile;
        self.user_manager.snapshots = um.snapshots;

        // Record this launch.
        self.user_manager.profile.record_launch(env!("CARGO_PKG_VERSION"));

        // Save profile immediately to persist the updated launch record.
        self.user_manager.save_profile();

        eprintln!(
            "[ChartPanelApp] user state loaded: {} presets, {} prim-templates, {} ind-templates",
            self.presets.len(),
            self.template_manager.primitive_templates.len(),
            self.template_manager.indicator_templates.len(),
        );
    }

    /// Sync the current in-memory state into `user_manager` and persist everything to disk.
    ///
    /// Saves profile, templates, presets, and settings snapshots. Errors are
    /// logged but non-fatal so that a save failure never crashes the app.
    pub fn save_user_state(&mut self) {
        // NOTE: Do NOT call self.user_manager.save_profile() here!
        // The per-window UserManager holds a stale copy of UserProfile
        // (with outdated window positions/sizes). The authoritative profile
        // save happens in App::save_all() which has the correct multi-window
        // state.

        // Templates are saved by App::save_all() from AppState.template_manager
        // (single source of truth shared across all windows via action queue).

        // Presets are saved by App::save_all() from AppState (single source of truth).

        // Settings snapshots are saved by App::save_all() from AppState.snapshots
        // (single source of truth for multi-window). Do NOT call save_snapshots()
        // here — the per-window UserManager holds a copy that may be stale.

        eprintln!("[ChartPanelApp] user state saved to disk (templates/presets/snapshots handled by App::save_all)");
    }

    /// Return a mutable reference to the per-leaf indicator overlay state.
    ///
    /// Creates a default state entry if one does not exist yet for the given leaf.
    pub fn indicator_overlay_state_for_leaf_mut(
        &mut self,
        leaf_id: uzor::panels::LeafId,
    ) -> &mut IndicatorOverlayState {
        self.indicator_overlay_states
            .entry(leaf_id)
            .or_default()
    }

    /// Return a shared reference to the per-leaf indicator overlay state.
    ///
    /// Returns a reference to a default state if one does not exist for the leaf.
    /// The state is lazily inserted on first mutable access; if it has not been
    /// created yet this will fall back to the single-window state as a sentinel.
    pub fn indicator_overlay_state_for_leaf(
        &self,
        leaf_id: uzor::panels::LeafId,
    ) -> Option<&IndicatorOverlayState> {
        self.indicator_overlay_states.get(&leaf_id)
    }

    /// Called by terminal when global theme changes.
    ///
    /// Updates this chart's internal ThemeManager to match the terminal preset,
    /// marking it dirty so the next render picks up the new colors.
    pub fn apply_terminal_theme(&mut self, preset_name: &str) {
        self.theme_manager.set_preset(preset_name);
    }

    /// Build a `ChartTheme` render struct from the current theme_manager state.
    ///
    /// Used by `render_chart_content` and other internal rendering paths.
    pub fn chart_theme_for_render(&self) -> ChartTheme {
        let rt = self.theme_manager.current();
        ChartTheme {
            background: rt.chart.background.clone(),
            grid_line: rt.chart.grid_line.clone(),
            text: rt.colors.text_primary.clone(),
            candle_up: rt.series.candle_up_body.clone(),
            candle_down: rt.series.candle_down_body.clone(),
            wick_up: rt.series.candle_up_wick.clone(),
            wick_down: rt.series.candle_down_wick.clone(),
            candle_up_border: rt.series.candle_up_border.clone(),
            candle_down_border: rt.series.candle_down_border.clone(),
            legend_value_up: rt.chart.legend_value_up.clone(),
            legend_value_down: rt.chart.legend_value_down.clone(),
            crosshair: rt.chart.crosshair_line.clone(),
            scale_bg: rt.chart.scale_bg.clone(),
            scale_border: rt.chart.scale_border.clone(),
            sub_pane_bg: rt.chart.background.clone(),
        }
    }

    /// Build a `ScaleTheme` render struct from the current theme_manager state.
    ///
    /// Maps RuntimeTheme color fields to every ScaleTheme slot so that price
    /// and time scales, the scale corner, and crosshair labels all respect the
    /// active theme preset.
    pub fn scale_theme_for_render(&self) -> ScaleTheme {
        let rt = self.theme_manager.current();
        ScaleTheme {
            scale_bg:                   rt.colors.toolbar_bg.clone(),
            scale_border:               rt.chart.scale_border.clone(),
            scale_text:                 rt.chart.scale_text.clone(),
            scale_text_medium:          rt.chart.scale_text_muted.clone(),
            scale_text_muted:           rt.chart.scale_text_muted.clone(),
            crosshair_label_bg:         rt.chart.crosshair_label_bg.clone(),
            crosshair_label_bg_styled:  rt.crosshair_label_bg_styled(),
            crosshair_label_text:       rt.chart.crosshair_label_text.clone(),
        }
    }

    /// Build a `FrameTheme` render struct from the current theme_manager state.
    ///
    /// Maps RuntimeTheme color fields to every FrameTheme slot so that chart
    /// frame borders and modal backgrounds all respect the active theme preset.
    pub fn frame_theme_for_render(&self) -> FrameTheme {
        let rt = self.theme_manager.current();
        FrameTheme {
            toolbar_bg:           rt.colors.toolbar_bg.clone(),
            toolbar_border:       rt.colors.ui_border.clone(),
            chart_border:         rt.chart.chart_border.clone(),
            frame_border:         rt.chart.frame_border.clone(),
            show_scale_separators: true,
        }
    }

    /// Build a `DropdownTheme` from the current theme_manager state.
    ///
    /// Maps `RuntimeTheme` color fields to every `DropdownTheme` slot so that
    /// dropdown menus, submenus and context menus all respect the active theme.
    pub fn dropdown_theme_for_render(&self) -> crate::ui::dropdown::DropdownTheme {
        let rt = self.theme_manager.current();
        crate::ui::dropdown::DropdownTheme {
            background:            rt.colors.dropdown_bg.clone(),
            border:                rt.colors.border.clone(),
            shadow:                "rgba(0,0,0,0.5)".to_string(),
            item_text:             rt.colors.text_primary.clone(),
            item_text_hover:       rt.colors.text_primary.clone(),
            item_text_disabled:    rt.colors.text_muted.clone(),
            item_bg_hover:         rt.colors.button_bg_hover.clone(),
            item_danger:           rt.colors.danger.clone(),
            item_danger_bg_hover:  "rgba(0,0,0,0.15)".to_string(), // always semi-transparent overlay
            header_text:           rt.colors.text_primary.clone(),
            header_border:         rt.colors.border.clone(),
            separator:             rt.colors.divider.clone(),
            shortcut_text:         rt.colors.text_muted.clone(),
        }
    }

    /// Build a `ToolbarTheme` (toolbar_render) from the current theme_manager state.
    ///
    /// Used by `render_toolbars_with_theme` so chart toolbars match the active theme.
    pub fn toolbar_theme_for_render(&self) -> crate::ui::toolbar_render::ToolbarTheme {
        let rt = self.theme_manager.current();
        crate::ui::toolbar_render::ToolbarTheme {
            background: rt.colors.toolbar_bg.clone(),
            dropdown_bg: rt.colors.dropdown_bg.clone(),
            separator: rt.colors.toolbar_divider.clone(),
            item_bg_hover: rt.colors.button_bg_hover.clone(),
            item_bg_active: rt.colors.button_bg_active.clone(),
            button_bg:       rt.colors.button_bg.clone(),
            button_bg_hover: rt.colors.button_bg_hover.clone(),
            item_text: rt.colors.text_primary.clone(),
            item_text_muted: rt.colors.text_secondary.clone(),
            item_text_hidden: mlc_core::apply_opacity(&rt.colors.text_primary, 0.5),
            item_text_hover: rt.colors.text_primary.clone(),
            item_text_active: rt.colors.text_active.clone(),
            accent: rt.colors.accent.clone(),
            accent_hover: rt.colors.accent_hover.clone(),
            success: rt.colors.success.clone(),
            danger: rt.colors.danger.clone(),
            warning: rt.colors.warning.clone(),
            sidebar_style: rt.style_params.toolbar_sidebar_style,
            terminal_bg: rt.colors.toolbar_bg.clone(),
            pane_header_focused: rt.colors.accent.clone(),
            pane_header_hover: rt.colors.button_bg_hover.clone(),
            pane_header_idle: rt.colors.toolbar_bg.clone(),
            bubble_user_bg: rt.colors.bubble_user_bg.clone(),
            bubble_user_border: rt.colors.accent.clone(),
            bubble_user_text: rt.colors.text_primary.clone(),
            bubble_tool_bg: rt.colors.toolbar_bg.clone(),
            bubble_tool_text: rt.colors.text_secondary.clone(),
            selection: rt.colors.text_selection.clone(),
            danger_hover_bg: mlc_core::apply_opacity(&rt.colors.danger, 0.25),
        }
    }

    /// Render chart toolbars using this panel's ThemeManager for colors.
    ///
    /// Delegates to `toolbar_state.render_toolbars` with the theme built from
    /// the chart's own ThemeManager (which is commanded by the terminal).
    /// Uses `self.toolbar_config` to determine which toolbars to render.
    pub fn render_toolbars_with_theme(
        &self,
        ctx: &mut dyn RenderContext,
        layout: &ChartPanelLayout,
        selected_primitive: Option<&SelectedPrimitiveConfig>,
        clock_time: Option<&str>,
        split_without_group: bool,
        active_symbol: Option<&str>,
        active_timeframe: Option<&str>,
        sidebar_w: f64,
        active_group_id: Option<mlc_core::tag_manager::BubbleId>,
        bubble_store: &mlc_core::tag_manager::BubbleStore,
    ) -> ChartToolbarRenderResult {
        let toolbar_theme = self.toolbar_theme_for_render();
        let dropdown_theme = self.dropdown_theme_for_render();
        // Get sync flags for the active window's group (used to show toggle state in layout_menu)
        let active_gid = active_group_id;
        let sync_flags = active_gid.and_then(|gid| bubble_store.bubble(gid)).map(|g| &g.sync_flags);
        let is_mono_group = active_gid.and_then(|gid| bubble_store.bubble(gid)).map(|g| g.members.len() <= 1).unwrap_or(true);
        let is_expanded = self.panel_grid.is_expanded();
        chart_toolbar_render_toolbars(&self.toolbar_state, ctx, layout, &self.toolbar_config, selected_primitive, Some(&toolbar_theme), Some(&dropdown_theme), clock_time, &self.presets, &self.active_preset_id, self.autosave_enabled, sync_flags, is_expanded, split_without_group, is_mono_group, active_symbol, active_timeframe, sidebar_w)
    }

    /// Render all chart-owned modals in a single call.
    ///
    /// This method encapsulates the rendering of:
    /// - Primitive settings modal (if open)
    /// - Primitive color picker popup (if open)
    /// - Indicator settings modal (if open, requires `indicator_source`)
    /// - Indicator color picker popup (if open)
    /// - Chart settings color picker popup (if open)
    ///
    /// The context menu, panel tree manager, and panel color tag picker stay in
    /// core because they are not chart-specific.
    ///
    /// # Parameters
    /// - `ctx` — render context
    /// - `modal_layout` — positional data for modal placement (computed from frame layout by core)
    /// - `drawing_manager` — active window's drawing manager (for primitive settings)
    /// - `indicator_source` — indicator data source (for indicator settings modal)
    /// - `chart_settings_data` — chart settings data for the chart settings modal (if open)
    /// - `theme_manager` — theme manager (for appearance tab)
    /// - `frame_theme` — frame theme for modal backgrounds
    /// - `toolbar_theme` — toolbar theme for color pickers
    /// - `toolbar_state` — toolbar state (passed through to primitive settings)
    /// - `current_time_ms` — current timestamp for cursor blink
    /// - `input_coordinator` — uzor input coordinator
    pub fn render_modals(
        &self,
        ctx: &mut dyn mlc_core::render::RenderContext,
        modal_layout: &ChartModalLayout,
        drawing_manager: Option<&mlc_core::drawing::DrawingManager>,
        indicator_source: Option<&dyn IndicatorSource>,
        chart_settings_data: Option<&ChartSettingsData>,
        theme_manager: Option<&mlc_core::theme::ThemeManager>,
        frame_theme: &crate::layout::FrameTheme,
        toolbar_theme: &crate::ui::toolbar_render::ToolbarTheme,
        toolbar_state: &crate::layout::toolbar_state::ToolbarState,
        current_time_ms: u64,
        input_coordinator: &mut uzor::input::InputCoordinator,
        bubble_store: &mlc_core::tag_manager::BubbleStore,
    ) -> ChartModalRenderResult {
        let mut result = ChartModalRenderResult::default();

        // Primitive settings modal
        if self.primitive_settings_state.is_open() {
            if let Some(dm) = drawing_manager {
                result.primitive_settings = Some(render_primitive_settings_modal(
                    ctx,
                    modal_layout.prim_screen_w,
                    modal_layout.prim_screen_h,
                    modal_layout.prim_modal_y,
                    &self.primitive_settings_state,
                    dm,
                    frame_theme,
                    toolbar_theme,
                    toolbar_state,
                    current_time_ms,
                    input_coordinator,
                    &self.template_manager.primitive_templates,
                ));
            }
        }

        // Compare settings modal
        if self.compare_settings_state.is_open() {
            result.compare_settings = Some(crate::layout::modals::compare_settings::render_compare_settings_modal(
                ctx,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                &self.compare_settings_state,
                frame_theme,
                toolbar_theme,
                input_coordinator,
                &self.template_manager.compare_templates,
            ));
        }

        // Alert settings modal
        if self.alert_settings_state.is_open() {
            result.alert_settings = Some(crate::layout::modals::alert_settings::render_alert_settings_modal(
                ctx,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                &self.alert_settings_state,
                frame_theme,
                toolbar_theme,
                input_coordinator,
            ));
        }

        // Indicator settings modal
        if self.indicator_settings_state.is_open() {
            if let Some(indicator_id) = self.indicator_settings_state.indicator_id {
                if let Some(source) = indicator_source {
                    if let Some(settings_data) = source.get_settings_data(indicator_id) {
                        result.indicator_settings = Some(render_indicator_settings_modal(
                            ctx,
                            modal_layout.ind_screen_w,
                            modal_layout.ind_screen_h,
                            modal_layout.chart_x,
                            modal_layout.chart_y,
                            &self.indicator_settings_state,
                            &settings_data.name,
                            &settings_data.params,
                            &settings_data.outputs,
                            settings_data.display_info.as_ref(),
                            settings_data.signals_enabled,
                            &settings_data.signal_display,
                            settings_data.timeframe_visibility.as_ref(),
                            current_time_ms,
                            frame_theme,
                            toolbar_theme,
                            input_coordinator,
                            &self.template_manager.indicator_templates,
                        ));
                    }
                }
            }
        }

        // Chart settings modal
        if self.chart_settings_state.is_open {
            if let (Some(data), Some(tm)) = (chart_settings_data, theme_manager) {
                result.chart_settings = Some(render_settings_modal(
                    ctx,
                    modal_layout.prim_screen_w,
                    modal_layout.prim_screen_h,
                    modal_layout.chart_x,
                    modal_layout.chart_y,
                    &self.chart_settings_state,
                    data,
                    tm,
                    frame_theme,
                    toolbar_theme,
                    current_time_ms,
                    input_coordinator,
                    &self.template_manager.chart_templates,
                ));
            }
        }

        // Overlay settings modal
        if self.overlay_settings_state.is_open {
            result.overlay_settings = Some(render_overlay_settings_modal(
                ctx,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                modal_layout.chart_x,
                modal_layout.chart_y,
                &self.overlay_settings_state,
                frame_theme,
                toolbar_theme,
                &self.theme_manager,
                &self.panel_grid,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                input_coordinator,
            ));
        }

        // Tags & Tabs modal
        if self.tags_tabs_state.is_open {
            result.tags_tabs = Some(render_tags_tabs_modal(
                ctx,
                &self.tags_tabs_state,
                &self.overlay_settings_state,
                frame_theme,
                toolbar_theme,
                &self.panel_grid,
                bubble_store,
                &self.leaf_color_tags,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                &self.theme_manager,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                input_coordinator,
            ));
        }

        // Preset name input modal — skip CreateIndicatorSet mode here;
        // it will be rendered AFTER the search overlay in lib.rs so it
        // appears visually on top.
        if self.preset_name_input.is_open
            && self.preset_name_input.mode != crate::ui::modal_settings::PresetNameInputMode::CreateIndicatorSet
        {
            use crate::layout::modals::preset_name_input::render_preset_name_input;
            result.preset_name_input = Some(render_preset_name_input(
                ctx,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                &self.preset_name_input,
                frame_theme,
                toolbar_theme,
                current_time_ms,
                input_coordinator,
            ));
        }

        // Template name overlay modal — primitive settings
        if self.primitive_settings_state.save_template_mode {
            if let Some(ref editing) = self.primitive_settings_state.template_name_editing {
                use crate::layout::modals::template_name_modal::render_template_name_modal;
                result.prim_template_name = Some(render_template_name_modal(
                    ctx,
                    modal_layout.prim_screen_w,
                    modal_layout.prim_screen_h,
                    editing,
                    "prim_tmpl",
                    frame_theme,
                    toolbar_theme,
                    current_time_ms,
                    input_coordinator,
                ));
            }
        }

        // Template name overlay modal — indicator settings
        if self.indicator_settings_state.save_template_mode {
            if let Some(ref editing) = self.indicator_settings_state.template_name_editing {
                use crate::layout::modals::template_name_modal::render_template_name_modal;
                result.ind_template_name = Some(render_template_name_modal(
                    ctx,
                    modal_layout.prim_screen_w,
                    modal_layout.prim_screen_h,
                    editing,
                    "ind_tmpl",
                    frame_theme,
                    toolbar_theme,
                    current_time_ms,
                    input_coordinator,
                ));
            }
        }

        // Template name overlay modal — compare settings
        if self.compare_settings_state.save_template_mode {
            if let Some(ref editing) = self.compare_settings_state.template_name_editing {
                use crate::layout::modals::template_name_modal::render_template_name_modal;
                result.cmp_template_name = Some(render_template_name_modal(
                    ctx,
                    modal_layout.prim_screen_w,
                    modal_layout.prim_screen_h,
                    editing,
                    "cmp_tmpl",
                    frame_theme,
                    toolbar_theme,
                    current_time_ms,
                    input_coordinator,
                ));
            }
        }

        // Template name overlay modal — chart settings
        if self.chart_settings_state.save_template_mode {
            if let Some(ref editing) = self.chart_settings_state.template_name_editing {
                use crate::layout::modals::template_name_modal::render_template_name_modal;
                result.chart_template_name = Some(render_template_name_modal(
                    ctx,
                    modal_layout.prim_screen_w,
                    modal_layout.prim_screen_h,
                    editing,
                    "chart_tmpl",
                    frame_theme,
                    toolbar_theme,
                    current_time_ms,
                    input_coordinator,
                ));
            }
        }

        // Chart browser modal
        if self.chart_browser.is_open {
            use crate::layout::modals::chart_browser::render_chart_browser;
            result.chart_browser = Some(render_chart_browser(
                ctx,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                &self.chart_browser,
                &self.presets,
                &self.active_preset_id,
                frame_theme,
                toolbar_theme,
                current_time_ms,
                input_coordinator,
            ));
        }

        // User settings modal
        if self.user_settings_state.is_open {
            use crate::layout::modals::user_settings::render_user_settings_modal;
            result.user_settings = Some(render_user_settings_modal(
                ctx,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                &self.user_settings_state,
                frame_theme,
                toolbar_theme,
                current_time_ms,
                input_coordinator,
            ));
        }

        // Skeleton overlay — opaque background fill that hides chart content
        // while the vault unlock / welcome wizard is shown.  Drawn BEFORE the
        // modal so the modal appears on top of the solid background.
        //
        // Fill only the chart content area (between the toolbars). The
        // VelloGpu path composites toolbars on top of the chart scene after
        // this overlay, so the old "fill the whole window" code worked there
        // by accident; the VelloCpu path renders everything into a single
        // pixel buffer in order (toolbars first, then chart + skeleton
        // overlay), so a full-window fill blanked out the left/top/bottom
        // toolbars that had already been drawn. Restricting the overlay to
        // the content rect keeps the toolbars visible on every backend.
        if self.user_settings_state.show_welcome_wizard
            || self.user_settings_state.show_profile_manager
        {
            ctx.set_fill_color(&toolbar_theme.button_bg);
            ctx.fill_rect(
                modal_layout.chart_x,
                modal_layout.chart_y,
                modal_layout.content_w,
                modal_layout.content_h,
            );
        }

        // Welcome Wizard — rendered on top of everything when active.
        // This is shown on first launch (no profile.json existed at startup).
        // It is non-closeable: the user must pick a mode to proceed.
        if self.user_settings_state.show_welcome_wizard {
            use crate::layout::modals::welcome_wizard::render_welcome_wizard;
            let text_color = &toolbar_theme.item_text.clone();
            if result.user_settings.is_none() {
                result.user_settings = Some(Default::default());
            }
            if let Some(ref mut ws_result) = result.user_settings {
                render_welcome_wizard(
                    ctx,
                    modal_layout.prim_screen_w,
                    modal_layout.prim_screen_h,
                    &self.user_settings_state,
                    text_color,
                    toolbar_theme,
                    frame_theme,
                    current_time_ms,
                    input_coordinator,
                    ws_result,
                );
            }
        }

        // Profile Manager overlay — unified modal for profile selection, vault unlock,
        // passphrase creation, and new profile creation.
        // Replaces the old vault_unlock overlay and vault_profile_picker.
        //
        // In skeleton mode the content area is passed so the manager can render
        // as a full-area login screen rather than a small centered modal.
        if self.user_settings_state.show_profile_manager {
            use crate::layout::modals::profile_manager::render_profile_manager;
            let text_color = &toolbar_theme.item_text.clone();
            if result.user_settings.is_none() {
                result.user_settings = Some(Default::default());
            }
            if let Some(ref mut ws_result) = result.user_settings {
                render_profile_manager(
                    ctx,
                    modal_layout.chart_x,
                    modal_layout.chart_y,
                    modal_layout.content_w,
                    modal_layout.content_h,
                    &self.user_settings_state,
                    text_color,
                    toolbar_theme,
                    frame_theme,
                    current_time_ms,
                    input_coordinator,
                    ws_result,
                );
            }
        }

        result
    }

    /// Renders color picker popups and the sync color grid popup.
    ///
    /// Must be called AFTER the sidebar is rendered so that these popups
    /// are drawn on top of the sidebar (correct z-order).
    /// The returned [`ChartModalRenderResult`] only populates `color_picker`
    /// and `sync_color_grid`; all other fields are `None`.
    pub fn render_color_picker_popups(
        &self,
        ctx: &mut dyn mlc_core::render::RenderContext,
        modal_layout: &ChartModalLayout,
        toolbar_theme: &crate::ui::toolbar_render::ToolbarTheme,
        input_coordinator: &mut uzor::input::InputCoordinator,
        bubble_store: &mlc_core::tag_manager::BubbleStore,
    ) -> ChartModalRenderResult {
        let mut result = ChartModalRenderResult::default();

        // Primitive color picker popup (above primitive settings modal)
        if self.primitive_settings_state.is_color_picker_open() {
            result.color_picker = Some(render_primitive_color_picker_popup(
                ctx,
                &self.primitive_settings_state,
                toolbar_theme,
                input_coordinator,
            ));
        }

        // Compare settings color picker popup (rendered above the compare settings modal)
        if self.compare_settings_state.is_open() && self.compare_settings_state.is_color_picker_open() {
            result.color_picker = Some(crate::layout::modals::compare_color_picker::render_compare_color_picker_popup(
                ctx,
                &self.compare_settings_state,
                toolbar_theme,
                input_coordinator,
            ));
        }

        // Indicator color picker popup (above indicator settings modal)
        if self.indicator_settings_state.is_color_picker_open() {
            result.color_picker = Some(render_indicator_color_picker_popup(
                ctx,
                &self.indicator_settings_state,
                toolbar_theme,
                input_coordinator,
            ));
        }

        // Chart settings color picker popup
        if self.chart_settings_state.is_color_picker_open() {
            result.color_picker = Some(render_chart_settings_color_picker_popup(
                ctx,
                &self.chart_settings_state,
                toolbar_theme,
                input_coordinator,
            ));
        }

        // Panel color tag picker popup (legacy L1/L2 — kept for non-panel use)
        if self.panel_color_picker.is_open() {
            result.color_picker = Some(render_panel_color_tag_picker_popup(
                ctx,
                &self.panel_color_picker,
                toolbar_theme,
                input_coordinator,
            ));
        }

        // Sync color grid popup — lightweight preset grid for panel color tags
        if self.sync_color_grid.is_open() {
            use crate::ui::sync_color_grid::{draw_sync_color_grid, SyncColorGridDrawResult};
            use uzor::{Rect, input::Sense};
            use crate::ui::z_order::ZLayer;

            let current_color = if let Some(lid) = self.sync_color_grid.target_leaf {
                self.leaf_color_tags.get(&lid).copied()
            } else if let Some(panel_id) = self.sync_color_grid.target_panel {
                use mlc_core::tag_manager::SyncMemberId;
                let member = SyncMemberId::Panel(panel_id);
                bubble_store
                    .group_for_member(member)
                    .and_then(|gid| bubble_store.bubble(gid))
                    .filter(|g| !g.auto_created)
                    .map(|g| g.color)
            } else {
                None
            };

            let draw_result: SyncColorGridDrawResult =
                draw_sync_color_grid(ctx, &self.sync_color_grid, current_color, toolbar_theme);

            // Register input zones on the ColorPicker layer (topmost interactive).
            let layer_id = ZLayer::ColorPicker.push_named(input_coordinator, "sync_color_grid");

            // Full-screen backdrop to catch clicks outside the popup
            input_coordinator.register_on_layer(
                "sync_color_grid:backdrop",
                Rect { x: 0.0, y: 0.0, width: modal_layout.prim_screen_w, height: modal_layout.prim_screen_h },
                Sense::CLICK,
                &layer_id,
            );

            let [px, py, pw, ph] = draw_result.popup_rect;
            // Background / "absorb click inside popup" rect
            input_coordinator.register_on_layer(
                "sync_color_grid:bg",
                Rect { x: px, y: py, width: pw, height: ph },
                Sense::CLICK,
                &layer_id,
            );

            // Individual swatch rects
            for &(idx, [sx, sy, sw, sh]) in &draw_result.swatch_rects {
                input_coordinator.register_on_layer(
                    format!("sync_color_grid:swatch:{}", idx),
                    Rect { x: sx, y: sy, width: sw, height: sh },
                    Sense::CLICK,
                    &layer_id,
                );
            }

            // "+" add-custom button
            if let Some([ax, ay, aw, ah]) = draw_result.add_button_rect {
                input_coordinator.register_on_layer(
                    "sync_color_grid:add",
                    Rect { x: ax, y: ay, width: aw, height: ah },
                    Sense::CLICK,
                    &layer_id,
                );
            }

            // Remove row
            let [rx, ry, rw, rh] = draw_result.remove_rect;
            input_coordinator.register_on_layer(
                "sync_color_grid:remove",
                Rect { x: rx, y: ry, width: rw, height: rh },
                Sense::CLICK,
                &layer_id,
            );

            result.sync_color_grid = Some(draw_result);
        }

        result
    }

    pub fn set_title(&mut self, title: &str) {
        self.title = title.to_string();
    }

    /// Open a chart-local modal.
    pub fn open_modal(&mut self, modal: ChartOpenModal) {
        self.active_modal = modal;
    }

    /// Close any chart-local modal.
    pub fn close_modal(&mut self) {
        self.active_modal = ChartOpenModal::None;
    }

    /// Toggle a chart-local modal.
    pub fn toggle_modal(&mut self, modal: ChartOpenModal) {
        if self.active_modal == modal {
            self.close_modal();
        } else {
            self.open_modal(modal);
        }
    }

    /// Check if any chart-local modal is open.
    pub fn is_modal_open(&self) -> bool {
        self.active_modal.is_open()
    }

    /// Open the overlay settings modal.
    pub fn open_overlay_settings(&mut self) {
        self.overlay_settings_state.open();
    }

    /// Open the overlay settings modal for a specific leaf (highlights it).
    pub fn open_overlay_settings_for_leaf(&mut self, leaf_id: uzor::panels::LeafId) {
        self.overlay_settings_state.open_for_leaf(leaf_id);
    }

    /// Close the overlay settings modal.
    pub fn close_overlay_settings(&mut self) {
        self.overlay_settings_state.close();
    }

    /// Open the Tags & Tabs modal (defaults to TABS sidebar).
    pub fn open_tags_tabs(&mut self) {
        self.tags_tabs_state.open();
        // Pre-select active leaf so MAP buttons appear immediately
        if let Some(leaf_id) = self.panel_grid.docking().active_leaf() {
            self.overlay_settings_state.selected_node_id = Some(leaf_id.0);
        }
    }

    /// Open the Tags & Tabs modal for a specific leaf (TABS sidebar, leaf highlighted).
    pub fn open_tags_tabs_for_leaf(&mut self, leaf_id: uzor::panels::LeafId) {
        self.tags_tabs_state.open();
        // Store the target leaf in the embedded overlay_settings state for highlighting
        self.overlay_settings_state.target_leaf_id = Some(leaf_id);
        // Also pre-select so MAP buttons appear immediately
        self.overlay_settings_state.selected_node_id = Some(leaf_id.0);
    }

    /// Close the Tags & Tabs modal.
    pub fn close_tags_tabs(&mut self) {
        self.tags_tabs_state.close();
    }

    /// Open the panel color tag picker for the given leaf.
    ///
    /// `anchor_rect` is `[x, y, w, h]` of the color tag square in screen
    /// coordinates (from `LeafTabHitZones.color_tag_rect`).
    pub fn open_panel_color_tag_picker(
        &mut self,
        leaf_id: uzor::panels::LeafId,
        anchor_rect: [f64; 4],
        window_w: f64,
        window_h: f64,
        current_color: Option<&str>,
    ) {
        self.panel_color_picker_leaf = Some(leaf_id);
        self.panel_color_picker.open_l1_smart(
            anchor_rect[0],
            anchor_rect[1],
            anchor_rect[2],
            anchor_rect[3],
            window_w,
            window_h,
            current_color,
        );
    }

    /// Close the panel color tag picker.
    pub fn close_panel_color_tag_picker(&mut self) {
        self.panel_color_picker.close();
        self.panel_color_picker_leaf = None;
    }

    /// Close panel color tag picker one level (L2→L1 or L1→Closed).
    pub fn close_panel_color_tag_picker_one_level(&mut self) {
        use crate::ui::color_picker_state::ColorPickerLevel;
        match self.panel_color_picker.level {
            ColorPickerLevel::L2 => {
                self.panel_color_picker.back_to_l1();
            }
            _ => {
                self.panel_color_picker.close();
                self.panel_color_picker_leaf = None;
            }
        }
    }

    /// Render all sub-charts within `area`, handling split layout.
    ///
    /// This is the chart-crate rendering entry point, called by core with the
    /// extended `mlc_core::render::RenderContext` (which supports coordinate
    /// conversion methods needed by the chart rendering pipeline).
    ///
    /// The method:
    /// 1. Computes sub-chart rectangles via the split manager.
    /// 2. For each leaf, builds `ChartRenderState` from its `ChartContainer`.
    /// 3. Calls `render_chart_window` for each sub-chart.
    pub fn render_chart_content(
        &mut self,
        ctx: &mut dyn mlc_core::render::RenderContext,
        area: LayoutRect,
        chart_panels: &std::collections::HashMap<mlc_core::state::chart_container_leaf::ChartPanelId, ChartPanelState>,
    ) {
        // Convert LayoutRect (f64) to uzor::panels::PanelRect (f32).
        let split_rect = uzor::panels::PanelRect {
            x: area.x as f32,
            y: area.y as f32,
            width: area.width as f32,
            height: area.height as f32,
        };

        // Compute sub-chart rectangles for this frame.
        self.panel_grid.layout(split_rect);

        // Snapshot leaf → rect pairs to avoid borrow conflicts while iterating.
        let leaf_rects: Vec<_> = self
            .panel_grid
            .panel_rects()
            .iter()
            .map(|(&leaf_id, &sub_rect)| (leaf_id, sub_rect))
            .collect();

        for (leaf_id, sub_rect) in leaf_rects {
            let window = match self.panel_grid.container_for_leaf(leaf_id) {
                Some(w) => w,
                None => continue,
            };

            // Resolve the panel state for this leaf — fall back to a temporary
            // default if the entry is missing (e.g. first frame before seed).
            let default_panel = ChartPanelState::new(window.chart_panel_id);
            let panel = chart_panels
                .get(&window.chart_panel_id)
                .unwrap_or(&default_panel);

            // Convert sub_rect (f32, 0,0-based relative) to absolute LayoutRect (f64)
            // by adding the content area offset so rendering hits the correct screen position.
            let available = LayoutRect {
                x: area.x + sub_rect.x as f64,
                y: area.y + sub_rect.y as f64,
                width: sub_rect.width as f64,
                height: sub_rect.height as f64,
            };

            // Clip rendering to this sub-window's allocated rectangle so that
            // primitives and candles do not bleed across sub-window boundaries.
            ctx.save();
            ctx.begin_path();
            ctx.rect(available.x, available.y, available.width, available.height);
            ctx.clip();

            let price_scale_width = window.scale_settings.price_scale_width;
            let time_scale_height = window.scale_settings.time_scale_height;

            let chart_layout = ChartAreaLayout::compute(
                available,
                price_scale_width,
                time_scale_height,
            );

            let chart_rect = ChartRect {
                x: chart_layout.chart.x,
                y: chart_layout.chart.y,
                width: chart_layout.chart.width,
                height: chart_layout.chart.height,
            };

            // Clone viewport and set correct dimensions for this sub-chart so that
            // bar_to_x() and price_to_y() produce correct positions within the sub-rect.
            let mut sub_viewport = window.viewport.clone();
            sub_viewport.chart_width = chart_layout.chart.width;
            sub_viewport.chart_height = chart_layout.chart.height;

            let chart_theme = self.chart_theme_for_render();
            let render_state = ChartRenderState {
                viewport: &sub_viewport,
                price_scale: &window.price_scale,
                time_scale: &window.time_scale,
                bars: &window.bars,
                grid: &panel.grid_options,
                crosshair: &window.crosshair,
                legend: &panel.legend,
                chart_rect,
                theme: &chart_theme,
                time_ticks: None,
                current_timeframe: None,
                disable_clip: false,
                time_format_settings: Some(&window.scale_settings.time_format),
                timeframe_interval_ms: None,
                scale_settings: Some(&window.scale_settings),
                body_enabled: true,
                border_enabled: true,
                wick_enabled: true,
                use_prev_close: false,
                footprint_bars: window.footprint_bars.as_deref(),
                renko_bars: window.renko_bars.as_deref(),
                pnf_columns: window.pnf_columns.as_deref(),
                kagi_segments: window.kagi_segments.as_deref(),
                cvd_line: window.cvd_line.as_deref(),
                tpo_sessions: window.tpo_sessions.as_deref(),
            };

            let render_config = ChartRenderConfig {
                chart_type: panel.chart_type,
                scale_theme: self.scale_theme_for_render(),
                ..ChartRenderConfig::default()
            };

            let corner_state = ScaleCornerState::default();

            render_chart_window(
                ctx,
                &chart_layout,
                &render_state,
                &render_config,
                &corner_state,
            );

            // Render drawing primitives for this sub-chart leaf.
            crate::layout::render_main_chart_primitives(
                ctx,
                &chart_layout,
                &render_state,
                &panel.drawing_manager,
            );

            // Restore the clip region saved at the start of this sub-window iteration.
            ctx.restore();
        }
    }

}

impl PanelApp for ChartPanelApp {
    fn title(&self) -> &str {
        &self.title
    }

    fn type_id(&self) -> &'static str {
        "chart"
    }

    fn min_size(&self) -> (f64, f64) {
        (400.0, 300.0)
    }

    fn toolbar_def(&self) -> Option<PanelToolbarDef> {
        Some(toolbar::left_toolbar())
    }

    fn toolbar_position(&self) -> ToolbarPosition {
        ToolbarPosition::Left
    }

    fn render_toolbar(
        &self,
        ctx: &mut dyn RenderContext,
        rect: PanelRect,
        theme: &PanelTheme,
        input: &PanelInput,
    ) -> Vec<HitZone> {
        // For now, return empty — rendering will be wired in Phase 2
        // when the terminal orchestrator calls this method
        let _ = (ctx, rect, theme, input);
        Vec::new()
    }

    // render_content is intentionally left as the default no-op.
    //
    // Chart rendering requires the chart-crate's extended RenderContext
    // (mlc_core::render::RenderContext) which adds coordinate-conversion methods
    // (bar_to_x, price_to_y, etc.) not present on uzor::render::RenderContext.
    //
    // Callers that need to render the chart must downcast via as_any_mut:
    //   panel.as_any_mut().downcast_mut::<ChartPanelApp>()
    //       .map(|c| c.render_chart_content(ctx, area));

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn handle_toolbar_click(&mut self, item_id: &str) -> Option<String> {
        match item_id {
            // Toggle buttons (lock and eye flip boolean state directly)
            "lock" | "eye" => {
                self.toolbar_state.toggle(item_id);
                None
            }
            // Magnet: single-click toggle / double-click dropdown.
            // The full logic (with crosshair mutation) is in handle_toolbar_click_with_chart.
            // This PanelApp shim only handles the boolean toggle side (no crosshair access here).
            "magnet" => None,
            // Quick-select dropdowns
            "cursor_tools" | "line_tools" | "fib_tools" | "pattern_tools"
            | "brush_tools" | "annotation_tools" | "projection_tools"
            | "delete_tools" => {
                if self.toolbar_state.handle_quick_select_click(item_id) {
                    // Tool activated via quick-select
                    // Return the tool id so the terminal can route to DrawingManager
                    self.toolbar_state.active_tool_id.clone()
                } else {
                    // Need to open dropdown — terminal handles popup rendering
                    Some(format!("open_dropdown:{}", item_id))
                }
            }
            // Icon tools dropdown (not quick-select)
            "icon_tools" => {
                Some(format!("open_dropdown:{}", item_id))
            }
            // Undo/redo — terminal-level action
            "undo" => Some("undo".to_string()),
            "redo" => Some("redo".to_string()),
            // Settings — terminal opens modal
            "settings_menu" | "chart_settings" => Some("open_modal:chart_settings".to_string()),
            "indicators" => Some("open_modal:indicator_search".to_string()),
            "expand_chart" => Some("toggle_expand".to_string()),
            // Layout / workspace menus
            "layout_menu" => Some("open_modal:layout_menu".to_string()),
            "workspace_menu" => Some("open_modal:workspace_menu".to_string()),
            // Symbol/timeframe — terminal opens modal
            "symbol_selector" => Some("open_modal:symbol_search".to_string()),
            "compare" => Some("open_modal:symbol_search_compare".to_string()),
            _ => None,
        }
    }

    fn handle_dropdown_select(&mut self, dropdown_id: &str, item_id: &str) -> Option<String> {
        match dropdown_id {
            // Drawing tool dropdowns — select tool and remember for quick-select
            "cursor_tools" | "line_tools" | "fib_tools" | "pattern_tools"
            | "brush_tools" | "annotation_tools" | "projection_tools" => {
                // Use the item_id as both the tool_id and icon lookup
                // The icon name is typically PascalCase of the tool id
                self.toolbar_state.select_tool(dropdown_id, item_id, item_id);
                self.toolbar_state.active_tool_id.clone()
            }
            // Delete actions
            "delete_tools" => {
                match item_id {
                    "delete_selected" => Some("delete_selected".to_string()),
                    "delete_all" => Some("delete_all".to_string()),
                    _ => None,
                }
            }
            // Icon/emoji tools
            "icon_tools" | "emoji_submenu" => {
                self.toolbar_state.active_tool_id = Some(item_id.to_string());
                self.toolbar_state.active_tool_id.clone()
            }
            // Chart type selector
            "chart_type_selector" => {
                Some(format!("set_chart_type:{}", item_id))
            }
            // Timeframe selector
            "timeframe_selector" => {
                Some(format!("set_timeframe:{}", item_id))
            }
            _ => None,
        }
    }

    fn supports_toolbar_grouping(&self) -> bool {
        true // Charts can share drawing tools in a branch group
    }
}
