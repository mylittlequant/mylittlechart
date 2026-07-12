//! Footprint cluster chart rendering — Bid × Ask split per price level.
//!
//! Textbook spec sourced from
//! `nemo/docs/mlc/research/footprint-cluster-modes-2026-06-17.md`
//! (ATAS "Cluster Chart", Sierra Chart "Numbers Bars", TradingView
//! Volume Footprint, Quantower, NinjaTrader Order Flow+, MotiveWave
//! Volume Imprint, Bookmap Footprint addon, flowsurface-rs,
//! focus1691/orderflow, MQL5 Part 9/11).
//!
//! Implements the minimum-viable mode (ATAS: "the most informative
//! footprint"):
//! - Per price level: `sell_vol × buy_vol` rendered side-by-side,
//!   bid (red) left of bar center, ask (green) right of bar center.
//! - POC row (max total volume) gets a yellow highlight band across
//!   the bar's vertical strip.
//! - Value Area (70% of total volume, greedy bidirectional expansion
//!   from POC) gets a subtle background shade across the whole bar
//!   column.
//! - Imbalance cell on the diagonal: `ask[i] / bid[i-1] >= 3.0` →
//!   accent the ask cell green; symmetric for bid.
//! - At extreme zoom-out (bar width < 40px or font < 7) → collapse to
//!   candle (fall through to `draw_candles`), matching ATAS / Bookmap.
//!
//! Cell layout mode (`FootprintDisplayOptions::cell_mode`,
//! `chart-type-render-doctrine-2026-07-09.md` §8) selects what the per-row
//! foreground text/graphic and background heat show — `BidAsk` (default,
//! described above) / `Delta` / `Volume` / `VolumeDeltaColor` / `Profile`
//! (mini-histogram, no text). POC/VA fills, absorption markers, and
//! imbalance/stacked-imbalance highlighting are orthogonal to this mode and
//! stay active in all five.
//!
//! Pixel math (verbatim from MQL5 reference):
//! - bar_w = `viewport.bar_width()`
//! - row_h = `chart_height * tick_size / price_range`, min 1px
//! - font_size = `clamp(bar_w / 8.5, 7, 16)`
//! - bid column: right-aligned text at `x - bar_w/4`
//! - ask column: left-aligned text at `x + bar_w/4`
//!
//! Tick size: inferred from the most common price step between adjacent
//! levels of the latest bar with >= 2 levels; falls back to
//! `price_range / 50` if no inference is possible.

use mlc_core::chart::{ChartTheme, FootprintCellMode};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::types::FootprintBar;
use super::ChartRenderState;

/// Minimum bar width (px) before we render text. Below this, collapse
/// to a candle visual.
const MIN_BAR_WIDTH_PX: f64 = 40.0;
/// Minimum font size (px) considered legible. Below this, collapse.
const MIN_FONT_SIZE_PX: f64 = 7.0;
// Per-cell diagonal imbalance threshold — colors a single cell's digits
// when `ask[i] / bid[i-1]` (or the symmetric bid case) clears this ratio.
// User-configurable (owner-approved settings matrix, wave 1, item 8) via
// `FootprintDisplayOptions::imbalance_threshold` (canon default 1.5×
// / 150%, read at render time as `cell_imbalance_ratio` below — no local
// const), distinct from the stacked-zone run-detection threshold feeding
// `FootprintBar::stacked_ask_start`/`stacked_bid_start`
// (`mlc_core::types::FOOTPRINT_STACKED_IMBALANCE_RATIO`, computed
// station/fold-side at a fixed 3.0× — see
// `mlc-backend/src/bubble/view_fold.rs`'s `footprint_point_to_bar`). The
// two were previously conflated under one `IMBALANCE_RATIO = 3.0` constant
// here (canon-fix wave 2, item 2). This renderer only READS the resulting
// `stacked_ask_start`/`stacked_bid_start` markers — it never recomputes the
// stacked-zone ratio locally.

/// Value Area cumulative fraction (industry standard).
const VALUE_AREA_FRACTION: f64 = 0.70;

/// Color palette for the footprint cluster.
///
/// `bid`/`ask` (the per-cell volume NUMBERS) are deliberately independent
/// of the Buy/Sell theme swatches — bug fix 2026-07-06: the swatches are
/// documented (Instrument-tab CLUSTERS SETTINGS) as controlling the
/// heat-fill color, not the digit color, so digits stay a fixed
/// high-legibility red/green pair regardless of the user's fill choice.
/// See `heat_fill_colors()` for the theme-driven pair used by the fill.
struct FpColors<'a> {
    bid: &'a str,
    ask: &'a str,
    bid_imb: &'a str,
    ask_imb: &'a str,
    poc_fill: &'a str,
    va_fill: &'a str,
}

/// Fixed defaults — volume-text digit colors are NOT user-configurable
/// (only the heat-fill color is, via the Buy/Sell swatches — see
/// `heat_fill_colors()`).
const DEFAULT_COLORS: FpColors<'static> = FpColors {
    bid: "#ef5350",
    ask: "#26a69a",
    bid_imb: "#ff5252",
    ask_imb: "#69f0ae",
    poc_fill: "rgba(255, 235, 59, 0.18)",
    va_fill: "rgba(120, 144, 156, 0.10)",
};

