//! TickerTape — one-row scrolling ticker strip (owner spec 2026-07-28,
//! TradingView-style tape): fed from the WATCHLIST, tickers flow left with
//! live price / day-change updates. Hosted by the trading container as the
//! bottom-most strip (under the trading-metrics row).
//!
//! The marquee offset is derived from wall-clock inside `render()` — no
//! animation state to advance — so the strip animates as long as the host
//! keeps repainting (tick.rs marks the sidebar dirty every tick while the
//! Trading tab is visible).

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

/// One tape entry — a live watchlist row projection.
#[derive(Clone, Debug)]
pub struct TapeItem {
    pub symbol: String,
    pub price: f64,
    pub change_pct: f64,
}

/// TickerTape panel state.
#[derive(Clone, Debug, Default)]
pub struct TickerTapeState {
    /// Live rows, watchlist order. Replaced wholesale each tick.
    pub items: Vec<TapeItem>,
}

impl TickerTapeState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_items(&mut self, items: Vec<TapeItem>) {
        self.items = items;
    }
}

/// Marquee speed, px/s.
const TAPE_SPEED: f64 = 30.0;
/// Gap between entries, px.
const TAPE_GAP: f64 = 26.0;

fn format_price(p: f64) -> String {
    if p >= 100.0 {
        format!("{:.2}", p)
    } else if p >= 1.0 {
        format!("{:.4}", p)
    } else {
        format!("{:.6}", p)
    }
}

impl TradingPanel for TickerTapeState {
    fn kind(&self) -> &'static str { "ticker_tape" }
    fn label(&self) -> &'static str { "Tape" }

    fn render(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        theme: &crate::panel_theme::PanelTheme,
        _coordinator: &mut uzor::InputCoordinator,
        _slot_prefix: &str,
    ) {
        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        if self.items.is_empty() {
            ctx.set_fill_color(&theme.text_muted);
            ctx.set_font("9px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("Watchlist is empty", (x + 8.0) as f64, (y + h / 2.0) as f64);
            return;
        }

        let mid_y = (y + h / 2.0) as f64;

        // Pre-measure every entry: (symbol, sym_w, price, price_w, change,
        // change_w, up) — one measure pass, then a cyclic draw loop.
        ctx.set_font("10px monospace");
        struct Seg {
            symbol: String,
            sym_w: f64,
            price: String,
            price_w: f64,
            change: String,
            change_w: f64,
            up: bool,
        }
        let segs: Vec<Seg> = self.items.iter().map(|it| {
            let symbol = it.symbol.clone();
            let price = format_price(it.price);
            let change = format!("{:+.2}%", it.change_pct);
            Seg {
                sym_w: ctx.measure_text(&symbol),
                price_w: ctx.measure_text(&price),
                change_w: ctx.measure_text(&change),
                symbol,
                price,
                change,
                up: it.change_pct >= 0.0,
            }
        }).collect();
        let seg_w = |s: &Seg| s.sym_w + 6.0 + s.price_w + 6.0 + s.change_w + TAPE_GAP;
        let total_w: f64 = segs.iter().map(seg_w).sum();
        if total_w <= 0.0 {
            return;
        }

        let now_ms = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as f64;
        let offset = (now_ms * TAPE_SPEED / 1000.0) % total_w;

        ctx.save();
        ctx.clip_rect(x as f64, y as f64, w as f64, h as f64);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);

        // Cyclic draw: start one full sequence before the visible left edge
        // so entries slide in seamlessly, keep laying entries until past the
        // right edge.
        let mut cx = x as f64 - offset;
        let right = (x + w) as f64;
        'outer: loop {
            for seg in &segs {
                let sw = seg_w(seg);
                if cx + sw >= x as f64 {
                    ctx.set_font("10px monospace");
                    ctx.set_fill_color(&theme.text_primary);
                    ctx.fill_text(&seg.symbol, cx, mid_y);
                    ctx.set_fill_color(&theme.text_muted);
                    ctx.fill_text(&seg.price, cx + seg.sym_w + 6.0, mid_y);
                    ctx.set_fill_color(if seg.up { &theme.buy_bright } else { &theme.sell_bright });
                    ctx.fill_text(&seg.change, cx + seg.sym_w + 6.0 + seg.price_w + 6.0, mid_y);
                }
                cx += sw;
                if cx > right {
                    break 'outer;
                }
            }
        }
        ctx.restore();
    }

    fn handle_click(&mut self, _local_id: &str, _x: f64, _y: f64) -> bool { false }
}
