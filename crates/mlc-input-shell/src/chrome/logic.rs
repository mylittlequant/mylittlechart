//! Chrome geometry logic — pure functions, no platform calls, no RenderContext.

use uzor::WidgetId;
use mlc_core::i18n::{TooltipKey, current_language};

use super::types::{
    CHROME_HEIGHT, ChromeContextMenu, ChromeHit, ChromeMenuAction, ChromeState,
};

// ── Layout constants (mirrored from vello chrome.rs) ─────────────────────────

const BORDER_WIDTH: f64 = 4.0;
const BUTTON_WIDTH: f64 = 46.0;
const CLOSE_WINDOW_BUTTON_WIDTH: f64 = 36.0;
const MASCOT_BUTTON_WIDTH: f64 = 0.0;
const MENU_BUTTON_WIDTH: f64 = 36.0;
const NEW_WINDOW_BUTTON_WIDTH: f64 = 36.0;
const NEW_TAB_BUTTON_WIDTH: f64 = 28.0;
const TAB_LEFT_MARGIN: f64 = 4.0;
const TAB_GAP: f64 = 1.0;
const TAB_CLOSE_SIZE: f64 = 16.0;
const TAB_PADDING_H: f64 = 12.0;

const CONTEXT_MENU_WIDTH: f64 = 160.0;
const CONTEXT_MENU_ITEM_HEIGHT: f64 = 28.0;
const CONTEXT_MENU_PADDING: f64 = 4.0;
const CONTEXT_MENU_ITEMS_LEN: usize = 2;

// ── ButtonPositions ───────────────────────────────────────────────────────────

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
        let close_x           = width - BUTTON_WIDTH;
        let maximize_x        = width - BUTTON_WIDTH * 2.0;
        let minimize_x        = width - BUTTON_WIDTH * 3.0;
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
        if x >= bp.maximize_x       { return ChromeHit::MaximizeButton; }
        if x >= bp.minimize_x       { return ChromeHit::MinimizeButton; }
        if x >= bp.close_window_left && x < bp.minimize_x        { return ChromeHit::CloseWindowButton; }
        if x >= bp.mascot_left       && x < bp.close_window_left { return ChromeHit::MascotButton; }
        if x >= bp.menu_left         && x < bp.mascot_left       { return ChromeHit::MenuButton; }
        if x >= bp.new_window_left   && x < bp.menu_left         { return ChromeHit::NewWindowButton; }

        // Tabs
        let mut cursor = TAB_LEFT_MARGIN;
        for (i, tw) in state.tab_widths.iter().enumerate() {
            let tab_right = cursor + tw;
            if x >= cursor && x < tab_right {
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

// ── Tooltip helpers ───────────────────────────────────────────────────────────

fn tooltip_for_hit(hit: ChromeHit, is_maximized: bool) -> Option<&'static str> {
    let lang = current_language();
    match hit {
        ChromeHit::CloseButton       => Some(TooltipKey::CloseApp.get(lang)),
        ChromeHit::CloseWindowButton => Some(TooltipKey::CloseWindow.get(lang)),
        ChromeHit::MinimizeButton    => Some(TooltipKey::Minimize.get(lang)),
        ChromeHit::MaximizeButton    => {
            if is_maximized {
                Some(TooltipKey::Restore.get(lang))
            } else {
                Some(TooltipKey::Maximize.get(lang))
            }
        }
        ChromeHit::NewWindowButton   => Some(TooltipKey::NewWindow.get(lang)),
        ChromeHit::MenuButton        => Some(TooltipKey::Menu.get(lang)),
        ChromeHit::NewTabButton      => Some(TooltipKey::NewTab.get(lang)),
        ChromeHit::TabClose(_)       => Some(TooltipKey::CloseTab.get(lang)),
        _ => None,
    }
}

fn widget_id_for_hit(hit: ChromeHit) -> Option<&'static str> {
    match hit {
        ChromeHit::CloseButton       => Some("chrome_close_app"),
        ChromeHit::CloseWindowButton => Some("chrome_close_window"),
        ChromeHit::MinimizeButton    => Some("chrome_minimize"),
        ChromeHit::MaximizeButton    => Some("chrome_maximize"),
        ChromeHit::NewWindowButton   => Some("chrome_new_window"),
        ChromeHit::MenuButton        => Some("chrome_menu"),
        ChromeHit::NewTabButton      => Some("chrome_new_tab"),
        ChromeHit::TabClose(_)       => Some("chrome_close_tab"),
        _ => None,
    }
}

/// Update chrome tooltip state. Call every frame when the cursor is in the chrome area.
pub fn update_tooltip(state: &mut ChromeState, cursor_x: f64, cursor_y: f64, time_ms: f64) {
    let hit = state.hovered;

    let widget_id = widget_id_for_hit(hit).map(WidgetId::from);
    state.tooltip.update(widget_id.clone(), time_ms);

    if let (Some(wid), Some(text)) = (widget_id, tooltip_for_hit(hit, state.is_maximized)) {
        state.tooltip.request_tooltip(wid, text.to_string(), (cursor_x, cursor_y), time_ms);
    }
}

// ── Context menu ──────────────────────────────────────────────────────────────

/// Hit-test the chrome context menu. Returns the action if a menu item was clicked.
pub fn context_menu_hit_test(menu: &ChromeContextMenu, x: f64, y: f64) -> Option<ChromeMenuAction> {
    if !menu.open { return None; }

    let item_count = CONTEXT_MENU_ITEMS_LEN as f64;
    let menu_h = CONTEXT_MENU_PADDING * 2.0 + item_count * CONTEXT_MENU_ITEM_HEIGHT;

    if x < menu.x || x >= menu.x + CONTEXT_MENU_WIDTH || y < menu.y || y >= menu.y + menu_h {
        return None;
    }

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

    let item_count = CONTEXT_MENU_ITEMS_LEN as f64;
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
    if idx < CONTEXT_MENU_ITEMS_LEN as i32 {
        menu.hovered_index = idx;
    } else {
        menu.hovered_index = -1;
    }
}

// ── Tab strip helpers ─────────────────────────────────────────────────────────

/// Returns the x coordinate of the left edge of the + button.
pub fn new_tab_button_x(state: &ChromeState) -> f64 {
    tab_strip_end_x(state)
}

fn tab_strip_end_x(state: &ChromeState) -> f64 {
    let mut cursor = TAB_LEFT_MARGIN;
    for tw in &state.tab_widths {
        cursor += tw + TAB_GAP;
    }
    cursor
}

/// Pre-compute tab widths from raw pixel measurements.
/// Caller (vello shell) measures text with `dyn RenderContext` and passes widths here.
pub fn apply_tab_widths(state: &mut ChromeState, widths: Vec<f64>) {
    state.tab_widths = widths;
}

/// Returns the expected tab text for width calculation — vello shell uses this
/// to know which strings to measure.
pub fn tab_names(state: &ChromeState) -> impl Iterator<Item = &str> {
    state.tabs.iter().map(|t| t.name.as_str())
}

/// Compute the raw tab width from a measured text width (mirrors vello chrome logic).
pub fn tab_width_from_text(text_w: f64) -> f64 {
    TAB_PADDING_H + text_w + TAB_CLOSE_SIZE + TAB_PADDING_H
}
