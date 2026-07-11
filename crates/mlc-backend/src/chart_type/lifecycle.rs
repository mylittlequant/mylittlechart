//! Client-side fold lifecycle — `BackendCore` methods for the kline-fold,
//! trade-fold, and CVD-companion-fold families (unified-data-plane Stages
//! Э3/Э4/Э5, 2026-07-09).
//!
//! Moved verbatim out of `lib.rs` (chart-type-engine-consolidation, PURE
//! MOVE, 2026-07-10) — these `impl BackendCore` blocks own the full
//! subscribe → reseed → apply-outputs → live-tick-fold lifecycle for every
//! chart type driven by a client-side fold machine instead of a
//! station-derived stream. `lib.rs`'s `tick_writers` dispatch (the
//! `LiveUpdate::BarUpdate` / `LiveUpdate::TradeUpdate` / `LiveUpdate::
//! LayerBackfilled` arms) calls these methods via `self.` — same crate,
//! same `impl BackendCore`, file location doesn't affect method resolution.

use digdigdig3::{AccountType, ExchangeId};
use mlc_core::ids::BubbleId;
use mlc_core::state::Timeframe;
use mlc_core::Bar;

use crate::{BackendCore, BubbleEvent};

/// TPO session ring cap — the max number of sessions (closed + 1 current)
/// kept on `bubble.tpo_sessions`. Mirrors dig3's own `TpoSessionState::
/// max_history_sessions` (`digdigdig3-station/src/derived.rs`) — the two
/// must move together since dig3 already discards anything past its own
/// cap before this client-side reseed/live path ever sees it (bumping only
/// the client-side constant without bumping dig3's would just add dead
/// headroom that dig3's ring never fills).
const MAX_TPO_SESSIONS: usize = 30;

/// Kline depth target (in 1m bars) the TPO `Kline1m` flavour backfills
/// toward — `~30 days x 1440 bars/day = 43_200`. TPO needs deep history to
/// populate its session ring (up to [`MAX_TPO_SESSIONS`] = 30 daily
/// sessions); the generic kline consumers on the same "1m" layer (e.g. a
/// plain candlestick chart sharing the layer) typically need far less, so
/// without a TPO-specific target the shared `ensure_kline_backfill` ramp
/// would happily stop at whatever smaller depth an earlier consumer already
/// satisfied (`RampState::require_target` only widens, it does not shrink —
/// see that method's doc comment in `backfill_engine.rs` — but it also
/// never widens on its own without a caller asking). Passing this constant
/// through `ensure_kline_backfill`'s `target_bars` makes the ramp
/// unconditionally deepen to cover every session TPO wants to render, even
/// when TPO is a late joiner on an already-satisfied "1m" layer.
pub(crate) const TPO_KLINE_DEPTH_BARS: u32 = 43_200;

impl BackendCore {
    // =========================================================================
    // === Client-side view fold (unified-data-plane Stage Э3, 2026-07-09) ====
    // =========================================================================

    /// Install (or replace) `bid`'s [`bubble::FoldRuntime`] for one of the 7
    /// kline-sufficient chart types, ensure the bubble's shared "1m" kline
    /// layer is network-fed, and reseed synchronously from whatever the
    /// layer already holds (instant if warm, empty if this is the very
    /// first touch and the async backfill hasn't landed a page yet).
    ///
    /// Entirely synchronous except the "ensure network feed" step (fire-and-
    /// forget `request_bars`/`ensure_kline_backfill`, same async machinery
    /// every other chart type already uses for its OWN kline sub — no new
    /// network primitive). The reseed itself touches zero network: it folds
    /// whatever `kline_layers["1m"]` already contains in memory.
    ///
    /// Returns the [`BubbleEvent`] the caller should feed through
    /// `ChartApp::react_to_bubble_event` to run the same viewport reconcile
    /// (`seed_fresh_load`/`reconcile_derived_series`) a station-driven
    /// `tick_writers` event would have triggered — `None` when `bid` doesn't
    /// resolve to a live bubble.
    pub fn subscribe_fold_chart_type(
        &self,
        bid: BubbleId,
        chart_type: &'static str,
        params: crate::bubble::FoldParams,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        derived_kline_target: Option<u32>,
    ) -> Option<BubbleEvent> {
        let one_minute = Timeframe::new("1m", 60_000);
        // Live feed: idempotent per (eid, at, symbol, "1m") — a cheap no-op
        // if another window already opened this exact kline sub. Kept ahead
        // of the reseed/install below (unlike `ensure_kline_backfill`, this
        // has no refold-race — the live-tick writer path independently
        // checks `active_fold.is_some()` per event, it never sweeps a
        // point-in-time bubble list the way `LayerBackfilled` does).
        self.bridge.request_bars(exchange_id, symbol, &one_minute, account_type, None, None, false);

        // Fix 2 (chart-type-engine-consolidation B1): install BEFORE
        // ensure — build + reseed the fold runtime over whatever the "1m"
        // layer holds RIGHT NOW, install it into the bubble, and only THEN
        // fire the async ramped backfill. This closes the install-order
        // race: `LiveUpdate::LayerBackfilled`'s refold sweep
        // (`BackendCore::tick_writers`) only re-folds bubbles that already
        // have `active_fold.is_some()` — if the backfill's first page had
        // landed and broadcast BEFORE `active_fold` was set, that sweep
        // would silently skip this bubble and the fresh page would never
        // reach it. By the time `ensure_kline_backfill` returns (it merely
        // enqueues a background task — the fetch itself is async), the
        // install below has already completed synchronously, so no
        // `LayerBackfilled` for this key can possibly land in between.
        let fold = crate::bubble::FoldRuntime::new(chart_type, params, exchange_id, symbol)?;
        let event = self.reseed_fold(bid, fold, exchange_id, symbol, account_type);

        // History: ramped backfill toward `derived_kline_target` (profile-
        // configurable, default handled by the caller) — progressive, every
        // page lands via the existing `ScrollBarsLoaded`/`BackfillComplete`
        // + `LayerBackfilled` writers already wired in `tick_writers`.
        self.bridge.ensure_kline_backfill(exchange_id, symbol, &one_minute, account_type, derived_kline_target);

        event
    }

    /// Re-seed `bid`'s existing (or freshly-installed) fold runtime from the
    /// bubble's current "1m" kline layer snapshot, replace the output slot
    /// wholesale, and return the reconcile-triggering [`BubbleEvent`].
    fn reseed_fold(
        &self,
        bid: BubbleId,
        mut fold: crate::bubble::FoldRuntime,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) -> Option<BubbleEvent> {
        let layer_snapshot: crate::bubble::RawLayer<Bar> = self.bubbles()
            .kline_layer(bid, "1m")
            .cloned()
            .unwrap_or_else(|| crate::bubble::RawLayer::new(0));
        let outputs = fold.reseed(&layer_snapshot);
        let chart_type = fold.chart_type();
        self.bubbles_mut().with_bubble_mut(bid, |b| b.active_fold = Some(fold));
        let account_type_label = account_type.short_label().to_string();
        self.apply_fold_reseed_outputs(bid, chart_type, outputs, exchange_id, symbol, &account_type_label)
    }

