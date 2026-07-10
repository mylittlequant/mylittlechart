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
    /// Kline re-cuts (2026-07-08 minimal-sufficient-source doctrine promoted
    /// range_bar/volume_bar/dollar_bar to Klines-backed history) — menu label
    /// "Batched Bars". Distinct from [`Self::TradeAggregation`] ("Trade
    /// Bars"), which stays genuinely trade-fed (`DataRequirement::DeepTrades`).
    BatchedBars,
    /// Genuinely trade-fed aggregation family (tick_bar, tick_imbalance_bar,
    /// volume_imbalance_bar, run_bar) — menu label "Trade Bars".
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
    /// (1m-klines-fed) + the trade-aggregation family promoted to
    /// kline-sufficient (2026-07-08 minimal-sufficient-source doctrine):
    /// range_bar/volume_bar/dollar_bar. Their HISTORY now seeds from
    /// paginated 1m klines via synthetic 4-leg trades (same path
    /// renko/pnf/kagi/tlb always used) + a shallow live trades_recent tail
    /// — deep aggTrades pagination is gone for this class.
    Klines,
    /// Needs REAL per-trade history (aggTrades pagination or equivalent) to
    /// show anything before "now" — footprint, tick_bar, tick/volume
    /// imbalance, run_bar. On a `RecentOnly` venue this chart_type either
    /// goes hybrid (footprint) or shows empty-space-before-live (counted
    /// bars).
    DeepTrades,
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
    /// Kline REST-page prepend via `request_scroll_bars` + the raw kline
    /// layer's `prepend_batch`/display-rebuild in `tick_writers` — sorted-
    /// append, reactor uses `capture_view_anchor`/`reconcile_view_keep_anchor`.
    KlinePrepend,
    /// In-place prepend-fold via `Station::rewarm_derived` — the live station
    /// subscription keeps running, older points are spliced in front of the
    /// existing ring under one lock and delivered as ONE batched
    /// `LiveUpdate::*HistoryPrepended` event; reactor uses
    /// `reconcile_derived_series`' prepend-shift the same way kline does.
    /// Covers renko/pnf/kagi/three_line_break/range_bar/volume_bar/tick_bar/
    /// dollar_bar/cvd/tpo (derived-prepend-fold-plan-2026-07-07, Phase 1) and,
    /// since unified-data-plane Stage Э4, tick_imbalance_bar/
    /// volume_imbalance_bar/run_bar too — full refold from the raw trade
    /// layer reproduces byte-identical EMA state, so the teardown+respawn
    /// requirement these three used to need no longer applies (see the
    /// `tick_imbalance_bar` row's doc comment below).
    DerivedPrependFold,
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

/// Which client-side fold machine (if any) computes a chart_type's series —
/// a pure identifier, no `digdigdig3_station` dependency (mlc-core stays
/// dig3-blind; `mlc-backend::bubble::view_fold` maps each variant to its
/// concrete `TradeTo*Derived`/`TpoFrom*Derived` machine type). Kills the
/// forward matches in `FoldMachine::new`/`TradeFoldMachine::new` (chart_type
/// str → machine) and the reverse matches in `FoldMachine::chart_type`/
/// `TradeFoldMachine::chart_type` (machine → chart_type str) — both become a
/// single match on this enum (chart-type-engine-consolidation B3, S1/S2).
///
/// `Tpo` covers both `TpoSource` flavours (`TradeBucket`/`Kline1m`) — the
/// registry has one `tpo` row regardless of source; `tpo_source` is a
/// separate runtime selector (`view_fold.rs::TradeFoldMachine::new`'s
/// `tpo_source` parameter), not a registry axis. `Cvd` is deliberately NOT a
/// variant — the CVD companion line is not a registry row (see this module's
/// header doc comment) and keeps its own direct construction in
/// `view_fold.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineKind {
    /// Plain render-only types and any chart_type with no client-side fold
    /// machine at all.
    None,
    RangeBar,
    VolumeBar,
    DollarBar,
    Renko,
    Pnf,
    Kagi,
    ThreeLineBreak,
    TickBar,
    TickImbalance,
    VolumeImbalance,
    RunBar,
    Footprint,
    Tpo,
}

// ── K0 contract additions (chart-type-engine-consolidation B2, 2026-07-10) ──
//
// Additive-only extension of the registry contract: identity keys for the
// reseed last-wins merge (today hand-written per arm in
// `mlc-backend::lib::apply_fold_reseed_outputs` /
// `apply_trade_fold_reseed_outputs`), render policy (today hardcoded in
// `mlc-app::active_series::effective_wick_flag` and threaded per-toggle
// fields), and the fixed numeric constants that today live only inside
// `mlc-app::input::chart_out_events::build_subscribe_params`. Existing
// consumers are unchanged by this step (B3 compresses them onto these
// fields); the registry becomes the single source of truth for future
// consumers and the first live one (`effective_wick_flag`).

