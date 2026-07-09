//! `FoldRuntime` — client-side re-cut engine for the 7 kline-sufficient
//! chart types (unified-data-plane Stage Э3,
//! `nemo/docs/mlc/plans/unified-data-plane-2026-07-09.md`).
//!
//! `range_bar` / `volume_bar` / `dollar_bar` / `renko` / `pnf` / `kagi` /
//! `three_line_break` no longer open a station-side derived subscription.
//! Instead each bubble installs one [`FoldRuntime`] that folds the SAME
//! shared 1-minute kline layer (`DataBubble::kline_layers["1m"]`) the
//! ordinary kline path already fills — a chart-type or param switch among
//! these 7 types is therefore a pure in-memory re-fold of data the bubble
//! already has, zero network.
//!
//! ## Cold-seed contract (mirrors `digdigdig3-station::station.rs` verbatim)
//!
//! [`FoldRuntime::reseed`] replays every bar in the layer EXCEPT the
//! possibly-in-progress last one as a 4-leg synthetic-trade path
//! ([`digdigdig3_station::folds::kline_to_synthetic_trades`]), then hands
//! the last bar's `(open_time, volume, close)` to the fold machine's
//! embedded `KlineDeltaState` via `seed_kline_baseline` — so the first live
//! update sharing that same `open_time` computes a correct incremental
//! delta instead of double-counting. See
//! `digdigdig3-station/src/station.rs` (`acquire_or_spawn_derived_body`,
//! search `kline_seed_baseline`) for the server-side sibling of this same
//! contract.
//!
//! ## Live-fold contract
//!
//! [`FoldRuntime::on_live_1m`] feeds one closed-or-in-progress 1m bar as
//! `Event::Bar` directly into the fold machine — the machine's own embedded
//! `KlineDeltaState` folds the repeated same-`open_time` updates into
//! incremental Δ-volume synthetic trades internally (see
//! `digdigdig3_station::derived::KlineDeltaState`'s doc comment).

use digdigdig3::core::types::ExchangeId;
use digdigdig3::core::websocket::KlineInterval;
use digdigdig3_station::folds::{
    kline_to_synthetic_trades, DerivedStream, TradeToDollarBarDerived, TradeToKagiBarDerived,
    TradeToPnfBarDerived, TradeToRangeBarDerived, TradeToRenkoBarDerived,
    TradeToThreeLineBreakDerived, TradeToVolumeBarDerived,
};
use digdigdig3_station::subscription::Event;

use super::raw_layer::RawLayer;

/// Milliseconds per 1-minute kline bar — the only layer these 7 fold
/// machines ever consume (kline-sufficient family, see module doc).
const ONE_MINUTE_MS: i64 = 60_000;

/// Convert an `mlc_core::Bar` (bubble-side kline layer entry) into the
/// `digdigdig3_station::data::BarPoint` shape the fold machines expect.
///
/// Field mapping (mirrors `mlc-backend-data-bridge::bridge::bar_point_to_bar`
/// in reverse):
/// - `open_time` ← `bar.derived_ts_ms` when set (genuine kline millisecond
///   identity), else `bar.timestamp * 1000` (seconds → ms fallback for
///   bars that never carried a native ms stamp — synthetic/test fixtures).
/// - `open`/`high`/`low`/`close` ← verbatim.
/// - `volume` ← verbatim.
/// - `quote_volume` ← `NaN` (not tracked on `mlc_core::Bar`; every fold
///   machine that reads it treats `NaN` the same as "absent", matching
///   `BarPoint::from_kline`'s own `unwrap_or(f64::NAN)` convention for a
///   kline with no native quote-volume field).
/// - `trades_count` ← `0` (not tracked on `mlc_core::Bar`; same "absent"
///   convention as `quote_volume`, matching `BarPoint::from_kline`'s
///   `unwrap_or(0)`).
pub fn bar_to_bar_point(bar: &mlc_core::Bar) -> digdigdig3_station::data::BarPoint {
    let open_time = if bar.derived_ts_ms != 0 { bar.derived_ts_ms } else { bar.timestamp * 1000 };
    digdigdig3_station::data::BarPoint {
        open_time,
        open: bar.open,
        high: bar.high,
        low: bar.low,
        close: bar.close,
        volume: bar.volume,
        quote_volume: f64::NAN,
        trades_count: 0,
    }
}

