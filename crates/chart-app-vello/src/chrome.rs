//! Custom window chrome (title bar) for a borderless window.
//!
//! Renders a 32-pixel-tall strip at `y = 0` containing:
//! - Tab buttons (one per preset) starting at the left margin.
//! - A "+" new-tab button after the last tab.
//! - A draggable caption area in the middle.
//! - A new-window, mascot, and menu button left of the window controls.
//! - Three window control buttons (minimize, maximize, close) on the right edge.
//! - A close-window button between the window controls and the mascot/menu group.
//!
//! Window control icons (minimize, maximize, close) are drawn as filled
//! rectangles / stroked lines for pixel-perfect crispness at any DPI.
//! Tab close icons and the action buttons use SVG icons from the chart icon set.

use uzor::render::{draw_svg_icon, draw_svg_multicolor, RenderContext, TextAlign, TextBaseline};
use uzor::{TooltipState, WidgetId, calculate_tooltip_position};
use uzor::i18n::{TooltipKey, current_language};
use zengeld_chart::ui::icons::icon_svg;

const MINI_MASCOT_SVG: &str = include_str!("../../../assets/mascot/mini_mascot.svg");

// ── Public constants ──────────────────────────────────────────────────────────

/// Height of the chrome strip in logical pixels.
pub const CHROME_HEIGHT: f64 = 32.0;

// ── Chrome Context Menu ─────────────────────────────────────────────────────

/// Items available in the chrome right-click context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromeMenuAction {
    CloseWindow,
    DeleteWindow,
}

/// State for the chrome strip's right-click context menu.
pub struct ChromeContextMenu {
    pub open: bool,
    /// Position where the menu was opened (logical px, relative to window).
    pub x: f64,
    pub y: f64,
    /// Which item is currently hovered (-1 = none).
    pub hovered_index: i32,
}

impl ChromeContextMenu {
    pub fn new() -> Self {
        Self { open: false, x: 0.0, y: 0.0, hovered_index: -1 }
    }

    pub fn open_at(&mut self, x: f64, y: f64) {
        self.open = true;
        self.x = x;
        self.y = y;
        self.hovered_index = -1;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.hovered_index = -1;
    }
}

const CONTEXT_MENU_WIDTH: f64 = 160.0;
const CONTEXT_MENU_ITEM_HEIGHT: f64 = 28.0;
const CONTEXT_MENU_PADDING: f64 = 4.0;
const CONTEXT_MENU_ITEMS: &[(&str, bool)] = &[
    ("Close Window", false),   // (label, is_danger)
    ("Delete Window", true),
];

/// Width of the resize border zone in logical pixels.
const BORDER_WIDTH: f64 = 4.0;

/// Width of each window control button (minimize/maximize/close_app) in logical pixels.
const BUTTON_WIDTH: f64 = 46.0;

/// Height of each button in logical pixels (equals CHROME_HEIGHT).
const BUTTON_HEIGHT: f64 = CHROME_HEIGHT;

// ── Tab layout constants ──────────────────────────────────────────────────────

/// Width of the close-window button in logical pixels.
const CLOSE_WINDOW_BUTTON_WIDTH: f64 = 36.0;

/// Width of the mascot button in logical pixels.
const MASCOT_BUTTON_WIDTH: f64 = 36.0;

/// Width of the menu button (gear icon) in logical pixels.
const MENU_BUTTON_WIDTH: f64 = 36.0;

/// Width of the new-window button in logical pixels.
const NEW_WINDOW_BUTTON_WIDTH: f64 = 36.0;

/// Width of the new-tab (+) button in logical pixels.
const NEW_TAB_BUTTON_WIDTH: f64 = 28.0;

/// Padding from left edge before the first tab.
const TAB_LEFT_MARGIN: f64 = 4.0;

/// Gap between tabs.
const TAB_GAP: f64 = 1.0;

/// Horizontal padding inside each tab (applied on each side).
const TAB_PADDING_H: f64 = 12.0;

/// Tab close button area size (the × icon zone).
const TAB_CLOSE_SIZE: f64 = 16.0;

