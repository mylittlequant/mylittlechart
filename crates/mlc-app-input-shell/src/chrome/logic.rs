//! Chrome geometry logic — pure functions, no platform calls, no RenderContext.

use uzor::WidgetId;
use mlc_core::i18n::{TooltipKey, current_language};

use super::types::{
    CHROME_HEIGHT, ChromeContextMenu, ChromeFlavor, ChromeHit, ChromeMenuAction, ChromeState,
};

// ── Layout constants (mirrored from vello chrome.rs) ─────────────────────────

const BORDER_WIDTH: f64 = 4.0;
const BUTTON_WIDTH: f64 = 46.0;
const CLOSE_WINDOW_BUTTON_WIDTH: f64 = 36.0;
const MASCOT_BUTTON_WIDTH: f64 = 0.0;
const MENU_BUTTON_WIDTH: f64 = 36.0;
const NEW_WINDOW_BUTTON_WIDTH: f64 = 36.0;
const NEW_TAB_BUTTON_WIDTH: f64 = 28.0;
/// Gold "Sign in" text button (Web flavor, cloud builds).
#[cfg(feature = "cloud")]
const LOGIN_BUTTON_WIDTH: f64 = 68.0;
/// Icon-sized cloud buttons (Web flavor, cloud builds).
#[cfg(feature = "cloud")]
const CLOUD_ICON_BUTTON_WIDTH: f64 = 30.0;
const TAB_LEFT_MARGIN: f64 = 4.0;
const TAB_GAP: f64 = 1.0;
const TAB_CLOSE_SIZE: f64 = 16.0;
const TAB_PADDING_H: f64 = 12.0;
/// Width of the "⋯" overflow button shown left of "+" when tabs overflow.
const TAB_OVERFLOW_WIDTH: f64 = 18.0;

// Tab-list dropdown (opened from the "⋯" overflow button).
const TAB_LIST_WIDTH: f64 = 200.0;
const TAB_LIST_ITEM_HEIGHT: f64 = 26.0;
const TAB_LIST_PADDING: f64 = 4.0;

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
    /// Web flavor: left edge of the "Sign in" button (cloud builds).
    #[cfg(feature = "cloud")]
    login_left: f64,
    /// Web flavor: Download button (cloud builds).
    #[cfg(feature = "cloud")]
    download_left: f64,
    /// Web flavor: Links button (cloud builds).
    #[cfg(feature = "cloud")]
    links_left: f64,
    /// Web flavor: News button (cloud builds).
    #[cfg(feature = "cloud")]
    news_left: f64,
}

impl ButtonPositions {
    fn compute(width: f64, flavor: ChromeFlavor) -> Self {
        let close_x           = width - BUTTON_WIDTH;
        let maximize_x        = width - BUTTON_WIDTH * 2.0;
        let minimize_x        = width - BUTTON_WIDTH * 3.0;
        let close_window_left = minimize_x - CLOSE_WINDOW_BUTTON_WIDTH;
        let mascot_left       = close_window_left - MASCOT_BUTTON_WIDTH;
        let new_window_left   = match flavor {
            ChromeFlavor::Native => mascot_left - MENU_BUTTON_WIDTH - NEW_WINDOW_BUTTON_WIDTH,
            ChromeFlavor::Web => 0.0,
        };
        let menu_left = match flavor {
            ChromeFlavor::Native => mascot_left - MENU_BUTTON_WIDTH,
            // Web: the gear is the rightmost button (mirrors render.rs).
            ChromeFlavor::Web => width - MENU_BUTTON_WIDTH,
        };
        // Cloud builds: right-to-left — gear, "Sign in", Download, Links, News.
        #[cfg(feature = "cloud")]
        let (login_left, download_left, links_left, news_left) = match flavor {
            ChromeFlavor::Web => {
                let login = menu_left - LOGIN_BUTTON_WIDTH;
                let dl = login - CLOUD_ICON_BUTTON_WIDTH;
                let li = dl - CLOUD_ICON_BUTTON_WIDTH;
                let ne = li - CLOUD_ICON_BUTTON_WIDTH;
                (login, dl, li, ne)
            }
            ChromeFlavor::Native => (0.0, 0.0, 0.0, 0.0),
        };
        Self {
            close_x, maximize_x, minimize_x, close_window_left, mascot_left,
            menu_left, new_window_left,
            #[cfg(feature = "cloud")]
            login_left,
            #[cfg(feature = "cloud")]
            download_left,
            #[cfg(feature = "cloud")]
            links_left,
            #[cfg(feature = "cloud")]
            news_left,
        }
    }
}