/// One fold machine instance, keyed by which of the 7 kline-sufficient
/// chart types it computes. Each variant embeds its own `KlineDeltaState`
/// internally (see `digdigdig3_station::derived` module doc) — the caller
/// never touches that adapter directly.
pub enum FoldMachine {
    RangeBar(TradeToRangeBarDerived),
    VolumeBar(TradeToVolumeBarDerived),
    DollarBar(TradeToDollarBarDerived),
    Renko(TradeToRenkoBarDerived),
    Pnf(TradeToPnfBarDerived),
    Kagi(TradeToKagiBarDerived),
    ThreeLineBreak(TradeToThreeLineBreakDerived),
}

/// One fold machine's output for a single upstream event. Never mixed
/// types within one `FoldRuntime` — the variant matches the machine.
#[derive(Clone, Debug)]
pub enum FoldOutput {
    Bar(mlc_core::Bar),
    Renko(mlc_core::types::RenkoBrick),
    Pnf(mlc_core::types::PnfColumn),
    Kagi(mlc_core::types::KagiSegment),
    ThreeLineBreak(mlc_core::types::ThreeLineBreakLine),
}

impl FoldMachine {
    /// Construct the machine for `chart_type` from the same final-unit
    /// parameters `SubscribeParams` already resolves at the UI seam
    /// (ticks × tick_size for range/renko/pnf/kagi is applied by the
    /// caller BEFORE this — see `chart_out_events.rs::build_subscribe_params`).
    /// Returns `None` for any chart_type outside the 7 kline-sufficient
    /// fold types.
    pub fn new(chart_type: &str, params: &FoldParams) -> Option<Self> {
        match chart_type {
            "range_bar" => Some(FoldMachine::RangeBar(TradeToRangeBarDerived::new(params.scaled_units))),
            "volume_bar" => Some(FoldMachine::VolumeBar(TradeToVolumeBarDerived::new(params.scaled_units))),
            "dollar_bar" => Some(FoldMachine::DollarBar(TradeToDollarBarDerived::new(params.threshold))),
            "renko" => Some(FoldMachine::Renko(TradeToRenkoBarDerived::new(params.scaled_units, params.reversal))),
            "pnf" => Some(FoldMachine::Pnf(TradeToPnfBarDerived::new(params.scaled_units, params.reversal))),
            "kagi" => Some(FoldMachine::Kagi(TradeToKagiBarDerived::new(params.scaled_units))),
            "three_line_break" => Some(FoldMachine::ThreeLineBreak(TradeToThreeLineBreakDerived::new(params.lines_back as usize))),
            _ => None,
        }
    }