// ── Tab data ──────────────────────────────────────────────────────────────────

/// A single tab in the chrome strip.
#[derive(Clone)]
pub struct Tab {
    /// Preset id (used to fire LoadPreset event).
    pub id: String,
    /// Display name shown on the tab.
    pub name: String,
    /// Whether this is the currently active tab.
    pub active: bool,
}

// ── ChromeHit ─────────────────────────────────────────────────────────────────

/// The region of the window chrome that was hit by a pointer event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromeHit {
    None,
    Caption,
    MinimizeButton,
    MaximizeButton,
    CloseButton,
    CloseWindowButton,
    MascotButton,
    MenuButton,
    Tab(usize),
    TabClose(usize),
    NewTabButton,
    NewWindowButton,
    ResizeTop,
    ResizeBottom,
    ResizeLeft,
    ResizeRight,
    ResizeTopLeft,
    ResizeTopRight,
    ResizeBottomLeft,
    ResizeBottomRight,
}

// ── ChromeColors ──────────────────────────────────────────────────────────────

/// Theme colours for the chrome strip, synced from the chart theme each frame.
pub struct ChromeColors {
    pub background: String,
    pub icon_normal: String,
    pub icon_hover: String,
    pub button_hover: String,
    pub close_hover: String,
    pub separator: String,
    pub tab_accent: String,
}

impl Default for ChromeColors {
    fn default() -> Self {
        Self {
            background:   "#131722".into(),
            icon_normal:  "#a6adc8".into(),
            icon_hover:   "#cdd6f4".into(),
            button_hover: "#1f2937".into(),
            close_hover:  "#e81123".into(),
            separator:    "#313244".into(),
            tab_accent:   "#3b82f6".into(),
        }
    }
}

// ── ChromeState ───────────────────────────────────────────────────────────────

/// Mutable rendering state for the chrome strip.
pub struct ChromeState {
    pub hovered: ChromeHit,
    pub is_maximized: bool,
    pub title: String,
    pub colors: ChromeColors,
    pub tabs: Vec<Tab>,
    /// Pre-computed tab widths (updated each frame via [`update_tab_widths`]).
    pub tab_widths: Vec<f64>,
    /// Right-click context menu state.
    pub context_menu: ChromeContextMenu,
    /// Tooltip state for chrome button hover tooltips.
    pub tooltip: TooltipState,
}

impl ChromeState {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            hovered:      ChromeHit::None,
            is_maximized: false,
            title:        title.into(),
            colors:       ChromeColors::default(),
            tabs:         Vec::new(),
            tab_widths:   Vec::new(),
            context_menu: ChromeContextMenu::new(),
            tooltip:      TooltipState::new(),
        }
    }
}

// ── update_tab_widths ─────────────────────────────────────────────────────────

/// Pre-compute tab widths using the render context for accurate text measurement.
pub fn update_tab_widths(ctx: &mut dyn RenderContext, state: &mut ChromeState) {
    ctx.set_font("12px sans-serif");
    state.tab_widths.clear();
    for tab in &state.tabs {
        let text_w = ctx.measure_text(&tab.name);
        let w = TAB_PADDING_H + text_w + TAB_CLOSE_SIZE + TAB_PADDING_H;
        state.tab_widths.push(w);
    }
}

// ── Button position helpers ───────────────────────────────────────────────────

/// Compute all right-side button left-edge positions given the window width.
///
/// Layout right-to-left:
/// `close_app | maximize | minimize | divider | close_window | divider | mascot | menu | new_window | ...tabs`
struct ButtonPositions {
    close_x: f64,
    maximize_x: f64,
    minimize_x: f64,
    close_window_left: f64,
    mascot_left: f64,
    menu_left: f64,
    new_window_left: f64,
}

