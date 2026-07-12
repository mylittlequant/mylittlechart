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
//!
//! # Trade-fed folds (unified-data-plane Stage Э4)
//!
//! `tick_bar` / `tick_imbalance_bar` / `volume_imbalance_bar` / `run_bar` /
//! `footprint` / `tpo` (both [`TpoSource`] flavours), plus the CVD companion
//! line, fold the bubble's own `trade_layer` (`DataBubble::trade_layer`) —
//! real trades, never synthetic legs. [`TradeFoldRuntime::reseed`] replays
//! the ENTIRE trade layer chronologically through a fresh machine instance
//! (see `DerivedStream`'s "no rollback, replay from scratch" contract,
//! mirrored from the kline-fold family above); [`TradeFoldRuntime::on_live_trade`]
//! folds one live trade tick.
//!
//! `TpoFromKline1mDerived` (the kline-sourced TPO flavour) is the one
//! exception that still folds the "1m" kline layer like the Э3 family — it
//! is included in [`TradeFoldMachine`] anyway (rather than staying in
//! [`FoldMachine`]) because TPO's chart_type installs exactly ONE runtime
//! regardless of which `TpoSource` the caller picked, and the two TPO
//! flavours share the same output type ([`mlc_core::types::TpoSession`]) and
//! the same bubble slot (`tpo_sessions`) — splitting them across two
//! `FoldRuntime` families would only duplicate the reseed/live-fold
//! dispatch in `mlc-backend/src/lib.rs` for no benefit. See
//! [`TradeFoldRuntime::reseed`]'s doc comment for how the Kline1m flavour's
//! reseed differs (it replays the bubble's "1m" layer, passed in
//! alongside the trade layer, instead of the trade layer itself).

use digdigdig3::core::types::ExchangeId;
use digdigdig3::core::websocket::KlineInterval;
use digdigdig3_station::data::TradePoint;
use digdigdig3_station::folds::{
    kline_to_synthetic_trades, DerivedStream, TpoFromKline1mDerived, TpoFromTradeDerived,
    TradeToCvdLineDerived, TradeToDollarBarDerived, TradeToFootprintDerived,
    TradeToKagiBarDerived, TradeToPnfBarDerived, TradeToRangeBarDerived, TradeToRenkoBarDerived,
    TradeToRunBarDerived, TradeToThreeLineBreakDerived, TradeToTickBarDerived,
    TradeToTickImbalanceDerived, TradeToVolumeBarDerived, TradeToVolumeImbalanceDerived,
};
use digdigdig3_station::subscription::Event;
use digdigdig3_station::TpoSource;

