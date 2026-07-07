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
//! This module is dead code as of Step 1 — no call site reads from it yet.

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

/// How a chart_type's history is deepened on scroll-to-edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeepenKind {
    /// Kline REST-page prepend via `request_scroll_bars` + `merge_scroll_bars` — sorted-append,
    /// reactor uses `capture_view_anchor`/`reconcile_view_keep_anchor`.
    KlinePrepend,
    /// Teardown+respawn of the dig3-station subscription with doubled warm_n — reactor uses
    /// `reconcile_derived_series` + `derived_series_stamp`.
    DerivedTeardownRespawn,
    /// No deepen support (dom/liq heatmaps have ring-buffer state, no history extension).
    Unsupported,
}

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
}

// ── TPO's TF whitelist ("all except tick") ─────────────────────────────────
//
// `allowed_timeframes` (active_series.rs:486-490) computes this as
// `ALL_TF_IDS - TICK_TF_IDS` at runtime rather than a literal. Mirrored here
// as an explicit static list (ALL_TF_IDS minus tf_1t/tf_10t/tf_100t/tf_1000t).
static TPO_TF_WHITELIST: &[&str] = &[
    "tf_1m", "tf_3m", "tf_5m", "tf_15m", "tf_30m",
    "tf_1h", "tf_2h", "tf_4h", "tf_6h", "tf_12h",
    "tf_1d", "tf_1w", "tf_1M",
];

static FOOTPRINT_TF_WHITELIST: &[&str] = &["tf_1m", "tf_3m", "tf_5m", "tf_15m", "tf_30m", "tf_1h"];

// ── param_spec option tables (mirrors chart_settings.rs::chart_type_param_spec) ──

static RENKO_OPTIONS: &[ChartTypeParamOption] = &[
    ChartTypeParamOption { value_id: "0.5", label: "0.5" },
    ChartTypeParamOption { value_id: "1", label: "1" },
    ChartTypeParamOption { value_id: "2", label: "2" },
    ChartTypeParamOption { value_id: "5", label: "5" },
    ChartTypeParamOption { value_id: "10", label: "10" },
    ChartTypeParamOption { value_id: "25", label: "25" },
    ChartTypeParamOption { value_id: "50", label: "50" },
    ChartTypeParamOption { value_id: "100", label: "100" },
];

static PNF_OPTIONS: &[ChartTypeParamOption] = RENKO_OPTIONS;

static KAGI_OPTIONS: &[ChartTypeParamOption] = &[
    ChartTypeParamOption { value_id: "0.5", label: "0.5" },
    ChartTypeParamOption { value_id: "1", label: "1" },
    ChartTypeParamOption { value_id: "2", label: "2" },
    ChartTypeParamOption { value_id: "5", label: "5" },
    ChartTypeParamOption { value_id: "10", label: "10" },
    ChartTypeParamOption { value_id: "25", label: "25" },
    ChartTypeParamOption { value_id: "50", label: "50" },
];

static THREE_LINE_BREAK_OPTIONS: &[ChartTypeParamOption] = &[
    ChartTypeParamOption { value_id: "2", label: "2" },
    ChartTypeParamOption { value_id: "3", label: "3" },
    ChartTypeParamOption { value_id: "4", label: "4" },
    ChartTypeParamOption { value_id: "6", label: "6" },
];

static RANGE_BAR_OPTIONS: &[ChartTypeParamOption] = &[
    ChartTypeParamOption { value_id: "1", label: "1" },
    ChartTypeParamOption { value_id: "5", label: "5" },
    ChartTypeParamOption { value_id: "10", label: "10" },
    ChartTypeParamOption { value_id: "25", label: "25" },
    ChartTypeParamOption { value_id: "50", label: "50" },
    ChartTypeParamOption { value_id: "100", label: "100" },
];

static VOLUME_BAR_OPTIONS: &[ChartTypeParamOption] = &[
    ChartTypeParamOption { value_id: "100", label: "100" },
    ChartTypeParamOption { value_id: "500", label: "500" },
    ChartTypeParamOption { value_id: "1000", label: "1,000" },
    ChartTypeParamOption { value_id: "5000", label: "5,000" },
    ChartTypeParamOption { value_id: "10000", label: "10,000" },
];

static DOLLAR_BAR_OPTIONS: &[ChartTypeParamOption] = &[
    ChartTypeParamOption { value_id: "100000", label: "100,000" },
    ChartTypeParamOption { value_id: "500000", label: "500,000" },
    ChartTypeParamOption { value_id: "1000000", label: "1,000,000" },
    ChartTypeParamOption { value_id: "5000000", label: "5,000,000" },
    ChartTypeParamOption { value_id: "10000000", label: "10,000,000" },
];

