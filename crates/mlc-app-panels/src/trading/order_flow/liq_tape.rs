//! Liquidation tape (plan D, owner spec 2026-08-01: «под LIQ сделаешь
//! кнопку тоже под BT, пусть текут лентой»).
//!
//! All-venue merged stream of forced closes in the BT-Aggregated grammar:
//! ONE column, newest at top, per-print venue identity in a trailing
//! EXCHANGE column. Liquidations are a futures-market concept — the
//! roster is every known venue's Futures feed; venues without a
//! liquidation stream simply never deliver (the feed self-heal keeps a
//! muted retry with backoff, same as every other tape).
//!
//! Feed: per-venue `SharedTradeSeries` rings filled by the bridge's
//! liquidation forwarder (`subscribe_liquidations` — same
//! `Series<TradePoint>` shape as trades; `side` = the forced order's
//! side, so side 0 (forced sell) = LONG position liquidated).

use serde::{Serialize, Deserialize};
use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;

use digdigdig3_station::data::LiquidationPoint;
use digdigdig3_station::series::Series as StationSeries;
use tokio::sync::RwLock as TokioRwLock;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

/// Handle to the station's `Series<LiquidationPoint>` ring for one
/// (exchange, symbol, account_type) — same shape as `SharedTradeSeries`,
/// distinct point type (33-byte compact liquidation print).
pub type SharedLiquidationSeries = Arc<TokioRwLock<StationSeries<LiquidationPoint>>>;

/// LiqTape panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LiqTapeId(pub u64);

/// Which side got liquidated (derived from the forced order's side).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LiqSide {
    /// Forced SELL — a long position was blown out.
    Long,
    /// Forced BUY — a short position was blown out.
    Short,
}

/// One liquidation print.
#[derive(Clone, Debug)]
pub struct LiqPrint {
    pub timestamp: i64,
    pub price: f64,
    pub quantity: f64,
    pub side: LiqSide,
    /// Local wall-clock at ingest (ms) — drives the new-row flash.
    pub arrival_ms: u64,
    /// Roster index of the source venue — drives the EXCHANGE column.
    pub venue: Option<u8>,
}

/// Liquidation tape panel state — all-venue aggregated by construction.
#[derive(Clone)]
pub struct LiqTapeState {
    /// Base symbol shared by every roster venue.
    pub symbol: String,
    /// Bound venue identity (persistence source only — the tape itself is
    /// always all-venue).
    pub exchange: String,
    pub account_type: String,
    /// Merged threshold-passing ring, newest at back.
    pub liqs: VecDeque<LiqPrint>,
    /// Minimum base-qty to show (0 = all — liquidations are sparse).
    pub size_threshold: f64,
    /// Minimum notional to show (`None` = all).
    pub notional_threshold: Option<f64>,
    /// Manual skin pick — `None` follows the theme-skin default.
    pub skin: Option<super::trade_tape::TapeSkin>,
    /// Tick size from a linked DOM (price formatting).
    pub dom_tick_size: Option<f64>,
    /// Scroll offset (ring scroll — pushes compensate, no pause).
    pub scroll_offset: f64,
    /// Largest quantity in the ring — size-bar scaling.
    pub max_qty_seen: f64,
    /// Roster + per-venue liq-series handles/cursors (`trade_handles` /
    /// `trade_cursors` carry the liquidation rings here).
    pub multi: super::multi_tape::MultiTapeState,
}

impl fmt::Debug for LiqTapeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LiqTapeState")
            .field("symbol", &self.symbol)
            .field("liqs_len", &self.liqs.len())
            .field("roster", &self.multi.entries)
            .finish()
    }
}

impl LiqTapeState {
    pub fn new() -> Self {
        Self {
            symbol: String::new(),
            exchange: String::new(),
            account_type: String::new(),
            liqs: VecDeque::new(),
            size_threshold: 0.0,
            notional_threshold: None,
            skin: None,
            dom_tick_size: None,
            scroll_offset: 0.0,
            max_qty_seen: 0.0,
            multi: super::multi_tape::MultiTapeState::new(),
        }
    }