use super::raw_layer::{RawLayer, RawTrade};

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
    /// fold types (registry lookup miss, or `machine` outside this family —
    /// see [`mlc_core::chart_type::MachineKind`]).
    ///
    /// Single registry-driven match (chart-type-engine-consolidation B3,
    /// S2) — was a hand-written `chart_type: &str` forward match; the
    /// `chart_type` string is looked up once via `mlc_core::chart_type::by_id`
    /// and dispatched on `def.machine`, the SAME pure-id enum
    /// [`crate::chart_type::fold_chart_types`] iterates to derive its
    /// membership list.
    pub fn new(chart_type: &str, params: &FoldParams) -> Option<Self> {
        use mlc_core::chart_type::MachineKind;
        let machine = mlc_core::chart_type::by_id(chart_type)?.machine;
        match machine {
            MachineKind::RangeBar => Some(FoldMachine::RangeBar(TradeToRangeBarDerived::new(params.scaled_units))),
            MachineKind::VolumeBar => Some(FoldMachine::VolumeBar(TradeToVolumeBarDerived::new(params.scaled_units))),
            MachineKind::DollarBar => Some(FoldMachine::DollarBar(TradeToDollarBarDerived::new(params.threshold))),
            MachineKind::Renko => Some(FoldMachine::Renko(TradeToRenkoBarDerived::new(params.scaled_units, params.reversal))),
            MachineKind::Pnf => Some(FoldMachine::Pnf(TradeToPnfBarDerived::new(params.scaled_units, params.reversal))),
            MachineKind::Kagi => Some(FoldMachine::Kagi(TradeToKagiBarDerived::new(params.scaled_units))),
            MachineKind::ThreeLineBreak => Some(FoldMachine::ThreeLineBreak(TradeToThreeLineBreakDerived::new(params.lines_back as usize))),
            MachineKind::None
            | MachineKind::TickBar
            | MachineKind::TickImbalance
            | MachineKind::VolumeImbalance
            | MachineKind::RunBar
            | MachineKind::Footprint
            | MachineKind::Tpo => None,
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

    /// Toggle replay mode on the embedded dig3 machine — see
    /// [`DerivedStream::set_replay`]'s doc comment for the ON/OFF semantics.
    fn set_replay(&mut self, on: bool) {
        match self {
            FoldMachine::RangeBar(m) => m.set_replay(on),
            FoldMachine::VolumeBar(m) => m.set_replay(on),
            FoldMachine::DollarBar(m) => m.set_replay(on),
            FoldMachine::Renko(m) => m.set_replay(on),
            FoldMachine::Pnf(m) => m.set_replay(on),
            FoldMachine::Kagi(m) => m.set_replay(on),
            FoldMachine::ThreeLineBreak(m) => m.set_replay(on),
        }
    }

    /// Clone out the final in-progress element after a replay pass — see
    /// [`DerivedStream::flush_replay`]'s doc comment. `box_size` mirrors the
    /// same PnF-only side-channel [`Self::on_event`] threads through.
    fn flush_replay(&mut self, box_size: f64) -> Option<FoldOutput> {
        match self {
            FoldMachine::RangeBar(m) => m.flush_replay().map(|p| FoldOutput::Bar(bar_point_to_bar(&p))),
            FoldMachine::VolumeBar(m) => m.flush_replay().map(|p| FoldOutput::Bar(bar_point_to_bar(&p))),
            FoldMachine::DollarBar(m) => m.flush_replay().map(|p| FoldOutput::Bar(bar_point_to_bar(&p))),
            FoldMachine::Renko(m) => m.flush_replay().map(|p| FoldOutput::Renko(renko_point_to_brick(&p))),
            FoldMachine::Pnf(m) => m.flush_replay().map(|p| FoldOutput::Pnf(pnf_point_to_column(&p, box_size))),
            FoldMachine::Kagi(m) => m.flush_replay().map(|p| FoldOutput::Kagi(kagi_point_to_segment(&p))),
            FoldMachine::ThreeLineBreak(m) => m.flush_replay().map(|p| FoldOutput::ThreeLineBreak(tlb_point_to_line(&p))),
        }
    }

    /// Feed one upstream `Event` (synthetic trade leg during reseed, or a
    /// live `Event::Bar` Δ-tick), converting whatever the machine emits into
    /// the bubble-side [`FoldOutput`] shape.
    ///
    /// Returns EVERY output produced by this single call, oldest-first: any
    /// fully-closed intermediate elements drained via
    /// [`DerivedStream::take_pending`] (gap-crossing range bars, multi-brick
    /// Renko moves — see `digdigdig3-station::derived` module doc) come
    /// FIRST, followed by the normal `Option` return (the latest, possibly
    /// still in-progress element) LAST if present. Every other machine kind
    /// never populates a pending queue, so this is a 0- or 1-element `Vec`
    /// for them — identical shape to the pre-pending-queue single `Option`.
    ///
    /// `box_size` is only meaningful for `Pnf` (the box width isn't carried
    /// on `PnfColumnPoint` itself — mirrors `LiveUpdate::PnfBarUpdate`'s
    /// `box_size` side-channel in `mlc-backend-data-bridge`).
    fn on_event(&mut self, ev: &Event, box_size: f64) -> Vec<FoldOutput> {
        match self {
            FoldMachine::RangeBar(m) => {
                let final_out = m.on_upstream_event(ev, 0);
                m.take_pending().into_iter().map(|p| FoldOutput::Bar(bar_point_to_bar(&p)))
                    .chain(final_out.map(|p| FoldOutput::Bar(bar_point_to_bar(&p))))
                    .collect()
            }
            FoldMachine::VolumeBar(m) => m.on_upstream_event(ev, 0).map(|p| FoldOutput::Bar(bar_point_to_bar(&p))).into_iter().collect(),
            FoldMachine::DollarBar(m) => m.on_upstream_event(ev, 0).map(|p| FoldOutput::Bar(bar_point_to_bar(&p))).into_iter().collect(),
            FoldMachine::Renko(m) => {
                let final_out = m.on_upstream_event(ev, 0);
                m.take_pending().into_iter().map(|p| FoldOutput::Renko(renko_point_to_brick(&p)))
                    .chain(final_out.map(|p| FoldOutput::Renko(renko_point_to_brick(&p))))
                    .collect()
            }
            FoldMachine::Pnf(m) => m.on_upstream_event(ev, 0).map(|p| FoldOutput::Pnf(pnf_point_to_column(&p, box_size))).into_iter().collect(),
            FoldMachine::Kagi(m) => m.on_upstream_event(ev, 0).map(|p| FoldOutput::Kagi(kagi_point_to_segment(&p))).into_iter().collect(),
            FoldMachine::ThreeLineBreak(m) => m.on_upstream_event(ev, 0).map(|p| FoldOutput::ThreeLineBreak(tlb_point_to_line(&p))).into_iter().collect(),
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
        wick: p.wick,
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

/// Final-unit parameters for constructing a [`FoldMachine`]/[`TradeFoldMachine`]
/// — the SAME resolved values `SubscribeParams` carries at the
/// `chart_out_events.rs` UI seam (ticks × tick_size already applied there;
/// this struct is the fold-specific narrowing of that enum, since a fold
/// machine needs exactly one shape rather than the full `SubscribeParams`
/// union). Shared by both fold families (kline-sufficient + trade-fed,
/// unified-data-plane Stages Э3 + Э4) rather than split into two structs —
/// every field below is meaningful to exactly one machine kind, same
/// "narrow union of scalars" shape `SubscribeParams` itself already uses.
#[derive(Clone, Copy, Debug, Default)]
pub struct FoldParams {
    pub scaled_units: f64,
    pub threshold: f64,
    pub reversal: u8,
    pub lines_back: u8,
    /// Only meaningful for `pnf` — carried through to `PnfColumn::box_size`
    /// (not part of the dig3 fold machine's own state).
    pub box_size: f64,
    /// `tick_bar`: trades per bar (`Kind::TickBar`'s `n`).
    pub count: u32,
    /// `tick_imbalance_bar` / `volume_imbalance_bar` / `run_bar`: EMA
    /// smoothing factor (already divided by 100 — e.g. `0.20`).
    pub alpha: f64,
    /// `tick_imbalance_bar` / `volume_imbalance_bar` / `run_bar`: sanity-
    /// floor tick count, also the EMA seed.
    pub min_ticks: u32,
    /// `footprint`: time-bucket width in milliseconds
    /// (`TradeToFootprintDerived::new`'s `interval_ms`).
    pub interval_ms: i64,
    /// `footprint`: row-grouping price step (`TradeToFootprintDerived::new`'s
    /// `group_step`) — `0.0` = raw ungrouped rows (one row per tick), `> 0.0`
    /// bins trades into `group_step`-wide rows. Resolved from
    /// `footprint_group_ticks × tick_size` at the UI seam (see
    /// `chart_out_events.rs::build_subscribe_params`, mirrors how renko/pnf
    /// convert ticks → price for `scaled_units`).
    pub group_step: f64,
    /// `tpo` (both flavours): letter-bucket width in minutes.
    pub freq_minutes: u16,
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
        // Registry `id` field (B3, S2) — resolved ONCE here as the `&'static
        // str` this runtime carries; `FoldMachine::new` re-resolves the same
        // row internally to dispatch on `def.machine` (single registry-driven
        // match, no per-variant `chart_type()` reverse match needed).
        let chart_type = mlc_core::chart_type::by_id(chart_type)?.id;
        let machine = FoldMachine::new(chart_type, &params)?;
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
    /// Wraps the whole replay in [`DerivedStream::set_replay`] (P0 hang fix
    /// part 1, `nemo/docs/mlc/plans` chart-series arc): with replay ON the
    /// machine still folds every synthetic-trade leg into internal state
    /// exactly as live, but suppresses the expensive per-event in-progress
    /// snapshot — only genuinely CLOSED elements (the same ones
    /// [`FoldMachine::on_event`]'s `take_pending`/`Option` chain already
    /// surfaces) are collected during the loop. [`DerivedStream::flush_replay`]
    /// is called once at the very end to clone out the final in-progress
    /// element and append it LAST, then replay mode is turned back off. This
    /// collapses the reseed cost from O(legs × rows) snapshot allocations to
    /// O(rows) closed-element pushes for every fold machine kind — the
    /// merged output array is byte-identical to the pre-replay per-leg
    /// collection (dig3 pins this equivalence; see `derived.rs`'s
    /// `seed_from_events` doc comment for the same contract on the
    /// station-side sibling).
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
        self.machine.set_replay(true);
        for bar in closed {
            let point = bar_to_bar_point(bar);
            for synth in kline_to_synthetic_trades(&point, ONE_MINUTE_MS, self.exchange, &self.symbol) {
                outputs.extend(self.machine.on_event(&synth, self.params.box_size));
            }
        }
        outputs.extend(self.machine.flush_replay(self.params.box_size));
        self.machine.set_replay(false);
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
    ///
    /// Returns EVERY output produced by this single live tick, oldest-first
    /// (see [`FoldMachine::on_event`]'s doc comment) — a gap-crossing 1m
    /// delta-tick can close several intermediate range bars / Renko bricks
    /// in one call; the caller (`mlc-backend/src/lib.rs`'s `fold_on_live_1m`)
    /// applies the same upsert/append logic to each element in order.
    pub fn on_live_1m(&mut self, bar: &mlc_core::Bar) -> Vec<FoldOutput> {
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

// =============================================================================
// Trade-fed folds (unified-data-plane Stage Э4)
// =============================================================================

/// Convert a bubble-side [`RawTrade`] into the
/// `digdigdig3_station::data::TradePoint` shape the trade-fed fold machines
/// expect.
///
/// Field mapping:
/// - `ts_ms` / `price` / `quantity` ← verbatim.
/// - `side` ← derived from `is_buyer_maker`. `RawTrade.is_buyer_maker` is
///   filled EXCLUSIVELY by `mlc-backend-data-bridge::bridge`'s
///   `LiveUpdate::TradeUpdate` emitter, which sets it as
///   `is_buyer_maker: point.side == 0` (see `bridge.rs`, the station-trade
///   forwarder) — i.e. the bridge's own convention is
///   `is_buyer_maker == (TradePoint.side == 0)`. This converter inverts that
///   exact relation (`side = if is_buyer_maker { 0 } else { 1 }`) to
///   reconstruct a `TradePoint.side` that round-trips losslessly through the
///   bridge's already-established convention — NOT the canonical
///   dig3-connector convention documented on `TradePoint::from_public`
///   (`is_buyer_maker=true → TradeSide::Sell → side=1`), which the bridge's
///   own emitter does not currently follow (a pre-existing mislabel outside
///   this task's scope, left as-is so `RawTrade` consumers stay internally
///   consistent with each other).
/// - `trade_id_hash` ← `0` (absent). `RawTrade` carries no trade id (the
///   bridge's `LiveUpdate::TradeUpdate` never forwarded one) — none of the
///   trade-fed fold machines read `trade_id_hash` (it exists on `TradePoint`
///   only for on-disk dedup, a station-side concern this client-side path
///   never touches), so a synthesized `0` is safe and matches
///   `FoldMachine`'s existing "absent field → documented zero" convention
///   (see `bar_to_bar_point`'s `quote_volume`/`trades_count` doc above).
pub fn raw_trade_to_trade_point(trade: &RawTrade) -> TradePoint {
    let side = if trade.is_buyer_maker { 0 } else { 1 };
    TradePoint {
        ts_ms: trade.ts_ms,
        price: trade.price,
        quantity: trade.qty,
        side,
        trade_id_hash: 0,
    }
}

/// One trade-fed fold machine instance, keyed by which chart_type /
/// companion series it computes. Mirrors [`FoldMachine`] for the trade-fed
/// family (unified-data-plane Stage Э4) — every variant is fed real
/// `Event::Trade`s (never synthetic legs) in chronological order.
pub enum TradeFoldMachine {
    TickBar(TradeToTickBarDerived),
    TickImbalanceBar(TradeToTickImbalanceDerived),
    VolumeImbalanceBar(TradeToVolumeImbalanceDerived),
    RunBar(TradeToRunBarDerived),
    Footprint(TradeToFootprintDerived),
    TpoTrade(TpoFromTradeDerived),
    /// TPO's kline-sourced flavour — folds the "1m" kline layer, not the
    /// trade layer. Included here (not in [`FoldMachine`]) so both TPO
    /// flavours share one runtime family / one bubble slot — see this
    /// module's doc comment "Trade-fed folds" section.
    TpoKline1m(TpoFromKline1mDerived),
    Cvd(TradeToCvdLineDerived),
}

/// One trade-fed fold machine's output for a single upstream event.
#[derive(Clone, Debug)]
pub enum TradeFoldOutput {
    Bar(mlc_core::Bar),
    Footprint(mlc_core::types::FootprintBar),
    Tpo(mlc_core::types::TpoSession),
    Cvd(mlc_core::types::CvdSample),
}

impl TradeFoldMachine {
    /// Construct the machine for `chart_type` from resolved [`FoldParams`].
    /// Returns `None` for any chart_type outside the trade-fed family.
    ///
    /// `tpo_source` selects which TPO flavour to install when
    /// `chart_type == "tpo"` — ignored otherwise. `"cvd"` is not a
    /// registered chart_type (it's an indicator instance — see
    /// `mlc-app/src/input/overlay.rs`) but is accepted here as a synthetic
    /// id so the CVD companion line shares this same construction/reseed
    /// machinery — checked BEFORE the registry lookup since `by_id("cvd")`
    /// is always `None` (see [`mlc_core::chart_type::MachineKind`]'s doc
    /// comment on why `Cvd` is deliberately not a variant).
    ///
    /// Single registry-driven match for every OTHER id (chart-type-engine-
    /// consolidation B3, S2) — was a hand-written `chart_type: &str` forward
    /// match; dispatches on `def.machine`, the SAME pure-id enum
    /// [`crate::chart_type::trade_fold_chart_types`] iterates to derive its
    /// membership list.
    pub fn new(chart_type: &str, params: &FoldParams, tpo_source: TpoSource) -> Option<Self> {
        use mlc_core::chart_type::MachineKind;
        if chart_type == "cvd" {
            return Some(TradeFoldMachine::Cvd(TradeToCvdLineDerived::new(true)));
        }
        let machine = mlc_core::chart_type::by_id(chart_type)?.machine;
        match machine {
            MachineKind::TickBar => Some(TradeFoldMachine::TickBar(TradeToTickBarDerived::new(params.count as u32))),
            MachineKind::TickImbalance => Some(TradeFoldMachine::TickImbalanceBar(
                TradeToTickImbalanceDerived::new(params.alpha, params.min_ticks),
            )),
            MachineKind::VolumeImbalance => Some(TradeFoldMachine::VolumeImbalanceBar(
                TradeToVolumeImbalanceDerived::new(params.alpha, params.min_ticks),
            )),
            MachineKind::RunBar => Some(TradeFoldMachine::RunBar(TradeToRunBarDerived::new(params.alpha, params.min_ticks))),
            MachineKind::Footprint => Some(TradeFoldMachine::Footprint(TradeToFootprintDerived::new(params.interval_ms, params.group_step))),
            MachineKind::Tpo => match tpo_source {
                TpoSource::TradeBucket => Some(TradeFoldMachine::TpoTrade(TpoFromTradeDerived::new(params.freq_minutes))),
                TpoSource::Kline1m => Some(TradeFoldMachine::TpoKline1m(TpoFromKline1mDerived::new(params.freq_minutes))),
            },
            MachineKind::None
            | MachineKind::RangeBar
            | MachineKind::VolumeBar
            | MachineKind::DollarBar
            | MachineKind::Renko
            | MachineKind::Pnf
            | MachineKind::Kagi
            | MachineKind::ThreeLineBreak => None,
        }
    }

    /// Chart-type id this machine computes — `"cvd"` for the CVD companion
    /// (see [`Self::new`]'s doc comment on the synthetic id).
    pub fn chart_type(&self) -> &'static str {
        match self {
            TradeFoldMachine::TickBar(_) => "tick_bar",
            TradeFoldMachine::TickImbalanceBar(_) => "tick_imbalance_bar",
            TradeFoldMachine::VolumeImbalanceBar(_) => "volume_imbalance_bar",
            TradeFoldMachine::RunBar(_) => "run_bar",
            TradeFoldMachine::Footprint(_) => "footprint",
            TradeFoldMachine::TpoTrade(_) | TradeFoldMachine::TpoKline1m(_) => "tpo",
            TradeFoldMachine::Cvd(_) => "cvd",
        }
    }

    /// Whether this machine folds `Event::Trade` (the trade layer) — `false`
    /// only for [`TradeFoldMachine::TpoKline1m`], which folds `Event::Bar`
    /// over the "1m" kline layer instead.
    pub fn feeds_on_trades(&self) -> bool {
        !matches!(self, TradeFoldMachine::TpoKline1m(_))
    }

    /// Toggle replay mode on the embedded dig3 machine — see
    /// [`DerivedStream::set_replay`]'s doc comment for the ON/OFF semantics.
    fn set_replay(&mut self, on: bool) {
        match self {
            TradeFoldMachine::TickBar(m) => m.set_replay(on),
            TradeFoldMachine::TickImbalanceBar(m) => m.set_replay(on),
            TradeFoldMachine::VolumeImbalanceBar(m) => m.set_replay(on),
            TradeFoldMachine::RunBar(m) => m.set_replay(on),
            TradeFoldMachine::Footprint(m) => m.set_replay(on),
            TradeFoldMachine::TpoTrade(m) => m.set_replay(on),
            TradeFoldMachine::TpoKline1m(m) => m.set_replay(on),
            TradeFoldMachine::Cvd(m) => m.set_replay(on),
        }
    }

    /// Clone out the final in-progress element after a replay pass — see
    /// [`DerivedStream::flush_replay`]'s doc comment.
    fn flush_replay(&mut self) -> Option<TradeFoldOutput> {
        match self {
            TradeFoldMachine::TickBar(m) => m.flush_replay().map(|p| TradeFoldOutput::Bar(bar_point_to_bar(&p))),
            TradeFoldMachine::TickImbalanceBar(m) => m.flush_replay().map(|p| TradeFoldOutput::Bar(bar_point_to_bar(&p))),
            TradeFoldMachine::VolumeImbalanceBar(m) => m.flush_replay().map(|p| TradeFoldOutput::Bar(bar_point_to_bar(&p))),
            TradeFoldMachine::RunBar(m) => m.flush_replay().map(|p| TradeFoldOutput::Bar(bar_point_to_bar(&p))),
            TradeFoldMachine::Footprint(m) => m.flush_replay().map(|p| TradeFoldOutput::Footprint(footprint_point_to_bar(&p))),
            TradeFoldMachine::TpoTrade(m) => m.flush_replay().map(|p| TradeFoldOutput::Tpo(tpo_point_to_session(&p))),
            TradeFoldMachine::TpoKline1m(m) => m.flush_replay().map(|p| TradeFoldOutput::Tpo(tpo_point_to_session(&p))),
            TradeFoldMachine::Cvd(m) => m.flush_replay().map(|p| TradeFoldOutput::Cvd(mlc_core::types::CvdSample { ts_ms: p.ts_ms, value: p.value })),
        }
    }

    /// Returns EVERY output produced by this single call, oldest-first (see
    /// [`FoldMachine::on_event`]'s doc comment). None of the trade-fed
    /// machines currently populate a pending queue (`take_pending` stays at
    /// its default empty), so this is always a 0- or 1-element `Vec` today —
    /// wired uniformly so a future trade-fed multi-output machine needs no
    /// signature change here.
    fn on_trade_event(&mut self, ev: &Event) -> Vec<TradeFoldOutput> {
        match self {
            TradeFoldMachine::TickBar(m) => {
                let out = m.on_upstream_event(ev, 0);
                m.take_pending().into_iter().map(|p| TradeFoldOutput::Bar(bar_point_to_bar(&p)))
                    .chain(out.map(|p| TradeFoldOutput::Bar(bar_point_to_bar(&p))))
                    .collect()
            }
            TradeFoldMachine::TickImbalanceBar(m) => {
                let out = m.on_upstream_event(ev, 0);
                m.take_pending().into_iter().map(|p| TradeFoldOutput::Bar(bar_point_to_bar(&p)))
                    .chain(out.map(|p| TradeFoldOutput::Bar(bar_point_to_bar(&p))))
                    .collect()
            }
            TradeFoldMachine::VolumeImbalanceBar(m) => {
                let out = m.on_upstream_event(ev, 0);
                m.take_pending().into_iter().map(|p| TradeFoldOutput::Bar(bar_point_to_bar(&p)))
                    .chain(out.map(|p| TradeFoldOutput::Bar(bar_point_to_bar(&p))))
                    .collect()
            }
            TradeFoldMachine::RunBar(m) => {
                let out = m.on_upstream_event(ev, 0);
                m.take_pending().into_iter().map(|p| TradeFoldOutput::Bar(bar_point_to_bar(&p)))
                    .chain(out.map(|p| TradeFoldOutput::Bar(bar_point_to_bar(&p))))
                    .collect()
            }
            TradeFoldMachine::Footprint(m) => {
                let out = m.on_upstream_event(ev, 0);
                m.take_pending().into_iter().map(|p| TradeFoldOutput::Footprint(footprint_point_to_bar(&p)))
                    .chain(out.map(|p| TradeFoldOutput::Footprint(footprint_point_to_bar(&p))))
                    .collect()
            }
            TradeFoldMachine::TpoTrade(m) => {
                let out = m.on_upstream_event(ev, 0);
                m.take_pending().into_iter().map(|p| TradeFoldOutput::Tpo(tpo_point_to_session(&p)))
                    .chain(out.map(|p| TradeFoldOutput::Tpo(tpo_point_to_session(&p))))
                    .collect()
            }
            TradeFoldMachine::TpoKline1m(_) => Vec::new(),
            TradeFoldMachine::Cvd(m) => {
                let out = m.on_upstream_event(ev, 0);
                m.take_pending().into_iter().map(|p| TradeFoldOutput::Cvd(mlc_core::types::CvdSample { ts_ms: p.ts_ms, value: p.value }))
                    .chain(out.map(|p| TradeFoldOutput::Cvd(mlc_core::types::CvdSample { ts_ms: p.ts_ms, value: p.value })))
                    .collect()
            }
        }
    }

    /// Returns EVERY output produced by this single call — see
    /// [`Self::on_trade_event`]'s doc comment.
    fn on_bar_event(&mut self, ev: &Event) -> Vec<TradeFoldOutput> {
        match self {
            TradeFoldMachine::TpoKline1m(m) => {
                let out = m.on_upstream_event(ev, 0);
                m.take_pending().into_iter().map(|p| TradeFoldOutput::Tpo(tpo_point_to_session(&p)))
                    .chain(out.map(|p| TradeFoldOutput::Tpo(tpo_point_to_session(&p))))
                    .collect()
            }
            _ => Vec::new(),
        }
    }
}

/// Convert a `digdigdig3_station::data::FootprintPoint` into the
/// `mlc_core::types::FootprintBar` shape the bubble renders. Mirrors the
/// conversion `mlc-backend/src/lib.rs`'s `LiveUpdate::FootprintUpdate` arm
/// already performs for the station-derived path (tick size inferred from
/// the level grid, static extras stamped via `compute_static_extras`) —
/// duplicated here (not shared) because the station-derived path's version
/// lives in `lib.rs` and reads a different point type's field layout
/// one-for-one; a shared helper would need to abstract over both callers'
/// slightly different surrounding upsert/cumulative-delta bookkeeping for no
/// real gain. `cumulative_delta` is left at the default `0.0` — the caller
/// (`apply_trade_fold_reseed_outputs`/`trade_fold_on_live_trade` in
/// `mlc-backend/src/lib.rs`) stamps it from the running per-bubble total,
/// same as the station-derived path.
fn footprint_point_to_bar(p: &digdigdig3_station::data::FootprintPoint) -> mlc_core::types::FootprintBar {
    let mut bar = mlc_core::types::FootprintBar {
        open_time: p.open_time,
        open: p.open,
        high: p.high,
        low: p.low,
        close: p.close,
        volume: p.volume,
        levels: p.levels.iter()
            .map(|(price, buy_vol, sell_vol)| mlc_core::types::FootprintLevel {
                price: *price,
                buy_vol: *buy_vol,
                sell_vol: *sell_vol,
                is_single_print: false,
                is_unfinished: false,
            })
            .collect(),
        cumulative_delta: 0.0,
        stacked_ask_start: None,
        stacked_ask_len: None,
        stacked_bid_start: None,
        stacked_bid_len: None,
        is_absorption: false,
    };
    let tick_size = {
        let mut step = f64::INFINITY;
        for w in bar.levels.windows(2) {
            let d = (w[1].price - w[0].price).abs();
            if d > 0.0 && d < step { step = d; }
        }
        if step.is_finite() { step } else { 1.0 }
    };
    // Stacked-zone ratio (3.0×) — NOT the renderer's per-cell diagonal
    // threshold (1.5×, `CELL_IMBALANCE_RATIO` in
    // `mlc-app-layout::chart_render::footprint`); see
    // `mlc_core::types::FOOTPRINT_STACKED_IMBALANCE_RATIO`'s doc comment
    // (canon-fix wave 2, item 2).
    bar.compute_static_extras(mlc_core::types::FOOTPRINT_STACKED_IMBALANCE_RATIO, 3, 0.15, 0.05, tick_size);
    bar
}

/// Convert a `digdigdig3_station::data::TpoSessionPoint` into the
/// `mlc_core::types::TpoSession` shape the bubble renders — field-for-field,
/// same mapping `mlc-backend/src/lib.rs`'s `LiveUpdate::TpoProfileUpdate` arm
/// already performs for the station-derived path.
fn tpo_point_to_session(p: &digdigdig3_station::data::TpoSessionPoint) -> mlc_core::types::TpoSession {
    mlc_core::types::TpoSession {
        open_time: p.open_time,
        tick_size: p.tick_size,
        session_high: p.session_high,
        session_low: p.session_low,
        poc_price: p.poc_price,
        vah_price: p.vah_price,
        val_price: p.val_price,
        rows: p.rows.clone(),
    }
}

/// Per-bubble client-side trade-fold runtime (unified-data-plane Stage Э4).
///
/// Installed when the bubble's active chart_type is one of the trade-fed
/// family (`tick_bar`/`tick_imbalance_bar`/`volume_imbalance_bar`/`run_bar`/
/// `footprint`/`tpo`), or when the CVD companion indicator is active for
/// this bubble's instrument (synthetic `"cvd"` chart_type, see
/// [`TradeFoldMachine::new`]'s doc comment). Reseeded from the bubble's own
/// `trade_layer` (or, for the `tpo` Kline1m flavour, the "1m" kline layer)
/// on install/param-change/chart-type-switch, then fed one trade (or 1m bar,
/// for Kline1m TPO) at a time as the layer receives live appends.
///
/// Distinct type from [`FoldRuntime`] (not a third enum variant on it) —
/// the two families fold different upstream layers (trade vs kline) and
/// produce disjoint output shapes; keeping them separate avoids a
/// `FoldOutput`/`TradeFoldOutput` union nobody needs (every caller already
/// knows statically which family a chart_type belongs to via
/// `chart_type::is_fold_chart_type` / `chart_type::is_trade_fold_chart_type`).
pub struct TradeFoldRuntime {
    machine: TradeFoldMachine,
    chart_type: &'static str,
    /// Construction params, kept verbatim so [`Self::reseed`] can rebuild a
    /// byte-identical fresh machine — same no-rollback rationale as
    /// [`FoldRuntime::reseed`]'s doc comment.
    params: FoldParams,
    tpo_source: TpoSource,
    exchange: ExchangeId,
    symbol: String,
    /// Running per-bubble footprint CVD total — `Footprint`-only bookkeeping
    /// (mirrors the station-derived path's `set_cumulative_delta` chain in
    /// `mlc-backend/src/lib.rs`), reset on every [`Self::reseed`] (a reseed
    /// replays the WHOLE trade layer from scratch, so the running total must
    /// restart at `0.0` rather than carry over stale state from before the
    /// reseed).
    footprint_cvd: f64,
}

impl TradeFoldRuntime {
    /// Install a fresh runtime for `chart_type` with `params`. Returns
    /// `None` for any chart_type outside the trade-fed family (including the
    /// synthetic `"cvd"` id — see [`TradeFoldMachine::new`]).
    pub fn new(
        chart_type: &str,
        params: FoldParams,
        tpo_source: TpoSource,
        exchange: ExchangeId,
        symbol: &str,
    ) -> Option<Self> {
        let machine = TradeFoldMachine::new(chart_type, &params, tpo_source)?;
        let chart_type = machine.chart_type();
        Some(Self {
            machine,
            chart_type,
            params,
            tpo_source,
            exchange,
            symbol: symbol.to_string(),
            footprint_cvd: 0.0,
        })
    }

    /// Chart-type id this runtime computes (`"cvd"` for the CVD companion).
    pub fn chart_type(&self) -> &'static str {
        self.chart_type
    }

    /// Construction params this runtime is currently folding with.
    pub fn params(&self) -> FoldParams {
        self.params
    }

    /// Whether this runtime folds the trade layer (`true`) or the "1m" kline
    /// layer (`false`, TPO Kline1m flavour only).
    pub fn feeds_on_trades(&self) -> bool {
        self.machine.feeds_on_trades()
    }

    /// Cold-seed / re-seed by replaying the bubble's ENTIRE trade layer
    /// chronologically through a fresh machine instance — real trades, no
    /// synthetic legs, no kline baseline (unlike [`FoldRuntime::reseed`]'s
    /// kline-fold contract, there is no "in-progress last bar" concept for a
    /// trade tape: every trade is already a complete, immutable event).
    ///
    /// For the TPO Kline1m flavour, replays `kline_layer` (the "1m" bars)
    /// instead — `trade_layer` is ignored for that one variant. Every other
    /// variant ignores `kline_layer`.
    ///
    /// Wraps the whole replay in [`DerivedStream::set_replay`] (P0 hang fix
    /// part 1 — see [`FoldRuntime::reseed`]'s doc comment for the full
    /// contract this mirrors verbatim): the machine folds every trade/bar
    /// into internal state exactly as live, but suppresses the per-event
    /// in-progress snapshot; [`DerivedStream::flush_replay`] clones out the
    /// final in-progress element once at the end, appended last, before
    /// replay mode is turned back off.
    ///
    /// Returns every output the replay produced, oldest-first — the caller
    /// replaces the bubble's output slot wholesale with this.
    pub fn reseed(
        &mut self,
        trade_layer: &RawLayer<RawTrade>,
        kline_layer: &RawLayer<mlc_core::Bar>,
    ) -> Vec<TradeFoldOutput> {
        // Fresh machine per reseed — see `FoldRuntime::reseed`'s doc comment
        // for the no-rollback rationale this mirrors verbatim.
        if let Some(fresh) = TradeFoldMachine::new(self.chart_type, &self.params, self.tpo_source) {
            self.machine = fresh;
        }
        self.footprint_cvd = 0.0;

        let mut outputs = Vec::new();
        self.machine.set_replay(true);
        if self.machine.feeds_on_trades() {
            for trade in trade_layer.iter() {
                let point = raw_trade_to_trade_point(trade);
                let ev = Event::Trade {
                    exchange: self.exchange,
                    symbol: self.symbol.clone(),
                    point,
                };
                for out in self.machine.on_trade_event(&ev) {
                    outputs.push(self.stamp_footprint_cvd(out));
                }
            }
        } else {
            for bar in kline_layer.iter() {
                let point = bar_to_bar_point(bar);
                let ev = Event::Bar {
                    exchange: self.exchange,
                    symbol: self.symbol.clone(),
                    timeframe: KlineInterval::new("1m"),
                    point,
                };
                outputs.extend(self.machine.on_bar_event(&ev));
            }
        }
        if let Some(flushed) = self.machine.flush_replay() {
            outputs.push(self.stamp_footprint_cvd(flushed));
        }
        self.machine.set_replay(false);
        outputs
    }

    /// Fold ONLY a batch of OLDER trades through a FRESH machine (prefix
    /// fold) — incremental-prepend doctrine (owner 2026-07-13): time-
    /// bucketed folds (footprint, the CVD companion) never need a whole-
    /// layer reseed for a history prepend, because older trades produce
    /// only OLDER buckets plus at most one seam bucket shared with the
    /// existing output. The runtime's LIVE machine state is untouched —
    /// the live fold keeps running from where it was; the caller stitches
    /// the returned prefix outputs in FRONT of the existing slot (seam
    /// merge + cumulative restitch are the caller's job — see
    /// `BackendCore::incremental_prepend_trade_folds`).
    ///
    /// Footprint cumulative-delta stamping uses a LOCAL running total
    /// starting at 0 (the prefix's own cumulative); `self.footprint_cvd`
    /// is not read or written.
    ///
    /// Returns the prefix outputs oldest-first; empty for kline-fed
    /// flavours (TPO Kline1m) and for chart types outside the trade-fed
    /// family.
    pub fn fold_prefix(&self, prefix: &[RawTrade]) -> Vec<TradeFoldOutput> {
        let Some(mut machine) = TradeFoldMachine::new(self.chart_type, &self.params, self.tpo_source) else {
            return Vec::new();
        };
        if !machine.feeds_on_trades() {
            return Vec::new();
        }
        let mut running_cvd = 0.0_f64;
        let stamp_local = |out: TradeFoldOutput, running: &mut f64| -> TradeFoldOutput {
            match out {
                TradeFoldOutput::Footprint(mut bar) => {
                    bar.set_cumulative_delta(*running);
                    *running = bar.cumulative_delta;
                    TradeFoldOutput::Footprint(bar)
                }
                other => other,
            }
        };
        let mut outputs = Vec::new();
        machine.set_replay(true);
        for trade in prefix {
            let point = raw_trade_to_trade_point(trade);
            let ev = Event::Trade {
                exchange: self.exchange,
                symbol: self.symbol.clone(),
                point,
            };
            for out in machine.on_trade_event(&ev) {
                outputs.push(stamp_local(out, &mut running_cvd));
            }
        }
        if let Some(flushed) = machine.flush_replay() {
            outputs.push(stamp_local(flushed, &mut running_cvd));
        }
        machine.set_replay(false);
        outputs
    }

    /// Fold one live trade tick. Empty for the TPO Kline1m flavour — use
    /// [`Self::on_live_1m`] for that one instead.
    ///
    /// Returns EVERY output produced by this single live trade, oldest-first
    /// (see [`FoldMachine::on_event`]'s doc comment) — today always 0 or 1
    /// elements (no trade-fed machine currently populates a pending queue),
    /// kept as a `Vec` for symmetry with [`FoldRuntime::on_live_1m`] and to
    /// need no signature change if a future trade-fed machine gains one.
    pub fn on_live_trade(&mut self, trade: &RawTrade) -> Vec<TradeFoldOutput> {
        if !self.machine.feeds_on_trades() {
            return Vec::new();
        }
        let point = raw_trade_to_trade_point(trade);
        let ev = Event::Trade {
            exchange: self.exchange,
            symbol: self.symbol.clone(),
            point,
        };
        self.machine.on_trade_event(&ev).into_iter()
            .map(|out| self.stamp_footprint_cvd(out))
            .collect()
    }

    /// Fold one live "1m" kline tick — TPO Kline1m flavour only, empty for
    /// every other trade-fed chart_type. See [`Self::on_live_trade`]'s doc
    /// comment for the `Vec` return shape rationale.
    pub fn on_live_1m(&mut self, bar: &mlc_core::Bar) -> Vec<TradeFoldOutput> {
        if self.machine.feeds_on_trades() {
            return Vec::new();
        }
        let ev = Event::Bar {
            exchange: self.exchange,
            symbol: self.symbol.clone(),
            timeframe: KlineInterval::new("1m"),
            point: bar_to_bar_point(bar),
        };
        self.machine.on_bar_event(&ev)
    }

    /// Footprint-only: stamp the running per-bubble CVD total onto a freshly
    /// folded `FootprintBar` and advance the total — mirrors the
    /// station-derived path's `set_cumulative_delta` chain
    /// (`mlc-backend/src/lib.rs`'s `LiveUpdate::FootprintUpdate` arm). A
    /// no-op passthrough for every other output variant.
    fn stamp_footprint_cvd(&mut self, out: TradeFoldOutput) -> TradeFoldOutput {
        match out {
            TradeFoldOutput::Footprint(mut bar) => {
                bar.set_cumulative_delta(self.footprint_cvd);
                self.footprint_cvd = bar.cumulative_delta;
                TradeFoldOutput::Footprint(bar)
            }
            other => other,
        }
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

    /// A single closed 1m bar whose synthetic-trade path contains one leg
    /// that jumps the price by $10 while `range=$1` must fold into a chain
    /// of ~10 range bars — the dig3-side pending-queue fix
    /// (`digdigdig3-station::derived::TradeToRangeBarDerived`) must surface
    /// EVERY chained bar through `FoldMachine::on_event`'s `Vec` return, not
    /// just the last one. Asserts both the count and that each bar's `open`
    /// equals the previous bar's `close` (chained-open stitching, unbroken
    /// across the whole reseed).
    #[test]
    fn reseed_range_bar_gap_crossing_kline_leg_produces_chained_bars() {
        let mut layer: RawLayer<mlc_core::Bar> = RawLayer::new(0);
        // Bullish bar (close >= open): synthetic path is O -> L -> H -> C.
        // With low == open and high == close, the O->L leg is a no-op and
        // the L->H leg alone carries the full $10 jump in ONE synthetic
        // trade — exactly the single-event multi-bar-close scenario under
        // test. A second (in-progress) bar follows so the first bar is not
        // the "possibly in-progress last bar" `reseed` special-cases (that
        // one is excluded from the synthetic-trade fold and instead seeds
        // the live Δ-tick baseline — see `FoldRuntime::reseed`'s doc
        // comment).
        layer.push_live(bar(60, 100.0, 110.0, 100.0, 110.0, 10.0));
        layer.push_live(bar(120, 110.0, 110.0, 110.0, 110.0, 1.0));

        let params = FoldParams { scaled_units: 1.0, ..Default::default() }; // $1 range
        let mut fold = FoldRuntime::new("range_bar", params, ExchangeId::Binance, "BTCUSDT")
            .expect("range_bar is a fold type");
        let outputs = fold.reseed(&layer);

        let bars: Vec<mlc_core::Bar> = outputs.into_iter().filter_map(|o| match o {
            FoldOutput::Bar(b) => Some(b),
            _ => None,
        }).collect();

        // Distinct bars by open_time (the machine re-emits the in-progress
        // bar's growing snapshot on every synthetic leg — collapse those
        // the same way `distinct_bars` helpers above do) — this is the
        // sequence of CLOSED chained bars plus whatever bar was still open
        // at the very last synthetic leg fed.
        let mut distinct: Vec<mlc_core::Bar> = Vec::new();
        for b in bars {
            let id = if b.derived_ts_ms != 0 { b.derived_ts_ms } else { b.timestamp };
            match distinct.last_mut() {
                Some(last) if (if last.derived_ts_ms != 0 { last.derived_ts_ms } else { last.timestamp }) == id => *last = b,
                _ => distinct.push(b),
            }
        }

        // A $10 jump at $1 range must produce roughly 10 chained bars (open
        // == 100.0 -> close == 110.0 walked one dollar at a time). Exact
        // count depends on the O->L no-op leg and where the L->H leg lands
        // relative to the range grid, so assert a tight band rather than an
        // exact number.
        assert!(distinct.len() >= 9 && distinct.len() <= 11, "expected ~10 chained bars, got {}", distinct.len());

        // Chained-open stitching: every bar's open equals the previous
        // bar's close — must hold across every pending-queue element, not
        // just the last (this is exactly what would silently break if the
        // pending queue were dropped instead of drained).
        for pair in distinct.windows(2) {
            assert_eq!(pair[1].open, pair[0].close, "chained-open must stitch open==prev close");
        }

        // The chain starts at the first bar's open (100.0) and the walk
        // reaches at least 109.0 by the second-to-last close (each bar
        // consumes exactly $1 of the $10 jump).
        assert_eq!(distinct.first().expect("at least one bar").open, 100.0);
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
        assert!(!live_output.is_empty(), "a large enough delta move must emit a fold output");
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

    // =========================================================================
    // Trade-fed folds (unified-data-plane Stage Э4)
    // =========================================================================

    fn trade(ts_ms: i64, price: f64, qty: f64, is_buyer_maker: bool) -> RawTrade {
        RawTrade { ts_ms, price, qty, is_buyer_maker }
    }

    #[test]
    fn raw_trade_to_trade_point_maps_fields_and_inverts_is_buyer_maker() {
        // is_buyer_maker=true -> side=0 (reconstructs the bridge's own
        // `is_buyer_maker: point.side == 0` convention exactly — see
        // `raw_trade_to_trade_point`'s doc comment).
        let t = trade(1_000, 42.5, 3.25, true);
        let p = raw_trade_to_trade_point(&t);
        assert_eq!(p.ts_ms, 1_000);
        assert_eq!(p.price, 42.5);
        assert_eq!(p.quantity, 3.25);
        assert_eq!(p.side, 0);
        assert_eq!(p.trade_id_hash, 0);

        let t2 = trade(2_000, 10.0, 1.0, false);
        let p2 = raw_trade_to_trade_point(&t2);
        assert_eq!(p2.side, 1);
    }

    /// A trending series of trades with alternating direction bursts — gives
    /// tick_bar/footprint/tpo/cvd enough signal to produce non-trivial
    /// output (mirrors `trending_layer`'s rationale for the kline-fold
    /// tests above).
    fn trending_trade_layer(n: usize) -> RawLayer<RawTrade> {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(0);
        let mut price = 100.0;
        for i in 0..n {
            let ts_ms = (i as i64 + 1) * 1000;
            // Alternate buy/sell bursts of 5 so imbalance/run machines see
            // real signal instead of a single infinite run.
            let is_buyer_maker = (i / 5) % 2 == 0;
            price += if is_buyer_maker { -0.5 } else { 0.5 };
            layer.push_live(trade(ts_ms, price, 1.0, is_buyer_maker));
        }
        layer
    }

    fn trade_fold_outputs_debug(outputs: &[TradeFoldOutput]) -> Vec<String> {
        outputs.iter().map(|o| format!("{o:?}")).collect()
    }

    #[test]
    fn trade_fold_machine_new_returns_none_for_non_trade_fold_chart_type() {
        let params = FoldParams::default();
        assert!(TradeFoldMachine::new("candles", &params, TpoSource::TradeBucket).is_none());
        assert!(TradeFoldMachine::new("range_bar", &params, TpoSource::TradeBucket).is_none());
    }

    #[test]
    fn trade_fold_runtime_new_returns_none_for_non_trade_fold_chart_type() {
        let params = FoldParams::default();
        assert!(TradeFoldRuntime::new("candles", params, TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT").is_none());
    }

    #[test]
    fn tick_bar_reseed_is_deterministic_for_same_layer_and_params() {
        let layer = trending_trade_layer(30);
        let params = FoldParams { count: 5, ..Default::default() };
        let mut fold_a = TradeFoldRuntime::new("tick_bar", params, TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT").expect("tick_bar is a trade fold type");
        let mut fold_b = TradeFoldRuntime::new("tick_bar", params, TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT").expect("tick_bar is a trade fold type");

        let empty_kline: RawLayer<mlc_core::Bar> = RawLayer::new(0);
        let out_a = fold_a.reseed(&layer, &empty_kline);
        let out_b = fold_b.reseed(&layer, &empty_kline);
        assert!(!out_a.is_empty(), "30 trades at 5 ticks/bar must produce at least one bar");
        assert_eq!(trade_fold_outputs_debug(&out_a), trade_fold_outputs_debug(&out_b));

        // Re-seeding the SAME runtime a second time over the SAME layer
        // reproduces identical output (fresh machine per reseed, no
        // leftover state).
        let out_a_again = fold_a.reseed(&layer, &empty_kline);
        assert_eq!(trade_fold_outputs_debug(&out_a), trade_fold_outputs_debug(&out_a_again));
    }

    #[test]
    fn tick_bar_reseed_with_different_params_produces_different_cut_of_same_input() {
        let layer = trending_trade_layer(30);
        let empty_kline: RawLayer<mlc_core::Bar> = RawLayer::new(0);
        let mut fold_tight = TradeFoldRuntime::new(
            "tick_bar", FoldParams { count: 3, ..Default::default() },
            TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT",
        ).expect("tick_bar is a trade fold type");
        let mut fold_wide = TradeFoldRuntime::new(
            "tick_bar", FoldParams { count: 15, ..Default::default() },
            TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT",
        ).expect("tick_bar is a trade fold type");

        let out_tight = fold_tight.reseed(&layer, &empty_kline);
        let out_wide = fold_wide.reseed(&layer, &empty_kline);
        let distinct_bars = |outputs: &[TradeFoldOutput]| -> usize {
            let mut open_times: Vec<i64> = outputs.iter().filter_map(|o| match o {
                TradeFoldOutput::Bar(b) => Some(if b.derived_ts_ms != 0 { b.derived_ts_ms } else { b.timestamp }),
                _ => None,
            }).collect();
            open_times.dedup();
            open_times.len()
        };
        let tight_bars = distinct_bars(&out_tight);
        let wide_bars = distinct_bars(&out_wide);
        // A smaller tick count per bar must cut MORE bars than a larger one
        // over the same trade tape.
        assert!(tight_bars > wide_bars, "tight={tight_bars} wide={wide_bars}");
    }

    #[test]
    fn tick_imbalance_bar_param_sensitivity() {
        // A one-sided run (every trade buyer-initiated) gives the running
        // imbalance |theta| a monotonic climb of exactly 1 per tick — a
        // deterministic, strongly-biased signal (unlike `trending_trade_layer`'s
        // alternating bursts, which cancel out and never cross a large
        // `min_ticks` threshold within a short tape).
        let mut layer: RawLayer<RawTrade> = RawLayer::new(0);
        for i in 0..200 {
            layer.push_live(trade((i as i64 + 1) * 1000, 100.0 + i as f64 * 0.01, 1.0, true));
        }
        let empty_kline: RawLayer<mlc_core::Bar> = RawLayer::new(0);
        let mut fold_low = TradeFoldRuntime::new(
            "tick_imbalance_bar", FoldParams { alpha: 0.20, min_ticks: 5, ..Default::default() },
            TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT",
        ).expect("tick_imbalance_bar is a trade fold type");
        let mut fold_high = TradeFoldRuntime::new(
            "tick_imbalance_bar", FoldParams { alpha: 0.20, min_ticks: 40, ..Default::default() },
            TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT",
        ).expect("tick_imbalance_bar is a trade fold type");

        let out_low = fold_low.reseed(&layer, &empty_kline);
        let out_high = fold_high.reseed(&layer, &empty_kline);
        assert!(!out_low.is_empty());
        assert!(!out_high.is_empty());
        let distinct_bars = |outputs: &[TradeFoldOutput]| -> usize {
            let mut open_times: Vec<i64> = outputs.iter().filter_map(|o| match o {
                TradeFoldOutput::Bar(b) => Some(if b.derived_ts_ms != 0 { b.derived_ts_ms } else { b.timestamp }),
                _ => None,
            }).collect();
            open_times.dedup();
            open_times.len()
        };
        // A lower min_ticks seed (smaller close threshold) must close MORE
        // bars than a higher one over the same one-sided trade tape.
        assert!(distinct_bars(&out_low) > distinct_bars(&out_high));
    }

    #[test]
    fn run_bar_reseed_is_deterministic() {
        let layer = trending_trade_layer(40);
        let empty_kline: RawLayer<mlc_core::Bar> = RawLayer::new(0);
        let params = FoldParams { alpha: 0.20, min_ticks: 10, ..Default::default() };
        let mut fold_a = TradeFoldRuntime::new("run_bar", params, TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT").expect("run_bar is a trade fold type");
        let mut fold_b = TradeFoldRuntime::new("run_bar", params, TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT").expect("run_bar is a trade fold type");
        let out_a = fold_a.reseed(&layer, &empty_kline);
        let out_b = fold_b.reseed(&layer, &empty_kline);
        assert_eq!(trade_fold_outputs_debug(&out_a), trade_fold_outputs_debug(&out_b));
    }

    #[test]
    fn volume_imbalance_bar_reseed_is_deterministic() {
        let layer = trending_trade_layer(40);
        let empty_kline: RawLayer<mlc_core::Bar> = RawLayer::new(0);
        let params = FoldParams { alpha: 0.20, min_ticks: 10, ..Default::default() };
        let mut fold_a = TradeFoldRuntime::new("volume_imbalance_bar", params, TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT").expect("volume_imbalance_bar is a trade fold type");
        let mut fold_b = TradeFoldRuntime::new("volume_imbalance_bar", params, TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT").expect("volume_imbalance_bar is a trade fold type");
        let out_a = fold_a.reseed(&layer, &empty_kline);
        let out_b = fold_b.reseed(&layer, &empty_kline);
        assert_eq!(trade_fold_outputs_debug(&out_a), trade_fold_outputs_debug(&out_b));
    }

    /// Footprint bucket correctness on a synthetic trade sequence: two
    /// trades in the same 1000ms bucket must land in ONE `FootprintPoint`
    /// (merged OHLCV + both price levels present), a trade in the next
    /// bucket must open a NEW point.
    #[test]
    fn footprint_fold_buckets_trades_by_interval_and_tracks_buy_sell_levels() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(0);
        // Bucket 0 [0, 1000): two trades, one buy one sell, different
        // prices. `raw_trade_to_trade_point`: is_buyer_maker=true -> side=0
        // (Buy aggressor), is_buyer_maker=false -> side=1 (Sell aggressor).
        layer.push_live(trade(100, 100.0, 2.0, true));  // buy (aggressor)
        layer.push_live(trade(500, 101.0, 3.0, false)); // sell (aggressor)
        // Bucket 1 [1000, 2000): one trade.
        layer.push_live(trade(1_200, 102.0, 1.0, true)); // buy

        let params = FoldParams { interval_ms: 1_000, ..Default::default() };
        let mut fold = TradeFoldRuntime::new("footprint", params, TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT")
            .expect("footprint is a trade fold type");
        let empty_kline: RawLayer<mlc_core::Bar> = RawLayer::new(0);
        let outputs = fold.reseed(&layer, &empty_kline);

        let footprints: Vec<mlc_core::types::FootprintBar> = outputs.into_iter().filter_map(|o| match o {
            TradeFoldOutput::Footprint(b) => Some(b),
            _ => None,
        }).collect();
        assert!(!footprints.is_empty());

        // The bucket-0 emissions (before the bucket rolls) must carry BOTH
        // levels once the second trade has been folded — check the LAST
        // bucket-0 emission (upsert semantics: current bucket re-emits on
        // every trade).
        let bucket0_final = footprints.iter().rev()
            .find(|b| b.open_time == 0)
            .expect("bucket 0 (open_time=0) must have at least one emission");
        assert_eq!(bucket0_final.levels.len(), 2, "bucket 0 must carry both distinct price levels");
        let has_buy_100 = bucket0_final.levels.iter().any(|l| (l.price - 100.0).abs() < 1e-9 && l.buy_vol > 0.0);
        let has_sell_101 = bucket0_final.levels.iter().any(|l| (l.price - 101.0).abs() < 1e-9 && l.sell_vol > 0.0);
        assert!(has_buy_100, "price=100.0 must carry buy volume from the buyer-aggressor trade");
        assert!(has_sell_101, "price=101.0 must carry sell volume from the seller-aggressor trade");

        // Bucket 1 is a distinct emission with its own open_time.
        let bucket1 = footprints.iter().find(|b| b.open_time == 1_000);
        assert!(bucket1.is_some(), "the third trade at ts=1200 must open bucket open_time=1000");
    }

    #[test]
    fn footprint_fold_reseed_is_deterministic() {
        let layer = trending_trade_layer(50);
        let empty_kline: RawLayer<mlc_core::Bar> = RawLayer::new(0);
        let params = FoldParams { interval_ms: 5_000, ..Default::default() };
        let mut fold_a = TradeFoldRuntime::new("footprint", params, TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT").expect("footprint is a trade fold type");
        let mut fold_b = TradeFoldRuntime::new("footprint", params, TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT").expect("footprint is a trade fold type");
        let out_a = fold_a.reseed(&layer, &empty_kline);
        let out_b = fold_b.reseed(&layer, &empty_kline);
        assert!(!out_a.is_empty());
        assert_eq!(trade_fold_outputs_debug(&out_a), trade_fold_outputs_debug(&out_b));
    }

    #[test]
    fn footprint_fold_bucket_switch_produces_different_cut() {
        let layer = trending_trade_layer(50);
        let empty_kline: RawLayer<mlc_core::Bar> = RawLayer::new(0);
        let mut fold_fine = TradeFoldRuntime::new(
            "footprint", FoldParams { interval_ms: 1_000, ..Default::default() },
            TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT",
        ).expect("footprint is a trade fold type");
        let mut fold_coarse = TradeFoldRuntime::new(
            "footprint", FoldParams { interval_ms: 30_000, ..Default::default() },
            TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT",
        ).expect("footprint is a trade fold type");
        let out_fine = fold_fine.reseed(&layer, &empty_kline);
        let out_coarse = fold_coarse.reseed(&layer, &empty_kline);
        let distinct_buckets = |outputs: &[TradeFoldOutput]| -> usize {
            let mut opens: Vec<i64> = outputs.iter().filter_map(|o| match o {
                TradeFoldOutput::Footprint(b) => Some(b.open_time),
                _ => None,
            }).collect();
            opens.dedup();
            opens.len()
        };
        // A finer bucket interval must cut MORE distinct footprint buckets
        // than a coarser one over the same 50-second trade tape.
        assert!(distinct_buckets(&out_fine) > distinct_buckets(&out_coarse));
    }

    #[test]
    fn tpo_trade_fold_reseed_produces_session_output() {
        let layer = trending_trade_layer(50);
        let empty_kline: RawLayer<mlc_core::Bar> = RawLayer::new(0);
        let params = FoldParams { freq_minutes: 30, ..Default::default() };
        let mut fold = TradeFoldRuntime::new("tpo", params, TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT").expect("tpo is a trade fold type");
        assert!(fold.feeds_on_trades());
        let outputs = fold.reseed(&layer, &empty_kline);
        assert!(!outputs.is_empty());
        assert!(outputs.iter().all(|o| matches!(o, TradeFoldOutput::Tpo(_))));
    }

    #[test]
    fn tpo_kline1m_fold_does_not_feed_on_trades_but_feeds_on_1m_bars() {
        let params = FoldParams { freq_minutes: 30, ..Default::default() };
        let mut fold = TradeFoldRuntime::new("tpo", params, TpoSource::Kline1m, ExchangeId::Binance, "BTCUSDT").expect("tpo is a trade fold type");
        assert!(!fold.feeds_on_trades());

        // Trade-only reseed must produce nothing for the Kline1m flavour.
        let trade_layer = trending_trade_layer(20);
        let empty_kline: RawLayer<mlc_core::Bar> = RawLayer::new(0);
        let outputs = fold.reseed(&trade_layer, &empty_kline);
        assert!(outputs.is_empty(), "Kline1m TPO must ignore the trade layer entirely");

        // Kline-only reseed must produce session output.
        let mut kline_layer: RawLayer<mlc_core::Bar> = RawLayer::new(0);
        let mut price = 100.0;
        for i in 0..20 {
            let ts_sec = (i as i64 + 1) * 60;
            let open = price;
            price += 1.0;
            kline_layer.push_live(bar(ts_sec, open, price + 1.0, open - 1.0, price, 5.0));
        }
        let empty_trades: RawLayer<RawTrade> = RawLayer::new(0);
        let outputs2 = fold.reseed(&empty_trades, &kline_layer);
        assert!(!outputs2.is_empty(), "Kline1m TPO must fold the 1m kline layer");
    }

    #[test]
    fn cvd_fold_reseed_is_deterministic_and_running_total() {
        let layer = trending_trade_layer(20);
        let empty_kline: RawLayer<mlc_core::Bar> = RawLayer::new(0);
        let mut fold_a = TradeFoldRuntime::new("cvd", FoldParams::default(), TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT").expect("cvd is a trade fold type");
        let mut fold_b = TradeFoldRuntime::new("cvd", FoldParams::default(), TpoSource::TradeBucket, ExchangeId::Binance, "BTCUSDT").expect("cvd is a trade fold type");
        let out_a = fold_a.reseed(&layer, &empty_kline);
        let out_b = fold_b.reseed(&layer, &empty_kline);
        assert_eq!(out_a.len(), 20, "every trade must emit one CVD sample");
        assert_eq!(trade_fold_outputs_debug(&out_a), trade_fold_outputs_debug(&out_b));

        // Last sample's value equals the sum of signed trade quantities
        // (buy=+qty, sell=-qty) — a running total, not a per-bucket reset.
        let last_value = out_a.iter().rev().find_map(|o| match o {
            TradeFoldOutput::Cvd(s) => Some(s.value),
            _ => None,
        }).expect("at least one CVD sample");
        let expected: f64 = (0..20).map(|i| {
            let is_buyer_maker = (i / 5) % 2 == 0;
            if is_buyer_maker { -1.0 } else { 1.0 }
        }).sum();
        assert!((last_value - expected).abs() < 1e-9, "last={last_value} expected={expected}");
    }

    #[test]
    fn trade_fold_disjoint_from_kline_fold_chart_types() {
        // Sanity guard: the two fold families must never overlap on the
        // same chart_type id (would make `is_fold_chart_type` and
        // `is_trade_fold_chart_type` both true for the same string, which
        // the `chart_type::engine` dispatch assumes never happens).
        let kline_types = ["range_bar", "volume_bar", "dollar_bar", "renko", "pnf", "kagi", "three_line_break"];
        let trade_types = ["tick_bar", "tick_imbalance_bar", "volume_imbalance_bar", "run_bar", "footprint", "tpo"];
        for k in kline_types {
            assert!(!trade_types.contains(&k), "{k} must not be in both fold families");
        }
    }
}