static TICK_BAR_OPTIONS: &[ChartTypeParamOption] = &[
    ChartTypeParamOption { value_id: "100", label: "100" },
    ChartTypeParamOption { value_id: "200", label: "200" },
    ChartTypeParamOption { value_id: "500", label: "500" },
    ChartTypeParamOption { value_id: "1000", label: "1,000" },
    ChartTypeParamOption { value_id: "2000", label: "2,000" },
];

static IMBALANCE_RUN_OPTIONS: &[ChartTypeParamOption] = &[
    ChartTypeParamOption { value_id: "50", label: "50" },
    ChartTypeParamOption { value_id: "100", label: "100" },
    ChartTypeParamOption { value_id: "200", label: "200" },
    ChartTypeParamOption { value_id: "500", label: "500" },
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
    },
    ChartTypeDef {
        id: "bars",
        label: "Bars",
        icon_id: "BarChart",
        group: ChartTypeGroup::Candles,
        kind: ChartTypeKind::RenderOnly,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
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
    },
    ChartTypeDef {
        id: "hollow_candles",
        label: "Hollow Candles",
        icon_id: "HollowCandles",
        group: ChartTypeGroup::Candles,
        kind: ChartTypeKind::RenderOnly,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
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
    },
    ChartTypeDef {
        id: "heikin_ashi",
        label: "Heikin Ashi",
        icon_id: "HeikinAshi",
        group: ChartTypeGroup::Candles,
        kind: ChartTypeKind::RenderOnly,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
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
    },
    ChartTypeDef {
        id: "step_line",
        label: "Step Line",
        icon_id: "StepLine",
        group: ChartTypeGroup::Lines,
        kind: ChartTypeKind::RenderOnly,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
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
    },
    ChartTypeDef {
        id: "line_markers",
        label: "Line With Markers",
        icon_id: "LineWithMarkers",
        group: ChartTypeGroup::Lines,
        kind: ChartTypeKind::RenderOnly,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
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
    },
    ChartTypeDef {
        id: "hlc_area",
        label: "HLC Area",
        icon_id: "HlcArea",
        group: ChartTypeGroup::Areas,
        kind: ChartTypeKind::RenderOnly,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
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
    },
    ChartTypeDef {
        id: "baseline",
        label: "Baseline",
        icon_id: "Baseline",
        group: ChartTypeGroup::Areas,
        kind: ChartTypeKind::RenderOnly,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
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
        hides_time_grid: true,
        hides_tf_picker: true,
        tf_whitelist: None,
        min_bar_spacing: None,
        // sync_default_indicators_for_chart_type (chart_out_events.rs:2662-2675):
        // range_bar falls into the `_ => &[]` arm, not the kline-render VOLUME arm.
        default_indicators: &[],
        param_spec: Some(ChartTypeParamSpec {
            section_title: "RANGE BARS SETTINGS",
            row_label: "Threshold",
            options: RANGE_BAR_OPTIONS,
            fixed_note: None,
        }),
        inline_default: Some("10.0"),
        deepen_kind: DeepenKind::DerivedTeardownRespawn,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
    },
    ChartTypeDef {
        id: "volume_bar",
        label: "Volume Bars",
        icon_id: "VolumeBar",
        group: ChartTypeGroup::TradeAggregation,
        kind: ChartTypeKind::TradeAggBar,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
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
        inline_default: Some("1000.0"),
        deepen_kind: DeepenKind::DerivedTeardownRespawn,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
    },
    ChartTypeDef {
        id: "tick_bar",
        label: "Tick Bars",
        icon_id: "Tick",
        group: ChartTypeGroup::TradeAggregation,
        kind: ChartTypeKind::TradeAggBar,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
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
        deepen_kind: DeepenKind::DerivedTeardownRespawn,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
    },
    ChartTypeDef {
        id: "dollar_bar",
        label: "Dollar Bars",
        icon_id: "BarChart",
        group: ChartTypeGroup::TradeAggregation,
        kind: ChartTypeKind::TradeAggBar,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
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
        inline_default: Some("1000000"),
        deepen_kind: DeepenKind::DerivedTeardownRespawn,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
    },
    ChartTypeDef {
        id: "tick_imbalance_bar",
        label: "Tick Imbalance",
        icon_id: "BarChart",
        group: ChartTypeGroup::TradeAggregation,
        kind: ChartTypeKind::TradeAggBar,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
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
    },
    ChartTypeDef {
        id: "volume_imbalance_bar",
        label: "Volume Imbalance",
        icon_id: "BarChart",
        group: ChartTypeGroup::TradeAggregation,
        kind: ChartTypeKind::TradeAggBar,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
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
    },
    ChartTypeDef {
        id: "run_bar",
        label: "Run Bars",
        icon_id: "BarChart",
        group: ChartTypeGroup::TradeAggregation,
        kind: ChartTypeKind::TradeAggBar,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
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
        hides_time_grid: false,
        hides_tf_picker: true,
        tf_whitelist: None,
        min_bar_spacing: None,
        // sync_default_indicators_for_chart_type: renko falls into `_ => &[]`.
        default_indicators: &[],
        param_spec: Some(ChartTypeParamSpec {
            section_title: "RENKO SETTINGS",
            row_label: "Box size",
            options: RENKO_OPTIONS,
            fixed_note: Some("Reversal: 2"),
        }),
        inline_default: Some("10.0"),
        deepen_kind: DeepenKind::DerivedTeardownRespawn,
        needs_ts_shadow: true,
        has_bespoke_settings_block: false,
    },
    ChartTypeDef {
        id: "pnf",
        label: "Point & Figure",
        icon_id: "BarChart",
        group: ChartTypeGroup::PriceAction,
        kind: ChartTypeKind::PriceActionTransform,
        series_variant: SeriesVariant::Pnf,
        bars_source: BarsSourceKind::Kline,
        hides_time_grid: false,
        hides_tf_picker: true,
        tf_whitelist: None,
        min_bar_spacing: Some(12.0),
        default_indicators: &[],
        param_spec: Some(ChartTypeParamSpec {
            section_title: "POINT & FIGURE SETTINGS",
            row_label: "Box size",
            options: PNF_OPTIONS,
            fixed_note: Some("Reversal: 3"),
        }),
        inline_default: Some("10.0"),
        deepen_kind: DeepenKind::DerivedTeardownRespawn,
        needs_ts_shadow: true,
        has_bespoke_settings_block: false,
    },
    ChartTypeDef {
        id: "kagi",
        label: "Kagi",
        icon_id: "LineChart",
        group: ChartTypeGroup::PriceAction,
        kind: ChartTypeKind::PriceActionTransform,
        series_variant: SeriesVariant::Kagi,
        bars_source: BarsSourceKind::Kline,
        hides_time_grid: false,
        hides_tf_picker: true,
        tf_whitelist: None,
        min_bar_spacing: None,
        default_indicators: &[],
        param_spec: Some(ChartTypeParamSpec {
            section_title: "KAGI SETTINGS",
            row_label: "Reversal",
            options: KAGI_OPTIONS,
            fixed_note: None,
        }),
        inline_default: Some("10.0"),
        deepen_kind: DeepenKind::DerivedTeardownRespawn,
        needs_ts_shadow: true,
        has_bespoke_settings_block: false,
    },
    ChartTypeDef {
        id: "three_line_break",
        label: "3 Line Break",
        icon_id: "BarChart",
        group: ChartTypeGroup::PriceAction,
        kind: ChartTypeKind::PriceActionTransform,
        series_variant: SeriesVariant::Tlb,
        bars_source: BarsSourceKind::Kline,
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
        deepen_kind: DeepenKind::DerivedTeardownRespawn,
        needs_ts_shadow: true,
        has_bespoke_settings_block: false,
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
        deepen_kind: DeepenKind::DerivedTeardownRespawn,
        needs_ts_shadow: false,
        has_bespoke_settings_block: true,
    },
    ChartTypeDef {
        id: "tpo",
        label: "Market Profile (TPO)",
        icon_id: "Histogram",
        group: ChartTypeGroup::Orderflow,
        kind: ChartTypeKind::OrderflowSlotted,
        series_variant: SeriesVariant::Tpo,
        bars_source: BarsSourceKind::Kline,
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
        deepen_kind: DeepenKind::DerivedTeardownRespawn,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
    },
    ChartTypeDef {
        id: "dom_heatmap",
        label: "DOM Heatmap",
        icon_id: "Histogram",
        group: ChartTypeGroup::Orderflow,
        kind: ChartTypeKind::OrderflowRingBuffer,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
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
    },
    ChartTypeDef {
        id: "liquidation_heatmap",
        label: "Liquidation Heatmap",
        icon_id: "Activity",
        group: ChartTypeGroup::Orderflow,
        kind: ChartTypeKind::OrderflowRingBuffer,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
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