/// Build the live palette for one frame. Digit colors stay the fixed
/// constant regardless of theme (see `FpColors` doc) — nothing here reads
/// `theme` today, but the signature is kept for symmetry with
/// `heat_fill_colors()` and in case a future non-heat-fill control wants
/// theme input.
fn theme_colors(_theme: &ChartTheme) -> FpColors<'static> {
    DEFAULT_COLORS
}

/// Convert a `#rrggbb`/`#rgb` hex color to an `rgba(r, g, b, a)` string.
/// Falls back to a neutral gray if the theme color fails to parse (should
/// not happen — theme colors are always validated hex on write).
fn hex_to_rgba(hex: &str, alpha: f64) -> String {
    let hex = hex.trim_start_matches('#');
    let (r, g, b) = if hex.len() == 6 {
        (
            u8::from_str_radix(&hex[0..2], 16).unwrap_or(128),
            u8::from_str_radix(&hex[2..4], 16).unwrap_or(128),
            u8::from_str_radix(&hex[4..6], 16).unwrap_or(128),
        )
    } else if hex.len() == 3 {
        (
            u8::from_str_radix(&hex[0..1], 16).unwrap_or(8) * 17,
            u8::from_str_radix(&hex[1..2], 16).unwrap_or(8) * 17,
            u8::from_str_radix(&hex[2..3], 16).unwrap_or(8) * 17,
        )
    } else {
        (120, 144, 156)
    };
    format!("rgba({}, {}, {}, {:.3})", r, g, b, alpha)
}

/// Heat-fill color pair — sourced from the Buy/Sell theme swatches
/// (`ChartTheme::footprint_buy`/`.footprint_sell`, user-editable via the
/// Instrument tab's CLUSTERS SETTINGS block). Bug fix 2026-07-06: the
/// heat fill used to be two hardcoded hex constants disconnected from the
/// swatches; now it reads the same theme fields the swatches write.
fn heat_fill_colors(theme: &ChartTheme) -> (&str, &str) {
    (&theme.footprint_buy, &theme.footprint_sell)
}

/// Stacked-imbalance + absorption palette (F1, F2).
struct FpExtraColors<'a> {
    stacked_ask_fill: &'a str,
    stacked_bid_fill: &'a str,
    absorption_fill: &'a str,
}

const EXTRA_COLORS: FpExtraColors<'static> = FpExtraColors {
    stacked_ask_fill: "rgba(105, 240, 174, 0.22)",
    stacked_bid_fill: "rgba(255, 82, 82, 0.22)",
    absorption_fill: "rgba(255, 152, 0, 0.16)",
};

// NOTE: the zoom-out `draw_candle_fallback` (footprint → plain candles below
// cluster-text readability) was removed 2026-07-05 — owner rule: footprint
// never morphs into a candle chart. Compressed zoom now keeps the cluster
// cells as dominant-side heat rects (see the `draw_text` gate in
// `draw_footprint_cluster`); only per-level bars WITHOUT levels (historical
// kline-approx tail) still draw a candle body, which is honest data shape.

