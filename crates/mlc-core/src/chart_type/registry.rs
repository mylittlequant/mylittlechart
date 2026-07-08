//! Data-only chart-type property matrix.
//!
//! Verified against source (2026-07-06, chart-type registry arc Step 1):
//! - `mlc-core/src/toolbar.rs::chart_type_items()` — id, label, icon_id, group.
//! - `mlc-app/src/active_series.rs` — `chart_type_hides_time_grid`,
//!   `chart_type_hides_timeframe_picker`, `chart_type_is_derived_seed`,
//!   `ts_shadow_for_chart_type`, `resolve_active_series`, `allowed_timeframes`.
//! - `mlc-app/src/input/chart_out_events.rs::sync_default_indicators_for_chart_type`
//!   — default companion indicators; `min_usable_bar_spacing` — min bar spacing.
//! - `mlc-app-layout/src/layout/modals/chart_settings.rs::chart_type_param_spec`
//!   — per-type parameter dropdown catalog.
//!
//! Was dead code as of Step 1 (no call site read from it yet). First live
//! consumer (2026-07-07): `mlc-app::active_series::chart_type_deepen_is_teardown`
//! reads `ChartTypeDef::deepen_kind`.

/// Coarse visual/data-source grouping, mirrors the dropdown section headers
/// in `toolbar.rs::chart_type_items()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartTypeGroup {
    Candles,
    Lines,
    Areas,
    TradeAggregation,
    PriceAction,
    Orderflow,
}

/// How a chart_type sources and renders its series.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartTypeKind {
    /// Plain render style over kline bars: candles, bars, hollow, heikin, line, step_line,
    /// line_markers, area, hlc_area, baseline.
    RenderOnly,
    /// Trade-aggregated bar family (own bar-size / threshold / count params, writes into
    /// `bubble.bars` still): range_bar, volume_bar, tick_bar, dollar_bar, tick_imbalance_bar,
    /// volume_imbalance_bar, run_bar.
    TradeAggBar,
    /// Non-time-uniform price-action transforms with their own bubble slot: renko, pnf, kagi,
    /// three_line_break.
    PriceActionTransform,
    /// Orderflow types with their own bubble slot + reactor path via BubbleEvent::*Appended:
    /// footprint, tpo.
    OrderflowSlotted,
    /// Orderflow types with ring-buffer bubble state, bypass ActiveSeries: dom_heatmap,
    /// liquidation_heatmap.
    OrderflowRingBuffer,
}

/// Mirrors `mlc-app::active_series::ActiveSeries`'s variant family — which
/// array a chart_type actually renders and paces its viewport off of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeriesVariant {
    Bars,
    Renko,
    Pnf,
    Kagi,
    Tlb,
    Tpo,
}

/// Which backdrop array a chart_type's OHLC data ultimately comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarsSourceKind {
    Kline,
    FootprintOhlc,
}

/// What class of upstream data a chart_type needs to render a REAL history
/// (not "what protocol" — dig3 capability tiers are venue-specific and
/// resolved at runtime via `BackendCore::data_mode_for`; this is the STATIC
/// classification used to decide which venues/account-types can ever show
/// real history for this chart_type, independent of which venue is
/// currently active). See
/// `docs/mlc/plans/hybrid-data-modes-plan-2026-07-08.md` Step 5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataRequirement {
    /// Backed by kline REST pagination — works on every TRUSTED venue
    /// (deepest cross-venue-uniform path). candle-family + price-action
    /// transforms (renko/pnf/kagi/tlb — synthetic-trades-from-klines) + tpo
    /// (1m-klines-fed).
    Klines,
    /// Needs REAL per-trade history (aggTrades pagination or equivalent) to
    /// show anything before "now" — footprint, counted-bar family. On a
    /// `RecentOnly` venue this chart_type either goes hybrid (footprint) or
    /// shows empty-space-before-live (counted bars).
    DeepTrades,
    /// No history concept at all — always live-only by nature, on every
    /// venue (dom/liquidation heatmap).
    LiveFeed,
}

/// Resolved data-availability class for one chart_type on the active
/// (exchange, account_type) — the dropdown-marking result computed by
/// `BackendCore::data_mode_for` (`mlc-backend`, which resolves
/// `DataRequirement` against the venue's `TradeHistoryCapabilities`). Pure
/// data, no dig3 types — mlc-core stays capability-blind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataMode {
    /// Klines-backed or LiveFeed-by-design type, or DeepTrades type on a
    /// venue/account with RestDeep/RestWindow capability — real history.
    Full,
    /// DeepTrades type on a RecentOnly venue, footprint specifically —
    /// hybrid (kline backdrop + live clusters).
    Hybrid,
    /// DeepTrades type on a RecentOnly venue, NOT footprint (counted-bar
    /// family) — live-only, empty space before the live window.
    LiveOnly,
    /// Capability unknown (exchange not connected yet / unrecognized type).
    Unknown,
}

