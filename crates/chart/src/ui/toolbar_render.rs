//! Self-contained toolbar renderer for the chart panel.
//!
//! Renders a [`PanelToolbarDef`] directly using [`RenderContext`], without
//! depending on `zengeld-core`.  The caller supplies layout information via
//! [`ToolbarRect`] and appearance via [`ToolbarTheme`].

use std::collections::HashMap;

use uzor::panel_api::{
    DropdownItemDef, PanelToolbarDef, SectionAlign, ToolbarItemDef, ToolbarOrientation,
};
use uzor::render::{draw_svg_icon, draw_svg_multicolor, RenderContext};

use super::icons::icon_svg;

const MINI_MASCOT_SVG: &str = include_str!("../../../../assets/mascot/mini_mascot.svg");

// =============================================================================
// Public types
// =============================================================================

/// Axis-aligned rectangle used for toolbar layout and hit-testing.
#[derive(Clone, Copy, Debug, Default)]
pub struct ToolbarRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ToolbarRect {
    /// Create a new rect from position and size.
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, width: w, height: h }
    }

    /// Right edge (exclusive).
    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    /// Bottom edge (exclusive).
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    /// Horizontal centre.
    pub fn center_x(&self) -> f64 {
        self.x + self.width / 2.0
    }

    /// Vertical centre.
    pub fn center_y(&self) -> f64 {
        self.y + self.height / 2.0
    }

    /// Returns `true` if the point `(px, py)` is inside the rect.
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }
}

// ---------------------------------------------------------------------------

/// Visual theme for toolbar rendering.
#[derive(Clone, Debug)]
pub struct ToolbarTheme {
    /// Toolbar background fill colour (CSS hex or rgba string).
    pub background: String,
    /// Dropdown / context menu background colour.
    ///
    /// This is a separate slot from `background` because dropdown menus
    /// often use a slightly different shade (e.g. slightly lighter/darker
    /// than the toolbar).  Modals that render inline dropdowns should use
    /// this field rather than `background`.
    pub dropdown_bg: String,
    /// Separator line colour.
    pub separator: String,
    /// Item background when hovered.
    pub item_bg_hover: String,
    /// Item background when active/selected.
    pub item_bg_active: String,
    /// Button background colour (idle state).
    pub button_bg: String,
    /// Button background colour on hover.
    pub button_bg_hover: String,
    /// Default icon / label colour.
    pub item_text: String,
    /// Muted icon / label colour (for secondary/disabled items).
    pub item_text_muted: String,
    /// Colour for hidden (invisible) items — item_text at 50% opacity.
    pub item_text_hidden: String,
    /// Icon / label colour on hover.
    pub item_text_hover: String,
    /// Icon / label colour when active.
    pub item_text_active: String,
    /// Accent colour for active indicator dots etc.
    pub accent: String,
    /// Accent colour on hover.
    pub accent_hover: String,
    /// Success colour (confirmations, positive states).
    pub success: String,
    /// Danger colour (errors, destructive actions, logout).
    pub danger: String,
    /// Warning colour (cautions, alerts).
    pub warning: String,
    /// When `true` the toolbar is rendered in a compact sidebar style
    /// (no background blur, narrower padding).
    pub sidebar_style: bool,
}

impl Default for ToolbarTheme {
    fn default() -> Self {
        Self {
            background: "#1e1e2e".into(),
            dropdown_bg: "#1e222d".into(),
            separator: "#313244".into(),
            item_bg_hover: "#45475a".into(),
            item_bg_active: "#585b70".into(),
            button_bg: "#1e222d".into(),
            button_bg_hover: "#2a2e39".into(),
            item_text: "#a6adc8".into(),
            item_text_muted: "#6c7086".into(),
            item_text_hidden: "#a6adc880".into(),
            item_text_hover: "#cdd6f4".into(),
            item_text_active: "#cdd6f4".into(),
            accent: "#3b82f6".into(),
            accent_hover: "#1e53e4".into(),
            success: "#26a69a".into(),
            danger: "#f23645".into(),
            warning: "#ff9800".into(),
            sidebar_style: false,
        }
    }
}

// ---------------------------------------------------------------------------

/// Output of [`render_panel_toolbar`].
///
/// Contains hit zones that the caller can use to map pointer events back to
/// toolbar items.
#[derive(Clone, Debug, Default)]
pub struct ToolbarRenderResult {
    /// `(item_id, rect)` pairs for every rendered interactive item.
    pub item_rects: Vec<(String, ToolbarRect)>,
}

// =============================================================================
// Main entry point
// =============================================================================

