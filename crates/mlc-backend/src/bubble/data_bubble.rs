use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use mlc_core::chart::types::price_scale::ScaleMode;
use mlc_core::drawing::primitives_v2::config::TemplateStyle;
use mlc_core::ids::{BubbleId, ConnectionStatus, SyncFlags};
use mlc_core::indicator_types::IndicatorGroupConfig;
use mlc_core::instrument::InstrumentAddress;
use mlc_core::state::CommandHistory;
use mlc_core::state::Timeframe;

use super::raw_layer::RawLayer;

/// Default ring capacity for one [`DataBubble::kline_layers`] entry (points
/// per touched timeframe). Overridden per-profile by
/// `UserProfile.domain.data_load.kline_layer_capacity` — see
/// `docs/mlc/plans/unified-data-plane-2026-07-09.md` Stage Э1.
pub const DEFAULT_KLINE_LAYER_CAPACITY: usize = 200_000;

/// Default ring capacity for a [`crate::trade_store::TradeStore`] entry.
/// Overridden per-profile by
/// `UserProfile.domain.data_load.trade_layer_capacity`.
pub const DEFAULT_TRADE_LAYER_CAPACITY: usize = 1_000_000;

/// Refold debounce window (P0 hang fix part 2) — see
/// [`DataBubble::last_refold_at`]'s doc comment. A `LayerBackfilled` page
/// arriving sooner than this after the bubble's last completed refold sets
/// [`DataBubble::refold_dirty`] instead of refolding immediately; streaming
/// backfill pages (~1/s) then collapse to at most 2 refolds/second per
/// bubble instead of one per page.
pub const REFOLD_MIN_INTERVAL_MS: u64 = 500;

/// Ownership marker for `DataBubble.bars` (single-writer contract).
///
/// `Kline` (default): the kline family owns `bars` — REST loads
/// (`BarsLoaded`/`BackfillComplete`/`ScrollBarsLoaded`), live kline WS
/// (`BarUpdate`) and trade-built bars (`TradeUpdate`) may write.
///
/// `FootprintOhlc`: the footprint pipeline owns `bars` — each
/// `FootprintUpdate` upserts the bar built from the FootprintPoint's own
/// OHLC, and every kline-family writer must drop its write for this
/// bubble. Set by the footprint arm of `ChangeChartType` (and preset
/// hydration of a footprint window); reset to `Kline` on switch to any
/// other chart type.
///
/// `TradeAggBars`: the trade-agg derived pipeline owns `bars` — the
/// registry's `ChartTypeKind::TradeAggBar` family (range_bar/volume_bar/
/// tick_bar/dollar_bar/tick_imbalance_bar/volume_imbalance_bar/run_bar)
/// streams its station-derived series directly into `bubble.bars` (the
/// SAME array the kline family renders — see `resolve_active_series`),
/// while `bubble.timeframe` stays whatever ordinary wall-clock TF was
/// active before the switch (never re-stamped, unlike footprint's bucket
/// interval). Before this variant existed, these bubbles stayed on the
/// default `Kline` value with no ownership marker of their own, so every
/// kline-family writer (`BarsLoaded`/`BarUpdate`/`BackfillComplete`/
/// `ScrollBarsLoaded`) treated them as kline-owned and stomped the
/// derived series (observed live: switching a range_bar param repainted
/// the pane with plain kline candles). Stamped at subscribe time by the
/// chart-type engine's `clear_slot_trade_agg_bars` (mirrors
/// `clear_slot_footprint_hybrid_aware`'s stamp for footprint); reset to
/// `Kline` on switch to any other chart type (`clear_slot_kline_bars`,
/// same reset every render-family/heatmap/price-action row already uses).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BarsSource {
    #[default]
    Kline,
    FootprintOhlc,
    TradeAggBars,
}

/// How much history backs this bubble's active series, derived from the
/// most recent seed/deepen outcome the bridge reported.
///
/// Bridge-agnostic — no `digdigdig3-station` types in the signature
/// (`mlc-backend` depends on `digdigdig3` directly but NOT
/// `digdigdig3-station`; the translation from `SeedOutcome`/
/// `TruncationReason` happens in `mlc-backend-data-bridge`, which sends the
/// already-reduced variant across the `LiveUpdate` channel — see
/// `nemo/docs/mlc/plans/hybrid-data-modes-plan-2026-07-08.md` Step 1).
///
/// PER-bubble, not per-chart_type: `DataBubble` renders one chart_type at a
/// time, so this is overwritten on every chart-type switch (`clear_slot_*`
/// resets it to `Unknown`) and repopulated by the first
/// `LiveUpdate::HistoryDepthReported` for the NEW active type.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HistoryDepth {
    /// No seed fetch ran yet for the active series (still loading, or this
    /// chart_type has no seed concept — e.g. dom/liquidation heatmap).
    #[default]
    Unknown,
    /// Seed fully satisfied the request, OR the venue has real deep/paginated
    /// history and no ceiling was hit — deepen ladder still has room.
    Full,
    /// Venue supports SOME real history but a hard/soft ceiling was hit
    /// (Binance futures 24h wall, MEXC/Gate window cap) — `achieved` is the
    /// last known depth in points; further deepen calls are pre-emptively
    /// skipped once the bridge reports this.
    Capped { achieved: usize },
    /// Venue capability is `RecentOnly` (Bybit 60/1000, KuCoin 100, ...) —
    /// no REST pagination exists at all. The active series is backed ONLY
    /// by what has accumulated from WS since subscribe; deepen is a
    /// permanent no-op for this instrument+chart_type combination.
    LiveOnly,
}

impl HistoryDepth {
    /// True when scroll-left / cold-open deepen should not even try.
    pub fn is_deepen_blocked(&self) -> bool {
        matches!(self, HistoryDepth::LiveOnly)
    }

    /// 1:1 mirror into the mlc-core-safe render-facing shape (hybrid-data-
    /// modes-plan-2026-07-08, Step 6) — `mlc-core` cannot see this type, so
    /// `build_chart_render_state` calls this to populate `ChartRenderState.
    /// history_depth` for the degraded-history badge.
    pub fn to_display(self) -> mlc_core::chart::render_data::HistoryDepthDisplay {
        use mlc_core::chart::render_data::HistoryDepthDisplay as Display;
        match self {
            HistoryDepth::Unknown => Display::Unknown,
            HistoryDepth::Full => Display::Full,
            HistoryDepth::Capped { achieved } => Display::Capped { achieved },
            HistoryDepth::LiveOnly => Display::LiveOnly,
        }
    }
}

/// A mathematical data space: bars, primitives, indicator configs, alerts,
/// and derived series for one instrument+timeframe.
///
/// `color`, `auto_created`, `members`, `member_overrides` live on the paired
/// `Tag` (`mlc_backend::tag::TagManager`), not here — bubble grouping /
/// color identity / member routing is Tag's domain (Step 6 doctrine
/// cutover, `docs/mlc/plans/doctrine-cutover-2026-06-27.md`). `DataBubble.id
/// == Tag.id` by the 1:1 pairing invariant.
pub struct DataBubble {
    pub id: BubbleId,

