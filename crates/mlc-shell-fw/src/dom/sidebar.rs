//! The right sidebar as uzor DOM: the frame, the header and the watchlist here, the
//! other panels' bodies in [`super::sidebar_panels`].
//!
//! Was: `mlc-app-sidebar-content` `render_right_sidebar` painting the frame, the
//! header, the pinned column header, the rows and the ticker footer by hand. Now: a
//! DOM tree over the same `SidebarState` and column config, placed where that code
//! placed each piece; the rows are a framework-owned scroll container
//! (`dom_scroll`, `overflow: auto`) whose offset the host copies into the panel's
//! scroll state. The app still lays the panel out and registers its hit zones, with
//! its drawing switched off (`dom_hosted.sidebar`, `mlc_app::paint_off`), so every
//! press goes to it as the raw press it always got (the host forwards it, as for
//! the toolbars): row selection and drag, the double-click, the column resize, the
//! metrics drag, the buttons.
//!
//! Covered: watchlist, alerts, events, object tree, signals, monitoring (its
//! Performance tab) and screener, including the watchlist's column menu, colour
//! picker, row drag, market counters and failing rows; outside the states
//! `ChartApp::sidebar_on_dom` refuses (a signal group that scrolls, the Connectors
//! tab). Agents and the slot panels are still painted by the app.

use mlc_app_layout::ui::toolbar_theme::ToolbarTheme;
use mlc_app_sidebar_content::render::{
    counters_block_h, format_price, format_price_compact, format_volume, watchlist_col_layout, watchlist_metrics_block_h, WATCHLIST_TYPE_COMPACT,
    WATCHLIST_TYPE_HEADER, WATCHLIST_TYPE_PAD,
};
use mlc_app_sidebar_content::state::{RightSidebarPanel, SidebarState};

use super::sidebar_panels as panels;
use mlc_app_sidebar_content::types::WatchlistColumnConfig;
use mlc_core::state::icons::Icon;
use uzor::types::Rect;
use uzor::ui::dom::El;

/// The rows' scroll container.
pub const VIEWPORT: &str = "right_sidebar:viewport";
/// Paint slot prefix of the row sparklines (`{SPARK}{row}`); the ticker's is [`TICKER_SPARK`].
pub const SPARK: &str = "sb:spark:";
/// How far the sparkline slots reach past the line's box: its round caps and joins
/// spill a pixel over, which the DOM would clip.
pub const SPARK_BLEED: f64 = 2.0;
/// The colour picker's "none" swatch cross (two 1.5 px strokes, round caps).
pub const SWATCH_X: &str = "sb:swatch_x";
/// The ticker footer's sparkline slot.
pub const TICKER_SPARK: &str = "sb:tspark";
/// The frosted backdrop's paint slot (the host calls `draw_blur_background` over the
/// sidebar box there).
pub const BLUR: &str = "sb:blur";
/// How far above the box the frame reaches (the backdrop smears up to here).
pub const OUT: f64 = 8.0;
/// The scrollbar's paint slot.
pub const SCROLLBAR: &str = "sb:scrollbar";

pub(crate) const HEADER_H: f64 = 40.0;
const SCROLLBAR_W: f64 = 8.0;
const COL_HEADER_H: f64 = 22.0;
const ROW_H: f64 = 36.0;
const PAD: f64 = 8.0;
const BTN: f64 = 16.0;
const BTN_PAD: f64 = 4.0;
const DEL: f64 = 12.0;
const DEL_PAD: f64 = 6.0;
const TEXT_BOX: f64 = 20.0;

/// Everything the tree is built from.
pub struct Sidebar<'a> {
    /// The sidebar, window coordinates.
    pub rect: Rect,
    pub theme: &'a ToolbarTheme,
    pub state: &'a SidebarState,
    pub columns: &'a WatchlistColumnConfig,
    /// Each row's colour flag (`""` = none).
    pub flags: &'a [String],
    /// The DOM element under the pointer last frame.
    pub hovered: Option<&'a str>,
    /// The app's scroll viewport of the panel body, window coordinates (its
    /// `RightSidebarResult::viewport_rect`).
    pub viewport: Rect,
    /// The body's content height as the app laid it out.
    pub app_content_h: f64,
    /// Now, unix ms (event times).
    pub now_ms: i64,
    /// The sidebar in the app's own coordinates (what its stored popup positions,
    /// like the colour picker's, are relative to).
    pub app_rect: Rect,
}

impl Sidebar<'_> {
    /// Height of the pinned metrics footer (0 when hidden).
    pub fn metrics_h(&self) -> f64 {
        watchlist_metrics_block_h(self.columns, self.state.ticker_section.as_ref(), &self.state.counter_rows, self.rect.height)
    }

    /// The rows' viewport height.
    pub fn view_h(&self) -> f64 {
        (self.rect.height - HEADER_H - COL_HEADER_H - 1.0 - self.metrics_h()).max(0.0)
    }

    /// The rows' content height (as the app reports it).
    pub fn content_h(&self) -> f64 {
        if self.state.watchlist_items.is_empty() {
            60.0
        } else {
            ROW_H * self.state.watchlist_items.len() as f64
        }
    }

    pub(crate) fn hov(&self, id: &str) -> bool {
        self.hovered == Some(id)
    }
}