    /// Replace `bid`'s fold output slot wholesale with `outputs` (a full
    /// reseed's worth of points, oldest-first) and build the matching
    /// reconcile-triggering [`BubbleEvent`]. Shared by
    /// [`Self::reseed_fold`] (install/param-change/chart-type-switch path)
    /// and the `LiveUpdate::LayerBackfilled` arm in [`Self::tick_writers`]
    /// (scroll-deepen refold path) — both are "the layer got deeper, redo
    /// the full fold" operations that differ only in WHEN they fire, not in
    /// how the result lands.
    pub(crate) fn apply_fold_reseed_outputs(
        &self,
        bid: BubbleId,
        chart_type: &'static str,
        outputs: Vec<crate::bubble::FoldOutput>,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type_label: &str,
    ) -> Option<BubbleEvent> {
        let symbol = symbol.to_string();
        let account_type_label = account_type_label.to_string();
        // Identity-key mapping is registry-driven (chart-type-engine-
        // consolidation B3, S6) — `def.identity` selects the merge key fn
        // via the shape's own `*_identity_key_fn` resolver instead of an
        // arm hardcoding "renko → renko_identity" as tribal per-arm
        // knowledge. The output-SHAPE match below (which `FoldOutput`
        // variant this chart_type's machine actually produces) necessarily
        // stays — that's data shape, not policy.
        let identity = mlc_core::chart_type::by_id(chart_type).map(|d| d.identity).unwrap_or(mlc_core::chart_type::IdentityKey::None);
        let event = match chart_type {
            "range_bar" | "volume_bar" | "dollar_bar" => {
                // The fold machine emits a snapshot on every consumed 1m
                // kline (in-progress bar growing), not just on bar-close. A
                // reseed replay must collapse same-identity snapshots to the
                // last one or the seed array keeps every intermediate as a
                // separate bar. Identity mirrors `fold_on_live_1m`'s
                // `last_ts_identity` rule exactly.
                let key_fn = mlc_core::chart_type::bar_identity_key_fn(identity).unwrap_or(mlc_core::chart_type::bar_identity);
                let mut bars: Vec<Bar> = Vec::new();
                for o in outputs {
                    let crate::bubble::FoldOutput::Bar(b) = o else { continue };
                    mlc_core::chart_type::merge_last_wins(&mut bars, b, key_fn);
                }
                let mut mgr = self.bubbles_mut();
                let prev_bars = mgr.bars(bid).to_vec();
                let tf_name = mgr.timeframe(bid).map(|tf| tf.name.clone()).unwrap_or_default();
                mgr.replace_bars(bid, bars.clone());
                drop(mgr);
                BubbleEvent::BarsLoaded {
                    bid, exchange_id, symbol, account_type: account_type_label,
                    timeframe: tf_name, bars, prev_bars,
                }
            }
            "renko" => {
                // Mirrors `append_renko`'s sorted upsert (the live path this
                // reseed must agree with).
                let key_fn = mlc_core::chart_type::renko_identity_key_fn(identity).unwrap_or(mlc_core::chart_type::renko_identity);
                let mut bricks: Vec<mlc_core::types::RenkoBrick> = Vec::new();
                for o in outputs {
                    let crate::bubble::FoldOutput::Renko(brick) = o else { continue };
                    mlc_core::chart_type::merge_last_wins(&mut bricks, brick, key_fn);
                }
                self.bubbles_mut().with_bubble_mut(bid, |b| b.renko_bars = Some(bricks));
                BubbleEvent::RenkoBarAppended { bid, exchange_id, symbol, account_type: account_type_label }
            }
            "pnf" => {
                // Mirrors `append_pnf`'s sorted upsert (the live path this
                // reseed must agree with).
                let key_fn = mlc_core::chart_type::pnf_identity_key_fn(identity).unwrap_or(mlc_core::chart_type::pnf_identity);
                let mut cols: Vec<mlc_core::types::PnfColumn> = Vec::new();
                for o in outputs {
                    let crate::bubble::FoldOutput::Pnf(col) = o else { continue };
                    mlc_core::chart_type::merge_last_wins(&mut cols, col, key_fn);
                }
                self.bubbles_mut().with_bubble_mut(bid, |b| b.pnf_columns = Some(cols));
                BubbleEvent::PnfBarAppended { bid, exchange_id, symbol, account_type: account_type_label }
            }
            "kagi" => {
                // Mirrors `append_kagi`'s sorted upsert (the live path this
                // reseed must agree with).
                let key_fn = mlc_core::chart_type::kagi_identity_key_fn(identity).unwrap_or(mlc_core::chart_type::kagi_identity);
                let mut segs: Vec<mlc_core::types::KagiSegment> = Vec::new();
                for o in outputs {
                    let crate::bubble::FoldOutput::Kagi(seg) = o else { continue };
                    mlc_core::chart_type::merge_last_wins(&mut segs, seg, key_fn);
                }
                self.bubbles_mut().with_bubble_mut(bid, |b| b.kagi_segments = Some(segs));
                BubbleEvent::KagiBarAppended { bid, exchange_id, symbol, account_type: account_type_label }
            }
            "three_line_break" => {
                // Mirrors `append_three_line_break`'s sorted upsert (the
                // live path this reseed must agree with).
                let key_fn = mlc_core::chart_type::three_line_break_identity_key_fn(identity)
                    .unwrap_or(mlc_core::chart_type::three_line_break_identity);
                let mut lines: Vec<mlc_core::types::ThreeLineBreakLine> = Vec::new();
                for o in outputs {
                    let crate::bubble::FoldOutput::ThreeLineBreak(line) = o else { continue };
                    mlc_core::chart_type::merge_last_wins(&mut lines, line, key_fn);
                }
                self.bubbles_mut().with_bubble_mut(bid, |b| b.three_line_break = Some(lines));
                BubbleEvent::TlbLineAppended { bid, exchange_id, symbol, account_type: account_type_label }
            }
            _ => return None,
        };
        Some(event)
    }