    // Owned state shared across all member windows
    pub primitives: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>>,
    pub indicator_configs: Vec<IndicatorGroupConfig>,
    /// Instrument this bubble tracks (exchange, symbol, account_type).
    /// Persistence (`DataBubbleSnapshot`) keeps the flat `String` triplet —
    /// converted at `from_snapshot` / `to_snapshot`.
    pub address: InstrumentAddress,
    pub timeframe: Timeframe,

    /// Active chart type name (e.g. `"candles"`, `"bars"`, `"line"`,
    /// `"footprint"`). Default: `"candles"`. Symmetric with `timeframe` —
    /// shared across every member window of this bubble.
    pub chart_type: &'static str,

    /// Numeric parameter for derived-bar chart types (range_bar / volume_bar).
    ///
    /// - For `"range_bar"`: price-move size (e.g. `10.0` USD) — converted to
    ///   ×1e8 fixed-point at the bridge boundary.
    /// - For `"volume_bar"`: cumulative volume per bar (e.g. `1000.0` units).
    /// - For `"footprint"`: unused — footprint uses `chart_type_interval` instead.
    /// - For plain render chart types: unused.
    ///
    /// Default `0.0` — code-side defaults (10.0 / 1000.0) are applied at
    /// subscribe time when this field is non-positive.
    pub chart_type_param: f64,

    /// Box-size computation mode for `chart_type_param` — meaningful only
    /// for `"renko"`/`"kagi"` (granularity arc, кусок 2). `Fixed` (default)
    /// means `chart_type_param` is a plain ladder/custom value; `Atr14`
    /// means it was last written by `BackendCore::atr14_box_ticks`'s
    /// recompute and should be treated as "derived", not "user-typed", by
    /// any UI that distinguishes the two. Reset to `Fixed` by
    /// [`super::store::BubbleStore::set_chart_type`] on a genuine
    /// chart-type CHANGE (same rule as `chart_type_param`'s own reset
    /// there) and by an explicit numeric pick (`UpdateChartTypeParam`).
    pub chart_type_param_mode: mlc_core::chart_type::ParamMode,

    /// Kline interval label for `"footprint"` chart type (e.g. `"1m"`, `"5m"`).
    /// Default: `"1m"`. Unused for non-footprint chart types.
    pub chart_type_interval: &'static str,

    /// Which properties are synchronized.
    pub sync_flags: SyncFlags,

    /// Bubble-persisted price-scale corner mode (Auto / Manual / Focus).
    ///
    /// Authoritative when `sync_flags.sync_viewport = true`. All members
    /// display this mode; clicking A/M/F on any peer updates it here and
    /// pushes to all peers via `propagate_viewport_to_sync_group`.
    /// When `sync_viewport = false` each window manages its own mode independently.
    pub scale_mode: ScaleMode,

    /// Shared undo/redo history for operations that affect all members
    /// (primitives, indicator configs). Window-local operations (viewport,
    /// symbol) are stored on the individual window's `command_history`
    /// instead. Chart-type undo records also live on the window's
    /// `command_history` — the intent (`Command::ChangeChartType`) is
    /// window-local even though the resulting value lives on the bubble.
    pub command_history: CommandHistory,

    /// Indicator instance ids that were active on the chart BEFORE it joined a
    /// color-tag sync group.  Populated at join time so desync can restore and
    /// unhide them.  Cleared after desync.
    pub pre_tag_indicator_ids: Vec<u64>,

    /// Bubble-level last-used drawing style per tool type.
    ///
    /// Shared via `Arc` with every member window's `DrawingManager` so that a
    /// style change in *any* window of the bubble is immediately visible in all
    /// peers (fixes bugs B1 + D1).  The `Arc` handle in the DM is rebounded
    /// whenever a window joins or leaves this bubble.
    pub last_used_style: Arc<RwLock<HashMap<String, TemplateStyle>>>,

    /// Previous session close price for this bubble's instrument (`bars[0].open`).
    /// Populated by `BarsLoaded` handlers; read by the renderer for the
    /// price-change badge.
    pub prev_close_price: Option<f64>,

    /// Live / Delayed / Disconnected indicator for this bubble's data feed.
    pub connection_status: ConnectionStatus,

    // =========================================================================
    // === Per-Kind series storage (Phase X0, 2026-06-19) ======================
    // =========================================================================
    // Owner mandate: the bubble is a storage hub. Bridge writes raw via the
    // append_*/replace_*/clear_* methods below. Methods mutate ONLY the
    // storage — no viewport calls, no derived recompute, no callbacks.
    // Consumers (ChartContainer) PULL state during their own frame prepare.
    // See nemo/docs/mlc/plans/xaxis-phase-x0-ownership-refactor-2026-06-19.md
    // =========================================================================

    /// OHLC bars for the bubble's instrument. Written by `BarsLoaded`,
    /// `BackfillComplete`, `ScrollBarsLoaded`, `BarUpdate`, `TradeUpdate`
    /// — but ONLY while `bars_source == BarsSource::Kline`. See
    /// [`BarsSource`] for the ownership contract.
    pub bars: Vec<mlc_core::Bar>,

    /// Who owns writes into `bars` right now (single-writer contract).
    ///
    /// Footprint is one chart type whose own derived pipeline
    /// (`FootprintUpdate`) writes OHLC bars into `bars` while the window
    /// stays on a TIME timeframe — so the tick-TF exclusion that protects
    /// range/volume/tick bubbles does not fire, and five independent kline
    /// writers (live kline WS `BarUpdate`, trade-built bars from
    /// `TradeUpdate`, `BarsLoaded` REST replace, `BackfillComplete`,
    /// `ScrollBarsLoaded`) were interleaving a 2000-bar kline timeline with
    /// the ~N footprint-derived bars, desyncing the index axis (blank
    /// clusters, grid drift, phantom X labels). The trade-agg-bar family
    /// (range/volume/tick/dollar/imbalance/run bars) has the same shape of
    /// bug (2026-07-08): they stream derived bars into the SAME `bars`
    /// array while `timeframe` stays a stale wall-clock TF, so they need
    /// their own `TradeAggBars` ownership marker. Every kline-family
    /// writer MUST check this flag before touching `bars`; `FootprintUpdate`
    /// and the trade-agg station pipeline write only when they hold
    /// ownership.
    pub bars_source: BarsSource,

    /// Stale-array tripwire (K1 lifecycle groundwork, chart-type-engine-
    /// consolidation B2). Bumped by every [`Self::replace_bars`]/
    /// [`Self::clear_bars`] call — the install/subscribe paths
    /// (`subscribe_fold_chart_type`/`subscribe_trade_fold_chart_type`/the
    /// plain kline path) record the generation they installed, and
    /// `debug_assert_generation` (render.rs) dlog-warns when the render
    /// snapshot's active generation disagrees with this stamp. Diagnostic
    /// only in this brick — never read by any non-debug code path.
    pub bars_generation: mlc_core::chart_type::BarsGeneration,

    /// Footprint cluster bars. Written by `FootprintUpdate`.
    pub footprint_bars: Option<Vec<mlc_core::types::FootprintBar>>,

    /// Renko bricks. Written by `RenkoBarUpdate`.
    pub renko_bars: Option<Vec<mlc_core::types::RenkoBrick>>,

