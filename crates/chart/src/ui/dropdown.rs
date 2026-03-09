//! Dropdown/popup menu rendering — copied from zengeld-core for chart panel self-containment.
//!
//! This is an exact copy of `zengeld-core/src/ui/render/dropdown.rs` adapted to
//! work without core dependencies. Uses `uzor::render::RenderContext` directly.

use uzor::render::{RenderContext, TextAlign, TextBaseline};
use super::toolbar_core::{WidgetRect, IconId};

/// Dropdown menu item
#[derive(Clone, Debug)]
pub enum DropdownItem {
    /// Regular menu item
    Item {
        id: String,
        icon: Option<IconId>,
        label: String,
        shortcut: Option<String>,
        disabled: bool,
        danger: bool,
        /// Muted text shown right-aligned (uses `shortcut_text` color from theme)
        subtitle: Option<String>,
        /// If set, draws a 2px colored vertical bar on the left edge of the item
        accent_color: Option<String>,
        /// If Some, draws a toggle switch on the right. The bool is the current state.
        /// Label becomes non-clickable; only the toggle area triggers the action.
        toggle: Option<bool>,
    },
    /// Separator line
    Separator,
    /// Header/title (non-clickable)
    Header {
        label: String,
    },
    /// Submenu (shows arrow, opens another menu)
    Submenu {
        id: String,
        icon: Option<IconId>,
        label: String,
    },
}

impl DropdownItem {
    pub fn item(id: &str, label: &str) -> Self {
        Self::Item {
            id: id.to_string(),
            icon: None,
            label: label.to_string(),
            shortcut: None,
            disabled: false,
            danger: false,
            subtitle: None,
            accent_color: None,
            toggle: None,
        }
    }

    pub fn with_icon(mut self, icon: impl Into<IconId>) -> Self {
        if let Self::Item { icon: ref mut i, .. } = self {
            *i = Some(icon.into());
        }
        self
    }

    pub fn with_shortcut(mut self, shortcut: &str) -> Self {
        if let Self::Item { shortcut: ref mut s, .. } = self {
            *s = Some(shortcut.to_string());
        }
        self
    }

    pub fn with_danger(mut self) -> Self {
        if let Self::Item { danger: ref mut d, .. } = self {
            *d = true;
        }
        self
    }

    pub fn with_disabled(mut self) -> Self {
        if let Self::Item { disabled: ref mut d, .. } = self {
            *d = true;
        }
        self
    }

    /// Set muted subtitle text shown right-aligned (displayed only when no shortcut is set).
    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        if let Self::Item { subtitle: ref mut s, .. } = self {
            *s = Some(subtitle.into());
        }
        self
    }

    /// Set a colored accent bar drawn on the left edge of the item.
    pub fn with_accent_color(mut self, color: impl Into<String>) -> Self {
        if let Self::Item { accent_color: ref mut c, .. } = self {
            *c = Some(color.into());
        }
        self
    }

    /// Add a toggle switch on the right side of the item.
    pub fn with_toggle(mut self, enabled: bool) -> Self {
        if let Self::Item { toggle: ref mut t, .. } = self {
            *t = Some(enabled);
        }
        self
    }

    pub fn separator() -> Self {
        Self::Separator
    }

    pub fn header(label: &str) -> Self {
        Self::Header {
            label: label.to_string(),
        }
    }

    pub fn submenu(id: &str, label: &str) -> Self {
        Self::Submenu {
            id: id.to_string(),
            icon: None,
            label: label.to_string(),
        }
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Item { id, .. } => Some(id),
            Self::Submenu { id, .. } => Some(id),
            Self::Separator | Self::Header { .. } => None,
        }
    }
}

/// Dropdown menu configuration
#[derive(Clone, Debug)]
pub struct DropdownConfig {
    /// Menu items
    pub items: Vec<DropdownItem>,
    /// Minimum width
    pub min_width: f64,
    /// Maximum width (0 for unlimited)
    pub max_width: f64,
    /// Item height
    pub item_height: f64,
    /// Separator height
    pub separator_height: f64,
    /// Header height
    pub header_height: f64,
    /// Padding around menu
    pub padding: f64,
    /// Item horizontal padding
    pub item_padding_x: f64,
    /// Corner radius
    pub radius: f64,
    /// Icon size
    pub icon_size: f64,
    /// Font size
    pub font_size: f64,
    /// Shadow blur
    pub shadow_blur: f64,
    /// If Some(n), render as grid with n columns instead of vertical list
    pub grid_columns: Option<u8>,
}