// ── Tab strip layout ──────────────────────────────────────────────────────────

/// Smallest width a tab compresses to when the strip runs out of room.
pub const MIN_TAB_WIDTH: f64 = 48.0;

/// Right edge of the tab strip: the left edge of the right-side control
/// group (window controls on Native, cloud icons + gear on Web). The tab
/// strip is everything between TAB_LEFT_MARGIN and this line — the buttons
/// are static, the tabs are dynamic and shrink to fit whatever is left.
pub fn tabs_right_limit(width: f64, flavor: ChromeFlavor) -> f64 {
    let bp = ButtonPositions::compute(width, flavor);
    match flavor {
        ChromeFlavor::Native => bp.new_window_left,
        ChromeFlavor::Web => {
            #[cfg(feature = "cloud")]
            { bp.news_left }
            #[cfg(not(feature = "cloud"))]
            { bp.menu_left }
        }
    }
}

/// Compress `state.tab_widths` so the whole strip fits left of the
/// right-side controls: the "+" button must always stay visible. Past the
/// floor the strip stays wider than the window and scrolls instead (see
/// `scroll_tab_strip`) — tabs never shrink below the floor and never slide
/// under the buttons.
///
/// When everything fits, each tab keeps its natural (text-measured) width.
/// On overflow they equalize — equal share of `available`, floored at
/// `MIN_TAB_WIDTH` — so a long rename cannot leave its neighbors permanently
/// squeezed. While a tab is being renamed live it keeps its natural width
/// (so the typed name stays fully visible) and the rest share what is left.
pub fn fit_tab_widths(state: &mut ChromeState, width: f64) {
    let n = state.tab_widths.len();
    if n == 0 {
        return;
    }
    let limit = tabs_right_limit(width, state.flavor);
    let reserved = TAB_LEFT_MARGIN
        + TAB_GAP * (n as f64 - 1.0).max(0.0)
        + 1.0                    // separator before "+"
        + NEW_TAB_BUTTON_WIDTH
        + 4.0;                   // breathing room before the controls
    let available = (limit - reserved).max(MIN_TAB_WIDTH);
    let natural: f64 = state.tab_widths.iter().sum();
    if natural <= available {
        return;
    }
    if let Some(ri) = state.renaming_tab {
        if ri < n {
            let live = state.tab_widths[ri].max(MIN_TAB_WIDTH);
            let rest_n = n - 1;
            if rest_n == 0 {
                state.tab_widths[ri] = live;
                return;
            }
            let rest_avail = (available - live).max(0.0);
            let each = (rest_avail / rest_n as f64).max(MIN_TAB_WIDTH);
            for (i, w) in state.tab_widths.iter_mut().enumerate() {
                if i == ri {
                    *w = live;
                } else {
                    *w = each;
                }
            }
            return;
        }
    }
    let each = (available / n as f64).max(MIN_TAB_WIDTH);
    for w in state.tab_widths.iter_mut() {
        *w = each;
    }
}