    /// Three Line Break lines. Written by `ThreeLineBreakUpdate`.
    pub three_line_break: Option<Vec<mlc_core::types::ThreeLineBreakLine>>,

    /// Point-and-Figure columns. Written by `PnfBarUpdate`.
    pub pnf_columns: Option<Vec<mlc_core::types::PnfColumn>>,

    /// Kagi segments. Written by `KagiBarUpdate`.
    pub kagi_segments: Option<Vec<mlc_core::types::KagiSegment>>,

    /// Cumulative Volume Delta samples. Written by the client-side CVD
    /// companion fold (`active_cvd_fold`, unified-data-plane Stage Э4/Э5).
    pub cvd_line: Option<Vec<mlc_core::types::CvdSample>>,

    /// TPO Market Profile session snapshots. Written by `TpoProfileUpdate`.
    pub tpo_sessions: Option<Vec<mlc_core::types::TpoSession>>,

    /// Open Interest Δ bars. Written by `OpenInterestUpdate`.
    pub oi_delta: Option<Vec<mlc_core::types::OiDeltaBar>>,

    /// Open-interest HISTORY per venue, `venue -> [(ts_ms, oi)]` ascending.
    /// The projected-liquidation overlay's input, kept per venue so the
    /// same rows serve both its modes: solo reads the chart's own venue,
    /// aggregated folds every venue that answered.
    ///
    /// Distinct from `oi_delta`, which is the OI Delta sub-pane's live WS
    /// series — that stream does not exist on Binance/KuCoin/HTX at all.
    pub oi_hist: std::collections::BTreeMap<String, Vec<(i64, f64)>>,

    /// Projected liquidation levels, modelled from `oi_delta` + bars by
    /// `ChartApp::refresh_liq_projection`. CACHED rather than derived at
    /// render time: the consumption scan is O(rows × bars) and would run
    /// every frame for a picture that only changes when a bar closes or
    /// open interest moves.
    pub liq_projection: Option<Vec<mlc_core::chart::liquidation_projection::ProjectedLevel>>,
    /// Signature of the inputs `liq_projection` was built from
    /// (bars, oi points, params fingerprint) — recompute exactly when it
    /// changes.
    pub liq_projection_key: (usize, usize, u64),

    /// Funding Rate cells. Written by `FundingRateUpdate`.
    pub funding_rate: Option<Vec<mlc_core::types::FundingRateCell>>,

    /// DOM heatmap ring buffer. Written by `DomHeatmapSnapshot`.
    pub dom_heatmap: Option<mlc_core::chart::dom_heatmap::DomHeatmapBuffer>,

    /// DOM trade-circle ring overlay. Written by `DomTradeDot`.
    pub dom_trade_dots: Option<Vec<mlc_core::chart::dom_heatmap::DomTradeDot>>,

    /// Realized-liquidation tape — prints at their real (ts, price), folded
    /// into levels at render time. Filled by the app's per-frame pull over
    /// the shared station rings, one cursor per venue, so an aggregated
    /// overlay and the LIQ panel read the same prints from the same source.
    pub liq_tape: Option<mlc_core::chart::liquidations::LiquidationTape>,

    // =========================================================================
    // === Alert storage (Step 3.0 doctrine cutover) ===========================
    // =========================================================================
    // Alerts are bubble data: persisted user-authored rows + the per-bubble
    // identity allocator.  The tick-to-tick last-price cache used for
    // crossing detection is manager state — it lives on
    // `AlertManager.last_price`, keyed by `BubbleId` (see
    // `crates/mlc-backend/src/alert/manager.rs`).
    // =========================================================================

    /// Alerts attached to this bubble.  Each alert targets data inside this
    /// bubble (price levels, drawings, indicators).  Persisted with the bubble.
    pub alerts: Vec<mlc_core::alert::AlertItem>,

    /// Per-bubble next-id counter for alerts.
    pub alert_next_id: u64,

    // =========================================================================
    // === In-flight guards (F4/F5, viewport doctrine audit 2026-07-07) =======
    // =========================================================================
    // Moved here from per-WINDOW fields on `ChartContainer`: the resource
    // being guarded (a scroll-left backfill / derived reseed fetch) is a
    // per-BUBBLE fetch, not a per-window one — two windows sharing one
    // bubble each firing their own backfill for the same bubble was the
    // race these flags previously failed to prevent.
    // =========================================================================

    /// `true` while an async scroll-left (historical bar extension) fetch is
    /// in flight for this bubble.
    ///
    /// Guards against issuing multiple concurrent scroll-fetch requests for
    /// the same bubble. Reset to `false` when `ScrollBarsAppended` arrives
    /// (or on error/timeout). Runtime-only state — not persisted to disk.
    pub scroll_fetch_in_flight: bool,

    /// When the current scroll fetch was started (for timeout reset).
    ///
    /// If `ScrollBarsAppended` is dropped (channel lag), the 10-second
    /// timeout resets `scroll_fetch_in_flight` so the bubble can fetch
    /// again. Runtime-only state — not persisted to disk.
    pub scroll_fetch_started: Option<web_time::Instant>,

    /// `true` once the ONE lazy follow-up prefetch has been scheduled for
    /// the current left-edge crossing (derived-prepend-fold-plan-2026-07-07,
    /// Task 2: "one batch now, one batch lazy"). Set when
    /// `BubbleEvent::ScrollBarsAppended` fires a background follow-up
    /// `request_scroll_backfill` for the SAME bubble right after a
    /// scroll-triggered page lands; cleared on the NEXT user-triggered
    /// crossing (`tick.rs`'s threshold check, when it arms
    /// `scroll_fetch_in_flight` for a fresh request) so each crossing gets
    /// exactly one prefetch pair, never an unbounded chain. Runtime-only
    /// state — not persisted to disk.
    pub scroll_prefetch_done: bool,

    /// How much real history backs the active chart_type's series right now.
    /// See [`HistoryDepth`]. Reset to `Unknown` on every chart-type switch
    /// (`clear_slot_*`); populated by the first
    /// `LiveUpdate::HistoryDepthReported` for the new active type.
    pub history_depth: HistoryDepth,

    // =========================================================================
    // === Raw data layers (unified-data-plane Stage Э1, 2026-07-09) ==========
    // =========================================================================
    // Bubble-scoped raw storage, independent of the active render series:
    // one kline ring per touched timeframe. Filled continuously by live WS
    // ticks and REST backfill; nothing clears them on chart-type/param/TF
    // switches — only ring capacity evicts (drop-oldest). ONLY a genuine
    // instrument reassignment (symbol switch) resets them — see
    // `BubbleStore::set_instrument`/`set_bid_instrument_full`.
    // See `docs/mlc/plans/unified-data-plane-2026-07-09.md`.
    //
    // The raw TRADE ring moved out of here (trade-data-plane-2026-07-13.md
    // §1.1, step 1) into `crate::trade_store::TradeStore` — one shared ring
    // per INSTRUMENT instead of one per bubble tracking that instrument.
    // =========================================================================

