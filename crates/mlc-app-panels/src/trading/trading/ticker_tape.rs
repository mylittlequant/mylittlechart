//! TickerTape — one-row scrolling ticker strip (owner spec 2026-07-28,
//! TradingView-style tape): fed from the WATCHLIST, tickers flow left with
//! live price / day-change updates. Hosted by the trading container as the
//! bottom-most strip (under the trading-metrics row).
//!
//! Interaction (owner spec 2026-07-28, round 2):
//! - entries print in the accepted instrument form `SYMBOL · EXCHANGE · AT`
//!   (same grammar the symbol searcher uses), with a Full/Short format
//!   setting — configured via the container's COMMON gear modal;
//! - hovering the strip pauses the marquee; dragging scrolls it by hand
//!   (`tape:body` drag, offset in state);
//! - hovering also reveals a gear at the right edge (`tape:gear`) — the
//!   app layer opens the container's common settings modal on click.

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

/// How a tape entry prints its instrument identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TapeFormat {
    /// `SYMBOL · EXCHANGE · AT` — the symbol-searcher grammar.
    #[default]
    Full,
    /// Symbol only.
    Short,
}

/// One tape entry — a live watchlist row projection.
#[derive(Clone, Debug)]
pub struct TapeItem {
    pub symbol: String,
    pub exchange: String,
    pub account_type: String,
    pub price: f64,
    pub change_pct: f64,
}

/// TickerTape panel state.
#[derive(Clone, Debug, Default)]
pub struct TickerTapeState {
    /// Live rows, watchlist order. Replaced wholesale each tick.
    pub items: Vec<TapeItem>,
    /// Entry identity format — common-gear-modal setting.
    pub format: TapeFormat,
    /// Marquee offset (px into the cyclic sequence). `Cell` because
    /// `render()` advances it from wall-clock while taking `&self`.
    pub offset: std::cell::Cell<f64>,
    /// Wall-clock of the last render advance (ms).
    last_ms: std::cell::Cell<f64>,
    /// True while a `tape:body` drag owns the offset (pauses the clock).
    pub drag_active: bool,
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
/// Width of the hover-revealed gear zone at the right edge, px.
const TAPE_GEAR_W: f64 = 22.0;

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
        coordinator: &mut uzor::InputCoordinator,
        slot_prefix: &str,
    ) {
        use uzor::input::Sense;

        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        let body_id = format!("{}:tape:body", slot_prefix);
        let gear_id = format!("{}:tape:gear", slot_prefix);
        let hovered = coordinator.is_hovered(&uzor::WidgetId::from(body_id.as_str()))
            || coordinator.is_hovered(&uzor::WidgetId::from(gear_id.as_str()));
        coordinator.register(
            body_id.as_str(),
            uzor::Rect::new(x as f64, y as f64, w as f64, h as f64),
            Sense::CLICK | Sense::HOVER | Sense::DRAG,
        );

        let mid_y = (y + h / 2.0) as f64;

        if self.items.is_empty() {
            ctx.set_fill_color(&theme.text_muted);
            ctx.set_font("9px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("Watchlist is empty", (x + 8.0) as f64, mid_y);
            return;
        }

        // Pre-measure every entry, then a cyclic draw loop.
        ctx.set_font("10px monospace");
        struct Seg {
            name: String,
            name_w: f64,
            price: String,
            price_w: f64,
            change: String,
            change_w: f64,
            up: bool,
        }
        let segs: Vec<Seg> = self.items.iter().map(|it| {
            let name = match self.format {
                TapeFormat::Short => it.symbol.clone(),
                TapeFormat::Full => {
                    let mut n = it.symbol.clone();
                    if !it.exchange.is_empty() {
                        n.push_str(" \u{00b7} ");
                        n.push_str(&it.exchange.to_uppercase());
                    }
                    if !it.account_type.is_empty() {
                        n.push_str(" \u{00b7} ");
                        n.push_str(&it.account_type);
                    }
                    n
                }
            };
            let price = format_price(it.price);
            let change = format!("{:+.2}%", it.change_pct);
            Seg {
                name_w: ctx.measure_text(&name),
                price_w: ctx.measure_text(&price),
                change_w: ctx.measure_text(&change),
                name,
                price,
                change,
                up: it.change_pct >= 0.0,
            }
        }).collect();
        let seg_w = |s: &Seg| s.name_w + 6.0 + s.price_w + 6.0 + s.change_w + TAPE_GAP;
        let total_w: f64 = segs.iter().map(seg_w).sum();
        if total_w <= 0.0 {
            return;
        }

        // Advance the marquee from wall-clock — paused while hovered or
        // hand-dragged; drags may have pushed the offset out of range, so
        // normalize either way.
        let now_ms = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as f64;
        let dt = (now_ms - self.last_ms.get()).clamp(0.0, 200.0);
        self.last_ms.set(now_ms);
        let mut offset = self.offset.get();
        if !hovered && !self.drag_active {
            offset += dt * TAPE_SPEED / 1000.0;
        }
        offset = offset.rem_euclid(total_w);
        self.offset.set(offset);

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
                    ctx.fill_text(&seg.name, cx, mid_y);
                    ctx.set_fill_color(&theme.text_muted);
                    ctx.fill_text(&seg.price, cx + seg.name_w + 6.0, mid_y);
                    ctx.set_fill_color(if seg.up { &theme.buy_bright } else { &theme.sell_bright });
                    ctx.fill_text(&seg.change, cx + seg.name_w + 6.0 + seg.price_w + 6.0, mid_y);
                }
                cx += sw;
                if cx > right {
                    break 'outer;
                }
            }
        }
        ctx.restore();

        // Hover-revealed gear at the right edge — opens the container's
        // common settings modal (routed in the app layer).
        if hovered {
            let gx = (x + w) as f64 - TAPE_GEAR_W;
            ctx.set_fill_color(&theme.header_bg);
            ctx.fill_rect(gx, y as f64, TAPE_GEAR_W, h as f64);
            let gear_hov = coordinator.is_hovered(&uzor::WidgetId::from(gear_id.as_str()));
            ctx.set_fill_color(if gear_hov { &theme.text_primary } else { &theme.text_muted });
            ctx.set_font("12px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("\u{2699}", gx + TAPE_GEAR_W / 2.0, mid_y);
            coordinator.register(
                gear_id.as_str(),
                uzor::Rect::new(gx, y as f64, TAPE_GEAR_W, h as f64),
                Sense::CLICK | Sense::HOVER,
            );
        }
    }

    fn handle_click(&mut self, _local_id: &str, _x: f64, _y: f64) -> bool { false }

    fn handle_drag_start(&mut self, local_id: &str, _x: f64, _y: f64) -> bool {
        if local_id == "tape:body" {
            self.drag_active = true;
            true
        } else {
            false
        }
    }

    fn handle_drag_move(&mut self, local_id: &str, dx: f64, _dy: f64) -> bool {
        if local_id == "tape:body" {
            // Content follows the cursor: dragging right rewinds the tape.
            self.offset.set(self.offset.get() - dx);
            true
        } else {
            false
        }
    }

    fn handle_drag_end(&mut self, local_id: &str) -> bool {
        if local_id == "tape:body" {
            self.drag_active = false;
            true
        } else {
            false
        }
    }
}