/// Render a [`PanelToolbarDef`] into `ctx`.
///
/// # Parameters
/// - `ctx`               – mutable render context
/// - `def`               – toolbar definition produced by the chart's `toolbar.rs`
/// - `rect`              – the pixel region the toolbar occupies
/// - `theme`             – colour / style settings
/// - `active_tool_id`    – ID of the currently-active drawing tool (for
///                          highlighting the matching button)
/// - `hovered_id`        – ID of the item currently under the pointer
/// - `toggled`           – map of item IDs whose toggle state is `true`
///                          (e.g. magnet, lock, eye)
/// - `quick_select_icons`– map of dropdown ID → icon name override, used when
///                          the user has previously selected a child tool
pub fn render_panel_toolbar(
    ctx: &mut dyn RenderContext,
    def: &PanelToolbarDef,
    rect: ToolbarRect,
    theme: &ToolbarTheme,
    active_tool_id: Option<&str>,
    hovered_id: Option<&str>,
    toggled: &HashMap<String, bool>,
    quick_select_icons: &HashMap<String, String>,
) -> ToolbarRenderResult {
    let mut result = ToolbarRenderResult::default();

    // -----------------------------------------------------------------------
    // Background
    // -----------------------------------------------------------------------
    ctx.set_fill_color(&theme.background);
    ctx.fill_rect(rect.x, rect.y, rect.width, rect.height);

    let is_vertical = def.orientation == ToolbarOrientation::Vertical;

    // -----------------------------------------------------------------------
    // Separate start-aligned and end-aligned sections
    // -----------------------------------------------------------------------
    let start_sections: Vec<&_> = def
        .sections
        .iter()
        .filter(|s| s.align == SectionAlign::Start)
        .collect();
    let end_sections: Vec<&_> = def
        .sections
        .iter()
        .filter(|s| s.align == SectionAlign::End)
        .collect();

    let padding = def.padding;
    let spacing = def.spacing;
    let item_size = def.item_size;

    // -----------------------------------------------------------------------
    // Layout cursor — we advance along the primary axis
    // -----------------------------------------------------------------------

    // Render start-aligned sections from the beginning of the toolbar
    let mut pos = if is_vertical { rect.y + padding } else { rect.x + padding };

    for section in &start_sections {
        if section.show_separator && pos > (if is_vertical { rect.y } else { rect.x }) + padding {
            draw_separator(ctx, is_vertical, &rect, pos, theme);
            pos += 6.0; // separator thickness + gap
        }

        for item in &section.items {
            let consumed = render_item(
                ctx,
                item,
                is_vertical,
                &rect,
                pos,
                item_size,
                padding,
                theme,
                active_tool_id,
                hovered_id,
                toggled,
                quick_select_icons,
                def.icon_size,
                &mut result.item_rects,
            );
            pos += consumed + spacing;
        }
    }

    // Render end-aligned sections from the end of the toolbar (reverse order)
    let mut end_pos = if is_vertical { rect.bottom() - padding } else { rect.right() - padding };

    // Calculate total width of end sections first
    let end_total = end_sections.iter().flat_map(|s| s.items.iter()).fold(0.0, |acc, item| {
        acc + item_natural_size(item, item_size) + spacing
    });
    end_pos -= end_total;
    if end_pos < pos {
        end_pos = pos; // prevent overlap
    }

    let mut end_cursor = end_pos;
    for section in &end_sections {
        if section.show_separator {
            draw_separator(ctx, is_vertical, &rect, end_cursor, theme);
            end_cursor += 6.0;
        }

        for item in &section.items {
            let consumed = render_item(
                ctx,
                item,
                is_vertical,
                &rect,
                end_cursor,
                item_size,
                padding,
                theme,
                active_tool_id,
                hovered_id,
                toggled,
                quick_select_icons,
                def.icon_size,
                &mut result.item_rects,
            );
            end_cursor += consumed + spacing;
        }
    }

    result
}

// =============================================================================
// Internal helpers
// =============================================================================