    /// Fold one live "1m" kline tick into `bid`'s active fold runtime, if
    /// installed, and append/mutate-last EVERY output into the bubble's slot
    /// (a gap-crossing delta-tick can close several intermediate range bars
    /// / Renko bricks in one call — see [`crate::bubble::FoldRuntime::
    /// on_live_1m`]'s doc comment; each is applied in order with the same
    /// upsert/append logic a single output used to get). `exchange_id`/
    /// `symbol`/`account_type_label` are the SAME identity the triggering
    /// `LiveUpdate::BarUpdate` event carried (`tick_writers` is the only
    /// caller) — not re-derived from the bubble, since a bubble might not
    /// even be the primary owner of this instrument key. Returns ONE
    /// reconcile-triggering [`BubbleEvent`] built from the LAST output
    /// (avoids event-spam for a multi-element chain — every intermediate
    /// output has already been durably applied to the bubble's slot by the
    /// time this returns), `None` when the bubble has no active fold runtime
    /// (not one of the 7 fold types) or the fold produced nothing
    /// (duplicate/absorbed tick).
    pub(crate) fn fold_on_live_1m(
        &self,
        bid: BubbleId,
        bar: Bar,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type_label: &str,
    ) -> Option<BubbleEvent> {
        let outputs = self.bubbles_mut().with_bubble_mut(bid, |b| {
            b.active_fold.as_mut().map(|f| f.on_live_1m(&bar))
        }).flatten()?;
        let symbol = symbol.to_string();
        let account_type_label = account_type_label.to_string();

        let mut event = None;
        for output in outputs {
            event = Some(match output {
                crate::bubble::FoldOutput::Bar(new_bar) => {
                    let mut mgr = self.bubbles_mut();
                    let last_ts_identity = mgr.bars(bid).last().map(|b| {
                        if b.derived_ts_ms != 0 { b.derived_ts_ms } else { b.timestamp }
                    });
                    let new_ts_identity = if new_bar.derived_ts_ms != 0 { new_bar.derived_ts_ms } else { new_bar.timestamp };
                    if last_ts_identity == Some(new_ts_identity) {
                        mgr.with_bubble_mut(bid, |b| {
                            if let Some(last) = b.last_bar_mut() {
                                last.open = new_bar.open;
                                last.high = new_bar.high;
                                last.low = new_bar.low;
                                last.close = new_bar.close;
                                last.volume = new_bar.volume;
                            }
                        });
                    } else {
                        mgr.with_bubble_mut(bid, |b| b.append_bar(new_bar));
                    }
                    drop(mgr);
                    BubbleEvent::BarUpdatePassthrough {
                        exchange_id, account_type: account_type_label.clone(), symbol: symbol.clone(),
                        bar: self.bubbles().bars(bid).last().copied().unwrap_or(new_bar),
                    }
                }
                crate::bubble::FoldOutput::Renko(brick) => {
                    self.bubbles_mut().with_bubble_mut(bid, |b| b.append_renko(brick));
                    BubbleEvent::RenkoBarAppended { bid, exchange_id, symbol: symbol.clone(), account_type: account_type_label.clone() }
                }
                crate::bubble::FoldOutput::Pnf(col) => {
                    self.bubbles_mut().with_bubble_mut(bid, |b| b.append_pnf(col));
                    BubbleEvent::PnfBarAppended { bid, exchange_id, symbol: symbol.clone(), account_type: account_type_label.clone() }
                }
                crate::bubble::FoldOutput::Kagi(seg) => {
                    self.bubbles_mut().with_bubble_mut(bid, |b| b.append_kagi(seg));
                    BubbleEvent::KagiBarAppended { bid, exchange_id, symbol: symbol.clone(), account_type: account_type_label.clone() }
                }
                crate::bubble::FoldOutput::ThreeLineBreak(line) => {
                    self.bubbles_mut().with_bubble_mut(bid, |b| b.append_three_line_break(line));
                    BubbleEvent::TlbLineAppended { bid, exchange_id, symbol: symbol.clone(), account_type: account_type_label.clone() }
                }
            });
        }
        event
    }

    // =========================================================================
    // === Client-side trade fold (unified-data-plane Stage Э4, 2026-07-09) ===
    // =========================================================================

    /// Install (or replace) `bid`'s [`bubble::TradeFoldRuntime`] for one of
    /// the trade-fed chart types (tick_bar/tick_imbalance_bar/
    /// volume_imbalance_bar/run_bar/footprint/tpo), ensure the bubble's
    /// trade layer is network-fed (continuous live subscribe + ramped
    /// backfill toward `derived_trade_target`), and reseed synchronously
    /// from whatever the layer already holds. Mirrors
    /// [`Self::subscribe_fold_chart_type`] exactly, one layer over (trade
    /// layer instead of the shared "1m" kline layer) — see that method's
    /// doc comment for the synchronous/async split rationale.
    ///
    /// `tpo_source` is ignored for every chart_type except `"tpo"` — see
    /// [`crate::bubble::TradeFoldMachine::new`]'s doc comment.
    ///
    /// The `"tpo"` Kline1m flavour also needs the "1m" kline layer
    /// network-fed (same as [`Self::subscribe_fold_chart_type`]'s callers
    /// use for the 7 kline-sufficient types) — handled here too so a caller
    /// never has to remember to call both entrypoints for that one flavour.
    pub fn subscribe_trade_fold_chart_type(
        &self,
        bid: BubbleId,
        chart_type: &'static str,
        params: crate::bubble::FoldParams,
        tpo_source: mlc_backend_data_bridge::TpoSource,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        derived_trade_target: Option<u32>,
    ) -> Option<BubbleEvent> {
        mlc_backend_data_bridge::dlog!(
            "[tradefold] subscribe ct={chart_type} count={} alpha={} min_ticks={} interval_ms={}",
            params.count, params.alpha, params.min_ticks, params.interval_ms
        );

        // Fix 2 (chart-type-engine-consolidation B1): install BEFORE
        // ensure — mirrors `subscribe_fold_chart_type`'s ordering fix, one
        // layer over (trade layer instead of "1m" kline). Build + reseed
        // the trade-fold runtime over whatever the trade layer (and, for
        // TPO Kline1m, the "1m" kline layer) holds RIGHT NOW, install it
        // into the bubble, and only THEN fire the async network calls
        // below (`subscribe_trades` itself starts an unbounded
        // `ensure_trade_backfill` page — see its doc comment — so it is
        // just as much a part of this race as the explicit
        // `derived_trade_target` call). By the time this method returns,
        // `active_trade_fold` is already installed, so the
        // `LiveUpdate::LayerBackfilled` refold sweep in `tick_writers` can
        // never find this bubble mid-install and skip it.
        let fold = crate::bubble::TradeFoldRuntime::new(chart_type, params, tpo_source, exchange_id, symbol)?;
        let event = self.reseed_trade_fold(bid, fold, exchange_id, symbol, account_type);

        // Live feed: `subscribe_trades` is idempotent per (eid, at, symbol)
        // and already fires the first shallow `ensure_trade_backfill` page
        // (see its doc comment) — calling it again here is a cheap no-op
        // when another window/indicator already opened the same trade sub.
        self.subscribe_trades(exchange_id, symbol, account_type);
        // History: ramped backfill toward `derived_trade_target`.
        self.bridge.ensure_trade_backfill(exchange_id, symbol, account_type, derived_trade_target.map(|t| t as usize));

        if matches!(chart_type, "tpo") && matches!(tpo_source, mlc_backend_data_bridge::TpoSource::Kline1m) {
            let one_minute = Timeframe::new("1m", 60_000);
            self.bridge.request_bars(exchange_id, symbol, &one_minute, account_type, None, None, false);
            // TPO-specific depth target (not `None`) — the shared "1m"
            // layer's ramp must deepen far enough to populate every session
            // in the ring even when a shallower consumer already opened
            // this layer first; see `TPO_KLINE_DEPTH_BARS`'s doc comment.
            self.bridge.ensure_kline_backfill(exchange_id, symbol, &one_minute, account_type, Some(TPO_KLINE_DEPTH_BARS));
        }

        event
    }