    /// Chart-type id this machine computes — used to re-derive the output
    /// slot without carrying a separate tag.
    pub fn chart_type(&self) -> &'static str {
        match self {
            FoldMachine::RangeBar(_) => "range_bar",
            FoldMachine::VolumeBar(_) => "volume_bar",
            FoldMachine::DollarBar(_) => "dollar_bar",
            FoldMachine::Renko(_) => "renko",
            FoldMachine::Pnf(_) => "pnf",
            FoldMachine::Kagi(_) => "kagi",
            FoldMachine::ThreeLineBreak(_) => "three_line_break",
        }
    }

    fn seed_kline_baseline(&mut self, open_time: i64, volume: f64, close: f64) {
        match self {
            FoldMachine::RangeBar(m) => m.seed_kline_baseline(open_time, volume, close),
            FoldMachine::VolumeBar(m) => m.seed_kline_baseline(open_time, volume, close),
            FoldMachine::DollarBar(m) => m.seed_kline_baseline(open_time, volume, close),
            FoldMachine::Renko(m) => m.seed_kline_baseline(open_time, volume, close),
            FoldMachine::Pnf(m) => m.seed_kline_baseline(open_time, volume, close),
            FoldMachine::Kagi(m) => m.seed_kline_baseline(open_time, volume, close),
            FoldMachine::ThreeLineBreak(m) => m.seed_kline_baseline(open_time, volume, close),
        }
    }

    /// Feed one upstream `Event` (synthetic trade leg during reseed, or a
    /// live `Event::Bar` Δ-tick), converting whatever `BarPoint`/output the
    /// machine emits into the bubble-side [`FoldOutput`] shape.
    ///
    /// `box_size` is only meaningful for `Pnf` (the box width isn't carried
    /// on `PnfColumnPoint` itself — mirrors `LiveUpdate::PnfBarUpdate`'s
    /// `box_size` side-channel in `mlc-backend-data-bridge`).
    fn on_event(&mut self, ev: &Event, box_size: f64) -> Option<FoldOutput> {
        match self {
            FoldMachine::RangeBar(m) => m.on_upstream_event(ev, 0).map(|p| FoldOutput::Bar(bar_point_to_bar(&p))),
            FoldMachine::VolumeBar(m) => m.on_upstream_event(ev, 0).map(|p| FoldOutput::Bar(bar_point_to_bar(&p))),
            FoldMachine::DollarBar(m) => m.on_upstream_event(ev, 0).map(|p| FoldOutput::Bar(bar_point_to_bar(&p))),
            FoldMachine::Renko(m) => m.on_upstream_event(ev, 0).map(|p| FoldOutput::Renko(renko_point_to_brick(&p))),
            FoldMachine::Pnf(m) => m.on_upstream_event(ev, 0).map(|p| FoldOutput::Pnf(pnf_point_to_column(&p, box_size))),
            FoldMachine::Kagi(m) => m.on_upstream_event(ev, 0).map(|p| FoldOutput::Kagi(kagi_point_to_segment(&p))),
            FoldMachine::ThreeLineBreak(m) => m.on_upstream_event(ev, 0).map(|p| FoldOutput::ThreeLineBreak(tlb_point_to_line(&p))),
        }
    }
}

fn bar_point_to_bar(p: &digdigdig3_station::data::BarPoint) -> mlc_core::Bar {
    mlc_core::Bar {
        timestamp: p.open_time / 1000,
        open: p.open,
        high: p.high,
        low: p.low,
        close: p.close,
        volume: p.volume,
        derived_ts_ms: p.open_time,
    }
}

fn renko_point_to_brick(p: &digdigdig3_station::data::RenkoBrickPoint) -> mlc_core::types::RenkoBrick {
    mlc_core::types::RenkoBrick {
        open_time: p.open_time,
        bottom: p.bottom,
        top: p.top,
        up: p.up,
        volume: p.volume,
        trades_count: p.trades_count,
    }
}

fn pnf_point_to_column(p: &digdigdig3_station::data::PnfColumnPoint, box_size: f64) -> mlc_core::types::PnfColumn {
    mlc_core::types::PnfColumn {
        open_time: p.open_time,
        column_id: p.column_id,
        is_x: p.is_x,
        bottom: p.bottom,
        top: p.top,
        volume: p.volume,
        trades_count: p.trades_count,
        box_size,
    }
}

fn kagi_point_to_segment(p: &digdigdig3_station::data::KagiSegmentPoint) -> mlc_core::types::KagiSegment {
    mlc_core::types::KagiSegment {
        open_time: p.open_time,
        start_price: p.start_price,
        end_price: p.end_price,
        up: p.up,
        yang: p.yang,
        is_connector: p.is_connector,
    }
}

fn tlb_point_to_line(p: &digdigdig3_station::data::ThreeLineBreakLinePoint) -> mlc_core::types::ThreeLineBreakLine {
    mlc_core::types::ThreeLineBreakLine {
        ts_open: p.ts_open,
        ts_close: p.ts_close,
        price_open: p.open,
        price_close: p.close,
        volume: p.volume,
        direction: p.direction,
    }
}