/// Which field on a fold/trade-fold output shape identifies "the same bar,
/// re-emitted" for the reseed last-wins collapse (see
/// `mlc_core::chart_type::merge_last_wins`). Mirrors the per-arm rules
/// hand-written today in `apply_fold_reseed_outputs` /
/// `apply_trade_fold_reseed_outputs` exactly — one row per distinct rule,
/// not per chart_type, so the mapping stays legible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityKey {
    /// No reseed-merge concept — plain kline-render / line/area types never
    /// go through a fold reseed at all.
    None,
    /// `Bar.derived_ts_ms` when non-zero, else `Bar.timestamp`. Used by the
    /// three kline-sufficient counted-bar types (range/volume/dollar) and
    /// the four trade-fed counted-bar types (tick/tick_imbalance/
    /// volume_imbalance/run) — all six share `FoldOutput::Bar` /
    /// `TradeFoldOutput::Bar`'s shape and `fold_on_live_1m`'s upsert rule.
    DerivedTsMsOrTimestamp,
    /// `open_time` field (renko/pnf/kagi bricks-columns-segments; footprint
    /// and tpo bars/sessions).
    OpenTime,
    /// `ts_open` field (three_line_break's own timestamp naming).
    TsOpen,
    /// `ts_ms` field (cvd samples).
    TsMs,
}

/// Whether/how a chart_type's candle-body wicks render — mirrors
/// `effective_wick_flag`'s literal logic today (hardcoded
/// `"range_bar"|"volume_bar"|"dollar_bar"` match).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WickPolicy {
    /// Wicks always follow the global instrument toggle unconditionally
    /// (candle-family render-only types).
    Always,
    /// Wicks never render regardless of the global toggle (non-candle
    /// primitives whose draw fn doesn't consume `wick_enabled` at all —
    /// renko/pnf/kagi/tlb/tpo/footprint/lines/areas).
    Off,
    /// Wicks follow the global instrument toggle AND a per-panel override
    /// default (range_bar/volume_bar/dollar_bar: open/high/low pins on
    /// these are chained-continuity artifacts, not genuine intrabar
    /// excursions, so they default OFF even when the global toggle is on).
    Toggle { default: bool },
}

/// Render-policy facet of a chart_type — today's `effective_wick_flag`
/// literal plus the `tick_intensity` per-panel toggle gate
/// (`ChartWindowRenderInputs::tick_intensity`, currently threaded
/// unconditionally for every chart_type from panel state; this field marks
/// which types actually have a meaningful intensity toggle at all).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderPolicy {
    pub wicks: WickPolicy,
    pub intensity_toggle: bool,
}

impl RenderPolicy {
    /// Per-panel `bar_family_wicks` override's initial value for this
    /// policy — `WickPolicy::Toggle { default }`'s `default`, `false` for
    /// `Always`/`Off` (no toggle concept, the field is inert per
    /// `ChartPanelState::bar_family_wicks`'s doc comment). Used at panel
    /// init (chart-type-engine-consolidation B3, S5) so the field's default
    /// is registry-derived rather than an independently hardcoded `false`.
    pub fn bar_family_wicks_default(self) -> bool {
        match self.wicks {
            WickPolicy::Toggle { default } => default,
            WickPolicy::Always | WickPolicy::Off => false,
        }
    }
}

/// Fixed (non-user-editable) numeric constants baked into a chart_type's
/// fold machine — today hardcoded only inside
/// `chart_out_events::build_subscribe_params` (renko reversal=2, pnf
/// reversal=3, imbalance/run alpha=0.20). Mirrors the `fixed_note` strings
/// already shown in each row's `param_spec` ("Reversal: 2" / "Reversal: 3" /
/// "Alpha: 0.20") — this is the machine-readable counterpart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedConsts {
    /// Renko/pnf reversal-brick count. `None` for every other type.
    pub reversal: Option<u8>,
    /// Tick/volume-imbalance and run-bar EMA alpha, ×100 (20 == 0.20) to
    /// stay integer/Eq/const-friendly. `None` for every other type.
    pub alpha_x100: Option<u16>,
}

impl FixedConsts {
    pub const NONE: FixedConsts = FixedConsts { reversal: None, alpha_x100: None };
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
    /// Static upstream-data classification (hybrid-data-modes-plan-2026-07-08,
    /// Step 5) — resolved against a venue's `TradeHistoryCapabilities` by
    /// `BackendCore::data_mode_for`, never read directly by mlc-core itself.
    pub data_requirement: DataRequirement,
    pub hides_time_grid: bool,
    pub hides_tf_picker: bool,
    /// None = no chart-type-specific TF gate (fall back to ALL_TF_IDS or hides_tf_picker=true).
    /// Some(slice) = explicit whitelist. Only footprint (9 TFs, 5s..1h) and tpo (all-except-tick) use this.
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
    /// Reseed last-wins merge key (K0 contract, B2). See [`IdentityKey`].
    pub identity: IdentityKey,
    /// Wick/intensity render facet (K0 contract, B2). See [`RenderPolicy`].
    pub render_policy: RenderPolicy,
    /// Fixed non-editable fold-machine constants (K0 contract, B2). See
    /// [`FixedConsts`].
    pub fixed_consts: FixedConsts,
    /// Which client-side fold machine (if any) computes this chart_type's
    /// series (K0 contract, B3, S1). See [`MachineKind`].
    pub machine: MachineKind,
}