/// Entry point — `super::candles::draw_footprint` routes here.
///
/// Hybrid mode (hybrid-data-modes-plan-2026-07-08, Step 3): on a
/// `RecentOnly` venue footprint runs hybrid — `state.bars` is the REAL
/// kline backdrop (kept alive by the engine's subscribe switch point, see
/// `mlc-backend::chart_type::engine::subscribe_footprint`) and
/// `state.footprint_bars` is only the live-accumulated tail, permanently
/// shorter than the backdrop. This entry draws the kline backdrop across
/// the FULL visible range first (reusing `draw_candles` as-is — it already
/// clips/iterates `state.bars` independently), then overlays cluster cells
/// ONLY for the suffix of the visible range that has a matching
/// `footprint_bars` entry, aligned by index from the tail
/// (`cluster_offset = backdrop_len - cluster_len`, valid under the Step 3
/// invariant that the kline backdrop TF always equals the footprint bucket
/// interval — see `chart_out_events.rs`'s hybrid backdrop-TF-alignment
/// block). In takeover mode (`cluster_len >= backdrop_len`, the ordinary
/// case on deep-history venues) `cluster_offset` is 0 and behavior is
/// unchanged from before this arc.
pub fn draw_footprint_cluster(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let fp_bars = state.footprint_bars.filter(|b| !b.is_empty());
    if fp_bars.is_none() && state.bars.is_empty() {
        // No footprint data yet AND no kline backdrop either — genuinely
        // nothing to show. Owner needs to see "we have nothing", not a
        // fake series.
        super::draw_waiting_placeholder(ctx, state, "Footprint: waiting for data…");
        return;
    }

    let backdrop_len = state.bars.len();
    let cluster_len = fp_bars.map_or(0, <[FootprintBar]>::len);
    // Hybrid signature: the backdrop (kline, `state.bars`) is strictly
    // LONGER than the cluster array (`footprint_bars`) — takeover mode's
    // OHLC-mirror keeps both arrays in lockstep (same length, written by
    // the same event handler), so `backdrop_len == cluster_len` there and
    // this stays false. Only draw the separate candle-backdrop PASS in the
    // genuinely hybrid case — takeover mode's existing per-bar
    // empty-levels candle fallback (below, in the main loop) already
    // covers its own narrower gap shape and must not get a second,
    // redundant candle body drawn underneath every cluster bar.
    let is_hybrid = backdrop_len > cluster_len;
    if is_hybrid {
        // Kline backdrop pass — draws its own visible range independently
        // (including the Glass-style clip extension).
        super::candles::draw_candles(ctx, state);
    }

    let Some(bars) = fp_bars else {
        // Hybrid cold-open: kline backdrop just drawn above (or nothing to
        // draw at all if `state.bars` was also empty — caught by the
        // placeholder branch above). No live clusters accumulated yet.
        return;
    };

    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let rect = &state.chart_rect;
    let bar_w = viewport.bar_width();

    // Zoomed out below cluster-text readability — clusters STAY clusters
    // (owner rule 2026-07-05: footprint must never morph into plain
    // candles). Below the text threshold we keep drawing every cell as a
    // colored heat rect (dominant-side color, alpha by volume share) and
    // only drop the per-level numbers/markers that would be illegible.
    let font_size = ((bar_w / 8.5).clamp(MIN_FONT_SIZE_PX, 16.0)).round();
    // User toggle (Instrument tab, "Volume text" checkbox) sits ON TOP of
    // the existing legibility gate — either one being false suppresses the
    // per-cell numbers; the heat-fill cells stay visible regardless.
    let draw_text = bar_w >= MIN_BAR_WIDTH_PX
        && font_size >= MIN_FONT_SIZE_PX
        && state.footprint_display.show_volume_text;
    let show_abs_markers = state.footprint_display.show_abs_markers;
    let heat_alpha_mult = state.footprint_display.heat_intensity.alpha_multiplier();
    // Per-cell diagonal imbalance ratio (owner-approved settings matrix,
    // wave 1, item 8) — user-configurable via the modal's "Imbalance
    // ratio" dropdown (1.5x/2x/3x/4x). Replaces the previously-hardcoded
    // `CELL_IMBALANCE_RATIO` constant read below. Distinct from the
    // STACKED ratio (`FOOTPRINT_STACKED_IMBALANCE_RATIO` = 3.0), which
    // stays a separate fold-side constant untouched by this control.
    let cell_imbalance_ratio = state.footprint_display.imbalance_threshold;
    // Cell layout mode (chart-type-render-doctrine 2026-07-09 §8) — decides
    // what the per-row foreground text/graphic and background heat show.
    // Orthogonal to everything else in this function: imbalance
    // highlighting, stacked-imbalance zones, POC/VA fills, and absorption
    // markers are computed and drawn identically in every mode; only the
    // per-level dispatch below (and, for `Profile`, the text-suppression
    // check) reads this value.
    let cell_mode = state.footprint_display.cell_mode;

    let colors = theme_colors(state.theme);
    let price_range = price_scale.price_max - price_scale.price_min;
    if price_range <= 0.0 || !price_range.is_finite() {
        return;
    }

    let tick_size = infer_tick_size(bars).max(price_range / 200.0);
    let row_h = (rect.height * tick_size / price_range).max(1.0);

    // Cluster cells are the newest `cluster_len` entries of the backdrop's
    // index axis — aligned to the TAIL, not the head (Step 3 index math).
    // In takeover mode `backdrop_len == cluster_len` (OHLC-mirror lockstep)
    // — `cluster_offset` collapses to 0 and every visible index maps 1:1
    // into `bars`, matching today's behavior exactly.
    let cluster_offset = backdrop_len.saturating_sub(cluster_len);

    // Visible range in the ACTIVE-SERIES index domain — viewport bookkeeping
    // for footprint keys off `bubble.bars` (see active_series.rs), i.e. the
    // backdrop length in hybrid mode, or `bars.len()` itself in takeover
    // mode where `backdrop_len == cluster_len`.
    let (vp_start, vp_end) = viewport.visible_range();
    let axis_len = backdrop_len.max(cluster_len);
    let vp_end = vp_end.min(axis_len);
    if vp_start >= vp_end {
        return;
    }
    // Intersect the visible range with the cluster sub-range
    // `[cluster_offset, axis_len)`, then map into `bars`' own index space.
    let start = vp_start.max(cluster_offset).saturating_sub(cluster_offset);
    let end = vp_end.saturating_sub(cluster_offset).min(bars.len());
    if start >= end {
        return;
    }

    // First: draw a thin OHLC wick line per bar so price excursion is
    // still readable behind the cluster text. In hybrid mode the backdrop
    // pass above already drew wicks for the WHOLE visible range (including
    // this cluster sub-range) — redraw here anyway for the cluster slice so
    // takeover mode (no separate backdrop pass) keeps its wicks, and the
    // hybrid tail gets the exact same wick styling regardless of source.
    draw_wicks(ctx, state, bars, start, end, bar_w, cluster_offset);

    // Then the per-level cluster grid. Bars with empty levels (historical
    // klines-only window — no per-trade data that far back) fall back to a
    // plain OHLC candle body so history reads as candles until clusters
    // exist; wick lines are already drawn above for every bar.
    let mut bars_drawn = 0_usize;
    let mut bars_skipped_empty = 0_usize;
    for i in start..end {
        let bar = &bars[i];
        // Global viewport index — see `draw_wicks`' `index_offset` doc.
        let gi = i + cluster_offset;
        if bar.levels.is_empty() {
            bars_skipped_empty += 1;
            let cx = viewport.bar_to_x(gi) + rect.x;
            let body_w = (bar_w * 0.6).max(1.0);
            if cx + body_w * 0.5 < rect.x || cx - body_w * 0.5 > rect.x + rect.width {
                continue;
            }
            let theme = state.theme;
            let up = bar.close >= bar.open;
            let oy = rect.y + price_to_y_local(bar.open, price_scale, rect.height);
            let cy = rect.y + price_to_y_local(bar.close, price_scale, rect.height);
            ctx.set_fill_color(if up { &theme.candle_up } else { &theme.candle_down });
            ctx.fill_rect(cx - body_w * 0.5, oy.min(cy), body_w, (oy - cy).abs().max(1.0));
            continue;
        }
        bars_drawn += 1;
        let cx = viewport.bar_to_x(gi) + rect.x;
        let cell_left  = cx - bar_w * 0.5;
        let cell_right = cx + bar_w * 0.5;

        // Skip bars entirely off-screen horizontally.
        if cell_right < rect.x || cell_left > rect.x + rect.width {
            continue;
        }

        // Quantize dig3's raw per-trade levels down to the inferred
        // display tick_size so ~25-40 rows land per bar (industry norm)
        // instead of thousands of one-tick smears that render as a
        // colored band without readable numbers.
        let binned = quantize_levels(bar, tick_size);
        let mut bar = bar.clone();
        bar.levels = binned;
        let bar = &bar;

        let poc_idx = bar.poc_idx();
        let va = bar.value_area(VALUE_AREA_FRACTION);

        // Value Area background strip (covers the whole column band).
        if let (Some(poc), Some((va_lo, va_hi))) = (poc_idx, va) {
            let top_price = bar.levels[va_hi].price;
            let bot_price = bar.levels[va_lo].price;
            let top_y = rect.y + price_to_y_local(top_price, price_scale, rect.height);
            let bot_y = rect.y + price_to_y_local(bot_price, price_scale, rect.height) + row_h;
            let h = (bot_y - top_y).max(row_h);
            ctx.set_fill_color(colors.va_fill);
            ctx.fill_rect(cell_left, top_y, bar_w, h);
            let _ = poc;
        }

        // F2: absorption bar tint clipped to the actual price excursion
        // (high → low) per MQL5 Part 11 spec. The predecessor painted
        // the full chart vertical; that washed out everything else when
        // an absorption bar was visible. Gated by the "ABS markers" toggle
        // (Instrument tab) together with the "ABS" text label below —
        // both are the same absorption-marker visual.
        if show_abs_markers && bar.is_absorption {
            let hy = rect.y + price_to_y_local(bar.high, price_scale, rect.height);
            let ly = rect.y + price_to_y_local(bar.low, price_scale, rect.height) + row_h;
            ctx.set_fill_color(EXTRA_COLORS.absorption_fill);
            ctx.fill_rect(cell_left, hy.min(ly), bar_w, (ly - hy).abs().max(row_h));
        }

        // F1: stacked imbalance zones — tinted row band across the
        // stacked levels (ascending price). Drawn AFTER VA so the stack
        // dominates the visual when both regions overlap. Uses the real
        // streak length recorded by compute_extras instead of the
        // hardcoded +2 the predecessor assumed.
        // Both stacked-band indices are CLAMPED to the bar's actual level
        // count — a bar whose extras were computed against a different
        // (longer) level array must degrade to a collapsed band, never
        // panic: a wasm index-out-of-bounds aborts the WHOLE module (live
        // incident 2026-07-13: `len is 11 but the index is 382` at deep
        // zoom-out froze the tab — dead crosshair, no reactions; looked
        // like OOM, was this line). The predecessor clamped only `hi_idx`.
        if !bar.levels.is_empty() {
            if let Some(start_idx) = bar.stacked_ask_start {
                let len = bar.stacked_ask_len.unwrap_or(3).max(1);
                let start_idx = start_idx.min(bar.levels.len() - 1);
                let hi_idx = (start_idx + len - 1).min(bar.levels.len() - 1);
                let lo_lv = &bar.levels[start_idx];
                let hi_lv = &bar.levels[hi_idx];
                let top_y = rect.y + price_to_y_local(hi_lv.price, price_scale, rect.height);
                let bot_y = rect.y + price_to_y_local(lo_lv.price, price_scale, rect.height) + row_h;
                ctx.set_fill_color(EXTRA_COLORS.stacked_ask_fill);
                ctx.fill_rect(cell_left, top_y, bar_w, (bot_y - top_y).max(row_h));
            }
            if let Some(start_idx) = bar.stacked_bid_start {
                let len = bar.stacked_bid_len.unwrap_or(3).max(1);
                let start_idx = start_idx.min(bar.levels.len() - 1);
                let hi_idx = (start_idx + len - 1).min(bar.levels.len() - 1);
                let lo_lv = &bar.levels[start_idx];
                let hi_lv = &bar.levels[hi_idx];
                let top_y = rect.y + price_to_y_local(hi_lv.price, price_scale, rect.height);
                let bot_y = rect.y + price_to_y_local(lo_lv.price, price_scale, rect.height) + row_h;
                ctx.set_fill_color(EXTRA_COLORS.stacked_bid_fill);
                ctx.fill_rect(cell_left, top_y, bar_w, (bot_y - top_y).max(row_h));
            }
        }

        // Max volume on this bar (for dim cell-background gradient).
        let max_vol = bar.levels.iter().map(|l| l.total_vol()).fold(0.0_f64, f64::max);
        // F3: 30% significance gate — only color cells whose own side's
        // share of the bar's max same-side volume is meaningful. Stops
        // tiny-cell imbalances from over-coloring.
        let max_buy = bar.levels.iter().map(|l| l.buy_vol).fold(0.0_f64, f64::max);
        let max_sell = bar.levels.iter().map(|l| l.sell_vol).fold(0.0_f64, f64::max);
        let sig_buy_gate = max_buy * 0.30;
        let sig_sell_gate = max_sell * 0.30;

        // Per-level rendering.
        let bid_x = cx - bar_w * 0.25;
        let ask_x = cx + bar_w * 0.25;

        ctx.set_font(&format!("{}px sans-serif", font_size as i32));

        for (li, lv) in bar.levels.iter().enumerate() {
            let y = rect.y + price_to_y_local(lv.price, price_scale, rect.height);
            if y + row_h < rect.y || y > rect.y + rect.height {
                continue;
            }

            // POC row tinted highlight (unchanged in every mode — POC/VA
            // fills are orthogonal to cell_mode per doctrine §8).
            if Some(li) == poc_idx {
                ctx.set_fill_color(colors.poc_fill);
                ctx.fill_rect(cell_left, y, bar_w, row_h);
            } else if max_vol > 0.0 {
                // Background heat-fill: dominant-side (BidAsk/Volume) or
                // delta-sign (Delta/VolumeDeltaColor) tint. `Profile` draws
                // no cell background — the mini-histogram bars themselves
                // carry the volume/delta signal instead. Bug fix
                // 2026-07-06 (preserved): applied in BOTH zoom regimes, not
                // just `!draw_text`, so Heat-intensity stays visible at
                // normal zoom. Alpha cap stays lower when text is drawn on
                // top (must not wash out legible numbers) and higher when
                // fully zoomed out (fill alone carries structure).
                let min_ratio = if draw_text { 0.05 } else { 0.02 };
                match cell_mode {
                    FootprintCellMode::BidAsk | FootprintCellMode::Volume => {
                        let ratio = (lv.total_vol() / max_vol).clamp(0.0, 1.0);
                        if ratio > min_ratio {
                            let base_cap = if draw_text { 0.08 + ratio * 0.35 } else { 0.12 + ratio * 0.55 };
                            let alpha = (base_cap * heat_alpha_mult).min(1.0);
                            let (buy_hex, sell_hex) = heat_fill_colors(state.theme);
                            let color = if lv.buy_vol >= lv.sell_vol {
                                hex_to_rgba(buy_hex, alpha)
                            } else {
                                hex_to_rgba(sell_hex, alpha)
                            };
                            ctx.set_fill_color(&color);
                            ctx.fill_rect(cell_left, y, bar_w, row_h);
                        }
                    }
                    FootprintCellMode::Delta | FootprintCellMode::VolumeDeltaColor => {
                        // Delta-sign tint, magnitude-scaled by |delta| share
                        // of the bar's max same-side volume (reuses the same
                        // significance gates as the diagonal-imbalance
                        // detector below, so background heat and imbalance
                        // highlighting agree on what counts as "significant").
                        let max_abs_delta = bar.levels.iter().map(|l| l.delta().abs()).fold(0.0_f64, f64::max);
                        if max_abs_delta > 0.0 {
                            let ratio = (lv.delta().abs() / max_abs_delta).clamp(0.0, 1.0);
                            if ratio > min_ratio {
                                let base_cap = if draw_text { 0.08 + ratio * 0.35 } else { 0.12 + ratio * 0.55 };
                                let alpha = (base_cap * heat_alpha_mult).min(1.0);
                                let (buy_hex, sell_hex) = heat_fill_colors(state.theme);
                                let color = if lv.delta() >= 0.0 {
                                    hex_to_rgba(buy_hex, alpha)
                                } else {
                                    hex_to_rgba(sell_hex, alpha)
                                };
                                ctx.set_fill_color(&color);
                                ctx.fill_rect(cell_left, y, bar_w, row_h);
                            }
                        }
                    }
                    FootprintCellMode::Profile => {}
                }
            }

            // F4: single print dashed border.
            if draw_text && lv.is_single_print {
                ctx.set_stroke_color("rgba(255, 255, 255, 0.55)");
                ctx.set_stroke_width(1.0);
                ctx.set_line_dash(&[3.0, 2.0]);
                ctx.stroke_rect(cell_left + 0.5, y + 0.5, bar_w - 1.0, row_h - 1.0);
                ctx.set_line_dash(&[]);
            }
            // F4: unfinished auction dot marker (at bar extreme with one
            // side zero) — placed at the relevant cell edge.
            if draw_text && lv.is_unfinished {
                ctx.set_fill_color("#ffd54f");
                let dot_r = (row_h * 0.18).clamp(2.0, 4.0);
                let dx = if lv.buy_vol == 0.0 { cell_right - dot_r - 1.0 } else { cell_left + dot_r + 1.0 };
                let dy = y + row_h * 0.5;
                ctx.begin_path();
                ctx.arc(dx, dy, dot_r, 0.0, std::f64::consts::TAU);
                ctx.fill();
            }

            // Diagonal imbalance: ask[i] vs bid[i-1] (ask-imbalance),
            // bid[i] vs ask[i+1] (bid-imbalance). With 30% significance
            // gate (F3): the *current* side's volume must clear 30% of
            // the bar's same-side max before we color.
            let ask_imb = li > 0
                && lv.buy_vol > sig_buy_gate
                && bar.levels[li - 1].sell_vol > 0.0
                && lv.buy_vol / bar.levels[li - 1].sell_vol >= cell_imbalance_ratio;
            let bid_imb = li + 1 < bar.levels.len()
                && lv.sell_vol > sig_sell_gate
                && bar.levels[li + 1].buy_vol > 0.0
                && lv.sell_vol / bar.levels[li + 1].buy_vol >= cell_imbalance_ratio;

            // Per-cell content dispatch (doctrine §8 "Cell layout modes").
            // Imbalance highlighting (`ask_imb`/`bid_imb`, computed above)
            // stays active in every mode — it only changes the digit color
            // in the text modes, same as it always did for `BidAsk`.
            match cell_mode {
                FootprintCellMode::BidAsk => {
                    // Bid (sell vol) on the LEFT, right-aligned.
                    if draw_text && lv.sell_vol > 0.0 {
                        let color = if bid_imb { colors.bid_imb } else { colors.bid };
                        ctx.set_fill_color(color);
                        ctx.set_text_align(TextAlign::Right);
                        ctx.set_text_baseline(TextBaseline::Middle);
                        let label = fmt_vol(lv.sell_vol);
                        ctx.fill_text(&label, bid_x, y + row_h * 0.5);
                    }
                    // Ask (buy vol) on the RIGHT, left-aligned.
                    if draw_text && lv.buy_vol > 0.0 {
                        let color = if ask_imb { colors.ask_imb } else { colors.ask };
                        ctx.set_fill_color(color);
                        ctx.set_text_align(TextAlign::Left);
                        ctx.set_text_baseline(TextBaseline::Middle);
                        let label = fmt_vol(lv.buy_vol);
                        ctx.fill_text(&label, ask_x, y + row_h * 0.5);
                    }
                }
                FootprintCellMode::Delta => {
                    // Single signed number (ask - bid), centered; text
                    // colored green/positive red/negative using the same
                    // digit-color pair the split layout uses for its sides
                    // (`colors.ask`/`colors.bid`), imbalance-highlighted the
                    // same way.
                    if draw_text {
                        let delta = lv.delta();
                        if delta != 0.0 {
                            let positive = delta > 0.0;
                            let color = match (positive, if positive { ask_imb } else { bid_imb }) {
                                (true, true) => colors.ask_imb,
                                (true, false) => colors.ask,
                                (false, true) => colors.bid_imb,
                                (false, false) => colors.bid,
                            };
                            ctx.set_fill_color(color);
                            ctx.set_text_align(TextAlign::Center);
                            ctx.set_text_baseline(TextBaseline::Middle);
                            let label = format!("{}{}", if positive { "+" } else { "-" }, fmt_vol(delta.abs()));
                            ctx.fill_text(&label, cx, y + row_h * 0.5);
                        }
                    }
                }
                FootprintCellMode::Volume | FootprintCellMode::VolumeDeltaColor => {
                    // Single unsigned total (bid + ask), centered, neutral
                    // text color regardless of imbalance/delta sign — the
                    // background (heat-fill dispatch above) is what encodes
                    // sign/magnitude for `VolumeDeltaColor`.
                    if draw_text {
                        let total = lv.total_vol();
                        if total > 0.0 {
                            ctx.set_fill_color(&state.theme.text);
                            ctx.set_text_align(TextAlign::Center);
                            ctx.set_text_baseline(TextBaseline::Middle);
                            let label = fmt_vol(total);
                            ctx.fill_text(&label, cx, y + row_h * 0.5);
                        }
                    }
                }
                FootprintCellMode::Profile => {
                    // No text — horizontal mini-histogram, left-anchored at
                    // the cell's left edge (ExoCharts/ATAS in-bar profile):
                    // bar LENGTH = this row's total volume share of the
                    // bar's max row volume × full cell width; the bar is
                    // SPLIT into bid|ask segments by their share of the
                    // row's own total (length encodes volume, split point
                    // encodes delta composition). Ignores the `draw_text`
                    // legibility gate (bars draw at any width).
                    let total = lv.total_vol();
                    if max_vol > 0.0 && total > 0.0 {
                        let len = ((total / max_vol).clamp(0.0, 1.0) * (bar_w - 2.0)).max(1.0);
                        let bar_h = (row_h - 2.0).max(1.0);
                        let bar_y = y + (row_h - bar_h) * 0.5;
                        let bid_w = len * (lv.sell_vol / total);
                        let ask_w = len - bid_w;
                        if bid_w > 0.0 {
                            ctx.set_fill_color(&state.theme.candle_down);
                            ctx.fill_rect(cell_left + 1.0, bar_y, bid_w, bar_h);
                        }
                        if ask_w > 0.0 {
                            ctx.set_fill_color(&state.theme.candle_up);
                            ctx.fill_rect(cell_left + 1.0 + bid_w, bar_y, ask_w, bar_h);
                        }
                    }
                }
            }
        }

        // F1 zone-edge triangle markers (layer 8).
        if draw_text && bar.stacked_ask_start.is_some() {
            ctx.set_fill_color(colors.ask_imb);
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.fill_text("▲", cx, rect.y + 2.0);
        }
        if draw_text && bar.stacked_bid_start.is_some() {
            ctx.set_fill_color(colors.bid_imb);
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Bottom);
            ctx.fill_text("▼", cx, rect.y + rect.height - 2.0);
        }

        // F2: "ABS" info label on absorption bars.
        if show_abs_markers && draw_text && bar.is_absorption {
            ctx.set_fill_color("rgba(255, 152, 0, 0.95)");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.set_font(&format!("{}px sans-serif", (font_size as i32 - 1).max(7)));
            ctx.fill_text("ABS", cx, rect.y + 14.0);
        }

        // NOTE: the per-column "faint vertical separator" that used to be
        // drawn here at cell_right was killed 2026-07-05 — full-height lines
        // at column EDGES read as a second, misaligned grid next to the
        // standard time grid (which passes through bar CENTERS). The
        // standard grid is back for footprint (active_series.rs
        // chart_type_hides_time_grid) and is the only vertical ruling.
    }

    eprintln!(
        "[fp-render] visible_range=[{}..{}) drawn_with_clusters={} skipped_empty_levels={} tick_size={:.4} row_h={:.2}",
        start, end, bars_drawn, bars_skipped_empty, tick_size, row_h,
    );

    // Reset text alignment so subsequent renderers do not inherit our
    // RIGHT/LEFT alignment.
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_line_dash(&[]);
}

