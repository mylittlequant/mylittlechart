//! Multi-exchange consolidated DOM panel.
//!
//! v2 (owner spec 2026-07-30, reference: Quantower-style Orderbook panel):
//! each exchange renders in its OWN column against a shared price ladder —
//! per-level depth bars form the book "wave" silhouette, big resting orders
//! are individually highlighted with a bright marker + quantity label, and
//! the bottom strip shows per-exchange bid/ask liquidity share within the
//! ±2% and ±10% windows around mid.
//!
//! Data path: NO delta events — each configured exchange gets a live
//! station `OrderBookTracker` handle (same shared-book source the single
//! DOM reads). `tick()` re-buckets the top of each book onto the common
//! grid every frame; handles are provisioned by the app layer's
//! feed-handle self-heal (`repair_panel_feed_handles`).
//!
//! # Editor popup — click-cycle pattern
//!
//! The in-panel config editor uses click-to-cycle for all three
//! dimensions (exchange add / grid / depth); the canonical config surface
//! is the DOM settings modal.

use std::collections::BTreeMap;

use mlc_core::theme::BarTexture;
use rust_decimal::prelude::ToPrimitive;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};
use crate::trading::order_flow::trade_tape::SharedOrderBookTracker;

/// Available exchanges for the cycle-add flow.
/// Owner adds by clicking; already-subscribed exchanges are skipped.
pub const KNOWN_EXCHANGES: &[&str] = &[
    "binance", "bybit", "okx", "bitget", "kucoin", "gateio",
];

/// Grid size (tick coalescence) presets, cycled by click.
pub const GRID_PRESETS: &[f64] = &[0.1, 0.5, 1.0, 5.0, 10.0];

/// Depth (rows per side) presets, cycled by click.
pub const DEPTH_PRESETS: &[usize] = &[10, 25, 50, 100];

/// Exchange identifier — opaque label so the panel doesn't pull a hard
/// dep on the `digdigdig3` crate. Wire up to `digdigdig3::ExchangeId`
/// at the app layer with `format!("{:?}", ex)`.
pub type ExchangeId = String;

/// Static configuration for a multi-exchange book.
#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct MultiDomConfig {
    pub exchanges: Vec<ExchangeId>,
    pub grid_size: f64,
    pub depth_per_side: usize,
    /// Bottom ±2% / ±10% bid-liquidity-share strip (owner toggle 2026-07-30).
    pub show_imbalance: bool,
}

impl Default for MultiDomConfig {
    fn default() -> Self {
        Self {
            exchanges: Vec::new(),
            grid_size: 0.5,
            depth_per_side: 25,
            show_imbalance: true,
        }
    }
}

/// Multi-exchange DOM state.
///
/// Buckets are keyed by `floor(price / grid)` as `i64` — float-exact
/// lookups between `tick()` and the renderer.
pub struct MultiDomState {
    pub symbol: String,
    pub config: MultiDomConfig,
    /// Live station book handles, parallel to `config.exchanges`.
    /// Filled by the app layer (`repair_panel_feed_handles`); cleared on
    /// symbol rebind so fresh handles get provisioned.
    pub handles: Vec<Option<SharedOrderBookTracker>>,
    /// Bucketed `(bids, asks)` per exchange — refreshed by [`Self::tick`].
    pub books_by_exchange: Vec<(BTreeMap<i64, f64>, BTreeMap<i64, f64>)>,
    /// `(bid_share_2pct, bid_share_10pct)` per exchange — refreshed by tick.
    pub imbalance: Vec<(f64, f64)>,
    pub mid_price: f64,
    /// Largest single bucketed level across all exchanges (bar scaling).
    pub max_level_qty: f64,
    /// Minimum level qty to draw a bar/number (the header F chip). The
    /// cumulative wave always uses the full book. 0.0 = off.
    pub min_level_qty: f64,
    /// Last empty-book reseed attempt (ms) — throttles the app layer's
    /// REST re-seed of columns whose tracker never got a snapshot.
    pub last_reseed_ms: i64,
    /// Current reseed throttle interval (ms). Starts at 5s; the app layer
    /// doubles it (cap 60s) while unhealthy-column reseeds keep firing
    /// without effect and resets it once every column is healthy — an
    /// unfixable column (banned venue, absent symbol) must not turn the
    /// repair loop into a REST/dial storm.
    pub reseed_backoff_ms: i64,
    /// Per-column tracker-liveness probe, parallel to `config.exchanges`:
    /// `(last seen tracker revision, ms timestamp when it last CHANGED)`.
    /// A live handle whose revision froze while the app runs means the
    /// station feed behind the column died — the app layer treats it like
    /// a stale column and reseeds (freeze detector, owner order 2026-07-30).
    pub col_revisions: Vec<(u64, i64)>,
    /// Previous repair-pass verdict — lets the repair loop log only the
    /// healthy↔unhealthy TRANSITIONS instead of spamming every pass.
    pub was_unhealthy: bool,

