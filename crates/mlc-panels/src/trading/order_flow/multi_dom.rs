//! Multi-exchange consolidated DOM panel.
//!
//! Track 7 of `nemo/docs/mlc/plans/renderer-wave-B-2026-06-18.md`.
//! Specs:
//! - `nemo/docs/mlc/research/specs/aggr-multi-exchange-pattern-2026-06-18.md`
//! - `nemo/docs/mlc/research/specs/multi-exchange-depth-panels-2026-06-17.md`
//!
//! Design:
//! - Per-exchange `BTreeMap<OrderedFloat, f64>` book state for bids + asks.
//! - All exchanges merged onto a common rounded-price grid (config.grid_size).
//! - Ladder: ask side high→low at top, bid side high→low at bottom.
//! - Renderer: N+1 columns (price + one per exchange). Cell intensity ∝ size
//!   relative to the merged-row max. Row's max-volume exchange gets a side
//!   accent stripe (cross-exchange dominance highlight).
//!
//! Subscriptions are wired through the live-data bridge by the consumer
//! (mlc-app). Each `LiveUpdate::OrderbookSnapshot` for any of the configured
//! exchanges calls [`MultiDomState::apply_snapshot`].
//!
//! # Editor popup — click-cycle pattern
//!
//! A full text-field widget would require the uzor text-input infrastructure
//! (heavy). Instead, the editor uses click-to-cycle for all three configurable
//! dimensions:
//!
//! - **Exchange add**: clicking "Add" cycles through a fixed menu of known
//!   exchanges (`KNOWN_EXCHANGES`) and adds the next one not already present.
//! - **Grid size**: clicking the current value cycles through `GRID_PRESETS`.
//! - **Depth**: clicking the current value cycles through `DEPTH_PRESETS`.
//!
//! This is intentional — owner agreed cycle pattern ships, textfield deferred.

use std::collections::BTreeMap;

use serde::{Serialize, Deserialize};

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

/// Available exchanges for the cycle-add flow.
/// Owner adds by clicking; already-subscribed exchanges are skipped.
pub const KNOWN_EXCHANGES: &[&str] = &[
    "binance", "bybit", "okx", "bitget", "kucoin", "gateio",
];

/// Grid size (tick coalescence) presets, cycled by click.
pub const GRID_PRESETS: &[f64] = &[0.1, 0.5, 1.0, 5.0, 10.0];

/// Depth (rows per side) presets, cycled by click.
pub const DEPTH_PRESETS: &[usize] = &[10, 25, 50, 100];

/// Multi-DOM panel ID — same shape as DomId.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MultiDomId(pub u64);

/// Local Eq/Ord newtype around f64 (no `ordered_float` dep).
#[derive(Clone, Copy, Debug)]
pub struct OrderedFloat(pub f64);

impl PartialEq for OrderedFloat {
    fn eq(&self, other: &Self) -> bool { self.0.to_bits() == other.0.to_bits() }
}
impl Eq for OrderedFloat {}
impl PartialOrd for OrderedFloat {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}
impl Ord for OrderedFloat {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.partial_cmp(&other.0).unwrap_or(std::cmp::Ordering::Equal)
    }
}

/// Exchange identifier — opaque label so the panel doesn't pull a hard
/// dep on the `digdigdig3` crate. Wire up to `digdigdig3::ExchangeId`
/// at the app layer with `format!("{:?}", ex)`.
pub type ExchangeId = String;

/// Snapshot of one merged price level — per-exchange volume + side flag.
#[derive(Clone, Debug)]
pub struct MultiLevel {
    pub price: f64,
    /// `per_exchange[i]` = volume at this price from `config.exchanges[i]`.
    pub per_exchange: Vec<f64>,
    pub is_ask: bool,
}

/// Static configuration for a multi-exchange book.
#[derive(Clone, Debug)]
pub struct MultiDomConfig {
    pub exchanges: Vec<ExchangeId>,
    pub grid_size: f64,
    pub depth_per_side: usize,
}