impl ChartTypeDef {
    /// Single canonical resolver for "what preset option does this
    /// chart_type's numeric parameter default to before the user has picked
    /// anything" — `inline_default` matched against `param_spec.options` by
    /// `value_id`, else the FIRST preset in `param_spec.options`. `None`
    /// when this chart_type has no `param_spec` at all (not a
    /// `Param`-resolution type, or a registry drift) OR when `inline_default`
    /// is set but doesn't literally match any option's `value_id` (a
    /// registry drift — see [`Self::default_param_value`]/
    /// [`Self::default_param_label`] for the raw-`inline_default` fallback
    /// that covers that case without silently substituting the wrong
    /// preset).
    ///
    /// Canon-fix wave 2, item 1: this replaces four independently-maintained
    /// copies of the exact same "`inline_default`-or-`options.first()`" rule
    /// that had drifted apart (`mlc-app::input::chart_out_events::
    /// default_chart_type_param`, `mlc-backend::chart_type::engine::
    /// registry_default_param`, `mlc-app-layout::panel_app`'s inline
    /// `tf_button_label` match arm, `mlc-app-layout::chart_settings::
    /// ChartTypeParamSpec::current_label`'s inline match arm) — every reader
    /// (legend, toolbar, settings modal, subscribe) now calls
    /// [`Self::default_param_value`]/[`Self::default_param_label`] so the
    /// sentinel (`param <= 0.0`) fallback can never diverge across surfaces
    /// again.
    pub fn default_param_option(&self) -> Option<&'static ChartTypeParamOption> {
        let spec = self.param_spec?;
        if let Some(d) = self.inline_default {
            return spec.options.iter().find(|o| o.value_id == d);
        }
        spec.options.first()
    }

    /// Numeric default: `inline_default` matched to an option's `value_id`
    /// when possible ([`Self::default_param_option`]), else `inline_default`
    /// parsed directly (covers a registry row whose `inline_default` isn't
    /// literally one of its own preset `value_id`s — e.g. `"10.0"` vs a
    /// `"10"` option), else the ladder's first preset, else `None`.
    pub fn default_param_value(&self) -> Option<f64> {
        if let Some(opt) = self.default_param_option() {
            return opt.value_id.parse::<f64>().ok();
        }
        self.inline_default
            .and_then(|d| d.parse::<f64>().ok())
            .or_else(|| self.param_spec?.options.first()?.value_id.parse::<f64>().ok())
    }

    /// Label default — same three-tier fallback as
    /// [`Self::default_param_value`], used by the toolbar TF button and the
    /// settings-modal dropdown to print the sentinel-state preset name (e.g.
    /// "8x" instead of "0x").
    pub fn default_param_label(&self) -> Option<&'static str> {
        if let Some(opt) = self.default_param_option() {
            return Some(opt.label);
        }
        self.inline_default.or_else(|| self.param_spec?.options.first().map(|o| o.label))
    }
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

static FOOTPRINT_TF_WHITELIST: &[&str] = &[
    "tf_5s", "tf_15s", "tf_30s",
    "tf_1m", "tf_3m", "tf_5m", "tf_15m", "tf_30m", "tf_1h",
];

// ── param_spec option tables (mirrors chart_settings.rs::chart_type_param_spec) ──

// Price-quantized price-action family (range_bar/renko/pnf/kagi) — owner
// doctrine 2026-07-08 resolution-axes rework: `param` for these four is
// denominated in INSTRUMENT TICKS (not absolute price).
// `build_subscribe_params` (mlc-app/src/input/chart_out_events.rs) converts
// `price_units = ticks * tick_size` at subscribe time. Per-type ladder split
// 2026-07-10: renko/pnf keep this shared box-size-in-ticks ladder (canon for
// both); range_bar and kagi have their OWN ladders below (see their doc
// comments) — box-size-in-ticks is not the right granularity for a range
// bar's threshold or a kagi reversal amount.
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