    // ---- UX state (ephemeral, not persisted) ----
    /// Cycle index into KNOWN_EXCHANGES for the settings-modal "Add" row.
    pub editor_exchange_cycle_idx: usize,
    /// Locked (true) = ladder recenters on mid every tick; Free = the user
    /// scrolls and the view keeps an absolute price anchor (the L/F chip).
    pub locked: bool,
    /// Absolute price at the vertical center of the ladder. Tracks mid while
    /// locked; frozen (scroll-adjusted) while free. 0.0 = not yet anchored.
    pub view_center_price: f64,
}

// ── Roster column identity ───────────────────────────────────────────────────
// A roster entry is an "exchange[:AT]" composite ("bybit:F"); a bare slug is
// Spot (legacy persisted rosters). Column identity = the (exchange, AT) pair —
// BYBIT:S and BYBIT:F are two distinct columns.

/// Exchange slug part of a roster entry.
pub fn col_exchange(entry: &str) -> &str {
    entry.split(':').next().unwrap_or(entry)
}

/// Account-type short label of a roster entry ("S" when absent — legacy).
pub fn col_at(entry: &str) -> &str {
    match entry.splitn(2, ':').nth(1) {
        Some(at) if !at.is_empty() => at,
        _ => "S",
    }
}

/// Canonical roster entry from an exchange slug + AT short label.
pub fn col_entry(exchange: &str, at: &str) -> String {
    let at = if at.is_empty() { "S" } else { at };
    format!("{}:{}", exchange.to_lowercase(), at.to_uppercase())
}

/// Does a roster entry denote this (exchange, AT) column?
pub fn col_matches(entry: &str, exchange: &str, at: &str) -> bool {
    let want_at = if at.is_empty() { "S" } else { at };
    col_exchange(entry).eq_ignore_ascii_case(exchange)
        && col_at(entry).eq_ignore_ascii_case(want_at)
}

/// Column header label — the system "EXCHANGE:AT" format.
pub fn col_label(entry: &str) -> String {
    format!(
        "{}:{}",
        col_exchange(entry).to_uppercase(),
        col_at(entry).to_uppercase()
    )
}

impl MultiDomState {
    pub fn new(symbol: String, config: MultiDomConfig) -> Self {
        let n = config.exchanges.len();
        Self {
            symbol,
            config,
            handles: (0..n).map(|_| None).collect(),
            books_by_exchange: (0..n).map(|_| (BTreeMap::new(), BTreeMap::new())).collect(),
            imbalance: vec![(0.5, 0.5); n],
            mid_price: 0.0,
            max_level_qty: 0.0,
            min_level_qty: 0.0,
            last_reseed_ms: 0,
            reseed_backoff_ms: 5_000,
            col_revisions: vec![(0, 0); n],
            was_unhealthy: false,
            editor_exchange_cycle_idx: 0,
            locked: true,
            view_center_price: 0.0,
        }
    }

    /// Add a column if not already present (entry = "exchange[:AT]",
    /// normalized to the canonical composite). Appends blank book/handle slots.
    pub fn add_exchange(&mut self, entry: ExchangeId) {
        let ex = col_exchange(&entry).to_string();
        let at = col_at(&entry).to_string();
        if self.config.exchanges.iter().any(|e| col_matches(e, &ex, &at)) {
            return;
        }
        self.config.exchanges.push(col_entry(&ex, &at));
        self.handles.push(None);
        self.books_by_exchange.push((BTreeMap::new(), BTreeMap::new()));
        self.imbalance.push((0.5, 0.5));
        self.col_revisions.push((0, 0));
    }

