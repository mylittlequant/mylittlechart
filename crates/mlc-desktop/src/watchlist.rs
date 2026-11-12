//! The watchlist column's state: the symbols, the search, the scroll. Matching is
//! uzor's (`search_filter`: every word, case- and accent-insensitive).

use uzor::ui::mobile::search_filter;
use uzor::Rect;

use crate::screen::{Sym, ROW_H};

/// One wheel line, px (the view reports the wheel in lines).
const LINE_PX: f64 = ROW_H;

/// Watchlist state.
#[derive(Clone, Debug, Default)]
pub struct Watchlist {
    pub all: Vec<Sym>,
    pub query: String,
    /// Index into `all` of the symbol on the chart.
    pub current: Option<usize>,
    /// Scroll of the rows, px.
    pub scroll: f64,
    /// The rows' viewport on the last frame.
    pub list_rect: Option<Rect>,
}

impl Watchlist {
    /// `all`, the chart on the first.
    pub fn new(all: Vec<Sym>) -> Self {
        let current = if all.is_empty() { None } else { Some(0) };
        Self { all, current, ..Self::default() }
    }

    /// The demo watchlist.
    pub fn default_symbols() -> Vec<Sym> {
        let s = |symbol: &str, venue: &str| Sym { symbol: symbol.into(), venue: venue.into() };
        let mut v = Vec::new();
        for (venue, list) in [
            ("Binance", &["BTC/USDT", "ETH/USDT", "SOL/USDT", "BNB/USDT", "XRP/USDT", "DOGE/USDT", "ADA/USDT", "AVAX/USDT", "LINK/USDT", "DOT/USDT", "TON/USDT", "LTC/USDT"][..]),
            ("Bybit", &["BTC/USDT", "ETH/USDT", "SOL/USDT", "ARB/USDT", "OP/USDT", "SUI/USDT"][..]),
            ("OKX", &["BTC/USDT", "ETH/USDT", "APT/USDT", "NEAR/USDT", "ATOM/USDT", "TRX/USDT"][..]),
        ] {
            v.extend(list.iter().map(|sym| s(sym, venue)));
        }
        v
    }

    /// Rows shown for the current search (indices into `all`) and which of them is
    /// on the chart.
    pub fn shown(&self) -> (Vec<usize>, Option<usize>) {
        let labels: Vec<String> = self.all.iter().map(|s| format!("{} {}", s.symbol, s.venue)).collect();
        let hits = search_filter(&labels, &self.query);
        let sel = self.current.and_then(|c| hits.iter().position(|&i| i == c));
        (hits, sel)
    }

    /// A new search: back to the top.
    pub fn set_query(&mut self, q: &str) {
        if q != self.query {
            self.query = q.to_string();
            self.scroll = 0.0;
        }
    }

    /// The largest scroll for the shown rows.
    pub fn max_scroll(&self) -> f64 {
        let n = self.shown().0.len() as f64;
        let h = self.list_rect.map_or(0.0, |r| r.height);
        (n * ROW_H - h).max(0.0)
    }

    /// The wheel `(dx, dy)` in lines at `pointer` scrolls the rows when over them.
    pub fn wheel(&mut self, pointer: Option<(f64, f64)>, delta: (f64, f64)) {
        let (Some((x, y)), Some(r)) = (pointer, self.list_rect) else { return };
        if delta.1 == 0.0 || x < r.x || x >= r.x + r.width || y < r.y || y >= r.y + r.height {
            return;
        }
        let px = delta.1 * LINE_PX;
        // Wheel up (positive) shows earlier rows.
        self.scroll = (self.scroll - px).clamp(0.0, self.max_scroll());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_narrows_and_keeps_the_current_row() {
        let mut w = Watchlist::new(Watchlist::default_symbols());
        assert_eq!(w.shown().0.len(), 24);
        assert_eq!(w.shown().1, Some(0));
        w.set_query("eth");
        let (hits, sel) = w.shown();
        assert_eq!(hits.len(), 3, "ETH on three venues");
        assert_eq!(sel, None, "BTC is not among them");
        w.set_query("btc okx");
        assert_eq!(w.shown().0.len(), 1);
        w.set_query("zzz");
        assert!(w.shown().0.is_empty());
    }

    #[test]
    fn wheel_scrolls_only_over_the_rows_and_clamps() {
        let mut w = Watchlist::new(Watchlist::default_symbols());
        w.list_rect = Some(Rect::new(1000.0, 100.0, 248.0, 300.0));
        w.wheel(Some((500.0, 200.0)), (0.0, -3.0));
        assert_eq!(w.scroll, 0.0, "not over the list");
        w.wheel(Some((1100.0, 200.0)), (0.0, -3.0));
        assert!(w.scroll > 0.0);
        w.wheel(Some((1100.0, 200.0)), (0.0, -300.0));
        assert_eq!(w.scroll, 24.0 * ROW_H - 300.0, "clamped at the end");
        w.wheel(Some((1100.0, 200.0)), (0.0, 1000.0));
        assert_eq!(w.scroll, 0.0);
    }
}