/// How a chart_type's history is deepened on scroll-to-edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeepenKind {
    /// Kline REST-page prepend via `request_scroll_bars` + `merge_scroll_bars` — sorted-append,
    /// reactor uses `capture_view_anchor`/`reconcile_view_keep_anchor`.
    KlinePrepend,
    /// In-place prepend-fold via `Station::rewarm_derived` — the live station
    /// subscription keeps running, older points are spliced in front of the
    /// existing ring under one lock and delivered as ONE batched
    /// `LiveUpdate::*HistoryPrepended` event; reactor uses
    /// `reconcile_derived_series`' prepend-shift the same way kline does.
    /// Covers renko/pnf/kagi/three_line_break/range_bar/volume_bar/tick_bar/
    /// dollar_bar/cvd/tpo (derived-prepend-fold-plan-2026-07-07, Phase 1).
    DerivedPrependFold,
    /// Teardown+respawn of the dig3-station subscription with doubled warm_n
    /// — reactor uses `reconcile_derived_series` + `derived_series_stamp`.
    /// Restricted to tick_imbalance_bar/volume_imbalance_bar/run_bar: their
    /// fold state is an EMA carried over the ENTIRE history, not
    /// reconstructible from already-emitted output (see
    /// `Station::rewarm_derived`'s `RewarmUnsupported` doc comment).
    DerivedTeardownRespawn,
    /// No deepen support (dom/liq heatmaps have ring-buffer state, no history extension).
    Unsupported,
}

/// Which resolution/granularity concept the TF toolbar button and its
/// dropdown surface for this chart_type (owner doctrine 2026-07-08): "the TF
/// button shows the resolution picker for the active instrument — seconds/
/// minutes for kline-family, ticks for tick-driven bars, ranges for
/// range-driven bars — the SAME button, adapted to the active chart_type".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionKind {
    /// Wall-clock/tick timeframe (`bubble.timeframe`) — candle family, TPO,
    /// and the two ring-buffer heatmaps. Button label = TF name unchanged;
    /// dropdown = the existing `timeframe_selector` TF list (whitelisted or
    /// full, per `tf_whitelist`/`hides_tf_picker`).
    Time,
    /// Bucket interval keyed by the SAME `bubble.timeframe` mechanism as
    /// `Time` (footprint's `UpdateChartTypeInterval` re-stamps
    /// `bubble.timeframe` — see `chart_out_events.rs`), restricted to a
    /// chart_type-specific whitelist (`tf_whitelist`). Distinct label from
    /// `Time` only in that the field is conceptually "bucket size", not
    /// "candle period" — rendering is identical (TF name).
    Interval,
    /// Numeric `chart_type_param`-driven resolution (range/tick/volume/
    /// dollar/imbalance/run bars, renko/pnf/kagi/three_line_break). Button
    /// label = formatted `chart_type_param` + `suffix`; dropdown lists
    /// `ChartTypeDef::param_spec`'s presets (single source of truth shared
    /// with the settings-modal numeric-param row).
    Param { suffix: &'static str },
}

/// `timeframe_selector` item-id prefix used for `ResolutionKind::Param`
/// dropdown entries (owner doctrine 2026-07-08). Lives in `mlc-core` (not
/// `mlc-app-layout`, where the dropdown is actually built) because the
/// dispatch side — `ChartToolbarState::handle_dropdown_select_with_chart`,
/// also `mlc-core` — needs to recognize the prefix without a reverse
/// dependency on the app layer; the id itself carries the numeric value, so
/// dispatch never needs to know the active chart_type.
pub const RESOLUTION_PARAM_ITEM_PREFIX: &str = "reso_param:";

/// One preset option in a `ChartTypeParamSpec`'s dropdown.
#[derive(Clone, Copy, Debug)]
pub struct ChartTypeParamOption {
    pub value_id: &'static str,
    pub label: &'static str,
}