    /// Raw kline rings keyed by timeframe name (e.g. `"1m"`, `"1h"`) — one
    /// per timeframe any window on this bubble has touched. Written by the
    /// kline-family `LiveUpdate` writers (`BarsLoaded`/`BackfillComplete`/
    /// `ScrollBarsLoaded`/`BarUpdate`) REGARDLESS of `bars_source` — this is
    /// raw storage, not the active display series.
    pub kline_layers: HashMap<String, RawLayer<mlc_core::Bar>>,

    /// Ring capacity applied to every entry of `kline_layers` (new TF layers
    /// created lazily use this value). Defaults to
    /// [`DEFAULT_KLINE_LAYER_CAPACITY`]; overridden via
    /// [`Self::set_layer_capacities`] from
    /// `UserProfile.domain.data_load.kline_layer_capacity`.
    pub kline_layer_capacity: usize,

    // =========================================================================
    // === Client-side view fold (unified-data-plane Stage Э3, 2026-07-09) ====
    // =========================================================================

    /// Installed when this bubble's active chart_type is one of the 7
    /// kline-sufficient fold types (range_bar/volume_bar/dollar_bar/renko/
    /// pnf/kagi/three_line_break) — `None` otherwise. See
    /// `crate::bubble::view_fold` module doc. Not carried across `Clone`
    /// (fold machines are stateful, non-`Clone` accumulators with no
    /// rollback support — a cloned bubble re-installs its own runtime on
    /// the next subscribe rather than sharing fold state).
    pub active_fold: Option<super::view_fold::FoldRuntime>,

    /// Installed when this bubble's active chart_type is one of the trade-
    /// fed fold types (tick_bar/tick_imbalance_bar/volume_imbalance_bar/
    /// run_bar/footprint/tpo) — unified-data-plane Stage Э4. `None`
    /// otherwise. Not carried across `Clone` — same stateful-accumulator
    /// rationale as `active_fold`.
    pub active_trade_fold: Option<super::view_fold::TradeFoldRuntime>,

    /// Installed when the CVD companion indicator is live for this bubble's
    /// instrument (unified-data-plane Stage Э4). Independent of
    /// `active_trade_fold`/`active_fold` — CVD is an indicator overlay, not
    /// a chart_type, so it can be installed here alongside EITHER a
    /// kline-fold OR a trade-fold active chart_type (a footprint window
    /// with a CVD companion instance both wants `active_trade_fold`'s
    /// `"footprint"` machine AND this `"cvd"` machine at once — sharing one
    /// slot between them would make the two fight over it). Not carried
    /// across `Clone` — same stateful-accumulator rationale as
    /// `active_fold`.
    pub active_cvd_fold: Option<super::view_fold::TradeFoldRuntime>,

    /// Installed when the bubble's ACTIVE timeframe is TRADE-BUILT (owner
    /// report 2026-07-16 — no native kline source and no native divisor
    /// exists for it on the active venue; see
    /// `mlc_backend_data_bridge::is_trade_built`). `None` otherwise —
    /// including while the active TF is native or client-side-aggregated
    /// from a native divisor (that family keeps riding the ordinary kline
    /// path unchanged). Folds the shared per-instrument `TradeStore` into
    /// `kline_layers[tf_name]` (+ `bubble.bars` when display-writable) —
    /// see `crate::bubble::trade_built_kline` module doc. Not carried
    /// across `Clone` — same stateful-accumulator rationale as
    /// `active_fold`.
    pub active_trade_built_kline: Option<super::trade_built_kline::TradeBuiltKlineRuntime>,

    // =========================================================================
    // === Refold debounce (P0 hang fix part 2, chart-series arc 2026-07-10) ==
    // =========================================================================
    // `LiveUpdate::LayerBackfilled` streams roughly one page/second while a
    // ramp is in flight; a full fold-machine reseed is heavy for deep layers
    // (renko/TPO). Without a debounce, every page saturates the main thread
    // ("страница не отвечает"). See `BackendCore::maybe_refold_*` in
    // `chart_type/lifecycle.rs`.
    // =========================================================================

    /// Wall-clock time of this bubble's last COMPLETED refold (any of
    /// `active_fold`/`active_trade_fold`/`active_cvd_fold`), independent per
    /// bubble. `None` before the first refold — the first `LayerBackfilled`
    /// after install always refolds immediately regardless of interval (cold
    /// responsiveness), which naturally follows from `None` never satisfying
    /// the "already refolded recently" debounce check. Runtime-only state —
    /// not persisted to disk, not carried across `Clone` (mirrors
    /// `active_fold`'s non-`Clone` rationale: a cloned bubble starts fresh).
    pub last_refold_at: Option<web_time::Instant>,

    /// Set when a `LayerBackfilled` page arrived within the debounce window
    /// of the last refold and was SKIPPED — the layer write already
    /// happened (via the `ScrollBarsLoaded`/`BackfillComplete` writers), only
    /// the fold-machine reseed was deferred. The next `LayerBackfilled` past
    /// the interval (or the terminal `exhausted: true` page, which ALWAYS
    /// refolds if this flag is set, guaranteeing the layer's final state is
    /// never left stale) performs the deferred refold and clears this flag.
    /// Runtime-only state — not persisted, not carried across `Clone`.
    pub refold_dirty: bool,
}


impl DataBubble {
    /// Reconstruct a [`DataBubble`] from a [`mlc_core::preset::snapshots::DataBubbleSnapshot`].
    ///
    /// The resulting bubble starts with an empty `primitives` list — primitives
    /// are restored separately via the `PrimitiveRegistry` and pushed in by the
    /// caller ([`crate::bubble::BubbleStore::restore_from_snapshots`]).
    ///
    /// Tag fields on the snapshot (`color`, `auto_created`, `members`,
    /// `panel_members`, `member_overrides`) are NOT read here — the paired
    /// `Tag` is reconstructed separately by
    /// `crate::tag::TagManager::restore_from_snapshots` from the same
    /// snapshot list (Step 6 doctrine cutover).
    pub fn from_snapshot(snap: &mlc_core::preset::snapshots::DataBubbleSnapshot) -> Self {
        let mut style_map = HashMap::new();
        for (type_id, raw) in &snap.last_used_style {
            if let Ok(style) = serde_json::from_value::<TemplateStyle>(raw.clone()) {
                style_map.insert(type_id.clone(), style);
            }
        }

        Self {
            id: BubbleId(snap.id),
            primitives: Vec::new(),
            indicator_configs: snap.indicator_configs.clone(),
            address: InstrumentAddress::new(
                snap.exchange.clone(),
                snap.symbol.clone(),
                snap.account_type.parse().unwrap_or_default(),
            ),
            timeframe: snap.timeframe.clone(),
            chart_type: "candles",
            chart_type_param: 0.0,
            chart_type_param_mode: mlc_core::chart_type::ParamMode::Fixed,
            chart_type_interval: "1m",
            sync_flags: snap.sync_flags.clone(),
            scale_mode: snap.scale_mode,
            command_history: CommandHistory::new(250),
            pre_tag_indicator_ids: Vec::new(),
            last_used_style: Arc::new(RwLock::new(style_map)),
            prev_close_price: None,
            connection_status: ConnectionStatus::default(),
            bars: Vec::new(),
            bars_source: BarsSource::default(),
            bars_generation: mlc_core::chart_type::BarsGeneration::NONE,
            footprint_bars: None,
            renko_bars: None,
            three_line_break: None,
            pnf_columns: None,
            kagi_segments: None,
            cvd_line: None,
            tpo_sessions: None,
            oi_delta: None,
            oi_hist: std::collections::BTreeMap::new(),
            liq_projection: None,
            liq_projection_key: (0, 0, 0),
            funding_rate: None,
            dom_heatmap: None,
            dom_trade_dots: None,
            liq_tape: None,
            alerts: snap.alerts.clone(),
            alert_next_id: snap.alerts.iter().map(|a| a.id).max().unwrap_or(0) + 1,
            scroll_fetch_in_flight: false,
            scroll_fetch_started: None,
            scroll_prefetch_done: false,
            history_depth: HistoryDepth::default(),
            kline_layers: HashMap::new(),
            kline_layer_capacity: DEFAULT_KLINE_LAYER_CAPACITY,
            active_fold: None,
            active_trade_fold: None,
            active_cvd_fold: None,
            active_trade_built_kline: None,
            last_refold_at: None,
            refold_dirty: false,
        }
    }