    /// Remove a column and its book/handle state by entry ("exchange[:AT]").
    pub fn remove_exchange(&mut self, entry: &str) {
        let ex = col_exchange(entry);
        let at = col_at(entry);
        if let Some(idx) = self.config.exchanges.iter().position(|e| col_matches(e, ex, at)) {
            self.config.exchanges.remove(idx);
            if idx < self.handles.len() { self.handles.remove(idx); }
            if idx < self.books_by_exchange.len() { self.books_by_exchange.remove(idx); }
            if idx < self.imbalance.len() { self.imbalance.remove(idx); }
            if idx < self.col_revisions.len() { self.col_revisions.remove(idx); }
        }
    }

    /// Change grid size; books re-bucket on the next tick.
    pub fn set_grid_size(&mut self, v: f64) {
        if v <= 0.0 { return; }
        self.config.grid_size = v;
    }

    /// Change depth per side (visible rows).
    pub fn set_depth_per_side(&mut self, v: usize) {
        if v == 0 { return; }
        self.config.depth_per_side = v;
    }

    /// Cycle grid size to the next preset value.
    pub fn cycle_grid_size(&mut self) {
        let current = self.config.grid_size;
        let next = GRID_PRESETS.iter()
            .find(|&&g| g > current + f64::EPSILON)
            .copied()
            .unwrap_or(GRID_PRESETS[0]);
        self.set_grid_size(next);
    }

    /// Cycle depth to the next preset value.
    pub fn cycle_depth(&mut self) {
        let current = self.config.depth_per_side;
        let next = DEPTH_PRESETS.iter()
            .find(|&&d| d > current)
            .copied()
            .unwrap_or(DEPTH_PRESETS[0]);
        self.set_depth_per_side(next);
    }

    /// Add the next unknown exchange from KNOWN_EXCHANGES (cycle-add).
    pub fn cycle_add_exchange(&mut self) -> Option<&str> {
        let n = KNOWN_EXCHANGES.len();
        for _ in 0..n {
            let candidate = KNOWN_EXCHANGES[self.editor_exchange_cycle_idx % n];
            self.editor_exchange_cycle_idx = (self.editor_exchange_cycle_idx + 1) % n;
            if !self.config.exchanges.iter().any(|e| e == candidate) {
                self.add_exchange(candidate.to_string());
                return self.config.exchanges.last().map(|s| s.as_str());
            }
        }
        None
    }

    /// Wheel/drag scroll — Free mode ONLY (owner rule: Locked means NO
    /// scroll; switch the L/F chip to F first). Moves the view anchor by
    /// whole rows.
    pub fn scroll_by(&mut self, dy: f64) {
        if self.locked || self.mid_price <= 0.0 { return; }
        if self.view_center_price <= 0.0 {
            self.view_center_price = self.mid_price;
        }
        let grid = self.config.grid_size.max(1e-9);
        let lines = (dy / ROW_HEIGHT as f64).round();
        self.view_center_price += lines * grid;
    }

    /// L/F toggle. Locking snaps the view back onto mid.
    pub fn set_locked(&mut self, locked: bool) {
        self.locked = locked;
        if locked && self.mid_price > 0.0 {
            self.view_center_price = self.mid_price;
        }
    }