impl Default for MultiDomConfig {
    fn default() -> Self {
        Self {
            exchanges: Vec::new(),
            grid_size: 0.5,
            depth_per_side: 25,
        }
    }
}

/// Multi-exchange DOM state (mode `state` per other panels).
pub struct MultiDomState {
    pub symbol: String,
    pub config: MultiDomConfig,
    /// `books_by_exchange[i]` = `(bids, asks)` for `config.exchanges[i]`.
    pub books_by_exchange: Vec<(BTreeMap<OrderedFloat, f64>, BTreeMap<OrderedFloat, f64>)>,
    /// Cached merged view — rebuilt by [`Self::rebuild_merged`].
    pub merged: Vec<MultiLevel>,
    pub mid_price: f64,

    // ---- editor / UX state (ephemeral, not persisted) ----

    /// True when the config editor overlay is visible.
    /// Toggled by right-click on the panel body.
    pub editor_open: bool,
    /// Cycle index into KNOWN_EXCHANGES for the "Add" button.
    /// Advances each time the user clicks "Add" until a non-present exchange
    /// is found; then that exchange is added and the index advances.
    pub editor_exchange_cycle_idx: usize,
}

impl MultiDomState {
    pub fn new(symbol: String, config: MultiDomConfig) -> Self {
        let n = config.exchanges.len();
        Self {
            symbol,
            config,
            books_by_exchange: (0..n).map(|_| (BTreeMap::new(), BTreeMap::new())).collect(),
            merged: Vec::new(),
            mid_price: 0.0,
            editor_open: false,
            editor_exchange_cycle_idx: 0,
        }
    }

    /// Snap a price down to the common grid boundary.
    #[inline]
    pub fn snap_grid(&self, price: f64) -> f64 {
        (price / self.config.grid_size).floor() * self.config.grid_size
    }

    /// Resolve an exchange label to its index in `config.exchanges`.
    pub fn exchange_idx(&self, label: &str) -> Option<usize> {
        self.config.exchanges.iter().position(|e| e == label)
    }

    /// Add an exchange if not already present.  Appends a blank book slot.
    pub fn add_exchange(&mut self, eid: ExchangeId) {
        if self.config.exchanges.iter().any(|e| *e == eid) {
            return;
        }
        self.config.exchanges.push(eid);
        self.books_by_exchange.push((BTreeMap::new(), BTreeMap::new()));
    }

    /// Remove an exchange and its book state by label.
    pub fn remove_exchange(&mut self, eid: &str) {
        if let Some(idx) = self.config.exchanges.iter().position(|e| e == eid) {
            self.config.exchanges.remove(idx);
            self.books_by_exchange.remove(idx);
            // Merged cache is stale — caller must call rebuild_merged() next frame.
            self.merged.clear();
        }
    }

    /// Change grid size and re-bucket all stored books.
    pub fn set_grid_size(&mut self, v: f64) {
        if v <= 0.0 { return; }
        self.config.grid_size = v;
        // Clear cached books — they will be refilled from the next live snapshot.
        for (b, a) in &mut self.books_by_exchange {
            b.clear();
            a.clear();
        }
        self.merged.clear();
    }