    /// Re-seed `bid`'s existing (or freshly-installed) trade-fold runtime
    /// from the bubble's current trade-layer (and, for TPO Kline1m, "1m"
    /// kline-layer) snapshot, replace the output slot wholesale, and return
    /// the reconcile-triggering [`BubbleEvent`].
    fn reseed_trade_fold(
        &self,
        bid: BubbleId,
        mut fold: crate::bubble::TradeFoldRuntime,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) -> Option<BubbleEvent> {
        let trade_snapshot: crate::bubble::RawLayer<crate::bubble::RawTrade> = self.bubbles()
            .trade_layer(bid)
            .cloned()
            .unwrap_or_else(|| crate::bubble::RawLayer::new(0));
        let kline_snapshot: crate::bubble::RawLayer<Bar> = self.bubbles()
            .kline_layer(bid, "1m")
            .cloned()
            .unwrap_or_else(|| crate::bubble::RawLayer::new(0));
        let outputs = fold.reseed(&trade_snapshot, &kline_snapshot);
        let chart_type = fold.chart_type();
        self.bubbles_mut().with_bubble_mut(bid, |b| b.active_trade_fold = Some(fold));
        let account_type_label = account_type.short_label().to_string();
        self.apply_trade_fold_reseed_outputs(bid, chart_type, outputs, exchange_id, symbol, &account_type_label)
    }

    /// Replace `bid`'s trade-fold output slot wholesale with `outputs` (a
    /// full reseed's worth of points, oldest-first) and build the matching
    /// reconcile-triggering [`BubbleEvent`]. Shared by
    /// [`Self::reseed_trade_fold`] and the `LiveUpdate::LayerBackfilled` arm
    /// in [`Self::tick_writers`] (scroll-deepen refold path for
    /// `layer == "trades"`) — mirrors [`Self::apply_fold_reseed_outputs`]'s
    /// role for the kline-fold family.
    pub(crate) fn apply_trade_fold_reseed_outputs(
        &self,
        bid: BubbleId,
        chart_type: &'static str,
        outputs: Vec<crate::bubble::TradeFoldOutput>,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type_label: &str,
    ) -> Option<BubbleEvent> {
        let symbol = symbol.to_string();
        let account_type_label = account_type_label.to_string();
        // Identity-key mapping is registry-driven (chart-type-engine-
        // consolidation B3, S6) — see `apply_fold_reseed_outputs`'s doc
        // comment for the same rationale. `"cvd"` is not a registry row
        // (see `view_fold.rs`'s doc comment on the synthetic id) — its
        // identity is `IdentityKey::TsMs` directly, not a `by_id` lookup.
        let identity = if chart_type == "cvd" {
            mlc_core::chart_type::IdentityKey::TsMs
        } else {
            mlc_core::chart_type::by_id(chart_type).map(|d| d.identity).unwrap_or(mlc_core::chart_type::IdentityKey::None)
        };
        let event = match chart_type {
            "tick_bar" | "tick_imbalance_bar" | "volume_imbalance_bar" | "run_bar" => {
                // The machines emit an output on EVERY consumed trade — an
                // in-progress snapshot of the currently-open bar (open frozen
                // at the bar's first trade, close walking) plus the final
                // closed shape when it rolls. The LIVE path merges those by
                // ts-identity in `trade_fold_on_live_trade`'s upsert branch;
                // a reseed replay MUST apply the same last-wins merge or the
                // seed array keeps every intermediate snapshot as a separate
                // bar (N copies per bar → solid staircase blocks on screen).
                let key_fn = mlc_core::chart_type::bar_identity_key_fn(identity).unwrap_or(mlc_core::chart_type::bar_identity);
                let mut bars: Vec<Bar> = Vec::new();
                for o in outputs {
                    let crate::bubble::TradeFoldOutput::Bar(b) = o else { continue };
                    mlc_core::chart_type::merge_last_wins(&mut bars, b, key_fn);
                }
                let mut mgr = self.bubbles_mut();
                let prev_bars = mgr.bars(bid).to_vec();
                let tf_name = mgr.timeframe(bid).map(|tf| tf.name.clone()).unwrap_or_default();
                mgr.replace_bars(bid, bars.clone());
                drop(mgr);
                BubbleEvent::BarsLoaded {
                    bid, exchange_id, symbol, account_type: account_type_label,
                    timeframe: tf_name, bars, prev_bars,
                }
            }
            "footprint" => {
                // Mirrors the live upsert in `trade_fold_on_live_trade`'s
                // `TradeFoldOutput::Footprint` arm (the machine emits a
                // growing-cluster snapshot per trade, not just on bar-close).
                let key_fn = mlc_core::chart_type::footprint_identity_key_fn(identity).unwrap_or(mlc_core::chart_type::footprint_identity);
                let mut bars: Vec<mlc_core::types::FootprintBar> = Vec::new();
                for o in outputs {
                    let crate::bubble::TradeFoldOutput::Footprint(b) = o else { continue };
                    mlc_core::chart_type::merge_last_wins(&mut bars, b, key_fn);
                }
                self.bubbles_mut().with_bubble_mut(bid, |b| {
                    // Fold-sourced footprint mirrors the OHLC ONLY when this
                    // bubble is on the full-takeover path (`bars_source ==
                    // FootprintOhlc`) — same gate the station-derived
                    // `LiveUpdate::FootprintUpdate` arm uses. Hybrid mode
                    // (RecentOnly venue, `bars_source` stays `Kline`) leaves
                    // `bubble.bars` alone — the kline backdrop writer owns
                    // it (see `Self::trade_fold_on_live_trade`'s hybrid
                    // handling for the matching live-path rule).
                    if b.bars_source == crate::bubble::BarsSource::FootprintOhlc {
                        let ohlc: Vec<mlc_core::Bar> = bars.iter().map(|fb| mlc_core::Bar::with_volume(
                            fb.open_time / 1000, fb.open, fb.high, fb.low, fb.close, fb.volume,
                        )).collect();
                        b.replace_bars(ohlc);
                    }
                    b.footprint_bars = Some(bars);
                });
                BubbleEvent::FootprintBarAppended { bid, exchange_id, symbol, account_type: account_type_label }
            }
            "tpo" => {
                // Dedup consecutive same-identity snapshots (the fold emits
                // one output per upstream event, many trades/bars share the
                // same session until it rolls) down to the LAST snapshot per
                // session via `merge_last_wins`, then cap to the same
                // recency window the live station-derived path uses
                // (`MAX_TPO_SESSIONS`) — the cap is TPO-specific bookkeeping
                // the shared helper doesn't know about, so it stays a
                // second pass here.
                let key_fn = mlc_core::chart_type::tpo_identity_key_fn(identity).unwrap_or(mlc_core::chart_type::tpo_identity);
                let mut deduped: Vec<mlc_core::types::TpoSession> = Vec::new();
                for o in outputs {
                    let crate::bubble::TradeFoldOutput::Tpo(s) = o else { continue };
                    mlc_core::chart_type::merge_last_wins(&mut deduped, s, key_fn);
                }
                mlc_backend_data_bridge::dlog!("[tradefold] tpo apply sessions={}", deduped.len());
                self.bubbles_mut().with_bubble_mut(bid, |b| {
                    if deduped.len() > MAX_TPO_SESSIONS {
                        let drop_n = deduped.len() - MAX_TPO_SESSIONS;
                        deduped.drain(0..drop_n);
                    }
                    b.tpo_sessions = Some(deduped);
                });
                BubbleEvent::TpoSessionUpdated { bid, exchange_id, symbol, account_type: account_type_label }
            }
            "cvd" => {
                // The machine emits one sample per consumed trade;
                // consecutive same-identity samples collapse to the last
                // value instead of oversampling the line.
                let key_fn = mlc_core::chart_type::cvd_identity_key_fn(identity).unwrap_or(mlc_core::chart_type::cvd_identity);
                let mut samples: Vec<mlc_core::types::CvdSample> = Vec::new();
                for o in outputs {
                    let crate::bubble::TradeFoldOutput::Cvd(s) = o else { continue };
                    mlc_core::chart_type::merge_last_wins(&mut samples, s, key_fn);
                }
                self.bubbles_mut().with_bubble_mut(bid, |b| b.cvd_line = Some(samples));
                BubbleEvent::CvdSampleAppended { bid, exchange_id, symbol, account_type: account_type_label }
            }
            _ => return None,
        };
        Some(event)
    }