    // =========================================================================
    // === Per-Kind storage mutators (Phase X0) ================================
    // =========================================================================
    // Mutate ONLY the storage field. No viewport calls, no derived recompute,
    // no callbacks. Consumers pull state and run their own reconcile.
    // =========================================================================

    /// Replace `bars` wholesale and bump [`Self::bars_generation`] to a
    /// fresh stamp for the bubble's CURRENT `chart_type`/`chart_type_param`
    /// — every full reseed/refold ends in a `replace_bars` call, so this is
    /// the one choke point that needs to record "the array now belongs to
    /// generation N" (see [`mlc_core::chart_type::BarsGeneration`]'s doc
    /// comment).
    pub fn replace_bars(&mut self, bars: Vec<mlc_core::Bar>) {
        self.bars = bars;
        self.bump_bars_generation();
    }
    pub fn append_bar(&mut self, bar: mlc_core::Bar) { self.bars.push(bar); }
    pub fn last_bar_mut(&mut self) -> Option<&mut mlc_core::Bar> { self.bars.last_mut() }
    /// Clear `bars` and bump [`Self::bars_generation`] — a chart-type
    /// switch's `clear_slot_*` always precedes the NEXT type's install, so
    /// the stale generation left over from the OLD type must not survive
    /// to be mistaken for the new one.
    pub fn clear_bars(&mut self) {
        self.bars.clear();
        self.bump_bars_generation();
    }

    /// Stamp a fresh [`mlc_core::chart_type::BarsGeneration`] for the
    /// bubble's current `chart_type`/`chart_type_param`. `param_bits` is
    /// the param's raw bit pattern (`f64::to_bits`) — a diagnostic
    /// fingerprint, not a replay key, so lossy reinterpretation is fine.
    fn bump_bars_generation(&mut self) {
        self.bars_generation = self.bars_generation.bump(self.chart_type, self.chart_type_param.to_bits());
    }

    pub fn append_footprint(&mut self, bar: mlc_core::types::FootprintBar) {
        self.footprint_bars.get_or_insert_with(Vec::new).push(bar);
    }
    pub fn last_footprint_mut(&mut self) -> Option<&mut mlc_core::types::FootprintBar> {
        self.footprint_bars.as_mut().and_then(|v| v.last_mut())
    }
    pub fn clear_footprint(&mut self) { self.footprint_bars = None; }
    /// Mark the footprint slot as "subscribed, seed in flight": `Some(empty)`
    /// instead of `None`. `ChartRenderState::has_any_series_data` treats
    /// `Some(_)` as a renderable series, so the pane draws its chrome + the
    /// in-pane waiting spinner instead of the bare loading skeleton (owner
    /// report 2026-07-05 #3: switching to footprint showed a fully blank
    /// pane for the whole seed).
    pub fn mark_footprint_pending(&mut self) {
        if self.footprint_bars.is_none() {
            self.footprint_bars = Some(Vec::new());
        }
    }

    /// Sorted upsert by `open_time`. Keeps `renko_bars` strictly
    /// ascending — `ChartContainer::reconcile_derived_series` binary-
    /// searches this array to detect prepended history, so an
    /// out-of-order arrival (chart-type re-entry, WS reconnect replay)
    /// must never silently break the sort invariant via a plain push.
    pub fn append_renko(&mut self, brick: mlc_core::types::RenkoBrick) {
        let store = self.renko_bars.get_or_insert_with(Vec::new);
        let ts = brick.open_time;
        match store.last() {
            None => store.push(brick),
            Some(last) if last.open_time == ts => {
                let idx = store.len() - 1;
                store[idx] = brick;
            }
            Some(last) if last.open_time < ts => store.push(brick),
            Some(_) => match store.binary_search_by_key(&ts, |b| b.open_time) {
                Ok(pos) => store[pos] = brick,
                Err(pos) => store.insert(pos, brick),
            },
        }
    }
    pub fn clear_renko(&mut self) { self.renko_bars = None; }

    /// Sorted upsert by `ts_open`. See `append_renko` for why this must
    /// not regress to a plain push.
    pub fn append_three_line_break(&mut self, line: mlc_core::types::ThreeLineBreakLine) {
        let store = self.three_line_break.get_or_insert_with(Vec::new);
        let ts = line.ts_open;
        match store.last() {
            None => store.push(line),
            Some(last) if last.ts_open == ts => {
                let idx = store.len() - 1;
                store[idx] = line;
            }
            Some(last) if last.ts_open < ts => store.push(line),
            Some(_) => match store.binary_search_by_key(&ts, |l| l.ts_open) {
                Ok(pos) => store[pos] = line,
                Err(pos) => store.insert(pos, line),
            },
        }
    }
    pub fn clear_three_line_break(&mut self) { self.three_line_break = None; }

    /// Sorted upsert by `open_time`. See `append_renko` for why this must
    /// not regress to a plain push. Also the single source of truth for
    /// PnF column merge — `LiveUpdate::PnfBarUpdate` no longer bypasses
    /// this with its own `column_id`-based tail check.
    pub fn append_pnf(&mut self, col: mlc_core::types::PnfColumn) {
        let store = self.pnf_columns.get_or_insert_with(Vec::new);
        let ts = col.open_time;
        match store.last() {
            None => store.push(col),
            Some(last) if last.open_time == ts => {
                let idx = store.len() - 1;
                store[idx] = col;
            }
            Some(last) if last.open_time < ts => store.push(col),
            Some(_) => match store.binary_search_by_key(&ts, |c| c.open_time) {
                Ok(pos) => store[pos] = col,
                Err(pos) => store.insert(pos, col),
            },
        }
    }
    pub fn last_pnf_mut(&mut self) -> Option<&mut mlc_core::types::PnfColumn> {
        self.pnf_columns.as_mut().and_then(|v| v.last_mut())
    }
    pub fn clear_pnf(&mut self) { self.pnf_columns = None; }