/// The panel's colours, as CSS.
pub fn css(t: &ToolbarTheme) -> String {
    format!(
        "div.s-root {{ position: relative; flex-shrink: 0; background: {bg}; color: {ink}; font-family: sans-serif; font-size: 12px }}
div.s-abs {{ position: absolute }}
div.s-fill {{ position: absolute; background: {sep} }}
div.s-txt {{ position: absolute; display: flex; align-items: center; white-space: nowrap; height: {TEXT_BOX}px; padding-top: 2px }}
div.s-txt.lo {{ padding-top: 4px }}
div.s-txt.l {{ justify-content: flex-start }}
div.s-txt.c {{ justify-content: center }}
div.s-txt.r {{ justify-content: flex-end }}
div.s-muted {{ color: {muted} }}
div.s-btn {{ position: absolute; width: {bs}px; height: {bs}px; border-radius: 4px; color: {muted} }}
div.s-btn.hov {{ background: {hover}; color: {ink} }}
div.s-icon {{ position: absolute; left: {BTN_PAD}px; top: {BTN_PAD}px; width: {BTN}px; height: {BTN}px }}
div.s-colhead {{ position: absolute; left: 0px; top: {HEADER_H}px; height: {COL_HEADER_H}px; background: {bg} }}
div.s-hit {{ position: absolute }}
div.s-view {{ position: absolute; left: 0px; top: {vt}px; overflow: auto; scrollbar-width: none }}
div.s-row {{ position: relative; height: {ROW_H}px }}
div.s-row.hov {{ background: {hover} }}
div.s-del {{ position: absolute; width: {db}px; height: {db}px; background: {dd}; color: {muted} }}
div.s-del > div.s-tint {{ position: absolute; left: 0px; top: 0px; width: {db}px; height: {db}px; background: {hover} }}
div.s-del.hov {{ color: #ff5252 }}
div.s-del > div.s-icon {{ left: {BTN_PAD}px; top: {BTN_PAD}px; width: {DEL}px; height: {DEL}px }}
div.s-foot {{ position: absolute; left: 0px; background: {bg} }}
div.s-at {{ position: absolute; white-space: nowrap; height: 40px }}
div.s-content {{ position: relative }}
div.s-clip {{ position: absolute; overflow: hidden }}
div.s-scroll {{ position: absolute; top: {vt}px; width: {SCROLLBAR_W}px }}",
        bg = t.background,
        ink = t.item_text,
        muted = t.item_text_muted,
        hover = t.item_bg_hover,
        sep = t.separator,
        dd = t.dropdown_bg,
        bs = BTN + 2.0 * BTN_PAD,
        db = DEL + 2.0 * BTN_PAD,
        vt = HEADER_H + COL_HEADER_H + 1.0,
    )
}

fn fill(l: f64, t: f64, w: f64, h: f64, color: &str) -> El {
    El::new("div.s-fill").css(format!("left: {l}px; top: {t}px; width: {w}px; height: {h}px; background: {color}"))
}

#[derive(Clone, Copy)]
enum Align {
    L,
    C,
    R,
}

/// `text` in `font` and `color`, vertically centred on `cy`, placed as the app's
/// `fill_text` with `align` at `x` (for `C`, the middle of `l..r`; for `R`, `r`).
fn text(text: &str, l: f64, r: f64, cy: f64, align: Align, font: &str, color: &str) -> El {
    let (cls, l, w) = match align {
        Align::L => ("div.s-txt.l", l, (r - l).max(0.0) + 64.0),
        Align::C => {
            // A box centred on the anchor, wide enough for any cell text.
            let mid = (l + r) / 2.0;
            ("div.s-txt.c", mid - 200.0, 400.0)
        }
        Align::R => ("div.s-txt.r", r - 400.0, 400.0),
    };
    // The app's middle baseline sits a pixel below the DOM's centred line box, two
    // for 12px, the big price and the monospace line.
    let lo = font.starts_with("12px") || font.contains("22px") || font.contains("monospace");
    let cls = if lo { format!("{cls}.lo") } else { cls.to_string() };
    El::new(cls)
        .css(format!("left: {l}px; top: {}px; width: {w}px; {}; color: {color}", cy - TEXT_BOX / 2.0, font_css(font)))
        .child(El::new("span").text(text.to_string()))
}

/// A canvas font string (`"bold 22px sans-serif"`) as CSS declarations.
pub(crate) fn font_css(font: &str) -> String {
    let mut out = Vec::new();
    let mut family = Vec::new();
    for part in font.split_whitespace() {
        if part == "bold" {
            out.push("font-weight: bold".to_string());
        } else if part.len() == 3 && part.ends_with("00") && part.parse::<u16>().is_ok() {
            out.push(format!("font-weight: {part}"));
        } else if part.ends_with("px") && part[..part.len() - 2].parse::<f64>().is_ok() {
            out.push(format!("font-size: {part}"));
        } else {
            family.push(part);
        }
    }
    if !family.is_empty() {
        out.push(format!("font-family: {}", family.join(" ")));
    }
    out.join("; ")
}

fn icon(icon: Icon, size: f64) -> El {
    let _ = size;
    El::new("div.s-icon").svg(icon.svg())
}

/// `text` cut to `max` px wide with a trailing "…", as the app's `truncate_to_width`.
pub(crate) fn truncate(measure: &mut dyn FnMut(&str, &str) -> f64, font: &str, text: &str, max: f64) -> String {
    if measure(font, text) <= max {
        return text.to_string();
    }
    let avail = max - measure(font, "\u{2026}");
    if avail <= 0.0 {
        return String::new();
    }
    let mut t = text.to_string();
    while !t.is_empty() && measure(font, &t) > avail {
        t.pop();
    }
    t.push('\u{2026}');
    t
}

struct Cols {
    labels: Vec<&'static str>,
    area_left: f64,
    area_right: f64,
    seps: Vec<f64>,
}

impl Cols {
    fn of(c: &WatchlistColumnConfig, content_w: f64, measure: &mut dyn FnMut(&str, &str) -> f64) -> Cols {
        let mut labels = vec!["Symbol"];
        for (on, l) in [
            (c.show_exchange, "Exchange"),
            (c.show_account_type, "Type"),
            (c.show_last_price, "Last"),
            (c.show_change_pct, "Chg %"),
            (c.show_spark, "Spark"),
            (c.show_change_abs, "Chg"),
        ] {
            if on {
                labels.push(l);
            }
        }
        if c.show_high_low {
            labels.push("High");
            labels.push("Low");
        }
        if c.show_volume {
            labels.push("Vol");
        }
        let usable = content_w - PAD * 2.0;
        let type_w = measure("10px sans-serif", WATCHLIST_TYPE_COMPACT) + WATCHLIST_TYPE_PAD;
        let type_col = labels.iter().position(|l| *l == WATCHLIST_TYPE_HEADER);
        let (_, seps) = watchlist_col_layout(labels.len(), PAD, usable, type_col, type_w, c.separator_offsets.as_deref());
        Cols { labels, area_left: PAD, area_right: PAD + usable, seps }
    }

    fn n(&self) -> usize {
        self.labels.len()
    }

    fn clip(&self, i: usize) -> (f64, f64) {
        let l = if i == 0 { self.area_left } else { self.seps[i - 1] };
        let r = if i < self.seps.len() { self.seps[i] } else { self.area_right };
        (l, r)
    }

    fn align(&self, i: usize) -> Align {
        if i == 0 {
            Align::L
        } else if i == self.n() - 1 {
            Align::R
        } else {
            Align::C
        }
    }

    /// The text box for column `i` (`align`'s anchor as the app computes it).
    fn cell(&self, i: usize, s: &str, cy: f64, font: &str, color: &str, measure: &mut dyn FnMut(&str, &str) -> f64) -> El {
        let (l, r) = self.clip(i);
        let w = measure(font, s);
        let x = match self.align(i) {
            Align::L => l + 2.0,
            Align::C => (l + r) / 2.0 - w / 2.0,
            Align::R => r - 2.0 - w,
        };
        text(s, pen(x), r.max(x + w), cy, Align::L, font, color)
    }
}

/// Where the app's text renderer starts a run whose pen is at `x`.
fn pen(x: f64) -> f64 {
    x
}

fn chg_color<'t>(t: &'t ToolbarTheme, pct: f64) -> &'t str {
    if pct > 0.0 {
        &t.success
    } else if pct < 0.0 {
        &t.danger
    } else {
        &t.item_text_muted
    }
}

