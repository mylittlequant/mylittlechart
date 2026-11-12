//! The window chrome strip (Native flavor) as uzor DOM.
//!
//! Was: `mlc-app-input-shell` `chrome::render`, every cell placed by hand from
//! `ButtonPositions`. Now: a DOM tree over the same [`chrome::ChromeState`]: the tabs
//! are a flex row (the widths stay the app's own `tab_widths`, fitted to the text by
//! `chrome::update_tab_widths`), the right cluster is a flex row of cells pinned to the right edge,
//! and the hover look is the app's own `hovered` (the app still hit-tests moves).
//!
//! Clicks: the cells with ids call the app's existing press actions through
//! [`mlc_app_input_shell::press_chrome_hit`] with the element they stand for
//! ([`hit_of`]). The tab bodies and the caption carry no id, so a press there goes
//! on to the app as before (tab drag, double-click rename, window drag).
//!
//! Covered: the states [`covers`] accepts. Any other (Web flavor, skeleton mode, a
//! tab being renamed or dragged, an overflowing strip, a signed-in account button)
//! is still painted by `chrome::render`, frame for frame.

use mlc_app_input_shell::chrome::{self, ChromeFlavor, ChromeHit, ChromeState};
use mlc_core::state::icons::icon_svg;
use uzor::ui::dom::El;

/// Id prefix of the strip's cells.
pub const ID: &str = "chrome";
const H: f64 = chrome::CHROME_HEIGHT;
const BUTTON_W: f64 = 46.0;
const SMALL_W: f64 = 36.0;
const NEW_TAB_W: f64 = 28.0;
const TAB_LEFT: f64 = 4.0;
const TAB_GAP: f64 = 4.0;
const TAB_CLOSE: f64 = 16.0;
/// Separators: 60 % of the strip, centred.
const SEP_H: f64 = H * 0.6;
const SEP_Y: f64 = (H - SEP_H) / 2.0;

/// Whether the DOM strip draws this state (else the app's own `chrome::render`).
pub fn covers(state: &ChromeState, width: f64, skeleton: bool) -> bool {
    state.flavor == ChromeFlavor::Native
        && !skeleton
        && state.renaming_tab.is_none()
        && !state.tab_drag.is_some_and(|d| d.active)
        && state.account_label.is_none()
        && chrome::max_tab_scroll(state, width) <= 0.5
        && state.tab_widths.len() == state.tabs.len()
}

/// The strip's colours, as CSS.
pub fn css(state: &ChromeState) -> String {
    let c = &state.colors;
    format!(
        "div.c-strip {{ position: relative; height: {H}px; background: {bg}; color: {ink}; font-size: 12px; font-family: sans-serif }}
div.c-tabs {{ display: flex; position: absolute; left: 0px; top: 0px; height: {H}px; padding-left: {TAB_LEFT}px; gap: {TAB_GAP}px; align-items: stretch }}
div.c-tab {{ position: relative; height: {H}px; flex-shrink: 0 }}
div.c-name {{ position: absolute; left: 0px; top: 0px; height: {H}px; overflow: hidden; display: flex; align-items: center; padding-top: 4px }}
div.c-mark {{ position: absolute; left: 0px; top: {mark_y}px; height: 2px; background: {accent} }}
div.c-mark.hov {{ background: {hover} }}
div.c-tclose {{ position: absolute; top: 3px; width: {tcw}px; height: {tch}px; border-radius: 6px; background: {bg}; padding: 6px 0 0 2px }}
div.c-tclose.hov {{ background: {hover}; color: {danger} }}
div.c-tclose > div {{ width: 14px; height: 14px }}
div.c-sep {{ position: absolute; top: {SEP_Y}px; width: 1px; height: {SEP_H}px; background: {sep} }}
div.c-new {{ position: absolute; top: 0px; height: {H}px; width: {nw}px }}
div.c-new.hov {{ background: {hover} }}
div.c-bar {{ position: absolute; background: {ink} }}
div.c-right {{ display: flex; position: absolute; right: 0px; top: 0px; width: {rw}px; height: {H}px }}
div.c-cell {{ position: relative; height: {H}px; width: {SMALL_W}px; flex-shrink: 0 }}
div.c-cell.wide {{ width: {BUTTON_W}px }}
div.c-cell.hov {{ background: {hover} }}
div.c-cell.close.hov {{ background: {danger} }}
div.c-cell.ctl.hov {{ color: {ink_hover} }}
div.c-icon {{ position: absolute; left: 9px; top: 7px; width: 18px; height: 18px }}
div.c-x {{ position: absolute }}
div.c-min {{ position: absolute; left: 18px; top: 16px; width: 10px; height: 1px; background: {ink} }}
div.c-cell.hov > div.c-min {{ background: {ink_hover} }}
div.c-max {{ position: absolute; left: 18px; top: 11px; width: 10px; height: 10px; border: 1px solid {ink} }}
div.c-cell.hov > div.c-max {{ border-color: {ink_hover} }}
div.c-max.under {{ left: 20px; top: 9px }}
div.c-max.over {{ background: {bg} }}
div.c-cell > div.c-sep {{ left: 0px }}
div.c-bottom {{ position: absolute; left: 0px; top: {bot}px; height: 1px; background: {sep} }}",
        bg = c.background,
        ink = c.icon_normal,
        ink_hover = c.icon_hover,
        accent = c.tab_accent,
        hover = c.button_hover,
        danger = c.close_hover,
        sep = c.separator,
        mark_y = H - 1.0 - 1.0 - 2.0,
        tcw = TAB_CLOSE + 1.0,
        tch = H - 9.0,
        nw = NEW_TAB_W - 1.0,
        bot = H - 1.0,
        rw = 3.0 * SMALL_W + 3.0 * BUTTON_W,
    )
}