impl ButtonPositions {
    fn compute(width: f64) -> Self {
        let close_x         = width - BUTTON_WIDTH;
        let maximize_x      = width - BUTTON_WIDTH * 2.0;
        let minimize_x      = width - BUTTON_WIDTH * 3.0;
        let close_window_left = minimize_x - CLOSE_WINDOW_BUTTON_WIDTH;
        let mascot_left       = close_window_left - MASCOT_BUTTON_WIDTH;
        let menu_left         = mascot_left - MENU_BUTTON_WIDTH;
        let new_window_left   = menu_left - NEW_WINDOW_BUTTON_WIDTH;
        Self { close_x, maximize_x, minimize_x, close_window_left, mascot_left, menu_left, new_window_left }
    }
}

// ── hit_test ──────────────────────────────────────────────────────────────────

pub fn hit_test(x: f64, y: f64, width: f64, height: f64, state: &ChromeState) -> ChromeHit {
    let in_top    = y < BORDER_WIDTH;
    let in_bottom = y >= height - BORDER_WIDTH;
    let in_left   = x < BORDER_WIDTH;
    let in_right  = x >= width - BORDER_WIDTH;

    if in_top && in_left  { return ChromeHit::ResizeTopLeft; }
    if in_top && in_right { return ChromeHit::ResizeTopRight; }
    if in_bottom && in_left  { return ChromeHit::ResizeBottomLeft; }
    if in_bottom && in_right { return ChromeHit::ResizeBottomRight; }
    if in_top    { return ChromeHit::ResizeTop; }
    if in_bottom { return ChromeHit::ResizeBottom; }
    if in_left   { return ChromeHit::ResizeLeft; }
    if in_right  { return ChromeHit::ResizeRight; }

    if y < CHROME_HEIGHT {
        let bp = ButtonPositions::compute(width);

        if x >= bp.close_x          { return ChromeHit::CloseButton; }
        if x >= bp.maximize_x        { return ChromeHit::MaximizeButton; }
        if x >= bp.minimize_x        { return ChromeHit::MinimizeButton; }
        if x >= bp.close_window_left && x < bp.minimize_x        { return ChromeHit::CloseWindowButton; }
        if x >= bp.mascot_left       && x < bp.close_window_left { return ChromeHit::MascotButton; }
        if x >= bp.menu_left         && x < bp.mascot_left       { return ChromeHit::MenuButton; }
        if x >= bp.new_window_left   && x < bp.menu_left         { return ChromeHit::NewWindowButton; }

        // Tabs
        let mut cursor = TAB_LEFT_MARGIN;
        for (i, tw) in state.tab_widths.iter().enumerate() {
            let tab_right = cursor + tw;
            if x >= cursor && x < tab_right {
                // Close × is flush right (last TAB_CLOSE_SIZE pixels)
                if x >= tab_right - TAB_CLOSE_SIZE {
                    return ChromeHit::TabClose(i);
                }
                return ChromeHit::Tab(i);
            }
            cursor = tab_right + TAB_GAP;
        }

        // "+" button
        let new_tab_right = cursor + NEW_TAB_BUTTON_WIDTH;
        if x >= cursor && x < new_tab_right {
            return ChromeHit::NewTabButton;
        }

        return ChromeHit::Caption;
    }

    ChromeHit::None
}

// ── Tooltip ───────────────────────────────────────────────────────────────────

/// Map a `ChromeHit` to a static tooltip string using the current i18n language.
///
/// Returns `None` for hits that have no tooltip (caption, resize borders, tabs, etc.).
fn tooltip_for_hit(hit: ChromeHit, is_maximized: bool) -> Option<&'static str> {
    let lang = current_language();
    match hit {
        ChromeHit::CloseButton      => Some(TooltipKey::CloseApp.get(lang)),
        ChromeHit::CloseWindowButton => Some(TooltipKey::CloseWindow.get(lang)),
        ChromeHit::MinimizeButton   => Some(TooltipKey::Minimize.get(lang)),
        ChromeHit::MaximizeButton   => {
            if is_maximized {
                Some(TooltipKey::Restore.get(lang))
            } else {
                Some(TooltipKey::Maximize.get(lang))
            }
        }
        ChromeHit::NewWindowButton  => Some(TooltipKey::NewWindow.get(lang)),
        ChromeHit::MenuButton       => Some(TooltipKey::Menu.get(lang)),
        _ => None,
    }
}