    /// Change depth per side (visible rows).
    pub fn set_depth_per_side(&mut self, v: usize) {
        if v == 0 { return; }
        self.config.depth_per_side = v;
        self.merged.clear();
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

    /// Add the next unknown exchange from KNOWN_EXCHANGES (cycle-add pattern).
    /// Returns the added exchange label, or None if all known exchanges are
    /// already subscribed.
    pub fn cycle_add_exchange(&mut self) -> Option<&str> {
        let n = KNOWN_EXCHANGES.len();
        for _ in 0..n {
            let candidate = KNOWN_EXCHANGES[self.editor_exchange_cycle_idx % n];
            self.editor_exchange_cycle_idx = (self.editor_exchange_cycle_idx + 1) % n;
            if !self.config.exchanges.iter().any(|e| e == candidate) {
                self.add_exchange(candidate.to_string());
                // Return a reference into the last element (just pushed).
                return self.config.exchanges.last().map(|s| s.as_str());
            }
        }
        None // All known exchanges already subscribed.
    }

    /// Replace the snapshot for one exchange. `bids` and `asks` are raw
    /// per-level `(price, qty)` tuples; the call buckets them onto the
    /// common grid.
    pub fn apply_snapshot<B, A>(&mut self, exchange_idx: usize, bids: B, asks: A)
    where
        B: IntoIterator<Item = (f64, f64)>,
        A: IntoIterator<Item = (f64, f64)>,
    {
        let grid = self.config.grid_size;
        let Some((b, a)) = self.books_by_exchange.get_mut(exchange_idx) else { return };
        b.clear();
        a.clear();
        for (price, qty) in bids {
            if qty <= 0.0 { continue; }
            let p = (price / grid).floor() * grid;
            *b.entry(OrderedFloat(p)).or_insert(0.0) += qty;
        }
        for (price, qty) in asks {
            if qty <= 0.0 { continue; }
            let p = (price / grid).floor() * grid;
            *a.entry(OrderedFloat(p)).or_insert(0.0) += qty;
        }
    }

    /// Rebuild [`Self::merged`] from current per-exchange books. Called
    /// each frame (cheap — N×M levels, both bounded by `depth_per_side`).
    pub fn rebuild_merged(&mut self) {
        let n = self.config.exchanges.len();
        let mut by_price_bid: BTreeMap<OrderedFloat, Vec<f64>> = BTreeMap::new();
        let mut by_price_ask: BTreeMap<OrderedFloat, Vec<f64>> = BTreeMap::new();
        for (i, (b, a)) in self.books_by_exchange.iter().enumerate() {
            for (p, q) in b {
                let row = by_price_bid.entry(*p).or_insert_with(|| vec![0.0; n]);
                row[i] += *q;
            }
            for (p, q) in a {
                let row = by_price_ask.entry(*p).or_insert_with(|| vec![0.0; n]);
                row[i] += *q;
            }
        }

        let best_bid = by_price_bid.keys().next_back().copied();
        let best_ask = by_price_ask.keys().next().copied();
        self.mid_price = match (best_bid, best_ask) {
            (Some(b), Some(a)) => (b.0 + a.0) * 0.5,
            (Some(b), None) => b.0,
            (None, Some(a)) => a.0,
            _ => self.mid_price,
        };

        self.merged.clear();
        // Asks (top of ladder) — keep highest above mid, then descend.
        // Output order: highest ask first → mid → lowest bid at bottom.
        let asks_take: Vec<MultiLevel> = by_price_ask.iter()
            .take(self.config.depth_per_side)
            .map(|(p, row)| MultiLevel { price: p.0, per_exchange: row.clone(), is_ask: true })
            .collect();
        // We want asks displayed high→low (top of ladder shows the
        // largest sell wall in walking-down order). BTreeMap iteration
        // is ascending, so reverse the slice we kept.
        let mut asks_rev: Vec<MultiLevel> = asks_take;
        asks_rev.reverse();
        self.merged.extend(asks_rev);
        for (p, row) in by_price_bid.iter().rev().take(self.config.depth_per_side) {
            self.merged.push(MultiLevel { price: p.0, per_exchange: row.clone(), is_ask: false });
        }
    }
}

impl Clone for MultiDomState {
    fn clone(&self) -> Self {
        Self {
            symbol: self.symbol.clone(),
            config: self.config.clone(),
            books_by_exchange: self.books_by_exchange.clone(),
            merged: self.merged.clone(),
            mid_price: self.mid_price,
            editor_open: false, // never copy editor open state to clones
            editor_exchange_cycle_idx: self.editor_exchange_cycle_idx,
        }
    }
}

impl std::fmt::Debug for MultiDomState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiDomState")
            .field("symbol", &self.symbol)
            .field("exchanges", &self.config.exchanges)
            .field("merged_len", &self.merged.len())
            .field("mid_price", &self.mid_price)
            .finish()
    }
}