    /// Fold one live trade tick into `bid`'s active trade-fold runtime, if
    /// installed, and append/upsert EVERY output into the bubble's slot, in
    /// order. Mirrors [`Self::fold_on_live_1m`]'s role (and multi-output
    /// handling) for the trade-fed family — `exchange_id`/`symbol`/
    /// `account_type_label` are the SAME identity the triggering
    /// `LiveUpdate::TradeUpdate` event carried. Returns ONE reconcile-
    /// triggering [`BubbleEvent`] built from the LAST output (see
    /// [`Self::fold_on_live_1m`]'s doc comment on the event-spam rationale),
    /// `None` when the bubble has no active trade-fold runtime or the fold
    /// produced nothing.
    pub(crate) fn trade_fold_on_live_trade(
        &self,
        bid: BubbleId,
        trade: crate::bubble::RawTrade,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type_label: &str,
    ) -> Option<BubbleEvent> {
        let outputs = self.bubbles_mut().with_bubble_mut(bid, |b| {
            b.active_trade_fold.as_mut().map(|f| f.on_live_trade(&trade))
        }).flatten()?;
        let symbol = symbol.to_string();
        let account_type_label = account_type_label.to_string();

        let mut event = None;
        for output in outputs {
            event = Some(match output {
                crate::bubble::TradeFoldOutput::Bar(new_bar) => {
                    let mut mgr = self.bubbles_mut();
                    let last_ts_identity = mgr.bars(bid).last().map(|b| {
                        if b.derived_ts_ms != 0 { b.derived_ts_ms } else { b.timestamp }
                    });
                    let new_ts_identity = if new_bar.derived_ts_ms != 0 { new_bar.derived_ts_ms } else { new_bar.timestamp };
                    mlc_backend_data_bridge::dlog!(
                        "[tradefold] live o={} h={} l={} c={} id={new_ts_identity} last={last_ts_identity:?} branch={} len={}",
                        new_bar.open, new_bar.high, new_bar.low, new_bar.close,
                        if last_ts_identity == Some(new_ts_identity) { "upsert" } else { "append" },
                        mgr.bars(bid).len()
                    );
                    if last_ts_identity == Some(new_ts_identity) {
                        mgr.with_bubble_mut(bid, |b| {
                            if let Some(last) = b.last_bar_mut() {
                                last.open = new_bar.open;
                                last.high = new_bar.high;
                                last.low = new_bar.low;
                                last.close = new_bar.close;
                                last.volume = new_bar.volume;
                            }
                        });
                    } else {
                        mgr.with_bubble_mut(bid, |b| b.append_bar(new_bar));
                    }
                    drop(mgr);
                    BubbleEvent::BarUpdatePassthrough {
                        exchange_id, account_type: account_type_label.clone(), symbol: symbol.clone(),
                        bar: self.bubbles().bars(bid).last().copied().unwrap_or(new_bar),
                    }
                }
                crate::bubble::TradeFoldOutput::Footprint(new_bar) => {
                    let mirror_ohlc = self.bubbles().bars_source(bid) == crate::bubble::BarsSource::FootprintOhlc;
                    self.bubbles_mut().with_bubble_mut(bid, |b| {
                        let ohlc_ts = new_bar.open_time / 1000;
                        let store = b.footprint_bars.get_or_insert_with(Vec::new);
                        match store.last_mut() {
                            Some(last) if last.open_time == new_bar.open_time => *last = new_bar.clone(),
                            _ => store.push(new_bar.clone()),
                        }
                        // Hybrid mode (RecentOnly venue): `mirror_ohlc == false`
                        // — the kline backdrop writer owns `bubble.bars`, same
                        // gate `LiveUpdate::FootprintUpdate`'s station-derived
                        // arm already uses (see that arm's doc comment).
                        if mirror_ohlc {
                            let ohlc = mlc_core::Bar::with_volume(
                                ohlc_ts, new_bar.open, new_bar.high, new_bar.low, new_bar.close, new_bar.volume,
                            );
                            match b.bars.last_mut() {
                                Some(last) if last.timestamp == ohlc_ts => *last = ohlc,
                                _ => b.append_bar(ohlc),
                            }
                        }
                    });
                    BubbleEvent::FootprintBarAppended { bid, exchange_id, symbol: symbol.clone(), account_type: account_type_label.clone() }
                }
                crate::bubble::TradeFoldOutput::Tpo(session) => {
                    self.bubbles_mut().with_bubble_mut(bid, |b| {
                        let store = b.tpo_sessions.get_or_insert_with(Vec::new);
                        match store.last_mut() {
                            Some(last) if last.open_time == session.open_time => *last = session,
                            _ => {
                                store.push(session);
                                while store.len() > MAX_TPO_SESSIONS {
                                    store.remove(0);
                                }
                            }
                        }
                    });
                    BubbleEvent::TpoSessionUpdated { bid, exchange_id, symbol: symbol.clone(), account_type: account_type_label.clone() }
                }
                crate::bubble::TradeFoldOutput::Cvd(sample) => {
                    self.bubbles_mut().with_bubble_mut(bid, |b| b.append_cvd(sample));
                    BubbleEvent::CvdSampleAppended { bid, exchange_id, symbol: symbol.clone(), account_type: account_type_label.clone() }
                }
            });
        }
        event
    }