impl Default for DropdownConfig {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            min_width: 180.0,
            max_width: 300.0,
            item_height: 32.0,
            separator_height: 9.0,
            header_height: 28.0,
            padding: 4.0,
            item_padding_x: 12.0,
            radius: 4.0,
            icon_size: 16.0,
            font_size: 13.0,
            shadow_blur: 24.0,
            grid_columns: None,
        }
    }
}

impl DropdownConfig {
    pub fn new(items: Vec<DropdownItem>) -> Self {
        Self {
            items,
            ..Default::default()
        }
    }

    /// Create a grid dropdown with specified columns
    pub fn new_grid(items: Vec<DropdownItem>, columns: u8) -> Self {
        Self {
            items,
            grid_columns: Some(columns),
            ..Default::default()
        }
    }

    /// Check if this is a grid layout dropdown
    pub fn is_grid(&self) -> bool {
        self.grid_columns.is_some()
    }

    /// Calculate required height for the menu
    pub fn calculate_height(&self) -> f64 {
        let mut height = self.padding * 2.0;
        for item in &self.items {
            height += match item {
                DropdownItem::Item { .. } | DropdownItem::Submenu { .. } => self.item_height,
                DropdownItem::Separator => self.separator_height,
                DropdownItem::Header { .. } => self.header_height,
            };
        }
        height
    }
}

/// Dropdown menu theme
#[derive(Clone, Debug)]
pub struct DropdownTheme {
    pub background: String,
    pub border: String,
    pub shadow: String,
    pub item_text: String,
    pub item_text_hover: String,
    pub item_text_disabled: String,
    pub item_bg_hover: String,
    pub item_danger: String,
    pub item_danger_bg_hover: String,
    pub header_text: String,
    pub header_border: String,
    pub separator: String,
    pub shortcut_text: String,
}

impl Default for DropdownTheme {
    fn default() -> Self {
        Self {
            background: "#1e222d".to_string(),
            border: "#363a45".to_string(),
            shadow: "rgba(0,0,0,0.5)".to_string(),
            item_text: "#d1d4dc".to_string(),
            item_text_hover: "#ffffff".to_string(),
            item_text_disabled: "#6a6d78".to_string(),
            item_bg_hover: "#2a2e39".to_string(),
            item_danger: "#f23645".to_string(),
            item_danger_bg_hover: "rgba(242,54,69,0.15)".to_string(),
            header_text: "#ffffff".to_string(),
            header_border: "#363a45".to_string(),
            separator: "#363a45".to_string(),
            shortcut_text: "#6a6d78".to_string(),
        }
    }
}

/// Dropdown rendering result
#[derive(Clone, Debug, Default)]
pub struct DropdownResult {
    /// ID of clicked item (if any)
    pub clicked: Option<String>,
    /// ID of hovered item (if any)
    pub hovered: Option<String>,
    /// ID of submenu to open (if hovering submenu item)
    pub open_submenu: Option<String>,
    /// Item rectangles (for hit testing)
    pub item_rects: Vec<(String, WidgetRect)>,
    /// Total menu rectangle
    pub menu_rect: WidgetRect,
}