// ============================================================================
// TradingPanel impl
// ============================================================================

const ROW_HEIGHT: f32 = 18.0;
const HEADER_HEIGHT: f32 = 18.0;
const PRICE_COL_WIDTH: f32 = 70.0;

/// Height of the editor overlay popup.
const EDITOR_ROW_H: f32 = 22.0;
/// Number of rows in the editor: header + per-exchange rows + add row + grid row + depth row
fn editor_height(n_exchanges: usize) -> f32 {
    EDITOR_ROW_H * (1 + n_exchanges + 1 + 1 + 1) as f32
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

        // Register right-click on the panel header to open/close editor.
        let header_id = format!("{}:multi_dom:header", slot_prefix);
        coordinator.register_composite(
            header_id,
            WidgetKind::BlackboxPanel,
            uzor::Rect::new(x as f64, y as f64, w as f64, HEADER_HEIGHT as f64),
            Sense::RIGHT_CLICK,
            &LayerId::main(),
        );

        // Explicit ⚙ gear button at the far right of the header.
        // Registered with CLICK so it is always reachable (right-click is unreliable on some
        // platforms / panel configurations; owner reported it does not work).
        let gear_btn_w = 20.0_f64;
        let gear_btn_x = (x + w) as f64 - gear_btn_w;
        let settings_id = format!("{}:multi_dom:settings", slot_prefix);
        coordinator.register_composite(
            settings_id,
            WidgetKind::BlackboxPanel,
            uzor::Rect::new(gear_btn_x, y as f64, gear_btn_w, HEADER_HEIGHT as f64),
            Sense::CLICK,
            &LayerId::main(),
        );

        // Register interactive body for hover/scroll/drag.
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

        // Empty-state hint (show even when no data yet but exchanges configured).
        if n == 0 {
            // Draw header with config hint even in empty state.
            self.draw_empty_header(ctx, x, y, w, theme, slot_prefix, coordinator);
            if self.editor_open {
                self.draw_editor_menu(ctx, x, y, w, theme, slot_prefix, coordinator);
            }
            return;
        }

        let avail_w = (w - PRICE_COL_WIDTH).max(40.0);
        let col_w = avail_w / n as f32;
        let header_y = y;
        let body_y = y + HEADER_HEIGHT;

        // Header row: "Price | <exchange labels...>" + "⚙" settings button.
        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, header_y as f64, w as f64, HEADER_HEIGHT as f64);
        ctx.set_fill_color(&theme.text_primary);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Price", (x + 4.0) as f64, (header_y + HEADER_HEIGHT * 0.5) as f64);
        for (i, ex) in self.config.exchanges.iter().enumerate() {
            let cx = x + PRICE_COL_WIDTH + col_w * i as f32 + col_w * 0.5;
            ctx.set_text_align(TextAlign::Center);
            ctx.fill_text(ex, cx as f64, (header_y + HEADER_HEIGHT * 0.5) as f64);
        }

        // Settings gear (⚙) at far-right corner of header.
        let gear_x = x + w - 18.0;
        let gear_y = header_y + HEADER_HEIGHT * 0.5;
        let gear_color = if self.editor_open { &theme.accent } else { &theme.text_muted };
        ctx.set_fill_color(gear_color);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("\u{2699}", gear_x as f64, gear_y as f64);

        if self.merged.is_empty() {
            ctx.set_fill_color(&theme.text_muted);
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(
                "Waiting for orderbook data…",
                (x + w * 0.5) as f64,
                (body_y + (h - HEADER_HEIGHT) * 0.5) as f64,
            );
        } else {
            self.draw_book(ctx, x, body_y, w, h - HEADER_HEIGHT, theme, n, col_w);
        }

        // Editor overlay (drawn on top of book).
        if self.editor_open {
            self.draw_editor_menu(ctx, x, y, w, theme, slot_prefix, coordinator);
        }

        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
    }

    fn handle_click(&mut self, local_id: &str, _x: f64, _y: f64) -> bool {
        // local_id examples:
        //   "multi_dom:header"          — right-click header → toggle editor
        //   "multi_dom:settings"        — explicit ⚙ gear button click → toggle editor
        //   "multi_dom:editor:del:0"    — delete exchange at index 0
        //   "multi_dom:editor:add"      — cycle-add next unknown exchange
        //   "multi_dom:editor:grid"     — cycle grid size
        //   "multi_dom:editor:depth"    — cycle depth per side

        if local_id == "multi_dom:header" || local_id == "multi_dom:settings" {
            self.editor_open = !self.editor_open;
            return true;
        }
        if local_id.starts_with("multi_dom:editor:") {
            let action = &local_id["multi_dom:editor:".len()..];
            if let Some(idx_str) = action.strip_prefix("del:") {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    if idx < self.config.exchanges.len() {
                        // App layer listens for exchange list changes via the
                        // state diff on next tick; actual unsubscribe is done
                        // in the app layer's remove_exchange hook.
                        // The panel marks the exchange for removal here.
                        let label = self.config.exchanges[idx].clone();
                        self.remove_exchange(&label);
                    }
                }
                return true;
            }
            if action == "add" {
                self.cycle_add_exchange();
                return true;
            }
            if action == "grid" {
                self.cycle_grid_size();
                return true;
            }
            if action == "depth" {
                self.cycle_depth();
                return true;
            }
        }
        false
    }
}