/// Return a stable widget-id string for tooltip-bearing chrome hits.
fn widget_id_for_hit(hit: ChromeHit) -> Option<&'static str> {
    match hit {
        ChromeHit::CloseButton       => Some("chrome_close_app"),
        ChromeHit::CloseWindowButton => Some("chrome_close_window"),
        ChromeHit::MinimizeButton    => Some("chrome_minimize"),
        ChromeHit::MaximizeButton    => Some("chrome_maximize"),
        ChromeHit::NewWindowButton   => Some("chrome_new_window"),
        ChromeHit::MenuButton        => Some("chrome_menu"),
        _ => None,
    }
}

/// Update chrome tooltip state. Call every frame when the cursor is in the chrome area.
///
/// `time_ms` is the elapsed time in milliseconds since some fixed reference point (e.g.
/// app start). `cursor_x` / `cursor_y` are logical pixel coordinates relative to the window.
pub fn update_tooltip(state: &mut ChromeState, cursor_x: f64, cursor_y: f64, time_ms: f64) {
    let hit = state.hovered;

    let widget_id = widget_id_for_hit(hit).map(WidgetId::new);
    state.tooltip.update(widget_id.clone(), time_ms);

    if let (Some(wid), Some(text)) = (widget_id, tooltip_for_hit(hit, state.is_maximized)) {
        state.tooltip.request_tooltip(wid, text.to_string(), (cursor_x, cursor_y), time_ms);
    }
}

/// Render the chrome tooltip if one is currently visible.
///
/// Must be called after [`render`]. The tooltip is drawn at the top of the compositing
/// stack, above chrome buttons and context menus. `screen_width` and `screen_height`
/// are the full window dimensions in logical pixels.
pub fn render_tooltip(
    ctx: &mut dyn RenderContext,
    state: &ChromeState,
    screen_width: f64,
    screen_height: f64,
) {
    render_tooltip_state(ctx, &state.tooltip, screen_width, screen_height);
}

/// Render a tooltip from any `TooltipState` (chrome, toolbar, etc.).
pub fn render_tooltip_state(
    ctx: &mut dyn RenderContext,
    tooltip: &TooltipState,
    screen_width: f64,
    screen_height: f64,
) {
    let active = match tooltip.get_active() {
        Some(t) => t,
        None => return,
    };

    let opacity = tooltip.get_opacity();
    if opacity <= 0.0 {
        return;
    }

    let text = &active.text;
    ctx.set_font("12px sans-serif");
    let text_width = ctx.measure_text(text);
    let pad = 6.0;
    let tw = text_width + pad * 2.0;
    let th = 12.0 + pad * 2.0; // font_size + 2*padding

    let (tx, ty) = calculate_tooltip_position(
        active.position,
        (tw, th),
        (screen_width, screen_height),
        (0.0, 20.0), // 0 horizontal offset, 20px below cursor
    );

    ctx.save();
    ctx.set_global_alpha(opacity);

    // Shadow
    ctx.set_fill_color("#00000060");
    ctx.fill_rounded_rect(tx + 1.0, ty + 1.0, tw, th, 4.0);

    // Background
    ctx.set_fill_color("#323232");
    ctx.fill_rounded_rect(tx, ty, tw, th, 4.0);

    // Text
    ctx.set_fill_color("#ffffff");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(text, tx + pad, ty + th / 2.0);

    ctx.restore();
}

// ── render ────────────────────────────────────────────────────────────────────