/// Draw a dropdown menu popup
///
/// # Parameters
/// - `ctx` - Render context
/// - `config` - Dropdown configuration
/// - `origin` - Top-left position of the menu
/// - `theme` - Dropdown theme
/// - `hovered_id` - Currently hovered item ID
/// - `draw_icon` - Callback to draw icons
///
/// # Returns
/// Dropdown result with item rectangles
pub fn draw_dropdown<F>(
    ctx: &mut dyn RenderContext,
    config: &DropdownConfig,
    origin: (f64, f64),
    theme: &DropdownTheme,
    hovered_id: Option<&str>,
    mut draw_icon: F,
) -> DropdownResult
where
    F: FnMut(&mut dyn RenderContext, &IconId, WidgetRect, &str),
{
    let mut result = DropdownResult::default();

    // Calculate content-based width by measuring all items
    ctx.set_font(&format!("{}px sans-serif", config.font_size));

    let mut content_width = config.min_width;

    for item in &config.items {
        match item {
            DropdownItem::Item { id: _, label, icon, shortcut, disabled: _, danger: _, subtitle, accent_color: _, toggle } => {
                // Measure label text
                let label_width = ctx.measure_text(label);
                let mut item_width = label_width;

                // Add icon space if present
                if icon.is_some() {
                    item_width += config.icon_size + 8.0; // icon + spacing
                }

                // Add toggle space if present (36px track + 8px gap)
                if toggle.is_some() {
                    item_width += 44.0;
                } else if let Some(sc) = shortcut {
                    let shortcut_width = ctx.measure_text(sc);
                    item_width += shortcut_width + 16.0; // spacing between label and shortcut
                } else if let Some(sub) = subtitle {
                    // Subtitle is shown only when there's no shortcut
                    let sub_width = ctx.measure_text(sub);
                    item_width += sub_width + 16.0;
                }

                // Add horizontal padding (both sides)
                item_width += config.item_padding_x * 2.0;

                // Track maximum width
                content_width = content_width.max(item_width);
            }
            DropdownItem::Header { label } => {
                let header_width = ctx.measure_text(label) + config.item_padding_x * 2.0;
                content_width = content_width.max(header_width);
            }
            DropdownItem::Separator => {
                // Separators don't affect width
            }
            DropdownItem::Submenu { id: _, icon, label } => {
                // Measure label text
                let label_width = ctx.measure_text(label);
                let mut item_width = label_width;

                // Add icon space if present
                if icon.is_some() {
                    item_width += config.icon_size + 8.0; // icon + spacing
                }

                // Add submenu arrow space
                item_width += 12.0; // arrow + spacing

                // Add horizontal padding (both sides)
                item_width += config.item_padding_x * 2.0;

                // Track maximum width
                content_width = content_width.max(item_width);
            }
        }
    }

    // Clamp to max_width if it's set (max_width > 0)
    let menu_width = if config.max_width > 0.0 {
        content_width.min(config.max_width)
    } else {
        content_width
    };

    let menu_height = config.calculate_height();

    let menu_rect = WidgetRect::new(origin.0, origin.1, menu_width, menu_height);
    result.menu_rect = menu_rect;

    // Draw shadow (simplified - just a slightly larger darker rect)
    ctx.set_fill_color("rgba(0,0,0,0.3)");
    ctx.fill_rounded_rect(
        menu_rect.x + 2.0,
        menu_rect.y + 4.0,
        menu_rect.width,
        menu_rect.height,
        config.radius,
    );

    // Blur background (FrostedGlass/LiquidGlass)
    ctx.draw_blur_background(menu_rect.x, menu_rect.y, menu_rect.width, menu_rect.height);

    // Draw background
    ctx.set_fill_color(&theme.background);
    ctx.fill_rounded_rect(menu_rect.x, menu_rect.y, menu_rect.width, menu_rect.height, config.radius);

    // Draw border
    ctx.set_stroke_color(&theme.border);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(menu_rect.x, menu_rect.y, menu_rect.width, menu_rect.height, config.radius);

    // Draw items
    let mut y = menu_rect.y + config.padding;

    for item in &config.items {
        match item {
            DropdownItem::Item { id, icon, label, shortcut, disabled, danger, subtitle, accent_color, toggle } => {
                let item_rect = WidgetRect::new(
                    menu_rect.x + config.padding,
                    y,
                    menu_rect.width - config.padding * 2.0,
                    config.item_height,
                );

                let is_hovered = hovered_id == Some(id.as_str()) && !*disabled;

                // Determine colors
                let (bg_color, text_color) = if *disabled {
                    (None, &theme.item_text_disabled)
                } else if *danger {
                    if is_hovered {
                        (Some(&theme.item_danger_bg_hover), &theme.item_danger)
                    } else {
                        (None, &theme.item_danger)
                    }
                } else if is_hovered {
                    (Some(&theme.item_bg_hover), &theme.item_text_hover)
                } else {
                    (None, &theme.item_text)
                };

                // Draw hover background
                if let Some(bg) = bg_color {
                    ctx.set_fill_color(bg);
                    ctx.fill_rounded_rect(item_rect.x, item_rect.y, item_rect.width, item_rect.height, 2.0);
                }

                // Draw accent bar on the left edge (2px wide, inset 4px from top and bottom)
                if let Some(ref accent) = accent_color {
                    ctx.set_fill_color(accent);
                    ctx.fill_rounded_rect(
                        item_rect.x,
                        item_rect.y + 4.0,
                        2.0,
                        item_rect.height - 8.0,
                        1.0,
                    );
                }

                // Draw icon
                let mut text_x = item_rect.x + config.item_padding_x;
                if let Some(icon) = icon {
                    let icon_rect = WidgetRect::new(
                        text_x,
                        item_rect.center_y() - config.icon_size / 2.0,
                        config.icon_size,
                        config.icon_size,
                    );
                    draw_icon(ctx, icon, icon_rect, text_color);
                    text_x += config.icon_size + 8.0;
                }

                // Draw label
                ctx.set_font(&format!("{}px sans-serif", config.font_size));
                ctx.set_fill_color(text_color);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(label, text_x, item_rect.center_y());

                // Draw toggle switch OR shortcut/subtitle on the right
                if let Some(is_on) = toggle {
                    // Toggle switch: 36x18 rounded pill
                    let track_w = 36.0;
                    let track_h = 18.0;
                    let track_x = item_rect.right() - config.item_padding_x - track_w;
                    let track_y = item_rect.center_y() - track_h / 2.0;
                    let track_r = track_h / 2.0;

                    // Track background
                    if *is_on {
                        ctx.set_fill_color("#2962ff");
                    } else {
                        ctx.set_fill_color(&theme.item_text_disabled);
                    }
                    ctx.fill_rounded_rect(track_x, track_y, track_w, track_h, track_r);

                    // Thumb circle (14px diameter, 2px inset) — drawn as rounded rect
                    let thumb_d = 14.0;
                    let thumb_x = if *is_on {
                        track_x + track_w - thumb_d - 2.0
                    } else {
                        track_x + 2.0
                    };
                    let thumb_y = track_y + (track_h - thumb_d) / 2.0;
                    ctx.set_fill_color("#ffffff");
                    ctx.fill_rounded_rect(thumb_x, thumb_y, thumb_d, thumb_d, thumb_d / 2.0);
                } else if let Some(shortcut) = shortcut {
                    // Draw shortcut (takes priority over subtitle)
                    ctx.set_fill_color(&theme.shortcut_text);
                    ctx.set_text_align(TextAlign::Right);
                    ctx.fill_text(shortcut, item_rect.right() - config.item_padding_x, item_rect.center_y());
                } else if let Some(ref sub) = subtitle {
                    // Draw subtitle only when no shortcut is present
                    ctx.set_fill_color(&theme.shortcut_text);
                    ctx.set_font(&format!("{}px sans-serif", config.font_size - 1.0));
                    ctx.set_text_align(TextAlign::Right);
                    ctx.fill_text(sub, item_rect.right() - config.item_padding_x, item_rect.center_y());
                }

                // Register click hit zone for all items so clicks inside the dropdown
                // are always consumed and don't fall through to the background handler.
                // Disabled items use a "__noop__:" prefix — the caller must treat them
                // as "click consumed, no action, keep dropdown open".
                if *disabled {
                    result.item_rects.push((format!("__noop__:{}", id), item_rect));
                } else {
                    result.item_rects.push((id.clone(), item_rect));
                }
                if is_hovered {
                    result.hovered = Some(id.clone());
                }

                y += config.item_height;
            }
            DropdownItem::Separator => {
                let sep_y = y + config.separator_height / 2.0;
                ctx.set_stroke_color(&theme.separator);
                ctx.set_stroke_width(1.0);
                ctx.begin_path();
                ctx.move_to(menu_rect.x + config.padding, sep_y);
                ctx.line_to(menu_rect.right() - config.padding, sep_y);
                ctx.stroke();

                y += config.separator_height;
            }
            DropdownItem::Header { label } => {
                let header_rect = WidgetRect::new(
                    menu_rect.x + config.padding,
                    y,
                    menu_rect.width - config.padding * 2.0,
                    config.header_height,
                );

                ctx.set_font(&format!("bold {}px sans-serif", config.font_size));
                ctx.set_fill_color(&theme.header_text);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(label, menu_rect.x + config.item_padding_x, y + config.header_height / 2.0);

                // Header bottom border
                ctx.set_stroke_color(&theme.header_border);
                ctx.set_stroke_width(1.0);
                ctx.begin_path();
                ctx.move_to(menu_rect.x + config.padding, y + config.header_height - 1.0);
                ctx.line_to(menu_rect.right() - config.padding, y + config.header_height - 1.0);
                ctx.stroke();

                // Register a noop hit zone so clicks on the header are consumed
                // and don't fall through to the background (which would close the dropdown).
                result.item_rects.push((
                    format!("__noop__:header:{}", label),
                    header_rect,
                ));

                y += config.header_height;
            }
            DropdownItem::Submenu { id, icon, label } => {
                let item_rect = WidgetRect::new(
                    menu_rect.x + config.padding,
                    y,
                    menu_rect.width - config.padding * 2.0,
                    config.item_height,
                );

                let is_hovered = hovered_id == Some(id.as_str());

                // Draw hover background
                if is_hovered {
                    ctx.set_fill_color(&theme.item_bg_hover);
                    ctx.fill_rounded_rect(item_rect.x, item_rect.y, item_rect.width, item_rect.height, 2.0);
                }

                let text_color = if is_hovered { &theme.item_text_hover } else { &theme.item_text };

                // Draw icon
                let mut text_x = item_rect.x + config.item_padding_x;
                if let Some(icon) = icon {
                    let icon_rect = WidgetRect::new(
                        text_x,
                        item_rect.center_y() - config.icon_size / 2.0,
                        config.icon_size,
                        config.icon_size,
                    );
                    draw_icon(ctx, icon, icon_rect, text_color);
                    text_x += config.icon_size + 8.0;
                }

                // Draw label
                ctx.set_font(&format!("{}px sans-serif", config.font_size));
                ctx.set_fill_color(text_color);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(label, text_x, item_rect.center_y());

                // Draw submenu arrow
                let arrow_x = item_rect.right() - config.item_padding_x;
                let arrow_y = item_rect.center_y();
                let arrow_size = 4.0;

                ctx.set_fill_color(text_color);
                ctx.begin_path();
                ctx.move_to(arrow_x - arrow_size, arrow_y - arrow_size);
                ctx.line_to(arrow_x, arrow_y);
                ctx.line_to(arrow_x - arrow_size, arrow_y + arrow_size);
                ctx.close_path();
                ctx.fill();

                result.item_rects.push((id.clone(), item_rect));
                if is_hovered {
                    result.hovered = Some(id.clone());
                    result.open_submenu = Some(id.clone());
                }

                y += config.item_height;
            }
        }
    }

    result
}