/// The whole panel, in sidebar coordinates. `measure(font, text)`: the app's text
/// width. Build it with `dom_scroll()`.
pub fn tree(w: &Sidebar<'_>, measure: &mut dyn FnMut(&str, &str) -> f64) -> El {
    let t = w.theme;
    let (rw, rh) = (w.rect.width, w.rect.height);
    let mut root = El::new("div.s-root").css(format!("width: {rw}px; height: {rh}px"));

    // Left border: the app's 1 px stroke on the edge, which lands on the pixel left of it.
    root = root.child(fill(-1.0, 0.0, 1.0, rh, &t.separator));
    root = header(root, w, measure);
    root = match w.state.right_panel {
        RightSidebarPanel::Watchlist => watchlist(root, w, measure),
        _ => panels::body(root, w, measure),
    };
    framed(root, w)
}

/// The panel's title and icon, as the app names them.
fn title(p: RightSidebarPanel) -> (&'static str, Icon) {
    match p {
        RightSidebarPanel::Watchlist => ("Watchlist", Icon::Watchlist),
        RightSidebarPanel::Alerts => ("Alerts", Icon::Alert),
        RightSidebarPanel::ObjectTree => ("Object Tree", Icon::Layers),
        RightSidebarPanel::Signals => ("Signals", Icon::Signal),
        RightSidebarPanel::Monitoring => ("Monitoring", Icon::Gauge),
        RightSidebarPanel::Screener => ("Screener", Icon::Screener),
        RightSidebarPanel::Events => ("Events", Icon::History),
        _ => ("", Icon::Close),
    }
}

