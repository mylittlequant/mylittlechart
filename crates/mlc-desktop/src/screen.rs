//! The desktop window as a DOM tree on uzor's desktop kit (`domkit`): a toolbar
//! (symbol, venue · timeframe · data state with a status dot, the timeframe
//! segmented control, reset), the chart in a paint slot, and a watchlist column
//! (search field + rows). `KIT_CSS` plus [`APP_CSS`]: greys only, opaque surfaces.

use uzor::ui::dom::{Edit, El};
use uzor::ui::domkit;

use mlc_chart_view::demo::TIMEFRAMES;
use mlc_chart_view::i18n::tf_label;

use crate::i18n::{t, K};

/// The chart's paint slot: key and hit id.
pub const CHART_SLOT: &str = "chart";
/// Timeframe segmented control: hits `tf:{i}`.
pub const TF_ID: &str = "tf";
/// The reset button.
pub const RESET_ID: &str = "reset";
/// The watchlist search field.
pub const SEARCH_ID: &str = "wl:q";
/// The watchlist rows: hits `wl:row:{i}` (index among the shown rows).
pub const LIST_ID: &str = "wl";
/// The watchlist's scroll box (its viewport).
pub const LIST_BOX: &str = "wl:box";
/// Watchlist column width, px.
pub const SIDE_W: f64 = 248.0;
/// Watchlist row height, px (fixed so the scroll math is exact).
pub const ROW_H: f64 = 30.0;

/// Mizu-like greys over the kit: the accent is light grey, selection a faint
/// lift, surfaces opaque.
pub const APP_CSS: &str = r#"
:root { --k-accent: #d6d6d6; --k-accent-hover: #e6e6e6; --k-on-accent: #121212; --k-bg: #141414; --k-panel: #1a1a1a;
        --k-fg: #e6e6e6; --k-muted: #8e8e8e; --k-line: #2a2a2a; --k-btn: #222222; --k-btn-hover: #2c2c2c;
        --k-field: #101010; --k-sunken: #161616; --k-hover: #232323; --k-hi: #2a2a2a; --k-sel: #262626;
        --k-track: #3a3a3a; --k-track-hover: #474747 }
div.d-root { display: flex; flex-direction: column; background: var(--k-bg); color: var(--k-fg); font-size: 13px }
div.d-bar { display: flex; align-items: center; gap: 12px; height: 44px; flex-shrink: 0; padding: 0 12px; background: var(--k-panel); border-bottom: 1px solid var(--k-line) }
div.d-sym { font-size: 15px; font-weight: 600; white-space: nowrap; flex-shrink: 0 }
div.d-status { display: flex; align-items: center; gap: 6px; color: var(--k-muted); white-space: nowrap; flex-shrink: 1; min-width: 0; overflow: hidden }
span.d-dot { width: 6px; height: 6px; border-radius: 3px; background: #4a4a4a; flex-shrink: 0 }
span.d-dot.on { background: #d6d6d6 }
div.d-gap { flex-grow: 1 }
div.d-main { display: flex; flex-grow: 1; min-height: 0 }
div.d-chart { flex-grow: 1; min-width: 0; background: #121212 }
div.d-side { display: flex; flex-direction: column; width: 248px; flex-shrink: 0; background: var(--k-panel); border-left: 1px solid var(--k-line) }
div.d-side-head { padding: 12px 12px 8px; color: var(--k-muted); font-size: 12px; font-weight: 600; flex-shrink: 0 }
div.d-search { padding: 0 8px 8px; flex-shrink: 0 }
div.d-list { flex-grow: 1; min-height: 0; overflow: hidden; padding: 0 4px }
div.d-list div.k-li { height: 30px; padding: 0 8px; display: flex; align-items: center; justify-content: space-between; border-radius: 6px }
div.d-list div.k-li-side { color: var(--k-muted); font-size: 12px }
div.d-empty { padding: 12px; color: var(--k-muted) }
"#;

/// One watchlist entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Sym {
    pub symbol: String,
    pub venue: String,
}

/// What the toolbar says.
#[derive(Clone, Debug)]
pub struct Header<'a> {
    pub symbol: &'a str,
    pub venue: &'a str,
    pub status: &'static str,
    pub live: bool,
}

/// What the watchlist column shows.
pub struct Side<'a> {
    pub query: &'a str,
    /// The rows shown (after the search), as indices into `all`.
    pub shown: &'a [usize],
    pub all: &'a [Sym],
    /// Index into `shown` of the chart's symbol, if shown.
    pub selected: Option<usize>,
    pub scroll: f64,
}

/// The window tree at timeframe `tf`.
pub fn tree(h: &Header<'_>, tf: usize, side: &Side<'_>) -> El {
    let labels: Vec<String> = TIMEFRAMES.iter().map(|(c, _)| tf_label(c)).collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let status = format!("{} · {} · {}", h.venue, tf_label(TIMEFRAMES[tf].0), h.status);
    let bar = El::new("div.d-bar").children([
        El::new("div.d-sym").text(h.symbol),
        El::new("div.d-status").child(El::new(if h.live { "span.d-dot.on" } else { "span.d-dot" })).child(El::new("span").text(status)),
        El::new("div.d-gap"),
        El::new("div").name(t(K::Timeframe)).child(domkit::segmented(TF_ID, &labels, tf)),
        El::new("button.k-btn.ghost").id(RESET_ID).tab().text(t(K::Reset)),
    ]);
    let rows: Vec<(&str, Option<&str>)> = side.shown.iter().map(|&i| (side.all[i].symbol.as_str(), Some(side.all[i].venue.as_str()))).collect();
    let list = if rows.is_empty() {
        El::new("div.d-list").id(LIST_BOX).child(El::new("div.d-empty").text(t(K::NoMatches)))
    } else {
        El::new("div.d-list").id(LIST_BOX).child(domkit::list(LIST_ID, &rows, side.selected)).scroll_top(side.scroll)
    };
    let col = El::new("div.d-side").children([
        El::new("div.d-side-head").text(t(K::Watchlist)),
        El::new("div.d-search").child(domkit::input(SEARCH_ID, Edit::new(side.query).placeholder(t(K::Search))).name(t(K::Search))),
        list,
    ]);
    El::new("div.k-root.d-root").css("height: 100%").child(bar).child(El::new("div.d-main").child(El::new("div.d-chart").paint_slot(CHART_SLOT).name(t(K::Chart))).child(col))
}

/// The timeframe a click on `id` picks.
pub fn picked_tf(id: &str) -> Option<usize> {
    domkit::picked(TF_ID, "", id).filter(|i| *i < TIMEFRAMES.len())
}

/// The shown-row index a click on `id` picks.
pub fn picked_row(id: &str) -> Option<usize> {
    domkit::picked(LIST_ID, "row", id)
}
