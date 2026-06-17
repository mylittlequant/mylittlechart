//! Footprint cluster chart rendering — Bid × Ask split per price level.
//!
//! Stub for the upcoming dedicated renderer. Full spec in
//! `nemo/docs/mlc/research/footprint-cluster-modes-2026-06-17.md`
//! (synthesised from ATAS, Sierra Chart, TradingView, Quantower,
//! NinjaTrader Order Flow+, MotiveWave, Bookmap, flowsurface-rs,
//! focus1691/orderflow, MQL5 reference).
//!
//! Textbook minimum-viable mode (ATAS: "the most informative
//! footprint"):
//! - Per active price level: two numbers `sell_vol × buy_vol` (bid
//!   left red, ask right green).
//! - One row per `tick_size` (or N-tick bucket).
//! - POC (Point of Control = level with max `buy_vol + sell_vol`)
//!   highlighted.
//! - Value Area (70% of total volume, greedy bidirectional expansion
//!   from POC) subtle background.
//! - Imbalance cells (`max / min >= 3.0` on the diagonal compare
//!   `ask[i] / bid[i-1]`) accent.
//! - At extreme zoom-out: collapse to a candlestick (ATAS, Bookmap).
//!
//! Pixel math (verified across vendors):
//! - bar_w = `chart_width / visible_bar_count` (uniform — footprint
//!   is index-axis like every other bar type).
//! - row_h = `chart_height / (price_range / tick_size)`, min 1px.
//! - font_size = `clamp(bar_w / 8.5, 7, 16)`.
//! - text suppressed when `bar_w < 40` or `font < 7`.
//!
//! Data plumbing pending:
//! - `ChartRenderState` needs an optional `footprint_bars` field.
//! - `LiveUpdate::FootprintUpdate` handler in `tick.rs:1250` needs to
//!   push into per-window `footprint_bars` storage (currently no-op).
//! - The bridge's `SharedFootprintSeries` already accumulates per
//!   `(exchange, account, symbol)`; the window just needs to grab the
//!   Arc when chart_type switches to footprint.
//!
//! Until that infra lands the public dispatch (`draw_footprint`)
//! falls back to `draw_candles` so the chart still renders something
//! meaningful (OHLCV summary per bar). Visual "lies": no cluster grid,
//! no per-level numbers, no POC/VA — but it's index-axis with the
//! correct timestamps, so the chart no longer ghosts onto a broken
//! time-axis like the pre-C4 cutover did.

use mlc_core::render::RenderContext;
use super::ChartRenderState;
use super::candles::draw_candles;

/// Draw the footprint cluster grid.
///
/// **Current behaviour: candle fallback.** Real renderer pending data
/// plumbing (see module docs). The dispatch site in
/// `super::candles::draw_footprint` routes here; callers do not need
/// to special-case the absence of footprint data.
pub fn draw_footprint_cluster(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    draw_candles(ctx, state);
}