impl MultiDomState {
    /// Draw the panel body when no exchanges are configured yet.
    fn draw_empty_header(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32, y: f32, w: f32,
        theme: &crate::panel_theme::PanelTheme,
        slot_prefix: &str,
        coordinator: &mut uzor::InputCoordinator,
    ) {
        use uzor::input::{Sense, WidgetKind, LayerId};

        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, HEADER_HEIGHT as f64);
        ctx.set_fill_color(&theme.text_muted);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(
            "Multi-DOM — click \u{2699} to add exchanges",
            (x + 4.0) as f64,
            (y + HEADER_HEIGHT * 0.5) as f64,
        );

        // Gear icon at far right.
        let gear_color = if self.editor_open { &theme.accent } else { &theme.text_muted };
        ctx.set_fill_color(gear_color);
        ctx.set_text_align(TextAlign::Center);
        ctx.fill_text(
            "\u{2699}",
            (x + w - 10.0) as f64,
            (y + HEADER_HEIGHT * 0.5) as f64,
        );

        // Register header hit-zone (right-click).
        let header_id = format!("{}:multi_dom:header", slot_prefix);
        coordinator.register_composite(
            header_id,
            WidgetKind::BlackboxPanel,
            uzor::Rect::new(x as f64, y as f64, w as f64, HEADER_HEIGHT as f64),
            Sense::RIGHT_CLICK,
            &LayerId::main(),
        );