    /// Fold one live "1m" kline tick into `bid`'s active trade-fold runtime
    /// IF it's the TPO Kline1m flavour (every other trade-fed chart_type's
    /// runtime is a no-op for `Event::Bar` — see
    /// [`crate::bubble::TradeFoldRuntime::on_live_1m`]). Mirrors
    /// [`Self::fold_on_live_1m`]'s upsert-into-`tpo_sessions` shape, applied
    /// to EVERY output in order (today always 0 or 1 — TPO never populates a
    /// pending queue — kept as a loop for symmetry with the other live-fold
    /// handlers, see [`Self::fold_on_live_1m`]'s doc comment).
    pub(crate) fn trade_fold_on_live_1m(
        &self,
        bid: BubbleId,
        bar: Bar,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type_label: &str,
    ) -> Option<BubbleEvent> {
        let sessions: Vec<mlc_core::types::TpoSession> = self.bubbles_mut().with_bubble_mut(bid, |b| {
            b.active_trade_fold.as_mut().map(|f| f.on_live_1m(&bar))
        }).flatten()?.into_iter().filter_map(|o| match o {
            crate::bubble::TradeFoldOutput::Tpo(s) => Some(s),
            _ => None,
        }).collect();
        if sessions.is_empty() {
            return None;
        }
        let symbol = symbol.to_string();
        let account_type_label = account_type_label.to_string();
        for session in sessions {
            self.bubbles_mut().with_bubble_mut(bid, |b| {
                let store = b.tpo_sessions.get_or_insert_with(Vec::new);
                match store.last_mut() {
                    Some(last) if last.open_time == session.open_time => *last = session,
                    _ => {
                        store.push(session);
                        while store.len() > MAX_TPO_SESSIONS {
                            store.remove(0);
                        }
                    }
                }
            });
        }
        Some(BubbleEvent::TpoSessionUpdated { bid, exchange_id, symbol, account_type: account_type_label })
    }

    // =========================================================================
    // === Client-side CVD companion fold (unified-data-plane Stage Э4/Э5) ====
    // =========================================================================

    /// Install (or replace) `bid`'s [`bubble::TradeFoldRuntime`] CVD
    /// companion runtime, ensure the trade layer is network-fed, and reseed
    /// synchronously from whatever the layer already holds. Called from
    /// `subscribe_cvd_line_for_indicator_add` (mlc-app) — the ONLY CVD data
    /// path since Stage Э5 removed the station-derived `request_cvd_line`
    /// subscribe.
    pub fn subscribe_cvd_fold(
        &self,
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        derived_trade_target: Option<u32>,
    ) -> Option<BubbleEvent> {
        self.subscribe_trades(exchange_id, symbol, account_type);
        self.bridge.ensure_trade_backfill(exchange_id, symbol, account_type, derived_trade_target.map(|t| t as usize));

        let mut fold = crate::bubble::TradeFoldRuntime::new(
            "cvd", crate::bubble::FoldParams::default(), mlc_backend_data_bridge::TpoSource::TradeBucket,
            exchange_id, symbol,
        )?;
        let trade_snapshot: crate::bubble::RawLayer<crate::bubble::RawTrade> = self.bubbles()
            .trade_layer(bid)
            .cloned()
            .unwrap_or_else(|| crate::bubble::RawLayer::new(0));
        let outputs = fold.reseed(&trade_snapshot, &crate::bubble::RawLayer::new(0));
        self.bubbles_mut().with_bubble_mut(bid, |b| b.active_cvd_fold = Some(fold));
        let account_type_label = account_type.short_label().to_string();
        self.apply_trade_fold_reseed_outputs(bid, "cvd", outputs, exchange_id, symbol, &account_type_label)
    }

    /// Fold one live trade tick into `bid`'s active CVD companion runtime,
    /// if installed. Mirrors [`Self::trade_fold_on_live_trade`]'s CVD arm,
    /// scoped to the independent `active_cvd_fold` slot. Applies EVERY
    /// output in order (today always 0 or 1 — CVD never populates a pending
    /// queue — kept as a loop for symmetry, see [`Self::fold_on_live_1m`]'s
    /// doc comment).
    pub(crate) fn cvd_fold_on_live_trade(
        &self,
        bid: BubbleId,
        trade: crate::bubble::RawTrade,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type_label: &str,
    ) -> Option<BubbleEvent> {
        let samples: Vec<mlc_core::types::CvdSample> = self.bubbles_mut().with_bubble_mut(bid, |b| {
            b.active_cvd_fold.as_mut().map(|f| f.on_live_trade(&trade))
        }).flatten()?.into_iter().filter_map(|o| match o {
            crate::bubble::TradeFoldOutput::Cvd(s) => Some(s),
            _ => None,
        }).collect();
        if samples.is_empty() {
            return None;
        }
        for sample in samples {
            self.bubbles_mut().with_bubble_mut(bid, |b| b.append_cvd(sample));
        }
        Some(BubbleEvent::CvdSampleAppended {
            bid, exchange_id, symbol: symbol.to_string(), account_type: account_type_label.to_string(),
        })
    }
}

#[cfg(test)]
mod fold_reseed_last_wins_tests {
    //! Regression coverage for the reseed-replay dup-bar bug (2026-07-10):
    //! fold machines emit an output snapshot on EVERY consumed event, not
    //! just on element-close. `apply_fold_reseed_outputs` /
    //! `apply_trade_fold_reseed_outputs` must collapse same-identity
    //! snapshots to the LAST one (mirrors the live-path upsert rule) before
    //! landing them in the bubble's slot — a straight `.collect()` would
    //! keep every intermediate snapshot as a separate element.
    //!
    //! Each case feeds a synthetic `outputs` vec with 3 snapshots sharing
    //! one identity (values 1.0/2.0/3.0, only the last must survive) plus
    //! 1 snapshot of a second identity, and expects exactly 2 elements in
    //! the bubble's slot afterward with the last-wins value for the first.

    use super::*;
    use mlc_backend_data_bridge::DataBridge;
    use mlc_core::ids::BubbleId;