/// Render the chrome strip.
///
/// When `skeleton_mode` is `true` (profile manager or welcome wizard is open),
/// only the window-control buttons (close_window, minimize, maximize, close_app)
/// and the background are rendered — tabs, +, new_window, mascot and menu are hidden.
pub fn render(ctx: &mut dyn RenderContext, state: &ChromeState, width: f64, skeleton_mode: bool) {
    let c = &state.colors;
    let bp = ButtonPositions::compute(width);

    // ── Background ──────────────────────────────────────────────────────────
    ctx.set_fill_color(&c.background);
    ctx.fill_rect(0.0, 0.0, width, CHROME_HEIGHT);

    // ── Hover backgrounds ───────────────────────────────────────────────────
    match state.hovered {
        ChromeHit::MinimizeButton => {
            ctx.set_fill_color(&c.button_hover);
            ctx.fill_rect(bp.minimize_x, 0.0, BUTTON_WIDTH, BUTTON_HEIGHT);
        }
        ChromeHit::MaximizeButton => {
            ctx.set_fill_color(&c.button_hover);
            ctx.fill_rect(bp.maximize_x, 0.0, BUTTON_WIDTH, BUTTON_HEIGHT);
        }
        ChromeHit::CloseButton => {
            ctx.set_fill_color(&c.close_hover);
            ctx.fill_rect(bp.close_x, 0.0, BUTTON_WIDTH, BUTTON_HEIGHT);
        }
        ChromeHit::CloseWindowButton => {
            ctx.set_fill_color(&c.button_hover);
            ctx.fill_rect(bp.close_window_left, 0.0, CLOSE_WINDOW_BUTTON_WIDTH, BUTTON_HEIGHT);
        }
        ChromeHit::MascotButton if !skeleton_mode => {
            ctx.set_fill_color(&c.button_hover);
            ctx.fill_rect(bp.mascot_left, 0.0, MASCOT_BUTTON_WIDTH, BUTTON_HEIGHT);
        }
        ChromeHit::MenuButton if !skeleton_mode => {
            ctx.set_fill_color(&c.button_hover);
            ctx.fill_rect(bp.menu_left, 0.0, MENU_BUTTON_WIDTH, BUTTON_HEIGHT);
        }
        ChromeHit::NewWindowButton if !skeleton_mode => {
            ctx.set_fill_color(&c.button_hover);
            ctx.fill_rect(bp.new_window_left, 0.0, NEW_WINDOW_BUTTON_WIDTH, BUTTON_HEIGHT);
        }
        ChromeHit::NewTabButton if !skeleton_mode => {
            let new_tab_x = tab_strip_end_x(state);
            ctx.set_fill_color(&c.button_hover);
            ctx.fill_rect(new_tab_x, 0.0, NEW_TAB_BUTTON_WIDTH, BUTTON_HEIGHT);
        }
        _ => {}
    }

    // ── Tabs (hidden in skeleton mode) ──────────────────────────────────────
    if !skeleton_mode {
        let mut cursor = TAB_LEFT_MARGIN;
        for (i, tab) in state.tabs.iter().enumerate() {
            let tw        = state.tab_widths.get(i).copied().unwrap_or(80.0);
            let tab_left  = cursor;
            let tab_right = cursor + tw;

            // Bottom accent line (2px)
            if tab.active {
                ctx.set_fill_color(&c.tab_accent);
                ctx.fill_rect(tab_left, CHROME_HEIGHT - 3.0, tw, 2.0);
            } else if state.hovered == ChromeHit::Tab(i)
                   || state.hovered == ChromeHit::TabClose(i)
            {
                ctx.set_fill_color(&c.icon_hover);
                ctx.fill_rect(tab_left, CHROME_HEIGHT - 3.0, tw, 2.0);
            }

            // Tab name text
            {
                let text_x = tab_left + TAB_PADDING_H;
                let text_color = if tab.active { &c.icon_hover } else { &c.icon_normal };
                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(text_color);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(&tab.name, text_x, CHROME_HEIGHT / 2.0);
            }

            // Tab close × button — flush right, icon turns red on hover
            {
                let close_rect_left = tab_right - TAB_CLOSE_SIZE;
                let icon_size = 14.0;
                let icon_color = if state.hovered == ChromeHit::TabClose(i) {
                    &c.close_hover
                } else {
                    &c.icon_normal
                };
                let icon_x = close_rect_left + (TAB_CLOSE_SIZE - icon_size) / 2.0;
                let icon_y = (CHROME_HEIGHT - icon_size) / 2.0;
                if let Some(svg) = icon_svg("close") {
                    draw_svg_icon(ctx, svg, icon_x, icon_y, icon_size, icon_size, icon_color);
                }
            }

            cursor = tab_right + TAB_GAP;
        }

        // "+" new tab button with divider on the left
        let new_tab_x = cursor;

        let div_h = CHROME_HEIGHT * 0.6;
        let div_y = (CHROME_HEIGHT - div_h) / 2.0;
        ctx.set_fill_color(&c.separator);
        ctx.fill_rect(new_tab_x, div_y, 1.0, div_h);

        let plus_color = if state.hovered == ChromeHit::NewTabButton {
            &c.icon_hover
        } else {
            &c.icon_normal
        };
        let plus_cx = (new_tab_x + NEW_TAB_BUTTON_WIDTH / 2.0).floor() + 0.5;
        let plus_cy = (CHROME_HEIGHT / 2.0).floor() + 0.5;
        let arm = 5.0;
        ctx.set_stroke_color(plus_color);
        ctx.set_stroke_width(1.5);
        ctx.begin_path();
        ctx.move_to(plus_cx - arm, plus_cy);
        ctx.line_to(plus_cx + arm, plus_cy);
        ctx.stroke();
        ctx.begin_path();
        ctx.move_to(plus_cx, plus_cy - arm);
        ctx.line_to(plus_cx, plus_cy + arm);
        ctx.stroke();

        // ── New-window icon (SVG, 18×18) ────────────────────────────────────
        {
            let icon_color = if state.hovered == ChromeHit::NewWindowButton {
                &c.icon_hover
            } else {
                &c.icon_normal
            };
            let icon_x = bp.new_window_left + (NEW_WINDOW_BUTTON_WIDTH - 18.0) / 2.0;
            let icon_y = (CHROME_HEIGHT - 18.0) / 2.0;
            if let Some(svg) = icon_svg("new_window") {
                draw_svg_icon(ctx, svg, icon_x, icon_y, 18.0, 18.0, icon_color);
            }
        }

        // ── Menu icon (gear, SVG 18×18) ─────────────────────────────────────
        {
            let icon_color = if state.hovered == ChromeHit::MenuButton {
                &c.icon_hover
            } else {
                &c.icon_normal
            };
            let icon_x = bp.menu_left + (MENU_BUTTON_WIDTH - 18.0) / 2.0;
            let icon_y = (CHROME_HEIGHT - 18.0) / 2.0;
            if let Some(svg) = icon_svg("settings") {
                draw_svg_icon(ctx, svg, icon_x, icon_y, 18.0, 18.0, icon_color);
            }
        }

        // ── Mascot icon (SVG, 24×24, full color) ─────────────────────────────
        {
            let icon_x = bp.mascot_left + (MASCOT_BUTTON_WIDTH - 24.0) / 2.0;
            let icon_y = (CHROME_HEIGHT - 24.0) / 2.0;
            draw_svg_multicolor(ctx, MINI_MASCOT_SVG, icon_x, icon_y, 24.0, 24.0);
        }
    }

    // ── Divider between mascot and close_window ─────────────────────────────
    // Rendered even in skeleton mode so the control group boundary is clear.
    {
        let div_h = CHROME_HEIGHT * 0.6;
        let div_y = (CHROME_HEIGHT - div_h) / 2.0;
        ctx.set_fill_color(&c.separator);
        ctx.fill_rect(bp.close_window_left, div_y, 1.0, div_h);
    }

    // ── Close-window icon × (smaller than close_app: arm 3.5px, stroke 1.0) ─
    {
        let icon_color = if state.hovered == ChromeHit::CloseWindowButton {
            &c.icon_hover
        } else {
            &c.icon_normal
        };
        let cx = bp.close_window_left + CLOSE_WINDOW_BUTTON_WIDTH / 2.0;
        let cy = CHROME_HEIGHT / 2.0;
        let s = 3.5;
        ctx.set_stroke_color(icon_color);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(cx - s, cy - s);
        ctx.line_to(cx + s, cy + s);
        ctx.stroke();
        ctx.begin_path();
        ctx.move_to(cx - s, cy + s);
        ctx.line_to(cx + s, cy - s);
        ctx.stroke();
    }

    // ── Divider between close_window and minimize ───────────────────────────
    {
        let div_h = CHROME_HEIGHT * 0.6;
        let div_y = (CHROME_HEIGHT - div_h) / 2.0;
        ctx.set_fill_color(&c.separator);
        ctx.fill_rect(bp.minimize_x, div_y, 1.0, div_h);
    }

    // ── Minimize icon ─ (filled rect, pixel-perfect) ────────────────────────
    {
        let icon_color = if state.hovered == ChromeHit::MinimizeButton {
            &c.icon_hover
        } else {
            &c.icon_normal
        };
        let cx = bp.minimize_x + BUTTON_WIDTH / 2.0;
        let cy = CHROME_HEIGHT / 2.0;
        ctx.set_fill_color(icon_color);
        ctx.fill_rect(cx - 5.0, cy, 10.0, 1.0);
    }

    // ── Maximize icon □ / restore (filled rect outlines) ────────────────────
    {
        let icon_color = if state.hovered == ChromeHit::MaximizeButton {
            &c.icon_hover
        } else {
            &c.icon_normal
        };
        let cx = bp.maximize_x + BUTTON_WIDTH / 2.0;
        let cy = CHROME_HEIGHT / 2.0;
        let half = 5.0;

        if state.is_maximized {
            // Back rect (offset 2px)
            draw_rect_outline(ctx, cx - half + 2.0, cy - half - 2.0, half * 2.0, half * 2.0, 1.0, icon_color);
            // Front rect — fill bg to occlude back corner, then outline
            ctx.set_fill_color(&c.background);
            ctx.fill_rect(cx - half, cy - half, half * 2.0, half * 2.0);
            draw_rect_outline(ctx, cx - half, cy - half, half * 2.0, half * 2.0, 1.0, icon_color);
        } else {
            draw_rect_outline(ctx, cx - half, cy - half, half * 2.0, half * 2.0, 1.0, icon_color);
        }
    }

    // ── Close icon × (stroked diagonals, 1.5px) ────────────────────────────
    {
        let icon_color = if state.hovered == ChromeHit::CloseButton {
            &c.icon_hover
        } else {
            &c.icon_normal
        };
        let cx = bp.close_x + BUTTON_WIDTH / 2.0;
        let cy = CHROME_HEIGHT / 2.0;
        let s = 5.0;
        ctx.set_stroke_color(icon_color);
        ctx.set_stroke_width(1.5);
        ctx.begin_path();
        ctx.move_to(cx - s, cy - s);
        ctx.line_to(cx + s, cy + s);
        ctx.stroke();
        ctx.begin_path();
        ctx.move_to(cx - s, cy + s);
        ctx.line_to(cx + s, cy - s);
        ctx.stroke();
    }

    // ── Bottom separator ────────────────────────────────────────────────────
    ctx.set_fill_color(&c.separator);
    ctx.fill_rect(0.0, CHROME_HEIGHT - 1.0, width, 1.0);
}