/// Check if point is inside dropdown menu
pub fn dropdown_hit_test(menu_rect: &WidgetRect, x: f64, y: f64) -> bool {
    menu_rect.contains(x, y)
}

/// Grid dropdown configuration
#[derive(Clone, Debug)]
pub struct GridDropdownConfig {
    /// Menu items (only Item variants are rendered)
    pub items: Vec<DropdownItem>,
    /// Number of columns in the grid
    pub columns: u8,
    /// Cell size (square cells)
    pub cell_size: f64,
    /// Icon size within cell
    pub icon_size: f64,
    /// Padding around the grid
    pub padding: f64,
    /// Gap between cells
    pub gap: f64,
    /// Corner radius
    pub radius: f64,
}

impl Default for GridDropdownConfig {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            columns: 2,
            cell_size: 32.0,
            icon_size: 20.0,
            padding: 6.0,
            gap: 2.0,
            radius: 4.0,
        }
    }
}

impl GridDropdownConfig {
    pub fn new(items: Vec<DropdownItem>, columns: u8) -> Self {
        Self {
            items,
            columns,
            ..Default::default()
        }
    }

    /// Count only action items (headers/separators are skipped in grid)
    fn action_item_count(&self) -> usize {
        self.items.iter().filter(|item| {
            matches!(item, DropdownItem::Item { .. })
        }).count()
    }