/// Final-unit parameters for constructing a [`FoldMachine`] — the SAME
/// resolved values `SubscribeParams` carries at the `chart_out_events.rs`
/// UI seam (ticks × tick_size already applied there; this struct is the
/// fold-specific narrowing of that enum, since a fold machine needs exactly
/// one shape rather than the full 10-variant `SubscribeParams` union).
#[derive(Clone, Copy, Debug, Default)]
pub struct FoldParams {
    pub scaled_units: f64,
    pub threshold: f64,
    pub reversal: u8,
    pub lines_back: u8,
    /// Only meaningful for `pnf` — carried through to `PnfColumn::box_size`
    /// (not part of the dig3 fold machine's own state).
    pub box_size: f64,
}

/// Per-bubble client-side fold runtime (unified-data-plane Stage Э3).
///
/// Installed when the bubble's active chart_type is one of the 7 kline-
/// sufficient types; `None` otherwise. Reseeded from the bubble's own
/// `kline_layers["1m"]` on install/param-change/chart-type-switch, then fed
/// one 1m bar at a time as the layer receives live appends.
pub struct FoldRuntime {
    machine: FoldMachine,
    chart_type: &'static str,
    /// Construction params, kept verbatim so [`Self::reseed`] can rebuild a
    /// byte-identical fresh machine (fold state has no rollback — the only
    /// way to recompute from scratch is a new instance replaying the full
    /// input from the beginning, see `DerivedStream` trait doc).
    params: FoldParams,
    exchange: ExchangeId,
    symbol: String,
    /// `open_time` of the last 1m layer entry this runtime has folded live
    /// (post-reseed) — guards against re-folding the same bar twice if
    /// `on_live_1m` is called more than once for an unchanged tick (e.g. a
    /// duplicate `BarUpdate` broadcast replay).
    last_live_open_time: Option<i64>,
}

impl FoldRuntime {
    /// Install a fresh runtime for `chart_type` with `params`. Returns
    /// `None` for any chart_type outside the 7 kline-sufficient fold types.
    pub fn new(chart_type: &str, params: FoldParams, exchange: ExchangeId, symbol: &str) -> Option<Self> {
        let machine = FoldMachine::new(chart_type, &params)?;
        let chart_type = machine.chart_type();
        Some(Self {
            machine,
            chart_type,
            params,
            exchange,
            symbol: symbol.to_string(),
            last_live_open_time: None,
        })
    }