// ── Context menu render / hit-test ────────────────────────────────────────────

/// Render the chrome context menu if open.
pub fn render_context_menu(ctx: &mut dyn RenderContext, menu: &ChromeContextMenu, colors: &ChromeColors) {
    if !menu.open { return; }

    let item_count = CONTEXT_MENU_ITEMS.len() as f64;
    let menu_h = CONTEXT_MENU_PADDING * 2.0 + item_count * CONTEXT_MENU_ITEM_HEIGHT;

    // Shadow
    ctx.set_fill_color("rgba(0,0,0,0.3)");
    ctx.fill_rect(menu.x + 2.0, menu.y + 2.0, CONTEXT_MENU_WIDTH, menu_h);

    // Background
    ctx.set_fill_color(&colors.background);
    ctx.fill_rect(menu.x, menu.y, CONTEXT_MENU_WIDTH, menu_h);

    // Border
    draw_rect_outline(ctx, menu.x, menu.y, CONTEXT_MENU_WIDTH, menu_h, 1.0, &colors.separator);

    // Items
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);

    for (i, (label, is_danger)) in CONTEXT_MENU_ITEMS.iter().enumerate() {
        let item_y = menu.y + CONTEXT_MENU_PADDING + i as f64 * CONTEXT_MENU_ITEM_HEIGHT;

        // Hover background
        if menu.hovered_index == i as i32 {
            let hover_color = if *is_danger { &colors.close_hover } else { &colors.button_hover };
            ctx.set_fill_color(hover_color);
            ctx.fill_rect(menu.x + 1.0, item_y, CONTEXT_MENU_WIDTH - 2.0, CONTEXT_MENU_ITEM_HEIGHT);
        }

        // Text
        let text_color = if *is_danger && menu.hovered_index == i as i32 {
            &colors.icon_hover  // white text on red bg
        } else if *is_danger {
            &colors.close_hover  // red text
        } else {
            &colors.icon_normal
        };
        ctx.set_fill_color(text_color);
        ctx.fill_text(label, menu.x + 12.0, item_y + CONTEXT_MENU_ITEM_HEIGHT / 2.0);
    }
}