    /// Calculate grid dimensions
    pub fn calculate_size(&self) -> (f64, f64) {
        let count = self.action_item_count();
        if count == 0 {
            return (self.padding * 2.0, self.padding * 2.0);
        }

        let cols = self.columns as usize;
        let rows = (count + cols - 1) / cols; // ceiling division

        let width = self.padding * 2.0
            + (cols as f64 * self.cell_size)
            + ((cols - 1) as f64 * self.gap);

        let height = self.padding * 2.0
            + (rows as f64 * self.cell_size)
            + ((rows.saturating_sub(1)) as f64 * self.gap);

        (width, height)
    }
}

/// Draw a grid-style dropdown menu (icon-only cells in a grid)
///
/// # Parameters
/// - `ctx` - Render context
/// - `config` - Grid dropdown configuration
/// - `origin` - Top-left position of the menu
/// - `theme` - Dropdown theme
/// - `hovered_id` - Currently hovered item ID
/// - `draw_icon` - Callback to draw icons
///
/// # Returns
/// Dropdown result with item rectangles
pub fn draw_grid_dropdown<F>(
    ctx: &mut dyn RenderContext,
    config: &GridDropdownConfig,
    origin: (f64, f64),
    theme: &DropdownTheme,
    hovered_id: Option<&str>,
    mut draw_icon: F,
) -> DropdownResult
where
    F: FnMut(&mut dyn RenderContext, &IconId, WidgetRect, &str),
{
    let mut result = DropdownResult::default();

    let (menu_width, menu_height) = config.calculate_size();
    let menu_rect = WidgetRect::new(origin.0, origin.1, menu_width, menu_height);
    result.menu_rect = menu_rect;

    // Draw shadow
    ctx.set_fill_color("rgba(0,0,0,0.3)");
    ctx.fill_rounded_rect(
        menu_rect.x + 2.0,
        menu_rect.y + 4.0,
        menu_rect.width,
        menu_rect.height,
        config.radius,
    );

    // Blur background (FrostedGlass/LiquidGlass)
    ctx.draw_blur_background(menu_rect.x, menu_rect.y, menu_rect.width, menu_rect.height);

    // Draw background
    ctx.set_fill_color(&theme.background);
    ctx.fill_rounded_rect(menu_rect.x, menu_rect.y, menu_rect.width, menu_rect.height, config.radius);

    // Draw border
    ctx.set_stroke_color(&theme.border);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(menu_rect.x, menu_rect.y, menu_rect.width, menu_rect.height, config.radius);

    // Collect only action items
    let action_items: Vec<_> = config.items.iter().filter_map(|item| {
        if let DropdownItem::Item { id, icon, label, disabled, .. } = item {
            Some((id, icon, label, *disabled))
        } else {
            None
        }
    }).collect();

    let cols = config.columns as usize;

    // Draw items in grid
    for (idx, (id, icon, _label, disabled)) in action_items.iter().enumerate() {
        let row = idx / cols;
        let col = idx % cols;

        let cell_x = menu_rect.x + config.padding + (col as f64 * (config.cell_size + config.gap));
        let cell_y = menu_rect.y + config.padding + (row as f64 * (config.cell_size + config.gap));

        let cell_rect = WidgetRect::new(cell_x, cell_y, config.cell_size, config.cell_size);

        let is_hovered = hovered_id == Some(id.as_str()) && !disabled;

        // Draw hover background
        if is_hovered {
            ctx.set_fill_color(&theme.item_bg_hover);
            ctx.fill_rounded_rect(cell_rect.x, cell_rect.y, cell_rect.width, cell_rect.height, 4.0);
        }

        // Determine icon color
        let icon_color = if *disabled {
            &theme.item_text_disabled
        } else if is_hovered {
            &theme.item_text_hover
        } else {
            &theme.item_text
        };

        // Draw icon centered in cell
        if let Some(icon_id) = icon {
            let icon_rect = WidgetRect::new(
                cell_rect.x + (cell_rect.width - config.icon_size) / 2.0,
                cell_rect.y + (cell_rect.height - config.icon_size) / 2.0,
                config.icon_size,
                config.icon_size,
            );
            draw_icon(ctx, icon_id, icon_rect, icon_color);
        }

        result.item_rects.push((id.to_string(), cell_rect));
        if is_hovered {
            result.hovered = Some(id.to_string());
        }
    }

    result
}