    /// Refresh bucketed books + imbalance windows from the live tracker
    /// handles. Called once per frame while the panel is in multi mode.
    pub fn tick(&mut self) {
        let n = self.config.exchanges.len();
        if self.handles.len() != n { self.handles.resize_with(n, || None); }
        if self.books_by_exchange.len() != n {
            self.books_by_exchange = (0..n).map(|_| (BTreeMap::new(), BTreeMap::new())).collect();
        }
        if self.imbalance.len() != n { self.imbalance.resize(n, (0.5, 0.5)); }
        if self.col_revisions.len() != n { self.col_revisions.resize(n, (0, 0)); }
        let grid = self.config.grid_size.max(1e-9);

        // Pass 1: raw top-of-book per exchange + per-venue mid candidates.
        // The view mid is the MEDIAN of the venue mids: one venue's ghost
        // book (stale unremoved levels around an old price) must not drag
        // the anchor away from the real market. Crossed bbo (bid >= ask)
        // marks exactly such a ghost book — excluded from the vote.
        let mut raw: Vec<(Vec<(f64, f64)>, Vec<(f64, f64)>)> = Vec::with_capacity(n);
        let mut mids: Vec<f64> = Vec::new();
        for i in 0..n {
            let mut bids: Vec<(f64, f64)> = Vec::new();
            let mut asks: Vec<(f64, f64)> = Vec::new();
            if let Some(h) = &self.handles[i] {
                let guard = h.blocking_read();
                if let Some((b, a)) = guard.bbo() {
                    let (b, a) = (b.to_f64().unwrap_or(0.0), a.to_f64().unwrap_or(0.0));
                    if b > 0.0 && a > b { mids.push((b + a) * 0.5); }
                }
                bids = guard.top_bids(500).iter()
                    .map(|(p, q)| (p.to_f64().unwrap_or(0.0), q.to_f64().unwrap_or(0.0)))
                    .collect();
                asks = guard.top_asks(500).iter()
                    .map(|(p, q)| (p.to_f64().unwrap_or(0.0), q.to_f64().unwrap_or(0.0)))
                    .collect();
            }
            raw.push((bids, asks));
        }
        if mids.is_empty() { return } // keep last state until a clean book lives
        mids.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        let mid = if mids.len() % 2 == 1 {
            mids[mids.len() / 2]
        } else {
            (mids[mids.len() / 2 - 1] + mids[mids.len() / 2]) * 0.5
        };
        self.mid_price = mid;
        if self.locked || self.view_center_price <= 0.0 {
            self.view_center_price = mid;
        }

        // Pass 2: bucket onto the grid + imbalance windows + bar-scale max.
        let mut max_q = 0.0_f64;
        for (i, (bids, asks)) in raw.iter().enumerate() {
            let (bmap, amap) = &mut self.books_by_exchange[i];
            bmap.clear();
            amap.clear();
            let (mut b2, mut a2, mut b10, mut a10) = (0.0, 0.0, 0.0, 0.0);
            for &(p, q) in bids {
                if q <= 0.0 || p <= 0.0 { continue; }
                let idx = (p / grid).floor() as i64;
                *bmap.entry(idx).or_insert(0.0) += q;
                if p >= mid * 0.98 { b2 += q; }
                if p >= mid * 0.90 { b10 += q; }
            }
            for &(p, q) in asks {
                if q <= 0.0 || p <= 0.0 { continue; }
                let idx = (p / grid).floor() as i64;
                *amap.entry(idx).or_insert(0.0) += q;
                if p <= mid * 1.02 { a2 += q; }
                if p <= mid * 1.10 { a10 += q; }
            }
            for q in bmap.values().chain(amap.values()) {
                if *q > max_q { max_q = *q; }
            }
            let share = |b: f64, a: f64| if b + a > 0.0 { b / (b + a) } else { 0.5 };
            self.imbalance[i] = (share(b2, a2), share(b10, a10));
        }
        self.max_level_qty = max_q;
    }
}

impl Clone for MultiDomState {
    fn clone(&self) -> Self {
        Self {
            symbol: self.symbol.clone(),
            config: self.config.clone(),
            handles: self.handles.clone(),
            books_by_exchange: self.books_by_exchange.clone(),
            imbalance: self.imbalance.clone(),
            mid_price: self.mid_price,
            max_level_qty: self.max_level_qty,
            min_level_qty: self.min_level_qty,
            last_reseed_ms: self.last_reseed_ms,
            reseed_backoff_ms: self.reseed_backoff_ms,
            col_revisions: self.col_revisions.clone(),
            was_unhealthy: self.was_unhealthy,
            editor_exchange_cycle_idx: self.editor_exchange_cycle_idx,
            locked: self.locked,
            view_center_price: self.view_center_price,
        }
    }
}