/// Render spec for a chart_type's single editable-parameter row (Instrument
/// tab top slot). Mirrors `mlc-app-layout::chart_settings::ChartTypeParamSpec`
/// field-for-field, using `&'static str` since every source value is a
/// string literal.
#[derive(Clone, Copy, Debug)]
pub struct ChartTypeParamSpec {
    pub section_title: &'static str,
    pub row_label: &'static str,
    pub options: &'static [ChartTypeParamOption],
    pub fixed_note: Option<&'static str>,
}

/// Static property row for one `chart_type` id.
#[derive(Clone, Copy, Debug)]
pub struct ChartTypeDef {
    pub id: &'static str,
    /// English fallback (localization keys handled at UI layer).
    pub label: &'static str,
    pub icon_id: &'static str,
    pub group: ChartTypeGroup,
    pub kind: ChartTypeKind,
    pub series_variant: SeriesVariant,
    pub bars_source: BarsSourceKind,
    /// Static upstream-data classification (hybrid-data-modes-plan-2026-07-08,
    /// Step 5) — resolved against a venue's `TradeHistoryCapabilities` by
    /// `BackendCore::data_mode_for`, never read directly by mlc-core itself.
    pub data_requirement: DataRequirement,
    pub hides_time_grid: bool,
    pub hides_tf_picker: bool,
    /// None = no chart-type-specific TF gate (fall back to ALL_TF_IDS or hides_tf_picker=true).
    /// Some(slice) = explicit whitelist. Only footprint (6 TFs) and tpo (all-except-tick) use this.
    pub tf_whitelist: Option<&'static [&'static str]>,
    pub min_bar_spacing: Option<f64>,
    pub default_indicators: &'static [&'static str],
    pub param_spec: Option<ChartTypeParamSpec>,
    /// Placeholder text for the toolbar dropdown's inline parameter input
    /// (`DropdownItemDef::action_with_input`'s `default_text`), mirrored from
    /// `toolbar.rs::chart_type_items()`. `Some` for every trade-agg /
    /// price-action bar type plus footprint/tpo; `None` for plain render
    /// styles and the two ring-buffer heatmaps. NOT derived from
    /// `param_spec` — footprint has `param_spec: None` but still carries an
    /// inline default (its editable surface is the bespoke settings block,
    /// not the numeric-param dropdown, yet the toolbar row still shows an
    /// inline TF text field).
    pub inline_default: Option<&'static str>,
    pub deepen_kind: DeepenKind,
    /// Whether reconcile needs a ts-shadow Vec<Bar> for drawings/labels (renko/pnf/kagi/tlb only).
    pub needs_ts_shadow: bool,
    /// Whether the settings modal renders a bespoke params block (footprint only).
    pub has_bespoke_settings_block: bool,
    /// Which resolution concept the TF toolbar button/dropdown surfaces for
    /// this chart_type (owner doctrine 2026-07-08). See [`ResolutionKind`].
    pub resolution: ResolutionKind,
}

// ── TPO's TF whitelist ("minutes and up") ───────────────────────────────────
//
// TPO's letter-frequency bucketing expects a wall-clock minute-or-coarser
// period — excludes the sub-minute second TFs (tf_1s/tf_5s/tf_15s/tf_30s)
// added by the resolution-axes rework (2026-07-08), same as it excluded the
// removed tick TFs (tf_1t/tf_10t/tf_100t/tf_1000t) before them.
static TPO_TF_WHITELIST: &[&str] = &[
    "tf_1m", "tf_3m", "tf_5m", "tf_15m", "tf_30m",
    "tf_1h", "tf_2h", "tf_4h", "tf_6h", "tf_12h",
    "tf_1d", "tf_1w", "tf_1M",
];

static FOOTPRINT_TF_WHITELIST: &[&str] = &["tf_1m", "tf_3m", "tf_5m", "tf_15m", "tf_30m", "tf_1h"];

// ── param_spec option tables (mirrors chart_settings.rs::chart_type_param_spec) ──

// Price-quantized price-action family (range_bar/renko/pnf/kagi) shares one
// ladder — owner doctrine 2026-07-08 resolution-axes rework: `param` for
// these four is now denominated in INSTRUMENT TICKS (not absolute price).
// `build_subscribe_params` (mlc-app/src/input/chart_out_events.rs) converts
// `price_units = ticks * tick_size` at subscribe time.
static RENKO_OPTIONS: &[ChartTypeParamOption] = &[
    ChartTypeParamOption { value_id: "1", label: "1" },
    ChartTypeParamOption { value_id: "2", label: "2" },
    ChartTypeParamOption { value_id: "5", label: "5" },
    ChartTypeParamOption { value_id: "10", label: "10" },
    ChartTypeParamOption { value_id: "20", label: "20" },
    ChartTypeParamOption { value_id: "50", label: "50" },
    ChartTypeParamOption { value_id: "100", label: "100" },
];

