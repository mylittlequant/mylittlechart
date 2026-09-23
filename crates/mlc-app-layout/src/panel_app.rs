//! PanelApp implementation for the chart panel
//!
//! Makes the chart panel an autonomous application that owns its toolbar
//! and handles its own drawing tool state.

use uzor::panel_api::{
    PanelToolbarDef, DropdownItemDef, ToolbarItemDef, SectionAlign,
    ToolbarIconId,
};
use uzor::render::{RenderContext, draw_svg_icon};
// Extended (chart-specific) trait — needed ONLY where this file calls
// `mlc-app-layout::ui::widgets::draw_input` (granularity-editor widget
// overlay, defect fix 2026-07-16), which is typed against it. Kept as a
// distinct alias rather than swapping the bare `RenderContext` import above:
// `draw_dropdown`/`draw_grid_dropdown`'s `F: FnMut(&mut dyn RenderContext,
// ...)` bound needs `chart_icon_renderer`'s fn-item signature to match the
// BASE trait exactly (fn-pointer coercion is invariant here, unlike a plain
// reference argument) — widening the bare alias breaks that call site.
use mlc_core::engine::render::RenderContext as ChartRenderContext;
use crate::layout::LayoutRect;
use crate::layout::ExtendedFrameLayout;
use mlc_core::scale_settings::ScaleSettings;
use mlc_core::state::toolbar_geometry::{ToolbarRect, ToolbarRenderResult};
use crate::ui::dropdown::{DropdownItem, DropdownConfig, DropdownTheme, draw_dropdown, GridDropdownConfig, draw_grid_dropdown};
use crate::ui::widgets::theme_sample_tokens;
use crate::ui::toolbar_core::{
    WidgetRect, IconId,
    ToolbarConfig as TcConfig, ToolbarSection, ToolbarItem as TcToolbarItem,
    ToolbarTheme as TcToolbarTheme, ToolbarOrientation as TcOrientation,
    SectionAlign as TcSectionAlign, ToolbarResult as TcToolbarResult,
    draw_toolbar_with_icons, calculate_section_width,
    apply_active_states, apply_toggle_icons, apply_quick_select_icons,
};
use mlc_core::state::selected_config::SelectedPrimitiveConfig;
use mlc_core::chart_type::RESOLUTION_PARAM_ITEM_PREFIX;
use mlc_core::i18n::{current_language, ConceptKey};

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
///
/// `active_chart_type_param` is the active window's bubble's live
/// `chart_type_param` (owner doctrine 2026-07-08: the TF button shows the
/// resolution picker for the active instrument/chart_type — ticks for
/// tick-driven bars, ranges for range-driven bars, etc). `None` when the
/// active chart_type's [`mlc_core::chart_type::ResolutionKind`] is not
/// `Param` (the button falls back to the TF-name label in that case).
///
/// `timeframe_native_map` marks `timeframe_selector` items (`ResolutionKind::
/// Time`) that are NOT natively served by the active venue with a
/// "(no history)" subtitle — writer: `ChartApp::timeframe_native_map`,
/// resolution-availability-2026-07-08 Phase 3. A missing/`None` map means no
/// marker — items stay fully clickable either way (same transparency-only
/// contract as `chart_type_data_modes`; the station still builds a
/// non-native interval live via trade aggregation, it just has no history).
///
/// `footprint_live_only` marks every footprint bucket item (`ResolutionKind::
/// Interval`) "live only" when the active venue's trade-history capability
/// is the `RecentOnly` tier — writer: `ChartApp::footprint_live_only`.
///
/// `pin_granularity_favorites` (user-granularity-templates-inner arc, кусок
/// 3 — `ProfileUiSettings::pin_granularity_favorites`) renders starred
/// granularity entries as a row of compact quick-pick buttons on the top
/// toolbar right after `timeframe_selector`, in addition to their normal
/// dropdown FAVORITES section. `false` (default) leaves the toolbar
/// unchanged from today.
///
/// `split_symbol_compare` (symbol-search-compare-inner arc, Wave 2, кусок 3
/// — `ProfileUiSettings::split_symbol_compare`) `false` (the new default)
/// merges the standalone `compare` icon button into `symbol_selector`'s
/// fast-search dropdown — the `compare` item is dropped from the control
/// strip entirely. `true` keeps today's two-button look.
///
/// `symbol_quick_search_render` (кусок 4) carries the live query text/
/// cursor/selection + top result rows for the `symbol_selector` dropdown's
/// inline fast-search editor, built once per frame in `mlc-app` from
/// `ChartApp::symbol_quick_edit` — mirrors `granularity_edit_render`'s role.
/// `None` whenever the editor isn't active this frame.
///
/// Quick search (2026-07-17 rework): when `symbol_quick_search` is on, ANY
/// click on `symbol_selector` (text or magnifier icon — one hit zone, no
/// sub-zone split) opens the fast-search with the active symbol seeded as
/// editable text directly ON the button; the dropdown shows live results.
/// The former separate "inline input" mode/flag is deleted.
pub fn chart_toolbar_render_toolbars(
    tb: &ChartToolbarState,
    ctx: &mut dyn ChartRenderContext,
    layout: &ChartPanelLayout,
    toolbar_config: &ToolbarConfig,
    selected_primitive: Option<&SelectedPrimitiveConfig>,
    toolbar_theme: Option<&crate::ui::toolbar_theme::ToolbarTheme>,
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
    active_chart_type_param: Option<f64>,
    chart_type_data_modes: Option<&std::collections::HashMap<&'static str, mlc_core::chart_type::DataMode>>,
    timeframe_native_map: Option<&std::collections::HashMap<String, bool>>,
    footprint_live_only: bool,
    granularity_favorites: &mlc_core::user_profile::GranularityFavorites,
    // Favorite drawing tool ids (`ProfileUiSettings::favorite_primitives`) —
    // favorite-primitives arc. Drives the floating favorites quick-pick bar
    // (rendered when `tb.favorites_bar_visible`) and the star trailing icons
    // on drawing-tool flyout rows.
    favorite_primitives: &[String],
    granularity_edit_render: Option<&mlc_core::chart_type::GranularityEditRender>,
    pin_granularity_favorites: bool,
    granularity_drag_render: Option<&mlc_core::chart_type::GranularityDragRender>,
    // Live snapshot of an in-progress favorites-bar item drag (drag-out to
    // remove — favorite-primitives arc), mirroring `granularity_drag_render`'s
    // role for dropdown row reorder. `None` when no item drag is active.
    favorites_drag_render: Option<&mlc_core::state::toolbar_state::FavoritesDragRender>,
    split_symbol_compare: bool,
    symbol_quick_search_render: Option<&mlc_core::state::modal_state::SymbolQuickSearchRender>,
    symbol_quick_search: bool,
    active_exchange: Option<&str>,
    active_account_type: Option<&str>,
    // Workspace-panel arc, Stage 4 (ON mode — `ProfileUiSettings::
    // trading_panel_topbar_takeover` on AND focus is the Trading tab's
    // container): drop every chart-semantic control-strip button — the
    // topbar no longer represents a chart. `symbol_selector` stays (its
    // text/click-target already retarget to the container at the call
    // site) along with every session-global button (`main_menu`,
    // `settings_menu`, `undo`, `redo`, `screenshot`). Same `retain`-by-id
    // idiom `split_symbol_compare` already uses to drop `compare` below —
    // no parallel composer.
    suppress_chart_controls: bool,
    // Trading-container embedded header instance (owner spec 2026-07-26):
    // quicksearch dropdown shows RESULTS ONLY — no settings toggles (global
    // topbar concerns that must not leak or be flipped from the container)
    // and no compare "+" icons (compare is chart-only; one symbol key).
    quicksearch_minimal: bool,
    // Toolbar clock hover animation (ascii cell-shader engine demo, owner
    // spec 2026-09-02): pointer position in the same window-coordinate
    // space as `layout`'s rects, and a real elapsed-time value, both
    // threaded from the caller that already knows the mouse
    // (`ChartApp::last_mouse_pos`) — `None`/`0.0` from callers that never
    // host a `Clock` item (e.g. embedded header toolbars).
    mouse_pos: Option<(f64, f64)>,
    anim_time_s: f64,
    anim_frame: u64,
    // Clock hover-glitch user toggle (`ProfileUiSettings::
    // clock_glitch_enabled`, clock popup's third checkbox, owner ask
    // 2026-09-02): `true` (default) keeps the ascii cell-shader hover demo
    // above; `false` always falls back to the plain `fill_text` draw path.
    clock_glitch_enabled: bool,
    // `UserSettingsState::account_signed_in` snapshot (screenshot-
    // destination arc): gates the ShareLink row in the camera button's
    // double-click destination dropdown — anonymous sessions see exactly
    // the Clipboard / DownloadFile rows.
    account_signed_in: bool,
    // `ChartApp::is_capturing_screenshot()` snapshot (screenshot-is-chart-
    // only arc, 2026-09-05): an open toolbar dropdown opens BELOW its
    // button, i.e. past the top-toolbar strip and into the cropped chart
    // rect — suppressed while a screenshot capture is in flight so it never
    // leaks into the PNG. Callers with no capture concept of their own
    // (embedded headers) pass `false`, unchanged from today.
    is_capturing_screenshot: bool,
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
                    // Star button renders active while the favorites bar is open.
                    mlc_core::toolbar::FAVORITE_TOOLS_BUTTON_ID => tb.favorites_bar_visible,
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
                icon_size: 30.0,
                spacing: 4.0,
                padding: 4.0,
                separator_size: 1.0,
                scroll_offset: tb.left_scroll_offset,
                cursor: None,
                anim_time_s: 0.0,
                anim_frame: 0,
                clock_glitch_enabled: false,
                // No "Bot" item lives in the drawing toolbar.
                mascot_anim: None,
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
                match id {
                    // Camera button (screenshot-destination arc): highlight
                    // while its destination dropdown is open, during the
                    // ~1s post-fire active phase (`is_active` covers both
                    // via `open_dropdown_id` / `screenshot_active()` —
                    // owner ruling 2026-09-05), or when a non-default
                    // destination is remembered.
                    "screenshot" => tb.is_active(id)
                        || tb.screenshot_destination != mlc_core::state::toolbar_state::ScreenshotDestination::Clipboard,
                    _ => tb.is_active(id),
                }
            });

            // Camera button icon follows the remembered destination (default
            // = Clipboard keeps the def's camera icon). Same swap mechanic
            // the left toolbar uses for Magnet/Lock/Eye above.
            control_sections = apply_toggle_icons(
                control_sections,
                |id| match id {
                    "screenshot" => tb.screenshot_destination != mlc_core::state::toolbar_state::ScreenshotDestination::Clipboard,
                    _ => false,
                },
                |id| match id {
                    "screenshot" => match tb.screenshot_destination {
                        mlc_core::state::toolbar_state::ScreenshotDestination::DownloadFile => Some("CloudDownload"),
                        mlc_core::state::toolbar_state::ScreenshotDestination::ShareLink => Some("NewWindow"),
                        mlc_core::state::toolbar_state::ScreenshotDestination::Clipboard => None,
                    },
                    _ => None,
                },
            );

            // Override symbol_selector and timeframe_selector text with actual live values.
            //
            // timeframe_selector's label dispatches on the active chart_type's
            // `ResolutionKind` (owner doctrine 2026-07-08: the TF button IS
            // the universal resolution picker for the active instrument) —
            // `Time`/`Interval` both show the TF name unchanged (Interval's
            // bucket size rides the same `bubble.timeframe` field as Time,
            // see `ResolutionKind::Interval`'s doc comment); `Param` shows
            // the formatted `chart_type_param` + suffix instead.
            let tf_button_label: Option<String> = match active_chart_type.and_then(mlc_core::chart_type::by_id) {
                Some(def) => match def.resolution {
                    mlc_core::chart_type::ResolutionKind::Param { suffix } => {
                        active_chart_type_param.map(|v| {
                            // `v <= 0.0` is the pre-first-subscribe sentinel —
                            // show the registry's canonical default preset
                            // label via `ChartTypeDef::default_param_label`
                            // (canon-fix wave 2, item 1: single resolver
                            // shared with `chart_settings.rs`'s
                            // `ChartTypeParamSpec::current_label` and
                            // `default_chart_type_param`
                            // (mlc-app/src/input/chart_out_events.rs)) instead
                            // of a misleading "0<suffix>" — label and
                            // subscribe must always agree.
                            if v > 0.0 {
                                format_resolution_param(v, suffix)
                            } else {
                                def.default_param_label()
                                    .map(|l| format_resolution_param_label(l, suffix))
                                    .unwrap_or_else(|| format_resolution_param(v, suffix))
                            }
                        })
                    }
                    mlc_core::chart_type::ResolutionKind::Time
                    | mlc_core::chart_type::ResolutionKind::Interval => active_timeframe.map(String::from),
                },
                None => active_timeframe.map(String::from),
            };
            // Smart TF button label (owner spec 2026-07-16 item 4): when
            // pinning is on AND the active resolution value is one of the
            // quick-pick chips currently rendered in the strip (same
            // family/active-match rule `granularity_quick_pick_items` uses
            // — see `active_granularity_is_pinned`), the button drops its
            // text entirely and renders icon+chevron-only, since the pinned
            // chip already shows the value redundantly right next to it.
            // Otherwise (pin off, or active value not pinned) the button
            // keeps its normal icon+text rendering unchanged.
            let tf_button_icon_only = pin_granularity_favorites
                && active_granularity_is_pinned(
                    active_chart_type,
                    active_timeframe,
                    active_chart_type_param,
                    granularity_favorites,
                );
            // Quick search = button-hosted editor (2026-07-17 rework: the
            // former mode-2 "dropdown-hosted input row" variant is DELETED —
            // the owner kept only the inline behavior; `symbol_inline_input`
            // no longer exists). While the editor is live this frame
            // (`symbol_quick_search_render` is `Some` exactly then,
            // mirroring the widget-overlay block below's own gate), suppress
            // `symbol_selector`'s static text entirely — REAL suppression,
            // not an opaque cover-rect — so `draw_toolbar_with_icons` never
            // paints the old label under the editable overlay text.
            let symbol_selector_editing = symbol_quick_search_render.is_some();
            // Adaptive symbol button (owner-approved variant A, 2026-07-17):
            // the button WIDTH fits the tier-1 label of the ACTIVE
            // instrument — "SYMBOL · EXCH · AT" (exchange UPPERCASE, owner
            // rule), clamped 150..220, 4px right pad. The measurement
            // depends only on the active key, so the width changes exactly
            // on a symbol switch and never jumps mid-session. When even the
            // clamp can't fit tier 1, fall back through the tiers —
            //   1. "SYMBOL · EXCH · AT"
            //   2. "SYMBOL · AT"      (account type outranks exchange)
            //   3. "SYMBOL"
            // Measured with the exact font/metrics `draw_toolbar_with_icons`'
            // Dropdown arm draws with (13px sans-serif; text starts at
            // icon_size+8 past the item's left edge).
            let symbol_button_layout: Option<(String, f64)> = active_symbol.map(|sym| {
                let exch = active_exchange.map(str::to_uppercase).unwrap_or_default();
                let at = active_account_type.unwrap_or("");
                ctx.set_font("13px sans-serif");
                let tier1 = if !exch.is_empty() && !at.is_empty() {
                    format!("{} · {} · {}", sym, exch, at)
                } else if !at.is_empty() {
                    format!("{} · {}", sym, at)
                } else {
                    sym.to_string()
                };
                // Hug the content (owner rule 2026-07-30: no hardcoded floor
                // — the button fits its label exactly), capped at 220.
                let width = (24.0 + 8.0 + ctx.measure_text(&tier1) + 4.0).min(220.0);
                let budget = width - (24.0 + 8.0) - 4.0;
                let mut candidates: Vec<String> = vec![tier1];
                if !at.is_empty() {
                    candidates.push(format!("{} · {}", sym, at));
                }
                candidates.push(sym.to_string());
                let label = candidates
                    .into_iter()
                    .find(|c| ctx.measure_text(c) <= budget)
                    .unwrap_or_else(|| sym.to_string());
                (label, width)
            });
            // One-off expansion while the inline editor is live (owner spec
            // 2026-07-17): the button grows to fit the full composite key
            // (and whatever the user has typed since) + caret headroom, and
            // snaps back when editing ends — per-frame override, no
            // persistent state. The strip shifting right during editing is
            // by design ("разово расширить сам рект").
            let symbol_editing_min_width: Option<f64> = if symbol_selector_editing {
                symbol_quick_search_render.map(|r| {
                    ctx.set_font("13px sans-serif");
                    let text_w = ctx.measure_text(&r.text);
                    // Content-hugging (owner rule 2026-07-30) — typed text +
                    // caret headroom, no hardcoded floor.
                    24.0 + 8.0 + text_w + 24.0 + 8.0
                })
            } else {
                None
            };
            if active_symbol.is_some() || tf_button_label.is_some() || tf_button_icon_only || symbol_selector_editing {
                for section in &mut control_sections {
                    for item in &mut section.items {
                        if let TcToolbarItem::Dropdown { id, text, show_chevron, active, min_width, .. } = item {
                            if id == "symbol_selector" {
                                if symbol_selector_editing {
                                    *text = None;
                                    // No full active fill while editing (owner
                                    // 2026-07-16: "целиком подсвечивать
                                    // эктивом тупо — не видно что выделено") —
                                    // the overlay draws a focus FRAME around
                                    // the button instead, so text selection
                                    // stays readable on the normal background.
                                    *active = false;
                                    if let Some(w) = symbol_editing_min_width {
                                        *min_width = w;
                                    }
                                } else if let Some((ref label, width)) = symbol_button_layout {
                                    *text = Some(label.clone());
                                    *min_width = width;
                                }
                            } else if id == "timeframe_selector" {
                                if tf_button_icon_only {
                                    *text = None;
                                    *show_chevron = true;
                                } else if let Some(ref label) = tf_button_label {
                                    *text = Some(label.clone());
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

            // Pinned-favorites quick-pick row (user-granularity-templates-
            // inner arc, кусок 3 — TradingView "pinned intervals" style).
            // Injected INTO the item flow directly after `timeframe_selector`
            // (not drawn as a post-pass overlay — the control strip is
            // packed, there is no free space to overlay into) so every
            // subsequent item shifts right naturally and the toolbar's
            // EXISTING overflow/scroll-chevron machinery
            // (`calculate_section_width` + `draw_toolbar_with_icons`'s own
            // clip/scroll accounting) applies to it for free. `false`/no
            // applicable starred entries -> zero items inserted, zero layout
            // shift from today.
            if pin_granularity_favorites && !suppress_chart_controls {
                let quick_pick_items = granularity_quick_pick_items(
                    ctx,
                    active_chart_type,
                    active_timeframe,
                    active_chart_type_param,
                    granularity_favorites,
                );
                if !quick_pick_items.is_empty() {
                    'outer: for section in &mut control_sections {
                        for idx in 0..section.items.len() {
                            if section.items[idx].id() == Some("timeframe_selector") {
                                let insert_at = idx + 1;
                                // Standard toolbar separator between the TF
                                // button and the first pinned quick-pick chip
                                // (owner: "должны быть отделены сепаратором
                                // от основного ТФ") — the SAME separator item
                                // kind `convert_toolbar_def_to_sections`/
                                // toolbar_core already renders elsewhere in
                                // this strip, no new drawing.
                                section.items.insert(insert_at, TcToolbarItem::separator());
                                for (offset, item) in quick_pick_items.into_iter().enumerate() {
                                    section.items.insert(insert_at + 1 + offset, item);
                                }
                                break 'outer;
                            }
                        }
                    }
                }
            }

            // Symbol/compare toolbar merge (symbol-search-compare-inner arc,
            // Wave 2, кусок 3 — owner spec 2026-07-16). `false` (default)
            // drops the standalone `compare` icon button from the control
            // strip: its function is reached via the merged
            // `symbol_selector` fast-search dropdown instead (see
            // `chart_toolbar_find_dropdown_items`'s `"symbol_selector"` arm
            // in this module). `true` keeps today's two-button look
            // unchanged — zero layout change from today when this flag is
            // on.
            if !split_symbol_compare {
                for section in &mut control_sections {
                    section.items.retain(|item| item.id() != Some("compare"));
                }
            }

            // Workspace-panel arc, Stage 4 ON mode: the topbar represents
            // the Trading tab's container, not a chart — drop every
            // chart-semantic control. `symbol_selector` stays (retargeted
            // at the call site); every session-global button stays too.
            if suppress_chart_controls {
                const CHART_SEMANTIC_IDS: [&str; 6] = [
                    "compare", "timeframe_selector", "chart_type_selector",
                    "indicators", "layout_menu", "presets_menu",
                ];
                for section in &mut control_sections {
                    section.items.retain(|item| {
                        !matches!(item.id(), Some(id) if CHART_SEMANTIC_IDS.contains(&id))
                    });
                }
            }

            // Screenshot-destination arc, anonymous-sharing ruling
            // (2026-09-05): a signed-out session cannot use `share_chart`
            // (the live-share link needs a cloud session) — drop the globe
            // button from the strip entirely rather than draw it disabled,
            // same `retain`-by-id idiom `split_symbol_compare` uses for
            // `compare` above. `panel_click.rs`'s intercept refusal stays as
            // a backstop for stale rendered state.
            if !account_signed_in {
                for section in &mut control_sections {
                    section.items.retain(|item| item.id() != Some("share_chart"));
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
                // No Clock item lives in the top control strip (see
                // `mlc_core::toolbar::bottom_toolbar` vs `top_toolbar`) —
                // nothing here reads `cursor`/`anim_time_s` today.
                cursor: None,
                anim_time_s: 0.0,
                anim_frame: 0,
                clock_glitch_enabled: false,
                // No "Bot" item lives in the top control strip.
                mascot_anim: None,
            };

            let control_rect = WidgetRect::new(
                layout.top_toolbar_rect.x,
                layout.top_toolbar_rect.y,
                layout.top_toolbar_rect.width,
                layout.top_toolbar_rect.height,
            );

            // Clip the row to its own rect: chevron-scrolled items draw at
            // negative offsets and, on an embedded mid-screen instance (the
            // trading container header), bled LEFT past the toolbar edge —
            // the full-window main toolbar only hid this because offscreen
            // pixels are invisible (owner defect 2026-07-26). Dropdowns are
            // drawn by a later pass, outside this clip.
            ctx.save();
            ctx.clip_rect(control_rect.x, control_rect.y, control_rect.width, control_rect.height);
            let clipped_result = draw_toolbar_with_icons(
                ctx,
                &control_config,
                control_rect,
                &theme,
                tb.hovered_top_toolbar_id.as_deref(),
            );
            ctx.restore();
            clipped_result
        } else {
            TcToolbarResult::default()
        };

        // --- Right toolbar (vertical, right side, sidebar style) ---
        let right_tc_result = if toolbar_config.right.is_some() {
            let right_def = toolbar_config.right.as_ref().unwrap();
            let mut right_sections = convert_toolbar_def_to_sections(right_def);

            // Accent-highlight the OPEN sidebar panel's button (owner order
            // 2026-07-31) — otherwise there is no visual cue which tab is
            // open. The app stamps the open panel's button id per frame.
            if let Some(active_id) = tb.active_right_toolbar_id.as_deref() {
                for section in &mut right_sections {
                    for item in &mut section.items {
                        if let TcToolbarItem::IconButton { id, active, .. } = item {
                            if id == active_id {
                                *active = true;
                            }
                        }
                    }
                }
            }

            // Right toolbar uses sidebar_style so it renders with the sidebar variant.
            let right_theme = TcToolbarTheme {
                sidebar_style: true,
                ..theme.clone()
            };

            let right_config = TcConfig {
                sections: right_sections,
                orientation: TcOrientation::Vertical,
                item_size: 42.0,
                icon_size: 30.0,
                spacing: 4.0,
                padding: 4.0,
                separator_size: 1.0,
                scroll_offset: tb.right_scroll_offset,
                cursor: None,
                anim_time_s: 0.0,
                anim_frame: 0,
                clock_glitch_enabled: false,
                // The right toolbar hosts the "Bot" agents-panel button —
                // the one item this whole `mascot_anim` field exists for
                // (owner ruling 2026-09-10). Sourced from `tb` (already a
                // `&ChartToolbarState` parameter here) rather than a new
                // function parameter of its own.
                mascot_anim: tb.mascot_anim,
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

        // Unread system-events badge (owner defect 2026-09-05): a small
        // accent dot at the top-right corner of the "events" button while
        // `tb.system_events_unread` is true — drawn AFTER the right toolbar
        // so it sits on top of the icon, and independent of the "panel is
        // open" highlight above (that highlight is the ONLY source of
        // `active` again; reusing it as an unread cue painted the button
        // active all the time, owner bug report 2026-09-05). Unread stays
        // lit while the panel is open too — it means "unread", not "closed".
        // Geometry comes from `right_tc_result.item_rects` (real per-item
        // rect from this frame's layout), never hardcoded.
        if tb.system_events_unread {
            if let Some((_, item_rect)) = right_tc_result.item_rects.iter().find(|(id, _)| id == "events") {
                let dot_r = 3.0;
                let ring_r = dot_r + 1.0;
                let cx = item_rect.x + item_rect.width - ring_r - 2.0;
                let cy = item_rect.y + ring_r + 2.0;
                ctx.set_fill_color(&theme.background);
                ctx.begin_path();
                ctx.arc(cx, cy, ring_r, 0.0, std::f64::consts::TAU);
                ctx.close_path();
                ctx.fill();
                ctx.set_fill_color(&theme.accent);
                ctx.begin_path();
                ctx.arc(cx, cy, dot_r, 0.0, std::f64::consts::TAU);
                ctx.close_path();
                ctx.fill();
            }
        }

        // Unread DM badge on the "community" (envelope) button — same small
        // accent dot as the "events" badge above (community-modal-and-forum-
        // ladder arc, piece 2), driven by `tb.community_unread`.
        if tb.community_unread {
            if let Some((_, item_rect)) = right_tc_result.item_rects.iter().find(|(id, _)| id == "community") {
                let dot_r = 3.0;
                let ring_r = dot_r + 1.0;
                let cx = item_rect.x + item_rect.width - ring_r - 2.0;
                let cy = item_rect.y + ring_r + 2.0;
                ctx.set_fill_color(&theme.background);
                ctx.begin_path();
                ctx.arc(cx, cy, ring_r, 0.0, std::f64::consts::TAU);
                ctx.close_path();
                ctx.fill();
                ctx.set_fill_color(&theme.accent);
                ctx.begin_path();
                ctx.arc(cx, cy, dot_r, 0.0, std::f64::consts::TAU);
                ctx.close_path();
                ctx.fill();
            }
        }

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
                // The Clock item lives here (`mlc_core::toolbar::
                // bottom_toolbar`) — hover-glitch ascii demo needs the live
                // pointer + a real elapsed-time value, both threaded in from
                // the caller that already knows the mouse (owner spec
                // 2026-09-02).
                cursor: mouse_pos,
                anim_time_s,
                anim_frame,
                clock_glitch_enabled,
                // No "Bot" item lives in the bottom toolbar.
                mascot_anim: None,
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
            // E2 — widget-ize the raw `(slot_id, label)` pairs into
            // dropdown-row `(item_id, label)` pairs. The "inline:wave_option:"
            // prefix is an app-layer widget-id convention, not something
            // `SelectedPrimitiveConfig` (a plain data snapshot) or
            // `toolbar_core.rs` (domain-agnostic chrome) should know about.
            let wave_slot_items: Vec<(String, String)> = config.wave_slots.iter()
                .map(|(slot_id, label)| (format!("inline:wave_option:{slot_id}"), label.clone()))
                .collect();
            // E3 — same widget-ization as `wave_slot_items` above, for the
            // succession picker's own row list.
            let successor_kind_items: Vec<(String, String)> = config.successor_kinds.iter()
                .map(|(type_id, display_name)| (format!("inline:wave_next_option:{type_id}"), display_name.clone()))
                .collect();
            let inline_section = ToolbarSection::inline_config(
                &config.name,
                &config.color,
                config.text_color.as_deref(),
                config.supports_text,
                config.width as u32,
                &config.style,
                config.locked,
                &wave_slot_items,
                &successor_kind_items,
                config.can_promote,
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
                cursor: None,
                anim_time_s: 0.0,
                anim_frame: 0,
                clock_glitch_enabled: false,
                // No "Bot" item lives in the floating inline-config bar.
                mascot_anim: None,
            };

            // Pre-compute exact bar width from section items (single-pass, no measurement draw).
            let sections_width: f64 = inline_tc_config.sections.iter()
                .map(|s| calculate_section_width(&mut *ctx, s, &inline_tc_config))
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

        // --- Floating favorites quick-pick bar (favorite-primitives arc) ---
        //
        // TradingView-style: toggled by the left toolbar's `favorite_tools`
        // star button. Layout: leading 6-dots drag grip, one icon button per
        // favorite drawing tool (or a dimmed placeholder star when empty),
        // trailing "+" button opening the flat all-tools picker dropdown.
        // Reuses the exact float/drag/edge-magnet mechanics of the inline
        // config bar above (`floating_inline_absolute_rect` +
        // `FloatingInlineBar`). Width sizes dynamically with the number of
        // favorites via `calculate_section_width`, same as the inline bar.
        let favorites_bar = if tb.favorites_bar_visible {
            let mut fav_items: Vec<TcToolbarItem> = vec![
                // Drag grip — no registry icon (no grip/dots icon exists in
                // `mlc_core::state::icons`); the 6 dots are drawn procedurally
                // over this slot after the main draw pass below.
                TcToolbarItem::icon_button(mlc_core::toolbar::FAVORITES_BAR_GRIP_ITEM_ID, IconId::new("__grip_dots__")),
            ];
            if favorite_primitives.is_empty() {
                // Placeholder slot so the user sees where favorites will land
                // (and so the empty bar still drags — the app routes press on
                // this id to the bar's drag start).
                fav_items.push(TcToolbarItem::icon_button(mlc_core::toolbar::FAVORITES_BAR_EMPTY_ITEM_ID, IconId::new("star")));
            } else {
                for tool_id in favorite_primitives {
                    fav_items.push(
                        TcToolbarItem::icon_button(tool_id, IconId::new(&favorite_tool_icon(tool_id)))
                            .with_active(tb.active_tool_id.as_deref() == Some(tool_id.as_str()))
                    );
                }
            }
            fav_items.push(TcToolbarItem::icon_button(mlc_core::toolbar::FAVORITES_BAR_ADD_ITEM_ID, IconId::new("plus")));
            let fav_section = ToolbarSection::new(fav_items);
            let fav_tc_config = TcConfig {
                sections: vec![fav_section],
                orientation: TcOrientation::Horizontal,
                item_size: 28.0,
                icon_size: 24.0,
                spacing: 4.0,
                padding: 4.0,
                separator_size: 1.0,
                scroll_offset: 0.0, // Favorites bar never overflows (sized to fit)
                cursor: None,
                anim_time_s: 0.0,
                anim_frame: 0,
                clock_glitch_enabled: false,
                // No "Bot" item lives in the favorites quick-pick bar.
                mascot_anim: None,
            };

            // Pre-compute exact bar width from section items (single-pass, no measurement draw).
            let sections_width: f64 = fav_tc_config.sections.iter()
                .map(|s| calculate_section_width(&mut *ctx, s, &fav_tc_config))
                .sum();
            let bar_width = sections_width + fav_tc_config.padding * 2.0;
            let bar_rect = floating_inline_absolute_rect(&tb.favorites_bar, layout, bar_width, &top_slide, &bottom_slide, sidebar_w);

            // Single draw pass: draw_toolbar_with_icons draws background + items at exact width.
            let bar_wr = WidgetRect::new(bar_rect.x, bar_rect.y, bar_width, bar_rect.height);
            let fav_result = draw_toolbar_with_icons(
                ctx,
                &fav_tc_config,
                bar_wr,
                &theme,
                tb.hovered_favorites_id.as_deref(),
            );

            // Border — suppress the edge that merges with the host toolbar when docked.
            ctx.set_fill_color(&theme.separator);
            let dock = tb.favorites_bar.dock_edge;
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

            // Drag grip: 6 dots (2 columns x 3 rows), theme-colored like any
            // normal icon. The grip item itself carries no registry icon.
            if let Some((_, gr)) = fav_result.item_rects.iter()
                .find(|(id, _)| id == mlc_core::toolbar::FAVORITES_BAR_GRIP_ITEM_ID)
            {
                ctx.set_fill_color(&theme.item_text);
                const DOT_R: f64 = 1.6;
                const DOT_GAP: f64 = 5.0;
                let grid_w = DOT_GAP + DOT_R * 2.0;
                let grid_h = DOT_GAP * 2.0 + DOT_R * 2.0;
                let gx = gr.x + (gr.width - grid_w) / 2.0 + DOT_R;
                let gy = gr.y + (gr.height - grid_h) / 2.0 + DOT_R;
                for row in 0..3 {
                    for col in 0..2 {
                        ctx.begin_path();
                        ctx.arc(gx + col as f64 * DOT_GAP, gy + row as f64 * DOT_GAP, DOT_R, 0.0, std::f64::consts::TAU);
                        ctx.fill();
                    }
                }
            }

            // Item drag-out visual (favorite-primitives arc): dim the source
            // slot (`disabled` does NOT dim in toolbar_core — disabled icons
            // draw in plain `item_text`, so the dim is an explicit alpha
            // overlay here) and float the tool's icon at the cursor so the
            // drag reads as an actual pick-up.
            if let Some(drag) = favorites_drag_render {
                if let Some((_, sr)) = fav_result.item_rects.iter()
                    .find(|(id, _)| id == &drag.tool_id)
                {
                    ctx.save();
                    ctx.set_global_alpha(0.55);
                    ctx.set_fill_color(&theme.background);
                    ctx.fill_rect(sr.x, sr.y, sr.width, sr.height);
                    ctx.restore();
                }
                if let Some(svg) = crate::ui::icons::icon_svg(&favorite_tool_icon(&drag.tool_id)) {
                    draw_svg_icon(ctx, svg, drag.cursor_x - 12.0, drag.cursor_y - 12.0, 24.0, 24.0, &theme.item_text_hover);
                }
                // In-bar reorder: vertical insertion line at the drop slot
                // when the cursor is over the bar (drop-outside is remove,
                // no line). Same accent as granularity-row reorder.
                if bar_rect.contains(drag.cursor_x, drag.cursor_y) {
                    let drop_idx = mlc_core::toolbar::favorites_bar_drop_index(
                        fav_result.item_rects.iter().map(|(id, r)| (id.as_str(), r.x, r.width)),
                        drag.cursor_x,
                    );
                    let tool_rects: Vec<_> = fav_result.item_rects.iter()
                        .filter(|(id, _)| mlc_core::toolbar::is_favorites_bar_tool_id(id))
                        .collect();
                    let drop_line = tool_rects.get(drop_idx)
                        .map(|(_, r)| (r.x, r.y, r.height))
                        .or_else(|| tool_rects.last().map(|(_, r)| (r.x + r.width, r.y, r.height)));
                    if let Some((lx, ly, lh)) = drop_line {
                        ctx.set_stroke_color("#2962ff");
                        ctx.set_stroke_width(2.0);
                        ctx.begin_path();
                        ctx.move_to(lx, ly);
                        ctx.line_to(lx, ly + lh);
                        ctx.stroke();
                    }
                }
            }

            let fav_rects: Vec<(String, ToolbarRect)> = fav_result.item_rects.into_iter()
                .map(|(id, wr)| (id, ToolbarRect::new(wr.x, wr.y, wr.width, wr.height)))
                .collect();

            Some(InlineConfigResult {
                item_rects: fav_rects,
                bar_rect: LayoutRect::new(bar_rect.x, bar_rect.y, bar_width, bar_rect.height),
            })
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

        // Widget overlay for `symbol_selector`'s quick-search editor,
        // hosted directly ON THE TOOLBAR BUTTON (2026-07-17 rework: this
        // is the ONLY quick-search input — the dropdown-hosted variant is
        // deleted). It is CHROMELESS (`InputConfig::with_chrome(false)` — no
        // background fill, no border, no focus frame) so the text renders
        // exactly like the button's own normal label, just editable, with
        // only a caret / selection highlight on top. The button's static
        // text was already suppressed to `None` above (real suppression via
        // the live-value-override pass, not an opaque cover-rect) whenever
        // this overlay is live, so there is nothing stale underneath.
        //
        // Host rect: the BUTTON's own `"symbol_selector"` item rect from
        // `control_result` — the input spans the SAME icon+text layout
        // `TcToolbarItem::Dropdown`'s own draw code uses (`icon_size + 8`
        // gap after a 4px icon inset, matching `toolbar_core.rs`'s Dropdown
        // draw arm) so the editable text lands in exactly the same place
        // the static label used to.
        let symbol_inline_input_result: Option<(WidgetRect, Vec<f64>)> = {
            symbol_quick_search_render.and_then(|render| {
                let button_rect = control_result.item_rects.iter()
                    .find(|(id, _)| id == "symbol_selector")
                    .map(|(_, r)| *r)?;
                // The strip can be chevron-scrolled: the item rect may sit
                // partially (or fully) outside the toolbar. Skip when fully
                // outside; otherwise CLIP the editor overlay to the strip so
                // it never paints past the toolbar bounds (owner defect
                // 2026-07-30: the input leaked left of the sidebar edge).
                let strip = &layout.top_toolbar_rect;
                if button_rect.right() <= strip.x || button_rect.x >= strip.x + strip.width {
                    return None;
                }
                ctx.save();
                ctx.begin_path();
                ctx.move_to(strip.x, strip.y);
                ctx.line_to(strip.x + strip.width, strip.y);
                ctx.line_to(strip.x + strip.width, strip.y + strip.height);
                ctx.line_to(strip.x, strip.y + strip.height);
                ctx.close_path();
                ctx.clip();
                // Matches toolbar_core.rs's Dropdown draw arm: icon at
                // `item_rect.x + 4.0`, `config.icon_size` wide (24.0 for the
                // top control strip), text starts at `icon_size + 8.0`.
                let text_x = button_rect.x + 24.0 + 8.0;

                use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig};
                use crate::ui::widgets::types::{WidgetState, WidgetTheme};

                // Focus FRAME around the whole button (owner 2026-07-16):
                // the button keeps its normal background while editing (the
                // active fill is suppressed in the live-override pass above)
                // and this outline is the only focus signal — selection
                // highlight inside the text stays readable.
                ctx.set_stroke_color("#2962ff");
                ctx.set_stroke_width(2.0);
                ctx.stroke_rounded_rect(
                    button_rect.x + 1.0,
                    button_rect.y + 1.0,
                    button_rect.width - 2.0,
                    button_rect.height - 2.0,
                    4.0,
                );

                let widget_theme = WidgetTheme {
                    bg_normal: theme.background.clone(),
                    bg_hover: theme.item_bg_hover.clone(),
                    bg_pressed: theme.item_bg_hover.clone(),
                    bg_disabled: theme.background.clone(),
                    text_normal: theme.item_text.clone(),
                    text_hover: theme.item_text_hover.clone(),
                    text_disabled: dd_theme_ref.item_text_disabled.clone(),
                    border_normal: dd_theme_ref.border.clone(),
                    border_hover: theme.item_bg_hover.clone(),
                    border_focused: "#2962ff".to_string(),
                    accent: "#2962ff".to_string(),
                    accent_hover: "#2962ff".to_string(),
                    success: theme.item_text.clone(),
                    warning: theme.item_text.clone(),
                    danger: dd_theme_ref.item_danger.clone(),
                };
                // No placeholder — the field is seeded with the active
                // symbol on open (`open_symbol_quick_dropdown`/
                // `symbol_quick_seed_text`), never empty on first frame.
                let input_config = InputConfig::new(&render.text)
                    .with_focused(true)
                    .with_cursor(render.cursor)
                    .with_selection(render.selection.map(|(s, _)| s), render.selection.map(|(_, e)| e))
                    .with_padding(0.0)
                    .with_chrome(false);
                let input_rect = uzor::types::Rect::new(
                    text_x,
                    button_rect.y,
                    (button_rect.right() - text_x - 4.0).max(0.0),
                    button_rect.height,
                );
                let input_result = draw_input(ctx, &input_config, WidgetState::Normal, input_rect, &widget_theme);
                if render.cursor_visible {
                    draw_input_cursor(ctx, input_result.cursor_x, input_result.cursor_y, input_result.cursor_height, &theme.item_text);
                }
                ctx.restore();
                let tr = input_result.text_rect;
                Some((
                    WidgetRect::new(tr.x, tr.y, tr.width, tr.height),
                    input_result.char_x_positions,
                ))
            })
        };

        // Render open dropdown if any. Suppressed while a screenshot capture
        // is in flight (screenshot-is-chart-only arc, 2026-09-05): a
        // top-toolbar dropdown opens BELOW its button, i.e. past the strip
        // and into the cropped chart rect.
        let dropdown_result = if is_capturing_screenshot {
            None
        } else if let Some(ref dropdown_id) = tb.open_dropdown_id {
            chart_toolbar_render_dropdown(tb, ctx, dropdown_id, toolbar_config, layout, &drawing_result, &control_result, dd_theme_ref, presets, active_preset_id, autosave_enabled, sync_flags, is_expanded, split_without_group, is_mono_group, allowed_timeframes, active_chart_type, active_chart_type_param, chart_type_data_modes, timeframe_native_map, footprint_live_only, granularity_favorites, favorite_primitives, granularity_edit_render, pin_granularity_favorites, granularity_drag_render, symbol_quick_search_render, symbol_quick_search, split_symbol_compare, quicksearch_minimal, account_signed_in)
        } else {
            None
        };

        // Render inline style/width/wave/wave-succession dropdown popup if
        // open — same screenshot-capture suppression as `dropdown_result`
        // above (opens beside the left toolbar, into the cropped chart rect).
        let inline_dropdown_result = if is_capturing_screenshot {
            None
        } else {
            chart_toolbar_render_inline_dropdown(
                tb,
                ctx,
                &inline_config,
                dd_theme_ref,
                &theme,
                layout.full_rect.height,
                selected_primitive.map(|c| c.wave_slots.as_slice()).unwrap_or(&[]),
                selected_primitive.map(|c| c.wave_slot_kinds.as_slice()).unwrap_or(&[]),
                selected_primitive.map(|c| c.successor_kinds.as_slice()).unwrap_or(&[]),
            )
        };

        ChartToolbarRenderResult {
            left_toolbar: drawing_result,
            top_toolbar: control_result,
            right_toolbar: right_result,
            bottom_toolbar: bottom_result,
            content_rect: layout.content_rect,
            dropdown_result,
            inline_config,
            favorites_bar,
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
            symbol_inline_input: symbol_inline_input_result,
        }
    }

/// Build the pinned-favorites quick-pick `ToolbarItem::Button`s injected
/// INTO the top control strip's item flow directly after `timeframe_selector`
/// — user-granularity-templates-inner arc, кусок 3 (owner spec 2026-07-16,
/// TradingView "pinned intervals" style; fixed 2026-07-16 after live
/// verification showed the packed strip has no free space for a post-pass
/// overlay — the row must participate in the SAME item flow so later items
/// shift right and the toolbar's existing overflow/scroll-chevron machinery
/// applies to it for free).
///
/// One compact text `Button` per starred granularity entry: starred TF names
/// for `Time`/`Interval`-resolution chart types, starred numeric params
/// (filtered to the active chart_type) for `Param`-resolution chart types —
/// same selection the dropdown's FAVORITES section uses. Each item's
/// `min_width` is set to its OWN measured-text width (+ compact padding) so
/// `calculate_section_width`/the draw pass size it as a tight chip rather
/// than the toolbar's default `item_size * 2.5` button width. Hit id is
/// `"{GRANULARITY_QUICK_PICK_PREFIX}{value_id}"`, reusing the exact
/// `tf_{name}` / `{RESOLUTION_PARAM_ITEM_PREFIX}{value}` value encoding the
/// dropdown rows use — `panel_click.rs` already routes this prefix.
/// Build the (hit_value_id, label, is_active) triples for the applicable
/// starred set, keyed by the active chart_type's `ResolutionKind`
/// (per-family scoping arc 2026-07-16 — mirrors `prepend_param_starred_
/// items`/`prepend_tf_starred_items`/`prepend_interval_starred_items`'s own
/// dispatch): `Param` uses `starred_params` (filtered to the active
/// chart_type), `Interval` (footprint) uses `starred_intervals`, `Time`
/// (incl. no active chart_type known yet) uses `starred_tfs`. `Interval`
/// entries reuse the PLAIN `tf_{name}` id encoding (not `ivl_`) — the click
/// router's `resolve_granularity_target` already disambiguates a bare
/// `tf_{name}` quick-pick id by the ACTIVE chart_type's `ResolutionKind` at
/// click time, so no separate encoding is needed here; this guarantees a
/// footprint quick-pick chip can never render (or be clicked into) the
/// kline `starred_tfs` bucket and vice versa — the exact leak the owner
/// reported ("остался запинен 1m на футпринте").
///
/// Shared by [`granularity_quick_pick_items`] (toolbar row build) and
/// [`active_granularity_is_pinned`] (TF button smart-label gate) so both
/// read the identical family-dispatch/active-match rule — no risk of the
/// button's label state disagreeing with which chip actually renders active
/// in the strip.
fn granularity_quick_pick_entries(
    active_chart_type: Option<&str>,
    active_timeframe: Option<&str>,
    active_chart_type_param: Option<f64>,
    granularity_favorites: &mlc_core::user_profile::GranularityFavorites,
) -> Vec<(String, String, bool)> {
    let resolution = active_chart_type.and_then(mlc_core::chart_type::by_id).map(|def| def.resolution);

    match resolution {
        Some(mlc_core::chart_type::ResolutionKind::Param { suffix }) => {
            let Some(ct) = active_chart_type else { return Vec::new() };
            granularity_favorites.starred_params.iter()
                .filter(|f| f.chart_type == ct)
                .map(|f| {
                    let value_id = format!("{}{}", RESOLUTION_PARAM_ITEM_PREFIX, f.value);
                    let label = format_resolution_param(f.value, suffix);
                    let is_active = active_chart_type_param
                        .map(|v| (v - f.value).abs() < f64::EPSILON)
                        .unwrap_or(false);
                    (value_id, label, is_active)
                })
                .collect()
        }
        Some(mlc_core::chart_type::ResolutionKind::Interval) => {
            granularity_favorites.starred_intervals.iter()
                .map(|name| {
                    let value_id = format!("tf_{}", name);
                    let is_active = active_timeframe == Some(name.as_str());
                    (value_id, name.clone(), is_active)
                })
                .collect()
        }
        _ => {
            granularity_favorites.starred_tfs.iter()
                .map(|name| {
                    let value_id = format!("tf_{}", name);
                    let is_active = active_timeframe == Some(name.as_str());
                    (value_id, name.clone(), is_active)
                })
                .collect()
        }
    }
}

/// Build the pinned-favorites quick-pick `ToolbarItem::Button`s injected
/// INTO the top control strip's item flow directly after `timeframe_selector`
/// — user-granularity-templates-inner arc, кусок 3 (owner spec 2026-07-16,
/// TradingView "pinned intervals" style; fixed 2026-07-16 after live
/// verification showed the packed strip has no free space for a post-pass
/// overlay — the row must participate in the SAME item flow so later items
/// shift right and the toolbar's existing overflow/scroll-chevron machinery
/// applies to it for free). `false`/no applicable starred entries -> zero
/// items inserted, zero layout shift from today.
///
/// One compact text `Button` per starred granularity entry: starred TF names
/// for `Time`/`Interval`-resolution chart types, starred numeric params
/// (filtered to the active chart_type) for `Param`-resolution chart types —
/// same selection the dropdown's FAVORITES section uses. Each item's
/// `min_width` is set to its OWN measured-text width (+ compact padding) so
/// `calculate_section_width`/the draw pass size it as a tight chip rather
/// than the toolbar's default `item_size * 2.5` button width. Hit id is
/// `"{GRANULARITY_QUICK_PICK_PREFIX}{value_id}"`, reusing the exact
/// `tf_{name}` / `{RESOLUTION_PARAM_ITEM_PREFIX}{value}` value encoding the
/// dropdown rows use — `panel_click.rs` already routes this prefix.
fn granularity_quick_pick_items(
    ctx: &mut dyn RenderContext,
    active_chart_type: Option<&str>,
    active_timeframe: Option<&str>,
    active_chart_type_param: Option<f64>,
    granularity_favorites: &mlc_core::user_profile::GranularityFavorites,
) -> Vec<TcToolbarItem> {
    let entries = granularity_quick_pick_entries(
        active_chart_type,
        active_timeframe,
        active_chart_type_param,
        granularity_favorites,
    );
    if entries.is_empty() {
        return Vec::new();
    }

    const BTN_PAD_X: f64 = 12.0;

    ctx.set_font("13px sans-serif");
    entries.into_iter().map(|(value_id, label, is_active)| {
        let text_w = ctx.measure_text(&label);
        let hit_id = format!("{}{}", mlc_core::chart_type::GRANULARITY_QUICK_PICK_PREFIX, value_id);
        TcToolbarItem::Button {
            id: hit_id,
            icon: None,
            text: Some(label),
            // Active entry gets the same `#2962ff` selection accent the
            // TF button's own active state uses elsewhere in this
            // dropdown/toolbar (pre-existing accent exception).
            active: is_active,
            disabled: false,
            min_width: text_w + BTN_PAD_X,
            strikethrough: false,
        }
    }).collect()
}

/// Whether the ACTIVE granularity value (the same "current TF/param/interval"
/// the TF button's own label reflects) is one of the quick-pick entries
/// currently rendered in the pinned-favorites strip — smart TF button label
/// gate (owner spec 2026-07-16 item 4). Reuses
/// [`granularity_quick_pick_entries`] so "is the active value pinned" can
/// never disagree with which chip [`granularity_quick_pick_items`] actually
/// marks active. `false` whenever `pin_granularity_favorites` is off (caller
/// gates that separately) or no starred entry matches the active value —
/// the button falls back to its normal icon+text rendering in both cases.
fn active_granularity_is_pinned(
    active_chart_type: Option<&str>,
    active_timeframe: Option<&str>,
    active_chart_type_param: Option<f64>,
    granularity_favorites: &mlc_core::user_profile::GranularityFavorites,
) -> bool {
    granularity_quick_pick_entries(
        active_chart_type,
        active_timeframe,
        active_chart_type_param,
        granularity_favorites,
    ).iter().any(|(_, _, is_active)| *is_active)
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
    ctx: &mut dyn ChartRenderContext,
    dropdown_id: &str,
    toolbar_config: &ToolbarConfig,
    layout: &ChartPanelLayout,
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
    active_chart_type: Option<&str>,
    active_chart_type_param: Option<f64>,
    chart_type_data_modes: Option<&std::collections::HashMap<&'static str, mlc_core::chart_type::DataMode>>,
    timeframe_native_map: Option<&std::collections::HashMap<String, bool>>,
    footprint_live_only: bool,
    granularity_favorites: &mlc_core::user_profile::GranularityFavorites,
    favorite_primitives: &[String],
    granularity_edit_render: Option<&mlc_core::chart_type::GranularityEditRender>,
    pin_granularity_favorites: bool,
    granularity_drag_render: Option<&mlc_core::chart_type::GranularityDragRender>,
    symbol_quick_search_render: Option<&mlc_core::state::modal_state::SymbolQuickSearchRender>,
    symbol_quick_search_enabled: bool,
    split_symbol_compare: bool,
    quicksearch_minimal: bool,
    account_signed_in: bool,
) -> Option<DropdownRenderInfo> {
        // 1. Find the button rect for this dropdown in the toolbar results.
        //    If open_dropdown_position is set (e.g. for new_tab_menu opened by chrome),
        //    skip the button-rect lookup and use the custom position directly.
        //    The custom position is honored ONLY for the dropdowns that set
        //    it themselves (new_tab_menu, favorites-bar "+" picker) — a stale
        //    anchor must never relocate an ordinary button-anchored dropdown
        //    (favorite-primitives arc defect: left-toolbar flyouts rendered
        //    next to the favorites bar).
        let custom_position = tb.open_dropdown_position.filter(|_| {
            dropdown_id == "new_tab_menu"
                || dropdown_id == mlc_core::toolbar::FAVORITES_ADD_DROPDOWN_ID
        });
        let (mut origin, is_left_toolbar) = if let Some(pos) = custom_position {
            // Custom position: caller provides exact (x, y) for the dropdown top-left.
            (pos, false)
        } else {
            let button_rect = drawing_result.item_rects.iter()
                .chain(control_result.item_rects.iter())
                .find(|(id, _)| id == dropdown_id)
                .map(|(_, r)| *r)?;

            // Determine position: right of button for vertical toolbar, below for horizontal
            let is_left_toolbar = drawing_result.item_rects.iter().any(|(id, _)| id == dropdown_id);
            let origin = if is_left_toolbar {
                // Vertical toolbar: dropdown opens to the right of the button
                (button_rect.right() + 2.0, button_rect.y)
            } else {
                // Horizontal toolbar: dropdown opens below the button
                (button_rect.x, button_rect.bottom() + 2.0)
            };
            (origin, is_left_toolbar)
        };

        // 2. Find the dropdown items from toolbar definitions.
        //
        // `timeframe_selector` is dynamic for `Param`-resolution chart_types
        // (owner doctrine 2026-07-08): the button's popup lists the
        // registry's numeric presets instead of the static TF list — built
        // fresh here so it always reflects the active chart_type/instrument.
        let mut items = if dropdown_id == "timeframe_selector" {
            resolution_param_dropdown_items(active_chart_type)
                .or_else(|| chart_toolbar_find_dropdown_items(tb, dropdown_id, toolbar_config, presets, active_preset_id, account_signed_in))?
        } else {
            chart_toolbar_find_dropdown_items(tb, dropdown_id, toolbar_config, presets, active_preset_id, account_signed_in)?
        };

        // Granularity favorites (user-granularity-templates-inner arc, кусок
        // 2 — owner spec 2026-07-16; per-family scoping extended 2026-07-16):
        // sections appended/prepended around the `timeframe_selector`
        // dropdown's builtin ladder, matching how `items` was just built
        // above — keyed by the active chart_type's `ResolutionKind`:
        // - `Param`-resolution chart types (range/tick/volume/... bars) got
        //   the numeric preset ladder from `resolution_param_dropdown_items`
        //   — prepend the chart_type-filtered starred FAVORITES section,
        //   append the chart_type-filtered `params` customs + "+ Add" row.
        // - `Interval`-resolution (footprint TODAY, the only member) gets its
        //   OWN third family — `interval_tfs`/`starred_intervals`, never
        //   `time_tfs`/`starred_tfs` (owner: "для футпринта вообще не вижу
        //   ни кастома ни фейворита").
        // - `Time`-resolution (incl. the `None` active chart_type fallback)
        //   gets the kline family — prepend the starred FAVORITES section,
        //   append `time_tfs` customs + "+ Add" row, but ONLY when the
        //   chart_type is TF-eligible at all (`resolution == Time &&
        //   tf_whitelist == None`; never tpo, matching кусок 1's exact gate
        //   minus footprint, which now has its own branch instead of being
        //   silently excluded).
        // The "+ Add" row's label shows live typed text ONLY while adding
        // (host == None) — while EDITING an existing custom (host ==
        // Some(target)) the Add row stays "+ Add" and the live text renders
        // in the TARGET row instead (defect fix 2026-07-16, owner spec (a):
        // in-place editing). See the widget-overlay block below (step 5b).
        let add_row_editing_text: Option<&str> = granularity_edit_render
            .filter(|r| r.host.is_none())
            .map(|r| r.text.as_str());
        if dropdown_id == "timeframe_selector" {
            let resolution = active_chart_type.and_then(mlc_core::chart_type::by_id).map(|def| def.resolution);
            match resolution {
                Some(mlc_core::chart_type::ResolutionKind::Param { .. }) => {
                    prepend_param_starred_items(&mut items, active_chart_type, granularity_favorites);
                    append_param_customs_and_add_row(&mut items, active_chart_type, granularity_favorites, add_row_editing_text);
                }
                Some(mlc_core::chart_type::ResolutionKind::Interval) => {
                    prepend_interval_starred_items(&mut items, granularity_favorites);
                    append_interval_customs_and_add_row(&mut items, granularity_favorites, add_row_editing_text);
                }
                _ => {
                    let tf_eligible = active_chart_type
                        .and_then(mlc_core::chart_type::by_id)
                        .map(|def| matches!(def.resolution, mlc_core::chart_type::ResolutionKind::Time) && def.tf_whitelist.is_none())
                        .unwrap_or(true); // no active chart_type known yet -> don't hide the section
                    if tf_eligible {
                        prepend_tf_starred_items(&mut items, granularity_favorites);
                        append_tf_customs_and_add_row(&mut items, granularity_favorites, add_row_editing_text);
                    }
                }
            }
        }

        // symbol_selector's inline fast-search dropdown (symbol-search-
        // compare-inner arc, Wave 2, кусок 4; Wave 4 adds mode 3 "inline" —
        // owner spec 2026-07-16). Replaces the placeholder
        // `chart_toolbar_find_dropdown_items` returned (input host row
        // only) with the full live list: input host row (mode 2 only —
        // top-10 result rows + separator + two toggle rows + "Advanced
        // search…" row (the input itself lives ON the toolbar button —
        // 2026-07-17 rework, the dropdown-hosted input row is gone). Built
        // here (not in the finder fn) because it needs
        // `symbol_quick_search_render`'s live per-frame query results,
        // mirroring how granularity favorites are appended to the
        // `timeframe_selector` base list above rather than built in the
        // finder.
        if dropdown_id == "symbol_selector" {
            items = symbol_quick_dropdown_items(symbol_quick_search_render, quicksearch_minimal);
        }

        // 3. Convert DropdownItemDef -> DropdownItem
        let active_preset_load_id = format!("preset_load:{}", active_preset_id);
        // Tracks whether the item currently being converted sits inside the
        // prepended FAVORITES header section (user-granularity-templates-
        // inner arc, кусок 2) — `prepend_tf_starred_items`/
        // `prepend_param_starred_items` emit `Header{"FAVORITES"}` at the
        // start and a trailing `Separator` right before the builtin ladder
        // resumes, so this flips true on the header and false again on that
        // separator. Rows inside the section get an UNSTAR-only trailing
        // icon; the SAME id's normal-position row (builtin ladder or
        // customs list, later in the same iteration) gets the full
        // star(+edit+trash) treatment — position, not id, decides which.
        let mut in_favorites_section = false;
        let dropdown_items: Vec<DropdownItem> = items.iter().filter_map(|item| {
            if dropdown_id == "timeframe_selector" {
                match item {
                    DropdownItemDef::Header { label } if label == "FAVORITES" => {
                        in_favorites_section = true;
                    }
                    DropdownItemDef::Separator if in_favorites_section => in_favorites_section = false,
                    _ => {}
                }
            }
            let mut di = convert_dropdown_item(item);
            // Explicit pin TOGGLE row (defect fix 2026-07-16, owner:
            // "тугл неактивен, нужно именно тугл явный") — the row itself is
            // minted by `push_granularity_pin_toggle_row` as a normal
            // dropdown action item; this just paints the standard
            // toggle-switch visual on it, same pattern `presets_menu`'s
            // `preset_autosave` row uses below.
            if dropdown_id == "timeframe_selector" {
                if let DropdownItem::Item { ref id, ref mut toggle, .. } = di {
                    if id == mlc_core::chart_type::GRANULARITY_PIN_TOGGLE_ACTION_ID {
                        *toggle = Some(pin_granularity_favorites);
                    }
                }
            }
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
            // Drawing-tool flyouts (favorite-primitives arc): every action row
            // gets a trailing favorite star — filled gold when the tool is in
            // `favorite_primitives`, hollow otherwise. The star's hit id is
            // `prim_star:{tool_id}` (the row's own id — the same tool id the
            // drawing-tool arm of `handle_dropdown_select_with_chart`
            // selects); `panel_click.rs` intercepts that prefix and keeps the
            // dropdown open, exactly like `gran_star:`.
            if mlc_core::toolbar::DRAWING_FLYOUT_IDS.contains(&dropdown_id)
                || dropdown_id == mlc_core::toolbar::FAVORITES_ADD_DROPDOWN_ID
            {
                if let DropdownItem::Item { ref id, ref mut trailing_icons, .. } = di {
                    let is_favorite = favorite_primitives.iter().any(|t| t == id);
                    *trailing_icons = favorite_primitive_trailing_icon(id, is_favorite);
                }
            }
            if dropdown_id == "timeframe_selector" {
                if let DropdownItem::Item { ref id, ref mut disabled, ref mut accent_color, ref mut subtitle, ref mut trailing_icons, .. } = di {
                    // FAVORITES-section rows carry the
                    // `GRANULARITY_FAV_ID_PREFIX` uniqueness wrapper (defect
                    // fix 2026-07-16 — see that const's doc comment). Every
                    // lookup below (whitelist membership, native-availability
                    // map, custom/starred membership) keys off the profile's
                    // REAL id shape (`tf_{name}` / `{RESOLUTION_PARAM_ITEM_
                    // PREFIX}{value}`), so strip the wrapper here and use the
                    // stripped id for all of them; the row's own `id` field
                    // (used for the click hit-zone / hover) stays the full
                    // prefixed string untouched.
                    let lookup_id: &str = id.strip_prefix(mlc_core::chart_type::GRANULARITY_FAV_ID_PREFIX)
                        .unwrap_or(id.as_str());
                    if let Some(value_id) = lookup_id.strip_prefix(RESOLUTION_PARAM_ITEM_PREFIX) {
                        // `ResolutionKind::Param` dropdown (owner doctrine
                        // 2026-07-08): items are real presets now, not a
                        // TF-inert placeholder list — the old "disable every
                        // item, `allowed_timeframes == None`" behavior no
                        // longer applies (that gate is TF-depth-specific and
                        // has no meaning for a numeric threshold). Mark the
                        // preset matching the bubble's live `chart_type_param`
                        // instead, mirroring the `chart_type_selector` accent
                        // pattern above.
                        let is_current = active_chart_type_param
                            .map(|v| value_id.parse::<f64>().ok() == Some(v))
                            .unwrap_or(false);
                        if is_current {
                            *accent_color = Some("#2962ff".to_string());
                        }
                        // Trailing icons (кусок 2): star always (builtin
                        // ladder preset or custom, `in_favorites_section`
                        // decides unstar-only vs full set); edit+trash only
                        // for a value that is a CUSTOM (present in
                        // `favorites.params` for this chart_type) — a
                        // builtin ladder preset is never editable/deletable.
                        let value: Option<f64> = value_id.parse().ok();
                        let is_custom = value
                            .map(|v| active_chart_type.map(|ct| granularity_favorites.params.iter()
                                .any(|f| f.chart_type == ct && f.value == v)).unwrap_or(false))
                            .unwrap_or(false);
                        let is_starred = value
                            .map(|v| active_chart_type.map(|ct| granularity_favorites.starred_params.iter()
                                .any(|f| f.chart_type == ct && f.value == v)).unwrap_or(false))
                            .unwrap_or(false);
                        // Star-action target must resolve to the ORIGINAL
                        // (unprefixed) id — `granularity_trailing_icons`
                        // mints `gran_star:{lookup_id}`, so a FAVORITES-
                        // section row's unstar click still hits the real
                        // profile entry, not the synthetic `gran_fav:` id.
                        *trailing_icons = granularity_trailing_icons(lookup_id, is_custom, is_starred, in_favorites_section);
                    } else {
                        // Granularity rows (кусок 1/2) are NOT TF items —
                        // exempt them from the TF whitelist/disable machinery
                        // below entirely. Their visibility is already decided
                        // at append time (`prepend_tf_starred_items` /
                        // `prepend_param_starred_items` /
                        // `append_tf_customs_and_add_row` /
                        // `append_param_customs_and_add_row` /
                        // `resolution_param_dropdown_items`' ATR row gate):
                        // - the two "+ Add" rows and the ATR (14) row
                        //   would otherwise hit `allowed_timeframes == None`
                        //   on Param-resolution types → `disabled` → the
                        //   click router sees `__noop__` and the input never
                        //   focuses (live bug 2026-07-15);
                        // - custom/starred TF items (`tf_{name}`, e.g.
                        //   `tf_7m`) aren't members of the builtin
                        //   `ALL_TF_IDS`-derived allowed set → `return None`
                        //   would silently omit the whole FAVORITES section
                        //   AND the customs list on Time types.
                        // `ivl_{name}` rows (footprint bucket-interval
                        // CUSTOM/FAVORITE — per-family scoping arc
                        // 2026-07-16) are a THIRD family alongside the
                        // `TF_FAVORITE_CUSTOM_INPUT_ITEM_ID`/`tf_*` rows
                        // handled below: never members of the builtin
                        // `ALL_TF_IDS`-derived `allowed_timeframes` set
                        // (same reason customs/starred TF ids aren't), so
                        // they get their own exemption + trailing-icon
                        // membership check against `interval_tfs`/
                        // `starred_intervals` instead of `time_tfs`/
                        // `starred_tfs`.
                        let is_granularity_row = lookup_id == mlc_core::chart_type::TF_FAVORITE_CUSTOM_INPUT_ITEM_ID
                            || lookup_id == mlc_core::chart_type::PARAM_FAVORITE_CUSTOM_INPUT_ITEM_ID
                            || lookup_id == mlc_core::chart_type::RESOLUTION_PARAM_ATR14_ITEM_ID
                            || lookup_id == mlc_core::chart_type::INTERVAL_FAVORITE_CUSTOM_INPUT_ITEM_ID
                            || lookup_id == mlc_core::chart_type::GRANULARITY_PIN_TOGGLE_ACTION_ID;
                        let interval_name = lookup_id.strip_prefix(mlc_core::chart_type::GRANULARITY_INTERVAL_ID_PREFIX);
                        let bare_name = lookup_id.strip_prefix("tf_");
                        let is_custom_tf = bare_name
                            .map(|n| granularity_favorites.time_tfs.iter().any(|f| f == n))
                            .unwrap_or(false);
                        let is_starred_tf = bare_name
                            .map(|n| granularity_favorites.starred_tfs.iter().any(|f| f == n))
                            .unwrap_or(false);
                        let is_custom_interval = interval_name
                            .map(|n| granularity_favorites.interval_tfs.iter().any(|f| f == n))
                            .unwrap_or(false);
                        let is_starred_interval = interval_name
                            .map(|n| granularity_favorites.starred_intervals.iter().any(|f| f == n))
                            .unwrap_or(false);
                        let is_tf_favorite = is_custom_tf || is_starred_tf;
                        let is_interval_row = interval_name.is_some();
                        // Whitelist filtering (2026-07-08 hide-not-blur): a
                        // chart_type with an explicit `tf_whitelist`
                        // (footprint/tpo) OMITS non-whitelisted TF items from
                        // the dropdown entirely, rather than rendering them
                        // disabled/greyed. `tf_whitelist: None` types resolve
                        // to `Some(ALL_TF_IDS)` here, which contains every id
                        // — the omission check below is a no-op for them and
                        // the full list still renders.
                        //
                        // `None` = TF picker is not applicable at all for the
                        // active chart_type (TF-inert families, `hides_tf_picker`)
                        // — keep disabling EVERY TF item; the button itself
                        // stays visible so its toolbar position stays stable
                        // across chart-type switches. This is a different
                        // mechanism than whitelist filtering and is unaffected.
                        //
                        // `ivl_*` rows are exempt from this membership check
                        // entirely (same rationale as `is_granularity_row`
                        // below — they are never members of `ALL_TF_IDS`).
                        if !is_granularity_row && !is_interval_row {
                            match allowed_timeframes {
                                None => *disabled = true,
                                Some(allowed) if !allowed.contains(lookup_id) && !is_tf_favorite => return None,
                                _ => {}
                            }
                        }

                        // Active chart type is Interval-resolution (footprint)
                        // — needed by BOTH the trailing-icon family dispatch
                        // below and the availability marking further down.
                        let is_footprint_interval = active_chart_type
                            .and_then(mlc_core::chart_type::by_id)
                            .map(|def| matches!(def.resolution, mlc_core::chart_type::ResolutionKind::Interval))
                            .unwrap_or(false);

                        // Trailing icons (кусок 2): every plain TF/interval
                        // row (builtin OR custom) gets a star; customs
                        // additionally get edit+trash. The "+ Add"/ATR rows
                        // get none. Star-action target uses `lookup_id` —
                        // see the Param branch's comment above for the
                        // rationale.
                        //
                        // Family dispatch (owner defect 2026-07-16, round
                        // 3 follow-up): footprint's WHITELIST rows keep
                        // their `tf_{name}` ids (only its CUSTOM rows are
                        // `ivl_`-prefixed), so on an Interval-kind chart
                        // type a bare `tf_` row's star state must read
                        // `starred_intervals` — reading `starred_tfs`
                        // there lit the kline-family star ("1m" gold on
                        // footprint) while the CLICK toggled the interval
                        // family (display/action mismatch).
                        if is_interval_row {
                            *trailing_icons = granularity_trailing_icons(lookup_id, is_custom_interval, is_starred_interval, in_favorites_section);
                        } else if is_footprint_interval {
                            if let Some(n) = bare_name {
                                let starred = granularity_favorites.starred_intervals.iter().any(|f| f == n);
                                // Whitelist rows are never customs — customs
                                // render as their own `ivl_`-prefixed rows.
                                *trailing_icons = granularity_trailing_icons(lookup_id, false, starred, in_favorites_section);
                            }
                        } else if !is_granularity_row {
                            *trailing_icons = granularity_trailing_icons(lookup_id, is_custom_tf, is_starred_tf, in_favorites_section);
                        }

                        // Availability marking (resolution-availability-
                        // 2026-07-08 Phase 3): transparency-only, same
                        // contract as `chart_type_selector` below — items
                        // stay fully clickable, `disabled` is never touched
                        // here (a non-native/live-only interval is still
                        // buildable live, just carries no REST history).
                        //
                        // `footprint_live_only` (RecentOnly trade-history
                        // tier) takes priority over the per-interval native
                        // map for `ResolutionKind::Interval` (footprint):
                        // footprint's bucket is derived from live trades
                        // regardless of interval, so EVERY bucket is
                        // live-only on a RecentOnly venue, not just the
                        // non-native ones.
                        // Mark vocabulary simplified to ONE word and theme
                        // colors only (owner rule 2026-07-15): "live" =
                        // this interval carries no REST history on this
                        // venue (built live). No accent bar — the subtitle
                        // renders in the theme's secondary text color.
                        if is_footprint_interval {
                            if footprint_live_only {
                                *subtitle = Some("live".to_string());
                            }
                        } else if let Some(native_map) = timeframe_native_map {
                            if native_map.get(lookup_id) == Some(&false) {
                                *subtitle = Some("live".to_string());
                            }
                        }
                    }
                }
            }
            if dropdown_id == "chart_type_selector" {
                if let DropdownItem::Item { ref id, ref mut subtitle, .. } = di {
                    // Data-availability marking, simplified vocabulary
                    // (owner rule 2026-07-15): bulk history (klines or
                    // trade zips) = no mark; REST-paginated history =
                    // "slow"; no history at all = "live"; no data source
                    // whatsoever = the type is HIDDEN from the dropdown
                    // (not disabled — absent). Marks are subtitle-only in
                    // the theme's secondary text color — no accent bars,
                    // no hardcoded palette. Marked items stay clickable.
                    if let Some(marks) = chart_type_data_modes {
                        match marks.get(id.as_str()) {
                            Some(mlc_core::chart_type::DataMode::Paged) => {
                                *subtitle = Some("slow".to_string());
                            }
                            Some(mlc_core::chart_type::DataMode::Hybrid)
                            | Some(mlc_core::chart_type::DataMode::LiveOnly) => {
                                *subtitle = Some("live".to_string());
                            }
                            Some(mlc_core::chart_type::DataMode::Unavailable) => {
                                return None;
                            }
                            _ => {}
                        }
                    }
                }
            }
            Some(di)
        }).collect();

        if dropdown_items.is_empty() {
            return None;
        }

        // 4b. In-place edit target (defect fix 2026-07-16, owner spec (a)):
        // when `granularity_edit_render.host` names an existing custom's
        // row, hide ITS trailing icons while the editor is active (spec
        // (b)) — the widget-overlay block below (step 5b) draws the real
        // text-input widget over this row's label area, so the star/edit/
        // trash icons would otherwise sit awkwardly next to/under it. Only
        // the row's NORMAL-POSITION twin is targeted (the id shape a plain
        // `tf_{name}` / `{RESOLUTION_PARAM_ITEM_PREFIX}{value}` row carries)
        // — a FAVORITES-section twin (if the same value is also starred)
        // keeps its own unstar icon untouched, matching how star/edit/trash
        // already treat the two positions independently.
        let edit_target_row_id: Option<String> = granularity_edit_render
            .and_then(|r| r.host.as_ref())
            .map(|target| match target {
                mlc_core::chart_type::GranularityTarget::Tf { name } => format!("tf_{}", name),
                mlc_core::chart_type::GranularityTarget::Param { value, .. } => {
                    format!("{}{}", RESOLUTION_PARAM_ITEM_PREFIX, value)
                }
                mlc_core::chart_type::GranularityTarget::Interval { name } => {
                    format!("{}{}", mlc_core::chart_type::GRANULARITY_INTERVAL_ID_PREFIX, name)
                }
            });
        let mut dropdown_items = dropdown_items;
        if let Some(ref target_id) = edit_target_row_id {
            if let Some(DropdownItem::Item { trailing_icons, .. }) = dropdown_items.iter_mut()
                .find(|di| di.id() == Some(target_id.as_str()))
            {
                trailing_icons.clear();
            }
        }

        // symbol_selector result rows (кусок 4): attach the star (watchlist
        // toggle) + plus (add-compare) trailing icons — done here rather
        // than inline in `symbol_quick_dropdown_items` because
        // `TrailingIcon` is a `mlc-app-layout::ui::dropdown` type the
        // `DropdownItemDef` builder (uzor-panel-api, shared with every other
        // static toolbar def) has no field for, same reason granularity's
        // star/edit/delete icons are attached in this post-processing pass
        // rather than at `DropdownItemDef` construction time.
        if dropdown_id == "symbol_selector" {
            for di in dropdown_items.iter_mut() {
                if let DropdownItem::Item { id, toggle, trailing_icons, .. } = di {
                    if let Some(key) = id.strip_prefix(SYMBOL_QUICK_ITEM_PREFIX) {
                        let in_watchlist = symbol_quick_search_render
                            .map(|r| &r.results)
                            .and_then(|results| results.iter().find(|res| {
                                format!("{}:{}:{}", res.symbol, res.exchange_id, res.account_type) == key
                            }))
                            .map(|res| res.in_watchlist)
                            .unwrap_or(false);
                        let section_icon = symbol_quick_search_render
                            .and_then(|r| r.section_icon.as_deref());
                        let (star_icon, star_color) = if let Some(icon) = section_icon {
                            // Section picker (owner order 2026-07-31): the
                            // trailing icon is the SECTION'S OWN slot svg
                            // («для каждой нужна своя иконка»); accent =
                            // this venue is inside THIS panel (roster
                            // column or the single-mode current venue).
                            // Tokenized: theme accent, never a hardcoded
                            // hex (owner rule 2026-07-31).
                            if in_watchlist {
                                (IconId::new(icon), Some(theme.accent.clone()))
                            } else {
                                (IconId::new(icon), None)
                            }
                        } else if in_watchlist {
                            (IconId::new("star_filled"), Some("#FFD700".to_string()))
                        } else {
                            (IconId::new("star"), None)
                        };
                        let mut star = crate::ui::dropdown::TrailingIcon::new(
                            format!("sym_quick_star:{}", key), star_icon,
                        );
                        if let Some(c) = star_color {
                            star = star.with_color(c);
                        }
                        if quicksearch_minimal {
                            *trailing_icons = vec![star];
                        } else {
                            let plus = crate::ui::dropdown::TrailingIcon::new(
                                format!("sym_quick_cmp:{}", key), IconId::new("Plus"),
                            );
                            *trailing_icons = vec![star, plus];
                        }
                    } else if id == "sym_quick:toggle_quick" {
                        *toggle = Some(symbol_quick_search_enabled);
                    } else if id == "sym_quick:toggle_split" {
                        *toggle = Some(split_symbol_compare);
                    }
                }
            }
        }

        // 5. Build config and render the main dropdown
        let mut config = DropdownConfig::new(dropdown_items);
        // Chart-type menu: two-column grid inside each section (owner spec
        // 2026-07-05) — headers/separators span the full row, items flow
        // 2 per row. Everything else keeps the classic vertical list.
        if dropdown_id == "chart_type_selector" {
            config.grid_columns = Some(2);
        }
        // Favorites bar "+" picker (favorite-primitives arc): the flat
        // all-drawing-tools list is far taller than any window as a single
        // column (`dropdown.rs` has no scrolling and no max-height) — flow
        // it 4 per row, same grid mechanic as the chart-type menu above.
        if dropdown_id == mlc_core::toolbar::FAVORITES_ADD_DROPDOWN_ID {
            config.grid_columns = Some(4);
        }
        // Keep the menu inside the hosting toolbar's horizontal span — on
        // the full-window main toolbar this is a no-op, on the embedded
        // trading-header instance it stops button-aligned menus escaping
        // past the window edge (owner defect 2026-07-26).
        if layout.top_toolbar_rect.width > 0.0 {
            config.clamp_x = Some((
                layout.top_toolbar_rect.x,
                layout.top_toolbar_rect.x + layout.top_toolbar_rect.width,
            ));
        }

        // Vertical fit guard for the left-rail flyout (defect fix
        // 2026-08-25): a quick_select group opening near the bottom of the
        // rail (e.g. pattern/smart-money/order-flow tools) would otherwise
        // draw its lower rows past the window edge — `dropdown.rs` has no
        // scrolling and no max-height, so anything below the bottom is
        // simply unreachable. Shift the origin up so the flyout's bottom
        // stays inside the panel, clamped so the top never rises above the
        // panel's top edge (content taller than the whole panel pins to the
        // top instead of overflowing both ends).
        if is_left_toolbar {
            let content_height = config.calculate_height();
            let panel_top = layout.full_rect.y;
            let panel_bottom = layout.full_rect.y + layout.full_rect.height;
            let overflow = (origin.1 + content_height) - panel_bottom;
            if overflow > 0.0 {
                origin.1 -= overflow;
            }
            origin.1 = origin.1.max(panel_top);
        }

        // Same vertical fit guard for the favorites bar's "+" picker
        // (favorite-primitives arc) — it opens at a custom position next to
        // the bar (`is_left_toolbar == false`), so the flyout guard above
        // does not cover it.
        if dropdown_id == mlc_core::toolbar::FAVORITES_ADD_DROPDOWN_ID {
            let content_height = config.calculate_height();
            let panel_top = layout.full_rect.y;
            let panel_bottom = layout.full_rect.y + layout.full_rect.height;
            let overflow = (origin.1 + content_height) - panel_bottom;
            if overflow > 0.0 {
                origin.1 -= overflow;
            }
            origin.1 = origin.1.max(panel_top);
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

        // Populated by the widget-overlay block below (mouse-support arc,
        // item 1) with `(text_rect, char_x_positions)` from `draw_input`'s
        // `InputResult` — fed into `TextFieldStore::update_field` by
        // `ChartApp::update_text_field_geometry` so click-to-cursor and
        // drag-select work against the SAME metrics just rendered here.
        let mut granularity_edit_input: Option<(WidgetRect, Vec<f64>)> = None;

        // Populated by the "watermark_submenu" block below (§6) the same
        // way `granularity_edit_input` is populated by §5b, just above —
        // see that field's own doc.
        let mut watermark_text_input: Option<(WidgetRect, Vec<f64>)> = None;

        // 5b. Widget overlay for the visible inline granularity editor
        // (defect fix 2026-07-16, owner spec (b)) — draws the SAME
        // centralized text-input widget every modal field uses
        // (`crate::ui::widgets::{draw_input, draw_input_cursor,
        // InputConfig}`, e.g. `chart_settings.rs` fields / the preset-name
        // input) directly over the host row's rect, AFTER `draw_dropdown`
        // already painted that row's label/trailing-icons — the widget's
        // opaque background covers the stale label so nothing double-
        // renders. Host row: the Add row (`TF_FAVORITE_CUSTOM_INPUT_ITEM_ID`
        // / `PARAM_FAVORITE_CUSTOM_INPUT_ITEM_ID` /
        // `INTERVAL_FAVORITE_CUSTOM_INPUT_ITEM_ID`) when `host == None`, or
        // `edit_target_row_id`'s row when `host == Some(target)` (spec (a):
        // editing renders in place, not relocated to the Add row).
        if dropdown_id == "timeframe_selector" {
            if let Some(edit) = granularity_edit_render {
                let host_id: Option<&str> = match &edit.host {
                    None => Some(mlc_core::chart_type::TF_FAVORITE_CUSTOM_INPUT_ITEM_ID)
                        .filter(|_| all_item_rects.iter().any(|(id, _)| id == mlc_core::chart_type::TF_FAVORITE_CUSTOM_INPUT_ITEM_ID))
                        .or_else(|| Some(mlc_core::chart_type::INTERVAL_FAVORITE_CUSTOM_INPUT_ITEM_ID)
                            .filter(|_| all_item_rects.iter().any(|(id, _)| id == mlc_core::chart_type::INTERVAL_FAVORITE_CUSTOM_INPUT_ITEM_ID)))
                        .or(Some(mlc_core::chart_type::PARAM_FAVORITE_CUSTOM_INPUT_ITEM_ID)),
                    Some(_) => edit_target_row_id.as_deref(),
                };
                if let Some(host_id) = host_id {
                    if let Some(row_rect) = all_item_rects.iter().find(|(id, _)| id == host_id).map(|(_, r)| *r) {
                        use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig};
                        use crate::ui::widgets::types::{WidgetState, WidgetTheme};

                        let widget_theme = WidgetTheme {
                            bg_normal: theme.background.clone(),
                            bg_hover: theme.item_bg_hover.clone(),
                            bg_pressed: theme.item_bg_hover.clone(),
                            bg_disabled: theme.background.clone(),
                            text_normal: theme.item_text.clone(),
                            text_hover: theme.item_text_hover.clone(),
                            text_disabled: theme.item_text_disabled.clone(),
                            border_normal: theme.border.clone(),
                            border_hover: theme.item_bg_hover.clone(),
                            border_focused: "#2962ff".to_string(),
                            accent: "#2962ff".to_string(),
                            accent_hover: "#2962ff".to_string(),
                            success: theme.item_text.clone(),
                            warning: theme.item_text.clone(),
                            danger: theme.item_danger.clone(),
                        };
                        let input_config = InputConfig::new(&edit.text)
                            .with_focused(true)
                            .with_cursor(edit.cursor)
                            .with_placeholder(if edit.host.is_none() { "e.g. 7m" } else { "" })
                            .with_selection(edit.selection.map(|(s, _)| s), edit.selection.map(|(_, e)| e))
                            .with_padding(12.0);
                        // `draw_input` takes `uzor::types::Rect` — a distinct
                        // (same-shape) type from this file's `WidgetRect`
                        // (`mlc_core::state::toolbar_geometry::WidgetRect`,
                        // no `From` conversion between them), so build one
                        // from the row rect's fields directly, inset by 2px
                        // so the widget's own border doesn't touch the
                        // dropdown row's edges.
                        let input_rect = uzor::types::Rect::new(
                            row_rect.x + 2.0,
                            row_rect.y + 2.0,
                            (row_rect.width - 4.0).max(0.0),
                            (row_rect.height - 4.0).max(0.0),
                        );
                        let input_result = draw_input(ctx, &input_config, WidgetState::Normal, input_rect, &widget_theme);
                        if edit.cursor_visible {
                            draw_input_cursor(ctx, input_result.cursor_x, input_result.cursor_y, input_result.cursor_height, &theme.item_text);
                        }
                        // Capture the widget's rendered text_rect + char
                        // boundary positions (mouse-support arc, item 1) —
                        // `text_rect` uses `uzor::types::Rect`, convert to
                        // this module's `WidgetRect` for the render-result
                        // struct (same shape, no `From` impl between them).
                        let tr = input_result.text_rect;
                        granularity_edit_input = Some((
                            WidgetRect::new(tr.x, tr.y, tr.width, tr.height),
                            input_result.char_x_positions,
                        ));
                    }
                }
            }
        }

        // (2026-07-17 rework: the former 5b' dropdown-hosted symbol input
        // overlay is deleted — the quick-search input lives ON the toolbar
        // button, drawn by `chart_toolbar_render_toolbars`' own overlay.)

        // 5c. Drag-to-reorder visual: dragged-row highlight + insertion-gap
        // line (owner spec 2026-07-16 items 2+3) — mirrors
        // `WatchlistModalState`'s drag-reorder overlay (`watchlist_modal.rs`,
        // "Drag-reorder visual" block) exactly: a semi-transparent accent
        // overlay on the row being dragged, plus a 2px theme-accent line at
        // the current drop index. Only paints for `timeframe_selector` —
        // rows are looked up via `granularity_section_row_ids` so the
        // highlight tracks the SAME index space `ReorderGranularity`'s
        // mutation handler operates on.
        if dropdown_id == "timeframe_selector" {
            if let Some(drag) = granularity_drag_render {
                let row_ids = granularity_section_row_ids(&drag.family, drag.section, granularity_favorites);
                if let Some(from_rect) = row_ids.get(drag.from_index)
                    .and_then(|id| all_item_rects.iter().find(|(rid, _)| rid == id))
                    .map(|(_, r)| *r)
                {
                    ctx.set_fill_color("#2962ff33");
                    ctx.fill_rect(from_rect.x, from_rect.y, from_rect.width, from_rect.height);
                }
                if let Some(drop_idx) = drag.drop_index {
                    // Drop line sits ABOVE the row currently at `drop_idx`,
                    // or below the last row when dropping past the end.
                    let drop_line_rect = row_ids.get(drop_idx)
                        .and_then(|id| all_item_rects.iter().find(|(rid, _)| rid == id))
                        .map(|(_, r)| (r.x, r.y, r.width))
                        .or_else(|| row_ids.last()
                            .and_then(|id| all_item_rects.iter().find(|(rid, _)| rid == id))
                            .map(|(_, r)| (r.x, r.bottom(), r.width)));
                    if let Some((lx, ly, lw)) = drop_line_rect {
                        ctx.set_stroke_color("#2962ff");
                        ctx.set_stroke_width(2.0);
                        ctx.begin_path();
                        ctx.move_to(lx, ly);
                        ctx.line_to(lx + lw, ly);
                        ctx.stroke();
                    }
                }
            }
        }

        // 6. Render submenu grid if one is open
        if let Some(ref submenu_id) = tb.open_submenu_id {
            // Find the submenu trigger item rect within the dropdown
            if let Some((_, submenu_trigger_rect)) = all_item_rects.iter().find(|(id, _)| id == submenu_id) {
                let submenu_trigger_rect = *submenu_trigger_rect;

                // Find submenu items from toolbar definitions
                if let Some((sub_items_def, grid_columns)) = chart_toolbar_find_submenu_items(tb, submenu_id, toolbar_config) {
                    let sub_items: Vec<DropdownItem> = sub_items_def.iter().map(|item| {
                        let mut di = convert_dropdown_item(item);
                        if let Some(id) = di.id() {
                            if let Some(&on) = tb.settings_toggles.get(id) {
                                di = di.with_toggle(on);
                            }
                        }
                        di
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
                        all_item_rects.extend(sub_result.item_rects.clone());

                        if submenu_id == "watermark_submenu" {
                            if let Some(row_rect) = sub_result.item_rects.iter()
                                .find(|(id, _)| id == "watermark_text")
                                .map(|(_, r)| *r)
                            {
                                use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig};
                                use crate::ui::widgets::types::{WidgetState, WidgetTheme};
                                let widget_theme = WidgetTheme {
                                    bg_normal: theme.background.clone(),
                                    bg_hover: theme.item_bg_hover.clone(),
                                    bg_pressed: theme.item_bg_hover.clone(),
                                    bg_disabled: theme.background.clone(),
                                    text_normal: theme.item_text.clone(),
                                    text_hover: theme.item_text_hover.clone(),
                                    text_disabled: theme.item_text_disabled.clone(),
                                    border_normal: theme.border.clone(),
                                    border_hover: theme.item_bg_hover.clone(),
                                    border_focused: "#2962ff".to_string(),
                                    accent: "#2962ff".to_string(),
                                    accent_hover: "#2962ff".to_string(),
                                    success: theme.item_text.clone(),
                                    warning: theme.item_text.clone(),
                                    danger: theme.item_danger.clone(),
                                };
                                let input_config = InputConfig::new(&tb.watermark_draft)
                                    .with_focused(tb.watermark_edit_focused)
                                    .with_cursor(tb.watermark_edit_cursor)
                                    .with_placeholder("…")
                                    .with_padding(8.0);
                                let input_rect = uzor::types::Rect::new(
                                    row_rect.x + 2.0,
                                    row_rect.y + 2.0,
                                    (row_rect.width - 4.0).max(0.0),
                                    (row_rect.height - 4.0).max(0.0),
                                );
                                let input_result = draw_input(ctx, &input_config, WidgetState::Normal, input_rect, &widget_theme);
                                if tb.watermark_edit_focused && tb.watermark_edit_cursor_visible {
                                    draw_input_cursor(ctx, input_result.cursor_x, input_result.cursor_y, input_result.cursor_height, &theme.item_text);
                                }
                                // Surface the rendered rect + char positions —
                                // see `watermark_text_input`'s own doc for why
                                // (stale-focus audit 2026-09-23).
                                watermark_text_input = Some((
                                    WidgetRect::new(input_rect.x, input_rect.y, input_rect.width, input_rect.height),
                                    input_result.char_x_positions,
                                ));
                            }
                        }

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
            granularity_edit_input,
            watermark_text_input,
        })
    }

/// Render the inline style, width, wave-slot (E2), or wave-succession (E3)
/// dropdown popup if one is open.
///
/// Looks up the button rect from `inline_config`, then calls `draw_dropdown` to
/// draw the popup below (or above) the button.  Returns `None` if no inline dropdown
/// is open or if the button rect cannot be found.
///
/// When there is not enough space below the button (button bottom + dropdown height
/// exceeds `panel_height`), the dropdown opens upward and the chevron icon on the
/// button is redrawn pointing up.
///
/// `wave_slots` is the selected primitive's own `SelectedPrimitiveConfig::
/// wave_slots` (`(slot_id, label)` pairs) — only read when the wave dropdown
/// is the one open AND `tb.pending_wave_slot` is `None` (the picker's own
/// FIRST step); empty for a non-Elliott selection, matching the
/// `"inline:wave"` button itself only existing when this is non-empty.
/// `wave_slot_kinds` is `SelectedPrimitiveConfig::wave_slot_kinds` —
/// `(slot_id, Vec<(kind_id, label_key)>)` pairs — read instead of
/// `wave_slots` once `tb.pending_wave_slot` names a slot (the picker's own
/// SECOND step: which structure goes in it). `successor_kinds` is the same
/// shape as `wave_slots` for `SelectedPrimitiveConfig::successor_kinds` —
/// only read when the wave-succession dropdown is the one open, matching
/// `"inline:wave_next"`'s own non-empty gate.
fn chart_toolbar_render_inline_dropdown(
    tb: &ChartToolbarState,
    ctx: &mut dyn RenderContext,
    inline_config: &Option<InlineConfigResult>,
    theme: &DropdownTheme,
    toolbar_theme: &TcToolbarTheme,
    panel_height: f64,
    wave_slots: &[(String, String)],
    wave_slot_kinds: &[(String, Vec<(String, ConceptKey)>)],
    successor_kinds: &[(String, String)],
) -> Option<InlineDropdownResult> {
    // Determine which inline dropdown is open (style takes priority)
    let (dropdown_id, button_id) = if tb.open_inline_style_dropdown {
        ("inline:style", "inline:style")
    } else if tb.open_inline_width_dropdown {
        ("inline:width", "inline:width")
    } else if tb.open_inline_wave_dropdown {
        ("inline:wave", "inline:wave")
    } else if tb.open_inline_wave_next_dropdown {
        ("inline:wave_next", "inline:wave_next")
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
        } else if dropdown_id == "inline:width" {
            // Width options 1–5
            (1u32..=5).map(|w| {
                crate::ui::dropdown::DropdownItem::item(
                    &format!("inline:width_option:{}", w),
                    &format!("{} px", w),
                )
            }).collect()
        } else if dropdown_id == "inline:wave" {
            match tb.pending_wave_slot.as_deref() {
                // Step 1 — wave-slot picker: one row per slot the selected
                // primitive's own points can seed a child into, labelled
                // the way that slot prints AT THE PARENT's degree
                // (`SelectedPrimitiveConfig::wave_slots`, sourced from
                // `WaveSlot::label`).
                None => wave_slots.iter().map(|(slot_id, label)| {
                    crate::ui::dropdown::DropdownItem::item(&format!("inline:wave_option:{slot_id}"), label)
                }).collect(),
                // Step 2 — wave-KIND picker for the slot chosen in step 1
                // (owner ask 2026-08-13: slot first, then which structure
                // goes in it). A leading back row steps to `None` again
                // without leaving the dropdown; the kind rows themselves
                // come from `SelectedPrimitiveConfig::wave_slot_kinds`,
                // resolved to this frame's own active language HERE (the
                // backend struct carries a `ConceptKey`, never a resolved
                // string — see that field's own doc).
                Some(slot_id) => {
                    let lang = current_language();
                    let mut items = vec![
                        crate::ui::dropdown::DropdownItem::item("inline:wave_back", ConceptKey::WaveBackLabel.get(lang)),
                    ];
                    if let Some((_, kinds)) = wave_slot_kinds.iter().find(|(s, _)| s == slot_id) {
                        for (kind_id, label_key) in kinds {
                            // Icon added 2026-08-14 (owner report — text-only
                            // rows for a structure kind that already has a
                            // registered toolbar icon; `elliott_icon_id` is
                            // the SAME split-on-`:` helper the context-menu
                            // wave popup uses for the same row shape).
                            items.push(crate::ui::dropdown::DropdownItem::item(
                                &format!("inline:wave_kind:{slot_id}:{kind_id}"),
                                label_key.get(lang),
                            ).with_icon(crate::ui::toolbar_core::IconId::new(
                                crate::ui::context_menu::elliott_icon_id(kind_id),
                            )));
                        }
                    }
                    items
                }
            }
        } else {
            // Wave-succession picker (E3) — one row per Elliott type valid
            // to follow the selected primitive at its own degree
            // (`SelectedPrimitiveConfig::successor_kinds`, sourced from
            // `elliott::count::successor_kinds` + each candidate type's own
            // registered `display_name`).
            successor_kinds.iter().map(|(type_id, display_name)| {
                // Icon added 2026-08-14 — see the wave-kind picker above;
                // `type_id` here is already the bare Elliott type id (no
                // `:subtype`), but `elliott_icon_id` is still the single
                // source of truth for the id spelling the icon match wants.
                crate::ui::dropdown::DropdownItem::item(&format!("inline:wave_next_option:{type_id}"), display_name)
                    .with_icon(crate::ui::toolbar_core::IconId::new(
                        crate::ui::context_menu::elliott_icon_id(type_id),
                    ))
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

/// Format a `ResolutionKind::Param` value for the TF button label / dropdown
/// item text — compact, trims trailing zeros, dollar-family gets a K/M
/// shorthand (owner spec 2026-07-08: "10R", "100T", "$100K" style).
///
/// `pub` (not crate-private) — the pane overlay tab legend (`mlc-app`,
/// `render_leaf_tab` caller) reuses this SAME formatting path so a
/// trade-agg-bar/price-action window's tab shows the live param label
/// ("20R", "0.1V") instead of the stale `bubble.timeframe.name` (root
/// cause fixed 2026-07-08, legend-honesty item).
pub fn format_resolution_param(value: f64, suffix: &str) -> String {
    if suffix == "$" {
        let abs = value.abs();
        if abs >= 1_000_000.0 {
            return format!("${}M", format_trimmed(value / 1_000_000.0));
        }
        if abs >= 1_000.0 {
            return format!("${}K", format_trimmed(value / 1_000.0));
        }
        return format!("${}", format_trimmed(value));
    }
    format!("{}{}", format_trimmed(value), suffix)
}

/// Format a registry preset's pre-formatted `label` (e.g. "1,000,000") with
/// its chart_type's suffix — same dollar-vs-suffix convention as
/// [`resolution_param_dropdown_items`]'s item labels, factored out so the
/// TF button's pre-first-subscribe fallback (sentinel `chart_type_param <=
/// 0.0`) renders identically to the dropdown's own first entry.
///
/// `pub` — reused by the pane overlay tab legend (`mlc-app`) for the same
/// pre-first-subscribe sentinel case.
pub fn format_resolution_param_label(preset_label: &str, suffix: &str) -> String {
    if suffix == "$" {
        format!("${}", preset_label)
    } else {
        format!("{}{}", preset_label, suffix)
    }
}

/// Trim a trailing `.0` (or more generally any trailing fractional zeros)
/// from an `f64`'s decimal rendering — mirrors
/// `chart_settings.rs::format_trimmed_f64` for the toolbar-side formatting
/// need (kept local: `mlc-app-layout` cannot depend on that modal-internal
/// helper).
fn format_trimmed(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        let s = format!("{:.2}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Build the trailing icon buttons for one `timeframe_selector` row —
/// user-granularity-templates-inner arc, кусок 2 (owner spec 2026-07-16).
///
/// - Inside the FAVORITES header section (`in_favorites_section == true`):
///   a single filled star (unstar on click). No edit/trash there — a
///   starred row is edited/deleted from its normal-position twin.
/// - Everywhere else: a star (filled if `is_starred`, empty otherwise, click
///   toggles); customs (`is_custom == true`) additionally get edit (pencil)
///   and delete (trash) icons. Builtins get star only.
///
/// Icon ids come straight from `mlc_core::state::icons` (`"star"`/
/// `"star_filled"`/`"pencil"`/`"delete"`) — the exact icon set the watchlist
/// symbol-search star uses (`search_overlay.rs`'s `Icon::Star`/
/// `Icon::StarFilled`, same `#FFD700` filled accent reused here verbatim).
fn granularity_trailing_icons(
    item_id: &str,
    is_custom: bool,
    is_starred: bool,
    in_favorites_section: bool,
) -> Vec<crate::ui::dropdown::TrailingIcon> {
    use crate::ui::dropdown::TrailingIcon;
    const STAR_FILLED_COLOR: &str = "#FFD700";

    let star_action = format!("{}{}", mlc_core::chart_type::GRANULARITY_STAR_ACTION_PREFIX, item_id);
    if in_favorites_section {
        // A FAVORITES-section row is, by construction, always starred —
        // the icon here is always the filled variant (unstar affordance).
        return vec![TrailingIcon::new(star_action, IconId::new("star_filled")).with_color(STAR_FILLED_COLOR)];
    }

    let mut icons = Vec::with_capacity(3);
    let (star_icon, star_color) = if is_starred {
        (IconId::new("star_filled"), Some(STAR_FILLED_COLOR.to_string()))
    } else {
        (IconId::new("star"), None)
    };
    let mut star = TrailingIcon::new(star_action, star_icon);
    if let Some(c) = star_color {
        star = star.with_color(c);
    }
    icons.push(star);

    if is_custom {
        let edit_action = format!("{}{}", mlc_core::chart_type::GRANULARITY_EDIT_ACTION_PREFIX, item_id);
        icons.push(TrailingIcon::new(edit_action, IconId::new("pencil")));
        let del_action = format!("{}{}", mlc_core::chart_type::GRANULARITY_DELETE_ACTION_PREFIX, item_id);
        icons.push(TrailingIcon::new(del_action, IconId::new("delete")));
    }
    icons
}

/// Trailing favorite-star for a drawing-tool flyout row (favorite-primitives
/// arc) — mirrors [`granularity_trailing_icons`]'s star minting verbatim:
/// filled `#FFD700` `star_filled` when the tool is already in
/// `ProfileUiSettings::favorite_primitives`, hollow `star` otherwise. The
/// action id is `prim_star:{tool_id}` (`mlc_core::toolbar::
/// PRIM_STAR_ACTION_PREFIX`); the click router toggles membership WITHOUT
/// closing the dropdown, same contract as `gran_star:`.
fn favorite_primitive_trailing_icon(
    tool_id: &str,
    is_favorite: bool,
) -> Vec<crate::ui::dropdown::TrailingIcon> {
    use crate::ui::dropdown::TrailingIcon;
    const STAR_FILLED_COLOR: &str = "#FFD700";

    let star_action = format!("{}{}", mlc_core::toolbar::PRIM_STAR_ACTION_PREFIX, tool_id);
    if is_favorite {
        vec![TrailingIcon::new(star_action, IconId::new("star_filled")).with_color(STAR_FILLED_COLOR)]
    } else {
        vec![TrailingIcon::new(star_action, IconId::new("star"))]
    }
}

/// Resolve the toolbar icon id for a favorite drawing tool (favorite-primitives
/// arc): the primitive registry's own `icon` metadata when `tool_id` is a
/// registered primitive, the id itself otherwise — cursor-mode ids
/// (`"crosshair"`, `"hand"`) are not primitives but resolve under their own
/// names in `mlc_core::state::icons`, same as the quick-select memory icons.
fn favorite_tool_icon(tool_id: &str) -> String {
    mlc_core::drawing::primitives_v2::PrimitiveRegistry::global()
        .read()
        .ok()
        .and_then(|registry| registry.get(tool_id).map(|m| m.icon.to_string()))
        .unwrap_or_else(|| tool_id.to_string())
}

/// Hit-id prefix for one `symbol_selector` fast-search result row
/// (symbol-search-compare-inner arc, Wave 2, кусок 4). The suffix is the
/// composite `"SYMBOL:exchange_id:account_type"` key — same shape the
/// full-screen search modal's `"modal_search:item:"` rows use
/// (`modals.rs`'s `rest.starts_with("item:")` parse), so
/// `panel_click.rs`'s click handler can reuse the identical rfind-twice
/// parse. `sym_quick_star:`/`sym_quick_cmp:` (trailing-icon actions) and
/// `sym_quick:item:` (row body click) all share this same key suffix.
pub const SYMBOL_QUICK_ITEM_PREFIX: &str = "sym_quick:item:";

/// Build the `symbol_selector` toolbar dropdown's full item list
/// (symbol-search-compare-inner arc; 2026-07-17 rework — the input lives ON
/// the toolbar button, the former dropdown-hosted input row and the
/// "Inline input" mode/toggle are deleted): top-10 live query results, a
/// separator, two toggle rows ("Quick search" / "Separate compare
/// button"), and an "Advanced search…" row that falls back to the
/// full-screen modal. Exchange names render UPPERCASE (display-only, owner
/// rule 2026-07-17 — the commit key keeps the canonical lowercase id).
///
/// Star/plus `TrailingIcon`s for the result rows are attached by the CALLER
/// (`chart_toolbar_render_dropdown`'s post-processing pass) since
/// `DropdownItemDef` (uzor-panel-api, shared with every static toolbar def)
/// has no trailing-icon field — same split `granularity_trailing_icons`
/// uses for the `timeframe_selector` dropdown's star/edit/delete icons.
fn symbol_quick_dropdown_items(
    render: Option<&mlc_core::state::modal_state::SymbolQuickSearchRender>,
    minimal: bool,
) -> Vec<DropdownItemDef> {
    let mut items: Vec<DropdownItemDef> = Vec::new();

    if let Some(r) = render {
        if !r.results.is_empty() {
            items.push(DropdownItemDef::Separator);
            for (idx, res) in r.results.iter().enumerate() {
                // Roster mode: the pinned already-added block ends here —
                // draw the separator between it and the search results.
                if r.pinned_count > 0 && idx == r.pinned_count {
                    items.push(DropdownItemDef::Separator);
                }
                let key = format!("{}:{}:{}", res.symbol, res.exchange_id, res.account_type);
                let label = format!(
                    "{} \u{b7} {} \u{b7} {}",
                    res.symbol, res.exchange.to_uppercase(), res.account_type,
                );
                items.push(DropdownItemDef::action(
                    format!("{}{}", SYMBOL_QUICK_ITEM_PREFIX, key),
                    label,
                ));
            }
        }
    }

    if !minimal {
        items.push(DropdownItemDef::Separator);
        items.push(DropdownItemDef::action("sym_quick:toggle_quick", "Quick search"));
        items.push(DropdownItemDef::action("sym_quick:toggle_split", "Separate compare button"));
        items.push(DropdownItemDef::Separator);
        items.push(
            DropdownItemDef::action("sym_quick:open_modal", "Advanced search\u{2026}")
                .with_icon(ToolbarIconId::new("Search")),
        );
    }

    items
}

/// Ordered dropdown row ids for one granularity FAVORITES/CUSTOM list —
/// drag-to-reorder support (owner spec 2026-07-16 items 2+3). Index `i` in
/// the returned `Vec` is exactly the `from_index`/`to_index`
/// `ChartOutEvent::ReorderGranularity` operates on for this
/// `(family, section)` pair — mirrors the EXACT emission order
/// `prepend_tf_starred_items`/`prepend_param_starred_items`/
/// `prepend_interval_starred_items` (Favorites) and
/// `append_tf_customs_and_add_row`/`append_param_customs_and_add_row`/
/// `append_interval_customs_and_add_row` (Customs) use when building
/// `items`, so index N here always names the same profile-list entry index
/// N those mutation-side helpers (and `move_within`/`move_within_filtered`
/// in `chart_out_events.rs`) work against.
///
/// Favorites-section ids carry the `GRANULARITY_FAV_ID_PREFIX` wrapper
/// (matching how those rows are actually keyed in `all_item_rects` — see
/// that const's doc comment); Customs-section ids are the plain
/// `tf_{name}` / `{RESOLUTION_PARAM_ITEM_PREFIX}{value}` / `ivl_{name}` row
/// ids.
pub fn granularity_section_row_ids(
    family: &mlc_core::chart_type::GranularityFamily,
    section: mlc_core::chart_type::GranularitySection,
    granularity_favorites: &mlc_core::user_profile::GranularityFavorites,
) -> Vec<String> {
    use mlc_core::chart_type::{GranularityFamily, GranularitySection, GRANULARITY_FAV_ID_PREFIX, GRANULARITY_INTERVAL_ID_PREFIX};

    match (family, section) {
        (GranularityFamily::Tf, GranularitySection::Favorites) => {
            granularity_favorites.starred_tfs.iter()
                .map(|name| format!("{}tf_{}", GRANULARITY_FAV_ID_PREFIX, name))
                .collect()
        }
        (GranularityFamily::Tf, GranularitySection::Customs) => {
            granularity_favorites.time_tfs.iter()
                .map(|name| format!("tf_{}", name))
                .collect()
        }
        (GranularityFamily::Interval, GranularitySection::Favorites) => {
            granularity_favorites.starred_intervals.iter()
                .map(|name| format!("{}{}{}", GRANULARITY_FAV_ID_PREFIX, GRANULARITY_INTERVAL_ID_PREFIX, name))
                .collect()
        }
        (GranularityFamily::Interval, GranularitySection::Customs) => {
            granularity_favorites.interval_tfs.iter()
                .map(|name| format!("{}{}", GRANULARITY_INTERVAL_ID_PREFIX, name))
                .collect()
        }
        (GranularityFamily::Param { chart_type }, GranularitySection::Favorites) => {
            granularity_favorites.starred_params.iter()
                .filter(|f| &f.chart_type == chart_type)
                .map(|f| format!("{}{}{}", GRANULARITY_FAV_ID_PREFIX, RESOLUTION_PARAM_ITEM_PREFIX, f.value))
                .collect()
        }
        (GranularityFamily::Param { chart_type }, GranularitySection::Customs) => {
            granularity_favorites.params.iter()
                .filter(|f| &f.chart_type == chart_type)
                .map(|f| format!("{}{}", RESOLUTION_PARAM_ITEM_PREFIX, f.value))
                .collect()
        }
    }
}

/// Build the `timeframe_selector` dropdown's item list for the active
/// chart_type's `ResolutionKind::Param` presets (owner doctrine 2026-07-08).
///
/// `None` when the active chart_type is unknown or not `Param`-resolution —
/// caller falls back to the static TF item list (`Time`/`Interval` share the
/// existing `timeframe_selector` toolbar-def item list unchanged).
///
/// Sourced from the SAME `ChartTypeDef::param_spec` catalog the settings-
/// modal numeric-param row reads (`chart_settings.rs::chart_type_param_spec`)
/// — single source of truth, no duplicated preset lists.
fn resolution_param_dropdown_items(active_chart_type: Option<&str>) -> Option<Vec<DropdownItemDef>> {
    let ct = active_chart_type?;
    let def = mlc_core::chart_type::by_id(ct)?;
    let mlc_core::chart_type::ResolutionKind::Param { suffix } = def.resolution else { return None };
    let spec = def.param_spec?;
    let mut items: Vec<DropdownItemDef> = spec.options.iter().map(|opt| {
        let label = format_resolution_param_label(opt.label, suffix);
        DropdownItemDef::action(format!("{}{}", RESOLUTION_PARAM_ITEM_PREFIX, opt.value_id), label)
    }).collect();
    // ATR(14) box-size mode — renko/kagi ONLY (granularity arc, кусок 2 —
    // `docs/mlc/research/industry-resolution-defaults-2026-07-15.md` §3/§5).
    // Appended after the plain numeric ladder, dispatched via the fixed
    // `RESOLUTION_PARAM_ATR14_ITEM_ID` (not a `RESOLUTION_PARAM_ITEM_PREFIX`
    // value-encoding id — ATR carries no numeric input of its own).
    if matches!(ct, "renko" | "kagi") {
        items.push(DropdownItemDef::action(
            mlc_core::chart_type::RESOLUTION_PARAM_ATR14_ITEM_ID,
            "ATR (14)",
        ));
    }
    Some(items)
}

/// Append the explicit "Pin to toolbar" toggle row to a FAVORITES section
/// being built by `prepend_*_starred_items` — defect fix 2026-07-16 (owner:
/// "тугл неактивен, нужно именно тугл явный"). Replaces the earlier pin
/// ICON rendered on the FAVORITES header itself (that trailing-icon zone
/// never read as an interactive toggle) with a standard dropdown row using
/// the SAME toggle-switch visual `presets_menu`'s `preset_autosave` row
/// uses — rendered LAST inside the section, after the starred entries and
/// before the section-ending separator. Reuses the existing
/// `gran_pin:toggle` hit id / `ToggleGranularityPin` dispatch unchanged
/// (`mlc-app::input::panel_click` already routes it and keeps the dropdown
/// open) — only the visual presentation changed, not the wiring.
fn push_granularity_pin_toggle_row(header: &mut Vec<DropdownItemDef>) {
    header.push(DropdownItemDef::action(
        mlc_core::chart_type::GRANULARITY_PIN_TOGGLE_ACTION_ID,
        "Pin to toolbar",
    ));
}

/// Prepend the FAVORITES section (starred TF rows — builtin or custom) to
/// the top of the `timeframe_selector` dropdown's item list — user-
/// granularity-templates-inner arc, кусок 2 (owner spec 2026-07-16: a star
/// on ANY row, not a separate favorites list).
///
/// Row ids carry the [`mlc_core::chart_type::GRANULARITY_FAV_ID_PREFIX`]
/// prefix (defect fix 2026-07-16) — a FAVORITES-section row is a SEPARATE
/// dropdown entry from its normal-position twin further down the list, so it
/// must have a distinct id or `draw_dropdown`'s id-keyed hover test lights
/// both rows at once (duplicate-id hover-leak bug). The click router
/// (`mlc-app::input::panel_click`) strips the prefix and dispatches through
/// the EXACT same `ChangeTimeframe`/`UpdateChartTypeParam` apply path as the
/// row's normal-position twin — zero new apply-side wiring beyond the strip.
fn prepend_tf_starred_items(
    items: &mut Vec<DropdownItemDef>,
    favorites: &mlc_core::user_profile::GranularityFavorites,
) {
    if favorites.starred_tfs.is_empty() {
        return;
    }
    let mut header: Vec<DropdownItemDef> = Vec::with_capacity(favorites.starred_tfs.len() + 3);
    header.push(DropdownItemDef::Header { label: "FAVORITES".to_string() });
    for name in &favorites.starred_tfs {
        header.push(DropdownItemDef::action(
            format!("{}tf_{}", mlc_core::chart_type::GRANULARITY_FAV_ID_PREFIX, name),
            name.clone(),
        ));
    }
    push_granularity_pin_toggle_row(&mut header);
    header.push(DropdownItemDef::Separator);
    header.append(items);
    *items = header;
}

/// Prepend the FAVORITES section (starred numeric-param rows — ladder or
/// custom) for the active `ResolutionKind::Param` chart type. See
/// [`prepend_tf_starred_items`] for the id-uniqueness/dispatch rationale.
/// Filtered to the active chart_type only (a range_bar star must never leak
/// into the volume_bar dropdown).
fn prepend_param_starred_items(
    items: &mut Vec<DropdownItemDef>,
    active_chart_type: Option<&str>,
    favorites: &mlc_core::user_profile::GranularityFavorites,
) {
    let Some(ct) = active_chart_type else { return };
    let suffix = match mlc_core::chart_type::by_id(ct).map(|def| def.resolution) {
        Some(mlc_core::chart_type::ResolutionKind::Param { suffix }) => suffix,
        _ => return,
    };
    let matching: Vec<&mlc_core::user_profile::ParamFavorite> = favorites.starred_params.iter()
        .filter(|f| f.chart_type == ct)
        .collect();
    if matching.is_empty() {
        return;
    }
    let mut header: Vec<DropdownItemDef> = Vec::with_capacity(matching.len() + 3);
    header.push(DropdownItemDef::Header { label: "FAVORITES".to_string() });
    for fav in matching {
        let label = format_resolution_param(fav.value, suffix);
        header.push(DropdownItemDef::action(
            format!("{}{}{}", mlc_core::chart_type::GRANULARITY_FAV_ID_PREFIX, RESOLUTION_PARAM_ITEM_PREFIX, fav.value),
            label,
        ));
    }
    push_granularity_pin_toggle_row(&mut header);
    header.push(DropdownItemDef::Separator);
    header.append(items);
    *items = header;
}

/// Prepend the FAVORITES section (starred footprint bucket-interval rows —
/// builtin or custom) for `ResolutionKind::Interval` chart types
/// (footprint) — per-family scoping arc 2026-07-16. Mirrors
/// [`prepend_tf_starred_items`] exactly (same flat-list shape as
/// `starred_tfs`, per owner spec: interval favorites are NOT chart-type
/// scoped — footprint is the only `Interval`-resolution chart type today),
/// except row ids use the `ivl_` prefix so a bucket-interval favorite can
/// never collide with (or leak into) a kline `Time` favorite sharing the
/// same literal name (e.g. starred footprint "5m" vs starred kline "5m" are
/// independent entries).
fn prepend_interval_starred_items(
    items: &mut Vec<DropdownItemDef>,
    favorites: &mlc_core::user_profile::GranularityFavorites,
) {
    if favorites.starred_intervals.is_empty() {
        return;
    }
    let mut header: Vec<DropdownItemDef> = Vec::with_capacity(favorites.starred_intervals.len() + 3);
    header.push(DropdownItemDef::Header { label: "FAVORITES".to_string() });
    for name in &favorites.starred_intervals {
        header.push(DropdownItemDef::action(
            format!("{}{}{}", mlc_core::chart_type::GRANULARITY_FAV_ID_PREFIX, mlc_core::chart_type::GRANULARITY_INTERVAL_ID_PREFIX, name),
            name.clone(),
        ));
    }
    push_granularity_pin_toggle_row(&mut header);
    header.push(DropdownItemDef::Separator);
    header.append(items);
    *items = header;
}

/// Append CUSTOM timeframe rows (after a separator, following the builtin
/// ladder) + the trailing "+ Add" row to the `timeframe_selector` dropdown's
/// item list — user-granularity-templates-inner arc, кусок 2.
///
/// Custom rows reuse the `tf_{name}` id convention (same apply-path rationale
/// as [`prepend_tf_starred_items`]); the post-processing closure in
/// `chart_toolbar_render_dropdown` attaches their `[star][edit][trash]`
/// trailing icons (customs only — builtins get star-only). The "+ Add" row
/// (`TF_FAVORITE_CUSTOM_INPUT_ITEM_ID`) begins inline creation — while
/// `editing_text` is `Some` (host == Add row), its label is overridden to
/// the live typed text; the REAL caret is drawn by the widget-overlay block
/// in `chart_toolbar_render_dropdown` (defect fix 2026-07-16 — no more fake
/// `▏` glyph baked into the label).
fn append_tf_customs_and_add_row(
    items: &mut Vec<DropdownItemDef>,
    favorites: &mlc_core::user_profile::GranularityFavorites,
    editing_text: Option<&str>,
) {
    if !favorites.time_tfs.is_empty() {
        items.push(DropdownItemDef::Separator);
        for name in &favorites.time_tfs {
            items.push(DropdownItemDef::action(format!("tf_{}", name), name.clone()));
        }
    }
    let add_label = match editing_text {
        Some(text) => text.to_string(),
        None => "+ Add".to_string(),
    };
    items.push(DropdownItemDef::action(
        mlc_core::chart_type::TF_FAVORITE_CUSTOM_INPUT_ITEM_ID,
        add_label,
    ));
}

/// Append CUSTOM numeric-param rows + the trailing "+ Add" row for the
/// active `ResolutionKind::Param` chart type. See
/// [`append_tf_customs_and_add_row`] for the id-reuse/trailing-icon/live-text
/// rationale. Filtered to the active chart_type only.
fn append_param_customs_and_add_row(
    items: &mut Vec<DropdownItemDef>,
    active_chart_type: Option<&str>,
    favorites: &mlc_core::user_profile::GranularityFavorites,
    editing_text: Option<&str>,
) {
    let Some(ct) = active_chart_type else { return };
    let suffix = match mlc_core::chart_type::by_id(ct).map(|def| def.resolution) {
        Some(mlc_core::chart_type::ResolutionKind::Param { suffix }) => suffix,
        _ => return,
    };
    let matching: Vec<&mlc_core::user_profile::ParamFavorite> = favorites.params.iter()
        .filter(|f| f.chart_type == ct)
        .collect();
    if !matching.is_empty() {
        items.push(DropdownItemDef::Separator);
        for fav in matching {
            let label = format_resolution_param(fav.value, suffix);
            items.push(DropdownItemDef::action(
                format!("{}{}", RESOLUTION_PARAM_ITEM_PREFIX, fav.value),
                label,
            ));
        }
    }
    let add_label = match editing_text {
        Some(text) => text.to_string(),
        None => "+ Add".to_string(),
    };
    items.push(DropdownItemDef::action(
        mlc_core::chart_type::PARAM_FAVORITE_CUSTOM_INPUT_ITEM_ID,
        add_label,
    ));
}

/// Append CUSTOM footprint bucket-interval rows + the trailing "+ Add" row
/// for `ResolutionKind::Interval` chart types — per-family scoping arc
/// 2026-07-16. See [`append_tf_customs_and_add_row`] for the id-reuse/
/// trailing-icon/live-text rationale; uses the `ivl_` id prefix (see
/// [`prepend_interval_starred_items`]) so a custom bucket interval never
/// collides with a kline `Time` custom of the same literal name.
fn append_interval_customs_and_add_row(
    items: &mut Vec<DropdownItemDef>,
    favorites: &mlc_core::user_profile::GranularityFavorites,
    editing_text: Option<&str>,
) {
    if !favorites.interval_tfs.is_empty() {
        items.push(DropdownItemDef::Separator);
        for name in &favorites.interval_tfs {
            items.push(DropdownItemDef::action(
                format!("{}{}", mlc_core::chart_type::GRANULARITY_INTERVAL_ID_PREFIX, name),
                name.clone(),
            ));
        }
    }
    let add_label = match editing_text {
        Some(text) => text.to_string(),
        None => "+ Add".to_string(),
    };
    items.push(DropdownItemDef::action(
        mlc_core::chart_type::INTERVAL_FAVORITE_CUSTOM_INPUT_ITEM_ID,
        add_label,
    ));
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
    account_signed_in: bool,
) -> Option<Vec<DropdownItemDef>> {
        // Special case: symbol_selector's inline fast-search dropdown
        // (symbol-search-compare-inner arc, Wave 2, кусок 4). The toolbar
        // def's own `symbol_selector` entry carries an empty static item
        // list (it never opened a dropdown before this arc — click used to
        // route straight to the full-screen modal), so the base fetch below
        // would return `Some(vec![])`. Returning a non-empty placeholder
        // here (just the input host row) makes `chart_toolbar_render_dropdown`
        // enter its item-building path unconditionally; the REAL dynamic
        // rows (live query results + toggle rows) are appended there
        // (mirrors how granularity favorites are appended to the
        // `timeframe_selector` base list, not built here) because building
        // them needs `symbol_quick_search_render` — live per-frame data this
        // finder fn has no access to (only static toolbar defs + presets).
        if dropdown_id == "symbol_selector" {
            // Inert placeholder — unconditionally replaced by
            // `symbol_quick_dropdown_items` in the render path; exists only
            // so the base fetch is non-empty.
            return Some(vec![DropdownItemDef::action("sym_quick:placeholder", "")]);
        }

        // Special case: magnet is an icon_button that opens a dropdown on double-click.
        // Its items are not in the toolbar definition — return them directly.
        if dropdown_id == "magnet" {
            return Some(vec![
                DropdownItemDef::action("magnet_ohlc", "Weak Magnet (OHLC)").with_icon(ToolbarIconId::new("Magnet")),
                DropdownItemDef::action("magnet_strong", "Strong Magnet (Body)").with_icon(ToolbarIconId::new("MagnetStrong")),
            ]);
        }

        // Special case: screenshot (camera) is an icon_button that opens a
        // destination dropdown on double-click — same shape as magnet above.
        // Selecting a row sets the destination AND fires immediately
        // (`handle_dropdown_select_with_chart`'s `"screenshot"` arm). The
        // ShareLink row exists ONLY for signed-in users (owner ruling —
        // anonymous sessions see exactly Clipboard / DownloadFile).
        if dropdown_id == "screenshot" {
            let mut items = vec![
                DropdownItemDef::action("shot_clipboard", "Clipboard").with_icon(ToolbarIconId::new("Copy")),
                DropdownItemDef::action("shot_download", "Download File").with_icon(ToolbarIconId::new("CloudDownload")),
            ];
            if account_signed_in {
                items.push(DropdownItemDef::action("shot_share_link", "Share Link").with_icon(ToolbarIconId::new("NewWindow")));
            }
            return Some(items);
        }

        // Special case: share_chart (globe) is an icon_button that opens a
        // dropdown on double-click — same shape as magnet/screenshot above
        // (native-share-viewer arc, 2026-09-06). The single-click action
        // stays "Share live chart"; the second row opens the "Open shared
        // chart…" overlay via `mlc_core::toolbar::OPEN_SHARE_ACTION_ID` (see
        // that constant's own doc for why this builder, not static
        // `ToolbarItemDef` data, owns the row list).
        if dropdown_id == "share_chart" {
            return Some(vec![
                DropdownItemDef::action("share_chart", "Share live chart").with_icon(ToolbarIconId::new("Globe")),
                DropdownItemDef::action(mlc_core::toolbar::OPEN_SHARE_ACTION_ID, "Open shared chart…"),
            ]);
        }

        // Special case: the favorites bar's "+" picker (favorite-primitives
        // arc) — a FLAT list of every drawing tool, built from the
        // `DRAWING_FLYOUT_IDS` flyout defs of the canonical left toolbar so
        // ids/labels/icons can never drift from the flyouts themselves
        // (headers/separators stripped, duplicate ids deduped — several tools
        // appear in more than one flyout). Trailing favorite stars are
        // attached in `chart_toolbar_render_dropdown`'s conversion loop.
        if dropdown_id == mlc_core::toolbar::FAVORITES_ADD_DROPDOWN_ID {
            let mut items = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for section in &mlc_core::toolbar::left_toolbar().sections {
                for item in &section.items {
                    if let ToolbarItemDef::Dropdown { id, items: flyout_items, .. } = item {
                        if !mlc_core::toolbar::DRAWING_FLYOUT_IDS.contains(id) {
                            continue;
                        }
                        for flyout_item in flyout_items {
                            if let DropdownItemDef::Action { id: action_id, .. } = flyout_item {
                                if seen.insert(action_id.clone()) {
                                    items.push(flyout_item.clone());
                                }
                            }
                        }
                    }
                }
            }
            return Some(items);
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
                ToolbarItemDef::Button { id, icon, text, min_width, strikethrough, .. } => {
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
                            strikethrough: *strikethrough,
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
/// Every icon id used by the dropdown/trailing-icon path resolves through
/// `icon_svg`'s name→SVG registry — no text-glyph fallback (the pinned-
/// favorites-row toggle's `"pin"` id used to fall back to a 📌 glyph before
/// `ICON_PIN` was added to the registry, defect fix 2026-07-16).
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
            } else if let Some(preset_id) = id.strip_prefix("theme_") {
                // Theme dropdown rows carry no icon — a colour sample goes
                // in that same leading slot instead (`toolbar.rs`'s
                // `theme_submenu_items` is the only producer of `theme_*`
                // action ids, all of them real preset ids).
                di = di.with_swatch(theme_sample_tokens(preset_id));
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