impl std::fmt::Debug for MultiDomState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiDomState")
            .field("symbol", &self.symbol)
            .field("exchanges", &self.config.exchanges)
            .field("mid_price", &self.mid_price)
            .field("live_handles", &self.handles.iter().filter(|h| h.is_some()).count())
            .finish()
    }
}

// ============================================================================
// TradingPanel impl
// ============================================================================

const ROW_HEIGHT: f32 = 16.0;
const HEADER_HEIGHT: f32 = 18.0;
const PRICE_COL_WIDTH: f32 = 64.0;
/// Bottom liquidity-imbalance strip: two 16px rows + separator.
const IMB_STRIP_H: f32 = 34.0;
/// A level at or above this fraction of the column max is a "big order".
const BIG_ORDER_FRAC: f64 = 0.35;

fn fmt_qty(q: f64) -> String {
    if q >= 1_000_000.0 {
        format!("{:.1}M", q / 1_000_000.0)
    } else if q >= 1_000.0 {
        format!("{:.1}K", q / 1_000.0)
    } else if q >= 100.0 {
        format!("{:.0}", q)
    } else {
        format!("{:.2}", q)
    }
}

impl TradingPanel for MultiDomState {
    fn kind(&self) -> &'static str { "multi_dom" }
    fn label(&self) -> &'static str { "Multi-Exchange DOM" }

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
        use uzor::input::{Sense, WidgetKind, LayerId};

        // No in-panel config chrome: all Multi-DOM configuration lives in the
        // section-header gear (canonical DOM settings modal). The body senses
        // scroll/drag for the Free view mode.
        let body_id = format!("{}:multi_dom:body", slot_prefix);
        coordinator.register_composite(
            body_id,
            WidgetKind::BlackboxPanel,
            uzor::Rect::new(x as f64, (y + HEADER_HEIGHT) as f64, w as f64, (h - HEADER_HEIGHT) as f64),
            Sense::SCROLL | Sense::HOVER | Sense::DRAG,
            &LayerId::main(),
        );

        // Background.
        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        let n = self.config.exchanges.len();
        if n == 0 {
            self.draw_empty_header(ctx, x, y, w, theme);
            return;
        }

        // ── Header row: "Price | <exchange labels...>" ────────────────────
        let avail_w = (w - PRICE_COL_WIDTH).max(40.0);
        let col_w = avail_w / n as f32;
        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, HEADER_HEIGHT as f64);
        ctx.set_fill_color(&theme.text_primary);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Price", (x + 4.0) as f64, (y + HEADER_HEIGHT * 0.5) as f64);
        for (i, ex) in self.config.exchanges.iter().enumerate() {
            let cx = x + PRICE_COL_WIDTH + col_w * i as f32 + col_w * 0.5;
            ctx.set_text_align(TextAlign::Center);
            // Dead handle → muted label (book not live yet).
            let live = self.handles.get(i).map(|h| h.is_some()).unwrap_or(false);
            ctx.set_fill_color(if live { &theme.text_primary } else { &theme.text_muted });
            // System format: "EXCHANGE:AT" (owner spec 2026-07-30).
            ctx.fill_text(&col_label(ex), cx as f64, (y + HEADER_HEIGHT * 0.5) as f64);
        }

        let body_y = y + HEADER_HEIGHT;
        let body_h = (h - HEADER_HEIGHT).max(0.0);

        if self.mid_price <= 0.0 {
            ctx.set_fill_color(&theme.text_muted);
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(
                "Waiting for orderbook data…",
                (x + w * 0.5) as f64,
                (body_y + body_h * 0.5) as f64,
            );
        } else {
            self.draw_book(ctx, x, body_y, w, body_h, theme, n, col_w);
        }

        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
    }

    fn handle_click(&mut self, _local_id: &str, _x: f64, _y: f64) -> bool {
        // All Multi-DOM config actions route through the canonical DOM
        // settings modal (dom_modal:* ids) — nothing panel-local to click.
        false
    }
}