    /// Seed/refresh the all-venue Futures roster for `symbol`. Idempotent —
    /// called on spawn and by the late symbol sync (BoundToChart panels
    /// hydrate with an empty symbol).
    pub fn ensure_roster(&mut self, symbol: &str) {
        if !self.multi.symbol.eq_ignore_ascii_case(symbol) {
            self.multi.reset_for_symbol(symbol);
            self.liqs.clear();
            self.scroll_offset = 0.0;
            self.max_qty_seen = 0.0;
        }
        for ex in super::multi_dom::KNOWN_EXCHANGES {
            self.multi.add_entry(&super::multi_dom::col_entry(ex, "F"));
        }
    }

    /// Merged pull: drain every roster handle past its own cursor, apply
    /// thresholds per print, tag with the venue index, merge time-sorted.
    pub fn tick(&mut self) {
        const MAX_LIQS: usize = 2000;
        self.multi.sync_len();
        let arrival_ms = web_time::SystemTime::now()
            .duration_since(web_time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let mut merged: Vec<LiqPrint> = Vec::new();
        for i in 0..self.multi.entries.len() {
            let Some(handle) = self.multi.liq_handles[i].clone() else { continue };
            let guard = handle.blocking_read();
            let snapshot = guard.snapshot();
            drop(guard);
            let cursor = self.multi.trade_cursors[i];
            let mut new_max_ts = cursor;
            for p in snapshot.iter().filter(|p| p.ts_ms > cursor) {
                if p.ts_ms > new_max_ts {
                    new_max_ts = p.ts_ms;
                }
                if p.quantity < self.size_threshold {
                    continue;
                }
                if let Some(notional_min) = self.notional_threshold {
                    if p.price * p.quantity < notional_min {
                        continue;
                    }
                }
                merged.push(LiqPrint {
                    timestamp: p.ts_ms,
                    price: p.price,
                    quantity: p.quantity,
                    // MLC's shipped side convention (liq-heatmap chain,
                    // consistent bridge→core→render): side==0 → SHORT
                    // liquidated, side==1 → LONG. The dig3-core struct
                    // doc states the opposite and is self-contradictory —
                    // do NOT re-derive from it (research doc §4,
                    // liq-feed-path-2026-08-01.md).
                    side: if p.side == 0 { LiqSide::Short } else { LiqSide::Long },
                    arrival_ms,
                    venue: Some(i as u8),
                });
            }
            self.multi.trade_cursors[i] = new_max_ts;
        }
        if merged.is_empty() {
            return;
        }
        merged.sort_by_key(|t| t.timestamp);
        for print in merged {
            if print.quantity > self.max_qty_seen {
                self.max_qty_seen = print.quantity;
            }
            if self.liqs.len() >= MAX_LIQS {
                self.liqs.pop_front();
            }
            self.liqs.push_back(print);
            // Ring scroll — scrolled-back view stays anchored, no pause.
            if self.scroll_offset > 0.0 {
                self.scroll_offset += 1.0;
            }
        }
    }

    pub fn handle_scroll(&mut self, delta: f64) {
        let raw = self.scroll_offset + delta * 30.0;
        let max_offset = self.liqs.len().saturating_sub(1) as f64;
        self.scroll_offset = raw.clamp(0.0, max_offset);
    }

    pub fn handle_double_click(&mut self) {
        self.scroll_offset = 0.0;
    }

    pub fn handle_drag(&mut self, _dx: f64, dy: f64) {
        let raw = self.scroll_offset - dy;
        let max_offset = self.liqs.len().saturating_sub(1) as f64;
        self.scroll_offset = raw.clamp(0.0, max_offset);
    }

    fn visible(&self, max_count: usize) -> Vec<&LiqPrint> {
        let skip = (self.scroll_offset as usize).min(self.liqs.len());
        self.liqs.iter().rev().skip(skip).take(max_count).collect()
    }

    fn size_bar_width(&self, print: &LiqPrint, max_width: f32) -> f32 {
        if self.max_qty_seen == 0.0 {
            return 0.0;
        }
        (print.quantity / self.max_qty_seen).clamp(0.0, 1.0) as f32 * max_width
    }
}

fn format_timestamp(ts: i64) -> String {
    let secs = (ts / 1000) % 86400;
    format!("{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
}

const LIQ_HEADER_HEIGHT: f32 = 18.0;
const LIQ_ROW_HEIGHT: f32 = 20.0;
const LIQ_LEFT_PAD: f32 = 6.0;

impl TradingPanel for LiqTapeState {
    fn kind(&self) -> &'static str { "liq_tape" }
    fn label(&self) -> &'static str { "Liquidations" }

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
        {
            let body_id = format!("{}:liqtape:body", slot_prefix);
            coordinator.register(
                body_id.as_str(),
                uzor::Rect::new(x as f64, y as f64, w as f64, h as f64),
                uzor::input::Sense::SCROLL | uzor::input::Sense::DRAG | uzor::input::Sense::DOUBLE_CLICK,
            );
        }
        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        // ── column layout (BT-Agg grammar: EXCHANGE closes the row) ──────
        struct ColDef {
            label: &'static str,
            width: f32,
        }
        let cols = [
            ColDef { label: "TIME",     width: 60.0 },
            ColDef { label: "SIDE",     width: 42.0 },
            ColDef { label: "PRICE",    width: 80.0 },
            ColDef { label: "SIZE",     width: 70.0 },
            ColDef { label: "NOTIONAL", width: 76.0 },
            ColDef { label: "EXCHANGE", width: 0.0 },
        ];
        let usable_w = w - LIQ_LEFT_PAD * 2.0;
        let gap = 4.0_f32;
        let fixed_w: f32 = cols.iter().take(cols.len() - 1).map(|c| c.width + gap).sum();
        let last_w = (usable_w - fixed_w).max(0.0);
        let mut col_xs: Vec<f32> = Vec::with_capacity(cols.len());
        {
            let mut cx = x + LIQ_LEFT_PAD;
            for (idx, col) in cols.iter().enumerate() {
                col_xs.push(cx);
                let w_used = if idx + 1 == cols.len() { last_w } else { col.width };
                cx += w_used + gap;
            }
        }

        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, LIQ_HEADER_HEIGHT as f64);
        ctx.set_fill_color(&theme.text_header);
        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        let header_y = (y + LIQ_HEADER_HEIGHT / 2.0) as f64;
        for (idx, col) in cols.iter().enumerate() {
            ctx.fill_text(col.label, col_xs[idx] as f64, header_y);
        }

        let available_height = h - LIQ_HEADER_HEIGHT;
        let max_rows = (available_height / LIQ_ROW_HEIGHT) as usize;
        let prints = self.visible(max_rows);

        if prints.is_empty() {
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.set_fill_color(&theme.text_muted);
            let live = self
                .multi
                .liq_handles
                .iter()
                .filter(|h| h.is_some())
                .count();
            ctx.fill_text(
                &format!("Waiting for liquidations… ({} venue feeds live)", live),
                (x + w / 2.0) as f64,
                (y + h / 2.0) as f64,
            );
            return;
        }

        let bar_max_width = w - LIQ_LEFT_PAD * 2.0;
        let skin = super::trade_tape::TapeSkin::effective(self.skin, theme);
        let flat_bars = theme.style.bar_texture == mlc_core::theme::BarTexture::Flat;
        let now_ms = web_time::SystemTime::now()
            .duration_since(web_time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        for (i, print) in prints.iter().enumerate() {
            let row_y = y + LIQ_HEADER_HEIGHT + (i as f32 * LIQ_ROW_HEIGHT);
            let text_y = (row_y + LIQ_ROW_HEIGHT / 2.0) as f64;

            // Size bar underlay: LONG liq = sell token (longs blown out),
            // SHORT liq = buy token — the industry liq-tape convention.
            let ratio = if self.max_qty_seen > 0.0 {
                (print.quantity / self.max_qty_seen).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let t_mix = if flat_bars {
                0.0
            } else if skin == super::trade_tape::TapeSkin::Classic {
                ratio * 0.5
            } else {
                ratio
            };
            let (base, bright) = match print.side {
                LiqSide::Long => (&theme.sell, &theme.sell_bright),
                LiqSide::Short => (&theme.buy, &theme.buy_bright),
            };
            let bar_width = self.size_bar_width(print, bar_max_width);
            ctx.set_fill_color(&mlc_core::mix_colors(base, bright, t_mix));
            ctx.fill_rect(x as f64, row_y as f64, bar_width as f64, LIQ_ROW_HEIGHT as f64);
            if skin == super::trade_tape::TapeSkin::Ramp
                && now_ms.saturating_sub(print.arrival_ms) < 250
            {
                ctx.set_fill_color(&mlc_core::apply_opacity(&theme.accent, 0.22));
                ctx.fill_rect(x as f64, row_y as f64, w as f64, LIQ_ROW_HEIGHT as f64);
            }

            ctx.set_fill_color(&theme.text_primary);
            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);

            ctx.fill_text(&format_timestamp(print.timestamp), col_xs[0] as f64, text_y);

            let (side_str, side_color) = match print.side {
                LiqSide::Long => ("LONG", &theme.sell),
                LiqSide::Short => ("SHORT", &theme.buy),
            };
            ctx.set_fill_color(side_color);
            ctx.fill_text(side_str, col_xs[1] as f64, text_y);
            ctx.set_fill_color(&theme.text_primary);

            ctx.fill_text(&format!("{:.4}", print.price), col_xs[2] as f64, text_y);
            ctx.fill_text(&format!("{:.4}", print.quantity), col_xs[3] as f64, text_y);
            ctx.fill_text(
                &format!("{:.2}", print.price * print.quantity),
                col_xs[4] as f64,
                text_y,
            );

            let venue_label = print
                .venue
                .and_then(|v| self.multi.entries.get(v as usize))
                .map(|e| super::multi_dom::col_label(e))
                .unwrap_or_default();
            ctx.set_fill_color(&theme.text_muted);
            ctx.fill_text(&venue_label, col_xs[5] as f64, text_y);

            ctx.set_fill_color(&theme.separator);
            ctx.fill_rect(x as f64, (row_y + LIQ_ROW_HEIGHT - 1.0) as f64, w as f64, 1.0);
        }
    }

    fn handle_click(&mut self, _local_id: &str, _x: f64, _y: f64) -> bool { false }

    fn handle_scroll(&mut self, local_id: &str, _dx: f64, dy: f64) -> bool {
        if local_id == "liqtape:body" {
            LiqTapeState::handle_scroll(self, dy);
            true
        } else {
            false
        }
    }

    fn handle_drag_start(&mut self, local_id: &str, _x: f64, _y: f64) -> bool {
        local_id == "liqtape:body"
    }

    fn handle_drag_move(&mut self, local_id: &str, dx: f64, dy: f64) -> bool {
        if local_id == "liqtape:body" {
            LiqTapeState::handle_drag(self, dx, dy);
            true
        } else {
            false
        }
    }

    fn handle_drag_end(&mut self, _local_id: &str) -> bool {
        true
    }

    fn handle_double_click(&mut self, local_id: &str, _x: f64, _y: f64) -> bool {
        if local_id == "liqtape:body" {
            LiqTapeState::handle_double_click(self);
            true
        } else {
            false
        }
    }
}