/// Draw a thin wick from each bar's low to high so the candle silhouette
/// remains visible behind the cluster numbers.
///
/// `index_offset` shifts `[start, end)` (local indices into `bars`) into
/// the viewport's GLOBAL index domain for `bar_to_x` — 0 in takeover mode
/// (`bars` IS the active series), `cluster_offset` in hybrid mode (`bars`
/// is only the live-accumulated tail of a longer kline backdrop).
fn draw_wicks(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    bars: &[FootprintBar],
    start: usize,
    end: usize,
    bar_w: f64,
    index_offset: usize,
) {
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let rect = &state.chart_rect;
    let theme = state.theme;
    ctx.set_stroke_width(1.0);
    ctx.set_line_dash(&[]);
    for i in start..end {
        let bar = &bars[i];
        let cx = viewport.bar_to_x(i + index_offset) + rect.x;
        let is_up = bar.is_bullish();
        let color = if is_up { &theme.wick_up } else { &theme.wick_down };
        ctx.set_stroke_color(color);
        let hy = rect.y + price_to_y_local(bar.high, price_scale, rect.height);
        let ly = rect.y + price_to_y_local(bar.low, price_scale, rect.height);
        ctx.begin_path();
        ctx.move_to(cx, hy);
        ctx.line_to(cx, ly);
        ctx.stroke();
        let _ = bar_w;
    }
}