/// The app element a cell id stands for.
pub fn hit_of(id: &str) -> Option<ChromeHit> {
    let rest = id.strip_prefix(ID)?.strip_prefix(':')?;
    Some(match rest {
        "new_tab" => ChromeHit::NewTabButton,
        "new_window" => ChromeHit::NewWindowButton,
        "menu" => ChromeHit::MenuButton,
        "close_window" => ChromeHit::CloseWindowButton,
        "minimize" => ChromeHit::MinimizeButton,
        "maximize" => ChromeHit::MaximizeButton,
        "close" => ChromeHit::CloseButton,
        _ => ChromeHit::TabClose(rest.strip_prefix("tab_close:")?.parse().ok()?),
    })
}

fn id(name: &str) -> String {
    format!("{ID}:{name}")
}

/// An × of two strokes `s` from the centre `(cx, cy)`, `w` wide, where
/// `chrome::render` stroked them: the icon box sits on whole pixels and the strokes
/// keep their fractional place inside it. A fractional pen is drawn as the strokes'
/// outlines, filled (the SVG icon painter rounds pen widths to whole pixels).
fn cross(cx: f64, cy: f64, s: f64, w: f64) -> El {
    let (x0, y0) = ((cx - s - w).floor(), (cy - s - w).floor());
    let size = (2.0 * (s + w)).ceil() + 1.0;
    let (ax, ay) = (cx - s - x0, cy - s - y0);
    let (bx, by) = (ax + 2.0 * s, ay + 2.0 * s);
    let p = w / 2.0 / std::f64::consts::SQRT_2;
    let quad = |x1: f64, y1: f64, x2: f64, y2: f64, nx: f64, ny: f64| {
        format!("M{} {}L{} {}L{} {}L{} {}Z", x1 + nx, y1 + ny, x2 + nx, y2 + ny, x2 - nx, y2 - ny, x1 - nx, y1 - ny)
    };
    let svg = if w.fract() == 0.0 {
        format!(r#"<svg viewBox="0 0 {size} {size}" fill="none" stroke="currentColor" stroke-width="{w}"><path d="M{ax} {ay}L{bx} {by}M{ax} {by}L{bx} {ay}"/></svg>"#)
    } else {
        format!(
            r#"<svg viewBox="0 0 {size} {size}" fill="currentColor" stroke="none"><path d="{}"/><path d="{}"/></svg>"#,
            quad(ax, ay, bx, by, p, -p),
            quad(ax, by, bx, ay, p, p),
        )
    };
    El::new("div.c-x").css(format!("left: {x0}px; top: {y0}px; width: {size}px; height: {size}px")).svg(svg)
}

fn cell(name: &str, cls: &str, hov: bool) -> El {
    let spec = if hov { format!("div.c-cell{cls}.hov") } else { format!("div.c-cell{cls}") };
    El::new(spec).id(id(name))
}

fn icon(name: &str) -> El {
    let e = El::new("div.c-icon");
    match icon_svg(name) {
        Some(svg) => e.svg(svg),
        None => e,
    }
}

/// The strip `width` wide, for `state` (which [`covers`] accepted). `text_w`: each
/// tab name's width at 12px, as the app measures it: the name starts where the old
/// drawing put it, to the fraction of a pixel.
pub fn tree(state: &ChromeState, width: f64, text_w: &[f64]) -> El {
    let hov = state.hovered;
    let mut tabs = El::new("div.c-tabs");
    for (i, tab) in state.tabs.iter().enumerate() {
        let tw = state.tab_widths[i];
        let shows_close = matches!(hov, ChromeHit::Tab(j) | ChromeHit::TabClose(j) | ChromeHit::TabRename(j) if j == i);
        let name_w = if shows_close { (tw - TAB_CLOSE).max(0.0) } else { tw };
        let mut t = El::new("div.c-tab").css(format!("width: {tw}px"));
        if tab.active {
            t = t.child(El::new("div.c-mark").css(format!("width: {tw}px")));
        } else if hov == ChromeHit::Tab(i) {
            t = t.child(El::new("div.c-mark.hov").css(format!("width: {tw}px")));
        }
        t = t.child(
            El::new("div.c-name")
                .css(format!("width: {name_w}px"))
                .child(El::new("span").css(format!("margin-left: {}px", (name_w - text_w.get(i).copied().unwrap_or(0.0)).max(0.0) / 2.0)).text(tab.name.clone())),
        );
        if shows_close {
            let spec = if hov == ChromeHit::TabClose(i) { "div.c-tclose.hov" } else { "div.c-tclose" };
            let mut x = El::new("div");
            if let Some(svg) = icon_svg("close") {
                x = x.svg(svg);
            }
            t = t.child(El::new(spec).id(id(&format!("tab_close:{i}"))).css(format!("left: {}px", tw - TAB_CLOSE - 1.0)).child(x));
        }
        tabs = tabs.child(t);
    }

    // Where `chrome::render` puts "+": right after the last tab (no trailing gap),
    // pinned short of the right cluster. (`chrome::new_tab_button_x`, which the hit
    // test reads, counts one gap more; the drawing is what is matched here.)
    let n = state.tab_widths.len() as f64;
    let natural = state.tab_widths.iter().sum::<f64>() + TAB_GAP * (n - 1.0).max(0.0);
    let nx = (TAB_LEFT - state.tab_scroll + natural).min(chrome::tabs_right_limit(width, state) - NEW_TAB_W);
    let plus_cx = (nx + NEW_TAB_W / 2.0).floor() + 0.5;
    let plus_cy = (H / 2.0).floor() + 0.5;
    let new_tab = El::new(if hov == ChromeHit::NewTabButton { "div.c-new.hov" } else { "div.c-new" })
        .id(id("new_tab"))
        .css(format!("left: {}px", nx + 1.0));
    let bar = |l: f64, t: f64, w: f64, h: f64| El::new("div.c-bar").css(format!("left: {l}px; top: {t}px; width: {w}px; height: {h}px"));

    let is_max = state.is_maximized;
    let maximize = {
        let mut c = cell("maximize", ".wide.ctl", hov == ChromeHit::MaximizeButton);
        if is_max {
            c = c.child(El::new("div.c-max.under")).child(El::new("div.c-max.over"));
        } else {
            c = c.child(El::new("div.c-max"));
        }
        c
    };
    let right = El::new("div.c-right")
        .child(cell("new_window", "", hov == ChromeHit::NewWindowButton).child(icon("new_window")))
        .child(cell("menu", "", hov == ChromeHit::MenuButton).child(icon("settings")))
        .child(
            cell("close_window", ".ctl", hov == ChromeHit::CloseWindowButton)
                .child(El::new("div.c-sep"))
                .child(cross(SMALL_W / 2.0, H / 2.0, 3.5, 1.0)),
        )
        .child(cell("minimize", ".wide.ctl", hov == ChromeHit::MinimizeButton).child(El::new("div.c-sep")).child(El::new("div.c-min")))
        .child(maximize)
        .child(cell("close", ".wide.close", hov == ChromeHit::CloseButton).child(cross(BUTTON_W / 2.0, H / 2.0, 5.0, 1.5)));

    El::new("div.c-strip")
        .css(format!("width: {width}px"))
        .child(tabs)
        .child(El::new("div.c-sep").css(format!("left: {nx}px")))
        .child(new_tab)
        .child(bar(plus_cx - 5.0, plus_cy - 0.75, 10.0, 1.5))
        .child(bar(plus_cx - 0.75, plus_cy - 5.0, 1.5, 10.0))
        .child(right)
        .child(El::new("div.c-bottom").css(format!("width: {width}px")))
}
