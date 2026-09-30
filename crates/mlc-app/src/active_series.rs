//! ActiveSeries seam (audit D8, `chart-types-xaxis-audit-2026-07-03.md` §3 /
//! `viewport-data-lifecycle-full-audit-2026-07-03.md` D8).
//!
//! Viewport bookkeeping (bar_count, reconcile anchors, sync-group ts→index,
//! time-scale labels, drawings ts→x, Y autoscale) was permanently keyed to
//! the KLINE array (`bubble.bars`) even for chart types that render their
//! OWN array (renko/pnf/kagi/three_line_break) — "two arrays, one index".
//! `ActiveSeries` is the one accessor that resolves "which array does THIS
//! chart_type actually render" so every bookkeeping call site can key off
//! the right length/timestamps/price-range instead of hard-wired
//! `bubble.bars`.
//!
//! This is UI-core bookkeeping vocabulary (which array feeds the viewport
//! for a given chart_type), not a domain type — it lives in `mlc-app`
//! alongside `ChartContainer`, not in `mlc-core`.

use mlc_core::Bar;
use mlc_core::types::{CvdSample, KagiSegment, PnfColumn, RenkoBrick, ThreeLineBreakLine, TpoSession};

use mlc_backend::bubble::{BubbleStore, DataBubble};
use mlc_core::ids::BubbleId;