/// The 40 px header: icon, title, the panel's buttons, × and the 2 px bar under it.
fn header(mut root: El, w: &Sidebar<'_>, measure: &mut dyn FnMut(&str, &str) -> f64) -> El {
    let t = w.theme;
    let rw = w.rect.width;
    let (name, ic) = title(w.state.right_panel);
    root = root.child(El::new("div.s-abs.s-muted").css(format!("left: 12px; top: {}px; width: 18px; height: 18px", (HEADER_H - 18.0) / 2.0)).svg(ic.svg()));
    root = root.child(text(name, 36.0, rw, HEADER_H / 2.0, Align::L, "13px sans-serif", &t.item_text));
    let close_x = rw - BTN - 2.0;
    let by = (HEADER_H - BTN) / 2.0;
    let mut buttons = vec![("right_sidebar_close", Icon::Close, close_x)];
    match w.state.right_panel {
        RightSidebarPanel::Watchlist => {
            buttons.insert(0, ("watchlist_column_config", Icon::MoreHorizontal, close_x - (BTN + 8.0)));
            buttons.insert(0, ("watchlist_open_modal", Icon::Grid, close_x - 2.0 * (BTN + 8.0)));
        }
        RightSidebarPanel::Alerts => {
            // Drawn muted, with no hover face.
            let x = close_x - BTN - 8.0;
            root = root.child(El::new("div.s-abs.s-muted").id("alert_add_button").css(format!("left: {x}px; top: {by}px; width: {BTN}px; height: {BTN}px")).svg(Icon::Plus.svg()));
        }
        RightSidebarPanel::Events | RightSidebarPanel::Screener => {
            let (id, label) = if w.state.right_panel == RightSidebarPanel::Events { ("events_mark_all_read", "Mark all read") } else { ("screener_scan", "Scan") };
            let font = "11px sans-serif";
            let bw = measure(font, label) + 14.0;
            let (bx, bh) = (close_x - bw - 8.0, 20.0);
            let bt = (HEADER_H - bh) / 2.0;
            let bg = if w.hov(id) { &t.item_bg_active } else { &t.item_bg_hover };
            root = root.child(El::new("div.s-abs").id(id).css(format!("left: {bx}px; top: {bt}px; width: {bw}px; height: {bh}px; border-radius: 4px; background: {bg}")));
            root = root.child(panels::mid_text(label, bx + bw / 2.0, bt + bh / 2.0, panels::At::C, font, &t.item_text, measure));
        }
        _ => {}
    }
    for (id, ic, x) in buttons {
        let spec = if w.hov(id) { "div.s-btn.hov" } else { "div.s-btn" };
        root = root.child(El::new(spec).id(id).css(format!("left: {}px; top: {}px", x - BTN_PAD, by - BTN_PAD)).child(icon(ic, BTN)));
    }
    // Header bottom bar (2 px; what is pinned under it covers its lower pixel).
    root.child(fill(0.0, HEADER_H - 1.0, rw, 2.0, &t.separator))
}