/// Hit-test the chrome context menu. Returns the action if a menu item was clicked,
/// or `None` if outside the menu (caller should close it).
pub fn context_menu_hit_test(menu: &ChromeContextMenu, x: f64, y: f64) -> Option<ChromeMenuAction> {
    if !menu.open { return None; }

    let item_count = CONTEXT_MENU_ITEMS.len() as f64;
    let menu_h = CONTEXT_MENU_PADDING * 2.0 + item_count * CONTEXT_MENU_ITEM_HEIGHT;

    // Check if inside menu bounds
    if x < menu.x || x >= menu.x + CONTEXT_MENU_WIDTH || y < menu.y || y >= menu.y + menu_h {
        return None;
    }

    // Which item?
    let rel_y = y - menu.y - CONTEXT_MENU_PADDING;
    if rel_y < 0.0 { return None; }
    let idx = (rel_y / CONTEXT_MENU_ITEM_HEIGHT) as usize;

    match idx {
        0 => Some(ChromeMenuAction::CloseWindow),
        1 => Some(ChromeMenuAction::DeleteWindow),
        _ => None,
    }
}

/// Update the hovered index based on mouse position.
pub fn context_menu_hover(menu: &mut ChromeContextMenu, x: f64, y: f64) {
    if !menu.open {
        menu.hovered_index = -1;
        return;
    }

    let item_count = CONTEXT_MENU_ITEMS.len() as f64;
    let menu_h = CONTEXT_MENU_PADDING * 2.0 + item_count * CONTEXT_MENU_ITEM_HEIGHT;

    if x < menu.x || x >= menu.x + CONTEXT_MENU_WIDTH || y < menu.y || y >= menu.y + menu_h {
        menu.hovered_index = -1;
        return;
    }

    let rel_y = y - menu.y - CONTEXT_MENU_PADDING;
    if rel_y < 0.0 {
        menu.hovered_index = -1;
        return;
    }
    let idx = (rel_y / CONTEXT_MENU_ITEM_HEIGHT) as i32;
    if idx < CONTEXT_MENU_ITEMS.len() as i32 {
        menu.hovered_index = idx;
    } else {
        menu.hovered_index = -1;
    }
}

// ── Public helpers ────────────────────────────────────────────────────────────

/// Returns the x coordinate of the left edge of the + button.
pub fn new_tab_button_x(state: &ChromeState) -> f64 {
    tab_strip_end_x(state)
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn tab_strip_end_x(state: &ChromeState) -> f64 {
    let mut cursor = TAB_LEFT_MARGIN;
    for tw in &state.tab_widths {
        cursor += tw + TAB_GAP;
    }
    cursor
}

/// Draw a rectangle outline using four filled rects (pixel-perfect edges).
fn draw_rect_outline(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, h: f64, t: f64, color: &str) {
    ctx.set_fill_color(color);
    ctx.fill_rect(x, y, w, t);             // top
    ctx.fill_rect(x, y + h - t, w, t);     // bottom
    ctx.fill_rect(x, y + t, t, h - t * 2.0);   // left
    ctx.fill_rect(x + w - t, y + t, t, h - t * 2.0); // right
}