static PNF_OPTIONS: &[ChartTypeParamOption] = RENKO_OPTIONS;
static KAGI_OPTIONS: &[ChartTypeParamOption] = RENKO_OPTIONS;
static RANGE_BAR_OPTIONS: &[ChartTypeParamOption] = RENKO_OPTIONS;

static THREE_LINE_BREAK_OPTIONS: &[ChartTypeParamOption] = &[
    ChartTypeParamOption { value_id: "2", label: "2" },
    ChartTypeParamOption { value_id: "3", label: "3" },
    ChartTypeParamOption { value_id: "4", label: "4" },
    ChartTypeParamOption { value_id: "6", label: "6" },
];

static VOLUME_BAR_OPTIONS: &[ChartTypeParamOption] = &[
    ChartTypeParamOption { value_id: "0.1", label: "0.1" },
    ChartTypeParamOption { value_id: "0.5", label: "0.5" },
    ChartTypeParamOption { value_id: "1", label: "1" },
    ChartTypeParamOption { value_id: "5", label: "5" },
    ChartTypeParamOption { value_id: "10", label: "10" },
    ChartTypeParamOption { value_id: "50", label: "50" },
    ChartTypeParamOption { value_id: "100", label: "100" },
    ChartTypeParamOption { value_id: "500", label: "500" },
    ChartTypeParamOption { value_id: "1000", label: "1,000" },
];

// Labels are plain K/M-shorthand digits, no `$` — the `$` prefix is added by
// each consumer (toolbar's `format_resolution_param_label`, settings-modal's
// verbatim display) so the registry stays presentation-neutral. Matches the
// TF button's own K/M shorthand for the live value (`format_resolution_param`).
static DOLLAR_BAR_OPTIONS: &[ChartTypeParamOption] = &[
    ChartTypeParamOption { value_id: "1000", label: "1K" },
    ChartTypeParamOption { value_id: "10000", label: "10K" },
    ChartTypeParamOption { value_id: "100000", label: "100K" },
    ChartTypeParamOption { value_id: "1000000", label: "1M" },
    ChartTypeParamOption { value_id: "10000000", label: "10M" },
];

static TICK_BAR_OPTIONS: &[ChartTypeParamOption] = &[
    ChartTypeParamOption { value_id: "1", label: "1" },
    ChartTypeParamOption { value_id: "2", label: "2" },
    ChartTypeParamOption { value_id: "5", label: "5" },
    ChartTypeParamOption { value_id: "10", label: "10" },
    ChartTypeParamOption { value_id: "25", label: "25" },
    ChartTypeParamOption { value_id: "50", label: "50" },
    ChartTypeParamOption { value_id: "100", label: "100" },
    ChartTypeParamOption { value_id: "250", label: "250" },
    ChartTypeParamOption { value_id: "500", label: "500" },
    ChartTypeParamOption { value_id: "1000", label: "1,000" },
];

static IMBALANCE_RUN_OPTIONS: &[ChartTypeParamOption] = &[
    ChartTypeParamOption { value_id: "10", label: "10" },
    ChartTypeParamOption { value_id: "25", label: "25" },
    ChartTypeParamOption { value_id: "50", label: "50" },
    ChartTypeParamOption { value_id: "100", label: "100" },
    ChartTypeParamOption { value_id: "250", label: "250" },
];

static TPO_OPTIONS: &[ChartTypeParamOption] = &[
    ChartTypeParamOption { value_id: "5", label: "5" },
    ChartTypeParamOption { value_id: "15", label: "15" },
    ChartTypeParamOption { value_id: "30", label: "30" },
    ChartTypeParamOption { value_id: "60", label: "60" },
];