/// The watchlist below the header: the pinned column header, the rows, the footer.
fn watchlist(mut root: El, w: &Sidebar<'_>, measure: &mut dyn FnMut(&str, &str) -> f64) -> El {
    let t = w.theme;
    let s = w.state;
    let (rw, rh) = (w.rect.width, w.rect.height);
    let content_w = rw - SCROLLBAR_W;

    // Column header, pinned.
    let cols = Cols::of(w.columns, content_w, measure);
    let mut head = El::new("div.s-colhead").id("watchlist:column_header").css(format!("width: {content_w}px"));
    let flag_fill = match (s.watchlist_sort_mode, w.hov("watchlist_sort_color")) {
        (0, false) => t.item_bg_hover.as_str(),
        (0, true) => t.item_bg_active.as_str(),
        (_, true) => "#ffffff",
        (1, _) => "#ef5350",
        (2, _) => "#6b7280",
        _ => "#555566",
    };
    head = head.child(fill(0.0, 0.0, 10.0, COL_HEADER_H, flag_fill).id("watchlist_sort_color"));
    let font = "10px sans-serif";
    let type_full = measure(font, WATCHLIST_TYPE_HEADER);
    let type_compact = measure(font, WATCHLIST_TYPE_COMPACT);
    for (i, label) in cols.labels.iter().enumerate() {
        let (l, r) = cols.clip(i);
        let cw = (r - l).max(0.0);
        if cw < 1.0 {
            continue;
        }
        let label = if *label == WATCHLIST_TYPE_HEADER {
            if cw + 0.5 >= type_full {
                WATCHLIST_TYPE_HEADER
            } else if cw + 0.5 >= type_compact {
                WATCHLIST_TYPE_COMPACT
            } else {
                continue;
            }
        } else {
            label
        };
        let shown = truncate(measure, font, label, cw);
        head = head.child(cols.cell(i, &shown, COL_HEADER_H / 2.0, font, &t.item_text_muted, measure));
        if label == WATCHLIST_TYPE_COMPACT {
            // The compact "T" carries a hover zone (its tooltip).
            head = head.child(El::new("div.s-hit").id("watchlist_col_type").css(format!("left: {l}px; top: 0px; width: {cw}px; height: {COL_HEADER_H}px")));
        }
    }
    for (k, &sx) in cols.seps.iter().enumerate() {
        let id = format!("watchlist_sep_{}", k + 1);
        let color = if w.hov(&id) { "#4a9eff" } else { t.separator.as_str() };
        head = head.child(fill(sx - 1.0, 3.0, 1.0, COL_HEADER_H - 6.0, color));
        // The stroke's ends, a little under half covered.
        let cap = mlc_core::mix_colors(&t.background, color, 0.43);
        head = head.child(fill(sx - 1.0, 2.0, 1.0, 1.0, &cap)).child(fill(sx - 1.0, COL_HEADER_H - 3.0, 1.0, 1.0, &cap));
        head = head.child(El::new("div.s-hit").id(id).css(format!("left: {}px; top: 0px; width: 8px; height: {COL_HEADER_H}px", sx - 0.5 - 4.0)));
    }
    root = root.child(head);
    root = root.child(fill(cols.area_left, HEADER_H + COL_HEADER_H, cols.area_right - cols.area_left, 1.0, &t.separator));

    // Rows.
    let view_h = w.view_h();
    // The border's other half shows where nothing paints over it: beside the body.
    root = root.child(El::new("div.s-fill").css(format!(
        "left: 0px; top: {}px; width: 1px; height: {}px; opacity: 0.5; background: {}",
        HEADER_H + COL_HEADER_H,
        rh - w.metrics_h() - HEADER_H - COL_HEADER_H,
        t.separator
    )));
    let mut view = El::new("div.s-view").id(VIEWPORT).css(format!("width: {rw}px; height: {view_h}px"));
    if s.watchlist_items.is_empty() {
        let mut empty = El::new("div.s-abs").css(format!("left: 0px; top: 0px; width: {content_w}px; height: 60px"));
        let mid = content_w / 2.0;
        // The app draws these top-aligned at `y` and `y + 20`.
        for (line, cy) in [("No symbols in watchlist", 5.0), ("Click + to add symbols", 25.0)] {
            let x = mid - measure("12px sans-serif", line) / 2.0;
            empty = empty.child(text(line, pen(x), content_w, cy, Align::L, "12px sans-serif", &t.item_text_muted));
        }
        view = view.child(empty);
    } else {
        let data_font = if cols.n() - 1 > 4 { "10px sans-serif" } else { "11px sans-serif" };
        let c = w.columns;
        for (i, item) in s.watchlist_items.iter().enumerate() {
            let row_id = format!("watchlist_{i}");
            let del_id = format!("watchlist_delete_{i}");
            let flag_id = format!("watchlist_flag_{i}");
            let hovered = w.hov(&row_id) || w.hov(&del_id);
            let mut row = El::new(if hovered { "div.s-row.hov" } else { "div.s-row" }).id(row_id).css(format!("width: {content_w}px"));
            let flag = w.flags.get(i).map(String::as_str).unwrap_or("");
            let flag_bg = if !flag.is_empty() {
                Some(flag)
            } else if w.hov(&flag_id) {
                Some(t.item_bg_hover.as_str())
            } else if hovered {
                Some(t.item_bg_active.as_str())
            } else {
                None
            };
            let bg = flag_bg.map(|c| format!("; background: {c}")).unwrap_or_default();
            row = row.child(El::new("div.s-hit").id(flag_id).css(format!("left: 0px; top: 0px; width: 10px; height: {ROW_H}px{bg}")));
            let cy = ROW_H / 2.0;
            let cell = |row: El, i: usize, s: &str, font: &str, color: &str, measure: &mut dyn FnMut(&str, &str) -> f64| -> El {
                let (l, r) = cols.clip(i);
                let cw = (r - l).max(0.0);
                if cw < 1.0 {
                    return row;
                }
                let max = if i == 0 { (r - (l + 2.0)).max(0.0) } else { cw };
                let shown = truncate(measure, font, s, max);
                row.child(cols.cell(i, &shown, cy, font, color, measure))
            };
            row = cell(row, 0, &item.symbol, "12px sans-serif", &t.item_text, measure);
            let mut k = 1;
            if c.show_exchange {
                row = cell(row, k, &mlc_core::venue_label(&item.exchange), "10px sans-serif", &t.item_text_muted, measure);
                k += 1;
            }
            if c.show_account_type {
                let at = if item.account_type.is_empty() { "S" } else { item.account_type.as_str() };
                row = cell(row, k, at, "10px sans-serif", &t.item_text_muted, measure);
                k += 1;
            }
            // A failing row says why across the rest of the row, in place of its
            // price, change and spark (the app's `row_status`).
            let status = item.status.as_deref();
            let preempt = status.is_some() && c.show_last_price;
            if c.show_last_price {
                if let Some(st) = status {
                    let (l, _) = cols.clip(k);
                    let span = (cols.clip(cols.n().saturating_sub(1)).1 - l - 4.0).max(0.0);
                    if span >= 1.0 {
                        let shown = truncate_words(measure, data_font, st, span);
                        row = row.child(panels::mid_text(&shown, l + 2.0, cy, panels::At::L, data_font, &t.danger, measure));
                    }
                } else {
                    row = cell(row, k, &format_price(item.last_price), data_font, &t.item_text, measure);
                }
                k += 1;
            }
            if c.show_change_pct {
                if !preempt {
                    row = cell(row, k, &format!("{:+.1}%", item.change_percent), data_font, chg_color(t, item.change_percent), measure);
                }
                k += 1;
            }
            if c.show_spark {
                let (l, r) = cols.clip(k);
                let cw = (r - l).max(0.0);
                if cw >= 14.0 && item.spark.len() >= 2 && !preempt {
                    let sw = (cw - 6.0).min(64.0);
                    row = row.child(El::new("div.s-abs").css(format!("left: {}px; top: {}px; width: {}px; height: {}px", l + 2.0 - SPARK_BLEED, 9.0 - SPARK_BLEED, sw + 2.0 * SPARK_BLEED, ROW_H - 18.0 + 2.0 * SPARK_BLEED)).slot(format!("{SPARK}{i}")));
                } else if cw >= 1.0 && item.spark.len() < 2 && !c.show_last_price {
                    if let Some(st) = status {
                        let shown = truncate_words(measure, data_font, st, cw);
                        let (x, at) = match cols.align(k) {
                            Align::L => (l + 2.0, panels::At::L),
                            Align::C => ((l + r) / 2.0, panels::At::C),
                            Align::R => (r - 2.0, panels::At::R),
                        };
                        row = row.child(panels::mid_text(&shown, x, cy, at, data_font, &t.danger, measure));
                    }
                }
                k += 1;
            }
            if c.show_change_abs {
                let abs = item.last_price - (item.last_price / (1.0 + item.change_percent / 100.0));
                row = cell(row, k, &format_price(abs), data_font, chg_color(t, item.change_percent), measure);
                k += 1;
            }
            if c.show_high_low {
                row = cell(row, k, &format_price_compact(item.high_24h), data_font, &t.item_text_muted, measure);
                row = cell(row, k + 1, &format_price_compact(item.low_24h), data_font, &t.item_text_muted, measure);
                k += 2;
            }
            if c.show_volume {
                row = cell(row, k, &format_volume(item.volume_24h), data_font, &t.item_text_muted, measure);
            }
            if hovered {
                let dx = content_w - DEL_PAD - DEL - BTN_PAD;
                let dy = (ROW_H - DEL) / 2.0 - BTN_PAD;
                let spec = if w.hov(&del_id) { "div.s-del.hov" } else { "div.s-del" };
                row = row.child(El::new(spec).id(del_id).css(format!("left: {dx}px; top: {dy}px")).child(El::new("div.s-tint")).child(icon(Icon::Close, DEL)));
            }
            view = view.child(row);
        }
        // Drag-reorder: the dragged row tinted, a 2 px line where it would land.
        if let Some(d) = s.watchlist_drag_index {
            let mut marks = El::new("div.s-abs").css("left: 0px; top: 0px; width: 0px; height: 0px");
            marks = marks.child(fill(0.0, d as f64 * ROW_H, content_w, ROW_H, &panels::hexa("#4a9eff", "33")));
            if let Some(drop) = s.watchlist_drop_index {
                let y = drop as f64 * ROW_H - 1.0;
                marks = marks.child(fill(cols.area_left, y, cols.area_right - cols.area_left, 2.0, "#4a9eff"));
                // The app strokes it with the round caps its sparklines left set:
                // a half disc past each end, ~0.82 of a pixel per row.
                for x in [cols.area_left - 1.0, cols.area_right] {
                    marks = marks.child(fill(x, y, 1.0, 2.0, &panels::alpha("#4a9eff", 0.815)));
                }
            }
            view = view.child(marks);
        }
    }
    root = root.child(view);
    if w.content_h() > view_h + 0.5 {
        root = root.child(El::new("div.s-scroll").css(format!("left: {content_w}px; height: {view_h}px")).slot(SCROLLBAR));
    }
    if let Some((row_idx, px, py)) = s.watchlist_color_picker_open {
        root = root.child(color_picker(w, row_idx, px - w.app_rect.x, py - w.app_rect.y, view_h));
    }

    // Ticker footer.
    let total = w.metrics_h();
    if total > 0.0 {
        let y0 = rh - total;
        let mut foot = El::new("div.s-foot").css(format!("top: {y0}px; width: {rw}px; height: {total}px"));
        if let Some(tk) = &s.ticker_section {
            let counters = counters_block_h(w.columns.show_market_counters, &s.counter_rows);
            let h = (total - counters).max(72.0);
            let y = counters;
            let up = tk.change_pct >= 0.0;
            let col = if up { &t.success } else { &t.danger };
            let at = if tk.account_type.is_empty() { "S" } else { &tk.account_type };
            if counters > 0.0 {
                foot = foot.children(counters_block(w, measure));
            }
            foot = foot.child(text(&format!("{} · {} · {}", tk.symbol, tk.exchange.to_uppercase(), at), 10.0, rw, y + 14.0, Align::L, "10px sans-serif", &t.item_text_muted));
            foot = foot.child(text(&format!("{} {:.2}", if up { "▲" } else { "▼" }, tk.last_price), 10.0, rw, y + 38.0, Align::L, "bold 22px sans-serif", col));
            let sign = if up { "+" } else { "" };
            foot = foot.child(text(&format!("{sign}{:.2}% · {sign}{:.2}", tk.change_pct, tk.change_abs), 10.0, rw, y + 58.0, Align::L, "11px monospace", col));
            if tk.spark.len() >= 2 {
                foot = foot.child(El::new("div.s-abs").css(format!("left: {}px; top: {}px; width: {}px; height: {}px", 8.0 - SPARK_BLEED, y + 66.0 - SPARK_BLEED, rw - 16.0 + 2.0 * SPARK_BLEED, h - 72.0 + 2.0 * SPARK_BLEED)).slot(TICKER_SPARK));
            }
        }
        root = root.child(foot);
        let hov = w.hov("watchlist_metrics_sep");
        root = root.child(fill(0.0, y0, rw, if hov { 2.0 } else { 1.0 }, if hov { "#4a9eff" } else { t.separator.as_str() }));
        root = root.child(El::new("div.s-hit").id("watchlist_metrics_sep").css(format!("left: 0px; top: {}px; width: {rw}px; height: 7px", y0 - 3.0)));
    }
    if s.watchlist_config_dropdown_open {
        root = root.child(column_menu(w, measure));
    }
    root
}