    /// Sorted upsert by `open_time`. See `append_renko` for why this must
    /// not regress to a plain push.
    pub fn append_kagi(&mut self, seg: mlc_core::types::KagiSegment) {
        let store = self.kagi_segments.get_or_insert_with(Vec::new);
        let ts = seg.open_time;
        match store.last() {
            None => store.push(seg),
            Some(last) if last.open_time == ts => {
                let idx = store.len() - 1;
                store[idx] = seg;
            }
            Some(last) if last.open_time < ts => store.push(seg),
            Some(_) => match store.binary_search_by_key(&ts, |s| s.open_time) {
                Ok(pos) => store[pos] = seg,
                Err(pos) => store.insert(pos, seg),
            },
        }
    }
    pub fn clear_kagi(&mut self) { self.kagi_segments = None; }

    /// Append/upsert one CVD sample. Samples are TIME-BUCKETED (owner
    /// doctrine 2026-07-13: as many CVD samples as bars — the per-trade
    /// unbucketed line grew by hundreds of thousands of points during a
    /// backfill and made every per-frame indicator recalc walk all of
    /// them). A sample landing on the SAME bucket ts as the current last
    /// replaces it (the bucket's running total updating live, exactly like
    /// a forming bar); a newer ts appends.
    pub fn append_cvd(&mut self, sample: mlc_core::types::CvdSample) {
        let store = self.cvd_line.get_or_insert_with(Vec::new);
        match store.last_mut() {
            Some(last) if last.ts_ms == sample.ts_ms => *last = sample,
            _ => store.push(sample),
        }
    }
    pub fn clear_cvd(&mut self) { self.cvd_line = None; }

    pub fn append_tpo(&mut self, sess: mlc_core::types::TpoSession) {
        self.tpo_sessions.get_or_insert_with(Vec::new).push(sess);
    }
    pub fn last_tpo_mut(&mut self) -> Option<&mut mlc_core::types::TpoSession> {
        self.tpo_sessions.as_mut().and_then(|v| v.last_mut())
    }
    pub fn clear_tpo(&mut self) { self.tpo_sessions = None; }

    pub fn append_oi(&mut self, point: mlc_core::types::OiDeltaBar) {
        self.oi_delta.get_or_insert_with(Vec::new).push(point);
    }
    pub fn clear_oi(&mut self) { self.oi_delta = None; }

    pub fn append_funding(&mut self, point: mlc_core::types::FundingRateCell) {
        self.funding_rate.get_or_insert_with(Vec::new).push(point);
    }
    pub fn clear_funding(&mut self) { self.funding_rate = None; }

    pub fn dom_heatmap_mut(&mut self) -> &mut Option<mlc_core::chart::dom_heatmap::DomHeatmapBuffer> {
        &mut self.dom_heatmap
    }
    pub fn clear_dom_heatmap(&mut self) { self.dom_heatmap = None; }

    pub fn append_dom_trade_dot(&mut self, dot: mlc_core::chart::dom_heatmap::DomTradeDot) {
        self.dom_trade_dots.get_or_insert_with(Vec::new).push(dot);
    }
    pub fn dom_trade_dots_mut(&mut self) -> Option<&mut Vec<mlc_core::chart::dom_heatmap::DomTradeDot>> {
        self.dom_trade_dots.as_mut()
    }
    pub fn clear_dom_trade_dots(&mut self) { self.dom_trade_dots = None; }

    pub fn liq_tape_mut(&mut self) -> &mut Option<mlc_core::chart::liquidations::LiquidationTape> {
        &mut self.liq_tape
    }
    pub fn clear_liq_tape(&mut self) { self.liq_tape = None; }

    /// Clear every per-kind derived-series slot except the one belonging to
    /// `keep` (a chart_type string, or `None` to clear all of them).
    ///
    /// Audit D9: chart-type switch used to clear NOTHING — the sibling
    /// slots (renko/pnf/kagi/tlb/tpo/heatmaps) kept whatever stale data
    /// a PRIOR chart_type left behind (e.g. renko box=10 bricks still in
    /// `renko_bars` after switching to box=20 — the re-subscribe emits fresh
    /// bricks, but the stale ones from the old box size were never dropped,
    /// so the pane briefly/permanently mixes both geometries).
    ///
    /// `ChangeChartType` calls this with `keep = None` (2026-07-06 fix —
    /// passing `Some(new_type)` there used to skip clearing the TARGET
    /// type's own slot, so re-entering a previously-visited derived type on
    /// the same bubble appended a fresh re-seed AFTER stale leftover data,
    /// breaking the ascending-timestamp invariant `reconcile_derived_series`
    /// relies on and stranding the viewport mid-history). `UpdateChartTypeParam`
    /// (same-type param refresh, e.g. renko box=10 -> box=20) also calls this
    /// with `keep = None` — both call sites always follow with a full fresh
    /// re-subscribe, so there is currently no caller that legitimately wants
    /// to preserve the target's own slot; `keep` stays as a general escape
    /// hatch for a future caller that reconciles in place instead of
    /// resubscribing.
    ///
    /// `bars` (kline) and `footprint_bars` are NEVER cleared here — kline
    /// data backs candle-family AND is the backdrop footprint depends on;
    /// footprint has its own re-subscribe-driven replacement via
    /// `append_footprint`.
    ///
    /// `cvd` is deliberately NOT in this list (2026-07-04 taxonomy fix): CVD
    /// was demoted from a chart_type to a real compute-catalog indicator
    /// instance, independent of `chart_type` (like `oi_delta`/
    /// `funding_rate`, which were never in this list either) — its
    /// lifecycle (both `cvd_line` and, since unified-data-plane Stage Э4,
    /// `active_cvd_fold`) is owned by `clear_cvd()` /
    /// `ChartApp::unsubscribe_cvd_line_if_last`, called when the last CVD
    /// instance for the bubble is removed.
    ///
    /// `dom_heatmap`/`liq_tape` are ALSO deliberately NOT in this list
    /// (chart-type → overlay migration, mirrors the CVD/OI/Funding
    /// precedent above) — their lifecycle is independent of `chart_type`,
    /// owned by `ChartOutEvent::Toggle{Dom,Liquidation}HeatmapOverlay`'s
    /// handler via `mlc_backend::chart_type::clear_slot_{dom,liq}_heatmap`,
    /// called when the overlay is toggled off (not on chart-type switch).
    pub fn clear_derived_slots(&mut self, keep: Option<&str>) {
        // Unified-data-plane Э3/Э4: both fold runtimes are stateful
        // accumulators tied to the OLD chart_type/params — every caller of
        // this method (`ChangeChartType`, `UpdateChartTypeParam`,
        // `ignite_derived_windows`) always follows with a fresh
        // `subscribe_fold_chart_type` call for the NEW target when it's one
        // of the fold types, which reinstalls a fresh runtime. Dropping them
        // unconditionally here (even when `keep` names a fold type — a
        // param change on the SAME type still needs a brand-new machine
        // instance, see `FoldRuntime::reseed`'s doc comment on why fold
        // state has no in-place update) mirrors the "always a fresh
        // re-subscribe follows" invariant already documented above for the
        // other derived slots. `active_cvd_fold` is untouched here — its
        // lifecycle is independent of chart_type (see doc comment above).
        self.active_fold = None;
        self.active_trade_fold = None;
        // TRADE-BUILT kline runtime (owner report 2026-07-16): serves ONLY
        // the render-only kline family (candles/bars/line/...) on its
        // installed TF — a switch AWAY from that family (this method's
        // caller, e.g. kline -> footprint/renko) has no further use for it.
        // A render-only-to-render-only switch (candles -> line) never calls
        // this method (`render_only_switch` short-circuits it at the call
        // site) so the runtime survives exactly the cases where it's still
        // needed. `abort_kline_bars` is the OTHER teardown path (TF switch
        // away from the trade-built TF while staying in the kline family).
        self.active_trade_built_kline = None;
        if keep != Some("renko") { self.clear_renko(); }
        if keep != Some("pnf") { self.clear_pnf(); }
        if keep != Some("kagi") { self.clear_kagi(); }
        if keep != Some("three_line_break") { self.clear_three_line_break(); }
        if keep != Some("tpo") { self.clear_tpo(); }
    }