// Kagi's reversal-amount ladder is its own — a much coarser NT-family
// progression than renko/pnf's box-size ticks (2026-07-10 per-type param
// ladder split). `inline_default` = "50".
static KAGI_OPTIONS: &[ChartTypeParamOption] = &[
    ChartTypeParamOption { value_id: "10", label: "10" },
    ChartTypeParamOption { value_id: "20", label: "20" },
    ChartTypeParamOption { value_id: "50", label: "50" },
    ChartTypeParamOption { value_id: "100", label: "100" },
    ChartTypeParamOption { value_id: "250", label: "250" },
    ChartTypeParamOption { value_id: "500", label: "500" },
];

// Range bar's own NT-family ladder — distinct from renko/pnf's box-size
// ticks (2026-07-10 per-type param ladder split). `inline_default` = "8".
static RANGE_BAR_OPTIONS: &[ChartTypeParamOption] = &[
    ChartTypeParamOption { value_id: "4", label: "4" },
    ChartTypeParamOption { value_id: "8", label: "8" },
    ChartTypeParamOption { value_id: "12", label: "12" },
    ChartTypeParamOption { value_id: "16", label: "16" },
    ChartTypeParamOption { value_id: "24", label: "24" },
    ChartTypeParamOption { value_id: "30", label: "30" },
    ChartTypeParamOption { value_id: "50", label: "50" },
    ChartTypeParamOption { value_id: "100", label: "100" },
];

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
        identity: IdentityKey::None,
        render_policy: RenderPolicy { wicks: WickPolicy::Always, intensity_toggle: false },
        fixed_consts: FixedConsts::NONE,
        machine: MachineKind::None,
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
        identity: IdentityKey::None,
        render_policy: RenderPolicy { wicks: WickPolicy::Always, intensity_toggle: false },
        fixed_consts: FixedConsts::NONE,
        machine: MachineKind::None,
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
        identity: IdentityKey::None,
        render_policy: RenderPolicy { wicks: WickPolicy::Always, intensity_toggle: false },
        fixed_consts: FixedConsts::NONE,
        machine: MachineKind::None,
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
        identity: IdentityKey::None,
        render_policy: RenderPolicy { wicks: WickPolicy::Always, intensity_toggle: false },
        fixed_consts: FixedConsts::NONE,
        machine: MachineKind::None,
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
        identity: IdentityKey::None,
        render_policy: RenderPolicy { wicks: WickPolicy::Off, intensity_toggle: false },
        fixed_consts: FixedConsts::NONE,
        machine: MachineKind::None,
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
        identity: IdentityKey::None,
        render_policy: RenderPolicy { wicks: WickPolicy::Off, intensity_toggle: false },
        fixed_consts: FixedConsts::NONE,
        machine: MachineKind::None,
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
        identity: IdentityKey::None,
        render_policy: RenderPolicy { wicks: WickPolicy::Off, intensity_toggle: false },
        fixed_consts: FixedConsts::NONE,
        machine: MachineKind::None,
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
        identity: IdentityKey::None,
        render_policy: RenderPolicy { wicks: WickPolicy::Off, intensity_toggle: false },
        fixed_consts: FixedConsts::NONE,
        machine: MachineKind::None,
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
        identity: IdentityKey::None,
        render_policy: RenderPolicy { wicks: WickPolicy::Off, intensity_toggle: false },
        fixed_consts: FixedConsts::NONE,
        machine: MachineKind::None,
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
        identity: IdentityKey::None,
        render_policy: RenderPolicy { wicks: WickPolicy::Off, intensity_toggle: false },
        fixed_consts: FixedConsts::NONE,
        machine: MachineKind::None,
    },
    // ── Batched Bars (kline re-cuts) ────────────────────────────────────
    ChartTypeDef {
        id: "range_bar",
        label: "Range Bars",
        icon_id: "RangeBar",
        group: ChartTypeGroup::BatchedBars,
        kind: ChartTypeKind::TradeAggBar,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        // Promoted to kline-sufficient (2026-07-08 minimal-sufficient-source
        // doctrine, dig3 0.3.29): HISTORY seeds from paginated 1m klines via
        // synthetic 4-leg trades, same path renko/pnf/kagi/tlb always used,
        // plus a shallow live trades_recent tail. Deep aggTrades pagination
        // is gone for range_bar.
        data_requirement: DataRequirement::Klines,
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
        inline_default: Some("8"),
        deepen_kind: DeepenKind::DerivedPrependFold,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Param { suffix: "R" },
        identity: IdentityKey::DerivedTsMsOrTimestamp,
        render_policy: RenderPolicy { wicks: WickPolicy::Toggle { default: false }, intensity_toggle: false },
        fixed_consts: FixedConsts::NONE,
        machine: MachineKind::RangeBar,
    },
    ChartTypeDef {
        id: "volume_bar",
        label: "Volume Bars",
        icon_id: "VolumeBar",
        group: ChartTypeGroup::BatchedBars,
        kind: ChartTypeKind::TradeAggBar,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        // Promoted to kline-sufficient (2026-07-08 minimal-sufficient-source
        // doctrine, dig3 0.3.29): HISTORY seeds from paginated 1m klines via
        // synthetic 4-leg trades, same path renko/pnf/kagi/tlb always used,
        // plus a shallow live trades_recent tail. Deep aggTrades pagination
        // is gone for volume_bar.
        data_requirement: DataRequirement::Klines,
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
        inline_default: Some("10"),
        deepen_kind: DeepenKind::DerivedPrependFold,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Param { suffix: "V" },
        identity: IdentityKey::DerivedTsMsOrTimestamp,
        render_policy: RenderPolicy { wicks: WickPolicy::Toggle { default: false }, intensity_toggle: false },
        fixed_consts: FixedConsts::NONE,
        machine: MachineKind::VolumeBar,
    },
    ChartTypeDef {
        id: "dollar_bar",
        label: "Dollar Bars",
        icon_id: "DollarBar",
        group: ChartTypeGroup::BatchedBars,
        kind: ChartTypeKind::TradeAggBar,
        series_variant: SeriesVariant::Bars,
        bars_source: BarsSourceKind::Kline,
        // Promoted to kline-sufficient (2026-07-08 minimal-sufficient-source
        // doctrine, dig3 0.3.29): HISTORY seeds from paginated 1m klines via
        // synthetic 4-leg trades, same path renko/pnf/kagi/tlb always used,
        // plus a shallow live trades_recent tail. Deep aggTrades pagination
        // is gone for dollar_bar.
        data_requirement: DataRequirement::Klines,
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
        identity: IdentityKey::DerivedTsMsOrTimestamp,
        render_policy: RenderPolicy { wicks: WickPolicy::Toggle { default: false }, intensity_toggle: false },
        fixed_consts: FixedConsts::NONE,
        machine: MachineKind::DollarBar,
    },
    // ── Trade Bars (genuinely trade-fed) ────────────────────────────────
    ChartTypeDef {
        id: "tick_bar",
        label: "Tick Bars",
        icon_id: "TickBar",
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
        identity: IdentityKey::DerivedTsMsOrTimestamp,
        render_policy: RenderPolicy { wicks: WickPolicy::Always, intensity_toggle: true },
        fixed_consts: FixedConsts::NONE,
        machine: MachineKind::TickBar,
    },
    ChartTypeDef {
        id: "tick_imbalance_bar",
        label: "Tick Imbalance",
        icon_id: "TickImbalanceBar",
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
        // Unified-data-plane Stage Э4: full refold from the raw trade layer
        // over-writes the EMA state each time (see
        // `digdigdig3_station::folds::TradeToTickImbalanceDerived` — a
        // fresh machine instance replaying the whole trade tape reproduces
        // byte-identical EMA state), so the old "EMA not reconstructible
        // from emitted output" limitation that forced a teardown+respawn
        // deepen under the STATION-side derived pipeline no longer applies
        // to the client-side fold path. Full refold from raw trades IS the
        // reconstruction.
        deepen_kind: DeepenKind::DerivedPrependFold,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Param { suffix: "T" },
        identity: IdentityKey::DerivedTsMsOrTimestamp,
        render_policy: RenderPolicy { wicks: WickPolicy::Always, intensity_toggle: false },
        fixed_consts: FixedConsts { reversal: None, alpha_x100: Some(20) },
        machine: MachineKind::TickImbalance,
    },
    ChartTypeDef {
        id: "volume_imbalance_bar",
        label: "Volume Imbalance",
        icon_id: "VolumeImbalanceBar",
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
        // See "tick_imbalance_bar" row above — Э4 full refold from raw
        // trades supersedes the teardown+respawn requirement.
        deepen_kind: DeepenKind::DerivedPrependFold,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Param { suffix: "V" },
        identity: IdentityKey::DerivedTsMsOrTimestamp,
        render_policy: RenderPolicy { wicks: WickPolicy::Always, intensity_toggle: false },
        fixed_consts: FixedConsts { reversal: None, alpha_x100: Some(20) },
        machine: MachineKind::VolumeImbalance,
    },
    ChartTypeDef {
        id: "run_bar",
        label: "Run Bars",
        icon_id: "RunBar",
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
        // See "tick_imbalance_bar" row above — Э4 full refold from raw
        // trades supersedes the teardown+respawn requirement.
        deepen_kind: DeepenKind::DerivedPrependFold,
        needs_ts_shadow: false,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Param { suffix: "RN" },
        identity: IdentityKey::DerivedTsMsOrTimestamp,
        render_policy: RenderPolicy { wicks: WickPolicy::Always, intensity_toggle: false },
        fixed_consts: FixedConsts { reversal: None, alpha_x100: Some(20) },
        machine: MachineKind::RunBar,
    },
    // ── Price Action ─────────────────────────────────────────────────────
    ChartTypeDef {
        id: "renko",
        label: "Renko",
        icon_id: "Renko",
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
        identity: IdentityKey::OpenTime,
        // `WickPolicy::Toggle` (canon-fix wave 2, item 3) — renko bricks
        // gained a `RenkoBrick::wick` field (intra-brick adverse excursion
        // beyond the body); "Show wicks" mirrors the `bar_family_wicks`
        // panel-toggle plumbing every other `Toggle` chart type already
        // uses (see `active_series::effective_wick_flag`,
        // `chart_settings.rs`'s checkbox row).
        render_policy: RenderPolicy { wicks: WickPolicy::Toggle { default: false }, intensity_toggle: false },
        fixed_consts: FixedConsts { reversal: Some(2), alpha_x100: None },
        machine: MachineKind::Renko,
    },
    ChartTypeDef {
        id: "pnf",
        label: "Point & Figure",
        icon_id: "Pnf",
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
        identity: IdentityKey::OpenTime,
        render_policy: RenderPolicy { wicks: WickPolicy::Off, intensity_toggle: false },
        fixed_consts: FixedConsts { reversal: Some(3), alpha_x100: None },
        machine: MachineKind::Pnf,
    },
    ChartTypeDef {
        id: "kagi",
        label: "Kagi",
        icon_id: "Kagi",
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
        inline_default: Some("50"),
        deepen_kind: DeepenKind::DerivedPrependFold,
        needs_ts_shadow: true,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Param { suffix: "RV" },
        identity: IdentityKey::OpenTime,
        render_policy: RenderPolicy { wicks: WickPolicy::Off, intensity_toggle: false },
        fixed_consts: FixedConsts::NONE,
        machine: MachineKind::Kagi,
    },
    ChartTypeDef {
        id: "three_line_break",
        label: "3 Line Break",
        icon_id: "ThreeLineBreak",
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
        identity: IdentityKey::TsOpen,
        render_policy: RenderPolicy { wicks: WickPolicy::Off, intensity_toggle: false },
        fixed_consts: FixedConsts::NONE,
        machine: MachineKind::ThreeLineBreak,
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
        identity: IdentityKey::OpenTime,
        render_policy: RenderPolicy { wicks: WickPolicy::Off, intensity_toggle: false },
        fixed_consts: FixedConsts::NONE,
        machine: MachineKind::Footprint,
    },
    ChartTypeDef {
        id: "tpo",
        label: "Market Profile (TPO)",
        icon_id: "Tpo",
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
        // One session = one index slot. Chosen wider than footprint's 50px
        // (a footprint cluster is a single bar's cell grid; a TPO block is
        // an entire session's letter grid, needs more room to stay legible)
        // — targets a ~100-140px session block at default zoom (see
        // `chart_render/tpo.rs`'s `render_session` width-fit math).
        min_bar_spacing: Some(120.0),
        default_indicators: &[],
        param_spec: Some(ChartTypeParamSpec {
            section_title: "MARKET PROFILE (TPO) SETTINGS",
            row_label: "Bucket minutes",
            options: TPO_OPTIONS,
            fixed_note: None,
        }),
        inline_default: Some("30"),
        deepen_kind: DeepenKind::DerivedPrependFold,
        // TPO is now a proper index-axis series (canon Sierra TPO chart) —
        // sessions position via `viewport.x_for_bar`/visible_range like
        // renko/pnf/kagi/tlb, so the axis needs the session-ts shadow for
        // time-scale labels + drawings ts→x (`chart_render/tpo.rs`).
        needs_ts_shadow: true,
        has_bespoke_settings_block: false,
        resolution: ResolutionKind::Time,
        identity: IdentityKey::OpenTime,
        render_policy: RenderPolicy { wicks: WickPolicy::Off, intensity_toggle: false },
        fixed_consts: FixedConsts::NONE,
        machine: MachineKind::Tpo,
    },
    // "dom_heatmap" / "liquidation_heatmap" REMOVED (chart-type → overlay
    // migration, mirrors Bookmap/ATAS/Coinglass): both are now builtin
    // overlay indicators (`overlay_dom_heatmap` / `overlay_liquidation_heatmap`
    // in the mli catalog, added/removed via the indicators picker on ANY
    // chart type) instead of their own chart_type registry rows. Draw
    // functions (`draw_dom_heatmap`/`draw_liquidation_heatmap`) and bubble
    // ring-buffer storage are unchanged — only the trigger moved from a
    // chart-type switch to `ChartOutEvent::Toggle{Dom,Liquidation}HeatmapOverlay`.
    // Legacy snapshots with `chart_type = "dom_heatmap"`/`"liquidation_heatmap"`
    // hydrate to `"candles"` (via `by_id(...).unwrap_or("candles")`) with the
    // corresponding overlay auto-enabled — see
    // `mlc-app/src/persistence/hydration_impl.rs`.
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
        ChartTypeGroup::BatchedBars => "Batched Bars",
        ChartTypeGroup::TradeAggregation => "Trade Bars",
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
        assert_eq!(all().len(), 23);
    }

    #[test]
    fn footprint_tf_whitelist_matches_bucket_intervals() {
        let d = by_id("footprint").unwrap();
        assert_eq!(
            d.tf_whitelist,
            Some(&[
                "tf_5s", "tf_15s", "tf_30s",
                "tf_1m", "tf_3m", "tf_5m", "tf_15m", "tf_30m", "tf_1h",
            ][..])
        );
    }

    #[test]
    fn tpo_is_a_shadow_backed_index_series() {
        let d = by_id("tpo").unwrap();
        assert!(d.needs_ts_shadow, "tpo sessions must feed the ts-shadow so axis labels use session dates");
        assert_eq!(d.min_bar_spacing, Some(120.0));
        // Wider than footprint — a session block carries an entire letter
        // grid, footprint a single bar's cell grid.
        let footprint_spacing = by_id("footprint").unwrap().min_bar_spacing.unwrap();
        assert!(d.min_bar_spacing.unwrap() >= footprint_spacing);
    }

    #[test]
    fn all_types_have_a_group() {
        // Placeholder — enum forces this at compile time. Kept for future assertions.
        for d in all() {
            let _ = d.group;
        }
    }

    // ── K0 contract completeness (chart-type-engine-consolidation B2) ──────

    /// `IdentityKey` must exactly match the per-arm rules hand-written today
    /// in `mlc-backend::lib::apply_fold_reseed_outputs` /
    /// `apply_trade_fold_reseed_outputs` (see this module's `IdentityKey`
    /// doc comment for the mapping). `RenderOnly` types never go through a
    /// fold reseed → `IdentityKey::None`; every other kind has a concrete
    /// key.
    #[test]
    fn identity_key_matches_reseed_arms() {
        let expected: &[(&str, IdentityKey)] = &[
            ("candles", IdentityKey::None),
            ("bars", IdentityKey::None),
            ("hollow_candles", IdentityKey::None),
            ("heikin_ashi", IdentityKey::None),
            ("line", IdentityKey::None),
            ("step_line", IdentityKey::None),
            ("line_markers", IdentityKey::None),
            ("area", IdentityKey::None),
            ("hlc_area", IdentityKey::None),
            ("baseline", IdentityKey::None),
            ("range_bar", IdentityKey::DerivedTsMsOrTimestamp),
            ("volume_bar", IdentityKey::DerivedTsMsOrTimestamp),
            ("dollar_bar", IdentityKey::DerivedTsMsOrTimestamp),
            ("tick_bar", IdentityKey::DerivedTsMsOrTimestamp),
            ("tick_imbalance_bar", IdentityKey::DerivedTsMsOrTimestamp),
            ("volume_imbalance_bar", IdentityKey::DerivedTsMsOrTimestamp),
            ("run_bar", IdentityKey::DerivedTsMsOrTimestamp),
            ("renko", IdentityKey::OpenTime),
            ("pnf", IdentityKey::OpenTime),
            ("kagi", IdentityKey::OpenTime),
            ("three_line_break", IdentityKey::TsOpen),
            ("footprint", IdentityKey::OpenTime),
            ("tpo", IdentityKey::OpenTime),
        ];
        assert_eq!(expected.len(), all().len(), "expected table must cover every registry row");
        for (id, key) in expected {
            let d = by_id(id).unwrap_or_else(|| panic!("missing registry row: {id}"));
            assert_eq!(d.identity, *key, "identity mismatch for {id}");
        }
    }

    /// `RenderOnly` types never carry a fold identity (no reseed concept) —
    /// every `TradeAggBar`/`PriceActionTransform`/`OrderflowSlotted` type
    /// does.
    #[test]
    fn render_only_types_have_no_identity() {
        for d in all() {
            if d.kind == ChartTypeKind::RenderOnly {
                assert_eq!(d.identity, IdentityKey::None, "{} is RenderOnly but carries a fold identity", d.id);
            } else {
                assert_ne!(d.identity, IdentityKey::None, "{} is not RenderOnly but has no fold identity", d.id);
            }
        }
    }

    /// `RenderPolicy` must match today's `effective_wick_flag` reality: only
    /// range_bar/volume_bar/dollar_bar/renko are `Toggle` (renko added
    /// canon-fix wave 2, item 3 — `RenkoBrick::wick` is a real intra-brick
    /// adverse excursion, gated by the same `bar_family_wicks` panel toggle
    /// the other three already use); only tick_bar has
    /// `intensity_toggle: true`.
    #[test]
    fn render_policy_matches_effective_wick_flag_reality() {
        for d in all() {
            let is_toggle = matches!(d.render_policy.wicks, WickPolicy::Toggle { .. });
            let expect_toggle = matches!(d.id, "range_bar" | "volume_bar" | "dollar_bar" | "renko");
            assert_eq!(is_toggle, expect_toggle, "wick Toggle policy mismatch for {}", d.id);
            let expect_intensity = d.id == "tick_bar";
            assert_eq!(d.render_policy.intensity_toggle, expect_intensity, "intensity_toggle mismatch for {}", d.id);
        }
    }

    /// `FixedConsts` must exactly mirror each row's `param_spec.fixed_note`
    /// string (renko "Reversal: 2", pnf "Reversal: 3",
    /// tick_imbalance/volume_imbalance/run "Alpha: 0.20") — the two are
    /// meant to describe the SAME constant, one human-readable, one
    /// machine-readable.
    #[test]
    fn fixed_consts_matches_param_spec_fixed_note() {
        let renko = by_id("renko").unwrap();
        assert_eq!(renko.fixed_consts.reversal, Some(2));
        assert_eq!(renko.param_spec.unwrap().fixed_note, Some("Reversal: 2"));

        let pnf = by_id("pnf").unwrap();
        assert_eq!(pnf.fixed_consts.reversal, Some(3));
        assert_eq!(pnf.param_spec.unwrap().fixed_note, Some("Reversal: 3"));

        for id in ["tick_imbalance_bar", "volume_imbalance_bar", "run_bar"] {
            let d = by_id(id).unwrap();
            assert_eq!(d.fixed_consts.alpha_x100, Some(20), "{id} alpha mismatch");
            assert_eq!(d.param_spec.unwrap().fixed_note, Some("Alpha: 0.20"), "{id} fixed_note mismatch");
        }

        // Every other row carries no fixed consts.
        for d in all() {
            if matches!(d.id, "renko" | "pnf" | "tick_imbalance_bar" | "volume_imbalance_bar" | "run_bar") {
                continue;
            }
            assert_eq!(d.fixed_consts, FixedConsts::NONE, "{} should have no fixed consts", d.id);
        }
    }

    /// `MachineKind` must exactly match `IdentityKey::None`-ness (B3, S1) —
    /// every `RenderOnly` type has no fold machine and vice versa. Also pins
    /// the exact id → variant mapping so a future registry row can't drift
    /// silently from `view_fold.rs`'s single `MachineKind` match.
    #[test]
    fn machine_kind_matches_identity_and_is_pinned() {
        let expected: &[(&str, MachineKind)] = &[
            ("candles", MachineKind::None),
            ("bars", MachineKind::None),
            ("hollow_candles", MachineKind::None),
            ("heikin_ashi", MachineKind::None),
            ("line", MachineKind::None),
            ("step_line", MachineKind::None),
            ("line_markers", MachineKind::None),
            ("area", MachineKind::None),
            ("hlc_area", MachineKind::None),
            ("baseline", MachineKind::None),
            ("range_bar", MachineKind::RangeBar),
            ("volume_bar", MachineKind::VolumeBar),
            ("dollar_bar", MachineKind::DollarBar),
            ("tick_bar", MachineKind::TickBar),
            ("tick_imbalance_bar", MachineKind::TickImbalance),
            ("volume_imbalance_bar", MachineKind::VolumeImbalance),
            ("run_bar", MachineKind::RunBar),
            ("renko", MachineKind::Renko),
            ("pnf", MachineKind::Pnf),
            ("kagi", MachineKind::Kagi),
            ("three_line_break", MachineKind::ThreeLineBreak),
            ("footprint", MachineKind::Footprint),
            ("tpo", MachineKind::Tpo),
        ];
        assert_eq!(expected.len(), all().len(), "expected table must cover every registry row");
        for (id, machine) in expected {
            let d = by_id(id).unwrap_or_else(|| panic!("missing registry row: {id}"));
            assert_eq!(d.machine, *machine, "machine mismatch for {id}");
            assert_eq!(
                d.machine == MachineKind::None,
                d.identity == IdentityKey::None,
                "{id}: machine/identity None-ness disagree",
            );
        }
    }

    /// `RenderPolicy::bar_family_wicks_default` (B3, S5) must return the
    /// `Toggle { default }` value for the 4 bar-family types (renko added
    /// canon-fix wave 2, item 3), and `false` for every other row (no
    /// toggle concept).
    #[test]
    fn bar_family_wicks_default_matches_wick_policy() {
        for d in all() {
            let expected = match d.render_policy.wicks {
                WickPolicy::Toggle { default } => default,
                WickPolicy::Always | WickPolicy::Off => false,
            };
            assert_eq!(
                d.render_policy.bar_family_wicks_default(), expected,
                "{} bar_family_wicks_default mismatch", d.id,
            );
        }
        // Today's 4 bar-family rows all default OFF.
        for id in ["range_bar", "volume_bar", "dollar_bar", "renko"] {
            let d = by_id(id).unwrap();
            assert!(!d.render_policy.bar_family_wicks_default(), "{id} should default OFF");
        }
    }
}