/// Draw a single toolbar separator line.
fn draw_separator(
    ctx: &mut dyn RenderContext,
    is_vertical: bool,
    rect: &ToolbarRect,
    pos: f64,
    theme: &ToolbarTheme,
) {
    ctx.set_stroke_color(&theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.set_line_dash(&[]);
    ctx.begin_path();
    if is_vertical {
        let margin = rect.width * 0.2;
        ctx.move_to(rect.x + margin, pos);
        ctx.line_to(rect.right() - margin, pos);
    } else {
        let margin = rect.height * 0.2;
        ctx.move_to(pos, rect.y + margin);
        ctx.line_to(pos, rect.bottom() - margin);
    }
    ctx.stroke();
}

/// Return the primary-axis size of an item without rendering it.
fn item_natural_size(item: &ToolbarItemDef, item_size: f64) -> f64 {
    match item {
        ToolbarItemDef::Separator => 6.0,
        ToolbarItemDef::Spacer => 8.0,
        ToolbarItemDef::Button { min_width, .. } | ToolbarItemDef::Dropdown { min_width, .. } => {
            if *min_width > 0.0 { *min_width } else { item_size }
        }
        ToolbarItemDef::IconButton { .. } => item_size,
    }
}

/// Render a single toolbar item and return the pixels it consumed on the
/// primary axis.
#[allow(clippy::too_many_arguments)]
fn render_item(
    ctx: &mut dyn RenderContext,
    item: &ToolbarItemDef,
    is_vertical: bool,
    rect: &ToolbarRect,
    pos: f64,
    item_size: f64,
    padding: f64,
    theme: &ToolbarTheme,
    active_tool_id: Option<&str>,
    hovered_id: Option<&str>,
    toggled: &HashMap<String, bool>,
    quick_select_icons: &HashMap<String, String>,
    icon_size: f64,
    hit_zones: &mut Vec<(String, ToolbarRect)>,
) -> f64 {
    match item {
        ToolbarItemDef::Separator => {
            draw_separator(ctx, is_vertical, rect, pos + 3.0, theme);
            6.0
        }
        ToolbarItemDef::Spacer => 8.0,

        ToolbarItemDef::IconButton { id, icon, .. } => {
            let item_rect = make_item_rect(is_vertical, rect, pos, item_size, padding);
            let is_hovered = hovered_id == Some(id);
            let is_active = toggled.get(*id).copied().unwrap_or(false);

            draw_item_bg(ctx, &item_rect, is_hovered, is_active, theme);

            let icon_name = icon.name();
            let color = pick_color(is_hovered, is_active, theme);
            render_icon(ctx, icon_name, &item_rect, icon_size, color);

            hit_zones.push((id.to_string(), item_rect));
            item_size
        }

        ToolbarItemDef::Button { id, icon, text, min_width, .. } => {
            let natural = if *min_width > 0.0 { *min_width } else { item_size };
            let item_rect = make_item_rect(is_vertical, rect, pos, natural, padding);
            let is_hovered = hovered_id == Some(id);
            let is_active = active_tool_id == Some(id)
                || toggled.get(*id).copied().unwrap_or(false);

            draw_item_bg(ctx, &item_rect, is_hovered, is_active, theme);

            let color = pick_color(is_hovered, is_active, theme);

            // Icon (left side when horizontal, centred when vertical)
            if let Some(icon_id) = icon {
                render_icon(ctx, icon_id.name(), &item_rect, icon_size, color);
            }

            // Text label
            if let Some(label) = text {
                draw_label(ctx, label, &item_rect, color, icon.is_some());
            }

            hit_zones.push((id.to_string(), item_rect));
            natural
        }

        ToolbarItemDef::Dropdown { id, icon, text, quick_select, min_width, items, .. } => {
            let natural = if *min_width > 0.0 { *min_width } else { item_size };
            let item_rect = make_item_rect(is_vertical, rect, pos, natural, padding);
            let is_hovered = hovered_id == Some(id);

            // For quick-select dropdowns, active when any child matches active_tool_id.
            // For regular dropdowns, active when id matches active_tool_id.
            let is_active = if *quick_select {
                active_tool_id
                    .map(|tid| is_tool_in_items(tid, items))
                    .unwrap_or(false)
            } else {
                active_tool_id == Some(id)
            };

            draw_item_bg(ctx, &item_rect, is_hovered, is_active, theme);

            let color = pick_color(is_hovered, is_active, theme);

            // Determine which icon to show:
            // 1. quick_select_icons override (user's last-selected child tool)
            // 2. the dropdown's own icon field
            let effective_icon_name: Option<String> = quick_select_icons
                .get(*id)
                .cloned()
                .or_else(|| icon.as_ref().map(|i| i.name().to_string()));

            if let Some(ref icon_name) = effective_icon_name {
                render_icon(ctx, icon_name, &item_rect, icon_size, color);
            }

            // Text label (e.g. selected timeframe "1H", selected symbol "BTCUSDT")
            if let Some(label) = text {
                draw_label(ctx, label, &item_rect, color, effective_icon_name.is_some());
            }

            // Chevron indicator for regular (non-quick-select) dropdowns with text
            if !quick_select && text.is_some() {
                draw_chevron(ctx, &item_rect, is_vertical, color);
            }

            hit_zones.push((id.to_string(), item_rect));
            natural
        }
    }
}

/// Build the pixel rect for an item given its position on the primary axis.
fn make_item_rect(
    is_vertical: bool,
    toolbar_rect: &ToolbarRect,
    pos: f64,
    size: f64,
    padding: f64,
) -> ToolbarRect {
    if is_vertical {
        // Vertical toolbar: items are centred horizontally, stack vertically
        let inset = padding * 0.5;
        ToolbarRect::new(
            toolbar_rect.x + inset,
            pos,
            toolbar_rect.width - inset * 2.0,
            size,
        )
    } else {
        // Horizontal toolbar: items are centred vertically, laid out horizontally
        let inset = padding * 0.5;
        ToolbarRect::new(
            pos,
            toolbar_rect.y + inset,
            size,
            toolbar_rect.height - inset * 2.0,
        )
    }
}

/// Draw the hover / active background rect for an item.
fn draw_item_bg(
    ctx: &mut dyn RenderContext,
    item_rect: &ToolbarRect,
    is_hovered: bool,
    is_active: bool,
    theme: &ToolbarTheme,
) {
    if is_active {
        ctx.set_fill_color(&theme.item_bg_active);
        fill_rounded_rect(ctx, item_rect, 4.0);
    } else if is_hovered {
        ctx.set_fill_color(&theme.item_bg_hover);
        fill_rounded_rect(ctx, item_rect, 4.0);
    }
}

/// Fill a rounded rectangle, delegating to the context's built-in helper.
fn fill_rounded_rect(ctx: &mut dyn RenderContext, r: &ToolbarRect, radius: f64) {
    ctx.fill_rounded_rect(r.x, r.y, r.width, r.height, radius);
}

/// Choose the appropriate text/icon colour based on state.
fn pick_color(is_hovered: bool, is_active: bool, theme: &ToolbarTheme) -> &str {
    if is_active {
        &theme.item_text_active
    } else if is_hovered {
        &theme.item_text_hover
    } else {
        &theme.item_text
    }
}

/// Render an SVG icon centred in `item_rect`.
fn render_icon(
    ctx: &mut dyn RenderContext,
    icon_name: &str,
    item_rect: &ToolbarRect,
    icon_size: f64,
    color: &str,
) {
    if icon_name == "Bot" {
        let ix = (item_rect.center_x() - icon_size / 2.0).floor();
        let iy = (item_rect.center_y() - icon_size / 2.0).floor();
        draw_svg_multicolor(ctx, MINI_MASCOT_SVG, ix, iy, icon_size, icon_size);
        return;
    }
    if let Some(svg) = icon_svg(icon_name) {
        let ix = (item_rect.center_x() - icon_size / 2.0).floor();
        let iy = (item_rect.center_y() - icon_size / 2.0).floor();
        draw_svg_icon(ctx, svg, ix, iy, icon_size, icon_size, color);
    }
}

/// Draw a simple text label inside `item_rect`.
///
/// `has_icon` shifts the text to the right so it doesn't overlap the icon.
fn draw_label(
    ctx: &mut dyn RenderContext,
    text: &str,
    item_rect: &ToolbarRect,
    color: &str,
    has_icon: bool,
) {
    ctx.set_fill_color(color);
    ctx.set_font("11px sans-serif");
    let x = if has_icon {
        item_rect.x + item_rect.height + 2.0 // icon occupies item_height worth of space
    } else {
        item_rect.center_x()
    };
    ctx.fill_text(text, x, item_rect.center_y() + 4.0);
}

/// Draw a small downward-pointing chevron to indicate a dropdown.
fn draw_chevron(
    ctx: &mut dyn RenderContext,
    item_rect: &ToolbarRect,
    _is_vertical: bool,
    color: &str,
) {
    let cx = item_rect.right() - 8.0;
    let cy = item_rect.center_y();
    let half = 3.0_f64;
    ctx.set_stroke_color(color);
    ctx.set_stroke_width(1.5);
    ctx.set_line_dash(&[]);
    ctx.begin_path();
    ctx.move_to(cx - half, cy - 1.5);
    ctx.line_to(cx, cy + 1.5);
    ctx.line_to(cx + half, cy - 1.5);
    ctx.stroke();
}

// =============================================================================
// Recursive helper: check whether a tool ID appears among dropdown items
// =============================================================================

/// Returns `true` if `tool_id` matches the ID of any `Action` leaf reachable
/// from `items` (including nested `Submenu` items).
fn is_tool_in_items(tool_id: &str, items: &[DropdownItemDef]) -> bool {
    for item in items {
        match item {
            DropdownItemDef::Action { id, .. } => {
                if *id == tool_id {
                    return true;
                }
            }
            DropdownItemDef::Submenu { items: sub_items, .. } => {
                if is_tool_in_items(tool_id, sub_items) {
                    return true;
                }
            }
            DropdownItemDef::Header { .. } | DropdownItemDef::Separator => {}
        }
    }
    false
}