/// Layout dropdown configuration for window layout selector
#[derive(Clone, Debug)]
pub struct LayoutDropdownConfig {
    /// Layout items grouped by window count (items with icons)
    pub layout_items: Vec<DropdownItem>,
    /// List items shown after separator (sync options, no icons)
    pub list_items: Vec<DropdownItem>,
    /// Cell size for layout icons
    pub cell_size: f64,
    /// Icon size within cell
    pub icon_size: f64,
    /// Padding around content
    pub padding: f64,
    /// Gap between cells
    pub gap: f64,
    /// Row height for list items
    pub item_height: f64,
    /// Separator height
    pub separator_height: f64,
    /// Corner radius
    pub radius: f64,
    /// Row label width (for "1", "2", "3", "4" labels)
    pub row_label_width: f64,
}

impl Default for LayoutDropdownConfig {
    fn default() -> Self {
        Self {
            layout_items: Vec::new(),
            list_items: Vec::new(),
            cell_size: 32.0,
            icon_size: 20.0,
            padding: 8.0,
            gap: 4.0,
            item_height: 28.0,
            separator_height: 9.0,
            radius: 4.0,
            row_label_width: 16.0,
        }
    }
}

impl LayoutDropdownConfig {
    pub fn new(layout_items: Vec<DropdownItem>, list_items: Vec<DropdownItem>) -> Self {
        Self {
            layout_items,
            list_items,
            ..Default::default()
        }
    }