/// Format a volume number compactly. < 100: 1 decimal; >= 1000: K; >= 1e6: M.
fn fmt_vol(v: f64) -> String {
    let v = v.max(0.0);
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.1}K", v / 1_000.0)
    } else if v >= 100.0 {
        format!("{:.0}", v)
    } else if v >= 10.0 {
        format!("{:.1}", v)
    } else {
        format!("{:.2}", v)
    }
}

/// Best-effort quantization step sized for ~32 visible rows per bar, derived
/// from the high-low span of the most recent bars (not level spacing).
fn infer_tick_size(bars: &[FootprintBar]) -> f64 {
    // Aim for ~25-40 visible rows per bar (industry-standard density for
    // legible cluster grids). The dig3 aggregator groups by raw f64 price,
    // so each trade tick becomes its own level — thousands of rows per bar
    // is the default, which renders as a vertical noise smear at typical
    // chart heights. Quantize down to a "round" step picked from the
    // ladder {0.01, 0.05, 0.1, 0.5, 1, 5, 10, 50, 100, 500, 1000}.
    const ROW_TARGET: f64 = 32.0;
    const STEP_LADDER: &[f64] = &[
        0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 50.0, 100.0, 500.0, 1000.0,
    ];
    let mut best_span = 0.0_f64;
    for bar in bars.iter().rev().take(8) {
        if bar.high > bar.low && bar.high.is_finite() && bar.low.is_finite() {
            let span = bar.high - bar.low;
            if span > best_span {
                best_span = span;
            }
        }
    }
    if best_span <= 0.0 {
        return 1.0;
    }
    let raw = best_span / ROW_TARGET;
    // Snap up to the next ladder step.
    for &s in STEP_LADDER {
        if s >= raw {
            return s;
        }
    }
    *STEP_LADDER.last().unwrap()
}