/// How much of the strip is hidden to the right (0 when everything fits).
pub fn max_tab_scroll(state: &ChromeState, width: f64) -> f64 {
    let n = state.tab_widths.len();
    if n == 0 {
        return 0.0;
    }
    let limit = tabs_right_limit(width, state.flavor);
    let strip_w = limit - TAB_LEFT_MARGIN - 1.0 - NEW_TAB_BUTTON_WIDTH - 4.0;
    let natural: f64 = state.tab_widths.iter().sum::<f64>()
        + TAB_GAP * (n as f64 - 1.0).max(0.0);
    (natural - strip_w).max(0.0)
}

/// Scroll the tab strip horizontally by `delta` px (wheel over the chrome).
/// Positive delta reveals tabs hidden to the right. Clamped both ends.
pub fn scroll_tab_strip(state: &mut ChromeState, width: f64, delta: f64) {
    let max = max_tab_scroll(state, width);
    state.tab_scroll = (state.tab_scroll + delta).clamp(0.0, max);
}

// ── hit_test ──────────────────────────────────────────────────────────────────

pub fn hit_test(x: f64, y: f64, width: f64, height: f64, state: &ChromeState) -> ChromeHit {
    // Resize edges only make sense in native (resizable) windows.
    if state.flavor == ChromeFlavor::Native {
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
    }

    if y < CHROME_HEIGHT {
        let bp = ButtonPositions::compute(width, state.flavor);

        match state.flavor {
            ChromeFlavor::Native => {
                if x >= bp.close_x          { return ChromeHit::CloseButton; }
                if x >= bp.maximize_x       { return ChromeHit::MaximizeButton; }
                if x >= bp.minimize_x       { return ChromeHit::MinimizeButton; }
                if x >= bp.close_window_left && x < bp.minimize_x        { return ChromeHit::CloseWindowButton; }
                if x >= bp.mascot_left       && x < bp.close_window_left { return ChromeHit::MascotButton; }
                if x >= bp.menu_left         && x < bp.mascot_left       { return ChromeHit::MenuButton; }
                if x >= bp.new_window_left   && x < bp.menu_left         { return ChromeHit::NewWindowButton; }
            }
            ChromeFlavor::Web => {
                // Web: gear is rightmost; cloud builds add the cloud buttons.
                if x >= bp.menu_left { return ChromeHit::MenuButton; }
                #[cfg(feature = "cloud")]
                {
                    if x >= bp.login_left && x < bp.menu_left { return ChromeHit::LoginButton; }
                    if x >= bp.download_left && x < bp.login_left { return ChromeHit::DownloadButton; }
                    if x >= bp.links_left && x < bp.download_left { return ChromeHit::LinksButton; }
                    if x >= bp.news_left && x < bp.links_left { return ChromeHit::NewsButton; }
                }
            }
        }

        // Tabs — the strip scrolls horizontally (state.tab_scroll) when it
        // overflows; zones are clamped to the visible strip so a tab clipped
        // at either edge doesn't get phantom hits outside it.
        let strip_limit = tabs_right_limit(width, state.flavor);

        // "⋯" overflow button, immediately left of "+" (rendered on top of
        // the clipped strip, so it wins over the tab zone beneath it).
        let plus_left = (tab_strip_end_x(state) - state.tab_scroll)
            .min(strip_limit - NEW_TAB_BUTTON_WIDTH);
        if max_tab_scroll(state, width) > 0.5
            && x >= plus_left - TAB_OVERFLOW_WIDTH && x < plus_left
        {
            return ChromeHit::TabOverflow;
        }

        let mut cursor = TAB_LEFT_MARGIN - state.tab_scroll;
        // When the strip overflows, tabs end at the "⋯" button — no slivers
        // of tab may peek (or hit-test) between "⋯"/"+" and the tab area.
        let tab_area_right = if max_tab_scroll(state, width) > 0.5 {
            plus_left - TAB_OVERFLOW_WIDTH
        } else {
            strip_limit
        };
        for (i, tw) in state.tab_widths.iter().enumerate() {
            let tab_right = cursor + tw;
            let vis_left = cursor.max(TAB_LEFT_MARGIN);
            let vis_right = tab_right.min(tab_area_right);
            if x >= vis_left && x < vis_right {
                // While a tab is being renamed inline its whole body is the
                // text field — the close × is hidden (render.rs), so it is
                // not hit-tested either.
                if state.renaming_tab == Some(i) {
                    return ChromeHit::Tab(i);
                }
                // Close × is drawn only while the tab is hovered (render.rs),
                // so its zone is hit-tested only then — an invisible button
                // must not eat clicks. The hover always registers one frame
                // earlier than the click, so this never misfires.
                let tab_hovered = matches!(state.hovered,
                    ChromeHit::Tab(h) | ChromeHit::TabClose(h) | ChromeHit::TabRename(h) if h == i);
                if tab_hovered && x >= tab_right - TAB_CLOSE_SIZE {
                    return ChromeHit::TabClose(i);
                }
                return ChromeHit::Tab(i);
            }
            cursor = tab_right + TAB_GAP;
        }

        // "+" button — always pinned inside the tab strip, never pushed
        // under the right-side controls (render.rs clamps it the same way).
        let cursor = cursor.min(strip_limit - NEW_TAB_BUTTON_WIDTH);
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

/// Returns the x coordinate of the left edge of the + button — after the
/// (scrolled) tabs, pinned inside the strip when they overflow.
pub fn new_tab_button_x(state: &ChromeState, width: f64) -> f64 {
    let limit = tabs_right_limit(width, state.flavor);
    (tab_strip_end_x(state) - state.tab_scroll).min(limit - NEW_TAB_BUTTON_WIDTH)
}

fn tab_strip_end_x(state: &ChromeState) -> f64 {
    let mut cursor = TAB_LEFT_MARGIN;
    for tw in &state.tab_widths {
        cursor += tw + TAB_GAP;
    }
    cursor
}

/// Geometry of the tab-list dropdown (opened from the "⋯" overflow button):
/// right-aligned under the "+"/"⋯" cluster, just below the chrome strip.
pub fn tab_list_rect(state: &ChromeState, width: f64) -> (f64, f64, f64, f64) {
    let limit = tabs_right_limit(width, state.flavor);
    let plus_left = (tab_strip_end_x(state) - state.tab_scroll)
        .min(limit - NEW_TAB_BUTTON_WIDTH);
    let h = TAB_LIST_PADDING * 2.0 + state.tabs.len() as f64 * TAB_LIST_ITEM_HEIGHT;
    let x = (plus_left + NEW_TAB_BUTTON_WIDTH - TAB_LIST_WIDTH).max(4.0);
    (x, CHROME_HEIGHT, TAB_LIST_WIDTH, h)
}

/// Hit-test the open tab-list dropdown: Some(tab index) on a row, None
/// outside. Coordinates are window-space (same as `hit_test`).
pub fn tab_list_hit_test(state: &ChromeState, x: f64, y: f64, width: f64) -> Option<usize> {
    if !state.tab_list_open {
        return None;
    }
    let (mx, my, mw, mh) = tab_list_rect(state, width);
    if x < mx || x >= mx + mw || y < my || y >= my + mh {
        return None;
    }
    let idx = ((y - my - TAB_LIST_PADDING) / TAB_LIST_ITEM_HEIGHT) as usize;
    if idx < state.tabs.len() {
        Some(idx)
    } else {
        None
    }
}

/// Update the hovered row of the open tab-list dropdown.
pub fn tab_list_hover(state: &mut ChromeState, x: f64, y: f64, width: f64) {
    state.tab_list_hovered = tab_list_hit_test(state, x, y, width).map(|i| i as i32).unwrap_or(-1);
}

/// Scroll the strip so tab `idx` is fully visible (used when a tab is
/// activated from the overflow list — it may be scrolled out of view).
pub fn scroll_tab_into_view(state: &mut ChromeState, width: f64, idx: usize) {
    if idx >= state.tab_widths.len() {
        return;
    }
    let limit = tabs_right_limit(width, state.flavor);
    let visible_right = limit - NEW_TAB_BUTTON_WIDTH - TAB_OVERFLOW_WIDTH - 5.0;
    let mut left = TAB_LEFT_MARGIN;
    for w in state.tab_widths.iter().take(idx) {
        left += w + TAB_GAP;
    }
    let right = left + state.tab_widths[idx];
    let cur_left = left - state.tab_scroll;
    let cur_right = right - state.tab_scroll;
    if cur_right > visible_right {
        scroll_tab_strip(state, width, cur_right - visible_right);
    } else if cur_left < TAB_LEFT_MARGIN {
        scroll_tab_strip(state, width, cur_left - TAB_LEFT_MARGIN);
    }
}

/// Left edge (window x, scroll applied) of tab `idx`.
pub fn tab_left_x(state: &ChromeState, idx: usize) -> f64 {
    let mut cursor = TAB_LEFT_MARGIN - state.tab_scroll;
    for w in state.tab_widths.iter().take(idx) {
        cursor += w + TAB_GAP;
    }
    cursor
}

/// Insertion index for a tab dropped at cursor x (window space): the count
/// of resting tabs whose MIDPOINT lies left of x. Range 0..=len.
pub fn tab_drop_index(state: &ChromeState, x: f64) -> usize {
    let mut cursor = TAB_LEFT_MARGIN - state.tab_scroll;
    let mut idx = 0;
    for w in &state.tab_widths {
        if x > cursor + w / 2.0 {
            idx += 1;
        }
        cursor += w + TAB_GAP;
    }
    idx
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::types::{ChromeFlavor, ChromeState};

    fn overflow_state(widths: Vec<f64>, renaming: Option<usize>) -> (ChromeState, f64) {
        let mut state = ChromeState::new("t");
        state.flavor = ChromeFlavor::Native;
        state.tab_widths = widths;
        state.renaming_tab = renaming;
        // Wide enough that MIN_TAB_WIDTH * n still fits, but the natural
        // sum does not — equalize (not the floor) is the interesting path.
        (state, 1200.0)
    }

    #[test]
    fn fit_keeps_natural_when_everything_fits() {
        let (mut state, width) = overflow_state(vec![80.0, 90.0, 100.0], None);
        fit_tab_widths(&mut state, width);
        assert_eq!(state.tab_widths, vec![80.0, 90.0, 100.0]);
    }

    #[test]
    fn fit_equalizes_on_overflow_after_rename() {
        let (mut state, width) = overflow_state(vec![300.0, 300.0, 300.0, 500.0], None);
        let before: f64 = state.tab_widths.iter().sum();
        fit_tab_widths(&mut state, width);
        let after: f64 = state.tab_widths.iter().sum();
        assert!(after < before, "overflow must shrink the strip");
        let first = state.tab_widths[0];
        for w in &state.tab_widths {
            assert!((w - first).abs() < 0.001, "overflow equalizes, got {:?}", state.tab_widths);
            assert!(*w >= MIN_TAB_WIDTH);
        }
    }

    #[test]
    fn fit_keeps_live_rename_natural_and_equalizes_neighbors() {
        let (mut state, width) = overflow_state(vec![300.0, 300.0, 300.0, 500.0], Some(3));
        fit_tab_widths(&mut state, width);
        assert!((state.tab_widths[3] - 500.0).abs() < 0.001, "live tab keeps natural");
        let rest = &state.tab_widths[..3];
        assert!((rest[0] - rest[1]).abs() < 0.001);
        assert!((rest[1] - rest[2]).abs() < 0.001);
        assert!(rest[0] >= MIN_TAB_WIDTH);
        assert!(rest[0] < 300.0, "neighbors give up width so the live tab can stay natural");
    }
}