/// The app's `truncate_to_width_word_boundary`: cut at the last whole word that
/// fits with a trailing "…".
fn truncate_words(measure: &mut dyn FnMut(&str, &str) -> f64, font: &str, text: &str, max: f64) -> String {
    if measure(font, text) <= max {
        return text.to_string();
    }
    let avail = max - measure(font, "\u{2026}");
    if avail <= 0.0 {
        return String::new();
    }
    let mut t = text.to_string();
    while !t.is_empty() && measure(font, &t) > avail {
        t.pop();
    }
    if let Some(sp) = t.trim_end().rfind(char::is_whitespace) {
        t.truncate(sp);
    }
    format!("{}\u{2026}", t.trim_end())
}

/// `render_watchlist_counters_block`: funding and OI change per venue, above the
/// ticker in the footer (footer coordinates).
fn counters_block(w: &Sidebar<'_>, measure: &mut dyn FnMut(&str, &str) -> f64) -> Vec<El> {
    use mlc_app_sidebar_content::render::{COUNTERS_CHROME_H, COUNTERS_ROW_H};
    use panels::{mid_text, At};
    let t = w.theme;
    let rw = w.rect.width;
    let (left, right) = (10.0, rw - 10.0);
    let (show_next, show_24h) = (rw >= 240.0, rw >= 200.0);
    let mut headers = vec!["VENUE", "FUND"];
    if show_next {
        headers.push("NEXT");
    }
    headers.push("OI 1H");
    if show_24h {
        headers.push("OI 24H");
    }
    let col_w = (right - left) / headers.len() as f64;
    let col_x = |i: usize| left + col_w * i as f64;
    let mut out = Vec::new();
    for (i, h) in headers.iter().enumerate() {
        out.push(mid_text(h, col_x(i), 8.0, At::L, "9px sans-serif", &t.item_text_muted, measure));
    }
    let font = "10px monospace";
    let pct = |v: Option<f64>| match v {
        Some(v) => (format!("{}{:.2}%", if v >= 0.0 { "+" } else { "" }, v), if v >= 0.0 { &t.success } else { &t.danger }),
        None => ("\u{2014}".to_string(), &t.item_text_muted),
    };
    for (r, row) in w.state.counter_rows.iter().enumerate() {
        let y = COUNTERS_CHROME_H - 4.0 + COUNTERS_ROW_H * r as f64 + COUNTERS_ROW_H / 2.0;
        let mut cells: Vec<(String, &String)> = vec![(row.venue.chars().take(3).collect::<String>().to_uppercase(), &t.item_text_muted)];
        cells.push(match row.funding_rate {
            Some(rate) => {
                let p = rate * 100.0;
                (format!("{p:.4}%"), if p >= 0.0 { &t.success } else { &t.danger })
            }
            None => ("\u{2014}".to_string(), &t.item_text_muted),
        });
        if show_next {
            let s = match row.countdown_secs {
                Some(secs) => {
                    let secs = secs.max(0);
                    format!("-{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
                }
                None => "\u{2014}".to_string(),
            };
            cells.push((s, &t.item_text_muted));
        }
        cells.push(pct(row.d1h_pct));
        if show_24h {
            cells.push(pct(row.d24h_pct));
        }
        for (i, (s, c)) in cells.iter().enumerate() {
            out.push(mid_text(s, col_x(i), y, At::L, font, c, measure));
        }
    }
    out
}

/// The colour-flag popup under a row (`watchlist_color_picker_open`), clipped to the
/// rows' viewport as the app draws it inside the scroll clip; sidebar coordinates.
fn color_picker(w: &Sidebar<'_>, row_idx: usize, x: f64, y: f64, view_h: f64) -> El {
    const COLORS: [&str; 8] = ["#ef5350", "#f59e0b", "#22c55e", "#3b82f6", "#a855f7", "#ec4899", "#6b7280", ""];
    let (sw, gap, pad) = (20.0, 4.0, 6.0);
    let pw = COLORS.len() as f64 * (sw + gap) - gap + pad * 2.0;
    let ph = sw + pad * 2.0;
    let t = w.theme;
    let vt = HEADER_H + COL_HEADER_H + 1.0;
    let (x, y) = (x, y - vt);
    let mut clip = El::new("div.s-clip").css(format!("left: 0px; top: {vt}px; width: {}px; height: {view_h}px", w.rect.width - SCROLLBAR_W));
    clip = clip.child(fill(x, y, pw, ph, &t.dropdown_bg));
    clip = clip.children(panels::checkbox_outline(x, y, pw, ph, &t.separator));
    for (i, c) in COLORS.iter().enumerate() {
        let sx = x + pad + i as f64 * (sw + gap);
        let sy = y + pad;
        let id = format!("watchlist_color_{row_idx}_{i}");
        if c.is_empty() {
            // The ×: the app's two 1.5 px strokes, painted by the host ([`SWATCH_X`]).
            clip = clip.child(
                El::new("div.s-abs").id(id).css(format!("left: {sx}px; top: {sy}px; width: {sw}px; height: {sw}px; background: {}", t.item_text_muted)).child(
                    El::new("div.s-abs").css(format!("left: 0px; top: 0px; width: {sw}px; height: {sw}px")).slot(SWATCH_X),
                ),
            );
        } else {
            clip = clip.child(fill(sx, sy, sw, sw, c).id(id));
        }
    }
    clip
}

/// `render_watchlist_config_dropdown`: the column checkboxes under the header,
/// right-aligned; sidebar coordinates.
fn column_menu(w: &Sidebar<'_>, measure: &mut dyn FnMut(&str, &str) -> f64) -> El {
    let t = w.theme;
    let c = w.columns;
    let (row_h, pad, cb, gap, mw, sep_h) = (28.0, 10.0, 10.0, 8.0, 180.0, 8.0);
    let options: [(&str, &str, bool); 11] = [
        ("show_exchange", "Exchange", c.show_exchange),
        ("show_account_type", "Type", c.show_account_type),
        ("show_last_price", "Last Price", c.show_last_price),
        ("show_change_pct", "Change %", c.show_change_pct),
        ("show_spark", "Sparkline", c.show_spark),
        ("show_change_abs", "Change", c.show_change_abs),
        ("show_volume", "Volume", c.show_volume),
        ("show_high_low", "High / Low", c.show_high_low),
        ("show_ticker", "Metrics panel", c.show_ticker_section),
        ("show_counters", "Fund/OI", c.show_market_counters),
        ("show_align_columns", "Align columns", c.align_columns),
    ];
    let mh = row_h * options.len() as f64 + pad * 2.0 + sep_h;
    let (x, y) = (w.rect.width - mw - 4.0, HEADER_H);
    let mut menu = El::new("div.s-abs").css("left: 0px; top: 0px; width: 0px; height: 0px");
    menu = menu.child(fill(x, y, mw, mh, &t.background).id("watchlist_cfg_backdrop"));
    menu = menu.children(panels::checkbox_outline(x, y, mw, mh, &t.separator));
    let last = options.len() - 1;
    for (i, (field, label, on)) in options.iter().enumerate() {
        let ry = y + pad + i as f64 * row_h + if i >= last { sep_h } else { 0.0 };
        if i == last {
            let ly = ry - sep_h / 2.0;
            menu = menu.child(panels::hline(x + pad, x + mw - pad, ly, &t.separator));
            // Its round caps: a half disc of radius ½ past each end.
            for cx in [x + pad - 1.0, x + mw - pad] {
                menu = menu.child(fill(cx, ly - 1.0, 1.0, 2.0, &panels::alpha(&t.separator, 0.196)));
            }
        }
        let id = format!("watchlist_cfg:{field}");
        let bg = if w.hov(&id) { format!("; background: {}", t.item_bg_hover) } else { String::new() };
        menu = menu.child(El::new("div.s-hit").id(id).css(format!("left: {x}px; top: {ry}px; width: {mw}px; height: {row_h}px{bg}")));
        let (cx, cy) = (x + pad, ry + (row_h - cb) / 2.0);
        menu = menu.children(panels::checkbox_outline(cx, cy, cb, cb, &t.item_text_muted));
        if *on {
            menu = menu.child(fill(cx + 1.0, cy + 1.0, cb - 2.0, cb - 2.0, "#4a9eff"));
        }
        menu = menu.child(panels::mid_text(label, cx + cb + gap, ry + row_h / 2.0, panels::At::L, "12px sans-serif", &t.item_text, measure));
    }
    menu
}

/// The root in its frame: the backdrop slot, the root, the border's outer pixel.
fn framed(root: El, w: &Sidebar<'_>) -> El {
    let t = w.theme;
    let (rw, rh) = (w.rect.width, w.rect.height);
    // The left border: the app strokes 1 px on the edge, half a pixel each side at
    // half cover, over its blurred backdrop, which reaches the pixel left of the
    // edge; the tree starts a pixel early for it ([`frame`]).
    El::new("div.s-frame")
        .css(format!("position: relative; display: flex; padding-left: 1px; padding-top: {OUT}px; width: {}px; height: {}px", rw + 1.0, rh + OUT))
        // The app's frosted backdrop first, over the box; what it smears beyond the
        // box stays (the frame reaches above it for that).
        .child(El::new("div.s-fill").slot(BLUR).css(format!("left: 0px; top: 0px; width: {}px; height: {}px; background: transparent", rw + 1.0, rh + OUT)))
        .child(root)
        // Last: something in the tree reaches a pixel left of its box.
        .child(El::new("div.s-fill").css(format!("left: 0px; top: {OUT}px; width: 1px; height: {rh}px; background: {}", mlc_core::mix_colors(&t.background, &t.separator, 0.5))))
}

/// The DOM rect for the sidebar at `rect` (window coordinates): one pixel wider on
/// the left, for its border, and [`OUT`] taller above, for its backdrop's smear.
pub fn frame(rect: Rect) -> Rect {
    Rect::new(rect.x - 1.0, rect.y - OUT, rect.width + 1.0, rect.height + OUT)
}