impl MultiDomState {
    /// Ladder + big-order markers + bottom imbalance strip.
    #[allow(clippy::too_many_arguments)]
    fn draw_book(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32, body_y: f32, w: f32, body_h: f32,
        theme: &crate::panel_theme::PanelTheme,
        n: usize,
        col_w: f32,
    ) {
        let grid = self.config.grid_size.max(1e-9);
        let strip_h = if self.config.show_imbalance { IMB_STRIP_H } else { 0.0 };
        let ladder_h = (body_h - strip_h).max(0.0);
        let rows = (ladder_h / ROW_HEIGHT).floor() as i64;
        if rows < 3 { return; }
        let half = rows / 2;
        let mid_idx = (self.mid_price / grid).floor() as i64;
        // View anchor: mid while Locked; the user's absolute scroll price in
        // Free (the L/F chip). Side/cum semantics stay keyed to mid.
        let anchor = if self.locked || self.view_center_price <= 0.0 {
            self.mid_price
        } else {
            self.view_center_price
        };
        let center_idx = (anchor / grid).floor() as i64;
        let max_norm = self.max_level_qty.max(1e-9);
        let big_thr = max_norm * BIG_ORDER_FRAC;

        // Cumulative depth from mid outward, per exchange per side — the
        // reference "waves": the stepped silhouette of book size at depth.
        // Sized to cover the visible window even when it is scrolled away
        // from mid (top/bottom visible grid index vs mid).
        let up = (center_idx + half - mid_idx).max(0) as usize;
        let down = (mid_idx - (center_idx - (rows - 1 - half)) + 1).max(0) as usize;
        let mut ask_cum: Vec<Vec<f64>> = vec![vec![0.0; up + 1]; n];
        let mut bid_cum: Vec<Vec<f64>> = vec![vec![0.0; down + 1]; n];
        let mut max_cum = 0.0_f64;
        for i in 0..n {
            let (bmap, amap) = &self.books_by_exchange[i];
            let mut c = 0.0;
            for k in 1..=up {
                c += amap.get(&(mid_idx + k as i64)).copied().unwrap_or(0.0);
                ask_cum[i][k] = c;
            }
            if c > max_cum { max_cum = c; }
            let mut c = 0.0;
            for k in 0..=down.saturating_sub(1) {
                c += bmap.get(&(mid_idx - k as i64)).copied().unwrap_or(0.0);
                bid_cum[i][k] = c;
            }
            if c > max_cum { max_cum = c; }
        }
        let max_cum = max_cum.max(1e-9);

        // System colors only (owner spec) — semantic buy/sell + highlights.
        let ask_stripe = mlc_core::apply_opacity(&theme.sell, 0.04);
        let bid_stripe = mlc_core::apply_opacity(&theme.buy, 0.04);
        let mid_stripe = mlc_core::apply_opacity(&theme.current_price, 0.12);
        let ask_wave = mlc_core::apply_opacity(&theme.sell, 0.13);
        let bid_wave = mlc_core::apply_opacity(&theme.buy, 0.13);
        // Volume-intensity bar gradient (skin doctrine 2026-07-31): bar
        // color runs side primary → side "bright" secondary as the level
        // grows, so walls glow hotter. Quantized to a small LUT computed
        // once per frame — no per-cell color math.
        const GRAD_STEPS: usize = 8;
        let texture = theme.style.bar_texture;
        let mut ask_grad: Vec<String> = Vec::with_capacity(GRAD_STEPS);
        let mut bid_grad: Vec<String> = Vec::with_capacity(GRAD_STEPS);
        for k in 0..GRAD_STEPS {
            // Flat texture: constant side color (LUT collapses to step 0).
            let t = if texture == BarTexture::Flat {
                0.0
            } else {
                k as f64 / (GRAD_STEPS - 1) as f64
            };
            let a = match texture {
                // Mosaic cell fill: intensity carried by ALPHA (no bar
                // length), so the ramp is much wider than the bar LUT's.
                BarTexture::Cells => 0.10 + 0.62 * t,
                _ => 0.45 + 0.35 * t,
            };
            ask_grad.push(mlc_core::apply_opacity(
                &mlc_core::mix_colors(&theme.sell, &theme.sell_bright, t), a));
            bid_grad.push(mlc_core::apply_opacity(
                &mlc_core::mix_colors(&theme.buy, &theme.buy_bright, t), a));
        }

        ctx.set_font("10px monospace");
        for r in 0..rows {
            let idx = center_idx + (half - r);
            let price = idx as f64 * grid;
            if price <= 0.0 { continue; }
            let row_y = (body_y + ROW_HEIGHT * r as f32) as f64;
            let is_ask = idx > mid_idx;
            let is_mid = idx == mid_idx;

            // Zebra underlay (skin texture layer) — under the side stripes.
            if theme.style.row_zebra && r % 2 == 1 {
                ctx.set_fill_color(&theme.row_bg_alt);
                ctx.fill_rect(x as f64, row_y, w as f64, ROW_HEIGHT as f64);
            }

            // Faint side stripe across the full row.
            let stripe = if is_ask {
                &ask_stripe
            } else if is_mid {
                &mid_stripe
            } else {
                &bid_stripe
            };
            ctx.set_fill_color(stripe);
            ctx.fill_rect(x as f64, row_y, w as f64, ROW_HEIGHT as f64);

            // Price column.
            ctx.set_fill_color(if is_mid { &theme.current_price } else { &theme.text_muted });
            ctx.set_text_align(TextAlign::Right);
            ctx.set_text_baseline(TextBaseline::Middle);
            let decimals = if grid >= 1.0 { 0 } else if grid >= 0.1 { 1 } else { 2 };
            ctx.fill_text(
                &format!("{:.*}", decimals, price),
                (x + PRICE_COL_WIDTH - 4.0) as f64,
                row_y + ROW_HEIGHT as f64 * 0.5,
            );

            // Per-exchange cells: cumulative wave underlay + level bar +
            // level/cumulative numbers + big-order marker.
            for i in 0..n {
                let cx = (x + PRICE_COL_WIDTH + col_w * i as f32) as f64;
                let cell_w = (col_w - 4.0) as f64;
                let (bmap, amap) = &self.books_by_exchange[i];
                let (q, cum) = if is_ask {
                    let k = (idx - mid_idx) as usize;
                    (
                        amap.get(&idx).copied().unwrap_or(0.0),
                        ask_cum[i].get(k).copied().unwrap_or(0.0),
                    )
                } else {
                    let k = (mid_idx - idx) as usize;
                    (
                        bmap.get(&idx).copied().unwrap_or(0.0),
                        bid_cum[i].get(k).copied().unwrap_or(0.0),
                    )
                };

                // Wave underlay — cumulative silhouette. Skipped in
                // mosaic mode (full-cell fills would bury it).
                if cum > 0.0 && texture != BarTexture::Cells {
                    let wf = (cum / max_cum).clamp(0.0, 1.0);
                    ctx.set_fill_color(if is_ask { &ask_wave } else { &bid_wave });
                    ctx.fill_rect(cx + 2.0, row_y, cell_w * wf, ROW_HEIGHT as f64);
                }

                if q <= 0.0 || q < self.min_level_qty { continue; }
                let frac = (q / max_norm).powf(0.6).clamp(0.04, 1.0);
                let big = q >= big_thr;
                let g = (frac * (GRAD_STEPS - 1) as f64).round() as usize;
                if texture == BarTexture::Cells {
                    // Compact mosaic (TapeSurf ref 14): the whole cell
                    // fills at volume intensity — no bar length. Dense,
                    // chartless; only big levels get a number.
                    ctx.set_fill_color(if is_ask { &ask_grad[g] } else { &bid_grad[g] });
                    ctx.fill_rect(cx + 2.0, row_y + 1.0, cell_w, (ROW_HEIGHT - 2.0) as f64);
                    if big {
                        ctx.set_fill_color(if is_ask { &theme.sell_bright } else { &theme.buy_bright });
                        ctx.fill_rect(cx + 2.0, row_y + 1.0, 2.0, (ROW_HEIGHT - 2.0) as f64);
                        ctx.set_text_baseline(TextBaseline::Middle);
                        ctx.set_fill_color(&theme.text_primary);
                        ctx.set_text_align(TextAlign::Left);
                        ctx.fill_text(&fmt_qty(q), cx + 6.0, row_y + ROW_HEIGHT as f64 * 0.5);
                    }
                    continue;
                }
                let bw = cell_w * frac;
                ctx.set_fill_color(if is_ask { &ask_grad[g] } else { &bid_grad[g] });
                ctx.fill_rect(cx + 2.0, row_y + 1.0, bw, (ROW_HEIGHT - 2.0) as f64);
                if big {
                    // Bright edge marker at the bar tip.
                    ctx.set_fill_color(if is_ask { &theme.sell_bright } else { &theme.buy_bright });
                    ctx.fill_rect(cx + 2.0 + bw - 2.0, row_y + 1.0, 2.0, (ROW_HEIGHT - 2.0) as f64);
                }
                // Numbers: level qty (left) + cumulative (right, muted) —
                // both when the column is wide enough, big levels always.
                ctx.set_text_baseline(TextBaseline::Middle);
                if cell_w >= 64.0 {
                    ctx.set_fill_color(if big { &theme.text_primary } else { &theme.text_muted });
                    ctx.set_text_align(TextAlign::Left);
                    ctx.fill_text(&fmt_qty(q), cx + 4.0, row_y + ROW_HEIGHT as f64 * 0.5);
                    ctx.set_fill_color(&theme.text_muted);
                    ctx.set_text_align(TextAlign::Right);
                    ctx.fill_text(&fmt_qty(cum), cx + cell_w - 2.0, row_y + ROW_HEIGHT as f64 * 0.5);
                } else if big {
                    ctx.set_fill_color(&theme.text_primary);
                    ctx.set_text_align(TextAlign::Left);
                    ctx.fill_text(&fmt_qty(q), cx + 4.0, row_y + ROW_HEIGHT as f64 * 0.5);
                }
            }

            // Mid separator line under the mid row.
            if is_mid {
                ctx.set_stroke_color(&theme.current_price);
                ctx.set_stroke_width(1.0);
                ctx.begin_path();
                ctx.move_to(x as f64, row_y + ROW_HEIGHT as f64);
                ctx.line_to((x + w) as f64, row_y + ROW_HEIGHT as f64);
                ctx.stroke();
            }
        }

        // ── Bottom strip: ±2% / ±10% bid-liquidity share per exchange ────
        if !self.config.show_imbalance {
            return;
        }
        let strip_y = (body_y + ladder_h) as f64;
        ctx.set_stroke_color(&theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(x as f64, strip_y);
        ctx.line_to((x + w) as f64, strip_y);
        ctx.stroke();
        for (row, label) in ["2%", "10%"].iter().enumerate() {
            let ry = strip_y + 1.0 + row as f64 * 16.0;
            ctx.set_fill_color(&theme.text_muted);
            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(label, (x + 4.0) as f64, ry + 8.0);
            let buy_half = mlc_core::apply_opacity(&theme.buy, 0.55);
            let sell_half = mlc_core::apply_opacity(&theme.sell, 0.55);
            for i in 0..n {
                let share = if row == 0 { self.imbalance[i].0 } else { self.imbalance[i].1 };
                let cx = (x + PRICE_COL_WIDTH + col_w * i as f32) as f64 + 2.0;
                let cw = (col_w - 6.0) as f64;
                let gw = cw * share.clamp(0.0, 1.0);
                ctx.set_fill_color(&buy_half);
                ctx.fill_rect(cx, ry + 2.0, gw, 12.0);
                ctx.set_fill_color(&sell_half);
                ctx.fill_rect(cx + gw, ry + 2.0, cw - gw, 12.0);
                ctx.set_fill_color(&theme.text_primary);
                ctx.set_text_align(TextAlign::Center);
                ctx.fill_text(
                    &format!("{:.0}%", share * 100.0),
                    cx + cw * 0.5,
                    ry + 8.0,
                );
            }
        }
    }

    /// Header when no exchanges are configured yet.
    fn draw_empty_header(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32, y: f32, w: f32,
        theme: &crate::panel_theme::PanelTheme,
    ) {
        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, HEADER_HEIGHT as f64);
        ctx.set_fill_color(&theme.text_muted);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(
            "Multi-DOM — add exchanges via the header \u{2699}",
            (x + 4.0) as f64,
            (y + HEADER_HEIGHT * 0.5) as f64,
        );
    }
}