/// Bin a bar's per-trade levels into a coarser `tick_size` ladder so the
/// cluster grid has ~25-40 legible rows instead of thousands of single-
/// trade entries from the dig3 aggregator.
fn quantize_levels(bar: &FootprintBar, tick_size: f64) -> Vec<mlc_core::types::FootprintLevel> {
    if tick_size <= 0.0 || bar.levels.is_empty() {
        return bar.levels.clone();
    }
    use std::collections::BTreeMap;
    let mut buckets: BTreeMap<i64, (f64, f64)> = BTreeMap::new();
    for lv in &bar.levels {
        let key = (lv.price / tick_size).round() as i64;
        let entry = buckets.entry(key).or_insert((0.0, 0.0));
        entry.0 += lv.buy_vol;
        entry.1 += lv.sell_vol;
    }
    buckets.into_iter().map(|(key, (buy, sell))| {
        mlc_core::types::FootprintLevel {
            price: key as f64 * tick_size,
            buy_vol: buy,
            sell_vol: sell,
            is_single_print: false,
            is_unfinished: false,
        }
    }).collect()
}

fn price_to_y_local(price: f64, ps: &mlc_core::chart::types::PriceScale, h: f64) -> f64 {
    let range = ps.price_max - ps.price_min;
    if range <= 0.0 {
        return h * 0.5;
    }
    h * (1.0 - (price - ps.price_min) / range)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlc_core::types::{FootprintBar, FootprintLevel};

    fn mk_bar(levels: Vec<(f64, f64, f64)>) -> FootprintBar {
        let lvs: Vec<FootprintLevel> = levels
            .into_iter()
            .map(|(p, b, s)| FootprintLevel { price: p, buy_vol: b, sell_vol: s, is_single_print: false, is_unfinished: false })
            .collect();
        let total: f64 = lvs.iter().map(|l| l.total_vol()).sum();
        let (open, close) = (lvs.first().map(|l| l.price).unwrap_or(0.0), lvs.last().map(|l| l.price).unwrap_or(0.0));
        let high = lvs.iter().map(|l| l.price).fold(f64::NEG_INFINITY, f64::max);
        let low  = lvs.iter().map(|l| l.price).fold(f64::INFINITY, f64::min);
        FootprintBar { open_time: 0, open, high, low, close, volume: total, levels: lvs, cumulative_delta: 0.0, stacked_ask_start: None, stacked_ask_len: None, stacked_bid_start: None, stacked_bid_len: None, is_absorption: false }
    }

    #[test]
    fn poc_picks_max_volume_level() {
        let bar = mk_bar(vec![
            (100.0, 1.0, 2.0),
            (101.0, 5.0, 4.0), // POC: 9.0
            (102.0, 0.5, 0.5),
        ]);
        assert_eq!(bar.poc_idx(), Some(1));
    }

    #[test]
    fn value_area_expands_70_percent_around_poc() {
        let bar = mk_bar(vec![
            (100.0, 1.0, 1.0),  // 2
            (101.0, 5.0, 5.0),  // 10 POC
            (102.0, 3.0, 3.0),  // 6
            (103.0, 1.0, 1.0),  // 2
        ]);
        // total=20, target=14. POC=10. Next-highest neighbour 102 (6) → 16 ≥ 14.
        let (lo, hi) = bar.value_area(0.70).unwrap();
        assert_eq!((lo, hi), (1, 2));
    }

    #[test]
    fn infer_tick_size_snaps_span_to_ladder_step() {
        // High-low span = 1.5, ROW_TARGET = 32.0 => raw = 1.5/32 ≈ 0.0469.
        // First ladder step >= raw is 0.05.
        let bar = mk_bar(vec![
            (100.0, 1.0, 1.0),
            (100.5, 1.0, 1.0),
            (101.5, 1.0, 1.0),
        ]);
        assert!((infer_tick_size(&[bar]) - 0.05).abs() < 1e-9);
    }

    #[test]
    fn infer_tick_size_picks_largest_span_of_last_8_bars() {
        // Only the trailing 8 bars are considered, and the widest span among
        // them wins even if it isn't the most recent bar.
        let narrow = mk_bar(vec![(100.0, 1.0, 1.0), (100.2, 1.0, 1.0)]); // span 0.2
        let wide = mk_bar(vec![(100.0, 1.0, 1.0), (500.0, 1.0, 1.0)]); // span 400
        // wide/32 = 12.5 => next ladder step is 50.0
        assert!((infer_tick_size(&[wide, narrow]) - 50.0).abs() < 1e-9);
    }

    #[test]
    fn infer_tick_size_degenerate_span_defaults_to_one() {
        let flat = mk_bar(vec![(100.0, 1.0, 1.0)]);
        assert!((infer_tick_size(&[flat]) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn fmt_vol_unit_buckets() {
        assert_eq!(fmt_vol(0.05),   "0.05");
        assert_eq!(fmt_vol(12.5),   "12.5");
        assert_eq!(fmt_vol(345.0),  "345");
        assert_eq!(fmt_vol(1_500.0), "1.5K");
        assert_eq!(fmt_vol(2_500_000.0), "2.5M");
    }
}