    /// Group layout items by window count (extracted from id pattern)
    /// Returns vec of (window_count, items)
    fn group_by_window_count(&self) -> Vec<(u8, Vec<&DropdownItem>)> {
        let mut groups: Vec<(u8, Vec<&DropdownItem>)> = vec![
            (1, Vec::new()),
            (2, Vec::new()),
            (3, Vec::new()),
            (4, Vec::new()),
        ];

        for item in &self.layout_items {
            if let DropdownItem::Item { id, .. } = item {
                let count = get_layout_window_count(id);
                if count >= 1 && count <= 4 {
                    groups[(count - 1) as usize].1.push(item);
                }
            }
        }

        // Filter out empty groups
        groups.into_iter().filter(|(_, items)| !items.is_empty()).collect()
    }

    /// Calculate total size of the dropdown
    pub fn calculate_size(&self) -> (f64, f64) {
        let groups = self.group_by_window_count();

        // Calculate grid section size
        let mut max_row_width = 0.0f64;
        let num_rows = groups.len();

        for (_, items) in &groups {
            let row_width = self.row_label_width + self.gap
                + (items.len() as f64 * self.cell_size)
                + ((items.len().saturating_sub(1)) as f64 * self.gap);
            max_row_width = max_row_width.max(row_width);
        }

        // Grid height: rows * cell_size + (rows-1) * gap
        let grid_height = if num_rows > 0 {
            (num_rows as f64 * self.cell_size) + ((num_rows - 1) as f64 * self.gap)
        } else {
            0.0
        };

        // List section: separator + only actual Item variants (not separators/headers)
        let list_item_count = self.list_items.iter().filter(|item| {
            matches!(item, DropdownItem::Item { .. })
        }).count();

        let list_height = if list_item_count > 0 {
            self.separator_height + (list_item_count as f64 * self.item_height)
        } else {
            0.0
        };

        let width = self.padding * 2.0 + max_row_width;
        let height = self.padding + grid_height + list_height + self.padding;

        (width, height)
    }
}

/// Get window count from layout item id
fn get_layout_window_count(id: &str) -> u8 {
    match id {
        "layout_single" => 1,
        "layout_split_h" | "layout_split_v" => 2,
        "layout_2left_1right" | "layout_1left_2right" |
        "layout_2top_1bottom" | "layout_1top_2bottom" |
        "layout_3columns" | "layout_3rows" => 3,
        "layout_grid_2x2" | "layout_1big_3small" => 4,
        _ => 0,
    }
}