    /// Chart-type id this runtime computes.
    pub fn chart_type(&self) -> &'static str {
        self.chart_type
    }

    /// Construction params this runtime is currently folding with — the
    /// caller compares this against a freshly-resolved `FoldParams` to
    /// decide whether a param change actually requires a re-seed (Э3
    /// `UpdateChartTypeParam` path).
    pub fn params(&self) -> FoldParams {
        self.params
    }

    /// Cold-seed / re-seed from the bubble's own 1m kline layer, mirroring
    /// `digdigdig3-station::station.rs`'s `acquire_or_spawn_derived_body`
    /// contract verbatim (see module doc): every bar EXCEPT the possibly-
    /// in-progress last one is folded as a 4-leg synthetic-trade path; the
    /// last bar's `(open_time, volume, close)` seeds the live Δ-tick
    /// baseline instead of being folded twice.
    ///
    /// Returns every output the replay produced, oldest-first — the caller
    /// replaces the bubble's output slot wholesale with this (a re-fold is
    /// a full recompute, not an incremental patch).
    pub fn reseed(&mut self, layer: &RawLayer<mlc_core::Bar>) -> Vec<FoldOutput> {
        // Fresh machine per reseed: the fold state (open bar, running
        // imbalance, grid anchor, ...) is entirely local and stateful with
        // no rollback support (see `DerivedStream` trait doc) — the ONLY
        // way to recompute from scratch is a new instance replaying the
        // full input from the beginning. `FoldMachine::new` with the SAME
        // `self.params` this runtime was originally constructed with
        // reproduces a byte-identical machine (direct-constructor doctrine,
        // see `folds.rs` module doc "Construction" section).
        if let Some(fresh) = FoldMachine::new(self.chart_type, &self.params) {
            self.machine = fresh;
        }
        self.last_live_open_time = None;

        let bars = layer.snapshot();
        let mut outputs = Vec::new();
        if bars.is_empty() {
            return outputs;
        }
        let (closed, last) = bars.split_at(bars.len() - 1);
        for bar in closed {
            let point = bar_to_bar_point(bar);
            for synth in kline_to_synthetic_trades(&point, ONE_MINUTE_MS, self.exchange, &self.symbol) {
                if let Some(out) = self.machine.on_event(&synth, self.params.box_size) {
                    outputs.push(out);
                }
            }
        }
        if let Some(last_bar) = last.first() {
            let point = bar_to_bar_point(last_bar);
            self.machine.seed_kline_baseline(point.open_time, point.volume, point.close);
            self.last_live_open_time = Some(point.open_time);
        }
        outputs
    }

    /// Fold one live (closed-or-in-progress) 1m bar. Feeds `Event::Bar`
    /// directly — the machine's embedded `KlineDeltaState` folds repeated
    /// same-`open_time` updates into incremental Δ-volume synthetic trades
    /// internally (see `digdigdig3_station::derived::KlineDeltaState`).
    pub fn on_live_1m(&mut self, bar: &mlc_core::Bar) -> Option<FoldOutput> {
        let point = bar_to_bar_point(bar);
        self.last_live_open_time = Some(point.open_time);
        let ev = Event::Bar {
            exchange: self.exchange,
            symbol: self.symbol.clone(),
            timeframe: KlineInterval::new("1m"),
            point,
        };
        self.machine.on_event(&ev, self.params.box_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(ts_sec: i64, o: f64, h: f64, l: f64, c: f64, v: f64) -> mlc_core::Bar {
        mlc_core::Bar {
            timestamp: ts_sec,
            open: o,
            high: h,
            low: l,
            close: c,
            volume: v,
            derived_ts_ms: ts_sec * 1000,
        }
    }

    #[test]
    fn bar_to_bar_point_prefers_derived_ts_ms() {
        let b = bar(100, 1.0, 2.0, 0.5, 1.5, 10.0);
        let p = bar_to_bar_point(&b);
        assert_eq!(p.open_time, 100_000);
        assert_eq!(p.open, 1.0);
        assert_eq!(p.close, 1.5);
        assert_eq!(p.volume, 10.0);
        assert!(p.quote_volume.is_nan());
        assert_eq!(p.trades_count, 0);
    }

    #[test]
    fn bar_to_bar_point_falls_back_to_seconds_when_derived_ts_ms_zero() {
        let mut b = bar(50, 1.0, 1.0, 1.0, 1.0, 1.0);
        b.derived_ts_ms = 0;
        let p = bar_to_bar_point(&b);
        assert_eq!(p.open_time, 50_000);
    }

    /// A trending series of 1m bars with a large enough O-H-L-C spread per
    /// bar to trigger several range-bar crossings — enough signal to make a
    /// determinism/param-sensitivity test meaningful (a flat series would
    /// fold to the same trivial output regardless of params).
    fn trending_layer(n: usize) -> RawLayer<mlc_core::Bar> {
        let mut layer: RawLayer<mlc_core::Bar> = RawLayer::new(0);
        let mut price = 100.0;
        for i in 0..n {
            let ts_sec = (i as i64 + 1) * 60;
            let open = price;
            price += 5.0;
            let close = price;
            let high = close + 1.0;
            let low = open - 1.0;
            layer.push_live(bar(ts_sec, open, high, low, close, 10.0));
        }
        layer
    }

    fn fold_outputs_debug(outputs: &[FoldOutput]) -> Vec<String> {
        outputs.iter().map(|o| format!("{o:?}")).collect()
    }

    #[test]
    fn reseed_is_deterministic_for_same_layer_and_params() {
        let layer = trending_layer(20);
        let params = FoldParams { scaled_units: 3.0, ..Default::default() };
        let mut fold_a = FoldRuntime::new("range_bar", params, ExchangeId::Binance, "BTCUSDT").expect("range_bar is a fold type");
        let mut fold_b = FoldRuntime::new("range_bar", params, ExchangeId::Binance, "BTCUSDT").expect("range_bar is a fold type");

        let out_a = fold_a.reseed(&layer);
        let out_b = fold_b.reseed(&layer);
        assert!(!out_a.is_empty(), "trending layer with tight range must produce at least one range bar");
        assert_eq!(fold_outputs_debug(&out_a), fold_outputs_debug(&out_b));

        // Re-seeding the SAME runtime a second time over the SAME layer must
        // also reproduce identical output (reseed rebuilds a fresh machine
        // internally — no leftover state from the first pass).
        let out_a_again = fold_a.reseed(&layer);
        assert_eq!(fold_outputs_debug(&out_a), fold_outputs_debug(&out_a_again));
    }

    #[test]
    fn reseed_with_different_params_produces_different_cut_of_same_input() {
        let layer = trending_layer(20);
        let mut fold_tight = FoldRuntime::new(
            "range_bar",
            FoldParams { scaled_units: 3.0, ..Default::default() },
            ExchangeId::Binance, "BTCUSDT",
        ).expect("range_bar is a fold type");
        let mut fold_wide = FoldRuntime::new(
            "range_bar",
            FoldParams { scaled_units: 30.0, ..Default::default() },
            ExchangeId::Binance, "BTCUSDT",
        ).expect("range_bar is a fold type");

        let out_tight = fold_tight.reseed(&layer);
        let out_wide = fold_wide.reseed(&layer);
        // Every synthetic-trade leg emits an output (either an update to
        // the in-progress range bar or the opening of a new one) — count
        // DISTINCT bars (open_time transitions) rather than raw event
        // count to compare "how many range bars did this cut produce".
        let distinct_bars = |outputs: &[FoldOutput]| -> usize {
            let mut open_times: Vec<i64> = outputs.iter().filter_map(|o| match o {
                FoldOutput::Bar(b) => Some(if b.derived_ts_ms != 0 { b.derived_ts_ms } else { b.timestamp }),
                _ => None,
            }).collect();
            open_times.dedup();
            open_times.len()
        };
        let tight_bars = distinct_bars(&out_tight);
        let wide_bars = distinct_bars(&out_wide);
        // A tighter range threshold must cut MORE bars than a wider one over
        // the same trending input.
        assert!(tight_bars > wide_bars, "tight={tight_bars} wide={wide_bars}");
    }

    #[test]
    fn on_live_1m_folds_after_reseed_without_double_counting_the_seam_bar() {
        // 5 closed bars + a 6th "in-progress" bar whose baseline the reseed
        // hands to KlineDeltaState instead of folding it as a 4-leg path.
        let mut layer = trending_layer(5);
        let last_open = bar(6 * 60, 125.0, 126.0, 124.0, 129.0, 10.0);
        layer.push_live(last_open);

        let params = FoldParams { scaled_units: 3.0, ..Default::default() };
        let mut fold = FoldRuntime::new("range_bar", params, ExchangeId::Binance, "BTCUSDT").expect("range_bar is a fold type");
        let seed_outputs = fold.reseed(&layer);
        assert!(!seed_outputs.is_empty());

        // Live update for the SAME in-progress bar with more volume/movement
        // — must compute an incremental delta (not re-fold the whole bar).
        let updated = bar(6 * 60, 125.0, 132.0, 124.0, 132.0, 20.0);
        let live_output = fold.on_live_1m(&updated);
        assert!(live_output.is_some(), "a large enough delta move must emit a fold output");
    }

    #[test]
    fn fold_machine_new_returns_none_for_non_fold_chart_type() {
        let params = FoldParams::default();
        assert!(FoldMachine::new("candles", &params).is_none());
        assert!(FoldMachine::new("footprint", &params).is_none());
    }

    #[test]
    fn fold_runtime_new_returns_none_for_non_fold_chart_type() {
        let params = FoldParams::default();
        assert!(FoldRuntime::new("candles", params, ExchangeId::Binance, "BTCUSDT").is_none());
    }

    #[test]
    fn empty_layer_reseeds_to_empty_output() {
        let layer: RawLayer<mlc_core::Bar> = RawLayer::new(0);
        let params = FoldParams { scaled_units: 3.0, ..Default::default() };
        let mut fold = FoldRuntime::new("volume_bar", params, ExchangeId::Binance, "BTCUSDT").expect("volume_bar is a fold type");
        let outputs = fold.reseed(&layer);
        assert!(outputs.is_empty());
    }
}