    /// Record a seed/deepen depth report for `chart_type`. Returns `true`
    /// when the report was applied.
    ///
    /// `history_depth` is a single field, not per-chart_type storage — a
    /// report for any chart_type OTHER than the one currently active on
    /// this bubble is a stale event from a just-switched-away subscribe
    /// racing the switch (same discipline as the `bars_source` ownership
    /// guards elsewhere) and is silently ignored (`false`).
    pub fn set_history_depth(&mut self, chart_type: &str, depth: HistoryDepth) -> bool {
        if self.chart_type != chart_type {
            return false;
        }
        self.history_depth = depth;
        true
    }

    // =========================================================================
    // === Raw data layers (unified-data-plane Stage Э1) =======================
    // =========================================================================

    /// Configure the ring capacity used by this bubble's raw kline layers.
    /// Applies immediately to every already-existing `kline_layers` entry;
    /// new TF layers created afterward via [`Self::kline_layer_mut`] pick up
    /// `kline_layer_capacity` at creation time. Called once from the app
    /// layer right after bubble creation, sourced from
    /// `UserProfile.domain.data_load.kline_layer_capacity`.
    ///
    /// Trade-layer capacity is no longer a per-bubble setting — see
    /// [`crate::trade_store::TradeStore::set_default_capacity`] (the shared
    /// per-instrument store's store-wide equivalent, trade-data-plane-2026-07-13.md
    /// §1.1 step 1).
    pub fn set_layer_capacities(&mut self, kline_capacity: usize) {
        self.kline_layer_capacity = kline_capacity;
        for layer in self.kline_layers.values_mut() {
            layer.set_capacity(kline_capacity);
        }
    }

    /// Get-or-create the raw kline layer for timeframe `tf`, using
    /// `kline_layer_capacity` for a freshly-created layer.
    pub fn kline_layer_mut(&mut self, tf: &str) -> &mut RawLayer<mlc_core::Bar> {
        let cap = self.kline_layer_capacity;
        self.kline_layers
            .entry(tf.to_string())
            .or_insert_with(|| RawLayer::new(cap))
    }

    /// Read-only access to the raw kline layer for timeframe `tf`, if any
    /// data has been written for it yet.
    pub fn kline_layer(&self, tf: &str) -> Option<&RawLayer<mlc_core::Bar>> {
        self.kline_layers.get(tf)
    }

    /// Seed the raw kline layer for timeframe `tf` with a fresh Phase-A
    /// block — see [`RawLayer::seed_block`]. Get-or-creates the layer like
    /// [`Self::kline_layer_mut`]. Returns the count of points from `block`
    /// that were actually new (replaces of an already-stored key don't
    /// count).
    pub fn kline_layer_seed_block(&mut self, tf: &str, block: Vec<mlc_core::Bar>) -> usize {
        self.kline_layer_mut(tf).seed_block(block)
    }

    /// Drop every raw kline layer (all touched-timeframe rings). Called
    /// ONLY when this bubble is reassigned to a different instrument
    /// (symbol switch) — see `BubbleStore::set_instrument` family. Chart-
    /// type/param/TF switches on the SAME instrument must never call this.
    ///
    /// Does NOT touch the shared [`crate::trade_store::TradeStore`] — that
    /// store is per-INSTRUMENT, not per-bubble, so a bubble's own identity
    /// reassignment has no bearing on it (see `TradeStore`'s module doc,
    /// "Instrument-switch semantics").
    pub fn reset_raw_layers(&mut self) {
        self.kline_layers.clear();
        // TRADE-BUILT kline runtime is INSTRUMENT-scoped (its `last_bars`
        // are the old symbol's series and its store key is the old
        // instrument) — surviving a symbol switch made `request_kline`'s
        // `already_active` guard skip the display-clearing empty
        // full-replace, and the stale series leaked into the new symbol's
        // chart (owner artifact 2026-07-17: BTCDOMUSDT ~5k bars mixed into
        // binance F BTCUSDT). The ApplySymbol re-ignition always follows
        // with a fresh `subscribe_trade_built_kline` when the TF is
        // trade-built on the new instrument.
        self.active_trade_built_kline = None;
        // The display array is equally instrument-scoped: for native TFs
        // the follow-up Phase A full-replace overwrites it anyway, but for
        // trade-built TFs the fresh seed can be TINY (cold store) and every
        // BarsLoaded consumer sees `prev_bars` — leaving the old symbol's
        // bars here lets an anchor/merge path resurrect them wholesale.
        self.bars.clear();
    }

    // =========================================================================
    // === Refold debounce (P0 hang fix part 2) ================================
    // =========================================================================

    /// Decide whether a `LayerBackfilled` page arriving right now should
    /// perform a refold immediately, mark the bubble dirty and skip, or
    /// (terminal page) always refold when dirty. See
    /// [`Self::last_refold_at`]/[`Self::refold_dirty`]'s doc comments for the
    /// full contract; [`REFOLD_MIN_INTERVAL_MS`] is the debounce window.
    ///
    /// `terminal` is `true` for the exhaustion page (`exhausted: true` on
    /// `mlc_backend_data_bridge::LiveUpdate::LayerBackfilled` — the type
    /// lives in a downstream crate this module doesn't depend on, so it's
    /// referenced by name only, not linked) — the GUARANTEE clause: the last
    /// page always ends with a refold if the bubble is dirty, even inside
    /// the debounce window, so a fold's history is never left stale after
    /// backfill stops.
    ///
    /// Returns `true` when the caller should run the (expensive) reseed now;
    /// on `true` the caller is responsible for calling
    /// [`Self::mark_refolded`] once the reseed actually completes (not done
    /// here — this method only decides, it never assumes the reseed
    /// succeeds).
    pub fn should_refold_now(&mut self, terminal: bool, now: web_time::Instant) -> bool {
        let within_debounce = self.last_refold_at
            .is_some_and(|last| now.saturating_duration_since(last).as_millis() < REFOLD_MIN_INTERVAL_MS as u128);
        if !within_debounce {
            return true;
        }
        if terminal && self.refold_dirty {
            return true;
        }
        // Debounced: record that a refold is owed without doing the work.
        self.refold_dirty = true;
        false
    }