/// Draw layout selector dropdown with grouped grid rows and list items
pub fn draw_layout_dropdown<F>(
    ctx: &mut dyn RenderContext,
    config: &LayoutDropdownConfig,
    origin: (f64, f64),
    theme: &DropdownTheme,
    hovered_id: Option<&str>,
    mut draw_icon: F,
) -> DropdownResult
where
    F: FnMut(&mut dyn RenderContext, &IconId, WidgetRect, &str),
{
    use uzor::render::TextAlign;

    let mut result = DropdownResult::default();

    let (menu_width, menu_height) = config.calculate_size();
    let menu_rect = WidgetRect::new(origin.0, origin.1, menu_width, menu_height);
    result.menu_rect = menu_rect;

    // Draw shadow
    ctx.set_fill_color("rgba(0,0,0,0.3)");
    ctx.fill_rounded_rect(
        menu_rect.x + 2.0,
        menu_rect.y + 4.0,
        menu_rect.width,
        menu_rect.height,
        config.radius,
    );

    // Blur background (FrostedGlass/LiquidGlass)
    ctx.draw_blur_background(menu_rect.x, menu_rect.y, menu_rect.width, menu_rect.height);

    // Draw background
    ctx.set_fill_color(&theme.background);
    ctx.fill_rounded_rect(menu_rect.x, menu_rect.y, menu_rect.width, menu_rect.height, config.radius);

    // Draw border
    ctx.set_stroke_color(&theme.border);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(menu_rect.x, menu_rect.y, menu_rect.width, menu_rect.height, config.radius);

    let groups = config.group_by_window_count();
    let num_groups = groups.len();
    let mut y = menu_rect.y + config.padding;

    // Draw grid rows
    for (idx, (window_count, items)) in groups.iter().enumerate() {
        let row_x = menu_rect.x + config.padding;

        // Draw row label (1, 2, 3, 4)
        ctx.set_fill_color(&theme.item_text_disabled);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.fill_text(
            &window_count.to_string(),
            row_x + 4.0,
            y + config.cell_size / 2.0 + 4.0,
        );

        // Draw layout icons
        let mut x = row_x + config.row_label_width + config.gap;

        for item in items {
            if let DropdownItem::Item { id, icon, disabled, .. } = item {
                let cell_rect = WidgetRect::new(x, y, config.cell_size, config.cell_size);
                let is_hovered = hovered_id == Some(id.as_str()) && !disabled;

                // Draw hover/selected background
                if is_hovered {
                    ctx.set_fill_color(&theme.item_bg_hover);
                    ctx.fill_rounded_rect(cell_rect.x, cell_rect.y, cell_rect.width, cell_rect.height, 4.0);
                }

                // Draw border
                ctx.set_stroke_color(&theme.border);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rounded_rect(cell_rect.x, cell_rect.y, cell_rect.width, cell_rect.height, 4.0);

                // Draw icon
                let icon_color = if *disabled {
                    &theme.item_text_disabled
                } else if is_hovered {
                    &theme.item_text_hover
                } else {
                    &theme.item_text
                };

                if let Some(icon_id) = icon {
                    let icon_rect = WidgetRect::new(
                        cell_rect.x + (cell_rect.width - config.icon_size) / 2.0,
                        cell_rect.y + (cell_rect.height - config.icon_size) / 2.0,
                        config.icon_size,
                        config.icon_size,
                    );
                    draw_icon(ctx, icon_id, icon_rect, icon_color);
                }

                result.item_rects.push((id.clone(), cell_rect));
                if is_hovered {
                    result.hovered = Some(id.clone());
                }

                x += config.cell_size + config.gap;
            }
        }

        // Add gap only between rows, not after the last one
        y += config.cell_size;
        if idx < num_groups - 1 {
            y += config.gap;
        }
    }

    // Draw separator and list items if present
    if !config.list_items.is_empty() {

        // Draw separator
        let sep_y = y + config.separator_height / 2.0;
        ctx.set_stroke_color(&theme.border);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(menu_rect.x + config.padding, sep_y);
        ctx.line_to(menu_rect.x + menu_rect.width - config.padding, sep_y);
        ctx.stroke();

        y += config.separator_height;

        // Draw list items
        for item in &config.list_items {
            if let DropdownItem::Item { id, label, disabled, .. } = item {
                let item_rect = WidgetRect::new(
                    menu_rect.x + config.padding,
                    y,
                    menu_rect.width - config.padding * 2.0,
                    config.item_height,
                );

                let is_hovered = hovered_id == Some(id.as_str()) && !disabled;

                // Draw hover background
                if is_hovered {
                    ctx.set_fill_color(&theme.item_bg_hover);
                    ctx.fill_rounded_rect(item_rect.x, item_rect.y, item_rect.width, item_rect.height, 2.0);
                }

                // Draw checkbox placeholder (square)
                let checkbox_size = 14.0;
                let checkbox_x = item_rect.x + 8.0;
                let checkbox_y = item_rect.y + (item_rect.height - checkbox_size) / 2.0;

                ctx.set_stroke_color(&theme.border);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rect(checkbox_x, checkbox_y, checkbox_size, checkbox_size);

                // Draw label
                let text_color = if *disabled {
                    &theme.item_text_disabled
                } else if is_hovered {
                    &theme.item_text_hover
                } else {
                    &theme.item_text
                };

                ctx.set_fill_color(text_color);
                ctx.set_font("13px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.fill_text(
                    label,
                    checkbox_x + checkbox_size + 8.0,
                    item_rect.y + item_rect.height / 2.0 + 4.0,
                );

                result.item_rects.push((id.clone(), item_rect));
                if is_hovered {
                    result.hovered = Some(id.clone());
                }

                y += config.item_height;
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dropdown_item() {
        let item = DropdownItem::item("test", "Test Item")
            .with_icon("icon")
            .with_shortcut("Ctrl+T");

        assert_eq!(item.id(), Some("test"));
    }

    #[test]
    fn test_dropdown_config() {
        let config = DropdownConfig::new(vec![
            DropdownItem::header("Section"),
            DropdownItem::item("a", "Item A"),
            DropdownItem::separator(),
            DropdownItem::item("b", "Item B"),
        ]);

        let height = config.calculate_height();
        assert!(height > 0.0);
    }
}

// Alias render_* functions (new naming convention)
pub use draw_dropdown as render_dropdown;
pub use draw_grid_dropdown as render_grid_dropdown;
pub use draw_layout_dropdown as render_layout_dropdown;