static REGISTRY: &[ChartTypeDef] = &[
    // ── Candles ──────────────────────────────────────────────────────────
    ChartTypeDef {
        id: "candles",
        label: "Candles",
        icon_id: "Candlestick",
        group: ChartTypeGroup::Candles,
        kind: ChartTypeKind::RenderOnly,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::Klines,
        hides_time_grid: false,
        hides_tf_picker: false,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &["VOLUME"],
        param_spec: None,
        inline_default: None,
        deepen_kind: DeepenKind::KlinePrepend,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Time,
    },
    ChartTypeDef {
        id: "bars",
        label: "Bars",
        icon_id: "BarChart",
        group: ChartTypeGroup::Candles,
        kind: ChartTypeKind::RenderOnly,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::Klines,
        hides_time_grid: false,
        hides_tf_picker: false,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &["VOLUME"],
        param_spec: None,
        inline_default: None,
        deepen_kind: DeepenKind::KlinePrepend,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Time,
    },
    ChartTypeDef {
        id: "hollow_candles",
        label: "Hollow Candles",
        icon_id: "HollowCandles",
        group: ChartTypeGroup::Candles,
        kind: ChartTypeKind::RenderOnly,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::Klines,
        hides_time_grid: false,
        hides_tf_picker: false,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &["VOLUME"],
        param_spec: None,
        inline_default: None,
        deepen_kind: DeepenKind::KlinePrepend,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Time,
    },
    ChartTypeDef {
        id: "heikin_ashi",
        label: "Heikin Ashi",
        icon_id: "HeikinAshi",
        group: ChartTypeGroup::Candles,
        kind: ChartTypeKind::RenderOnly,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::Klines,
        hides_time_grid: false,
        hides_tf_picker: false,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &["VOLUME"],
        param_spec: None,
        inline_default: None,
        deepen_kind: DeepenKind::KlinePrepend,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Time,
    },
    // ── Lines ────────────────────────────────────────────────────────────
    ChartTypeDef {
        id: "line",
        label: "Line",
        icon_id: "LineChart",
        group: ChartTypeGroup::Lines,
        kind: ChartTypeKind::RenderOnly,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::Klines,
        hides_time_grid: false,
        hides_tf_picker: false,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &["VOLUME"],
        param_spec: None,
        inline_default: None,
        deepen_kind: DeepenKind::KlinePrepend,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Time,
    },
    ChartTypeDef {
        id: "step_line",
        label: "Step Line",
        icon_id: "StepLine",
        group: ChartTypeGroup::Lines,
        kind: ChartTypeKind::RenderOnly,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::Klines,
        hides_time_grid: false,
        hides_tf_picker: false,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &["VOLUME"],
        param_spec: None,
        inline_default: None,
        deepen_kind: DeepenKind::KlinePrepend,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Time,
    },
    ChartTypeDef {
        id: "line_markers",
        label: "Line With Markers",
        icon_id: "LineWithMarkers",
        group: ChartTypeGroup::Lines,
        kind: ChartTypeKind::RenderOnly,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::Klines,
        hides_time_grid: false,
        hides_tf_picker: false,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &["VOLUME"],
        param_spec: None,
        inline_default: None,
        deepen_kind: DeepenKind::KlinePrepend,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Time,
    },
    // ── Areas & Columns ──────────────────────────────────────────────────
    ChartTypeDef {
        id: "area",
        label: "Area",
        icon_id: "AreaChart",
        group: ChartTypeGroup::Areas,
        kind: ChartTypeKind::RenderOnly,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::Klines,
        hides_time_grid: false,
        hides_tf_picker: false,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &["VOLUME"],
        param_spec: None,
        inline_default: None,
        deepen_kind: DeepenKind::KlinePrepend,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Time,
    },
    ChartTypeDef {
        id: "hlc_area",
        label: "HLC Area",
        icon_id: "HlcArea",
        group: ChartTypeGroup::Areas,
        kind: ChartTypeKind::RenderOnly,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::Klines,
        hides_time_grid: false,
        hides_tf_picker: false,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &["VOLUME"],
        param_spec: None,
        inline_default: None,
        deepen_kind: DeepenKind::KlinePrepend,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Time,
    },
    ChartTypeDef {
        id: "baseline",
        label: "Baseline",
        icon_id: "Baseline",
        group: ChartTypeGroup::Areas,
        kind: ChartTypeKind::RenderOnly,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::Klines,
        hides_time_grid: false,
        hides_tf_picker: false,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &["VOLUME"],
        param_spec: None,
        inline_default: None,
        deepen_kind: DeepenKind::KlinePrepend,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Time,
    },
    // ── Trade Aggregation ────────────────────────────────────────────────
    ChartTypeDef {
        id: "range_bar",
        label: "Range Bars",
        icon_id: "RangeBar",
        group: ChartTypeGroup::TradeAggregation,
        kind: ChartTypeKind::TradeAggBar,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::DeepTrades,
        hides_time_grid: true,
        hides_tf_picker: true,
        tf_whitelist: None,
        min_bar_spacing: None,
        // sync_default_indicators_for_chart_type (chart_out_events.rs:2662-2675):
        // range_bar falls into the `_ => &[]` arm, not the kline-render VOLUME arm.
        default_indicators: &[],
        param_spec: Some(ChartTypeParamSpec {
            section_title: "RANGE BARS SETTINGS",
            row_label: "Threshold (ticks)",
            options: RANGE_BAR_OPTIONS,
            fixed_note: None,
        }),
        inline_default: Some("10"),
        deepen_kind: DeepenKind::DerivedPrependFold,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Param { suffix: "R" },
    },
    ChartTypeDef {
        id: "volume_bar",
        label: "Volume Bars",
        icon_id: "VolumeBar",
        group: ChartTypeGroup::TradeAggregation,
        kind: ChartTypeKind::TradeAggBar,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::DeepTrades,
        hides_time_grid: true,
        hides_tf_picker: true,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &[],
        param_spec: Some(ChartTypeParamSpec {
            section_title: "VOLUME BARS SETTINGS",
            row_label: "Threshold",
            options: VOLUME_BAR_OPTIONS,
            fixed_note: None,
        }),
        inline_default: Some("10.0"),
        deepen_kind: DeepenKind::DerivedPrependFold,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Param { suffix: "V" },
    },
    ChartTypeDef {
        id: "tick_bar",
        label: "Tick Bars",
        icon_id: "Tick",
        group: ChartTypeGroup::TradeAggregation,
        kind: ChartTypeKind::TradeAggBar,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::DeepTrades,
        hides_time_grid: true,
        hides_tf_picker: true,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &[],
        param_spec: Some(ChartTypeParamSpec {
            section_title: "TICK BARS SETTINGS",
            row_label: "Ticks per bar",
            options: TICK_BAR_OPTIONS,
            fixed_note: None,
        }),
        inline_default: Some("100"),
        deepen_kind: DeepenKind::DerivedPrependFold,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Param { suffix: "T" },
    },
    ChartTypeDef {
        id: "dollar_bar",
        label: "Dollar Bars",
        icon_id: "BarChart",
        group: ChartTypeGroup::TradeAggregation,
        kind: ChartTypeKind::TradeAggBar,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::DeepTrades,
        hides_time_grid: true,
        hides_tf_picker: true,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &[],
        param_spec: Some(ChartTypeParamSpec {
            section_title: "DOLLAR BARS SETTINGS",
            row_label: "Threshold",
            options: DOLLAR_BAR_OPTIONS,
            fixed_note: None,
        }),
        inline_default: Some("100000"),
        deepen_kind: DeepenKind::DerivedPrependFold,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Param { suffix: "$" },
    },
    ChartTypeDef {
        id: "tick_imbalance_bar",
        label: "Tick Imbalance",
        icon_id: "BarChart",
        group: ChartTypeGroup::TradeAggregation,
        kind: ChartTypeKind::TradeAggBar,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::DeepTrades,
        hides_time_grid: true,
        hides_tf_picker: true,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &[],
        param_spec: Some(ChartTypeParamSpec {
            section_title: "TICK IMBALANCE SETTINGS",
            row_label: "Min ticks",
            options: IMBALANCE_RUN_OPTIONS,
            fixed_note: Some("Alpha: 0.20"),
        }),
        inline_default: Some("50"),
        deepen_kind: DeepenKind::DerivedTeardownRespawn,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Param { suffix: "T" },
    },
    ChartTypeDef {
        id: "volume_imbalance_bar",
        label: "Volume Imbalance",
        icon_id: "BarChart",
        group: ChartTypeGroup::TradeAggregation,
        kind: ChartTypeKind::TradeAggBar,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::DeepTrades,
        hides_time_grid: true,
        hides_tf_picker: true,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &[],
        param_spec: Some(ChartTypeParamSpec {
            section_title: "VOLUME IMBALANCE SETTINGS",
            row_label: "Min ticks",
            options: IMBALANCE_RUN_OPTIONS,
            fixed_note: Some("Alpha: 0.20"),
        }),
        inline_default: Some("50"),
        deepen_kind: DeepenKind::DerivedTeardownRespawn,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Param { suffix: "V" },
    },
    ChartTypeDef {
        id: "run_bar",
        label: "Run Bars",
        icon_id: "BarChart",
        group: ChartTypeGroup::TradeAggregation,
        kind: ChartTypeKind::TradeAggBar,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::DeepTrades,
        hides_time_grid: true,
        hides_tf_picker: true,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &[],
        param_spec: Some(ChartTypeParamSpec {
            section_title: "RUN BARS SETTINGS",
            row_label: "Min ticks",
            options: IMBALANCE_RUN_OPTIONS,
            fixed_note: Some("Alpha: 0.20"),
        }),
        inline_default: Some("50"),
        deepen_kind: DeepenKind::DerivedTeardownRespawn,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Param { suffix: "RN" },
    },
    // ── Price Action ─────────────────────────────────────────────────────
    ChartTypeDef {
        id: "renko",
        label: "Renko",
        icon_id: "BarChart",
        group: ChartTypeGroup::PriceAction,
        kind: ChartTypeKind::PriceActionTransform,
        series_variant: SeriesVariant::Renko,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::Klines,
        hides_time_grid: false,
        hides_tf_picker: true,
        tf_whitelist: None,
        min_bar_spacing: None,
        // sync_default_indicators_for_chart_type: renko falls into `_ => &[]`.
        default_indicators: &[],
        param_spec: Some(ChartTypeParamSpec {
            section_title: "RENKO SETTINGS",
            row_label: "Box size (ticks)",
            options: RENKO_OPTIONS,
            fixed_note: Some("Reversal: 2"),
        }),
        inline_default: Some("10"),
        deepen_kind: DeepenKind::DerivedPrependFold,
        needs_ts_shadow: true,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Param { suffix: "B" },
    },
    ChartTypeDef {
        id: "pnf",
        label: "Point & Figure",
        icon_id: "BarChart",
        group: ChartTypeGroup::PriceAction,
        kind: ChartTypeKind::PriceActionTransform,
        series_variant: SeriesVariant::Pnf,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::Klines,
        hides_time_grid: false,
        hides_tf_picker: true,
        tf_whitelist: None,
        min_bar_spacing: Some(12.0),
        default_indicators: &[],
        param_spec: Some(ChartTypeParamSpec {
            section_title: "POINT & FIGURE SETTINGS",
            row_label: "Box size (ticks)",
            options: PNF_OPTIONS,
            fixed_note: Some("Reversal: 3"),
        }),
        inline_default: Some("10"),
        deepen_kind: DeepenKind::DerivedPrependFold,
        needs_ts_shadow: true,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Param { suffix: "B" },
    },
    ChartTypeDef {
        id: "kagi",
        label: "Kagi",
        icon_id: "LineChart",
        group: ChartTypeGroup::PriceAction,
        kind: ChartTypeKind::PriceActionTransform,
        series_variant: SeriesVariant::Kagi,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::Klines,
        hides_time_grid: false,
        hides_tf_picker: true,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &[],
        param_spec: Some(ChartTypeParamSpec {
            section_title: "KAGI SETTINGS",
            row_label: "Reversal (ticks)",
            options: KAGI_OPTIONS,
            fixed_note: None,
        }),
        inline_default: Some("10"),
        deepen_kind: DeepenKind::DerivedPrependFold,
        needs_ts_shadow: true,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Param { suffix: "RV" },
    },
    ChartTypeDef {
        id: "three_line_break",
        label: "3 Line Break",
        icon_id: "BarChart",
        group: ChartTypeGroup::PriceAction,
        kind: ChartTypeKind::PriceActionTransform,
        series_variant: SeriesVariant::Tlb,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::Klines,
        hides_time_grid: false,
        hides_tf_picker: true,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &[],
        param_spec: Some(ChartTypeParamSpec {
            section_title: "3 LINE BREAK SETTINGS",
            row_label: "Lines back",
            options: THREE_LINE_BREAK_OPTIONS,
            fixed_note: None,
        }),
        inline_default: Some("3"),
        deepen_kind: DeepenKind::DerivedPrependFold,
        needs_ts_shadow: true,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Param { suffix: "L" },
    },
    // ── Orderflow (heterogeneous) ────────────────────────────────────────
    ChartTypeDef {
        id: "footprint",
        label: "Clusters (Footprint)",
        icon_id: "Footprint",
        group: ChartTypeGroup::Orderflow,
        kind: ChartTypeKind::OrderflowSlotted,
        // Deliberate — viewport bookkeeping piggybacks bubble.bars
        // (active_series.rs:322-327), footprint's own slot
        // (bubble.footprint_bars) is not what ActiveSeries resolves to.
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::FootprintOhlc,
        data_requirement: DataRequirement::DeepTrades,
        hides_time_grid: false,
        hides_tf_picker: false,
        tf_whitelist: Some(FOOTPRINT_TF_WHITELIST),
        min_bar_spacing: Some(50.0),
        default_indicators: &["CVD"],
        // footprint intentionally absent from chart_type_param_spec's catalog
        // (chart_settings.rs:2317-2323) — its editable surface is the bespoke
        // settings block, not the generic numeric-param dropdown.
        param_spec: None,
        // toolbar.rs still shows an inline TF text field for footprint despite
        // param_spec being None — see field doc comment above.
        inline_default: Some("1m"),
        deepen_kind: DeepenKind::DerivedPrependFold,
        needs_ts_shadow: false,
        has_bespoke_settings_block: true,
        resolution: ResolutionKind::Interval,
    },
    ChartTypeDef {
        id: "tpo",
        label: "Market Profile (TPO)",
        icon_id: "Histogram",
        group: ChartTypeGroup::Orderflow,
        kind: ChartTypeKind::OrderflowSlotted,
        series_variant: SeriesVariant::Tpo,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::Klines,
        hides_time_grid: true,
        // NOTE: source says false, see hand-raise in Step 1 deliverable —
        // `chart_type_hides_timeframe_picker` (active_series.rs:411-426)
        // does NOT include "tpo"; `allowed_timeframes` gives tpo an explicit
        // whitelist (all TFs except tick) rather than hiding the picker.
        hides_tf_picker: false,
        tf_whitelist: Some(TPO_TF_WHITELIST),
        min_bar_spacing: None,
        default_indicators: &[],
        param_spec: Some(ChartTypeParamSpec {
            section_title: "MARKET PROFILE (TPO) SETTINGS",
            row_label: "Bucket minutes",
            options: TPO_OPTIONS,
            fixed_note: None,
        }),
        inline_default: Some("30"),
        deepen_kind: DeepenKind::DerivedPrependFold,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Time,
    },
    ChartTypeDef {
        id: "dom_heatmap",
        label: "DOM Heatmap",
        icon_id: "Histogram",
        group: ChartTypeGroup::Orderflow,
        kind: ChartTypeKind::OrderflowRingBuffer,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::LiveFeed,
        hides_time_grid: false,
        hides_tf_picker: false,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &[],
        param_spec: None,
        inline_default: None,
        deepen_kind: DeepenKind::Unsupported,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Time,
    },
    ChartTypeDef {
        id: "liquidation_heatmap",
        label: "Liquidation Heatmap",
        icon_id: "Activity",
        group: ChartTypeGroup::Orderflow,
        kind: ChartTypeKind::OrderflowRingBuffer,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        data_requirement: DataRequirement::LiveFeed,
        hides_time_grid: false,
        hides_tf_picker: false,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &[],
        param_spec: None,
        inline_default: None,
        deepen_kind: DeepenKind::Unsupported,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Time,
    },
];