    /// Record that a refold just completed at `now` — clears
    /// [`Self::refold_dirty`] and stamps [`Self::last_refold_at`]. Call this
    /// ONLY after [`Self::should_refold_now`] returned `true` and the reseed
    /// actually ran.
    pub fn mark_refolded(&mut self, now: web_time::Instant) {
        self.last_refold_at = Some(now);
        self.refold_dirty = false;
    }
}

impl Clone for DataBubble {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            primitives: self.primitives.iter().map(|p| p.clone_box()).collect(),
            indicator_configs: self.indicator_configs.clone(),
            address: self.address.clone(),
            timeframe: self.timeframe.clone(),
            chart_type: self.chart_type,
            chart_type_param: self.chart_type_param,
            chart_type_param_mode: self.chart_type_param_mode,
            chart_type_interval: self.chart_type_interval,
            sync_flags: self.sync_flags.clone(),
            scale_mode: self.scale_mode,
            command_history: CommandHistory::new(250),
            pre_tag_indicator_ids: self.pre_tag_indicator_ids.clone(),
            // Clone the Arc — both copies point to the same shared store.
            last_used_style: Arc::clone(&self.last_used_style),
            prev_close_price: self.prev_close_price,
            connection_status: self.connection_status,
            // === Per-Kind storage (Phase X0) ===
            bars: self.bars.clone(),
            bars_source: self.bars_source,
            bars_generation: self.bars_generation,
            footprint_bars: self.footprint_bars.clone(),
            renko_bars: self.renko_bars.clone(),
            three_line_break: self.three_line_break.clone(),
            pnf_columns: self.pnf_columns.clone(),
            kagi_segments: self.kagi_segments.clone(),
            cvd_line: self.cvd_line.clone(),
            tpo_sessions: self.tpo_sessions.clone(),
            oi_delta: self.oi_delta.clone(),
            oi_hist: self.oi_hist.clone(),
            liq_projection: self.liq_projection.clone(),
            liq_projection_key: self.liq_projection_key,
            funding_rate: self.funding_rate.clone(),
            dom_heatmap: self.dom_heatmap.clone(),
            dom_trade_dots: self.dom_trade_dots.clone(),
            liq_tape: self.liq_tape.clone(),
            alerts: self.alerts.clone(),
            alert_next_id: self.alert_next_id,
            scroll_fetch_in_flight: self.scroll_fetch_in_flight,
            scroll_fetch_started: self.scroll_fetch_started,
            scroll_prefetch_done: self.scroll_prefetch_done,
            history_depth: self.history_depth,
            kline_layers: self.kline_layers.clone(),
            kline_layer_capacity: self.kline_layer_capacity,
            // Not carried across Clone — see field doc comment.
            active_fold: None,
            active_trade_fold: None,
            active_cvd_fold: None,
            active_trade_built_kline: None,
            last_refold_at: None,
            refold_dirty: false,
        }
    }
}

impl std::fmt::Debug for DataBubble {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataBubble")
            .field("id", &self.id)
            .field("primitives_count", &self.primitives.len())
            .field("indicator_configs", &self.indicator_configs)
            .field("address", &self.address)
            .field("timeframe", &self.timeframe)
            .field("chart_type", &self.chart_type)
            .field("chart_type_param", &self.chart_type_param)
            .field("chart_type_param_mode", &self.chart_type_param_mode)
            .field("chart_type_interval", &self.chart_type_interval)
            .field("sync_flags", &self.sync_flags)
            .field("command_history_size", &self.command_history.size())
            .field("kline_layers_tfs", &self.kline_layers.len())
            .finish()
    }
}

#[cfg(test)]
mod refold_debounce_tests {
    //! State-machine coverage for [`DataBubble::should_refold_now`] /
    //! [`DataBubble::mark_refolded`] (P0 hang fix part 2, chart-series arc
    //! 2026-07-10) — a fake clock built from a fixed `web_time::Instant`
    //! base plus `Duration` offsets, since `web_time::Instant` has no public
    //! constructor from an arbitrary point (only `now()` + arithmetic).
    use super::*;

    fn bubble() -> DataBubble {
        let mut store = crate::bubble::BubbleStore::new();
        let bid = store.create_bubble("BTCUSDT".to_string(), Timeframe::m1());
        store.bubble(bid).cloned().expect("just-created bubble")
    }

    #[test]
    fn first_refold_after_install_is_immediate() {
        let mut b = bubble();
        assert!(b.last_refold_at.is_none());
        // Cold: no prior refold recorded — must refold immediately
        // regardless of the debounce interval (cold responsiveness).
        assert!(b.should_refold_now(false, web_time::Instant::now()));
    }

    #[test]
    fn page_within_debounce_window_defers_and_sets_dirty() {
        let mut b = bubble();
        let t0 = web_time::Instant::now();
        assert!(b.should_refold_now(false, t0));
        b.mark_refolded(t0);
        assert!(!b.refold_dirty);

        // A non-terminal page 100ms later (< 500ms interval) must defer.
        let t1 = t0 + std::time::Duration::from_millis(100);
        assert!(!b.should_refold_now(false, t1), "page inside the debounce window must defer");
        assert!(b.refold_dirty, "deferred page must mark the bubble dirty");
    }

    #[test]
    fn page_past_debounce_window_refolds_and_clears_dirty() {
        let mut b = bubble();
        let t0 = web_time::Instant::now();
        b.mark_refolded(t0);

        let t1 = t0 + std::time::Duration::from_millis(100);
        assert!(!b.should_refold_now(false, t1));
        assert!(b.refold_dirty);

        // A later page past REFOLD_MIN_INTERVAL_MS must refold and clear dirty.
        let t2 = t0 + std::time::Duration::from_millis(REFOLD_MIN_INTERVAL_MS + 1);
        assert!(b.should_refold_now(false, t2), "page past the debounce window must refold");
        b.mark_refolded(t2);
        assert!(!b.refold_dirty);
        assert_eq!(b.last_refold_at, Some(t2));
    }

    #[test]
    fn terminal_page_always_refolds_when_dirty_even_inside_window() {
        let mut b = bubble();
        let t0 = web_time::Instant::now();
        b.mark_refolded(t0);

        // Defer a non-terminal page inside the window — sets dirty.
        let t1 = t0 + std::time::Duration::from_millis(50);
        assert!(!b.should_refold_now(false, t1));
        assert!(b.refold_dirty);

        // The terminal (exhausted) page arrives STILL inside the debounce
        // window — the GUARANTEE clause must override the debounce and
        // refold anyway, since the bubble is dirty.
        let t2 = t0 + std::time::Duration::from_millis(80);
        assert!(b.should_refold_now(true, t2), "terminal page must always refold when dirty, even inside the window");
    }

    #[test]
    fn terminal_page_without_dirty_still_respects_debounce() {
        let mut b = bubble();
        let t0 = web_time::Instant::now();
        b.mark_refolded(t0);

        // No deferred page in between — bubble is clean. A terminal page
        // arriving inside the window with nothing owed must NOT force an
        // extra refold (there is nothing stale to catch up on).
        let t1 = t0 + std::time::Duration::from_millis(50);
        assert!(!b.should_refold_now(true, t1), "terminal page with a clean bubble still respects the debounce");
    }
}