        // Register gear button hit-zone (left-click).
        let gear_btn_w = 20.0_f64;
        let gear_btn_x = (x + w) as f64 - gear_btn_w;
        let settings_id = format!("{}:multi_dom:settings", slot_prefix);
        coordinator.register_composite(
            settings_id,
            WidgetKind::BlackboxPanel,
            uzor::Rect::new(gear_btn_x, y as f64, gear_btn_w, HEADER_HEIGHT as f64),
            Sense::CLICK,
            &LayerId::main(),
        );
    }

    /// Draw the book ladder (asks top, bids bottom).
    fn draw_book(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32, body_y: f32, w: f32, body_h: f32,
        theme: &crate::panel_theme::PanelTheme,
        n: usize,
        _col_w: f32,
    ) {
        let avail_w = (w - PRICE_COL_WIDTH).max(40.0);
        let col_w_actual = avail_w / n as f32;

        // Per-row max volume for the dominance highlight.
        let level_max_row = |row: &[f64]| row.iter().cloned().fold(0.0_f64, f64::max);

        // Determine the global row-max for cell-intensity normalization.
        let global_max: f64 = self.merged.iter()
            .map(|lvl| level_max_row(&lvl.per_exchange))
            .fold(0.0, f64::max);
        let max_norm = if global_max > 0.0 { global_max } else { 1.0 };

        let max_rows = ((body_h) / ROW_HEIGHT).floor() as usize;
        let rows = self.merged.len().min(max_rows);

        ctx.set_font("11px monospace");
        for r in 0..rows {
            let lvl = &self.merged[r];
            let row_y = body_y + ROW_HEIGHT * r as f32;

            // Faint row-stripe by side.
            let stripe = if lvl.is_ask {
                "rgba(239, 83, 80, 0.05)"
            } else {
                "rgba(38, 166, 154, 0.05)"
            };
            ctx.set_fill_color(stripe);
            ctx.fill_rect(x as f64, row_y as f64, w as f64, ROW_HEIGHT as f64);

            // Price column.
            ctx.set_fill_color(&theme.text_primary);
            ctx.set_text_align(TextAlign::Right);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(
                &format!("{:.2}", lvl.price),
                (x + PRICE_COL_WIDTH - 4.0) as f64,
                (row_y + ROW_HEIGHT * 0.5) as f64,
            );

            // Determine dominant exchange for this row.
            let row_max = level_max_row(&lvl.per_exchange);
            let dominant_idx = lvl.per_exchange.iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i);

            for (i, vol) in lvl.per_exchange.iter().enumerate() {
                let cx = x + PRICE_COL_WIDTH + col_w_actual * i as f32;
                let intensity = (vol / max_norm).clamp(0.0, 1.0);
                let bg_alpha = (0.15 + 0.6 * intensity).clamp(0.0, 1.0);
                let bg = if lvl.is_ask {
                    format!("rgba(239, 83, 80, {:.3})", bg_alpha)
                } else {
                    format!("rgba(38, 166, 154, {:.3})", bg_alpha)
                };
                ctx.set_fill_color(&bg);
                ctx.fill_rect(cx as f64, row_y as f64, col_w_actual as f64, ROW_HEIGHT as f64);

                // Dominance accent (cross-exchange flag).
                if dominant_idx == Some(i) && row_max > 0.0 {
                    let accent = if lvl.is_ask { "#ef5350" } else { "#26a69a" };
                    ctx.set_fill_color(accent);
                    let strip_w = 2.0;
                    let strip_x = if lvl.is_ask { cx + col_w_actual - strip_w } else { cx };
                    ctx.fill_rect(strip_x as f64, row_y as f64, strip_w as f64, ROW_HEIGHT as f64);
                }

                // Volume text.
                if *vol > 0.0 {
                    ctx.set_fill_color(&theme.text_primary);
                    ctx.set_text_align(TextAlign::Center);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    let label = if *vol >= 1_000_000.0 {
                        format!("{:.1}M", vol / 1_000_000.0)
                    } else if *vol >= 1_000.0 {
                        format!("{:.1}K", vol / 1_000.0)
                    } else {
                        format!("{:.2}", vol)
                    };
                    ctx.fill_text(
                        &label,
                        (cx + col_w_actual * 0.5) as f64,
                        (row_y + ROW_HEIGHT * 0.5) as f64,
                    );
                }
            }
        }

        // Mid-price separator line.
        if self.mid_price > 0.0 {
            let mut split_row: Option<usize> = None;
            for (i, lvl) in self.merged.iter().enumerate() {
                if !lvl.is_ask {
                    split_row = Some(i);
                    break;
                }
            }
            if let Some(r) = split_row {
                let line_y = body_y + ROW_HEIGHT * r as f32;
                ctx.set_stroke_color(&theme.separator);
                ctx.set_stroke_width(1.0);
                ctx.begin_path();
                ctx.move_to(x as f64, line_y as f64);
                ctx.line_to((x + w) as f64, line_y as f64);
                ctx.stroke();
            }
        }
    }

    /// Draw the editor overlay popup.
    ///
    /// Layout (each row = EDITOR_ROW_H px):
    /// ```text
    /// ┌─ Multi-DOM config ────────────────┐
    /// │ [x] binance              [del]    │
    /// │ [x] bybit                [del]    │
    /// │ [+] Add next exchange             │  (cycle-add)
    /// │ Grid: 0.5  (click to cycle)       │
    /// │ Depth: 25  (click to cycle)       │
    /// └───────────────────────────────────┘
    /// ```
    fn draw_editor_menu(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32, y: f32, w: f32,
        theme: &crate::panel_theme::PanelTheme,
        slot_prefix: &str,
        coordinator: &mut uzor::InputCoordinator,
    ) {
        use uzor::input::{Sense, WidgetKind, LayerId};

        let n = self.config.exchanges.len();
        let popup_h = editor_height(n);
        let popup_x = x + 4.0;
        let popup_y = y + HEADER_HEIGHT + 2.0;
        let popup_w = (w - 8.0).max(120.0);

        // Background + border.
        ctx.set_fill_color("#1e2330");
        ctx.fill_rect(popup_x as f64, popup_y as f64, popup_w as f64, popup_h as f64);
        ctx.set_stroke_color(&theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(popup_x as f64, popup_y as f64);
        ctx.line_to((popup_x + popup_w) as f64, popup_y as f64);
        ctx.line_to((popup_x + popup_w) as f64, (popup_y + popup_h) as f64);
        ctx.line_to(popup_x as f64, (popup_y + popup_h) as f64);
        ctx.line_to(popup_x as f64, popup_y as f64);
        ctx.stroke();

        ctx.set_font("11px sans-serif");
        ctx.set_text_baseline(TextBaseline::Middle);

        // --- Header row ---
        let row0_y = popup_y;
        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(popup_x as f64, row0_y as f64, popup_w as f64, EDITOR_ROW_H as f64);
        ctx.set_fill_color(&theme.text_primary);
        ctx.set_text_align(TextAlign::Left);
        ctx.fill_text(
            "Multi-DOM Config",
            (popup_x + 6.0) as f64,
            (row0_y + EDITOR_ROW_H * 0.5) as f64,
        );

        // --- Per-exchange rows ---
        for (i, ex) in self.config.exchanges.iter().enumerate() {
            let row_y = popup_y + EDITOR_ROW_H * (1 + i) as f32;

            // Alternate row shade.
            let row_bg = if i % 2 == 0 { "#1a1f2e" } else { "#222840" };
            ctx.set_fill_color(row_bg);
            ctx.fill_rect(popup_x as f64, row_y as f64, popup_w as f64, EDITOR_ROW_H as f64);

            // Exchange label.
            ctx.set_fill_color(&theme.text_primary);
            ctx.set_text_align(TextAlign::Left);
            ctx.fill_text(ex, (popup_x + 6.0) as f64, (row_y + EDITOR_ROW_H * 0.5) as f64);

            // [del] button on the right.
            let del_w = 32.0_f32;
            let del_x = popup_x + popup_w - del_w - 4.0;
            ctx.set_fill_color("#5a2020");
            ctx.fill_rect(del_x as f64, (row_y + 3.0) as f64, del_w as f64, (EDITOR_ROW_H - 6.0) as f64);
            ctx.set_fill_color("#ef5350");
            ctx.set_text_align(TextAlign::Center);
            ctx.fill_text("del", (del_x + del_w * 0.5) as f64, (row_y + EDITOR_ROW_H * 0.5) as f64);

            // Register del hit-zone.
            let del_id = format!("{}:multi_dom:editor:del:{}", slot_prefix, i);
            coordinator.register_composite(
                del_id,
                WidgetKind::BlackboxPanel,
                uzor::Rect::new(del_x as f64, row_y as f64, del_w as f64, EDITOR_ROW_H as f64),
                Sense::CLICK,
                &LayerId::popup(),
            );
        }

        // --- Add row ---
        let add_row_y = popup_y + EDITOR_ROW_H * (1 + n) as f32;
        ctx.set_fill_color("#1a2a1a");
        ctx.fill_rect(popup_x as f64, add_row_y as f64, popup_w as f64, EDITOR_ROW_H as f64);
        ctx.set_fill_color("#26a69a");
        ctx.set_text_align(TextAlign::Left);
        ctx.fill_text(
            "+ Add next exchange (cycle)",
            (popup_x + 6.0) as f64,
            (add_row_y + EDITOR_ROW_H * 0.5) as f64,
        );
        let add_id = format!("{}:multi_dom:editor:add", slot_prefix);
        coordinator.register_composite(
            add_id,
            WidgetKind::BlackboxPanel,
            uzor::Rect::new(popup_x as f64, add_row_y as f64, popup_w as f64, EDITOR_ROW_H as f64),
            Sense::CLICK,
            &LayerId::popup(),
        );

        // --- Grid row ---
        let grid_row_y = popup_y + EDITOR_ROW_H * (1 + n + 1) as f32;
        ctx.set_fill_color("#1a1f2e");
        ctx.fill_rect(popup_x as f64, grid_row_y as f64, popup_w as f64, EDITOR_ROW_H as f64);
        ctx.set_fill_color(&theme.text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.fill_text("Grid:", (popup_x + 6.0) as f64, (grid_row_y + EDITOR_ROW_H * 0.5) as f64);
        ctx.set_fill_color(&theme.accent);
        let grid_val = format!("{}  (click to cycle)", self.config.grid_size);
        ctx.fill_text(&grid_val, (popup_x + 44.0) as f64, (grid_row_y + EDITOR_ROW_H * 0.5) as f64);
        let grid_id = format!("{}:multi_dom:editor:grid", slot_prefix);
        coordinator.register_composite(
            grid_id,
            WidgetKind::BlackboxPanel,
            uzor::Rect::new(popup_x as f64, grid_row_y as f64, popup_w as f64, EDITOR_ROW_H as f64),
            Sense::CLICK,
            &LayerId::popup(),
        );

        // --- Depth row ---
        let depth_row_y = popup_y + EDITOR_ROW_H * (1 + n + 2) as f32;
        ctx.set_fill_color("#222840");
        ctx.fill_rect(popup_x as f64, depth_row_y as f64, popup_w as f64, EDITOR_ROW_H as f64);
        ctx.set_fill_color(&theme.text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.fill_text("Depth:", (popup_x + 6.0) as f64, (depth_row_y + EDITOR_ROW_H * 0.5) as f64);
        ctx.set_fill_color(&theme.accent);
        let depth_val = format!("{}  (click to cycle)", self.config.depth_per_side);
        ctx.fill_text(&depth_val, (popup_x + 50.0) as f64, (depth_row_y + EDITOR_ROW_H * 0.5) as f64);
        let depth_id = format!("{}:multi_dom:editor:depth", slot_prefix);
        coordinator.register_composite(
            depth_id,
            WidgetKind::BlackboxPanel,
            uzor::Rect::new(popup_x as f64, depth_row_y as f64, popup_w as f64, EDITOR_ROW_H as f64),
            Sense::CLICK,
            &LayerId::popup(),
        );
    }
}

/// Lightweight wrapper for PanelKind registration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MultiDomPanel {
    id: MultiDomId,
    title: String,
}

impl MultiDomPanel {
    pub fn new(id: MultiDomId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> MultiDomId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "multi_dom" }
    pub fn kind_label(&self) -> &'static str { "Multi-Exchange DOM" }
    pub fn min_size(&self) -> (f32, f32) { (200.0, 120.0) }
}