    fn build_core() -> std::sync::Arc<BackendCore> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio rt");
        let (bridge, _live_rx, _connector_ready_rx) = rt.block_on(DataBridge::new());
        BackendCore::new(std::sync::Arc::new(bridge), None)
    }

    fn make_bubble(core: &BackendCore) -> BubbleId {
        core.bubbles_mut().create_bubble("BTCUSDT".to_string(), Timeframe::m1())
    }

    #[test]
    fn range_bar_reseed_collapses_same_identity_snapshots() {
        let core = build_core();
        let bid = make_bubble(&core);
        let outputs = vec![
            crate::bubble::FoldOutput::Bar(Bar { timestamp: 100, derived_ts_ms: 100_000, close: 1.0, ..Bar::default() }),
            crate::bubble::FoldOutput::Bar(Bar { timestamp: 100, derived_ts_ms: 100_000, close: 2.0, ..Bar::default() }),
            crate::bubble::FoldOutput::Bar(Bar { timestamp: 100, derived_ts_ms: 100_000, close: 3.0, ..Bar::default() }),
            crate::bubble::FoldOutput::Bar(Bar { timestamp: 160, derived_ts_ms: 160_000, close: 9.0, ..Bar::default() }),
        ];
        core.apply_fold_reseed_outputs(bid, "range_bar", outputs, ExchangeId::Binance, "BTCUSDT", "S");
        let mgr = core.bubbles();
        let bars = mgr.bars(bid);
        assert_eq!(bars.len(), 2, "3 same-identity snapshots + 1 distinct must collapse to 2 bars");
        assert_eq!(bars[0].close, 3.0, "must keep the LAST snapshot's value, not the first");
        assert_eq!(bars[1].close, 9.0);
    }

    #[test]
    fn renko_reseed_collapses_same_open_time_snapshots() {
        let core = build_core();
        let bid = make_bubble(&core);
        let mk = |open_time: i64, top: f64| mlc_core::types::RenkoBrick {
            open_time, bottom: 0.0, top, up: true, volume: 0.0, trades_count: 0, wick: 0.0,
        };
        let outputs = vec![
            crate::bubble::FoldOutput::Renko(mk(100, 1.0)),
            crate::bubble::FoldOutput::Renko(mk(100, 2.0)),
            crate::bubble::FoldOutput::Renko(mk(100, 3.0)),
            crate::bubble::FoldOutput::Renko(mk(200, 9.0)),
        ];
        core.apply_fold_reseed_outputs(bid, "renko", outputs, ExchangeId::Binance, "BTCUSDT", "S");
        let mgr = core.bubbles();
        let bricks = mgr.renko_bars(bid).expect("renko_bars must be populated");
        assert_eq!(bricks.len(), 2);
        assert_eq!(bricks[0].top, 3.0, "must keep the LAST snapshot's value, not the first");
        assert_eq!(bricks[1].top, 9.0);
    }

    #[test]
    fn pnf_reseed_collapses_same_open_time_snapshots() {
        let core = build_core();
        let bid = make_bubble(&core);
        let mk = |open_time: i64, top: f64| mlc_core::types::PnfColumn {
            open_time, column_id: 1, is_x: true, bottom: 0.0, top, volume: 0.0, trades_count: 0, box_size: 1.0,
        };
        let outputs = vec![
            crate::bubble::FoldOutput::Pnf(mk(100, 1.0)),
            crate::bubble::FoldOutput::Pnf(mk(100, 2.0)),
            crate::bubble::FoldOutput::Pnf(mk(100, 3.0)),
            crate::bubble::FoldOutput::Pnf(mk(200, 9.0)),
        ];
        core.apply_fold_reseed_outputs(bid, "pnf", outputs, ExchangeId::Binance, "BTCUSDT", "S");
        let mgr = core.bubbles();
        let cols = mgr.pnf_columns(bid).expect("pnf_columns must be populated");
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0].top, 3.0, "must keep the LAST snapshot's value, not the first");
        assert_eq!(cols[1].top, 9.0);
    }

    #[test]
    fn kagi_reseed_collapses_same_open_time_snapshots() {
        let core = build_core();
        let bid = make_bubble(&core);
        let mk = |open_time: i64, end_price: f64| mlc_core::types::KagiSegment {
            open_time, start_price: 0.0, end_price, up: true, yang: true, is_connector: false,
        };
        let outputs = vec![
            crate::bubble::FoldOutput::Kagi(mk(100, 1.0)),
            crate::bubble::FoldOutput::Kagi(mk(100, 2.0)),
            crate::bubble::FoldOutput::Kagi(mk(100, 3.0)),
            crate::bubble::FoldOutput::Kagi(mk(200, 9.0)),
        ];
        core.apply_fold_reseed_outputs(bid, "kagi", outputs, ExchangeId::Binance, "BTCUSDT", "S");
        let mgr = core.bubbles();
        let segs = mgr.kagi_segments(bid).expect("kagi_segments must be populated");
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].end_price, 3.0, "must keep the LAST snapshot's value, not the first");
        assert_eq!(segs[1].end_price, 9.0);
    }

    #[test]
    fn three_line_break_reseed_collapses_same_ts_open_snapshots() {
        let core = build_core();
        let bid = make_bubble(&core);
        let mk = |ts_open: i64, price_close: f64| mlc_core::types::ThreeLineBreakLine {
            ts_open, ts_close: ts_open, price_open: 0.0, price_close, volume: 0.0, direction: 0,
        };
        let outputs = vec![
            crate::bubble::FoldOutput::ThreeLineBreak(mk(100, 1.0)),
            crate::bubble::FoldOutput::ThreeLineBreak(mk(100, 2.0)),
            crate::bubble::FoldOutput::ThreeLineBreak(mk(100, 3.0)),
            crate::bubble::FoldOutput::ThreeLineBreak(mk(200, 9.0)),
        ];
        core.apply_fold_reseed_outputs(bid, "three_line_break", outputs, ExchangeId::Binance, "BTCUSDT", "S");
        let mgr = core.bubbles();
        let lines = mgr.three_line_break(bid).expect("three_line_break must be populated");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].price_close, 3.0, "must keep the LAST snapshot's value, not the first");
        assert_eq!(lines[1].price_close, 9.0);
    }

    #[test]
    fn tick_bar_reseed_collapses_same_identity_snapshots() {
        let core = build_core();
        let bid = make_bubble(&core);
        let outputs = vec![
            crate::bubble::TradeFoldOutput::Bar(Bar { timestamp: 100, derived_ts_ms: 100_000, close: 1.0, ..Bar::default() }),
            crate::bubble::TradeFoldOutput::Bar(Bar { timestamp: 100, derived_ts_ms: 100_000, close: 2.0, ..Bar::default() }),
            crate::bubble::TradeFoldOutput::Bar(Bar { timestamp: 100, derived_ts_ms: 100_000, close: 3.0, ..Bar::default() }),
            crate::bubble::TradeFoldOutput::Bar(Bar { timestamp: 160, derived_ts_ms: 160_000, close: 9.0, ..Bar::default() }),
        ];
        core.apply_trade_fold_reseed_outputs(bid, "tick_bar", outputs, ExchangeId::Binance, "BTCUSDT", "S");
        let mgr = core.bubbles();
        let bars = mgr.bars(bid);
        assert_eq!(bars.len(), 2, "3 same-identity snapshots + 1 distinct must collapse to 2 bars");
        assert_eq!(bars[0].close, 3.0, "must keep the LAST snapshot's value, not the first");
        assert_eq!(bars[1].close, 9.0);
    }

    #[test]
    fn footprint_reseed_collapses_same_open_time_snapshots() {
        let core = build_core();
        let bid = make_bubble(&core);
        let mk = |open_time: i64, close: f64| mlc_core::types::FootprintBar {
            open_time, close, ..mlc_core::types::FootprintBar::default()
        };
        let outputs = vec![
            crate::bubble::TradeFoldOutput::Footprint(mk(100, 1.0)),
            crate::bubble::TradeFoldOutput::Footprint(mk(100, 2.0)),
            crate::bubble::TradeFoldOutput::Footprint(mk(100, 3.0)),
            crate::bubble::TradeFoldOutput::Footprint(mk(200, 9.0)),
        ];
        core.apply_trade_fold_reseed_outputs(bid, "footprint", outputs, ExchangeId::Binance, "BTCUSDT", "S");
        let mgr = core.bubbles();
        let bars = mgr.footprint_bars(bid).expect("footprint_bars must be populated");
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].close, 3.0, "must keep the LAST snapshot's value, not the first");
        assert_eq!(bars[1].close, 9.0);
    }

    #[test]
    fn cvd_reseed_collapses_same_ts_ms_snapshots() {
        let core = build_core();
        let bid = make_bubble(&core);
        let mk = |ts_ms: i64, value: f64| mlc_core::types::CvdSample { ts_ms, value };
        let outputs = vec![
            crate::bubble::TradeFoldOutput::Cvd(mk(100, 1.0)),
            crate::bubble::TradeFoldOutput::Cvd(mk(100, 2.0)),
            crate::bubble::TradeFoldOutput::Cvd(mk(100, 3.0)),
            crate::bubble::TradeFoldOutput::Cvd(mk(200, 9.0)),
        ];
        core.apply_trade_fold_reseed_outputs(bid, "cvd", outputs, ExchangeId::Binance, "BTCUSDT", "S");
        let mgr = core.bubbles();
        let samples = mgr.cvd_line(bid).expect("cvd_line must be populated");
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].value, 3.0, "must keep the LAST snapshot's value, not the first");
        assert_eq!(samples[1].value, 9.0);
    }

    /// «4R != 100R» re-cut regression (chart-type-engine-consolidation B4,
    /// T2.2) — end-to-end through the SAME seams the real subscribe/refold
    /// path uses: `FoldRuntime::reseed` over a synthetic "1m" `RawLayer`,
    /// then `apply_fold_reseed_outputs` (the exact merge-and-land call the
    /// live reseed path makes) rather than hand-building `FoldOutput`s. A
    /// tight range threshold (small R) MUST cut visibly more bars than a
    /// wide one (big R) over the SAME input — the live re-cut bug this
    /// pins had chart-type/param switches over-fold identically because a
    /// shallow "1m" layer ramp was starved after the first fold type
    /// claimed it (see this crate's docs on the 4R=100R bug); this test
    /// pins the FOLD MATH side (param actually changes the cut), not the
    /// backfill-ramp side (covered elsewhere).
    #[test]
    fn range_bar_recut_small_r_produces_far_more_bars_than_big_r() {
        // Synthetic price walk: sin (amplitude 200) riding a linear trend
        // (+8/bar) over 200 1m bars, each expanded into a 4-leg synthetic
        // trade path (O -> L/H -> H/L -> C) by `kline_to_synthetic_trades`
        // — the actual granularity a range-bar fold walks over. Amplitude
        // and trend both dwarf both R thresholds under test (R=1 tight,
        // R=25 wide) so neither collapses to a near-empty cut.
        const N: usize = 200;
        const SMALL_R: f64 = 1.0;
        const BIG_R: f64 = 25.0;
        let mut layer: crate::bubble::RawLayer<Bar> = crate::bubble::RawLayer::new(0);
        let mut base = 1000.0_f64;
        for i in 0..N {
            let ts_sec = (i as i64 + 1) * 60;
            let open = base + 200.0 * (i as f64 * 0.3).sin();
            base += 8.0;
            let close = base + 200.0 * ((i + 1) as f64 * 0.3).sin();
            let high = open.max(close) + 15.0;
            let low = open.min(close) - 15.0;
            layer.push_live(Bar {
                timestamp: ts_sec,
                open, high, low, close,
                volume: 10.0,
                derived_ts_ms: ts_sec * 1000,
            });
        }

        let run = |scaled_units: f64| -> Vec<Bar> {
            let core = build_core();
            let bid = make_bubble(&core);
            let params = crate::bubble::FoldParams { scaled_units, ..Default::default() };
            let mut fold = crate::bubble::FoldRuntime::new("range_bar", params, ExchangeId::Binance, "BTCUSDT")
                .expect("range_bar is a fold type");
            let outputs = fold.reseed(&layer);
            core.apply_fold_reseed_outputs(bid, "range_bar", outputs, ExchangeId::Binance, "BTCUSDT", "S");
            let bars = core.bubbles().bars(bid).to_vec();
            bars
        };

        let small_bars = run(SMALL_R);
        let big_bars = run(BIG_R);

        assert!(
            small_bars.len() > 3 * big_bars.len(),
            "small R={SMALL_R} must cut far more bars than big R={BIG_R}: small={} big={}",
            small_bars.len(), big_bars.len(),
        );
        // Both cuts must be non-trivial relative to the input — neither R
        // starved down to a near-empty fold of the 200-bar layer (the
        // actual shape of the live "same tiny window forever" bug: an
        // apparently successful re-cut that only ever touches a sliver of
        // the layer). Compared against the raw bar count, not the 4x
        // synthetic-trade-leg expansion, since a range-bar cut's element
        // count is naturally far smaller than its leg-granularity input.
        assert!(small_bars.len() > N / 2, "small R cut only {} bars from {N} input bars", small_bars.len());
        assert!(big_bars.len() > N / 2, "big R cut only {} bars from {N} input bars", big_bars.len());

        // Every CLOSED big-R bar (all but the last, which may still be
        // in-progress) must be pinned to AT LEAST the configured range on
        // its crossing side and must chain open-to-prior-close — the "4R
        // vs 100R identical" bug's other face was bars that didn't
        // actually respect the configured threshold at all. Width can
        // exceed R (not just equal it): the crossing side is pinned exactly
        // to `range` (`TradeToRangeBarDerived::fold_trade`'s `bar.high =
        // close_price` / `bar.low = close_price` on the crossing branch),
        // but the OPPOSITE side keeps whatever organic excursion the bar
        // absorbed before it crossed — real (non-single-gap-leg) price
        // walks routinely produce this, so `>=` is the correct invariant
        // here (the single-leg case where width == R exactly is already
        // pinned by `reseed_range_bar_gap_crossing_kline_leg_produces_chained_bars`).
        let (closed, _last) = big_bars.split_at(big_bars.len().saturating_sub(1));
        for (i, b) in closed.iter().enumerate() {
            assert!(
                b.high - b.low >= BIG_R - 1e-9,
                "closed big-R bar #{i} has |high-low|={} < R={BIG_R}", b.high - b.low,
            );
            if i > 0 {
                assert!(
                    (b.open - closed[i - 1].close).abs() < 1e-9,
                    "big-R bar #{i}.open ({}) must equal bar #{}.close ({})",
                    b.open, i - 1, closed[i - 1].close,
                );
            }
        }
    }
}