/// All chart-type rows.
pub fn all() -> &'static [ChartTypeDef] {
    REGISTRY
}

/// Look up a single row by its `chart_type` id.
pub fn by_id(id: &str) -> Option<&'static ChartTypeDef> {
    REGISTRY.iter().find(|d| d.id == id)
}

/// Display label for a `ChartTypeGroup`'s dropdown section header, mirrors
/// `toolbar.rs::chart_type_items()`'s literal `DropdownItemDef::Header` text.
pub fn group_label(group: ChartTypeGroup) -> &'static str {
    match group {
        ChartTypeGroup::Candles => "Candles",
        ChartTypeGroup::Lines => "Lines",
        ChartTypeGroup::Areas => "Areas & Columns",
        ChartTypeGroup::TradeAggregation => "Trade Aggregation",
        ChartTypeGroup::PriceAction => "Price Action",
        ChartTypeGroup::Orderflow => "Orderflow",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_ids_unique() {
        let ids: Vec<&str> = all().iter().map(|d| d.id).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len(), "duplicate chart_type id in registry");
    }

    #[test]
    fn count_matches_expected() {
        assert_eq!(all().len(), 25);
    }

    #[test]
    fn footprint_tf_whitelist_matches_bucket_intervals() {
        let d = by_id("footprint").unwrap();
        assert_eq!(
            d.tf_whitelist,
            Some(&["tf_1m", "tf_3m", "tf_5m", "tf_15m", "tf_30m", "tf_1h"][..])
        );
    }

    #[test]
    fn all_types_have_a_group() {
        // Placeholder — enum forces this at compile time. Kept for future assertions.
        for d in all() {
            let _ = d.group;
        }
    }
}