/// The array a chart_type actually renders and paces its viewport off of.
///
/// Resolved once per bookkeeping call via [`resolve_active_series`]. Each
/// variant borrows straight out of the bubble — no cloning at resolve time.
#[derive(Clone, Copy)]
pub enum ActiveSeries<'a> {
    /// Kline-family (candles/bars/line/area/...), range/volume/tick/dollar
    /// bars (station writes station-derived series directly into
    /// `bubble.bars`, so they ARE the kline array), footprint (needs the
    /// kline backdrop to stay in sync), dom/liquidation heatmap (own
    /// ring-buffer bookkeeping is intentionally ts-bucket-vs-kline-index,
    /// see `dom_heatmap.rs`/`liquidation_heatmap.rs`), and CVD (ts-domain
    /// is deliberately the visible KLINE range — `cvd.rs` draw_cvd_line_into_rect).
    Bars(&'a [Bar]),
    Renko(&'a [RenkoBrick]),
    Pnf(&'a [PnfColumn]),
    Kagi(&'a [KagiSegment]),
    Tlb(&'a [ThreeLineBreakLine]),
    /// Unreachable via [`resolve_active_series`] as of the 2026-07-04
    /// taxonomy fix — CVD was demoted from a full-pane chart_type (which
    /// used to resolve here) to a subpane overlay whose viewport now
    /// borrows kline bookkeeping (`ActiveSeries::Bars`), like OI/Funding.
    /// Kept (not deleted) per task scope — no wide refactor of the match
    /// arms below that still exhaustively handle it.
    Cvd(&'a [CvdSample]),
    /// TPO sessions — one index slot per session, positioned via
    /// `viewport.x_for_bar`/`visible_range` like renko/pnf/kagi/tlb
    /// (`chart_render/tpo.rs`'s `draw_tpo`). `len()`/`ts_ms_at`/`y_range`
    /// feed the same viewport bookkeeping (bar_count, snap, Y autoscale) as
    /// every other derived series.
    Tpo(&'a [TpoSession]),
}

/// Spacing assumed when a series is too short to have a measured gap.
const FALLBACK_GAP_MS: f64 = 60_000.0;

impl<'a> ActiveSeries<'a> {
    /// Number of elements in the active array.
    pub fn len(&self) -> usize {
        match self {
            ActiveSeries::Bars(s) => s.len(),
            ActiveSeries::Renko(s) => s.len(),
            ActiveSeries::Pnf(s) => s.len(),
            ActiveSeries::Kagi(s) => s.len(),
            ActiveSeries::Tlb(s) => s.len(),
            ActiveSeries::Cvd(s) => s.len(),
            ActiveSeries::Tpo(s) => s.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Timestamp (milliseconds) of the element at `idx`, if in range.
    pub fn ts_ms_at(&self, idx: usize) -> Option<i64> {
        match self {
            ActiveSeries::Bars(s) => s.get(idx).map(|b| b.ts_ms()),
            ActiveSeries::Renko(s) => s.get(idx).map(|b| b.ts_ms()),
            ActiveSeries::Pnf(s) => s.get(idx).map(|c| c.ts_ms()),
            ActiveSeries::Kagi(s) => s.get(idx).map(|k| k.ts_ms()),
            ActiveSeries::Tlb(s) => s.get(idx).map(|l| l.ts_ms()),
            ActiveSeries::Cvd(s) => s.get(idx).map(|c| c.ts_ms),
            ActiveSeries::Tpo(s) => s.get(idx).map(|t| t.ts_ms()),
        }
    }

    /// Binary-search the active series for the index whose timestamp is
    /// closest to (not after) `ts_ms`. Mirrors `find_bar_for_timestamp_ms`'s
    /// contract (extrapolates via `idx_for_ts`'s callers when out of range —
    /// callers needing extrapolation go through [`ActiveSeries::to_bar_shadow`]
    /// + the existing `mlc_core` timestamp helpers instead of duplicating
    /// extrapolation logic here).
    pub fn idx_for_ts(&self, ts_ms: i64) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        let n = self.len();
        // First index whose ts > ts_ms (binary search over the index range —
        // `Range<usize>` has no `partition_point`, unlike slices).
        let mut lo = 0usize;
        let mut hi = n;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.ts_ms_at(mid).unwrap_or(i64::MIN) <= ts_ms {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let idx = lo;
        if idx == 0 {
            Some(0)
        } else if idx >= n {
            Some(n - 1)
        } else {
            Some(idx - 1)
        }
    }

    /// Gap (ms) between the first two elements, or [`FALLBACK_GAP_MS`] when
    /// the series has fewer than two or they do not advance.
    fn first_gap_ms(&self) -> f64 {
        match (self.ts_ms_at(0), self.ts_ms_at(1)) {
            (Some(a), Some(b)) if b > a => (b - a) as f64,
            _ => FALLBACK_GAP_MS,
        }
    }

    /// Gap (ms) between the last two elements, same fallback.
    fn last_gap_ms(&self) -> f64 {
        let n = self.len();
        if n < 2 {
            return FALLBACK_GAP_MS;
        }
        match (self.ts_ms_at(n - 2), self.ts_ms_at(n - 1)) {
            (Some(a), Some(b)) if b > a => (b - a) as f64,
            _ => FALLBACK_GAP_MS,
        }
    }

    /// Timestamp (ms) at the fractional element position `pos`. Positions
    /// between elements interpolate; positions before the first or past the
    /// last extrapolate by the nearest gap. `None` for an empty series or a
    /// non-finite position.
    ///
    /// The live share stream sends viewport and cursor as timestamps, so two
    /// sides with different loaded history still point at the same moment.
    pub fn ts_at_position(&self, pos: f64) -> Option<i64> {
        let n = self.len();
        if n == 0 || !pos.is_finite() {
            return None;
        }
        let last = (n - 1) as f64;
        if pos <= 0.0 {
            return Some(self.ts_ms_at(0)? + (pos * self.first_gap_ms()).round() as i64);
        }
        if pos >= last {
            return Some(self.ts_ms_at(n - 1)? + ((pos - last) * self.last_gap_ms()).round() as i64);
        }
        let i = pos.floor() as usize;
        let frac = pos - i as f64;
        let a = self.ts_ms_at(i)?;
        let b = self.ts_ms_at(i + 1)?;
        Some(a + ((b - a) as f64 * frac).round() as i64)
    }

    /// Inverse of [`Self::ts_at_position`]: the fractional element position
    /// of `ts_ms` in THIS series. Before the first element the position is
    /// negative, past the last it exceeds `len() - 1`. `None` when empty.
    pub fn position_at_ts(&self, ts_ms: i64) -> Option<f64> {
        let n = self.len();
        if n == 0 {
            return None;
        }
        let first = self.ts_ms_at(0)?;
        if ts_ms <= first {
            return Some((ts_ms - first) as f64 / self.first_gap_ms());
        }
        let last = self.ts_ms_at(n - 1)?;
        if ts_ms >= last {
            return Some((n - 1) as f64 + (ts_ms - last) as f64 / self.last_gap_ms());
        }
        let i = self.idx_for_ts(ts_ms)?;
        let a = self.ts_ms_at(i)?;
        let b = self.ts_ms_at(i + 1)?;
        if b <= a {
            return Some(i as f64);
        }
        Some(i as f64 + (ts_ms - a) as f64 / (b - a) as f64)
    }

    /// (low, high) price range across the visible index window
    /// `[visible.start, visible.end)` — feeds Y autoscale. Returns `None`
    /// when the range is empty or every candidate min/max stays non-finite
    /// (mirrors `PriceScale::calc_auto_scale`'s empty-input no-op).
    pub fn y_range(&self, visible: std::ops::Range<usize>) -> Option<(f64, f64)> {
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        let end = visible.end.min(self.len());
        let start = visible.start.min(end);
        match self {
            ActiveSeries::Bars(s) => {
                for b in &s[start..end] {
                    min = min.min(b.low);
                    max = max.max(b.high);
                }
            }
            ActiveSeries::Renko(s) => {
                for b in &s[start..end] {
                    min = min.min(b.bottom);
                    max = max.max(b.top);
                }
            }
            ActiveSeries::Pnf(s) => {
                for c in &s[start..end] {
                    min = min.min(c.bottom);
                    max = max.max(c.top);
                }
            }
            ActiveSeries::Kagi(s) => {
                for k in &s[start..end] {
                    min = min.min(k.start_price.min(k.end_price));
                    max = max.max(k.start_price.max(k.end_price));
                }
            }
            ActiveSeries::Tlb(s) => {
                for l in &s[start..end] {
                    min = min.min(l.low());
                    max = max.max(l.high());
                }
            }
            ActiveSeries::Cvd(s) => {
                for c in &s[start..end] {
                    min = min.min(c.value);
                    max = max.max(c.value);
                }
            }
            ActiveSeries::Tpo(s) => {
                for t in &s[start..end] {
                    min = min.min(t.session_low);
                    max = max.max(t.session_high);
                }
            }
        }
        if min.is_finite() && max.is_finite() {
            Some((min, max))
        } else {
            None
        }
    }

    /// Build a `Vec<Bar>` shadow carrying ONLY the active series' timestamps
    /// (seconds), for legacy `&[Bar]`-shaped APIs that only read `.timestamp`
    /// for ts↔index conversion: `mlc_core::find_bar_for_timestamp`,
    /// `TimeScale::generate_ticks`, `RenderContext::set_bars`/`ts_to_x_ms`.
    ///
    /// OHLC fields are zeroed — every one of those call sites reads
    /// `.timestamp` exclusively for the derived-chart-type path (verified:
    /// `find_bar_for_timestamp` binary-searches `.timestamp`;
    /// `generate_ticks`'s calendar-boundary algorithm reads `.timestamp` for
    /// bucketing; `ts_to_x_ms` → `timestamp_ms_to_bar_f64` reads
    /// `.timestamp` only). For `Bars` this is a no-op passthrough (already a
    /// `&[Bar]`) — callers should prefer borrowing directly in that case;
    /// this method exists for the derived variants.
    pub fn to_bar_shadow(&self) -> Vec<Bar> {
        // Renko/Pnf/Kagi/Tlb `open_time`/`ts_ms()` are all millisecond
        // timestamps verbatim from the upstream event (see `RenkoBrick::
        // ts_ms`, `PnfColumn::ts_ms`, `KagiSegment::ts_ms`,
        // `ThreeLineBreakLine::ts_ms` — every one returns its ms field
        // unchanged). `Bar.timestamp` is a SECONDS contract (see doc on
        // `Bar::timestamp`), so each ms value must be divided down and the
        // verbatim ms preserved in `derived_ts_ms` — mirrors the
        // `timestamp = open_time/1000`, `derived_ts_ms = open_time`
        // convention used by `kline_to_bar`/`bar_point_to_bar`.
        match self {
            ActiveSeries::Bars(s) => s.to_vec(),
            ActiveSeries::Renko(s) => s.iter().map(|b| ms_to_bar_shadow(b.open_time)).collect(),
            ActiveSeries::Pnf(s) => s.iter().map(|c| ms_to_bar_shadow(c.open_time)).collect(),
            ActiveSeries::Kagi(s) => s.iter().map(|k| ms_to_bar_shadow(k.open_time)).collect(),
            ActiveSeries::Tlb(s) => s.iter().map(|l| ms_to_bar_shadow(l.ts_ms())).collect(),
            ActiveSeries::Cvd(s) => s.iter().map(|c| ms_to_bar_shadow(c.ts_ms)).collect(),
            ActiveSeries::Tpo(s) => s.iter().map(|t| ms_to_bar_shadow(t.open_time)).collect(),
        }
    }
}

/// Build an OHLC-zeroed shadow `Bar` from a millisecond timestamp, matching
/// the `timestamp = ms/1000` / `derived_ts_ms = ms` convention used
/// everywhere else a ms-native event is lowered into a `Bar` (see
/// `kline_to_bar` in `mlc-backend-data-bridge/src/convert.rs` and
/// `bar_point_to_bar` in `mlc-backend/src/bubble/view_fold.rs`).
#[inline]
fn ms_to_bar_shadow(ts_ms: i64) -> Bar {
    Bar {
        timestamp: ts_ms / 1000,
        derived_ts_ms: ts_ms,
        ..Bar::new(0, 0.0, 0.0, 0.0, 0.0)
    }
}

/// Build the `Vec<Bar>` timestamp shadow that `ChartRenderState.bars` should
/// carry for time-scale labels (`TimeScale::generate_ticks`) and drawings
/// ts→x (`RenderContext::set_bars`/`ts_to_x_ms`) — audit D8 items (c)/(e)
/// from `chart-types-xaxis-audit-2026-07-03.md` §3.
///
/// Narrower than [`resolve_active_series`]: renko/pnf/kagi/
/// three_line_break/tpo get a shadow — their emission-order timeline IS what a
/// drawing/label positioned on them should follow. CVD is deliberately
/// EXCLUDED even though it has its own bookkeeping slot: `cvd.rs`'s
/// `draw_cvd_line_into_rect` reads `state.bars` on purpose to establish its
/// ts-domain as the visible KLINE range (own documented X strategy, not a
/// bug) — overriding it here would break that by design. TPO's
/// emission-order timeline (session `open_time`) IS the shadow, mirroring
/// renko/pnf/kagi/tlb — one session per index slot, axis labels read the
/// session date. Every other chart_type (candle-family,
/// range/volume/tick/dollar bars, footprint, dom/liquidation heatmap)
/// already lives in or intentionally reads `bubble.bars` — returns `None`
/// (caller keeps the real kline bars unchanged).
/// Counted-bar chart types: aggregation by trade count/volume/dollar/etc.,
/// so successive bars close at wildly non-uniform wall-clock intervals —
/// a regular vertical time grid over them is meaningless. Per-bar time
/// LABELS stay (they carry real bar-close timestamps, handled by the
/// per-bar tick path). Kept out of the `non_temporal` flag because
/// their axis IS temporal — just non-uniform.
pub fn chart_type_hides_time_grid(chart_type: &str) -> bool {
    mlc_core::chart_type::by_id(chart_type).map(|d| d.hides_time_grid).unwrap_or(false)
}

/// Effective wick flag for `mlc_backend::bubble::ChartWindowRenderInputs::wicks_effective`
/// — `draw_candles`' `state.wick_enabled` gate, precomputed here (once, at
/// the render-state build boundary) so `chart_type` is never threaded into
/// the renderer itself.
///
/// Registry-driven (K0 contract, B2 — `ChartTypeDef::render_policy`; first
/// live consumer of the field, proving the contract ahead of B3's full
/// consumer compression): `WickPolicy::Always` keeps the plain
/// `instrument_wick_enabled` global setting unchanged (candle family, and —
/// today's reality, unchanged by this rewrite — tick_bar/tick_imbalance_bar/
/// volume_imbalance_bar/run_bar, whose draw fns either ignore
/// `wick_enabled` entirely or share `draw_candles` without a bar-family
/// override); `WickPolicy::Off` forces wicks off regardless of the global
/// toggle (non-candle primitives whose draw fn never reads
/// `state.wick_enabled` at all); `WickPolicy::Toggle { .. }` is
/// range_bar/volume_bar/dollar_bar (their open/high/low pins are
/// chained-continuity artifacts — canonical range-bar/visual-continuity open
/// = prev bar's close, not genuine intrabar excursions the way candle
/// wicks are) plus renko (canon-fix wave 2, item 3 — `RenkoBrick::wick` IS a
/// genuine intra-brick adverse excursion, but still gated behind the
/// per-panel toggle so it defaults off like its bar-family siblings), so
/// `bar_family_wicks` (per-panel settings toggle) is required on top of the
/// global toggle to show them. Unknown/unregistered
/// chart_type ids (defensive fallback, mirrors `chart_type_is_derived_seed`)
/// keep the plain global toggle.
pub fn effective_wick_flag(chart_type: &str, instrument_wick_enabled: bool, bar_family_wicks: bool) -> bool {
    let Some(def) = mlc_core::chart_type::by_id(chart_type) else {
        return instrument_wick_enabled;
    };
    match def.render_policy.wicks {
        mlc_core::chart_type::WickPolicy::Always => instrument_wick_enabled,
        mlc_core::chart_type::WickPolicy::Off => false,
        mlc_core::chart_type::WickPolicy::Toggle { .. } => instrument_wick_enabled && bar_family_wicks,
    }
}

/// `chart_type`s whose scroll-left backfill must go through
/// `DataBridge::request_deeper_seed_for_active` (full teardown + doubled-
/// depth reseed of a stateful derived aggregator) instead of
/// `request_scroll_bars` (kline REST-page prepend).
///
/// Kline family (candles/line/area/hollow_candles/heikin_ashi/bars/
/// step_line/line_markers/hlc_area/baseline/histogram) and
/// `"footprint"` are NOT here — footprint's viewport bookkeeping still
/// piggybacks `bubble.bars` (see `resolve_active_series`'s doc comment),
/// and every kline-family type extends via the existing
/// `request_scroll_bars` machinery, which stays untouched.
///
/// See `nemo/docs/mlc/plans/dig3-station-0325-derived-seeding-2026-07-04.md`
/// for why derived state machines cannot use a "prepend older bars" fetch —
/// the only way to deepen their history is a full station-side
/// unsubscribe + respawn with a larger warm depth.
///
/// Registry-driven: "derived" = any `chart_type` whose `ChartTypeKind` is not
/// `RenderOnly` (trade-agg bars, price-action transforms, orderflow-slotted
/// types all reseed via `request_deeper_seed_for_active`).
///
/// Defensive fallback: `"cvd"` is no longer reachable via `ChangeChartType`
/// (the 2026-07-04 taxonomy fix demoted CVD from a full-pane chart_type to a
/// subpane indicator instance, so it has no registry row), but historical
/// presets / agent-API callers may still pass the string. Kept `true` here to
/// preserve today's behavior — drop this arm only once no external caller can
/// still send `"cvd"` through this path.
pub fn chart_type_is_derived_seed(chart_type: &str) -> bool {
    if let Some(def) = mlc_core::chart_type::by_id(chart_type) {
        return def.kind != mlc_core::chart_type::ChartTypeKind::RenderOnly;
    }
    chart_type == "cvd"
}

/// Whether `chart_type` is one of the 7 trade-aggregation bar types
/// (range_bar/volume_bar/tick_bar/dollar_bar/tick_imbalance_bar/
/// volume_imbalance_bar/run_bar) — the family that writes its derived
/// series directly into `bubble.bars` while keeping `bubble.timeframe`
/// unstamped (unlike footprint, whose `UpdateChartTypeInterval` re-stamps
/// it; see `chart_out_events.rs`).
///
/// Single source of truth for the "is this window's active chart_type a
/// trade-agg-bar" test — both `BarUpdatePassthrough`'s bars-ownership gate
/// and `TradeUpdatePassthrough`'s kline-builder exclusion (react.rs) must
/// agree on this set, or a trade-agg-bar window gets a second writer
/// (raw-trade kline candles built on top of the station-derived series
/// into the same `bubble.bars`, root cause fixed 2026-07-08). Registry-driven
/// (`ChartTypeDef::kind == ChartTypeKind::TradeAggBar`) instead of a
/// hand-copied string list.
pub fn chart_type_is_trade_agg_bar(chart_type: &str) -> bool {
    mlc_core::chart_type::by_id(chart_type)
        .map(|def| def.kind == mlc_core::chart_type::ChartTypeKind::TradeAggBar)
        .unwrap_or(false)
}

pub fn ts_shadow_for_chart_type(chart_type: &str, bubble: &DataBubble) -> Option<Vec<Bar>> {
    let needs_shadow = mlc_core::chart_type::by_id(chart_type).map(|d| d.needs_ts_shadow).unwrap_or(false);
    if !needs_shadow {
        return None;
    }
    let series = resolve_active_series(chart_type, bubble);
    Some(series.to_bar_shadow())
}

/// Resolve the [`ActiveSeries`] a `chart_type` renders, from the bubble that
/// backs it. Mirrors the `draw_series` string-dispatch table
/// (`render_chart.rs:205-246`).
///
/// Falls back to `Bars` for any chart_type not explicitly matched — same
/// fallback discipline as `draw_series`' `_ => draw_candles(ctx, state)`
/// arm, so an unknown/new chart_type never panics on an absent slot.
pub fn resolve_active_series<'a>(chart_type: &str, bubble: &'a DataBubble) -> ActiveSeries<'a> {
    use mlc_core::chart_type::SeriesVariant;
    let variant =
        mlc_core::chart_type::by_id(chart_type).map(|d| d.series_variant).unwrap_or(SeriesVariant::Bars);
    match variant {
        SeriesVariant::Renko => bubble.renko_bars.as_deref().map(ActiveSeries::Renko),
        SeriesVariant::Pnf => bubble.pnf_columns.as_deref().map(ActiveSeries::Pnf),
        SeriesVariant::Kagi => bubble.kagi_segments.as_deref().map(ActiveSeries::Kagi),
        SeriesVariant::Tlb => bubble.three_line_break.as_deref().map(ActiveSeries::Tlb),
        SeriesVariant::Tpo => bubble.tpo_sessions.as_deref().map(ActiveSeries::Tpo),
        // candles/bars/hollow_candles/heikin_ashi/line/step_line/line_markers/
        // area/hlc_area/baseline/histogram: bubble.bars directly.
        //
        // range_bar/volume_bar/tick_bar/dollar_bar/tick_imbalance_bar/
        // volume_imbalance_bar/run_bar: station writes these series directly
        // INTO bubble.bars (see `BarUpdatePassthrough`, react.rs) — they ARE
        // the kline array for bookkeeping purposes, no separate slot exists.
        //
        // footprint: bubble.footprint_bars is its own slot, but footprint's
        // viewport bookkeeping (bar_count / time labels) stays keyed to
        // bubble.bars by design — footprint needs the kline backdrop
        // (candle wicks) rendered underneath the cluster grid, and the
        // renderer clamps footprint_bars against its own length per draw
        // call independent of viewport.bar_count.
        //
        // dom_heatmap/liquidation_heatmap: own ring buffers, own virtual-index
        // (`bucket_idx = (ts-oldest)/bucket_ms`) computed straight from each
        // cell's ts_ms, never from viewport.bar_count — genuinely bypass this
        // seam; resolving them to Bars is inert (nothing reads it for X).
        SeriesVariant::Bars => None,
    }
    .unwrap_or(ActiveSeries::Bars(&bubble.bars))
}

/// Owned snapshot mirroring [`ActiveSeries`]'s variants, for call sites that
/// must resolve the active series once and then release the bubble lock
/// before mutating windows across a loop (the pattern `active_chart_bars()`
/// already uses for the plain-`Bars` case — this extends it to the derived
/// variants so those call sites don't silently stay kline-only).
#[derive(Clone, Default)]
pub enum OwnedActiveSeries {
    #[default]
    Empty,
    Bars(Vec<Bar>),
    Renko(Vec<RenkoBrick>),
    Pnf(Vec<PnfColumn>),
    Kagi(Vec<KagiSegment>),
    Tlb(Vec<ThreeLineBreakLine>),
    Cvd(Vec<CvdSample>),
    Tpo(Vec<TpoSession>),
}

impl OwnedActiveSeries {
    /// Borrow this owned snapshot as an [`ActiveSeries`].
    pub fn as_active_series(&self) -> ActiveSeries<'_> {
        match self {
            OwnedActiveSeries::Empty => ActiveSeries::Bars(&[]),
            OwnedActiveSeries::Bars(v) => ActiveSeries::Bars(v),
            OwnedActiveSeries::Renko(v) => ActiveSeries::Renko(v),
            OwnedActiveSeries::Pnf(v) => ActiveSeries::Pnf(v),
            OwnedActiveSeries::Kagi(v) => ActiveSeries::Kagi(v),
            OwnedActiveSeries::Tlb(v) => ActiveSeries::Tlb(v),
            OwnedActiveSeries::Cvd(v) => ActiveSeries::Cvd(v),
            OwnedActiveSeries::Tpo(v) => ActiveSeries::Tpo(v),
        }
    }
}

/// Same resolution table as [`resolve_active_series`], cloning the matched
/// slot into an owned snapshot so the caller can release the bubble borrow.
pub fn resolve_active_series_owned(chart_type: &str, bubble: &DataBubble) -> OwnedActiveSeries {
    match resolve_active_series(chart_type, bubble) {
        ActiveSeries::Bars(s) => OwnedActiveSeries::Bars(s.to_vec()),
        ActiveSeries::Renko(s) => OwnedActiveSeries::Renko(s.to_vec()),
        ActiveSeries::Pnf(s) => OwnedActiveSeries::Pnf(s.to_vec()),
        ActiveSeries::Kagi(s) => OwnedActiveSeries::Kagi(s.to_vec()),
        ActiveSeries::Tlb(s) => OwnedActiveSeries::Tlb(s.to_vec()),
        ActiveSeries::Cvd(s) => OwnedActiveSeries::Cvd(s.to_vec()),
        ActiveSeries::Tpo(s) => OwnedActiveSeries::Tpo(s.to_vec()),
    }
}

/// Same resolution table, sourced from a [`BubbleStore`] (parked-preset
/// path — `live_preset_cache` entries own a `BubbleStore` directly rather
/// than going through the `BubblesMut`/`Bubbles` facade, and `BubbleStore`'s
/// own `&DataBubble` accessor is crate-private to `mlc-backend`; its
/// per-slot public accessors give the same data without needing it).
pub fn resolve_active_series_owned_from_store(
    chart_type: &str,
    store: &BubbleStore,
    bid: BubbleId,
) -> OwnedActiveSeries {
    use mlc_core::chart_type::SeriesVariant;
    let variant =
        mlc_core::chart_type::by_id(chart_type).map(|d| d.series_variant).unwrap_or(SeriesVariant::Bars);
    match variant {
        SeriesVariant::Renko => store.renko_bars(bid).map(|s| OwnedActiveSeries::Renko(s.to_vec())),
        SeriesVariant::Pnf => store.pnf_columns(bid).map(|s| OwnedActiveSeries::Pnf(s.to_vec())),
        SeriesVariant::Kagi => store.kagi_segments(bid).map(|s| OwnedActiveSeries::Kagi(s.to_vec())),
        SeriesVariant::Tlb => store.three_line_break(bid).map(|s| OwnedActiveSeries::Tlb(s.to_vec())),
        SeriesVariant::Tpo => store.tpo_sessions(bid).map(|s| OwnedActiveSeries::Tpo(s.to_vec())),
        SeriesVariant::Bars => None,
    }
    .unwrap_or_else(|| OwnedActiveSeries::Bars(store.bars(bid).to_vec()))
}

/// `chart_type`s where the timeframe concept is inert — the rendered series
/// is not keyed by wall-clock/tick timeframe at all (it's a price/volume/
/// count-driven transform), so a TF picker has no effect on what's drawn.
/// Mirrors [`chart_type_hides_time_grid`]'s counted-bar family plus the
/// non-temporal price-action transforms (renko/pnf/kagi/three_line_break).
pub fn chart_type_hides_timeframe_picker(chart_type: &str) -> bool {
    mlc_core::chart_type::by_id(chart_type).map(|d| d.hides_tf_picker).unwrap_or(false)
}

/// The full set of `tf_*` dropdown item ids from the timeframe_selector
/// toolbar definitions (`mlc-core/src/toolbar.rs`'s `standalone_top_toolbar`
/// / `top_toolbar`). Kept as an independent literal here (not read back from
/// the toolbar defs) so this writer is self-contained — see
/// [`allowed_timeframes`].
pub(crate) const ALL_TF_IDS: &[&str] = &[
    "tf_1s", "tf_5s", "tf_15s", "tf_30s",
    "tf_1m", "tf_3m", "tf_5m", "tf_15m", "tf_30m",
    "tf_1h", "tf_2h", "tf_4h", "tf_6h", "tf_12h",
    "tf_1d", "tf_1w", "tf_1M",
];

/// Set of `tf_*` ids the timeframe picker should offer as clickable, or
/// `None` when the picker is not applicable at all for this `chart_type`
/// (callers hide the whole control rather than render a fully-disabled one).
///
/// NO depth-gating (owner rule 2026-07-05, replaces the 2026-07-04 W3
/// depth-gate): kline TFs aggregate server-side — switching to ANY of them
/// fires a fresh REST backfill at that resolution, so the locally loaded
/// series' depth says nothing about whether the TF is usable. The old gate
/// (`depth_ms >= interval * 10`) blocked 1M on an 83-day 1H chart and
/// blocked everything above 2H after a footprint→klines round-trip.
///
/// Chart-type COMPATIBILITY gating (owner report 2026-07-06 regression): a
/// separate concern from depth. Some chart types physically cannot use tick
/// TFs (footprint's bucket_interval, TPO's letter-frequency) — the bucket
/// pipeline expects a millisecond duration, tick TFs express a trade-count
/// window. Similarly, footprint's server-side bucket only supports 1m..1h.
/// This whitelist survived a prior implementation and got accidentally
/// dropped when the depth-gate rewrite collapsed the function body — it is
/// restored here as a distinct, chart_type-keyed filter that runs
/// independently of any depth check.
pub fn allowed_timeframes(
    _active_series: ActiveSeries<'_>,
    chart_type: &str,
    _now_ms: i64,
) -> Option<std::collections::HashSet<&'static str>> {
    let def = mlc_core::chart_type::by_id(chart_type)?;
    if def.hides_tf_picker {
        return None;
    }
    match def.tf_whitelist {
        Some(slice) => Some(slice.iter().copied().collect()),
        None => Some(ALL_TF_IDS.iter().copied().collect()),
    }
}

/// Convert a `timeframe_selector` dropdown item id (`"tf_1h"`, `"tf_1M"`,
/// `"tf_5s"`, ...) to the canonical dig3 kline-interval string
/// (`"1h"`, `"1M"`, `"5s"`, ...) consumed by
/// `KlineIntervalCapabilities::supports` (resolution-availability-2026-07-08
/// Phase 3).
///
/// Mirrors `mlc_backend_data_bridge::convert::interval_label`'s case rule
/// (chart uses uppercase for hours+, exchanges/dig3 use lowercase — EXCEPT
/// month, which stays `"1M"` on both sides; that is the one case-sensitive
/// pair, `"1M"` month vs `"1m"` minute) but starts from the dropdown item id
/// (`tf_` prefix, already-lowercase minutes/seconds, already-uppercase-only-
/// for-month) rather than a `Timeframe.name`. Returns `None` for an id that
/// doesn't match the `tf_` prefix convention (defensive — every real
/// `timeframe_selector` item id is `tf_*`).
pub fn tf_id_to_dig3_interval(tf_id: &str) -> Option<String> {
    let suffix = tf_id.strip_prefix("tf_")?;
    if suffix.ends_with('M') {
        // Month — canonical form is already uppercase-M on both sides.
        Some(suffix.to_string())
    } else {
        // Everything else (seconds/minutes/hours/days/weeks) is lowercase
        // on the wire; dropdown ids already carry lowercase suffixes for
        // every one of these (`tf_1h`, `tf_1d`, `tf_1w`, ...) so this is a
        // no-op today, but normalizing defensively keeps the mapping
        // correct if an uppercase id variant is ever added.
        Some(suffix.to_lowercase())
    }
}

/// Duration in milliseconds for a canonical dig3 kline-interval string
/// (`"1s"`, `"5m"`, `"2h"`, `"1d"`, `"1w"`, `"1M"`, ...) — the numeric-suffix
/// convention [`tf_id_to_dig3_interval`] produces. Used by the aggregatability
/// predicate ([`timeframe_aggregates_from`] in `mlc-app/src/lib.rs`,
/// granularity arc defect fix 2026-07-16) to test whether one interval is an
/// exact integer multiple of another.
///
/// Month (`M`) uses a fixed 30-day approximation for divisibility purposes
/// only (calendar months vary 28-31 days, so no exact millisecond count
/// exists) — this only affects whether some OTHER interval divides evenly
/// into "1 month", a comparison that in practice never fires for the
/// second/minute/hour/day intervals this predicate actually gates.
///
/// Returns `None` for a suffix outside the `s/m/h/d/w/M` set (defensive —
/// mirrors [`tf_id_to_dig3_interval`]'s own closed suffix range).
pub fn dig3_interval_to_ms(interval: &str) -> Option<i64> {
    let bytes = interval.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let unit = *bytes.last()?;
    let n_str = std::str::from_utf8(&bytes[..bytes.len().saturating_sub(1)]).ok()?;
    let n: i64 = n_str.parse().ok()?;
    if n <= 0 {
        return None;
    }
    const SEC: i64 = 1_000;
    const MIN: i64 = 60 * SEC;
    const HOUR: i64 = 60 * MIN;
    const DAY: i64 = 24 * HOUR;
    match unit {
        b's' => Some(n * SEC),
        b'm' => Some(n * MIN),
        b'h' => Some(n * HOUR),
        b'd' => Some(n * DAY),
        b'w' => Some(n * 7 * DAY),
        b'M' => Some(n * 30 * DAY),
        _ => None,
    }
}
